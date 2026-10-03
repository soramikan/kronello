use std::collections::{BTreeMap, BTreeSet};

use kronello_model::{
    Composition, CompositionError, CompositionId, CompositionInstance, CompositionInstanceId,
    CompositionReference, ContentId, DescriptorRef, DesignExtent, FiniteF64, InstancePath,
    JsonError, ModelError, NodeId, NodeKind, ParentGraph, Property, PropertyId, PropertyKey,
    PropertySource, SchemaKey, SchemaRegistry, Value, from_json, validate_compositions,
};
use kronello_time::{Duration, FrameRate, Time, TimeMap, TimeRange};
use uuid::Uuid;

fn node_id(value: u128) -> NodeId {
    NodeId::from_uuid(Uuid::from_u128(value))
}
fn comp_id(value: u128) -> CompositionId {
    CompositionId::from_uuid(Uuid::from_u128(value))
}
fn instance_id(value: u128) -> CompositionInstanceId {
    CompositionInstanceId::from_uuid(Uuid::from_u128(value))
}
fn node(id: u128, kind: NodeKind) -> SceneNode {
    SceneNode {
        effects: vec![],
        id: node_id(id),
        kind,
        containment_parent: None,
        transform_parent: None,
        child_order: vec![],
        active_range: TimeRange::new(Time::new(-1, 2).unwrap(), Time::new(3, 2).unwrap()).unwrap(),
        properties: vec![],
    }
}
use kronello_model::SceneNode;
fn composition(id: u128, nodes: Vec<SceneNode>) -> Composition {
    Composition {
        id: comp_id(id),
        duration: Duration::new(Time::from_integer(5)).unwrap(),
        design_extent: DesignExtent::new(1920.0, 1080.0).unwrap(),
        edit_rate: FrameRate::new(30000, 1001).unwrap(),
        root_nodes: nodes
            .iter()
            .filter(|n| n.containment_parent.is_none())
            .map(|n| n.id)
            .collect(),
        nodes,
        properties: vec![],
    }
}
fn instance(id: u128, target: u128) -> NodeKind {
    NodeKind::CompositionInstance(CompositionInstance {
        id: instance_id(id),
        definition_ref: comp_id(target),
        input_bindings: BTreeMap::new(),
        local_time_map: TimeMap::linear(Time::new(-1, 3).unwrap(), Time::new(2, 3).unwrap())
            .unwrap(),
        seed: 42,
    })
}
fn registry() -> SchemaRegistry {
    SchemaRegistry::with_builtin()
}
fn opacity(id: PropertyId) -> Property {
    let registry = registry();
    let descriptor = registry
        .lookup(&SchemaKey::new("kronello.opacity").unwrap())
        .unwrap();
    Property::new(
        id,
        DescriptorRef::new(descriptor),
        PropertySource::Constant(Value::Scalar(FiniteF64::new(1.0).unwrap())),
        vec![],
        &registry,
    )
    .unwrap()
}
fn edge(composition: u128, node: u128, instance: u128, target: u128) -> CompositionReference {
    CompositionReference {
        composition: comp_id(composition),
        node: node_id(node),
        instance: instance_id(instance),
        definition_ref: comp_id(target),
    }
}

#[test]
fn shared_definition_placements_have_distinct_runtime_property_keys() {
    let property = opacity(PropertyId::new());
    let property_id = property.id();
    let mut shared_node = node(30, NodeKind::Null);
    shared_node.properties.push(property);
    let definitions = vec![
        composition(
            1,
            vec![node(10, instance(100, 2)), node(11, instance(101, 2))],
        ),
        composition(2, vec![shared_node]),
    ];
    assert_eq!(validate_compositions(&definitions, &registry()), Ok(()));
    let a = InstancePath::root().child(instance_id(100));
    let b = InstancePath::root().child(instance_id(101));
    assert_eq!(a.resolve(comp_id(1), &definitions).unwrap().id, comp_id(2));
    assert_eq!(b.resolve(comp_id(1), &definitions).unwrap().id, comp_id(2));
    let keys = BTreeSet::from([
        PropertyKey {
            instance_path: a,
            node: node_id(30),
            property: property_id,
        },
        PropertyKey {
            instance_path: b,
            node: node_id(30),
            property: property_id,
        },
    ]);
    assert_eq!(keys.len(), 2);
}

#[test]
fn nested_instance_paths_keep_ancestor_placements_and_survive_storage_reordering() {
    let mut definitions = vec![
        composition(
            1,
            vec![node(10, instance(100, 2)), node(11, instance(101, 2))],
        ),
        composition(2, vec![node(20, instance(200, 3))]),
        composition(3, vec![node(30, NodeKind::Null)]),
    ];
    let a = InstancePath::new(vec![instance_id(100), instance_id(200)]);
    let b = InstancePath::new(vec![instance_id(101), instance_id(200)]);
    assert_ne!(a, b);
    assert_eq!(a.ids(), &[instance_id(100), instance_id(200)]);
    assert_eq!(a.resolve(comp_id(1), &definitions).unwrap().id, comp_id(3));
    definitions[0].nodes.reverse();
    definitions[0].root_nodes.reverse();
    definitions.reverse();
    assert_eq!(validate_compositions(&definitions, &registry()), Ok(()));
    for path in [a, b] {
        assert_eq!(
            path.resolve(comp_id(1), &definitions).unwrap().id,
            comp_id(3)
        );
        let decoded: InstancePath = from_json(&serde_json::to_string(&path).unwrap()).unwrap();
        assert_eq!(decoded, path);
    }
    assert_eq!(
        InstancePath::root()
            .resolve(comp_id(1), &definitions)
            .unwrap()
            .id,
        comp_id(1)
    );
}

#[test]
fn instance_paths_reject_wrong_ancestor_missing_root_and_missing_definition() {
    let definitions = vec![
        composition(1, vec![node(10, instance(100, 2))]),
        composition(2, vec![]),
    ];
    assert_eq!(
        InstancePath::new(vec![instance_id(100)]).resolve(comp_id(2), &definitions),
        Err(CompositionError::InvalidInstancePath {
            composition: comp_id(2),
            instance: instance_id(100),
            depth: 0
        })
    );
    assert_eq!(
        InstancePath::root().resolve(comp_id(3), &definitions),
        Err(CompositionError::CompositionNotFound { id: comp_id(3) })
    );
    assert!(
        matches!(InstancePath::new(vec![instance_id(100)]).resolve(comp_id(1), &definitions[..1]), Err(CompositionError::MissingDefinition { definition_ref, .. }) if definition_ref == comp_id(2))
    );
}

#[test]
fn containment_cycle_reports_closed_parent_path_only() {
    let mut a = node(10, NodeKind::Group);
    let mut b = node(20, NodeKind::Group);
    a.containment_parent = Some(b.id);
    a.child_order = vec![b.id];
    b.containment_parent = Some(a.id);
    b.child_order = vec![a.id];
    let errors = validate_compositions(&[composition(1, vec![b, a])], &registry()).unwrap_err();
    assert_eq!(
        errors,
        vec![CompositionError::ContainmentCycle {
            composition: comp_id(1),
            path: vec![node_id(10), node_id(20), node_id(10)]
        }]
    );
}

#[test]
fn transform_cycle_reports_closed_parent_path_only() {
    let mut a = node(10, NodeKind::Null);
    let mut b = node(20, NodeKind::Null);
    a.transform_parent = Some(b.id);
    b.transform_parent = Some(a.id);
    assert_eq!(
        validate_compositions(&[composition(1, vec![b, a])], &registry()),
        Err(vec![CompositionError::TransformCycle {
            composition: comp_id(1),
            path: vec![node_id(10), node_id(20), node_id(10)]
        }])
    );
}

#[test]
fn reference_cycle_reports_definition_and_responsible_placement_edges() {
    let definitions = vec![
        composition(3, vec![node(30, instance(300, 1))]),
        composition(1, vec![node(10, instance(100, 2))]),
        composition(2, vec![node(20, instance(200, 3))]),
    ];
    assert_eq!(
        validate_compositions(&definitions, &registry()),
        Err(vec![CompositionError::CompositionReferenceCycle {
            path: vec![
                edge(1, 10, 100, 2),
                edge(2, 20, 200, 3),
                edge(3, 30, 300, 1)
            ]
        }])
    );
}

#[test]
fn all_three_cycle_categories_are_returned_together() {
    let mut a = node(10, instance(100, 2));
    let mut b = node(20, NodeKind::Group);
    a.containment_parent = Some(b.id);
    a.child_order = vec![b.id];
    b.containment_parent = Some(a.id);
    b.child_order = vec![a.id];
    a.transform_parent = Some(b.id);
    b.transform_parent = Some(a.id);
    let errors = validate_compositions(
        &[
            composition(1, vec![a, b]),
            composition(2, vec![node(30, instance(300, 1))]),
        ],
        &registry(),
    )
    .unwrap_err();
    assert_eq!(errors.len(), 3);
    assert!(errors.contains(&CompositionError::ContainmentCycle {
        composition: comp_id(1),
        path: vec![node_id(10), node_id(20), node_id(10)]
    }));
    assert!(errors.contains(&CompositionError::TransformCycle {
        composition: comp_id(1),
        path: vec![node_id(10), node_id(20), node_id(10)]
    }));
    assert!(
        errors.contains(&CompositionError::CompositionReferenceCycle {
            path: vec![edge(1, 10, 100, 2), edge(2, 30, 300, 1)]
        })
    );
}

#[test]
fn self_cycles_and_disconnected_cycles_are_diagnosed() {
    let mut a = node(10, instance(100, 1));
    a.containment_parent = Some(a.id);
    a.child_order = vec![a.id];
    a.transform_parent = Some(a.id);
    let mut b = node(20, NodeKind::Null);
    b.transform_parent = Some(b.id);
    let errors = validate_compositions(&[composition(1, vec![a, b])], &registry()).unwrap_err();
    assert_eq!(errors.len(), 4);
    assert!(errors.contains(&CompositionError::ContainmentCycle {
        composition: comp_id(1),
        path: vec![node_id(10), node_id(10)]
    }));
    assert!(errors.contains(&CompositionError::TransformCycle {
        composition: comp_id(1),
        path: vec![node_id(20), node_id(20)]
    }));
    assert!(
        errors.contains(&CompositionError::CompositionReferenceCycle {
            path: vec![edge(1, 10, 100, 1)]
        })
    );
    assert_eq!(
        InstancePath::new(vec![instance_id(100)]).resolve(
            comp_id(1),
            &[composition(1, vec![node(10, instance(100, 1))])]
        ),
        Err(CompositionError::CompositionReferenceCycle {
            path: vec![edge(1, 10, 100, 1)]
        })
    );
}

#[test]
fn ownership_and_transform_graphs_are_independent() {
    let mut group = node(10, NodeKind::Group);
    let mut null = node(20, NodeKind::Null);
    group.child_order = vec![null.id];
    null.containment_parent = Some(group.id);
    // The union of these edges cycles; neither individual graph does.
    group.transform_parent = Some(null.id);
    let definition = composition(1, vec![null, group]);
    assert_eq!(definition.root_nodes, vec![node_id(10)]);
    assert_eq!(validate_compositions(&[definition], &registry()), Ok(()));
}

#[test]
fn convergent_reference_dag_is_valid() {
    let definitions = vec![
        composition(
            1,
            vec![node(10, instance(100, 2)), node(11, instance(101, 3))],
        ),
        composition(2, vec![node(20, instance(200, 4))]),
        composition(3, vec![node(30, instance(300, 4))]),
        composition(4, vec![]),
    ];
    assert_eq!(validate_compositions(&definitions, &registry()), Ok(()));
}

#[test]
fn missing_parents_are_typed_per_graph_and_definition_refs_are_checked() {
    let mut a = node(10, instance(100, 2));
    a.containment_parent = Some(node_id(99));
    a.transform_parent = Some(node_id(99));
    let errors = validate_compositions(&[composition(1, vec![a])], &registry()).unwrap_err();
    for graph in [ParentGraph::Containment, ParentGraph::Transform] {
        assert!(errors.contains(&CompositionError::MissingParent {
            composition: comp_id(1),
            graph,
            node: node_id(10),
            parent: node_id(99)
        }));
    }
    assert!(errors.contains(&CompositionError::MissingDefinition {
        composition: comp_id(1),
        node: node_id(10),
        instance: instance_id(100),
        definition_ref: comp_id(2)
    }));
}

#[test]
fn root_and_child_order_must_exactly_match_ownership() {
    let mut group = node(10, NodeKind::Group);
    let mut child = node(20, NodeKind::Null);
    child.containment_parent = Some(group.id);
    group.child_order = vec![child.id];
    let valid = composition(1, vec![group, child]);
    for order in [
        vec![],
        vec![node_id(20), node_id(20)],
        vec![node_id(10)],
        vec![node_id(99)],
    ] {
        let mut invalid = valid.clone();
        invalid.nodes[0].child_order = order;
        assert!(
            validate_compositions(&[invalid], &registry())
                .unwrap_err()
                .contains(&CompositionError::InvalidChildOrder {
                    composition: comp_id(1),
                    parent: Some(node_id(10))
                })
        );
    }
    let mut invalid = valid;
    invalid.root_nodes.push(node_id(20));
    assert!(
        validate_compositions(&[invalid], &registry())
            .unwrap_err()
            .contains(&CompositionError::InvalidChildOrder {
                composition: comp_id(1),
                parent: None
            })
    );
}

#[test]
fn duplicate_composition_node_instance_and_property_ids_are_rejected() {
    let property_id = PropertyId::new();
    let mut first = composition(1, vec![node(10, instance(100, 2))]);
    first.properties.push(opacity(property_id));
    let mut second = composition(2, vec![node(10, instance(100, 3))]);
    second.properties.push(opacity(property_id));
    let errors = validate_compositions(
        &[first.clone(), second, composition(3, vec![])],
        &registry(),
    )
    .unwrap_err();
    assert!(errors.contains(&CompositionError::DuplicateNodeId { id: node_id(10) }));
    assert!(errors.contains(&CompositionError::DuplicateInstanceId {
        id: instance_id(100)
    }));
    assert!(errors.contains(&CompositionError::DuplicatePropertyId { id: property_id }));
    let duplicates = [first.clone(), first];
    assert!(
        validate_compositions(&duplicates, &registry())
            .unwrap_err()
            .contains(&CompositionError::DuplicateCompositionId { id: comp_id(1) })
    );
    assert_eq!(
        InstancePath::root().resolve(comp_id(1), &duplicates),
        Err(CompositionError::DuplicateCompositionId { id: comp_id(1) })
    );
}

#[test]
fn bindings_use_existing_property_validation_without_mutating_definition() {
    let property_id = PropertyId::new();
    let mut target = composition(2, vec![]);
    target.properties.push(opacity(property_id));
    let mut placement = node(10, instance(100, 2));
    let NodeKind::CompositionInstance(ref mut inst) = placement.kind else {
        unreachable!()
    };
    inst.input_bindings.insert(
        property_id,
        PropertySource::Constant(Value::Scalar(FiniteF64::new(0.5).unwrap())),
    );
    let valid = vec![composition(1, vec![placement]), target];
    assert_eq!(validate_compositions(&valid, &registry()), Ok(()));
    assert_eq!(
        valid[1].properties[0].source(),
        &PropertySource::Constant(Value::Scalar(FiniteF64::new(1.0).unwrap()))
    );
    for value in [
        Value::Scalar(FiniteF64::new(2.0).unwrap()),
        Value::Bool(true),
    ] {
        let mut invalid = valid.clone();
        let NodeKind::CompositionInstance(ref mut inst) = invalid[0].nodes[0].kind else {
            unreachable!()
        };
        inst.input_bindings
            .insert(property_id, PropertySource::Constant(value));
        assert!(
            matches!(&validate_compositions(&invalid, &registry()).unwrap_err()[0], CompositionError::InvalidInputBinding { property, .. } if *property == property_id)
        );
    }
    let mut invalid = valid;
    let NodeKind::CompositionInstance(ref mut inst) = invalid[0].nodes[0].kind else {
        unreachable!()
    };
    let missing = PropertyId::new();
    inst.input_bindings
        .insert(missing, PropertySource::Constant(Value::Bool(true)));
    assert!(
        validate_compositions(&invalid, &registry())
            .unwrap_err()
            .contains(&CompositionError::MissingInputProperty {
                instance: instance_id(100),
                definition_ref: comp_id(2),
                property: missing
            })
    );
}

#[test]
fn document_properties_are_validated_against_registry() {
    let mut definition = composition(1, vec![node(10, NodeKind::Null)]);
    let property_id = PropertyId::new();
    definition.nodes[0].properties.push(opacity(property_id));
    let errors = validate_compositions(&[definition], &SchemaRegistry::new()).unwrap_err();
    assert!(
        matches!(&errors[0], CompositionError::InvalidProperty { composition, node: Some(node), property, source: ModelError::DescriptorNotFound { .. } } if *composition == comp_id(1) && *node == node_id(10) && *property == property_id)
    );
}

#[test]
fn all_node_kinds_rational_times_bindings_and_keys_round_trip() {
    let property_id = PropertyId::new();
    let mut target = composition(2, vec![]);
    target.properties.push(opacity(property_id));
    let mut placement = node(50, instance(500, 2));
    let NodeKind::CompositionInstance(ref mut inst) = placement.kind else {
        unreachable!()
    };
    inst.input_bindings.insert(
        property_id,
        PropertySource::Constant(Value::Scalar(FiniteF64::new(0.5).unwrap())),
    );
    let nodes = vec![
        node(10, NodeKind::Group),
        node(20, NodeKind::Null),
        node(
            30,
            NodeKind::Shape {
                content_ref: ContentId::new(),
            },
        ),
        node(
            40,
            NodeKind::Text {
                content_ref: ContentId::new(),
            },
        ),
        placement,
    ];
    let definitions = vec![composition(1, nodes), target];
    let json = serde_json::to_string(&definitions).unwrap();
    let decoded: Vec<Composition> = from_json(&json).unwrap();
    assert_eq!(decoded, definitions);
    assert_eq!(validate_compositions(&decoded, &registry()), Ok(()));
    assert!(json.contains("\"num\":\"-1\",\"den\":\"2\""));
    assert!(json.contains("\"num\":\"30000\",\"den\":\"1001\""));
    let active = decoded[0].nodes[0].active_range;
    assert!(active.contains(Time::new(-1, 2).unwrap()));
    assert!(!active.contains(Time::new(3, 2).unwrap()));
    let NodeKind::CompositionInstance(inst) = &decoded[0].nodes[4].kind else {
        unreachable!()
    };
    assert_eq!(
        inst.local_time_map.map(Time::ONE).unwrap(),
        Time::new(1, 3).unwrap()
    );
    assert_eq!(inst.seed, 42);
    let key = PropertyKey {
        instance_path: InstancePath::root().child(inst.id),
        node: node_id(10),
        property: property_id,
    };
    assert_eq!(
        from_json::<PropertyKey>(&serde_json::to_string(&key).unwrap()).unwrap(),
        key
    );
}

#[test]
fn invalid_extent_duration_range_and_unknown_structures_are_rejected() {
    for dimensions in [
        (0.0, 1.0),
        (1.0, -1.0),
        (f64::INFINITY, 1.0),
        (1.0, f64::NAN),
    ] {
        assert!(DesignExtent::new(dimensions.0, dimensions.1).is_err());
    }
    let value = serde_json::to_value(composition(1, vec![node(10, NodeKind::Null)])).unwrap();
    let mut bad = value.clone();
    bad["future"] = true.into();
    assert!(matches!(
        from_json::<Composition>(&bad.to_string()),
        Err(JsonError::IncompatibleStructure { .. })
    ));
    let mut bad = value.clone();
    bad["nodes"][0]["kind"] = serde_json::json!({"kind":"future_node"});
    assert!(matches!(
        from_json::<Composition>(&bad.to_string()),
        Err(JsonError::IncompatibleStructure { .. })
    ));
    let mut bad = value.clone();
    bad["duration"]["num"] = "-1".into();
    assert!(from_json::<Composition>(&bad.to_string()).is_err());
    let mut bad = value.clone();
    bad["nodes"][0]["active_range"]["end"]["num"] = "-5".into();
    assert!(from_json::<Composition>(&bad.to_string()).is_err());
    let mut bad = value;
    bad["design_extent"]["width"] = 0.into();
    assert!(from_json::<Composition>(&bad.to_string()).is_err());
}

#[test]
fn reference_cycle_path_excludes_acyclic_prefix_and_is_order_independent() {
    let mut definitions = vec![
        composition(
            1,
            vec![node(10, instance(100, 2)), node(11, instance(101, 4))],
        ),
        composition(2, vec![node(20, instance(200, 3))]),
        composition(3, vec![node(30, instance(300, 2))]),
        composition(4, vec![]),
    ];
    let expected = Err(vec![CompositionError::CompositionReferenceCycle {
        path: vec![edge(2, 20, 200, 3), edge(3, 30, 300, 2)],
    }]);
    assert_eq!(validate_compositions(&definitions, &registry()), expected);
    for definition in &mut definitions {
        definition.nodes.reverse();
        definition.root_nodes.reverse();
    }
    definitions.reverse();
    assert_eq!(validate_compositions(&definitions, &registry()), expected);
    assert_eq!(
        InstancePath::new(vec![instance_id(100), instance_id(200), instance_id(300)])
            .resolve(comp_id(1), &definitions),
        Err(CompositionError::CompositionReferenceCycle {
            path: vec![edge(2, 20, 200, 3), edge(3, 30, 300, 2)]
        })
    );
}

#[test]
fn deeply_nested_parent_graphs_validate_without_recursive_traversal() {
    let mut nodes: Vec<_> = (1..=4096).map(|id| node(id, NodeKind::Group)).collect();
    for index in 0..nodes.len() - 1 {
        let child = nodes[index + 1].id;
        let parent = nodes[index].id;
        nodes[index].child_order.push(child);
        nodes[index + 1].containment_parent = Some(parent);
        // Traversal from the smallest ID descends the entire transform chain.
        nodes[index].transform_parent = Some(child);
    }
    assert_eq!(
        validate_compositions(&[composition(1, nodes)], &registry()),
        Ok(())
    );
}

#[test]
fn duplicate_nodes_and_instances_within_one_definition_are_rejected() {
    let definitions = [
        composition(
            1,
            vec![node(10, instance(100, 2)), node(10, instance(100, 2))],
        ),
        composition(2, vec![]),
    ];
    let errors = validate_compositions(&definitions, &registry()).unwrap_err();
    assert!(errors.contains(&CompositionError::DuplicateNodeId { id: node_id(10) }));
    assert!(errors.contains(&CompositionError::DuplicateInstanceId {
        id: instance_id(100)
    }));
    assert_eq!(
        InstancePath::new(vec![instance_id(100)]).resolve(comp_id(1), &definitions),
        Err(CompositionError::DuplicateInstanceId {
            id: instance_id(100)
        })
    );
}
