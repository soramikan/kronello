#![cfg(target_os = "macos")]

use kronello_framebridge::{
    PathKind,
    videotoolbox::{probe_cvpixelbuffer_import, probe_videotoolbox_decode},
};
use kronello_gpu::GpuContext;

#[test]
fn test_cvpixelbuffer_import_to_gpu() {
    let gpu = GpuContext::new().expect("Metal adapter required for CVPixelBuffer import");
    eprintln!("CoreVideo adapter: {:?}", gpu.adapter_info);
    let report = probe_cvpixelbuffer_import(&gpu).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(report.path, PathKind::CvPixelBufferImport);
    assert_eq!(report.transfers.cpu_upload_pixel_bytes, 0);
    assert_eq!(report.transfers.cpu_upload_control_bytes, 0);
    assert_eq!(report.transfers.gpu_copy_bytes, 0);
    assert_eq!(report.transfers.gpu_readback_operations, 1);
    assert_eq!(report.transfers.gpu_readback_bytes, 512);
    eprintln!("{report:?}");
}

#[test]
#[ignore = "requires local VideoToolbox codec availability; explicitly measure on Metal"]
fn test_videotoolbox_decode_import_to_gpu() {
    let gpu = GpuContext::new().expect("Metal adapter required for VideoToolbox import");
    eprintln!("VideoToolbox adapter: {:?}", gpu.adapter_info);
    let report = probe_videotoolbox_decode(&gpu, false).unwrap_or_else(|error| panic!("{error}"));
    assert!(matches!(
        report.path,
        PathKind::VideoToolboxDecodeBgra8 | PathKind::VideoToolboxDecodeNv12Biplanar
    ));
    assert_eq!(report.transfers.cpu_upload_pixel_bytes, 0);
    assert_eq!(report.transfers.cpu_upload_control_bytes, 0);
    assert_eq!(report.transfers.gpu_copy_bytes, 0);
    assert_eq!(report.transfers.gpu_readback_operations, 3);
    assert_eq!(report.transfers.gpu_readback_bytes, 3 * 64 * 64 * 4);
    eprintln!("{report:?}");
}

#[test]
#[ignore = "optional explicit NV12 R8/RG8 plane import measurement on Metal"]
fn test_videotoolbox_nv12_plane_import_to_gpu() {
    let gpu = GpuContext::new().expect("Metal adapter required for NV12 import");
    let report = probe_videotoolbox_decode(&gpu, true).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(report.path, PathKind::VideoToolboxDecodeNv12Biplanar);
    assert_eq!(report.transfers.gpu_readback_operations, 3);
    eprintln!("{report:?}");
}
