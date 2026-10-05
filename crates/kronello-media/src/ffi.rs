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
    fn km_encoder_open(
        k: *mut c_void,
        path: *const c_char,
        name: *const c_char,
        width: c_int,
        height: c_int,
        num: c_int,
        den: c_int,
    ) -> *mut c_void;
    fn km_encoder_close(e: *mut c_void);
    fn km_encoder_frame(e: *mut c_void, rgba: *const u8, pts: i64) -> c_int;
    fn km_encoder_finish(e: *mut c_void) -> c_int;
    fn km_encoder_frame_size(e: *mut c_void) -> c_int;
    fn km_encoder_format(e: *mut c_void) -> *const c_char;
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
pub(crate) struct NativeDecoder<'a> {
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
    pub(crate) fn open(
        runtime: &'a NativeRuntime,
        path: &Path,
        codec: &CodecCapability,
        width: i32,
        height: i32,
        time_base: Rational,
    ) -> Result<Self, MediaError> {
        let path = path_string(path)?;
        let name = CString::new(codec.name.as_str())
            .map_err(|_| MediaError::InvalidInput("codec name".into()))?;
        let num = i32::try_from(time_base.numerator())
            .map_err(|_| MediaError::InvalidInput("time base numerator".into()))?;
        let den = i32::try_from(time_base.denominator())
            .map_err(|_| MediaError::InvalidInput("time base denominator".into()))?;
        let ptr = NonNull::new(unsafe {
            km_encoder_open(
                runtime.0.as_ptr(),
                path.as_ptr(),
                name.as_ptr(),
                width,
                height,
                num,
                den,
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
    // Caller validates byte length against the encoder dimensions before this call.
    pub(crate) fn frame(&mut self, pixels: &[u8], pts: i64) -> Result<(), MediaError> {
        if unsafe { km_encoder_frame(self.ptr.as_ptr(), pixels.as_ptr(), pts) } < 0 {
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let error = NativeEncoder::open(
            &runtime.native,
            temp.path(),
            &codec,
            0,
            64,
            Rational::new(1, 24).unwrap(),
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
    fn km_audio_pts(a: *mut c_void) -> i64;
    fn km_audio_input_samples(a: *mut c_void) -> c_int;
    fn km_audio_count(a: *mut c_void) -> c_int;
    fn km_audio_copy(a: *mut c_void, out: *mut f32, capacity: c_int) -> c_int;
    fn km_audio_next(a: *mut c_void) -> c_int;
    fn km_audio_encode(
        k: *mut c_void,
        path: *const c_char,
        samples: *const i32,
        count: i64,
        alac: c_int,
    ) -> c_int;
    fn km_probe_open(k: *mut c_void, path: *const c_char) -> *mut c_void;
    fn km_probe_close(k: *mut c_void, format: *mut c_void);
    fn km_probe_count(format: *mut c_void) -> c_int;
    fn km_probe_stream(format: *mut c_void, index: c_int, out: *mut NativeStreamInfo);
    fn km_probe_codec(k: *mut c_void, format: *mut c_void, index: c_int) -> *const c_char;
    fn km_probe_tag(k: *mut c_void, format: *mut c_void, key: *const c_char) -> *const c_char;
    fn km_mux_av(
        k: *mut c_void,
        video: *const c_char,
        audio: *const c_char,
        path: *const c_char,
        render_hash: *const c_char,
        export_hash: *const c_char,
        profile: c_int,
    ) -> c_int;
}

pub(crate) struct AudioChunk {
    pub pts: Option<Rational>,
    pub input_samples: usize,
    pub rate: u32,
    pub channels: u32,
    pub frames: Vec<[f32; 2]>,
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
                let detail = self.runtime.error_detail();
                return Err(
                    if matches!(
                        detail.operation.as_str(),
                        "unsupported audio channel layout (mono/stereo only)"
                            | "audio format changes within stream"
                    ) {
                        MediaError::UnsupportedFeature(self.runtime.error())
                    } else {
                        MediaError::Decode(self.runtime.error())
                    },
                );
            }
            if ret == 0 {
                return Ok(None);
            }
            let count = km_audio_count(self.ptr.as_ptr());
            if !(0..=2_097_152).contains(&count) {
                return Err(MediaError::Decode("invalid audio copy size".into()));
            }
            let mut samples = vec![0.0; count as usize * 2];
            if km_audio_copy(self.ptr.as_ptr(), samples.as_mut_ptr(), count * 2) != count * 2 {
                return Err(MediaError::Decode("audio copy failed".into()));
            }
            let pts = km_audio_pts(self.ptr.as_ptr());
            let input_samples = km_audio_input_samples(self.ptr.as_ptr());
            let rate = km_audio_rate(self.ptr.as_ptr());
            let channels = km_audio_channels(self.ptr.as_ptr());
            if input_samples < 0 || rate <= 0 || !(1..=2).contains(&channels) {
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
                frames: samples.chunks_exact(2).map(|s| [s[0], s[1]]).collect(),
            }))
        }
    }
}
impl Drop for NativeAudioDecoder<'_> {
    fn drop(&mut self) {
        unsafe { km_audio_close(self.ptr.as_ptr()) }
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
    pub(crate) fn encode_audio(
        &self,
        output: &Path,
        samples: &[i32],
        alac: bool,
    ) -> Result<(), MediaError> {
        let path = path_string(output)?;
        let count = i64::try_from(samples.len() / 2)
            .map_err(|_| MediaError::InvalidInput("PCM size overflow".into()))?;
        // SAFETY: exactly two S32 samples per frame, live for the entire synchronous call.
        if unsafe {
            km_audio_encode(
                self.0.as_ptr(),
                path.as_ptr(),
                samples.as_ptr(),
                count,
                i32::from(alac),
            )
        } < 0
        {
            return Err(MediaError::Encode(self.error()));
        }
        Ok(())
    }
    pub(crate) fn probe(&self, path: &Path) -> Result<MediaProbe, MediaError> {
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
                    width: u32::try_from(info.width).ok().filter(|v| *v != 0),
                    height: u32::try_from(info.height).ok().filter(|v| *v != 0),
                });
            }
            let tag = |name: &str| {
                let key = CString::new(name).expect("constant tag without NUL");
                string(km_probe_tag(self.0.as_ptr(), ptr.as_ptr(), key.as_ptr()))
            };
            let result = MediaProbe {
                streams,
                render_snapshot_hash: tag("kronello_render_snapshot_hash"),
                export_snapshot_hash: tag("kronello_export_snapshot_hash"),
            };
            drop(probe);
            Ok(result)
        }
    }
    pub(crate) fn mux_av(
        &self,
        video: &Path,
        audio: &Path,
        output: &Path,
        render_hash: &str,
        export_hash: &str,
        profile: MovieProfile,
    ) -> Result<(), MediaError> {
        let video = path_string(video)?;
        let audio = path_string(audio)?;
        let output = path_string(output)?;
        let render_hash = CString::new(render_hash)
            .map_err(|_| MediaError::InvalidInput("hash contains NUL".into()))?;
        let export_hash = CString::new(export_hash)
            .map_err(|_| MediaError::InvalidInput("hash contains NUL".into()))?;
        // SAFETY: all strings and the runtime live throughout the synchronous mux.
        if unsafe {
            km_mux_av(
                self.0.as_ptr(),
                video.as_ptr(),
                audio.as_ptr(),
                output.as_ptr(),
                render_hash.as_ptr(),
                export_hash.as_ptr(),
                profile.native_id(),
            )
        } < 0
        {
            return Err(MediaError::Encode(self.error()));
        }
        Ok(())
    }
}
