use kronello_eval::*;
use kronello_model::*;
use kronello_time::{Time, TimeMap};
use std::collections::BTreeMap;

fn scalar(x: f64) -> Value {
    Value::Scalar(FiniteF64::new(x).unwrap())
}
fn expr(nodes: Vec<ExpressionNode>) -> Expression {
    Expression {
        id: ExpressionId::new(),
        version: EXPRESSION_VERSION,
        value_type: ValueType::Scalar,
        budget: ExpressionBudget::default(),
        nodes,
    }
}
fn fixture(e: &Expression) -> (Composition, RuntimePropertyKey) {
    let p: Project =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    let DocumentObject::Known(mut c) = p.compositions[0].clone() else {
        panic!()
    };
    c.nodes.truncate(1);
    c.root_nodes.truncate(1);
    c.nodes[0].kind = NodeKind::Null;
    c.nodes[0]
        .properties
        .retain(|p| p.descriptor().key.as_str() == "kronello.opacity");
    let property = &mut c.nodes[0].properties[0];
    property
        .set_source(
            PropertySource::Expression(e.id),
            &SchemaRegistry::with_builtin(),
        )
        .unwrap();
    let key = PropertyKey {
        instance_path: InstancePath::root(),
        node: c.nodes[0].id,
        property: c.nodes[0].properties[0].id(),
    }
    .into();
    (c, key)
}
fn graph<'a>(
    c: &'a [Composition],
    e: &'a [Expression],
    curves: &'a [AnimationCurve],
    registry: &'a SchemaRegistry,
) -> Result<DependencyGraph<'a>, EvaluationError> {
    static EMPTY: BTreeMap<RuntimePropertyKey, RuntimePropertyKey> = BTreeMap::new();
    static DEPS: BTreeMap<RuntimePropertyKey, Vec<RuntimePropertyKey>> = BTreeMap::new();
    DependencyGraph::compile(
        EvaluationSnapshot {
            compositions: c,
            curves,
            expressions: e,
            registry,
            reference_bindings: &EMPTY,
            dependencies: &DEPS,
            working_space: ColorSpace::LinearRec709,
        },
        c[0].id,
    )
}

#[test]
fn canonical_ast_roundtrip_types_and_forbidden_capabilities() {
    let e = expr(vec![
        ExpressionNode::Literal(scalar(0.2)),
        ExpressionNode::Time,
        ExpressionNode::Add { left: 0, right: 1 },
    ]);
    e.validate().unwrap();
    assert_eq!(
        e,
        serde_json::from_str::<Expression>(&serde_json::to_string(&e).unwrap()).unwrap()
    );
    for capability in [
        "network",
        "file",
        "clock",
        "environment",
        "random",
        "loop",
        "dynamic_lookup",
        "eval",
        "shell",
    ] {
        let mut raw = serde_json::to_value(&e).unwrap();
        raw["nodes"] = serde_json::json!([{capability: {"argument": "untrusted data"}}]);
        assert!(
            serde_json::from_value::<Expression>(raw.clone()).is_err(),
            "{capability}"
        );
        let mut p = Project::default();
        p.expressions
            .push(serde_json::from_value(raw.clone()).unwrap());
        assert_eq!(serde_json::to_value(&p.expressions[0]).unwrap(), raw);
        assert!(p.ensure_editable().is_err());
    }
    for nodes in [
        vec![
            ExpressionNode::Literal(scalar(1.0)),
            ExpressionNode::Add { left: 0, right: 0 },
        ],
        vec![
            ExpressionNode::Literal(scalar(1.0)),
            ExpressionNode::Literal(scalar(2.0)),
        ],
        vec![
            ExpressionNode::Literal(Value::Bool(true)),
            ExpressionNode::Sin { input: 0 },
        ],
        vec![ExpressionNode::Time, ExpressionNode::Sin { input: 9 }],
    ] {
        assert!(expr(nodes).validate().is_err());
    }
}

#[test]
fn static_dependencies_cycles_missing_and_wrong_types() {
    let mut e = expr(vec![ExpressionNode::Literal(scalar(0.4))]);
    let (mut c, key) = fixture(&e);
    let mut input = c.nodes[0].properties[0].clone();
    // A separate composition input has a stable identity.
    input = Property::new(
        PropertyId::new(),
        input.descriptor().clone(),
        PropertySource::Constant(scalar(0.3)),
        vec![],
        &SchemaRegistry::with_builtin(),
    )
    .unwrap();
    e.nodes = vec![ExpressionNode::Property {
        node: None,
        property: input.id(),
        value_type: ValueType::Scalar,
    }];
    c.properties.push(input);
    let comps = [c.clone()];
    let expressions = [e.clone()];
    let registry = SchemaRegistry::with_builtin();
    let g = graph(&comps, &expressions, &[], &registry).unwrap();
    assert_eq!(g.dependencies(&key).unwrap().len(), 1);
    assert_eq!(g.evaluate_property(&key, Time::ZERO).unwrap(), scalar(0.3));
    e.nodes = vec![ExpressionNode::Property {
        node: Some(c.nodes[0].id),
        property: c.nodes[0].properties[0].id(),
        value_type: ValueType::Scalar,
    }];
    let err = graph(&[c.clone()], &[e.clone()], &[], &registry)
        .err()
        .unwrap();
    let EvaluationError::DependencyCycle { path } = err else {
        panic!("{err:?}")
    };
    assert_eq!(path, vec![key.clone(), key]);
    e.nodes = vec![ExpressionNode::Property {
        node: None,
        property: PropertyId::new(),
        value_type: ValueType::Scalar,
    }];
    assert!(matches!(
        graph(&[c.clone()], &[e.clone()], &[], &registry),
        Err(EvaluationError::PropertyNotFound(_))
    ));
    e.nodes = vec![ExpressionNode::Property {
        node: None,
        property: c.properties[0].id(),
        value_type: ValueType::Angle,
    }];
    e.value_type = ValueType::Angle;
    assert_eq!(
        graph(&[c], &[e], &[], &registry).err().unwrap().code(),
        "EVALUATION_ERROR"
    );
}

#[test]
fn instruction_memory_sample_node_dependency_budgets_are_typed() {
    for resource in [
        "instructions",
        "memory_bytes",
        "samples",
        "nodes",
        "dependencies",
    ] {
        let mut e = expr(vec![ExpressionNode::Literal(scalar(0.5))]);
        let (mut c, key) = fixture(&e);
        if matches!(resource, "samples" | "dependencies") {
            let input = Property::new(
                PropertyId::new(),
                c.nodes[0].properties[0].descriptor().clone(),
                PropertySource::Constant(scalar(0.4)),
                vec![],
                &SchemaRegistry::with_builtin(),
            )
            .unwrap();
            e.nodes = vec![ExpressionNode::Property {
                node: None,
                property: input.id(),
                value_type: ValueType::Scalar,
            }];
            c.properties.push(input);
        }
        match resource {
            "instructions" => e.budget.instructions = 0,
            "memory_bytes" => e.budget.memory_bytes = 0,
            "samples" => e.budget.samples = 0,
            "nodes" => e.budget.nodes = 0,
            "dependencies" => e.budget.dependencies = 0,
            _ => unreachable!(),
        }
        let comps = [c];
        let expressions = [e];
        let registry = SchemaRegistry::with_builtin();
        let result = match graph(&comps, &expressions, &[], &registry) {
            Ok(g) => g.evaluate_property(&key, Time::ZERO),
            Err(e) => Err(e),
        };
        let err = result.unwrap_err();
        assert_eq!(
            err.code(),
            "EXPRESSION_BUDGET_EXCEEDED",
            "{resource}: {err:?}"
        );
    }
}

#[test]
fn fixed_seed_noise_is_independent_of_query_order_and_scoped_by_instance() {
    let e = expr(vec![
        ExpressionNode::Time,
        ExpressionNode::Noise {
            seed: 123,
            element: 9,
            input: 0,
        },
        ExpressionNode::Literal(scalar(0.5)),
        ExpressionNode::Multiply { left: 1, right: 2 },
        ExpressionNode::Literal(scalar(0.5)),
        ExpressionNode::Add { left: 3, right: 4 },
    ]);
    let (c, key) = fixture(&e);
    let mut root = c.clone();
    root.id = CompositionId::new();
    root.nodes.clear();
    root.root_nodes.clear();
    for _ in 0..2 {
        let mut n = c.nodes[0].clone();
        n.id = NodeId::new();
        n.properties.clear();
        n.kind = NodeKind::CompositionInstance(CompositionInstance {
            id: CompositionInstanceId::new(),
            definition_ref: c.id,
            input_bindings: BTreeMap::new(),
            local_time_map: TimeMap::linear(Time::ZERO, kronello_time::Rational::ONE).unwrap(),
            seed: 0,
        });
        root.root_nodes.push(n.id);
        root.nodes.push(n);
    }
    let id = |i: usize| match &root.nodes[i].kind {
        NodeKind::CompositionInstance(i) => i.id,
        _ => unreachable!(),
    };
    let RuntimePropertyKey::Node(mut k) = key else {
        panic!()
    };
    k.instance_path = InstancePath::root().child(id(0));
    let a = RuntimePropertyKey::Node(k.clone());
    k.instance_path = InstancePath::root().child(id(1));
    let b = RuntimePropertyKey::Node(k);
    let comps = [root, c];
    let expressions = [e];
    let registry = SchemaRegistry::with_builtin();
    let g = graph(&comps, &expressions, &[], &registry).unwrap();
    let times: Vec<_> = (0..257).map(|n| Time::new(n, 257).unwrap()).collect();
    let forward: Vec<_> = times
        .iter()
        .map(|t| g.evaluate_property(&a, *t).unwrap())
        .collect();
    for i in (0..times.len()).rev() {
        assert_eq!(g.evaluate_property(&a, times[i]).unwrap(), forward[i]);
    }
    for i in (0..times.len()).map(|n| (n * 73) % 257) {
        assert_eq!(g.evaluate_property(&a, times[i]).unwrap(), forward[i]);
    }
    assert_ne!(
        g.evaluate_property(&a, Time::ZERO).unwrap(),
        g.evaluate_property(&b, Time::ZERO).unwrap()
    );
    assert_eq!(
        g.evaluate_properties(&[a.clone(), b.clone()], Time::ZERO)
            .unwrap(),
        g.evaluate_properties(&[b, a], Time::ZERO).unwrap()
    );
}

#[test]
fn rational_curve_samples_and_arithmetic_failures() {
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
                time: Time::from_integer(1),
                value: scalar(1.0),
                interpolation: CurveInterpolation::Linear,
            },
        ],
    )
    .unwrap();
    let e = expr(vec![ExpressionNode::CurveSample {
        curve: curve.id(),
        offset: Time::new(1, 4).unwrap(),
        value_type: ValueType::Scalar,
    }]);
    let (c, key) = fixture(&e);
    let registry = SchemaRegistry::with_builtin();
    let comps = [c];
    let expressions = [e];
    let curves = [curve];
    assert_eq!(
        graph(&comps, &expressions, &curves, &registry)
            .unwrap()
            .evaluate_property(&key, Time::new(1, 4).unwrap())
            .unwrap(),
        scalar(0.5)
    );
    let e = expr(vec![
        ExpressionNode::Literal(scalar(1.0)),
        ExpressionNode::Literal(scalar(0.0)),
        ExpressionNode::Divide { left: 0, right: 1 },
    ]);
    let (c, key) = fixture(&e);
    let comps = [c];
    let expressions = [e];
    assert_eq!(
        graph(&comps, &expressions, &[], &registry)
            .unwrap()
            .evaluate_property(&key, Time::ZERO)
            .unwrap_err()
            .code(),
        "EVALUATION_ERROR"
    );
}

#[test]
fn default_sample_ceiling_counts_repeated_references() {
    let mut e = expr(vec![ExpressionNode::Literal(scalar(0.4))]);
    let (mut c, key) = fixture(&e);
    let input = Property::new(
        PropertyId::new(),
        c.nodes[0].properties[0].descriptor().clone(),
        PropertySource::Constant(scalar(0.001)),
        vec![],
        &SchemaRegistry::with_builtin(),
    )
    .unwrap();
    let reference = ExpressionNode::Property {
        node: None,
        property: input.id(),
        value_type: ValueType::Scalar,
    };
    c.properties.push(input);
    e.nodes = vec![reference.clone()];
    for _ in 1..65 {
        let left = (e.nodes.len() - 1) as u32;
        let right = e.nodes.len() as u32;
        e.nodes.push(reference.clone());
        e.nodes.push(ExpressionNode::Add { left, right });
    }
    assert_eq!(e.dependencies().len(), 1);
    e.validate().unwrap();
    let comps = [c];
    let expressions = [e];
    let registry = SchemaRegistry::with_builtin();
    let g = graph(&comps, &expressions, &[], &registry).unwrap();
    assert!(matches!(
        g.evaluate_property(&key, Time::ZERO),
        Err(EvaluationError::Expression {
            source: ExpressionError::Budget("samples"),
            ..
        })
    ));
}

#[test]
fn literal_and_upstream_payload_memory_are_bounded() {
    let mut e = expr(vec![ExpressionNode::Literal(Value::String(
        "x".repeat(1_048_576),
    ))]);
    e.value_type = ValueType::String;
    assert_eq!(e.validate(), Err(ExpressionError::Budget("memory_bytes")));
    let mut registry = SchemaRegistry::with_builtin();
    let descriptor = PropertyDescriptor::try_from(DescriptorDefinition::new(
        DescriptorId::new(),
        SchemaKey::new("test.expression_string").unwrap(),
        "String",
        ValueType::String,
        Unit::Dimensionless,
        Value::String(String::new()),
    ))
    .unwrap();
    registry.register(descriptor.clone()).unwrap();
    let descriptor = DescriptorRef::new(&descriptor);
    let upstream = Property::new(
        PropertyId::new(),
        descriptor.clone(),
        PropertySource::Constant(Value::String("x".repeat(1_048_576))),
        vec![],
        &registry,
    )
    .unwrap();
    e.nodes = vec![ExpressionNode::Property {
        node: None,
        property: upstream.id(),
        value_type: ValueType::String,
    }];
    let mut placeholder = expr(vec![ExpressionNode::Literal(scalar(0.4))]);
    placeholder.id = e.id;
    let (mut c, _) = fixture(&placeholder);
    let consumer = Property::new(
        PropertyId::new(),
        descriptor,
        PropertySource::Expression(e.id),
        vec![],
        &registry,
    )
    .unwrap();
    let key = PropertyKey {
        instance_path: InstancePath::root(),
        node: c.nodes[0].id,
        property: consumer.id(),
    }
    .into();
    c.properties.push(upstream);
    c.nodes[0].properties = vec![consumer];
    e.validate().unwrap();
    let comps = [c];
    let expressions = [e];
    assert_eq!(
        graph(&comps, &expressions, &[], &registry)
            .unwrap()
            .evaluate_property(&key, Time::ZERO)
            .unwrap_err()
            .code(),
        "EXPRESSION_BUDGET_EXCEEDED"
    );
}

#[test]
fn arithmetic_builtins_and_explicit_constructors_have_typed_results() {
    let literal = |v| ExpressionNode::Literal(scalar(v));
    let cases = [
        (
            vec![
                literal(0.2),
                literal(0.3),
                ExpressionNode::Add { left: 0, right: 1 },
            ],
            scalar(0.5),
        ),
        (
            vec![
                literal(0.75),
                literal(0.25),
                ExpressionNode::Subtract { left: 0, right: 1 },
            ],
            scalar(0.5),
        ),
        (
            vec![
                literal(0.5),
                literal(0.5),
                ExpressionNode::Multiply { left: 0, right: 1 },
            ],
            scalar(0.25),
        ),
        (
            vec![
                literal(0.25),
                literal(0.5),
                ExpressionNode::Divide { left: 0, right: 1 },
            ],
            scalar(0.5),
        ),
        (
            vec![
                literal(2.0),
                literal(0.0),
                literal(0.5),
                ExpressionNode::Clamp {
                    value: 0,
                    min: 1,
                    max: 2,
                },
            ],
            scalar(0.5),
        ),
        (
            vec![
                literal(0.0),
                literal(1.0),
                literal(0.25),
                ExpressionNode::Lerp {
                    from: 0,
                    to: 1,
                    amount: 2,
                },
            ],
            scalar(0.25),
        ),
        (
            vec![literal(0.0), ExpressionNode::Sin { input: 0 }],
            scalar(0.0),
        ),
        (
            vec![
                literal(3.0),
                literal(4.0),
                ExpressionNode::Vec2 { x: 0, y: 1 },
            ],
            Value::Vec2([FiniteF64::new(3.0).unwrap(), FiniteF64::new(4.0).unwrap()]),
        ),
        (
            vec![
                literal(3.0),
                literal(4.0),
                literal(5.0),
                ExpressionNode::Vec3 { x: 0, y: 1, z: 2 },
            ],
            Value::Vec3([
                FiniteF64::new(3.0).unwrap(),
                FiniteF64::new(4.0).unwrap(),
                FiniteF64::new(5.0).unwrap(),
            ]),
        ),
        (
            vec![literal(720.0), ExpressionNode::Angle { degrees: 0 }],
            Value::Angle(FiniteF64::new(720.0).unwrap()),
        ),
    ];
    for (nodes, expected) in cases {
        let mut e = expr(nodes);
        e.value_type = expected.value_type();
        let mut placeholder = expr(vec![literal(0.4)]);
        placeholder.id = e.id;
        let (mut c, _) = fixture(&placeholder);
        let mut registry = SchemaRegistry::with_builtin();
        let unit = if e.value_type == ValueType::Angle {
            Unit::Degrees
        } else {
            Unit::Dimensionless
        };
        let descriptor = PropertyDescriptor::try_from(DescriptorDefinition::new(
            DescriptorId::new(),
            SchemaKey::new("test.expression_result").unwrap(),
            "Result",
            e.value_type,
            unit,
            expected.clone(),
        ))
        .unwrap();
        registry.register(descriptor.clone()).unwrap();
        let consumer = Property::new(
            PropertyId::new(),
            DescriptorRef::new(&descriptor),
            PropertySource::Expression(e.id),
            vec![],
            &registry,
        )
        .unwrap();
        let key = PropertyKey {
            instance_path: InstancePath::root(),
            node: c.nodes[0].id,
            property: consumer.id(),
        }
        .into();
        c.nodes[0].properties = vec![consumer];
        e.validate().unwrap();
        let comps = [c];
        let expressions = [e];
        assert_eq!(
            graph(&comps, &expressions, &[], &registry)
                .unwrap()
                .evaluate_property(&key, Time::ZERO)
                .unwrap(),
            expected
        );
    }
}

#[test]
fn immutable_audio_feature_expression_uses_mapped_rational_time() {
    let id = AssetId::new();
    let data = AudioAnalysisDataAsset {
        id,
        source: AudioAnalysisSource::Asset {
            asset: AssetId::new(),
            stream_index: 0,
            content_hash: "a".repeat(64),
        },
        config: AudioAnalysisConfig {
            version: 1,
            sample_rate: 48000,
            window: 32,
            hop: 32,
            bands: vec![],
            time_map: TimeMap::linear(Time::ZERO, kronello_time::Rational::ONE).unwrap(),
        },
        start_sample: 0,
        sample_count: 64,
        frames: vec![
            AudioAnalysisFrame {
                time: Time::ZERO,
                rms: 0.25,
                band_energy: vec![],
                onset: 0.25,
                beat: true,
            },
            AudioAnalysisFrame {
                time: Time::new(32, 48000).unwrap(),
                rms: 0.75,
                band_energy: vec![],
                onset: 0.5,
                beat: false,
            },
        ],
    };
    let mut e = expr(vec![ExpressionNode::AudioFeature {
        asset: id,
        feature: AudioFeature::Rms,
        offset: Time::ZERO,
    }]);
    assert!(e.validate().is_err());
    e.version = 2;
    e.validate().unwrap();
    assert!(
        e.dependencies()
            .contains(&ExpressionDependency::AudioAnalysis(id))
    );
    let (c, key) = fixture(&e);
    let cs = [c];
    let es = [e];
    let ds = [data];
    let registry = SchemaRegistry::with_builtin();
    let refs = BTreeMap::new();
    let deps = BTreeMap::new();
    let g = DependencyGraph::compile_with_audio(
        EvaluationSnapshot {
            compositions: &cs,
            expressions: &es,
            curves: &[],
            registry: &registry,
            reference_bindings: &refs,
            dependencies: &deps,
            working_space: ColorSpace::LinearRec709,
        },
        cs[0].id,
        &ds,
    )
    .unwrap();
    for (time, want) in [
        (Time::new(32, 48000).unwrap(), 0.75),
        (Time::ZERO, 0.25),
        (Time::new(32, 48000).unwrap(), 0.75),
    ] {
        assert_eq!(g.evaluate_property(&key, time).unwrap(), scalar(want));
    }
    assert!(
        g.evaluate_property(&key, Time::new(64, 48000).unwrap())
            .is_err()
    );
}

#[test]
fn v3_dynamic_past_property_is_rational_pure_static_and_bounded() {
    let source = expr(vec![ExpressionNode::Time]);
    let (mut c, _) = fixture(&source);
    let source_node = c.nodes[0].id;
    let source_property = c.nodes[0].properties[0].id();
    let mut past = expr(vec![
        ExpressionNode::Time,
        ExpressionNode::Literal(scalar(0.5)),
        ExpressionNode::Multiply { left: 0, right: 1 },
        ExpressionNode::PropertySample {
            node: Some(source_node),
            property: source_property,
            value_type: ValueType::Scalar,
            lookback: 2,
        },
    ]);
    past.version = 3;
    let (mut consumer, _) = fixture(&past);
    consumer.nodes[0].id = NodeId::new();
    consumer.nodes[0].properties[0] = Property::new(
        PropertyId::new(),
        consumer.nodes[0].properties[0].descriptor().clone(),
        PropertySource::Expression(past.id),
        vec![],
        &SchemaRegistry::with_builtin(),
    )
    .unwrap();
    let key: RuntimePropertyKey = PropertyKey {
        instance_path: InstancePath::root(),
        node: consumer.nodes[0].id,
        property: consumer.nodes[0].properties[0].id(),
    }
    .into();
    c.root_nodes.push(consumer.nodes[0].id);
    c.nodes.push(consumer.nodes.remove(0));
    let registry = SchemaRegistry::with_builtin();
    let cs = [c.clone()];
    let es = [source.clone(), past.clone()];
    let g = graph(&cs, &es, &[], &registry).unwrap();
    for n in [4, 1, 3, 0, 2, 4, 8] {
        assert_eq!(
            g.evaluate_property(&key, Time::new(n, 4).unwrap()).unwrap(),
            scalar(n as f64 / 8.0)
        );
    }
    for (n, expected_ns) in [(1, 0), (2, 1), (3, 1)] {
        assert_eq!(
            g.evaluate_property(&key, Time::new(n, 1_000_000_000).unwrap())
                .unwrap(),
            scalar(expected_ns as f64 / 1_000_000_000.0)
        );
    }
    let mut future = past.clone();
    future.nodes[0] = ExpressionNode::Literal(scalar(-1.0));
    assert!(
        graph(&cs, &[source.clone(), future], &[], &registry)
            .unwrap()
            .evaluate_property(&key, Time::ZERO)
            .is_err()
    );
    assert!(g.dependencies(&key).unwrap().iter().any(|k| matches!(k,
        RuntimePropertyKey::Node(k) if k.node == source_node)));
    // Sampling a past value does not permit a temporal self-cycle.
    let mut cycle = past.clone();
    if let ExpressionNode::PropertySample { node, property, .. } = &mut cycle.nodes[3] {
        *node = Some(c.nodes[1].id);
        *property = c.nodes[1].properties[0].id();
    }
    assert!(matches!(
        graph(&cs, &[source.clone(), cycle], &[], &registry),
        Err(EvaluationError::DependencyCycle { .. })
    ));
    // Root lookback then normal target mapping, rather than subtracting local times.
    let mut root = c.clone();
    root.id = CompositionId::new();
    root.nodes.clear();
    root.root_nodes.clear();
    let instance_id = CompositionInstanceId::new();
    let mut placement = c.nodes[0].clone();
    placement.id = NodeId::new();
    placement.properties.clear();
    placement.kind = NodeKind::CompositionInstance(CompositionInstance {
        id: instance_id,
        definition_ref: c.id,
        input_bindings: BTreeMap::new(),
        local_time_map: TimeMap::linear(Time::ZERO, kronello_time::Rational::new(1, 2).unwrap())
            .unwrap(),
        seed: 0,
    });
    root.root_nodes.push(placement.id);
    root.nodes.push(placement);
    let RuntimePropertyKey::Node(mut mapped_key) = key.clone() else {
        panic!()
    };
    mapped_key.instance_path = InstancePath::root().child(instance_id);
    let mapped_cs = [root, c.clone()];
    assert_eq!(
        graph(&mapped_cs, &es, &[], &registry)
            .unwrap()
            .evaluate_property(&mapped_key.into(), Time::from_integer(1))
            .unwrap(),
        scalar(0.375)
    );
    // The outer sample ceiling includes inner static Property reads as actual work.
    let input = Property::new(
        PropertyId::new(),
        c.nodes[0].properties[0].descriptor().clone(),
        PropertySource::Constant(scalar(0.25)),
        vec![],
        &registry,
    )
    .unwrap();
    let mut nested_source = source.clone();
    nested_source.nodes = vec![ExpressionNode::Property {
        node: None,
        property: input.id(),
        value_type: ValueType::Scalar,
    }];
    let mut nested_c = c.clone();
    nested_c.properties.push(input);
    let mut sample_limited = past.clone();
    sample_limited.budget.samples = 1;
    assert!(matches!(
        graph(
            &[nested_c],
            &[nested_source, sample_limited],
            &[],
            &registry
        )
        .unwrap()
        .evaluate_property(&key, Time::new(1, 2).unwrap()),
        Err(EvaluationError::Expression {
            source: ExpressionError::Budget("samples"),
            ..
        })
    ));
    let mut bounded = past.clone();
    bounded.budget.samples = 1; // Nested source has no samples; this succeeds.
    graph(&cs, &[source.clone(), bounded.clone()], &[], &registry)
        .unwrap()
        .evaluate_property(&key, Time::new(1, 2).unwrap())
        .unwrap();
    let mut memory_limited = bounded.clone();
    memory_limited.budget.memory_bytes = 512;
    memory_limited.validate().unwrap();
    assert!(matches!(
        graph(&cs, &[source.clone(), memory_limited], &[], &registry)
            .unwrap()
            .evaluate_property(&key, Time::new(1, 2).unwrap()),
        Err(EvaluationError::Expression {
            source: ExpressionError::Budget("memory_bytes"),
            ..
        })
    ));
    bounded.budget.instructions = 5; // Includes actual nested property/expression work.
    assert!(
        graph(&cs, &[source.clone(), bounded], &[], &registry)
            .unwrap()
            .evaluate_property(&key, Time::new(1, 2).unwrap())
            .is_err()
    );
    for version in [1, 2] {
        let mut legacy = past.clone();
        legacy.version = version;
        assert!(legacy.validate().is_err());
    }
}

#[test]
fn v3_continuous_noise_is_smooth_reproducible_and_bounded() {
    let mut e = expr(vec![
        ExpressionNode::Time,
        ExpressionNode::ContinuousNoise {
            seed: 19,
            element: 7,
            input: 0,
        },
        ExpressionNode::Literal(scalar(0.5)),
        ExpressionNode::Multiply { left: 1, right: 2 },
        ExpressionNode::Literal(scalar(0.5)),
        ExpressionNode::Add { left: 3, right: 4 },
    ]);
    e.version = 3;
    let (c, key) = fixture(&e);
    let cs = [c];
    let es = [e.clone()];
    let registry = SchemaRegistry::with_builtin();
    let g = graph(&cs, &es, &[], &registry).unwrap();
    let value = |t| match g.evaluate_property(&key, t).unwrap() {
        Value::Scalar(v) => v.get(),
        _ => panic!(),
    };
    let center = value(Time::new(1, 1).unwrap());
    for t in [
        Time::new(999999, 1000000).unwrap(),
        Time::new(1000001, 1000000).unwrap(),
    ] {
        assert!((value(t) - center).abs() < 1e-12);
    }
    let a = value(Time::new(1, 4).unwrap());
    assert!((a - value(Time::new(1, 3).unwrap())).abs() > 1e-8);
    assert_eq!(a, value(Time::new(1, 4).unwrap()));
    assert!(
        g.evaluate_property(&key, Time::new(1_000_000_001, 1).unwrap())
            .is_err()
    );
    e.version = 2;
    assert!(e.validate().is_err());
}

#[test]
fn v3_data_asset_cells_are_typed_hashed_static_and_bounded() {
    let data = ExpressionDataAsset::new(
        AssetId::new(),
        DataTable {
            columns: [("v".into(), ValueType::Scalar)].into(),
            rows: vec![
                [("v".into(), scalar(0.2))].into(),
                [("v".into(), scalar(0.7))].into(),
            ],
        },
    )
    .unwrap();
    let mut e = expr(vec![
        ExpressionNode::Time,
        ExpressionNode::DataAssetCell {
            asset: data.id,
            column: "v".into(),
            row: 0,
            value_type: ValueType::Scalar,
        },
    ]);
    e.version = 3;
    let (c, key) = fixture(&e);
    let cs = [c];
    let es = [e.clone()];
    let ds = [data.clone()];
    let registry = SchemaRegistry::with_builtin();
    let refs = BTreeMap::new();
    let deps = BTreeMap::new();
    let snapshot = || EvaluationSnapshot {
        compositions: &cs,
        expressions: &es,
        curves: &[],
        registry: &registry,
        reference_bindings: &refs,
        dependencies: &deps,
        working_space: ColorSpace::LinearRec709,
    };
    let g = DependencyGraph::compile_with_data(snapshot(), cs[0].id, &[], &ds).unwrap();
    for n in [1, 0, 1] {
        assert_eq!(
            g.evaluate_property(&key, Time::from_integer(n)).unwrap(),
            scalar(if n == 0 { 0.2 } else { 0.7 })
        );
    }
    for at in [
        Time::new(1, 2).unwrap(),
        Time::from_integer(-1),
        Time::from_integer(2),
    ] {
        assert!(g.evaluate_property(&key, at).is_err());
    }
    assert!(
        e.dependencies()
            .contains(&ExpressionDependency::DataAsset(data.id))
    );
    assert!(DependencyGraph::compile_with_data(snapshot(), cs[0].id, &[], &[]).is_err());
    let mut changed = data;
    changed.table.rows[0].insert("v".into(), scalar(0.9));
    assert!(DependencyGraph::compile_with_data(snapshot(), cs[0].id, &[], &[changed]).is_err());
    e.version = 2;
    assert!(e.validate().is_err());
}
