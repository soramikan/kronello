//! Safety audit: opaque pointers are uniquely owned, children borrow runtime,
//! C strings are copied while their native owner lives, and every pixel copy
//! uses a size queried from FFmpeg. None of these types implement Send/Sync.
use crate::*;
use kronello_time::Rational;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::Path;
use std::ptr::NonNull;
#[repr(C)]
#[derive(Default)]
struct FrameInfo {
    pts: i64,
    duration: i64,
    width: c_int,
    height: c_int,
    format: c_int,
    primaries: c_int,
    transfer: c_int,
    matrix: c_int,
    range: c_int,
}
unsafe extern "C" {
    #[cfg(test)]
    fn km_codec_hardware_capable(capabilities: c_int) -> c_int;
    fn km_open(directory: *const c_char, error: *mut c_char, capacity: usize) -> *mut c_void;
    fn km_close(k: *mut c_void);
    fn km_error(k: *mut c_void) -> *const c_char;
    fn km_error_code(k: *mut c_void) -> c_int;
    fn km_error_operation(k: *mut c_void) -> *const c_char;
    fn km_error_detail(k: *mut c_void) -> *const c_char;
    fn km_info(k: *mut c_void, index: c_int) -> *const c_char;
    fn km_version(k: *mut c_void, index: c_int) -> u32;
    fn km_codec(
        k: *mut c_void,
        cursor: *mut *mut c_void,
        encoder: *mut c_int,
        decoder: *mut c_int,
        hardware: *mut c_int,
    ) -> *const c_char;
    fn km_hw(k: *mut c_void, kind: *mut c_int) -> *const c_char;
    fn km_decoder_open(k: *mut c_void, path: *const c_char) -> *mut c_void;
    fn km_decoder_open_stream(k: *mut c_void, path: *const c_char, stream: c_int) -> *mut c_void;
    fn km_video_rgba(
        k: *mut c_void,
        input: *const u8,
        input_size: c_int,
        format: *const c_char,
        width: c_int,
        height: c_int,
        full_range: c_int,
        output: *mut u8,
    ) -> c_int;
    fn km_video_rgba64(
        k: *mut c_void,
        input: *const u8,
        input_size: c_int,
        format: *const c_char,
        width: c_int,
        height: c_int,
        full_range: c_int,
        bt2020: c_int,
        output: *mut u8,
    ) -> c_int;
    fn km_decoder_close(d: *mut c_void);
    fn km_decoder_name(d: *mut c_void) -> *const c_char;
    fn km_decoder_time_base(d: *mut c_void, num: *mut c_int, den: *mut c_int);
    fn km_decoder_origin(d: *mut c_void) -> i64;
    fn km_decoder_stream(d: *mut c_void) -> c_int;
    fn km_decoder_duration(d: *mut c_void) -> i64;
    fn km_decoder_seek(d: *mut c_void, target: i64) -> c_int;
    fn km_decoder_next(d: *mut c_void) -> c_int;
    fn km_frame_info(d: *mut c_void, out: *mut FrameInfo);
    fn km_frame_label(d: *mut c_void, index: c_int) -> *const c_char;
    fn km_frame_copy(d: *mut c_void, buffer: *mut u8, size: c_int) -> c_int;
    fn km_encoder_open_color(
        k: *mut c_void,
        path: *const c_char,
        name: *const c_char,
        width: c_int,
        height: c_int,
        num: c_int,
        den: c_int,
        hdr: c_int,
    ) -> *mut c_void;
    fn km_encoder_close(e: *mut c_void);
    fn km_encoder_frame(e: *mut c_void, rgba: *const u8, pts: i64, duration: i64) -> c_int;
    fn km_encoder_finish(e: *mut c_void) -> c_int;
    fn km_encoder_frame_size(e: *mut c_void) -> c_int;
    fn km_encoder_format(e: *mut c_void) -> *const c_char;
    fn km_encoder_open_dnx(
        k: *mut c_void,
        path: *const c_char,
        width: c_int,
        height: c_int,
        num: c_int,
        den: c_int,
        kind: c_int,
    ) -> *mut c_void;
    fn km_gif_open(
        k: *mut c_void,
        width: c_int,
        height: c_int,
        num: c_int,
        den: c_int,
    ) -> *mut c_void;
    fn km_gif_frame(e: *mut c_void, rgba: *const u8) -> c_int;
    fn km_gif_palette(e: *mut c_void) -> c_int;
    fn km_gif_encode_start(e: *mut c_void, path: *const c_char) -> c_int;
    fn km_gif_encode_frame(e: *mut c_void, rgba: *const u8) -> c_int;
    fn km_gif_encode_flush(e: *mut c_void) -> c_int;
    fn km_gif_close(e: *mut c_void);
}
fn path_string(path: &Path) -> Result<CString, MediaError> {
    CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| MediaError::InvalidInput("path contains NUL".into()))
}
// SAFETY: caller supplies either NULL or a live FFmpeg/shim NUL-terminated string.
unsafe fn string(pointer: *const c_char) -> String {
    if pointer.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(pointer) }
            .to_string_lossy()
            .into_owned()
    }
}
pub(crate) struct NativeRuntime(NonNull<c_void>);
impl NativeRuntime {
    pub(crate) fn open(directory: &Path) -> Result<Self, MediaError> {
        let path = path_string(directory)?;
        let mut error = [0 as c_char; 512];
        // SAFETY: both buffers live throughout the call; km_open returns ownership.
        let ptr = unsafe { km_open(path.as_ptr(), error.as_mut_ptr(), error.len()) };
        NonNull::new(ptr)
            .map(Self)
            .ok_or_else(|| MediaError::FfmpegUnavailable(unsafe { string(error.as_ptr()) }))
    }
    fn error(&self) -> String {
        unsafe { string(km_error(self.0.as_ptr())) }
    }
    fn error_detail(&self) -> FfmpegErrorDetail {
        // SAFETY: all fields belong to this live context and are copied immediately.
        unsafe {
            FfmpegErrorDetail {
                code: km_error_code(self.0.as_ptr()),
                operation: string(km_error_operation(self.0.as_ptr())),
                message: string(km_error_detail(self.0.as_ptr())),
            }
        }
    }
    pub(crate) fn capabilities(&self, path: PathBuf, substituted: bool) -> MediaCapabilities {
        // SAFETY: all functions only read this live context; returned strings are copied.
        unsafe {
            let libraries = [
                ("avutil", 0, 1, 2),
                ("avcodec", 1, 3, 4),
                ("avformat", 2, 5, 6),
                ("swscale", 3, 7, 8),
                ("swresample", 4, 9, 10),
            ]
            .into_iter()
            .map(|(name, index, license, config)| LibraryCapability {
                name: name.into(),
                version: km_version(self.0.as_ptr(), index),
                license: string(km_info(self.0.as_ptr(), license)),
                configuration: string(km_info(self.0.as_ptr(), config)),
            })
            .collect::<Vec<_>>();
            let distribution_eligible = libraries.iter().all(|l| {
                l.license.starts_with("LGPL")
                    && !l.configuration.contains("--enable-gpl")
                    && !l.configuration.contains("--enable-nonfree")
            });
            let mut codecs = Vec::new();
            let mut cursor = std::ptr::null_mut();
            loop {
                let (mut encoder, mut decoder, mut hardware) = (0, 0, 0);
                let ptr = km_codec(
                    self.0.as_ptr(),
                    &mut cursor,
                    &mut encoder,
                    &mut decoder,
                    &mut hardware,
                );
                if ptr.is_null() {
                    break;
                }
                codecs.push(CodecCapability {
                    name: string(ptr),
                    encoder: encoder != 0,
                    decoder: decoder != 0,
                    hardware: hardware != 0,
                });
            }
            codecs.sort_by(|a, b| a.name.cmp(&b.name));
            let mut hwaccels = Vec::new();
            let mut kind = 0;
            loop {
                let ptr = km_hw(self.0.as_ptr(), &mut kind);
                if ptr.is_null() {
                    break;
                }
                hwaccels.push(string(ptr));
            }
            MediaCapabilities {
                schema_version: 1,
                ffmpeg_version: string(km_info(self.0.as_ptr(), 0)),
                library_directory: path,
                substituted,
                libraries,
                distribution_eligible,
                development_only: !distribution_eligible,
                codecs,
                hwaccels,
            }
        }
    }
}
impl Drop for NativeRuntime {
    fn drop(&mut self) {
        unsafe { km_close(self.0.as_ptr()) }
    }
}
pub(crate) struct RawFrame {
    pub pts: Rational,
    pub duration: Rational,
    pub width: u32,
    pub height: u32,
    pub labels: [String; 5],
    pub pixels: Vec<u8>,
}
fn needs_prores_range_verification(stream: &MediaStream) -> bool {
    stream.kind == StreamKind::Video
        && stream.codec == "prores"
        && matches!(stream.color_range.as_deref(), None | Some("unknown"))
}
fn verify_prores_range(stream: &mut MediaStream, frame: &RawFrame) -> bool {
    if !needs_prores_range_verification(stream)
        || stream.width != Some(frame.width)
        || stream.height != Some(frame.height)
        || [
            stream.pixel_format.as_deref(),
            stream.color_primaries.as_deref(),
            stream.color_transfer.as_deref(),
            stream.color_matrix.as_deref(),
        ] != std::array::from_fn(|i| Some(frame.labels[i].as_str()))
        || frame.labels[4] != "tv"
    {
        return false;
    }
    stream.color_range = Some(frame.labels[4].clone());
    true
}
#[cfg(test)]
mod prores_probe_tests {
    use super::*;
    #[test]
    fn unspecified_prores_range_requires_matching_native_frame_evidence() {
        let stream: MediaStream = serde_json::from_value(serde_json::json!({
            "index":0,"kind":"video","codec":"prores",
            "time_base":{"num":"1","den":"24"},"start":null,"duration":null,
            "sample_rate":null,"channels":null,"width":16,"height":16,
            "pixel_format":"yuv422p10le","color_primaries":"bt2020",
            "color_transfer":"smpte2084","color_matrix":"bt2020nc","color_range":"unknown"
        }))
        .unwrap();
        let mut frame = RawFrame {
            pts: Rational::ZERO,
            duration: Rational::ZERO,
            width: 16,
            height: 16,
            labels: ["yuv422p10le", "bt2020", "smpte2084", "bt2020nc", "tv"].map(str::to_owned),
            pixels: vec![],
        };
        let mut verified = stream.clone();
        assert!(verify_prores_range(&mut verified, &frame));
        assert_eq!(verified.color_range.as_deref(), Some("tv"));
        for i in 0..5 {
            let original = frame.labels[i].clone();
            frame.labels[i] = "mismatch".into();
            let mut unchanged = stream.clone();
            assert!(!verify_prores_range(&mut unchanged, &frame));
            assert_eq!(unchanged, stream);
            frame.labels[i] = original;
        }
        frame.width = 32;
        let mut unchanged = stream.clone();
        assert!(!verify_prores_range(&mut unchanged, &frame));
        assert_eq!(unchanged, stream);
        frame.width = 16;
        for (codec, range) in [("prores", "pc"), ("prores", "tv"), ("h264", "unknown")] {
            let mut explicit = stream.clone();
            explicit.codec = codec.into();
            explicit.color_range = Some(range.into());
            let before = explicit.clone();
            assert!(!verify_prores_range(&mut explicit, &frame));
            assert_eq!(explicit, before);
        }
    }
}
pub(crate) struct NativeDecoder<'a> {
    path: std::path::PathBuf,
    ptr: NonNull<c_void>,
    runtime: &'a NativeRuntime,
    pub time_base: Rational,
    pub name: String,
}
impl<'a> NativeDecoder<'a> {
    pub(crate) fn open(runtime: &'a NativeRuntime, path: &Path) -> Result<Self, MediaError> {
        Self::open_stream(runtime, path, None)
    }
    pub(crate) fn open_stream(
        runtime: &'a NativeRuntime,
        path: &Path,
        stream: Option<u32>,
    ) -> Result<Self, MediaError> {
        let source_path = path.to_path_buf();
        let path = path_string(path)?;
        let ptr = NonNull::new(match stream {
            Some(stream) => {
                let stream = c_int::try_from(stream)
                    .map_err(|_| MediaError::InvalidInput("stream index overflow".into()))?;
                unsafe { km_decoder_open_stream(runtime.0.as_ptr(), path.as_ptr(), stream) }
            }
            None => unsafe { km_decoder_open(runtime.0.as_ptr(), path.as_ptr()) },
        })
        .ok_or_else(|| MediaError::Decode(runtime.error()))?;
        let (mut num, mut den) = (0, 0);
        unsafe { km_decoder_time_base(ptr.as_ptr(), &mut num, &mut den) };
        let time_base = match Rational::new(i64::from(num), i64::from(den)) {
            Ok(t) if t > Rational::ZERO => t,
            _ => {
                unsafe { km_decoder_close(ptr.as_ptr()) };
                return Err(MediaError::Decode("invalid stream time base".into()));
            }
        };
        Ok(Self {
            path: source_path,
            ptr,
            runtime,
            time_base,
            name: unsafe { string(km_decoder_name(ptr.as_ptr())) },
        })
    }
    pub(crate) fn origin(&self) -> i64 {
        unsafe { km_decoder_origin(self.ptr.as_ptr()) }
    }
    pub(crate) fn stream(&self) -> u32 {
        unsafe { km_decoder_stream(self.ptr.as_ptr()) as u32 }
    }
    pub(crate) fn duration(&self) -> Result<Option<Rational>, MediaError> {
        let duration = unsafe { km_decoder_duration(self.ptr.as_ptr()) };
        if duration == i64::MIN {
            Ok(None)
        } else {
            Ok(Some(
                Rational::from_integer(duration).checked_mul(self.time_base)?,
            ))
        }
    }
    pub(crate) fn runtime(&self) -> &'a NativeRuntime {
        self.runtime
    }
    pub(crate) fn restart_origin(&mut self) -> Result<(), MediaError> {
        if self.origin() < 0 {
            // Timestamp seeking cannot reliably rewind negative-origin TS.
            // Reopening the same stream starts from its exact first packet.
            let replacement = Self::open_stream(self.runtime, &self.path, Some(self.stream()))?;
            *self = replacement;
            Ok(())
        } else {
            self.seek(self.origin())
        }
    }
    pub(crate) fn seek(&mut self, pts: i64) -> Result<(), MediaError> {
        if unsafe { km_decoder_seek(self.ptr.as_ptr(), pts) } < 0 {
            Err(MediaError::Decode(self.runtime.error()))
        } else {
            Ok(())
        }
    }
    pub(crate) fn next(&mut self) -> Result<Option<RawFrame>, MediaError> {
        let ret = unsafe { km_decoder_next(self.ptr.as_ptr()) };
        if ret < 0 {
            return Err(MediaError::Decode(self.runtime.error()));
        }
        if ret == 0 {
            return Ok(None);
        }
        let mut info = FrameInfo::default();
        unsafe { km_frame_info(self.ptr.as_ptr(), &mut info) };
        if info.pts == i64::MIN || info.duration < 0 || info.width <= 0 || info.height <= 0 {
            return Err(MediaError::Decode(
                "missing PTS or invalid frame metadata".into(),
            ));
        }
        if i64::from(info.width) * i64::from(info.height) > 16_777_216 {
            return Err(MediaError::UnsupportedFeature(
                "decoded video pixel budget".into(),
            ));
        }
        let size = unsafe { km_frame_copy(self.ptr.as_ptr(), std::ptr::null_mut(), 0) };
        if size < 0 {
            return Err(MediaError::Decode("unsupported native pixel layout".into()));
        }
        if size > 134_217_728 {
            return Err(MediaError::UnsupportedFeature(
                "decoded video byte budget".into(),
            ));
        }
        let mut pixels = vec![0; size as usize];
        // SAFETY: buffer has exactly the size queried for the unchanged native frame.
        if unsafe { km_frame_copy(self.ptr.as_ptr(), pixels.as_mut_ptr(), size) } != size {
            return Err(MediaError::Decode("pixel copy failed".into()));
        }
        Ok(Some(RawFrame {
            pts: Rational::from_integer(info.pts).checked_mul(self.time_base)?,
            duration: Rational::from_integer(info.duration).checked_mul(self.time_base)?,
            width: info.width as u32,
            height: info.height as u32,
            labels: std::array::from_fn(|i| unsafe {
                string(km_frame_label(self.ptr.as_ptr(), i as c_int))
            }),
            pixels,
        }))
    }
}
pub(crate) fn video_rgba(
    runtime: &NativeRuntime,
    frame: &kronello_render::DecodedVideoFrame,
    full_range: bool,
) -> Result<Vec<u8>, MediaError> {
    if frame.width == 0
        || frame.height == 0
        || u64::from(frame.width) * u64::from(frame.height) > 16_777_216
    {
        return Err(MediaError::InvalidInput(
            "video frame dimensions/budget".into(),
        ));
    }
    let format = CString::new(frame.pixel_format.as_str())
        .map_err(|_| MediaError::InvalidInput("video pixel format".into()))?;
    let size = c_int::try_from(frame.pixels.len())
        .map_err(|_| MediaError::InvalidInput("video pixel budget".into()))?;
    let mut output = vec![0; frame.width as usize * frame.height as usize * 4];
    // SAFETY: dimensions and exact packed input size are verified by the shim;
    // the RGBA output allocation covers width*height*4 and lives through the call.
    if unsafe {
        km_video_rgba(
            runtime.0.as_ptr(),
            frame.pixels.as_ptr(),
            size,
            format.as_ptr(),
            frame.width as c_int,
            frame.height as c_int,
            c_int::from(full_range),
            output.as_mut_ptr(),
        )
    } < 0
    {
        return Err(MediaError::Decode(runtime.error()));
    }
    Ok(output)
}
pub(crate) fn video_rgba64(
    runtime: &NativeRuntime,
    frame: &kronello_render::DecodedVideoFrame,
    full_range: bool,
    bt2020: bool,
) -> Result<Vec<u8>, MediaError> {
    if frame.width == 0
        || frame.height == 0
        || u64::from(frame.width) * u64::from(frame.height) > 16_777_216
    {
        return Err(MediaError::InvalidInput(
            "video frame dimensions/budget".into(),
        ));
    }
    let format = CString::new(frame.pixel_format.as_str())
        .map_err(|_| MediaError::InvalidInput("video pixel format".into()))?;
    let size = c_int::try_from(frame.pixels.len())
        .map_err(|_| MediaError::InvalidInput("video pixel budget".into()))?;
    let mut output = vec![0; frame.width as usize * frame.height as usize * 8];
    // SAFETY: dimensions and exact packed input size are verified by the shim;
    // the RGBA64 output allocation covers width*height*8 and lives through the call.
    if unsafe {
        km_video_rgba64(
            runtime.0.as_ptr(),
            frame.pixels.as_ptr(),
            size,
            format.as_ptr(),
            frame.width as c_int,
            frame.height as c_int,
            c_int::from(full_range),
            c_int::from(bt2020),
            output.as_mut_ptr(),
        )
    } < 0
    {
        return Err(MediaError::Decode(runtime.error()));
    }
    Ok(output)
}
impl Drop for NativeDecoder<'_> {
    fn drop(&mut self) {
        unsafe { km_decoder_close(self.ptr.as_ptr()) }
    }
}
pub(crate) struct NativeEncoder<'a> {
    ptr: NonNull<c_void>,
    runtime: &'a NativeRuntime,
    pub pixel_format: String,
    pub frame_size: u64,
}
impl<'a> NativeEncoder<'a> {
    pub(crate) fn open_color(
        runtime: &'a NativeRuntime,
        path: &Path,
        codec: &CodecCapability,
        width: i32,
        height: i32,
        time_base: Rational,
        hdr: Option<kronello_render::HdrTransfer>,
    ) -> Result<Self, MediaError> {
        let path = path_string(path)?;
        let name = CString::new(codec.name.as_str())
            .map_err(|_| MediaError::InvalidInput("codec name".into()))?;
        let num = i32::try_from(time_base.numerator())
            .map_err(|_| MediaError::InvalidInput("time base numerator".into()))?;
        let den = i32::try_from(time_base.denominator())
            .map_err(|_| MediaError::InvalidInput("time base denominator".into()))?;
        let ptr = NonNull::new(unsafe {
            km_encoder_open_color(
                runtime.0.as_ptr(),
                path.as_ptr(),
                name.as_ptr(),
                width,
                height,
                num,
                den,
                match hdr {
                    None => 0,
                    Some(kronello_render::HdrTransfer::Pq) => 1,
                    Some(kronello_render::HdrTransfer::Hlg) => 2,
                },
            )
        })
        .ok_or_else(|| {
            if codec.hardware {
                MediaError::EncoderUnavailable {
                    encoder: codec.name.clone(),
                    reason: "native encoder initialization failed".into(),
                    ffmpeg: Some(runtime.error_detail()),
                }
            } else {
                MediaError::Encode(runtime.error())
            }
        })?;
        Ok(Self {
            ptr,
            runtime,
            pixel_format: unsafe { string(km_encoder_format(ptr.as_ptr())) },
            frame_size: unsafe { km_encoder_frame_size(ptr.as_ptr()) }.max(0) as u64,
        })
    }
    /// MEDIA-004 DNxHD/DNxHR delivery encoder. `kind` indexes the closed C
    /// `DNX_KINDS` table; the runtime capability check for `dnxhd` is the
    /// caller's responsibility before opening.
    pub(crate) fn open_dnx(
        runtime: &'a NativeRuntime,
        path: &Path,
        kind: c_int,
        width: i32,
        height: i32,
        time_base: Rational,
    ) -> Result<Self, MediaError> {
        let path = path_string(path)?;
        let num = i32::try_from(time_base.numerator())
            .map_err(|_| MediaError::InvalidInput("time base numerator".into()))?;
        let den = i32::try_from(time_base.denominator())
            .map_err(|_| MediaError::InvalidInput("time base denominator".into()))?;
        // SAFETY: runtime/path live through the call; the returned context is owned.
        let ptr = NonNull::new(unsafe {
            km_encoder_open_dnx(
                runtime.0.as_ptr(),
                path.as_ptr(),
                width,
                height,
                num,
                den,
                kind,
            )
        })
        .ok_or_else(|| {
            let detail = runtime.error_detail();
            if detail.operation == "encoder unavailable" {
                MediaError::EncoderUnavailable {
                    encoder: "dnxhd".into(),
                    reason: "native encoder initialization failed".into(),
                    ffmpeg: Some(detail),
                }
            } else {
                MediaError::Encode(runtime.error())
            }
        })?;
        Ok(Self {
            ptr,
            runtime,
            pixel_format: unsafe { string(km_encoder_format(ptr.as_ptr())) },
            frame_size: unsafe { km_encoder_frame_size(ptr.as_ptr()) }.max(0) as u64,
        })
    }
    // Caller validates byte length against the encoder dimensions before this call.
    pub(crate) fn frame(
        &mut self,
        pixels: &[u8],
        pts: i64,
        duration: i64,
    ) -> Result<(), MediaError> {
        if unsafe { km_encoder_frame(self.ptr.as_ptr(), pixels.as_ptr(), pts, duration) } < 0 {
            Err(MediaError::Encode(self.runtime.error()))
        } else {
            Ok(())
        }
    }
    pub(crate) fn finish(&mut self) -> Result<(), MediaError> {
        if unsafe { km_encoder_finish(self.ptr.as_ptr()) } < 0 {
            Err(MediaError::Encode(self.runtime.error()))
        } else {
            Ok(())
        }
    }
}
impl Drop for NativeEncoder<'_> {
    fn drop(&mut self) {
        unsafe { km_encoder_close(self.ptr.as_ptr()) }
    }
}

/// MEDIA-004 deterministic indexed-color GIF session. Phase contract is
/// enforced natively: `frame` collects the palette histogram for every
/// rendered frame once, `palette` derives the median-cut table, then
/// `encode_start`/`encode_frame`/`flush` map the same pixels through the
/// fixed Bayer-ordered LUT into the gif muxer.
pub(crate) struct NativeGifEncoder<'a> {
    ptr: NonNull<c_void>,
    runtime: &'a NativeRuntime,
}
impl<'a> NativeGifEncoder<'a> {
    pub(crate) fn open(
        runtime: &'a NativeRuntime,
        width: u32,
        height: u32,
        time_base: Rational,
    ) -> Result<Self, MediaError> {
        let width = c_int::try_from(width)
            .map_err(|_| MediaError::InvalidInput("GIF width overflow".into()))?;
        let height = c_int::try_from(height)
            .map_err(|_| MediaError::InvalidInput("GIF height overflow".into()))?;
        let num = i32::try_from(time_base.numerator())
            .map_err(|_| MediaError::InvalidInput("time base numerator".into()))?;
        let den = i32::try_from(time_base.denominator())
            .map_err(|_| MediaError::InvalidInput("time base denominator".into()))?;
        // SAFETY: runtime is live; the returned context is uniquely owned.
        let ptr = NonNull::new(unsafe { km_gif_open(runtime.0.as_ptr(), width, height, num, den) })
            .ok_or_else(|| MediaError::Encode(runtime.error()))?;
        Ok(Self { ptr, runtime })
    }
    /// Phase 1: fold one rendered RGBA8 frame into the palette histogram.
    pub(crate) fn frame(&mut self, rgba: &[u8]) -> Result<(), MediaError> {
        // SAFETY: the shim reads width*height*4 bytes from a live buffer.
        if unsafe { km_gif_frame(self.ptr.as_ptr(), rgba.as_ptr()) } < 0 {
            return Err(MediaError::Encode(self.runtime.error()));
        }
        Ok(())
    }
    /// End phase 1 and derive the fixed 256-color palette.
    pub(crate) fn palette(&mut self) -> Result<(), MediaError> {
        if unsafe { km_gif_palette(self.ptr.as_ptr()) } < 0 {
            return Err(MediaError::Encode(self.runtime.error()));
        }
        Ok(())
    }
    /// Phase 2: open the gif encoder/muxer at `path` (a staged artifact).
    pub(crate) fn encode_start(&mut self, path: &Path) -> Result<(), MediaError> {
        let path = path_string(path)?;
        if unsafe { km_gif_encode_start(self.ptr.as_ptr(), path.as_ptr()) } < 0 {
            let detail = self.runtime.error_detail();
            return Err(if detail.operation == "encoder unavailable" {
                MediaError::EncoderUnavailable {
                    encoder: "gif".into(),
                    reason: "native encoder initialization failed".into(),
                    ffmpeg: Some(detail),
                }
            } else {
                MediaError::Encode(self.runtime.error())
            });
        }
        Ok(())
    }
    pub(crate) fn encode_frame(&mut self, rgba: &[u8]) -> Result<(), MediaError> {
        // SAFETY: the shim reads width*height*4 bytes from a live buffer.
        if unsafe { km_gif_encode_frame(self.ptr.as_ptr(), rgba.as_ptr()) } < 0 {
            return Err(MediaError::Encode(self.runtime.error()));
        }
        Ok(())
    }
    pub(crate) fn flush(&mut self) -> Result<(), MediaError> {
        if unsafe { km_gif_encode_flush(self.ptr.as_ptr()) } < 0 {
            return Err(MediaError::Encode(self.runtime.error()));
        }
        Ok(())
    }
}
impl Drop for NativeGifEncoder<'_> {
    fn drop(&mut self) {
        unsafe { km_gif_close(self.ptr.as_ptr()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hevc_delivery_mux_configuration_requires_hvc1() {
        for profile in [
            MovieProfile::ProResPcm24,
            MovieProfile::Av1Mp4AlacV1,
            MovieProfile::H264AlacV1,
        ] {
            // SAFETY: the same pure tag selector is used by km_mux_av before writing its header.
            assert_eq!(unsafe { km_mux_video_tag(profile.native_id()) }, 0);
        }
        // SAFETY: a closed profile ID is accepted without opening a codec or device.
        assert_eq!(
            unsafe { km_mux_video_tag(MovieProfile::HevcAlacV1.native_id()) },
            u32::from_le_bytes(*b"hvc1")
        );
    }

    #[test]
    fn native_probe_recognizes_hardware_and_hybrid_capability_bits() {
        // Public FFmpeg AV_CODEC_CAP_* bit values. Exercise the same C predicate
        // used by codec enumeration without loading FFmpeg or opening a device.
        const HARDWARE: c_int = 1 << 18;
        const HYBRID: c_int = 1 << 19;
        const DELAY: c_int = 1 << 5;
        for (flags, expected) in [
            (0, false),
            (DELAY, false),
            (HARDWARE, true),
            (HYBRID, true),
            (HARDWARE | HYBRID | DELAY, true),
            (HYBRID | DELAY, true),
        ] {
            // SAFETY: the pure C predicate accepts every integer bit pattern.
            assert_eq!(unsafe { km_codec_hardware_capable(flags) } != 0, expected);
        }
    }

    #[test]
    fn encoder_open_failure_preserves_ffmpeg_code_operation_and_message() {
        let runtime = MediaRuntime::load().unwrap();
        // Exercise a real avcodec_open2 failure without requiring a hardware
        // device: mark the mandatory ProRes encoder as hardware at this private
        // boundary and supply invalid dimensions, bypassing public validation.
        let mut codec = runtime
            .capabilities()
            .select_encoder(EncodeCodec::ProRes)
            .unwrap()
            .clone();
        codec.hardware = true;
        let temp = tempfile::NamedTempFile::new().unwrap();
        let error = NativeEncoder::open_color(
            &runtime.native,
            temp.path(),
            &codec,
            0,
            64,
            Rational::new(1, 24).unwrap(),
            None,
        )
        .err()
        .expect("zero width must fail avcodec_open2");
        assert_eq!(error.code(), "ENCODER_UNAVAILABLE");
        let display = error.to_string();
        let MediaError::EncoderUnavailable {
            encoder,
            ffmpeg: Some(detail),
            ..
        } = error
        else {
            panic!("structured FFmpeg failure required: {display}");
        };
        assert_eq!(encoder, "prores_ks");
        assert!(detail.code < 0);
        assert_eq!(
            detail.operation,
            "avcodec_open2(prores_ks, yuv422p10le, 0x64, time_base=1/24)"
        );
        assert!(!detail.message.is_empty());
        assert!(display.contains(&detail.code.to_string()));
        assert!(display.contains(&detail.message));
    }
}

#[repr(C)]
#[derive(Default)]
struct NativeStreamInfo {
    start: i64,
    duration: i64,
    channel_mask: i64,
    kind: c_int,
    num: c_int,
    den: c_int,
    rate: c_int,
    channels: c_int,
    width: c_int,
    height: c_int,
}
unsafe extern "C" {
    fn km_audio_open(k: *mut c_void, path: *const c_char, stream: c_int) -> *mut c_void;
    fn km_audio_close(a: *mut c_void);
    fn km_audio_time_base(a: *mut c_void, num: *mut c_int, den: *mut c_int);
    fn km_audio_rate(a: *mut c_void) -> c_int;
    fn km_audio_channels(a: *mut c_void) -> c_int;
    fn km_audio_mask(a: *mut c_void) -> i64;
    fn km_audio_pts(a: *mut c_void) -> i64;
    fn km_audio_input_samples(a: *mut c_void) -> c_int;
    fn km_audio_count(a: *mut c_void) -> c_int;
    fn km_audio_copy(a: *mut c_void, out: *mut f32, capacity: c_int) -> c_int;
    fn km_audio_next(a: *mut c_void) -> c_int;
    fn km_audio_encoder_open(
        k: *mut c_void,
        path: *const c_char,
        kind: c_int,
        channels: c_int,
        mask: i64,
    ) -> *mut c_void;
    fn km_audio_encoder_close(e: *mut c_void);
    fn km_audio_encoder_block(e: *mut c_void) -> c_int;
    fn km_audio_encoder_frame(e: *mut c_void, samples: *const i32, count: c_int) -> c_int;
    fn km_audio_encoder_finish(e: *mut c_void) -> c_int;
    fn km_audio_encode(
        k: *mut c_void,
        path: *const c_char,
        samples: *const i32,
        count: i64,
        kind: c_int,
        channels: c_int,
        mask: i64,
    ) -> c_int;
    fn km_probe_open(k: *mut c_void, path: *const c_char) -> *mut c_void;
    fn km_probe_close(k: *mut c_void, format: *mut c_void);
    fn km_probe_count(format: *mut c_void) -> c_int;
    fn km_probe_format_duration(format: *mut c_void) -> i64;
    fn km_probe_stream(format: *mut c_void, index: c_int, out: *mut NativeStreamInfo);
    fn km_probe_codec(k: *mut c_void, format: *mut c_void, index: c_int) -> *const c_char;
    fn km_probe_color(
        k: *mut c_void,
        format: *mut c_void,
        index: c_int,
        field: c_int,
    ) -> *const c_char;
    fn km_probe_codec_tag(format: *mut c_void, index: c_int) -> u32;
    #[cfg(test)]
    fn km_mux_video_tag(profile: c_int) -> u32;
    fn km_probe_tag(k: *mut c_void, format: *mut c_void, key: *const c_char) -> *const c_char;
    fn km_probe_chapter_count(format: *mut c_void) -> c_int;
    fn km_probe_chapter(
        format: *mut c_void,
        index: c_int,
        id: *mut i64,
        start: *mut i64,
        end: *mut i64,
        num: *mut c_int,
        den: *mut c_int,
    );
    fn km_probe_chapter_title(k: *mut c_void, format: *mut c_void, index: c_int) -> *const c_char;
    fn km_mux_av(
        k: *mut c_void,
        video: *const c_char,
        audio: *const c_char,
        path: *const c_char,
        render_hash: *const c_char,
        export_hash: *const c_char,
        profile: c_int,
        audio_channels: c_int,
        chapters: *const KmChapter,
        chapter_count: c_int,
    ) -> c_int;
}

/// Chapter times cross the boundary as ticks of the fixed 1/48000 master
/// clock; `title` must stay live through the mux call.
#[repr(C)]
pub(crate) struct KmChapter {
    pub start: i64,
    pub end: i64,
    pub title: *const c_char,
}

/// One drained resampler output. `samples` is interleaved f32 in `mask` order
/// and holds exactly `count * mask.channels()` values (count = frames).
pub(crate) struct AudioChunk {
    pub pts: Option<Rational>,
    pub input_samples: usize,
    pub rate: u32,
    /// Native source channel count; identical to `mask.channels()`.
    pub channels: u32,
    /// Delivered sample layout — the source's own mask, never a fold-down.
    pub mask: kronello_model::ChannelMask,
    pub samples: Vec<f32>,
}
/// ADR-0124: native layout rejections become typed audio layout errors.
fn audio_error(runtime: &NativeRuntime) -> MediaError {
    let detail = runtime.error_detail();
    match detail.operation.as_str() {
        "unsupported audio channel layout" => {
            kronello_audio::AudioError::UnsupportedChannelLayout(runtime.error()).into()
        }
        "audio format changes within stream" => MediaError::UnsupportedFeature(runtime.error()),
        _ => MediaError::Decode(runtime.error()),
    }
}
pub(crate) struct NativeAudioDecoder<'a> {
    ptr: NonNull<c_void>,
    runtime: &'a NativeRuntime,
    pub time_base: Rational,
}
impl<'a> NativeAudioDecoder<'a> {
    pub(crate) fn open(
        runtime: &'a NativeRuntime,
        path: &Path,
        stream: u32,
    ) -> Result<Self, MediaError> {
        let path = path_string(path)?;
        let stream = i32::try_from(stream)
            .map_err(|_| MediaError::InvalidInput("stream index overflow".into()))?;
        // SAFETY: runtime and path live through the call, handle ownership is transferred.
        let ptr = NonNull::new(unsafe { km_audio_open(runtime.0.as_ptr(), path.as_ptr(), stream) })
            .ok_or_else(|| MediaError::Decode(runtime.error()))?;
        let (mut num, mut den) = (0, 0);
        unsafe { km_audio_time_base(ptr.as_ptr(), &mut num, &mut den) };
        let time_base = match Rational::new(i64::from(num), i64::from(den)) {
            Ok(v) if v > Rational::ZERO => v,
            _ => {
                unsafe { km_audio_close(ptr.as_ptr()) };
                return Err(MediaError::Decode("invalid audio stream time base".into()));
            }
        };
        Ok(Self {
            ptr,
            runtime,
            time_base,
        })
    }
    pub(crate) fn next(&mut self) -> Result<Option<AudioChunk>, MediaError> {
        // SAFETY: the uniquely owned live handle is unchanged during the queried copy.
        unsafe {
            let ret = km_audio_next(self.ptr.as_ptr());
            if ret < 0 {
                return Err(audio_error(self.runtime));
            }
            if ret == 0 {
                return Ok(None);
            }
            let count = km_audio_count(self.ptr.as_ptr());
            if !(0..=2_097_152).contains(&count) {
                return Err(MediaError::Decode("invalid audio copy size".into()));
            }
            let bits = km_audio_mask(self.ptr.as_ptr());
            let mask = u64::try_from(bits)
                .ok()
                .and_then(|bits| kronello_model::ChannelMask::from_bits(bits).ok())
                .ok_or_else(|| {
                    MediaError::Audio(kronello_audio::AudioError::UnsupportedChannelLayout(
                        format!("decoder reported channel mask {bits:#x}"),
                    ))
                })?;
            let channels = km_audio_channels(self.ptr.as_ptr());
            if channels <= 0 || channels != mask.channels() as c_int {
                return Err(MediaError::Audio(
                    kronello_audio::AudioError::UnsupportedChannelLayout(format!(
                        "decoded frame count {channels} disagrees with layout {}",
                        mask.name()
                    )),
                ));
            }
            let total = count as usize * mask.channels();
            let mut samples = vec![0.0; total];
            if km_audio_copy(self.ptr.as_ptr(), samples.as_mut_ptr(), total as c_int)
                != total as c_int
            {
                return Err(MediaError::Decode("audio copy failed".into()));
            }
            let pts = km_audio_pts(self.ptr.as_ptr());
            let input_samples = km_audio_input_samples(self.ptr.as_ptr());
            let rate = km_audio_rate(self.ptr.as_ptr());
            if input_samples < 0 || rate <= 0 {
                return Err(MediaError::Decode("invalid decoded audio metadata".into()));
            }
            Ok(Some(AudioChunk {
                pts: if pts == i64::MIN {
                    None
                } else {
                    Some(Rational::from_integer(pts).checked_mul(self.time_base)?)
                },
                input_samples: input_samples as usize,
                rate: rate as u32,
                channels: channels as u32,
                mask,
                samples,
            }))
        }
    }
}
impl Drop for NativeAudioDecoder<'_> {
    fn drop(&mut self) {
        unsafe { km_audio_close(self.ptr.as_ptr()) }
    }
}
/// Closed set of delivery audio encoders; indexes the C `AUDIO_KINDS` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub(crate) enum AudioEncoderKind {
    Pcm24 = 0,
    Alac = 1,
    Aac = 2,
    Opus = 3,
    /// MEDIA-004: MP3 (libmp3lame CBR) standalone deliverable.
    Mp3 = 4,
    /// MEDIA-004: FLAC (native) lossless standalone deliverable.
    Flac = 5,
}
impl AudioEncoderKind {
    /// Registered FFmpeg encoder name, used for capability discovery.
    pub(crate) fn encoder_name(self) -> &'static str {
        match self {
            Self::Pcm24 => "pcm_s24le",
            Self::Alac => "alac",
            Self::Aac => "aac",
            Self::Opus => "libopus",
            Self::Mp3 => "libmp3lame",
            Self::Flac => "flac",
        }
    }
    /// Codec name as reported by stream probes.
    pub(crate) fn codec_name(self) -> &'static str {
        match self {
            Self::Pcm24 => "pcm_s24le",
            Self::Alac => "alac",
            Self::Aac => "aac",
            Self::Opus => "opus",
            Self::Mp3 => "mp3",
            Self::Flac => "flac",
        }
    }
    /// Intermediate file suffix while staging a movie output. Standalone MP3
    /// and FLAC deliveries encode straight to their staged artifact, so this
    /// is only meaningful for the intermediate kinds.
    pub(crate) fn intermediate_suffix(self) -> &'static str {
        match self {
            Self::Opus => "audio.webm",
            _ => "audio.mov",
        }
    }
}
pub(crate) struct NativeAudioEncoder<'a> {
    ptr: NonNull<c_void>,
    runtime: &'a NativeRuntime,
    pub block: usize,
    channels: usize,
}
impl<'a> NativeAudioEncoder<'a> {
    /// The requested layout must come from the closed ADR-0124 set; the native
    /// boundary repeats that check and never substitutes another mask.
    pub(crate) fn open(
        runtime: &'a NativeRuntime,
        path: &Path,
        kind: AudioEncoderKind,
        layout: kronello_model::ChannelMask,
    ) -> Result<Self, MediaError> {
        let path = path_string(path)?;
        let channels = c_int::try_from(layout.channels())
            .map_err(|_| MediaError::InvalidInput("audio channel budget".into()))?;
        // SAFETY: the runtime and path remain live; returned context is owned.
        let ptr = NonNull::new(unsafe {
            km_audio_encoder_open(
                runtime.0.as_ptr(),
                path.as_ptr(),
                kind as c_int,
                channels,
                u64::from(layout) as i64,
            )
        })
        .ok_or_else(|| {
            let detail = runtime.error_detail();
            if detail.operation == "unsupported audio channel layout" {
                MediaError::Audio(kronello_audio::AudioError::UnsupportedChannelLayout(
                    runtime.error(),
                ))
            } else if detail.operation == "audio encoder unavailable" {
                // MEDIA-004: missing closed-profile encoders are a typed
                // capability failure, never a silent substitution.
                MediaError::EncoderUnavailable {
                    encoder: kind.encoder_name().into(),
                    reason: "native audio encoder is missing".into(),
                    ffmpeg: Some(detail),
                }
            } else {
                MediaError::Encode(runtime.error())
            }
        })?;
        let block = unsafe { km_audio_encoder_block(ptr.as_ptr()) } as usize;
        Ok(Self {
            ptr,
            runtime,
            block,
            channels: layout.channels(),
        })
    }
    pub(crate) fn frame(&mut self, samples: &[i32]) -> Result<(), MediaError> {
        if samples.is_empty()
            || !samples.len().is_multiple_of(self.channels)
            || samples.len() / self.channels > self.block
        {
            return Err(MediaError::InvalidInput(
                "audio encoder block length".into(),
            ));
        }
        // SAFETY: `channels` interleaved samples per frame, bounded by the
        // owned codec block; the encoder was opened for exactly this layout.
        if unsafe {
            km_audio_encoder_frame(
                self.ptr.as_ptr(),
                samples.as_ptr(),
                (samples.len() / self.channels) as c_int,
            )
        } < 0
        {
            return Err(MediaError::Encode(self.runtime.error()));
        }
        Ok(())
    }
    pub(crate) fn finish(&mut self) -> Result<(), MediaError> {
        // SAFETY: the unique live encoder has accepted all its input.
        if unsafe { km_audio_encoder_finish(self.ptr.as_ptr()) } < 0 {
            return Err(MediaError::Encode(self.runtime.error()));
        }
        Ok(())
    }
}
impl Drop for NativeAudioEncoder<'_> {
    fn drop(&mut self) {
        // SAFETY: uniquely owned handle, destroyed before its borrowed runtime.
        unsafe { km_audio_encoder_close(self.ptr.as_ptr()) }
    }
}

struct NativeProbe<'a> {
    ptr: NonNull<c_void>,
    runtime: &'a NativeRuntime,
}
impl Drop for NativeProbe<'_> {
    fn drop(&mut self) {
        unsafe { km_probe_close(self.runtime.0.as_ptr(), self.ptr.as_ptr()) }
    }
}
impl NativeRuntime {
    pub(crate) fn probe_codec_tag(
        &self,
        path: &Path,
        stream_index: u32,
    ) -> Result<u32, MediaError> {
        let path = path_string(path)?;
        // SAFETY: the stream index is checked against the live probe's stream count.
        unsafe {
            let ptr = NonNull::new(km_probe_open(self.0.as_ptr(), path.as_ptr()))
                .ok_or_else(|| MediaError::Decode(self.error()))?;
            let probe = NativeProbe { ptr, runtime: self };
            let count = km_probe_count(probe.ptr.as_ptr());
            if !(0..=1024).contains(&count) || i64::from(stream_index) >= i64::from(count) {
                return Err(MediaError::InvalidInput(
                    "probe stream index out of range".into(),
                ));
            }
            Ok(km_probe_codec_tag(
                probe.ptr.as_ptr(),
                stream_index as c_int,
            ))
        }
    }
    pub(crate) fn encode_audio(
        &self,
        output: &Path,
        samples: &[i32],
        kind: AudioEncoderKind,
        layout: kronello_model::ChannelMask,
    ) -> Result<(), MediaError> {
        let path = path_string(output)?;
        let channels = layout.channels();
        if samples.is_empty() || !samples.len().is_multiple_of(channels) {
            return Err(MediaError::InvalidInput(
                "PCM input is not whole frames for the declared layout".into(),
            ));
        }
        let count = i64::try_from(samples.len() / channels)
            .map_err(|_| MediaError::InvalidInput("PCM size overflow".into()))?;
        // SAFETY: `channels` interleaved S32 samples per frame, live for the
        // entire synchronous call; the layout was already validated.
        if unsafe {
            km_audio_encode(
                self.0.as_ptr(),
                path.as_ptr(),
                samples.as_ptr(),
                count,
                kind as c_int,
                channels as c_int,
                u64::from(layout) as i64,
            )
        } < 0
        {
            let detail = self.error_detail();
            return Err(if detail.operation == "unsupported audio channel layout" {
                MediaError::Audio(kronello_audio::AudioError::UnsupportedChannelLayout(
                    self.error(),
                ))
            } else {
                MediaError::Encode(self.error())
            });
        }
        Ok(())
    }
    pub(crate) fn probe(&self, path: &Path) -> Result<MediaProbe, MediaError> {
        let source_path = path;
        let path = path_string(path)?;
        // SAFETY: all native stream accesses are bounded by the queried count;
        // strings are copied while the probe is live. RAII frees on every error.
        unsafe {
            let ptr = NonNull::new(km_probe_open(self.0.as_ptr(), path.as_ptr()))
                .ok_or_else(|| MediaError::Decode(self.error()))?;
            let probe = NativeProbe { ptr, runtime: self };
            let count = km_probe_count(ptr.as_ptr());
            if !(0..=1024).contains(&count) {
                return Err(MediaError::Decode("stream budget exceeded".into()));
            }
            let mut streams = Vec::new();
            for index in 0..count {
                let mut info = NativeStreamInfo::default();
                km_probe_stream(ptr.as_ptr(), index, &mut info);
                let time_base = Rational::new(i64::from(info.num), i64::from(info.den))?;
                if time_base <= Rational::ZERO {
                    return Err(MediaError::Decode("invalid probe time base".into()));
                }
                let time = |tick: i64| -> Result<Option<Rational>, MediaError> {
                    if tick == i64::MIN {
                        Ok(None)
                    } else {
                        Ok(Some(Rational::from_integer(tick).checked_mul(time_base)?))
                    }
                };
                streams.push(MediaStream {
                    index: index as u32,
                    kind: match info.kind {
                        0 => StreamKind::Video,
                        1 => StreamKind::Audio,
                        _ => StreamKind::Other,
                    },
                    codec: string(km_probe_codec(self.0.as_ptr(), ptr.as_ptr(), index)),
                    time_base,
                    start: time(info.start)?,
                    duration: time(info.duration)?,
                    sample_rate: u32::try_from(info.rate).ok().filter(|v| *v != 0),
                    channels: u32::try_from(info.channels).ok().filter(|v| *v != 0),
                    channel_mask: u64::try_from(info.channel_mask).ok().filter(|v| *v != 0),
                    width: u32::try_from(info.width).ok().filter(|v| *v != 0),
                    height: u32::try_from(info.height).ok().filter(|v| *v != 0),
                    pixel_format: (info.kind == 0)
                        .then(|| string(km_probe_color(self.0.as_ptr(), ptr.as_ptr(), index, 0))),
                    color_primaries: (info.kind == 0)
                        .then(|| string(km_probe_color(self.0.as_ptr(), ptr.as_ptr(), index, 1))),
                    color_transfer: (info.kind == 0)
                        .then(|| string(km_probe_color(self.0.as_ptr(), ptr.as_ptr(), index, 2))),
                    color_matrix: (info.kind == 0)
                        .then(|| string(km_probe_color(self.0.as_ptr(), ptr.as_ptr(), index, 3))),
                    color_range: (info.kind == 0)
                        .then(|| string(km_probe_color(self.0.as_ptr(), ptr.as_ptr(), index, 4))),
                });
            }
            let tag = |name: &str| {
                let key = CString::new(name).expect("constant tag without NUL");
                string(km_probe_tag(self.0.as_ptr(), ptr.as_ptr(), key.as_ptr()))
            };
            // MEDIA-004: container chapters, rescaled from each chapter's own
            // time base into the rational master clock.
            let chapter_count = km_probe_chapter_count(ptr.as_ptr());
            if !(0..=1024).contains(&chapter_count) {
                return Err(MediaError::Decode("chapter budget exceeded".into()));
            }
            let mut chapters = Vec::with_capacity(chapter_count as usize);
            for index in 0..chapter_count {
                let (mut id, mut start, mut end, mut num, mut den) = (0i64, 0i64, 0i64, 0, 0);
                km_probe_chapter(
                    ptr.as_ptr(),
                    index,
                    &mut id,
                    &mut start,
                    &mut end,
                    &mut num,
                    &mut den,
                );
                let time_base = Rational::new(i64::from(num), i64::from(den))?;
                if time_base <= Rational::ZERO {
                    return Err(MediaError::Decode("invalid chapter time base".into()));
                }
                chapters.push(crate::MediaChapter {
                    id,
                    start: Rational::from_integer(start).checked_mul(time_base)?,
                    end: Rational::from_integer(end).checked_mul(time_base)?,
                    title: string(km_probe_chapter_title(self.0.as_ptr(), ptr.as_ptr(), index)),
                });
            }
            let format_duration = km_probe_format_duration(ptr.as_ptr());
            let mut result = MediaProbe {
                streams,
                // Container duration is in AV_TIME_BASE (microseconds); WebM
                // streams carry no per-stream duration, only this segment total.
                duration: (format_duration != i64::MIN && format_duration > 0)
                    .then(|| Rational::new(format_duration, 1_000_000))
                    .transpose()?,
                render_snapshot_hash: tag("kronello_render_snapshot_hash"),
                export_snapshot_hash: tag("kronello_export_snapshot_hash"),
                chapters,
            };
            drop(probe);
            // MOV nclc does not carry a range flag. Older FFmpeg versions leave
            // the ProRes stream range unspecified, while its native decoder
            // reports the bitstream's limited range on the decoded frame.
            for stream in &mut result.streams {
                if needs_prores_range_verification(stream)
                    && let Ok(mut decoder) =
                        NativeDecoder::open_stream(self, source_path, Some(stream.index))
                    && let Ok(Some(frame)) = decoder.next()
                {
                    verify_prores_range(stream, &frame);
                }
            }
            Ok(result)
        }
    }
    #[allow(clippy::too_many_arguments)] // Mirrors the native mux signature one to one.
    pub(crate) fn mux_av(
        &self,
        video: &Path,
        audio: &Path,
        output: &Path,
        render_hash: &str,
        export_hash: &str,
        profile: MovieProfile,
        audio_channels: u32,
        chapters: &[crate::MediaChapter],
    ) -> Result<(), MediaError> {
        let video = path_string(video)?;
        let audio = path_string(audio)?;
        let output = path_string(output)?;
        let render_hash = CString::new(render_hash)
            .map_err(|_| MediaError::InvalidInput("hash contains NUL".into()))?;
        let export_hash = CString::new(export_hash)
            .map_err(|_| MediaError::InvalidInput("hash contains NUL".into()))?;
        let audio_channels = c_int::try_from(audio_channels)
            .map_err(|_| MediaError::InvalidInput("audio channel budget".into()))?;
        if chapters.len() > 1024 {
            return Err(MediaError::InvalidInput("chapter budget exceeded".into()));
        }
        // Chapter times cross the boundary as ticks of the fixed 1/48000
        // master clock; titles are borrowed for the synchronous call only.
        let mut titles = Vec::with_capacity(chapters.len());
        let mut native = Vec::with_capacity(chapters.len());
        for chapter in chapters {
            let title = CString::new(chapter.title.as_str())
                .map_err(|_| MediaError::InvalidInput("chapter title contains NUL".into()))?;
            let start = chapter_ticks(chapter.start)?;
            let end = chapter_ticks(chapter.end)?;
            if end <= start {
                return Err(MediaError::InvalidInput(
                    "chapter collapses below the container time base".into(),
                ));
            }
            titles.push(title);
            native.push(KmChapter {
                start,
                end,
                title: std::ptr::null(),
            });
        }
        for (entry, title) in native.iter_mut().zip(&titles) {
            entry.title = title.as_ptr();
        }
        // SAFETY: all strings, the chapter array and the runtime live
        // throughout the synchronous mux; titles outlive the call.
        if unsafe {
            km_mux_av(
                self.0.as_ptr(),
                video.as_ptr(),
                audio.as_ptr(),
                output.as_ptr(),
                render_hash.as_ptr(),
                export_hash.as_ptr(),
                profile.native_id(),
                audio_channels,
                native.as_ptr(),
                native.len() as c_int,
            )
        } < 0
        {
            return Err(MediaError::Encode(self.error()));
        }
        Ok(())
    }
}
/// Round a nonnegative rational time to 1/48000 master-clock ticks, half up.
fn chapter_ticks(time: Rational) -> Result<i64, MediaError> {
    let scaled = time.checked_mul(Rational::from_integer(48_000))?;
    let n = scaled.numerator();
    let d = scaled.denominator();
    if n < 0 {
        return Err(MediaError::InvalidInput("negative chapter time".into()));
    }
    Ok(n.div_euclid(d) + i64::from(n.rem_euclid(d) * 2 >= d))
}
