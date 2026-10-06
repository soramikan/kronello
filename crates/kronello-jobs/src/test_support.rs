//! Opt-in test cleanup; never enabled by production CLI/MCP dependencies.
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kronello_platform::ProcessGuard;
use rusqlite::OpenFlags;

use crate::{JobError, JobRecord};

/// Create before launching any submitter; drop before removing the state root.
/// Discover registered workers even when a response assertion panics before an
/// ID can be returned. Explicit capture retains Windows process identity.
pub struct WorkerCleanup {
    root: PathBuf,
    workers: RefCell<BTreeMap<u32, ProcessGuard>>,
}
impl WorkerCleanup {
    pub fn new(root: &Path) -> Result<Self, JobError> {
        kronello_platform::adopt_test_workers()?;
        Ok(Self {
            root: root.into(),
            workers: RefCell::new(BTreeMap::new()),
        })
    }
    pub fn capture_registered(&self) -> Result<Vec<u32>, JobError> {
        let path = self.root.join("jobs.sqlite3");
        if !path.exists() {
            return Ok(Vec::new());
        }
        let db = rusqlite::Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        let mut statement = db.prepare("SELECT record FROM jobs")?;
        let mut captured = Vec::new();
        for text in statement.query_map([], |row| row.get::<_, String>(0))? {
            let r: JobRecord = serde_json::from_str(&text?)?;
            if let Some(pid) = r.worker_pid {
                let mut workers = self.workers.borrow_mut();
                if let std::collections::btree_map::Entry::Vacant(entry) = workers.entry(pid)
                    && kronello_platform::process_is_alive(pid)
                {
                    let guard = match ProcessGuard::capture(pid) {
                        Ok(guard) => guard,
                        Err(_) if !kronello_platform::process_is_alive(pid) => continue,
                        Err(error) => return Err(error.into()),
                    };
                    entry.insert(guard);
                    if let Some(registry) = std::env::var_os("KRONELLO_TEST_WORKER_REGISTRY") {
                        use std::io::Write;
                        let mut file = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(registry)?;
                        let detach_mode = std::fs::read_to_string(
                            self.root.join("jobs").join(&r.id).join("worker.log"),
                        )
                        .ok()
                        .and_then(|log| {
                            log.lines().find_map(|line| {
                                if !line.starts_with("worker launch job=")
                                    && !line.starts_with("worker detach_mode: ")
                                {
                                    return None;
                                }
                                let (_, value) = line.split_once("detach_mode: \"")?;
                                let (mode, _) = value.split_once('"')?;
                                matches!(mode, "breakaway" | "in_parent_job" | "setsid")
                                    .then(|| mode.to_owned())
                            })
                        });
                        let mut line = serde_json::to_vec(
                            &serde_json::json!({"pid":pid,"job":r.id,"root":self.root,"detach_mode":detach_mode}),
                        )?;
                        line.push(b'\n');
                        file.write_all(&line)?;
                    }
                }
                captured.push(pid);
            }
        }
        Ok(captured)
    }
    /// Wait without signalling: a terminal DB record may precede process exit,
    /// and adopted Linux workers remain zombies until the test reaps them.
    // This shared file is also included by state tests that only need cleanup.
    #[allow(dead_code)]
    pub fn wait_for_exit(&self, pid: u32, timeout: std::time::Duration) -> Result<(), JobError> {
        self.capture_registered()?;
        if let Some(worker) = self.workers.borrow().get(&pid) {
            worker.wait_for_exit(timeout)?;
        } else if kronello_platform::process_is_alive(pid) {
            return Err(JobError::new(
                "TEST_WORKER_NOT_CAPTURED",
                "live worker was not captured",
            ));
        }
        Ok(())
    }
    pub fn reap(&self) -> Result<(), JobError> {
        self.capture_registered()?;
        for worker in self.workers.borrow().values() {
            worker.terminate_and_wait()?;
        }
        Ok(())
    }
}
impl Drop for WorkerCleanup {
    fn drop(&mut self) {
        if let Err(error) = self.reap() {
            if std::thread::panicking() {
                eprintln!("test worker cleanup failed during unwind: {error}");
            } else {
                panic!("test worker cleanup failed: {error}");
            }
        }
    }
}
