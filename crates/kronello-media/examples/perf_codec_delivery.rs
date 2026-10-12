//! Release delivery-path codec probe with synthetic, deterministic tile input.
//! This isolates encoding/fan-out/mux costs; it does not measure scene rendering.
use kronello_audio::{AudioSourceMode, ClippingPolicy};
use kronello_media::*;
use kronello_model::*;
use kronello_render::*;
use kronello_time::{FrameRate, Time, TimeRange};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{cell::Cell, path::PathBuf, time::Instant};

struct PatternBackend(Cell<u64>);
impl RenderBackend for PatternBackend {
    fn name(&self) -> &str {
        "synthetic_codec_probe"
    }
    fn execute(&self, dag: &RenderDag) -> Result<BackendFrame, RenderError> {
        let region = dag.region();
        let number = self.0.get();
        self.0.set(number + 1);
        let linear = (0..region.pixels[1])
            .flat_map(|y| {
                (0..region.pixels[0]).map(move |x| {
                    let x = x + region.origin[0] as u32;
                    let y = y + region.origin[1] as u32;
                    let n = number as u32;
                    [
                        ((3 * x + y + 7 * n) % 256) as f32 / 255.0,
                        ((x + 2 * y + 13 * n) % 256) as f32 / 255.0,
                        ((x / 8 + y / 8 + 3 * n) % 256) as f32 / 255.0,
                        1.0,
                    ]
                })
            })
            .collect::<Vec<_>>();
        Ok(BackendFrame {
            display: linear.clone(),
            linear,
        })
    }
}

fn main() {
    if cfg!(debug_assertions) {
        panic!("use --release");
    }
    let directory = PathBuf::from(std::env::args().nth(1).expect("new output directory"));
    let filter = std::env::args().nth(2);
    std::fs::create_dir(&directory).unwrap();
    let runtime = MediaRuntime::load().unwrap();
    let mut cases = Vec::new();
    for size in [[640, 360], [1920, 1080]] {
        for (label, profile) in [
            ("prores", MovieProfile::ProResPcm24),
            ("dnxhr_hqx", MovieProfile::DnxhrHqxMovPcm24V1),
            ("svt_av1", MovieProfile::Av1Mp4AacV1),
        ] {
            let case = format!("{label}-{}", size[0]);
            if filter.as_ref().is_some_and(|f| f != &case) {
                continue;
            }
            let mut project: Project =
                serde_json::from_str(include_str!("../../../examples/perf-codec.project.json"))
                    .unwrap();
            let DocumentObject::Known(sequence) = &mut project.sequences[0] else {
                panic!("fixed sequence must be known")
            };
            sequence.extent = DesignExtent::new(size[0] as f64, size[1] as f64).unwrap();
            let target = RenderTarget::Sequence {
                sequence: sequence.id,
            };
            let render =
                RenderSnapshot::for_target(&project, target, 0, RenderProfile::default()).unwrap();
            let snapshot = if profile == MovieProfile::ProResPcm24 {
                AvExportSnapshot::with_audio(&render, AudioSourceMode::Silence, vec![])
            } else {
                AvExportSnapshot::with_movie_profile(
                    &render,
                    AudioSourceMode::Silence,
                    vec![],
                    profile,
                )
            }
            .unwrap();
            let mut trials = Vec::new();
            for iteration in 0..9 {
                let output = directory.join(format!("{case}-{iteration}.{}", profile.container()));
                let request = AvExportRequest {
                    output: output.clone(),
                    range: TimeRange::new(Time::ZERO, Time::new(1, 2).unwrap()).unwrap(),
                    frame_rate: FrameRate::new(24, 1).unwrap(),
                    region: OutputRegion {
                        origin: [0.0; 2],
                        extent: size.map(f64::from),
                        pixels: size,
                    },
                    background: [0.0; 3],
                    clipping: ClippingPolicy::Reject,
                    chapters: ChapterPolicy::Omit,
                    outputs: vec![],
                };
                let backend = PatternBackend(Cell::new(0));
                let mut checkpoints = Vec::new();
                let start = Instant::now();
                let report = runtime
                    .export_delivery_with_checkpoint(
                        &[&snapshot],
                        &directory.join("unused.kronello"),
                        &[],
                        &backend,
                        &request,
                        &mut |n| {
                            checkpoints.push(n);
                            Ok(())
                        },
                    )
                    .unwrap();
                let ms = start.elapsed().as_secs_f64() * 1000.0;
                assert_eq!(report.frames.len(), 12);
                assert_eq!(checkpoints.last(), Some(&12));
                trials.push(json!({
                    "iteration":iteration,"ms":ms,"output":output,
                    "encoded_sha256":content_hash(&output).unwrap(),
                    "backend_calls":backend.0.get(),"checkpoints":checkpoints,
                    "render_snapshot_hash":report.render_snapshot_hash,
                    "export_snapshot_hash":report.export_snapshot_hash,
                    "probe":report.probe,"streaming":report.streaming,
                    "outputs":report.outputs,"sample_range":report.sample_range
                }));
            }
            let mut warm = trials[1..]
                .iter()
                .map(|t| t["ms"].as_f64().unwrap())
                .collect::<Vec<_>>();
            warm.sort_by(f64::total_cmp);
            cases.push(json!({"case":case,"profile":profile,"size":size,
                "cold_ms":trials[0]["ms"],"warm_p50_ms":warm[4],
                "warm_p95_ms":warm[7],"trials":trials}));
        }
    }
    println!("{}", serde_json::to_string_pretty(&json!({
        "schema_version":2,
        "binary_sha256":content_hash(&std::env::current_exe().unwrap()).unwrap(),
        "fixture_sha256":format!("{:x}", Sha256::digest(include_bytes!("../../../examples/perf-codec.project.json"))),
        "quiet_host":false,"frame_count":12,"time_base":"1/24",
        "timed_scope":"synthetic tile generation, audio encode, video encode, mux, validation and publication; runtime load and hashes excluded",
        "capabilities":runtime.capabilities(),"cases":cases
    })).unwrap());
}
