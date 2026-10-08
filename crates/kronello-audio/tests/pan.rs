//! GUI-012 (ADR-0138): authored constant stereo balance `kronello.audio.pan`.
//! The version-2 evaluator applies constant-power gains after clip gain and
//! fades; the version-1 basic plan rejects pan with a typed error.
use kronello_audio::*;
use kronello_model::*;
use kronello_time::{FrameRate, SampleRate, Time, TimeMap, TimeRange};

fn t(n: i64, d: i64) -> Time {
    Time::new(n, d).unwrap()
}
fn r(a: Time, b: Time) -> TimeRange {
    TimeRange::new(a, b).unwrap()
}
fn scalar(v: f64) -> Value {
    Value::Scalar(FiniteF64::new(v).unwrap())
}
fn property(key: &str, source: PropertySource<Value>) -> Property {
    let registry = SchemaRegistry::with_builtin();
    Property::new(
        PropertyId::new(),
        DescriptorRef::new(registry.lookup(&SchemaKey::new(key).unwrap()).unwrap()),
        source,
        vec![],
        &registry,
    )
    .unwrap()
}
fn pan(v: f64) -> Property {
    property("kronello.audio.pan", PropertySource::Constant(scalar(v)))
}
fn fixture(pan_value: Option<f64>) -> (Project, SequenceId, AssetId) {
    let asset = Asset {
        id: AssetId::new(),
        kind: AssetKind::Audio,
        content_hash: "b".repeat(64),
        locator: AssetLocator {
            relative: Some("pan.wav".into()),
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
    let (sequence, aid) = (SequenceId::new(), asset.id);
    let mut p = Project::default();
    p.assets.push(DocumentObject::Known(asset));
    p.sequences.push(DocumentObject::Known(Sequence {
        id: sequence,
        extent: DesignExtent::new(8.0, 8.0).unwrap(),
        frame_rate: FrameRate::new(24, 1).unwrap(),
        audio_rate: SampleRate::HZ_48000,
        working_space: ColorSpace::LinearRec709,
        tracks: vec![Track {
            state: None,
            id: TrackId::new(),
            kind: TrackKind::Audio,
            clips: vec![Clip {
                id: ClipId::new(),
                source_ref: SourceRef::Asset {
                    asset: aid,
                    stream_index: 0,
                },
                timeline_range: r(Time::ZERO, t(1, 1)),
                source_in: Time::ZERO,
                time_map: TimeMap::linear(Time::ZERO, Time::ONE).unwrap(),
                enabled: true,
                audio_retime: AudioRetimePolicy::Reject,
                reverse_sampling: None,
                volume: None,
                pan: pan_value.map(|v| Box::new(pan(v))),
                links: vec![],
                effects: vec![],
                masks: vec![],
                properties: vec![],
                markers: vec![],
            }],
        }],
        transitions: vec![],
        markers: vec![],
        work_area: None,
        targets: None,
    }));
    (p, sequence, aid)
}
fn sources(asset: AssetId) -> AudioSources {
    [(
        (asset, 0),
        // Constant full-scale-ish stereo content makes channel ratios exact.
        AudioBuffer::new((0..48000).map(|_| [0.25, 0.25]).collect()).unwrap(),
    )]
    .into()
}
fn clip_mut(p: &mut Project) -> &mut Clip {
    let DocumentObject::Known(s) = &mut p.sequences[0] else {
        panic!()
    };
    &mut s.tracks[0].clips[0]
}
fn mix(project: &Project, id: SequenceId, src: &AudioSources) -> Bus {
    DocumentAudioPlan::compile_version(project, AudioTarget::Sequence(id), 2)
        .unwrap()
        .mix(src, r(Time::ZERO, t(1, 1)))
        .unwrap()
}

#[test]
fn pan_constant_power_moves_the_stereo_image() {
    let (mut p, id, aid) = fixture(None);
    let src = sources(aid);
    let mut frame = |pan_value: Option<f64>| {
        clip_mut(&mut p).pan = pan_value.map(|v| Box::new(pan(v)));
        mix(&p, id, &src).buffer().frames()[100]
    };
    // Hard left: full left gain, silent right. Hard right: the mirror.
    // cos(pi/2) is ~-4.4e-8 rather than exact zero, so compare with epsilon.
    let left = frame(Some(-1.0));
    let right = frame(Some(1.0));
    assert_eq!(left[0], 0.25);
    assert!(left[1].abs() < 1e-6, "{left:?}");
    assert!(right[0].abs() < 1e-6, "{right:?}");
    assert_eq!(right[1], 0.25);
    // Center is the -3 dB constant-power point: 0.25 * cos(pi/4).
    let expected = 0.25 * std::f32::consts::FRAC_1_SQRT_2;
    let center = frame(Some(0.0));
    assert!((center[0] - expected).abs() < 1e-6, "{center:?}");
    assert!((center[1] - expected).abs() < 1e-6, "{center:?}");
    // Absent pan leaves the mix untouched.
    assert_eq!(frame(None), [0.25, 0.25]);
}

#[test]
fn pan_validation_is_constant_only_and_bounded() {
    // Curves are rejected by the descriptor capabilities at construction.
    let registry = SchemaRegistry::with_builtin();
    assert!(
        Property::new(
            PropertyId::new(),
            DescriptorRef::new(
                registry
                    .lookup(&SchemaKey::new("kronello.audio.pan").unwrap())
                    .unwrap()
            ),
            PropertySource::Curve(CurveId::new()),
            vec![],
            &registry,
        )
        .is_err()
    );
    // Out-of-range constants are rejected by the descriptor range.
    assert!(
        Property::new(
            PropertyId::new(),
            DescriptorRef::new(
                registry
                    .lookup(&SchemaKey::new("kronello.audio.pan").unwrap())
                    .unwrap()
            ),
            PropertySource::Constant(scalar(1.5)),
            vec![],
            &registry,
        )
        .is_err()
    );
    // A wrong descriptor key is rejected by the shared clip contract.
    let volume = property(
        "kronello.audio.volume",
        PropertySource::Constant(scalar(0.5)),
    );
    assert_eq!(validate_pan(&volume), Err(ModelError::SourceNotAllowed));
    validate_pan(&pan(-1.0)).unwrap();
    validate_pan(&pan(1.0)).unwrap();
}

#[test]
fn pan_requires_the_version_2_evaluator() {
    let (p, id, _) = fixture(Some(0.5));
    let err = DocumentAudioPlan::compile(&p, AudioTarget::Sequence(id)).unwrap_err();
    assert_eq!(err.code(), "UNSUPPORTED_FEATURE");
    assert!(err.to_string().contains("AUDIO-004"), "{err}");
    // Compile-time validation also runs on the version-2 path: a wrong-key
    // property stored by an unchecked writer is still a typed error.
    let (mut p, id, _) = fixture(Some(0.5));
    let DocumentObject::Known(s) = &mut p.sequences[0] else {
        panic!()
    };
    s.tracks[0].clips[0].pan = Some(Box::new(property(
        "kronello.audio.volume",
        PropertySource::Constant(scalar(0.0)),
    )));
    let err = DocumentAudioPlan::compile_version(&p, AudioTarget::Sequence(id), 2).unwrap_err();
    assert!(
        err.to_string().contains("not allowed by this descriptor"),
        "{err}"
    );
}
