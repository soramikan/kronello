//! BGRA8 single-plane IOSurface experiment. Does not expose arbitrary native handles.
use super::{Measurement, PathKind};
use kronello_gpu::{GpuContext, GpuError, TransferStats};
use objc2_core_foundation::{CFDictionary, CFNumber};
use objc2_io_surface::{
    IOSurfaceLockOptions, IOSurfaceRef, kIOSurfaceBytesPerElement, kIOSurfaceHeight,
    kIOSurfacePixelFormat, kIOSurfaceWidth,
};
use objc2_metal::{
    MTLDevice, MTLPixelFormat, MTLStorageMode, MTLTextureDescriptor, MTLTextureType,
    MTLTextureUsage,
};
use std::time::Instant;

// A lifetime token only: it exposes no surface access, only thread-safe CFRelease.
struct SurfaceLifetime(objc2_core_foundation::CFRetained<IOSurfaceRef>);
// SAFETY: IOSurface retain/release are thread-safe. Pixel access remains private
// to measure(), under lock and a GPU completion fence; this token cannot access it.
unsafe impl Send for SurfaceLifetime {}
unsafe impl Sync for SurfaceLifetime {}
impl SurfaceLifetime {
    fn release(self) {
        drop(self.0);
    }
}

pub(super) fn measure(gpu: &GpuContext, path: PathKind) -> Result<Measurement, GpuError> {
    let start = Instant::now();
    // SAFETY: Framework constants have process lifetime; integer values match
    // IOSurface's documented dictionary types. Dictionary retains all values.
    let properties = unsafe {
        CFDictionary::from_slices(
            &[
                kIOSurfaceWidth,
                kIOSurfaceHeight,
                kIOSurfaceBytesPerElement,
                kIOSurfacePixelFormat,
            ],
            &[
                &*CFNumber::new_i32(2),
                &*CFNumber::new_i32(2),
                &*CFNumber::new_i32(4),
                &*CFNumber::new_i32(i32::from_be_bytes(*b"BGRA")),
            ],
        )
    };
    // SAFETY: The dictionary above contains only valid IOSurface numeric properties.
    let surface = unsafe { IOSurfaceRef::new(properties.as_opaque()) }.ok_or(
        GpuError::UnsupportedFeature("IOSurfaceCreate returned null"),
    )?;
    let allocation_elapsed = start.elapsed();
    let row = surface.bytes_per_row();
    if surface.width() != 2
        || surface.height() != 2
        || surface.bytes_per_element() != 4
        || row < 8
        || surface.alloc_size() < row * 2
    {
        return Err(GpuError::InvalidInput("unexpected IOSurface layout"));
    }
    let seed_pixel = [64u8, 128, 192, 255];
    // SAFETY: This surface is private to this function, no GPU work references it
    // yet. Null seed is allowed. Bounds are checked above. Lock covers all writes.
    unsafe {
        if surface.lock(IOSurfaceLockOptions::empty(), std::ptr::null_mut()) != 0 {
            return Err(GpuError::Readback("IOSurfaceLock write failed".into()));
        }
        let base = surface.base_address().as_ptr().cast::<u8>();
        for y in 0..2 {
            for x in 0..2 {
                std::ptr::copy_nonoverlapping(seed_pixel.as_ptr(), base.add(y * row + x * 4), 4);
            }
        }
        if surface.unlock(IOSurfaceLockOptions::empty(), std::ptr::null_mut()) != 0 {
            return Err(GpuError::Readback("IOSurfaceUnlock write failed".into()));
        }
    }
    let descriptor = MTLTextureDescriptor::new();
    descriptor.setTextureType(MTLTextureType::Type2D);
    descriptor.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
    // SAFETY: Fixed positive 2x2 size agrees with the single-plane surface.
    unsafe {
        descriptor.setWidth(2);
        descriptor.setHeight(2);
    }
    descriptor.setStorageMode(MTLStorageMode::Shared);
    descriptor.setUsage(MTLTextureUsage::ShaderRead | MTLTextureUsage::RenderTarget);
    let import_start = Instant::now();
    // SAFETY: We borrow the actual wgpu device; no raw device modification or
    // external GPU submissions occur. The guard is dropped before wgpu calls.
    let hal_device = unsafe { gpu.device.as_hal::<wgpu::hal::api::Metal>() }
        .ok_or(GpuError::UnsupportedFeature("wgpu device is not Metal"))?;
    let native = hal_device
        .raw_device()
        .newTextureWithDescriptor_iosurface_plane(&descriptor, &surface, 0)
        .ok_or(GpuError::UnsupportedFeature(
            "MTLTexture from IOSurface returned null",
        ))?;
    drop(hal_device);
    let extent = wgpu::Extent3d {
        width: 2,
        height: 2,
        depth_or_array_layers: 1,
    };
    // SAFETY: Native texture was created on this wgpu device, fully initialized
    // under IOSurfaceLock, BGRA8, 2D, one mip/layer/sample. Metal has no image
    // layout transition. The drop callback retains the surface until HAL release.
    let hal_texture = unsafe {
        wgpu::hal::metal::Device::texture_from_raw(
            native,
            wgpu::TextureFormat::Bgra8Unorm,
            MTLTextureType::Type2D,
            1,
            1,
            extent.into(),
            Some(Box::new({
                let retained = SurfaceLifetime(surface.clone());
                move || retained.release()
            })),
        )
    };
    let texture_descriptor = wgpu::TextureDescriptor {
        label: Some("IOSurface M0 import"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    };
    // SAFETY: Descriptor exactly matches the native texture; initialized contents
    // must be preserved. RESOURCE is the sampled state; Metal tracks hazards.
    let imported = unsafe {
        gpu.device.create_texture_from_hal::<wgpu::hal::api::Metal>(
            hal_texture,
            &texture_descriptor,
            wgpu::TextureUses::RESOURCE,
        )
    };
    let import_elapsed = import_start.elapsed();
    let mut transfers = TransferStats::default();
    if path == PathKind::IoSurfaceImport {
        // Read known BGRA through a wgpu compute shader into RGBA8.
        let shader=gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {label:Some("IOSurface sample"),source:wgpu::ShaderSource::Wgsl("@group(0) @binding(0) var s: texture_2d<f32>; @group(0) @binding(1) var o: texture_storage_2d<rgba8unorm,write>; @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) id:vec3<u32>) {textureStore(o,vec2<i32>(id.xy),textureLoad(s,vec2<i32>(id.xy),0));}".into())});
        let pipeline = gpu
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("IOSurface sample"),
                layout: None,
                module: &shader,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });
        let output = gpu.texture(
            2,
            2,
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        )?;
        let source_view = imported.create_view(&Default::default());
        let output_view = output.create_view(&Default::default());
        let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("IOSurface sample"),
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
            ],
        });
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(2, 2, 1);
        }
        gpu.queue.submit([encoder.finish()]);
        let actual = gpu.read_texture(&output, 4, &mut transfers)?;
        if actual != [192, 128, 64, 255].repeat(4) {
            return Err(GpuError::Readback(format!(
                "IOSurface shader pixel mismatch: {actual:?}"
            )));
        }
    } else {
        // GPU render directly into the imported IOSurface. The validation CPU
        // read below happens only after wgpu's completion fence.
        let view = imported.create_view(&Default::default());
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("IOSurface output"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 1.0,
                            g: 0.5,
                            b: 0.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
        }
        gpu.queue.submit([encoder.finish()]);
        gpu.wait()?;
        // SAFETY: Completion fence guarantees no GPU writers. Lock covers reads,
        // private surface has no aliases outside the retained HAL texture.
        unsafe {
            if surface.lock(IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut()) != 0 {
                return Err(GpuError::Readback("IOSurfaceLock read failed".into()));
            }
            let base = surface.base_address().as_ptr().cast::<u8>();
            let mut actual = Vec::new();
            for y in 0..2 {
                actual.extend_from_slice(std::slice::from_raw_parts(base.add(y * row), 8));
            }
            let unlocked = surface.unlock(IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut());
            if unlocked != 0 || actual != [0, 128, 255, 255].repeat(4) {
                return Err(GpuError::Readback(format!(
                    "IOSurface output mismatch: {actual:?}, unlock={unlocked}"
                )));
            }
        }
    }
    Ok(Measurement {
        path,
        elapsed: start.elapsed(),
        transfers,
        detail: format!(
            "2x2 BGRA8 single plane; IOSurfaceCreate={allocation_elapsed:?}; same-device MTLTexture+HAL import={import_elapsed:?}; CPU seed=16 bytes excluded from transfer counters; import/output adds no CPU copy; validation readback/lock explicitly separate; VideoToolbox unmeasured"
        ),
    })
}
