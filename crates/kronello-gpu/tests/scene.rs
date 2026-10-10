mod common;

#[test]
fn gpu_native_preview_texture_matches_export_color_conversion() {
    let size = RenderSize::pixels(8, 8);
    let scene = DrawScene {
        nodes: vec![rectangle(
            [0.0; 2],
            [8.0; 2],
            paint([0.8, 0.4, 0.2, 0.5], InputSpace::Srgb),
        )],
        roots: vec![0],
    };
    let transform = OutputTransform {
        space: InputSpace::LinearRec709,
        alpha: OutputAlpha::Premultiplied,
    };
    let texture = gpu()
        .render_scene_texture(size, &scene, WorkingSpace::LinearRec709, transform)
        .unwrap();
    let mut readback = TransferStats::default();
    let bytes = gpu().read_texture(&texture, 8, &mut readback).unwrap();
    let actual = decode_rgba16f(&bytes).unwrap();
    let expected = gpu()
        .render_scene_output(size, &scene, WorkingSpace::LinearRec709, transform)
        .unwrap();
    assert_eq!(actual, expected.pixels);
    assert_eq!(texture.size().width, 8);
    assert_eq!(texture.size().height, 8);
}

use common::*;
use kronello_gpu::*;
use kronello_testkit::{FrameDescriptor, LinearFrame, PixelTolerance, compare_pixels};
use std::sync::OnceLock;
fn gpu() -> &'static GpuContext {
    static GPU: OnceLock<GpuContext> = OnceLock::new();
    GPU.get_or_init(|| {
        let gpu = GpuContext::new().expect("GPU required; no fallback or skip");
        eprintln!("GPU-002 adapter: {:?}", gpu.adapter_info);
        gpu
    })
}
fn compare(n: u32, working: WorkingSpace, expected: &[[f32; 4]], actual: &[[f32; 4]]) {
    let d = FrameDescriptor {
        width: n,
        height: n,
        origin: [0; 2],
        time: [0, 1],
        working_space: if working == WorkingSpace::LinearRec709 {
            kronello_testkit::WorkingSpace::LinearRec709
        } else {
            kronello_testkit::WorkingSpace::LinearRec2020
        },
        color_pipeline_id: "vec003-grid4-v2".into(),
        samples_per_frame: 16,
        seed: 0,
    };
    compare_pixels(
        LinearFrame {
            descriptor: &d,
            pixels: expected,
        },
        LinearFrame {
            descriptor: &d,
            pixels: actual,
        },
        PixelTolerance::default(),
    )
    .unwrap();
}
#[test]
fn cpu_shader_parses_and_validates() {
    let module = naga::front::wgsl::parse_str(SCENE_SHADER).unwrap();
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .unwrap();
}
#[test]
fn cpu_known_coverage_winding_stroke_and_empty_scene() {
    let scene = DrawScene {
        nodes: vec![rectangle(
            [0.5, 0.0],
            [1.0, 1.0],
            paint([1.0, 0.5, 0.0, 1.0], InputSpace::LinearRec709),
        )],
        roots: vec![0],
    };
    assert_eq!(
        render_scene_reference(RenderSize::pixels(1, 1), &scene, WorkingSpace::LinearRec709)
            .unwrap(),
        vec![[0.5, 0.25, 0.0, 0.5]]
    );
    let odd = render_scene_reference(
        RenderSize::pixels(8, 8),
        &fill_rules(FillRule::Evenodd),
        WorkingSpace::LinearRec709,
    )
    .unwrap();
    let winding = render_scene_reference(
        RenderSize::pixels(8, 8),
        &fill_rules(FillRule::Nonzero),
        WorkingSpace::LinearRec709,
    )
    .unwrap();
    assert_eq!(odd[3 * 8 + 3], [0.0; 4]);
    assert_eq!(winding[3 * 8 + 3], [1.0; 4]);
    let mut path = match rectangle(
        [0.0; 2],
        [1.0; 2],
        paint([1.0; 4], InputSpace::LinearRec709),
    ) {
        DrawNode::Path(p) => p,
        _ => unreachable!(),
    };
    path.contours = vec![Contour {
        points: vec![[0.5, 0.5], [2.5, 0.5]],
        closed: false,
    }];
    path.fill = None;
    path.stroke = Some(RoundStroke {
        join: StrokeJoin::Round,
        cap: StrokeCap::Round,
        miter_limit: 4.0,
        paint: paint([1.0; 4], InputSpace::LinearRec709),
        width: 1.0,
    });
    let p = render_scene_reference(
        RenderSize::pixels(3, 1),
        &DrawScene {
            nodes: vec![DrawNode::Path(path)],
            roots: vec![0],
        },
        WorkingSpace::LinearRec709,
    )
    .unwrap();
    assert_eq!(p[1], [1.0; 4]);
    assert_eq!(p[0][3], 0.875);
    assert_eq!(p[2][3], 0.875);
    assert_eq!(
        render_scene_reference(
            RenderSize::pixels(1, 1),
            &DrawScene {
                nodes: vec![],
                roots: vec![]
            },
            WorkingSpace::LinearRec709
        )
        .unwrap(),
        vec![[0.0; 4]]
    );
}
#[test]
fn cpu_invalid_references_cycles_geometry_and_depth_are_typed() {
    for scene in [
        DrawScene {
            nodes: vec![],
            roots: vec![0],
        },
        DrawScene {
            nodes: vec![DrawNode::Group {
                children: vec![0],
                opacity: 0.5,
            }],
            roots: vec![0],
        },
        DrawScene {
            nodes: vec![DrawNode::Masked {
                source: 0,
                matte: 3,
                kind: MaskKind::Alpha,
            }],
            roots: vec![0],
        },
    ] {
        assert!(matches!(scene.validate(), Err(GpuError::InvalidInput(_))));
    }
    let mut scene = isolated();
    if let DrawNode::Path(p) = &mut scene.nodes[0] {
        p.contours[0].points[0][0] = f32::NAN;
    }
    assert!(matches!(scene.validate(), Err(GpuError::InvalidInput(_))));
    let nodes = (0..33)
        .map(|i| DrawNode::Group {
            children: if i == 0 { vec![] } else { vec![i - 1] },
            opacity: 1.0,
        })
        .collect();
    assert!(matches!(
        DrawScene {
            nodes,
            roots: vec![32]
        }
        .validate(),
        Err(GpuError::UnsupportedFeature(_))
    ));
    assert!(matches!(
        render_scene_reference(
            RenderSize::pixels(u32::MAX, u32::MAX),
            &DrawScene {
                nodes: vec![],
                roots: vec![]
            },
            WorkingSpace::LinearRec709
        ),
        Err(GpuError::SurfaceBudgetExceeded)
    ));
}
#[test]
fn cpu_nested_opacity_and_mask_are_independently_analytic() {
    let p = render_scene_reference(
        RenderSize::pixels(8, 8),
        &isolated(),
        WorkingSpace::LinearRec709,
    )
    .unwrap();
    assert_eq!(p[3 * 8 + 3], [0.0, 0.0, 0.25, 0.25]);
    let wrong = color::source_over([0.0, 0.0, 0.25, 0.25], [0.25, 0.0, 0.0, 0.25]);
    assert!((wrong[3] - p[3 * 8 + 3][3]).abs() > 0.1);
    for working in [WorkingSpace::LinearRec709, WorkingSpace::LinearRec2020] {
        let alpha =
            render_scene_reference(RenderSize::pixels(8, 8), &matte(MaskKind::Alpha), working)
                .unwrap();
        assert_eq!(alpha[3 * 8 + 3][3], 0.375);
        assert_eq!(alpha[0], [0.0; 4]);
        let luma = render_scene_reference(
            RenderSize::pixels(8, 8),
            &matte(MaskKind::Luminance),
            working,
        )
        .unwrap();
        // Encoded .8 green must be decoded; red/blue contribute independently.
        let expected709 = 0.75
            * 0.5
            * (0.2126 * color::srgb_decode(0.1)
                + 0.7152 * color::srgb_decode(0.8)
                + 0.0722 * color::srgb_decode(0.3));
        assert!((luma[3 * 8 + 3][3] - expected709).abs() < 0.0001);
    }
}
#[test]
fn gpu_all_scene_pixels_match_cpu_reference() {
    for (id, n, space, scene) in scenes() {
        eprintln!("scene: {id}");
        let expected = render_scene_reference(RenderSize::pixels(n, n), &scene, space).unwrap();
        let actual = gpu()
            .render_scene(RenderSize::pixels(n, n), &scene, space)
            .unwrap();
        compare(n, space, &expected, &actual.pixels);
        assert_eq!(actual.transfers.cpu_upload_pixel_bytes, 0);
        assert_eq!(actual.transfers.gpu_readback_operations, 2);
        assert_eq!(actual.transfers.gpu_copy_operations, 1);
        for p in actual.pixels {
            if p[3] == 0.0 {
                assert_eq!(p[..3], [0.0; 3]);
            }
        }
    }
}
#[test]
fn gpu_isolated_overlap_differs_from_wrong_child_opacity() {
    let scene = isolated();
    let actual = gpu()
        .render_scene(RenderSize::pixels(8, 8), &scene, WorkingSpace::LinearRec709)
        .unwrap();
    assert_eq!(actual.pixels[27], [0.0, 0.0, 0.25, 0.25]);
    let mut wrong = scene;
    for id in [0, 1] {
        if let DrawNode::Path(p) = &mut wrong.nodes[id] {
            p.fill.as_mut().unwrap().paint.rgba[3] = 0.5;
        }
    }
    if let DrawNode::Group { opacity, .. } = &mut wrong.nodes[2] {
        *opacity = 1.0;
    }
    let bad = gpu()
        .render_scene(RenderSize::pixels(8, 8), &wrong, WorkingSpace::LinearRec709)
        .unwrap();
    assert!((bad.pixels[27][3] - actual.pixels[27][3]).abs() > 0.1);
    assert!((bad.pixels[27][0] - actual.pixels[27][0]).abs() > 0.1);
}
#[test]
fn gpu_matte_not_drawn_twice_and_shared_reference_is_reusable() {
    let mut scene = matte(MaskKind::Alpha);
    scene.nodes.push(DrawNode::Masked {
        source: 0,
        matte: 1,
        kind: MaskKind::Luminance,
    });
    scene.roots = vec![2, 3];
    let size = RenderSize::pixels(8, 8);
    let expected = render_scene_reference(size, &scene, WorkingSpace::LinearRec709).unwrap();
    compare(
        8,
        WorkingSpace::LinearRec709,
        &expected,
        &gpu()
            .render_scene(size, &scene, WorkingSpace::LinearRec709)
            .unwrap()
            .pixels,
    );
    scene.roots = vec![2];
    let single = gpu()
        .render_scene(size, &scene, WorkingSpace::LinearRec709)
        .unwrap();
    assert_eq!(single.pixels[27][3], 0.375);
    assert_eq!(single.pixels[0], [0.0; 4]);
    scene.roots.push(1);
    let visible = gpu()
        .render_scene(size, &scene, WorkingSpace::LinearRec709)
        .unwrap();
    assert!((visible.pixels[27][3] - single.pixels[27][3]).abs() > 0.1);
}
#[test]
fn gpu_shape_and_glyph_edges_have_no_dark_fringe() {
    for (scene, n, rgb) in [
        (
            DrawScene {
                nodes: vec![rectangle(
                    [0.3, 0.3],
                    [6.7, 6.7],
                    paint([1.0, 0.5, 0.25, 1.0], InputSpace::LinearRec709),
                )],
                roots: vec![0],
            },
            8,
            [1.0, 0.5, 0.25],
        ),
        (
            glyph(),
            32,
            [
                1.0,
                color::srgb_decode(80.0 / 255.0),
                color::srgb_decode(20.0 / 255.0),
            ],
        ),
    ] {
        let actual = gpu()
            .render_scene(RenderSize::pixels(n, n), &scene, WorkingSpace::LinearRec709)
            .unwrap();
        assert!(actual.pixels.iter().any(|p| p[3] > 0.0 && p[3] < 1.0));
        for p in actual.pixels {
            for i in 0..3 {
                assert!((p[i] - rgb[i] * p[3]).abs() <= 1.0 / 1024.0);
            }
        }
    }
}
#[test]
fn cpu_output_threshold_and_transfer_are_explicit() {
    let output = OutputTransform {
        space: InputSpace::Srgb,
        alpha: OutputAlpha::Straight,
    };
    for alpha in [0.0, color::ALPHA_EPSILON / 2.0, color::ALPHA_EPSILON] {
        assert_eq!(
            convert_output_reference([alpha, 0.0, 0.0, alpha], WorkingSpace::LinearRec709, output)
                .unwrap(),
            [0.0, 0.0, 0.0, alpha]
        );
    }
    let p =
        convert_output_reference([0.5, 0.0, 0.0, 0.5], WorkingSpace::LinearRec709, output).unwrap();
    assert!((p[0] - 1.0).abs() < 1e-6);
    assert_eq!(p[1..], [0.0, 0.0, 0.5]);
}
#[test]
fn gpu_color_output_unpremultiplies_before_encoding_and_keeps_alpha() {
    for working in [WorkingSpace::LinearRec709, WorkingSpace::LinearRec2020] {
        for input_alpha in [
            0.0,
            color::ALPHA_EPSILON / 2.0,
            color::ALPHA_EPSILON,
            color::ALPHA_EPSILON * 2.0,
            0.5,
            1.0,
        ] {
            let scene = DrawScene {
                nodes: vec![rectangle(
                    [0.0; 2],
                    [1.0; 2],
                    paint([0.8, 0.4, 0.2, input_alpha], InputSpace::Srgb),
                )],
                roots: vec![0],
            };
            let size = RenderSize::pixels(1, 1);
            let internal = gpu().render_scene(size, &scene, working).unwrap();
            for space in [
                InputSpace::Srgb,
                InputSpace::LinearRec709,
                InputSpace::LinearRec2020,
            ] {
                for alpha in [OutputAlpha::Straight, OutputAlpha::Premultiplied] {
                    let transform = OutputTransform { space, alpha };
                    let out = gpu()
                        .render_scene_output(size, &scene, working, transform)
                        .unwrap();
                    let expected =
                        convert_output_reference(internal.pixels[0], working, transform).unwrap();
                    for (a, e) in out.pixels[0].into_iter().zip(expected) {
                        assert!(
                            (a - e).abs() <= 1.0 / 1024.0 * e.abs().max(1.0),
                            "{working:?} {transform:?} {input_alpha}: {a} vs {e}"
                        );
                    }
                    assert_eq!(out.pixels[0][3], internal.pixels[0][3]);
                    if input_alpha == 0.5
                        && space == InputSpace::Srgb
                        && alpha == OutputAlpha::Straight
                    {
                        assert!((out.pixels[0][0] - 0.8).abs() < 0.001);
                    }
                }
            }
        }
    }
}

#[test]
fn cpu_all_catalog_scenes_are_finite_with_zero_transparent_rgb() {
    for (id, n, working, scene) in scenes() {
        let pixels = render_scene_reference(RenderSize::pixels(n, n), &scene, working).unwrap();
        assert!(pixels.iter().any(|p| p[3] > 0.0), "{id}");
        for p in pixels {
            assert!(p.iter().all(|v| v.is_finite()));
            assert!((0.0..=1.0).contains(&p[3]));
            if p[3] == 0.0 {
                assert_eq!(p[..3], [0.0; 3]);
            }
        }
    }
}
#[test]
fn gpu_round_stroke_open_contour_and_design_scale_match_analytic_values() {
    let scene = DrawScene {
        nodes: vec![DrawNode::Path(PathDraw {
            stroke_geometry: None,
            fill_gradient: None,
            stroke_gradient: None,
            paint_transform: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            contours: vec![Contour {
                points: vec![[0.5, 0.5], [2.5, 0.5]],
                closed: false,
            }],
            fill: None,
            stroke: Some(RoundStroke {
                join: StrokeJoin::Round,
                cap: StrokeCap::Round,
                miter_limit: 4.0,
                paint: paint([1.0; 4], InputSpace::LinearRec709),
                width: 1.0,
            }),
        })],
        roots: vec![0],
    };
    let out = gpu()
        .render_scene(RenderSize::pixels(3, 1), &scene, WorkingSpace::LinearRec709)
        .unwrap();
    assert_eq!(out.pixels, vec![[0.875; 4], [1.0; 4], [0.875; 4]]);
    let scene = DrawScene {
        nodes: vec![rectangle(
            [0.5, 0.0],
            [1.0, 1.0],
            paint([1.0; 4], InputSpace::LinearRec709),
        )],
        roots: vec![0],
    };
    let size = RenderSize {
        design_extent: [1.0; 2],
        output_resolution: [2, 2],
    };
    assert_eq!(
        gpu()
            .render_scene(size, &scene, WorkingSpace::LinearRec709)
            .unwrap()
            .pixels,
        vec![[0.0; 4], [1.0; 4], [0.0; 4], [1.0; 4]]
    );
}
#[test]
fn gpu_negative_hdr_rgb_and_alpha_underflow_have_explicit_semantics() {
    let scene = DrawScene {
        nodes: vec![rectangle(
            [0.0; 2],
            [1.0; 2],
            paint([-0.125, 2.0, 4.0, 0.5], InputSpace::LinearRec2020),
        )],
        roots: vec![0],
    };
    assert_eq!(
        gpu()
            .render_scene(
                RenderSize::pixels(1, 1),
                &scene,
                WorkingSpace::LinearRec2020
            )
            .unwrap()
            .pixels,
        vec![[-0.0625, 1.0, 2.0, 0.5]]
    );
    let scene = DrawScene {
        nodes: vec![rectangle(
            [0.0; 2],
            [1.0; 2],
            paint([4.0, 2.0, 1.0, 1.0 / 33554432.0], InputSpace::LinearRec709),
        )],
        roots: vec![0],
    };
    assert_eq!(
        gpu()
            .render_scene(RenderSize::pixels(1, 1), &scene, WorkingSpace::LinearRec709)
            .unwrap()
            .pixels,
        vec![[0.0; 4]]
    );
    let scene = DrawScene {
        nodes: vec![rectangle(
            [0.0; 2],
            [1.0; 2],
            paint([100_000.0, 0.0, 0.0, 1.0], InputSpace::LinearRec709),
        )],
        roots: vec![0],
    };
    assert!(matches!(
        gpu().render_scene(RenderSize::pixels(1, 1), &scene, WorkingSpace::LinearRec709),
        Err(GpuError::InvalidInput(_))
    ));
    assert!(matches!(
        gpu().render_scene(
            RenderSize::pixels(8192, 8192),
            &isolated(),
            WorkingSpace::LinearRec709
        ),
        Err(GpuError::SurfaceBudgetExceeded)
    ));
}

#[test]
fn cpu_surface_overflow_is_rejected_even_when_hidden_by_later_draws() {
    let scene = DrawScene {
        nodes: vec![
            rectangle(
                [0.0; 2],
                [1.0; 2],
                paint([65504.0, 0.0, 0.0, 1.0], InputSpace::LinearRec709),
            ),
            rectangle(
                [0.0; 2],
                [1.0; 2],
                paint([100000.0, 0.0, 0.0, 0.5], InputSpace::LinearRec709),
            ),
            rectangle(
                [0.0; 2],
                [1.0; 2],
                paint([0.0, 0.0, 0.0, 1.0], InputSpace::LinearRec709),
            ),
        ],
        roots: vec![0, 1, 2],
    };
    assert!(matches!(
        render_scene_reference(RenderSize::pixels(1, 1), &scene, WorkingSpace::LinearRec709),
        Err(GpuError::InvalidInput(_))
    ));
    let p = [50000.0, 0.0, 0.0, 0.5];
    assert!(matches!(
        convert_output_reference(
            p,
            WorkingSpace::LinearRec709,
            OutputTransform {
                space: InputSpace::LinearRec709,
                alpha: OutputAlpha::Straight
            }
        ),
        Err(GpuError::InvalidInput(_))
    ));
    assert_eq!(
        convert_output_reference(
            p,
            WorkingSpace::LinearRec709,
            OutputTransform {
                space: InputSpace::LinearRec709,
                alpha: OutputAlpha::Premultiplied
            }
        )
        .unwrap(),
        p
    );
}
#[test]
fn gpu_surface_overflow_status_is_sticky_and_output_association_is_checked() {
    let scene = DrawScene {
        nodes: vec![
            rectangle(
                [0.0; 2],
                [1.0; 2],
                paint([65504.0, 0.0, 0.0, 1.0], InputSpace::LinearRec709),
            ),
            rectangle(
                [0.0; 2],
                [1.0; 2],
                paint([100000.0, 0.0, 0.0, 0.5], InputSpace::LinearRec709),
            ),
            rectangle(
                [0.0; 2],
                [1.0; 2],
                paint([0.0, 0.0, 0.0, 1.0], InputSpace::LinearRec709),
            ),
        ],
        roots: vec![0, 1, 2],
    };
    assert!(matches!(
        gpu().render_scene(RenderSize::pixels(1, 1), &scene, WorkingSpace::LinearRec709),
        Err(GpuError::InvalidInput(_))
    ));
    let scene = DrawScene {
        nodes: vec![scene.nodes[1].clone()],
        roots: vec![0],
    };
    assert!(
        gpu()
            .render_scene(RenderSize::pixels(1, 1), &scene, WorkingSpace::LinearRec709)
            .is_ok()
    );
    assert!(matches!(
        gpu().render_scene_output(
            RenderSize::pixels(1, 1),
            &scene,
            WorkingSpace::LinearRec709,
            OutputTransform {
                space: InputSpace::LinearRec709,
                alpha: OutputAlpha::Straight
            }
        ),
        Err(GpuError::InvalidInput(_))
    ));
    assert!(
        gpu()
            .render_scene_output(
                RenderSize::pixels(1, 1),
                &scene,
                WorkingSpace::LinearRec709,
                OutputTransform {
                    space: InputSpace::LinearRec709,
                    alpha: OutputAlpha::Premultiplied
                }
            )
            .is_ok()
    );
}

#[test]
fn cpu_gradient_pad_premultiplied_equal_offsets_and_working_conversion() {
    let mut g = GradientPaint {
        spread: Default::default(),
        interpolation: Default::default(),
        interpolation_version: 1,
        transform: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        geometry: GradientGeometry::Linear {
            start: [0.0, 0.0],
            end: [2.0, 0.0],
        },
        stops: vec![
            GradientStop {
                offset: 0.0,
                paint: paint([1.0, 0.0, 0.0, 1.0], InputSpace::Srgb),
            },
            GradientStop {
                offset: 1.0,
                paint: paint([0.0, 0.0, 1.0, 0.0], InputSpace::Srgb),
            },
        ],
    };
    g.validate().unwrap();
    assert_eq!(
        g.sample([-2.0, 0.0], WorkingSpace::LinearRec709),
        [1.0, 0.0, 0.0, 1.0]
    );
    assert_eq!(g.sample([3.0, 0.0], WorkingSpace::LinearRec709), [0.0; 4]);
    let midpoint = g.sample([1.0, 0.0], WorkingSpace::LinearRec709);
    assert_eq!(midpoint, [0.5, 0.0, 0.0, 0.5]);
    assert_eq!(
        color::unpremultiply_external(midpoint),
        [1.0, 0.0, 0.0, 0.5]
    );
    let wrong_straight_then_premultiply = color::premultiply([0.5, 0.0, 0.5, 0.5]);
    assert!(midpoint[0] > wrong_straight_then_premultiply[0]);
    let converted = g.sample([1.0, 0.0], WorkingSpace::LinearRec2020);
    let red = color::to_working(
        [1.0, 0.0, 0.0, 1.0],
        InputSpace::Srgb,
        WorkingSpace::LinearRec2020,
    );
    assert_eq!(converted, red.map(|v| v * 0.5));
    g.geometry = GradientGeometry::Radial {
        center: [2.0, 3.0],
        radius: 2.0,
    };
    assert_eq!(
        g.sample([2.0, 3.0], WorkingSpace::LinearRec709),
        [1.0, 0.0, 0.0, 1.0]
    );
    assert_eq!(g.sample([3.0, 3.0], WorkingSpace::LinearRec709), midpoint);
    assert_eq!(g.sample([5.0, 3.0], WorkingSpace::LinearRec709), [0.0; 4]);
    g.stops.insert(
        1,
        GradientStop {
            offset: 0.0,
            paint: paint([0.0, 1.0, 0.0, 1.0], InputSpace::LinearRec709),
        },
    );
    assert_eq!(
        g.sample([2.0, 3.0], WorkingSpace::LinearRec709),
        [0.0, 1.0, 0.0, 1.0]
    );
    assert_eq!(
        g.sample([3.0, 3.0], WorkingSpace::LinearRec709),
        [0.0, 0.5, 0.0, 0.5]
    );
    let mut linear = g.clone();
    linear.geometry = GradientGeometry::Linear {
        start: [0.0, 0.0],
        end: [2.0, 0.0],
    };
    linear.stops = vec![
        GradientStop {
            offset: 0.0,
            paint: paint([1.0, 0.0, 0.0, 1.0], InputSpace::LinearRec709),
        },
        GradientStop {
            offset: 0.5,
            paint: paint([0.0, 1.0, 0.0, 1.0], InputSpace::LinearRec709),
        },
        GradientStop {
            offset: 0.5,
            paint: paint([0.0, 0.0, 1.0, 1.0], InputSpace::LinearRec709),
        },
        GradientStop {
            offset: 1.0,
            paint: paint([0.0, 0.0, 0.0, 1.0], InputSpace::LinearRec709),
        },
    ];
    assert_eq!(
        linear.sample([0.5, 0.0], WorkingSpace::LinearRec709),
        [0.5, 0.5, 0.0, 1.0]
    );
    assert_eq!(
        linear.sample([1.0, 0.0], WorkingSpace::LinearRec709),
        [0.0, 0.0, 1.0, 1.0]
    );
    for geometry in [
        GradientGeometry::Linear {
            start: [0.0; 2],
            end: [0.0; 2],
        },
        GradientGeometry::Radial {
            center: [0.0; 2],
            radius: 0.0,
        },
    ] {
        let mut invalid = linear.clone();
        invalid.geometry = geometry;
        assert!(invalid.validate().is_err());
    }
    for count in [0, 1, 257] {
        let mut invalid = linear.clone();
        invalid.stops = vec![linear.stops[0]; count];
        assert!(invalid.validate().is_err());
    }
    g.stops[1].offset = -0.1;
    assert!(g.validate().is_err());
    g.stops[1].offset = 1.1;
    assert!(g.validate().is_err());
    g.stops[1].offset = f32::NAN;
    assert!(g.validate().is_err());
}
#[test]
fn gpu_stroke_styles_and_gradients_match_cpu_in_both_working_spaces() {
    for (id, n, _, scene) in vec003_scenes() {
        for working in [WorkingSpace::LinearRec709, WorkingSpace::LinearRec2020] {
            eprintln!("VEC-003 {id}: {working:?}");
            let expected =
                render_scene_reference(RenderSize::pixels(n, n), &scene, working).unwrap();
            let actual = gpu()
                .render_scene(RenderSize::pixels(n, n), &scene, working)
                .unwrap();
            compare(n, working, &expected, &actual.pixels);
        }
    }
}

#[test]
fn gpu_vec004_gradients_match_cpu_in_both_working_spaces() {
    for (id, n, _, scene) in vec004_scenes() {
        for working in [WorkingSpace::LinearRec709, WorkingSpace::LinearRec2020] {
            eprintln!("VEC-004 {id}: {working:?}");
            let expected =
                render_scene_reference(RenderSize::pixels(n, n), &scene, working).unwrap();
            let actual = gpu()
                .render_scene(RenderSize::pixels(n, n), &scene, working)
                .unwrap();
            compare(n, working, &expected, &actual.pixels);
        }
    }
}

#[test]
fn cpu_fx_shader_parses_and_validates() {
    let module = naga::front::wgsl::parse_str(EFFECT_SHADER).unwrap();
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .unwrap();
}
#[test]
fn cpu_fx_gaussian_impulse_is_linear_premultiplied_and_transparent_edges() {
    let scene = DrawScene {
        nodes: vec![
            rectangle(
                [3.0, 3.0],
                [4.0, 4.0],
                paint([0.5, 0.0, 0.0, 0.5], InputSpace::Srgb),
            ),
            DrawNode::Effect {
                source: 0,
                effect: PixelEffect::GaussianBlur { sigma: [1.0; 2] },
            },
        ],
        roots: vec![1],
    };
    let pixels =
        render_scene_reference(RenderSize::pixels(8, 8), &scene, WorkingSpace::LinearRec709)
            .unwrap();
    let kernel = kronello_render::gaussian_kernel(1.0).unwrap();
    let norm: f32 = kernel.iter().sum();
    let half = |v| half::f16::from_f32(v).to_f32();
    let input = half(color::srgb_decode(0.5) * 0.5);
    let mut expected_sum = 0.0;
    for y in 0..8 {
        for x in 0..8 {
            // Independent impulse derivation includes horizontal and vertical stores.
            let rgb = if x < 7 && y < 7 {
                half(half(input * kernel[x] / norm) * kernel[y] / norm)
            } else {
                0.0
            };
            let alpha = if x < 7 && y < 7 {
                half(half(0.5 * kernel[x] / norm) * kernel[y] / norm)
            } else {
                0.0
            };
            let p = pixels[y * 8 + x];
            assert_eq!(p, [rgb, 0.0, 0.0, alpha]);
            expected_sum += alpha;
        }
    }
    assert_eq!(pixels.iter().map(|p| p[3]).sum::<f32>(), expected_sum);
}
#[test]
fn cpu_fx_shadow_zero_sigma_fractional_offset_color_opacity_and_under_source() {
    let shadow_color =
        kronello_model::Color::new(kronello_model::ColorSpace::Srgb, [0.5, 0.0, 0.0], 0.5).unwrap();
    let scene = DrawScene {
        nodes: vec![
            rectangle(
                [2.0; 2],
                [3.0; 2],
                paint([0.0, 1.0, 0.0, 0.5], InputSpace::LinearRec709),
            ),
            DrawNode::Effect {
                source: 0,
                effect: PixelEffect::DropShadow {
                    sigma: [0.0; 2],
                    offset: [0.5, 0.0],
                    color: shadow_color,
                    opacity: 0.5,
                },
            },
        ],
        roots: vec![1],
    };
    let pixels =
        render_scene_reference(RenderSize::pixels(6, 6), &scene, WorkingSpace::LinearRec709)
            .unwrap();
    let alpha = 0.5 * 0.5 * 0.5 * 0.5;
    assert!(
        (pixels[2 * 6 + 3][0] - half::f16::from_f32(color::srgb_decode(0.5) * alpha).to_f32())
            .abs()
            < 1e-7
    );
    assert_eq!(pixels[2 * 6 + 3][3], alpha);
    assert_eq!(pixels[2 * 6 + 2][1], 0.5);
    assert_eq!(pixels[2 * 6 + 2][3], 0.5 + alpha * 0.5);
}
#[test]
fn gpu_fx_blur_shadow_match_cpu_reference_in_both_working_spaces() {
    for working in [WorkingSpace::LinearRec709, WorkingSpace::LinearRec2020] {
        for shadow in [false, true] {
            let scene = effect_scene(shadow, working);
            let expected =
                render_scene_reference(RenderSize::pixels(16, 16), &scene, working).unwrap();
            let actual = gpu()
                .render_scene(RenderSize::pixels(16, 16), &scene, working)
                .unwrap();
            compare(16, working, &expected, &actual.pixels);
            assert_eq!(actual.transfers.cpu_upload_pixel_operations, 0);
        }
    }
}

#[test]
fn gpu_fx002_transformed_kernels_and_offsets_match_cpu_in_both_spaces() {
    for (_, size, _, scene) in fx002_scenes() {
        for working in [WorkingSpace::LinearRec709, WorkingSpace::LinearRec2020] {
            let expected =
                render_scene_reference(RenderSize::pixels(size, size), &scene, working).unwrap();
            let actual = gpu()
                .render_scene(RenderSize::pixels(size, size), &scene, working)
                .unwrap();
            compare(size, working, &expected, &actual.pixels);
            assert_eq!(actual.transfers.cpu_upload_pixel_operations, 0);
        }
    }
}
#[test]
fn cpu_fx002_affine_impulse_matches_independent_elliptical_gaussian() {
    let covariance = [5.0, 1.0, 1.0];
    let mut source = vec![[0.0; 4]; 31 * 31];
    source[15 * 31 + 15] = [0.25, 0.0, 0.0, 0.5];
    let scene = DrawScene {
        nodes: vec![
            DrawNode::Raster(source),
            DrawNode::Effect {
                source: 0,
                effect: PixelEffect::AffineGaussianBlur { covariance },
            },
        ],
        roots: vec![1],
    };
    let pixels = render_scene_reference(
        RenderSize::pixels(31, 31),
        &scene,
        WorkingSpace::LinearRec709,
    )
    .unwrap();
    let mut weights = vec![];
    for y in -3..=3 {
        for x in -7..=7 {
            let q = f64::from(x * x - 2 * x * y + 5 * y * y) / 4.0;
            if q <= 9.0 {
                weights.push((x, y, (-0.5 * q).exp()));
            }
        }
    }
    let sum: f64 = weights.iter().map(|(_, _, w)| w).sum();
    let norm: f32 = weights.iter().map(|(_, _, w)| (w / sum) as f32).sum();
    let half = |v| half::f16::from_f32(v).to_f32();
    let mut expected = vec![[0.0; 4]; 31 * 31];
    for (x, y, w) in weights {
        let w = (w / sum) as f32;
        expected[((15 + y) * 31 + 15 + x) as usize] =
            [half(0.25 * w / norm), 0.0, 0.0, half(0.5 * w / norm)];
    }
    for (i, (actual, expected)) in pixels.iter().zip(expected).enumerate() {
        assert_eq!(*actual, expected, "affine impulse pixel {i}");
    }
    let invalid = DrawScene {
        nodes: vec![
            DrawNode::Raster(vec![[0.0; 4]; 1]),
            DrawNode::Effect {
                source: 0,
                effect: PixelEffect::AffineGaussianBlur {
                    covariance: [1.0, 1.0, 1.0],
                },
            },
        ],
        roots: vec![1],
    };
    assert!(matches!(
        render_scene_reference(
            RenderSize::pixels(1, 1),
            &invalid,
            WorkingSpace::LinearRec709
        ),
        Err(GpuError::UnsupportedFeature(_))
    ));
}

#[test]
fn cpu_vec005_catalog_rasterizes_local_affine_and_alignment() {
    for (_, n, _, scene) in vec005_scenes() {
        scene.validate().unwrap();
        for working in [WorkingSpace::LinearRec709, WorkingSpace::LinearRec2020] {
            let pixels = render_scene_reference(RenderSize::pixels(n, n), &scene, working).unwrap();
            assert!(pixels.iter().any(|p| p[3] > 0.0));
            assert!(pixels.iter().any(|p| p[3] == 0.0));
        }
    }
}
#[test]
fn gpu_vec005_strokes_match_cpu_in_both_working_spaces() {
    for (id, n, _, scene) in vec005_scenes() {
        for working in [WorkingSpace::LinearRec709, WorkingSpace::LinearRec2020] {
            eprintln!("VEC-005 {id}: {working:?}");
            let expected =
                render_scene_reference(RenderSize::pixels(n, n), &scene, working).unwrap();
            let actual = gpu()
                .render_scene(RenderSize::pixels(n, n), &scene, working)
                .unwrap();
            compare(n, working, &expected, &actual.pixels);
        }
    }
}

#[test]
fn cpu_vec005_golden_samples_avoid_discontinuity_ties() {
    for (id, n, _, scene) in vec005_scenes() {
        let baseline =
            render_scene_reference(RenderSize::pixels(n, n), &scene, WorkingSpace::LinearRec709)
                .unwrap();
        for dx in [-0.00001, 0.00001] {
            for dy in [-0.00001, 0.00001] {
                let mut perturbed = scene.clone();
                let DrawNode::Path(path) = &mut perturbed.nodes[0] else {
                    panic!()
                };
                let g = path.stroke_geometry.as_mut().unwrap();
                g.output_to_local[0][2] += dx;
                g.output_to_local[1][2] += dy;
                for c in &mut path.contours {
                    for p in &mut c.points {
                        p[0] += dx;
                        p[1] += dy;
                    }
                }
                let pixels = render_scene_reference(
                    RenderSize::pixels(n, n),
                    &perturbed,
                    WorkingSpace::LinearRec709,
                )
                .unwrap();
                assert_eq!(
                    pixels, baseline,
                    "{id}: sample too close to stroke/fill discontinuity at perturbation {dx}/{dy}"
                );
            }
        }
    }
}

#[test]
fn gpu_inverted_alpha_and_luminance_mattes_match_reference_in_both_working_spaces() {
    for working in [WorkingSpace::LinearRec709, WorkingSpace::LinearRec2020] {
        for kind in [MaskKind::AlphaInverted, MaskKind::LuminanceInverted] {
            let scene = matte(kind);
            let size = RenderSize::pixels(8, 8);
            let expected = render_scene_reference(size, &scene, working).unwrap();
            let actual = gpu().render_scene(size, &scene, working).unwrap();
            compare(8, working, &expected, &actual.pixels);
            assert!(actual.pixels.iter().any(|p| p[3] > 0.0));
        }
    }
}

fn blend_scene(src: [f32; 4], dst: [f32; 4], mode: kronello_model::BlendMode) -> DrawScene {
    DrawScene {
        nodes: vec![
            DrawNode::Raster(vec![dst; 4]),
            DrawNode::Raster(vec![src; 4]),
            DrawNode::Blend {
                source: 1,
                backdrop: 0,
                mode,
            },
        ],
        roots: vec![2],
    }
}
#[test]
fn cpu_linear_blend_transparency_hdr_and_partial_alpha_have_correct_pixels() {
    use kronello_model::BlendMode::*;
    for (mode, expected) in [
        (Normal, [0.275, 0.275, 0.1375, 0.625]),
        (Multiply, [0.195, 0.265, 0.0925, 0.625]),
        (Screen, [0.28, 0.335, 0.145, 0.625]),
    ] {
        let pixels = render_scene_reference(
            RenderSize::pixels(2, 2),
            &blend_scene([0.2, 0.05, 0.1, 0.25], [0.1, 0.3, 0.05, 0.5], mode),
            WorkingSpace::LinearRec709,
        )
        .unwrap();
        for pixel in pixels {
            for (a, e) in pixel.into_iter().zip(expected) {
                assert!((a - e).abs() < 1e-6);
            }
        }
        for color in [[0.2, 0.05, 0.1, 0.25], [4., -0.5, 2., 1.]] {
            for (src, dst) in [(color, [0.; 4]), ([0.; 4], color)] {
                let pixels = render_scene_reference(
                    RenderSize::pixels(2, 2),
                    &blend_scene(src, dst, mode),
                    WorkingSpace::LinearRec2020,
                )
                .unwrap();
                assert_eq!(pixels, vec![color; 4]);
            }
        }
    }
    let multiply = render_scene_reference(
        RenderSize::pixels(2, 2),
        &blend_scene([2., 0.5, 1., 1.], [3., 0.5, 2., 1.], Multiply),
        WorkingSpace::LinearRec2020,
    )
    .unwrap();
    assert_eq!(multiply[0], [6., 0.25, 2., 1.]);
    let screen = render_scene_reference(
        RenderSize::pixels(2, 2),
        &blend_scene([2., 0.5, 1., 1.], [3., 0.5, 2., 1.], Screen),
        WorkingSpace::LinearRec2020,
    )
    .unwrap();
    assert_eq!(screen[0], [-1., 0.75, 1., 1.]);
}
#[test]
fn gpu_linear_blend_matches_cpu_after_isolation_opacity_effect_and_matte() {
    use kronello_model::BlendMode::*;
    for working in [WorkingSpace::LinearRec709, WorkingSpace::LinearRec2020] {
        for mode in [Normal, Multiply, Screen] {
            for (src, dst) in [
                ([0.4, 0.1, 0.2, 0.5], [0.1, 0.3, 0.05, 0.5]),
                ([0.; 4], [4., -0.5, 2., 1.]),
                ([4., -0.5, 2., 1.], [0.; 4]),
                ([2., 0.5, 1., 1.], [3., 0.5, 2., 1.]),
            ] {
                let scene = DrawScene {
                    nodes: vec![
                        DrawNode::Raster(vec![dst; 4]),
                        DrawNode::Raster(vec![src; 4]),
                        DrawNode::Group {
                            children: vec![1],
                            opacity: 0.5,
                        },
                        DrawNode::Effect {
                            source: 2,
                            effect: kronello_render::PixelEffect::GaussianBlur {
                                sigma: [0.5, 0.75],
                            },
                        },
                        DrawNode::Raster(vec![[0., 0., 0., 0.5]; 4]),
                        DrawNode::Masked {
                            source: 3,
                            matte: 4,
                            kind: MaskKind::Alpha,
                        },
                        DrawNode::Blend {
                            source: 5,
                            backdrop: 0,
                            mode,
                        },
                    ],
                    roots: vec![6],
                };
                let size = RenderSize::pixels(2, 2);
                let expected = render_scene_reference(size, &scene, working).unwrap();
                let actual = gpu().render_scene(size, &scene, working).unwrap();
                compare(2, working, &expected, &actual.pixels);
            }
        }
    }
}

#[test]
fn cpu_fx003_blend_oracle_matches_w3c_channel_functions() {
    use kronello_model::BlendMode::*;
    // Opaque operands isolate the per-channel blend function B(cb, cs).
    let b = [0.3, 0.7, 0.2, 1.0];
    let s = [0.6, 0.2, 0.9, 1.0];
    let near = |actual: [f32; 4], rgb: [f32; 3]| {
        for (a, e) in actual.into_iter().zip([rgb[0], rgb[1], rgb[2], 1.0]) {
            assert!((a - e).abs() < 1e-6, "{actual:?} != {e}");
        }
    };
    let blend = |mode| {
        render_scene_reference(
            RenderSize::pixels(2, 2),
            &blend_scene(s, b, mode),
            WorkingSpace::LinearRec709,
        )
        .unwrap()[0]
    };
    let sqrt_03 = 0.3_f32.sqrt();
    near(blend(Multiply), [0.18, 0.14, 0.18]);
    near(blend(Screen), [0.72, 0.76, 0.92]);
    near(blend(Darken), [0.3, 0.2, 0.2]);
    near(blend(Lighten), [0.6, 0.7, 0.9]);
    near(blend(ColorDodge), [0.75, 0.875, 1.0]);
    near(blend(ColorBurn), [0.0, 0.0, 1.0 - 0.8 / 0.9]);
    near(
        blend(HardLight),
        [
            1.0 - 2.0 * 0.7 * 0.4,
            2.0 * 0.7 * 0.2,
            1.0 - 2.0 * 0.8 * 0.1,
        ],
    );
    near(
        blend(SoftLight),
        [
            0.3 + 0.2 * (sqrt_03 - 0.3),
            0.7 - 0.6 * 0.7 * 0.3,
            0.2 + 0.8 * ((((16.0 * 0.2 - 12.0) * 0.2 + 4.0) * 0.2) - 0.2),
        ],
    );
    near(blend(Difference), [0.3, 0.5, 0.7]);
    near(blend(Exclusion), [0.54, 0.62, 0.74]);
    near(blend(Overlay), [0.36, 1.0 - 2.0 * 0.3 * 0.8, 0.36]);
    near(blend(LinearDodge), [0.9, 0.9, 1.1]);
    near(blend(LinearBurn), [-0.1, -0.1, 0.1]);
    near(blend(VividLight), [0.375, 0.25, 1.0]);
    near(blend(LinearLight), [0.5, 0.1, 1.0]);
    // Non-separable modes pin the Rec.601 luma and no-gamut-clip contract.
    let lum = |c: [f32; 3]| c[0] * 0.3 + c[1] * 0.59 + c[2] * 0.11;
    let cb = [0.3, 0.7, 0.2];
    let cs = [0.6, 0.2, 0.9];
    let dl = lum(cs) - lum(cb);
    // Luminosity keeps the backdrop hue/saturation at the source luma; Color
    // keeps the source hue/saturation at the backdrop luma (no gamut clip).
    near(blend(Luminosity), [cb[0] + dl, cb[1] + dl, cb[2] + dl]);
    near(blend(Color), [cs[0] - dl, cs[1] - dl, cs[2] - dl]);
    // Partial alpha still composites through the W3C α terms and keeps the
    // output alpha equal to source-over.
    let partial = render_scene_reference(
        RenderSize::pixels(2, 2),
        &blend_scene([0.2, 0.05, 0.1, 0.25], [0.1, 0.3, 0.05, 0.5], HardLight),
        WorkingSpace::LinearRec709,
    )
    .unwrap()[0];
    let cs = [0.8, 0.2, 0.4];
    let cb = [0.2, 0.6, 0.1];
    // HardLight applies overlay_branch(s, b): channel 0 branches high
    // (s=0.8 > 0.5) while channels 1 and 2 take the 2*b*s branch.
    let b = [
        1.0 - 2.0 * (1.0 - cb[0]) * (1.0 - cs[0]),
        2.0 * cb[1] * cs[1],
        2.0 * cb[2] * cs[2],
    ];
    let (sa, da) = (0.25f32, 0.5f32);
    let expected: [f32; 4] = [
        sa * cs[0] * (1.0 - da) + da * cb[0] * (1.0 - sa) + sa * da * b[0],
        sa * cs[1] * (1.0 - da) + da * cb[1] * (1.0 - sa) + sa * da * b[1],
        sa * cs[2] * (1.0 - da) + da * cb[2] * (1.0 - sa) + sa * da * b[2],
        sa + da * (1.0 - sa),
    ];
    for (a, e) in partial.into_iter().zip(expected) {
        assert!((a - e).abs() < 1e-6, "{partial:?} != {expected:?}");
    }
}

#[test]
fn gpu_fx003_all_blend_modes_match_cpu_reference_in_both_spaces() {
    use kronello_model::BlendMode::*;
    let modes = [
        Normal,
        Multiply,
        Screen,
        Overlay,
        Darken,
        Lighten,
        ColorDodge,
        ColorBurn,
        HardLight,
        SoftLight,
        Difference,
        Exclusion,
        LinearDodge,
        LinearBurn,
        VividLight,
        LinearLight,
        Hue,
        Saturation,
        Color,
        Luminosity,
    ];
    for working in [WorkingSpace::LinearRec709, WorkingSpace::LinearRec2020] {
        for mode in modes {
            for (src, dst) in [
                ([0.4, 0.1, 0.2, 0.5], [0.1, 0.3, 0.05, 0.5]),
                // Dodge/burn divisions amplify the f16 surface quantization
                // near the saturation cusp; these inputs stay interior, per
                // the golden rule of avoiding exact ties.
                ([0.6, 0.4, 0.5, 1.0], [0.3, 0.7, 0.1, 1.0]),
                ([0.05, 0.02, 0.08, 0.25], [0.4, 0.5, 0.1, 0.75]),
            ] {
                let scene = blend_scene(src, dst, mode);
                let size = RenderSize::pixels(2, 2);
                let expected = render_scene_reference(size, &scene, working).unwrap();
                let actual = gpu().render_scene(size, &scene, working).unwrap();
                let d = FrameDescriptor {
                    width: 2,
                    height: 2,
                    origin: [0; 2],
                    time: [0, 1],
                    working_space: if working == WorkingSpace::LinearRec709 {
                        kronello_testkit::WorkingSpace::LinearRec709
                    } else {
                        kronello_testkit::WorkingSpace::LinearRec2020
                    },
                    color_pipeline_id: "vec003-grid4-v2".into(),
                    samples_per_frame: 16,
                    seed: 0,
                };
                compare_pixels(
                    LinearFrame {
                        descriptor: &d,
                        pixels: &expected,
                    },
                    LinearFrame {
                        descriptor: &d,
                        pixels: &actual.pixels,
                    },
                    PixelTolerance::default(),
                )
                .unwrap_or_else(|e| {
                    panic!(
                        "{mode:?} {src:?} over {dst:?}: {e}\nexpected {expected:?}\nactual {:?}",
                        actual.pixels
                    )
                });
            }
        }
    }
}

fn color_effect_scene(effect: kronello_render::PixelEffect, input: [f32; 4]) -> DrawScene {
    DrawScene {
        nodes: vec![
            DrawNode::Raster(vec![input; 4]),
            DrawNode::Effect { source: 0, effect },
        ],
        roots: vec![1],
    }
}

#[test]
fn cpu_color002_pointwise_effects_preserve_alpha_and_extended_range() {
    use kronello_render::PixelEffect::*;
    let run = |effect, input| {
        render_scene_reference(
            RenderSize::pixels(2, 2),
            &color_effect_scene(effect, input),
            WorkingSpace::LinearRec709,
        )
        .unwrap()[0]
    };
    // Effect stages round-trip through RGBA16F surfaces.
    let h = |v: f32| half::f16::from_f32(v).to_f32();
    // Exposure: premultiplied RGB is scaled by 2^e and offset; alpha is kept.
    let p = run(
        ColorExposure {
            exposure: 1.0,
            offset: 0.1,
        },
        [0.25, 0.5, 1.0, 0.5],
    );
    for (a, e) in p.into_iter().zip([0.6, 1.1, 2.1, 0.5].map(h)) {
        assert_eq!(a, e, "{p:?}");
    }
    // HDR and negative premultiplied values pass through unclamped.
    let hdr = run(
        ColorExposure {
            exposure: -1.0,
            offset: 0.0,
        },
        [4.0, -0.5, 2.0, 1.0],
    );
    for (a, e) in hdr.into_iter().zip([2.0, -0.25, 1.0, 1.0].map(h)) {
        assert_eq!(a, e, "{hdr:?}");
    }
    // Levels: linear remap with gamma; extrapolation stays linear-domain.
    let levels = ColorLevels {
        in_black: 0.0,
        in_white: 0.5,
        gamma: 1.0,
        out_black: 0.25,
        out_white: 0.75,
    };
    for (a, e) in run(levels, [0.25, 0.5, 0.125, 1.0])
        .into_iter()
        .zip([0.5, 0.75, 0.375, 1.0])
    {
        assert!((a - e).abs() < 1e-6);
    }
    // Curves: control points are interpolated exactly; identity is stable.
    let identity = run(
        ColorCurves {
            points: vec![[0.0, 0.0], [1.0, 1.0]],
        },
        [0.3, 0.6, 0.9, 0.75],
    );
    for (a, e) in identity.into_iter().zip([0.3, 0.6, 0.9, 0.75].map(h)) {
        assert!((a - e).abs() < 1e-6);
    }
    let curved = run(
        ColorCurves {
            points: vec![[0.0, 0.0], [0.5, 0.75], [1.0, 1.0]],
        },
        [0.5, 0.25, 0.75, 1.0],
    );
    assert!((curved[0] - 0.75).abs() < 1e-6, "{curved:?}");
    // HSL: hue shift of +120 degrees maps pure red onto pure green exactly.
    let shifted = run(
        ColorHsl {
            hue_shift: 120.0,
            saturation: 1.0,
            lightness: 0.0,
        },
        [1.0, 0.0, 0.0, 0.4],
    );
    for (a, e) in shifted.into_iter().zip([0.0, 1.0, 0.0, 0.4].map(h)) {
        assert!((a - e).abs() < 1e-6, "{shifted:?}");
    }
    // Saturation zero removes chroma at constant lightness.
    let gray = run(
        ColorHsl {
            hue_shift: 0.0,
            saturation: 0.0,
            lightness: 0.0,
        },
        [0.5, 0.0, 1.0, 1.0],
    );
    assert!((gray[0] - gray[1]).abs() < 1e-6 && (gray[1] - gray[2]).abs() < 1e-6);
    assert!((gray[0] - 0.5).abs() < 1e-6, "{gray:?}");
}

#[test]
fn gpu_color002_pointwise_effects_match_cpu_reference_in_both_spaces() {
    use kronello_render::PixelEffect::*;
    let effects = [
        ColorExposure {
            exposure: 1.25,
            offset: -0.05,
        },
        ColorLevels {
            in_black: 0.1,
            in_white: 0.9,
            gamma: 2.2,
            out_black: -0.05,
            out_white: 1.05,
        },
        ColorCurves {
            points: vec![
                [0.0, 0.0],
                [0.25, 0.4],
                [0.5, 0.55],
                [0.75, 0.7],
                [1.0, 1.0],
            ],
        },
        ColorHsl {
            hue_shift: 30.0,
            saturation: 1.25,
            lightness: -0.05,
        },
    ];
    for working in [WorkingSpace::LinearRec709, WorkingSpace::LinearRec2020] {
        for effect in &effects {
            // Partial alpha, HDR and negative premultiplied channels exercise
            // the unclamped contract on both paths.
            for input in [
                [0.25, 0.5, 1.0, 0.5],
                [4.0, -0.5, 2.0, 1.0],
                [0.05, 0.1, 0.02, 0.25],
            ] {
                let scene = color_effect_scene(effect.clone(), input);
                let size = RenderSize::pixels(2, 2);
                let expected = render_scene_reference(size, &scene, working).unwrap();
                let actual = gpu().render_scene(size, &scene, working).unwrap();
                compare(2, working, &expected, &actual.pixels);
            }
        }
    }
}

/// COLOR-003 fixture lattice (ADR-0113): a 5^3 channel-rotating `.cube` with a
/// non-default domain so normalization, tetrahedral interpolation and endpoint
/// clamping are all exercised on both backends.
fn color003_lut() -> kronello_model::CubeLut {
    let size = 5u32;
    let mut text =
        String::from("LUT_3D_SIZE 5\nDOMAIN_MIN -0.1 -0.1 -0.1\nDOMAIN_MAX 1.1 1.1 1.1\n");
    for b in 0..size {
        for g in 0..size {
            for r in 0..size {
                let (r, g, b) = (
                    f32::from(r as u8) / (size - 1) as f32,
                    f32::from(g as u8) / (size - 1) as f32,
                    f32::from(b as u8) / (size - 1) as f32,
                );
                // Output = channel rotation with a small deterministic bias.
                text.push_str(&format!("{g} {} {r}\n", (b * 0.8 + 0.1).min(1.0)));
            }
        }
    }
    kronello_model::CubeLut::parse(text.as_bytes()).unwrap()
}

#[test]
fn cpu_color003_lut_samples_straight_rgb_preserves_alpha_and_clamps_domain() {
    let lut = color003_lut();
    let run = |intensity, input| {
        render_scene_reference(
            RenderSize::pixels(2, 2),
            &color_effect_scene(
                kronello_render::PixelEffect::ColorLut {
                    lut: lut.clone(),
                    intensity,
                },
                input,
            ),
            WorkingSpace::LinearRec709,
        )
        .unwrap()[0]
    };
    let h = |v: f32| half::f16::from_f32(v).to_f32();
    // intensity 0 is the identity even through premultiplied storage.
    let input = [0.25, 0.5, 1.0, 0.5];
    let identity = run(0.0, input);
    for (a, e) in identity.into_iter().zip(input.map(h)) {
        assert_eq!(a, e, "{identity:?}");
    }
    // Full intensity samples the rotated lattice; alpha is exactly preserved.
    let mapped = run(1.0, input);
    assert_eq!(mapped[3], h(0.5));
    assert_ne!(mapped[0], h(0.25));
    // HDR premultiplied input unpremultiplies to a domain-external straight
    // value and clamps to the lattice endpoint color instead of failing.
    let hdr = run(1.0, [8.0, 4.0, 2.0, 1.0]);
    assert!(hdr.iter().all(|v| v.is_finite()));
}

#[test]
fn gpu_color003_lut_matches_cpu_reference_in_both_spaces() {
    let lut = color003_lut();
    for working in [WorkingSpace::LinearRec709, WorkingSpace::LinearRec2020] {
        for intensity in [0.35, 1.0] {
            // Partial alpha, HDR and domain-external premultiplied channels
            // cover the straight-sample clamp on both backends.
            for input in [
                [0.25, 0.5, 1.0, 0.5],
                [4.0, -0.5, 2.0, 1.0],
                [0.05, 0.1, 0.02, 0.25],
            ] {
                let scene = color_effect_scene(
                    kronello_render::PixelEffect::ColorLut {
                        lut: lut.clone(),
                        intensity,
                    },
                    input,
                );
                let size = RenderSize::pixels(2, 2);
                let expected = render_scene_reference(size, &scene, working).unwrap();
                let actual = gpu().render_scene(size, &scene, working).unwrap();
                compare(2, working, &expected, &actual.pixels);
            }
        }
    }
}

/// TRACK-002 (ADR-0122): a 4x4 raster whose texel value encodes its position
/// (`[x/4, y/4, 0, 1]` — all f16-exact) lets nearest taps identify the
/// sampled source cell directly.
fn track002_scene(
    frame: [[f32; 3]; 2],
    border: kronello_model::StabilizeBorder,
    sampling: kronello_model::StabilizeSampling,
) -> DrawScene {
    let mut src = vec![[0.0; 4]; 16];
    for y in 0..4u32 {
        for x in 0..4u32 {
            src[(y * 4 + x) as usize] = [x as f32 / 4.0, y as f32 / 4.0, 0.0, 1.0];
        }
    }
    DrawScene {
        nodes: vec![
            DrawNode::Raster(src),
            DrawNode::Effect {
                source: 0,
                effect: PixelEffect::Stabilize {
                    frame,
                    unmap: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                    size: [4.0, 4.0],
                    border,
                    fill: kronello_model::Color::new(
                        kronello_model::ColorSpace::Srgb,
                        [1.0, 0.0, 0.0],
                        0.5,
                    )
                    .unwrap(),
                    sampling,
                },
            },
        ],
        roots: vec![1],
    }
}
#[test]
fn cpu_track002_stabilize_borders_and_sampling() {
    use kronello_model::{StabilizeBorder, StabilizeSampling};
    let half = |v: f32| half::f16::from_f32(v).to_f32();
    let size = RenderSize::pixels(4, 4);
    // Identity warp reproduces the source under both samplers.
    for sampling in [StabilizeSampling::Nearest, StabilizeSampling::Bilinear] {
        let scene = track002_scene(
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            StabilizeBorder::Fill,
            sampling,
        );
        let px = render_scene_reference(size, &scene, WorkingSpace::LinearRec709).unwrap();
        for y in 0..4usize {
            for x in 0..4usize {
                assert_eq!(
                    px[y * 4 + x],
                    [half(x as f32 / 4.0), half(y as f32 / 4.0), 0.0, 1.0],
                    "({x},{y}) {sampling:?}"
                );
            }
        }
    }
    // Shift by -2: positions s = x - 1.5 map to texel x - 2.
    let shifted = [[1.0, 0.0, -2.0], [0.0, 1.0, 0.0]];
    let fill = [half(0.5), 0.0, 0.0, half(0.5)];
    let px = render_scene_reference(
        size,
        &track002_scene(shifted, StabilizeBorder::Fill, StabilizeSampling::Nearest),
        WorkingSpace::LinearRec709,
    )
    .unwrap();
    for y in 0..4usize {
        assert_eq!(px[y * 4], fill);
        assert_eq!(px[y * 4 + 1], fill);
        for x in 2..4usize {
            assert_eq!(
                px[y * 4 + x],
                [half((x - 2) as f32 / 4.0), half(y as f32 / 4.0), 0.0, 1.0],
                "fill ({x},{y})"
            );
        }
    }
    // Replicate clamps s into the extent: edge columns read texel 0.
    let px = render_scene_reference(
        size,
        &track002_scene(
            shifted,
            StabilizeBorder::Replicate,
            StabilizeSampling::Nearest,
        ),
        WorkingSpace::LinearRec709,
    )
    .unwrap();
    for y in 0..4usize {
        for x in 0..2usize {
            assert_eq!(px[y * 4 + x], [0.0, half(y as f32 / 4.0), 0.0, 1.0]);
        }
    }
    // Reflect folds s = -1.5 onto +1.5 → texel 1, and s = -0.5 onto +0.5 →
    // texel 0.
    let px = render_scene_reference(
        size,
        &track002_scene(
            shifted,
            StabilizeBorder::Reflect,
            StabilizeSampling::Nearest,
        ),
        WorkingSpace::LinearRec709,
    )
    .unwrap();
    for y in 0..4usize {
        for x in 0..2usize {
            let expected_x = (1 - x) as f32 / 4.0;
            assert_eq!(
                px[y * 4 + x],
                [half(expected_x), half(y as f32 / 4.0), 0.0, 1.0],
                "reflect ({x},{y})"
            );
        }
    }
    // Bilinear at a half-texel shift blends the bracketing texels 50/50.
    let px = render_scene_reference(
        size,
        &track002_scene(
            [[1.0, 0.0, -1.5], [0.0, 1.0, 0.0]],
            StabilizeBorder::Fill,
            StabilizeSampling::Bilinear,
        ),
        WorkingSpace::LinearRec709,
    )
    .unwrap();
    for y in 0..4usize {
        assert_eq!(px[y * 4], fill);
        // s = 0 centers on texel 0: the -1 tap is outside and contributes
        // transparent black, halving the read value and alpha.
        assert_eq!(px[y * 4 + 1], [0.0, half(y as f32 / 8.0), 0.0, 0.5]);
        for x in 2..4usize {
            assert_eq!(
                px[y * 4 + x],
                [
                    half((2 * x - 3) as f32 / 8.0),
                    half(y as f32 / 4.0),
                    0.0,
                    1.0
                ],
                "bilinear ({x},{y})"
            );
        }
    }
}
#[test]
fn gpu_track002_stabilize_matches_cpu_reference() {
    use kronello_model::{StabilizeBorder, StabilizeSampling};
    for working in [WorkingSpace::LinearRec709, WorkingSpace::LinearRec2020] {
        for (frame, border, sampling) in [
            (
                [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                StabilizeBorder::Fill,
                StabilizeSampling::Bilinear,
            ),
            (
                [[1.0, 0.0, -2.0], [0.0, 1.0, 0.0]],
                StabilizeBorder::Replicate,
                StabilizeSampling::Nearest,
            ),
            (
                [[1.0, 0.0, -1.5], [0.0, 1.0, -0.5]],
                StabilizeBorder::Reflect,
                StabilizeSampling::Bilinear,
            ),
        ] {
            let scene = track002_scene(frame, border, sampling);
            let size = RenderSize::pixels(4, 4);
            let expected = render_scene_reference(size, &scene, working).unwrap();
            let actual = gpu().render_scene(size, &scene, working).unwrap();
            compare(4, working, &expected, &actual.pixels);
        }
    }
}
