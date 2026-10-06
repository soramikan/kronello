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
        audio_retime: AudioRetimePolicy::Reject,
        volume: Some(Box::new(volume(PropertySource::Constant(scalar(0.5))))),
        links: vec![],
        properties: vec![],
        effects: vec![],
    };
    let s = Sequence {
        id: SequenceId::new(),
        extent: DesignExtent::new(8.0, 8.0).unwrap(),
        frame_rate: FrameRate::new(30000, 1001).unwrap(),
        audio_rate: SampleRate::HZ_48000,
        working_space: ColorSpace::LinearRec709,
        tracks: vec![Track {
            id: TrackId::new(),
            kind: TrackKind::Video,
            clips: vec![clip],
        }],
        transitions: vec![],
    };
    let id = s.id;
    p.sequences.push(DocumentObject::Known(s));
    id
}
#[test]
fn nested_placements_trim_negative_grid_and_request_order_are_independent() {
    let (mut p, child, asset) = fixture();
    let DocumentObject::Known(c) = &p.compositions[0] else {
        panic!()
    };
    let mut parent = c.clone();
    parent.id = CompositionId::new();
    parent.nodes.clear();
    parent.root_nodes.clear();
    // Same definition at two distinct placements; source and gain contexts stay separate.
    for offset in [t(0, 1), t(-1, 20)] {
        let n = node(
            NodeKind::CompositionInstance(CompositionInstance {
                id: CompositionInstanceId::new(),
                definition_ref: child,
                input_bindings: Default::default(),
                local_time_map: TimeMap::linear(offset, Time::ONE).unwrap(),
                seed: 0,
            }),
            r(Time::ZERO, t(1, 5)),
            vec![],
        );
        parent.root_nodes.push(n.id);
        parent.nodes.push(n);
    }
    let root = parent.id;
    p.compositions.push(DocumentObject::Known(parent));
    let seq = sequence(&mut p, root, t(-1, 20), t(3, 20));
    let plan = DocumentAudioPlan::compile(&p, AudioTarget::Sequence(seq)).unwrap();
    assert_eq!(plan.clips().len(), 2);
    let src = sources(asset);
    let range = r(t(-1, 20), t(3, 20));
    let full = plan.mix(&src, range).unwrap();
    let first = plan.mix(&src, r(range.start(), Time::ZERO)).unwrap();
    let second = plan.mix(&src, r(Time::ZERO, range.end())).unwrap();
    let joined: Vec<_> = first
        .buffer()
        .frames()
        .iter()
        .chain(second.buffer().frames())
        .copied()
        .collect();
    assert_eq!(full.buffer().frames(), joined);
    // At sequence sample -2400, first instance's local time is .01: source .02, gain .25.
    assert_eq!(full.buffer().frames()[0], [960.0 / 100000.0 * 0.25; 2]);
    let before = full.buffer().frames()[2400];
    let DocumentObject::Known(s) = &mut p.sequences[0] else {
        panic!()
    };
    s.tracks[0].clips[0] = s.tracks[0].clips[0]
        .trimmed(r(Time::ZERO, t(1, 10)))
        .unwrap();
    let trimmed = DocumentAudioPlan::compile(&p, AudioTarget::Sequence(seq))
        .unwrap()
        .mix(&src, r(Time::ZERO, t(1, 10)))
        .unwrap();
    assert_eq!(trimmed.buffer().frames()[0], before);
    assert_eq!(
        trimmed.buffer().frames(),
        &full.buffer().frames()[2400..7200]
    );
}
#[test]
fn volume_curves_use_source_time_and_reject_negative_evaluated_gain() {
    let (mut p, root, asset) = fixture();
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
    let id = curve.id();
    p.curves.push(DocumentObject::Known(curve));
    let seq = sequence(&mut p, root, Time::ZERO, t(1, 10));
    let DocumentObject::Known(s) = &mut p.sequences[0] else {
        panic!()
    };
    s.tracks[0].clips[0].volume = Some(Box::new(volume(PropertySource::Curve(id))));
    let plan = DocumentAudioPlan::compile(&p, AudioTarget::Sequence(seq)).unwrap();
    let src = sources(asset);
    let bus = plan.mix(&src, r(Time::ZERO, t(1, 20))).unwrap();
    assert!((bus.buffer().frames()[0][0] - 960.0 / 100000.0 * 0.5 * 0.1).abs() < 1e-9);
    let curve = AnimationCurve::try_from(CurveDefinition {
        id,
        value_type: ValueType::Scalar,
        interpolation_version: INTERPOLATION_VERSION,
        keys: vec![Keyframe {
            time: Time::ZERO,
            value: scalar(-0.1),
            interpolation: CurveInterpolation::Hold,
        }],
    })
    .unwrap();
    p.curves[0] = DocumentObject::Known(curve);
    assert_eq!(plan.mix(&src, r(Time::ZERO, t(1, 20))).unwrap(), bus);
    assert_eq!(
        DocumentAudioPlan::compile(&p, AudioTarget::Sequence(seq))
            .unwrap()
            .mix(&src, r(Time::ZERO, t(1, 20)))
            .unwrap_err()
            .code(),
        "INVALID_AUDIO_INPUT"
    );
}
#[test]
fn retime_effect_generator_missing_assets_and_recursive_audio_fail_typed() {
    let (mut p, root, _) = fixture();
    let DocumentObject::Known(c) = &mut p.compositions[0] else {
        panic!()
    };
    let NodeKind::Media(m) = &mut c.nodes[0].kind else {
        panic!()
    };
    m.time_map = TimeMap::linear(Time::ZERO, t(2, 1)).unwrap();
    assert_eq!(
        DocumentAudioPlan::compile(&p, AudioTarget::Composition(root))
            .unwrap_err()
            .code(),
        "UNSUPPORTED_FEATURE"
    );
    m_restore(&mut p);
    p.assets.clear();
    assert_eq!(
        DocumentAudioPlan::compile(&p, AudioTarget::Composition(root))
            .unwrap_err()
            .code(),
        "ASSET_MISSING"
    );
    let (mut p, root, _) = fixture();
    let DocumentObject::Known(c) = &mut p.compositions[0] else {
        panic!()
    };
    c.nodes[0].effects.push(Effect::Opaque(
        serde_json::json!({"effect_id":"future.audio.effect"}),
    ));
    assert_eq!(
        DocumentAudioPlan::compile(&p, AudioTarget::Composition(root))
            .unwrap_err()
            .code(),
        "UNSUPPORTED_FEATURE"
    );
    let (mut p, root, _) = fixture();
    let DocumentObject::Known(c) = &mut p.compositions[0] else {
        panic!()
    };
    let recursive = node(
        NodeKind::CompositionInstance(CompositionInstance {
            id: CompositionInstanceId::new(),
            definition_ref: root,
            input_bindings: Default::default(),
            local_time_map: TimeMap::linear(Time::ZERO, Time::ONE).unwrap(),
            seed: 0,
        }),
        r(Time::ZERO, t(1, 1)),
        vec![],
    );
    c.root_nodes.push(recursive.id);
    c.nodes.push(recursive);
    assert_eq!(
        DocumentAudioPlan::compile(&p, AudioTarget::Composition(root))
            .unwrap_err()
            .code(),
        "INVALID_AUDIO_INPUT"
    );
    let (mut p, root, _) = fixture();
    let seq = sequence(&mut p, root, Time::ZERO, t(1, 10));
    let DocumentObject::Known(s) = &mut p.sequences[0] else {
        panic!()
    };
    s.tracks[0].kind = TrackKind::Audio;
    s.tracks[0].clips[0].source_ref = SourceRef::Generator {
        generator: "tone".into(),
        version: 1,
        color: Color::from_srgb8([0; 3], None),
    };
    // Shared storage now accepts audio Generators; evaluator 1 still rejects them.
    assert_eq!(
        DocumentAudioPlan::compile(&p, AudioTarget::Sequence(seq))
            .unwrap_err()
            .code(),
        "UNSUPPORTED_FEATURE"
    );
}
fn m_restore(p: &mut Project) {
    let DocumentObject::Known(c) = &mut p.compositions[0] else {
        panic!()
    };
    let NodeKind::Media(m) = &mut c.nodes[0].kind else {
        panic!()
    };
    m.time_map = TimeMap::linear(Time::ZERO, Time::ONE).unwrap();
}

#[test]
fn fractional_asset_trim_keeps_affine_sample_phase() {
    let (mut p, root, asset) = fixture();
    let seq = sequence(&mut p, root, t(1, 30000), t(1, 10));
    let DocumentObject::Known(s) = &mut p.sequences[0] else {
        panic!()
    };
    s.tracks[0].kind = TrackKind::Audio;
    s.tracks[0].clips[0].source_ref = SourceRef::Asset {
        asset,
        stream_index: 0,
    };
    // A source starting at zero on an NTSC/subsample placement must not invent
    // negative pre-roll. Source samples are pushed onto the absolute floor grid.
    s.tracks[0].clips[0].source_in = Time::ZERO;
    let src = sources(asset);
    let original = DocumentAudioPlan::compile(&p, AudioTarget::Sequence(seq)).unwrap();
    assert_eq!(original.clips()[0].source_in, Time::ZERO);
    let requested = r(t(101, 480000), t(1, 20));
    let expected = original.mix(&src, requested).unwrap();
    let DocumentObject::Known(s) = &mut p.sequences[0] else {
        panic!()
    };
    s.tracks[0].clips[0] = s.tracks[0].clips[0].trimmed(requested).unwrap();
    let actual = DocumentAudioPlan::compile(&p, AudioTarget::Sequence(seq))
        .unwrap()
        .mix(&src, requested)
        .unwrap();
    assert_eq!(actual, expected);
}
#[test]
fn crossfade_between_audible_composition_clips_fails_typed() {
    let (mut p, root, _) = fixture();
    let seq = sequence(&mut p, root, Time::ZERO, t(1, 10));
    let DocumentObject::Known(s) = &mut p.sequences[0] else {
        panic!()
    };
    let mut incoming = s.tracks[0].clips[0].clone();
    incoming.id = ClipId::new();
    incoming.timeline_range = r(t(1, 20), t(3, 20));
    incoming.volume = None;
    s.transitions.push(Transition {
        outgoing: s.tracks[0].clips[0].id,
        incoming: incoming.id,
        range: r(t(1, 20), t(1, 10)),
        kind: TransitionKind::Crossfade,
        version: 1,
    });
    s.tracks[0].clips.push(incoming);
    let s = s.clone();
    s.validate(&p).unwrap();
    let error = DocumentAudioPlan::compile(&p, AudioTarget::Sequence(seq)).unwrap_err();
    assert_eq!(error.code(), "UNSUPPORTED_FEATURE");
    assert!(error.to_string().contains("crossfade"), "{error}");
}
#[test]
fn disabled_media_node_is_silent() {
    let (mut p, root, _) = fixture();
    let seq = sequence(&mut p, root, Time::ZERO, t(1, 10));
    let enabled = DocumentAudioPlan::compile(&p, AudioTarget::Sequence(seq)).unwrap();
    assert_eq!(enabled.clips().len(), 1);
    let DocumentObject::Known(c) = &mut p.compositions[0] else {
        panic!()
    };
    c.nodes[0].enabled = false;
    let disabled = DocumentAudioPlan::compile(&p, AudioTarget::Sequence(seq)).unwrap();
    assert!(disabled.clips().is_empty());
}
