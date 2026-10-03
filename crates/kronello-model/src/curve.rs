use crate::{CurveId, FiniteF64, InterpolationMode, ModelError, Value, ValueType};
use kronello_time::Time;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Meaning version, independent of the document schema or application version.
pub const INTERPOLATION_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Error)]
pub enum CurveError {
    #[error(transparent)]
    Value(#[from] ModelError),
    #[error("time handles must satisfy 0 <= x1 <= x2 <= 1")]
    NonMonotonicHandles,
    #[error("duplicate key time: {time:?}")]
    DuplicateKeyTime { time: Time },
    #[error("key times must strictly increase")]
    UnorderedKeys,
    #[error("no key at time: {time:?}")]
    KeyNotFound { time: Time },
    #[error("interpolation meaning version {version} is unsupported")]
    UnsupportedInterpolationVersion { version: u32 },
}

/// Normalized time/value-progress handles between (0, 0) and (1, 1).
/// Ordered x handles guarantee monotonic time. Finite y handles may overshoot.
/// These are dimensionless progress values, never stored timestamps.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "BezierWire", into = "BezierWire")]
pub struct TimeBezier {
    control1: [FiniteF64; 2],
    control2: [FiniteF64; 2],
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BezierWire {
    control1: [FiniteF64; 2],
    control2: [FiniteF64; 2],
}

impl TimeBezier {
    pub fn new(control1: [f64; 2], control2: [f64; 2]) -> Result<Self, CurveError> {
        let control1 = [FiniteF64::new(control1[0])?, FiniteF64::new(control1[1])?];
        let control2 = [FiniteF64::new(control2[0])?, FiniteF64::new(control2[1])?];
        let (x1, x2) = (control1[0].get(), control2[0].get());
        if !(0.0 <= x1 && x1 <= x2 && x2 <= 1.0) {
            return Err(CurveError::NonMonotonicHandles);
        }
        Ok(Self { control1, control2 })
    }
    pub fn control1(self) -> [f64; 2] {
        self.control1.map(FiniteF64::get)
    }
    pub fn control2(self) -> [f64; 2] {
        self.control2.map(FiniteF64::get)
    }
}
impl TryFrom<BezierWire> for TimeBezier {
    type Error = CurveError;
    fn try_from(w: BezierWire) -> Result<Self, Self::Error> {
        Self::new(
            w.control1.map(FiniteF64::get),
            w.control2.map(FiniteF64::get),
        )
    }
}
impl From<TimeBezier> for BezierWire {
    fn from(b: TimeBezier) -> Self {
        Self {
            control1: b.control1,
            control2: b.control2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum CurveInterpolation {
    Hold,
    Linear,
    Cubic(TimeBezier),
}
impl CurveInterpolation {
    pub const fn mode(self) -> InterpolationMode {
        match self {
            Self::Hold => InterpolationMode::Hold,
            Self::Linear => InterpolationMode::Linear,
            Self::Cubic(_) => InterpolationMode::Cubic,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keyframe {
    pub time: Time,
    pub value: Value,
    /// Applies to [this key, next key). Validated even on the last key.
    pub interpolation: CurveInterpolation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurveDefinition {
    pub id: CurveId,
    pub value_type: ValueType,
    pub keys: Vec<Keyframe>,
    pub interpolation_version: u32,
}

/// Validated, strictly time-ordered document data; evaluation lives in
/// kronello-animation. Unknown meaning versions can be retained and serialized,
/// but cannot be edited or evaluated as version 1. Empty curves are editable;
/// evaluating one is an explicit error, never an implicit default value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "CurveDefinition", into = "CurveDefinition")]
pub struct AnimationCurve(CurveDefinition);

impl AnimationCurve {
    pub fn new(
        id: CurveId,
        value_type: ValueType,
        keys: Vec<Keyframe>,
    ) -> Result<Self, CurveError> {
        Self::try_from(CurveDefinition {
            id,
            value_type,
            keys,
            interpolation_version: INTERPOLATION_VERSION,
        })
    }
    pub fn id(&self) -> CurveId {
        self.0.id
    }
    pub fn value_type(&self) -> ValueType {
        self.0.value_type
    }
    pub fn keys(&self) -> &[Keyframe] {
        &self.0.keys
    }
    pub fn interpolation_version(&self) -> u32 {
        self.0.interpolation_version
    }
    pub fn ensure_supported_version(&self) -> Result<(), CurveError> {
        if self.0.interpolation_version != INTERPOLATION_VERSION {
            return Err(CurveError::UnsupportedInterpolationVersion {
                version: self.0.interpolation_version,
            });
        }
        Ok(())
    }
    fn validate_key(&self, key: &Keyframe) -> Result<(), CurveError> {
        let actual = key.value.value_type();
        if actual != self.0.value_type {
            return Err(ModelError::ValueTypeMismatch {
                expected: self.0.value_type,
                actual,
            }
            .into());
        }
        self.0
            .value_type
            .validate_interpolation(key.interpolation.mode())?;
        Ok(())
    }
    /// Rejects duplicate normalized rational times without changing the curve.
    pub fn insert_key(&mut self, key: Keyframe) -> Result<(), CurveError> {
        self.ensure_supported_version()?;
        self.validate_key(&key)?;
        match self.0.keys.binary_search_by_key(&key.time, |k| k.time) {
            Ok(_) => Err(CurveError::DuplicateKeyTime { time: key.time }),
            Err(index) => {
                self.0.keys.insert(index, key);
                Ok(())
            }
        }
    }
    /// Inserts or replaces deliberately; returns the replaced key, if any.
    pub fn upsert_key(&mut self, key: Keyframe) -> Result<Option<Keyframe>, CurveError> {
        self.ensure_supported_version()?;
        self.validate_key(&key)?;
        match self.0.keys.binary_search_by_key(&key.time, |k| k.time) {
            Ok(index) => Ok(Some(std::mem::replace(&mut self.0.keys[index], key))),
            Err(index) => {
                self.0.keys.insert(index, key);
                Ok(None)
            }
        }
    }
    /// Replaces only an existing key at exactly the supplied rational time.
    pub fn replace_key(&mut self, key: Keyframe) -> Result<Keyframe, CurveError> {
        self.ensure_supported_version()?;
        self.validate_key(&key)?;
        let index = self
            .0
            .keys
            .binary_search_by_key(&key.time, |k| k.time)
            .map_err(|_| CurveError::KeyNotFound { time: key.time })?;
        Ok(std::mem::replace(&mut self.0.keys[index], key))
    }
}
impl TryFrom<CurveDefinition> for AnimationCurve {
    type Error = CurveError;
    fn try_from(definition: CurveDefinition) -> Result<Self, Self::Error> {
        let curve = Self(definition);
        for key in &curve.0.keys {
            curve.validate_key(key)?;
        }
        for pair in curve.0.keys.windows(2) {
            if pair[0].time == pair[1].time {
                return Err(CurveError::DuplicateKeyTime { time: pair[0].time });
            }
            if pair[0].time > pair[1].time {
                return Err(CurveError::UnorderedKeys);
            }
        }
        Ok(curve)
    }
}
impl From<AnimationCurve> for CurveDefinition {
    fn from(curve: AnimationCurve) -> Self {
        curve.0
    }
}
