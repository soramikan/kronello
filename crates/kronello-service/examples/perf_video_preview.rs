//! Video preview latency probe: isolates decode, resolve and GPU submission
//! stages of `preview_dag` + `preview_texture` for a video-backed sequence.
//! Usage: perf_video_preview <movie.mov> <project.kronello> <none|blur> [frames] [WxH]
use kronello_gpu::GpuContext;
use kronello_media::{MediaRuntime, content_hash};
use kronello_model::*;
use kronello_render::{OutputRegion, RenderTarget};
use kronello_service::{BackendSelection, CreateRequest, FrameRenderRequest, Request, Service};
use kronello_store::{OpenOptions, ProjectStore};
use kronello_time::{FrameRate, Rational, SampleRate, Time, TimeMap, TimeRange};
use serde_json::json;
use std::path::Path;
use std::time::Instant;

fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

fn create_project(
    movie: &Path,
    project_path: &Path,
    blur: bool,
) -> (Asset, SequenceId, Time, Time) {
    let runtime = MediaRuntime::load().unwrap();
    let mut decoder = runtime.open_video(movie).unwrap();
    let stream = decoder.stream_metadata().unwrap();
    let source_in = stream.start_time.unwrap_or(Time::ZERO);
    let duration = stream.duration.expect("movie duration");
    let asset = Asset {
        id: AssetId::new(),
        content_hash: content_hash(movie).unwrap(),
        kind: AssetKind::Video,
        streams: vec![stream],
        locator: AssetLocator {
            relative: None,
            absolute: Some(movie.to_str().unwrap().into()),
        },
    };
    let mut clip = Clip {
        id: ClipId::new(),
        source_ref: SourceRef::Asset {
            asset: asset.id,
            stream_index: 0,
        },
        timeline_range: TimeRange::new(Time::ZERO, duration).unwrap(),
        source_in,
        time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
        enabled: true,
        audio_retime: AudioRetimePolicy::Reject,
        reverse_sampling: None,
        volume: None,
        links: vec![],
        properties: vec![],
        effects: vec![],
        masks: vec![],
        markers: vec![],
        pan: None,
    };
    if blur {
        let registry = kronello_render::render_registry();
        let sigma = Property::new(
            PropertyId::new(),
            DescriptorRef::new(
                registry
                    .lookup(&SchemaKey::new("kronello.effect.sigma").unwrap())
                    .unwrap(),
            ),
            PropertySource::Constant(Value::Scalar(FiniteF64::new(4.0).unwrap())),
            vec![],
            &registry,
        )
        .unwrap();
        clip.properties.push(sigma.clone());
        clip.effects = vec![Effect::Known(EffectDefinition {
            effect_id: "kronello.gaussian_blur".into(),
            version: 1,
            parameters: EffectParameters::GaussianBlur { sigma: sigma.id() },
        })];
    }
    let sequence = Sequence {
        id: SequenceId::new(),
        extent: DesignExtent::new(7680.0, 4320.0).unwrap(),
        frame_rate: FrameRate::new(24, 1).unwrap(),
        audio_rate: SampleRate::HZ_48000,
        working_space: ColorSpace::LinearRec709,
        tracks: vec![Track {
            state: None,
            id: TrackId::new(),
            kind: TrackKind::Video,
            clips: vec![clip],
        }],
        transitions: vec![],
        markers: vec![],
        work_area: None,
        targets: None,
    };
    let id = sequence.id;
    let mut project = Project::default();
    project.assets.push(DocumentObject::Known(asset.clone()));
    project.sequences.push(DocumentObject::Known(sequence));
    Service::new(BackendSelection::CpuReference)
        .dispatch(Request::ProjectCreate(CreateRequest {
            plan_hash: None,
            idempotency_key: None,
            project: project_path.into(),
            document: project,
        }))
        .unwrap();
    (asset, id, source_in, duration)
}

fn summarize(samples: &[f64]) -> serde_json::Value {
    let mut sorted = samples.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    json!({
        "n": samples.len(),
        "p50_ms": sorted[sorted.len() / 2],
        "max_ms": sorted.last().copied().unwrap_or(0.0),
        "mean_ms": samples.iter().sum::<f64>() / samples.len().max(1) as f64,
    })
}

fn main() {
    if cfg!(debug_assertions) {
        panic!("use cargo build --release");
    }
    let args: Vec<_> = std::env::args().collect();
    assert!(
        args.len() >= 4,
        "perf_video_preview movie.mov project.kronello none|blur [frames] [WxH]"
    );
    let movie = Path::new(&args[1]).to_path_buf();
    let project_path = Path::new(&args[2]).to_path_buf();
    let blur = args[3].as_str() == "blur";
    let frames: usize = args.get(4).map(|s| s.parse().unwrap()).unwrap_or(24);
    let pixels: [u32; 2] = args
        .get(5)
        .map(|s| {
            let (w, h) = s.split_once('x').unwrap();
            [w.parse().unwrap(), h.parse().unwrap()]
        })
        .unwrap_or([1512, 851]);

    let (asset, sequence, origin, _duration) = create_project(&movie, &project_path, blur);
    let runtime = MediaRuntime::load().unwrap();
    let gpu = GpuContext::new().unwrap();
    let service = Service::with_backend(&gpu);
    let step_num: i64 = 1;
    let step_den: i64 = 24;
    let mut report = serde_json::Map::new();
    report.insert("pixels".into(), json!(pixels));
    report.insert("blur".into(), json!(blur));

    // Stage A: decode-only costs under the exact contracts the render paths use.
    let mut load_ms = vec![];
    let mut open_decode_ms = vec![];
    let mut image_ms = vec![];
    let mut persistent_ms = vec![];
    let mut persistent = runtime.open_video_stream(&movie, 0).unwrap();
    for i in 0..frames {
        let time = origin
            .checked_add(Rational::new(i as i64 * step_num, step_den).unwrap())
            .unwrap();
        let t = Instant::now();
        let runtime2 = MediaRuntime::load().unwrap();
        load_ms.push(ms(t));
        let t = Instant::now();
        let mut dec = runtime2.open_video_stream(&movie, 0).unwrap();
        dec.decode_at(time).unwrap();
        open_decode_ms.push(ms(t));
        let t = Instant::now();
        let image = runtime2
            .decode_video_image(&asset, &project_path, 0, time, ColorSpace::LinearRec709)
            .unwrap();
        std::hint::black_box(&image);
        image_ms.push(ms(t));
        let t = Instant::now();
        persistent.decode_at(time).unwrap();
        persistent_ms.push(ms(t));
    }
    report.insert("media_runtime_load_ms".into(), summarize(&load_ms));
    report.insert("fresh_open_decode_at_ms".into(), summarize(&open_decode_ms));
    report.insert("fresh_decode_video_image_ms".into(), summarize(&image_ms));
    report.insert("persistent_decode_at_ms".into(), summarize(&persistent_ms));

    // Stage B: the full redraw path the GUI drives per frame. The persistent
    // `MediaSession` + device-resident upload path is what the native preview
    // uses: the DAG is built unresolved, media resolves once (decode + GPU
    // upload + shader sampling), and the scene composite runs on device.
    let mut session = kronello_service::MediaSession::new();
    let mut dag_ms = vec![];
    let mut resolve_ms = vec![];
    let mut tex_ms = vec![];
    let mut wait_ms = vec![];
    let mut total_ms = vec![];
    for i in 0..frames {
        let time = origin
            .checked_add(Rational::new(i as i64 * step_num, step_den).unwrap())
            .unwrap();
        let request = FrameRenderRequest {
            input: kronello_service::RenderInput {
                project: project_path.clone(),
                composition: None,
                target: Some(RenderTarget::Sequence { sequence }),
                region: OutputRegion {
                    origin: [0.0; 2],
                    extent: [7680.0, 4320.0],
                    pixels,
                },
                profile: Default::default(),
                fonts: vec![],
                media_proxies: kronello_render::MediaProxyMode::Off,
                luts: vec![],
            },
            time,
            backend: Some(BackendSelection::Gpu),
        };
        let total = Instant::now();
        let t = Instant::now();
        let (_revision, dag) = service.preview_dag_unresolved(&request).unwrap();
        dag_ms.push(ms(t));
        let t = Instant::now();
        let resolved = service
            .resolve_dag_resident(&dag, &project_path, &mut session, &gpu)
            .unwrap();
        resolve_ms.push(ms(t));
        let t = Instant::now();
        let texture = gpu
            .preview_texture_resident(
                &resolved.dag,
                &resolved.resident,
                &resolved.input_identities,
            )
            .unwrap();
        tex_ms.push(ms(t));
        let t = Instant::now();
        gpu.wait().unwrap();
        wait_ms.push(ms(t));
        std::hint::black_box(texture);
        total_ms.push(ms(total));
    }
    report.insert("preview_dag_ms".into(), summarize(&dag_ms));
    report.insert("preview_resolve_ms".into(), summarize(&resolve_ms));
    report.insert("preview_texture_ms".into(), summarize(&tex_ms));
    report.insert("gpu_wait_ms".into(), summarize(&wait_ms));
    report.insert("redraw_total_ms".into(), summarize(&total_ms));
    report.insert(
        "decoder_pool_stats".into(),
        serde_json::to_value(session.decoder_pool_stats()).unwrap(),
    );

    // Store-open cost alone (what preview_dag pays before compiling).
    let mut store_ms = vec![];
    for _ in 0..frames.min(8) {
        let t = Instant::now();
        let store = ProjectStore::open(&project_path, OpenOptions::default()).unwrap();
        let snap = store.snapshot().unwrap();
        std::hint::black_box(&snap);
        store.close().unwrap();
        store_ms.push(ms(t));
    }
    report.insert("store_open_snapshot_ms".into(), summarize(&store_ms));
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}
