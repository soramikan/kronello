//! AUDIO-011 host-boundary tests: deterministic fixture plugin through the
//! real detached helper binary — describe/process/unload, crash, timeout,
//! missing bundle, hash mismatch, unsupported ABI and protocol violations.
//! Runs only with `--features test-support`.
#![cfg(feature = "test-support")]
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use kronello_plugin::test_support::{FIXTURE_CLASS_ID, build_fixture_bundle};
use kronello_plugin::*;

fn helper_command() -> HelperCommand {
    HelperCommand {
        program: PathBuf::from(env!("CARGO_BIN_EXE_kronello-plugin-host")),
        args: Vec::new(),
        envs: Vec::new(),
    }
}
static BUNDLE: OnceLock<PathBuf> = OnceLock::new();
fn bundle() -> PathBuf {
    BUNDLE
        .get_or_init(|| {
            let dir =
                std::env::temp_dir().join(format!("kronello-plugin-test-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            build_fixture_bundle(&dir).expect("compile vst3 fixture")
        })
        .clone()
}
fn fixture_spec(params: &[(u32, f64)]) -> PluginSpec {
    let path = bundle();
    let sha256 = bundle_manifest_hash(&path).expect("manifest hash");
    PluginSpec {
        format: PluginFormat::Vst3,
        path: Some(path),
        sha256: Some(sha256),
        component: FIXTURE_CLASS_ID.into(),
        version: Some("1.0.0".into()),
        parameters: params
            .iter()
            .map(|&(id, value)| PluginParameter { id, value })
            .collect(),
    }
}
fn ramp(frames: usize, channels: usize) -> Vec<f32> {
    (0..frames * channels)
        .map(|i| ((i % 977) as f32 / 977.0) * 2.0 - 1.0)
        .collect()
}
fn write_f32(path: &Path, data: &[f32]) {
    let mut bytes = Vec::with_capacity(data.len() * 4);
    for s in data {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, bytes).unwrap();
}
fn read_f32(path: &Path) -> Vec<f32> {
    std::fs::read(path)
        .unwrap()
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
        .collect()
}
fn io_pair(dir: &Path, frames: u64) -> HelperIo {
    HelperIo {
        input: dir.join("in.f32le"),
        output: dir.join("out.f32le"),
        frames,
        sample_rate: 48_000,
        channels: 2,
    }
}

#[test]
fn fixture_describe_reports_class_and_identity() {
    let spec = fixture_spec(&[]);
    let response = run_helper(
        &helper_command(),
        &HelperRequest::describe(spec.clone(), 30_000),
        Duration::from_secs(30),
    )
    .expect("describe");
    let report = response.report.expect("report");
    assert_eq!(report.name, "Kronello Test Gain");
    assert_eq!(report.vendor, "Kronello");
    assert_eq!(report.version, "1.0.0");
    assert!(
        report
            .classes
            .iter()
            .any(|c| c.class_id == FIXTURE_CLASS_ID && c.name == "Kronello Test Gain")
    );
}

#[test]
fn fixture_process_roundtrip_and_unload() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("lifecycle.log");
    let spec = fixture_spec(&[(0, 0.5)]);
    let io = io_pair(dir.path(), 5000);
    let input = ramp(5000, 2);
    write_f32(&io.input, &input);
    let command = helper_command().with_env("KRONELLO_PLUGIN_FIXTURE_LOG", log.to_str().unwrap());
    let response = run_helper(
        &command,
        &HelperRequest::process(spec, io.clone(), 30_000),
        Duration::from_secs(30),
    )
    .expect("process");
    assert_eq!(response.frames, Some(5000));
    let output = read_f32(&io.output);
    assert_eq!(output.len(), input.len());
    eprintln!("in[0..4]={:?} out[0..4]={:?}", &input[..4], &output[..4]);
    eprintln!("io={io:?}");
    for (got, want) in output.iter().zip(&input) {
        assert_eq!(*got, want * 0.5, "gain parameter not applied");
    }
    // Unload ordering: ModuleExit marker must appear after process.
    let marks = std::fs::read_to_string(&log).unwrap();
    let lines: Vec<&str> = marks.lines().collect();
    assert_eq!(lines.first().copied(), Some("module_entry"));
    assert_eq!(lines.last().copied(), Some("module_exit"));
    assert!(lines.contains(&"process"));
}

#[test]
fn fixture_crash_is_typed_plugin_failed() {
    let dir = tempfile::tempdir().unwrap();
    let spec = fixture_spec(&[]);
    let io = io_pair(dir.path(), 1024);
    write_f32(&io.input, &ramp(1024, 2));
    let command = helper_command().with_env("KRONELLO_PLUGIN_FIXTURE_CRASH", "1");
    let err = run_helper(
        &command,
        &HelperRequest::process(spec, io, 30_000),
        Duration::from_secs(30),
    )
    .unwrap_err();
    assert_eq!(err.code(), "PLUGIN_FAILED");
}

#[test]
fn fixture_hang_killed_by_worker_timeout() {
    let dir = tempfile::tempdir().unwrap();
    let spec = fixture_spec(&[]);
    let io = io_pair(dir.path(), 1024);
    write_f32(&io.input, &ramp(1024, 2));
    // Helper watchdog stays above the worker timeout so run_helper's kill
    // path is what fires → PLUGIN_TIMEOUT.
    let command = helper_command().with_env("KRONELLO_PLUGIN_FIXTURE_HANG_MS", "30000");
    let err = run_helper(
        &command,
        &HelperRequest::process(spec, io, 60_000),
        Duration::from_millis(2_000),
    )
    .unwrap_err();
    assert_eq!(err.code(), "PLUGIN_TIMEOUT");
}

#[test]
fn fixture_hang_killed_by_helper_watchdog() {
    let dir = tempfile::tempdir().unwrap();
    let spec = fixture_spec(&[]);
    let io = io_pair(dir.path(), 1024);
    write_f32(&io.input, &ramp(1024, 2));
    let command = helper_command().with_env("KRONELLO_PLUGIN_FIXTURE_HANG_MS", "30000");
    // Helper watchdog at 1.5 s aborts the helper before the 30 s worker cap.
    let err = run_helper(
        &command,
        &HelperRequest::process(spec, io, 1_500),
        Duration::from_secs(30),
    )
    .unwrap_err();
    assert_eq!(err.code(), "PLUGIN_FAILED");
}

#[test]
fn missing_bundle_is_typed() {
    let spec = PluginSpec {
        format: PluginFormat::Vst3,
        path: Some(PathBuf::from("/definitely/missing/bundle.vst3")),
        sha256: Some("0".repeat(64)),
        component: FIXTURE_CLASS_ID.into(),
        version: None,
        parameters: Vec::new(),
    };
    let err = run_helper(
        &helper_command(),
        &HelperRequest::describe(spec, 30_000),
        Duration::from_secs(30),
    )
    .unwrap_err();
    assert_eq!(err.code(), "PLUGIN_MISSING");
}

#[test]
fn hash_mismatch_is_typed() {
    let mut spec = fixture_spec(&[]);
    spec.sha256 = Some("f".repeat(64));
    let err = run_helper(
        &helper_command(),
        &HelperRequest::describe(spec, 30_000),
        Duration::from_secs(30),
    )
    .unwrap_err();
    assert_eq!(err.code(), "ASSET_HASH_MISMATCH");
}

#[test]
fn unknown_class_is_typed() {
    let mut spec = fixture_spec(&[]);
    spec.component = "00".repeat(16);
    let err = run_helper(
        &helper_command(),
        &HelperRequest::describe(spec, 30_000),
        Duration::from_secs(30),
    )
    .unwrap_err();
    assert_eq!(err.code(), "PLUGIN_MISSING");
}

#[test]
fn non_module_file_is_unsupported_abi() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("not-a-plugin.vst3");
    std::fs::write(&file, b"this is not a loadable image").unwrap();
    let spec = PluginSpec {
        format: PluginFormat::Vst3,
        path: Some(file.clone()),
        sha256: Some(bundle_manifest_hash(&file).unwrap()),
        component: FIXTURE_CLASS_ID.into(),
        version: None,
        parameters: Vec::new(),
    };
    let err = run_helper(
        &helper_command(),
        &HelperRequest::describe(spec, 30_000),
        Duration::from_secs(30),
    )
    .unwrap_err();
    assert_eq!(err.code(), "UNSUPPORTED_FEATURE");
}

#[test]
fn malformed_helper_output_is_protocol_error() {
    // /bin/echo exits 0 with non-JSON output → PLUGIN_PROTOCOL.
    let command = HelperCommand {
        program: PathBuf::from("/bin/echo"),
        args: vec!["hi".into()],
        envs: Vec::new(),
    };
    let spec = fixture_spec(&[]);
    let err = run_helper(
        &command,
        &HelperRequest::describe(spec, 30_000),
        Duration::from_secs(30),
    )
    .unwrap_err();
    assert_eq!(err.code(), "PLUGIN_PROTOCOL");
}

#[test]
fn missing_helper_binary_is_typed() {
    let command = HelperCommand {
        program: PathBuf::from("/definitely/missing/helper"),
        args: Vec::new(),
        envs: Vec::new(),
    };
    let spec = fixture_spec(&[]);
    let err = run_helper(
        &command,
        &HelperRequest::describe(spec, 30_000),
        Duration::from_secs(30),
    )
    .unwrap_err();
    assert_eq!(err.code(), "PLUGIN_FAILED");
}

#[cfg(target_os = "macos")]
#[test]
fn audio_unit_delay_describe_and_process() {
    // AUDelay (aufx:dely:appl) ships with every macOS install.
    let spec = PluginSpec {
        format: PluginFormat::AudioUnit,
        path: None,
        sha256: None,
        component: "aufx:dely:appl".into(),
        version: None,
        parameters: Vec::new(),
    };
    let probe = match run_helper(
        &helper_command(),
        &HelperRequest::describe(spec.clone(), 60_000),
        Duration::from_secs(60),
    ) {
        Ok(r) => r,
        Err(PluginError::Missing(_)) => {
            eprintln!("skipping: system AUDelay not registered on this host");
            return;
        }
        Err(e) => panic!("describe failed: {e}"),
    };
    let report = probe.report.unwrap();
    assert!(!report.version.is_empty());
    let dir = tempfile::tempdir().unwrap();
    let io = io_pair(dir.path(), 4800);
    let input = ramp(4800, 2);
    write_f32(&io.input, &input);
    // AUDelay default 1 s dry mix — output must be finite and non-silent.
    let response = run_helper(
        &helper_command(),
        &HelperRequest::process(spec, io.clone(), 60_000),
        Duration::from_secs(60),
    )
    .expect("au process");
    assert_eq!(response.frames, Some(4800));
    let output = read_f32(&io.output);
    assert_eq!(output.len(), input.len());
    assert!(output.iter().all(|s| s.is_finite()));
    assert!(output.iter().any(|s| s.abs() > 1e-6));
}

#[cfg(not(target_os = "macos"))]
#[test]
fn audio_unit_is_typed_unsupported_off_macos() {
    let spec = PluginSpec {
        format: PluginFormat::AudioUnit,
        path: None,
        sha256: None,
        component: "aufx:dely:appl".into(),
        version: None,
        parameters: Vec::new(),
    };
    let err = run_helper(
        &helper_command(),
        &HelperRequest::describe(spec, 30_000),
        Duration::from_secs(30),
    )
    .unwrap_err();
    assert_eq!(err.code(), "UNSUPPORTED_FEATURE");
}

#[test]
fn spec_validation_and_manifest_determinism() {
    let path = bundle();
    let a = bundle_manifest_hash(&path).unwrap();
    let b = bundle_manifest_hash(&path).unwrap();
    assert_eq!(a, b);
    let spec = fixture_spec(&[]);
    spec.validate().unwrap();
    verify_spec_pin(&spec).unwrap();
    let mut bad = spec.clone();
    bad.sha256 = Some("not-hex".into());
    assert_eq!(bad.validate().unwrap_err().code(), "INVALID_REQUEST");
    let mut bad_params = spec.clone();
    bad_params.parameters = vec![PluginParameter { id: 0, value: 1.5 }];
    assert_eq!(bad_params.validate().unwrap_err().code(), "INVALID_REQUEST");
}
