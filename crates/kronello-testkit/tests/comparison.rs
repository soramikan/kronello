use kronello_testkit::{
    FrameDescriptor, LinearFrame, PixelError, PixelTolerance, WorkingSpace, compare_finite_values,
    compare_pixels, compare_semantic,
};

fn descriptor(width: u32) -> FrameDescriptor {
    FrameDescriptor {
        width,
        height: 1,
        origin: [0, 0],
        time: [0, 1],
        working_space: WorkingSpace::LinearRec2020,
        color_pipeline_id: "qa001-linear-v1".to_owned(),
        samples_per_frame: 1,
        seed: 0,
    }
}

fn compare(e: &[[f32; 4]], a: &[[f32; 4]]) -> Result<kronello_testkit::PixelReport, PixelError> {
    let d = descriptor(e.len() as u32);
    compare_pixels(
        LinearFrame {
            descriptor: &d,
            pixels: e,
        },
        LinearFrame {
            descriptor: &d,
            pixels: a,
        },
        PixelTolerance::default(),
    )
}

#[test]
fn semantic_values_are_exact_and_diagnostics_keep_the_path() {
    assert!(compare_semantic("layout.bounds", &[1, 2, 3, 4], &[1, 2, 3, 4]).is_ok());
    let error = compare_finite_values("property.x", &[1.0], &[1.00001]).unwrap_err();
    assert_eq!(error.path, "property.x");
    assert!(error.to_string().contains("expected"));
    assert!(compare_finite_values("geometry", &[f64::INFINITY], &[f64::INFINITY]).is_err());
    assert!(compare_finite_values("geometry", &[0.0], &[f64::NAN]).is_err());
}

#[test]
fn hdr_negative_rgb_and_small_positive_alpha_are_preserved() {
    let values = [[4.0, 2.0, -0.125, 1.0], [0.00001, 0.0, 0.0, 0.00001]];
    let report = compare(&values, &values).unwrap();
    assert_eq!(report.compared_pixels, 2);
    assert_eq!(report.max_rgb_error, 0.0);
}

#[test]
fn tolerance_boundary_is_inclusive_and_scales_for_hdr_rgb() {
    let expected = [[1.0, 4.0, -4.0, 0.5]];
    let actual = [[
        1.0 + 1.0 / 1024.0,
        4.0 + 4.0 / 1024.0,
        -4.0 - 4.0 / 1024.0,
        0.5 + 1.0 / 1024.0,
    ]];
    assert!(compare(&expected, &actual).is_ok());
    let actual = [[1.0, 4.01, -4.0, 0.5]];
    assert!(matches!(
        compare(&expected, &actual),
        Err(PixelError::Mismatch(_))
    ));
}

#[test]
fn every_bad_pixel_is_reported_without_an_outlier_allowance() {
    let expected = [[0.5, 0.0, 0.0, 1.0]; 4];
    let mut actual = expected;
    actual[1][0] = 0.7;
    actual[3][3] = 0.5;
    let Err(PixelError::Mismatch(report)) = compare(&expected, &actual) else {
        panic!("expected a mismatch report")
    };
    assert_eq!(report.compared_pixels, 4);
    assert_eq!(report.mismatched_pixels, 2);
    assert_eq!(report.first_mismatch, Some(1));
    assert_eq!(report.max_alpha_error, 0.5);
}

#[test]
fn invalid_pixels_fail_even_when_both_sides_are_equal() {
    for pixel in [
        [f32::NAN, 0.0, 0.0, 1.0],
        [f32::INFINITY, 0.0, 0.0, 1.0],
        [0.0, 0.0, 0.0, -0.1],
        [0.0, 0.0, 0.0, 1.1],
        [0.01, 0.0, 0.0, 0.0],
    ] {
        assert!(matches!(
            compare(&[pixel], &[pixel]),
            Err(PixelError::InvalidPixel { .. })
        ));
    }
    let good = [[0.0, 0.0, 0.0, 0.0]];
    let bad = [[1.0, 0.0, 0.0, 0.0]];
    assert!(matches!(
        compare(&good, &bad),
        Err(PixelError::InvalidPixel { side: "actual", .. })
    ));
}

#[test]
fn metadata_and_buffers_must_match() {
    let expected = descriptor(1);
    let pixels = [[0.0, 0.0, 0.0, 1.0]];
    let e = LinearFrame {
        descriptor: &expected,
        pixels: &pixels,
    };
    for actual in [
        FrameDescriptor {
            working_space: WorkingSpace::LinearRec709,
            ..expected.clone()
        },
        FrameDescriptor {
            origin: [1, 0],
            ..expected.clone()
        },
        FrameDescriptor {
            time: [1, 24],
            ..expected.clone()
        },
        FrameDescriptor {
            color_pipeline_id: "other".to_owned(),
            ..expected.clone()
        },
        FrameDescriptor {
            samples_per_frame: 2,
            ..expected.clone()
        },
        FrameDescriptor {
            seed: 1,
            ..expected.clone()
        },
    ] {
        assert_eq!(
            compare_pixels(
                e,
                LinearFrame {
                    descriptor: &actual,
                    pixels: &pixels
                },
                PixelTolerance::default()
            ),
            Err(PixelError::DescriptorMismatch)
        );
    }
    assert!(matches!(
        compare_pixels(
            e,
            LinearFrame {
                descriptor: &expected,
                pixels: &[]
            },
            PixelTolerance::default()
        ),
        Err(PixelError::BufferLength { .. })
    ));
}

#[test]
fn empty_frames_and_invalid_rationals_cannot_pass() {
    for time in [[0, 0], [2, 4], [0, 2], [1, -2]] {
        let d = FrameDescriptor {
            time,
            ..descriptor(1)
        };
        let pixels = [[0.0; 4]];
        let frame = LinearFrame {
            descriptor: &d,
            pixels: &pixels,
        };
        assert_eq!(
            compare_pixels(frame, frame, PixelTolerance::default()),
            Err(PixelError::InvalidDescriptor)
        );
    }
    assert_eq!(compare(&[], &[]), Err(PixelError::InvalidDescriptor));
}

#[test]
fn invalid_tolerances_fail() {
    let d = descriptor(1);
    let p = [[0.0; 4]];
    let frame = LinearFrame {
        descriptor: &d,
        pixels: &p,
    };
    for v in [-0.1, f64::NAN, f64::INFINITY] {
        let t = PixelTolerance {
            rgb_absolute: v,
            ..PixelTolerance::default()
        };
        assert_eq!(
            compare_pixels(frame, frame, t),
            Err(PixelError::InvalidTolerance)
        );
    }
    let hdr = [[4.0, 0.0, 0.0, 1.0]];
    let frame = LinearFrame {
        descriptor: &d,
        pixels: &hdr,
    };
    let tolerance = PixelTolerance {
        rgb_relative: f64::MAX,
        ..PixelTolerance::default()
    };
    assert_eq!(
        compare_pixels(frame, frame, tolerance),
        Err(PixelError::InvalidTolerance)
    );
}
