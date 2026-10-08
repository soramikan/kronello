//! Versioned audio channel layouts (ADR-0124).
//!
//! [`ChannelMask`] is a u64 bitset whose bit values follow the FFmpeg/SMPTE
//! speaker order (`AV_CHAN_*`) so the audited media boundary needs no
//! remapping. Only the closed set of standard layouts is accepted — mono,
//! stereo, 5.1 (side), 5.1 (rear/back) and 7.1 — per ADR-0124's explicit
//! layout contract; arbitrary speaker positions are rejected.
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// Layout descriptor version carried by [`ChannelMask`] values (ADR-0124).
pub const CHANNEL_MASK_VERSION: u32 = 1;

/// `UNSUPPORTED_CHANNEL_LAYOUT` rejection for masks outside the closed set or
/// for layouts whose channel count does not match the mask population.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("UNSUPPORTED_CHANNEL_LAYOUT: {detail}")]
pub struct ChannelLayoutError {
    detail: String,
}
impl ChannelLayoutError {
    fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

/// Versioned speaker-layout bitset (ADR-0124).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChannelMask(u64);
impl ChannelMask {
    /// Front left speaker bit (`AV_CHAN_FRONT_LEFT`).
    pub const FRONT_LEFT: u64 = 1 << 0;
    /// Front right speaker bit (`AV_CHAN_FRONT_RIGHT`).
    pub const FRONT_RIGHT: u64 = 1 << 1;
    /// Front center speaker bit (`AV_CHAN_FRONT_CENTER`).
    pub const FRONT_CENTER: u64 = 1 << 2;
    /// Low-frequency effects channel bit (`AV_CHAN_LOW_FREQUENCY`).
    pub const LOW_FREQUENCY: u64 = 1 << 3;
    /// Rear/back left speaker bit (`AV_CHAN_BACK_LEFT`).
    pub const BACK_LEFT: u64 = 1 << 4;
    /// Rear/back right speaker bit (`AV_CHAN_BACK_RIGHT`).
    pub const BACK_RIGHT: u64 = 1 << 5;
    /// Side left speaker bit (`AV_CHAN_SIDE_LEFT`).
    pub const SIDE_LEFT: u64 = 1 << 9;
    /// Side right speaker bit (`AV_CHAN_SIDE_RIGHT`).
    pub const SIDE_RIGHT: u64 = 1 << 10;

    /// Single front-center channel.
    pub const MONO: Self = Self(Self::FRONT_CENTER);
    /// Front left + front right.
    pub const STEREO: Self = Self(Self::FRONT_LEFT | Self::FRONT_RIGHT);
    /// 5.1 with side surrounds: FL, FR, C, LFE, SL, SR.
    pub const SURROUND_5_1: Self = Self(
        Self::FRONT_LEFT
            | Self::FRONT_RIGHT
            | Self::FRONT_CENTER
            | Self::LOW_FREQUENCY
            | Self::SIDE_LEFT
            | Self::SIDE_RIGHT,
    );
    /// 5.1 with rear/back surrounds: FL, FR, C, LFE, BL, BR.
    pub const SURROUND_5_1_BACK: Self = Self(
        Self::FRONT_LEFT
            | Self::FRONT_RIGHT
            | Self::FRONT_CENTER
            | Self::LOW_FREQUENCY
            | Self::BACK_LEFT
            | Self::BACK_RIGHT,
    );
    /// 7.1: FL, FR, C, LFE, BL, BR, SL, SR.
    pub const SURROUND_7_1: Self = Self(
        Self::FRONT_LEFT
            | Self::FRONT_RIGHT
            | Self::FRONT_CENTER
            | Self::LOW_FREQUENCY
            | Self::BACK_LEFT
            | Self::BACK_RIGHT
            | Self::SIDE_LEFT
            | Self::SIDE_RIGHT,
    );

    /// Every mask in the closed ADR-0124 set.
    pub const SUPPORTED: &'static [Self] = &[
        Self::MONO,
        Self::STEREO,
        Self::SURROUND_5_1,
        Self::SURROUND_5_1_BACK,
        Self::SURROUND_7_1,
    ];

    /// Accepts only the closed ADR-0124 set; anything else is a typed
    /// `UNSUPPORTED_CHANNEL_LAYOUT` rejection rather than an implicit layout.
    pub fn from_bits(bits: u64) -> Result<Self, ChannelLayoutError> {
        Self::SUPPORTED
            .iter()
            .copied()
            .find(|mask| mask.0 == bits)
            .ok_or_else(|| ChannelLayoutError::new(format!("unsupported channel mask 0x{bits:x}")))
    }
    /// Raw speaker bits, always in FFmpeg/SMPTE order.
    pub fn bits(self) -> u64 {
        self.0
    }
    /// Number of channels carried by this layout.
    pub fn channels(self) -> usize {
        self.0.count_ones() as usize
    }
    /// True when `index` (native channel order) addresses the LFE channel.
    /// Compressor/limiter paths exclude only this channel from detection and
    /// application; every other effect applies to every channel.
    pub fn is_lfe(self, index: usize) -> bool {
        self.channel_bit(index) == Some(Self::LOW_FREQUENCY)
    }
    /// Speaker bit for `index` in native channel order.
    pub fn channel_bit(self, index: usize) -> Option<u64> {
        self.channel_bits().get(index).copied()
    }
    /// Speaker bits in ascending bit order, which equals FFmpeg's native
    /// channel order for every supported layout.
    pub fn channel_bits(self) -> Vec<u64> {
        let mut bits = Vec::with_capacity(self.channels());
        for bit_index in 0..64 {
            let bit = 1u64 << bit_index;
            if self.0 & bit != 0 {
                bits.push(bit);
            }
        }
        bits
    }
    /// Human-readable layout name for diagnostics and reports.
    pub fn name(self) -> &'static str {
        match self {
            Self::MONO => "mono",
            Self::STEREO => "stereo",
            Self::SURROUND_5_1 => "5.1",
            Self::SURROUND_5_1_BACK => "5.1(back)",
            Self::SURROUND_7_1 => "7.1",
            _ => "unknown",
        }
    }
    /// `UNSUPPORTED_CHANNEL_LAYOUT` when `count` does not equal this mask's
    /// population; used at container/codec boundaries where the two values can
    /// disagree.
    pub fn require_channels(self, count: usize) -> Result<Self, ChannelLayoutError> {
        if self.channels() != count {
            return Err(ChannelLayoutError::new(format!(
                "channel mask {} ({}) has {} speakers, not {count}",
                self.name(),
                self.0,
                self.channels()
            )));
        }
        Ok(self)
    }
}
impl From<ChannelMask> for u64 {
    fn from(mask: ChannelMask) -> u64 {
        mask.bits()
    }
}
impl TryFrom<u64> for ChannelMask {
    type Error = ChannelLayoutError;
    fn try_from(bits: u64) -> Result<Self, Self::Error> {
        Self::from_bits(bits)
    }
}
impl Serialize for ChannelMask {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(self.0)
    }
}
impl<'de> Deserialize<'de> for ChannelMask {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let bits = u64::deserialize(deserializer)?;
        Self::from_bits(bits).map_err(serde::de::Error::custom)
    }
}
impl schemars::JsonSchema for ChannelMask {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ChannelMask".into()
    }
    fn schema_id() -> std::borrow::Cow<'static, str> {
        concat!(module_path!(), "::ChannelMask").into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "integer",
            "format": "uint64",
            "minimum": 0,
            "description": "Versioned speaker-layout bitset (channel_mask v1): 1=mono, 3=stereo, 1551=5.1(side), 63=5.1(back), 1599=7.1"
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closed_set_rejects_unknown_bits() {
        assert_eq!(ChannelMask::from_bits(3), Ok(ChannelMask::STEREO));
        assert_eq!(ChannelMask::from_bits(0x60f), Ok(ChannelMask::SURROUND_5_1));
        assert_eq!(
            ChannelMask::from_bits(0x3f),
            Ok(ChannelMask::SURROUND_5_1_BACK)
        );
        assert_eq!(ChannelMask::from_bits(0x63f), Ok(ChannelMask::SURROUND_7_1));
        assert_eq!(ChannelMask::from_bits(4), Ok(ChannelMask::MONO));
        for bits in [0u64, 2, 5, 0x60e, 0xffff_ffff_ffff_ffff] {
            let error = ChannelMask::from_bits(bits).unwrap_err();
            assert!(error.to_string().contains("UNSUPPORTED_CHANNEL_LAYOUT"));
        }
    }
    #[test]
    fn channel_order_matches_native_bit_order() {
        let bits = ChannelMask::SURROUND_5_1.channel_bits();
        assert_eq!(
            bits,
            vec![
                ChannelMask::FRONT_LEFT,
                ChannelMask::FRONT_RIGHT,
                ChannelMask::FRONT_CENTER,
                ChannelMask::LOW_FREQUENCY,
                ChannelMask::SIDE_LEFT,
                ChannelMask::SIDE_RIGHT
            ]
        );
        assert!(ChannelMask::SURROUND_5_1.is_lfe(3));
        assert!(!ChannelMask::SURROUND_5_1.is_lfe(2));
        assert!(!ChannelMask::STEREO.is_lfe(0));
    }
    #[test]
    fn serde_roundtrips_bits() {
        let mask = ChannelMask::SURROUND_5_1;
        let text = serde_json::to_string(&mask).expect("serialize");
        assert_eq!(text, "1551");
        assert_eq!(serde_json::from_str::<ChannelMask>("1551").unwrap(), mask);
        assert!(serde_json::from_str::<ChannelMask>("7").is_err());
    }
}
