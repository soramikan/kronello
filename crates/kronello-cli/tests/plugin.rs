//! AUDIO-011 end-to-end (ADR-0131): `kronello audio plugin_probe` and
//! `audio plugin_process` through the real CLI binary — the detached worker
//! resolves the plugin helper, loads the deterministic VST3 fixture inside
//! that helper process only, and publishes a PCM24 `.mov` with a receipt.
//! Project bytes are never touched and bundle bytes never enter project
//! state.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use kronello_audio::{AudioBuffer, Bus, ClippingPolicy};
use kronello_jobs::{JobConfig, JobRecord, JobStatus, JobStore};
use kronello_media::{MediaRuntime, StreamKind, content_hash};
use kronello_model::*;
use kronello_plugin::test_support::{FIXTURE_CLASS_ID, build_fixture_bundle};
use kronello_plugin::{HELPER_PATH_ENV, HelperRequest, bundle_manifest_hash};
use kronello_service::{BackendSelection, Service};
use serde_json::{Value, json};

fn kronello() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kronello"))
}
/// `target/debug/kronello-plugin-host` next to this test binary's deps dir,
/// built on demand for `cargo test -p kronello-cli` runs.
fn host_binary() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let target_dir = exe.parent()?.parent()?;
    let binary = target_dir.join(format!(
        "kronello-plugin-host{}",
        std::env::consts::EXE_SUFFIX
    ));
    if binary.is_file() {
        return Some(binary);
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let status =
        std::process::Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
            .args([
                "build",
                "-p",
                "kronello-plugin",
                "--bin",
                "kronello-plugin-host",
            ])
            .current_dir(root)
            .status()
            .ok()?;
    (status.success() && binary.is_file()).then_some(binary)
}
static BUNDLE: OnceLock<PathBuf> = OnceLock::new();
fn bundle() -> PathBuf {
    BUNDLE
        .get_or_init(|| {
            let dir = std::env::temp_dir()
                .join(format!("kronello-cli-plugin-test-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            build_fixture_bundle(&dir).expect("compile vst3 fixture")
        })
        .clone()
}
fn fixture_spec(parameters: &[(u32, f64)]) -> Value {
    let path = bundle();
    json!({
        "format": "vst3",
        "path": path,
        "sha256": bundle_manifest_hash(&path).unwrap(),
        "component": FIXTURE_CLASS_ID,
        "version": "1.0.0",
        "parameters": parameters.iter().map(|&(id, value)| json!({"id": id, "value": value})).collect::<Vec<_>>(),
    })
}
/// 100 ms of deterministic stereo PCM24 audio registered as a locked asset.
fn audio_asset(dir: &Path, name: &str, id: AssetId) -> (PathBuf, Asset, Vec<[f32; 2]>) {
    let path = dir.join(name);
    let frames: Vec<[f32; 2]> = (0..4_800)
        .map(|i| {
            let s = (((i % 480) as f32 / 480.0) * 2.0 - 1.0) * 0.5;
            [s, s]
        })
        .collect();
    let runtime = MediaRuntime::load().unwrap();
    runtime
        .encode_audio(
            &Bus::new(0, AudioBuffer::new(frames.clone()).unwrap()),
            ClippingPolicy::Reject,
            &path,
        )
        .unwrap();
    let probe = runtime.probe(&path).unwrap();
    let stream = probe
        .streams
        .iter()
        .find(|s| s.kind == StreamKind::Audio)
        .expect("audio stream");
    let hash = content_hash(&path).unwrap();
    (
        path,
        Asset {
            id,
            content_hash: hash,
            kind: AssetKind::Audio,
            streams: vec![StreamMetadata {
                index: stream.index,
                codec: stream.codec.clone(),
                time_base: stream.time_base,
                duration: stream.duration,
                start_time: stream.start,
                width: None,
                height: None,
                pixel_format: None,
                color_primaries: None,
                color_transfer: None,
                color_matrix: None,
                color_range: None,
            }],
            locator: AssetLocator {
                relative: Some(name.into()),
                absolute: None,
            },
        },
        frames,
    )
}
struct Fixture {
    cleanup: kronello_jobs::test_support::WorkerCleanup,
    _temp: tempfile::TempDir,
    project: PathBuf,
    state: PathBuf,
    asset: Asset,
    frames: Vec<[f32; 2]>,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("state");
        let cleanup = kronello_jobs::test_support::WorkerCleanup::new(&state).unwrap();
        let id = AssetId::new();
        let (_path, asset, frames) = audio_asset(temp.path(), "source.mov", id);
        let mut document = Project::default();
        document.assets.push(DocumentObject::Known(asset.clone()));
        let project = temp.path().join("project.kronello");
        let response = Service::new(BackendSelection::CpuReference).execute_json(
            &json!({"operation":"project.create","project":project,"document":document})
                .to_string(),
        );
        assert_eq!(
            serde_json::to_value(&response).unwrap()["status"],
            "success"
        );
        Self {
            cleanup,
            _temp: temp,
            project,
            state,
            asset,
            frames,
        }
    }
    /// Spawn the real CLI: argv selects the operation, stdin carries the
    /// untagged request object. The helper path is pinned through the
    /// environment so the detached worker inherits it.
    fn cli(&self, argv: &[&str], body: Value, envs: &[(&str, &str)]) -> Value {
        let mut command = kronello();
        command
            .arg("--backend")
            .arg("cpu-reference")
            .args(argv)
            .env("KRONELLO_STATE_ROOT", &self.state)
            .env("KRONELLO_JOB_HEARTBEAT_MS", "50")
            .env("KRONELLO_JOB_TIMEOUT_MS", "30000")
            .env_remove("KRONELLO_PLUGIN_FIXTURE_CRASH")
            .env_remove("KRONELLO_PLUGIN_FIXTURE_HANG_MS")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for &(key, value) in envs {
            command.env(key, value);
        }
        let mut child = command.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(body.to_string().as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        self.cleanup.capture_registered().unwrap();
        let response: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
            panic!(
                "CLI output is not a response ({e}): stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        jsonschema::validator_for(&kronello_service::api_json_schema())
            .unwrap()
            .validate(&response)
            .unwrap();
        response
    }
    fn store(&self) -> JobStore {
        let mut config = JobConfig::at(&self.state);
        config.heartbeat_interval = Duration::from_millis(50);
        config.heartbeat_timeout = Duration::from_secs(30);
        JobStore::open(config).unwrap()
    }
    fn wait(&self, id: &str) -> JobRecord {
        let start = Instant::now();
        loop {
            let record = self.store().get(id).unwrap();
            if !record.status.active() {
                return record;
            }
            assert!(
                start.elapsed() < Duration::from_secs(60),
                "timed out: {record:?}; log: {}",
                std::fs::read_to_string(self.store().directory(id).unwrap().join("worker.log"))
                    .unwrap_or_default()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

#[test]
fn plugin_helper_reentry_speaks_the_protocol() {
    // `kronello plugin-helper` is the third helper resolution leg and the
    // only path that loads plugin code inside this binary family.
    let mut child = kronello()
        .arg("plugin-helper")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let spec = fixture_spec(&[]);
    let request = serde_json::to_vec(&HelperRequest::describe(
        serde_json::from_value(spec).unwrap(),
        30_000,
    ))
    .unwrap();
    child.stdin.take().unwrap().write_all(&request).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "helper failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "ok");
    assert_eq!(response["report"]["name"], "Kronello Test Gain");
}

#[test]
fn plugin_probe_and_process_run_end_to_end() {
    let fixture = Fixture::new();
    let spec = fixture_spec(&[(0, 0.5)]);
    let helper = host_binary();
    let envs: Vec<(&str, &str)> = helper
        .as_deref()
        .map(|p| vec![(HELPER_PATH_ENV, p.to_str().unwrap())])
        .unwrap_or_default();
    // Probe: one bounded describe exchange through the detached helper.
    let probe = fixture.cli(&["audio", "plugin_probe"], json!({"plugin": spec}), &envs);
    assert_eq!(probe["status"], "success", "{probe}");
    assert_eq!(
        probe["result"]["value"]["report"]["name"],
        "Kronello Test Gain"
    );
    assert_eq!(probe["result"]["value"]["report"]["vendor"], "Kronello");

    // Process: submit a fixed-input job; the detached worker decodes the
    // locked asset, hands it to the helper, and publishes the receipted .mov.
    let destination = fixture._temp.path().join("processed.mov");
    let before = std::fs::read(&fixture.project).unwrap();
    let submitted = fixture.cli(
        &["audio", "plugin_process"],
        json!({
            "project": fixture.project,
            "asset": fixture.asset.id,
            "plugin": spec,
            "destination": destination,
        }),
        &envs,
    );
    assert_eq!(submitted["status"], "success", "{submitted}");
    let id = submitted["result"]["value"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let record = fixture.wait(&id);
    assert_eq!(
        record.status,
        JobStatus::Succeeded,
        "job failed: {:?}; log: {}",
        record.error,
        std::fs::read_to_string(fixture.store().directory(&id).unwrap().join("worker.log"))
            .unwrap_or_default()
    );
    // Receipted publication: destination exists, report validates, and the
    // project file is byte-identical (plugin processing mutates nothing).
    assert!(destination.is_file());
    let result = record.result.unwrap();
    assert_eq!(result["validated"], true);
    assert_eq!(result["report"]["plugin"]["component"], FIXTURE_CLASS_ID);
    assert_eq!(result["report"]["plugin"]["name"], "Kronello Test Gain");
    assert_eq!(result["report"]["frames"], 4_800);
    assert_eq!(std::fs::read(&fixture.project).unwrap(), before);
    // The published movie decodes to the fixture's 0.5 gain output.
    let runtime = MediaRuntime::load().unwrap();
    let decoded = runtime.decode_audio(&destination, 0).unwrap();
    assert_eq!(decoded.buffer.frames().len(), fixture.frames.len());
    for (index, (got, input)) in decoded
        .buffer
        .frames()
        .iter()
        .zip(&fixture.frames)
        .enumerate()
    {
        for ch in 0..2 {
            // PCM24 quantization + f32→pcm roundtrip slack.
            assert!(
                (got[ch] - input[ch] * 0.5).abs() < 1e-4,
                "frame {index} ch {ch}: got {} want {}",
                got[ch],
                input[ch] * 0.5
            );
        }
    }
    // The project export carries asset metadata only — no plugin bytes and
    // no plugin-specific document entries.
    let export = fixture.cli(
        &["project", "export"],
        json!({"project": fixture.project}),
        &[],
    );
    let exported = export["result"]["value"]["document"].to_string();
    assert!(
        !exported.contains("plugin"),
        "document holds no plugin data"
    );
    let stored = std::fs::read(&fixture.project).unwrap();
    assert!(
        !stored.windows(6).any(|w| w == b"plugin"),
        "project file holds no plugin data"
    );
}

#[test]
fn plugin_failures_are_typed_through_the_job() {
    let fixture = Fixture::new();
    let helper = host_binary().expect("kronello-plugin-host binary");
    let envs = [(HELPER_PATH_ENV, helper.to_str().unwrap())];
    // Hash mismatches are rejected at submit before any helper spawn.
    let mut bad = fixture_spec(&[]);
    bad["sha256"] = json!("f".repeat(64));
    let response = fixture.cli(
        &["audio", "plugin_process"],
        json!({
            "project": fixture.project,
            "asset": fixture.asset.id,
            "plugin": bad,
            "destination": fixture._temp.path().join("bad.mov"),
        }),
        &envs,
    );
    assert_eq!(response["status"], "error");
    assert_eq!(response["error"]["code"], "ASSET_HASH_MISMATCH");
    // A crashing plugin fails the job as PLUGIN_FAILED — the worker and the
    // service survive and the destination is never published.
    let destination = fixture._temp.path().join("crashed.mov");
    let submitted = fixture.cli(
        &["audio", "plugin_process"],
        json!({
            "project": fixture.project,
            "asset": fixture.asset.id,
            "plugin": fixture_spec(&[]),
            "destination": destination,
            "deadline_ms": 30_000,
        }),
        &[
            (HELPER_PATH_ENV, helper.to_str().unwrap()),
            ("KRONELLO_PLUGIN_FIXTURE_CRASH", "1"),
        ],
    );
    assert_eq!(submitted["status"], "success", "{submitted}");
    let id = submitted["result"]["value"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let record = fixture.wait(&id);
    assert_eq!(record.status, JobStatus::Failed);
    assert_eq!(record.error.unwrap().code, "PLUGIN_FAILED");
    assert!(!destination.exists());
}
