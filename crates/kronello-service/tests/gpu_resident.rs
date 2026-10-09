#![cfg(target_os = "macos")]
use kronello_media::{MediaRuntime, content_hash};
use kronello_model::*;
use kronello_render::{OutputRegion, RenderTarget};
use kronello_service::*;
use kronello_time::{FrameRate, Rational, SampleRate, Time, TimeMap, TimeRange};
use std::path::Path;

fn time(n: i64, d: i64) -> Time {
    Time::new(n, d).unwrap()
}
fn create_project(movie: &Path, project_path: &Path) -> (SequenceId, Time, Time) {
    create_project_as(movie, project_path, false)
}
fn create_project_as(
    movie: &Path,
    project_path: &Path,
    forge_sdr: bool,
) -> (SequenceId, Time, Time) {
    let runtime = MediaRuntime::load().unwrap();
    let mut decoder = runtime.open_video(movie).unwrap();
    let mut stream = decoder.stream_metadata().unwrap();
    if forge_sdr {
        stream.pixel_format = Some("yuv420p".into());
        stream.color_primaries = Some("bt709".into());
        stream.color_transfer = Some("bt709".into());
        stream.color_matrix = Some("bt709".into());
        stream.color_range = Some("tv".into());
    }
    let source_in = stream.start_time.unwrap_or(Time::ZERO);
    let duration = stream.duration.expect("generated movie duration");
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
    let clip = Clip {
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
    let sequence = Sequence {
        id: SequenceId::new(),
        extent: DesignExtent::new(64.0, 64.0).unwrap(),
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
    project.assets.push(DocumentObject::Known(asset));
    project.sequences.push(DocumentObject::Known(sequence));
    Service::new(BackendSelection::CpuReference)
        .dispatch(Request::ProjectCreate(CreateRequest {
            plan_hash: None,
            idempotency_key: None,
            project: project_path.into(),
            document: project,
        }))
        .unwrap();
    (id, source_in, duration)
}
fn frame(path: &Path, sequence: SequenceId, at: Time, selection: BackendSelection) -> FrameResult {
    let request = FrameRenderRequest {
        input: RenderInput {
            project: path.into(),
            composition: None,
            target: Some(RenderTarget::Sequence { sequence }),
            region: OutputRegion {
                origin: [0.0; 2],
                extent: [64.0; 2],
                pixels: [64; 2],
            },
            profile: Default::default(),
            fonts: vec![],
            media_proxies: kronello_render::MediaProxyMode::Off,
            luts: vec![],
        },
        time: at,
        backend: Some(selection),
    };
    let ResultData::Frame(result) = Service::new(BackendSelection::CpuReference)
        .dispatch(Request::RenderFrame(request))
        .unwrap()
    else {
        panic!("frame expected")
    };
    *result
}
fn verify_codec(encoder: &str, filter: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let temp = tempfile::tempdir().unwrap();
    for source in ["cfr-24-1.nut", "vfr.nut", "bframes.nut"] {
        let movie = temp.path().join(format!("{source}.mov"));
        let status = std::process::Command::new(root.join("target/native/ffmpeg-lgpl/bin/ffmpeg"))
            .args(["-v", "error", "-nostdin", "-copyts", "-i"])
            .arg(root.join("target/fixtures/generated/media").join(source))
            .args([
                "-an",
                "-tag:v",
                if encoder == "hevc_videotoolbox" {
                    "hvc1"
                } else {
                    "avc1"
                },
                "-vf",
                &format!("{filter},setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709:range=limited"),
                "-fps_mode",
                "passthrough",
                "-c:v",
                encoder,
                "-allow_sw",
                "0",
                "-bf",
                "2",
                "-b:v",
                "10000000",
                "-movflags",
                "write_colr",
                "-color_primaries",
                "bt709",
                "-color_trc",
                "bt709",
                "-colorspace",
                "bt709",
                "-color_range",
                "tv",
            ])
            .arg(&movie)
            .status()
            .unwrap();
        assert!(status.success(), "hardware codec fixture creation {source}");
        let project = temp.path().join(format!("{source}.kronello"));
        let (id, origin, duration) = create_project(&movie, &project);
        let times = if source == "vfr.nut" {
            vec![
                time(0, 1),
                time(1, 30),
                time(3, 30),
                time(6, 30),
                time(10, 30),
            ]
        } else if source == "bframes.nut" {
            vec![
                time(1, 24),
                time(2, 24),
                time(3, 24),
                time(4, 24),
                time(5, 24),
            ]
        } else {
            vec![
                time(0, 1),
                time(1, 24),
                time(2, 24),
                time(3, 24),
                time(4, 24),
            ]
        };
        for selection in [
            BackendSelection::GpuResidentBgra8,
            BackendSelection::GpuResidentNv12,
        ] {
            for i in [3, 0, 2, 1, 3] {
                let actual = frame(&project, id, times[i], selection);
                let expected = frame(&project, id, times[i], BackendSelection::CpuReference);
                let max = actual
                    .linear
                    .iter()
                    .zip(&expected.linear)
                    .flat_map(|(a, b)| a.iter().zip(b.iter()).map(|(a, b)| (a - b).abs()))
                    .fold(0.0f32, f32::max);
                eprintln!(
                    "GPU003 comparison {source} {selection:?} index={i} actual0={:?} expected0={:?} actualmiddle={:?} expectedmiddle={:?}",
                    actual.linear[0],
                    expected.linear[0],
                    actual.linear[2048],
                    expected.linear[2048]
                );
                let native_format = if selection == BackendSelection::GpuResidentBgra8 {
                    kronello_framebridge::resident::ResidentFormat::Bgra8
                } else {
                    kronello_framebridge::resident::ResidentFormat::Nv12VideoRange
                };
                let native = kronello_framebridge::resident::decode_file_with_interval(
                    &movie,
                    0,
                    (
                        times[i].checked_add(origin).unwrap().numerator(),
                        times[i].checked_add(origin).unwrap().denominator(),
                    ),
                    native_format,
                    [
                        (origin.numerator(), origin.denominator()),
                        (
                            origin.checked_add(duration).unwrap().numerator(),
                            origin.checked_add(duration).unwrap().denominator(),
                        ),
                    ],
                )
                .unwrap();
                let reference = native
                    .read_for_validation(
                        kronello_gpu::WorkingSpace::LinearRec709,
                        kronello_framebridge::resident::VideoTransfer::Bt709,
                    )
                    .unwrap();
                let native_error = actual
                    .linear
                    .iter()
                    .zip(&reference.pixels)
                    .flat_map(|(a, b)| a.iter().zip(b.iter()).map(|(a, b)| (a - b).abs()))
                    .fold(0.0f32, f32::max);
                let source_time = times[i].checked_add(origin).unwrap();
                let raw_native = kronello_framebridge::resident::decode_file_with_interval(
                    &movie,
                    0,
                    (source_time.numerator(), source_time.denominator()),
                    kronello_framebridge::resident::ResidentFormat::Nv12VideoRange,
                    [
                        (origin.numerator(), origin.denominator()),
                        (
                            origin.checked_add(duration).unwrap().numerator(),
                            origin.checked_add(duration).unwrap().denominator(),
                        ),
                    ],
                )
                .unwrap()
                .read_for_validation(
                    kronello_gpu::WorkingSpace::LinearRec709,
                    kronello_framebridge::resident::VideoTransfer::Bt709,
                )
                .unwrap();
                let runtime = MediaRuntime::load().unwrap();
                let mut decoder = runtime.open_video(&movie).unwrap();
                let raw_cpu = decoder.decode_at(source_time).unwrap();
                assert_eq!(raw_cpu.pixel_format, "yuv420p");
                assert_eq!(
                    i128::from(native.presentation_time.value)
                        * i128::from(raw_cpu.pts.denominator()),
                    i128::from(raw_cpu.pts.numerator())
                        * i128::from(native.presentation_time.timescale),
                    "native/software exact PTS mismatch"
                );
                if filter.contains("geq") {
                    assert!(
                        max < 0.045,
                        "uniform time/color fixture mismatch {source} {selection:?} max={max}"
                    );
                }

                let y_error = raw_native.planes[0]
                    .iter()
                    .zip(&raw_cpu.pixels[..4096])
                    .map(|(a, b)| a.abs_diff(*b))
                    .max()
                    .unwrap();
                let chroma_error = raw_native.planes[1]
                    .chunks_exact(2)
                    .enumerate()
                    .flat_map(|(i, p)| {
                        [
                            p[0].abs_diff(raw_cpu.pixels[4096 + i]),
                            p[1].abs_diff(raw_cpu.pixels[5120 + i]),
                        ]
                    })
                    .max()
                    .unwrap();
                assert!(
                    y_error <= 2 && chroma_error <= 2,
                    "native/software decoded YUV mismatch Y={y_error} chroma={chroma_error}"
                );
                eprintln!(
                    "GPU003 raw decode comparison y_error={y_error}, chroma_error={chroma_error}, validationCPUbytes={}",
                    raw_native.cpu_pixel_bytes
                );
                let max_index = actual
                    .linear
                    .iter()
                    .zip(&expected.linear)
                    .enumerate()
                    .max_by(|(_, (a, b)), (_, (c, d))| {
                        a.iter()
                            .zip(b.iter())
                            .map(|(a, b)| (a - b).abs())
                            .fold(0.0f32, f32::max)
                            .total_cmp(
                                &c.iter()
                                    .zip(d.iter())
                                    .map(|(c, d)| (c - d).abs())
                                    .fold(0.0f32, f32::max),
                            )
                    })
                    .unwrap()
                    .0;
                eprintln!(
                    "GPU003 native oracle pts={:?}, max={native_error}, validationCPUbytes={}, swscale all-pixel max={max} at ({},{}), hardware={:?}, swscale={:?}, native={:?}",
                    native.presentation_time,
                    reference.cpu_pixel_bytes,
                    max_index % 64,
                    max_index / 64,
                    actual.linear[max_index],
                    expected.linear[max_index],
                    reference.pixels[max_index]
                );
                assert!(
                    native_error < 0.002,
                    "{source} {selection:?} {i} native conversion/composite mismatch={native_error}"
                );
                assert!(
                    actual
                        .metadata
                        .input_path
                        .starts_with("hardware_videotoolbox")
                );
                let stats = actual.metadata.transfer_stats.unwrap();
                assert_eq!(stats.cpu_upload_pixel_bytes, 0);
                // Scene root/output GPU copies are reported explicitly.
                assert_eq!(stats.gpu_copy_bytes, 0);
                assert_eq!(stats.gpu_copy_operations, 0);
                assert_eq!(stats.gpu_readback_operations, 3);
                assert_eq!(stats.gpu_wait_operations, 3);
                assert!(stats.cpu_upload_control_bytes >= 48);
                assert!(stats.gpu_readback_bytes > 0);
                eprintln!(
                    "GPU003 {source} {selection:?} time={:?} max={max}, transfers={stats:?}",
                    times[i]
                );
            }
        }
        let runtime = MediaRuntime::load().unwrap();
        let mut decoder = runtime.open_video(&movie).unwrap();
        let metadata = decoder.stream_metadata().unwrap();
        let last = metadata
            .start_time
            .unwrap_or(Time::ZERO)
            .checked_add(metadata.duration.unwrap())
            .unwrap();
        assert_eq!(
            kronello_media::validate_resident_source_time(&metadata, last)
                .unwrap_err()
                .code(),
            "FRAME_NOT_FOUND"
        );
        assert_eq!(
            kronello_media::validate_resident_source_time(
                &metadata,
                metadata
                    .start_time
                    .unwrap_or(Time::ZERO)
                    .checked_sub(time(1, 1000000))
                    .unwrap()
            )
            .unwrap_err()
            .code(),
            "FRAME_NOT_FOUND"
        );
        let error = kronello_framebridge::resident::decode_file(
            &movie,
            0,
            (1, 1),
            kronello_framebridge::resident::ResidentFormat::Nv12VideoRange,
        );
        assert!(error.is_err(), "half-open video source end must fail");
    }
}

#[test]
#[ignore = "requires LGPL FFmpeg fixtures, actual VT hardware encoder/decoder and Metal"]
fn service_hardware_decode_resident_render_formats_cfr_vfr_bframes() {
    verify_codec("h264_videotoolbox", "scale=64:64");
}
#[test]
#[ignore = "requires actual HEVC VideoToolbox hardware and Metal"]
fn service_hevc_resident_formats_cfr_vfr_bframes() {
    verify_codec("hevc_videotoolbox", "scale=64:64");
}
#[test]
#[ignore = "requires actual VideoToolbox hardware and Metal"]
fn service_exact_pts_uniform_temporal_color_fixture() {
    verify_codec(
        "h264_videotoolbox",
        "scale=64:64,geq=lum='64+20*N':cb='112+2*N':cr='144-2*N'",
    );
}

#[test]
#[ignore = "requires actual HEVC VideoToolbox hardware and Metal"]
fn actual_hdr_ten_bit_full_range_reject_forged_sdr_locks() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let temp = tempfile::tempdir().unwrap();
    for (name, pixel, primaries, transfer, matrix, range) in [
        ("pq10", "p010le", "bt2020", "smpte2084", "bt2020nc", "tv"),
        ("sdr10", "p010le", "bt709", "bt709", "bt709", "tv"),
        ("full8", "nv12", "bt709", "bt709", "bt709", "pc"),
        ("srgb8", "nv12", "bt709", "iec61966-2-1", "bt709", "tv"),
    ] {
        let movie = temp.path().join(format!("{name}.mov"));
        let status = std::process::Command::new(root.join("target/native/ffmpeg-lgpl/bin/ffmpeg"))
            .args(["-v", "error", "-nostdin", "-i"])
            .arg(root.join("target/fixtures/generated/media/cfr-24-1.nut"))
            .args([
                "-an",
                "-vf",
                &format!("scale=64:64,setparams=color_primaries={primaries}:color_trc={transfer}:colorspace={matrix}:range={}", if range=="pc" {"full"} else {"limited"}),
                "-pix_fmt",
                pixel,
                "-c:v",
                "hevc_videotoolbox",
                "-tag:v",
                "hvc1",
                "-allow_sw",
                "0",
                "-movflags",
                "write_colr",
                "-color_primaries",
                primaries,
                "-color_trc",
                transfer,
                "-colorspace",
                matrix,
                "-color_range",
                range,
            ])
            .arg(&movie)
            .status()
            .unwrap();
        assert!(status.success(), "actual hardware negative fixture {name}");
        let project = temp.path().join(format!("{name}.kronello"));
        let (sequence, origin, _) = create_project_as(&movie, &project, true);
        for backend in [
            BackendSelection::GpuResidentBgra8,
            BackendSelection::GpuResidentNv12,
        ] {
            let error = Service::new(BackendSelection::CpuReference)
                .dispatch(Request::RenderFrame(FrameRenderRequest {
                    input: RenderInput {
                        project: project.clone(),
                        composition: None,
                        target: Some(RenderTarget::Sequence { sequence }),
                        region: OutputRegion {
                            origin: [0.0; 2],
                            extent: [64.0; 2],
                            pixels: [64; 2],
                        },
                        profile: Default::default(),
                        fonts: vec![],
                        media_proxies: kronello_render::MediaProxyMode::Off,
                        luts: vec![],
                    },
                    time: Time::ZERO,
                    backend: Some(backend),
                }))
                .unwrap_err();
            assert_eq!(error.code, "UNSUPPORTED_FEATURE", "{name} {backend:?}");
            assert!(
                error.message.contains("actual compressed"),
                "{name} {backend:?}: {error:?}"
            );
            eprintln!("GPU003 actual {name} forged lock rejection origin={origin:?}: {error:?}");
        }
    }
}
/// PERF-001 follow-up: the preview software-decode upload path must match the
/// explicit software `RasterInput` path pixel-for-pixel within f16 storage
/// tolerance, and must actually take the resident branch.
#[test]
#[ignore = "requires LGPL FFmpeg fixtures, Metal and a generated movie"]
fn service_software_upload_resident_matches_software_preview() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let temp = tempfile::tempdir().unwrap();
    let movie = temp.path().join("upload-resident.mov");
    let status = std::process::Command::new(root.join("target/native/ffmpeg-lgpl/bin/ffmpeg"))
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(root.join("target/fixtures/generated/media/cfr-24-1.nut"))
        .args([
            "-an",
            "-vf",
            "scale=64:64,setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709:range=limited",
            "-c:v",
            "ffv1",
            "-pix_fmt",
            "yuv420p",
            "-movflags",
            "write_colr",
            "-color_primaries",
            "bt709",
            "-color_trc",
            "bt709",
            "-colorspace",
            "bt709",
            "-color_range",
            "tv",
        ])
        .arg(&movie)
        .status()
        .unwrap();
    assert!(status.success(), "software fixture creation");
    let project = temp.path().join("upload-resident.kronello");
    let (id, origin, _duration) = create_project(&movie, &project);
    let gpu = kronello_gpu::GpuContext::new().unwrap();
    let service = Service::with_backend(&gpu);
    let mut session = MediaSession::new();
    let mut readback = kronello_gpu::TransferStats::default();
    for frame_index in [3, 0, 2, 1, 3] {
        let time = origin
            .checked_add(Rational::new(frame_index, 24).unwrap())
            .unwrap();
        let request = FrameRenderRequest {
            input: RenderInput {
                project: project.clone(),
                composition: None,
                target: Some(RenderTarget::Sequence { sequence: id }),
                region: OutputRegion {
                    origin: [0.0; 2],
                    extent: [64.0; 2],
                    pixels: [64; 2],
                },
                profile: Default::default(),
                fonts: vec![],
                media_proxies: kronello_render::MediaProxyMode::Off,
                luts: vec![],
            },
            time,
            backend: Some(BackendSelection::Gpu),
        };
        let (_revision, unresolved) = service.preview_dag_unresolved(&request).unwrap();
        let resolved = service
            .resolve_dag_resident(&unresolved, &project, &mut session, &gpu)
            .unwrap();
        assert!(
            !resolved.resident.is_empty(),
            "eligible video node must take the upload-resident branch"
        );
        let actual_texture = gpu
            .preview_texture_resident(
                &resolved.dag,
                &resolved.resident,
                &resolved.input_identities,
            )
            .unwrap();
        let actual = kronello_gpu::decode_rgba16f(
            &gpu.read_texture(&actual_texture, 8, &mut readback).unwrap(),
        )
        .unwrap();
        let (_revision, dag, identities) =
            service.preview_dag_media(&request, &mut session).unwrap();
        let expected_texture = gpu.preview_texture_with_inputs(&dag, &identities).unwrap();
        let expected = kronello_gpu::decode_rgba16f(
            &gpu.read_texture(&expected_texture, 8, &mut readback)
                .unwrap(),
        )
        .unwrap();
        let max = actual
            .iter()
            .zip(&expected)
            .flat_map(|(a, b)| a.iter().zip(b.iter()).map(|(a, b)| (a - b).abs()))
            .fold(0.0f32, f32::max);
        eprintln!(
            "upload-resident vs software preview frame={frame_index} max={max} actual0={:?} expected0={:?}",
            actual[0], expected[0]
        );
        assert!(
            max < 0.02,
            "upload-resident/software preview mismatch frame={frame_index} max={max}"
        );
    }
}
