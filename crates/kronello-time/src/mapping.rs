use crate::{Rational, Time, TimeError};
use serde::{Deserialize, Serialize};

/// An exact parent/local control point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TimeMapPoint {
    pub parent: Time,
    pub local: Time,
}

/// Exact, stateless mappings. Reverse playback, looping, stopping and nonlinear
/// mapping are unsupported. Future nonlinear variants require a versioned
/// quantization, rounding and evaluation contract; no floating time is stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(try_from = "MapWire", into = "MapWire")]
#[non_exhaustive]
pub enum TimeMap {
    Linear(LinearTimeMap),
    PiecewiseLinear(PiecewiseTimeMap),
}

/// A validated positive-speed affine mapping over all representable times.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinearTimeMap {
    offset: Time,
    speed: Rational,
}

/// Validated strictly increasing control points; no extrapolation is performed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PiecewiseTimeMap {
    points: Vec<TimeMapPoint>,
}

impl TimeMap {
    /// local = offset + parent * speed. Speed is dimensionless and positive.
    pub fn linear(offset: Time, speed: Rational) -> Result<Self, TimeError> {
        if speed <= Rational::ZERO {
            return Err(TimeError::UnsupportedMapSlope);
        }
        Ok(Self::Linear(LinearTimeMap { offset, speed }))
    }

    /// At least two points, in strictly increasing parent and local order.
    /// The evaluation domain includes both the first and last control point.
    pub fn piecewise_linear(points: Vec<TimeMapPoint>) -> Result<Self, TimeError> {
        if points.len() < 2 {
            return Err(TimeError::TooFewMapPoints);
        }
        for pair in points.windows(2) {
            if pair[0].parent >= pair[1].parent {
                return Err(TimeError::UnorderedMapPoints);
            }
            if pair[0].local >= pair[1].local {
                return Err(TimeError::UnsupportedMapSlope);
            }
        }
        Ok(Self::PiecewiseLinear(PiecewiseTimeMap { points }))
    }

    /// Evaluate without frame snapping, mutable state, or implicit clamping.
    /// Each arithmetic step must be representable by the rational contract.
    pub fn map(&self, parent: Time) -> Result<Time, TimeError> {
        match self {
            Self::Linear(map) => map.offset.checked_add(parent.checked_mul(map.speed)?),
            Self::PiecewiseLinear(map) => {
                // Construction guarantees at least two ordered points.
                let points = &map.points;
                if parent < points[0].parent || parent > points[points.len() - 1].parent {
                    return Err(TimeError::OutsideMapDomain);
                }
                let index = points.partition_point(|point| point.parent < parent);
                let right = points[index];
                if parent == right.parent {
                    return Ok(right.local);
                }
                let left = points[index - 1];
                let fraction = parent
                    .checked_sub(left.parent)?
                    .checked_div(right.parent.checked_sub(left.parent)?)?;
                left.local
                    .checked_add(right.local.checked_sub(left.local)?.checked_mul(fraction)?)
            }
        }
    }
}

impl LinearTimeMap {
    pub const fn offset(&self) -> Time {
        self.offset
    }
    pub const fn speed(&self) -> Rational {
        self.speed
    }
}

impl PiecewiseTimeMap {
    pub fn points(&self) -> &[TimeMapPoint] {
        &self.points
    }
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum MapWire {
    Linear { offset: Time, speed: Rational },
    PiecewiseLinear { points: Vec<TimeMapPoint> },
}

impl TryFrom<MapWire> for TimeMap {
    type Error = TimeError;

    fn try_from(value: MapWire) -> Result<Self, Self::Error> {
        match value {
            MapWire::Linear { offset, speed } => Self::linear(offset, speed),
            MapWire::PiecewiseLinear { points } => Self::piecewise_linear(points),
        }
    }
}

impl From<TimeMap> for MapWire {
    fn from(value: TimeMap) -> Self {
        match value {
            TimeMap::Linear(map) => Self::Linear {
                offset: map.offset,
                speed: map.speed,
            },
            TimeMap::PiecewiseLinear(map) => Self::PiecewiseLinear { points: map.points },
        }
    }
}
