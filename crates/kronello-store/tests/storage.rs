use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use kronello_model::{
    AnimationCurve, Composition, CompositionId, CurveId, CurveInterpolation, DesignExtent,
    DocumentObject, FiniteF64, Keyframe, Project, PropertyId, Value as ModelValue, ValueType,
};
use kronello_store::{
    ApplyRequest, ChangedKey, DetectedLocation, LocationDetector, Mutation, OpenMode, OpenOptions,
    ProjectStore, StoreError, detect_location, render_cache_location,
};
use kronello_time::{Duration as ModelDuration, FrameRate, Time};
use rusqlite::Connection;
use serde_json::{Value, json};
use tempfile::TempDir;
use uuid::Uuid;

fn options(mode: OpenMode) -> OpenOptions {
    OpenOptions { mode }
}
fn open(path: &Path) -> ProjectStore {
    ProjectStore::open(path, options(OpenMode::ForceNormal)).unwrap()
}
fn fixture() -> (TempDir, PathBuf, ProjectStore) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("test.kronello");
    let store = open(&path);
    (directory, path, store)
}
fn request(revision: u64, name: &str) -> ApplyRequest {
    ApplyRequest {
        base_revision: revision,
        session_id: Uuid::new_v4(),
        mutations: vec![Mutation::Set {
            path: vec!["name".into()],
            value: json!(name),
        }],
        changed_keys: BTreeSet::from([
            ChangedKey::Value {
                object_id: Uuid::new_v4(),
                property_id: PropertyId::new(),
            },
            ChangedKey::Structure {
                object_id: Uuid::new_v4(),
                parent_container_id: Uuid::new_v4(),
            },
        ]),
        idempotency_key: None,
        undo_of: None,
    }
}
fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut path = path.as_os_str().to_owned();
    path.push(suffix);
    path.into()
}

#[test]
fn atomic_apply_persists_event_keys_inverse_receipt_and_snapshot() {
    let (_directory, path, mut store) = fixture();
    let mut update = request(0, "first");
    update.idempotency_key = Some("retry-1".into());
    update.undo_of = Some(Uuid::new_v4());
    let session = update.session_id;
    let keys = update.changed_keys.clone();
    let event = store.apply(update.clone()).unwrap();
    assert_eq!(
        store.idempotency_record("retry-1").unwrap().unwrap().result,
        event
    );
    assert!(store.idempotency_record("absent").unwrap().is_none());
    assert_eq!(event.revision, 1);
    assert_eq!(event.session_id, session);
    assert_eq!(event.changed_keys, keys);
    assert_eq!(event.undo_of, update.undo_of);
    assert_eq!(
        event.inverse,
        vec![Mutation::Set {
            path: vec!["name".into()],
            value: json!("")
        }]
    );
    assert_eq!(store.snapshot().unwrap(), store.snapshot_at(1).unwrap());
    assert_eq!(store.events_since(0).unwrap(), vec![event.clone()]);
    // A second connection sees document, revision, event and receipt together.
    let observer = open(&path);
    assert_eq!(observer.snapshot().unwrap().document.name, "first");
    assert_eq!(observer.events_since(0).unwrap(), vec![event.clone()]);
    let sql = Connection::open(&path).unwrap();
    let receipt: (String, i64, String) = sql
        .query_row(
            "SELECT event_id,revision,payload FROM idempotency WHERE key='retry-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(receipt.0, event.id.to_string());
    assert_eq!(receipt.1, 1);
    assert_eq!(
        serde_json::from_str::<Value>(&receipt.2).unwrap(),
        serde_json::to_value(&update).unwrap()
    );
    let mut inverse = request(1, "unused");
    inverse.mutations = event.inverse;
    inverse.undo_of = Some(event.id);
    store.apply(inverse).unwrap();
    assert_eq!(store.snapshot().unwrap().document.name, "");
    update.base_revision = 2;
    assert_eq!(
        store.apply(update).unwrap_err().code(),
        "IDEMPOTENCY_KEY_EXISTS"
    );
}

#[test]
fn failure_after_document_update_rolls_back_all_tables() {
    let (_directory, path, mut store) = fixture();
    let sql = Connection::open(&path).unwrap();
    sql.execute_batch("CREATE TRIGGER reject_event BEFORE INSERT ON events BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
    let before = store.snapshot().unwrap();
    let mut update = request(0, "must roll back");
    update.idempotency_key = Some("failed".into());
    assert!(store.apply(update).is_err());
    assert_eq!(store.snapshot().unwrap(), before);
    assert_eq!(store.snapshot_at(0).unwrap(), before);
    assert!(store.snapshot_at(1).is_err());
    assert!(store.events_since(0).unwrap().is_empty());
    assert_eq!(
        sql.query_row("SELECT count(*) FROM idempotency", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    sql.execute_batch("DROP TRIGGER reject_event;").unwrap();
    store.apply(request(0, "recovered")).unwrap();
}

#[test]
fn invalid_patch_is_atomic_and_inverse_reverses_a_batch() {
    let (_directory, _path, mut store) = fixture();
    let mut update = request(0, "partial");
    update.mutations.push(Mutation::Remove {
        path: vec!["missing".into()],
    });
    assert_eq!(store.apply(update).unwrap_err().code(), "INVALID_MUTATION");
    assert_eq!(store.snapshot().unwrap().revision, 0);
    let mut update = request(0, "first");
    update.mutations.push(Mutation::Set {
        path: vec!["name".into()],
        value: json!("second"),
    });
    let event = store.apply(update).unwrap();
    let mut undo = request(1, "");
    undo.mutations = event.inverse;
    store.apply(undo).unwrap();
    assert_eq!(store.snapshot().unwrap().document.name, "");
}

#[test]
fn stale_revision_rejected_across_connections() {
    let (_directory, path, mut first) = fixture();
    let mut second = open(&path);
    first.apply(request(0, "winner")).unwrap();
    let error = second.apply(request(0, "stale")).unwrap_err();
    assert!(matches!(
        error,
        StoreError::RevisionConflict {
            base: 0,
            current: 1
        }
    ));
    assert_eq!(second.snapshot().unwrap().document.name, "winner");
    assert_eq!(second.events_since(0).unwrap().len(), 1);
}

#[test]
fn restore_full_snapshot_without_replaying_events() {
    let (_directory, path, mut store) = fixture();
    store.apply(request(0, "saved")).unwrap();
    let saved = store.snapshot_at(1).unwrap();
    store.apply(request(1, "later")).unwrap();
    let sql = Connection::open(&path).unwrap();
    // Old command meanings are not necessary for restoration.
    sql.execute(
        "UPDATE events SET mutations='opaque historical command' WHERE revision=1",
        [],
    )
    .unwrap();
    let event = store.restore_snapshot(2, Uuid::new_v4(), 1).unwrap();
    assert_eq!(event.revision, 3);
    assert_eq!(store.snapshot().unwrap().document, saved.document);
    drop(sql);
    store.close().unwrap();
    assert_eq!(open(&path).snapshot().unwrap().document, saved.document);
}

#[test]
fn failed_migration_is_nondestructive_including_ddl_and_internal_version() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("migration.kronello");
    let mut store = ProjectStore::open(&path, options(OpenMode::ForceSafe)).unwrap();
    store.apply(request(0, "original")).unwrap();
    let before = fs::read(&path).unwrap();
    let snapshot = store.snapshot().unwrap();
    let events = store.events_since(0).unwrap();
    let error = store.migrate_schema(2, |tx| {
        tx.execute_batch("ALTER TABLE project ADD COLUMN accidental TEXT; UPDATE project SET document='broken'; DELETE FROM events;")?;
        tx.pragma_update(None, "user_version", 2)?;
        Err(StoreError::InvalidMutation("injected migration failure".into()))
    }).unwrap_err();
    assert_eq!(error.code(), "MIGRATION_FAILED");
    assert_eq!(store.snapshot().unwrap(), snapshot);
    assert_eq!(store.events_since(0).unwrap(), events);
    assert_eq!(fs::read(&path).unwrap(), before);
    // A migration callback returning success must still pass document validation.
    assert_eq!(
        store
            .migrate_schema(2, |tx| {
                tx.execute("UPDATE project SET document='invalid JSON'", [])?;
                Ok(())
            })
            .unwrap_err()
            .code(),
        "MIGRATION_FAILED"
    );
    assert_eq!(store.snapshot().unwrap(), snapshot);
    store.close().unwrap();
    let sql = Connection::open(&path).unwrap();
    assert_eq!(
        sql.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .unwrap(),
        1
    );
    assert!(sql.prepare("SELECT accidental FROM project").is_err());
    drop(sql);
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn unsupported_versions_and_unknown_database_leave_original_unchanged() {
    let (_directory, path, store) = fixture();
    store.close().unwrap();
    let sql = Connection::open(&path).unwrap();
    sql.pragma_update(None, "user_version", 999).unwrap();
    drop(sql);
    let before = fs::read(&path).unwrap();
    assert_eq!(
        ProjectStore::open(&path, options(OpenMode::ForceNormal))
            .err()
            .unwrap()
            .code(),
        "UNSUPPORTED_SCHEMA_VERSION"
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    let sql = Connection::open(&path).unwrap();
    sql.pragma_update(None, "user_version", 0).unwrap();
    drop(sql);
    let before = fs::read(&path).unwrap();
    assert_eq!(
        ProjectStore::open(&path, options(OpenMode::ForceNormal))
            .err()
            .unwrap()
            .code(),
        "MIGRATION_FAILED"
    );
    assert_eq!(fs::read(&path).unwrap(), before);
}

fn project_with_data() -> Project {
    let curve = AnimationCurve::new(
        CurveId::new(),
        ValueType::Scalar,
        vec![Keyframe {
            time: Time::new(1, 3).unwrap(),
            value: ModelValue::Scalar(FiniteF64::new(0.12345678901234566).unwrap()),
            interpolation: CurveInterpolation::Linear,
        }],
    )
    .unwrap();
    let composition = Composition {
        id: CompositionId::new(),
        duration: ModelDuration::new(Time::new(7, 3).unwrap()).unwrap(),
        design_extent: DesignExtent::new(1920.0, 1080.0).unwrap(),
        edit_rate: FrameRate::new(30000, 1001).unwrap(),
        root_nodes: vec![],
        nodes: vec![],
        properties: vec![],
    };
    Project {
        compositions: vec![DocumentObject::Known(composition)],
        curves: vec![DocumentObject::Known(curve)],
        ..Project::default()
    }
}

#[test]
fn public_json_roundtrips_rationals_and_nested_opaque_content() {
    let (_directory, path, mut store) = fixture();
    let project = project_with_data();
    let mut input = serde_json::to_value(&project).unwrap();
    assert_eq!(
        input["curves"][0]["keys"][0]["time"],
        json!({"num":"1","den":"3"})
    );
    input["future_project"] = json!({"feature": [null,true,"1",1]});
    input["compositions"][0]["future_composition"] =
        json!({"reference":Uuid::new_v4(),"nested":[1,2,3]});
    input["curves"][0]["keys"][0]["interpolation"] =
        json!({"kind":"future_curve","value":{"meaning":99}});
    input["curves"][0]["future_curve"] = json!(0.12345678901234566);
    let event = store
        .import_json(0, Uuid::new_v4(), &input.to_string())
        .unwrap();
    assert!(!event.changed_keys.is_empty());
    assert_eq!(
        serde_json::from_str::<Value>(&store.export_json().unwrap()).unwrap(),
        input
    );
    assert_eq!(
        store.apply(request(1, "unsafe edit")).unwrap_err().code(),
        "UNSUPPORTED_FEATURE"
    );
    let hash = store.content_hash().unwrap();
    let exported = store.export_json().unwrap();
    store.close().unwrap();
    let mut reopened = open(&path);
    assert_eq!(reopened.content_hash().unwrap(), hash);
    assert_eq!(
        serde_json::from_str::<Value>(&reopened.export_json().unwrap()).unwrap(),
        input
    );
    reopened.import_json(1, Uuid::new_v4(), &exported).unwrap();
    assert_eq!(reopened.content_hash().unwrap(), hash);
    let mut changed = input.clone();
    changed["future_project"]["feature"][0] = json!("changed");
    reopened
        .import_json(2, Uuid::new_v4(), &changed.to_string())
        .unwrap();
    assert_ne!(reopened.content_hash().unwrap(), hash);
}

#[test]
fn unknown_semantic_version_is_preserved_but_not_editable() {
    let (_directory, _path, mut store) = fixture();
    let project = Project {
        semantic_version: 999,
        ..Project::default()
    };
    store
        .import_json(0, Uuid::new_v4(), &serde_json::to_string(&project).unwrap())
        .unwrap();
    assert_eq!(store.snapshot().unwrap().document, project);
    assert_eq!(
        store.apply(request(1, "unsafe")).unwrap_err().code(),
        "UNSUPPORTED_FEATURE"
    );
    let mut input = serde_json::to_value(project).unwrap();
    input["schema_version"] = json!(999);
    let before = store.snapshot().unwrap();
    assert_eq!(
        store
            .import_json(1, Uuid::new_v4(), &input.to_string())
            .unwrap_err()
            .code(),
        "UNSUPPORTED_SCHEMA_VERSION"
    );
    assert_eq!(store.snapshot().unwrap(), before);
}

#[test]
fn committed_schema_matches_rust_types() {
    let generated =
        serde_json::to_string_pretty(&kronello_model::project_json_schema()).unwrap() + "\n";
    assert_eq!(
        generated,
        include_str!("../../../schemas/project-v1.schema.json")
    );
    let schema: Value = serde_json::from_str(&generated).unwrap();
    assert_eq!(
        schema["$defs"]["Rational"]["properties"]["num"]["type"],
        "string"
    );
    assert_eq!(
        schema["$defs"]["Rational"]["properties"]["den"]["type"],
        "string"
    );
}

#[test]
fn compact_retains_boundary_snapshot_and_event_without_changing_document() {
    let (_directory, _path, mut store) = fixture();
    let mut update = request(0, "one");
    update.idempotency_key = Some("retained".into());
    let first_event = store.apply(update).unwrap();
    store.apply(request(1, "two")).unwrap();
    store.apply(request(2, "three")).unwrap();
    let boundary = store.snapshot_at(2).unwrap();
    let current = store.snapshot().unwrap();
    let previous_bytes = store.history_size().unwrap().bytes;
    store.compact(2).unwrap();
    assert_eq!(
        store
            .idempotency_record("retained")
            .unwrap()
            .unwrap()
            .result,
        first_event
    );
    assert_eq!(store.snapshot().unwrap(), current);
    assert_eq!(store.snapshot_at(2).unwrap(), boundary);
    assert_eq!(
        store
            .events_since(0)
            .unwrap()
            .iter()
            .map(|event| event.revision)
            .collect::<Vec<_>>(),
        vec![2, 3]
    );
    assert_eq!(
        store.snapshot_at(1).unwrap_err().code(),
        "SNAPSHOT_NOT_FOUND"
    );
    assert!(store.history_size().unwrap().bytes < previous_bytes);
    assert!(!store.history_size().unwrap().warning);
    let mut update = request(3, "duplicate");
    update.idempotency_key = Some("retained".into());
    assert_eq!(
        store.apply(update).unwrap_err().code(),
        "IDEMPOTENCY_KEY_EXISTS"
    );
    assert_eq!(store.compact(4).unwrap_err().code(), "SNAPSHOT_NOT_FOUND");
    assert_eq!(store.snapshot().unwrap(), current);
    store.restore_snapshot(3, Uuid::new_v4(), 2).unwrap();
    assert_eq!(store.snapshot().unwrap().document.name, "two");
}

#[test]
fn cache_is_outside_single_file_project_and_database_has_no_cache_tables() {
    let (directory, path, store) = fixture();
    let cache = render_cache_location(&path).unwrap();
    assert!(cache.is_absolute());
    assert!(!cache.starts_with(directory.path().canonicalize().unwrap()));
    let sql = Connection::open(&path).unwrap();
    let mut query = sql
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap();
    let tables = query
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        tables,
        vec!["events", "idempotency", "project", "snapshots"]
    );
    drop(query);
    drop(sql);
    store.close().unwrap();
    let files = fs::read_dir(directory.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    assert_eq!(files, vec!["test.kronello"]);
}

struct FixedDetector(DetectedLocation);
impl LocationDetector for FixedDetector {
    fn detect(&self, _: &Path) -> Result<DetectedLocation, StoreError> {
        Ok(self.0.clone())
    }
}
#[test]
fn location_detection_and_explicit_overrides_are_injectable() {
    let home = Path::new("/users/editor");
    for folder in [
        "Library/CloudStorage/Dropbox",
        "Library/Mobile Documents/com~apple~CloudDocs",
        "Dropbox",
        "OneDrive - Studio",
        "Google Drive",
        "GoogleDrive",
    ] {
        assert_eq!(
            detect_location(
                &home.join(folder).join("test.kronello"),
                Some(home),
                Some("apfs")
            ),
            DetectedLocation::SyncFolder
        );
    }
    for fs in ["smbfs", "cifs", "nfs", "nfs4", "afpfs", "webdav", "davfs"] {
        assert_eq!(
            detect_location(Path::new("/mnt/project.kronello"), None, Some(fs)),
            DetectedLocation::NetworkFileSystem
        );
    }
    assert_eq!(
        detect_location(
            &home.join("Dropbox-not-a-sync-folder/test.kronello"),
            Some(home),
            Some("apfs")
        ),
        DetectedLocation::Local
    );
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("safe.kronello");
    let store = ProjectStore::open_with_detector(
        &path,
        OpenOptions::default(),
        &FixedDetector(DetectedLocation::SyncFolder),
    )
    .unwrap();
    assert!(store.safe_mode());
    assert_eq!(store.journal_mode().unwrap(), "delete");
    store.close().unwrap();
    let store = ProjectStore::open_with_detector(
        &path,
        options(OpenMode::ForceNormal),
        &FixedDetector(DetectedLocation::NetworkFileSystem),
    )
    .unwrap();
    assert!(!store.safe_mode());
    assert_eq!(store.journal_mode().unwrap(), "wal");
    store.close().unwrap();
    let store = ProjectStore::open_with_detector(
        &path,
        options(OpenMode::ForceSafe),
        &FixedDetector(DetectedLocation::Local),
    )
    .unwrap();
    assert!(store.safe_mode());
    store.close().unwrap();
    assert!(!sidecar(&path, "-wal").exists());
    assert!(!sidecar(&path, "-shm").exists());
}

struct Actor {
    child: Child,
    ready: PathBuf,
    release: PathBuf,
    result: PathBuf,
}
impl Actor {
    fn spawn(directory: &Path, project: &Path, mode: &str) -> Self {
        let token = Uuid::new_v4().to_string();
        let ready = directory.join(format!("{token}.ready"));
        let release = directory.join(format!("{token}.release"));
        let result = directory.join(format!("{token}.result"));
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "subprocess_actor", "--nocapture"])
            .env("KRONELLO_CHILD_MODE", mode)
            .env("KRONELLO_CHILD_PROJECT", project)
            .env("KRONELLO_CHILD_READY", &ready)
            .env("KRONELLO_CHILD_RELEASE", &release)
            .env("KRONELLO_CHILD_RESULT", &result)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        Self {
            child,
            ready,
            release,
            result,
        }
    }
    fn wait_ready(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !self.ready.exists() {
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "child exited before ready"
            );
            assert!(Instant::now() < deadline, "child readiness timed out");
            thread::sleep(Duration::from_millis(10));
        }
    }
    fn finish(&mut self) -> String {
        fs::write(&self.release, b"go").unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "child failed: {status}");
                break;
            }
            assert!(Instant::now() < deadline, "child exit timed out");
            thread::sleep(Duration::from_millis(10));
        }
        fs::read_to_string(&self.result).unwrap()
    }
}
impl Drop for Actor {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[test]
fn subprocess_actor() {
    let Ok(mode) = std::env::var("KRONELLO_CHILD_MODE") else {
        return;
    };
    let project = PathBuf::from(std::env::var_os("KRONELLO_CHILD_PROJECT").unwrap());
    let ready = PathBuf::from(std::env::var_os("KRONELLO_CHILD_READY").unwrap());
    let release = PathBuf::from(std::env::var_os("KRONELLO_CHILD_RELEASE").unwrap());
    let result = PathBuf::from(std::env::var_os("KRONELLO_CHILD_RESULT").unwrap());
    if mode == "probe_normal" || mode == "probe_safe" {
        let mode = if mode == "probe_normal" {
            OpenMode::ForceNormal
        } else {
            OpenMode::ForceSafe
        };
        let outcome = ProjectStore::open(&project, options(mode))
            .map(|store| {
                store.close().unwrap();
                "OPENED".to_owned()
            })
            .unwrap_or_else(|error| error.code().to_owned());
        fs::write(result, outcome).unwrap();
        fs::write(ready, b"ready").unwrap();
        return;
    }
    let open_mode = if mode == "hold_safe" {
        OpenMode::ForceSafe
    } else {
        OpenMode::ForceNormal
    };
    let mut store = ProjectStore::open(&project, options(open_mode)).unwrap();
    let base_revision = store.snapshot().unwrap().revision;
    let mut crashing_connection = None;
    if mode == "crash" {
        assert_eq!(base_revision, 0);
        store.apply(request(0, "durable before crash")).unwrap();
        let connection = Connection::open(&project).unwrap();
        connection.execute_batch("BEGIN IMMEDIATE; UPDATE project SET revision=99,document=json_set(document,'$.name','uncommitted');").unwrap();
        crashing_connection = Some(connection);
    }
    fs::write(ready, b"ready").unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while !release.exists() {
        assert!(Instant::now() < deadline, "parent release timed out");
        thread::sleep(Duration::from_millis(10));
    }
    if mode == "writer" {
        let outcome = store
            .apply(request(base_revision, "child winner"))
            .map(|_| "COMMITTED".to_owned())
            .unwrap_or_else(|error| error.code().to_owned());
        fs::write(result, outcome).unwrap();
    } else {
        fs::write(result, "CLOSED").unwrap();
    }
    drop(crashing_connection);
    store.close().unwrap();
}

#[test]
fn separate_process_writers_serialize_and_reject_one_stale_base() {
    let (directory, path, store) = fixture();
    let mut first = Actor::spawn(directory.path(), &path, "writer");
    let mut second = Actor::spawn(directory.path(), &path, "writer");
    first.wait_ready();
    second.wait_ready();
    fs::write(&first.release, b"go").unwrap();
    fs::write(&second.release, b"go").unwrap();
    let outcomes = BTreeSet::from([first.finish(), second.finish()]);
    assert_eq!(
        outcomes,
        BTreeSet::from(["COMMITTED".to_owned(), "REVISION_CONFLICT".to_owned()])
    );
    assert_eq!(store.snapshot().unwrap().revision, 1);
    assert_eq!(store.events_since(0).unwrap().len(), 1);
}

#[test]
fn safe_mode_locks_out_other_processes_even_with_normal_override() {
    let (directory, path, store) = fixture();
    store.close().unwrap();
    let mut holder = Actor::spawn(directory.path(), &path, "hold_safe");
    holder.wait_ready();
    for mode in ["probe_safe", "probe_normal"] {
        let mut probe = Actor::spawn(directory.path(), &path, mode);
        probe.wait_ready();
        assert_eq!(probe.finish(), "PROJECT_LOCKED");
    }
    assert_eq!(holder.finish(), "CLOSED");
    let mut probe = Actor::spawn(directory.path(), &path, "probe_normal");
    probe.wait_ready();
    assert_eq!(probe.finish(), "OPENED");
}

#[test]
fn safe_override_cannot_exclude_an_existing_normal_process() {
    let (directory, path, store) = fixture();
    let mut probe = Actor::spawn(directory.path(), &path, "probe_safe");
    probe.wait_ready();
    assert_eq!(probe.finish(), "PROJECT_LOCKED");
    store.close().unwrap();
}

#[test]
fn last_process_close_removes_wal_and_shm() {
    let (directory, path, mut store) = fixture();
    store.apply(request(0, "committed")).unwrap();
    let mut holder = Actor::spawn(directory.path(), &path, "hold_normal");
    holder.wait_ready();
    store.close().unwrap();
    assert!(sidecar(&path, "-wal").exists());
    assert!(sidecar(&path, "-shm").exists());
    assert_eq!(holder.finish(), "CLOSED");
    assert!(!sidecar(&path, "-wal").exists());
    assert!(!sidecar(&path, "-shm").exists());
    assert_eq!(open(&path).snapshot().unwrap().document.name, "committed");
}

#[test]
fn killed_process_recovers_committed_wal_and_discards_inflight_write() {
    let (directory, path, store) = fixture();
    store.close().unwrap();
    let mut crashing = Actor::spawn(directory.path(), &path, "crash");
    crashing.wait_ready();
    assert!(sidecar(&path, "-wal").exists());
    crashing.child.kill().unwrap();
    assert!(!crashing.child.wait().unwrap().success());
    assert!(sidecar(&path, "-wal").exists());
    let mut recovered = open(&path);
    assert_eq!(recovered.snapshot().unwrap().revision, 1);
    assert_eq!(
        recovered.snapshot().unwrap().document.name,
        "durable before crash"
    );
    assert_eq!(recovered.events_since(0).unwrap().len(), 1);
    recovered.apply(request(1, "write after recovery")).unwrap();
    recovered.close().unwrap();
    assert!(!sidecar(&path, "-wal").exists());
    assert!(!sidecar(&path, "-shm").exists());
    assert_eq!(open(&path).snapshot().unwrap().revision, 2);
}

#[test]
fn history_warning_threshold_is_inclusive_and_never_prunes_automatically() {
    use kronello_store::{HISTORY_WARNING_BYTES, HistorySize};
    assert!(!HistorySize::from_bytes(HISTORY_WARNING_BYTES - 1).warning);
    assert!(HistorySize::from_bytes(HISTORY_WARNING_BYTES).warning);
    assert!(HistorySize::from_bytes(HISTORY_WARNING_BYTES + 1).warning);
    let (_directory, _path, mut store) = fixture();
    assert_eq!(store.history_size().unwrap().bytes, 0);
    store.apply(request(0, "é")).unwrap();
    let event = store.events_since(0).unwrap().remove(0);
    let base = store.snapshot_at(0).unwrap();
    let expected = serde_json::to_vec(&event.mutations).unwrap().len()
        + serde_json::to_vec(&event.inverse).unwrap().len()
        + serde_json::to_vec(&event.changed_keys).unwrap().len()
        + serde_json::to_vec(&base.document).unwrap().len();
    assert_eq!(store.history_size().unwrap().bytes, expected as u64);
    assert_eq!(store.events_since(0).unwrap().len(), 1);
}

#[test]
fn public_schema_validates_known_and_opaque_exports() {
    let schema = serde_json::to_value(kronello_model::project_json_schema()).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let mut document = serde_json::to_value(project_with_data()).unwrap();
    assert!(validator.is_valid(&document));
    document["curves"][0]["future"] = json!({"opaque":[false,null,"retained"]});
    assert!(validator.is_valid(&document));
    document["schema_version"] = json!(999);
    assert!(!validator.is_valid(&document));
    document["schema_version"] = json!(1);
    document["id"] = json!(42);
    assert!(!validator.is_valid(&document));
}

#[test]
fn system_detector_uses_actual_filesystem_and_mode_lock_covers_symlinks() {
    use kronello_store::SystemLocationDetector;
    let (directory, path, store) = fixture();
    assert_eq!(
        SystemLocationDetector.detect(&path).unwrap(),
        DetectedLocation::Local
    );
    store.close().unwrap();
    #[cfg(unix)]
    {
        let alias = directory.path().join("alias.kronello");
        std::os::unix::fs::symlink(&path, &alias).unwrap();
        let store = ProjectStore::open_with_detector(
            &alias,
            OpenOptions::default(),
            &FixedDetector(DetectedLocation::SyncFolder),
        )
        .unwrap();
        assert!(store.safe_mode());
        assert_eq!(
            ProjectStore::open(&path, options(OpenMode::ForceNormal))
                .err()
                .unwrap()
                .code(),
            "PROJECT_LOCKED"
        );
    }
}

#[test]
fn creation_is_serialized_when_separate_processes_open_a_new_project() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("new.kronello");
    let mut first = Actor::spawn(directory.path(), &path, "writer");
    let mut second = Actor::spawn(directory.path(), &path, "writer");
    first.wait_ready();
    second.wait_ready();
    fs::write(&first.release, b"go").unwrap();
    fs::write(&second.release, b"go").unwrap();
    assert_eq!(
        BTreeSet::from([first.finish(), second.finish()]),
        BTreeSet::from(["COMMITTED".to_owned(), "REVISION_CONFLICT".to_owned()])
    );
    let store = open(&path);
    assert_eq!(store.snapshot().unwrap().revision, 1);
}

#[test]
fn property_source_patch_and_generated_inverse_roundtrip_real_model_data() {
    use kronello_model::{DescriptorRef, Property, PropertySource, SchemaKey, SchemaRegistry};
    let (_directory, _path, mut store) = fixture();
    let registry = SchemaRegistry::with_builtin();
    let descriptor = registry
        .lookup(&SchemaKey::new("kronello.opacity").unwrap())
        .unwrap();
    let property_id = PropertyId::new();
    let original = Property::new(
        property_id,
        DescriptorRef::new(descriptor),
        PropertySource::Constant(ModelValue::Scalar(FiniteF64::new(0.5).unwrap())),
        vec![],
        &registry,
    )
    .unwrap();
    let mut project = project_with_data();
    let DocumentObject::Known(composition) = &mut project.compositions[0] else {
        unreachable!()
    };
    let composition_id = composition.id;
    composition.properties.push(original.clone());
    store
        .import_json(0, Uuid::new_v4(), &serde_json::to_string(&project).unwrap())
        .unwrap();
    let DocumentObject::Known(composition) = &mut project.compositions[0] else {
        unreachable!()
    };
    composition.properties[0]
        .set_source(
            PropertySource::Curve(match &project.curves[0] {
                DocumentObject::Known(curve) => curve.id(),
                _ => unreachable!(),
            }),
            &registry,
        )
        .unwrap();
    let mut update = request(1, "unused");
    update.mutations = vec![Mutation::Set {
        path: vec!["compositions".into()],
        value: serde_json::to_value(&project.compositions).unwrap(),
    }];
    update.changed_keys = BTreeSet::from([ChangedKey::Value {
        object_id: composition_id.as_uuid(),
        property_id,
    }]);
    let event = store.apply(update).unwrap();
    assert_eq!(store.snapshot().unwrap().document, project);
    let mut undo = request(2, "unused");
    undo.mutations = event.inverse;
    undo.changed_keys = event.changed_keys;
    store.apply(undo).unwrap();
    let snapshot = store.snapshot().unwrap();
    let DocumentObject::Known(composition) = &snapshot.document.compositions[0] else {
        unreachable!()
    };
    assert_eq!(composition.properties[0], original);
}

#[test]
fn opaque_json_retains_integer_precision_beyond_u64() {
    let (_directory, path, mut store) = fixture();
    let mut input = serde_json::to_value(project_with_data()).unwrap();
    let integer: Value = serde_json::from_str("18446744073709551616001").unwrap();
    input["future_large_integer"] = integer.clone();
    input["compositions"][0]["future_nested_integer"] = integer;
    store
        .import_json(0, Uuid::new_v4(), &input.to_string())
        .unwrap();
    let result: Value = serde_json::from_str(&store.export_json().unwrap()).unwrap();
    assert_eq!(result, input);
    assert!(result["compositions"][0]["future_nested_integer"].is_number());
    assert_eq!(
        result["future_large_integer"].to_string(),
        "18446744073709551616001"
    );
    assert_eq!(
        result["compositions"][0]["future_nested_integer"].to_string(),
        "18446744073709551616001"
    );
    let event = store.events_since(0).unwrap().remove(0);
    let Mutation::Set { value, .. } = &event.mutations[0] else {
        unreachable!()
    };
    assert_eq!(
        value["future_large_integer"].to_string(),
        "18446744073709551616001"
    );
    store.close().unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&open(&path).export_json().unwrap()).unwrap(),
        input
    );
}

#[test]
fn duplicate_public_envelope_fields_are_rejected_without_changes() {
    let (_directory, _path, mut store) = fixture();
    let before = store.snapshot().unwrap();
    let input = serde_json::to_string(&Project::default()).unwrap();
    let duplicate = input.replacen(
        "\"schema_version\":1",
        "\"schema_version\":1,\"schema_version\":1",
        1,
    );
    assert_ne!(input, duplicate);
    assert!(store.import_json(0, Uuid::new_v4(), &duplicate).is_err());
    assert_eq!(store.snapshot().unwrap(), before);
}
