//! FX-003 transition lowering: wipe reveal, slide translation, dip underlay
//! (ADR-0109). Pixel truth comes from the CPU reference backend; the scene IR
//! and DAG structure are asserted separately.
use kronello_gpu::render_adapter::CpuReferenceBackend;
use kronello_model::*;
use kronello_render::*;
use kronello_time::{FrameRate, Rational, SampleRate, Time, TimeMap, TimeRange};

fn t(n: i64, d: i64) -> Time {
    Time::new(n, d).unwrap()
}
fn clip(start_num: i64, start_den: i64, end_num: i64, end_den: i64, rgb: [f64; 3]) -> Clip {
    Clip {
        id: ClipId::new(),
        source_ref: SourceRef::Generator {
            generator: SOLID_GENERATOR_ID.into(),
            version: 1,
            color: Color::new(ColorSpace::LinearRec709, rgb, 1.0).unwrap(),
        },
        timeline_range: TimeRange::new(t(start_num, start_den), t(end_num, end_den)).unwrap(),
        source_in: Time::ZERO,
        time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
        audio_retime: Default::default(),
        reverse_sampling: None,
        volume: None,
        links: vec![],
        effects: vec![],
        properties: vec![],
    }
}
/// 2x2 sequence: red covers [0, 3/2), blue covers [1, 2), overlap [1, 3/2).
fn project(
    kind: TransitionKind,
    params: Option<TransitionParams>,
    version: u32,
) -> (Project, ClipId, ClipId) {
    let outgoing = clip(0, 1, 3, 2, [1.0, 0.0, 0.0]);
    let incoming = clip(1, 1, 2, 1, [0.0, 0.0, 1.0]);
    let overlap = TimeRange::new(t(1, 1), t(3, 2)).unwrap();
    let ids = (outgoing.id, incoming.id);
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
                    clips: vec![outgoing, incoming],
                }],
                transitions: vec![Transition {
                    outgoing: ids.0,
                    incoming: ids.1,
                    range: overlap,
                    kind,
                    params,
                    version,
                }],
            })],
            ..Project::default()
        },
        ids.0,
        ids.1,
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
fn incoming_node(scene: &SceneIr, incoming: ClipId) -> &SceneNodeIr {
    scene
        .nodes
        .iter()
        .find(|n| n.key.node.as_uuid() == incoming.as_uuid())
        .expect("incoming clip node")
}
/// Pixel index order is row-major: (0,0), (1,0), (0,1), (1,1).
fn near_pixels(actual: &[[f32; 4]], expected: &[[f32; 4]]) {
    assert_eq!(actual.len(), expected.len());
    for (a, e) in actual.iter().zip(expected) {
        for (a, e) in a.iter().zip(e) {
            assert!((a - e).abs() < 1e-3, "{actual:?} != {expected:?}");
        }
    }
}
const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

#[test]
fn fx003_wipe_reveals_incoming_clip_with_hard_edge() {
    for (direction, left_column) in [
        (TransitionDirection::Left, true),
        (TransitionDirection::Right, false),
    ] {
        let (p, _, incoming) = project(
            TransitionKind::Wipe,
            Some(TransitionParams::Wipe(WipeParams { direction })),
            1,
        );
        let s = snapshot(&p);
        let scene = build_scene_ir(&s, t(5, 4), &[]).unwrap();
        // p = 1/2 over the 2x2 extent: a half-extent axis-aligned reveal.
        let [SceneTransition::Reveal { min, max }] =
            incoming_node(&scene, incoming).transitions[..]
        else {
            panic!("wipe must lower to a reveal rectangle")
        };
        if direction == TransitionDirection::Left {
            assert_eq!(min, [0.0, 0.0]);
            assert_eq!(max, [1.0, 2.0]);
        } else {
            assert_eq!(min, [1.0, 0.0]);
            assert_eq!(max, [2.0, 2.0]);
        }
        let row = [
            if left_column { BLUE } else { RED },
            if left_column { RED } else { BLUE },
        ];
        near_pixels(&render(&s, t(5, 4)), &[row[0], row[1], row[0], row[1]]);
        // Before and after the interval the composite is a clean cut.
        near_pixels(&render(&s, t(3, 4)), &[RED; 4]);
        near_pixels(&render(&s, t(3, 2)), &[BLUE; 4]);
    }
}

#[test]
fn fx003_wipe_vertical_directions_use_the_height_axis() {
    for (direction, top_visible) in [
        (TransitionDirection::Up, true),
        (TransitionDirection::Down, false),
    ] {
        let (p, _, _) = project(
            TransitionKind::Wipe,
            Some(TransitionParams::Wipe(WipeParams { direction })),
            1,
        );
        let s = snapshot(&p);
        let pixels = render(&s, t(5, 4));
        let (top, bottom) = if top_visible {
            (BLUE, RED)
        } else {
            (RED, BLUE)
        };
        near_pixels(&pixels, &[top, top, bottom, bottom]);
    }
}

#[test]
fn fx003_slide_translates_the_incoming_clip_node() {
    for (direction, offset) in [
        (TransitionDirection::Left, [-1.0, 0.0]),
        (TransitionDirection::Right, [1.0, 0.0]),
        (TransitionDirection::Up, [0.0, -1.0]),
        (TransitionDirection::Down, [0.0, 1.0]),
    ] {
        let (p, _, incoming) = project(
            TransitionKind::Slide,
            Some(TransitionParams::Slide(SlideParams { direction })),
            1,
        );
        let s = snapshot(&p);
        let scene = build_scene_ir(&s, t(5, 4), &[]).unwrap();
        // Slide is a node translation, not a mask: no transition payload.
        let node = incoming_node(&scene, incoming);
        assert!(node.transitions.is_empty());
        assert_eq!(node.world_transform.0[0][2], offset[0]);
        assert_eq!(node.world_transform.0[1][2], offset[1]);
        let pixels = render(&s, t(5, 4));
        // p = 1/2: the incoming clip covers one half; the outgoing clip keeps
        // filling the rest of the frame (a slide never masks the outgoing).
        match direction {
            TransitionDirection::Left => near_pixels(&pixels, &[BLUE, RED, BLUE, RED]),
            TransitionDirection::Right => near_pixels(&pixels, &[RED, BLUE, RED, BLUE]),
            TransitionDirection::Up => near_pixels(&pixels, &[BLUE, BLUE, RED, RED]),
            TransitionDirection::Down => near_pixels(&pixels, &[RED, RED, BLUE, BLUE]),
        }
    }
}

#[test]
fn fx003_dip_crossfades_through_the_midpoint_color() {
    let (p, _, _) = project(
        TransitionKind::Dip,
        Some(TransitionParams::Dip(DipParams {
            color: Color::new(ColorSpace::LinearRec709, [0.0; 3], 1.0).unwrap(),
        })),
        1,
    );
    let s = snapshot(&p);
    // p = 1/4: half-faded dip color over the still-opaque outgoing clip.
    near_pixels(&render(&s, t(9, 8)), &[[0.5, 0.0, 0.0, 1.0]; 4]);
    // p = 1/2: the dip color is fully opaque and the incoming clip is absent.
    near_pixels(&render(&s, t(5, 4)), &[[0.0, 0.0, 0.0, 1.0]; 4]);
    // p = 3/4: the incoming clip fades in over the fully opaque dip color.
    near_pixels(&render(&s, t(11, 8)), &[[0.0, 0.0, 0.5, 1.0]; 4]);
    // The DAG lowers the dip as a solid underlay composited below the clip.
    let scene = build_scene_ir(&s, t(5, 4), &[]).unwrap();
    let dag = build_render_dag(&scene, RenderProfile::default(), region()).unwrap();
    assert!(
        dag.nodes()
            .iter()
            .any(|n| matches!(n, DagNode::SolidRect { .. })),
        "dip requires a solid underlay node"
    );
}

#[test]
fn fx003_transition_versions_and_params_are_typed_failures() {
    // Kind/params mismatches are rejected by sequence validation during
    // snapshot creation.
    let (p, _, _) = project(TransitionKind::Wipe, None, 1);
    let DocumentObject::Known(sequence) = &p.sequences[0] else {
        panic!()
    };
    let target = RenderTarget::Sequence {
        sequence: sequence.id,
    };
    let result = RenderSnapshot::for_target(&p, target, 7, RenderProfile::default());
    assert!(matches!(
        result,
        Err(RenderError::Sequence(SequenceError::Invalid(_)))
    ));
    // An unsupported transition version is a typed failure during snapshot
    // creation, not a silent fallback.
    let (p, _, _) = project(TransitionKind::Crossfade, None, 2);
    let DocumentObject::Known(sequence) = &p.sequences[0] else {
        panic!()
    };
    let result = RenderSnapshot::for_target(
        &p,
        RenderTarget::Sequence {
            sequence: sequence.id,
        },
        7,
        RenderProfile::default(),
    );
    assert!(matches!(result, Err(RenderError::UnsupportedFeature(_))));
}
