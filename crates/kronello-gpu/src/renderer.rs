use crate::{GpuError, InputSpace, Layer, RenderSize, TransferStats, WorkingSpace, pixel_count};
use half::f16;
use std::time::Duration;
use wgpu::util::DeviceExt;
pub const SHADER: &str = include_str!("composite.wgsl");

pub struct GpuContext {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter_info: wgpu::AdapterInfo,
    pipeline: wgpu::ComputePipeline,
}
#[derive(Debug)]
pub struct RenderOutput {
    pub width: u32,
    pub height: u32,
    pub working_space: WorkingSpace,
    pub design_extent: [f32; 2],
    pub pixels: Vec<[f32; 4]>,
    pub rgba16f: Vec<u8>,
    pub transfers: TransferStats,
}
impl GpuContext {
    pub fn new() -> Result<Self, GpuError> {
        let backends = match std::env::var("WGPU_BACKEND").as_deref() {
            Ok("metal") => wgpu::Backends::METAL,
            Ok("vulkan") => wgpu::Backends::VULKAN,
            Ok(_) => {
                return Err(GpuError::UnsupportedFeature(
                    "spike supports explicit metal or vulkan backend",
                ));
            }
            Err(std::env::VarError::NotPresent) => {
                if cfg!(target_os = "macos") {
                    wgpu::Backends::METAL
                } else {
                    wgpu::Backends::VULKAN
                }
            }
            Err(_) => return Err(GpuError::InvalidInput("invalid WGPU_BACKEND")),
        };
        pollster::block_on(Self::with_backends(backends))
    }
    pub async fn with_backends(backends: wgpu::Backends) -> Result<Self, GpuError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .map_err(|e| GpuError::AdapterUnavailable(e.to_string()))?;
        let usages = wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST;
        if !adapter
            .get_texture_format_features(wgpu::TextureFormat::Rgba16Float)
            .allowed_usages
            .contains(usages)
        {
            return Err(GpuError::UnsupportedFeature(
                "RGBA16F sampled/storage/copy textures",
            ));
        }
        let adapter_info = adapter.get_info();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("GPU-001"),
                ..Default::default()
            })
            .await
            .map_err(|e| GpuError::DeviceUnavailable(e.to_string()))?;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("composite"),
            entries: &[
                texture_entry(0),
                texture_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba16Float,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("composite"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("composite"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("composite"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        Ok(Self {
            device,
            queue,
            adapter_info,
            pipeline,
        })
    }
    fn check_size(&self, width: u32, height: u32) -> Result<(), GpuError> {
        pixel_count(width, height)?;
        let limits = self.device.limits();
        if width > limits.max_texture_dimension_2d
            || height > limits.max_texture_dimension_2d
            || width.div_ceil(8) > limits.max_compute_workgroups_per_dimension
            || height.div_ceil(8) > limits.max_compute_workgroups_per_dimension
        {
            return Err(GpuError::UnsupportedFeature("image exceeds device limits"));
        }
        Ok(())
    }
    pub fn texture(
        &self,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
        usage: wgpu::TextureUsages,
    ) -> Result<wgpu::Texture, GpuError> {
        self.check_size(width, height)?;
        Ok(self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("GPU-001 image"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        }))
    }
    fn composite_texture(
        &self,
        size: RenderSize,
        layers: &[Layer],
        working: WorkingSpace,
    ) -> Result<(wgpu::Texture, TransferStats), GpuError> {
        size.validate()?;
        let [width, height] = size.output_resolution;
        self.check_size(width, height)?;
        for layer in layers {
            layer.validate()?;
            self.check_size(layer.image.width, layer.image.height)?;
        }
        let usage = wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST;
        let textures = [
            self.texture(width, height, wgpu::TextureFormat::Rgba16Float, usage)?,
            self.texture(width, height, wgpu::TextureFormat::Rgba16Float, usage)?,
        ];
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let mut stats = TransferStats::default();
        let mut current = 0;
        for layer in layers {
            let source = self.texture(
                layer.image.width,
                layer.image.height,
                wgpu::TextureFormat::Rgba32Float,
                wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            )?;
            let bytes: Vec<u8> = layer
                .image
                .pixels
                .iter()
                .flatten()
                .flat_map(|v| v.to_le_bytes())
                .collect();
            self.queue.write_texture(
                source.as_image_copy(),
                &bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(layer.image.width * 16),
                    rows_per_image: Some(layer.image.height),
                },
                source.size(),
            );
            stats.cpu_upload_pixel_bytes += bytes.len() as u64;
            stats.cpu_upload_pixel_operations += 1;
            let (sin, cos) = layer.rotation_degrees.to_radians().sin_cos();
            let mut uniform: Vec<u8> = layer
                .size
                .iter()
                .chain(&layer.translation)
                .chain([cos, sin].iter())
                .flat_map(|v| v.to_le_bytes())
                .collect();
            uniform.extend_from_slice(
                &(match layer.image.space {
                    InputSpace::Srgb => 0u32,
                    InputSpace::LinearRec709 => 1,
                    InputSpace::LinearRec2020 => 2,
                })
                .to_le_bytes(),
            );
            uniform.extend_from_slice(
                &(match working {
                    WorkingSpace::LinearRec709 => 0u32,
                    WorkingSpace::LinearRec2020 => 1,
                })
                .to_le_bytes(),
            );
            uniform.extend(size.pixel_scale().iter().flat_map(|v| v.to_le_bytes()));
            uniform.extend([0u8; 8]);
            let buffer = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("transform and color"),
                    contents: &uniform,
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            stats.cpu_upload_control_bytes += uniform.len() as u64;
            stats.cpu_upload_control_operations += 1;
            let source_view = source.create_view(&Default::default());
            let previous = textures[current].create_view(&Default::default());
            let output = textures[1 - current].create_view(&Default::default());
            let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("layer"),
                layout: &self.pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&source_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&previous),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&output),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: buffer.as_entire_binding(),
                    },
                ],
            });
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &group, &[]);
                pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
            }
            current = 1 - current;
        }
        self.queue.submit([encoder.finish()]);
        Ok((textures[current].clone(), stats))
    }
    pub fn render(
        &self,
        size: RenderSize,
        layers: &[Layer],
        working: WorkingSpace,
    ) -> Result<RenderOutput, GpuError> {
        let (texture, stats) = self.composite_texture(size, layers, working)?;
        self.finish_render(size, &texture, working, stats)
    }
    /// Minimal isolated root group: composite children first, then apply opacity
    /// to both premultiplied RGB and alpha in a second RGBA16F GPU pass.
    pub fn render_isolated_group(
        &self,
        size: RenderSize,
        children: &[Layer],
        opacity: f32,
        working: WorkingSpace,
    ) -> Result<RenderOutput, GpuError> {
        if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
            return Err(GpuError::InvalidInput(
                "group opacity must be finite in [0, 1]",
            ));
        }
        let (texture, mut stats) = self.composite_texture(size, children, working)?;
        let [width, height] = size.output_resolution;
        let output = self.texture(
            width,
            height,
            wgpu::TextureFormat::Rgba16Float,
            wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        )?;
        let shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("isolated opacity"),
                source: wgpu::ShaderSource::Wgsl(include_str!("opacity.wgsl").into()),
            });
        let pipeline = self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("isolated opacity"),
                layout: None,
                module: &shader,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });
        let mut uniform = opacity.to_le_bytes().to_vec();
        uniform.extend([0u8; 12]);
        let buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("group opacity"),
                contents: &uniform,
                usage: wgpu::BufferUsages::UNIFORM,
            });
        stats.cpu_upload_control_bytes += 16;
        stats.cpu_upload_control_operations += 1;
        let source_view = texture.create_view(&Default::default());
        let output_view = output.create_view(&Default::default());
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("isolated opacity"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&source_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&output_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: buffer.as_entire_binding(),
                },
            ],
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
        }
        self.queue.submit([encoder.finish()]);
        self.finish_render(size, &output, working, stats)
    }
    fn finish_render(
        &self,
        size: RenderSize,
        texture: &wgpu::Texture,
        working: WorkingSpace,
        mut stats: TransferStats,
    ) -> Result<RenderOutput, GpuError> {
        let [width, height] = size.output_resolution;
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let copy = self.texture(
            width,
            height,
            wgpu::TextureFormat::Rgba16Float,
            wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
        )?;
        encoder.copy_texture_to_texture(texture.as_image_copy(), copy.as_image_copy(), copy.size());
        stats.gpu_copy_bytes = u64::from(width) * u64::from(height) * 8;
        stats.gpu_copy_operations = 1;
        self.queue.submit([encoder.finish()]);
        let bytes = self.read_texture(&copy, 8, &mut stats)?;
        let pixels = decode_rgba16f(&bytes)?;
        Ok(RenderOutput {
            width,
            height,
            working_space: working,
            design_extent: size.design_extent,
            pixels,
            rgba16f: bytes,
            transfers: stats,
        })
    }
    /// Synchronous completion fence for native interop. Bounded wait, no busy polling.
    pub fn wait(&self) -> Result<(), GpuError> {
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(30)),
            })
            .map_err(|e| GpuError::Readback(e.to_string()))?;
        Ok(())
    }
    /// Readback includes alignment padding in TransferStats but strips it in the result.
    pub fn read_texture(
        &self,
        texture: &wgpu::Texture,
        bytes_per_pixel: u32,
        stats: &mut TransferStats,
    ) -> Result<Vec<u8>, GpuError> {
        let expected_bpp = match texture.format() {
            wgpu::TextureFormat::Rgba16Float => 8,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm => 4,
            _ => return Err(GpuError::UnsupportedFeature("readback texture format")),
        };
        if bytes_per_pixel != expected_bpp
            || !texture.usage().contains(wgpu::TextureUsages::COPY_SRC)
        {
            return Err(GpuError::InvalidInput("readback format or usage mismatch"));
        }
        let row = texture.width() * bytes_per_pixel;
        let aligned =
            row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let size = u64::from(aligned) * u64::from(texture.height());
        if size > self.device.limits().max_buffer_size {
            return Err(GpuError::UnsupportedFeature(
                "readback exceeds buffer limit",
            ));
        }
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(aligned),
                    rows_per_image: Some(texture.height()),
                },
            },
            texture.size(),
        );
        self.queue.submit([encoder.finish()]);
        let (send, receive) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = send.send(result);
            });
        self.wait()?;
        receive
            .recv_timeout(Duration::from_secs(1))
            .map_err(|e| GpuError::Readback(e.to_string()))?
            .map_err(|e| GpuError::Readback(e.to_string()))?;
        let mapped = buffer
            .slice(..)
            .get_mapped_range()
            .map_err(|e| GpuError::Readback(e.to_string()))?;
        let bytes = mapped
            .chunks_exact(aligned as usize)
            .flat_map(|r| r[..row as usize].iter().copied())
            .collect();
        drop(mapped);
        buffer.unmap();
        stats.gpu_readback_bytes += size;
        stats.gpu_readback_operations += 1;
        Ok(bytes)
    }
}
fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}
pub fn decode_rgba16f(bytes: &[u8]) -> Result<Vec<[f32; 4]>, GpuError> {
    if !bytes.len().is_multiple_of(8) {
        return Err(GpuError::InvalidInput("invalid RGBA16F length"));
    }
    bytes
        .chunks_exact(8)
        .map(|p| {
            let value =
                std::array::from_fn(|i| f16::from_le_bytes([p[i * 2], p[i * 2 + 1]]).to_f32());
            if value.iter().any(|v| !v.is_finite()) {
                Err(GpuError::InvalidInput("non-finite RGBA16F"))
            } else if !(0.0..=1.0).contains(&value[3])
                || (value[3] == 0.0 && value[..3].iter().any(|v| *v != 0.0))
            {
                Err(GpuError::InvalidInput("invalid premultiplied RGBA16F"))
            } else {
                Ok(value)
            }
        })
        .collect()
}
