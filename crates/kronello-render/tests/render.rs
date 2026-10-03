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
        wrap_width: wrap.id(),
        line_height: line.id(),
        alignment: alignment.id(),
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
    assert_eq!(error.code(), "UNSUPPORTED_FEATURE");
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
    for (time, expected) in previous.into_iter().rev() {
        assert_eq!(
            render_frame(
                &s,
                &fonts(),
                &gpu,
                FrameRequest {
                    time,
                    region: region()
                }
            )
            .unwrap(),
            expected
        );
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
            start: [f(0.0); 2],
            end: [f(0.0); 2],
            stops: vec![],
        },
        Gradient::Radial {
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
fn vec004_vec005_payloads_roundtrip_but_fail_final_render() {
    let (mut p, id) = gradient_project(false);
    let width = constant("kronello.stroke_width", scalar(2.0));
    let join = constant("kronello.shape.stroke_join", Value::Enum("miter".into()));
    let cap = constant("kronello.shape.stroke_cap", Value::Enum("butt".into()));
    let limit = constant("kronello.shape.miter_limit", scalar(4.0));
    let DocumentObject::Known(shape) = &mut p.shapes[0] else {
        panic!()
    };
    shape.stroke = Some(Stroke {
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
fn text_gradient_is_preserved_as_opaque_and_snapshot_refuses_it() {
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
