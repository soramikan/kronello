use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use kronello_model::*;
use kronello_service::*;
use kronello_store::{ChangedKey, Event, Mutation};
use kronello_time::Time;
use serde_json::{Value as Json, json};
use uuid::Uuid;

fn document() -> Project {
    let mut p: Project =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    let DocumentObject::Known(c) = &mut p.compositions[0] else {
        panic!()
    };
    c.nodes.truncate(1);
    c.root_nodes.truncate(1);
    c.nodes[0].kind = NodeKind::Null;
    c.nodes[0].properties.retain(|p| {
        p.descriptor().key.as_str() == "kronello.opacity"
            || p.descriptor().key.as_str() == "kronello.transform.position"
    });
    p.shapes.clear();
    p.texts.clear();
    p
}
fn comp(p: &Project) -> &Composition {
    let DocumentObject::Known(c) = &p.compositions[0] else {
        panic!()
    };
    c
}
fn property(p: &Project) -> &Property {
    comp(p).nodes[0]
        .properties
        .iter()
        .find(|p| p.descriptor().key.as_str() == "kronello.opacity")
        .unwrap()
}
fn scalar(v: f64) -> Value {
    Value::Scalar(FiniteF64::new(v).unwrap())
}
fn modifier(enabled: bool) -> Modifier {
    Modifier {
        id: ModifierId::new(),
        key: SchemaKey::new("example.unsupported").unwrap(),
        version: 1,
        enabled,
        parameters: BTreeMap::from([("amount".into(), scalar(0.5))]),
    }
}
fn run(value: Json) -> Result<ResultData, ServiceError> {
    let request: Request = serde_json::from_str(&value.to_string()).unwrap();
    Service::new(BackendSelection::CpuReference).dispatch(request)
}
fn exported(path: &Path) -> ExportResult {
    let ResultData::Export(r) = run(json!({"operation":"project.export","project":path})).unwrap()
    else {
        panic!()
    };
    *r
}
fn plan(path: &Path, commands: Json) -> EditPlan {
    let ResultData::Plan(p) = run(json!({"operation":"edit.plan","project":path,"base_revision":exported(path).revision,"commands":commands})).unwrap() else { panic!() };
    *p
}
fn apply(path: &Path, p: &EditPlan, key: &str) -> Event {
    let request = json!({"operation":"edit.apply","project":path,"base_revision":p.base_revision,"plan_hash":p.plan_hash,"commands":p.commands,"idempotency_key":key,"session_id":Uuid::from_u128(11)});
    let ResultData::Edit(e) = run(request.clone()).unwrap() else {
        panic!()
    };
    let ResultData::Edit(replay) = run(request).unwrap() else {
        panic!()
    };
    assert_eq!(e, replay);
    e
}
fn undo_request(path: &Path, e: &Event, key: &str) -> Json {
    json!({"operation":"edit.undo","project":path,"base_revision":exported(path).revision,"event_id":e.id,"session_id":Uuid::from_u128(12),"idempotency_key":key})
}
fn undo(path: &Path, e: &Event, key: &str) -> Event {
    let ResultData::Edit(e) = run(undo_request(path, e, key)).unwrap() else {
        panic!()
    };
    e
}
fn sample_request(path: &Path, p: &Project) -> Json {
    json!({"operation":"property.sample","project":path,"composition":comp(p).id,"keys":[{"kind":"node","instance_path":[],"node":comp(p).nodes[0].id,"property":property(p).id()}],"times":[Time::ZERO]})
}

#[test]
fn modifier_order_inverse_conflicts_and_selective_undo() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("modifiers.kronello");
    let p = document();
    run(json!({"operation":"project.create","project":path,"document":p})).unwrap();
    let object = comp(&p).nodes[0].id;
    let id = property(&p).id();
    let a = modifier(false);
    let b = modifier(false);
    let inserted = plan(
        &path,
        json!([
            {"modifier_insert":{"object":object,"property":id,"modifier":a,"index":0}},
            {"modifier_insert":{"object":object,"property":id,"modifier":b,"index":0}}
        ]),
    );
    assert_eq!(
        property(&inserted.candidate).modifiers(),
        &[b.clone(), a.clone()]
    );
    assert_eq!(
        inserted.changed_keys,
        BTreeSet::from([ChangedKey::Value {
            object_id: object.as_uuid(),
            property_id: id
        }])
    );
    let first = apply(&path, &inserted, "insert-modifiers");
    let reordered = plan(
        &path,
        json!([{ "modifier_reorder":{"object":object,"property":id,"order":[a.id,b.id]} }]),
    );
    assert!(
        matches!(&reordered.mutations[..], [Mutation::Set {path, ..}] if path.last().unwrap() == "modifiers")
    );
    let mut value = serde_json::to_value(&reordered.candidate).unwrap();
    for inverse in &reordered.inverse {
        inverse.apply(&mut value).unwrap();
    }
    assert_eq!(
        serde_json::from_value::<Project>(value).unwrap(),
        inserted.candidate
    );
    let second = apply(&path, &reordered, "reorder-modifiers");
    let err = run(undo_request(&path, &first, "conflicting-undo")).unwrap_err();
    assert_eq!(err.code, "UNDO_CONFLICT");
    assert!(
        err.details
            .unwrap()
            .to_string()
            .contains(&second.id.to_string())
    );
    let order_inverse = undo(&path, &second, "undo-order");
    let inverse_conflict = run(undo_request(&path, &first, "active-inverse-conflict")).unwrap_err();
    assert_eq!(inverse_conflict.code, "UNDO_CONFLICT");
    assert!(
        inverse_conflict
            .details
            .unwrap()
            .to_string()
            .contains(&order_inverse.id.to_string())
    );
    assert_eq!(
        property(&exported(&path).document).modifiers(),
        &[b.clone(), a.clone()]
    );
    // An unrelated Property update must survive selective Undo of the chain.
    let position = comp(&p).nodes[0]
        .properties
        .iter()
        .find(|p| p.descriptor().key.as_str() == "kronello.transform.position")
        .unwrap();
    let selective_path = dir.path().join("selective.kronello");
    run(json!({"operation":"project.create","project":selective_path,"document":p})).unwrap();
    let selective_plan = plan(
        &selective_path,
        serde_json::to_value(&inserted.commands).unwrap(),
    );
    let selective_first = apply(&selective_path, &selective_plan, "selective-insert");
    let unrelated = plan(
        &selective_path,
        json!([{ "property_source_set":{"object":object,"property":position.id(),"source":{"kind":"constant","value":{"kind":"vec2","value":[12.0,18.0]}}} }]),
    );
    apply(&selective_path, &unrelated, "unrelated-position");
    let undone = undo(&selective_path, &selective_first, "undo-insert");
    let saved = exported(&selective_path);
    assert!(property(&saved.document).modifiers().is_empty());
    assert_eq!(
        comp(&saved.document).nodes[0]
            .properties
            .iter()
            .find(|v| v.id() == position.id())
            .unwrap()
            .source(),
        &PropertySource::Constant(Value::Vec2([
            FiniteF64::new(12.0).unwrap(),
            FiniteF64::new(18.0).unwrap()
        ]))
    );
    undo(&selective_path, &undone, "redo-insert");
    assert_eq!(
        property(&exported(&selective_path).document).modifiers(),
        &[b.clone(), a.clone()]
    );
    let mut replaced = b.clone();
    replaced.parameters.insert("amount".into(), scalar(0.9));
    let changed = plan(
        &path,
        json!([
            {"modifier_replace":{"object":object,"property":id,"modifier":replaced}},
            {"modifier_remove":{"object":object,"property":id,"modifier":a.id}}
        ]),
    );
    let changed_event = apply(&path, &changed, "replace-remove");
    assert_eq!(property(&exported(&path).document).modifiers(), &[replaced]);
    undo(&path, &changed_event, "undo-replace-remove");
    assert_eq!(property(&exported(&path).document).modifiers(), &[b, a]);
    // A source update is on the same Value key as a Modifier update.
    let chain = plan(
        &path,
        json!([{ "modifier_reorder":{"object":object,"property":id,"order":property(&exported(&path).document).modifiers().iter().rev().map(|m|m.id).collect::<Vec<_>>()} }]),
    );
    let chain_event = apply(&path, &chain, "chain-for-source-conflict");
    let source = plan(
        &path,
        json!([{ "property_source_set":{"object":object,"property":id,"source":{"kind":"constant","value":scalar(0.7)}} }]),
    );
    apply(&path, &source, "same-property-source");
    assert_eq!(
        run(undo_request(&path, &chain_event, "source-conflict"))
            .unwrap_err()
            .code,
        "UNDO_CONFLICT"
    );
}

#[test]
fn unsupported_modifiers_preserve_storage_but_block_evaluation_and_final_render() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("boundary.kronello");
    let p = document();
    run(json!({"operation":"project.create","project":path,"document":p})).unwrap();
    let object = comp(&p).nodes[0].id;
    let id = property(&p).id();
    let mut m = modifier(true);
    let expression = Expression {
        id: ExpressionId::new(),
        version: EXPRESSION_VERSION,
        value_type: ValueType::Scalar,
        budget: Default::default(),
        nodes: vec![ExpressionNode::Literal(scalar(0.4))],
    };
    let planned = plan(
        &path,
        json!([
            {"expression_set":{"expression":expression}},
            {"property_source_set":{"object":object,"property":id,"source":{"kind":"expression","value":expression.id}}},
            {"modifier_insert":{"object":object,"property":id,"modifier":m,"index":0}}
        ]),
    );
    let event = apply(&path, &planned, "expression-modifier");
    assert_eq!(
        property(&exported(&path).document).modifiers(),
        &[m.clone()]
    );
    assert_eq!(
        run(sample_request(&path, &p)).unwrap_err().code,
        "UNSUPPORTED_FEATURE"
    );
    let input = json!({"project":path,"composition":comp(&p).id,"region":{"origin":[0,0],"extent":[64,32],"pixels":[8,4]}});
    assert_eq!(
        run(json!({"operation":"render.frame","input":input,"time":Time::ZERO}))
            .unwrap_err()
            .code,
        "UNSUPPORTED_FEATURE"
    );
    let output = dir.path().join("frames");
    assert_eq!(run(json!({"operation":"render.sequence","input":input,"range":{"start":Time::ZERO,"end":Time::new(1,1).unwrap()},"frame_rate":{"num":"1","den":"1"},"output_directory":output})).unwrap_err().code,"UNSUPPORTED_FEATURE");
    assert!(!output.exists());
    let undone = undo(&path, &event, "undo-expression-modifier");
    assert_eq!(exported(&path).document, p);
    undo(&path, &undone, "redo-expression-modifier");
    // Disabled means explicitly bypassed by the existing evaluator, not a
    // substitute for an enabled unsupported algorithm.
    m.enabled = false;
    let disabled = plan(
        &path,
        json!([{ "modifier_replace":{"object":object,"property":id,"modifier":m} }]),
    );
    let disable_event = apply(&path, &disabled, "disable-modifier");
    let ResultData::Samples(sample) = run(sample_request(&path, &p)).unwrap() else {
        panic!()
    };
    assert_eq!(sample.samples[0].values, vec![scalar(0.4)]);
    assert!(run(json!({"operation":"render.frame","input":input,"time":Time::ZERO})).is_ok());
    undo(&path, &disable_event, "undo-disable");
    assert_eq!(
        run(sample_request(&path, &p)).unwrap_err().code,
        "UNSUPPORTED_FEATURE"
    );
    assert!(property(&exported(&path).document).modifiers()[0].enabled);
}

#[test]
fn modifier_commands_reject_invalid_ids_order_versions_and_raw_history_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("invalid.kronello");
    let p = document();
    run(json!({"operation":"project.create","project":path,"document":p})).unwrap();
    let object = comp(&p).nodes[0].id;
    let id = property(&p).id();
    let m = modifier(false);
    let inserted = plan(
        &path,
        json!([{ "modifier_insert":{"object":object,"property":id,"modifier":m,"index":0} }]),
    );
    apply(&path, &inserted, "initial");
    let snapshot = exported(&path);
    let mut bad_version = m.clone();
    bad_version.version = 0;
    for command in [
        json!({"modifier_insert":{"object":object,"property":id,"modifier":m,"index":0}}),
        json!({"modifier_insert":{"object":object,"property":id,"modifier":modifier(false),"index":2}}),
        json!({"modifier_replace":{"object":object,"property":id,"modifier":modifier(false)}}),
        json!({"modifier_replace":{"object":object,"property":id,"modifier":bad_version}}),
        json!({"modifier_remove":{"object":object,"property":id,"modifier":ModifierId::new()}}),
        json!({"modifier_reorder":{"object":object,"property":id,"order":[m.id,m.id]}}),
        json!({"modifier_reorder":{"object":object,"property":id,"order":[]}}),
        json!({"modifier_reorder":{"object":object,"property":id,"order":[ModifierId::new()]}}),
        json!({"modifier_remove":{"object":Uuid::new_v4(),"property":id,"modifier":m.id}}),
        json!({"modifier_remove":{"object":object,"property":PropertyId::new(),"modifier":m.id}}),
    ] {
        assert_eq!(run(json!({"operation":"edit.plan","project":path,"base_revision":snapshot.revision,"commands":[command]})).unwrap_err().code,"INVALID_EDIT");
    }
    for field in ["patch", "mutations", "inverse", "changed_keys"] {
        let mut request = json!({"operation":"edit.plan","project":path,"base_revision":snapshot.revision,"commands":[]});
        request[field] = json!([]);
        let Response::Error { error } =
            Service::new(BackendSelection::CpuReference).execute_json(&request.to_string())
        else {
            panic!()
        };
        assert_eq!(error.code, "INVALID_REQUEST");
    }
    assert_eq!(exported(&path).document, snapshot.document);
    assert_eq!(exported(&path).revision, snapshot.revision);
}
