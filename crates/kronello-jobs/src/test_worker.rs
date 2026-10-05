//! Test-only executable. The payload replaces rendering; all job state and
//! process/publication behavior uses production jobs/platform functions.
use kronello_jobs::{JobConfig, JobError, JobStore, Submission};
use std::path::PathBuf;
use std::time::Duration;

fn run() -> Result<(), JobError> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if matches!(
        args.first().map(String::as_str),
        Some("stress" | "raw-stress")
    ) {
        return connection_stress(args[0] == "raw-stress");
    }
    if matches!(
        args.first().map(String::as_str),
        Some("parent" | "restricted-parent")
    ) {
        #[cfg(windows)]
        if args[0] == "restricted-parent" {
            kronello_platform::prohibit_test_parent_breakaway()?;
        }
        let store = JobStore::open(JobConfig::from_env()?)?;
        let input = std::fs::read(&args[1])?;
        let r = store.submit(
            &input,
            Submission {
                engine_version: "test".into(),
                project_id: "fixed".into(),
                revision: "1".into(),
                snapshot_hash: "fixed".into(),
                output_profile: serde_json::json!({}),
                destination: PathBuf::from(&args[2]),
                total_frames: 1,
            },
        )?;
        store.spawn(&r.id, &std::env::current_exe()?)?;
        println!("{}", serde_json::to_string(&store.get(&r.id)?)?);
        return Ok(());
    }
    if args.len() != 3 || args[0] != "worker" || args[1] != "--job" {
        return Err(JobError::new("INVALID_REQUEST", "test worker arguments"));
    }
    kronello_jobs::detach_worker()?;
    eprintln!("test worker detached pid={}", std::process::id());
    let store = JobStore::open(JobConfig::from_env()?)?;
    let id = &args[2];
    let heartbeat = kronello_jobs::WorkerHeartbeat::start(store.clone(), id.clone());
    let result = (|| {
        store.wait_for_slot(id)?;
        let r = store.get(id)?;
        let input: serde_json::Value = serde_json::from_slice(&store.input(&r)?)?;
        let gate = PathBuf::from(input["gate"].as_str().unwrap());
        while !gate.exists() {
            store.checkpoint(id, 0)?;
            std::thread::sleep(Duration::from_millis(20));
        }
        let staged = r.destination.with_extension(format!("{id}.staged"));
        let directory = input["directory"].as_bool().unwrap_or(false);
        if directory {
            std::fs::create_dir(&staged)?;
            std::fs::write(staged.join("fixed"), b"fixed job payload")?;
        } else {
            std::fs::write(&staged, b"fixed job payload")?;
        }
        if let Some(gate) = input["publication_gate"].as_str() {
            std::fs::write(
                r.destination.with_extension(format!("{id}.ready")),
                b"ready",
            )?;
            while !std::path::Path::new(gate).exists() {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        let result = store.publish(id, serde_json::json!({"validated":true}), || {
            kronello_jobs::publish_path(&staged, &r.destination)
        });
        if directory {
            let _ = std::fs::remove_dir_all(&staged);
        } else {
            let _ = std::fs::remove_file(&staged);
        }
        eprintln!(
            "test worker publication outcome job={id} code={}",
            result.as_ref().err().map_or("OK", JobError::code)
        );
        result
    })();
    drop(heartbeat);
    if let Err(error) = &result {
        store.finish_error(id, error)?;
    }
    result
}

fn connection_stress(raw: bool) -> Result<(), JobError> {
    let store = JobStore::open(JobConfig::from_env()?)?;
    let r = store.submit(
        b"fixed",
        Submission {
            engine_version: "test".into(),
            project_id: "fixed".into(),
            revision: "1".into(),
            snapshot_hash: "fixed".into(),
            output_profile: serde_json::json!({}),
            destination: store.config().state_root.join("unused"),
            total_frames: 1000,
        },
    )?;
    store.claim(&r.id)?;
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let peer_barrier = barrier.clone();
    let peer_store = store.clone();
    let peer_id = r.id.clone();
    let raw_path = store.config().state_root.join("jobs.sqlite3");
    let peer_path = raw_path.clone();
    eprintln!(
        "connection stress raw={raw} sqlite={} iterations=1000 threads=2",
        rusqlite::version()
    );
    let peer = std::thread::spawn(move || -> Result<(), JobError> {
        peer_barrier.wait();
        for completed in 1..=1000 {
            if raw {
                raw_connection_cycle(&peer_path)?;
            } else {
                loop {
                    match peer_store.heartbeat(&peer_id) {
                        Ok(()) => break,
                        Err(error) if error.is_retryable_contention() => std::thread::yield_now(),
                        Err(error) => return Err(error),
                    }
                }
            }
            if completed % 50 == 0 {
                eprintln!("connection stress progress thread=heartbeat writes={completed}");
            }
        }
        Ok(())
    });
    barrier.wait();
    for completed in 0..1000 {
        if raw {
            raw_connection_cycle(&raw_path)?;
        } else {
            store.checkpoint(&r.id, completed)?;
        }
        if (completed + 1) % 50 == 0 {
            eprintln!(
                "connection stress progress thread=checkpoint writes={}",
                completed + 1
            );
        }
    }
    peer.join().expect("connection stress thread")?;
    eprintln!("connection stress completed raw={raw} writes=2000");
    Ok(())
}
fn raw_connection_cycle(path: &std::path::Path) -> Result<(), JobError> {
    // Diagnostic baseline only, never used by production jobs. Recreates the
    // old concurrent open/WAL-close behavior in a disposable child process.
    let mut db = rusqlite::Connection::open(path)?;
    db.busy_timeout(Duration::from_secs(5))?;
    db.execute_batch("PRAGMA synchronous=FULL")?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    tx.execute("UPDATE jobs SET record=json_set(record,'$.completed_frames',json_extract(record,'$.completed_frames')+1)", [])?;
    tx.commit()?;
    Ok(())
}
fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
