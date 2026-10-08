//! AI-002 (ADR-0125) end-to-end: a real `scene.detect` worker decodes a
//! two-scene clip, publishes a boundary receipt, registers the versioned
//! asset, and `scene.apply` marks the sequence through the shared edit path.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use kronello_jobs::{JobConfig, JobStatus, JobStore};
use kronello_media::{EncodeCodec, EncodeFrame, EncodeRequest, MediaRuntime, content_hash};
use kronello_model::{
    Asset, AssetId, AssetKind, AssetLocator, Clip, ClipId, DesignExtent, DocumentObject, Project,
    Sequence, SequenceId, SourceRef, Track, TrackId, TrackKind,
};
use kronello_time::{FrameRate, Rational, SampleRate, Time, TimeMap, TimeRange};
use serde_json::{Value as Json, json};

fn r(n: i64, d: i64) -> Rational {
    Rational::new(n, d).unwrap()
}

/// Two-scene ProRes clip at 1/24: frames 0..4 dark, frames 4..8 bright.
fn write_clip(path: &Path) {
    MediaRuntime::load()
        .unwrap()
        .encode_video_stream(
            &EncodeRequest {
                output: path.into(),
                codec: EncodeCodec::ProRes,
                width: 32,
                height: 24,
                time_base: r(1, 24),
            },
            8,
            &mut |index| {
                let mut rgba = vec![0u8; 32 * 24 * 4];
                // Scene A is a uniform dark field; scene B is deterministic
                // noise, so the cut fires both histogram and edge scores.
                for (p, pixel) in rgba.chunks_exact_mut(4).enumerate() {
                    let (x, y) = (p as u32 % 32, p as u32 / 32);
                    let value = if index < 4 {
                        16u8
                    } else {
                        ((x * 37 + y * 91 + index as u32 * 11) % 256) as u8
                    };
                    pixel.copy_from_slice(&[value, value, value, 255]);
                }
                Ok(EncodeFrame {
                    pts: r(index as i64, 24),
                    rgba,
                })
            },
        )
        .unwrap();
}

struct Fixture {
    _cleanup: kronello_jobs::test_support::WorkerCleanup,
    _tempdir: tempfile::TempDir,
    project: PathBuf,
    state: PathBuf,
    asset: AssetId,
    sequence: SequenceId,
}
impl Fixture {
    fn new() -> Self {
        let tempdir = tempfile::tempdir().unwrap();
        let state = tempdir.path().join("state");
        let project = tempdir.path().join("clip.kronello");
        let asset = AssetId::new();
        let sequence = SequenceId::new();
        write_clip(&tempdir.path().join("clip.mov"));
        let metadata = MediaRuntime::load()
            .unwrap()
            .open_video_stream(&tempdir.path().join("clip.mov"), 0)
            .unwrap()
            .stream_metadata()
            .unwrap();
        let mut document = Project::default();
        document.assets.push(DocumentObject::Known(Asset {
            id: asset,
            content_hash: content_hash(&tempdir.path().join("clip.mov")).unwrap(),
            kind: AssetKind::Video,
            streams: vec![metadata],
            locator: AssetLocator {
                relative: Some("clip.mov".into()),
                absolute: None,
            },
        }));
        document.sequences.push(DocumentObject::Known(Sequence {
            id: sequence,
            extent: DesignExtent::new(32.0, 24.0).unwrap(),
            frame_rate: FrameRate::new(24, 1).unwrap(),
            audio_rate: SampleRate::HZ_48000,
            working_space: kronello_model::ColorSpace::LinearRec709,
            tracks: vec![Track {
                state: None,
                id: TrackId::new(),
                kind: TrackKind::Video,
                clips: vec![Clip {
                    id: ClipId::new(),
                    source_ref: SourceRef::Asset {
                        asset,
                        stream_index: 0,
                    },
                    timeline_range: TimeRange::new(Time::ZERO, r(8, 24)).unwrap(),
                    source_in: Time::ZERO,
                    time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
                    enabled: true,
                    audio_retime: kronello_model::AudioRetimePolicy::Reject,
                    reverse_sampling: None,
                    volume: None,
                    links: vec![],
                    effects: vec![],
                    masks: vec![],
                    properties: vec![],
                    markers: vec![],
                }],
            }],
            transitions: vec![],
            markers: vec![],
            work_area: None,
            targets: None,
        }));
        let fixture = Self {
            _cleanup: kronello_jobs::test_support::WorkerCleanup::new(&state).unwrap(),
            _tempdir: tempdir,
            project,
            state,
            asset,
            sequence,
        };
        fixture.cli(
            json!({"operation":"project.create","project":fixture.project,
                "document":serde_json::to_value(&document).unwrap()}),
        );
        fixture
    }
    fn command(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_kronello"));
        c.args(["--backend", "cpu-reference"])
            .env("KRONELLO_STATE_ROOT", &self.state)
            .env("KRONELLO_JOB_HEARTBEAT_MS", "50")
            .env("KRONELLO_JOB_TIMEOUT_MS", "30000");
        c
    }
    fn cli(&self, request: Json) -> Json {
        self.cli_args(&[], request)
    }
    /// CLI verb tokens inject the operation tag; payloads stay untagged.
    fn cli_args(&self, verbs: &[&str], request: Json) -> Json {
        let mut command = self.command();
        command.args(verbs);
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(request.to_string().as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        if !output.status.success() {
            panic!(
                "cli failed verbs={verbs:?} request={request} stderr={}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let response: Json = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            response["status"], "success",
            "verbs={verbs:?} request={request} response={response}"
        );
        response["result"]["value"].clone()
    }
    fn wait(&self, id: &str, status: JobStatus) -> kronello_jobs::JobRecord {
        let store = JobStore::open(JobConfig::at(&self.state)).unwrap();
        let start = Instant::now();
        loop {
            let record = store.get(id).unwrap();
            if record.status == status {
                return record;
            }
            assert!(
                start.elapsed() < Duration::from_secs(60),
                "timed out: {record:?}; log: {}",
                std::fs::read_to_string(store.directory(id).unwrap().join("worker.log"))
                    .unwrap_or_default()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn export(&self) -> Json {
        self.cli(json!({"operation":"project.export","project":self.project}))
    }
}

#[test]
fn scene_detect_publishes_boundary_asset_and_apply_marks_sequence() {
    let fixture = Fixture::new();
    // `kronello scene detect` submits the fixed-input detection job.
    let record = fixture.cli_args(
        &["scene", "detect"],
        json!({"project": fixture.project, "asset": fixture.asset, "stream_index": 0}),
    );
    let record = fixture.wait(record["id"].as_str().unwrap(), JobStatus::Succeeded);
    // The managed `<stem>.scene/<asset>.json` receipt is the boundary asset.
    assert_eq!(record.destination.extension().unwrap(), "json");
    assert_eq!(
        record.destination.parent().unwrap().file_name().unwrap(),
        "clip.scene"
    );
    let receipt: Json =
        serde_json::from_slice(&std::fs::read(&record.destination).unwrap()).unwrap();
    assert_eq!(receipt["frames_analyzed"], 8);
    let boundaries = receipt["boundaries"].as_array().unwrap();
    assert_eq!(boundaries.len(), 1, "{receipt}");
    // The dark->bright cut lands on the first bright frame: source time 4/24.
    assert_eq!(boundaries[0]["time"], json!({"num": "1", "den": "6"}));
    let confidence = boundaries[0]["confidence"].as_f64().unwrap();
    assert!(confidence > 0.5, "cut confidence {confidence}");
    // The worker registered the versioned asset through the store.
    let exported = fixture.export();
    let scene_assets = exported["document"]["scene_boundary_assets"]
        .as_array()
        .unwrap();
    assert_eq!(scene_assets.len(), 1);
    assert_eq!(
        scene_assets[0]["content_hash"], receipt["content_hash"],
        "document asset matches the published receipt"
    );
    let scene_asset = scene_assets[0]["id"].clone();
    // `job.resume` re-validates the published result without rerunning.
    let resumed = fixture.cli(json!({"operation":"job.resume","job":record.id}));
    assert_eq!(resumed["status"], "succeeded", "{resumed}");
    // `kronello scene apply` maps the boundary to a sequence marker.
    let base_revision = exported["revision"].clone();
    let edit = fixture.cli_args(
        &["scene", "apply"],
        json!({
            "project": fixture.project,
            "base_revision": base_revision,
            "idempotency_key": "scene-apply-1",
            "session_id": "96607679-eefd-407a-a3a8-59943f2bd82f",
            "scene_asset": scene_asset,
            "sequence": fixture.sequence,
            "mode": "markers",
        }),
    );
    assert!(edit["id"].is_string(), "{edit}");
    let exported = fixture.export();
    let markers = exported["document"]["sequences"][0]["markers"]
        .as_array()
        .unwrap();
    assert_eq!(markers.len(), 1, "{exported}");
    // source_in 0: the 4/24 source cut maps to sequence time 4/24 = 1/6.
    assert_eq!(markers[0]["time"], json!({"num": "1", "den": "6"}));
    assert!(
        markers[0]["comment"]
            .as_str()
            .unwrap_or_default()
            .starts_with("scene boundary")
    );
    // The same idempotent retry replays the committed event even though the
    // project revision moved on.
    let replay = fixture.cli_args(
        &["scene", "apply"],
        json!({
            "project": fixture.project,
            "base_revision": base_revision,
            "idempotency_key": "scene-apply-1",
            "session_id": "96607679-eefd-407a-a3a8-59943f2bd82f",
            "scene_asset": scene_asset,
            "sequence": fixture.sequence,
            "mode": "markers",
        }),
    );
    assert_eq!(replay["id"], edit["id"]);
    assert_eq!(replay["revision"], edit["revision"]);
}
