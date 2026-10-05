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
        Err(GpuError::UnsupportedFeature(_))
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
        Err(GpuError::UnsupportedFeature(_))
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
