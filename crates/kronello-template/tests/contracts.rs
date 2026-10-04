use kronello_model::*;
use kronello_template::*;
use kronello_time::{Duration, Time};
use std::collections::BTreeMap;
fn t(n: i64, d: i64) -> Time {
    Time::new(n, d).unwrap()
}

#[test]
fn protected_hold_loop_stretch_have_exact_boundaries_and_no_history() {
    let (_, mut d) = fixture();
    for mode in [
        TemplateMiddleMode::Hold,
        TemplateMiddleMode::Loop,
        TemplateMiddleMode::Stretch,
    ] {
        d.duration_policy.middle_mode = mode;
        let map = duration_map(duration(5, 1), duration(10, 1), &d.duration_policy).unwrap();
        let encoded = serde_json::to_string(&map).unwrap();
        assert_eq!(
            serde_json::from_str::<kronello_time::TimeMap>(&encoded).unwrap(),
            map
        );
        for time in [Time::ZERO, t(1, 5), t(2, 5)] {
            assert_eq!(map.map(time).unwrap(), time);
        }
        for elapsed in [Time::ZERO, t(1, 10), t(3, 10)] {
            assert_eq!(
                map.map(t(97, 10).checked_add(elapsed).unwrap()).unwrap(),
                t(47, 10).checked_add(elapsed).unwrap()
            );
        }
        let samples = [t(47, 10), t(1, 1), t(9, 1), t(46, 10)];
        let forward: Vec<_> = samples.iter().map(|time| map.map(*time).unwrap()).collect();
        for (time, expected) in samples.iter().zip(forward).rev() {
            assert_eq!(map.map(*time).unwrap(), expected);
        }
        match mode {
            TemplateMiddleMode::Hold => assert_eq!(map.map(t(9, 1)).unwrap(), t(2, 5)),
            TemplateMiddleMode::Loop => {
                assert_eq!(map.map(t(46, 10)).unwrap(), t(46, 10));
                assert_eq!(map.map(t(47, 10)).unwrap(), t(2, 5));
                assert_eq!(map.map(t(9, 1)).unwrap(), t(2, 5));
                assert_eq!(map.map(t(95, 10)).unwrap(), t(9, 10));
            }
            TemplateMiddleMode::Stretch => assert_eq!(map.map(t(101, 20)).unwrap(), t(51, 20)),
        }
        for requested in [duration(1, 1), duration(119, 100), duration(7, 10)] {
            assert_eq!(
                duration_map(duration(5, 1), requested, &d.duration_policy)
                    .unwrap_err()
                    .code(),
                "DURATION_TOO_SHORT"
            );
        }
        assert!(duration_map(duration(5, 1), duration(6, 5), &d.duration_policy).is_ok());
        assert!(map.map(t(-1, 10)).is_err());
        assert!(map.map(t(101, 10)).is_err());
    }
    assert!(serde_json::from_value::<kronello_time::TimeMap>(serde_json::json!({
        "kind":"protected", "authoring":{"num":"0","den":"1"}, "requested":{"num":"1","den":"1"},
        "intro":{"num":"0","den":"1"}, "outro":{"num":"0","den":"1"}, "mode":"loop"
    })).is_err());
}

#[test]
fn table_schema_projections_variants_and_authoring_pins_are_explicit() {
    let mut p: Project =
        serde_json::from_str(include_str!("../../../examples/template-002.project.json")).unwrap();
    let mut d: TemplateDefinition = serde_json::from_str(include_str!(
        "../../../examples/template-002.definition.json"
    ))
    .unwrap();
    d.content_hash = authoring_hash(&p, d.composition_ref).unwrap();
    for v in d.variants.values_mut() {
        v.content_hash = authoring_hash(&p, v.composition_ref).unwrap();
    }
    validate_definition(&p, &d).unwrap();
    let portrait = selected_definition(&d, Some("portrait")).unwrap();
    assert_ne!(portrait.composition_ref, d.composition_ref);
    assert_eq!(
        portrait.public_inputs["data"].default,
        d.public_inputs["data"].default
    );
    assert_ne!(
        portrait.public_inputs["data"].target,
        d.public_inputs["data"].target
    );
    let input = &d.public_inputs["data"];
    let Value::DataTable(default) = &input.default else {
        panic!()
    };
    for mutation in 0..5 {
        let mut bad = default.clone();
        match mutation {
            0 => {
                bad.rows[0].remove("headline");
            }
            1 => {
                bad.rows[0].insert("private".into(), Value::Bool(true));
            }
            2 => {
                bad.rows[0].insert(
                    "headline".into(),
                    Value::Scalar(FiniteF64::new(1.0).unwrap()),
                );
            }
            3 => {
                bad.columns.insert("headline".into(), ValueType::Bool);
            }
            _ => {
                bad.rows.clear();
            }
        }
        let value = Value::DataTable(bad);
        let values = BTreeMap::from([("data".into(), value)]);
        assert!(input_bindings(&d, &values).is_err());
    }
    let mut collision = d.clone();
    collision
        .public_inputs
        .insert("duplicate".into(), collision.public_inputs["data"].clone());
    assert!(validate_definition(&p, &collision).is_err());
    let mut missing = d.clone();
    missing
        .variants
        .get_mut("portrait")
        .unwrap()
        .targets
        .clear();
    assert!(validate_definition(&p, &missing).is_err());
    assert_eq!(
        selected_definition(&d, Some("missing")).unwrap_err().code(),
        "TEMPLATE_VARIANT_NOT_FOUND"
    );
    let id = portrait.composition_ref;
    let DocumentObject::Known(c) = p
        .compositions
        .iter_mut()
        .find(|c| matches!(c,DocumentObject::Known(c) if c.id == id))
        .unwrap()
    else {
        panic!()
    };
    c.design_extent = DesignExtent::new(100.0, 200.0).unwrap();
    assert_eq!(
        validate_definition(&p, &d).unwrap_err().code(),
        "TEMPLATE_DEFINITION_CHANGED"
    );
}
fn duration(n: i64, d: i64) -> Duration {
    Duration::new(t(n, d)).unwrap()
}
fn fixture() -> (Project, TemplateDefinition) {
    let p: Project =
        serde_json::from_str(include_str!("../../../examples/template-001.project.json")).unwrap();
    let mut d: TemplateDefinition = serde_json::from_str(include_str!(
        "../../../examples/template-001.definition.json"
    ))
    .unwrap();
    d.content_hash = authoring_hash(&p, d.composition_ref).unwrap();
    (p, d)
}
#[test]
fn five_to_eight_seconds_preserves_intro_and_outro_sampled_values_exactly() {
    let (_, d) = fixture();
    let five = duration_map(duration(5, 1), duration(5, 1), &d.duration_policy).unwrap();
    let eight = duration_map(duration(5, 1), duration(8, 1), &d.duration_policy).unwrap();
    let scalar = |v| Value::Scalar(FiniteF64::new(v).unwrap());
    let curve = AnimationCurve::new(
        CurveId::new(),
        ValueType::Scalar,
        vec![
            Keyframe {
                time: Time::ZERO,
                value: scalar(0.0),
                interpolation: CurveInterpolation::Linear,
            },
            Keyframe {
                time: t(2, 5),
                value: scalar(1.0),
                interpolation: CurveInterpolation::Hold,
            },
            Keyframe {
                time: t(47, 10),
                value: scalar(1.0),
                interpolation: CurveInterpolation::Linear,
            },
            Keyframe {
                time: t(5, 1),
                value: scalar(0.0),
                interpolation: CurveInterpolation::Hold,
            },
        ],
    )
    .unwrap();
    for time in [Time::ZERO, t(1, 10), t(1, 5), t(3, 10), t(2, 5)] {
        assert_eq!(five.map(time).unwrap(), eight.map(time).unwrap());
        assert_eq!(
            kronello_animation::sample(&curve, five.map(time).unwrap()).unwrap(),
            kronello_animation::sample(&curve, eight.map(time).unwrap()).unwrap()
        );
    }
    for elapsed in [Time::ZERO, t(1, 20), t(1, 10), t(1, 5), t(3, 10)] {
        let a = t(47, 10).checked_add(elapsed).unwrap();
        let b = t(77, 10).checked_add(elapsed).unwrap();
        assert_eq!(five.map(a).unwrap(), eight.map(b).unwrap());
        assert_eq!(
            kronello_animation::sample(&curve, five.map(a).unwrap()).unwrap(),
            kronello_animation::sample(&curve, eight.map(b).unwrap()).unwrap()
        );
    }
    assert_eq!(eight.map(t(81, 20)).unwrap(), t(51, 20));
    assert!(matches!(
        duration_map(duration(5, 1), duration(1, 1), &d.duration_policy),
        Err(TemplateError::DurationTooShort)
    ));
    assert!(eight.map(t(81, 10)).is_err());
}
#[test]
fn defaults_constraints_and_public_input_isolation() {
    let (_, d) = fixture();
    let mut a = TemplateInstance {
        id: CompositionInstanceId::new(),
        definition_ref: d.id,
        version: d.version.clone(),
        duration: duration(5, 1),
        variant: None,
        inputs: BTreeMap::new(),
    };
    let b = a.clone();
    a.inputs
        .insert("headline".into(), Value::String("別の見出し".into()));
    assert_ne!(
        resolved_inputs(&d, &a).unwrap(),
        resolved_inputs(&d, &b).unwrap()
    );
    assert_eq!(
        d.public_inputs["headline"].default,
        Value::String("日本語".into())
    );
    a.inputs.insert("private".into(), Value::Bool(true));
    assert!(resolved_inputs(&d, &a).is_err());
    a.inputs.remove("private");
    a.version = "2.0.0".into();
    assert!(resolved_inputs(&d, &a).is_err());
    let scalar = |v| Value::Scalar(FiniteF64::new(v).unwrap());
    let n = TemplateInput {
        value_type: ValueType::Scalar,
        default: scalar(2.0),
        target: d.public_inputs["accent"].target.clone(),
        minimum: Some(FiniteF64::new(1.0).unwrap()),
        maximum: Some(FiniteF64::new(3.0).unwrap()),
        choices: vec![],
    };
    assert!(validate_input(&n, &scalar(4.0)).is_err());
    assert!(validate_input(&n, &Value::String("2".into())).is_err());
    assert!(validate_input(&n, &scalar(3.0)).is_ok());
}
#[test]
fn reachable_authoring_content_is_immutable() {
    let (mut p, d) = fixture();
    validate_definition(&p, &d).unwrap();
    let DocumentObject::Known(text) = &mut p.texts[0] else {
        panic!()
    };
    text.text = "変更".into();
    assert!(matches!(
        validate_definition(&p, &d),
        Err(TemplateError::DefinitionChanged)
    ));
    let (mut p, d) = fixture();
    let DocumentObject::Known(c) = &mut p.compositions[1] else {
        panic!()
    };
    c.design_extent = DesignExtent::new(100.0, 100.0).unwrap();
    assert!(matches!(
        validate_definition(&p, &d),
        Err(TemplateError::DefinitionChanged)
    ));
}
#[test]
fn bounds_padding_and_typed_overflow() {
    let padding = [FiniteF64::new(4.0).unwrap(), FiniteF64::new(2.0).unwrap()];
    let (size, origin) = band_values([0.0, 0.0], [36.0, 16.0], [20.0, 5.0], padding).unwrap();
    assert_eq!(size.map(FiniteF64::get), [44.0, 20.0]);
    assert_eq!(origin.map(FiniteF64::get), [16.0, 3.0]);
    let node = NodeId::new();
    check_lines(node, 2, 2).unwrap();
    let error = check_lines(node, 3, 2).unwrap_err();
    assert_eq!(error.code(), "TEMPLATE_OVERFLOW");
    assert!(matches!(error,TemplateError::Overflow {node:n,actual:3,maximum:2} if n==node));
}

#[test]
fn nested_template_inputs_and_definition_are_frozen() {
    let (mut project, nested) = fixture();
    project
        .templates
        .push(DocumentObject::Known(nested.clone()));
    let instance = TemplateInstance {
        id: CompositionInstanceId::new(),
        definition_ref: nested.id,
        version: nested.version.clone(),
        duration: duration(8, 1),
        variant: None,
        inputs: BTreeMap::new(),
    };
    let DocumentObject::Known(root) = &mut project.compositions[0] else {
        panic!()
    };
    let node = SceneNode {
        id: NodeId::new(),
        kind: NodeKind::CompositionInstance(CompositionInstance {
            id: instance.id,
            definition_ref: nested.composition_ref,
            input_bindings: BTreeMap::new(),
            local_time_map: duration_map(duration(5, 1), duration(8, 1), &nested.duration_policy)
                .unwrap(),
            seed: 0,
        }),
        containment_parent: None,
        transform_parent: None,
        child_order: vec![],
        active_range: kronello_time::TimeRange::from_start_duration(Time::ZERO, instance.duration)
            .unwrap(),
        properties: vec![],
        effects: vec![],
    };
    root.root_nodes.push(node.id);
    root.nodes.push(node);
    let mut parent = nested.clone();
    parent.id = uuid::Uuid::new_v4();
    parent.template_id = uuid::Uuid::new_v4();
    parent.composition_ref = root.id;
    parent.public_inputs.clear();
    parent.constraints = TemplateConstraints::default();
    project
        .template_instances
        .push(DocumentObject::Known(instance));
    parent.content_hash = authoring_hash(&project, parent.composition_ref).unwrap();
    validate_definition(&project, &parent).unwrap();
    let original = project.clone();
    let DocumentObject::Known(instance) = &mut project.template_instances[0] else {
        panic!()
    };
    instance
        .inputs
        .insert("headline".into(), Value::String("変更".into()));
    assert!(matches!(
        validate_definition(&project, &parent),
        Err(TemplateError::DefinitionChanged)
    ));
    let mut project = original;
    let DocumentObject::Known(nested) = &mut project.templates[0] else {
        panic!()
    };
    nested.public_inputs.get_mut("headline").unwrap().default = Value::String("新版".into());
    assert!(matches!(
        validate_definition(&project, &parent),
        Err(TemplateError::DefinitionChanged)
    ));
}
