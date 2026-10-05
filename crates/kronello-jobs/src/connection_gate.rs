//! FIFO connection ownership with bounded waiting and cancellation.
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::JobError;

pub(crate) struct ConnectionGate {
    state: Mutex<State>,
    changed: Condvar,
}
struct State {
    held: bool,
    waiters: VecDeque<Arc<()>>,
}
impl ConnectionGate {
    pub(crate) const fn new() -> Self {
        Self {
            state: Mutex::new(State {
                held: false,
                waiters: VecDeque::new(),
            }),
            changed: Condvar::new(),
        }
    }
    pub(crate) fn acquire(&self, timeout: Duration) -> Result<ConnectionLease<'_>, JobError> {
        let start = Instant::now();
        let ticket = Arc::new(());
        let poisoned = |_| JobError::new("JOB_STORAGE_ERROR", "connection lifetime gate poisoned");
        let mut state = self.state.lock().map_err(poisoned)?;
        state.waiters.push_back(ticket.clone());
        loop {
            if !state.held
                && state
                    .waiters
                    .front()
                    .is_some_and(|first| Arc::ptr_eq(first, &ticket))
            {
                state.waiters.pop_front();
                state.held = true;
                return Ok(ConnectionLease(self));
            }
            let remaining = timeout.saturating_sub(start.elapsed());
            if remaining.is_zero() {
                state.waiters.retain(|queued| !Arc::ptr_eq(queued, &ticket));
                self.changed.notify_all();
                return Err(JobError::new(
                    "JOB_PROCESS_BUSY",
                    "connection lifetime gate contended",
                ));
            }
            state = self
                .changed
                .wait_timeout(state, remaining)
                .map_err(|_| {
                    JobError::new("JOB_STORAGE_ERROR", "connection lifetime gate poisoned")
                })?
                .0;
        }
    }
}
pub(crate) struct ConnectionLease<'a>(&'a ConnectionGate);
impl Drop for ConnectionLease<'_> {
    fn drop(&mut self) {
        // Preserve poisoning for subsequent acquisitions, but never panic
        // while releasing an existing connection during error unwinding.
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.held = false;
        self.0.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_for_waiters(gate: &ConnectionGate, count: usize) {
        let start = Instant::now();
        while gate.state.lock().unwrap().waiters.len() != count {
            assert!(start.elapsed() < Duration::from_secs(2));
            std::thread::yield_now();
        }
    }

    #[test]
    fn immediate_reacquisition_cannot_overtake_a_waiting_writer() {
        for _ in 0..32 {
            let gate = Arc::new(ConnectionGate::new());
            let held = gate.acquire(Duration::ZERO).unwrap();
            let peer = gate.clone();
            let (send, receive) = std::sync::mpsc::channel();
            let waiter = std::thread::spawn(move || {
                let _lease = peer.acquire(Duration::from_secs(2)).unwrap();
                send.send(()).unwrap();
            });
            wait_for_waiters(&gate, 1);
            drop(held);
            let _lease = gate.acquire(Duration::from_secs(2)).unwrap();
            receive
                .try_recv()
                .expect("waiting writer must acquire first");
            waiter.join().unwrap();
        }
    }

    #[test]
    fn timed_out_waiter_does_not_block_the_next_writer() {
        let gate = Arc::new(ConnectionGate::new());
        let held = gate.acquire(Duration::ZERO).unwrap();
        let first = gate.clone();
        let timeout = std::thread::spawn(move || {
            let start = Instant::now();
            let error = first.acquire(Duration::from_millis(50)).err().unwrap();
            assert_eq!(error.code(), "JOB_PROCESS_BUSY");
            assert!(start.elapsed() < Duration::from_secs(1));
        });
        wait_for_waiters(&gate, 1);
        let second = gate.clone();
        let next = std::thread::spawn(move || {
            let _lease = second.acquire(Duration::from_secs(2)).unwrap();
        });
        timeout.join().unwrap();
        wait_for_waiters(&gate, 1);
        drop(held);
        next.join().unwrap();
        gate.acquire(Duration::ZERO).unwrap();
    }
}
