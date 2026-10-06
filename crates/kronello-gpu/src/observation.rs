//! Reentrant request ownership serializes renderer threads around cumulative
//! observations. No mutex is held while commands, decoder callbacks or waits run.
use std::sync::{Arc, Condvar, Mutex};
use std::thread::ThreadId;
use std::time::{Duration, Instant};
#[derive(Default)]
struct OwnerState {
    owner: Option<ThreadId>,
    depth: u64,
}
#[derive(Default)]
pub(crate) struct ObservationState {
    owner: Mutex<OwnerState>,
    available: Condvar,
}
pub(crate) struct RenderScope {
    state: Arc<ObservationState>,
    owner: ThreadId,
    _thread_bound: std::marker::PhantomData<std::rc::Rc<()>>,
}
impl kronello_render::RenderObservationScope for RenderScope {}
impl Drop for RenderScope {
    fn drop(&mut self) {
        let mut state = self.state.owner.lock().unwrap_or_else(|e| e.into_inner());
        if state.owner == Some(self.owner) {
            state.depth -= 1;
            if state.depth == 0 {
                state.owner = None;
                self.state.available.notify_all();
            }
        }
    }
}
impl crate::GpuContext {
    pub(crate) fn render_scope(&self) -> Result<RenderScope, crate::GpuError> {
        self.acquire_observation(Duration::from_secs(30))
    }
    pub(crate) fn try_render_scope(&self) -> Result<RenderScope, crate::GpuError> {
        self.acquire_observation(Duration::ZERO)
    }
    pub fn observation_scope(
        &self,
    ) -> Result<Box<dyn kronello_render::RenderObservationScope + '_>, crate::GpuError> {
        Ok(Box::new(self.render_scope()?))
    }
    /// Explicit nonblocking observation ownership. Shared frame rendering uses
    /// bounded serialization instead, so ordinary peer requests can both succeed.
    pub fn try_observation_scope(
        &self,
    ) -> Result<Box<dyn kronello_render::RenderObservationScope + '_>, crate::GpuError> {
        Ok(Box::new(self.try_render_scope()?))
    }
    fn acquire_observation(&self, timeout: Duration) -> Result<RenderScope, crate::GpuError> {
        let owner = std::thread::current().id();
        let deadline = Instant::now() + timeout;
        let mut state = self
            .observation
            .owner
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        while state.owner.is_some_and(|current| current != owner) {
            let Some(remaining) = deadline
                .checked_duration_since(Instant::now())
                .filter(|remaining| !remaining.is_zero())
            else {
                return Err(crate::GpuError::ObservationBusy);
            };
            let (next, _) = self
                .observation
                .available
                .wait_timeout(state, remaining)
                .unwrap_or_else(|e| e.into_inner());
            state = next;
        }
        state.owner = Some(owner);
        state.depth += 1;
        Ok(RenderScope {
            state: self.observation.clone(),
            owner,
            _thread_bound: std::marker::PhantomData,
        })
    }
}
