//! Backend-independent exact rational time in seconds.
//!
//! All arithmetic that can overflow returns [`TimeError`]. No arithmetic operator
//! overloads or floating-point conversions are provided. Frame/sample rounding
//! uses mathematical floor, including before time zero.

mod error;
mod interval;
mod mapping;
mod rate;
mod rational;

pub use error::TimeError;
pub use interval::{Duration, TimeRange};
pub use mapping::{
    FlowFallbackPolicy, FrameInterpolation, LinearTimeMap, OpticalFlowConfig, PiecewiseTimeMap,
    ProtectedMiddleMode, ProtectedTimeMap, TimeMap, TimeMapPoint,
};
pub use rate::{FrameRate, SampleRate};
pub use rational::Rational;

/// A rational number interpreted as seconds, including negative timestamps.
pub type Time = Rational;
