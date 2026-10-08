//! FX-008 (ADR-0137): versioned remaining-standard effects. CPU reference
//! semantics, scene validation for the two new node kinds, and the
//! GPU/WGSL vs CPU oracle agreement on real hardware.
mod common;
use common::*;
use kronello_gpu::*;
use kronello_model as model;
use kronello_testkit::{FrameDescriptor, LinearFrame, PixelTolerance, compare_pixels};
use std::sync::OnceLock;
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
fn gpu() -> &'static GpuContext {
    static GPU: OnceLock<GpuContext> = OnceLock::new();
    GPU.get_or_init(|| {
        let gpu = GpuContext::new().expect("GPU required; no fallback or skip");
        eprintln!("FX-008 adapter: {:?}", gpu.adapter_info);
        gpu
    })
}
fn two_tones(a: [f32; 4], b: [f32; 4]) -> DrawScene {
    DrawScene {
        nodes: vec![
            rectangle([0.0; 2], [8.0, 16.0], paint(a, InputSpace::LinearRec709)),
            rectangle([8.0, 0.0], [16.0; 2], paint(b, InputSpace::LinearRec709)),
            DrawNode::Group {
                children: vec![0, 1],
                opacity: 1.0,
            },
        ],
        roots: vec![2],
    }
}
fn with_effect(effect: PixelEffect) -> DrawScene {
    let mut scene = two_tones([0.8, 0.2, 0.1, 1.0], [0.05, 0.4, 0.7, 1.0]);
    scene.nodes.push(DrawNode::Effect { source: 2, effect });
    scene.roots = vec![3];
    scene
}
fn reference(scene: &DrawScene) -> Vec<[f32; 4]> {
    scene.validate().unwrap();
    render_scene_reference(
        RenderSize::pixels(16, 16),
        scene,
        WorkingSpace::LinearRec709,
    )
    .unwrap()
}
fn straight(p: [f32; 4]) -> [f32; 4] {
    if p[3] <= 0.0 {
        [0.0; 4]
    } else {
        [p[0] / p[3], p[1] / p[3], p[2] / p[3], p[3]]
    }
}
/// Surfaces round to binary16 at every boundary; semantic comparisons
/// tolerate a few ulp of f16 quantization.
fn near(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() <= 2.0e-3)
}

#[test]
fn cpu_fx008_scenes_register_in_the_golden_harness_catalog() {
    let ids: Vec<&str> = scenes().iter().map(|(id, ..)| *id).collect();
    for id in [
        "fx008-grain",
        "fx008-mosaic",
        "fx008-channel-mixer",
        "fx008-invert",
        "fx008-tint",
        "fx008-directional-blur",
        "fx008-radial-blur",
        "fx008-displace",
        "fx008-generate",
    ] {
        assert!(ids.contains(&id), "{id} missing from the harness catalog");
    }
}
#[test]
fn cpu_fx008_scenes_validate_and_render_deterministically() {
    for (id, n, working, scene) in fx008_scenes() {
        scene.validate().unwrap_or_else(|e| panic!("{id}: {e:?}"));
        let a = render_scene_reference(RenderSize::pixels(n, n), &scene, working).unwrap();
        let b = render_scene_reference(RenderSize::pixels(n, n), &scene, working).unwrap();
        assert_eq!(a, b, "{id} must be deterministic");
    }
}
#[test]
fn cpu_fx008_effect_map_requires_displace_and_generate_requires_generate() {
    let mut scene = two_tones([0.8, 0.2, 0.1, 1.0], [0.05, 0.4, 0.7, 1.0]);
    scene.nodes.push(DrawNode::EffectMap {
        source: 2,
        map: 0,
        effect: PixelEffect::Invert {
            channel: model::InvertChannel::Rgb,
        },
    });
    scene.roots = vec![3];
    let error = scene.validate().unwrap_err();
    assert!(
        matches!(error, GpuError::InvalidInput(_)),
        "non-displace EffectMap must be a typed error, got {error:?}"
    );
    let mut scene = DrawScene {
        nodes: vec![DrawNode::Generate {
            effect: PixelEffect::Mosaic {
                block_size: 2.0,
                basis: model::MosaicBasis::Center,
            },
        }],
        roots: vec![0],
    };
    let error = scene.validate().unwrap_err();
    assert!(
        matches!(error, GpuError::InvalidInput(_)),
        "non-generate Generate node must be a typed error, got {error:?}"
    );
    scene.nodes[0] = DrawNode::Generate {
        effect: PixelEffect::Generate {
            generator: model::GenerateKind::Grid,
            color_a: model::Color::from_srgb8([0, 0, 0], None),
            color_b: model::Color::from_srgb8([255, 255, 255], None),
            point_a: [0.0, 0.0],
            point_b: [16.0, 16.0],
            cell_size: 4.0,
            line_width: 1.0,
        },
    };
    scene.validate().unwrap();
}
#[test]
fn cpu_fx008_grain_is_seeded_and_alpha_preserving() {
    let scene = with_effect(PixelEffect::Grain {
        amount: 0.4,
        size: 1.0,
        monochrome: true,
        seed: 42,
    });
    let pixels = reference(&scene);
    // Deterministic but not flat: the noise must actually vary per cell.
    let unique: std::collections::BTreeSet<u32> = pixels.iter().map(|p| p[0].to_bits()).collect();
    assert!(unique.len() > 8, "grain noise must vary across cells");
    for &p in &pixels {
        assert!(p.iter().all(|v| v.is_finite()));
        assert!(p[..3].iter().all(|&v| v.abs() <= p[3] + 0.2));
    }
    // A different seed must produce a different raster.
    let other = with_effect(PixelEffect::Grain {
        amount: 0.4,
        size: 1.0,
        monochrome: true,
        seed: 43,
    });
    assert_ne!(pixels, reference(&other), "seeds must change the noise");
}
#[test]
fn cpu_fx008_mosaic_quantizes_to_block_centers() {
    let scene = with_effect(PixelEffect::Mosaic {
        block_size: 4.0,
        basis: model::MosaicBasis::Center,
    });
    let pixels = reference(&scene);
    // Each 4x4 block is a single sampled color.
    for by in 0..4 {
        for bx in 0..4 {
            let color = pixels[by * 4 * 16 + bx * 4 + 1];
            for y in by * 4..by * 4 + 4 {
                for x in bx * 4..bx * 4 + 4 {
                    assert_eq!(pixels[y * 16 + x], color, "block {bx},{by}");
                }
            }
        }
    }
    // The left half sampled from block centers is the left color.
    assert_eq!(pixels[16 + 1], pixels[16 + 2]);
}
#[test]
fn cpu_fx008_invert_and_mixer_are_exact_straight_channel_ops() {
    let scene = with_effect(PixelEffect::Invert {
        channel: model::InvertChannel::Rgb,
    });
    let plain = two_tones([0.8, 0.2, 0.1, 1.0], [0.05, 0.4, 0.7, 1.0]);
    let inverted = reference(&scene);
    let source = reference(&plain);
    for (o, i) in inverted.iter().zip(&source) {
        let s = straight(*i);
        let os = straight(*o);
        assert!((os[0] - (1.0 - s[0])).abs() < 2.0e-3);
        assert!((os[1] - (1.0 - s[1])).abs() < 2.0e-3);
        assert!((os[2] - (1.0 - s[2])).abs() < 2.0e-3);
        assert_eq!(o[3], i[3], "alpha preserved");
    }
    let swap = with_effect(PixelEffect::ChannelMixer {
        matrix: [
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    });
    let swapped = reference(&swap);
    for (o, i) in swapped.iter().zip(&source) {
        assert!(near(*o, [i[2], i[1], i[0], i[3]]), "premultiplied r/b swap");
    }
}
#[test]
fn cpu_fx008_blurs_average_taps_and_keep_finite() {
    let plain = two_tones([0.8, 0.2, 0.1, 1.0], [0.05, 0.4, 0.7, 1.0]);
    let source = reference(&plain);
    for effect in [
        PixelEffect::DirectionalBlur {
            direction: [1.0, 0.0],
            length: 4.0,
        },
        PixelEffect::RadialBlur {
            mode: model::RadialBlurMode::Zoom,
            amount: 0.5,
            center: [8.0, 8.0],
        },
    ] {
        let blurred = reference(&with_effect(effect));
        for (o, i) in blurred.iter().zip(&source) {
            assert!(o.iter().all(|v| v.is_finite()));
            // Averaging stays within the input's channel envelope here.
            let lo = i.iter().cloned().fold(f32::INFINITY, f32::min);
            let hi = i.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
            assert!(o.iter().all(|&v| v >= lo - 1.0 && v <= hi + 1.0));
        }
    }
}
#[test]
fn cpu_fx008_displace_warps_by_map_and_generate_is_source_free() {
    // A flat 0.5 map is the neutral (2v-1 = 0) displacement: identity warp.
    let mut scene = two_tones([0.8, 0.2, 0.1, 1.0], [0.05, 0.4, 0.7, 1.0]);
    scene.nodes.push(rectangle(
        [0.0; 2],
        [16.0; 2],
        paint([0.5, 0.5, 0.5, 1.0], InputSpace::LinearRec709),
    ));
    scene.nodes.push(DrawNode::EffectMap {
        source: 2,
        map: 3,
        effect: PixelEffect::Displace {
            channel_x: model::DisplaceChannel::Luminance,
            channel_y: model::DisplaceChannel::Luminance,
            displacement: [[2.0, 0.0], [0.0, 2.0]],
        },
    });
    scene.roots = vec![4];
    let neutral = reference(&scene);
    let plain = reference(&two_tones([0.8, 0.2, 0.1, 1.0], [0.05, 0.4, 0.7, 1.0]));
    assert!(
        neutral.iter().zip(&plain).all(|(a, b)| near(*a, *b)),
        "0.5 luminance map must be the identity"
    );
    // A 1.0 map pushes samples +2 px on each axis: the seam moves.
    let mut scene = two_tones([0.8, 0.2, 0.1, 1.0], [0.05, 0.4, 0.7, 1.0]);
    scene.nodes.push(rectangle(
        [0.0; 2],
        [16.0; 2],
        paint([1.0, 1.0, 1.0, 1.0], InputSpace::LinearRec709),
    ));
    scene.nodes.push(DrawNode::EffectMap {
        source: 2,
        map: 3,
        effect: PixelEffect::Displace {
            channel_x: model::DisplaceChannel::Luminance,
            channel_y: model::DisplaceChannel::Red,
            displacement: [[2.0, 0.0], [0.0, 0.0]],
        },
    });
    scene.roots = vec![4];
    let warped = reference(&scene);
    // Output pixel (6,8) samples source (8,8): the right-hand color.
    assert!(near(warped[8 * 16 + 6], plain[8 * 16 + 8]));
    // Generate ignores the source entirely.
    let scene = DrawScene {
        nodes: vec![DrawNode::Generate {
            effect: PixelEffect::Generate {
                generator: model::GenerateKind::Checkerboard,
                color_a: model::Color::from_srgb8([255, 0, 0], None),
                color_b: model::Color::from_srgb8([0, 0, 255], None),
                point_a: [0.0, 0.0],
                point_b: [16.0, 16.0],
                cell_size: 8.0,
                line_width: 1.0,
            },
        }],
        roots: vec![0],
    };
    let pixels = reference(&scene);
    assert_eq!(pixels[0], pixels[7], "same checker cell repeats");
    assert_ne!(pixels[0], pixels[8], "adjacent x cell differs");
    assert_ne!(pixels[0], pixels[8 * 16], "adjacent y cell differs");
}
#[test]
fn gpu_fx008_scenes_match_cpu_oracle_in_both_working_spaces() {
    for (id, n, _, scene) in fx008_scenes() {
        for working in [WorkingSpace::LinearRec709, WorkingSpace::LinearRec2020] {
            eprintln!("FX-008 {id}: {working:?}");
            let expected =
                render_scene_reference(RenderSize::pixels(n, n), &scene, working).unwrap();
            let actual = gpu()
                .render_scene(RenderSize::pixels(n, n), &scene, working)
                .unwrap();
            compare(n, working, &expected, &actual.pixels);
        }
    }
}
