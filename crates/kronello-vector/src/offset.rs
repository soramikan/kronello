//! Vertex-normal offset for FX-004 mask expansion (ADR-0114). Distance is in
//! the same local design_px as the flattened input; the result keeps the
//! input's subpath/closure structure so downstream coverage stays identical
//! between CPU and GPU backends.
use crate::{FlattenedPath, Polyline, VectorError};

/// Maximum miter stretch relative to `distance`; spikes fall back to a bevel
/// so a near-reversing corner cannot produce unbounded coordinates.
const MITER_LIMIT: f64 = 4.0;

fn unit_normal(edge: [f64; 2]) -> Option<[f64; 2]> {
    let length = edge[0].hypot(edge[1]);
    if !length.is_finite() || length <= f64::EPSILON {
        return None;
    }
    Some([edge[1] / length, -edge[0] / length])
}

fn signed_area(points: &[[f64; 2]]) -> f64 {
    let mut area = 0.0;
    for i in 0..points.len() {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        area += a[0] * b[1] - b[0] * a[1];
    }
    area * 0.5
}

/// Offset every subpath by `distance` design_px. Positive moves outward on
/// closed contours (winding-aware) and outward relative to direction on open
/// chains; negative contracts. Degenerate edges inherit the surviving edge's
/// normal; a fully degenerate contour is returned unchanged.
pub fn offset_path(path: &FlattenedPath, distance: f64) -> Result<FlattenedPath, VectorError> {
    if !distance.is_finite() {
        return Err(VectorError::NonFiniteGeometry);
    }
    if distance == 0.0 {
        return Ok(path.clone());
    }
    let mut subpaths = Vec::with_capacity(path.subpaths.len());
    for contour in &path.subpaths {
        let mut points: Vec<[f64; 2]> = Vec::with_capacity(contour.points.len());
        for p in &contour.points {
            if p.iter().any(|v| !v.is_finite()) {
                return Err(VectorError::NonFiniteGeometry);
            }
            if points.last() != Some(p) {
                points.push(*p);
            }
        }
        if contour.closed && points.len() > 1 && points.first() == points.last() {
            points.pop();
        }
        if points.len() < 2 {
            subpaths.push(Polyline {
                points,
                closed: contour.closed,
            });
            continue;
        }
        let count = if contour.closed {
            points.len()
        } else {
            points.len() - 1
        };
        let mut edges: Vec<Option<[f64; 2]>> = Vec::with_capacity(count);
        for i in 0..count {
            let a = points[i];
            let b = points[(i + 1) % points.len()];
            edges.push(unit_normal([b[0] - a[0], b[1] - a[1]]));
        }
        // Outward direction for a closed ring follows its signed winding;
        // open chains have no interior, so their single normal is a convention.
        let winding = if contour.closed && signed_area(&points) < 0.0 {
            -1.0
        } else {
            1.0
        };
        let normal_at =
            |i: usize| -> Option<[f64; 2]> { edges[i].map(|n| [n[0] * winding, n[1] * winding]) };
        let mut offset = Vec::with_capacity(points.len());
        for i in 0..points.len() {
            // Previous edge normal (into the vertex) and next (out of it).
            let prev = if contour.closed {
                normal_at((i + points.len() - 1) % points.len())
            } else {
                i.checked_sub(1).and_then(normal_at)
            };
            let next = if contour.closed {
                normal_at(i)
            } else {
                normal_at(i.min(count.saturating_sub(1)))
            };
            let p = points[i];
            let displaced = match (prev, next) {
                (Some(a), Some(b)) => {
                    let dot = a[0] * b[0] + a[1] * b[1];
                    let denom = 1.0 + dot;
                    if denom <= (1.0 / (MITER_LIMIT * MITER_LIMIT)).max(1e-8) {
                        // Spike: clamp to a single normal direction.
                        [p[0] + a[0] * distance, p[1] + a[1] * distance]
                    } else {
                        let miter = [a[0] + b[0], a[1] + b[1]];
                        let length = distance / denom;
                        let cap = MITER_LIMIT * distance.abs();
                        let clamped = if length.abs() > cap {
                            cap * length.signum()
                        } else {
                            length
                        };
                        [p[0] + miter[0] * clamped, p[1] + miter[1] * clamped]
                    }
                }
                (Some(n), None) | (None, Some(n)) => {
                    [p[0] + n[0] * distance, p[1] + n[1] * distance]
                }
                (None, None) => p,
            };
            if displaced.iter().any(|v| !v.is_finite() || v.abs() > 1e12) {
                return Err(VectorError::NonFiniteGeometry);
            }
            offset.push(displaced);
        }
        subpaths.push(Polyline {
            points: offset,
            closed: contour.closed,
        });
    }
    Ok(FlattenedPath { subpaths })
}
