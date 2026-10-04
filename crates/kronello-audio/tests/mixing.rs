use kronello_audio::*;
use kronello_model::AssetId;
use kronello_time::{FrameRate, Rational, Time, TimeRange};
fn t(n: i64, d: i64) -> Time {
    Time::new(n, d).unwrap()
}
fn range(a: i64, b: i64) -> TimeRange {
    TimeRange::new(t(a, 48000), t(b, 48000)).unwrap()
}
fn clip(asset: AssetId, start: i64, end: i64, source_in: i64, gain: f32) -> AudioClip {
    AudioClip {
        asset,
        stream_index: 0,
        placement: range(start, end),
        source_in: t(source_in, 48000),
        gain: Gain::new(gain).unwrap(),
    }
}
#[test]
fn absolute_sample_boundaries_cover_fractional_frames_without_drift() {
    for (n, d, pattern) in [
        (24, 1, vec![2000; 5]),
        (30000, 1001, vec![1601, 1602, 1601, 1602, 1602]),
        (60000, 1001, vec![800, 801, 801, 801, 801]),
    ] {
        let rate = FrameRate::new(n, d).unwrap();
        let actual: Vec<_> = (0..5)
            .map(|i| {
                let samples = sample_range(rate.frame_range(i).unwrap()).unwrap();
                samples.end - samples.start
            })
            .collect();
        assert_eq!(actual, pattern);
        for frame in [-1_000_000, -1, 0, 1, 2, 1_000_000, 86_400 * n / d] {
            let time = rate.frame_to_time(frame).unwrap();
            let index = sample_index(time).unwrap();
            assert_eq!(
                i128::from(index),
                (i128::from(frame) * i128::from(d) * 48000).div_euclid(i128::from(n))
            );
            let error = time
                .checked_sub(SAMPLE_RATE.sample_to_time(index).unwrap())
                .unwrap();
            assert!(error >= Rational::ZERO && error < t(1, 48000));
            let here = sample_range(rate.frame_range(frame).unwrap()).unwrap();
            let next = sample_range(rate.frame_range(frame + 1).unwrap()).unwrap();
            assert_eq!(here.end, next.start);
        }
    }
}
#[test]
fn overlap_gain_trim_negative_placement_and_silence_preserve_float_headroom() {
    let id = AssetId::new();
    let sources = AudioSources::from([(
        (id, 0),
        AudioBuffer::new(vec![[0.75, -0.5], [0.5, 0.25], [1.0, -1.0], [0.2, 0.4]]).unwrap(),
    )]);
    let clips = [clip(id, -1, 2, 1, 2.0), clip(id, 0, 2, 0, 1.0)];
    let bus = mix(&clips, &sources, range(-2, 3)).unwrap();
    assert_eq!(bus.start_sample(), -2);
    assert_eq!(
        bus.buffer().frames(),
        &[
            [0.0, 0.0],
            [1.0, 0.5],
            [2.75, -2.5],
            [0.9, 1.05],
            [0.0, 0.0]
        ]
    );
    assert!(matches!(
        bus.quantize_pcm24(ClippingPolicy::Reject),
        Err(AudioError::Clipping { samples: 3 })
    ));
    let q = bus.quantize_pcm24(ClippingPolicy::Saturate).unwrap();
    assert_eq!(q.clipped_samples, 3);
    assert_eq!(q.samples[4], 8_388_607 * 256);
    assert_eq!(q.samples[5], i32::MIN);
}
#[test]
fn partitioned_mixing_matches_whole_bus_at_ntsc_boundaries_and_arbitrary_request_order() {
    for rate in [
        FrameRate::new(24, 1).unwrap(),
        FrameRate::new(30000, 1001).unwrap(),
        FrameRate::new(60000, 1001).unwrap(),
    ] {
        let id = AssetId::new();
        let end = sample_index(rate.frame_to_time(5).unwrap()).unwrap();
        let source = AudioBuffer::new(
            (0..end)
                .map(|i| [i as f32 / 100000.0, -(i as f32) / 100000.0])
                .collect(),
        )
        .unwrap();
        let sources = AudioSources::from([((id, 0), source)]);
        let clip = AudioClip {
            asset: id,
            stream_index: 0,
            placement: TimeRange::new(Time::ZERO, rate.frame_to_time(5).unwrap()).unwrap(),
            source_in: Time::ZERO,
            gain: Gain::UNITY,
        };
        let whole = mix(std::slice::from_ref(&clip), &sources, clip.placement).unwrap();
        let mut parts = vec![vec![]; 5];
        for frame in [4, 1, 3, 0, 2] {
            let batch = mix(
                std::slice::from_ref(&clip),
                &sources,
                rate.frame_range(frame).unwrap(),
            )
            .unwrap();
            parts[frame as usize] = batch.buffer().frames().to_vec();
        }
        assert_eq!(parts.concat(), whole.buffer().frames());
    }
}
#[test]
fn pcm24_rounding_full_scale_endpoints_and_no_implicit_clipping() {
    let id = AssetId::new();
    let sources = AudioSources::from([(
        (id, 0),
        AudioBuffer::new(vec![
            [-1.0, 1.0],
            [0.5, -0.5],
            [0.5 / 8388608.0, -0.5 / 8388608.0],
        ])
        .unwrap(),
    )]);
    let bus = mix(&[clip(id, 0, 3, 0, 1.0)], &sources, range(0, 3)).unwrap();
    let encoded = bus.quantize_pcm24(ClippingPolicy::Reject).unwrap();
    assert_eq!(
        encoded.samples,
        vec![
            i32::MIN,
            8_388_607 * 256,
            1_073_741_824,
            -1_073_741_824,
            256,
            -256
        ]
    );
    assert_eq!(encoded.clipped_samples, 0);
}
#[test]
fn invalid_gain_nonfinite_sources_overflow_missing_and_short_assets_fail() {
    for gain in [-1.0, f32::NAN, f32::INFINITY] {
        assert!(Gain::new(gain).is_err());
    }
    assert!(AudioBuffer::new(vec![[f32::NAN, 0.0]]).is_err());
    let id = AssetId::new();
    let clips = [clip(id, 0, 2, 0, 1.0)];
    assert!(matches!(
        mix(&clips, &AudioSources::new(), range(0, 2)),
        Err(AudioError::AssetMissing(_))
    ));
    let sources = AudioSources::from([((id, 0), AudioBuffer::new(vec![[0.1, 0.1]]).unwrap())]);
    assert!(matches!(
        mix(&clips, &sources, range(0, 2)),
        Err(AudioError::SourceTooShort(_))
    ));
    let source = AudioBuffer::new(vec![[2.0, 0.0]]).unwrap();
    assert!(matches!(
        apply_gain(&source, Gain::new(f32::MAX).unwrap()),
        Err(AudioError::Overflow)
    ));
    assert!(sample_index(t(i64::MAX, 1)).is_err());
    assert!(
        mix(
            &[],
            &AudioSources::new(),
            range(0, MAX_AUDIO_FRAMES as i64 + 1)
        )
        .is_err()
    );
}
#[test]
fn source_trim_and_placement_boundaries_use_absolute_floor() {
    let id = AssetId::new();
    let sources = AudioSources::from([(
        (id, 0),
        AudioBuffer::new(vec![[0.1, 0.2], [0.3, 0.4], [0.5, 0.6]]).unwrap(),
    )]);
    let clip = AudioClip {
        asset: id,
        stream_index: 0,
        placement: TimeRange::new(t(1, 96000), t(5, 96000)).unwrap(),
        source_in: t(3, 96000),
        gain: Gain::UNITY,
    };
    let bus = mix(&[clip], &sources, range(-1, 3)).unwrap();
    assert_eq!(
        bus.buffer().frames(),
        &[[0.0, 0.0], [0.3, 0.4], [0.5, 0.6], [0.0, 0.0]]
    );
}
