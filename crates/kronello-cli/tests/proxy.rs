//! ADR-0119 end-to-end: a real `proxy.generate` worker produces a registered
//! proxy, preview substitution decodes it, and file outputs stay original.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use kronello_jobs::{JobConfig, JobStatus, JobStore};
use kronello_media::{EncodeCodec, EncodeFrame, EncodeRequest, MediaRuntime, content_hash};
use kronello_model::{
    Asset, AssetId, AssetKind, AssetLocator, Composition, CompositionId, DescriptorRef,
    DesignExtent, DocumentObject, FiniteF64, MediaNode, NodeId, NodeKind, Project, Property,
    PropertyId, PropertySource, SceneNode, SchemaKey, Value,
};
use kronello_time::{Duration as KDuration, FrameRate, Rational, Time, TimeMap, TimeRange};
use serde_json::{Value as Json, json};
use sha2::Digest;

fn r(n: i64, d: i64) -> Rational {
    Rational::new(n, d).unwrap()
}

/// 1px red/green checkerboard ProRes clip. Half-scale bilinear downsampling
/// collapses the checker to a near-uniform blend, so a rendered original shows
/// strong per-pixel contrast while a rendered proxy does not.
fn write_clip(path: &Path, width: u32, height: u32, frames: usize) {
    MediaRuntime::load()
        .unwrap()
        .encode_video_stream(
            &EncodeRequest {
                output: path.into(),
                codec: EncodeCodec::ProRes,
                width,
                height,
                time_base: r(1, 24),
            },
            frames,
            &mut |index| {
                let mut rgba = Vec::with_capacity((width * height * 4) as usize);
                for p in 0..(width * height) {
                    let (x, y) = (p % width, p / width);
                    let px = if (x + y + index as u32).is_multiple_of(2) {
                        [220, 40, 40]
                    } else {
                        [40, 220, 40]
                    };
                    rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
                }
                Ok(EncodeFrame {
                    pts: r(index as i64, 24),
                    rgba,
                })
            },
        )
        .unwrap();
}

fn document(dir: &Path, asset: AssetId) -> Json {
    write_clip(&dir.join("original.mov"), 32, 24, 3);
    let file = dir.join("original.mov");
    let metadata = MediaRuntime::load()
        .unwrap()
        .open_video_stream(&file, 0)
        .unwrap()
        .stream_metadata()
        .unwrap();
    let volume_id = PropertyId::new();
    let node_id = NodeId::new();
    let composition = CompositionId::new();
    let registry = kronello_render::render_registry();
    let volume = Property::new(
        volume_id,
        DescriptorRef::new(
            registry
                .lookup(&SchemaKey::new("kronello.audio.volume").unwrap())
                .unwrap(),
        ),
        PropertySource::Constant(Value::Scalar(FiniteF64::new(1.0).unwrap())),
        vec![],
        &registry,
    )
    .unwrap();
    let mut document = Project::default();
    document.assets.push(DocumentObject::Known(Asset {
        id: asset,
        content_hash: content_hash(&file).unwrap(),
        kind: AssetKind::Video,
        streams: vec![metadata],
        locator: AssetLocator {
            relative: Some("original.mov".into()),
            absolute: None,
        },
    }));
    document
        .compositions
        .push(DocumentObject::Known(Composition {
            id: composition,
            duration: KDuration::new(r(1, 1)).unwrap(),
            design_extent: DesignExtent::new(32.0, 24.0).unwrap(),
            edit_rate: FrameRate::new(24, 1).unwrap(),
            root_nodes: vec![node_id],
            nodes: vec![SceneNode {
                tags: Default::default(),
                name: None,
                enabled: true,
                effects: vec![],
                id: node_id,
                kind: NodeKind::Media(MediaNode {
                    asset,
                    stream_index: 0,
                    source_in: Time::ZERO,
                    time_map: TimeMap::linear(Time::ZERO, r(1, 1)).unwrap(),
                    volume: volume_id,
                }),
                containment_parent: None,
                transform_parent: None,
                child_order: vec![],
                active_range: TimeRange::new(Time::ZERO, r(1, 1)).unwrap(),
                properties: vec![volume],
            }],
            properties: vec![],
        }));
    serde_json::to_value(document).unwrap()
}

struct Fixture {
    _cleanup: kronello_jobs::test_support::WorkerCleanup,
    _tempdir: tempfile::TempDir,
    project: PathBuf,
    state: PathBuf,
    composition: CompositionId,
    asset: AssetId,
}
impl Fixture {
    fn new() -> Self {
        let tempdir = tempfile::tempdir().unwrap();
        let state = tempdir.path().join("state");
        let project = tempdir.path().join("clip.kronello");
        let asset = AssetId::new();
        let document = document(tempdir.path(), asset);
        let composition =
            serde_json::from_value::<CompositionId>(document["compositions"][0]["id"].clone())
                .unwrap();
        let fixture = Self {
            _cleanup: kronello_jobs::test_support::WorkerCleanup::new(&state).unwrap(),
            _tempdir: tempdir,
            project,
            state,
            composition,
            asset,
        };
        fixture.cli(
            json!({"operation":"project.create","project":fixture.project,
                "document":document}),
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
    fn frame(&self, proxies: &str) -> Json {
        self.cli(json!({"operation":"render.frame",
            "input":{"project":self.project,"composition":self.composition,
                "region":{"origin":[0,0],"extent":[32,24],"pixels":[32,24]},
                "media_proxies":proxies},
            "time":{"num":"0","den":"1"}}))
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
}

/// Number of adjacent same-row output pixel pairs that are exactly equal.
/// The 1px checkerboard original keeps adjacent pixels distinct; a decoded
/// half-scale proxy upscaled by nearest repeat produces identical 2x1 pairs.
fn paired_pairs(frame: &Json) -> usize {
    let display = frame["display"].as_array().unwrap();
    display
        .chunks_exact(32)
        .map(|row| row.windows(2).filter(|w| w[0] == w[1]).count())
        .sum()
}

#[test]
fn proxy_job_registers_link_and_preview_substitutes_it() {
    let fixture = Fixture::new();
    // `kronello proxy generate` submits a fixed-input transcode job.
    let jobs = fixture.cli_args(
        &["proxy", "generate"],
        json!({"project": fixture.project, "assets": [fixture.asset], "scale": 0.5}),
    );
    let job_id = jobs["jobs"][0]["id"].as_str().unwrap().to_owned();
    let record = fixture.wait(&job_id, JobStatus::Succeeded);
    let proxy_file = record.destination.clone();
    assert!(proxy_file.is_file());
    assert_eq!(proxy_file.extension().unwrap(), "mov");
    assert!(
        proxy_file
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with(".proxies")
    );
    // The registered proxy asset records the published file's content hash.
    let exported = fixture.cli(json!({"operation":"project.export","project":fixture.project}));
    let assets = exported["document"]["assets"].as_array().unwrap();
    let proxy_asset = assets
        .iter()
        .find(|a| a["id"] != json!(fixture.asset))
        .expect("proxy asset registered");
    assert_eq!(
        proxy_asset["content_hash"].as_str().unwrap(),
        format!(
            "{:x}",
            sha2::Sha256::digest(std::fs::read(&proxy_file).unwrap())
        )
    );
    // The worker registered the link through the project edit path.
    let status = fixture.cli_args(&["proxy", "status"], json!({"project": fixture.project}));
    assert_eq!(status["proxies"].as_array().unwrap().len(), 1);
    let entry = &status["proxies"][0];
    assert_eq!(entry["state"], "ready");
    assert_eq!(entry["link"]["original"], json!(fixture.asset));
    assert_eq!(entry["link"]["scale"], json!(0.5));
    assert_eq!(entry["link"]["width"], json!(16));
    // Original decode preserves the checkerboard's distinct neighbours.
    let off = paired_pairs(&fixture.frame("off"));
    assert!(off < 64, "original keeps the checkerboard: {off}");
    // Prefer mode decodes the half-scale proxy: identical 2x1 pairs appear.
    let prefer = paired_pairs(&fixture.frame("prefer"));
    assert!(prefer >= 300, "proxy decode upscales uniformly: {prefer}");
    // A missing proxy file falls back to the original instead of failing.
    std::fs::remove_file(&proxy_file).unwrap();
    let status = fixture.cli_args(&["proxy", "status"], json!({"project": fixture.project}));
    assert_eq!(status["proxies"][0]["state"], "missing");
    let fallen_back = paired_pairs(&fixture.frame("prefer"));
    assert!(
        fallen_back < 64,
        "missing proxy falls back to the original: {fallen_back}"
    );
    // proxy.clear removes the link; prefer stays on the original.
    let revision = fixture.cli(json!({
        "operation":"project.info","project":fixture.project}))["revision"]
        .as_str()
        .unwrap()
        .to_owned();
    fixture.cli_args(
        &["proxy", "clear"],
        json!({"project": fixture.project, "base_revision": revision,
            "asset": fixture.asset}),
    );
    let status = fixture.cli_args(&["proxy", "status"], json!({"project": fixture.project}));
    assert!(status["proxies"].as_array().unwrap().is_empty());
    let cleared = paired_pairs(&fixture.frame("prefer"));
    assert!(cleared < 64, "cleared proxy falls back: {cleared}");
}
