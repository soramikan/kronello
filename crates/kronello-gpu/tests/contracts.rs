use kronello_gpu::{color::*, *};
use kronello_testkit::{FrameDescriptor, LinearFrame, PixelTolerance, compare_pixels};
use std::sync::OnceLock;
fn gpu() -> &'static GpuContext {
    static GPU: OnceLock<GpuContext> = OnceLock::new();
    GPU.get_or_init(|| {
        let gpu = GpuContext::new().expect("GPU adapter required; no skip or fallback");
        eprintln!("GPU-001 adapter: {:?}", gpu.adapter_info);
        gpu
    })
}
fn compare(w: u32, h: u32, space: WorkingSpace, expected: &[[f32; 4]], actual: &[[f32; 4]]) {
    let d = FrameDescriptor {
        width: w,
        height: h,
        origin: [0; 2],
        time: [0, 1],
        working_space: if space == WorkingSpace::LinearRec709 {
            kronello_testkit::WorkingSpace::LinearRec709
        } else {
            kronello_testkit::WorkingSpace::LinearRec2020
        },
        color_pipeline_id: "gpu001-linear-v1".into(),
        samples_per_frame: 1,
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
fn cpu_srgb_transfer_function_breakpoints() {
    // Independently evaluated piecewise IEC sRGB equations, not calls to the oracle.
    for (encoded, linear) in [
        (0.0, 0.0),
        (0.04044, 0.003130031),
        (0.04045, 0.003130805),
        (0.04046, 0.0031315945),
        (0.5, 0.21404114),
        (1.0, 1.0),
    ] {
        assert!((srgb_decode(encoded) - linear).abs() < 1e-7);
    }
    for (linear, encoded) in [
        (0.0031208, 0.040320735),
        (0.0031308, 0.040449936),
        (0.0031408, 0.04057682),
    ] {
        assert!((srgb_encode(linear) - encoded).abs() < 2e-7);
    }
    for value in [0.0, 0.003, 0.04045, 0.25, 0.5, 1.0] {
        assert!((srgb_encode(srgb_decode(value)) - value).abs() < 1e-6);
    }
}
#[test]
fn cpu_d65_primaries_white_and_round_trip() {
    let rgb = convert_primaries(
        [1.0, 0.0, 0.0],
        WorkingSpace::LinearRec709,
        WorkingSpace::LinearRec2020,
    );
    // For the primary vector, each dot product contains one matrix entry times
    // exactly 1 and two zeros, so this f32 equality is intentionally exact.
    assert_eq!(rgb, [0.627404, 0.0690973, 0.0163914]);
    for (from, to) in [
        (WorkingSpace::LinearRec709, WorkingSpace::LinearRec2020),
        (WorkingSpace::LinearRec2020, WorkingSpace::LinearRec709),
    ] {
        for value in convert_primaries([1.0; 3], from, to) {
            assert!((value - 1.0).abs() < 1e-6);
        }
        for rgb in [[1.0; 3], [0.1, 0.5, 0.9], [-0.125, 2.0, 4.0]] {
            let back = convert_primaries(convert_primaries(rgb, from, to), to, from);
            for i in 0..3 {
                assert!((back[i] - rgb[i]).abs() < 1e-5);
            }
        }
    }
}
#[test]
fn cpu_alpha_and_external_boundary() {
    assert_eq!(premultiply([1.0, 2.0, -1.0, 0.0]), [0.0; 4]);
    assert_eq!(
        source_over([0.5, 0.0, 0.0, 0.5], [0.0, 0.0, 1.0, 1.0]),
        [0.5, 0.0, 0.5, 1.0]
    );
    for a in [0.0, ALPHA_EPSILON / 2.0, ALPHA_EPSILON, ALPHA_EPSILON * 2.0] {
        let p = premultiply([4.0, 2.0, -0.125, a]);
        assert_eq!(p[0], 4.0 * a);
        let external = unpremultiply_external(p);
        assert_eq!(external[3], a);
        assert_eq!(external[0], if a > ALPHA_EPSILON { 4.0 } else { 0.0 });
    }
}
#[test]
fn unavailable_adapter_is_typed_error() {
    assert!(matches!(
        pollster::block_on(GpuContext::with_backends(wgpu::Backends::empty())),
        Err(GpuError::AdapterUnavailable(_))
    ));
}
#[test]
fn invalid_input_and_pam_formats_fail() {
    assert!(
        Image::solid([f32::NAN, 0.0, 0.0, 1.0], InputSpace::LinearRec709)
            .validate()
            .is_err()
    );
    assert!(
        Image::solid([2.0, 0.0, 0.0, 1.0], InputSpace::Srgb)
            .validate()
            .is_err()
    );
    assert!(
        Image::solid([0.0, 0.0, 0.0, 1.01], InputSpace::LinearRec709)
            .validate()
            .is_err()
    );
    assert!(
        Image::from_pam(b"P7\nWIDTH 1\nHEIGHT 1\nDEPTH 3\nMAXVAL 255\nTUPLTYPE RGB\nENDHDR\nabc")
            .is_err()
    );
    assert!(render_reference(RenderSize::pixels(0, 1), &[], WorkingSpace::LinearRec709).is_err());
    assert!(
        RenderSize {
            design_extent: [f32::NAN, 1.0],
            output_resolution: [1, 1]
        }
        .validate()
        .is_err()
    );
}
#[test]
fn gpu_srgb_orange_independent_values_and_same_color_in_both_spaces() {
    // #F59E0B: normalize 245,158,11 by 255, evaluate sRGB piecewise equations
    // in f64 independently, then multiply the published D65 709->2020 matrix.
    const LINEAR_709: [f32; 4] = [0.91309865, 0.34191442, 0.0033465358, 1.0];
    const LINEAR_2020: [f32; 4] = [0.685_613, 0.3775348, 0.048057124, 1.0];
    let layers = [Layer::rectangle(
        [1.0, 1.0],
        [245.0 / 255.0, 158.0 / 255.0, 11.0 / 255.0, 1.0],
        InputSpace::Srgb,
    )];
    let a = gpu()
        .render(
            RenderSize::pixels(1, 1),
            &layers,
            WorkingSpace::LinearRec709,
        )
        .unwrap();
    let b = gpu()
        .render(
            RenderSize::pixels(1, 1),
            &layers,
            WorkingSpace::LinearRec2020,
        )
        .unwrap();
    compare(1, 1, WorkingSpace::LinearRec709, &[LINEAR_709], &a.pixels);
    compare(1, 1, WorkingSpace::LinearRec2020, &[LINEAR_2020], &b.pixels);
    assert!((a.pixels[0][0] - b.pixels[0][0]).abs() > 0.2);
    let p = b.pixels[0];
    let rgb = convert_primaries(
        [p[0], p[1], p[2]],
        WorkingSpace::LinearRec2020,
        WorkingSpace::LinearRec709,
    );
    compare(
        1,
        1,
        WorkingSpace::LinearRec709,
        &a.pixels,
        &[[rgb[0], rgb[1], rgb[2], p[3]]],
    );
}
#[test]
fn gpu_source_over_independent_premultiplied_values() {
    let red = Layer::rectangle([1.0, 1.0], [1.0, 0.0, 0.0, 0.5], InputSpace::LinearRec709);
    for (layers, expected) in [
        (vec![red.clone()], [0.5, 0.0, 0.0, 0.5]),
        (
            vec![
                Layer::rectangle([1.0, 1.0], [0.0, 0.0, 1.0, 1.0], InputSpace::LinearRec709),
                red,
            ],
            [0.5, 0.0, 0.5, 1.0],
        ),
        (
            vec![Layer::rectangle(
                [1.0, 1.0],
                [1.0, 0.8, 0.7, 0.0],
                InputSpace::Srgb,
            )],
            [0.0; 4],
        ),
    ] {
        let result = gpu()
            .render(
                RenderSize::pixels(1, 1),
                &layers,
                WorkingSpace::LinearRec709,
            )
            .unwrap();
        compare(
            1,
            1,
            WorkingSpace::LinearRec709,
            &[expected],
            &result.pixels,
        );
    }
    assert_eq!(
        gpu()
            .render(RenderSize::pixels(1, 1), &[], WorkingSpace::LinearRec709)
            .unwrap()
            .pixels,
        vec![[0.0; 4]]
    );
}
#[test]
fn gpu_pam_each_pixel_independent() {
    let bytes = std::fs::read(kronello_testkit::resolve_fixture("alpha").unwrap()).unwrap();
    let image = Image::from_pam(&bytes).unwrap();
    assert_eq!([image.width, image.height], [4, 1]);
    // Fixture: transparent red, red a=128/255, green opaque, blue a=1/255.
    // sRGB primary values decode exactly 0 or 1. Alpha is not gamma decoded.
    const EXPECTED: [[f32; 4]; 4] = [
        [0.0; 4],
        [0.5019608, 0.0, 0.0, 0.5019608],
        [0.0, 1.0, 0.0, 1.0],
        [0.0, 0.0, 0.003921569, 0.003921569],
    ];
    let layer = Layer {
        size: [4.0, 1.0],
        image,
        translation: [0.0; 2],
        rotation_degrees: 0.0,
    };
    let a = gpu()
        .render(
            RenderSize::pixels(4, 1),
            &[layer],
            WorkingSpace::LinearRec709,
        )
        .unwrap();
    compare(4, 1, WorkingSpace::LinearRec709, &EXPECTED, &a.pixels);
}
#[test]
fn gpu_positive_90_degrees_maps_x_to_y() {
    let mut rotated = Layer::rectangle([2.0, 3.0], [0.5, 0.25, 0.8, 0.5], InputSpace::Srgb);
    rotated.translation = [5.0, 1.0];
    rotated.rotation_degrees = 90.0;
    kronello_testkit::compare_finite_values(
        "rotated +X endpoint",
        &[5.0, 3.0],
        &rotated.transform_point([2.0, 0.0]).unwrap().map(f64::from),
    )
    .unwrap();
    let a = gpu()
        .render(
            RenderSize::pixels(8, 8),
            &[rotated.clone()],
            WorkingSpace::LinearRec709,
        )
        .unwrap();
    for y in 0..8 {
        for x in 0..8 {
            assert_eq!(
                a.pixels[y * 8 + x][3],
                if (2..5).contains(&x) && (1..3).contains(&y) {
                    0.5
                } else {
                    0.0
                }
            );
        }
    }
    compare(
        8,
        8,
        WorkingSpace::LinearRec709,
        &render_reference(
            RenderSize::pixels(8, 8),
            &[rotated],
            WorkingSpace::LinearRec709,
        )
        .unwrap(),
        &a.pixels,
    );
}
#[test]
fn gpu_design_extent_independent_of_output_resolution() {
    let mut layer = Layer::rectangle([2.0, 3.0], [1.0, 0.0, 0.0, 0.5], InputSpace::LinearRec709);
    layer.translation = [1.0, 2.0];
    let mut coverage = Vec::new();
    for scale in [1, 2] {
        let size = RenderSize {
            design_extent: [8.0, 8.0],
            output_resolution: [8 * scale, 8 * scale],
        };
        let a = gpu()
            .render(size, &[layer.clone()], WorkingSpace::LinearRec709)
            .unwrap();
        assert_eq!(a.design_extent, [8.0, 8.0]);
        let mut count = 0;
        for y in 0..8 * scale {
            for x in 0..8 * scale {
                let p = a.pixels[(y * 8 * scale + x) as usize];
                let inside = (scale..3 * scale).contains(&x) && (2 * scale..5 * scale).contains(&y);
                assert_eq!(
                    p,
                    if inside {
                        [0.5, 0.0, 0.0, 0.5]
                    } else {
                        [0.0; 4]
                    }
                );
                if inside {
                    count += 1;
                }
            }
        }
        coverage.push(count);
        compare(
            8 * scale,
            8 * scale,
            WorkingSpace::LinearRec709,
            &render_reference(size, &[layer.clone()], WorkingSpace::LinearRec709).unwrap(),
            &a.pixels,
        );
    }
    assert_eq!(coverage, vec![6, 24]); // width and height double; area quadruples.
}
#[test]
fn gpu_isolated_group_opacity_is_not_distributed_to_children() {
    let children = [
        Layer::rectangle([1.0, 1.0], [1.0, 0.0, 0.0, 0.5], InputSpace::LinearRec709),
        Layer::rectangle([1.0, 1.0], [0.0, 0.0, 1.0, 0.5], InputSpace::LinearRec709),
    ];
    let size = RenderSize::pixels(1, 1);
    let result = gpu()
        .render_isolated_group(size, &children, 0.5, WorkingSpace::LinearRec709)
        .unwrap();
    // Children: [0.25,0,0.5,0.75]; group opacity 0.5: [0.125,0,0.25,0.375].
    compare(
        1,
        1,
        WorkingSpace::LinearRec709,
        &[[0.125, 0.0, 0.25, 0.375]],
        &result.pixels,
    );
    let mut distributed = children.clone();
    for l in &mut distributed {
        l.image.pixels[0][3] *= 0.5;
    }
    let wrong = gpu()
        .render(size, &distributed, WorkingSpace::LinearRec709)
        .unwrap();
    compare(
        1,
        1,
        WorkingSpace::LinearRec709,
        &[[0.1875, 0.0, 0.25, 0.4375]],
        &wrong.pixels,
    );
    assert_ne!(result.pixels, wrong.pixels);
    assert_eq!(result.transfers.cpu_upload_pixel_bytes, 32);
    assert_eq!(result.transfers.cpu_upload_pixel_operations, 2);
    assert_eq!(result.transfers.cpu_upload_control_bytes, 112);
    assert_eq!(result.transfers.gpu_readback_operations, 1);
    for opacity in [-0.1, 1.1, f32::NAN] {
        assert!(matches!(
            gpu().render_isolated_group(size, &children, opacity, WorkingSpace::LinearRec709),
            Err(GpuError::InvalidInput(_))
        ));
    }
}
#[test]
fn gpu_oracle_scene_and_transfer_stats() {
    let image = Image::from_pam(
        &std::fs::read(kronello_testkit::resolve_fixture("alpha").unwrap()).unwrap(),
    )
    .unwrap();
    let mut rotated = Layer::rectangle([2.0, 3.0], [0.5, 0.25, 0.8, 0.5], InputSpace::Srgb);
    rotated.translation = [5.0, 1.0];
    rotated.rotation_degrees = 90.0;
    let texture = Layer {
        size: [image.width as f32, image.height as f32],
        image,
        translation: [1.0, 4.0],
        rotation_degrees: 0.0,
    };
    for space in [WorkingSpace::LinearRec709, WorkingSpace::LinearRec2020] {
        let layers = vec![
            Layer::rectangle([8.0, 8.0], [0.0, 0.0, 1.0, 1.0], InputSpace::LinearRec709),
            rotated.clone(),
            texture.clone(),
        ];
        let size = RenderSize::pixels(8, 8);
        let actual = gpu().render(size, &layers, space).unwrap();
        compare(
            8,
            8,
            space,
            &render_reference(size, &layers, space).unwrap(),
            &actual.pixels,
        );
        let s = &actual.transfers;
        assert_eq!(s.cpu_upload_pixel_operations, 3);
        assert_eq!(
            s.cpu_upload_pixel_bytes,
            16 + 16 + (texture.image.pixels.len() * 16) as u64
        );
        assert_eq!(s.cpu_upload_control_bytes, 144);
        assert_eq!(s.cpu_upload_control_operations, 3);
        assert_eq!(s.gpu_copy_bytes, 8 * 8 * 8);
        assert_eq!(s.gpu_copy_operations, 1);
        assert_eq!(s.gpu_readback_bytes, 256 * 8);
        assert_eq!(s.gpu_readback_operations, 1);
    }
}
#[test]
fn gpu_hdr_and_tiny_alpha_are_not_clamped() {
    let colors = [
        [4.0, 2.0, -0.125, 1.0],
        [1.0, 0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0, ALPHA_EPSILON],
        [1.0, 0.0, 0.0, ALPHA_EPSILON * 2.0],
    ];
    let layers = colors
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            let mut l = Layer::rectangle([1.0, 1.0], c, InputSpace::LinearRec2020);
            l.translation = [i as f32, 0.0];
            l
        })
        .collect::<Vec<_>>();
    for space in [WorkingSpace::LinearRec709, WorkingSpace::LinearRec2020] {
        let actual = gpu()
            .render(RenderSize::pixels(4, 1), &layers, space)
            .unwrap();
        compare(
            4,
            1,
            space,
            &render_reference(RenderSize::pixels(4, 1), &layers, space).unwrap(),
            &actual.pixels,
        );
        assert_eq!(actual.pixels[1], [0.0; 4]);
        assert_eq!(actual.pixels[2][3], ALPHA_EPSILON);
        assert!(actual.pixels[0][0] > 1.0);
        assert!(actual.pixels[0][2] < 0.0);
    }
}
