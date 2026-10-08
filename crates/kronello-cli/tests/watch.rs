//! FLOW-003 (ADR-0130): `kronello watch` end-to-end through the shipped verb.
//! The watch loop resolves the stored preset once, submits stable files
//! through `export.batch`, and reports a typed exit status.
use std::process::Command;

use kronello_jobs::{JobConfig, JobStore};
use kronello_model::{EXPORT_PRESET_VERSION, ExportPreset, ExportPresetId, ExportRegion};
use kronello_service::{BackendSelection, Service};
use kronello_time::{FrameRate, Rational, TimeRange};
use serde_json::{Value, json};

struct Fixture {
    _cleanup: kronello_jobs::test_support::WorkerCleanup,
    temp: tempfile::TempDir,
    project: std::path::PathBuf,
    state: std::path::PathBuf,
    watch_dir: std::path::PathBuf,
    output: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("watch.kronello");
        let state = temp.path().join("state");
        let watch_dir = temp.path().join("in");
        let output = temp.path().join("out");
        std::fs::create_dir(&watch_dir).unwrap();
        std::fs::create_dir(&output).unwrap();
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
        document.as_object_mut().unwrap().remove("texts");
        let composition = document["compositions"][0]["id"].clone();
        document["export_presets"] = json!([serde_json::to_value(ExportPreset {
            version: EXPORT_PRESET_VERSION,
            id: ExportPresetId::new(),
            name: "watch-out".into(),
            composition: serde_json::from_value(json!(composition)).unwrap(),
            target: None,
            range: TimeRange::new(Rational::ZERO, Rational::new(1, 24).unwrap()).unwrap(),
            frame_rate: FrameRate::new(24, 1).unwrap(),
            region: ExportRegion {
                origin: [0.0, 0.0],
                extent: [64.0, 32.0],
                pixels: [8, 4],
            },
            profile: Default::default(),
            output: kronello_model::ExportOutput::ImageSequence,
            required_features: vec![],
        })
        .unwrap()]);
        let response = Service::new(BackendSelection::CpuReference).execute_json(
            &json!({"operation":"project.create","project":project,"document":document})
                .to_string(),
        );
        let response = serde_json::to_value(response).unwrap();
        assert_eq!(response["status"], "success", "{response}");
        Self {
            _cleanup: kronello_jobs::test_support::WorkerCleanup::new(&state).unwrap(),
            temp,
            project,
            state,
            watch_dir,
            output,
        }
    }
    fn watch(&self, extra: &[&str]) -> std::process::Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_kronello"));
        command
            .args(["--backend", "cpu-reference", "watch"])
            .args(extra)
            .env("KRONELLO_STATE_ROOT", &self.state);
        command.output().unwrap()
    }
    fn args(&self, preset: &str) -> Vec<String> {
        vec![
            "--project".into(),
            self.project.display().to_string(),
            "--directory".into(),
            self.watch_dir.display().to_string(),
            "--preset".into(),
            preset.into(),
            "--output".into(),
            self.output.display().to_string(),
            "--poll-ms".into(),
            "50".into(),
            "--once".into(),
        ]
    }
}
fn stdout_lines(output: &std::process::Output) -> Vec<Value> {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn watch_once_submits_stable_files_through_the_stored_preset() {
    let fixture = Fixture::new();
    std::fs::write(fixture.watch_dir.join("clip b.mov"), b"second").unwrap();
    std::fs::write(fixture.watch_dir.join("clip a.mov"), b"first").unwrap();
    std::fs::write(fixture.watch_dir.join(".hidden"), b"ignored").unwrap();
    let output = fixture.watch(
        &fixture
            .args("watch-out")
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let lines = stdout_lines(&output);
    // Two per-file submissions in lexical order, then the final response.
    let watch: Vec<_> = lines.iter().filter(|l| l.get("watch").is_some()).collect();
    assert_eq!(watch.len(), 2);
    assert!(watch.iter().all(|l| l["watch"] == "submitted"));
    assert!(watch[0]["file"].as_str().unwrap().ends_with("clip a.mov"));
    assert!(watch[1]["file"].as_str().unwrap().ends_with("clip b.mov"));
    assert!(watch.iter().all(|l| l["job"].is_string()));
    assert_eq!(lines.last().unwrap()["status"], "success");
    let jobs = JobStore::open(JobConfig::at(&fixture.state))
        .unwrap()
        .list()
        .unwrap();
    assert_eq!(jobs.len(), 2);
    // A second --once pass sees every file already stable and resubmits; the
    // destination names repeat, so items replay their recorded jobs.
    let again = fixture.watch(
        &fixture
            .args("watch-out")
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    assert!(
        again.status.success(),
        "{}",
        String::from_utf8_lossy(&again.stderr)
    );
    let watch: Vec<_> = stdout_lines(&again)
        .into_iter()
        .filter(|l| l.get("watch").is_some())
        .collect();
    assert!(watch.iter().all(|l| l["watch"] == "replayed"));
    assert_eq!(
        JobStore::open(JobConfig::at(&fixture.state))
            .unwrap()
            .list()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn watch_once_marks_failed_items_and_exits_nonzero() {
    let fixture = Fixture::new();
    let file = fixture.watch_dir.join("clip.mov");
    std::fs::write(&file, b"bytes").unwrap();
    // Preoccupy the deterministic destination name: <stem>-<hash12>.
    let hash = kronello_media::content_hash(&file).unwrap();
    std::fs::create_dir(fixture.output.join(format!("clip-{}", &hash[..12]))).unwrap();
    let output = fixture.watch(
        &fixture
            .args("watch-out")
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    assert!(!output.status.success());
    let lines = stdout_lines(&output);
    let failed = lines.iter().find(|l| l["watch"] == "failed").unwrap();
    assert_eq!(failed["error"]["code"], "OUTPUT_EXISTS");
    assert!(String::from_utf8_lossy(&output.stderr).contains("WATCH_SUBMIT_FAILED"));
}

#[test]
fn watch_rejects_bad_arguments_and_unknown_presets() {
    let fixture = Fixture::new();
    // Output inside the watched directory would feed results back as inputs.
    let mut nested = fixture.args("watch-out");
    nested[7] = fixture.watch_dir.join("out").display().to_string();
    let output = fixture.watch(&nested.iter().map(String::as_str).collect::<Vec<_>>());
    assert!(!output.status.success());
    let missing_preset = fixture.watch(
        &fixture
            .args("absent")
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    assert!(!missing_preset.status.success());
    assert!(String::from_utf8_lossy(&missing_preset.stderr).contains("PRESET_MISSING"));
    let missing_dir = fixture.watch(&[
        "--project",
        &fixture.project.display().to_string(),
        "--directory",
        &fixture.temp.path().join("absent").display().to_string(),
        "--preset",
        "watch-out",
        "--output",
        &fixture.output.display().to_string(),
        "--once",
    ]);
    assert!(!missing_dir.status.success());
    assert!(String::from_utf8_lossy(&missing_dir.stderr).contains("WATCH_DIRECTORY_MISSING"));
    // The preset id resolves exactly like its name.
    let response = Service::new(BackendSelection::CpuReference)
        .execute_json(&json!({"operation":"project.export","project":fixture.project}).to_string());
    let document = serde_json::to_value(response).unwrap();
    let preset_id = document["result"]["value"]["document"]["export_presets"][0]["id"]
        .as_str()
        .unwrap();
    std::fs::write(fixture.watch_dir.join("by-id.mov"), b"stable").unwrap();
    let by_id = fixture.watch(
        &fixture
            .args(preset_id)
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    assert!(
        by_id.status.success(),
        "{}",
        String::from_utf8_lossy(&by_id.stderr)
    );
    assert!(
        stdout_lines(&by_id).iter().any(
            |l| l["watch"] == "submitted" && l["file"].as_str().unwrap().ends_with("by-id.mov")
        )
    );
}

#[test]
fn watch_ignores_the_project_file_inside_the_watched_directory() {
    let temp = tempfile::tempdir().unwrap();
    let state = temp.path().join("state");
    let watch_dir = temp.path().join("in");
    let output = temp.path().join("out");
    std::fs::create_dir(&watch_dir).unwrap();
    std::fs::create_dir(&output).unwrap();
    // The project lives inside the watched directory; the scan must skip it.
    let project = watch_dir.join("watched.kronello");
    let mut document: Value =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    let composition = document["compositions"][0]["id"].clone();
    document["export_presets"] = json!([serde_json::to_value(ExportPreset {
        version: EXPORT_PRESET_VERSION,
        id: ExportPresetId::new(),
        name: "watch-out".into(),
        composition: serde_json::from_value(json!(composition)).unwrap(),
        target: None,
        range: TimeRange::new(Rational::ZERO, Rational::new(1, 24).unwrap()).unwrap(),
        frame_rate: FrameRate::new(24, 1).unwrap(),
        region: ExportRegion {
            origin: [0.0, 0.0],
            extent: [64.0, 32.0],
            pixels: [8, 4],
        },
        profile: Default::default(),
        output: kronello_model::ExportOutput::ImageSequence,
        required_features: vec![],
    })
    .unwrap()]);
    let _cleanup = kronello_jobs::test_support::WorkerCleanup::new(&state).unwrap();
    let response = Service::new(BackendSelection::CpuReference).execute_json(
        &json!({"operation":"project.create","project":project,"document":document}).to_string(),
    );
    assert_eq!(
        serde_json::to_value(&response).unwrap()["status"],
        "success"
    );
    let run = Command::new(env!("CARGO_BIN_EXE_kronello"))
        .args(["--backend", "cpu-reference", "watch"])
        .args([
            "--project",
            &project.display().to_string(),
            "--directory",
            &watch_dir.display().to_string(),
            "--preset",
            "watch-out",
            "--output",
            &output.display().to_string(),
            "--once",
        ])
        .env("KRONELLO_STATE_ROOT", &state)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let lines = stdout_lines(&run);
    // Nothing submitted: the only file present is the project itself.
    assert!(lines.iter().all(|l| l.get("watch").is_none()));
}
