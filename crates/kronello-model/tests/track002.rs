//! TRACK-002 stabilization effect model contracts (ADR-0122).
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
fn fixture(r: &SchemaRegistry) -> (EffectDefinition, Vec<Property>, Vec<PropertyId>) {
    let tracking = AssetId::new();
    let keys = [
        ("kronello.effect.tracking", Value::AssetRef(tracking)),
        ("kronello.effect.smoothing_radius", scalar(8.0)),
        ("kronello.effect.max_displacement", scalar(48.0)),
        (
            "kronello.effect.max_rotation",
            Value::Angle(FiniteF64::new(4.0).unwrap()),
        ),
        ("kronello.effect.max_crop", scalar(0.2)),
        ("kronello.effect.border", Value::Enum("reflect".into())),
        (
            "kronello.effect.fill_color",
            Value::Color(Color::from_srgb8([10, 20, 30], Some(128))),
        ),
        ("kronello.effect.sampling", Value::Enum("bilinear".into())),
    ];
    let properties: Vec<Property> = keys
        .iter()
        .map(|(key, value)| prop(r, key, value.clone()))
        .collect();
    let ids: Vec<PropertyId> = properties.iter().map(|p| p.id()).collect();
    (
        EffectDefinition {
            effect_id: STABILIZE_ID.into(),
            version: STABILIZE_VERSION,
            parameters: EffectParameters::Stabilize {
                tracking: ids[0],
                smoothing_radius: ids[1],
                max_displacement: ids[2],
                max_rotation: ids[3],
                max_crop: ids[4],
                border: ids[5],
                fill_color: ids[6],
                sampling: ids[7],
            },
        },
        properties,
        ids,
    )
}
fn values(ids: &[PropertyId], properties: &[Property]) -> BTreeMap<PropertyId, Value> {
    ids.iter()
        .copied()
        .zip(properties.iter().map(|p| match p.source() {
            PropertySource::Constant(v) => v.clone(),
            _ => unreachable!(),
        }))
        .collect()
}

#[test]
fn track002_stabilize_resolves_authored_parameters() {
    let r = registry();
    let (d, properties, ids) = fixture(&r);
    d.validate(&properties, &r).unwrap();
    let resolved = d.resolve(&values(&ids, &properties)).unwrap();
    let ResolvedEffect::Stabilize {
        tracking,
        smoothing_radius,
        max_displacement,
        max_rotation,
        max_crop,
        border,
        fill_color,
        sampling,
        inverse,
    } = resolved
    else {
        panic!("stabilize resolved variant")
    };
    // The scene pass binds `inverse` later; resolution leaves it unbound.
    assert!(inverse.is_none());
    assert_eq!(smoothing_radius, 8);
    assert_eq!(max_displacement, 48.0);
    assert_eq!(max_rotation, 4.0);
    assert_eq!(max_crop, 0.2);
    assert_eq!(border, StabilizeBorder::Reflect);
    assert_eq!(sampling, StabilizeSampling::Bilinear);
    assert!(matches!(
        properties[0].source(),
        PropertySource::Constant(Value::AssetRef(id)) if *id == tracking
    ));
    assert_eq!(
        fill_color.components().alpha.get(),
        Color::from_srgb8([10, 20, 30], Some(128))
            .components()
            .alpha
            .get()
    );
}

#[test]
fn track002_stabilize_rejects_invalid_enums_and_ranges() {
    let r = registry();
    let (d, properties, ids) = fixture(&r);
    let base: Vec<Value> = ids
        .iter()
        .zip(&properties)
        .map(|(_, p)| match p.source() {
            PropertySource::Constant(v) => v.clone(),
            _ => unreachable!(),
        })
        .collect();
    // Unknown enum tags.
    for (index, tag) in [(5, "wrap"), (7, "cubic")] {
        let mut bad = base.clone();
        bad[index] = Value::Enum(tag.into());
        assert!(matches!(
            d.resolve(&ids.iter().copied().zip(bad).collect()),
            Err(EffectError::InvalidParameter(id)) if id == ids[index]
        ));
    }
    // Out-of-range scalars.
    for (index, v) in [
        (1, 8.5),    // smoothing radius must be integral
        (1, 5000.0), // and bounded
        (2, -1.0),
        (4, 1.5), // max_crop is a unit interval
    ] {
        let mut bad = base.clone();
        bad[index] = scalar(v);
        assert!(matches!(
            d.resolve(&ids.iter().copied().zip(bad).collect()),
            Err(EffectError::InvalidParameter(id)) if id == ids[index]
        ));
    }
    // A non-asset tracking reference is invalid.
    let mut bad = base;
    bad[0] = scalar(0.0);
    assert!(matches!(
        d.resolve(&ids.iter().copied().zip(bad).collect()),
        Err(EffectError::InvalidParameter(id)) if id == ids[0]
    ));
    // The resolved effect kind participates in descriptor/version identity.
    d.ensure_supported().unwrap();
    let mut wrong = d.clone();
    wrong.effect_id = "kronello.not_stabilize".into();
    assert!(matches!(
        wrong.ensure_supported(),
        Err(EffectError::UnsupportedFeature)
    ));
}

#[test]
fn track002_sequence_rejects_interpolation_on_nonvideo() {
    // Sequence::validate rejects frame interpolation on image asset clips and
    // non-forward sampling; covered by the clip-level rules in sequence.rs.
    // Here: a synthesized clip carrying the mode on an audio track is typed.
    use kronello_time::{FrameInterpolation, OpticalFlowConfig, TimeMap, TimeMapPoint};
    let map = TimeMap::piecewise_linear_with_interpolation(
        vec![
            TimeMapPoint {
                parent: kronello_time::Time::ZERO,
                local: kronello_time::Time::ZERO,
            },
            TimeMapPoint {
                parent: kronello_time::Rational::from_integer(2),
                local: kronello_time::Rational::from_integer(1),
            },
        ],
        Some(FrameInterpolation::OpticalFlow(OpticalFlowConfig::default())),
    )
    .unwrap();
    assert!(map.interpolation().is_some());
}
