//! Heartbeat writes and an independent monotonic deadline watchdog.
use crate::{JobStore, now_ms};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

pub struct WorkerHeartbeat {
    stop_pulse: mpsc::Sender<()>,
    stop_watchdog: mpsc::Sender<()>,
    pulse: Option<std::thread::JoinHandle<()>>,
    watchdog: Option<std::thread::JoinHandle<()>>,
}
impl WorkerHeartbeat {
    pub fn start(store: JobStore, id: String) -> Self {
        let start = Instant::now();
        let last_success = Arc::new(AtomicU64::new(0));
        let (stop_pulse, receive) = mpsc::channel();
        let (stop_watchdog, watch_receive) = mpsc::channel();
        let timeout = store.config().heartbeat_timeout;
        let interval = store.config().heartbeat_interval;
        let health = last_success.clone();
        let watch_id = id.clone();
        let watchdog = std::thread::spawn(move || {
            while watch_receive.recv_timeout(interval.min(Duration::from_millis(100)))
                == Err(mpsc::RecvTimeoutError::Timeout)
            {
                let elapsed = start.elapsed().as_millis().min(u64::MAX as u128) as u64;
                if elapsed.saturating_sub(health.load(Ordering::Acquire))
                    >= timeout.as_millis().min(u64::MAX as u128) as u64
                {
                    eprintln!(
                        "worker heartbeat deadline exceeded job={watch_id} at_ms={} timeout_ms={}; exiting for interrupted recovery",
                        now_ms(),
                        timeout.as_millis()
                    );
                    // No SQLite call here: the pulse may be blocked inside its
                    // native mutex. Exit makes PID liveness expire and leaves
                    // recovery to a later reader, without publishing success.
                    std::process::exit(1);
                }
            }
        });
        let pulse = std::thread::spawn(move || {
            eprintln!("worker heartbeat started job={id} at_ms={}", now_ms());
            let mut missed = false;
            while receive.recv_timeout(interval) == Err(mpsc::RecvTimeoutError::Timeout) {
                match store.heartbeat(&id) {
                    Ok(()) => {
                        last_success.store(
                            start.elapsed().as_millis().min(u64::MAX as u128) as u64,
                            Ordering::Release,
                        );
                        if missed {
                            eprintln!("worker heartbeat recovered job={id} at_ms={}", now_ms());
                            missed = false;
                        }
                    }
                    Err(error) => {
                        eprintln!(
                            "worker heartbeat failed job={id} at_ms={}: {error}",
                            now_ms()
                        );
                        if !error.is_retryable_heartbeat() {
                            break;
                        }
                        missed = true;
                    }
                }
            }
        });
        Self {
            stop_pulse,
            stop_watchdog,
            pulse: Some(pulse),
            watchdog: Some(watchdog),
        }
    }
}
impl Drop for WorkerHeartbeat {
    fn drop(&mut self) {
        let _ = self.stop_pulse.send(());
        // Keep the watchdog alive while joining: a native mutex stall must not
        // turn worker shutdown itself into an unbounded wait.
        if self.pulse.take().is_some_and(|pulse| pulse.join().is_err()) {
            eprintln!("worker heartbeat thread panicked");
        }
        let _ = self.stop_watchdog.send(());
        if self
            .watchdog
            .take()
            .is_some_and(|watchdog| watchdog.join().is_err())
        {
            eprintln!("worker heartbeat watchdog panicked");
        }
    }
}
