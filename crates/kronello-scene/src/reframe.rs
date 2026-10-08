//! AI-003 (ADR-0126): confidence-weighted gaze centroid, deterministic
//! temporal smoothing and a target-aspect crop window clamped to the source.

use kronello_model::{ReframeEasing, SmartReframeSettings, TrackingDataAsset};
use kronello_time::Time;
use thiserror::Error;

/// All-lost frames carrying at least this fraction of the smoothing-window
/// weight fail `TRACKING_INSUFFICIENT` instead of silently drifting.
pub const REFRAME_MAX_LOST_WEIGHT_FRACTION: f64 = 0.5;

#[derive(Debug, Clone, PartialEq, Error)]
pub enum ReframeError {
    #[error("invalid smart reframe request: {0}")]
    InvalidInput(String),
    /// Versioned rule: no silent center crop when tracking loses its subject.
    #[error("tracking data insufficient at the requested source time")]
    Insufficient,
}
impl ReframeError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput(_) => "INVALID_REQUEST",
            Self::Insufficient => "TRACKING_INSUFFICIENT",
        }
    }
}

fn seconds(time: Time) -> f64 {
    time.numerator() as f64 / time.denominator() as f64
}

fn ease(easing: ReframeEasing, u: f64) -> f64 {
    match easing {
        ReframeEasing::Linear => u,
        ReframeEasing::EaseIn => u * u,
        ReframeEasing::EaseOut => 1.0 - (1.0 - u) * (1.0 - u),
        ReframeEasing::EaseInOut => u * u * (3.0 - 2.0 * u),
    }
}

/// Largest `[width, height]` rectangle with aspect `target` inside `source`.
fn fitted_window(source: [f64; 2], target: f64) -> [f64; 2] {
    let by_width = source[0] / target;
    if by_width <= source[1] {
        [source[0], by_width]
    } else {
        [source[1] * target, source[1]]
    }
}

/// Crop window `[x, y, width, height]` in source pixels for `source_time`.
///
/// Gaze is the confidence-weighted centroid of the selected tracked points
/// inside `settings.smoothing_window` around `source_time`; each sample is
/// time-weighted by `1 - ease(|dt| / window)`. A frame is *all-lost* when
/// every selected point is missing or has a nonpositive confidence. The
/// all-lost weight fraction must stay below
/// [`REFRAME_MAX_LOST_WEIGHT_FRACTION`] and at least one valid sample must
/// exist; otherwise [`ReframeError::Insufficient`] (never a center crop).
///
/// The window size is `fitted * scale` with
/// `scale = (1 - padding) / max_zoom + padding`, so `padding = 0` is the
/// tightest allowed crop and `padding = 1` the largest target-aspect window
/// fitting the source; the gaze-centered window is then clamped inside the
/// source bounds.
pub fn reframe_window(
    tracking: &TrackingDataAsset,
    settings: &SmartReframeSettings,
    source_time: Time,
    source_size: [f64; 2],
) -> Result<[f64; 4], ReframeError> {
    settings
        .validate()
        .map_err(|e| ReframeError::InvalidInput(e.to_string()))?;
    tracking
        .validate()
        .map_err(|e| ReframeError::InvalidInput(e.to_string()))?;
    if !source_size[0].is_finite()
        || !source_size[1].is_finite()
        || source_size[0] <= 0.0
        || source_size[1] <= 0.0
    {
        return Err(ReframeError::InvalidInput("invalid source size".into()));
    }
    // `TrackedFrame::points` parallels `seeds`: index `i` tracks seed `i`.
    let selected: std::collections::BTreeSet<usize> = if settings.seeds.is_empty() {
        (0..tracking.seeds.len()).collect()
    } else {
        let mut selected = std::collections::BTreeSet::new();
        for &index in &settings.seeds {
            if index as usize >= tracking.seeds.len() {
                return Err(ReframeError::InvalidInput(
                    "smart reframe seed index out of range".into(),
                ));
            }
            selected.insert(index as usize);
        }
        selected
    };
    if selected.is_empty() {
        return Err(ReframeError::InvalidInput(
            "smart reframe selects no seeds".into(),
        ));
    }

    let window = settings.smoothing_window.as_time();
    let window_seconds = seconds(window);
    let mut numerator = [0.0f64; 2];
    let mut weight_sum = 0.0;
    let mut lost_weight = 0.0;
    let mut total_weight = 0.0;
    for frame in &tracking.frames {
        let offset = frame
            .time
            .checked_sub(source_time)
            .map(|dt| seconds(dt).abs())
            .map_err(|_| ReframeError::InvalidInput("source time overflow".into()))?;
        if offset > window_seconds {
            continue;
        }
        let u = if window_seconds > 0.0 {
            (offset / window_seconds).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let time_weight = 1.0 - ease(settings.easing, u);
        if time_weight <= 0.0 {
            continue;
        }
        total_weight += time_weight;
        let mut frame_valid = false;
        for (index, point) in frame.points.iter().enumerate() {
            if !selected.contains(&index) {
                continue;
            }
            // A nonpositive correlation score is the "point lost" signal;
            // missing selected entries count as lost for the frame too.
            if point.confidence.get() > 0.0 {
                let w = time_weight * point.confidence.get();
                numerator[0] += point.x.get() * w;
                numerator[1] += point.y.get() * w;
                weight_sum += w;
                frame_valid = true;
            }
        }
        if !frame_valid {
            lost_weight += time_weight;
        }
    }
    if weight_sum <= 0.0 || lost_weight >= total_weight * REFRAME_MAX_LOST_WEIGHT_FRACTION {
        return Err(ReframeError::Insufficient);
    }
    let gaze = [numerator[0] / weight_sum, numerator[1] / weight_sum];

    let fitted = fitted_window(source_size, settings.target_aspect.get());
    let scale = (1.0 - settings.padding.get()) / settings.max_zoom.get() + settings.padding.get();
    let size = [fitted[0] * scale, fitted[1] * scale];
    let center = [gaze[0] * source_size[0], gaze[1] * source_size[1]];
    let x = (center[0] - size[0] * 0.5).clamp(0.0, (source_size[0] - size[0]).max(0.0));
    let y = (center[1] - size[1] * 0.5).clamp(0.0, (source_size[1] - size[1]).max(0.0));
    Ok([x, y, size[0], size[1]])
}
