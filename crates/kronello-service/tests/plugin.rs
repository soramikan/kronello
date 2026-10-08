//! AUDIO-011 service coverage (ADR-0131): `audio.plugin_probe` runs the
//! pinned describe exchange through the detached helper, `audio.plugin_process`
//! freezes a fixed-input job, and every failure class (missing bundle, hash
//! mismatch, malformed helper, bad spec, bad media input) is a typed error.
//! The real detached worker pipeline is exercised end-to-end in
//! `crates/kronello-cli/tests/plugin.rs`.
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use kronello_audio::{AudioBuffer, Bus, ClippingPolicy};
use kronello_jobs::{JobConfig, JobStore};
use kronello_media::{MediaRuntime, StreamKind, content_hash};
use kronello_model::*;
use kronello_plugin::test_support::{FIXTURE_CLASS_ID, build_fixture_bundle};
use kronello_plugin::{
    HelperCommand, PluginFormat, PluginParameter, PluginSpec, bundle_manifest_hash,
};
use kronello_service::*;

fn service() -> Service<'static> {
    Service::new(BackendSelection::CpuReference)
}
fn err_code(service: &Service<'_>, request: Request) -> String {
    service.dispatch(request).unwrap_err().code
}
/// Locate the standalone helper binary: `target/debug/kronello-plugin-host`
/// beside the test binary's `deps/` directory, building it on demand for
/// `cargo test -p kronello-service` runs where workspace bins are absent.
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
fn helper_command() -> HelperCommand {
    HelperCommand {
        program: host_binary().expect("kronello-plugin-host binary"),
        args: Vec::new(),
        envs: Vec::new(),
    }
}
static BUNDLE: OnceLock<PathBuf> = OnceLock::new();
fn bundle() -> PathBuf {
    BUNDLE
        .get_or_init(|| {
            let dir = std::env::temp_dir().join(format!(
                "kronello-service-plugin-test-{}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            build_fixture_bundle(&dir).expect("compile vst3 fixture")
        })
        .clone()
}
fn fixture_spec(parameters: &[(u32, f64)]) -> PluginSpec {
    let path = bundle();
    PluginSpec {
        format: PluginFormat::Vst3,
        sha256: Some(bundle_manifest_hash(&path).unwrap()),
        path: Some(path),
        component: FIXTURE_CLASS_ID.into(),
        version: Some("1.0.0".into()),
        parameters: parameters
            .iter()
            .map(|&(id, value)| PluginParameter { id, value })
            .collect(),
    }
}
/// 100 ms of stereo PCM24 `.mov`, registered as a locked audio asset.
fn audio_asset(dir: &Path, name: &str, id: AssetId) -> Asset {
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
            &Bus::new(0, AudioBuffer::new(frames).unwrap()),
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
    Asset {
        id,
        content_hash: content_hash(&path).unwrap(),
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
    }
}
/// Project with one locked audio asset; returns (dir, project path, asset).
fn project_with_audio() -> (tempfile::TempDir, PathBuf, Asset) {
    let dir = tempfile::tempdir().unwrap();
    let asset = audio_asset(dir.path(), "source.mov", AssetId::new());
    let mut document = Project::default();
    document.assets.push(DocumentObject::Known(asset.clone()));
    let path = dir.path().join("project.kronello");
    service()
        .dispatch(Request::ProjectCreate(CreateRequest {
            plan_hash: None,
            idempotency_key: None,
            project: path.clone(),
            document,
        }))
        .unwrap();
    (dir, path, asset)
}

#[test]
fn probe_reports_fixture_identity_through_detached_helper() {
    if host_binary().is_none() {
        eprintln!("skipping: kronello-plugin-host binary unavailable");
        return;
    }
    let service = service().with_plugin_helper(helper_command());
    let ResultData::PluginProbe(result) = service
        .dispatch(Request::AudioPluginProbe(PluginProbeRequest {
            plugin: fixture_spec(&[]),
            deadline_ms: Some(60_000),
        }))
        .unwrap()
    else {
        panic!("expected plugin probe result")
    };
    let report = result.report;
    assert_eq!(report.name, "Kronello Test Gain");
    assert_eq!(report.vendor, "Kronello");
    assert_eq!(report.version, "1.0.0");
    assert!(
        report
            .classes
            .iter()
            .any(|c| c.class_id == FIXTURE_CLASS_ID)
    );
}

#[test]
fn probe_failures_are_typed_before_and_after_spawn() {
    let spec = fixture_spec(&[]);
    let missing = HelperCommand {
        program: PathBuf::from("/definitely/missing/helper"),
        args: Vec::new(),
        envs: Vec::new(),
    };
    let probed = service().with_plugin_helper(missing);
    let probe = |plugin: PluginSpec| {
        probed
            .dispatch(Request::AudioPluginProbe(PluginProbeRequest {
                plugin,
                deadline_ms: None,
            }))
            .unwrap_err()
            .code
    };
    // The content pin is verified in this process before any helper spawn:
    // a broken helper path cannot mask an ASSET_HASH_MISMATCH.
    let mut pinned = spec.clone();
    pinned.sha256 = Some("f".repeat(64));
    assert_eq!(probe(pinned), "ASSET_HASH_MISMATCH");
    // A well-formed pin over a missing bundle is PLUGIN_MISSING; dropping
    // the required sha256 instead is INVALID_REQUEST.
    let mut gone = spec.clone();
    gone.path = Some(PathBuf::from("/definitely/missing/plugin.vst3"));
    assert_eq!(probe(gone.clone()), "PLUGIN_MISSING");
    gone.sha256 = None;
    assert_eq!(probe(gone), "INVALID_REQUEST");
    // Only after the pin verifies does the missing helper surface.
    assert_eq!(probe(spec.clone()), "PLUGIN_FAILED");
    // Malformed helper output is a protocol violation.
    let echo = service().with_plugin_helper(HelperCommand {
        program: PathBuf::from("/bin/echo"),
        args: vec!["hi".into()],
        envs: Vec::new(),
    });
    assert_eq!(
        echo.dispatch(Request::AudioPluginProbe(PluginProbeRequest {
            plugin: spec.clone(),
            deadline_ms: None,
        }))
        .unwrap_err()
        .code,
        "PLUGIN_PROTOCOL"
    );
    // Spec and deadline validation is typed and cheap.
    let mut bad = spec.clone();
    bad.component = "not-hex".into();
    assert_eq!(probe(bad), "INVALID_REQUEST");
    // VST3 without a bundle path is a missing-plugin error, not malformed.
    let mut vst3_no_path = spec.clone();
    vst3_no_path.path = None;
    assert_eq!(probe(vst3_no_path), "PLUGIN_MISSING");
    assert_eq!(
        service()
            .dispatch(Request::AudioPluginProbe(PluginProbeRequest {
                plugin: spec.clone(),
                deadline_ms: Some(10),
            }))
            .unwrap_err()
            .code,
        "INVALID_REQUEST"
    );
    // A remote locator for the bundle is rejected by request sanitation.
    let mut remote = spec.clone();
    remote.path = Some(PathBuf::from("https://example.invalid/plugin.vst3"));
    assert_eq!(probe(remote), "INVALID_REQUEST");
}

#[test]
fn process_submit_validates_request_project_and_asset() {
    let (dir, project, asset) = project_with_audio();
    let spec = fixture_spec(&[]);
    let destination = dir.path().join("out.mov");
    let request = |_service: &Service<'_>| PluginProcessRequest {
        project: project.clone(),
        expected_revision: None,
        asset: asset.id,
        stream_index: None,
        plugin: spec.clone(),
        destination: destination.clone(),
        deadline_ms: Some(60_000),
    };
    let stubbed = service()
        .with_job_config(JobConfig::at(dir.path().join("state")))
        .with_worker_executable(PathBuf::from("/usr/bin/false"));
    // Happy path: a queued fixed-input job with the pinned input as profile.
    let ResultData::Job(record) = stubbed
        .dispatch(Request::AudioPluginProcess(request(&stubbed)))
        .unwrap()
    else {
        panic!("expected job record")
    };
    assert_eq!(record.status, kronello_jobs::JobStatus::Queued);
    assert_eq!(record.destination, destination);
    assert!(record.total_frames >= 4_800);
    assert_eq!(
        record.output_profile["plugin"]["component"],
        FIXTURE_CLASS_ID
    );
    assert_eq!(record.output_profile["asset"]["id"], asset.id.to_string());
    // The fixed input file carries the plugin payload — and no bundle bytes.
    let store = JobStore::open(JobConfig::at(dir.path().join("state"))).unwrap();
    let fixed: serde_json::Value = serde_json::from_slice(&store.input(&record).unwrap()).unwrap();
    assert_eq!(fixed["plugin"]["plugin"]["component"], FIXTURE_CLASS_ID);
    assert_eq!(
        fixed["plugin"]["plugin"]["sha256"],
        serde_json::Value::String(spec.sha256.clone().unwrap())
    );
    assert!(fixed.get("snapshot").is_none() && fixed.get("proxy").is_none());

    // Typed submit rejections — none of them spawns a worker.
    let unconfigured = service(); // no job store at all still validates first
    let mut bad = request(&unconfigured);
    bad.destination = dir.path().join("out.wav");
    assert_eq!(
        err_code(&unconfigured, Request::AudioPluginProcess(bad)),
        "INVALID_MEDIA_INPUT"
    );
    let mut bad = request(&unconfigured);
    bad.project = dir.path().join("missing.kronello");
    assert_eq!(
        err_code(&unconfigured, Request::AudioPluginProcess(bad)),
        "PROJECT_NOT_FOUND"
    );
    let mut bad = request(&unconfigured);
    bad.asset = AssetId::new();
    assert_eq!(
        err_code(&unconfigured, Request::AudioPluginProcess(bad)),
        "ASSET_MISSING"
    );
    let mut bad = request(&unconfigured);
    bad.stream_index = Some(99);
    assert_eq!(
        err_code(&unconfigured, Request::AudioPluginProcess(bad)),
        "INVALID_MEDIA_INPUT"
    );
    let mut bad = request(&unconfigured);
    bad.plugin.sha256 = Some("f".repeat(64));
    assert_eq!(
        err_code(&unconfigured, Request::AudioPluginProcess(bad)),
        "ASSET_HASH_MISMATCH"
    );
    let mut bad = request(&unconfigured);
    bad.expected_revision = Some("999".into());
    assert_eq!(
        err_code(&unconfigured, Request::AudioPluginProcess(bad)),
        "REVISION_CONFLICT"
    );
    // Existing destination and wrong asset kind are typed as well.
    std::fs::write(dir.path().join("taken.mov"), b"x").unwrap();
    let mut bad = request(&unconfigured);
    bad.destination = dir.path().join("taken.mov");
    assert_eq!(
        err_code(&unconfigured, Request::AudioPluginProcess(bad)),
        "OUTPUT_EXISTS"
    );
}

#[test]
fn process_submit_rejects_non_audio_streams_and_kinds() {
    let dir = tempfile::tempdir().unwrap();
    // Image asset: wrong kind entirely.
    let image = Asset {
        id: AssetId::new(),
        content_hash: "a".repeat(64),
        kind: AssetKind::Image,
        streams: vec![],
        locator: AssetLocator {
            relative: Some("frame.png".into()),
            absolute: None,
        },
    };
    let mut document = Project::default();
    document.assets.push(DocumentObject::Known(image.clone()));
    let path = dir.path().join("project.kronello");
    service()
        .dispatch(Request::ProjectCreate(CreateRequest {
            plan_hash: None,
            idempotency_key: None,
            project: path.clone(),
            document,
        }))
        .unwrap();
    let spec = fixture_spec(&[]);
    assert_eq!(
        err_code(
            &service(),
            Request::AudioPluginProcess(PluginProcessRequest {
                project: path.clone(),
                expected_revision: None,
                asset: image.id,
                stream_index: None,
                plugin: spec.clone(),
                destination: dir.path().join("out.mov"),
                deadline_ms: None,
            })
        ),
        "INVALID_MEDIA_INPUT"
    );
    // Asset whose only stream is video has no default audio stream.
    let video = Asset {
        id: AssetId::new(),
        content_hash: "b".repeat(64),
        kind: AssetKind::Video,
        streams: vec![StreamMetadata {
            index: 0,
            codec: "prores".into(),
            time_base: kronello_time::Rational::new(1, 24).unwrap(),
            duration: None,
            start_time: None,
            width: Some(64),
            height: Some(32),
            pixel_format: Some("yuv422p10le".into()),
            color_primaries: None,
            color_transfer: None,
            color_matrix: None,
            color_range: None,
        }],
        locator: AssetLocator {
            relative: Some("clip.mov".into()),
            absolute: None,
        },
    };
    let mut document = Project::default();
    document.assets.push(DocumentObject::Known(video.clone()));
    let path2 = dir.path().join("video.kronello");
    service()
        .dispatch(Request::ProjectCreate(CreateRequest {
            plan_hash: None,
            idempotency_key: None,
            project: path2.clone(),
            document,
        }))
        .unwrap();
    for stream_index in [None, Some(0)] {
        assert_eq!(
            err_code(
                &service(),
                Request::AudioPluginProcess(PluginProcessRequest {
                    project: path2.clone(),
                    expected_revision: None,
                    asset: video.id,
                    stream_index,
                    plugin: spec.clone(),
                    destination: dir.path().join(format!("v{stream_index:?}.mov")),
                    deadline_ms: None,
                })
            ),
            "INVALID_MEDIA_INPUT"
        );
    }
}
