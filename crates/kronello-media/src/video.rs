use crate::*;
use kronello_render::{DecodedVideoFrame, RenderError, VideoDecodeBackend};
use kronello_time::Rational;
use std::path::Path;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct VideoDecodeStats {
    pub seeks: u64,
    pub decoded_frames: u64,
    pub interval_hits: u64,
    pub peak_cached_frame_bytes: u64,
    pub cache_clone_bytes: u64,
    pub returned_clone_bytes: u64,
}
pub struct VideoDecoder<'a> {
    pub(crate) native: ffi::NativeDecoder<'a>,
    pub(crate) report: MediaPathReport,
    pub(crate) stats: VideoDecodeStats,
    pub(crate) current: Option<DecodedVideoFrame>,
    pub(crate) lookahead: Option<ffi::RawFrame>,
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
        Ok(VideoDecoder {
            native,
            report,
            stats: VideoDecodeStats::default(),
            current: None,
            lookahead: None,
        })
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
            None,
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
        self.encode_video_stream_with_capabilities(request, count, frame, &self.capabilities, None)
    }
    /// Explicit straight Rec.2100 RGBA64LE input; no 8-bit intermediary.
    pub fn encode_hdr_video_stream(
        &self,
        request: &EncodeRequest,
        count: usize,
        transfer: kronello_render::HdrTransfer,
        frame: &mut dyn FnMut(usize) -> Result<EncodeFrame, MediaError>,
    ) -> Result<MediaPathReport, MediaError> {
        if request.codec != EncodeCodec::ProRes {
            return Err(MediaError::UnsupportedFeature(
                "HDR encoder profile supports only ProRes 10-bit".into(),
            ));
        }
        self.encode_video_stream_with_capabilities(
            request,
            count,
            frame,
            &self.capabilities,
            Some(transfer),
        )
    }
    fn encode_video_stream_with_capabilities(
        &self,
        request: &EncodeRequest,
        count: usize,
        produce: &mut dyn FnMut(usize) -> Result<EncodeFrame, MediaError>,
        capabilities: &MediaCapabilities,
        hdr: Option<kronello_render::HdrTransfer>,
    ) -> Result<MediaPathReport, MediaError> {
        let stride = if hdr.is_some() { 8 } else { 4 };
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
            || width > i32::MAX / stride
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
            .and_then(|s| s.checked_mul(stride as usize))
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
        let mut encoder = ffi::NativeEncoder::open_color(
            &self.native,
            temp.path(),
            codec,
            width,
            height,
            request.time_base,
            hdr,
        )?;
        let mut previous = None;
        for index in 0..count {
            let frame = produce(index)?;
            let tick = frame.pts.checked_div(request.time_base)?;
            if frame.rgba.len() != size
                || tick.denominator() != 1
                || tick.numerator() < 0
                || previous.is_some_and(|p| frame.pts <= p)
                || frame.rgba.chunks_exact(stride as usize).any(|p| {
                    if hdr.is_some() {
                        p[6] != 255 || p[7] != 255
                    } else {
                        p[3] != 255
                    }
                })
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
            input_pixel_format: if hdr.is_some() { "rgba64le" } else { "rgba" }.into(),
            output_pixel_format: pixel_format,
            transfer_path: if hdr.is_some() {
                "cpu_rec2100_rgba64_to_prores_10bit"
            } else if codec.hardware {
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
    pub(crate) fn cached_frame_bytes(&self) -> usize {
        self.current.as_ref().map_or(0, |f| f.pixels.len())
            + self.lookahead.as_ref().map_or(0, |f| f.pixels.len())
    }
    pub fn decode_stats(&self) -> VideoDecodeStats {
        self.stats
    }
    fn seek_origin(&mut self) -> Result<(), MediaError> {
        self.current = None;
        self.lookahead = None;
        self.native.restart_origin()?;
        self.stats.seeks += 1;
        Ok(())
    }
    pub fn stream_metadata(&mut self) -> Result<kronello_model::StreamMetadata, MediaError> {
        self.seek_origin()?;
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
    /// Exact presentation intervals with bounded forward state. Backward requests
    /// restart at the indexed origin; no GOP or VFR duration approximation is used.
    pub fn decode_at(&mut self, time: Rational) -> Result<DecodedVideoFrame, MediaError> {
        let result = self.decode_at_inner(time);
        if result.is_err() {
            self.current = None;
            self.lookahead = None;
        }
        result
    }
    fn decode_at_inner(&mut self, time: Rational) -> Result<DecodedVideoFrame, MediaError> {
        if let Some(frame) = &self.current {
            if time >= frame.pts && time < frame.end {
                let cloned = frame.clone();
                let bytes = cloned.pixels.len() as u64;
                self.stats.interval_hits += 1;
                self.stats.returned_clone_bytes += bytes;
                self.record_copy(bytes)?;
                return Ok(cloned);
            }
            if time < frame.pts {
                self.seek_origin()?;
            }
        } else {
            self.seek_origin()?;
        }
        let mut current = if self.current.take().is_some() {
            self.lookahead.take()
        } else {
            self.next()?
        };
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
                let decoded = DecodedVideoFrame {
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
                };
                let bytes = decoded.pixels.len() as u64
                    + next.as_ref().map_or(0, |n| n.pixels.len() as u64);
                self.stats.peak_cached_frame_bytes = self.stats.peak_cached_frame_bytes.max(bytes);
                self.lookahead = next;
                self.stats.cache_clone_bytes += decoded.pixels.len() as u64;
                self.record_copy(decoded.pixels.len() as u64)?;
                self.current = Some(decoded.clone());
                return Ok(decoded);
            }
            current = next;
        }
    }
    fn record_copy(&mut self, bytes: u64) -> Result<(), MediaError> {
        self.report.transfers.cpu_copy_bytes = self
            .report
            .transfers
            .cpu_copy_bytes
            .checked_add(bytes)
            .ok_or_else(|| MediaError::Decode("transfer counter overflow".into()))?;
        Ok(())
    }
    fn next(&mut self) -> Result<Option<ffi::RawFrame>, MediaError> {
        let frame = self.native.next()?;
        if let Some(f) = &frame {
            self.stats.decoded_frames += 1;
            self.report.input_pixel_format = f.labels[0].clone();
            self.report.output_pixel_format = f.labels[0].clone();
            self.record_copy(f.pixels.len() as u64)?;
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
