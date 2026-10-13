//! Direct packed delivery conversion on the existing frame's row lattice.
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OutputColor {
    Sdr { tone_map: bool },
    Hdr(kronello_render::HdrTransfer),
}
impl OutputColor {
    pub fn stride(self) -> usize {
        if matches!(self, Self::Hdr(_)) { 8 } else { 4 }
    }
}
fn sdr_pixel(
    pixel: [f32; 4],
    background: [f32; 3],
    destination: &mut [u8],
) -> Result<(), MediaError> {
    for channel in 0..3 {
        // Preserve the original f32 operations and transfer/rounding boundary.
        let value = pixel[channel] + background[channel] * (1.0 - pixel[3]);
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(MediaError::UnsupportedFeature(
                "SDR export cannot encode out-of-gamut/HDR samples".into(),
            ));
        }
        let encoded = if value < 0.018 {
            value * 4.5
        } else {
            1.099 * value.powf(0.45) - 0.099
        };
        destination[channel] = (encoded * 255.0).round().clamp(0.0, 255.0) as u8;
    }
    destination[3] = 255;
    Ok(())
}

fn hdr_pixel(
    pixel: [f32; 4],
    background: [f32; 3],
    transfer: kronello_render::HdrTransfer,
    destination: &mut [u8],
) -> Result<(), MediaError> {
    let alpha = f64::from(pixel[3]);
    if !alpha.is_finite() || !(0.0..=1.0).contains(&alpha) {
        return Err(MediaError::InvalidInput("HDR alpha".into()));
    }
    let rgb = [0, 1, 2].map(|i| f64::from(pixel[i]) + f64::from(background[i]) * (1.0 - alpha));
    let code = transfer.encode(rgb).ok_or_else(|| {
        MediaError::UnsupportedFeature(
            "HDR output outside transfer range; explicit gamut/tone conversion required".into(),
        )
    })?;
    for (index, value) in code.into_iter().enumerate() {
        destination[index * 2..index * 2 + 2]
            .copy_from_slice(&((value * 65535.0).round() as u16).to_le_bytes());
    }
    destination[6..8].copy_from_slice(&u16::MAX.to_le_bytes());
    Ok(())
}
pub(super) fn packed_into(
    pixels: &[[f32; 4]],
    background: [f32; 3],
    color: OutputColor,
    destination: &mut [u8],
    row_stride: usize,
    offset: [u32; 2],
    tile: [u32; 2],
) -> Result<(), MediaError> {
    let width = tile[0] as usize;
    let height = tile[1] as usize;
    let bytes = color.stride();
    let row_bytes = width
        .checked_mul(bytes)
        .ok_or_else(|| MediaError::InvalidInput("color row overflow".into()))?;
    let x = (offset[0] as usize)
        .checked_mul(bytes)
        .ok_or_else(|| MediaError::InvalidInput("color offset overflow".into()))?;
    let last = (offset[1] as usize)
        .checked_add(height.saturating_sub(1))
        .and_then(|row| row.checked_mul(row_stride))
        .and_then(|start| start.checked_add(x))
        .and_then(|start| start.checked_add(row_bytes));
    if width == 0
        || height == 0
        || width.checked_mul(height) != Some(pixels.len())
        || x.checked_add(row_bytes).is_none_or(|end| end > row_stride)
        || last.is_none_or(|end| end > destination.len())
    {
        return Err(MediaError::InvalidInput(
            "color destination stride/offset/pixel count".into(),
        ));
    }
    for (row, source) in pixels.chunks_exact(width).enumerate() {
        let start = (offset[1] as usize + row) * row_stride + x;
        for (pixel, packed) in source
            .iter()
            .zip(destination[start..start + row_bytes].chunks_exact_mut(bytes))
        {
            match color {
                OutputColor::Hdr(transfer) => hdr_pixel(*pixel, background, transfer, packed)?,
                OutputColor::Sdr { tone_map: false } => sdr_pixel(*pixel, background, packed)?,
                OutputColor::Sdr { tone_map: true } => {
                    // The old intermediate Vec stored this exact f32 boundary.
                    let mapped = kronello_render::hdr_to_sdr_linear([
                        pixel[0] + background[0] * (1.0 - pixel[3]),
                        pixel[1] + background[1] * (1.0 - pixel[3]),
                        pixel[2] + background[2] * (1.0 - pixel[3]),
                        1.0,
                    ]);
                    sdr_pixel(mapped, [0.0; 3], packed)?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oracle(
        pixels: &[[f32; 4]],
        background: [f32; 3],
        color: OutputColor,
    ) -> Result<Vec<u8>, MediaError> {
        match color {
            OutputColor::Hdr(transfer) => hdr_rgba(pixels, background, transfer),
            OutputColor::Sdr { tone_map: false } => bt709_rgba(pixels, background),
            OutputColor::Sdr { tone_map: true } => {
                let mapped: Vec<_> = pixels
                    .iter()
                    .map(|pixel| {
                        kronello_render::hdr_to_sdr_linear([
                            pixel[0] + background[0] * (1.0 - pixel[3]),
                            pixel[1] + background[1] * (1.0 - pixel[3]),
                            pixel[2] + background[2] * (1.0 - pixel[3]),
                            1.0,
                        ])
                    })
                    .collect();
                bt709_rgba(&mapped, [0.0; 3])
            }
        }
    }

    #[test]
    fn packed_rows_match_old_transfer_and_preserve_padding() {
        for color in [
            OutputColor::Sdr { tone_map: false },
            OutputColor::Sdr { tone_map: true },
            OutputColor::Hdr(kronello_render::HdrTransfer::Pq),
            OutputColor::Hdr(kronello_render::HdrTransfer::Hlg),
        ] {
            let scale = if color == (OutputColor::Sdr { tone_map: false }) {
                1.0
            } else {
                4.0
            };
            let mut pixels: Vec<_> = (0..4096)
                .map(|i| {
                    let alpha = (i % 257) as f32 / 256.0;
                    [
                        ((i * 17) % 4096) as f32 / 4096.0 * alpha * scale,
                        ((i * 31) % 4096) as f32 / 4096.0 * alpha * scale,
                        ((i * 43) % 4096) as f32 / 4096.0 * alpha * scale,
                        alpha,
                    ]
                })
                .collect();
            // Include adjacent floats at the SDR piecewise transfer boundary.
            pixels[0] = [
                f32::from_bits(0.018_f32.to_bits() - 1),
                0.018,
                f32::from_bits(0.018_f32.to_bits() + 1),
                1.0,
            ];
            pixels[1] = [0.0, -0.0, 1.0, 1.0];
            let background = [0.125, 0.25, 0.5];
            let expected = oracle(&pixels, background, color).unwrap();
            let stride = 70 * color.stride() + 7;
            let mut destination = vec![0xa5; stride * 68];
            let original = destination.clone();
            packed_into(
                &pixels,
                background,
                color,
                &mut destination,
                stride,
                [3, 2],
                [64, 64],
            )
            .unwrap();
            for row in 0..68 {
                for byte in 0..stride {
                    let offset = row * stride + byte;
                    if (2..66).contains(&row)
                        && (3 * color.stride()..67 * color.stride()).contains(&byte)
                    {
                        assert_eq!(
                            destination[offset],
                            expected[(row - 2) * 64 * color.stride() + byte - 3 * color.stride()]
                        );
                    } else {
                        assert_eq!(destination[offset], original[offset]);
                    }
                }
            }
        }
    }

    #[test]
    fn invalid_samples_keep_original_error_and_bad_lattice_never_writes() {
        for color in [
            OutputColor::Sdr { tone_map: false },
            OutputColor::Sdr { tone_map: true },
            OutputColor::Hdr(kronello_render::HdrTransfer::Pq),
            OutputColor::Hdr(kronello_render::HdrTransfer::Hlg),
        ] {
            for pixel in [
                [f32::NAN, 0.0, 0.0, 1.0],
                [f32::INFINITY, 0.0, 0.0, 1.0],
                [-1.0, 0.0, 0.0, 1.0],
                [100.0, 0.0, 0.0, 1.0],
                [0.0, 0.0, 0.0, f32::NAN],
            ] {
                let expected = oracle(&[pixel], [0.0; 3], color);
                let mut bytes = vec![0; color.stride()];
                let actual = packed_into(
                    &[pixel],
                    [0.0; 3],
                    color,
                    &mut bytes,
                    color.stride(),
                    [0, 0],
                    [1, 1],
                );
                match (expected, actual) {
                    (Ok(expected), Ok(())) => assert_eq!(expected, bytes),
                    (Err(expected), Err(actual)) => {
                        assert_eq!(expected.to_string(), actual.to_string())
                    }
                    results => panic!("different outcome: {results:?}"),
                }
            }
        }
        for (stride, offset, tile) in [
            (4, [1, 0], [1, 1]),
            (8, [0, 4], [1, 1]),
            (usize::MAX, [0, u32::MAX], [1, 1]),
            (8, [0, 0], [2, 1]),
            (8, [0, 0], [0, 1]),
        ] {
            let mut destination = [0xa5; 16];
            assert!(
                packed_into(
                    &[[0.0; 4]],
                    [0.0; 3],
                    OutputColor::Sdr { tone_map: false },
                    &mut destination,
                    stride,
                    offset,
                    tile
                )
                .is_err()
            );
            assert_eq!(destination, [0xa5; 16]);
        }
    }
}
