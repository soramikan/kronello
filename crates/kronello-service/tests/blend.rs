use kronello_model::*;
use kronello_render::{OutputRegion, RenderSnapshot, RenderTarget};
use kronello_service::*;
use kronello_time::{FrameRate, Rational, SampleRate, Time, TimeMap, TimeRange};
use serde_json::json;
use uuid::Uuid;

fn run(request: serde_json::Value) -> Result<ResultData, ServiceError> {
    Service::new(BackendSelection::CpuReference).dispatch(serde_json::from_value(request).unwrap())
}
fn export(path: &std::path::Path) -> ExportResult {
    let ResultData::Export(r) = run(json!({"operation":"project.export","project":path})).unwrap()
    else {
        panic!()
    };
    *r
}
fn property(key: &str, value: Value) -> Property {
    let registry = kronello_render::render_registry();
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
fn fixture() -> (Project, SequenceId, ClipId) {
    let mut p: Project =
        serde_json::from_str(include_str!("../../../examples/ffi-preview.project.json")).unwrap();
    let range = TimeRange::new(Time::ZERO, Time::from_integer(1)).unwrap();
    let make_clip = |rgb| Clip {
        id: ClipId::new(),
        source_ref: SourceRef::Generator {
            generator: SOLID_GENERATOR_ID.into(),
            version: 1,
            color: Color::new(ColorSpace::LinearRec709, rgb, 0.5).unwrap(),
        },
        timeline_range: range,
        source_in: Time::ZERO,
        time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
        reverse_sampling: None,
        audio_retime: AudioRetimePolicy::Reject,
        volume: None,
        links: vec![],
        effects: vec![],
        properties: vec![],
    };
    let bottom = make_clip([0.2, 0.6, 0.1]);
    let mut top = make_clip([0.8, 0.2, 0.4]);
    top.properties.push(property(
        "kronello.opacity",
        Value::Scalar(FiniteF64::new(0.5).unwrap()),
    ));
    let clip = top.id;
    let sequence = Sequence {
        id: SequenceId::new(),
        extent: DesignExtent::new(2., 2.).unwrap(),
        frame_rate: FrameRate::new(24, 1).unwrap(),
        audio_rate: SampleRate::HZ_48000,
        working_space: ColorSpace::LinearRec709,
        tracks: vec![
            Track {
                state: None,
                id: TrackId::new(),
                kind: TrackKind::Video,
                clips: vec![bottom],
            },
            Track {
                state: None,
                id: TrackId::new(),
                kind: TrackKind::Video,
                clips: vec![top],
            },
        ],
        transitions: vec![],
    };
    let id = sequence.id;
    p.sequences.push(DocumentObject::Known(sequence));
    (p, id, clip)
}
fn image(path: &std::path::Path, sequence: SequenceId) -> FrameResult {
    let ResultData::Frame(r) = run(json!({"operation":"render.frame","input":{"project":path,"target":RenderTarget::Sequence{sequence},
        "region":{"origin":[0.,0.],"extent":[2.,2.],"pixels":[2,2]}},"time":Time::ZERO})).unwrap() else { panic!() };
    *r
}
fn near(actual: [f32; 4], expected: [f32; 4]) {
    for (a, e) in actual.into_iter().zip(expected) {
        assert!((a - e).abs() < 1e-6, "{actual:?} != {expected:?}");
    }
}
#[test]
fn shared_clip_blend_real_render_revision_retry_undo_and_snapshot_pins() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("blend.kronello");
    let (p, sequence, clip) = fixture();
    run(json!({"operation":"project.create","project":path,"document":p})).unwrap();
    let normal = image(&path, sequence);
    near(normal.linear[0], [0.275, 0.275, 0.1375, 0.625]);
    let first = export(&path);
    // Legacy pin absence is recognized only when no authored non-normal blend exists.
    let snapshot = RenderSnapshot::for_target(
        &first.document,
        RenderTarget::Sequence { sequence },
        0,
        Default::default(),
    )
    .unwrap();
    let mut old = serde_json::to_value(&snapshot).unwrap();
    old["semantic_versions"]
        .as_object_mut()
        .unwrap()
        .remove("blend");
    let old: RenderSnapshot = serde_json::from_value(old).unwrap();
    kronello_render::build_scene_ir(&old, Time::ZERO, &[]).unwrap();
    let mut blend_keys = vec![];
    for (mode, expected) in [
        ("multiply", [0.195, 0.265, 0.0925, 0.625]),
        ("screen", [0.28, 0.335, 0.145, 0.625]),
    ] {
        let previous = export(&path);
        let DocumentObject::Known(s) = &previous.document.sequences[0] else {
            panic!()
        };
        let c = &s.tracks[1].clips[0];
        let mut properties = c.properties.clone();
        properties.push(property(BLEND_KEY, Value::Enum(mode.into())));
        let commands = vec![EditCommand::Timeline(Box::new(
            TimelineCommand::ClipSetEffects {
                sequence,
                clip,
                properties,
                effects: c.effects.clone(),
            },
        ))];
        let ResultData::Plan(plan)=run(json!({"operation":"edit.plan","project":path,"base_revision":previous.revision,"commands":commands})).unwrap()else{panic!()};
        let request = json!({"operation":"edit.apply","project":path,"base_revision":previous.revision,"commands":plan.commands,"plan_hash":plan.plan_hash,"session_id":Uuid::new_v4(),"idempotency_key":mode});
        let ResultData::Edit(event) = run(request.clone()).unwrap() else {
            panic!()
        };
        let ResultData::Edit(retry) = run(request.clone()).unwrap() else {
            panic!()
        };
        assert_eq!(event, retry);
        near(image(&path, sequence).linear[0], expected);
        let current = export(&path);
        assert_ne!(current.revision, previous.revision);
        let snapshot = RenderSnapshot::for_target(
            &current.document,
            RenderTarget::Sequence { sequence },
            0,
            Default::default(),
        )
        .unwrap();
        let scene = kronello_render::build_scene_ir(&snapshot, Time::ZERO, &[]).unwrap();
        let dag = kronello_render::build_render_dag(
            &scene,
            Default::default(),
            OutputRegion {
                origin: [0.; 2],
                extent: [2.; 2],
                pixels: [2; 2],
            },
        )
        .unwrap();
        let keys = kronello_render::RasterCacheKey::for_dag(&dag, "blend-reference").unwrap();
        let blend = dag
            .nodes()
            .iter()
            .position(|n| matches!(n, kronello_render::DagNode::Blend { .. }))
            .unwrap();
        blend_keys.push(keys[blend].unwrap().digest());
        for pin in [None, Some(2)] {
            let mut invalid = serde_json::to_value(&snapshot).unwrap();
            invalid["semantic_versions"]["blend"] = json!(pin);
            let invalid: RenderSnapshot = serde_json::from_value(invalid).unwrap();
            assert!(matches!(
                kronello_render::build_scene_ir(&invalid, Time::ZERO, &[]),
                Err(kronello_render::RenderError::UnsupportedFeature(_))
            ));
        }
        let mut stale = request;
        stale["idempotency_key"] = json!(format!("stale-{mode}"));
        assert_eq!(run(stale).unwrap_err().code, "REVISION_CONFLICT");
        run(json!({"operation":"edit.undo","project":path,"base_revision":current.revision,"event_id":event.id,"session_id":Uuid::new_v4(),"idempotency_key":format!("undo-{mode}")})).unwrap();
        assert_eq!(export(&path).document, previous.document);
        assert_eq!(image(&path, sequence).linear, normal.linear);
    }
    assert_ne!(
        blend_keys[0], blend_keys[1],
        "blend mode participates in raster cache identity"
    );
}
#[test]
fn blend_validation_rejects_unknown_duplicate_and_animated_modes() {
    let registry = SchemaRegistry::with_builtin();
    let descriptor = registry
        .lookup(&SchemaKey::new(BLEND_KEY).unwrap())
        .unwrap();
    assert!(
        Property::new(
            PropertyId::new(),
            DescriptorRef::new(descriptor),
            PropertySource::Constant(Value::Enum("overlay".into())),
            vec![],
            &registry
        )
        .is_err()
    );
    assert!(
        Property::new(
            PropertyId::new(),
            DescriptorRef::new(descriptor),
            PropertySource::Curve(CurveId::new()),
            vec![],
            &registry
        )
        .is_err()
    );
    let (mut p, sequence, clip) = fixture();
    let DocumentObject::Known(s) = &mut p.sequences[0] else {
        panic!()
    };
    s.tracks[1].clips[0].properties.extend([
        property(BLEND_KEY, Value::Enum("normal".into())),
        property(BLEND_KEY, Value::Enum("screen".into())),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("invalid.kronello");
    assert!(run(json!({"operation":"project.create","project":path,"document":p})).is_err());
    let _ = (sequence, clip);
}
