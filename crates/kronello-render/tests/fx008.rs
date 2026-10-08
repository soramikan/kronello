//! FX-008 remaining standard effects (ADR-0137/0139): DAG forms, typed
//! displacement-map errors, and CPU-reference pixel truth.
use kronello_gpu::render_adapter::CpuReferenceBackend;
use kronello_model::*;
use kronello_render::*;
use kronello_time::{Duration, FrameRate, Time, TimeRange};
use std::collections::BTreeMap;

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
fn enumeration(v: &str) -> Value {
    Value::Enum(v.into())
}
fn constant(key: &str, value: Value) -> Property {
    let registry = render_registry();
    let descriptor = registry.lookup(&SchemaKey::new(key).unwrap()).unwrap();
    Property::new(
        PropertyId::new(),
        DescriptorRef::new(descriptor),
        PropertySource::Constant(value),
        vec![],
        &registry,
    )
    .unwrap()
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
fn region() -> OutputRegion {
    OutputRegion {
        origin: [0.0; 2],
        extent: [64.0, 32.0],
        pixels: [64, 32],
    }
}
fn project(nodes: Vec<SceneNode>, shapes: Vec<Shape>) -> (Project, CompositionId) {
    let c = composition(nodes);
    let id = c.id;
    (
        Project {
            compositions: vec![DocumentObject::Known(c)],
            shapes: shapes.into_iter().map(DocumentObject::Known).collect(),
            ..Project::default()
        },
        id,
    )
}
fn snap(p: &Project, c: CompositionId) -> RenderSnapshot {
    RenderSnapshot::new(p, c, 7, RenderProfile::default()).unwrap()
}
fn frame(s: &RenderSnapshot, time: Time) -> RenderedFrame {
    render_frame(
        s,
        &[],
        &CpuReferenceBackend,
        FrameRequest {
            time,
            region: region(),
        },
    )
    .unwrap()
}
fn scene_key(node: NodeId) -> SceneKey {
    SceneKey {
        instance_path: InstancePath::root(),
        node,
    }
}
fn positioned(mut n: SceneNode, x: f64, y: f64) -> SceneNode {
    n.properties
        .push(constant("kronello.transform.position", v2(x, y)));
    n
}
fn mixer(rows: [[f64; 4]; 4]) -> Value {
    let columns = BTreeMap::from([
        ("red".to_string(), ValueType::Scalar),
        ("green".to_string(), ValueType::Scalar),
        ("blue".to_string(), ValueType::Scalar),
        ("alpha".to_string(), ValueType::Scalar),
    ]);
    Value::DataTable(DataTable {
        columns,
        rows: rows
            .iter()
            .map(|r| {
                BTreeMap::from([
                    ("red".to_string(), Value::Scalar(f(r[0]))),
                    ("green".to_string(), Value::Scalar(f(r[1]))),
                    ("blue".to_string(), Value::Scalar(f(r[2]))),
                    ("alpha".to_string(), Value::Scalar(f(r[3]))),
                ])
            })
            .collect(),
    })
}
fn add_effect(
    n: &mut SceneNode,
    effect_id: &str,
    properties: Vec<Property>,
    parameters: impl Fn(&[PropertyId]) -> EffectParameters,
) {
    let ids: Vec<PropertyId> = properties.iter().map(|p| p.id()).collect();
    n.effects.push(Effect::Known(EffectDefinition {
        effect_id: effect_id.into(),
        version: 1,
        parameters: parameters(&ids),
    }));
    n.properties.extend(properties);
}
fn displace_effect(n: &mut SceneNode) {
    let properties = vec![
        constant(
            "kronello.effect.displace_channel_x",
            enumeration("luminance"),
        ),
        constant(
            "kronello.effect.displace_channel_y",
            enumeration("luminance"),
        ),
        constant("kronello.effect.displace_scale_x", scalar(16.0)),
        constant("kronello.effect.displace_scale_y", scalar(0.0)),
    ];
    add_effect(n, DISPLACE_ID, properties, |ids| {
        EffectParameters::Displace {
            channel_x: ids[0],
            channel_y: ids[1],
            scale_x: ids[2],
            scale_y: ids[3],
        }
    });
}
fn checkerboard_effect(n: &mut SceneNode) {
    let properties = vec![
        constant("kronello.effect.generate_kind", enumeration("checkerboard")),
        constant(
            "kronello.effect.generate_color_a",
            Value::Color(Color::new(ColorSpace::LinearRec709, [0.0, 0.0, 1.0], 1.0).unwrap()),
        ),
        constant(
            "kronello.effect.generate_color_b",
            Value::Color(Color::new(ColorSpace::LinearRec709, [1.0, 0.0, 0.0], 1.0).unwrap()),
        ),
        constant("kronello.effect.generate_point_a", v2(0.0, 0.0)),
        constant("kronello.effect.generate_point_b", v2(64.0, 64.0)),
        constant("kronello.effect.generate_cell_size", scalar(16.0)),
        constant("kronello.effect.generate_line_width", scalar(0.0)),
    ];
    add_effect(n, GENERATE_ID, properties, |ids| {
        EffectParameters::Generate {
            generator: ids[0],
            color_a: ids[1],
            color_b: ids[2],
            point_a: ids[3],
            point_b: ids[4],
            cell_size: ids[5],
            line_width: ids[6],
        }
    });
}
fn single_rect_project(
    color: [f64; 3],
    alpha: f64,
    effect_id: &str,
    properties: Vec<Property>,
    parameters: impl Fn(&[PropertyId]) -> EffectParameters,
) -> (Project, CompositionId) {
    let (mut n, shape) = rectangle(
        [64.0, 32.0],
        Color::new(ColorSpace::LinearRec709, color, alpha).unwrap(),
    );
    add_effect(&mut n, effect_id, properties, parameters);
    project(vec![n], vec![shape])
}

#[test]
fn fx008_displace_lowers_to_two_input_effect_map_and_warps() {
    // A transparent map raster (all zeros) drives luminance 0, so the
    // signed displacement is -scale_x = -16 px in the source.
    let (source, source_shape) = rectangle(
        [16.0, 16.0],
        Color::new(ColorSpace::LinearRec709, [1.0, 0.0, 0.0], 1.0).unwrap(),
    );
    let mut source = positioned(source, 32.0, 16.0);
    displace_effect(&mut source);
    let (map, map_shape) = rectangle(
        [64.0, 32.0],
        Color::new(ColorSpace::LinearRec709, [1.0, 1.0, 1.0], 0.0).unwrap(),
    );
    let map = positioned(map, 0.0, 0.0);
    let map_id = map.id;
    let source_id = source.id;
    let (p, id) = project(vec![map, source], vec![map_shape, source_shape]);
    let bound = snap(&p, id).with_displacement_maps(vec![DisplacementBinding {
        source: scene_key(source_id),
        map: scene_key(map_id),
    }]);
    let ir = build_scene_ir(&bound, t(0, 1), &[]).unwrap();
    let dag = build_render_dag(&ir, bound.profile(), region()).unwrap();
    // Exactly one EffectMap node consuming exactly two inputs; the map node
    // also keeps its normal draw position.
    let mapped: Vec<_> = dag
        .nodes()
        .iter()
        .enumerate()
        .filter(|(_, n)| matches!(n, DagNode::EffectMap { .. }))
        .collect();
    assert_eq!(mapped.len(), 1);
    let (
        _,
        DagNode::EffectMap {
            source: s, map: m, ..
        },
    ) = mapped[0]
    else {
        unreachable!()
    };
    assert_ne!(s, m);
    for (i, node) in dag.nodes().iter().enumerate() {
        assert!(node.inputs().iter().all(|input| *input < i));
    }
    // Output (48,16) samples source (32,16): inside the red rect.
    let displaced = frame(&bound, t(0, 1));
    assert_eq!(displaced.pixels.linear[16 * 64 + 48], [1.0, 0.0, 0.0, 1.0]);
    // Output (32,16) samples source (16,16): outside the rect.
    assert_eq!(displaced.pixels.linear[16 * 64 + 32], [0.0; 4]);
    // Without the effect the same composition paints (32,16) red.
    let mut plain = p.clone();
    let DocumentObject::Known(c) = &mut plain.compositions[0] else {
        panic!()
    };
    c.nodes[1].effects.clear();
    let baseline_snapshot = snap(&plain, id);
    let baseline = frame(&baseline_snapshot, t(0, 1));
    assert_eq!(baseline.pixels.linear[16 * 64 + 32], [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(baseline.pixels.linear[16 * 64 + 48], [0.0; 4]);
}

#[test]
fn fx008_displace_missing_or_duplicate_bindings_are_typed_errors() {
    let (source, source_shape) = rectangle(
        [16.0, 16.0],
        Color::new(ColorSpace::LinearRec709, [1.0, 0.0, 0.0], 1.0).unwrap(),
    );
    let mut source = positioned(source, 32.0, 16.0);
    displace_effect(&mut source);
    let (map, map_shape) = rectangle(
        [64.0, 32.0],
        Color::new(ColorSpace::LinearRec709, [1.0, 1.0, 1.0], 0.0).unwrap(),
    );
    let map_id = map.id;
    let source_id = source.id;
    let (p, id) = project(vec![map, source], vec![map_shape, source_shape]);
    // No binding at all: the effect node has no second input.
    let unbound = snap(&p, id);
    let ir = build_scene_ir(&unbound, t(0, 1), &[]).unwrap();
    let error = build_render_dag(&ir, unbound.profile(), region()).unwrap_err();
    assert_eq!(error.code(), "DISPLACE_MAP_MISSING");
    // A binding whose map key names no active scene node fails the same way.
    let missing = snap(&p, id).with_displacement_maps(vec![DisplacementBinding {
        source: scene_key(source_id),
        map: scene_key(NodeId::new()),
    }]);
    let ir = build_scene_ir(&missing, t(0, 1), &[]).unwrap();
    let error = build_render_dag(&ir, missing.profile(), region()).unwrap_err();
    assert_eq!(error.code(), "DISPLACE_MAP_MISSING");
    // Duplicate bindings for the same source are rejected, not first-wins.
    let duplicate = snap(&p, id).with_displacement_maps(vec![
        DisplacementBinding {
            source: scene_key(source_id),
            map: scene_key(map_id),
        },
        DisplacementBinding {
            source: scene_key(source_id),
            map: scene_key(map_id),
        },
    ]);
    let ir = build_scene_ir(&duplicate, t(0, 1), &[]).unwrap();
    let error = build_render_dag(&ir, duplicate.profile(), region()).unwrap_err();
    assert_eq!(error.code(), "DISPLACE_MAP_MISSING");
}

#[test]
fn fx008_generate_is_a_source_free_leaf_covering_the_surface() {
    // The authored rect is a small green square; `kronello.generate`
    // discards it and synthesizes the checkerboard over the whole region.
    let (n, shape) = rectangle(
        [8.0, 8.0],
        Color::new(ColorSpace::LinearRec709, [0.0, 1.0, 0.0], 1.0).unwrap(),
    );
    let mut n = positioned(n, 8.0, 8.0);
    checkerboard_effect(&mut n);
    let (p, id) = project(vec![n], vec![shape]);
    let snap = snap(&p, id);
    let ir = build_scene_ir(&snap, t(0, 1), &[]).unwrap();
    let dag = build_render_dag(&ir, snap.profile(), region()).unwrap();
    let leaves: Vec<_> = dag
        .nodes()
        .iter()
        .filter(|n| matches!(n, DagNode::Generate { .. }))
        .collect();
    assert_eq!(leaves.len(), 1);
    let DagNode::Generate { effect, bounds } = leaves[0] else {
        unreachable!()
    };
    assert!(matches!(effect, PixelEffect::Generate { .. }));
    assert_eq!(bounds.max, [64.0, 32.0]);
    assert!(leaves[0].inputs().is_empty());
    // Checkerboard parity at cell 16: (0,0) blue, (17,0) red, (17,17) blue.
    let generated = frame(&snap, t(0, 1));
    assert_eq!(generated.pixels.linear[0], [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(generated.pixels.linear[17], [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(generated.pixels.linear[17 * 64 + 17], [0.0, 0.0, 1.0, 1.0]);
    // The green rect content is discarded even inside its own bounds.
    assert_eq!(generated.pixels.linear[8 * 64 + 8], [0.0, 0.0, 1.0, 1.0]);
}

#[test]
fn fx008_invert_mixer_and_tint_semantics() {
    // Invert rgb on a quarter-tone solid: straight inversion re-premultiplies.
    let (p, id) = single_rect_project(
        [0.25, 0.5, 0.75],
        1.0,
        INVERT_ID,
        vec![constant(
            "kronello.effect.invert_channel",
            enumeration("rgb"),
        )],
        |ids| EffectParameters::Invert { channel: ids[0] },
    );
    let inverted = frame(&snap(&p, id), t(0, 1));
    assert_eq!(inverted.pixels.linear[16 * 64 + 32], [0.75, 0.5, 0.25, 1.0]);
    // Invert alpha keeps straight chroma under 1-a coverage.
    let (p, id) = single_rect_project(
        [1.0, 0.0, 0.0],
        0.25,
        INVERT_ID,
        vec![constant(
            "kronello.effect.invert_channel",
            enumeration("alpha"),
        )],
        |ids| EffectParameters::Invert { channel: ids[0] },
    );
    let inverted = frame(&snap(&p, id), t(0, 1));
    assert_eq!(inverted.pixels.linear[16 * 64 + 32], [0.75, 0.0, 0.0, 0.75]);
    // Channel mixer applies the row-major matrix to premultiplied rgba:
    // swapping r and b of [1, 0.25, 0.5, 1] yields [0.5, 0.25, 1, 1].
    let (p, id) = single_rect_project(
        [1.0, 0.25, 0.5],
        1.0,
        CHANNEL_MIXER_ID,
        vec![constant(
            "kronello.effect.matrix",
            mixer([
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ]),
        )],
        |ids| EffectParameters::ChannelMixer { matrix: ids[0] },
    );
    let swapped = frame(&snap(&p, id), t(0, 1));
    assert_eq!(swapped.pixels.linear[16 * 64 + 32], [0.5, 0.25, 1.0, 1.0]);
    // Tint maps straight luma 0.5 halfway between authored black/white.
    let (p, id) = single_rect_project(
        [0.5, 0.5, 0.5],
        1.0,
        TINT_ID,
        vec![
            constant(
                "kronello.effect.map_black",
                Value::Color(Color::new(ColorSpace::LinearRec709, [1.0, 0.0, 0.0], 1.0).unwrap()),
            ),
            constant(
                "kronello.effect.map_white",
                Value::Color(Color::new(ColorSpace::LinearRec709, [0.0, 0.0, 1.0], 1.0).unwrap()),
            ),
            constant("kronello.effect.tint_amount", scalar(1.0)),
        ],
        |ids| EffectParameters::Tint {
            map_black: ids[0],
            map_white: ids[1],
            amount: ids[2],
        },
    );
    let tinted = frame(&snap(&p, id), t(0, 1));
    assert_eq!(tinted.pixels.linear[16 * 64 + 32], [0.5, 0.0, 0.5, 1.0]);
}

#[test]
fn fx008_mosaic_basis_selects_block_sample() {
    // A linear black->white gradient quantized into 8 px blocks. Output
    // x=33 sits in block 4: `center` samples the gradient at x=36 while
    // `edge` samples x=32. Chaining generate -> mosaic on one node also
    // proves a Generate leaf feeds downstream single-input effects.
    let scene = |basis: &str| {
        let (n, shape) = rectangle(
            [8.0, 8.0],
            Color::new(ColorSpace::LinearRec709, [0.0, 1.0, 0.0], 1.0).unwrap(),
        );
        let mut n = positioned(n, 0.0, 0.0);
        add_effect(
            &mut n,
            GENERATE_ID,
            vec![
                constant(
                    "kronello.effect.generate_kind",
                    enumeration("gradient_linear"),
                ),
                constant(
                    "kronello.effect.generate_color_a",
                    Value::Color(
                        Color::new(ColorSpace::LinearRec709, [0.0, 0.0, 0.0], 1.0).unwrap(),
                    ),
                ),
                constant(
                    "kronello.effect.generate_color_b",
                    Value::Color(
                        Color::new(ColorSpace::LinearRec709, [1.0, 1.0, 1.0], 1.0).unwrap(),
                    ),
                ),
                constant("kronello.effect.generate_point_a", v2(0.0, 0.0)),
                constant("kronello.effect.generate_point_b", v2(64.0, 0.0)),
                constant("kronello.effect.generate_cell_size", scalar(16.0)),
                constant("kronello.effect.generate_line_width", scalar(0.0)),
            ],
            |ids| EffectParameters::Generate {
                generator: ids[0],
                color_a: ids[1],
                color_b: ids[2],
                point_a: ids[3],
                point_b: ids[4],
                cell_size: ids[5],
                line_width: ids[6],
            },
        );
        add_effect(
            &mut n,
            MOSAIC_ID,
            vec![
                constant("kronello.effect.block_size", scalar(8.0)),
                constant("kronello.effect.mosaic_basis", enumeration(basis)),
            ],
            |ids| EffectParameters::Mosaic {
                block_size: ids[0],
                basis: ids[1],
            },
        );
        project(vec![n], vec![shape])
    };
    let (p, id) = scene("center");
    let center = frame(&snap(&p, id), t(0, 1));
    let c = center.pixels.linear[16 * 64 + 33][0];
    assert!((c - 36.5 / 64.0).abs() < 0.01, "center {c}");
    let (p, id) = scene("edge");
    let edge = frame(&snap(&p, id), t(0, 1));
    let e = edge.pixels.linear[16 * 64 + 33][0];
    assert!((e - 32.5 / 64.0).abs() < 0.01, "edge {e}");
    assert_ne!(c, e);
}

#[test]
fn fx008_blurs_bleed_beyond_source_bounds() {
    // Directional blur with length 8 along +x bleeds coverage 4 px each way.
    let (n, shape) = rectangle(
        [8.0, 8.0],
        Color::new(ColorSpace::LinearRec709, [1.0, 0.0, 0.0], 1.0).unwrap(),
    );
    let mut n = positioned(n, 32.0, 16.0);
    let props = vec![
        constant("kronello.effect.angle", Value::Angle(f(0.0))),
        constant("kronello.effect.length", scalar(8.0)),
    ];
    add_effect(&mut n, DIRECTIONAL_BLUR_ID, props, |ids| {
        EffectParameters::DirectionalBlur {
            angle: ids[0],
            length: ids[1],
        }
    });
    let (p, id) = project(vec![n], vec![shape]);
    let blurred = frame(&snap(&p, id), t(0, 1));
    // `position` is the shape's top-left: unblurred coverage spans
    // x in [32,40); the +-4 px kernel bleeds to [28,44).
    assert!(blurred.pixels.linear[16 * 64 + 29][0] > 0.0);
    assert!(blurred.pixels.linear[16 * 64 + 42][0] > 0.0);
    assert_eq!(blurred.pixels.linear[16 * 64 + 20][0], 0.0);
    // Radial zoom blur streaks energy outward from the authored center.
    // `radial_center` is node-local (LocalDesign): the rect's own center.
    let (n, shape) = rectangle(
        [4.0, 4.0],
        Color::new(ColorSpace::LinearRec709, [1.0, 0.0, 0.0], 1.0).unwrap(),
    );
    let mut n = positioned(n, 32.0, 16.0);
    let props = vec![
        constant("kronello.effect.radial_mode", enumeration("zoom")),
        constant("kronello.effect.radial_amount", scalar(0.5)),
        constant("kronello.effect.radial_center", v2(2.0, 2.0)),
    ];
    add_effect(&mut n, RADIAL_BLUR_ID, props, |ids| {
        EffectParameters::RadialBlur {
            mode: ids[0],
            amount: ids[1],
            center: ids[2],
        }
    });
    let (p, id) = project(vec![n], vec![shape]);
    let radial = frame(&snap(&p, id), t(0, 1));
    // The rect covers output x in [32,36) around center (34,18): output
    // pixel 37 samples back toward the center and picks up rect energy,
    // while a far pixel stays empty.
    assert!(radial.pixels.linear[18 * 64 + 37][0] > 0.0);
    assert_eq!(radial.pixels.linear[18 * 64 + 44][0], 0.0);
}

#[test]
fn fx008_grain_is_deterministic_and_time_varying() {
    let (p, id) = single_rect_project(
        [0.5, 0.5, 0.5],
        1.0,
        GRAIN_ID,
        vec![
            constant("kronello.effect.grain_amount", scalar(0.5)),
            constant("kronello.effect.grain_size", scalar(2.0)),
            constant("kronello.effect.monochrome", Value::Bool(true)),
            constant("kronello.effect.seed", scalar(7.0)),
        ],
        |ids| EffectParameters::Grain {
            amount: ids[0],
            size: ids[1],
            monochrome: ids[2],
            seed: ids[3],
        },
    );
    let snap = snap(&p, id);
    let a = frame(&snap, t(0, 1));
    let b = frame(&snap, t(0, 1));
    assert_eq!(a.pixels.linear, b.pixels.linear);
    // Noise perturbs the flat gray: not every pixel is exactly 0.5.
    assert!(a.pixels.linear.iter().any(|p| (p[0] - 0.5).abs() > 1e-3));
    // The rational scene time folds into the seed: a different instant is a
    // different deterministic field.
    let later = frame(&snap, t(1, 1));
    assert_ne!(a.pixels.linear, later.pixels.linear);
}

#[test]
fn fx008_cache_keys_cover_both_effect_map_inputs() {
    let (source, source_shape) = rectangle(
        [16.0, 16.0],
        Color::new(ColorSpace::LinearRec709, [1.0, 0.0, 0.0], 1.0).unwrap(),
    );
    let mut source = positioned(source, 32.0, 16.0);
    displace_effect(&mut source);
    let (map, map_shape) = rectangle(
        [64.0, 32.0],
        Color::new(ColorSpace::LinearRec709, [1.0, 1.0, 1.0], 0.0).unwrap(),
    );
    let map_id = map.id;
    let source_id = source.id;
    let (p, id) = project(vec![map, source], vec![map_shape, source_shape]);
    let bound = snap(&p, id).with_displacement_maps(vec![DisplacementBinding {
        source: scene_key(source_id),
        map: scene_key(map_id),
    }]);
    let ir = build_scene_ir(&bound, t(0, 1), &[]).unwrap();
    let dag = build_render_dag(&ir, bound.profile(), region()).unwrap();
    // Cache identity must name both inputs so a map change invalidates the
    // warped result: `inputs()` feeds the per-node cache key stream.
    let (i, _) = dag
        .nodes()
        .iter()
        .enumerate()
        .find(|(_, n)| matches!(n, DagNode::EffectMap { .. }))
        .unwrap();
    assert_eq!(dag.nodes()[i].inputs().len(), 2);
}
