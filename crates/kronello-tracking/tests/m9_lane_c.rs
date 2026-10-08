//! TRACK-002/003 acceptance: deterministic stabilization correction and
//! optical-flow intermediate-frame synthesis on synthetic inputs only.
use kronello_model::{
    AssetId, FiniteF64, TrackedFrame, TrackedPoint, TrackingDataAsset, TrackingMode, TrackingSeed,
    TrackingSource,
};
use kronello_time::{
    FlowFallbackPolicy, FrameInterpolation, OpticalFlowConfig, Rational, Time, TimeRange,
};

const W: u32 = 32;
const H: u32 = 32;

fn r(n: i64, d: i64) -> Rational {
    Rational::new(n, d).unwrap()
}
fn finite(v: f64) -> FiniteF64 {
    FiniteF64::new(v).unwrap()
}
fn config() -> OpticalFlowConfig {
    OpticalFlowConfig {
        block_radius: 2,
        search_radius: 6,
        levels: 2,
        confidence_floor: r(1, 4),
        max_low_confidence: r(1, 2),
        flow_fallback: None,
    }
}

/// 32x32 luma frames: black background with a patterned 8x8 block whose
/// top-left sits at (4 + shift*i, 12).
fn luma_block(count: usize, shift: i64) -> Vec<Vec<u8>> {
    (0..count)
        .map(|i| {
            let mut gray = vec![0u8; (W * H) as usize];
            for y in 0..8i64 {
                for x in 0..8i64 {
                    let (px, py) = (4 + shift * i as i64 + x, 12 + y);
                    if (0..W as i64).contains(&px) {
                        gray[(py * W as i64 + px) as usize] =
                            (((x * 37 + y * 53) % 200) as u8) + 32;
                    }
                }
            }
            gray
        })
        .collect()
}

fn opaque_frames(count: usize, shift: i64) -> Vec<Vec<[f32; 4]>> {
    luma_block(count, shift)
        .into_iter()
        .map(|gray| {
            gray.iter()
                .map(|&v| {
                    let v = f32::from(v) / 255.0;
                    [v, v, v, 1.0]
                })
                .collect()
        })
        .collect()
}

#[test]
fn track003_flow_recovers_integer_translation_and_is_deterministic() {
    let frames = luma_block(2, 2);
    let cfg = config();
    let fwd = kronello_tracking::estimate_flow(&frames[0], &frames[1], W, H, &cfg).unwrap();
    let bwd = kronello_tracking::estimate_flow(&frames[1], &frames[0], W, H, &cfg).unwrap();
    assert_eq!(fwd.version, kronello_tracking::FLOW_FIELD_VERSION);
    let cell = |f: &kronello_tracking::FlowField, x: u32, y: u32| {
        let block = 2 * f.block_radius + 1;
        ((y / block) * f.grid[0] + (x / block)) as usize
    };
    // The cell centered inside the moved block reports the exact +2 shift.
    assert_eq!(fwd.vectors[cell(&fwd, 12, 16)], [2.0, 0.0]);
    assert_eq!(bwd.vectors[cell(&bwd, 13, 16)], [-2.0, 0.0]);
    // Identical rerun: bit-identical fields.
    let rerun = kronello_tracking::estimate_flow(&frames[0], &frames[1], W, H, &cfg).unwrap();
    assert_eq!(rerun, fwd);
    // Forward/backward consistency keeps confidence at the exact match.
    let mut f = fwd.clone();
    let mut b = bwd.clone();
    kronello_tracking::consistency_combine(&mut f, &b);
    kronello_tracking::consistency_combine(&mut b, &f);
    kronello_tracking::confidence_gate(&f, &b, &cfg).unwrap();
}

#[test]
fn track003_confidence_gate_rejects_decorrelated_and_fallback_blends() {
    let (w, h) = (16u32, 16u32);
    // A horizontal ramp against its mirror: every template block is perfectly
    // anti-correlated with every candidate, so per-cell NCC is -1, mapped
    // confidence is 0 and the low-confidence ratio exceeds the authored cap.
    let a: Vec<u8> = (0..w * h).map(|i| ((i % w) * 16) as u8).collect();
    let b: Vec<u8> = a.iter().map(|v| 240 - v).collect();
    let cfg = OpticalFlowConfig {
        block_radius: 1,
        search_radius: 2,
        levels: 1,
        ..config()
    };
    let fwd = kronello_tracking::estimate_flow(&a, &b, w, h, &cfg).unwrap();
    let bwd = kronello_tracking::estimate_flow(&b, &a, w, h, &cfg).unwrap();
    let err = kronello_tracking::confidence_gate(&fwd, &bwd, &cfg).unwrap_err();
    assert_eq!(err.code(), "FLOW_CONFIDENCE_LOW");
    assert!(matches!(
        err,
        kronello_tracking::FlowError::ConfidenceLow { .. }
    ));
    // The authored blend fallback is a plain crossfade, independent of flow.
    let lo = vec![[0.0; 4]; (w * h) as usize];
    let hi = vec![[1.0; 4]; (w * h) as usize];
    let blended = kronello_tracking::blend_frames(&lo, &hi, 0.25).unwrap();
    assert_eq!(blended[0], [0.25; 4]);
    assert!(kronello_tracking::blend_frames(&lo, &hi[..hi.len() - 1], 0.5).is_err());
    let _ = FrameInterpolation::OpticalFlow;
    let _ = FlowFallbackPolicy::Blend;
}

#[test]
fn track003_interpolate_places_block_at_the_fraction() {
    let frames = opaque_frames(2, 2);
    let cfg = config();
    let mut fwd =
        kronello_tracking::estimate_flow(&luma_block(2, 2)[0], &luma_block(2, 2)[1], W, H, &cfg)
            .unwrap();
    let lumas = luma_block(2, 2);
    let mut bwd = kronello_tracking::estimate_flow(&lumas[1], &lumas[0], W, H, &cfg).unwrap();
    kronello_tracking::consistency_combine(&mut fwd, &bwd);
    kronello_tracking::consistency_combine(&mut bwd, &fwd);
    let mid = kronello_tracking::interpolate_frames(&frames[0], &frames[1], W, H, &fwd, &bwd, 0.5)
        .unwrap();
    assert_eq!(mid.len(), (W * H) as usize);
    // Midpoint places the block between its endpoints: the texel under the
    // interpolated center is bright while the extreme corner stays dark.
    let center = mid[(16 * W + 9) as usize];
    assert!(center[0] > 0.2, "midpoint block luma: {center:?}");
    assert!(mid[(W + 1) as usize][0] < 0.05);
    // Determinism: identical inputs produce identical pixels.
    let rerun =
        kronello_tracking::interpolate_frames(&frames[0], &frames[1], W, H, &fwd, &bwd, 0.5)
            .unwrap();
    assert_eq!(rerun, mid);
    // Shape/type errors stay typed.
    assert!(
        kronello_tracking::interpolate_frames(&frames[0], &frames[1], W, H, &fwd, &bwd, 1.5)
            .is_err()
    );
}

fn tracking_asset(translations: &[f64]) -> TrackingDataAsset {
    TrackingDataAsset {
        id: AssetId::new(),
        version: 1,
        source: TrackingSource {
            asset: AssetId::new(),
            stream_index: 0,
            content_hash: "a".repeat(64),
        },
        mode: TrackingMode::Points,
        seeds: vec![TrackingSeed {
            x: finite(0.5),
            y: finite(0.5),
            template_radius: 4,
            search_radius: 4,
        }],
        range: TimeRange::new(
            Time::ZERO,
            Rational::from_integer(translations.len() as i64),
        )
        .unwrap(),
        sample_rate: Rational::ONE,
        // Tracked point shifts by `translations[i]` pixels on x.
        frames: translations
            .iter()
            .enumerate()
            .map(|(i, &dx)| TrackedFrame {
                time: Rational::from_integer(i as i64),
                points: vec![TrackedPoint {
                    x: finite((15.5 + dx) / 31.0),
                    y: finite(0.5),
                    confidence: finite(1.0),
                }],
                homography: None,
            })
            .collect(),
        content_hash: "b".repeat(64),
    }
}

fn params(radius: u32, max_crop: f64) -> kronello_tracking::StabilizeParams {
    kronello_tracking::StabilizeParams {
        smoothing_radius: radius,
        max_displacement: 64.0,
        max_rotation: 5.0,
        max_crop,
    }
}

#[test]
fn track002_correction_inverse_is_identity_for_constant_motion() {
    let data = tracking_asset(&[2.0, 2.0, 2.0]);
    let m = kronello_tracking::correction_inverse(&data, Time::ONE, &params(1, 0.5), [32.0, 32.0])
        .unwrap();
    for (i, v) in m.iter().flatten().enumerate() {
        let expected = if i == 0 || i == 4 { 1.0 } else { 0.0 };
        assert!((v - expected).abs() < 1e-9, "m[{i}] = {v}");
    }
}

#[test]
fn track002_smoothed_correction_counters_single_frame_spike() {
    // Raw translations [0, 0, 10, 0, 0]; the radius-1 smoothed value at the
    // spike is 10/3, so C translates by 10/3 - 10 = -20/3 px and C^-1 returns
    // +20/3.
    let data = tracking_asset(&[0.0, 0.0, 10.0, 0.0, 0.0]);
    let m = kronello_tracking::correction_inverse(
        &data,
        Rational::from_integer(2),
        &params(1, 0.5),
        [32.0, 32.0],
    )
    .unwrap();
    assert!((m[0][0] - 1.0).abs() < 1e-9);
    assert!((m[1][1] - 1.0).abs() < 1e-9);
    assert!((m[0][1]).abs() < 1e-9 && (m[1][0]).abs() < 1e-9);
    assert!((m[0][2] - 20.0 / 3.0).abs() < 1e-6, "tx = {}", m[0][2]);
    assert!((m[1][2]).abs() < 1e-9);
    // Half-sample times interpolate between the bracketing tracked samples.
    let between =
        kronello_tracking::correction_inverse(&data, r(3, 2), &params(1, 1.0), [32.0, 32.0])
            .unwrap();
    // raw(1.5) = 5, smooth(1.5) = 10/3 ⇒ C translates by -5/3, C^-1 by +5/3.
    assert!(
        (between[0][2] - 5.0 / 3.0).abs() < 1e-6,
        "tx = {}",
        between[0][2]
    );
}

#[test]
fn track002_stabilize_errors_are_typed() {
    let data = tracking_asset(&[0.0, 0.0, 10.0, 0.0, 0.0]);
    // Beyond the tracked range.
    assert_eq!(
        kronello_tracking::correction_inverse(
            &data,
            Rational::from_integer(9),
            &params(1, 0.5),
            [32.0, 32.0],
        )
        .unwrap_err()
        .code(),
        "TRACKING_DATA_MISSING"
    );
    // The spike correction uncovers ~21% of the frame: max_crop 0.1 rejects.
    let err = kronello_tracking::correction_inverse(
        &data,
        Rational::from_integer(2),
        &params(1, 0.1),
        [32.0, 32.0],
    )
    .unwrap_err();
    assert_eq!(err.code(), "STABILIZE_CROP_EXCEEDED");
    // All-zero confidence leaves no usable sample.
    let mut lost = tracking_asset(&[0.0, 0.0]);
    for f in &mut lost.frames {
        f.points[0].confidence = finite(0.0);
    }
    assert_eq!(
        kronello_tracking::correction_inverse(&lost, Time::ZERO, &params(1, 0.5), [32.0, 32.0])
            .unwrap_err()
            .code(),
        "TRACKING_INSUFFICIENT"
    );
    // Non-finite extent is invalid input, not a panic.
    assert_eq!(
        kronello_tracking::correction_inverse(&data, Time::ZERO, &params(1, 0.5), [f64::NAN, 32.0])
            .unwrap_err()
            .code(),
        "INVALID_INPUT"
    );
}
