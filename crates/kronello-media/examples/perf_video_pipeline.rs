//! Same binary, real GPU, serial versus two-frame producer/encoder overlap.
use kronello_audio::{AudioSourceMode, ClippingPolicy};
use kronello_gpu::GpuContext;
use kronello_media::*;
use kronello_model::{DocumentObject, Project};
use kronello_render::*;
use kronello_time::{FrameRate, Time, TimeRange};
use serde_json::json;
use std::{path::PathBuf, time::Instant};
fn snapshot(temporal: bool) -> RenderSnapshot {
    let mut document: Project =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    let DocumentObject::Known(composition) = &mut document.compositions[0] else {
        panic!()
    };
    composition.nodes.truncate(1);
    composition.root_nodes.truncate(1);
    let id = composition.id;
    document.texts.clear();
    RenderSnapshot::new(
        &document,
        id,
        1,
        RenderProfile {
            temporal: temporal.then_some(TemporalSettings {
                frame_rate: FrameRate::new(24, 1).unwrap(),
                shutter_angle: Time::new(180, 1).unwrap(),
                shutter_phase: Time::new(-1, 4).unwrap(),
                samples: 3,
                cut_policy: CutPolicy::AvoidCrossing,
            }),
            ..RenderProfile::default()
        },
    )
    .unwrap()
}
fn export_snapshot(render: &RenderSnapshot, profile: MovieProfile) -> AvExportSnapshot {
    if profile == MovieProfile::ProResPcm24 {
        AvExportSnapshot::with_audio(render, AudioSourceMode::Silence, vec![])
    } else {
        AvExportSnapshot::with_movie_profile(render, AudioSourceMode::Silence, vec![], profile)
    }
    .unwrap()
}
fn main() {
    if cfg!(debug_assertions) {
        panic!("use --release");
    }
    let directory = PathBuf::from(std::env::args().nth(1).expect("new output directory"));
    let mode = std::env::args().nth(2).expect("serial or two_frames");
    let selected = match mode.as_str() {
        "serial" => VideoPipelineMode::Serial,
        "two_frames" => VideoPipelineMode::TwoFrames,
        _ => panic!("invalid mode"),
    };
    std::fs::create_dir(&directory).unwrap();
    let runtime = MediaRuntime::load()
        .unwrap()
        .with_video_pipeline_mode(selected);
    let mut cases = Vec::new();
    for (size, profile, temporal, fanout) in [
        ([640, 360], MovieProfile::ProResPcm24, false, false),
        ([640, 360], MovieProfile::ProResPcm24, true, false),
        ([640, 360], MovieProfile::DnxhrHqxMovPcm24V1, false, false),
        ([640, 360], MovieProfile::DnxhrHqxMovPcm24V1, true, false),
        ([1920, 1080], MovieProfile::ProResPcm24, false, false),
        ([1920, 1080], MovieProfile::ProResPcm24, true, false),
        ([640, 360], MovieProfile::ProResPcm24, false, true),
    ] {
        let label = format!("{profile:?}-{}-temporal{temporal}-fanout{fanout}", size[0]);
        let render = snapshot(temporal);
        let primary = export_snapshot(&render, profile);
        let extra: Vec<_> = if fanout {
            vec![MovieProfile::GifV1, MovieProfile::FlacV1]
        } else {
            vec![]
        };
        let mut snapshots = vec![primary];
        for profile in &extra {
            snapshots.push(export_snapshot(&render, *profile));
        }
        let refs: Vec<_> = snapshots.iter().collect();
        let begin = Instant::now();
        let gpu = GpuContext::new().unwrap();
        let gpu_init_ms = begin.elapsed().as_secs_f64() * 1000.0;
        let mut trials = vec![];
        for iteration in 0..9 {
            let output = directory.join(format!("{label}-{iteration}.{}", profile.container()));
            let request = AvExportRequest {
                output: output.clone(),
                range: TimeRange::new(Time::new(1, 2).unwrap(), Time::ONE).unwrap(),
                frame_rate: FrameRate::new(24, 1).unwrap(),
                region: OutputRegion {
                    origin: [0.0; 2],
                    extent: [64.0, 32.0],
                    pixels: size,
                },
                background: [0.0; 3],
                clipping: ClippingPolicy::Reject,
                chapters: ChapterPolicy::Omit,
                outputs: extra
                    .iter()
                    .enumerate()
                    .map(|(leg, profile)| DeliveryOutput {
                        output: directory.join(format!(
                            "{label}-{iteration}-leg{leg}.{}",
                            profile.container()
                        )),
                        profile: *profile,
                        background: [0.0; 3],
                        chapters: ChapterPolicy::Omit,
                    })
                    .collect(),
            };
            let mut checkpoints = vec![];
            let begin = Instant::now();
            let report = runtime
                .export_delivery_with_checkpoint(
                    &refs,
                    &directory.join("unused.kronello"),
                    &[],
                    &gpu,
                    &request,
                    &mut |n| {
                        checkpoints.push(n);
                        Ok(())
                    },
                )
                .unwrap();
            let wall_ms = begin.elapsed().as_secs_f64() * 1000.0;
            assert_eq!(report.frames.len(), 12);
            assert_eq!(checkpoints.last(), Some(&12));
            let files:Vec<_>=report.outputs.iter().map(|leg|json!({"path":leg.output,"sha256":content_hash(&leg.output).unwrap(),"profile":leg.profile})).collect();
            trials.push(json!({"iteration":iteration,"wall_ms":wall_ms,"files":files,"checkpoints":checkpoints,"frames":report.frames,"probe":report.probe,"sample_range":report.sample_range,"streaming":report.streaming,"leg_reports":report.outputs}));
        }
        let mut warm: Vec<_> = trials[1..]
            .iter()
            .map(|trial| trial["wall_ms"].as_f64().unwrap())
            .collect();
        warm.sort_by(f64::total_cmp);
        let bytes_per_bundle =
            u64::from(size[0]) * u64::from(size[1]) * 4 * if fanout { 2 } else { 1 };
        cases.push(json!({"case":label,"size":size,"profile":profile,"temporal":temporal,"fanout":fanout,"gpu_init_ms":gpu_init_ms,"cold_ms":trials[0]["wall_ms"],"warm_p50_ms":warm[4],"warm_p95_ms":warm[7],"converted_frame_payload_bytes":bytes_per_bundle*if mode=="serial" {1} else {2},"trials":trials}));
    }
    println!("{}",serde_json::to_string_pretty(&json!({"version":1,"mode":mode,"quiet_host":false,"same_binary":true,"frame_count":12,"frame_rate":"24/1","range":"[1/2,1)","capabilities":runtime.capabilities(),"scope":"real GPU shape render, optional3 shutter samples, PCM24 silence, ProRes4/DNx2 and optional GIF/FLAC fanout, encode/mux/probe/publication; runtime/GPU creation and file hashes outside timing; first compile included in cold","comparison":"same pipeline and fresh encoder-thread runtime; one reusable bundle serializes versus default two bundles overlap; not the prior single-thread implementation binary","cases":cases})).unwrap());
}
