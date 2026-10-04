use kronello_service::{Service, ServiceError};
use serde_json::Value;
use std::ffi::c_void;

#[cfg(target_os = "macos")]
mod metal {
    use super::*;
    use kronello_service::Request;
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
        // Drop after surface and GPU objects.
        _layer: Layer,
    }
    fn failure(code: &str, e: impl std::fmt::Display) -> ServiceError {
        ServiceError::new(code, e.to_string())
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
            let mut preview = Self {
                surface,
                gpu,
                config,
                pipeline,
                layout,
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
            let (revision, dag) = service.preview_dag(&request)?;
            let texture = self.gpu.preview_texture(&dag)?;
            let frame = match self.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(frame)
                | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
                other => return Err(failure("SURFACE_ACQUIRE_FAILED", format!("{other:?}"))),
            };
            let source = texture.create_view(&Default::default());
            let target = frame.texture.create_view(&Default::default());
            use wgpu::util::DeviceExt;
            let [x, y] = dag.crop_origin();
            let bytes = [x as u32, y as u32, 0, 0]
                .into_iter()
                .flat_map(u32::to_ne_bytes)
                .collect::<Vec<_>>();
            let crop = self
                .gpu
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("preview crop"),
                    contents: &bytes,
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            let bindings = self
                .gpu
                .device
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("preview"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&source),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: crop.as_entire_binding(),
                        },
                    ],
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
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &bindings, &[]);
                pass.draw(0..3, 0..1);
            }
            self.gpu.queue.submit([encoder.finish()]);
            self.gpu.queue.present(frame);
            Ok(
                json!({"status":"success","preview":{"revision":revision,"pixels":[self.config.width,self.config.height],"backend":"metal","image_readbacks":0}}),
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
