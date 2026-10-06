use kronello_audio::{AudioBuffer, analyze_audio};
use kronello_model::{AssetId, AudioAnalysisConfig, AudioAnalysisSource, AudioFeature};
use kronello_time::{Rational, Time, TimeMap};
fn config() -> AudioAnalysisConfig {
    AudioAnalysisConfig {
        version: 1,
        sample_rate: 48000,
        window: 1024,
        hop: 256,
        bands: vec![[900, 1100], [4000, 6000]],
        time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
    }
}
fn source() -> AudioAnalysisSource {
    AudioAnalysisSource::Asset {
        asset: AssetId::new(),
        stream_index: 0,
        content_hash: "a".repeat(64),
    }
}
#[test]
fn tone_energy_rms_and_exact_random_access() {
    // Exact FFT bin 20 = 937.5 Hz, amplitude 0.5.
    let frames: Vec<_> = (0..2048)
        .map(|i| {
            let v = (std::f64::consts::TAU * 20.0 * i as f64 / 1024.0).sin() as f32 * 0.5;
            [v, v]
        })
        .collect();
    let data = analyze_audio(
        AssetId::new(),
        source(),
        config(),
        0,
        &AudioBuffer::new(frames).unwrap(),
    )
    .unwrap();
    assert!((data.frames[0].rms - 0.5 / 2.0f64.sqrt()).abs() < 1e-8);
    assert!((data.frames[0].band_energy[0] - 0.125).abs() < 1e-8);
    assert!(data.frames[0].band_energy[1] < 1e-12);
    assert_eq!(data.frames[3].time, Time::new(768, 48000).unwrap());
    for index in [3, 0, 2, 1, 3] {
        assert_eq!(
            data.sample(Time::new(index * 256, 48000).unwrap(), AudioFeature::Rms)
                .unwrap(),
            data.frames[index as usize].rms
        );
    }
    assert!(
        data.sample(Time::new(2048, 48000).unwrap(), AudioFeature::Rms)
            .is_err()
    );
    let mut shifted = data.clone();
    shifted.config.time_map =
        TimeMap::linear(Time::new(256, 48000).unwrap(), Rational::ONE).unwrap();
    assert_eq!(
        shifted.sample(Time::ZERO, AudioFeature::Rms).unwrap(),
        data.frames[1].rms
    );
}
#[test]
fn impulse_onset_beats_silence_padding_and_limits() {
    let silence = analyze_audio(
        AssetId::new(),
        source(),
        config(),
        17,
        &AudioBuffer::new(vec![[0.0; 2]; 1025]).unwrap(),
    )
    .unwrap();
    assert_eq!(silence.frames.len(), 5);
    assert!(
        silence
            .frames
            .iter()
            .all(|f| f.rms == 0.0 && !f.beat && f.onset == 0.0)
    );
    assert_eq!(silence.frames[0].time, Time::new(17, 48000).unwrap());
    let pulse = analyze_audio(
        AssetId::new(),
        source(),
        config(),
        0,
        &AudioBuffer::new(vec![[0.5; 2]; 1024]).unwrap(),
    )
    .unwrap();
    assert!(pulse.frames[0].beat);
    assert_eq!(pulse.frames[0].onset, 0.5);
    assert!(pulse.frames.iter().skip(1).all(|f| !f.beat));
    let mut bad = config();
    bad.window = 1000;
    assert!(
        analyze_audio(
            AssetId::new(),
            source(),
            bad,
            0,
            &AudioBuffer::new(vec![]).unwrap()
        )
        .is_err()
    );
    let mut corrupt = pulse;
    corrupt.frames[0].time = Time::new(1, 48000).unwrap();
    assert!(corrupt.validate().is_err());
}

#[test]
fn feature_count_and_stereo_band_work_are_bounded_before_analysis() {
    let mut tiny_hop = config();
    tiny_hop.hop = 1;
    tiny_hop.window = 32;
    let buffer = AudioBuffer::new(vec![[0.0; 2]; 65537]).unwrap();
    assert!(matches!(
        analyze_audio(AssetId::new(), source(), tiny_hop, 0, &buffer),
        Err(kronello_audio::AudioError::BudgetExceeded(_))
    ));
    let mut many_bands = config();
    many_bands.window = 8192;
    many_bands.hop = 8192;
    many_bands.bands = vec![[1, 100]; 32];
    assert_eq!(
        many_bands.work_for_samples(8192),
        Some(2 * 8192 * (13 + 32 + 1))
    );
    let mut invalid = many_bands;
    invalid.hop = 0;
    assert_eq!(invalid.work_for_samples(8192), None);
}
