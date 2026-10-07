//! COLOR-002 end-to-end render coverage: sequence clip effects resolve through
//! the scene IR into pointwise `PixelEffect` DAG nodes and the CPU reference
//! backend produces binary16-quantized working-space pixels (ADR-0108).
use kronello_gpu::color::apply_color;
use kronello_gpu::render_adapter::CpuReferenceBackend;
use kronello_model::*;
use kronello_render::*;
use kronello_time::{FrameRate, Rational, SampleRate, Time, TimeMap, TimeRange};
use std::collections::BTreeMap;

fn t(n: i64, d: i64) -> Time {
    Time::new(n, d).unwrap()
}
fn scalar(v: f64) -> Value {
    Value::Scalar(FiniteF64::new(v).unwrap())
}
fn angle(v: f64) -> Value {
    Value::Angle(FiniteF64::new(v).unwrap())
}
fn curve_table(points: &[(f64, f64)]) -> Value {
    Value::DataTable(DataTable {
        columns: BTreeMap::from([
            ("x".to_string(), ValueType::Scalar),
            ("y".to_string(), ValueType::Scalar),
        ]),
        rows: points
            .iter()
            .map(|&(x, y)| {
                BTreeMap::from([("x".to_string(), scalar(x)), ("y".to_string(), scalar(y))])
            })
            .collect(),
    })
}
/// Pair each `(descriptor key, value)` with a `Property` and build the
/// `EffectParameters` payload from the created ids.
fn effect(
    effect_id: &str,
    parameters: &[(&str, Value)],
    build: impl Fn(&[PropertyId]) -> EffectParameters,
) -> (Effect, Vec<Property>) {
    let registry = render_registry();
    let properties = parameters
        .iter()
        .map(|(key, value)| {
            let d = registry.lookup(&SchemaKey::new(*key).unwrap()).unwrap();
            Property::new(
                PropertyId::new(),
                DescriptorRef::new(d),
                PropertySource::Constant(value.clone()),
                vec![],
                &registry,
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
/// 4x1 sequence with a single solid generator clip of the given straight color.
fn project(
    rgb: [f64; 3],
    alpha: f64,
    space: ColorSpace,
    fx: Vec<(Effect, Vec<Property>)>,
) -> Project {
    let (effects, properties): (Vec<Effect>, Vec<Vec<Property>>) = fx.into_iter().unzip();
    let clip = Clip {
        id: ClipId::new(),
        source_ref: SourceRef::Generator {
            generator: SOLID_GENERATOR_ID.into(),
            version: 1,
            color: Color::new(space, rgb, alpha).unwrap(),
        },
        timeline_range: TimeRange::new(Time::ZERO, t(1, 1)).unwrap(),
        source_in: Time::ZERO,
        time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
        audio_retime: Default::default(),
        reverse_sampling: None,
        volume: None,
        links: vec![],
        effects,
        markers: vec![],
        properties: properties.into_iter().flatten().collect(),
    };
    Project {
        sequences: vec![DocumentObject::Known(Sequence {
            id: SequenceId::new(),
            extent: DesignExtent::new(4.0, 1.0).unwrap(),
            frame_rate: FrameRate::new(24, 1).unwrap(),
            audio_rate: SampleRate::HZ_48000,
            working_space: space,
            tracks: vec![Track {
                state: None,
                id: TrackId::new(),
                kind: TrackKind::Video,
                clips: vec![clip],
            }],
            transitions: vec![],
            markers: vec![],
            work_area: None,
        })],
        ..Project::default()
    }
}
fn render(p: &Project) -> Vec<[f32; 4]> {
    let DocumentObject::Known(s) = &p.sequences[0] else {
        panic!()
    };
    let snapshot = RenderSnapshot::for_target(
        p,
        RenderTarget::Sequence { sequence: s.id },
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
                extent: [4.0, 1.0],
                pixels: [4, 1],
            },
        },
    )
    .unwrap()
    .pixels
    .linear
}
fn half(v: f32) -> f32 {
    half::f16::from_f32(v).to_f32()
}
/// Expected pixel: the pointwise oracle result normalized through the
/// binary16 surface stage, including the zero-alpha reset.
fn expected(input: [f32; 4], effects: &[PixelEffect]) -> [f32; 4] {
    let mut p = input;
    for e in effects {
        p = apply_color(p, e).map(half);
    }
    if p[3] == 0.0 { [0.0; 4] } else { p }
}
fn premult(rgb: [f64; 3], alpha: f64) -> [f32; 4] {
    [
        half((rgb[0] * alpha) as f32),
        half((rgb[1] * alpha) as f32),
        half((rgb[2] * alpha) as f32),
        half(alpha as f32),
    ]
}

#[test]
fn color002_exposure_applies_pointwise_and_keeps_extended_range() {
    let (fx, props) = effect(
        COLOR_EXPOSURE_ID,
        &[
            ("kronello.effect.exposure", scalar(1.0)),
            ("kronello.effect.exposure_offset", scalar(-0.25)),
        ],
        |ids| EffectParameters::ColorExposure {
            exposure: ids[0],
            offset: ids[1],
        },
    );
    let p = project(
        [0.5, 0.25, 1.0],
        1.0,
        ColorSpace::LinearRec709,
        vec![(fx, props)],
    );
    let effect = PixelEffect::ColorExposure {
        exposure: 1.0,
        offset: -0.25,
    };
    let want = expected(premult([0.5, 0.25, 1.0], 1.0), &[effect]);
    for px in render(&p) {
        assert_eq!(px, want);
    }
    // HDR output stays above 1.0; nothing is clamped.
    assert!(want[2] > 1.0);
}

#[test]
fn color002_effects_stack_in_authored_order() {
    let mk = |ev: f64, black: f64| {
        (
            effect(
                COLOR_EXPOSURE_ID,
                &[
                    ("kronello.effect.exposure", scalar(ev)),
                    ("kronello.effect.exposure_offset", scalar(0.0)),
                ],
                |ids| EffectParameters::ColorExposure {
                    exposure: ids[0],
                    offset: ids[1],
                },
            ),
            effect(
                COLOR_LEVELS_ID,
                &[
                    ("kronello.effect.in_black", scalar(black)),
                    ("kronello.effect.in_white", scalar(1.0)),
                    ("kronello.effect.gamma", scalar(1.0)),
                    ("kronello.effect.out_black", scalar(0.0)),
                    ("kronello.effect.out_white", scalar(1.0)),
                ],
                |ids| EffectParameters::ColorLevels {
                    in_black: ids[0],
                    in_white: ids[1],
                    gamma: ids[2],
                    out_black: ids[3],
                    out_white: ids[4],
                },
            ),
        )
    };
    let exposure = PixelEffect::ColorExposure {
        exposure: -1.0,
        offset: 0.0,
    };
    let levels = PixelEffect::ColorLevels {
        in_black: 0.25,
        in_white: 1.0,
        gamma: 1.0,
        out_black: 0.0,
        out_white: 1.0,
    };
    let rgb = [0.75, 0.5, 0.25];
    let (a, b) = mk(-1.0, 0.25);
    let forward = project(rgb, 1.0, ColorSpace::LinearRec709, vec![a, b]);
    let (a, b) = mk(-1.0, 0.25);
    let reversed = project(rgb, 1.0, ColorSpace::LinearRec709, vec![b, a]);
    let input = premult(rgb, 1.0);
    let want_forward = expected(input, &[exposure.clone(), levels.clone()]);
    let want_reversed = expected(input, &[levels, exposure]);
    assert_ne!(want_forward, want_reversed);
    assert_eq!(render(&forward)[0], want_forward);
    assert_eq!(render(&reversed)[0], want_reversed);
}

#[test]
fn color002_alpha_is_preserved_through_pointwise_effects() {
    for (id, params, build, pixel) in [
        (
            COLOR_EXPOSURE_ID,
            vec![
                ("kronello.effect.exposure", scalar(0.0)),
                ("kronello.effect.exposure_offset", scalar(0.1)),
            ],
            Box::new(|ids: &[PropertyId]| EffectParameters::ColorExposure {
                exposure: ids[0],
                offset: ids[1],
            }) as Box<dyn Fn(&[PropertyId]) -> EffectParameters>,
            PixelEffect::ColorExposure {
                exposure: 0.0,
                offset: 0.1,
            },
        ),
        (
            COLOR_HSL_ID,
            vec![
                ("kronello.effect.hue_shift", angle(60.0)),
                ("kronello.effect.saturation", scalar(0.5)),
                ("kronello.effect.lightness", scalar(0.05)),
            ],
            Box::new(|ids: &[PropertyId]| EffectParameters::ColorHsl {
                hue_shift: ids[0],
                saturation: ids[1],
                lightness: ids[2],
            }) as Box<dyn Fn(&[PropertyId]) -> EffectParameters>,
            PixelEffect::ColorHsl {
                hue_shift: 60.0,
                saturation: 0.5,
                lightness: 0.05,
            },
        ),
    ] {
        let (fx, props) = effect(id, &params, |ids| build(ids));
        let p = project(
            [0.8, 0.4, 0.2],
            0.5,
            ColorSpace::LinearRec709,
            vec![(fx, props)],
        );
        let want = expected(premult([0.8, 0.4, 0.2], 0.5), &[pixel]);
        assert_eq!(render(&p)[0], want);
        assert_eq!(want[3], half(0.5), "{id} must not touch alpha");
    }
}

#[test]
fn color002_curves_apply_monotone_cubic_per_channel() {
    let (fx, props) = effect(
        COLOR_CURVES_ID,
        &[(
            "kronello.effect.curve",
            curve_table(&[(0.0, 0.0), (0.25, 0.75), (1.0, 1.0)]),
        )],
        |ids| EffectParameters::ColorCurves { curve: ids[0] },
    );
    let p = project(
        [0.25, 0.5, 0.75],
        1.0,
        ColorSpace::LinearRec709,
        vec![(fx, props)],
    );
    let want = expected(
        premult([0.25, 0.5, 0.75], 1.0),
        &[PixelEffect::ColorCurves {
            points: vec![[0.0, 0.0], [0.25, 0.75], [1.0, 1.0]],
        }],
    );
    for px in render(&p) {
        assert_eq!(px, want);
    }
}

#[test]
fn color002_scene_ir_resolves_clip_effects_and_dag_has_pointwise_nodes() {
    let (fx, props) = effect(
        COLOR_HSL_ID,
        &[
            ("kronello.effect.hue_shift", angle(30.0)),
            ("kronello.effect.saturation", scalar(1.0)),
            ("kronello.effect.lightness", scalar(0.0)),
        ],
        |ids| EffectParameters::ColorHsl {
            hue_shift: ids[0],
            saturation: ids[1],
            lightness: ids[2],
        },
    );
    let p = project([0.5; 3], 1.0, ColorSpace::LinearRec709, vec![(fx, props)]);
    let DocumentObject::Known(s) = &p.sequences[0] else {
        panic!()
    };
    let snapshot = RenderSnapshot::for_target(
        &p,
        RenderTarget::Sequence { sequence: s.id },
        7,
        RenderProfile::default(),
    )
    .unwrap();
    assert_eq!(
        snapshot.semantic_versions().effects.get(COLOR_HSL_ID),
        Some(&1)
    );
    let scene = build_scene_ir(&snapshot, t(1, 2), &[]).unwrap();
    assert_eq!(
        scene.nodes[0].effects,
        vec![ResolvedEffect::ColorHsl {
            hue_shift: 30.0,
            saturation: 1.0,
            lightness: 0.0
        }]
    );
    let dag = build_render_dag(
        &scene,
        RenderProfile::default(),
        OutputRegion {
            origin: [0.0; 2],
            extent: [4.0, 1.0],
            pixels: [4, 1],
        },
    )
    .unwrap();
    assert!(dag.nodes().iter().any(|n| matches!(
        n,
        DagNode::Effect {
            effect: PixelEffect::ColorHsl { .. },
            ..
        }
    )));
}

#[test]
fn color002_unsupported_effect_version_fails_at_evaluation() {
    let (mut fx, props) = effect(
        COLOR_EXPOSURE_ID,
        &[
            ("kronello.effect.exposure", scalar(0.0)),
            ("kronello.effect.exposure_offset", scalar(0.0)),
        ],
        |ids| EffectParameters::ColorExposure {
            exposure: ids[0],
            offset: ids[1],
        },
    );
    let Effect::Known(d) = &mut fx else { panic!() };
    d.version = 2;
    let p = project([0.5; 3], 1.0, ColorSpace::LinearRec709, vec![(fx, props)]);
    let DocumentObject::Known(s) = &p.sequences[0] else {
        panic!()
    };
    let snapshot = RenderSnapshot::for_target(
        &p,
        RenderTarget::Sequence { sequence: s.id },
        7,
        RenderProfile::default(),
    )
    .unwrap();
    assert!(matches!(
        build_scene_ir(&snapshot, t(1, 2), &[]),
        Err(RenderError::Effect(EffectError::UnsupportedFeature))
            | Err(RenderError::UnsupportedFeature(_))
    ));
}
