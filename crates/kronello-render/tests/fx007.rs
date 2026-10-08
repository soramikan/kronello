//! FX-007 adjustment clips (ADR-0116): an adjustment clip applies its
//! `clip.effects` to the composited video below it over its own timeline
//! range. Pixel truth comes from the CPU reference backend.
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
fn solid(rgb: [f64; 3], start: Time, end: Time) -> Clip {
    Clip {
        id: ClipId::new(),
        source_ref: SourceRef::Generator {
            generator: SOLID_GENERATOR_ID.into(),
            version: 1,
            color: Color::new(ColorSpace::LinearRec709, rgb, 1.0).unwrap(),
        },
        timeline_range: TimeRange::new(start, end).unwrap(),
        source_in: Time::ZERO,
        time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
        audio_retime: Default::default(),
        reverse_sampling: None,
        volume: None,
        pan: None,
        links: vec![],
        enabled: true,
        effects: vec![],
        masks: vec![],
        markers: vec![],
        properties: vec![],
    }
}
/// An adjustment clip applying exposure `ev` (2^ev multiplier) over `range`.
fn adjustment(range: TimeRange, ev: f64) -> Clip {
    let exposure = prop(
        "kronello.effect.exposure",
        PropertySource::Constant(scalar(ev)),
    );
    let offset = prop(
        "kronello.effect.exposure_offset",
        PropertySource::Constant(scalar(0.0)),
    );
    Clip {
        id: ClipId::new(),
        source_ref: SourceRef::Adjustment,
        timeline_range: range,
        source_in: Time::ZERO,
        time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
        audio_retime: Default::default(),
        reverse_sampling: None,
        volume: None,
        pan: None,
        links: vec![],
        enabled: true,
        effects: vec![Effect::Known(EffectDefinition {
            effect_id: COLOR_EXPOSURE_ID.into(),
            version: 1,
            parameters: EffectParameters::ColorExposure {
                exposure: exposure.id(),
                offset: offset.id(),
            },
        })],
        masks: vec![],
        markers: vec![],
        properties: vec![exposure, offset],
    }
}
fn sequence(tracks: Vec<Track>) -> Project {
    Project {
        sequences: vec![DocumentObject::Known(Sequence {
            id: SequenceId::new(),
            extent: DesignExtent::new(2.0, 2.0).unwrap(),
            frame_rate: FrameRate::new(24, 1).unwrap(),
            audio_rate: SampleRate::HZ_48000,
            working_space: ColorSpace::LinearRec709,
            tracks,
            transitions: vec![],
            markers: vec![],
            work_area: None,
            targets: None,
        })],
        ..Project::default()
    }
}
fn video(clips: Vec<Clip>) -> Track {
    Track {
        state: None,
        id: TrackId::new(),
        kind: TrackKind::Video,
        clips,
    }
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
fn render(s: &RenderSnapshot, time: Time) -> Vec<[f32; 4]> {
    render_frame(
        s,
        &[],
        &CpuReferenceBackend,
        FrameRequest {
            time,
            region: OutputRegion {
                origin: [0.0; 2],
                extent: [2.0; 2],
                pixels: [2; 2],
            },
        },
    )
    .unwrap()
    .pixels
    .linear
}
fn near(actual: [f32; 4], expected: [f32; 4]) {
    for (a, e) in actual.into_iter().zip(expected) {
        assert!((a - e).abs() < 1e-3, "{actual:?} != {expected:?}");
    }
}

#[test]
fn adjustment_rewrites_everything_composited_below_it() {
    let p = sequence(vec![
        video(vec![solid([1.0, 0.0, 0.0], Time::ZERO, t(1, 1))]),
        video(vec![adjustment(
            TimeRange::new(Time::ZERO, t(1, 1)).unwrap(),
            1.0,
        )]),
    ]);
    for px in render(&snapshot(&p), t(1, 2)) {
        near(px, [2.0, 0.0, 0.0, 1.0]);
    }
}

#[test]
fn the_adjustment_only_applies_inside_its_timeline_range() {
    let p = sequence(vec![
        video(vec![solid([1.0, 0.0, 0.0], Time::ZERO, t(1, 1))]),
        video(vec![adjustment(
            TimeRange::new(Time::ZERO, t(1, 2)).unwrap(),
            1.0,
        )]),
    ]);
    for px in render(&snapshot(&p), t(1, 4)) {
        near(px, [2.0, 0.0, 0.0, 1.0]);
    }
    for px in render(&snapshot(&p), t(3, 4)) {
        near(px, [1.0, 0.0, 0.0, 1.0]);
    }
}

#[test]
fn clips_above_the_adjustment_keep_their_color() {
    // Green on the top track for the first half; red below for the whole
    // duration; the adjustment sits between them.
    let p = sequence(vec![
        video(vec![solid([1.0, 0.0, 0.0], Time::ZERO, t(1, 1))]),
        video(vec![adjustment(
            TimeRange::new(Time::ZERO, t(1, 1)).unwrap(),
            1.0,
        )]),
        video(vec![solid([0.0, 1.0, 0.0], Time::ZERO, t(1, 2))]),
    ]);
    for px in render(&snapshot(&p), t(1, 4)) {
        near(px, [0.0, 1.0, 0.0, 1.0]);
    }
    for px in render(&snapshot(&p), t(3, 4)) {
        near(px, [2.0, 0.0, 0.0, 1.0]);
    }
}

#[test]
fn stacked_adjustments_compose_in_track_order() {
    // +1 EV then +1 EV over the lower composite: red -> 2x -> 4x.
    let p = sequence(vec![
        video(vec![solid([0.25, 0.0, 0.0], Time::ZERO, t(1, 1))]),
        video(vec![adjustment(
            TimeRange::new(Time::ZERO, t(1, 1)).unwrap(),
            1.0,
        )]),
        video(vec![adjustment(
            TimeRange::new(Time::ZERO, t(1, 1)).unwrap(),
            1.0,
        )]),
    ]);
    for px in render(&snapshot(&p), t(1, 2)) {
        near(px, [1.0, 0.0, 0.0, 1.0]);
    }
}

#[test]
fn adjustment_masks_scope_where_the_effect_applies() {
    let mut adj = adjustment(TimeRange::new(Time::ZERO, t(1, 1)).unwrap(), 1.0);
    let path = prop(
        MASK_PATH_KEY,
        PropertySource::Constant(Value::Path(Path {
            segments: vec![
                PathSegment::MoveTo([f(0.0), f(0.0)]),
                PathSegment::LineTo([f(1.0), f(0.0)]),
                PathSegment::LineTo([f(1.0), f(2.0)]),
                PathSegment::LineTo([f(0.0), f(2.0)]),
                PathSegment::Close,
            ],
        })),
    );
    let feather = prop(MASK_FEATHER_KEY, PropertySource::Constant(scalar(0.0)));
    let expansion = prop(MASK_EXPANSION_KEY, PropertySource::Constant(scalar(0.0)));
    let opacity = prop(MASK_OPACITY_KEY, PropertySource::Constant(scalar(1.0)));
    adj.masks = vec![Mask {
        id: MaskId::new(),
        path: path.id(),
        mode: MaskMode::Add,
        feather: feather.id(),
        expansion: expansion.id(),
        opacity: opacity.id(),
        invert: false,
        closed: true,
    }];
    adj.properties.extend([path, feather, expansion, opacity]);
    let p = sequence(vec![
        video(vec![solid([1.0, 0.0, 0.0], Time::ZERO, t(1, 1))]),
        video(vec![adj]),
    ]);
    let pixels = render(&snapshot(&p), t(1, 2));
    // Row-major pixels: left column adjusted, right column untouched.
    near(pixels[0], [2.0, 0.0, 0.0, 1.0]);
    near(pixels[2], [2.0, 0.0, 0.0, 1.0]);
    near(pixels[1], [1.0, 0.0, 0.0, 1.0]);
    near(pixels[3], [1.0, 0.0, 0.0, 1.0]);
}

#[test]
fn an_adjustment_over_nothing_draws_nothing() {
    let p = sequence(vec![video(vec![adjustment(
        TimeRange::new(Time::ZERO, t(1, 1)).unwrap(),
        1.0,
    )])]);
    for px in render(&snapshot(&p), t(1, 2)) {
        near(px, [0.0; 4]);
    }
}

#[test]
fn adjustment_clips_on_invalid_tracks_or_with_retime_fail_typed() {
    // Audio track placement is a typed validation error, not silent output.
    let mut bad_track = adjustment(TimeRange::new(Time::ZERO, t(1, 1)).unwrap(), 1.0);
    bad_track.source_ref = SourceRef::Adjustment;
    let p = Project {
        sequences: vec![DocumentObject::Known(Sequence {
            id: SequenceId::new(),
            extent: DesignExtent::new(2.0, 2.0).unwrap(),
            frame_rate: FrameRate::new(24, 1).unwrap(),
            audio_rate: SampleRate::HZ_48000,
            working_space: ColorSpace::LinearRec709,
            tracks: vec![Track {
                state: None,
                id: TrackId::new(),
                kind: TrackKind::Audio,
                clips: vec![bad_track],
            }],
            transitions: vec![],
            markers: vec![],
            work_area: None,
            targets: None,
        })],
        ..Project::default()
    };
    let DocumentObject::Known(s) = &p.sequences[0] else {
        panic!()
    };
    let error = RenderSnapshot::for_target(
        &p,
        RenderTarget::Sequence { sequence: s.id },
        7,
        RenderProfile::default(),
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("adjustment"),
        "unexpected error: {error}"
    );
}
