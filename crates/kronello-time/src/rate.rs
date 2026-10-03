use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::{Rational, Time, TimeError, TimeRange};

/// A positive exact number of frames per second.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(try_from = "Rational", into = "Rational")]
pub struct FrameRate(Rational);

impl FrameRate {
    pub fn new(num: i64, den: i64) -> Result<Self, TimeError> {
        Self::try_from(Rational::new(num, den)?)
    }

    pub const fn as_rational(self) -> Rational {
        self.0
    }

    /// Exact timestamp of an integer frame boundary (frames may be negative).
    pub fn frame_to_time(self, frame: i64) -> Result<Time, TimeError> {
        Rational::from_wide(
            i128::from(frame)
                .checked_mul(i128::from(self.0.denominator()))
                .ok_or(TimeError::Overflow)?,
            i128::from(self.0.numerator()),
        )
    }

    /// Exact fractional frame coordinate; this never quantizes evaluation time.
    pub fn time_to_frame(self, time: Time) -> Result<Rational, TimeError> {
        time.checked_mul(self.0)
    }

    /// Mathematical floor of the fractional frame coordinate.
    pub fn frame_floor(self, time: Time) -> Result<i64, TimeError> {
        let num = i128::from(time.numerator())
            .checked_mul(i128::from(self.0.numerator()))
            .ok_or(TimeError::Overflow)?;
        let den = i128::from(time.denominator())
            .checked_mul(i128::from(self.0.denominator()))
            .ok_or(TimeError::Overflow)?;
        i64::try_from(num.div_euclid(den)).map_err(|_| TimeError::Overflow)
    }

    pub fn frame_range(self, frame: i64) -> Result<TimeRange, TimeError> {
        let next = frame.checked_add(1).ok_or(TimeError::Overflow)?;
        TimeRange::new(self.frame_to_time(frame)?, self.frame_to_time(next)?)
    }
}

impl TryFrom<Rational> for FrameRate {
    type Error = TimeError;

    fn try_from(value: Rational) -> Result<Self, Self::Error> {
        if value <= Rational::ZERO {
            Err(TimeError::InvalidRate)
        } else {
            Ok(Self(value))
        }
    }
}

impl From<FrameRate> for Rational {
    fn from(value: FrameRate) -> Self {
        value.0
    }
}

/// Positive integer samples per second. Indices share the time-zero origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(try_from = "u32", into = "u32")]
pub struct SampleRate(u32);

impl SampleRate {
    pub const HZ_48000: Self = Self(48_000);

    pub fn new(hz: u32) -> Result<Self, TimeError> {
        if hz == 0 {
            Err(TimeError::InvalidRate)
        } else {
            Ok(Self(hz))
        }
    }

    pub const fn hz(self) -> u32 {
        self.0
    }

    pub fn sample_to_time(self, sample: i64) -> Result<Time, TimeError> {
        Time::new(sample, i64::from(self.0))
    }

    pub fn sample_floor(self, time: Time) -> Result<i64, TimeError> {
        let num = i128::from(time.numerator())
            .checked_mul(i128::from(self.0))
            .ok_or(TimeError::Overflow)?;
        i64::try_from(num.div_euclid(i128::from(time.denominator())))
            .map_err(|_| TimeError::Overflow)
    }

    /// Audio batch boundaries are floor(start * hz)..floor(end * hz).
    ///
    /// The same absolute boundary always yields the same index. Batches for
    /// adjacent frames therefore have no gaps or overlaps, without accumulating
    /// rounded per-frame lengths. This is a batching convention, not a test that
    /// every sample's timestamp lies within the unrounded time range.
    pub fn samples_for_range(self, range: TimeRange) -> Result<Range<i64>, TimeError> {
        Ok(self.sample_floor(range.start())?..self.sample_floor(range.end())?)
    }

    pub fn samples_for_frame(self, rate: FrameRate, frame: i64) -> Result<Range<i64>, TimeError> {
        self.samples_for_range(rate.frame_range(frame)?)
    }
}

impl TryFrom<u32> for SampleRate {
    type Error = TimeError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<SampleRate> for u32 {
    fn from(value: SampleRate) -> Self {
        value.0
    }
}
