//! AUDIO-010 media boundary: native channel-mask decode/encode, 5.1
//! round-trip, explicit export layouts and typed layout rejections.
use kronello_audio::*;
use kronello_gpu::render_adapter::CpuReferenceBackend;
use kronello_media::*;
use kronello_model::*;
use kronello_render::*;
use kronello_time::{Duration, FrameRate, Rational, Time, TimeRange};
use std::f64::consts::{FRAC_PI_3, TAU};
use std::path::Path;

fn t(n: i64, d: i64) -> Rational {
    Rational::new(n, d).unwrap()
}
/// PCM16 WAV with a WAVEFORMATEXTENSIBLE `dwChannelMask`. Each channel carries
/// the same 440 Hz sine at a distinct phase so channel order survives intact.
fn write_wave_ext(path: &Path, rate: u32, channels: u16, mask: u32, frames: usize) {
    let mut data = Vec::new();
    for i in 0..frames {
        for channel in 0..channels {
            let phase = TAU * 440.0 * i as f64 / f64::from(rate) + f64::from(channel) * FRAC_PI_3;
            let sample = (0.25 * phase.sin() * 32767.0).round() as i16;
            data.extend(sample.to_le_bytes());
        }
    }
    let mut fmt = Vec::new();
    fmt.extend(0xFFFEu16.to_le_bytes()); // WAVE_FORMAT_EXTENSIBLE
    fmt.extend(channels.to_le_bytes());
    fmt.extend(rate.to_le_bytes());
    fmt.extend((rate * u32::from(channels) * 2).to_le_bytes());
    fmt.extend((channels * 2).to_le_bytes());
    fmt.extend(16u16.to_le_bytes());
    fmt.extend(22u16.to_le_bytes()); // cbSize
    fmt.extend(16u16.to_le_bytes()); // wValidBitsPerSample
    fmt.extend(mask.to_le_bytes()); // dwChannelMask
    fmt.extend(1u32.to_le_bytes()); // SubFormat PCM, Data1
    fmt.extend(0u16.to_le_bytes()); // Data2
    fmt.extend(0x0010u16.to_le_bytes()); // Data3
    fmt.extend([0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71]); // Data4
    let mut bytes = Vec::new();
    bytes.extend(b"RIFF");
    bytes.extend((52 + data.len() as u32).to_le_bytes());
    bytes.extend(b"WAVE");
    bytes.extend(b"fmt ");
    bytes.extend((fmt.len() as u32).to_le_bytes());
    bytes.extend(fmt);
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
/// Expected s16 PCM amplitude at frame/channel of `write_wave_ext`.
fn pcm16(i: usize, channel: usize, rate: u32) -> f32 {
    let phase = TAU * 440.0 * i as f64 / f64::from(rate) + channel as f64 * FRAC_PI_3;
    (0.25 * phase.sin() * 32767.0).round() as f32 / 32768.0
}
#[test]
fn surround_wav_decodes_native_masks_and_preserves_channel_order() {
    let runtime = MediaRuntime::load().unwrap();
    let dir = tempfile::tempdir().unwrap();
    for (mask, layout, channels) in [
        (0x3f_u32, ChannelMask::SURROUND_5_1_BACK, 6_usize),
        (0x60f, ChannelMask::SURROUND_5_1, 6),
        (0x63f, ChannelMask::SURROUND_7_1, 8),
    ] {
        let path = dir.path().join(format!("{mask:x}.wav"));
        write_wave_ext(&path, 48000, channels as u16, mask, 4800);
        let decoded = runtime.decode_audio(&path, 0).unwrap();
        assert_eq!(decoded.buffer.mask(), layout, "mask {mask:x}");
        assert_eq!(decoded.buffer.channels(), channels);
        assert_eq!(decoded.source_channels as usize, channels);
        assert_eq!(decoded.buffer.frame_count(), 4800);
        for i in [0, 100, 1000, 4799] {
            let frame = decoded.buffer.frame(i).unwrap();
            for (channel, &actual) in frame.iter().enumerate() {
                let expected = pcm16(i, channel, 48000);
                assert!(
                    (f64::from(actual) - f64::from(expected)).abs() < 1e-6,
                    "mask {mask:x} frame {i} channel {channel}: {actual} vs {expected}"
                );
            }
        }
        let probe = runtime.probe(&path).unwrap();
        let stream = probe
            .streams
            .iter()
            .find(|s| s.kind == StreamKind::Audio)
            .unwrap();
        assert_eq!(stream.channels, Some(channels as u32));
        assert_eq!(stream.channel_mask, Some(u64::from(mask)));
    }
}
#[test]
fn pcm24_surround_encode_decode_roundtrip_preserves_mask_and_samples() {
    let runtime = MediaRuntime::load().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.wav");
    write_wave_ext(&path, 48000, 6, 0x60f, 4800);
    let decoded = runtime.decode_audio(&path, 0).unwrap();
    let asset_id = AssetId::new();
    let sources = ChannelSources::from([((asset_id, 0), decoded.buffer.clone())]);
    let clip = AudioClip {
        asset: asset_id,
        stream_index: 0,
        placement: TimeRange::new(Time::ZERO, t(4800, 48000)).unwrap(),
        source_in: Rational::ZERO,
        gain: Gain::UNITY,
    };
    let bus = mix_channels(
        std::slice::from_ref(&clip),
        &sources,
        clip.placement,
        ChannelMask::SURROUND_5_1,
    )
    .unwrap();
    let output = dir.path().join("surround.mov");
    let report = runtime
        .encode_audio_channels(&bus, ClippingPolicy::Reject, &output)
        .unwrap();
    assert_eq!(report.channels, 6);
    assert_eq!(report.frames, 4800);
    assert_eq!(report.clipped_samples, 0);
    let probe = runtime.probe(&output).unwrap();
    let stream = &probe.streams[0];
    assert_eq!(stream.codec, "pcm_s24le");
    assert_eq!(stream.channels, Some(6));
    assert_eq!(
        stream.channel_mask.map(ChannelMask::from_bits),
        Some(Ok(ChannelMask::SURROUND_5_1))
    );
    let roundtrip = runtime.decode_audio(&output, 0).unwrap();
    assert_eq!(roundtrip.buffer.mask(), ChannelMask::SURROUND_5_1);
    assert_eq!(roundtrip.buffer.frame_count(), 4800);
    for (actual, expected) in roundtrip
        .buffer
        .samples()
        .iter()
        .zip(bus.buffer().samples())
    {
        assert!((actual - expected).abs() <= 1.0 / 8388608.0);
    }
    // The channel count is part of the mux contract: declaring stereo against
    // a 5.1 intermediate is a typed failure.
    assert!(matches!(
        runtime.mux_movie(
            &output,
            &output,
            &dir.path().join("never.mov"),
            &"0".repeat(64),
            &"1".repeat(64),
            MovieProfile::ProResPcm24,
            ChannelMask::STEREO,
        ),
        Err(e) if e.code() == "ENCODE_ERROR" || e.code() == "INVALID_MEDIA_INPUT"
    ));
}
#[test]
fn export_av_multichannel_layout_reaches_the_mux_and_probe() {
    let runtime = MediaRuntime::load().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("surround.wav");
    write_wave_ext(&path, 48000, 6, 0x60f, 48000);
    let a = asset(&path);
    let (p, id) = project(a.clone());
    let render = RenderSnapshot::new(&p, id, 7, RenderProfile::default()).unwrap();
    let rate = FrameRate::new(24, 1).unwrap();
    let output = dir.path().join("surround.mov");
    let request = request(&output, rate);
    // A declared non-stereo layout pins audio envelope 3.
    let snapshot = AvExportSnapshot::with_audio_layout(
        &render,
        AudioSourceMode::Explicit,
        vec![clip(&a, request.range, 1.0)],
        None,
        ChannelMask::SURROUND_5_1,
    )
    .unwrap();
    assert_eq!(snapshot.audio_layout(), ChannelMask::SURROUND_5_1);
    // Stereo is rejected by the layout constructor (standard constructors
    // keep producing byte-identical envelopes).
    assert_eq!(
        AvExportSnapshot::with_audio_layout(
            &render,
            AudioSourceMode::Explicit,
            vec![],
            None,
            ChannelMask::STEREO,
        )
        .unwrap_err()
        .code(),
        "INVALID_MEDIA_INPUT"
    );
    // The layout is part of the snapshot identity.
    let stereo = AvExportSnapshot::new(&render, snapshot.clips().to_vec()).unwrap();
    assert_ne!(
        snapshot.content_hash().unwrap(),
        stereo.content_hash().unwrap()
    );
    let report = runtime
        .export_av(
            &snapshot,
            &dir.path().join("project.kronello"),
            &[],
            &CpuReferenceBackend,
            &request,
        )
        .unwrap();
    assert_eq!(report.audio.channels, 6);
    assert_eq!(report.audio_profile_version, 3);
    report
        .probe
        .verify_movie_layout(MovieProfile::ProResPcm24, ChannelMask::SURROUND_5_1)
        .unwrap();
    let stream = report
        .probe
        .streams
        .iter()
        .find(|s| s.kind == StreamKind::Audio)
        .unwrap();
    let decoded = runtime.decode_audio(&output, stream.index).unwrap();
    assert_eq!(decoded.buffer.mask(), ChannelMask::SURROUND_5_1);
    assert_eq!(decoded.buffer.frame_count(), report.audio.frames);
    // The decoded output equals the evaluator's own 5.1 mix within PCM24.
    let source = runtime
        .decode_asset_audio(&a, &dir.path().join("project.kronello"), 0)
        .unwrap()
        .buffer;
    let sources = ChannelSources::from([((a.id, 0), source)]);
    let expected = mix_channels(
        snapshot.clips(),
        &sources,
        request.range,
        ChannelMask::SURROUND_5_1,
    )
    .unwrap();
    for (actual, expected) in decoded
        .buffer
        .samples()
        .iter()
        .zip(expected.buffer().samples())
    {
        assert!((actual - expected).abs() <= 1.0 / 8388608.0);
    }
}
#[test]
fn export_av_stereo_layout_is_the_documented_explicit_downmix() {
    let runtime = MediaRuntime::load().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("surround.wav");
    write_wave_ext(&path, 48000, 6, 0x60f, 48000);
    let a = asset(&path);
    let (p, id) = project(a.clone());
    let render = RenderSnapshot::new(&p, id, 7, RenderProfile::default()).unwrap();
    let rate = FrameRate::new(24, 1).unwrap();
    let output = dir.path().join("stereo.mov");
    let request = request(&output, rate);
    // A stereo export of multichannel content must equal the evaluator's
    // explicit ITU downmix — never an implicit codec/container fold-down.
    let snapshot = AvExportSnapshot::new(&render, vec![clip(&a, request.range, 1.0)]).unwrap();
    let report = runtime
        .export_av(
            &snapshot,
            &dir.path().join("project.kronello"),
            &[],
            &CpuReferenceBackend,
            &request,
        )
        .unwrap();
    assert_eq!(report.audio.channels, 2);
    report.probe.verify_av().unwrap();
    let stream = report
        .probe
        .streams
        .iter()
        .find(|s| s.kind == StreamKind::Audio)
        .unwrap();
    let decoded = runtime.decode_audio(&output, stream.index).unwrap();
    assert_eq!(decoded.buffer.channels(), 2);
    let source = runtime
        .decode_asset_audio(&a, &dir.path().join("project.kronello"), 0)
        .unwrap()
        .buffer;
    assert_eq!(source.mask(), ChannelMask::SURROUND_5_1);
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
        .zip(expected.buffer().samples())
    {
        assert!((actual - expected).abs() <= 1.0 / 8388608.0);
    }
}
#[test]
fn undeclared_layouts_and_layout_changes_stay_typed_errors() {
    let runtime = MediaRuntime::load().unwrap();
    let dir = tempfile::tempdir().unwrap();
    // A >2 channel file with no speaker mask is rejected, never guessed.
    let plain = dir.path().join("plain.wav");
    let mut data = Vec::new();
    for _ in 0..4800 {
        for _ in 0..6 {
            data.extend(0i16.to_le_bytes());
        }
    }
    let mut bytes = Vec::new();
    bytes.extend(b"RIFF");
    bytes.extend((36 + data.len() as u32).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(6u16.to_le_bytes());
    bytes.extend(48000u32.to_le_bytes());
    bytes.extend((48000 * 12u32).to_le_bytes());
    bytes.extend(12u16.to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend((data.len() as u32).to_le_bytes());
    bytes.extend(data);
    std::fs::write(&plain, bytes).unwrap();
    let error = runtime.decode_audio(&plain, 0).unwrap_err();
    assert_eq!(error.code(), "UNSUPPORTED_CHANNEL_LAYOUT");
    // An unknown bit combination in the declared mask is rejected identically.
    assert_eq!(
        ChannelMask::from_bits(0x607).unwrap_err().to_string(),
        "UNSUPPORTED_CHANNEL_LAYOUT: unsupported channel mask 0x607"
    );
    // A mono WAV still normalizes through FFmpeg's documented default.
    let mono = dir.path().join("mono.wav");
    write_wave_ext(&mono, 48000, 1, ChannelMask::MONO.bits() as u32, 4800);
    let decoded = runtime.decode_audio(&mono, 0).unwrap();
    assert_eq!(decoded.buffer.mask(), ChannelMask::MONO);
    assert_eq!(decoded.buffer.channels(), 1);
}
