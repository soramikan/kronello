use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

use kronello_service::{BackendSelection, ExecutionControl, Response, Service};
use serde_json::{Value, json};

struct Control {
    cancelled: AtomicBool,
    progress: Mutex<Vec<(u64, u64)>>,
}
impl ExecutionControl for Control {
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
    fn progress(&self, completed: u64, total: u64) {
        self.progress.lock().unwrap().push((completed, total));
        if completed == 1 {
            self.cancelled.store(true, Ordering::SeqCst);
        }
    }
}
fn document() -> Value {
    let mut document: Value =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    document["compositions"][0]["nodes"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    document["compositions"][0]["root_nodes"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    document["texts"] = json!([]);
    document
}

#[test]
fn sequence_cancellation_uses_service_checkpoints_and_removes_partial_output() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("source.kronello");
    let destination = temp.path().join("frames");
    let service = Service::new(BackendSelection::CpuReference);
    let doc = document();
    let create = service.execute_json(
        &json!({"operation":"project.create","project":path,"document":doc}).to_string(),
    );
    assert!(matches!(create, Response::Success { .. }), "{create:?}");
    let control = Control {
        cancelled: AtomicBool::new(false),
        progress: Mutex::new(Vec::new()),
    };
    let response = service.execute_json_with_control(&json!({"operation":"render.sequence","input":{"project":path,"composition":doc["compositions"][0]["id"],"region":{"origin":[0.0,0.0],"extent":[64.0,32.0],"pixels":[8,4]}},"range":{"start":{"num":"0","den":"1"},"end":{"num":"1","den":"1"}},"frame_rate":{"num":"4","den":"1"},"output_directory":destination}).to_string(), &control);
    match response {
        Response::Error { error } => assert_eq!(error.code, "REQUEST_CANCELLED"),
        _ => panic!("{response:?}"),
    }
    assert_eq!(*control.progress.lock().unwrap(), vec![(0, 4), (1, 4)]);
    assert!(!destination.exists());
}

#[test]
fn controlled_dispatch_stops_before_project_creation_and_queries_are_read_only() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("source.kronello");
    let service = Service::new(BackendSelection::CpuReference);
    let payload =
        json!({"operation":"project.create","project":path,"document":document()}).to_string();
    let control = Control {
        cancelled: AtomicBool::new(true),
        progress: Mutex::new(Vec::new()),
    };
    assert!(
        matches!(service.execute_json_with_control(&payload, &control), Response::Error { error } if error.code == "REQUEST_CANCELLED")
    );
    assert!(!path.exists());
    assert!(matches!(
        service.execute_json(&payload),
        Response::Success { .. }
    ));
    let service = service.with_read_only_inspection();
    let before = std::fs::read(&path).unwrap();
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    for operation in ["project.info", "project.export"] {
        assert!(matches!(
            service.execute_json(&json!({"operation":operation,"project":path}).to_string()),
            Response::Success { .. }
        ));
    }
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        modified
    );
    // Inspection leaves no persistent writer/exclusive lock: a normal writer
    // can open and close afterward, applying its usual sidecar cleanup policy.
    let writer =
        kronello_store::ProjectStore::open(&path, kronello_store::OpenOptions::default()).unwrap();
    writer.close().unwrap();
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
}
