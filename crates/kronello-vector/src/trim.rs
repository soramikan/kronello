use crate::{FlattenedPath, Polyline, VectorError};

/// Trim concatenated contour lengths. Partial contours are open. Whole-path
/// selection preserves closure, and wrapping never joins distinct contours.
pub fn trim_path(
    path: &FlattenedPath,
    start: f64,
    end: f64,
    offset: f64,
) -> Result<FlattenedPath, VectorError> {
    if !start.is_finite()
        || !end.is_finite()
        || !offset.is_finite()
        || !(0.0..=1.0).contains(&start)
        || !(start..=1.0).contains(&end)
    {
        return Err(VectorError::InvalidRequest);
    }
    if end - start == 1.0 {
        return Ok(path.clone());
    }
    if start == end {
        return Ok(FlattenedPath { subpaths: vec![] });
    }
    let mut edges = Vec::new();
    let mut length = 0.0;
    for (contour, subpath) in path.subpaths.iter().enumerate() {
        let count = subpath.points.len();
        for index in 0..count.saturating_sub(1) + usize::from(subpath.closed && count > 1) {
            let a = subpath.points[index];
            let b = subpath.points[(index + 1) % count];
            let edge_length = (a[0] - b[0]).hypot(a[1] - b[1]);
            if !edge_length.is_finite() {
                return Err(VectorError::NonFiniteGeometry);
            }
            if edge_length > 0.0 {
                edges.push((contour, a, b, length, length + edge_length));
            }
            length += edge_length;
            if edges.len() > 65536 {
                return Err(VectorError::GeometryBudgetExceeded);
            }
        }
    }
    if !length.is_finite() {
        return Err(VectorError::NonFiniteGeometry);
    }
    if length == 0.0 {
        return Ok(FlattenedPath { subpaths: vec![] });
    }
    let begin = (start + offset.rem_euclid(1.0)).rem_euclid(1.0);
    let finish = begin + end - start;
    let intervals = if finish > 1.0 {
        vec![(begin * length, length), (0.0, (finish - 1.0) * length)]
    } else {
        vec![(begin * length, finish * length)]
    };
    let mut subpaths: Vec<Polyline> = vec![];
    for (lo, hi) in intervals {
        let mut previous_contour = None;
        for &(contour, a, b, l, r) in &edges {
            let (left, right) = (lo.max(l), hi.min(r));
            if left >= right {
                continue;
            }
            let point = |v: f64| {
                let t = (v - l) / (r - l);
                [a[0] * (1.0 - t) + b[0] * t, a[1] * (1.0 - t) + b[1] * t]
            };
            let (p, q) = (point(left), point(right));
            if previous_contour == Some(contour)
                && subpaths.last().is_some_and(|s| s.points.last() == Some(&p))
            {
                subpaths
                    .last_mut()
                    .expect("existing contour")
                    .points
                    .push(q);
            } else {
                subpaths.push(Polyline {
                    points: vec![p, q],
                    closed: false,
                });
            }
            previous_contour = Some(contour);
        }
    }
    // Only one closed contour can make the document-end/document-start seam
    // contiguous. Keep its partial selection open, with a join at that seam.
    if finish > 1.0
        && path.subpaths.len() == 1
        && path.subpaths[0].closed
        && subpaths.len() == 2
        && subpaths[0].points.last() == subpaths[1].points.first()
    {
        let second = subpaths.pop().expect("second fragment");
        subpaths[0].points.extend(second.points.into_iter().skip(1));
    }
    Ok(FlattenedPath { subpaths })
}
