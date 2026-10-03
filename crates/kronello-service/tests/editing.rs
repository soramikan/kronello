use kronello_model::*;
use kronello_service::*;
use kronello_store::{ChangedKey, Event, OpenOptions, ProjectStore};
use kronello_time::Time;
use serde_json::{Value as Json, json};
use std::path::{Path, PathBuf};
use uuid::Uuid;

fn fixture() -> Project {
    serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap()
}
fn service() -> Service<'static> {
    Service::new(BackendSelection::CpuReference)
}
fn setup() -> (tempfile::TempDir, PathBuf, Project) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("edit.kronello");
    let p = fixture();
    service()
        .dispatch(Request::ProjectCreate(CreateRequest {
            project: path.clone(),
            document: p.clone(),
        }))
        .unwrap();
    (dir, path, p)
}
fn export(path: &Path) -> ExportResult {
    let ResultData::Export(r) = service()
        .dispatch(Request::ProjectExport(ProjectRequest {
            project: path.into(),
        }))
        .unwrap()
    else {
        panic!()
    };
    r
}
fn comp(p: &Project) -> &Composition {
    let DocumentObject::Known(c) = &p.compositions[0] else {
        panic!()
    };
    c
}
fn source(p: &Project, node: usize, prop: usize) -> &PropertySource<Value> {
    comp(p).nodes[node].properties[prop].source()
}
fn scalar(x: f64) -> Value {
    Value::Scalar(FiniteF64::new(x).unwrap())
}
fn change(p: &Project, node: usize, prop: usize, x: f64) -> EditCommand {
    EditCommand::PropertySourceSet {
        object: comp(p).nodes[node].id.as_uuid(),
        property: comp(p).nodes[node].properties[prop].id(),
        source: PropertySource::Constant(scalar(x)),
        curve: None,
    }
}
fn plan(path: &Path, base: &str, commands: Vec<EditCommand>) -> EditPlan {
    let ResultData::Plan(p) = service()
        .dispatch(Request::EditPlan(PlanRequest {
            project: path.into(),
            base_revision: base.into(),
            commands,
        }))
        .unwrap()
    else {
        panic!()
    };
    *p
}
fn request(path: &Path, plan: &EditPlan, key: &str) -> EditApplyRequest {
    EditApplyRequest {
        project: path.into(),
        base_revision: plan.base_revision.clone(),
        plan_hash: plan.plan_hash.clone(),
        commands: plan.commands.clone(),
        session_id: Uuid::from_u128(99),
        idempotency_key: key.into(),
    }
}
fn apply_request(r: EditApplyRequest) -> Result<Event, ServiceError> {
    let ResultData::Edit(e) = service().dispatch(Request::EditApply(r))? else {
        panic!()
    };
    Ok(e)
}
fn apply(path: &Path, commands: Vec<EditCommand>, key: &str) -> Event {
    let p = plan(path, &export(path).revision, commands);
    apply_request(request(path, &p, key)).unwrap()
}
fn undo_request(path: &Path, event: &Event, key: &str) -> UndoRequest {
    UndoRequest {
        project: path.into(),
        base_revision: export(path).revision,
        session_id: Uuid::from_u128(101),
        idempotency_key: key.into(),
        event_id: event.id,
    }
}
fn undo(path: &Path, event: &Event, key: &str) -> Event {
    let ResultData::Edit(e) = service()
        .dispatch(Request::EditUndo(undo_request(path, event, key)))
        .unwrap()
    else {
        panic!()
    };
    e
}
fn history(path: &Path, since: &str) -> HistoryResult {
    let ResultData::History(h) = service()
        .dispatch(Request::HistoryList(HistoryRequest {
            project: path.into(),
            since_revision: since.into(),
            limit: 100,
            session_id: None,
        }))
        .unwrap()
    else {
        panic!()
    };
    h
}
fn node(kind: NodeKind, p: &Project, parent: Option<NodeId>) -> SceneNode {
    SceneNode {
        id: NodeId::new(),
        kind,
        containment_parent: parent,
        transform_parent: None,
        child_order: vec![],
        active_range: comp(p).nodes[0].active_range,
        properties: vec![],
    }
}

#[test]
fn deterministic_plan_hash_revision_checks_receipts_and_no_client_keys() {
    let (_dir, path, p) = setup();
    let command = change(&p, 0, 1, 2.5);
    let a = plan(&path, "1", vec![command.clone()]);
    let b = plan(&path, "01", vec![command]);
    assert_eq!(
        serde_json::to_value(&a).unwrap(),
        serde_json::to_value(&b).unwrap()
    );
    assert_eq!(
        a.changed_keys,
        [ChangedKey::Value {
            object_id: comp(&p).nodes[0].id.as_uuid(),
            property_id: comp(&p).nodes[0].properties[1].id()
        }]
        .into()
    );
    assert_eq!(export(&path).revision, "1");
    assert_eq!(history(&path, "0").events.len(), 1);
    let good = request(&path, &a, "apply-1");
    let mut bad = good.clone();
    bad.plan_hash = "bad".into();
    assert_eq!(apply_request(bad).unwrap_err().code, "PLAN_HASH_MISMATCH");
    let mut bad = good.clone();
    bad.commands = vec![change(&p, 0, 1, 3.0)];
    assert_eq!(apply_request(bad).unwrap_err().code, "PLAN_HASH_MISMATCH");
    assert_eq!(export(&path).document, p);
    let mut bad = good.clone();
    bad.idempotency_key.clear();
    assert_eq!(apply_request(bad).unwrap_err().code, "INVALID_REQUEST");
    let event = apply_request(good.clone()).unwrap();
    assert_eq!(event.revision, 2);
    assert_eq!(apply_request(good.clone()).unwrap(), event);
    let mut normalized = good.clone();
    normalized.base_revision = "01".into();
    assert_eq!(apply_request(normalized).unwrap(), event);
    let mut bad = good.clone();
    bad.session_id = Uuid::from_u128(100);
    assert_eq!(
        apply_request(bad).unwrap_err().code,
        "IDEMPOTENCY_KEY_REUSED"
    );
    let mut bad = good.clone();
    bad.commands = vec![change(&p, 0, 1, 3.0)];
    assert_eq!(
        apply_request(bad).unwrap_err().code,
        "IDEMPOTENCY_KEY_REUSED"
    );
    let mut stale = good.clone();
    stale.idempotency_key = "different".into();
    assert_eq!(apply_request(stale).unwrap_err().code, "REVISION_CONFLICT");
    assert_eq!(
        service()
            .dispatch(Request::EditPlan(PlanRequest {
                project: path.clone(),
                base_revision: "1".into(),
                commands: a.commands.clone()
            }))
            .unwrap_err()
            .code,
        "REVISION_CONFLICT"
    );
    let store = ProjectStore::open(&path, OpenOptions::default()).unwrap();
    let receipt = store.idempotency_record("apply-1").unwrap().unwrap();
    assert_eq!(receipt.result, event);
    assert!(receipt.service_payload.is_some());
    store.close().unwrap();
    let mut json = serde_json::to_value(Request::EditApply(good)).unwrap();
    json["changed_keys"] = json!([]);
    assert!(serde_json::from_str::<Request>(&json.to_string()).is_err());
}

#[test]
fn selective_undo_preserves_other_property_redo_and_history_status() {
    let (_dir, path, p) = setup();
    let a = apply(&path, vec![change(&p, 0, 1, 2.0)], "a");
    // Another scalar descriptor on the same node is independent.
    let index = comp(&p).nodes[0]
        .properties
        .iter()
        .position(|p| p.descriptor().key.as_str() == "kronello.opacity")
        .unwrap();
    let value = match comp(&p).nodes[0].properties[index].source() {
        PropertySource::Constant(Value::Scalar(_)) => scalar(0.25),
        _ => panic!(),
    };
    let b = apply(
        &path,
        vec![EditCommand::PropertySourceSet {
            object: comp(&p).nodes[0].id.as_uuid(),
            property: comp(&p).nodes[0].properties[index].id(),
            source: PropertySource::Constant(value.clone()),
            curve: None,
        }],
        "b",
    );
    let u = undo(&path, &a, "undo-a");
    assert_eq!(u.undo_of, Some(a.id));
    assert_eq!(u.revision, 4);
    let now = export(&path).document;
    assert_eq!(source(&now, 0, 1), source(&p, 0, 1));
    assert_eq!(source(&now, 0, index), &PropertySource::Constant(value));
    let h = history(&path, "2");
    assert_eq!(h.revision, "4");
    assert_eq!(h.events.len(), 2);
    assert!(!h.events[0].undone);
    assert_eq!(h.events[0].event.id, b.id);
    assert!(
        history(&path, "0")
            .events
            .iter()
            .find(|e| e.event.id == a.id)
            .unwrap()
            .undone
    );
    assert_eq!(
        service()
            .dispatch(Request::EditUndo(undo_request(&path, &a, "again")))
            .unwrap_err()
            .code,
        "EVENT_ALREADY_UNDONE"
    );
    let redo = undo(&path, &u, "redo-a");
    assert_eq!(redo.undo_of, Some(u.id));
    assert_eq!(
        source(&export(&path).document, 0, 1),
        &PropertySource::Constant(scalar(2.0))
    );
    let h = history(&path, "0");
    assert!(!h.events.iter().find(|e| e.event.id == a.id).unwrap().undone);
    assert!(h.events.iter().find(|e| e.event.id == u.id).unwrap().undone);
}

#[test]
fn undo_conflict_lists_events_keys_and_rejects_whole_batch() {
    let (_dir, path, p) = setup();
    let a = apply(&path, vec![change(&p, 0, 1, 2.0)], "a");
    let b = apply(&path, vec![change(&p, 0, 1, 3.0)], "b");
    let before = export(&path);
    let count = history(&path, "0").events.len();
    let error = service()
        .dispatch(Request::EditUndo(undo_request(&path, &a, "conflict")))
        .unwrap_err();
    assert_eq!(error.code, "UNDO_CONFLICT");
    let details = error.details.unwrap();
    assert_eq!(details["conflicts"][0]["event_id"], b.id.to_string());
    assert_eq!(
        details["conflicts"][0]["keys"],
        serde_json::to_value(&b.changed_keys).unwrap()
    );
    assert_eq!(export(&path).revision, before.revision);
    assert_eq!(export(&path).document, before.document);
    assert_eq!(history(&path, "0").events.len(), count);
    let store = ProjectStore::open(&path, OpenOptions::default()).unwrap();
    assert!(store.idempotency_record("conflict").unwrap().is_none());
    store.close().unwrap();
    let u = undo(&path, &b, "undo-b");
    // The undone B is excluded; its still-active inverse is itself an event,
    // and ADR-0026 requires explicitly targeting that event when keys overlap.
    let e = service()
        .dispatch(Request::EditUndo(undo_request(&path, &a, "conflict-2")))
        .unwrap_err();
    assert_eq!(
        e.details.unwrap()["conflicts"][0]["event_id"],
        u.id.to_string()
    );
}

#[test]
fn structure_conflicts_cover_parent_containers_and_object_values() {
    let (_dir, path, p) = setup();
    let c = comp(&p).id;
    let a = node(NodeKind::Null, &p, None);
    let b = node(NodeKind::Null, &p, None);
    let added = apply(
        &path,
        vec![EditCommand::NodeAdd {
            composition: c,
            node: a.clone(),
            index: 2,
        }],
        "add-a",
    );
    let sibling = apply(
        &path,
        vec![EditCommand::NodeAdd {
            composition: c,
            node: b,
            index: 3,
        }],
        "add-b",
    );
    let e = service()
        .dispatch(Request::EditUndo(undo_request(&path, &added, "undo-add")))
        .unwrap_err();
    assert_eq!(e.code, "UNDO_CONFLICT");
    assert_eq!(
        e.details.unwrap()["conflicts"][0]["event_id"],
        sibling.id.to_string()
    );
    let (_dir, path, p) = setup();
    let a = apply(&path, vec![change(&p, 0, 1, 2.0)], "property");
    let removed = apply(
        &path,
        vec![EditCommand::NodeRemove {
            composition: comp(&p).id,
            node: comp(&p).nodes[0].id,
        }],
        "remove",
    );
    let e = service()
        .dispatch(Request::EditUndo(undo_request(&path, &a, "undo-property")))
        .unwrap_err();
    assert_eq!(e.code, "UNDO_CONFLICT");
    assert_eq!(
        e.details.unwrap()["conflicts"][0]["event_id"],
        removed.id.to_string()
    );
}

#[test]
fn keyframe_commands_shared_consumers_and_inverse_roundtrip() {
    let (_dir, path, p) = setup();
    let DocumentObject::Known(c) = &p.curves[0] else {
        panic!()
    };
    let id = c.id();
    let mut key = c.keys()[0].clone();
    key.time = Time::new(1, 2).unwrap();
    let a = apply(
        &path,
        vec![EditCommand::KeyframeInsert {
            curve: id,
            key: key.clone(),
        }],
        "insert",
    );
    assert!(a.changed_keys.iter().any(|k|matches!(k,ChangedKey::Value {object_id,..} if *object_id==comp(&p).nodes[0].id.as_uuid())));
    undo(&path, &a, "undo-insert");
    assert_eq!(export(&path).document, p);
    let mut key2 = key.clone();
    key2.time = Time::new(3, 4).unwrap();
    apply(
        &path,
        vec![
            EditCommand::KeyframeUpsert {
                curve: id,
                key: key.clone(),
            },
            EditCommand::KeyframeUpsert {
                curve: id,
                key: key.clone(),
            },
            EditCommand::KeyframeInsert {
                curve: id,
                key: key2,
            },
        ],
        "upsert",
    );
    apply(
        &path,
        vec![
            EditCommand::KeyframeReplace {
                curve: id,
                key: key.clone(),
            },
            EditCommand::KeyframeRemove {
                curve: id,
                time: key.time,
            },
        ],
        "replace-remove",
    );
    let before = export(&path);
    let result = service().dispatch(Request::EditPlan(PlanRequest {
        project: path.clone(),
        base_revision: before.revision.clone(),
        commands: vec![EditCommand::KeyframeInsert {
            curve: id,
            key: c.keys()[0].clone(),
        }],
    }));
    assert_eq!(result.unwrap_err().code, "INVALID_EDIT");
    assert_eq!(export(&path).document, before.document);
    let result = service().dispatch(Request::EditPlan(PlanRequest {
        project: path.clone(),
        base_revision: before.revision,
        commands: vec![EditCommand::KeyframeReplace { curve: id, key }],
    }));
    assert_eq!(result.unwrap_err().code, "INVALID_EDIT");
}

#[test]
fn typed_node_composition_instance_content_operations_roundtrip() {
    let (_dir, path, p) = setup();
    let cid = comp(&p).id;
    let group = node(NodeKind::Group, &p, None);
    let child = node(NodeKind::Null, &p, Some(group.id));
    let added = apply(
        &path,
        vec![
            EditCommand::NodeAdd {
                composition: cid,
                node: group.clone(),
                index: 2,
            },
            EditCommand::NodeAdd {
                composition: cid,
                node: child.clone(),
                index: 0,
            },
        ],
        "nodes",
    );
    assert!(added.changed_keys.contains(&ChangedKey::Structure {
        object_id: child.id.as_uuid(),
        parent_container_id: group.id.as_uuid()
    }));
    let reparent = apply(
        &path,
        vec![EditCommand::NodeReparent {
            composition: cid,
            node: child.id,
            parent: None,
            index: 0,
        }],
        "parent",
    );
    assert!(reparent.changed_keys.contains(&ChangedKey::Structure {
        object_id: child.id.as_uuid(),
        parent_container_id: group.id.as_uuid()
    }));
    assert!(reparent.changed_keys.contains(&ChangedKey::Structure {
        object_id: child.id.as_uuid(),
        parent_container_id: cid.as_uuid()
    }));
    undo(&path, &reparent, "undo-parent");
    let transform = apply(
        &path,
        vec![EditCommand::TransformParentSet {
            composition: cid,
            node: child.id,
            parent: Some(group.id),
        }],
        "transform",
    );
    undo(&path, &transform, "undo-transform");
    let current = export(&path);
    let mut order = comp(&current.document).root_nodes.clone();
    order.reverse();
    let reorder = apply(
        &path,
        vec![EditCommand::NodeReorder {
            composition: cid,
            parent: None,
            order,
        }],
        "order",
    );
    undo(&path, &reorder, "undo-order");
    let removed = apply(
        &path,
        vec![EditCommand::NodeRemove {
            composition: cid,
            node: group.id,
        }],
        "remove",
    );
    assert_eq!(comp(&export(&path).document).nodes.len(), 2);
    undo(&path, &removed, "undo-remove");
    assert_eq!(comp(&export(&path).document).nodes.len(), 4);
    let mut definition = comp(&p).clone();
    definition.id = CompositionId::new();
    definition.nodes.clear();
    definition.root_nodes.clear();
    definition.properties.clear();
    let new = apply(
        &path,
        vec![EditCommand::CompositionCreate {
            composition: definition.clone(),
        }],
        "composition",
    );
    let instance = CompositionInstance {
        id: CompositionInstanceId::new(),
        definition_ref: definition.id,
        input_bindings: Default::default(),
        local_time_map: kronello_time::TimeMap::linear(Time::ZERO, kronello_time::Rational::ONE)
            .unwrap(),
        seed: 42,
    };
    let placed = apply(
        &path,
        vec![EditCommand::InstancePlace {
            composition: cid,
            node: node(NodeKind::CompositionInstance(instance), &p, None),
            index: 3,
        }],
        "instance",
    );
    undo(&path, &placed, "undo-instance");
    // Undoing the creation is blocked by later structural events in its parent
    // only when they touch the same object/container; the placement's inverse
    // itself retains the touched definition key if the resource is consumed.
    let _ = new;
    let DocumentObject::Known(shape) = &p.shapes[0] else {
        panic!()
    };
    let mut shape = shape.clone();
    shape.fill = None;
    let event = apply(&path, vec![EditCommand::ShapeSet { shape }], "shape");
    undo(&path, &event, "undo-shape");
    assert_eq!(export(&path).document.shapes, p.shapes);
    let DocumentObject::Known(text) = &p.texts[0] else {
        panic!()
    };
    let mut text = text.clone();
    text.text = "字幕字".into();
    let event = apply(&path, vec![EditCommand::TextSet { text }], "text");
    undo(&path, &event, "undo-text");
    assert_eq!(export(&path).document.texts, p.texts);
}

#[test]
fn invalid_candidates_and_undo_validation_are_atomic() {
    let (_dir, path, p) = setup();
    let cid = comp(&p).id;
    let n = comp(&p).nodes[0].id;
    for commands in [
        vec![
            change(&p, 0, 1, 2.0),
            EditCommand::NodeReparent {
                composition: cid,
                node: n,
                parent: Some(n),
                index: 0,
            },
        ],
        vec![EditCommand::NodeReorder {
            composition: cid,
            parent: None,
            order: vec![n, n],
        }],
        vec![EditCommand::PropertySourceSet {
            object: n.as_uuid(),
            property: comp(&p).nodes[0].properties[1].id(),
            source: PropertySource::Curve(CurveId::new()),
            curve: None,
        }],
        vec![EditCommand::InstancePlace {
            composition: cid,
            node: node(NodeKind::Null, &p, None),
            index: 0,
        }],
    ] {
        let error = service()
            .dispatch(Request::EditPlan(PlanRequest {
                project: path.clone(),
                base_revision: "1".into(),
                commands,
            }))
            .unwrap_err();
        assert_eq!(error.code, "INVALID_EDIT");
        assert_eq!(export(&path).document, p);
        assert_eq!(export(&path).revision, "1");
    }
    // A raw storage import is allowed to contain invalid compositions; typed
    // service edits and undo still validate the whole resulting candidate.
    let mut imported = p.clone();
    let DocumentObject::Known(c) = &mut imported.compositions[0] else {
        panic!()
    };
    c.root_nodes.clear();
    let mut store = ProjectStore::open(&path, OpenOptions::default()).unwrap();
    let imported_event = store
        .import_json(
            1,
            Uuid::new_v4(),
            &serde_json::to_string(&imported).unwrap(),
        )
        .unwrap();
    store.close().unwrap();
    let u = undo(&path, &imported_event, "undo-invalid-import");
    let before = export(&path);
    let error = service()
        .dispatch(Request::EditUndo(undo_request(
            &path,
            &u,
            "redo-invalid-import",
        )))
        .unwrap_err();
    assert_eq!(error.code, "INVALID_EDIT");
    assert_eq!(export(&path).document, before.document);
    assert_eq!(export(&path).revision, before.revision);
}

#[test]
fn optional_content_collection_inverse_preserves_independent_insertions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("content.kronello");
    let mut p = fixture();
    p.compositions.clear();
    p.shapes.clear();
    p.texts.clear();
    service()
        .dispatch(Request::ProjectCreate(CreateRequest {
            project: path.clone(),
            document: p.clone(),
        }))
        .unwrap();
    let DocumentObject::Known(first) = fixture().shapes.remove(0) else {
        panic!()
    };
    let mut second = first.clone();
    second.id = ContentId::new();
    let a = apply(
        &path,
        vec![EditCommand::ShapeSet {
            shape: first.clone(),
        }],
        "first",
    );
    apply(
        &path,
        vec![EditCommand::ShapeSet {
            shape: second.clone(),
        }],
        "second",
    );
    undo(&path, &a, "undo-first");
    assert_eq!(
        export(&path).document.shapes,
        vec![DocumentObject::Known(second)]
    );
}

#[test]
fn strict_command_decoding_rejects_unknown_and_duplicate_fields() {
    let c = change(&fixture(), 0, 1, 1.25);
    let encoded = serde_json::to_string(&c).unwrap();
    let decoded: EditCommand = serde_json::from_str(&encoded).unwrap();
    assert_eq!(
        serde_json::to_value(c).unwrap(),
        serde_json::to_value(decoded).unwrap()
    );
    let mut json: Json = serde_json::from_str(&encoded).unwrap();
    json["property_source_set"]["changed_keys"] = json!([]);
    assert!(serde_json::from_str::<EditCommand>(&json.to_string()).is_err());
    assert!(serde_json::from_str::<EditCommand>(r#"{"node_remove":{"composition":"e706c050-13fa-4654-9350-dcef30f1d792","node":"adcfcd01-95a0-4292-ba82-61369bf4ac8c","node":"adcfcd01-95a0-4292-ba82-61369bf4ac8c"}}"#).is_err());
}

#[test]
fn shared_curve_edits_derive_all_consumers_and_source_creation_undo() {
    let (_dir, path, p) = setup();
    let DocumentObject::Known(curve) = &p.curves[0] else {
        panic!()
    };
    let second = comp(&p).nodes[1]
        .properties
        .iter()
        .find(|p| p.descriptor().key.as_str() == "kronello.transform.position")
        .unwrap();
    apply(
        &path,
        vec![EditCommand::PropertySourceSet {
            object: comp(&p).nodes[1].id.as_uuid(),
            property: second.id(),
            source: PropertySource::Curve(curve.id()),
            curve: None,
        }],
        "share",
    );
    let mut key = curve.keys()[0].clone();
    key.time = Time::new(1, 2).unwrap();
    let event = apply(
        &path,
        vec![EditCommand::KeyframeInsert {
            curve: curve.id(),
            key,
        }],
        "shared-curve",
    );
    let consumers: Vec<_> = event
        .changed_keys
        .iter()
        .filter(|k| matches!(k, ChangedKey::Value { .. }))
        .collect();
    assert_eq!(consumers.len(), 2);
    undo(&path, &event, "undo-shared");
    let (_dir, path, p) = setup();
    let prop = &comp(&p).nodes[0].properties[1];
    let new_curve = AnimationCurve::new(
        CurveId::new(),
        ValueType::Scalar,
        vec![Keyframe {
            time: Time::ZERO,
            value: scalar(1.0),
            interpolation: CurveInterpolation::Linear,
        }],
    )
    .unwrap();
    let event = apply(
        &path,
        vec![EditCommand::PropertySourceSet {
            object: comp(&p).nodes[0].id.as_uuid(),
            property: prop.id(),
            source: PropertySource::Curve(new_curve.id()),
            curve: Some(new_curve.clone()),
        }],
        "new-curve",
    );
    assert_eq!(export(&path).document.curves.len(), 2);
    undo(&path, &event, "undo-new-curve");
    assert_eq!(export(&path).document, p);
    let event = apply(
        &path,
        vec![EditCommand::PropertySourceSet {
            object: comp(&p).nodes[0].id.as_uuid(),
            property: prop.id(),
            source: PropertySource::Curve(new_curve.id()),
            curve: Some(new_curve.clone()),
        }],
        "new-curve-again",
    );
    let shared = apply(
        &path,
        vec![EditCommand::PropertySourceSet {
            object: comp(&p).nodes[0].id.as_uuid(),
            property: comp(&p).nodes[0].properties[4].id(),
            source: PropertySource::Curve(new_curve.id()),
            curve: None,
        }],
        "bind-created",
    );
    let error = service()
        .dispatch(Request::EditUndo(undo_request(
            &path,
            &event,
            "undo-used-curve",
        )))
        .unwrap_err();
    assert_eq!(error.code, "UNDO_CONFLICT");
    assert_eq!(
        error.details.unwrap()["conflicts"][0]["event_id"],
        shared.id.to_string()
    );
}

#[test]
fn service_commit_failure_rolls_back_document_history_and_receipt() {
    let (_dir, path, p) = setup();
    let planned = plan(&path, "1", vec![change(&p, 0, 1, 2.0)]);
    let mut store = ProjectStore::open(&path, OpenOptions::default()).unwrap();
    store.migrate_schema(1,|tx| {tx.execute_batch("CREATE TRIGGER reject_events BEFORE INSERT ON events BEGIN SELECT RAISE(ABORT,'injected event failure'); END;")?; Ok(())}).unwrap();
    store.close().unwrap();
    let error = apply_request(request(&path, &planned, "rollback")).unwrap_err();
    assert_eq!(error.code, "STORAGE_ERROR");
    assert_eq!(export(&path).document, p);
    assert_eq!(history(&path, "0").events.len(), 1);
    let store = ProjectStore::open(&path, OpenOptions::default()).unwrap();
    assert!(store.idempotency_record("rollback").unwrap().is_none());
    assert_eq!(store.snapshot().unwrap().revision, 1);
    store.close().unwrap();
}

#[test]
fn stable_id_patches_replay_and_compact_with_service_receipts() {
    let (_dir, path, p) = setup();
    let added = apply(
        &path,
        vec![EditCommand::NodeAdd {
            composition: comp(&p).id,
            node: node(NodeKind::Null, &p, None),
            index: 0,
        }],
        "node",
    );
    let at_two = export(&path).document;
    let edited = apply(&path, vec![change(&p, 0, 1, 2.0)], "value");
    let at_three = export(&path).document;
    undo(&path, &edited, "undo-value");
    undo(&path, &added, "undo-node");
    assert_eq!(export(&path).document, p);
    let mut store = ProjectStore::open(&path, OpenOptions::default()).unwrap();
    assert_eq!(store.snapshot_at(2).unwrap().document, at_two);
    assert_eq!(store.snapshot_at(3).unwrap().document, at_three);
    assert_eq!(store.snapshot_at(4).unwrap().document, at_two);
    assert_eq!(store.snapshot_at(5).unwrap().document, p);
    store.compact(3).unwrap();
    assert_eq!(store.snapshot_at(4).unwrap().document, at_two);
    assert_eq!(store.snapshot_at(5).unwrap().document, p);
    assert_eq!(
        store.idempotency_record("node").unwrap().unwrap().result,
        added
    );
    store.close().unwrap();
}

#[test]
fn cross_kind_object_uuid_aliases_are_rejected_before_planning() {
    let (_dir, path, p) = setup();
    let mut aliased = node(NodeKind::Null, &p, None);
    aliased.id = NodeId::from_uuid(comp(&p).id.as_uuid());
    let error = service()
        .dispatch(Request::EditPlan(PlanRequest {
            project: path.clone(),
            base_revision: "1".into(),
            commands: vec![EditCommand::NodeAdd {
                composition: comp(&p).id,
                node: aliased,
                index: 0,
            }],
        }))
        .unwrap_err();
    assert_eq!(error.code, "INVALID_EDIT");
    assert_eq!(export(&path).revision, "1");
    assert_eq!(export(&path).document, p);
}

#[test]
fn concurrent_service_apply_and_undo_are_serialized() {
    use std::sync::{Arc, Barrier};
    let (_dir, path, p) = setup();
    let planned = plan(&path, "1", vec![change(&p, 0, 1, 2.0)]);
    let r = request(&path, &planned, "same");
    let barrier = Arc::new(Barrier::new(3));
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let barrier = barrier.clone();
            let r = r.clone();
            std::thread::spawn(move || {
                barrier.wait();
                apply_request(r)
            })
        })
        .collect();
    barrier.wait();
    let results: Vec<_> = workers
        .into_iter()
        .map(|w| w.join().unwrap().unwrap())
        .collect();
    assert_eq!(results[0], results[1]);
    assert_eq!(export(&path).revision, "2");
    let planned = plan(&path, "2", vec![change(&p, 0, 1, 3.0)]);
    let barrier = Arc::new(Barrier::new(3));
    let workers: Vec<_> = ["left", "right"]
        .into_iter()
        .map(|key| {
            let barrier = barrier.clone();
            let r = request(&path, &planned, key);
            std::thread::spawn(move || {
                barrier.wait();
                apply_request(r)
            })
        })
        .collect();
    barrier.wait();
    let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results.iter().find_map(|r| r.as_ref().err()).unwrap().code,
        "REVISION_CONFLICT"
    );
    let event = results.into_iter().find_map(Result::ok).unwrap();
    let undo = undo_request(&path, &event, "racing-undo");
    let planned = plan(&path, "3", vec![change(&p, 0, 1, 4.0)]);
    let edit = request(&path, &planned, "racing-edit");
    let barrier = Arc::new(Barrier::new(3));
    let one = {
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            service().dispatch(Request::EditUndo(undo))
        })
    };
    let two = {
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            service().dispatch(Request::EditApply(edit))
        })
    };
    barrier.wait();
    let results = [one.join().unwrap(), two.join().unwrap()];
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results.iter().find_map(|r| r.as_ref().err()).unwrap().code,
        "REVISION_CONFLICT"
    );
    assert_eq!(export(&path).revision, "4");
    assert_eq!(history(&path, "0").events.len(), 4);
}

#[test]
fn composition_inverse_and_instance_reference_conflicts() {
    let (_dir, path, p) = setup();
    let mut definition = comp(&p).clone();
    definition.id = CompositionId::new();
    definition.nodes.clear();
    definition.root_nodes.clear();
    definition.properties.clear();
    let created = apply(
        &path,
        vec![EditCommand::CompositionCreate {
            composition: definition.clone(),
        }],
        "create-definition",
    );
    let u = undo(&path, &created, "undo-definition");
    assert_eq!(export(&path).document, p);
    let restored = undo(&path, &u, "redo-definition");
    assert_eq!(export(&path).document.compositions.len(), 2);
    let instance = CompositionInstance {
        id: CompositionInstanceId::new(),
        definition_ref: definition.id,
        input_bindings: Default::default(),
        local_time_map: kronello_time::TimeMap::linear(Time::ZERO, kronello_time::Rational::ONE)
            .unwrap(),
        seed: 0,
    };
    let placed = apply(
        &path,
        vec![EditCommand::InstancePlace {
            composition: comp(&p).id,
            node: node(NodeKind::CompositionInstance(instance), &p, None),
            index: 0,
        }],
        "place-definition",
    );
    let error = service()
        .dispatch(Request::EditUndo(undo_request(
            &path,
            &restored,
            "undo-used-definition",
        )))
        .unwrap_err();
    assert_eq!(error.code, "UNDO_CONFLICT");
    assert_eq!(
        error.details.unwrap()["conflicts"][0]["event_id"],
        placed.id.to_string()
    );
}
