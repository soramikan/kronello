//! COLOR-002 versioned color-correction effect contracts (ADR-0108).
use kronello_model::*;
use std::collections::BTreeMap;

fn registry() -> SchemaRegistry {
    let mut r = SchemaRegistry::with_builtin();
    for d in effect_descriptors() {
        r.register(d).unwrap();
    }
    r
}
fn prop(r: &SchemaRegistry, key: &str, value: Value) -> Property {
    let d = r.lookup(&SchemaKey::new(key).unwrap()).unwrap();
    Property::new(
        PropertyId::new(),
        DescriptorRef::new(d),
        PropertySource::Constant(value),
        vec![],
        r,
    )
    .unwrap()
}
fn scalar(v: f64) -> Value {
    Value::Scalar(FiniteF64::new(v).unwrap())
}
fn angle(v: f64) -> Value {
    Value::Angle(FiniteF64::new(v).unwrap())
}
fn curve_table(points: &[(f64, f64)]) -> Value {
    let columns = BTreeMap::from([
        ("x".to_string(), ValueType::Scalar),
        ("y".to_string(), ValueType::Scalar),
    ]);
    let rows = points
        .iter()
        .map(|&(x, y)| BTreeMap::from([("x".to_string(), scalar(x)), ("y".to_string(), scalar(y))]))
        .collect();
    Value::DataTable(DataTable { columns, rows })
}
/// Build properties from `(key, default)` pairs and pair them with an
/// `EffectDefinition` whose parameter ids come from the built properties.
fn definition(
    r: &SchemaRegistry,
    effect_id: &str,
    parameters: &[(&str, Value)],
    build: impl Fn(&[PropertyId]) -> EffectParameters,
) -> (EffectDefinition, Vec<Property>, Vec<PropertyId>) {
    let properties = parameters
        .iter()
        .map(|(key, value)| prop(r, key, value.clone()))
        .collect::<Vec<_>>();
    let ids = properties.iter().map(|p| p.id()).collect::<Vec<_>>();
    (
        EffectDefinition {
            effect_id: effect_id.into(),
            version: 1,
            parameters: build(&ids),
        },
        properties,
        ids,
    )
}
fn resolve_values(ids: &[PropertyId], values: &[Value]) -> BTreeMap<PropertyId, Value> {
    ids.iter().copied().zip(values.iter().cloned()).collect()
}

#[test]
fn color002_exposure_resolves_within_bounds_and_preserves_alpha_contract() {
    let r = registry();
    let (d, properties, ids) = definition(
        &r,
        COLOR_EXPOSURE_ID,
        &[
            ("kronello.effect.exposure", scalar(0.0)),
            ("kronello.effect.exposure_offset", scalar(0.0)),
        ],
        |ids| EffectParameters::ColorExposure {
            exposure: ids[0],
            offset: ids[1],
        },
    );
    let [exposure, offset] = ids[..] else {
        panic!()
    };
    d.validate(&properties, &r).unwrap();
    let resolved = d
        .resolve(&resolve_values(&ids, &[scalar(2.0), scalar(-0.25)]))
        .unwrap();
    assert_eq!(
        resolved,
        ResolvedEffect::ColorExposure {
            exposure: 2.0,
            offset: -0.25
        }
    );
    // Bounds are typed rejections on the offending parameter, not clamps.
    assert!(matches!(
        d.resolve(&resolve_values(&ids, &[scalar(1025.0), scalar(0.0)])),
        Err(EffectError::InvalidParameter(id)) if id == exposure
    ));
    assert!(matches!(
        d.resolve(&resolve_values(&ids, &[scalar(0.0), scalar(1_000_001.0)])),
        Err(EffectError::InvalidParameter(id)) if id == offset
    ));
    assert!(d.resolve(&BTreeMap::new()).is_err());
    // Unknown versions and foreign ids are unsupported, never executed.
    for (id, version) in [
        (COLOR_EXPOSURE_ID, 0),
        (COLOR_EXPOSURE_ID, 2),
        ("vendor.exposure", 1),
    ] {
        let mut unsupported = d.clone();
        unsupported.effect_id = id.into();
        unsupported.version = version;
        assert!(matches!(
            unsupported.validate(&properties, &r),
            Err(EffectError::UnsupportedFeature)
        ));
    }
    // Parameter descriptors pin the value type and unit contract.
    let wrong = prop(
        &r,
        "kronello.effect.offset",
        Value::Vec2([FiniteF64::new(0.0).unwrap(); 2]),
    );
    let mismatched = EffectDefinition {
        parameters: EffectParameters::ColorExposure {
            exposure: wrong.id(),
            offset: wrong.id(),
        },
        ..d.clone()
    };
    assert!(matches!(
        mismatched.validate(&[wrong], &r),
        Err(EffectError::InvalidParameter(_))
    ));
}

#[test]
fn color002_levels_rejects_degenerate_ranges_at_resolution() {
    let r = registry();
    let defaults = [
        scalar(0.0),
        scalar(1.0),
        scalar(1.0),
        scalar(0.0),
        scalar(1.0),
    ];
    let (d, properties, ids) = definition(
        &r,
        COLOR_LEVELS_ID,
        &[
            ("kronello.effect.in_black", defaults[0].clone()),
            ("kronello.effect.in_white", defaults[1].clone()),
            ("kronello.effect.gamma", defaults[2].clone()),
            ("kronello.effect.out_black", defaults[3].clone()),
            ("kronello.effect.out_white", defaults[4].clone()),
        ],
        |ids| EffectParameters::ColorLevels {
            in_black: ids[0],
            in_white: ids[1],
            gamma: ids[2],
            out_black: ids[3],
            out_white: ids[4],
        },
    );
    d.validate(&properties, &r).unwrap();
    let resolve = |v: [f64; 5]| d.resolve(&resolve_values(&ids, &v.map(scalar)));
    // in_white <= in_black is a typed error on the in_white parameter.
    for bad in [[0.5, 0.5, 1.0, 0.0, 1.0], [0.5, 0.25, 1.0, 0.0, 1.0]] {
        assert!(matches!(
            resolve(bad),
            Err(EffectError::InvalidParameter(id)) if id == ids[1]
        ));
    }
    // gamma must be strictly positive; zero and negatives are rejected.
    for gamma in [0.0, -1.0] {
        assert!(matches!(
            resolve([0.0, 1.0, gamma, 0.0, 1.0]),
            Err(EffectError::InvalidParameter(id)) if id == ids[2]
        ));
    }
    assert_eq!(
        resolve([0.1, 0.9, 2.0, -0.1, 1.1]).unwrap(),
        ResolvedEffect::ColorLevels {
            in_black: 0.1,
            in_white: 0.9,
            gamma: 2.0,
            out_black: -0.1,
            out_white: 1.1,
        }
    );
    // HDR-friendly output range is bounded but not clamped in either direction.
    assert!(resolve([0.0, 1.0, 1.0, -1_000_000.0, 1_000_000.0]).is_ok());
    assert!(matches!(
        resolve([0.0, 1.0, 1.0, 0.0, 1_000_001.0]),
        Err(EffectError::InvalidParameter(id)) if id == ids[4]
    ));
}

#[test]
fn color002_curves_table_shape_and_monotonicity_are_structural() {
    let r = registry();
    let (d, properties, ids) = definition(
        &r,
        COLOR_CURVES_ID,
        &[(
            "kronello.effect.curve",
            curve_table(&[(0.0, 0.0), (1.0, 1.0)]),
        )],
        |ids| EffectParameters::ColorCurves { curve: ids[0] },
    );
    let curve = ids[0];
    d.validate(&properties, &r).unwrap();
    let resolve =
        |points: &[(f64, f64)]| d.resolve(&BTreeMap::from([(curve, curve_table(points))]));
    assert_eq!(
        resolve(&[(0.0, 0.0), (0.5, 0.75), (1.0, 1.0)]).unwrap(),
        ResolvedEffect::ColorCurves {
            curve: vec![[0.0, 0.0], [0.5, 0.75], [1.0, 1.0]]
        }
    );
    for points in [
        // Fewer than two rows.
        vec![[0.0, 0.0]],
        // Non-increasing x coordinates.
        vec![[0.0, 0.0], [0.5, 0.5], [0.5, 1.0]],
        vec![[0.0, 0.0], [0.75, 0.5], [0.5, 1.0]],
        // Coordinates outside the unit interval.
        vec![[0.0, 0.0], [1.5, 1.0]],
        vec![[0.0, -0.5], [1.0, 1.0]],
    ] {
        let pairs: Vec<(f64, f64)> = points.iter().map(|p| (p[0], p[1])).collect();
        assert!(matches!(
            resolve(&pairs),
            Err(EffectError::InvalidParameter(id)) if id == curve
        ));
    }
    // Row budget: 64 points pass, 65 are rejected purely by count.
    let at_limit: Vec<(f64, f64)> = (0..CURVES_MAX_POINTS)
        .map(|i| (i as f64 / 63.0, i as f64 / 63.0))
        .collect();
    assert!(resolve(&at_limit).is_ok());
    let over: Vec<(f64, f64)> = (0..=CURVES_MAX_POINTS)
        .map(|i| (i as f64 / CURVES_MAX_POINTS as f64, 0.5))
        .collect();
    assert_eq!(over.len(), CURVES_MAX_POINTS + 1);
    assert!(matches!(
        resolve(&over),
        Err(EffectError::InvalidParameter(id)) if id == curve
    ));
    // Wrong column names, column count, or value types are structural errors.
    let wrong_columns = Value::DataTable(DataTable {
        columns: BTreeMap::from([
            ("x".to_string(), ValueType::Scalar),
            ("z".to_string(), ValueType::Scalar),
        ]),
        rows: vec![],
    });
    assert!(matches!(
        d.resolve(&BTreeMap::from([(curve, wrong_columns)])),
        Err(EffectError::InvalidParameter(id)) if id == curve
    ));
    assert!(matches!(
        d.resolve(&BTreeMap::from([(curve, scalar(0.5))])),
        Err(EffectError::InvalidParameter(id)) if id == curve
    ));
    // The curve reference requires the data-table descriptor type.
    let wrong = prop(&r, "kronello.effect.exposure", scalar(0.0));
    let mismatched = EffectDefinition {
        parameters: EffectParameters::ColorCurves { curve: wrong.id() },
        ..d.clone()
    };
    assert!(matches!(
        mismatched.validate(&[wrong], &r),
        Err(EffectError::InvalidParameter(_))
    ));
}

#[test]
fn color002_hsl_uses_degrees_and_bounded_multipliers() {
    let r = registry();
    let (d, properties, ids) = definition(
        &r,
        COLOR_HSL_ID,
        &[
            ("kronello.effect.hue_shift", angle(0.0)),
            ("kronello.effect.saturation", scalar(1.0)),
            ("kronello.effect.lightness", scalar(0.0)),
        ],
        |ids| EffectParameters::ColorHsl {
            hue_shift: ids[0],
            saturation: ids[1],
            lightness: ids[2],
        },
    );
    let [hue, saturation, lightness] = ids[..] else {
        panic!()
    };
    d.validate(&properties, &r).unwrap();
    assert_eq!(
        d.resolve(&resolve_values(
            &ids,
            &[angle(120.0), scalar(0.5), scalar(0.1)]
        ))
        .unwrap(),
        ResolvedEffect::ColorHsl {
            hue_shift: 120.0,
            saturation: 0.5,
            lightness: 0.1,
        }
    );
    // Hue is an Angle value in degrees; a Scalar is a type mismatch.
    assert!(matches!(
        d.resolve(&resolve_values(&ids, &[scalar(120.0), scalar(1.0), scalar(0.0)])),
        Err(EffectError::InvalidParameter(id)) if id == hue
    ));
    let _ = (saturation, lightness);
    // Negative saturation deterministically inverts chroma and is allowed;
    // only magnitude beyond the scalar budget is rejected.
    assert!(
        d.resolve(&resolve_values(
            &ids,
            &[angle(0.0), scalar(-2.0), scalar(0.0)]
        ))
        .is_ok()
    );
    assert!(matches!(
        d.resolve(&resolve_values(&ids, &[angle(1_000_001.0), scalar(1.0), scalar(0.0)])),
        Err(EffectError::InvalidParameter(id)) if id == hue
    ));
}

#[test]
fn color002_effect_ids_are_distinct_versioned_and_described() {
    let r = registry();
    // Descriptors for every parameter key exist with the documented defaults.
    for (key, default) in [
        ("kronello.effect.exposure", scalar(0.0)),
        ("kronello.effect.exposure_offset", scalar(0.0)),
        ("kronello.effect.in_black", scalar(0.0)),
        ("kronello.effect.in_white", scalar(1.0)),
        ("kronello.effect.gamma", scalar(1.0)),
        ("kronello.effect.out_black", scalar(0.0)),
        ("kronello.effect.out_white", scalar(1.0)),
        ("kronello.effect.hue_shift", angle(0.0)),
        ("kronello.effect.saturation", scalar(1.0)),
        ("kronello.effect.lightness", scalar(0.0)),
    ] {
        let d = r.lookup(&SchemaKey::new(key).unwrap()).unwrap();
        assert_eq!(d.definition().default, default, "{key}");
    }
    let curve = r
        .lookup(&SchemaKey::new("kronello.effect.curve").unwrap())
        .unwrap();
    assert_eq!(curve.definition().value_type, ValueType::DataTable);
    // gamma zero is excluded at the descriptor range level as well.
    let gamma = r
        .lookup(&SchemaKey::new("kronello.effect.gamma").unwrap())
        .unwrap();
    assert!(gamma.validate_value(&scalar(0.0)).is_err());
    // Every color effect id lives in the kronello.color namespace.
    for id in [
        COLOR_EXPOSURE_ID,
        COLOR_LEVELS_ID,
        COLOR_CURVES_ID,
        COLOR_HSL_ID,
    ] {
        assert!(id.starts_with("kronello.color."));
    }
    let schema = serde_json::to_value(project_json_schema()).unwrap();
    let variants = serde_json::to_string(&schema["$defs"]["EffectParameters"]).unwrap();
    for kind in [
        "color_exposure",
        "color_levels",
        "color_curves",
        "color_hsl",
    ] {
        assert!(variants.contains(kind), "{kind} missing from schema");
    }
}
