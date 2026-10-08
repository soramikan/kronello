//! TRACK-003 (ADR-0123): deterministic intermediate-frame synthesis on
//! premultiplied linear `f32` RGBA pixels (the `VideoImage` pixel contract).
//!
//! Each output pixel reads its cell's forward/backward vector, samples the
//! lo/hi frames bilinearly at the warped positions, and weights the two
//! contributions by `(1 - f) * conf_fwd` and `f * conf_bwd`. Edge taps
//! replicate the border pixel. All arithmetic is sequential f32 in a fixed
//! scan order; identical inputs produce bit-identical outputs.

use crate::flow::{FlowError, FlowField};

/// Edge-replicated bilinear sample of `image` (premultiplied f32 RGBA, `width
/// x height`) at pixel-lattice position `(x, y)` where texel `i` spans
/// `[i, i+1)` and centers at `i + 0.5`.
fn bilinear(image: &[[f32; 4]], width: u32, height: u32, x: f32, y: f32) -> [f32; 4] {
    let (w, h) = (width as f32, height as f32);
    let (x, y) = (x.clamp(0.0, w - 1e-6), y.clamp(0.0, h - 1e-6));
    let (fx, fy) = (x - 0.5, y - 0.5);
    let (x0, y0) = (fx.floor().max(0.0), fy.floor().max(0.0));
    let (x1, y1) = ((x0 + 1.0).min(w - 1.0), (y0 + 1.0).min(h - 1.0));
    let (tx, ty) = ((fx - x0).clamp(0.0, 1.0), (fy - y0).clamp(0.0, 1.0));
    let p = |x: f32, y: f32| image[(y as u32 * width + x as u32) as usize];
    let (a, b, c, d) = (p(x0, y0), p(x1, y0), p(x0, y1), p(x1, y1));
    let mut out = [0.0f32; 4];
    for k in 0..4 {
        out[k] = a[k] * (1.0 - tx) * (1.0 - ty)
            + b[k] * tx * (1.0 - ty)
            + c[k] * (1.0 - tx) * ty
            + d[k] * tx * ty;
    }
    out
}

/// Flow field cell index for pixel `(x, y)`; the last row/column covers the
/// frame remainder.
fn cell_index(field: &FlowField, x: u32, y: u32) -> usize {
    let block = 2 * field.block_radius + 1;
    let (i, j) = (
        (x / block).min(field.grid[0] - 1),
        (y / block).min(field.grid[1] - 1),
    );
    (j * field.grid[0] + i) as usize
}

/// Bidirectional-warp intermediate frame at `fraction` ∈ `[0, 1]` between
/// `lo` (fraction 0) and `hi` (fraction 1). `fwd` maps `lo -> hi` positions;
/// `bwd` maps `hi -> lo`. Grid shapes must match the frame dimensions.
pub fn interpolate_frames(
    lo: &[[f32; 4]],
    hi: &[[f32; 4]],
    width: u32,
    height: u32,
    fwd: &FlowField,
    bwd: &FlowField,
    fraction: f64,
) -> Result<Vec<[f32; 4]>, FlowError> {
    let pixels = width as usize * height as usize;
    if lo.len() != pixels
        || hi.len() != pixels
        || !(0.0..=1.0).contains(&fraction)
        || !fraction.is_finite()
    {
        return Err(FlowError::InvalidInput("interpolate frame shape".into()));
    }
    let expected = (fwd.grid[0] * fwd.grid[1]) as usize;
    if fwd.vectors.len() != expected
        || bwd.vectors.len() != (bwd.grid[0] * bwd.grid[1]) as usize
        || fwd.confidence.len() != expected
    {
        return Err(FlowError::InvalidInput("flow field shape".into()));
    }
    let f = fraction as f32;
    let mut out = Vec::with_capacity(pixels);
    for y in 0..height {
        for x in 0..width {
            let fi = cell_index(fwd, x, y);
            let bi = cell_index(bwd, x, y);
            let fv = fwd.vectors[fi];
            let bv = bwd.vectors[bi];
            // Warped source positions on each neighbor frame.
            let sa = bilinear(
                lo,
                width,
                height,
                x as f32 - f * fv[0],
                y as f32 - f * fv[1],
            );
            let sb = bilinear(
                hi,
                width,
                height,
                x as f32 + (1.0 - f) * bv[0],
                y as f32 + (1.0 - f) * bv[1],
            );
            let wa = (1.0 - f) * fwd.confidence[fi];
            let wb = f * bwd.confidence[bi];
            let wsum = wa + wb;
            let mut px = [0.0f32; 4];
            for k in 0..4 {
                px[k] = if wsum > f32::EPSILON {
                    (sa[k] * wa + sb[k] * wb) / wsum
                } else {
                    sa[k] * (1.0 - f) + sb[k] * f
                };
            }
            out.push(px);
        }
    }
    Ok(out)
}

/// Explicit authored crossfade fallback (ADR-0123 `flow_fallback: "blend"`).
/// Reached only when the document opts in; never a silent flow substitute.
pub fn blend_frames(
    lo: &[[f32; 4]],
    hi: &[[f32; 4]],
    fraction: f64,
) -> Result<Vec<[f32; 4]>, FlowError> {
    if lo.len() != hi.len() || !(0.0..=1.0).contains(&fraction) {
        return Err(FlowError::InvalidInput("blend frame shape".into()));
    }
    let f = fraction as f32;
    Ok(lo
        .iter()
        .zip(hi)
        .map(|(a, b)| std::array::from_fn(|k| a[k] * (1.0 - f) + b[k] * f))
        .collect())
}
