use kronello_model::*;
use kronello_service::*;
use kronello_time::{Duration, Time};
use std::collections::BTreeMap;
use uuid::Uuid;
fn d(n: i64) -> Duration {
    Duration::new(Time::from_integer(n)).unwrap()
}
fn export(service: &Service<'_>, path: &std::path::Path) -> ExportResult {
    let ResultData::Export(r) = service
        .dispatch(Request::ProjectExport(ProjectRequest {
            project: path.into(),
        }))
        .unwrap()
    else {
        panic!()
    };
    r
}
fn error(result: Result<ResultData, ServiceError>, code: &str) {
    assert_eq!(result.unwrap_err().code, code);
}
#[test]
fn edition_pin_input_commands_revision_idempotency_and_undo() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("template.kronello");
    let service = Service::new(BackendSelection::CpuReference);
    let document: Project =
        serde_json::from_str(include_str!("../../../examples/template-001.project.json")).unwrap();
    let root = match &document.compositions[0] {
        DocumentObject::Known(c) => c.id,
        _ => panic!(),
    };
    service
        .dispatch(Request::ProjectCreate(CreateRequest {
            project: path.clone(),
            document,
        }))
        .unwrap();
    let definition: TemplateDefinition = serde_json::from_str(include_str!(
        "../../../examples/template-001.definition.json"
    ))
    .unwrap();
    let session = Uuid::new_v4();
    let define = TemplateDefineRequest {
        project: path.clone(),
        base_revision: "1".into(),
        session_id: session,
        idempotency_key: "define".into(),
        definition: definition.clone(),
    };
    service
        .dispatch(Request::TemplateDefine(define.clone()))
        .unwrap();
    service.dispatch(Request::TemplateDefine(define)).unwrap();
    assert_eq!(export(&service, &path).revision, "2");
    let instance = TemplateInstance {
        id: CompositionInstanceId::new(),
        definition_ref: definition.id,
        version: definition.version.clone(),
        duration: d(8),
        inputs: BTreeMap::new(),
    };
    let instantiate = TemplateInstantiateRequest {
        project: path.clone(),
        base_revision: "2".into(),
        session_id: session,
        idempotency_key: "place".into(),
        composition: root,
        node: NodeId::new(),
        index: 0,
        instance: instance.clone(),
    };
    service
        .dispatch(Request::TemplateInstantiate(instantiate.clone()))
        .unwrap();
    service
        .dispatch(Request::TemplateInstantiate(instantiate))
        .unwrap();
    let set = TemplateSetInputRequest {
        project: path.clone(),
        base_revision: "3".into(),
        session_id: session,
        idempotency_key: "headline".into(),
        instance: instance.id,
        name: "headline".into(),
        value: Value::String("変更された日本語".into()),
    };
    let ResultData::Edit(event) = service
        .dispatch(Request::TemplateSetInput(set.clone()))
        .unwrap()
    else {
        panic!()
    };
    service
        .dispatch(Request::TemplateSetInput(set.clone()))
        .unwrap();
    let mut private = set.clone();
    private.base_revision = "4".into();
    private.idempotency_key = "private".into();
    private.name = "private".into();
    error(
        service.dispatch(Request::TemplateSetInput(private)),
        "INPUT_NOT_PUBLIC",
    );
    let mut bad = set.clone();
    bad.idempotency_key = "bad".into();
    bad.base_revision = "4".into();
    bad.value = Value::Bool(true);
    error(
        service.dispatch(Request::TemplateSetInput(bad)),
        "INVALID_TEMPLATE",
    );
    let before = export(&service, &path);
    assert_eq!(before.revision, "4");
    let DocumentObject::Known(i) = &before.document.template_instances[0] else {
        panic!()
    };
    assert_eq!(i.inputs["headline"], set.value);
    service
        .dispatch(Request::EditUndo(UndoRequest {
            project: path.clone(),
            base_revision: "4".into(),
            session_id: session,
            idempotency_key: "undo".into(),
            event_id: event.id,
        }))
        .unwrap();
    let after = export(&service, &path);
    let DocumentObject::Known(i) = &after.document.template_instances[0] else {
        panic!()
    };
    assert!(i.inputs.is_empty());
    assert_eq!(i.version, "1.0.0");
    let mut v2 = definition.clone();
    v2.id = Uuid::new_v4();
    v2.version = "2.0.0".into();
    v2.public_inputs.get_mut("headline").unwrap().default = Value::String("新版".into());
    service
        .dispatch(Request::TemplateDefine(TemplateDefineRequest {
            project: path.clone(),
            base_revision: "5".into(),
            session_id: session,
            idempotency_key: "v2".into(),
            definition: v2,
        }))
        .unwrap();
    let final_document = export(&service, &path).document;
    let DocumentObject::Known(i) = &final_document.template_instances[0] else {
        panic!()
    };
    assert_eq!(i.definition_ref, definition.id);
    assert_eq!(i.version, "1.0.0");
    assert_eq!(final_document.templates.len(), 2);
    let mut replacement = definition.clone();
    replacement
        .public_inputs
        .get_mut("headline")
        .unwrap()
        .default = Value::String("改変".into());
    error(
        service.dispatch(Request::TemplateDefine(TemplateDefineRequest {
            project: path.clone(),
            base_revision: "6".into(),
            session_id: session,
            idempotency_key: "replace".into(),
            definition: replacement,
        })),
        "TEMPLATE_VERSION_EXISTS",
    );
    error(
        service.dispatch(Request::TemplateSetInput(TemplateSetInputRequest {
            idempotency_key: "stale".into(),
            ..set
        })),
        "REVISION_CONFLICT",
    );
}
#[test]
fn authoring_edit_cannot_change_a_published_edition() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("template.kronello");
    let service = Service::new(BackendSelection::CpuReference);
    let document: Project =
        serde_json::from_str(include_str!("../../../examples/template-001.project.json")).unwrap();
    let mut text = match &document.texts[0] {
        DocumentObject::Known(t) => t.clone(),
        _ => panic!(),
    };
    service
        .dispatch(Request::ProjectCreate(CreateRequest {
            project: path.clone(),
            document,
        }))
        .unwrap();
    let definition = serde_json::from_str(include_str!(
        "../../../examples/template-001.definition.json"
    ))
    .unwrap();
    service
        .dispatch(Request::TemplateDefine(TemplateDefineRequest {
            project: path.clone(),
            base_revision: "1".into(),
            session_id: Uuid::new_v4(),
            idempotency_key: "define".into(),
            definition,
        }))
        .unwrap();
    text.text = "別".into();
    text.styles[0].range.end = text.text.len();
    error(
        service.dispatch(Request::EditPlan(PlanRequest {
            project: path.clone(),
            base_revision: "2".into(),
            commands: vec![EditCommand::TextSet { text }],
        })),
        "TEMPLATE_DEFINITION_CHANGED",
    );
    assert_eq!(export(&service, &path).revision, "2");
}

#[test]
fn instance_duration_edit_rebuilds_map_and_import_cannot_republish_definition() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("template.kronello");
    let service = Service::new(BackendSelection::CpuReference);
    let document: Project =
        serde_json::from_str(include_str!("../../../examples/template-001.project.json")).unwrap();
    let root = match &document.compositions[0] {
        DocumentObject::Known(c) => c.id,
        _ => panic!(),
    };
    service
        .dispatch(Request::ProjectCreate(CreateRequest {
            project: path.clone(),
            document,
        }))
        .unwrap();
    let definition: TemplateDefinition = serde_json::from_str(include_str!(
        "../../../examples/template-001.definition.json"
    ))
    .unwrap();
    let session = Uuid::new_v4();
    service
        .dispatch(Request::TemplateDefine(TemplateDefineRequest {
            project: path.clone(),
            base_revision: "1".into(),
            session_id: session,
            idempotency_key: "define".into(),
            definition: definition.clone(),
        }))
        .unwrap();
    let instance = TemplateInstance {
        id: CompositionInstanceId::new(),
        definition_ref: definition.id,
        version: definition.version,
        duration: d(5),
        inputs: Default::default(),
    };
    service
        .dispatch(Request::TemplateInstantiate(TemplateInstantiateRequest {
            project: path.clone(),
            base_revision: "2".into(),
            session_id: session,
            idempotency_key: "place".into(),
            composition: root,
            node: NodeId::new(),
            index: 0,
            instance: instance.clone(),
        }))
        .unwrap();
    service
        .dispatch(Request::TemplateSetDuration(TemplateSetDurationRequest {
            project: path.clone(),
            base_revision: "3".into(),
            session_id: session,
            idempotency_key: "duration".into(),
            instance: instance.id,
            duration: d(8),
        }))
        .unwrap();
    let mut saved = export(&service, &path);
    let DocumentObject::Known(root) = &saved.document.compositions[0] else {
        panic!()
    };
    let NodeKind::CompositionInstance(p) = &root.nodes[0].kind else {
        panic!()
    };
    assert_eq!(
        p.local_time_map.map(Time::new(79, 10).unwrap()).unwrap(),
        Time::new(49, 10).unwrap()
    );
    let DocumentObject::Known(definition) = &mut saved.document.templates[0] else {
        panic!()
    };
    definition
        .public_inputs
        .get_mut("headline")
        .unwrap()
        .default = Value::String("再公開".into());
    error(
        service.dispatch(Request::ProjectImport(ImportRequest {
            project: path.clone(),
            base_revision: "4".into(),
            document: saved.document,
        })),
        "TEMPLATE_DEFINITION_CHANGED",
    );
    assert_eq!(export(&service, &path).revision, "4");
    error(
        service.dispatch(Request::TemplateSetDuration(TemplateSetDurationRequest {
            project: path.clone(),
            base_revision: "4".into(),
            session_id: session,
            idempotency_key: "short".into(),
            instance: instance.id,
            duration: d(1),
        })),
        "DURATION_TOO_SHORT",
    );
}

#[test]
fn future_template_contracts_round_trip_through_create_import_export() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("future.kronello");
    let service = Service::new(BackendSelection::CpuReference);
    let mut document: serde_json::Value =
        serde_json::from_str(include_str!("../../../examples/template-001.project.json")).unwrap();
    let mut definition: serde_json::Value = serde_json::from_str(include_str!(
        "../../../examples/template-001.definition.json"
    ))
    .unwrap();
    definition["future_constraint"] = serde_json::json!({"mode":"responsive"});
    document["templates"] = serde_json::json!([definition]);
    document["template_instances"] = serde_json::json!([{
        "id": Uuid::new_v4(), "future_binding": {"source":"data_table"}
    }]);
    service
        .dispatch(Request::ProjectCreate(CreateRequest {
            project: path.clone(),
            document: serde_json::from_value(document.clone()).unwrap(),
        }))
        .unwrap();
    assert_eq!(
        serde_json::to_value(export(&service, &path).document).unwrap(),
        document
    );
    document["name"] = serde_json::json!("preserved future templates");
    service
        .dispatch(Request::ProjectImport(ImportRequest {
            project: path.clone(),
            base_revision: "1".into(),
            document: serde_json::from_value(document.clone()).unwrap(),
        }))
        .unwrap();
    assert_eq!(
        serde_json::to_value(export(&service, &path).document).unwrap(),
        document
    );
    error(
        service.dispatch(Request::EditPlan(PlanRequest {
            project: path,
            base_revision: "2".into(),
            commands: vec![],
        })),
        "UNSUPPORTED_FEATURE",
    );
}

#[test]
fn import_cannot_obscure_published_content_or_definition_with_opaque_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pinned.kronello");
    let service = Service::new(BackendSelection::CpuReference);
    let document =
        serde_json::from_str(include_str!("../../../examples/template-001.project.json")).unwrap();
    service
        .dispatch(Request::ProjectCreate(CreateRequest {
            project: path.clone(),
            document,
        }))
        .unwrap();
    service
        .dispatch(Request::TemplateDefine(TemplateDefineRequest {
            project: path.clone(),
            base_revision: "1".into(),
            session_id: Uuid::new_v4(),
            idempotency_key: "define".into(),
            definition: serde_json::from_str(include_str!(
                "../../../examples/template-001.definition.json"
            ))
            .unwrap(),
        }))
        .unwrap();
    let original = serde_json::to_value(export(&service, &path).document).unwrap();
    for collection in ["texts", "templates"] {
        let mut altered = original.clone();
        altered[collection][0]["future_field"] = serde_json::json!(true);
        error(
            service.dispatch(Request::ProjectImport(ImportRequest {
                project: path.clone(),
                base_revision: "2".into(),
                document: serde_json::from_value(altered).unwrap(),
            })),
            "TEMPLATE_DEFINITION_CHANGED",
        );
        assert_eq!(export(&service, &path).revision, "2");
    }
}
