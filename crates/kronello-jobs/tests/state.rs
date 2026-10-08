use kronello_jobs::*;
use serde_json::json;
use std::time::Duration;

fn claim_retrying_contention(store: &JobStore, id: &str) -> Result<bool, JobError> {
    let start = std::time::Instant::now();
    loop {
        match store.claim(id) {
            Err(error)
                if error.is_retryable_contention() && start.elapsed() < Duration::from_secs(5) =>
            {
                std::thread::sleep(Duration::from_millis(1));
            }
            result => return result,
        }
    }
}

#[cfg(unix)]
#[path = "../src/test_support.rs"]
mod cleanup;

#[test]
#[cfg(unix)]
fn spawn_registers_owner_before_worker_initialization() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let executable = temp.path().join("delayed-worker.sh");
    std::fs::write(&executable, "#!/bin/sh\nexec /bin/sleep 60\n").unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let store = JobStore::open(JobConfig::at(temp.path())).unwrap();
    let record = submit(&store);
    let _cleanup = cleanup::WorkerCleanup::new(temp.path()).unwrap();
    store.spawn(&record.id, &executable).unwrap();
    _cleanup.capture_registered().unwrap();
    let owner = store.get(&record.id).unwrap();
    let pid =
        nix::unistd::Pid::from_raw(owner.worker_pid.expect("spawn must register child PID") as i32);
    let db = rusqlite::Connection::open(temp.path().join("jobs.sqlite3")).unwrap();
    let mut expired = owner;
    expired.heartbeat_at_ms = now_ms() - 60000;
    db.execute(
        "UPDATE jobs SET record=?1 WHERE id=?2",
        rusqlite::params![serde_json::to_string(&expired).unwrap(), record.id],
    )
    .unwrap();
    let observed = store.get(&record.id);
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGKILL).unwrap();
    assert_eq!(observed.unwrap().status, JobStatus::Queued);
}

#[test]
fn heartbeat_lock_wait_is_bounded_and_can_be_retried() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = JobConfig::at(temp.path());
    config.heartbeat_interval = Duration::from_millis(50);
    config.heartbeat_timeout = Duration::from_secs(3);
    let store = JobStore::open(config).unwrap();
    let record = submit(&store);
    assert!(claim_retrying_contention(&store, &record.id).unwrap());
    let original_heartbeat = store.get(&record.id).unwrap().heartbeat_at_ms;
    let db = rusqlite::Connection::open(temp.path().join("jobs.sqlite3")).unwrap();
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    let pulse_store = store.clone();
    let id = record.id.clone();
    let (send, receive) = std::sync::mpsc::channel();
    let pulse = std::thread::spawn(move || send.send(pulse_store.heartbeat(&id)).unwrap());
    let bounded = receive.recv_timeout(Duration::from_millis(500));
    // Release the lock before asserting so a failed test cannot strand a thread.
    db.execute_batch("ROLLBACK").unwrap();
    pulse.join().unwrap();
    let error = bounded
        .expect("heartbeat waited behind a writer for over 500 ms")
        .unwrap_err();
    // Concurrent tests may consume the process-local gate before this call
    // reaches the held SQLite writer lock. Both waits must stay bounded and
    // return retryable contention, with no assumption about which wins first.
    assert!(error.is_retryable_heartbeat(), "{error}");
    // Releasing this database's writer does not release other tests' gate
    // leases. Exercise the same bounded retry as the heartbeat worker.
    let retry_started = std::time::Instant::now();
    loop {
        match store.heartbeat(&record.id) {
            Err(error)
                if error.is_retryable_heartbeat()
                    && retry_started.elapsed() < Duration::from_secs(5) =>
            {
                std::thread::sleep(Duration::from_millis(1));
            }
            result => {
                result.unwrap();
                break;
            }
        }
    }
    let observed = store.get(&record.id).unwrap();
    assert_eq!(observed.status, JobStatus::Running);
    assert!(observed.heartbeat_at_ms > original_heartbeat);
}

#[test]
fn expired_heartbeat_of_live_owner_preserves_queued_and_running_leases() {
    let temp = tempfile::tempdir().unwrap();
    let store = JobStore::open(JobConfig::at(temp.path())).unwrap();
    let running = submit(&store);
    assert!(claim_retrying_contention(&store, &running.id).unwrap());
    let queued = submit(&store);
    assert!(!claim_retrying_contention(&store, &queued.id).unwrap());
    let db = rusqlite::Connection::open(temp.path().join("jobs.sqlite3")).unwrap();
    for record in [&running, &queued] {
        let mut expired = store.get(&record.id).unwrap();
        expired.heartbeat_at_ms = now_ms() - 60000;
        db.execute(
            "UPDATE jobs SET record=?1 WHERE id=?2",
            rusqlite::params![serde_json::to_string(&expired).unwrap(), record.id],
        )
        .unwrap();
    }
    assert_eq!(store.get(&running.id).unwrap().status, JobStatus::Running);
    assert_eq!(store.get(&queued.id).unwrap().status, JobStatus::Queued);
    store.prune().unwrap();
    assert!(!claim_retrying_contention(&store, &queued.id).unwrap());
    store.heartbeat(&running.id).unwrap();
}

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
    assert!(!claim_retrying_contention(&store, &c.id).unwrap());
    let sa = store.clone();
    let sb = store.clone();
    let aid = a.id.clone();
    let bid = b.id.clone();
    // claim() deliberately returns bounded, retryable contention. Exercise the
    // worker's retry path while racing two slots, rather than treating a busy
    // process-local connection gate as a terminal scheduler failure.
    let ta = std::thread::spawn(move || sa.wait_for_slot(&aid));
    let tb = std::thread::spawn(move || sb.wait_for_slot(&bid));
    ta.join().unwrap().unwrap();
    tb.join().unwrap().unwrap();
    assert_eq!(store.get(&a.id).unwrap().status, JobStatus::Running);
    assert_eq!(store.get(&b.id).unwrap().status, JobStatus::Running);
    assert!(!claim_retrying_contention(&store, &c.id).unwrap());
    store
        .finish_error(&a.id, &JobError::new("TEST_FAILURE", "test"))
        .unwrap();
    assert!(claim_retrying_contention(&store, &c.id).unwrap());
}

#[test]
fn owned_queued_polling_does_not_write_but_heartbeat_and_promotion_do() {
    let temp = tempfile::tempdir().unwrap();
    let store = JobStore::open(JobConfig::at(temp.path())).unwrap();
    let running = submit(&store);
    assert!(claim_retrying_contention(&store, &running.id).unwrap());
    let queued = submit(&store);
    assert!(!claim_retrying_contention(&store, &queued.id).unwrap());
    let db = rusqlite::Connection::open(temp.path().join("jobs.sqlite3")).unwrap();
    db.execute_batch(
        "CREATE TABLE observed_updates(id TEXT);
        CREATE TRIGGER count_job_updates AFTER UPDATE ON jobs
        BEGIN INSERT INTO observed_updates VALUES(NEW.id); END;",
    )
    .unwrap();
    let writes = || -> i64 {
        db.query_row(
            "SELECT count(*) FROM observed_updates WHERE id=?1",
            [&queued.id],
            |row| row.get(0),
        )
        .unwrap()
    };
    for _ in 0..20 {
        assert!(!claim_retrying_contention(&store, &queued.id).unwrap());
    }
    assert_eq!(
        writes(),
        0,
        "occupied-slot polls must not flush redundant updates"
    );
    assert_eq!(
        store.get(&queued.id).unwrap().worker_pid,
        Some(std::process::id())
    );
    store.heartbeat(&queued.id).unwrap();
    assert_eq!(writes(), 1, "queued liveness still has an explicit writer");
    store
        .finish_error(&running.id, &JobError::new("TEST_FAILURE", "done"))
        .unwrap();
    assert!(claim_retrying_contention(&store, &queued.id).unwrap());
    assert_eq!(writes(), 2);
    assert_eq!(store.get(&queued.id).unwrap().status, JobStatus::Running);
}
#[test]
fn stale_lease_cannot_publish_or_become_successful_again() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = JobConfig::at(temp.path());
    config.heartbeat_interval = Duration::from_millis(1);
    config.heartbeat_timeout = Duration::from_secs(1);
    let store = JobStore::open(config).unwrap();
    let r = submit(&store);
    assert!(claim_retrying_contention(&store, &r.id).unwrap());
    let db = rusqlite::Connection::open(temp.path().join("jobs.sqlite3")).unwrap();
    let mut expired = store.get(&r.id).unwrap();
    expired.heartbeat_at_ms = now_ms() - 2000;
    expired.worker_pid = None;
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
    assert_eq!(
        claim_retrying_contention(&store, &next.id)
            .unwrap_err()
            .code(),
        "JOB_INTERRUPTED"
    );
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
/// FLOW-004 (ADR-0135): `stop.request` is a filesystem signal polled by the
/// owning worker — not a state transition — and `publish_attempt_frames`
/// commits the frame count an open-ended session discovers only at stop.
#[test]
fn session_stop_marker_and_publish_attempt_frames_commit_final_count() {
    let temp = tempfile::tempdir().unwrap();
    let store = JobStore::open(JobConfig::at(temp.path())).unwrap();
    // Open-ended capture session: total_frames is a placeholder at submit.
    let record = store
        .submit(
            b"fixed",
            Submission {
                engine_version: "test".into(),
                project_id: "test".into(),
                revision: "1".into(),
                snapshot_hash: "test".into(),
                output_profile: json!({"capture_asset_id": "asset"}),
                destination: temp.path().join("capture.mov"),
                total_frames: 0,
            },
        )
        .unwrap();
    assert!(!store.stop_requested(&record.id).unwrap());
    let returned = store.request_stop(&record.id).unwrap();
    assert_eq!(returned.id, record.id);
    assert!(store.stop_requested(&record.id).unwrap());
    // The marker is not a state change and a repeated request is idempotent.
    assert_eq!(store.get(&record.id).unwrap().status, JobStatus::Queued);
    store.request_stop(&record.id).unwrap();
    assert!(claim_retrying_contention(&store, &record.id).unwrap());
    let staging = store.staging(&record).unwrap();
    let staged = staging.output();
    std::fs::write(&staged, b"finalized").unwrap();
    store
        .publish_attempt_frames(
            record.id.as_str(),
            0,
            7,
            json!({"report": {"frames": 7}}),
            || publish_path(&staged, &record.destination),
        )
        .unwrap();
    let done = store.get(&record.id).unwrap();
    assert_eq!(done.status, JobStatus::Succeeded);
    assert_eq!(done.total_frames, 7);
    assert_eq!(done.completed_frames, 7);
    assert!(record.destination.is_file());
    // Stopping a terminal session is a no-op, not an error.
    assert_eq!(
        store.request_stop(&record.id).unwrap().status,
        JobStatus::Succeeded
    );
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
/// FLOW-003 (ADR-0130): keyed submissions replay identical payloads, reject
/// key reuse with different content, and commit key+job atomically.
#[test]
fn keyed_submission_replays_rejects_reuse_and_scopes_projects() {
    let temp = tempfile::tempdir().unwrap();
    let store = JobStore::open(JobConfig::at(temp.path())).unwrap();
    let submission = || Submission {
        engine_version: "test".into(),
        project_id: "project-a".into(),
        revision: "1".into(),
        snapshot_hash: "snap".into(),
        output_profile: json!({"kind": "render"}),
        destination: temp.path().join("out-a"),
        total_frames: 3,
    };
    let KeyedSubmission::Submitted(first) = store
        .submit_keyed(b"fixed-a", submission(), "batch-1", "payload-a")
        .unwrap()
    else {
        panic!("first keyed submit must create a job")
    };
    // Identical payload replays the same job without a second input directory.
    let KeyedSubmission::Replayed(again) = store
        .submit_keyed(b"ignored", submission(), "batch-1", "payload-a")
        .unwrap()
    else {
        panic!("identical payload must replay")
    };
    assert_eq!(again.id, first.id);
    assert_eq!(
        std::fs::read_dir(temp.path().join("jobs")).unwrap().count(),
        1
    );
    assert_eq!(
        store
            .replay("project-a", "batch-1", "payload-a")
            .unwrap()
            .unwrap()
            .id,
        first.id
    );
    // A different payload under the same key is a typed conflict everywhere.
    for error in [
        store.replay("project-a", "batch-1", "other").unwrap_err(),
        store
            .submit_keyed(b"fixed-b", submission(), "batch-1", "other")
            .unwrap_err(),
    ] {
        assert_eq!(error.code(), "IDEMPOTENCY_KEY_REUSED");
    }
    // Keys are scoped to the submitting project.
    let mut other = submission();
    other.project_id = "project-b".into();
    assert!(matches!(
        store
            .submit_keyed(b"fixed-b", other, "batch-1", "other")
            .unwrap(),
        KeyedSubmission::Submitted(_)
    ));
    // Length limits: 1..256 UTF-8 bytes.
    assert_eq!(
        store
            .submit_keyed(b"x", submission(), "", "p")
            .unwrap_err()
            .code(),
        "INVALID_REQUEST"
    );
    assert_eq!(
        store
            .submit_keyed(b"x", submission(), &"k".repeat(257), "p")
            .unwrap_err()
            .code(),
        "INVALID_REQUEST"
    );
    assert!(matches!(
        store
            .submit_keyed(b"x", submission(), &"k".repeat(256), "p")
            .unwrap(),
        KeyedSubmission::Submitted(_)
    ));
}
/// A key whose job row disappeared is dropped and resubmitted cleanly.
#[test]
fn keyed_submission_cleans_up_dangling_keys() {
    let temp = tempfile::tempdir().unwrap();
    let store = JobStore::open(JobConfig::at(temp.path())).unwrap();
    let submission = || Submission {
        engine_version: "test".into(),
        project_id: "p".into(),
        revision: "1".into(),
        snapshot_hash: "s".into(),
        output_profile: json!({}),
        destination: temp.path().join("out"),
        total_frames: 1,
    };
    let KeyedSubmission::Submitted(first) =
        store.submit_keyed(b"in", submission(), "k", "v").unwrap()
    else {
        panic!()
    };
    let db = rusqlite::Connection::open(temp.path().join("jobs.sqlite3")).unwrap();
    db.execute("DELETE FROM jobs WHERE id=?1", [&first.id])
        .unwrap();
    drop(db);
    assert!(
        store.replay("p", "k", "v").unwrap().is_none(),
        "dangling key reads as absent"
    );
    let KeyedSubmission::Submitted(second) =
        store.submit_keyed(b"in", submission(), "k", "v").unwrap()
    else {
        panic!("dangling key must be replaced by a fresh job")
    };
    assert_ne!(second.id, first.id);
}
/// Schema version 1 databases gain job_keys through the version-2 migration.
#[test]
fn version_one_database_migrates_to_keyed_schema() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("jobs")).unwrap();
    std::fs::create_dir_all(temp.path().join("job-results")).unwrap();
    let db = rusqlite::Connection::open(temp.path().join("jobs.sqlite3")).unwrap();
    db.execute_batch(
        "CREATE TABLE jobs(seq INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT NOT NULL UNIQUE, record TEXT NOT NULL); PRAGMA user_version=1;",
    )
    .unwrap();
    drop(db);
    let store = JobStore::open(JobConfig::at(temp.path())).unwrap();
    assert!(matches!(
        store.submit_keyed(
            b"in",
            Submission {
                engine_version: "t".into(),
                project_id: "p".into(),
                revision: "1".into(),
                snapshot_hash: "s".into(),
                output_profile: json!({}),
                destination: temp.path().join("o"),
                total_frames: 1,
            },
            "k",
            "v",
        ),
        Ok(KeyedSubmission::Submitted(_))
    ));
}
