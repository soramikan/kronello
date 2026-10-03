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
