//! Compare the legacy software backend and the production service-scope backend
//! through the same fixed-snapshot movie pipeline. No alternative render path.
use kronello_media::*;
use kronello_model::*;
use kronello_render::*;
use kronello_time::*;
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};
use std::{path::Path, time::Instant};
fn t(n: i64) -> Time {
    Time::new(n, 24).unwrap()
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if cfg!(debug_assertions) {
        return Err("release required".into());
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output_dir = root.join("target/perf-001-evidence/product-seek");
    std::fs::create_dir_all(&output_dir)?;
    let dir = tempfile::tempdir()?;
    let source = dir.path().join("source.mov");
    let runtime = MediaRuntime::load()?;
    const FRAMES: usize = 6;
    const WIDTH: u32 = 1920;
    const HEIGHT: u32 = 1080;
    runtime.encode_video_stream(
        &EncodeRequest {
            output: source.clone(),
            codec: EncodeCodec::ProRes,
            width: WIDTH,
            height: HEIGHT,
            time_base: t(1),
        },
        FRAMES,
        &mut |i| {
            Ok(EncodeFrame {
                pts: t(i as i64),
                rgba: [i as u8, 91, 163, 255].repeat((WIDTH * HEIGHT) as usize),
            })
        },
    )?;
    let metadata = runtime.open_video(&source)?.stream_metadata()?;
    let asset = Asset {
        id: AssetId::new(),
        kind: AssetKind::Video,
        content_hash: content_hash(&source)?,
        locator: AssetLocator {
            relative: None,
            absolute: Some(source.to_str().unwrap().into()),
        },
        streams: vec![metadata],
    };
    let registry = render_registry();
    let descriptor = registry
        .lookup(&SchemaKey::new("kronello.audio.volume")?)
        .unwrap();
    let volume = Property::new(
        PropertyId::new(),
        DescriptorRef::new(descriptor),
        PropertySource::Constant(Value::Scalar(FiniteF64::new(1.0)?)),
        vec![],
        &registry,
    )?;
    let node = SceneNode {
        id: NodeId::new(),
        name: None,
        tags: Default::default(),
        enabled: true,
        kind: NodeKind::Media(MediaNode {
            asset: asset.id,
            stream_index: 0,
            source_in: Time::ZERO,
            time_map: TimeMap::linear(Time::ZERO, Time::ONE)?,
            volume: volume.id(),
        }),
        containment_parent: None,
        transform_parent: None,
        child_order: vec![],
        active_range: TimeRange::new(Time::ZERO, t(FRAMES as i64))?,
        properties: vec![volume],
        effects: vec![],
    };
    let composition = Composition {
        id: CompositionId::new(),
        duration: Duration::new(t(FRAMES as i64))?,
        design_extent: DesignExtent::new(WIDTH as f64, HEIGHT as f64)?,
        edit_rate: FrameRate::new(24, 1)?,
        root_nodes: vec![node.id],
        nodes: vec![node],
        properties: vec![],
    };
    let id = composition.id;
    let project = Project {
        assets: vec![DocumentObject::Known(asset)],
        compositions: vec![DocumentObject::Known(composition)],
        ..Project::default()
    };
    let snapshot = RenderSnapshot::new(&project, id, 1, RenderProfile::default())?;
    let fixed =
        AvExportSnapshot::with_audio(&snapshot, kronello_audio::AudioSourceMode::Silence, vec![])?;
    let cpu = kronello_gpu::render_adapter::CpuReferenceBackend;
    let selected = std::env::args().nth(1).unwrap_or_else(|| "both".into());
    if !matches!(selected.as_str(), "before" | "after" | "both") {
        return Err("before|after|both".into());
    }
    let mut movie_identity = String::new();
    let mut phases = Vec::<Json>::new();
    let mut reference: Option<Vec<DecodedVideoFrame>> = None;
    for phase in ["before", "after"] {
        if selected != "both" && selected != phase {
            continue;
        }
        let mut samples = vec![];
        let mut counters = Vec::new();
        let mut pool_counters = Vec::new();
        for iteration in 0..23 {
            let output = dir.path().join(format!("{phase}-{iteration}.mov"));
            let old = VideoRenderBackend {
                backend: &cpu,
                project_path: dir.path(),
            };
            let sequential = SequentialVideoRenderBackend::new(&cpu, dir.path(), Ok(&runtime));
            let backend: &dyn RenderBackend = if phase == "before" { &old } else { &sequential };
            let request = AvExportRequest {
                output: output.clone(),
                range: TimeRange::new(Time::ZERO, t(FRAMES as i64))?,
                frame_rate: FrameRate::new(24, 1)?,
                region: OutputRegion {
                    origin: [0.0; 2],
                    extent: [WIDTH as f64, HEIGHT as f64],
                    pixels: [128, 72],
                },
                background: [0.0; 3],
                clipping: kronello_audio::ClippingPolicy::Reject,
            };
            let start = Instant::now();
            let report = runtime.export_av(&fixed, dir.path(), &[], backend, &request)?;
            let elapsed = start.elapsed().as_nanos() as u64;
            report.probe.verify_av()?;
            if iteration >= 3 {
                samples.push(elapsed);
                counters.push(sequential.decoder_stats());
                pool_counters.push(sequential.pool_stats());
            }
            // Decode all final native frames outside timing; prove exact movie output.
            let mut decoder = runtime.open_video(&output)?;
            let actual: Vec<_> = (0..FRAMES)
                .map(|i| decoder.decode_at(t(i as i64)).unwrap())
                .collect();
            let mut digest = Sha256::new();
            for frame in &actual {
                digest.update(serde_json::to_vec(&json!([
                    frame.pts,
                    frame.end,
                    frame.width,
                    frame.height,
                    &frame.pixel_format,
                    &frame.color_primaries,
                    &frame.color_transfer,
                    &frame.color_matrix,
                    &frame.color_range
                ]))?);
                digest.update(&frame.pixels);
            }
            movie_identity = format!("{:x}", digest.finalize());
            if let Some(expected) = &reference {
                assert_eq!(&actual, expected);
            } else {
                reference = Some(actual);
            }
        }
        let mut sorted = samples.clone();
        sorted.sort_unstable();
        phases.push(json!({"phase":phase,"samples_ns":samples,"p50_ns":sorted[9],"p95_ns":sorted[18],"decode_stats":counters,"pool_stats":pool_counters,"exact_final_movie_native_planes_pts_end_color":"pass"}));
    }
    let report = json!({"schema_version":1,"selected":selected,"exact_final_movie_identity":movie_identity,"source_sha256":content_hash(&source)?,"build_profile":"release","runtime_version":runtime.capabilities().ffmpeg_version,"source_identities":{"video.rs":format!("{:x}",Sha256::digest(include_bytes!("../src/video.rs"))),"render.rs":format!("{:x}",Sha256::digest(include_bytes!("../src/render.rs"))),"export.rs":format!("{:x}",Sha256::digest(include_bytes!("../src/export.rs")))},"fixture":{"codec":"prores","frames":FRAMES,"width":WIDTH,"height":HEIGHT,"recipe":"Native LGPL runtime encode_video_stream; pts=i/24; opaqueRGBA[i as u8,91,163,255]"},"pipeline":"same export_av fixed snapshot + CPU-reference render + ProRes/PCM24 encoder/mux/probe; production service-scope SequentialVideoRenderBackend after vs legacy per-frame VideoRenderBackend before","warmup":3,"repetitions":20,"resource_categories":{"native_planes_retained_limit_bytes":134217728,"native_decoder_count_limit":2,"actual_current_lookahead_bytes":WIDTH*HEIGHT*8,"decode_native_codec_internal_peak":"not exposed by FFmpeg; whole-process RSS separate","encoder_native_internal_peak":"not exposed by FFmpeg; whole-process RSS separate","encoder_frontend_rgba_bytes":128*72*4,"render_full_linear_display_bytes":128*72*32,"output_video_frames_in_flight":1},"phases":phases});
    let path = output_dir.join(format!("measurements-{selected}.json"));
    std::fs::write(&path, serde_json::to_vec_pretty(&report)?)?;
    println!("{}", path.display());
    Ok(())
}
