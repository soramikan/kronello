//! AUDIO-005: versioned AAC-LC and Opus delivery audio profiles.
//! Lossy outputs are verified by exact decoded length, zero origin, and
//! bounded signal error rather than sample equality.
use kronello_audio::*;
use kronello_gpu::render_adapter::CpuReferenceBackend;
use kronello_media::*;
use kronello_model::*;
use kronello_render::*;
use kronello_time::{Duration, FrameRate, Rational, TimeRange};
use std::path::Path;

fn t(n: i64, d: i64) -> Rational {
    Rational::new(n, d).unwrap()
}

fn bus_of(samples: Vec<[f32; 2]>) -> (Bus, usize) {
    let count = samples.len();
    let range = TimeRange::new(t(0, 1), t(count as i64, 48000)).unwrap();
    let id = AssetId::new();
    let sources = AudioSources::from([((id, 0), AudioBuffer::new(samples).unwrap())]);
    let clip = AudioClip {
        asset: id,
        stream_index: 0,
        placement: range,
        source_in: t(0, 1),
        gain: Gain::UNITY,
    };
    (mix(&[clip], &sources, range).unwrap(), count)
}

fn signal(kind: &str, count: usize) -> Vec<[f32; 2]> {
    (0..count)
        .map(|i| {
            let s = i as f64;
            match kind {
                "silence" => [0.0, 0.0],
                "sine" => {
                    let v = (2.0 * std::f64::consts::PI * 440.0 * s / 48000.0).sin() * 0.8;
                    [v as f32, v as f32]
                }
                "multitone" => {
                    let v = ((2.0 * std::f64::consts::PI * 440.0 * s / 48000.0).sin()
                        + 0.5 * (2.0 * std::f64::consts::PI * 1760.0 * s / 48000.0).sin()
                        + 0.25 * (2.0 * std::f64::consts::PI * 7040.0 * s / 48000.0).sin())
                        * 0.4;
                    [v as f32, (v * 0.9) as f32]
                }
                "pulse" => {
                    if i == count / 2 {
                        [0.95, -0.95]
                    } else {
                        [0.0, 0.0]
                    }
                }
                _ => unreachable!(),
            }
        })
        .collect()
}

/// Normalized cross-correlation at zero lag; 1.0 is an identical direction.
fn cosine_similarity(want: &[[f32; 2]], got: &[[f32; 2]]) -> f64 {
    let (mut dot, mut wa, mut ga) = (0.0, 0.0, 0.0);
    for (w, g) in want.iter().zip(got) {
        for ch in 0..2 {
            dot += f64::from(w[ch]) * f64::from(g[ch]);
            wa += f64::from(w[ch]) * f64::from(w[ch]);
            ga += f64::from(g[ch]) * f64::from(g[ch]);
        }
    }
    dot / (wa.sqrt() * ga.sqrt())
}

/// Lossy delivery profiles are verified against the pinned FFmpeg 9 ABI;
/// older system runtimes do not round-trip priming/discard identically.
fn delivery_runtime() -> Option<MediaRuntime> {
    let runtime = MediaRuntime::load().unwrap();
    if runtime.capabilities().ffmpeg_version.starts_with("9.") {
        return Some(runtime);
    }
    eprintln!(
        "skipping: delivery audio requires the pinned FFmpeg 9 runtime, found {}",
        runtime.capabilities().ffmpeg_version
    );
    None
}

#[test]
fn aac_and_opus_delivery_audio_exact_length_bounded_error() {
    let Some(runtime) = delivery_runtime() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    for codec in [DeliveryAudioCodec::Aac, DeliveryAudioCodec::Opus] {
        let (extension, codec_name) = match codec {
            DeliveryAudioCodec::Aac => ("mp4", "aac"),
            DeliveryAudioCodec::Opus => ("webm", "opus"),
            DeliveryAudioCodec::Alac => unreachable!(),
        };
        for kind in ["silence", "sine", "multitone", "pulse"] {
            // Opus discard padding requires more than one packet: content at
            // or below the priming margin (frame_size - pre_skip) is refused
            // below rather than silently padded.
            let counts: &[usize] = match codec {
                DeliveryAudioCodec::Opus => &[48000, 33600, 961],
                _ => &[48000, 33600, 961, 2],
            };
            for &count in counts {
                let source = signal(kind, count);
                let (bus, _) = bus_of(source.clone());
                let path = dir
                    .path()
                    .join(format!("{codec_name}-{kind}-{count}.{extension}"));
                let report = runtime
                    .encode_delivery_audio(codec, &bus, ClippingPolicy::Reject, &path)
                    .unwrap();
                assert_eq!(report.codec, codec_name);
                assert_eq!(report.frames, count);
                let probe = runtime.probe(&path).unwrap();
                let audio = probe
                    .streams
                    .iter()
                    .find(|s| s.kind == StreamKind::Audio)
                    .unwrap();
                assert_eq!(audio.codec, codec_name);
                assert_eq!(audio.sample_rate, Some(48_000));
                assert_eq!(audio.channels, Some(2));
                // Priming/padding must be signalled so decoding lands at zero
                // with exactly the input sample count, never padded.
                let decoded = runtime
                    .decode_audio(&path, 0)
                    .unwrap_or_else(|e| panic!("{codec_name} {kind} {count}: {e:?}"));
                assert_eq!(decoded.source_start, t(0, 1), "{codec_name} {kind} {count}");
                assert_eq!(decoded.source_rate, 48_000, "{codec_name} {kind} {count}");
                assert_eq!(
                    decoded.buffer.frame_count(),
                    count,
                    "{codec_name} {kind} {count}"
                );
                let got = decoded.buffer.stereo_frames().unwrap();
                if kind == "silence" {
                    assert!(
                        got.iter()
                            .flatten()
                            .all(|v| v.is_finite() && v.abs() < 0.001),
                        "{codec_name} silence leak"
                    );
                } else if kind == "pulse" {
                    let peak = got
                        .iter()
                        .enumerate()
                        .max_by(|a, b| {
                            a.1[0]
                                .abs()
                                .partial_cmp(&b.1[0].abs())
                                .unwrap_or(std::cmp::Ordering::Equal)
                        })
                        .map(|(i, _)| i)
                        .unwrap();
                    assert_eq!(peak, count / 2, "{codec_name} pulse position {count}");
                } else {
                    let similarity = cosine_similarity(&source, &got);
                    assert!(
                        similarity > 0.97,
                        "{codec_name} {kind} similarity {similarity} ({count} samples)"
                    );
                }
            }
        }
        // Inputs at or below the priming margin cannot signal discard padding
        // on a single packet: the encode must fail with a typed refusal.
        if codec == DeliveryAudioCodec::Opus {
            let (bus, _) = bus_of(signal("sine", 640));
            let error = runtime
                .encode_delivery_audio(
                    codec,
                    &bus,
                    ClippingPolicy::Reject,
                    &dir.path().join("short.webm"),
                )
                .unwrap_err();
            assert!(
                error.to_string().contains("Opus input"),
                "short Opus must be a typed refusal, got {error:?}"
            );
        }
        // Lossy intermediates must never clobber a published file.
        let path = dir.path().join(format!("exists.{extension}"));
        let (bus, _) = bus_of(signal("sine", 4800));
        runtime
            .encode_delivery_audio(codec, &bus, ClippingPolicy::Reject, &path)
            .unwrap();
        let before = std::fs::read(&path).unwrap();
        assert_eq!(
            runtime
                .encode_delivery_audio(codec, &bus, ClippingPolicy::Reject, &path)
                .unwrap_err()
                .code(),
            "OUTPUT_EXISTS"
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
}

fn snapshot() -> RenderSnapshot {
    let id = CompositionId::new();
    let project = Project {
        compositions: vec![DocumentObject::Known(Composition {
            id,
            duration: Duration::new(t(1, 1)).unwrap(),
            design_extent: DesignExtent::new(64.0, 64.0).unwrap(),
            edit_rate: FrameRate::new(24, 1).unwrap(),
            root_nodes: vec![],
            nodes: vec![],
            properties: vec![],
        })],
        ..Project::default()
    };
    RenderSnapshot::new(&project, id, 7, Default::default()).unwrap()
}

fn lossy_movie_roundtrip(profile: MovieProfile) {
    let Some(runtime) = delivery_runtime() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let render = snapshot();
    let av =
        AvExportSnapshot::with_movie_profile(&render, AudioSourceMode::Silence, vec![], profile)
            .unwrap();
    let restored: AvExportSnapshot =
        serde_json::from_slice(&serde_json::to_vec(&av).unwrap()).unwrap();
    assert_eq!(restored.content_hash().unwrap(), av.content_hash().unwrap());
    for rate in [
        FrameRate::new(24, 1).unwrap(),
        FrameRate::new(30000, 1001).unwrap(),
    ] {
        let range = TimeRange::new(
            rate.frame_to_time(1).unwrap(),
            rate.frame_to_time(4).unwrap(),
        )
        .unwrap();
        let req = AvExportRequest {
            output: dir.path().join(format!("{rate:?}.{}", profile.container())),
            range,
            frame_rate: rate,
            region: OutputRegion {
                origin: [0.0, 0.0],
                extent: [64.0, 64.0],
                pixels: [64, 64],
            },
            background: [0.1, 0.2, 0.3],
            clipping: ClippingPolicy::Reject,
        };
        let report = runtime
            .export_av(
                &restored,
                Path::new("absent.kronello"),
                &[],
                &CpuReferenceBackend,
                &req,
            )
            .unwrap();
        report.probe.verify_movie(profile).unwrap();
        let expected_audio = match profile {
            MovieProfile::Av1WebmOpusV1 => "opus",
            _ => "aac",
        };
        let audio = report
            .probe
            .streams
            .iter()
            .find(|s| s.kind == StreamKind::Audio)
            .unwrap();
        assert_eq!(audio.codec, expected_audio);
        // A/V sync and exact terminal sample count through the remuxed movie.
        let decoded = runtime.decode_audio(&req.output, audio.index).unwrap();
        assert_eq!(decoded.source_start, t(0, 1));
        assert_eq!(decoded.buffer.frame_count(), report.audio.frames);
    }
}

#[test]
#[ignore = "pending host run: requires physical VideoToolbox H.264 encoder"]
fn h264_aac_movie_profile() {
    lossy_movie_roundtrip(MovieProfile::H264AacV1);
}
#[test]
#[ignore = "pending host run: requires physical VideoToolbox HEVC encoder"]
fn hevc_aac_movie_profile() {
    lossy_movie_roundtrip(MovieProfile::HevcAacV1);
}
#[test]
fn av1_mp4_aac_movie_profile() {
    lossy_movie_roundtrip(MovieProfile::Av1Mp4AacV1);
}
#[test]
fn av1_webm_opus_movie_profile() {
    lossy_movie_roundtrip(MovieProfile::Av1WebmOpusV1);
}
