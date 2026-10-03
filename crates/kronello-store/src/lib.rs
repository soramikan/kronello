//! Synchronous SQLite document storage. Command policy and selective undo belong
//! to kronello-service; this crate never evaluates expressions or starts Tokio.
mod location;
mod store;

pub use location::{
    DetectedLocation, LocationDetector, OpenMode, OpenOptions, SystemLocationDetector,
    detect_location, render_cache_location,
};
pub use store::{
    ApplyRequest, ChangedKey, Event, HISTORY_WARNING_BYTES, HistorySize, IdempotencyRecord,
    Mutation, ProjectStore, Revision, Snapshot, StoreError,
};
