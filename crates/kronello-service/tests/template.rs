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
    *r
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
            plan_hash: None,
            idempotency_key: None,
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
        variant: None,
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
            plan_hash: None,
            idempotency_key: None,
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
            plan_hash: None,
            idempotency_key: None,
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
        variant: None,
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
            plan_hash: None,
            idempotency_key: None,
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
            plan_hash: None,
            idempotency_key: None,
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
            plan_hash: None,
            idempotency_key: None,
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
            plan_hash: None,
            idempotency_key: None,
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
                plan_hash: None,
                idempotency_key: None,
                project: path.clone(),
                base_revision: "2".into(),
                document: serde_json::from_value(altered).unwrap(),
            })),
            "TEMPLATE_DEFINITION_CHANGED",
        );
        assert_eq!(export(&service, &path).revision, "2");
    }
}

fn template2_fixture() -> (Project, TemplateDefinition, CompositionId) {
    let p: Project =
        serde_json::from_str(include_str!("../../../examples/template-002.project.json")).unwrap();
    let d: TemplateDefinition = serde_json::from_str(include_str!(
        "../../../examples/template-002.definition.json"
    ))
    .unwrap();
    let DocumentObject::Known(root) = &p.compositions[0] else {
        panic!()
    };
    let id = root.id;
    (p, d, id)
}
fn fonts(p: &Project) -> Vec<FontInput> {
    let DocumentObject::Known(text) = &p.texts[0] else {
        panic!()
    };
    vec![FontInput {
        identity: text.styles[0].font.clone(),
        path: std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/fixtures/external/NotoSansCJKjp-Regular.otf"),
    }]
}
fn publish(
    service: &Service<'_>,
    path: &std::path::Path,
    definition: TemplateDefinition,
    revision: &str,
    key: &str,
) {
    service
        .dispatch(Request::TemplateDefine(TemplateDefineRequest {
            project: path.into(),
            base_revision: revision.into(),
            session_id: Uuid::nil(),
            idempotency_key: key.into(),
            definition,
        }))
        .unwrap();
}
fn place2(
    service: &Service<'_>,
    path: &std::path::Path,
    root: CompositionId,
    instance: TemplateInstance,
    revision: &str,
    index: usize,
) {
    service
        .dispatch(Request::TemplateInstantiate(TemplateInstantiateRequest {
            project: path.into(),
            base_revision: revision.into(),
            session_id: Uuid::nil(),
            idempotency_key: format!("place-{index}"),
            composition: root,
            node: NodeId::new(),
            index,
            instance,
        }))
        .unwrap();
}
fn instance2(d: &TemplateDefinition, duration: i64, variant: Option<&str>) -> TemplateInstance {
    TemplateInstance {
        id: CompositionInstanceId::new(),
        definition_ref: d.id,
        version: d.version.clone(),
        duration: crate_duration(duration),
        variant: variant.map(str::to_owned),
        inputs: BTreeMap::new(),
    }
}
fn crate_duration(n: i64) -> Duration {
    Duration::new(Time::from_integer(n)).unwrap()
}
fn pin(p: &Project, id: CompositionInstanceId) -> &TemplateInstance {
    p.template_instances
        .iter()
        .find_map(|object| match object {
            DocumentObject::Known(i) if i.id == id => Some(i),
            _ => None,
        })
        .unwrap()
}
#[test]
fn migration_diff_previews_are_read_only_deterministic_and_explicit_apply_is_undoable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("migration.kronello");
    let service = Service::new(BackendSelection::CpuReference);
    let (p, definition, root) = template2_fixture();
    let fonts = fonts(&p);
    service
        .dispatch(Request::ProjectCreate(CreateRequest {
            plan_hash: None,
            idempotency_key: None,
            project: path.clone(),
            document: p,
        }))
        .unwrap();
    publish(&service, &path, definition.clone(), "1", "v1");
    let a = instance2(&definition, 5, None);
    let b = instance2(&definition, 8, Some("portrait"));
    place2(&service, &path, root, a.clone(), "2", 0);
    place2(&service, &path, root, b.clone(), "3", 1);
    let mut v2 = definition.clone();
    v2.id = Uuid::new_v4();
    v2.version = "2.0.0".into();
    let Value::DataTable(table) = &mut v2.public_inputs.get_mut("data").unwrap().default else {
        panic!()
    };
    table.rows[0].insert("headline".into(), Value::String("日".into()));
    table.columns.insert("unbound/~".into(), ValueType::String);
    table.rows[0].insert("unbound/~".into(), Value::String("material".into()));
    publish(&service, &path, v2.clone(), "4", "v2");
    let original = export(&service, &path);
    assert_eq!(pin(&original.document, a.id), &a);
    assert_eq!(pin(&original.document, b.id), &b);
    let request = TemplateMigrationPlanRequest {
        project: path.clone(),
        base_revision: "5".into(),
        instance: a.id,
        definition: v2.id,
        variant: Some("portrait".into()),
        inputs: None,
        time: Time::ONE,
        region: None,
        fonts: fonts.clone(),
    };
    let ResultData::TemplateMigrationPlan(result) = service
        .dispatch(Request::TemplateMigrationPlan(request.clone()))
        .unwrap()
    else {
        panic!()
    };
    assert!(
        result.before.diagnostic.is_none(),
        "{:?}",
        result.before.diagnostic
    );
    assert!(
        result.after.diagnostic.is_none(),
        "{:?}",
        result.after.diagnostic
    );
    assert_eq!(
        result.before.design_extent,
        DesignExtent::new(64.0, 32.0).unwrap()
    );
    assert_eq!(
        result.after.design_extent,
        DesignExtent::new(32.0, 64.0).unwrap()
    );
    assert!(
        result
            .changes
            .iter()
            .any(|c| c.field == "/instance/version")
    );
    assert!(
        result
            .changes
            .iter()
            .any(|c| c.field.starts_with("/definition/public_inputs/data"))
    );
    assert!(
        result
            .before
            .nodes
            .iter()
            .any(|n| n.evaluated.text.as_deref() == Some("日本語"))
    );
    assert!(
        result
            .after
            .nodes
            .iter()
            .any(|n| n.evaluated.text.as_deref() == Some("日"))
    );
    assert!(
        result
            .changes
            .iter()
            .any(|c| c.field.ends_with("/unbound~1~0"))
    );
    for preview in [&result.before, &result.after] {
        let text = preview
            .nodes
            .iter()
            .find(|n| n.evaluated.text.is_some())
            .unwrap();
        assert_ne!(
            text.evaluated.bounds.layout_bounds,
            text.evaluated.bounds.ink_bounds
        );
        assert_eq!(
            text.evaluated.bounds.ink_bounds,
            text.evaluated.bounds.visual_bounds
        );
    }
    let repeated = service
        .dispatch(Request::TemplateMigrationPlan(request))
        .unwrap();
    assert_eq!(
        serde_json::to_value(&repeated).unwrap(),
        serde_json::to_value(ResultData::TemplateMigrationPlan(result.clone())).unwrap()
    );
    assert_eq!(export(&service, &path).document, original.document);
    assert_eq!(export(&service, &path).revision, "5");
    // Import remains unable to change a pin, even when the candidate is valid.
    error(
        service.dispatch(Request::ProjectImport(ImportRequest {
            plan_hash: None,
            idempotency_key: None,
            project: path.clone(),
            base_revision: "5".into(),
            document: result.plan.candidate.clone(),
        })),
        "INVALID_TEMPLATE",
    );
    let apply = EditApplyRequest {
        project: path.clone(),
        base_revision: "5".into(),
        plan_hash: result.plan.plan_hash.clone(),
        idempotency_key: "migrate".into(),
        session_id: Uuid::nil(),
        commands: result.plan.commands.clone(),
    };
    let mut tampered = apply.clone();
    let EditCommand::Template(command) = &mut tampered.commands[0] else {
        panic!()
    };
    let TemplateCommand::Migrate { instance, .. } = command.as_mut() else {
        panic!()
    };
    *instance = b.id;
    error(
        service.dispatch(Request::EditApply(tampered)),
        "PLAN_HASH_MISMATCH",
    );
    let ResultData::Edit(event) = service.dispatch(Request::EditApply(apply.clone())).unwrap()
    else {
        panic!()
    };
    service.dispatch(Request::EditApply(apply)).unwrap();
    let migrated = export(&service, &path);
    assert_eq!(migrated.revision, "6");
    assert_eq!(pin(&migrated.document, a.id).definition_ref, v2.id);
    assert_eq!(
        pin(&migrated.document, a.id).variant.as_deref(),
        Some("portrait")
    );
    assert_eq!(pin(&migrated.document, b.id), &b);
    service
        .dispatch(Request::EditUndo(UndoRequest {
            project: path.clone(),
            base_revision: "6".into(),
            session_id: Uuid::nil(),
            idempotency_key: "undo-migrate".into(),
            event_id: event.id,
        }))
        .unwrap();
    let undone = export(&service, &path);
    assert_eq!(pin(&undone.document, a.id), &a);
    assert_eq!(pin(&undone.document, b.id), &b);
}

#[test]
fn data_projection_variant_inputs_durations_versions_and_preview_failures_are_isolated() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data.kronello");
    let service = Service::new(BackendSelection::CpuReference);
    let (p, definition, root) = template2_fixture();
    let fonts = fonts(&p);
    service
        .dispatch(Request::ProjectCreate(CreateRequest {
            plan_hash: None,
            idempotency_key: None,
            project: path.clone(),
            document: p,
        }))
        .unwrap();
    publish(&service, &path, definition.clone(), "1", "v1");
    let mut a = instance2(&definition, 5, None);
    let b = instance2(&definition, 10, Some("portrait"));
    let Value::DataTable(mut table) = definition.public_inputs["data"].default.clone() else {
        panic!()
    };
    table.rows[0].insert("headline".into(), Value::String("一\n二".into()));
    a.inputs
        .insert("data".into(), Value::DataTable(table.clone()));
    place2(&service, &path, root, a.clone(), "2", 0);
    place2(&service, &path, root, b.clone(), "3", 1);
    for (i, expected) in [(&a, "一\n二"), (&b, "日本語")] {
        let ResultData::TemplatePreview(preview) = service
            .dispatch(Request::TemplatePreview(TemplatePreviewRequest {
                project: path.clone(),
                instance: i.clone(),
                time: Time::ONE,
                region: None,
                fonts: fonts.clone(),
            }))
            .unwrap()
        else {
            panic!()
        };
        assert!(preview.diagnostic.is_none(), "{:?}", preview.diagnostic);
        assert!(
            preview
                .nodes
                .iter()
                .any(|n| n.evaluated.text.as_deref() == Some(expected))
        );
        let band = &preview.definition.constraints.bands[0];
        let text = preview
            .nodes
            .iter()
            .find(|n| n.key.node == band.text_node)
            .unwrap();
        let background = preview
            .nodes
            .iter()
            .find(|n| n.key.node == band.band_node)
            .unwrap();
        let Value::Vec2(size) = background.evaluated.properties[&band.size_property] else {
            panic!()
        };
        let ink = text.evaluated.bounds.ink_bounds.unwrap();
        for (axis, dimension) in size.iter().enumerate() {
            assert!(
                (dimension.get()
                    - (ink.max[axis] - ink.min[axis] + 2.0 * band.padding[axis].get()))
                .abs()
                    < 1e-10
            );
        }
    }
    table.rows[0].insert("private".into(), Value::Bool(true));
    error(
        service.dispatch(Request::TemplateSetInput(TemplateSetInputRequest {
            project: path.clone(),
            base_revision: "4".into(),
            session_id: Uuid::nil(),
            idempotency_key: "bad-table".into(),
            instance: a.id,
            name: "data".into(),
            value: Value::DataTable(table),
        })),
        "INVALID_DATA_TABLE",
    );
    error(
        service.dispatch(Request::TemplateSetInput(TemplateSetInputRequest {
            project: path.clone(),
            base_revision: "4".into(),
            session_id: Uuid::nil(),
            idempotency_key: "private".into(),
            instance: a.id,
            name: "private".into(),
            value: Value::Bool(true),
        })),
        "INPUT_NOT_PUBLIC",
    );
    let mut long = a.clone();
    let Value::DataTable(table) = long.inputs.get_mut("data").unwrap() else {
        panic!()
    };
    table.rows[0].insert("headline".into(), Value::String("一\n二\n三".into()));
    let ResultData::TemplatePreview(preview) = service
        .dispatch(Request::TemplatePreview(TemplatePreviewRequest {
            project: path.clone(),
            instance: long,
            time: Time::ONE,
            region: None,
            fonts,
        }))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(preview.diagnostic.unwrap().code, "TEMPLATE_OVERFLOW");
    assert!(preview.nodes.is_empty());
    let saved = export(&service, &path);
    assert_eq!(saved.revision, "4");
    assert_eq!(pin(&saved.document, a.id), &a);
    assert_eq!(pin(&saved.document, b.id), &b);
}

#[test]
fn media_slots_validate_refs_expose_bindings_and_reject_final_execution() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("media.kronello");
    let service = Service::new(BackendSelection::CpuReference);
    let (mut p, mut definition, root) = template2_fixture();
    let fonts = fonts(&p);
    definition.variants.clear();
    let slot = NodeId::new();
    let asset = AssetId::new();
    p.assets.push(DocumentObject::Known(Asset {
        id: asset,
        content_hash: "a".repeat(64),
        kind: AssetKind::Image,
        streams: vec![],
        locator: AssetLocator {
            relative: Some("logo.png".into()),
            absolute: None,
        },
    }));
    let second_asset = AssetId::new();
    let DocumentObject::Known(mut asset2) = p.assets[0].clone() else {
        panic!()
    };
    asset2.id = second_asset;
    asset2.content_hash = "b".repeat(64);
    asset2.locator.relative = Some("alternate.png".into());
    p.assets.push(DocumentObject::Known(asset2));
    let DocumentObject::Known(c) = p
        .compositions
        .iter_mut()
        .find(|c| matches!(c,DocumentObject::Known(c) if c.id == definition.composition_ref))
        .unwrap()
    else {
        panic!()
    };
    c.root_nodes.push(slot);
    c.nodes.push(SceneNode {
        tags: Default::default(),
        name: None,
        enabled: true,
        id: slot,
        kind: NodeKind::Null,
        containment_parent: None,
        transform_parent: None,
        child_order: vec![],
        active_range: kronello_time::TimeRange::from_start_duration(Time::ZERO, c.duration)
            .unwrap(),
        properties: vec![],
        effects: vec![],
    });
    definition.public_inputs.insert(
        "logo".into(),
        TemplateInput {
            value_type: ValueType::AssetRef,
            default: Value::AssetRef(asset),
            target: TemplateInputTarget::MediaSlot { node: slot },
            minimum: None,
            maximum: None,
            choices: vec![],
        },
    );
    service
        .dispatch(Request::ProjectCreate(CreateRequest {
            plan_hash: None,
            idempotency_key: None,
            project: path.clone(),
            document: p,
        }))
        .unwrap();
    publish(&service, &path, definition.clone(), "1", "media-definition");
    let instance = instance2(&definition, 5, None);
    place2(&service, &path, root, instance.clone(), "2", 0);
    let mut second = instance2(&definition, 8, None);
    second
        .inputs
        .insert("logo".into(), Value::AssetRef(second_asset));
    place2(&service, &path, root, second.clone(), "3", 1);
    let ResultData::TemplatePreview(second_preview) = service
        .dispatch(Request::TemplatePreview(TemplatePreviewRequest {
            project: path.clone(),
            instance: second.clone(),
            time: Time::ONE,
            fonts: fonts.clone(),
            region: None,
        }))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(second_preview.media_slots[&slot], second_asset);
    assert_eq!(
        second_preview.diagnostic.unwrap().code,
        "UNSUPPORTED_FEATURE"
    );
    let ResultData::TemplatePreview(preview) = service
        .dispatch(Request::TemplatePreview(TemplatePreviewRequest {
            project: path.clone(),
            instance: instance.clone(),
            time: Time::ONE,
            region: None,
            fonts: fonts.clone(),
        }))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(preview.media_slots[&slot], asset);
    assert_eq!(preview.diagnostic.unwrap().code, "UNSUPPORTED_FEATURE");
    error(
        service.dispatch(Request::RenderFrame(FrameRenderRequest {
            input: RenderInput {
                project: path.clone(),
                composition: Some(root),
                target: None,
                region: kronello_render::OutputRegion {
                    origin: [0.0; 2],
                    extent: [64.0, 32.0],
                    pixels: [64, 32],
                },
                profile: Default::default(),
                fonts,
            },
            time: Time::ONE,
        })),
        "UNSUPPORTED_FEATURE",
    );
    error(
        service.dispatch(Request::TemplateSetInput(TemplateSetInputRequest {
            project: path.clone(),
            base_revision: "4".into(),
            session_id: Uuid::nil(),
            idempotency_key: "missing".into(),
            instance: instance.id,
            name: "logo".into(),
            value: Value::AssetRef(AssetId::new()),
        })),
        "ASSET_MISSING",
    );
    let saved = export(&service, &path);
    assert_eq!(saved.revision, "4");
    assert_eq!(pin(&saved.document, instance.id), &instance);
    assert_eq!(pin(&saved.document, second.id), &second);
}

#[test]
fn migration_requires_explicit_resolution_and_variant_content_cannot_be_obscured() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("resolution.kronello");
    let service = Service::new(BackendSelection::CpuReference);
    let (p, definition, root) = template2_fixture();
    let fonts = fonts(&p);
    service
        .dispatch(Request::ProjectCreate(CreateRequest {
            plan_hash: None,
            idempotency_key: None,
            project: path.clone(),
            document: p,
        }))
        .unwrap();
    publish(&service, &path, definition.clone(), "1", "v1");
    let mut instance = instance2(&definition, 5, Some("portrait"));
    instance.inputs.insert(
        "data".into(),
        definition.public_inputs["data"].default.clone(),
    );
    place2(&service, &path, root, instance.clone(), "2", 0);
    let original = export(&service, &path);
    let mut opaque = serde_json::to_value(&original.document).unwrap();
    let variant = &definition.variants["portrait"];
    let id = serde_json::to_value(variant.composition_ref).unwrap();
    let c = opaque["compositions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|c| c["id"] == id)
        .unwrap();
    c["future_layout"] = serde_json::json!(true);
    error(
        service.dispatch(Request::ProjectImport(ImportRequest {
            plan_hash: None,
            idempotency_key: None,
            project: path.clone(),
            base_revision: "3".into(),
            document: serde_json::from_value(opaque).unwrap(),
        })),
        "TEMPLATE_DEFINITION_CHANGED",
    );
    let mut next = definition.clone();
    next.id = Uuid::new_v4();
    next.version = "2.0.0".into();
    next.variants.clear();
    let old_inputs: TemplateDefinition = serde_json::from_str(include_str!(
        "../../../examples/template-001.definition.json"
    ))
    .unwrap();
    next.public_inputs = old_inputs.public_inputs;
    publish(&service, &path, next.clone(), "3", "v2");
    let r = TemplateMigrationPlanRequest {
        project: path.clone(),
        base_revision: "4".into(),
        instance: instance.id,
        definition: next.id,
        variant: None,
        inputs: None,
        time: Time::ONE,
        region: None,
        fonts: fonts.clone(),
    };
    error(
        service.dispatch(Request::TemplateMigrationPlan(r.clone())),
        "INVALID_TEMPLATE",
    );
    let ResultData::TemplateMigrationPlan(plan) = service
        .dispatch(Request::TemplateMigrationPlan(
            TemplateMigrationPlanRequest {
                inputs: Some(BTreeMap::new()),
                ..r.clone()
            },
        ))
        .unwrap()
    else {
        panic!()
    };
    assert!(
        plan.changes
            .iter()
            .any(|c| c.field == "/instance/inputs/data" && c.after.is_null())
    );
    assert!(plan.after.diagnostic.is_none());
    error(
        service.dispatch(Request::TemplateMigrationPlan(
            TemplateMigrationPlanRequest {
                base_revision: "3".into(),
                ..r.clone()
            },
        )),
        "REVISION_CONFLICT",
    );
    next.id = Uuid::new_v4();
    next.version = "3.0.0".into();
    next.template_id = Uuid::new_v4();
    publish(&service, &path, next.clone(), "4", "different-family");
    error(
        service.dispatch(Request::TemplateMigrationPlan(
            TemplateMigrationPlanRequest {
                base_revision: "5".into(),
                definition: next.id,
                inputs: Some(BTreeMap::new()),
                ..r
            },
        )),
        "TEMPLATE_MIGRATION_INCOMPATIBLE",
    );
    assert_eq!(
        pin(&export(&service, &path).document, instance.id),
        &instance
    );
}
