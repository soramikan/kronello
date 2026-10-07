//! Explicit software decode, SDR color conversion and CPU sampling boundary.
use crate::*;
use kronello_model::{Asset, ColorSpace, StreamMetadata};
use kronello_render::{
    BackendFrame, RenderBackend, RenderCache, RenderDag, RenderError, VideoImage,
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
    ) -> Result<VideoDecoder<'_>, MediaError> {
        let path = path.canonicalize()?;
        if !path.is_file() {
            return Err(MediaError::InvalidInput(
                "expected a local regular file".into(),
            ));
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
    ) -> Result<VideoImage, MediaError> {
        let path = resolve_asset(asset, project_path)?;
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
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn decode_video_image_from_decoder(
        &self,
        decoder: &mut VideoDecoder<'_>,
        asset: &Asset,
        project_path: &Path,
        stream_index: u32,
        time: Time,
        working: ColorSpace,
        hdr: Option<kronello_render::HdrSettings>,
        reverse_sampling: bool,
    ) -> Result<VideoImage, MediaError> {
        resolve_asset(asset, project_path)?;
        let frame = if reverse_sampling {
            decoder.decode_at_reverse(time)?
        } else {
            decoder.decode_at(time)?
        };
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
        if hdr.is_some() {
            let rgba = ffi::video_rgba64(&self.native, &frame, policy.range == "pc", is_hdr)?;
            resolve_asset(asset, project_path)?;
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
            return Ok(VideoImage {
                size: [frame.width, frame.height],
                pixels,
            });
        }
        let rgba = ffi::video_rgba(&self.native, &frame, policy.range == "pc")?;
        resolve_asset(asset, project_path)?;
        let pixels = rgba
            .chunks_exact(4)
            .map(|p| {
                let mut rgb = [0, 1, 2].map(|i| {
                    let v = f64::from(p[i]) / 255.0;
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
                });
                if working == ColorSpace::LinearRec2020 {
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
            })
            .collect();
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
        let runtime = if dag
            .nodes()
            .iter()
            .any(|n| matches!(n, kronello_render::DagNode::VideoDraw { asset, .. } if asset.kind == kronello_model::AssetKind::Video))
        {
            Some(MediaRuntime::load().map_err(render_error)?)
        } else {
            None
        };
        let resolved = dag.resolve_video(|asset, stream, time, working, reverse_sampling| {
            if asset.kind == kronello_model::AssetKind::Image {
                return crate::decode_image_asset(asset, self.project_path, stream, working)
                    .map_err(render_error);
            }
            runtime
                .as_ref()
                .expect("video runtime")
                .decode_video_image_with_sampling(
                    asset,
                    self.project_path,
                    stream,
                    time,
                    working,
                    dag.hdr(),
                    reverse_sampling,
                )
                .map_err(render_error)
        })?;
        self.backend.execute_with_cache(&resolved, cache)
    }
}
/// Native handles live only for one service render scope, never in pure caches.
/// At most two decoders and 128 MiB of retained presentation buffers survive calls.
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
    runtime: Result<&'a MediaRuntime, &'a MediaError>,
    pool: std::cell::RefCell<Vec<(String, VideoDecoder<'a>)>>,
    pool_stats: std::cell::RefCell<DecoderPoolStats>,
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
            runtime,
            pool: std::cell::RefCell::new(Vec::new()),
            pool_stats: std::cell::RefCell::new(DecoderPoolStats::default()),
        }
    }
    pub fn pool_stats(&self) -> DecoderPoolStats {
        *self.pool_stats.borrow()
    }
    pub fn decoder_stats(&self) -> Vec<VideoDecodeStats> {
        self.pool
            .borrow()
            .iter()
            .map(|(_, d)| d.decode_stats())
            .collect()
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
        let resolved = dag.resolve_video(|asset, stream, time, working, reverse_sampling| {
            if asset.kind == kronello_model::AssetKind::Image {
                return crate::decode_image_asset(asset, self.base.project_path, stream, working)
                    .map_err(render_error);
            }
            let runtime = self.runtime.map_err(|error| RenderError::Backend {
                code: error.code(),
                message: error.to_string(),
            })?;
            // Revalidate the local content on every access, including interval hits.
            let path = resolve_asset(asset, self.base.project_path).map_err(render_error)?;
            let key = format!(
                "{}:{stream}:{}",
                path.display(),
                serde_json::to_string(asset).map_err(|e| RenderError::Backend {
                    code: "INVALID_INPUT",
                    message: e.to_string()
                })?
            );
            let mut pool = self.pool.borrow_mut();
            let mut stats = self.pool_stats.borrow_mut();
            let mut entry = if let Some(index) = pool.iter().position(|(k, _)| *k == key) {
                stats.hits += 1;
                pool.remove(index)
            } else {
                stats.misses += 1;
                if pool.len() >= 2 {
                    pool.remove(0);
                    stats.evictions += 1;
                }
                (
                    key,
                    runtime
                        .open_video_stream(&path, stream)
                        .map_err(render_error)?,
                )
            };
            let result = runtime.decode_video_image_from_decoder(
                &mut entry.1,
                asset,
                self.base.project_path,
                stream,
                time,
                working,
                dag.hdr(),
                reverse_sampling,
            );
            pool.push(entry);
            while pool
                .iter()
                .map(|(_, d)| d.cached_frame_bytes())
                .sum::<usize>()
                > 128 * 1024 * 1024
            {
                pool.remove(0);
                stats.evictions += 1;
            }
            stats.active_decoders = pool.len();
            stats.retained_frame_bytes = pool.iter().map(|(_, d)| d.cached_frame_bytes()).sum();
            stats.peak_retained_frame_bytes = stats
                .peak_retained_frame_bytes
                .max(stats.retained_frame_bytes);
            result.map_err(render_error)
        })?;
        self.base.backend.execute_with_cache(&resolved, cache)
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
                    extent,
                    output_to_local,
                    ..
                } = node
                {
                    if *reverse_sampling {
                        return Err(RenderError::UnsupportedFeature("reverse_grid_v1 requires explicit software presentation-interval decode".into()));
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
                        "../../kronello-framebridge/src/resident.wgsl"
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
