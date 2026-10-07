use kronello_model::*;
use kronello_render::{CacheConfig, RenderCache, RenderSnapshot, build_scene_ir_with_cache};
use kronello_time::{Duration, Time};
fn fixture() -> (Project, CompositionId) {
    let mut p: Project =
        serde_json::from_str(include_str!("../../../examples/ffi-preview.project.json")).unwrap();
    let DocumentObject::Known(source) = &p.compositions[0] else {
        panic!()
    };
    let source = source.clone();
    let mut main = source.clone();
    main.id = CompositionId::new();
    let mut node = source.nodes[0].clone();
    node.id = NodeId::new();
    node.properties = Vec::new();
    let registry = kronello_render::render_registry();
    for descriptor in simulation_descriptors() {
        let value = match descriptor.key().as_str() {
            "kronello.simulation.velocity" => {
                Value::Vec2([FiniteF64::new(20.).unwrap(), FiniteF64::new(0.).unwrap()])
            }
            _ => descriptor.definition().default.clone(),
        };
        node.properties.push(
            Property::new(
                PropertyId::new(),
                DescriptorRef::new(&descriptor),
                PropertySource::Constant(value),
                vec![],
                &registry,
            )
            .unwrap(),
        );
    }
    let ids: Vec<_> = node.properties.iter().map(Property::id).collect();
    let id = ContentId::new();
    node.kind = NodeKind::Simulation { content_ref: id };
    node.child_order = Vec::new();
    node.containment_parent = None;
    node.transform_parent = None;
    main.root_nodes = vec![node.id];
    main.nodes = vec![node];
    p.simulations
        .push(DocumentObject::Known(ParticleSimulation {
            id,
            version: 1,
            source: RepeatSource {
                composition: source.id,
                root: source.root_nodes[0],
            },
            start: Time::ZERO,
            step: Duration::new(Time::new(1, 10).unwrap()).unwrap(),
            lifetime: Duration::new(Time::ONE).unwrap(),
            emission_interval: Duration::new(Time::new(1, 5).unwrap()).unwrap(),
            seed: 7,
            inputs: ParticlePropertyInputs {
                origin: ids[0],
                velocity: ids[1],
                jitter: ids[2],
                acceleration: ids[3],
                enabled: ids[4],
                birth_count: ids[5],
            },
            source_bindings: Default::default(),
        }));
    let root = main.id;
    p.compositions.push(DocumentObject::Known(main));
    (p, root)
}
#[test]
fn integrated_simulation_is_order_independent_and_pinned() {
    let (p, root) = fixture();
    let snapshot = RenderSnapshot::new(&p, root, 0, Default::default()).unwrap();
    let mut cache = RenderCache::default();
    for time in [
        Time::new(7, 10).unwrap(),
        Time::new(1, 10).unwrap(),
        Time::new(5, 10).unwrap(),
        Time::new(7, 10).unwrap(),
    ] {
        let warm = build_scene_ir_with_cache(&snapshot, time, &[], &mut cache).unwrap();
        let cold = build_scene_ir_with_cache(
            &snapshot,
            time,
            &[],
            &mut RenderCache::new(CacheConfig::disabled()),
        )
        .unwrap();
        assert_eq!(warm, cold);
        assert!(warm.nodes.len() > 1);
    }
    let mut wire = serde_json::to_value(&snapshot).unwrap();
    wire["semantic_versions"]
        .as_object_mut()
        .unwrap()
        .remove("simulation");
    let unpinned: RenderSnapshot = serde_json::from_value(wire).unwrap();
    assert_eq!(
        unpinned.validate().unwrap_err().code(),
        "UNSUPPORTED_FEATURE"
    );
}

#[test]
fn checkpoint_reuses_independent_appearance_but_replays_changed_dynamics() {
    let (mut p, root) = fixture();
    let time = Time::new(4, 5).unwrap();
    let mut cache = RenderCache::default();
    let snapshot = RenderSnapshot::new(&p, root, 0, Default::default()).unwrap();
    let first = build_scene_ir_with_cache(&snapshot, time, &[], &mut cache).unwrap();
    assert_eq!(cache.stats().simulation.replayed_steps, 8);
    let DocumentObject::Known(source) = &mut p.compositions[0] else {
        panic!()
    };
    let color = source.nodes[0]
        .properties
        .iter_mut()
        .find(|p| {
            p.descriptor().key.as_str().contains("fill")
                && matches!(p.source(), PropertySource::Constant(Value::Color(_)))
        })
        .unwrap();
    let registry = kronello_render::render_registry();
    let PropertySource::Constant(Value::Color(old)) = color.source() else {
        panic!()
    };
    color
        .set_source(
            PropertySource::Constant(Value::Color(
                Color::new(old.space(), [0.1, 0.2, 0.3], 0.7).unwrap(),
            )),
            &registry,
        )
        .unwrap();
    let first_pixels = pixels(
        &snapshot,
        time,
        &mut RenderCache::new(CacheConfig::disabled()),
    );
    cache.reset_stats();
    let snapshot = RenderSnapshot::new(&p, root, 1, Default::default()).unwrap();
    let appearance = build_scene_ir_with_cache(&snapshot, time, &[], &mut cache).unwrap();
    assert_eq!(cache.stats().simulation.replayed_steps, 0);
    assert_eq!(cache.stats().simulation.checkpoint_hits, 1);
    assert_ne!(first_pixels, pixels(&snapshot, time, &mut cache));

    for node in &first.nodes {
        let after = appearance.nodes.iter().find(|n| n.key == node.key).unwrap();
        assert_eq!(node.world_transform, after.world_transform);
    }
    let DocumentObject::Known(main) = p
        .compositions
        .iter_mut()
        .find(|c| matches!(c,DocumentObject::Known(c)if c.id==root))
        .unwrap()
    else {
        panic!()
    };
    main.nodes[0].properties[1]
        .set_source(
            PropertySource::Constant(Value::Vec2([
                FiniteF64::new(40.).unwrap(),
                FiniteF64::new(0.).unwrap(),
            ])),
            &registry,
        )
        .unwrap();
    cache.reset_stats();
    let snapshot = RenderSnapshot::new(&p, root, 2, Default::default()).unwrap();
    let changed = build_scene_ir_with_cache(&snapshot, time, &[], &mut cache).unwrap();
    assert_eq!(cache.stats().simulation.replayed_steps, 8);
    assert_eq!(cache.stats().simulation.checkpoint_hits, 0);
    assert!(first.nodes.iter().any(|n| {
        changed
            .nodes
            .iter()
            .any(|m| m.key == n.key && m.world_transform != n.world_transform)
    }));
}

#[test]
fn shared_authored_ids_have_independent_scope_bindings() {
    let (mut p, emitter) = fixture();
    let registry = kronello_render::render_registry();
    let descriptor = registry
        .lookup(&SchemaKey::new("kronello.simulation.velocity").unwrap())
        .unwrap();
    let input = Property::new(
        PropertyId::new(),
        DescriptorRef::new(descriptor),
        PropertySource::Constant(descriptor.definition().default.clone()),
        vec![],
        &registry,
    )
    .unwrap();
    let expression = Expression {
        id: ExpressionId::new(),
        version: 1,
        value_type: ValueType::Vec2,
        budget: Default::default(),
        nodes: vec![ExpressionNode::Property {
            node: None,
            property: input.id(),
            value_type: ValueType::Vec2,
        }],
    };
    let DocumentObject::Known(main) = p
        .compositions
        .iter_mut()
        .find(|c| matches!(c,DocumentObject::Known(c)if c.id==emitter))
        .unwrap()
    else {
        panic!()
    };
    main.nodes[0].properties[1]
        .set_source(PropertySource::Expression(expression.id), &registry)
        .unwrap();
    main.properties.push(input.clone());
    let mut wrapper = main.clone();
    wrapper.id = CompositionId::new();
    wrapper.properties = Vec::new();
    wrapper.nodes = Vec::new();
    wrapper.root_nodes = Vec::new();
    let instances = [CompositionInstanceId::new(), CompositionInstanceId::new()];
    for (id, speed) in instances.into_iter().zip([10., 30.]) {
        let mut n = main.nodes[0].clone();
        n.id = NodeId::new();
        n.properties = Vec::new();
        n.kind = NodeKind::CompositionInstance(CompositionInstance {
            id,
            definition_ref: emitter,
            input_bindings: std::collections::BTreeMap::from([(
                input.id(),
                PropertySource::Constant(Value::Vec2([
                    FiniteF64::new(speed).unwrap(),
                    FiniteF64::new(0.).unwrap(),
                ])),
            )]),
            local_time_map: kronello_time::TimeMap::linear(Time::ZERO, Time::ONE).unwrap(),
            seed: 0,
        });
        wrapper.root_nodes.push(n.id);
        wrapper.nodes.push(n);
    }
    let root = wrapper.id;
    p.compositions.push(DocumentObject::Known(wrapper));
    p.expressions.push(DocumentObject::Known(expression));
    let snapshot = RenderSnapshot::new(&p, root, 0, Default::default()).unwrap();
    let scene = build_scene_ir_with_cache(
        &snapshot,
        Time::new(1, 2).unwrap(),
        &[],
        &mut RenderCache::default(),
    )
    .unwrap();
    let max_x = |id| {
        scene
            .nodes
            .iter()
            .filter(|n| {
                n.key.instance_path.ids().first() == Some(&id)
                    && n.key.instance_path.ids().len() == 2
            })
            .map(|n| n.world_transform.0[0][2])
            .fold(f64::NEG_INFINITY, f64::max)
    };
    assert!((max_x(instances[1]) - max_x(instances[0]) - 10.).abs() < 1e-10);
    let mut multi_cache = RenderCache::default();
    build_scene_ir_with_cache(&snapshot, Time::new(4, 5).unwrap(), &[], &mut multi_cache).unwrap();
    multi_cache.reset_stats();
    let warm =
        build_scene_ir_with_cache(&snapshot, Time::new(4, 5).unwrap(), &[], &mut multi_cache)
            .unwrap();
    assert_eq!(multi_cache.stats().simulation.checkpoint_hits, 2);
    assert_eq!(multi_cache.stats().simulation.replayed_steps, 0);
    assert_eq!(
        warm,
        build_scene_ir_with_cache(
            &snapshot,
            Time::new(4, 5).unwrap(),
            &[],
            &mut RenderCache::new(CacheConfig::disabled())
        )
        .unwrap()
    );
    let DocumentObject::Known(wrapper) = p.compositions.last_mut().unwrap() else {
        panic!()
    };
    wrapper.nodes.reverse();
    wrapper.root_nodes.reverse();
    let snapshot = RenderSnapshot::new(&p, root, 1, Default::default()).unwrap();
    let reordered = build_scene_ir_with_cache(
        &snapshot,
        Time::new(1, 2).unwrap(),
        &[],
        &mut RenderCache::default(),
    )
    .unwrap();
    for node in &scene.nodes {
        let other = reordered.nodes.iter().find(|n| n.key == node.key).unwrap();
        assert_eq!(node.world_transform, other.world_transform);
    }
}

fn clock_fixture(map: kronello_time::TimeMap) -> (Project, CompositionId, CompositionInstanceId) {
    let (mut p, emitter) = fixture();
    let registry = kronello_render::render_registry();
    let descriptor = registry
        .lookup(&SchemaKey::new("kronello.simulation.velocity").unwrap())
        .unwrap();
    let input = Property::new(
        PropertyId::new(),
        DescriptorRef::new(descriptor),
        PropertySource::Constant(descriptor.definition().default.clone()),
        vec![],
        &registry,
    )
    .unwrap();
    let child_expression = Expression {
        id: ExpressionId::new(),
        version: 1,
        value_type: ValueType::Vec2,
        budget: Default::default(),
        nodes: vec![ExpressionNode::Property {
            node: None,
            property: input.id(),
            value_type: ValueType::Vec2,
        }],
    };
    let parent_expression = Expression {
        id: ExpressionId::new(),
        version: 1,
        value_type: ValueType::Vec2,
        budget: Default::default(),
        nodes: vec![
            ExpressionNode::Time,
            ExpressionNode::Literal(Value::Scalar(FiniteF64::new(10.).unwrap())),
            ExpressionNode::Multiply { left: 0, right: 1 },
            ExpressionNode::Literal(Value::Scalar(FiniteF64::new(0.).unwrap())),
            ExpressionNode::Vec2 { x: 2, y: 3 },
        ],
    };
    let DocumentObject::Known(main) = p
        .compositions
        .iter_mut()
        .find(|c| matches!(c,DocumentObject::Known(c)if c.id==emitter))
        .unwrap()
    else {
        panic!()
    };
    main.nodes[0].properties[1]
        .set_source(PropertySource::Expression(child_expression.id), &registry)
        .unwrap();
    main.properties.push(input.clone());
    let mut wrapper = main.clone();
    wrapper.id = CompositionId::new();
    wrapper.properties = Vec::new();
    let mut n = main.nodes[0].clone();
    n.id = NodeId::new();
    n.properties = Vec::new();
    let instance = CompositionInstanceId::new();
    n.kind = NodeKind::CompositionInstance(CompositionInstance {
        id: instance,
        definition_ref: emitter,
        input_bindings: std::collections::BTreeMap::from([(
            input.id(),
            PropertySource::Expression(parent_expression.id),
        )]),
        local_time_map: map,
        seed: 0,
    });
    n.active_range = kronello_time::TimeRange::new(Time::ZERO, Time::new(3, 1).unwrap()).unwrap();
    wrapper.duration = Duration::new(Time::new(3, 1).unwrap()).unwrap();
    wrapper.nodes = vec![n];
    wrapper.root_nodes = vec![wrapper.nodes[0].id];
    let root = wrapper.id;
    p.compositions.push(DocumentObject::Known(wrapper));
    p.expressions.extend([
        DocumentObject::Known(child_expression),
        DocumentObject::Known(parent_expression),
    ]);
    (p, root, instance)
}
fn pixels(snapshot: &RenderSnapshot, time: Time, cache: &mut RenderCache) -> Vec<[f32; 4]> {
    use kronello_render::RenderBackend;
    let scene = build_scene_ir_with_cache(snapshot, time, &[], cache).unwrap();
    let dag = kronello_render::build_render_dag(
        &scene,
        snapshot.profile(),
        kronello_render::OutputRegion {
            origin: [0.; 2],
            extent: [64.; 2],
            pixels: [64; 2],
        },
    )
    .unwrap();
    kronello_gpu::render_adapter::CpuReferenceBackend
        .execute(&dag)
        .unwrap()
        .linear
}
#[test]
fn loop_hold_and_affine_parent_inputs_use_canonical_source_clock() {
    use kronello_time::{ProtectedMiddleMode, TimeMap};
    for mode in [ProtectedMiddleMode::Loop, ProtectedMiddleMode::Hold] {
        let map = TimeMap::protected(
            Duration::new(Time::ONE).unwrap(),
            Duration::new(Time::new(3, 1).unwrap()).unwrap(),
            Duration::new(Time::new(1, 10).unwrap()).unwrap(),
            Duration::new(Time::new(1, 10).unwrap()).unwrap(),
            mode,
        )
        .unwrap();
        let (p, root, _) = clock_fixture(map);
        let snapshot = RenderSnapshot::new(&p, root, 0, Default::default()).unwrap();
        let mut cache = RenderCache::default();
        let first = pixels(&snapshot, Time::new(1, 2).unwrap(), &mut cache);
        for time in [
            Time::new(13, 10).unwrap(),
            Time::new(21, 10).unwrap(),
            Time::new(1, 2).unwrap(),
        ] {
            assert_eq!(first, pixels(&snapshot, time, &mut cache));
            assert_eq!(
                first,
                pixels(
                    &snapshot,
                    time,
                    &mut RenderCache::new(CacheConfig::disabled())
                )
            );
        }
        assert!(first.iter().any(|p| p[3] > 0.));
    }
    let map = TimeMap::linear(Time::ZERO, Time::new(2, 1).unwrap()).unwrap();
    let (p, root, instance) = clock_fixture(map);
    let snapshot = RenderSnapshot::new(&p, root, 0, Default::default()).unwrap();
    let scene = build_scene_ir_with_cache(
        &snapshot,
        Time::new(1, 4).unwrap(),
        &[],
        &mut RenderCache::default(),
    )
    .unwrap();
    let xs: Vec<_> = scene
        .nodes
        .iter()
        .filter(|n| {
            n.key.instance_path.ids().first() == Some(&instance)
                && n.key.instance_path.ids().len() == 1
        })
        .map(|n| n.world_transform.0[0][2])
        .collect();
    let min = xs.iter().copied().fold(f64::INFINITY, f64::min);
    let max = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    assert!(
        (max - min - 0.3).abs() < 1e-10,
        "canonical parent p=q/2 must sample the birth velocity, {xs:?}"
    );
}

#[test]
fn nested_instance_inputs_use_composed_canonical_source_clock() {
    use kronello_time::TimeMap;
    let (mut p, wrapper_root, instance) =
        clock_fixture(TimeMap::linear(Time::ZERO, Time::new(2, 1).unwrap()).unwrap());
    let DocumentObject::Known(wrapper) = p
        .compositions
        .iter()
        .find(|c| matches!(c, DocumentObject::Known(c) if c.id == wrapper_root))
        .unwrap()
    else {
        panic!()
    };
    let mut outer = wrapper.clone();
    outer.id = CompositionId::new();
    let mut node = wrapper.nodes[0].clone();
    node.id = NodeId::new();
    let outer_instance = CompositionInstanceId::new();
    node.kind = NodeKind::CompositionInstance(CompositionInstance {
        id: outer_instance,
        definition_ref: wrapper_root,
        input_bindings: Default::default(),
        local_time_map: TimeMap::linear(Time::ZERO, Time::new(2, 1).unwrap()).unwrap(),
        seed: 0,
    });
    outer.nodes = vec![node];
    outer.root_nodes = vec![outer.nodes[0].id];
    let outer_root = outer.id;
    p.compositions.push(DocumentObject::Known(outer));
    let nested = RenderSnapshot::new(&p, outer_root, 0, Default::default()).unwrap();
    let single = RenderSnapshot::new(&p, wrapper_root, 0, Default::default()).unwrap();
    // Nested q=2u inside u=2t composes to emitter local q=4t; birth inputs must
    // still sample the parent expression at the wrapper canonical u=q/2, which
    // matches the single-level wrapper rendered at u=2t.
    for (outer_time, wrapper_time) in [
        (Time::new(1, 4).unwrap(), Time::new(1, 2).unwrap()),
        (Time::new(2, 5).unwrap(), Time::new(4, 5).unwrap()),
        (Time::new(1, 10).unwrap(), Time::new(1, 5).unwrap()),
    ] {
        for cache in [
            &mut RenderCache::default(),
            &mut RenderCache::new(CacheConfig::disabled()),
        ] {
            let nested_scene = build_scene_ir_with_cache(&nested, outer_time, &[], cache).unwrap();
            let single_scene =
                build_scene_ir_with_cache(&single, wrapper_time, &[], cache).unwrap();
            let xs = |scene: &kronello_render::SceneIr| {
                let mut xs: Vec<_> = scene
                    .nodes
                    .iter()
                    .filter(|n| n.key.instance_path.ids().contains(&instance))
                    .map(|n| n.world_transform.0[0][2])
                    .collect();
                xs.sort_by(f64::total_cmp);
                xs
            };
            let (nested_xs, single_xs) = (xs(&nested_scene), xs(&single_scene));
            assert!(
                nested_xs.len() > 1 && nested_xs == single_xs,
                "nested clock {nested_xs:?} must equal single-level clock {single_xs:?}"
            );
        }
    }
}

#[test]
fn empty_checkpoint_payload_respects_zero_and_tiny_byte_capacity() {
    use kronello_render::CacheCapacity;
    let (mut p, root) = fixture();
    let registry = kronello_render::render_registry();
    let DocumentObject::Known(main) = p
        .compositions
        .iter_mut()
        .find(|c| matches!(c,DocumentObject::Known(c)if c.id==root))
        .unwrap()
    else {
        panic!()
    };
    main.nodes[0].properties[4]
        .set_source(PropertySource::Constant(Value::Bool(false)), &registry)
        .unwrap();
    let snapshot = RenderSnapshot::new(&p, root, 0, Default::default()).unwrap();
    for bytes in [0, 128, 256, 1024] {
        let mut cache = RenderCache::new(CacheConfig {
            simulation: CacheCapacity {
                entries: 100,
                bytes,
            },
            ..Default::default()
        });
        for _ in 0..2 {
            build_scene_ir_with_cache(&snapshot, Time::new(4, 5).unwrap(), &[], &mut cache)
                .unwrap();
        }
        let stats = cache.stats().simulation;
        assert!(128 * (stats.checkpoints + stats.cached_particles) <= bytes);
        if bytes < 256 {
            assert_eq!(stats.checkpoints, 0);
            assert_eq!(stats.checkpoint_hits, 0);
        }
    }
}

#[test]
fn actual_reverse_sequence_references_forward_source_states() {
    use kronello_time::{FrameRate, SampleRate, TimeMap, TimeRange};
    let (mut p, root) = fixture();
    let clip = Clip {
        id: ClipId::new(),
        source_ref: SourceRef::Composition { composition: root },
        timeline_range: TimeRange::new(Time::ZERO, Time::ONE).unwrap(),
        source_in: Time::ONE,
        time_map: TimeMap::linear(Time::ZERO, Time::ONE).unwrap(),
        audio_retime: AudioRetimePolicy::ReverseResampleV1,
        reverse_sampling: Some(ReverseSampling::ReverseGridV1),
        volume: None,
        links: vec![],
        effects: vec![],
        properties: vec![],
        markers: vec![],
    };
    let id = SequenceId::new();
    p.sequences.push(DocumentObject::Known(Sequence {
        id,
        extent: DesignExtent::new(64., 64.).unwrap(),
        frame_rate: FrameRate::new(24, 1).unwrap(),
        audio_rate: SampleRate::new(48000).unwrap(),
        working_space: ColorSpace::LinearRec709,
        tracks: vec![Track {
            id: TrackId::new(),
            kind: TrackKind::Video,
            state: None,
            clips: vec![clip],
        }],
        transitions: vec![],
        markers: vec![],
        work_area: None,
    }));
    let reverse = RenderSnapshot::for_target(
        &p,
        kronello_render::RenderTarget::Sequence { sequence: id },
        0,
        Default::default(),
    )
    .unwrap();
    let mut forward = p.clone();
    let DocumentObject::Known(s) = &mut forward.sequences[0] else {
        panic!()
    };
    s.tracks[0].clips[0].source_in = Time::ZERO;
    s.tracks[0].clips[0].reverse_sampling = None;
    s.tracks[0].clips[0].audio_retime = AudioRetimePolicy::ResampleV1;
    let forward = RenderSnapshot::for_target(
        &forward,
        kronello_render::RenderTarget::Sequence { sequence: id },
        0,
        Default::default(),
    )
    .unwrap();
    let mut reverse_cache = RenderCache::default();
    for time in [
        Time::new(1, 5).unwrap(),
        Time::new(3, 5).unwrap(),
        Time::ZERO,
        Time::new(1, 5).unwrap(),
    ] {
        let q = reverse_grid_time(
            Time::ONE.checked_sub(time).unwrap(),
            Time::new(24, 1).unwrap(),
        )
        .unwrap();
        assert_eq!(
            pixels(&reverse, time, &mut reverse_cache),
            pixels(&forward, q, &mut RenderCache::new(CacheConfig::disabled()))
        );
    }
}

fn repeat_parent(
    p: &mut Project,
    source: CompositionId,
    seed: u64,
) -> (CompositionId, ContentId, CompositionInstanceId) {
    let DocumentObject::Known(source_comp) = p
        .compositions
        .iter()
        .find(|c| matches!(c,DocumentObject::Known(c)if c.id==source))
        .unwrap()
    else {
        panic!()
    };
    let source_comp = source_comp.clone();
    let mut comp = source_comp.clone();
    comp.id = CompositionId::new();
    comp.properties = Vec::new();
    let mut node = source_comp.nodes[0].clone();
    node.id = NodeId::new();
    node.properties = Vec::new();
    let id = ContentId::new();
    node.kind = NodeKind::Repeater { content_ref: id };
    comp.nodes = vec![node];
    comp.root_nodes = vec![comp.nodes[0].id];
    let instance = CompositionInstanceId::new();
    p.repeaters.push(DocumentObject::Known(Repeater {
        id,
        version: 1,
        source: RepeatSource {
            composition: source,
            root: source_comp.root_nodes[0],
        },
        instances: vec![RepeatInstance {
            id: instance,
            placement: NodeId::new(),
            seed,
            enabled: true,
            active_range: source_comp.nodes[0].active_range,
            local_time_map: kronello_time::TimeMap::linear(Time::ZERO, Time::ONE).unwrap(),
            properties: vec![],
            effects: vec![],
            input_bindings: Default::default(),
            expanded_source: None,
        }],
    }));
    let root = comp.id;
    p.compositions.push(DocumentObject::Known(comp));
    (root, id, instance)
}
#[test]
fn nested_expansion_preserves_simulation_birth_jitter_and_noise_identity() {
    use kronello_service::{BackendSelection, EditCommand, Request, ResultData, Service};
    use serde_json::json;
    let (mut p, emitter) = fixture();
    let registry = kronello_render::render_registry();
    let DocumentObject::Known(c) = p
        .compositions
        .iter_mut()
        .find(|c| matches!(c,DocumentObject::Known(c)if c.id==emitter))
        .unwrap()
    else {
        panic!()
    };
    c.nodes[0].properties[2]
        .set_source(
            PropertySource::Constant(Value::Vec2([
                FiniteF64::new(5.).unwrap(),
                FiniteF64::new(3.).unwrap(),
            ])),
            &registry,
        )
        .unwrap();
    let noise = Expression {
        id: ExpressionId::new(),
        version: 1,
        value_type: ValueType::Scalar,
        budget: Default::default(),
        nodes: vec![
            ExpressionNode::Time,
            ExpressionNode::Noise {
                seed: 13,
                element: 7,
                input: 0,
            },
            ExpressionNode::Literal(Value::Scalar(FiniteF64::new(1.).unwrap())),
            ExpressionNode::Add { left: 1, right: 2 },
            ExpressionNode::Literal(Value::Scalar(FiniteF64::new(2.).unwrap())),
            ExpressionNode::Divide { left: 3, right: 4 },
        ],
    };
    let DocumentObject::Known(source) = &mut p.compositions[0] else {
        panic!()
    };
    source.nodes[0]
        .properties
        .iter_mut()
        .find(|p| p.descriptor().key.as_str() == "kronello.opacity")
        .unwrap()
        .set_source(PropertySource::Expression(noise.id), &registry)
        .unwrap();
    p.expressions.push(DocumentObject::Known(noise));
    let (inner, inner_id, inner_instance) = repeat_parent(&mut p, emitter, 11);
    let (root, outer_id, outer_instance) = repeat_parent(&mut p, inner, 23);
    let original = p.clone();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested-simulation.kronello");
    let service = Service::new(BackendSelection::CpuReference);
    let run = |v| {
        service
            .dispatch(serde_json::from_value::<Request>(v).unwrap())
            .unwrap()
    };
    run(json!({"operation":"project.create","project":path,"document":p}));
    let before: Vec<_> = [Time::new(1, 5).unwrap(), Time::new(4, 5).unwrap()]
        .into_iter()
        .map(|t| {
            pixels(
                &RenderSnapshot::new(&p, root, 1, Default::default()).unwrap(),
                t,
                &mut RenderCache::default(),
            )
        })
        .collect();
    for (repeater, instance, revision) in
        [(inner_id, inner_instance, 1), (outer_id, outer_instance, 2)]
    {
        let ResultData::Plan(plan) = run(
            json!({"operation":"edit.plan","project":path,"base_revision":revision.to_string(),"commands":[EditCommand::RepeaterExpand{repeater,instance,expansion_id:uuid::Uuid::new_v4()}]}),
        ) else {
            panic!()
        };
        run(
            json!({"operation":"edit.apply","project":path,"base_revision":revision.to_string(),"session_id":uuid::Uuid::new_v4(),"idempotency_key":format!("expand-{revision}"),"plan_hash":plan.plan_hash,"commands":plan.commands}),
        );
        let ResultData::Export(export) = run(json!({"operation":"project.export","project":path}))
        else {
            panic!()
        };
        p = export.document;
        for (time, expected) in [Time::new(1, 5).unwrap(), Time::new(4, 5).unwrap()]
            .into_iter()
            .zip(&before)
        {
            assert_eq!(
                &pixels(
                    &RenderSnapshot::new(&p, root, revision + 1, Default::default()).unwrap(),
                    time,
                    &mut RenderCache::default()
                ),
                expected
            );
        }
    }
    assert_eq!(
        &p.compositions[..original.compositions.len()],
        original.compositions.as_slice()
    );
    let aliases = p.simulation_context().unwrap();
    assert!(!aliases.is_empty());
    let DocumentObject::Known(original_emitter) = &original.simulations[0] else {
        panic!()
    };
    assert!(
        aliases
            .values()
            .all(|target| *target == original_emitter.id)
    );
    let mut reordered = p.clone();
    reordered.repeaters.reverse();
    assert_eq!(aliases, reordered.simulation_context().unwrap());
    let mut malformed = p.clone();
    let DocumentObject::Known(r) = malformed
        .repeaters
        .iter_mut()
        .find(|r| matches!(r,DocumentObject::Known(r)if r.id==outer_id))
        .unwrap()
    else {
        panic!()
    };
    let expanded = r.instances[0].expanded_source.as_mut().unwrap();
    let keys: Vec<_> = expanded.simulation_aliases.keys().copied().collect();
    assert!(keys.len() >= 2);
    expanded.simulation_aliases.insert(keys[0], keys[1]);
    expanded.simulation_aliases.insert(keys[1], keys[0]);
    assert_eq!(
        malformed.simulation_context().unwrap_err().code(),
        "SIMULATION_IDENTITY"
    );
    malformed.repeaters.reverse();
    assert_eq!(
        malformed.simulation_context().unwrap_err().code(),
        "SIMULATION_IDENTITY"
    );
    let mut malformed = p.clone();
    let DocumentObject::Known(r) = malformed
        .repeaters
        .iter_mut()
        .find(|r| matches!(r,DocumentObject::Known(r)if r.id==outer_id))
        .unwrap()
    else {
        panic!()
    };
    let expanded = r.instances[0].expanded_source.as_mut().unwrap();
    expanded.simulation_aliases.insert(keys[0], keys[0]);
    assert_eq!(
        malformed.simulation_context().unwrap_err().code(),
        "SIMULATION_IDENTITY"
    );
}

#[test]
fn transitive_curve_ast_table_and_audio_changes_invalidate_physics() {
    let (mut p, root) = fixture();
    let registry = kronello_render::render_registry();
    let vector = |x| Value::Vec2([FiniteF64::new(x).unwrap(), FiniteF64::new(0.).unwrap()]);
    let curve_id = CurveId::new();
    let curve = |x| {
        AnimationCurve::new(
            curve_id,
            ValueType::Vec2,
            vec![Keyframe {
                time: Time::ZERO,
                value: vector(x),
                interpolation: CurveInterpolation::Linear,
            }],
        )
        .unwrap()
    };
    p.curves.push(DocumentObject::Known(curve(20.)));
    let expression = Expression {
        id: ExpressionId::new(),
        version: 3,
        value_type: ValueType::Vec2,
        budget: Default::default(),
        nodes: vec![ExpressionNode::CurveSample {
            curve: curve_id,
            offset: Time::ZERO,
            value_type: ValueType::Vec2,
        }],
    };
    let DocumentObject::Known(main) = p
        .compositions
        .iter_mut()
        .find(|c| matches!(c,DocumentObject::Known(c)if c.id==root))
        .unwrap()
    else {
        panic!()
    };
    main.nodes[0].properties[1]
        .set_source(PropertySource::Expression(expression.id), &registry)
        .unwrap();
    let expression_id = expression.id;
    p.expressions.push(DocumentObject::Known(expression));
    let time = Time::new(4, 5).unwrap();
    let mut cache = RenderCache::default();
    let mut previous = pixels(
        &RenderSnapshot::new(&p, root, 0, Default::default()).unwrap(),
        time,
        &mut cache,
    );
    let check = |p: &Project, cache: &mut RenderCache, previous: &mut Vec<[f32; 4]>| {
        cache.reset_stats();
        let snapshot = RenderSnapshot::new(p, root, 0, Default::default()).unwrap();
        let changed = pixels(&snapshot, time, cache);
        let stats = cache.stats().simulation;
        assert_eq!(stats.checkpoint_hits, 0);
        assert_eq!(stats.replayed_steps, 8);
        assert_eq!(
            changed,
            pixels(
                &snapshot,
                time,
                &mut RenderCache::new(CacheConfig::disabled())
            )
        );
        assert_ne!(&changed, previous);
        *previous = changed;
    };
    *p.curves
        .iter_mut()
        .find(|c| matches!(c,DocumentObject::Known(c)if c.id()==curve_id))
        .unwrap() = DocumentObject::Known(curve(40.));
    check(&p, &mut cache, &mut previous);
    let expr = p
        .expressions
        .iter_mut()
        .find_map(|e| match e {
            DocumentObject::Known(e) if e.id == expression_id => Some(e),
            _ => None,
        })
        .unwrap();
    expr.nodes = vec![ExpressionNode::Literal(vector(60.))];
    check(&p, &mut cache, &mut previous);
    let data = ExpressionDataAsset::new(
        AssetId::new(),
        DataTable {
            columns: std::collections::BTreeMap::from([("velocity".into(), ValueType::Vec2)]),
            rows: vec![std::collections::BTreeMap::from([(
                "velocity".into(),
                vector(80.),
            )])],
        },
    )
    .unwrap();
    let data_id = data.id;
    p.expression_data_assets.push(DocumentObject::Known(data));
    let expr = p
        .expressions
        .iter_mut()
        .find_map(|e| match e {
            DocumentObject::Known(e) if e.id == expression_id => Some(e),
            _ => None,
        })
        .unwrap();
    expr.nodes = vec![
        ExpressionNode::Literal(Value::Scalar(FiniteF64::new(0.).unwrap())),
        ExpressionNode::DataAssetCell {
            asset: data_id,
            column: "velocity".into(),
            row: 0,
            value_type: ValueType::Vec2,
        },
    ];
    check(&p, &mut cache, &mut previous);
    let DocumentObject::Known(data) = &mut p.expression_data_assets[0] else {
        panic!()
    };
    data.table.rows[0].insert("velocity".into(), vector(100.));
    data.content_hash = data.computed_hash().unwrap();
    check(&p, &mut cache, &mut previous);
    let audio_id = AssetId::new();
    p.audio_analyses
        .push(DocumentObject::Known(AudioAnalysisDataAsset {
            id: audio_id,
            source: AudioAnalysisSource::Bus {
                snapshot_hash: "b".repeat(64),
                target: "fixed".into(),
                evaluator_version: 2,
            },
            config: AudioAnalysisConfig {
                version: 1,
                sample_rate: 48000,
                window: 32,
                hop: 32,
                bands: vec![],
                time_map: kronello_time::TimeMap::linear(Time::ZERO, Time::new(1, 2000).unwrap())
                    .unwrap(),
            },
            start_sample: 0,
            sample_count: 32,
            frames: vec![AudioAnalysisFrame {
                time: Time::ZERO,
                rms: 0.25,
                band_energy: vec![],
                onset: 0.,
                beat: false,
            }],
        }));
    let expr = p
        .expressions
        .iter_mut()
        .find_map(|e| match e {
            DocumentObject::Known(e) if e.id == expression_id => Some(e),
            _ => None,
        })
        .unwrap();
    expr.nodes = vec![
        ExpressionNode::AudioFeature {
            asset: audio_id,
            feature: AudioFeature::Rms,
            offset: Time::ZERO,
        },
        ExpressionNode::Literal(Value::Scalar(FiniteF64::new(200.).unwrap())),
        ExpressionNode::Multiply { left: 0, right: 1 },
        ExpressionNode::Literal(Value::Scalar(FiniteF64::new(0.).unwrap())),
        ExpressionNode::Vec2 { x: 2, y: 3 },
    ];
    check(&p, &mut cache, &mut previous);
    let DocumentObject::Known(audio) = &mut p.audio_analyses[0] else {
        panic!()
    };
    audio.frames[0].rms = 0.5;
    check(&p, &mut cache, &mut previous);
}

#[test]
fn nested_piecewise_and_past_property_sample_dynamics_are_not_baked() {
    use kronello_time::{TimeMap, TimeMapPoint};
    let piecewise = TimeMap::piecewise_linear(vec![
        TimeMapPoint {
            parent: Time::ZERO,
            local: Time::ZERO,
        },
        TimeMapPoint {
            parent: Time::new(1, 2).unwrap(),
            local: Time::new(4, 5).unwrap(),
        },
        TimeMapPoint {
            parent: Time::ONE,
            local: Time::ONE,
        },
    ])
    .unwrap();
    let (p, root, instance) = clock_fixture(piecewise);
    let snapshot = RenderSnapshot::new(&p, root, 0, Default::default()).unwrap();
    let scene = build_scene_ir_with_cache(
        &snapshot,
        Time::new(1, 2).unwrap(),
        &[],
        &mut RenderCache::default(),
    )
    .unwrap();
    let max = scene
        .nodes
        .iter()
        .filter(|n| n.key.instance_path.ids() == [instance])
        .map(|n| n.world_transform.0[0][2])
        .fold(0., f64::max);
    assert!(
        (max - 1.).abs() < 1e-10,
        "piecewise inverse samples birth velocities, got {max}"
    );
    let (mut p, root, instance) = clock_fixture(TimeMap::linear(Time::ZERO, Time::ONE).unwrap());
    let registry = kronello_render::render_registry();
    let scalar_descriptor = registry
        .lookup(&SchemaKey::new("kronello.simulation.velocity").unwrap())
        .unwrap();
    let curve = AnimationCurve::new(
        CurveId::new(),
        ValueType::Vec2,
        vec![
            Keyframe {
                time: Time::ZERO,
                value: Value::Vec2([FiniteF64::new(0.).unwrap(); 2]),
                interpolation: CurveInterpolation::Linear,
            },
            Keyframe {
                time: Time::ONE,
                value: Value::Vec2([FiniteF64::new(10.).unwrap(), FiniteF64::new(0.).unwrap()]),
                interpolation: CurveInterpolation::Linear,
            },
        ],
    )
    .unwrap();
    let property = Property::new(
        PropertyId::new(),
        DescriptorRef::new(scalar_descriptor),
        PropertySource::Curve(curve.id()),
        vec![],
        &registry,
    )
    .unwrap();
    let DocumentObject::Known(wrapper) = p.compositions.last_mut().unwrap() else {
        panic!()
    };
    let node = wrapper.nodes[0].id;
    wrapper.nodes[0].properties.push(property.clone());
    p.curves.push(DocumentObject::Known(curve));
    let DocumentObject::Known(parent_expression) = p.expressions.last_mut().unwrap() else {
        panic!()
    };
    parent_expression.version = 3;
    parent_expression.nodes = vec![
        ExpressionNode::Literal(Value::Scalar(FiniteF64::new(0.1).unwrap())),
        ExpressionNode::PropertySample {
            node: Some(node),
            property: property.id(),
            value_type: ValueType::Vec2,
            lookback: 0,
        },
    ];
    let snapshot = RenderSnapshot::new(&p, root, 0, Default::default()).unwrap();
    let scene = build_scene_ir_with_cache(
        &snapshot,
        Time::new(1, 2).unwrap(),
        &[],
        &mut RenderCache::default(),
    )
    .unwrap();
    let max = scene
        .nodes
        .iter()
        .filter(|n| n.key.instance_path.ids() == [instance])
        .map(|n| n.world_transform.0[0][2])
        .fold(0., f64::max);
    assert!(
        (max - 0.3).abs() < 1e-10,
        "birth at .2 samples parent curve at .1, got {max}"
    );
}

#[test]
fn simulation_sources_render_template_bands_and_expanded_layout_changes_invalidate() {
    use kronello_service::{BackendSelection, EditCommand, Request, ResultData, Service};
    use serde_json::{Value as Json, json};
    use std::collections::BTreeMap;
    let mut p: Project = serde_json::from_str(include_str!(
        "../../../examples/m5-repeat-template.project.json"
    ))
    .unwrap();
    let DocumentObject::Known(repeat) = &p.repeaters[0] else {
        panic!()
    };
    let repeat = repeat.clone();
    let root = p
        .compositions
        .iter()
        .find_map(|c| match c {
            DocumentObject::Known(c)
                if c.nodes.iter().any(
                    |n| matches!(n.kind,NodeKind::Repeater{content_ref}if content_ref==repeat.id),
                ) =>
            {
                Some(c.id)
            }
            _ => None,
        })
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("template-simulation.kronello");
    let service = Service::new(BackendSelection::CpuReference);
    let run = |v| {
        service
            .dispatch(serde_json::from_value::<Request>(v).unwrap())
            .unwrap()
    };
    run(json!({"operation":"project.create","project":path,"document":p}));
    let ResultData::Plan(plan) = run(
        json!({"operation":"edit.plan","project":path,"base_revision":"1","commands":[EditCommand::RepeaterExpand{repeater:repeat.id,instance:repeat.instances[0].id,expansion_id:uuid::Uuid::new_v4()}]}),
    ) else {
        panic!()
    };
    run(
        json!({"operation":"edit.apply","project":path,"base_revision":"1","session_id":uuid::Uuid::new_v4(),"idempotency_key":"expand-template","plan_hash":plan.plan_hash,"commands":plan.commands}),
    );
    let ResultData::Export(export) = run(json!({"operation":"project.export","project":path}))
    else {
        panic!()
    };
    p = export.document;
    // Fresh authored particle source has independent identities; the unchanged
    // template edition continues to use its original text/band content.
    fn remap(value: &mut Json, ids: &mut BTreeMap<uuid::Uuid, uuid::Uuid>) {
        match value {
            Json::String(s) => {
                if let Ok(id) = uuid::Uuid::parse_str(s) {
                    *s = ids.entry(id).or_insert_with(uuid::Uuid::new_v4).to_string();
                }
            }
            Json::Array(a) => {
                for v in a {
                    remap(v, ids)
                }
            }
            Json::Object(o) => {
                for v in o.values_mut() {
                    remap(v, ids)
                }
            }
            _ => {}
        }
    }
    let mut leaf: Json =
        serde_json::from_str(include_str!("../../../examples/ffi-preview.project.json")).unwrap();
    remap(&mut leaf, &mut BTreeMap::new());
    let leaf: Project = serde_json::from_value(leaf).unwrap();
    let DocumentObject::Known(leaf_comp) = &leaf.compositions[0] else {
        panic!()
    };
    let leaf_source = RepeatSource {
        composition: leaf_comp.id,
        root: leaf_comp.root_nodes[0],
    };
    p.compositions.extend(leaf.compositions);
    p.shapes.extend(leaf.shapes);
    p.curves.extend(leaf.curves);
    let (temp, emitter) = fixture();
    let DocumentObject::Known(emitter_comp) = temp
        .compositions
        .iter()
        .find(|c| matches!(c,DocumentObject::Known(c)if c.id==emitter))
        .unwrap()
    else {
        panic!()
    };
    let mut emitter_node = emitter_comp.nodes[0].clone();
    let DocumentObject::Known(record) = &temp.simulations[0] else {
        panic!()
    };
    let mut record = record.clone();
    record.source = leaf_source;
    let DocumentObject::Known(r) = &p.repeaters[0] else {
        panic!()
    };
    let expanded = r.instances[0].expanded_source.as_ref().unwrap();
    let (comp_id, rules) = expanded.layout_constraints.iter().next().unwrap();
    let comp_id = *comp_id;
    let band = rules.bands[0].clone();
    let expr = Expression {
        id: ExpressionId::new(),
        version: 1,
        value_type: ValueType::Vec2,
        budget: Default::default(),
        nodes: vec![ExpressionNode::Property {
            node: Some(band.band_node),
            property: band.size_property,
            value_type: ValueType::Vec2,
        }],
    };
    emitter_node.properties[0]
        .set_source(
            PropertySource::Expression(expr.id),
            &kronello_render::render_registry(),
        )
        .unwrap();
    let DocumentObject::Known(c) = p
        .compositions
        .iter_mut()
        .find(|c| matches!(c,DocumentObject::Known(c)if c.id==comp_id))
        .unwrap()
    else {
        panic!()
    };
    c.root_nodes.push(emitter_node.id);
    c.nodes.push(emitter_node);
    p.expressions.push(DocumentObject::Known(expr));
    p.simulations.push(DocumentObject::Known(record));
    let DocumentObject::Known(text) = &p.texts[0] else {
        panic!()
    };
    let identity = text.styles[0].font.clone();
    let font = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/fixtures/external/NotoSansCJKjp-Regular.otf"),
    )
    .unwrap();
    let fonts = [kronello_text::FontData {
        identity: &identity,
        bytes: &font,
    }];
    let mut cache = RenderCache::default();
    let time = Time::new(4, 5).unwrap();
    let snapshot = RenderSnapshot::new(&p, root, 0, Default::default()).unwrap();
    let before = build_scene_ir_with_cache(&snapshot, time, &fonts, &mut cache).unwrap();
    let mut changed = p.clone();
    let DocumentObject::Known(r) = &mut changed.repeaters[0] else {
        panic!()
    };
    r.instances[0]
        .expanded_source
        .as_mut()
        .unwrap()
        .layout_constraints
        .get_mut(&comp_id)
        .unwrap()
        .bands[0]
        .padding[0] = FiniteF64::new(band.padding[0].get() + 10.).unwrap();
    cache.reset_stats();
    let snapshot = RenderSnapshot::new(&changed, root, 1, Default::default()).unwrap();
    let after = build_scene_ir_with_cache(&snapshot, time, &fonts, &mut cache).unwrap();
    assert_eq!(cache.stats().simulation.checkpoint_hits, 0);
    assert_eq!(cache.stats().simulation.replayed_steps, 8);
    let cold = build_scene_ir_with_cache(
        &snapshot,
        time,
        &fonts,
        &mut RenderCache::new(CacheConfig::disabled()),
    )
    .unwrap();
    assert_eq!(after, cold);
    assert!(before.nodes.iter().any(|n| {
        after
            .nodes
            .iter()
            .any(|m| m.key == n.key && m.world_transform != n.world_transform)
    }));
    use kronello_render::RenderBackend;
    let region = kronello_render::OutputRegion {
        origin: [0.; 2],
        extent: [128.; 2],
        pixels: [64; 2],
    };
    let warm_pixels = kronello_gpu::render_adapter::CpuReferenceBackend
        .execute(&kronello_render::build_render_dag(&after, snapshot.profile(), region).unwrap())
        .unwrap()
        .linear;
    let cold_pixels = kronello_gpu::render_adapter::CpuReferenceBackend
        .execute(&kronello_render::build_render_dag(&cold, snapshot.profile(), region).unwrap())
        .unwrap()
        .linear;
    assert_eq!(warm_pixels, cold_pixels);
    assert!(warm_pixels.iter().any(|p| p[3] > 0.));
}

/// A particle emitter inside the expanded band composition, so kernel inputs
/// may read layout-derived band values. `text_active` bounds the band text
/// node and the band follower itself, keeping scene evaluation clean once the
/// text is gone; `emitter_active` bounds the simulation node; `link_band`
/// decides whether the emitter origin reads the band size property.
fn band_layout_fixture(
    text_active: kronello_time::TimeRange,
    emitter_active: kronello_time::TimeRange,
    link_band: bool,
) -> (Project, CompositionId) {
    use kronello_service::{BackendSelection, EditCommand, Request, ResultData, Service};
    use serde_json::{Value as Json, json};
    use std::collections::BTreeMap;
    let mut p: Project = serde_json::from_str(include_str!(
        "../../../examples/m5-repeat-template.project.json"
    ))
    .unwrap();
    let DocumentObject::Known(repeat) = &p.repeaters[0] else {
        panic!()
    };
    let repeat = repeat.clone();
    let root = p
        .compositions
        .iter()
        .find_map(|c| match c {
            DocumentObject::Known(c)
                if c.nodes.iter().any(
                    |n| matches!(n.kind,NodeKind::Repeater{content_ref}if content_ref==repeat.id),
                ) =>
            {
                Some(c.id)
            }
            _ => None,
        })
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("band-layout-simulation.kronello");
    let service = Service::new(BackendSelection::CpuReference);
    let run = |v| {
        service
            .dispatch(serde_json::from_value::<Request>(v).unwrap())
            .unwrap()
    };
    run(json!({"operation":"project.create","project":path,"document":p}));
    let ResultData::Plan(plan) = run(
        json!({"operation":"edit.plan","project":path,"base_revision":"1","commands":[EditCommand::RepeaterExpand{repeater:repeat.id,instance:repeat.instances[0].id,expansion_id:uuid::Uuid::new_v4()}]}),
    ) else {
        panic!()
    };
    run(
        json!({"operation":"edit.apply","project":path,"base_revision":"1","session_id":uuid::Uuid::new_v4(),"idempotency_key":"expand-band-layout","plan_hash":plan.plan_hash,"commands":plan.commands}),
    );
    let ResultData::Export(export) = run(json!({"operation":"project.export","project":path}))
    else {
        panic!()
    };
    p = export.document;
    // Fresh authored particle source has independent identities; the unchanged
    // template edition continues to use its original text/band content.
    fn remap(value: &mut Json, ids: &mut BTreeMap<uuid::Uuid, uuid::Uuid>) {
        match value {
            Json::String(s) => {
                if let Ok(id) = uuid::Uuid::parse_str(s) {
                    *s = ids.entry(id).or_insert_with(uuid::Uuid::new_v4).to_string();
                }
            }
            Json::Array(a) => {
                for v in a {
                    remap(v, ids)
                }
            }
            Json::Object(o) => {
                for v in o.values_mut() {
                    remap(v, ids)
                }
            }
            _ => {}
        }
    }
    let mut leaf: Json =
        serde_json::from_str(include_str!("../../../examples/ffi-preview.project.json")).unwrap();
    remap(&mut leaf, &mut BTreeMap::new());
    let leaf: Project = serde_json::from_value(leaf).unwrap();
    let DocumentObject::Known(leaf_comp) = &leaf.compositions[0] else {
        panic!()
    };
    let leaf_source = RepeatSource {
        composition: leaf_comp.id,
        root: leaf_comp.root_nodes[0],
    };
    p.compositions.extend(leaf.compositions);
    p.shapes.extend(leaf.shapes);
    p.curves.extend(leaf.curves);
    let (temp, emitter) = fixture();
    let DocumentObject::Known(emitter_comp) = temp
        .compositions
        .iter()
        .find(|c| matches!(c,DocumentObject::Known(c)if c.id==emitter))
        .unwrap()
    else {
        panic!()
    };
    let mut emitter_node = emitter_comp.nodes[0].clone();
    let DocumentObject::Known(record) = &temp.simulations[0] else {
        panic!()
    };
    let mut record = record.clone();
    record.source = leaf_source;
    let DocumentObject::Known(r) = &p.repeaters[0] else {
        panic!()
    };
    let expanded = r.instances[0].expanded_source.as_ref().unwrap();
    let (comp_id, rules) = expanded.layout_constraints.iter().next().unwrap();
    let comp_id = *comp_id;
    let band = rules.bands[0].clone();
    emitter_node.active_range = emitter_active;
    let mut expressions = Vec::new();
    if link_band {
        let expr = Expression {
            id: ExpressionId::new(),
            version: 1,
            value_type: ValueType::Vec2,
            budget: Default::default(),
            nodes: vec![ExpressionNode::Property {
                node: Some(band.band_node),
                property: band.size_property,
                value_type: ValueType::Vec2,
            }],
        };
        emitter_node.properties[0]
            .set_source(
                PropertySource::Expression(expr.id),
                &kronello_render::render_registry(),
            )
            .unwrap();
        expressions.push(expr);
    }
    let DocumentObject::Known(c) = p
        .compositions
        .iter_mut()
        .find(|c| matches!(c,DocumentObject::Known(c)if c.id==comp_id))
        .unwrap()
    else {
        panic!()
    };
    c.nodes
        .iter_mut()
        .find(|n| n.id == band.text_node)
        .unwrap()
        .active_range = text_active;
    c.nodes
        .iter_mut()
        .find(|n| n.id == band.band_node)
        .unwrap()
        .active_range = text_active;
    c.root_nodes.push(emitter_node.id);
    c.nodes.push(emitter_node);
    p.expressions
        .extend(expressions.into_iter().map(DocumentObject::Known));
    p.simulations.push(DocumentObject::Known(record));
    (p, root)
}
fn band_fonts(p: &Project) -> (kronello_model::FontRef, Vec<u8>) {
    let DocumentObject::Known(text) = &p.texts[0] else {
        panic!()
    };
    (
        text.styles[0].font.clone(),
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/fixtures/external/NotoSansCJKjp-Regular.otf"),
        )
        .unwrap(),
    )
}
#[test]
fn simulation_layout_inputs_do_not_depend_on_tick_history() {
    use kronello_render::RenderBackend;
    let full = kronello_time::TimeRange::new(Time::ZERO, Time::new(5, 1).unwrap()).unwrap();
    let emitter_range =
        kronello_time::TimeRange::new(Time::ZERO, Time::new(5, 1).unwrap()).unwrap();
    // The queried source time lands on kernel tick 9; checkpoint stride is 8,
    // so a warm query resumes from the committed tick-8 state and replays one
    // step instead of sampling every tick since the start.
    let time = Time::new(9, 10).unwrap();
    // Band text stays active: sequential, repeated, resumed and disabled
    // evaluation must agree on the full scene and the rendered pixels.
    let (p, root) = band_layout_fixture(full, emitter_range, true);
    let (identity, bytes) = band_fonts(&p);
    let fonts = [kronello_text::FontData {
        identity: &identity,
        bytes: &bytes,
    }];
    let snapshot = RenderSnapshot::new(&p, root, 0, Default::default()).unwrap();
    let mut sequential = RenderCache::default();
    let cold = build_scene_ir_with_cache(&snapshot, time, &fonts, &mut sequential).unwrap();
    let warm = build_scene_ir_with_cache(&snapshot, time, &fonts, &mut sequential).unwrap();
    assert_eq!(cold, warm);
    let mut resumed = RenderCache::default();
    build_scene_ir_with_cache(&snapshot, Time::new(4, 5).unwrap(), &fonts, &mut resumed).unwrap();
    resumed.reset_stats();
    let hit = build_scene_ir_with_cache(&snapshot, time, &fonts, &mut resumed).unwrap();
    assert_eq!(resumed.stats().simulation.checkpoint_hits, 1);
    assert_eq!(resumed.stats().simulation.replayed_steps, 1);
    assert_eq!(cold, hit);
    let disabled = build_scene_ir_with_cache(
        &snapshot,
        time,
        &fonts,
        &mut RenderCache::new(CacheConfig::disabled()),
    )
    .unwrap();
    assert_eq!(cold, disabled);
    let region = kronello_render::OutputRegion {
        origin: [0.; 2],
        extent: [128.; 2],
        pixels: [64; 2],
    };
    let pixels = |ir: &kronello_render::SceneIr| {
        kronello_gpu::render_adapter::CpuReferenceBackend
            .execute(&kronello_render::build_render_dag(ir, snapshot.profile(), region).unwrap())
            .unwrap()
            .linear
    };
    assert_eq!(pixels(&cold), pixels(&hit));
    assert!(pixels(&cold).iter().any(|p| p[3] > 0.));
    // Band text and follower end at kernel tick 5 (1/2s with step 1/10s). The
    // layout-derived input is absent from that tick on, so every consumer must
    // fail with the same typed MissingLayoutInput no matter which ticks were
    // evaluated earlier inside the same runtime: sequential replay cannot keep
    // a stale layout alive for a resumed query to miss, and vice versa.
    let half = kronello_time::TimeRange::new(Time::ZERO, Time::new(1, 2).unwrap()).unwrap();
    let (p, root) = band_layout_fixture(half, half, true);
    let (identity, bytes) = band_fonts(&p);
    let fonts = [kronello_text::FontData {
        identity: &identity,
        bytes: &bytes,
    }];
    let snapshot = RenderSnapshot::new(&p, root, 0, Default::default()).unwrap();
    let mut sequential = RenderCache::default();
    let mut resumed = RenderCache::default();
    let _ = build_scene_ir_with_cache(&snapshot, Time::new(4, 5).unwrap(), &fonts, &mut resumed);
    let mut disabled = RenderCache::new(CacheConfig::disabled());
    let results = [
        build_scene_ir_with_cache(&snapshot, time, &fonts, &mut sequential).map(|_| ()),
        build_scene_ir_with_cache(&snapshot, time, &fonts, &mut sequential).map(|_| ()),
        build_scene_ir_with_cache(&snapshot, time, &fonts, &mut resumed).map(|_| ()),
        build_scene_ir_with_cache(&snapshot, time, &fonts, &mut disabled).map(|_| ()),
    ];
    let errors: Vec<String> = results
        .iter()
        .map(|result| match result {
            Err(kronello_render::RenderError::Evaluation(
                kronello_eval::EvaluationError::MissingLayoutInput(_),
            )) => result.as_ref().unwrap_err().to_string(),
            _ => panic!(
                "every evaluation mode must fail with MissingLayoutInput, got {:?}",
                results
                    .iter()
                    .map(|r| r.as_ref().map(|_| ()).map_err(|e| e.to_string()))
                    .collect::<Vec<_>>()
            ),
        })
        .collect();
    assert!(
        errors.iter().all(|e| *e == errors[0]),
        "the missing layout input must be identical across evaluation modes: {errors:?}"
    );
    // A checkpoint already committed the ticks whose layouts needed the font:
    // resuming after them does not re-consult skipped ticks' font data, while
    // cold replay samples them and reports the typed hash error. Both are
    // pure-per-tick results; the resumed state is the one committed earlier.
    let (p, root) = band_layout_fixture(half, emitter_range, false);
    let (identity, bytes) = band_fonts(&p);
    let good = [kronello_text::FontData {
        identity: &identity,
        bytes: &bytes,
    }];
    let mut corrupt_bytes = bytes.clone();
    corrupt_bytes[0] ^= 0xff;
    let corrupt = [kronello_text::FontData {
        identity: &identity,
        bytes: &corrupt_bytes,
    }];
    let snapshot = RenderSnapshot::new(&p, root, 0, Default::default()).unwrap();
    let mut resumed = RenderCache::default();
    build_scene_ir_with_cache(&snapshot, Time::new(4, 5).unwrap(), &good, &mut resumed).unwrap();
    resumed.reset_stats();
    let hit = build_scene_ir_with_cache(&snapshot, time, &corrupt, &mut resumed).unwrap();
    assert_eq!(resumed.stats().simulation.checkpoint_hits, 1);
    assert_eq!(resumed.stats().simulation.replayed_steps, 1);
    let clean = build_scene_ir_with_cache(
        &snapshot,
        time,
        &good,
        &mut RenderCache::new(CacheConfig::disabled()),
    )
    .unwrap();
    assert_eq!(hit, clean);
    let cold = build_scene_ir_with_cache(
        &snapshot,
        time,
        &corrupt,
        &mut RenderCache::new(CacheConfig::disabled()),
    );
    assert_eq!(cold.unwrap_err().code(), "ASSET_HASH_MISMATCH");
}
