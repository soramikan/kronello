use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};

fn spawn() -> Child {
    Command::new(env!("CARGO_BIN_EXE_kronello"))
        .args(["--events", "ndjson", "--backend", "cpu-reference"])
        .env_remove("KRONELLO_TEST_ADAPTER_UNAVAILABLE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}
fn wait(child: &mut Child) -> std::process::ExitStatus {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            panic!("stream failed to terminate");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
fn read_line(reader: &mut impl BufRead) -> Value {
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}
fn fixture() -> (tempfile::TempDir, Value) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stream.kronello");
    let doc: Value =
        serde_json::from_str(include_str!("../../../examples/ffi-preview.project.json")).unwrap();
    let service = kronello_service::Service::new(kronello_service::BackendSelection::CpuReference);
    assert!(matches!(
        service.execute_json(
            &json!({"operation":"project.create","project":path,"document":doc}).to_string()
        ),
        kronello_service::Response::Success { .. }
    ));
    let request = json!({"operation":"render.sequence","input":{"project":path,"composition":doc["compositions"][0]["id"],"fonts":[],"region":{"origin":[0,0],"extent":[64,32],"pixels":[16,8]}},"range":{"start":{"num":"0","den":"1"},"end":{"num":"1","den":"1"}},"frame_rate":{"num":"4","den":"1"},"output_directory":dir.path().join("frames")});
    (dir, request)
}
#[test]
fn ndjson_header_progress_terminal_and_error_are_versioned_schema_records() {
    let (_dir, request) = fixture();
    for (request, expected) in [
        (request, "end"),
        (
            json!({"operation":"project.info","project":"missing.kronello"}),
            "error",
        ),
        (json!({"operation":"no.such.operation"}), "error"),
    ] {
        let mut child = spawn();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(request.to_string().as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        let records: Vec<Value> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(
            records[0],
            json!({"record":"header","version":1,"sequence":0})
        );
        let api = kronello_service::api_json_schema();
        let schema = json!({"$ref":"#/$defs/CliEvent","$defs":api["$defs"]});
        let validator = jsonschema::validator_for(&schema).unwrap();
        for (sequence, record) in records.iter().enumerate() {
            assert_eq!(record["sequence"], sequence);
            validator.validate(record).unwrap();
        }
        assert_eq!(records.last().unwrap()["record"], "terminal");
        assert_eq!(records.last().unwrap()["outcome"], expected);
        assert_eq!(
            records.iter().filter(|r| r["record"] == "terminal").count(),
            1
        );
        assert_eq!(output.status.success(), expected == "end");
        if expected == "end" {
            assert_eq!(records[1]["record"], "progress");
            assert_eq!(records[1]["completed"], 0);
            assert_eq!(records[1]["total"], 4);
        } else {
            assert!(!output.stderr.is_empty());
        }
    }
}
#[cfg(unix)]
#[test]
fn sigint_while_reading_request_emits_cancelled_terminal_and_nonzero_exit() {
    let mut child = spawn();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    assert_eq!(read_line(&mut reader)["record"], "header");
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(child.id() as i32),
        nix::sys::signal::Signal::SIGINT,
    )
    .unwrap();
    assert!(!wait(&mut child).success());
    let terminal = read_line(&mut reader);
    assert_eq!(terminal["outcome"], "cancelled");
    assert_eq!(terminal["response"]["error"]["code"], "REQUEST_CANCELLED");
}
#[cfg(unix)]
#[test]
fn sigint_at_progress_cancels_sequence_without_manifest() {
    let (_dir, mut request) = fixture();
    request["frame_rate"] = json!({"num":"10000","den":"1"});
    let mut child = spawn();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    assert_eq!(read_line(&mut reader)["record"], "header");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(request.to_string().as_bytes())
        .unwrap();
    assert_eq!(read_line(&mut reader)["record"], "progress");
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(child.id() as i32),
        nix::sys::signal::Signal::SIGINT,
    )
    .unwrap();
    assert!(!wait(&mut child).success());
    let records: Vec<Value> = reader
        .lines()
        .map(|line| serde_json::from_str(&line.unwrap()).unwrap())
        .collect();
    assert_eq!(records.last().unwrap()["outcome"], "cancelled");
    assert!(!std::path::Path::new(request["output_directory"].as_str().unwrap()).exists());
}
#[test]
fn closed_stdout_cancels_at_progress_and_reports_only_to_stderr() {
    let (_dir, request) = fixture();
    let mut child = spawn();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    assert_eq!(read_line(&mut reader)["record"], "header");
    drop(reader);
    child
        .stdin
        .take()
        .unwrap()
        .write_all(request.to_string().as_bytes())
        .unwrap();
    assert!(!wait(&mut child).success());
    let output = child.wait_with_output().unwrap();
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("OUTPUT_IO_ERROR")
    );
    assert!(!std::path::Path::new(request["output_directory"].as_str().unwrap()).exists());
}
#[test]
fn default_stdout_remains_exactly_serialized_response_and_one_newline() {
    let (_dir, request) = fixture();
    let project = request["input"]["project"].clone();
    let request = json!({"operation":"project.info","project":project});
    let expected = kronello_service::Service::new(kronello_service::BackendSelection::CpuReference)
        .execute_json(&request.to_string());
    let output = Command::new(env!("CARGO_BIN_EXE_kronello"))
        .args(["--request-json", &request.to_string()])
        .output()
        .unwrap();
    assert_eq!(
        output.stdout,
        format!("{}\n", serde_json::to_string(&expected).unwrap()).as_bytes()
    );
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    // A flag-shaped option value must not opt into streaming.
    let output = Command::new(env!("CARGO_BIN_EXE_kronello"))
        .args(["--request-json", "--events"])
        .output()
        .unwrap();
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "error");
    assert!(response.get("record").is_none());
    assert!(!output.status.success());
}
