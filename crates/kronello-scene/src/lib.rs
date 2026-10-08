//! Deterministic scene-boundary scoring (ADR-0125) and smart-reframe window
//! math (ADR-0126). Integer luma and explicit integer scan order only:
//! identical inputs always produce identical results. No filesystem, clock,
//! randomness, external processes or ML models.

mod detect;
mod reframe;

pub use detect::{
    DetectedBoundary, SceneDetectError, SceneFrame, detect_boundaries, estimate_work,
};
pub use reframe::{REFRAME_MAX_LOST_WEIGHT_FRACTION, ReframeError, reframe_window};
