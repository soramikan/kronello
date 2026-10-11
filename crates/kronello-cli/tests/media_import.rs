use serde_json::{Value, json};
use std::process::Command;
use uuid::Uuid;

fn cli(args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_kronello"))
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    response["result"]["value"].clone()
}
#[test]
fn media_import_flags_and_json_share_the_service() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("cli.kronello");
    let path = dir.path().join("audio with spaces.mov");
    kronello_media::MediaRuntime::load()
        .unwrap()
        .encode_audio(
            &kronello_audio::Bus::new(
                0,
                kronello_audio::AudioBuffer::new(vec![[0.1, 0.1]; 4800]).unwrap(),
            ),
            kronello_audio::ClippingPolicy::Reject,
            &path,
        )
        .unwrap();
    cli(&["--request-json", &json!({"operation":"project.create","project":project,"document":kronello_model::Project::default()}).to_string()]);
    let asset = Uuid::new_v4().to_string();
    let session = Uuid::new_v4().to_string();
    let event = cli(&[
        "media",
        "import",
        "--project",
        project.to_str().unwrap(),
        "--asset",
        &asset,
        "--path",
        path.to_str().unwrap(),
        "--session-id",
        &session,
        "--idempotency-key",
        "import",
    ]);
    let replay = cli(&[
        "media",
        "import",
        "--request-json",
        &json!({"project":project,"asset":asset,"path":path,
        "base_revision":"1","session_id":session,"idempotency_key":"import"})
        .to_string(),
    ]);
    assert_eq!(event, replay);
    let assets = cli(&[
        "media",
        "query",
        "--request-json",
        &json!({"project":project}).to_string(),
    ]);
    assert_eq!(assets["assets"][0]["detail"]["kind"], "audio");
    assert_eq!(assets["assets"][0]["asset"], asset);
    let bad = Command::new(env!("CARGO_BIN_EXE_kronello"))
        .args([
            "media",
            "import",
            "--project",
            project.to_str().unwrap(),
            "--asset",
            &asset,
            "--path",
            "https://example.invalid/a.mov",
        ])
        .output()
        .unwrap();
    assert!(!bad.status.success());
    let error: Value = serde_json::from_slice(&bad.stdout).unwrap();
    assert_eq!(error["error"]["code"], "INVALID_REQUEST");
}
