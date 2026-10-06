use kronello_gpu::{color, *};

fn ramp() -> GradientPaint {
    GradientPaint {
        spread: GradientSpread::Pad,
        interpolation: GradientInterpolation::WorkingLinearPremultiplied,
        interpolation_version: 1,
        transform: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        geometry: GradientGeometry::Linear {
            start: [0.0; 2],
            end: [1.0, 0.0],
        },
        stops: vec![
            GradientStop {
                offset: 0.0,
                paint: Paint {
                    rgba: [0.0, 0.0, 0.0, 1.0],
                    space: InputSpace::Srgb,
                },
            },
            GradientStop {
                offset: 1.0,
                paint: Paint {
                    rgba: [1.0; 4],
                    space: InputSpace::Srgb,
                },
            },
        ],
    }
}
fn sample(g: &GradientPaint, p: [f32; 2]) -> f32 {
    g.sample(p, WorkingSpace::LinearRec709)[0]
}
fn near(a: f32, b: f32) {
    assert!((a - b).abs() < 0.00001, "{a} != {b}");
}

#[test]
fn cpu_vec004_default_paint_matches_vec003_interpolation_exactly() {
    let mut g = ramp();
    g.stops = vec![
        GradientStop {
            offset: 0.25,
            paint: Paint {
                rgba: [0.7, 0.2, 0.1, 0.8],
                space: InputSpace::Srgb,
            },
        },
        GradientStop {
            offset: 0.75,
            paint: Paint {
                rgba: [-0.2, 1.4, 0.6, 0.3],
                space: InputSpace::LinearRec2020,
            },
        },
        GradientStop {
            offset: 0.75,
            paint: Paint {
                rgba: [0.0, 0.0, 1.0, 0.0],
                space: InputSpace::LinearRec709,
            },
        },
    ];
    for working in [WorkingSpace::LinearRec709, WorkingSpace::LinearRec2020] {
        let a = color::to_working(g.stops[0].paint.rgba, g.stops[0].paint.space, working);
        let b = color::to_working(g.stops[1].paint.rgba, g.stops[1].paint.space, working);
        for radial in [false, true] {
            g.geometry = if radial {
                GradientGeometry::Radial {
                    center: [0.0; 2],
                    radius: 1.0,
                }
            } else {
                GradientGeometry::Linear {
                    start: [0.0; 2],
                    end: [1.0, 0.0],
                }
            };
            for t in [0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 1.0, 2.0] {
                let expected = if t < 0.25 {
                    a
                } else if t < 0.75 {
                    let f = (t - 0.25) / 0.5;
                    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * f)
                } else {
                    [0.0; 4]
                };
                assert_eq!(g.sample([t, 0.0], working), expected);
            }
        }
        // VEC-003 accepted every finite positive radial radius, including
        // radii beyond the coordinate bound imposed on gradient centers.
        g.geometry = GradientGeometry::Radial {
            center: [0.0; 2],
            radius: 2_000_000.0,
        };
        g.validate().unwrap();
        assert_eq!(
            g.sample([1_000_000.0, 0.0], working),
            std::array::from_fn(|i| a[i] + (b[i] - a[i]) * 0.5)
        );
        // Very small legacy radii can overflow the float32 parameter. Pad
        // still unambiguously chooses the last stop, preserving old pixels.
        g.geometry = GradientGeometry::Radial {
            center: [0.0; 2],
            radius: f32::MIN_POSITIVE,
        };
        g.validate().unwrap();
        assert_eq!(g.sample([1_000_000.0, 0.0], working), [0.0; 4]);
    }
}

#[test]
fn cpu_vec004_spread_negative_periods_boundaries_and_equal_offsets() {
    let mut g = ramp();
    g.spread = GradientSpread::Repeat;
    for (x, expected) in [
        (-2.25, 0.75),
        (-1.0, 0.0),
        (0.0, 0.0),
        (1.0, 0.0),
        (2.25, 0.25),
    ] {
        near(sample(&g, [x, 0.0]), expected);
    }
    g.spread = GradientSpread::Reflect;
    for (x, expected) in [
        (-2.25, 0.25),
        (-1.0, 1.0),
        (0.0, 0.0),
        (1.0, 1.0),
        (2.0, 0.0),
        (3.25, 0.75),
    ] {
        near(sample(&g, [x, 0.0]), expected);
    }
    g.stops.insert(
        1,
        GradientStop {
            offset: 0.0,
            paint: Paint {
                rgba: [0.5, 0.5, 0.5, 1.0],
                space: InputSpace::LinearRec709,
            },
        },
    );
    g.spread = GradientSpread::Repeat;
    near(sample(&g, [1.0, 0.0]), 0.5);
}

#[test]
fn cpu_vec004_focal_circle_family_and_conic_clockwise_seam() {
    let mut g = ramp();
    g.geometry = GradientGeometry::FocalRadial {
        center: [2.0, 0.0],
        radius: 4.0,
        focal: [1.0, 0.0],
        focal_radius: 1.0,
    };
    g.validate().unwrap();
    // t=0.5: center=1.5, radius=2.5; all four circle points share t.
    for p in [[4.0, 0.0], [-1.0, 0.0], [1.5, 2.5], [1.5, -2.5]] {
        near(sample(&g, p), 0.5);
    }
    near(sample(&g, [1.0, 0.0]), 0.0);
    near(sample(&g, [6.0, 0.0]), 1.0);
    g.geometry = GradientGeometry::Conic {
        center: [0.0; 2],
        start_angle: 90.0,
        sweep_angle: 360.0,
    };
    g.validate().unwrap();
    for (p, expected) in [
        ([0.0, 1.0], 0.0),
        ([-1.0, 0.0], 0.25),
        ([0.0, -1.0], 0.5),
        ([1.0, 0.0], 0.75),
        ([0.0, 0.0], 0.0),
    ] {
        near(sample(&g, p), expected);
    }
    g.geometry = GradientGeometry::Conic {
        center: [0.0; 2],
        start_angle: 0.0,
        sweep_angle: 180.0,
    };
    g.spread = GradientSpread::Reflect;
    near(sample(&g, [0.0, -1.0]), 0.5);
}

#[test]
fn cpu_vec004_interpolation_modes_preserve_hidden_straight_rgb_and_working_space() {
    let mut g = ramp();
    g.interpolation = GradientInterpolation::SrgbStraight;
    near(sample(&g, [0.5, 0.0]), color::srgb_decode(0.5));
    g.interpolation = GradientInterpolation::WorkingLinearStraight;
    near(sample(&g, [0.5, 0.0]), 0.5);
    g.stops[0].paint.rgba = [1.0, 0.0, 0.0, 1.0];
    g.stops[1].paint.rgba = [0.0, 0.0, 1.0, 0.0];
    for (mode, expected) in [
        (
            GradientInterpolation::WorkingLinearPremultiplied,
            [0.5, 0.0, 0.0, 0.5],
        ),
        (
            GradientInterpolation::WorkingLinearStraight,
            [0.25, 0.0, 0.25, 0.5],
        ),
        (
            GradientInterpolation::SrgbStraight,
            [
                color::srgb_decode(0.5) * 0.5,
                0.0,
                color::srgb_decode(0.5) * 0.5,
                0.5,
            ],
        ),
        (
            GradientInterpolation::SrgbPremultiplied,
            [0.5, 0.0, 0.0, 0.5],
        ),
    ] {
        g.interpolation = mode;
        let actual = g.sample([0.5, 0.0], WorkingSpace::LinearRec709);
        for i in 0..4 {
            near(actual[i], expected[i]);
        }
        let rgb = color::convert_primaries(
            [expected[0], expected[1], expected[2]],
            WorkingSpace::LinearRec709,
            WorkingSpace::LinearRec2020,
        );
        let actual = g.sample([0.5, 0.0], WorkingSpace::LinearRec2020);
        for i in 0..3 {
            near(actual[i], rgb[i]);
        }
        assert_eq!(g.sample([1.0, 0.0], WorkingSpace::LinearRec709), [0.0; 4]);
    }
}

#[test]
fn cpu_vec004_transform_and_invalid_combinations_fail_explicitly() {
    let mut g = ramp();
    g.transform = [[0.0, 0.5, -1.0], [-1.0, 0.0, 0.0]];
    g.validate().unwrap();
    near(sample(&g, [7.0, 3.0]), 0.5);
    g.interpolation_version = 2;
    assert!(matches!(g.validate(), Err(GpuError::UnsupportedFeature(_))));
    g.interpolation_version = 1;
    g.transform = [[0.0; 3]; 2];
    assert!(g.validate().is_err());
    g.transform = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    g.geometry = GradientGeometry::FocalRadial {
        center: [0.0; 2],
        radius: 2.0,
        focal: [1.0, 0.0],
        focal_radius: 1.0,
    };
    assert!(g.validate().is_err());
    g.geometry = GradientGeometry::Conic {
        center: [0.0; 2],
        start_angle: 0.0,
        sweep_angle: 0.0,
    };
    assert!(g.validate().is_err());
}

fn overflowing_gradient_scenes() -> (RenderSize, Vec<DrawScene>) {
    let mut gradient = ramp();
    gradient.geometry = GradientGeometry::FocalRadial {
        center: [0.0; 2],
        radius: 1_000_000.0,
        focal: [0.0; 2],
        focal_radius: 0.0,
    };
    gradient.transform = [[1_000_000.0, 0.0, 0.0], [0.0, 1_000_000.0, 0.0]];
    let scene = DrawScene {
        roots: vec![0],
        nodes: vec![DrawNode::Path(PathDraw {
            stroke_geometry: None,
            contours: vec![Contour {
                points: vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]],
                closed: true,
            }],
            fill: Some(Fill {
                paint: Paint {
                    rgba: [1.0; 4],
                    space: InputSpace::Srgb,
                },
                rule: FillRule::Nonzero,
            }),
            stroke: None,
            fill_gradient: Some(Box::new(gradient)),
            stroke_gradient: None,
            paint_transform: [[1.0e12, 0.0, 0.0], [0.0, 1.0e12, 0.0]],
        })],
    };
    let mut linear = scene.clone();
    let DrawNode::Path(path) = &mut linear.nodes[0] else {
        unreachable!()
    };
    path.fill_gradient.as_mut().unwrap().geometry = GradientGeometry::Linear {
        start: [0.0; 2],
        end: [1.0, 0.0],
    };
    path.paint_transform = [[f32::MAX, 0.0, 0.0], [0.0, f32::MAX, 0.0]];
    let mut radial = scene.clone();
    let DrawNode::Path(path) = &mut radial.nodes[0] else {
        unreachable!()
    };
    path.fill_gradient.as_mut().unwrap().geometry = GradientGeometry::Radial {
        center: [0.0; 2],
        radius: f32::MIN_POSITIVE,
    };
    path.fill_gradient.as_mut().unwrap().spread = GradientSpread::Repeat;
    let scenes = vec![scene, linear, radial];
    for scene in &scenes {
        scene.validate().unwrap();
    }
    (
        RenderSize {
            design_extent: [4.0; 2],
            output_resolution: [4; 2],
        },
        scenes,
    )
}

#[test]
fn cpu_vec004_nonfinite_parameter_fails_instead_of_selecting_a_stop() {
    let (size, scenes) = overflowing_gradient_scenes();
    // All inputs are finite and validated. The focal discriminant overflows
    // float32 even though the mathematically correct parameter is finite.
    for scene in scenes {
        assert!(matches!(
            render_scene_reference(size, &scene, WorkingSpace::LinearRec709),
            Err(GpuError::InvalidInput(_))
        ));
    }
}

#[test]
fn gpu_vec004_nonfinite_parameter_fails_instead_of_selecting_a_stop() {
    let gpu = GpuContext::new().expect("GPU required; no fallback");
    let (size, scenes) = overflowing_gradient_scenes();
    for scene in scenes {
        assert!(matches!(
            gpu.render_scene(size, &scene, WorkingSpace::LinearRec709),
            Err(GpuError::InvalidInput(_))
        ));
    }
}

fn transparent_gradient_scene(mode: GradientInterpolation) -> DrawScene {
    let (_, mut scenes) = overflowing_gradient_scenes();
    let mut scene = scenes.remove(0);
    let DrawNode::Path(path) = &mut scene.nodes[0] else {
        unreachable!()
    };
    path.paint_transform = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let gradient = path.fill_gradient.as_mut().unwrap();
    gradient.geometry = GradientGeometry::Linear {
        start: [0.0; 2],
        end: [4.0, 0.0],
    };
    gradient.transform = path.paint_transform;
    gradient.interpolation = mode;
    // Finite, valid straight colors overflow the Rec.2020 -> Rec.709 matrix.
    // Their zero alpha must still produce zero premultiplied paint.
    for stop in &mut gradient.stops {
        stop.paint = Paint {
            rgba: [f32::MAX, -f32::MAX, 0.0, 0.0],
            space: InputSpace::LinearRec2020,
        };
    }
    scene
}

const INTERPOLATION_MODES: [GradientInterpolation; 4] = [
    GradientInterpolation::WorkingLinearPremultiplied,
    GradientInterpolation::WorkingLinearStraight,
    GradientInterpolation::SrgbStraight,
    GradientInterpolation::SrgbPremultiplied,
];

#[test]
fn cpu_vec004_transparent_overflowing_rgb_keeps_zero_paint() {
    for mode in INTERPOLATION_MODES {
        let scene = transparent_gradient_scene(mode);
        let actual =
            render_scene_reference(RenderSize::pixels(4, 4), &scene, WorkingSpace::LinearRec709)
                .unwrap();
        assert_eq!(actual, vec![[0.0; 4]; 16]);
    }
}

#[test]
fn gpu_vec004_transparent_overflowing_rgb_keeps_zero_paint() {
    let gpu = GpuContext::new().expect("GPU required; no fallback");
    for mode in INTERPOLATION_MODES {
        let scene = transparent_gradient_scene(mode);
        let actual = gpu
            .render_scene(RenderSize::pixels(4, 4), &scene, WorkingSpace::LinearRec709)
            .unwrap();
        assert_eq!(actual.pixels, vec![[0.0; 4]; 16]);
    }
}
