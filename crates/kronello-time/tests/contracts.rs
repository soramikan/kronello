use kronello_time::{
    Duration, FrameRate, Rational, SampleRate, Time, TimeError, TimeMap, TimeMapPoint, TimeRange,
};

fn r(num: i64, den: i64) -> Rational {
    Rational::new(num, den).unwrap()
}
fn t(seconds: i64) -> Time {
    Time::from_integer(seconds)
}
fn point(parent: i64, local: i64) -> TimeMapPoint {
    TimeMapPoint {
        parent: t(parent),
        local: t(local),
    }
}

#[test]
fn adr0043_normalization_and_negative_time() {
    assert_eq!(r(2, 4), r(1, 2));
    assert_eq!(r(1, -2), r(-1, 2));
    assert_eq!(r(-2, -4), r(1, 2));
    for den in [1, -1, 12, i64::MIN, i64::MAX] {
        assert_eq!(r(0, den), Time::ZERO);
    }
    assert!(r(-1, 2) < Time::ZERO);
    assert_eq!(r(-1, 2).floor(), -1);
    assert_eq!(r(-4, 2).floor(), -2);
}

#[test]
fn adr0043_zero_denominator_is_typed_error() {
    for num in [0, 1, i64::MIN] {
        assert_eq!(Rational::new(num, 0), Err(TimeError::ZeroDenominator));
    }
    assert_eq!(t(1).checked_div(t(0)), Err(TimeError::DivisionByZero));
}

#[test]
fn adr0043_overflow_is_typed_error() {
    assert_eq!(Rational::new(1, i64::MIN), Err(TimeError::Overflow));
    assert_eq!(Rational::new(i64::MIN, -1), Err(TimeError::Overflow));
    assert_eq!(t(i64::MAX).checked_add(t(1)), Err(TimeError::Overflow));
    assert_eq!(t(i64::MIN).checked_sub(t(1)), Err(TimeError::Overflow));
    assert_eq!(t(i64::MAX).checked_mul(t(2)), Err(TimeError::Overflow));
    assert_eq!(t(i64::MIN).checked_div(t(-1)), Err(TimeError::Overflow));
    assert_eq!(t(i64::MIN).checked_neg(), Err(TimeError::Overflow));
    assert_eq!(
        r(1, i64::MAX).checked_mul(r(1, 2)),
        Err(TimeError::Overflow)
    );
}

#[test]
fn wide_intermediates_reduce_before_i64_conversion() {
    assert_eq!(r(i64::MIN, i64::MIN), Rational::ONE);
    assert_eq!(r(2, i64::MIN), r(-1, 1_i64 << 62));
    assert_eq!(
        r(i64::MAX, 2).checked_add(r(i64::MAX, 2)).unwrap(),
        t(i64::MAX)
    );
    assert_eq!(
        r(i64::MAX, 2).checked_mul(r(2, i64::MAX)).unwrap(),
        Rational::ONE
    );
    assert_eq!(
        r(i64::MIN, 3).checked_sub(r(i64::MIN, 3)).unwrap(),
        Rational::ZERO
    );
    assert_eq!(
        r(i64::MIN, 3).checked_div(r(i64::MIN, 3)).unwrap(),
        Rational::ONE
    );
    assert!(r(i64::MIN, i64::MAX) < r(-1, 1));
    assert!(r(i64::MAX, i64::MAX - 1) > Rational::ONE);
}

#[test]
fn adr0043_duration_is_nonnegative() {
    assert_eq!(Duration::new(t(-1)), Err(TimeError::NegativeDuration));
    assert_eq!(Duration::new(t(0)).unwrap(), Duration::ZERO);
    let one = Duration::new(t(1)).unwrap();
    assert_eq!(one.checked_add(one).unwrap().as_time(), t(2));
    assert_eq!(
        Duration::ZERO.checked_sub(one),
        Err(TimeError::NegativeDuration)
    );
    assert_eq!(
        Duration::new(t(i64::MAX)).unwrap().checked_add(one),
        Err(TimeError::Overflow)
    );
}

#[test]
fn adr0043_half_open_endpoints_and_duration() {
    let range = TimeRange::new(t(-1), t(2)).unwrap();
    assert!(range.contains(t(-1)));
    assert!(range.contains(r(3, 2)));
    assert!(!range.contains(t(2)));
    assert!(!range.contains(t(-2)));
    assert_eq!(range.duration().unwrap().as_time(), t(3));
    assert_eq!(
        TimeRange::from_start_duration(t(-1), range.duration().unwrap()).unwrap(),
        range
    );
    assert_eq!(TimeRange::new(t(2), t(-1)), Err(TimeError::ReversedRange));
    let empty = TimeRange::new(t(2), t(2)).unwrap();
    assert!(empty.is_empty());
    assert!(!empty.contains(t(2)));
    assert_eq!(empty.duration().unwrap(), Duration::ZERO);
    assert_eq!(range.intersection(empty), None);
}

#[test]
fn intervals_intersect_or_are_adjacent_without_overlap() {
    let left = TimeRange::new(t(0), t(1)).unwrap();
    let right = TimeRange::new(t(1), t(2)).unwrap();
    assert!(left.is_adjacent_to(right));
    assert!(right.is_adjacent_to(left));
    assert_eq!(left.intersection(right), None);
    let overlap = TimeRange::new(r(1, 2), r(3, 2)).unwrap();
    assert_eq!(
        left.intersection(overlap),
        Some(TimeRange::new(r(1, 2), t(1)).unwrap())
    );
    assert!(!left.is_adjacent_to(overlap));
    assert!(!left.is_adjacent_to(TimeRange::new(t(1), t(1)).unwrap()));
    assert_eq!(left.intersection(TimeRange::new(t(3), t(4)).unwrap()), None);
}

#[test]
fn interval_arithmetic_overflow_is_reported() {
    assert_eq!(
        TimeRange::new(t(i64::MIN), t(i64::MAX)).unwrap().duration(),
        Err(TimeError::Overflow)
    );
    assert_eq!(
        TimeRange::from_start_duration(t(i64::MAX), Duration::new(t(1)).unwrap()),
        Err(TimeError::Overflow)
    );
}

#[test]
fn json_uses_decimal_strings_without_integer_precision_loss() {
    let time = t(9_007_199_254_740_993);
    let json = serde_json::to_string(&time).unwrap();
    assert_eq!(json, r#"{"num":"9007199254740993","den":"1"}"#);
    assert_eq!(serde_json::from_str::<Time>(&json).unwrap(), time);
    for time in [t(i64::MIN), t(i64::MAX), r(1, i64::MAX)] {
        assert_eq!(
            serde_json::from_str::<Time>(&serde_json::to_string(&time).unwrap()).unwrap(),
            time
        );
    }
}

#[test]
fn json_normalizes_rational_inputs() {
    for (json, expected) in [
        (r#"{"num":"2","den":"4"}"#, r(1, 2)),
        (r#"{"num":"1","den":"-2"}"#, r(-1, 2)),
        (r#"{"num":"0","den":"-5"}"#, t(0)),
        (r#"{"num":"-0","den":"2"}"#, t(0)),
    ] {
        assert_eq!(serde_json::from_str::<Time>(json).unwrap(), expected);
    }
}

#[test]
fn json_rejects_numbers_and_invalid_rationals() {
    for json in [
        r#"{"num":1,"den":"2"}"#,
        r#"{"num":"1","den":2}"#,
        r#"{"num":1.0,"den":2.0}"#,
        r#"{"num":"1","den":"0"}"#,
        r#"{"num":"9223372036854775808","den":"1"}"#,
        r#"{"num":"1","den":"-9223372036854775808"}"#,
        r#"{"num":"1e3","den":"1"}"#,
        r#"{"num":" 1","den":"1"}"#,
        r#"{"num":"+1","den":"1"}"#,
        r#"{"num":"1.0","den":"1"}"#,
        r#"{"num":"","den":"1"}"#,
        r#"{"num":"1"}"#,
        r#"{"num":"1","num":"2","den":"1"}"#,
        r#"{"num":"1","den":"2","extra":0}"#,
    ] {
        assert!(
            serde_json::from_str::<Time>(json).is_err(),
            "accepted {json}"
        );
    }
}

#[test]
fn json_cannot_bypass_duration_range_or_rate_validation() {
    assert!(serde_json::from_str::<Duration>(r#"{"num":"-1","den":"1"}"#).is_err());
    assert!(
        serde_json::from_str::<TimeRange>(
            r#"{"start":{"num":"2","den":"1"},"end":{"num":"1","den":"1"}}"#
        )
        .is_err()
    );
    assert!(serde_json::from_str::<FrameRate>(r#"{"num":"0","den":"1"}"#).is_err());
    assert!(serde_json::from_str::<SampleRate>("0").is_err());
    let range = TimeRange::new(t(-1), t(4)).unwrap();
    assert_eq!(
        serde_json::from_str::<TimeRange>(&serde_json::to_string(&range).unwrap()).unwrap(),
        range
    );
    let duration = Duration::new(r(1, 2)).unwrap();
    assert_eq!(
        serde_json::from_str::<Duration>(&serde_json::to_string(&duration).unwrap()).unwrap(),
        duration
    );
    let rate = FrameRate::new(30_000, 1001).unwrap();
    assert_eq!(
        serde_json::from_str::<FrameRate>(&serde_json::to_string(&rate).unwrap()).unwrap(),
        rate
    );
    assert_eq!(
        serde_json::from_str::<SampleRate>("48000").unwrap(),
        SampleRate::HZ_48000
    );
}

#[test]
fn frame_conversions_preserve_subframe_and_negative_times() {
    let rate = FrameRate::new(30_000, 1001).unwrap();
    assert_eq!(rate.frame_to_time(1).unwrap(), r(1001, 30_000));
    assert_eq!(rate.frame_to_time(30_000).unwrap(), t(1001));
    assert_eq!(rate.time_to_frame(r(1001, 60_000)).unwrap(), r(1, 2));
    assert_eq!(rate.frame_floor(r(1001, 60_000)).unwrap(), 0);
    assert_eq!(rate.frame_floor(r(-1001, 60_000)).unwrap(), -1);
    assert_eq!(
        rate.frame_floor(rate.frame_to_time(-2).unwrap()).unwrap(),
        -2
    );
    assert_eq!(FrameRate::new(0, 1), Err(TimeError::InvalidRate));
    assert_eq!(FrameRate::new(-24, 1), Err(TimeError::InvalidRate));
    assert_eq!(FrameRate::new(24, 0), Err(TimeError::ZeroDenominator));
    assert_eq!(
        FrameRate::new(-24, -1).unwrap(),
        FrameRate::new(24, 1).unwrap()
    );
}

#[test]
fn frame_and_sample_overflow_are_reported() {
    let rate = FrameRate::new(24, 1).unwrap();
    assert_eq!(rate.frame_range(i64::MAX), Err(TimeError::Overflow));
    assert_eq!(rate.frame_floor(t(i64::MAX)), Err(TimeError::Overflow));
    assert_eq!(rate.time_to_frame(t(i64::MAX)), Err(TimeError::Overflow));
    assert_eq!(
        FrameRate::new(1, i64::MAX).unwrap().frame_to_time(2),
        Err(TimeError::Overflow)
    );
    assert_eq!(
        SampleRate::HZ_48000.sample_floor(t(i64::MAX)),
        Err(TimeError::Overflow)
    );
    // Floor need not narrow an intermediate rational denominator to i64.
    assert_eq!(
        FrameRate::new(1, i64::MAX)
            .unwrap()
            .frame_floor(r(1, i64::MAX))
            .unwrap(),
        0
    );
}

#[test]
fn audio_boundaries_use_absolute_floor_without_drift() {
    let rate = FrameRate::new(30_000, 1001).unwrap();
    let audio = SampleRate::HZ_48000;
    let expected = [0..1601, 1601..3203, 3203..4804, 4804..6406, 6406..8008];
    for (frame, range) in expected.into_iter().enumerate() {
        assert_eq!(audio.samples_for_frame(rate, frame as i64).unwrap(), range);
    }
    assert_eq!(audio.samples_for_frame(rate, -1).unwrap(), -1602..0);
    assert_eq!(audio.sample_to_time(-1).unwrap(), r(-1, 48_000));
    assert_eq!(audio.sample_floor(r(-1, 96_000)).unwrap(), -1);
    assert_eq!(SampleRate::new(0), Err(TimeError::InvalidRate));
    assert_eq!(
        audio
            .samples_for_range(TimeRange::new(r(1, 3), r(1, 3)).unwrap())
            .unwrap(),
        16_000..16_000
    );
}

#[test]
fn long_duration_frame_audio_boundaries_remain_exact() {
    let rate = FrameRate::new(30_000, 1001).unwrap();
    let frame = 30_000 * 60 * 24 * 365 * 10;
    let range = SampleRate::HZ_48000.samples_for_frame(rate, frame).unwrap();
    assert_eq!(range.start, (frame / 30_000) * 1001 * 48_000);
    assert_eq!(range.end - range.start, 1601);
}

#[test]
fn linear_time_map_is_exact_and_reports_overflow() {
    let map = TimeMap::linear(t(-2), r(3, 2)).unwrap();
    assert_eq!(map.map(r(1, 3)).unwrap(), r(-3, 2));
    assert_eq!(map.map(t(-2)).unwrap(), t(-5));
    assert_eq!(
        TimeMap::linear(t(0), t(0)),
        Err(TimeError::UnsupportedMapSlope)
    );
    assert_eq!(
        TimeMap::linear(t(0), t(-1)),
        Err(TimeError::UnsupportedMapSlope)
    );
    assert_eq!(
        TimeMap::linear(t(1), t(1)).unwrap().map(t(i64::MAX)),
        Err(TimeError::Overflow)
    );
}

#[test]
fn piecewise_map_interpolates_exactly_at_and_between_knots() {
    let map = TimeMap::piecewise_linear(vec![point(-2, -1), point(0, 3), point(3, 4)]).unwrap();
    for (parent, local) in [(-2, -1), (0, 3), (3, 4)] {
        assert_eq!(map.map(t(parent)).unwrap(), t(local));
    }
    assert_eq!(map.map(r(-1, 2)).unwrap(), t(2));
    assert_eq!(map.map(t(1)).unwrap(), r(10, 3));
    assert_eq!(map.map(t(-3)), Err(TimeError::OutsideMapDomain));
    assert_eq!(map.map(r(7, 2)), Err(TimeError::OutsideMapDomain));
    // Endpoint lookup does not require an overflowing subtraction.
    let wide =
        TimeMap::piecewise_linear(vec![point(i64::MIN, i64::MIN), point(i64::MAX, i64::MAX)])
            .unwrap();
    assert_eq!(wide.map(t(i64::MIN)).unwrap(), t(i64::MIN));
    assert_eq!(wide.map(t(0)), Err(TimeError::Overflow));
}

#[test]
fn piecewise_map_rejects_invalid_order_and_unsupported_slopes() {
    for points in [vec![], vec![point(0, 0)]] {
        assert_eq!(
            TimeMap::piecewise_linear(points),
            Err(TimeError::TooFewMapPoints)
        );
    }
    for points in [
        vec![point(0, 0), point(0, 1)],
        vec![point(1, 0), point(0, 1)],
    ] {
        assert_eq!(
            TimeMap::piecewise_linear(points),
            Err(TimeError::UnorderedMapPoints)
        );
    }
    for points in [
        vec![point(0, 1), point(1, 0)],
        vec![point(0, 2), point(1, 2), point(2, 1)],
    ] {
        assert_eq!(
            TimeMap::piecewise_linear(points),
            Err(TimeError::UnsupportedMapSlope)
        );
    }
}

#[test]
fn piecewise_hold_segments_pin_local_and_invert_deterministically() {
    // Ramp, then a hold, then another ramp: local plateaus are legal.
    let map = TimeMap::piecewise_linear(vec![point(0, 0), point(2, 1), point(4, 1), point(6, 3)])
        .unwrap();
    // map() returns the pinned local value across the whole hold, endpoints
    // included.
    assert_eq!(map.map(t(2)).unwrap(), t(1));
    assert_eq!(map.map(t(3)).unwrap(), t(1));
    assert_eq!(map.map(t(4)).unwrap(), t(1));
    assert_eq!(map.map(t(5)).unwrap(), t(2));
    // The map is non-injective across a hold, so the inverse resolves to the
    // hold segment's starting parent: the earliest parent mapping to the
    // local value.
    assert_eq!(map.inverse_canonical(t(1)).unwrap(), t(2));
    assert_eq!(map.inverse_canonical(t(2)).unwrap(), t(5));
    // Consecutive hold segments still resolve to the first hold's start.
    let chained = TimeMap::piecewise_linear(vec![
        point(0, 0),
        point(1, 1),
        point(2, 1),
        point(3, 1),
        point(4, 2),
    ])
    .unwrap();
    assert_eq!(chained.inverse_canonical(t(1)).unwrap(), t(1));
    let map = match map {
        TimeMap::PiecewiseLinear(map) => map,
        _ => unreachable!(),
    };
    assert!(!map.is_hold(t(0)));
    assert!(!map.is_hold(t(1)));
    assert!(map.is_hold(t(2)));
    assert!(map.is_hold(t(3)));
    // The final control point owns the last segment; it is not a hold.
    assert!(!map.is_hold(t(4)));
    assert!(!map.is_hold(t(6)));
    assert_eq!(map.slope_at(t(3)).unwrap(), Rational::ZERO);
    assert_eq!(map.slope_at(t(5)).unwrap(), Rational::ONE);
    assert_eq!(map.slope_at(t(1)).unwrap(), r(1, 2));
}

#[test]
fn map_json_roundtrips_and_preserves_validation() {
    for map in [
        TimeMap::linear(r(1, 2), r(3, 2)).unwrap(),
        TimeMap::piecewise_linear(vec![point(0, 1), point(1, 3)]).unwrap(),
    ] {
        assert_eq!(
            serde_json::from_str::<TimeMap>(&serde_json::to_string(&map).unwrap()).unwrap(),
            map
        );
    }
    assert!(
        serde_json::from_str::<TimeMap>(
            r#"{"kind":"linear","offset":{"num":"0","den":"1"},"speed":{"num":"0","den":"1"}}"#
        )
        .is_err()
    );
    assert!(serde_json::from_str::<TimeMap>(r#"{"kind":"piecewise_linear","points":[]}"#).is_err());
    assert!(serde_json::from_str::<TimeMap>(r#"{"kind":"nonlinear"}"#).is_err());
}

#[test]
fn track003_frame_interpolation_wire_and_validation() {
    use kronello_time::{FlowFallbackPolicy, FrameInterpolation, OpticalFlowConfig};
    let config = OpticalFlowConfig {
        block_radius: 2,
        search_radius: 4,
        levels: 2,
        confidence_floor: r(1, 4),
        max_low_confidence: r(1, 2),
        flow_fallback: Some(FlowFallbackPolicy::Blend),
    };
    config.validate().unwrap();
    let interpolation = FrameInterpolation::OpticalFlow(config);
    let map = TimeMap::piecewise_linear_with_interpolation(
        vec![point(0, 0), point(2, 1)],
        Some(interpolation),
    )
    .unwrap();
    assert_eq!(map.interpolation(), Some(interpolation));
    // The authored mode rides the map identity through the wire form.
    let wire = serde_json::to_string(&map).unwrap();
    let back: TimeMap = serde_json::from_str(&wire).unwrap();
    assert_eq!(back, map);
    // Linear/protected maps carry no authored mode.
    assert_eq!(TimeMap::linear(t(0), t(1)).unwrap().interpolation(), None);
    // Out-of-range fields fail construction and direct validation with the
    // typed error.
    for bad in [
        OpticalFlowConfig {
            block_radius: 0,
            ..config
        },
        OpticalFlowConfig {
            search_radius: 65,
            ..config
        },
        OpticalFlowConfig {
            levels: 7,
            ..config
        },
        OpticalFlowConfig {
            confidence_floor: r(3, 2),
            ..config
        },
        OpticalFlowConfig {
            max_low_confidence: r(-1, 2),
            ..config
        },
    ] {
        assert_eq!(bad.validate(), Err(TimeError::InvalidFrameInterpolation));
        assert_eq!(
            TimeMap::piecewise_linear_with_interpolation(
                vec![point(0, 0), point(2, 1)],
                Some(FrameInterpolation::OpticalFlow(bad)),
            ),
            Err(TimeError::InvalidFrameInterpolation)
        );
    }
    // The wire form round-trips through validation on deserialization.
    let bad_wire = serde_json::json!({
        "kind": "piecewise_linear",
        "points": [
            {"parent": {"num": "0", "den": "1"}, "local": {"num": "0", "den": "1"}},
            {"parent": {"num": "2", "den": "1"}, "local": {"num": "1", "den": "1"}}
        ],
        "interpolation": {
            "mode": "optical_flow",
            "block_radius": 0,
            "search_radius": 4,
            "levels": 2,
            "confidence_floor": {"num": "1", "den": "4"},
            "max_low_confidence": {"num": "1", "den": "2"},
            "flow_fallback": null
        }
    });
    assert!(serde_json::from_value::<TimeMap>(bad_wire).is_err());
}
