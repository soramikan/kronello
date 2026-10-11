//! Release audio-only and mixed delivery probe, including decoded output bits.
use kronello_audio::{AudioClip, AudioSourceMode, ClippingPolicy, Gain};
use kronello_gpu::render_adapter::CpuReferenceBackend;
use kronello_media::*;
use kronello_model::*;
use kronello_render::*;
use kronello_time::{FrameRate, Rational, Time, TimeMap, TimeRange};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{cell::Cell, path::PathBuf, time::Instant};

fn t(n: i64, d: i64) -> Time {
    Time::new(n, d).unwrap()
}
struct CountedBackend(Cell<u64>);
impl RenderBackend for CountedBackend {
    fn name(&self) -> &str {
        "cpu_reference"
    }
    fn execute(&self, dag: &RenderDag) -> Result<BackendFrame, RenderError> {
        self.0.set(self.0.get() + 1);
        CpuReferenceBackend.execute(dag)
    }
}
fn main() {
    if cfg!(debug_assertions) {
        panic!("use --release");
    }
    let audio_file = PathBuf::from(std::env::args().nth(1).expect("audio input"))
        .canonicalize()
        .unwrap();
    let directory = PathBuf::from(std::env::args().nth(2).expect("new output directory"));
    std::fs::create_dir(&directory).unwrap();
    let runtime = MediaRuntime::load().unwrap();
    let probe = runtime.probe(&audio_file).unwrap();
    let stream = probe
        .streams
        .iter()
        .find(|s| s.kind == StreamKind::Audio)
        .unwrap();
    let asset = Asset {
        id: AssetId::new(),
        content_hash: content_hash(&audio_file).unwrap(),
        kind: AssetKind::Audio,
        locator: AssetLocator {
            relative: None,
            absolute: Some(audio_file.to_str().unwrap().into()),
        },
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
    };
    let mut cases = Vec::new();
    for (size, profile, mixed) in [
        ([64, 36], MovieProfile::FlacV1, false),
        ([640, 360], MovieProfile::FlacV1, false),
        ([1920, 1080], MovieProfile::FlacV1, false),
        ([64, 36], MovieProfile::Mp3V1, false),
        ([640, 360], MovieProfile::Mp3V1, false),
        ([1920, 1080], MovieProfile::Mp3V1, false),
        ([640, 360], MovieProfile::FlacV1, true),
    ] {
        let sequence = Sequence {
            id: SequenceId::new(),
            extent: DesignExtent::new(size[0] as f64, size[1] as f64).unwrap(),
            frame_rate: FrameRate::new(24, 1).unwrap(),
            audio_rate: kronello_time::SampleRate::HZ_48000,
            working_space: ColorSpace::LinearRec709,
            tracks: vec![Track {
                id: TrackId::new(),
                state: None,
                kind: TrackKind::Video,
                clips: vec![Clip {
                    id: ClipId::new(),
                    source_ref: SourceRef::Generator {
                        generator: SOLID_GENERATOR_ID.into(),
                        version: 1,
                        color: Color::from_srgb8([40, 90, 120], None),
                    },
                    timeline_range: TimeRange::new(Time::ZERO, t(1, 1)).unwrap(),
                    source_in: Time::ZERO,
                    time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
                    enabled: true,
                    audio_retime: AudioRetimePolicy::Reject,
                    reverse_sampling: None,
                    volume: None,
                    pan: None,
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
        };
        let target = RenderTarget::Sequence {
            sequence: sequence.id,
        };
        let project = Project {
            assets: vec![DocumentObject::Known(asset.clone())],
            sequences: vec![DocumentObject::Known(sequence)],
            ..Project::default()
        };
        let render =
            RenderSnapshot::for_target(&project, target, 0, RenderProfile::default()).unwrap();
        let clips = vec![AudioClip {
            asset: asset.id,
            stream_index: stream.index,
            placement: TimeRange::new(t(-1, 8), t(7, 8)).unwrap(),
            source_in: t(1, 8),
            gain: Gain::new(0.6).unwrap(),
        }];
        let primary = AvExportSnapshot::with_movie_profile(
            &render,
            AudioSourceMode::Explicit,
            clips.clone(),
            profile,
        )
        .unwrap();
        let movie =
            AvExportSnapshot::with_audio(&render, AudioSourceMode::Explicit, clips).unwrap();
        let snapshots = if mixed {
            vec![&primary, &movie]
        } else {
            vec![&primary]
        };
        let mut measurements = Vec::new();
        for iteration in 0..9 {
            let label = format!("{}-{:?}-{mixed}-{iteration}", size[0], profile);
            let file = directory.join(format!("{label}.{}", profile.container()));
            let request = AvExportRequest {
                output: file.clone(),
                range: TimeRange::new(t(1, 4), t(3, 4)).unwrap(),
                frame_rate: FrameRate::new(24, 1).unwrap(),
                region: OutputRegion {
                    origin: [0.0; 2],
                    extent: size.map(f64::from),
                    pixels: size,
                },
                background: [0.1, 0.2, 0.3],
                clipping: ClippingPolicy::Reject,
                chapters: ChapterPolicy::Omit,
                outputs: if mixed {
                    vec![DeliveryOutput {
                        output: directory.join(format!("{label}.mov")),
                        profile: MovieProfile::ProResPcm24,
                        background: [0.1, 0.2, 0.3],
                        chapters: ChapterPolicy::Omit,
                    }]
                } else {
                    vec![]
                },
            };
            let backend = CountedBackend(Cell::new(0));
            let mut checkpoints = Vec::new();
            let start = Instant::now();
            let report = runtime
                .export_delivery_with_checkpoint(
                    &snapshots,
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
            // The baseline native MP3 decoder rejected gapless timestamps.
            // Keep the same independent CLI oracle on both comparison sides,
            // outside timing; media APIs never silently fall back to this CLI.
            let (samples, decoded_start, decoder) = if profile == MovieProfile::Mp3V1 {
                let decoded = std::process::Command::new("ffmpeg")
                    .args(["-nostdin", "-v", "error", "-i"])
                    .arg(&file)
                    .args(["-map", "0:a:0", "-f", "f32le", "-acodec", "pcm_f32le", "-"])
                    .output()
                    .unwrap();
                assert!(
                    decoded.status.success(),
                    "{}",
                    String::from_utf8_lossy(&decoded.stderr)
                );
                assert_eq!(decoded.stdout.len() % 8, 0);
                (
                    decoded
                        .stdout
                        .chunks_exact(4)
                        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
                        .collect::<Vec<_>>(),
                    None,
                    "independent_ffmpeg_cli",
                )
            } else {
                let decoded = runtime.decode_audio(&file, 0).unwrap();
                (
                    decoded.buffer.samples().to_vec(),
                    Some(decoded.source_start),
                    "native_media",
                )
            };
            let mut hash = Sha256::new();
            for sample in &samples {
                assert!(sample.is_finite());
                hash.update(sample.to_bits().to_le_bytes());
            }
            measurements.push(json!({"ms":ms,"backend_calls":backend.0.get(),"video_metadata_frames":report.frames.len(),
                "sample_range":report.sample_range,"audio":report.audio,"decoded_frames":samples.len()/2,
                "decoded_start":decoded_start,"decoder":decoder,"decoded_sha256":format!("{:x}",hash.finalize()),
                "encoded_audio_sha256":content_hash(&file).unwrap(),
                "checkpoints":checkpoints,"streaming":report.streaming,"probe":report.probe}));
        }
        let mut warm: Vec<_> = measurements[1..]
            .iter()
            .map(|m| m["ms"].as_f64().unwrap())
            .collect();
        warm.sort_by(f64::total_cmp);
        cases.push(
            json!({"size":size,"profile":profile,"mixed":mixed,"cold_ms":measurements[0]["ms"],
            "warm_p50_ms":warm[4],"warm_p95_ms":warm[7],"measurements":measurements}),
        );
    }
    println!(
        "{}",
        json!({"path":"software render + native audio encode/probe/publication; decoded hash outside timing",
        "warm_quantiles":"8 warm samples; p50 upper-middle index 4, p95 max index 7","cases":cases})
    );
}
