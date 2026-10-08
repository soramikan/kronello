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
#[test]
fn effect_parameters_validate_ownership_types_units_and_final_ranges() {
    let r = registry();
    let p = prop(
        &r,
        "kronello.effect.sigma",
        Value::Scalar(FiniteF64::new(1.0).unwrap()),
    );
    let d = EffectDefinition {
        effect_id: GAUSSIAN_BLUR_ID.into(),
        version: 1,
        parameters: EffectParameters::GaussianBlur { sigma: p.id() },
    };
    assert!(d.validate(std::slice::from_ref(&p), &r).is_ok());
    assert!(d.validate(&[], &r).is_err());
    for key in ["kronello.opacity", "kronello.transform.rotation"] {
        let value = if key.ends_with("rotation") {
            Value::Angle(FiniteF64::new(1.0).unwrap())
        } else {
            Value::Scalar(FiniteF64::new(1.0).unwrap())
        };
        let wrong = prop(&r, key, value);
        let d = EffectDefinition {
            parameters: EffectParameters::GaussianBlur { sigma: wrong.id() },
            ..d.clone()
        };
        assert!(d.validate(&[wrong], &r).is_err());
    }
    assert!(
        d.resolve(&BTreeMap::from([(
            p.id(),
            Value::Scalar(FiniteF64::new(-1.0).unwrap())
        )]))
        .is_err()
    );
    assert!(d.resolve(&BTreeMap::new()).is_err());
    assert!(kronello_model::from_json::<EffectDefinition>(&format!("{{\"effect_id\":\"{}\",\"version\":1,\"parameters\":{{\"kind\":\"gaussian_blur\",\"sigma\":\"{}\"}}}}",GAUSSIAN_BLUR_ID,p.id())).is_ok());
}
#[test]
fn unknown_effect_keeps_arbitrary_precision_and_unknown_fields() {
    let raw = r#"{"effect_id":"vendor.future","version":1,"parameters":{"kind":"future","precise":1234567890123456789012345678901234567890},"new_field":true}"#;
    let e: Effect = serde_json::from_str(raw).unwrap();
    assert!(matches!(e, Effect::Opaque(_)));
    assert_eq!(
        serde_json::to_value(&e).unwrap(),
        serde_json::from_str::<serde_json::Value>(raw).unwrap()
    );
    assert!(matches!(
        e.definition(),
        Err(EffectError::UnsupportedFeature)
    ));
}
#[test]
fn effect_schema_describes_known_parameters_and_opaque_preservation() {
    let schema = serde_json::to_value(project_json_schema()).unwrap();
    assert!(schema["$defs"]["SceneNode"]["properties"]["effects"].is_object());
    assert!(schema["$defs"]["EffectDefinition"]["properties"]["effect_id"].is_object());
    assert!(schema["$defs"]["EffectParameters"]["oneOf"].is_array());
}

#[test]
fn fx002_versions_are_explicit_and_legacy_resolution_is_unchanged() {
    let sigma = PropertyId::new();
    let mut definition = EffectDefinition {
        effect_id: GAUSSIAN_BLUR_ID.into(),
        version: 1,
        parameters: EffectParameters::GaussianBlur { sigma },
    };
    let values = BTreeMap::from([(sigma, Value::Scalar(FiniteF64::new(1.0).unwrap()))]);
    assert_eq!(
        definition.resolve(&values).unwrap(),
        ResolvedEffect::GaussianBlur { sigma: 1.0 }
    );
    definition.version = 2;
    assert_eq!(
        definition.resolve(&values).unwrap(),
        ResolvedEffect::AffineGaussianBlur {
            sigma: 1.0,
            linear: [[1.0, 0.0], [0.0, 1.0]]
        }
    );
    definition.version = 3;
    assert!(matches!(
        definition.resolve(&values),
        Err(EffectError::UnsupportedFeature)
    ));
}
/// AUDIO-011 (ADR-0131): the `kronello.audio.plugin` binding validates as a
/// document effect and always resolves to typed errors — execution belongs to
/// the detached plugin worker, never the model/evaluator.
fn plugin_properties(r: &SchemaRegistry) -> Vec<Property> {
    let table = Value::DataTable(DataTable {
        columns: BTreeMap::from([
            ("param".to_string(), ValueType::Scalar),
            ("value".to_string(), ValueType::Scalar),
        ]),
        rows: vec![BTreeMap::from([
            (
                "param".to_string(),
                Value::Scalar(FiniteF64::new(0.0).unwrap()),
            ),
            (
                "value".to_string(),
                Value::Scalar(FiniteF64::new(0.5).unwrap()),
            ),
        ])],
    });
    vec![
        prop(
            r,
            "kronello.effect.plugin_bundle",
            Value::String("/plugins/Acme.vst3".into()),
        ),
        prop(
            r,
            "kronello.effect.plugin_format",
            Value::Enum("vst3".into()),
        ),
        prop(
            r,
            "kronello.effect.plugin_component",
            Value::String("6b726f6e656c6c6f746573746761696e".into()),
        ),
        prop(
            r,
            "kronello.effect.plugin_sha256",
            Value::String("a".repeat(64)),
        ),
        prop(
            r,
            "kronello.effect.plugin_version",
            Value::String("1.0.0".into()),
        ),
        prop(r, "kronello.effect.plugin_parameters", table),
    ]
}
fn plugin_definition(properties: &[Property]) -> EffectDefinition {
    EffectDefinition {
        effect_id: AUDIO_PLUGIN_ID.into(),
        version: AUDIO_PLUGIN_VERSION,
        parameters: EffectParameters::AudioPlugin {
            bundle: properties[0].id(),
            format: properties[1].id(),
            component: properties[2].id(),
            sha256: properties[3].id(),
            plugin_version: properties[4].id(),
            parameters: properties[5].id(),
        },
    }
}
fn plugin_values(properties: &[Property]) -> BTreeMap<PropertyId, Value> {
    properties
        .iter()
        .map(|p| {
            let PropertySource::Constant(value) = p.source() else {
                panic!()
            };
            (p.id(), value.clone())
        })
        .collect()
}
#[test]
fn audio_plugin_binding_validates_then_resolves_to_typed_errors() {
    let r = registry();
    let properties = plugin_properties(&r);
    let definition = plugin_definition(&properties);
    definition.validate(&properties, &r).unwrap();
    assert!(definition.validate(&properties[..5], &r).is_err());
    assert!(matches!(
        definition.resolve(&plugin_values(&properties)),
        Err(EffectError::UnsupportedFeature)
    ));
    assert!(matches!(
        definition.resolve_audio(&plugin_values(&properties)),
        Err(EffectError::UnsupportedFeature)
    ));
    // Wire roundtrip keeps the authored binding shape.
    let json = serde_json::to_value(&definition).unwrap();
    assert_eq!(json["effect_id"], "kronello.audio.plugin");
    assert_eq!(json["parameters"]["kind"], "audio_plugin");
    let parsed: EffectDefinition = serde_json::from_value(json).unwrap();
    assert_eq!(parsed, definition);
    // The six binding descriptors live in the reserved 49xx range.
    for key in [
        "plugin_bundle",
        "plugin_format",
        "plugin_component",
        "plugin_sha256",
        "plugin_version",
        "plugin_parameters",
    ] {
        let d = r
            .lookup(&SchemaKey::new(format!("kronello.effect.{key}")).unwrap())
            .unwrap();
        let digits = d.id().as_uuid().as_u128();
        assert_eq!((digits >> 64) & 0xff00, 0x4900, "{key} descriptor range");
    }
}
#[test]
fn audio_plugin_binding_rejects_malformed_values_typed() {
    let r = registry();
    let properties = plugin_properties(&r);
    let definition = plugin_definition(&properties);
    let mut values = plugin_values(&properties);
    let set = |values: &mut BTreeMap<PropertyId, Value>, index: usize, value: Value| {
        values.insert(properties[index].id(), value);
    };
    // Bad hash: not 64 lowercase hex.
    {
        let mut v = values.clone();
        set(&mut v, 3, Value::String("not-hex".into()));
        assert!(matches!(
            definition.resolve_audio(&v),
            Err(EffectError::InvalidParameter(id)) if id == properties[3].id()
        ));
    }
    // Unknown format enum.
    {
        let mut v = values.clone();
        set(&mut v, 1, Value::Enum("ladspa".into()));
        assert!(matches!(
            definition.resolve_audio(&v),
            Err(EffectError::InvalidParameter(id)) if id == properties[1].id()
        ));
    }
    // VST3 component ids are 32 hex chars.
    {
        let mut v = values.clone();
        set(&mut v, 2, Value::String("xyz".into()));
        assert!(matches!(
            definition.resolve_audio(&v),
            Err(EffectError::InvalidParameter(id)) if id == properties[2].id()
        ));
    }
    // VST3 normalized parameter values live in 0..=1.
    {
        let mut v = values.clone();
        set(
            &mut v,
            5,
            Value::DataTable(DataTable {
                columns: BTreeMap::from([
                    ("param".to_string(), ValueType::Scalar),
                    ("value".to_string(), ValueType::Scalar),
                ]),
                rows: vec![BTreeMap::from([
                    (
                        "param".to_string(),
                        Value::Scalar(FiniteF64::new(0.0).unwrap()),
                    ),
                    (
                        "value".to_string(),
                        Value::Scalar(FiniteF64::new(1.5).unwrap()),
                    ),
                ])],
            }),
        );
        assert!(matches!(
            definition.resolve_audio(&v),
            Err(EffectError::InvalidParameter(id)) if id == properties[5].id()
        ));
    }
    // VST3 requires a bundle path.
    {
        let mut v = values.clone();
        set(&mut v, 0, Value::String(String::new()));
        assert!(matches!(
            definition.resolve_audio(&v),
            Err(EffectError::InvalidParameter(id)) if id == properties[0].id()
        ));
    }
    // A wrong value type is rejected by the typed reference contract.
    values.insert(
        properties[0].id(),
        Value::Scalar(FiniteF64::new(1.0).unwrap()),
    );
    assert!(matches!(
        definition.resolve_audio(&values),
        Err(EffectError::InvalidParameter(id)) if id == properties[0].id()
    ));
}
