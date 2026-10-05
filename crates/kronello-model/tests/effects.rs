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
