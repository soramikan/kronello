//! Per-user execution state. No project database or evaluation dependency.
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

mod connection_gate;
mod heartbeat;
pub use heartbeat::WorkerHeartbeat;

// SQLite 3.51.1 unix VFS can invert its global/inode mutexes when one
// connection closes WAL while another thread opens the same DB. Hold this
// process-local gate through all SQLite access and connection destruction.
// Separate processes still coordinate through SQLite's normal file locks.
static CONNECTION_LIFETIME: connection_gate::ConnectionGate =
    connection_gate::ConnectionGate::new();
struct JobConnection {
    // Field order matters: Connection closes before the gate is released.
    db: Connection,
    _lifetime: connection_gate::ConnectionLease<'static>,
}
impl std::ops::Deref for JobConnection {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        &self.db
    }
}
impl std::ops::DerefMut for JobConnection {
    fn deref_mut(&mut self) -> &mut Connection {
        &mut self.db
    }
}

#[cfg(feature = "test-support")]
pub mod test_support;

#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error("{code}: {message}")]
    Typed { code: String, message: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
impl JobError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self::Typed {
            code: code.into(),
            message: message.into(),
        }
    }
    pub fn code(&self) -> &str {
        match self {
            Self::Typed { code, .. } => code,
            Self::Io(_) => "IO_ERROR",
            Self::Sqlite(_) => "JOB_STORAGE_ERROR",
            Self::Json(_) => "INVALID_JOB_INPUT",
        }
    }
    pub fn is_retryable_heartbeat(&self) -> bool {
        self.is_retryable_contention()
    }
    pub fn is_retryable_contention(&self) -> bool {
        if matches!(self, Self::Typed { code, .. } if code == "JOB_PROCESS_BUSY") {
            return true;
        }
        matches!(self, Self::Sqlite(rusqlite::Error::SqliteFailure(error, _))
            if matches!(error.code, rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked))
    }
}
#[derive(Debug, Clone)]
pub struct JobConfig {
    pub state_root: PathBuf,
    pub slots: usize,
    pub heartbeat_interval: Duration,
    pub heartbeat_timeout: Duration,
    pub retention: Duration,
}
impl JobConfig {
    pub fn at(state_root: impl Into<PathBuf>) -> Self {
        Self {
            state_root: state_root.into(),
            slots: 1,
            heartbeat_interval: Duration::from_secs(1),
            heartbeat_timeout: Duration::from_secs(30),
            retention: Duration::from_secs(30 * 86400),
        }
    }
    pub fn from_env() -> Result<Self, JobError> {
        let root = match std::env::var_os("KRONELLO_STATE_ROOT") {
            Some(root) => PathBuf::from(root),
            None => {
                let base = directories::BaseDirs::new().ok_or_else(|| {
                    JobError::new("JOB_CONFIG_ERROR", "user state directory unavailable")
                })?;
                #[cfg(target_os = "macos")]
                let path = base.home_dir().join("Library/Application Support/Kronello");
                #[cfg(not(target_os = "macos"))]
                let path = base.data_local_dir().join("Kronello");
                path
            }
        };
        let mut config = Self::at(root);
        fn number(name: &str, default: u64) -> Result<u64, JobError> {
            match std::env::var(name) {
                Ok(s) => s
                    .parse()
                    .map_err(|_| JobError::new("JOB_CONFIG_ERROR", format!("invalid {name}"))),
                Err(std::env::VarError::NotPresent) => Ok(default),
                Err(e) => Err(JobError::new("JOB_CONFIG_ERROR", e.to_string())),
            }
        }
        config.slots = usize::try_from(number("KRONELLO_JOB_SLOTS", 1)?)
            .map_err(|_| JobError::new("JOB_CONFIG_ERROR", "slots overflow"))?;
        config.heartbeat_interval =
            Duration::from_millis(number("KRONELLO_JOB_HEARTBEAT_MS", 1000)?);
        config.heartbeat_timeout = Duration::from_millis(number("KRONELLO_JOB_TIMEOUT_MS", 30000)?);
        config.retention =
            Duration::from_secs(number("KRONELLO_JOB_RETENTION_SECONDS", 30 * 86400)?);
        config.validate()?;
        Ok(config)
    }
    fn validate(&self) -> Result<(), JobError> {
        if self.state_root.as_os_str().is_empty()
            || self.slots == 0
            || self.heartbeat_interval.is_zero()
            || self.heartbeat_timeout <= self.heartbeat_interval
        {
            return Err(JobError::new(
                "JOB_CONFIG_ERROR",
                "root, slots and heartbeat limits are invalid",
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Canceled,
    Interrupted,
}
impl JobStatus {
    pub fn active(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JobFailure {
    pub code: String,
    pub message: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JobRecord {
    pub id: String,
    pub submitted_at_ms: i64,
    pub engine_version: String,
    pub project_id: String,
    pub revision: String,
    pub snapshot_hash: String,
    pub input_hash: String,
    pub output_profile: serde_json::Value,
    pub destination: PathBuf,
    pub status: JobStatus,
    pub completed_frames: u64,
    pub total_frames: u64,
    pub heartbeat_at_ms: i64,
    pub worker_pid: Option<u32>,
    pub cancel_requested: bool,
    pub finished_at_ms: Option<i64>,
    pub result: Option<serde_json::Value>,
    pub error: Option<JobFailure>,
    pub directory_pruned: bool,
}
pub struct Submission {
    pub engine_version: String,
    pub project_id: String,
    pub revision: String,
    pub snapshot_hash: String,
    pub output_profile: serde_json::Value,
    pub destination: PathBuf,
    pub total_frames: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PruneResult {
    pub pruned: Vec<String>,
}
#[derive(Debug, Clone)]
pub struct JobStore {
    config: JobConfig,
}
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}
fn duration_ms(d: Duration) -> i64 {
    d.as_millis().min(i64::MAX as u128) as i64
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn records(db: &Connection) -> Result<Vec<JobRecord>, JobError> {
    let mut stmt = db.prepare("SELECT record FROM jobs ORDER BY seq")?;
    stmt.query_map([], |row| row.get::<_, String>(0))?
        .map(|s| Ok(serde_json::from_str(&s?)?))
        .collect()
}
fn read(db: &Connection, id: &str) -> Result<JobRecord, JobError> {
    let text: Option<String> = db
        .query_row("SELECT record FROM jobs WHERE id=?1", [id], |r| r.get(0))
        .optional()?;
    Ok(serde_json::from_str(
        &text.ok_or_else(|| JobError::new("JOB_NOT_FOUND", id))?,
    )?)
}
fn save(db: &Connection, record: &JobRecord) -> Result<(), JobError> {
    db.execute(
        "UPDATE jobs SET record=?1 WHERE id=?2",
        params![serde_json::to_string(record)?, record.id],
    )?;
    Ok(())
}
fn recover(db: &Connection, timeout: Duration) -> Result<(), JobError> {
    let now = now_ms();
    for mut r in records(db)? {
        if r.status.active()
            && now.saturating_sub(r.heartbeat_at_ms) > duration_ms(timeout)
            && !r.worker_pid.is_some_and(worker_is_alive)
        {
            r.status = JobStatus::Interrupted;
            r.finished_at_ms = Some(now);
            r.error = Some(JobFailure {
                code: "JOB_INTERRUPTED".into(),
                message: "worker heartbeat expired".into(),
            });
            save(db, &r)?;
        }
    }
    Ok(())
}
fn worker_is_alive(pid: u32) -> bool {
    kronello_platform::process_is_alive(pid)
}
impl JobStore {
    pub fn open(config: JobConfig) -> Result<Self, JobError> {
        config.validate()?;
        std::fs::create_dir_all(config.state_root.join("jobs"))?;
        let store = Self { config };
        let db = store.connect()?;
        let version: u32 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version == 0 {
            db.execute_batch("PRAGMA journal_mode=WAL; BEGIN IMMEDIATE; CREATE TABLE IF NOT EXISTS jobs(seq INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT NOT NULL UNIQUE, record TEXT NOT NULL); PRAGMA user_version=1; COMMIT;")?;
        }
        Ok(store)
    }
    pub fn config(&self) -> &JobConfig {
        &self.config
    }
    fn connect(&self) -> Result<JobConnection, JobError> {
        self.connect_with_timeout(Duration::from_secs(5))
    }
    fn connect_with_timeout(&self, timeout: Duration) -> Result<JobConnection, JobError> {
        let lifetime = CONNECTION_LIFETIME.acquire(timeout)?;
        let db = Connection::open(self.config.state_root.join("jobs.sqlite3"))?;
        db.busy_timeout(timeout)?;
        db.execute_batch("PRAGMA synchronous=FULL;")?;
        let version: u32 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > 1 {
            return Err(JobError::new("UNSUPPORTED_SCHEMA_VERSION", "job database"));
        }
        Ok(JobConnection {
            db,
            _lifetime: lifetime,
        })
    }
    pub fn directory(&self, id: &str) -> Result<PathBuf, JobError> {
        let parsed = uuid::Uuid::parse_str(id)
            .map_err(|_| JobError::new("INVALID_REQUEST", "job ID must be UUID"))?;
        if parsed.to_string() != id {
            return Err(JobError::new(
                "INVALID_REQUEST",
                "job ID must be canonical UUID",
            ));
        }
        Ok(self.config.state_root.join("jobs").join(id))
    }
    pub fn submit(&self, input: &[u8], submission: Submission) -> Result<JobRecord, JobError> {
        self.prune()?;
        let id = uuid::Uuid::new_v4().to_string();
        let directory = self.directory(&id)?;
        std::fs::create_dir(&directory)?;
        let result = (|| {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join("input.json"))?;
            file.write_all(input)?;
            file.sync_all()?;
            let now = now_ms();
            let r = JobRecord {
                id,
                submitted_at_ms: now,
                engine_version: submission.engine_version,
                project_id: submission.project_id,
                revision: submission.revision,
                snapshot_hash: submission.snapshot_hash,
                input_hash: hash(input),
                output_profile: submission.output_profile,
                destination: submission.destination,
                status: JobStatus::Queued,
                completed_frames: 0,
                total_frames: submission.total_frames,
                heartbeat_at_ms: now,
                worker_pid: None,
                cancel_requested: false,
                finished_at_ms: None,
                result: None,
                error: None,
                directory_pruned: false,
            };
            self.connect()?.execute(
                "INSERT INTO jobs(id,record) VALUES(?1,?2)",
                params![r.id, serde_json::to_string(&r)?],
            )?;
            Ok(r)
        })();
        if result.is_err() {
            let _ = std::fs::remove_dir_all(directory);
        }
        result
    }
    pub fn input(&self, r: &JobRecord) -> Result<Vec<u8>, JobError> {
        let bytes = std::fs::read(self.directory(&r.id)?.join("input.json"))?;
        if hash(&bytes) != r.input_hash {
            return Err(JobError::new(
                "JOB_INPUT_HASH_MISMATCH",
                "fixed input bytes changed",
            ));
        }
        Ok(bytes)
    }
    pub fn list(&self) -> Result<Vec<JobRecord>, JobError> {
        let mut db = self.connect()?;
        // Most polling is a WAL read, not a competing writer. Recheck recovery
        // inside the writer transaction only when a snapshot contains expiry.
        let snapshot = records(&db)?;
        if !snapshot.iter().any(|r| {
            r.status.active()
                && now_ms().saturating_sub(r.heartbeat_at_ms)
                    > duration_ms(self.config.heartbeat_timeout)
                && !r.worker_pid.is_some_and(worker_is_alive)
        }) {
            return Ok(snapshot);
        }
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        recover(&tx, self.config.heartbeat_timeout)?;
        let list = records(&tx)?;
        tx.commit()?;
        Ok(list)
    }
    pub fn get(&self, id: &str) -> Result<JobRecord, JobError> {
        self.directory(id)?;
        self.list()?
            .into_iter()
            .find(|r| r.id == id)
            .ok_or_else(|| JobError::new("JOB_NOT_FOUND", id))
    }
    fn update(
        &self,
        id: &str,
        f: impl FnOnce(&mut JobRecord) -> Result<(), JobError>,
    ) -> Result<JobRecord, JobError> {
        self.update_with_timeout(id, Duration::from_secs(5), f)
    }
    fn update_with_timeout(
        &self,
        id: &str,
        timeout: Duration,
        f: impl FnOnce(&mut JobRecord) -> Result<(), JobError>,
    ) -> Result<JobRecord, JobError> {
        let mut db = self.connect_with_timeout(timeout)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut r = read(&tx, id)?;
        f(&mut r)?;
        save(&tx, &r)?;
        tx.commit()?;
        Ok(r)
    }
    pub fn cancel(&self, id: &str) -> Result<JobRecord, JobError> {
        self.get(id)?;
        self.update(id, |r| {
            if r.status.active() {
                r.cancel_requested = true;
            }
            Ok(())
        })
    }
    pub fn heartbeat(&self, id: &str) -> Result<(), JobError> {
        // Apply the bound before any SQLite operation, including connection setup.
        // A busy writer must not park this thread for the normal five seconds.
        let mut db = self.connect_with_timeout(self.worker_lock_budget())?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut r = read(&tx, id)?;
        if !r.status.active() || r.worker_pid.is_some_and(|pid| pid != std::process::id()) {
            return Err(JobError::new(
                "JOB_INTERRUPTED",
                "worker no longer owns job",
            ));
        }
        r.heartbeat_at_ms = now_ms();
        save(&tx, &r)?;
        tx.commit()?;
        Ok(())
    }
    pub fn claim(&self, id: &str) -> Result<bool, JobError> {
        let mut db = self.connect_with_timeout(self.worker_lock_budget())?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        recover(&tx, self.config.heartbeat_timeout)?;
        let mut r = read(&tx, id)?;
        if !r.status.active() {
            tx.commit()?;
            return Err(JobError::new("JOB_INTERRUPTED", "job is terminal"));
        }
        if r.worker_pid.is_some_and(|pid| pid != std::process::id()) {
            tx.commit()?;
            return Err(JobError::new(
                "JOB_ALREADY_OWNED",
                "another worker owns job",
            ));
        }
        let ownership_changed = r.worker_pid.is_none();
        r.worker_pid = Some(std::process::id());
        if r.cancel_requested {
            save(&tx, &r)?;
            tx.commit()?;
            return Err(JobError::new("JOB_CANCELED", "cancel requested"));
        }
        let all = records(&tx)?;
        let running = all
            .iter()
            .filter(|r| r.status == JobStatus::Running)
            .count();
        let rank = all
            .iter()
            .filter(|r| r.status == JobStatus::Queued)
            .position(|r| r.id == id)
            .unwrap_or(usize::MAX);
        let acquired =
            r.status == JobStatus::Queued && rank < self.config.slots.saturating_sub(running);
        if acquired {
            r.status = JobStatus::Running;
        }
        // A queued worker already has a dedicated heartbeat pulse. Polling
        // the occupied slots must not add a FULL synchronous write every
        // 50 ms, starving submit/cancel writers on slower filesystems.
        if acquired || ownership_changed {
            r.heartbeat_at_ms = now_ms();
            save(&tx, &r)?;
        }
        tx.commit()?;
        Ok(acquired)
    }
    fn worker_lock_budget(&self) -> Duration {
        Duration::from_millis(100)
            .min(self.config.heartbeat_interval)
            .min(self.config.heartbeat_timeout / 4)
    }
    /// Contention is a scheduler retry, never a terminal job failure. Ownership,
    /// cancellation and terminal errors are still returned without retrying.
    pub fn wait_for_slot(&self, id: &str) -> Result<(), JobError> {
        let start = std::time::Instant::now();
        let mut contended = false;
        loop {
            match self.claim(id) {
                Ok(true) => {
                    eprintln!(
                        "worker slot acquired job={id} wait_ms={} contention={contended}",
                        start.elapsed().as_millis()
                    );
                    return Ok(());
                }
                Ok(false) => (),
                Err(error) if error.is_retryable_contention() => {
                    if !contended {
                        eprintln!(
                            "worker slot contention job={id} at_ms={}: {error}",
                            now_ms()
                        );
                    }
                    contended = true;
                }
                Err(error) => return Err(error),
            }
            std::thread::sleep(
                self.config
                    .heartbeat_interval
                    .min(Duration::from_millis(50)),
            );
        }
    }
    pub fn checkpoint(&self, id: &str, completed: u64) -> Result<(), JobError> {
        loop {
            let result = self.update_with_timeout(id, self.worker_lock_budget(), |r| {
                if r.status != JobStatus::Running || r.worker_pid != Some(std::process::id()) {
                    return Err(JobError::new("JOB_INTERRUPTED", "execution lease lost"));
                }
                if r.cancel_requested {
                    return Err(JobError::new("JOB_CANCELED", "cancel requested"));
                }
                r.completed_frames = completed;
                r.heartbeat_at_ms = now_ms();
                Ok(())
            });
            match result {
                Ok(_) => return Ok(()),
                Err(error) if error.is_retryable_contention() => {
                    eprintln!(
                        "worker checkpoint contention job={id} at_ms={}: {error}",
                        now_ms()
                    );
                    std::thread::sleep(
                        self.config
                            .heartbeat_interval
                            .min(Duration::from_millis(50)),
                    );
                }
                Err(error) => return Err(error),
            }
        }
    }
    pub fn finish_error(&self, id: &str, error: &JobError) -> Result<(), JobError> {
        self.update(id, |r| {
            if r.status.active() && r.worker_pid.is_none_or(|pid| pid == std::process::id()) {
                r.status = if error.code() == "JOB_CANCELED" {
                    JobStatus::Canceled
                } else {
                    JobStatus::Failed
                };
                r.finished_at_ms = Some(now_ms());
                r.error = Some(JobFailure {
                    code: error.code().into(),
                    message: error.to_string(),
                });
            }
            Ok(())
        })?;
        Ok(())
    }
    /// Fence stale workers and cancellations through the same transaction as
    /// publication. The callback must only perform the short atomic rename.
    pub fn publish(
        &self,
        id: &str,
        result: serde_json::Value,
        publish: impl FnOnce() -> Result<(), JobError>,
    ) -> Result<(), JobError> {
        self.update(id, |r| {
            if r.status != JobStatus::Running || r.worker_pid != Some(std::process::id()) {
                return Err(JobError::new("JOB_INTERRUPTED", "execution lease lost"));
            }
            if r.cancel_requested {
                return Err(JobError::new("JOB_CANCELED", "cancel requested"));
            }
            publish()?;
            r.status = JobStatus::Succeeded;
            r.completed_frames = r.total_frames;
            r.result = Some(result);
            r.finished_at_ms = Some(now_ms());
            Ok(())
        })?;
        Ok(())
    }
    pub fn prune(&self) -> Result<PruneResult, JobError> {
        let mut db = self.connect()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        recover(&tx, self.config.heartbeat_timeout)?;
        let mut pruned = Vec::new();
        for mut r in records(&tx)? {
            if matches!(
                r.status,
                JobStatus::Succeeded | JobStatus::Failed | JobStatus::Canceled
            ) && !r.directory_pruned
                && r.finished_at_ms.is_some_and(|finished| {
                    now_ms().saturating_sub(finished) > duration_ms(self.config.retention)
                })
            {
                let path = self.directory(&r.id)?;
                match std::fs::remove_dir_all(path) {
                    Ok(()) => (),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                    Err(e) => return Err(e.into()),
                }
                r.directory_pruned = true;
                save(&tx, &r)?;
                pruned.push(r.id);
            }
        }
        tx.commit()?;
        Ok(PruneResult { pruned })
    }
    pub fn spawn(&self, id: &str, executable: &Path) -> Result<(), JobError> {
        let directory = self.directory(id)?;
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(directory.join("worker.log"))?;
        let mut command = Command::new(executable);
        command
            .args(["worker", "--job", id])
            .env("KRONELLO_STATE_ROOT", &self.config.state_root)
            .env("KRONELLO_WORKER_LOG", directory.join("worker.log"))
            .env("KRONELLO_JOB_SLOTS", self.config.slots.to_string())
            .env(
                "KRONELLO_JOB_HEARTBEAT_MS",
                self.config.heartbeat_interval.as_millis().to_string(),
            )
            .env(
                "KRONELLO_JOB_TIMEOUT_MS",
                self.config.heartbeat_timeout.as_millis().to_string(),
            )
            .env(
                "KRONELLO_JOB_RETENTION_SECONDS",
                self.config.retention.as_secs().to_string(),
            )
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log);
        // The child calls setsid before opening state. It must not be a group
        // leader beforehand. No inherited transport pipe can keep a caller alive.
        let mut child = None;
        // Spawn and register under the writer lock, so recovery cannot observe
        // an unregistered child even if either process is delayed at startup.
        if let Err(error) = self.update(id, |r| {
            if !r.status.active() {
                return Err(JobError::new("JOB_INTERRUPTED", "job is terminal"));
            }
            if r.worker_pid.is_some() {
                return Err(JobError::new(
                    "JOB_ALREADY_OWNED",
                    "another worker owns job",
                ));
            }
            #[cfg(windows)]
            let spawned = kronello_platform::spawn_detached(&command);
            #[cfg(not(windows))]
            let spawned = command.spawn();
            let spawned = spawned.map_err(|e| {
                JobError::new(
                    "WORKER_DETACH_ERROR",
                    format!("independent spawn failed: {e}"),
                )
            })?;
            #[cfg(windows)]
            let mode = spawned.detach_mode();
            #[cfg(not(windows))]
            let mode = "setsid";
            r.worker_pid = Some(spawned.id());
            r.heartbeat_at_ms = now_ms();
            child = Some(spawned);
            // Keep the child owned before fallible logging: an I/O failure must
            // reach the rollback/kill/wait path, never leak an unregistered PID.
            use std::io::Write;
            let mut log = std::fs::OpenOptions::new()
                .append(true)
                .open(directory.join("worker.log"))?;
            log.write_all(
                format!(
                    "worker launch job={id} pid={} detach_mode: \"{mode}\"\n",
                    r.worker_pid.unwrap()
                )
                .as_bytes(),
            )?;
            Ok(())
        }) {
            if let Some(mut child) = child {
                let _ = child.kill();
                let _ = child.wait();
            }
            return Err(error);
        }
        let mut child = child.ok_or_else(|| JobError::new("JOB_SPAWN_ERROR", "child missing"))?;
        // Reap normal completion while the submitter is alive; on its exit the
        // OS reparents the independent worker. This thread does not block return.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }
}
pub fn detach_worker() -> Result<(), JobError> {
    kronello_platform::detach_worker()
        .map_err(|e| JobError::new("WORKER_DETACH_ERROR", e.to_string()))
}
/// Atomic no-clobber publication on the destination volume, for files and dirs.
pub fn publish_path(staged: &Path, destination: &Path) -> Result<(), JobError> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        rustix::fs::renameat_with(
            rustix::fs::CWD,
            staged,
            rustix::fs::CWD,
            destination,
            rustix::fs::RenameFlags::NOREPLACE,
        )
        .map_err(|e| {
            if e == rustix::io::Errno::EXIST {
                JobError::new("OUTPUT_EXISTS", destination.display().to_string())
            } else if e == rustix::io::Errno::XDEV {
                JobError::new(
                    "OUTPUT_CROSS_VOLUME",
                    "publication requires the destination volume",
                )
            } else {
                JobError::Io(e.into())
            }
        })?;
        Ok(())
    }
    #[cfg(windows)]
    {
        kronello_platform::rename_noreplace(staged, destination).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                JobError::new("OUTPUT_EXISTS", destination.display().to_string())
            } else if e.raw_os_error() == Some(17) {
                JobError::new(
                    "OUTPUT_CROSS_VOLUME",
                    "publication requires the destination volume",
                )
            } else {
                JobError::Io(e)
            }
        })
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    {
        let _ = (staged, destination);
        Err(JobError::new(
            "UNSUPPORTED_FEATURE",
            "atomic publication unavailable",
        ))
    }
}
