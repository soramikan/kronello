//! IO-001 (ADR-0134): runtime detection probes for external outputs are
//! `dlopen` attempts only — they must never panic, never link anything, and
//! the Syphon publish boundary must reject typed when the framework or its
//! prerequisites are absent.
use kronello_framebridge::output;
#[cfg(target_os = "macos")]
use kronello_gpu::GpuContext;

#[test]
fn detection_probes_never_panic() {
    // Results are environment-dependent; the contract is that probing is a
    // pure side-effect-free report.
    let _ = output::syphon_detected();
    let _ = output::decklink_detected();
    let _ = output::ndi_detected();
    // Repeated probes agree with themselves (detection is cached per
    // process, which the contract documents).
    assert_eq!(output::syphon_detected(), output::syphon_detected());
}

#[test]
#[cfg(target_os = "macos")]
fn syphon_server_rejects_without_framework() {
    use objc2_metal::MTLCreateSystemDefaultDevice;
    let Some(device) = MTLCreateSystemDefaultDevice() else {
        eprintln!("skipping: no Metal device");
        return;
    };
    let result = output::SyphonServer::create("kronello-test", &device);
    if output::syphon_detected() {
        // Framework present: creation must succeed or fail only because the
        // environment cannot host a server (still a typed error).
        match result {
            Ok(server) => drop(server),
            Err(e) => panic!("framework detected but create failed typed: {e}"),
        }
    } else {
        assert!(matches!(
            result,
            Err(kronello_gpu::GpuError::UnsupportedFeature(_))
        ));
    }
}

#[test]
#[cfg(target_os = "macos")]
fn publish_texture_pairs_native_and_hal_handles() {
    let Ok(gpu) = GpuContext::new() else {
        eprintln!("skipping: no GPU context");
        return;
    };
    let Ok((native, texture)) = output::publish_texture(&gpu, 64, 32) else {
        panic!("publish texture creation failed");
    };
    use objc2_metal::MTLTexture;
    assert_eq!(native.width(), 64);
    assert_eq!(native.height(), 32);
    assert_eq!(
        native.pixelFormat(),
        objc2_metal::MTLPixelFormat::BGRA8Unorm
    );
    assert_eq!(texture.format(), wgpu::TextureFormat::Bgra8Unorm);
    // Zero and oversized extents are typed rejects, not clamped guesses.
    for (w, h) in [(0, 32), (64, 0), (16385, 32), (64, 16385)] {
        assert!(output::publish_texture(&gpu, w, h).is_err(), "{w}x{h}");
    }
}
