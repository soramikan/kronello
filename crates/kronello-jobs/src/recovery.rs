//! Hash-bound staging ownership and publication receipts.
use super::*;
use std::io::Read;

#[derive(Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Identity {
    job: String,
    input_hash: String,
    snapshot_hash: String,
    attempt: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, JobStore, JobRecord) {
        let temp = tempfile::tempdir().unwrap();
        let store = JobStore::open(JobConfig::at(temp.path())).unwrap();
        let record = store
            .submit(
                b"fixed",
                Submission {
                    engine_version: "test".into(),
                    project_id: "test".into(),
                    revision: "1".into(),
                    snapshot_hash: "snapshot".into(),
                    output_profile: serde_json::json!({}),
                    destination: temp.path().join("output"),
                    total_frames: 3,
                },
            )
            .unwrap();
        store
            .finish_error(&record.id, &JobError::new("TEST_FAILURE", "stopped"))
            .unwrap();
        (temp, store, record)
    }
    #[test]
    fn resume_rejects_unowned_stage_and_changed_input_without_deleting_them() {
        let (_temp, store, record) = fixture();
        let stage = store.staging(&record).unwrap();
        let path = stage.path.clone();
        std::mem::forget(stage);
        let marker = path.join("owner.json");
        let mut owner = Identity::of(&record);
        owner.input_hash = "other input".into();
        std::fs::write(&marker, serde_json::to_vec(&owner).unwrap()).unwrap();
        assert_eq!(
            store.resume(&record.id).unwrap_err().code(),
            "JOB_STAGING_OWNERSHIP_MISMATCH"
        );
        assert!(marker.exists());
        std::fs::write(
            store.directory(&record.id).unwrap().join("input.json"),
            b"changed",
        )
        .unwrap();
        assert_eq!(
            store.resume(&record.id).unwrap_err().code(),
            "JOB_INPUT_HASH_MISMATCH"
        );
        assert!(marker.exists());
    }
    #[test]
    fn oversized_owner_and_untrusted_receipt_are_rejected() {
        let (_temp, store, record) = fixture();
        let stage = store.staging(&record).unwrap();
        let marker = stage.path.join("owner.json");
        std::fs::write(&marker, vec![b'x'; 16385]).unwrap();
        assert_eq!(
            store.resume(&record.id).unwrap_err().code(),
            "JOB_STAGING_OWNERSHIP_MISMATCH"
        );
        assert!(marker.exists());
        std::fs::write(&marker, serde_json::to_vec(&Identity::of(&record)).unwrap()).unwrap();
        drop(stage);
        let resumed = store.resume(&record.id).unwrap();
        assert!(store.claim(&record.id).unwrap());
        let stage = store.staging(&resumed).unwrap();
        std::fs::write(stage.output(), b"validated output").unwrap();
        let running = store.get(&record.id).unwrap();
        store
            .prepare_publication(
                &running,
                &stage.output(),
                serde_json::json!({"validated":true}),
            )
            .unwrap();
        let prepared = store.get(&record.id).unwrap();
        assert!(store.publication_result(&prepared).unwrap().is_some());
        std::fs::write(store.receipt_path(&prepared).unwrap(), b"{} ").unwrap();
        assert_eq!(
            store.publication_result(&prepared).unwrap_err().code(),
            "JOB_INPUT_HASH_MISMATCH"
        );
    }
    #[test]
    #[cfg(unix)]
    fn symlinked_artifact_and_staging_are_never_followed_or_removed() {
        let (temp, store, record) = fixture();
        let foreign = temp.path().join("foreign");
        std::fs::create_dir(&foreign).unwrap();
        std::fs::write(foreign.join("keep"), b"keep").unwrap();
        let path = stage_path(&record).unwrap();
        std::os::unix::fs::symlink(&foreign, &path).unwrap();
        assert_eq!(
            store.resume(&record.id).unwrap_err().code(),
            "JOB_STAGING_OWNERSHIP_MISMATCH"
        );
        assert!(foreign.join("keep").exists());
        std::fs::remove_file(&path).unwrap();
        let resumed = store.resume(&record.id).unwrap();
        let stage = store.staging(&resumed).unwrap();
        std::os::unix::fs::symlink(foreign.join("keep"), stage.output()).unwrap();
        assert_eq!(
            store
                .prepare_publication(&resumed, &stage.output(), serde_json::json!({}))
                .unwrap_err()
                .code(),
            "OUTPUT_VALIDATION_FAILED"
        );
        drop(stage);
        assert!(foreign.join("keep").exists());
    }
    #[test]
    fn partial_receipt_without_destination_can_be_discarded_and_retried() {
        let (_temp, store, record) = fixture();
        let receipt = store.receipt_path(&record).unwrap();
        std::fs::write(&receipt, b"{partial").unwrap();
        let resumed = store.resume(&record.id).unwrap();
        let _stage = store.staging(&resumed).unwrap();
        assert_eq!(resumed.attempt, 1);
        assert!(!receipt.exists());
        assert!(resumed.publication_hash.is_none());
    }
    #[test]
    fn slow_receipt_and_reconcile_validation_leave_peer_heartbeat_free_and_prune_keeps_result() {
        let (_temp, store, record) = fixture();
        let resumed = store.resume(&record.id).unwrap();
        assert!(store.claim(&record.id).unwrap());
        let stage = store.staging(&resumed).unwrap();
        std::fs::write(stage.output(), b"validated artifact").unwrap();
        let running = store.get(&record.id).unwrap();
        let peer = store
            .submit(
                b"peer",
                Submission {
                    engine_version: "test".into(),
                    project_id: "peer".into(),
                    revision: "1".into(),
                    snapshot_hash: "peer".into(),
                    output_profile: serde_json::json!({}),
                    destination: store.config.state_root.join("peer-output"),
                    total_frames: 1,
                },
            )
            .unwrap();
        assert!(!store.claim(&peer.id).unwrap());
        let (ready, pause) = std::sync::mpsc::sync_channel(1);
        let (release, wait) = std::sync::mpsc::sync_channel(1);
        let worker_store = store.clone();
        let output = stage.output();
        let preparation = std::thread::spawn(move || {
            worker_store.prepare_publication_inner(
                &running,
                &output,
                serde_json::json!({"validated":true,"report":{"frames":[1,2,3]}}),
                || {
                    ready.send(()).unwrap();
                    wait.recv().unwrap();
                },
            )
        });
        pause
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        let heartbeat = store.heartbeat(&peer.id);
        release.send(()).unwrap();
        preparation.join().unwrap().unwrap();
        heartbeat.expect("receipt I/O must not hold the connection gate");
        store
            .publish_attempt(&record.id, resumed.attempt, serde_json::json!({}), || {
                publish_path(&stage.output(), &record.destination)
            })
            .unwrap();
        store
            .update(&record.id, |r| {
                r.status = JobStatus::Interrupted;
                r.worker_pid = None;
                Ok(())
            })
            .unwrap();
        let (ready, pause) = std::sync::mpsc::sync_channel(1);
        let (release, wait) = std::sync::mpsc::sync_channel(1);
        let recovery_store = store.clone();
        let id = record.id.clone();
        let recovery = std::thread::spawn(move || {
            recovery_store.resume_inner(&id, || {
                ready.send(()).unwrap();
                wait.recv().unwrap();
            })
        });
        pause
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        let heartbeat = store.heartbeat(&peer.id);
        release.send(()).unwrap();
        let recovered = recovery.join().unwrap().unwrap();
        heartbeat.expect("full artifact verification must not hold the connection gate");
        assert_eq!(recovered.status, JobStatus::Succeeded);
        let result = recovered.result.unwrap();
        assert_eq!(result["report"]["frames"], serde_json::json!([1, 2, 3]));
        let mut pruning = store.clone();
        pruning.config.retention = std::time::Duration::ZERO;
        pruning
            .update(&record.id, |r| {
                r.finished_at_ms = Some(now_ms() - 1);
                Ok(())
            })
            .unwrap();
        assert_eq!(pruning.prune().unwrap().pruned, vec![record.id.clone()]);
        assert!(!store.directory(&record.id).unwrap().exists());
        assert_eq!(store.get(&record.id).unwrap().result.unwrap(), result);
        assert_eq!(
            store
                .list()
                .unwrap()
                .iter()
                .find(|r| r.id == record.id)
                .unwrap()
                .result
                .as_ref(),
            Some(&result)
        );
    }
    #[test]
    fn expired_resume_controller_cannot_launch_or_fail_a_new_attempt() {
        let (temp, store, record) = fixture();
        let stale = store.resume(&record.id).unwrap();
        assert_eq!(stale.attempt, 1);
        // The controller is suspended before PID registration. A polling peer
        // recovers its expired queue and wins the next generation, without sleeps.
        store
            .update(&record.id, |current| {
                current.heartbeat_at_ms =
                    now_ms() - duration_ms(store.config.heartbeat_timeout) - 1;
                Ok(())
            })
            .unwrap();
        assert_eq!(
            store.get(&record.id).unwrap().status,
            JobStatus::Interrupted
        );
        let newer = store.resume(&record.id).unwrap();
        assert_eq!(newer.attempt, 2);
        let invalid_executable = temp.path().join("missing-worker");
        let stale_launch = store
            .spawn_attempt(&record.id, stale.attempt, &invalid_executable)
            .unwrap_err();
        assert_eq!(stale_launch.code(), "JOB_INTERRUPTED");
        assert_eq!(
            store
                .finish_error_attempt(&record.id, stale.attempt, &stale_launch)
                .unwrap_err()
                .code(),
            "JOB_INTERRUPTED"
        );
        let untouched = store.get(&record.id).unwrap();
        assert_eq!(untouched.attempt, newer.attempt);
        assert_eq!(untouched.status, JobStatus::Queued);
        assert!(untouched.worker_pid.is_none());
        assert!(untouched.error.is_none());
        // The current controller still records its own genuine spawn failure.
        let current_launch = store
            .spawn_attempt(&record.id, newer.attempt, &invalid_executable)
            .unwrap_err();
        assert_eq!(current_launch.code(), "WORKER_DETACH_ERROR");
        store
            .finish_error_attempt(&record.id, newer.attempt, &current_launch)
            .unwrap();
        let failed = store.get(&record.id).unwrap();
        assert_eq!(failed.status, JobStatus::Failed);
        assert_eq!(failed.attempt, newer.attempt);
        assert_eq!(failed.error.unwrap().code, "WORKER_DETACH_ERROR");
    }
    #[test]
    fn concurrent_resume_has_one_winner_and_old_attempt_cannot_remove_or_publish_new_work() {
        let (_temp, store, record) = fixture();
        let old_stage = store.staging(&record).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let mut threads = Vec::new();
        for _ in 0..2 {
            let store = store.clone();
            let id = record.id.clone();
            let barrier = barrier.clone();
            threads.push(std::thread::spawn(move || {
                barrier.wait();
                store.resume(&id)
            }));
        }
        barrier.wait();
        let outcomes: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        assert_eq!(outcomes.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(
            outcomes
                .iter()
                .find_map(|r| r.as_ref().err())
                .unwrap()
                .code(),
            "JOB_ALREADY_OWNED"
        );
        let resumed = store.get(&record.id).unwrap();
        assert_eq!(resumed.attempt, 1);
        let new_stage = store.staging(&resumed).unwrap();
        std::fs::write(new_stage.output(), b"new work").unwrap();
        drop(old_stage);
        assert!(new_stage.output().exists());
        assert!(store.claim(&record.id).unwrap());
        assert_eq!(
            store
                .prepare_publication(&record, &new_stage.output(), serde_json::json!({}))
                .unwrap_err()
                .code(),
            "JOB_INTERRUPTED"
        );
        let mut published = false;
        assert_eq!(
            store
                .publish_attempt(&record.id, record.attempt, serde_json::json!({}), || {
                    published = true;
                    Ok(())
                })
                .unwrap_err()
                .code(),
            "JOB_INTERRUPTED"
        );
        assert!(!published);
        store.cancel(&record.id).unwrap();
        let current = store.get(&record.id).unwrap();
        assert_eq!(
            store
                .prepare_publication(&current, &new_stage.output(), serde_json::json!({}))
                .unwrap_err()
                .code(),
            "JOB_CANCELED"
        );
        assert!(new_stage.output().exists());
    }
}
impl Identity {
    fn of(record: &JobRecord) -> Self {
        Self {
            job: record.id.clone(),
            input_hash: record.input_hash.clone(),
            snapshot_hash: record.snapshot_hash.clone(),
            attempt: record.attempt,
        }
    }
}
#[derive(Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Artifact {
    path: PathBuf,
    bytes: u64,
    sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    identity: Identity,
    artifacts: Vec<Artifact>,
    result: serde_json::Value,
}
fn digest_file(file: &mut std::fs::File) -> Result<(u64, String), JobError> {
    let mut digest = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        bytes += count as u64;
        digest.update(&buffer[..count]);
    }
    Ok((bytes, format!("{:x}", digest.finalize())))
}
fn artifacts(path: &Path) -> Result<Vec<Artifact>, JobError> {
    fn walk(
        root: &Path,
        relative: &Path,
        out: &mut Vec<Artifact>,
        budget: &mut usize,
    ) -> Result<(), JobError> {
        if relative.components().count() > 4
            || relative.as_os_str().len() > 4096
            || out.len() >= 4_000_001
        {
            return Err(JobError::new(
                "RESOURCE_LIMIT",
                "artifact tree exceeds sequence bounds",
            ));
        }
        let path = if relative.as_os_str().is_empty() {
            root.to_path_buf()
        } else {
            root.join(relative)
        };
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.is_file() {
            *budget = budget.saturating_add(relative.as_os_str().len().saturating_add(256));
            if *budget > 256 * 1024 * 1024 {
                return Err(JobError::new(
                    "RESOURCE_LIMIT",
                    "artifact manifest exceeds memory budget",
                ));
            }
            let mut file = std::fs::File::open(path)?;
            let (bytes, sha256) = digest_file(&mut file)?;
            if bytes != metadata.len() || bytes != file.metadata()?.len() {
                return Err(JobError::new(
                    "OUTPUT_VALIDATION_FAILED",
                    "artifact length changed during verification",
                ));
            }
            out.push(Artifact {
                path: relative.into(),
                bytes,
                sha256,
            });
        } else if metadata.is_dir() && relative.as_os_str().is_empty() {
            for entry in std::fs::read_dir(path)? {
                let entry = entry?;
                walk(root, &relative.join(entry.file_name()), out, budget)?;
            }
        } else {
            return Err(JobError::new(
                "OUTPUT_VALIDATION_FAILED",
                "artifact is not a regular file or directory",
            ));
        }
        Ok(())
    }
    let mut entries = Vec::new();
    walk(path, Path::new(""), &mut entries, &mut 0)?;
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    if entries.is_empty() {
        return Err(JobError::new("OUTPUT_VALIDATION_FAILED", "empty artifact"));
    }
    Ok(entries)
}
fn write_json(path: &Path, value: &impl Serialize) -> Result<(), JobError> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    serde_json::to_writer(&mut file, value)?;
    if file.metadata()?.len() > 512 * 1024 * 1024 {
        return Err(JobError::new(
            "RESOURCE_LIMIT",
            "publication receipt exceeds manifest bounds",
        ));
    }
    file.sync_all()?;
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}
pub struct JobStaging {
    path: PathBuf,
    identity: Identity,
}
impl JobStaging {
    pub fn output(&self) -> PathBuf {
        self.path.join("output")
    }
}
impl Drop for JobStaging {
    fn drop(&mut self) {
        let _ = remove_owned(&self.path, &self.identity);
    }
}
fn stage_path(record: &JobRecord) -> Result<PathBuf, JobError> {
    let parent = record
        .destination
        .parent()
        .ok_or_else(|| JobError::new("INVALID_REQUEST", "destination has no parent"))?;
    Ok(parent.join(format!(
        ".kronello-job-{}-{}-{}",
        record.id, record.input_hash, record.attempt
    )))
}
#[derive(PartialEq)]
struct DestinationStamp {
    bytes: u64,
    modified: Option<std::time::SystemTime>,
    directory: bool,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}
fn destination_stamp(path: &Path) -> Result<Option<DestinationStamp>, JobError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        result => result?,
    };
    if !metadata.is_file() && !metadata.is_dir() {
        return Err(JobError::new(
            "OUTPUT_VALIDATION_FAILED",
            "destination is not a regular artifact",
        ));
    }
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;
    Ok(Some(DestinationStamp {
        bytes: metadata.len(),
        modified: metadata.modified().ok(),
        directory: metadata.is_dir(),
        #[cfg(unix)]
        identity: (
            metadata.dev(),
            metadata.ino(),
            metadata.ctime(),
            metadata.ctime_nsec(),
        ),
    }))
}
fn remove_owned(path: &Path, identity: &Identity) -> Result<(), JobError> {
    if !verify_owned(path, identity)? {
        return Ok(());
    }
    match std::fs::remove_dir_all(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        result => result.map_err(Into::into),
    }
}
fn verify_owned(path: &Path, identity: &Identity) -> Result<bool, JobError> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
        Ok(meta) if !meta.is_dir() => {
            return Err(JobError::new(
                "JOB_STAGING_OWNERSHIP_MISMATCH",
                "staging path is not a directory",
            ));
        }
        Ok(_) => {}
    }
    let owner_path = path.join("owner.json");
    let owner_metadata = std::fs::symlink_metadata(&owner_path)?;
    if !owner_metadata.is_file() || owner_metadata.len() > 16384 {
        return Err(JobError::new(
            "JOB_STAGING_OWNERSHIP_MISMATCH",
            "ownership marker is not a regular file",
        ));
    }
    let owner: Identity = serde_json::from_slice(&std::fs::read(owner_path)?)?;
    if &owner != identity {
        return Err(JobError::new(
            "JOB_STAGING_OWNERSHIP_MISMATCH",
            "staging identity differs",
        ));
    }
    Ok(true)
}
impl JobStore {
    pub fn staging(&self, record: &JobRecord) -> Result<JobStaging, JobError> {
        // The active worker's independent heartbeat remains free to run while
        // potentially large abandoned directories are removed.
        for attempt in 0..record.attempt {
            let mut old = record.clone();
            old.attempt = attempt;
            remove_owned(&stage_path(&old)?, &Identity::of(&old))?;
            let receipt = self.receipt_path(&old)?;
            match std::fs::remove_file(receipt) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
                Ok(()) => {}
            }
        }
        let path = stage_path(record)?;
        std::fs::create_dir(&path)?;
        let identity = Identity::of(record);
        if let Err(error) = write_json(&path.join("owner.json"), &identity) {
            let _ = std::fs::remove_dir_all(&path);
            return Err(error);
        }
        Ok(JobStaging { path, identity })
    }
    /// Persist validated bytes before the filesystem/SQLite commit boundary.
    pub fn prepare_publication(
        &self,
        record: &JobRecord,
        output: &Path,
        result: serde_json::Value,
    ) -> Result<(), JobError> {
        self.prepare_publication_inner(record, output, result, || {})
    }
    fn prepare_publication_inner(
        &self,
        record: &JobRecord,
        output: &Path,
        result: serde_json::Value,
        before_anchor: impl FnOnce(),
    ) -> Result<(), JobError> {
        self.input(record)?;
        let receipt = Receipt {
            identity: Identity::of(record),
            artifacts: artifacts(output)?,
            result,
        };
        let path = self.receipt_path(record)?;
        write_json(&path, &receipt)?;
        let receipt_hash = digest_file(&mut std::fs::File::open(&path)?)?.1;
        before_anchor();
        self.update(&record.id, |current| {
            if current.status != JobStatus::Running
                || current.worker_pid != Some(std::process::id())
                || current.attempt != record.attempt
            {
                return Err(JobError::new("JOB_INTERRUPTED", "publication lease lost"));
            }
            if current.cancel_requested {
                return Err(JobError::new("JOB_CANCELED", "cancel requested"));
            }
            current.publication_hash = Some(receipt_hash);
            Ok(())
        })?;
        Ok(())
    }
    fn receipt_path(&self, record: &JobRecord) -> Result<PathBuf, JobError> {
        self.directory(&record.id)?;
        Ok(self
            .config
            .state_root
            .join("job-results")
            .join(format!("{}-{}.json", record.id, record.attempt)))
    }
    pub fn publication_result(
        &self,
        record: &JobRecord,
    ) -> Result<Option<serde_json::Value>, JobError> {
        Ok(self.read_receipt(record)?.map(|receipt| receipt.result))
    }
    pub(crate) fn hydrate_results(
        &self,
        mut records: Vec<JobRecord>,
    ) -> Result<Vec<JobRecord>, JobError> {
        for record in &mut records {
            if record.status == JobStatus::Succeeded
                && record.result.is_none()
                && record.publication_hash.is_some()
            {
                record.result = self.publication_result(record)?;
                if record.result.is_none() {
                    return Err(JobError::new(
                        "OUTPUT_VALIDATION_FAILED",
                        "published result receipt missing",
                    ));
                }
            }
        }
        Ok(records)
    }
    fn read_receipt(&self, record: &JobRecord) -> Result<Option<Receipt>, JobError> {
        let path = self.receipt_path(record)?;
        if !path.exists() {
            return Ok(None);
        }
        let metadata = std::fs::symlink_metadata(&path)?;
        if !metadata.is_file() {
            return Err(JobError::new(
                "OUTPUT_VALIDATION_FAILED",
                "publication receipt is not a regular file",
            ));
        }
        if metadata.len() > 512 * 1024 * 1024 {
            return Err(JobError::new(
                "RESOURCE_LIMIT",
                "publication receipt exceeds manifest bounds",
            ));
        }
        let bytes = std::fs::read(path)?;
        if record.publication_hash.as_deref() != Some(hash(&bytes).as_str()) {
            return Err(JobError::new(
                "JOB_INPUT_HASH_MISMATCH",
                "publication receipt hash differs",
            ));
        }
        let receipt: Receipt = serde_json::from_slice(&bytes)?;
        if receipt.identity != Identity::of(record) {
            return Err(JobError::new(
                "JOB_INPUT_HASH_MISMATCH",
                "publication receipt identity differs",
            ));
        }
        Ok(Some(receipt))
    }
    fn reconcile_record(&self, record: &mut JobRecord) -> Result<bool, JobError> {
        let path = self.receipt_path(record)?;
        if !path.exists() || !record.destination.exists() {
            return Ok(false);
        }
        self.input(record)?;
        let receipt = self.read_receipt(record)?.ok_or_else(|| {
            JobError::new("OUTPUT_VALIDATION_FAILED", "missing publication receipt")
        })?;
        if receipt.identity != Identity::of(record)
            || receipt.artifacts != artifacts(&record.destination)?
        {
            return Err(JobError::new(
                "OUTPUT_VALIDATION_FAILED",
                "published artifact differs from validated receipt",
            ));
        }
        record.status = JobStatus::Succeeded;
        record.completed_frames = record.total_frames;
        record.result = Some(receipt.result);
        record.error = None;
        record.finished_at_ms = Some(now_ms());
        Ok(true)
    }
    /// Reset a terminal execution only after fixed inputs have been revalidated.
    pub fn resume(&self, id: &str) -> Result<JobRecord, JobError> {
        self.resume_inner(id, || {})
    }
    fn resume_inner(
        &self,
        id: &str,
        after_validation: impl FnOnce(),
    ) -> Result<JobRecord, JobError> {
        let mut original = self.get(id)?;
        original.result = None;
        if original.status.active() || original.worker_pid.is_some_and(worker_is_alive) {
            return Err(JobError::new(
                "JOB_ALREADY_OWNED",
                "execution is still alive",
            ));
        }
        if original.directory_pruned {
            return Err(JobError::new(
                "JOB_INPUT_UNAVAILABLE",
                "fixed input was pruned",
            ));
        }
        self.input(&original)?;
        let stamp = destination_stamp(&original.destination)?;
        let mut verified = original.clone();
        let reconciled = self.reconcile_record(&mut verified)?;
        verified.result = None;
        verify_owned(&stage_path(&original)?, &Identity::of(&original))?;
        if !reconciled && (original.status == JobStatus::Succeeded || stamp.is_some()) {
            return Err(JobError::new(
                "OUTPUT_EXISTS",
                "destination cannot be replaced",
            ));
        }
        after_validation();
        let resumed = self.update(id, |record| {
            if record.status != original.status
                || record.worker_pid != original.worker_pid
                || record.status.active()
                || record.worker_pid.is_some_and(worker_is_alive)
                || record.attempt != original.attempt
            {
                return Err(JobError::new(
                    "JOB_ALREADY_OWNED",
                    "execution changed during recovery validation",
                ));
            }
            if record.input_hash != original.input_hash
                || record.snapshot_hash != original.snapshot_hash
                || record.publication_hash != original.publication_hash
                || record.directory_pruned != original.directory_pruned
                || record.destination != original.destination
            {
                return Err(JobError::new(
                    "JOB_INPUT_HASH_MISMATCH",
                    "recovery identity changed",
                ));
            }
            if destination_stamp(&record.destination)? != stamp {
                return Err(JobError::new(
                    "OUTPUT_VALIDATION_FAILED",
                    "destination changed during recovery",
                ));
            }
            if reconciled {
                record.status = JobStatus::Succeeded;
                record.completed_frames = record.total_frames;
                record.result = None;
                record.error = None;
                record.finished_at_ms = Some(now_ms());
                return Ok(());
            }
            record.status = JobStatus::Queued;
            record.attempt = record
                .attempt
                .checked_add(1)
                .ok_or_else(|| JobError::new("RESOURCE_LIMIT", "job attempt overflow"))?;
            record.worker_pid = None;
            record.completed_frames = 0;
            record.cancel_requested = false;
            record.finished_at_ms = None;
            record.error = None;
            record.result = None;
            record.publication_hash = None;
            record.heartbeat_at_ms = now_ms();
            Ok(())
        })?;
        if reconciled {
            remove_owned(&stage_path(&original)?, &Identity::of(&original))?;
        }
        Ok(self.hydrate_results(vec![resumed])?.remove(0))
    }
}
