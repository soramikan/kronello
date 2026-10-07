//! FX-004 mask expansion geometry contracts (ADR-0114): `offset_path` moves
//! flattened contours in local design_px, preserves open/closed topology, and
//! fails with typed errors on non-finite input.
use kronello_vector::{FlattenedPath, Polyline, VectorError, offset_path};

fn contour(points: &[[f64; 2]], closed: bool) -> FlattenedPath {
    FlattenedPath {
        subpaths: vec![Polyline {
            points: points.to_vec(),
            closed,
        }],
    }
}

fn square_ccw() -> Vec<[f64; 2]> {
    vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]
}

fn signed_area(points: &[[f64; 2]], closed: bool) -> f64 {
    let count = if closed {
        points.len()
    } else {
        points.len() - 1
    };
    let mut area = 0.0;
    for i in 0..count {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        area += a[0] * b[1] - b[0] * a[1];
    }
    area * 0.5
}

#[test]
fn zero_distance_returns_the_input_unchanged() {
    let path = contour(&square_ccw(), true);
    assert_eq!(offset_path(&path, 0.0).unwrap(), path);
}

#[test]
fn closed_ccw_contour_expands_outward_and_contracts_inward() {
    let path = contour(&square_ccw(), true);
    let grown = offset_path(&path, 1.0).unwrap();
    let shrunk = offset_path(&path, -1.0).unwrap();
    assert_eq!(grown.subpaths[0].points.len(), 4);
    assert!(grown.subpaths[0].closed);
    // A 10x10 square offset by 1 px miters the 90-degree corners to the
    // adjacent square with corners at (-1,-1)/(11,-1)/(11,11)/(-1,11).
    assert_eq!(
        grown.subpaths[0].points,
        vec![[-1.0, -1.0], [11.0, -1.0], [11.0, 11.0], [-1.0, 11.0]]
    );
    assert_eq!(
        shrunk.subpaths[0].points,
        vec![[1.0, 1.0], [9.0, 1.0], [9.0, 9.0], [1.0, 9.0]]
    );
    assert_eq!(signed_area(&grown.subpaths[0].points, true), 144.0);
    assert_eq!(signed_area(&shrunk.subpaths[0].points, true), 64.0);
}

#[test]
fn clockwise_winding_still_expands_outward() {
    let mut points = square_ccw();
    points.reverse();
    let path = contour(&points, true);
    let grown = offset_path(&path, 1.0).unwrap();
    // The reversed square starts at (0,10); outward is still expansion.
    assert_eq!(
        grown.subpaths[0].points,
        vec![[-1.0, 11.0], [11.0, 11.0], [11.0, -1.0], [-1.0, -1.0]]
    );
}

#[test]
fn open_chain_offsets_along_its_normal_and_stays_open() {
    let path = contour(&[[0.0, 0.0], [10.0, 0.0]], false);
    let moved = offset_path(&path, 2.0).unwrap();
    assert!(!moved.subpaths[0].closed);
    assert_eq!(moved.subpaths[0].points, vec![[0.0, -2.0], [10.0, -2.0]]);
}

#[test]
fn degenerate_and_duplicate_points_do_not_panic_or_inflate() {
    let single = contour(&[[3.0, 3.0]], true);
    assert_eq!(offset_path(&single, 5.0).unwrap(), single);
    // A closed ring whose explicit last point duplicates the first collapses
    // back to the canonical four vertices.
    let mut dup = square_ccw();
    dup.push([0.0, 0.0]);
    dup.push([0.0, 0.0]);
    let path = contour(&dup, true);
    let grown = offset_path(&path, 1.0).unwrap();
    assert_eq!(grown.subpaths[0].points.len(), 4);
}

#[test]
fn sharp_spike_corners_are_clamped_to_the_miter_budget() {
    // A near-reversing V shape would explode a naive miter join; the offset
    // must stay finite and bounded relative to the distance.
    let path = contour(
        &[
            [0.0, 0.0],
            [10.0, 0.0],
            [0.2, 0.01],
            [10.0, 1.0],
            [0.0, 1.0],
        ],
        true,
    );
    let moved = offset_path(&path, 2.0).unwrap();
    for p in &moved.subpaths[0].points {
        assert!(p[0].is_finite() && p[1].is_finite());
        assert!(p[0].abs() <= 10.0 + 4.0 * 2.0 + 1.0);
        assert!(p[1].abs() <= 1.0 + 4.0 * 2.0 + 1.0);
    }
}

#[test]
fn non_finite_distance_or_geometry_fails_typed() {
    let path = contour(&square_ccw(), true);
    assert!(matches!(
        offset_path(&path, f64::NAN),
        Err(VectorError::NonFiniteGeometry)
    ));
    assert!(matches!(
        offset_path(&path, f64::INFINITY),
        Err(VectorError::NonFiniteGeometry)
    ));
    let bad = contour(&[[0.0, 0.0], [f64::NAN, 1.0], [1.0, 1.0]], false);
    assert!(matches!(
        offset_path(&bad, 1.0),
        Err(VectorError::NonFiniteGeometry)
    ));
}

#[test]
fn multiple_subpaths_offset_independently_and_keep_structure() {
    let path = FlattenedPath {
        subpaths: vec![
            Polyline {
                points: square_ccw(),
                closed: true,
            },
            Polyline {
                points: vec![[20.0, 0.0], [30.0, 0.0]],
                closed: false,
            },
        ],
    };
    let moved = offset_path(&path, 1.0).unwrap();
    assert_eq!(moved.subpaths.len(), 2);
    assert!(moved.subpaths[0].closed);
    assert!(!moved.subpaths[1].closed);
    assert_eq!(moved.subpaths[1].points, vec![[20.0, -1.0], [30.0, -1.0]]);
}
