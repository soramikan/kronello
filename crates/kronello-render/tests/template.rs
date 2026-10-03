use kronello_model::*;
use kronello_render::*;
use kronello_template::{authoring_hash, duration_map};
use kronello_text::FontData;
use kronello_time::{Duration, Time, TimeRange};
use std::collections::BTreeMap;
fn fixture() -> (Project, CompositionId, TemplateDefinition) {
    let mut p: Project =
        serde_json::from_str(include_str!("../../../examples/template-001.project.json")).unwrap();
    let mut d: TemplateDefinition = serde_json::from_str(include_str!(
        "../../../examples/template-001.definition.json"
    ))
    .unwrap();
    d.content_hash = authoring_hash(&p, d.composition_ref).unwrap();
    p.templates.push(DocumentObject::Known(d.clone()));
    let root = match &p.compositions[0] {
        DocumentObject::Known(c) => c.id,
        _ => panic!(),
    };
    (p, root, d)
}
fn place(
    p: &mut Project,
    d: &TemplateDefinition,
    duration: i64,
    text: &str,
    color: [u8; 3],
) -> CompositionInstanceId {
    let id = CompositionInstanceId::new();
    let duration = Duration::new(Time::from_integer(duration)).unwrap();
    let i = TemplateInstance {
        id,
        definition_ref: d.id,
        version: d.version.clone(),
        duration,
        inputs: BTreeMap::from([
            ("headline".into(), Value::String(text.into())),
            (
                "accent".into(),
                Value::Color(Color::from_srgb8(color, None)),
            ),
        ]),
    };
    let map = duration_map(
        Duration::new(Time::from_integer(5)).unwrap(),
        duration,
        &d.duration_policy,
    )
    .unwrap();
    let n = SceneNode {
        id: NodeId::new(),
        kind: NodeKind::CompositionInstance(CompositionInstance {
            id,
            definition_ref: d.composition_ref,
            input_bindings: BTreeMap::new(),
            local_time_map: map,
            seed: 0,
        }),
        containment_parent: None,
        transform_parent: None,
        child_order: vec![],
        active_range: TimeRange::from_start_duration(Time::ZERO, duration).unwrap(),
        properties: vec![],
        effects: vec![],
    };
    let DocumentObject::Known(root) = &mut p.compositions[0] else {
        panic!()
    };
    root.root_nodes.push(n.id);
    root.nodes.push(n);
    p.template_instances.push(DocumentObject::Known(i));
    id
}
fn with_fonts(run: impl FnOnce(&[FontData<'_>])) {
    let bytes =
        std::fs::read(kronello_testkit::resolve_fixture("noto-sans-cjk-jp").unwrap()).unwrap();
    let identity = kronello_text::pin_font(&bytes, 0).unwrap();
    run(&[FontData {
        identity: &identity,
        bytes: &bytes,
    }]);
}
fn snapshot(p: &Project, root: CompositionId) -> RenderSnapshot {
    RenderSnapshot::new(p, root, 1, RenderProfile::default()).unwrap()
}
#[test]
fn japanese_instances_have_independent_text_color_duration_and_following_bands() {
    let (mut p, root, d) = fixture();
    let five = place(&mut p, &d, 5, "日", [255, 0, 0]);
    let eight = place(&mut p, &d, 8, "日本語日本語", [0, 0, 255]);
    let frozen = snapshot(&p, root);
    with_fonts(|fonts| {
        let scene = build_scene_ir(&frozen, Time::ONE, fonts).unwrap();
        let band = &d.constraints.bands[0];
        let mut extents = vec![];
        let mut line_counts = vec![];
        for (id, color) in [(five, [255, 0, 0]), (eight, [0, 0, 255])] {
            let path = InstancePath::root().child(id);
            let text = scene
                .nodes
                .iter()
                .find(|n| n.key.instance_path == path && n.key.node == band.text_node)
                .unwrap();
            let background = scene
                .nodes
                .iter()
                .find(|n| n.key.instance_path == path && n.key.node == band.band_node)
                .unwrap();
            let SceneContent::Text(layout) = &text.content else {
                panic!()
            };
            let SceneContent::Shape { resolved, .. } = &background.content else {
                panic!()
            };
            let ResolvedGeometry::Rectangle { size, .. } = resolved.geometry else {
                panic!()
            };
            for (axis, dimension) in size.iter().enumerate() {
                assert_eq!(
                    dimension.get(),
                    layout.layout_bounds.max[axis] - layout.layout_bounds.min[axis]
                        + 2.0 * band.padding[axis].get()
                );
                assert_eq!(
                    background.world_transform.0[axis][2],
                    text.world_transform.0[axis][2] + layout.layout_bounds.min[axis]
                        - band.padding[axis].get()
                );
            }
            assert_eq!(
                resolved.fill.as_ref().unwrap().color,
                Color::from_srgb8(color, None)
            );
            extents.push(size.map(FiniteF64::get));
            line_counts.push(layout.lines.len());
            assert!(layout.lines.len() <= 2);
        }
        // layout_bounds uses the authored wrap width, not visible glyph width.
        assert_eq!(extents[0][0], extents[1][0]);
        assert_ne!(extents[0][1], extents[1][1]);
        assert_eq!(line_counts, [1, 2]);
        let late = build_scene_ir(&frozen, Time::from_integer(6), fonts).unwrap();
        assert!(
            !late
                .nodes
                .iter()
                .any(|n| n.key.instance_path == InstancePath::root().child(five))
        );
        assert!(
            late.nodes
                .iter()
                .any(|n| n.key.instance_path == InstancePath::root().child(eight))
        );
        let frame = render_frame(
            &frozen,
            fonts,
            &kronello_gpu::render_adapter::CpuReferenceBackend,
            FrameRequest {
                time: Time::ONE,
                region: OutputRegion {
                    origin: [0.0; 2],
                    extent: [64.0, 32.0],
                    pixels: [64, 32],
                },
            },
        )
        .unwrap();
        assert!(frame.pixels.linear.iter().any(|p| p[3] > 0.0));
        let again = build_scene_ir(&frozen, Time::ONE, fonts).unwrap();
        assert_eq!(again, scene);
    });
}
#[test]
fn overflow_is_typed_and_sequence_has_no_published_output() {
    let (mut p, root, d) = fixture();
    place(&mut p, &d, 8, "一\n二\n三", [255, 0, 0]);
    let frozen = snapshot(&p, root);
    with_fonts(|fonts| {
        let error = build_scene_ir(&frozen, Time::ONE, fonts).unwrap_err();
        assert_eq!(error.code(), "TEMPLATE_OVERFLOW");
        assert!(matches!(
            error,
            RenderError::Template(kronello_template::TemplateError::Overflow {
                actual: 3,
                maximum: 2,
                ..
            })
        ));
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("frames");
        let result = render_sequence(
            &frozen,
            fonts,
            &kronello_gpu::render_adapter::CpuReferenceBackend,
            SequenceRequest {
                range: TimeRange::new(Time::ZERO, Time::ONE).unwrap(),
                frame_rate: kronello_time::FrameRate::new(1, 1).unwrap(),
                region: OutputRegion {
                    origin: [0.0; 2],
                    extent: [64.0, 32.0],
                    pixels: [64, 32],
                },
            },
            &output,
        );
        assert_eq!(result.unwrap_err().code(), "TEMPLATE_OVERFLOW");
        assert!(!output.exists());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    });
}
#[test]
fn frozen_snapshot_does_not_read_new_instance_inputs() {
    let (mut p, root, d) = fixture();
    place(&mut p, &d, 8, "日", [255, 0, 0]);
    let frozen = snapshot(&p, root);
    let DocumentObject::Known(i) = &mut p.template_instances[0] else {
        panic!()
    };
    i.inputs
        .insert("headline".into(), Value::String("一\n二\n三".into()));
    with_fonts(|fonts| {
        build_scene_ir(&frozen, Time::ONE, fonts).unwrap();
        assert_eq!(
            build_scene_ir(&snapshot(&p, root), Time::ONE, fonts)
                .unwrap_err()
                .code(),
            "TEMPLATE_OVERFLOW"
        );
    });
}

#[test]
fn inactive_placement_parent_skips_layout_and_overflow() {
    let (mut project, root, definition) = fixture();
    place(&mut project, &definition, 8, "一\n二\n三", [255, 0, 0]);
    let DocumentObject::Known(composition) = &mut project.compositions[0] else {
        panic!()
    };
    let group = NodeId::new();
    composition.nodes[0].containment_parent = Some(group);
    composition.root_nodes = vec![group];
    composition.nodes.push(SceneNode {
        id: group,
        kind: NodeKind::Group,
        containment_parent: None,
        transform_parent: None,
        child_order: vec![composition.nodes[0].id],
        active_range: TimeRange::new(Time::ZERO, Time::ONE).unwrap(),
        properties: vec![],
        effects: vec![],
    });
    let frozen = snapshot(&project, root);
    with_fonts(|fonts| {
        assert_eq!(
            build_scene_ir(&frozen, Time::ZERO, fonts)
                .unwrap_err()
                .code(),
            "TEMPLATE_OVERFLOW"
        );
        assert!(
            build_scene_ir(&frozen, Time::ONE, fonts)
                .unwrap()
                .nodes
                .is_empty()
        );
        assert!(
            build_scene_ir(&frozen, Time::from_integer(9), fonts)
                .unwrap()
                .nodes
                .is_empty()
        );
    });
}

#[test]
fn independent_opaque_templates_are_preserved_but_required_contracts_fail_render() {
    let (mut project, root, definition) = fixture();
    let mut encoded = serde_json::to_value(&project).unwrap();
    encoded["templates"][0]["future_constraint"] = serde_json::json!(true);
    encoded["template_instances"] = serde_json::json!([{
        "id": uuid::Uuid::new_v4(), "future_binding": true
    }]);
    let future: Project = serde_json::from_value(encoded.clone()).unwrap();
    let frozen = snapshot(&future, root);
    assert!(
        build_scene_ir(&frozen, Time::ONE, &[])
            .unwrap()
            .nodes
            .is_empty()
    );
    assert_eq!(serde_json::to_value(frozen.project()).unwrap(), encoded);
    place(&mut project, &definition, 8, "日", [255, 0, 0]);
    for collection in ["templates", "template_instances"] {
        let mut encoded = serde_json::to_value(&project).unwrap();
        encoded[collection][0]["future_contract"] = serde_json::json!(true);
        let required: Project = serde_json::from_value(encoded).unwrap();
        let error = RenderSnapshot::new(&required, root, 1, RenderProfile::default()).unwrap_err();
        assert_eq!(error.code(), "UNSUPPORTED_FEATURE");
    }
}
