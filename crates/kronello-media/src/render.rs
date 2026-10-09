//! Explicit software decode, SDR color conversion and CPU sampling boundary.
use crate::*;
use kronello_model::{Asset, ColorSpace, StreamMetadata};
use kronello_render::{
    BackendFrame, DecodedVideoFrame, RenderBackend, RenderCache, RenderDag, RenderError, VideoImage,
};
use kronello_time::Time;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VideoColorPolicy {
    pub primaries: String,
    pub transfer: String,
    pub matrix: String,
    pub range: String,
    /// Missing tags are visible assumptions, never inferred from image pixels.
    pub assumptions: Vec<String>,
}
fn absent(tag: Option<&str>) -> bool {
    tag.is_none_or(|t| matches!(t, "" | "unknown" | "unspecified" | "N/A"))
}
pub fn video_color_policy(stream: &StreamMetadata) -> Result<VideoColorPolicy, MediaError> {
    let format = stream
        .pixel_format
        .as_deref()
        .ok_or_else(|| MediaError::UnsupportedFeature("video pixel format missing".into()))?;
    let rgb = matches!(format, "rgb24" | "bgr24" | "rgba" | "bgra" | "gbrp");
    if !rgb
        && !matches!(
            format,
            "yuv420p" | "yuv422p" | "yuv444p" | "yuv420p10le" | "yuv422p10le" | "yuv444p10le"
        )
    {
        return Err(MediaError::UnsupportedFeature(format!(
            "video format/alpha {format}"
        )));
    }
    let mut assumptions = vec![];
    let mut tag = |raw: Option<&str>, name: &str, default: &str| {
        if absent(raw) {
            assumptions.push(format!("untagged {name}: {default}"));
            default.to_string()
        } else {
            raw.expect("present tag").to_string()
        }
    };
    let primaries = tag(stream.color_primaries.as_deref(), "primaries", "bt709");
    let transfer = tag(
        stream.color_transfer.as_deref(),
        "transfer",
        if rgb { "iec61966-2-1" } else { "bt709" },
    );
    let matrix = tag(
        stream.color_matrix.as_deref(),
        "matrix",
        if rgb { "gbr" } else { "bt709" },
    );
    let range = tag(
        stream.color_range.as_deref(),
        "range",
        if rgb { "pc" } else { "tv" },
    );
    if primaries != "bt709"
        || !matches!(transfer.as_str(), "bt709" | "iec61966-2-1")
        || matrix != if rgb { "gbr" } else { "bt709" }
        || !matches!(range.as_str(), "pc" | "tv")
        || (rgb && range != "pc")
    {
        return Err(MediaError::UnsupportedFeature(
            "video color tags (only SDR BT.709/sRGB supported)".into(),
        ));
    }
    Ok(VideoColorPolicy {
        primaries,
        transfer,
        matrix,
        range,
        assumptions,
    })
}

fn hdr_video_color_policy(stream: &StreamMetadata) -> Result<VideoColorPolicy, MediaError> {
    let format = stream.pixel_format.as_deref().unwrap_or("");
    if stream.color_primaries.as_deref() != Some("bt2020")
        || !matches!(
            stream.color_transfer.as_deref(),
            Some("smpte2084" | "arib-std-b67")
        )
        || stream.color_matrix.as_deref() != Some("bt2020nc")
        || !matches!(stream.color_range.as_deref(), Some("tv" | "pc"))
        || !matches!(format, "yuv420p10le" | "yuv422p10le" | "yuv444p10le")
    {
        return Err(MediaError::UnsupportedFeature(
            "HDR requires explicit Rec.2020 PQ/HLG 10-bit native YUV tags".into(),
        ));
    }
    Ok(VideoColorPolicy {
        primaries: "bt2020".into(),
        transfer: stream.color_transfer.clone().unwrap(),
        matrix: "bt2020nc".into(),
        range: stream.color_range.clone().unwrap(),
        assumptions: vec![],
    })
}

impl MediaRuntime {
    pub fn open_video_stream(
        &self,
        path: &Path,
        stream_index: u32,
    ) -> Result<VideoDecoder, MediaError> {
        let path = path.canonicalize()?;
        if !path.is_file() {
            return Err(MediaError::InvalidInput(
                "expected a local regular file".into(),
            ));
        }
        if let Some(detection) = crate::raw::sniff_camera_raw(&path)? {
            return Err(detection.unsupported());
        }
        let native = ffi::NativeDecoder::open_stream(&self.native, &path, Some(stream_index))?;
        if native.stream() != stream_index {
            return Err(MediaError::UnsupportedFeature(
                "requested video stream was not selected".into(),
            ));
        }
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
    pub fn decode_video_image(
        &self,
        asset: &Asset,
        project_path: &Path,
        stream_index: u32,
        time: Time,
        working: ColorSpace,
    ) -> Result<VideoImage, MediaError> {
        self.decode_video_image_with_hdr(asset, project_path, stream_index, time, working, None)
    }
    pub fn decode_video_image_with_hdr(
        &self,
        asset: &Asset,
        project_path: &Path,
        stream_index: u32,
        time: Time,
        working: ColorSpace,
        hdr: Option<kronello_render::HdrSettings>,
    ) -> Result<VideoImage, MediaError> {
        self.decode_video_image_with_sampling(
            asset,
            project_path,
            stream_index,
            time,
            working,
            hdr,
            false,
            None,
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub fn decode_video_image_with_sampling(
        &self,
        asset: &Asset,
        project_path: &Path,
        stream_index: u32,
        time: Time,
        working: ColorSpace,
        hdr: Option<kronello_render::HdrSettings>,
        reverse_sampling: bool,
        interpolation: Option<kronello_time::FrameInterpolation>,
    ) -> Result<VideoImage, MediaError> {
        // Camera RAW video codecs own their decode boundary (ADR-0136): a
        // CinemaDNG asset's content hash is a frame manifest, and ProRes RAW
        // never reaches the FFmpeg decoder.
        match asset
            .streams
            .iter()
            .find(|s| s.index == stream_index)
            .map(|s| s.codec.as_str())
        {
            Some(crate::raw::CINEMADNG_CODEC) => {
                return crate::raw::decode_cinemadng_image(
                    asset,
                    project_path,
                    stream_index,
                    time,
                    working,
                    reverse_sampling,
                    interpolation,
                );
            }
            Some(codec) if crate::raw::PRORES_RAW_CODECS.contains(&codec) => {
                return crate::raw::decode_prores_raw_image(
                    asset,
                    project_path,
                    stream_index,
                    time,
                    working,
                    reverse_sampling,
                    interpolation,
                );
            }
            _ => {}
        }
        let path = self.resolve_verified(asset, project_path)?;
        let mut decoder = self.open_video_stream(&path, stream_index)?;
        self.decode_video_image_from_decoder(
            &mut decoder,
            asset,
            project_path,
            stream_index,
            time,
            working,
            hdr,
            reverse_sampling,
            interpolation,
        )
    }
    /// Shared decode prelude: resolve the asset, present the frame containing
    /// `time`, verify the decoded frame still satisfies the asset's locked
    /// stream contract, and pin the applicable color policy.
    #[allow(clippy::too_many_arguments)]
    fn prepare_video_frame(
        &self,
        decoder: &mut VideoDecoder,
        asset: &Asset,
        project_path: &Path,
        stream_index: u32,
        time: Time,
        working: ColorSpace,
        hdr: Option<kronello_render::HdrSettings>,
        reverse_sampling: bool,
    ) -> Result<(DecodedVideoFrame, VideoColorPolicy, bool), MediaError> {
        let timing = std::env::var_os("KRONELLO_MEDIA_TIMING").is_some();
        let t0 = timing.then(std::time::Instant::now);
        self.resolve_verified(asset, project_path)?;
        let t1 = timing.then(std::time::Instant::now);
        let frame = if reverse_sampling {
            decoder.decode_at_reverse(time)?
        } else {
            decoder.decode_at(time)?
        };
        if let (Some(t0), Some(t1)) = (t0, t1) {
            eprintln!(
                "    prepare: resolve_asset={:.2}ms decode_at={:.2}ms",
                t1.duration_since(t0).as_secs_f64() * 1000.0,
                t1.elapsed().as_secs_f64() * 1000.0
            );
        }
        let metadata = StreamMetadata {
            index: stream_index,
            codec: String::new(),
            time_base: decoder.native.time_base,
            duration: None,
            start_time: None,
            width: Some(frame.width),
            height: Some(frame.height),
            pixel_format: Some(frame.pixel_format.clone()),
            color_primaries: Some(frame.color_primaries.clone()),
            color_transfer: Some(frame.color_transfer.clone()),
            color_matrix: Some(frame.color_matrix.clone()),
            color_range: Some(frame.color_range.clone()),
        };
        let locked = asset
            .streams
            .iter()
            .find(|s| s.index == stream_index)
            .ok_or_else(|| MediaError::InvalidInput("video stream lock missing".into()))?;
        if locked.width != Some(frame.width)
            || locked.height != Some(frame.height)
            || locked.pixel_format.as_deref() != Some(frame.pixel_format.as_str())
        {
            return Err(MediaError::InvalidInput(
                "decoded video dimensions/native format differ from locked metadata".into(),
            ));
        }
        let is_hdr = matches!(
            locked.color_transfer.as_deref(),
            Some("smpte2084" | "arib-std-b67")
        );
        if is_hdr && (hdr.is_none() || working != ColorSpace::LinearRec2020) {
            return Err(MediaError::UnsupportedFeature(
                "HDR video requires explicit pinned HDR LinearRec2020 profile".into(),
            ));
        }
        let policy_fn = if is_hdr {
            hdr_video_color_policy
        } else {
            video_color_policy
        };
        let pinned = policy_fn(locked)?;
        let policy = policy_fn(&metadata)?;
        if (
            pinned.primaries.as_str(),
            pinned.transfer.as_str(),
            pinned.matrix.as_str(),
            pinned.range.as_str(),
        ) != (
            policy.primaries.as_str(),
            policy.transfer.as_str(),
            policy.matrix.as_str(),
            policy.range.as_str(),
        ) {
            return Err(MediaError::UnsupportedFeature(
                "decoded video color differs from locked color contract".into(),
            ));
        }
        Ok((frame, policy, is_hdr))
    }
    /// The rgba64 working-space conversion shared by the eager and lazy paths.
    fn rgba64_image(
        &self,
        frame: &DecodedVideoFrame,
        asset: &Asset,
        project_path: &Path,
        policy: &VideoColorPolicy,
        working: ColorSpace,
        is_hdr: bool,
    ) -> Result<VideoImage, MediaError> {
        let rgba = ffi::video_rgba64(&self.native, frame, policy.range == "pc", is_hdr)?;
        self.resolve_verified(asset, project_path)?;
        let transfer = if policy.transfer == "smpte2084" {
            kronello_render::HdrTransfer::Pq
        } else {
            kronello_render::HdrTransfer::Hlg
        };
        let pixels = rgba
            .chunks_exact(8)
            .map(|p| {
                let code = [0, 1, 2]
                    .map(|i| f64::from(u16::from_le_bytes([p[2 * i], p[2 * i + 1]])) / 65535.0);
                let mut rgb = if is_hdr {
                    transfer.decode(code)
                } else {
                    code.map(|v| {
                        if policy.transfer == "iec61966-2-1" {
                            if v <= 0.04045 {
                                v / 12.92
                            } else {
                                ((v + 0.055) / 1.055).powf(2.4)
                            }
                        } else if v < 0.081 {
                            v / 4.5
                        } else {
                            ((v + 0.099) / 1.099).powf(1.0 / 0.45)
                        }
                    })
                };
                if !is_hdr && working == ColorSpace::LinearRec2020 {
                    rgb = [
                        0.6274039 * rgb[0] + 0.3292830 * rgb[1] + 0.0433131 * rgb[2],
                        0.0690973 * rgb[0] + 0.9195404 * rgb[1] + 0.0113623 * rgb[2],
                        0.0163914 * rgb[0] + 0.0880133 * rgb[1] + 0.8955953 * rgb[2],
                    ];
                }
                let alpha = f64::from(u16::from_le_bytes([p[6], p[7]])) / 65535.0;
                [
                    (rgb[0] * alpha) as f32,
                    (rgb[1] * alpha) as f32,
                    (rgb[2] * alpha) as f32,
                    alpha as f32,
                ]
            })
            .collect();
        Ok(VideoImage {
            size: [frame.width, frame.height],
            pixels,
        })
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn decode_video_image_from_decoder(
        &self,
        decoder: &mut VideoDecoder,
        asset: &Asset,
        project_path: &Path,
        stream_index: u32,
        time: Time,
        working: ColorSpace,
        hdr: Option<kronello_render::HdrSettings>,
        reverse_sampling: bool,
        interpolation: Option<kronello_time::FrameInterpolation>,
    ) -> Result<VideoImage, MediaError> {
        let (frame, policy, is_hdr) = self.prepare_video_frame(
            decoder,
            asset,
            project_path,
            stream_index,
            time,
            working,
            hdr,
            reverse_sampling,
        )?;
        // TRACK-003 (ADR-0123): optical-flow synthesis replaces the
        // nearest-frame sample; v1 restricts it to SDR forward sampling.
        if interpolation.is_some() {
            if reverse_sampling {
                return Err(MediaError::UnsupportedFeature(
                    "optical-flow interpolation requires forward sampling".into(),
                ));
            }
            if is_hdr || hdr.is_some() {
                return Err(MediaError::UnsupportedFeature(
                    "optical-flow interpolation requires an SDR source".into(),
                ));
            }
            return self.flow_image(decoder, &frame, time, interpolation, &policy, working);
        }
        if hdr.is_some() {
            return self.rgba64_image(&frame, asset, project_path, &policy, working, is_hdr);
        }
        let rgba = ffi::video_rgba(&self.native, &frame, policy.range == "pc")?;
        self.resolve_verified(asset, project_path)?;
        let pixels = sdr_working_pixels(&rgba, &policy, working);
        if working == ColorSpace::Srgb {
            return Err(MediaError::UnsupportedFeature(
                "nonlinear video working space".into(),
            ));
        }
        Ok(VideoImage {
            size: [frame.width, frame.height],
            pixels,
        })
    }
    /// Source-producing variant of `decode_video_image_from_decoder`: the SDR
    /// path keeps the decoded frame as packed RGBA8 and converts only sampled
    /// output pixels. The returned identity is a serialized content address
    /// for the decoded image; callers add the node's sampling parameters to
    /// form a raster cache identity.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn decode_video_source_from_decoder(
        &self,
        decoder: &mut VideoDecoder,
        asset: &Asset,
        project_path: &Path,
        stream_index: u32,
        time: Time,
        working: ColorSpace,
        hdr: Option<kronello_render::HdrSettings>,
        reverse_sampling: bool,
        interpolation: Option<kronello_time::FrameInterpolation>,
    ) -> Result<SourcedVideo, MediaError> {
        let timing = std::env::var_os("KRONELLO_MEDIA_TIMING").is_some();
        let t0 = timing.then(std::time::Instant::now);
        let (frame, policy, is_hdr) = self.prepare_video_frame(
            decoder,
            asset,
            project_path,
            stream_index,
            time,
            working,
            hdr,
            reverse_sampling,
        )?;
        let t1 = timing.then(std::time::Instant::now);
        let source = if interpolation.is_some() {
            if reverse_sampling {
                return Err(MediaError::UnsupportedFeature(
                    "optical-flow interpolation requires forward sampling".into(),
                ));
            }
            if is_hdr || hdr.is_some() {
                return Err(MediaError::UnsupportedFeature(
                    "optical-flow interpolation requires an SDR source".into(),
                ));
            }
            kronello_render::VideoSource::from_image(self.flow_image(
                decoder,
                &frame,
                time,
                interpolation,
                &policy,
                working,
            )?)
        } else if hdr.is_some() {
            kronello_render::VideoSource::from_image(self.rgba64_image(
                &frame,
                asset,
                project_path,
                &policy,
                working,
                is_hdr,
            )?)
        } else {
            if working == ColorSpace::Srgb {
                return Err(MediaError::UnsupportedFeature(
                    "nonlinear video working space".into(),
                ));
            }
            let rgba = ffi::video_rgba(&self.native, &frame, policy.range == "pc")?;
            self.resolve_verified(asset, project_path)?;
            sdr_video_source(rgba, frame.width, frame.height, &policy, working)
        };
        if let (Some(t0), Some(t1)) = (t0, t1) {
            eprintln!(
                "  video_source: decode={:.2}ms convert={:.2}ms fmt={}",
                t1.duration_since(t0).as_secs_f64() * 1000.0,
                t1.elapsed().as_secs_f64() * 1000.0,
                frame.pixel_format
            );
        }
        // Everything the decoded image depends on: the decoder implementation
        // (runtime + codec), the locked source identity, the authored request,
        // and the presented frame actually decoded.
        let identity = serde_json::json!([
            "ffmpeg-video-source-v1",
            self.capabilities.ffmpeg_version,
            decoder.native.name,
            asset.content_hash,
            stream_index,
            time,
            reverse_sampling,
            interpolation,
            frame.pts,
            frame.end,
            frame.width,
            frame.height,
            frame.pixel_format,
            frame.color_primaries,
            frame.color_transfer,
            frame.color_matrix,
            frame.color_range,
            hdr.is_some(),
        ])
        .to_string();
        Ok(SourcedVideo { source, identity })
    }

    /// TRACK-003 (ADR-0123): synthesize the exact source instant between the
    /// two neighboring presented frames by deterministic bidirectional flow
    /// warping. `frame` is the presented frame containing `time`; when `time`
    /// lands exactly on its pts or no successor exists, the decoded frame is
    /// already the exact answer and no flow is estimated.
    fn flow_image(
        &self,
        decoder: &mut VideoDecoder,
        frame: &DecodedVideoFrame,
        time: Time,
        interpolation: Option<kronello_time::FrameInterpolation>,
        policy: &VideoColorPolicy,
        working: ColorSpace,
    ) -> Result<VideoImage, MediaError> {
        let Some(kronello_time::FrameInterpolation::OpticalFlow(config)) = interpolation else {
            return Err(MediaError::InvalidInput(
                "unsupported frame interpolation mode".into(),
            ));
        };
        config.validate()?;
        let full_range = policy.range == "pc";
        let to_pixels = |f: &DecodedVideoFrame| -> Result<Vec<[f32; 4]>, MediaError> {
            let rgba = ffi::video_rgba(&self.native, f, full_range)?;
            Ok(sdr_working_pixels(&rgba, policy, working))
        };
        if time == frame.pts {
            return Ok(VideoImage {
                size: [frame.width, frame.height],
                pixels: to_pixels(frame)?,
            });
        }
        let next = match decoder.decode_at(frame.end) {
            Ok(next) => next,
            // The tail presented frame has no successor to blend with.
            Err(MediaError::FrameNotFound(_)) => {
                return Ok(VideoImage {
                    size: [frame.width, frame.height],
                    pixels: to_pixels(frame)?,
                });
            }
            Err(e) => return Err(e),
        };
        if next.width != frame.width || next.height != frame.height || next.pts != frame.end {
            return Err(MediaError::Decode(
                "flow neighbor presentation mismatch".into(),
            ));
        }
        let span = frame.end.checked_sub(frame.pts)?;
        let offset = time.checked_sub(frame.pts)?;
        let fraction_r = offset.checked_div(span)?;
        let fraction = fraction_r.numerator() as f64 / fraction_r.denominator() as f64;
        // Flow is estimated on fixed-point luma of the packed RGBA8 output;
        // colors are warped afterwards in working space.
        let rgba_lo = ffi::video_rgba(&self.native, frame, full_range)?;
        let rgba_hi = ffi::video_rgba(&self.native, &next, full_range)?;
        let (width, height) = (frame.width, frame.height);
        let luma_lo = luma_of_rgba8(&rgba_lo);
        let luma_hi = luma_of_rgba8(&rgba_hi);
        let mut fwd = kronello_tracking::estimate_flow(&luma_lo, &luma_hi, width, height, &config)?;
        let mut bwd = kronello_tracking::estimate_flow(&luma_hi, &luma_lo, width, height, &config)?;
        kronello_tracking::consistency_combine(&mut fwd, &bwd);
        kronello_tracking::consistency_combine(&mut bwd, &fwd);
        let lo = sdr_working_pixels(&rgba_lo, policy, working);
        let hi = sdr_working_pixels(&rgba_hi, policy, working);
        let pixels = match kronello_tracking::confidence_gate(&fwd, &bwd, &config) {
            Ok(_) => kronello_tracking::interpolate_frames(
                &lo, &hi, width, height, &fwd, &bwd, fraction,
            )?,
            // Only the authored fallback policy may downgrade to a crossfade;
            // otherwise the confidence failure stays a typed error.
            Err(e) => match config.flow_fallback {
                Some(kronello_time::FlowFallbackPolicy::Blend) => {
                    kronello_tracking::blend_frames(&lo, &hi, fraction)?
                }
                None => return Err(e.into()),
            },
        };
        Ok(VideoImage {
            size: [width, height],
            pixels,
        })
    }
}

/// SDR packed RGBA8 to premultiplied working-space pixels. The transfer
/// decode and optional Rec.709 -> Rec.2020 matrix match the scalar video path
/// exactly.
fn sdr_working_pixels(
    rgba: &[u8],
    policy: &VideoColorPolicy,
    working: ColorSpace,
) -> Vec<[f32; 4]> {
    let sampler = SdrSampler::new(policy, working);
    rgba.chunks_exact(4).map(|p| sampler.pixel(p)).collect()
}
/// Bit-exact SDR RGBA8 -> premultiplied working-space conversion with the
/// per-code-value transfer decode precomputed in a 256-entry LUT. `pixel`
/// evaluates the identical f64 arithmetic the scalar path used.
#[derive(Clone)]
struct SdrSampler {
    lut: std::sync::Arc<[f64; 256]>,
    to_rec2020: bool,
}
impl SdrSampler {
    fn new(policy: &VideoColorPolicy, working: ColorSpace) -> Self {
        let srgb = policy.transfer == "iec61966-2-1";
        let lut = std::array::from_fn(|i| {
            let v = i as f64 / 255.0;
            if srgb {
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            } else if v < 0.081 {
                v / 4.5
            } else {
                ((v + 0.099) / 1.099).powf(1.0 / 0.45)
            }
        });
        Self {
            lut: std::sync::Arc::new(lut),
            to_rec2020: working == ColorSpace::LinearRec2020,
        }
    }
    fn pixel(&self, p: &[u8]) -> [f32; 4] {
        let mut rgb = [
            self.lut[p[0] as usize],
            self.lut[p[1] as usize],
            self.lut[p[2] as usize],
        ];
        if self.to_rec2020 {
            rgb = [
                0.6274039 * rgb[0] + 0.3292830 * rgb[1] + 0.0433131 * rgb[2],
                0.0690973 * rgb[0] + 0.9195404 * rgb[1] + 0.0113623 * rgb[2],
                0.0163914 * rgb[0] + 0.0880133 * rgb[1] + 0.8955953 * rgb[2],
            ];
        }
        let alpha = f64::from(p[3]) / 255.0;
        [
            (rgb[0] * alpha) as f32,
            (rgb[1] * alpha) as f32,
            (rgb[2] * alpha) as f32,
            alpha as f32,
        ]
    }
}
/// A decoded SDR frame kept as packed RGBA8 plus its conversion LUT; output
/// pixels are converted only where the DAG's mapping samples them.
fn sdr_video_source(
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    policy: &VideoColorPolicy,
    working: ColorSpace,
) -> kronello_render::VideoSource {
    let sampler = SdrSampler::new(policy, working);
    let row = width as usize;
    kronello_render::VideoSource::new([width, height], move |x, y| {
        sampler.pixel(&rgba[(y * row + x) * 4..][..4])
    })
}

/// Fixed-point BT.601 luma of packed RGBA8 input; the deterministic flow
/// estimator consumes these integer samples.
fn luma_of_rgba8(rgba: &[u8]) -> Vec<u8> {
    rgba.chunks_exact(4)
        .map(|p| {
            ((77u32 * u32::from(p[0]) + 150u32 * u32::from(p[1]) + 25u32 * u32::from(p[2])) >> 8)
                as u8
        })
        .collect()
}

/// A decoded source image plus the serialized content identity that produced
/// it. The identity covers the decoder implementation, locked source, and
/// authored request — not the DAG node's sampling window, which the resolver
/// appends when forming a raster identity.
pub struct SourcedVideo {
    pub source: kronello_render::VideoSource,
    pub identity: String,
}
/// A media-resolved DAG plus the content-addressed identity of every
/// `RasterInput` it produced, keyed by DAG node index. Supplying the map to
/// `RasterCacheKey::for_dag_with_inputs` keeps per-node cache keys
/// proportional to the identity strings rather than the pixel payloads.
pub struct ResolvedDagMedia {
    pub dag: RenderDag,
    pub input_identities: std::collections::BTreeMap<usize, String>,
}
/// Stateful media resolution context shared across frames. It owns the lazily
/// loaded FFmpeg runtime and a bounded pool of open decoders keyed by resolved
/// path + stream + locked asset, so sequential redraws decode forward instead
/// of reopening and re-seeking per frame. The pool bound covers 8K 10-bit
/// sources: at most two decoders and 512 MiB of retained presentation buffers
/// survive calls (two current+lookahead pairs at ~100 MiB each). A session is
/// not `Sync`; hosts serialize redraws.
#[derive(Default)]
pub struct MediaSession {
    runtime: Option<Result<MediaRuntime, (&'static str, String)>>,
    decoders: Vec<(String, VideoDecoder)>,
    stats: DecoderPoolStats,
}
impl MediaSession {
    pub fn new() -> Self {
        Self::default()
    }
    /// Seed the session with an already-attempted runtime load; the error form
    /// keeps the typed `(code, message)` pair the render surface reports.
    pub fn with_runtime(runtime: Result<MediaRuntime, MediaError>) -> Self {
        Self {
            runtime: Some(runtime.map_err(|e| (e.code(), e.to_string()))),
            ..Self::default()
        }
    }
    /// Borrow an existing loaded runtime without loading one — used by
    /// backends whose callers own the load attempt.
    pub fn sharing_runtime(runtime: Result<&MediaRuntime, &MediaError>) -> Self {
        Self {
            runtime: Some(runtime.cloned().map_err(|e| (e.code(), e.to_string()))),
            ..Self::default()
        }
    }
    pub fn decoder_pool_stats(&self) -> DecoderPoolStats {
        self.stats
    }
    pub fn decoder_stats(&self) -> Vec<VideoDecodeStats> {
        self.decoders
            .iter()
            .map(|(_, d)| d.decode_stats())
            .collect()
    }
    fn runtime(&mut self) -> Result<&MediaRuntime, RenderError> {
        if self.runtime.is_none() {
            // A failed lazy load is not cached: a transient dlopen failure
            // must not disable video for the session's lifetime. Errors
            // seeded through `with_runtime`/`sharing_runtime` stay sticky
            // because callers injected them deliberately.
            match MediaRuntime::load() {
                Ok(runtime) => self.runtime = Some(Ok(runtime)),
                Err(e) => {
                    return Err(RenderError::Backend {
                        code: e.code(),
                        message: e.to_string(),
                    });
                }
            }
        }
        match self.runtime.as_ref() {
            Some(Ok(runtime)) => Ok(runtime),
            Some(Err((code, message))) => Err(RenderError::Backend {
                code,
                message: message.clone(),
            }),
            None => unreachable!("runtime slot filled above"),
        }
    }
    /// Pooled software decode to a lazy `VideoSource` plus the decoded image's
    /// content identity. Pool keys revalidate the resolved local path and the
    /// full locked asset on every access.
    #[allow(clippy::too_many_arguments)]
    fn video_source(
        &mut self,
        asset: &Asset,
        project_path: &Path,
        stream_index: u32,
        time: Time,
        working: ColorSpace,
        hdr: Option<kronello_render::HdrSettings>,
        reverse_sampling: bool,
        interpolation: Option<kronello_time::FrameInterpolation>,
    ) -> Result<SourcedVideo, RenderError> {
        // Revalidate the local content on every access, including pool hits;
        // unchanged stat fingerprints reuse the runtime's completed verification.
        let runtime = self.runtime()?.clone();
        let path = runtime
            .resolve_verified(asset, project_path)
            .map_err(render_error)?;
        let key = format!(
            "{}:{stream_index}:{}",
            path.display(),
            serde_json::to_string(asset).map_err(|e| RenderError::Backend {
                code: "INVALID_INPUT",
                message: e.to_string()
            })?
        );
        let mut decoder = self.take_decoder(&runtime, &path, stream_index, key.clone())?;
        let timing = std::env::var_os("KRONELLO_MEDIA_TIMING").is_some();
        let t0 = timing.then(std::time::Instant::now);
        let result = runtime.decode_video_source_from_decoder(
            &mut decoder,
            asset,
            project_path,
            stream_index,
            time,
            working,
            hdr,
            reverse_sampling,
            interpolation,
        );
        if let Some(t0) = t0 {
            eprintln!(
                "media_source: total={:.2}ms frame_bytes={}",
                t0.elapsed().as_secs_f64() * 1000.0,
                decoder.cached_frame_bytes()
            );
        }
        self.return_decoder(key, decoder);
        result.map_err(render_error)
    }
    /// Device-resident decode for the native preview: the pooled decoder
    /// presents the frame, it uploads to the device, and the shared media
    /// shader performs output-space sampling, transfer decode and
    /// premultiply. Nodes outside the upload contract (smart-reframe crop,
    /// reverse or interpolated sampling, HDR, nonlinear working space)
    /// return `Ok(None)` so the caller resolves them through the explicit
    /// software path; decode failures stay typed errors.
    #[allow(clippy::too_many_arguments)]
    fn video_resident(
        &mut self,
        gpu: &kronello_gpu::GpuContext,
        asset: &Asset,
        project_path: &Path,
        stream_index: u32,
        time: Time,
        working: ColorSpace,
        hdr: Option<kronello_render::HdrSettings>,
        reverse_sampling: bool,
        interpolation: Option<kronello_time::FrameInterpolation>,
        extent: [f64; 2],
        output_to_local: [[f64; 3]; 2],
        output: [u32; 2],
    ) -> Result<Option<(kronello_gpu::ResidentImage, String)>, RenderError> {
        if reverse_sampling
            || interpolation.is_some()
            || hdr.is_some()
            || !matches!(
                working,
                ColorSpace::LinearRec709 | ColorSpace::LinearRec2020
            )
        {
            return Ok(None);
        }
        let runtime = self.runtime()?.clone();
        let path = runtime
            .resolve_verified(asset, project_path)
            .map_err(render_error)?;
        let key = format!(
            "{}:{stream_index}:{}",
            path.display(),
            serde_json::to_string(asset).map_err(|e| RenderError::Backend {
                code: "INVALID_INPUT",
                message: e.to_string()
            })?
        );
        let mut decoder = self.take_decoder(&runtime, &path, stream_index, key.clone())?;
        let result = (|| {
            // `hdr.is_none()` here (checked above), so `prepare_video_frame`
            // already rejects HDR sources; `is_hdr` cannot be true.
            let (frame, policy, _) = runtime
                .prepare_video_frame(
                    &mut decoder,
                    asset,
                    project_path,
                    stream_index,
                    time,
                    working,
                    hdr,
                    reverse_sampling,
                )
                .map_err(render_error)?;
            let rgba;
            let upload = if frame.pixel_format == "yuv420p"
                && policy.range == "tv"
                && policy.matrix == "bt709"
            {
                let luma = frame.width as usize * frame.height as usize;
                let chroma_w = (frame.width as usize).div_ceil(2);
                let chroma = chroma_w * (frame.height as usize).div_ceil(2);
                if frame.pixels.len() != luma + 2 * chroma {
                    return Err(render_error(MediaError::Decode(
                        "unexpected yuv420p plane layout".into(),
                    )));
                }
                kronello_gpu::ResidentUpload::Yuv420pTv709 {
                    y: &frame.pixels[..luma],
                    u: &frame.pixels[luma..luma + chroma],
                    v: &frame.pixels[luma + chroma..],
                    size: [frame.width, frame.height],
                }
            } else {
                rgba = ffi::video_rgba(&runtime.native, &frame, policy.range == "pc")
                    .map_err(render_error)?;
                kronello_gpu::ResidentUpload::Rgba8 {
                    pixels: &rgba,
                    size: [frame.width, frame.height],
                }
            };
            let mut transfers = kronello_gpu::TransferStats::default();
            let image = gpu
                .sample_upload_to_working(
                    upload,
                    output,
                    extent,
                    output_to_local,
                    match working {
                        ColorSpace::LinearRec709 => kronello_gpu::WorkingSpace::LinearRec709,
                        ColorSpace::LinearRec2020 => kronello_gpu::WorkingSpace::LinearRec2020,
                        _ => unreachable!("gated above"),
                    },
                    policy.transfer == "iec61966-2-1",
                    &mut transfers,
                )
                .map_err(|error: kronello_gpu::GpuError| RenderError::Backend {
                    code: error.code(),
                    message: error.to_string(),
                })?;
            use sha2::Digest;
            let shader = sha2::Sha256::digest(kronello_gpu::RESIDENT_MEDIA_SHADER.as_bytes());
            let identity = serde_json::json!([
                "ffmpeg-upload-video-v1",
                runtime.capabilities.ffmpeg_version,
                decoder.native.name,
                asset.content_hash,
                stream_index,
                time,
                format!("{shader:x}"),
                frame.pts,
                frame.end,
                frame.width,
                frame.height,
                frame.pixel_format,
                frame.color_primaries,
                frame.color_transfer,
                frame.color_matrix,
                frame.color_range,
            ])
            .to_string();
            Ok(Some((image, identity)))
        })();
        self.return_decoder(key, decoder);
        result
    }
    fn take_decoder(
        &mut self,
        runtime: &MediaRuntime,
        path: &Path,
        stream_index: u32,
        key: String,
    ) -> Result<VideoDecoder, RenderError> {
        if let Some(index) = self.decoders.iter().position(|(k, _)| *k == key) {
            self.stats.hits += 1;
            Ok(self.decoders.remove(index).1)
        } else {
            self.stats.misses += 1;
            if self.decoders.len() >= 2 {
                self.decoders.remove(0);
                self.stats.evictions += 1;
            }
            runtime
                .open_video_stream(path, stream_index)
                .map_err(render_error)
        }
    }
    fn return_decoder(&mut self, key: String, decoder: VideoDecoder) {
        self.decoders.push((key, decoder));
        while self
            .decoders
            .iter()
            .map(|(_, d)| d.cached_frame_bytes())
            .sum::<usize>()
            > 512 * 1024 * 1024
        {
            self.decoders.remove(0);
            self.stats.evictions += 1;
        }
        self.stats.active_decoders = self.decoders.len();
        self.stats.retained_frame_bytes = self
            .decoders
            .iter()
            .map(|(_, d)| d.cached_frame_bytes())
            .sum();
        self.stats.peak_retained_frame_bytes = self
            .stats
            .peak_retained_frame_bytes
            .max(self.stats.retained_frame_bytes);
    }
}
/// Raster-identity helper: the node-level sampling parameters join the decoded
/// source identity so two nodes sharing a decoded frame still key distinctly.
fn raster_identity(
    source_identity: &str,
    extent: &[f64; 2],
    crop: &Option<[f64; 4]>,
    output_to_local: &[[f64; 3]; 2],
) -> Result<String, RenderError> {
    serde_json::to_string(&(source_identity, extent, crop, output_to_local)).map_err(|e| {
        RenderError::Backend {
            code: "INVALID_INPUT",
            message: e.to_string(),
        }
    })
}
/// Resolve a DAG's external media through the explicit software decode
/// contract using `session`'s persistent runtime and decoder pool. Shared by
/// `VideoRenderBackend`, the sequential backend and the native preview
/// adapter so no backend ever lowers an unresolved `VideoDraw`; decode
/// failures stay typed errors and images decode without loading the video
/// runtime.
pub fn resolve_dag_media_in(
    dag: &RenderDag,
    project_path: &Path,
    session: &mut MediaSession,
) -> Result<ResolvedDagMedia, RenderError> {
    let mut input_identities = std::collections::BTreeMap::new();
    let resolved = dag.resolve_video(
        |index, asset, stream, time, working, reverse_sampling, interpolation| {
            let (extent, crop, output_to_local) = match dag.nodes().get(index) {
                Some(kronello_render::DagNode::VideoDraw {
                    extent,
                    crop,
                    output_to_local,
                    ..
                }) => (*extent, *crop, *output_to_local),
                _ => {
                    return Err(RenderError::InvalidInput(
                        "video resolve index outside VideoDraw".into(),
                    ));
                }
            };
            if asset.kind == kronello_model::AssetKind::Image {
                if interpolation.is_some() {
                    return Err(RenderError::UnsupportedFeature(
                        "frame interpolation requires a video asset".into(),
                    ));
                }
                let image = crate::decode_image_asset(asset, project_path, stream, working)
                    .map_err(render_error)?;
                let identity = serde_json::to_string(&(
                    "image-decode-v1",
                    asset.content_hash.as_str(),
                    stream,
                    image.size,
                ))
                .map_err(|e| RenderError::Backend {
                    code: "INVALID_INPUT",
                    message: e.to_string(),
                })?;
                input_identities.insert(
                    index,
                    raster_identity(&identity, &extent, &crop, &output_to_local)?,
                );
                return Ok(kronello_render::VideoSource::from_image(image));
            }
            if asset
                .streams
                .iter()
                .find(|s| s.index == stream)
                .is_some_and(|s| crate::raw::is_camera_raw_video_codec(&s.codec))
            {
                // Camera RAW paths never need the FFmpeg runtime.
                let image = crate::raw::decode_video_dispatch(
                    asset,
                    project_path,
                    stream,
                    time,
                    working,
                    reverse_sampling,
                    interpolation,
                )
                .map_err(render_error)?;
                let identity = serde_json::to_string(&(
                    "camera-raw-video-v1",
                    asset.content_hash.as_str(),
                    stream,
                    time,
                    reverse_sampling,
                    interpolation,
                    image.size,
                ))
                .map_err(|e| RenderError::Backend {
                    code: "INVALID_INPUT",
                    message: e.to_string(),
                })?;
                input_identities.insert(
                    index,
                    raster_identity(&identity, &extent, &crop, &output_to_local)?,
                );
                return Ok(kronello_render::VideoSource::from_image(image));
            }
            let sourced = session.video_source(
                asset,
                project_path,
                stream,
                time,
                working,
                dag.hdr(),
                reverse_sampling,
                interpolation,
            )?;
            input_identities.insert(
                index,
                raster_identity(&sourced.identity, &extent, &crop, &output_to_local)?,
            );
            Ok(sourced.source)
        },
    )?;
    Ok(ResolvedDagMedia {
        dag: resolved,
        input_identities,
    })
}
/// A media-resolved DAG plus device-resident video inputs for the native
/// preview: `resident` keys `VideoDraw` node indexes and `input_identities`
/// carries the content address of every resolved or resident source.
pub struct ResolvedResidentMedia {
    pub dag: RenderDag,
    pub resident: std::collections::BTreeMap<usize, kronello_gpu::ResidentImage>,
    pub input_identities: std::collections::BTreeMap<usize, String>,
}
/// Preview-only mixed resolution: eligible `VideoDraw` nodes decode through
/// the session's pooled software decoder and upload to the device for
/// shader-side sampling; smart-reframe crops, reverse/interpolated sampling,
/// HDR and nonlinear working spaces keep the explicit software `RasterInput`
/// path per node, and images/camera RAW always take it. The composed result
/// still reports every source's content identity.
pub fn resolve_dag_media_resident(
    dag: &RenderDag,
    gpu: &kronello_gpu::GpuContext,
    project_path: &Path,
    session: &mut MediaSession,
) -> Result<ResolvedResidentMedia, RenderError> {
    let output = dag.execution_region().pixels;
    let mut resident = std::collections::BTreeMap::new();
    let mut input_identities = std::collections::BTreeMap::new();
    let resolved = dag.resolve_video_selective(
        |index, asset, stream, time, working, reverse_sampling, interpolation| {
            let (extent, crop, output_to_local) = match dag.nodes().get(index) {
                Some(kronello_render::DagNode::VideoDraw {
                    extent,
                    crop,
                    output_to_local,
                    ..
                }) => (*extent, *crop, *output_to_local),
                _ => {
                    return Err(RenderError::InvalidInput(
                        "video resolve index outside VideoDraw".into(),
                    ));
                }
            };
            if asset.kind == kronello_model::AssetKind::Image {
                if interpolation.is_some() {
                    return Err(RenderError::UnsupportedFeature(
                        "frame interpolation requires a video asset".into(),
                    ));
                }
                let image = crate::decode_image_asset(asset, project_path, stream, working)
                    .map_err(render_error)?;
                let identity = serde_json::to_string(&(
                    "image-decode-v1",
                    asset.content_hash.as_str(),
                    stream,
                    image.size,
                ))
                .map_err(|e| RenderError::Backend {
                    code: "INVALID_INPUT",
                    message: e.to_string(),
                })?;
                input_identities.insert(
                    index,
                    raster_identity(&identity, &extent, &crop, &output_to_local)?,
                );
                return Ok(kronello_render::VideoResolution::Source(
                    kronello_render::VideoSource::from_image(image),
                ));
            }
            if asset
                .streams
                .iter()
                .find(|s| s.index == stream)
                .is_some_and(|s| crate::raw::is_camera_raw_video_codec(&s.codec))
            {
                let image = crate::raw::decode_video_dispatch(
                    asset,
                    project_path,
                    stream,
                    time,
                    working,
                    reverse_sampling,
                    interpolation,
                )
                .map_err(render_error)?;
                let identity = serde_json::to_string(&(
                    "camera-raw-video-v1",
                    asset.content_hash.as_str(),
                    stream,
                    time,
                    reverse_sampling,
                    interpolation,
                    image.size,
                ))
                .map_err(|e| RenderError::Backend {
                    code: "INVALID_INPUT",
                    message: e.to_string(),
                })?;
                input_identities.insert(
                    index,
                    raster_identity(&identity, &extent, &crop, &output_to_local)?,
                );
                return Ok(kronello_render::VideoResolution::Source(
                    kronello_render::VideoSource::from_image(image),
                ));
            }
            if crop.is_none()
                && let Some((image, identity)) = session.video_resident(
                    gpu,
                    asset,
                    project_path,
                    stream,
                    time,
                    working,
                    dag.hdr(),
                    reverse_sampling,
                    interpolation,
                    extent,
                    output_to_local,
                    output,
                )?
            {
                resident.insert(index, image);
                input_identities.insert(
                    index,
                    raster_identity(&identity, &extent, &crop, &output_to_local)?,
                );
                return Ok(kronello_render::VideoResolution::Deferred);
            }
            let sourced = session.video_source(
                asset,
                project_path,
                stream,
                time,
                working,
                dag.hdr(),
                reverse_sampling,
                interpolation,
            )?;
            input_identities.insert(
                index,
                raster_identity(&sourced.identity, &extent, &crop, &output_to_local)?,
            );
            Ok(kronello_render::VideoResolution::Source(sourced.source))
        },
    )?;
    Ok(ResolvedResidentMedia {
        dag: resolved,
        resident,
        input_identities,
    })
}
/// Resolve a DAG's external media through the explicit software decode
/// contract. Shared by `VideoRenderBackend` and the native preview adapter so
/// no backend ever lowers an unresolved `VideoDraw`; decode failures stay
/// typed errors and images decode without loading the video runtime.
pub fn resolve_dag_media(dag: &RenderDag, project_path: &Path) -> Result<RenderDag, RenderError> {
    Ok(resolve_dag_media_in(dag, project_path, &mut MediaSession::new())?.dag)
}
/// The source project path supplies only a stable locator base. Project state
/// and asset locks come exclusively from the RenderDag's fixed snapshot.
pub struct VideoRenderBackend<'a> {
    pub backend: &'a dyn RenderBackend,
    pub project_path: &'a Path,
}
impl RenderBackend for VideoRenderBackend<'_> {
    fn begin_observation_scope(
        &self,
    ) -> Result<Option<Box<dyn kronello_render::RenderObservationScope + '_>>, RenderError> {
        self.backend.begin_observation_scope()
    }
    fn cache_namespace(&self) -> Option<String> {
        self.backend
            .cache_namespace()
            .map(|n| format!("software-video-sdr-hdr203-v1:{n}"))
    }

    fn display_from_linear(
        &self,
        linear: &[[f32; 4]],
        working: ColorSpace,
    ) -> Result<Vec<[f32; 4]>, RenderError> {
        self.backend.display_from_linear(linear, working)
    }
    fn name(&self) -> &str {
        self.backend.name()
    }
    fn input_path(&self) -> &str {
        if self.backend.name() == "wgpu_rgba16f" {
            "software_video_decode_native_8_16bit_cpu_pinned_sdr_hdr203_color_cpu_nearest_sample_explicit_gpu_pixel_upload"
        } else {
            "software_video_decode_native_8_16bit_cpu_pinned_sdr_hdr203_color_cpu_nearest_sample_to_selected_backend"
        }
    }
    fn image_input_path(&self) -> &str {
        if self.backend.name() == "wgpu_rgba16f" {
            "png_native_8_16bit_cpu_color_premultiply_nearest_explicit_gpu_upload"
        } else {
            "png_native_8_16bit_cpu_color_premultiply_nearest_selected_backend"
        }
    }
    fn transfer_stats(&self) -> Option<kronello_render::RenderTransferStats> {
        self.backend.transfer_stats()
    }
    fn transfer_stats_total(&self) -> Option<kronello_render::RenderTransferStats> {
        self.backend.transfer_stats_total()
    }
    fn resource_cache_stats(&self) -> Option<kronello_render::RenderResourceCacheStats> {
        self.backend.resource_cache_stats()
    }
    fn execute(&self, dag: &RenderDag) -> Result<BackendFrame, RenderError> {
        self.execute_with_cache(
            dag,
            &mut RenderCache::new(kronello_render::CacheConfig::disabled()),
        )
    }
    fn execute_with_cache(
        &self,
        dag: &RenderDag,
        cache: &mut RenderCache,
    ) -> Result<BackendFrame, RenderError> {
        let _scope = self.begin_observation_scope()?;
        let resolved = resolve_dag_media_in(dag, self.project_path, &mut MediaSession::new())?;
        self.backend
            .execute_with_inputs(&resolved.dag, cache, &resolved.input_identities)
    }
}
/// Native handles live only for one service render scope, never in pure caches.
/// At most two decoders and 512 MiB of retained presentation buffers survive calls.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct DecoderPoolStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub active_decoders: usize,
    pub retained_frame_bytes: usize,
    pub peak_retained_frame_bytes: usize,
}
pub struct SequentialVideoRenderBackend<'a> {
    base: VideoRenderBackend<'a>,
    session: std::cell::RefCell<MediaSession>,
}
impl<'a> SequentialVideoRenderBackend<'a> {
    pub fn new(
        backend: &'a dyn RenderBackend,
        project_path: &'a Path,
        runtime: Result<&'a MediaRuntime, &'a MediaError>,
    ) -> Self {
        Self {
            base: VideoRenderBackend {
                backend,
                project_path,
            },
            session: std::cell::RefCell::new(MediaSession::sharing_runtime(runtime)),
        }
    }
    pub fn pool_stats(&self) -> DecoderPoolStats {
        self.session.borrow().decoder_pool_stats()
    }
    pub fn decoder_stats(&self) -> Vec<VideoDecodeStats> {
        self.session.borrow().decoder_stats()
    }
}
impl RenderBackend for SequentialVideoRenderBackend<'_> {
    fn begin_observation_scope(
        &self,
    ) -> Result<Option<Box<dyn kronello_render::RenderObservationScope + '_>>, RenderError> {
        self.base.begin_observation_scope()
    }
    fn name(&self) -> &str {
        self.base.name()
    }
    fn cache_namespace(&self) -> Option<String> {
        self.base.cache_namespace()
    }
    fn input_path(&self) -> &str {
        self.base.input_path()
    }
    fn image_input_path(&self) -> &str {
        self.base.image_input_path()
    }
    fn transfer_stats(&self) -> Option<kronello_render::RenderTransferStats> {
        self.base.transfer_stats()
    }
    fn transfer_stats_total(&self) -> Option<kronello_render::RenderTransferStats> {
        self.base.transfer_stats_total()
    }
    fn resource_cache_stats(&self) -> Option<kronello_render::RenderResourceCacheStats> {
        self.base.resource_cache_stats()
    }
    fn display_from_linear(
        &self,
        linear: &[[f32; 4]],
        working: ColorSpace,
    ) -> Result<Vec<[f32; 4]>, RenderError> {
        self.base.display_from_linear(linear, working)
    }
    fn execute(&self, dag: &RenderDag) -> Result<BackendFrame, RenderError> {
        self.execute_with_cache(
            dag,
            &mut RenderCache::new(kronello_render::CacheConfig::disabled()),
        )
    }
    fn execute_with_cache(
        &self,
        dag: &RenderDag,
        cache: &mut RenderCache,
    ) -> Result<BackendFrame, RenderError> {
        let _scope = self.begin_observation_scope()?;
        let resolved =
            resolve_dag_media_in(dag, self.base.project_path, &mut self.session.borrow_mut())?;
        self.base
            .backend
            .execute_with_inputs(&resolved.dag, cache, &resolved.input_identities)
    }
}
fn render_error(error: MediaError) -> RenderError {
    RenderError::Backend {
        code: error.code(),
        message: error.to_string(),
    }
}

/// Validate canonical locked media bounds before native demux edit-list timing.
pub fn validate_resident_source_time(
    stream: &StreamMetadata,
    time: Time,
) -> Result<(), MediaError> {
    let start = stream.start_time.unwrap_or(Time::ZERO);
    let duration = stream
        .duration
        .ok_or_else(|| MediaError::UnsupportedFeature("resident stream duration missing".into()))?;
    let end = start.checked_add(duration)?;
    if time < start || time >= end {
        return Err(MediaError::FrameNotFound(format!(
            "{time:?} outside locked [{start:?},{end:?})"
        )));
    }
    Ok(())
}

/// Explicit selection of the guaranteed hardware decode path. Software fallback
/// is available only by separately selecting VideoRenderBackend.
pub struct ResidentVideoRenderBackend<'a> {
    pub gpu: &'a kronello_gpu::GpuContext,
    pub project_path: &'a Path,
    pub format: ResidentVideoFormat,
    transfers: std::cell::RefCell<kronello_gpu::TransferStats>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResidentVideoFormat {
    Bgra8,
    Nv12VideoRange,
}
impl<'a> ResidentVideoRenderBackend<'a> {
    pub fn new(
        gpu: &'a kronello_gpu::GpuContext,
        project_path: &'a Path,
        format: ResidentVideoFormat,
    ) -> Self {
        Self {
            gpu,
            project_path,
            format,
            transfers: Default::default(),
        }
    }
}
impl RenderBackend for ResidentVideoRenderBackend<'_> {
    fn begin_observation_scope(
        &self,
    ) -> Result<Option<Box<dyn kronello_render::RenderObservationScope + '_>>, RenderError> {
        self.gpu.begin_observation_scope()
    }
    fn requires_gpu_resident(&self) -> bool {
        true
    }
    fn transfer_stats(&self) -> Option<kronello_render::RenderTransferStats> {
        Some(self.transfers.borrow().render_stats())
    }

    fn transfer_stats_total(&self) -> Option<kronello_render::RenderTransferStats> {
        Some(self.transfers.borrow().render_stats())
    }
    fn cache_namespace(&self) -> Option<String> {
        self.gpu
            .cache_namespace()
            .map(|n| format!("hardware-video-{:?}-v1:{n}", self.format))
    }

    fn name(&self) -> &str {
        "wgpu_rgba16f"
    }
    fn input_path(&self) -> &str {
        match self.format {
            ResidentVideoFormat::Bgra8 => {
                "hardware_videotoolbox_bgra8_iosurface_metal_gpu_color_sample_final_output_readback"
            }
            ResidentVideoFormat::Nv12VideoRange => {
                "hardware_videotoolbox_nv12_iosurface_metal_gpu_color_sample_final_output_readback"
            }
        }
    }
    fn display_from_linear(
        &self,
        linear: &[[f32; 4]],
        working: ColorSpace,
    ) -> Result<Vec<[f32; 4]>, RenderError> {
        self.gpu.display_from_linear(linear, working)
    }
    fn resource_cache_stats(&self) -> Option<kronello_render::RenderResourceCacheStats> {
        let mut stats = self.gpu.render_cache_stats();
        stats.persistent_disk_policy =
            kronello_render::PersistentRasterCachePolicy::DisabledForGpuResident;
        Some(stats)
    }
    fn execute(&self, dag: &RenderDag) -> Result<BackendFrame, RenderError> {
        let _scope = self.begin_observation_scope()?;
        #[cfg(not(target_os = "macos"))]
        {
            let _ = dag;
            Err(RenderError::UnsupportedFeature("require_gpu_resident Metal/VideoToolbox requires macOS; select explicit software backend".into()))
        }
        #[cfg(target_os = "macos")]
        {
            use kronello_framebridge::resident::{
                ResidentFormat, VideoTransfer, decode_file_with_interval,
            };
            use kronello_gpu::{TransferStats, WorkingSpace};
            let native_error =
                |e: kronello_framebridge::videotoolbox::NativeError| RenderError::Backend {
                    code: "UNSUPPORTED_FEATURE",
                    message: e.to_string(),
                };
            let mut resident = std::collections::BTreeMap::new();
            let mut source_keys = std::collections::BTreeMap::new();
            let mut stats = TransferStats::default();
            for (index, node) in dag.nodes().iter().enumerate() {
                if let kronello_render::DagNode::VideoDraw {
                    asset,
                    stream_index,
                    time,
                    reverse_sampling,
                    interpolation,
                    extent,
                    crop,
                    output_to_local,
                    ..
                } = node
                {
                    if crop.is_some() {
                        // AI-003 (ADR-0126): resident sampling has no crop
                        // window channel yet; the software path is the
                        // authoritative implementation.
                        return Err(RenderError::UnsupportedFeature(
                            "smart reframe crop requires explicit software backend".into(),
                        ));
                    }
                    if *reverse_sampling {
                        return Err(RenderError::UnsupportedFeature("reverse_grid_v1 requires explicit software presentation-interval decode".into()));
                    }
                    if interpolation.is_some() {
                        return Err(RenderError::UnsupportedFeature(
                            "resident decode requires explicit software optical-flow interpolation"
                                .into(),
                        ));
                    }
                    let metadata = asset
                        .streams
                        .iter()
                        .find(|s| s.index == *stream_index)
                        .ok_or_else(|| {
                            RenderError::UnsupportedFeature("missing video stream metadata".into())
                        })?;
                    if !matches!(metadata.codec.as_str(), "h264" | "hevc")
                        || metadata.pixel_format.as_deref() != Some("yuv420p")
                    {
                        return Err(RenderError::UnsupportedFeature("require_gpu_resident accepts H.264/HEVC hardware decode only; select explicit software backend".into()));
                    }
                    validate_resident_source_time(metadata, *time).map_err(render_error)?;
                    let policy = video_color_policy(metadata).map_err(render_error)?;
                    if policy.range != "tv" || policy.matrix != "bt709" {
                        return Err(RenderError::UnsupportedFeature(
                            "resident video requires tagged/assumed SDR BT.709 limited-range YCbCr"
                                .into(),
                        ));
                    }
                    let path = resolve_asset(asset, self.project_path).map_err(render_error)?;
                    let start = metadata.start_time.unwrap_or(Time::ZERO);
                    let end = start
                        .checked_add(metadata.duration.expect("validated resident duration"))
                        .map_err(|e| RenderError::InvalidInput(e.to_string()))?;
                    let frame = decode_file_with_interval(
                        &path,
                        *stream_index,
                        (time.numerator(), time.denominator()),
                        match self.format {
                            ResidentVideoFormat::Bgra8 => ResidentFormat::Bgra8,
                            ResidentVideoFormat::Nv12VideoRange => ResidentFormat::Nv12VideoRange,
                        },
                        [
                            (start.numerator(), start.denominator()),
                            (end.numerator(), end.denominator()),
                        ],
                    )
                    .map_err(native_error)?;
                    if metadata.width != Some(frame.size()[0])
                        || metadata.height != Some(frame.size()[1])
                    {
                        return Err(RenderError::InvalidInput(
                            "resident frame metadata dimensions differ".into(),
                        ));
                    }
                    let image = frame
                        .sample_to_working(
                            self.gpu,
                            dag.execution_region().pixels,
                            *extent,
                            *output_to_local,
                            match dag.working_space() {
                                ColorSpace::LinearRec709 => WorkingSpace::LinearRec709,
                                ColorSpace::LinearRec2020 => WorkingSpace::LinearRec2020,
                                _ => {
                                    return Err(RenderError::UnsupportedFeature(
                                        "resident nonlinear working space".into(),
                                    ));
                                }
                            },
                            if policy.transfer == "iec61966-2-1" {
                                VideoTransfer::Srgb
                            } else {
                                VideoTransfer::Bt709
                            },
                            &mut stats,
                        )
                        .map_err(native_error)?;
                    resolve_asset(asset, self.project_path).map_err(render_error)?;
                    use sha2::Digest;
                    let conversion = sha2::Sha256::digest(include_bytes!(
                        "../../kronello-gpu/src/resident_media.wgsl"
                    ));
                    let identity = format!(
                        "native-video-source-v1:{}:{metadata:?}:{:?}:{conversion:x}",
                        asset.content_hash, self.format
                    );
                    source_keys.insert(
                        index,
                        kronello_render::RasterCacheKey::external_source(
                            &identity,
                            dag.execution_region(),
                            dag.working_space(),
                            &self.cache_namespace().unwrap(),
                        )?,
                    );
                    resident.insert(index, image);
                }
            }
            let (frame, final_stats) =
                self.gpu
                    .execute_resident_video_cached_with_stats(dag, &resident, &source_keys)?;
            stats.accumulate(&final_stats);
            self.transfers.borrow_mut().accumulate(&stats);
            Ok(frame)
        }
    }
}
