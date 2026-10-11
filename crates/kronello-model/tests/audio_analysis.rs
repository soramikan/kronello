use kronello_model::*;
use kronello_time::{Rational, Time, TimeMap};

fn config() -> AudioAnalysisConfig {
    AudioAnalysisConfig {
        version: 2,
        sample_rate: 48000,
        window: 2048,
        hop: 1600,
        bands: vec![[0, 1000]; 8],
        time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
    }
}

#[test]
fn config_diagnostics_identify_each_field_and_allowed_values() {
    let cases = [
        ("version", 3, "version", "1..=2"),
        ("sample_rate", 44100, "sample_rate", "48000"),
        ("window", 16, "window", "32..=8192"),
        ("window", 16384, "window", "32..=8192"),
        ("window", 1000, "window", "power of two"),
        ("hop", 0, "hop", "1..=28800000"),
        ("hop", 28800001, "hop", "1..=28800000"),
    ];
    for (field, actual, kind, allowed) in cases {
        let mut value = serde_json::to_value(config()).unwrap();
        value[field] = actual.into();
        let c: AudioAnalysisConfig = serde_json::from_value(value).unwrap();
        let error = c.validate().unwrap_err();
        assert_eq!(serde_json::to_value(&error).unwrap()["kind"], kind);
        assert!(error.to_string().contains(allowed), "{error}");
        assert!(error.to_string().contains(&actual.to_string()), "{error}");
    }
    let mut c = config();
    c.bands = vec![[0, 1000]; 33];
    assert_eq!(c.validate(), Err(AudioAnalysisError::Bands { actual: 33 }));
    for (low, high) in [(100, 100), (101, 100), (0, 24001)] {
        c.bands = vec![[0, 1], [low, high]];
        let error = c.validate().unwrap_err();
        assert_eq!(
            error,
            AudioAnalysisError::Band {
                index: 1,
                low,
                high
            }
        );
        assert!(error.to_string().contains("0 <= low < high <= 24000"));
    }
    c.bands.clear();
    c.validate().unwrap();
    c.version = 1;
    c.hop = 2049;
    assert_eq!(
        c.validate(),
        Err(AudioAnalysisError::Hop {
            actual: 2049,
            allowed: 2048
        })
    );
}

#[test]
fn ten_minute_budget_is_versioned_and_all_limits_report_required_and_allowed() {
    let mut c = config();
    assert_eq!(c.work_for_samples(28_800_000), Some(1_474_560_000));
    c.validate_samples(28_800_000).unwrap();
    for (samples, resource, required, allowed) in [
        (28_800_001, "sample_count", 28_800_001, 28_800_000),
        (usize::MAX, "sample_count", usize::MAX, 28_800_000),
    ] {
        assert_eq!(
            c.validate_samples(samples),
            Err(AudioAnalysisError::Budget {
                resource: resource.into(),
                required,
                allowed,
            })
        );
    }
    c.hop = 1;
    assert_eq!(
        c.validate_samples(65537),
        Err(AudioAnalysisError::Budget {
            resource: "windows".into(),
            required: 65537,
            allowed: 65536,
        })
    );
    c.hop = 1600;
    c.version = 1;
    assert_eq!(
        c.validate_samples(28_800_000),
        Err(AudioAnalysisError::Budget {
            resource: "work".into(),
            required: 1_474_560_000,
            allowed: 100_000_000,
        })
    );
    c.version = 2;
    c.bands = vec![[0, 1000]; 32];
    let error = c.validate_samples(28_800_000).unwrap_err();
    assert_eq!(
        error,
        AudioAnalysisError::Budget {
            resource: "work".into(),
            required: 3_244_032_000,
            allowed: 1_500_000_000,
        }
    );
    assert!(
        error
            .to_string()
            .contains("required 3244032000, allowed 1500000000")
    );
}
