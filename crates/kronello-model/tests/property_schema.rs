use kronello_model::*;
use serde_json::{Value as JsonValue, json};
use std::collections::BTreeMap;

fn number(value: f64) -> FiniteF64 {
    FiniteF64::new(value).expect("finite fixture")
}
fn scalar(value: f64) -> Value {
    Value::Scalar(number(value))
}
fn key(value: &str) -> SchemaKey {
    SchemaKey::new(value).expect("valid fixture key")
}
fn definition(name: &str) -> DescriptorDefinition {
    DescriptorDefinition::new(
        DescriptorId::new(),
        key(name),
        "Display name",
        ValueType::Scalar,
        Unit::Dimensionless,
        scalar(0.5),
    )
}
fn descriptor(name: &str) -> PropertyDescriptor {
    PropertyDescriptor::new(definition(name)).unwrap()
}
fn opacity() -> PropertyDescriptor {
    let mut d = definition("kronello.opacity");
    d.range = Some(ValueRange::Scalar(
        NumericRange::inclusive(0.0, 1.0).unwrap(),
    ));
    PropertyDescriptor::new(d).unwrap()
}
fn registry_with(d: PropertyDescriptor) -> (SchemaRegistry, DescriptorRef) {
    let reference = DescriptorRef::new(&d);
    let mut registry = SchemaRegistry::new();
    registry.register(d).unwrap();
    (registry, reference)
}
fn modifier(name: &str) -> Modifier {
    Modifier {
        id: ModifierId::new(),
        key: key(name),
        version: 1,
        enabled: true,
        parameters: BTreeMap::new(),
    }
}
fn assert_incompatible<T: serde::de::DeserializeOwned>(input: &str) {
    assert!(
        matches!(
            from_json::<T>(input),
            Err(JsonError::IncompatibleStructure { .. })
        ),
        "input unexpectedly accepted: {input}"
    );
}

#[test]
fn descriptor_round_trip_expresses_common_schema() {
    let d = opacity();
    let encoded = serde_json::to_string(&d).unwrap();
    assert_eq!(PropertyDescriptor::from_json(&encoded).unwrap(), d);
    let wire: JsonValue = serde_json::from_str(&encoded).unwrap();
    for field in [
        "id",
        "key",
        "value_type",
        "unit",
        "interpolation_modes",
        "range",
        "default",
        "animatable",
        "capabilities",
        "version",
    ] {
        assert!(
            wire.get(field).is_some(),
            "missing common schema field {field}"
        );
    }
    assert_eq!(d.definition().value_type, ValueType::Scalar);
    assert_eq!(d.definition().unit, Unit::Dimensionless);
    assert_eq!(d.definition().interpolation_modes.len(), 3);
}

#[test]
fn ids_and_keys_survive_display_name_changes() {
    let mut d = descriptor("kronello.position");
    let identity = (d.id(), d.key().clone(), d.version());
    d.rename("位置");
    assert_eq!((d.id(), d.key().clone(), d.version()), identity);
    let (mut registry, reference) = registry_with(d);
    let p = Property::new(
        PropertyId::new(),
        reference.clone(),
        PropertySource::Constant(scalar(0.5)),
        vec![],
        &registry,
    )
    .unwrap();
    let property_id = p.id();
    registry.rename(&reference.key, "Renamed again").unwrap();
    assert_eq!(reference.resolve(&registry).unwrap().id(), identity.0);
    assert_eq!(p.id(), property_id);
    p.validate(&registry).unwrap();
}

#[test]
fn uuid_ids_round_trip_and_are_distinct_types() {
    let p = PropertyId::new();
    let curve = CurveId::new();
    let expression = ExpressionId::new();
    assert_ne!(p.as_uuid(), curve.as_uuid());
    assert_ne!(curve.as_uuid(), expression.as_uuid());
    assert_eq!(p.as_uuid().get_version_num(), 4);
    assert_eq!(
        from_json::<PropertyId>(&serde_json::to_string(&p).unwrap()).unwrap(),
        p
    );
    assert_eq!(
        from_json::<CurveId>(&serde_json::to_string(&curve).unwrap()).unwrap(),
        curve
    );
    assert_eq!(
        from_json::<ExpressionId>(&serde_json::to_string(&expression).unwrap()).unwrap(),
        expression
    );
    assert_incompatible::<PropertyId>("17");
    assert_incompatible::<PropertyId>("\"Display name\"");
}

#[test]
fn stable_keys_reject_invalid_namespaces() {
    for invalid in [
        "",
        ".opacity",
        "opacity.",
        "a..b",
        "Display Name",
        "a/../b",
        "日本語",
    ] {
        assert!(matches!(
            SchemaKey::new(invalid),
            Err(ModelError::InvalidSchemaKey { .. })
        ));
    }
    assert_eq!(
        key("vendor.effect.opacity_v1").as_str(),
        "vendor.effect.opacity_v1"
    );
}

#[test]
fn invalid_descriptor_type_unit_and_default_are_rejected() {
    let mut d = definition("test.opacity");
    d.unit = Unit::Degrees;
    assert!(matches!(
        PropertyDescriptor::new(d),
        Err(ModelError::IncompatibleUnit { .. })
    ));
    let mut d = definition("test.opacity");
    d.default = Value::Bool(false);
    assert!(matches!(
        PropertyDescriptor::new(d),
        Err(ModelError::ValueTypeMismatch { .. })
    ));
    let mut d = definition("test.opacity");
    d.range = Some(ValueRange::Scalar(
        NumericRange::inclusive(0.0, 0.2).unwrap(),
    ));
    assert!(matches!(
        PropertyDescriptor::new(d.clone()),
        Err(ModelError::OutOfRange { .. })
    ));
    assert!(matches!(
        PropertyDescriptor::from_json(&serde_json::to_string(&d).unwrap()),
        Err(JsonError::Validation(ModelError::OutOfRange { .. }))
    ));
}

#[test]
fn interpolation_is_derived_and_discrete_types_reject_linear_and_cubic() {
    for t in [
        ValueType::Scalar,
        ValueType::Vec2,
        ValueType::Vec3,
        ValueType::Angle,
        ValueType::Color,
    ] {
        assert_eq!(
            t.interpolation_modes(),
            &[
                InterpolationMode::Hold,
                InterpolationMode::Linear,
                InterpolationMode::Cubic
            ]
        );
    }
    for (t, value, unit) in [
        (ValueType::Bool, Value::Bool(false), Unit::Dimensionless),
        (
            ValueType::Enum,
            Value::Enum("on".into()),
            Unit::Dimensionless,
        ),
        (
            ValueType::String,
            Value::String("text".into()),
            Unit::Dimensionless,
        ),
        (
            ValueType::AssetRef,
            Value::AssetRef(AssetId::new()),
            Unit::Dimensionless,
        ),
        (
            ValueType::Path,
            Value::Path(Path { segments: vec![] }),
            Unit::DesignPx,
        ),
    ] {
        let mut d = DescriptorDefinition::new(
            DescriptorId::new(),
            key("test.discrete"),
            "Discrete",
            t,
            unit,
            value,
        );
        assert_eq!(t.interpolation_modes(), &[InterpolationMode::Hold]);
        PropertyDescriptor::new(d.clone()).unwrap();
        for mode in [InterpolationMode::Linear, InterpolationMode::Cubic] {
            d.interpolation_modes.insert(mode);
            assert!(matches!(
                PropertyDescriptor::new(d.clone()),
                Err(ModelError::IncompatibleInterpolation { .. })
            ));
            d.interpolation_modes.remove(&mode);
        }
    }
}

#[test]
fn empty_modes_and_inconsistent_capabilities_are_rejected() {
    let mut d = definition("test.scalar");
    d.interpolation_modes.clear();
    assert!(matches!(
        PropertyDescriptor::new(d),
        Err(ModelError::MissingInterpolation)
    ));
    let mut d = definition("test.scalar");
    d.animatable = false;
    assert!(matches!(
        PropertyDescriptor::new(d.clone()),
        Err(ModelError::IncompatibleCapabilities)
    ));
    d.capabilities.curves = false;
    d.capabilities.expressions = false;
    d.interpolation_modes.clear();
    PropertyDescriptor::new(d).unwrap();
}

#[test]
fn coordinate_space_must_match_design_units() {
    let mut d = DescriptorDefinition::new(
        DescriptorId::new(),
        key("test.position"),
        "Position",
        ValueType::Vec2,
        Unit::DesignPx,
        Value::Vec2([number(-100.0), number(200.0)]),
    );
    assert_eq!(d.coordinate_space, Some(CoordinateSpace::LocalDesign));
    for space in [
        CoordinateSpace::LocalDesign,
        CoordinateSpace::ParentDesign,
        CoordinateSpace::CompositionDesign,
    ] {
        d.coordinate_space = Some(space);
        let descriptor = PropertyDescriptor::new(d.clone()).unwrap();
        assert_eq!(
            PropertyDescriptor::from_json(&serde_json::to_string(&descriptor).unwrap()).unwrap(),
            descriptor
        );
    }
    d.coordinate_space = None;
    assert!(matches!(
        PropertyDescriptor::new(d),
        Err(ModelError::IncompatibleCoordinateSpace)
    ));
    let mut d = definition("test.scalar");
    d.coordinate_space = Some(CoordinateSpace::CompositionDesign);
    assert!(matches!(
        PropertyDescriptor::new(d),
        Err(ModelError::IncompatibleCoordinateSpace)
    ));
}

#[test]
fn numeric_ranges_validate_open_closed_and_unbounded_endpoints() {
    let closed = NumericRange::inclusive(0.0, 1.0).unwrap();
    closed.validate_component(number(0.0), 0).unwrap();
    closed.validate_component(number(1.0), 0).unwrap();
    let open = NumericRange {
        min: Some(NumericBound {
            value: number(0.0),
            inclusive: false,
        }),
        max: Some(NumericBound {
            value: number(1.0),
            inclusive: false,
        }),
    };
    for value in [0.0, 1.0] {
        assert!(matches!(
            open.validate_component(number(value), 0),
            Err(ModelError::OutOfRange { .. })
        ));
    }
    open.validate_component(number(0.5), 0).unwrap();
    NumericRange {
        min: None,
        max: None,
    }
    .validate_component(number(-1e300), 0)
    .unwrap();
    NumericRange {
        min: None,
        max: closed.max,
    }
    .validate_component(number(-1.0), 0)
    .unwrap();
    NumericRange {
        min: closed.min,
        max: None,
    }
    .validate_component(number(1e300), 0)
    .unwrap();
    assert!(matches!(
        NumericRange::inclusive(2.0, 1.0),
        Err(ModelError::InvalidRange)
    ));
    NumericRange::inclusive(1.0, 1.0)
        .unwrap()
        .validate_component(number(1.0), 0)
        .unwrap();
    assert!(matches!(
        NumericRange {
            min: Some(NumericBound {
                value: number(1.0),
                inclusive: false
            }),
            max: closed.max
        }
        .validate(),
        Err(ModelError::InvalidRange)
    ));
}

#[test]
fn vector_ranges_are_per_component_and_shape_checked() {
    let ranges = [
        NumericRange::inclusive(-10.0, 10.0).unwrap(),
        NumericRange::inclusive(0.0, 1.0).unwrap(),
    ];
    let mut d = DescriptorDefinition::new(
        DescriptorId::new(),
        key("test.vec2"),
        "Vector",
        ValueType::Vec2,
        Unit::Dimensionless,
        Value::Vec2([number(-2.0), number(0.5)]),
    );
    d.range = Some(ValueRange::Vec2(ranges));
    let descriptor = PropertyDescriptor::new(d.clone()).unwrap();
    assert!(matches!(
        descriptor.validate_value(&Value::Vec2([number(1.0), number(2.0)])),
        Err(ModelError::OutOfRange { component: 1, .. })
    ));
    d.range = Some(ValueRange::Scalar(ranges[0]));
    assert!(matches!(
        PropertyDescriptor::new(d),
        Err(ModelError::IncompatibleRange { .. })
    ));
    let range3 = ValueRange::Vec3([
        ranges[0],
        ranges[1],
        NumericRange::inclusive(10.0, 20.0).unwrap(),
    ]);
    range3
        .validate_value(&Value::Vec3([number(0.0), number(0.5), number(15.0)]))
        .unwrap();
    assert!(matches!(
        range3.validate_value(&Value::Vec3([number(0.0), number(0.5), number(25.0)])),
        Err(ModelError::OutOfRange { component: 2, .. })
    ));
}

#[test]
fn descriptor_rejects_invalid_intervals_and_versions() {
    let mut d = definition("test.scalar");
    d.range = Some(ValueRange::Scalar(NumericRange {
        min: Some(NumericBound {
            value: number(2.0),
            inclusive: true,
        }),
        max: Some(NumericBound {
            value: number(1.0),
            inclusive: true,
        }),
    }));
    assert!(matches!(
        PropertyDescriptor::new(d),
        Err(ModelError::InvalidRange)
    ));
    for version in [0, 2, u32::MAX] {
        let mut d = definition("test.scalar");
        d.version = version;
        assert_eq!(
            PropertyDescriptor::new(d).unwrap_err(),
            ModelError::UnsupportedDescriptorVersion { version }
        );
    }
}

#[test]
fn property_source_variants_are_exclusive_in_rust_and_json() {
    let sources: [PropertySource<Value>; 3] = [
        PropertySource::Constant(scalar(0.5)),
        PropertySource::Curve(CurveId::new()),
        PropertySource::Expression(ExpressionId::new()),
    ];
    for source in sources {
        let encoded = serde_json::to_string(&source).unwrap();
        assert_eq!(
            from_json::<PropertySource<Value>>(&encoded).unwrap(),
            source
        );
        let wire: JsonValue = serde_json::from_str(&encoded).unwrap();
        assert_eq!(wire.as_object().unwrap().len(), 2);
        assert!(wire.get("kind").is_some());
        assert!(wire.get("value").is_some());
    }
    let typed: PropertySource<bool> = PropertySource::Constant(true);
    assert_eq!(
        from_json::<PropertySource<bool>>(&serde_json::to_string(&typed).unwrap()).unwrap(),
        typed
    );
}

#[test]
fn json_rejects_competing_source_payloads_and_duplicate_tags() {
    let id = CurveId::new().to_string();
    for input in [
        json!({"kind":"constant", "value":{"kind":"scalar", "value":0.5}, "curve":id}).to_string(),
        json!({"kind":"curve", "value":id, "expression":ExpressionId::new()}).to_string(),
        json!({"constant":0.5, "curve":id}).to_string(),
        json!({"kind":"curve", "value":[id, ExpressionId::new().to_string()]}).to_string(),
        format!(r#"{{"kind":"constant","kind":"curve","value":"{id}"}}"#),
        r#"{"kind":"constant","value":true,"value":false}"#.into(),
        json!({"kind":"curve", "value":id, "constant":null}).to_string(),
    ] {
        assert_incompatible::<PropertySource<Value>>(&input);
    }
}

#[test]
fn source_switching_removes_residue_and_rejection_is_atomic() {
    let (registry, reference) = registry_with(opacity());
    let mut p = Property::new(
        PropertyId::new(),
        reference,
        PropertySource::Constant(scalar(0.5)),
        vec![],
        &registry,
    )
    .unwrap();
    let curve = CurveId::new();
    let expression = ExpressionId::new();
    p.set_source(PropertySource::Curve(curve), &registry)
        .unwrap();
    assert_eq!(
        serde_json::to_value(p.source()).unwrap(),
        json!({"kind":"curve", "value":curve})
    );
    p.set_source(PropertySource::Expression(expression), &registry)
        .unwrap();
    assert_eq!(
        serde_json::to_value(p.source()).unwrap(),
        json!({"kind":"expression", "value":expression})
    );
    p.set_source(PropertySource::Constant(scalar(0.75)), &registry)
        .unwrap();
    assert_eq!(
        serde_json::to_value(p.source()).unwrap(),
        json!({"kind":"constant", "value":{"kind":"scalar", "value":0.75}})
    );
    let before = p.clone();
    assert!(matches!(
        p.set_source(PropertySource::Constant(scalar(1.01)), &registry),
        Err(ModelError::OutOfRange { .. })
    ));
    assert_eq!(p, before);
    assert!(matches!(
        p.set_source(PropertySource::Constant(Value::Bool(false)), &registry),
        Err(ModelError::ValueTypeMismatch { .. })
    ));
    assert_eq!(p, before);
}

#[test]
fn source_switching_revalidates_existing_modifiers() {
    let d = descriptor("test.scalar");
    let mut restricted = d.definition().clone();
    restricted.capabilities.modifiers = false;
    let (registry, reference) = registry_with(d);
    let mut p = Property::new(
        PropertyId::new(),
        reference,
        PropertySource::Constant(scalar(0.5)),
        vec![modifier("test.effect")],
        &registry,
    )
    .unwrap();
    let (restricted_registry, _) = registry_with(PropertyDescriptor::new(restricted).unwrap());
    let before = p.clone();
    assert!(matches!(
        p.set_source(PropertySource::Curve(CurveId::new()), &restricted_registry),
        Err(ModelError::ModifiersNotAllowed)
    ));
    assert_eq!(p, before);
}

#[test]
fn adr0043_continuous_degrees_retain_zero_to_720_and_negative_turns() {
    let d = PropertyDescriptor::new(DescriptorDefinition::new(
        DescriptorId::new(),
        key("kronello.rotation"),
        "Rotation",
        ValueType::Angle,
        Unit::Degrees,
        Value::Angle(number(0.0)),
    ))
    .unwrap();
    let (registry, reference) = registry_with(d);
    let mut p = Property::new(
        PropertyId::new(),
        reference,
        PropertySource::Constant(Value::Angle(number(0.0))),
        vec![],
        &registry,
    )
    .unwrap();
    for angle in [720.0, -1080.0, 360.0, 0.0] {
        p.set_source(
            PropertySource::Constant(Value::Angle(number(angle))),
            &registry,
        )
        .unwrap();
        let encoded = serde_json::to_string(&p).unwrap();
        assert_eq!(
            Property::from_json(&encoded, &registry).unwrap().source(),
            &PropertySource::Constant(Value::Angle(number(angle)))
        );
    }
}

#[test]
fn adr0043_nonfinite_numbers_are_rejected_at_all_numeric_boundaries() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(matches!(
            FiniteF64::new(value),
            Err(ModelError::NonFinite { .. })
        ));
        assert!(matches!(
            Color::new(ColorSpace::LinearRec709, [value, 0.0, 0.0], 1.0),
            Err(ModelError::NonFinite { .. })
        ));
        assert!(matches!(
            Color::new(ColorSpace::Srgb, [0.0; 3], value),
            Err(ModelError::NonFinite { .. })
        ));
        assert!(matches!(
            NumericRange::inclusive(value, 1.0),
            Err(ModelError::NonFinite { .. })
        ));
    }
    for input in ["null", "\"NaN\"", "\"Infinity\""] {
        assert_incompatible::<FiniteF64>(input);
    }
    assert!(from_json::<FiniteF64>("1e999").is_err());
    for input in [
        r#"{"kind":"vec2","value":[0,null]}"#,
        r#"{"kind":"angle","value":"Infinity"}"#,
        r#"{"kind":"path","value":{"segments":[{"kind":"move_to","value":[0,"NaN"]}]}}"#,
    ] {
        assert_incompatible::<Value>(input);
    }
}

#[test]
fn adr0043_opacity_range_rejects_without_clamping() {
    let (registry, reference) = registry_with(opacity());
    for value in [-0.001, 1.001] {
        assert!(matches!(
            Property::new(
                PropertyId::new(),
                reference.clone(),
                PropertySource::Constant(scalar(value)),
                vec![],
                &registry
            ),
            Err(ModelError::OutOfRange { .. })
        ));
    }
    for value in [0.0, 1.0] {
        Property::new(
            PropertyId::new(),
            reference.clone(),
            PropertySource::Constant(scalar(value)),
            vec![],
            &registry,
        )
        .unwrap();
    }
}

#[test]
fn adr0043_ordered_modifiers_defer_range_until_final_value() {
    let (registry, reference) = registry_with(opacity());
    let modifiers = vec![modifier("kronello.scale"), modifier("kronello.clamp")];
    let p = Property::new(
        PropertyId::new(),
        reference,
        PropertySource::Constant(scalar(2.0)),
        modifiers.clone(),
        &registry,
    )
    .unwrap();
    assert_eq!(p.modifiers(), modifiers);
    let encoded = serde_json::to_string(&p).unwrap();
    assert_eq!(Property::from_json(&encoded, &registry).unwrap(), p);
    p.validate_final_value(&scalar(1.0), &registry).unwrap();
    assert!(matches!(
        p.validate_final_value(&scalar(2.0), &registry),
        Err(ModelError::OutOfRange { .. })
    ));
    let mut p = p;
    assert!(matches!(
        p.set_modifiers(vec![], &registry),
        Err(ModelError::OutOfRange { .. })
    ));
    assert_eq!(p.modifiers(), modifiers);
    let mut disabled = modifiers;
    for modifier in &mut disabled {
        modifier.enabled = false;
    }
    assert!(matches!(
        p.set_modifiers(disabled, &registry),
        Err(ModelError::OutOfRange { .. })
    ));
}

#[test]
fn adr0044_srgb_inputs_normalize_and_save_explicit_space() {
    let color = Color::from_srgb_hex("#F59E0B").unwrap();
    assert_eq!(color, Color::from_srgb8([245, 158, 11], None));
    assert_eq!(color.space(), ColorSpace::Srgb);
    let c = color.components();
    assert_eq!(
        [c.r.get(), c.g.get(), c.b.get(), c.alpha.get()],
        [245.0 / 255.0, 158.0 / 255.0, 11.0 / 255.0, 1.0]
    );
    let encoded = serde_json::to_string(&color).unwrap();
    assert_eq!(serde_json::to_value(color).unwrap()["space"], "srgb");
    assert_eq!(from_json::<Color>(&encoded).unwrap(), color);
    assert_eq!(
        Color::from_srgb_hex("#f59e0b80").unwrap(),
        Color::from_srgb8([245, 158, 11], Some(128))
    );
    for input in ["F59E0B", "#123", "#GG0000", "#12é34", "#1234567890"] {
        assert_eq!(
            Color::from_srgb_hex(input).unwrap_err(),
            ModelError::InvalidSrgbHex
        );
    }
}

#[test]
fn adr0044_colorspace_less_saved_json_is_rejected() {
    assert_incompatible::<Color>(r#"{"components":{"r":1,"g":0,"b":0}}"#);
    assert_incompatible::<Color>("\"#F59E0B\"");
    let c: Color = from_json(r#"{"space":"srgb","components":{"r":1,"g":0,"b":0}}"#).unwrap();
    assert_eq!(c.components().alpha.get(), 1.0);
    assert!(
        serde_json::to_value(c).unwrap()["components"]
            .get("alpha")
            .is_some()
    );
}

#[test]
fn adr0044_straight_rgb_and_alpha_are_independent() {
    for alpha in [0.0, 1e-20, 0.5, 1.0] {
        let c = Color::new(ColorSpace::Srgb, [0.8, 0.5, 0.2], alpha).unwrap();
        let round_trip: Color = from_json(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(round_trip.components().r.get(), 0.8);
        assert_eq!(round_trip.components().alpha.get(), alpha);
    }
    let c = Color::new(ColorSpace::LinearRec2020, [-0.25, 4.0, 0.5], 0.0).unwrap();
    assert_eq!(c.components().r.get(), -0.25);
    assert_eq!(c.components().g.get(), 4.0);
    for alpha in [-0.1, 1.1] {
        assert!(matches!(
            Color::new(ColorSpace::LinearRec709, [0.0; 3], alpha),
            Err(ModelError::OutOfRange { component: 3, .. })
        ));
    }
    for rgb in [[-0.1, 0.0, 0.0], [0.0, 1.1, 0.0]] {
        assert!(matches!(
            Color::new(ColorSpace::Srgb, rgb, 1.0),
            Err(ModelError::OutOfRange { .. })
        ));
    }
    assert_incompatible::<Color>(r#"{"space":"srgb","components":{"r":1.1,"g":0,"b":0}}"#);
}

#[test]
fn adr0044_color_interpolation_declares_linear_straight_contract() {
    let mut d = DescriptorDefinition::new(
        DescriptorId::new(),
        key("test.color"),
        "Color",
        ValueType::Color,
        Unit::Dimensionless,
        Value::Color(Color::from_srgb8([128, 0, 255], Some(0))),
    );
    assert_eq!(d.color_interpolation_space, None);
    for space in [
        None,
        Some(ColorSpace::LinearRec709),
        Some(ColorSpace::LinearRec2020),
    ] {
        d.color_interpolation_space = space;
        let descriptor = PropertyDescriptor::new(d.clone()).unwrap();
        assert_eq!(
            PropertyDescriptor::from_json(&serde_json::to_string(&descriptor).unwrap()).unwrap(),
            descriptor
        );
    }
    d.color_interpolation_space = Some(ColorSpace::Srgb);
    assert!(matches!(
        PropertyDescriptor::new(d),
        Err(ModelError::InvalidColorInterpolationSpace)
    ));
    let mut d = definition("test.scalar");
    d.color_interpolation_space = Some(ColorSpace::LinearRec709);
    assert!(matches!(
        PropertyDescriptor::new(d),
        Err(ModelError::InvalidColorInterpolationSpace)
    ));
}

#[test]
fn registry_enumeration_is_deterministic_and_duplicates_are_atomic() {
    let a = descriptor("test.a");
    let b = descriptor("test.b");
    let mut first = SchemaRegistry::new();
    let mut second = SchemaRegistry::new();
    assert!(first.is_empty());
    first.register(b.clone()).unwrap();
    first.register(a.clone()).unwrap();
    second.register(a.clone()).unwrap();
    second.register(b.clone()).unwrap();
    assert_eq!(
        first.iter().collect::<Vec<_>>(),
        second.iter().collect::<Vec<_>>()
    );
    assert_eq!(
        first
            .iter()
            .map(|(key, _)| key.as_str())
            .collect::<Vec<_>>(),
        ["test.a", "test.b"]
    );
    let mut duplicate_key = definition("test.a");
    duplicate_key.default = scalar(0.8);
    assert!(matches!(
        first.register(PropertyDescriptor::new(duplicate_key).unwrap()),
        Err(ModelError::DuplicateSchemaKey { .. })
    ));
    let mut duplicate_id = definition("test.c");
    duplicate_id.id = a.id();
    assert!(matches!(
        first.register(PropertyDescriptor::new(duplicate_id).unwrap()),
        Err(ModelError::DuplicateDescriptorId)
    ));
    assert_eq!(first.len(), 2);
    assert_eq!(first.lookup(&key("test.a")).unwrap(), &a);
    assert!(matches!(
        first.lookup(&key("test.missing")),
        Err(ModelError::DescriptorNotFound { .. })
    ));
}

#[test]
fn property_validates_descriptor_references_and_versions() {
    let (registry, reference) = registry_with(opacity());
    let missing = DescriptorRef {
        key: key("test.missing"),
        version: 1,
    };
    assert!(matches!(
        Property::new(
            PropertyId::new(),
            missing,
            PropertySource::Constant(scalar(0.5)),
            vec![],
            &registry
        ),
        Err(ModelError::DescriptorNotFound { .. })
    ));
    let mismatch = DescriptorRef {
        version: 2,
        ..reference
    };
    assert!(matches!(
        Property::new(
            PropertyId::new(),
            mismatch,
            PropertySource::Constant(scalar(0.5)),
            vec![],
            &registry
        ),
        Err(ModelError::DescriptorVersionMismatch { .. })
    ));
}

#[test]
fn property_capabilities_and_modifier_identity_are_checked() {
    let mut d = definition("test.scalar");
    d.capabilities = Capabilities {
        curves: false,
        expressions: false,
        modifiers: false,
    };
    let (registry, reference) = registry_with(PropertyDescriptor::new(d).unwrap());
    for source in [
        PropertySource::Curve(CurveId::new()),
        PropertySource::Expression(ExpressionId::new()),
    ] {
        assert!(matches!(
            Property::new(
                PropertyId::new(),
                reference.clone(),
                source,
                vec![],
                &registry
            ),
            Err(ModelError::SourceNotAllowed)
        ));
    }
    assert!(matches!(
        Property::new(
            PropertyId::new(),
            reference,
            PropertySource::Constant(scalar(0.5)),
            vec![modifier("test.effect")],
            &registry
        ),
        Err(ModelError::ModifiersNotAllowed)
    ));
    let (registry, reference) = registry_with(opacity());
    let m = modifier("test.effect");
    assert!(matches!(
        Property::new(
            PropertyId::new(),
            reference.clone(),
            PropertySource::Constant(scalar(0.5)),
            vec![m.clone(), m],
            &registry
        ),
        Err(ModelError::DuplicateModifierId)
    ));
    let mut m = modifier("test.effect");
    m.version = 0;
    assert!(matches!(
        Property::new(
            PropertyId::new(),
            reference,
            PropertySource::Constant(scalar(0.5)),
            vec![m],
            &registry
        ),
        Err(ModelError::InvalidModifierVersion)
    ));
}

struct Catalog {
    curve: CurveId,
    expression: ExpressionId,
    value_type: ValueType,
}
impl SourceResolver for Catalog {
    fn curve_value_type(&self, id: CurveId) -> Option<ValueType> {
        (id == self.curve).then_some(self.value_type)
    }
    fn expression_value_type(&self, id: ExpressionId) -> Option<ValueType> {
        (id == self.expression).then_some(self.value_type)
    }
}
#[test]
fn curve_and_expression_references_validate_type_and_existence() {
    let (registry, reference) = registry_with(opacity());
    let mut catalog = Catalog {
        curve: CurveId::new(),
        expression: ExpressionId::new(),
        value_type: ValueType::Scalar,
    };
    let mut p = Property::new(
        PropertyId::new(),
        reference,
        PropertySource::Curve(catalog.curve),
        vec![],
        &registry,
    )
    .unwrap();
    p.validate_sources(&registry, &catalog).unwrap();
    catalog.value_type = ValueType::Bool;
    assert!(matches!(
        p.validate_sources(&registry, &catalog),
        Err(ModelError::ValueTypeMismatch { .. })
    ));
    p.set_source(PropertySource::Curve(CurveId::new()), &registry)
        .unwrap();
    assert!(matches!(
        p.validate_sources(&registry, &catalog),
        Err(ModelError::CurveNotFound { .. })
    ));
    p.set_source(PropertySource::Expression(catalog.expression), &registry)
        .unwrap();
    assert!(matches!(
        p.validate_sources(&registry, &catalog),
        Err(ModelError::ValueTypeMismatch { .. })
    ));
    catalog.value_type = ValueType::Scalar;
    p.validate_sources(&registry, &catalog).unwrap();
    p.set_source(PropertySource::Expression(ExpressionId::new()), &registry)
        .unwrap();
    assert!(matches!(
        p.validate_sources(&registry, &catalog),
        Err(ModelError::ExpressionNotFound { .. })
    ));
}

#[test]
fn adr0045_unknown_fields_and_enum_values_are_typed_failures() {
    let mut d = serde_json::to_value(opacity()).unwrap();
    d["future_field"] = json!({"meaning":"must not disappear"});
    assert!(matches!(
        PropertyDescriptor::from_json(&d.to_string()),
        Err(JsonError::IncompatibleStructure { .. })
    ));
    assert_incompatible::<PropertyDescriptor>(&d.to_string());
    for (field, value) in [
        ("value_type", json!("transform3d")),
        ("unit", json!("radians")),
        ("interpolation_modes", json!(["hold", "future"])),
    ] {
        let mut d = serde_json::to_value(opacity()).unwrap();
        d[field] = value;
        assert_incompatible::<PropertyDescriptor>(&d.to_string());
    }
    assert_incompatible::<Color>(
        r#"{"space":"srgb","components":{"r":0,"g":0,"b":0,"future":true}}"#,
    );
    assert_incompatible::<Color>(
        r#"{"space":"srgb","components":{"r":0,"g":0,"b":0},"future":true}"#,
    );
    assert_incompatible::<Color>(r#"{"space":"future","components":{"r":0,"g":0,"b":0}}"#);
    assert_incompatible::<Value>(r#"{"kind":"scalar","value":0,"future":1}"#);
    assert_incompatible::<PathSegment>(
        r#"{"kind":"cubic_to","value":{"control1":[0,0],"control2":[1,1],"end":[2,2],"future":true}}"#,
    );
    assert_incompatible::<NumericRange>(r#"{"min":null,"max":null,"future":true}"#);
}

#[test]
fn adr0045_property_unknown_fields_do_not_produce_partial_success() {
    let (registry, reference) = registry_with(opacity());
    let p = Property::new(
        PropertyId::new(),
        reference,
        PropertySource::Constant(scalar(0.5)),
        vec![modifier("test.clamp")],
        &registry,
    )
    .unwrap();
    for location in [
        "root",
        "descriptor",
        "source",
        "modifier",
        "capabilities",
        "range",
    ] {
        let mut wire = serde_json::to_value(&p).unwrap();
        match location {
            "root" => wire["future"] = json!(1),
            "descriptor" => wire["descriptor"]["future"] = json!(1),
            "source" => wire["source"]["future"] = json!(1),
            "modifier" => wire["modifiers"][0]["future"] = json!(1),
            "capabilities" | "range" => {
                let mut d = serde_json::to_value(opacity()).unwrap();
                d[location]["future"] = json!(1);
                assert_incompatible::<PropertyDescriptor>(&d.to_string());
                continue;
            }
            _ => unreachable!(),
        }
        let original = wire.to_string();
        assert!(matches!(
            Property::from_json(&original, &registry),
            Err(JsonError::IncompatibleStructure { .. })
        ));
        assert_eq!(serde_json::from_str::<JsonValue>(&original).unwrap(), wire);
    }
}

#[test]
fn json_syntax_errors_are_distinct_from_structure_errors() {
    assert!(matches!(
        from_json::<PropertyDescriptor>("{"),
        Err(JsonError::InvalidJson { .. })
    ));
    assert!(matches!(
        from_json::<PropertyDescriptor>("null"),
        Err(JsonError::IncompatibleStructure { .. })
    ));
}

#[test]
fn all_value_variants_round_trip_without_backend_types() {
    for value in [
        scalar(-1e300),
        Value::Vec2([number(1.0), number(2.0)]),
        Value::Vec3([number(1.0), number(2.0), number(3.0)]),
        Value::Angle(number(720.0)),
        Value::Color(Color::from_srgb8([1, 2, 3], None)),
        Value::Bool(true),
        Value::Enum("choice".into()),
        Value::String("字幕 is data".into()),
        Value::AssetRef(AssetId::new()),
        Value::Path(Path {
            segments: vec![
                PathSegment::MoveTo([number(0.0); 2]),
                PathSegment::LineTo([number(1.0); 2]),
                PathSegment::CubicTo {
                    control1: [number(1.0); 2],
                    control2: [number(2.0); 2],
                    end: [number(3.0); 2],
                },
                PathSegment::Close,
            ],
        }),
    ] {
        let round_trip: Value = from_json(&serde_json::to_string(&value).unwrap()).unwrap();
        assert_eq!(round_trip.value_type(), value.value_type());
        assert_eq!(round_trip, value);
    }
}

#[test]
fn finite_json_round_trip_retains_extremes_subnormals_and_sampled_bits() {
    let assert_round_trip = |value: f64| {
        let finite = number(value);
        let decoded: FiniteF64 = from_json(&serde_json::to_string(&finite).unwrap()).unwrap();
        assert_eq!(decoded.get().to_bits(), value.to_bits());
    };
    for value in [
        0.0,
        -0.0,
        f64::MAX,
        f64::MIN,
        f64::MIN_POSITIVE,
        f64::from_bits(1),
        -f64::from_bits(1),
        3.0 / 255.0,
    ] {
        assert_round_trip(value);
    }
    // Fixed deterministic sample exercises exponents/mantissas without adding
    // runtime randomness or an implementation-mirroring oracle.
    let mut bits = 0x8c14_5089_47ab_0061_u64;
    for _ in 0..4096 {
        bits = bits.wrapping_mul(6364136223846793005).wrapping_add(1);
        let value = f64::from_bits(bits);
        if value.is_finite() {
            assert_round_trip(value);
        }
    }
}

#[test]
fn all_srgb8_components_round_trip_without_applying_transfer_function() {
    for component in 0..=255_u8 {
        let color = Color::from_srgb8([component; 3], Some(component));
        let decoded: Color = from_json(&serde_json::to_string(&color).unwrap()).unwrap();
        assert_eq!(decoded, color);
        let c = decoded.components();
        assert_eq!(c.r.get(), f64::from(component) / 255.0);
        assert_eq!(c.r, c.g);
        assert_eq!(c.r, c.b);
        assert_eq!(c.r, c.alpha);
    }
}
