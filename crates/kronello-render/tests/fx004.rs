//! FX-004 clip mask rendering (ADR-0114): authored mask stacks multiply clip
//! alpha after content drawing and before clip effects. Pixel truth comes
//! from the CPU reference backend.
use kronello_gpu::render_adapter::CpuReferenceBackend;
use kronello_model::*;
use kronello_render::*;
use kronello_time::{FrameRate, Rational, SampleRate, Time, TimeMap, TimeRange};

fn t(n: i64, d: i64) -> Time {
    Time::new(n, d).unwrap()
}
fn f(v: f64) -> FiniteF64 {
    FiniteF64::new(v).unwrap()
}
fn scalar(v: f64) -> Value {
    Value::Scalar(f(v))
}
fn rect_path(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Value {
    Value::Path(Path {
        segments: vec![
            PathSegment::MoveTo([f(min_x), f(min_y)]),
            PathSegment::LineTo([f(max_x), f(min_y)]),
            PathSegment::LineTo([f(max_x), f(max_y)]),
            PathSegment::LineTo([f(min_x), f(max_y)]),
            PathSegment::Close,
        ],
    })
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
struct MaskParts {
    mask: Mask,
    properties: Vec<Property>,
}
/// Rect mask over `[min_x, min_y] -> [max_x, max_y]` in clip-local design_px.
fn rect_mask(
    mode: MaskMode,
    min_x: f64,
    max_x: f64,
    feather: f64,
    expansion: f64,
    opacity: f64,
    invert: bool,
) -> MaskParts {
    let path = prop(
        MASK_PATH_KEY,
        PropertySource::Constant(rect_path(min_x, 0.0, max_x, 2.0)),
    );
    let feather_p = prop(MASK_FEATHER_KEY, PropertySource::Constant(scalar(feather)));
    let expansion_p = prop(
        MASK_EXPANSION_KEY,
        PropertySource::Constant(scalar(expansion)),
    );
    let opacity_p = prop(MASK_OPACITY_KEY, PropertySource::Constant(scalar(opacity)));
    (
        Mask {
            id: MaskId::new(),
            path: path.id(),
            mode,
            feather: feather_p.id(),
            expansion: expansion_p.id(),
            opacity: opacity_p.id(),
            invert,
            closed: true,
        },
        vec![path, feather_p, expansion_p, opacity_p],
    )
        .into()
}
impl From<(Mask, Vec<Property>)> for MaskParts {
    fn from((mask, properties): (Mask, Vec<Property>)) -> Self {
        Self { mask, properties }
    }
}
/// 2x2 sequence with one full-frame red generator clip carrying `masks`.
fn project(parts: Vec<MaskParts>) -> (Project, ClipId) {
    let (masks, properties): (Vec<Mask>, Vec<Vec<Property>>) =
        parts.into_iter().map(|p| (p.mask, p.properties)).unzip();
    let clip = Clip {
        id: ClipId::new(),
        source_ref: SourceRef::Generator {
            generator: SOLID_GENERATOR_ID.into(),
            version: 1,
            color: Color::new(ColorSpace::LinearRec709, [1.0, 0.0, 0.0], 1.0).unwrap(),
        },
        timeline_range: TimeRange::new(Time::ZERO, t(1, 1)).unwrap(),
        source_in: Time::ZERO,
        time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
        audio_retime: Default::default(),
        reverse_sampling: None,
        volume: None,
        pan: None,
        links: vec![],
        enabled: true,
        effects: vec![],
        masks,
        markers: vec![],
        properties: properties.into_iter().flatten().collect(),
    };
    let id = clip.id;
    (
        Project {
            sequences: vec![DocumentObject::Known(Sequence {
                id: SequenceId::new(),
                extent: DesignExtent::new(2.0, 2.0).unwrap(),
                frame_rate: FrameRate::new(24, 1).unwrap(),
                audio_rate: SampleRate::HZ_48000,
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
fn snapshot(p: &Project) -> RenderSnapshot {
    let DocumentObject::Known(s) = &p.sequences[0] else {
        panic!()
    };
    RenderSnapshot::for_target(
        p,
        RenderTarget::Sequence { sequence: s.id },
        7,
        RenderProfile::default(),
    )
    .unwrap()
}
fn region() -> OutputRegion {
    OutputRegion {
        origin: [0.0; 2],
        extent: [2.0; 2],
        pixels: [2; 2],
    }
}
fn render(s: &RenderSnapshot, time: Time) -> Vec<[f32; 4]> {
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
    .pixels
    .linear
}
/// Pixel order is row-major: (0,0), (1,0), (0,1), (1,1).
fn alpha(pixels: &[[f32; 4]]) -> [f32; 4] {
    [pixels[0][3], pixels[1][3], pixels[2][3], pixels[3][3]]
}

#[test]
fn add_mask_restricts_the_clip_to_its_path() {
    let (p, _) = project(vec![rect_mask(
        MaskMode::Add,
        0.0,
        1.0,
        0.0,
        0.0,
        1.0,
        false,
    )]);
    let pixels = render(&snapshot(&p), t(1, 2));
    let [a00, a10, a01, a11] = alpha(&pixels);
    assert!(a00 > 0.99 && a01 > 0.99, "left column covered: {pixels:?}");
    assert!(a10 < 0.01 && a11 < 0.01, "right column empty: {pixels:?}");
    assert!(pixels[0][0] > 0.99 && pixels[0][2] < 0.01, "red survives");
}

#[test]
fn a_leading_subtract_mask_inverts_from_full_coverage() {
    let (p, _) = project(vec![rect_mask(
        MaskMode::Subtract,
        0.0,
        1.0,
        0.0,
        0.0,
        1.0,
        false,
    )]);
    let [a00, a10, a01, a11] = alpha(&render(&snapshot(&p), t(1, 2)));
    assert!(a00 < 0.01 && a01 < 0.01, "subtracted side empty");
    assert!(a10 > 0.99 && a11 > 0.99, "kept side drawn");
}

#[test]
fn add_then_intersect_intersects_coverage_in_authored_order() {
    let (p, _) = project(vec![
        rect_mask(MaskMode::Add, 0.0, 2.0, 0.0, 0.0, 1.0, false),
        rect_mask(MaskMode::Intersect, 0.0, 1.0, 0.0, 0.0, 1.0, false),
    ]);
    let [a00, a10, a01, a11] = alpha(&render(&snapshot(&p), t(1, 2)));
    assert!(a00 > 0.99 && a01 > 0.99);
    assert!(a10 < 0.01 && a11 < 0.01);
}

#[test]
fn difference_mode_excludes_the_overlap() {
    let (p, _) = project(vec![
        rect_mask(MaskMode::Add, 0.0, 1.0, 0.0, 0.0, 1.0, false),
        rect_mask(MaskMode::Difference, 0.0, 2.0, 0.0, 0.0, 1.0, false),
    ]);
    let [a00, a10, a01, a11] = alpha(&render(&snapshot(&p), t(1, 2)));
    assert!(a00 < 0.01 && a01 < 0.01, "overlap excluded");
    assert!(a10 > 0.99 && a11 > 0.99, "difference drawn");
}

#[test]
fn invert_flips_the_mask_coverage() {
    let (p, _) = project(vec![rect_mask(
        MaskMode::Add,
        0.0,
        1.0,
        0.0,
        0.0,
        1.0,
        true,
    )]);
    let [a00, a10, a01, a11] = alpha(&render(&snapshot(&p), t(1, 2)));
    assert!(a00 < 0.01 && a01 < 0.01);
    assert!(a10 > 0.99 && a11 > 0.99);
}

#[test]
fn mask_opacity_scales_coverage_inside_the_path() {
    let (p, _) = project(vec![rect_mask(
        MaskMode::Add,
        0.0,
        1.0,
        0.0,
        0.0,
        0.5,
        false,
    )]);
    let [a00, a10, _, _] = alpha(&render(&snapshot(&p), t(1, 2)));
    assert!((a00 - 0.5).abs() < 0.05, "half coverage: {a00}");
    assert!(a10 < 0.01);
}

#[test]
fn feather_softens_the_coverage_edge() {
    let (p, _) = project(vec![rect_mask(
        MaskMode::Add,
        0.0,
        1.0,
        0.8,
        0.0,
        1.0,
        false,
    )]);
    let [a00, a10, a01, a11] = alpha(&render(&snapshot(&p), t(1, 2)));
    // The feather crosses the pixel-1 boundary: both columns become partial.
    assert!(a10 > 0.01 && a10 < 0.99, "softened edge: {a10}");
    assert!(a11 > 0.01 && a11 < 0.99, "softened edge: {a11}");
    assert!(a00 > a10 && a01 > a11, "coverage falls off outward");
}

#[test]
fn expansion_grows_and_shrinks_the_path_in_design_pixels() {
    // Half-frame mask [0,1] expanded by 0.5 covers the left pixel fully and
    // half of the right pixel.
    let (p, _) = project(vec![rect_mask(
        MaskMode::Add,
        0.0,
        1.0,
        0.0,
        0.5,
        1.0,
        false,
    )]);
    let [a00, a10, a01, a11] = alpha(&render(&snapshot(&p), t(1, 2)));
    assert!(
        a00 > 0.99 && a01 > 0.99,
        "grown keeps left: {pixels:?}",
        pixels = [a00, a10, a01, a11]
    );
    assert!(a10 > 0.3 && a10 < 0.7, "grown covers half of right: {a10}");
    // Negative expansion of the full-width mask contracts it to the middle
    // halves of both columns.
    let (p, _) = project(vec![rect_mask(
        MaskMode::Add,
        0.0,
        2.0,
        0.0,
        -0.5,
        1.0,
        false,
    )]);
    let [a00, a10, a01, a11] = alpha(&render(&snapshot(&p), t(1, 2)));
    for a in [a00, a10, a01, a11] {
        assert!(a > 0.05 && a < 0.8, "contracted to partial: {a}");
    }
}

#[test]
fn mask_properties_evaluate_in_sequence_time() {
    // Opacity animates 1 -> 0 across the clip; the masked side fades out.
    let mut parts = rect_mask(MaskMode::Add, 0.0, 1.0, 0.0, 0.0, 1.0, false);
    let curve = AnimationCurve::new(
        CurveId::new(),
        ValueType::Scalar,
        vec![
            Keyframe {
                time: Time::ZERO,
                value: scalar(1.0),
                interpolation: CurveInterpolation::Linear,
            },
            Keyframe {
                time: t(1, 1),
                value: scalar(0.0),
                interpolation: CurveInterpolation::Linear,
            },
        ],
    )
    .unwrap();
    let curve_id = curve.id();
    parts.properties[3]
        .set_source(PropertySource::Curve(curve_id), &render_registry())
        .unwrap();
    let (mut p, _) = project(vec![parts]);
    p.curves.push(DocumentObject::Known(curve));
    let s = snapshot(&p);
    let [a00_start, ..] = alpha(&render(&s, Time::ZERO));
    let [a00_mid, a10_mid, _, _] = alpha(&render(&s, t(1, 2)));
    let [a00_end, a10_end, _, _] = alpha(&render(&s, t(99, 100)));
    assert!(a00_start > 0.99);
    assert!((a00_mid - 0.5).abs() < 0.05, "mid fade: {a00_mid}");
    assert!(a10_mid < 0.01 && a10_end < 0.01, "outside stays empty");
    assert!(a00_end < 0.05, "fully faded: {a00_end}");
}

#[test]
fn animated_path_steps_coverage_at_fixed_times() {
    // The mask path itself is a `ValueType::Path` property: a Hold curve
    // jumps between two authored rectangles at its keyframe times, and each
    // render samples the path at that fixed instant.
    let mut parts = rect_mask(MaskMode::Add, 0.0, 1.0, 0.0, 0.0, 1.0, false);
    let curve = AnimationCurve::new(
        CurveId::new(),
        ValueType::Path,
        vec![
            Keyframe {
                time: Time::ZERO,
                value: rect_path(0.0, 0.0, 1.0, 2.0),
                interpolation: CurveInterpolation::Hold,
            },
            Keyframe {
                time: t(1, 2),
                value: rect_path(0.0, 0.0, 2.0, 2.0),
                interpolation: CurveInterpolation::Hold,
            },
        ],
    )
    .unwrap();
    let curve_id = curve.id();
    parts.properties[0]
        .set_source(PropertySource::Curve(curve_id), &render_registry())
        .unwrap();
    let (mut p, _) = project(vec![parts]);
    p.curves.push(DocumentObject::Known(curve));
    let s = snapshot(&p);
    // Before the step: only the left half is covered.
    let [a00, a10, a01, a11] = alpha(&render(&s, t(1, 4)));
    assert!(a00 > 0.99 && a01 > 0.99, "left covered: {a00} {a01}");
    assert!(a10 < 0.01 && a11 < 0.01, "right empty: {a10} {a11}");
    // At and after the keyframe the path is the full frame.
    let [b00, b10, b01, b11] = alpha(&render(&s, t(3, 4)));
    for a in [b00, b10, b01, b11] {
        assert!(a > 0.99, "full coverage after step: {a}");
    }
}

#[test]
fn mask_coverage_applies_before_clip_effects() {
    // A clip-level blur after the mask softens the already-masked edge; the
    // masked-out side stays empty because the mask removed that alpha first.
    let mut parts = rect_mask(MaskMode::Add, 0.0, 1.0, 0.0, 0.0, 1.0, false);
    parts.properties.push(prop(
        "kronello.effect.sigma",
        PropertySource::Constant(scalar(4.0)),
    ));
    let (mut p, _) = project(vec![parts]);
    let DocumentObject::Known(s) = &mut p.sequences[0] else {
        panic!()
    };
    let blur_sigma = s.tracks[0].clips[0]
        .properties
        .iter()
        .find(|pr| pr.descriptor().key.as_str() == "kronello.effect.sigma")
        .unwrap()
        .id();
    s.tracks[0].clips[0].effects = vec![Effect::Known(EffectDefinition {
        effect_id: GAUSSIAN_BLUR_ID.into(),
        version: 1,
        parameters: EffectParameters::GaussianBlur { sigma: blur_sigma },
    })];
    let [a00, a10, _, _] = alpha(&render(&snapshot(&p), t(1, 2)));
    // Mask-then-effect ordering: the blur bleeds into the masked-out column;
    // had the mask applied after the effect the edge would stay hard.
    assert!(
        a10 > 0.01 && a10 < 0.95,
        "blur bleeds across the mask edge: {a10}"
    );
    assert!(a00 > a10, "masked side keeps more energy: {a00} vs {a10}");
}
