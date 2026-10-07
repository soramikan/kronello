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
    pub struct Preview {
        surface: wgpu::Surface<'static>,
        gpu: kronello_gpu::GpuContext,
        config: wgpu::SurfaceConfiguration,
        pipeline: wgpu::RenderPipeline,
        layout: wgpu::BindGroupLayout,
        pipeline_scaled: wgpu::RenderPipeline,
        layout_scaled: wgpu::BindGroupLayout,
        sampler: wgpu::Sampler,
        // Drop after surface and GPU objects.
        _layer: Layer,
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
    impl Preview {
        pub fn attach(layer: Layer, width: u32, height: u32) -> Result<Self, ServiceError> {
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: wgpu::Backends::METAL,
                ..wgpu::InstanceDescriptor::new_without_display_handle()
            });
            // SAFETY: Layer retains the CAMetalLayer through the surface lifetime.
            let surface = unsafe {
                instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(
                    Retained::as_ptr(&layer.0).cast_mut().cast(),
                ))
            }
            .map_err(|e| failure("SURFACE_UNAVAILABLE", e))?;
            let adapter =
                pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                    compatible_surface: Some(&surface),
                    ..Default::default()
                }))
                .map_err(|e| failure("ADAPTER_UNAVAILABLE", e))?;
            let gpu = pollster::block_on(kronello_gpu::GpuContext::with_adapter(&adapter))
                .map_err(|e| failure("DEVICE_UNAVAILABLE", e))?;
            let caps = surface.get_capabilities(&adapter);
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
            let mut preview = Self {
                surface,
                gpu,
                config,
                pipeline,
                layout,
                pipeline_scaled,
                layout_scaled,
                sampler,
                _layer: layer,
            };
            preview.resize(width, height)?;
            Ok(preview)
        }
        pub fn resize(&mut self, width: u32, height: u32) -> Result<(), ServiceError> {
            let limit = self.gpu.device.limits().max_texture_dimension_2d;
            if width == 0 || height == 0 || width > limit || height > limit {
                return Err(ServiceError::invalid("surface size outside device limits"));
            }
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.gpu.device, &self.config);
            Ok(())
        }
        pub fn redraw(&mut self, service: &Service<'_>, json: &str) -> Result<Value, ServiceError> {
            let Request::RenderFrame(request) = serde_json::from_str(json)? else {
                return Err(ServiceError::invalid("preview requires render.frame"));
            };
            if request.input.region.pixels != [self.config.width, self.config.height] {
                return Err(ServiceError::invalid(
                    "render region pixels must match surface size",
                ));
            }
            let cpu = request.backend == Some(BackendSelection::CpuReference);
            // Preview is a proxy: when a scene's intermediate-surface estimate
            // exceeds the shared budget the frame is rendered smaller and
            // scaled up at presentation instead of failing with
            // UNSUPPORTED_FEATURE. `scale` converts surface pixels into
            // rendered-region pixels and `crop_origin` stays in texture pixels.
            let (revision, texture, scale, crop_origin, backend): (
                String,
                wgpu::Texture,
                [f32; 2],
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
                                pixels[0] as f32 / self.config.width as f32,
                                pixels[1] as f32 / self.config.height as f32,
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
                                scale,
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
                let mut request = request;
                let mut attempts = 0;
                let (revision, dag) = loop {
                    let (revision, dag) = service.preview_dag(&request)?;
                    let (rendered, surfaces) = self.gpu.preview_surface_estimate(&dag)?;
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
                let scale = [
                    request.input.region.pixels[0] as f32 / self.config.width as f32,
                    request.input.region.pixels[1] as f32 / self.config.height as f32,
                ];
                let texture = self.gpu.preview_texture(&dag)?;
                (revision, texture, scale, dag.crop_origin(), "metal".into())
            };
            let mut frame = self.surface.get_current_texture();
            if matches!(frame, wgpu::CurrentSurfaceTexture::Outdated) {
                // The layer changed under us; reconfigure once and retry.
                self.surface.configure(&self.gpu.device, &self.config);
                frame = self.surface.get_current_texture();
            }
            let frame = match frame {
                wgpu::CurrentSurfaceTexture::Success(frame)
                | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
                // Not failures: the host skips this frame and redraws when visible.
                skipped @ (wgpu::CurrentSurfaceTexture::Occluded
                | wgpu::CurrentSurfaceTexture::Timeout) => {
                    let skipped = if matches!(skipped, wgpu::CurrentSurfaceTexture::Occluded) {
                        "occluded"
                    } else {
                        "timeout"
                    };
                    return Ok(
                        json!({"status":"success","preview":{"revision":revision,"pixels":[self.config.width,self.config.height],"backend":backend,"image_readbacks":0,"presented":false,"skipped":skipped}}),
                    );
                }
                other => return Err(failure("SURFACE_ACQUIRE_FAILED", format!("{other:?}"))),
            };
            let scaled = scale != [1.0, 1.0];
            let source = texture.create_view(&Default::default());
            let target = frame.texture.create_view(&Default::default());
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
                uniform = self
                    .gpu
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
                            resource: wgpu::BindingResource::TextureView(&source),
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
                uniform = self
                    .gpu
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
                            resource: wgpu::BindingResource::TextureView(&source),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: uniform.as_entire_binding(),
                        },
                    ],
                )
            };
            let bindings = self
                .gpu
                .device
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("preview"),
                    layout,
                    entries: &entries,
                });
            let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("native preview"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &target,
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
            self.gpu.queue.submit([encoder.finish()]);
            self.gpu.queue.present(frame);
            // Submission timestamp, not a physical scanout measurement.
            unsafe extern "C" {
                fn mach_absolute_time() -> u64;
            }
            // SAFETY: platform clock call without pointer arguments.
            let presentation_host_time = unsafe { mach_absolute_time() };
            Ok(
                json!({"status":"success","preview":{"revision":revision,"pixels":[self.config.width,self.config.height],"backend":backend,"image_readbacks":0,"cpu_uploads":u8::from(cpu),"presented":true,"presentation_host_time":presentation_host_time.to_string()}}),
            )
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
}
