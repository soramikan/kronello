use kronello_service::{Service, ServiceError};
use serde_json::Value;
use std::ffi::c_void;

#[cfg(target_os = "macos")]
mod metal {
    use super::*;
    use kronello_service::{BackendSelection, Request};
    use objc2::{rc::Retained, runtime::AnyObject};
    use serde_json::json;
    pub struct Layer(pub Retained<AnyObject>);
    // SAFETY: the retained CAMetalLayer is only used by Metal on the dedicated
    // worker. AppKit view installation stays on the main thread. No NSView or
    // other main-thread-only Objective-C object crosses this boundary.
    unsafe impl Send for Layer {}
    impl Layer {
        pub unsafe fn retain(ptr: *mut c_void) -> Self {
            // SAFETY: C contract requires a live CAMetalLayer.
            Self(unsafe { Retained::retain(ptr.cast::<AnyObject>()) }.expect("non-null layer"))
        }
    }
    /// IO-001 (ADR-0134): the rendered program frame handed to every output
    /// route. One redraw produces exactly one `FrameSource`, so the program
    /// monitor, the reference monitor, and Syphon all present the identical
    /// pixels through the identical presentation transform.
    pub struct FrameSource {
        pub revision: String,
        pub texture: wgpu::Texture,
        /// Rendered-region pixel extent after budget-fit shrinking.
        pub pixels: [u32; 2],
        pub crop_origin: [usize; 2],
        pub backend: String,
        pub cpu_uploads: u8,
    }
    impl FrameSource {
        pub fn view(&self) -> wgpu::TextureView {
            self.texture.create_view(&Default::default())
        }
        /// Scale mapping target surface pixels into rendered-region pixels,
        /// the `transform.xy` contract of `preview_scaled.wgsl`.
        pub fn scale_for(&self, surface: [u32; 2]) -> [f32; 2] {
            [
                self.pixels[0] as f32 / surface[0].max(1) as f32,
                self.pixels[1] as f32 / surface[1].max(1) as f32,
            ]
        }
    }
    /// Whether a present attempt put a frame on the output. Occlusion and
    /// acquire timeouts are typed skips, never failures.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum PresentOutcome {
        Presented,
        Skipped(&'static str),
    }
    fn failure(code: &str, e: impl std::fmt::Display) -> ServiceError {
        ServiceError::new(code, e.to_string())
    }
    /// Shared intermediate-surface bound from check_scene_budget. A preview
    /// scene whose estimate does not fit is re-rendered smaller and scaled up
    /// at presentation instead of failing the frame.
    const SURFACE_BUDGET: u64 = 512 * 1024 * 1024;
    const MAX_FIT_ATTEMPTS: u32 = 8;
    fn is_surface_budget(e: &ServiceError) -> bool {
        e.code == "UNSUPPORTED_FEATURE" && e.message.contains("surface budget")
    }
    fn halve(pixels: [u32; 2]) -> [u32; 2] {
        [(pixels[0] / 2).max(1), (pixels[1] / 2).max(1)]
    }
    /// A CAMetalLayer surface for one output destination.
    pub fn metal_surface(
        instance: &wgpu::Instance,
        layer: &Layer,
    ) -> Result<wgpu::Surface<'static>, ServiceError> {
        // SAFETY: Layer retains the CAMetalLayer through the surface lifetime.
        unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(
                Retained::as_ptr(&layer.0).cast_mut().cast(),
            ))
        }
        .map_err(|e| failure("SURFACE_UNAVAILABLE", e))
    }
    /// Fullscreen-triangle presentation blit with the exact crop/scale and
    /// SDR sRGB encode the program monitor uses. Every `OutputDevice` renders
    /// through this same transform, so semantic parity is by construction.
    pub struct Blit {
        pipeline: wgpu::RenderPipeline,
        layout: wgpu::BindGroupLayout,
        pipeline_scaled: wgpu::RenderPipeline,
        layout_scaled: wgpu::BindGroupLayout,
        sampler: wgpu::Sampler,
    }
    impl Blit {
        pub fn new(gpu: &kronello_gpu::GpuContext, format: wgpu::TextureFormat) -> Self {
            let layout = gpu
                .device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("preview"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                    ],
                });
            let shader = gpu
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("preview"),
                    source: wgpu::ShaderSource::Wgsl(include_str!("preview.wgsl").into()),
                });
            let pipeline_layout =
                gpu.device
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("preview"),
                        bind_group_layouts: &[Some(&layout)],
                        immediate_size: 0,
                    });
            let pipeline = gpu
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("preview"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vs"),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some("fs"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    primitive: Default::default(),
                    depth_stencil: None,
                    multisample: Default::default(),
                    multiview_mask: None,
                    cache: None,
                });
            let layout_scaled =
                gpu.device
                    .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                        label: Some("preview scaled"),
                        entries: &[
                            wgpu::BindGroupLayoutEntry {
                                binding: 0,
                                visibility: wgpu::ShaderStages::FRAGMENT,
                                ty: wgpu::BindingType::Texture {
                                    sample_type: wgpu::TextureSampleType::Float {
                                        filterable: true,
                                    },
                                    view_dimension: wgpu::TextureViewDimension::D2,
                                    multisampled: false,
                                },
                                count: None,
                            },
                            wgpu::BindGroupLayoutEntry {
                                binding: 1,
                                visibility: wgpu::ShaderStages::FRAGMENT,
                                ty: wgpu::BindingType::Buffer {
                                    ty: wgpu::BufferBindingType::Uniform,
                                    has_dynamic_offset: false,
                                    min_binding_size: None,
                                },
                                count: None,
                            },
                            wgpu::BindGroupLayoutEntry {
                                binding: 2,
                                visibility: wgpu::ShaderStages::FRAGMENT,
                                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                                count: None,
                            },
                        ],
                    });
            let shader_scaled = gpu
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("preview scaled"),
                    source: wgpu::ShaderSource::Wgsl(include_str!("preview_scaled.wgsl").into()),
                });
            let pipeline_layout_scaled =
                gpu.device
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("preview scaled"),
                        bind_group_layouts: &[Some(&layout_scaled)],
                        immediate_size: 0,
                    });
            let pipeline_scaled =
                gpu.device
                    .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                        label: Some("preview scaled"),
                        layout: Some(&pipeline_layout_scaled),
                        vertex: wgpu::VertexState {
                            module: &shader_scaled,
                            entry_point: Some("vs"),
                            compilation_options: Default::default(),
                            buffers: &[],
                        },
                        fragment: Some(wgpu::FragmentState {
                            module: &shader_scaled,
                            entry_point: Some("fs"),
                            compilation_options: Default::default(),
                            targets: &[Some(wgpu::ColorTargetState {
                                format,
                                blend: None,
                                write_mask: wgpu::ColorWrites::ALL,
                            })],
                        }),
                        primitive: Default::default(),
                        depth_stencil: None,
                        multisample: Default::default(),
                        multiview_mask: None,
                        cache: None,
                    });
            let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("preview"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            });
            Self {
                pipeline,
                layout,
                pipeline_scaled,
                layout_scaled,
                sampler,
            }
        }
        /// Draw the program frame into `target`. `scaled` selects the sampled
        /// transform; `scale`/`crop_origin` follow the shader contracts.
        pub fn draw(
            &self,
            gpu: &kronello_gpu::GpuContext,
            source: &wgpu::TextureView,
            scale: [f32; 2],
            crop_origin: [usize; 2],
            target: &wgpu::TextureView,
            scaled: bool,
        ) {
            use wgpu::util::DeviceExt;
            let uniform;
            let (layout, entries): (&wgpu::BindGroupLayout, Vec<wgpu::BindGroupEntry>) = if scaled {
                let bytes = [
                    scale[0],
                    scale[1],
                    crop_origin[0] as f32,
                    crop_origin[1] as f32,
                ]
                .into_iter()
                .flat_map(f32::to_ne_bytes)
                .collect::<Vec<_>>();
                uniform = gpu
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("preview transform"),
                        contents: &bytes,
                        usage: wgpu::BufferUsages::UNIFORM,
                    });
                (
                    &self.layout_scaled,
                    vec![
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(source),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: uniform.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                    ],
                )
            } else {
                let [x, y] = crop_origin;
                let bytes = [x as u32, y as u32, 0, 0]
                    .into_iter()
                    .flat_map(u32::to_ne_bytes)
                    .collect::<Vec<_>>();
                uniform = gpu
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("preview crop"),
                        contents: &bytes,
                        usage: wgpu::BufferUsages::UNIFORM,
                    });
                (
                    &self.layout,
                    vec![
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(source),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: uniform.as_entire_binding(),
                        },
                    ],
                )
            };
            let bindings = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("preview"),
                layout,
                entries: &entries,
            });
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("output presentation"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                pass.set_pipeline(if scaled {
                    &self.pipeline_scaled
                } else {
                    &self.pipeline
                });
                pass.set_bind_group(0, &bindings, &[]);
                pass.draw(0..3, 0..1);
            }
            gpu.queue.submit([encoder.finish()]);
        }
    }
    /// One CAMetalLayer surface plus its presentation state. Preview surfaces
    /// and the reference-monitor output are both `Presentation`s bound to the
    /// shared preview GPU context.
    pub struct Presentation {
        surface: wgpu::Surface<'static>,
        config: wgpu::SurfaceConfiguration,
        blit: Blit,
        // Drop after surface and GPU objects.
        _layer: Layer,
    }
    impl Presentation {
        /// Bind an already-created surface to `gpu`. The surface must have
        /// been created by the same instance that produced `adapter`.
        pub fn bind(
            adapter: &wgpu::Adapter,
            gpu: &kronello_gpu::GpuContext,
            surface: wgpu::Surface<'static>,
            layer: Layer,
            width: u32,
            height: u32,
        ) -> Result<Self, ServiceError> {
            let caps = surface.get_capabilities(adapter);
            let format = caps
                .formats
                .iter()
                .copied()
                .find(|f| *f == wgpu::TextureFormat::Bgra8Unorm)
                .ok_or_else(|| failure("UNSUPPORTED_FEATURE", "BGRA8 unorm surface required"))?;
            let config = wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format,
                width,
                height,
                color_space: wgpu::SurfaceColorSpace::Srgb,
                present_mode: wgpu::PresentMode::Fifo,
                desired_maximum_frame_latency: 2,
                alpha_mode: wgpu::CompositeAlphaMode::Opaque,
                view_formats: vec![],
            };
            let mut presentation = Self {
                surface,
                config,
                blit: Blit::new(gpu, format),
                _layer: layer,
            };
            presentation.resize(gpu, width, height)?;
            Ok(presentation)
        }
        pub fn size(&self) -> [u32; 2] {
            [self.config.width, self.config.height]
        }
        pub fn resize(
            &mut self,
            gpu: &kronello_gpu::GpuContext,
            width: u32,
            height: u32,
        ) -> Result<(), ServiceError> {
            let limit = gpu.device.limits().max_texture_dimension_2d;
            if width == 0 || height == 0 || width > limit || height > limit {
                return Err(ServiceError::invalid("surface size outside device limits"));
            }
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&gpu.device, &self.config);
            Ok(())
        }
        /// Acquire a drawable, blit `source` through the shared transform and
        /// present. Occluded/timeout acquires are typed skips.
        pub fn present(
            &mut self,
            gpu: &kronello_gpu::GpuContext,
            source: &wgpu::TextureView,
            scale: [f32; 2],
            crop_origin: [usize; 2],
        ) -> Result<PresentOutcome, ServiceError> {
            let mut frame = self.surface.get_current_texture();
            if matches!(frame, wgpu::CurrentSurfaceTexture::Outdated) {
                // The layer changed under us; reconfigure once and retry.
                self.surface.configure(&gpu.device, &self.config);
                frame = self.surface.get_current_texture();
            }
            let frame = match frame {
                wgpu::CurrentSurfaceTexture::Success(frame)
                | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
                // Not failures: the host skips this frame and redraws when visible.
                skipped @ (wgpu::CurrentSurfaceTexture::Occluded
                | wgpu::CurrentSurfaceTexture::Timeout) => {
                    return Ok(
                        if matches!(skipped, wgpu::CurrentSurfaceTexture::Occluded) {
                            PresentOutcome::Skipped("occluded")
                        } else {
                            PresentOutcome::Skipped("timeout")
                        },
                    );
                }
                other => {
                    return Err(failure("SURFACE_ACQUIRE_FAILED", format!("{other:?}")));
                }
            };
            let target = frame.texture.create_view(&Default::default());
            self.blit.draw(
                gpu,
                source,
                scale,
                crop_origin,
                &target,
                scale != [1.0, 1.0],
            );
            gpu.queue.present(frame);
            Ok(PresentOutcome::Presented)
        }
    }
    pub struct Preview {
        instance: wgpu::Instance,
        adapter: wgpu::Adapter,
        gpu: kronello_gpu::GpuContext,
        presentation: Presentation,
        /// Persistent software-decode session: successive `render_frame` calls
        /// reuse pooled decoders so playback decodes sequentially instead of
        /// reopening and seeking every frame.
        media: kronello_service::MediaSession,
    }
    impl Preview {
        pub fn attach(layer: Layer, width: u32, height: u32) -> Result<Self, ServiceError> {
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: wgpu::Backends::METAL,
                ..wgpu::InstanceDescriptor::new_without_display_handle()
            });
            let surface = metal_surface(&instance, &layer)?;
            let adapter =
                pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                    compatible_surface: Some(&surface),
                    ..Default::default()
                }))
                .map_err(|e| failure("ADAPTER_UNAVAILABLE", e))?;
            let gpu = pollster::block_on(kronello_gpu::GpuContext::with_adapter(&adapter))
                .map_err(|e| failure("DEVICE_UNAVAILABLE", e))?;
            let presentation = Presentation::bind(&adapter, &gpu, surface, layer, width, height)?;
            Ok(Self {
                instance,
                adapter,
                gpu,
                presentation,
                media: kronello_service::MediaSession::new(),
            })
        }
        /// The shared device every external output of this session renders
        /// through, so one program frame can fan out without cross-device
        /// copies.
        pub fn gpu(&self) -> &kronello_gpu::GpuContext {
            &self.gpu
        }
        /// IO-001: a second surface on this preview's instance/adapter — the
        /// reference-monitor output destination. The caller chooses which
        /// NSScreen's layer this is; display color space is applied on the
        /// layer by the host (CAMetalLayer colorspace).
        pub fn output_presentation(
            &self,
            layer: Layer,
            width: u32,
            height: u32,
        ) -> Result<Presentation, ServiceError> {
            let surface = metal_surface(&self.instance, &layer)?;
            Presentation::bind(&self.adapter, &self.gpu, surface, layer, width, height)
        }
        pub fn resize(&mut self, width: u32, height: u32) -> Result<(), ServiceError> {
            self.presentation.resize(&self.gpu, width, height)
        }
        /// Render stage of a redraw: decode the request, budget-fit the
        /// region, and produce the shared frame texture. Never presents.
        pub fn render_frame(
            &mut self,
            service: &Service<'_>,
            json: &str,
        ) -> Result<FrameSource, ServiceError> {
            let Request::RenderFrame(request) = serde_json::from_str(json)? else {
                return Err(ServiceError::invalid("preview requires render.frame"));
            };
            if request.input.region.pixels != self.presentation.size() {
                return Err(ServiceError::invalid(
                    "render region pixels must match surface size",
                ));
            }
            let cpu = request.backend == Some(BackendSelection::CpuReference);
            // Preview is a proxy: when a scene's intermediate-surface estimate
            // exceeds the shared budget the frame is rendered smaller and
            // scaled up at presentation instead of failing with
            // UNSUPPORTED_FEATURE.
            let (revision, texture, pixels, crop_origin, backend): (
                String,
                wgpu::Texture,
                [u32; 2],
                [usize; 2],
                String,
            ) = if cpu {
                let mut request = request;
                let mut attempts = 0;
                loop {
                    let pixels = request.input.region.pixels;
                    match service.render_requested_frame(&request) {
                        Ok(rendered) => {
                            let scale = [
                                pixels[0] as f32 / self.presentation.size()[0] as f32,
                                pixels[1] as f32 / self.presentation.size()[1] as f32,
                            ];
                            let scaled = scale != [1.0, 1.0];
                            let bytes: Vec<u8> = if scaled {
                                rendered
                                    .pixels
                                    .linear
                                    .iter()
                                    .flatten()
                                    .flat_map(|v| half::f16::from_f32(*v).to_bits().to_le_bytes())
                                    .collect()
                            } else {
                                rendered
                                    .pixels
                                    .linear
                                    .iter()
                                    .flatten()
                                    .flat_map(|v| v.to_ne_bytes())
                                    .collect()
                            };
                            let bytes_per_row = pixels[0] * if scaled { 8 } else { 16 };
                            let texture =
                                self.gpu.device.create_texture(&wgpu::TextureDescriptor {
                                    label: Some("explicit CPU reference preview"),
                                    size: wgpu::Extent3d {
                                        width: pixels[0],
                                        height: pixels[1],
                                        depth_or_array_layers: 1,
                                    },
                                    mip_level_count: 1,
                                    sample_count: 1,
                                    dimension: wgpu::TextureDimension::D2,
                                    format: if scaled {
                                        wgpu::TextureFormat::Rgba16Float
                                    } else {
                                        wgpu::TextureFormat::Rgba32Float
                                    },
                                    usage: wgpu::TextureUsages::COPY_DST
                                        | wgpu::TextureUsages::TEXTURE_BINDING,
                                    view_formats: &[],
                                });
                            self.gpu.queue.write_texture(
                                texture.as_image_copy(),
                                &bytes,
                                wgpu::TexelCopyBufferLayout {
                                    offset: 0,
                                    bytes_per_row: Some(bytes_per_row),
                                    rows_per_image: Some(pixels[1]),
                                },
                                wgpu::Extent3d {
                                    width: pixels[0],
                                    height: pixels[1],
                                    depth_or_array_layers: 1,
                                },
                            );
                            break (
                                rendered.metadata.revision,
                                texture,
                                pixels,
                                [0, 0],
                                rendered.metadata.backend,
                            );
                        }
                        Err(error)
                            if attempts < MAX_FIT_ATTEMPTS
                                && is_surface_budget(&error)
                                && pixels != [1, 1] =>
                        {
                            request.input.region.pixels = halve(pixels);
                            attempts += 1;
                        }
                        Err(error) => return Err(error),
                    }
                }
            } else {
                // Budget-fit sizes against the unresolved DAG so retries never
                // re-decode media; resolution happens once at the accepted
                // output size.
                let mut request = request;
                let mut attempts = 0;
                let (revision, dag) = loop {
                    let (revision, dag) = service.preview_dag_unresolved(&request)?;
                    let (rendered, surfaces) =
                        self.gpu.preview_surface_estimate_unresolved(&dag)?;
                    let budget = u64::from(rendered[0]) * u64::from(rendered[1]) * 8 * surfaces;
                    if budget <= SURFACE_BUDGET
                        || attempts >= MAX_FIT_ATTEMPTS
                        || request.input.region.pixels == [1, 1]
                    {
                        break (revision, dag);
                    }
                    request.input.region.pixels = halve(request.input.region.pixels);
                    attempts += 1;
                };
                let resolved = service.resolve_dag_resident(
                    &dag,
                    &request.input.project,
                    &mut self.media,
                    &self.gpu,
                )?;
                let kronello_service::ResolvedResidentMedia {
                    dag,
                    resident,
                    input_identities: identities,
                } = resolved;
                let texture = self
                    .gpu
                    .preview_texture_resident(&dag, &resident, &identities)?;
                (
                    revision,
                    texture,
                    request.input.region.pixels,
                    dag.crop_origin(),
                    "metal".into(),
                )
            };
            Ok(FrameSource {
                revision,
                texture,
                pixels,
                crop_origin,
                backend,
                cpu_uploads: u8::from(cpu),
            })
        }
        /// Present stage of a program-monitor redraw: blit the shared frame
        /// onto this preview's surface and report presentation metadata.
        pub fn present(&mut self, source: &FrameSource) -> Result<Value, ServiceError> {
            let scale = source.scale_for(self.presentation.size());
            let view = source.view();
            let outcome = self
                .presentation
                .present(&self.gpu, &view, scale, source.crop_origin)?;
            let size = self.presentation.size();
            match outcome {
                PresentOutcome::Skipped(skipped) => Ok(
                    json!({"status":"success","preview":{"revision":source.revision,"pixels":size,"backend":source.backend,"image_readbacks":0,"presented":false,"skipped":skipped}}),
                ),
                PresentOutcome::Presented => {
                    // Submission timestamp, not a physical scanout measurement.
                    unsafe extern "C" {
                        fn mach_absolute_time() -> u64;
                    }
                    // SAFETY: platform clock call without pointer arguments.
                    let presentation_host_time = unsafe { mach_absolute_time() };
                    Ok(
                        json!({"status":"success","preview":{"revision":source.revision,"pixels":size,"backend":source.backend,"image_readbacks":0,"cpu_uploads":source.cpu_uploads,"presented":true,"presentation_host_time":presentation_host_time.to_string()}}),
                    )
                }
            }
        }
        pub fn redraw(&mut self, service: &Service<'_>, json: &str) -> Result<Value, ServiceError> {
            let source = self.render_frame(service, json)?;
            self.present(&source)
        }
    }
}
#[cfg(target_os = "macos")]
pub use metal::*;

#[cfg(not(target_os = "macos"))]
pub struct Layer;
#[cfg(not(target_os = "macos"))]
impl Layer {
    pub unsafe fn retain(_: *mut c_void) -> Self {
        Self
    }
}
/// IO-001 stubs: external outputs can never activate off macOS.
#[cfg(not(target_os = "macos"))]
pub struct FrameSource;
#[cfg(not(target_os = "macos"))]
pub struct Presentation;
#[cfg(not(target_os = "macos"))]
pub struct Preview;
#[cfg(not(target_os = "macos"))]
impl Preview {
    pub fn attach(_: Layer, _: u32, _: u32) -> Result<Self, ServiceError> {
        Err(ServiceError::new(
            "UNSUPPORTED_FEATURE",
            "native preview requires macOS",
        ))
    }
    pub fn resize(&mut self, _: u32, _: u32) -> Result<(), ServiceError> {
        Err(ServiceError::new(
            "UNSUPPORTED_FEATURE",
            "native preview requires macOS",
        ))
    }
    pub fn redraw(&mut self, _: &Service<'_>, _: &str) -> Result<Value, ServiceError> {
        Err(ServiceError::new(
            "UNSUPPORTED_FEATURE",
            "native preview requires macOS",
        ))
    }
    /// IO-001 stubs: the FFI worker compiles the same call surface on every
    /// platform; each entry point still fails with a typed error here.
    pub fn output_presentation(
        &self,
        _: Layer,
        _: u32,
        _: u32,
    ) -> Result<Presentation, ServiceError> {
        Err(ServiceError::new(
            "UNSUPPORTED_FEATURE",
            "external output requires macOS",
        ))
    }
    pub fn render_frame(&mut self, _: &Service<'_>, _: &str) -> Result<FrameSource, ServiceError> {
        Err(ServiceError::new(
            "UNSUPPORTED_FEATURE",
            "native preview requires macOS",
        ))
    }
    pub fn present(&mut self, _: &FrameSource) -> Result<Value, ServiceError> {
        Err(ServiceError::new(
            "UNSUPPORTED_FEATURE",
            "native preview requires macOS",
        ))
    }
}
