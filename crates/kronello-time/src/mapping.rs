use crate::{Duration, Rational, Time, TimeError};
use serde::{Deserialize, Serialize};

/// An exact parent/local control point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TimeMapPoint {
    pub parent: Time,
    pub local: Time,
}

/// TRACK-003 (ADR-0123): the explicit failure policy authored on an
/// optical-flow interpolation. There is no silent fallback; `blend` is the
/// only permitted degraded path and must be authored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FlowFallbackPolicy {
    Blend,
}

/// TRACK-003 (ADR-0123): deterministic pyramidal block-matching parameters.
/// Integer radii bound the search cost; confidence thresholds are exact
/// rationals in `[0, 1]` so the whole configuration stays `Eq` and hashes
/// identically through the document/caches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpticalFlowConfig {
    /// Half-size of the matched block; the block spans `2*r+1` pixels.
    pub block_radius: u32,
    /// Half-size of the exhaustive search window at the coarsest level.
    pub search_radius: u32,
    /// Pyramid levels including the base image.
    pub levels: u32,
    /// Per-cell forward/backward consistency confidence floor in `[0, 1]`.
    pub confidence_floor: Rational,
    /// Maximum tolerated fraction of low-confidence cells in `[0, 1]`;
    /// exceeding it rejects with `FLOW_CONFIDENCE_LOW` (or blends, when
    /// `flow_fallback` is authored).
    pub max_low_confidence: Rational,
    /// Explicit degraded-mode policy; `None` rejects on low confidence.
    pub flow_fallback: Option<FlowFallbackPolicy>,
}

impl OpticalFlowConfig {
    /// Conservative defaults used when callers construct a config without
    /// specific tuning; all fields remain explicit in the document.
    pub fn default_config() -> Self {
        Self {
            block_radius: 4,
            search_radius: 8,
            levels: 3,
            confidence_floor: Rational::new(1, 4).expect("static rational"),
            max_low_confidence: Rational::new(1, 2).expect("static rational"),
            flow_fallback: None,
        }
    }

    /// Bounds and finiteness enforced at construction and deserialization.
    pub fn validate(&self) -> Result<(), TimeError> {
        let unit = |v: Rational| v >= Rational::ZERO && v <= Rational::ONE;
        if !(1..=16).contains(&self.block_radius)
            || !(1..=64).contains(&self.search_radius)
            || !(1..=6).contains(&self.levels)
            || !unit(self.confidence_floor)
            || !unit(self.max_low_confidence)
        {
            return Err(TimeError::InvalidFrameInterpolation);
        }
        Ok(())
    }
}

impl Default for OpticalFlowConfig {
    fn default() -> Self {
        Self::default_config()
    }
}

/// TRACK-003 (ADR-0123): intermediate-frame synthesis mode attached to a
/// piecewise retime. Absence keeps the legacy nearest-frame sampling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum FrameInterpolation {
    /// Bidirectional flow warps the neighboring decoded frames.
    OpticalFlow(OpticalFlowConfig),
}

/// Exact, stateless mappings. Protected middle segments support hold and loop;
/// piecewise linear segments with equal `local` endpoints express authored
/// hold (freeze) intervals. Reverse playback remains unsupported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(try_from = "MapWire", into = "MapWire")]
#[non_exhaustive]
pub enum TimeMap {
    Linear(LinearTimeMap),
    PiecewiseLinear(PiecewiseTimeMap),
    Protected(ProtectedTimeMap),
}

/// A validated positive-speed affine mapping over all representable times.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinearTimeMap {
    offset: Time,
    speed: Rational,
}

/// Validated control points: strictly increasing `parent` and non-decreasing
/// `local`. Equal adjacent `local` values form a hold segment over which the
/// mapped source time stays pinned. No extrapolation is performed. The
/// optional `interpolation` mode selects intermediate-frame synthesis when
/// the mapped source time lands inside a source frame's presentation
/// interval; `None` preserves the legacy single-frame sample.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PiecewiseTimeMap {
    points: Vec<TimeMapPoint>,
    interpolation: Option<FrameInterpolation>,
}

/// Only the middle is retimed; intro/outro retain unit speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProtectedMiddleMode {
    Hold,
    Loop,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectedTimeMap {
    authoring: Duration,
    requested: Duration,
    intro: Duration,
    outro: Duration,
    mode: ProtectedMiddleMode,
}
impl TimeMap {
    pub fn protected(
        authoring: Duration,
        requested: Duration,
        intro: Duration,
        outro: Duration,
        mode: ProtectedMiddleMode,
    ) -> Result<Self, TimeError> {
        let protected = intro.as_time().checked_add(outro.as_time())?;
        if authoring.as_time() <= protected || requested.as_time() <= protected {
            return Err(TimeError::OutsideMapDomain);
        }
        Ok(Self::Protected(ProtectedTimeMap {
            authoring,
            requested,
            intro,
            outro,
            mode,
        }))
    }

    /// local = offset + parent * speed. Speed is dimensionless and positive.
    pub fn linear(offset: Time, speed: Rational) -> Result<Self, TimeError> {
        if speed <= Rational::ZERO {
            return Err(TimeError::UnsupportedMapSlope);
        }
        Ok(Self::Linear(LinearTimeMap { offset, speed }))
    }

    /// At least two points, in strictly increasing parent order and
    /// non-decreasing local order. Equal adjacent `local` values form a hold
    /// (freeze) segment; a decreasing `local` edge is rejected with
    /// `TimeMapError::UnsupportedMapSlope`. The evaluation domain includes
    /// both the first and last control point.
    pub fn piecewise_linear(points: Vec<TimeMapPoint>) -> Result<Self, TimeError> {
        Self::piecewise_linear_with_interpolation(points, None)
    }

    /// TRACK-003 (ADR-0123): the same validated control points plus an
    /// optional intermediate-frame synthesis mode. The mode is part of the
    /// map identity and follows the map through serialization and hashing.
    pub fn piecewise_linear_with_interpolation(
        points: Vec<TimeMapPoint>,
        interpolation: Option<FrameInterpolation>,
    ) -> Result<Self, TimeError> {
        if points.len() < 2 {
            return Err(TimeError::TooFewMapPoints);
        }
        for pair in points.windows(2) {
            if pair[0].parent >= pair[1].parent {
                return Err(TimeError::UnorderedMapPoints);
            }
            if pair[0].local > pair[1].local {
                return Err(TimeError::UnsupportedMapSlope);
            }
        }
        if let Some(FrameInterpolation::OpticalFlow(config)) = interpolation {
            config.validate()?;
        }
        Ok(Self::PiecewiseLinear(PiecewiseTimeMap {
            points,
            interpolation,
        }))
    }

    /// The authored intermediate-frame synthesis mode, if any.
    pub fn interpolation(&self) -> Option<FrameInterpolation> {
        match self {
            Self::PiecewiseLinear(map) => map.interpolation,
            _ => None,
        }
    }

    /// Evaluate without frame snapping, mutable state, or implicit clamping.
    /// Simulation inputs use the forward authored clock; protected playback
    /// chooses displayed state but never changes its dynamics history.
    pub fn canonical_source_clock(&self) -> Result<Self, TimeError> {
        match self {
            Self::Protected(_) => Self::linear(Time::ZERO, Rational::ONE),
            _ => Ok(self.clone()),
        }
    }
    /// Exact inverse of the canonical monotone clock, including endpoints.
    /// Piecewise maps are non-injective across a hold segment, so the inverse
    /// is deterministic: a local value inside or on a hold resolves to the
    /// hold segment's starting parent (the earliest parent mapping to it).
    pub fn inverse_canonical(&self, local: Time) -> Result<Time, TimeError> {
        match self {
            Self::Protected(_) => Ok(local),
            Self::Linear(map) => local.checked_sub(map.offset)?.checked_div(map.speed),
            Self::PiecewiseLinear(map) => {
                let points = &map.points;
                if local < points[0].local || local > points[points.len() - 1].local {
                    return Err(TimeError::OutsideMapDomain);
                }
                let index = points.partition_point(|point| point.local < local);
                let right = points[index];
                if local == right.local {
                    return Ok(right.parent);
                }
                let left = points[index - 1];
                let fraction = local
                    .checked_sub(left.local)?
                    .checked_div(right.local.checked_sub(left.local)?)?;
                left.parent.checked_add(
                    right
                        .parent
                        .checked_sub(left.parent)?
                        .checked_mul(fraction)?,
                )
            }
        }
    }

    /// Each arithmetic step must be representable by the rational contract.
    pub fn map(&self, parent: Time) -> Result<Time, TimeError> {
        match self {
            Self::Linear(map) => map.offset.checked_add(parent.checked_mul(map.speed)?),
            Self::Protected(map) => {
                if parent < Time::ZERO || parent > map.requested.as_time() {
                    return Err(TimeError::OutsideMapDomain);
                }
                let intro = map.intro.as_time();
                let end = map.requested.as_time().checked_sub(map.outro.as_time())?;
                let source_end = map.authoring.as_time().checked_sub(map.outro.as_time())?;
                if parent < intro {
                    return Ok(parent);
                }
                if parent >= end {
                    return source_end.checked_add(parent.checked_sub(end)?);
                }
                match map.mode {
                    ProtectedMiddleMode::Hold => Ok(intro),
                    ProtectedMiddleMode::Loop => {
                        let period = source_end.checked_sub(intro)?;
                        let elapsed = parent.checked_sub(intro)?;
                        let cycles = elapsed.checked_div(period)?.floor();
                        intro.checked_add(
                            elapsed.checked_sub(period.checked_mul(Time::from_integer(cycles))?)?,
                        )
                    }
                }
            }
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

    /// TRACK-003 (ADR-0123): the authored synthesis mode for this map.
    pub fn interpolation(&self) -> Option<FrameInterpolation> {
        self.interpolation
    }

    /// Segment index owning `parent`. A control point belongs to the segment
    /// it starts; times before the first control point resolve to segment 0
    /// and times past the last point to the final segment, matching the
    /// constant-slope extension used by audio resampling.
    fn segment_index(&self, parent: Time) -> usize {
        let points = &self.points;
        (points
            .partition_point(|point| point.parent <= parent)
            .saturating_sub(1))
        .min(points.len() - 2)
    }

    /// Whether `parent` falls on a hold segment (zero source-time advance).
    /// Construction guarantees at least two points, so `segment_index` never
    /// underflows.
    pub fn is_hold(&self, parent: Time) -> bool {
        let segment = self.segment_index(parent);
        self.points[segment].local == self.points[segment + 1].local
    }

    /// The rational source-time rate of the segment owning `parent`. A hold
    /// segment reports zero; out-of-domain times use the extension rule from
    /// `segment_index`.
    pub fn slope_at(&self, parent: Time) -> Result<Rational, TimeError> {
        let segment = self.segment_index(parent);
        let left = &self.points[segment];
        let right = &self.points[segment + 1];
        right
            .local
            .checked_sub(left.local)?
            .checked_div(right.parent.checked_sub(left.parent)?)
    }
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum MapWire {
    Linear {
        offset: Time,
        speed: Rational,
    },
    PiecewiseLinear {
        points: Vec<TimeMapPoint>,
        /// TRACK-003: absent keeps the legacy single-frame sample.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        interpolation: Option<FrameInterpolation>,
    },
    Protected {
        authoring: Duration,
        requested: Duration,
        intro: Duration,
        outro: Duration,
        mode: ProtectedMiddleMode,
    },
}

impl TryFrom<MapWire> for TimeMap {
    type Error = TimeError;

    fn try_from(value: MapWire) -> Result<Self, Self::Error> {
        match value {
            MapWire::Linear { offset, speed } => Self::linear(offset, speed),
            MapWire::PiecewiseLinear {
                points,
                interpolation,
            } => Self::piecewise_linear_with_interpolation(points, interpolation),
            MapWire::Protected {
                authoring,
                requested,
                intro,
                outro,
                mode,
            } => Self::protected(authoring, requested, intro, outro, mode),
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
            TimeMap::PiecewiseLinear(map) => Self::PiecewiseLinear {
                points: map.points,
                interpolation: map.interpolation,
            },
            TimeMap::Protected(map) => Self::Protected {
                authoring: map.authoring,
                requested: map.requested,
                intro: map.intro,
                outro: map.outro,
                mode: map.mode,
            },
        }
    }
}
