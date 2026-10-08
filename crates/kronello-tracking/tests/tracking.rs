//! Deterministic tracking acceptance: synthetic RGBA fixtures only.
use kronello_model::{AssetId, FiniteF64, TrackingMode, TrackingSeed, TrackingSource};
use kronello_time::{Rational, Time, TimeRange};

const W: u32 = 32;
const H: u32 = 32;

fn r(n: i64, d: i64) -> Rational {
    Rational::new(n, d).unwrap()
}

fn finite(v: f64) -> FiniteF64 {
    FiniteF64::new(v).unwrap()
}

fn range(frames: usize) -> TimeRange {
    TimeRange::new(Time::ZERO, Rational::from_integer(frames as i64)).unwrap()
}

fn seed(x: f64, y: f64) -> TrackingSeed {
    TrackingSeed {
        x: finite(x),
        y: finite(y),
        template_radius: 4,
        search_radius: 4,
    }
}

fn source() -> TrackingSource {
    TrackingSource {
        asset: AssetId::new(),
        stream_index: 0,
        content_hash: "a".repeat(64),
    }
}

/// Fill `rgba` with a fixed patterned block whose top-left pixel is (bx, by).
fn stamp(rgba: &mut [u8], bx: i64, by: i64) {
    for y in 0..8i64 {
        for x in 0..8i64 {
            let (px, py) = (bx + x, by + y);
            if px < 0 || py < 0 || px >= W as i64 || py >= H as i64 {
                continue;
            }
            let pixel = &mut rgba[((py * W as i64 + px) * 4) as usize..];
            let v = (((x * 37 + y * 53) % 256) as u8).max(32);
            pixel[0] = v;
            pixel[1] = v.rotate_left(3);
            pixel[2] = 255 - v;
            pixel[3] = 255;
        }
    }
}

/// `frames` black frames with the block translating (+dx, +dy) per frame.
fn moving_block(count: usize, dx: i64, dy: i64) -> Vec<Vec<u8>> {
    (0..count)
        .map(|i| {
            let mut rgba = vec![0u8; (W * H * 4) as usize];
            for p in rgba.chunks_exact_mut(4) {
                p[3] = 255;
            }
            stamp(&mut rgba, 4 + dx * i as i64, 12 + dy * i as i64);
            rgba
        })
        .collect()
}

fn view(frames: &[Vec<u8>], step: i64) -> Vec<kronello_tracking::TrackingFrame<'_>> {
    frames
        .iter()
        .enumerate()
        .map(|(i, rgba)| kronello_tracking::TrackingFrame {
            time: Rational::from_integer(step * i as i64),
            width: W,
            height: H,
            rgba,
        })
        .collect()
}

#[test]
fn point_tracking_follows_translating_block_deterministically() {
    let raw = moving_block(5, 1, 0);
    let frames = view(&raw, 1);
    // Block center: stamp x=[4+i..12+i) ⇒ normalized center at frame 0 ≈ 8/31.
    let seeds = [seed(8.0 / 31.0, 16.0 / 31.0)];
    let result = kronello_tracking::analyze(
        AssetId::new(),
        source(),
        TrackingMode::Points,
        &seeds,
        range(5),
        r(24, 1),
        W,
        H,
        &frames,
    )
    .unwrap();
    assert_eq!(result.frames.len(), 5);
    // One pixel per frame; x must increase exactly 1/31 per frame.
    let xs: Vec<f64> = result.frames.iter().map(|f| f.points[0].x.get()).collect();
    for (i, x) in xs.iter().enumerate() {
        assert_eq!(*x, (8 + i as i64) as f64 / 31.0, "frame {i}");
    }
    assert!(
        result
            .frames
            .iter()
            .all(|f| f.points[0].confidence.get() > 0.99)
    );
    result.validate().unwrap();

    // Bit-identical rerun: same seeds/range/id/source ⇒ same hash and payload.
    let rerun = kronello_tracking::analyze(
        result.id,
        result.source.clone(),
        TrackingMode::Points,
        &seeds,
        range(5),
        r(24, 1),
        W,
        H,
        &frames,
    )
    .unwrap();
    assert_eq!(rerun, result);
}

#[test]
fn plane_tracking_derives_translation_homography() {
    let raw = moving_block(4, 0, 1);
    let frames = view(&raw, 1);
    let seeds = [
        seed(5.0 / 31.0, 13.0 / 31.0),
        seed(11.0 / 31.0, 13.0 / 31.0),
        seed(11.0 / 31.0, 19.0 / 31.0),
        seed(5.0 / 31.0, 19.0 / 31.0),
    ];
    let result = kronello_tracking::analyze(
        AssetId::new(),
        source(),
        TrackingMode::Plane,
        &seeds,
        range(4),
        r(30, 1),
        W,
        H,
        &frames,
    )
    .unwrap();
    assert_eq!(result.frames.len(), 4);
    let first = &result.frames[0];
    let identity = first.homography.unwrap();
    for (i, v) in identity.iter().enumerate() {
        let expected = if i % 4 == 0 { 1.0 } else { 0.0 };
        assert!((v.get() - expected).abs() < 1e-9, "identity[{i}] = {v:?}");
    }
    let last = result.frames[3].homography.unwrap();
    // Pure +3px vertical translation: h12 = 3/31.
    assert!((last[0].get() - 1.0).abs() < 1e-9);
    assert!((last[2].get()).abs() < 1e-9);
    assert!((last[4].get() - 1.0).abs() < 1e-9);
    assert!((last[5].get() - 3.0 / 31.0).abs() < 1e-9);
    // Table view exposes h columns for expressions.
    let table = result.data_table();
    for name in ["h00", "h11", "h22", "confidence", "x0", "y3"] {
        assert!(table.columns.contains_key(name), "{name}");
    }
    assert_eq!(table.rows.len(), 4);
}

#[test]
fn tracking_rejects_invalid_and_overbudget_inputs() {
    let raw = moving_block(2, 0, 0);
    let frames = view(&raw, 1);
    let seeds = [seed(0.5, 0.5)];
    // Empty frame set.
    assert!(
        kronello_tracking::analyze(
            AssetId::new(),
            source(),
            TrackingMode::Points,
            &seeds,
            range(2),
            r(24, 1),
            W,
            H,
            &[],
        )
        .is_err()
    );
    // Plane mode requires exactly four seeds.
    assert!(
        kronello_tracking::analyze(
            AssetId::new(),
            source(),
            TrackingMode::Plane,
            &seeds,
            range(2),
            r(24, 1),
            W,
            H,
            &frames,
        )
        .is_err()
    );
    // Work budget: huge radii on many frames.
    let big = TrackingSeed {
        x: finite(0.5),
        y: finite(0.5),
        template_radius: 32,
        search_radius: 64,
    };
    let many: Vec<Vec<u8>> = moving_block(2, 0, 0);
    let many_frames: Vec<_> = (0..kronello_model::TRACKING_MAX_FRAMES as usize)
        .map(|i| kronello_tracking::TrackingFrame {
            time: Rational::from_integer(i as i64),
            width: W,
            height: H,
            rgba: many[i % 2].as_slice(),
        })
        .collect();
    let seeds8 = vec![big; 8];
    let error = kronello_tracking::analyze(
        AssetId::new(),
        source(),
        TrackingMode::Points,
        &seeds8,
        range(kronello_model::TRACKING_MAX_FRAMES as usize),
        r(24, 1),
        W,
        H,
        &many_frames,
    )
    .unwrap_err();
    assert_eq!(error.code(), "TRACKING_BUDGET_EXCEEDED");
}

#[test]
fn homography4_solves_exact_known_maps() {
    // Identity.
    let square = [[0.1, 0.1], [0.9, 0.1], [0.9, 0.9], [0.1, 0.9]];
    let h = kronello_tracking::homography4(square, square).unwrap();
    for (i, v) in h.iter().enumerate() {
        let expected = if i % 4 == 0 { 1.0 } else { 0.0 };
        assert!((v - expected).abs() < 1e-9, "h[{i}] = {v}");
    }
    // Pure translation.
    let shifted: [[f64; 2]; 4] =
        std::array::from_fn(|i| [square[i][0] + 0.05, square[i][1] - 0.02]);
    let h = kronello_tracking::homography4(square, shifted).unwrap();
    assert!((h[0] - 1.0).abs() < 1e-9);
    assert!((h[2] - 0.05).abs() < 1e-9);
    assert!((h[4] - 1.0).abs() < 1e-9);
    assert!((h[5] + 0.02).abs() < 1e-9);
    assert!(h[6].abs() < 1e-9 && h[7].abs() < 1e-9);
    // Degenerate (collinear) input yields no map.
    let line = [[0.0, 0.0], [0.5, 0.5], [1.0, 1.0], [0.25, 0.25]];
    assert!(kronello_tracking::homography4(line, line).is_none());
}
