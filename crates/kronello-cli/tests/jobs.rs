use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use kronello_jobs::{JobConfig, JobRecord, JobStatus, JobStore, Submission};
use kronello_service::{BackendSelection, Service};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

struct Fixture {
    cleanup: kronello_jobs::test_support::WorkerCleanup,
    temp: tempfile::TempDir,
    project: PathBuf,
    state: PathBuf,
    document: Value,
    heartbeat_timeout: Duration,
    wait_timeout: Duration,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("source.kronello");
        let state = temp.path().join("state");
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
        document.as_object_mut().unwrap().remove("texts");
        let fixture = Self {
            cleanup: kronello_jobs::test_support::WorkerCleanup::new(&state).unwrap(),
            temp,
            project,
            state,
            document,
            heartbeat_timeout: Duration::from_secs(30),
            wait_timeout: Duration::from_secs(60),
        };
        fixture.service(json!({"operation":"project.create","project":fixture.project,"document":fixture.document}));
        fixture
    }
    fn service(&self, request: Value) -> Value {
        let response = Service::new(BackendSelection::CpuReference)
            .with_job_config(JobConfig::at(&self.state))
            .execute_json(&request.to_string());
        let response = serde_json::to_value(response).unwrap();
        assert_eq!(response["status"], "success", "{response}");
        response["result"]["value"].clone()
    }
    fn command(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_kronello"));
        c.args(["--backend", "cpu-reference"])
            .env("KRONELLO_STATE_ROOT", &self.state)
            .env("KRONELLO_JOB_HEARTBEAT_MS", "50")
            .env(
                "KRONELLO_JOB_TIMEOUT_MS",
                self.heartbeat_timeout.as_millis().to_string(),
            )
            .env_remove("KRONELLO_JOB_SLOTS")
            .env_remove("KRONELLO_JOB_RETENTION_SECONDS")
            .env_remove("KRONELLO_TEST_JOB_GATE")
            .env_remove("KRONELLO_TEST_JOB_CORRUPT_OUTPUT")
            .env_remove("KRONELLO_TEST_ADAPTER_UNAVAILABLE");
        c
    }
    fn cli(&self, request: Value, gate: Option<&Path>, corrupt: bool) -> Value {
        let mut c = self.command();
        if let Some(gate) = gate {
            c.env("KRONELLO_TEST_JOB_GATE", gate);
        }
        if corrupt {
            c.env("KRONELLO_TEST_JOB_CORRUPT_OUTPUT", "1");
        }
        let mut child = c
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(request.to_string().as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        self.cleanup.capture_registered().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        let _: kronello_service::Response = serde_json::from_slice(&output.stdout).unwrap();
        jsonschema::validator_for(&kronello_service::api_json_schema())
            .unwrap()
            .validate(&response)
            .unwrap();
        response["result"]["value"].clone()
    }
    fn render(&self, destination: &Path) -> Value {
        json!({"input":{"project":self.project,"composition":self.document["compositions"][0]["id"],
            "region":{"origin":[0,0],"extent":[64,32],"pixels":[64,32]}},
            "range":{"start":{"num":"0","den":"1"},"end":{"num":"1","den":"8"}},
            "frame_rate":{"num":"24","den":"1"},"output_directory":destination})
    }
    fn submit(&self, name: &str, gate: Option<&Path>) -> JobRecord {
        self.submit_request(
            json!({"operation":"render.submit","render":self.render(&self.temp.path().join(name))}),
            gate,
            false,
        )
    }
    fn submit_request(&self, request: Value, gate: Option<&Path>, corrupt: bool) -> JobRecord {
        serde_json::from_value(self.cli(request, gate, corrupt)).unwrap()
    }
    fn store(&self) -> JobStore {
        let mut c = JobConfig::at(&self.state);
        c.heartbeat_interval = Duration::from_millis(50);
        c.heartbeat_timeout = self.heartbeat_timeout;
        JobStore::open(c).unwrap()
    }
    fn wait(&self, id: &str, status: JobStatus) -> JobRecord {
        let start = Instant::now();
        loop {
            let r = self.store().get(id).unwrap();
            if r.status == status {
                return r;
            }
            assert!(
                r.status.active(),
                "unexpected terminal record {r:?}; log: {}",
                std::fs::read_to_string(self.store().directory(id).unwrap().join("worker.log"))
                    .unwrap_or_default()
            );
            assert!(
                start.elapsed() < self.wait_timeout,
                "timed out: {r:?}; log: {}",
                std::fs::read_to_string(self.store().directory(id).unwrap().join("worker.log"))
                    .unwrap_or_default()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

#[test]
fn cli_exit_detaches_worker_and_preserves_project_bytes_and_mtime() {
    let f = Fixture::new();
    let bytes = std::fs::read(&f.project).unwrap();
    let mtime = std::fs::metadata(&f.project).unwrap().modified().unwrap();
    let gate = f.temp.path().join("release");
    let submitted = f.submit("output", Some(&gate));
    assert_eq!(submitted.status, JobStatus::Queued);
    let running = f.wait(&submitted.id, JobStatus::Running);
    let pid = running.worker_pid.unwrap() as i32;
    #[cfg(unix)]
    assert_eq!(
        nix::unistd::getsid(Some(nix::unistd::Pid::from_raw(pid)))
            .unwrap()
            .as_raw(),
        pid
    );
    assert_ne!(pid, std::process::id() as i32);
    assert!(
        f.store()
            .directory(&submitted.id)
            .unwrap()
            .join("input.json")
            .is_file()
    );
    assert!(f.state.join("jobs.sqlite3").is_file());
    assert!(!submitted.destination.exists());
    std::fs::write(&gate, b"release").unwrap();
    let finished = f.wait(&submitted.id, JobStatus::Succeeded);
    assert_eq!(finished.completed_frames, 3);
    assert_eq!(finished.result.as_ref().unwrap()["validated"], true);
    let queried: JobRecord = serde_json::from_value(f.cli(
        json!({"operation":"job.get","job":submitted.id}),
        None,
        false,
    ))
    .unwrap();
    assert_eq!(queried.status, JobStatus::Succeeded);
    assert_eq!(std::fs::read(&f.project).unwrap(), bytes);
    assert_eq!(
        std::fs::metadata(&f.project).unwrap().modified().unwrap(),
        mtime
    );
    assert!(submitted.destination.join("sequence.json").is_file());
    assert_eq!(
        f.cli(json!({"operation":"job.list"}), None, false)["jobs"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn sequence_target_job_preserves_placements_after_trim_and_project_removal() {
    let f = Fixture::new();
    let sequence = kronello_model::SequenceId::new();
    let track = kronello_model::TrackId::new();
    let clip = kronello_model::ClipId::new();
    let session = track.as_uuid();
    let original_range = json!({"start":{"num":"0","den":"1"},"end":{"num":"1","den":"8"}});
    let definition = json!({"id":sequence,"extent":{"width":64.0,"height":32.0},
        "frame_rate":{"num":"24","den":"1"},"audio_rate":48000,"working_space":"linear_rec709",
        "tracks":[{"id":track,"kind":"video","clips":[{"id":clip,
            "source_ref":{"kind":"composition","composition":f.document["compositions"][0]["id"]},
            "timeline_range":original_range,"source_in":{"num":"0","den":"1"},
            "time_map":{"kind":"linear","offset":{"num":"0","den":"1"},"speed":{"num":"1","den":"1"}},
            "audio_retime":"reject","links":[],"effects":[]}]}]});
    f.cli(
        json!({"operation":"sequence.create","project":f.project,"base_revision":"1",
        "session_id":session,"idempotency_key":"sequence-job","sequence":definition}),
        None,
        false,
    );
    let baseline = f.temp.path().join("sequence-baseline");
    let mut render = f.render(&baseline);
    render["input"]
        .as_object_mut()
        .unwrap()
        .remove("composition");
    render["input"]["target"] = json!({"kind":"sequence","sequence":sequence});
    let mut synchronous = render.clone();
    synchronous["operation"] = json!("render.sequence");
    let expected = f.cli(synchronous, None, false);
    for invalid_input in [
        {
            let mut both = render["input"].clone();
            both["composition"] = f.document["compositions"][0]["id"].clone();
            both
        },
        {
            let mut neither = render["input"].clone();
            neither.as_object_mut().unwrap().remove("target");
            neither
        },
    ] {
        let mut invalid_render = render.clone();
        invalid_render["input"] = invalid_input;
        invalid_render["output_directory"] = json!(f.temp.path().join("invalid"));
        let response = Service::new(BackendSelection::CpuReference)
            .with_job_config(JobConfig::at(&f.state))
            .execute_json(
                &json!({"operation":"render.submit","render":invalid_render}).to_string(),
            );
        let response = serde_json::to_value(response).unwrap();
        assert_eq!(response["error"]["code"], "INVALID_REQUEST");
    }
    let gate = f.temp.path().join("sequence-release");
    render["output_directory"] = json!(f.temp.path().join("sequence-job"));
    let submitted = f.submit_request(
        json!({"operation":"render.submit","render":render,
        "output":{"format":"image_sequence"}}),
        Some(&gate),
        false,
    );
    f.wait(&submitted.id, JobStatus::Running);
    let mut movie_render = render.clone();
    movie_render["output_directory"] = json!(f.temp.path().join("sequence.mov"));
    let movie = f.submit_request(
        json!({"operation":"render.submit","render":movie_render,
            "output":{"format":"pro_res_mov","background":[0.0,0.0,0.0],"clips":[]}}),
        Some(&gate),
        false,
    );
    let fixed: Value = serde_json::from_slice(
        &std::fs::read(
            f.store()
                .directory(&submitted.id)
                .unwrap()
                .join("input.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(fixed["snapshot"]["project"]["sequences"][0], definition);
    assert_eq!(fixed["snapshot"]["sequence"], json!(sequence));
    assert_eq!(fixed["snapshot"]["revision"], 2);
    assert_eq!(fixed["snapshot"]["project"]["assets"], f.document["assets"]);
    assert_eq!(
        fixed["snapshot"]["semantic_versions"]["document"],
        f.document["semantic_version"]
    );
    f.cli(
        json!({"operation":"clip.trim","project":f.project,"base_revision":"2",
        "session_id":session,"idempotency_key":"trim-after-submit","sequence":sequence,"clip":clip,
        "range":{"start":{"num":"1","den":"24"},"end":{"num":"1","den":"8"}}}),
        None,
        false,
    );
    let changed = f.temp.path().join("sequence-changed");
    let mut after = render.clone();
    after["operation"] = json!("render.sequence");
    after["output_directory"] = json!(changed);
    f.cli(after, None, false);
    assert_ne!(
        std::fs::read(changed.join("frame-00000000.png")).unwrap(),
        std::fs::read(baseline.join("frame-00000000.png")).unwrap()
    );
    std::fs::remove_file(&f.project).unwrap();
    std::fs::write(gate, b"release").unwrap();
    let done = f.wait(&submitted.id, JobStatus::Succeeded);
    assert_eq!(done.completed_frames, 3);
    let manifest: Value = serde_json::from_slice(
        &std::fs::read(submitted.destination.join("sequence.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest, expected);
    for frame in manifest["frames"].as_array().unwrap() {
        for artifact in ["numeric", "display"] {
            let name = frame[artifact]["name"].as_str().unwrap();
            assert_eq!(
                std::fs::read(submitted.destination.join(name)).unwrap(),
                std::fs::read(baseline.join(name)).unwrap()
            );
        }
        assert_eq!(frame["metadata"]["revision"], "2");
        assert_eq!(
            frame["metadata"]["snapshot_content_hash"],
            submitted.snapshot_hash
        );
    }
    let movie_done = f.wait(&movie.id, JobStatus::Succeeded);
    assert_eq!(movie_done.completed_frames, 3);
    assert_eq!(movie.snapshot_hash, submitted.snapshot_hash);
    let report = &movie_done.result.as_ref().unwrap()["report"];
    assert_eq!(report["render_snapshot_hash"], submitted.snapshot_hash);
    assert_eq!(
        report["audio_render_snapshot_hash"],
        submitted.snapshot_hash
    );
    let streams = report["probe"]["streams"].as_array().unwrap();
    assert_eq!(streams.len(), 2);
    assert!(streams.iter().any(|s| s["codec"] == "prores"));
    assert!(streams.iter().any(|s| s["codec"] == "pcm_s24le"));
    let decoded = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(&movie.destination)
        .args([
            "-map",
            "0:a:0",
            "-f",
            "f32le",
            "-acodec",
            "pcm_f32le",
            "pipe:1",
        ])
        .output()
        .unwrap();
    assert!(
        decoded.status.success(),
        "{}",
        String::from_utf8_lossy(&decoded.stderr)
    );
    assert!(!decoded.stdout.is_empty());
    let (samples, remainder) = decoded.stdout.as_chunks::<4>();
    assert!(remainder.is_empty());
    assert!(
        samples
            .iter()
            .all(|sample| f32::from_le_bytes(*sample) == 0.0)
    );
}

#[test]
fn fifo_one_slot_and_fixed_snapshot_survive_project_edits() {
    let f = Fixture::new();
    let baseline = f.temp.path().join("baseline");
    f.service(json!({"operation":"render.sequence","input":f.render(&baseline)["input"],
        "range":f.render(&baseline)["range"],"frame_rate":f.render(&baseline)["frame_rate"],"output_directory":baseline}));
    let gate = f.temp.path().join("release");
    let first = f.submit_request(json!({"operation":"render.submit", "expected_revision":"1", "render":f.render(&f.temp.path().join("first"))}), Some(&gate), false);
    f.wait(&first.id, JobStatus::Running);
    let second = f.submit("second", None);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(f.store().get(&second.id).unwrap().status, JobStatus::Queued);
    assert!(!second.destination.exists());
    let mut edited = f.document.clone();
    edited["compositions"][0]["nodes"][0]["properties"][2]["source"]["value"]["value"]["components"]
        ["r"] = json!(0.0);
    edited["compositions"][0]["nodes"][0]["properties"][2]["source"]["value"]["value"]["components"]
        ["g"] = json!(1.0);
    f.service(json!({"operation":"project.import","project":f.project,"base_revision":"1","document":edited}));
    std::fs::write(gate, b"release").unwrap();
    let done1 = f.wait(&first.id, JobStatus::Succeeded);
    let done2 = f.wait(&second.id, JobStatus::Succeeded);
    assert!(done2.finished_at_ms >= done1.finished_at_ms);
    for output in [&first.destination, &second.destination] {
        assert_eq!(
            std::fs::read(output.join("frame-00000000.png")).unwrap(),
            std::fs::read(baseline.join("frame-00000000.png")).unwrap()
        );
        let manifest: Value =
            serde_json::from_slice(&std::fs::read(output.join("sequence.json")).unwrap()).unwrap();
        assert_eq!(manifest["frames"][0]["metadata"]["revision"], "1");
        assert_eq!(
            manifest["frames"][0]["metadata"]["snapshot_content_hash"],
            first.snapshot_hash
        );
    }
    let after = f.temp.path().join("after");
    f.service(json!({"operation":"render.sequence","input":f.render(&after)["input"],
        "range":f.render(&after)["range"],"frame_rate":f.render(&after)["frame_rate"],"output_directory":after}));
    assert_ne!(
        std::fs::read(after.join("frame-00000000.png")).unwrap(),
        std::fs::read(first.destination.join("frame-00000000.png")).unwrap()
    );
}

#[test]
fn sigkill_is_detected_by_heartbeat_and_releases_slot() {
    let mut f = Fixture::new();
    f.heartbeat_timeout = Duration::from_secs(1);
    let first = f.submit("killed", Some(&f.temp.path().join("never")));
    let running = f.wait(&first.id, JobStatus::Running);
    let second = f.submit("next", None);
    kronello_platform::ProcessGuard::capture(running.worker_pid.unwrap())
        .unwrap()
        .terminate_and_wait()
        .unwrap();
    let dead = f.wait(&first.id, JobStatus::Interrupted);
    assert_eq!(dead.error.unwrap().code, "JOB_INTERRUPTED");
    f.wait(&second.id, JobStatus::Succeeded);
    assert!(!first.destination.exists());
}

#[test]
fn worker_heartbeat_retries_writer_contention_and_logs_recovery() {
    let f = Fixture::new();
    let gate = f.temp.path().join("release");
    let submitted = f.submit("output", Some(&gate));
    f.wait(&submitted.id, JobStatus::Running);
    let log = f
        .store()
        .directory(&submitted.id)
        .unwrap()
        .join("worker.log");
    let db = rusqlite::Connection::open(f.state.join("jobs.sqlite3")).unwrap();
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    let start = Instant::now();
    let failed = loop {
        if std::fs::read_to_string(&log)
            .unwrap()
            .contains("worker heartbeat failed")
        {
            break true;
        }
        if start.elapsed() > Duration::from_secs(5) {
            break false;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    db.execute_batch("ROLLBACK").unwrap();
    let start = Instant::now();
    let recovered = loop {
        if std::fs::read_to_string(&log)
            .unwrap()
            .contains("worker heartbeat recovered")
        {
            break true;
        }
        if start.elapsed() > Duration::from_secs(5) {
            break false;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    std::fs::write(gate, b"release").unwrap();
    f.wait(&submitted.id, JobStatus::Succeeded);
    assert!(
        failed && recovered,
        "{}",
        std::fs::read_to_string(&log).unwrap()
    );
    assert!(
        std::fs::read_to_string(&log)
            .unwrap()
            .contains("worker startup pid=")
    );
}

#[test]
fn queued_worker_starts_after_writer_lock_exceeds_old_busy_timeout() {
    let f = Fixture::new();
    let gate = f.temp.path().join("release");
    let first = f.submit("blocker", Some(&gate));
    f.wait(&first.id, JobStatus::Running);
    let second = f.submit("contended", None);
    let db = rusqlite::Connection::open(f.state.join("jobs.sqlite3")).unwrap();
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    // Old claim() propagated SQLITE_BUSY after five seconds, killing the worker
    // while its record could remain queued. This exceeds that fixed timeout.
    std::thread::sleep(Duration::from_secs(6));
    let observed = f.store().get(&second.id).unwrap();
    assert_eq!(observed.status, JobStatus::Queued);
    db.execute_batch("ROLLBACK").unwrap();
    std::fs::write(gate, b"release").unwrap();
    f.wait(&first.id, JobStatus::Succeeded);
    let done = f.wait(&second.id, JobStatus::Succeeded);
    let log =
        std::fs::read_to_string(f.store().directory(&done.id).unwrap().join("worker.log")).unwrap();
    assert!(log.contains("worker slot contention"), "{log}");
    assert!(log.contains("worker slot acquired"), "{log}");
}

#[test]
fn worker_checkpoint_and_heartbeat_share_serialized_connection_lifetimes() {
    let f = Fixture::new();
    let mut render = f.render(&f.temp.path().join("checkpoint-stress"));
    render["range"]["end"] = json!({"num":"4","den":"1"});
    let r = f.submit_request(
        json!({"operation":"render.submit","render":render}),
        None,
        false,
    );
    let done = f.wait(&r.id, JobStatus::Succeeded);
    assert_eq!(done.completed_frames, 96);
}

#[test]
fn failed_test_reaps_detached_workers_before_fixture_removal() {
    for timeout in [false, true] {
        let mut f = Fixture::new();
        let a = f.submit("never", Some(&f.temp.path().join("never")));
        let pid = f.wait(&a.id, JobStatus::Running).worker_pid.unwrap();
        f.wait_timeout = Duration::from_millis(100);
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let f = f;
            if timeout {
                f.wait(&a.id, JobStatus::Succeeded);
            } else {
                panic!("intentional test failure with live detached worker")
            }
        }));
        assert!(failure.is_err());
        assert!(!kronello_platform::process_is_alive(pid), "orphan {pid}");
    }
}

#[test]
#[cfg(unix)]
fn suspended_live_worker_is_not_interrupted_by_an_expired_heartbeat() {
    struct ResumeWorker(nix::unistd::Pid);
    impl Drop for ResumeWorker {
        fn drop(&mut self) {
            let _ = nix::sys::signal::kill(self.0, nix::sys::signal::Signal::SIGCONT);
        }
    }
    let f = Fixture::new();
    let gate = f.temp.path().join("release");
    let submitted = f.submit("output", Some(&gate));
    let running = f.wait(&submitted.id, JobStatus::Running);
    let pid = nix::unistd::Pid::from_raw(running.worker_pid.unwrap() as i32);
    let mut resume = None;
    rewrite_record(&f, &submitted.id, |r| {
        // Hold the writer lock before stopping the process, so SIGSTOP cannot
        // freeze its heartbeat in the middle of a write transaction.
        nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGSTOP).unwrap();
        resume = Some(ResumeWorker(pid));
        r.heartbeat_at_ms = kronello_jobs::now_ms() - 60000
    });
    let observed = f.store().get(&submitted.id).unwrap();
    drop(resume);
    std::fs::write(gate, b"release").unwrap();
    assert_eq!(observed.status, JobStatus::Running);
    f.wait(&submitted.id, JobStatus::Succeeded);
}

#[test]
fn duplicate_worker_cannot_fail_or_steal_an_active_job() {
    let f = Fixture::new();
    let gate = f.temp.path().join("release");
    let submitted = f.submit("output", Some(&gate));
    let original = f.wait(&submitted.id, JobStatus::Running);
    let child = Command::new(env!("CARGO_BIN_EXE_kronello"))
        .args(["worker", "--job", &submitted.id])
        .env("KRONELLO_STATE_ROOT", &f.state)
        .env_remove("KRONELLO_TEST_JOB_GATE")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // This deliberate duplicate is never registered as the job owner, so it
    // needs its own guard in addition to the fixture's registered-worker guard.
    let duplicate = kronello_platform::ProcessGuard::capture(child.id()).unwrap();
    let output = child.wait_with_output().unwrap();
    duplicate.wait_for_exit(Duration::from_secs(5)).unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("JOB_ALREADY_OWNED"));
    let current = f.store().get(&submitted.id).unwrap();
    assert_eq!(current.status, JobStatus::Running);
    assert_eq!(current.worker_pid, original.worker_pid);
    std::fs::write(gate, b"release").unwrap();
    f.wait(&submitted.id, JobStatus::Succeeded);
}

#[test]
fn cancel_running_and_queued_jobs_cleans_temporary_output() {
    let f = Fixture::new();
    let gate = f.temp.path().join("never");
    let first = f.submit("running", Some(&gate));
    f.wait(&first.id, JobStatus::Running);
    let second = f.submit("queued", Some(&gate));
    f.cli(
        json!({"operation":"job.cancel","job":second.id}),
        None,
        false,
    );
    f.wait(&second.id, JobStatus::Canceled);
    f.cli(
        json!({"operation":"job.cancel","job":first.id}),
        None,
        false,
    );
    f.wait(&first.id, JobStatus::Canceled);
    assert!(!first.destination.exists() && !second.destination.exists());
    assert!(std::fs::read_dir(f.temp.path()).unwrap().all(|e| {
        !e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".kronello-job-")
    }));
}

#[test]
fn output_validation_failure_and_destination_race_preserve_deliverables() {
    let f = Fixture::new();
    let bad = f.submit_request(
        json!({"operation":"render.submit","render":f.render(&f.temp.path().join("bad"))}),
        None,
        true,
    );
    assert_eq!(
        f.wait(&bad.id, JobStatus::Failed).error.unwrap().code,
        "OUTPUT_VALIDATION_FAILED"
    );
    assert!(!bad.destination.exists());
    let gate = f.temp.path().join("release");
    let race = f.submit("race", Some(&gate));
    f.wait(&race.id, JobStatus::Running);
    std::fs::create_dir(&race.destination).unwrap();
    std::fs::write(
        race.destination.join("existing.txt"),
        b"existing deliverable",
    )
    .unwrap();
    std::fs::write(gate, b"release").unwrap();
    assert_eq!(
        f.wait(&race.id, JobStatus::Failed).error.unwrap().code,
        "OUTPUT_EXISTS"
    );
    assert_eq!(
        std::fs::read(race.destination.join("existing.txt")).unwrap(),
        b"existing deliverable"
    );
    assert!(!race.destination.join("sequence.json").exists());
}

fn rewrite_record(f: &Fixture, id: &str, edit: impl FnOnce(&mut JobRecord)) {
    let mut db = rusqlite::Connection::open(f.state.join("jobs.sqlite3")).unwrap();
    let tx = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    let text: String = tx
        .query_row("SELECT record FROM jobs WHERE id=?1", [id], |r| r.get(0))
        .unwrap();
    let mut record: JobRecord = serde_json::from_str(&text).unwrap();
    edit(&mut record);
    tx.execute(
        "UPDATE jobs SET record=?1 WHERE id=?2",
        rusqlite::params![serde_json::to_string(&record).unwrap(), id],
    )
    .unwrap();
    tx.commit().unwrap();
}
fn historical(f: &Fixture, status: JobStatus, age_days: i64) -> JobRecord {
    let r = f
        .store()
        .submit(
            b"historical input",
            Submission {
                engine_version: "test".into(),
                project_id: "test".into(),
                revision: "1".into(),
                snapshot_hash: "test".into(),
                output_profile: json!({}),
                destination: f.temp.path().join("historical"),
                total_frames: 1,
            },
        )
        .unwrap();
    rewrite_record(f, &r.id, |r| {
        r.status = status;
        r.finished_at_ms = Some(kronello_jobs::now_ms() - age_days * 86400000);
    });
    r
}
#[test]
fn submit_and_manual_prune_keep_records_and_exclude_interrupted() {
    let f = Fixture::new();
    let old = historical(&f, JobStatus::Succeeded, 31);
    let interrupted = historical(&f, JobStatus::Interrupted, 90);
    let recent = historical(&f, JobStatus::Canceled, 29);
    // Creation of a later historical row also invokes submit-time pruning.
    assert!(!f.store().directory(&old.id).unwrap().exists());
    let failed = historical(&f, JobStatus::Failed, 31);
    let pruned = f.cli(json!({"operation":"job.prune"}), None, false);
    assert!(
        pruned["pruned"]
            .as_array()
            .unwrap()
            .contains(&json!(failed.id))
    );
    assert!(f.store().get(&old.id).unwrap().directory_pruned);
    assert!(f.store().get(&failed.id).unwrap().directory_pruned);
    assert!(f.store().directory(&interrupted.id).unwrap().is_dir());
    assert!(f.store().directory(&recent.id).unwrap().is_dir());
    let old_cancel = historical(&f, JobStatus::Canceled, 31);
    let actual = f.submit("actual", None);
    f.wait(&actual.id, JobStatus::Succeeded);
    assert!(!f.store().directory(&old_cancel.id).unwrap().exists());
    assert!(f.store().get(&old_cancel.id).unwrap().directory_pruned);
}

#[test]
fn changed_asset_content_fails_worker_with_typed_error() {
    let f = Fixture::new();
    let gate = f.temp.path().join("release");
    let blocker = f.submit("blocker", Some(&gate));
    f.wait(&blocker.id, JobStatus::Running);
    let asset_path = f.temp.path().join("asset.bin");
    std::fs::write(&asset_path, b"original").unwrap();
    let mut document = f.document.clone();
    document["assets"] = json!([{"id":"363c4456-c273-4ad6-a00c-a90bfb7af44d","kind":"audio",
        "content_hash":format!("{:x}", Sha256::digest(b"original")),"streams":[],"locator":{"relative":"asset.bin","absolute":asset_path}}]);
    f.service(json!({"operation":"project.import","project":f.project,"base_revision":"1","document":document}));
    let queued = f.submit("mismatch", None);
    assert_eq!(f.store().get(&queued.id).unwrap().status, JobStatus::Queued);
    std::fs::write(asset_path, b"modified").unwrap();
    std::fs::write(gate, b"release").unwrap();
    f.wait(&blocker.id, JobStatus::Succeeded);
    assert_eq!(
        f.wait(&queued.id, JobStatus::Failed).error.unwrap().code,
        "ASSET_HASH_MISMATCH"
    );
    assert!(!queued.destination.exists());
}

#[test]
fn worker_verifies_saved_font_lock_before_output() {
    let f = Fixture::new();
    let document: Value =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    f.service(json!({"operation":"project.import","project":f.project,"base_revision":"1","document":document}));
    let font = f.temp.path().join("wrong-font.otf");
    std::fs::write(&font, b"wrong locked font bytes").unwrap();
    let mut render = f.render(&f.temp.path().join("font-output"));
    render["input"]["fonts"] =
        json!([{"identity":document["texts"][0]["styles"][0]["font"],"path":font}]);
    let submitted = f.submit_request(
        json!({"operation":"render.submit","render":render}),
        None,
        false,
    );
    assert_eq!(
        f.wait(&submitted.id, JobStatus::Failed).error.unwrap().code,
        "ASSET_HASH_MISMATCH"
    );
    assert!(!submitted.destination.exists());
}

#[test]
fn worker_checks_saved_schema_semantics_features_and_input_hash() {
    for (field, expected) in [
        ("schema", "UNSUPPORTED_SCHEMA_VERSION"),
        ("semantics", "UNSUPPORTED_FEATURE"),
        ("features", "UNSUPPORTED_FEATURE"),
        ("hash", "JOB_INPUT_HASH_MISMATCH"),
    ] {
        let f = Fixture::new();
        let gate = f.temp.path().join("release");
        let blocker = f.submit("blocker", Some(&gate));
        f.wait(&blocker.id, JobStatus::Running);
        let queued = f.submit("future", None);
        let path = f.store().directory(&queued.id).unwrap().join("input.json");
        let mut input: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        match field {
            "schema" => input["schema_version"] = json!(999),
            "semantics" => input["snapshot"]["semantic_versions"]["document"] = json!(999),
            "features" => input["request"]["required_features"] = json!(["future-engine-feature"]),
            _ => input["request"]["required_features"] = json!(["tampered"]),
        }
        let bytes = serde_json::to_vec(&input).unwrap();
        std::fs::write(path, &bytes).unwrap();
        if field != "hash" {
            rewrite_record(&f, &queued.id, |r| {
                r.input_hash = format!("{:x}", Sha256::digest(&bytes))
            });
        }
        std::fs::write(gate, b"release").unwrap();
        f.wait(&blocker.id, JobStatus::Succeeded);
        assert_eq!(
            f.wait(&queued.id, JobStatus::Failed).error.unwrap().code,
            expected
        );
        assert!(!queued.destination.exists());
    }
}

#[test]
fn expression_job_pins_ast_and_renders_after_source_project_removal() {
    let f = Fixture::new();
    let node = &f.document["compositions"][0]["nodes"][0];
    let property = node["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["descriptor"]["key"] == "kronello.opacity")
        .unwrap();
    let id = "173087e0-c21b-43de-9371-8e1da051095a";
    let expression = |value| json!({"expression_set":{"expression":{"id":id,"version":1,"value_type":"scalar","nodes":[{"literal":{"kind":"scalar","value":value}}]}}});
    let commands = json!([expression(0.4), {"property_source_set":{"object":node["id"],"property":property["id"],"source":{"kind":"expression","value":id}}}]);
    let plan = f.service(json!({"operation":"edit.plan","project":f.project,"base_revision":"1","commands":commands}));
    f.service(json!({"operation":"edit.apply","project":f.project,"base_revision":"1","commands":commands,"plan_hash":plan["plan_hash"],"session_id":"96607679-eefd-407a-a3a8-59943f2bd82f","idempotency_key":"set-expression"}));
    let gate = f.temp.path().join("expression-release");
    let submitted = f.submit_request(json!({"operation":"render.submit","render":f.render(&f.temp.path().join("expression-output")),"required_features":["expression"]}), Some(&gate), false);
    f.wait(&submitted.id, JobStatus::Running);
    let commands = json!([expression(0.8)]);
    let plan = f.service(json!({"operation":"edit.plan","project":f.project,"base_revision":"2","commands":commands}));
    f.service(json!({"operation":"edit.apply","project":f.project,"base_revision":"2","commands":commands,"plan_hash":plan["plan_hash"],"session_id":"96607679-eefd-407a-a3a8-59943f2bd82f","idempotency_key":"change-expression"}));
    std::fs::remove_file(&f.project).unwrap();
    std::fs::write(gate, b"release").unwrap();
    let finished = f.wait(&submitted.id, JobStatus::Succeeded);
    assert_eq!(finished.revision, "2");
    assert_eq!(finished.completed_frames, 3);
    let input: Value = serde_json::from_slice(
        &std::fs::read(
            f.store()
                .directory(&submitted.id)
                .unwrap()
                .join("input.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        input["snapshot"]["project"]["expressions"][0]["nodes"][0]["literal"]["value"],
        0.4
    );
    assert_eq!(input["snapshot"]["semantic_versions"]["expression"], 1);
    let snapshot: kronello_render::RenderSnapshot =
        serde_json::from_str(&input["snapshot"].to_string()).unwrap();
    let time = serde_json::from_value(json!({"num":"0","den":"1"})).unwrap();
    let ir = kronello_render::build_scene_ir(&snapshot, time, &[]).unwrap();
    assert_eq!(ir.nodes[0].opacity, 0.4);
    let metadata: Value =
        serde_json::from_slice(&std::fs::read(finished.destination.join("sequence.json")).unwrap())
            .unwrap();
    for frame in metadata["frames"].as_array().unwrap() {
        assert_eq!(
            frame["metadata"]["snapshot_content_hash"],
            submitted.snapshot_hash
        );
    }
}

#[test]
fn mov_export_uses_fixed_video_and_audio_snapshot() {
    let f = Fixture::new();
    let audio = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/data/sine-48k-stereo.wav")
        .canonicalize()
        .unwrap();
    let mut document = f.document.clone();
    let asset_id = "363c4456-c273-4ad6-a00c-a90bfb7af44d";
    document["assets"] = json!([{"id":asset_id,"kind":"audio","content_hash":format!("{:x}", Sha256::digest(std::fs::read(&audio).unwrap())),
        "streams":[],"locator":{"absolute":audio}}]);
    f.service(json!({"operation":"project.import","project":f.project,"base_revision":"1","document":document}));
    let render = f.render(&f.temp.path().join("audio.mov"));
    let submitted = f.submit_request(json!({"operation":"render.submit","render":render,
        "output":{"format":"pro_res_mov","background":[0.1,0.1,0.1],"clips":[{"asset":asset_id,"stream_index":0,
            "placement":{"start":{"num":"0","den":"1"},"end":{"num":"1","den":"24"}},"source_in":{"num":"0","den":"1"},"gain":0.5}]}}), None, false);
    let finished = f.wait(&submitted.id, JobStatus::Succeeded);
    let report = &finished.result.unwrap()["report"];
    assert_eq!(
        report["render_snapshot_hash"],
        report["audio_render_snapshot_hash"]
    );
    assert_eq!(report["render_snapshot_hash"], submitted.snapshot_hash);
    let streams = report["probe"]["streams"].as_array().unwrap();
    assert_eq!(streams.len(), 2);
    assert!(streams.iter().any(|s| s["codec"] == "pcm_s24le"));
    assert_eq!(report["frames"].as_array().unwrap().len(), 3);
    assert!(submitted.destination.is_file());
}

#[test]
fn invalid_temporary_mov_is_rejected_before_final_filename_exists() {
    let f = Fixture::new();
    let submitted = f.submit_request(
        json!({"operation":"render.submit","render":f.render(&f.temp.path().join("bad.mov")),
        "output":{"format":"pro_res_mov","clips":[],"background":[0.1,0.1,0.1]}}),
        None,
        true,
    );
    assert_eq!(
        f.wait(&submitted.id, JobStatus::Failed).error.unwrap().code,
        "OUTPUT_VALIDATION_FAILED"
    );
    assert!(!submitted.destination.exists());
    assert!(std::fs::read_dir(f.temp.path()).unwrap().all(|e| {
        !e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".kronello-job-")
    }));
}
#[test]
fn nle2_video_generator_fixed_job_survives_clip_edit_and_project_removal() {
    let f = Fixture::new();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/fixtures/generated/media/cfr-24-1.nut")
        .canonicalize()
        .unwrap();
    let asset = "00620000-0000-4000-8000-000000000001";
    let sequence = "00620000-0000-4000-8000-000000000002";
    let clip = "00620000-0000-4000-8000-000000000004";
    let mut doc = f.document.clone();
    doc["assets"] = json!([{"id":asset,"kind":"video","content_hash":format!("{:x}",Sha256::digest(std::fs::read(&source).unwrap())),"locator":{"relative":null,"absolute":source},"streams":[{"index":0,"codec":"rawvideo","time_base":{"num":"1","den":"24"},"duration":null,"start_time":{"num":"0","den":"1"},"width":16,"height":16,"pixel_format":"yuv420p","color_primaries":null,"color_transfer":null,"color_matrix":null,"color_range":null}]}]);
    doc["sequences"] = json!([{"id":sequence,"extent":{"width":16.0,"height":16.0},"frame_rate":{"num":"24","den":"1"},"audio_rate":48000,"working_space":"linear_rec709","tracks":[{"id":"00620000-0000-4000-8000-000000000003","kind":"video","clips":[{"id":clip,"source_ref":{"kind":"asset","asset":asset,"stream_index":0},"timeline_range":{"start":{"num":"0","den":"1"},"end":{"num":"1","den":"8"}},"source_in":{"num":"0","den":"1"},"time_map":{"kind":"linear","offset":{"num":"0","den":"1"},"speed":{"num":"1","den":"1"}},"links":[],"effects":[],"audio_retime":"reject"}]},{"id":"00620000-0000-4000-8000-000000000005","kind":"video","clips":[{"id":"00620000-0000-4000-8000-000000000006","source_ref":{"kind":"generator","generator":"kronello.solid","version":1,"color":{"space":"srgb","components":{"r":1.0,"g":0.0,"b":0.0,"alpha":0.25}}},"timeline_range":{"start":{"num":"0","den":"1"},"end":{"num":"1","den":"8"}},"source_in":{"num":"0","den":"1"},"time_map":{"kind":"linear","offset":{"num":"0","den":"1"},"speed":{"num":"1","den":"1"}},"links":[],"effects":[],"audio_retime":"reject"}]}]}]);
    f.service(json!({"operation":"project.import","project":f.project,"base_revision":"1","document":doc}));
    let reference = f.temp.path().join("video-reference");
    let destination = f.temp.path().join("video-fixed");
    let gate = f.temp.path().join("release-video");
    let mut render = f.render(&reference);
    render["input"]
        .as_object_mut()
        .unwrap()
        .remove("composition");
    render["input"]["target"] = json!({"kind":"sequence","sequence":sequence});
    render["input"]["region"] = json!({"origin":[0,0],"extent":[16,16],"pixels":[16,16]});
    let mut request = render.clone();
    request["operation"] = json!("render.sequence");
    f.cli(request, None, false);
    render["output_directory"] = json!(destination);
    let job = f.submit_request(
        json!({"operation":"render.submit","render":render}),
        Some(&gate),
        false,
    );
    f.wait(&job.id, JobStatus::Running);
    let commands = json!([{"timeline":{"clip_trim":{"sequence":sequence,"clip":clip,"range":{"start":{"num":"1","den":"24"},"end":{"num":"1","den":"8"}}}}}]);
    let plan=f.service(json!({"operation":"edit.plan","project":f.project,"base_revision":"2","commands":commands}));
    f.service(json!({"operation":"edit.apply","project":f.project,"base_revision":"2","commands":commands,"plan_hash":plan["plan_hash"],"session_id":"00620000-0000-4000-8000-000000000007","idempotency_key":"trim-video-after-submit"}));
    std::fs::remove_file(&f.project).unwrap();
    std::fs::write(&gate, b"release").unwrap();
    f.wait(&job.id, JobStatus::Succeeded);
    for entry in std::fs::read_dir(&reference).unwrap() {
        let path = entry.unwrap().path();
        assert_eq!(
            std::fs::read(&path).unwrap(),
            std::fs::read(destination.join(path.file_name().unwrap())).unwrap(),
            "{}",
            path.display()
        );
    }
}

fn audio_sequence_fixture(f: &Fixture, gain: f64) -> (Value, Value) {
    use sha2::Digest;
    let source = f.temp.path().join("audio.wav");
    let frames = 24000u32;
    let bytes = frames * 4;
    let mut wav = Vec::new();
    wav.extend(b"RIFF");
    wav.extend((36 + bytes).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(48000u32.to_le_bytes());
    wav.extend(192000u32.to_le_bytes());
    wav.extend(4u16.to_le_bytes());
    wav.extend(16u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend(bytes.to_le_bytes());
    for _ in 0..frames {
        wav.extend(4096i16.to_le_bytes());
        wav.extend((-4096i16).to_le_bytes());
    }
    std::fs::write(&source, &wav).unwrap();
    let asset = kronello_model::AssetId::new();
    let sequence = kronello_model::SequenceId::new();
    let volume = json!({"id":kronello_model::PropertyId::new(),"descriptor":{"key":"kronello.audio.volume","version":1},"source":{"kind":"constant","value":{"kind":"scalar","value":gain}},"modifiers":[]});
    let placement = json!({"start":{"num":"0","den":"1"},"end":{"num":"1","den":"5"}});
    let mut document = f.document.clone();
    document["assets"] = json!([{"id":asset,"kind":"audio","content_hash":format!("{:x}",sha2::Sha256::digest(&wav)),
        "locator":{"relative":null,"absolute":source},"streams":[{"index":0,"codec":"pcm_s16le","time_base":{"num":"1","den":"48000"},"duration":{"num":"1","den":"2"},
            "width":null,"height":null,"pixel_format":null,"color_primaries":null,"color_transfer":null,"color_matrix":null,"color_range":null}]}]);
    document["sequences"] = json!([{"id":sequence,"extent":{"width":64.0,"height":32.0},"frame_rate":{"num":"30000","den":"1001"},"audio_rate":48000,"working_space":"linear_rec709",
        "tracks":[{"id":kronello_model::TrackId::new(),"kind":"video","clips":[{"id":kronello_model::ClipId::new(),"source_ref":{"kind":"composition","composition":document["compositions"][0]["id"]},"timeline_range":placement,"source_in":{"num":"0","den":"1"},"time_map":{"kind":"linear","offset":{"num":"0","den":"1"},"speed":{"num":"1","den":"1"}},"links":[],"effects":[]}]},
            {"id":kronello_model::TrackId::new(),"kind":"audio","clips":[{"id":kronello_model::ClipId::new(),"source_ref":{"kind":"asset","asset":asset,"stream_index":0},"timeline_range":placement,"source_in":{"num":"0","den":"1"},"time_map":{"kind":"linear","offset":{"num":"0","den":"1"},"speed":{"num":"1","den":"1"}},"volume":volume,"links":[],"effects":[]}]}]}]);
    let mut render = f.render(&f.temp.path().join("document.mov"));
    render["input"]
        .as_object_mut()
        .unwrap()
        .remove("composition");
    render["input"]["target"] = json!({"kind":"sequence","sequence":sequence});
    render["frame_rate"] = json!({"num":"30000","den":"1001"});
    render["range"] = json!({"start":{"num":"0","den":"1"},"end":{"num":"1001","den":"10000"}});
    (document, render)
}
#[test]
fn document_audio_sync_and_fixed_job_survive_volume_edit_and_project_deletion_ntsc() {
    let f = Fixture::new();
    let (document, mut render) = audio_sequence_fixture(&f, 0.5);
    f.service(json!({"operation":"project.import","project":f.project,"base_revision":"1","document":document}));
    let profile = json!({"format":"pro_res_mov","profile_version":2,"audio":"document","clips":[],"background":[0,0,0]});
    let baseline = f.cli(
        json!({"operation":"render.export","render":render,"output":profile}),
        None,
        false,
    );
    let baseline_file = render["output_directory"].as_str().unwrap().to_owned();
    let gate = f.temp.path().join("audio-release");
    render["output_directory"] = json!(f.temp.path().join("job.mov"));
    let job = f.submit_request(
        json!({"operation":"render.submit","render":render,"output":profile}),
        Some(&gate),
        false,
    );
    f.wait(&job.id, JobStatus::Running);
    let fixed: Value = serde_json::from_slice(
        &std::fs::read(f.store().directory(&job.id).unwrap().join("input.json")).unwrap(),
    )
    .unwrap();
    use sha2::Digest;
    let bytes = std::fs::read(f.store().directory(&job.id).unwrap().join("input.json")).unwrap();
    assert_eq!(
        format!("{:x}", sha2::Sha256::digest(&bytes)),
        job.input_hash
    );
    let mut other_mode = fixed.clone();
    other_mode["request"]["output"]["audio"] = json!("silence");
    assert_ne!(
        sha2::Sha256::digest(serde_json::to_vec(&fixed).unwrap()),
        sha2::Sha256::digest(serde_json::to_vec(&other_mode).unwrap())
    );
    assert_eq!(fixed["request"]["output"]["audio"], "document");
    assert_eq!(fixed["request"]["output"]["profile_version"], 2);
    let mut changed = document.clone();
    changed["sequences"][0]["tracks"][1]["clips"][0]["volume"]["source"]["value"]["value"] =
        json!(0.0);
    changed["compositions"][0]["nodes"][0]["properties"][2]["source"]["value"]["value"]["components"]
        ["r"] = json!(0.0);
    f.service(json!({"operation":"project.import","project":f.project,"base_revision":"2","document":changed}));
    std::fs::remove_file(&f.project).unwrap();
    std::fs::write(gate, b"release").unwrap();
    let done = f.wait(&job.id, JobStatus::Succeeded);
    let report = &done.result.as_ref().unwrap()["report"];
    assert_eq!(report["frames"], baseline["frames"]);
    assert_eq!(
        report["export_snapshot_hash"],
        baseline["export_snapshot_hash"]
    );
    assert_eq!(report["sample_range"], json!({"start":0,"end":4804}));
    assert_eq!(report["audio"]["frames"], 4804);
    let runtime = kronello_media::MediaRuntime::load().unwrap();
    runtime
        .probe(&job.destination)
        .unwrap()
        .verify_av()
        .unwrap();
    let baseline_audio = runtime.decode_audio(Path::new(&baseline_file), 1).unwrap();
    let job_audio = runtime.decode_audio(&job.destination, 1).unwrap();
    assert_eq!(job_audio.buffer, baseline_audio.buffer);
    assert_eq!(job_audio.buffer.frames()[0], [0.0625, -0.0625]);
    let mut original_video = runtime.open_video(Path::new(&baseline_file)).unwrap();
    let mut fixed_video = runtime.open_video(&job.destination).unwrap();
    for frame in baseline["frames"].as_array().unwrap() {
        let time = serde_json::from_value(frame["time"].clone()).unwrap();
        assert_eq!(
            original_video.decode_at(time).unwrap().pixels,
            fixed_video.decode_at(time).unwrap().pixels
        );
    }
}
#[test]
fn document_audio_worker_missing_hash_mismatch_and_clipping_never_publish() {
    for failure in ["missing", "hash", "clipping"] {
        let f = Fixture::new();
        let (document, render) =
            audio_sequence_fixture(&f, if failure == "clipping" { 16.0 } else { 0.5 });
        f.service(json!({"operation":"project.import","project":f.project,"base_revision":"1","document":document}));
        let gate = f.temp.path().join("failure-release");
        let job=f.submit_request(json!({"operation":"render.submit","render":render,"output":{"format":"pro_res_mov","profile_version":2,"audio":"document","clips":[],"background":[0,0,0]}}),Some(&gate),false);
        f.wait(&job.id, JobStatus::Running);
        let asset = PathBuf::from(
            document["assets"][0]["locator"]["absolute"]
                .as_str()
                .unwrap(),
        );
        match failure {
            "missing" => std::fs::remove_file(asset).unwrap(),
            "hash" => std::fs::write(asset, b"changed").unwrap(),
            _ => (),
        }
        std::fs::write(gate, b"release").unwrap();
        let done = f.wait(&job.id, JobStatus::Failed);
        assert_eq!(
            done.error.unwrap().code,
            match failure {
                "missing" => "ASSET_MISSING",
                "hash" => "ASSET_HASH_MISMATCH",
                _ => "AUDIO_CLIPPING",
            }
        );
        assert!(!job.destination.exists());
    }
}

#[test]
fn audio4_retime_gain_generator_crossfade_fixed_job_matches_sync_ntsc() {
    let f = Fixture::new();
    let (mut document, mut render) = audio_sequence_fixture(&f, 0.5);
    let gain = kronello_model::PropertyId::new();
    let outgoing = &mut document["sequences"][0]["tracks"][1]["clips"][0];
    outgoing["timeline_range"]["end"] = json!({"num":"1","den":"10"});
    outgoing["time_map"]["speed"] = json!({"num":"2","den":"1"});
    outgoing["audio_retime"] = json!("resample_v1");
    outgoing["properties"] = json!([{"id":gain,"descriptor":{"key":"kronello.audio.volume","version":1},"source":{"kind":"constant","value":{"kind":"scalar","value":0.5}},"modifiers":[]}]);
    outgoing["effects"] = json!([{"effect_id":"kronello.audio.gain","version":1,"parameters":{"kind":"audio_gain","gain":gain}}]);
    let outgoing_id = outgoing["id"].clone();
    let mut incoming = outgoing.clone();
    incoming["id"] = json!(kronello_model::ClipId::new());
    incoming["timeline_range"] =
        json!({"start":{"num":"1","den":"20"},"end":{"num":"1","den":"5"}});
    incoming["source_ref"] =
        json!({"kind":"generator","generator":"kronello.audio.tone440","version":1});
    incoming["time_map"]["speed"] = json!({"num":"1","den":"1"});
    incoming["effects"] = json!([]);
    incoming["properties"] = json!([]);
    incoming["volume"] = Value::Null;
    document["sequences"][0]["transitions"] = json!([{"outgoing":outgoing_id,"incoming":incoming["id"],"range":{"start":{"num":"1","den":"20"},"end":{"num":"1","den":"10"}},"kind":"crossfade","version":1}]);
    document["sequences"][0]["tracks"][1]["clips"]
        .as_array_mut()
        .unwrap()
        .push(incoming);
    f.service(json!({"operation":"project.import","project":f.project,"base_revision":"1","document":document}));
    let query = f.service(json!({"operation":"sequence.query","project":f.project,"sequence":document["sequences"][0]["id"]}));
    let generator = query["clips"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["kind"] == "generator")
        .unwrap();
    assert!(generator["unsupported_reason"].is_null(), "{generator}");
    let profile = json!({"format":"pro_res_mov","profile_version":3,"audio":"document","clips":[],"background":[0,0,0]});
    let baseline = f.cli(
        json!({"operation":"render.export","render":render,"output":profile}),
        None,
        false,
    );
    let baseline_file = render["output_directory"].as_str().unwrap().to_owned();
    let gate = f.temp.path().join("audio-release");
    render["output_directory"] = json!(f.temp.path().join("job.mov"));
    let job = f.submit_request(
        json!({"operation":"render.submit","render":render,"output":profile}),
        Some(&gate),
        false,
    );
    f.wait(&job.id, JobStatus::Running);
    let fixed: Value = serde_json::from_slice(
        &std::fs::read(f.store().directory(&job.id).unwrap().join("input.json")).unwrap(),
    )
    .unwrap();
    use sha2::Digest;
    let bytes = std::fs::read(f.store().directory(&job.id).unwrap().join("input.json")).unwrap();
    assert_eq!(
        format!("{:x}", sha2::Sha256::digest(&bytes)),
        job.input_hash
    );
    let mut other_mode = fixed.clone();
    other_mode["request"]["output"]["audio"] = json!("silence");
    assert_ne!(
        sha2::Sha256::digest(serde_json::to_vec(&fixed).unwrap()),
        sha2::Sha256::digest(serde_json::to_vec(&other_mode).unwrap())
    );
    assert_eq!(fixed["request"]["output"]["audio"], "document");
    assert_eq!(fixed["request"]["output"]["profile_version"], 3);
    let mut changed = document.clone();
    changed["sequences"][0]["tracks"][1]["clips"][0]["volume"]["source"]["value"]["value"] =
        json!(0.0);
    changed["compositions"][0]["nodes"][0]["properties"][2]["source"]["value"]["value"]["components"]
        ["r"] = json!(0.0);
    f.service(json!({"operation":"project.import","project":f.project,"base_revision":"2","document":changed}));
    std::fs::remove_file(&f.project).unwrap();
    std::fs::write(gate, b"release").unwrap();
    let done = f.wait(&job.id, JobStatus::Succeeded);
    let report = &done.result.as_ref().unwrap()["report"];
    assert_eq!(report["frames"], baseline["frames"]);
    assert_eq!(
        report["export_snapshot_hash"],
        baseline["export_snapshot_hash"]
    );
    assert_eq!(report["sample_range"], json!({"start":0,"end":4804}));
    assert_eq!(report["audio"]["frames"], 4804);
    let runtime = kronello_media::MediaRuntime::load().unwrap();
    runtime
        .probe(&job.destination)
        .unwrap()
        .verify_av()
        .unwrap();
    let baseline_audio = runtime.decode_audio(Path::new(&baseline_file), 1).unwrap();
    let job_audio = runtime.decode_audio(&job.destination, 1).unwrap();
    assert_eq!(job_audio.buffer, baseline_audio.buffer);
    assert_eq!(report["audio_profile_version"], 3);
    let typed: kronello_model::Project = serde_json::from_value(document.clone()).unwrap();
    let sequence = serde_json::from_value(document["sequences"][0]["id"].clone()).unwrap();
    let plan = kronello_audio::DocumentAudioPlan::compile_version(
        &typed,
        kronello_audio::AudioTarget::Sequence(sequence),
        2,
    )
    .unwrap();
    let audio_clip = &plan.clips()[0];
    let audio_asset = typed
        .assets
        .iter()
        .find_map(|a| match a {
            kronello_model::DocumentObject::Known(a) if a.id == audio_clip.asset => Some(a),
            _ => None,
        })
        .unwrap();
    let source = runtime
        .decode_asset_audio(audio_asset, &f.project, audio_clip.stream_index)
        .unwrap()
        .buffer;
    let sources = [((audio_clip.asset, audio_clip.stream_index), source)].into();
    let range = serde_json::from_value(render["range"].clone()).unwrap();
    let bus = plan.mix(&sources, range).unwrap();
    let pcm = bus
        .quantize_pcm24(kronello_audio::ClippingPolicy::Reject)
        .unwrap();
    for (actual, expected) in job_audio.buffer.frames().iter().flatten().zip(pcm.samples) {
        assert_eq!(*actual, (expected / 256) as f32 / 8388608.0);
    }
    assert_eq!(job_audio.buffer.frames()[0], [0.03125, -0.03125]);
    let mut original_video = runtime.open_video(Path::new(&baseline_file)).unwrap();
    let mut fixed_video = runtime.open_video(&job.destination).unwrap();
    for frame in baseline["frames"].as_array().unwrap() {
        let time = serde_json::from_value(frame["time"].clone()).unwrap();
        assert_eq!(
            original_video.decode_at(time).unwrap().pixels,
            fixed_video.decode_at(time).unwrap().pixels
        );
    }
}

fn delivery_sync_fixed_job_and_alac_match_quantized_evaluator(
    format: &str,
    profile: kronello_media::MovieProfile,
) {
    let f = Fixture::new();
    let (document, mut render) = audio_sequence_fixture(&f, 0.5);
    render["input"]["region"]["pixels"] = json!([64, 64]);
    f.service(json!({"operation":"project.import","project":f.project,"base_revision":"1","document":document}));
    let extension = if format == "av1_mp4" { "mp4" } else { "mov" };
    let baseline_file = f.temp.path().join(format!("baseline.{extension}"));
    render["output_directory"] = json!(baseline_file);
    let output = json!({"format":format,"profile_version":1,"audio":"document","audio_codec":"alac","clips":[],"background":[0,0,0]});
    let baseline = f.cli(
        json!({"operation":"render.export","render":render,"output":output}),
        None,
        false,
    );
    let gate = f.temp.path().join("release-delivery");
    render["output_directory"] = json!(f.temp.path().join(format!("job.{extension}")));
    let job = f.submit_request(
        json!({"operation":"render.submit","render":render,"output":output}),
        Some(&gate),
        false,
    );
    f.wait(&job.id, JobStatus::Running);
    let input = std::fs::read(f.store().directory(&job.id).unwrap().join("input.json")).unwrap();
    assert_eq!(format!("{:x}", Sha256::digest(&input)), job.input_hash);
    let fixed: Value = serde_json::from_slice(&input).unwrap();
    assert_eq!(
        fixed["request"]["output"],
        serde_json::to_value(
            serde_json::from_value::<kronello_service::JobOutput>(output.clone()).unwrap()
        )
        .unwrap()
    );
    let mut changed_profile = fixed.clone();
    changed_profile["request"]["output"]["profile_version"] = json!(2);
    assert_ne!(
        Sha256::digest(&input),
        Sha256::digest(serde_json::to_vec(&changed_profile).unwrap())
    );
    let mut edited = document.clone();
    edited["sequences"][0]["tracks"][1]["clips"][0]["volume"]["source"]["value"]["value"] =
        json!(0);
    edited["compositions"][0]["nodes"][0]["properties"][2]["source"]["value"]["value"]["components"]
        ["r"] = json!(0);
    f.service(json!({"operation":"project.import","project":f.project,"base_revision":"2","document":edited}));
    std::fs::remove_file(&f.project).unwrap();
    std::fs::write(gate, b"release").unwrap();
    let done = f.wait(&job.id, JobStatus::Succeeded);
    let report = &done.result.as_ref().unwrap()["report"];
    assert_eq!(report["frames"], baseline["frames"]);
    assert_eq!(
        report["export_snapshot_hash"],
        baseline["export_snapshot_hash"]
    );
    assert_eq!(
        report["movie_profile"],
        serde_json::to_value(profile).unwrap()
    );
    assert_eq!(report["audio_profile_version"], 3);
    assert_eq!(report["sample_range"], json!({"start":0,"end":4804}));
    let runtime = kronello_media::MediaRuntime::load().unwrap();
    runtime
        .probe(&job.destination)
        .unwrap()
        .verify_movie(profile)
        .unwrap();
    let original = runtime.decode_audio(&baseline_file, 1).unwrap();
    let decoded = runtime.decode_audio(&job.destination, 1).unwrap();
    assert_eq!(
        serde_json::to_value(decoded.source_start).unwrap(),
        json!({"num":"0","den":"1"})
    );
    assert_eq!(decoded.buffer, original.buffer);
    assert_eq!(decoded.buffer.frames().len(), 4804);
    let typed: kronello_model::Project = serde_json::from_value(document.clone()).unwrap();
    let sequence = serde_json::from_value(document["sequences"][0]["id"].clone()).unwrap();
    let plan = kronello_audio::DocumentAudioPlan::compile_version(
        &typed,
        kronello_audio::AudioTarget::Sequence(sequence),
        2,
    )
    .unwrap();
    let clip = &plan.clips()[0];
    let asset = typed
        .assets
        .iter()
        .find_map(|a| match a {
            kronello_model::DocumentObject::Known(a) if a.id == clip.asset => Some(a),
            _ => None,
        })
        .unwrap();
    let source = runtime
        .decode_asset_audio(asset, &f.project, clip.stream_index)
        .unwrap()
        .buffer;
    let bus = plan
        .mix(
            &[((clip.asset, clip.stream_index), source)].into(),
            serde_json::from_value(render["range"].clone()).unwrap(),
        )
        .unwrap();
    let pcm = bus
        .quantize_pcm24(kronello_audio::ClippingPolicy::Reject)
        .unwrap();
    for (actual, expected) in decoded.buffer.frames().iter().flatten().zip(pcm.samples) {
        assert_eq!(
            actual.to_bits(),
            ((expected / 256) as f32 / 8388608.0).to_bits()
        );
    }
    let mut original_video = runtime.open_video(&baseline_file).unwrap();
    let mut fixed_video = runtime.open_video(&job.destination).unwrap();
    for i in 0..3 {
        let time =
            serde_json::from_value(json!({"num":(i * 1001).to_string(),"den":"30000"})).unwrap();
        let original = original_video.decode_at(time).unwrap();
        let fixed = fixed_video.decode_at(time).unwrap();
        assert_eq!(fixed.pts, time);
        let end = serde_json::from_value(json!({"num":((i + 1) * 1001).to_string(),"den":"30000"}))
            .unwrap();
        assert_eq!(fixed.end, end);
        assert_eq!(fixed.pixels, original.pixels);
    }
    // A destination created after submission remains unchanged at publication.
    let f = Fixture::new();
    let (document, mut render) = audio_sequence_fixture(&f, 0.5);
    render["input"]["region"]["pixels"] = json!([64, 64]);
    f.service(json!({"operation":"project.import","project":f.project,"base_revision":"1","document":document}));
    render["output_directory"] = json!(f.temp.path().join(format!("race.{extension}")));
    let gate = f.temp.path().join("release-race");
    let job = f.submit_request(
        json!({"operation":"render.submit","render":render,"output":output}),
        Some(&gate),
        false,
    );
    f.wait(&job.id, JobStatus::Running);
    std::fs::write(&job.destination, b"existing delivery").unwrap();
    std::fs::write(gate, b"release").unwrap();
    let done = f.wait(&job.id, JobStatus::Failed);
    assert_eq!(done.error.unwrap().code, "OUTPUT_EXISTS");
    assert_eq!(
        std::fs::read(job.destination).unwrap(),
        b"existing delivery"
    );
    println!("{}", serde_json::to_string(report).unwrap());
}
#[test]
fn av1_delivery_sync_fixed_job_and_alac_match_quantized_evaluator() {
    delivery_sync_fixed_job_and_alac_match_quantized_evaluator(
        "av1_mp4",
        kronello_media::MovieProfile::Av1Mp4AlacV1,
    );
}
#[test]
#[ignore = "pending host run: requires physical VideoToolbox H.264 encoder"]
fn host_h264_delivery_sync_fixed_job_and_alac_match_quantized_evaluator() {
    delivery_sync_fixed_job_and_alac_match_quantized_evaluator(
        "h264_mov",
        kronello_media::MovieProfile::H264AlacV1,
    );
}
#[test]
#[ignore = "pending host run: requires physical VideoToolbox HEVC encoder"]
fn host_hevc_delivery_sync_fixed_job_and_alac_match_quantized_evaluator() {
    delivery_sync_fixed_job_and_alac_match_quantized_evaluator(
        "hevc_mov",
        kronello_media::MovieProfile::HevcAlacV1,
    );
}

#[test]
fn inspected_revision_fence_rejects_submit_and_export_without_job_or_worker() {
    let f = Fixture::new();
    // A missing font would otherwise fail synchronous export before its fence.
    let full_document: Value =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    f.service(json!({"operation":"project.import","project":f.project,"base_revision":"1","document":full_document}));
    for operation in ["render.submit", "render.export"] {
        let request = json!({"operation":operation,"expected_revision":"0", "render":f.render(&f.temp.path().join("fenced.mov")), "output":{"format":"pro_res_mov","profile_version":3,"audio":"silence","clips":[],"background":[0,0,0]}});
        let mut command = f.command();
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(request.to_string().as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["error"]["code"], "REVISION_CONFLICT", "{response}");
        assert!(f.store().list().unwrap().is_empty());
        assert!(!f.temp.path().join("fenced.mov").exists());
        let response = Service::new(BackendSelection::CpuReference)
            .with_job_config(JobConfig::at(&f.state))
            .execute_json(&request.to_string());
        let response = serde_json::to_value(response).unwrap();
        assert_eq!(response["error"]["code"], "REVISION_CONFLICT");
        assert!(f.store().list().unwrap().is_empty());
    }
}
