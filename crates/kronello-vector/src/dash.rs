//! Bounded local arclength subdivision, shared by all render backends.
use crate::{FlattenedPath, Polyline};
use kronello_model::{ShapeError, validate_dash_array};

pub const MAX_DASH_SEGMENTS: usize = 16_384;

/// Positive offset consumes the pattern before the contour starts. Each contour
/// starts at the same phase; a closed contour never restarts phase at its seam.
pub fn dash_path(
    path: &FlattenedPath,
    array: &[f64],
    offset: f64,
) -> Result<FlattenedPath, ShapeError> {
    validate_dash_array(array)?;
    if !offset.is_finite() {
        return Err(ShapeError::InvalidDashArray);
    }
    if array.is_empty() {
        return Ok(path.clone());
    }
    let mut pattern = array.to_vec();
    if pattern.len() % 2 == 1 {
        pattern.extend_from_slice(array);
    }
    let period: f64 = pattern.iter().sum();
    if !period.is_finite() || period <= 0.0 {
        return Err(ShapeError::InvalidDashArray);
    }
    let mut result = Vec::new();
    let mut work = 0usize;
    let mut tick = || {
        work += 1;
        if work > MAX_DASH_SEGMENTS {
            Err(ShapeError::StrokeBudgetExceeded)
        } else {
            Ok(())
        }
    };
    for contour in &path.subpaths {
        let mut points = Vec::new();
        for p in &contour.points {
            if p.iter().any(|x| !x.is_finite()) {
                return Err(ShapeError::InvalidDashArray);
            }
            if points.last() != Some(p) {
                points.push(*p);
            }
        }
        if contour.closed && points.len() > 1 && points.first() == points.last() {
            points.pop();
        }
        if points.is_empty() {
            continue;
        }
        let mut index = 0;
        let mut phase = offset.rem_euclid(period);
        // Keep a zero-length entry at an exact phase boundary for cap emission.
        while phase > 0.0 && phase >= pattern[index] {
            phase -= pattern[index];
            index = (index + 1) % pattern.len();
        }
        let mut remaining = pattern[index] - phase;
        let mut fragments: Vec<Polyline> = Vec::new();
        let mut active: Vec<[f64; 2]> = Vec::new();
        let count = if contour.closed {
            points.len()
        } else {
            points.len().saturating_sub(1)
        };
        if points.len() == 1 {
            if !contour.closed {
                for _ in 0..pattern.len() {
                    tick()?;
                    if index % 2 == 0 {
                        fragments.push(Polyline {
                            points: points.clone(),
                            closed: false,
                        });
                    }
                    if remaining > 0.0 {
                        break;
                    }
                    index = (index + 1) % pattern.len();
                    remaining = pattern[index];
                }
            }
            result.extend(fragments);
            continue;
        }
        for edge in 0..count {
            let a = points[edge];
            let b = points[(edge + 1) % points.len()];
            let length = (b[0] - a[0]).hypot(b[1] - a[1]);
            if !length.is_finite() {
                return Err(ShapeError::InvalidDashArray);
            }
            if length == 0.0 {
                continue;
            }
            let at = |d: f64| {
                if d == length {
                    b
                } else if d == 0.0 {
                    a
                } else {
                    [0, 1].map(|i| a[i] + (b[i] - a[i]) * (d / length))
                }
            };
            let mut position = 0.0;
            while position < length {
                tick()?;
                if remaining == 0.0 {
                    if index % 2 == 0 && pattern[index] == 0.0 {
                        fragments.push(Polyline {
                            points: vec![at(position)],
                            closed: false,
                        });
                    }
                    index = (index + 1) % pattern.len();
                    remaining = pattern[index];
                    continue;
                }
                let distance = remaining.min(length - position);
                let end = position + distance;
                if end <= position {
                    return Err(ShapeError::StrokeBudgetExceeded);
                }
                if index % 2 == 0 {
                    if active.is_empty() {
                        active.push(at(position));
                    }
                    active.push(at(end));
                } else if !active.is_empty() {
                    fragments.push(Polyline {
                        points: std::mem::take(&mut active),
                        closed: false,
                    });
                }
                remaining -= distance;
                position = end;
            }
        }
        if !active.is_empty() {
            fragments.push(Polyline {
                points: active,
                closed: false,
            });
        }
        // Zero paint at the final endpoint has the same cap semantics as at start.
        if remaining == 0.0 {
            for _ in 0..pattern.len() {
                index = (index + 1) % pattern.len();
                if pattern[index] != 0.0 {
                    break;
                }
                tick()?;
                if index % 2 == 0 {
                    fragments.push(Polyline {
                        points: vec![if contour.closed {
                            points[0]
                        } else {
                            *points.last().unwrap()
                        }],
                        closed: false,
                    });
                }
            }
        }
        if contour.closed {
            let start = points[0];
            let first = fragments
                .iter()
                .position(|f| f.points.len() > 1 && f.points.first() == Some(&start));
            let last = fragments
                .iter()
                .rposition(|f| f.points.len() > 1 && f.points.last() == Some(&start));
            if let (Some(first), Some(last)) = (first, last) {
                if first == last {
                    fragments[first].closed = true;
                    fragments[first].points.pop();
                } else {
                    let mut tail = fragments.remove(last);
                    let head = fragments.remove(first);
                    tail.points.extend(head.points.into_iter().skip(1));
                    fragments.insert(first, tail);
                }
            }
        }
        result.extend(fragments);
        if result.len() > MAX_DASH_SEGMENTS {
            return Err(ShapeError::StrokeBudgetExceeded);
        }
    }
    Ok(FlattenedPath { subpaths: result })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn line() -> FlattenedPath {
        FlattenedPath {
            subpaths: vec![Polyline {
                points: vec![[0.0, 0.0], [10.0, 0.0]],
                closed: false,
            }],
        }
    }
    #[test]
    fn vec005_dash_phase_odd_zero_and_budget() {
        let a = dash_path(&line(), &[2.0, 1.0], 0.0).unwrap();
        assert_eq!(a.subpaths[0].points, [[0.0, 0.0], [2.0, 0.0]]);
        assert_eq!(
            dash_path(&line(), &[2.0, 1.0], -1.0).unwrap(),
            dash_path(&line(), &[2.0, 1.0], 5.0).unwrap()
        );
        assert_eq!(
            dash_path(&line(), &[2.0], 0.0).unwrap(),
            dash_path(&line(), &[2.0, 2.0], 0.0).unwrap()
        );
        let dots = dash_path(&line(), &[0.0, 2.0], 0.0).unwrap();
        assert_eq!(dots.subpaths.len(), 6);
        assert!(dots.subpaths.iter().all(|p| p.points.len() == 1));
        for array in [
            vec![-1.0, 2.0],
            vec![0.0, 0.0],
            vec![f64::NAN],
            vec![f64::INFINITY],
        ] {
            assert!(matches!(
                dash_path(&line(), &array, 0.0),
                Err(ShapeError::InvalidDashArray)
            ));
        }
        assert!(matches!(
            dash_path(&line(), &[1e-9, 1e-9], 0.0),
            Err(ShapeError::StrokeBudgetExceeded)
        ));
    }
    #[test]
    fn vec005_closed_seam_preserves_join_and_contour_phase() {
        let p = Polyline {
            points: vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]],
            closed: true,
        };
        let path = FlattenedPath {
            subpaths: vec![p.clone(), p],
        };
        let dashed = dash_path(&path, &[6.0, 2.0], 1.0).unwrap();
        assert_eq!(dashed.subpaths.len(), 4);
        assert_eq!(dashed.subpaths[..2], dashed.subpaths[2..]);
        assert!(
            dashed.subpaths[0]
                .points
                .windows(3)
                .any(|p| p[1] == [0.0, 0.0])
        );
        let solid = dash_path(&path, &[100.0, 1.0], 0.0).unwrap();
        assert!(solid.subpaths.iter().all(|p| p.closed));
    }
}
