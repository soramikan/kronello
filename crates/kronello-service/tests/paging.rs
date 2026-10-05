//! API-002: stable identities, fixed revisions, and transport-neutral paging.
use kronello_service::{BackendSelection, Response, Service};
use serde_json::{Value, json};

fn execute(request: Value) -> Value {
    serde_json::to_value(
        Service::new(BackendSelection::CpuReference).execute_json(&request.to_string()),
    )
    .unwrap()
}
fn ok(request: Value) -> Value {
    let response = execute(request);
    assert_eq!(response["status"], "success", "{response}");
    response["result"]["value"].clone()
}
fn error(request: Value, code: &str) {
    assert_eq!(execute(request)["error"]["code"], code);
}
struct Fixture {
    _dir: tempfile::TempDir,
    path: std::path::PathBuf,
    doc: Value,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("page.kronello");
        let mut doc: Value =
            serde_json::from_str(include_str!("../../../examples/ffi-preview.project.json"))
                .unwrap();
        doc["id"] = json!(uuid::Uuid::new_v4());
        let original = doc["compositions"][0]["nodes"][0].clone();
        let mut nodes = Vec::new();
        let original_shape = doc["shapes"][0].clone();
        let mut shapes = Vec::new();
        for _ in 0..4 {
            let mut n = original.clone();
            n["id"] = json!(uuid::Uuid::new_v4());
            n["tags"] = json!(["é", "Scene"]);
            let mut shape = original_shape.to_string();
            for property in n["properties"].as_array_mut().unwrap() {
                let old = property["id"].as_str().unwrap().to_owned();
                property["id"] = json!(uuid::Uuid::new_v4());
                shape = shape.replace(&old, property["id"].as_str().unwrap());
            }
            let mut shape: Value = serde_json::from_str(&shape).unwrap();
            shape["id"] = json!(uuid::Uuid::new_v4());
            n["kind"]["value"]["content_ref"] = shape["id"].clone();
            shapes.push(shape);
            nodes.push(n);
        }
        // Storage order deliberately differs from ownership order.
        doc["compositions"][0]["root_nodes"] = nodes.iter().map(|n| n["id"].clone()).collect();
        nodes.reverse();
        doc["shapes"] = json!(shapes);
        doc["compositions"][0]["nodes"] = json!(nodes);
        ok(json!({"operation":"project.create","project":path,"document":doc}));
        Self {
            _dir: dir,
            path,
            doc,
        }
    }
    fn scene(&self) -> Value {
        json!({"operation":"scene.query","project":self.path,"composition":self.doc["compositions"][0]["id"]})
    }
    fn tags(&self, revision: &str, node: Value, tags: Value, key: &str) -> Value {
        let commands = json!([{ "node_tags_set": {"composition":self.doc["compositions"][0]["id"],"node":node,"tags":tags}}]);
        let plan = ok(
            json!({"operation":"edit.plan","project":self.path,"base_revision":revision,"commands":commands}),
        );
        ok(
            json!({"operation":"edit.apply","project":self.path,"base_revision":revision,"commands":commands,"plan_hash":plan["plan_hash"],"session_id":"96607679-eefd-407a-a3a8-59943f2bd82f","idempotency_key":key}),
        )
    }
}
#[test]
fn scene_search_pages_preserve_order_identity_revision_and_evaluation() {
    let f = Fixture::new();
    let mut query = f.scene();
    query["search"] = json!({"tags":["e\u{301}","Scene"],"kinds":["shape"],"range":{"start":{"num":"0","den":"1"},"end":{"num":"1","den":"1"}}});
    for evaluated in [false, true] {
        if evaluated {
            query["evaluation"] = json!({"time":{"num":"0","den":"1"},"fonts":[]});
        }
        let full = ok(query.clone());
        assert_eq!(full["nodes"].as_array().unwrap().len(), 4);
        assert_eq!(
            full["nodes"][0]["key"]["node"],
            f.doc["compositions"][0]["root_nodes"][0]
        );
        query["limit"] = json!(1);
        let first = ok(query.clone());
        let mut page = first.clone();
        let mut rows = vec![page["nodes"][0].clone()];
        while let Some(cursor) = page.get("next_cursor") {
            query["cursor"] = cursor.clone();
            page = ok(query.clone());
            assert_eq!(page["revision"], full["revision"]);
            assert_eq!(page["roots"], full["roots"]);
            rows.extend(page["nodes"].as_array().unwrap().iter().cloned());
        }
        assert_eq!(rows, *full["nodes"].as_array().unwrap());
        query.as_object_mut().unwrap().remove("cursor");
        query.as_object_mut().unwrap().remove("limit");
        if evaluated {
            assert!(rows[0].get("evaluated").is_some());
        }
    }
    query["search"]["tags"] = json!(["scene"]);
    assert!(ok(query.clone())["nodes"].as_array().unwrap().is_empty());
    query["search"] = json!({"range":{"start":{"num":"3","den":"1"},"end":{"num":"4","den":"1"}}});
    assert!(ok(query.clone())["nodes"].as_array().unwrap().is_empty());
    query["search"]["range"]["end"] = json!({"num":"3","den":"1"});
    error(query.clone(), "INVALID_REQUEST");
    query["search"]["range"]["end"] = json!({"num":"2","den":"1"});
    error(query, "INVALID_REQUEST");
}
#[test]
fn scene_cursor_survives_edits_and_rejects_changed_parameters_and_expiry() {
    let f = Fixture::new();
    let mut query = f.scene();
    query["limit"] = json!(1);
    let first = ok(query.clone());
    let node = f.doc["compositions"][0]["root_nodes"][1].clone();
    f.tags("1", node, json!(["changed"]), "tags");
    query["cursor"] = first["next_cursor"].clone();
    let second = ok(query.clone());
    assert_eq!(second["revision"], "1");
    assert_eq!(second["nodes"][0]["tags"], json!(["Scene", "é"]));
    for (field, value) in [
        ("limit", json!(2)),
        ("expand_instances", json!(true)),
        ("search", json!({"kinds":["text"]})),
        (
            "evaluation",
            json!({"time":{"num":"0","den":"1"},"fonts":[]}),
        ),
    ] {
        let mut changed = query.clone();
        changed[field] = value;
        error(changed, "CURSOR_MISMATCH");
    }
    let other = Fixture::new();
    let mut changed = query.clone();
    changed["project"] = json!(other.path);
    error(changed, "CURSOR_MISMATCH");
    let mut bad = query.clone();
    bad["cursor"] = json!("k1.00.bad");
    error(bad, "INVALID_CURSOR");
    let mut store = kronello_store::ProjectStore::open(&f.path, Default::default()).unwrap();
    store.compact(2).unwrap();
    store.close().unwrap();
    error(query, "CURSOR_EXPIRED");
}
#[test]
fn fixed_history_pages_ignore_concurrent_edit_undo_redo_and_expire_on_compact() {
    let f = Fixture::new();
    let a = f.tags(
        "1",
        f.doc["compositions"][0]["root_nodes"][0].clone(),
        json!(["a"]),
        "a",
    );
    f.tags(
        "2",
        f.doc["compositions"][0]["root_nodes"][1].clone(),
        json!(["b"]),
        "b",
    );
    let mut query =
        json!({"operation":"history.list","project":f.path,"since_revision":"0","limit":1});
    let first = ok(query.clone());
    let undo = ok(
        json!({"operation":"edit.undo","project":f.path,"base_revision":"3","session_id":uuid::Uuid::new_v4(),"idempotency_key":"undo","event_id":a["id"]}),
    );
    query["cursor"] = first["next_cursor"].clone();
    let during_undo = ok(query.clone());
    assert_eq!(during_undo["revision"], "3");
    assert_eq!(during_undo["events"][0]["event"]["id"], a["id"]);
    assert_eq!(during_undo["events"][0]["undone"], false);
    let redo = ok(
        json!({"operation":"edit.undo","project":f.path,"base_revision":"4","session_id":uuid::Uuid::new_v4(),"idempotency_key":"redo","event_id":undo["id"]}),
    );
    f.tags(
        "5",
        f.doc["compositions"][0]["root_nodes"][2].clone(),
        json!(["c"]),
        "c",
    );
    let mut rows = first["events"].as_array().unwrap().clone();
    let mut page = first.clone();
    while let Some(cursor) = page.get("next_cursor") {
        query["cursor"] = cursor.clone();
        page = ok(query.clone());
        assert_eq!(page["revision"], "3");
        rows.extend(page["events"].as_array().unwrap().iter().cloned());
    }
    assert_eq!(rows.len(), 3);
    let ids: std::collections::BTreeSet<_> = rows
        .iter()
        .map(|e| e["event"]["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 3);
    assert!(rows.iter().all(|e| e["undone"] == false));
    // A new fixed snapshot captures undone=true; subsequent Redo must not mix it.
    let undone_again = ok(
        json!({"operation":"edit.undo","project":f.path,"base_revision":"6","session_id":uuid::Uuid::new_v4(),"idempotency_key":"undo-again","event_id":redo["id"]}),
    );
    let mut filtered = json!({"operation":"history.list","project":f.path,"since_revision":"1","limit":1,"session_id":"96607679-eefd-407a-a3a8-59943f2bd82f"});
    let undone = ok(filtered.clone());
    assert_eq!(undone["events"][0]["undone"], true);
    filtered["cursor"] = undone["next_cursor"].clone();
    assert_eq!(ok(filtered.clone())["revision"], "7");
    let mut frozen = json!({"operation":"history.list","project":f.path,"limit":1});
    let frozen_first = ok(frozen.clone());
    ok(
        json!({"operation":"edit.undo","project":f.path,"base_revision":"7","session_id":uuid::Uuid::new_v4(),"idempotency_key":"redo-again","event_id":undone_again["id"]}),
    );
    frozen["cursor"] = frozen_first["next_cursor"].clone();
    let still_undone = ok(frozen);
    assert_eq!(still_undone["revision"], "7");
    assert_eq!(still_undone["events"][0]["event"]["id"], a["id"]);
    assert_eq!(still_undone["events"][0]["undone"], true);
    query["cursor"] = first["next_cursor"].clone();
    for (field, value) in [
        ("limit", json!(2)),
        ("since_revision", json!("1")),
        ("session_id", json!(uuid::Uuid::new_v4())),
    ] {
        let mut changed = query.clone();
        changed[field] = value;
        error(changed, "CURSOR_MISMATCH");
    }
    let mut store = kronello_store::ProjectStore::open(&f.path, Default::default()).unwrap();
    store.compact(4).unwrap();
    store.close().unwrap();
    error(query, "CURSOR_EXPIRED");
}
#[test]
fn tags_are_validated_round_tripped_and_selectively_undoable() {
    let f = Fixture::new();
    let node = f.doc["compositions"][0]["root_nodes"][0].clone();
    for tags in [
        json!([""]),
        json!([" leading"]),
        json!(["trailing "]),
        json!(["a\nb"]),
        json!(["e\u{301}"]),
        json!(["界".repeat(22)]),
        json!((0..33).map(|n| n.to_string()).collect::<Vec<_>>()),
    ] {
        error(
            json!({"operation":"edit.plan","project":f.path,"base_revision":"1","commands":[{"node_tags_set":{"composition":f.doc["compositions"][0]["id"],"node":node,"tags":tags}}]}),
            "INVALID_REQUEST",
        );
    }
    let event = f.tags("1", node.clone(), json!(["日本語", "é"]), "replace");
    let exported = ok(json!({"operation":"project.export","project":f.path}));
    assert_eq!(exported["document"]["schema_version"], 1);
    let nodes = exported["document"]["compositions"][0]["nodes"]
        .as_array()
        .unwrap();
    assert_eq!(
        nodes.iter().find(|n| n["id"] == node).unwrap()["tags"],
        json!(["é", "日本語"])
    );
    ok(
        json!({"operation":"edit.undo","project":f.path,"base_revision":"2","session_id":uuid::Uuid::new_v4(),"idempotency_key":"undo","event_id":event["id"]}),
    );
    let scene = ok(f.scene());
    assert_eq!(scene["nodes"][0]["tags"], json!(["Scene", "é"]));
    let mut doc = f.doc.clone();
    doc["compositions"][0]["nodes"][0]["tags"] = json!([""]);
    let response = Service::new(BackendSelection::CpuReference).execute_json(&json!({"operation":"project.create","project":f._dir.path().join("bad.kronello"),"document":doc}).to_string());
    assert!(matches!(response, Response::Error { error } if error.code == "INVALID_MUTATION"));
}

#[test]
fn history_reader_racing_a_writer_and_compaction_returns_only_fixed_rows_or_expiry() {
    let f = Fixture::new();
    f.tags(
        "1",
        f.doc["compositions"][0]["root_nodes"][0].clone(),
        json!(["a"]),
        "race-a",
    );
    f.tags(
        "2",
        f.doc["compositions"][0]["root_nodes"][1].clone(),
        json!(["b"]),
        "race-b",
    );
    let expected = ok(json!({"operation":"history.list","project":f.path,"limit":100}));
    let mut query = json!({"operation":"history.list","project":f.path,"limit":1});
    let first = ok(query.clone());
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let writer_barrier = barrier.clone();
    let path = f.path.clone();
    let writer = std::thread::spawn(move || {
        let mut store = kronello_store::ProjectStore::open(&path, Default::default()).unwrap();
        writer_barrier.wait();
        for _ in 0..8 {
            let current = store.snapshot().unwrap();
            store
                .import_json(
                    current.revision,
                    uuid::Uuid::new_v4(),
                    &serde_json::to_string(&current.document).unwrap(),
                )
                .unwrap();
        }
        store.compact(4).unwrap();
        store.close().unwrap();
    });
    barrier.wait();
    let mut rows = first["events"].as_array().unwrap().clone();
    let mut page = first;
    while let Some(cursor) = page.get("next_cursor") {
        query["cursor"] = cursor.clone();
        let response = execute(query.clone());
        if response["status"] == "error" {
            assert_eq!(response["error"]["code"], "CURSOR_EXPIRED");
            break;
        }
        page = response["result"]["value"].clone();
        assert_eq!(page["revision"], expected["revision"]);
        rows.extend(page["events"].as_array().unwrap().iter().cloned());
    }
    assert_eq!(rows, expected["events"].as_array().unwrap()[..rows.len()]);
    writer.join().unwrap();
    error(query, "CURSOR_EXPIRED");
}
