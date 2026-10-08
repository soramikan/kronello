//! FX-003 blend mode set and parameterized transition contracts (ADR-0109).
use kronello_model::*;
use kronello_time::{FrameRate, Rational, SampleRate, Time, TimeMap, TimeRange};

fn t(n: i64, d: i64) -> Time {
    Time::new(n, d).unwrap()
}
fn blend_property(source: PropertySource<Value>) -> Property {
    let registry = SchemaRegistry::with_builtin();
    let descriptor = registry
        .lookup(&SchemaKey::new(BLEND_KEY).unwrap())
        .unwrap();
    Property::new(
        PropertyId::new(),
        DescriptorRef::new(descriptor),
        source,
        vec![],
        &registry,
    )
    .unwrap()
}
fn clip(start: i64, end: i64) -> Clip {
    Clip {
        id: ClipId::new(),
        source_ref: SourceRef::Generator {
            generator: SOLID_GENERATOR_ID.into(),
            version: 1,
            color: Color::from_srgb8([128; 3], None),
        },
        timeline_range: TimeRange::new(t(start, 1), t(end, 1)).unwrap(),
        source_in: Time::ZERO,
        time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
        enabled: true,
        audio_retime: Default::default(),
        reverse_sampling: None,
        volume: None,
        pan: None,
        links: vec![],
        effects: vec![],
        masks: vec![],
        markers: vec![],
        properties: vec![],
    }
}
/// Two generator clips overlapping on [1, 3/2) inside one video track.
fn overlapping_sequence() -> (Sequence, Project, ClipId, ClipId) {
    let mut outgoing = clip(0, 2);
    outgoing.timeline_range = TimeRange::new(t(0, 1), t(3, 2)).unwrap();
    let incoming = clip(1, 2);
    let sequence = Sequence {
        id: SequenceId::new(),
        extent: DesignExtent::new(4.0, 4.0).unwrap(),
        frame_rate: FrameRate::new(24, 1).unwrap(),
        audio_rate: SampleRate::HZ_48000,
        working_space: ColorSpace::LinearRec709,
        tracks: vec![Track {
            state: None,
            id: TrackId::new(),
            kind: TrackKind::Video,
            clips: vec![outgoing.clone(), incoming.clone()],
        }],
        transitions: vec![],
        markers: vec![],
        work_area: None,
        targets: None,
    };
    (sequence, Project::default(), outgoing.id, incoming.id)
}

#[test]
fn fx003_blend_mode_wire_names_cover_the_closed_w3c_set() {
    // The wire names are the stable public contract; serde and from_value
    // must agree on all twenty modes (3 pre-existing + 17 added).
    let cases: [(&str, BlendMode); 20] = [
        ("normal", BlendMode::Normal),
        ("multiply", BlendMode::Multiply),
        ("screen", BlendMode::Screen),
        ("overlay", BlendMode::Overlay),
        ("darken", BlendMode::Darken),
        ("lighten", BlendMode::Lighten),
        ("color_dodge", BlendMode::ColorDodge),
        ("color_burn", BlendMode::ColorBurn),
        ("hard_light", BlendMode::HardLight),
        ("soft_light", BlendMode::SoftLight),
        ("difference", BlendMode::Difference),
        ("exclusion", BlendMode::Exclusion),
        ("linear_dodge", BlendMode::LinearDodge),
        ("linear_burn", BlendMode::LinearBurn),
        ("vivid_light", BlendMode::VividLight),
        ("linear_light", BlendMode::LinearLight),
        ("hue", BlendMode::Hue),
        ("saturation", BlendMode::Saturation),
        ("color", BlendMode::Color),
        ("luminosity", BlendMode::Luminosity),
    ];
    for (name, mode) in cases {
        assert_eq!(
            BlendMode::from_value(&Value::Enum(name.into())).unwrap(),
            mode,
            "{name}"
        );
        assert_eq!(serde_json::to_value(mode).unwrap(), name);
        let round_trip: BlendMode = serde_json::from_value(name.into()).unwrap();
        assert_eq!(round_trip, mode);
    }
    for rejected in [
        Value::Enum("phoenix".into()),
        Value::Enum("Overlay".into()),
        Value::Enum("".into()),
        Value::Scalar(FiniteF64::new(1.0).unwrap()),
    ] {
        assert!(matches!(
            BlendMode::from_value(&rejected),
            Err(ModelError::InvalidBlendMode)
        ));
    }
}

#[test]
fn fx003_blend_property_stays_single_constant_and_authored() {
    let single = [blend_property(PropertySource::Constant(Value::Enum(
        "vivid_light".into(),
    )))];
    assert_eq!(
        BlendMode::from_properties(&single).unwrap(),
        BlendMode::VividLight
    );
    // A second authored blend property on one layer is a typed rejection.
    let duplicated = [
        blend_property(PropertySource::Constant(Value::Enum("normal".into()))),
        blend_property(PropertySource::Constant(Value::Enum("hue".into()))),
    ];
    assert!(matches!(
        BlendMode::from_properties(&duplicated),
        Err(ModelError::InvalidBlendMode)
    ));
    // The descriptor itself rejects non-constant sources and unknown names.
    assert!(
        Property::new(
            PropertyId::new(),
            DescriptorRef::new(
                SchemaRegistry::with_builtin()
                    .lookup(&SchemaKey::new(BLEND_KEY).unwrap())
                    .unwrap()
            ),
            PropertySource::Curve(CurveId::new()),
            vec![],
            &SchemaRegistry::with_builtin(),
        )
        .is_err()
    );
    // Absence remains the legacy source-over mode.
    assert_eq!(BlendMode::from_properties(&[]).unwrap(), BlendMode::Normal);
}

#[test]
fn fx003_transition_serde_defaults_and_closed_sets() {
    // Legacy documents without `kind`/`params` stay deserializable? No: kind
    // is required going forward, but `params` defaults to absent for the
    // pre-FX-003 crossfade shape (ADR-0109).
    let (mut sequence, project, outgoing, incoming) = overlapping_sequence();
    sequence.transitions.push(Transition {
        outgoing,
        incoming,
        range: TimeRange::new(t(1, 1), t(3, 2)).unwrap(),
        kind: TransitionKind::Crossfade,
        params: None,
        version: 1,
    });
    sequence.validate(&project).unwrap();
    let raw = serde_json::to_value(&sequence.transitions[0]).unwrap();
    assert!(!raw.as_object().unwrap().contains_key("params"));
    let decoded: Transition = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(decoded, sequence.transitions[0]);
    assert_eq!(decoded.params, None);
    // Unknown kinds and directions fail serde, not validation.
    let mut bad_kind = raw.clone();
    bad_kind["kind"] = serde_json::json!("warp");
    assert!(serde_json::from_value::<Transition>(bad_kind).is_err());
    let mut bad_direction = raw.clone();
    bad_direction["kind"] = serde_json::json!("wipe");
    bad_direction["params"] = serde_json::json!({"wipe": {"direction": "diagonal"}});
    assert!(serde_json::from_value::<Transition>(bad_direction).is_err());
    // deny_unknown_fields protects the closed payload shapes.
    let mut extra = raw.clone();
    extra["surprise"] = serde_json::json!(true);
    assert!(serde_json::from_value::<Transition>(extra).is_err());
    let mut extra_param = raw.clone();
    extra_param["kind"] = serde_json::json!("wipe");
    extra_param["params"] = serde_json::json!({"wipe": {"direction": "left", "feather": 1.0}});
    assert!(serde_json::from_value::<Transition>(extra_param).is_err());
}

#[test]
fn fx003_transition_kind_params_pairing_is_validated() {
    let (mut sequence, project, _, _) = overlapping_sequence();
    let overlap = TimeRange::new(t(1, 1), t(3, 2)).unwrap();
    let wipe = TransitionParams::Wipe(WipeParams {
        direction: TransitionDirection::Right,
    });
    let slide = TransitionParams::Slide(SlideParams {
        direction: TransitionDirection::Up,
    });
    let dip = TransitionParams::Dip(DipParams {
        color: Color::from_srgb8([0; 3], None),
    });
    // Every documented kind/params combination validates.
    for (kind, params) in [
        (TransitionKind::Crossfade, None),
        (TransitionKind::Wipe, Some(wipe)),
        (TransitionKind::Slide, Some(slide)),
        (TransitionKind::Dip, Some(dip)),
    ] {
        sequence.transitions = vec![Transition {
            outgoing: sequence.tracks[0].clips[0].id,
            incoming: sequence.tracks[0].clips[1].id,
            range: overlap,
            kind,
            params,
            version: 1,
        }];
        sequence.validate(&project).unwrap();
    }
    // Mismatched or missing payloads are typed structural errors.
    for (kind, params) in [
        (TransitionKind::Wipe, None),
        (TransitionKind::Slide, None),
        (TransitionKind::Dip, None),
        (TransitionKind::Crossfade, Some(wipe)),
        (TransitionKind::Wipe, Some(slide)),
        (TransitionKind::Slide, Some(dip)),
        (TransitionKind::Dip, Some(wipe)),
    ] {
        sequence.transitions = vec![Transition {
            outgoing: sequence.tracks[0].clips[0].id,
            incoming: sequence.tracks[0].clips[1].id,
            range: overlap,
            kind,
            params,
            version: 1,
        }];
        assert!(matches!(
            sequence.validate(&project),
            Err(SequenceError::Invalid(_))
        ));
    }
}

#[test]
fn fx003_transition_overlap_rules_apply_to_every_kind() {
    let (mut sequence, project, _, _) = overlapping_sequence();
    let overlap = TimeRange::new(t(1, 1), t(3, 2)).unwrap();
    // A transition whose range does not equal the actual clip intersection is
    // structurally invalid for every kind, not just crossfade.
    for (kind, params) in [
        (TransitionKind::Crossfade, None),
        (
            TransitionKind::Dip,
            Some(TransitionParams::Dip(DipParams {
                color: Color::from_srgb8([0; 3], None),
            })),
        ),
    ] {
        let mut s = sequence.clone();
        s.tracks[0].clips[0].timeline_range = TimeRange::new(t(0, 1), t(2, 1)).unwrap();
        s.transitions = vec![Transition {
            outgoing: s.tracks[0].clips[0].id,
            incoming: s.tracks[0].clips[1].id,
            range: overlap,
            kind,
            params,
            version: 1,
        }];
        assert!(matches!(
            s.validate(&project),
            Err(SequenceError::Invalid(_))
        ));
    }
    // An undeclared overlap elsewhere on the track is a CLIP_OVERLAP even when
    // a valid non-crossfade transition exists between another pair.
    let mut blocker = clip(0, 3);
    blocker.timeline_range = TimeRange::new(t(7, 4), t(9, 4)).unwrap();
    sequence.tracks[0].clips.push(blocker);
    sequence.transitions = vec![Transition {
        outgoing: sequence.tracks[0].clips[0].id,
        incoming: sequence.tracks[0].clips[1].id,
        range: overlap,
        kind: TransitionKind::Dip,
        params: Some(TransitionParams::Dip(DipParams {
            color: Color::from_srgb8([0; 3], None),
        })),
        version: 1,
    }];
    assert!(matches!(
        sequence.validate(&project),
        Err(SequenceError::Overlap(_))
    ));
}
