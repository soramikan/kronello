//! COLOR-003 `.cube` LUT effect contracts (ADR-0113): the `lut` parameter is
//! an `asset_ref` to a Data asset, `intensity` is a bounded scalar, and the
//! resolved value carries the asset id only — never lattice bytes.
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
fn asset() -> Value {
    Value::AssetRef(AssetId::from_uuid(uuid::Uuid::new_v4()))
}
fn definition(
    r: &SchemaRegistry,
    parameters: &[(&str, Value)],
) -> (EffectDefinition, Vec<Property>, Vec<PropertyId>) {
    let properties = parameters
        .iter()
        .map(|(key, value)| prop(r, key, value.clone()))
        .collect::<Vec<_>>();
    let ids = properties.iter().map(|p| p.id()).collect::<Vec<_>>();
    (
        EffectDefinition {
            effect_id: COLOR_LUT_ID.into(),
            version: 1,
            parameters: EffectParameters::ColorLut {
                lut: ids[0],
                intensity: ids[1],
            },
        },
        properties,
        ids,
    )
}
fn values(ids: &[PropertyId], lut: &Value, intensity: Value) -> BTreeMap<PropertyId, Value> {
    BTreeMap::from([(ids[0], lut.clone()), (ids[1], intensity)])
}

#[test]
fn color003_lut_resolves_asset_reference_and_bounded_intensity() {
    let r = registry();
    let (d, properties, ids) = definition(
        &r,
        &[
            ("kronello.effect.lut", asset()),
            ("kronello.effect.intensity", scalar(1.0)),
        ],
    );
    d.validate(&properties, &r).unwrap();
    let reference = asset();
    let Value::AssetRef(id) = reference else {
        panic!()
    };
    assert_eq!(
        d.resolve(&values(&ids, &reference, scalar(0.25))).unwrap(),
        ResolvedEffect::ColorLut {
            lut: id,
            intensity: 0.25
        }
    );
    // intensity is the documented 0..=1 identity blend; outside is rejected.
    // (Non-finite scalars cannot be built: FiniteF64 rejects them at the type
    // level, so only in-range numerics reach the bound check.)
    for intensity in [-0.01, 1.01] {
        assert!(matches!(
            d.resolve(&values(&ids, &reference, scalar(intensity))),
            Err(EffectError::InvalidParameter(bad)) if bad == ids[1]
        ));
    }
    // The lut parameter requires an asset_ref value, not a scalar or string.
    for wrong in [scalar(0.5), Value::String("lut.cube".into())] {
        assert!(matches!(
            d.resolve(&values(&ids, &wrong, scalar(1.0))),
            Err(EffectError::InvalidParameter(bad)) if bad == ids[0]
        ));
    }
    assert!(d.resolve(&BTreeMap::new()).is_err());
}

#[test]
fn color003_lut_descriptor_types_and_versions_are_pinned() {
    let r = registry();
    let lut = r
        .lookup(&SchemaKey::new("kronello.effect.lut").unwrap())
        .unwrap();
    assert_eq!(lut.definition().value_type, ValueType::AssetRef);
    assert_eq!(lut.definition().unit, Unit::Dimensionless);
    let intensity = r
        .lookup(&SchemaKey::new("kronello.effect.intensity").unwrap())
        .unwrap();
    assert_eq!(intensity.definition().default, scalar(1.0));
    // `intensity` is a shared nonnegative-scalar descriptor (glow accepts
    // values above 1); the LUT-specific 0..=1 blend bound is enforced by
    // ColorLut resolution in the test above.
    assert!(intensity.validate_value(&scalar(-0.5)).is_err());
    assert!(intensity.validate_value(&scalar(0.5)).is_ok());
    let (d, properties, _) = definition(
        &r,
        &[
            ("kronello.effect.lut", asset()),
            ("kronello.effect.intensity", scalar(1.0)),
        ],
    );
    for (id, version) in [(COLOR_LUT_ID, 0), (COLOR_LUT_ID, 2), ("vendor.lut", 1)] {
        let mut unsupported = d.clone();
        unsupported.effect_id = id.into();
        unsupported.version = version;
        assert!(matches!(
            unsupported.validate(&properties, &r),
            Err(EffectError::UnsupportedFeature)
        ));
    }
    // Referencing a scalar-typed property as the lut parameter is rejected by
    // descriptor type checking, before any value is read.
    let wrong = prop(&r, "kronello.effect.exposure", scalar(0.0));
    let mismatched = EffectDefinition {
        parameters: EffectParameters::ColorLut {
            lut: wrong.id(),
            intensity: wrong.id(),
        },
        ..d.clone()
    };
    assert!(matches!(
        mismatched.validate(&[wrong], &r),
        Err(EffectError::InvalidParameter(_))
    ));
    // The kind tag and effect id join the versioned schema.
    let schema = serde_json::to_value(project_json_schema()).unwrap();
    assert!(
        serde_json::to_string(&schema["$defs"]["EffectParameters"])
            .unwrap()
            .contains("color_lut")
    );
}
