//! PNG source values retain their native 8/16-bit precision until conversion.
use crate::{MediaError, resolve_asset};
use kronello_model::{Asset, AssetKind, ColorSpace};
use kronello_render::VideoImage;
use std::{fs::File, io::BufReader, path::Path};

/// Static PNG images do not depend on presentation time. Other image codecs and
/// arbitrary ICC/chromaticity transforms are explicitly unsupported in version 1.
pub fn decode_image_asset(
    asset: &Asset,
    project_path: &Path,
    stream_index: u32,
    working: ColorSpace,
) -> Result<VideoImage, MediaError> {
    if asset.kind != AssetKind::Image || working == ColorSpace::Srgb {
        return Err(MediaError::UnsupportedFeature(
            "image asset/linear working space required".into(),
        ));
    }
    let stream = asset
        .streams
        .iter()
        .find(|s| s.index == stream_index)
        .ok_or_else(|| MediaError::InvalidInput("image stream missing".into()))?;
    // Camera RAW stills take their exclusive LibRaw path (ADR-0136); a locked
    // RAW codec must never fall through to PNG, and PNG never to LibRaw.
    if crate::raw::is_raw_still_codec(&stream.codec) {
        return crate::raw::decode_raw_image_asset(asset, project_path, stream_index, working);
    }
    if stream.codec != "png" {
        return Err(MediaError::UnsupportedFeature(format!(
            "native image codec {}",
            stream.codec
        )));
    }
    let path = resolve_asset(asset, project_path)?;
    let mut decoder = png::Decoder::new(BufReader::new(File::open(&path)?));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder
        .read_info()
        .map_err(|e| MediaError::Decode(e.to_string()))?;
    let info = reader.info();
    let expected_chromaticities =
        png::SourceChromaticities::new((0.3127, 0.3290), (0.64, 0.33), (0.30, 0.60), (0.15, 0.06));
    if info.icc_profile.is_some()
        || info
            .source_chromaticities
            .is_some_and(|c| c != expected_chromaticities)
        || info.animation_control.is_some()
    {
        return Err(MediaError::UnsupportedFeature(
            "PNG ICC/chromaticities/animation require an explicit supported color/source contract"
                .into(),
        ));
    }
    let native_format = match (info.color_type, info.bit_depth) {
        (png::ColorType::Rgb, png::BitDepth::Sixteen) => "rgb48be",
        (png::ColorType::Rgba, png::BitDepth::Sixteen) => "rgba64be",
        (png::ColorType::Grayscale, png::BitDepth::Sixteen) => "gray16be",
        (png::ColorType::GrayscaleAlpha, png::BitDepth::Sixteen) => "ya16be",
        (png::ColorType::Rgb, _) => "rgb24",
        (png::ColorType::Rgba, _) => "rgba",
        (png::ColorType::Grayscale, _) => "gray",
        (png::ColorType::GrayscaleAlpha, _) => "ya8",
        (png::ColorType::Indexed, _) => "pal8",
    };
    if stream.pixel_format.as_deref() != Some(native_format) {
        return Err(MediaError::InvalidInput(
            "PNG native format differs from locked metadata".into(),
        ));
    }
    let gamma = info.gamma().map(|g| g.into_scaled());
    let linear = gamma == Some(100000);
    if gamma.is_some_and(|g| g != 100000 && g != 45455) {
        return Err(MediaError::UnsupportedFeature("PNG transfer gamma".into()));
    }
    for (tag, allowed) in [
        (
            stream.color_primaries.as_deref(),
            &(["bt709", "unknown", "unspecified"])[..],
        ),
        (
            stream.color_matrix.as_deref(),
            &(["gbr", "unknown", "unspecified"])[..],
        ),
        (
            stream.color_range.as_deref(),
            &(["pc", "unknown", "unspecified"])[..],
        ),
    ] {
        if tag.is_some_and(|tag| !allowed.contains(&tag)) {
            return Err(MediaError::UnsupportedFeature(
                "PNG locked color tags".into(),
            ));
        }
    }
    if stream.color_transfer.as_deref().is_some_and(|tag| {
        !matches!(tag, "unknown" | "unspecified")
            && tag != if linear { "linear" } else { "iec61966-2-1" }
    }) {
        return Err(MediaError::UnsupportedFeature(
            "PNG source and locked transfer disagree".into(),
        ));
    }
    if stream.width != Some(info.width) || stream.height != Some(info.height) {
        return Err(MediaError::InvalidInput(
            "PNG dimensions differ from locked metadata".into(),
        ));
    }
    let bytes = reader
        .output_buffer_size()
        .ok_or_else(|| MediaError::Decode("PNG buffer size overflow".into()))?;
    let count = u64::from(info.width) * u64::from(info.height);
    if count.checked_mul(16).is_none_or(|b| b > 256 * 1024 * 1024) || bytes > 256 * 1024 * 1024 {
        return Err(MediaError::UnsupportedFeature(
            "PNG decoded surface budget exceeded".into(),
        ));
    }
    let mut buffer = vec![0; bytes];
    let output = reader
        .next_frame(&mut buffer)
        .map_err(|e| MediaError::Decode(e.to_string()))?;
    let sample_bytes = match output.bit_depth {
        png::BitDepth::Eight => 1,
        png::BitDepth::Sixteen => 2,
        _ => {
            return Err(MediaError::UnsupportedFeature(
                "PNG native bit depth".into(),
            ));
        }
    };
    let components = output.color_type.samples();
    let pixels = buffer[..output.buffer_size()]
        .chunks_exact(sample_bytes * components)
        .map(|p| {
            let value = |i| {
                if sample_bytes == 1 {
                    f64::from(p[i]) / 255.0
                } else {
                    f64::from(u16::from_be_bytes([p[2 * i], p[2 * i + 1]])) / 65535.0
                }
            };
            let (rgb, alpha) = match output.color_type {
                png::ColorType::Rgb => ([value(0), value(1), value(2)], 1.0),
                png::ColorType::Rgba => ([value(0), value(1), value(2)], value(3)),
                png::ColorType::Grayscale => ([value(0); 3], 1.0),
                png::ColorType::GrayscaleAlpha => ([value(0); 3], value(1)),
                png::ColorType::Indexed => unreachable!("EXPAND decodes palette"),
            };
            let mut rgb = rgb.map(|v| {
                if linear {
                    v
                } else if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            });
            if working == ColorSpace::LinearRec2020 {
                rgb = [
                    0.6274039 * rgb[0] + 0.3292830 * rgb[1] + 0.0433131 * rgb[2],
                    0.0690973 * rgb[0] + 0.9195404 * rgb[1] + 0.0113623 * rgb[2],
                    0.0163914 * rgb[0] + 0.0880133 * rgb[1] + 0.8955953 * rgb[2],
                ];
            }
            [
                (rgb[0] * alpha) as f32,
                (rgb[1] * alpha) as f32,
                (rgb[2] * alpha) as f32,
                alpha as f32,
            ]
        })
        .collect();
    resolve_asset(asset, project_path)?;
    Ok(VideoImage {
        size: [output.width, output.height],
        pixels,
    })
}
