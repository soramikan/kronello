use std::collections::BTreeMap;
use std::sync::OnceLock;

use kronello_gpu::render_adapter::CpuReferenceBackend;
use kronello_model::*;
use kronello_render::*;
use kronello_text::{FontData, pin_font};
use kronello_time::{Duration, FrameRate, Time, TimeMap, TimeRange};
use sha2::{Digest, Sha256};

fn t(n: i64, d: i64) -> Time {
    Time::new(n, d).unwrap()
}
fn f(v: f64) -> FiniteF64 {
    FiniteF64::new(v).unwrap()
}
fn v2(x: f64, y: f64) -> Value {
    Value::Vec2([f(x), f(y)])
}
fn scalar(v: f64) -> Value {
    Value::Scalar(f(v))
}
fn font() -> &'static (Vec<u8>, FontRef) {
    static FONT: OnceLock<(Vec<u8>, FontRef)> = OnceLock::new();
    FONT.get_or_init(|| {
        let bytes =
            std::fs::read(kronello_testkit::resolve_fixture("noto-sans-cjk-jp").unwrap()).unwrap();
        let identity = pin_font(&bytes, 0).unwrap();
        (bytes, identity)
    })
}
fn fonts() -> [FontData<'static>; 1] {
    [FontData {
        identity: &font().1,
        bytes: &font().0,
    }]
}
fn prop(key: &str, source: PropertySource<Value>) -> Property {
    let registry = render_registry();
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
    prop(key, PropertySource::Constant(value))
}
fn node(kind: NodeKind, properties: Vec<Property>) -> SceneNode {
    SceneNode {
        tags: Default::default(),
        name: None,
        enabled: true,
        effects: vec![],
        id: NodeId::new(),
        kind,
        containment_parent: None,
        transform_parent: None,
        child_order: vec![],
        active_range: TimeRange::new(t(-2, 1), t(3, 1)).unwrap(),
        properties,
    }
}
fn composition(nodes: Vec<SceneNode>) -> Composition {
    Composition {
        id: CompositionId::new(),
        duration: Duration::new(t(3, 1)).unwrap(),
        design_extent: DesignExtent::new(64.0, 32.0).unwrap(),
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
fn rectangle(size: [f64; 2], color: Color) -> (SceneNode, Shape) {
    let size = constant("kronello.shape.size", v2(size[0], size[1]));
    let radius = constant("kronello.shape.corner_radius", scalar(0.0));
    let fill = constant("kronello.fill_color", Value::Color(color));
    let shape = Shape {
        id: ContentId::new(),
        geometry: ShapeGeometry::Rectangle {
            size: size.id(),
            corner_radius: radius.id(),
        },
        fill: Some(Fill {
            gradient: None,
            color: fill.id(),
            rule: FillRule::Nonzero,
        }),
        stroke: None,
    };
    (
        node(
            NodeKind::Shape {
                content_ref: shape.id,
            },
            vec![size, radius, fill],
        ),
        shape,
    )
}
fn project() -> (Project, CompositionId) {
    let curve = AnimationCurve::new(
        CurveId::new(),
        ValueType::Vec2,
        vec![
            Keyframe {
                time: t(0, 1),
                value: v2(1.0, 1.0),
                interpolation: CurveInterpolation::Linear,
            },
            Keyframe {
                time: t(1, 1),
                value: v2(9.0, 1.0),
                interpolation: CurveInterpolation::Linear,
            },
        ],
    )
    .unwrap();
    let (mut shape_node, shape) = rectangle([16.0, 12.0], Color::from_srgb8([200, 40, 10], None));
    shape_node.properties.push(prop(
        "kronello.transform.position",
        PropertySource::Curve(curve.id()),
    ));
    shape_node
        .properties
        .push(constant("kronello.opacity", scalar(0.75)));
    let size = constant("kronello.text.font_size", scalar(12.0));
    let fill = constant(
        "kronello.fill_color",
        Value::Color(Color::from_srgb8([20, 160, 60], None)),
    );
    let wrap = constant("kronello.text.wrap_width", scalar(50.0));
    let line = constant("kronello.text.line_height", scalar(16.0));
    let alignment = constant("kronello.text.alignment", Value::Enum("start".into()));
    let text = TextDocument {
        id: ContentId::new(),
        layout_version: TEXT_LAYOUT_VERSION,
        text: "日本語".into(),
        styles: vec![TextStyleSpan {
            gradient: None,
            range: TextRange {
                start: 0,
                end: "日本語".len(),
            },
            font: font().1.clone(),
            size: size.id(),
            fill: fill.id(),
        }],
        direction: TextDirection::Horizontal,
        ruby: vec![],
        character_animations: vec![],
        wrap_width: wrap.id(),
        line_height: line.id(),
        alignment: alignment.id(),
        path: None,
    };
    let text_node = node(
        NodeKind::Text {
            content_ref: text.id,
        },
        vec![
            size,
            fill,
            wrap,
            line,
            alignment,
            constant("kronello.transform.position", v2(20.0, 5.0)),
        ],
    );
    let c = composition(vec![shape_node, text_node]);
    let id = c.id;
    (
        Project {
            compositions: vec![DocumentObject::Known(c)],
            curves: vec![DocumentObject::Known(curve)],
            shapes: vec![DocumentObject::Known(shape)],
            texts: vec![DocumentObject::Known(text)],
            ..Project::default()
        },
        id,
    )
}
fn region() -> OutputRegion {
    OutputRegion {
        origin: [0.0; 2],
        extent: [64.0, 32.0],
        pixels: [64, 32],
    }
}
fn snapshot(p: &Project, c: CompositionId) -> RenderSnapshot {
    RenderSnapshot::new(p, c, 7, RenderProfile::default()).unwrap()
}
fn frame(s: &RenderSnapshot, time: Time) -> RenderedFrame {
    render_frame(
        s,
        &fonts(),
        &CpuReferenceBackend,
        FrameRequest {
            time,
            region: region(),
        },
    )
    .unwrap()
}
fn comp_mut(p: &mut Project) -> &mut Composition {
    match &mut p.compositions[0] {
        DocumentObject::Known(c) => c,
        _ => panic!(),
    }
}

#[test]
fn cpu_visibility_snapshot_versions_preserve_legacy_and_hide_disabled_subtrees() {
    let (mut child, shape) = rectangle([16.0, 12.0], Color::from_srgb8([200, 40, 10], None));
    let mut parent = node(NodeKind::Group, vec![]);
    parent.child_order.push(child.id);
    child.containment_parent = Some(parent.id);
    let composition = composition(vec![parent, child]);
    let id = composition.id;
    let mut project = Project {
        name: "Visibility".into(),
        ..Project::default()
    };
    project
        .compositions
        .push(DocumentObject::Known(composition));
    project.shapes.push(DocumentObject::Known(shape));
    let visible = snapshot(&project, id);
    assert_eq!(visible.semantic_versions().visibility, 2);
    let render = |snapshot: &RenderSnapshot| {
        render_frame(
            snapshot,
            &[],
            &CpuReferenceBackend,
            FrameRequest {
                time: Time::ZERO,
                region: region(),
            },
        )
        .unwrap()
    };
    let visible_frame = render(&visible);
    let mut legacy = serde_json::to_value(&visible).unwrap();
    legacy["semantic_versions"]
        .as_object_mut()
        .unwrap()
        .remove("visibility");
    let legacy: RenderSnapshot = serde_json::from_value(legacy).unwrap();
    assert_eq!(legacy.semantic_versions().visibility, 1);
    assert_eq!(render(&legacy).pixels, visible_frame.pixels);
    comp_mut(&mut project).nodes[0].enabled = false;
    let disabled = snapshot(&project, id);
    let hidden = render(&disabled);
    assert_ne!(visible_frame.pixels, hidden.pixels);
    assert!(hidden.pixels.linear.iter().all(|pixel| *pixel == [0.0; 4]));
    let mut legacy_contract = disabled.semantic_versions().clone();
    legacy_contract.visibility = 1;
    assert_eq!(
        RenderSnapshot::with_contract(&project, id, 7, disabled.profile(), legacy_contract, vec![])
            .unwrap_err()
            .code(),
        "UNSUPPORTED_FEATURE"
    );
}

#[test]
fn animated_shape_japanese_text_random_access_and_resolution_preserve_document() {
    let (mut p, id) = project();
    let before = serde_json::to_vec(&p).unwrap();
    let s = snapshot(&p, id);
    let times = [t(0, 1), t(1, 3), t(2, 3), t(1, 1)];
    let forward: Vec<_> = times.iter().map(|time| frame(&s, *time)).collect();
    assert_ne!(forward[0].pixels.linear, forward[3].pixels.linear);
    // An analytic pixel moves from the animated rectangle's edge into its body.
    assert!(forward[0].pixels.linear[2 * 64 + 2][3] > 0.7);
    assert_eq!(forward[3].pixels.linear[2 * 64 + 2][3], 0.0);
    for i in [3, 1, 0, 2, 1] {
        assert_eq!(frame(&s, times[i]), forward[i]);
    }
    let ir = build_scene_ir(&s, t(1, 3), &fonts()).unwrap();
    let layout = ir
        .nodes
        .iter()
        .find_map(|n| match &n.content {
            SceneContent::Text(l) => Some(l),
            _ => None,
        })
        .unwrap();
    assert_eq!(layout.glyphs.len(), 3);
    assert!(
        forward[0]
            .pixels
            .linear
            .iter()
            .any(|p| p[1] > p[0] && p[3] > 0.0)
    );
    for r in [
        OutputRegion {
            pixels: [128, 64],
            ..region()
        },
        OutputRegion {
            origin: [16.0, 0.0],
            extent: [32.0, 32.0],
            pixels: [32, 32],
        },
        OutputRegion {
            pixels: [32, 48],
            ..region()
        },
    ] {
        let dag = build_render_dag(&ir, s.profile(), r).unwrap();
        assert!(
            dag.nodes()
                .iter()
                .any(|n| matches!(n, DagNode::TextLayout { .. }))
        );
        render_frame(
            &s,
            &fonts(),
            &CpuReferenceBackend,
            FrameRequest {
                time: t(1, 3),
                region: r,
            },
        )
        .unwrap();
        assert_eq!(build_scene_ir(&s, t(1, 3), &fonts()).unwrap(), ir);
    }
    let full = frame(&s, t(1, 3));
    let crop = render_frame(
        &s,
        &fonts(),
        &CpuReferenceBackend,
        FrameRequest {
            time: t(1, 3),
            region: OutputRegion {
                origin: [16.0, 0.0],
                extent: [32.0, 32.0],
                pixels: [32, 32],
            },
        },
    )
    .unwrap();
    for y in 0..32 {
        assert_eq!(
            &crop.pixels.linear[y * 32..(y + 1) * 32],
            &full.pixels.linear[y * 64 + 16..y * 64 + 48]
        );
    }
    assert_eq!(serde_json::to_vec(&p).unwrap(), before);
    p.name = "edited after snapshot".into();
    assert_eq!(frame(&s, t(1, 3)), forward[1]);
}

#[test]
fn sequence_rational_grid_files_png16_raw_truth_metadata_and_nonoverwrite() {
    let (p, id) = project();
    let s = snapshot(&p, id);
    let tmp = tempfile::tempdir().unwrap();
    let directory = tmp.path().join("sequence");
    let request = SequenceRequest {
        range: TimeRange::new(t(1, 100), t(11, 100)).unwrap(),
        frame_rate: FrameRate::new(30000, 1001).unwrap(),
        region: region(),
    };
    let sequence =
        render_sequence(&s, &fonts(), &CpuReferenceBackend, request, &directory).unwrap();
    assert_eq!(sequence.frames.len(), 3);
    for (ordinal, out) in sequence.frames.iter().enumerate() {
        let index = ordinal as i64 + 1;
        assert_eq!(out.metadata.time, t(index * 1001, 30000));
        assert_eq!(out.metadata.frame_index, Some(index.to_string()));
        assert_eq!(out.metadata.sequence_number, Some(ordinal as u64));
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(directory.join(&out.metadata_file)).unwrap())
                .unwrap();
        assert_eq!(
            json["time"]["num"],
            out.metadata.time.numerator().to_string()
        );
        assert_eq!(
            json["time"]["den"],
            out.metadata.time.denominator().to_string()
        );
        assert_eq!(json["snapshot_content_hash"], s.content_hash().unwrap());
        assert_eq!(json["revision"], "7");
        assert_eq!(json["working_space"], "linear_rec709");
        assert_eq!(json["numeric"]["alpha"], "premultiplied");
        assert_eq!(json["numeric"]["transfer_function"], "linear");
        assert_eq!(json["numeric"]["byte_order"], "little_endian");
        assert_eq!(json["display"]["alpha"], "straight");
        assert_eq!(json["display"]["color_space"], "srgb");
        assert_eq!(json["display"]["pixel_format"], "rgba16_unorm_png");
        assert_eq!(json["semantic_versions"]["document"], 1);
        assert_eq!(json["semantic_versions"]["interpolation"], 1);
        assert_eq!(json["semantic_versions"]["layout"], 1);
        assert_eq!(json["semantic_versions"]["color"], COLOR_VERSION);
        assert_eq!(out.metadata.font_locks, vec![font().1.clone()]);
        assert_eq!(
            out.metadata.design_to_pixel,
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
        );
        for file in [&out.numeric, &out.display] {
            let bytes = std::fs::read(directory.join(&file.name)).unwrap();
            assert_eq!(bytes.len() as u64, file.bytes);
            assert_eq!(format!("{:x}", Sha256::digest(&bytes)), file.sha256);
        }
        let expected = frame(&s, out.metadata.time);
        let raw = std::fs::read(directory.join(&out.numeric.name)).unwrap();
        assert_eq!(raw, encode_rgba16f(&expected.pixels.linear).unwrap());
        let mut decoder = png::Decoder::new(std::io::Cursor::new(
            std::fs::read(directory.join(&out.display.name)).unwrap(),
        ))
        .read_info()
        .unwrap();
        assert_eq!(decoder.info().bit_depth, png::BitDepth::Sixteen);
        assert_eq!(decoder.info().color_type, png::ColorType::Rgba);
        assert!(decoder.info().srgb.is_some());
        let mut data = vec![0; decoder.output_buffer_size().unwrap()];
        let info = decoder.next_frame(&mut data).unwrap();
        assert_eq!([info.width, info.height], region().pixels);
        for (encoded, pixel) in data[..info.buffer_size()]
            .chunks_exact(8)
            .zip(&expected.pixels.display)
        {
            for (sample, v) in encoded.chunks_exact(2).zip(pixel) {
                assert_eq!(
                    u16::from_be_bytes(sample.try_into().unwrap()),
                    (v.clamp(0.0, 1.0) * 65535.0).round() as u16
                );
            }
        }
    }
    let manifest = std::fs::read(directory.join("sequence.json")).unwrap();
    let parsed: SequenceMetadata = serde_json::from_slice(&manifest).unwrap();
    assert_eq!(parsed, sequence);
    assert!(render_sequence(&s, &fonts(), &CpuReferenceBackend, request, &directory).is_err());
    assert_eq!(
        std::fs::read(directory.join("sequence.json")).unwrap(),
        manifest
    );
    assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 10);
}

#[test]
fn negative_fractional_range_end_exclusion_empty_and_overflow() {
    let rate = FrameRate::new(30000, 1001).unwrap();
    let range = TimeRange::new(t(-1, 10), rate.frame_to_time(2).unwrap()).unwrap();
    let samples = frame_samples(range, rate).unwrap();
    assert_eq!(
        samples.iter().map(|p| p.0).collect::<Vec<_>>(),
        vec![-2, -1, 0, 1]
    );
    assert!(samples.iter().all(|(_, time)| range.contains(*time)));
    assert!(
        frame_samples(TimeRange::new(t(1, 100), t(2, 100)).unwrap(), rate)
            .unwrap()
            .is_empty()
    );
    assert!(
        frame_samples(
            TimeRange::new(t(i64::MAX - 1, 1), t(i64::MAX, 1)).unwrap(),
            rate
        )
        .is_err()
    );
    assert!(
        frame_samples(
            TimeRange::new(t(0, 1), t(1_000_001, 1)).unwrap(),
            FrameRate::new(1, 1).unwrap()
        )
        .is_err()
    );
}

#[test]
fn snapshot_versions_hash_locks_profile_and_independent_unknown_content() {
    let (mut p, id) = project();
    let s = snapshot(&p, id);
    let hash = s.content_hash().unwrap();
    assert_ne!(
        hash,
        RenderSnapshot::new(&p, id, 8, RenderProfile::default())
            .unwrap()
            .content_hash()
            .unwrap()
    );
    assert_ne!(
        hash,
        RenderSnapshot::new(
            &p,
            id,
            7,
            RenderProfile {
                working_space: ColorSpace::LinearRec2020,
                ..RenderProfile::default()
            }
        )
        .unwrap()
        .content_hash()
        .unwrap()
    );
    let mut version = s.semantic_versions().clone();
    version.layout = 99;
    assert_eq!(
        RenderSnapshot::with_contract(&p, id, 7, s.profile(), version, vec![])
            .unwrap_err()
            .code(),
        "UNSUPPORTED_FEATURE"
    );
    let mut version = s.semantic_versions().clone();
    version.bounds = 99;
    assert_eq!(
        RenderSnapshot::with_contract(&p, id, 7, s.profile(), version, vec![])
            .unwrap_err()
            .code(),
        "UNSUPPORTED_FEATURE"
    );
    let value = serde_json::to_value(&s).unwrap();
    let restored: RenderSnapshot = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(restored.content_hash().unwrap(), hash);
    assert_eq!(frame(&restored, t(1, 3)), frame(&s, t(1, 3)));
    let mut bad = value.clone();
    bad.as_object_mut().unwrap().remove("semantic_versions");
    assert!(serde_json::from_value::<RenderSnapshot>(bad).is_err());
    let mut bad = value;
    bad["font_locks"] = serde_json::json!([]);
    let bad: RenderSnapshot = serde_json::from_value(bad).unwrap();
    assert!(
        render_frame(
            &bad,
            &fonts(),
            &CpuReferenceBackend,
            FrameRequest {
                time: t(0, 1),
                region: region()
            }
        )
        .is_err()
    );
    p.compositions.push(DocumentObject::Opaque(OpaqueObject {
        id: uuid::Uuid::new_v4(),
        fields: BTreeMap::from([("unknown_feature".into(), serde_json::json!("future"))]),
    }));
    let independent = snapshot(&p, id);
    assert_ne!(independent.content_hash().unwrap(), hash);
    assert_eq!(
        frame(&independent, t(0, 1)).pixels,
        frame(&s, t(0, 1)).pixels
    );
    p.unknown_fields
        .insert("future_effect".into(), serde_json::json!(1));
    assert_eq!(
        RenderSnapshot::new(&p, id, 7, s.profile())
            .unwrap_err()
            .code(),
        "UNSUPPORTED_FEATURE"
    );
}

#[test]
fn expression_missing_font_glyph_hash_and_failed_sequence_are_typed() {
    let (mut p, id) = project();
    let registry = render_registry();
    comp_mut(&mut p).nodes[0].properties[3]
        .set_source(PropertySource::Expression(ExpressionId::new()), &registry)
        .unwrap();
    let s = snapshot(&p, id);
    let error = render_frame(
        &s,
        &fonts(),
        &CpuReferenceBackend,
        FrameRequest {
            time: t(0, 1),
            region: region(),
        },
    )
    .unwrap_err();
    assert_eq!(error.code(), "EVALUATION_ERROR");
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("failed");
    assert!(
        render_sequence(
            &s,
            &fonts(),
            &CpuReferenceBackend,
            SequenceRequest {
                range: TimeRange::new(t(0, 1), t(1, 1)).unwrap(),
                frame_rate: FrameRate::new(24, 1).unwrap(),
                region: region()
            },
            &out
        )
        .is_err()
    );
    assert!(!out.exists());
    let (p, id) = project();
    let s = snapshot(&p, id);
    assert_eq!(
        render_frame(
            &s,
            &[],
            &CpuReferenceBackend,
            FrameRequest {
                time: t(0, 1),
                region: region()
            }
        )
        .unwrap_err()
        .code(),
        "ASSET_MISSING"
    );
    let mut bytes = font().0.clone();
    bytes[50] ^= 1;
    assert_eq!(
        render_frame(
            &s,
            &[FontData {
                identity: &font().1,
                bytes: &bytes
            }],
            &CpuReferenceBackend,
            FrameRequest {
                time: t(0, 1),
                region: region()
            }
        )
        .unwrap_err()
        .code(),
        "ASSET_HASH_MISMATCH"
    );
    let (mut p, id) = project();
    let DocumentObject::Known(text) = &mut p.texts[0] else {
        panic!()
    };
    text.text = "\u{10ffff}".into();
    text.styles[0].range.end = text.text.len();
    let s = snapshot(&p, id);
    assert_eq!(
        render_frame(
            &s,
            &fonts(),
            &CpuReferenceBackend,
            FrameRequest {
                time: t(0, 1),
                region: region()
            }
        )
        .unwrap_err()
        .code(),
        "GLYPH_MISSING"
    );
}

fn group_project() -> (Project, CompositionId, NodeId, NodeId) {
    let (mut a, sa) = rectangle(
        [16.0, 16.0],
        Color::new(ColorSpace::LinearRec709, [1.0, 0.0, 0.0], 0.5).unwrap(),
    );
    let (mut b, sb) = rectangle(
        [16.0, 16.0],
        Color::new(ColorSpace::LinearRec709, [0.0, 1.0, 0.0], 0.5).unwrap(),
    );
    let mut group = node(
        NodeKind::Group,
        vec![constant("kronello.opacity", scalar(0.5))],
    );
    group.child_order = vec![a.id, b.id];
    a.containment_parent = Some(group.id);
    b.containment_parent = Some(group.id);
    let (matte, sm) = rectangle(
        [8.0, 16.0],
        Color::new(ColorSpace::LinearRec709, [1.0; 3], 0.5).unwrap(),
    );
    let source = group.id;
    let mask = matte.id;
    let c = composition(vec![group, a, b, matte]);
    let id = c.id;
    (
        Project {
            compositions: vec![DocumentObject::Known(c)],
            shapes: vec![
                DocumentObject::Known(sa),
                DocumentObject::Known(sb),
                DocumentObject::Known(sm),
            ],
            ..Project::default()
        },
        id,
        source,
        mask,
    )
}
fn masked_snapshot(
    p: &Project,
    id: CompositionId,
    source: NodeId,
    matte: NodeId,
    kind: MatteKind,
) -> RenderSnapshot {
    RenderSnapshot::with_contract(
        p,
        id,
        7,
        RenderProfile::default(),
        SemanticVersions::current(1),
        vec![MatteBinding {
            source: SceneKey {
                instance_path: InstancePath::root(),
                node: source,
            },
            matte: SceneKey {
                instance_path: InstancePath::root(),
                node: matte,
            },
            kind,
            visible: false,
        }],
    )
    .unwrap()
}
#[test]
fn dag_is_topological_group_opacity_once_and_matte_is_not_displayed() {
    let (p, id, source, matte) = group_project();
    let s = masked_snapshot(&p, id, source, matte, MatteKind::Alpha);
    let ir = build_scene_ir(&s, t(0, 1), &[]).unwrap();
    let dag = build_render_dag(&ir, s.profile(), region()).unwrap();
    for (i, node) in dag.nodes().iter().enumerate() {
        assert!(node.inputs().iter().all(|input| *input < i));
    }
    assert!(matches!(
        dag.nodes()[dag.output()],
        DagNode::OutputTransform { .. }
    ));
    assert!(
        dag.nodes()
            .iter()
            .any(|n| matches!(n, DagNode::Mask { .. }))
    );
    let frame = render_frame(
        &s,
        &[],
        &CpuReferenceBackend,
        FrameRequest {
            time: t(0, 1),
            region: region(),
        },
    )
    .unwrap();
    assert_eq!(
        frame.pixels.linear[2 * 64 + 2],
        [0.0625, 0.125, 0.0, 0.1875]
    );
    assert_eq!(frame.pixels.linear[2 * 64 + 12], [0.0; 4]);
    let unmasked = snapshot(&p, id);
    assert_ne!(
        frame.pixels.linear,
        render_frame(
            &unmasked,
            &[],
            &CpuReferenceBackend,
            FrameRequest {
                time: t(0, 1),
                region: region()
            }
        )
        .unwrap()
        .pixels
        .linear
    );
    let mut bad = ir.clone();
    bad.mattes.push(MatteBinding {
        source: SceneKey {
            instance_path: InstancePath::root(),
            node: matte,
        },
        matte: SceneKey {
            instance_path: InstancePath::root(),
            node: source,
        },
        kind: MatteKind::Alpha,
        visible: false,
    });
    assert!(build_render_dag(&bad, s.profile(), region()).is_err());
}

#[test]
fn nested_instance_paths_active_range_and_transform_parent_are_preserved() {
    let (mut child, shape) = rectangle([8.0; 2], Color::from_srgb8([255, 0, 0], None));
    let mut parent = node(
        NodeKind::Null,
        vec![constant("kronello.transform.position", v2(3.0, 2.0))],
    );
    parent.child_order = vec![child.id];
    child.containment_parent = Some(parent.id);
    child.transform_parent = Some(parent.id);
    let target = composition(vec![parent, child]);
    let a = node(
        NodeKind::CompositionInstance(CompositionInstance {
            id: CompositionInstanceId::new(),
            definition_ref: target.id,
            input_bindings: BTreeMap::new(),
            local_time_map: TimeMap::linear(t(0, 1), t(1, 1)).unwrap(),
            seed: 0,
        }),
        vec![],
    );
    let b = node(
        NodeKind::CompositionInstance(CompositionInstance {
            id: CompositionInstanceId::new(),
            definition_ref: target.id,
            input_bindings: BTreeMap::new(),
            local_time_map: TimeMap::linear(t(0, 1), t(1, 1)).unwrap(),
            seed: 0,
        }),
        vec![constant("kronello.transform.position", v2(20.0, 0.0))],
    );
    let root = composition(vec![a, b]);
    let id = root.id;
    let p = Project {
        compositions: vec![DocumentObject::Known(root), DocumentObject::Known(target)],
        shapes: vec![DocumentObject::Known(shape)],
        ..Project::default()
    };
    let s = snapshot(&p, id);
    let ir = build_scene_ir(&s, t(0, 1), &[]).unwrap();
    let shapes: Vec<_> = ir
        .nodes
        .iter()
        .filter(|n| matches!(n.content, SceneContent::Shape { .. }))
        .collect();
    assert_eq!(shapes.len(), 2);
    assert_ne!(shapes[0].key, shapes[1].key);
    assert_eq!(
        shapes[0].world_transform.transform_point([0.0; 2]),
        [3.0, 2.0]
    );
    assert_eq!(
        shapes[1].world_transform.transform_point([0.0; 2]),
        [23.0, 2.0]
    );
    let frame = render_frame(
        &s,
        &[],
        &CpuReferenceBackend,
        FrameRequest {
            time: t(0, 1),
            region: region(),
        },
    )
    .unwrap();
    assert_eq!(frame.pixels.linear[3 * 64 + 4], [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(frame.pixels.linear[3 * 64 + 24], [1.0, 0.0, 0.0, 1.0]);
    assert!(build_scene_ir(&s, t(3, 1), &[]).unwrap().nodes.is_empty());
}

#[test]
fn invalid_regions_pixels_and_numeric_rounding_are_rejected_or_explicit() {
    for r in [
        OutputRegion {
            pixels: [0, 32],
            ..region()
        },
        OutputRegion {
            extent: [f64::NAN, 32.0],
            ..region()
        },
        OutputRegion {
            extent: [-1.0, 32.0],
            ..region()
        },
    ] {
        assert!(r.validate().is_err());
    }
    assert!(encode_rgba16f(&[[1.0, 0.0, 0.0, 0.0]]).is_err());
    assert!(encode_rgba16f(&[[f32::NAN, 0.0, 0.0, 1.0]]).is_err());
    assert!(encode_rgba16f(&[[65505.0, 0.0, 0.0, 1.0]]).is_err());
    assert_eq!(
        encode_rgba16f(&[[1.0, 0.5, 0.0, 1.0]]).unwrap(),
        vec![0, 60, 0, 56, 0, 0, 0, 60]
    );
    assert_eq!(encode_rgba16f(&[[1e-10; 4]]).unwrap(), vec![0; 8]);
}

#[test]
fn gpu_animated_shape_and_japanese_text_match_cpu_all_pixels_and_order() {
    let (p, id) = project();
    let s = snapshot(&p, id);
    let gpu = kronello_gpu::GpuContext::new().expect("GPU adapter required; no skip or fallback");
    eprintln!("RENDER-001 adapter: {:?}", gpu.adapter_info);
    let mut previous = Vec::new();
    for time in [t(0, 1), t(1, 3), t(2, 3), t(1, 1)] {
        let cpu = frame(&s, time);
        let gpu_frame = render_frame(
            &s,
            &fonts(),
            &gpu,
            FrameRequest {
                time,
                region: region(),
            },
        )
        .unwrap();
        compare(
            &cpu.pixels.linear,
            &gpu_frame.pixels.linear,
            2.0_f32.powi(-10),
        );
        compare(
            &cpu.pixels.display,
            &gpu_frame.pixels.display,
            2.0_f32.powi(-9),
        );
        previous.push((time, gpu_frame));
    }
    for (time, mut expected) in previous.into_iter().rev() {
        let mut actual = render_frame(
            &s,
            &fonts(),
            &gpu,
            FrameRequest {
                time,
                region: region(),
            },
        )
        .unwrap();
        // Resource observations depend on cache warmth, while every pixel and
        // semantic metadata field must remain independent of request order.
        actual.metadata.transfer_stats = None;
        expected.metadata.transfer_stats = None;
        actual.metadata.resource_cache_stats = None;
        expected.metadata.resource_cache_stats = None;
        assert_eq!(actual, expected);
    }
    let cpu = render_frame(
        &s,
        &fonts(),
        &CpuReferenceBackend,
        FrameRequest {
            time: t(1, 3),
            region: OutputRegion {
                origin: [16.0, 0.0],
                extent: [32.0, 32.0],
                pixels: [64, 64],
            },
        },
    )
    .unwrap();
    let actual = render_frame(
        &s,
        &fonts(),
        &gpu,
        FrameRequest {
            time: t(1, 3),
            region: cpu.metadata.region,
        },
    )
    .unwrap();
    compare(&cpu.pixels.linear, &actual.pixels.linear, 2.0_f32.powi(-10));
}
fn compare(expected: &[[f32; 4]], actual: &[[f32; 4]], tolerance: f32) {
    assert_eq!(expected.len(), actual.len());
    for (i, (e, a)) in expected.iter().zip(actual).enumerate() {
        for c in 0..4 {
            assert!(
                a[c].is_finite() && (a[c] - e[c]).abs() <= tolerance * e[c].abs().max(1.0),
                "pixel {i} channel {c}: expected {e:?}, actual {a:?}"
            );
        }
        assert!((0.0..=1.0).contains(&a[3]));
        if a[3] == 0.0 {
            assert_eq!(&a[..3], &[0.0; 3]);
        }
    }
}
#[test]
fn gpu_isolated_alpha_and_luma_mattes_and_sequence_match_reference() {
    let gpu = kronello_gpu::GpuContext::new().expect("GPU adapter required");
    let (p, id, source, matte) = group_project();
    for kind in [MatteKind::Alpha, MatteKind::Luminance] {
        let s = masked_snapshot(&p, id, source, matte, kind);
        let cpu = render_frame(
            &s,
            &[],
            &CpuReferenceBackend,
            FrameRequest {
                time: t(0, 1),
                region: region(),
            },
        )
        .unwrap();
        let actual = render_frame(
            &s,
            &[],
            &gpu,
            FrameRequest {
                time: t(0, 1),
                region: region(),
            },
        )
        .unwrap();
        compare(&cpu.pixels.linear, &actual.pixels.linear, 2.0_f32.powi(-10));
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("gpu-sequence");
        let sequence = render_sequence(
            &s,
            &[],
            &gpu,
            SequenceRequest {
                range: TimeRange::new(t(0, 1), t(1, 12)).unwrap(),
                frame_rate: FrameRate::new(24, 1).unwrap(),
                region: region(),
            },
            &out,
        )
        .unwrap();
        assert_eq!(sequence.frames.len(), 2);
        assert_eq!(sequence.frames[0].metadata.backend, "wgpu_rgba16f");
        let bytes = std::fs::read(out.join(&sequence.frames[0].numeric.name)).unwrap();
        assert_eq!(bytes, encode_rgba16f(&actual.pixels.linear).unwrap());
    }
}

#[test]
fn supported_stroke_styles_nonuniform_transform_and_zero_scale() {
    let (mut n, mut shape) = rectangle([16.0; 2], Color::from_srgb8([255, 0, 0], None));
    let color = constant(
        "kronello.shape.stroke_color",
        Value::Color(Color::from_srgb8([0, 0, 255], None)),
    );
    let width = constant("kronello.stroke_width", scalar(4.0));
    let join = constant("kronello.shape.stroke_join", Value::Enum("round".into()));
    let cap = constant("kronello.shape.stroke_cap", Value::Enum("round".into()));
    let miter = constant("kronello.shape.miter_limit", scalar(4.0));
    shape.stroke = Some(Stroke {
        options: None,
        gradient: None,
        color: color.id(),
        width: width.id(),
        join: join.id(),
        cap: cap.id(),
        miter_limit: miter.id(),
    });
    n.properties.extend([color, width, join, cap, miter]);
    let c = composition(vec![n]);
    let id = c.id;
    let mut p = Project {
        compositions: vec![DocumentObject::Known(c)],
        shapes: vec![DocumentObject::Known(shape)],
        ..Project::default()
    };
    let s = snapshot(&p, id);
    let bounds = build_scene_ir(&s, t(0, 1), &[]).unwrap().nodes[0].bounds;
    assert_eq!(
        bounds.layout_bounds,
        Some(DesignBounds {
            min: [0.0; 2],
            max: [16.0; 2]
        })
    );
    assert_eq!(
        bounds.ink_bounds,
        Some(DesignBounds {
            min: [-2.0; 2],
            max: [18.0; 2]
        })
    );
    assert_eq!(bounds.visual_bounds, bounds.ink_bounds);
    let frame = render_frame(
        &s,
        &[],
        &CpuReferenceBackend,
        FrameRequest {
            time: t(0, 1),
            region: region(),
        },
    )
    .unwrap();
    assert_eq!(frame.pixels.linear[8 * 64 + 16], [0.0, 0.0, 1.0, 1.0]);
    comp_mut(&mut p).nodes[0]
        .properties
        .push(constant("kronello.transform.scale", v2(2.0, 1.0)));
    let s = snapshot(&p, id);
    assert_eq!(
        render_frame(
            &s,
            &[],
            &CpuReferenceBackend,
            FrameRequest {
                time: t(0, 1),
                region: region()
            }
        )
        .unwrap_err()
        .code(),
        "UNSUPPORTED_FEATURE"
    );
    comp_mut(&mut p).nodes[0].properties.pop();
    comp_mut(&mut p).nodes[0].properties[5]
        .set_source(
            PropertySource::Constant(Value::Enum("miter".into())),
            &render_registry(),
        )
        .unwrap();
    let s = snapshot(&p, id);
    render_frame(
        &s,
        &[],
        &CpuReferenceBackend,
        FrameRequest {
            time: t(0, 1),
            region: region(),
        },
    )
    .unwrap();
    let DocumentObject::Known(shape) = &mut p.shapes[0] else {
        panic!()
    };
    shape.stroke = None;
    comp_mut(&mut p).nodes[0]
        .properties
        .push(constant("kronello.transform.scale", v2(0.0, 0.0)));
    let s = snapshot(&p, id);
    let frame = render_frame(
        &s,
        &[],
        &CpuReferenceBackend,
        FrameRequest {
            time: t(0, 1),
            region: region(),
        },
    )
    .unwrap();
    assert!(frame.pixels.linear.iter().all(|p| *p == [0.0; 4]));
}

#[test]
fn later_frame_failure_rolls_back_staged_files_and_hdr_numeric_truth_is_unclipped() {
    struct Failing(std::cell::Cell<u32>);
    impl RenderBackend for Failing {
        fn name(&self) -> &str {
            "injected_failure"
        }
        fn execute(&self, dag: &RenderDag) -> Result<BackendFrame, RenderError> {
            let count = self.0.get();
            self.0.set(count + 1);
            if count == 1 {
                return Err(RenderError::Backend {
                    code: "DEVICE_UNAVAILABLE",
                    message: "injected second frame failure".into(),
                });
            }
            CpuReferenceBackend.execute(dag)
        }
    }
    let (n, shape) = rectangle(
        [16.0; 2],
        Color::new(ColorSpace::LinearRec709, [2.0, -0.5, 0.0], 1.0).unwrap(),
    );
    let c = composition(vec![n]);
    let id = c.id;
    let p = Project {
        compositions: vec![DocumentObject::Known(c)],
        shapes: vec![DocumentObject::Known(shape)],
        ..Project::default()
    };
    let s = snapshot(&p, id);
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("incomplete");
    let request = SequenceRequest {
        range: TimeRange::new(t(0, 1), t(1, 12)).unwrap(),
        frame_rate: FrameRate::new(24, 1).unwrap(),
        region: region(),
    };
    assert_eq!(
        render_sequence(&s, &[], &Failing(std::cell::Cell::new(0)), request, &out)
            .unwrap_err()
            .code(),
        "DEVICE_UNAVAILABLE"
    );
    assert!(!out.exists());
    let out = dir.path().join("complete");
    let sequence = render_sequence(&s, &[], &CpuReferenceBackend, request, &out).unwrap();
    let numeric = kronello_gpu::decode_rgba16f(
        &std::fs::read(out.join(&sequence.frames[0].numeric.name)).unwrap(),
    )
    .unwrap();
    assert_eq!(numeric[2 * 64 + 2], [2.0, -0.5, 0.0, 1.0]);
    let bytes = std::fs::read(out.join(&sequence.frames[0].display.name)).unwrap();
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .unwrap();
    let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
    reader.next_frame(&mut buffer).unwrap();
    let pixel = &buffer[(2 * 64 + 2) * 8..(2 * 64 + 3) * 8];
    assert_eq!(pixel, &[255, 255, 0, 0, 0, 0, 255, 255]);
    assert_eq!(sequence.frames[0].metadata.numeric.clipping, "none");
    assert!(
        sequence.frames[0]
            .metadata
            .display
            .clipping
            .contains("no_tone_mapping")
    );
}

fn cache_project() -> (Project, CompositionId) {
    let (mut p, id) = project();
    let mut text_node = comp_mut(&mut p).nodes[1].clone();
    text_node.id = NodeId::new();
    let mut ids = BTreeMap::new();
    for property in &mut text_node.properties {
        let replacement = prop(
            property.descriptor().key.as_str(),
            property.source().clone(),
        );
        ids.insert(property.id(), replacement.id());
        *property = replacement;
    }
    let DocumentObject::Known(mut text) = p.texts[0].clone() else {
        panic!()
    };
    text.id = ContentId::new();
    text.text = "文字".into();
    text.styles[0].range.end = text.text.len();
    text.styles[0].size = ids[&text.styles[0].size];
    text.styles[0].fill = ids[&text.styles[0].fill];
    text.wrap_width = ids[&text.wrap_width];
    text.line_height = ids[&text.line_height];
    text.alignment = ids[&text.alignment];
    text_node.kind = NodeKind::Text {
        content_ref: text.id,
    };
    comp_mut(&mut p).root_nodes.push(text_node.id);
    comp_mut(&mut p).nodes.push(text_node);
    p.texts.push(DocumentObject::Known(text));
    (p, id)
}
fn change_text_property(p: &mut Project, key: &str, value: Value) {
    let node = &mut comp_mut(p).nodes[1];
    if let Some(property) = node
        .properties
        .iter_mut()
        .find(|p| p.descriptor().key.as_str() == key)
    {
        property
            .set_source(PropertySource::Constant(value), &render_registry())
            .unwrap();
    } else {
        node.properties.push(constant(key, value));
    }
}
fn text_mut(p: &mut Project) -> &mut TextDocument {
    let DocumentObject::Known(text) = &mut p.texts[0] else {
        panic!()
    };
    text
}
fn cached_frame(
    s: &RenderSnapshot,
    time: Time,
    region: OutputRegion,
    cache: &mut RenderCache,
) -> RenderedFrame {
    render_frame_with_cache(
        s,
        &fonts(),
        &CpuReferenceBackend,
        FrameRequest { time, region },
        cache,
    )
    .unwrap()
}

#[test]
fn cache_position_opacity_rotation_scale_reuse_layout_across_renders_and_frames() {
    let (p, id) = cache_project();
    for (key, value) in [
        ("kronello.transform.position", v2(25.0, 6.0)),
        ("kronello.opacity", scalar(0.5)),
        ("kronello.transform.rotation", Value::Angle(f(30.0))),
        ("kronello.transform.scale", v2(2.0, 2.0)),
    ] {
        let mut cache = RenderCache::default();
        cached_frame(&snapshot(&p, id), t(0, 1), region(), &mut cache);
        cache.reset_stats();
        let mut changed = p.clone();
        change_text_property(&mut changed, key, value);
        let s = snapshot(&changed, id);
        assert_eq!(
            cached_frame(&s, t(0, 1), region(), &mut cache),
            frame(&s, t(0, 1))
        );
        assert_eq!(cache.stats().layout.hits, 2, "{key}");
        assert_eq!(cache.stats().layout.misses, 0, "{key}");
        if key == "kronello.transform.position" {
            assert_eq!(cache.stats().geometry.hits, 6);
            assert_eq!(cache.stats().geometry.misses, 0);
            assert_eq!(cache.stats().raster.hits, 3);
            assert_eq!(cache.stats().raster.misses, 3);
        }
        cache.reset_stats();
        cached_frame(&s, t(1, 3), region(), &mut cache);
        assert_eq!(cache.stats().layout.hits, 2);
        assert_eq!(cache.stats().layout.misses, 0);
    }
}

#[test]
fn cache_color_invalidates_only_changed_text_raster() {
    let (mut p, id) = cache_project();
    let mut cache = RenderCache::default();
    cached_frame(&snapshot(&p, id), t(0, 1), region(), &mut cache);
    cache.reset_stats();
    change_text_property(
        &mut p,
        "kronello.fill_color",
        Value::Color(Color::from_srgb8([230, 80, 20], None)),
    );
    let s = snapshot(&p, id);
    assert_eq!(
        cached_frame(&s, t(0, 1), region(), &mut cache),
        frame(&s, t(0, 1))
    );
    let stats = cache.stats();
    assert_eq!((stats.layout.hits, stats.layout.misses), (2, 0));
    assert_eq!((stats.geometry.hits, stats.geometry.misses), (6, 0));
    assert_eq!((stats.raster.hits, stats.raster.misses), (3, 3));
}

#[test]
fn cache_text_size_wrap_line_height_alignment_invalidate_only_affected_layout_and_downstream() {
    let (p, id) = cache_project();
    for change in 0..5 {
        let mut cache = RenderCache::default();
        cached_frame(&snapshot(&p, id), t(0, 1), region(), &mut cache);
        cache.reset_stats();
        let mut changed = p.clone();
        match change {
            0 => text_mut(&mut changed).text = "本語日".into(),
            1 => change_text_property(&mut changed, "kronello.text.font_size", scalar(14.0)),
            2 => change_text_property(&mut changed, "kronello.text.wrap_width", scalar(22.0)),
            3 => change_text_property(&mut changed, "kronello.text.line_height", scalar(20.0)),
            _ => change_text_property(
                &mut changed,
                "kronello.text.alignment",
                Value::Enum("end".into()),
            ),
        }
        let s = snapshot(&changed, id);
        assert_eq!(
            cached_frame(&s, t(0, 1), region(), &mut cache),
            frame(&s, t(0, 1))
        );
        let stats = cache.stats();
        assert_eq!(
            (stats.layout.hits, stats.layout.misses),
            (1, 1),
            "change {change}"
        );
        assert_eq!(
            (stats.geometry.hits, stats.geometry.misses),
            (3, 3),
            "change {change}"
        );
        assert_eq!(
            (stats.raster.hits, stats.raster.misses),
            (3, 3),
            "change {change}"
        );
    }
}

#[test]
fn cache_font_lock_change_invalidates_downstream_even_with_identical_outlines() {
    let (mut p, id) = cache_project();
    let mut cache = RenderCache::default();
    let before = cached_frame(&snapshot(&p, id), t(0, 1), region(), &mut cache);
    cache.reset_stats();
    // Valid trailing font bytes change the lock, while preserving every glyph.
    let mut changed_bytes = font().0.clone();
    changed_bytes.push(0);
    let changed_font = pin_font(&changed_bytes, 0).unwrap();
    assert_ne!(changed_font.sha256, font().1.sha256);
    text_mut(&mut p).styles[0].font = changed_font.clone();
    let sources = [
        fonts()[0],
        FontData {
            identity: &changed_font,
            bytes: &changed_bytes,
        },
    ];
    let s = snapshot(&p, id);
    let request = FrameRequest {
        time: t(0, 1),
        region: region(),
    };
    let actual =
        render_frame_with_cache(&s, &sources, &CpuReferenceBackend, request, &mut cache).unwrap();
    assert_eq!(
        actual,
        render_frame(&s, &sources, &CpuReferenceBackend, request).unwrap()
    );
    assert_eq!(actual.pixels, before.pixels);
    let stats = cache.stats();
    assert_eq!((stats.layout.hits, stats.layout.misses), (1, 1));
    assert_eq!((stats.geometry.hits, stats.geometry.misses), (3, 3));
    assert_eq!((stats.raster.hits, stats.raster.misses), (3, 3));
}

#[test]
fn cache_values_use_content_runtime_property_and_exact_time_excluding_revision() {
    let (p, id) = cache_project();
    let s = snapshot(&p, id);
    let mut cache = RenderCache::default();
    let expected = cached_frame(&s, t(1, 3), region(), &mut cache);
    let first = cache.stats();
    assert!(first.values.misses > 0);
    assert_eq!(first.values.inserts, first.values.misses);
    cache.reset_stats();
    let revision = RenderSnapshot::new(&p, id, 900, RenderProfile::default()).unwrap();
    assert_ne!(revision.content_hash().unwrap(), s.content_hash().unwrap());
    assert_eq!(
        revision.evaluation_content_hash().unwrap(),
        s.evaluation_content_hash().unwrap()
    );
    assert_eq!(
        cached_frame(&revision, t(2, 6), region(), &mut cache).pixels,
        expected.pixels
    );
    assert_eq!(cache.stats().values.misses, 0);
    assert!(cache.stats().values.hits >= first.values.misses);
    cache.reset_stats();
    cached_frame(&s, t(1, 2), region(), &mut cache);
    assert_eq!(cache.stats().values.misses, first.values.misses);
    let mut changed = p.clone();
    change_text_property(&mut changed, "kronello.opacity", scalar(0.6));
    cache.reset_stats();
    cached_frame(&snapshot(&changed, id), t(1, 3), region(), &mut cache);
    assert!(cache.stats().values.misses > 0);
}

#[test]
fn cache_state_order_eviction_clear_resolution_match_direct_cpu_execution() {
    let (p, id) = cache_project();
    let s = snapshot(&p, id);
    let tiny = CacheCapacity {
        entries: 1,
        bytes: 4096,
    };
    let mut tiny_cache = RenderCache::new(CacheConfig {
        values: tiny,
        layout: tiny,
        geometry: tiny,
        raster: tiny,
        temporal: tiny,
        simulation: tiny,
    });
    let mut warm = RenderCache::default();
    let mut disabled = RenderCache::new(CacheConfig::disabled());
    let requests = [
        (t(0, 1), region()),
        (t(1, 3), region()),
        (
            t(1, 1),
            OutputRegion {
                pixels: [32, 16],
                ..region()
            },
        ),
        (
            t(0, 1),
            OutputRegion {
                origin: [4.0, 2.0],
                ..region()
            },
        ),
    ];
    for i in [3, 0, 2, 1, 0, 3] {
        let (time, r) = requests[i];
        let scene = build_scene_ir(&s, time, &fonts()).unwrap();
        let dag = build_render_dag(&scene, s.profile(), r).unwrap();
        let direct = CpuReferenceBackend.execute(&dag).unwrap();
        for cache in [&mut warm, &mut tiny_cache, &mut disabled] {
            let actual = cached_frame(&s, time, r, cache);
            assert_eq!(actual.pixels, direct);
            assert_eq!(
                encode_rgba16f(&actual.pixels.linear).unwrap(),
                encode_rgba16f(&direct.linear).unwrap()
            );
        }
    }
    assert!(tiny_cache.stats().values.evictions > 0);
    for stats in [
        tiny_cache.stats().values,
        tiny_cache.stats().layout,
        tiny_cache.stats().geometry,
        tiny_cache.stats().raster,
    ] {
        assert!(stats.entries <= 1 && stats.bytes <= 4096);
    }
    for stats in [
        disabled.stats().values,
        disabled.stats().layout,
        disabled.stats().geometry,
        disabled.stats().raster,
    ] {
        assert_eq!(stats.entries, 0);
    }
    let before = cached_frame(&s, t(0, 1), region(), &mut warm);
    warm.clear();
    assert_eq!(warm.stats().layout.entries, 0);
    assert_eq!(cached_frame(&s, t(0, 1), region(), &mut warm), before);
}

#[test]
fn cache_warm_layout_still_rejects_missing_corrupt_and_duplicate_fonts() {
    let (p, id) = cache_project();
    let s = snapshot(&p, id);
    let mut cache = RenderCache::default();
    cached_frame(&s, t(0, 1), region(), &mut cache);
    let request = FrameRequest {
        time: t(0, 1),
        region: region(),
    };
    for (sources, code) in [
        (vec![], "ASSET_MISSING"),
        (
            vec![FontData {
                identity: &font().1,
                bytes: b"broken",
            }],
            "ASSET_HASH_MISMATCH",
        ),
        (vec![fonts()[0], fonts()[0]], "UNSUPPORTED_FEATURE"),
    ] {
        assert_eq!(
            render_frame_with_cache(&s, &sources, &CpuReferenceBackend, request, &mut cache)
                .unwrap_err()
                .code(),
            code
        );
    }
}

#[test]
fn cache_sequence_matches_uncached_artifacts_and_reports_cross_frame_hits() {
    let (p, id) = cache_project();
    let s = snapshot(&p, id);
    let root = tempfile::tempdir().unwrap();
    let request = SequenceRequest {
        range: TimeRange::new(t(0, 1), t(1, 12)).unwrap(),
        frame_rate: FrameRate::new(24, 1).unwrap(),
        region: region(),
    };
    let mut cache = RenderCache::default();
    let cached = render_sequence_with_cache(
        &s,
        &fonts(),
        &CpuReferenceBackend,
        request,
        root.path().join("cached"),
        &mut cache,
    )
    .unwrap();
    let direct = render_sequence(
        &s,
        &fonts(),
        &CpuReferenceBackend,
        request,
        root.path().join("direct"),
    )
    .unwrap();
    assert_eq!(cached, direct);
    for entry in std::fs::read_dir(root.path().join("direct")).unwrap() {
        let name = entry.unwrap().file_name();
        assert_eq!(
            std::fs::read(root.path().join("direct").join(&name)).unwrap(),
            std::fs::read(root.path().join("cached").join(&name)).unwrap()
        );
    }
    assert_eq!(
        (cache.stats().layout.hits, cache.stats().layout.misses),
        (2, 2)
    );
    assert_eq!(
        (cache.stats().geometry.hits, cache.stats().geometry.misses),
        (6, 6)
    );
    assert_eq!(
        (cache.stats().raster.hits, cache.stats().raster.misses),
        (5, 7)
    );
}

#[test]
fn cache_shape_color_and_output_mapping_invalidate_at_their_own_levels() {
    let (mut p, id) = cache_project();
    let mut cache = RenderCache::default();
    cached_frame(&snapshot(&p, id), t(0, 1), region(), &mut cache);
    cache.reset_stats();
    comp_mut(&mut p).nodes[0]
        .properties
        .iter_mut()
        .find(|p| p.descriptor().key.as_str() == "kronello.fill_color")
        .unwrap()
        .set_source(
            PropertySource::Constant(Value::Color(Color::from_srgb8([10, 80, 230], None))),
            &render_registry(),
        )
        .unwrap();
    let s = snapshot(&p, id);
    assert_eq!(
        cached_frame(&s, t(0, 1), region(), &mut cache),
        frame(&s, t(0, 1))
    );
    let stats = cache.stats();
    assert_eq!((stats.layout.hits, stats.layout.misses), (2, 0));
    assert_eq!((stats.geometry.hits, stats.geometry.misses), (6, 0));
    assert_eq!((stats.raster.hits, stats.raster.misses), (5, 1));
    cache.reset_stats();
    cached_frame(
        &s,
        t(0, 1),
        OutputRegion {
            origin: [1.0, 2.0],
            ..region()
        },
        &mut cache,
    );
    let stats = cache.stats();
    assert_eq!((stats.layout.hits, stats.layout.misses), (2, 0));
    assert_eq!((stats.geometry.hits, stats.geometry.misses), (6, 0));
    assert_eq!((stats.raster.hits, stats.raster.misses), (0, 6));
    cache.reset_stats();
    cached_frame(
        &s,
        t(0, 1),
        OutputRegion {
            pixels: [32, 16],
            ..region()
        },
        &mut cache,
    );
    let stats = cache.stats();
    assert_eq!((stats.layout.hits, stats.layout.misses), (2, 0));
    assert_eq!((stats.geometry.hits, stats.geometry.misses), (0, 6));
    assert_eq!((stats.raster.hits, stats.raster.misses), (0, 6));
}

fn gradient_project(animated: bool) -> (Project, CompositionId) {
    let (mut n, mut shape) = rectangle([16.0; 2], Color::from_srgb8([255, 0, 0], None));
    let initial = constant(
        "kronello.shape.gradient_color",
        Value::Color(Color::from_srgb8([255, 0, 0], None)),
    );
    shape.fill.as_mut().unwrap().color = initial.id();
    n.properties[2] = initial;
    let color_curve = AnimationCurve::new(
        CurveId::new(),
        ValueType::Color,
        vec![
            Keyframe {
                time: t(0, 1),
                value: Value::Color(
                    Color::new(ColorSpace::LinearRec709, [0.0, 1.0, 0.0], 1.0).unwrap(),
                ),
                interpolation: CurveInterpolation::Linear,
            },
            Keyframe {
                time: t(1, 1),
                value: Value::Color(
                    Color::new(ColorSpace::LinearRec709, [0.0, 0.0, 1.0], 1.0).unwrap(),
                ),
                interpolation: CurveInterpolation::Linear,
            },
        ],
    )
    .unwrap();
    let offset_curve = AnimationCurve::new(
        CurveId::new(),
        ValueType::Scalar,
        vec![
            Keyframe {
                time: t(0, 1),
                value: scalar(0.5),
                interpolation: CurveInterpolation::Linear,
            },
            Keyframe {
                time: t(1, 1),
                value: scalar(1.0),
                interpolation: CurveInterpolation::Linear,
            },
        ],
    )
    .unwrap();
    let color = if animated {
        prop(
            "kronello.shape.gradient_color",
            PropertySource::Curve(color_curve.id()),
        )
    } else {
        constant(
            "kronello.shape.gradient_color",
            Value::Color(Color::from_srgb8([0, 0, 255], None)),
        )
    };
    let start = constant("kronello.shape.gradient_offset", scalar(0.0));
    let end = if animated {
        prop(
            "kronello.shape.gradient_offset",
            PropertySource::Curve(offset_curve.id()),
        )
    } else {
        constant("kronello.shape.gradient_offset", scalar(1.0))
    };
    let fill = shape.fill.as_mut().unwrap();
    fill.gradient = Some(Box::new(Gradient::Linear {
        options: Default::default(),
        start: [f(0.0); 2],
        end: [f(16.0), f(0.0)],
        stops: vec![
            GradientStop {
                color: fill.color,
                offset: start.id(),
            },
            GradientStop {
                color: color.id(),
                offset: end.id(),
            },
        ],
    }));
    n.properties.extend([color, start, end]);
    let c = composition(vec![n]);
    let id = c.id;
    (
        Project {
            compositions: vec![DocumentObject::Known(c)],
            shapes: vec![DocumentObject::Known(shape)],
            curves: if animated {
                vec![
                    DocumentObject::Known(color_curve),
                    DocumentObject::Known(offset_curve),
                ]
            } else {
                vec![]
            },
            ..Project::default()
        },
        id,
    )
}
#[test]
fn gradient_stop_color_and_offset_animate_through_scene_evaluator() {
    let (p, id) = gradient_project(true);
    let s = snapshot(&p, id);
    for (time, offset, green, blue) in [
        (t(0, 1), 0.5, 1.0, 0.0),
        (t(1, 2), 0.75, 0.5, 0.5),
        (t(1, 1), 1.0, 0.0, 1.0),
    ] {
        let ir = build_scene_ir(&s, time, &[]).unwrap();
        let SceneContent::Shape { resolved, .. } = &ir.nodes[0].content else {
            panic!()
        };
        let gradient = resolved.fill.as_ref().unwrap().gradient.as_ref().unwrap();
        assert_eq!(gradient.stops[1].offset, offset);
        let color = gradient.stops[1].color.components();
        assert_eq!(color.g.get(), green);
        assert_eq!(color.b.get(), blue);
        render_frame(
            &s,
            &[],
            &CpuReferenceBackend,
            FrameRequest {
                time,
                region: region(),
            },
        )
        .unwrap();
    }
    let versions = s.semantic_versions();
    assert_eq!(versions.stroke_geometry, STROKE_GEOMETRY_VERSION);
    assert_eq!(
        versions.gradient_interpolation,
        GRADIENT_INTERPOLATION_VERSION
    );
    assert_eq!(versions.coverage, COVERAGE_VERSION);
}

#[test]
fn vec004_legacy_project_without_options_keeps_identical_pixels() {
    for radial in [false, true] {
        let (mut p, id) = gradient_project(true);
        if radial {
            let DocumentObject::Known(shape) = &mut p.shapes[0] else {
                panic!()
            };
            let gradient = shape.fill.as_mut().unwrap().gradient.as_mut().unwrap();
            **gradient = Gradient::Radial {
                center: [f(8.0); 2],
                radius: f(8.0),
                stops: gradient.stops().to_vec(),
                options: Default::default(),
            };
        }
        let mut json = serde_json::to_value(&p).unwrap();
        let shape = &mut json["shapes"][0];
        for paint in ["fill", "stroke"] {
            if let Some(gradient) = shape
                .get_mut(paint)
                .and_then(|paint| paint.get_mut("gradient"))
                .and_then(serde_json::Value::as_object_mut)
            {
                assert!(gradient.remove("options").is_some());
            }
        }
        serde_json::from_str::<Shape>(&serde_json::to_string(shape).unwrap())
            .expect("legacy shape must remain supported");
        let old: Project = serde_json::from_value(json).unwrap();
        for working_space in [ColorSpace::LinearRec709, ColorSpace::LinearRec2020] {
            let profile = RenderProfile {
                working_space,
                ..Default::default()
            };
            let current = RenderSnapshot::new(&p, id, 1, profile).unwrap();
            let legacy = RenderSnapshot::new(&old, id, 1, profile).unwrap();
            for time in [t(0, 1), t(1, 2), t(1, 1)] {
                let request = FrameRequest {
                    time,
                    region: region(),
                };
                assert_eq!(
                    render_frame(&current, &[], &CpuReferenceBackend, request)
                        .unwrap()
                        .pixels,
                    render_frame(&legacy, &[], &CpuReferenceBackend, request)
                        .unwrap()
                        .pixels
                );
            }
        }
    }
}

#[test]
fn vec004_unknown_interpolation_version_has_shared_unsupported_code() {
    let (mut p, id) = gradient_project(false);
    let DocumentObject::Known(shape) = &mut p.shapes[0] else {
        panic!()
    };
    let Gradient::Linear { options, .. } = shape
        .fill
        .as_mut()
        .unwrap()
        .gradient
        .as_deref_mut()
        .unwrap()
    else {
        panic!()
    };
    options.interpolation_version = 2;
    let gradient = shape.fill.as_ref().unwrap().gradient.clone().unwrap();
    let error = render_frame(
        &snapshot(&p, id),
        &[],
        &CpuReferenceBackend,
        FrameRequest {
            time: t(0, 1),
            region: region(),
        },
    )
    .unwrap_err();
    assert_eq!(error.code(), "UNSUPPORTED_FEATURE");
    assert!(matches!(
        error,
        RenderError::Shape(ShapeError::UnsupportedGradientVersion)
    ));

    let stop_ids: Vec<_> = gradient
        .stops()
        .iter()
        .flat_map(|s| [s.color, s.offset])
        .collect();
    let properties: Vec<_> = comp_mut(&mut p).nodes[0]
        .properties
        .iter()
        .filter(|property| stop_ids.contains(&property.id()))
        .cloned()
        .collect();
    let (mut text_project, text_id) = project();
    let DocumentObject::Known(text) = &mut text_project.texts[0] else {
        panic!()
    };
    text.styles[0].gradient = Some(gradient);
    comp_mut(&mut text_project).nodes[1]
        .properties
        .extend(properties);
    let error = render_frame(
        &snapshot(&text_project, text_id),
        &fonts(),
        &CpuReferenceBackend,
        FrameRequest {
            time: t(0, 1),
            region: region(),
        },
    )
    .unwrap_err();
    assert_eq!(error.code(), "UNSUPPORTED_FEATURE");
    assert!(matches!(
        error,
        RenderError::Text(TextError::Gradient(ShapeError::UnsupportedGradientVersion))
    ));
}

#[test]
fn vec004_bbox_and_gradient_transform_match_local_mapping_and_reject_degeneracy() {
    let (mut p, id) = gradient_project(false);
    let local = cached_frame(
        &snapshot(&p, id),
        t(0, 1),
        region(),
        &mut RenderCache::default(),
    );
    let DocumentObject::Known(shape) = &mut p.shapes[0] else {
        panic!()
    };
    let Gradient::Linear { end, options, .. } = shape
        .fill
        .as_mut()
        .unwrap()
        .gradient
        .as_deref_mut()
        .unwrap()
    else {
        panic!()
    };
    *end = [f(0.5), f(0.0)];
    options.units = GradientUnits::ObjectBoundingBox;
    options.transform = [[f(2.0), f(0.0), f(0.0)], [f(0.0), f(1.0), f(0.0)]];
    let bbox = cached_frame(
        &snapshot(&p, id),
        t(0, 1),
        region(),
        &mut RenderCache::default(),
    );
    assert_eq!(bbox.pixels.linear, local.pixels.linear);
    let ir = build_scene_ir(&snapshot(&p, id), t(0, 1), &[]).unwrap();
    let dag = build_render_dag(&ir, RenderProfile::default(), region()).unwrap();
    let lowered = dag
        .nodes()
        .iter()
        .find_map(|n| match n {
            DagNode::CoverageDraw { path, .. } => path.fill_gradient.as_ref(),
            _ => None,
        })
        .unwrap();
    assert_eq!(lowered.options.units, GradientUnits::LocalDesign);
    assert_eq!(lowered.options.transform[0][0], f(32.0));
    // Geometry cache is independent of units, spread, and interpolation paint.
    let mut cache = RenderCache::default();
    cached_frame(&snapshot(&p, id), t(0, 1), region(), &mut cache);
    cache.reset_stats();
    let DocumentObject::Known(shape) = &mut p.shapes[0] else {
        panic!()
    };
    let Gradient::Linear { options, .. } = shape
        .fill
        .as_mut()
        .unwrap()
        .gradient
        .as_deref_mut()
        .unwrap()
    else {
        panic!()
    };
    options.interpolation = GradientInterpolation::SrgbStraight;
    cached_frame(&snapshot(&p, id), t(0, 1), region(), &mut cache);
    assert!(cache.stats().geometry.hits > 0);
    assert!(cache.stats().raster.misses > 0);
    comp_mut(&mut p).nodes[0].properties[0]
        .set_source(PropertySource::Constant(v2(0.0, 16.0)), &render_registry())
        .unwrap();
    assert!(
        render_frame(
            &snapshot(&p, id),
            &[],
            &CpuReferenceBackend,
            FrameRequest {
                time: t(0, 1),
                region: region()
            }
        )
        .is_err()
    );
}

#[test]
fn vec004_text_gradient_reuses_layout_and_keeps_one_object_coordinate_system() {
    let (mut p, id) = project();
    let plain = build_scene_ir(&snapshot(&p, id), t(0, 1), &fonts()).unwrap();
    let red = constant(
        "kronello.shape.gradient_color",
        Value::Color(Color::from_srgb8([255, 0, 0], None)),
    );
    let blue = constant(
        "kronello.shape.gradient_color",
        Value::Color(Color::from_srgb8([0, 0, 255], None)),
    );
    let start = constant("kronello.shape.gradient_offset", scalar(0.0));
    let end = constant("kronello.shape.gradient_offset", scalar(1.0));
    let gradient = Gradient::Linear {
        start: [f(0.0); 2],
        end: [f(1.0), f(0.0)],
        options: GradientOptions {
            units: GradientUnits::ObjectBoundingBox,
            interpolation: GradientInterpolation::SrgbStraight,
            ..Default::default()
        },
        stops: vec![
            GradientStop {
                color: red.id(),
                offset: start.id(),
            },
            GradientStop {
                color: blue.id(),
                offset: end.id(),
            },
        ],
    };
    let mut cache = RenderCache::default();
    let before = cached_frame(&snapshot(&p, id), t(0, 1), region(), &mut cache);
    let DocumentObject::Known(text) = &mut p.texts[0] else {
        panic!()
    };
    text.styles[0].gradient = Some(Box::new(gradient));
    comp_mut(&mut p).nodes[1]
        .properties
        .extend([red, blue, start, end]);
    let imported: Project = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
    assert_eq!(imported, p);
    p = imported;
    cache.reset_stats();
    let after = cached_frame(&snapshot(&p, id), t(0, 1), region(), &mut cache);
    assert_ne!(before.pixels.linear, after.pixels.linear);
    assert!(cache.stats().layout.hits > 0);
    assert_eq!(cache.stats().layout.misses, 0);
    let painted = build_scene_ir(&snapshot(&p, id), t(0, 1), &fonts()).unwrap();
    let extract = |ir: &SceneIr| {
        ir.nodes
            .iter()
            .find_map(|n| match &n.content {
                SceneContent::Text(l) => Some(l.clone()),
                _ => None,
            })
            .unwrap()
    };
    let mut layout = extract(&painted);
    assert!(layout.glyphs.iter().all(|g| g.gradient.is_some()));
    for g in &mut layout.glyphs {
        g.gradient = None;
    }
    assert_eq!(layout, extract(&plain));
    let dag = build_render_dag(&painted, RenderProfile::default(), region()).unwrap();
    let gradients: Vec<_> = dag
        .nodes()
        .iter()
        .filter_map(|n| match n {
            DagNode::CoverageDraw { path, .. } => path.fill_gradient.as_ref(),
            _ => None,
        })
        .collect();
    assert_eq!(gradients.len(), layout.glyphs.len());
    assert!(
        gradients
            .windows(2)
            .all(|pair| pair[0].options.transform == pair[1].options.transform)
    );
}

fn vec004_projects() -> Vec<(Project, CompositionId)> {
    (0..4)
        .map(|kind| {
            let (mut p, id) = gradient_project(true);
            let DocumentObject::Known(shape) = &mut p.shapes[0] else {
                panic!()
            };
            let stops = shape
                .fill
                .as_ref()
                .unwrap()
                .gradient
                .as_ref()
                .unwrap()
                .stops()
                .to_vec();
            let options = GradientOptions {
                units: GradientUnits::ObjectBoundingBox,
                spread: if kind % 2 == 0 {
                    GradientSpread::Repeat
                } else {
                    GradientSpread::Reflect
                },
                interpolation: [
                    GradientInterpolation::WorkingLinearPremultiplied,
                    GradientInterpolation::WorkingLinearStraight,
                    GradientInterpolation::SrgbStraight,
                    GradientInterpolation::SrgbPremultiplied,
                ][kind],
                transform: [[f(0.8), f(0.15), f(0.05)], [f(-0.1), f(0.9), f(0.05)]],
                ..Default::default()
            };
            shape.fill.as_mut().unwrap().gradient = Some(Box::new(match kind {
                0 => Gradient::Linear {
                    start: [f(0.0); 2],
                    end: [f(0.5), f(0.0)],
                    stops,
                    options,
                },
                1 => Gradient::Radial {
                    center: [f(0.5); 2],
                    radius: f(0.4),
                    stops,
                    options,
                },
                2 => Gradient::FocalRadial {
                    center: [f(0.5); 2],
                    radius: f(0.4),
                    focal: [f(0.4), f(0.5)],
                    focal_radius: f(0.05),
                    stops,
                    options,
                },
                _ => Gradient::Conic {
                    center: [f(0.5); 2],
                    start_angle: f(20.0),
                    sweep_angle: f(240.0),
                    stops,
                    options,
                },
            }));
            comp_mut(&mut p).nodes[0].properties.extend([
                constant("kronello.transform.position", v2(4.0, 3.0)),
                constant("kronello.transform.rotation", Value::Angle(f(15.0))),
            ]);
            (p, id)
        })
        .collect()
}

#[test]
fn vec004_all_geometries_lower_through_bbox_animation_and_cache() {
    for (p, id) in vec004_projects() {
        let imported: Project = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(imported, p);
        let p = imported;
        for working_space in [ColorSpace::LinearRec709, ColorSpace::LinearRec2020] {
            let s = RenderSnapshot::new(
                &p,
                id,
                1,
                RenderProfile {
                    working_space,
                    ..Default::default()
                },
            )
            .unwrap();
            let mut cache = RenderCache::default();
            for time in [t(1, 1), t(0, 1), t(1, 2), t(1, 1)] {
                let request = FrameRequest {
                    time,
                    region: region(),
                };
                let direct = render_frame(&s, &[], &CpuReferenceBackend, request).unwrap();
                let cached =
                    render_frame_with_cache(&s, &[], &CpuReferenceBackend, request, &mut cache)
                        .unwrap();
                assert_eq!(direct.pixels, cached.pixels);
                assert!(direct.pixels.linear.iter().flatten().all(|v| v.is_finite()));
            }
            assert!(cache.stats().geometry.hits > 0);
            assert!(cache.stats().raster.hits > 0);
        }
    }
}

#[test]
fn gpu_vec004_bbox_transform_animation_match_cpu_reference() {
    let gpu = kronello_gpu::GpuContext::new().expect("GPU required; no fallback");
    for (p, id) in vec004_projects() {
        for working_space in [ColorSpace::LinearRec709, ColorSpace::LinearRec2020] {
            let s = RenderSnapshot::new(
                &p,
                id,
                1,
                RenderProfile {
                    working_space,
                    ..Default::default()
                },
            )
            .unwrap();
            for time in [t(0, 1), t(1, 2), t(1, 1)] {
                let request = FrameRequest {
                    time,
                    region: region(),
                };
                let cpu = render_frame(&s, &[], &CpuReferenceBackend, request).unwrap();
                let actual = render_frame(&s, &[], &gpu, request).unwrap();
                compare(&cpu.pixels.linear, &actual.pixels.linear, 2.0_f32.powi(-10));
            }
        }
    }
}
#[test]
fn gradient_color_changes_invalidate_raster_and_geometry_changes_invalidate_both() {
    let (mut p, id) = gradient_project(false);
    let mut cache = RenderCache::default();
    let first = cached_frame(&snapshot(&p, id), t(0, 1), region(), &mut cache);
    cache.reset_stats();
    comp_mut(&mut p).nodes[0].properties[3]
        .set_source(
            PropertySource::Constant(Value::Color(Color::from_srgb8([0, 255, 0], None))),
            &render_registry(),
        )
        .unwrap();
    let second = cached_frame(&snapshot(&p, id), t(0, 1), region(), &mut cache);
    assert_ne!(first, second);
    assert_eq!(
        (cache.stats().geometry.hits, cache.stats().geometry.misses),
        (1, 0)
    );
    assert_eq!(
        (cache.stats().raster.hits, cache.stats().raster.misses),
        (0, 1)
    );
    cache.reset_stats();
    comp_mut(&mut p).nodes[0].properties[0]
        .set_source(PropertySource::Constant(v2(12.0, 16.0)), &render_registry())
        .unwrap();
    cached_frame(&snapshot(&p, id), t(0, 1), region(), &mut cache);
    assert_eq!(
        (cache.stats().geometry.hits, cache.stats().geometry.misses),
        (0, 1)
    );
    assert_eq!(
        (cache.stats().raster.hits, cache.stats().raster.misses),
        (0, 1)
    );
}
#[test]
fn invalid_gradient_stops_and_geometry_fail_with_typed_shape_errors() {
    let (p, id) = gradient_project(false);
    for invalid in [-0.1, 1.1] {
        let DocumentObject::Known(shape) = &p.shapes[0] else {
            panic!()
        };
        let DocumentObject::Known(c) = &p.compositions[0] else {
            panic!()
        };
        let mut values: BTreeMap<_, _> = c.nodes[0]
            .properties
            .iter()
            .map(|p| match p.source() {
                PropertySource::Constant(v) => (p.id(), v.clone()),
                _ => panic!(),
            })
            .collect();
        values.insert(c.nodes[0].properties[5].id(), scalar(invalid));
        assert!(matches!(
            shape.resolve(&values),
            Err(ShapeError::InvalidParameter { .. })
        ));
    }
    let mut bad = p.clone();
    comp_mut(&mut bad).nodes[0].properties[4]
        .set_source(PropertySource::Constant(scalar(1.0)), &render_registry())
        .unwrap();
    comp_mut(&mut bad).nodes[0].properties[5]
        .set_source(PropertySource::Constant(scalar(0.5)), &render_registry())
        .unwrap();
    assert!(matches!(
        build_scene_ir(&snapshot(&bad, id), t(0, 1), &[]),
        Err(RenderError::Shape(ShapeError::InvalidGradient))
    ));
    for geometry in [
        Gradient::Linear {
            options: Default::default(),
            start: [f(0.0); 2],
            end: [f(0.0); 2],
            stops: vec![],
        },
        Gradient::Radial {
            options: Default::default(),
            center: [f(0.0); 2],
            radius: f(0.0),
            stops: vec![],
        },
    ] {
        let mut bad = p.clone();
        let DocumentObject::Known(shape) = &mut bad.shapes[0] else {
            panic!()
        };
        shape.fill.as_mut().unwrap().gradient = Some(Box::new(geometry));
        assert!(matches!(
            build_scene_ir(&snapshot(&bad, id), t(0, 1), &[]),
            Err(RenderError::Shape(ShapeError::InvalidGradient))
        ));
    }
}
#[test]
fn malformed_gradient_and_vec005_payloads_roundtrip_but_fail_final_render() {
    let (mut p, id) = gradient_project(false);
    let width = constant("kronello.stroke_width", scalar(2.0));
    let join = constant("kronello.shape.stroke_join", Value::Enum("miter".into()));
    let cap = constant("kronello.shape.stroke_cap", Value::Enum("butt".into()));
    let limit = constant("kronello.shape.miter_limit", scalar(4.0));
    let DocumentObject::Known(shape) = &mut p.shapes[0] else {
        panic!()
    };
    shape.stroke = Some(Stroke {
        options: None,
        gradient: None,
        color: shape.fill.as_ref().unwrap().color,
        width: width.id(),
        join: join.id(),
        cap: cap.id(),
        miter_limit: limit.id(),
    });
    comp_mut(&mut p).nodes[0]
        .properties
        .extend([width, join, cap, limit]);
    let base = serde_json::to_value(&p).unwrap();
    for (target, field, value) in [
        ("gradient", "spread", serde_json::json!("repeat")),
        ("gradient", "spread", serde_json::json!("reflect")),
        ("gradient", "focal", serde_json::json!([1.0, 2.0])),
        ("gradient", "kind", serde_json::json!("conic")),
        ("gradient", "interpolation_space", serde_json::json!("srgb")),
        ("gradient", "units", serde_json::json!("bounding_box")),
        (
            "gradient",
            "transform",
            serde_json::json!([1, 0, 0, 1, 0, 0]),
        ),
        ("stroke", "dash", serde_json::json!([2, 3])),
        ("stroke", "alignment", serde_json::json!("inside")),
    ] {
        let mut value_json = base.clone();
        let shape = value_json["shapes"][0].as_object_mut().unwrap();
        if target == "gradient" {
            shape.get_mut("fill").unwrap()["gradient"][field] = value;
        } else {
            shape.get_mut("stroke").unwrap()[field] = value;
        }
        let imported: Project = serde_json::from_value(value_json.clone()).unwrap();
        assert_eq!(serde_json::to_value(&imported).unwrap(), value_json);
        assert!(matches!(imported.shapes[0], DocumentObject::Opaque(_)));
        let error = render_frame(
            &snapshot(&imported, id),
            &[],
            &CpuReferenceBackend,
            FrameRequest {
                time: t(0, 1),
                region: region(),
            },
        )
        .unwrap_err();
        assert_eq!(error.code(), "UNSUPPORTED_FEATURE");
    }
}
#[test]
fn gpu_gradient_dag_transform_and_animation_match_cpu_reference() {
    let gpu = kronello_gpu::GpuContext::new().expect("GPU required");
    let (mut p, id) = gradient_project(true);
    comp_mut(&mut p).nodes[0].properties.extend([
        constant("kronello.transform.position", v2(24.0, 8.0)),
        constant("kronello.transform.rotation", Value::Angle(f(25.0))),
        constant("kronello.transform.scale", v2(1.3, 1.3)),
    ]);
    for working in [ColorSpace::LinearRec709, ColorSpace::LinearRec2020] {
        let s = RenderSnapshot::new(
            &p,
            id,
            1,
            RenderProfile {
                working_space: working,
                ..RenderProfile::default()
            },
        )
        .unwrap();
        for time in [t(0, 1), t(1, 2), t(1, 1)] {
            let request = FrameRequest {
                time,
                region: region(),
            };
            let cpu = render_frame(&s, &[], &CpuReferenceBackend, request).unwrap();
            let actual = render_frame(&s, &[], &gpu, request).unwrap();
            compare(&cpu.pixels.linear, &actual.pixels.linear, 2.0_f32.powi(-10));
        }
    }
}

#[test]
fn malformed_text_gradient_is_preserved_as_opaque_and_snapshot_refuses_it() {
    let (p, id) = cache_project();
    let mut value = serde_json::to_value(&p).unwrap();
    value["texts"][0]["styles"][0]["gradient"] = serde_json::json!({"kind":"linear","stops":[{"offset":0,"color":"red"},{"offset":1,"color":"blue"}]});
    let imported: Project = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(&imported).unwrap(), value);
    assert!(matches!(imported.texts[0], DocumentObject::Opaque(_)));
    assert_eq!(
        RenderSnapshot::new(&imported, id, 1, RenderProfile::default())
            .unwrap_err()
            .code(),
        "UNSUPPORTED_FEATURE"
    );
}

fn fx_project() -> (Project, CompositionId) {
    let (mut n, shape) = rectangle(
        [4.0, 6.0],
        Color::new(ColorSpace::Srgb, [0.8, 0.2, 0.1], 0.65).unwrap(),
    );
    let sigma = constant("kronello.effect.sigma", scalar(1.0));
    let offset = constant("kronello.effect.offset", v2(2.25, -1.5));
    let color = constant(
        "kronello.effect.color",
        Value::Color(Color::new(ColorSpace::Srgb, [0.2, 0.5, 0.9], 0.7).unwrap()),
    );
    let opacity = constant("kronello.effect.opacity", scalar(0.6));
    n.effects.push(Effect::Known(EffectDefinition {
        effect_id: DROP_SHADOW_ID.into(),
        version: 1,
        parameters: EffectParameters::DropShadow {
            sigma: sigma.id(),
            offset: offset.id(),
            color: color.id(),
            opacity: opacity.id(),
        },
    }));
    n.properties.extend([
        sigma,
        offset,
        color,
        opacity,
        constant("kronello.transform.position", v2(6.0, 8.0)),
    ]);
    let c = composition(vec![n]);
    let id = c.id;
    (
        Project {
            compositions: vec![DocumentObject::Known(c)],
            shapes: vec![DocumentObject::Known(shape)],
            ..Project::default()
        },
        id,
    )
}
fn fx_region() -> OutputRegion {
    OutputRegion {
        origin: [0.0; 2],
        extent: [24.0; 2],
        pixels: [24; 2],
    }
}
fn fx_dag(p: &Project, id: CompositionId, region: OutputRegion) -> RenderDag {
    let snapshot = RenderSnapshot::new(p, id, 0, RenderProfile::default()).unwrap();
    let scene = build_scene_ir(&snapshot, t(0, 1), &[]).unwrap();
    build_render_dag(&scene, RenderProfile::default(), region).unwrap()
}

#[test]
fn inspect_path_reports_halos_budgets_and_preserves_the_live_render_cache() {
    let (p, id) = fx_project();
    let snapshot = snapshot(&p, id);
    let mut live = RenderCache::default();
    let request = FrameRequest {
        time: Time::ZERO,
        region: fx_region(),
    };
    let expected =
        render_frame_with_cache(&snapshot, &[], &CpuReferenceBackend, request, &mut live).unwrap();
    let before = live.stats();
    let mut isolated = RenderCache::default();
    let scene = build_scene_ir_with_cache(&snapshot, Time::ZERO, &[], &mut isolated).unwrap();
    let plan = explain_render_path(
        &scene,
        snapshot.profile(),
        fx_region(),
        ExplainBackend::Gpu,
        &mut isolated,
    )
    .unwrap();
    assert!(
        plan.notices
            .iter()
            .any(|n| n.code == "EFFECT_HALO_EXPANSION")
    );
    assert!(plan.tiles[0].stages.iter().any(|s| s.code == "EFFECT"));
    let dag = build_render_dag(&scene, snapshot.profile(), fx_region()).unwrap();
    assert_eq!(plan.tiles[0].execution, dag.execution_region());
    assert_eq!(plan.tiles[0].stages.len(), dag.nodes().len());
    for (stage, node) in plan.tiles[0].stages.iter().zip(dag.nodes()) {
        assert_eq!(stage.inputs, node.inputs());
    }
    assert_eq!(live.stats(), before);
    assert_eq!(
        render_frame_with_cache(&snapshot, &[], &CpuReferenceBackend, request, &mut live)
            .unwrap()
            .pixels,
        expected.pixels
    );
    assert_eq!(live.stats().raster.misses, before.raster.misses);
    let mut heavy = scene.clone();
    for _ in 0..180 {
        let mut n = heavy.nodes[0].clone();
        n.key.node = NodeId::new();
        heavy.nodes.push(n);
    }
    let large = OutputRegion {
        origin: [0.0; 2],
        extent: [512.0; 2],
        pixels: [512; 2],
    };
    let plan = explain_render_path(
        &heavy,
        snapshot.profile(),
        large,
        ExplainBackend::Gpu,
        &mut RenderCache::default(),
    )
    .unwrap();
    assert!(
        plan.notices
            .iter()
            .any(|n| n.code == "SURFACE_BUDGET_EXCEEDED"
                && n.actual_estimate.unwrap() > n.limit.unwrap())
    );
    assert!(!plan.executed);
}
#[test]
fn fx_halo_requests_and_transformed_visual_bounds_are_analytical() {
    let (p, id) = fx_project();
    let scene = build_scene_ir(&snapshot(&p, id), t(0, 1), &[]).unwrap();
    assert_eq!(
        scene.nodes[0].bounds.ink_bounds,
        Some(DesignBounds {
            min: [6.0, 8.0],
            max: [10.0, 14.0],
        })
    );
    assert_eq!(
        scene.nodes[0].bounds.visual_bounds,
        Some(DesignBounds {
            min: [5.25, 3.5],
            max: [15.25, 15.5],
        })
    );
    let dag = fx_dag(&p, id, fx_region());
    let effect_index = dag
        .nodes()
        .iter()
        .position(|n| matches!(n, DagNode::Effect { .. }))
        .unwrap();
    let b = dag.bounds()[effect_index];
    assert_eq!(
        b.ink_bounds,
        Some(PixelBounds {
            min: [6.0, 8.0],
            max: [10.0, 14.0]
        })
    );
    assert_eq!(
        b.visual_bounds,
        Some(PixelBounds {
            min: [5.0, 3.0],
            max: [16.0, 16.0]
        })
    );
    let DagNode::Effect { source, .. } = dag.nodes()[effect_index] else {
        unreachable!()
    };
    assert_eq!(
        dag.input_requests()[source],
        Some(PixelBounds {
            min: [-6.0, -2.0],
            max: [25.0, 29.0]
        })
    );
    assert_eq!(
        dag.execution_region(),
        OutputRegion {
            origin: [-6.0, -2.0],
            extent: [31.0, 31.0],
            pixels: [31; 2]
        }
    );
}

#[test]
fn group_bounds_union_children_and_apply_group_effect_after_child_effects() {
    let (mut p, id, group_id, _) = group_project();
    let sigma = constant("kronello.effect.sigma", scalar(2.0));
    let group = &mut comp_mut(&mut p).nodes[0];
    group.effects.push(Effect::Known(EffectDefinition {
        effect_id: GAUSSIAN_BLUR_ID.into(),
        version: 1,
        parameters: EffectParameters::GaussianBlur { sigma: sigma.id() },
    }));
    group.properties.push(sigma);
    let scene = build_scene_ir(&snapshot(&p, id), Time::ZERO, &[]).unwrap();
    let group = scene.nodes.iter().find(|n| n.key.node == group_id).unwrap();
    assert_eq!(
        group.bounds.layout_bounds,
        Some(DesignBounds {
            min: [0.0; 2],
            max: [16.0; 2]
        })
    );
    assert_eq!(group.bounds.ink_bounds, group.bounds.layout_bounds);
    assert_eq!(
        group.bounds.visual_bounds,
        Some(DesignBounds {
            min: [-6.0; 2],
            max: [22.0; 2]
        })
    );
}
fn assert_fx_crop(backend: &dyn RenderBackend) {
    let (mut p, id) = fx_project();
    // A second blur forces multiple backward halo expansions.
    let DocumentObject::Known(c) = &mut p.compositions[0] else {
        unreachable!()
    };
    let sigma = c.nodes[0]
        .properties
        .iter()
        .find(|p| p.descriptor().key.as_str() == "kronello.effect.sigma")
        .unwrap()
        .id();
    c.nodes[0].effects.push(Effect::Known(EffectDefinition {
        effect_id: GAUSSIAN_BLUR_ID.into(),
        version: 1,
        parameters: EffectParameters::GaussianBlur { sigma },
    }));
    for scale in [0.5, 1.0, 2.0] {
        let full = OutputRegion {
            pixels: [(24.0 * scale) as u32; 2],
            ..fx_region()
        };
        let crop = OutputRegion {
            origin: [4.0, 6.0],
            extent: [12.0, 10.0],
            pixels: [(12.0 * scale) as u32, (10.0 * scale) as u32],
        };
        let a = backend.execute(&fx_dag(&p, id, full)).unwrap();
        let b = backend.execute(&fx_dag(&p, id, crop)).unwrap();
        for y in 0..crop.pixels[1] as usize {
            for x in 0..crop.pixels[0] as usize {
                let src = a.linear[(y + (6.0 * scale) as usize) * full.pixels[0] as usize
                    + x
                    + (4.0 * scale) as usize];
                let dst = b.linear[y * crop.pixels[0] as usize + x];
                if backend.name() == "cpu_reference_float32" {
                    assert_eq!(src, dst);
                }
                for ch in 0..4 {
                    assert!(
                        (src[ch] - dst[ch]).abs() < 1.0 / 1024.0,
                        "{scale} {x} {y} {ch}: {src:?} {dst:?}"
                    );
                }
            }
        }
    }
}
#[test]
fn fx_cropped_roi_matches_full_render_with_stacked_effects_at_three_scales() {
    assert_fx_crop(&CpuReferenceBackend);
}

#[test]
fn tiled_frame_matches_full_frame_across_shadow_halo_and_partial_tiles() {
    let (mut project, id) = fx_project();
    let DocumentObject::Known(c) = &mut project.compositions[0] else {
        unreachable!()
    };
    let sigma = c.nodes[0]
        .properties
        .iter()
        .find(|p| p.descriptor().key.as_str() == "kronello.effect.sigma")
        .unwrap()
        .id();
    c.nodes[0].effects.push(Effect::Known(EffectDefinition {
        effect_id: GAUSSIAN_BLUR_ID.into(),
        version: 1,
        parameters: EffectParameters::GaussianBlur { sigma },
    }));
    for scale in [0.5, 1.0, 2.0] {
        // The shape and two effect halos straddle x=512; final tile is partial.
        let region = OutputRegion {
            origin: [6.0 - 510.0 / scale, 0.0],
            extent: [529.0 / scale, 35.0 / scale],
            pixels: [529, 35],
        };
        let snapshot = RenderSnapshot::new(&project, id, 7, RenderProfile::default()).unwrap();
        let expected = CpuReferenceBackend
            .execute(&fx_dag(&project, id, region))
            .unwrap();
        let actual = render_frame(
            &snapshot,
            &[],
            &CpuReferenceBackend,
            FrameRequest {
                time: t(0, 1),
                region,
            },
        )
        .unwrap();
        assert_eq!(actual.pixels, expected, "scale {scale}");
        let mut streamed = vec![[0.0; 4]; 529 * 35];
        render_frame_tiles(
            &snapshot,
            &[],
            &CpuReferenceBackend,
            FrameRequest {
                time: t(0, 1),
                region,
            },
            &mut |[x, y], tile, output| {
                for row in 0..tile.pixels[1] as usize {
                    let dest = (y as usize + row) * 529 + x as usize;
                    let source = row * tile.pixels[0] as usize;
                    let width = tile.pixels[0] as usize;
                    streamed[dest..dest + width]
                        .copy_from_slice(&output.linear[source..source + width]);
                }
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(streamed, expected.linear, "streamed halo scale {scale}");
        assert_eq!(actual.metadata.region, region);
        assert_eq!(actual.metadata.revision, "7");
        assert_eq!(actual.metadata.design_to_pixel, region.design_to_pixel().0);
        assert!(actual.pixels.linear.iter().any(|p| p[3] > 0.0));
    }
}
#[test]
fn gpu_fx_cropped_roi_matches_full_render_with_stacked_effects() {
    assert_fx_crop(&kronello_gpu::GpuContext::new().unwrap());
}
#[test]
fn fx_effect_animation_versions_and_cache_identity() {
    let (mut p, id) = fx_project();
    let curve = AnimationCurve::new(
        CurveId::new(),
        ValueType::Scalar,
        vec![
            Keyframe {
                time: t(0, 1),
                value: scalar(0.0),
                interpolation: CurveInterpolation::Linear,
            },
            Keyframe {
                time: t(1, 1),
                value: scalar(2.0),
                interpolation: CurveInterpolation::Linear,
            },
        ],
    )
    .unwrap();
    let DocumentObject::Known(c) = &mut p.compositions[0] else {
        unreachable!()
    };
    let prop = c.nodes[0]
        .properties
        .iter_mut()
        .find(|p| p.descriptor().key.as_str() == "kronello.effect.sigma")
        .unwrap();
    prop.set_source(PropertySource::Curve(curve.id()), &render_registry())
        .unwrap();
    p.curves.push(DocumentObject::Known(curve));
    let snapshot = RenderSnapshot::new(&p, id, 0, RenderProfile::default()).unwrap();
    assert_eq!(
        snapshot.semantic_versions().effects,
        BTreeMap::from([
            (GAUSSIAN_BLUR_ID.into(), 2),
            (DROP_SHADOW_ID.into(), 2),
            (kronello_model::COLOR_EXPOSURE_ID.into(), 1),
            (kronello_model::COLOR_LEVELS_ID.into(), 1),
            (kronello_model::COLOR_CURVES_ID.into(), 1),
            (kronello_model::COLOR_HSL_ID.into(), 1),
            (kronello_model::KEYING_CHROMA_ID.into(), 1),
            (kronello_model::KEYING_LUMA_ID.into(), 1),
            (kronello_model::GLOW_ID.into(), 1),
            (kronello_model::SHARPEN_ID.into(), 1),
            (kronello_model::VIGNETTE_ID.into(), 1),
            (kronello_model::CORNER_PIN_ID.into(), 1),
            (kronello_model::COLOR_LUT_ID.into(), 1),
        ])
    );
    let mut cache = RenderCache::new(CacheConfig::default());
    let mut identities = vec![];
    for (time, expected) in [(t(0, 1), 0.0), (t(1, 2), 1.0), (t(1, 1), 2.0)] {
        let scene = build_scene_ir(&snapshot, time, &[]).unwrap();
        let ResolvedEffect::DropShadow { sigma, .. } = scene.nodes[0].effects[0] else {
            unreachable!()
        };
        assert_eq!(sigma, expected);
        let dag = build_render_dag(&scene, RenderProfile::default(), fx_region()).unwrap();
        let keys = RasterCacheKey::for_dag(&dag, "cpu-reference-f32-v1").unwrap();
        let i = dag
            .nodes()
            .iter()
            .position(|n| matches!(n, DagNode::Effect { .. }))
            .unwrap();
        identities.push(keys[i]);
        let plain = CpuReferenceBackend.execute(&dag).unwrap();
        let cached = CpuReferenceBackend
            .execute_with_cache(&dag, &mut cache)
            .unwrap();
        assert_eq!(plain, cached);
        assert_eq!(
            cached,
            CpuReferenceBackend
                .execute_with_cache(&dag, &mut cache)
                .unwrap()
        );
    }
    assert!(identities.windows(2).all(|v| v[0] != v[1]));
    assert!(cache.stats().raster.hits >= 3);
    let mut wire = serde_json::to_value(snapshot).unwrap();
    wire["semantic_versions"]["effects"][GAUSSIAN_BLUR_ID] = serde_json::json!(99);
    let restored: RenderSnapshot = serde_json::from_value(wire).unwrap();
    assert_eq!(
        restored.validate().unwrap_err().code(),
        "UNSUPPORTED_FEATURE"
    );
}
#[test]
fn fx_unknown_effects_roundtrip_and_fail_final_render() {
    let (p, id) = fx_project();
    for unknown in [
        serde_json::json!({"effect_id":"vendor.future","version":1,"parameters":{"kind":"future","raw":[1,2]}}),
        serde_json::json!({"effect_id":GAUSSIAN_BLUR_ID,"version":99,"parameters":{"kind":"gaussian_blur","sigma":PropertyId::new()}}),
    ] {
        let mut wire = serde_json::to_value(&p).unwrap();
        wire["compositions"][0]["nodes"][0]["effects"] = serde_json::json!([unknown]);
        let restored: Project = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&restored).unwrap(), wire);
        assert!(restored.ensure_editable().is_err());
        let snapshot = RenderSnapshot::new(&restored, id, 0, RenderProfile::default()).unwrap();
        assert_eq!(
            build_scene_ir(&snapshot, t(0, 1), &[]).unwrap_err().code(),
            "UNSUPPORTED_FEATURE"
        );
    }
}

#[test]
fn fx_local_halos_offsets_follow_uniform_scale_rotation_and_reject_anisotropy() {
    let (mut p, id) = fx_project();
    let DocumentObject::Known(c) = &mut p.compositions[0] else {
        unreachable!()
    };
    c.nodes[0].properties.extend([
        constant("kronello.transform.scale", v2(2.0, 2.0)),
        constant("kronello.transform.rotation", Value::Angle(f(90.0))),
    ]);
    let dag = fx_dag(&p, id, fx_region());
    let i = dag
        .nodes()
        .iter()
        .position(|n| matches!(n, DagNode::Effect { .. }))
        .unwrap();
    let b = dag.bounds()[i].visual_bounds.unwrap();
    for (actual, expected) in b.min.into_iter().chain(b.max).zip([-9.0, 6.0, 15.0, 27.0]) {
        assert!((actual - expected).abs() < 1e-10);
    }
    let scene = build_scene_ir(&snapshot(&p, id), Time::ZERO, &[]).unwrap();
    let b = scene.nodes[0].bounds.visual_bounds.unwrap();
    for (actual, expected) in b.min.into_iter().chain(b.max).zip([-9.0, 6.5, 15.0, 26.5]) {
        assert!((actual - expected).abs() < 1e-10);
    }
    let DocumentObject::Known(c) = &mut p.compositions[0] else {
        unreachable!()
    };
    c.nodes[0]
        .properties
        .iter_mut()
        .find(|p| p.descriptor().key.as_str() == "kronello.transform.scale")
        .unwrap()
        .set_source(PropertySource::Constant(v2(2.0, 1.0)), &render_registry())
        .unwrap();
    let snapshot = RenderSnapshot::new(&p, id, 0, RenderProfile::default()).unwrap();
    assert_eq!(
        build_scene_ir(&snapshot, t(0, 1), &[]).unwrap_err().code(),
        "UNSUPPORTED_FEATURE"
    );
}

#[test]
fn fx_stack_order_is_semantic_and_group_isolation_is_retained() {
    let (mut p, id) = fx_project();
    let DocumentObject::Known(c) = &mut p.compositions[0] else {
        unreachable!()
    };
    let sigma = c.nodes[0]
        .properties
        .iter()
        .find(|p| p.descriptor().key.as_str() == "kronello.effect.sigma")
        .unwrap()
        .id();
    c.nodes[0].effects.push(Effect::Known(EffectDefinition {
        effect_id: GAUSSIAN_BLUR_ID.into(),
        version: 1,
        parameters: EffectParameters::GaussianBlur { sigma },
    }));
    let a = fx_dag(&p, id, fx_region());
    let DocumentObject::Known(c) = &mut p.compositions[0] else {
        unreachable!()
    };
    c.nodes[0].effects.reverse();
    let b = fx_dag(&p, id, fx_region());
    assert_ne!(
        CpuReferenceBackend.execute(&a).unwrap(),
        CpuReferenceBackend.execute(&b).unwrap()
    );
    assert_ne!(
        RasterCacheKey::for_dag(&a, "cpu").unwrap().last(),
        RasterCacheKey::for_dag(&b, "cpu").unwrap().last()
    );
    for dag in [&a, &b] {
        let effect_index = dag
            .nodes()
            .iter()
            .position(|n| matches!(n, DagNode::Effect { .. }))
            .unwrap();
        let DagNode::Effect { source, .. } = dag.nodes()[effect_index] else {
            unreachable!()
        };
        assert!(matches!(
            dag.nodes()[source],
            DagNode::IsolatedComposite { .. }
        ));
    }
}

fn fx002_project(case: usize) -> (Project, CompositionId) {
    let (mut p, id) = fx_project();
    let c = comp_mut(&mut p);
    for e in &mut c.nodes[0].effects {
        let Effect::Known(e) = e else { panic!() };
        e.version = 2;
    }
    let sigma = c.nodes[0]
        .properties
        .iter()
        .find(|p| p.descriptor().key.as_str() == "kronello.effect.sigma")
        .unwrap()
        .id();
    c.nodes[0].effects.push(Effect::Known(EffectDefinition {
        effect_id: GAUSSIAN_BLUR_ID.into(),
        version: 2,
        parameters: EffectParameters::GaussianBlur { sigma },
    }));
    let (scale, rotation) = match case {
        0 => ((1.0, 1.0), 37.0),
        1 => ((2.0, 0.75), 0.0),
        _ => ((1.5, 0.8), 28.0),
    };
    c.nodes[0].properties.extend([
        constant("kronello.transform.scale", v2(scale.0, scale.1)),
        constant("kronello.transform.rotation", Value::Angle(f(rotation))),
    ]);
    if case == 2 {
        // Nonuniform parent scale after child rotation produces actual shear.
        let parent = node(
            NodeKind::Null,
            vec![constant("kronello.transform.scale", v2(1.2, 0.7))],
        );
        c.nodes[0].transform_parent = Some(parent.id);
        c.root_nodes.push(parent.id);
        c.nodes.push(parent);
    }
    (p, id)
}
fn assert_fx002_crop_and_tiles(backend: &dyn RenderBackend) {
    for case in 0..3 {
        let (p, id) = fx002_project(case);
        for working in [ColorSpace::LinearRec709, ColorSpace::LinearRec2020] {
            let snapshot = RenderSnapshot::new(
                &p,
                id,
                7,
                RenderProfile {
                    working_space: working,
                    ..RenderProfile::default()
                },
            )
            .unwrap();
            let scene = build_scene_ir(&snapshot, Time::ZERO, &[]).unwrap();
            for scale in [0.5, 1.0, 2.0] {
                // Both transforms and stack halos cross x=512; last tile is partial.
                let full = OutputRegion {
                    origin: [6.0 - 510.0 / scale, -4.0],
                    extent: [529.0 / scale, 40.0 / scale],
                    pixels: [529, 40],
                };
                let dag = build_render_dag(&scene, snapshot.profile(), full).unwrap();
                let expected = backend.execute(&dag).unwrap();
                assert!(
                    expected.linear.iter().any(|p| p[3] > 0.01),
                    "empty test case {case}"
                );
                let actual = render_frame(
                    &snapshot,
                    &[],
                    backend,
                    FrameRequest {
                        time: Time::ZERO,
                        region: full,
                    },
                )
                .unwrap();
                compare(&expected.linear, &actual.pixels.linear, 1.0 / 1024.0);
                if backend.name() == "cpu_reference_float32" {
                    for (i, (a, b)) in actual
                        .pixels
                        .linear
                        .iter()
                        .zip(&expected.linear)
                        .enumerate()
                    {
                        assert_eq!(a, b, "tile case {case} {working:?} scale {scale} pixel {i}");
                    }
                    for (i, (a, b)) in actual
                        .pixels
                        .display
                        .iter()
                        .zip(&expected.display)
                        .enumerate()
                    {
                        assert_eq!(
                            a, b,
                            "display tile case {case} {working:?} scale {scale} pixel {i}"
                        );
                    }
                }
                let crop = OutputRegion {
                    origin: [full.origin[0] + 498.0 / scale, full.origin[1] + 4.0 / scale],
                    extent: [28.0 / scale, 30.0 / scale],
                    pixels: [28, 30],
                };
                let cropped = backend
                    .execute(&build_render_dag(&scene, snapshot.profile(), crop).unwrap())
                    .unwrap();
                let mut reference = vec![];
                for y in 0..30 {
                    reference.extend_from_slice(
                        &expected.linear[(y + 4) * 529 + 498..(y + 4) * 529 + 526],
                    );
                }
                compare(&reference, &cropped.linear, 1.0 / 1024.0);
                if backend.name() == "cpu_reference_float32" {
                    for (i, (a, b)) in reference.iter().zip(&cropped.linear).enumerate() {
                        assert_eq!(a, b, "crop case {case} {working:?} scale {scale} pixel {i}");
                    }
                }
                if backend.name() != "cpu_reference_float32" {
                    let cpu = CpuReferenceBackend.execute(&dag).unwrap();
                    compare(&cpu.linear, &expected.linear, 1.0 / 1024.0);
                }
            }
        }
    }
}
#[test]
fn fx002_rotation_nonuniform_shear_crop_and_tile_boundaries_match() {
    assert_fx002_crop_and_tiles(&CpuReferenceBackend);
}
#[test]
fn gpu_fx002_rotation_nonuniform_shear_crop_and_tile_boundaries_match_cpu() {
    assert_fx002_crop_and_tiles(&kronello_gpu::GpuContext::new().expect("GPU required"));
}
#[test]
fn fx002_legacy_snapshot_pin_and_effect_cache_identity() {
    let (legacy, id) = fx_project();
    let snapshot = snapshot(&legacy, id);
    let expected = CpuReferenceBackend
        .execute(&fx_dag(&legacy, id, fx_region()))
        .unwrap();
    let mut wire = serde_json::to_value(&snapshot).unwrap();
    for name in [GAUSSIAN_BLUR_ID, DROP_SHADOW_ID] {
        wire["semantic_versions"]["effects"][name] = serde_json::json!(1);
    }
    let restored: RenderSnapshot = serde_json::from_value(wire.clone()).unwrap();
    restored.validate().unwrap();
    assert_eq!(
        render_frame(
            &restored,
            &[],
            &CpuReferenceBackend,
            FrameRequest {
                time: Time::ZERO,
                region: fx_region()
            }
        )
        .unwrap()
        .pixels,
        expected
    );
    wire["project"]["compositions"][0]["nodes"][0]["effects"][0]["version"] = serde_json::json!(2);
    let restored: RenderSnapshot = serde_json::from_value(wire).unwrap();
    assert_eq!(
        build_scene_ir(&restored, Time::ZERO, &[])
            .unwrap_err()
            .code(),
        "UNSUPPORTED_FEATURE"
    );
    let mut upgraded = legacy.clone();
    let Effect::Known(effect) = &mut comp_mut(&mut upgraded).nodes[0].effects[0] else {
        panic!()
    };
    effect.version = 2;
    let old = fx_dag(&legacy, id, fx_region());
    let new = fx_dag(&upgraded, id, fx_region());
    assert_ne!(
        RasterCacheKey::for_dag(&old, "cpu").unwrap(),
        RasterCacheKey::for_dag(&new, "cpu").unwrap()
    );
    assert_ne!(expected, CpuReferenceBackend.execute(&new).unwrap());
}

fn vec005_project(alignment: StrokeAlignment, animated: bool) -> (Project, CompositionId) {
    let (mut n, mut shape) = rectangle([12.3, 9.7], Color::from_srgb8([255, 0, 0], None));
    shape.fill = None;
    let color = constant(
        "kronello.shape.stroke_color",
        Value::Color(Color::from_srgb8([40, 180, 250], None)),
    );
    let width = constant("kronello.stroke_width", scalar(1.7));
    let join = constant("kronello.shape.stroke_join", Value::Enum("round".into()));
    let cap = constant("kronello.shape.stroke_cap", Value::Enum("round".into()));
    let limit = constant("kronello.shape.miter_limit", scalar(4.0));
    let curve = AnimationCurve::new(
        CurveId::new(),
        ValueType::Scalar,
        vec![
            Keyframe {
                time: t(0, 1),
                value: scalar(0.37),
                interpolation: CurveInterpolation::Linear,
            },
            Keyframe {
                time: t(1, 1),
                value: scalar(3.37),
                interpolation: CurveInterpolation::Linear,
            },
        ],
    )
    .unwrap();
    let offset = if animated {
        prop(
            "kronello.shape.dash_offset",
            PropertySource::Curve(curve.id()),
        )
    } else {
        constant("kronello.shape.dash_offset", scalar(0.37))
    };
    shape.stroke = Some(Stroke {
        options: Some(Box::new(StrokeOptions {
            geometry_version: EXTENDED_STROKE_VERSION.into(),
            alignment,
            fill_rule: FillRule::Evenodd,
            dash_array: vec![f(3.17), f(1.31), f(2.23)],
            dash_offset: offset.id(),
        })),
        gradient: None,
        color: color.id(),
        width: width.id(),
        join: join.id(),
        cap: cap.id(),
        miter_limit: limit.id(),
    });
    n.properties.extend([
        color,
        width,
        join,
        cap,
        limit,
        offset,
        constant("kronello.transform.position", v2(10.031, 7.019)),
        constant("kronello.transform.scale", v2(1.3, 0.8)),
        constant("kronello.transform.skew", Value::Angle(f(17.0))),
    ]);
    let c = composition(vec![n]);
    let id = c.id;
    (
        Project {
            compositions: vec![DocumentObject::Known(c)],
            shapes: vec![DocumentObject::Known(shape)],
            curves: if animated {
                vec![DocumentObject::Known(curve)]
            } else {
                vec![]
            },
            ..Default::default()
        },
        id,
    )
}

#[test]
fn vec005_offset_animation_instances_and_raster_identity() {
    let (mut p, id) = vec005_project(StrokeAlignment::Center, true);
    let root = composition(
        [0, 1]
            .map(|i| {
                node(
                    NodeKind::CompositionInstance(CompositionInstance {
                        id: CompositionInstanceId::new(),
                        definition_ref: id,
                        input_bindings: Default::default(),
                        local_time_map: TimeMap::linear(t(i, 2), t(1, 1)).unwrap(),
                        seed: 42,
                    }),
                    vec![],
                )
            })
            .to_vec(),
    );
    let root_id = root.id;
    p.compositions.push(DocumentObject::Known(root));
    let scene = build_scene_ir(&snapshot(&p, root_id), Time::ZERO, &[]).unwrap();
    let offsets: Vec<_> = scene
        .nodes
        .iter()
        .filter_map(|n| match &n.content {
            SceneContent::Shape { resolved, .. } => Some((
                n.key.instance_path.clone(),
                resolved
                    .stroke
                    .as_ref()
                    .unwrap()
                    .options
                    .as_ref()
                    .unwrap()
                    .dash_offset,
            )),
            _ => None,
        })
        .collect();
    assert_eq!(offsets.len(), 2);
    assert_ne!(offsets[0].0, offsets[1].0);
    assert_eq!((offsets[0].1, offsets[1].1), (0.37, 1.87));
    let s = snapshot(&p, id);
    let mut cache = RenderCache::default();
    let before = cached_frame(&s, t(0, 1), region(), &mut cache);
    cache.reset_stats();
    let after = cached_frame(&s, t(1, 2), region(), &mut cache);
    assert_ne!(before.pixels.linear, after.pixels.linear);
    assert!(cache.stats().geometry.hits > 0);
    assert!(cache.stats().raster.misses > 0);
    let again = cached_frame(&s, t(0, 1), region(), &mut cache);
    assert_eq!(before.pixels, again.pixels);
}

fn assert_vec005_affine_crop(backend: &dyn RenderBackend) {
    for alignment in [
        StrokeAlignment::Center,
        StrokeAlignment::Inside,
        StrokeAlignment::Outside,
    ] {
        let (p, id) = vec005_project(alignment, true);
        for working_space in [ColorSpace::LinearRec709, ColorSpace::LinearRec2020] {
            let s = RenderSnapshot::new(
                &p,
                id,
                0,
                RenderProfile {
                    working_space,
                    ..Default::default()
                },
            )
            .unwrap();
            for time in [t(0, 1), t(1, 2), t(1, 1)] {
                let ir = build_scene_ir(&s, time, &[]).unwrap();
                let full_dag = build_render_dag(&ir, s.profile(), region()).unwrap();
                let full = backend.execute(&full_dag).unwrap();
                let expected = CpuReferenceBackend.execute(&full_dag).unwrap();
                for (a, b) in full.linear.iter().zip(&expected.linear) {
                    for i in 0..4 {
                        assert!((a[i] - b[i]).abs() <= (1.0 / 1024.0) * b[i].abs().max(1.0));
                    }
                }
                let crop = OutputRegion {
                    origin: [8.0, 5.0],
                    extent: [32.0, 20.0],
                    pixels: [32, 20],
                };
                let actual = backend
                    .execute(&build_render_dag(&ir, s.profile(), crop).unwrap())
                    .unwrap();
                for y in 0..20 {
                    for x in 0..32 {
                        assert_eq!(actual.linear[y * 32 + x], full.linear[(y + 5) * 64 + x + 8]);
                    }
                }
                let bounds = ir.nodes[0].bounds;
                let visual = bounds.visual_bounds.unwrap();
                assert_eq!(bounds.ink_bounds, bounds.visual_bounds);
                for y in 0..32 {
                    for x in 0..64 {
                        if full.linear[y * 64 + x][3] > 0.0 {
                            assert!(
                                (x as f64) + 1.0 >= visual.min[0] && (x as f64) <= visual.max[0]
                            );
                            assert!(
                                (y as f64) + 1.0 >= visual.min[1] && (y as f64) <= visual.max[1]
                            );
                        }
                    }
                }
            }
        }
    }
}
#[test]
fn vec005_nonuniform_skew_bounds_and_roi_are_conservative() {
    assert_vec005_affine_crop(&CpuReferenceBackend);
}
#[test]
fn gpu_vec005_affine_animation_alignment_and_roi_match_cpu() {
    assert_vec005_affine_crop(&kronello_gpu::GpuContext::new().unwrap());
}

#[test]
fn vec005_legacy_snapshot_pixels_and_unknown_versions() {
    let (mut p, id) = vec005_project(StrokeAlignment::Center, false);
    let DocumentObject::Known(shape) = &mut p.shapes[0] else {
        panic!()
    };
    shape.stroke.as_mut().unwrap().options = None;
    comp_mut(&mut p).nodes[0].properties.retain(|p| {
        !matches!(
            p.descriptor().key.as_str(),
            "kronello.transform.scale" | "kronello.transform.skew"
        )
    });
    let current = snapshot(&p, id);
    let mut wire = serde_json::to_value(&current).unwrap();
    wire["semantic_versions"]["stroke_geometry"] = serde_json::json!(LEGACY_STROKE_VERSION);
    let old = vec005_restore(&wire).unwrap();
    assert_eq!(
        frame(&current, Time::ZERO).pixels,
        frame(&old, Time::ZERO).pixels
    );
    wire["semantic_versions"]["stroke_geometry"] = serde_json::json!("future");
    assert_eq!(
        vec005_restore(&wire).unwrap_err().code(),
        "UNSUPPORTED_FEATURE"
    );
    let (p, id) = vec005_project(StrokeAlignment::Center, false);
    let mut wire = serde_json::to_value(snapshot(&p, id)).unwrap();
    wire["semantic_versions"]["stroke_geometry"] = serde_json::json!(LEGACY_STROKE_VERSION);
    let old = vec005_restore(&wire).unwrap();
    assert_eq!(
        build_scene_ir(&old, Time::ZERO, &[]).unwrap_err().code(),
        "UNSUPPORTED_FEATURE"
    );
}

#[test]
fn vec005_invalid_dash_open_alignment_budget_and_version_are_typed() {
    for (kind, code) in [
        (0, "STROKE_INVALID_DASH"),
        (1, "STROKE_OPEN_ALIGNMENT"),
        (2, "STROKE_BUDGET_EXCEEDED"),
        (3, "UNSUPPORTED_FEATURE"),
    ] {
        let (mut p, id) = vec005_project(StrokeAlignment::Inside, false);
        let DocumentObject::Known(shape) = &mut p.shapes[0] else {
            panic!()
        };
        let options = shape.stroke.as_mut().unwrap().options.as_mut().unwrap();
        match kind {
            0 => options.dash_array = vec![f(-1.0), f(2.0)],
            1 => {
                let path = constant(
                    "kronello.shape.path",
                    Value::Path(Path {
                        segments: vec![
                            PathSegment::MoveTo([f(0.0); 2]),
                            PathSegment::LineTo([f(10.0), f(0.0)]),
                        ],
                    }),
                );
                shape.geometry = ShapeGeometry::BezierPath { path: path.id() };
                comp_mut(&mut p).nodes[0].properties.push(path);
            }
            2 => options.dash_array = vec![f(1e-9), f(1e-9)],
            _ => options.geometry_version = "future".into(),
        }
        let result = build_scene_ir(&snapshot(&p, id), Time::ZERO, &[])
            .and_then(|ir| build_render_dag(&ir, RenderProfile::default(), region()));
        assert_eq!(result.unwrap_err().code(), code);
    }
}

fn vec005_restore(wire: &serde_json::Value) -> Result<RenderSnapshot, RenderError> {
    let snapshot: RenderSnapshot = serde_json::from_value(wire.clone()).unwrap();
    snapshot.validate()?;
    Ok(snapshot)
}

#[test]
fn streaming_rejects_cumulative_halo_before_backend_allocation() {
    let (mut project, id) = fx_project();
    let c = comp_mut(&mut project);
    let sigma = c.nodes[0]
        .properties
        .iter()
        .find(|p| p.descriptor().key.as_str() == "kronello.effect.sigma")
        .unwrap()
        .id();
    c.nodes[0]
        .properties
        .iter_mut()
        .find(|p| p.id() == sigma)
        .unwrap()
        .set_source(PropertySource::Constant(scalar(20.0)), &render_registry())
        .unwrap();
    c.nodes[0].effects = (0..16)
        .map(|_| {
            Effect::Known(EffectDefinition {
                effect_id: GAUSSIAN_BLUR_ID.into(),
                version: 1,
                parameters: EffectParameters::GaussianBlur { sigma },
            })
        })
        .collect();
    let snap = RenderSnapshot::new(&project, id, 0, RenderProfile::default()).unwrap();
    let mut tiles = 0;
    let region = OutputRegion {
        origin: [0.0; 2],
        extent: [512.0; 2],
        pixels: [512; 2],
    };
    let error = render_frame_tiles(
        &snap,
        &[],
        &CpuReferenceBackend,
        FrameRequest {
            time: t(0, 1),
            region,
        },
        &mut |_, _, _| {
            tiles += 1;
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(error.code(), "UNSUPPORTED_FEATURE", "{error:?}");
    assert!(error.to_string().contains("streaming tile surface budget"));
    assert_eq!(tiles, 0);
}

fn temporal_settings() -> TemporalSettings {
    TemporalSettings {
        frame_rate: FrameRate::new(24, 1).unwrap(),
        shutter_angle: t(180, 1),
        shutter_phase: t(-1, 4),
        samples: 8,
        cut_policy: CutPolicy::AvoidCrossing,
    }
}
#[test]
fn temporal_whole_composition_matches_independent_subtime_baseline_and_tiles() {
    let (p, c) = project();
    let s = snapshot(&p, c);
    let request = FrameRequest {
        time: t(1, 2),
        region: region(),
    };
    let result = render_temporal_frame(
        &s,
        &fonts(),
        &CpuReferenceBackend,
        request,
        temporal_settings(),
    )
    .unwrap();
    let mut expected = vec![[0.0_f64; 4]; 64 * 32];
    for sample in &result.temporal.samples {
        let reference = frame(&s, sample.time);
        let weight = sample.weight.numerator() as f64 / sample.weight.denominator() as f64;
        for (sum, p) in expected.iter_mut().zip(reference.pixels.linear) {
            for i in 0..4 {
                sum[i] += f64::from(p[i]) * weight;
            }
        }
    }
    assert_eq!(
        result.frame.pixels.linear,
        expected
            .into_iter()
            .map(|p| p.map(|v| v as f32))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        result.frame.pixels.display,
        CpuReferenceBackend
            .display_from_linear(&result.frame.pixels.linear, s.profile().working_space)
            .unwrap()
    );
    let mut tiles = vec![];
    let metadata = render_temporal_frame_tiles(
        &s,
        &fonts(),
        &CpuReferenceBackend,
        request,
        temporal_settings(),
        &mut |_, _, frame| {
            tiles.push(frame);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(tiles, vec![result.frame.pixels]);
    assert_eq!(metadata, result.temporal);
    assert_eq!(result.frame.metadata.time, request.time);
}
#[test]
fn temporal_phase_is_rational_and_zero_exposure_collapses_duplicate_samples() {
    let (p, c) = project();
    let s = snapshot(&p, c);
    let settings = temporal_settings();
    let samples = temporal_samples(&s, t(1, 2), settings).unwrap();
    assert_eq!(samples[0].time, t(377, 768));
    assert_eq!(samples.last().unwrap().time, t(391, 768));
    let zero = temporal_samples(
        &s,
        t(1, 2),
        TemporalSettings {
            shutter_angle: Time::ZERO,
            ..settings
        },
    )
    .unwrap();
    assert_eq!(
        zero,
        vec![TemporalSample {
            time: t(1, 2),
            weight: Time::ONE
        }]
    );
    assert!(
        temporal_samples(
            &s,
            Time::ZERO,
            TemporalSettings {
                samples: 4097,
                ..settings
            }
        )
        .is_err()
    );
}
#[test]
fn temporal_sequence_exports_exact_plan_and_rejects_rate_mismatch_before_publication() {
    let (p, c) = project();
    let s = snapshot(&p, c);
    let directory = tempfile::tempdir().unwrap();
    let request = SequenceRequest {
        range: TimeRange::new(Time::ZERO, t(1, 24)).unwrap(),
        frame_rate: FrameRate::new(24, 1).unwrap(),
        region: region(),
    };
    let sequence = render_temporal_sequence_with_checkpoint(
        &s,
        &fonts(),
        &CpuReferenceBackend,
        request,
        temporal_settings(),
        directory.path().join("temporal"),
        &mut |_| Ok(()),
    )
    .unwrap();
    assert!(sequence.frames[0].metadata.temporal.is_some());
    let out = directory.path().join("bad-rate");
    assert!(
        render_temporal_sequence_with_checkpoint(
            &s,
            &fonts(),
            &CpuReferenceBackend,
            request,
            TemporalSettings {
                frame_rate: FrameRate::new(30, 1).unwrap(),
                ..temporal_settings()
            },
            &out,
            &mut |_| Ok(())
        )
        .is_err()
    );
    assert!(!out.exists());
}

fn temporal_cut_sequence() -> (Project, SequenceId) {
    let id = SequenceId::new();
    let clips = [(0, 1, [255, 0, 0]), (1, 2, [0, 0, 255])].map(|(start, end, rgb)| Clip {
        id: ClipId::new(),
        source_ref: SourceRef::Generator {
            generator: SOLID_GENERATOR_ID.into(),
            version: 1,
            color: Color::from_srgb8(rgb, None),
        },
        timeline_range: TimeRange::new(t(start, 1), t(end, 1)).unwrap(),
        source_in: Time::ZERO,
        time_map: TimeMap::linear(Time::ZERO, Time::ONE).unwrap(),
        enabled: true,
        audio_retime: Default::default(),
        reverse_sampling: None,
        volume: None,
        links: vec![],
        effects: vec![],
        masks: vec![],
        properties: vec![],
        markers: vec![],
    });
    (
        Project {
            sequences: vec![DocumentObject::Known(Sequence {
                id,
                extent: DesignExtent::new(2.0, 2.0).unwrap(),
                frame_rate: FrameRate::new(24, 1).unwrap(),
                audio_rate: kronello_time::SampleRate::HZ_48000,
                working_space: ColorSpace::LinearRec709,
                tracks: vec![Track {
                    state: None,
                    id: TrackId::new(),
                    kind: TrackKind::Video,
                    clips: clips.to_vec(),
                }],
                transitions: vec![],
                markers: vec![],
                work_area: None,
                targets: None,
            })],
            ..Project::default()
        },
        id,
    )
}
#[test]
fn temporal_cut_policy_clips_to_incoming_half_open_interval() {
    let (p, id) = temporal_cut_sequence();
    let s = RenderSnapshot::for_target(
        &p,
        RenderTarget::Sequence { sequence: id },
        7,
        RenderProfile::default(),
    )
    .unwrap();
    let settings = TemporalSettings {
        frame_rate: FrameRate::new(1, 1).unwrap(),
        shutter_angle: t(360, 1),
        shutter_phase: t(-1, 2),
        samples: 8,
        cut_policy: CutPolicy::AvoidCrossing,
    };
    let clipped = temporal_samples(&s, Time::ONE, settings).unwrap();
    assert!(
        clipped
            .iter()
            .all(|s| s.time >= Time::ONE && s.time < t(2, 1))
    );
    assert_eq!(clipped[0].time, t(33, 32));
    let crossing = temporal_samples(
        &s,
        Time::ONE,
        TemporalSettings {
            cut_policy: CutPolicy::AllowCrossing,
            ..settings
        },
    )
    .unwrap();
    assert!(crossing.iter().any(|s| s.time < Time::ONE));
    let r = FrameRequest {
        time: Time::ONE,
        region: OutputRegion {
            origin: [0.0; 2],
            extent: [2.0; 2],
            pixels: [2; 2],
        },
    };
    let frame = render_temporal_frame(&s, &[], &CpuReferenceBackend, r, settings).unwrap();
    assert_eq!(frame.frame.pixels.linear[0], [0.0, 0.0, 1.0, 1.0]);
    let mixed = render_temporal_frame(
        &s,
        &[],
        &CpuReferenceBackend,
        r,
        TemporalSettings {
            cut_policy: CutPolicy::AllowCrossing,
            ..settings
        },
    )
    .unwrap();
    assert_eq!(mixed.frame.pixels.linear[0], [0.5, 0.0, 0.5, 1.0]);
}
#[test]
fn temporal_negative_ntsc_exposure_and_nested_scope_are_exact() {
    let (p, id) = temporal_cut_sequence();
    let s = RenderSnapshot::for_target(
        &p,
        RenderTarget::Sequence { sequence: id },
        7,
        RenderProfile::default(),
    )
    .unwrap();
    let settings = TemporalSettings {
        frame_rate: FrameRate::new(30000, 1001).unwrap(),
        shutter_angle: t(360, 1),
        shutter_phase: t(-1, 2),
        samples: 2,
        cut_policy: CutPolicy::AllowCrossing,
    };
    let times = temporal_samples(&s, t(-1, 1), settings).unwrap();
    assert_eq!(times[0].time, t(-121001, 120000));
    assert_eq!(times[1].time, t(-118999, 120000));
    let (child, shape) = rectangle([2.0; 2], Color::from_srgb8([255, 0, 0], None));
    let target = composition(vec![child]);
    let root = composition(vec![node(
        NodeKind::CompositionInstance(CompositionInstance {
            id: CompositionInstanceId::new(),
            definition_ref: target.id,
            input_bindings: BTreeMap::new(),
            local_time_map: TimeMap::linear(Time::ZERO, t(2, 1)).unwrap(),
            seed: 0,
        }),
        vec![],
    )]);
    let id = root.id;
    let p = Project {
        compositions: vec![DocumentObject::Known(root), DocumentObject::Known(target)],
        shapes: vec![DocumentObject::Known(shape)],
        ..Project::default()
    };
    struct Counting(std::cell::Cell<usize>);
    impl RenderBackend for Counting {
        fn name(&self) -> &str {
            "counting"
        }
        fn execute(&self, dag: &RenderDag) -> Result<BackendFrame, RenderError> {
            self.0.set(self.0.get() + 1);
            CpuReferenceBackend.execute(dag)
        }
        fn display_from_linear(
            &self,
            p: &[[f32; 4]],
            w: ColorSpace,
        ) -> Result<Vec<[f32; 4]>, RenderError> {
            CpuReferenceBackend.display_from_linear(p, w)
        }
    }
    let backend = Counting(std::cell::Cell::new(0));
    let frame = render_temporal_frame(
        &snapshot(&p, id),
        &[],
        &backend,
        FrameRequest {
            time: t(1, 2),
            region: OutputRegion {
                origin: [0.0; 2],
                extent: [4.0; 2],
                pixels: [4; 2],
            },
        },
        settings,
    )
    .unwrap();
    assert_eq!(backend.0.get(), 2);
    assert_eq!(frame.temporal.samples.len(), 2);
}

#[test]
fn temporal_crossfade_endpoints_are_not_hard_cuts() {
    let (mut p, id) = temporal_cut_sequence();
    let DocumentObject::Known(sequence) = &mut p.sequences[0] else {
        panic!()
    };
    sequence.tracks[0].clips[0].timeline_range = TimeRange::new(Time::ZERO, t(3, 2)).unwrap();
    let outgoing = sequence.tracks[0].clips[0].id;
    let incoming = sequence.tracks[0].clips[1].id;
    sequence.transitions.push(Transition {
        outgoing,
        incoming,
        range: TimeRange::new(Time::ONE, t(3, 2)).unwrap(),
        kind: TransitionKind::Crossfade,
        params: None,
        version: 1,
    });
    let s = RenderSnapshot::for_target(
        &p,
        RenderTarget::Sequence { sequence: id },
        7,
        RenderProfile::default(),
    )
    .unwrap();
    let settings = TemporalSettings {
        frame_rate: FrameRate::new(1, 1).unwrap(),
        shutter_angle: t(360, 1),
        shutter_phase: t(-1, 2),
        samples: 8,
        cut_policy: CutPolicy::AvoidCrossing,
    };
    let at_start = temporal_samples(&s, Time::ONE, settings).unwrap();
    assert!(at_start.iter().any(|s| s.time < Time::ONE));
    let at_end = temporal_samples(&s, t(3, 2), settings).unwrap();
    assert!(at_end.iter().any(|s| s.time > t(3, 2)));
}
#[test]
fn temporal_cache_region_shutter_and_nested_time_map_invalidate_and_remain_bounded() {
    let (child, shape) = rectangle([2.0; 2], Color::from_srgb8([255, 0, 0], None));
    let target = composition(vec![child]);
    let root = composition(vec![node(
        NodeKind::CompositionInstance(CompositionInstance {
            id: CompositionInstanceId::new(),
            definition_ref: target.id,
            input_bindings: BTreeMap::new(),
            local_time_map: TimeMap::linear(Time::ZERO, Time::ONE).unwrap(),
            seed: 0,
        }),
        vec![],
    )]);
    let id = root.id;
    let mut p = Project {
        compositions: vec![DocumentObject::Known(root), DocumentObject::Known(target)],
        shapes: vec![DocumentObject::Known(shape)],
        ..Project::default()
    };
    let s = snapshot(&p, id);
    let request = FrameRequest {
        time: t(1, 2),
        region: OutputRegion {
            origin: [0.0; 2],
            extent: [4.0; 2],
            pixels: [4; 2],
        },
    };
    let settings = TemporalSettings {
        samples: 2,
        ..temporal_settings()
    };
    let mut cache = RenderCache::default();
    let first = render_temporal_frame_with_cache(
        &s,
        &[],
        &CpuReferenceBackend,
        request,
        settings,
        &mut cache,
    )
    .unwrap();
    let warm = render_temporal_frame_with_cache(
        &s,
        &[],
        &CpuReferenceBackend,
        request,
        settings,
        &mut cache,
    )
    .unwrap();
    assert_eq!(first, warm);
    assert_eq!(cache.stats().temporal.hits, 1);
    render_temporal_frame_with_cache(
        &s,
        &[],
        &CpuReferenceBackend,
        FrameRequest {
            region: OutputRegion {
                origin: [1.0, 0.0],
                ..request.region
            },
            ..request
        },
        settings,
        &mut cache,
    )
    .unwrap();
    assert_eq!(cache.stats().temporal.misses, 2);
    render_temporal_frame_with_cache(
        &s,
        &[],
        &CpuReferenceBackend,
        request,
        TemporalSettings {
            shutter_phase: Time::ZERO,
            ..settings
        },
        &mut cache,
    )
    .unwrap();
    assert_eq!(cache.stats().temporal.misses, 3);
    let DocumentObject::Known(root) = &mut p.compositions[0] else {
        panic!()
    };
    let NodeKind::CompositionInstance(instance) = &mut root.nodes[0].kind else {
        panic!()
    };
    instance.local_time_map = TimeMap::linear(t(4, 1), Time::ONE).unwrap();
    let changed = snapshot(&p, id);
    let cached = render_temporal_frame_with_cache(
        &changed,
        &[],
        &CpuReferenceBackend,
        request,
        settings,
        &mut cache,
    )
    .unwrap();
    let direct =
        render_temporal_frame(&changed, &[], &CpuReferenceBackend, request, settings).unwrap();
    assert_eq!(cached, direct);
    assert_ne!(first.frame.pixels, cached.frame.pixels);
    assert_eq!(cache.stats().temporal.misses, 4);
    let tiny = CacheCapacity {
        entries: 1,
        bytes: 512,
    };
    let mut bounded = RenderCache::new(CacheConfig {
        temporal: tiny,
        simulation: tiny,
        ..CacheConfig::disabled()
    });
    for time in [Time::ZERO, t(1, 2), Time::ONE] {
        render_temporal_frame_with_cache(
            &s,
            &[],
            &CpuReferenceBackend,
            FrameRequest { time, ..request },
            settings,
            &mut bounded,
        )
        .unwrap();
    }
    assert_eq!(bounded.stats().temporal.entries, 1);
    assert_eq!(bounded.stats().temporal.evictions, 2);
    assert!(bounded.stats().temporal.bytes <= 512);
    bounded.clear();
    assert_eq!(bounded.stats().temporal.entries, 0);
}

#[test]
fn temporal_gpu_matches_whole_composition_cpu_and_warm_cache() {
    let gpu = kronello_gpu::GpuContext::new().expect("GPU required; no fallback");
    let (p, id) = temporal_cut_sequence();
    let s = RenderSnapshot::for_target(
        &p,
        RenderTarget::Sequence { sequence: id },
        7,
        RenderProfile::default(),
    )
    .unwrap();
    let settings = TemporalSettings {
        frame_rate: FrameRate::new(1, 1).unwrap(),
        shutter_angle: t(360, 1),
        shutter_phase: t(-1, 2),
        samples: 4,
        cut_policy: CutPolicy::AllowCrossing,
    };
    let r = FrameRequest {
        time: Time::ONE,
        region: OutputRegion {
            origin: [0.0; 2],
            extent: [2.0; 2],
            pixels: [2; 2],
        },
    };
    let cpu = render_temporal_frame(&s, &[], &CpuReferenceBackend, r, settings).unwrap();
    let mut cache = RenderCache::default();
    let cold = render_temporal_frame_with_cache(&s, &[], &gpu, r, settings, &mut cache).unwrap();
    compare(
        &cpu.frame.pixels.linear,
        &cold.frame.pixels.linear,
        1.0 / 1024.0,
    );
    compare(
        &cpu.frame.pixels.display,
        &cold.frame.pixels.display,
        1.0 / 1024.0,
    );
    let warm = render_temporal_frame_with_cache(&s, &[], &gpu, r, settings, &mut cache).unwrap();
    assert_eq!(cold.frame.pixels, warm.frame.pixels);
    assert_eq!(cache.stats().temporal.hits, 1);
}

#[test]
fn temporal_warm_cache_cannot_hide_missing_fonts_or_changed_effect_halo() {
    let (p, id) = project();
    let s = snapshot(&p, id);
    let r = FrameRequest {
        time: t(1, 2),
        region: OutputRegion {
            pixels: [4, 2],
            ..region()
        },
    };
    let settings = TemporalSettings {
        samples: 2,
        ..temporal_settings()
    };
    let mut cache = RenderCache::default();
    render_temporal_frame_with_cache(&s, &fonts(), &CpuReferenceBackend, r, settings, &mut cache)
        .unwrap();
    assert_eq!(
        render_temporal_frame_with_cache(&s, &[], &CpuReferenceBackend, r, settings, &mut cache)
            .unwrap_err()
            .code(),
        "ASSET_MISSING"
    );
    let (child, shape) = rectangle([2.0; 2], Color::from_srgb8([255, 0, 0], None));
    let c = composition(vec![child]);
    let id = c.id;
    let mut p = Project {
        compositions: vec![DocumentObject::Known(c)],
        shapes: vec![DocumentObject::Known(shape)],
        ..Project::default()
    };
    let r = FrameRequest {
        time: t(1, 2),
        region: OutputRegion {
            origin: [0.0; 2],
            extent: [4.0; 2],
            pixels: [4; 2],
        },
    };
    let mut cache = RenderCache::default();
    let unblurred = render_temporal_frame_with_cache(
        &snapshot(&p, id),
        &[],
        &CpuReferenceBackend,
        r,
        settings,
        &mut cache,
    )
    .unwrap();
    let sigma = constant("kronello.effect.sigma", scalar(1.0));
    let node = &mut comp_mut(&mut p).nodes[0];
    node.effects.push(Effect::Known(EffectDefinition {
        effect_id: GAUSSIAN_BLUR_ID.into(),
        version: 1,
        parameters: EffectParameters::GaussianBlur { sigma: sigma.id() },
    }));
    node.properties.push(sigma);
    let s = snapshot(&p, id);
    let blurred =
        render_temporal_frame_with_cache(&s, &[], &CpuReferenceBackend, r, settings, &mut cache)
            .unwrap();
    assert_ne!(unblurred.frame.pixels, blurred.frame.pixels);
    assert_eq!(cache.stats().temporal.misses, 2);
    assert_eq!(
        blurred,
        render_temporal_frame(&s, &[], &CpuReferenceBackend, r, settings).unwrap()
    );
}

#[test]
fn temporal_semantic_pin_is_required_and_legacy_snapshot_hash_is_preserved() {
    let (p, id) = temporal_cut_sequence();
    let snapshot = RenderSnapshot::for_target(
        &p,
        RenderTarget::Sequence { sequence: id },
        7,
        RenderProfile::default(),
    )
    .unwrap();
    let mut legacy = serde_json::to_value(&snapshot).unwrap();
    legacy["semantic_versions"]
        .as_object_mut()
        .unwrap()
        .remove("temporal");
    let restored: RenderSnapshot = serde_json::from_value(legacy.clone()).unwrap();
    restored.validate().unwrap();
    assert_eq!(serde_json::to_value(&restored).unwrap(), legacy);
    assert_eq!(
        restored.content_hash().unwrap(),
        format!("{:x}", Sha256::digest(serde_json::to_vec(&legacy).unwrap()))
    );
    legacy["profile"]["temporal"] = serde_json::to_value(temporal_settings()).unwrap();
    let missing: RenderSnapshot = serde_json::from_value(legacy.clone()).unwrap();
    assert_eq!(
        missing.validate().unwrap_err().code(),
        "UNSUPPORTED_FEATURE"
    );
    legacy["semantic_versions"]["temporal"] = serde_json::json!(99);
    let future: RenderSnapshot = serde_json::from_value(legacy).unwrap();
    assert_eq!(future.validate().unwrap_err().code(), "UNSUPPORTED_FEATURE");
}

#[test]
#[ignore = "8K actual GPU host acceptance; run explicitly with --ignored"]
fn color001_8k_offline_text_mask_glow_preserves_hdr_and_alpha() {
    use std::io::{Seek, SeekFrom, Write};
    let (mut p, id) = project();
    let (matte, shape) = rectangle(
        [54.0, 32.0],
        Color::new(ColorSpace::LinearRec2020, [1.0; 3], 0.5).unwrap(),
    );
    let matte_id = matte.id;
    p.shapes.push(DocumentObject::Known(shape));
    let DocumentObject::Known(c) = &mut p.compositions[0] else {
        panic!()
    };
    c.design_extent = DesignExtent::new(1024.0, 576.0).unwrap();
    let text = c
        .nodes
        .iter_mut()
        .find(|n| matches!(n.kind, NodeKind::Text { .. }))
        .unwrap();
    let source = text.id;
    for property in &mut text.properties {
        if property.descriptor().key.as_str() == "kronello.fill_color" {
            *property = Property::new(
                property.id(),
                property.descriptor().clone(),
                PropertySource::Constant(Value::Color(
                    Color::new(ColorSpace::LinearRec2020, [3.0, 2.0, 1.0], 1.0).unwrap(),
                )),
                vec![],
                &render_registry(),
            )
            .unwrap();
        }
    }
    let sigma = constant("kronello.effect.sigma", scalar(0.5));
    let offset = constant("kronello.effect.offset", v2(0.0, 0.0));
    let color = constant(
        "kronello.effect.color",
        Value::Color(Color::new(ColorSpace::LinearRec2020, [3.0, 2.0, 1.0], 1.0).unwrap()),
    );
    let opacity = constant("kronello.effect.opacity", scalar(0.5));
    text.effects.push(Effect::Known(EffectDefinition {
        effect_id: DROP_SHADOW_ID.into(),
        version: 2,
        parameters: EffectParameters::DropShadow {
            sigma: sigma.id(),
            offset: offset.id(),
            color: color.id(),
            opacity: opacity.id(),
        },
    }));
    text.properties.extend([sigma, offset, color, opacity]);
    c.root_nodes.push(matte_id);
    c.nodes.push(matte);
    let profile = RenderProfile {
        working_space: ColorSpace::LinearRec2020,
        hdr: Some(HdrSettings {
            transfer: HdrTransfer::Pq,
        }),
        ..Default::default()
    };
    let snapshot = RenderSnapshot::with_contract(
        &p,
        id,
        1,
        profile,
        SemanticVersions::current(1),
        vec![MatteBinding {
            source: SceneKey {
                instance_path: InstancePath::root(),
                node: source,
            },
            matte: SceneKey {
                instance_path: InstancePath::root(),
                node: matte_id,
            },
            kind: MatteKind::Alpha,
            visible: false,
        }],
    )
    .unwrap();
    let gpu = kronello_gpu::GpuContext::new()
        .expect("8K acceptance requires explicit actual GPU; no fallback");
    let oracle = render_frame(
        &snapshot,
        &fonts(),
        &CpuReferenceBackend,
        FrameRequest {
            time: Time::ZERO,
            region: OutputRegion {
                origin: [20.0, 40.0 / 7.5],
                extent: [64.0 / 7.5; 2],
                pixels: [64; 2],
            },
        },
    )
    .unwrap();
    let boundary_oracle = render_frame(
        &snapshot,
        &fonts(),
        &CpuReferenceBackend,
        FrameRequest {
            time: Time::ZERO,
            region: OutputRegion {
                origin: [375.0 / 7.5, 40.0 / 7.5],
                extent: [64.0 / 7.5; 2],
                pixels: [64; 2],
            },
        },
    )
    .unwrap();
    assert!(boundary_oracle.pixels.linear.iter().any(|p| p[3] > 0.0));
    for row in 0..64 {
        for column in 30..64 {
            assert_eq!(boundary_oracle.pixels.linear[row * 64 + column][3], 0.0);
        }
    }
    let mut compared = false;
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/m4-acceptance/color-001");
    std::fs::create_dir_all(&directory).unwrap();
    let mut file = std::fs::File::create(directory.join("8k-linear.rgba16f")).unwrap();
    let mut display_file = std::fs::File::create(directory.join("8k-display.rgba8")).unwrap();
    display_file.set_len(7680 * 4320 * 4).unwrap();
    file.set_len(7680 * 4320 * 8).unwrap();
    let mut count = 0_u64;
    let mut high = false;
    let mut coverage = false;
    let metadata = render_frame_tiles(
        &snapshot,
        &fonts(),
        &gpu,
        FrameRequest {
            time: Time::ZERO,
            region: OutputRegion {
                origin: [0.0; 2],
                extent: [1024.0, 576.0],
                pixels: [7680, 4320],
            },
        },
        &mut |[x, y], tile, output| {
            if [x, y] == [0, 0] {
                for row in 0..64 {
                    for column in 0..64 {
                        let actual =
                            output.linear[(40 + row) * tile.pixels[0] as usize + 150 + column];
                        let expected = oracle.pixels.linear[row * 64 + column];
                        for i in 0..4 {
                            assert!(
                                (actual[i] - expected[i]).abs() < 0.01,
                                "8K GPU/CPU ROI: {actual:?} vs {expected:?}"
                            );
                        }
                    }
                }
                for row in 0..64 {
                    for column in 0..64 {
                        let actual =
                            output.linear[(40 + row) * tile.pixels[0] as usize + 375 + column];
                        let expected = boundary_oracle.pixels.linear[row * 64 + column];
                        for i in 0..4 {
                            assert!(
                                (actual[i] - expected[i]).abs() < 0.01,
                                "8K matte boundary GPU/CPU: {actual:?} vs {expected:?}"
                            );
                        }
                    }
                }
                let mut encoder = png::Encoder::new(
                    std::fs::File::create(directory.join("8k-top-left-tile-display.png"))?,
                    tile.pixels[0],
                    tile.pixels[1],
                );
                encoder.set_color(png::ColorType::Rgba);
                encoder.set_depth(png::BitDepth::Eight);
                encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
                let mut writer = encoder
                    .write_header()
                    .map_err(|e| RenderError::InvalidInput(e.to_string()))?;
                let display: Vec<_> = output
                    .display
                    .iter()
                    .flat_map(|p| p.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8))
                    .collect();
                writer
                    .write_image_data(&display)
                    .map_err(|e| RenderError::InvalidInput(e.to_string()))?;
                writer
                    .finish()
                    .map_err(|e| RenderError::InvalidInput(e.to_string()))?;
                compared = true;
            }
            count += output.linear.len() as u64;
            high |= output.linear.iter().any(|p| p[0] > 1.0);
            coverage |= output.linear.iter().any(|p| p[3] > 0.0 && p[3] < 1.0);
            let bytes = encode_rgba16f(&output.linear)?;
            for row in 0..tile.pixels[1] as usize {
                file.seek(SeekFrom::Start(
                    ((y as u64 + row as u64) * 7680 + x as u64) * 8,
                ))?;
                let start = row * tile.pixels[0] as usize * 8;
                file.write_all(&bytes[start..start + tile.pixels[0] as usize * 8])?;
                display_file.seek(SeekFrom::Start(
                    ((y as u64 + row as u64) * 7680 + x as u64) * 4,
                ))?;
                let display: Vec<_> = output.display
                    [row * tile.pixels[0] as usize..(row + 1) * tile.pixels[0] as usize]
                    .iter()
                    .flat_map(|p| p.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8))
                    .collect();
                display_file.write_all(&display)?;
            }
            Ok(())
        },
    )
    .unwrap();
    file.sync_all().unwrap();
    display_file.sync_all().unwrap();
    drop(display_file);
    let mut encoder = png::Encoder::new(
        std::fs::File::create(directory.join("8k-display.png")).unwrap(),
        7680,
        4320,
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let mut writer = encoder.write_header().unwrap();
    {
        let mut stream = writer.stream_writer().unwrap();
        let mut source = std::fs::File::open(directory.join("8k-display.rgba8")).unwrap();
        std::io::copy(&mut source, &mut stream).unwrap();
        stream.finish().unwrap();
    }
    writer.finish().unwrap();
    eprintln!(
        "8K artifacts {}: {} pixels; numeric {} bytes; maximum linear/display tile payload {} bytes",
        directory.display(),
        count,
        count * 8,
        512 * 512 * 32
    );
    assert_eq!(count, 7680 * 4320);
    assert_eq!(file.metadata().unwrap().len(), count * 8);
    assert!(high && coverage && compared);
    assert_eq!(metadata.numeric.clipping, "none");
    assert!(metadata.display.clipping.starts_with("display_only"));
}

#[test]
fn color001_legacy_hdr_pin_hash_and_future_profile_rejection() {
    let (p, id) = project();
    let snapshot = RenderSnapshot::new(&p, id, 1, Default::default()).unwrap();
    let mut legacy = serde_json::to_value(snapshot).unwrap();
    legacy["semantic_versions"]
        .as_object_mut()
        .unwrap()
        .remove("hdr");
    let restored: RenderSnapshot = serde_json::from_value(legacy.clone()).unwrap();
    restored.validate().unwrap();
    assert_eq!(serde_json::to_value(&restored).unwrap(), legacy);
    assert_eq!(
        restored.content_hash().unwrap(),
        format!("{:x}", Sha256::digest(serde_json::to_vec(&legacy).unwrap()))
    );
    legacy["profile"]["working_space"] = serde_json::json!("linear_rec2020");
    legacy["profile"]["hdr"] = serde_json::json!({"transfer":"pq"});
    let missing: RenderSnapshot = serde_json::from_value(legacy.clone()).unwrap();
    assert_eq!(
        missing.validate().unwrap_err().code(),
        "UNSUPPORTED_FEATURE"
    );
    legacy["semantic_versions"]["hdr"] = serde_json::json!(99);
    let future: RenderSnapshot = serde_json::from_value(legacy).unwrap();
    assert_eq!(future.validate().unwrap_err().code(), "UNSUPPORTED_FEATURE");
    let hdr = RenderSnapshot::new(
        &p,
        id,
        1,
        RenderProfile {
            working_space: ColorSpace::LinearRec2020,
            hdr: Some(HdrSettings {
                transfer: HdrTransfer::Hlg,
            }),
            ..Default::default()
        },
    )
    .unwrap();
    let metadata = render_frame(
        &hdr,
        &fonts(),
        &CpuReferenceBackend,
        FrameRequest {
            time: Time::ZERO,
            region: region(),
        },
    )
    .unwrap()
    .metadata;
    assert_eq!(metadata.hdr.unwrap().reference_white_nits, 203);
    assert_eq!(metadata.hdr.unwrap().hlg_peak_nits, Some(1000));
}

#[test]
fn text002_document_properties_ruby_vertical_render_and_reflow() {
    let (mut p, c) = project();
    let curve = AnimationCurve::new(
        CurveId::new(),
        ValueType::Vec2,
        vec![
            Keyframe {
                time: t(0, 1),
                value: v2(0.0, 0.0),
                interpolation: CurveInterpolation::Linear,
            },
            Keyframe {
                time: t(1, 1),
                value: v2(5.0, 0.0),
                interpolation: CurveInterpolation::Linear,
            },
        ],
    )
    .unwrap();
    let offset = prop(
        "kronello.text.character_offset",
        PropertySource::Curve(curve.id()),
    );
    let opacity = constant("kronello.text.character_opacity", scalar(0.5));
    let DocumentObject::Known(text) = &mut p.texts[0] else {
        panic!()
    };
    text.layout_version = 2;
    text.direction = TextDirection::VerticalRl;
    text.ruby = vec![RubyAssociation {
        base: TextRange {
            start: 0,
            end: "日本".len(),
        },
        text: "にほん".into(),
    }];
    text.character_animations = vec![CharacterAnimation {
        source: TextRange {
            start: 0,
            end: "日".len(),
        },
        expected_text: "日".into(),
        offset: offset.id(),
        opacity: opacity.id(),
        scale: None,
        rotation: None,
        fill: None,
        mode: AnimatorMode::Step,
        seed: None,
        follow_smoothing: None,
    }];
    comp_mut(&mut p).nodes[1]
        .properties
        .extend([offset, opacity]);
    p.curves.push(DocumentObject::Known(curve));
    let s = snapshot(&p, c);
    let first = frame(&s, t(0, 1));
    let last = frame(&s, t(1, 1));
    assert_ne!(first.pixels, last.pixels);
    assert_eq!(frame(&s, t(0, 1)).pixels, first.pixels);
    let DocumentObject::Known(text) = &p.texts[0] else {
        panic!()
    };
    let wrap = text.wrap_width;
    let property = comp_mut(&mut p).nodes[1]
        .properties
        .iter_mut()
        .find(|prop| prop.id() == wrap)
        .unwrap();
    property
        .set_source(PropertySource::Constant(scalar(26.0)), &render_registry())
        .unwrap();
    let reflowed = frame(&snapshot(&p, c), t(1, 1));
    assert_ne!(reflowed.pixels, last.pixels);
}

#[test]
fn vec006_text_path_property_rasterizes_along_the_guide() {
    let (mut p, c) = project();
    let flat = frame(&snapshot(&p, c), t(0, 1));
    let path = constant(
        "kronello.text.path",
        Value::Path(Path {
            segments: vec![
                PathSegment::MoveTo([f(0.0), f(24.0)]),
                PathSegment::CubicTo {
                    control1: [f(20.0), f(4.0)],
                    control2: [f(44.0), f(4.0)],
                    end: [f(64.0), f(24.0)],
                },
            ],
        }),
    );
    let DocumentObject::Known(text) = &mut p.texts[0] else {
        panic!()
    };
    text.layout_version = 2;
    text.path = Some(path.id());
    comp_mut(&mut p).nodes[1].properties.push(path);
    let guided = frame(&snapshot(&p, c), t(0, 1));
    assert_ne!(guided.pixels, flat.pixels);
    assert_eq!(frame(&snapshot(&p, c), t(0, 1)).pixels, guided.pixels);
    // A vertical or ruby guide combination is a typed rejection, not pixels.
    let DocumentObject::Known(text) = &mut p.texts[0] else {
        panic!()
    };
    text.direction = TextDirection::VerticalRl;
    let rejected = render_frame(
        &snapshot(&p, c),
        &fonts(),
        &CpuReferenceBackend,
        FrameRequest {
            time: t(0, 1),
            region: region(),
        },
    );
    assert_eq!(rejected.unwrap_err().code(), "UNSUPPORTED_FEATURE");
}

fn gui007_reverse_fixture() -> (Project, Sequence) {
    let rate = FrameRate::new(30000, 1001).unwrap();
    let step = rate.frame_to_time(1).unwrap();
    let duration = rate.frame_to_time(2).unwrap();
    let (mut red, red_shape) = rectangle([8.0, 8.0], Color::from_srgb8([255, 0, 0], None));
    red.active_range = TimeRange::new(Time::ZERO, step).unwrap();
    let (mut blue, blue_shape) = rectangle([8.0, 8.0], Color::from_srgb8([0, 0, 255], None));
    blue.active_range = TimeRange::new(step, duration).unwrap();
    let mut source = composition(vec![red, blue]);
    source.duration = Duration::new(duration).unwrap();
    source.edit_rate = rate;
    let sequence = Sequence {
        id: SequenceId::new(),
        extent: source.design_extent,
        frame_rate: rate,
        audio_rate: kronello_time::SampleRate::HZ_48000,
        working_space: ColorSpace::LinearRec709,
        tracks: vec![Track {
            state: None,
            id: TrackId::new(),
            kind: TrackKind::Video,
            clips: vec![Clip {
                id: ClipId::new(),
                source_ref: SourceRef::Composition {
                    composition: source.id,
                },
                timeline_range: TimeRange::new(Time::ZERO, duration).unwrap(),
                source_in: duration,
                time_map: TimeMap::linear(Time::ZERO, Time::ONE).unwrap(),
                enabled: true,
                audio_retime: AudioRetimePolicy::ReverseResampleV1,
                reverse_sampling: Some(ReverseSampling::ReverseGridV1),
                volume: None,
                links: vec![],
                properties: vec![],
                effects: vec![],
                masks: vec![],
                markers: vec![],
            }],
        }],
        transitions: vec![],
        markers: vec![],
        work_area: None,
        targets: None,
    };
    let project = Project {
        compositions: vec![DocumentObject::Known(source)],
        shapes: vec![
            DocumentObject::Known(red_shape),
            DocumentObject::Known(blue_shape),
        ],
        sequences: vec![DocumentObject::Known(sequence.clone())],
        ..Project::default()
    };
    (project, sequence)
}

#[test]
fn gui007_reverse_composition_renders_first_and_last_ntsc_source_frames() {
    let (project, sequence) = gui007_reverse_fixture();
    let rate = sequence.frame_rate;
    let step = rate.frame_to_time(1).unwrap();
    let duration = rate.frame_to_time(2).unwrap();
    let snapshot = RenderSnapshot::for_target(
        &project,
        RenderTarget::Sequence {
            sequence: sequence.id,
        },
        0,
        Default::default(),
    )
    .unwrap();
    let render = |time| {
        render_frame(
            &snapshot,
            &[],
            &CpuReferenceBackend,
            FrameRequest {
                time,
                region: OutputRegion {
                    origin: [0.0; 2],
                    extent: [64.0, 32.0],
                    pixels: [64, 32],
                },
            },
        )
        .unwrap()
    };
    assert!(render(Time::ZERO).pixels.linear[65][2] > 0.99);
    assert!(render(step).pixels.linear[65][0] > 0.99);
    assert!(render(step.checked_div(t(2, 1)).unwrap()).pixels.linear[65][2] > 0.99);
    assert_eq!(render(duration).pixels.linear[65], [0.0; 4]);
    let mut wire = serde_json::to_value(&snapshot).unwrap();
    wire["semantic_versions"]
        .as_object_mut()
        .unwrap()
        .remove("reverse_sampling");
    assert!(
        serde_json::from_value::<RenderSnapshot>(wire)
            .unwrap()
            .validate()
            .is_err()
    );
}

#[test]
fn gui007_reverse_temporal_boundaries_and_nested_maps_preserve_exact_source_grid() {
    let (mut project, sequence) = gui007_reverse_fixture();
    let rate = sequence.frame_rate;
    let step = rate.frame_to_time(1).unwrap();
    let duration = rate.frame_to_time(2).unwrap();
    let source_id = match sequence.tracks[0].clips[0].source_ref {
        SourceRef::Composition { composition } => composition,
        _ => unreachable!(),
    };
    let mut outer = composition(vec![node(
        NodeKind::CompositionInstance(CompositionInstance {
            id: CompositionInstanceId::new(),
            definition_ref: source_id,
            input_bindings: BTreeMap::new(),
            local_time_map: TimeMap::linear(step.checked_div(t(2, 1)).unwrap(), t(1, 2)).unwrap(),
            seed: 0,
        }),
        vec![],
    )]);
    outer.edit_rate = rate;
    outer.duration = Duration::new(duration).unwrap();
    let outer_id = outer.id;
    project.compositions.push(DocumentObject::Known(outer));
    let DocumentObject::Known(seq) = &mut project.sequences[0] else {
        unreachable!()
    };
    seq.tracks[0].clips[0].source_ref = SourceRef::Composition {
        composition: outer_id,
    };
    let snapshot = RenderSnapshot::for_target(
        &project,
        RenderTarget::Sequence {
            sequence: sequence.id,
        },
        7,
        Default::default(),
    )
    .unwrap();
    let region = OutputRegion {
        origin: [0.0; 2],
        extent: [64.0, 32.0],
        pixels: [64, 32],
    };
    for cut_policy in [CutPolicy::AllowCrossing, CutPolicy::AvoidCrossing] {
        let settings = TemporalSettings {
            frame_rate: rate,
            shutter_angle: t(360, 1),
            shutter_phase: t(-1, 2),
            samples: 8,
            cut_policy,
        };
        for time in [
            Time::ZERO,
            step,
            duration
                .checked_sub(step.checked_div(t(4, 1)).unwrap())
                .unwrap(),
            duration,
        ] {
            let samples = temporal_samples(&snapshot, time, settings).unwrap();
            let mut expected = [0.0_f32; 4];
            for sample in &samples {
                // Reverse outer sample selects its exact predecessor grid cell;
                // the ordinary nested map remains offset + positive speed.
                let color = if sample.time < Time::ZERO || sample.time >= duration {
                    [0.0; 4]
                } else if sample.time < step {
                    [0.0, 0.0, 1.0, 1.0]
                } else {
                    [1.0, 0.0, 0.0, 1.0]
                };
                let weight = sample.weight.numerator() as f32 / sample.weight.denominator() as f32;
                for channel in 0..4 {
                    expected[channel] += weight * color[channel];
                }
            }
            let mut cache = RenderCache::default();
            let request = FrameRequest { time, region };
            let cold = render_temporal_frame_with_cache(
                &snapshot,
                &[],
                &CpuReferenceBackend,
                request,
                settings,
                &mut cache,
            )
            .unwrap();
            let warm = render_temporal_frame_with_cache(
                &snapshot,
                &[],
                &CpuReferenceBackend,
                request,
                settings,
                &mut cache,
            )
            .unwrap();
            assert_eq!(cold, warm);
            assert_eq!(cache.stats().temporal.hits, 1);
            for (actual, expected) in cold.frame.pixels.linear[65].iter().zip(expected) {
                assert!(
                    (actual - expected).abs() < 1e-6,
                    "{cut_policy:?} {time:?}: {actual} != {expected}"
                );
            }
        }
    }
    // Reverse sampling is pure: seeking backward after the endpoint gives the
    // same nested child result, without changing authored positive TimeMaps.
    let render = |time| {
        render_frame(
            &snapshot,
            &[],
            &CpuReferenceBackend,
            FrameRequest { time, region },
        )
        .unwrap()
    };
    assert!(render(step).pixels.linear[65][0] > 0.99);
    assert!(render(Time::ZERO).pixels.linear[65][2] > 0.99);
    assert_eq!(render(duration).pixels.linear[65], [0.0; 4]);
    assert!(render(Time::ZERO).pixels.linear[65][2] > 0.99);
    // A fractional upper endpoint chooses the containing source cell; it must
    // not require a uniform-frame duration or subtract a floating epsilon.
    let fractional = step.checked_mul(t(5, 2)).unwrap();
    let DocumentObject::Known(seq) = &mut project.sequences[0] else {
        unreachable!()
    };
    seq.tracks[0].clips[0].source_in = fractional;
    seq.tracks[0].clips[0].timeline_range = TimeRange::new(Time::ZERO, fractional).unwrap();
    let DocumentObject::Known(outer) = project.compositions.last_mut().unwrap() else {
        unreachable!()
    };
    outer.duration = Duration::new(fractional).unwrap();
    let fractional_snapshot = RenderSnapshot::for_target(
        &project,
        RenderTarget::Sequence {
            sequence: sequence.id,
        },
        8,
        Default::default(),
    )
    .unwrap();
    let fractional_render = |time| {
        render_frame(
            &fractional_snapshot,
            &[],
            &CpuReferenceBackend,
            FrameRequest { time, region },
        )
        .unwrap()
    };
    assert!(fractional_render(Time::ZERO).pixels.linear[65][2] > 0.99);
    assert!(
        fractional_render(step.checked_mul(t(3, 2)).unwrap())
            .pixels
            .linear[65][0]
            > 0.99
    );
    assert!(
        fractional_render(step.checked_mul(t(2, 1)).unwrap())
            .pixels
            .linear[65][0]
            > 0.99
    );
    assert_eq!(fractional_render(fractional).pixels.linear[65], [0.0; 4]);
    let wire = serde_json::to_value(&fractional_snapshot).unwrap();
    let roundtrip: RenderSnapshot = serde_json::from_value(wire).unwrap();
    roundtrip.validate().unwrap();
    assert_eq!(
        fractional_render(Time::ZERO),
        render_frame(
            &roundtrip,
            &[],
            &CpuReferenceBackend,
            FrameRequest {
                time: Time::ZERO,
                region
            }
        )
        .unwrap()
    );
}
