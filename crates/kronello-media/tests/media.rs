use kronello_media::*;
use kronello_time::Rational;
use std::path::{Path, PathBuf};
fn r(n: i64, d: i64) -> Rational {
    Rational::new(n, d).unwrap()
}
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures/generated/media")
}
#[test]
fn cfr_vfr_bframes_seek_exact_intervals_and_drain() {
    let runtime = MediaRuntime::load().unwrap();
    let cases = [
        ("cfr-24-1.nut", (0..6).map(|i| r(i, 24)).collect::<Vec<_>>()),
        ("cfr-25-1.nut", (0..6).map(|i| r(i, 25)).collect()),
        ("cfr-30-1.nut", (0..6).map(|i| r(i, 30)).collect()),
        (
            "cfr-30000-1001.nut",
            (0..6).map(|i| r(i * 1001, 30000)).collect(),
        ),
        (
            "cfr-60000-1001.nut",
            (0..6).map(|i| r(i * 1001, 60000)).collect(),
        ),
        (
            "vfr.nut",
            [0, 1, 3, 6, 10, 15].into_iter().map(|i| r(i, 30)).collect(),
        ),
        ("bframes.nut", (1..7).map(|i| r(i, 24)).collect()),
    ];
    for (name, times) in cases {
        let mut decoder = runtime.open_video(&fixtures().join(name)).unwrap();
        let metadata = decoder.stream_metadata().unwrap();
        assert_eq!(metadata.width, Some(16));
        assert!(metadata.time_base > Rational::ZERO);
        assert_eq!(metadata.pixel_format.as_deref(), Some("yuv420p"));
        // Reverse, repeated and forward requests must be independent of history.
        for i in [4, 0, 3, 1, 5, 2, 0, 5] {
            let start = times[i];
            let end = times.get(i + 1).copied().unwrap_or(
                start
                    .checked_add(if name == "vfr.nut" {
                        r(1, 30)
                    } else {
                        times[1].checked_sub(times[0]).unwrap()
                    })
                    .unwrap(),
            );
            let middle = start
                .checked_add(end)
                .unwrap()
                .checked_div(r(2, 1))
                .unwrap();
            for query in [start, middle, end.checked_sub(r(1, 1000000)).unwrap()] {
                let frame = decoder
                    .decode_at(query)
                    .unwrap_or_else(|e| panic!("{name} {query:?}: {e}"));
                assert_eq!(frame.pts, start, "{name}");
                assert_eq!(frame.end, end, "{name}");
                assert_eq!(frame.pixels.len(), 384);
            }
        }
        let report = decoder.path_report();
        assert_eq!(report.execution, ExecutionKind::Software);
        assert_eq!(report.input_pixel_format, "yuv420p");
        assert!(report.transfers.cpu_copy_bytes > 0);
        assert_eq!(report.transfers.cpu_readback_bytes, 0);
        assert_eq!(
            decoder
                .decode_at(times[0].checked_sub(r(1, 100)).unwrap())
                .unwrap_err()
                .code(),
            "FRAME_NOT_FOUND"
        );
        let last = times[5]
            .checked_add(if name == "vfr.nut" {
                r(1, 30)
            } else {
                times[1].checked_sub(times[0]).unwrap()
            })
            .unwrap();
        assert_eq!(
            decoder.decode_at(last).unwrap_err().code(),
            "FRAME_NOT_FOUND"
        );
    }
}
#[test]
fn native_hdr_preserves_ten_bit_planes_and_tags() {
    let runtime = MediaRuntime::load().unwrap();
    for (file, transfer) in [("hdr-pq.mkv", "smpte2084"), ("hdr-hlg.mkv", "arib-std-b67")] {
        let mut decoder = runtime.open_video(&fixtures().join(file)).unwrap();
        let frame = decoder.decode_at(r(0, 1)).unwrap();
        assert_eq!(frame.pixel_format, "yuv420p10le");
        assert_eq!(frame.pixels.len(), 768);
        assert_eq!(frame.color_transfer, transfer);
        assert_eq!(frame.color_primaries, "bt2020");
        assert_eq!(frame.color_matrix, "bt2020nc");
        assert_eq!(frame.color_range, "tv");
        assert_eq!(
            u16::from_le_bytes(frame.pixels[0..2].try_into().unwrap()),
            64
        );
        assert_eq!(
            u16::from_le_bytes(frame.pixels[30..32].try_into().unwrap()),
            940
        );
    }
}
#[test]
fn loaded_libraries_report_configuration_and_override_errors() {
    let runtime = MediaRuntime::load().unwrap();
    let cap = runtime.capabilities();
    assert_eq!(cap.libraries.len(), 4);
    assert!(cap.codecs.iter().any(|c| c.decoder && c.name == "mpeg4"));
    assert!(!cap.ffmpeg_version.is_empty());
    assert_eq!(cap.development_only, !cap.distribution_eligible);
    if cap
        .libraries
        .iter()
        .any(|l| l.configuration.contains("--enable-gpl"))
    {
        assert!(!cap.distribution_eligible);
        assert_eq!(
            cap.verify_distribution().unwrap_err().code(),
            "DISTRIBUTION_LICENSE_ERROR"
        );
    }
    let substituted = MediaRuntime::load_directory(cap.library_directory.clone(), true).unwrap();
    assert!(substituted.capabilities().substituted);
    assert_eq!(substituted.capabilities().libraries, cap.libraries);
    assert_eq!(
        MediaRuntime::load_directory(PathBuf::from("/no/such/ffmpeg"), true)
            .err()
            .unwrap()
            .code(),
        "FFMPEG_UNAVAILABLE"
    );
}
#[test]
fn software_codecs_encode_and_missing_hardware_is_typed() {
    let runtime = MediaRuntime::load().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let frames = (0..4)
        .map(|i| EncodeFrame {
            pts: r(i, 24),
            rgba: [16_u8, 80, 160, 255].repeat(64 * 64),
        })
        .collect::<Vec<_>>();
    for codec in [EncodeCodec::Av1, EncodeCodec::ProRes] {
        let request = EncodeRequest {
            output: temp.path().join(format!("{codec:?}.media")),
            codec,
            width: 64,
            height: 64,
            time_base: r(1, 24),
        };
        let report = runtime.encode_video(&request, &frames).unwrap();
        assert_eq!(report.execution, ExecutionKind::Software);
        assert!(report.encoder.is_some());
        assert_eq!(report.transfers.cpu_copy_bytes, 0);
        assert_eq!(report.transfers.cpu_conversion_input_bytes, 65536);
        assert_eq!(report.transfers.cpu_upload_bytes, 0);
        let mut decoder = runtime.open_video(&request.output).unwrap();
        // MOV track timescale preserves the input rational time base.
        let frame = decoder.decode_at(r(0, 1)).unwrap();
        assert_eq!(frame.pts, r(0, 1));
        assert_eq!(frame.width, 64);
        for i in 0..4 {
            assert_eq!(decoder.decode_at(r(i, 24)).unwrap().pts, r(i, 24));
        }
        assert_eq!(
            runtime.encode_video(&request, &frames).unwrap_err().code(),
            "OUTPUT_EXISTS"
        );
    }
    let mut cap = runtime.capabilities().clone();
    cap.codecs.retain(|c| !c.hardware);
    // Even if development libx264/libx265 exist, they are never an alternative.
    for codec in [EncodeCodec::H264, EncodeCodec::Hevc] {
        let request = EncodeRequest {
            output: temp.path().join("unavailable.mkv"),
            codec,
            width: 64,
            height: 64,
            time_base: r(1, 24),
        };
        let error = runtime
            .encode_video_with_capabilities(&request, &frames, &cap)
            .unwrap_err();
        assert_eq!(error.code(), "ENCODER_UNAVAILABLE");
        assert!(matches!(
            error,
            MediaError::EncoderUnavailable { ffmpeg: None, .. }
        ));
        assert!(!request.output.exists());
    }
}
#[test]
fn rejects_invalid_pts_alpha_and_preserves_output() {
    let runtime = MediaRuntime::load().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let request = EncodeRequest {
        output: temp.path().join("bad.mov"),
        codec: EncodeCodec::ProRes,
        width: 16,
        height: 16,
        time_base: r(1, 24),
    };
    for frame in [
        EncodeFrame {
            pts: r(1, 48),
            rgba: [0_u8, 0, 0, 255].repeat(256),
        },
        EncodeFrame {
            pts: r(0, 1),
            rgba: vec![0; 1024],
        },
        EncodeFrame {
            pts: r(0, 1),
            rgba: vec![255; 3],
        },
    ] {
        assert_eq!(
            runtime.encode_video(&request, &[frame]).unwrap_err().code(),
            "INVALID_MEDIA_INPUT"
        );
        assert!(!request.output.exists());
    }
}

#[test]
fn runtime_environment_override_is_used_without_fallback() {
    match std::env::var("KRONELLO_MEDIA_OVERRIDE_CHILD").as_deref() {
        Ok("invalid") => assert_eq!(
            MediaRuntime::load().err().unwrap().code(),
            "FFMPEG_UNAVAILABLE"
        ),
        Ok("valid") => assert!(MediaRuntime::load().unwrap().capabilities().substituted),
        _ => {
            let runtime = MediaRuntime::load().unwrap();
            for (mode, path) in [
                ("valid", runtime.capabilities().library_directory.clone()),
                (
                    "invalid",
                    PathBuf::from("/missing/kronello-ffmpeg-override"),
                ),
            ] {
                let output = std::process::Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "runtime_environment_override_is_used_without_fallback",
                    ])
                    .env("KRONELLO_MEDIA_OVERRIDE_CHILD", mode)
                    .env("KRONELLO_FFMPEG_LIB_DIR", path)
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
    }
}

#[test]
fn videotoolbox_hybrid_registration_is_hardware_capable() {
    let runtime = MediaRuntime::load().unwrap();
    for codec in [EncodeCodec::H264, EncodeCodec::Hevc] {
        let name = match codec {
            EncodeCodec::H264 => "h264_videotoolbox",
            _ => "hevc_videotoolbox",
        };
        if let Some(detected) = runtime
            .capabilities()
            .codecs
            .iter()
            .find(|c| c.encoder && c.name == name)
        {
            assert!(
                detected.hardware,
                "VideoToolbox HYBRID registrations must be selectable with allow_sw=0"
            );
            assert_eq!(
                runtime.capabilities().select_encoder(codec).unwrap().name,
                name
            );
        } else {
            assert_eq!(
                runtime
                    .capabilities()
                    .select_encoder(codec)
                    .unwrap_err()
                    .code(),
                "ENCODER_UNAVAILABLE"
            );
        }
    }
}
