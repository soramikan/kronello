//! AI-002/AI-003 pure-crate coverage: deterministic boundary detection and
//! smart-reframe window math (ADR-0125/0126). No filesystem, decode or jobs.
use kronello_model::*;
use kronello_scene::*;
use kronello_time::{Duration, Rational, Time, TimeRange};

fn r(n: i64, d: i64) -> Rational {
    Rational::new(n, d).unwrap()
}
fn finite(v: f64) -> FiniteF64 {
    FiniteF64::new(v).unwrap()
}
fn t(tick: i64) -> Time {
    r(tick, 24)
}

/// Solid RGBA8 frames at 1/24-second ticks.
fn solid_frames(times: &[Time], width: u32, height: u32, rgb: [u8; 3]) -> Vec<Vec<u8>> {
    times
        .iter()
        .map(|_| {
            let mut rgba = vec![0u8; (width * height * 4) as usize];
            for pixel in rgba.chunks_exact_mut(4) {
                pixel[..3].copy_from_slice(&rgb);
                pixel[3] = 255;
            }
            rgba
        })
        .collect()
}
fn scene_frames<'a>(times: &[Time], pixels: &'a [Vec<u8>], w: u32, h: u32) -> Vec<SceneFrame<'a>> {
    times
        .iter()
        .zip(pixels)
        .map(|(&time, rgba)| SceneFrame {
            time,
            width: w,
            height: h,
            rgba,
        })
        .collect()
}

#[test]
fn detect_emits_cut_at_first_frame_of_new_scene_deterministically() {
    // Four dark frames cut to four bright frames; the transition lands at
    // score index 3, so the boundary time is frames[4].
    let times: Vec<_> = (0..8).map(t).collect();
    let mut pixels = solid_frames(&times[..4], 16, 16, [10, 10, 10]);
    pixels.extend(solid_frames(&times[4..], 16, 16, [240, 240, 240]));
    let frames = scene_frames(&times, &pixels, 16, 16);
    let first = detect_boundaries(&frames, &SceneDetectionParams::default()).unwrap();
    let second = detect_boundaries(&frames, &SceneDetectionParams::default()).unwrap();
    assert_eq!(first, second, "detection is deterministic");
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].time, t(4));
    assert!((0.0..=1.0).contains(&first[0].confidence));
}

#[test]
fn detect_reports_no_boundaries_for_uniform_and_single_frame_input() {
    let times: Vec<_> = (0..6).map(t).collect();
    let pixels = solid_frames(&times, 16, 16, [120, 60, 200]);
    let frames = scene_frames(&times, &pixels, 16, 16);
    assert!(
        detect_boundaries(&frames, &SceneDetectionParams::default())
            .unwrap()
            .is_empty()
    );
    // A lone transition above the noise floor is still a boundary.
    let times = [t(0), t(1)];
    let pixels = vec![vec![0u8; 16 * 16 * 4], vec![255u8; 16 * 16 * 4]];
    let frames = scene_frames(&times, &pixels, 16, 16);
    let boundaries = detect_boundaries(&frames, &SceneDetectionParams::default()).unwrap();
    assert_eq!(boundaries.len(), 1);
    assert_eq!(boundaries[0].time, t(1));
}

#[test]
fn detect_rejects_nonuniform_truncated_and_unordered_input() {
    let times = [t(0), t(1)];
    let pixels = [vec![0u8; 16 * 16 * 4], vec![0u8; 8 * 8 * 4]];
    let frames = vec![
        SceneFrame {
            time: times[0],
            width: 16,
            height: 16,
            rgba: &pixels[0],
        },
        SceneFrame {
            time: times[1],
            width: 8,
            height: 8,
            rgba: &pixels[1],
        },
    ];
    let error = detect_boundaries(&frames, &SceneDetectionParams::default()).unwrap_err();
    assert_eq!(error.code(), "INVALID_REQUEST");
    let pixels = [vec![0u8; 16 * 16 * 4], vec![0u8; 16 * 16 * 4]];
    let frames = vec![
        SceneFrame {
            time: t(1),
            width: 16,
            height: 16,
            rgba: &pixels[0],
        },
        SceneFrame {
            time: t(1),
            width: 16,
            height: 16,
            rgba: &pixels[1],
        },
    ];
    let error = detect_boundaries(&frames, &SceneDetectionParams::default()).unwrap_err();
    assert_eq!(error.code(), "INVALID_REQUEST");
    let oversized = vec![SceneFrame {
        time: t(0),
        width: 0,
        height: 16,
        rgba: &[],
    }];
    let error = detect_boundaries(&oversized, &SceneDetectionParams::default()).unwrap_err();
    assert_eq!(error.code(), "SCENE_BUDGET_EXCEEDED");
}

type TrackingRows = Vec<(i64, Vec<(f64, f64, f64)>)>;

fn tracking(frames: TrackingRows, seeds: usize) -> TrackingDataAsset {
    let start = r(0, 24);
    let end = r(frames.len() as i64, 24);
    let mut data = TrackingDataAsset {
        id: AssetId::new(),
        version: TRACKING_VERSION,
        source: TrackingSource {
            asset: AssetId::new(),
            stream_index: 0,
            content_hash: "a".repeat(64),
        },
        mode: TrackingMode::Points,
        seeds: (0..seeds)
            .map(|_| TrackingSeed {
                x: finite(0.5),
                y: finite(0.5),
                template_radius: 4,
                search_radius: 8,
            })
            .collect(),
        range: TimeRange::new(start, end.max(r(1, 24))).unwrap(),
        sample_rate: r(24, 1),
        frames: frames
            .into_iter()
            .map(|(tick, points)| TrackedFrame {
                time: t(tick),
                points: points
                    .into_iter()
                    .map(|(x, y, confidence)| TrackedPoint {
                        x: finite(x),
                        y: finite(y),
                        confidence: finite(confidence),
                    })
                    .collect(),
                homography: None,
            })
            .collect(),
        content_hash: String::new(),
    };
    data.content_hash = data.computed_hash().unwrap();
    data
}

fn settings(padding: f64, max_zoom: f64) -> SmartReframeSettings {
    SmartReframeSettings {
        version: SMART_REFRAME_VERSION,
        seeds: vec![],
        smoothing_window: Duration::new(r(1, 24)).unwrap(),
        padding: finite(padding),
        max_zoom: finite(max_zoom),
        target_aspect: finite(9.0 / 16.0),
        easing: ReframeEasing::Linear,
    }
}

#[test]
fn reframe_centers_window_on_weighted_gaze_and_clamps_to_source() {
    // Single seed at x=0.25; the largest 9:16 window in 1920x1080 is
    // 607.5x1080, centered on 480 -> x = 176.25.
    let data = tracking(vec![(0, vec![(0.25, 0.5, 1.0)])], 1);
    let window = reframe_window(&data, &settings(0.0, 1.0), Time::ZERO, [1920.0, 1080.0]).unwrap();
    assert_eq!(window[1], 0.0);
    assert_eq!(window[3], 1080.0);
    assert!((window[2] - 607.5).abs() < 1e-6);
    assert!((window[0] - 176.25).abs() < 1e-6);
    // Gaze at the left edge clamps the window inside the source.
    let data = tracking(vec![(0, vec![(0.0, 0.5, 1.0)])], 1);
    let window = reframe_window(&data, &settings(0.0, 1.0), Time::ZERO, [1920.0, 1080.0]).unwrap();
    assert_eq!(window[0], 0.0);
    // Deterministic: identical inputs produce identical output.
    let again = reframe_window(&data, &settings(0.0, 1.0), Time::ZERO, [1920.0, 1080.0]).unwrap();
    assert_eq!(window, again);
}

#[test]
fn reframe_padding_and_max_zoom_scale_the_window() {
    let data = tracking(vec![(0, vec![(0.5, 0.5, 1.0)])], 1);
    // padding = 1 is the largest window that fits regardless of zoom cap.
    let window = reframe_window(&data, &settings(1.0, 4.0), Time::ZERO, [1920.0, 1080.0]).unwrap();
    assert!((window[2] - 607.5).abs() < 1e-6);
    // padding = 0 with max_zoom = 2 halves the fitted window.
    let window = reframe_window(&data, &settings(0.0, 2.0), Time::ZERO, [1920.0, 1080.0]).unwrap();
    assert!((window[2] - 303.75).abs() < 1e-6);
    assert!((window[3] - 540.0).abs() < 1e-6);
}

#[test]
fn reframe_selects_seeds_and_weights_confidence() {
    // Two seeds inside the unclamped band; selecting seed 1 only must
    // follow the second point.
    let data = tracking(vec![(0, vec![(0.7, 0.5, 1.0), (0.3, 0.5, 1.0)])], 2);
    let mut selected = settings(1.0, 1.0);
    selected.seeds = vec![1];
    let window = reframe_window(&data, &selected, Time::ZERO, [1920.0, 1080.0]).unwrap();
    let center = window[0] + window[2] / 2.0;
    // seed 1 sits at x=0.3 * 1920 = 576.
    assert!(
        (center - 576.0).abs() < 1e-6,
        "window follows selected seed"
    );
    // Equal weights put the centroid between both points: 0.5 * 1920 = 960.
    let window = reframe_window(&data, &settings(1.0, 1.0), Time::ZERO, [1920.0, 1080.0]).unwrap();
    assert!((window[0] + window[2] / 2.0 - 960.0).abs() < 1e-6);
    // Out-of-range seed selection is a typed request error.
    let mut invalid = settings(0.0, 1.0);
    invalid.seeds = vec![7];
    let error = reframe_window(&data, &invalid, Time::ZERO, [1920.0, 1080.0]).unwrap_err();
    assert_eq!(error.code(), "INVALID_REQUEST");
}

#[test]
fn reframe_fails_tracking_insufficient_instead_of_center_crop() {
    // No valid point inside the window: never a silent center crop.
    let data = tracking(vec![(0, vec![(0.5, 0.5, 0.0)])], 1);
    let error =
        reframe_window(&data, &settings(0.0, 1.0), Time::ZERO, [1920.0, 1080.0]).unwrap_err();
    assert_eq!(error, ReframeError::Insufficient);
    assert_eq!(error.code(), "TRACKING_INSUFFICIENT");
    // A lost frame inside the smoothing window exceeding half the weight.
    let data = tracking(
        vec![
            (0, vec![(0.5, 0.5, 1.0)]),
            (1, vec![(0.5, 0.5, 0.0)]),
            (2, vec![(0.5, 0.5, 0.0)]),
        ],
        1,
    );
    let error = reframe_window(&data, &settings(0.0, 1.0), r(2, 24), [1920.0, 1080.0]).unwrap_err();
    assert_eq!(error, ReframeError::Insufficient);
}

#[test]
fn reframe_smoothing_window_blends_neighbor_frames() {
    // Center frame tracked at x=0.5, neighbors at x=0.0/1.0 with weaker
    // temporal weight: the centroid stays near center, not at either edge.
    let data = tracking(
        vec![
            (0, vec![(0.0, 0.5, 1.0)]),
            (1, vec![(0.5, 0.5, 1.0)]),
            (2, vec![(1.0, 0.5, 1.0)]),
        ],
        1,
    );
    let window = reframe_window(&data, &settings(1.0, 1.0), t(1), [1920.0, 1080.0]).unwrap();
    assert!((window[0] + window[2] / 2.0 - 960.0).abs() < 1e-6);
    // A zero smoothing window samples only the exact source time.
    let mut exact = settings(1.0, 1.0);
    exact.smoothing_window = Duration::new(Time::ZERO).unwrap();
    let window = reframe_window(&data, &exact, t(2), [1920.0, 1080.0]).unwrap();
    // x=1.0 clamps the window against the right edge.
    assert!((window[0] + window[2] - 1920.0).abs() < 1e-6);
}
