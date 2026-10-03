use thiserror::Error;

/// Failures in exact time construction, arithmetic, or evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum TimeError {
    #[error("a rational denominator must not be zero")]
    ZeroDenominator,
    #[error("division by zero")]
    DivisionByZero,
    #[error("exact time arithmetic overflow")]
    Overflow,
    #[error("duration must be nonnegative")]
    NegativeDuration,
    #[error("interval end precedes its start")]
    ReversedRange,
    #[error("frame and sample rates must be positive")]
    InvalidRate,
    #[error("piecewise mapping needs at least two points")]
    TooFewMapPoints,
    #[error("mapping parent timestamps must strictly increase")]
    UnorderedMapPoints,
    #[error("mapping must strictly increase; reverse playback and hold are unsupported")]
    UnsupportedMapSlope,
    #[error("timestamp is outside the piecewise mapping domain")]
    OutsideMapDomain,
}
