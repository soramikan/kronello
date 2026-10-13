//! Operational deadlines, independent of frame/block frequency or model time.
use crate::{JobError, JobRecord, JobStore};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct WorkerProgressStats {
    pub callbacks: u64,
    pub control_checks: u64,
    pub progress_writes: u64,
    /// Actual gap at callback gates; includes native work and scheduler delay.
    pub max_control_gap_us: u64,
}
pub struct WorkerProgress {
    store: JobStore,
    id: String,
    attempt: u64,
    latest: u64,
    persisted: u64,
    control_interval: Duration,
    progress_interval: Duration,
    last_control: Option<Instant>,
    last_progress: Option<Instant>,
    stats: WorkerProgressStats,
}
impl WorkerProgress {
    pub fn new(store: JobStore, record: &JobRecord) -> Self {
        Self::with_intervals(
            store,
            record,
            Duration::from_millis(50),
            Duration::from_millis(200),
        )
    }
    /// Runtime policy only; it never enters a request, snapshot or content hash.
    pub fn with_intervals(
        store: JobStore,
        record: &JobRecord,
        control_interval: Duration,
        progress_interval: Duration,
    ) -> Self {
        Self {
            store,
            id: record.id.clone(),
            attempt: record.attempt,
            latest: record.completed_frames,
            persisted: record.completed_frames,
            control_interval,
            progress_interval,
            last_control: None,
            last_progress: None,
            stats: WorkerProgressStats::default(),
        }
    }
    pub fn stats(&self) -> WorkerProgressStats {
        self.stats
    }
    pub fn checkpoint(&mut self, completed: u64) -> Result<(), JobError> {
        self.checkpoint_at(completed, Instant::now())
    }
    fn checkpoint_at(&mut self, completed: u64, now: Instant) -> Result<(), JobError> {
        self.stats.callbacks += 1;
        self.latest = self.latest.max(completed);
        if self
            .last_control
            .is_none_or(|last| now.duration_since(last) >= self.control_interval)
        {
            self.control(now)?;
        }
        if self.latest != self.persisted
            && self
                .last_progress
                .is_none_or(|last| now.duration_since(last) >= self.progress_interval)
        {
            self.persist(now)?;
        }
        Ok(())
    }
    fn control(&mut self, now: Instant) -> Result<(), JobError> {
        self.stats.control_checks += 1;
        if let Some(last) = self.last_control {
            self.stats.max_control_gap_us = self
                .stats
                .max_control_gap_us
                .max(now.duration_since(last).as_micros().min(u64::MAX as u128) as u64);
        }
        self.store.check_running_attempt(&self.id, self.attempt)?;
        self.last_control = Some(now);
        Ok(())
    }
    fn persist(&mut self, now: Instant) -> Result<(), JobError> {
        if self
            .store
            .persist_progress_attempt(&self.id, self.attempt, self.latest)?
        {
            self.stats.progress_writes += 1;
        }
        self.persisted = self.latest;
        self.last_progress = Some(now);
        Ok(())
    }
    pub fn flush(&mut self, completed: u64) -> Result<(), JobError> {
        self.flush_at(completed, Instant::now())
    }
    fn flush_at(&mut self, completed: u64, now: Instant) -> Result<(), JobError> {
        self.latest = self.latest.max(completed);
        self.control(now)?;
        if self.latest != self.persisted {
            self.persist(now)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{JobConfig, JobStatus, Submission};
    fn fixture() -> (tempfile::TempDir, JobStore, JobRecord) {
        let temp = tempfile::tempdir().unwrap();
        let mut store = JobStore::open(JobConfig::at(temp.path())).unwrap();
        store.write_diagnostics = Some(std::sync::Arc::default());
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
                    total_frames: 10000,
                },
            )
            .unwrap();
        loop {
            match store.claim(&record.id) {
                Err(error) if error.is_retryable_contention() => std::thread::yield_now(),
                result => {
                    assert!(result.unwrap());
                    break;
                }
            }
        }
        let record = store.get(&record.id).unwrap();
        (temp, store, record)
    }
    #[test]
    fn ten_thousand_callbacks_coalesce_writes_and_do_not_replace_heartbeats() {
        let (_temp, store, record) = fixture();
        let before = store.write_diagnostics().unwrap();
        let start = Instant::now();
        let mut progress = WorkerProgress::new(store.clone(), &record);
        for number in 0..=10000 {
            progress
                .checkpoint_at(number, start + Duration::from_micros(number * 100))
                .unwrap();
        }
        progress
            .flush_at(10000, start + Duration::from_secs(1))
            .unwrap();
        let stats = progress.stats();
        assert_eq!(stats.callbacks, 10001);
        assert_eq!(stats.control_checks, 22);
        assert_eq!(stats.progress_writes, 6);
        assert_eq!(
            store
                .write_diagnostics()
                .unwrap()
                .committed_write_transactions
                - before.committed_write_transactions,
            6
        );
        let observed = store.get(&record.id).unwrap();
        assert_eq!(observed.completed_frames, 10000);
        assert_eq!(observed.heartbeat_at_ms, record.heartbeat_at_ms);
        for number in 0..=10000 {
            progress
                .checkpoint_at(number, start + Duration::from_secs(2))
                .unwrap();
        }
        assert_eq!(progress.stats().progress_writes, 6);
    }
    #[test]
    fn read_write_race_and_same_pid_new_attempt_are_fenced() {
        for cancel in [true, false] {
            let (_temp, store, record) = fixture();
            let start = Instant::now();
            let mut progress = WorkerProgress::new(store.clone(), &record);
            progress.checkpoint_at(0, start).unwrap();
            if cancel {
                store.cancel(&record.id).unwrap();
            } else {
                store
                    .update(&record.id, |record| {
                        record.attempt += 1;
                        Ok(())
                    })
                    .unwrap();
            }
            // The read is not due, but an attempted write must still recheck
            // cancellation and generation in the authoritative transaction.
            let expected = if cancel {
                "JOB_CANCELED"
            } else {
                "JOB_INTERRUPTED"
            };
            assert_eq!(
                progress
                    .checkpoint_at(1, start + Duration::from_millis(1))
                    .unwrap_err()
                    .code(),
                expected
            );
            assert_eq!(
                progress
                    .flush_at(1, start + Duration::from_millis(2))
                    .unwrap_err()
                    .code(),
                expected
            );
            assert_eq!(store.get(&record.id).unwrap().completed_frames, 0);
            assert_eq!(
                store
                    .publish_attempt(
                        &record.id,
                        record.attempt,
                        serde_json::json!({}),
                        || panic!("fenced worker must not publish")
                    )
                    .unwrap_err()
                    .code(),
                expected
            );
        }
    }
    #[test]
    fn control_reads_do_not_compete_with_wal_writers_and_duplicate_progress_does_not_commit() {
        let (temp, store, record) = fixture();
        let before = store
            .write_diagnostics()
            .unwrap()
            .committed_write_transactions;
        let db = rusqlite::Connection::open(temp.path().join("jobs.sqlite3")).unwrap();
        db.execute_batch("BEGIN IMMEDIATE").unwrap();
        store
            .check_running_attempt(&record.id, record.attempt)
            .unwrap();
        db.execute_batch("ROLLBACK").unwrap();
        assert!(
            !store
                .persist_progress_attempt(&record.id, record.attempt, 0)
                .unwrap()
        );
        assert_eq!(
            store
                .write_diagnostics()
                .unwrap()
                .committed_write_transactions,
            before
        );
        assert!(
            store
                .persist_progress_attempt(&record.id, record.attempt, 7)
                .unwrap()
        );
        assert!(
            !store
                .persist_progress_attempt(&record.id, record.attempt, 6)
                .unwrap()
        );
        assert_eq!(store.get(&record.id).unwrap().completed_frames, 7);
        store
            .update(&record.id, |record| {
                record.status = JobStatus::Interrupted;
                Ok(())
            })
            .unwrap();
        assert_eq!(
            store
                .check_running_attempt(&record.id, record.attempt)
                .unwrap_err()
                .code(),
            "JOB_INTERRUPTED"
        );
    }
}
