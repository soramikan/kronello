//! IO-001 (ADR-0134): the `OutputDevice` contract and the per-session output
//! set driven by the FFI worker. External outputs are runtime resources, never
//! document state. Activation is only through the explicit `io.output.*`
//! commands; absent transports, frameworks, SDKs, or hardware are typed
//! `UNSUPPORTED_FEATURE`/`SURFACE_UNAVAILABLE` errors — never a silent no-op
//! and never an implicit fallback to another output kind.
use kronello_service::{
    IoOutputEnableRequest, IoOutputListResult, IoOutputStateResult, OutputDeviceKind, ServiceError,
    UnimplementedVendorAdapter, output_device_list, vendor_output_open, with_active,
};
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// The format/colorspace/pacing contract one output route honors. Surfaced
/// through `io.output.list` detail so callers can reason about the route
/// without platform types leaking across the FFI boundary.
#[derive(Debug, Clone, Copy)]
pub struct OutputContract {
    pub format: &'static str,
    pub colorspace: &'static str,
    pub pacing: &'static str,
}

/// The typed contract every external output route implements. `present` gets
/// the identical `FrameSource` the program monitor presented; an output may
/// skip only with a typed outcome and must fail with a typed error otherwise.
#[cfg(target_os = "macos")]
pub trait OutputDevice {
    fn kind(&self) -> OutputDeviceKind;
    fn contract(&self) -> OutputContract;
    fn present(
        &mut self,
        source: &crate::preview::FrameSource,
        gpu: &kronello_gpu::GpuContext,
    ) -> Result<crate::preview::PresentOutcome, ServiceError>;
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use crate::preview::{Blit, FrameSource, PresentOutcome, Presentation};
    use kronello_gpu::GpuContext;

    /// Fullscreen reference monitor: a CAMetalLayer on a non-main NSScreen,
    /// bound to the program preview's device. The host applies the target
    /// display color space on the layer; the shared blit re-encodes the
    /// identical frame for this surface's size.
    pub struct RefMonitorOutput {
        presentation: Presentation,
    }
    impl RefMonitorOutput {
        pub fn new(presentation: Presentation) -> Self {
            Self { presentation }
        }
        pub fn resize(
            &mut self,
            width: u32,
            height: u32,
            gpu: &GpuContext,
        ) -> Result<(), ServiceError> {
            self.presentation.resize(gpu, width, height)
        }
    }
    impl OutputDevice for RefMonitorOutput {
        fn kind(&self) -> OutputDeviceKind {
            OutputDeviceKind::RefMonitor
        }
        fn contract(&self) -> OutputContract {
            OutputContract {
                format: "BGRA8 CAMetalLayer drawable",
                colorspace: "linear Rec.709 premultiplied working frame, SDR sRGB encode; the host assigns NSScreen.colorSpace to the layer",
                pacing: "Core Animation FIFO present (vsync-paced swapchain)",
            }
        }
        fn present(
            &mut self,
            source: &FrameSource,
            gpu: &GpuContext,
        ) -> Result<PresentOutcome, ServiceError> {
            // Always the sampled transform: the external surface is sized by
            // its display, not by the monitor view, so the frame is scaled to
            // fill it with the same linear→sRGB encode.
            let scale = source.scale_for(self.presentation.size());
            let view = source.view();
            self.presentation
                .present(gpu, &view, scale, source.crop_origin)
        }
    }

    /// Syphon server publishing one private BGRA8 MTLTexture per frame. The
    /// framework is runtime-detected; absent Syphon is a typed reject at
    /// enable time, never a silent drop of frames.
    pub struct SyphonOutput {
        server: kronello_framebridge::output::SyphonServer,
        blit: Blit,
        texture: Option<(
            objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2_metal::MTLTexture>>,
            wgpu::Texture,
        )>,
    }
    impl SyphonOutput {
        pub fn create(name: &str, gpu: &GpuContext) -> Result<Self, ServiceError> {
            if !kronello_framebridge::output::syphon_detected() {
                return Err(ServiceError::new(
                    "UNSUPPORTED_FEATURE",
                    "Syphon framework not detected at runtime (install Syphon.framework to publish)",
                ));
            }
            // SAFETY: borrow the Metal device the preview GPU context owns;
            // the server and published textures stay on this device.
            let device = unsafe { gpu.device.as_hal::<wgpu::hal::api::Metal>() }
                .ok_or_else(|| {
                    ServiceError::new(
                        "UNSUPPORTED_FEATURE",
                        "Syphon output requires the Metal preview device",
                    )
                })?
                .raw_device()
                .clone();
            let server = kronello_framebridge::output::SyphonServer::create(name, &device)
                .map_err(gpu_error)?;
            Ok(Self {
                server,
                blit: Blit::new(gpu, wgpu::TextureFormat::Bgra8Unorm),
                texture: None,
            })
        }
    }
    fn gpu_error(e: kronello_gpu::GpuError) -> ServiceError {
        let code = match &e {
            kronello_gpu::GpuError::AdapterUnavailable(_) => "ADAPTER_UNAVAILABLE",
            kronello_gpu::GpuError::DeviceUnavailable(_) => "DEVICE_UNAVAILABLE",
            kronello_gpu::GpuError::UnsupportedFeature(_) => "UNSUPPORTED_FEATURE",
            kronello_gpu::GpuError::InvalidInput(_) => "INVALID_INPUT",
            kronello_gpu::GpuError::Readback(_) => "READBACK_FAILED",
            kronello_gpu::GpuError::CacheIo(_) => "CACHE_IO",
            kronello_gpu::GpuError::ObservationBusy => "RENDER_BACKEND_BUSY",
        };
        ServiceError::new(code, e.to_string())
    }
    impl OutputDevice for SyphonOutput {
        fn kind(&self) -> OutputDeviceKind {
            OutputDeviceKind::Syphon
        }
        fn contract(&self) -> OutputContract {
            OutputContract {
                format: "BGRA8 MTLTexture published through SyphonMetalServer",
                colorspace: "linear Rec.709 premultiplied working frame, SDR sRGB encode (program-monitor transform)",
                pacing: "per-redraw publish; command-buffer completion bounds each frame",
            }
        }
        fn present(
            &mut self,
            source: &FrameSource,
            gpu: &GpuContext,
        ) -> Result<PresentOutcome, ServiceError> {
            let size = source.pixels;
            let recreate = !matches!(&self.texture, Some((_, t)) if t.size().width == size[0] && t.size().height == size[1]);
            if recreate {
                self.texture = Some(
                    kronello_framebridge::output::publish_texture(gpu, size[0], size[1])
                        .map_err(gpu_error)?,
                );
            }
            let (native, texture) = self.texture.as_ref().expect("publish texture");
            // The publish texture matches the rendered region 1:1, so the
            // exact crop pipeline (integer texel fetch) applies the shared
            // encode without the scaled path's half-texel sampling offset.
            self.blit.draw(
                gpu,
                &source.view(),
                [1.0, 1.0],
                source.crop_origin,
                &texture.create_view(&Default::default()),
                false,
            );
            gpu.queue.submit([]);
            self.server
                .publish(native.as_ref(), size[0], size[1])
                .map_err(gpu_error)?;
            Ok(PresentOutcome::Presented)
        }
    }
}

/// Per-session output devices owned by the FFI worker. Presence in a field
/// means the transport resource exists (attached surface / created server);
/// `active` holds the kinds the caller explicitly enabled.
#[derive(Default)]
pub struct OutputSet {
    #[cfg(target_os = "macos")]
    ref_monitor: Option<native::RefMonitorOutput>,
    #[cfg(target_os = "macos")]
    syphon: Option<native::SyphonOutput>,
    active: BTreeSet<OutputDeviceKind>,
}

/// Slot reserved for the reference-monitor layer on the shared surface ABI.
/// Preview slots 0-2 are program/source/export; output surfaces use 8+.
pub const REF_MONITOR_SLOT: u32 = 8;

impl OutputSet {
    #[cfg(target_os = "macos")]
    pub fn attach_ref_monitor(&mut self, presentation: crate::preview::Presentation) {
        self.ref_monitor = Some(native::RefMonitorOutput::new(presentation));
    }
    #[cfg(not(target_os = "macos"))]
    pub fn attach_ref_monitor(&mut self, _: crate::preview::Presentation) {}

    pub fn resize_ref_monitor(
        &mut self,
        #[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
        gpu: &kronello_gpu::GpuContext,
        width: u32,
        height: u32,
    ) -> Result<(), ServiceError> {
        #[cfg(target_os = "macos")]
        {
            let Some(device) = self.ref_monitor.as_mut() else {
                return Err(ServiceError::new(
                    "SURFACE_NOT_ATTACHED",
                    "attach the reference monitor surface first",
                ));
            };
            device.resize(width, height, gpu)
        }
        #[cfg(not(target_os = "macos"))]
        Err(ServiceError::new(
            "UNSUPPORTED_FEATURE",
            "external output requires macOS",
        ))
    }

    /// Shared enumeration with honest runtime detection plus this session's
    /// activation state. `native_session` is true here because the FFI worker
    /// is the transport that can actually drive outputs.
    pub fn list(&self) -> IoOutputListResult {
        with_active(output_device_list(true), &self.active)
    }

    /// Explicit activation. Each kind creates/binds its transport on first
    /// enable and every precondition failure is a typed error.
    pub fn enable(
        &mut self,
        request: &IoOutputEnableRequest,
        #[cfg_attr(not(target_os = "macos"), allow(unused_variables))] program: Option<
            &crate::preview::Preview,
        >,
    ) -> Result<IoOutputStateResult, ServiceError> {
        match request.kind {
            OutputDeviceKind::Sdi | OutputDeviceKind::Ndi => {
                let detected = match request.kind {
                    OutputDeviceKind::Sdi => kronello_framebridge::output::decklink_detected(),
                    _ => kronello_framebridge::output::ndi_detected(),
                };
                vendor_output_open(&UnimplementedVendorAdapter::new(request.kind, detected))
                    .map(|()| self.state(request.kind))
            }
            OutputDeviceKind::RefMonitor | OutputDeviceKind::Syphon => {
                self.enable_native(request, program)
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn enable_native(
        &mut self,
        request: &IoOutputEnableRequest,
        program: Option<&crate::preview::Preview>,
    ) -> Result<IoOutputStateResult, ServiceError> {
        match request.kind {
            OutputDeviceKind::RefMonitor => {
                if self.ref_monitor.is_none() {
                    return Err(ServiceError::new(
                        "SURFACE_UNAVAILABLE",
                        "reference monitor output requires an attached output surface; open the reference monitor window first",
                    ));
                }
                self.active.insert(OutputDeviceKind::RefMonitor);
                Ok(self.state(request.kind))
            }
            OutputDeviceKind::Syphon => {
                if self.syphon.is_none() {
                    let program = program.ok_or_else(|| {
                        ServiceError::new(
                            "SURFACE_UNAVAILABLE",
                            "Syphon output requires the attached program monitor surface",
                        )
                    })?;
                    let name = request.name.as_deref().unwrap_or("Kronello Program");
                    self.syphon = Some(native::SyphonOutput::create(name, program.gpu())?);
                }
                self.active.insert(OutputDeviceKind::Syphon);
                Ok(self.state(request.kind))
            }
            OutputDeviceKind::Sdi | OutputDeviceKind::Ndi => unreachable!("handled above"),
        }
    }
    #[cfg(not(target_os = "macos"))]
    fn enable_native(
        &mut self,
        request: &IoOutputEnableRequest,
        _: Option<&crate::preview::Preview>,
    ) -> Result<IoOutputStateResult, ServiceError> {
        Err(kronello_service::external_output_requires_native_session(
            request.kind,
        ))
    }

    /// Deactivate an output kind. The transport resource is released so a
    /// later enable rebuilds a clean route; disabling an inactive kind is a
    /// typed acknowledgement, not an error.
    pub fn disable(&mut self, kind: OutputDeviceKind) -> IoOutputStateResult {
        self.active.remove(&kind);
        #[cfg(target_os = "macos")]
        match kind {
            OutputDeviceKind::RefMonitor => self.ref_monitor = None,
            OutputDeviceKind::Syphon => self.syphon = None,
            _ => (),
        }
        self.state(kind)
    }

    fn state(&self, kind: OutputDeviceKind) -> IoOutputStateResult {
        IoOutputStateResult {
            kind,
            active: self.active.contains(&kind),
        }
    }

    /// Fan the program frame out to every explicitly enabled output. Per-kind
    /// outcomes are reported, never fatal to the program monitor itself.
    #[cfg(target_os = "macos")]
    pub fn present_all(
        &mut self,
        source: &crate::preview::FrameSource,
        gpu: &kronello_gpu::GpuContext,
    ) -> Value {
        let mut report = serde_json::Map::new();
        for kind in self.active.clone() {
            let device: Option<&mut dyn OutputDevice> = match kind {
                OutputDeviceKind::RefMonitor => self
                    .ref_monitor
                    .as_mut()
                    .map(|d| d as &mut dyn OutputDevice),
                OutputDeviceKind::Syphon => {
                    self.syphon.as_mut().map(|d| d as &mut dyn OutputDevice)
                }
                // Vendor kinds can never be active in this build.
                _ => None,
            };
            let Some(device) = device else {
                continue;
            };
            let outcome = match device.present(source, gpu) {
                Ok(crate::preview::PresentOutcome::Presented) => {
                    json!({"presented":true,"contract":contract_json(device.contract())})
                }
                Ok(crate::preview::PresentOutcome::Skipped(reason)) => {
                    json!({"presented":false,"skipped":reason})
                }
                Err(e) => {
                    json!({"presented":false,"error":{"code":e.code,"message":e.message}})
                }
            };
            report.insert(device.kind().wire_name().to_string(), outcome);
        }
        Value::Object(report)
    }
    #[cfg(not(target_os = "macos"))]
    pub fn present_all(
        &mut self,
        _: &crate::preview::FrameSource,
        _: &kronello_gpu::GpuContext,
    ) -> Value {
        Value::Object(serde_json::Map::new())
    }

    pub fn has_active(&self) -> bool {
        !self.active.is_empty()
    }
}

#[cfg(target_os = "macos")]
fn contract_json(contract: OutputContract) -> Value {
    json!({
        "format": contract.format,
        "colorspace": contract.colorspace,
        "pacing": contract.pacing,
    })
}

/// IO-001 acceptance evidence (ADR-0134): the external-output presentation
/// path renders through the identical transform the program monitor uses.
/// This test feeds a known linear premultiplied frame through `Blit` into an
/// offscreen BGRA8 target — the same draw every `OutputDevice` performs —
/// and compares the readback against the shader's SDR sRGB encode computed
/// on CPU. GPU-capable environments only; on machines without Metal the
/// construction fails and the test skips.
#[cfg(all(test, target_os = "macos"))]
mod tests {
    use crate::preview::Blit;
    use kronello_gpu::{GpuContext, TransferStats};

    /// The fragment encode in preview.wgsl / preview_scaled.wgsl.
    fn encode_srgb(linear: f32) -> u8 {
        let linear = linear.clamp(0.0, 1.0);
        let encoded = if linear <= 0.003_130_8 {
            12.92 * linear
        } else {
            1.055 * linear.powf(1.0 / 2.4) - 0.055
        };
        (encoded * 255.0).round() as u8
    }

    #[test]
    fn external_output_blit_matches_program_transform() {
        let Ok(gpu) = GpuContext::new() else {
            eprintln!("skipping: no GPU context");
            return;
        };
        let blit = Blit::new(&gpu, wgpu::TextureFormat::Bgra8Unorm);
        // Known linear premultiplied Rec.709 values (alpha forces 1.0 in the
        // presentation shader, so B channel parity includes the alpha case).
        let pixels: [[f32; 4]; 4] = [
            [0.5, 0.25, 0.125, 1.0],
            [0.9, 0.1, 0.001, 1.0],
            [0.0, 0.0, 0.0, 0.0],
            [0.2, 0.3, 0.4, 0.5],
        ];
        let source = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("parity source"),
            size: wgpu::Extent3d {
                width: 2,
                height: 2,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let bytes: Vec<u8> = pixels
            .iter()
            .flatten()
            .flat_map(|v| half::f16::from_f32(*v).to_bits().to_le_bytes())
            .collect();
        gpu.queue.write_texture(
            source.as_image_copy(),
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(16),
                rows_per_image: Some(2),
            },
            wgpu::Extent3d {
                width: 2,
                height: 2,
                depth_or_array_layers: 1,
            },
        );
        // The external-output target: offscreen BGRA8, exactly what the
        // Syphon publish texture and the ref-monitor drawable are.
        let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("parity target"),
            size: wgpu::Extent3d {
                width: 2,
                height: 2,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        // The same-size path every OutputDevice takes for a 1:1 publish: the
        // exact crop pipeline applies the program transform without the
        // scaled path's half-texel sampling offset.
        blit.draw(
            &gpu,
            &source.create_view(&Default::default()),
            [1.0, 1.0],
            [0, 0],
            &target.create_view(&Default::default()),
            false,
        );
        let mut transfers = TransferStats::default();
        let readback = gpu.read_texture(&target, 4, &mut transfers).unwrap();
        for (index, pixel) in pixels.iter().enumerate() {
            let offset = index * 4;
            // BGRA order; alpha is forced opaque by the presentation shader.
            for (channel, linear) in [pixel[2], pixel[1], pixel[0]].iter().enumerate() {
                let expected = encode_srgb(*linear);
                let actual = readback[offset + channel];
                assert!(
                    actual.abs_diff(expected) <= 1,
                    "pixel {index} channel {channel}: expected {expected}, got {actual}"
                );
            }
            assert_eq!(readback[offset + 3], 255, "alpha must be opaque");
        }
    }

    /// The scaled transform (ref-monitor path when the display surface
    /// differs from the rendered region): a uniform source makes the
    /// sampling position irrelevant, so the readback must equal the exact
    /// encoded value.
    #[test]
    fn external_output_scaled_blit_encodes_uniform_frame() {
        let Ok(gpu) = GpuContext::new() else {
            eprintln!("skipping: no GPU context");
            return;
        };
        let blit = Blit::new(&gpu, wgpu::TextureFormat::Bgra8Unorm);
        let color = [0.3f32, 0.2, 0.1, 1.0];
        let source = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("parity scaled source"),
            size: wgpu::Extent3d {
                width: 4,
                height: 4,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let bytes: Vec<u8> = (0..16)
            .flat_map(|_| color)
            .flat_map(|v| half::f16::from_f32(v).to_bits().to_le_bytes())
            .collect();
        gpu.queue.write_texture(
            source.as_image_copy(),
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(32),
                rows_per_image: Some(4),
            },
            wgpu::Extent3d {
                width: 4,
                height: 4,
                depth_or_array_layers: 1,
            },
        );
        let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("parity scaled target"),
            size: wgpu::Extent3d {
                width: 2,
                height: 2,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        // The ref-monitor transform: surface pixels scale into
        // rendered-region pixels through the same shader the program
        // monitor uses for budget-fitted frames.
        blit.draw(
            &gpu,
            &source.create_view(&Default::default()),
            [2.0, 2.0],
            [0, 0],
            &target.create_view(&Default::default()),
            true,
        );
        let mut transfers = TransferStats::default();
        let readback = gpu.read_texture(&target, 4, &mut transfers).unwrap();
        for offset in (0..readback.len()).step_by(4) {
            for (channel, linear) in [color[2], color[1], color[0]].iter().enumerate() {
                let expected = encode_srgb(*linear);
                let actual = readback[offset + channel];
                assert!(
                    actual.abs_diff(expected) <= 1,
                    "channel {channel}: expected {expected}, got {actual}"
                );
            }
            assert_eq!(readback[offset + 3], 255);
        }
    }
}
