use kronello_model::*;
use kronello_service::*;
use kronello_time::{Rational, Time, TimeMap, TimeRange};
#[test]
fn shared_command_persists_fixed_bus_features_and_conflicts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("analysis.kronello");
    let project: Project =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    let composition = match &project.compositions[0] {
        DocumentObject::Known(c) => c.id,
        _ => panic!(),
    };
    let service = Service::new(BackendSelection::Gpu);
    service
        .dispatch(Request::ProjectCreate(CreateRequest {
            project: path.clone(),
            document: project,
            plan_hash: None,
            idempotency_key: None,
        }))
        .unwrap();
    let stored = kronello_store::ProjectStore::read_snapshot(&path).unwrap();
    let id = AssetId::new();
    let request = AudioAnalyzeRequest {
        project: path.clone(),
        base_revision: stored.revision.to_string(),
        id,
        input: AudioAnalyzeInput::Bus {
            target: RenderTarget::Composition { composition },
            range: TimeRange::new(Time::ZERO, Time::new(1, 100).unwrap()).unwrap(),
        },
        config: AudioAnalysisConfig {
            version: 1,
            sample_rate: 48000,
            window: 32,
            hop: 32,
            bands: vec![[0, 1000]],
            time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
        },
        idempotency_key: Some("analysis-once".into()),
    };
    let response = service
        .execute_json(&serde_json::to_string(&Request::AudioAnalyze(request.clone())).unwrap());
    assert!(matches!(response, Response::Success { .. }), "{response:?}");
    let first = serde_json::to_value(&response).unwrap();
    let retry = service
        .execute_json(&serde_json::to_string(&Request::AudioAnalyze(request.clone())).unwrap());
    assert_eq!(first, serde_json::to_value(retry).unwrap());
    let mut reused = request.clone();
    reused.config.bands = vec![[0, 2000]];
    assert_eq!(
        service
            .dispatch(Request::AudioAnalyze(reused))
            .unwrap_err()
            .code,
        "IDEMPOTENCY_KEY_REUSED"
    );
    let reopened = kronello_store::ProjectStore::read_snapshot(&path).unwrap();
    let DocumentObject::Known(data) = &reopened.document.audio_analyses[0] else {
        panic!()
    };
    assert_eq!(data.id, id);
    assert_eq!(data.sample_count, 480);
    assert_eq!(data.frames.len(), 15);
    assert!(matches!(
        data.source,
        AudioAnalysisSource::Bus {
            evaluator_version: 2,
            ..
        }
    ));
    assert_eq!(data.sample(Time::ZERO, AudioFeature::Rms).unwrap(), 0.0);
    let mut without_key = request;
    without_key.idempotency_key = None;
    assert_eq!(
        service
            .dispatch(Request::AudioAnalyze(without_key))
            .unwrap_err()
            .code,
        "REVISION_CONFLICT"
    );
}

#[test]
fn renderer_consumes_persisted_features_and_rejects_changed_source() {
    let mut p: Project =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    p.shapes.clear();
    p.texts.clear();
    p.curves.clear();
    let id = AssetId::new();
    let source = AssetId::new();
    p.assets.push(DocumentObject::Known(Asset {
        id: source,
        kind: AssetKind::Audio,
        content_hash: "a".repeat(64),
        streams: vec![],
        locator: AssetLocator {
            relative: Some("audio.wav".into()),
            absolute: None,
        },
    }));
    p.audio_analyses
        .push(DocumentObject::Known(AudioAnalysisDataAsset {
            id,
            source: AudioAnalysisSource::Asset {
                asset: source,
                stream_index: 0,
                content_hash: "a".repeat(64),
            },
            config: AudioAnalysisConfig {
                version: 1,
                sample_rate: 48000,
                window: 32,
                hop: 32,
                bands: vec![],
                time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
            },
            start_sample: 0,
            sample_count: 32,
            frames: vec![AudioAnalysisFrame {
                time: Time::ZERO,
                rms: 0.5,
                band_energy: vec![],
                onset: 0.5,
                beat: true,
            }],
        }));
    let expression = Expression {
        id: ExpressionId::new(),
        version: 2,
        value_type: ValueType::Scalar,
        budget: Default::default(),
        nodes: vec![ExpressionNode::AudioFeature {
            asset: id,
            feature: AudioFeature::Rms,
            offset: Time::ZERO,
        }],
    };
    let DocumentObject::Known(c) = &mut p.compositions[0] else {
        panic!()
    };
    c.nodes.truncate(1);
    c.root_nodes.truncate(1);
    c.nodes[0].kind = NodeKind::Null;
    c.nodes[0]
        .properties
        .retain(|p| p.descriptor().key.as_str() == "kronello.opacity");
    c.nodes[0].properties[0]
        .set_source(
            PropertySource::Expression(expression.id),
            &SchemaRegistry::with_builtin(),
        )
        .unwrap();
    let composition = c.id;
    p.expressions.push(DocumentObject::Known(expression));
    let snapshot =
        kronello_render::RenderSnapshot::new(&p, composition, 1, Default::default()).unwrap();
    for _ in 0..3 {
        kronello_render::build_scene_ir(&snapshot, Time::ZERO, &[]).unwrap();
    }
    assert_eq!(
        snapshot.semantic_versions().expression,
        EXPRESSION_SUPPORTED_VERSION
    );
    let mut pin2 = serde_json::to_value(&snapshot).unwrap();
    pin2["semantic_versions"]["expression"] = serde_json::json!(2);
    let pin2: kronello_render::RenderSnapshot = serde_json::from_value(pin2).unwrap();
    assert_eq!(pin2.semantic_versions().expression, 2);
    kronello_render::build_scene_ir(&pin2, Time::ZERO, &[]).unwrap();
    let mut legacy = serde_json::to_value(&snapshot).unwrap();
    legacy["semantic_versions"]
        .as_object_mut()
        .unwrap()
        .remove("expression");
    let legacy: kronello_render::RenderSnapshot = serde_json::from_value(legacy).unwrap();
    assert_eq!(legacy.semantic_versions().expression, EXPRESSION_VERSION);
    assert!(kronello_render::build_scene_ir(&legacy, Time::ZERO, &[]).is_err());

    let DocumentObject::Known(asset) = &mut p.assets[0] else {
        panic!()
    };
    asset.content_hash = "b".repeat(64);
    assert!(p.audio_analysis_inputs().is_err());
    let changed =
        kronello_render::RenderSnapshot::new(&p, composition, 2, Default::default()).unwrap();
    assert!(kronello_render::build_scene_ir(&changed, Time::ZERO, &[]).is_err());
}

#[test]
fn future_analysis_meaning_is_retained_but_not_executed() {
    let mut p = Project::default();
    let data = AudioAnalysisDataAsset {
        id: AssetId::new(),
        source: AudioAnalysisSource::Bus {
            snapshot_hash: "a".repeat(64),
            target: "fixed".into(),
            evaluator_version: 2,
        },
        config: AudioAnalysisConfig {
            version: 99,
            sample_rate: 48000,
            window: 32,
            hop: 32,
            bands: vec![],
            time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
        },
        start_sample: 0,
        sample_count: 0,
        frames: vec![],
    };
    p.audio_analyses.push(DocumentObject::Known(data));
    p.validate_storage().unwrap();
    assert!(p.ensure_editable().is_err());
    let roundtrip: Project = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
    assert_eq!(roundtrip, p);
}
