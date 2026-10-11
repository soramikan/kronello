//! Actual GPU plane selection and delivery comparison in one release binary.
use kronello_audio::{AudioSourceMode, ClippingPolicy};
use kronello_gpu::GpuContext;
use kronello_media::*;
use kronello_model::{DocumentObject, Project};
use kronello_render::*;
use kronello_time::{FrameRate, Time, TimeRange};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

/// Compatibility backend forces the prior linear+display GPU boundary,
/// including intermediate temporal samples. All observations are forwarded.
struct Paired<'a>(&'a GpuContext);
impl RenderBackend for Paired<'_> {
    fn begin_observation_scope(
        &self,
    ) -> Result<Option<Box<dyn RenderObservationScope + '_>>, RenderError> {
        self.0.begin_observation_scope()
    }
    fn name(&self) -> &str {
        self.0.name()
    }
    fn cache_namespace(&self) -> Option<String> {
        self.0.cache_namespace()
    }
    fn transfer_stats_total(&self) -> Option<RenderTransferStats> {
        self.0.transfer_stats_total()
    }
    fn transfer_stats(&self) -> Option<RenderTransferStats> {
        self.0.transfer_stats()
    }
    fn resource_cache_stats(&self) -> Option<RenderResourceCacheStats> {
        self.0.resource_cache_stats()
    }
    fn execute(&self, dag: &RenderDag) -> Result<BackendFrame, RenderError> {
        self.0.execute(dag)
    }
    fn execute_with_cache(
        &self,
        dag: &RenderDag,
        cache: &mut RenderCache,
    ) -> Result<BackendFrame, RenderError> {
        self.0.execute_with_cache(dag, cache)
    }
    fn execute_with_inputs(
        &self,
        dag: &RenderDag,
        cache: &mut RenderCache,
        inputs: &std::collections::BTreeMap<usize, String>,
    ) -> Result<BackendFrame, RenderError> {
        self.0.execute_with_inputs(dag, cache, inputs)
    }
    fn display_from_linear(
        &self,
        linear: &[[f32; 4]],
        working: kronello_model::ColorSpace,
    ) -> Result<Vec<[f32; 4]>, RenderError> {
        self.0.display_from_linear(linear, working)
    }
}
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
    let profile = RenderProfile {
        temporal: temporal.then_some(TemporalSettings {
            frame_rate: FrameRate::new(24, 1).unwrap(),
            shutter_angle: Time::new(180, 1).unwrap(),
            shutter_phase: Time::new(-1, 4).unwrap(),
            samples: 3,
            cut_policy: CutPolicy::AvoidCrossing,
        }),
        ..RenderProfile::default()
    };
    RenderSnapshot::new(&document, id, 1, profile).unwrap()
}
fn percentiles(trials: &[serde_json::Value], key: &str) -> serde_json::Value {
    let mut warm = trials[1..]
        .iter()
        .map(|v| v[key].as_f64().unwrap())
        .collect::<Vec<_>>();
    warm.sort_by(f64::total_cmp);
    json!({"cold_ms":trials[0][key],"warm_p50_ms":warm[4],"warm_p95_ms":warm[7]})
}
fn main() {
    if cfg!(debug_assertions) {
        panic!("use --release");
    }
    let directory = PathBuf::from(std::env::args().nth(1).expect("new output directory"));
    std::fs::create_dir(&directory).unwrap();
    let shared_diagnostic = std::env::args().nth(2).as_deref() == Some("shared-1080");
    let runtime = MediaRuntime::load().unwrap();
    let mut frame_cases = Vec::new();
    for size in [[640, 360], [1920, 1080], [3840, 2160]] {
        for temporal in [false, true] {
            if shared_diagnostic && (size != [1920, 1080] || temporal) {
                continue;
            }
            let snapshot = snapshot(temporal);
            let create = Instant::now();
            let paired = GpuContext::new().unwrap();
            let paired_init_ms = create.elapsed().as_secs_f64() * 1000.0;
            let create = Instant::now();
            let linear = GpuContext::new().unwrap();
            let linear_init_ms = create.elapsed().as_secs_f64() * 1000.0;
            let backend = Paired(if shared_diagnostic { &linear } else { &paired });
            let request = FrameRequest {
                time: Time::new(1, 2).unwrap(),
                region: OutputRegion {
                    origin: [0.0; 2],
                    extent: [64.0, 32.0],
                    pixels: size,
                },
            };
            let mut paired_trials = Vec::new();
            let mut linear_trials = Vec::new();
            for iteration in 0..9 {
                for selected_linear in if iteration % 2 == 0 {
                    [false, true]
                } else {
                    [true, false]
                } {
                    let mut hash = Sha256::new();
                    let mut sink_time = Duration::ZERO;
                    let mut consume =
                        |offset: [u32; 2], tile: OutputRegion, pixels: &[[f32; 4]]| {
                            let start = Instant::now();
                            for value in offset {
                                hash.update(value.to_le_bytes());
                            }
                            for value in tile.pixels {
                                hash.update(value.to_le_bytes());
                            }
                            for pixel in pixels {
                                for value in pixel {
                                    hash.update(value.to_bits().to_le_bytes());
                                }
                            }
                            sink_time += start.elapsed();
                            Ok(())
                        };
                    let start = Instant::now();
                    let metadata = if selected_linear {
                        render_frame_linear_tiles(
                            &snapshot,
                            &[],
                            &linear,
                            request,
                            &mut |offset, tile, pixels| consume(offset, tile, &pixels),
                        )
                    } else {
                        render_frame_tiles(
                            &snapshot,
                            &[],
                            &backend,
                            request,
                            &mut |offset, tile, pixels| consume(offset, tile, &pixels.linear),
                        )
                    }
                    .unwrap();
                    let elapsed = start.elapsed();
                    let trial = json!({"iteration":iteration,"wall_ms":elapsed.as_secs_f64()*1000.0,
                        "sink_ms":sink_time.as_secs_f64()*1000.0,"render_ms":elapsed.saturating_sub(sink_time).as_secs_f64()*1000.0,
                        "linear_sha256":format!("{:x}",hash.finalize()),"metadata":metadata});
                    if selected_linear {
                        linear_trials.push(trial)
                    } else {
                        paired_trials.push(trial)
                    }
                }
            }
            for (a, b) in paired_trials.iter().zip(&linear_trials) {
                assert_eq!(a["linear_sha256"], b["linear_sha256"]);
            }
            frame_cases.push(json!({"size":size,"temporal":temporal,"adapter":format!("{:?}",linear.adapter_info),
                "paired_init_ms":paired_init_ms,"linear_init_ms":linear_init_ms,
                "paired":percentiles(&paired_trials,"render_ms"),"linear":percentiles(&linear_trials,"render_ms"),
                "paired_trials":paired_trials,"linear_trials":linear_trials}));
        }
    }
    let mut deliveries = Vec::new();
    let size = [640, 360];
    for temporal in [false, true] {
        if shared_diagnostic {
            continue;
        }
        let render = snapshot(temporal);
        let snapshot =
            AvExportSnapshot::with_audio(&render, AudioSourceMode::Silence, vec![]).unwrap();
        let paired = GpuContext::new().unwrap();
        let linear = GpuContext::new().unwrap();
        let backend = Paired(&paired);
        let mut paired_trials = Vec::new();
        let mut linear_trials = Vec::new();
        for iteration in 0..9 {
            for selected_linear in if iteration % 2 == 0 {
                [false, true]
            } else {
                [true, false]
            } {
                let mode = if selected_linear { "linear" } else { "paired" };
                let output = directory.join(format!("{mode}-{temporal}-{iteration}.mov"));
                let request = AvExportRequest {
                    output: output.clone(),
                    range: TimeRange::new(Time::new(1, 2).unwrap(), Time::new(1, 1).unwrap())
                        .unwrap(),
                    frame_rate: FrameRate::new(24, 1).unwrap(),
                    region: OutputRegion {
                        origin: [0.0; 2],
                        extent: [64.0, 32.0],
                        pixels: size,
                    },
                    background: [0.1, 0.2, 0.3],
                    clipping: ClippingPolicy::Reject,
                    chapters: ChapterPolicy::Omit,
                    outputs: vec![],
                };
                let selected: &dyn RenderBackend = if selected_linear { &linear } else { &backend };
                let start = Instant::now();
                let report = runtime
                    .export_av(
                        &snapshot,
                        &directory.join("unused.kronello"),
                        &[],
                        selected,
                        &request,
                    )
                    .unwrap();
                let ms = start.elapsed().as_secs_f64() * 1000.0;
                let mut transfers = serde_json::Map::new();
                for frame in &report.frames {
                    let values =
                        serde_json::to_value(frame.transfer_stats.as_ref().unwrap()).unwrap();
                    for (key, value) in values.as_object().unwrap() {
                        let previous = transfers
                            .get(key)
                            .and_then(serde_json::Value::as_u64)
                            .unwrap_or(0);
                        transfers.insert(
                            key.clone(),
                            json!(previous.checked_add(value.as_u64().unwrap()).unwrap()),
                        );
                    }
                }
                let trial = json!({"iteration":iteration,"ms":ms,"output":output,
                    "encoded_sha256":content_hash(&output).unwrap(),"transfers":transfers,"probe":report.probe,
                    "streaming":report.streaming,"sample_range":report.sample_range,"video_frames":report.frames.len()});
                if selected_linear {
                    linear_trials.push(trial)
                } else {
                    paired_trials.push(trial)
                }
            }
        }
        for (a, b) in paired_trials.iter().zip(&linear_trials) {
            assert_eq!(a["encoded_sha256"], b["encoded_sha256"]);
        }
        deliveries.push(json!({"size":size,"temporal":temporal,
            "paired":percentiles(&paired_trials,"ms"),"linear":percentiles(&linear_trials,"ms"),
            "paired_trials":paired_trials,"linear_trials":linear_trials}));
    }
    println!("{}",serde_json::to_string_pretty(&json!({"quiet_host":false,"same_binary":true,"shared_diagnostic":shared_diagnostic,
        "order":if shared_diagnostic {"alternating paired/linear per iteration; shared GPU context; first linear call already has warm scene cache"} else {"alternating paired/linear per iteration; independent GPU contexts per case"},
        "frame_timing":"render and waits excluding sink hash time; GPU creation separately; compilation included in first call",
        "delivery_timing":"production export including audio, rendering, native encoder, mux, validation and publication; output hashes excluded",
        "frame_cases":frame_cases,"deliveries":deliveries})).unwrap());
}
