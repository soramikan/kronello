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
        chapters: kronello_media::ChapterPolicy::Transfer,
        outputs: Vec::new(),
    }
}

#[test]
fn shared_audio_preserves_clipping_and_native_blocks_against_independent_exports() {
    let runtime = MediaRuntime::load().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let wave = dir.path().join("shared.wav");
    write_wave(&wave, 48000, 2, 48000);
    let asset = asset(&wave);
    let clips = vec![clip(
        &asset,
        TimeRange::new(Time::ZERO, t(1, 1)).unwrap(),
        4.0,
    )];
    let (project, composition) = project(asset);
    let render = RenderSnapshot::new(&project, composition, 0, Default::default()).unwrap();
    let snapshots = [MovieProfile::FlacV1, MovieProfile::Mp3V1].map(|profile| {
        AvExportSnapshot::with_movie_profile(
            &render,
            AudioSourceMode::Explicit,
            clips.clone(),
            profile,
        )
        .unwrap()
    });
    let mut separate = Vec::new();
    for (index, snapshot) in snapshots.iter().enumerate() {
        let path = dir.path().join(format!(
            "separate-{index}.{}",
            snapshot.movie_profile().container()
        ));
        let mut request = request(&path, FrameRate::new(24, 1).unwrap());
        request.clipping = ClippingPolicy::Saturate;
        separate.push(
            runtime
                .export_delivery_with_checkpoint(
                    &[snapshot],
                    &dir.path().join("absent.kronello"),
                    &[],
                    &CpuReferenceBackend,
                    &request,
                    &mut |_| Ok(()),
                )
                .unwrap(),
        );
    }
    let first = dir.path().join("shared.flac");
    let second = dir.path().join("shared.mp3");
    let mut request = request(&first, FrameRate::new(24, 1).unwrap());
    request.clipping = ClippingPolicy::Saturate;
    request.outputs = vec![DeliveryOutput {
        output: second.clone(),
        profile: MovieProfile::Mp3V1,
        background: [0.0; 3],
        chapters: ChapterPolicy::Omit,
    }];
    let diagnostics = ExportDiagnosticsCollector::enabled();
    let _scope = diagnostics.activate();
    let shared = runtime
        .export_delivery_with_checkpoint(
            &[&snapshots[0], &snapshots[1]],
            &dir.path().join("absent.kronello"),
            &[],
            &CpuReferenceBackend,
            &request,
            &mut |_| Ok(()),
        )
        .unwrap();
    let counts = diagnostics.snapshot().unwrap().counts;
    assert_eq!(counts["audio_mix_groups"], 1);
    assert_eq!(counts["audio_mixed_frames"], 6000);
    for (index, output) in [first, second].iter().enumerate() {
        let expected = separate[index].outputs[0].audio.as_ref().unwrap();
        let actual = shared.outputs[index].audio.as_ref().unwrap();
        assert!(actual.clipped_samples > 0);
        assert_eq!(actual, expected);
        assert_eq!(
            std::fs::read(output).unwrap(),
            std::fs::read(&separate[index].outputs[0].output).unwrap()
        );
    }
    let first = dir.path().join("reject.flac");
    let second = dir.path().join("reject.mp3");
    request.output = first.clone();
    request.outputs[0].output = second.clone();
    request.clipping = ClippingPolicy::Reject;
    let error = runtime
        .export_delivery_with_checkpoint(
            &[&snapshots[0], &snapshots[1]],
            &dir.path().join("absent.kronello"),
            &[],
            &CpuReferenceBackend,
            &request,
            &mut |_| Ok(()),
        )
        .unwrap_err();
    assert_eq!(error.code(), "AUDIO_CLIPPING");
    assert!(!first.exists() && !second.exists());
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
    assert_eq!(decoded.buffer, legacy.buffer);
}
#[test]
fn audio_source_cache_shares_verified_pcm_and_preserves_source_budget() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shared.wav");
    write_wave(&path, 44100, 1, 4410);
    let asset = asset(&path);
    let project = dir.path().join("project.kronello");
    let runtime = MediaRuntime::load().unwrap();
    let mut cache = AudioSourceCache::default();
    let first = cache.decode(&runtime, &asset, &project, 0, 4800).unwrap();
    let mut alias = asset.clone();
    alias.id = AssetId::new();
    let second = cache.decode(&runtime, &alias, &project, 0, 4800).unwrap();
    assert_eq!(first.buffer, second.buffer);
    assert_eq!(second.source_rate, 44100);
    assert_eq!(second.source_channels, 1);
    assert_eq!(second.source_start, first.source_start);
    assert_eq!(second.stream_index, first.stream_index);
    if cfg!(unix) {
        assert!(std::sync::Arc::ptr_eq(&first, &second));
        assert_eq!(cache.stats().hits, 1);
        assert_eq!(cache.stats().decoded_sources, 1);
        assert_eq!(cache.stats().retained_pcm_bytes, 4800 * 4);
    }
    assert_eq!(
        cache
            .decode(&runtime, &asset, &project, 0, 4799)
            .unwrap_err()
            .code(),
        "AUDIO_BUDGET_EXCEEDED"
    );
    // Stream selection remains part of the identity and the native validator.
    assert!(cache.decode(&runtime, &asset, &project, 1, 4800).is_err());
    cache.clear();
    assert_eq!(cache.stats().retained_sources, 0);
    assert_eq!(cache.stats().retained_pcm_bytes, 0);
    // An active plan owns its PCM independently of retained cache entries.
    assert_eq!(first.buffer.frame_count(), 4800);
}
#[test]
fn audio_source_cache_enforces_entry_and_pcm_limits_without_mutating_owners() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = MediaRuntime::load().unwrap();
    let project = dir.path().join("project.kronello");
    let assets: Vec<_> = (0..3)
        .map(|i| {
            let path = dir.path().join(format!("{i}.wav"));
            write_wave(&path, 48000, 2, 128 + i);
            asset(&path)
        })
        .collect();
    for limits in [
        AudioSourceCacheLimits {
            entries: 1,
            pcm_bytes: 4096,
        },
        AudioSourceCacheLimits {
            entries: 3,
            pcm_bytes: 1040,
        },
    ] {
        let mut cache = AudioSourceCache::new(limits);
        let owner = cache
            .decode(&runtime, &assets[0], &project, 0, 1000)
            .unwrap();
        for asset in &assets[1..] {
            cache.decode(&runtime, asset, &project, 0, 1000).unwrap();
            assert!(cache.stats().retained_pcm_bytes <= limits.pcm_bytes);
            assert!(cache.stats().retained_sources <= limits.entries);
        }
        assert_eq!(owner.buffer.frame_count(), 128);
        if cfg!(unix) {
            assert_eq!(cache.stats().evictions, 2);
        }
        let reloaded = cache
            .decode(&runtime, &assets[0], &project, 0, 1000)
            .unwrap();
        assert!(!std::sync::Arc::ptr_eq(&owner, &reloaded));
        assert_eq!(owner.buffer, reloaded.buffer);
    }
    for limits in [
        AudioSourceCacheLimits {
            entries: 0,
            pcm_bytes: 4096,
        },
        AudioSourceCacheLimits {
            entries: 3,
            pcm_bytes: 1023,
        },
    ] {
        let mut cache = AudioSourceCache::new(limits);
        let first = cache
            .decode(&runtime, &assets[0], &project, 0, 128)
            .unwrap();
        let second = cache
            .decode(&runtime, &assets[0], &project, 0, 128)
            .unwrap();
        assert!(!std::sync::Arc::ptr_eq(&first, &second));
        assert_eq!(cache.stats().retained_pcm_bytes, 0);
        assert_eq!(cache.stats().bypasses, 2);
    }
}
#[test]
fn audio_source_cache_rejects_rewrites_replacements_missing_and_wrong_relative_assets() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = MediaRuntime::load().unwrap();
    let path = dir.path().join("original.wav");
    write_wave(&path, 48000, 2, 128);
    let original = std::fs::read(&path).unwrap();
    let asset = asset(&path);
    let project = dir.path().join("project.kronello");
    let mut cache = AudioSourceCache::default();
    let owner = cache.decode(&runtime, &asset, &project, 0, 128).unwrap();
    let modified = path.metadata().unwrap().modified().unwrap();
    let mut wrong = original.clone();
    wrong[50] ^= 1;
    std::fs::write(&path, &wrong).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(modified))
        .unwrap();
    assert_eq!(path.metadata().unwrap().len(), original.len() as u64);
    assert_eq!(path.metadata().unwrap().modified().unwrap(), modified);
    assert_eq!(
        cache
            .decode(&runtime, &asset, &project, 0, 128)
            .unwrap_err()
            .code(),
        "ASSET_HASH_MISMATCH"
    );
    std::fs::write(&path, &original).unwrap();
    let restored = cache.decode(&runtime, &asset, &project, 0, 128).unwrap();
    assert!(!std::sync::Arc::ptr_eq(&owner, &restored));
    let replacement = dir.path().join("replacement.wav");
    std::fs::write(&replacement, &wrong).unwrap();
    std::fs::rename(&replacement, &path).unwrap();
    assert_eq!(
        cache
            .decode(&runtime, &asset, &project, 0, 128)
            .unwrap_err()
            .code(),
        "ASSET_HASH_MISMATCH"
    );
    std::fs::remove_file(&path).unwrap();
    assert_eq!(
        cache
            .decode(&runtime, &asset, &project, 0, 128)
            .unwrap_err()
            .code(),
        "ASSET_MISSING"
    );
    std::fs::create_dir(&path).unwrap();
    assert_eq!(
        cache
            .decode(&runtime, &asset, &project, 0, 128)
            .unwrap_err()
            .code(),
        "INVALID_MEDIA_INPUT"
    );
    std::fs::remove_dir(&path).unwrap();
    std::fs::write(&path, &original).unwrap();
    assert_eq!(
        cache
            .decode(&runtime, &asset, &project, 0, 128)
            .unwrap()
            .buffer,
        owner.buffer
    );
    let relative = dir.path().join("relative.wav");
    std::fs::write(relative, &wrong).unwrap();
    let mut redirected = asset.clone();
    redirected.locator.relative = Some("relative.wav".into());
    assert_eq!(
        cache
            .decode(&runtime, &redirected, &project, 0, 128)
            .unwrap_err()
            .code(),
        "ASSET_HASH_MISMATCH"
    );
    assert_eq!(owner.buffer.frame_count(), 128);
}
#[test]
fn bundled_pcm_fixture_decodes_exact_stereo_samples() {
    let runtime = MediaRuntime::load().unwrap();
    let bytes = std::fs::read(fixture()).unwrap();
    let decoded = runtime.decode_audio(&fixture(), 0).unwrap();
    assert_eq!(decoded.source_rate, 48000);
    assert_eq!(decoded.source_channels, 2);
    assert_eq!(decoded.source_start, Rational::ZERO);
    assert_eq!(decoded.buffer.frame_count(), 4800);
    let expected: Vec<_> = bytes[44..]
        .chunks_exact(4)
        .map(|b| {
            [
                f32::from(i16::from_le_bytes([b[0], b[1]])) / 32768.0,
                f32::from(i16::from_le_bytes([b[2], b[3]])) / 32768.0,
            ]
        })
        .collect();
    assert_eq!(decoded.buffer.stereo_frames().unwrap(), expected);
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
        // The decoder preserves the source layout: mono stays mono, never an
        // implicit stereo duplication.
        assert_eq!(
            decoded.buffer.mask(),
            if channels == 1 {
                kronello_model::ChannelMask::MONO
            } else {
                kronello_model::ChannelMask::STEREO
            }
        );
        assert_eq!(decoded.buffer.channels(), usize::from(channels));
        assert_eq!(
            decoded.buffer.frame_count(),
            4800,
            "drained sample count at {rate}"
        );
        for i in 64..4736 {
            let expected = 0.5 * (2.0 * std::f64::consts::PI * 440.0 * i as f64 / 48000.0).sin();
            let frame = decoded.buffer.frame(i).unwrap();
            assert!(
                (f64::from(frame[0]) - expected).abs() < 0.0003,
                "{rate}: sample {i}"
            );
            if channels == 2 {
                assert_eq!(frame[1], -frame[0]);
            }
        }
        assert!(
            decoded.buffer.frame(4790).unwrap()[0].abs() > 0.01,
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
        "UNSUPPORTED_CHANNEL_LAYOUT"
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
    assert_eq!(decoded.buffer.frame_count(), 4);
    for (actual, expected) in decoded.buffer.samples().iter().zip(frames.iter().flatten()) {
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
fn mp3_gapless_decode_keeps_nonzero_samples_and_exact_trimmed_length() {
    let runtime = MediaRuntime::load().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.wav");
    write_wave(&source, 48_000, 2, 96_000);
    let a = asset(&source);
    let decoded = runtime.decode_audio(&source, 0).unwrap();
    let range = TimeRange::new(t(1, 4), t(3, 4)).unwrap();
    let sources = AudioSources::from([(
        (a.id, 0),
        AudioBuffer::new(decoded.buffer.stereo_frames().unwrap()).unwrap(),
    )]);
    let c = clip(&a, TimeRange::new(Time::ZERO, t(1, 1)).unwrap(), 0.6);
    let bus = mix(std::slice::from_ref(&c), &sources, range).unwrap();
    let output = dir.path().join("trim.mp3");
    let (project, composition) = project(a);
    let render = RenderSnapshot::new(&project, composition, 0, RenderProfile::default()).unwrap();
    let snapshot = AvExportSnapshot::with_movie_profile(
        &render,
        AudioSourceMode::Explicit,
        vec![c],
        MovieProfile::Mp3V1,
    )
    .unwrap();
    let mut req = request(&output, FrameRate::new(24, 1).unwrap());
    req.range = range;
    runtime
        .export_delivery(&[&snapshot], dir.path(), &[], &CpuReferenceBackend, &req)
        .unwrap();
    let actual = runtime.decode_audio(&output, 0).unwrap();
    assert_eq!(actual.buffer.frame_count(), 24_000);
    assert_eq!(
        Some(actual.source_start),
        runtime.probe(&output).unwrap().streams[0].start
    );
    let expected: Vec<_> = bus.buffer().frames().iter().flatten().copied().collect();
    let actual_samples = actual.buffer.samples();
    assert_eq!(actual_samples.len(), expected.len());
    let max_error = actual_samples
        .iter()
        .zip(&expected)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0_f32, f32::max);
    assert!(max_error < 0.02, "lossy MP3 error {max_error}");
    assert!(actual_samples.iter().any(|s| s.abs() > 0.1));
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
            report.audio.as_ref().unwrap().frames,
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
        assert_eq!(
            audio_duration,
            t(report.audio.as_ref().unwrap().frames as i64, 48000)
        );
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
        assert_eq!(
            decoded.buffer.frame_count(),
            report.audio.as_ref().unwrap().frames
        );
        let source = runtime
            .decode_asset_audio(&a, &dir.path().join("project.kronello"), 0)
            .unwrap()
            .buffer;
        let sources = ChannelSources::from([((a.id, 0), source)]);
        let expected = mix_channels(
            snapshot.clips(),
            &sources,
            request.range,
            ChannelMask::STEREO,
        )
        .unwrap();
        for (actual, expected) in decoded
            .buffer
            .samples()
            .iter()
            .zip(expected.buffer().samples().iter())
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
        assert_eq!(decoded.buffer.samples().iter().any(|v| *v != 0.0), audible);
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
    assert!(decoded.buffer.samples().iter().any(|v| *v != 0.0));

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
        "UNSUPPORTED_CHANNEL_LAYOUT"
    );
    assert!(!req.output.exists());
}
