#![cfg(feature = "test-worker")]
use kronello_jobs::{JobConfig, JobRecord, JobStatus, JobStore, test_support::WorkerCleanup};
use serde_json::json;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

struct Fixture {
    cleanup: WorkerCleanup,
    temp: tempfile::TempDir,
    store: JobStore,
}
impl Fixture {
    fn new() -> Self {
        Self::with_slots(1)
    }
    fn with_slots(slots: usize) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("state");
        let cleanup = WorkerCleanup::new(&root).unwrap();
        let mut config = JobConfig::at(root);
        config.heartbeat_interval = Duration::from_millis(50);
        config.heartbeat_timeout = Duration::from_secs(1);
        config.retention = Duration::ZERO;
        config.slots = slots;
        let store = JobStore::open(config).unwrap();
        Self {
            cleanup,
            temp,
            store,
        }
    }
    fn submit(&self, name: &str, directory: bool) -> JobRecord {
        self.submit_fenced(name, directory, false)
    }
    fn submit_fenced(&self, name: &str, directory: bool, fenced: bool) -> JobRecord {
        self.submit_to(name, directory, fenced, &self.temp.path().join(name))
    }
    fn submit_to(
        &self,
        name: &str,
        directory: bool,
        fenced: bool,
        destination: &Path,
    ) -> JobRecord {
        self.submit_using_parent(name, directory, fenced, destination, "parent")
    }
    fn submit_using_parent(
        &self,
        name: &str,
        directory: bool,
        fenced: bool,
        destination: &Path,
        parent_mode: &str,
    ) -> JobRecord {
        let input = self.temp.path().join(format!("{name}.json"));
        let publication_gate = fenced.then(|| self.temp.path().join(format!("{name}.publish")));
        std::fs::write(
            &input,
            serde_json::to_vec(&json!({"gate":self.gate(name),"directory":directory,"publication_gate":publication_gate})).unwrap(),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_kronello-job-test-worker"))
            .arg(parent_mode)
            .arg(input)
            .arg(destination)
            .env("KRONELLO_STATE_ROOT", &self.store.config().state_root)
            .env("KRONELLO_JOB_HEARTBEAT_MS", "50")
            .env("KRONELLO_JOB_TIMEOUT_MS", "1000")
            .env("KRONELLO_JOB_SLOTS", self.store.config().slots.to_string())
            .env("KRONELLO_JOB_RETENTION_SECONDS", "0")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        self.cleanup.capture_registered().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        // wait_with_output returned: the separate submitter exited, its worker
        // owns no stdout/stderr pipe that could keep this call waiting.
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn gate(&self, name: &str) -> std::path::PathBuf {
        self.temp.path().join(format!("{name}.gate"))
    }
    fn release(&self, name: &str) {
        std::fs::write(self.gate(name), b"go").unwrap();
    }
    fn wait(&self, id: &str, status: JobStatus) -> JobRecord {
        let start = Instant::now();
        loop {
            let r = self.store.get(id).unwrap();
            if r.status == status {
                return r;
            }
            assert!(r.status.active(), "unexpected {r:?}");
            assert!(start.elapsed() < Duration::from_secs(30), "timed out {r:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn reaped(&self, pid: u32) {
        self.cleanup.reap().unwrap();
        assert!(
            !kronello_platform::process_is_alive(pid),
            "orphan worker {pid}"
        );
    }
}

#[test]
fn parent_exit_fixed_input_fifo_heartbeats_forced_kill_and_prune() {
    let f = Fixture::new();
    let a = f.submit("first", false);
    let running = f.wait(&a.id, JobStatus::Running);
    let b = f.submit("second", true);
    let queued = f.store.get(&b.id).unwrap();
    assert_eq!(queued.status, JobStatus::Queued);
    std::thread::sleep(Duration::from_millis(250));
    assert!(f.store.get(&a.id).unwrap().heartbeat_at_ms > running.heartbeat_at_ms);
    assert!(f.store.get(&b.id).unwrap().heartbeat_at_ms > queued.heartbeat_at_ms);
    // Mutate the source request, never the saved job input.
    std::fs::write(f.temp.path().join("second.json"), b"changed").unwrap();
    f.release("first");
    f.wait(&a.id, JobStatus::Succeeded);
    f.wait(&b.id, JobStatus::Running);
    f.release("second");
    f.wait(&b.id, JobStatus::Succeeded);
    assert_eq!(std::fs::read(&a.destination).unwrap(), b"fixed job payload");
    assert_eq!(
        std::fs::read(b.destination.join("fixed")).unwrap(),
        b"fixed job payload"
    );
    // Reap before pruning Windows open log handles.
    f.cleanup.reap().unwrap();
    let interrupted = f.submit("killed", false);
    let r = f.wait(&interrupted.id, JobStatus::Running);
    kronello_platform::ProcessGuard::capture(r.worker_pid.unwrap())
        .unwrap()
        .terminate_and_wait()
        .unwrap();
    let next = f.submit("next", false);
    f.wait(&interrupted.id, JobStatus::Interrupted);
    f.wait(&next.id, JobStatus::Running);
    f.release("next");
    f.wait(&next.id, JobStatus::Succeeded);
    f.cleanup.reap().unwrap();
    std::thread::sleep(Duration::from_millis(5));
    f.store.prune().unwrap();
    for id in [&a.id, &b.id, &next.id] {
        assert!(f.store.get(id).unwrap().directory_pruned);
    }
    assert!(!f.store.get(&interrupted.id).unwrap().directory_pruned);
    assert!(f.store.directory(&interrupted.id).unwrap().exists());
    f.reaped(r.worker_pid.unwrap());
}

#[test]
fn processes_reject_existing_file_empty_directory_cancel_and_lost_lease() {
    for directory in [false, true] {
        let f = Fixture::new();
        let a = f.submit("existing", directory);
        f.wait(&a.id, JobStatus::Running);
        if directory {
            std::fs::create_dir(&a.destination).unwrap();
        } else {
            std::fs::write(&a.destination, b"existing").unwrap();
        }
        f.release("existing");
        assert_eq!(
            f.wait(&a.id, JobStatus::Failed).error.unwrap().code,
            "OUTPUT_EXISTS"
        );
        if directory {
            assert_eq!(std::fs::read_dir(&a.destination).unwrap().count(), 0);
        } else {
            assert_eq!(std::fs::read(&a.destination).unwrap(), b"existing");
        }
    }
    for lost_lease in [false, true] {
        let f = Fixture::new();
        let a = f.submit_fenced("fenced", false, true);
        f.wait(&a.id, JobStatus::Running);
        f.release("fenced");
        let ready = a.destination.with_extension(format!("{}.ready", a.id));
        let start = Instant::now();
        while !ready.exists() {
            assert!(start.elapsed() < Duration::from_secs(30));
            std::thread::sleep(Duration::from_millis(20));
        }
        if lost_lease {
            let mut db =
                rusqlite::Connection::open(f.store.config().state_root.join("jobs.sqlite3"))
                    .unwrap();
            let tx = db
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .unwrap();
            let text: String = tx
                .query_row("SELECT record FROM jobs WHERE id=?1", [&a.id], |row| {
                    row.get(0)
                })
                .unwrap();
            let mut r: JobRecord = serde_json::from_str(&text).unwrap();
            r.status = JobStatus::Interrupted;
            tx.execute(
                "UPDATE jobs SET record=?1 WHERE id=?2",
                rusqlite::params![serde_json::to_string(&r).unwrap(), a.id],
            )
            .unwrap();
            tx.commit().unwrap();
        } else {
            f.store.cancel(&a.id).unwrap();
        }
        std::fs::write(f.temp.path().join("fenced.publish"), b"publish").unwrap();
        f.wait(
            &a.id,
            if lost_lease {
                JobStatus::Interrupted
            } else {
                JobStatus::Canceled
            },
        );
        let log = f.store.directory(&a.id).unwrap().join("worker.log");
        let expected = if lost_lease {
            "code=JOB_INTERRUPTED"
        } else {
            "code=JOB_CANCELED"
        };
        let start = Instant::now();
        while !std::fs::read_to_string(&log).unwrap().contains(expected) {
            assert!(start.elapsed() < Duration::from_secs(30));
            std::thread::sleep(Duration::from_millis(20));
        }
        f.cleanup.reap().unwrap();
        assert!(!a.destination.exists());
    }
}

#[test]
fn panic_and_timeout_cleanup_leave_no_workers() {
    for timeout in [false, true] {
        let f = Fixture::new();
        let a = f.submit("never", false);
        let r = f.wait(&a.id, JobStatus::Running);
        let pid = r.worker_pid.unwrap();
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _owned = f;
            if timeout {
                assert!(
                    Instant::now().elapsed() > Duration::from_secs(1),
                    "intentional timeout"
                );
            } else {
                panic!("intentional assertion failure")
            }
        }));
        assert!(panic.is_err());
        assert!(
            !kronello_platform::process_is_alive(pid),
            "orphan worker after unwind: {pid}"
        );
    }
}

#[test]
fn same_volume_publication_has_one_winner_with_two_real_processes() {
    let f = Fixture::with_slots(2);
    let a = f.submit_fenced("winner", false, true);
    f.wait(&a.id, JobStatus::Running);
    let b = f.submit_to("loser", false, true, &a.destination);
    f.wait(&b.id, JobStatus::Running);
    f.release("winner");
    f.release("loser");
    // Both independent processes have prepared staging before either is
    // permitted to call the production transactional publication fence.
    let start = Instant::now();
    for r in [&a, &b] {
        while !r
            .destination
            .with_extension(format!("{}.ready", r.id))
            .exists()
        {
            assert!(start.elapsed() < Duration::from_secs(30));
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    std::fs::write(f.temp.path().join("winner.publish"), b"publish").unwrap();
    std::fs::write(f.temp.path().join("loser.publish"), b"publish").unwrap();
    let records = loop {
        let records = [f.store.get(&a.id).unwrap(), f.store.get(&b.id).unwrap()];
        if records.iter().all(|r| !r.status.active()) {
            break records;
        }
        assert!(start.elapsed() < Duration::from_secs(30));
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(
        records
            .iter()
            .filter(|r| r.status == JobStatus::Succeeded)
            .count(),
        1
    );
    let failed = records
        .iter()
        .find(|r| r.status == JobStatus::Failed)
        .unwrap();
    assert_eq!(failed.error.as_ref().unwrap().code, "OUTPUT_EXISTS");
    assert_eq!(std::fs::read(&a.destination).unwrap(), b"fixed job payload");
}

#[test]
fn concurrent_connection_lifecycle_and_worker_writes_make_progress() {
    let f = Fixture::new();
    let log = f.temp.path().join("stress.log");
    struct ChildCleanup(std::process::Child);
    impl Drop for ChildCleanup {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let child = Command::new(env!("CARGO_BIN_EXE_kronello-job-test-worker"))
        .arg("stress")
        .env("KRONELLO_STATE_ROOT", &f.store.config().state_root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    let mut child = ChildCleanup(child);
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "connection stress deadlocked"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    let text = std::fs::read_to_string(log).unwrap();
    assert!(status.success(), "{text}");
    assert!(text.contains("writes=2000"), "{text}");
}

#[test]
fn heartbeat_deadline_exits_worker_and_recovers_interrupted() {
    let f = Fixture::new();
    let r = f.submit("stalled", false);
    let active = f.wait(&r.id, JobStatus::Running);
    let guard = kronello_platform::ProcessGuard::capture(active.worker_pid.unwrap()).unwrap();
    let db = rusqlite::Connection::open(f.store.config().state_root.join("jobs.sqlite3")).unwrap();
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    // More than 20 unsuccessful 50-ms intervals: the watchdog sends no DB call.
    let exited = guard.wait_for_exit(Duration::from_secs(5));
    db.execute_batch("ROLLBACK").unwrap();
    exited.unwrap();
    f.wait(&r.id, JobStatus::Interrupted);
    let text =
        std::fs::read_to_string(f.store.directory(&r.id).unwrap().join("worker.log")).unwrap();
    assert!(
        text.contains("worker heartbeat deadline exceeded"),
        "{text}"
    );
    assert!(!r.destination.exists());
}

#[test]
fn queued_kill_cancel_and_two_slots_preserve_fifo() {
    let f = Fixture::with_slots(2);
    let a = f.submit("first", false);
    f.wait(&a.id, JobStatus::Running);
    let b = f.submit("second", false);
    f.wait(&b.id, JobStatus::Running);
    let killed = f.submit("queued-kill", false);
    let canceled = f.submit("queued-cancel", false);
    let next = f.submit("next", false);
    assert_eq!(f.store.get(&next.id).unwrap().status, JobStatus::Queued);
    kronello_platform::ProcessGuard::capture(f.store.get(&killed.id).unwrap().worker_pid.unwrap())
        .unwrap()
        .terminate_and_wait()
        .unwrap();
    f.store.cancel(&canceled.id).unwrap();
    f.wait(&canceled.id, JobStatus::Canceled);
    f.wait(&killed.id, JobStatus::Interrupted);
    f.release("second");
    f.wait(&b.id, JobStatus::Succeeded);
    f.wait(&next.id, JobStatus::Running);
    // First keeps the other slot occupied while the next FIFO job starts.
    assert_eq!(f.store.get(&a.id).unwrap().status, JobStatus::Running);
    f.release("first");
    f.release("next");
    f.wait(&a.id, JobStatus::Succeeded);
    f.wait(&next.id, JobStatus::Succeeded);
    assert!(!killed.destination.exists());
    assert!(!canceled.destination.exists());
}

#[test]
#[ignore = "requires KRONELLO_JOB_SECOND_VOLUME on a distinct volume"]
fn cross_volume_is_typed_when_a_second_volume_is_supplied() {
    let root = std::env::var_os("KRONELLO_JOB_SECOND_VOLUME").expect("second volume required");
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    std::fs::write(&source, b"fixed").unwrap();
    let target = Path::new(&root).join(format!("kronello-{}.out", uuid::Uuid::new_v4()));
    assert_eq!(
        kronello_jobs::publish_path(&source, &target)
            .unwrap_err()
            .code(),
        "OUTPUT_CROSS_VOLUME"
    );
    assert!(source.exists());
    assert!(!target.exists());
}

#[test]
#[cfg(windows)]
fn breakaway_denied_worker_survives_parent_exit_in_parent_job() {
    let f = Fixture::new();
    let record = f.submit_using_parent(
        "limited",
        false,
        false,
        &f.temp.path().join("limited"),
        "restricted-parent",
    );
    // submit_using_parent has already waited for the restricted parent to exit.
    f.wait(&record.id, JobStatus::Running);
    f.release("limited");
    f.wait(&record.id, JobStatus::Succeeded);
    f.cleanup.reap().unwrap();
    let log =
        std::fs::read_to_string(f.store.directory(&record.id).unwrap().join("worker.log")).unwrap();
    assert!(
        log.contains("worker launch") && log.contains("detach_mode: \"in_parent_job\""),
        "{log}"
    );
    assert!(
        log.contains("worker detach_mode: \"in_parent_job\""),
        "{log}"
    );
    assert_eq!(
        std::fs::read(record.destination).unwrap(),
        b"fixed job payload"
    );
}

#[test]
fn unrelated_spawn_error_remains_typed_and_has_no_worker() {
    let f = Fixture::new();
    let record = f
        .store
        .submit(
            b"fixed",
            kronello_jobs::Submission {
                engine_version: "test".into(),
                project_id: "fixed".into(),
                revision: "1".into(),
                snapshot_hash: "fixed".into(),
                output_profile: json!({}),
                destination: f.temp.path().join("never"),
                total_frames: 1,
            },
        )
        .unwrap();
    let error = f
        .store
        .spawn(&record.id, &f.temp.path().join("missing-worker.exe"))
        .unwrap_err();
    assert_eq!(error.code(), "WORKER_DETACH_ERROR");
    assert!(f.store.get(&record.id).unwrap().worker_pid.is_none());
    assert!(!record.destination.exists());
}
