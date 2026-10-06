//! Request lifetime control, independent of persistent jobs and transport IDs.

/// Synchronous operations check before dispatch. Sequence rendering also checks
/// between frames and before manifest publication. A single frame, transaction,
/// or native codec call cannot be interrupted while it is executing.
pub trait ExecutionControl: Send + Sync {
    fn is_cancelled(&self) -> bool {
        false
    }
    fn progress(&self, _completed: u64, _total: u64) {}
}

impl ExecutionControl for () {}
