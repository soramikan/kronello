//! FLOW-004 capture/ingest (ADR-0135) end-to-end: a real `capture.start`
//! detached worker records the deterministic synthetic source, `capture.stop`
//! finalizes provisional frames into a hashed/registered asset, and
//! `capture.status` reports sessions plus typed orphans. Recovery publishes a
//! killed worker's spool through `job.resume`.
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use kronello_jobs::{JobConfig, JobRecord, JobStatus, JobStore};
use serde_json::{Value as Json, json};

struct Fixture {
    cleanup: kronello_jobs::test_support::WorkerCleanup,
    temp: tempfile::TempDir,
    project: PathBuf,
    state: PathBuf,
    heartbeat_timeout: Duration,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("clip.kronello");
        let state = temp.path().join("state");
        let fixture = Self {
            cleanup: kronello_jobs::test_support::WorkerCleanup::new(&state).unwrap(),
            temp,
            project,
            state,
            heartbeat_timeout: Duration::from_secs(30),
        };
        let document: Json = serde_json::to_value(kronello_model::Project::default()).unwrap();
        fixture.cli(
            json!({"operation":"project.create","project":fixture.project,
            "document":document}),
        );
        fixture
    }
    fn command(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_kronello"));
        c.args(["--backend", "cpu-reference"])
            .env("KRONELLO_STATE_ROOT", &self.state)
            .env("KRONELLO_JOB_HEARTBEAT_MS", "50")
            .env(
                "KRONELLO_JOB_TIMEOUT_MS",
                self.heartbeat_timeout.as_millis().to_string(),
            );
        c
    }
    /// Full JSON response including errors; `cli` below asserts success.
    fn cli_response(&self, verbs: &[&str], request: Json) -> Json {
        let mut command = self.command();
        command.args(verbs);
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
        self.cleanup.capture_registered().unwrap();
        let response: Json = serde_json::from_slice(&output.stdout).unwrap();
        assert!(
            output.status.success() || response["status"] == "error",
            "verbs={verbs:?} stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        response
    }
    fn cli(&self, request: Json) -> Json {
        self.cli_args(&[], request)
    }
    /// CLI verb tokens inject the operation tag; payloads stay untagged.
    fn cli_args(&self, verbs: &[&str], request: Json) -> Json {
        let response = self.cli_response(verbs, request);
        assert_eq!(response["status"], "success", "{response}");
        response["result"]["value"].clone()
    }
    fn cli_error(&self, verbs: &[&str], request: Json) -> Json {
        let response = self.cli_response(verbs, request);
        assert_eq!(response["status"], "error", "{response}");
        response["error"].clone()
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
            let record = self.store().get(id).unwrap();
            if record.status == status {
                return record;
            }
            assert!(
                record.status.active(),
                "unexpected terminal record {record:?}; log: {}",
                std::fs::read_to_string(self.store().directory(id).unwrap().join("worker.log"))
                    .unwrap_or_default()
            );
            assert!(
                start.elapsed() < Duration::from_secs(60),
                "timed out: {record:?}; log: {}",
                std::fs::read_to_string(self.store().directory(id).unwrap().join("worker.log"))
                    .unwrap_or_default()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn wait_frames(&self, id: &str, frames: u64) {
        let start = Instant::now();
        while self.store().get(id).unwrap().completed_frames < frames {
            assert!(
                start.elapsed() < Duration::from_secs(30),
                "job {id} stalled"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    /// The managed capture directory `<stem>.capture/` beside the project.
    /// Canonicalized because the service stores canonical destinations (the
    /// macOS tempdir is `/var` → `/private/var`).
    fn capture_dir(&self) -> PathBuf {
        let dir = self.temp.path().join("clip.capture");
        if dir.exists() {
            dir.canonicalize().unwrap()
        } else {
            self.temp
                .path()
                .canonicalize()
                .unwrap()
                .join("clip.capture")
        }
    }
    fn synthetic_request(&self) -> Json {
        json!({
            "project": self.project,
            "source": {"kind": "synthetic"},
            "format": {
                "width": 16,
                "height": 8,
                "frame_rate": {"num": "24", "den": "1"},
                "codec": "pro_res",
                "color": "bt709",
            },
        })
    }
    /// `capture.start` through the `capture start` CLI verbs.
    fn start(&self, request: Json) -> JobRecord {
        serde_json::from_value(self.cli_args(&["capture", "start"], request)).unwrap()
    }
    /// `capture.status` payload for the project.
    fn status(&self) -> Json {
        self.cli_args(&["capture", "status"], json!({"project": self.project}))
    }
    fn expire_terminated_worker(&self, record: &JobRecord) {
        assert!(!kronello_platform::process_is_alive(
            record.worker_pid.unwrap()
        ));
        let mut db = rusqlite::Connection::open(self.state.join("jobs.sqlite3")).unwrap();
        let tx = db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .unwrap();
        let text: String = tx
            .query_row("SELECT record FROM jobs WHERE id=?1", [&record.id], |r| {
                r.get(0)
            })
            .unwrap();
        let mut record: JobRecord = serde_json::from_str(&text).unwrap();
        record.heartbeat_at_ms =
            kronello_jobs::now_ms() - self.heartbeat_timeout.as_millis() as i64 - 1000;
        tx.execute(
            "UPDATE jobs SET record=?1 WHERE id=?2",
            rusqlite::params![serde_json::to_string(&record).unwrap(), record.id],
        )
        .unwrap();
        tx.commit().unwrap();
    }
}

fn exported_asset(fixture: &Fixture, asset: &str) -> Json {
    let exported = fixture.cli(json!({"operation":"project.export","project":fixture.project}));
    exported["document"]["assets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == *asset)
        .cloned()
        .expect("capture asset registered")
}

#[test]
fn synthetic_capture_publishes_registered_asset() {
    let fixture = Fixture::new();
    let mut request = fixture.synthetic_request();
    request["max_frames"] = json!(6);
    let job = fixture.start(request);
    assert!(job.status.active(), "submitted job runs detached: {job:?}");
    // The destination is a managed `<stem>.capture/<asset>.mov` file.
    let asset_id = job.output_profile["capture_asset_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        job.destination,
        fixture.capture_dir().join(format!("{asset_id}.mov"))
    );
    assert!(!job.destination.exists(), "nothing exists before finalize");
    let record = fixture.wait(&job.id, JobStatus::Succeeded);
    // The authoritative frame count is discovered at stop and committed.
    assert_eq!(record.total_frames, 6);
    assert_eq!(record.completed_frames, 6);
    assert!(record.destination.is_file());
    let report = &record.result.as_ref().unwrap()["report"];
    assert_eq!(report["frames"], 6);
    // The registered asset mirrors exactly the published bytes.
    let asset = exported_asset(&fixture, &asset_id);
    assert_eq!(asset["kind"], "video");
    assert_eq!(
        asset["content_hash"].as_str().unwrap(),
        kronello_media::content_hash(&record.destination).unwrap()
    );
    assert_eq!(asset["content_hash"], report["content_hash"]);
    // Container probe agrees with the recorded format.
    let probe = kronello_media::MediaRuntime::load()
        .unwrap()
        .probe(&record.destination)
        .unwrap();
    let video = probe
        .streams
        .iter()
        .find(|s| s.kind == kronello_media::StreamKind::Video)
        .unwrap();
    assert_eq!((video.width, video.height), (Some(16), Some(8)));
    // 6 frames at 24 fps is the normalized duration 1/4 s.
    let duration = video.duration.unwrap();
    assert_eq!(duration.numerator() * 24, 6 * duration.denominator());
    // Status reports the finished session, no live spool, no orphans.
    let status = fixture.status();
    let sessions = status["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["job"]["id"], job.id);
    assert_eq!(sessions[0]["asset_registered"], true);
    assert_eq!(sessions[0]["asset"], json!(asset_id));
    assert!(sessions[0]["provisional"].is_null());
    assert_eq!(sessions[0]["stop_requested"], false);
    assert_eq!(status["orphans"].as_array().unwrap().len(), 0);
}

#[test]
fn capture_stop_publishes_partial_recording() {
    let fixture = Fixture::new();
    let mut request = fixture.synthetic_request();
    request["format"]["frame_rate"] = json!({"num": "240", "den": "1"});
    let job = fixture.start(request);
    fixture.wait_frames(&job.id, 3);
    let stopped: JobRecord = serde_json::from_value(fixture.cli_args(
        &["capture", "stop"],
        json!({
            "project": fixture.project, "job": job.id,
        }),
    ))
    .unwrap();
    assert_eq!(stopped.id, job.id);
    let record = fixture.wait(&job.id, JobStatus::Succeeded);
    let frames = record.total_frames;
    assert!(frames >= 3, "recorded frames survive a graceful stop");
    assert!(record.destination.is_file());
    let status = fixture.status();
    assert_eq!(status["sessions"][0]["job"]["status"], "succeeded");
    assert!(status["orphans"].as_array().unwrap().is_empty());
}

#[test]
fn capture_stop_rejects_foreign_jobs() {
    let fixture = Fixture::new();
    let error = fixture.cli_error(
        &["capture", "stop"],
        json!({"project": fixture.project, "job": uuid::Uuid::new_v4().to_string()}),
    );
    assert_eq!(error["code"], "JOB_NOT_FOUND");
    let mut request = fixture.synthetic_request();
    request["max_frames"] = json!(2);
    let job = fixture.start(request);
    fixture.wait(&job.id, JobStatus::Succeeded);
    // A second stop on a terminal session is an idempotent no-op.
    fixture.cli_args(
        &["capture", "stop"],
        json!({"project": fixture.project, "job": job.id}),
    );
}

#[test]
fn capture_status_reports_orphans_for_canceled_and_unknown_sessions() {
    let fixture = Fixture::new();
    // A canceled session leaves its provisional spool as a typed orphan.
    let mut request = fixture.synthetic_request();
    request["format"]["frame_rate"] = json!({"num": "240", "den": "1"});
    let job = fixture.start(request);
    fixture.wait_frames(&job.id, 2);
    fixture.cli_args(&["job", "cancel"], json!({"job": job.id}));
    fixture.wait(&job.id, JobStatus::Canceled);
    // A spool with no job record is an orphan with no session reference.
    let stranger = uuid::Uuid::new_v4().to_string();
    std::fs::write(
        fixture
            .capture_dir()
            .join(format!("{stranger}.provisional")),
        vec![0u8; 16 * 8 * 4],
    )
    .unwrap();
    // Non-provisional files in the capture area are never orphans.
    std::fs::write(fixture.capture_dir().join("notes.txt"), b"unrelated").unwrap();
    let status = fixture.status();
    let orphans = status["orphans"].as_array().unwrap();
    let canceled = orphans
        .iter()
        .find(|o| o["job"] == json!(job.id))
        .expect("canceled spool is an orphan");
    assert_eq!(canceled["job_status"], "canceled");
    assert!(canceled["bytes"].as_u64().unwrap() >= 2 * 16 * 8 * 4);
    assert!(
        canceled["provisional"]
            .as_str()
            .unwrap()
            .ends_with(".provisional")
    );
    let unknown = orphans
        .iter()
        .find(|o| o["job"].is_null() || o["job"] == json!(stranger))
        .expect("unrecorded spool is an orphan");
    assert_eq!(unknown["bytes"], 16 * 8 * 4);
    assert_eq!(orphans.len(), 2);
}

#[test]
fn capture_deck_and_start_are_typed_unsupported() {
    let fixture = Fixture::new();
    let error = fixture.cli_error(&["capture", "deck_probe"], json!({}));
    assert_eq!(error["code"], "UNSUPPORTED_FEATURE");
    let mut request = fixture.synthetic_request();
    request["source"] = json!({"kind": "deck", "device": "decklink"});
    let error = fixture.cli_error(&["capture", "start"], request);
    assert_eq!(error["code"], "UNSUPPORTED_FEATURE");
    let mut request = fixture.synthetic_request();
    request["source"] = json!({"kind": "deck", "device": "rs422", "device_id": "deck-0"});
    let error = fixture.cli_error(&["capture", "start"], request);
    assert_eq!(error["code"], "UNSUPPORTED_FEATURE");
}

#[test]
fn capture_start_validates_and_replays_idempotent_key() {
    let fixture = Fixture::new();
    // Odd dimensions and a zero frame bound are typed validation failures.
    let mut request = fixture.synthetic_request();
    request["format"]["width"] = json!(15);
    let error = fixture.cli_error(&["capture", "start"], request.clone());
    assert_eq!(error["code"], "INVALID_REQUEST");
    let mut request = fixture.synthetic_request();
    request["max_frames"] = json!(0);
    let error = fixture.cli_error(&["capture", "start"], request);
    assert_eq!(error["code"], "INVALID_REQUEST");
    let mut request = fixture.synthetic_request();
    request["format"]["frame_rate"] = json!({"num": "0", "den": "1"});
    assert_eq!(
        fixture.cli_error(&["capture", "start"], request)["code"],
        "INVALID_REQUEST"
    );
    // A keyed start replays the recorded job instead of recording twice.
    let mut request = fixture.synthetic_request();
    request["max_frames"] = json!(2);
    request["idempotency_key"] = json!("capture-test-1");
    let first = fixture.start(request.clone());
    let replay = fixture.start(request.clone());
    assert_eq!(replay.id, first.id);
    // The same key with different content is a typed conflict.
    request["max_frames"] = json!(4);
    let error = fixture.cli_error(&["capture", "start"], request);
    assert_eq!(error["code"], "IDEMPOTENCY_KEY_REUSED");
    fixture.wait(&first.id, JobStatus::Succeeded);
}

#[test]
fn interrupted_capture_resume_publishes_spool() {
    let fixture = Fixture::new();
    let job = fixture.start(fixture.synthetic_request());
    let running = fixture.wait(&job.id, JobStatus::Running);
    fixture.wait_frames(&job.id, 2);
    let provisional = fixture
        .capture_dir()
        .join(format!("{}.provisional", job.id));
    assert!(provisional.is_file(), "worker spools provisional frames");
    let spool_bytes = std::fs::metadata(&provisional).unwrap().len();
    assert!(spool_bytes >= 2 * 16 * 8 * 4);
    // A dead worker leaves the spool; recovery marks the session interrupted.
    kronello_platform::ProcessGuard::capture(running.worker_pid.unwrap())
        .unwrap()
        .terminate_and_wait()
        .unwrap();
    fixture.expire_terminated_worker(&running);
    fixture.wait(&job.id, JobStatus::Interrupted);
    let status = fixture.status();
    let orphan = status["orphans"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["job"] == json!(job.id))
        .expect("dead worker's spool is an orphan");
    assert_eq!(orphan["job_status"], "interrupted");
    // Resuming the session finalizes the already-recorded frames: a live
    // source cannot be rewound, so the spool — not a fresh capture — is the
    // recoverable record.
    let resumed: JobRecord =
        serde_json::from_value(fixture.cli_args(&["job", "resume"], json!({"job": job.id})))
            .unwrap();
    assert_eq!(resumed.attempt, 1);
    let finished = fixture.wait(&job.id, JobStatus::Succeeded);
    let frames = finished.total_frames;
    assert_eq!(frames, spool_bytes / (16 * 8 * 4));
    assert!(finished.destination.is_file());
    let asset = exported_asset(
        &fixture,
        finished.output_profile["capture_asset_id"]
            .as_str()
            .unwrap(),
    );
    assert_eq!(
        asset["content_hash"].as_str().unwrap(),
        kronello_media::content_hash(&finished.destination).unwrap()
    );
    // Publication consumed the provisional spool; no orphans remain.
    let status = fixture.status();
    assert!(status["orphans"].as_array().unwrap().is_empty());
    assert_eq!(status["sessions"][0]["job"]["status"], "succeeded");
}

#[test]
fn synthetic_source_output_is_deterministic() {
    let fixture = Fixture::new();
    let mut hashes = Vec::new();
    for _ in 0..2 {
        let mut request = fixture.synthetic_request();
        request["max_frames"] = json!(3);
        let job = fixture.start(request);
        let record = fixture.wait(&job.id, JobStatus::Succeeded);
        hashes.push(record.result.as_ref().unwrap()["report"]["content_hash"].clone());
    }
    assert_eq!(
        hashes[0], hashes[1],
        "identical synthetic sessions hash identically"
    );
}

#[test]
fn capture_requests_use_local_locators() {
    let fixture = Fixture::new();
    let error = fixture.cli_error(
        &["capture", "start"],
        json!({"project": "https://example.invalid/x.kronello",
            "source": {"kind": "synthetic"},
            "format": {"width": 16, "height": 8,
                "frame_rate": {"num": "24", "den": "1"},
                "codec": "pro_res", "color": "bt709"}}),
    );
    assert_eq!(error["code"], "INVALID_REQUEST");
}
