use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const SCRIPT: &str = include_str!("../../../tests/qa-002/scenarios.json");
const SESSION: &str = "532f0694-f5ee-4299-b05e-103b3c3c7481";

fn cli(request: &Value, state: &Path) -> Value {
    let mut process = Command::new(env!("CARGO_BIN_EXE_kronello"))
        .env("KRONELLO_STATE_ROOT", state)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    process
        .stdin
        .take()
        .unwrap()
        .write_all(request.to_string().as_bytes())
        .unwrap();
    let output = process.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "success", "{response}");
    response["result"]["value"].clone()
}

struct Mcp {
    process: Child,
    input: Option<ChildStdin>,
    responses: Receiver<String>,
    reader: Option<std::thread::JoinHandle<()>>,
    id: u64,
}
impl Mcp {
    fn start(state: &Path) -> Self {
        let binary = std::env::var_os("KRONELLO_QA_MCP_BINARY")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_BIN_EXE_kronello"))
                    .with_file_name(format!("kronello-mcp{}", std::env::consts::EXE_SUFFIX))
            });
        assert!(
            binary.is_file(),
            "Build the real MCP binary first: cargo build -p kronello-mcp --locked ({})",
            binary.display()
        );
        let mut process = Command::new(binary)
            .env("KRONELLO_STATE_ROOT", state)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let input = process.stdin.take();
        let output = process.stdout.take().unwrap();
        let (sender, responses) = mpsc::channel();
        let reader = Some(std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                if sender.send(line.unwrap()).is_err() {
                    break;
                }
            }
        }));
        let mut client = Self {
            process,
            input,
            responses,
            reader,
            id: 0,
        };
        let initialized = client.rpc("initialize", json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"qa-002","version":"1"}}));
        assert_eq!(initialized["protocolVersion"], "2025-11-25");
        client.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
        client
    }
    fn send(&mut self, message: Value) {
        writeln!(self.input.as_mut().unwrap(), "{message}").unwrap();
        self.input.as_mut().unwrap().flush().unwrap();
    }
    fn rpc(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        self.send(json!({"jsonrpc":"2.0","id":self.id,"method":method,"params":params}));
        let line = self
            .responses
            .recv_timeout(Duration::from_secs(30))
            .expect("QA MCP response timeout");
        let response: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["id"], self.id);
        assert_eq!(response["jsonrpc"], "2.0");
        assert!(response.get("error").is_none(), "{response}");
        response["result"].clone()
    }
    fn call(&mut self, request: &Value) -> Value {
        let mut arguments = request.clone();
        let operation = arguments
            .as_object_mut()
            .unwrap()
            .remove("operation")
            .unwrap();
        let result = self.rpc(
            "tools/call",
            json!({"name":operation,"arguments":arguments}),
        );
        assert_ne!(result["isError"], true, "{result}");
        let text: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(text, result["structuredContent"]);
        result["structuredContent"].clone()
    }
    fn finish(&mut self) {
        self.input.take();
        for _ in 0..100 {
            if let Some(status) = self.process.try_wait().unwrap() {
                assert!(status.success());
                self.reader.take().unwrap().join().unwrap();
                assert!(
                    self.responses.try_recv().is_err(),
                    "Unexpected MCP stdout after EOF"
                );
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("MCP did not exit after EOF");
    }
}
impl Drop for Mcp {
    fn drop(&mut self) {
        self.input.take();
        let _ = self.process.kill();
        let _ = self.process.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

// Canonical object order; preserve arrays and every string's exact Unicode bytes.
// Known finite numeric fields normalize 1 and 1.0 across Foundation/Serde encoders.
fn canonical(value: &Value) -> Value {
    match value {
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, v)| (key.clone(), canonical(v)))
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.iter().map(canonical).collect()),
        Value::Number(number) => json!(number.as_f64().unwrap()),
        _ => value.clone(),
    }
}
fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(&canonical(value)).unwrap()
}
fn snapshot(mut call: impl FnMut(&Value) -> Value, path: &Path) -> Value {
    let export = call(&json!({"operation":"project.export","project":path}));
    let history =
        call(&json!({"operation":"history.list","project":path,"since_revision":"0","limit":1000}));
    json!({"document":export["document"],"revision":export["revision"],"event_count":history["events"].as_array().unwrap().len()})
}

fn run(
    script: &Value,
    identities: Option<&Value>,
    path: &Path,
    mut call: impl FnMut(&Value) -> Value,
) -> Value {
    call(&json!({"operation":"project.create","project":path,"document":script["document"]}));
    let baseline = snapshot(&mut call, path);
    let mut steps = Vec::new();
    let mut undo = Vec::new();
    let mut redo = Vec::new();
    for (index, step) in script["steps"].as_array().unwrap().iter().enumerate() {
        let revision = (index + 1).to_string();
        let default_key = format!("6bac3597-e2ad-4290-89a3-{:012}", index + 1);
        let client = identities
            .map(|gui| gui["steps"][index]["client"].clone())
            .unwrap_or_else(|| json!({"session_id":SESSION,"idempotency_key":default_key}));
        let session = client["session_id"]
            .as_str()
            .expect("GUI session ID must be captured");
        let key = client["idempotency_key"]
            .as_str()
            .expect("GUI idempotency key must be captured");
        let action = step["action"].as_str().unwrap();
        let event = if matches!(action, "undo" | "redo") {
            let target = if action == "undo" {
                undo.pop().unwrap()
            } else {
                redo.pop().unwrap()
            };
            let inverse = call(
                &json!({"operation":"edit.undo","project":path,"base_revision":revision,"event_id":target,"session_id":session,"idempotency_key":key}),
            );
            if action == "undo" {
                redo.push(inverse["id"].clone());
            } else {
                undo.push(inverse["id"].clone());
            }
            inverse
        } else {
            assert!(
                step["commands"].is_array(),
                "Every non-Undo scenario must pin commands"
            );
            let plan = call(
                &json!({"operation":"edit.plan","project":path,"base_revision":revision,"commands":step["commands"]}),
            );
            let event = call(
                &json!({"operation":"edit.apply","project":path,"base_revision":revision,"commands":step["commands"],"plan_hash":plan["plan_hash"],"session_id":session,"idempotency_key":key}),
            );
            undo.push(event["id"].clone());
            redo.clear();
            event
        };
        assert_eq!(event["revision"].as_u64(), Some((index + 2) as u64));
        let mut state = snapshot(&mut call, path);
        assert_eq!(state["revision"], (index + 2).to_string());
        assert_eq!(state["event_count"], index + 2); // Creation is one Event.
        state["name"] = step["name"].clone();
        state["action"] = step["action"].clone();
        state["client"] = client;
        state["canonical_document_sha256"] =
            json!(format!("{:x}", Sha256::digest(bytes(&state["document"]))));
        steps.push(state);
    }
    json!({"baseline":baseline,"steps":steps})
}

#[test]
fn scripted_cli_mcp_and_optional_gui_snapshots_match() {
    let script: Value = serde_json::from_str(SCRIPT).unwrap();
    let evidence = std::env::var_os("KRONELLO_QA_EVIDENCE_DIR").map(PathBuf::from);
    let gui: Option<Value> = evidence.as_ref().map(|directory| {
        serde_json::from_slice(
            &std::fs::read(directory.join("gui.json")).expect("Run direct Swift QA checks first"),
        )
        .unwrap()
    });
    if let Some(gui) = &gui {
        assert_eq!(
            bytes(&gui["fixture"]),
            bytes(&script),
            "GUI evidence used a different scenario fixture"
        );
    }
    let folder = tempfile::tempdir().unwrap();
    let cli_result = run(
        &script,
        gui.as_ref(),
        &folder.path().join("cli.kronello"),
        |request| cli(request, folder.path()),
    );
    let mut mcp = Mcp::start(folder.path());
    let mcp_result = run(
        &script,
        gui.as_ref(),
        &folder.path().join("mcp.kronello"),
        |request| mcp.call(request),
    );
    mcp.finish();
    assert_eq!(
        bytes(&cli_result),
        bytes(&mcp_result),
        "Independent CLI/MCP edits diverged"
    );
    let mut gui_compared = false;
    if let Some(gui) = &gui {
        assert_eq!(bytes(&gui["baseline"]), bytes(&cli_result["baseline"]));
        let gui_steps = gui["steps"].as_array().unwrap();
        let cli_steps = cli_result["steps"].as_array().unwrap();
        assert_eq!(gui_steps.len(), cli_steps.len());
        for (gui, cli) in gui_steps.iter().zip(cli_steps) {
            for field in [
                "name",
                "action",
                "client",
                "document",
                "revision",
                "event_count",
            ] {
                assert_eq!(
                    bytes(&gui[field]),
                    bytes(&cli[field]),
                    "GUI/CLI mismatch: {} / {field}",
                    cli["name"]
                );
            }
        }
        gui_compared = true;
    }
    let report = json!({"format":1,"fixture":script,"independent_projects":true,"gui_compared":gui_compared,"cli":cli_result,"mcp":mcp_result});
    if let Some(directory) = evidence {
        std::fs::write(
            directory.join("cli-mcp.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
    }
    println!(
        "QA-002: {} independent CLI/MCP scenarios, GUI compared: {gui_compared}",
        report["fixture"]["steps"].as_array().unwrap().len()
    );
}
