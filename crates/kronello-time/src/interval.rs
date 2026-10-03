use serde::{Deserialize, Serialize};

use crate::{Rational, Time, TimeError};

/// A nonnegative length in seconds. Zero is permitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "Rational", into = "Rational")]
pub struct Duration(Time);

impl Duration {
    pub const ZERO: Self = Self(Time::ZERO);

    pub fn new(seconds: Time) -> Result<Self, TimeError> {
        if seconds < Time::ZERO {
            Err(TimeError::NegativeDuration)
        } else {
            Ok(Self(seconds))
        }
    }

    pub const fn as_time(self) -> Time {
        self.0
    }

    pub fn checked_add(self, other: Self) -> Result<Self, TimeError> {
        Self::new(self.0.checked_add(other.0)?)
    }

    pub fn checked_sub(self, other: Self) -> Result<Self, TimeError> {
        Self::new(self.0.checked_sub(other.0)?)
    }
}

impl TryFrom<Rational> for Duration {
    type Error = TimeError;

    fn try_from(value: Rational) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<Duration> for Rational {
    fn from(value: Duration) -> Self {
        value.0
    }
}

/// A half-open interval [start, end). Equal endpoints describe an empty range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "RangeWire", into = "RangeWire")]
pub struct TimeRange {
    start: Time,
    end: Time,
}

impl TimeRange {
    pub fn new(start: Time, end: Time) -> Result<Self, TimeError> {
        if end < start {
            Err(TimeError::ReversedRange)
        } else {
            Ok(Self { start, end })
        }
    }

    pub fn from_start_duration(start: Time, duration: Duration) -> Result<Self, TimeError> {
        Self::new(start, start.checked_add(duration.as_time())?)
    }

    pub const fn start(self) -> Time {
        self.start
    }

    pub const fn end(self) -> Time {
        self.end
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }

    pub fn contains(self, time: Time) -> bool {
        self.start <= time && time < self.end
    }

    pub fn duration(self) -> Result<Duration, TimeError> {
        Duration::new(self.end.checked_sub(self.start)?)
    }

    /// Empty and merely touching intersections return None.
    pub fn intersection(self, other: Self) -> Option<Self> {
        let start = self.start.max(other.start);
        let end = self.end.min(other.end);
        (start < end).then_some(Self { start, end })
    }

    /// True only for nonempty intervals that meet at one endpoint.
    pub fn is_adjacent_to(self, other: Self) -> bool {
        !self.is_empty()
            && !other.is_empty()
            && (self.end == other.start || other.end == self.start)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RangeWire {
    start: Time,
    end: Time,
}

impl TryFrom<RangeWire> for TimeRange {
    type Error = TimeError;

    fn try_from(value: RangeWire) -> Result<Self, Self::Error> {
        Self::new(value.start, value.end)
    }
}

impl From<TimeRange> for RangeWire {
    fn from(value: TimeRange) -> Self {
        Self {
            start: value.start,
            end: value.end,
        }
    }
}
