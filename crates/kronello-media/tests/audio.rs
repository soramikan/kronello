use kronello_audio::*;
use kronello_gpu::render_adapter::CpuReferenceBackend;
use kronello_media::*;
use kronello_model::*;
use kronello_render::*;
use kronello_time::{Duration, FrameRate, Rational, Time, TimeRange};
use std::path::{Path, PathBuf};
fn t(n: i64, d: i64) -> Rational {
    Rational::new(n, d).unwrap()
}
fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/data/sine-48k-stereo.wav")
}
fn write_wave(path: &Path, rate: u32, channels: u16, frames: usize) {
    let mut data = Vec::new();
    for i in 0..frames {
        let sample = (0.5
            * (2.0 * std::f64::consts::PI * 440.0 * i as f64 / f64::from(rate)).sin()
            * 32767.0)
            .round() as i16;
        for channel in 0..channels {
            data.extend(if channel == 0 { sample } else { -sample }.to_le_bytes());
        }
    }
    let mut bytes = Vec::new();
    bytes.extend(b"RIFF");
    bytes.extend((36 + data.len() as u32).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(channels.to_le_bytes());
    bytes.extend(rate.to_le_bytes());
    bytes.extend((rate * u32::from(channels) * 2).to_le_bytes());
    bytes.extend((channels * 2).to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend((data.len() as u32).to_le_bytes());
    bytes.extend(data);
    std::fs::write(path, bytes).unwrap();
}
fn asset(path: &Path) -> Asset {
    Asset {
        id: AssetId::new(),
        kind: AssetKind::Audio,
        content_hash: content_hash(path).unwrap(),
        streams: vec![],
        locator: AssetLocator {
            relative: None,
            absolute: Some(path.canonicalize().unwrap().to_string_lossy().into_owned()),
        },
    }
}
fn clip(asset: &Asset, range: TimeRange, gain: f32) -> AudioClip {
    AudioClip {
        asset: asset.id,
        stream_index: 0,
        placement: range,
        source_in: Rational::ZERO,
        gain: Gain::new(gain).unwrap(),
    }
}
fn project(asset: Asset) -> (Project, CompositionId) {
    let id = CompositionId::new();
    let c = Composition {
        id,
        duration: Duration::new(t(1, 1)).unwrap(),
        design_extent: DesignExtent::new(16.0, 16.0).unwrap(),
        edit_rate: FrameRate::new(24, 1).unwrap(),
        root_nodes: vec![],
        nodes: vec![],
        properties: vec![],
    };
    let project = Project {
        compositions: vec![DocumentObject::Known(c)],
        assets: vec![DocumentObject::Known(asset)],
        ..Project::default()
    };
    (project, id)
}
fn request(path: &Path, rate: FrameRate) -> AvExportRequest {
    AvExportRequest {
        output: path.into(),
        range: TimeRange::new(
            rate.frame_to_time(1).unwrap(),
            rate.frame_to_time(4).unwrap(),
        )
        .unwrap(),
        frame_rate: rate,
        region: OutputRegion {
            origin: [0.0, 0.0],
            extent: [16.0, 16.0],
            pixels: [16, 16],
        },
        background: [0.1, 0.2, 0.3],
        clipping: ClippingPolicy::Reject,
    }
}
#[test]
fn bounded_decode_rejects_before_exceeding_remaining_source_budget() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bounded.wav");
    write_wave(&path, 48000, 2, 128);
    let asset = asset(&path);
    let runtime = MediaRuntime::load().unwrap();
    for budget in [0, 1, 127] {
        let error = runtime
            .decode_asset_audio_bounded(&asset, dir.path(), 0, budget)
            .unwrap_err();
        assert_eq!(error.code(), "AUDIO_BUDGET_EXCEEDED");
    }
    let decoded = runtime
        .decode_asset_audio_bounded(&asset, dir.path(), 0, 128)
        .unwrap();
    let legacy = runtime.decode_asset_audio(&asset, dir.path(), 0).unwrap();
    assert_eq!(decoded.buffer.frames(), legacy.buffer.frames());
}
#[test]
fn bundled_pcm_fixture_decodes_exact_stereo_samples() {
    let runtime = MediaRuntime::load().unwrap();
    let bytes = std::fs::read(fixture()).unwrap();
    let decoded = runtime.decode_audio(&fixture(), 0).unwrap();
    assert_eq!(decoded.source_rate, 48000);
    assert_eq!(decoded.source_channels, 2);
    assert_eq!(decoded.source_start, Rational::ZERO);
    assert_eq!(decoded.buffer.frames().len(), 4800);
    let expected: Vec<_> = bytes[44..]
        .chunks_exact(4)
        .map(|b| {
            [
                f32::from(i16::from_le_bytes([b[0], b[1]])) / 32768.0,
                f32::from(i16::from_le_bytes([b[2], b[3]])) / 32768.0,
            ]
        })
        .collect();
    assert_eq!(decoded.buffer.frames(), expected);
}
#[test]
fn swresample_converts_mono_stereo_rates_and_drains_tail() {
    let runtime = MediaRuntime::load().unwrap();
    let dir = tempfile::tempdir().unwrap();
    for (rate, channels) in [(44100, 1), (32000, 1), (96000, 2)] {
        let path = dir.path().join(format!("{rate}-{channels}.wav"));
        write_wave(&path, rate, channels, rate as usize / 10);
        let decoded = runtime.decode_audio(&path, 0).unwrap();
        assert_eq!(decoded.source_rate, rate);
        assert_eq!(decoded.source_channels, u32::from(channels));
        assert_eq!(
            decoded.buffer.frames().len(),
            4800,
            "drained sample count at {rate}"
        );
        for i in 64..4736 {
            let expected = 0.5 * (2.0 * std::f64::consts::PI * 440.0 * i as f64 / 48000.0).sin();
            let frame = decoded.buffer.frames()[i];
            assert!(
                (f64::from(frame[0]) - expected).abs() < 0.0003,
                "{rate}: sample {i}"
            );
            assert_eq!(frame[1], if channels == 1 { frame[0] } else { -frame[0] });
        }
        assert!(
            decoded.buffer.frames()[4790][0].abs() > 0.01,
            "resampler flush retains the tail"
        );
    }
}
#[test]
fn unsupported_channel_layout_and_stream_selection_fail() {
    let runtime = MediaRuntime::load().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("surround.wav");
    write_wave(&path, 48000, 3, 4800);
    assert_eq!(
        runtime.decode_audio(&path, 0).unwrap_err().code(),
        "UNSUPPORTED_FEATURE"
    );
    assert!(runtime.decode_audio(&fixture(), 1).is_err());
    assert_eq!(
        runtime
            .decode_audio(&fixture(), u32::MAX)
            .unwrap_err()
            .code(),
        "INVALID_MEDIA_INPUT"
    );
}
#[test]
fn asset_audio_hash_mismatch_and_missing_fail_without_substitution() {
    let runtime = MediaRuntime::load().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.wav");
    std::fs::copy(fixture(), &path).unwrap();
    let mut a = asset(&path);
    let project_path = dir.path().join("project.kronello");
    assert_eq!(
        runtime
            .decode_asset_audio(&a, &project_path, 0)
            .unwrap()
            .buffer,
        runtime.decode_audio(&path, 0).unwrap().buffer
    );
    a.content_hash = "0".repeat(64);
    assert_eq!(
        runtime
            .decode_asset_audio(&a, &project_path, 0)
            .unwrap_err()
            .code(),
        "ASSET_HASH_MISMATCH"
    );
    std::fs::remove_file(path).unwrap();
    assert_eq!(
        runtime
            .decode_asset_audio(&a, &project_path, 0)
            .unwrap_err()
            .code(),
        "ASSET_MISSING"
    );
}
#[test]
fn pcm24_roundtrip_quantization_clipping_policy_and_output_rollback() {
    let runtime = MediaRuntime::load().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let id = AssetId::new();
    let frames = vec![[-1.0, 1.0], [0.5, -0.5], [0.12345, -0.12345], [0.0, 0.0]];
    let sources = AudioSources::from([((id, 0), AudioBuffer::new(frames.clone()).unwrap())]);
    let clip = AudioClip {
        asset: id,
        stream_index: 0,
        placement: TimeRange::new(t(0, 1), t(4, 48000)).unwrap(),
        source_in: Rational::ZERO,
        gain: Gain::UNITY,
    };
    let bus = mix(std::slice::from_ref(&clip), &sources, clip.placement).unwrap();
    let output = dir.path().join("pcm.mov");
    let report = runtime
        .encode_audio(&bus, ClippingPolicy::Reject, &output)
        .unwrap();
    assert_eq!(report.frames, 4);
    assert_eq!(report.clipped_samples, 0);
    let decoded = runtime.decode_audio(&output, 0).unwrap();
    assert_eq!(decoded.source_start, Rational::ZERO);
    assert_eq!(decoded.buffer.frames().len(), 4);
    for (actual, expected) in decoded
        .buffer
        .frames()
        .iter()
        .flatten()
        .zip(frames.iter().flatten())
    {
        assert!((actual - expected).abs() <= 1.0 / 8388608.0);
    }
    let probe = runtime.probe(&output).unwrap();
    assert_eq!(probe.streams[0].codec, "pcm_s24le");
    assert_eq!(probe.streams[0].duration, Some(t(4, 48000)));
    let before = std::fs::read(&output).unwrap();
    assert_eq!(
        runtime
            .encode_audio(&bus, ClippingPolicy::Reject, &output)
            .unwrap_err()
            .code(),
        "OUTPUT_EXISTS"
    );
    assert_eq!(std::fs::read(&output).unwrap(), before);
    let loud = AudioClip {
        gain: Gain::new(2.0).unwrap(),
        ..clip
    };
    let bus = mix(std::slice::from_ref(&loud), &sources, loud.placement).unwrap();
    let output = dir.path().join("loud.mov");
    assert_eq!(
        runtime
            .encode_audio(&bus, ClippingPolicy::Reject, &output)
            .unwrap_err()
            .code(),
        "AUDIO_CLIPPING"
    );
    assert!(!output.exists());
    let report = runtime
        .encode_audio(&bus, ClippingPolicy::Saturate, &output)
        .unwrap();
    assert_eq!(report.clipped_samples, 2);
}
#[test]
fn export_fixed_snapshot_muxes_av_with_exact_pts_and_sample_precision_at_three_rates() {
    let runtime = MediaRuntime::load().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.wav");
    write_wave(&path, 44100, 1, 44100);
    for (n, d) in [(24, 1), (30000, 1001), (60000, 1001)] {
        let rate = FrameRate::new(n, d).unwrap();
        let output = dir.path().join(format!("av-{n}.mov"));
        let request = request(&output, rate);
        let a = asset(&path);
        let (mut p, id) = project(a.clone());
        let render = RenderSnapshot::new(&p, id, 7, RenderProfile::default()).unwrap();
        let snapshot = AvExportSnapshot::new(&render, vec![clip(&a, request.range, 0.5)]).unwrap();
        let stored = serde_json::to_vec(&snapshot).unwrap();
        let restored: AvExportSnapshot = serde_json::from_slice(&stored).unwrap();
        assert_eq!(
            snapshot.content_hash().unwrap(),
            restored.content_hash().unwrap()
        );
        let changed = AvExportSnapshot::new(&render, vec![clip(&a, request.range, 0.25)]).unwrap();
        assert_ne!(
            snapshot.content_hash().unwrap(),
            changed.content_hash().unwrap()
        );
        assert_eq!(
            snapshot.render().content_hash().unwrap(),
            changed.render().content_hash().unwrap()
        );
        p.name = "edited after submission".into();
        p.assets.clear();
        assert_ne!(
            RenderSnapshot::new(&p, id, 8, RenderProfile::default())
                .unwrap()
                .content_hash()
                .unwrap(),
            render.content_hash().unwrap()
        );
        let report = runtime
            .export_av(
                &restored,
                &dir.path().join("project.kronello"),
                &[],
                &CpuReferenceBackend,
                &request,
            )
            .unwrap();
        report.probe.verify_av().unwrap();
        assert_eq!(report.frames.len(), 3);
        assert_eq!(report.render_snapshot_hash, render.content_hash().unwrap());
        assert_eq!(
            report.audio_render_snapshot_hash,
            report.render_snapshot_hash
        );
        assert_eq!(
            report.probe.render_snapshot_hash,
            report.render_snapshot_hash
        );
        assert_eq!(
            report.probe.export_snapshot_hash,
            snapshot.content_hash().unwrap()
        );
        let expected_range = sample_range(request.range).unwrap();
        assert_eq!(report.sample_range, expected_range);
        assert_eq!(
            report.audio.frames,
            (expected_range.end - expected_range.start) as usize
        );
        let video_duration = report
            .probe
            .streams
            .iter()
            .find(|s| s.kind == StreamKind::Video)
            .unwrap()
            .duration
            .unwrap();
        let audio_duration = report
            .probe
            .streams
            .iter()
            .find(|s| s.kind == StreamKind::Audio)
            .unwrap()
            .duration
            .unwrap();
        assert_eq!(video_duration, rate.frame_to_time(3).unwrap());
        assert_eq!(audio_duration, t(report.audio.frames as i64, 48000));
        let delta = video_duration.checked_sub(audio_duration).unwrap();
        assert!(delta < t(1, 48000) && delta > t(-1, 48000));
        let mut decoder = runtime.open_video(&output).unwrap();
        for (ordinal, metadata) in report.frames.iter().enumerate() {
            assert_eq!(metadata.snapshot_content_hash, report.render_snapshot_hash);
            assert_eq!(
                metadata.time,
                rate.frame_to_time(ordinal as i64 + 1).unwrap()
            );
            let pts = rate.frame_to_time(ordinal as i64).unwrap();
            let frame = decoder.decode_at(pts).unwrap();
            assert_eq!(frame.pts, pts);
            assert_eq!(frame.end, rate.frame_to_time(ordinal as i64 + 1).unwrap());
        }
        let audio_stream = report
            .probe
            .streams
            .iter()
            .find(|s| s.kind == StreamKind::Audio)
            .unwrap()
            .index;
        let decoded = runtime.decode_audio(&output, audio_stream).unwrap();
        assert_eq!(decoded.source_start, Rational::ZERO);
        assert_eq!(decoded.buffer.frames().len(), report.audio.frames);
        let source = runtime
            .decode_asset_audio(&a, &dir.path().join("project.kronello"), 0)
            .unwrap()
            .buffer;
        let sources = AudioSources::from([((a.id, 0), source)]);
        let expected = mix(snapshot.clips(), &sources, request.range).unwrap();
        for (actual, expected) in decoded
            .buffer
            .frames()
            .iter()
            .flatten()
            .zip(expected.buffer().frames().iter().flatten())
        {
            assert!((actual - expected).abs() <= 1.0 / 8388608.0);
        }
    }
}
#[test]
fn export_rejects_bad_ranges_versions_missing_assets_clipping_and_existing_outputs() {
    let runtime = MediaRuntime::load().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let a = asset(&fixture());
    let (p, id) = project(a.clone());
    let render = RenderSnapshot::new(&p, id, 0, RenderProfile::default()).unwrap();
    let rate = FrameRate::new(24, 1).unwrap();
    let mut request = request(&dir.path().join("failed.mov"), rate);
    request.range = TimeRange::new(t(0, 1), t(1, 24)).unwrap();
    let snapshot = AvExportSnapshot::new(&render, vec![clip(&a, request.range, 100.0)]).unwrap();
    assert_eq!(
        runtime
            .export_av(
                &snapshot,
                Path::new("project.kronello"),
                &[],
                &CpuReferenceBackend,
                &request
            )
            .unwrap_err()
            .code(),
        "AUDIO_CLIPPING"
    );
    assert!(!request.output.exists());
    request.range = TimeRange::new(t(0, 1), t(1, 100)).unwrap();
    assert_eq!(
        runtime
            .export_av(
                &snapshot,
                Path::new("project.kronello"),
                &[],
                &CpuReferenceBackend,
                &request
            )
            .unwrap_err()
            .code(),
        "INVALID_MEDIA_INPUT"
    );
    assert!(!request.output.exists());
    let mut value = serde_json::to_value(&snapshot).unwrap();
    value["schema_version"] = 999.into();
    let unsupported: AvExportSnapshot = serde_json::from_value(value).unwrap();
    assert_eq!(
        unsupported.validate().unwrap_err().code(),
        "UNSUPPORTED_FEATURE"
    );
    let mut missing = clip(&a, request.range, 1.0);
    missing.asset = AssetId::new();
    assert_eq!(
        AvExportSnapshot::new(&render, vec![missing])
            .unwrap_err()
            .code(),
        "ASSET_MISSING"
    );
    request.range = TimeRange::new(t(0, 1), t(1, 24)).unwrap();
    let silent = AvExportSnapshot::new(&render, vec![]).unwrap();
    std::fs::write(&request.output, b"existing").unwrap();
    assert_eq!(
        runtime
            .export_av(
                &silent,
                Path::new("project.kronello"),
                &[],
                &CpuReferenceBackend,
                &request
            )
            .unwrap_err()
            .code(),
        "OUTPUT_EXISTS"
    );
    assert_eq!(std::fs::read(&request.output).unwrap(), b"existing");
}
#[test]
fn mux_rejects_duration_mismatch_before_publication() {
    let runtime = MediaRuntime::load().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let video = dir.path().join("video.mov");
    let audio = dir.path().join("audio.mov");
    let output = dir.path().join("mismatch.mov");
    runtime
        .encode_video(
            &EncodeRequest {
                output: video.clone(),
                codec: EncodeCodec::ProRes,
                width: 16,
                height: 16,
                time_base: t(1, 24),
            },
            &[EncodeFrame {
                pts: t(0, 1),
                rgba: vec![255; 16 * 16 * 4],
            }],
        )
        .unwrap();
    let bus = mix(
        &[],
        &AudioSources::new(),
        TimeRange::new(t(0, 1), t(1, 10)).unwrap(),
    )
    .unwrap();
    runtime
        .encode_audio(&bus, ClippingPolicy::Reject, &audio)
        .unwrap();
    assert!(
        runtime
            .mux_av(&video, &audio, &output, &"1".repeat(64), &"2".repeat(64))
            .is_err()
    );
    assert!(!output.exists());
}

#[test]
fn document_audio_source_modes_are_explicit_backward_compatible_and_hashed() {
    let runtime = MediaRuntime::load().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.wav");
    write_wave(&path, 48000, 2, 48000);
    let mut a = asset(&path);
    a.streams.push(StreamMetadata {
        index: 0,
        codec: "pcm_s16le".into(),
        time_base: t(1, 48000),
        duration: Some(t(1, 1)),
        start_time: None,
        width: None,
        height: None,
        pixel_format: None,
        color_primaries: None,
        color_transfer: None,
        color_matrix: None,
        color_range: None,
    });
    let aid = a.id;
    let (mut p, id) = project(a);
    let registry = SchemaRegistry::with_builtin();
    let volume = Property::new(
        PropertyId::new(),
        DescriptorRef::new(
            registry
                .lookup(&SchemaKey::new("kronello.audio.volume").unwrap())
                .unwrap(),
        ),
        PropertySource::Constant(Value::Scalar(FiniteF64::new(0.5).unwrap())),
        vec![],
        &registry,
    )
    .unwrap();
    let node = SceneNode {
        tags: Default::default(),
        name: None,
        enabled: true,
        id: NodeId::new(),
        kind: NodeKind::Media(MediaNode {
            asset: aid,
            stream_index: 0,
            source_in: Time::ZERO,
            time_map: kronello_time::TimeMap::linear(Time::ZERO, Time::ONE).unwrap(),
            volume: volume.id(),
        }),
        active_range: TimeRange::new(Time::ZERO, t(1, 1)).unwrap(),
        properties: vec![volume],
        containment_parent: None,
        transform_parent: None,
        child_order: vec![],
        effects: vec![],
    };
    let DocumentObject::Known(c) = &mut p.compositions[0] else {
        panic!()
    };
    c.root_nodes.push(node.id);
    c.nodes.push(node);
    let render = RenderSnapshot::new(&p, id, 1, Default::default()).unwrap();
    let legacy = AvExportSnapshot::new(&render, vec![]).unwrap();
    assert!(
        serde_json::to_value(&legacy)
            .unwrap()
            .get("audio")
            .is_none()
    );
    let restored: AvExportSnapshot =
        serde_json::from_slice(&serde_json::to_vec(&legacy).unwrap()).unwrap();
    assert_eq!(restored.audio(), AudioSourceMode::Explicit);
    assert_eq!(
        restored.content_hash().unwrap(),
        legacy.content_hash().unwrap()
    );
    let document =
        AvExportSnapshot::with_audio(&render, AudioSourceMode::Document, vec![]).unwrap();
    assert_eq!(document.clips().len(), 1);
    let silence = AvExportSnapshot::with_audio(&render, AudioSourceMode::Silence, vec![]).unwrap();
    assert_ne!(
        document.content_hash().unwrap(),
        silence.content_hash().unwrap()
    );
    assert_ne!(
        silence.content_hash().unwrap(),
        legacy.content_hash().unwrap()
    );
    for mode in [AudioSourceMode::Document, AudioSourceMode::Silence] {
        assert_eq!(
            AvExportSnapshot::with_audio(&render, mode, document.clips().to_vec())
                .unwrap_err()
                .code(),
            "INVALID_MEDIA_INPUT"
        );
    }
    // Every mode is deliberate; explicit/silence ignore document audio, never add it twice.
    for (name, snapshot, audible) in [
        ("legacy", legacy, false),
        ("silence", silence, false),
        ("document", document, true),
    ] {
        let req = request(
            &dir.path().join(format!("{name}.mov")),
            FrameRate::new(30000, 1001).unwrap(),
        );
        let report = runtime
            .export_av(
                &snapshot,
                &dir.path().join("project.kronello"),
                &[],
                &CpuReferenceBackend,
                &req,
            )
            .unwrap();
        report.probe.verify_av().unwrap();
        let decoded = runtime.decode_audio(&req.output, 1).unwrap();
        assert_eq!(
            decoded.buffer.frames().iter().flatten().any(|v| *v != 0.0),
            audible
        );
    }
    let DocumentObject::Known(a) = &mut p.assets[0] else {
        panic!()
    };
    a.kind = AssetKind::Video;
    let render = RenderSnapshot::new(&p, id, 1, Default::default()).unwrap();
    let snapshot =
        AvExportSnapshot::with_audio(&render, AudioSourceMode::Document, vec![]).unwrap();
    let req = request(
        &dir.path().join("video-container-audio.mov"),
        FrameRate::new(24, 1).unwrap(),
    );
    runtime
        .export_av(
            &snapshot,
            &dir.path().join("project.kronello"),
            &[],
            &CpuReferenceBackend,
            &req,
        )
        .unwrap();
    let decoded = runtime.decode_audio(&req.output, 1).unwrap();
    assert!(decoded.buffer.frames().iter().flatten().any(|v| *v != 0.0));

    // Visual streams contribute no document audio; container audio remains explicit.
    let DocumentObject::Known(a) = &mut p.assets[0] else {
        panic!()
    };
    a.streams[0].width = Some(16);
    a.streams[0].height = Some(16);
    let render = RenderSnapshot::new(&p, id, 1, Default::default()).unwrap();
    let visual = AvExportSnapshot::with_audio(&render, AudioSourceMode::Document, vec![]).unwrap();
    assert!(visual.clips().is_empty());

    // Unsupported actual audio still fails before publication.
    write_wave(&path, 48000, 3, 48000);
    let DocumentObject::Known(a) = &mut p.assets[0] else {
        panic!()
    };
    a.kind = AssetKind::Audio;
    a.content_hash = content_hash(&path).unwrap();
    a.streams[0].width = None;
    a.streams[0].height = None;
    let render = RenderSnapshot::new(&p, id, 1, Default::default()).unwrap();
    let snapshot =
        AvExportSnapshot::with_audio(&render, AudioSourceMode::Document, vec![]).unwrap();
    let req = request(
        &dir.path().join("unsupported-audio.mov"),
        FrameRate::new(24, 1).unwrap(),
    );
    assert_eq!(
        runtime
            .export_av(
                &snapshot,
                &dir.path().join("project.kronello"),
                &[],
                &CpuReferenceBackend,
                &req
            )
            .unwrap_err()
            .code(),
        "UNSUPPORTED_FEATURE"
    );
    assert!(!req.output.exists());
}
