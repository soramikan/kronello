use kronello_model::*;

fn number(value: f64) -> FiniteF64 {
    FiniteF64::new(value).unwrap()
}

fn key(value: &str) -> SchemaKey {
    SchemaKey::new(value).unwrap()
}

fn descriptor(registry: &SchemaRegistry, name: &str) -> PropertyDescriptor {
    registry.lookup(&key(name)).unwrap().clone()
}

#[test]
fn builtin_registry_has_fixed_uuid_v4_key_mappings() {
    let registry = SchemaRegistry::with_builtin();
    let mappings = [
        (
            BLEND_MODE_ID,
            BLEND_KEY,
            "7ba7fae2-3e1d-4bd9-9b84-2e6a6a9fe101",
        ),
        (
            AUDIO_VOLUME_ID,
            "kronello.audio.volume",
            "e9cf4a80-2b64-4b8e-9e29-dfe6bc119a63",
        ),
        (
            TRANSFORM_POSITION_ID,
            "kronello.transform.position",
            "60b3f16c-3677-4c83-8b90-f175f965ee98",
        ),
        (
            TRANSFORM_ANCHOR_ID,
            "kronello.transform.anchor",
            "83e2ea66-c48b-4cc9-9f70-2b9fa2e0584f",
        ),
        (
            TRANSFORM_SCALE_ID,
            "kronello.transform.scale",
            "faec5922-1cdb-44da-a24c-4f3d8fc03739",
        ),
        (
            TRANSFORM_ROTATION_ID,
            "kronello.transform.rotation",
            "24c59f64-041a-45a4-9cbe-dc709cc0cf98",
        ),
        (
            TRANSFORM_SKEW_ID,
            "kronello.transform.skew",
            "7d31c350-09e9-4380-aa73-f886297e8364",
        ),
        (
            OPACITY_ID,
            "kronello.opacity",
            "a3e91b5c-c1d2-4950-8f03-156cfa7d3b9b",
        ),
        (
            FILL_COLOR_ID,
            "kronello.fill_color",
            "e35b24d6-139d-4eea-b7dc-4a289770a9c5",
        ),
        (
            STROKE_WIDTH_ID,
            "kronello.stroke_width",
            "98c01340-51f5-4b72-a039-722ba5b3a3b0",
        ),
        (
            MASK_PATH_ID,
            "kronello.mask.path",
            "f0000000-0010-4400-8000-000000000001",
        ),
        (
            MASK_FEATHER_ID,
            "kronello.mask.feather",
            "f0000000-0010-4400-8000-000000000002",
        ),
        (
            MASK_EXPANSION_ID,
            "kronello.mask.expansion",
            "f0000000-0010-4400-8000-000000000003",
        ),
        (
            MASK_OPACITY_ID,
            "kronello.mask.opacity",
            "f0000000-0010-4400-8000-000000000004",
        ),
    ];
    assert_eq!(registry.len(), mappings.len());
    for (id, name, uuid) in mappings {
        assert_eq!(id.to_string(), uuid);
        assert_eq!(id.as_uuid().get_version_num(), 4);
        assert_eq!(id.as_uuid().get_variant(), uuid::Variant::RFC4122);
        assert_eq!(descriptor(&registry, name).id(), id);
    }
}

#[test]
fn builtin_registries_have_identical_ids_keys_and_contents() {
    let first = SchemaRegistry::with_builtin();
    let second = SchemaRegistry::with_builtin();
    assert_eq!(
        first.iter().collect::<Vec<_>>(),
        second.iter().collect::<Vec<_>>()
    );
}

#[test]
fn builtin_registry_enumeration_is_lexical_and_repeatable() {
    let expected = [
        "kronello.audio.volume",
        "kronello.blend_mode",
        "kronello.fill_color",
        "kronello.mask.expansion",
        "kronello.mask.feather",
        "kronello.mask.opacity",
        "kronello.mask.path",
        "kronello.opacity",
        "kronello.stroke_width",
        "kronello.transform.anchor",
        "kronello.transform.position",
        "kronello.transform.rotation",
        "kronello.transform.scale",
        "kronello.transform.skew",
    ];
    let registry = SchemaRegistry::with_builtin();
    for _ in 0..2 {
        assert_eq!(
            registry
                .iter()
                .map(|(key, _)| key.as_str())
                .collect::<Vec<_>>(),
            expected
        );
        let another = SchemaRegistry::with_builtin();
        assert_eq!(
            another
                .iter()
                .map(|(key, _)| key.as_str())
                .collect::<Vec<_>>(),
            expected
        );
    }
}

#[test]
fn builtin_descriptor_defaults_match_spec_and_validate() {
    let registry = SchemaRegistry::with_builtin();
    let defaults = [
        ("kronello.transform.position", Value::Vec2([number(0.0); 2])),
        ("kronello.transform.anchor", Value::Vec2([number(0.0); 2])),
        ("kronello.transform.scale", Value::Vec2([number(1.0); 2])),
        ("kronello.transform.rotation", Value::Angle(number(0.0))),
        ("kronello.transform.skew", Value::Angle(number(0.0))),
        ("kronello.opacity", Value::Scalar(number(1.0))),
        (
            "kronello.fill_color",
            Value::Color(Color::from_srgb8([0; 3], None)),
        ),
        ("kronello.stroke_width", Value::Scalar(number(0.0))),
    ];
    for (name, expected) in defaults {
        let d = descriptor(&registry, name);
        assert_eq!(d.definition().default, expected, "{name}");
        d.validate_value(&expected).unwrap();
        assert_eq!(
            PropertyDescriptor::from_json(&serde_json::to_string(&d).unwrap()).unwrap(),
            d
        );
    }
}

#[test]
fn builtin_descriptor_types_units_and_interpolation_match_contract() {
    let registry = SchemaRegistry::with_builtin();
    for (name, value_type, unit) in [
        (
            "kronello.transform.position",
            ValueType::Vec2,
            Unit::DesignPx,
        ),
        ("kronello.transform.anchor", ValueType::Vec2, Unit::DesignPx),
        (
            "kronello.transform.scale",
            ValueType::Vec2,
            Unit::Dimensionless,
        ),
        (
            "kronello.transform.rotation",
            ValueType::Angle,
            Unit::Degrees,
        ),
        ("kronello.transform.skew", ValueType::Angle, Unit::Degrees),
        ("kronello.opacity", ValueType::Scalar, Unit::Dimensionless),
        ("kronello.fill_color", ValueType::Color, Unit::Dimensionless),
        ("kronello.stroke_width", ValueType::Scalar, Unit::DesignPx),
    ] {
        let d = descriptor(&registry, name);
        let definition = d.definition();
        assert_eq!(definition.value_type, value_type);
        assert_eq!(definition.unit, unit);
        assert_eq!(definition.version, 1);
        assert!(definition.animatable);
        assert_eq!(
            definition
                .interpolation_modes
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            [
                InterpolationMode::Hold,
                InterpolationMode::Linear,
                InterpolationMode::Cubic
            ]
        );
    }
}

#[test]
fn builtin_position_and_anchor_use_parent_and_local_design_spaces() {
    let registry = SchemaRegistry::with_builtin();
    for (name, space) in [
        ("kronello.transform.position", CoordinateSpace::ParentDesign),
        ("kronello.transform.anchor", CoordinateSpace::LocalDesign),
    ] {
        let d = descriptor(&registry, name);
        assert_eq!(d.definition().coordinate_space, Some(space));
        assert_eq!(d.definition().range, None);
        d.validate_value(&Value::Vec2([number(-100.0), number(100_000.0)]))
            .unwrap();
    }
    assert_eq!(
        descriptor(&registry, "kronello.mask.path")
            .definition()
            .coordinate_space,
        Some(CoordinateSpace::LocalDesign)
    );
    for (_, d) in registry.iter() {
        let spatial = matches!(
            d.definition().value_type,
            ValueType::Vec2 | ValueType::Vec3 | ValueType::Path
        ) && d.definition().unit == Unit::DesignPx;
        if !spatial {
            assert_eq!(d.definition().coordinate_space, None);
        }
    }
}

#[test]
fn builtin_scale_allows_negative_and_zero_ratios() {
    let registry = SchemaRegistry::with_builtin();
    let d = descriptor(&registry, "kronello.transform.scale");
    assert_eq!(d.definition().range, None);
    for values in [[-1.0, 0.0], [0.0, -2.0], [f64::MAX, -f64::MAX]] {
        d.validate_value(&Value::Vec2(values.map(number))).unwrap();
    }
}

#[test]
fn builtin_rotation_and_skew_preserve_unbounded_continuous_degrees() {
    let registry = SchemaRegistry::with_builtin();
    for name in ["kronello.transform.rotation", "kronello.transform.skew"] {
        let d = descriptor(&registry, name);
        assert_eq!(d.definition().range, None);
        for degrees in [720.0, -720.0, f64::MAX] {
            let value = Value::Angle(number(degrees));
            d.validate_value(&value).unwrap();
            let property = Property::new(
                PropertyId::new(),
                DescriptorRef::new(&d),
                PropertySource::Constant(value.clone()),
                vec![],
                &registry,
            )
            .unwrap();
            let json = serde_json::to_string(&property).unwrap();
            let restored = Property::from_json(&json, &registry).unwrap();
            assert_eq!(restored.source(), &PropertySource::Constant(value));
        }
    }
}

#[test]
fn builtin_opacity_rejects_out_of_range_without_clamping() {
    let registry = SchemaRegistry::with_builtin();
    let d = descriptor(&registry, "kronello.opacity");
    assert_eq!(
        d.definition().range,
        Some(ValueRange::Scalar(
            NumericRange::inclusive(0.0, 1.0).unwrap()
        ))
    );
    for value in [0.0, 1.0] {
        d.validate_value(&Value::Scalar(number(value))).unwrap();
    }
    for value in [-0.1, 1.5] {
        let invalid = Value::Scalar(number(value));
        assert_eq!(
            d.validate_value(&invalid),
            Err(ModelError::OutOfRange {
                component: 0,
                value
            })
        );
        assert_eq!(invalid, Value::Scalar(number(value)));
        assert!(matches!(
            Property::new(
                PropertyId::new(),
                DescriptorRef::new(&d),
                PropertySource::Constant(invalid),
                vec![],
                &registry
            ),
            Err(ModelError::OutOfRange { .. })
        ));
    }
}

#[test]
fn builtin_stroke_width_rejects_negative_without_clamping() {
    let registry = SchemaRegistry::with_builtin();
    let d = descriptor(&registry, "kronello.stroke_width");
    assert_eq!(
        d.definition().range,
        Some(ValueRange::Scalar(NumericRange {
            min: Some(NumericBound {
                value: number(0.0),
                inclusive: true
            }),
            max: None,
        }))
    );
    for value in [0.0, 1.0, f64::MAX] {
        d.validate_value(&Value::Scalar(number(value))).unwrap();
    }
    let invalid = Value::Scalar(number(-1.0));
    assert_eq!(
        d.validate_value(&invalid),
        Err(ModelError::OutOfRange {
            component: 0,
            value: -1.0
        })
    );
    assert_eq!(invalid, Value::Scalar(number(-1.0)));
    assert!(matches!(
        Property::new(
            PropertyId::new(),
            DescriptorRef::new(&d),
            PropertySource::Constant(invalid),
            vec![],
            &registry
        ),
        Err(ModelError::OutOfRange { .. })
    ));
}

#[test]
fn builtin_fill_color_defaults_to_srgb_black_and_working_linear_interpolation() {
    let registry = SchemaRegistry::with_builtin();
    let d = descriptor(&registry, "kronello.fill_color");
    assert_eq!(d.definition().color_interpolation_space, None);
    assert_eq!(d.definition().range, None);
    let Value::Color(color) = d.definition().default else {
        panic!("expected Color")
    };
    assert_eq!(color.space(), ColorSpace::Srgb);
    assert_eq!(
        color.components(),
        ColorComponents {
            r: number(0.0),
            g: number(0.0),
            b: number(0.0),
            alpha: number(1.0),
        }
    );
    for space in [ColorSpace::LinearRec709, ColorSpace::LinearRec2020] {
        d.validate_value(&Value::Color(
            Color::new(space, [-0.25, 4.0, 0.5], 0.0).unwrap(),
        ))
        .unwrap();
    }
}

#[test]
fn builtin_reregistration_rejects_duplicate_keys_atomically() {
    let mut registry = SchemaRegistry::with_builtin();
    let original = SchemaRegistry::with_builtin();
    for (key, d) in original.iter() {
        assert_eq!(
            registry.register(d.clone()),
            Err(ModelError::DuplicateSchemaKey { key: key.clone() })
        );
        assert_eq!(
            registry.iter().collect::<Vec<_>>(),
            original.iter().collect::<Vec<_>>()
        );
    }
}

#[test]
fn builtin_key_conflicts_reject_different_ids_atomically() {
    let mut registry = SchemaRegistry::with_builtin();
    let original = SchemaRegistry::with_builtin();
    for (key, d) in original.iter() {
        let mut conflicting = d.definition().clone();
        conflicting.id = DescriptorId::from_uuid(uuid::Uuid::from_u128(
            0x01234567_89ab_4cde_8f01_23456789abcd,
        ));
        conflicting.name = "Conflicting definition".into();
        assert_eq!(
            registry.register(PropertyDescriptor::new(conflicting).unwrap()),
            Err(ModelError::DuplicateSchemaKey { key: key.clone() })
        );
        assert_eq!(
            registry.iter().collect::<Vec<_>>(),
            original.iter().collect::<Vec<_>>()
        );
    }
}

#[test]
fn builtin_id_conflicts_reject_different_keys_atomically() {
    let mut registry = SchemaRegistry::with_builtin();
    let original = SchemaRegistry::with_builtin();
    for (_, d) in original.iter() {
        let mut conflicting = d.definition().clone();
        conflicting.key = key("extension.conflicting_identity");
        assert_eq!(
            registry.register(PropertyDescriptor::new(conflicting).unwrap()),
            Err(ModelError::DuplicateDescriptorId)
        );
        assert_eq!(
            registry.iter().collect::<Vec<_>>(),
            original.iter().collect::<Vec<_>>()
        );
    }
}
