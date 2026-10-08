use kronello_audio::*;
use kronello_model::*;
use kronello_time::{Duration, FrameRate, SampleRate, Time, TimeMap, TimeRange};

fn t(n: i64, d: i64) -> Time {
    Time::new(n, d).unwrap()
}
fn r(a: Time, b: Time) -> TimeRange {
    TimeRange::new(a, b).unwrap()
}
fn volume(source: PropertySource<Value>) -> Property {
    let registry = SchemaRegistry::with_builtin();
    Property::new(
        PropertyId::new(),
        DescriptorRef::new(
            registry
                .lookup(&SchemaKey::new("kronello.audio.volume").unwrap())
                .unwrap(),
        ),
        source,
        vec![],
        &registry,
    )
    .unwrap()
}
fn scalar(v: f64) -> Value {
    Value::Scalar(FiniteF64::new(v).unwrap())
}
// Deserialize the legacy wire shape so new defaulted SceneNode fields do
// not require test-only struct literal changes when integration is merged.
fn node(kind: NodeKind, active_range: TimeRange, properties: Vec<Property>) -> SceneNode {
    serde_json::from_value(serde_json::json!({
        "id": NodeId::new(),
        "kind": kind,
        "properties": properties,
        "active_range": active_range,
        "child_order": [],
        "containment_parent": null,
        "transform_parent": null,
        "effects": []
    }))
    .unwrap()
}
fn fixture() -> (Project, CompositionId, AssetId) {
    let mut p: Project =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    p.shapes.clear();
    p.texts.clear();
    p.curves.clear();
    let DocumentObject::Known(c) = &mut p.compositions[0] else {
        panic!()
    };
    c.nodes.clear();
    c.root_nodes.clear();
    c.properties.clear();
    c.duration = Duration::new(t(1, 1)).unwrap();
    let asset = Asset {
        id: AssetId::new(),
        kind: AssetKind::Audio,
        content_hash: "a".repeat(64),
        locator: AssetLocator {
            relative: Some("source.wav".into()),
            absolute: None,
        },
        streams: vec![StreamMetadata {
            index: 0,
            codec: "pcm_s16le".into(),
            time_base: t(1, 48000),
            duration: Some(t(1, 1)),
            start_time: None,
            width: None,
            height: None,
            pixel_format: None,
            color_primaries: None,
            color_transfer: None,
            color_matrix: None,
            color_range: None,
        }],
    };
    let (id, aid) = (c.id, asset.id);
    let v = volume(PropertySource::Constant(scalar(0.5)));
    let node = node(
        NodeKind::Media(MediaNode {
            asset: aid,
            stream_index: 0,
            source_in: t(1, 100),
            time_map: TimeMap::linear(Time::ZERO, Time::ONE).unwrap(),
            volume: v.id(),
        }),
        r(Time::ZERO, t(1, 10)),
        vec![v],
    );
    c.root_nodes.push(node.id);
    c.nodes.push(node);
    p.assets.push(DocumentObject::Known(asset));
    (p, id, aid)
}
fn sources(asset: AssetId) -> AudioSources {
    [(
        (asset, 0),
        AudioBuffer::new((0..48000).map(|i| [i as f32 / 100000.0; 2]).collect()).unwrap(),
    )]
    .into()
}
fn sequence(p: &mut Project, root: CompositionId, start: Time, end: Time) -> SequenceId {
    let clip = Clip {
        id: ClipId::new(),
        source_ref: SourceRef::Composition { composition: root },
        timeline_range: r(start, end),
        source_in: t(1, 100),
        time_map: TimeMap::linear(Time::ZERO, Time::ONE).unwrap(),
        enabled: true,
        audio_retime: AudioRetimePolicy::Reject,
        reverse_sampling: None,
        volume: Some(Box::new(volume(PropertySource::Constant(scalar(0.5))))),
        pan: None,
        links: vec![],
        properties: vec![],
        effects: vec![],
        masks: vec![],
        markers: vec![],
    };
    let s = Sequence {
        id: SequenceId::new(),
        extent: DesignExtent::new(8.0, 8.0).unwrap(),
        frame_rate: FrameRate::new(30000, 1001).unwrap(),
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
    };
    let id = s.id;
    p.sequences.push(DocumentObject::Known(s));
    id
}
fn audio_sequence(p: &mut Project, root: CompositionId, asset: AssetId) -> SequenceId {
    let id = sequence(p, root, t(1, 30000), t(1, 10));
    let c = clip_mut(p);
    c.source_ref = SourceRef::Asset {
        asset,
        stream_index: 0,
    };
    c.audio_retime = AudioRetimePolicy::ResampleV1;
    c.volume = None;
    let DocumentObject::Known(s) = &mut p.sequences[0] else {
        panic!()
    };
    s.tracks[0].kind = TrackKind::Audio;
    id
}
fn clip_mut(p: &mut Project) -> &mut Clip {
    let DocumentObject::Known(s) = &mut p.sequences[0] else {
        panic!()
    };
    &mut s.tracks[0].clips[0]
}
fn advanced(p: &Project, id: SequenceId) -> DocumentAudioPlan {
    DocumentAudioPlan::compile_version(p, AudioTarget::Sequence(id), AUDIO_EVALUATION_VERSION)
        .unwrap()
}
fn ramp_expected(time: Time) -> f32 {
    (time.numerator() as f64 / time.denominator() as f64 * 48000.0 / 100000.0) as f32
}
#[test]
fn resample_linear_pitch_source_range_and_fractional_trim_stretch() {
    let (mut p, root, asset) = fixture();
    let id = audio_sequence(&mut p, root, asset);
    let src = sources(asset);
    for speed in [t(1, 2), t(2, 1)] {
        clip_mut(&mut p).time_map = TimeMap::linear(Time::ZERO, speed).unwrap();
        let authored = clip_mut(&mut p).clone();
        let full = advanced(&p, id).mix(&src, authored.timeline_range).unwrap();
        assert_eq!(full.start_sample(), 1);
        for index in [0, 10, 100, 1000] {
            let source = authored
                .local_time(t(full.start_sample() + index as i64, 48000))
                .unwrap();
            assert!((full.buffer().frames()[index][0] - ramp_expected(source)).abs() < 2e-8);
        }
        let trim = r(t(101, 480000), t(1, 20));
        let expected = advanced(&p, id).mix(&src, trim).unwrap();
        *clip_mut(&mut p) = authored.trimmed(trim).unwrap();
        assert_eq!(advanced(&p, id).mix(&src, trim).unwrap(), expected);
        *clip_mut(&mut p) = authored
            .stretched(r(authored.timeline_range.start(), t(1, 5)))
            .unwrap();
        let stretched = clip_mut(&mut p).clone();
        assert_eq!(
            stretched
                .local_time(stretched.timeline_range.end())
                .unwrap(),
            authored.local_time(authored.timeline_range.end()).unwrap()
        );
        let bus = advanced(&p, id)
            .mix(&src, stretched.timeline_range)
            .unwrap();
        let source = stretched
            .local_time(t(bus.start_sample() + 1000, 48000))
            .unwrap();
        assert!((bus.buffer().frames()[1000][0] - ramp_expected(source)).abs() < 2e-8);
        *clip_mut(&mut p) = authored;
    }
    // Every request validates the complete source interval; short sources are
    // never replaced by silence, even for a disjoint batch or a muted clip.
    clip_mut(&mut p).volume = Some(Box::new(volume(PropertySource::Constant(scalar(0.0)))));
    let short = [((asset, 0), AudioBuffer::new(vec![[0.0; 2]; 100]).unwrap())].into();
    assert_eq!(
        advanced(&p, id)
            .mix(&short, r(t(2, 1), t(3, 1)))
            .unwrap_err()
            .code(),
        "AUDIO_SOURCE_TOO_SHORT"
    );
    assert_eq!(
        DocumentAudioPlan::compile(&p, AudioTarget::Sequence(id))
            .unwrap_err()
            .code(),
        "UNSUPPORTED_FEATURE"
    );
}
#[test]
fn piecewise_resampling_breakpoint_and_trim_preserve_source_coordinates() {
    use kronello_time::TimeMapPoint;
    let (mut p, root, asset) = fixture();
    let id = audio_sequence(&mut p, root, asset);
    let c = clip_mut(&mut p);
    c.time_map = TimeMap::piecewise_linear(vec![
        TimeMapPoint {
            parent: Time::ZERO,
            local: Time::ZERO,
        },
        TimeMapPoint {
            parent: t(1, 100),
            local: t(1, 200),
        },
        TimeMapPoint {
            parent: t(1, 10),
            local: t(37, 200),
        },
    ])
    .unwrap();
    let authored = c.clone();
    let src = sources(asset);
    let full = advanced(&p, id).mix(&src, authored.timeline_range).unwrap();
    for sample in [1, 100, 481, 482, 500, 1000, 4799] {
        let source = if sample == 1 {
            authored
                .source_in
                .checked_add(
                    t(sample, 48000)
                        .checked_sub(authored.timeline_range.start())
                        .unwrap()
                        .checked_mul(t(1, 2))
                        .unwrap(),
                )
                .unwrap()
        } else {
            authored.local_time(t(sample, 48000)).unwrap()
        };
        assert!(
            (full.buffer().frames()[(sample - 1) as usize][0] - ramp_expected(source)).abs() < 2e-8
        );
    }
    let trim = r(t(201, 480000), t(1, 20));
    let expected = advanced(&p, id).mix(&src, trim).unwrap();
    *clip_mut(&mut p) = authored.trimmed(trim).unwrap();
    assert_eq!(advanced(&p, id).mix(&src, trim).unwrap(), expected);
    let range = r(t(3, 100), t(4, 100));
    assert_eq!(
        advanced(&p, id).mix(&src, range).unwrap().buffer().frames(),
        &full.buffer().frames()[1439..1919]
    );
}
fn gain_effect(property: &Property) -> Effect {
    Effect::Known(EffectDefinition {
        effect_id: AUDIO_GAIN_ID.into(),
        version: 1,
        parameters: EffectParameters::AudioGain {
            gain: property.id(),
        },
    })
}
#[test]
fn effects_curves_generators_are_owned_and_arbitrary_batch_order_is_exact() {
    let (mut p, root, asset) = fixture();
    let id = audio_sequence(&mut p, root, asset);
    let c = clip_mut(&mut p);
    c.timeline_range = r(Time::ZERO, t(1, 10));
    c.source_in = Time::ZERO;
    c.source_ref = SourceRef::Generator {
        generator: AUDIO_GENERATOR_TONE.into(),
        version: 1,
        color: Color::from_srgb8([0; 3], None),
    };
    let curve = AnimationCurve::try_from(CurveDefinition {
        id: CurveId::new(),
        value_type: ValueType::Scalar,
        interpolation_version: INTERPOLATION_VERSION,
        keys: vec![
            Keyframe {
                time: Time::ZERO,
                value: scalar(0.0),
                interpolation: CurveInterpolation::Linear,
            },
            Keyframe {
                time: t(1, 10),
                value: scalar(1.0),
                interpolation: CurveInterpolation::Hold,
            },
        ],
    })
    .unwrap();
    let effect = volume(PropertySource::Curve(curve.id()));
    c.effects = vec![gain_effect(&effect)];
    c.properties = vec![effect];
    p.curves.push(DocumentObject::Known(curve));
    let plan = advanced(&p, id);
    assert!(plan.clips().is_empty());
    let src = AudioSources::new();
    let range = r(Time::ZERO, t(1, 10));
    let full = plan.mix(&src, range).unwrap();
    assert_eq!(full.buffer().frames()[0], [0.0; 2]);
    let expected = (440.0 * 100.0 / 48000.0 * std::f64::consts::TAU).sin() as f32
        * 0.25
        * (100.0 / 4800.0) as f32;
    assert!((full.buffer().frames()[100][0] - expected).abs() < 1e-8);
    let second = plan.mix(&src, r(t(101, 480000), range.end())).unwrap();
    let first = plan.mix(&src, r(range.start(), t(101, 480000))).unwrap();
    let joined: Vec<_> = first
        .buffer()
        .frames()
        .iter()
        .chain(second.buffer().frames())
        .copied()
        .collect();
    assert_eq!(joined, full.buffer().frames());
    p.curves.clear();
    clip_mut(&mut p).effects.clear();
    assert_eq!(plan.mix(&src, range).unwrap(), full);
    clip_mut(&mut p).properties.clear();
    clip_mut(&mut p).source_ref = SourceRef::Generator {
        generator: AUDIO_GENERATOR_SILENCE.into(),
        version: 1,
        color: Color::from_srgb8([0; 3], None),
    };
    assert!(
        advanced(&p, id)
            .mix(&src, range)
            .unwrap()
            .buffer()
            .frames()
            .iter()
            .all(|f| *f == [0.0; 2])
    );
}
#[test]
fn audio_and_inherited_composition_crossfade_use_linear_half_open_sample_weights() {
    for inherited in [false, true] {
        let (mut p, root, asset) = fixture();
        let id = sequence(&mut p, root, Time::ZERO, t(1, 10));
        let c = clip_mut(&mut p);
        c.volume = None;
        if !inherited {
            c.source_ref = SourceRef::Asset {
                asset,
                stream_index: 0,
            };
            c.source_in = Time::ZERO;
        }
        let DocumentObject::Known(s) = &mut p.sequences[0] else {
            panic!()
        };
        if !inherited {
            s.tracks[0].kind = TrackKind::Audio;
        }
        let mut incoming = s.tracks[0].clips[0].clone();
        incoming.id = ClipId::new();
        incoming.timeline_range = r(t(10001, 480000), t(3, 20));
        incoming.volume = Some(Box::new(volume(PropertySource::Constant(scalar(0.0)))));
        s.transitions = vec![Transition {
            outgoing: s.tracks[0].clips[0].id,
            incoming: incoming.id,
            range: r(incoming.timeline_range.start(), t(1, 10)),
            kind: TransitionKind::Crossfade,
            params: None,
            version: 1,
        }];
        s.tracks[0].clips.push(incoming);
        let src = [((asset, 0), AudioBuffer::new(vec![[0.5; 2]; 48000]).unwrap())].into();
        let plan = advanced(&p, id);
        let full = plan.mix(&src, r(Time::ZERO, t(3, 20))).unwrap();
        let amplitude = if inherited { 0.25 } else { 0.5 };
        // Inherited Media ends at composition .1; the sequence source starts .01.
        assert_eq!(full.buffer().frames()[1000], [amplitude; 2]);
        assert_eq!(full.buffer().frames()[2900], [amplitude * 0.5; 2]);
        if !inherited {
            assert_eq!(
                full.buffer().frames()[4799],
                [amplitude * (1.0 / 3800.0) as f32; 2]
            );
        }
        assert_eq!(full.buffer().frames()[4800], [0.0; 2]);
        let late = plan.mix(&src, r(t(2900, 48000), t(3, 20))).unwrap();
        assert_eq!(late.buffer().frames(), &full.buffer().frames()[2900..]);
        assert_eq!(
            DocumentAudioPlan::compile(&p, AudioTarget::Sequence(id))
                .unwrap_err()
                .code(),
            "UNSUPPORTED_FEATURE"
        );
        let DocumentObject::Known(s) = &mut p.sequences[0] else {
            panic!()
        };
        s.tracks[0].clips[0].volume = Some(Box::new(volume(PropertySource::Constant(scalar(0.0)))));
        s.tracks[0].clips[1].volume = None;
        // Check the incoming side independently, rather than deriving the
        // reference from the mixer used by the export regression.
        let incoming = advanced(&p, id).mix(&src, r(Time::ZERO, t(3, 20))).unwrap();
        assert_eq!(incoming.buffer().frames()[999], [0.0; 2]);
        assert_eq!(incoming.buffer().frames()[1000], [0.0; 2]);
        assert_eq!(incoming.buffer().frames()[2900], [amplitude * 0.5; 2]);
        assert_eq!(
            incoming.buffer().frames()[4799],
            [amplitude * (3799.0 / 3800.0) as f32; 2]
        );
        assert_eq!(incoming.buffer().frames()[4800], [amplitude; 2]);
        let DocumentObject::Known(s) = &mut p.sequences[0] else {
            panic!()
        };
        s.transitions[0].version = 99;
        assert_eq!(
            DocumentAudioPlan::compile_version(&p, AudioTarget::Sequence(id), 2)
                .unwrap_err()
                .code(),
            "UNSUPPORTED_FEATURE"
        );
    }
}
#[test]
fn visual_clip_properties_preserve_inherited_audio_bits() {
    let (mut p, root, asset) = fixture();
    let id = sequence(&mut p, root, Time::ZERO, t(1, 10));
    let src = sources(asset);
    let range = r(Time::ZERO, t(1, 10));
    let expected = DocumentAudioPlan::compile(&p, AudioTarget::Sequence(id))
        .unwrap()
        .mix(&src, range)
        .unwrap();
    let registry = SchemaRegistry::with_builtin();
    let opacity = Property::new(
        PropertyId::new(),
        DescriptorRef::new(
            registry
                .lookup(&SchemaKey::new("kronello.opacity").unwrap())
                .unwrap(),
        ),
        PropertySource::Constant(scalar(0.0)),
        vec![],
        &registry,
    )
    .unwrap();
    clip_mut(&mut p).properties.push(opacity);
    // Visual opacity, even zero, does not mute or invalidate inherited audio.
    assert_eq!(advanced(&p, id).mix(&src, range).unwrap(), expected);
    assert_eq!(
        DocumentAudioPlan::compile(&p, AudioTarget::Sequence(id))
            .unwrap()
            .mix(&src, range)
            .unwrap(),
        expected
    );
}

#[test]
fn inherited_bus_work_is_budgeted_even_for_disjoint_batches() {
    let (mut p, root, asset) = fixture();
    let id = sequence(&mut p, root, Time::ZERO, t(1, 10));
    let DocumentObject::Known(s) = &mut p.sequences[0] else {
        panic!()
    };
    for _ in 0..3 {
        let mut track = s.tracks[0].clone();
        track.id = TrackId::new();
        track.clips[0].id = ClipId::new();
        s.tracks.push(track);
    }
    // Four inherited mixers would each initialize and scan a 600-second Bus,
    // although none of their placements intersects this request.
    assert_eq!(
        advanced(&p, id)
            .mix(&sources(asset), r(t(10, 1), t(610, 1)))
            .unwrap_err()
            .code(),
        "AUDIO_BUDGET_EXCEEDED"
    );
}
#[test]
fn unsupported_contracts_nonfinite_overflow_and_resource_budgets_fail_typed() {
    use kronello_time::{ProtectedMiddleMode, TimeMapPoint};
    let (mut p, root, asset) = fixture();
    let id = audio_sequence(&mut p, root, asset);
    assert_eq!(
        DocumentAudioPlan::compile_version(&p, AudioTarget::Sequence(id), 99)
            .unwrap_err()
            .code(),
        "UNSUPPORTED_FEATURE"
    );
    clip_mut(&mut p).time_map = TimeMap::protected(
        Duration::new(t(1, 1)).unwrap(),
        Duration::new(t(1, 1)).unwrap(),
        Duration::new(t(1, 10)).unwrap(),
        Duration::new(t(1, 10)).unwrap(),
        ProtectedMiddleMode::Hold,
    )
    .unwrap();
    assert_eq!(
        DocumentAudioPlan::compile_version(&p, AudioTarget::Sequence(id), 2)
            .unwrap_err()
            .code(),
        "UNSUPPORTED_FEATURE"
    );
    clip_mut(&mut p).time_map = TimeMap::piecewise_linear(
        (0..1025)
            .map(|i| TimeMapPoint {
                parent: t(i, 1024),
                local: t(i, 1024),
            })
            .collect(),
    )
    .unwrap();
    assert_eq!(
        DocumentAudioPlan::compile_version(&p, AudioTarget::Sequence(id), 2)
            .unwrap_err()
            .code(),
        "AUDIO_BUDGET_EXCEEDED"
    );
    clip_mut(&mut p).time_map = TimeMap::linear(Time::ZERO, Time::ONE).unwrap();
    clip_mut(&mut p).effects = vec![Effect::Opaque(serde_json::json!({"effect_id":"future"}))];
    assert_eq!(
        DocumentAudioPlan::compile_version(&p, AudioTarget::Sequence(id), 2)
            .unwrap_err()
            .code(),
        "UNSUPPORTED_FEATURE"
    );
    clip_mut(&mut p).effects = vec![Effect::Opaque(serde_json::json!({})); 17];
    assert_eq!(
        DocumentAudioPlan::compile_version(&p, AudioTarget::Sequence(id), 2)
            .unwrap_err()
            .code(),
        "AUDIO_BUDGET_EXCEEDED"
    );
    clip_mut(&mut p).effects.clear();
    for (generator, version) in [("future", 1), (AUDIO_GENERATOR_TONE, 99)] {
        clip_mut(&mut p).source_ref = SourceRef::Generator {
            generator: generator.into(),
            version,
            color: Color::from_srgb8([0; 3], None),
        };
        assert_eq!(
            DocumentAudioPlan::compile_version(&p, AudioTarget::Sequence(id), 2)
                .unwrap_err()
                .code(),
            "UNSUPPORTED_FEATURE"
        );
    }
    clip_mut(&mut p).source_ref = SourceRef::Generator {
        generator: AUDIO_GENERATOR_TONE.into(),
        version: 1,
        color: Color::from_srgb8([0; 3], None),
    };
    let effect = volume(PropertySource::Constant(scalar(f64::from(f32::MAX))));
    let c = clip_mut(&mut p);
    c.effects = vec![gain_effect(&effect), gain_effect(&effect)];
    c.properties = vec![effect];
    assert_eq!(
        advanced(&p, id)
            .mix(&AudioSources::new(), r(Time::ZERO, t(1, 10)))
            .unwrap_err()
            .code(),
        "AUDIO_OVERFLOW"
    );
    let c = clip_mut(&mut p);
    c.effects.clear();
    c.properties.clear();
    c.timeline_range = r(Time::ZERO, t(500, 1));
    let DocumentObject::Known(s) = &mut p.sequences[0] else {
        panic!()
    };
    let mut second = s.tracks[0].clone();
    second.id = TrackId::new();
    second.clips[0].id = ClipId::new();
    s.tracks.push(second);
    assert_eq!(
        advanced(&p, id)
            .mix(&AudioSources::new(), r(Time::ZERO, t(500, 1)))
            .unwrap_err()
            .code(),
        "AUDIO_BUDGET_EXCEEDED"
    );
    assert_eq!(
        advanced(&p, id)
            .mix(&AudioSources::new(), r(Time::ZERO, t(601, 1)))
            .unwrap_err()
            .code(),
        "AUDIO_BUDGET_EXCEEDED"
    );
    assert_eq!(
        AudioBuffer::new(vec![[f32::NAN, 0.0]]).unwrap_err().code(),
        "INVALID_AUDIO_INPUT"
    );
    assert_eq!(
        Gain::new(f32::INFINITY).unwrap_err().code(),
        "INVALID_AUDIO_INPUT"
    );
}
#[test]
fn legacy_versions_keep_existing_sample_bits() {
    let (mut p, root, asset) = fixture();
    let id = sequence(&mut p, root, t(-1, 20), t(1, 20));
    let src = sources(asset);
    let range = r(t(-1, 20), t(1, 20));
    let legacy = DocumentAudioPlan::compile(&p, AudioTarget::Sequence(id))
        .unwrap()
        .mix(&src, range)
        .unwrap();
    assert_eq!(
        DocumentAudioPlan::compile_version(&p, AudioTarget::Sequence(id), 1)
            .unwrap()
            .mix(&src, range)
            .unwrap(),
        legacy
    );
    assert_eq!(advanced(&p, id).mix(&src, range).unwrap(), legacy);
}

#[test]
fn resampling_shifts_pitch_and_requires_only_the_used_interpolation_tail() {
    let (mut p, root, asset) = fixture();
    let id = audio_sequence(&mut p, root, asset);
    let c = clip_mut(&mut p);
    c.timeline_range = r(Time::ZERO, t(1, 100));
    c.source_in = Time::ZERO;
    c.time_map = TimeMap::linear(Time::ZERO, t(2, 1)).unwrap();
    let sine = AudioBuffer::new(
        (0..2000)
            .map(|n| [(440.0 * n as f64 / 48000.0 * std::f64::consts::TAU).sin() as f32; 2])
            .collect(),
    )
    .unwrap();
    let source = [((asset, 0), sine)].into();
    let range = c.timeline_range;
    let bus = advanced(&p, id).mix(&source, range).unwrap();
    for (index, frame) in bus.buffer().frames().iter().enumerate() {
        let expected = (880.0 * index as f64 / 48000.0 * std::f64::consts::TAU).sin() as f32;
        assert!((frame[0] - expected).abs() < 1e-6);
    }
    let c = clip_mut(&mut p);
    c.timeline_range = r(Time::ZERO, t(1, 48000));
    c.time_map = TimeMap::linear(Time::ZERO, Time::ONE).unwrap();
    let one = [((asset, 0), AudioBuffer::new(vec![[0.5; 2]]).unwrap())].into();
    assert_eq!(
        advanced(&p, id)
            .mix(&one, r(Time::ZERO, t(1, 48000)))
            .unwrap()
            .buffer()
            .frames(),
        &[[0.5; 2]]
    );
    clip_mut(&mut p).source_in = t(1, 96000);
    assert_eq!(
        advanced(&p, id)
            .mix(&one, r(Time::ZERO, t(1, 48000)))
            .unwrap_err()
            .code(),
        "AUDIO_SOURCE_TOO_SHORT"
    );
    let two = [(
        (asset, 0),
        AudioBuffer::new(vec![[0.25; 2], [0.75; 2]]).unwrap(),
    )]
    .into();
    assert_eq!(
        advanced(&p, id)
            .mix(&two, r(Time::ZERO, t(1, 48000)))
            .unwrap()
            .buffer()
            .frames(),
        &[[0.5; 2]]
    );
    let c = clip_mut(&mut p);
    c.source_in = Time::ZERO;
    c.timeline_range = r(t(1, 96000), t(3, 96000));
    let range = c.timeline_range;
    assert_eq!(
        advanced(&p, id).mix(&two, range).unwrap_err().code(),
        "AUDIO_SOURCE_TOO_SHORT"
    );
}

#[test]
fn negative_effect_curve_unknown_version_and_curve_budget_are_typed_errors() {
    let (mut p, root, asset) = fixture();
    let id = audio_sequence(&mut p, root, asset);
    let curve = AnimationCurve::try_from(CurveDefinition {
        id: CurveId::new(),
        value_type: ValueType::Scalar,
        interpolation_version: 1,
        keys: vec![Keyframe {
            time: Time::ZERO,
            value: scalar(-1.0),
            interpolation: CurveInterpolation::Hold,
        }],
    })
    .unwrap();
    let property = volume(PropertySource::Curve(curve.id()));
    clip_mut(&mut p).effects = vec![gain_effect(&property)];
    clip_mut(&mut p).properties = vec![property];
    p.curves = vec![DocumentObject::Known(curve.clone())];
    let source = sources(asset);
    assert_eq!(
        advanced(&p, id)
            .mix(&source, r(Time::ZERO, t(1, 10)))
            .unwrap_err()
            .code(),
        "INVALID_AUDIO_INPUT"
    );
    let Effect::Known(effect) = &mut clip_mut(&mut p).effects[0] else {
        panic!()
    };
    effect.version = 99;
    assert_eq!(
        DocumentAudioPlan::compile_version(&p, AudioTarget::Sequence(id), 2)
            .unwrap_err()
            .code(),
        "UNSUPPORTED_FEATURE"
    );
    let Effect::Known(effect) = &mut clip_mut(&mut p).effects[0] else {
        panic!()
    };
    effect.version = 1;
    p.curves = vec![DocumentObject::Known(
        AnimationCurve::try_from(CurveDefinition {
            id: curve.id(),
            value_type: ValueType::Scalar,
            interpolation_version: 1,
            keys: (0..4097)
                .map(|n| Keyframe {
                    time: t(n, 48000),
                    value: scalar(1.0),
                    interpolation: CurveInterpolation::Hold,
                })
                .collect(),
        })
        .unwrap(),
    )];
    assert_eq!(
        DocumentAudioPlan::compile_version(&p, AudioTarget::Sequence(id), 2)
            .unwrap_err()
            .code(),
        "AUDIO_BUDGET_EXCEEDED"
    );
}

#[test]
fn fractional_breakpoint_trim_uses_the_versioned_first_segment_extension() {
    use kronello_time::TimeMapPoint;
    let (mut p, root, asset) = fixture();
    let id = audio_sequence(&mut p, root, asset);
    let c = clip_mut(&mut p);
    c.source_in = t(1, 48000);
    c.timeline_range = r(Time::ZERO, t(2, 48000));
    c.time_map = TimeMap::piecewise_linear(vec![
        TimeMapPoint {
            parent: Time::ZERO,
            local: Time::ZERO,
        },
        TimeMapPoint {
            parent: t(1, 96000),
            local: t(1, 192000),
        },
        TimeMapPoint {
            parent: t(2, 48000),
            local: t(13, 192000),
        },
    ])
    .unwrap();
    let authored = c.clone();
    let source = [(
        (asset, 0),
        AudioBuffer::new((0..8).map(|n| [n as f32; 2]).collect()).unwrap(),
    )]
    .into();
    let before = advanced(&p, id)
        .mix(&source, authored.timeline_range)
        .unwrap();
    *clip_mut(&mut p) = authored.trimmed(r(t(1, 96000), t(2, 48000))).unwrap();
    let after = advanced(&p, id)
        .mix(&source, authored.timeline_range)
        .unwrap();
    assert_eq!(before.buffer().frames()[0], [1.0; 2]);
    assert_eq!(after.buffer().frames()[0], [0.25; 2]);
    assert_eq!(after.buffer().frames()[1], before.buffer().frames()[1]);
}

#[test]
fn empty_sample_assets_and_extreme_crossfade_bounds_never_hide_errors_or_panic() {
    let (mut p, root, asset) = fixture();
    let id = audio_sequence(&mut p, root, asset);
    clip_mut(&mut p).timeline_range = r(Time::ZERO, t(1, 96000));
    assert_eq!(
        advanced(&p, id)
            .mix(&AudioSources::new(), r(Time::ZERO, t(1, 96000)))
            .unwrap_err()
            .code(),
        "ASSET_MISSING"
    );
    let c = clip_mut(&mut p);
    c.source_ref = SourceRef::Generator {
        generator: AUDIO_GENERATOR_SILENCE.into(),
        version: 1,
        color: Color::from_srgb8([0; 3], None),
    };
    c.source_in = Time::ZERO;
    c.timeline_range = r(t(-180_000_000_000_000, 1), t(180_000_000_000_000, 1));
    let DocumentObject::Known(s) = &mut p.sequences[0] else {
        panic!()
    };
    let mut incoming = s.tracks[0].clips[0].clone();
    incoming.id = ClipId::new();
    incoming.timeline_range = r(t(-179_000_000_000_000, 1), t(181_000_000_000_000, 1));
    s.transitions = vec![Transition {
        outgoing: s.tracks[0].clips[0].id,
        incoming: incoming.id,
        range: r(
            incoming.timeline_range.start(),
            s.tracks[0].clips[0].timeline_range.end(),
        ),
        kind: TransitionKind::Crossfade,
        params: None,
        version: 1,
    }];
    s.tracks[0].clips.push(incoming);
    assert_eq!(
        DocumentAudioPlan::compile_version(&p, AudioTarget::Sequence(id), 2)
            .unwrap_err()
            .code(),
        "AUDIO_OVERFLOW"
    );
    let DocumentObject::Known(s) = &mut p.sequences[0] else {
        panic!()
    };
    s.tracks[0].clips[0].timeline_range = r(Time::ZERO, t(1, 96000));
    s.tracks[0].clips[1].timeline_range = r(t(1, 192000), t(3, 192000));
    s.transitions[0].range = r(t(1, 192000), t(1, 96000));
    assert_eq!(
        DocumentAudioPlan::compile_version(&p, AudioTarget::Sequence(id), 2)
            .unwrap_err()
            .code(),
        "INVALID_AUDIO_INPUT"
    );
}

#[test]
fn gui007_reverse_ramp_samples_and_fractional_neighbors_are_exact() {
    let (mut p, root, asset) = fixture();
    let id = audio_sequence(&mut p, root, asset);
    let c = clip_mut(&mut p);
    c.timeline_range = r(Time::ZERO, t(4, 48000));
    c.source_in = t(4, 48000);
    c.time_map = TimeMap::linear(Time::ZERO, Time::ONE).unwrap();
    c.reverse_sampling = Some(ReverseSampling::ReverseGridV1);
    c.audio_retime = AudioRetimePolicy::ReverseResampleV1;
    let source = [(
        (asset, 0),
        AudioBuffer::new(vec![[0.1; 2], [0.2; 2], [0.3; 2], [0.4; 2]]).unwrap(),
    )]
    .into();
    assert_eq!(
        advanced(&p, id)
            .mix(&source, r(Time::ZERO, t(4, 48000)))
            .unwrap()
            .buffer()
            .frames(),
        &[[0.4; 2], [0.3; 2], [0.2; 2], [0.1; 2]]
    );
    let full = advanced(&p, id)
        .mix(&source, r(Time::ZERO, t(4, 48000)))
        .unwrap();
    let tail = advanced(&p, id)
        .mix(&source, r(t(2, 48000), t(4, 48000)))
        .unwrap();
    assert_eq!(tail.buffer().frames(), &full.buffer().frames()[2..]);
    let c = clip_mut(&mut p);
    c.timeline_range = r(Time::ZERO, t(1, 48000));
    c.source_in = t(5, 96000);
    let bus = advanced(&p, id)
        .mix(&source, r(Time::ZERO, t(1, 48000)))
        .unwrap();
    assert!((bus.buffer().frames()[0][0] - 0.25).abs() < 1e-7);
    clip_mut(&mut p).source_in = t(1, 96000);
    assert!(DocumentAudioPlan::compile_version(&p, AudioTarget::Sequence(id), 2).is_err());
}

#[test]
fn gui007_reverse_nested_composition_audio_matches_reversed_forward_samples() {
    let (mut p, root, asset) = fixture();
    let id = sequence(&mut p, root, Time::ZERO, t(1, 10));
    let c = clip_mut(&mut p);
    c.source_in = t(1, 10);
    c.volume = None;
    c.reverse_sampling = Some(ReverseSampling::ReverseGridV1);
    c.audio_retime = AudioRetimePolicy::ReverseResampleV1;
    let sources = sources(asset);
    let forward = DocumentAudioPlan::compile(&p, AudioTarget::Composition(root))
        .unwrap()
        .mix(&sources, r(Time::ZERO, t(1, 10)))
        .unwrap();
    let reverse = advanced(&p, id)
        .mix(&sources, r(Time::ZERO, t(1, 10)))
        .unwrap();
    let expected: Vec<_> = forward.buffer().frames().iter().rev().copied().collect();
    assert_eq!(reverse.buffer().frames(), expected);
    let slice = advanced(&p, id)
        .mix(&sources, r(t(1, 20), t(1, 10)))
        .unwrap();
    assert_eq!(slice.buffer().frames(), &expected[2400..]);
}

#[test]
fn gui007_track_mute_changes_pcm_and_preserves_authored_clip() {
    let (mut p, root, asset) = fixture();
    let id = audio_sequence(&mut p, root, asset);
    let sources = sources(asset);
    let range = r(t(1, 100), t(1, 20));
    let original = p.clone();
    let before = advanced(&p, id).mix(&sources, range).unwrap();
    assert!(before.buffer().frames().iter().any(|f| f[0] != 0.0));
    let DocumentObject::Known(sequence) = &mut p.sequences[0] else {
        panic!()
    };
    sequence.tracks[0].state = Some(TrackState {
        visible: true,
        muted: true,
        locked: false,
    });
    let after = advanced(&p, id).mix(&sources, range).unwrap();
    assert!(after.buffer().frames().iter().all(|f| *f == [0.0; 2]));
    let DocumentObject::Known(sequence) = &mut p.sequences[0] else {
        panic!()
    };
    sequence.tracks[0].state = None;
    assert_eq!(p, original);
    assert_eq!(advanced(&p, id).mix(&sources, range).unwrap(), before);
}

#[test]
fn disabled_clip_mixes_silence_while_keeping_timeline_occupancy() {
    let (mut p, root, asset) = fixture();
    let id = audio_sequence(&mut p, root, asset);
    let sources = sources(asset);
    let range = r(t(1, 100), t(1, 20));
    let before = advanced(&p, id).mix(&sources, range).unwrap();
    assert!(before.buffer().frames().iter().any(|f| f[0] != 0.0));
    // NLE-005: the placement still occupies its range but contributes nothing.
    clip_mut(&mut p).enabled = false;
    assert_eq!(clip_mut(&mut p).timeline_range, r(t(1, 30000), t(1, 10)));
    assert_eq!(
        advanced(&p, id)
            .mix(&sources, range)
            .unwrap()
            .buffer()
            .frames()
            .iter()
            .filter(|f| **f != [0.0; 2])
            .count(),
        0
    );
    clip_mut(&mut p).enabled = true;
    assert_eq!(advanced(&p, id).mix(&sources, range).unwrap(), before);
}

#[test]
fn piecewise_hold_mixes_silence_and_ramps_resample_at_segment_speed() {
    use kronello_time::TimeMapPoint;
    let (mut p, root, asset) = fixture();
    let id = audio_sequence(&mut p, root, asset);
    let clip = clip_mut(&mut p);
    // Half-speed ramp for 10 ms, a 10 ms hold, then a double-speed ramp.
    clip.time_map = TimeMap::piecewise_linear(vec![
        TimeMapPoint {
            parent: Time::ZERO,
            local: Time::ZERO,
        },
        TimeMapPoint {
            parent: t(1, 100),
            local: t(1, 200),
        },
        TimeMapPoint {
            parent: t(1, 50),
            local: t(1, 200),
        },
        TimeMapPoint {
            parent: t(1, 10),
            local: t(41, 200),
        },
    ])
    .unwrap();
    let authored = clip.clone();
    let sources = sources(asset);
    // The placement starts at 1/30000 s, so the hold covers absolute samples
    // 482..=960 and each sloped neighbor resamples at its own local rate.
    let mix = advanced(&p, id)
        .mix(&sources, authored.timeline_range)
        .unwrap();
    let frames = mix.buffer().frames();
    let start = mix.start_sample();
    for sample in [483, 700, 959] {
        assert_eq!(
            frames[(sample - start) as usize],
            [0.0; 2],
            "hold sample {sample} must be silent"
        );
    }
    for sample in [2, 240, 481, 962, 1200, 2000, 4799] {
        let source = authored.local_time(t(sample, 48000)).unwrap();
        assert!(
            (frames[(sample - start) as usize][0] - ramp_expected(source)).abs() < 2e-8,
            "sloped sample {sample}"
        );
    }
}
