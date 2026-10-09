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
/// Owns its native decoder; the shared `Rc` runtime inside `NativeDecoder`
/// keeps the loaded library alive for the decoder's exact span.
pub struct VideoDecoder {
    pub(crate) native: ffi::NativeDecoder,
    pub(crate) report: MediaPathReport,
    pub(crate) stats: VideoDecodeStats,
    pub(crate) current: Option<DecodedVideoFrame>,
    pub(crate) lookahead: Option<ffi::RawFrame>,
}
impl MediaRuntime {
    /// Open a canonical local file. URL and playlist sources are rejected.
    /// Detected camera RAW containers are rejected before FFmpeg sees them:
    /// stills decode through LibRaw, CinemaDNG through the frame sequence,
    /// ProRes RAW through the macOS native path, and BRAW/R3D never decode.
    pub fn open_video(&self, path: &Path) -> Result<VideoDecoder, MediaError> {
        let path = path.canonicalize()?;
        if !path.is_file() {
            return Err(MediaError::InvalidInput(
                "expected a local regular file".into(),
            ));
        }
        if let Some(detection) = crate::raw::sniff_camera_raw(&path)? {
            return Err(detection.unsupported());
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
            || width & 1 != 0
            || height & 1 != 0
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
        let mut pending: Option<(EncodeFrame, i64)> = None;
        let mut last_span = 1i64;
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
            let tick = tick.numerator();
            // Packet spans are the distance to the next submitted PTS so
            // non-unit tick spacing survives into the container duration;
            // the final frame inherits the last observed span.
            if let Some((frame, pts)) = pending.replace((frame, tick)) {
                last_span = tick - pts;
                encoder.frame(&frame.rgba, pts, last_span)?;
            }
        }
        if let Some((frame, pts)) = pending {
            encoder.frame(&frame.rgba, pts, last_span)?;
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
impl VideoDecoder {
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
        let result = self.decode_at_inner(time, false);
        if result.is_err() {
            self.current = None;
            self.lookahead = None;
        }
        result
    }
    /// Reverse endpoint selection uses the exact presentation interval (pts, end].
    /// No nominal frame rate or epsilon is used, including variable-rate media.
    pub fn decode_at_reverse(&mut self, time: Rational) -> Result<DecodedVideoFrame, MediaError> {
        let result = self.decode_at_inner(time, true);
        if result.is_err() {
            self.current = None;
            self.lookahead = None;
        }
        result
    }
    fn decode_at_inner(
        &mut self,
        time: Rational,
        reverse: bool,
    ) -> Result<DecodedVideoFrame, MediaError> {
        if let Some(frame) = &self.current {
            if if reverse {
                time > frame.pts && time <= frame.end
            } else {
                time >= frame.pts && time < frame.end
            } {
                let cloned = frame.clone();
                let bytes = cloned.pixels.len() as u64;
                self.stats.interval_hits += 1;
                self.stats.returned_clone_bytes += bytes;
                self.record_copy(bytes)?;
                return Ok(cloned);
            }
            if time < frame.pts || (reverse && time == frame.pts) {
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
            if frame.pts > time || (reverse && frame.pts == time) {
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
            if time < end || (reverse && time == end) {
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
/// Explicit SDR BT.709 RGBA8 frame for bounded offline analysis/transcoding.
/// Produced by the same color path as `decode_video_image`; HDR sources are
/// rejected by the shared `video_color_policy` gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbaVideoFrame {
    pub pts: Rational,
    pub duration: Rational,
    pub width: u32,
    pub height: u32,
    /// Straight opaque RGBA8, `width * height * 4` bytes.
    pub rgba: Vec<u8>,
}
impl VideoDecoder {
    /// Decode the next presentation-order frame into explicit SDR RGBA8.
    /// Deterministic: integer luma-friendly output, no clock or device input.
    pub fn next_rgba(&mut self) -> Result<Option<RgbaVideoFrame>, MediaError> {
        let frame = self.next()?;
        let Some(frame) = frame else {
            return Ok(None);
        };
        let [
            pixel_format,
            color_primaries,
            color_transfer,
            color_matrix,
            color_range,
        ] = frame.labels.clone();
        let metadata = kronello_model::StreamMetadata {
            index: self.native.stream(),
            codec: String::new(),
            time_base: self.native.time_base,
            duration: None,
            start_time: None,
            width: Some(frame.width),
            height: Some(frame.height),
            pixel_format: Some(pixel_format),
            color_primaries: Some(color_primaries),
            color_transfer: Some(color_transfer),
            color_matrix: Some(color_matrix),
            color_range: Some(color_range),
        };
        let policy = video_color_policy(&metadata)?;
        let decoded = DecodedVideoFrame {
            pts: frame.pts,
            end: frame.pts.checked_add(frame.duration).unwrap_or(frame.pts),
            width: frame.width,
            height: frame.height,
            pixel_format: metadata.pixel_format.clone().unwrap_or_default(),
            color_primaries: metadata.color_primaries.clone().unwrap_or_default(),
            color_transfer: metadata.color_transfer.clone().unwrap_or_default(),
            color_matrix: metadata.color_matrix.clone().unwrap_or_default(),
            color_range: metadata.color_range.clone().unwrap_or_default(),
            pixels: frame.pixels,
        };
        let rgba = ffi::video_rgba(self.native.runtime(), &decoded, policy.range == "pc")?;
        Ok(Some(RgbaVideoFrame {
            pts: frame.pts,
            duration: frame.duration,
            width: decoded.width,
            height: decoded.height,
            rgba,
        }))
    }
    /// Decode the frame whose presentation interval covers `time` into explicit
    /// SDR RGBA8. Deterministic fixed-snapshot preview used for thumbnails
    /// (FLOW-002, ADR-0129): same timestamp always yields the same frame and
    /// the same color path as `next_rgba`.
    pub fn rgba_at(&mut self, time: Rational) -> Result<RgbaVideoFrame, MediaError> {
        let decoded = self.decode_at(time)?;
        let metadata = kronello_model::StreamMetadata {
            index: self.native.stream(),
            codec: String::new(),
            time_base: self.native.time_base,
            duration: None,
            start_time: None,
            width: Some(decoded.width),
            height: Some(decoded.height),
            pixel_format: Some(decoded.pixel_format.clone()),
            color_primaries: Some(decoded.color_primaries.clone()),
            color_transfer: Some(decoded.color_transfer.clone()),
            color_matrix: Some(decoded.color_matrix.clone()),
            color_range: Some(decoded.color_range.clone()),
        };
        let policy = video_color_policy(&metadata)?;
        let rgba = ffi::video_rgba(self.native.runtime(), &decoded, policy.range == "pc")?;
        let duration = decoded.end.checked_sub(decoded.pts)?;
        Ok(RgbaVideoFrame {
            pts: decoded.pts,
            duration,
            width: decoded.width,
            height: decoded.height,
            rgba,
        })
    }
}
/// Deterministic fixed-point bilinear scale of packed opaque RGBA8.
/// Integer arithmetic only; output alpha is always 255.
pub fn scale_rgba8(
    source: &[u8],
    width: u32,
    height: u32,
    dest_width: u32,
    dest_height: u32,
) -> Result<Vec<u8>, MediaError> {
    let (sw, sh, dw, dh) = (
        width as usize,
        height as usize,
        dest_width as usize,
        dest_height as usize,
    );
    if sw == 0
        || sh == 0
        || dw == 0
        || dh == 0
        || source.len()
            != sw
                .checked_mul(sh)
                .and_then(|s| s.checked_mul(4))
                .ok_or(MediaError::InvalidInput("source size overflow".into()))?
        || dw.checked_mul(dh).is_none()
    {
        return Err(MediaError::InvalidInput(
            "scale dimensions/source size".into(),
        ));
    }
    if dw == sw && dh == sh {
        return Ok(source.to_vec());
    }
    let mut out = vec![0u8; dw * dh * 4];
    // 16.16 fixed-point edge-to-edge mapping: dst x maps onto src pixel
    // coordinate `x * (sw - 1) / (dw - 1)`; degenerate axes stay at 0.
    let axis = |i: usize, dst: usize, src: usize| -> u64 {
        if dst <= 1 || src <= 1 {
            0
        } else {
            ((i as u64 * (src as u64 - 1)) << 16) / (dst as u64 - 1)
        }
    };
    for y in 0..dh {
        let sy = axis(y, dh, sh);
        let y0 = (sy >> 16) as usize;
        let y1 = (y0 + 1).min(sh - 1);
        let fy = sy & 0xFFFF;
        for x in 0..dw {
            let sx = axis(x, dw, sw);
            let x0 = (sx >> 16) as usize;
            let x1 = (x0 + 1).min(sw - 1);
            let fx = sx & 0xFFFF;
            let at =
                |px: usize, py: usize, c: usize| -> u64 { source[(py * sw + px) * 4 + c] as u64 };
            for c in 0..3 {
                let top = at(x0, y0, c) * (0x10000 - fx) + at(x1, y0, c) * fx;
                let bot = at(x0, y1, c) * (0x10000 - fx) + at(x1, y1, c) * fx;
                out[(y * dw + x) * 4 + c] = ((top * (0x10000 - fy) + bot * fy) >> 32) as u8;
            }
            out[(y * dw + x) * 4 + 3] = 255;
        }
    }
    Ok(out)
}
impl VideoDecodeBackend for VideoDecoder {
    fn frame_at(&mut self, time: Rational) -> Result<DecodedVideoFrame, RenderError> {
        self.decode_at(time).map_err(|e| RenderError::Backend {
            code: e.code(),
            message: e.to_string(),
        })
    }
}
