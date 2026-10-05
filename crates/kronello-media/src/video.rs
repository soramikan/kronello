use crate::*;
use kronello_render::{DecodedVideoFrame, RenderError, VideoDecodeBackend};
use kronello_time::Rational;
use std::path::Path;

pub struct VideoDecoder<'a> {
    pub(crate) native: ffi::NativeDecoder<'a>,
    pub(crate) report: MediaPathReport,
}
impl MediaRuntime {
    /// Open a canonical local file. URL and playlist sources are rejected.
    pub fn open_video(&self, path: &Path) -> Result<VideoDecoder<'_>, MediaError> {
        let path = path.canonicalize()?;
        if !path.is_file() {
            return Err(MediaError::InvalidInput(
                "expected a local regular file".into(),
            ));
        }
        let native = ffi::NativeDecoder::open(&self.native, &path)?;
        let report = MediaPathReport {
            decoder: Some(native.name.clone()),
            encoder: None,
            execution: ExecutionKind::Software,
            input_pixel_format: String::new(),
            output_pixel_format: String::new(),
            transfer_path: "software_decode_to_cpu_native_planes".into(),
            transfers: MediaTransferStats::default(),
        };
        Ok(VideoDecoder { native, report })
    }
    /// Explicit SDR BT.709 RGBA8 straight input. HDR/linear input belongs to the
    /// color pipeline and cannot be implicitly clipped into this interface.
    pub fn encode_video(
        &self,
        request: &EncodeRequest,
        frames: &[EncodeFrame],
    ) -> Result<MediaPathReport, MediaError> {
        self.encode_video_with_capabilities(request, frames, &self.capabilities)
    }
    pub fn encode_video_with_capabilities(
        &self,
        request: &EncodeRequest,
        frames: &[EncodeFrame],
        capabilities: &MediaCapabilities,
    ) -> Result<MediaPathReport, MediaError> {
        self.encode_video_stream_with_capabilities(
            request,
            frames.len(),
            &mut |index| Ok(frames[index].clone()),
            capabilities,
        )
    }
    /// Synchronous producer backpressure: exactly one RGBA8 frame is retained.
    /// The temporary file is removed if producing, encoding or finishing fails.
    pub fn encode_video_stream(
        &self,
        request: &EncodeRequest,
        count: usize,
        frame: &mut dyn FnMut(usize) -> Result<EncodeFrame, MediaError>,
    ) -> Result<MediaPathReport, MediaError> {
        self.encode_video_stream_with_capabilities(request, count, frame, &self.capabilities)
    }
    fn encode_video_stream_with_capabilities(
        &self,
        request: &EncodeRequest,
        count: usize,
        produce: &mut dyn FnMut(usize) -> Result<EncodeFrame, MediaError>,
        capabilities: &MediaCapabilities,
    ) -> Result<MediaPathReport, MediaError> {
        let codec = capabilities.select_encoder(request.codec)?;
        let actual = self.capabilities.select_encoder(request.codec)?;
        if actual.name != codec.name {
            return Err(MediaError::EncoderUnavailable {
                encoder: codec.name.clone(),
                reason: "injected selection differs from the loaded encoder".into(),
                ffmpeg: None,
            });
        }
        let width = i32::try_from(request.width)
            .map_err(|_| MediaError::InvalidInput("width overflow".into()))?;
        let height = i32::try_from(request.height)
            .map_err(|_| MediaError::InvalidInput("height overflow".into()))?;
        if width <= 0
            || width > i32::MAX / 4
            || height <= 0
            || width % 2 != 0
            || height % 2 != 0
            || request.time_base <= Rational::ZERO
            || count == 0
        {
            return Err(MediaError::InvalidInput(
                "positive even dimensions, time base and frames required".into(),
            ));
        }
        let size = (width as usize)
            .checked_mul(height as usize)
            .and_then(|s| s.checked_mul(4))
            .ok_or_else(|| MediaError::InvalidInput("frame size overflow".into()))?;
        let parent = request
            .output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        if request.output.exists() {
            return Err(MediaError::OutputExists(request.output.clone()));
        }
        let temp = tempfile::NamedTempFile::new_in(parent)?;
        let mut encoder = ffi::NativeEncoder::open(
            &self.native,
            temp.path(),
            codec,
            width,
            height,
            request.time_base,
        )?;
        let mut previous = None;
        for index in 0..count {
            let frame = produce(index)?;
            let tick = frame.pts.checked_div(request.time_base)?;
            if frame.rgba.len() != size
                || tick.denominator() != 1
                || tick.numerator() < 0
                || previous.is_some_and(|p| frame.pts <= p)
                || frame.rgba.chunks_exact(4).any(|p| p[3] != 255)
            {
                return Err(MediaError::InvalidInput(
                    "opaque RGBA length and strictly increasing integral nonnegative PTS required"
                        .into(),
                ));
            }
            previous = Some(frame.pts);
            encoder.frame(&frame.rgba, tick.numerator())?;
        }
        encoder.finish()?;
        let pixel_format = encoder.pixel_format.clone();
        let converted_bytes = encoder
            .frame_size
            .checked_mul(count as u64)
            .ok_or_else(|| MediaError::InvalidInput("transfer counter overflow".into()))?;
        drop(encoder);
        temp.as_file().sync_all()?;
        temp.persist_noclobber(&request.output).map_err(|e| {
            if e.error.kind() == std::io::ErrorKind::AlreadyExists {
                MediaError::OutputExists(request.output.clone())
            } else {
                MediaError::Io(e.error)
            }
        })?;
        let bytes = u64::try_from(size)
            .ok()
            .and_then(|s| s.checked_mul(count as u64))
            .ok_or_else(|| MediaError::InvalidInput("transfer counter overflow".into()))?;
        Ok(MediaPathReport {
            decoder: None,
            encoder: Some(codec.name.clone()),
            execution: if codec.hardware {
                ExecutionKind::Hardware
            } else {
                ExecutionKind::Software
            },
            input_pixel_format: "rgba".into(),
            output_pixel_format: pixel_format,
            transfer_path: if codec.hardware {
                "cpu_rgba_to_hardware_encoder"
            } else {
                "cpu_rgba_to_software_encoder"
            }
            .into(),
            transfers: MediaTransferStats {
                cpu_copy_bytes: 0,
                cpu_conversion_input_bytes: bytes,
                cpu_conversion_output_bytes: converted_bytes,
                cpu_upload_bytes: if codec.hardware { converted_bytes } else { 0 },
                ..Default::default()
            },
        })
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EncodeRequest {
    pub output: PathBuf,
    pub codec: EncodeCodec,
    pub width: u32,
    pub height: u32,
    pub time_base: Rational,
}
#[derive(Debug, Clone)]
pub struct EncodeFrame {
    /// Exact presentation time; each frame spans one EncodeRequest.time_base tick.
    pub pts: Rational,
    pub rgba: Vec<u8>,
}
impl VideoDecoder<'_> {
    pub fn stream_metadata(&mut self) -> Result<kronello_model::StreamMetadata, MediaError> {
        self.native.seek(self.native.origin())?;
        let frame = self
            .next()?
            .ok_or_else(|| MediaError::Decode("empty video stream".into()))?;
        let [
            pixel_format,
            color_primaries,
            color_transfer,
            color_matrix,
            color_range,
        ] = frame.labels;
        Ok(kronello_model::StreamMetadata {
            index: self.native.stream(),
            codec: self.native.name.clone(),
            time_base: self.native.time_base,
            duration: self.native.duration()?,
            start_time: Some(frame.pts),
            width: Some(frame.width),
            height: Some(frame.height),
            pixel_format: Some(pixel_format),
            color_primaries: Some(color_primaries),
            color_transfer: Some(color_transfer),
            color_matrix: Some(color_matrix),
            color_range: Some(color_range),
        })
    }
    pub fn path_report(&self) -> &MediaPathReport {
        &self.report
    }
    /// Conservative exact seek: reset to the first indexed presentation point
    /// and decode forward. This avoids assuming a VFR frame duration or a GOP
    /// preroll bound. Keyframe acceleration is deferred, never a correctness fallback.
    pub fn decode_at(&mut self, time: Rational) -> Result<DecodedVideoFrame, MediaError> {
        self.native.seek(self.native.origin())?;
        let mut current = self.next()?;
        loop {
            let Some(frame) = current else {
                return Err(MediaError::FrameNotFound(format!("{time:?}")));
            };
            if frame.pts > time {
                return Err(MediaError::FrameNotFound(format!("{time:?}")));
            }
            let next = self.next()?;
            let end = match &next {
                Some(n) if n.pts > frame.pts => n.pts,
                Some(_) => {
                    return Err(MediaError::Decode(
                        "non-increasing presentation timestamps".into(),
                    ));
                }
                None if frame.duration > Rational::ZERO => frame.pts.checked_add(frame.duration)?,
                None => {
                    return Err(MediaError::Decode(
                        "last frame has no presentation duration".into(),
                    ));
                }
            };
            if time < end {
                let [
                    pixel_format,
                    color_primaries,
                    color_transfer,
                    color_matrix,
                    color_range,
                ] = frame.labels;
                return Ok(DecodedVideoFrame {
                    pts: frame.pts,
                    end,
                    width: frame.width,
                    height: frame.height,
                    pixel_format,
                    color_primaries,
                    color_transfer,
                    color_matrix,
                    color_range,
                    pixels: frame.pixels,
                });
            }
            current = next;
        }
    }
    fn next(&mut self) -> Result<Option<ffi::RawFrame>, MediaError> {
        let frame = self.native.next()?;
        if let Some(f) = &frame {
            self.report.input_pixel_format = f.labels[0].clone();
            self.report.output_pixel_format = f.labels[0].clone();
            self.report.transfers.cpu_copy_bytes = self
                .report
                .transfers
                .cpu_copy_bytes
                .checked_add(f.pixels.len() as u64)
                .ok_or_else(|| MediaError::Decode("transfer counter overflow".into()))?;
        }
        Ok(frame)
    }
}
impl VideoDecodeBackend for VideoDecoder<'_> {
    fn frame_at(&mut self, time: Rational) -> Result<DecodedVideoFrame, RenderError> {
        self.decode_at(time).map_err(|e| RenderError::Backend {
            code: e.code(),
            message: e.to_string(),
        })
    }
}
