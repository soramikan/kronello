//! FX-005/FX-006 end-to-end render coverage (ADR-0115): clip-level keying,
//! glow, sharpen, vignette and corner-pin resolve through the scene IR into
//! `PixelEffect` DAG nodes, and the CPU reference backend applies the
//! documented premultiplied-space semantics.
use kronello_gpu::render_adapter::CpuReferenceBackend;
use kronello_model::*;
use kronello_render::*;
use kronello_time::{FrameRate, Rational, SampleRate, Time, TimeMap, TimeRange};

fn t(n: i64, d: i64) -> Time {
    Time::new(n, d).unwrap()
}
fn scalar(v: f64) -> Value {
    Value::Scalar(FiniteF64::new(v).unwrap())
}
fn vec2(x: f64, y: f64) -> Value {
    Value::Vec2([FiniteF64::new(x).unwrap(), FiniteF64::new(y).unwrap()])
}
fn color(r: u8, g: u8, b: u8) -> Value {
    Value::Color(Color::from_srgb8([r, g, b], None))
}
fn effect(
    registry: &SchemaRegistry,
    effect_id: &str,
    parameters: &[(&str, Value)],
    build: impl Fn(&[PropertyId]) -> EffectParameters,
) -> (Effect, Vec<Property>) {
    let properties = parameters
        .iter()
        .map(|(key, value)| {
            let d = registry.lookup(&SchemaKey::new(*key).unwrap()).unwrap();
            Property::new(
                PropertyId::new(),
                DescriptorRef::new(d),
                PropertySource::Constant(value.clone()),
                vec![],
                registry,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let ids = properties.iter().map(|p| p.id()).collect::<Vec<_>>();
    (
        Effect::Known(EffectDefinition {
            effect_id: effect_id.into(),
            version: 1,
            parameters: build(&ids),
        }),
        properties,
    )
}
/// WxH sequence with a single solid generator clip of the given straight color.
fn project(
    extent: [f64; 2],
    rgb: [f64; 3],
    alpha: f64,
    color_space: ColorSpace,
    fx: Vec<(Effect, Vec<Property>)>,
) -> (Project, SequenceId) {
    let (effects, properties): (Vec<Effect>, Vec<Vec<Property>>) = fx.into_iter().unzip();
    let clip = Clip {
        id: ClipId::new(),
        source_ref: SourceRef::Generator {
            generator: SOLID_GENERATOR_ID.into(),
            version: 1,
            color: Color::new(color_space, rgb, alpha).unwrap(),
        },
        timeline_range: TimeRange::new(Time::ZERO, t(1, 1)).unwrap(),
        source_in: Time::ZERO,
        time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
        audio_retime: Default::default(),
        reverse_sampling: None,
        volume: None,
        links: vec![],
        enabled: true,
        effects,
        masks: vec![],
        markers: vec![],
        properties: properties.into_iter().flatten().collect(),
    };
    let id = SequenceId::new();
    (
        Project {
            sequences: vec![DocumentObject::Known(Sequence {
                id,
                extent: DesignExtent::new(extent[0], extent[1]).unwrap(),
                frame_rate: FrameRate::new(24, 1).unwrap(),
                audio_rate: SampleRate::HZ_48000,
                // Sequences always evaluate in a linear working space.
                working_space: ColorSpace::LinearRec709,
                tracks: vec![Track {
                    state: None,
                    id: TrackId::new(),
                    kind: TrackKind::Video,
                    clips: vec![clip],
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
fn render(p: &Project, target: SequenceId, pixels: [u32; 2], extent: [f64; 2]) -> Vec<[f32; 4]> {
    let snapshot = RenderSnapshot::for_target(
        p,
        RenderTarget::Sequence { sequence: target },
        7,
        RenderProfile::default(),
    )
    .unwrap();
    render_frame(
        &snapshot,
        &[],
        &CpuReferenceBackend,
        FrameRequest {
            time: t(1, 2),
            region: OutputRegion {
                origin: [0.0; 2],
                extent,
                pixels,
            },
        },
    )
    .unwrap()
    .pixels
    .linear
}
fn dag(
    p: &Project,
    target: SequenceId,
    pixels: [u32; 2],
    extent: [f64; 2],
) -> (RenderSnapshot, RenderDag) {
    let snapshot = RenderSnapshot::for_target(
        p,
        RenderTarget::Sequence { sequence: target },
        7,
        RenderProfile::default(),
    )
    .unwrap();
    let scene = build_scene_ir(&snapshot, t(1, 2), &[]).unwrap();
    let dag = build_render_dag(
        &scene,
        RenderProfile::default(),
        OutputRegion {
            origin: [0.0; 2],
            extent,
            pixels,
        },
    )
    .unwrap();
    (snapshot, dag)
}
fn chroma_effect(r: &SchemaRegistry, similarity: f64, spill: f64) -> (Effect, Vec<Property>) {
    effect(
        r,
        KEYING_CHROMA_ID,
        &[
            ("kronello.effect.key_color", color(0, 177, 64)),
            ("kronello.effect.similarity", scalar(similarity)),
            ("kronello.effect.edge_shrink", scalar(0.0)),
            ("kronello.effect.edge_feather", scalar(0.0)),
            ("kronello.effect.spill", scalar(spill)),
        ],
        |ids| EffectParameters::ChromaKey {
            key_color: ids[0],
            similarity: ids[1],
            edge_shrink: ids[2],
            edge_feather: ids[3],
            spill: ids[4],
        },
    )
}

#[test]
fn fx005_chroma_key_removes_key_color_and_preserves_others() {
    let r = render_registry();
    // The solid matches the sRGB key color exactly: the whole clip keys out.
    let (fx, props) = chroma_effect(&r, 0.4, 0.5);
    let (p, seq) = project(
        [4.0, 1.0],
        [0.0, 177.0 / 255.0, 64.0 / 255.0],
        1.0,
        ColorSpace::Srgb,
        vec![(fx, props)],
    );
    for px in render(&p, seq, [4, 1], [4.0, 1.0]) {
        assert_eq!(px, [0.0; 4], "keyed pixel must be transparent");
    }
    // A contrasting red pixel survives: alpha stays 1 and rgb is untouched.
    let (fx, props) = chroma_effect(&r, 0.4, 0.5);
    let (p, seq) = project(
        [4.0, 1.0],
        [1.0, 0.0, 0.0],
        1.0,
        ColorSpace::Srgb,
        vec![(fx, props)],
    );
    for px in render(&p, seq, [4, 1], [4.0, 1.0]) {
        assert_eq!(px[3], 1.0);
        assert!(px[0] > 0.9, "{px:?}");
    }
    // Transparent input never gains coverage from keying.
    let (fx, props) = chroma_effect(&r, 0.4, 0.5);
    let (p, seq) = project(
        [4.0, 1.0],
        [0.0, 1.0, 0.0],
        0.0,
        ColorSpace::Srgb,
        vec![(fx, props)],
    );
    for px in render(&p, seq, [4, 1], [4.0, 1.0]) {
        assert_eq!(px, [0.0; 4]);
    }
}

#[test]
fn fx005_luma_key_drives_alpha_from_luminance_distance() {
    let r = render_registry();
    let luma = |ids: &[PropertyId]| EffectParameters::LumaKey {
        key_luma: ids[0],
        tolerance: ids[1],
        edge_shrink: ids[2],
        edge_feather: ids[3],
    };
    let mk = |key_luma: f64, tolerance: f64| {
        effect(
            &r,
            KEYING_LUMA_ID,
            &[
                ("kronello.effect.key_luma", scalar(key_luma)),
                ("kronello.effect.tolerance", scalar(tolerance)),
                ("kronello.effect.edge_shrink", scalar(0.0)),
                ("kronello.effect.edge_feather", scalar(0.0)),
            ],
            luma,
        )
    };
    // Black pixel against key_luma 0 with tolerance 0.1 keys out fully.
    let (fx, props) = mk(0.0, 0.1);
    let (p, seq) = project(
        [4.0, 1.0],
        [0.0; 3],
        1.0,
        ColorSpace::LinearRec709,
        vec![(fx, props)],
    );
    for px in render(&p, seq, [4, 1], [4.0, 1.0]) {
        assert_eq!(px, [0.0; 4]);
    }
    // White pixel against the same key is untouched.
    let (fx, props) = mk(0.0, 0.1);
    let (p, seq) = project(
        [4.0, 1.0],
        [1.0; 3],
        1.0,
        ColorSpace::LinearRec709,
        vec![(fx, props)],
    );
    for px in render(&p, seq, [4, 1], [4.0, 1.0]) {
        assert_eq!(px, [1.0; 4]);
    }
    // Tolerance zero disables the matte entirely.
    let (fx, props) = mk(0.0, 0.0);
    let (p, seq) = project(
        [4.0, 1.0],
        [0.0; 3],
        1.0,
        ColorSpace::LinearRec709,
        vec![(fx, props)],
    );
    for px in render(&p, seq, [4, 1], [4.0, 1.0]) {
        assert_eq!(px[3], 1.0);
    }
}

#[test]
fn fx006_glow_adds_blurred_bloom_and_expands_bounds() {
    let r = render_registry();
    let (fx, props) = effect(
        &r,
        GLOW_ID,
        &[
            ("kronello.effect.threshold", scalar(0.5)),
            ("kronello.effect.radius", scalar(1.0)),
            ("kronello.effect.intensity", scalar(1.0)),
        ],
        |ids| EffectParameters::Glow {
            threshold: ids[0],
            radius: ids[1],
            intensity: ids[2],
        },
    );
    // Opaque white solid: every pixel blooms, and alpha stays source-over.
    let (p, seq) = project(
        [4.0, 4.0],
        [1.0; 3],
        1.0,
        ColorSpace::LinearRec709,
        vec![(fx, props)],
    );
    for px in render(&p, seq, [4, 4], [4.0, 4.0]) {
        // src + blurred white bloom: interior is > 1 (additive, no clamp below 1).
        assert!(px[0] > 1.0, "{px:?}");
        assert!(px[3] <= 1.0);
    }
    // The glow halo grows the node visual bounds by the 3-sigma support.
    let (p2, seq2) = {
        let r = render_registry();
        let (fx, props) = effect(
            &r,
            GLOW_ID,
            &[
                ("kronello.effect.threshold", scalar(0.5)),
                ("kronello.effect.radius", scalar(1.0)),
                ("kronello.effect.intensity", scalar(1.0)),
            ],
            |ids| EffectParameters::Glow {
                threshold: ids[0],
                radius: ids[1],
                intensity: ids[2],
            },
        );
        project(
            [4.0, 4.0],
            [1.0; 3],
            1.0,
            ColorSpace::LinearRec709,
            vec![(fx, props)],
        )
    };
    let (_, dag) = dag(&p2, seq2, [4, 4], [4.0, 4.0]);
    let glow = dag
        .nodes()
        .iter()
        .find_map(|n| match n {
            DagNode::Effect {
                effect: PixelEffect::Glow { radius, .. },
                ..
            } => Some(*radius),
            _ => None,
        })
        .expect("glow node");
    assert_eq!(glow, [1.0; 2]);
    let glow_effect = PixelEffect::Glow {
        threshold: 0.5,
        radius: [1.0; 2],
        intensity: 1.0,
    };
    let input = PixelBounds {
        min: [0.0; 2],
        max: [4.0; 2],
    };
    assert_eq!(
        glow_effect.output_bounds(input),
        PixelBounds {
            min: [-3.0; 2],
            max: [7.0; 2]
        }
    );
    assert_eq!(glow_effect.halo(), [3.0; 2]);
}

#[test]
fn fx006_sharpen_and_vignette_keep_alpha_and_edge_contracts() {
    let r = render_registry();
    // Sharpen with zero amount is the identity on a flat field.
    let (fx, props) = effect(
        &r,
        SHARPEN_ID,
        &[
            ("kronello.effect.amount", scalar(0.0)),
            ("kronello.effect.radius", scalar(1.0)),
        ],
        |ids| EffectParameters::Sharpen {
            amount: ids[0],
            radius: ids[1],
        },
    );
    let (p, seq) = project(
        [4.0, 1.0],
        [0.5; 3],
        1.0,
        ColorSpace::LinearRec709,
        vec![(fx, props)],
    );
    for px in render(&p, seq, [4, 1], [4.0, 1.0]) {
        assert_eq!(px[3], 1.0);
        assert!((px[0] - 0.5).abs() < 0.01, "{px:?}");
    }
    // Vignette amount 1 with midpoint 0 fully darkens the corner pixels but
    // never touches alpha.
    let (fx, props) = effect(
        &r,
        VIGNETTE_ID,
        &[
            ("kronello.effect.amount", scalar(1.0)),
            ("kronello.effect.midpoint", scalar(0.0)),
            ("kronello.effect.feather", scalar(0.5)),
            ("kronello.effect.roundness", scalar(1.0)),
        ],
        |ids| EffectParameters::Vignette {
            amount: ids[0],
            midpoint: ids[1],
            feather: ids[2],
            roundness: ids[3],
        },
    );
    let (p, seq) = project(
        [4.0, 4.0],
        [1.0; 3],
        1.0,
        ColorSpace::LinearRec709,
        vec![(fx, props)],
    );
    let out = render(&p, seq, [4, 4], [4.0, 4.0]);
    assert_eq!(out[0][3], 1.0);
    assert!(out[0][0] < 0.9, "corner darkens: {:?}", out[0]);
    let center = out[4 + 1];
    assert!(
        center[0] > out[0][0],
        "center stays brighter than the corner"
    );
}

#[test]
fn fx006_corner_pin_warps_content_quad_and_reports_hull_bounds() {
    let r = render_registry();
    // Pin the full-frame solid into the inner 2x2 quad of a 4x4 frame.
    let (fx, props) = effect(
        &r,
        CORNER_PIN_ID,
        &[
            ("kronello.effect.top_left", vec2(1.0, 1.0)),
            ("kronello.effect.top_right", vec2(3.0, 1.0)),
            ("kronello.effect.bottom_right", vec2(3.0, 3.0)),
            ("kronello.effect.bottom_left", vec2(1.0, 3.0)),
        ],
        |ids| EffectParameters::CornerPin {
            top_left: ids[0],
            top_right: ids[1],
            bottom_right: ids[2],
            bottom_left: ids[3],
        },
    );
    let (p, seq) = project(
        [4.0, 4.0],
        [0.25, 0.5, 0.75],
        1.0,
        ColorSpace::LinearRec709,
        vec![(fx, props)],
    );
    let out = render(&p, seq, [4, 4], [4.0, 4.0]);
    // Corners outside the pinned quad are transparent; the interior holds the
    // solid's premultiplied color.
    assert_eq!(out[0], [0.0; 4]);
    let inside = out[4 + 1];
    assert_eq!(inside[3], 1.0);
    assert!((inside[0] - 0.25).abs() < 1e-3, "{inside:?}");
    // Output bounds are exactly the destination quad hull.
    let (_, dag) = dag(&p, seq, [4, 4], [4.0, 4.0]);
    let pin = dag
        .nodes()
        .iter()
        .find_map(|n| match n {
            DagNode::Effect {
                effect: PixelEffect::CornerPin { pins, source },
                ..
            } => Some((*pins, *source)),
            _ => None,
        })
        .expect("corner pin node");
    let hull = corner_pin_hull(pin.0);
    assert_eq!(
        hull,
        PixelBounds {
            min: [1.0; 2],
            max: [3.0; 2]
        }
    );
    // The DAG builder filled the source quad with the input's visual bounds.
    assert_eq!(
        pin.1,
        Some(PixelBounds {
            min: [0.0; 2],
            max: [4.0; 2]
        })
    );
    // Identity pins reproduce the source exactly.
    let (fx, props) = effect(
        &r,
        CORNER_PIN_ID,
        &[
            ("kronello.effect.top_left", vec2(0.0, 0.0)),
            ("kronello.effect.top_right", vec2(4.0, 0.0)),
            ("kronello.effect.bottom_right", vec2(4.0, 4.0)),
            ("kronello.effect.bottom_left", vec2(0.0, 4.0)),
        ],
        |ids| EffectParameters::CornerPin {
            top_left: ids[0],
            top_right: ids[1],
            bottom_right: ids[2],
            bottom_left: ids[3],
        },
    );
    let (p, seq) = project(
        [4.0, 4.0],
        [0.25, 0.5, 0.75],
        1.0,
        ColorSpace::LinearRec709,
        vec![(fx, props)],
    );
    for px in render(&p, seq, [4, 4], [4.0, 4.0]) {
        assert!((px[0] - 0.25).abs() < 1e-3 && px[3] == 1.0, "{px:?}");
    }
}

#[test]
fn fx006_corner_pin_rejects_degenerate_quads() {
    // Collinear or self-intersecting pin quads are typed errors in validate.
    for pins in [
        [[0.0, 0.0], [4.0, 0.0], [0.0, 0.0], [4.0, 4.0]],
        [[0.0, 0.0], [4.0, 4.0], [4.0, 0.0], [0.0, 4.0]],
        [[1.0, 1.0]; 4],
    ] {
        let effect = PixelEffect::CornerPin { pins, source: None };
        assert!(effect.validate().is_err(), "{pins:?}");
    }
    // A simple axis-aligned quad validates.
    PixelEffect::CornerPin {
        pins: [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]],
        source: None,
    }
    .validate()
    .unwrap();
    // corner_pin_inverse maps quad corners to source rect corners.
    let rect = PixelBounds {
        min: [10.0, 20.0],
        max: [18.0, 28.0],
    };
    let pins = [[1.0, 1.0], [3.0, 1.0], [3.0, 3.0], [1.0, 3.0]];
    let m = corner_pin_inverse(rect, pins).unwrap();
    for (pin, want) in pins
        .iter()
        .zip([[10.0_f32, 20.0], [18.0, 20.0], [18.0, 28.0], [10.0, 28.0]])
    {
        let h = [
            m[0][0] * pin[0] + m[0][1] * pin[1] + m[0][2],
            m[1][0] * pin[0] + m[1][1] * pin[1] + m[1][2],
            m[2][0] * pin[0] + m[2][1] * pin[1] + m[2][2],
        ];
        assert!(
            (h[0] / h[2] - want[0]).abs() < 1e-4 && (h[1] / h[2] - want[1]).abs() < 1e-4,
            "{pin:?} -> {h:?} want {want:?}"
        );
    }
}

#[test]
fn fx005006_semantic_versions_and_effect_nodes_appear_in_dag() {
    let r = render_registry();
    let (fx, props) = chroma_effect(&r, 0.4, 0.5);
    let (p, seq) = project(
        [4.0, 1.0],
        [1.0; 3],
        1.0,
        ColorSpace::LinearRec709,
        vec![(fx, props)],
    );
    let (snapshot, dag) = dag(&p, seq, [4, 1], [4.0, 1.0]);
    for id in [
        KEYING_CHROMA_ID,
        KEYING_LUMA_ID,
        GLOW_ID,
        SHARPEN_ID,
        VIGNETTE_ID,
        CORNER_PIN_ID,
    ] {
        assert_eq!(
            snapshot.semantic_versions().effects.get(id),
            Some(&1),
            "{id}"
        );
    }
    assert!(dag.nodes().iter().any(|n| matches!(
        n,
        DagNode::Effect {
            effect: PixelEffect::ChromaKey { .. },
            ..
        }
    )));
    // Standard effects carry the shared FX-005/006 kernel tag and v1 semver.
    let effect = dag
        .nodes()
        .iter()
        .find_map(|n| match n {
            DagNode::Effect { effect, .. } => Some(effect.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(effect.semantic_version(), 1);
    assert_eq!(effect.kernel_version(), STANDARD_KERNEL_VERSION);
}
