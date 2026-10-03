use std::fs;
use std::path::Path;

use half::f16;
use kronello_model::{ColorSpace, CompositionId, FontRef};
use kronello_text::FontData;
use kronello_time::{FrameRate, Time, TimeRange};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    BackendFrame, OutputRegion, RenderBackend, RenderError, RenderSnapshot, SemanticVersions,
    build_render_dag, build_scene_ir,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameRequest {
    pub time: Time,
    pub region: OutputRegion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageFormat {
    pub color_space: ColorSpace,
    pub transfer_function: String,
    pub alpha: String,
    pub association_space: ColorSpace,
    pub pixel_format: String,
    pub channel_order: String,
    pub row_order: String,
    pub byte_order: String,
    pub clipping: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameMetadata {
    pub schema_version: u32,
    pub snapshot_schema_version: u32,
    pub project_schema_version: u32,
    pub snapshot_content_hash: String,
    pub revision: String,
    pub composition: CompositionId,
    pub semantic_versions: SemanticVersions,
    pub font_locks: Vec<FontRef>,
    pub time: Time,
    /// Decimal absolute time-zero frame index; absent for arbitrary still time.
    pub frame_index: Option<String>,
    pub sequence_number: Option<u64>,
    pub design_extent: [f64; 2],
    pub region: OutputRegion,
    pub design_to_pixel: [[f64; 3]; 2],
    pub working_space: ColorSpace,
    pub flatten_tolerance_px: f64,
    pub numeric: ImageFormat,
    pub display: ImageFormat,
    pub backend: String,
}
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedFrame {
    pub pixels: BackendFrame,
    pub metadata: FrameMetadata,
}

pub fn render_frame(
    snapshot: &RenderSnapshot,
    fonts: &[FontData<'_>],
    backend: &dyn RenderBackend,
    request: FrameRequest,
) -> Result<RenderedFrame, RenderError> {
    request.region.validate()?;
    let scene = build_scene_ir(snapshot, request.time, fonts)?;
    let dag = build_render_dag(&scene, snapshot.profile(), request.region)?;
    let pixels = backend.execute(&dag)?;
    let count = u64::from(request.region.pixels[0]) * u64::from(request.region.pixels[1]);
    if pixels.linear.len() as u64 != count || pixels.display.len() as u64 != count {
        return Err(RenderError::InvalidInput(
            "backend returned wrong pixel count".into(),
        ));
    }
    validate_pixels(&pixels.linear, true)?;
    validate_pixels(&pixels.display, false)?;
    let working = snapshot.profile().working_space;
    let metadata = FrameMetadata {
        schema_version: 1,
        snapshot_schema_version: crate::SNAPSHOT_SCHEMA_VERSION,
        project_schema_version: snapshot.project().schema_version,
        snapshot_content_hash: snapshot.content_hash()?,
        revision: snapshot.revision().to_string(),
        composition: snapshot.composition(),
        semantic_versions: snapshot.semantic_versions().clone(),
        font_locks: snapshot.font_locks().to_vec(),
        time: request.time,
        frame_index: None,
        sequence_number: None,
        design_extent: scene.design_extent,
        region: request.region,
        design_to_pixel: request.region.design_to_pixel().0,
        working_space: working,
        flatten_tolerance_px: snapshot.profile().flatten_tolerance_px,
        numeric: ImageFormat {
            color_space: working,
            transfer_function: "linear".into(),
            alpha: "premultiplied".into(),
            association_space: working,
            pixel_format: "rgba16f".into(),
            channel_order: "RGBA".into(),
            row_order: "top_to_bottom".into(),
            byte_order: "little_endian".into(),
            clipping: "none".into(),
        },
        display: ImageFormat {
            color_space: ColorSpace::Srgb,
            transfer_function: "srgb".into(),
            alpha: "straight".into(),
            association_space: ColorSpace::Srgb,
            pixel_format: "rgba16_unorm_png".into(),
            channel_order: "RGBA".into(),
            row_order: "top_to_bottom".into(),
            byte_order: "big_endian".into(),
            clipping: "unit_interval_after_output_transform; no_tone_mapping".into(),
        },
        backend: backend.name().into(),
    };
    Ok(RenderedFrame { pixels, metadata })
}

fn validate_pixels(pixels: &[[f32; 4]], internal: bool) -> Result<(), RenderError> {
    if pixels.iter().any(|p| {
        p.iter().any(|v| !v.is_finite())
            || p[..3].iter().any(|v| v.abs() > 65504.0)
            || !(0.0..=1.0).contains(&p[3])
            || (internal && p[3] == 0.0 && p[..3].iter().any(|v| *v != 0.0))
    }) {
        return Err(RenderError::InvalidInput(
            "invalid finite/alpha/RGBA16F backend pixel".into(),
        ));
    }
    Ok(())
}
/// Row-major RGBA binary16, little endian, without a header. Alpha rounding to
/// zero also zeros RGB; positive tiny internal alpha receives no epsilon.
pub fn encode_rgba16f(pixels: &[[f32; 4]]) -> Result<Vec<u8>, RenderError> {
    validate_pixels(pixels, true)?;
    let mut bytes = Vec::with_capacity(pixels.len() * 8);
    for p in pixels {
        let mut p = p.map(f16::from_f32);
        if p[3] == f16::ZERO {
            p[..3].fill(f16::ZERO);
        }
        for component in p {
            bytes.extend(component.to_bits().to_le_bytes());
        }
    }
    Ok(bytes)
}
fn encode_png(pixels: &[[f32; 4]], size: [u32; 2]) -> Result<Vec<u8>, RenderError> {
    validate_pixels(pixels, false)?;
    let samples: Vec<u8> = pixels
        .iter()
        .flatten()
        .flat_map(|v| ((v.clamp(0.0, 1.0) * 65535.0).round() as u16).to_be_bytes())
        .collect();
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, size[0], size[1]);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Sixteen);
        encoder.set_source_srgb(png::SrgbRenderingIntent::RelativeColorimetric);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&samples)?;
        writer.finish()?;
    }
    Ok(bytes)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SequenceRequest {
    pub range: TimeRange,
    pub frame_rate: FrameRate,
    pub region: OutputRegion,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputFile {
    pub name: String,
    pub bytes: u64,
    pub sha256: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SequenceFrame {
    pub metadata: FrameMetadata,
    pub numeric: OutputFile,
    pub display: OutputFile,
    pub metadata_file: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SequenceMetadata {
    pub schema_version: u32,
    pub range: TimeRange,
    pub frame_rate: FrameRate,
    pub frame_grid: String,
    pub frames: Vec<SequenceFrame>,
}

/// Exact time-zero frame grid, including negative indices. Bounds are ceil of
/// rate * start/end, so every sample is in [start,end), without accumulated steps.
pub fn frame_samples(range: TimeRange, rate: FrameRate) -> Result<Vec<(i64, Time)>, RenderError> {
    let ceil = |t: Time| -> Result<i64, RenderError> {
        let t = rate.time_to_frame(t)?;
        t.floor()
            .checked_add(i64::from(t.numerator() % t.denominator() != 0))
            .ok_or(kronello_time::TimeError::Overflow.into())
    };
    let start = ceil(range.start())?;
    let end = ceil(range.end())?;
    let count = i128::from(end) - i128::from(start);
    if count > 1_000_000 {
        return Err(RenderError::UnsupportedFeature(
            "sequence exceeds one million frames".into(),
        ));
    }
    (start..end)
        .map(|i| Ok((i, rate.frame_to_time(i)?)))
        .collect()
}
struct OutputGuard<'a> {
    path: &'a Path,
    committed: bool,
}
impl Drop for OutputGuard<'_> {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_dir_all(self.path);
        }
    }
}
fn write_artifact(directory: &Path, name: String, bytes: &[u8]) -> Result<OutputFile, RenderError> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join(&name))?;
    use std::io::Write;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(OutputFile {
        name,
        bytes: bytes.len() as u64,
        sha256: format!("{:x}", Sha256::digest(bytes)),
    })
}
/// Creates an exclusively new output directory. Existing outputs are refused.
/// All files are rendered into staging; the sequence manifest is published last.
/// An error rolls back this invocation's new directory, never existing output.
/// This is synchronous offline export; job cancellation/resume is a later task.
pub fn render_sequence(
    snapshot: &RenderSnapshot,
    fonts: &[FontData<'_>],
    backend: &dyn RenderBackend,
    request: SequenceRequest,
    directory: impl AsRef<Path>,
) -> Result<SequenceMetadata, RenderError> {
    snapshot.validate()?;
    request.region.validate()?;
    let samples = frame_samples(request.range, request.frame_rate)?;
    let directory = directory.as_ref();
    fs::create_dir(directory)?;
    let mut guard = OutputGuard {
        path: directory,
        committed: false,
    };
    let staging = tempfile::Builder::new()
        .prefix(".render-staging-")
        .tempdir_in(directory)?;
    let mut frames = Vec::with_capacity(samples.len());
    for (ordinal, (index, time)) in samples.into_iter().enumerate() {
        let mut frame = render_frame(
            snapshot,
            fonts,
            backend,
            FrameRequest {
                time,
                region: request.region,
            },
        )?;
        frame.metadata.frame_index = Some(index.to_string());
        frame.metadata.sequence_number = Some(ordinal as u64);
        let numeric = write_artifact(
            staging.path(),
            format!("frame-{ordinal:08}.rgba16f"),
            &encode_rgba16f(&frame.pixels.linear)?,
        )?;
        let display = write_artifact(
            staging.path(),
            format!("frame-{ordinal:08}.png"),
            &encode_png(&frame.pixels.display, request.region.pixels)?,
        )?;
        let metadata_file = format!("frame-{ordinal:08}.json");
        write_artifact(
            staging.path(),
            metadata_file.clone(),
            &serde_json::to_vec_pretty(&frame.metadata)?,
        )?;
        frames.push(SequenceFrame {
            metadata: frame.metadata,
            numeric,
            display,
            metadata_file,
        });
    }
    let metadata = SequenceMetadata {
        schema_version: 1,
        range: request.range,
        frame_rate: request.frame_rate,
        frame_grid: "absolute_time_zero; ceil(start*fps)..ceil(end*fps)".into(),
        frames,
    };
    write_artifact(
        staging.path(),
        "sequence.json".into(),
        &serde_json::to_vec_pretty(&metadata)?,
    )?;
    for frame in &metadata.frames {
        for name in [
            &frame.numeric.name,
            &frame.display.name,
            &frame.metadata_file,
        ] {
            fs::rename(staging.path().join(name), directory.join(name))?;
        }
    }
    fs::rename(
        staging.path().join("sequence.json"),
        directory.join("sequence.json"),
    )?;
    staging.close()?;
    guard.committed = true;
    Ok(metadata)
}
