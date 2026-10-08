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
#[test]
fn alac_pcm24_bit_exact_partial_final_frames_and_no_clobber() {
    let runtime = MediaRuntime::load().unwrap();
    let dir = tempfile::tempdir().unwrap();
    for count in [1, 31, 4095, 4096, 4097, 4804] {
        let range = TimeRange::new(t(0, 1), t(count, 48000)).unwrap();
        let id = AssetId::new();
        let samples = (0..count)
            .map(|i| {
                if i == 0 {
                    [-1.0, 1.0]
                } else if i == count - 1 {
                    [0.123456, -0.654321]
                } else {
                    [i as f32 / 8192.0, -(i as f32) / 8192.0]
                }
            })
            .collect();
        let sources = AudioSources::from([((id, 0), AudioBuffer::new(samples).unwrap())]);
        let clip = AudioClip {
            asset: id,
            stream_index: 0,
            placement: range,
            source_in: t(0, 1),
            gain: Gain::UNITY,
        };
        let bus = mix(&[clip], &sources, range).unwrap();
        let expected = bus.quantize_pcm24(ClippingPolicy::Reject).unwrap();
        let path = dir.path().join(format!("{count}.mp4"));
        let report = runtime
            .encode_alac(&bus, ClippingPolicy::Reject, &path)
            .unwrap();
        assert_eq!(report.codec, "alac");
        assert_eq!(report.frames, count as usize);
        let decoded = runtime.decode_audio(&path, 0).unwrap();
        assert_eq!(decoded.source_start, t(0, 1));
        assert_eq!(decoded.buffer.frame_count(), count as usize);
        for (got, want) in decoded.buffer.samples().iter().zip(&expected.samples) {
            assert_eq!(
                got.to_bits(),
                (*want as f32 / 2147483648.0).to_bits(),
                "{count} samples"
            );
        }
        let probe = runtime.probe(&path).unwrap();
        assert_eq!(probe.streams[0].start, Some(t(0, 1)));
        assert_eq!(probe.streams[0].duration, Some(t(count, 48000)));
        let before = std::fs::read(&path).unwrap();
        assert_eq!(
            runtime
                .encode_alac(&bus, ClippingPolicy::Reject, &path)
                .unwrap_err()
                .code(),
            "OUTPUT_EXISTS"
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
}
fn movie_roundtrip(profile: MovieProfile) {
    let runtime = MediaRuntime::load().unwrap();
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
        FrameRate::new(60000, 1001).unwrap(),
    ] {
        let range = TimeRange::new(
            rate.frame_to_time(1).unwrap(),
            rate.frame_to_time(4).unwrap(),
        )
        .unwrap();
        let req = AvExportRequest {
            output: dir.path().join(format!(
                "{rate:?}.{}",
                if profile == MovieProfile::Av1Mp4AlacV1 {
                    "mp4"
                } else {
                    "mov"
                }
            )),
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
        if profile == MovieProfile::HevcAlacV1 {
            let video = report
                .probe
                .streams
                .iter()
                .find(|s| s.kind == StreamKind::Video)
                .unwrap();
            assert_eq!(
                runtime.probe_codec_tag(&req.output, video.index).unwrap(),
                u32::from_le_bytes(*b"hvc1")
            );
        } else if profile == MovieProfile::Av1Mp4AlacV1 {
            assert_eq!(
                runtime.probe_codec_tag(&req.output, 0).unwrap(),
                u32::from_le_bytes(*b"av01")
            );
            assert_eq!(
                runtime.probe_codec_tag(&req.output, 2).unwrap_err().code(),
                "INVALID_MEDIA_INPUT"
            );
        }
        assert_eq!(report.movie_profile, Some(profile));
        assert_eq!(report.audio_profile_version, 3);
        assert_eq!(
            report.probe.export_snapshot_hash,
            av.content_hash().unwrap()
        );
        let expected_encoder = match profile {
            MovieProfile::Av1Mp4AlacV1 => "libsvtav1",
            MovieProfile::H264AlacV1 => "h264_videotoolbox",
            MovieProfile::HevcAlacV1 => "hevc_videotoolbox",
            _ => unreachable!(),
        };
        assert_eq!(report.video.encoder.as_deref(), Some(expected_encoder));
        assert_eq!(
            report.video.execution,
            if profile == MovieProfile::Av1Mp4AlacV1 {
                ExecutionKind::Software
            } else {
                ExecutionKind::Hardware
            }
        );
        assert_eq!(
            report.video.transfer_path,
            if profile == MovieProfile::Av1Mp4AlacV1 {
                "cpu_rgba_to_software_encoder"
            } else {
                "cpu_rgba_to_hardware_encoder"
            }
        );
        let decoded = runtime.decode_audio(&req.output, 1).unwrap();
        let samples = sample_range(range).unwrap();
        let n = (samples.end - samples.start) as usize;
        assert_eq!(decoded.source_start, t(0, 1));
        assert_eq!(decoded.buffer.stereo_frames().unwrap(), vec![[0.0, 0.0]; n]);
        let mut video = runtime.open_video(&req.output).unwrap();
        for i in 0..3 {
            let pts = rate.frame_to_time(i).unwrap();
            let frame = video.decode_at(pts).unwrap();
            assert_eq!(frame.pts, pts);
            assert_eq!(frame.end, rate.frame_to_time(i + 1).unwrap());
        }
        let end = rate.frame_to_time(3).unwrap();
        assert_eq!(video.decode_at(end).unwrap_err().code(), "FRAME_NOT_FOUND");
        let audio_last = t(n as i64 - 1, 48000);
        assert!(audio_last < end);
        let delta = end.checked_sub(t(n as i64, 48000)).unwrap();
        assert!(delta > t(-1, 48000) && delta < t(1, 48000));
        let before = std::fs::read(&req.output).unwrap();
        assert_eq!(
            runtime
                .export_av(
                    &restored,
                    Path::new("absent.kronello"),
                    &[],
                    &CpuReferenceBackend,
                    &req
                )
                .unwrap_err()
                .code(),
            "OUTPUT_EXISTS"
        );
        assert_eq!(std::fs::read(&req.output).unwrap(), before);
        println!("{}", serde_json::to_string(&report).unwrap());
    }
}
#[test]
fn av1_alac_movie_roundtrip_pts_duration_and_publication() {
    movie_roundtrip(MovieProfile::Av1Mp4AlacV1);
}
#[test]
#[ignore = "pending host run: requires physical VideoToolbox H.264 encoder"]
fn host_h264_alac_movie_roundtrip_pts_duration_and_publication() {
    movie_roundtrip(MovieProfile::H264AlacV1);
}
#[test]
#[ignore = "pending host run: requires physical VideoToolbox HEVC encoder"]
fn host_hevc_alac_movie_roundtrip_pts_duration_and_publication() {
    movie_roundtrip(MovieProfile::HevcAlacV1);
}
#[test]
fn delivery_profile_hash_versions_and_missing_hardware_are_closed() {
    let render = snapshot();
    let old =
        AvExportSnapshot::with_audio_profile(&render, AudioSourceMode::Silence, vec![], 3).unwrap();
    assert!(
        serde_json::to_value(&old)
            .unwrap()
            .get("movie_profile")
            .is_none()
    );
    let mut hashes = std::collections::BTreeSet::new();
    hashes.insert(old.content_hash().unwrap());
    for profile in [
        MovieProfile::Av1Mp4AlacV1,
        MovieProfile::H264AlacV1,
        MovieProfile::HevcAlacV1,
    ] {
        let av = AvExportSnapshot::with_movie_profile(
            &render,
            AudioSourceMode::Silence,
            vec![],
            profile,
        )
        .unwrap();
        assert!(hashes.insert(av.content_hash().unwrap()));
        let mut invalid = serde_json::to_value(av).unwrap();
        invalid["schema_version"] = 2.into();
        let invalid: AvExportSnapshot = serde_json::from_value(invalid).unwrap();
        assert_eq!(
            invalid.validate().unwrap_err().code(),
            "UNSUPPORTED_FEATURE"
        );
    }
    let mut capabilities = MediaRuntime::load().unwrap().capabilities().clone();
    capabilities.codecs.retain(|c| !c.hardware);
    for profile in [MovieProfile::H264AlacV1, MovieProfile::HevcAlacV1] {
        assert_eq!(
            capabilities
                .select_encoder(profile.video_codec())
                .unwrap_err()
                .code(),
            "ENCODER_UNAVAILABLE"
        );
    }
}
