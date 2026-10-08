//! TRACK-003 (ADR-0123): deterministic dense optical flow and intermediate
//! frame synthesis.
//!
//! Algorithm: integer-sum pyramidal block matching over fixed-point BT.601
//! luma. Levels are 2x2 box averages; the coarsest level performs an
//! exhaustive integer-window scan of `±search_radius`, and each finer level
//! refines the upsampled estimate within `±2` pixels. Ties resolve by
//! smallest Manhattan distance then lowest (dy, dx), mirroring `track_one`.
//! Per-cell confidence multiplies the base-level match NCC by a
//! forward/backward consistency term `clamp(1 - |residual| / block, 0, 1)`.
//! All sums are exact integers; a single final division produces each scalar.
//! No randomness, wall clock, or external process.

use kronello_time::{OpticalFlowConfig, Rational};
use thiserror::Error;

/// Versioned flow contract; stored in serialized diagnostics and part of the
/// content identity wherever a flow field is recorded.
pub const FLOW_FIELD_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq)]
pub struct FlowField {
    pub version: u32,
    /// Matching block half-size used to build this field.
    pub block_radius: u32,
    /// Cell grid `[columns, rows]` covering the frame at `2*block_radius+1`
    /// pixels per cell (ceil division; edge cells extend past the border).
    pub grid: [u32; 2],
    /// Per-cell motion `[dx, dy]` in `from`-frame pixels, row-major.
    pub vectors: Vec<[f32; 2]>,
    /// Per-cell combined confidence in `[0, 1]`.
    pub confidence: Vec<f32>,
}

impl FlowField {
    /// Fraction of cells whose confidence is below `floor`. Exact count over
    /// exact count, returned as f64 for threshold comparison.
    pub fn low_confidence_ratio(&self, floor: f64) -> f64 {
        let floor = floor as f32;
        let low = self.confidence.iter().filter(|&&c| c < floor).count();
        if self.confidence.is_empty() {
            0.0
        } else {
            low as f64 / self.confidence.len() as f64
        }
    }
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum FlowError {
    #[error("optical-flow confidence too low: {low_ratio} below threshold")]
    ConfidenceLow { low_ratio: f64 },
    #[error("invalid optical-flow input: {0}")]
    InvalidInput(String),
}

impl FlowError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::ConfidenceLow { .. } => "FLOW_CONFIDENCE_LOW",
            Self::InvalidInput(_) => "INVALID_INPUT",
        }
    }
}

fn invalid(reason: &str) -> FlowError {
    FlowError::InvalidInput(reason.into())
}

fn to_f64(v: Rational) -> f64 {
    v.numerator() as f64 / v.denominator() as f64
}

/// 2x2 box downsample with deterministic rounding; minimum size 1.
fn downsample(gray: &[u8], width: u32, height: u32) -> (Vec<u8>, u32, u32) {
    let (nw, nh) = ((width / 2).max(1), (height / 2).max(1));
    let mut out = vec![0u8; (nw * nh) as usize];
    for y in 0..nh {
        for x in 0..nw {
            let (sx, sy) = (2 * x, 2 * y);
            let a = gray[(sy * width + sx) as usize] as u32;
            let b = gray[(sy * width + (sx + 1).min(width - 1)) as usize] as u32;
            let c = gray[((sy + 1).min(height - 1) * width + sx) as usize] as u32;
            let d =
                gray[((sy + 1).min(height - 1) * width + (sx + 1).min(width - 1)) as usize] as u32;
            out[(y * nw + x) as usize] = ((a + b + c + d + 2) >> 2) as u8;
        }
    }
    (out, nw, nh)
}

/// Clamped block sample; border taps replicate the edge pixel.
fn block_at(gray: &[u8], width: u32, height: u32, cx: i64, cy: i64, r: u32) -> Vec<u8> {
    let (w, h, r) = (width as i64, height as i64, r as i64);
    let side = (2 * r + 1) as usize;
    let mut out = vec![0u8; side * side];
    for ty in 0..side as i64 {
        for tx in 0..side as i64 {
            let x = (cx + tx - r).clamp(0, w - 1);
            let y = (cy + ty - r).clamp(0, h - 1);
            out[(ty * side as i64 + tx) as usize] = gray[(y * w + x) as usize];
        }
    }
    out
}

/// Sum of absolute differences of two equal-length blocks; exact i64.
fn sad(a: &[u8], b: &[u8]) -> i64 {
    a.iter()
        .zip(b)
        .map(|(&x, &y)| (i64::from(x) - i64::from(y)).abs())
        .sum()
}

/// Normalized cross-correlation in [-1, 1]; constant patches compare by
/// equality like `ncc` in the point tracker.
fn ncc(a: &[u8], b: &[u8]) -> f64 {
    let n = a.len() as i64;
    let (mut sa, mut sb, mut saa, mut sbb, mut sab) = (0i64, 0i64, 0i64, 0i64, 0i64);
    for (&x, &y) in a.iter().zip(b) {
        let (x, y) = (i64::from(x), i64::from(y));
        sa += x;
        sb += y;
        saa += x * x;
        sbb += y * y;
        sab += x * y;
    }
    let da = saa * n - sa * sa;
    let db = sbb * n - sb * sb;
    if da <= 0 || db <= 0 {
        return if da <= 0 && db <= 0 && sa == sb {
            1.0
        } else {
            0.0
        };
    }
    let num = (sab * n - sa * sb) as f64;
    (num / ((da as f64) * (db as f64)).sqrt()).clamp(-1.0, 1.0)
}

/// Exhaustive integer scan around `(cx + fx, cy + fy)` within `±search`,
/// returning the best `(dx, dy)` relative to the cell center. Scan order is
/// increasing dy then dx; ties keep the smallest Manhattan distance, then the
/// lowest (dy, dx) — identical ordering to the seed tracker.
#[allow(clippy::too_many_arguments)]
fn match_cell(
    from: &[u8],
    to: &[u8],
    width: u32,
    height: u32,
    cx: i64,
    cy: i64,
    init: (i64, i64),
    r: u32,
    search: i64,
) -> (i64, i64) {
    let template = block_at(from, width, height, cx, cy, r);
    let mut best: Option<(i64, i64, i64, i64)> = None; // (sad, dist, dy, dx)
    for dy in -search..=search {
        for dx in -search..=search {
            let cand = block_at(to, width, height, cx + init.0 + dx, cy + init.1 + dy, r);
            let score = sad(&template, &cand);
            let dist = (init.0 + dx).abs() + (init.1 + dy).abs();
            // Lower score wins; ties: smaller distance then (dy, dx).
            let candidate = (-score, -dist, -dy, -dx);
            if best.is_none_or(|b| candidate > b) {
                best = Some(candidate);
            }
        }
    }
    let (_, _, neg_dy, neg_dx) = best.expect("nonempty search window");
    (init.0 - neg_dx, init.1 - neg_dy)
}

/// Estimate `from -> to` block motion at `config` parameters. The returned
/// field's `confidence` holds the base-level match NCC mapped to `[0, 1]`;
/// `consistency_combine` folds in the forward/backward residual.
pub fn estimate_flow(
    from: &[u8],
    to: &[u8],
    width: u32,
    height: u32,
    config: &OpticalFlowConfig,
) -> Result<FlowField, FlowError> {
    config.validate().map_err(|e| invalid(&e.to_string()))?;
    if width == 0
        || height == 0
        || from.len() != (width as usize) * (height as usize)
        || to.len() != (width as usize) * (height as usize)
    {
        return Err(invalid("flow frame shape"));
    }
    let block = 2 * config.block_radius + 1;
    // Shared pyramid for both frames; level 0 is the original.
    let mut from_levels = vec![(from.to_vec(), width, height)];
    let mut to_levels = vec![(to.to_vec(), width, height)];
    for _ in 1..config.levels {
        let (ref g, w, h) = *from_levels.last().expect("base level");
        from_levels.push(downsample(g, w, h));
        let (ref g, w, h) = *to_levels.last().expect("base level");
        to_levels.push(downsample(g, w, h));
    }
    // Coarser-level cells and their grid, carried into the next refinement.
    let mut prev: Option<(Vec<[i64; 2]>, u32)> = None;
    let mut field = None;
    for level in (0..config.levels as usize).rev() {
        let (ref fg, fw, fh) = from_levels[level];
        let (ref tg, _, _) = to_levels[level];
        let lcols = fw.div_ceil(block);
        let lrows = fh.div_ceil(block);
        let center =
            |i: u32, extent: u32| ((i * block + config.block_radius).min(extent - 1)) as i64;
        let mut cells = vec![[0i64; 2]; (lcols * lrows) as usize];
        for j in 0..lrows {
            for i in 0..lcols {
                let (cx, cy) = (center(i, fw), center(j, fh));
                // The coarsest level scans the full authored window from a
                // zero estimate; finer levels refine the doubled coarser
                // estimate within a fixed ±2 window.
                let (init, search) = match &prev {
                    None => ((0, 0), i64::from(config.search_radius)),
                    Some((coarse, ccols)) => {
                        let (ci, cj) = ((i / 2).min(ccols - 1), (j / 2));
                        let crows = coarse.len() as u32 / ccols;
                        let est = coarse[(cj.min(crows - 1) * ccols + ci) as usize];
                        ((2 * est[0], 2 * est[1]), 2)
                    }
                };
                let (dx, dy) =
                    match_cell(fg, tg, fw, fh, cx, cy, init, config.block_radius, search);
                cells[(j * lcols + i) as usize] = [dx, dy];
            }
        }
        if level == 0 {
            let mut vectors = Vec::with_capacity(cells.len());
            let mut confidence = Vec::with_capacity(cells.len());
            for j in 0..lrows {
                for i in 0..lcols {
                    let (cx, cy) = (center(i, fw), center(j, fh));
                    let [dx, dy] = cells[(j * lcols + i) as usize];
                    let template = block_at(fg, fw, fh, cx, cy, config.block_radius);
                    let matched = block_at(tg, fw, fh, cx + dx, cy + dy, config.block_radius);
                    vectors.push([dx as f32, dy as f32]);
                    confidence.push(((ncc(&template, &matched) + 1.0) / 2.0) as f32);
                }
            }
            field = Some(FlowField {
                version: FLOW_FIELD_VERSION,
                block_radius: config.block_radius,
                grid: [lcols, lrows],
                vectors,
                confidence,
            });
        }
        prev = Some((cells, lcols));
    }
    field.ok_or_else(|| invalid("flow pyramid produced no base level"))
}

/// Fold forward/backward residuals into `fwd.confidence`: each cell's
/// confidence multiplies by `clamp(1 - |residual| / block, 0, 1)` where the
/// residual is `fwd[i] + bwd[cell(p + fwd[i])]` at the from-frame cell.
/// `from`/`to` here refer to the direction each field was estimated in.
pub fn consistency_combine(fwd: &mut FlowField, bwd: &FlowField) {
    let (cols, rows) = (fwd.grid[0], fwd.grid[1]);
    let block = (2 * fwd.block_radius + 1) as f32;
    let (bcols, brows) = (bwd.grid[0], bwd.grid[1]);
    for j in 0..rows {
        for i in 0..cols {
            let idx = (j * cols + i) as usize;
            let v = fwd.vectors[idx];
            let (cx, cy) = (
                i * (2 * fwd.block_radius + 1) + fwd.block_radius,
                j * (2 * fwd.block_radius + 1) + fwd.block_radius,
            );
            let (px, py) = ((cx as f32 + v[0]) as i64, (cy as f32 + v[1]) as i64);
            let cell = 2 * bwd.block_radius as i64 + 1;
            let (bi, bj) = (
                px.div_euclid(cell).clamp(0, bcols as i64 - 1),
                py.div_euclid(cell).clamp(0, brows as i64 - 1),
            );
            let bv = bwd.vectors[(bj as u32 * bcols + bi as u32) as usize];
            let residual = ((v[0] + bv[0]).powi(2) + (v[1] + bv[1]).powi(2)).sqrt();
            let consistency = (1.0 - residual / block).clamp(0.0, 1.0);
            fwd.confidence[idx] *= consistency;
        }
    }
}

/// Low-confidence fraction of `fwd` and `bwd` combined. Cells below
/// `config.confidence_floor` are low; the ratio uses the larger of the two
/// fields' low fractions.
pub fn low_confidence_ratio(fwd: &FlowField, bwd: &FlowField, config: &OpticalFlowConfig) -> f64 {
    let floor = to_f64(config.confidence_floor);
    fwd.low_confidence_ratio(floor)
        .max(bwd.low_confidence_ratio(floor))
}

/// Reject when the combined low-confidence fraction exceeds
/// `config.max_low_confidence`; the authored fallback policy selects
/// crossfade over typed rejection.
pub fn confidence_gate(
    fwd: &FlowField,
    bwd: &FlowField,
    config: &OpticalFlowConfig,
) -> Result<f64, FlowError> {
    let ratio = low_confidence_ratio(fwd, bwd, config);
    if ratio > to_f64(config.max_low_confidence) {
        return Err(FlowError::ConfidenceLow { low_ratio: ratio });
    }
    Ok(ratio)
}
