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
        name: None,
        enabled: true,
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
        name: None,
        enabled: true,
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

fn number(v: f64) -> FiniteF64 {
    FiniteF64::new(v).unwrap()
}
fn constant(name: &str, value: Value) -> Property {
    let registry = render_registry();
    Property::new(
        PropertyId::new(),
        DescriptorRef::new(registry.lookup(&SchemaKey::new(name).unwrap()).unwrap()),
        PropertySource::Constant(value),
        vec![],
        &registry,
    )
    .unwrap()
}
fn refresh(p: &mut Project, d: &mut TemplateDefinition) {
    d.content_hash = authoring_hash(p, d.composition_ref).unwrap();
    p.templates[0] = DocumentObject::Known(d.clone());
}
fn add_text_effects(p: &mut Project, d: &TemplateDefinition) {
    let DocumentObject::Known(c) = &mut p.compositions[1] else {
        panic!()
    };
    let text = c
        .nodes
        .iter_mut()
        .find(|n| n.id == d.constraints.bands[0].text_node)
        .unwrap();
    let sigma = constant("kronello.effect.sigma", Value::Scalar(number(1.0)));
    let offset = constant(
        "kronello.effect.offset",
        Value::Vec2([number(3.0), number(-2.0)]),
    );
    let color = constant(
        "kronello.effect.color",
        Value::Color(Color::from_srgb8([0, 0, 0], None)),
    );
    let opacity = constant("kronello.effect.opacity", Value::Scalar(number(1.0)));
    text.effects = vec![
        Effect::Known(EffectDefinition {
            effect_id: DROP_SHADOW_ID.into(),
            version: 1,
            parameters: EffectParameters::DropShadow {
                sigma: sigma.id(),
                offset: offset.id(),
                color: color.id(),
                opacity: opacity.id(),
            },
        }),
        Effect::Known(EffectDefinition {
            effect_id: GAUSSIAN_BLUR_ID.into(),
            version: 1,
            parameters: EffectParameters::GaussianBlur { sigma: sigma.id() },
        }),
    ];
    text.properties.extend([sigma, offset, color, opacity]);
}

#[test]
fn explicit_bounds_stages_follow_short_whitespace_multiline_and_transformed_text() {
    with_fonts(|fonts| {
        for stage in [BoundsStage::Layout, BoundsStage::Ink, BoundsStage::Visual] {
            for value in ["日", " 日 ", "日\n本", "   ", ""] {
                for rotation in [0.0, 33.0] {
                    let (mut p, root, mut d) = fixture();
                    d.constraints.bands[0].bounds = stage;
                    add_text_effects(&mut p, &d);
                    let DocumentObject::Known(c) = &mut p.compositions[1] else {
                        panic!()
                    };
                    let text = c
                        .nodes
                        .iter_mut()
                        .find(|n| n.id == d.constraints.bands[0].text_node)
                        .unwrap();
                    text.properties.extend([
                        constant(
                            "kronello.transform.rotation",
                            Value::Angle(number(rotation)),
                        ),
                        constant(
                            "kronello.transform.scale",
                            Value::Vec2([number(-1.25), number(1.25)]),
                        ),
                    ]);
                    refresh(&mut p, &mut d);
                    let id = place(&mut p, &d, 8, value, [255, 0, 0]);
                    let frozen = snapshot(&p, root);
                    let scene = build_scene_ir(&frozen, Time::ONE, fonts).unwrap();
                    let path = InstancePath::root().child(id);
                    let binding = &d.constraints.bands[0];
                    let text = scene
                        .nodes
                        .iter()
                        .find(|n| n.key.instance_path == path && n.key.node == binding.text_node)
                        .unwrap();
                    let band = scene
                        .nodes
                        .iter()
                        .find(|n| n.key.instance_path == path && n.key.node == binding.band_node)
                        .unwrap();
                    let SceneContent::Text(layout) = &text.content else {
                        panic!()
                    };
                    assert_eq!(layout.ink_bounds.is_none(), value.trim().is_empty());
                    let chosen = text.bounds.select(stage).unwrap_or_else(|| {
                        let origin = text.world_transform.transform_point([0.0; 2]);
                        DesignBounds {
                            min: origin,
                            max: origin,
                        }
                    });
                    let SceneContent::Shape { resolved, .. } = &band.content else {
                        panic!()
                    };
                    let ResolvedGeometry::Rectangle { size, .. } = resolved.geometry else {
                        panic!()
                    };
                    for (axis, dimension) in size.iter().enumerate() {
                        assert!(
                            (dimension.get()
                                - (chosen.max[axis] - chosen.min[axis]
                                    + 2.0 * binding.padding[axis].get()))
                            .abs()
                                < 1e-10
                        );
                        assert!(
                            (band.world_transform.0[axis][2]
                                - (chosen.min[axis] - binding.padding[axis].get()))
                            .abs()
                                < 1e-10
                        );
                    }
                    if !value.trim().is_empty() {
                        assert_ne!(text.bounds.layout_bounds, text.bounds.ink_bounds);
                        assert_ne!(text.bounds.ink_bounds, text.bounds.visual_bounds);
                    }
                    if value == "日" && rotation == 33.0 {
                        let frame = render_frame(
                            &frozen,
                            fonts,
                            &kronello_gpu::render_adapter::CpuReferenceBackend,
                            FrameRequest {
                                time: Time::ONE,
                                region: OutputRegion {
                                    origin: [-32.0; 2],
                                    extent: [96.0; 2],
                                    pixels: [24; 2],
                                },
                            },
                        )
                        .unwrap();
                        assert!(frame.pixels.linear.iter().any(|p| p[3] > 0.0));
                        let dag = build_render_dag(
                            &scene,
                            RenderProfile::default(),
                            frame.metadata.region,
                        )
                        .unwrap();
                        assert!(dag.nodes().iter().any(|n| matches!(n,
                            DagNode::Geometry { key, resolved: actual } if key == &band.key && actual == resolved)));
                    }
                    if value == "日" && rotation == 0.0 {
                        let mut cache = RenderCache::new(CacheConfig::default());
                        build_scene_ir_with_cache(
                            &frozen,
                            Time::new(1, 2).unwrap(),
                            fonts,
                            &mut cache,
                        )
                        .unwrap();
                        assert_eq!(
                            build_scene_ir_with_cache(&frozen, Time::ONE, fonts, &mut cache)
                                .unwrap(),
                            scene
                        );
                    }
                }
            }
        }
    });
}

#[test]
fn semantic_visual_bounds_cover_cpu_text_pixels_at_multiple_output_scales() {
    let (mut p, root, mut d) = fixture();
    add_text_effects(&mut p, &d);
    let DocumentObject::Known(c) = &mut p.compositions[1] else {
        panic!()
    };
    c.nodes[0]
        .properties
        .iter_mut()
        .find(|p| p.descriptor().key.as_str() == "kronello.opacity")
        .unwrap()
        .set_source(
            PropertySource::Constant(Value::Scalar(number(0.0))),
            &render_registry(),
        )
        .unwrap();
    refresh(&mut p, &mut d);
    place(&mut p, &d, 8, "日", [255, 0, 0]);
    let frozen = snapshot(&p, root);
    with_fonts(|fonts| {
        let scene = build_scene_ir(&frozen, Time::ONE, fonts).unwrap();
        let text = scene
            .nodes
            .iter()
            .find(|n| n.key.node == d.constraints.bands[0].text_node)
            .unwrap();
        let b = text.bounds.visual_bounds.unwrap();
        for scale in [1, 2, 3] {
            let frame = render_frame(
                &frozen,
                fonts,
                &kronello_gpu::render_adapter::CpuReferenceBackend,
                FrameRequest {
                    time: Time::ONE,
                    region: OutputRegion {
                        origin: [-16.0; 2],
                        extent: [80.0, 64.0],
                        pixels: [80 * scale, 64 * scale],
                    },
                },
            )
            .unwrap();
            let mut painted = 0;
            for (i, pixel) in frame.pixels.linear.iter().enumerate() {
                if pixel[3] == 0.0 {
                    continue;
                }
                painted += 1;
                let point = [
                    -16.0 + ((i % (80 * scale) as usize) as f64 + 0.5) / f64::from(scale),
                    -16.0 + ((i / (80 * scale) as usize) as f64 + 0.5) / f64::from(scale),
                ];
                for (axis, coordinate) in point.into_iter().enumerate() {
                    assert!(
                        coordinate >= b.min[axis] - 1.0 / f64::from(scale)
                            && coordinate <= b.max[axis] + 1.0 / f64::from(scale)
                    );
                }
            }
            assert!(painted > 0);
            assert_eq!(build_scene_ir(&frozen, Time::ONE, fonts).unwrap(), scene);
        }
    });
}

#[test]
fn indivisible_width_overflow_is_typed_for_templates_and_plain_text_and_not_published() {
    let (mut p, root, mut d) = fixture();
    let DocumentObject::Known(c) = &mut p.compositions[1] else {
        panic!()
    };
    let text = &mut c.nodes[1];
    let wrap = text
        .properties
        .iter_mut()
        .find(|p| p.descriptor().key.as_str() == "kronello.text.wrap_width")
        .unwrap();
    wrap.set_source(
        PropertySource::Constant(Value::Scalar(number(1.0))),
        &render_registry(),
    )
    .unwrap();
    refresh(&mut p, &mut d);
    place(&mut p, &d, 8, "日", [255, 0, 0]);
    with_fonts(|fonts| {
        for root in [root, d.composition_ref] {
            let frozen = snapshot(&p, root);
            let error = build_scene_ir(&frozen, Time::ONE, fonts).unwrap_err();
            assert_eq!(error.code(), "LAYOUT_OVERFLOW");
            assert!(matches!(
                error,
                RenderError::LayoutOverflow {
                    line: 0,
                    wrap_width: 1.0,
                    ..
                }
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
            assert_eq!(result.unwrap_err().code(), "LAYOUT_OVERFLOW");
            assert!(!output.exists());
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        }
    });
}

#[test]
fn visual_following_in_shared_parent_space_and_singular_parent_is_diagnosed() {
    with_fonts(|fonts| {
        for scale in [0.0, 2.0] {
            let (mut p, root, mut d) = fixture();
            d.constraints.bands[0].bounds = BoundsStage::Visual;
            add_text_effects(&mut p, &d);
            let DocumentObject::Known(c) = &mut p.compositions[1] else {
                panic!()
            };
            let parent = NodeId::new();
            let children = c.root_nodes.clone();
            for node in &mut c.nodes {
                node.containment_parent = Some(parent);
                node.transform_parent = Some(parent);
            }
            c.nodes.push(SceneNode {
                name: None,
                enabled: true,
                id: parent,
                kind: NodeKind::Group,
                containment_parent: None,
                transform_parent: None,
                child_order: children,
                active_range: TimeRange::new(Time::ZERO, Time::from_integer(5)).unwrap(),
                properties: vec![
                    constant("kronello.transform.scale", Value::Vec2([number(scale); 2])),
                    constant(
                        "kronello.transform.position",
                        Value::Vec2([number(4.0), number(5.0)]),
                    ),
                ],
                effects: vec![],
            });
            c.root_nodes = vec![parent];
            refresh(&mut p, &mut d);
            place(&mut p, &d, 8, "日", [255, 0, 0]);
            let result = build_scene_ir(&snapshot(&p, root), Time::ONE, fonts);
            if scale == 0.0 {
                assert_eq!(result.unwrap_err().code(), "LAYOUT_SINGULAR_TRANSFORM");
            } else {
                let scene = result.unwrap();
                let text = scene
                    .nodes
                    .iter()
                    .find(|n| n.key.node == d.constraints.bands[0].text_node)
                    .unwrap();
                let band = scene
                    .nodes
                    .iter()
                    .find(|n| n.key.node == d.constraints.bands[0].band_node)
                    .unwrap();
                let ink = band.bounds.ink_bounds.unwrap();
                let visual = text.bounds.visual_bounds.unwrap();
                for axis in 0..2 {
                    assert!(
                        (ink.min[axis]
                            - (visual.min[axis]
                                - scale * d.constraints.bands[0].padding[axis].get()))
                        .abs()
                            < 1e-10
                    );
                    assert!(
                        (ink.max[axis]
                            - (visual.max[axis]
                                + scale * d.constraints.bands[0].padding[axis].get()))
                        .abs()
                            < 1e-10
                    );
                }
            }
        }
    });
}

#[test]
fn bounds_query_preserves_renderer_rejection_of_nonuniform_blur() {
    let (mut p, root, mut d) = fixture();
    add_text_effects(&mut p, &d);
    let DocumentObject::Known(c) = &mut p.compositions[1] else {
        panic!()
    };
    c.nodes[1].properties.push(constant(
        "kronello.transform.scale",
        Value::Vec2([number(2.0), number(1.0)]),
    ));
    refresh(&mut p, &mut d);
    place(&mut p, &d, 8, "日", [255, 0, 0]);
    with_fonts(|fonts| {
        assert_eq!(
            build_scene_ir(&snapshot(&p, root), Time::ONE, fonts)
                .unwrap_err()
                .code(),
            "UNSUPPORTED_FEATURE"
        );
    });
}

#[test]
fn nonleaf_text_follower_is_explicitly_unsupported() {
    let (mut p, root, mut d) = fixture();
    let DocumentObject::Known(c) = &mut p.compositions[1] else {
        panic!()
    };
    let text_id = c.nodes[1].id;
    let child_id = NodeId::new();
    c.nodes[1].child_order.push(child_id);
    c.nodes.push(SceneNode {
        name: None,
        enabled: true,
        id: child_id,
        kind: NodeKind::Null,
        containment_parent: Some(text_id),
        transform_parent: None,
        child_order: vec![],
        active_range: TimeRange::new(Time::ZERO, Time::from_integer(5)).unwrap(),
        properties: vec![],
        effects: vec![],
    });
    refresh(&mut p, &mut d);
    place(&mut p, &d, 8, "日", [255, 0, 0]);
    assert_eq!(
        RenderSnapshot::new(&p, root, 1, RenderProfile::default())
            .unwrap_err()
            .code(),
        "UNSUPPORTED_FEATURE"
    );
}

fn remap<T: serde::Serialize + serde::de::DeserializeOwned>(
    value: &T,
    ids: &BTreeMap<String, String>,
) -> T {
    fn visit(value: &mut serde_json::Value, ids: &BTreeMap<String, String>) {
        match value {
            serde_json::Value::String(s) => {
                if let Some(new) = ids.get(s) {
                    *s = new.clone()
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    visit(value, ids)
                }
            }
            serde_json::Value::Object(values) => {
                for value in values.values_mut() {
                    visit(value, ids)
                }
            }
            _ => (),
        }
    }
    let mut value = serde_json::to_value(value).unwrap();
    visit(&mut value, ids);
    serde_json::from_value(value).unwrap()
}

#[test]
fn visual_band_dependencies_are_scheduled_independently_of_constraint_order() {
    let (mut p, root, mut d) = fixture();
    d.constraints.bands[0].bounds = BoundsStage::Visual;
    let DocumentObject::Known(c) = &p.compositions[1] else {
        panic!()
    };
    let DocumentObject::Known(shape) = &p.shapes[0] else {
        panic!()
    };
    let DocumentObject::Known(content) = &p.texts[0] else {
        panic!()
    };
    let ids: BTreeMap<_, _> = [
        c.nodes[0].id.to_string(),
        c.nodes[1].id.to_string(),
        shape.id.to_string(),
        content.id.to_string(),
    ]
    .into_iter()
    .chain(
        c.nodes
            .iter()
            .flat_map(|n| n.properties.iter().map(|p| p.id().to_string())),
    )
    .map(|old| (old, uuid::Uuid::new_v4().to_string()))
    .collect();
    let mut band: SceneNode = remap(&c.nodes[0], &ids);
    let mut text: SceneNode = remap(&c.nodes[1], &ids);
    band.transform_parent = Some(c.nodes[0].id);
    text.transform_parent = band.transform_parent;
    let second: TemplateBandBinding = remap(&d.constraints.bands[0], &ids);
    let shape: Shape = remap(shape, &ids);
    let content: TextDocument = remap(content, &ids);
    let DocumentObject::Known(c) = &mut p.compositions[1] else {
        panic!()
    };
    c.root_nodes.extend([band.id, text.id]);
    c.nodes.extend([band, text]);
    p.shapes.push(DocumentObject::Known(shape));
    p.texts.push(DocumentObject::Known(content));
    d.constraints.bands.insert(0, second); // Authored consumer precedes its producer.
    refresh(&mut p, &mut d);
    place(&mut p, &d, 8, "日", [255, 0, 0]);
    let reversed = snapshot(&p, root);
    with_fonts(|fonts| {
        let actual = build_scene_ir(&reversed, Time::ONE, fonts).unwrap();
        d.constraints.bands.reverse();
        refresh(&mut p, &mut d);
        let ordered = snapshot(&p, root);
        assert_eq!(build_scene_ir(&ordered, Time::ONE, fonts).unwrap(), actual);
    });
}
