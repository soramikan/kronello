//! Device-bound image ownership at the native media boundary.
use crate::{GpuContext, GpuError, TransferStats, WorkingSpace};
use wgpu::util::DeviceExt;

/// Shared resident media sampling shader: BT.709/sRGB inverse transfer,
/// working-space matrix, premultiply and output-space nearest sampling. The
/// VideoToolbox import path and the software-decode upload path run the same
/// code so their cache identities stay comparable.
pub const RESIDENT_MEDIA_SHADER: &str = include_str!("resident_media.wgsl");

/// Decoded source pixels uploaded for GPU-resident sampling. The enum records
/// which `mode.x` path the shared shader takes.
pub enum ResidentUpload<'a> {
    /// Packed RGBA8 (8 bpc) already colorspace-converted by the decode layer;
    /// the shader applies only the transfer decode and working-space matrix.
    Rgba8 { pixels: &'a [u8], size: [u32; 2] },
    /// Planar video-range BT.709 YCbCr 4:2:0 8-bit. `u`/`v` are `size/2`
    /// planes interleaved into an RG8 texture for the shader's NV12 path.
    Yuv420pTv709 {
        y: &'a [u8],
        u: &'a [u8],
        v: &'a [u8],
        size: [u32; 2],
    },
}

#[derive(Debug, Clone)]
pub struct ResidentImage {
    texture: wgpu::Texture,
    identity: std::sync::Arc<()>,
    working: WorkingSpace,
    _allocation: crate::allocation::AllocationGuard,
}
impl ResidentImage {
    /// Allocate on the recorded device; no foreign texture can forge ownership.
    /// Producers initialize through this device queue before scene consumption.
    pub fn allocate(
        gpu: &GpuContext,
        size: [u32; 2],
        working: WorkingSpace,
    ) -> Result<Self, GpuError> {
        let _scope = gpu.render_scope()?;
        let texture = gpu.texture(
            size[0],
            size[1],
            wgpu::TextureFormat::Rgba16Float,
            wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
        )?;
        Ok(Self {
            texture,
            identity: gpu.identity.clone(),
            _allocation: gpu.track_resource(
                crate::allocation::ResourceKind::Resident,
                u64::from(size[0]) * u64::from(size[1]) * 8,
            ),
            working,
        })
    }
    pub(crate) fn allocation_guard(&self) -> crate::allocation::AllocationGuard {
        self._allocation.clone()
    }
    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }
    pub fn working_space(&self) -> WorkingSpace {
        self.working
    }
    pub fn validate(&self) -> Result<(), GpuError> {
        if self.texture.format() != wgpu::TextureFormat::Rgba16Float
            || self.texture.dimension() != wgpu::TextureDimension::D2
            || self.texture.depth_or_array_layers() != 1
            || self.texture.sample_count() != 1
            || !self
                .texture
                .usage()
                .contains(wgpu::TextureUsages::TEXTURE_BINDING)
        {
            return Err(GpuError::InvalidInput("resident image format/usage"));
        }
        Ok(())
    }
    pub fn validate_for(
        &self,
        gpu: &GpuContext,
        size: [u32; 2],
        working: WorkingSpace,
    ) -> Result<(), GpuError> {
        self.validate()?;
        if !std::sync::Arc::ptr_eq(&self.identity, &gpu.identity) {
            return Err(GpuError::InvalidInput(
                "resident image belongs to another device",
            ));
        }
        if [self.texture.width(), self.texture.height()] != size || self.working != working {
            return Err(GpuError::InvalidInput(
                "resident image dimensions/working space",
            ));
        }
        Ok(())
    }
}

impl GpuContext {
    /// Software-decode resident path: upload the decoded frame and run the
    /// shared media shader on device so output-space sampling, transfer
    /// decode and premultiply happen at the execution resolution instead of
    /// materializing a full-resolution working-space frame on the CPU.
    #[allow(clippy::too_many_arguments)]
    pub fn sample_upload_to_working(
        &self,
        upload: ResidentUpload<'_>,
        output: [u32; 2],
        extent: [f64; 2],
        output_to_local: [[f64; 3]; 2],
        working: WorkingSpace,
        srgb_transfer: bool,
        stats: &mut TransferStats,
    ) -> Result<ResidentImage, GpuError> {
        let _scope = self.render_scope()?;
        if output.contains(&0)
            || extent
                .iter()
                .any(|v| !v.is_finite() || *v <= 0.0 || *v > 1e12)
            || output_to_local
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || v.abs() > 1e12)
        {
            return Err(GpuError::InvalidInput("resident sampling transform"));
        }
        let source_usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
        let (first, second, nv12) = match upload {
            ResidentUpload::Rgba8 { pixels, size } => {
                if pixels.len() != size[0] as usize * size[1] as usize * 4 {
                    return Err(GpuError::InvalidInput("resident RGBA8 upload size"));
                }
                let texture = self.texture(
                    size[0],
                    size[1],
                    wgpu::TextureFormat::Rgba8Unorm,
                    source_usage,
                )?;
                self.queue.write_texture(
                    texture.as_image_copy(),
                    pixels,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(size[0] * 4),
                        rows_per_image: Some(size[1]),
                    },
                    wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                );
                stats.cpu_upload_pixel_bytes += pixels.len() as u64;
                stats.cpu_upload_pixel_operations += 1;
                let texture = std::sync::Arc::new(texture);
                (texture.clone(), texture, false)
            }
            ResidentUpload::Yuv420pTv709 { y, u, v, size } => {
                let [w, h] = size;
                let chroma = [w.div_ceil(2), h.div_ceil(2)];
                if y.len() != w as usize * h as usize
                    || u.len() != chroma[0] as usize * chroma[1] as usize
                    || v.len() != u.len()
                {
                    return Err(GpuError::InvalidInput("resident YUV420p upload size"));
                }
                let y_texture = self.texture(w, h, wgpu::TextureFormat::R8Unorm, source_usage)?;
                self.queue.write_texture(
                    y_texture.as_image_copy(),
                    y,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(w),
                        rows_per_image: Some(h),
                    },
                    wgpu::Extent3d {
                        width: w,
                        height: h,
                        depth_or_array_layers: 1,
                    },
                );
                // The shared shader reads an NV12-style interleaved chroma
                // plane; planar U/V join into RG8 on upload.
                let mut uv = vec![0u8; u.len() * 2];
                for (dst, (&a, &b)) in uv.chunks_exact_mut(2).zip(u.iter().zip(v)) {
                    dst[0] = a;
                    dst[1] = b;
                }
                let uv_texture = self.texture(
                    chroma[0],
                    chroma[1],
                    wgpu::TextureFormat::Rg8Unorm,
                    source_usage,
                )?;
                self.queue.write_texture(
                    uv_texture.as_image_copy(),
                    &uv,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(chroma[0] * 2),
                        rows_per_image: Some(chroma[1]),
                    },
                    wgpu::Extent3d {
                        width: chroma[0],
                        height: chroma[1],
                        depth_or_array_layers: 1,
                    },
                );
                stats.cpu_upload_pixel_bytes += (y.len() + uv.len()) as u64;
                stats.cpu_upload_pixel_operations += 2;
                (
                    std::sync::Arc::new(y_texture),
                    std::sync::Arc::new(uv_texture),
                    true,
                )
            }
        };
        let image = ResidentImage::allocate(self, output, working)?;
        let pipeline = self.resident_pipeline.get_or_init(|| {
            let shader = self
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("resident media conversion"),
                    source: wgpu::ShaderSource::Wgsl(RESIDENT_MEDIA_SHADER.into()),
                });
            self.device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("resident media"),
                    layout: None,
                    module: &shader,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    cache: None,
                })
        });
        let values = [
            output_to_local[0][0] as f32,
            output_to_local[0][1] as f32,
            output_to_local[0][2] as f32,
            extent[0] as f32,
            output_to_local[1][0] as f32,
            output_to_local[1][1] as f32,
            output_to_local[1][2] as f32,
            extent[1] as f32,
        ];
        let mut bytes: Vec<u8> = values.into_iter().flat_map(f32::to_le_bytes).collect();
        bytes.extend(
            [
                u32::from(nv12),
                u32::from(working == WorkingSpace::LinearRec2020),
                u32::from(srgb_transfer),
                0,
            ]
            .into_iter()
            .flat_map(u32::to_le_bytes),
        );
        let uniform = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("resident media mapping"),
                contents: &bytes,
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let views = [
            first.create_view(&Default::default()),
            second.create_view(&Default::default()),
            image.texture().create_view(&Default::default()),
        ];
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("resident media"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&views[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&views[1]),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&views[2]),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: uniform.as_entire_binding(),
                },
            ],
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(output[0].div_ceil(8), output[1].div_ceil(8), 1);
        }
        self.queue.submit([encoder.finish()]);
        stats.gpu_compute_dispatches += 1;
        stats.cpu_upload_control_bytes += bytes.len() as u64;
        stats.cpu_upload_control_operations += 1;
        Ok(image)
    }
}
