#![cfg(target_os = "macos")]
use kronello_framebridge::resident::{ResidentFormat, VideoTransfer, generated_hardware_frame};
use kronello_gpu::{DrawNode, DrawScene, GpuContext, RenderSize, TransferStats, WorkingSpace};

fn verify(format: ResidentFormat) {
    let gpu = GpuContext::new().expect("Metal device required");
    let frame = generated_hardware_frame(format).expect("strict hardware decoder required");
    let mut conversion = TransferStats::default();
    let image = frame
        .sample_to_working(
            &gpu,
            [64, 64],
            [64.0, 64.0],
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            WorkingSpace::LinearRec709,
            VideoTransfer::Bt709,
            &mut conversion,
        )
        .unwrap();
    // Session is already invalidated. Release the caller's native buffer before
    // any GPU completion wait; HAL/queue ownership must keep imported input live.
    drop(frame);
    let scene = DrawScene {
        nodes: vec![
            DrawNode::GpuRaster(image.clone()),
            DrawNode::Group {
                children: vec![0],
                opacity: 0.5,
            },
        ],
        roots: vec![1],
    };
    let output = gpu
        .render_scene(
            RenderSize::pixels(64, 64),
            &scene,
            WorkingSpace::LinearRec709,
        )
        .unwrap();
    assert_eq!(conversion.cpu_upload_pixel_bytes, 0);
    assert_eq!(conversion.gpu_readback_bytes, 0);
    assert_eq!(conversion.gpu_copy_bytes, 0);
    assert_eq!(conversion.cpu_upload_control_bytes, 48);
    assert_eq!(output.transfers.cpu_upload_pixel_bytes, 0);
    assert!(output.transfers.gpu_readback_bytes > 0);
    for p in &output.pixels {
        assert!((p[3] - 0.5).abs() < 0.001);
        assert!(p.iter().all(|v| v.is_finite()));
    }
    for (x, y, gray) in [
        (16usize, 16usize, 32f32),
        (48, 16, 96.0),
        (16, 48, 160.0),
        (48, 48, 224.0),
    ] {
        let v = gray / 255.0;
        let expected = if v < 0.081 {
            v / 4.5
        } else {
            ((v + 0.099) / 1.099).powf(1.0 / 0.45)
        } * 0.5;
        for actual in &output.pixels[y * 64 + x][..3] {
            assert!(
                (*actual - expected).abs() < 0.012,
                "{format:?} quadrant {gray}: actual={actual} expected={expected}"
            );
        }
    }
    let other = GpuContext::new().unwrap();
    assert!(
        image
            .validate_for(&other, [64, 64], WorkingSpace::LinearRec709)
            .is_err()
    );
    assert!(
        image
            .validate_for(&gpu, [32, 64], WorkingSpace::LinearRec709)
            .is_err()
    );
    assert!(
        image
            .validate_for(&gpu, [64, 64], WorkingSpace::LinearRec2020)
            .is_err()
    );
    assert!(
        kronello_gpu::render_scene_reference(
            RenderSize::pixels(64, 64),
            &scene,
            WorkingSpace::LinearRec709
        )
        .is_err()
    );
    eprintln!(
        "GPU003 format={format:?}, conversion={conversion:?}, final={:?}, device={:?}",
        output.transfers, gpu.adapter_info
    );
}
#[test]
#[ignore = "requires actual VideoToolbox hardware decoder and Metal"]
fn hardware_bgra_decode_color_composite_retained_lifetime() {
    verify(ResidentFormat::Bgra8);
}
#[test]
#[ignore = "requires actual VideoToolbox hardware decoder and Metal"]
fn hardware_nv12_decode_color_composite_retained_lifetime() {
    verify(ResidentFormat::Nv12VideoRange);
}
