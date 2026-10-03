use kronello_model::*;
use kronello_service::*;
use kronello_time::{Time, TimeMap};
use serde_json::{Value as Json, json};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
use uuid::Uuid;

fn service() -> Service<'static> {
    Service::new(BackendSelection::Gpu)
}
fn fixture() -> Project {
    serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap()
}
fn comp(p: &Project) -> &Composition {
    let DocumentObject::Known(c) = &p.compositions[0] else {
        panic!()
    };
    c
}
fn setup(document: Project) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("api.kronello");
    service()
        .dispatch(Request::ProjectCreate(CreateRequest {
            project: project.clone(),
            document,
        }))
        .unwrap();
    (dir, project)
}
fn success(response: Response) -> ResultData {
    let Response::Success { result } = response else {
        panic!("{response:?}")
    };
    result
}
fn invalid(json: Json) {
    let Response::Error { error } = service().execute_json(&json.to_string()) else {
        panic!()
    };
    assert_eq!(error.code, "INVALID_REQUEST", "{error:?}");
}
fn scene(path: &Path, composition: CompositionId, expand_instances: bool) -> SceneQueryResult {
    let ResultData::Scene(r) = service()
        .dispatch(Request::SceneQuery(SceneQueryRequest {
            project: path.into(),
            composition,
            expand_instances,
        }))
        .unwrap()
    else {
        panic!()
    };
    r
}

#[test]
fn scene_tree_preserves_order_parents_ranges_and_instance_identity() {
    let mut p = fixture();
    let DocumentObject::Known(c) = &mut p.compositions[0] else {
        panic!()
    };
    let original = c.clone();
    // Text is owned by Shape but transform-parented to the placement frame.
    c.root_nodes.pop();
    let child_id = c.nodes[1].id;
    c.nodes[0].child_order.push(child_id);
    c.nodes[1].containment_parent = Some(c.nodes[0].id);
    let definition = c.id;
    let mut root = original.clone();
    root.id = CompositionId::new();
    root.properties.clear();
    root.nodes.clear();
    root.root_nodes.clear();
    for _ in 0..2 {
        let id = NodeId::new();
        root.root_nodes.push(id);
        root.nodes.push(SceneNode {
            id,
            kind: NodeKind::CompositionInstance(CompositionInstance {
                id: CompositionInstanceId::new(),
                definition_ref: definition,
                input_bindings: Default::default(),
                local_time_map: TimeMap::linear(Time::ZERO, kronello_time::Rational::ONE).unwrap(),
                seed: 0,
            }),
            containment_parent: None,
            transform_parent: None,
            child_order: vec![],
            active_range: original.nodes[0].active_range,
            properties: vec![],
        });
    }
    let root_id = root.id;
    p.compositions.push(DocumentObject::Known(root));
    let (dir, path) = setup(p);
    let plain = scene(&path, root_id, false);
    assert_eq!(plain.nodes.len(), 2);
    let expanded = scene(&path, root_id, true);
    assert_eq!(expanded.nodes.len(), 6);
    assert_eq!(expanded.roots, plain.roots);
    let a = &expanded.nodes[1];
    let text = &expanded.nodes[2];
    let b = &expanded.nodes[4];
    assert_eq!(a.key.node, b.key.node);
    assert_ne!(a.key.instance_path, b.key.instance_path);
    assert_eq!(a.containment_parent, Some(expanded.nodes[0].key.clone()));
    assert_eq!(text.containment_parent, Some(a.key.clone()));
    assert_eq!(text.transform_parent, Some(expanded.nodes[0].key.clone()));
    assert_eq!(a.active_range, original.nodes[0].active_range);
    assert_eq!(a.children, vec![text.key.clone()]);
    assert!(matches!(a.kind, NodeKind::Shape { .. }));
    assert_eq!(expanded.revision, "1");
    assert!(dir.path().is_dir());
}

#[test]
fn property_samples_use_rational_times_typed_values_units_and_failures() {
    let p = fixture();
    let c = comp(&p);
    let composition = c.id;
    let position = c.nodes[0]
        .properties
        .iter()
        .find(|p| p.descriptor().key.as_str() == "kronello.transform.position")
        .unwrap();
    let key = SampleKey::Node {
        instance_path: InstancePath::root(),
        node: c.nodes[0].id,
        property: position.id(),
    };
    let (dir, path) = setup(p);
    let times = vec![
        Time::new(1, 2).unwrap(),
        Time::ZERO,
        Time::new(1, 2).unwrap(),
    ];
    let request = PropertySampleRequest {
        project: path.clone(),
        composition,
        keys: vec![key],
        times: times.clone(),
    };
    let ResultData::Samples(result) = service()
        .dispatch(Request::PropertySample(request.clone()))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(result.times, times);
    assert_eq!(result.samples[0].unit, Unit::DesignPx);
    assert_eq!(result.samples[0].value_type, ValueType::Vec2);
    assert_eq!(result.samples[0].values[0], result.samples[0].values[2]);
    assert_ne!(result.samples[0].values[0], result.samples[0].values[1]);
    assert!(matches!(&result.samples[0].values[0], Value::Vec2(_)));
    let mut bad = serde_json::to_value(Request::PropertySample(request)).unwrap();
    bad["times"] = json!([]);
    invalid(bad.clone());
    bad["times"] = json!([{"num":"1", "den":"0"}]);
    invalid(bad);
    let result_json = serde_json::to_string(&Response::Success {
        result: ResultData::Samples(result),
    })
    .unwrap();
    let decoded: Response = serde_json::from_str(&result_json).unwrap();
    assert!(matches!(decoded, Response::Success { .. }));
    assert!(dir.path().is_dir());
}

#[test]
fn capabilities_registry_media_extension_without_device_initialization() {
    let media = MediaCapabilities {
        runtime_version: "test-runtime".into(),
        decoders: vec!["h264".into()],
        encoders: vec!["prores".into()],
        hwaccels: vec!["videotoolbox".into()],
    };
    let ResultData::Capabilities(c) = success(
        service()
            .with_media_capabilities(media)
            .execute_json(r#"{"operation":"capabilities.get"}"#),
    ) else {
        panic!()
    };
    assert_eq!(c.commands.len(), 13);
    assert_eq!(c.api_schema_version, 1);
    assert_eq!(c.semantic_versions.document, PROJECT_SEMANTIC_VERSION);
    assert_eq!(c.media.unwrap().hwaccels, vec!["videotoolbox"]);
    assert!(c.effects.is_empty());
    assert!(c.backends.contains(&"cpu_reference_float32".into()));
    let mutating: Vec<_> = c
        .commands
        .iter()
        .filter(|c| !c.read_only)
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(
        mutating,
        [
            "project.create",
            "project.import",
            "edit.apply",
            "edit.undo"
        ]
    );
    let ResultData::Capabilities(c) =
        success(service().execute_json(r#"{"operation":"capabilities.get"}"#))
    else {
        panic!()
    };
    assert!(c.media.is_none());
}

#[test]
fn public_schema_matches_and_all_registry_schemas_are_safe() {
    let generated = api_json_schema();
    let committed: Json =
        serde_json::from_str(include_str!("../../../schemas/api-v1.schema.json")).unwrap();
    assert_eq!(generated, committed);
    let validator = jsonschema::validator_for(&generated).unwrap();
    let response =
        serde_json::to_value(service().execute_json(r#"{"operation":"capabilities.get"}"#))
            .unwrap();
    assert!(validator.is_valid(&response), "{response}");
    let operations: BTreeSet<_> = generated["$defs"]["Request"]["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            v["properties"]["operation"]["const"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(
        operations,
        command_registry().into_iter().map(|c| c.name).collect()
    );
    fn safe(value: &Json, root: &Json, visited: &mut BTreeSet<String>) {
        if let Some(reference) = value.get("$ref").and_then(Json::as_str)
            && visited.insert(reference.into())
        {
            safe(
                root.pointer(reference.strip_prefix('#').unwrap()).unwrap(),
                root,
                visited,
            );
        }
        if let Some(properties) = value.get("properties").and_then(Json::as_object) {
            for name in properties.keys() {
                assert!(
                    ![
                        "shell",
                        "exec",
                        "script",
                        "url",
                        "fetch_url",
                        "ffmpeg_args",
                        "raw_ffmpeg_args",
                        "command_line"
                    ]
                    .contains(&name.as_str()),
                    "unsafe schema field {name}"
                );
            }
        }
        match value {
            Json::Object(o) => {
                for v in o.values() {
                    safe(v, root, visited);
                }
            }
            Json::Array(a) => {
                for v in a {
                    safe(v, root, visited);
                }
            }
            _ => {}
        }
    }
    for c in command_registry() {
        for reference in [&c.request_schema, &c.response_schema] {
            let pointer = reference.split_once('#').unwrap().1;
            assert!(generated.pointer(pointer).is_some(), "{reference}");
        }
        safe(
            generated
                .pointer(c.request_schema.split_once('#').unwrap().1)
                .unwrap(),
            &generated,
            &mut BTreeSet::new(),
        );
    }
    assert!(!validator.is_valid(&json!({"operation":"capabilities.get", "shell":"touch /tmp/x"})));
}

#[test]
fn execution_fields_uris_and_duplicate_envelopes_are_invalid_requests() {
    for name in [
        "shell",
        "exec",
        "url",
        "fetch_url",
        "ffmpeg_args",
        "command_line",
    ] {
        let mut request = json!({"operation":"capabilities.get"});
        request[name] = json!("untrusted");
        invalid(request);
    }
    for path in [
        "https://example.invalid/x.kronello",
        "file:///tmp/x.kronello",
        "pipe:0",
        "data:text/plain,test",
    ] {
        invalid(json!({"operation":"project.info", "project":path}));
    }
    let mut document = serde_json::to_value(fixture()).unwrap();
    document["assets"] = json!([{"locator":{"absolute":"https://example.invalid/movie.mp4"}}]);
    invalid(
        json!({"operation":"project.create", "project":"/tmp/no-file.kronello", "document": document}),
    );
    let Response::Error { error } =
        service().execute_json(r#"{"operation":"capabilities.get","operation":"project.info"}"#)
    else {
        panic!()
    };
    assert_eq!(error.code, "INVALID_REQUEST");
    // Strings that resemble commands/URLs in material names remain data.
    let mut document = fixture();
    document.name = "https://example.invalid/; $(touch /tmp/never)".into();
    let (_dir, path) = setup(document);
    let ResultData::Project(info) = service()
        .dispatch(Request::ProjectInfo(ProjectRequest { project: path }))
        .unwrap()
    else {
        panic!()
    };
    assert!(info.name.contains("$(touch"));
}

#[test]
fn history_pages_sessions_changed_keys_and_undo_outside_page() {
    let p = fixture();
    let c = comp(&p);
    let command = EditCommand::PropertySourceSet {
        object: c.nodes[0].id.as_uuid(),
        property: c.nodes[0].properties[1].id(),
        source: PropertySource::Constant(Value::Scalar(FiniteF64::new(2.0).unwrap())),
        curve: None,
    };
    let (_dir, path) = setup(p);
    let session = Uuid::new_v4();
    let ResultData::Plan(plan) = service()
        .dispatch(Request::EditPlan(PlanRequest {
            project: path.clone(),
            base_revision: "1".into(),
            commands: vec![command],
        }))
        .unwrap()
    else {
        panic!()
    };
    let ResultData::Edit(event) = service()
        .dispatch(Request::EditApply(EditApplyRequest {
            project: path.clone(),
            base_revision: "1".into(),
            commands: plan.commands,
            plan_hash: plan.plan_hash,
            idempotency_key: "apply".into(),
            session_id: session,
        }))
        .unwrap()
    else {
        panic!()
    };
    let ResultData::Edit(undo) = service()
        .dispatch(Request::EditUndo(UndoRequest {
            project: path.clone(),
            base_revision: "2".into(),
            session_id: session,
            idempotency_key: "undo".into(),
            event_id: event.id,
        }))
        .unwrap()
    else {
        panic!()
    };
    let get = |since: &str, limit, filter| {
        let ResultData::History(h) = service()
            .dispatch(Request::HistoryList(HistoryRequest {
                project: path.clone(),
                since_revision: since.into(),
                limit,
                session_id: filter,
            }))
            .unwrap()
        else {
            panic!()
        };
        h
    };
    let first = get("1", 1, Some(session));
    assert_eq!(first.revision, "3");
    assert_eq!(first.next_since_revision.as_deref(), Some("2"));
    assert_eq!(first.events[0].event, event);
    assert!(first.events[0].undone);
    assert!(!first.events[0].event.changed_keys.is_empty());
    let last = get(
        first.next_since_revision.as_ref().unwrap(),
        1,
        Some(session),
    );
    assert_eq!(last.events[0].event.undo_of, Some(event.id));
    assert_eq!(last.events[0].event, undo);
    assert!(!last.events[0].undone);
    assert!(last.next_since_revision.is_none());
    assert!(get("0", 100, Some(Uuid::new_v4())).events.is_empty());
    // An Undo of Undo from another session must affect filtered status too.
    service()
        .dispatch(Request::EditUndo(UndoRequest {
            project: path.clone(),
            base_revision: "3".into(),
            session_id: Uuid::new_v4(),
            idempotency_key: "redo-other-session".into(),
            event_id: undo.id,
        }))
        .unwrap();
    let filtered = get("1", 100, Some(session));
    assert_eq!(filtered.events.len(), 2);
    assert!(!filtered.events[0].undone);
    assert!(filtered.events[1].undone);
    assert_eq!(filtered.revision, "4");
    invalid(json!({"operation":"history.list", "project":path, "limit":0}));
}

#[test]
fn every_request_payload_and_envelope_matches_schema_and_denies_execution_fields() {
    let schema = api_json_schema();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let p = fixture();
    let composition = comp(&p).id;
    let path = "local.kronello";
    let uuid = Uuid::new_v4();
    let input = json!({"project":path, "composition":composition,
        "region":{"origin":[0.0,0.0],"extent":[64.0,32.0],"pixels":[64,32]}});
    let time = json!({"num":"1", "den":"2"});
    let requests = vec![
        json!({"operation":"project.create", "project":path, "document":p}),
        json!({"operation":"project.import", "project":path, "base_revision":"1", "document":p}),
        json!({"operation":"project.export", "project":path}),
        json!({"operation":"project.info", "project":path}),
        json!({"operation":"render.frame", "input":input, "time":time}),
        json!({"operation":"render.sequence", "input":input, "range":{"start":{"num":"0","den":"1"},"end":time},
            "frame_rate":{"num":"24","den":"1"},"output_directory":"frames"}),
        json!({"operation":"edit.plan", "project":path, "base_revision":"1", "commands":[]}),
        json!({"operation":"edit.apply", "project":path, "base_revision":"1", "commands":[], "plan_hash":"hash",
            "idempotency_key":"key", "session_id":uuid}),
        json!({"operation":"edit.undo", "project":path, "base_revision":"1", "session_id":uuid, "idempotency_key":"key", "event_id":uuid}),
        json!({"operation":"history.list", "project":path}),
        json!({"operation":"scene.query", "project":path, "composition":composition}),
        json!({"operation":"property.sample", "project":path, "composition":composition, "keys":[
            {"kind":"node", "instance_path":[], "node":comp(&p).nodes[0].id,"property":comp(&p).nodes[0].properties[0].id()}], "times":[time]}),
        json!({"operation":"capabilities.get"}),
    ];
    assert_eq!(requests.len(), command_registry().len());
    for request in requests {
        assert!(validator.is_valid(&request), "{request}");
        let typed: Request = serde_json::from_str(&request.to_string()).unwrap();
        let normalized = serde_json::to_value(typed).unwrap();
        assert!(validator.is_valid(&normalized), "{normalized}");
        let command = command_registry()
            .into_iter()
            .find(|c| c.name == request["operation"])
            .unwrap();
        let mut payload = normalized.clone();
        payload.as_object_mut().unwrap().remove("operation");
        let mut payload_schema = schema
            .pointer(command.request_schema.split_once('#').unwrap().1)
            .unwrap()
            .clone();
        payload_schema["$defs"] = schema["$defs"].clone();
        assert!(
            jsonschema::validator_for(&payload_schema)
                .unwrap()
                .is_valid(&payload),
            "{payload}"
        );
        for field in ["shell", "url", "ffmpeg_args"] {
            let mut bad = request.clone();
            bad[field] = json!("execute");
            assert!(!validator.is_valid(&bad), "{bad}");
            invalid(bad);
        }
    }
    for response in [
        json!({"status":"error","error":{"code":"INVALID_REQUEST","message":"x"}}),
        json!({"status":"success","result":{"kind":"export","value":{"revision":"1","document":p}}}),
    ] {
        assert!(validator.is_valid(&response));
        serde_json::from_str::<Response>(&response.to_string()).unwrap();
        let mut bad = response;
        bad["shell"] = json!("x");
        assert!(!validator.is_valid(&bad));
        assert!(serde_json::from_str::<Response>(&bad.to_string()).is_err());
    }
}

#[test]
fn sampling_resolves_composition_inputs_placement_bindings_and_local_time() {
    let mut p = fixture();
    let registry = SchemaRegistry::with_builtin();
    let descriptor = registry
        .lookup(&SchemaKey::new("kronello.opacity").unwrap())
        .unwrap();
    let input = Property::new(
        PropertyId::new(),
        DescriptorRef::new(descriptor),
        PropertySource::Constant(Value::Scalar(FiniteF64::new(0.5).unwrap())),
        vec![],
        &registry,
    )
    .unwrap();
    let DocumentObject::Known(c) = &mut p.compositions[0] else {
        panic!()
    };
    c.properties.push(input.clone());
    let definition = c.clone();
    let position = definition.nodes[0]
        .properties
        .iter()
        .find(|p| p.descriptor().key.as_str() == "kronello.transform.position")
        .unwrap();
    let mut root = definition.clone();
    root.id = CompositionId::new();
    root.properties.clear();
    root.nodes.clear();
    root.root_nodes.clear();
    let placement = CompositionInstanceId::new();
    let node = NodeId::new();
    root.root_nodes.push(node);
    root.nodes.push(SceneNode {
        id: node,
        kind: NodeKind::CompositionInstance(CompositionInstance {
            id: placement,
            definition_ref: definition.id,
            input_bindings: [(
                input.id(),
                PropertySource::Constant(Value::Scalar(FiniteF64::new(0.75).unwrap())),
            )]
            .into(),
            local_time_map: TimeMap::linear(Time::new(1, 2).unwrap(), kronello_time::Rational::ONE)
                .unwrap(),
            seed: 0,
        }),
        containment_parent: None,
        transform_parent: None,
        child_order: vec![],
        active_range: definition.nodes[0].active_range,
        properties: vec![],
    });
    let root_id = root.id;
    p.compositions.push(DocumentObject::Known(root));
    let (_dir, path) = setup(p);
    let ResultData::Samples(result) = service()
        .dispatch(Request::PropertySample(PropertySampleRequest {
            project: path.clone(),
            composition: root_id,
            times: vec![Time::ZERO],
            keys: vec![
                SampleKey::Composition {
                    instance_path: InstancePath::new(vec![placement]),
                    composition: definition.id,
                    property: input.id(),
                },
                SampleKey::Node {
                    instance_path: InstancePath::new(vec![placement]),
                    node: definition.nodes[0].id,
                    property: position.id(),
                },
            ],
        }))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(
        result.samples[0].values,
        vec![Value::Scalar(FiniteF64::new(0.75).unwrap())]
    );
    let ResultData::Samples(direct) = service()
        .dispatch(Request::PropertySample(PropertySampleRequest {
            project: path.clone(),
            composition: definition.id,
            times: vec![Time::new(1, 2).unwrap()],
            keys: vec![SampleKey::Node {
                instance_path: InstancePath::root(),
                node: definition.nodes[0].id,
                property: position.id(),
            }],
        }))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(result.samples[1].values, direct.samples[0].values);
    let error = service()
        .dispatch(Request::PropertySample(PropertySampleRequest {
            project: path,
            composition: root_id,
            times: vec![Time::ZERO],
            keys: vec![SampleKey::Node {
                instance_path: InstancePath::root(),
                node: definition.nodes[0].id,
                property: position.id(),
            }],
        }))
        .unwrap_err();
    assert_eq!(error.code, "INVALID_REQUEST");
}

#[test]
fn unsupported_expression_and_enabled_modifier_fail_without_partial_samples() {
    for expression in [false, true] {
        let mut json = serde_json::to_value(fixture()).unwrap();
        if expression {
            json["compositions"][0]["nodes"][0]["properties"][0]["source"] =
                json!({"kind":"expression", "value":Uuid::new_v4()});
        } else {
            json["compositions"][0]["nodes"][0]["properties"][0]["modifiers"] = json!([{
                "id":Uuid::new_v4(), "key":"test.unsupported", "version":1, "enabled":true, "parameters":{}
            }]);
        }
        let p: Project = serde_json::from_str(&json.to_string()).unwrap();
        let c = comp(&p);
        let composition = c.id;
        let key = SampleKey::Node {
            instance_path: InstancePath::root(),
            node: c.nodes[0].id,
            property: c.nodes[0].properties[0].id(),
        };
        let (_dir, path) = setup(p);
        let error = service()
            .dispatch(Request::PropertySample(PropertySampleRequest {
                project: path,
                composition,
                times: vec![Time::ZERO, Time::new(1, 2).unwrap()],
                keys: vec![key],
            }))
            .unwrap_err();
        assert_eq!(error.code, "UNSUPPORTED_FEATURE");
    }
}

#[test]
fn actual_results_for_every_command_match_envelope_and_registry_schemas() {
    let schema = api_json_schema();
    let envelope = jsonschema::validator_for(&schema).unwrap();
    let engine = Service::new(BackendSelection::CpuReference);
    let mut checked = BTreeSet::new();
    let mut execute = |request: Json| {
        envelope.validate(&request).unwrap();
        let typed: Request = serde_json::from_str(&request.to_string()).unwrap();
        envelope
            .validate(&serde_json::to_value(typed).unwrap())
            .unwrap();
        let name = request["operation"].as_str().unwrap();
        let response = engine.execute_json(&request.to_string());
        assert!(matches!(response, Response::Success { .. }), "{response:?}");
        let json = serde_json::to_value(&response).unwrap();
        assert!(envelope.is_valid(&json), "{name}: {json}");
        let decoded: Response = serde_json::from_str(&json.to_string()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), json);
        let command = command_registry()
            .into_iter()
            .find(|c| c.name == name)
            .unwrap();
        let mut value_schema = schema
            .pointer(command.response_schema.split_once('#').unwrap().1)
            .unwrap()
            .clone();
        value_schema["$defs"] = schema["$defs"].clone();
        assert!(
            jsonschema::validator_for(&value_schema)
                .unwrap()
                .is_valid(&json["result"]["value"]),
            "{name}: {}",
            json["result"]["value"]
        );
        checked.insert(name.to_owned());
        json["result"]["value"].clone()
    };
    // Small Shape-only rendering exercises metadata schemas without fonts or GPU.
    let mut document = fixture();
    let DocumentObject::Known(c) = &mut document.compositions[0] else {
        panic!()
    };
    c.nodes.truncate(1);
    c.root_nodes.truncate(1);
    let composition = c.id;
    let node = c.nodes[0].id;
    let property = c.nodes[0].properties[1].id();
    document.texts.clear();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("contracts.kronello");
    execute(json!({"operation":"project.create", "project":path, "document":document}));
    execute(
        json!({"operation":"project.import", "project":path, "base_revision":"1", "document":document}),
    );
    execute(json!({"operation":"project.info", "project":path}));
    execute(json!({"operation":"project.export", "project":path}));
    let commands = json!([{"property_source_set":{"object":node, "property":property,
        "source":{"kind":"constant", "value":{"kind":"scalar", "value":2.0}}}}]);
    let plan = execute(
        json!({"operation":"edit.plan", "project":path, "base_revision":"2", "commands":commands}),
    );
    let session = Uuid::new_v4();
    let event = execute(
        json!({"operation":"edit.apply", "project":path, "base_revision":"2", "commands":commands,
        "plan_hash":plan["plan_hash"], "session_id":session, "idempotency_key":"contract-apply"}),
    );
    execute(
        json!({"operation":"edit.undo", "project":path, "base_revision":"3", "event_id":event["id"],
        "session_id":session, "idempotency_key":"contract-undo"}),
    );
    execute(json!({"operation":"history.list", "project":path}));
    execute(json!({"operation":"scene.query", "project":path, "composition":composition}));
    execute(
        json!({"operation":"property.sample", "project":path, "composition":composition,
        "keys":[{"kind":"node", "instance_path":[], "node":node, "property":property}],
        "times":[{"num":"1", "den":"2"}]}),
    );
    execute(json!({"operation":"capabilities.get"}));
    let input = json!({"project":path, "composition":composition,
        "region":{"origin":[0.0,0.0], "extent":[64.0,32.0], "pixels":[8,4]}});
    execute(json!({"operation":"render.frame", "input":input, "time":{"num":"0", "den":"1"}}));
    execute(json!({"operation":"render.sequence", "input":input,
        "range":{"start":{"num":"0","den":"1"}, "end":{"num":"1","den":"1"}},
        "frame_rate":{"num":"1","den":"1"}, "output_directory":dir.path().join("frames")}));
    assert_eq!(
        checked,
        command_registry().into_iter().map(|c| c.name).collect()
    );
}

#[test]
fn all_filesystem_boundaries_reject_uris_before_access() {
    let invalid_locator = |request: Json| {
        // These are valid request shapes: rejection must come from the local
        // filesystem policy, not from an unrelated deserialization error.
        let typed: Request = serde_json::from_str(&request.to_string()).unwrap();
        let error = service().dispatch(typed).unwrap_err();
        assert_eq!(error.code, "INVALID_REQUEST", "{error:?}");
        assert!(
            error.message.contains("local filesystem paths"),
            "{error:?}"
        );
    };
    let document = serde_json::to_value(fixture()).unwrap();
    let composition = &document["compositions"][0]["id"];
    let input = json!({"project":"missing.kronello", "composition":composition,
        "region":{"origin":[0.0,0.0],"extent":[64.0,32.0],"pixels":[8,4]}});
    let time = json!({"num":"0","den":"1"});
    let mut font_input = input.clone();
    font_input["fonts"] = json!([{"identity":document["texts"][0]["styles"][0]["font"], "path":"https://example.invalid/font.otf"}]);
    invalid_locator(json!({"operation":"render.frame", "input":font_input, "time":time}));
    let mut project_input = input.clone();
    project_input["project"] = json!("https://example.invalid/project.kronello");
    invalid_locator(json!({"operation":"render.frame", "input":project_input, "time":time}));
    invalid_locator(
        json!({"operation":"render.sequence", "input":input, "range":{"start":time,"end":{"num":"1","den":"1"}},
        "frame_rate":{"num":"1","den":"1"}, "output_directory":"https://example.invalid/output"}),
    );
    for slot in [
        "relative",
        "absolute",
        "relative_path",
        "absolute_path",
        "locator",
    ] {
        let mut document = document.clone();
        document["assets"] = json!([{"locator":{slot:"https://example.invalid/movie.mp4"}}]);
        for operation in ["project.create", "project.import"] {
            let mut request =
                json!({"operation":operation, "project":"missing.kronello", "document":document});
            if operation == "project.import" {
                request["base_revision"] = json!("0");
            }
            invalid_locator(request);
        }
    }
}
