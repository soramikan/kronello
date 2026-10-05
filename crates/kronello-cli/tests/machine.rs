use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use kronello_service::{Request, Response, Service};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn document() -> Value {
    serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap()
}

#[test]
fn explain_subcommands_and_tagged_requests_share_read_only_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("inspect.kronello");
    let doc: Value =
        serde_json::from_str(include_str!("../../../examples/ffi-preview.project.json")).unwrap();
    call(
        &["project", "create"],
        json!({"project":path,"document":doc}),
        true,
    );
    let node = json!({"project":path,"composition":doc["compositions"][0]["id"],"key":{"instance_path":[],"node":doc["compositions"][0]["nodes"][0]["id"]},"time":{"num":"0","den":"1"}});
    let via_subcommand = call(&["node", "explain"], node.clone(), true);
    assert_eq!(via_subcommand["result"]["kind"], "node_explanation");
    let mut tagged = node;
    tagged["operation"] = json!("node.explain");
    assert_eq!(call(&[], tagged, true), via_subcommand);
    let render = json!({"input":{"project":path,"composition":doc["compositions"][0]["id"],"region":{"origin":[0,0],"extent":[64,32],"pixels":[8,4]}},"time":{"num":"0","den":"1"}});
    let (result, _) = invoke_with_env(
        &["render", "explain"],
        &render.to_string(),
        true,
        Some(("KRONELLO_TEST_ADAPTER_UNAVAILABLE", "1")),
    );
    assert_eq!(result["result"]["value"]["plan"]["executed"], false);
    assert_eq!(result["result"]["value"]["plan"]["backend"], "gpu");
    assert_eq!(
        call(
            &["--backend", "cpu-reference", "render", "explain"],
            render,
            true
        )["result"]["value"]["plan"]["backend"],
        "cpu_reference"
    );
    assert_eq!(
        call(&["project", "info"], json!({"project":path}), true)["result"]["value"]["revision"],
        "1"
    );
}

#[test]
fn expression_commands_and_samples_use_the_shared_cli_api() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("expression.kronello");
    let doc = document();
    call(
        &["project", "create"],
        json!({"project":path,"document":doc}),
        true,
    );
    let c = &doc["compositions"][0];
    let node = &c["nodes"][0];
    let property = node["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["descriptor"]["key"] == "kronello.opacity")
        .unwrap();
    let id = "173087e0-c21b-43de-9371-8e1da051095a";
    let commands = json!([
        {"expression_set":{"expression":{"id":id,"version":1,"value_type":"scalar","nodes":[{"literal":{"kind":"scalar","value":0.4}}]}}},
        {"property_source_set":{"object":node["id"],"property":property["id"],"source":{"kind":"expression","value":id}}}
    ]);
    let planned = call(
        &["edit", "plan"],
        json!({"project":path,"base_revision":"1","commands":commands}),
        true,
    );
    let payload = json!({"project":path,"base_revision":"1","commands":commands,"plan_hash":planned["result"]["value"]["plan_hash"],"session_id":"96607679-eefd-407a-a3a8-59943f2bd82f","idempotency_key":"expression"});
    let applied = call(&["edit", "apply"], payload.clone(), true);
    assert_eq!(call(&["edit", "apply"], payload, true), applied);
    let samples = call(
        &["property", "sample"],
        json!({"project":path,"composition":c["id"],"keys":[{"kind":"node","instance_path":[],"node":node["id"],"property":property["id"]}],"times":[{"num":"1","den":"2"}]}),
        true,
    );
    assert_eq!(samples["result"]["value"]["revision"], "2");
    assert_eq!(
        samples["result"]["value"]["samples"][0]["values"][0],
        json!({"kind":"scalar","value":0.4})
    );
}
#[test]
fn example_conforms_to_public_schema_and_pins_fixture() {
    let schema: Value =
        serde_json::from_str(include_str!("../../../schemas/project-v1.schema.json")).unwrap();
    jsonschema::validator_for(&schema)
        .unwrap()
        .validate(&document())
        .unwrap();
    let font =
        std::fs::read(kronello_testkit::resolve_fixture("noto-sans-cjk-jp").unwrap()).unwrap();
    assert_eq!(
        document()["texts"][0]["styles"][0]["font"]["sha256"],
        format!("{:x}", Sha256::digest(font))
    );
}
fn invoke(args: &[&str], input: &str, success: bool) -> (Value, String) {
    invoke_with_env(args, input, success, None)
}
fn invoke_with_env(
    args: &[&str],
    input: &str,
    success: bool,
    environment: Option<(&str, &str)>,
) -> (Value, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_kronello"));
    // Keep ambient fault injection from changing unrelated tests.
    command.env_remove("KRONELLO_TEST_ADAPTER_UNAVAILABLE");
    if let Some((name, value)) = environment {
        command.env(name, value);
    }
    let mut child = command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    // Parsing the entire stdout rejects appended log lines or extra documents.
    let result: Value = serde_json::from_str(&stdout).expect(&stdout);
    let _: Response = serde_json::from_str(&stdout).unwrap();
    assert_eq!(output.status.success(), success, "{stdout}\n{stderr}");
    assert_eq!(result["status"], if success { "success" } else { "error" });
    if !success {
        assert!(output.status.code().is_some_and(|code| code != 0));
        assert!(stderr.contains(result["error"]["code"].as_str().unwrap()));
    }
    (result, stderr)
}
fn call(command: &[&str], payload: Value, success: bool) -> Value {
    invoke(command, &payload.to_string(), success).0
}
fn create(path: &Path) -> Value {
    call(
        &["project", "create"],
        json!({"project": path, "document": document()}),
        true,
    )
}
fn input(path: &Path) -> Value {
    json!({
        "project": path,
        "composition": document()["compositions"][0]["id"],
        "region": {"origin": [0,0], "extent": [64,32], "pixels": [64,32]},
        "profile": {"working_space": "linear_rec709", "flatten_tolerance_px": 0.02},
        "fonts": [{"identity": document()["texts"][0]["styles"][0]["font"],
                   "path": kronello_testkit::resolve_fixture("noto-sans-cjk-jp").unwrap()}],
    })
}
fn sequence(path: &Path, out: &Path) -> Value {
    json!({"input": input(path),
        "range": {"start": {"num": "0", "den": "1"}, "end": {"num": "3", "den": "2"}},
        "frame_rate": {"num": "8", "den": "3"}, "output_directory": out,
    })
}
fn error_code(value: &Value) -> &str {
    value["error"]["code"].as_str().unwrap()
}

#[test]
fn requests_errors_and_diagnostics_are_single_json_documents() {
    for (args, input) in [
        (vec![], "not JSON"),
        (vec![], "{}"),
        (vec!["--backend", "automatic"], "{}"),
        (vec!["--raw-ffmpeg-args"], "{}"),
        (
            vec!["project", "info"],
            "{\"project\":\"a.kronello\",\"project\":\"b.kronello\"}",
        ),
        (
            vec!["project", "info"],
            "{\"project\":\"a.kronello\",\"unknown\":true}",
        ),
        (
            vec!["project", "info"],
            "{\"operation\":\"project.info\",\"project\":\"a.kronello\"}",
        ),
    ] {
        assert_eq!(
            error_code(&invoke(&args, input, false).0),
            "INVALID_REQUEST"
        );
    }
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.kronello");
    let request = json!({"operation": "project.info", "project": missing});
    assert_eq!(
        error_code(&call(&[], request.clone(), false)),
        "PROJECT_NOT_FOUND"
    );
    assert_eq!(
        error_code(&invoke(&["--request-json", &request.to_string()], "", false).0),
        "PROJECT_NOT_FOUND"
    );
    assert!(!missing.exists());
}

#[test]
fn create_import_export_info_preserve_public_document_and_revision() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("demo.kronello");
    let mut doc = document();
    doc["future_field"] = json!({"text": "$(touch forbidden); 外部文字列"});
    let created = call(
        &["project", "create"],
        json!({"project": path, "document": doc}),
        true,
    );
    assert_eq!(created["result"]["value"]["revision"], "1");
    let exported = call(
        &[],
        json!({"operation": "project.export", "project": path}),
        true,
    );
    assert_eq!(exported["result"]["value"]["document"], doc);
    let before = std::fs::read(&path).unwrap();
    assert_eq!(
        error_code(&call(
            &["project", "create"],
            json!({"project": path, "document": doc}),
            false
        )),
        "PROJECT_EXISTS"
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    for revision in ["0", "-1", "18446744073709551616"] {
        let response = call(
            &["project", "import"],
            json!({"project": path, "base_revision": revision, "document": doc}),
            false,
        );
        assert_eq!(
            error_code(&response),
            if revision == "0" {
                "REVISION_CONFLICT"
            } else {
                "INVALID_REQUEST"
            }
        );
    }
    doc["name"] = json!("imported 日本語");
    let imported = call(
        &["project", "import"],
        json!({"project": path, "base_revision": "1", "document": doc}),
        true,
    );
    assert_eq!(imported["result"]["value"]["revision"], "2");
    let info = call(&["project", "info"], json!({"project": path}), true);
    assert_eq!(info["result"]["value"]["name"], "imported 日本語");
    assert_eq!(
        info["result"]["value"]["content_hash"],
        imported["result"]["value"]["content_hash"]
    );
    assert_eq!(
        call(&["project", "export"], json!({"project": path}), true)["result"]["value"]["document"],
        doc
    );
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        1,
        "closed store must have no sidecars"
    );
}

fn headless(backend: Option<&str>) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("demo.kronello");
    create(&path);
    // Import through the public API as well as creating the initial document.
    call(
        &["project", "import"],
        json!({"project": path, "base_revision": "1", "document": document()}),
        true,
    );
    let out = dir.path().join("frames");
    let mut args = vec![];
    if let Some(backend) = backend {
        args.extend(["--backend", backend]);
    }
    args.extend(["render", "sequence"]);
    let (response, stderr) = invoke(&args, &sequence(&path, &out).to_string(), true);
    if backend.is_some() {
        assert!(stderr.is_empty(), "{stderr}");
    }
    let manifest = &response["result"]["value"];
    let typed: kronello_render::SequenceMetadata =
        serde_json::from_value(manifest.clone()).unwrap();
    assert_eq!(typed.frames.len(), 4);
    assert_eq!(
        serde_json::to_value(typed.frames[1].metadata.time).unwrap(),
        json!({"num":"3", "den":"8"})
    );
    assert_eq!(std::fs::read_dir(&out).unwrap().count(), 13);
    let disk: Value =
        serde_json::from_slice(&std::fs::read(out.join("sequence.json")).unwrap()).unwrap();
    assert_eq!(&disk, manifest);
    for (ordinal, frame) in typed.frames.iter().enumerate() {
        assert_eq!(frame.metadata.revision, "2");
        assert_eq!(
            frame.metadata.backend,
            if backend.is_some() {
                "cpu_reference_float32"
            } else {
                "wgpu_rgba16f"
            }
        );
        assert_eq!(frame.metadata.frame_index, Some(ordinal.to_string()));
        assert_eq!(frame.metadata.sequence_number, Some(ordinal as u64));
        assert_eq!(frame.metadata.font_locks.len(), 1);
        assert_eq!(
            serde_json::to_value(&frame.metadata.font_locks[0]).unwrap(),
            document()["texts"][0]["styles"][0]["font"]
        );
        for artifact in [&frame.numeric, &frame.display] {
            let bytes = std::fs::read(out.join(&artifact.name)).unwrap();
            assert_eq!(bytes.len() as u64, artifact.bytes);
            assert_eq!(format!("{:x}", Sha256::digest(bytes)), artifact.sha256);
        }
        assert_eq!(
            std::fs::read(out.join(&frame.numeric.name)).unwrap().len(),
            64 * 32 * 8
        );
        let stored: kronello_render::FrameMetadata =
            serde_json::from_slice(&std::fs::read(out.join(&frame.metadata_file)).unwrap())
                .unwrap();
        assert_eq!(stored, frame.metadata);
        let png = std::fs::read(out.join(&frame.display.name)).unwrap();
        let mut reader = png::Decoder::new(std::io::Cursor::new(png))
            .read_info()
            .unwrap();
        assert_eq!(reader.info().width, 64);
        assert_eq!(reader.info().height, 32);
        assert_eq!(reader.info().bit_depth, png::BitDepth::Sixteen);
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut pixels).unwrap();
        // Green pixels come only from the Japanese glyphs, not the red shape.
        assert!(
            pixels
                .chunks_exact(8)
                .any(
                    |p| u16::from_be_bytes([p[2], p[3]]) > u16::from_be_bytes([p[0], p[1]])
                        && u16::from_be_bytes([p[6], p[7]]) > 0
                )
        );
    }
    let hashes: std::collections::BTreeSet<_> =
        typed.frames.iter().map(|f| &f.numeric.sha256).collect();
    assert_eq!(
        hashes.len(),
        4,
        "animated shape must change every sampled frame"
    );
    let before = std::fs::read(out.join("sequence.json")).unwrap();
    let error = invoke(&args, &sequence(&path, &out).to_string(), false).0;
    assert_eq!(error_code(&error), "OUTPUT_IO_ERROR");
    assert_eq!(std::fs::read(out.join("sequence.json")).unwrap(), before);
}
#[test]
fn cpu_headless_animated_shape_japanese_text_sequence_and_metadata() {
    headless(Some("cpu-reference"));
}
#[test]
fn gpu_headless_default_backend_animated_shape_japanese_text_sequence() {
    headless(None);
}

#[test]
#[cfg(debug_assertions)]
fn adapter_unavailable_default_backend_is_typed_without_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("demo.kronello");
    create(&path);
    let out = dir.path().join("frames");
    let injected = Some(("KRONELLO_TEST_ADAPTER_UNAVAILABLE", "1"));
    // Exercise both the implicit default and explicit GPU selection. Neither
    // may invoke the CPU renderer or publish frames/metadata after failure.
    for args in [
        vec!["render", "sequence"],
        vec!["--backend", "gpu", "render", "sequence"],
    ] {
        let (response, stderr) =
            invoke_with_env(&args, &sequence(&path, &out).to_string(), false, injected);
        assert_eq!(error_code(&response), "ADAPTER_UNAVAILABLE");
        assert_eq!(response.as_object().unwrap().len(), 2);
        assert!(response.get("result").is_none());
        let message = response["error"]["message"].as_str().unwrap();
        assert!(message.contains("test-only injected adapter unavailability"));
        assert_eq!(stderr, format!("ADAPTER_UNAVAILABLE: {message}\n"));
        assert!(!response.to_string().contains("cpu_reference_float32"));
        assert!(!stderr.contains("cpu_reference_float32"));
        assert!(
            !out.exists(),
            "failure must not publish any output directory"
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    // A positive control proves this same request is renderable and that the
    // factory fault does not disable explicitly requested CPU execution.
    let (response, _) = invoke_with_env(
        &["--backend", "cpu-reference", "render", "sequence"],
        &sequence(&path, &out).to_string(),
        true,
        injected,
    );
    assert_eq!(
        response["result"]["value"]["frames"][0]["metadata"]["backend"],
        "cpu_reference_float32"
    );
    assert!(out.join("sequence.json").is_file());
}

#[test]
fn font_missing_hash_mismatch_unsupported_and_invalid_time_are_typed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("demo.kronello");
    create(&path);
    let out = dir.path().join("failed");
    let args = ["--backend", "cpu-reference", "render", "sequence"];
    let mut request = sequence(&path, &out);
    request["input"]["fonts"] = json!([]);
    assert_eq!(error_code(&call(&args, request, false)), "FONT_MISSING");
    assert!(!out.exists());
    let mut request = sequence(&path, &out);
    request["input"]["fonts"][0]["path"] = json!(dir.path().join("missing.otf"));
    assert_eq!(error_code(&call(&args, request, false)), "FONT_MISSING");
    let wrong_font = dir.path().join("wrong.otf");
    std::fs::write(&wrong_font, b"wrong font").unwrap();
    let mut request = sequence(&path, &out);
    request["input"]["fonts"][0]["path"] = json!(wrong_font);
    assert_eq!(
        error_code(&call(&args, request, false)),
        "ASSET_HASH_MISMATCH"
    );
    let mut request = sequence(&path, &out);
    request["range"]["end"]["den"] = json!("0");
    assert_eq!(error_code(&call(&args, request, false)), "INVALID_REQUEST");
    let mut request = sequence(&path, &out);
    request["input"]["region"]["pixels"] = json!([0, 32]);
    assert_eq!(error_code(&call(&args, request, false)), "RENDER_ERROR");
    let mut doc = document();
    doc["future_effect"] = json!(true);
    call(
        &["project", "import"],
        json!({"project": path, "base_revision": "1", "document": doc}),
        true,
    );
    assert_eq!(
        error_code(&call(&args, sequence(&path, &out), false)),
        "UNSUPPORTED_FEATURE"
    );
    assert!(!out.exists());
}

#[test]
fn frame_query_and_injected_backend_share_service_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("demo.kronello");
    create(&path);
    let payload = json!({"operation": "render.frame", "input": input(&path), "time": {"num": "1", "den": "2"}});
    let result = call(&["--backend", "cpu-reference"], payload.clone(), true);
    let frame = &result["result"]["value"];
    assert_eq!(frame["linear"].as_array().unwrap().len(), 2048);
    assert_eq!(frame["metadata"]["time"], json!({"num":"1", "den":"2"}));
    assert!(frame["metadata"]["frame_index"].is_null());
    struct Failing(std::cell::Cell<usize>);
    impl kronello_render::RenderBackend for Failing {
        fn name(&self) -> &str {
            "test_injected"
        }
        fn execute(
            &self,
            _: &kronello_render::RenderDag,
        ) -> Result<kronello_render::BackendFrame, kronello_render::RenderError> {
            self.0.set(self.0.get() + 1);
            Err(kronello_render::RenderError::Backend {
                code: "DEVICE_UNAVAILABLE",
                message: "injected failure".into(),
            })
        }
    }
    let backend = Failing(std::cell::Cell::new(0));
    let request: Request = serde_json::from_value(payload).unwrap();
    let response = Service::with_backend(&backend).execute(request);
    let Response::Error { error } = response else {
        panic!("injected error required")
    };
    assert_eq!(error.code, "DEVICE_UNAVAILABLE");
    assert_eq!(backend.0.get(), 1);
}

#[test]
fn locked_project_and_invalid_create_leave_existing_state_intact() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("demo.kronello");
    create(&path);
    let store = kronello_store::ProjectStore::open(
        &path,
        kronello_store::OpenOptions {
            mode: kronello_store::OpenMode::ForceSafe,
        },
    )
    .unwrap();
    assert_eq!(
        error_code(&call(&["project", "info"], json!({"project": path}), false)),
        "PROJECT_LOCKED"
    );
    store.close().unwrap();
    let missing = dir.path().join("invalid.kronello");
    let mut invalid = document();
    invalid["schema_version"] = json!(99);
    assert_eq!(
        error_code(&call(
            &["project", "create"],
            json!({"project": missing, "document": invalid}),
            false
        )),
        "UNSUPPORTED_SCHEMA_VERSION"
    );
    assert!(!missing.exists());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

fn edit_commands(value: f64) -> Value {
    let d = document();
    json!([{"property_source_set": {
        "object":d["compositions"][0]["nodes"][0]["id"],
        "property":d["compositions"][0]["nodes"][0]["properties"][1]["id"],
        "source":{"kind":"constant","value":{"kind":"scalar","value":value}}
    }}])
}
fn edit_payload(path: &Path, base: &str, key: &str, commands: Value) -> Value {
    let planned = call(
        &["edit", "plan"],
        json!({"project":path,"base_revision":base,"commands":commands}),
        true,
    );
    json!({"project":path,"base_revision":base,"plan_hash":planned["result"]["value"]["plan_hash"],
        "idempotency_key":key,"session_id":"1b549e15-9862-4168-a638-0cd2f2b0e6b1","commands":commands})
}

#[test]
fn persisted_edit_receipts_replay_from_real_cli_processes_after_edits_and_compact() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.kronello");
    create(&path);
    let payload = edit_payload(&path, "1", "first", edit_commands(2.0));
    let first = call(&["edit", "apply"], payload.clone(), true);
    // Every call starts a fresh CLI process; no process memory can hold receipts.
    assert_eq!(call(&["edit", "apply"], payload.clone(), true), first);
    let second_payload = edit_payload(&path, "2", "second", edit_commands(3.0));
    let second = call(&["edit", "apply"], second_payload, true);
    let undo = json!({"project":path,"base_revision":"3","event_id":second["result"]["value"]["id"],"idempotency_key":"undo","session_id":"1b549e15-9862-4168-a638-0cd2f2b0e6b1"});
    let undone = call(&["edit", "undo"], undo.clone(), true);
    assert_eq!(call(&["edit", "undo"], undo, true), undone);
    assert_eq!(call(&["edit", "apply"], payload.clone(), true), first);
    let mut changed = payload.clone();
    changed["commands"] = edit_commands(3.0);
    assert_eq!(
        error_code(&call(&["edit", "apply"], changed, false)),
        "IDEMPOTENCY_KEY_REUSED"
    );
    let mut stale = payload.clone();
    stale["idempotency_key"] = json!("stale");
    assert_eq!(
        error_code(&call(&["edit", "apply"], stale, false)),
        "REVISION_CONFLICT"
    );
    let h = call(
        &["history", "list"],
        json!({"project":path,"since_revision":"1"}),
        true,
    );
    assert_eq!(h["result"]["value"]["revision"], "4");
    assert_eq!(h["result"]["value"]["events"].as_array().unwrap().len(), 3);
    let mut store =
        kronello_store::ProjectStore::open(&path, kronello_store::OpenOptions::default()).unwrap();
    store.compact(3).unwrap();
    store.close().unwrap();
    // The complete receipt remains even when the original event is compacted.
    assert_eq!(call(&["edit", "apply"], payload, true), first);
    let h = call(&["history", "list"], json!({"project":path}), true);
    assert_eq!(h["result"]["value"]["revision"], "4");
    assert_eq!(
        call(&["project", "info"], json!({"project":path}), true)["result"]["value"]["revision"],
        "4"
    );
}

#[test]
fn concurrent_cli_same_key_returns_one_event_and_different_keys_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("concurrent.kronello");
    create(&path);
    let payload = edit_payload(&path, "1", "same", edit_commands(2.0));
    let spawn = |payload: &Value| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_kronello"))
            .args(["edit", "apply"])
            .env_remove("KRONELLO_TEST_ADAPTER_UNAVAILABLE")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(payload.to_string().as_bytes())
            .unwrap();
        child
    };
    let a = spawn(&payload);
    let b = spawn(&payload);
    let a = a.wait_with_output().unwrap();
    let b = b.wait_with_output().unwrap();
    assert!(a.status.success(), "{}", String::from_utf8_lossy(&a.stderr));
    assert!(b.status.success(), "{}", String::from_utf8_lossy(&b.stderr));
    let a: Value = serde_json::from_slice(&a.stdout).unwrap();
    let b: Value = serde_json::from_slice(&b.stdout).unwrap();
    assert_eq!(a, b);
    assert_eq!(a["result"]["value"]["revision"], 2);
    let first = edit_payload(&path, "2", "left", edit_commands(2.25));
    let mut second = first.clone();
    second["idempotency_key"] = json!("right");
    let a = spawn(&first);
    let b = spawn(&second);
    let a = a.wait_with_output().unwrap();
    let b = b.wait_with_output().unwrap();
    assert_ne!(a.status.success(), b.status.success());
    let error = if a.status.success() { b } else { a };
    let error: Value = serde_json::from_slice(&error.stdout).unwrap();
    assert_eq!(error_code(&error), "REVISION_CONFLICT");
    assert_eq!(
        call(&["project", "info"], json!({"project":path}), true)["result"]["value"]["revision"],
        "3"
    );
}

#[test]
fn structured_api_queries_and_empty_capabilities_payload_from_real_cli() {
    let capabilities = call(&["capabilities", "get"], json!({}), true);
    assert_eq!(capabilities["result"]["kind"], "capabilities");
    assert_eq!(
        capabilities["result"]["value"]["commands"]
            .as_array()
            .unwrap()
            .len(),
        36
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("query.kronello");
    create(&path);
    let doc = document();
    let composition = &doc["compositions"][0]["id"];
    let scene = call(
        &["scene", "query"],
        json!({"project":path, "composition":composition}),
        true,
    );
    assert_eq!(scene["result"]["kind"], "scene");
    assert_eq!(
        scene["result"]["value"]["nodes"].as_array().unwrap().len(),
        2
    );
    let sample = call(
        &["property", "sample"],
        json!({"project":path, "composition":composition,
        "keys":[{"kind":"node", "instance_path":[], "node":doc["compositions"][0]["nodes"][0]["id"],
            "property":doc["compositions"][0]["nodes"][0]["properties"][0]["id"]}],
        "times":[{"num":"1","den":"2"}]}),
        true,
    );
    assert_eq!(sample["result"]["kind"], "samples");
    assert_eq!(sample["result"]["value"]["samples"][0]["unit"], "design_px");
    let error = call(
        &["capabilities", "get"],
        json!({"shell":"touch /tmp/never"}),
        false,
    );
    assert_eq!(error_code(&error), "INVALID_REQUEST");
}

#[test]
fn history_pagination_and_execution_input_rejection_from_real_cli() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.kronello");
    create(&path);
    let payload = edit_payload(&path, "1", "change", edit_commands(2.0));
    let session = payload["session_id"].clone();
    let event = call(&["edit", "apply"], payload, true);
    let event_id = &event["result"]["value"]["id"];
    call(
        &["edit", "undo"],
        json!({"project":path,"base_revision":"2","session_id":session,
            "event_id":event_id,"idempotency_key":"undo"}),
        true,
    );
    let first = call(
        &["history", "list"],
        json!({"project":path,"since_revision":"1","limit":1,"session_id":session}),
        true,
    );
    let history = &first["result"]["value"];
    assert_eq!(history["revision"], "3");
    assert_eq!(history["events"][0]["event"]["id"], *event_id);
    assert_eq!(history["events"][0]["event"]["session_id"], session);
    assert!(
        !history["events"][0]["event"]["changed_keys"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(history["events"][0]["undone"], true);
    let last = call(
        &[],
        json!({"operation":"history.list","project":path,"limit":1,"session_id":session,
            "since_revision":history["next_since_revision"]}),
        true,
    );
    assert_eq!(
        last["result"]["value"]["events"][0]["event"]["undo_of"],
        *event_id
    );
    assert!(last["result"]["value"]["next_since_revision"].is_null());
    let before = call(&["project", "export"], json!({"project":path}), true);
    for field in ["shell", "url", "ffmpeg_args"] {
        let mut request = json!({"project":path});
        request[field] = json!("untrusted");
        assert_eq!(
            error_code(&call(&["project", "info"], request, false)),
            "INVALID_REQUEST"
        );
    }
    assert_eq!(
        error_code(&call(
            &["project", "info"],
            json!({"project":"https://example.invalid/movie.kronello"}),
            false
        )),
        "INVALID_REQUEST"
    );
    let duplicate = invoke(
        &["capabilities", "get"],
        r#"{"operation":"capabilities.get"}"#,
        false,
    )
    .0;
    assert_eq!(error_code(&duplicate), "INVALID_REQUEST");
    assert_eq!(
        call(&["project", "export"], json!({"project":path}), true),
        before
    );
}

#[test]
fn template_commands_share_schema_service_and_report_final_overflow() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("template.kronello");
    let document: Value =
        serde_json::from_str(include_str!("../../../examples/template-001.project.json")).unwrap();
    let definition: Value = serde_json::from_str(include_str!(
        "../../../examples/template-001.definition.json"
    ))
    .unwrap();
    call(
        &["project", "create"],
        json!({"project":project,"document":document}),
        true,
    );
    let session = "d42e2df2-f299-4e2d-811d-52dab580a772";
    let instance = "9e2d1247-479c-47db-ad74-c89b362e00aa";
    call(
        &["template", "define"],
        json!({"project":project,"base_revision":"1","session_id":session,"idempotency_key":"define","definition":definition}),
        true,
    );
    call(
        &["template", "instantiate"],
        json!({"project":project,"base_revision":"2","session_id":session,"idempotency_key":"place","composition":document["compositions"][0]["id"],"node":"7706a562-00d9-4a1b-9467-2cd97c57d3d4","index":0,"instance":{"id":instance,"definition_ref":definition["id"],"version":"1.0.0","duration":{"num":"5","den":"1"},"inputs":{}}}),
        true,
    );
    call(
        &["template", "set_duration"],
        json!({"project":project,"base_revision":"3","session_id":session,"idempotency_key":"duration","instance":instance,"duration":{"num":"8","den":"1"}}),
        true,
    );
    let exported = call(&["project", "export"], json!({"project":project}), true);
    let saved = &exported["result"]["value"]["document"];
    let schema: Value =
        serde_json::from_str(include_str!("../../../schemas/project-v1.schema.json")).unwrap();
    jsonschema::validator_for(&schema)
        .unwrap()
        .validate(saved)
        .unwrap();
    assert_eq!(
        saved["template_instances"][0]["duration"],
        json!({"num":"8","den":"1"})
    );
    assert_eq!(saved["template_instances"][0]["version"], "1.0.0");
    assert_eq!(
        saved["templates"][0]["content_hash"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
    call(
        &["template", "set_input"],
        json!({"project":project,"base_revision":"4","session_id":session,"idempotency_key":"headline","instance":instance,"name":"headline","value":{"kind":"string","value":"一\n二\n三"}}),
        true,
    );
    let output = dir.path().join("frames");
    let input = json!({"project":project,"composition":document["compositions"][0]["id"],"region":{"origin":[0,0],"extent":[64,32],"pixels":[64,32]},"fonts":[{"identity":document["texts"][0]["styles"][0]["font"],"path":kronello_testkit::resolve_fixture("noto-sans-cjk-jp").unwrap()}]});
    let result = call(
        &["--backend", "cpu-reference", "render", "sequence"],
        json!({"input":input,"range":{"start":{"num":"0","den":"1"},"end":{"num":"1","den":"1"}},"frame_rate":{"num":"1","den":"1"},"output_directory":output}),
        false,
    );
    assert_eq!(result["error"]["code"], "TEMPLATE_OVERFLOW");
    assert_eq!(result["error"]["details"]["actual_lines"], 3);
    assert!(!output.exists());
}

#[test]
fn unavailable_ffmpeg_returns_typed_error_without_default_library_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing-ffmpeg");
    let (result, _) = invoke_with_env(
        &["capabilities", "get"],
        "{}",
        false,
        Some(("KRONELLO_FFMPEG_LIB_DIR", missing.to_str().unwrap())),
    );
    assert_eq!(error_code(&result), "FFMPEG_UNAVAILABLE");
}

#[test]
fn media_commands_use_shared_service_and_preserve_source_revision_on_collect() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("media.kronello");
    let source = dir.path().join("asset.bin");
    std::fs::write(&source, b"media cli").unwrap();
    let asset = kronello_model::AssetId::new();
    let mut doc = document();
    doc["assets"] = json!([{
        "id":asset, "content_hash":format!("{:x}", Sha256::digest(b"media cli")),
        "kind":"video", "streams":[], "locator":{"relative":"asset.bin", "absolute":null}
    }]);
    call(
        &["project", "create"],
        json!({"project":path, "document":doc}),
        true,
    );
    let search = dir.path().join("search");
    std::fs::create_dir(&search).unwrap();
    std::fs::rename(source, search.join("renamed.bin")).unwrap();
    let updated = call(
        &["asset", "relink"],
        json!({"project":path, "base_revision":"1", "asset":asset, "search_directory":search}),
        true,
    );
    let collected = call(
        &["project", "collect"],
        json!({"project":path, "output_directory":dir.path().join("collected")}),
        true,
    );
    assert_eq!(collected["result"]["kind"], "collected");
    assert_eq!(collected["result"]["value"]["asset_count"], 1);
    let info = call(&["project", "info"], json!({"project":path}), true);
    assert_eq!(
        info["result"]["value"]["revision"],
        updated["result"]["value"]["revision"]
    );
}

#[test]
fn nle_sequence_target_and_clip_trim_use_shared_machine_commands() {
    let p: Value =
        serde_json::from_str(include_str!("../../../examples/nle-001.project.json")).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nle-cli.kronello");
    invoke(
        &["project", "create"],
        &json!({"project":path,"document":p}).to_string(),
        true,
    );
    let seq = &p["sequences"][0];
    let clip = &seq["tracks"][0]["clips"][0];
    let (event,_)=invoke(&["clip","trim"],&json!({"project":path,"base_revision":"1","session_id":"e20090e7-c3de-44e7-bb91-fd15b94bcde4","idempotency_key":"trim","sequence":seq["id"],"clip":clip["id"],"range":{"start":{"num":"9","den":"4"},"end":{"num":"11","den":"4"}}}).to_string(),true);
    let (pixels,_)=invoke(&["--backend","cpu-reference","render","frame"],&json!({"input":{"project":path,"target":{"kind":"sequence","sequence":seq["id"]},"region":{"origin":[0.0,0.0],"extent":[64.0,32.0],"pixels":[64,32]}},"time":{"num":"5","den":"2"}}).to_string(),true);
    assert_eq!(
        pixels["result"]["value"]["metadata"]["target"]["kind"],
        "sequence"
    );
    assert_eq!(
        pixels["result"]["value"]["metadata"]["backend"],
        "cpu_reference_float32"
    );
    invoke(&["edit","undo"],&json!({"project":path,"base_revision":"2","session_id":"e20090e7-c3de-44e7-bb91-fd15b94bcde4","idempotency_key":"undo-trim","event_id":event["result"]["value"]["id"]}).to_string(),true);
    let (exported, _) = invoke(
        &["project", "export"],
        &json!({"project":path}).to_string(),
        true,
    );
    assert_eq!(exported["result"]["value"]["document"], p);
}

#[test]
fn template2_cli_previews_and_migration_plan_share_schema_and_explicit_apply() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("template2.kronello");
    let document: Value =
        serde_json::from_str(include_str!("../../../examples/template-002.project.json")).unwrap();
    let definition: Value = serde_json::from_str(include_str!(
        "../../../examples/template-002.definition.json"
    ))
    .unwrap();
    let session = "d42e2df2-f299-4e2d-811d-52dab580a772";
    let instance = json!({"id":"9e2d1247-479c-47db-ad74-c89b362e00aa","definition_ref":definition["id"],
        "version":"1.0.0","duration":{"num":"8","den":"1"},"variant":"portrait","inputs":{}});
    let fonts = json!([{"identity":document["texts"][0]["styles"][0]["font"],
        "path":kronello_testkit::resolve_fixture("noto-sans-cjk-jp").unwrap()}]);
    call(
        &["project", "create"],
        json!({"project":project,"document":document}),
        true,
    );
    call(
        &["template", "define"],
        json!({"project":project,"base_revision":"1","session_id":session,"idempotency_key":"define","definition":definition}),
        true,
    );
    call(
        &["template", "instantiate"],
        json!({"project":project,"base_revision":"2","session_id":session,"idempotency_key":"place",
        "composition":document["compositions"][0]["id"],"node":"7706a562-00d9-4a1b-9467-2cd97c57d3d4","index":0,"instance":instance}),
        true,
    );
    let preview = call(
        &["template", "preview"],
        json!({"project":project,"instance":instance,"time":{"num":"1","den":"1"},"fonts":fonts}),
        true,
    );
    let schema: Value =
        serde_json::from_str(include_str!("../../../schemas/api-v1.schema.json")).unwrap();
    jsonschema::validator_for(&schema)
        .unwrap()
        .validate(&preview)
        .unwrap();
    assert_eq!(preview["result"]["kind"], "template_preview");
    assert!(preview["result"]["value"]["diagnostic"].is_null());
    assert_eq!(
        preview["result"]["value"]["design_extent"],
        json!({"width":32.0,"height":64.0})
    );
    let mut next = definition.clone();
    next["id"] = json!("b6288cb1-55bd-48af-ae0b-e66eac4114a0");
    next["version"] = json!("2.0.0");
    next["duration_policy"]["middle_mode"] = json!("hold");
    call(
        &["template", "define"],
        json!({"project":project,"base_revision":"3","session_id":session,"idempotency_key":"v2","definition":next}),
        true,
    );
    let planned = call(
        &["--backend", "cpu-reference", "template", "migration_plan"],
        json!({"project":project,"base_revision":"4","instance":instance["id"],
        "definition":next["id"],"variant":"portrait","time":{"num":"1","den":"1"},"fonts":fonts,"region":{"origin":[0.0,0.0],"extent":[32.0,64.0],"pixels":[16,32]}}),
        true,
    );
    jsonschema::validator_for(&schema)
        .unwrap()
        .validate(&planned)
        .unwrap();
    let value = &planned["result"]["value"];
    assert_eq!(
        value["before"]["frame"]["metadata"]["backend"],
        "cpu_reference_float32"
    );
    assert_eq!(
        value["after"]["frame"]["metadata"]["backend"],
        "cpu_reference_float32"
    );
    assert!(
        value["after"]["frame"]["display"]
            .as_array()
            .unwrap()
            .iter()
            .any(|pixel| pixel[3].as_f64().unwrap() > 0.0)
    );
    assert_eq!(value["before"]["local_time"], json!({"num":"1","den":"1"}));
    assert_eq!(value["after"]["local_time"], json!({"num":"2","den":"5"}));
    let saved = call(&["project", "export"], json!({"project":project}), true);
    assert_eq!(saved["result"]["value"]["revision"], "4");
    assert_eq!(
        saved["result"]["value"]["document"]["template_instances"][0]["version"],
        "1.0.0"
    );
    call(
        &["edit", "apply"],
        json!({"project":project,"base_revision":"4","session_id":session,"idempotency_key":"migrate",
        "commands":value["plan"]["commands"],"plan_hash":value["plan"]["plan_hash"]}),
        true,
    );
    let saved = call(&["project", "export"], json!({"project":project}), true);
    assert_eq!(saved["result"]["value"]["revision"], "5");
    assert_eq!(
        saved["result"]["value"]["document"]["template_instances"][0]["version"],
        "2.0.0"
    );
}

#[test]
fn nle2_generator_query_and_move_use_shared_cli_plan_apply() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("generator.kronello");
    let mut doc: Value =
        serde_json::from_str(include_str!("../../../examples/nle-001.project.json")).unwrap();
    doc["sequences"][0]["tracks"][0]["clips"][0]["source_ref"] = json!({"kind":"generator","generator":"kronello.solid","version":1,"color":{"space":"srgb","components":{"r":1.0,"g":0.0,"b":0.0,"alpha":1.0}}});
    call(
        &["project", "create"],
        json!({"project":path,"document":doc}),
        true,
    );
    let sequence = &doc["sequences"][0]["id"];
    let clip = &doc["sequences"][0]["tracks"][0]["clips"][0]["id"];
    let query = call(
        &["sequence", "query"],
        json!({"project":path,"sequence":sequence}),
        true,
    );
    assert_eq!(query["result"]["kind"], "timeline");
    assert_eq!(query["result"]["value"]["clips"][0]["kind"], "generator");
    let commands = json!([{"timeline":{"clip_move":{"sequence":sequence,"clip":clip,"delta":{"num":"1","den":"1"},"linked":false}}}]);
    let plan = call(
        &["edit", "plan"],
        json!({"project":path,"base_revision":"1","commands":commands}),
        true,
    );
    let payload = json!({"project":path,"base_revision":"1","commands":commands,"plan_hash":plan["result"]["value"]["plan_hash"],"session_id":"ab12cd34-0000-4000-8000-000000000001","idempotency_key":"nle2-cli"});
    let event = call(&["edit", "apply"], payload.clone(), true);
    assert_eq!(call(&["edit", "apply"], payload, true), event);
    let query = call(
        &["sequence", "query"],
        json!({"project":path,"sequence":sequence}),
        true,
    );
    assert_eq!(query["result"]["value"]["revision"], "2");
    call(
        &["edit", "undo"],
        json!({"project":path,"base_revision":"2","event_id":event["result"]["value"]["id"],"session_id":"ab12cd34-0000-4000-8000-000000000001","idempotency_key":"nle2-cli-undo"}),
        true,
    );
    let restored = call(
        &["sequence", "query"],
        json!({"project":path,"sequence":sequence}),
        true,
    );
    assert_eq!(restored["result"]["value"]["sequence"], doc["sequences"][0]);
}

#[test]
fn json_order_cli_machine_process() {
    let (response, _) = invoke(
        &[],
        r#"{"commands":[{"property_source_set":{"source":{"value":{"value":1.5,"kind":"scalar"},"kind":"constant"},"property":"00000000-0000-0000-0000-000000000002","object":"00000000-0000-0000-0000-000000000001"}}],"base_revision":"0","project":"missing-json-order.kronello","operation":"edit.plan"}"#,
        false,
    );
    assert_eq!(response["error"]["code"], "PROJECT_NOT_FOUND");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("json-order.kronello");
    let doc: Value =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    let node = &doc["compositions"][0]["nodes"][0];
    let property = node["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["descriptor"]["key"] == "kronello.opacity")
        .unwrap();

    call(
        &["project", "create"],
        json!({"project":path,"document":doc}),
        true,
    );
    let canonical = json!({"operation":"edit.plan","project":path,"base_revision":"1","commands":[{"property_source_set":{"object":node["id"],"property":property["id"],"source":{"kind":"constant","value":{"kind":"scalar","value":0.5}}}}]});
    let raw = canonical.to_string().replace(
        r#""kind":"scalar","value":0.5"#,
        r#""value":0.5,"kind":"scalar""#,
    );
    assert_ne!(raw, canonical.to_string());
    let (actual, _) = invoke(&[], &raw, true);
    assert_eq!(actual, call(&[], canonical, true));
    let invalid = raw.replace(r#""value":0.5"#, r#""value":"bad""#);
    assert_eq!(
        invoke(&[], &invalid, false).0["error"]["code"],
        "INVALID_REQUEST"
    );
}
