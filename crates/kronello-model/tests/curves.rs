use kronello_model::*;
use kronello_time::Time;
use serde_json::json;

fn scalar(v: f64) -> Value {
    Value::Scalar(FiniteF64::new(v).unwrap())
}
fn key(num: i64, den: i64, v: f64) -> Keyframe {
    Keyframe {
        time: Time::new(num, den).unwrap(),
        value: scalar(v),
        interpolation: CurveInterpolation::Linear,
    }
}
fn curve() -> AnimationCurve {
    AnimationCurve::new(
        CurveId::new(),
        ValueType::Scalar,
        vec![key(0, 1, 0.0), key(1, 2, 10.0)],
    )
    .unwrap()
}

#[test]
fn explicit_insert_upsert_replace_preserve_sorted_unique_rational_times() {
    let mut c = curve();
    let before = c.clone();
    assert_eq!(
        c.insert_key(key(2, 4, 20.0)),
        Err(CurveError::DuplicateKeyTime {
            time: Time::new(1, 2).unwrap()
        })
    );
    assert_eq!(c, before);
    assert_eq!(
        c.upsert_key(key(2, 4, 20.0)).unwrap(),
        Some(key(1, 2, 10.0))
    );
    assert_eq!(c.replace_key(key(1, 2, 30.0)).unwrap(), key(1, 2, 20.0));
    let before = c.clone();
    assert!(matches!(
        c.replace_key(key(3, 4, 1.0)),
        Err(CurveError::KeyNotFound { .. })
    ));
    assert_eq!(c, before);
    assert_eq!(c.upsert_key(key(-1, 1, -10.0)).unwrap(), None);
    c.insert_key(key(1, 4, 5.0)).unwrap();
    assert_eq!(
        c.keys().iter().map(|k| k.time).collect::<Vec<_>>(),
        vec![
            Time::from_integer(-1),
            Time::ZERO,
            Time::new(1, 4).unwrap(),
            Time::new(1, 2).unwrap()
        ]
    );
}

#[test]
fn type_errors_leave_all_key_operations_atomic() {
    let mut c = curve();
    let before = c.clone();
    let invalid = Keyframe {
        value: Value::Bool(true),
        ..key(1, 2, 0.0)
    };
    let expected = Err(CurveError::Value(ModelError::ValueTypeMismatch {
        expected: ValueType::Scalar,
        actual: ValueType::Bool,
    }));
    assert_eq!(c.insert_key(invalid.clone()), expected);
    assert!(matches!(
        c.upsert_key(invalid.clone()),
        Err(CurveError::Value(ModelError::ValueTypeMismatch { .. }))
    ));
    assert!(matches!(
        c.replace_key(invalid),
        Err(CurveError::Value(ModelError::ValueTypeMismatch { .. }))
    ));
    assert_eq!(c, before);
}

#[test]
fn construction_and_import_reject_duplicate_unordered_or_mismatched_keys() {
    let id = CurveId::new();
    assert!(matches!(
        AnimationCurve::new(id, ValueType::Scalar, vec![key(1, 2, 1.0), key(2, 4, 2.0)]),
        Err(CurveError::DuplicateKeyTime { .. })
    ));
    assert_eq!(
        AnimationCurve::new(id, ValueType::Scalar, vec![key(1, 1, 1.0), key(0, 1, 0.0)]),
        Err(CurveError::UnorderedKeys)
    );
    assert!(matches!(
        AnimationCurve::new(id, ValueType::Vec2, vec![key(0, 1, 0.0)]),
        Err(CurveError::Value(ModelError::ValueTypeMismatch { .. }))
    ));
    let mut wire = serde_json::to_value(curve()).unwrap();
    wire["keys"][1]["time"] = json!({"num":"0", "den":"1"});
    assert!(serde_json::from_value::<AnimationCurve>(wire).is_err());
}

#[test]
fn handles_validate_monotonic_time_finite_values_and_allow_value_overshoot() {
    for (a, b) in [
        ([-0.1, 0.0], [0.5, 1.0]),
        ([0.8, 0.0], [0.2, 1.0]),
        ([0.5, 0.0], [1.1, 1.0]),
    ] {
        assert_eq!(TimeBezier::new(a, b), Err(CurveError::NonMonotonicHandles));
    }
    for (a, b) in [
        ([f64::NAN, 0.0], [0.5, 1.0]),
        ([0.5, f64::INFINITY], [0.5, 1.0]),
        ([0.0, 0.0], [f64::NEG_INFINITY, 1.0]),
    ] {
        assert!(matches!(
            TimeBezier::new(a, b),
            Err(CurveError::Value(ModelError::NonFinite { .. }))
        ));
    }
    assert!(TimeBezier::new([0.0, -3.0], [1.0, 4.0]).is_ok());
    assert!(TimeBezier::new([0.5, 0.0], [0.5, 1.0]).is_ok());
    assert!(
        serde_json::from_value::<TimeBezier>(json!({"control1":[0.8,0.0],"control2":[0.2,1.0]}))
            .is_err()
    );
}

#[test]
fn discrete_and_path_types_accept_only_hold_even_for_last_key() {
    let values = [
        Value::Bool(true),
        Value::Enum("mode".into()),
        Value::String("text".into()),
        Value::AssetRef(AssetId::new()),
        Value::Path(Path { segments: vec![] }),
    ];
    for value in values {
        let value_type = value.value_type();
        for interpolation in [
            CurveInterpolation::Linear,
            CurveInterpolation::Cubic(TimeBezier::new([0.0, 0.0], [1.0, 1.0]).unwrap()),
        ] {
            let key = Keyframe {
                time: Time::ZERO,
                value: value.clone(),
                interpolation,
            };
            assert_eq!(
                AnimationCurve::new(CurveId::new(), value_type, vec![key]),
                Err(CurveError::Value(ModelError::IncompatibleInterpolation {
                    value_type,
                    mode: interpolation.mode()
                }))
            );
        }
        AnimationCurve::new(
            CurveId::new(),
            value_type,
            vec![Keyframe {
                time: Time::ZERO,
                value,
                interpolation: CurveInterpolation::Hold,
            }],
        )
        .unwrap();
    }
}

#[test]
fn curve_json_round_trip_is_strict_and_keeps_rational_times_and_semantics() {
    let mut c = curve();
    let mut cubic_key = key(1, 3, 9.0);
    cubic_key.interpolation =
        CurveInterpolation::Cubic(TimeBezier::new([0.25, -0.5], [0.75, 1.5]).unwrap());
    c.insert_key(cubic_key).unwrap();
    let encoded = serde_json::to_string(&c).unwrap();
    assert_eq!(serde_json::from_str::<AnimationCurve>(&encoded).unwrap(), c);
    let wire = serde_json::to_value(&c).unwrap();
    assert_eq!(wire["interpolation_version"], 1);
    assert_eq!(wire["keys"][1]["time"], json!({"num":"1","den":"3"}));
    let mut unknown = wire.clone();
    unknown["future"] = json!(true);
    assert!(from_json::<AnimationCurve>(&unknown.to_string()).is_err());
    let mut numeric_time = wire.clone();
    numeric_time["keys"][1]["time"]["num"] = json!(1);
    assert!(from_json::<AnimationCurve>(&numeric_time.to_string()).is_err());
    let mut mode = wire.clone();
    mode["keys"][0]["interpolation"]["kind"] = json!("future");
    assert!(from_json::<AnimationCurve>(&mode.to_string()).is_err());
    let mut nonfinite = wire;
    nonfinite["keys"][0]["value"]["value"] = json!(null);
    assert!(from_json::<AnimationCurve>(&nonfinite.to_string()).is_err());
}

#[test]
fn unknown_meaning_versions_are_preserved_but_not_editable() {
    let mut wire = serde_json::to_value(curve()).unwrap();
    wire["interpolation_version"] = json!(99);
    let mut c: AnimationCurve = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&c).unwrap(), wire);
    let before = c.clone();
    assert_eq!(
        c.insert_key(key(1, 4, 2.0)),
        Err(CurveError::UnsupportedInterpolationVersion { version: 99 })
    );
    assert!(matches!(
        c.upsert_key(key(1, 4, 2.0)),
        Err(CurveError::UnsupportedInterpolationVersion { .. })
    ));
    assert!(matches!(
        c.replace_key(key(1, 2, 2.0)),
        Err(CurveError::UnsupportedInterpolationVersion { .. })
    ));
    assert_eq!(c, before);
}
