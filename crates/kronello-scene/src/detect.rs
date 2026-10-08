//! AI-002 (ADR-0125): deterministic cut-boundary detection on decoded RGBA8
//! frames. Score combines a luminance-histogram difference with an
//! edge-change score; an adaptive threshold emits boundaries. Pure integer
//! scan order, so identical inputs produce identical boundaries.

use kronello_model::SceneDetectionParams;
use kronello_time::Time;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Error)]
pub enum SceneDetectError {
    #[error("invalid scene detection request: {0}")]
    InvalidInput(String),
    #[error("scene detection work budget exceeded: {0}")]
    Budget(String),
}
impl SceneDetectError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput(_) => "INVALID_REQUEST",
            Self::Budget(_) => "SCENE_BUDGET_EXCEEDED",
        }
    }
}

/// One decoded source frame: opaque straight RGBA8 plus exact timing.
/// Successive frames must be in strictly increasing `time` order.
pub struct SceneFrame<'a> {
    pub time: Time,
    pub width: u32,
    pub height: u32,
    pub rgba: &'a [u8],
}

#[derive(Debug, Clone, PartialEq)]
pub struct DetectedBoundary {
    /// Presentation time of the first frame of the new scene.
    pub time: Time,
    /// Peak score normalized to `[0, 1]` against `255`.
    pub confidence: f64,
    /// Raw combined score in `[0, 255]` (histogram + edge terms).
    pub score: f64,
}

/// Worst-case integer work estimate; `None` when the bound overflows u64.
pub fn estimate_work(frames: u64, width: u32, height: u32) -> Option<u64> {
    let pixels = u64::from(width).checked_mul(u64::from(height))?;
    // Luma extraction plus two feature passes plus differencing.
    frames.checked_mul(pixels.checked_mul(4)?)
}

/// Fixed-point BT.601 luma from packed RGBA8: `(77r + 150g + 29b) >> 8`.
fn luma(frame: &SceneFrame<'_>) -> Vec<u8> {
    frame
        .rgba
        .chunks_exact(4)
        .map(|p| ((77 * p[0] as u32 + 150 * p[1] as u32 + 29 * p[2] as u32) >> 8) as u8)
        .collect()
}

/// 32-bin luminance histogram.
fn histogram(luma_data: &[u8]) -> [u64; 32] {
    let mut histogram = [0u64; 32];
    for &value in luma_data {
        histogram[(value >> 3) as usize] += 1;
    }
    histogram
}

/// Normalized sum of absolute histogram differences, scaled to `[0, 255]`.
fn histogram_score(previous: &[u64; 32], next: &[u64; 32], pixels: u64) -> f64 {
    if pixels == 0 {
        return 0.0;
    }
    let difference: u64 = previous
        .iter()
        .zip(next.iter())
        .map(|(a, b)| a.abs_diff(*b))
        .sum();
    // Full replacement differs by 2 * pixels.
    (difference as f64) / (2.0 * pixels as f64) * 255.0
}

/// Integer Sobel-style horizontal gradient magnitude, one `i32` per pixel.
fn gradient(luma_data: &[u8], width: u32, height: u32) -> Vec<i32> {
    let width = width as usize;
    let height = height as usize;
    let mut out = vec![0i32; luma_data.len()];
    if width < 3 || height < 3 {
        return out;
    }
    for y in 1..height - 1 {
        let row = y * width;
        for x in 1..width - 1 {
            let i = row + x;
            let gx = luma_data[i - width + 1] as i32
                + 2 * luma_data[i + 1] as i32
                + luma_data[i + width + 1] as i32
                - luma_data[i - width - 1] as i32
                - 2 * luma_data[i - 1] as i32
                - luma_data[i + width - 1] as i32;
            out[i] = gx.abs();
        }
    }
    out
}

/// Edge-change score: fraction of pixels whose edge magnitude crossed the
/// fixed threshold between frames, scaled to `[0, 255]`.
fn edge_score(previous: &[i32], next: &[i32], pixels: u64) -> f64 {
    if pixels == 0 {
        return 0.0;
    }
    const THRESHOLD: i32 = 128;
    let changed = previous
        .iter()
        .zip(next.iter())
        .filter(|(a, b)| (**a >= THRESHOLD) != (**b >= THRESHOLD))
        .count() as u64;
    (changed as f64) / (pixels as f64) * 255.0
}

struct Features {
    histogram: [u64; 32],
    gradient: Vec<i32>,
}

fn features(frame: &SceneFrame<'_>) -> Features {
    let luma_data = luma(frame);
    Features {
        histogram: histogram(&luma_data),
        gradient: gradient(&luma_data, frame.width, frame.height),
    }
}

/// Detect cut boundaries in `frames` (already inside the analyzed range,
/// strictly increasing in time, uniform dimensions). A boundary is reported
/// at `frames[i].time` when the transition `frames[i-1] -> frames[i]` is a
/// local score peak above the adaptive threshold.
///
/// Threshold: `mean + threshold_sigma * stddev` over all transition scores;
/// peaks below that line are still emitted when they dominate
/// `peak_ratio * mean` (safety net for low-variance material). A peak must
/// also sit at least `min_spacing_frames` transitions after the previous one.
pub fn detect_boundaries(
    frames: &[SceneFrame<'_>],
    params: &SceneDetectionParams,
) -> Result<Vec<DetectedBoundary>, SceneDetectError> {
    params
        .validate()
        .map_err(|e| SceneDetectError::InvalidInput(e.to_string()))?;
    if frames.len() > kronello_model::SCENE_DETECT_MAX_FRAMES as usize {
        return Err(SceneDetectError::Budget("frame count".into()));
    }
    let Some(first) = frames.first() else {
        return Ok(Vec::new());
    };
    if first.width == 0
        || first.height == 0
        || u64::from(first.width) * u64::from(first.height)
            > u64::from(kronello_model::SCENE_DETECT_MAX_PIXELS)
    {
        return Err(SceneDetectError::Budget("frame dimensions".into()));
    }
    if frames.iter().any(|f| {
        f.width != first.width
            || f.height != first.height
            || f.rgba.len() != first.width as usize * first.height as usize * 4
    }) {
        return Err(SceneDetectError::InvalidInput(
            "non-uniform or truncated frame".into(),
        ));
    }
    if frames.windows(2).any(|pair| pair[1].time <= pair[0].time) {
        return Err(SceneDetectError::InvalidInput(
            "frame times must be strictly increasing".into(),
        ));
    }

    let mut previous = features(first);
    let mut scores = Vec::with_capacity(frames.len().saturating_sub(1));
    for frame in &frames[1..] {
        let next = features(frame);
        let pixels = u64::from(first.width) * u64::from(first.height);
        let histogram = histogram_score(&previous.histogram, &next.histogram, pixels);
        let edge = edge_score(&previous.gradient, &next.gradient, pixels);
        let weight = params.edge_weight.get();
        scores.push(histogram * (1.0 - weight) + edge * weight);
        previous = next;
    }
    if scores.is_empty() {
        return Ok(Vec::new());
    }

    let mean = scores.iter().sum::<f64>() / scores.len() as f64;
    let variance =
        scores.iter().map(|s| (s - mean) * (s - mean)).sum::<f64>() / scores.len() as f64;
    let threshold = (mean + params.threshold_sigma.get() * variance.sqrt())
        .max(mean * params.peak_ratio.get().clamp(0.0, 1.0));
    let fallback = mean * params.peak_ratio.get();

    let spacing = params.min_spacing_frames as usize;
    let mut boundaries = Vec::new();
    let mut last_emitted: Option<usize> = None;
    for i in 1..scores.len() - 1 {
        let score = scores[i];
        let is_peak = score > scores[i - 1] && score >= scores[i + 1];
        let strong = score >= threshold || score >= fallback;
        if !is_peak || !strong {
            continue;
        }
        if last_emitted.is_some_and(|last| i - last < spacing) {
            continue;
        }
        last_emitted = Some(i);
        boundaries.push(DetectedBoundary {
            // Transition index `i` is frames[i] -> frames[i+1]; the boundary
            // sits at the first frame of the new scene.
            time: frames[i + 1].time,
            confidence: (score / 255.0).clamp(0.0, 1.0),
            score,
        });
    }
    // A single transition cannot be a strict local peak; accept a lone
    // transition and leading/trailing edges when they dominate the band.
    if scores.len() >= 2 {
        for (i, score) in [(0, scores[0]), (scores.len() - 1, scores[scores.len() - 1])] {
            let neighbor = if i == 0 { scores[1] } else { scores[i - 1] };
            if score <= neighbor
                || !(score >= threshold || score >= fallback)
                || last_emitted.is_some_and(|last| i.abs_diff(last) < spacing)
            {
                continue;
            }
            boundaries.push(DetectedBoundary {
                time: frames[i + 1].time,
                confidence: (score / 255.0).clamp(0.0, 1.0),
                score,
            });
        }
    } else if scores[0] >= 1.0 {
        boundaries.push(DetectedBoundary {
            time: frames[1].time,
            confidence: (scores[0] / 255.0).clamp(0.0, 1.0),
            score: scores[0],
        });
    }
    boundaries.sort_by_key(|b| b.time);
    boundaries.dedup_by(|a, b| a.time == b.time);
    Ok(boundaries)
}
