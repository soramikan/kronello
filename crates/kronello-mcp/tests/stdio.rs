use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use kronello_mcp::SUPPORTED_PROTOCOL_VERSIONS;
use serde_json::{Value, json};

struct Client {
    child: Child,
    input: Option<ChildStdin>,
    lines: Receiver<String>,
    reader: std::thread::JoinHandle<()>,
    next_id: u64,
}
impl Client {
    fn spawn(args: &[&str], inject_adapter_failure: bool) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_kronello-mcp"));
        command
            .args(args)
            .env_remove("KRONELLO_TEST_ADAPTER_UNAVAILABLE");
        if inject_adapter_failure {
            command.env("KRONELLO_TEST_ADAPTER_UNAVAILABLE", "1");
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take();
        let stdout = child.stdout.take().unwrap();
        let (sender, lines) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if sender.send(line.unwrap()).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            input,
            lines,
            reader,
            next_id: 1,
        }
    }
    fn send(&mut self, value: &Value) {
        self.send_raw(&value.to_string());
    }
    fn send_raw(&mut self, line: &str) {
        let input = self.input.as_mut().unwrap();
        input.write_all(line.as_bytes()).unwrap();
        input.write_all(b"\n").unwrap();
        input.flush().unwrap();
    }
    fn receive(&self) -> Value {
        let line = self
            .lines
            .recv_timeout(Duration::from_secs(20))
            .expect("MCP response timeout");
        let value: Value = serde_json::from_str(&line).expect(&line);
        assert_eq!(value["jsonrpc"], "2.0");
        assert!(
            value.get("result").is_some() ^ value.get("error").is_some(),
            "{value}"
        );
        value
    }
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}));
        let response = self.receive();
        assert_eq!(response["id"], id);
        response
    }
    fn initialize(&mut self, version: &str) -> Value {
        self.request(
            "initialize",
            json!({"protocolVersion":version,"capabilities":{},
            "clientInfo":{"name":"test-client","version":"1"}}),
        )
    }
    fn ready(&mut self, version: &str) {
        let response = self.initialize(version);
        assert_eq!(response["result"]["protocolVersion"], version);
        assert_eq!(
            response["result"]["capabilities"],
            json!({"tools":{"listChanged":false}})
        );
        self.send(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    }
    fn call(&mut self, name: &str, arguments: Value) -> Value {
        let response = self.request("tools/call", json!({"name":name,"arguments":arguments}));
        assert!(response.get("error").is_none(), "{response}");
        let result = response["result"].clone();
        assert_eq!(result["content"].as_array().unwrap().len(), 1);
        assert_eq!(result["content"][0]["type"], "text");
        let text: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(text, result["structuredContent"]);
        result
    }
    fn schemas(&mut self) -> BTreeMap<String, Value> {
        let response = self.request("tools/list", json!({}));
        assert!(response.get("error").is_none(), "{response}");
        response["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| (tool["name"].as_str().unwrap().to_owned(), tool.clone()))
            .collect()
    }
    fn finish(mut self) -> String {
        drop(self.input.take());
        let output = self.child.wait_with_output().unwrap();
        self.reader.join().unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(
            self.lines.try_iter().next().is_none(),
            "unsolicited stdout output"
        );
        String::from_utf8(output.stderr).unwrap()
    }
}
fn validate(tool: &Value, result: &Value) {
    jsonschema::validator_for(&tool["outputSchema"])
        .unwrap()
        .validate(&result["structuredContent"])
        .unwrap();
}
fn assert_error(result: &Value, code: &str) {
    assert_eq!(result["isError"], true, "{result}");
    assert_eq!(result["structuredContent"]["status"], "error");
    assert_eq!(result["structuredContent"]["error"]["code"], code);
}
fn document() -> Value {
    let mut document: Value =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    document["compositions"][0]["nodes"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    document["compositions"][0]["root_nodes"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    document["texts"] = json!([]);
    let request: kronello_service::Request = serde_json::from_str(
        &json!({"operation":"project.create","project":"unused.kronello","document":document})
            .to_string(),
    )
    .unwrap();
    serde_json::to_value(request).unwrap()["document"].take()
}
fn render_input(path: &std::path::Path, document: &Value) -> Value {
    json!({"project":path,"composition":document["compositions"][0]["id"],
        "region":{"origin":[0.0,0.0],"extent":[64.0,32.0],"pixels":[8,4]}})
}

#[test]
fn versions_negotiate_and_registry_schemas_are_self_contained() {
    let api = kronello_service::api_json_schema();
    let committed: Value =
        serde_json::from_str(include_str!("../../../schemas/api-v1.schema.json")).unwrap();
    assert_eq!(api, committed);
    for version in SUPPORTED_PROTOCOL_VERSIONS {
        let mut client = Client::spawn(&[], false);
        assert_eq!(
            client.request("tools/list", json!({}))["error"]["code"],
            -32002
        );
        let unsupported = client.initialize("1900-01-01");
        assert_eq!(unsupported["error"]["code"], -32602);
        assert_eq!(
            unsupported["error"]["data"]["supported"],
            json!(SUPPORTED_PROTOCOL_VERSIONS)
        );
        assert_eq!(unsupported["error"]["data"]["requested"], "1900-01-01");
        let initialized = client.initialize(version);
        assert_eq!(initialized["result"]["protocolVersion"], version);
        assert_eq!(
            client.request("tools/list", json!({}))["error"]["code"],
            -32002
        );
        client.send(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
        assert_eq!(client.initialize(version)["error"]["code"], -32602);
        let schemas = client.schemas();
        assert_eq!(
            schemas.keys().cloned().collect::<BTreeSet<_>>(),
            kronello_service::command_registry()
                .into_iter()
                .map(|c| c.name)
                .collect()
        );
        assert!(!schemas.contains_key("project.open"));
        for command in kronello_service::command_registry() {
            let tool = &schemas[&command.name];
            assert_eq!(tool["inputSchema"]["type"], "object");
            assert_eq!(tool["outputSchema"]["type"], "object");
            jsonschema::validator_for(&tool["inputSchema"]).unwrap();
            jsonschema::validator_for(&tool["outputSchema"]).unwrap();
            let mut input = tool["inputSchema"].clone();
            input.as_object_mut().unwrap().remove("$schema");
            input.as_object_mut().unwrap().remove("$defs");
            assert_eq!(
                &input,
                api.pointer(command.request_schema.split_once('#').unwrap().1)
                    .unwrap()
            );
            assert_eq!(
                tool["outputSchema"]["anyOf"][0]["$ref"],
                format!("#{}", command.response_schema.split_once('#').unwrap().1)
            );
            assert_eq!(
                tool["_meta"]["kronello"]["readOnlyProject"],
                command.read_only
            );
            if command.name != "capabilities.get" {
                let missing = client.call(&command.name, json!({}));
                assert_error(&missing, "INVALID_REQUEST");
                validate(tool, &missing);
            }
        }
        let capabilities = client.call("capabilities.get", json!({}));
        assert_eq!(capabilities["isError"], false);
        validate(&schemas["capabilities.get"], &capabilities);
        assert_eq!(
            capabilities["structuredContent"]["commands"],
            serde_json::to_value(kronello_service::command_registry()).unwrap()
        );
        assert_eq!(
            client.request("tools/list", json!({"cursor":"unknown"}))["error"]["code"],
            -32602
        );
        assert_eq!(
            client.request("tools/call", json!({"name":"project.open","arguments":{}}))["error"]["code"],
            -32602
        );
        assert_eq!(
            client.request("unknown", json!({}))["error"]["code"],
            -32601
        );
        assert!(client.finish().contains("INVALID_REQUEST"));
    }
}

#[test]
fn public_api_fixture_commands_return_schema_valid_success_from_real_binary() {
    for version in SUPPORTED_PROTOCOL_VERSIONS {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("contracts.kronello");
        let document = document();
        let composition = &document["compositions"][0]["id"];
        let node = &document["compositions"][0]["nodes"][0]["id"];
        let property = &document["compositions"][0]["nodes"][0]["properties"][1]["id"];
        let session = "fedcba98-7654-4321-8abc-123456789abc";
        let mut client = Client::spawn(&["--backend", "cpu-reference"], false);
        client.ready(version);
        let schemas = client.schemas();
        let mut checked = BTreeSet::new();
        let mut execute = |name: &str, arguments: Value| {
            jsonschema::validator_for(&schemas[name]["inputSchema"])
                .unwrap()
                .validate(&arguments)
                .unwrap();
            if name != "capabilities.get" {
                let mut missing_project = arguments.clone();
                let target = if name.starts_with("render.") {
                    &mut missing_project["input"]
                } else {
                    &mut missing_project
                };
                assert!(target.as_object_mut().unwrap().remove("project").is_some());
                let missing = client.call(name, missing_project);
                assert_error(&missing, "INVALID_REQUEST");
                validate(&schemas[name], &missing);
            }
            let result = client.call(name, arguments);
            assert_eq!(result["isError"], false, "{name}: {result}");
            validate(&schemas[name], &result);
            checked.insert(name.to_owned());
            result["structuredContent"].clone()
        };
        execute(
            "project.create",
            json!({"project":path,"document":document}),
        );
        execute(
            "project.import",
            json!({"project":path,"base_revision":"1","document":document}),
        );
        execute("project.info", json!({"project":path}));
        execute("project.export", json!({"project":path}));
        let commands = json!([{"property_source_set":{"object":node,"property":property,
        "source":{"kind":"constant","value":{"kind":"scalar","value":2.0}}}}]);
        let plan = execute(
            "edit.plan",
            json!({"project":path,"base_revision":"2","commands":commands}),
        );
        let event = execute(
            "edit.apply",
            json!({"project":path,"base_revision":"2","commands":commands,
        "plan_hash":plan["plan_hash"],"session_id":session,"idempotency_key":"apply"}),
        );
        execute(
            "edit.undo",
            json!({"project":path,"base_revision":"3","event_id":event["id"],
        "session_id":session,"idempotency_key":"undo"}),
        );
        execute("history.list", json!({"project":path}));
        execute(
            "scene.query",
            json!({"project":path,"composition":composition}),
        );
        execute(
            "property.sample",
            json!({"project":path,"composition":composition,
        "keys":[{"kind":"node","instance_path":[],"node":node,"property":property}],
        "times":[{"num":"1","den":"2"}]}),
        );
        execute("capabilities.get", json!({}));
        let input = render_input(&path, &document);
        let frame = execute(
            "render.frame",
            json!({"input":input,"time":{"num":"0","den":"1"}}),
        );
        assert_eq!(frame["metadata"]["backend"], "cpu_reference_float32");
        execute(
            "render.sequence",
            json!({"input":input,
        "range":{"start":{"num":"0","den":"1"},"end":{"num":"1","den":"1"}},
        "frame_rate":{"num":"1","den":"1"},"output_directory":dir.path().join("frames")}),
        );
        assert!(dir.path().join("frames/sequence.json").exists());
        let template_document: Value =
            serde_json::from_str(include_str!("../../../examples/template-001.project.json"))
                .unwrap();
        let definition: Value = serde_json::from_str(include_str!(
            "../../../examples/template-001.definition.json"
        ))
        .unwrap();
        let template_path = dir.path().join("template.kronello");
        execute(
            "project.create",
            json!({"project":template_path,"document":template_document}),
        );
        execute(
            "template.define",
            json!({"project":template_path,"base_revision":"1","session_id":session,
        "idempotency_key":"define","definition":definition}),
        );
        let instance = "abcddcba-7654-4321-8abc-123456789abc";
        execute(
            "template.instantiate",
            json!({"project":template_path,"base_revision":"2","session_id":session,
        "idempotency_key":"place","composition":template_document["compositions"][0]["id"],
        "node":"abcddcba-1234-4321-8abc-123456789abc","index":0,
        "instance":{"id":instance,"definition_ref":definition["id"],"version":"1.0.0",
            "duration":{"num":"5","den":"1"},"inputs":{}}}),
        );
        execute(
            "template.set_input",
            json!({"project":template_path,"base_revision":"3","session_id":session,
        "idempotency_key":"input","instance":instance,"name":"headline","value":{"kind":"string","value":"日本語"}}),
        );
        execute(
            "template.set_duration",
            json!({"project":template_path,"base_revision":"4","session_id":session,
        "idempotency_key":"duration","instance":instance,"duration":{"num":"8","den":"1"}}),
        );
        // Success fixtures cover the 17 commands at MCP-001 implementation time.
        // New registry entries are discovered/validated generically in the version
        // test and do not require a transport-specific command list here.
        assert_eq!(checked.len(), 17);
        assert!(client.finish().contains("INVALID_REQUEST"));
    }
}

#[test]
fn interleaved_projects_require_explicit_targets_and_preserve_material_data() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.kronello");
    let b = dir.path().join("b.kronello");
    let mut document_a = document();
    let mut document_b = document();
    document_a["name"] = json!(format!(
        "A: $(touch {}) shell ffmpeg_args https://invalid.test",
        dir.path().join("never").display()
    ));
    document_b["name"] = json!("B");
    document_b["id"] = json!("fedcba98-1234-4321-8abc-123456789abc");
    let mut client = Client::spawn(&[], false);
    client.ready("2025-06-18");
    let schemas = client.schemas();
    assert_eq!(
        client.call("project.create", json!({"project":a,"document":document_a}))["isError"],
        false
    );
    assert_eq!(
        client.call("project.create", json!({"project":b,"document":document_b}))["isError"],
        false
    );
    for target in [&a, &b, &a, &b] {
        let result = client.call("project.info", json!({"project":target}));
        validate(&schemas["project.info"], &result);
        let expected = if target == &a {
            &document_a
        } else {
            &document_b
        };
        assert_eq!(result["structuredContent"]["project_id"], expected["id"]);
        assert_eq!(result["structuredContent"]["name"], expected["name"]);
    }
    let missing = client.call("project.info", json!({}));
    assert_error(&missing, "INVALID_REQUEST");
    validate(&schemas["project.info"], &missing);
    document_b["name"] = json!("B updated");
    assert_eq!(
        client.call(
            "project.import",
            json!({"project":b,"base_revision":"1","document":document_b})
        )["isError"],
        false
    );
    let conflict = client.call(
        "project.import",
        json!({"project":b,"base_revision":"1","document":document_b}),
    );
    assert_error(&conflict, "REVISION_CONFLICT");
    validate(&schemas["project.import"], &conflict);
    for (target, document, revision) in [(&a, &document_a, "1"), (&b, &document_b, "2")] {
        let result = client.call("project.export", json!({"project":target}));
        assert_eq!(result["structuredContent"]["document"], *document);
        assert_eq!(result["structuredContent"]["revision"], revision);
    }
    assert!(!dir.path().join("never").exists());
    assert!(client.finish().contains("REVISION_CONFLICT"));
}

#[test]
fn malformed_messages_duplicates_execution_fields_and_notifications_are_safe() {
    let dir = tempfile::tempdir().unwrap();
    let mut client = Client::spawn(&[], false);
    client.send_raw("{broken");
    assert_eq!(client.receive()["error"]["code"], -32700);
    client.send_raw("[]");
    assert_eq!(client.receive()["error"]["code"], -32600);
    client.send_raw(r#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#);
    assert_eq!(client.receive()["error"]["code"], -32600);
    client.send_raw(r#"{"jsonrpc":"2.0","jsonrpc":"2.0","id":123,"method":"ping"}"#);
    assert_eq!(client.receive()["error"]["code"], -32600);
    client.ready("2025-11-25");
    let schemas = client.schemas();
    client.send_raw(r#"{"jsonrpc":"2.0","id":"duplicate","method":"tools/call","params":{"name":"project.info","arguments":{"project":"a.kronello","project":"b.kronello"}}}"#);
    let duplicate = client.receive();
    assert_eq!(duplicate["id"], "duplicate");
    assert_error(&duplicate["result"], "INVALID_REQUEST");
    validate(&schemas["project.info"], &duplicate["result"]);
    for arguments in [
        json!({"project":"https://invalid.test/file.kronello"}),
        json!({"project":"a.kronello","shell":"touch never"}),
        json!({"project":"a.kronello","url":"https://invalid.test"}),
        json!({"project":"a.kronello","ffmpeg_args":["-i","file"]}),
        json!({"operation":"project.export","project":"a.kronello"}),
        json!([]),
    ] {
        let invalid = client.call("project.info", arguments);
        assert_error(&invalid, "INVALID_REQUEST");
        validate(&schemas["project.info"], &invalid);
    }
    let path = dir.path().join("notification.kronello");
    client.send(
        &json!({"jsonrpc":"2.0","method":"tools/call","params":{"name":"project.create",
        "arguments":{"project":path,"document":document()}}}),
    );
    client.send(
        &json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":99}}),
    );
    assert_eq!(client.request("ping", json!({}))["result"], json!({}));
    assert!(!path.exists());
    let not_found = client.call("project.info", json!({"project":path}));
    assert_error(&not_found, "PROJECT_NOT_FOUND");
    validate(&schemas["project.info"], &not_found);
    let null_arguments = client.call("capabilities.get", Value::Null);
    assert_error(&null_arguments, "INVALID_REQUEST");
    validate(&schemas["capabilities.get"], &null_arguments);
    assert_eq!(
        client.request("tools/list", Value::Null)["error"]["code"],
        -32602
    );
    let omitted_arguments = client.request("tools/call", json!({"name":"capabilities.get"}));
    assert_eq!(omitted_arguments["result"]["isError"], false);
    validate(&schemas["capabilities.get"], &omitted_arguments["result"]);
    let malformed = client.request(
        "tools/call",
        json!({"name":"project.info","name_extra":"other"}),
    );
    assert_eq!(malformed["error"]["code"], -32602);
    assert!(client.finish().contains("PROJECT_NOT_FOUND"));
}

#[test]
fn default_gpu_failure_never_falls_back_and_explicit_cpu_selection_works() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("backend.kronello");
    let document = document();
    let mut client = Client::spawn(&[], true);
    client.ready("2025-11-25");
    let schemas = client.schemas();
    assert_eq!(
        client.call(
            "project.create",
            json!({"project":path,"document":document})
        )["isError"],
        false
    );
    let output = dir.path().join("must-not-exist");
    let arguments = json!({"input":render_input(&path, &document),
        "range":{"start":{"num":"0","den":"1"},"end":{"num":"1","den":"1"}},
        "frame_rate":{"num":"1","den":"1"},"output_directory":output});
    let failed = client.call("render.sequence", arguments.clone());
    assert_error(&failed, "ADAPTER_UNAVAILABLE");
    validate(&schemas["render.sequence"], &failed);
    assert!(!output.exists());
    assert!(client.finish().contains("ADAPTER_UNAVAILABLE"));
    let mut cpu = Client::spawn(&["--backend", "cpu-reference"], true);
    cpu.ready("2025-06-18");
    let success = cpu.call("render.sequence", arguments);
    assert_eq!(success["isError"], false);
    validate(&schemas["render.sequence"], &success);
    assert!(output.join("sequence.json").exists());
    assert_eq!(cpu.finish(), "");
}

#[test]
fn oversized_message_is_bounded_and_next_request_still_works() {
    let mut client = Client::spawn(&[], false);
    client.send_raw(&" ".repeat(kronello_mcp::MAX_MESSAGE_BYTES + 64));
    let response = client.receive();
    assert_eq!(response["id"], Value::Null);
    assert_eq!(response["error"]["code"], -32600);
    assert_eq!(client.request("ping", json!({}))["result"], json!({}));
    assert_eq!(client.finish(), "");
}

#[test]
fn startup_options_keep_stdout_protocol_only() {
    for args in [
        &["--help"][..],
        &["--backend", "automatic"][..],
        &["--shell"][..],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_kronello-mcp"))
            .args(args)
            .output()
            .unwrap();
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
        assert_eq!(output.status.success(), args == ["--help"]);
    }
}
