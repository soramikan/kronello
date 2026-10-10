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
        session_id: None,
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

fn execute(request: serde_json::Value) -> serde_json::Value {
    serde_json::to_value(
        Service::new(BackendSelection::CpuReference).execute_json(&request.to_string()),
    )
    .unwrap()
}

fn song_config() -> AudioAnalysisConfig {
    AudioAnalysisConfig {
        version: 2,
        sample_rate: 48000,
        window: 2048,
        hop: 1600,
        bands: vec![[0, 500], [500, 1000], [1000, 24000]],
        time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
    }
}

#[test]
fn full_song_pcm_is_persisted_and_evaluated_at_video_frame_times() {
    use serde_json::json;
    use std::io::Write;
    // 241.5 s of stereo PCM16, generated in one-second blocks (no large fixture).
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("song.kronello");
    let wav = dir.path().join("song.wav");
    let samples = 11_592_000_u32;
    let mut file = std::fs::File::create(&wav).unwrap();
    file.write_all(b"RIFF").unwrap();
    file.write_all(&(36 + samples * 4).to_le_bytes()).unwrap();
    file.write_all(b"WAVEfmt ").unwrap();
    file.write_all(&16_u32.to_le_bytes()).unwrap();
    file.write_all(&1_u16.to_le_bytes()).unwrap();
    file.write_all(&2_u16.to_le_bytes()).unwrap();
    file.write_all(&48000_u32.to_le_bytes()).unwrap();
    file.write_all(&192000_u32.to_le_bytes()).unwrap();
    file.write_all(&4_u16.to_le_bytes()).unwrap();
    file.write_all(&16_u16.to_le_bytes()).unwrap();
    file.write_all(b"data").unwrap();
    file.write_all(&(samples * 4).to_le_bytes()).unwrap();
    let block: Vec<u8> = (0..48000)
        .flat_map(|i| {
            let value = ((std::f64::consts::TAU * (i % 64) as f64 / 64.0).sin() * 8192.0) as i16;
            let bytes = value.to_le_bytes();
            [bytes[0], bytes[1], bytes[0], bytes[1]]
        })
        .collect();
    for _ in 0..241 {
        file.write_all(&block).unwrap();
    }
    file.write_all(&block[..24000 * 4]).unwrap();
    drop(file);
    let asset = AssetId::new();
    let id = AssetId::new();
    let session = uuid::Uuid::new_v4();
    let mut project: Project =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    project.assets.push(DocumentObject::Known(Asset {
        id: asset,
        kind: AssetKind::Audio,
        content_hash: kronello_media::content_hash(&wav).unwrap(),
        streams: vec![],
        locator: AssetLocator {
            relative: Some("song.wav".into()),
            absolute: None,
        },
    }));
    let created =
        execute(json!({"operation":"project.create", "project":path, "document":project}));
    assert_eq!(created["status"], "success", "{created}");
    let request = json!({"operation":"audio.analyze", "project":path, "base_revision":"1",
        "session_id":session, "idempotency_key":"song", "id":id,
        "input":{"kind":"asset","asset":asset,"stream_index":0}, "config":song_config()});
    let result = execute(request.clone());
    assert_eq!(result["status"], "success", "{result}");
    let stored = kronello_store::ProjectStore::read_snapshot(&path).unwrap();
    let data = stored.document.audio_analysis_inputs().unwrap();
    assert_eq!(data[0].sample_count, samples);
    assert_eq!(data[0].frames.len(), 7245);
    assert_eq!(data[0].config.version, 2);
    assert!((data[0].frames[0].rms - 0.25 / 2.0_f64.sqrt()).abs() < 0.0001);
    assert!(data[0].frames[0].band_energy[1] > 0.03);
    assert!(data[0].frames[0].band_energy[0] < 1e-8);
    let receipt = kronello_store::ProjectStore::read_idempotency_record(&path, "song")
        .unwrap()
        .unwrap();
    assert_eq!(receipt.result.session_id, session);
    // Replay is independent of the current revision and the media's existence.
    std::fs::remove_file(&wav).unwrap();
    assert_eq!(execute(request.clone()), result);
    let mut reused = request;
    reused["session_id"] = json!(uuid::Uuid::new_v4());
    assert_eq!(execute(reused)["error"]["code"], "IDEMPOTENCY_KEY_REUSED");

    use kronello_eval::{DependencyGraph, EvaluationSnapshot, RuntimePropertyKey};
    for feature in [
        AudioFeature::Rms,
        AudioFeature::BandEnergy { band: 1 },
        AudioFeature::Onset,
    ] {
        let expression = Expression {
            id: ExpressionId::new(),
            version: 2,
            value_type: ValueType::Scalar,
            budget: Default::default(),
            nodes: vec![ExpressionNode::AudioFeature {
                asset: id,
                feature,
                offset: Time::ZERO,
            }],
        };
        let DocumentObject::Known(mut composition) = stored.document.compositions[0].clone() else {
            panic!()
        };
        composition.nodes.truncate(1);
        composition.root_nodes.truncate(1);
        composition.nodes[0].kind = NodeKind::Null;
        composition.nodes[0]
            .properties
            .retain(|p| p.descriptor().key.as_str() == "kronello.opacity");
        let registry = SchemaRegistry::with_builtin();
        composition.nodes[0].properties[0]
            .set_source(PropertySource::Expression(expression.id), &registry)
            .unwrap();
        let key: RuntimePropertyKey = PropertyKey {
            instance_path: InstancePath::root(),
            node: composition.nodes[0].id,
            property: composition.nodes[0].properties[0].id(),
        }
        .into();
        let compositions = [composition];
        let expressions = [expression];
        let refs = Default::default();
        let deps = Default::default();
        let graph = DependencyGraph::compile_with_audio(
            EvaluationSnapshot {
                compositions: &compositions,
                expressions: &expressions,
                curves: &[],
                registry: &registry,
                reference_bindings: &refs,
                dependencies: &deps,
                working_space: ColorSpace::LinearRec709,
            },
            compositions[0].id,
            &data,
        )
        .unwrap();
        for frame in [7244, 0, 3600, 1, 7244] {
            let time = Time::new(frame, 30).unwrap();
            let expected =
                Value::Scalar(FiniteF64::new(data[0].sample(time, feature).unwrap()).unwrap());
            assert_eq!(graph.evaluate_property(&key, time).unwrap(), expected);
        }
        assert!(
            graph
                .evaluate_property(&key, Time::new(483, 2).unwrap())
                .is_err()
        );
    }
}

#[test]
fn shared_json_diagnostics_are_actionable_before_bus_preparation() {
    use serde_json::json;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("diagnostics.kronello");
    assert_eq!(
        execute(
            json!({"operation":"project.create", "project":path, "document":Project::default()})
        )["status"],
        "success"
    );
    // Missing target proves rejected requests never prepare/render the bus.
    let request = json!({"operation":"audio.analyze", "project":path, "base_revision":"1", "id":AssetId::new(),
        "config":song_config(), "input":{"kind":"bus", "target":{"kind":"composition", "composition":CompositionId::new()},
        "range":TimeRange::new(Time::ZERO, Time::new(600, 1).unwrap()).unwrap()}});
    for (field, value, kind) in [
        ("version", json!(99), "version"),
        ("sample_rate", json!(44100), "sample_rate"),
        ("window", json!(1000), "window"),
        ("hop", json!(0), "hop"),
        ("bands", json!(vec![[0, 1000]; 33]), "bands"),
        ("bands", json!([[1000, 1000]]), "band"),
    ] {
        let mut bad = request.clone();
        bad["config"][field] = value;
        let response = execute(bad);
        assert_eq!(response["error"]["code"], "INVALID_REQUEST", "{response}");
        assert_eq!(response["error"]["details"]["kind"], kind);
        assert!(
            response["error"]["message"]
                .as_str()
                .unwrap()
                .contains(field)
        );
    }
    for (hop, bands, resource, required, allowed) in [
        (1, 3, "windows", 28_800_000, 65_536),
        (1600, 32, "work", 3_244_032_000_u64, 1_500_000_000),
    ] {
        let mut bad = request.clone();
        bad["config"]["hop"] = json!(hop);
        bad["config"]["bands"] = json!(vec![[0, 1000]; bands]);
        let response = execute(bad);
        assert_eq!(
            response["error"]["code"], "AUDIO_BUDGET_EXCEEDED",
            "{response}"
        );
        assert_eq!(
            response["error"]["details"],
            json!({"kind":"budget", "resource":resource, "required":required, "allowed":allowed})
        );
    }
    let mut bad = request;
    bad["input"]["range"] = json!(TimeRange::new(Time::ZERO, Time::new(601, 1).unwrap()).unwrap());
    let response = execute(bad);
    assert_eq!(
        response["error"]["details"],
        json!({"kind":"budget", "resource":"sample_count", "required":28_848_000, "allowed":28_800_000})
    );
    assert_eq!(
        kronello_store::ProjectStore::read_snapshot(&path)
            .unwrap()
            .revision,
        1
    );
}
