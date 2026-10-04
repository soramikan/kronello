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
        Ok(VideoDecoder { native, report })
    }
    pub fn decode_video_image(
        &self,
        asset: &Asset,
        project_path: &Path,
        stream_index: u32,
        time: Time,
        working: ColorSpace,
    ) -> Result<VideoImage, MediaError> {
        let path = resolve_asset(asset, project_path)?;
        let mut decoder = self.open_video_stream(&path, stream_index)?;
        let frame = decoder.decode_at(time)?;
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
        let policy = video_color_policy(&metadata)?;
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
    fn name(&self) -> &str {
        self.backend.name()
    }
    fn input_path(&self) -> &str {
        if self.backend.name() == "wgpu_rgba16f" {
            "software_video_decode_cpu_sdr_color_cpu_nearest_sample_explicit_gpu_pixel_upload"
        } else {
            "software_video_decode_cpu_sdr_color_cpu_nearest_sample_to_selected_backend"
        }
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
        let runtime = if dag
            .nodes()
            .iter()
            .any(|n| matches!(n, kronello_render::DagNode::VideoDraw { .. }))
        {
            Some(MediaRuntime::load().map_err(render_error)?)
        } else {
            None
        };
        let resolved = dag.resolve_video(|asset, stream, time, working| {
            runtime
                .as_ref()
                .expect("video runtime")
                .decode_video_image(asset, self.project_path, stream, time, working)
                .map_err(render_error)
        })?;
        self.backend.execute_with_cache(&resolved, cache)
    }
}
fn render_error(error: MediaError) -> RenderError {
    RenderError::Backend {
        code: error.code(),
        message: error.to_string(),
    }
}
