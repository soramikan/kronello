//! AUDIO-010 (ADR-0124): deterministic WSOLA pitch-preserving retime and
//! multichannel (channel-masked) mixing with explicit ITU downmix.
use kronello_audio::*;
use kronello_model::*;
use kronello_time::{FrameRate, SampleRate, Time, TimeMap, TimeRange};

fn t(n: i64, d: i64) -> Time {
    Time::new(n, d).unwrap()
}
fn r(a: Time, b: Time) -> TimeRange {
    TimeRange::new(a, b).unwrap()
}
/// Project with one audio asset and a sequence holding a single audio clip.
/// The clip's retime policy and time map are set by the caller.
fn fixture(asset_channels: usize) -> (Project, SequenceId, AssetId, ClipId) {
    let asset = Asset {
        id: AssetId::new(),
        kind: AssetKind::Audio,
        content_hash: "a".repeat(64),
        locator: AssetLocator {
            relative: Some("source.wav".into()),
            absolute: None,
        },
        streams: vec![StreamMetadata {
            index: 0,
            codec: "pcm_f32le".into(),
            time_base: t(1, 48000),
            duration: Some(t(2, 1)),
            start_time: None,
            width: None,
            height: None,
            pixel_format: None,
            color_primaries: None,
            color_transfer: None,
            color_matrix: None,
            color_range: None,
        }],
    };
    assert!((1..=8).contains(&asset_channels));
    let clip = Clip {
        id: ClipId::new(),
        source_ref: SourceRef::Asset {
            asset: asset.id,
            stream_index: 0,
        },
        timeline_range: r(Time::ZERO, t(1, 2)),
        source_in: Time::ZERO,
        time_map: TimeMap::linear(Time::ZERO, Time::ONE).unwrap(),
        enabled: true,
        audio_retime: AudioRetimePolicy::Reject,
        reverse_sampling: None,
        volume: None,
        links: vec![],
        effects: vec![],
        masks: vec![],
        properties: vec![],
        markers: vec![],
    };
    let clip_id = clip.id;
    let sequence = Sequence {
        id: SequenceId::new(),
        extent: DesignExtent::new(8.0, 8.0).unwrap(),
        frame_rate: FrameRate::new(24, 1).unwrap(),
        audio_rate: SampleRate::HZ_48000,
        working_space: ColorSpace::LinearRec709,
        tracks: vec![Track {
            state: None,
            id: TrackId::new(),
            kind: TrackKind::Audio,
            clips: vec![clip],
        }],
        transitions: vec![],
        markers: vec![],
        work_area: None,
        targets: None,
    };
    let sequence_id = sequence.id;
    let asset_id = asset.id;
    let project = Project {
        assets: vec![DocumentObject::Known(asset)],
        sequences: vec![DocumentObject::Known(sequence)],
        ..Project::default()
    };
    (project, sequence_id, asset_id, clip_id)
}
fn clip_mut(project: &mut Project) -> &mut Clip {
    let DocumentObject::Known(sequence) = &mut project.sequences[0] else {
        panic!()
    };
    &mut sequence.tracks[0].clips[0]
}
fn plan(project: &Project, sequence: SequenceId) -> DocumentAudioPlan {
    DocumentAudioPlan::compile_version(
        project,
        AudioTarget::Sequence(sequence),
        AUDIO_EVALUATION_VERSION,
    )
    .unwrap()
}
/// Dominant period in samples by normalized autocorrelation over `lag` in
/// [lo, hi]; deterministic first-maximum wins on ties.
fn best_lag(samples: &[f32], lo: usize, hi: usize) -> usize {
    let mut best = lo;
    let mut best_score = f64::NEG_INFINITY;
    for lag in lo..=hi.min(samples.len() / 2) {
        let mut dot = 0.0_f64;
        let mut ea = 0.0_f64;
        let mut eb = 0.0_f64;
        for i in 0..samples.len() - lag {
            let a = f64::from(samples[i]);
            let b = f64::from(samples[i + lag]);
            dot += a * b;
            ea += a * a;
            eb += b * b;
        }
        let score = if ea > 0.0 && eb > 0.0 {
            dot / (ea * eb).sqrt()
        } else {
            0.0
        };
        if score > best_score {
            best_score = score;
            best = lag;
        }
    }
    best
}
/// 240 Hz sine: exactly 200 samples per period at 48 kHz, so the
/// autocorrelation lag lands on an integer for both pitch and aliasing tests.
fn sine(frames: usize) -> AudioBuffer {
    AudioBuffer::new(
        (0..frames)
            .map(|i| [0.5 * (std::f64::consts::TAU * i as f64 / 200.0).sin() as f32; 2])
            .collect(),
    )
    .unwrap()
}
#[test]
fn wsola_retime_preserves_pitch_and_duration_deterministically() {
    let (mut p, id, asset, _clip) = fixture(2);
    clip_mut(&mut p).audio_retime = AudioRetimePolicy::PitchPreserveV1;
    clip_mut(&mut p).time_map = TimeMap::linear(Time::ZERO, t(2, 1)).unwrap();
    let sources = AudioSources::from([((asset, 0), sine(96_000))]);
    let range = clip_mut(&mut p).timeline_range;
    let bus = plan(&p, id).mix(&sources, range).unwrap();
    // Output duration follows the authored placement, not the stretched input.
    assert_eq!(bus.buffer().frames().len(), 24_000);
    // Pitch is preserved: the dominant period stays at the 240 Hz / 200-sample
    // source period rather than the 480 Hz result of resample_v1.
    let mono: Vec<f32> = bus.buffer().frames()[4096..20_000]
        .iter()
        .map(|f| f[0])
        .collect();
    assert_eq!(best_lag(&mono, 40, 400), 200);
    clip_mut(&mut p).audio_retime = AudioRetimePolicy::ResampleV1;
    let shifted = plan(&p, id).mix(&sources, range).unwrap();
    let shifted_mono: Vec<f32> = shifted.buffer().frames()[4096..20_000]
        .iter()
        .map(|f| f[0])
        .collect();
    assert_eq!(best_lag(&shifted_mono, 40, 400), 100);
    clip_mut(&mut p).audio_retime = AudioRetimePolicy::PitchPreserveV1;
    // Determinism: identical repeat evaluation, and a partitioned render
    // reproduces the continuous render bit-exactly (stateful WSOLA cursors
    // always evaluate from the placement start, ADR-0117/0124).
    let repeat = plan(&p, id).mix(&sources, range).unwrap();
    assert_eq!(bus.buffer().frames(), repeat.buffer().frames());
    let cut = t(1001, 48000);
    let left = plan(&p, id).mix(&sources, r(range.start(), cut)).unwrap();
    let right = plan(&p, id).mix(&sources, r(cut, range.end())).unwrap();
    let joined: Vec<_> = left
        .buffer()
        .frames()
        .iter()
        .chain(right.buffer().frames())
        .copied()
        .collect();
    assert_eq!(joined, bus.buffer().frames());
    // A hold segment (piecewise slope 0) emits silence per the NLE-006
    // contract instead of repeating frozen source samples.
    clip_mut(&mut p).time_map = TimeMap::piecewise_linear(vec![
        kronello_time::TimeMapPoint {
            parent: Time::ZERO,
            local: Time::ZERO,
        },
        kronello_time::TimeMapPoint {
            parent: t(1, 4),
            local: t(1, 4),
        },
        kronello_time::TimeMapPoint {
            parent: t(1, 2),
            local: t(1, 4),
        },
    ])
    .unwrap();
    let held = plan(&p, id).mix(&sources, range).unwrap();
    // Hold applies per WSOLA hop at the anchor: the first hop fully inside the
    // hold (anchor 12288) and everything after it is silent.
    assert!(
        held.buffer().frames()[12_288..]
            .iter()
            .all(|f| *f == [0.0; 2])
    );
}
#[test]
fn multichannel_sources_keep_layout_or_downmix_only_through_the_matrix() {
    let (mut p, id, asset, _clip) = fixture(6);
    clip_mut(&mut p).audio_retime = AudioRetimePolicy::ResampleV1;
    // Channel-distinct constant signal: FL .1 FR .2 C .3 LFE .4 SL .5 SR .6.
    let amplitude = [0.1_f32, 0.2, 0.3, 0.4, 0.5, 0.6];
    let mut samples = vec![0.0_f32; 24_000 * 6];
    for frame in 0..24_000 {
        for channel in 0..6 {
            samples[frame * 6 + channel] = amplitude[channel];
        }
    }
    let source = ChannelBuffer::new(ChannelMask::SURROUND_5_1, samples).unwrap();
    let sources = ChannelSources::from([((asset, 0), source)]);
    let range = clip_mut(&mut p).timeline_range;
    // 5.1 target: same-position passthrough at unity, no synthesis.
    let surround = plan(&p, id)
        .mix_channels(&sources, range, ChannelMask::SURROUND_5_1)
        .unwrap();
    assert_eq!(surround.buffer().mask(), ChannelMask::SURROUND_5_1);
    assert_eq!(surround.buffer().frame_count(), 24_000);
    assert_eq!(surround.buffer().frame(1000).unwrap(), amplitude);
    // Stereo target: explicit ITU fold-down — L = FL + k·(C + SL), LFE dropped.
    let stereo = plan(&p, id)
        .mix_channels(&sources, range, ChannelMask::STEREO)
        .unwrap();
    assert_eq!(stereo.buffer().mask(), ChannelMask::STEREO);
    let k = std::f64::consts::FRAC_1_SQRT_2;
    let left = (0.1_f64 + (0.3 + 0.5) * k) as f32;
    let right = (0.2_f64 + (0.3 + 0.6) * k) as f32;
    let frame = stereo.buffer().frame(1000).unwrap();
    assert!((f64::from(frame[0]) - f64::from(left)).abs() < 1e-7);
    assert!((f64::from(frame[1]) - f64::from(right)).abs() < 1e-7);
    // The stereo compatibility path performs the same explicit downmix.
    let legacy = plan(&p, id).mix_reader(&sources, range).unwrap();
    assert_eq!(
        legacy.buffer().frames(),
        stereo.buffer().stereo_frames().unwrap().as_slice()
    );
    // Mono target: −6 dB fronts, −3 dB center, −9 dB surrounds; LFE dropped.
    let mono = plan(&p, id)
        .mix_channels(&sources, range, ChannelMask::MONO)
        .unwrap();
    let expected = (0.05_f64 + 0.1 + 0.3 * k + (0.5 + 0.6) * (k / 2.0)) as f32;
    assert!((f64::from(mono.buffer().frame(1000).unwrap()[0]) - f64::from(expected)).abs() < 1e-7);
    // There is no implicit fold-down: a multichannel bus cannot be exposed as
    // the legacy stereo Bus without an explicit conversion.
    let error = plan(&p, id)
        .mix_channels(&sources, range, ChannelMask::SURROUND_5_1)
        .unwrap()
        .into_stereo_bus()
        .unwrap_err();
    assert_eq!(error.code(), "UNSUPPORTED_CHANNEL_LAYOUT");
    // A mask/count mismatch at a reader boundary is the same typed failure.
    let mismatched = MismatchedSources(ChannelSources::from([(
        (asset, 0),
        ChannelBuffer::new(ChannelMask::SURROUND_7_1, vec![0.0; 24_000 * 8]).unwrap(),
    )]));
    assert_eq!(
        plan(&p, id)
            .mix_channels(&mismatched, range, ChannelMask::STEREO)
            .unwrap_err()
            .code(),
        "UNSUPPORTED_CHANNEL_LAYOUT"
    );
}
/// Reports 5.1 while the stored buffer is actually 7.1: the mismatch is a
/// typed UNSUPPORTED_CHANNEL_LAYOUT, never a silent fold-down.
struct MismatchedSources(ChannelSources);
impl ChannelSourceReader for MismatchedSources {
    fn layout(&self, asset: AssetId, stream: u32) -> Result<ChannelMask, AudioError> {
        // Report 5.1 while the stored buffer is actually 7.1: read_frame fails.
        let _ = self.0.layout(asset, stream)?;
        Ok(ChannelMask::SURROUND_5_1)
    }
    fn frame_count(&self, asset: AssetId, stream: u32) -> Result<usize, AudioError> {
        self.0.frame_count(asset, stream)
    }
    fn read_frame(
        &self,
        asset: AssetId,
        stream: u32,
        index: usize,
        out: &mut [f32],
    ) -> Result<(), AudioError> {
        if out.len() != 6 {
            return Err(AudioError::UnsupportedChannelLayout(
                "reader frame width".into(),
            ));
        }
        self.0.read_frame(asset, stream, index, out)
    }
}
#[test]
fn wsola_rejects_unsupported_maps_and_short_sources_with_typed_errors() {
    let (mut p, id, asset, _clip) = fixture(2);
    clip_mut(&mut p).audio_retime = AudioRetimePolicy::PitchPreserveV1;
    clip_mut(&mut p).time_map = TimeMap::linear(Time::ZERO, t(2, 1)).unwrap();
    let sources = AudioSources::from([((asset, 0), sine(96_000))]);
    let range = clip_mut(&mut p).timeline_range;
    assert!(plan(&p, id).mix(&sources, range).is_ok());
    // The placement at speed 2 consumes 48 000 source samples; a 24 000
    // sample source is a typed AUDIO_SOURCE_TOO_SHORT — never zero-filled.
    let short = AudioSources::from([((asset, 0), sine(24_000))]);
    assert_eq!(
        plan(&p, id).mix(&short, range).unwrap_err().code(),
        "AUDIO_SOURCE_TOO_SHORT"
    );
    // Pitch-preserved retime cannot combine with reverse playback: the model
    // requires reverse_grid_v1 to pair with reverse_resample_v1, so plan
    // compilation fails before any sample evaluation.
    clip_mut(&mut p).reverse_sampling = Some(ReverseSampling::ReverseGridV1);
    clip_mut(&mut p).source_in = t(1, 1);
    assert_eq!(
        DocumentAudioPlan::compile_version(&p, AudioTarget::Sequence(id), AUDIO_EVALUATION_VERSION)
            .unwrap_err()
            .code(),
        "UNSUPPORTED_FEATURE"
    );
}
#[test]
fn channel_bus_quantizes_every_channel_with_shared_pcm24_rules() {
    let (mut p, id, asset, _clip) = fixture(6);
    clip_mut(&mut p).audio_retime = AudioRetimePolicy::ResampleV1;
    let mut samples = vec![0.0_f32; 24_000 * 6];
    for frame in 0..24_000 {
        for channel in 0..6 {
            samples[frame * 6 + channel] = if channel % 2 == 0 { 0.5 } else { -0.5 };
        }
    }
    let source = ChannelBuffer::new(ChannelMask::SURROUND_5_1, samples).unwrap();
    let sources = ChannelSources::from([((asset, 0), source)]);
    let range = clip_mut(&mut p).timeline_range;
    let bus = plan(&p, id)
        .mix_channels(&sources, range, ChannelMask::SURROUND_5_1)
        .unwrap();
    let pcm = bus.quantize_pcm24(ClippingPolicy::Reject).unwrap();
    assert_eq!(pcm.samples.len(), bus.buffer().frame_count() * 6);
    assert_eq!(pcm.samples.len() % 6, 0);
    // ±0.5 quantizes to the same int24 codes as the stereo path produces:
    // +0.5 → +2^30, -0.5 → -2^30 in high-24-bit S32 form.
    for chunk in pcm.samples.chunks_exact(6) {
        assert_eq!(
            chunk,
            &[
                1 << 30,
                -(1 << 30),
                1 << 30,
                -(1 << 30),
                1 << 30,
                -(1 << 30)
            ]
        );
    }
}
