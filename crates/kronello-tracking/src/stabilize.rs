//! TRACK-002 (ADR-0122): tracking-data-driven stabilization transforms.
//!
//! Each tracked frame reduces to a source-space similarity `S: seed -> tracked
//! position` (translation + rotation + uniform scale). The correction
//! `C = S_smooth ∘ S_raw^-1` — composed about the frame center — is clamped by
//! the authored displacement/rotation limits and validated against `max_crop`.
//! Normalized tracked coordinates convert to the source pixel lattice with
//! `x_extent = x * (extent - 1) + 0.5`, matching the decoder's center lattice.
//!
//! Determinism: fixed index-order scans, unweighted least squares over points
//! with `confidence > 0`, and a single similarity solve per frame. No random
//! sampling, wall clock, or external process.

use kronello_model::TrackingDataAsset;
use kronello_time::Time;
use thiserror::Error;

/// Evaluation parameters already resolved by the effect layer. All magnitudes
/// are source-frame pixels; `max_rotation` is degrees; `max_crop` is the unit
/// interval fraction of the frame area allowed to lose coverage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StabilizeParams {
    /// Symmetric smoothing half-window in tracked samples.
    pub smoothing_radius: u32,
    /// Correction displacement magnitude limit, in source pixels.
    pub max_displacement: f64,
    /// Correction rotation magnitude limit, in degrees.
    pub max_rotation: f64,
    /// Maximum fraction of the output frame that may be uncovered.
    pub max_crop: f64,
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum StabilizeError {
    /// Referenced data is absent, malformed, stale, or has no sample at the
    /// requested source time.
    #[error("tracking data missing or out of range")]
    MissingData,
    /// Every usable tracked sample is lost inside the smoothing window.
    #[error("insufficient usable tracking samples")]
    Insufficient,
    /// The clamped correction uncovers more than `max_crop` of the frame.
    #[error("stabilize crop {uncovered} exceeds max_crop {max_crop}")]
    CropExceeded { uncovered: f64, max_crop: f64 },
    /// Invalid numeric input (non-finite extent or parameters).
    #[error("invalid stabilize input: {0}")]
    InvalidInput(String),
}

impl StabilizeError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::MissingData => "TRACKING_DATA_MISSING",
            Self::Insufficient => "TRACKING_INSUFFICIENT",
            Self::CropExceeded { .. } => "STABILIZE_CROP_EXCEEDED",
            Self::InvalidInput(_) => "INVALID_INPUT",
        }
    }
}

/// Per-frame similarity `(tx, ty, a, b)` where the linear part is
/// `[[a, -b], [b, a]]` mapping seed positions onto observed positions, both in
/// extent pixel space. `None` marks a lost frame.
type Similarity = Option<[f64; 4]>;

fn similarity_fit(seeds: &[[f64; 2]], observed: &[[f64; 2]]) -> Similarity {
    let n = seeds.len() as f64;
    let cr = [
        seeds.iter().map(|p| p[0]).sum::<f64>() / n,
        seeds.iter().map(|p| p[1]).sum::<f64>() / n,
    ];
    let co = [
        observed.iter().map(|p| p[0]).sum::<f64>() / n,
        observed.iter().map(|p| p[1]).sum::<f64>() / n,
    ];
    if seeds.len() < 2 {
        return Some([co[0] - cr[0], co[1] - cr[1], 1.0, 0.0]);
    }
    let (mut num_re, mut num_im, mut den) = (0.0, 0.0, 0.0);
    for (r, o) in seeds.iter().zip(observed) {
        let (rx, ry) = (r[0] - cr[0], r[1] - cr[1]);
        let (ox, oy) = (o[0] - co[0], o[1] - co[1]);
        num_re += ox * rx + oy * ry;
        num_im += oy * rx - ox * ry;
        den += rx * rx + ry * ry;
    }
    if den < 1e-12 {
        // Identical reference points: the best similarity is a translation.
        return Some([co[0] - cr[0], co[1] - cr[1], 1.0, 0.0]);
    }
    let (a, b) = (num_re / den, num_im / den);
    if a * a + b * b < 1e-18 {
        // Observed points collapsed while the seeds did not; the frame is
        // unusable rather than a near-zero scale warp.
        return None;
    }
    Some([
        co[0] - (a * cr[0] - b * cr[1]),
        co[1] - (b * cr[0] + a * cr[1]),
        a,
        b,
    ])
}

/// Rational time fraction `t` within `[t0, t1]`, exact at the endpoints.
fn fraction(t: Time, t0: Time, t1: Time) -> f64 {
    if t1 == t0 {
        return 0.0;
    }
    let span = t1.checked_sub(t0).expect("ordered bracket");
    let elapsed = t.checked_sub(t0).expect("ordered bracket");
    let f = elapsed.checked_div(span).expect("nonzero span");
    f.numerator() as f64 / f.denominator() as f64
}

fn lerp_params(a: [f64; 4], b: [f64; 4], f: f64) -> [f64; 4] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * f)
}

/// Interpolate the per-frame table at `t`, linear between the nearest valid
/// samples. Lost samples are skipped; a one-sided bracket holds the nearest
/// valid value. `None` when no valid sample exists at all.
fn sample_series(series: &[Similarity], times: &[Time], t: Time) -> Option<[f64; 4]> {
    let hi = times.partition_point(|&ft| ft <= t);
    let lo_valid = |mut i: usize| -> Option<usize> {
        loop {
            if series[i].is_some() {
                return Some(i);
            }
            if i == 0 {
                return None;
            }
            i -= 1;
        }
    };
    let hi_valid = |mut i: usize| -> Option<usize> {
        while i < series.len() {
            if series[i].is_some() {
                return Some(i);
            }
            i += 1;
        }
        None
    };
    let lo = if hi > 0 { lo_valid(hi - 1) } else { None };
    let hi = hi_valid(hi);
    match (lo, hi) {
        (Some(a), Some(b)) if a != b => {
            let f = fraction(t, times[a], times[b]);
            match (series[a], series[b]) {
                (Some(pa), Some(pb)) => Some(lerp_params(pa, pb, f)),
                _ => series[a].or(series[b]),
            }
        }
        (Some(a), _) => series[a],
        (None, Some(b)) => series[b],
        (None, None) => None,
    }
}

/// Convex clip of `polygon` against the rectangle `[0,w] x [0,h]`
/// (Sutherland-Hodgman) returning the clipped polygon's shoelace area.
fn clipped_area(poly: &[[f64; 2]], w: f64, h: f64) -> f64 {
    let mut current = poly.to_vec();
    let clip = |poly: &mut Vec<[f64; 2]>,
                inside: &dyn Fn([f64; 2]) -> bool,
                edge: &dyn Fn([f64; 2], [f64; 2]) -> [f64; 2]| {
        if poly.is_empty() {
            return;
        }
        let mut out = Vec::with_capacity(poly.len() + 1);
        let mut prev = *poly.last().expect("nonempty");
        for &cur in poly.iter() {
            if inside(cur) {
                if !inside(prev) {
                    out.push(edge(prev, cur));
                }
                out.push(cur);
            } else if inside(prev) {
                out.push(edge(prev, cur));
            }
            prev = cur;
        }
        *poly = out;
    };
    // Intersect each segment with one axis-aligned clip line at a time.
    clip(&mut current, &|p| p[0] >= 0.0, &|a, b| {
        let t = (0.0 - a[0]) / (b[0] - a[0]);
        [0.0, a[1] + (b[1] - a[1]) * t]
    });
    clip(&mut current, &|p| p[0] <= w, &|a, b| {
        let t = (w - a[0]) / (b[0] - a[0]);
        [w, a[1] + (b[1] - a[1]) * t]
    });
    clip(&mut current, &|p| p[1] >= 0.0, &|a, b| {
        let t = (0.0 - a[1]) / (b[1] - a[1]);
        [a[0] + (b[0] - a[0]) * t, 0.0]
    });
    clip(&mut current, &|p| p[1] <= h, &|a, b| {
        let t = (h - a[1]) / (b[1] - a[1]);
        [a[0] + (b[0] - a[0]) * t, h]
    });
    let mut area = 0.0;
    for i in 0..current.len() {
        let j = (i + 1) % current.len();
        area += current[i][0] * current[j][1] - current[j][0] * current[i][1];
    }
    area.abs() / 2.0
}

/// TRACK-002: inverse correction in source extent pixel space for the source
/// time `t`, as a row-major 2x3 affine `[[a, b, tx], [d, e, ty]]` mapping
/// output-frame positions onto corrected source positions.
///
/// `extent` is the video frame size in source pixels. `data` must already be
/// structurally validated and content-locked to the source asset by the
/// caller; failures map to the ADR-0122 typed codes.
pub fn correction_inverse(
    data: &TrackingDataAsset,
    t: Time,
    params: &StabilizeParams,
    extent: [f64; 2],
) -> Result<[[f64; 3]; 2], StabilizeError> {
    let [w, h] = extent;
    if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) {
        return Err(StabilizeError::InvalidInput(
            "non-finite frame extent".into(),
        ));
    }
    if ![
        params.max_displacement,
        params.max_rotation,
        params.max_crop,
    ]
    .iter()
    .all(|v| v.is_finite())
        || params.max_displacement < 0.0
        || params.max_rotation < 0.0
        || !(0.0..=1.0).contains(&params.max_crop)
    {
        return Err(StabilizeError::InvalidInput(
            "invalid stabilize limits".into(),
        ));
    }
    let n = data.frames.len();
    if n == 0 || t < data.frames[0].time || t > data.frames[n - 1].time {
        return Err(StabilizeError::MissingData);
    }
    // Per-frame reference -> observed similarity in extent pixel space.
    let to_extent = |v: f64, extent: f64| v * (extent - 1.0) + 0.5;
    let seeds_extent: Vec<[f64; 2]> = data
        .seeds
        .iter()
        .map(|s| [to_extent(s.x.get(), w), to_extent(s.y.get(), h)])
        .collect();
    let raw: Vec<Similarity> = data
        .frames
        .iter()
        .map(|frame| {
            let usable: Vec<usize> = (0..frame.points.len())
                .filter(|&i| frame.points[i].confidence.get() > 0.0)
                .collect();
            if usable.is_empty() {
                return None;
            }
            let refs: Vec<[f64; 2]> = usable.iter().map(|&i| seeds_extent[i]).collect();
            let obs: Vec<[f64; 2]> = usable
                .iter()
                .map(|&i| {
                    [
                        to_extent(frame.points[i].x.get(), w),
                        to_extent(frame.points[i].y.get(), h),
                    ]
                })
                .collect();
            similarity_fit(&refs, &obs)
        })
        .collect();
    let times: Vec<Time> = data.frames.iter().map(|f| f.time).collect();
    // Truncated box smoothing over the symmetric sample window; only valid
    // samples contribute, in ascending index order.
    let radius = params.smoothing_radius as usize;
    let smoothed: Vec<Similarity> = (0..n)
        .map(|j| {
            raw[j].map(|_| {
                let mut sum = [0.0; 4];
                let mut count = 0.0;
                for p in raw
                    .iter()
                    .take((j.saturating_add(radius)).min(n - 1) + 1)
                    .skip(j.saturating_sub(radius))
                    .flatten()
                {
                    for (s, &v) in sum.iter_mut().zip(p.iter()) {
                        *s += v;
                    }
                    count += 1.0;
                }
                sum.map(|v| v / count)
            })
        })
        .collect();
    let Some(raw_t) = sample_series(&raw, &times, t) else {
        return Err(StabilizeError::Insufficient);
    };
    let Some(smooth_t) = sample_series(&smoothed, &times, t) else {
        return Err(StabilizeError::Insufficient);
    };
    // C = S_smooth . S_raw^-1, complex form. w = a + bi is the linear part.
    let [tx_r, ty_r, ar, br] = raw_t;
    let [tx_s, ty_s, as_, bs] = smooth_t;
    let den = ar * ar + br * br;
    if den < 1e-18 {
        return Err(StabilizeError::Insufficient);
    }
    let wc = [(as_ * ar + bs * br) / den, (bs * ar - as_ * br) / den];
    // t_c = t_s - w_c * t_r (complex multiply on the translation pair).
    let tc = [
        tx_s - (wc[0] * tx_r - wc[1] * ty_r),
        ty_s - (wc[1] * tx_r + wc[0] * ty_r),
    ];
    // Clamp rotation and displacement magnitude to the authored limits.
    let scale = (wc[0] * wc[0] + wc[1] * wc[1]).sqrt();
    let mut theta = wc[1].atan2(wc[0]);
    let limit = params.max_rotation.to_radians();
    theta = theta.clamp(-limit, limit);
    let tc_len = (tc[0] * tc[0] + tc[1] * tc[1]).sqrt();
    let tc = if tc_len > params.max_displacement && tc_len > 0.0 {
        let k = params.max_displacement / tc_len;
        [tc[0] * k, tc[1] * k]
    } else {
        tc
    };
    // Compose about the frame center: C(p) = c + L(p - c) + t_c.
    let center = [w / 2.0, h / 2.0];
    let la = scale * theta.cos();
    let lb = scale * theta.sin();
    // C = [[la, -lb, cx + tx - la*cx + lb*cy], [lb, la, cy + ty - lb*cx - la*cy]]
    let c_matrix = [
        [la, -lb, center[0] + tc[0] - la * center[0] + lb * center[1]],
        [lb, la, center[1] + tc[1] - lb * center[0] - la * center[1]],
    ];
    // Crop coverage: fraction of the output frame not covered by C(frame).
    let corners = [[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]];
    let apply = |m: &[[f64; 3]; 2], p: [f64; 2]| {
        [
            m[0][0] * p[0] + m[0][1] * p[1] + m[0][2],
            m[1][0] * p[0] + m[1][1] * p[1] + m[1][2],
        ]
    };
    let quad: Vec<[f64; 2]> = corners.iter().map(|&p| apply(&c_matrix, p)).collect();
    let covered = clipped_area(&quad, w, h);
    let uncovered = 1.0 - covered / (w * h);
    if uncovered > params.max_crop + 1e-12 {
        return Err(StabilizeError::CropExceeded {
            uncovered,
            max_crop: params.max_crop,
        });
    }
    // Invert the similarity: L^-1 = L^T / |w|^2; C^-1(q) = L^-1(q - c - t) + c.
    let det = la * la + lb * lb;
    if det < 1e-18 {
        return Err(StabilizeError::Insufficient);
    }
    let inv_l = [[la / det, lb / det], [-lb / det, la / det]];
    let shifted = |p: [f64; 2]| [p[0] - center[0] - tc[0], p[1] - center[1] - tc[1]];
    let inv = |p: [f64; 2]| {
        let s = shifted(p);
        [
            inv_l[0][0] * s[0] + inv_l[0][1] * s[1] + center[0],
            inv_l[1][0] * s[0] + inv_l[1][1] * s[1] + center[1],
        ]
    };
    // Row-major 2x3 for C^-1(q) = M q + v.
    let origin = inv([0.0, 0.0]);
    let ex = inv([1.0, 0.0]);
    let ey = inv([0.0, 1.0]);
    let m = [
        [ex[0] - origin[0], ey[0] - origin[0], origin[0]],
        [ex[1] - origin[1], ey[1] - origin[1], origin[1]],
    ];
    if m.iter().flatten().any(|v| !v.is_finite()) {
        return Err(StabilizeError::InvalidInput("non-finite correction".into()));
    }
    Ok(m)
}
