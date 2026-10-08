//! FX-005 keying and FX-006 standard effect model contracts (ADR-0115).
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
fn vec2(x: f64, y: f64) -> Value {
    Value::Vec2([FiniteF64::new(x).unwrap(), FiniteF64::new(y).unwrap()])
}
fn color(r: u8, g: u8, b: u8) -> Value {
    Value::Color(Color::from_srgb8([r, g, b], None))
}
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
fn fx005_chroma_key_resolves_and_enforces_unit_interval() {
    let r = registry();
    let (d, properties, ids) = definition(
        &r,
        KEYING_CHROMA_ID,
        &[
            ("kronello.effect.key_color", color(0, 177, 64)),
            ("kronello.effect.similarity", scalar(0.4)),
            ("kronello.effect.edge_shrink", scalar(0.0)),
            ("kronello.effect.edge_feather", scalar(0.0)),
            ("kronello.effect.spill", scalar(0.5)),
        ],
        |ids| EffectParameters::ChromaKey {
            key_color: ids[0],
            similarity: ids[1],
            edge_shrink: ids[2],
            edge_feather: ids[3],
            spill: ids[4],
        },
    );
    d.validate(&properties, &r).unwrap();
    let resolved = d.resolve(&resolve_values(&ids, &[])).ok();
    // Resolution needs every referenced property's value.
    assert!(resolved.is_none());
    let resolved = d
        .resolve(&resolve_values(
            &ids,
            &[
                color(0, 177, 64),
                scalar(0.4),
                scalar(1.5),
                scalar(0.75),
                scalar(1.0),
            ],
        ))
        .unwrap();
    let ResolvedEffect::ChromaKey {
        key_color,
        similarity,
        edge_shrink,
        edge_feather,
        spill,
    } = resolved
    else {
        panic!("chroma key resolved variant")
    };
    assert_eq!(similarity, 0.4);
    assert_eq!(edge_shrink, 1.5);
    assert_eq!(edge_feather, 0.75);
    assert_eq!(spill, 1.0);
    assert_eq!(
        key_color.components().g.get(),
        Color::from_srgb8([0, 177, 64], None).components().g.get()
    );
    // similarity and spill are unit-interval; lengths share the scalar budget.
    for (index, bad) in [(1, 1.01), (4, -0.001)] {
        let mut values = [
            color(0, 177, 64),
            scalar(0.4),
            scalar(0.0),
            scalar(0.0),
            scalar(0.5),
        ];
        values[index] = scalar(bad);
        assert!(matches!(
            d.resolve(&resolve_values(&ids, &values)),
            Err(EffectError::InvalidParameter(id)) if id == ids[index]
        ));
    }
    for index in [2, 3] {
        let mut values = [
            color(0, 177, 64),
            scalar(0.4),
            scalar(0.0),
            scalar(0.0),
            scalar(0.5),
        ];
        values[index] = scalar(1_000_001.0);
        assert!(matches!(
            d.resolve(&resolve_values(&ids, &values)),
            Err(EffectError::InvalidParameter(id)) if id == ids[index]
        ));
    }
    // A scalar in the color slot is a type mismatch on that parameter.
    let mut values = [
        scalar(0.4),
        scalar(0.4),
        scalar(0.0),
        scalar(0.0),
        scalar(0.5),
    ];
    values[0] = scalar(0.5);
    assert!(matches!(
        d.resolve(&resolve_values(&ids, &values)),
        Err(EffectError::InvalidParameter(id)) if id == ids[0]
    ));
}

#[test]
fn fx005_luma_key_resolves_and_rejects_out_of_range() {
    let r = registry();
    let (d, properties, ids) = definition(
        &r,
        KEYING_LUMA_ID,
        &[
            ("kronello.effect.key_luma", scalar(0.0)),
            ("kronello.effect.tolerance", scalar(0.1)),
            ("kronello.effect.edge_shrink", scalar(0.0)),
            ("kronello.effect.edge_feather", scalar(0.0)),
        ],
        |ids| EffectParameters::LumaKey {
            key_luma: ids[0],
            tolerance: ids[1],
            edge_shrink: ids[2],
            edge_feather: ids[3],
        },
    );
    d.validate(&properties, &r).unwrap();
    assert_eq!(
        d.resolve(&resolve_values(
            &ids,
            &[scalar(0.25), scalar(0.2), scalar(1.0), scalar(0.5)]
        ))
        .unwrap(),
        ResolvedEffect::LumaKey {
            key_luma: 0.25,
            tolerance: 0.2,
            edge_shrink: 1.0,
            edge_feather: 0.5,
        }
    );
    for (index, bad) in [(0, 1.01), (0, -0.01), (1, 1.5), (2, -1.0), (3, 1e7)] {
        let mut values = [scalar(0.5), scalar(0.1), scalar(0.0), scalar(0.0)];
        values[index] = scalar(bad);
        assert!(matches!(
            d.resolve(&resolve_values(&ids, &values)),
            Err(EffectError::InvalidParameter(id)) if id == ids[index]
        ));
    }
}

#[test]
fn fx006_glow_sharpen_vignette_resolve_with_budgets() {
    let r = registry();
    let (glow, glow_props, glow_ids) = definition(
        &r,
        GLOW_ID,
        &[
            ("kronello.effect.threshold", scalar(0.8)),
            ("kronello.effect.radius", scalar(8.0)),
            ("kronello.effect.intensity", scalar(1.0)),
        ],
        |ids| EffectParameters::Glow {
            threshold: ids[0],
            radius: ids[1],
            intensity: ids[2],
        },
    );
    glow.validate(&glow_props, &r).unwrap();
    assert_eq!(
        glow.resolve(&resolve_values(
            &glow_ids,
            &[scalar(0.5), scalar(12.0), scalar(2.0)]
        ))
        .unwrap(),
        ResolvedEffect::Glow {
            threshold: 0.5,
            radius: 12.0,
            intensity: 2.0,
        }
    );
    // Threshold/intensity/radius are nonnegative scalars, never unit-interval.
    for (index, bad) in [(0, -0.1), (1, -1.0), (2, -2.0), (1, 1_000_001.0)] {
        let mut values = [scalar(0.8), scalar(8.0), scalar(1.0)];
        values[index] = scalar(bad);
        assert!(matches!(
            glow.resolve(&resolve_values(&glow_ids, &values)),
            Err(EffectError::InvalidParameter(id)) if id == glow_ids[index]
        ));
    }
    // Intensity above 1.0 stays legal: glow is additive, not a lerp.
    assert!(
        glow.resolve(&resolve_values(
            &glow_ids,
            &[scalar(0.8), scalar(8.0), scalar(4.0)]
        ))
        .is_ok()
    );
    let (sharpen, sharpen_props, sharpen_ids) = definition(
        &r,
        SHARPEN_ID,
        &[
            ("kronello.effect.amount", scalar(0.5)),
            ("kronello.effect.radius", scalar(2.0)),
        ],
        |ids| EffectParameters::Sharpen {
            amount: ids[0],
            radius: ids[1],
        },
    );
    sharpen.validate(&sharpen_props, &r).unwrap();
    assert_eq!(
        sharpen
            .resolve(&resolve_values(&sharpen_ids, &[scalar(1.5), scalar(3.0)]))
            .unwrap(),
        ResolvedEffect::Sharpen {
            amount: 1.5,
            radius: 3.0,
        }
    );
    let (vignette, vignette_props, vignette_ids) = definition(
        &r,
        VIGNETTE_ID,
        &[
            ("kronello.effect.amount", scalar(0.5)),
            ("kronello.effect.midpoint", scalar(0.5)),
            ("kronello.effect.feather", scalar(0.5)),
            ("kronello.effect.roundness", scalar(0.5)),
        ],
        |ids| EffectParameters::Vignette {
            amount: ids[0],
            midpoint: ids[1],
            feather: ids[2],
            roundness: ids[3],
        },
    );
    vignette.validate(&vignette_props, &r).unwrap();
    assert_eq!(
        vignette
            .resolve(&resolve_values(
                &vignette_ids,
                &[scalar(0.75), scalar(0.25), scalar(0.9), scalar(1.0)]
            ))
            .unwrap(),
        ResolvedEffect::Vignette {
            amount: 0.75,
            midpoint: 0.25,
            feather: 0.9,
            roundness: 1.0,
        }
    );
    // amount/midpoint/roundness are unit-interval; feather is nonnegative.
    for (index, bad) in [(0, 1.01), (1, -0.5), (2, -0.1), (3, 1.5)] {
        let mut values = [scalar(0.5), scalar(0.5), scalar(0.5), scalar(0.5)];
        values[index] = scalar(bad);
        assert!(matches!(
            vignette.resolve(&resolve_values(&vignette_ids, &values)),
            Err(EffectError::InvalidParameter(id)) if id == vignette_ids[index]
        ));
    }
}

#[test]
fn fx006_corner_pin_uses_four_composition_space_vec2_points() {
    let r = registry();
    let (d, properties, ids) = definition(
        &r,
        CORNER_PIN_ID,
        &[
            ("kronello.effect.top_left", vec2(0.0, 0.0)),
            ("kronello.effect.top_right", vec2(100.0, 0.0)),
            ("kronello.effect.bottom_right", vec2(100.0, 100.0)),
            ("kronello.effect.bottom_left", vec2(0.0, 100.0)),
        ],
        |ids| EffectParameters::CornerPin {
            top_left: ids[0],
            top_right: ids[1],
            bottom_right: ids[2],
            bottom_left: ids[3],
        },
    );
    d.validate(&properties, &r).unwrap();
    assert_eq!(
        d.resolve(&resolve_values(
            &ids,
            &[
                vec2(10.0, 10.0),
                vec2(200.0, 20.0),
                vec2(190.0, 180.0),
                vec2(0.0, 200.0),
            ]
        ))
        .unwrap(),
        ResolvedEffect::CornerPin {
            corners: [[10.0, 10.0], [200.0, 20.0], [190.0, 180.0], [0.0, 200.0]],
        }
    );
    // Nonfinite/magnitude-bound pins and wrong value types are typed errors.
    let mut values = [
        vec2(0.0, 0.0),
        vec2(1.0, 0.0),
        vec2(1.0, 1.0),
        vec2(0.0, 1.0),
    ];
    values[1] = vec2(2_000_000.0, 0.0);
    assert!(matches!(
        d.resolve(&resolve_values(&ids, &values)),
        Err(EffectError::InvalidParameter(id)) if id == ids[1]
    ));
    values[1] = scalar(0.0);
    assert!(matches!(
        d.resolve(&resolve_values(&ids, &values)),
        Err(EffectError::InvalidParameter(id)) if id == ids[1]
    ));
    // A scalar-typed property in a Vec2 slot fails validate, not resolve.
    let wrong = prop(&r, "kronello.effect.sigma", scalar(0.0));
    let mismatched = EffectDefinition {
        parameters: EffectParameters::CornerPin {
            top_left: wrong.id(),
            top_right: ids[1],
            bottom_right: ids[2],
            bottom_left: ids[3],
        },
        ..d.clone()
    };
    assert!(matches!(
        mismatched.validate(&[wrong], &r),
        Err(EffectError::InvalidParameter(_))
    ));
    // Corner descriptors carry absolute Composition design coordinates.
    for key in [
        "kronello.effect.top_left",
        "kronello.effect.top_right",
        "kronello.effect.bottom_right",
        "kronello.effect.bottom_left",
    ] {
        let d = r.lookup(&SchemaKey::new(key).unwrap()).unwrap();
        assert_eq!(d.definition().unit, Unit::DesignPx, "{key}");
        assert_eq!(
            d.definition().coordinate_space,
            Some(CoordinateSpace::CompositionDesign),
            "{key}"
        );
    }
}

#[test]
fn fx005006_effect_ids_are_versioned_described_and_supported() {
    let r = registry();
    for id in [
        KEYING_CHROMA_ID,
        KEYING_LUMA_ID,
        GLOW_ID,
        SHARPEN_ID,
        VIGNETTE_ID,
        CORNER_PIN_ID,
    ] {
        assert!(id.starts_with("kronello."), "{id}");
    }
    // Documented descriptor defaults exist for every new parameter key.
    for (key, default) in [
        ("kronello.effect.key_luma", scalar(0.0)),
        ("kronello.effect.similarity", scalar(0.4)),
        ("kronello.effect.tolerance", scalar(0.1)),
        ("kronello.effect.edge_shrink", scalar(0.0)),
        ("kronello.effect.edge_feather", scalar(0.0)),
        ("kronello.effect.spill", scalar(0.5)),
        ("kronello.effect.threshold", scalar(0.8)),
        ("kronello.effect.radius", scalar(8.0)),
        ("kronello.effect.intensity", scalar(1.0)),
        ("kronello.effect.amount", scalar(0.5)),
        ("kronello.effect.midpoint", scalar(0.5)),
        ("kronello.effect.feather", scalar(0.5)),
        ("kronello.effect.roundness", scalar(0.5)),
    ] {
        let d = r.lookup(&SchemaKey::new(key).unwrap()).unwrap();
        assert_eq!(d.definition().default, default, "{key}");
    }
    let key_color = r
        .lookup(&SchemaKey::new("kronello.effect.key_color").unwrap())
        .unwrap();
    assert_eq!(key_color.definition().value_type, ValueType::Color);
    // Unit-interval ranges live on the descriptors as well as resolution.
    for key in [
        "kronello.effect.similarity",
        "kronello.effect.spill",
        "kronello.effect.key_luma",
        "kronello.effect.tolerance",
        "kronello.effect.midpoint",
        "kronello.effect.roundness",
    ] {
        let d = r.lookup(&SchemaKey::new(key).unwrap()).unwrap();
        assert!(d.validate_value(&scalar(1.5)).is_err(), "{key}");
        assert!(d.validate_value(&scalar(-0.5)).is_err(), "{key}");
        assert!(d.validate_value(&scalar(0.5)).is_ok(), "{key}");
    }
    // Version 0 and 2 remain unsupported; version pins are semantic.
    let r2 = registry();
    let (d, properties, _) = definition(
        &r2,
        GLOW_ID,
        &[
            ("kronello.effect.threshold", scalar(0.8)),
            ("kronello.effect.radius", scalar(8.0)),
            ("kronello.effect.intensity", scalar(1.0)),
        ],
        |ids| EffectParameters::Glow {
            threshold: ids[0],
            radius: ids[1],
            intensity: ids[2],
        },
    );
    for version in [0, 2] {
        let mut unsupported = d.clone();
        unsupported.version = version;
        assert!(matches!(
            unsupported.validate(&properties, &r2),
            Err(EffectError::UnsupportedFeature)
        ));
    }
    let mut foreign = d.clone();
    foreign.effect_id = "vendor.glow".into();
    assert!(matches!(
        foreign.validate(&properties, &r2),
        Err(EffectError::UnsupportedFeature)
    ));
    // The serde schema exposes the new kind tags.
    let schema = serde_json::to_value(project_json_schema()).unwrap();
    let variants = serde_json::to_string(&schema["$defs"]["EffectParameters"]).unwrap();
    for kind in [
        "chroma_key",
        "luma_key",
        "glow",
        "sharpen",
        "vignette",
        "corner_pin",
    ] {
        assert!(variants.contains(kind), "{kind} missing from schema");
    }
}

#[test]
fn fx005006_unknown_effects_stay_opaque_and_round_trip() {
    // Unrelated opaque effects still round-trip verbatim and are never
    // resolved or validated as known effects.
    let raw = serde_json::json!({
        "effect_id": "vendor.keyer",
        "version": 7,
        "parameters": {"kind": "vendor_keyer", "gain": 4}
    });
    let effect: Effect = serde_json::from_value(raw.clone()).unwrap();
    assert!(matches!(effect, Effect::Opaque(_)));
    assert_eq!(serde_json::to_value(&effect).unwrap(), raw);
    assert!(matches!(
        effect.definition(),
        Err(EffectError::UnsupportedFeature)
    ));
}
