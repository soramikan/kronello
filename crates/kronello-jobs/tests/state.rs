use kronello_jobs::*;
use serde_json::json;
use std::time::Duration;

fn submit(store: &JobStore) -> JobRecord {
    store
        .submit(
            b"fixed",
            Submission {
                engine_version: "test".into(),
                project_id: "test".into(),
                revision: "1".into(),
                snapshot_hash: "test".into(),
                output_profile: json!({}),
                destination: store.config().state_root.join("output"),
                total_frames: 3,
            },
        )
        .unwrap()
}
#[test]
fn transactional_slots_obey_fifo_and_configured_parallelism() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = JobConfig::at(temp.path());
    config.slots = 2;
    let store = JobStore::open(config).unwrap();
    let a = submit(&store);
    let b = submit(&store);
    let c = submit(&store);
    assert!(!store.claim(&c.id).unwrap());
    let sa = store.clone();
    let sb = store.clone();
    let aid = a.id.clone();
    let bid = b.id.clone();
    let ta = std::thread::spawn(move || sa.claim(&aid).unwrap());
    let tb = std::thread::spawn(move || sb.claim(&bid).unwrap());
    assert!(ta.join().unwrap() && tb.join().unwrap());
    assert!(!store.claim(&c.id).unwrap());
    store
        .finish_error(&a.id, &JobError::new("TEST_FAILURE", "test"))
        .unwrap();
    assert!(store.claim(&c.id).unwrap());
}
#[test]
fn stale_lease_cannot_publish_or_become_successful_again() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = JobConfig::at(temp.path());
    config.heartbeat_interval = Duration::from_millis(1);
    config.heartbeat_timeout = Duration::from_secs(1);
    let store = JobStore::open(config).unwrap();
    let r = submit(&store);
    assert!(store.claim(&r.id).unwrap());
    let db = rusqlite::Connection::open(temp.path().join("jobs.sqlite3")).unwrap();
    let mut expired = store.get(&r.id).unwrap();
    expired.heartbeat_at_ms = now_ms() - 2000;
    db.execute(
        "UPDATE jobs SET record=?1 WHERE id=?2",
        rusqlite::params![serde_json::to_string(&expired).unwrap(), r.id],
    )
    .unwrap();
    assert_eq!(store.get(&r.id).unwrap().status, JobStatus::Interrupted);
    let mut published = false;
    let error = store
        .publish(&r.id, json!({}), || {
            published = true;
            Ok(())
        })
        .unwrap_err();
    assert_eq!(error.code(), "JOB_INTERRUPTED");
    assert!(!published);
    store
        .finish_error(&r.id, &JobError::new("TEST_FAILURE", "test"))
        .unwrap();
    assert_eq!(store.get(&r.id).unwrap().status, JobStatus::Interrupted);
    assert!(store.directory(&r.id).unwrap().exists());
    // claim must commit stale-heartbeat recovery even when rejecting execution.
    let next = submit(&store);
    let mut expired = store.get(&next.id).unwrap();
    expired.heartbeat_at_ms = now_ms() - 2000;
    db.execute(
        "UPDATE jobs SET record=?1 WHERE id=?2",
        rusqlite::params![serde_json::to_string(&expired).unwrap(), next.id],
    )
    .unwrap();
    assert_eq!(store.claim(&next.id).unwrap_err().code(), "JOB_INTERRUPTED");
    store
        .finish_error(&next.id, &JobError::new("TEST_FAILURE", "test"))
        .unwrap();
    assert_eq!(store.get(&next.id).unwrap().status, JobStatus::Interrupted);
}
#[test]
fn publication_is_atomic_and_never_clobbers_existing_file_or_empty_directory() {
    let temp = tempfile::tempdir().unwrap();
    for directory in [false, true] {
        let staged = temp.path().join(if directory {
            "staged-dir"
        } else {
            "staged-file"
        });
        let final_path = temp
            .path()
            .join(if directory { "final-dir" } else { "final-file" });
        if directory {
            std::fs::create_dir(&staged).unwrap();
            std::fs::write(staged.join("data"), b"new").unwrap();
            std::fs::create_dir(&final_path).unwrap();
        } else {
            std::fs::write(&staged, b"new").unwrap();
            std::fs::write(&final_path, b"existing").unwrap();
        }
        assert_eq!(
            publish_path(&staged, &final_path).unwrap_err().code(),
            "OUTPUT_EXISTS"
        );
        assert!(staged.exists());
        if directory {
            assert_eq!(std::fs::read_dir(&final_path).unwrap().count(), 0);
            std::fs::remove_dir(&final_path).unwrap();
        } else {
            assert_eq!(std::fs::read(&final_path).unwrap(), b"existing");
            std::fs::remove_file(&final_path).unwrap();
        }
        publish_path(&staged, &final_path).unwrap();
        assert!(!staged.exists());
        assert_eq!(
            std::fs::read(if directory {
                final_path.join("data")
            } else {
                final_path
            })
            .unwrap(),
            b"new"
        );
    }
}
#[test]
fn invalid_configuration_ids_and_newer_database_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = JobConfig::at(temp.path());
    config.slots = 0;
    assert!(JobStore::open(config).is_err());
    let store = JobStore::open(JobConfig::at(temp.path())).unwrap();
    assert_eq!(
        store.get("../../outside").unwrap_err().code(),
        "INVALID_REQUEST"
    );
    let db = rusqlite::Connection::open(temp.path().join("jobs.sqlite3")).unwrap();
    db.execute_batch("PRAGMA user_version=999;").unwrap();
    assert!(JobStore::open(JobConfig::at(temp.path())).is_err());
}
