use std::collections::BTreeMap;

use kronello_eval::{
    DependencyDeclarations, DependencyGraph, EvaluatedScene, EvaluationError, EvaluationSnapshot,
    ReferenceBindings, RuntimePropertyKey, TransformValues,
};
use kronello_model::*;
use kronello_time::{Duration, FrameRate, Time, TimeMap, TimeMapPoint, TimeRange};

fn t(n: i64, d: i64) -> Time {
    Time::new(n, d).unwrap()
}
fn f(x: f64) -> FiniteF64 {
    FiniteF64::new(x).unwrap()
}
fn scalar(x: f64) -> Value {
    Value::Scalar(f(x))
}
fn vec2(x: f64, y: f64) -> Value {
    Value::Vec2([f(x), f(y)])
}
fn property(key: &str, source: PropertySource<Value>) -> Property {
    let registry = SchemaRegistry::with_builtin();
    let descriptor = registry.lookup(&SchemaKey::new(key).unwrap()).unwrap();
    Property::new(
        PropertyId::new(),
        DescriptorRef::new(descriptor),
        source,
        vec![],
        &registry,
    )
    .unwrap()
}
fn constant(key: &str, value: Value) -> Property {
    property(key, PropertySource::Constant(value))
}
fn unsupported_opacity() -> Property {
    let mut p = constant("kronello.opacity", scalar(0.5));
    p.set_modifiers(
        vec![Modifier {
            id: ModifierId::new(),
            key: SchemaKey::new("future.unimplemented").unwrap(),
            version: 1,
            enabled: true,
            parameters: BTreeMap::new(),
        }],
        &SchemaRegistry::with_builtin(),
    )
    .unwrap();
    p
}
fn node(properties: Vec<Property>) -> SceneNode {
    SceneNode {
        effects: vec![],
        id: NodeId::new(),
        kind: NodeKind::Null,
        containment_parent: None,
        transform_parent: None,
        child_order: vec![],
        active_range: TimeRange::new(t(-100, 1), t(100, 1)).unwrap(),
        properties,
    }
}
fn composition(nodes: Vec<SceneNode>) -> Composition {
    Composition {
        id: CompositionId::new(),
        duration: Duration::new(t(100, 1)).unwrap(),
        design_extent: DesignExtent::new(1920.0, 1080.0).unwrap(),
        edit_rate: FrameRate::new(24, 1).unwrap(),
        root_nodes: nodes
            .iter()
            .filter(|n| n.containment_parent.is_none())
            .map(|n| n.id)
            .collect(),
        nodes,
        properties: vec![],
    }
}
fn placement(target: CompositionId, map: TimeMap) -> SceneNode {
    let mut n = node(vec![]);
    n.kind = NodeKind::CompositionInstance(CompositionInstance {
        id: CompositionInstanceId::new(),
        definition_ref: target,
        input_bindings: BTreeMap::new(),
        local_time_map: map,
        seed: 5,
    });
    n
}
fn instance(n: &SceneNode) -> &CompositionInstance {
    match &n.kind {
        NodeKind::CompositionInstance(i) => i,
        _ => panic!(),
    }
}
fn node_key(path: InstancePath, n: &SceneNode, p: usize) -> RuntimePropertyKey {
    PropertyKey {
        instance_path: path,
        node: n.id,
        property: n.properties[p].id(),
    }
    .into()
}
fn input_key(path: InstancePath, c: &Composition, p: usize) -> RuntimePropertyKey {
    RuntimePropertyKey::Composition {
        instance_path: path,
        composition: c.id,
        property: c.properties[p].id(),
    }
}
fn graph<'a>(
    comps: &'a [Composition],
    curves: &'a [AnimationCurve],
    refs: &'a ReferenceBindings,
    deps: &'a DependencyDeclarations,
    registry: &'a SchemaRegistry,
) -> Result<DependencyGraph<'a>, EvaluationError> {
    DependencyGraph::compile(
        EvaluationSnapshot {
            expressions: &[],
            compositions: comps,
            curves,
            registry,
            reference_bindings: refs,
            dependencies: deps,
            working_space: ColorSpace::LinearRec709,
        },
        comps[0].id,
    )
}
fn curve(value_type: ValueType, start: Value, end: Value) -> AnimationCurve {
    AnimationCurve::new(
        CurveId::new(),
        value_type,
        vec![
            Keyframe {
                time: t(-10, 1),
                value: start,
                interpolation: CurveInterpolation::Linear,
            },
            Keyframe {
                time: t(10, 1),
                value: end,
                interpolation: CurveInterpolation::Linear,
            },
        ],
    )
    .unwrap()
}
fn shuffle<T>(values: &mut [T], seed: &mut u64) {
    for i in (1..values.len()).rev() {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        values.swap(i, (*seed as usize) % (i + 1));
    }
}

#[test]
fn forward_reverse_shuffled_properties_nodes_and_257_arbitrary_times_are_identical() {
    let curves = vec![
        curve(ValueType::Vec2, vec2(-20.0, 10.0), vec2(20.0, -10.0)),
        curve(
            ValueType::Angle,
            Value::Angle(f(0.0)),
            Value::Angle(f(720.0)),
        ),
    ];
    let mut parent = node(vec![
        property(
            "kronello.transform.position",
            PropertySource::Curve(curves[0].id()),
        ),
        property(
            "kronello.transform.rotation",
            PropertySource::Curve(curves[1].id()),
        ),
    ]);
    let mut child = node(vec![
        constant("kronello.transform.position", vec2(3.0, 4.0)),
        constant("kronello.opacity", scalar(0.75)),
    ]);
    child.containment_parent = Some(parent.id);
    child.transform_parent = Some(parent.id);
    parent.child_order.push(child.id);
    let mut shared = composition(vec![parent, child]);
    shared
        .properties
        .push(constant("kronello.transform.position", vec2(0.0, 0.0)));
    let mut root = composition(vec![
        placement(shared.id, TimeMap::linear(t(1, 3), t(2, 3)).unwrap()),
        placement(shared.id, TimeMap::linear(t(-1, 2), t(3, 2)).unwrap()),
    ]);
    let mut refs = ReferenceBindings::new();
    for p in &mut root.nodes {
        p.properties.push(property(
            "kronello.transform.position",
            PropertySource::Curve(curves[0].id()),
        ));
        let source = node_key(InstancePath::root(), p, 0);
        if let NodeKind::CompositionInstance(i) = &mut p.kind {
            i.input_bindings.insert(
                shared.properties[0].id(),
                PropertySource::Constant(vec2(0.0, 0.0)),
            );
            refs.insert(
                input_key(InstancePath::root().child(i.id), &shared, 0),
                source,
            );
        }
    }
    let defs = vec![root, shared];
    let deps = DependencyDeclarations::new();
    let registry = SchemaRegistry::with_builtin();
    let g = graph(&defs, &curves, &refs, &deps, &registry).unwrap();
    let mut keys: Vec<_> = g.keys().cloned().collect();
    let mut seed = 0x94ab79;
    let times: Vec<_> = (-128..=128).map(|n| t(n, 17)).collect();
    let expected: BTreeMap<Time, EvaluatedScene> = times
        .iter()
        .map(|time| (*time, g.evaluate_scene(*time).unwrap()))
        .collect();
    let expected_values: BTreeMap<_, _> = times
        .iter()
        .map(|time| (*time, g.evaluate_properties(&keys, *time).unwrap()))
        .collect();
    let mut storage = defs.clone();
    for c in &mut storage {
        c.nodes.reverse();
        for n in &mut c.nodes {
            n.properties.reverse();
        }
    }
    let reversed = graph(&storage, &curves, &refs, &deps, &registry).unwrap();
    let mut reordered_times = times.clone();
    reordered_times.reverse();
    keys.reverse();
    for time in &reordered_times {
        assert_eq!(expected[time], reversed.evaluate_scene(*time).unwrap());
        assert_eq!(
            expected_values[time],
            g.evaluate_properties(&keys, *time).unwrap()
        );
    }
    for c in &mut storage {
        shuffle(&mut c.nodes, &mut seed);
        for n in &mut c.nodes {
            shuffle(&mut n.properties, &mut seed);
        }
    }
    let random = graph(&storage, &curves, &refs, &deps, &registry).unwrap();
    shuffle(&mut reordered_times, &mut seed);
    shuffle(&mut keys, &mut seed);
    for time in reordered_times {
        assert_eq!(expected[&time], random.evaluate_scene(time).unwrap());
        assert_eq!(
            expected_values[&time],
            g.evaluate_properties(&keys, time).unwrap()
        );
        for key in &keys {
            assert_eq!(
                g.evaluate_property(key, time).unwrap(),
                expected_values[&time][key]
            );
        }
    }
}

#[test]
fn nested_instances_map_local_time_and_parent_binding_time_exactly() {
    let c = curve(ValueType::Vec2, vec2(-10.0, 0.0), vec2(10.0, 0.0));
    let leaf = composition(vec![node(vec![property(
        "kronello.transform.position",
        PropertySource::Curve(c.id()),
    )])]);
    let mut mid = composition(vec![placement(
        leaf.id,
        TimeMap::linear(t(1, 1), t(3, 1)).unwrap(),
    )]);
    mid.properties.push(property(
        "kronello.transform.position",
        PropertySource::Curve(c.id()),
    ));
    let mut outer = placement(mid.id, TimeMap::linear(t(1, 1), t(2, 1)).unwrap());
    if let NodeKind::CompositionInstance(i) = &mut outer.kind {
        i.input_bindings
            .insert(mid.properties[0].id(), PropertySource::Curve(c.id()));
    }
    let root = composition(vec![outer]);
    let path = InstancePath::root().child(instance(&root.nodes[0]).id);
    let leaf_path = path.child(instance(&mid.nodes[0]).id);
    let input = input_key(path.clone(), &mid, 0);
    let defs = vec![root, mid, leaf];
    let curves = vec![c];
    let refs = ReferenceBindings::new();
    let deps = DependencyDeclarations::new();
    let registry = SchemaRegistry::with_builtin();
    let g = graph(&defs, &curves, &refs, &deps, &registry).unwrap();
    assert_eq!(g.local_time(&path, t(1, 1)).unwrap(), t(3, 1));
    assert_eq!(g.local_time(&leaf_path, t(1, 1)).unwrap(), t(10, 1));
    assert_eq!(
        g.evaluate_property(&input, t(1, 1)).unwrap(),
        vec2(1.0, 0.0)
    );
    let scene = g.evaluate_scene(t(1, 1)).unwrap();
    assert_eq!(scene.nodes[2].local_time, t(10, 1));
    assert_eq!(scene.nodes[2].transform.position, [10.0, 0.0]);
    assert_eq!(scene.inputs[&input], vec2(1.0, 0.0));
}

#[test]
fn placement_input_references_parent_property_and_validates_target_range() {
    let c = curve(ValueType::Scalar, scalar(-1.0), scalar(2.0));
    // An unbounded dimensionless upstream can exceed the target input range.
    let mut registry = SchemaRegistry::with_builtin();
    let descriptor = PropertyDescriptor::new(DescriptorDefinition::new(
        DescriptorId::new(),
        SchemaKey::new("test.factor").unwrap(),
        "Factor",
        ValueType::Scalar,
        Unit::Dimensionless,
        scalar(0.0),
    ))
    .unwrap();
    registry.register(descriptor.clone()).unwrap();
    let parent = node(vec![
        Property::new(
            PropertyId::new(),
            DescriptorRef::new(&descriptor),
            PropertySource::Curve(c.id()),
            vec![],
            &registry,
        )
        .unwrap(),
    ]);
    let mut child = composition(vec![]);
    child
        .properties
        .push(constant("kronello.opacity", scalar(0.25)));
    let mut p = placement(child.id, TimeMap::linear(t(5, 1), t(2, 1)).unwrap());
    if let NodeKind::CompositionInstance(i) = &mut p.kind {
        i.input_bindings.insert(
            child.properties[0].id(),
            PropertySource::Constant(scalar(0.5)),
        );
    }
    let source = node_key(InstancePath::root(), &parent, 0);
    let target = input_key(InstancePath::root().child(instance(&p).id), &child, 0);
    let defs = vec![composition(vec![parent, p]), child];
    let curves = vec![c];
    let refs = ReferenceBindings::from([(target.clone(), source.clone())]);
    let deps = DependencyDeclarations::new();
    let g = graph(&defs, &curves, &refs, &deps, &registry).unwrap();
    assert!(g.dependencies(&target).unwrap().contains(&source));
    assert_eq!(
        g.evaluate_property(&target, Time::ZERO).unwrap(),
        scalar(0.5)
    );
    assert!(
        matches!(g.evaluate_property(&target,t(10,1)),Err(EvaluationError::InvalidValue{key,source:ModelError::OutOfRange{..}}) if key==target)
    );
}

#[test]
fn cycles_report_full_closed_runtime_path_and_responsible_properties() {
    let n = node(vec![
        constant("kronello.opacity", scalar(0.5)),
        constant("kronello.transform.rotation", Value::Angle(f(10.0))),
        constant("kronello.transform.scale", vec2(1.0, 1.0)),
    ]);
    let a = node_key(InstancePath::root(), &n, 0);
    let b = node_key(InstancePath::root(), &n, 1);
    let c = node_key(InstancePath::root(), &n, 2);
    let defs = vec![composition(vec![n])];
    let refs = ReferenceBindings::new();
    let registry = SchemaRegistry::with_builtin();
    let deps = DependencyDeclarations::from([
        (a.clone(), vec![b.clone()]),
        (b.clone(), vec![c.clone()]),
        (c.clone(), vec![a.clone()]),
    ]);
    let err = graph(&defs, &[], &refs, &deps, &registry).err().unwrap();
    assert_eq!(err.code(), "PROPERTY_DEPENDENCY_CYCLE");
    match err {
        EvaluationError::DependencyCycle { path } => {
            assert_eq!(path.len(), 4);
            assert_eq!(path.first(), path.last());
            for edge in path.windows(2) {
                assert!(deps[&edge[0]].contains(&edge[1]));
            }
            assert!(path.contains(&a) && path.contains(&b) && path.contains(&c));
        }
        _ => panic!(),
    }
    let deps = DependencyDeclarations::from([(a.clone(), vec![a.clone()])]);
    assert_eq!(
        graph(&defs, &[], &refs, &deps, &registry).err().unwrap(),
        EvaluationError::DependencyCycle {
            path: vec![a.clone(), a]
        }
    );
}

#[test]
fn cycle_across_placement_reference_and_declared_parent_dependency_is_diagnosed() {
    let parent = node(vec![constant("kronello.opacity", scalar(0.5))]);
    let mut child = composition(vec![]);
    child
        .properties
        .push(constant("kronello.opacity", scalar(0.5)));
    let mut p = placement(child.id, TimeMap::linear(Time::ZERO, t(1, 1)).unwrap());
    if let NodeKind::CompositionInstance(i) = &mut p.kind {
        i.input_bindings.insert(
            child.properties[0].id(),
            PropertySource::Constant(scalar(0.5)),
        );
    }
    let a = node_key(InstancePath::root(), &parent, 0);
    let b = input_key(InstancePath::root().child(instance(&p).id), &child, 0);
    let refs = ReferenceBindings::from([(b.clone(), a.clone())]);
    let deps = DependencyDeclarations::from([(a.clone(), vec![b.clone()])]);
    let defs = vec![composition(vec![parent, p]), child];
    let registry = SchemaRegistry::with_builtin();
    let err = graph(&defs, &[], &refs, &deps, &registry).err().unwrap();
    match err {
        EvaluationError::DependencyCycle { path } => {
            assert_eq!(path.len(), 3);
            assert_eq!(path.first(), path.last());
            assert!(path.contains(&a) && path.contains(&b));
        }
        _ => panic!(),
    }
}

#[test]
fn missing_expressions_fail_compilation_and_modifiers_versions_are_unsupported() {
    let mut n = node(vec![property(
        "kronello.opacity",
        PropertySource::Expression(ExpressionId::new()),
    )]);
    let key = node_key(InstancePath::root(), &n, 0);
    let refs = ReferenceBindings::new();
    let deps = DependencyDeclarations::new();
    let registry = SchemaRegistry::with_builtin();
    let defs = vec![composition(vec![n.clone()])];
    let err = graph(&defs, &[], &refs, &deps, &registry).err().unwrap();
    assert_eq!(err.code(), "EVALUATION_ERROR");
    assert!(matches!(err,EvaluationError::InvalidValue{key:k,..} if k==key));
    n.properties[0]
        .set_source(PropertySource::Constant(scalar(0.5)), &registry)
        .unwrap();
    n.properties[0]
        .set_modifiers(
            vec![Modifier {
                id: ModifierId::new(),
                key: SchemaKey::new("future.clamp").unwrap(),
                version: 1,
                enabled: true,
                parameters: BTreeMap::new(),
            }],
            &registry,
        )
        .unwrap();
    let defs = vec![composition(vec![n.clone()])];
    let g = graph(&defs, &[], &refs, &deps, &registry).unwrap();
    assert_eq!(
        g.evaluate_scene(Time::ZERO).unwrap_err().code(),
        "UNSUPPORTED_FEATURE"
    );
    let mut modifier = n.properties[0].modifiers()[0].clone();
    modifier.enabled = false;
    n.properties[0]
        .set_modifiers(vec![modifier], &registry)
        .unwrap();
    let c = AnimationCurve::try_from(CurveDefinition {
        id: CurveId::new(),
        value_type: ValueType::Scalar,
        keys: vec![Keyframe {
            time: Time::ZERO,
            value: scalar(0.5),
            interpolation: CurveInterpolation::Hold,
        }],
        interpolation_version: 999,
    })
    .unwrap();
    n.properties[0]
        .set_source(PropertySource::Curve(c.id()), &registry)
        .unwrap();
    let defs = vec![composition(vec![n])];
    let curves = vec![c];
    let g = graph(&defs, &curves, &refs, &deps, &registry).unwrap();
    assert_eq!(
        g.evaluate_scene(Time::ZERO).unwrap_err().code(),
        "UNSUPPORTED_FEATURE"
    );
}

#[test]
fn active_ranges_are_half_open_and_inactive_containment_skips_descendants_and_maps() {
    let mut parent = node(vec![]);
    parent.active_range = TimeRange::new(Time::ZERO, t(1, 1)).unwrap();
    let mut child = node(vec![unsupported_opacity()]);
    child.containment_parent = Some(parent.id);
    parent.child_order.push(child.id);
    let nested = composition(vec![node(vec![])]);
    let mut p = placement(
        nested.id,
        TimeMap::piecewise_linear(vec![
            TimeMapPoint {
                parent: Time::ZERO,
                local: Time::ZERO,
            },
            TimeMapPoint {
                parent: t(1, 1),
                local: t(1, 1),
            },
        ])
        .unwrap(),
    );
    p.active_range = parent.active_range;
    let defs = vec![composition(vec![parent, child, p]), nested];
    let refs = ReferenceBindings::new();
    let deps = DependencyDeclarations::new();
    let registry = SchemaRegistry::with_builtin();
    let g = graph(&defs, &[], &refs, &deps, &registry).unwrap();
    assert!(g.evaluate_scene(t(1, 1)).unwrap().nodes.is_empty());
    assert!(g.evaluate_scene(t(-1, 1)).unwrap().nodes.is_empty());
    assert!(g.evaluate_scene(t(2, 1)).unwrap().nodes.is_empty());
    assert_eq!(
        g.evaluate_scene(Time::ZERO).unwrap_err().code(),
        "UNSUPPORTED_FEATURE"
    );
}

#[test]
fn transform_parent_is_independent_of_containment_draw_order_and_activity() {
    let a = node(vec![constant(
        "kronello.transform.position",
        vec2(100.0, 0.0),
    )]);
    let mut b = node(vec![constant(
        "kronello.transform.position",
        vec2(10.0, 0.0),
    )]);
    b.active_range = TimeRange::new(t(1, 1), t(2, 1)).unwrap();
    let mut child = node(vec![constant(
        "kronello.transform.position",
        vec2(2.0, 0.0),
    )]);
    child.containment_parent = Some(a.id);
    child.transform_parent = Some(b.id);
    let mut a = a;
    a.child_order.push(child.id);
    let defs = vec![composition(vec![a.clone(), b, child.clone()])];
    let refs = ReferenceBindings::new();
    let deps = DependencyDeclarations::new();
    let registry = SchemaRegistry::with_builtin();
    let g = graph(&defs, &[], &refs, &deps, &registry).unwrap();
    let scene = g.evaluate_scene(Time::ZERO).unwrap();
    assert_eq!(
        scene.nodes.iter().map(|n| n.key.node).collect::<Vec<_>>(),
        vec![a.id, child.id]
    );
    assert_eq!(
        scene.nodes[1].world_transform.transform_point([0.0, 0.0]),
        [12.0, 0.0]
    );
}

#[test]
fn placement_world_transform_and_containment_frames_are_preserved() {
    let nested = composition(vec![node(vec![constant(
        "kronello.transform.position",
        vec2(2.0, 3.0),
    )])]);
    let mut p = placement(nested.id, TimeMap::linear(Time::ZERO, t(1, 1)).unwrap());
    p.properties
        .push(constant("kronello.transform.position", vec2(10.0, 20.0)));
    let id = p.id;
    let defs = vec![composition(vec![p]), nested];
    let refs = ReferenceBindings::new();
    let deps = DependencyDeclarations::new();
    let registry = SchemaRegistry::with_builtin();
    let g = graph(&defs, &[], &refs, &deps, &registry).unwrap();
    let scene = g.evaluate_scene(Time::ZERO).unwrap();
    assert_eq!(scene.nodes[1].containment_parent.as_ref().unwrap().node, id);
    assert_eq!(
        scene.nodes[1].world_transform.transform_point([0.0, 0.0]),
        [12.0, 23.0]
    );
}

#[test]
fn transform_matrix_obeys_anchor_scale_x_skew_clockwise_rotation_position_order() {
    let transform = TransformValues {
        position: [10.0, 20.0],
        anchor: [1.0, 2.0],
        rotation: 90.0,
        scale: [2.0, -3.0],
        skew: 45.0,
        opacity: 0.5,
    };
    // [2,3] -> subtract anchor [1,1] -> scale [2,-3] -> shear [-1,-3]
    // -> clockwise rotation [3,-1] -> translate [13,19].
    let point = transform.affine().transform_point([2.0, 3.0]);
    assert!((point[0] - 13.0).abs() < 1e-12);
    assert!((point[1] - 19.0).abs() < 1e-12);
    let rotations = TransformValues {
        rotation: 720.0,
        ..transform
    };
    assert_eq!(rotations.rotation, 720.0);
}

#[test]
fn curve_overshoot_final_range_type_missing_curve_and_nonfinite_matrix_fail_explicitly() {
    let c = curve(ValueType::Scalar, scalar(0.0), scalar(2.0));
    let defs = vec![composition(vec![node(vec![property(
        "kronello.opacity",
        PropertySource::Curve(c.id()),
    )])])];
    let refs = ReferenceBindings::new();
    let deps = DependencyDeclarations::new();
    let registry = SchemaRegistry::with_builtin();
    let g = graph(&defs, &[], &refs, &deps, &registry).unwrap();
    assert!(matches!(
        g.evaluate_scene(Time::ZERO),
        Err(EvaluationError::InvalidValue {
            source: ModelError::CurveNotFound { .. },
            ..
        })
    ));
    let curves = vec![c];
    let g = graph(&defs, &curves, &refs, &deps, &registry).unwrap();
    assert!(matches!(
        g.evaluate_scene(t(10, 1)),
        Err(EvaluationError::InvalidValue {
            source: ModelError::OutOfRange { .. },
            ..
        })
    ));
    let defs = vec![composition(vec![node(vec![property(
        "kronello.transform.position",
        PropertySource::Curve(curves[0].id()),
    )])])];
    let g = graph(&defs, &curves, &refs, &deps, &registry).unwrap();
    assert!(matches!(
        g.evaluate_scene(Time::ZERO),
        Err(EvaluationError::InvalidValue {
            source: ModelError::ValueTypeMismatch { .. },
            ..
        })
    ));
    let defs = vec![composition(vec![node(vec![
        constant("kronello.transform.anchor", vec2(f64::MAX, f64::MAX)),
        constant("kronello.transform.scale", vec2(f64::MAX, f64::MAX)),
    ])])];
    let g = graph(&defs, &[], &refs, &deps, &registry).unwrap();
    assert!(matches!(
        g.evaluate_scene(Time::ZERO),
        Err(EvaluationError::NonFiniteTransform { .. })
    ));
}

#[test]
fn dangling_edges_invalid_input_references_and_duplicate_descriptors_are_rejected() {
    let n = node(vec![constant("kronello.opacity", scalar(0.5))]);
    let a = node_key(InstancePath::root(), &n, 0);
    let bogus = PropertyKey {
        instance_path: InstancePath::root(),
        node: n.id,
        property: PropertyId::new(),
    }
    .into();
    let defs = vec![composition(vec![n.clone()])];
    let refs = ReferenceBindings::new();
    let deps = DependencyDeclarations::from([(a.clone(), vec![bogus])]);
    let registry = SchemaRegistry::with_builtin();
    assert!(matches!(
        graph(&defs, &[], &refs, &deps, &registry),
        Err(EvaluationError::PropertyNotFound(_))
    ));
    let refs = ReferenceBindings::from([(a.clone(), a)]);
    let deps = DependencyDeclarations::new();
    assert!(matches!(
        graph(&defs, &[], &refs, &deps, &registry),
        Err(EvaluationError::InvalidReferenceBinding(_))
    ));
    let mut n = n;
    n.properties.push(constant("kronello.opacity", scalar(0.8)));
    let defs = vec![composition(vec![n])];
    assert!(matches!(
        graph(&defs, &[], &ReferenceBindings::new(), &deps, &registry),
        Err(EvaluationError::DuplicateNodeDescriptor { .. })
    ));
}

#[test]
fn inactive_transform_parent_only_requires_transform_properties() {
    let mut parent = node(vec![
        constant("kronello.transform.position", vec2(3.0, 4.0)),
        unsupported_opacity(),
    ]);
    parent.active_range = TimeRange::new(t(1, 1), t(2, 1)).unwrap();
    let mut child = node(vec![]);
    child.transform_parent = Some(parent.id);
    let defs = vec![composition(vec![parent, child])];
    let refs = ReferenceBindings::new();
    let deps = DependencyDeclarations::new();
    let registry = SchemaRegistry::with_builtin();
    let g = graph(&defs, &[], &refs, &deps, &registry).unwrap();
    let scene = g.evaluate_scene(Time::ZERO).unwrap();
    assert_eq!(scene.nodes.len(), 1);
    assert_eq!(
        scene.nodes[0].world_transform.transform_point([0.0, 0.0]),
        [3.0, 4.0]
    );
}

#[test]
fn time_map_domain_overflow_and_unknown_instance_paths_are_typed_errors() {
    let child = composition(vec![node(vec![])]);
    let root = composition(vec![placement(
        child.id,
        TimeMap::piecewise_linear(vec![
            TimeMapPoint {
                parent: Time::ZERO,
                local: Time::ZERO,
            },
            TimeMapPoint {
                parent: t(1, 1),
                local: t(1, 1),
            },
        ])
        .unwrap(),
    )]);
    let path = InstancePath::root().child(instance(&root.nodes[0]).id);
    let defs = vec![root, child];
    let refs = ReferenceBindings::new();
    let deps = DependencyDeclarations::new();
    let registry = SchemaRegistry::with_builtin();
    let g = graph(&defs, &[], &refs, &deps, &registry).unwrap();
    assert!(
        matches!(g.evaluate_scene(t(2,1)),Err(EvaluationError::TimeMapping{instance_path,source:kronello_time::TimeError::OutsideMapDomain}) if instance_path==path)
    );
    let unknown = InstancePath::new(vec![CompositionInstanceId::new()]);
    assert_eq!(
        g.local_time(&unknown, Time::ZERO).unwrap_err(),
        EvaluationError::InstancePathNotFound(unknown)
    );
    let mut defs = defs;
    if let NodeKind::CompositionInstance(i) = &mut defs[0].nodes[0].kind {
        i.local_time_map = TimeMap::linear(Time::from_integer(i64::MAX), t(2, 1)).unwrap();
    }
    let g = graph(&defs, &[], &refs, &deps, &registry).unwrap();
    assert!(matches!(
        g.local_time(&path, Time::from_integer(i64::MAX)),
        Err(EvaluationError::TimeMapping {
            source: kronello_time::TimeError::Overflow,
            ..
        })
    ));
}

#[test]
fn convergent_dependency_dag_and_disabled_modifier_do_not_change_values() {
    let registry = SchemaRegistry::with_builtin();
    let mut n = node(vec![
        constant("kronello.opacity", scalar(0.5)),
        constant("kronello.transform.rotation", Value::Angle(f(45.0))),
        constant("kronello.transform.scale", vec2(2.0, 3.0)),
    ]);
    n.properties[0]
        .set_modifiers(
            vec![Modifier {
                id: ModifierId::new(),
                key: SchemaKey::new("future.noop").unwrap(),
                version: 1,
                enabled: false,
                parameters: BTreeMap::new(),
            }],
            &registry,
        )
        .unwrap();
    let a = node_key(InstancePath::root(), &n, 0);
    let b = node_key(InstancePath::root(), &n, 1);
    let c = node_key(InstancePath::root(), &n, 2);
    let deps = DependencyDeclarations::from([
        (a.clone(), vec![b.clone(), c.clone()]),
        (b.clone(), vec![c.clone()]),
    ]);
    let refs = ReferenceBindings::new();
    let defs = vec![composition(vec![n])];
    let g = graph(&defs, &[], &refs, &deps, &registry).unwrap();
    assert_eq!(
        g.evaluate_properties(&[a.clone(), b.clone(), c.clone()], Time::ZERO)
            .unwrap(),
        g.evaluate_properties(&[c, b, a.clone()], Time::ZERO)
            .unwrap()
    );
    assert_eq!(g.evaluate_property(&a, Time::ZERO).unwrap(), scalar(0.5));
}

#[test]
fn reference_overrides_require_existing_binding_parent_scope_and_matching_units() {
    let parent = node(vec![constant("kronello.stroke_width", scalar(0.5))]);
    let mut child = composition(vec![]);
    child
        .properties
        .push(constant("kronello.opacity", scalar(0.5)));
    let p = placement(child.id, TimeMap::linear(Time::ZERO, t(1, 1)).unwrap());
    let target = input_key(InstancePath::root().child(instance(&p).id), &child, 0);
    let source = node_key(InstancePath::root(), &parent, 0);
    let mut defs = vec![composition(vec![parent, p]), child];
    let refs = ReferenceBindings::from([(target.clone(), source)]);
    let deps = DependencyDeclarations::new();
    let registry = SchemaRegistry::with_builtin();
    assert!(matches!(
        graph(&defs, &[], &refs, &deps, &registry),
        Err(EvaluationError::InvalidReferenceBinding(_))
    ));
    let input = defs[1].properties[0].id();
    if let NodeKind::CompositionInstance(i) = &mut defs[0].nodes[1].kind {
        i.input_bindings
            .insert(input, PropertySource::Constant(scalar(0.5)));
    }
    // Existing binding, but upstream has design_px units and target dimensionless.
    assert!(matches!(
        graph(&defs, &[], &refs, &deps, &registry),
        Err(EvaluationError::InvalidReferenceBinding(_))
    ));
    let refs = ReferenceBindings::from([(target.clone(), target)]);
    assert!(matches!(
        graph(&defs, &[], &refs, &deps, &registry),
        Err(EvaluationError::InvalidReferenceBinding(_))
    ));
}

#[test]
fn declared_layout_values_schedule_consumers_and_reject_reverse_wrap_cycle() {
    let mut registry = SchemaRegistry::with_builtin();
    for descriptor in shape_descriptors().into_iter().chain(text_descriptors()) {
        registry.register(descriptor).unwrap();
    }
    let p: Project =
        serde_json::from_str(include_str!("../../../examples/template-001.project.json")).unwrap();
    let definitions: Vec<_> = p
        .compositions
        .iter()
        .filter_map(|c| match c {
            DocumentObject::Known(c) => Some(c.clone()),
            _ => None,
        })
        .collect();
    let curves: Vec<_> = p
        .curves
        .iter()
        .filter_map(|c| match c {
            DocumentObject::Known(c) => Some(c.clone()),
            _ => None,
        })
        .collect();
    let c = &definitions[1];
    let text = &c.nodes[1];
    let band = &c.nodes[0];
    let size = node_key(InstancePath::root(), band, 0);
    let wrap = node_key(InstancePath::root(), text, 2);
    let layout = RuntimePropertyKey::LayoutValue {
        instance_path: InstancePath::root(),
        text: text.id,
        consumer: band.properties[0].id(),
    };
    let deps = DependencyDeclarations::from([
        (layout.clone(), vec![wrap.clone()]),
        (size.clone(), vec![layout.clone()]),
    ]);
    let refs = ReferenceBindings::new();
    let compile = |deps| {
        DependencyGraph::compile(
            EvaluationSnapshot {
                expressions: &[],
                compositions: &definitions,
                curves: &curves,
                registry: &registry,
                reference_bindings: &refs,
                dependencies: deps,
                working_space: ColorSpace::LinearRec709,
            },
            c.id,
        )
    };
    let graph = compile(&deps).unwrap();
    assert_eq!(
        graph.dependencies(&size).unwrap(),
        &std::collections::BTreeSet::from([layout.clone()])
    );
    let value = vec2(44.0, 20.0);
    let inputs = BTreeMap::from([(layout.clone(), value.clone())]);
    assert_eq!(
        graph
            .evaluate_properties_with_inputs(std::slice::from_ref(&size), Time::ZERO, &inputs)
            .unwrap()[&size],
        value
    );
    assert!(matches!(
        graph.evaluate_property(&size, Time::ZERO),
        Err(EvaluationError::MissingLayoutInput(_))
    ));
    let mut reverse_deps = deps.clone();
    reverse_deps.insert(wrap.clone(), vec![size.clone()]);
    let error = compile(&reverse_deps).err().unwrap();
    let EvaluationError::DependencyCycle { path } = error else {
        panic!()
    };
    assert!(path.contains(&size));
    assert!(path.contains(&wrap));
    assert!(path.contains(&layout));
    assert_eq!(path.first(), path.last());
}
