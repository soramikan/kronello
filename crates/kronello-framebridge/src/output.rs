//! IO-001 (ADR-0134): external monitor output detection and the Syphon Metal
//! publish boundary. Proprietary SDKs (Blackmagic DeckLink, NDI) are never
//! bundled or linked; detection is a runtime `dlopen` probe so callers can
//! answer with typed `UNSUPPORTED_FEATURE` instead of a silent no-op.

use std::ffi::c_void;

#[cfg(target_os = "macos")]
mod native {
    use std::ffi::{c_char, c_void};
    unsafe extern "C" {
        pub fn kr_output_syphon_detected() -> i32;
        pub fn kr_output_decklink_detected() -> i32;
        pub fn kr_output_ndi_detected() -> i32;
        pub fn kr_syphon_server_create(name: *const c_char, device: *mut c_void) -> *mut c_void;
        pub fn kr_syphon_has_clients(server: *mut c_void) -> i32;
        pub fn kr_syphon_publish(
            server: *mut c_void,
            texture: *mut c_void,
            command_queue: *mut c_void,
            width: u32,
            height: u32,
        ) -> i32;
        pub fn kr_syphon_server_stop(server: *mut c_void);
        pub fn kr_syphon_server_release(server: *mut c_void);
    }
}

/// Whether the Syphon framework (BSD-2-Clause) is present at runtime. Probed
/// once per process; a later manual install is not re-detected.
pub fn syphon_detected() -> bool {
    #[cfg(target_os = "macos")]
    // SAFETY: detection probe without side effects beyond dlopen.
    unsafe {
        native::kr_output_syphon_detected() != 0
    }
    #[cfg(not(target_os = "macos"))]
    false
}
/// Whether the Blackmagic DeckLink SDK is present. Detection does not imply
/// any working adapter; enabling an SDI output is still a typed reject.
pub fn decklink_detected() -> bool {
    #[cfg(target_os = "macos")]
    // SAFETY: detection probe without side effects beyond dlopen.
    unsafe {
        native::kr_output_decklink_detected() != 0
    }
    #[cfg(not(target_os = "macos"))]
    false
}
/// Whether an NDI runtime library is present. Same boundary rule as SDI.
pub fn ndi_detected() -> bool {
    #[cfg(target_os = "macos")]
    // SAFETY: detection probe without side effects beyond dlopen.
    unsafe {
        native::kr_output_ndi_detected() != 0
    }
    #[cfg(not(target_os = "macos"))]
    false
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use kronello_gpu::{GpuContext, GpuError};
    use objc2::{rc::Retained, runtime::ProtocolObject};
    use objc2_metal::{
        MTLCommandQueue, MTLDevice, MTLPixelFormat, MTLStorageMode, MTLTexture,
        MTLTextureDescriptor, MTLTextureType, MTLTextureUsage,
    };
    use std::ffi::CString;

    /// Lifetime token only: retain/release are thread-safe and the token
    /// carries no texture access. Same contract as `macos.rs::SurfaceLifetime`.
    struct NativeTextureLifetime(Retained<ProtocolObject<dyn MTLTexture>>);
    // SAFETY: ObjC retain/release are thread-safe; the token only frees the
    // texture after the HAL object is done with it.
    unsafe impl Send for NativeTextureLifetime {}
    unsafe impl Sync for NativeTextureLifetime {}
    impl NativeTextureLifetime {
        fn release(self) {
            drop(self.0);
        }
    }

    /// A same-device native BGRA8 texture paired with its wgpu HAL import: the
    /// external output renders the shared presentation transform into the wgpu
    /// handle and hands the identical native object to Syphon. Mirrors the
    /// IOSurface import contract in `macos.rs`.
    pub fn publish_texture(
        gpu: &GpuContext,
        width: u32,
        height: u32,
    ) -> Result<(Retained<ProtocolObject<dyn MTLTexture>>, wgpu::Texture), GpuError> {
        if width == 0 || height == 0 || width > 16384 || height > 16384 {
            return Err(GpuError::InvalidInput(
                "publish texture size outside limits",
            ));
        }
        // SAFETY: Borrow the wgpu Metal device; no external GPU submission.
        let hal = unsafe { gpu.device.as_hal::<wgpu::hal::api::Metal>() }
            .ok_or(GpuError::UnsupportedFeature("wgpu device is not Metal"))?;
        let descriptor = MTLTextureDescriptor::new();
        descriptor.setTextureType(MTLTextureType::Type2D);
        descriptor.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
        // SAFETY: Positive 2D extents validated above.
        unsafe {
            descriptor.setWidth(width as usize);
            descriptor.setHeight(height as usize);
        }
        descriptor.setStorageMode(MTLStorageMode::Private);
        descriptor.setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
        let native = hal
            .raw_device()
            .newTextureWithDescriptor(&descriptor)
            .ok_or(GpuError::UnsupportedFeature(
                "MTLTexture for output publish returned null",
            ))?;
        drop(hal);
        let extent = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        // SAFETY: Native texture was created on this device, fully initialized
        // storage-private, BGRA8, 2D, one mip/layer/sample. The drop callback
        // keeps the retained native handle alive for HAL release.
        let hal_texture = unsafe {
            wgpu::hal::metal::Device::texture_from_raw(
                native.clone(),
                wgpu::TextureFormat::Bgra8Unorm,
                MTLTextureType::Type2D,
                1,
                1,
                extent.into(),
                Some(Box::new({
                    let retained = NativeTextureLifetime(native.clone());
                    move || retained.release()
                })),
            )
        };
        let texture_descriptor = wgpu::TextureDescriptor {
            label: Some("external output publish"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        };
        // SAFETY: Descriptor exactly matches the native texture; RESOURCE is
        // the render-target/sampled state Metal tracks itself.
        let texture = unsafe {
            gpu.device.create_texture_from_hal::<wgpu::hal::api::Metal>(
                hal_texture,
                &texture_descriptor,
                wgpu::TextureUses::RESOURCE,
            )
        };
        Ok((native, texture))
    }

    /// A Syphon Metal server created on an explicit `MTLDevice`. The retained
    /// command queue is created once and serializes publish commits, which is
    /// the output's frame pacing.
    pub struct SyphonServer {
        raw: *mut c_void,
        queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    }
    // SAFETY: server and queue are confined to the output worker that owns the
    // SyphonServer; Syphon documents server objects as thread-safe for publish.
    unsafe impl Send for SyphonServer {}

    impl SyphonServer {
        /// Create a server, or a typed reject when the framework/server setup
        /// is unavailable. `device` must be the same MTLDevice that produces
        /// the published textures.
        pub fn create(
            name: &str,
            device: &ProtocolObject<dyn MTLDevice>,
        ) -> Result<Self, GpuError> {
            if !super::syphon_detected() {
                return Err(GpuError::UnsupportedFeature(
                    "Syphon framework not detected (install Syphon.framework)",
                ));
            }
            let queue = device
                .newCommandQueue()
                .ok_or(GpuError::UnsupportedFeature(
                    "MTLCommandQueue creation failed",
                ))?;
            let cname = CString::new(name)
                .map_err(|_| GpuError::InvalidInput("Syphon server name contains NUL"))?;
            // SAFETY: device is a live MTLDevice for the call duration. The
            // returned server pointer is retained by the C side and released
            // exactly once in Drop below.
            let raw = unsafe {
                native::kr_syphon_server_create(
                    cname.as_ptr(),
                    std::ptr::from_ref(device).cast::<c_void>().cast_mut(),
                )
            };
            if raw.is_null() {
                return Err(GpuError::UnsupportedFeature(
                    "SyphonMetalServer initialization failed",
                ));
            }
            Ok(Self { raw, queue })
        }
        /// Whether clients are currently attached to this server.
        pub fn has_clients(&self) -> bool {
            // SAFETY: server pointer is live for self's lifetime.
            unsafe { native::kr_syphon_has_clients(self.raw) != 0 }
        }
        /// Publish one BGRA8 texture frame. `texture` must live on the same
        /// device as the server. The C side commits the publish command buffer
        /// and waits for completion, pacing this call.
        pub fn publish(
            &self,
            texture: &ProtocolObject<dyn MTLTexture>,
            width: u32,
            height: u32,
        ) -> Result<(), GpuError> {
            // SAFETY: texture/queue/server pointers are live for the call;
            // the publish commit finishes before return.
            let ok = unsafe {
                native::kr_syphon_publish(
                    self.raw,
                    std::ptr::from_ref(texture).cast::<c_void>().cast_mut(),
                    Retained::as_ptr(&self.queue).cast::<c_void>().cast_mut(),
                    width,
                    height,
                )
            };
            if ok == 0 {
                return Err(GpuError::Readback("Syphon frame publish failed".into()));
            }
            Ok(())
        }
        /// Stop broadcasting. Idempotent; also runs on Drop.
        pub fn stop(&self) {
            // SAFETY: server pointer is live for self's lifetime.
            unsafe { native::kr_syphon_server_stop(self.raw) }
        }
    }
    impl Drop for SyphonServer {
        fn drop(&mut self) {
            // SAFETY: stop is idempotent; the retained server is released once.
            unsafe {
                native::kr_syphon_server_stop(self.raw);
                native::kr_syphon_server_release(self.raw);
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub use imp::{SyphonServer, publish_texture};
