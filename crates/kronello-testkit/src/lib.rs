//! CPU test comparisons and local fixture lookup. This crate neither evaluates scenes nor renders images.
//!
//! Semantic comparisons are exact. Pixel comparisons require matching descriptors,
//! finite linear premultiplied RGBA, and every pixel to satisfy the tolerance.

use std::fmt::{Debug, Display, Formatter};

mod fixtures;
pub use fixtures::{EXTERNAL_FIXTURE_DIR_ENV, FixtureError, FixtureResolver, resolve_fixture};

/// A diagnostic retaining the location and both semantic values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticMismatch {
    pub path: String,
    pub expected: String,
    pub actual: String,
}

impl Display for SemanticMismatch {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: expected {}, got {}",
            self.path, self.expected, self.actual
        )
    }
}

impl std::error::Error for SemanticMismatch {}

/// Compare values, layouts, IDs, or serialized snapshots without pixel tolerance.
pub fn compare_semantic<T: PartialEq + Debug + ?Sized>(
    path: &str,
    expected: &T,
    actual: &T,
) -> Result<(), SemanticMismatch> {
    if expected == actual {
        Ok(())
    } else {
        Err(SemanticMismatch {
            path: path.to_owned(),
            expected: format!("{expected:?}"),
            actual: format!("{actual:?}"),
        })
    }
}

/// Exact finite float comparison for geometry and evaluated numeric properties.
pub fn compare_finite_values(
    path: &str,
    expected: &[f64],
    actual: &[f64],
) -> Result<(), SemanticMismatch> {
    if expected.iter().chain(actual).any(|v| !v.is_finite()) {
        return Err(SemanticMismatch {
            path: path.to_owned(),
            expected: "finite semantic values".to_owned(),
            actual: "non-finite value".to_owned(),
        });
    }
    compare_semantic(path, expected, actual)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkingSpace {
    LinearRec709,
    LinearRec2020,
}

/// Metadata must match before comparing any pixel. Time is a normalized rational.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameDescriptor {
    pub width: u32,
    pub height: u32,
    /// Origin of the requested region in output pixels.
    pub origin: [i32; 2],
    pub time: [i64; 2],
    pub working_space: WorkingSpace,
    pub color_pipeline_id: String,
    pub samples_per_frame: u32,
    pub seed: u64,
}

/// Decoded CPU pixels in linear working space with premultiplied alpha.
/// RGBA16F callers must decode little-endian binary16 without an SDR transform.
#[derive(Debug, Clone, Copy)]
pub struct LinearFrame<'a> {
    pub descriptor: &'a FrameDescriptor,
    pub pixels: &'a [[f32; 4]],
}

/// Version 1 provisional thresholds from the fixed-environment golden policy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PixelTolerance {
    pub rgb_absolute: f64,
    pub rgb_relative: f64,
    pub alpha_absolute: f64,
}

impl Default for PixelTolerance {
    fn default() -> Self {
        Self {
            rgb_absolute: 1.0 / 1024.0,
            rgb_relative: 1.0 / 1024.0,
            alpha_absolute: 1.0 / 1024.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PixelReport {
    pub compared_pixels: usize,
    pub mismatched_pixels: usize,
    pub first_mismatch: Option<usize>,
    pub max_rgb_error: f64,
    pub max_alpha_error: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PixelError {
    InvalidTolerance,
    InvalidDescriptor,
    DescriptorMismatch,
    BufferLength {
        expected: usize,
        actual: usize,
    },
    InvalidPixel {
        side: &'static str,
        index: usize,
        reason: &'static str,
    },
    Mismatch(PixelReport),
}

impl Display for PixelError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "pixel comparison failed: {self:?}")
    }
}

impl std::error::Error for PixelError {}

fn validate_frame(frame: LinearFrame<'_>, side: &'static str) -> Result<(), PixelError> {
    let d = frame.descriptor;
    let [num, den] = d.time;
    let mut a = num.unsigned_abs();
    let mut b = den.unsigned_abs();
    while b != 0 {
        (a, b) = (b, a % b);
    }
    if d.width == 0
        || d.height == 0
        || den <= 0
        || a != 1
        || d.samples_per_frame == 0
        || d.color_pipeline_id.is_empty()
    {
        return Err(PixelError::InvalidDescriptor);
    }
    let count = usize::try_from(d.width)
        .ok()
        .and_then(|w| {
            usize::try_from(d.height)
                .ok()
                .and_then(|h| w.checked_mul(h))
        })
        .ok_or(PixelError::InvalidDescriptor)?;
    if frame.pixels.len() != count {
        return Err(PixelError::BufferLength {
            expected: count,
            actual: frame.pixels.len(),
        });
    }
    for (index, pixel) in frame.pixels.iter().enumerate() {
        let reason = if pixel.iter().any(|v| !v.is_finite()) {
            Some("non-finite component")
        } else if !(0.0..=1.0).contains(&pixel[3]) {
            Some("alpha outside [0, 1]")
        } else if pixel[3] == 0.0 && pixel[..3].iter().any(|v| *v != 0.0) {
            Some("nonzero RGB at zero alpha")
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(PixelError::InvalidPixel {
                side,
                index,
                reason,
            });
        }
    }
    Ok(())
}

/// Compare all components, preserving negative and greater-than-one HDR values.
/// No registration, edge exclusion, clamping, blur, or outlier allowance is applied.
/// Environment fingerprint validation remains the responsibility of the GPU harness.
pub fn compare_pixels(
    expected: LinearFrame<'_>,
    actual: LinearFrame<'_>,
    tolerance: PixelTolerance,
) -> Result<PixelReport, PixelError> {
    if [
        tolerance.rgb_absolute,
        tolerance.rgb_relative,
        tolerance.alpha_absolute,
    ]
    .iter()
    .any(|v| !v.is_finite() || *v < 0.0)
    {
        return Err(PixelError::InvalidTolerance);
    }
    validate_frame(expected, "expected")?;
    validate_frame(actual, "actual")?;
    if expected.descriptor != actual.descriptor {
        return Err(PixelError::DescriptorMismatch);
    }
    let mut report = PixelReport {
        compared_pixels: expected.pixels.len(),
        mismatched_pixels: 0,
        first_mismatch: None,
        max_rgb_error: 0.0,
        max_alpha_error: 0.0,
    };
    for (index, (e, a)) in expected.pixels.iter().zip(actual.pixels).enumerate() {
        let mut mismatch = false;
        for channel in 0..4 {
            let difference = (f64::from(a[channel]) - f64::from(e[channel])).abs();
            let limit = if channel == 3 {
                report.max_alpha_error = report.max_alpha_error.max(difference);
                tolerance.alpha_absolute
            } else {
                report.max_rgb_error = report.max_rgb_error.max(difference);
                tolerance
                    .rgb_absolute
                    .max(tolerance.rgb_relative * f64::from(e[channel]).abs())
            };
            if !limit.is_finite() {
                return Err(PixelError::InvalidTolerance);
            }
            mismatch |= difference > limit;
        }
        if mismatch {
            report.mismatched_pixels += 1;
            report.first_mismatch.get_or_insert(index);
        }
    }
    if report.mismatched_pixels == 0 {
        Ok(report)
    } else {
        Err(PixelError::Mismatch(report))
    }
}
