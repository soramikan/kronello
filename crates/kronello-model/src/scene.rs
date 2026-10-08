//! Deterministic scene-boundary detection results (ADR-0125) and smart-reframe
//! rule settings (ADR-0126). Stored data is a versioned document object.
use crate::{AssetId, FiniteF64, ProjectError};
use kronello_time::{Duration, Time, TimeRange};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SCENE_BOUNDARY_VERSION: u32 = 1;
/// Worker-side decoded frame bound for one `scene.detect` job.
pub const SCENE_DETECT_MAX_FRAMES: u32 = 8192;
/// Per-frame pixel bound shared by every detection backend.
pub const SCENE_DETECT_MAX_PIXELS: u32 = 16_777_216;
pub const SMART_REFRAME_VERSION: u32 = 1;

fn invalid(reason: &str) -> ProjectError {
    ProjectError::InvalidDocument(reason.into())
}

fn hash_text(value: &str) -> Result<(), ProjectError> {
    if value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        Ok(())
    } else {
        Err(invalid("invalid content hash"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SceneSource {
    pub asset: AssetId,
    pub stream_index: u32,
    /// Locked `Asset::content_hash`; mismatches mark the result stale.
    #[schemars(regex(pattern = "^[0-9a-f]{64}$"))]
    pub content_hash: String,
}

/// Versioned detection parameters baked into the boundary payload hash.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SceneDetectionParams {
    /// Edge-change weight mixed into the histogram difference, in `[0, 1]`.
    pub edge_weight: FiniteF64,
    /// Adaptive threshold offset: `mean + threshold_sigma * stddev`.
    pub threshold_sigma: FiniteF64,
    /// A peak must dominate `peak_ratio * mean` when it misses the sigma band.
    pub peak_ratio: FiniteF64,
    /// Minimum spacing between emitted boundaries, in sampled frames.
    pub min_spacing_frames: u32,
}
impl Default for SceneDetectionParams {
    fn default() -> Self {
        Self {
            edge_weight: FiniteF64::new(0.5).expect("finite default"),
            threshold_sigma: FiniteF64::new(3.0).expect("finite default"),
            peak_ratio: FiniteF64::new(4.0).expect("finite default"),
            min_spacing_frames: 2,
        }
    }
}
impl SceneDetectionParams {
    pub fn validate(&self) -> Result<(), ProjectError> {
        if !(0.0..=1.0).contains(&self.edge_weight.get())
            || self.threshold_sigma.get() <= 0.0
            || self.peak_ratio.get() < 1.0
            || self.min_spacing_frames == 0
            || self.min_spacing_frames > 1024
        {
            return Err(invalid("invalid scene detection parameters"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SceneBoundary {
    /// Rational source presentation time of the first frame of the new scene.
    pub time: Time,
    /// Normalized detector confidence in `[0, 1]`.
    pub confidence: FiniteF64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SceneBoundaryAsset {
    pub id: AssetId,
    pub version: u32,
    pub source: SceneSource,
    /// Parameters that produced `boundaries`; part of the versioned payload.
    pub params: SceneDetectionParams,
    /// Analyzed source-time interval `[start, end)`.
    pub range: TimeRange,
    /// Decoded frames that produced `boundaries` (provenance, not a lookup).
    pub frames_analyzed: u32,
    /// Cut boundaries in strictly increasing source time.
    pub boundaries: Vec<SceneBoundary>,
    /// SHA-256 over the versioned payload; recomputed on validation.
    #[schemars(regex(pattern = "^[0-9a-f]{64}$"))]
    pub content_hash: String,
}
impl SceneBoundaryAsset {
    /// SHA-256 over every versioned identity field, in serialized form.
    pub fn computed_hash(&self) -> Result<String, ProjectError> {
        #[derive(Serialize)]
        struct Payload<'a> {
            version: u32,
            source: &'a SceneSource,
            params: &'a SceneDetectionParams,
            range: &'a TimeRange,
            frames_analyzed: u32,
            boundaries: &'a [SceneBoundary],
        }
        let bytes = serde_json::to_vec(&Payload {
            version: self.version,
            source: &self.source,
            params: &self.params,
            range: &self.range,
            frames_analyzed: self.frames_analyzed,
            boundaries: &self.boundaries,
        })
        .map_err(|e| invalid(&e.to_string()))?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
    /// Structural and identity validation; no filesystem access.
    pub fn validate(&self) -> Result<(), ProjectError> {
        if self.version != SCENE_BOUNDARY_VERSION {
            return Err(ProjectError::UnsupportedMeaning);
        }
        hash_text(&self.source.content_hash)?;
        hash_text(&self.content_hash)?;
        self.params.validate()?;
        if self.range.is_empty()
            || self.frames_analyzed == 0
            || self.frames_analyzed > SCENE_DETECT_MAX_FRAMES
            || self.boundaries.len() >= self.frames_analyzed as usize
        {
            return Err(invalid("invalid scene boundary asset bounds"));
        }
        let mut previous = None;
        for boundary in &self.boundaries {
            if boundary.time <= self.range.start()
                || boundary.time >= self.range.end()
                || previous.is_some_and(|t| boundary.time <= t)
                || !(0.0..=1.0).contains(&boundary.confidence.get())
            {
                return Err(invalid("invalid scene boundary"));
            }
            previous = Some(boundary.time);
        }
        if self.content_hash != self.computed_hash()? {
            return Err(invalid("scene boundary asset hash mismatch"));
        }
        Ok(())
    }
}
impl crate::Project {
    /// Validated boundary assets with live sources, in document order.
    pub fn scene_boundary_inputs(&self) -> Result<Vec<SceneBoundaryAsset>, ProjectError> {
        let mut result = Vec::new();
        for data in &self.scene_boundary_assets {
            let crate::DocumentObject::Known(data) = data else {
                return Err(ProjectError::UnsupportedMeaning);
            };
            data.validate()?;
            if !self.assets.iter().any(|a| {
                matches!(a, crate::DocumentObject::Known(a)
                    if a.id == data.source.asset && a.content_hash == data.source.content_hash)
            }) {
                return Err(invalid("stale scene boundary source"));
            }
            result.push(data.clone());
        }
        Ok(result)
    }
    /// One validated boundary asset by id with a live-source check.
    pub fn scene_boundary_asset(&self, id: AssetId) -> Result<SceneBoundaryAsset, ProjectError> {
        let data = self
            .scene_boundary_assets
            .iter()
            .find_map(|a| match a {
                crate::DocumentObject::Known(a) if a.id == id => Some(a),
                _ => None,
            })
            .ok_or_else(|| invalid("scene boundary asset missing"))?;
        data.validate()?;
        if !self.assets.iter().any(|a| {
            matches!(a, crate::DocumentObject::Known(a)
                if a.id == data.source.asset && a.content_hash == data.source.content_hash)
        }) {
            return Err(invalid("stale scene boundary source"));
        }
        Ok(data.clone())
    }
}

/// Temporal smoothing kernel shape (ADR-0126). The window weight is
/// `1 - ease(u)` on the normalized offset `u = |dt| / smoothing_window`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReframeEasing {
    /// Tent kernel `1 - u`.
    #[default]
    Linear,
    /// `1 - u^2`: flat near center, sharp window edge.
    EaseIn,
    /// `(1 - u)^2`: sharp near center, flat window edge.
    EaseOut,
    /// Smoothstep complement `1 - (3u^2 - 2u^3)`.
    EaseInOut,
}
/// Versioned smart-reframe settings (ADR-0126): seed selection, temporal
/// smoothing window, padding, zoom cap and easing are all hash-covered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SmartReframeSettings {
    pub version: u32,
    /// Selected `TrackingDataAsset::seeds` indexes feeding the gaze centroid;
    /// empty selects every seed in stable order.
    pub seeds: Vec<u32>,
    /// Symmetric temporal smoothing radius in source seconds; zero samples
    /// only frames exactly at the source time.
    pub smoothing_window: Duration,
    /// Gaze margin fraction in `[0, 1]`: `0` is the tightest allowed crop,
    /// `1` the largest target-aspect window fitting the source.
    pub padding: FiniteF64,
    /// `>= 1`: the crop never shrinks below `1 / max_zoom` of the fitted
    /// window (the tightest allowed crop scale).
    pub max_zoom: FiniteF64,
    /// Target crop aspect ratio `width / height` in source pixels.
    pub target_aspect: FiniteF64,
    pub easing: ReframeEasing,
}
impl SmartReframeSettings {
    pub fn validate(&self) -> Result<(), ProjectError> {
        if self.version != SMART_REFRAME_VERSION {
            return Err(ProjectError::UnsupportedMeaning);
        }
        if self.smoothing_window.as_time() < Time::ZERO
            || !(0.0..=1.0).contains(&self.padding.get())
            || self.max_zoom.get() < 1.0
            || self.target_aspect.get() <= 0.0
            || self.seeds.len() > crate::TRACKING_MAX_SEEDS
            || self
                .seeds
                .iter()
                .any(|s| *s as usize >= crate::TRACKING_MAX_SEEDS)
            || self
                .seeds
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != self.seeds.len()
        {
            return Err(invalid("invalid smart reframe settings"));
        }
        Ok(())
    }
}
/// AI-003 (ADR-0126): a media node inside a template composition reframed
/// from the confidence-weighted gaze of a `TrackingDataAsset`. The window is
/// injected as a layout-derived Vec2 pair through the shared dependency path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SmartReframeRule {
    /// Media (`Media` or `Null` slot) node inside the placement's composition.
    pub node: crate::NodeId,
    /// `TrackingDataAsset` supplying the gaze samples.
    pub tracking: AssetId,
    /// `kronello.media.crop_origin` property on the media node.
    pub crop_origin_property: crate::PropertyId,
    /// `kronello.media.crop_size` property on the media node.
    pub crop_size_property: crate::PropertyId,
    pub settings: SmartReframeSettings,
}
