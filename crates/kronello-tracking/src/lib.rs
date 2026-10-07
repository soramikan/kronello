//! Deterministic point/plane motion tracking on decoded RGBA8 frames
//! (ADR-0118). Integer luma and explicit integer scan order only: identical
//! inputs always produce bit-identical results. No filesystem, clock or
//! randomness.

use kronello_model::{
    FiniteF64, TrackedFrame, TrackedPoint, TrackingDataAsset, TrackingMode, TrackingSeed,
    TrackingSource,
};
use kronello_time::{Rational, Time, TimeRange};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Error)]
pub enum TrackingError {
    #[error("invalid tracking request: {0}")]
    InvalidInput(String),
    #[error("tracking work budget exceeded: {0}")]
    Budget(String),
}
impl TrackingError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput(_) => "INVALID_REQUEST",
            Self::Budget(_) => "TRACKING_BUDGET_EXCEEDED",
        }
    }
}

/// One decoded source frame: opaque straight RGBA8 plus exact timing.
pub struct TrackingFrame<'a> {
    pub time: Time,
    pub width: u32,
    pub height: u32,
    pub rgba: &'a [u8],
}

/// Conservative synchronous-analysis ceiling; larger ranges need a future job.
pub const TRACKING_WORK_BUDGET: u64 = 8_000_000_000;

/// Worst-case integer work estimate; `None` when the bound overflows u64.
pub fn estimate_work(frames: u64, seeds: &[TrackingSeed], width: u32, height: u32) -> Option<u64> {
    let pixels = u64::from(width).checked_mul(u64::from(height))?;
    let mut per_frame = 0u64;
    for seed in seeds {
        let template = u64::from(seed.template_radius)
            .checked_mul(2)?
            .checked_add(1)?;
        let window = u64::from(seed.search_radius)
            .checked_mul(2)?
            .checked_add(1)?;
        per_frame = per_frame.checked_add(
            template
                .checked_mul(template)?
                .checked_mul(window.checked_mul(window)?)?,
        )?;
    }
    // Luma extraction is charged once per frame.
    frames.checked_mul(per_frame.checked_add(pixels)?)
}

fn invalid(reason: &str) -> TrackingError {
    TrackingError::InvalidInput(reason.into())
}

/// Fixed-point BT.601 luma from packed RGBA8: `(77r + 150g + 29b) >> 8`.
fn luma(frame: &TrackingFrame<'_>) -> Vec<u8> {
    frame
        .rgba
        .chunks_exact(4)
        .map(|p| ((77 * p[0] as u32 + 150 * p[1] as u32 + 29 * p[2] as u32) >> 8) as u8)
        .collect()
}

fn to_pixel(x: f64, extent: u32) -> i64 {
    (x * i64::from(extent).saturating_sub(1) as f64).round() as i64
}

fn to_normalized(x: i64, extent: u32) -> f64 {
    x as f64 / i64::from(extent).saturating_sub(1).max(1) as f64
}

/// Template patch around a pixel center; border pixels clamp to the edge so
/// seeds may sit at the frame boundary.
fn patch(gray: &[u8], width: u32, height: u32, cx: i64, cy: i64, radius: u32, out: &mut [u8]) {
    let (w, h, r) = (width as i64, height as i64, i64::from(radius));
    let t = 2 * r + 1;
    for ty in 0..t {
        for tx in 0..t {
            let x = (cx + tx - r).clamp(0, w - 1);
            let y = (cy + ty - r).clamp(0, h - 1);
            out[(ty * t + tx) as usize] = gray[(y * w + x) as usize];
        }
    }
}

/// Normalized cross-correlation of two equal-length patches. All sums are
/// i64; the single final division is deterministic.
fn ncc(template: &[u8], candidate: &[u8]) -> f64 {
    let n = template.len() as i64;
    let (mut st, mut sf, mut stt, mut sff, mut stf) = (0i64, 0i64, 0i64, 0i64, 0i64);
    for (t, f) in template.iter().zip(candidate) {
        let (t, f) = (i64::from(*t), i64::from(*f));
        st += t;
        sf += f;
        stt += t * t;
        sff += f * f;
        stf += t * f;
    }
    let den_a = stt * n - st * st;
    let den_b = sff * n - sf * sf;
    if den_a <= 0 || den_b <= 0 {
        // Constant patch: identical means correlate fully, otherwise zero.
        return if den_a <= 0 && den_b <= 0 && st == sf {
            1.0
        } else {
            0.0
        };
    }
    let num = (stf * n - st * sf) as f64;
    (num / ((den_a as f64) * (den_b as f64)).sqrt()).clamp(-1.0, 1.0)
}

/// Exhaustive integer scan over `±search_radius` around `prev`. Highest NCC
/// wins; ties resolve by smallest |dx|+|dy|, then lowest (dy, dx).
fn track_one(
    gray: &[u8],
    template: &[u8],
    width: u32,
    height: u32,
    seed: &TrackingSeed,
    prev: (f64, f64),
) -> TrackedPoint {
    let r = i64::from(seed.template_radius);
    let s = i64::from(seed.search_radius);
    let px = to_pixel(prev.0, width);
    let py = to_pixel(prev.1, height);
    let t = (2 * r + 1) as usize;
    let mut window = vec![0u8; t * t];
    // (score, distance, dy, dx); lexicographic compare keeps the best.
    let mut best: Option<(u64, i64, i64, i64)> = None;
    for dy in -s..=s {
        for dx in -s..=s {
            patch(
                gray,
                width,
                height,
                px + dx,
                py + dy,
                seed.template_radius,
                &mut window,
            );
            let score = ncc(template, &window);
            // Map [-1, 1] into a total order for tie-stable comparison.
            let key = (score + 1.0).to_bits();
            let dist = dx.abs() + dy.abs();
            let candidate = (key, -dist, -dy, -dx);
            if best.is_none_or(|b| candidate > b) {
                best = Some(candidate);
            }
        }
    }
    let (key, neg_dist, neg_dy, neg_dx) = best.expect("nonempty search window");
    let (dy, dx) = (-neg_dy, -neg_dx);
    let x = (px + dx).clamp(0, i64::from(width) - 1);
    let y = (py + dy).clamp(0, i64::from(height) - 1);
    let _ = neg_dist;
    TrackedPoint {
        x: FiniteF64::new(to_normalized(x, width)).expect("normalized coordinate"),
        y: FiniteF64::new(to_normalized(y, height)).expect("normalized coordinate"),
        confidence: FiniteF64::new(f64::from_bits(key) - 1.0).expect("bounded score"),
    }
}

/// Direct linear transform for exactly four point pairs, solved by Gaussian
/// elimination with fixed partial pivoting. Deterministic; `None` when the
/// system is degenerate.
pub fn homography4(from: [[f64; 2]; 4], to: [[f64; 2]; 4]) -> Option<[f64; 9]> {
    // Solve A h = b for the 8 unknowns of h (h22 = 1).
    let mut a = [[0.0; 9]; 8];
    let mut b = [0.0; 8];
    for i in 0..4 {
        let (x, y) = (from[i][0], from[i][1]);
        let (u, v) = (to[i][0], to[i][1]);
        a[2 * i] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, 0.0];
        a[2 * i + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, 0.0];
        b[2 * i] = u;
        b[2 * i + 1] = v;
    }
    let mut m = [[0.0; 9]; 8];
    for i in 0..8 {
        m[i][..8].copy_from_slice(&a[i][..8]);
        m[i][8] = b[i];
    }
    for col in 0..8 {
        let pivot = (col..8).max_by(|&i, &j| {
            m[i][col]
                .abs()
                .partial_cmp(&m[j][col].abs())
                .expect("finite")
        })?;
        if m[pivot][col].abs() < 1e-12 {
            return None;
        }
        m.swap(col, pivot);
        let d = m[col][col];
        for item in m[col].iter_mut() {
            *item /= d;
        }
        for row in 0..8 {
            if row == col {
                continue;
            }
            let f = m[row][col];
            let (row_ref, col_ref): (&mut [f64; 9], &[f64; 9]) = if row > col {
                let (lo, hi) = m.split_at_mut(row);
                (&mut hi[0], &lo[col])
            } else {
                let (lo, hi) = m.split_at_mut(col);
                (&mut lo[row], &hi[0])
            };
            for (cell, c) in row_ref.iter_mut().zip(col_ref.iter()) {
                *cell -= f * c;
            }
        }
    }
    let h = [
        m[0][8], m[1][8], m[2][8], m[3][8], m[4][8], m[5][8], m[6][8], m[7][8], 1.0,
    ];
    if h.iter().all(|v| v.is_finite()) {
        Some(h)
    } else {
        None
    }
}

/// Track `seeds` through `frames` (presentation order, at most
/// `TRACKING_MAX_FRAMES`) and assemble the document object. The first frame
/// anchors each template; `time` rows must lie inside `range`.
#[allow(clippy::too_many_arguments)]
pub fn analyze(
    id: kronello_model::AssetId,
    source: TrackingSource,
    mode: TrackingMode,
    seeds: &[TrackingSeed],
    range: TimeRange,
    sample_rate: Rational,
    width: u32,
    height: u32,
    frames: &[TrackingFrame<'_>],
) -> Result<TrackingDataAsset, TrackingError> {
    let expected = match mode {
        TrackingMode::Points => 1..=kronello_model::TRACKING_MAX_SEEDS,
        TrackingMode::Plane => 4..=4,
    };
    if width == 0
        || height == 0
        || !expected.contains(&seeds.len())
        || frames.is_empty()
        || frames.len() > kronello_model::TRACKING_MAX_FRAMES as usize
        || sample_rate <= Rational::ZERO
        || range.is_empty()
    {
        return Err(invalid("invalid tracking input bounds"));
    }
    for seed in seeds {
        seed.validate().map_err(|e| invalid(&e.to_string()))?;
    }
    if let Some(work) = estimate_work(frames.len() as u64, seeds, width, height) {
        if work > TRACKING_WORK_BUDGET {
            return Err(TrackingError::Budget(format!(
                "{work} exceeds {TRACKING_WORK_BUDGET}"
            )));
        }
    } else {
        return Err(TrackingError::Budget("work estimate overflow".into()));
    }
    for frame in frames {
        if frame.width != width
            || frame.height != height
            || frame.rgba.len() != width as usize * height as usize * 4
            || !range.contains(frame.time)
        {
            return Err(invalid("tracking frame shape or time"));
        }
    }
    // Templates anchor on the first tracked frame.
    let first = TrackingFrame {
        time: frames[0].time,
        width,
        height,
        rgba: frames[0].rgba,
    };
    let gray0 = luma(&first);
    let templates: Vec<Vec<u8>> = seeds
        .iter()
        .map(|seed| {
            let cx = to_pixel(seed.x.get(), width);
            let cy = to_pixel(seed.y.get(), height);
            let mut out = vec![0u8; ((2 * seed.template_radius + 1).pow(2)) as usize];
            patch(
                &gray0,
                width,
                height,
                cx,
                cy,
                seed.template_radius,
                &mut out,
            );
            out
        })
        .collect();
    let mut positions: Vec<(f64, f64)> = seeds.iter().map(|s| (s.x.get(), s.y.get())).collect();
    let anchor: [[f64; 2]; 4] =
        std::array::from_fn(|i| positions.get(i).map(|p| [p.0, p.1]).unwrap_or([0.0, 0.0]));
    let mut rows = Vec::with_capacity(frames.len());
    for frame in frames {
        let owned = TrackingFrame {
            time: frame.time,
            width,
            height,
            rgba: frame.rgba,
        };
        let gray = luma(&owned);
        let mut points = Vec::with_capacity(seeds.len());
        for (i, seed) in seeds.iter().enumerate() {
            let point = track_one(&gray, &templates[i], width, height, seed, positions[i]);
            positions[i] = (point.x.get(), point.y.get());
            points.push(point);
        }
        let homography = if mode == TrackingMode::Plane {
            let to: [[f64; 2]; 4] = std::array::from_fn(|i| [points[i].x.get(), points[i].y.get()]);
            homography4(anchor, to)
                .map(|h| h.map(|v| FiniteF64::new(v).expect("finite homography")))
        } else {
            None
        };
        rows.push(TrackedFrame {
            time: frame.time,
            points,
            homography,
        });
    }
    let mut asset = TrackingDataAsset {
        id,
        version: kronello_model::TRACKING_VERSION,
        source,
        mode,
        seeds: seeds.to_vec(),
        range,
        sample_rate,
        frames: rows,
        content_hash: String::new(),
    };
    asset.content_hash = asset.computed_hash().map_err(|e| invalid(&e.to_string()))?;
    asset.validate().map_err(|e| invalid(&e.to_string()))?;
    Ok(asset)
}
