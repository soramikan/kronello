use rusqlite::Connection;

#[test]
fn bundled_sqlite_includes_vfs_deadlock_and_wal_reset_fixes() {
    // 3.51.2 fixes the VFS lock inversion; 3.51.3 also fixes WAL reset.
    // A downgrade must fail immediately instead of hanging a parallel suite.
    assert!(
        rusqlite::version_number() >= 3_051_003,
        "{}",
        rusqlite::version()
    );
}

#[cfg(unix)]
#[test]
fn concurrent_wal_commit_and_connection_close_finish_in_a_subprocess() {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let root = tempfile::tempdir().unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "wal_connection_stress_worker"])
        .env("KRONELLO_TEST_SQLITE_WAL_STRESS_ROOT", root.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "SQLite stress worker: {status}");
            break;
        }
        if start.elapsed() > Duration::from_secs(30) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("SQLite WAL open/commit/close deadlocked");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "runs in a bounded subprocess through the concurrency test"]
fn wal_connection_stress_worker() {
    use std::sync::{Arc, Barrier};
    use std::time::Duration;

    let root = std::path::PathBuf::from(
        std::env::var_os("KRONELLO_TEST_SQLITE_WAL_STRESS_ROOT")
            .expect("owned subprocess stress root"),
    );
    let paths: Vec<_> = (0..4)
        .map(|i| root.join(format!("wal-{i}.sqlite3")))
        .collect();
    for path in &paths {
        Connection::open(path).unwrap().execute_batch(
            "PRAGMA journal_mode=WAL; CREATE TABLE counter(value INTEGER NOT NULL); INSERT INTO counter VALUES(0);",
        ).unwrap();
    }
    let barrier = Arc::new(Barrier::new(16));
    let workers: Vec<_> = (0..16)
        .map(|i| {
            let path = paths[i % 4].clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                for _ in 0..50 {
                    let connection = Connection::open(&path).unwrap();
                    connection.busy_timeout(Duration::from_secs(5)).unwrap();
                    connection
                        .execute_batch("BEGIN IMMEDIATE; UPDATE counter SET value=value+1; COMMIT;")
                        .unwrap();
                    connection.close().unwrap();
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    for path in paths {
        let count: i64 = Connection::open(path)
            .unwrap()
            .query_row("SELECT value FROM counter", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 200);
    }
}
