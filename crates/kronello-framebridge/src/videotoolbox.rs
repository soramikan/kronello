//! CoreVideo / VideoToolbox M0 probes, isolated from the pure model.
//! Imported textures retain their pixel buffer, CVMetalTexture and cache until HAL drop.
use crate::{Measurement, PathKind};
use kronello_gpu::{GpuContext, TransferStats};
use objc2_core_foundation::{
    CFBoolean, CFDictionary, CFNumber, CFRetained, CFString, CFType, Type,
};
use objc2_core_media::*;
use objc2_core_video::*;
use objc2_metal::{MTLPixelFormat, MTLTextureType};
use objc2_video_toolbox::*;
use std::{
    ffi::c_void,
    fmt,
    ptr::{self, NonNull},
    sync::Mutex,
    time::Instant,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeStage {
    PixelBufferCreate,
    PixelBufferWrite,
    TextureCacheCreate,
    EncoderCreate,
    EncoderConfigure,
    Encode,
    EncoderComplete,
    FormatDescription,
    DecoderCreate,
    Decode,
    DecoderWait,
    HardwareDecoderQuery,
    IoSurfaceVerify,
    Import,
    Sample,
    Readback,
    PixelCompare,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeError {
    pub stage: NativeStage,
    pub status: Option<i32>,
    pub detail: String,
}
impl fmt::Display for NativeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "UNSUPPORTED_FEATURE: stage={:?}, OSStatus={:?}, {}",
            self.stage, self.status, self.detail
        )
    }
}
impl std::error::Error for NativeError {}
fn error(stage: NativeStage, detail: impl Into<String>) -> NativeError {
    NativeError {
        stage,
        status: None,
        detail: detail.into(),
    }
}
fn status(stage: NativeStage, code: i32, detail: &str) -> Result<(), NativeError> {
    if code == 0 {
        Ok(())
    } else {
        Err(NativeError {
            stage,
            status: Some(code),
            detail: detail.into(),
        })
    }
}
fn owned<T: objc2_core_foundation::Type>(
    pointer: *mut T,
    stage: NativeStage,
) -> Result<CFRetained<T>, NativeError> {
    let pointer = NonNull::new(pointer)
        .ok_or_else(|| error(stage, "native API returned null with status 0"))?;
    // SAFETY: Only Create/Copy out-pointers (+1 ownership) reach this helper.
    Ok(unsafe { CFRetained::from_raw(pointer) })
}
fn attrs(format: u32) -> CFRetained<CFDictionary> {
    let number = CFNumber::new_i64(i64::from(format));
    let empty = CFDictionary::<CFType, CFType>::empty();
    // SAFETY: CoreVideo keys are process-lifetime constants. The IOSurface value
    // must be a dictionary, Metal compatibility a boolean, format a CFNumber.
    unsafe {
        CFDictionary::<CFString, CFType>::from_slices(
            &[
                kCVPixelBufferIOSurfacePropertiesKey,
                kCVPixelBufferMetalCompatibilityKey,
                kCVPixelBufferPixelFormatTypeKey,
            ],
            &[
                (*empty).as_ref(),
                CFBoolean::new(true).as_ref(),
                (*number).as_ref(),
            ],
        )
        .as_opaque()
        .retain()
    }
}
fn make_buffer(width: usize, height: usize) -> Result<CFRetained<CVPixelBuffer>, NativeError> {
    let attributes = attrs(kCVPixelFormatType_32BGRA);
    let mut raw = ptr::null_mut();
    // SAFETY: Fixed positive sizes, valid typed attributes, writable out-pointer.
    status(
        NativeStage::PixelBufferCreate,
        unsafe {
            CVPixelBufferCreate(
                None,
                width,
                height,
                kCVPixelFormatType_32BGRA,
                Some(&attributes),
                NonNull::from(&mut raw),
            )
        },
        "CVPixelBufferCreate BGRA8",
    )?;
    let buffer = owned(raw, NativeStage::PixelBufferCreate)?;
    verify_surface(&buffer)?;
    Ok(buffer)
}
fn verify_surface(buffer: &CVPixelBuffer) -> Result<(), NativeError> {
    if CVPixelBufferGetIOSurface(Some(buffer)).is_none() {
        return Err(error(
            NativeStage::IoSurfaceVerify,
            "CVPixelBufferGetIOSurface returned nil",
        ));
    }
    Ok(())
}
fn seed(buffer: &CVPixelBuffer, pattern: bool) -> Result<Vec<u8>, NativeError> {
    let (w, h) = (
        CVPixelBufferGetWidth(buffer),
        CVPixelBufferGetHeight(buffer),
    );
    let row = CVPixelBufferGetBytesPerRow(buffer);
    if CVPixelBufferGetPixelFormatType(buffer) != kCVPixelFormatType_32BGRA || row < w * 4 {
        return Err(error(
            NativeStage::PixelBufferWrite,
            "unexpected BGRA8 buffer layout",
        ));
    }
    // SAFETY: Buffer is private and has no submitted encode/GPU users. Lock
    // covers writes; checked dimensions/stride bound every access.
    unsafe {
        status(
            NativeStage::PixelBufferWrite,
            CVPixelBufferLockBaseAddress(buffer, CVPixelBufferLockFlags::empty()),
            "lock for CPU seed",
        )?;
    }
    let base = CVPixelBufferGetBaseAddress(buffer).cast::<u8>();
    let mut expected = Vec::with_capacity(w * h * 4);
    let result = if base.is_null() {
        Err(error(NativeStage::PixelBufferWrite, "null base address"))
    } else {
        for y in 0..h {
            for x in 0..w {
                let p = if pattern {
                    let gray =
                        [32, 96, 160, 224][usize::from(x >= w / 2) + 2 * usize::from(y >= h / 2)];
                    [gray, gray, gray, 255]
                } else {
                    [
                        [64, 128, 192, 255],
                        [1, 2, 3, 255],
                        [200, 100, 50, 255],
                        [255, 0, 128, 255],
                    ][(y * w + x) % 4]
                };
                // SAFETY: BGRA8 writes stay within the locked valid row width.
                unsafe {
                    ptr::copy_nonoverlapping(p.as_ptr(), base.add(y * row + x * 4), 4);
                }
                expected.extend([p[2], p[1], p[0], p[3]]);
            }
        }
        Ok(expected)
    };
    // SAFETY: The matching lock above succeeded; unlock on every exit path.
    let code = unsafe { CVPixelBufferUnlockBaseAddress(buffer, CVPixelBufferLockFlags::empty()) };
    status(NativeStage::PixelBufferWrite, code, "unlock CPU seed")?;
    result
}
// Tokens allow retain/release across callback/HAL threads, not concurrent pixel
// access. Pixel and sample buffers are immutable while asynchronous work uses them.
struct PixelToken(CFRetained<CVPixelBuffer>);
struct SampleToken(CFRetained<CMSampleBuffer>);
// SAFETY: CF retain/release are thread-safe; no mutation is exposed by tokens.
unsafe impl Send for PixelToken {}
// SAFETY: Samples come from completed encoder callbacks and remain immutable;
// moving the token only transfers a retained handle, not mutable sample access.
unsafe impl Send for SampleToken {}
struct TextureLifetime {
    pixel: PixelToken,
    texture: CFRetained<CVMetalTexture>,
    cache: CFRetained<CVMetalTextureCache>,
}
// SAFETY: A HAL drop callback can only release these retained handles. It exposes
// no buffer/cache operations; all CPU access ends before import/submission.
unsafe impl Send for TextureLifetime {}
// SAFETY: Shared access exposes no native operations. The one-shot HAL callback
// owns the token and only releases its retained handles after GPU use ends.
unsafe impl Sync for TextureLifetime {}
impl TextureLifetime {
    fn release(self) {
        let Self {
            pixel,
            texture,
            cache,
        } = self;
        drop(texture);
        drop(pixel);
        drop(cache);
    }
}
fn cache(gpu: &GpuContext) -> Result<CFRetained<CVMetalTextureCache>, NativeError> {
    // SAFETY: Borrow the exact wgpu Metal device without external GPU submission.
    let hal = unsafe { gpu.device.as_hal::<wgpu::hal::api::Metal>() }
        .ok_or_else(|| error(NativeStage::TextureCacheCreate, "wgpu device is not Metal"))?;
    let mut raw = ptr::null_mut();
    // SAFETY: Device stays borrowed during Create; output is writable and retained.
    status(
        NativeStage::TextureCacheCreate,
        unsafe {
            CVMetalTextureCache::create(None, None, hal.raw_device(), None, NonNull::from(&mut raw))
        },
        "CVMetalTextureCacheCreate on same MTLDevice",
    )?;
    owned(raw, NativeStage::TextureCacheCreate)
}
fn import_plane(
    gpu: &GpuContext,
    buffer: &CFRetained<CVPixelBuffer>,
    cache: &CFRetained<CVMetalTextureCache>,
    plane: usize,
    nv12: bool,
) -> Result<wgpu::Texture, NativeError> {
    verify_surface(buffer)?;
    let (width, height, metal, format) = if nv12 {
        if plane > 1 || CVPixelBufferGetPlaneCount(buffer) != 2 {
            return Err(error(NativeStage::Import, "NV12 must have two planes"));
        }
        (
            CVPixelBufferGetWidthOfPlane(buffer, plane),
            CVPixelBufferGetHeightOfPlane(buffer, plane),
            if plane == 0 {
                MTLPixelFormat::R8Unorm
            } else {
                MTLPixelFormat::RG8Unorm
            },
            if plane == 0 {
                wgpu::TextureFormat::R8Unorm
            } else {
                wgpu::TextureFormat::Rg8Unorm
            },
        )
    } else {
        if CVPixelBufferGetPixelFormatType(buffer) != kCVPixelFormatType_32BGRA {
            return Err(error(
                NativeStage::Import,
                format!(
                    "BGRA requested, actual format={:#x}",
                    CVPixelBufferGetPixelFormatType(buffer)
                ),
            ));
        }
        (
            CVPixelBufferGetWidth(buffer),
            CVPixelBufferGetHeight(buffer),
            MTLPixelFormat::BGRA8Unorm,
            wgpu::TextureFormat::Bgra8Unorm,
        )
    };
    let mut raw = ptr::null_mut();
    // SAFETY: Format/dimensions/plane match the retained buffer; same-device cache.
    status(
        NativeStage::Import,
        unsafe {
            CVMetalTextureCache::create_texture_from_image(
                None,
                cache,
                buffer,
                None,
                metal,
                width,
                height,
                plane,
                NonNull::from(&mut raw),
            )
        },
        "CVMetalTextureCacheCreateTextureFromImage",
    )?;
    let texture = owned(raw, NativeStage::Import)?;
    let native = CVMetalTextureGetTexture(&texture)
        .ok_or_else(|| error(NativeStage::Import, "CVMetalTextureGetTexture returned nil"))?;
    let lifetime = TextureLifetime {
        pixel: PixelToken(buffer.clone()),
        texture,
        cache: cache.clone(),
    };
    let extent = wgpu::Extent3d {
        width: u32::try_from(width).map_err(|_| error(NativeStage::Import, "width overflow"))?,
        height: u32::try_from(height).map_err(|_| error(NativeStage::Import, "height overflow"))?,
        depth_or_array_layers: 1,
    };
    // SAFETY: Cache uses this wgpu device. Native texture is initialized by CPU
    // unlock or completed VT callback; descriptor/format/plane dimensions agree.
    // The callback retains CVPixelBuffer + CVMetalTexture + cache until HAL drop.
    let hal = unsafe {
        wgpu::hal::metal::Device::texture_from_raw(
            native,
            format,
            MTLTextureType::Type2D,
            1,
            1,
            extent.into(),
            Some(Box::new(move || lifetime.release())),
        )
    };
    let descriptor = wgpu::TextureDescriptor {
        label: Some("CVPixelBuffer import"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    };
    // SAFETY: Texture and descriptor satisfy the same-device HAL import contract.
    Ok(unsafe {
        gpu.device.create_texture_from_hal::<wgpu::hal::api::Metal>(
            hal,
            &descriptor,
            wgpu::TextureUses::RESOURCE,
        )
    })
}
fn sample(
    gpu: &GpuContext,
    first: &wgpu::Texture,
    second: Option<&wgpu::Texture>,
    stats: &mut TransferStats,
) -> Result<Vec<u8>, NativeError> {
    let shader = if second.is_some() {
        "@group(0) @binding(0) var y:texture_2d<f32>; @group(0) @binding(1) var uv:texture_2d<f32>; @group(0) @binding(2) var o:texture_storage_2d<rgba8unorm,write>; @compute @workgroup_size(8,8) fn main(@builtin(global_invocation_id) i:vec3<u32>) {if any(i.xy>=textureDimensions(o)){return;} let chroma=textureLoad(uv,vec2<i32>(i.xy/2u),0).rg; textureStore(o,vec2<i32>(i.xy),vec4<f32>(textureLoad(y,vec2<i32>(i.xy),0).r,chroma,1.0));}"
    } else {
        "@group(0) @binding(0) var s:texture_2d<f32>; @group(0) @binding(1) var o:texture_storage_2d<rgba8unorm,write>; @compute @workgroup_size(8,8) fn main(@builtin(global_invocation_id) i:vec3<u32>) {if any(i.xy>=textureDimensions(o)){return;} textureStore(o,vec2<i32>(i.xy),textureLoad(s,vec2<i32>(i.xy),0));}"
    };
    let module = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("CV sampling"),
            source: wgpu::ShaderSource::Wgsl(shader.into()),
        });
    let pipeline = gpu
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("CV sampling"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
    let output = gpu
        .texture(
            first.width(),
            first.height(),
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        )
        .map_err(|e| error(NativeStage::Sample, e.to_string()))?;
    let input = first.create_view(&Default::default());
    let other = second.map(|t| t.create_view(&Default::default()));
    let out = output.create_view(&Default::default());
    let mut entries = vec![wgpu::BindGroupEntry {
        binding: 0,
        resource: wgpu::BindingResource::TextureView(&input),
    }];
    if let Some(ref view) = other {
        entries.push(wgpu::BindGroupEntry {
            binding: 1,
            resource: wgpu::BindingResource::TextureView(view),
        });
    }
    entries.push(wgpu::BindGroupEntry {
        binding: if second.is_some() { 2 } else { 1 },
        resource: wgpu::BindingResource::TextureView(&out),
    });
    let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("CV sampling"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &entries,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(first.width().div_ceil(8), first.height().div_ceil(8), 1);
    }
    gpu.queue.submit([encoder.finish()]);
    gpu.read_texture(&output, 4, stats)
        .map_err(|e| error(NativeStage::Readback, e.to_string()))
}
/// Stage A, exact byte comparison. CPU seed is preparation, not an import upload.
pub fn probe_cvpixelbuffer_import(gpu: &GpuContext) -> Result<Measurement, NativeError> {
    let start = Instant::now();
    let buffer = make_buffer(2, 2)?;
    let expected = seed(&buffer, false)?;
    let cache = cache(gpu)?;
    let texture = import_plane(gpu, &buffer, &cache, 0, false)?;
    // Drop caller references before submission to exercise the HAL lifetime token.
    drop(buffer);
    drop(cache);
    let mut transfers = TransferStats::default();
    let actual = sample(gpu, &texture, None, &mut transfers)?;
    if actual != expected {
        return Err(error(
            NativeStage::PixelCompare,
            format!("expected={expected:?}, actual={actual:?}"),
        ));
    }
    Ok(Measurement {path:PathKind::CvPixelBufferImport,elapsed:start.elapsed(),transfers,detail:"2x2 BGRA8, IOSurface-backed CVPixelBuffer -> same-device CVMetalTextureCache -> HAL -> shader; exact pixels; CPU seed=16B excluded; validation readback=512B; CV handles retained by HAL drop token".into()})
}

#[derive(Default)]
struct Encoded {
    frames: Mutex<Vec<(i32, Option<SampleToken>)>>,
}
#[derive(Default)]
struct Decoded {
    frames: Mutex<Vec<(i32, Option<PixelToken>)>>,
}
unsafe extern "C-unwind" fn encoded_callback(
    context: *mut c_void,
    _frame: *mut c_void,
    code: i32,
    _flags: VTEncodeInfoFlags,
    sample: *mut CMSampleBuffer,
) {
    // SAFETY: Session guard keeps Box<Encoded> alive until invalidate completes;
    // callback-owned sample is retained before returning, mutex serializes access.
    let state = unsafe { &*context.cast::<Encoded>() };
    // SAFETY: A non-null callback sample is valid here; retain extends its life
    // past callback return, and SampleToken owns the matching release.
    let sample = NonNull::new(sample).map(|p| SampleToken(unsafe { CFRetained::retain(p) }));
    state
        .frames
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push((code, sample));
}
unsafe extern "C-unwind" fn decoded_callback(
    context: *mut c_void,
    _frame: *mut c_void,
    code: i32,
    _flags: VTDecodeInfoFlags,
    image: *mut CVImageBuffer,
    _pts: CMTime,
    _duration: CMTime,
) {
    // SAFETY: Decoder guard owns context through wait/invalidate; image is a
    // CVPixelBuffer from VT and is retained before the output callback returns.
    let state = unsafe { &*context.cast::<Decoded>() };
    // SAFETY: A non-null decoded image is valid during this callback. Retain
    // gives PixelToken ownership through subsequent import and GPU completion.
    let image = NonNull::new(image).map(|p| PixelToken(unsafe { CFRetained::retain(p) }));
    state
        .frames
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push((code, image));
}
struct Encoder {
    session: CFRetained<VTCompressionSession>,
    state: Box<Encoded>,
    inputs: Vec<PixelToken>,
}
impl Drop for Encoder {
    fn drop(&mut self) {
        // SAFETY: Keep callback state and input buffers alive until invalidation has
        // canceled/completed pending work; no callback may use state afterwards.
        unsafe {
            VTCompressionSession::invalidate(&self.session);
        }
    }
}
struct Decoder {
    session: CFRetained<VTDecompressionSession>,
    state: Box<Decoded>,
}
impl Drop for Decoder {
    fn drop(&mut self) {
        // SAFETY: Wait and invalidate while callback state is still alive, even when
        // the probe exits early due to an OSStatus error.
        unsafe {
            VTDecompressionSession::wait_for_asynchronous_frames(&self.session);
            VTDecompressionSession::invalidate(&self.session);
        }
    }
}
fn encode() -> Result<(Vec<SampleToken>, Vec<u8>), NativeError> {
    let mut state = Box::<Encoded>::default();
    let attributes = attrs(kCVPixelFormatType_32BGRA);
    let mut raw = ptr::null_mut();
    // SAFETY: Stable Box callback context, typed BGRA input, positive 64x64 size.
    status(
        NativeStage::EncoderCreate,
        unsafe {
            VTCompressionSession::create(
                None,
                64,
                64,
                kCMVideoCodecType_H264,
                None,
                Some(&attributes),
                None,
                Some(encoded_callback),
                (&mut *state as *mut Encoded).cast(),
                NonNull::from(&mut raw),
            )
        },
        "VTCompressionSessionCreate H.264",
    )?;
    let mut encoder = Encoder {
        session: owned(raw, NativeStage::EncoderCreate)?,
        state,
        inputs: Vec::new(),
    };
    // SAFETY: Supported typed properties. Failures are reported, never ignored.
    unsafe {
        for (key, value) in [
            (
                kVTCompressionPropertyKey_AllowFrameReordering,
                CFBoolean::new(false).as_ref(),
            ),
            (
                kVTCompressionPropertyKey_RealTime,
                CFBoolean::new(true).as_ref(),
            ),
        ] {
            status(
                NativeStage::EncoderConfigure,
                VTSessionSetProperty((*encoder.session).as_ref(), key, Some(value)),
                "encoder boolean property",
            )?;
        }
        status(
            NativeStage::EncoderConfigure,
            VTSessionSetProperty(
                (*encoder.session).as_ref(),
                kVTCompressionPropertyKey_Quality,
                Some((*CFNumber::new_f64(1.0)).as_ref()),
            ),
            "encoder Quality=1",
        )?;
    }
    let mut expected = Vec::new();
    for index in 0..3 {
        let buffer = make_buffer(64, 64)?;
        expected = seed(&buffer, true)?;
        encoder.inputs.push(PixelToken(buffer));
        // SAFETY: Rational timestamps are monotonic; retain all submitted inputs
        // until CompleteFrames/invalidate, and source refcon is intentionally null.
        status(
            NativeStage::Encode,
            unsafe {
                VTCompressionSession::encode_frame(
                    &encoder.session,
                    &encoder.inputs[index].0,
                    CMTime::new(index as i64, 30),
                    CMTime::new(1, 30),
                    None,
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            },
            "VTCompressionSessionEncodeFrame",
        )?;
    }
    // SAFETY: Invalid time requests completion of all submitted frames.
    status(
        NativeStage::EncoderComplete,
        unsafe { VTCompressionSession::complete_frames(&encoder.session, kCMTimeInvalid) },
        "VTCompressionSessionCompleteFrames",
    )?;
    let results = std::mem::take(
        &mut *encoder
            .state
            .frames
            .lock()
            .unwrap_or_else(|e| e.into_inner()),
    );
    let mut frames = Vec::new();
    for (code, sample) in results {
        status(NativeStage::Encode, code, "compression output callback")?;
        frames.push(sample.ok_or_else(|| error(NativeStage::Encode, "null CMSampleBuffer"))?);
    }
    if frames.len() != 3 {
        return Err(error(
            NativeStage::Encode,
            format!("expected 3 encoded frames, got {}", frames.len()),
        ));
    }
    Ok((frames, expected))
}
fn decode(
    samples: &[SampleToken],
    nv12: bool,
) -> Result<(Vec<PixelToken>, Option<bool>, i32), NativeError> {
    let first = samples
        .first()
        .ok_or_else(|| error(NativeStage::FormatDescription, "no sample"))?;
    // SAFETY: Retained valid compressed sample from completed encoder.
    let format = unsafe { first.0.format_description() }.ok_or_else(|| {
        error(
            NativeStage::FormatDescription,
            "sample has no format description",
        )
    })?;
    let pixel_format = if nv12 {
        kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
    } else {
        kCVPixelFormatType_32BGRA
    };
    let attributes = attrs(pixel_format);
    let mut state = Box::<Decoded>::default();
    let mut raw = ptr::null_mut();
    let callback = VTDecompressionOutputCallbackRecord {
        decompressionOutputCallback: Some(decoded_callback),
        decompressionOutputRefCon: (&mut *state as *mut Decoded).cast(),
    };
    // SAFETY: Callback/context stable until session invalidation; format comes
    // from encoder sample; output attributes specify IOSurface/Metal compatibility.
    status(
        NativeStage::DecoderCreate,
        unsafe {
            VTDecompressionSession::create(
                None,
                &format,
                None,
                Some(&attributes),
                &callback,
                NonNull::from(&mut raw),
            )
        },
        "VTDecompressionSessionCreate",
    )?;
    let decoder = Decoder {
        session: owned(raw, NativeStage::DecoderCreate)?,
        state,
    };
    for sample in samples {
        // SAFETY: Samples retained for all asynchronous decoder work; no frame
        // refcon, and callback records every status and retained pixel buffer.
        status(
            NativeStage::Decode,
            unsafe {
                VTDecompressionSession::decode_frame(
                    &decoder.session,
                    &sample.0,
                    VTDecodeFrameFlags::empty(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            },
            "VTDecompressionSessionDecodeFrame",
        )?;
    }
    // SAFETY: Flush and wait before inspecting retained callback outputs.
    status(
        NativeStage::DecoderWait,
        unsafe { VTDecompressionSession::finish_delayed_frames(&decoder.session) },
        "finish delayed frames",
    )?;
    // SAFETY: The decoder guard still owns the live session and callback context;
    // all compressed samples remain retained until pending output has completed.
    status(
        NativeStage::DecoderWait,
        unsafe { VTDecompressionSession::wait_for_asynchronous_frames(&decoder.session) },
        "wait asynchronous frames",
    )?;
    let mut property: *mut CFType = ptr::null_mut();
    // SAFETY: Correct VT session/property; Copy writes a +1 CFType to output.
    let query = unsafe {
        VTSessionCopyProperty(
            (*decoder.session).as_ref(),
            kVTDecompressionPropertyKey_UsingHardwareAcceleratedVideoDecoder,
            None,
            (&mut property as *mut *mut CFType).cast(),
        )
    };
    let hardware = if query == 0 {
        let value = owned(property, NativeStage::HardwareDecoderQuery)?;
        Some(
            value
                .downcast_ref::<CFBoolean>()
                .ok_or_else(|| {
                    error(
                        NativeStage::HardwareDecoderQuery,
                        "hardware property is not CFBoolean",
                    )
                })?
                .as_bool(),
        )
    } else {
        None
    };
    let results = std::mem::take(
        &mut *decoder
            .state
            .frames
            .lock()
            .unwrap_or_else(|e| e.into_inner()),
    );
    let mut frames = Vec::new();
    for (code, image) in results {
        status(NativeStage::Decode, code, "decompression output callback")?;
        let image =
            image.ok_or_else(|| error(NativeStage::Decode, "null decoded CVPixelBuffer"))?;
        verify_surface(&image.0)?;
        frames.push(image);
    }
    if frames.len() != samples.len() {
        return Err(error(
            NativeStage::Decode,
            format!(
                "expected {} decoded frames, got {}",
                samples.len(),
                frames.len()
            ),
        ));
    }
    Ok((frames, hardware, query))
}
fn verify_nv12(buffer: &CVPixelBuffer, actual: &[u8]) -> Result<(), NativeError> {
    let w = CVPixelBufferGetWidth(buffer);
    let h = CVPixelBufferGetHeight(buffer);
    if w != 64
        || h != 64
        || CVPixelBufferGetPlaneCount(buffer) != 2
        || CVPixelBufferGetWidthOfPlane(buffer, 0) != w
        || CVPixelBufferGetHeightOfPlane(buffer, 0) != h
        || CVPixelBufferGetWidthOfPlane(buffer, 1) != w / 2
        || CVPixelBufferGetHeightOfPlane(buffer, 1) != h / 2
    {
        return Err(error(
            NativeStage::PixelCompare,
            "unexpected NV12 plane dimensions",
        ));
    }
    // SAFETY: Decoder has completed, no CPU/GPU writers. Read lock covers both
    // planes; explicit dimensions and strides are checked before pointer access.
    unsafe {
        status(
            NativeStage::PixelCompare,
            CVPixelBufferLockBaseAddress(buffer, CVPixelBufferLockFlags::ReadOnly),
            "NV12 verification lock",
        )?;
    }
    let y = CVPixelBufferGetBaseAddressOfPlane(buffer, 0).cast::<u8>();
    let uv = CVPixelBufferGetBaseAddressOfPlane(buffer, 1).cast::<u8>();
    let yr = CVPixelBufferGetBytesPerRowOfPlane(buffer, 0);
    let ur = CVPixelBufferGetBytesPerRowOfPlane(buffer, 1);
    let result = if y.is_null() || uv.is_null() || yr < w || ur < w || actual.len() != w * h * 4 {
        Err(error(
            NativeStage::PixelCompare,
            "invalid NV12 verification layout",
        ))
    } else {
        let mut matches = true;
        for row in 0..h {
            for col in 0..w {
                // SAFETY: Valid 64x64 NV12 planes, row widths/lengths checked above.
                let expected = unsafe {
                    [
                        *y.add(row * yr + col),
                        *uv.add((row / 2) * ur + (col / 2) * 2),
                        *uv.add((row / 2) * ur + (col / 2) * 2 + 1),
                        255,
                    ]
                };
                matches &= actual[(row * w + col) * 4..(row * w + col + 1) * 4] == expected;
            }
        }
        if matches {
            Ok(())
        } else {
            Err(error(
                NativeStage::PixelCompare,
                "NV12 shader plane bytes differ from locked decoder output",
            ))
        }
    };
    // SAFETY: Matched successful ReadOnly lock, on both success and failure.
    status(
        NativeStage::PixelCompare,
        unsafe { CVPixelBufferUnlockBaseAddress(buffer, CVPixelBufferLockFlags::ReadOnly) },
        "NV12 verification unlock",
    )?;
    result
}
fn import_decoded(
    gpu: &GpuContext,
    samples: &[SampleToken],
    expected: &[u8],
    nv12: bool,
) -> Result<Measurement, NativeError> {
    let start = Instant::now();
    let (frames, hardware, query) = decode(samples, nv12)?;
    let cache = cache(gpu)?;
    let mut transfers = TransferStats::default();
    let mut max_error = 0u8;
    for frame in &frames {
        let format = CVPixelBufferGetPixelFormatType(&frame.0);
        if nv12
            && format != kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
            && format != kCVPixelFormatType_420YpCbCr8BiPlanarFullRange
        {
            return Err(error(
                NativeStage::Import,
                format!("NV12 requested, actual format={format:#x}"),
            ));
        }
        let first = import_plane(gpu, &frame.0, &cache, 0, nv12)?;
        let second = if nv12 {
            Some(import_plane(gpu, &frame.0, &cache, 1, true)?)
        } else {
            None
        };
        let actual = sample(gpu, &first, second.as_ref(), &mut transfers)?;
        if nv12 {
            verify_nv12(&frame.0, &actual)?;
        } else {
            if actual.len() != expected.len() {
                return Err(error(
                    NativeStage::PixelCompare,
                    "decoded frame size mismatch",
                ));
            }
            max_error = max_error.max(
                actual
                    .iter()
                    .zip(expected)
                    .map(|(a, b)| a.abs_diff(*b))
                    .max()
                    .unwrap_or(0),
            );
            if max_error > 2 {
                return Err(error(
                    NativeStage::PixelCompare,
                    format!(
                        "lossy BGRA max channel error {max_error} > 2; hardware_decoder={hardware:?}, query_OSStatus={query}"
                    ),
                ));
            }
        }
    }
    Ok(Measurement {
        path: if nv12 {
            PathKind::VideoToolboxDecodeNv12Biplanar
        } else {
            PathKind::VideoToolboxDecodeBgra8
        },
        elapsed: start.elapsed(),
        transfers,
        detail: format!(
            "3 H.264 64x64 quadrant grayscale frames encoded in memory; decoded_frames={}; hardware_decoder={hardware:?}; hardware_query_OSStatus={query}; IOSurface verified; same-device CV cache/HAL; max_BGRA_error={}; NV12 only validates R8/RG8 bytes, no YCbCr->RGB; CPU seeds and codec internal conversion excluded from transfer stats; elapsed covers decode/import/validation only",
            frames.len(),
            if nv12 {
                "not compared".to_owned()
            } else {
                max_error.to_string()
            }
        ),
    })
}
/// Stage B. BGRA failure is reported before an explicit NV12 attempt. A successful
/// NV12 attempt is never reported as successful BGRA color comparison.
pub fn probe_videotoolbox_decode(
    gpu: &GpuContext,
    nv12_only: bool,
) -> Result<Measurement, NativeError> {
    let (samples, expected) = encode()?;
    if nv12_only {
        return import_decoded(gpu, &samples, &expected, true);
    }
    match import_decoded(gpu, &samples, &expected, false) {
        Ok(report) => Ok(report),
        Err(bgra) if bgra.stage != NativeStage::PixelCompare => {
            eprintln!("BGRA attempt failed: {bgra}; attempting NV12 biplanar");
            match import_decoded(gpu, &samples, &expected, true) {
                Ok(mut report) => {
                    report.detail = format!(
                        "BGRA unsupported: {bgra}; explicit NV12 alternative: {}",
                        report.detail
                    );
                    Ok(report)
                }
                Err(mut nv12) => {
                    nv12.detail = format!("BGRA attempt: {bgra}; NV12 attempt: {}", nv12.detail);
                    Err(nv12)
                }
            }
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_error_preserves_stage_and_osstatus() {
        let failure = status(NativeStage::DecoderCreate, -12906, "decoder create").unwrap_err();
        assert_eq!(failure.stage, NativeStage::DecoderCreate);
        assert_eq!(failure.status, Some(-12906));
        assert!(failure.to_string().contains("UNSUPPORTED_FEATURE"));
        assert!(failure.to_string().contains("DecoderCreate"));
        assert!(failure.to_string().contains("-12906"));
        assert_eq!(error(NativeStage::Import, "nil texture").status, None);
    }
}
