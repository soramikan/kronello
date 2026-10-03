//! Pure, bounded arbitrary-time curve evaluation. No playback state or backend.
//! Version 1 uses rational segment arithmetic and 64 bisection steps to invert
//! normalized time Beziers. Floating-point values are never saved as key times.

mod color;

use kronello_model::{
    AnimationCurve, ColorSpace, CurveError, CurveInterpolation, FiniteF64, ModelError, TimeBezier,
    Value,
};
use kronello_time::{Time, TimeError};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Error)]
pub enum AnimationError {
    #[error(transparent)]
    Curve(#[from] CurveError),
    #[error(transparent)]
    Value(#[from] ModelError),
    #[error(transparent)]
    Time(#[from] TimeError),
    #[error("curve {id} has no keys")]
    EmptyCurve { id: kronello_model::CurveId },
    #[error("working color space must be linear")]
    InvalidWorkingColorSpace,
}

/// Standalone sampling uses linear Rec.709. Sequence callers must use
/// sample_in_space with their working space (or the descriptor's explicit space).
pub fn sample(curve: &AnimationCurve, time: Time) -> Result<Value, AnimationError> {
    sample_in_space(curve, time, ColorSpace::LinearRec709)
}

/// Segments are [left.time, right.time); an exact key owns its stored value.
/// Before/after the keys, extend the nearest value without extrapolation.
/// Color values, including exact keys and Hold, are converted to working linear
/// straight RGB with independent alpha. Discrete values only support Hold.
/// Descriptor ranges are validated after the caller's complete modifier chain.
pub fn sample_in_space(
    curve: &AnimationCurve,
    time: Time,
    working_space: ColorSpace,
) -> Result<Value, AnimationError> {
    curve.ensure_supported_version()?;
    if working_space == ColorSpace::Srgb {
        return Err(AnimationError::InvalidWorkingColorSpace);
    }
    let keys = curve.keys();
    if keys.is_empty() {
        return Err(AnimationError::EmptyCurve { id: curve.id() });
    }
    let next = keys.partition_point(|key| key.time <= time);
    if next == 0 {
        return working_value(&keys[0].value, working_space);
    }
    let left = &keys[next - 1];
    if next == keys.len() || left.time == time || left.interpolation == CurveInterpolation::Hold {
        return working_value(&left.value, working_space);
    }
    let right = &keys[next];
    // Form differences before conversion: absolute f64 times would collapse
    // close rational keys at large offsets. Unrepresentable arithmetic is typed.
    let ratio = time
        .checked_sub(left.time)?
        .checked_div(right.time.checked_sub(left.time)?)?;
    let progress = ratio.numerator() as f64 / ratio.denominator() as f64;
    let progress = match left.interpolation {
        CurveInterpolation::Linear => progress,
        CurveInterpolation::Cubic(handles) => eased_progress(handles, progress)?,
        CurveInterpolation::Hold => unreachable!("handled before segment arithmetic"),
    };
    interpolate(
        &working_value(&left.value, working_space)?,
        &working_value(&right.value, working_space)?,
        progress,
    )
}

fn working_value(value: &Value, space: ColorSpace) -> Result<Value, AnimationError> {
    match value {
        Value::Color(value) => Ok(Value::Color(color::to_working(*value, space)?)),
        _ => Ok(value.clone()),
    }
}

fn blend(a: f64, b: f64, progress: f64) -> f64 {
    if a == b {
        a
    } else {
        (1.0 - progress) * a + progress * b
    }
}

fn bezier(a: f64, b: f64, parameter: f64) -> f64 {
    let ab = blend(0.0, a, parameter);
    let bc = blend(a, b, parameter);
    let cd = blend(b, 1.0, parameter);
    blend(
        blend(ab, bc, parameter),
        blend(bc, cd, parameter),
        parameter,
    )
}

fn eased_progress(handles: TimeBezier, progress: f64) -> Result<f64, AnimationError> {
    if progress == 0.0 || progress == 1.0 {
        return Ok(progress);
    }
    let [x1, y1] = handles.control1();
    let [x2, y2] = handles.control2();
    let (mut low, mut high) = (0.0, 1.0);
    // Fixed iteration count is independent of evaluation order or prior samples.
    // Bisection also handles a zero endpoint derivative without Newton failure.
    for _ in 0..64 {
        let mid = (low + high) * 0.5;
        if bezier(x1, x2, mid) < progress {
            low = mid;
        } else {
            high = mid;
        }
    }
    Ok(FiniteF64::new(bezier(y1, y2, (low + high) * 0.5))?.get())
}

fn interpolate(a: &Value, b: &Value, progress: f64) -> Result<Value, AnimationError> {
    let component = |a: FiniteF64, b: FiniteF64| FiniteF64::new(blend(a.get(), b.get(), progress));
    Ok(match (a, b) {
        (Value::Scalar(a), Value::Scalar(b)) => Value::Scalar(component(*a, *b)?),
        (Value::Angle(a), Value::Angle(b)) => Value::Angle(component(*a, *b)?),
        (Value::Vec2(a), Value::Vec2(b)) => {
            Value::Vec2([component(a[0], b[0])?, component(a[1], b[1])?])
        }
        (Value::Vec3(a), Value::Vec3(b)) => Value::Vec3([
            component(a[0], b[0])?,
            component(a[1], b[1])?,
            component(a[2], b[2])?,
        ]),
        (Value::Color(a), Value::Color(b)) => {
            let ac = a.components();
            let bc = b.components();
            Value::Color(kronello_model::Color::new(
                a.space(),
                [
                    component(ac.r, bc.r)?.get(),
                    component(ac.g, bc.g)?.get(),
                    component(ac.b, bc.b)?.get(),
                ],
                component(ac.alpha, bc.alpha)?.get(),
            )?)
        }
        _ => {
            return Err(ModelError::IncompatibleInterpolation {
                value_type: a.value_type(),
                mode: kronello_model::InterpolationMode::Linear,
            }
            .into());
        }
    })
}
