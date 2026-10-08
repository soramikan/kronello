use kronello_audio::{AudioClip, ClippingPolicy, Gain};
use kronello_gpu::render_adapter::CpuReferenceBackend;
use kronello_media::*;
use kronello_model::*;
use kronello_render::*;
use kronello_time::{Duration, FrameRate, Time, TimeRange};
use std::io::Write;
use std::path::Path;

fn snapshot(assets: Vec<Asset>) -> RenderSnapshot {
    let id = CompositionId::new();
    let project = Project {
        assets: assets.into_iter().map(DocumentObject::Known).collect(),
        compositions: vec![DocumentObject::Known(Composition {
            id,
            duration: Duration::new(Time::from_integer(700)).unwrap(),
            design_extent: DesignExtent::new(1024.0, 1024.0).unwrap(),
            edit_rate: FrameRate::new(24, 1).unwrap(),
            root_nodes: vec![],
            nodes: vec![],
            properties: vec![],
        })],
        ..Project::default()
    };
    RenderSnapshot::new(&project, id, 0, RenderProfile::default()).unwrap()
}
fn request(
    path: &Path,
    rate: FrameRate,
    start: i64,
    count: i64,
    pixels: [u32; 2],
) -> AvExportRequest {
    AvExportRequest {
        output: path.into(),
        range: TimeRange::new(
            rate.frame_to_time(start).unwrap(),
            rate.frame_to_time(start + count).unwrap(),
        )
        .unwrap(),
        frame_rate: rate,
        region: OutputRegion {
            origin: [-3.0, 2.0],
            extent: [1024.0, 1024.0],
            pixels,
        },
        background: [0.1, 0.2, 0.3],
        clipping: ClippingPolicy::Reject,
    }
}
struct Pattern;
impl RenderBackend for Pattern {
    fn name(&self) -> &str {
        "streaming_test_pattern"
    }
    fn execute(&self, dag: &RenderDag) -> Result<BackendFrame, RenderError> {
        let region = dag.region();
        let linear: Vec<_> = (0..region.pixels[1])
            .flat_map(|y| {
                (0..region.pixels[0]).map(move |x| {
                    let px = region.origin[0]
                        + (f64::from(x) + 0.5) * region.extent[0] / f64::from(region.pixels[0]);
                    let py = region.origin[1]
                        + (f64::from(y) + 0.5) * region.extent[1] / f64::from(region.pixels[1]);
                    [
                        ((px.floor() as i64).rem_euclid(17) as f32) / 32.0,
                        ((py.floor() as i64).rem_euclid(13) as f32) / 32.0,
                        0.25,
                        1.0,
                    ]
                })
            })
            .collect();
        Ok(BackendFrame {
            display: linear.clone(),
            linear,
        })
    }
}
#[test]
fn streamed_tiles_match_whole_frame_at_edges_and_stop_on_sink_failure() {
    let render = snapshot(vec![]);
    let req = FrameRequest {
        time: Time::new(-1001, 30000).unwrap(),
        region: request(
            Path::new("unused"),
            FrameRate::new(24, 1).unwrap(),
            0,
            1,
            [1030, 518],
        )
        .region,
    };
    let whole = render_frame(&render, &[], &Pattern, req).unwrap();
    let mut gathered = BackendFrame {
        linear: vec![[0.0; 4]; 1030 * 518],
        display: vec![[0.0; 4]; 1030 * 518],
    };
    let mut tiles = 0;
    let metadata = render_frame_tiles(
        &render,
        &[],
        &Pattern,
        req,
        &mut |[x, y], region, pixels| {
            assert!(region.pixels.iter().all(|v| *v <= 512));
            for row in 0..region.pixels[1] as usize {
                let dest = (y as usize + row) * 1030 + x as usize;
                let source = row * region.pixels[0] as usize;
                let width = region.pixels[0] as usize;
                gathered.linear[dest..dest + width]
                    .copy_from_slice(&pixels.linear[source..source + width]);
                gathered.display[dest..dest + width]
                    .copy_from_slice(&pixels.display[source..source + width]);
            }
            tiles += 1;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(tiles, 6);
    assert_eq!(whole.metadata, metadata);
    assert_eq!(whole.pixels, gathered);
    let mut count = 0;
    let error = render_frame_tiles(&render, &[], &Pattern, req, &mut |_, _, _| {
        count += 1;
        Err(RenderError::Io(std::io::Error::from_raw_os_error(28)))
    })
    .unwrap_err();
    assert_eq!(error.code(), "OUTPUT_IO_ERROR");
    assert_eq!(count, 1);
}
fn wave(path: &Path, seconds: u32) -> Asset {
    let frames = seconds * 48000;
    let bytes = frames * 4;
    let mut file = std::fs::File::create(path).unwrap();
    file.write_all(b"RIFF").unwrap();
    file.write_all(&(36 + bytes).to_le_bytes()).unwrap();
    file.write_all(b"WAVEfmt ").unwrap();
    file.write_all(&16_u32.to_le_bytes()).unwrap();
    file.write_all(&1_u16.to_le_bytes()).unwrap();
    file.write_all(&2_u16.to_le_bytes()).unwrap();
    file.write_all(&48000_u32.to_le_bytes()).unwrap();
    file.write_all(&192000_u32.to_le_bytes()).unwrap();
    file.write_all(&4_u16.to_le_bytes()).unwrap();
    file.write_all(&16_u16.to_le_bytes()).unwrap();
    file.write_all(b"data").unwrap();
    file.write_all(&bytes.to_le_bytes()).unwrap();
    let block: Vec<_> = (0..48000)
        .flat_map(|i| {
            let value = ((i % 257) as i16 - 128) * 128;
            value
                .to_le_bytes()
                .into_iter()
                .chain((-value).to_le_bytes())
        })
        .collect();
    for _ in 0..seconds {
        file.write_all(&block).unwrap();
    }
    drop(file);
    Asset {
        id: AssetId::new(),
        kind: AssetKind::Audio,
        content_hash: content_hash(path).unwrap(),
        streams: vec![],
        locator: AssetLocator {
            relative: None,
            absolute: Some(path.to_string_lossy().into_owned()),
        },
    }
}
#[test]
fn streamed_audio_matches_whole_mix_and_pts_for_negative_and_ntsc_ranges() {
    let runtime = MediaRuntime::load().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let asset = wave(&dir.path().join("source.wav"), 2);
    let render = snapshot(vec![asset.clone()]);
    for (rate, start) in [
        (FrameRate::new(24, 1).unwrap(), -3),
        (FrameRate::new(30000, 1001).unwrap(), 1),
        (FrameRate::new(60000, 1001).unwrap(), -1),
    ] {
        let req = request(
            &dir.path().join(format!("{rate:?}-{start}.mov")),
            rate,
            start,
            7,
            [18, 16],
        );
        let clip = AudioClip {
            asset: asset.id,
            stream_index: 0,
            placement: req.range,
            source_in: Time::new(1, 7).unwrap(),
            gain: Gain::new(0.5).unwrap(),
        };
        let av = AvExportSnapshot::new(&render, vec![clip.clone()]).unwrap();
        let report = runtime
            .export_av(
                &av,
                &dir.path().join("project.kronello"),
                &[],
                &CpuReferenceBackend,
                &req,
            )
            .unwrap();
        let decoded = runtime.decode_audio(&req.output, 1).unwrap();
        let source = runtime
            .decode_asset_audio(&asset, &dir.path().join("project.kronello"), 0)
            .unwrap()
            .buffer;
        let sources = kronello_audio::ChannelSources::from([((asset.id, 0), source)]);
        let expected = kronello_audio::mix_channels(
            &[clip],
            &sources,
            req.range,
            kronello_model::ChannelMask::STEREO,
        )
        .unwrap()
        .quantize_pcm24(req.clipping)
        .unwrap();
        assert_eq!(decoded.buffer.samples().len(), expected.samples.len());
        for (actual, expected) in decoded.buffer.samples().iter().zip(expected.samples) {
            assert_eq!(actual.to_bits(), (expected as f32 / 2147483648.0).to_bits());
        }
        assert_eq!(
            report.sample_range,
            kronello_audio::sample_range(req.range).unwrap()
        );
        let mut decoder = runtime.open_video(&req.output).unwrap();
        for i in 0..7 {
            let frame = decoder.decode_at(rate.frame_to_time(i).unwrap()).unwrap();
            assert_eq!(frame.pts, rate.frame_to_time(i).unwrap());
            assert_eq!(frame.end, rate.frame_to_time(i + 1).unwrap());
        }
    }
}
#[test]
fn cancellation_and_encoder_failure_remove_all_temporary_outputs() {
    let runtime = MediaRuntime::load().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let av = AvExportSnapshot::new(&snapshot(vec![]), vec![]).unwrap();
    let req = request(
        &dir.path().join("result.mov"),
        FrameRate::new(24, 1).unwrap(),
        0,
        3,
        [18, 16],
    );
    let error = runtime
        .export_av_with_checkpoint(
            &av,
            &dir.path().join("project.kronello"),
            &[],
            &CpuReferenceBackend,
            &req,
            &mut |frame| {
                if frame == 1 {
                    Err(MediaError::Encode("injected cancellation".into()))
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
    assert_eq!(error.code(), "ENCODE_ERROR");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    struct Full;
    impl RenderBackend for Full {
        fn name(&self) -> &str {
            "injected_capacity_error"
        }
        fn execute(&self, _: &RenderDag) -> Result<BackendFrame, RenderError> {
            Err(RenderError::Io(std::io::Error::from_raw_os_error(28)))
        }
    }
    assert_eq!(
        runtime
            .export_av(&av, &dir.path().join("project.kronello"), &[], &Full, &req)
            .unwrap_err()
            .code(),
        "OUTPUT_IO_ERROR"
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    let mut invalid = req.clone();
    invalid.region.pixels = [17, 16];
    assert!(
        runtime
            .export_av(
                &av,
                &dir.path().join("project.kronello"),
                &[],
                &CpuReferenceBackend,
                &invalid
            )
            .is_err()
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    // A competing publication after preflight must survive the final fence.
    let race = runtime
        .export_av_with_checkpoint(
            &av,
            &dir.path().join("project.kronello"),
            &[],
            &CpuReferenceBackend,
            &req,
            &mut |frame| {
                if frame == 1 {
                    std::fs::write(&req.output, b"existing").unwrap();
                }
                Ok(())
            },
        )
        .unwrap_err();
    assert_eq!(race.code(), "OUTPUT_EXISTS");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    assert_eq!(std::fs::read(&req.output).unwrap(), b"existing");
    std::fs::write(&req.output, b"existing").unwrap();
    assert_eq!(
        runtime
            .export_av(
                &av,
                &dir.path().join("project.kronello"),
                &[],
                &CpuReferenceBackend,
                &req
            )
            .unwrap_err()
            .code(),
        "OUTPUT_EXISTS"
    );
    assert_eq!(std::fs::read(&req.output).unwrap(), b"existing");
}
#[test]
#[ignore = "host memory/I/O acceptance; writes over 500 MB of temporary media"]
fn long_export_exceeds_source_and_bus_limits() {
    let runtime = MediaRuntime::load().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let asset = wave(&dir.path().join("long.wav"), 601);
    let render = snapshot(vec![asset.clone()]);
    let req = request(
        &dir.path().join("long.mov"),
        FrameRate::new(1, 1).unwrap(),
        -2,
        601,
        [18, 16],
    );
    let av = AvExportSnapshot::new(
        &render,
        vec![AudioClip {
            asset: asset.id,
            stream_index: 0,
            placement: req.range,
            source_in: Time::ZERO,
            gain: Gain::UNITY,
        }],
    )
    .unwrap();
    let report = runtime
        .export_av(
            &av,
            &dir.path().join("p.kronello"),
            &[],
            &CpuReferenceBackend,
            &req,
        )
        .unwrap();
    assert_eq!(report.audio.frames, 601 * 48000);
    let mut ordinal = 0_usize;
    let decoded = runtime
        .decode_audio_stream(&req.output, 1, &mut |chunk, mask| {
            assert_eq!(mask, kronello_model::ChannelMask::STEREO);
            for frame in chunk.chunks_exact(2) {
                let value = ((ordinal % 48000 % 257) as i16 - 128) * 128;
                let expected = [f32::from(value) / 32768.0, -f32::from(value) / 32768.0];
                assert_eq!(frame, expected.as_slice(), "sample {ordinal}");
                ordinal += 1;
            }
            Ok(())
        })
        .unwrap();
    assert_eq!(decoded.source_start, Time::ZERO);
    assert_eq!(decoded.frames, report.audio.frames);
    assert_eq!(ordinal, report.audio.frames);
    assert_eq!(report.frames.len(), 601);
    assert_eq!(
        report.probe.streams[0].duration,
        Some(Time::from_integer(601))
    );
    assert_eq!(
        report.probe.streams[1].duration,
        Some(Time::from_integer(601))
    );
    let io = report.streaming.as_ref().unwrap();
    assert_eq!(io.audio_spool_write_bytes, 601 * 48000 * 8);
    assert_eq!(io.audio_window_read_bytes, io.audio_spool_write_bytes);
    println!("streaming_io={}", serde_json::to_string(io).unwrap());
    println!(
        "long frames={} audio_samples={} output_bytes={}",
        report.frames.len(),
        report.audio.frames,
        std::fs::metadata(&req.output).unwrap().len()
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
}
#[test]
#[ignore = "host 4K memory/I/O acceptance"]
fn four_k_export_exceeds_previous_video_payload_limit() {
    let runtime = MediaRuntime::load().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let av = AvExportSnapshot::new(&snapshot(vec![]), vec![]).unwrap();
    let req = request(
        &dir.path().join("4k.mov"),
        FrameRate::new(24, 1).unwrap(),
        0,
        9,
        [3840, 2160],
    );
    let report = runtime
        .export_av(
            &av,
            &dir.path().join("p.kronello"),
            &[],
            &CpuReferenceBackend,
            &req,
        )
        .unwrap();
    assert_eq!(report.frames.len(), 9);
    assert!(report.video.transfers.cpu_conversion_input_bytes > 256 * 1024 * 1024);
    assert_eq!(report.audio.frames, 18000);
    println!(
        "streaming_io={}",
        serde_json::to_string(report.streaming.as_ref().unwrap()).unwrap()
    );
    println!(
        "4K rgba_bytes={} output_bytes={}",
        3840_u64 * 2160 * 4 * 9,
        std::fs::metadata(&req.output).unwrap().len()
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
#[ignore = "run with measure_export_streaming.py --file-limit-bytes 65536"]
fn native_write_capacity_failure_removes_temporary_outputs() {
    assert_eq!(
        std::env::var("KRONELLO_EXPORT_TEST_FILE_LIMIT").unwrap(),
        "65536"
    );
    let runtime = MediaRuntime::load().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("existing.mov");
    std::fs::write(&old, b"keep existing artifact").unwrap();
    let req = request(
        &dir.path().join("result.mov"),
        FrameRate::new(24, 1).unwrap(),
        0,
        24,
        [18, 16],
    );
    let av = AvExportSnapshot::new(&snapshot(vec![]), vec![]).unwrap();
    let error = runtime
        .export_av(
            &av,
            &dir.path().join("p.kronello"),
            &[],
            &CpuReferenceBackend,
            &req,
        )
        .unwrap_err();
    assert_eq!(error.code(), "ENCODE_ERROR", "{error}");
    assert!(error.to_string().contains("File too large"), "{error}");
    assert!(!req.output.exists());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    assert_eq!(std::fs::read(old).unwrap(), b"keep existing artifact");
    println!("native write failure={error}");
}

#[test]
#[ignore = "run with measure_export_streaming.py --file-limit-bytes 16384"]
fn source_spool_capacity_failure_removes_temporary_outputs() {
    assert_eq!(
        std::env::var("KRONELLO_EXPORT_TEST_FILE_LIMIT").unwrap(),
        "16384"
    );
    let runtime = MediaRuntime::load().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("existing.mov");
    std::fs::write(&old, b"keep existing artifact").unwrap();
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/data/sine-48k-stereo.wav")
        .canonicalize()
        .unwrap();
    let asset = Asset {
        id: AssetId::new(),
        kind: AssetKind::Audio,
        content_hash: content_hash(&path).unwrap(),
        streams: vec![],
        locator: AssetLocator {
            relative: None,
            absolute: Some(path.to_string_lossy().into_owned()),
        },
    };
    let render = snapshot(vec![asset.clone()]);
    let req = request(
        &dir.path().join("result.mov"),
        FrameRate::new(24, 1).unwrap(),
        0,
        24,
        [18, 16],
    );
    let av = AvExportSnapshot::new(
        &render,
        vec![AudioClip {
            asset: asset.id,
            stream_index: 0,
            placement: req.range,
            source_in: Time::ZERO,
            gain: Gain::UNITY,
        }],
    )
    .unwrap();
    let error = runtime
        .export_av(
            &av,
            &dir.path().join("p.kronello"),
            &[],
            &CpuReferenceBackend,
            &req,
        )
        .unwrap_err();
    assert_eq!(error.code(), "MEDIA_IO_ERROR", "{error}");
    assert!(error.to_string().contains("File too large"), "{error}");
    assert!(!req.output.exists());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    assert_eq!(std::fs::read(old).unwrap(), b"keep existing artifact");
    println!("source spool write failure={error}");
}
