//! Immutable, versioned offline audio features; lookup never decodes audio.
use crate::{AssetId, ProjectError};
use kronello_time::{Rational, Time, TimeMap};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const AUDIO_ANALYSIS_VERSION: u32 = 1;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AudioAnalysisConfig {
    pub version: u32,
    pub sample_rate: u32,
    pub window: u32,
    pub hop: u32,
    /// Inclusive low / exclusive high frequencies, in Hz.
    pub bands: Vec<[u32; 2]>,
    /// Consumer-local time maps to the absolute analysed sample grid.
    pub time_map: TimeMap,
}
impl AudioAnalysisConfig {
    pub fn validate(&self) -> Result<(), ProjectError> {
        if self.version != AUDIO_ANALYSIS_VERSION
            || self.sample_rate != 48000
            || !(32..=8192).contains(&self.window)
            || !self.window.is_power_of_two()
            || self.hop == 0
            || self.hop > self.window
            || self.bands.len() > 32
            || self
                .bands
                .iter()
                .any(|[a, b]| a >= b || *b > self.sample_rate / 2)
        {
            return Err(ProjectError::InvalidDocument(
                "invalid audio analysis config".into(),
            ));
        }
        Ok(())
    }
    /// Both channel FFTs and all band/bin membership operations, charged before allocation.
    pub fn work_for_samples(&self, samples: usize) -> Option<usize> {
        let hop = usize::try_from(self.hop).ok().filter(|h| *h > 0)?;
        if self.window == 0 {
            return None;
        }
        let windows = samples.div_ceil(hop);
        let n = usize::try_from(self.window).ok()?;
        let per_window = n.checked_mul(2)?.checked_mul(
            (self.window.ilog2() as usize)
                .checked_add(self.bands.len())?
                .checked_add(1)?,
        )?;
        windows.checked_mul(per_window)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AudioAnalysisSource {
    Asset {
        asset: AssetId,
        stream_index: u32,
        content_hash: String,
    },
    /// Canonical fixed Project snapshot hash, target id and evaluator version.
    Bus {
        snapshot_hash: String,
        target: String,
        evaluator_version: u32,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AudioAnalysisFrame {
    pub time: Time,
    pub rms: f64,
    pub band_energy: Vec<f64>,
    /// Positive RMS difference from the preceding analysis window.
    pub onset: f64,
    /// Thresholded onset with a fixed 100 ms refractory interval, not tempo inference.
    pub beat: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AudioAnalysisDataAsset {
    pub id: AssetId,
    pub source: AudioAnalysisSource,
    pub config: AudioAnalysisConfig,
    pub start_sample: i64,
    pub sample_count: u32,
    pub frames: Vec<AudioAnalysisFrame>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AudioFeature {
    Rms,
    BandEnergy { band: u32 },
    Onset,
    Beat,
}
impl AudioAnalysisDataAsset {
    pub fn validate(&self) -> Result<(), ProjectError> {
        self.config.validate()?;
        let hash = match &self.source {
            AudioAnalysisSource::Asset { content_hash, .. } => content_hash,
            AudioAnalysisSource::Bus {
                snapshot_hash,
                target,
                evaluator_version,
            } => {
                if target.is_empty() || *evaluator_version != 2 {
                    return Err(ProjectError::InvalidDocument(
                        "invalid analysis bus identity".into(),
                    ));
                }
                snapshot_hash
            }
        };
        let count = u64::from(self.sample_count).div_ceil(u64::from(self.config.hop));
        if hash.len() != 64
            || !hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || self.start_sample < 0
            || count > 65536
            || self
                .config
                .work_for_samples(self.sample_count as usize)
                .is_none_or(|w| w > 100_000_000)
            || self.sample_count > 28_800_000
            || self.frames.len() as u64 != count
        {
            return Err(ProjectError::InvalidDocument(
                "invalid audio analysis identity or size".into(),
            ));
        }
        for (i, frame) in self.frames.iter().enumerate() {
            let sample = self
                .start_sample
                .checked_add(i as i64 * i64::from(self.config.hop))
                .ok_or_else(|| ProjectError::InvalidDocument("analysis time overflow".into()))?;
            let time = Time::new(sample, i64::from(self.config.sample_rate))
                .map_err(|e| ProjectError::InvalidDocument(e.to_string()))?;
            if frame.time != time
                || frame.band_energy.len() != self.config.bands.len()
                || [frame.rms, frame.onset]
                    .into_iter()
                    .chain(frame.band_energy.iter().copied())
                    .any(|v| !v.is_finite() || v < 0.0)
            {
                return Err(ProjectError::InvalidDocument(
                    "invalid audio analysis frame".into(),
                ));
            }
        }
        Ok(())
    }
    /// Zero-order hold inside the half-open analysed domain. Out-of-domain is an error.
    pub fn sample(&self, time: Time, feature: AudioFeature) -> Result<f64, ProjectError> {
        let time = self
            .config
            .time_map
            .map(time)
            .map_err(|e| ProjectError::InvalidDocument(e.to_string()))?;
        let offset = time
            .checked_mul(
                Rational::new(i64::from(self.config.sample_rate), 1)
                    .map_err(|e| ProjectError::InvalidDocument(e.to_string()))?,
            )
            .and_then(|v| v.checked_sub(Rational::new(self.start_sample, 1)?))
            .map_err(|e| ProjectError::InvalidDocument(e.to_string()))?;
        if offset < Rational::ZERO
            || offset
                >= Rational::new(i64::from(self.sample_count), 1)
                    .map_err(|e| ProjectError::InvalidDocument(e.to_string()))?
        {
            return Err(ProjectError::InvalidDocument(
                "audio feature outside analysed domain".into(),
            ));
        }
        let index = usize::try_from(offset.floor() / i64::from(self.config.hop))
            .map_err(|_| ProjectError::InvalidDocument("analysis index overflow".into()))?;
        let f = self
            .frames
            .get(index)
            .ok_or_else(|| ProjectError::InvalidDocument("missing audio feature frame".into()))?;
        match feature {
            AudioFeature::Rms => Ok(f.rms),
            AudioFeature::Onset => Ok(f.onset),
            AudioFeature::Beat => Ok(if f.beat { 1.0 } else { 0.0 }),
            AudioFeature::BandEnergy { band } => {
                f.band_energy.get(band as usize).copied().ok_or_else(|| {
                    ProjectError::InvalidDocument("missing audio feature band".into())
                })
            }
        }
    }
}

impl crate::Project {
    /// Resolve immutable analysis inputs and reject stale direct-media provenance.
    pub fn audio_analysis_inputs(&self) -> Result<Vec<AudioAnalysisDataAsset>, ProjectError> {
        let mut result = Vec::new();
        for data in &self.audio_analyses {
            let crate::DocumentObject::Known(data) = data else {
                return Err(ProjectError::UnsupportedMeaning);
            };
            data.validate()?;
            if let AudioAnalysisSource::Asset {
                asset,
                content_hash,
                ..
            } = &data.source
                && !self.assets.iter().any(|a| matches!(a, crate::DocumentObject::Known(a) if a.id == *asset && a.content_hash == *content_hash))
            { return Err(ProjectError::InvalidDocument("stale audio analysis source".into())); }
            result.push(data.clone());
        }
        Ok(result)
    }
}
