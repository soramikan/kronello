//! Guaranteed Metal/VideoToolbox decode, import, and GPU-only color/sampling.
use crate::videotoolbox::{self, NativeError, NativeStage, PixelToken, SampleToken};
use kronello_gpu::{GpuContext, ResidentImage, TransferStats, WorkingSpace};
use objc2_core_foundation::CFRetained;
use objc2_core_media::{CMSampleBuffer, CMTime};
use objc2_core_video::{CVPixelBufferGetHeight, CVPixelBufferGetWidth};
use std::{
    ffi::{CString, c_char, c_void},
    path::Path,
    ptr::NonNull,
};

unsafe extern "C" {
    fn kronello_fb_read_samples(
        path: *const c_char,
        stream: u32,
        time_num: i64,
        time_den: i64,
        canonical_origin: i32,
        samples: *mut *mut c_void,
        capacity: usize,
        count: *mut usize,
        error: *mut c_char,
        error_size: usize,
    ) -> i32;
    fn kronello_fb_read_samples_raw(
        path: *const c_char,
        stream: u32,
        time_num: i64,
        time_den: i64,
        canonical_origin: i32,
        samples: *mut *mut c_void,
        capacity: usize,
        count: *mut usize,
        error: *mut c_char,
        error_size: usize,
    ) -> i32;
}
fn error(stage: NativeStage, detail: impl Into<String>) -> NativeError {
    NativeError {
        stage,
        status: None,
        detail: detail.into(),
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResidentFormat {
    Bgra8,
    Nv12VideoRange,
    /// ProRes RAW decoder output: linear scene-referred 64-bit RGBA half.
    ProResRawRgbah,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoTransfer {
    Bt709,
    Srgb,
}

/// Native buffers own the decoder output after session invalidation. Texture HAL
/// ownership further retains buffers, cache and CVMetalTexture until GPU release.
pub struct HardwareFrame {
    buffer: PixelToken,
    pub format: ResidentFormat,
    pub presentation_time: CMTime,
}
impl HardwareFrame {
    pub fn size(&self) -> [u32; 2] {
        [
            CVPixelBufferGetWidth(&self.buffer.0) as u32,
            CVPixelBufferGetHeight(&self.buffer.0) as u32,
        ]
    }
    /// Completed callback output -> same MTLDevice import -> compute conversion.
    /// No CPU pixel lock, copy, upload or readback occurs here.
    // Independent spatial, color, device and accounting contracts are explicit.
    #[allow(clippy::too_many_arguments)]
    pub fn sample_to_working(
        &self,
        gpu: &GpuContext,
        output: [u32; 2],
        extent: [f64; 2],
        output_to_local: [[f64; 3]; 2],
        working: WorkingSpace,
        transfer: VideoTransfer,
        stats: &mut TransferStats,
    ) -> Result<ResidentImage, NativeError> {
        let _scope = gpu
            .observation_scope()
            .map_err(|e| error(NativeStage::Sample, e.to_string()))?;
        if output.contains(&0)
            || extent
                .iter()
                .any(|v| !v.is_finite() || *v <= 0.0 || *v > 1e12)
            || output_to_local
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || v.abs() > 1e12)
        {
            return Err(error(
                NativeStage::Sample,
                "invalid resident sampling transform",
            ));
        }
        let cache = videotoolbox::cache(gpu)?;
        let nv12 = self.format == ResidentFormat::Nv12VideoRange;
        let first = videotoolbox::import_plane(gpu, &self.buffer.0, &cache, 0, nv12)?;
        let second = if nv12 {
            videotoolbox::import_plane(gpu, &self.buffer.0, &cache, 1, true)?
        } else {
            first.clone()
        };
        let image = ResidentImage::allocate(gpu, output, working)
            .map_err(|e| error(NativeStage::Sample, e.to_string()))?;
        let output_texture = image.texture();
        let shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("resident BT709 media conversion"),
                source: wgpu::ShaderSource::Wgsl(include_str!("resident.wgsl").into()),
            });
        let pipeline = gpu
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("resident media"),
                layout: None,
                module: &shader,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });
        use wgpu::util::DeviceExt;
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
                u32::from(transfer == VideoTransfer::Srgb),
                0,
            ]
            .into_iter()
            .flat_map(u32::to_le_bytes),
        );
        let uniform = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("resident media mapping"),
                contents: &bytes,
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let views = [
            first.create_view(&Default::default()),
            second.create_view(&Default::default()),
            output_texture.create_view(&Default::default()),
        ];
        let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
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
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(output[0].div_ceil(8), output[1].div_ceil(8), 1);
        }
        gpu.queue.submit([encoder.finish()]);
        stats.gpu_compute_dispatches += 1;
        stats.cpu_upload_control_bytes += bytes.len() as u64;
        stats.cpu_upload_control_operations += 1;
        // GPU shader writes are computation, not a texture copy or CPU transfer.
        Ok(image)
    }
}

fn decode_selected(
    samples: &[SampleToken],
    time: (i64, i64),
    format: ResidentFormat,
) -> Result<HardwareFrame, NativeError> {
    if time.1 <= 0 {
        return Err(error(NativeStage::Decode, "invalid rational time"));
    }
    let mode = match format {
        ResidentFormat::Bgra8 => videotoolbox::DecodeMode::SdrBgra8,
        ResidentFormat::Nv12VideoRange => videotoolbox::DecodeMode::SdrNv12,
        ResidentFormat::ProResRawRgbah => videotoolbox::DecodeMode::ProResRaw,
    };
    let (mut frames, hardware, _, timestamps) =
        videotoolbox::decode(samples, mode, true, Some(time))?;
    if hardware != Some(true) {
        return Err(error(
            NativeStage::HardwareDecoderQuery,
            "require_gpu_resident hardware property false",
        ));
    }
    let mut selected = None;
    for (index, pts) in timestamps.iter().enumerate() {
        if pts.timescale <= 0 {
            return Err(error(
                NativeStage::Decode,
                "invalid decoded presentation timestamp",
            ));
        }
        let lhs = i128::from(pts.value) * i128::from(time.1);
        let rhs = i128::from(time.0) * i128::from(pts.timescale);
        if lhs <= rhs
            && selected.is_none_or(|j: usize| {
                i128::from(pts.value) * i128::from(timestamps[j].timescale)
                    > i128::from(timestamps[j].value) * i128::from(pts.timescale)
            })
        {
            selected = Some(index);
        }
    }
    let selected = selected.ok_or_else(|| {
        error(
            NativeStage::Decode,
            "no frame at requested presentation time",
        )
    })?;
    Ok(HardwareFrame {
        buffer: frames.swap_remove(selected),
        format,
        presentation_time: timestamps[selected],
    })
}
/// Local regular-file compressed demux; strict hardware decode. Unsupported
/// codecs, stream selection, format or resource bounds return explicit errors.
/// Entire clip is bounded to 262144 compressed samples / 128 MiB; long clips require a
/// future streaming demux adapter and never silently select software decode.
pub fn decode_file(
    path: &Path,
    stream: u32,
    time: (i64, i64),
    format: ResidentFormat,
) -> Result<HardwareFrame, NativeError> {
    decode_file_inner(path, stream, time, format, None)
}
pub fn decode_file_with_interval(
    path: &Path,
    stream: u32,
    time: (i64, i64),
    format: ResidentFormat,
    interval: [(i64, i64); 2],
) -> Result<HardwareFrame, NativeError> {
    if time.1 <= 0
        || interval.iter().any(|(_, den)| *den <= 0)
        || i128::from(time.0) * i128::from(interval[0].1)
            < i128::from(interval[0].0) * i128::from(time.1)
        || i128::from(time.0) * i128::from(interval[1].1)
            >= i128::from(interval[1].0) * i128::from(time.1)
    {
        return Err(error(
            NativeStage::FrameSelection,
            "time outside canonical locked half-open stream interval",
        ));
    }
    decode_file_inner(path, stream, time, format, Some(interval[0]))
}
fn decode_file_inner(
    path: &Path,
    stream: u32,
    time: (i64, i64),
    format: ResidentFormat,
    origin: Option<(i64, i64)>,
) -> Result<HardwareFrame, NativeError> {
    let path = path
        .canonicalize()
        .map_err(|e| error(NativeStage::FormatDescription, e.to_string()))?;
    if !path.is_file() {
        return Err(error(
            NativeStage::FormatDescription,
            "local regular file required",
        ));
    }
    use std::os::unix::ffi::OsStrExt;
    let cpath = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| error(NativeStage::FormatDescription, "path NUL"))?;
    let mut pointers = vec![std::ptr::null_mut(); 262144];
    let mut count = 0;
    let mut diagnostic = [0u8; 512];
    let raw_mode = format == ResidentFormat::ProResRawRgbah;
    // SAFETY: Fixed-capacity initialized out-array, live NUL path and writable
    // diagnostic; native returns +1 CMSampleBuffer handles for count entries.
    let code = unsafe {
        (if raw_mode {
            kronello_fb_read_samples_raw
        } else {
            kronello_fb_read_samples
        })(
            cpath.as_ptr(),
            stream,
            time.0,
            time.1,
            i32::from(origin.is_some()),
            pointers.as_mut_ptr(),
            pointers.len(),
            &mut count,
            diagnostic.as_mut_ptr().cast(),
            diagnostic.len(),
        )
    };
    let mut samples = Vec::with_capacity(count);
    for pointer in pointers.into_iter().take(count) {
        let pointer = NonNull::new(pointer.cast::<CMSampleBuffer>())
            .ok_or_else(|| error(NativeStage::FormatDescription, "null compressed sample"))?;
        // SAFETY: copyNextSampleBuffer returned +1 ownership, released by token.
        samples.push(SampleToken(unsafe { CFRetained::from_raw(pointer) }));
    }
    if code != 0 {
        return Err(error(
            if code == -6 {
                NativeStage::FrameSelection
            } else {
                NativeStage::FormatDescription
            },
            String::from_utf8_lossy(&diagnostic)
                .trim_end_matches('\0')
                .to_string(),
        ));
    }
    if let Some((num, den)) = origin {
        let timescale = i32::try_from(den).map_err(|_| {
            error(
                NativeStage::FrameSelection,
                "canonical origin denominator exceeds native time range",
            )
        })?;
        if timescale <= 0 {
            return Err(error(
                NativeStage::FrameSelection,
                "canonical origin denominator must be positive",
            ));
        }
        let mut first = None;
        for sample in &samples {
            // SAFETY: Completed retained compressed sample.
            let pts = unsafe { sample.0.output_presentation_time_stamp() };
            if pts.timescale <= 0 {
                return Err(error(
                    NativeStage::FrameSelection,
                    "invalid compressed output PTS",
                ));
            }
            if first.is_none_or(|previous: CMTime| {
                i128::from(pts.value) * i128::from(previous.timescale)
                    < i128::from(previous.value) * i128::from(pts.timescale)
            }) {
                first = Some(pts);
            }
        }
        let first =
            first.ok_or_else(|| error(NativeStage::FormatDescription, "no compressed samples"))?;
        for sample in &samples {
            // SAFETY: Retained completed sample, only this demux owns timing changes.
            let pts = unsafe { sample.0.output_presentation_time_stamp() };
            let relative = exact_time_add(pts, first, -1)?;
            // SAFETY: Canonical origin has a strictly positive checked timescale.
            let canonical = exact_time_add(relative, unsafe { CMTime::new(num, timescale) }, 1)?;
            // SAFETY: Exact finite presentation time and retained private sample.
            let status = unsafe { sample.0.set_output_presentation_time_stamp(canonical) };
            if status != 0 {
                return Err(error(
                    NativeStage::FrameSelection,
                    format!("canonical output PTS OSStatus={status}"),
                ));
            }
        }
    }
    if std::env::var_os("KRONELLO_GPU003_DIAGNOSTICS").is_some() {
        for sample in &samples {
            // SAFETY: Retained completed compressed sample used only for diagnostics.
            eprintln!(
                "GPU003 compressed PTS={:?}, outputPTS={:?}",
                unsafe { sample.0.presentation_time_stamp() },
                unsafe { sample.0.output_presentation_time_stamp() }
            );
        }
    }
    decode_selected(&samples, time, format)
}

pub fn generated_hardware_frame(format: ResidentFormat) -> Result<HardwareFrame, NativeError> {
    let (samples, _) = videotoolbox::encode()?;
    decode_selected(&samples, (0, 1), format)
}

/// Explicit validation read of native output. Never called by resident render.
/// CPU bytes read/copied are returned separately and excluded from production
/// transfer counters; this is an oracle over the actual native decoder output.
pub struct NativeValidationImage {
    pub pixels: Vec<[f32; 4]>,
    pub size: [u32; 2],
    pub cpu_pixel_bytes: u64,
    pub planes: Vec<Vec<u8>>,
}
impl HardwareFrame {
    pub fn read_for_validation(
        &self,
        working: WorkingSpace,
        transfer: VideoTransfer,
    ) -> Result<NativeValidationImage, NativeError> {
        use objc2_core_video::*;
        let buffer = &self.buffer.0;
        // SAFETY: A completed, retained decoder output is locked read-only.
        let status =
            unsafe { CVPixelBufferLockBaseAddress(buffer, CVPixelBufferLockFlags::ReadOnly) };
        if status != 0 {
            return Err(error(
                NativeStage::Readback,
                format!("validation pixel lock OSStatus={status}"),
            ));
        }
        let [width, height] = self.size();
        let result = (|| {
            let read = |plane: Option<usize>, channels: usize| -> Result<Vec<u8>, NativeError> {
                let (base, w, h, stride) = if let Some(plane) = plane {
                    (
                        CVPixelBufferGetBaseAddressOfPlane(buffer, plane).cast::<u8>(),
                        CVPixelBufferGetWidthOfPlane(buffer, plane),
                        CVPixelBufferGetHeightOfPlane(buffer, plane),
                        CVPixelBufferGetBytesPerRowOfPlane(buffer, plane),
                    )
                } else {
                    (
                        CVPixelBufferGetBaseAddress(buffer).cast::<u8>(),
                        CVPixelBufferGetWidth(buffer),
                        CVPixelBufferGetHeight(buffer),
                        CVPixelBufferGetBytesPerRow(buffer),
                    )
                };
                let row = w
                    .checked_mul(channels)
                    .ok_or_else(|| error(NativeStage::Readback, "validation plane row overflow"))?;
                let count = row.checked_mul(h).ok_or_else(|| {
                    error(NativeStage::Readback, "validation plane size overflow")
                })?;
                if base.is_null() || row > stride || count > 512 * 1024 * 1024 {
                    return Err(error(
                        NativeStage::Readback,
                        "validation plane address/size",
                    ));
                }
                let mut bytes = Vec::with_capacity(count);
                for y in 0..h {
                    // SAFETY: The locked CV plane owns h rows of stride bytes;
                    // row is bounded by stride and native dimensions/channels.
                    bytes.extend_from_slice(unsafe {
                        std::slice::from_raw_parts(base.add(y * stride), row)
                    });
                }
                Ok(bytes)
            };
            let first = read(
                if self.format == ResidentFormat::Bgra8 {
                    None
                } else {
                    Some(0)
                },
                if self.format == ResidentFormat::Bgra8 {
                    4
                } else {
                    1
                },
            )?;
            let second = if self.format == ResidentFormat::Nv12VideoRange {
                read(Some(1), 2)?
            } else {
                Vec::new()
            };
            let uv_width = CVPixelBufferGetWidthOfPlane(buffer, 1);
            let inverse = |v: f32| match transfer {
                VideoTransfer::Srgb if v <= 0.04045 => v / 12.92,
                VideoTransfer::Srgb => ((v + 0.055) / 1.055).max(0.0).powf(2.4),
                VideoTransfer::Bt709 if v < 0.081 => v / 4.5,
                VideoTransfer::Bt709 => ((v + 0.099) / 1.099).max(0.0).powf(1.0 / 0.45),
            };
            let mut pixels = Vec::with_capacity(width as usize * height as usize);
            for y in 0..height as usize {
                for x in 0..width as usize {
                    let (rgb, alpha) = if self.format == ResidentFormat::Bgra8 {
                        let i = (y * width as usize + x) * 4;
                        (
                            [
                                f32::from(first[i + 2]) / 255.0,
                                f32::from(first[i + 1]) / 255.0,
                                f32::from(first[i]) / 255.0,
                            ],
                            f32::from(first[i + 3]) / 255.0,
                        )
                    } else {
                        let luma = (f32::from(first[y * width as usize + x]) - 16.0) / 219.0;
                        let i = ((y / 2) * uv_width + x / 2) * 2;
                        let cb = (f32::from(second[i]) - 128.0) / 224.0;
                        let cr = (f32::from(second[i + 1]) - 128.0) / 224.0;
                        (
                            [
                                luma + 1.5748 * cr,
                                luma - 0.187324 * cb - 0.468124 * cr,
                                luma + 1.8556 * cb,
                            ],
                            1.0,
                        )
                    };
                    let mut rgb = rgb.map(inverse);
                    if working == WorkingSpace::LinearRec2020 {
                        let v = rgb;
                        rgb = [
                            0.6274039 * v[0] + 0.329_283 * v[1] + 0.0433131 * v[2],
                            0.0690973 * v[0] + 0.9195404 * v[1] + 0.0113623 * v[2],
                            0.0163914 * v[0] + 0.0880133 * v[1] + 0.8955953 * v[2],
                        ];
                    }
                    pixels.push([rgb[0] * alpha, rgb[1] * alpha, rgb[2] * alpha, alpha]);
                }
            }
            Ok(NativeValidationImage {
                pixels,
                size: [width, height],
                cpu_pixel_bytes: (first.len() + second.len()) as u64,
                planes: if self.format == ResidentFormat::Bgra8 {
                    vec![first]
                } else {
                    vec![first, second]
                },
            })
        })();
        // SAFETY: Matches the successful read-only lock, including error paths.
        let status =
            unsafe { CVPixelBufferUnlockBaseAddress(buffer, CVPixelBufferLockFlags::ReadOnly) };
        if status != 0 {
            return Err(error(
                NativeStage::Readback,
                format!("validation pixel unlock OSStatus={status}"),
            ));
        }
        result
    }
}

fn exact_time_add(a: CMTime, b: CMTime, sign: i128) -> Result<CMTime, NativeError> {
    let numerator = i128::from(a.value) * i128::from(b.timescale)
        + sign * i128::from(b.value) * i128::from(a.timescale);
    let denominator = i128::from(a.timescale) * i128::from(b.timescale);
    let (mut x, mut y) = (numerator.abs(), denominator);
    while y != 0 {
        let r = x % y;
        x = y;
        y = r;
    }
    let value = i64::try_from(numerator / x).map_err(|_| {
        error(
            NativeStage::FrameSelection,
            "native exact time numerator overflow",
        )
    })?;
    let scale = i32::try_from(denominator / x).map_err(|_| {
        error(
            NativeStage::FrameSelection,
            "native exact time denominator overflow",
        )
    })?;
    // SAFETY: Reduced exact ratio retains a positive checked native timescale.
    Ok(unsafe { CMTime::new(value, scale) })
}

/// ADR-0136/0081 capability probe: true only when VideoToolbox reports ProRes
/// RAW or RAW-HQ hardware decode support. This is a capability query, never a
/// promise; decode still verifies the session hardware property.
pub fn prores_raw_hardware_supported() -> bool {
    videotoolbox::prores_raw_supported()
}

/// ProRes RAW decode: demux `aprn`/`aprh` compressed samples and
/// hardware-decode into a linear scene-referred 64RGBAHalf buffer. Codec
/// admission, hardware decode and output format are all verified.
pub fn decode_file_prores_raw(
    path: &Path,
    stream: u32,
    time: (i64, i64),
    interval: [(i64, i64); 2],
) -> Result<HardwareFrame, NativeError> {
    if !prores_raw_hardware_supported() {
        return Err(error(
            NativeStage::HardwareDecoderQuery,
            "ProRes RAW hardware decode unsupported on this system",
        ));
    }
    if time.1 <= 0
        || interval.iter().any(|(_, den)| *den <= 0)
        || i128::from(time.0) * i128::from(interval[0].1)
            < i128::from(interval[0].0) * i128::from(time.1)
        || i128::from(time.0) * i128::from(interval[1].1)
            >= i128::from(interval[1].0) * i128::from(time.1)
    {
        return Err(error(
            NativeStage::FrameSelection,
            "time outside canonical locked half-open stream interval",
        ));
    }
    decode_file_inner(
        path,
        stream,
        time,
        ResidentFormat::ProResRawRgbah,
        Some(interval[0]),
    )
}

fn half_to_f32(h: u16) -> f32 {
    let sign = u32::from(h >> 15);
    let exponent = u32::from((h >> 10) & 0x1f);
    let fraction = f64::from(h & 0x3ff);
    let value = match exponent {
        0 => fraction * (2f64).powi(-24),
        31 => {
            if fraction == 0.0 {
                f64::INFINITY
            } else {
                f64::NAN
            }
        }
        e => (1.0 + fraction / 1024.0) * (2f64).powi(e as i32 - 15),
    };
    (if sign == 1 { -value } else { value }) as f32
}

impl HardwareFrame {
    /// Color primaries attached to the decoded buffer, mapped to the locked
    /// tag names. `None` means the decoder produced an untagged buffer and the
    /// locked stream contract stands.
    pub fn color_primaries(&self) -> Option<String> {
        use objc2_core_foundation::CFString;
        use objc2_core_video::{
            kCVImageBufferColorPrimaries_ITU_R_709_2, kCVImageBufferColorPrimaries_ITU_R_2020,
            kCVImageBufferColorPrimariesKey,
        };
        // SAFETY: Retained decoded buffer; Copy returns a +1 CFType.
        let value = unsafe {
            self.buffer
                .0
                .attachment(kCVImageBufferColorPrimariesKey, std::ptr::null_mut())
        }?;
        let text = value.downcast_ref::<CFString>().map(|s| s.to_string())?;
        // SAFETY: Process-lifetime CFString constants; read-only compare.
        let (r2020, r709) = unsafe {
            (
                kCVImageBufferColorPrimaries_ITU_R_2020.to_string(),
                kCVImageBufferColorPrimaries_ITU_R_709_2.to_string(),
            )
        };
        Some(if text == r2020 {
            "bt2020".to_string()
        } else if text == r709 {
            "bt709".to_string()
        } else {
            text
        })
    }

    /// CPU read of the ProRes RAW 64RGBAHalf buffer into f32 RGBA (linear,
    /// scene-referred, unclamped). Validation/deterministic conversion only.
    pub fn linear_pixels(&self) -> Result<Vec<f32>, NativeError> {
        use objc2_core_video::{
            CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
            CVPixelBufferGetPixelFormatType, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags,
            CVPixelBufferUnlockBaseAddress, kCVPixelFormatType_64RGBAHalf,
        };
        if self.format != ResidentFormat::ProResRawRgbah
            || CVPixelBufferGetPixelFormatType(&self.buffer.0) != kCVPixelFormatType_64RGBAHalf
        {
            return Err(error(
                NativeStage::Readback,
                "linear_pixels requires a 64RGBAHalf ProRes RAW buffer",
            ));
        }
        let buffer = &self.buffer.0;
        // SAFETY: A completed, retained decoder output is locked read-only.
        let status =
            unsafe { CVPixelBufferLockBaseAddress(buffer, CVPixelBufferLockFlags::ReadOnly) };
        if status != 0 {
            return Err(error(
                NativeStage::Readback,
                format!("pixel lock OSStatus={status}"),
            ));
        }
        let [width, height] = self.size();
        let result = (|| {
            let row_bytes = (width as usize)
                .checked_mul(8)
                .and_then(|r| (height as usize).checked_mul(r))
                .ok_or_else(|| error(NativeStage::Readback, "surface size overflow"))?;
            if row_bytes > 1024 * 1024 * 1024 {
                return Err(error(
                    NativeStage::Readback,
                    "64RGBAHalf surface budget exceeded",
                ));
            }
            let stride = CVPixelBufferGetBytesPerRow(buffer);
            let base = CVPixelBufferGetBaseAddress(buffer).cast::<u8>();
            let row = width as usize * 8;
            if base.is_null() || row > stride {
                return Err(error(NativeStage::Readback, "invalid buffer layout"));
            }
            let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
            for y in 0..height as usize {
                // SAFETY: Locked plane owns height rows of stride bytes; the
                // row slice stays inside the checked row width.
                let start = unsafe { base.add(y * stride) };
                for x in 0..width as usize {
                    for channel in 0..4 {
                        // SAFETY: in-bounds within the checked row.
                        let h = unsafe {
                            u16::from_ne_bytes([
                                *start.add(x * 8 + channel * 2),
                                *start.add(x * 8 + channel * 2 + 1),
                            ])
                        };
                        pixels.push(half_to_f32(h));
                    }
                }
            }
            Ok(pixels)
        })();
        // SAFETY: Matches the successful read-only lock, including error paths.
        let status =
            unsafe { CVPixelBufferUnlockBaseAddress(buffer, CVPixelBufferLockFlags::ReadOnly) };
        if status != 0 {
            return Err(error(
                NativeStage::Readback,
                format!("pixel unlock OSStatus={status}"),
            ));
        }
        result
    }
}
