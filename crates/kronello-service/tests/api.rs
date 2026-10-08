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
            plan_hash: None,
            idempotency_key: None,
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
            search: Default::default(),
            limit: None,
            cursor: None,
            evaluation: None,
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
            tags: Default::default(),
            name: None,
            enabled: true,
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
            effects: vec![],
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
    // API-002 pages keep repeated definitions distinct by complete runtime key.
    let mut request = json!({"operation":"scene.query","project":path,"composition":root_id,
        "expand_instances":true,"limit":1,"search":{"kinds":["shape"]}});
    let first = serde_json::to_value(service().execute_json(&request.to_string())).unwrap();
    request["cursor"] = first["result"]["value"]["next_cursor"].clone();
    let second = serde_json::to_value(service().execute_json(&request.to_string())).unwrap();
    assert_eq!(
        first["result"]["value"]["nodes"][0]["key"]["node"],
        second["result"]["value"]["nodes"][0]["key"]["node"]
    );
    assert_ne!(
        first["result"]["value"]["nodes"][0]["key"]["instance_path"],
        second["result"]["value"]["nodes"][0]["key"]["instance_path"]
    );
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
        fonts: None,
        luts: None,
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
    let media: MediaCapabilities = kronello_media::MediaCapabilities {
        schema_version: 1,
        ffmpeg_version: "test-runtime".into(),
        library_directory: "test-libraries".into(),
        substituted: true,
        libraries: vec![kronello_media::LibraryCapability {
            name: "avcodec".into(),
            version: 1,
            license: "LGPL".into(),
            configuration: "--disable-gpl".into(),
        }],
        distribution_eligible: true,
        development_only: false,
        codecs: vec![kronello_media::CodecCapability {
            name: "prores".into(),
            encoder: true,
            decoder: true,
            hardware: false,
        }],
        hwaccels: vec!["videotoolbox".into()],
    }
    .into();
    let ResultData::Capabilities(c) = success(
        service()
            .with_media_capabilities(media)
            .execute_json(r#"{"operation":"capabilities.get"}"#),
    ) else {
        panic!()
    };
    assert_eq!(c.commands.len(), command_registry().len());
    assert!(
        c.commands
            .iter()
            .any(|command| command.name == "job.resume")
    );
    assert_eq!(c.api_schema_version, 1);
    assert_eq!(c.semantic_versions.document, PROJECT_SEMANTIC_VERSION);
    let media = c.media.unwrap();
    assert_eq!(media.hwaccels, vec!["videotoolbox"]);
    assert_eq!(media.runtime_version, "test-runtime");
    assert_eq!(media.ffmpeg_version, media.runtime_version);
    assert_eq!(media.encoders, vec!["prores"]);
    assert_eq!(media.decoders, vec!["prores"]);
    assert_eq!(media.libraries[0].license, "LGPL");
    assert!(media.substituted && media.distribution_eligible && !media.development_only);
    assert_eq!(
        c.effects,
        [
            "kronello.gaussian_blur",
            "kronello.drop_shadow",
            "kronello.audio.gain",
            "kronello.audio.eq",
            "kronello.audio.hpf",
            "kronello.audio.lpf",
            "kronello.audio.compressor",
            "kronello.audio.limiter",
            "kronello.color.exposure",
            "kronello.color.levels",
            "kronello.color.curves",
            "kronello.color.hsl",
            "kronello.color.lut",
        ]
    );
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
            "audio.analyze",
            "track.analyze",
            "proxy.clear",
            "audio.normalize",
            "sequence.create",
            "clip.place",
            "clip.trim",
            "clip.stretch",
            "clip.angle_switch",
            "multicam.create",
            "edit.insert",
            "edit.overwrite",
            "instance.retime",
            "template_instance.retime",
            "project.create",
            "project.import",
            "asset.relink",
            "edit.apply",
            "edit.undo",
            "template.define",
            "template.instantiate",
            "template.set_input",
            "template.set_duration",
            "captions.import",
            "lut.import"
        ]
    );
    let ResultData::Capabilities(c) =
        success(service().execute_json(r#"{"operation":"capabilities.get"}"#))
    else {
        panic!()
    };
    let actual = kronello_media::MediaRuntime::load()
        .unwrap()
        .capabilities()
        .clone();
    let reported = c.media.unwrap();
    assert_eq!(reported.ffmpeg_version, actual.ffmpeg_version);
    assert_eq!(reported.libraries, actual.libraries);
    assert_eq!(reported.codecs, actual.codecs);
    assert_eq!(reported.hwaccels, actual.hwaccels);
    assert_eq!(reported.schema_version, actual.schema_version);
    assert_eq!(reported.library_directory, actual.library_directory);
    assert_eq!(reported.substituted, actual.substituted);
    assert_eq!(reported.distribution_eligible, actual.distribution_eligible);
    assert_eq!(reported.development_only, actual.development_only);
}

#[test]
fn public_schema_matches_and_all_registry_schemas_are_safe() {
    let generated = api_json_schema();
    if std::env::var_os("KRONELLO_UPDATE_API_SCHEMA").as_deref() == Some(std::ffi::OsStr::new("1"))
    {
        std::fs::write(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/api-v1.schema.json"),
            serde_json::to_string_pretty(&generated).unwrap() + "\n",
        )
        .unwrap();
        return;
    }
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
        for field in ["project", "search_directory"] {
            let mut request = json!({"operation":"asset.relink", "project":"local.kronello",
                "base_revision":"1", "asset":Uuid::new_v4(), "search_directory":"assets"});
            request[field] = json!(path);
            invalid(request);
        }
        for field in ["project", "output_directory"] {
            let mut request = json!({"operation":"project.collect", "project":"local.kronello",
                "output_directory":"collected"});
            request[field] = json!(path);
            invalid(request);
        }
        for field in ["relative", "absolute"] {
            let mut document = serde_json::to_value(fixture()).unwrap();
            let mut asset = json!({"id":Uuid::new_v4(), "content_hash":"a".repeat(64),
                "kind":"video", "streams":[], "locator":{"relative":null,"absolute":null}});
            asset["locator"][field] = json!(path);
            document["assets"] = json!([asset]);
            invalid(
                json!({"operation":"project.create", "project":"local.kronello", "document":document}),
            );
            invalid(
                json!({"operation":"project.import", "project":"local.kronello", "base_revision":"1", "document":document}),
            );
        }
    }
    let mut document = serde_json::to_value(fixture()).unwrap();
    document["assets"] =
        json!([{"id":Uuid::new_v4(), "locator":{"absolute":"https://example.invalid/movie.mp4"}}]);
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
                cursor: None,
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
    let definition: Json = serde_json::from_str(include_str!(
        "../../../examples/template-001.definition.json"
    ))
    .unwrap();
    let instance = json!({"id":uuid, "definition_ref":definition["id"], "version":"1.0.0", "duration":time, "inputs":{}});
    let sequence = json!({"id":Uuid::new_v4(), "extent":{"width":64.0,"height":32.0}, "frame_rate":{"num":"24","den":"1"}, "audio_rate":48000, "working_space":"linear_rec709", "tracks":[]});
    let clip = json!({"id":Uuid::new_v4(), "source_ref":{"kind":"composition","composition":composition}, "timeline_range":{"start":{"num":"0","den":"1"},"end":time}, "source_in":{"num":"0","den":"1"}, "time_map":{"kind":"linear","offset":{"num":"0","den":"1"},"speed":{"num":"1","den":"1"}}, "links":[],"effects":[]});
    let requests = vec![
        json!({"operation":"font.pin","path":"local.otf","face_index":0}),
        json!({"operation":"svg.inspect","svg":"<svg><path d='M0 0L1 1' fill='#abc'/></svg>"}),
        json!({"operation":"svg.export","paths":[]}),
        json!({"operation":"svg.import_plan","project":path,"base_revision":"1","composition":composition,"svg":"<svg/>","targets":[]}),
        json!({"operation":"audio.analyze", "project":path, "base_revision":"1", "id":uuid,
            "input":{"kind":"bus","target":{"kind":"composition","composition":composition},"range":clip["timeline_range"]},
            "config":{"version":1,"sample_rate":48000,"window":32,"hop":32,"bands":[],"time_map":clip["time_map"]}}),
        json!({"operation":"project.create_plan", "project":path, "document":p}),
        json!({"operation":"project.import_plan", "project":path, "base_revision":"1", "document":p}),
        json!({"operation":"sequence.query", "project":path,"sequence":uuid}),
        json!({"operation":"sequence.create", "project":path,"base_revision":"1","session_id":uuid,"idempotency_key":"seq","sequence":sequence}),
        json!({"operation":"clip.place", "project":path,"base_revision":"1","session_id":uuid,"idempotency_key":"clip","sequence":uuid,"track":uuid,"clip":clip}),
        json!({"operation":"clip.trim", "project":path,"base_revision":"1","session_id":uuid,"idempotency_key":"trim","sequence":uuid,"clip":uuid,"range":clip["timeline_range"]}),
        json!({"operation":"clip.stretch", "project":path,"base_revision":"1","session_id":uuid,"idempotency_key":"stretch","sequence":uuid,"clip":uuid,"range":clip["timeline_range"]}),
        json!({"operation":"clip.angle_switch", "project":path,"base_revision":"1","session_id":uuid,"idempotency_key":"angle","sequence":uuid,"clip":uuid,"angle":uuid}),
        json!({"operation":"multicam.create", "project":path,"base_revision":"1","session_id":uuid,"idempotency_key":"multicam","multicam":uuid,"sync":"manual",
            "angles":[{"id":"11111111-2222-4333-8444-555555555555","asset":uuid,"stream_index":0}],
            "offsets":{"11111111-2222-4333-8444-555555555555":{"num":"0","den":"1"}}}),
        json!({"operation":"edit.insert", "project":path,"base_revision":"1","session_id":uuid,"idempotency_key":"insert","sequence":uuid,"clip":uuid,
            "source":clip["source_ref"],"source_range":clip["timeline_range"],"at":{"num":"0","den":"1"}}),
        json!({"operation":"edit.overwrite", "project":path,"base_revision":"1","session_id":uuid,"idempotency_key":"overwrite","sequence":uuid,"clip":uuid,
            "source":clip["source_ref"],"source_range":clip["timeline_range"],"at":{"num":"0","den":"1"},"split_tail":uuid}),
        json!({"operation":"instance.retime", "project":path,"base_revision":"1","session_id":uuid,"idempotency_key":"retime","composition":composition,"node":uuid,"time_map":clip["time_map"]}),
        json!({"operation":"template_instance.retime", "project":path,"base_revision":"1","session_id":uuid,"idempotency_key":"retime-template","instance":uuid,"duration":time}),
        json!({"operation":"render.export", "render":{"input":input,"range":{"start":{"num":"0","den":"1"},"end":time},"frame_rate":{"num":"24","den":"1"},"output_directory":"movie.mov"},"output":{"format":"pro_res_mov","clips":[],"background":[0,0,0]}}),
        json!({"operation":"render.submit","render":{"input":input,"range":{"start":{"num":"0","den":"1"},"end":time},
            "frame_rate":{"num":"24","den":"1"},"output_directory":"frames"}}),
        json!({"operation":"job.get","job":uuid.to_string()}),
        json!({"operation":"job.cancel","job":uuid.to_string()}),
        json!({"operation":"job.resume","job":uuid.to_string()}),
        json!({"operation":"job.list"}),
        json!({"operation":"job.prune"}),
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
        json!({"operation":"expression.format", "project":path, "expression_id":uuid}),
        json!({"operation":"history.list", "project":path}),
        json!({"operation":"scene.query", "project":path, "composition":composition}),
        json!({"operation":"node.explain", "project":path, "composition":composition,"key":{"instance_path":[],"node":uuid},"time":time}),
        json!({"operation":"render.explain", "input":input, "time":time}),
        json!({"operation":"property.sample", "project":path, "composition":composition, "keys":[
            {"kind":"node", "instance_path":[], "node":comp(&p).nodes[0].id,"property":comp(&p).nodes[0].properties[0].id()}], "times":[time]}),
        json!({"operation":"capabilities.get"}),
        json!({"operation":"asset.relink", "project":path, "base_revision":"1", "asset":uuid, "search_directory":"assets"}),
        json!({"operation":"project.collect", "project":path, "output_directory":"collected"}),
        json!({"operation":"template.define", "project":path, "base_revision":"1", "session_id":uuid, "idempotency_key":"define", "definition":definition}),
        json!({"operation":"template.instantiate", "project":path, "base_revision":"1", "session_id":uuid, "idempotency_key":"place", "composition":composition, "node":uuid, "index":0, "instance":instance}),
        json!({"operation":"template.set_input", "project":path, "base_revision":"1", "session_id":uuid, "idempotency_key":"input", "instance":uuid, "name":"headline", "value":{"kind":"string", "value":"text"}}),
        json!({"operation":"template.set_duration", "project":path, "base_revision":"1", "session_id":uuid, "idempotency_key":"duration", "instance":uuid, "duration":time}),
        json!({"operation":"template.preview","project":path,"instance":instance,"time":time,"fonts":[]}),
        json!({"operation":"template.migration_plan","project":path,"base_revision":"1","instance":uuid,"definition":definition["id"],"time":time,"fonts":[]}),
        json!({"operation":"captions.import_plan","project":path,"base_revision":"1","sequence":uuid,"track":uuid,"format":"srt",
            "content":"1\n00:00:01,000 --> 00:00:02,000\ncue\n",
            "style":{"font":{"family":"TestSans","postscript_name":"TestSans-Regular","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","face_index":0},"size":24.0,"fill":{"space":"srgb","components":{"r":1.0,"g":1.0,"b":1.0,"alpha":1.0}}},
            "cue_ids":[{"caption":uuid,"clip":uuid}]}),
        json!({"operation":"captions.import","session_id":uuid,"idempotency_key":"captions",
            "plan":{"project":path,"base_revision":"1","sequence":uuid,"track":uuid,"format":"srt",
            "content":"1\n00:00:01,000 --> 00:00:02,000\ncue\n",
            "style":{"font":{"family":"TestSans","postscript_name":"TestSans-Regular","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","face_index":0},"size":24.0,"fill":{"space":"srgb","components":{"r":1.0,"g":1.0,"b":1.0,"alpha":1.0}}},
            "cue_ids":[{"caption":uuid,"clip":uuid}]}}),
        json!({"operation":"captions.export","project":path,"sequence":uuid,"format":"vtt"}),
        json!({"operation":"track.analyze","project":path,"base_revision":"1","id":uuid,
            "asset":uuid,"stream_index":0,"mode":"points",
            "seeds":[{"x":0.5,"y":0.5,"template_radius":8,"search_radius":16}],
            "range":{"start":{"num":"0","den":"1"},"end":time}}),
        json!({"operation":"proxy.generate","project":path,"assets":[uuid],"scale":0.5}),
        json!({"operation":"proxy.status","project":path}),
        json!({"operation":"proxy.clear","project":path,"base_revision":"1","asset":uuid}),
        json!({"operation":"audio.loudness","project":path,"base_revision":"1",
            "input":{"kind":"sequence","sequence":uuid}}),
        json!({"operation":"audio.normalize","project":path,"base_revision":"1",
            "session_id":uuid,"idempotency_key":"normalize","sequence":uuid,"clip":uuid,"target_lufs":-16.0}),
        json!({"operation":"lut.import","project":path,"base_revision":"1","session_id":uuid,"idempotency_key":"lut","path":"a.cube","asset":uuid}),
        json!({"operation":"inspect.scopes","input":input,"time":time}),
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
        for field in [
            "shell",
            "exec",
            "script",
            "url",
            "fetch_url",
            "ffmpeg_args",
            "raw_ffmpeg_args",
            "command_line",
        ] {
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
        tags: Default::default(),
        name: None,
        enabled: true,
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
        effects: vec![],
    });
    let root_id = root.id;
    p.compositions.push(DocumentObject::Known(root));
    let (_dir, path) = setup(p);
    let ResultData::Samples(result) = service()
        .dispatch(Request::PropertySample(PropertySampleRequest {
            fonts: None,
            luts: None,
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
            fonts: None,
            luts: None,
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
            fonts: None,
            luts: None,
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
fn missing_expression_and_enabled_modifier_fail_without_partial_samples() {
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
                fonts: None,
                luts: None,
                project: path,
                composition,
                times: vec![Time::ZERO, Time::new(1, 2).unwrap()],
                keys: vec![key],
            }))
            .unwrap_err();
        assert_eq!(
            error.code,
            if expression {
                "EVALUATION_ERROR"
            } else {
                "UNSUPPORTED_FEATURE"
            }
        );
    }
}

#[test]
fn actual_results_for_every_command_match_envelope_and_registry_schemas() {
    let schema = api_json_schema();
    let envelope = jsonschema::validator_for(&schema).unwrap();
    let job_state = tempfile::tempdir().unwrap();
    let engine = Service::new(BackendSelection::CpuReference)
        .with_job_config(kronello_jobs::JobConfig::at(job_state.path()))
        // This test covers submit's successful wire result. Actual detached
        // execution and completion are checked by CLI/MCP process integration.
        .with_worker_executable(PathBuf::from("/usr/bin/false"));
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
    let retime_composition = kronello_model::CompositionId::new();
    let retime_node = kronello_model::NodeId::new();
    document.compositions.push(kronello_model::DocumentObject::Known(serde_json::from_value(json!({
        "id":retime_composition, "duration":{"num":"2","den":"1"}, "design_extent":{"width":64.0,"height":32.0}, "edit_rate":{"num":"24","den":"1"},
        "root_nodes":[retime_node], "properties":[], "nodes":[{"id":retime_node,"kind":{"kind":"composition_instance","value":{"id":Uuid::new_v4(),"definition_ref":composition,"input_bindings":{},"local_time_map":{"kind":"linear","offset":{"num":"0","den":"1"},"speed":{"num":"1","den":"1"}},"seed":0}},"containment_parent":null,"transform_parent":null,"child_order":[],"active_range":{"start":{"num":"0","den":"1"},"end":{"num":"2","den":"1"}},"properties":[]}]
    })).unwrap()));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("contracts.kronello");
    execute(json!({"operation":"project.create_plan", "project":path, "document":document}));
    execute(json!({"operation":"project.create", "project":path, "document":document}));
    execute(
        json!({"operation":"project.import_plan", "project":path, "base_revision":"1", "document":document}),
    );
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
    execute(json!({"operation":"expression.format", "project":path,
        "expression":{"id":Uuid::new_v4(), "version":1, "value_type":"scalar",
        "nodes":[{"literal":{"kind":"scalar","value":0.5}}]}}));
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
    execute(json!({"operation":"render.explain", "input":input, "time":{"num":"0", "den":"1"}}));
    execute(
        json!({"operation":"node.explain", "project":path, "composition":composition,
        "key":{"instance_path":[],"node":node}, "time":{"num":"0", "den":"1"}}),
    );
    execute(json!({"operation":"render.sequence", "input":input,
        "range":{"start":{"num":"0","den":"1"}, "end":{"num":"1","den":"1"}},
        "frame_rate":{"num":"1","den":"1"}, "output_directory":dir.path().join("frames")}));
    execute(json!({"operation":"render.export", "render":{"input":input,
        "range":{"start":{"num":"0","den":"1"},"end":{"num":"1","den":"24"}},
        "frame_rate":{"num":"24","den":"1"},"output_directory":dir.path().join("sync.mov")},
        "output":{"format":"pro_res_mov","clips":[],"background":[0,0,0]}}));
    let job = execute(json!({"operation":"render.submit", "render":{"input":input,
        "range":{"start":{"num":"0","den":"1"},"end":{"num":"1","den":"1"}},
        "frame_rate":{"num":"1","den":"1"},"output_directory":dir.path().join("job-frames")}}));
    // Save a real fixed input as a failed, unclaimed job, then exercise the
    // service's validation and detached-worker launch through job.resume.
    let original: kronello_jobs::JobRecord = serde_json::from_value(job.clone()).unwrap();
    let jobs =
        kronello_jobs::JobStore::open(kronello_jobs::JobConfig::at(job_state.path())).unwrap();
    let retry = jobs
        .submit(
            &jobs.input(&original).unwrap(),
            kronello_jobs::Submission {
                engine_version: original.engine_version.clone(),
                project_id: original.project_id.clone(),
                revision: original.revision.clone(),
                snapshot_hash: original.snapshot_hash.clone(),
                output_profile: original.output_profile.clone(),
                destination: original.destination.clone(),
                total_frames: original.total_frames,
            },
        )
        .unwrap();
    jobs.finish_error(
        &retry.id,
        &kronello_jobs::JobError::new("TEST_FAILURE", "retry fixture"),
    )
    .unwrap();
    let resumed = execute(json!({"operation":"job.resume","job":retry.id}));
    assert_eq!(resumed["attempt"], 1);
    assert_eq!(resumed["input_hash"], retry.input_hash);
    assert_eq!(resumed["status"], "queued");
    execute(json!({"operation":"job.get","job":job["id"]}));
    execute(json!({"operation":"job.cancel","job":job["id"]}));
    execute(json!({"operation":"job.list"}));
    execute(json!({"operation":"job.prune"}));
    let template_document: Json =
        serde_json::from_str(include_str!("../../../examples/template-001.project.json")).unwrap();
    let definition: Json = serde_json::from_str(include_str!(
        "../../../examples/template-001.definition.json"
    ))
    .unwrap();
    let template_path = dir.path().join("template-contracts.kronello");
    execute(
        json!({"operation":"project.create", "project":template_path, "document":template_document}),
    );
    execute(
        json!({"operation":"template.define", "project":template_path, "base_revision":"1", "session_id":session, "idempotency_key":"define", "definition":definition}),
    );
    let instance = Uuid::new_v4();
    execute(
        json!({"operation":"template.instantiate", "project":template_path, "base_revision":"2", "session_id":session, "idempotency_key":"place", "composition":template_document["compositions"][0]["id"], "node":Uuid::new_v4(), "index":0, "instance":{"id":instance, "definition_ref":definition["id"], "version":"1.0.0", "duration":{"num":"5", "den":"1"}, "inputs":{}}}),
    );
    execute(
        json!({"operation":"template.set_input", "project":template_path, "base_revision":"3", "session_id":session, "idempotency_key":"input", "instance":instance, "name":"headline", "value":{"kind":"string", "value":"日本語"}}),
    );
    execute(
        json!({"operation":"template.set_duration", "project":template_path, "base_revision":"4", "session_id":session, "idempotency_key":"duration", "instance":instance, "duration":{"num":"8", "den":"1"}}),
    );
    execute(
        json!({"operation":"template_instance.retime","project":template_path,"base_revision":"5","session_id":session,"idempotency_key":"retime-template","instance":instance,"duration":{"num":"6","den":"1"}}),
    );
    execute(
        json!({"operation":"font.pin","path":std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures/external/NotoSansCJKjp-Regular.otf"),"face_index":0}),
    );
    let template_fonts = json!([{"identity":template_document["texts"][0]["styles"][0]["font"],
        "path":std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures/external/NotoSansCJKjp-Regular.otf")}]);
    execute(
        json!({"operation":"template.preview","project":template_path,
        "instance":{"id":instance,"definition_ref":definition["id"],"version":"1.0.0","duration":{"num":"6","den":"1"},"inputs":{}},
        "time":{"num":"1","den":"1"},"fonts":template_fonts}),
    );
    execute(
        json!({"operation":"template.migration_plan","project":template_path,"base_revision":"6",
        "instance":instance,"definition":definition["id"],"time":{"num":"1","den":"1"},"fonts":template_fonts}),
    );
    let sequence_id = Uuid::new_v4();
    let track_id = Uuid::new_v4();
    let clip_id = Uuid::new_v4();
    execute(
        json!({"operation":"sequence.create","project":path,"base_revision":"4","session_id":session,"idempotency_key":"create-seq","sequence":{"id":sequence_id,"extent":{"width":64.0,"height":32.0},"frame_rate":{"num":"24","den":"1"},"audio_rate":48000,"working_space":"linear_rec709","tracks":[{"id":track_id,"kind":"video","clips":[]}]}}),
    );
    execute(
        json!({"operation":"clip.place","project":path,"base_revision":"5","session_id":session,"idempotency_key":"place-clip","sequence":sequence_id,"track":track_id,"clip":{"id":clip_id,"source_ref":{"kind":"composition","composition":composition},"timeline_range":{"start":{"num":"0","den":"1"},"end":{"num":"2","den":"1"}},"source_in":{"num":"0","den":"1"},"time_map":{"kind":"linear","offset":{"num":"0","den":"1"},"speed":{"num":"1","den":"1"}},"links":[],"effects":[]}}),
    );
    execute(json!({"operation":"sequence.query","project":path,"sequence":sequence_id}));
    execute(
        json!({"operation":"clip.trim","project":path,"base_revision":"6","session_id":session,"idempotency_key":"trim-clip","sequence":sequence_id,"clip":clip_id,"range":{"start":{"num":"1","den":"4"},"end":{"num":"7","den":"4"}}}),
    );
    execute(
        json!({"operation":"clip.stretch","project":path,"base_revision":"7","session_id":session,"idempotency_key":"stretch-clip","sequence":sequence_id,"clip":clip_id,"range":{"start":{"num":"1","den":"4"},"end":{"num":"9","den":"4"}}}),
    );
    execute(
        json!({"operation":"instance.retime","project":path,"base_revision":"8","session_id":session,"idempotency_key":"retime-instance","composition":retime_composition,"node":retime_node,"time_map":{"kind":"linear","offset":{"num":"0","den":"1"},"speed":{"num":"1","den":"2"}}}),
    );
    execute(
        json!({"operation":"audio.analyze", "project":path, "base_revision":"9", "id":Uuid::new_v4(),
        "input":{"kind":"bus","target":{"kind":"composition","composition":composition},"range":{"start":{"num":"0","den":"1"},"end":{"num":"1","den":"100"}}},
        "config":{"version":1,"sample_rate":48000,"window":32,"hop":32,"bands":[],"time_map":{"kind":"linear","offset":{"num":"0","den":"1"},"speed":{"num":"1","den":"1"}}}}),
    );
    // Captions share the same plan/apply path: plan is read-only, import
    // applies an ordinary edit, export reserializes stored cue documents.
    let caption_style = json!({"font":{"family":"TestSans","postscript_name":"TestSans-Regular",
        "sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","face_index":0},
        "size":24.0,"fill":{"space":"srgb","components":{"r":1.0,"g":1.0,"b":1.0,"alpha":1.0}}});
    let srt = "1\n00:00:01,000 --> 00:00:02,000\nこんにちは\n";
    let caption_ids = json!([{"caption":Uuid::new_v4(),"clip":Uuid::new_v4()}]);
    execute(
        json!({"operation":"captions.import_plan","project":path,"base_revision":"10",
            "sequence":sequence_id,"track":Uuid::new_v4(),"format":"srt","content":srt,
            "style":caption_style,"cue_ids":caption_ids}),
    );
    execute(
        json!({"operation":"captions.import","session_id":session,"idempotency_key":"captions",
            "plan":{"project":path,"base_revision":"10","sequence":sequence_id,
            "track":Uuid::new_v4(),"format":"srt","content":srt,
            "style":caption_style,"cue_ids":caption_ids}}),
    );
    execute(
        json!({"operation":"captions.export","project":path,"sequence":sequence_id,"format":"srt"}),
    );
    // AUDIO-008: a Generator clip exercises loudness and normalization through
    // the shared audio plan and edit pipeline without media files.
    let audio_sequence = Uuid::new_v4();
    let audio_track = Uuid::new_v4();
    let audio_clip = Uuid::new_v4();
    execute(
        json!({"operation":"sequence.create","project":path,"base_revision":"11","session_id":session,"idempotency_key":"create-audio-seq","sequence":{"id":audio_sequence,"extent":{"width":64.0,"height":32.0},"frame_rate":{"num":"24","den":"1"},"audio_rate":48000,"working_space":"linear_rec709","tracks":[{"id":audio_track,"kind":"audio","clips":[]}]}}),
    );
    execute(
        json!({"operation":"clip.place","project":path,"base_revision":"12","session_id":session,"idempotency_key":"place-audio-clip","sequence":audio_sequence,"track":audio_track,"clip":{"id":audio_clip,"source_ref":{"kind":"generator","generator":"kronello.audio.tone440"},"timeline_range":{"start":{"num":"0","den":"1"},"end":{"num":"2","den":"1"}},"source_in":{"num":"0","den":"1"},"time_map":{"kind":"linear","offset":{"num":"0","den":"1"},"speed":{"num":"1","den":"1"}},"links":[],"effects":[]}}),
    );
    execute(
        json!({"operation":"audio.loudness","project":path,"base_revision":"13",
            "input":{"kind":"sequence","sequence":audio_sequence}}),
    );
    execute(
        json!({"operation":"audio.loudness","project":path,"base_revision":"13",
            "input":{"kind":"clip","sequence":audio_sequence,"clip":audio_clip}}),
    );
    execute(
        json!({"operation":"audio.normalize","project":path,"base_revision":"13",
            "session_id":session,"idempotency_key":"normalize","sequence":audio_sequence,
            "clip":audio_clip,"target_lufs":-20.0}),
    );
    let report = execute(
        json!({"operation":"svg.inspect","svg":"<svg><path d='M0 0L10 0L10 10Z' fill='#abc'/></svg>"}),
    );
    execute(json!({"operation":"svg.export","paths":report["paths"]}));
    let svg_path = dir.path().join("svg-contracts.kronello");
    let mut svg_document = fixture();
    let DocumentObject::Known(svg_composition) = &mut svg_document.compositions[0] else {
        panic!()
    };
    let mut svg_node = svg_composition.nodes[0].clone();
    svg_node.id = NodeId::new();
    svg_node.properties.clear();
    svg_node.containment_parent = None;
    svg_node.transform_parent = None;
    svg_node.child_order.clear();
    let svg_shape = ContentId::new();
    svg_node.kind = NodeKind::Shape {
        content_ref: svg_shape,
    };
    let svg_composition_id = svg_composition.id;
    svg_composition.nodes.clear();
    svg_composition.root_nodes.clear();
    svg_document.shapes.clear();
    svg_document.texts.clear();
    let created =
        execute(json!({"operation":"project.create","project":svg_path,"document":svg_document}));
    execute(
        json!({"operation":"svg.import_plan","project":svg_path,"base_revision":created["revision"],"composition":svg_composition_id,
        "svg":"<svg><path d='M0 0L10 0L10 10Z' fill='#abc'/></svg>",
        "targets":[{"shape":svg_shape,"path_property":PropertyId::new(),"fill_property":PropertyId::new(),"node":svg_node,"index":0}]}),
    );
    let media_path = dir.path().join("media-contracts.kronello");
    let source = dir.path().join("asset.bin");
    std::fs::write(&source, b"media contract").unwrap();
    let asset = Asset {
        id: AssetId::new(),
        content_hash: kronello_media::content_hash(&source).unwrap(),
        kind: AssetKind::Video,
        streams: vec![],
        locator: AssetLocator {
            relative: Some("asset.bin".into()),
            absolute: None,
        },
    };
    let media_document = Project {
        assets: vec![DocumentObject::Known(asset.clone())],
        ..Project::default()
    };
    execute(json!({"operation":"project.create", "project":media_path, "document":media_document}));
    let search = dir.path().join("search");
    std::fs::create_dir(&search).unwrap();
    std::fs::rename(source, search.join("renamed.bin")).unwrap();
    execute(
        json!({"operation":"asset.relink", "project":media_path, "base_revision":"1", "asset":asset.id, "search_directory":search}),
    );
    execute(
        json!({"operation":"project.collect", "project":media_path, "output_directory":dir.path().join("collected")}),
    );
    // A real tiny clip backs the tracking and proxy command contracts. The
    // proxy link is registered directly in the authored document; generation
    // itself is exercised through `proxy.generate` against the stub worker.
    let clip_path = dir.path().join("clip.mov");
    let runtime = kronello_media::MediaRuntime::load().unwrap();
    runtime
        .encode_video_stream(
            &kronello_media::EncodeRequest {
                output: clip_path.clone(),
                codec: kronello_media::EncodeCodec::ProRes,
                width: 16,
                height: 16,
                time_base: kronello_time::Rational::new(1, 24).unwrap(),
            },
            2,
            &mut |index| {
                let mut rgba = Vec::with_capacity(16 * 16 * 4);
                for p in 0..256u32 {
                    let (x, y) = (p % 16, p / 16);
                    rgba.extend_from_slice(&[
                        ((x * 36 + y * 11 + index as u32 * 3) % 256) as u8,
                        ((x * 5 + y * 29) % 256) as u8,
                        ((x * 17 + y * 7) % 256) as u8,
                        255,
                    ]);
                }
                Ok(kronello_media::EncodeFrame {
                    pts: kronello_time::Rational::new(index as i64, 24).unwrap(),
                    rgba,
                })
            },
        )
        .unwrap();
    let clip_stream = runtime
        .open_video_stream(&clip_path, 0)
        .unwrap()
        .stream_metadata()
        .unwrap();
    let video_id = AssetId::new();
    let proxy_id = AssetId::new();
    let clip_hash = kronello_media::content_hash(&clip_path).unwrap();
    let mut proxy_stream = clip_stream.clone();
    proxy_stream.index = 0;
    proxy_stream.width = Some(8);
    proxy_stream.height = Some(8);
    let video_document = Project {
        assets: vec![
            DocumentObject::Known(Asset {
                id: video_id,
                content_hash: clip_hash.clone(),
                kind: AssetKind::Video,
                streams: vec![clip_stream],
                locator: AssetLocator {
                    relative: Some("clip.mov".into()),
                    absolute: None,
                },
            }),
            DocumentObject::Known(Asset {
                id: proxy_id,
                content_hash: "0000000000000000000000000000000000000000000000000000000000000000"
                    .into(),
                kind: AssetKind::Video,
                streams: vec![proxy_stream],
                locator: AssetLocator {
                    relative: Some("clip.proxies/proxy.mov".into()),
                    absolute: None,
                },
            }),
        ],
        proxies: vec![ProxyLink {
            original: video_id,
            proxy: proxy_id,
            original_stream_index: 0,
            proxy_stream_index: 0,
            scale: FiniteF64::new(0.5).unwrap(),
            width: 8,
            height: 8,
            source_content_hash: clip_hash,
            source_duration: None,
            job: None,
        }],
        ..Project::default()
    };
    let video_path = dir.path().join("video-contracts.kronello");
    execute(json!({"operation":"project.create", "project":video_path, "document":video_document}));
    execute(
        json!({"operation":"track.analyze", "project":video_path, "base_revision":"1",
            "id":Uuid::new_v4(), "asset":video_id, "stream_index":0, "mode":"points",
            "seeds":[{"x":0.5,"y":0.5,"template_radius":4,"search_radius":4}],
            "range":{"start":{"num":"0","den":"1"},"end":{"num":"1","den":"12"}}}),
    );
    // The injected stub worker never runs; submission still returns a record.
    execute(
        json!({"operation":"proxy.generate", "project":video_path, "assets":[video_id], "scale":0.5}),
    );
    execute(json!({"operation":"proxy.status", "project":video_path}));
    execute(
        json!({"operation":"proxy.clear", "project":video_path, "base_revision":"2",
            "asset":video_id}),
    );
    // NLE-007/GUI-011 (ADR-0127/0128): multicam creation, per-clip angle
    // switching and shared three-point insert/overwrite all ride the same
    // plan/apply event pipeline against a real media asset.
    let mc_sequence = Uuid::new_v4();
    let mc_track = Uuid::new_v4();
    let multicam_id = Uuid::new_v4();
    let (angle_a, angle_b) = (Uuid::new_v4(), Uuid::new_v4());
    let mc_clip = Uuid::new_v4();
    let mut mc_offsets = serde_json::Map::new();
    for angle in [angle_a, angle_b] {
        mc_offsets.insert(angle.to_string(), json!({"num":"0","den":"1"}));
    }
    execute(
        json!({"operation":"sequence.create","project":video_path,"base_revision":"3","session_id":session,"idempotency_key":"create-mc-seq","sequence":{"id":mc_sequence,"extent":{"width":64.0,"height":32.0},"frame_rate":{"num":"24","den":"1"},"audio_rate":48000,"working_space":"linear_rec709","tracks":[{"id":mc_track,"kind":"video","clips":[]}]}}),
    );
    execute(
        json!({"operation":"multicam.create","project":video_path,"base_revision":"4","session_id":session,"idempotency_key":"multicam","multicam":multicam_id,"name":"Camera Group","sync":"manual",
            "angles":[{"id":angle_a,"asset":video_id,"stream_index":0},{"id":angle_b,"asset":video_id,"stream_index":0}],
            "offsets":mc_offsets}),
    );
    execute(
        json!({"operation":"clip.place","project":video_path,"base_revision":"5","session_id":session,"idempotency_key":"place-mc-clip","sequence":mc_sequence,"track":mc_track,"clip":{"id":mc_clip,"source_ref":{"kind":"multicam","multicam":multicam_id,"angle":angle_a},"timeline_range":{"start":{"num":"0","den":"1"},"end":{"num":"1","den":"24"}},"source_in":{"num":"0","den":"1"},"time_map":{"kind":"linear","offset":{"num":"0","den":"1"},"speed":{"num":"1","den":"1"}},"links":[],"effects":[]}}),
    );
    execute(
        json!({"operation":"clip.angle_switch","project":video_path,"base_revision":"6","session_id":session,"idempotency_key":"angle-switch","sequence":mc_sequence,"clip":mc_clip,"angle":angle_b}),
    );
    execute(
        json!({"operation":"edit.insert","project":video_path,"base_revision":"7","session_id":session,"idempotency_key":"insert-clip","sequence":mc_sequence,"clip":Uuid::new_v4(),
            "source":{"kind":"asset","asset":video_id,"stream_index":0},"source_range":{"start":{"num":"0","den":"1"},"end":{"num":"1","den":"24"}},"at":{"num":"1","den":"24"},"track":mc_track}),
    );
    execute(
        json!({"operation":"edit.overwrite","project":video_path,"base_revision":"8","session_id":session,"idempotency_key":"overwrite-clip","sequence":mc_sequence,"clip":Uuid::new_v4(),
            "source":{"kind":"asset","asset":video_id,"stream_index":0},"source_range":{"start":{"num":"0","den":"1"},"end":{"num":"1","den":"24"}},"at":{"num":"0","den":"1"},"track":mc_track}),
    );
    // COLOR-003/004: import a `.cube` as a hash-pinned Data asset, then query
    // deterministic scope bins over the composited frame.
    let cube = dir.path().join("contract.cube");
    std::fs::write(
        &cube,
        "LUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n",
    )
    .unwrap();
    let revision =
        execute(json!({"operation":"project.export","project":path}))["revision"].clone();
    execute(
        json!({"operation":"lut.import","project":path,"base_revision":revision,
        "session_id":session,"idempotency_key":"lut","path":cube,"asset":Uuid::new_v4()}),
    );
    execute(json!({"operation":"inspect.scopes","input":input,"time":{"num":"0","den":"1"}}));
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
    let definition: Json = serde_json::from_str(include_str!(
        "../../../examples/template-001.definition.json"
    ))
    .unwrap();
    let uuid = Uuid::new_v4();
    let project = "https://example.invalid/template.kronello";
    for request in [
        json!({"operation":"template.define", "project":project, "base_revision":"1", "session_id":uuid, "idempotency_key":"define", "definition":definition}),
        json!({"operation":"template.instantiate", "project":project, "base_revision":"1", "session_id":uuid, "idempotency_key":"place", "composition":composition, "node":uuid, "index":0, "instance":{"id":uuid,"definition_ref":definition["id"],"version":"1.0.0","duration":{"num":"5","den":"1"},"inputs":{}}}),
        json!({"operation":"template.set_input", "project":project, "base_revision":"1", "session_id":uuid, "idempotency_key":"input", "instance":uuid, "name":"headline", "value":{"kind":"string", "value":"text"}}),
        json!({"operation":"template.set_duration", "project":project, "base_revision":"1", "session_id":uuid, "idempotency_key":"duration", "instance":uuid, "duration":{"num":"5","den":"1"}}),
        json!({"operation":"template.preview","project":project,"instance":{"id":uuid,"definition_ref":definition["id"],"version":"1.0.0","duration":{"num":"5","den":"1"},"inputs":{}},"time":time,"fonts":[]}),
        json!({"operation":"template.migration_plan","project":project,"base_revision":"1","instance":uuid,"definition":definition["id"],"time":time,"fonts":[]}),
    ] {
        invalid_locator(request);
    }
    for uri in [
        "https://example.invalid/media",
        "file:///tmp/media",
        "pipe:0",
    ] {
        invalid_locator(
            json!({"operation":"asset.relink", "project":uri, "base_revision":"1", "asset":uuid, "search_directory":"local"}),
        );
        invalid_locator(
            json!({"operation":"asset.relink", "project":"missing.kronello", "base_revision":"1", "asset":uuid, "search_directory":uri}),
        );
        invalid_locator(
            json!({"operation":"project.collect", "project":uri, "output_directory":"local"}),
        );
        invalid_locator(
            json!({"operation":"project.collect", "project":"missing.kronello", "output_directory":uri}),
        );
    }
    let unsafe_fonts = json!([{"identity":document["texts"][0]["styles"][0]["font"],"path":"https://example.invalid/font.otf"}]);
    invalid_locator(
        json!({"operation":"template.preview","project":"missing.kronello",
        "instance":{"id":uuid,"definition_ref":definition["id"],"version":"1.0.0","duration":{"num":"5","den":"1"},"inputs":{}},"time":time,"fonts":unsafe_fonts}),
    );
    invalid_locator(
        json!({"operation":"template.migration_plan","project":"missing.kronello","base_revision":"1",
        "instance":uuid,"definition":definition["id"],"time":time,"fonts":unsafe_fonts}),
    );
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
        document["assets"] =
            json!([{"id":Uuid::new_v4(), "locator":{slot:"https://example.invalid/movie.mp4"}}]);
        for operation in [
            "project.create",
            "project.import",
            "project.create_plan",
            "project.import_plan",
        ] {
            let mut request =
                json!({"operation":operation, "project":"missing.kronello", "document":document});
            if operation == "project.import" || operation == "project.import_plan" {
                request["base_revision"] = json!("0");
            }
            invalid_locator(request);
        }
    }
}

#[test]
fn temporal_profile_reaches_shared_frame_sequence_and_fixed_job_input() {
    let mut p = fixture();
    p.texts.clear();
    for c in &mut p.compositions {
        if let DocumentObject::Known(c) = c {
            c.nodes.retain(|n| !matches!(n.kind, NodeKind::Text { .. }));
            c.root_nodes
                .retain(|id| c.nodes.iter().any(|n| n.id == *id));
        }
    }
    let c = comp(&p).id;
    let (_dir, path) = setup(p);
    let service = Service::new(BackendSelection::CpuReference);
    let settings = json!({"frame_rate":{"num":"24","den":"1"},"shutter_angle":{"num":"180","den":"1"},"shutter_phase":{"num":"-1","den":"4"},"samples":2,"cut_policy":"avoid_crossing"});
    let input = json!({"project":path,"composition":c,"region":{"origin":[0,0],"extent":[64,32],"pixels":[4,2]},"profile":{"working_space":"linear_rec709","flatten_tolerance_px":0.02,"temporal":settings}});
    let response = service.execute_json(
        &json!({"operation":"render.frame","input":input,"time":{"num":"1","den":"2"}}).to_string(),
    );
    let result = success(response);
    let ResultData::Frame(frame) = result else {
        panic!("expected frame");
    };
    assert_eq!(frame.metadata.temporal.as_ref().unwrap().samples.len(), 2);
    let out = _dir.path().join("temporal-sequence");
    let render = json!({"input":input,"range":{"start":{"num":"0","den":"1"},"end":{"num":"1","den":"24"}},"frame_rate":{"num":"24","den":"1"},"output_directory":out});
    let mut sequence_request = render.clone();
    sequence_request["operation"] = json!("render.sequence");
    let response = service.execute_json(&sequence_request.to_string());
    let ResultData::Sequence(sequence) = success(response) else {
        panic!("expected sequence");
    };
    assert!(sequence.frames[0].metadata.temporal.is_some());
    // Job submission serializes the same profile without a job-only shutter model.
    let submit: RenderSubmitRequest =
        serde_json::from_value(json!({"render":render,"output":{"format":"image_sequence"}}))
            .unwrap();
    assert_eq!(submit.render.input.profile.temporal.unwrap().samples, 2);
}
