use std::cmp::Ordering;

use serde::{Deserialize, Deserializer, Serialize, Serializer, ser::SerializeStruct};

use crate::TimeError;

/// A normalized rational: coprime i64 numerator/denominator, denominator > 0.
///
/// JSON components are decimal strings. Deserialization normalizes inputs, but
/// rejects numbers, non-decimal strings and out-of-i64 components.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rational {
    num: i64,
    den: i64,
}

impl Rational {
    pub const ZERO: Self = Self { num: 0, den: 1 };
    pub const ONE: Self = Self { num: 1, den: 1 };

    pub fn new(num: i64, den: i64) -> Result<Self, TimeError> {
        Self::from_wide(i128::from(num), i128::from(den))
    }

    pub const fn from_integer(value: i64) -> Self {
        Self { num: value, den: 1 }
    }

    pub const fn numerator(self) -> i64 {
        self.num
    }

    pub const fn denominator(self) -> i64 {
        self.den
    }

    pub fn checked_add(self, other: Self) -> Result<Self, TimeError> {
        let left = i128::from(self.num).checked_mul(i128::from(other.den));
        let right = i128::from(other.num).checked_mul(i128::from(self.den));
        Self::from_wide(
            left.and_then(|a| right.and_then(|b| a.checked_add(b)))
                .ok_or(TimeError::Overflow)?,
            i128::from(self.den)
                .checked_mul(i128::from(other.den))
                .ok_or(TimeError::Overflow)?,
        )
    }

    pub fn checked_sub(self, other: Self) -> Result<Self, TimeError> {
        let left = i128::from(self.num).checked_mul(i128::from(other.den));
        let right = i128::from(other.num).checked_mul(i128::from(self.den));
        Self::from_wide(
            left.and_then(|a| right.and_then(|b| a.checked_sub(b)))
                .ok_or(TimeError::Overflow)?,
            i128::from(self.den)
                .checked_mul(i128::from(other.den))
                .ok_or(TimeError::Overflow)?,
        )
    }

    pub fn checked_mul(self, other: Self) -> Result<Self, TimeError> {
        Self::from_wide(
            i128::from(self.num)
                .checked_mul(i128::from(other.num))
                .ok_or(TimeError::Overflow)?,
            i128::from(self.den)
                .checked_mul(i128::from(other.den))
                .ok_or(TimeError::Overflow)?,
        )
    }

    pub fn checked_div(self, other: Self) -> Result<Self, TimeError> {
        if other.num == 0 {
            return Err(TimeError::DivisionByZero);
        }
        Self::from_wide(
            i128::from(self.num)
                .checked_mul(i128::from(other.den))
                .ok_or(TimeError::Overflow)?,
            i128::from(self.den)
                .checked_mul(i128::from(other.num))
                .ok_or(TimeError::Overflow)?,
        )
    }

    pub fn checked_neg(self) -> Result<Self, TimeError> {
        Self::from_wide(
            i128::from(self.num)
                .checked_neg()
                .ok_or(TimeError::Overflow)?,
            i128::from(self.den),
        )
    }

    pub fn floor(self) -> i64 {
        // The denominator is positive; MIN / -1 is impossible.
        self.num.div_euclid(self.den)
    }

    pub(crate) fn from_wide(mut num: i128, mut den: i128) -> Result<Self, TimeError> {
        if den == 0 {
            return Err(TimeError::ZeroDenominator);
        }
        if num == 0 {
            return Ok(Self::ZERO);
        }
        if den < 0 {
            num = num.checked_neg().ok_or(TimeError::Overflow)?;
            den = den.checked_neg().ok_or(TimeError::Overflow)?;
        }
        let mut a = num.unsigned_abs();
        let mut b = den.unsigned_abs();
        while b != 0 {
            (a, b) = (b, a % b);
        }
        let divisor = i128::try_from(a).map_err(|_| TimeError::Overflow)?;
        Ok(Self {
            num: i64::try_from(num / divisor).map_err(|_| TimeError::Overflow)?,
            den: i64::try_from(den / divisor).map_err(|_| TimeError::Overflow)?,
        })
    }
}

impl Ord for Rational {
    fn cmp(&self, other: &Self) -> Ordering {
        // Each product of two i64 values is exactly representable by i128.
        (i128::from(self.num) * i128::from(other.den))
            .cmp(&(i128::from(other.num) * i128::from(self.den)))
    }
}

impl PartialOrd for Rational {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Serialize for Rational {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("Rational", 2)?;
        state.serialize_field("num", &self.num.to_string())?;
        state.serialize_field("den", &self.den.to_string())?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for Rational {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            num: String,
            den: String,
        }
        fn decimal<E: serde::de::Error>(value: &str) -> Result<i64, E> {
            let digits = value.strip_prefix('-').unwrap_or(value);
            if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
                return Err(E::custom("expected a signed decimal integer string"));
            }
            value.parse().map_err(E::custom)
        }
        let wire = Wire::deserialize(deserializer)?;
        Self::new(decimal(&wire.num)?, decimal(&wire.den)?).map_err(serde::de::Error::custom)
    }
}
