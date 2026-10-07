use kronello_framebridge::{CONCRETE_PATHS, PathKind, SpikePath, TransferPath};
use kronello_gpu::{GpuContext, GpuError};
#[test]
fn gpu_residency_is_explicit() {
    for path in [
        PathKind::CpuUpload,
        PathKind::GpuReadback,
        PathKind::VideoToolbox,
    ] {
        assert!(matches!(
            path.require_gpu_resident(),
            Err(GpuError::UnsupportedFeature(_))
        ));
    }
    assert!(PathKind::GpuCopy.require_gpu_resident().is_ok());
    #[cfg(not(target_os = "macos"))]
    for path in [
        PathKind::IoSurfaceImport,
        PathKind::IoSurfaceOutput,
        PathKind::CvPixelBufferImport,
        PathKind::VideoToolboxDecodeBgra8,
        PathKind::VideoToolboxDecodeNv12Biplanar,
    ] {
        assert!(matches!(
            path.require_gpu_resident(),
            Err(GpuError::UnsupportedFeature(_))
        ));
    }
}
#[test]
fn measure_transfer_paths() {
    let gpu = GpuContext::new().expect("GPU adapter required for transfer measurements");
    eprintln!("FrameBridge adapter: {:?}", gpu.adapter_info);
    for kind in [
        PathKind::CpuUpload,
        PathKind::GpuCopy,
        PathKind::GpuReadback,
    ] {
        let path = SpikePath(kind);
        assert_eq!(path.kind(), kind);
        let measurement = path.measure(&gpu).unwrap();
        assert_eq!(measurement.path, kind);
        let s = &measurement.transfers;
        assert_eq!(s.cpu_upload_control_bytes, 0);
        assert_eq!(s.cpu_upload_control_operations, 0);
        assert_eq!(
            s.cpu_upload_pixel_bytes,
            if kind == PathKind::CpuUpload {
                32768
            } else {
                0
            }
        );
        assert_eq!(
            s.gpu_copy_bytes,
            if kind == PathKind::GpuCopy { 32768 } else { 0 }
        );
        assert_eq!(
            s.gpu_readback_bytes,
            if kind == PathKind::GpuReadback {
                32768
            } else {
                0
            }
        );
        eprintln!("{measurement:?}");
    }
    assert!(matches!(
        SpikePath(PathKind::VideoToolbox).measure(&gpu),
        Err(GpuError::UnsupportedFeature(_))
    ));
}
#[cfg(target_os = "macos")]
#[test]
fn iosurface_import_and_output() {
    let gpu = GpuContext::new().expect("Metal adapter required");
    // Both stages must work; never turn a failed import into a successful skip.
    for kind in [PathKind::IoSurfaceImport, PathKind::IoSurfaceOutput] {
        let measurement = SpikePath(kind).measure(&gpu).unwrap();
        assert_eq!(measurement.transfers.cpu_upload_pixel_bytes, 0);
        assert_eq!(measurement.transfers.gpu_copy_bytes, 0);
        assert_eq!(
            measurement.transfers.gpu_readback_bytes,
            if kind == PathKind::IoSurfaceImport {
                512
            } else {
                0
            }
        );
        eprintln!("{measurement:?}");
    }
}
#[cfg(not(target_os = "macos"))]
#[test]
fn native_paths_are_typed_unsupported() {
    let gpu = GpuContext::new().expect("Vulkan adapter required");
    for kind in [
        PathKind::IoSurfaceImport,
        PathKind::IoSurfaceOutput,
        PathKind::CvPixelBufferImport,
        PathKind::VideoToolboxDecodeBgra8,
        PathKind::VideoToolboxDecodeNv12Biplanar,
    ] {
        assert!(matches!(
            SpikePath(kind).measure(&gpu),
            Err(GpuError::UnsupportedFeature(_))
        ));
    }
}

#[test]
fn advertised_paths_have_explicit_transfer_or_decode_meaning() {
    assert_eq!(CONCRETE_PATHS.len(), 8);
    assert!(!CONCRETE_PATHS.contains(&PathKind::VideoToolbox));
    assert!(CONCRETE_PATHS.contains(&PathKind::VideoToolboxDecodeBgra8));
    assert!(CONCRETE_PATHS.contains(&PathKind::VideoToolboxDecodeNv12Biplanar));
    for (i, path) in CONCRETE_PATHS.iter().enumerate() {
        assert!(!CONCRETE_PATHS[..i].contains(path));
    }
    let Err(GpuError::UnsupportedFeature(message)) = PathKind::VideoToolbox.require_gpu_resident()
    else {
        panic!("generic selector must fail");
    };
    assert!(message.contains("VideoToolboxDecodeBgra8"));
    assert!(message.contains("VideoToolboxDecodeNv12Biplanar"));
    assert!(message.contains("no encode path"));
}
#[test]
fn generic_measurement_matches_policy_rejection() {
    let gpu = GpuContext::new().expect("GPU adapter required");
    let Err(GpuError::UnsupportedFeature(policy)) = PathKind::VideoToolbox.require_gpu_resident()
    else {
        panic!()
    };
    let Err(GpuError::UnsupportedFeature(execution)) =
        SpikePath(PathKind::VideoToolbox).measure(&gpu)
    else {
        panic!()
    };
    assert_eq!(policy, execution);
}
