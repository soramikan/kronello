//! Opt-in execution measurements; never part of authored snapshot identity.
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExecutionStageMeasurement {
    pub calls: u64,
    pub elapsed_us: u64,
    /// First 64 observations only; never grows with the export frame count.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(length(max = 64))]
    pub first_samples_us: Vec<u64>,
    #[serde(skip)]
    remainder_ns: u16,
}
impl ExecutionStageMeasurement {
    fn record(&mut self, duration: std::time::Duration) {
        // Carry fractions across calls; flooring each short call separately
        // would make thousands of sub-microsecond stages appear to cost zero.
        let fractional = self.remainder_ns + (duration.subsec_nanos() % 1000) as u16;
        self.calls = self.calls.saturating_add(1);
        self.elapsed_us = self
            .elapsed_us
            .saturating_add(micros(duration))
            .saturating_add(u64::from(fractional / 1000));
        self.remainder_ns = fractional % 1000;
        if self.first_samples_us.len() < 64 {
            self.first_samples_us.push(micros(duration));
        }
    }
}

/// Stage times may overlap on producer and consumer threads. Their sum is
/// neither wall time nor GPU timestamp time. Payload gauges exclude allocator,
/// codec and driver memory; process RSS must be measured independently.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExecutionDiagnostics {
    pub schema_version: u32,
    pub wall_us: u64,
    pub stages: BTreeMap<String, ExecutionStageMeasurement>,
    pub counts: BTreeMap<String, u64>,
    pub peak_payload_bytes: BTreeMap<String, u64>,
}

#[derive(Clone, Default)]
pub struct ExecutionDiagnosticsCollector(Option<Arc<State>>);
struct State {
    start: Instant,
    measurements: Mutex<ExecutionDiagnostics>,
}
impl ExecutionDiagnosticsCollector {
    pub fn enabled() -> Self {
        Self(Some(Arc::new(State {
            start: Instant::now(),
            measurements: Mutex::new(ExecutionDiagnostics {
                schema_version: 1,
                ..ExecutionDiagnostics::default()
            }),
        })))
    }
    /// Process execution policy. No request or document field enables this.
    pub fn from_environment() -> Self {
        if let Some(active) = ACTIVE.with(|slot| slot.borrow().clone()) {
            return active;
        }
        if std::env::var_os("KRONELLO_EXPORT_DIAGNOSTICS").as_deref()
            == Some(std::ffi::OsStr::new("1"))
        {
            Self::enabled()
        } else {
            Self::default()
        }
    }
    /// Install a thread-scoped observer. Restore the previous observer on
    /// return or unwind; the guard cannot move to another thread.
    pub fn activate(&self) -> ExecutionDiagnosticsScope {
        let previous = ACTIVE.with(|slot| slot.replace(Some(self.clone())));
        ExecutionDiagnosticsScope {
            previous,
            _thread: std::marker::PhantomData,
        }
    }
    pub fn time(&self, stage: &'static str) -> ExecutionStageTimer {
        ExecutionStageTimer {
            state: self.0.clone(),
            start: self.0.as_ref().map(|_| Instant::now()),
            stage,
        }
    }
    pub fn record_duration(&self, name: &'static str, duration: std::time::Duration) {
        if let Some(state) = &self.0 {
            let mut measurements = state.measurements.lock().unwrap_or_else(|e| e.into_inner());
            measurements
                .stages
                .entry(name.into())
                .or_default()
                .record(duration);
        }
    }
    pub fn count(&self, name: &'static str, amount: u64) {
        if let Some(state) = &self.0 {
            let mut measurements = state.measurements.lock().unwrap_or_else(|e| e.into_inner());
            let count = measurements.counts.entry(name.into()).or_default();
            *count = count.saturating_add(amount);
        }
    }
    pub fn peak_payload(&self, name: &'static str, bytes: u64) {
        if let Some(state) = &self.0 {
            let mut measurements = state.measurements.lock().unwrap_or_else(|e| e.into_inner());
            let peak = measurements
                .peak_payload_bytes
                .entry(name.into())
                .or_default();
            *peak = (*peak).max(bytes);
        }
    }
    pub fn snapshot(&self) -> Option<ExecutionDiagnostics> {
        self.0.as_ref().map(|state| {
            let mut result = state
                .measurements
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            result.wall_us = micros(state.start.elapsed());
            result
        })
    }
}
thread_local! {
    static ACTIVE: std::cell::RefCell<Option<ExecutionDiagnosticsCollector>> = const { std::cell::RefCell::new(None) };
}
pub struct ExecutionDiagnosticsScope {
    previous: Option<ExecutionDiagnosticsCollector>,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}
impl Drop for ExecutionDiagnosticsScope {
    fn drop(&mut self) {
        ACTIVE.with(|slot| slot.replace(self.previous.take()));
    }
}
/// Observational host time only; it is never given to expression evaluation.
pub fn measure_execution_stage(stage: &'static str) -> ExecutionStageTimer {
    ACTIVE.with(|slot| {
        slot.borrow().as_ref().map_or_else(
            || ExecutionDiagnosticsCollector::default().time(stage),
            |collector| collector.time(stage),
        )
    })
}
pub fn count_execution(name: &'static str, amount: u64) {
    ACTIVE.with(|slot| {
        if let Some(collector) = slot.borrow().as_ref() {
            collector.count(name, amount);
        }
    });
}
pub fn execution_diagnostics_enabled() -> bool {
    ACTIVE.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|collector| collector.0.is_some())
    })
}
pub fn record_execution_duration(name: &'static str, duration: std::time::Duration) {
    ACTIVE.with(|slot| {
        if let Some(collector) = slot.borrow().as_ref() {
            collector.record_duration(name, duration);
        }
    });
}
pub fn observe_execution_payload(name: &'static str, bytes: u64) {
    ACTIVE.with(|slot| {
        if let Some(collector) = slot.borrow().as_ref() {
            collector.peak_payload(name, bytes);
        }
    });
}
pub struct ExecutionStageTimer {
    state: Option<Arc<State>>,
    start: Option<Instant>,
    stage: &'static str,
}
fn micros(duration: std::time::Duration) -> u64 {
    duration.as_micros().min(u128::from(u64::MAX)) as u64
}
impl Drop for ExecutionStageTimer {
    fn drop(&mut self) {
        if let (Some(state), Some(start)) = (&self.state, self.start) {
            let elapsed = start.elapsed();
            let mut measurements = state.measurements.lock().unwrap_or_else(|e| e.into_inner());
            let stage = measurements.stages.entry(self.stage.into()).or_default();
            stage.record(elapsed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aggregate_time_preserves_precision_and_bounds_the_observation_samples() {
        let mut stage = ExecutionStageMeasurement::default();
        for _ in 0..10_000 {
            stage.record(std::time::Duration::from_nanos(750));
        }
        assert_eq!(stage.calls, 10_000);
        assert_eq!(stage.elapsed_us, 7_500);
        assert_eq!(stage.first_samples_us.len(), 64);
        let wire = serde_json::to_value(stage).unwrap();
        assert!(wire.get("remainder_ns").is_none());
        assert_eq!(wire["elapsed_us"], 7_500);
    }
    #[test]
    fn nested_scopes_restore_observers_after_unwind_and_disabled_work() {
        let outer = ExecutionDiagnosticsCollector::enabled();
        let inner = ExecutionDiagnosticsCollector::enabled();
        let _outer = outer.activate();
        count_execution("outer", 1);
        {
            let disabled = ExecutionDiagnosticsCollector::default();
            let _scope = disabled.activate();
            count_execution("disabled", 1);
        }
        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _inner = inner.activate();
            count_execution("inner", 1);
            panic!("restore measurement scope");
        }));
        assert!(unwind.is_err());
        count_execution("outer", 1);
        let outer = outer.snapshot().unwrap();
        assert_eq!(outer.counts["outer"], 2);
        assert!(!outer.counts.contains_key("inner"));
        assert!(!outer.counts.contains_key("disabled"));
        assert_eq!(inner.snapshot().unwrap().counts["inner"], 1);
    }
    #[test]
    fn disabled_collection_has_no_measurements() {
        let collector = ExecutionDiagnosticsCollector::default();
        let _timer = collector.time("unused");
        collector.count("unused", 1);
        collector.peak_payload("unused", 10);
        assert!(collector.snapshot().is_none());
    }
    #[test]
    fn scoped_threads_share_counts_and_peak_without_summing_peaks() {
        let collector = ExecutionDiagnosticsCollector::enabled();
        std::thread::scope(|scope| {
            for bytes in [10, 20, 5] {
                let collector = collector.clone();
                scope.spawn(move || {
                    let _timer = collector.time("encode");
                    collector.count("frames", 1);
                    collector.peak_payload("bundle", bytes);
                });
            }
        });
        let result = collector.snapshot().unwrap();
        assert_eq!(result.stages["encode"].calls, 3);
        assert_eq!(result.counts["frames"], 3);
        assert_eq!(result.peak_payload_bytes["bundle"], 20);
    }
}
