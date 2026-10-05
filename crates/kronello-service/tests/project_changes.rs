use kronello_model::Project;
use kronello_service::*;
use kronello_store::{OpenOptions, ProjectStore};
use serde_json::{Value, json};

fn execute(request: Value) -> Response {
    Service::new(BackendSelection::CpuReference).execute_json(&request.to_string())
}
fn ok(request: Value) -> Value {
    let response = execute(request);
    assert!(matches!(response, Response::Success { .. }), "{response:?}");
    serde_json::to_value(response).unwrap()["result"]["value"].clone()
}
fn error(request: Value, code: &str) {
    let Response::Error { error } = execute(request) else {
        panic!()
    };
    assert_eq!(error.code, code, "{error:?}");
}

#[test]
fn project_plans_bind_target_and_snapshot_and_receipts_survive_compaction() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("create.kronello");
    let doc = Project::default();
    let plan = ok(json!({"operation":"project.create_plan","project":path,"document":doc}));
    let other = dir.path().join("other.kronello");
    error(
        json!({"operation":"project.create","project":other,"document":doc,"plan_hash":plan["plan_hash"],"idempotency_key":"other"}),
        "PLAN_HASH_MISMATCH",
    );
    assert!(!other.exists());
    let create = json!({"operation":"project.create","project":path,"document":doc,"plan_hash":plan["plan_hash"],"idempotency_key":"create"});
    let created = ok(create.clone());
    let mut changed = doc.clone();
    changed.name = "imported".into();
    let plan = ok(
        json!({"operation":"project.import_plan","project":path,"base_revision":"1","document":changed}),
    );
    let import = json!({"operation":"project.import","project":path,"base_revision":"1","document":changed,"plan_hash":plan["plan_hash"],"idempotency_key":"import"});
    let imported = ok(import.clone());
    ok(json!({"operation":"project.import","project":path,"base_revision":"2","document":doc}));
    let mut store = ProjectStore::open(&path, OpenOptions::default()).unwrap();
    store.compact(3).unwrap();
    assert!(
        store
            .events_since(0)
            .unwrap()
            .iter()
            .all(|e| e.revision == 3)
    );
    store.close().unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(ok(create), created);
    assert_eq!(ok(import), imported);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    // Same revision in a replacement file must not make an old import plan valid.
    let replacement = dir.path().join("replacement.kronello");
    let replacement_doc = Project::default();
    ok(json!({"operation":"project.create","project":replacement,"document":replacement_doc}));
    let plan = ok(
        json!({"operation":"project.import_plan","project":replacement,"base_revision":"1","document":doc}),
    );
    std::fs::remove_file(&replacement).unwrap();
    let mut different = replacement_doc;
    different.name = "replaced file".into();
    ok(json!({"operation":"project.create","project":replacement,"document":different}));
    error(
        json!({"operation":"project.import","project":replacement,"base_revision":"1","document":doc,"plan_hash":plan["plan_hash"],"idempotency_key":"replacement"}),
        "PLAN_HASH_MISMATCH",
    );
    assert_eq!(
        ok(json!({"operation":"project.info","project":replacement}))["revision"],
        "1"
    );
}

#[test]
fn project_plans_preserve_opaque_data_and_reject_client_history_and_invalid_keys() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("opaque.kronello");
    let mut doc = serde_json::to_value(Project::default()).unwrap();
    doc["future"] = json!({"literal":"$(inert text)","value":42});
    let plan = ok(json!({"operation":"project.create_plan","project":path,"document":doc}));
    assert_eq!(plan["candidate"]["future"], doc["future"]);
    ok(
        json!({"operation":"project.create","project":path,"document":doc,"plan_hash":plan["plan_hash"],"idempotency_key":"opaque"}),
    );
    assert_eq!(
        ok(json!({"operation":"project.export","project":path}))["document"],
        doc
    );
    let mut changed = doc.clone();
    changed["future"]["value"] = json!(43);
    let plan = ok(
        json!({"operation":"project.import_plan","project":path,"base_revision":"1","document":changed}),
    );
    ok(
        json!({"operation":"project.import","project":path,"base_revision":"1","document":changed,"plan_hash":plan["plan_hash"],"idempotency_key":"opaque-import"}),
    );
    assert_eq!(
        ok(json!({"operation":"project.export","project":path}))["document"],
        changed
    );
    let before = std::fs::read(&path).unwrap();
    for operation in [
        "project.create_plan",
        "project.import_plan",
        "project.create",
        "project.import",
    ] {
        for field in ["patch", "mutations", "inverse", "changed_keys"] {
            let mut request = json!({"operation":operation,"project":path,"document":doc});
            if operation.contains("import") {
                request["base_revision"] = json!("2");
            }
            request[field] = json!([]);
            error(request, "INVALID_REQUEST");
        }
    }
    for key in ["".to_owned(), "x".repeat(257)] {
        error(
            json!({"operation":"project.create","project":path,"document":doc,"idempotency_key":key}),
            "INVALID_REQUEST",
        );
        error(
            json!({"operation":"project.import","project":path,"base_revision":"2","document":doc,"idempotency_key":key}),
            "INVALID_REQUEST",
        );
    }
    assert_eq!(std::fs::read(&path).unwrap(), before);
}
