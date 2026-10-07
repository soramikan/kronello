use kronello_model::*;
use kronello_render::{RenderSnapshot, RenderTarget};
use kronello_service::*;
use kronello_time::{Rational, Time, TimeMap};
use serde_json::{Value as Json, json};
use uuid::Uuid;
fn write_fixture(project: &Project, name: &str) {
    if let Some(root) = std::env::var_os("KRONELLO_WRITE_REPEAT_FIXTURE") {
        let path = std::path::Path::new(&root).join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, serde_json::to_vec_pretty(project).unwrap()).unwrap();
    }
}
fn run(request: Json) -> Result<ResultData, ServiceError> {
    Service::new(BackendSelection::CpuReference).dispatch(serde_json::from_value(request).unwrap())
}
fn property(key: &str, value: Value) -> Property {
    let r = kronello_render::render_registry();
    let d = r.lookup(&SchemaKey::new(key).unwrap()).unwrap();
    Property::new(
        PropertyId::new(),
        DescriptorRef::new(d),
        PropertySource::Constant(value),
        vec![],
        &r,
    )
    .unwrap()
}
fn fixture() -> (Project, CompositionId, ContentId) {
    let mut p: Project =
        serde_json::from_str(include_str!("../../../examples/ffi-preview.project.json")).unwrap();
    let DocumentObject::Known(source) = &mut p.compositions[0] else {
        panic!()
    };
    let source_id = source.id;
    let source_root = source.root_nodes[0];
    // A nested definition exercises fresh copied nested IDs and noise aliases.
    let inner = source.clone();
    let mut wrapper = source.clone();
    wrapper.id = CompositionId::new();
    wrapper.root_nodes = vec![NodeId::new()];
    let mut placement = inner.nodes[0].clone();
    placement.id = wrapper.root_nodes[0];
    placement.properties = vec![];
    placement.kind = NodeKind::CompositionInstance(CompositionInstance {
        id: CompositionInstanceId::new(),
        definition_ref: source_id,
        input_bindings: Default::default(),
        local_time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
        seed: 123,
    });
    wrapper.nodes = vec![placement];
    let repeat_source = RepeatSource {
        composition: wrapper.id,
        root: wrapper.root_nodes[0],
    };
    let mut parent = wrapper.clone();
    parent.id = CompositionId::new();
    parent.root_nodes = vec![NodeId::new()];
    let repeat_id = ContentId::new();
    let mut node = parent.nodes[0].clone();
    node.id = parent.root_nodes[0];
    node.kind = NodeKind::Repeater {
        content_ref: repeat_id,
    };
    node.name = Some("Authored repeater".into());
    parent.nodes = vec![node];
    let instances = (0..3)
        .map(|index| RepeatInstance {
            id: CompositionInstanceId::new(),
            placement: NodeId::new(),
            seed: 100 + index,
            enabled: true,
            active_range: inner.nodes[0].active_range,
            local_time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
            properties: vec![property(
                "kronello.transform.position",
                Value::Vec2([
                    FiniteF64::new(index as f64 * 20.).unwrap(),
                    FiniteF64::new(0.).unwrap(),
                ]),
            )],
            effects: vec![],
            input_bindings: Default::default(),
            expanded_source: None,
        })
        .collect();
    p.repeaters.push(DocumentObject::Known(Repeater {
        id: repeat_id,
        version: 1,
        source: repeat_source,
        instances,
    }));
    p.compositions.push(DocumentObject::Known(wrapper));
    let id = parent.id;
    p.compositions.push(DocumentObject::Known(parent));
    let r = kronello_render::render_registry();
    let expression = Expression {
        id: ExpressionId::new(),
        version: 1,
        value_type: ValueType::Scalar,
        budget: Default::default(),
        nodes: vec![
            ExpressionNode::Time,
            ExpressionNode::Noise {
                seed: 7,
                element: 9,
                input: 0,
            },
            ExpressionNode::Literal(Value::Scalar(FiniteF64::new(1.).unwrap())),
            ExpressionNode::Add { left: 1, right: 2 },
            ExpressionNode::Literal(Value::Scalar(FiniteF64::new(2.).unwrap())),
            ExpressionNode::Divide { left: 3, right: 4 },
        ],
    };
    let mut opacity = property(
        "kronello.opacity",
        Value::Scalar(FiniteF64::new(1.).unwrap()),
    );
    opacity
        .set_source(PropertySource::Expression(expression.id), &r)
        .unwrap();
    let DocumentObject::Known(source) = &mut p.compositions[0] else {
        panic!()
    };
    assert_eq!(source.root_nodes, [source_root]);
    source.nodes[0]
        .properties
        .retain(|p| p.descriptor().key.as_str() != "kronello.opacity");
    source.nodes[0].properties.push(opacity);
    p.expressions.push(DocumentObject::Known(expression));
    // The shared source is also an immutable template edition. Expand must not edit it.
    let source = p.repeater(repeat_id).unwrap().source.composition;
    let hash = kronello_template::authoring_hash(&p, source).unwrap();
    p.templates.push(DocumentObject::Known(TemplateDefinition {
        id: Uuid::new_v4(),
        template_id: Uuid::new_v4(),
        version: "1.0.0".into(),
        composition_ref: source,
        public_inputs: Default::default(),
        variants: Default::default(),
        duration_policy: TemplateDurationPolicy {
            intro: kronello_time::Duration::ZERO,
            outro: kronello_time::Duration::ZERO,
            minimum_middle: kronello_time::Duration::ZERO,
            middle_mode: TemplateMiddleMode::Stretch,
        },
        constraints: Default::default(),
        content_hash: hash,
    }));
    (p, id, repeat_id)
}
fn export(path: &std::path::Path) -> ExportResult {
    let ResultData::Export(r) = run(json!({"operation":"project.export","project":path})).unwrap()
    else {
        panic!()
    };
    *r
}
fn image(path: &std::path::Path, composition: CompositionId, time: Time) -> FrameResult {
    let ResultData::Frame(r)=run(json!({"operation":"render.frame","input":{"project":path,"target":RenderTarget::Composition{composition},"region":{"origin":[0.,0.],"extent":[64.,32.],"pixels":[64,32]}},"time":time})).unwrap()else{panic!()};
    *r
}
fn apply(path: &std::path::Path, commands: Vec<EditCommand>, key: &str) -> kronello_store::Event {
    let previous = export(path);
    let ResultData::Plan(plan)=run(json!({"operation":"edit.plan","project":path,"base_revision":previous.revision,"commands":commands})).unwrap()else{panic!()};
    let request = json!({"operation":"edit.apply","project":path,"base_revision":previous.revision,"commands":plan.commands,"plan_hash":plan.plan_hash,"session_id":Uuid::new_v4(),"idempotency_key":key});
    let ResultData::Edit(result) = run(request.clone()).unwrap() else {
        panic!()
    };
    let ResultData::Edit(replay) = run(request).unwrap() else {
        panic!()
    };
    assert_eq!(result, replay);
    result
}
#[test]
fn expand_is_explicit_independent_seed_preserving_and_undoable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("repeat.kronello");
    let (p, root, id) = fixture();
    write_fixture(&p, "repeat.project.json");
    run(json!({"operation":"project.create","project":path,"document":p})).unwrap();
    let original = export(&path);
    let times = [
        Time::ZERO,
        Time::new(1, 2).unwrap(),
        Time::new(3, 4).unwrap(),
    ];
    let frames: Vec<_> = times
        .iter()
        .map(|t| image(&path, root, *t).linear)
        .collect();
    assert!(frames.iter().flatten().any(|p| p[3] > 0.));
    assert_ne!(frames[0], frames[1]);
    let DocumentObject::Known(repeater) = &original.document.repeaters[0] else {
        panic!()
    };
    let instance = repeater.instances[1].id;
    let seed = repeater.instances[1].seed;
    let event = apply(
        &path,
        vec![EditCommand::RepeaterExpand {
            repeater: id,
            instance,
            expansion_id: Uuid::new_v4(),
        }],
        "expand",
    );
    let expanded = export(&path);
    assert_eq!(
        &expanded.document.compositions[..original.document.compositions.len()],
        original.document.compositions.as_slice()
    );
    assert_eq!(
        &expanded.document.shapes[..original.document.shapes.len()],
        original.document.shapes.as_slice()
    );
    assert_eq!(expanded.document.templates, original.document.templates);
    let DocumentObject::Known(r) = &expanded.document.repeaters[0] else {
        panic!()
    };
    assert_eq!(r.instances[1].id, instance);
    assert_eq!(r.instances[1].seed, seed);
    assert!(r.instances[1].expanded_source.is_some());
    assert!(r.instances[0].expanded_source.is_none());
    for (t, expected) in times.iter().zip(&frames) {
        assert_eq!(
            image(&path, root, *t).linear,
            *expected,
            "expand preserves nested noise and pixels"
        );
    }
    // Editing the copied leaf changes only this instance, retaining the shared original.
    let source_id = r.instances[1]
        .expanded_source
        .as_ref()
        .unwrap()
        .source
        .composition;
    let wrapper = expanded
        .document
        .compositions
        .iter()
        .find_map(|c| {
            if let DocumentObject::Known(c) = c {
                (c.id == source_id).then_some(c)
            } else {
                None
            }
        })
        .unwrap();
    let NodeKind::CompositionInstance(nested) = &wrapper.nodes[0].kind else {
        panic!()
    };
    let copied_leaf = expanded
        .document
        .compositions
        .iter()
        .find_map(|c| {
            if let DocumentObject::Known(c) = c {
                (c.id == nested.definition_ref).then_some(c)
            } else {
                None
            }
        })
        .unwrap();
    let size = copied_leaf.nodes[0]
        .properties
        .iter()
        .find(|p| p.descriptor().key.as_str() == "kronello.shape.size")
        .unwrap();
    let edit = apply(
        &path,
        vec![EditCommand::PropertySourceSet {
            object: copied_leaf.nodes[0].id.as_uuid(),
            property: size.id(),
            source: PropertySource::Constant(Value::Vec2([
                FiniteF64::new(5.).unwrap(),
                FiniteF64::new(5.).unwrap(),
            ])),
            curve: None,
        }],
        "individual",
    );
    let modified = export(&path);
    assert_eq!(
        modified.document.compositions[0],
        original.document.compositions[0]
    );
    assert_ne!(image(&path, root, times[1]).linear, frames[1]);
    run(json!({"operation":"edit.undo","project":path,"base_revision":modified.revision,"event_id":edit.id,"session_id":Uuid::new_v4(),"idempotency_key":"undo-individual"})).unwrap();
    let revision = export(&path).revision;
    let error = run(json!({"operation":"edit.undo","project":path,"base_revision":revision,"event_id":event.id,"session_id":Uuid::new_v4(),"idempotency_key":"undo-expand-conflict"})).unwrap_err();
    assert_eq!(error.code, "UNDO_CONFLICT");
    assert_eq!(export(&path).document, expanded.document);
    // An uncontested expansion uses ordinary persisted undo and removes every copied object.
    let clean_path = dir.path().join("undo-expand.kronello");
    run(json!({"operation":"project.create","project":clean_path,"document":original.document}))
        .unwrap();
    let event = apply(
        &clean_path,
        vec![EditCommand::RepeaterExpand {
            repeater: id,
            instance,
            expansion_id: Uuid::new_v4(),
        }],
        "expand-clean",
    );
    let revision = export(&clean_path).revision;
    run(json!({"operation":"edit.undo","project":clean_path,"base_revision":revision,"event_id":event.id,"session_id":Uuid::new_v4(),"idempotency_key":"undo-expand"})).unwrap();
    assert_eq!(export(&clean_path).document, original.document);
    assert_eq!(image(&clean_path, root, times[1]).linear, frames[1]);
}
#[test]
fn stable_seed_context_reordering_pins_and_typed_rejection() {
    let (p, root, id) = fixture();
    let snapshot = RenderSnapshot::for_target(
        &p,
        RenderTarget::Composition { composition: root },
        0,
        Default::default(),
    )
    .unwrap();
    let scene = kronello_render::build_scene_ir(&snapshot, Time::new(1, 2).unwrap(), &[]).unwrap();
    let mut reordered = p.clone();
    let DocumentObject::Known(r) = &mut reordered.repeaters[0] else {
        panic!()
    };
    r.instances.reverse();
    let s = RenderSnapshot::for_target(
        &reordered,
        RenderTarget::Composition { composition: root },
        0,
        Default::default(),
    )
    .unwrap();
    let second = kronello_render::build_scene_ir(&s, Time::new(1, 2).unwrap(), &[]).unwrap();
    for node in &scene.nodes {
        let other = second.nodes.iter().find(|n| n.key == node.key).unwrap();
        assert_eq!(node.opacity, other.opacity);
        assert_eq!(node.world_transform, other.world_transform);
    }
    let mut changed = p.clone();
    let DocumentObject::Known(r) = &mut changed.repeaters[0] else {
        panic!()
    };
    r.instances[0].seed += 1;
    let s = RenderSnapshot::for_target(
        &changed,
        RenderTarget::Composition { composition: root },
        0,
        Default::default(),
    )
    .unwrap();
    let third = kronello_render::build_scene_ir(&s, Time::new(1, 2).unwrap(), &[]).unwrap();
    assert!(scene.nodes.iter().any(|n| {
        third
            .nodes
            .iter()
            .any(|m| m.key == n.key && m.opacity != n.opacity)
    }));
    let mut wire = serde_json::to_value(snapshot).unwrap();
    wire["semantic_versions"]
        .as_object_mut()
        .unwrap()
        .remove("repeater");
    let unpinned: RenderSnapshot = serde_json::from_value(wire).unwrap();
    assert_eq!(
        unpinned.validate().unwrap_err().code(),
        "UNSUPPORTED_FEATURE"
    );
    let mut bad = p.clone();
    let DocumentObject::Known(r) = &mut bad.repeaters[0] else {
        panic!()
    };
    r.version = 99;
    assert_eq!(
        bad.validate_repeaters().unwrap_err().code(),
        "UNSUPPORTED_FEATURE"
    );
    let mut bad = p.clone();
    let DocumentObject::Known(r) = &mut bad.repeaters[0] else {
        panic!()
    };
    r.instances[1].id = r.instances[0].id;
    assert_eq!(
        bad.validate_repeaters().unwrap_err().code(),
        "REPEATER_IDENTITY"
    );
    let mut bad = p.clone();
    let DocumentObject::Known(r) = &mut bad.repeaters[0] else {
        panic!()
    };
    r.source.composition = root;
    r.source.root = if let DocumentObject::Known(c) = &bad.compositions[2] {
        c.root_nodes[0]
    } else {
        panic!()
    };
    assert_eq!(
        bad.validate_repeaters().unwrap_err().code(),
        "REPEATER_CYCLE"
    );
    assert_eq!(p.repeater(id).unwrap().instances.len(), 3);
}

#[test]
fn nested_template_variant_inputs_layout_and_time_survive_materialization() {
    let mut p: Project =
        serde_json::from_str(include_str!("../../../examples/template-002.project.json")).unwrap();
    let mut definition: TemplateDefinition = serde_json::from_str(include_str!(
        "../../../examples/template-002.definition.json"
    ))
    .unwrap();
    let portrait = definition.variants["portrait"].composition_ref;
    let expression = Expression {
        id: ExpressionId::new(),
        version: 1,
        value_type: ValueType::Scalar,
        budget: Default::default(),
        nodes: vec![
            ExpressionNode::Time,
            ExpressionNode::Noise {
                seed: 41,
                element: 22,
                input: 0,
            },
            ExpressionNode::Literal(Value::Scalar(FiniteF64::new(1.).unwrap())),
            ExpressionNode::Add { left: 1, right: 2 },
            ExpressionNode::Literal(Value::Scalar(FiniteF64::new(2.).unwrap())),
            ExpressionNode::Divide { left: 3, right: 4 },
        ],
    };
    let c = p
        .compositions
        .iter_mut()
        .find_map(|c| {
            if let DocumentObject::Known(c) = c {
                (c.id == portrait).then_some(c)
            } else {
                None
            }
        })
        .unwrap();
    let band = c
        .nodes
        .iter_mut()
        .find(|n| matches!(n.kind, NodeKind::Shape { .. }))
        .unwrap();
    let mut opacity = property(
        "kronello.opacity",
        Value::Scalar(FiniteF64::new(1.).unwrap()),
    );
    opacity
        .set_source(
            PropertySource::Expression(expression.id),
            &kronello_render::render_registry(),
        )
        .unwrap();
    band.properties
        .retain(|p| p.descriptor().key.as_str() != "kronello.opacity");
    band.properties.push(opacity);
    p.expressions.push(DocumentObject::Known(expression));
    definition.content_hash =
        kronello_template::authoring_hash(&p, definition.composition_ref).unwrap();
    for variant in definition.variants.values_mut() {
        variant.content_hash =
            kronello_template::authoring_hash(&p, variant.composition_ref).unwrap();
    }
    let selected = kronello_template::selected_definition(&definition, Some("portrait")).unwrap();
    let duration = kronello_time::Duration::new(Time::from_integer(8)).unwrap();
    let mut table = if let Value::DataTable(t) = definition.public_inputs["data"].default.clone() {
        t
    } else {
        panic!()
    };
    table.rows[0].insert("headline".into(), Value::String("共有".into()));
    table.rows[0].insert(
        "accent".into(),
        Value::Color(Color::new(ColorSpace::Srgb, [0.1, 0.6, 0.3], 1.).unwrap()),
    );
    let template = TemplateInstance {
        id: CompositionInstanceId::new(),
        definition_ref: definition.id,
        version: definition.version.clone(),
        duration,
        variant: Some("portrait".into()),
        inputs: std::collections::BTreeMap::from([("data".into(), Value::DataTable(table))]),
    };
    let source_duration = kronello_template::composition(&p, selected.composition_ref)
        .unwrap()
        .duration;
    let source_root = NodeId::new();
    let DocumentObject::Known(wrapper) = &mut p.compositions[0] else {
        panic!()
    };
    wrapper.root_nodes = vec![source_root];
    wrapper.nodes = vec![SceneNode {
        id: source_root,
        name: None,
        tags: Default::default(),
        enabled: true,
        effects: vec![],
        containment_parent: None,
        transform_parent: None,
        child_order: vec![],
        active_range: kronello_time::TimeRange::new(Time::ZERO, duration.as_time()).unwrap(),
        properties: vec![],
        kind: NodeKind::CompositionInstance(CompositionInstance {
            id: template.id,
            definition_ref: selected.composition_ref,
            input_bindings: Default::default(),
            local_time_map: kronello_template::duration_map(
                source_duration,
                duration,
                &selected.duration_policy,
            )
            .unwrap(),
            seed: 99,
        }),
    }];
    let source = RepeatSource {
        composition: wrapper.id,
        root: source_root,
    };
    let mut parent = wrapper.clone();
    parent.id = CompositionId::new();
    parent.root_nodes = vec![NodeId::new()];
    let repeat = ContentId::new();
    parent.nodes[0].id = parent.root_nodes[0];
    parent.nodes[0].kind = NodeKind::Repeater {
        content_ref: repeat,
    };
    let root = parent.id;
    let instance = RepeatInstance {
        id: CompositionInstanceId::new(),
        placement: NodeId::new(),
        seed: 73,
        enabled: true,
        active_range: parent.nodes[0].active_range,
        local_time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
        properties: vec![],
        effects: vec![],
        input_bindings: Default::default(),
        expanded_source: None,
    };
    let instance_id = instance.id;
    p.repeaters.push(DocumentObject::Known(Repeater {
        id: repeat,
        version: 1,
        source,
        instances: vec![instance],
    }));
    p.compositions.push(DocumentObject::Known(parent));
    p.templates.push(DocumentObject::Known(definition));
    p.template_instances.push(DocumentObject::Known(template));
    let DocumentObject::Known(text) = &p.texts[0] else {
        panic!()
    };
    let fonts = vec![FontInput {
        identity: text.styles[0].font.clone(),
        path: std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/fixtures/external/NotoSansCJKjp-Regular.otf"),
    }];
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("template-repeat.kronello");
    write_fixture(&p, "repeat-template.project.json");
    run(json!({"operation":"project.create","project":path,"document":p})).unwrap();
    let render = |time| {
        let ResultData::Frame(f)=run(json!({"operation":"render.frame","input":{"project":path,"composition":root,"region":{"origin":[0.,0.],"extent":[64.,64.],"pixels":[64,64]},"fonts":fonts},"time":time})).unwrap()else{panic!()};
        f.linear.clone()
    };
    let before = export(&path);
    let times = [
        Time::new(1, 5).unwrap(),
        Time::new(1, 2).unwrap(),
        Time::from_integer(4),
        Time::new(39, 5).unwrap(),
    ];
    let frames: Vec<_> = times.iter().map(|t| render(*t)).collect();
    assert!(frames.iter().flatten().any(|p| p[3] > 0.));
    apply(
        &path,
        vec![EditCommand::RepeaterExpand {
            repeater: repeat,
            instance: instance_id,
            expansion_id: Uuid::new_v4(),
        }],
        "materialize",
    );
    let after = export(&path);
    assert_eq!(
        &after.document.compositions[..before.document.compositions.len()],
        before.document.compositions.as_slice()
    );
    assert_eq!(after.document.templates, before.document.templates);
    assert_eq!(
        after.document.template_instances,
        before.document.template_instances
    );
    for (time, frame) in times.iter().zip(&frames) {
        assert_eq!(
            render(*time),
            *frame,
            "variant, public table text/color, layout and protected time map survive"
        );
    }
    let DocumentObject::Known(r) = &after.document.repeaters[0] else {
        panic!()
    };
    let source = r.instances[0].expanded_source.as_ref().unwrap();
    assert_eq!(source.layout_constraints.len(), 1);
    let materialized_id = *source.layout_constraints.keys().next().unwrap();
    let c = kronello_template::composition(&after.document, materialized_id).unwrap();
    let node = c
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Text { .. }))
        .unwrap();
    let NodeKind::Text { content_ref } = node.kind else {
        panic!()
    };
    let mut text = after
        .document
        .texts
        .iter()
        .find_map(|t| {
            if let DocumentObject::Known(t) = t {
                (t.id == content_ref).then_some(t.clone())
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(text.text, "共有");
    text.text = "別".into();
    text.styles[0].range.end = text.text.len();
    apply(
        &path,
        vec![EditCommand::TextSet { text }],
        "materialized-text",
    );
    assert_ne!(render(times[1]), frames[1]);
    assert_eq!(export(&path).document.templates, before.document.templates);
}
