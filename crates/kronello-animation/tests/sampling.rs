use kronello_animation::{AnimationError, sample, sample_in_space};
use kronello_model::*;
use kronello_time::{Time, TimeError};

fn f(v: f64) -> FiniteF64 {
    FiniteF64::new(v).unwrap()
}
fn scalar(v: f64) -> Value {
    Value::Scalar(f(v))
}
fn curve(a: Value, b: Value, interpolation: CurveInterpolation) -> AnimationCurve {
    AnimationCurve::new(
        CurveId::new(),
        a.value_type(),
        vec![
            Keyframe {
                time: Time::ZERO,
                value: a,
                interpolation,
            },
            Keyframe {
                time: Time::ONE,
                value: b,
                interpolation: CurveInterpolation::Hold,
            },
        ],
    )
    .unwrap()
}
fn assert_close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-12, "{a} != {b}");
}
fn number(v: Value) -> f64 {
    match v {
        Value::Scalar(v) | Value::Angle(v) => v.get(),
        _ => panic!("expected number"),
    }
}
fn cubic(a: [f64; 2], b: [f64; 2]) -> CurveInterpolation {
    CurveInterpolation::Cubic(TimeBezier::new(a, b).unwrap())
}

#[test]
fn half_open_hold_and_exact_key_boundaries() {
    let mut c = curve(scalar(10.0), scalar(20.0), CurveInterpolation::Hold);
    c.insert_key(Keyframe {
        time: Time::from_integer(2),
        value: scalar(30.0),
        interpolation: CurveInterpolation::Linear,
    })
    .unwrap();
    for (time, expected) in [
        (Time::from_integer(-1), 10.0),
        (Time::ZERO, 10.0),
        (Time::new(999, 1000).unwrap(), 10.0),
        (Time::ONE, 20.0),
        (Time::new(1999, 1000).unwrap(), 20.0),
        (Time::from_integer(2), 30.0),
        (Time::from_integer(9), 30.0),
    ] {
        assert_eq!(sample(&c, time).unwrap(), scalar(expected));
    }
}

#[test]
fn linear_golden_subframes_and_continuous_two_turn_angle() {
    for (a, b) in [
        (scalar(0.0), scalar(720.0)),
        (Value::Angle(f(0.0)), Value::Angle(f(720.0))),
    ] {
        let c = curve(a, b, CurveInterpolation::Linear);
        for (n, d, expected) in [
            (-1, 2, 0.0),
            (0, 1, 0.0),
            (1, 4, 180.0),
            (1, 2, 360.0),
            (3, 4, 540.0),
            (1, 1, 720.0),
            (2, 1, 720.0),
        ] {
            assert_eq!(
                number(sample(&c, Time::new(n, d).unwrap()).unwrap()),
                expected
            );
        }
    }
}

#[test]
fn same_time_key_operations_have_explicit_sampling_results() {
    let mut c = curve(
        Value::Angle(f(0.0)),
        Value::Angle(f(720.0)),
        CurveInterpolation::Linear,
    );
    let mid = Time::new(1, 2).unwrap();
    assert_eq!(sample(&c, mid).unwrap(), Value::Angle(f(360.0)));
    let replacement = Keyframe {
        time: Time::new(2, 2).unwrap(),
        value: Value::Angle(f(1440.0)),
        interpolation: CurveInterpolation::Hold,
    };
    assert!(matches!(
        c.insert_key(replacement.clone()),
        Err(CurveError::DuplicateKeyTime { .. })
    ));
    assert_eq!(sample(&c, mid).unwrap(), Value::Angle(f(360.0)));
    assert!(c.upsert_key(replacement).unwrap().is_some());
    assert_eq!(sample(&c, mid).unwrap(), Value::Angle(f(720.0)));
    c.replace_key(Keyframe {
        time: Time::ZERO,
        value: Value::Angle(f(-720.0)),
        interpolation: CurveInterpolation::Hold,
    })
    .unwrap();
    assert_eq!(sample(&c, mid).unwrap(), Value::Angle(f(-720.0)));
    assert_eq!(sample(&c, Time::ONE).unwrap(), Value::Angle(f(1440.0)));
}

#[test]
fn cubic_solves_time_bezier_instead_of_using_time_as_parameter() {
    // x(t)=t^3, y(t)=3t^2-2t^3; x=1/8 implies t=1/2, y=1/2.
    let c = curve(scalar(0.0), scalar(100.0), cubic([0.0, 0.0], [0.0, 1.0]));
    assert_close(number(sample(&c, Time::new(1, 8).unwrap()).unwrap()), 50.0);
    // x(t)=t, y(t)=smoothstep(t).
    let c = curve(
        Value::Angle(f(0.0)),
        Value::Angle(f(720.0)),
        cubic([1.0 / 3.0, 0.0], [2.0 / 3.0, 1.0]),
    );
    for (n, d, expected) in [(1, 4, 112.5), (1, 2, 360.0), (3, 4, 607.5)] {
        assert_close(
            number(sample(&c, Time::new(n, d).unwrap()).unwrap()),
            expected,
        );
    }
    assert_eq!(sample(&c, Time::ONE).unwrap(), Value::Angle(f(720.0)));
}

#[test]
fn cubic_flat_endpoints_and_overshoot_are_explicit() {
    for handles in [cubic([0.0, 0.0], [0.0, 1.0]), cubic([1.0, 0.0], [1.0, 1.0])] {
        let c = curve(scalar(-1.0), scalar(1.0), handles);
        assert_eq!(sample(&c, Time::ZERO).unwrap(), scalar(-1.0));
        assert_eq!(sample(&c, Time::ONE).unwrap(), scalar(1.0));
        assert!(number(sample(&c, Time::new(1, 1000000).unwrap()).unwrap()).is_finite());
        assert!(number(sample(&c, Time::new(999999, 1000000).unwrap()).unwrap()).is_finite());
    }
    let c = curve(
        scalar(0.0),
        scalar(1.0),
        cubic([1.0 / 3.0, 2.0], [2.0 / 3.0, 2.0]),
    );
    assert_close(number(sample(&c, Time::new(1, 2).unwrap()).unwrap()), 1.625);
    let d = SchemaRegistry::with_builtin()
        .lookup(&SchemaKey::new("kronello.opacity").unwrap())
        .unwrap()
        .clone();
    assert!(matches!(
        d.validate_value(&sample(&c, Time::new(1, 2).unwrap()).unwrap()),
        Err(ModelError::OutOfRange { .. })
    ));
}

#[test]
fn vectors_support_hold_linear_and_cubic_componentwise() {
    for interpolation in [
        CurveInterpolation::Hold,
        CurveInterpolation::Linear,
        cubic([1.0 / 3.0, 0.0], [2.0 / 3.0, 1.0]),
    ] {
        let progress = if interpolation == CurveInterpolation::Hold {
            0.0
        } else if interpolation == CurveInterpolation::Linear {
            0.25
        } else {
            0.15625
        };
        let c = curve(
            Value::Vec2([f(-2.0), f(10.0)]),
            Value::Vec2([f(2.0), f(30.0)]),
            interpolation,
        );
        let Value::Vec2(result) = sample(&c, Time::new(1, 4).unwrap()).unwrap() else {
            panic!("vector")
        };
        assert_close(result[0].get(), -2.0 + 4.0 * progress);
        assert_close(result[1].get(), 10.0 + 20.0 * progress);
        let c = curve(
            Value::Vec3([f(-2.0), f(10.0), f(5.0)]),
            Value::Vec3([f(2.0), f(30.0), f(-5.0)]),
            interpolation,
        );
        let Value::Vec3(result) = sample(&c, Time::new(1, 4).unwrap()).unwrap() else {
            panic!("vector")
        };
        for (v, expected) in result.into_iter().zip([
            -2.0 + 4.0 * progress,
            10.0 + 20.0 * progress,
            5.0 - 10.0 * progress,
        ]) {
            assert_close(v.get(), expected);
        }
    }
}

#[test]
fn discrete_values_switch_only_at_key_times() {
    let pairs = [
        (Value::Bool(false), Value::Bool(true)),
        (Value::Enum("a".into()), Value::Enum("b".into())),
        (Value::String("a".into()), Value::String("b".into())),
        (
            Value::AssetRef(AssetId::new()),
            Value::AssetRef(AssetId::new()),
        ),
    ];
    for (a, b) in pairs {
        let c = curve(a.clone(), b.clone(), CurveInterpolation::Hold);
        assert_eq!(sample(&c, Time::new(999, 1000).unwrap()).unwrap(), a);
        assert_eq!(sample(&c, Time::ONE).unwrap(), b);
    }
}

fn color(rgb: [f64; 3], alpha: f64, space: ColorSpace) -> Value {
    Value::Color(Color::new(space, rgb, alpha).unwrap())
}
fn components(v: Value) -> [f64; 4] {
    let Value::Color(c) = v else { panic!("color") };
    let p = c.components();
    [p.r.get(), p.g.get(), p.b.get(), p.alpha.get()]
}

#[test]
fn color_interpolation_is_linear_straight_rgb_with_independent_alpha() {
    for interpolation in [
        CurveInterpolation::Hold,
        CurveInterpolation::Linear,
        cubic([1.0 / 3.0, 0.0], [2.0 / 3.0, 1.0]),
    ] {
        let c = curve(
            color([1.0, 0.0, 0.0], 0.0, ColorSpace::Srgb),
            color([0.0, 0.0, 1.0], 1.0, ColorSpace::Srgb),
            interpolation,
        );
        let progress = if interpolation == CurveInterpolation::Hold {
            0.0
        } else if interpolation == CurveInterpolation::Linear {
            0.25
        } else {
            0.15625
        };
        let result = sample(&c, Time::new(1, 4).unwrap()).unwrap();
        let Value::Color(v) = result else {
            panic!("color")
        };
        assert_eq!(v.space(), ColorSpace::LinearRec709);
        for (a, b) in
            components(Value::Color(v))
                .into_iter()
                .zip([1.0 - progress, 0.0, progress, progress])
        {
            assert_close(a, b);
        }
        assert_eq!(
            components(sample(&c, Time::ZERO).unwrap()),
            [1.0, 0.0, 0.0, 0.0]
        );
    }
    let c = curve(
        color([0.5, 0.04045, 0.0], 0.25, ColorSpace::Srgb),
        color([1.0, 1.0, 1.0], 0.75, ColorSpace::Srgb),
        CurveInterpolation::Linear,
    );
    let start = components(sample(&c, Time::ZERO).unwrap());
    assert_close(start[0], 0.21404114048223255);
    assert_close(start[1], 0.0031308049535603713);
    assert_eq!(start[3], 0.25);
    let middle = components(sample(&c, Time::new(1, 2).unwrap()).unwrap());
    assert_close(middle[0], 0.6070205702411163);
    assert_eq!(middle[3], 0.5);
}

#[test]
fn working_space_conversion_retains_negative_and_high_rgb_and_mixed_spaces() {
    let c = curve(
        color([1.0, 0.0, 0.0], 0.5, ColorSpace::LinearRec2020),
        color([0.0, 1.0, 0.0], 1.0, ColorSpace::LinearRec709),
        CurveInterpolation::Linear,
    );
    let start = components(sample(&c, Time::ZERO).unwrap());
    assert_eq!(start, [1.660491, -0.1245505, -0.0181508, 0.5]);
    let middle = components(
        sample_in_space(&c, Time::new(1, 2).unwrap(), ColorSpace::LinearRec2020).unwrap(),
    );
    for (a, b) in middle
        .into_iter()
        .zip([0.664641, 0.4597702, 0.04400665, 0.75])
    {
        assert_close(a, b);
    }
    assert_eq!(
        sample_in_space(&c, Time::ZERO, ColorSpace::Srgb),
        Err(AnimationError::InvalidWorkingColorSpace)
    );
}

#[test]
fn alpha_overshoot_is_an_error_without_clamping() {
    let c = curve(
        color([0.0; 3], 0.0, ColorSpace::LinearRec709),
        color([1.0; 3], 1.0, ColorSpace::LinearRec709),
        cubic([1.0 / 3.0, 2.0], [2.0 / 3.0, 2.0]),
    );
    assert!(matches!(
        sample(&c, Time::new(1, 2).unwrap()),
        Err(AnimationError::Value(ModelError::OutOfRange {
            component: 3,
            ..
        }))
    ));
}

#[test]
fn empty_single_key_unknown_version_and_time_overflow_are_typed() {
    let empty = AnimationCurve::new(CurveId::new(), ValueType::Scalar, vec![]).unwrap();
    assert_eq!(
        sample(&empty, Time::ZERO),
        Err(AnimationError::EmptyCurve { id: empty.id() })
    );
    let single = AnimationCurve::new(
        CurveId::new(),
        ValueType::Scalar,
        vec![Keyframe {
            time: Time::ONE,
            value: scalar(7.0),
            interpolation: CurveInterpolation::Linear,
        }],
    )
    .unwrap();
    for time in [
        Time::from_integer(i64::MIN),
        Time::ONE,
        Time::from_integer(i64::MAX),
    ] {
        assert_eq!(sample(&single, time).unwrap(), scalar(7.0));
    }
    let mut definition = CurveDefinition::from(single);
    definition.interpolation_version = 99;
    let unknown = AnimationCurve::try_from(definition).unwrap();
    assert_eq!(
        sample(&unknown, Time::ONE),
        Err(AnimationError::Curve(
            CurveError::UnsupportedInterpolationVersion { version: 99 }
        ))
    );
    let huge = AnimationCurve::new(
        CurveId::new(),
        ValueType::Scalar,
        vec![
            Keyframe {
                time: Time::from_integer(i64::MIN),
                value: scalar(0.0),
                interpolation: CurveInterpolation::Linear,
            },
            Keyframe {
                time: Time::from_integer(i64::MAX),
                value: scalar(1.0),
                interpolation: CurveInterpolation::Hold,
            },
        ],
    )
    .unwrap();
    assert_eq!(
        sample(&huge, Time::ZERO),
        Err(AnimationError::Time(TimeError::Overflow))
    );
}

#[test]
fn large_absolute_times_do_not_collapse_close_rational_keys() {
    let base = Time::from_integer(1_i64 << 52);
    let quarter = base.checked_add(Time::new(1, 4).unwrap()).unwrap();
    let end = base.checked_add(Time::new(1, 2).unwrap()).unwrap();
    let c = AnimationCurve::new(
        CurveId::new(),
        ValueType::Scalar,
        vec![
            Keyframe {
                time: base,
                value: scalar(0.0),
                interpolation: CurveInterpolation::Linear,
            },
            Keyframe {
                time: end,
                value: scalar(100.0),
                interpolation: CurveInterpolation::Hold,
            },
        ],
    )
    .unwrap();
    assert_eq!(sample(&c, quarter).unwrap(), scalar(50.0));
}

#[test]
fn nonfinite_interpolation_is_rejected_but_opposite_extremes_blend_safely() {
    let c = curve(
        scalar(-f64::MAX),
        scalar(f64::MAX),
        CurveInterpolation::Linear,
    );
    assert_eq!(sample(&c, Time::new(1, 2).unwrap()).unwrap(), scalar(0.0));
    let c = curve(
        scalar(0.0),
        scalar(f64::MAX),
        cubic([1.0 / 3.0, 2.0], [2.0 / 3.0, 2.0]),
    );
    assert!(matches!(
        sample(&c, Time::new(1, 2).unwrap()),
        Err(AnimationError::Value(ModelError::NonFinite { .. }))
    ));
}

#[test]
fn generated_curve_property_evaluation_order_and_json_roundtrip_do_not_change_results() {
    // Reproducible generated cases cover varying signed values, rational key
    // times, handles, and sampling permutations without random runtime inputs.
    let mut state = 0x123456789abcdef0_u64;
    let mut next = || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        state
    };
    for case in 0..128 {
        let interpolation = match case % 3 {
            0 => CurveInterpolation::Hold,
            1 => CurveInterpolation::Linear,
            _ => {
                let x1 = (next() % 500) as f64 / 1000.0;
                let x2 = 0.5 + (next() % 501) as f64 / 1000.0;
                cubic(
                    [x1, (next() % 200) as f64 / 100.0 - 0.5],
                    [x2, (next() % 200) as f64 / 100.0 - 0.5],
                )
            }
        };
        let keys = (0..5)
            .map(|n| Keyframe {
                time: Time::new(n * 7 - 14, 3).unwrap(),
                value: scalar((next() % 20000) as f64 / 10.0 - 1000.0),
                interpolation,
            })
            .collect();
        let c = AnimationCurve::new(CurveId::new(), ValueType::Scalar, keys).unwrap();
        let times: Vec<_> = (-40..=40).map(|n| Time::new(n, 7).unwrap()).collect();
        let expected: Vec<_> = times.iter().map(|t| sample(&c, *t).unwrap()).collect();
        for index in (0..times.len()).rev() {
            assert_eq!(sample(&c, times[index]).unwrap(), expected[index]);
        }
        let mut indices: Vec<_> = (0..times.len()).collect();
        for n in (1..indices.len()).rev() {
            indices.swap(n, (next() % (n as u64 + 1)) as usize);
        }
        let decoded: AnimationCurve =
            serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        for index in indices {
            assert_eq!(sample(&decoded, times[index]).unwrap(), expected[index]);
        }
    }
}
