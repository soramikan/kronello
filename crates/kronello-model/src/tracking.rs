//! Deterministic offline motion-tracking results (ADR-0118). Stored data is a
//! versioned document object; expressions read it through the derived
//! [`DataTable`] view that backs `ExpressionNode::DataAssetCell` sampling.
use crate::{AssetId, DataTable, FiniteF64, ProjectError, Value, ValueType};
use kronello_time::{Rational, Time, TimeRange};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const TRACKING_VERSION: u32 = 1;
/// Synchronous analysis bound (ADR-0118); longer ranges remain a future job.
pub const TRACKING_MAX_FRAMES: u32 = 4000;
/// Point-mode seed budget; plane mode always uses exactly four corners.
pub const TRACKING_MAX_SEEDS: usize = 8;
pub const TRACKING_MAX_TEMPLATE_RADIUS: u32 = 32;
pub const TRACKING_MAX_SEARCH_RADIUS: u32 = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TrackingMode {
    /// One or more independent template-match points.
    Points,
    /// Four tracked corners reduced to a per-frame homography.
    Plane,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TrackingSource {
    pub asset: AssetId,
    pub stream_index: u32,
    /// Locked `Asset::content_hash`; mismatches mark the result stale.
    #[schemars(regex(pattern = "^[0-9a-f]{64}$"))]
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TrackingSeed {
    /// Normalized source-space seed center in `[0, 1]` on both axes.
    pub x: FiniteF64,
    pub y: FiniteF64,
    /// Match-template half extent in source pixels.
    pub template_radius: u32,
    /// Per-frame search half extent around the previous position.
    pub search_radius: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TrackedPoint {
    /// Normalized source-space match center in `[0, 1]` on both axes.
    pub x: FiniteF64,
    pub y: FiniteF64,
    /// Normalized cross-correlation score in `[-1, 1]`.
    pub confidence: FiniteF64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TrackedFrame {
    /// Exact rational source presentation time of the sampled frame.
    pub time: Time,
    /// One entry per seed (points) or per corner (plane).
    pub points: Vec<TrackedPoint>,
    /// Plane mode: row-major homography from first-frame normalized
    /// coordinates onto this frame's normalized coordinates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homography: Option<[FiniteF64; 9]>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TrackingDataAsset {
    pub id: AssetId,
    pub version: u32,
    pub source: TrackingSource,
    pub mode: TrackingMode,
    /// Seeds in stable order; plane mode stores the four corners.
    pub seeds: Vec<TrackingSeed>,
    /// Analyzed source-time interval `[start, end)`.
    pub range: TimeRange,
    /// Nominal sample cadence of `frames` rows in source seconds^-1.
    pub sample_rate: Rational,
    pub frames: Vec<TrackedFrame>,
    /// SHA-256 over the versioned payload; recomputed on validation.
    #[schemars(regex(pattern = "^[0-9a-f]{64}$"))]
    pub content_hash: String,
}

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

impl TrackingSeed {
    pub fn validate(&self) -> Result<(), ProjectError> {
        if !(0.0..=1.0).contains(&self.x.get())
            || !(0.0..=1.0).contains(&self.y.get())
            || !(4..=TRACKING_MAX_TEMPLATE_RADIUS).contains(&self.template_radius)
            || self.search_radius == 0
            || self.search_radius > TRACKING_MAX_SEARCH_RADIUS
        {
            return Err(invalid("invalid tracking seed"));
        }
        Ok(())
    }
}

impl TrackedPoint {
    fn validate(&self) -> Result<(), ProjectError> {
        if !(0.0..=1.0).contains(&self.x.get())
            || !(0.0..=1.0).contains(&self.y.get())
            || !(-1.0..=1.0).contains(&self.confidence.get())
        {
            return Err(invalid("invalid tracked point"));
        }
        Ok(())
    }
}

impl TrackingDataAsset {
    /// Points: `frame`, `confidence`, then `x{i}`/`y{i}`/`score{i}` per seed.
    /// Plane: `frame`, `confidence`, `x0..x3`, `y0..y3`, `h00..h22`.
    /// A missing homography yields the identity matrix.
    pub fn data_table(&self) -> DataTable {
        let mut columns = std::collections::BTreeMap::new();
        columns.insert("frame".to_string(), ValueType::Scalar);
        columns.insert("confidence".to_string(), ValueType::Scalar);
        let count = self.seeds.len();
        for i in 0..count {
            columns.insert(format!("x{i}"), ValueType::Scalar);
            columns.insert(format!("y{i}"), ValueType::Scalar);
            columns.insert(format!("score{i}"), ValueType::Scalar);
        }
        if self.mode == TrackingMode::Plane {
            for name in [
                "h00", "h01", "h02", "h10", "h11", "h12", "h20", "h21", "h22",
            ] {
                columns.insert(name.to_string(), ValueType::Scalar);
            }
        }
        let scalar = |v: f64| Value::Scalar(FiniteF64::new(v).expect("validated finite value"));
        let mut rows = Vec::with_capacity(self.frames.len());
        for (index, frame) in self.frames.iter().enumerate() {
            let mut row = std::collections::BTreeMap::new();
            row.insert("frame".to_string(), scalar(index as f64));
            row.insert(
                "confidence".to_string(),
                scalar(
                    frame
                        .points
                        .iter()
                        .map(|p| p.confidence.get())
                        .fold(1.0, f64::min),
                ),
            );
            for (i, point) in frame.points.iter().enumerate() {
                row.insert(format!("x{i}"), scalar(point.x.get()));
                row.insert(format!("y{i}"), scalar(point.y.get()));
                row.insert(format!("score{i}"), scalar(point.confidence.get()));
            }
            if self.mode == TrackingMode::Plane {
                let identity = [
                    FiniteF64::new(1.0).unwrap(),
                    FiniteF64::new(0.0).unwrap(),
                    FiniteF64::new(0.0).unwrap(),
                    FiniteF64::new(0.0).unwrap(),
                    FiniteF64::new(1.0).unwrap(),
                    FiniteF64::new(0.0).unwrap(),
                    FiniteF64::new(0.0).unwrap(),
                    FiniteF64::new(0.0).unwrap(),
                    FiniteF64::new(1.0).unwrap(),
                ];
                let h = frame.homography.unwrap_or(identity);
                for (i, name) in [
                    "h00", "h01", "h02", "h10", "h11", "h12", "h20", "h21", "h22",
                ]
                .iter()
                .enumerate()
                {
                    row.insert(name.to_string(), Value::Scalar(h[i]));
                }
            }
            rows.push(row);
        }
        DataTable { columns, rows }
    }
    /// SHA-256 over every versioned identity field, in serialized form.
    pub fn computed_hash(&self) -> Result<String, ProjectError> {
        #[derive(Serialize)]
        struct Payload<'a> {
            version: u32,
            source: &'a TrackingSource,
            mode: TrackingMode,
            seeds: &'a [TrackingSeed],
            range: &'a TimeRange,
            sample_rate: &'a Rational,
            frames: &'a [TrackedFrame],
        }
        let bytes = serde_json::to_vec(&Payload {
            version: self.version,
            source: &self.source,
            mode: self.mode,
            seeds: &self.seeds,
            range: &self.range,
            sample_rate: &self.sample_rate,
            frames: &self.frames,
        })
        .map_err(|e| invalid(&e.to_string()))?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
    /// Structural and identity validation; no filesystem access.
    pub fn validate(&self) -> Result<(), ProjectError> {
        if self.version != TRACKING_VERSION {
            return Err(ProjectError::UnsupportedMeaning);
        }
        hash_text(&self.source.content_hash)?;
        hash_text(&self.content_hash)?;
        let expected = match self.mode {
            TrackingMode::Points => 1..=TRACKING_MAX_SEEDS,
            TrackingMode::Plane => 4..=4,
        };
        if !expected.contains(&self.seeds.len())
            || self.sample_rate <= Rational::ZERO
            || self.range.is_empty()
            || self.frames.is_empty()
            || self.frames.len() > TRACKING_MAX_FRAMES as usize
        {
            return Err(invalid("invalid tracking asset bounds"));
        }
        for seed in &self.seeds {
            seed.validate()?;
        }
        let mut previous = None;
        for frame in &self.frames {
            if !self.range.contains(frame.time) || previous.is_some_and(|t| frame.time <= t) {
                return Err(invalid("non-monotonic tracking frame time"));
            }
            previous = Some(frame.time);
            if frame.points.len() != self.seeds.len() {
                return Err(invalid("tracked point count differs from seeds"));
            }
            for point in &frame.points {
                point.validate()?;
            }
            match self.mode {
                TrackingMode::Points if frame.homography.is_some() => {
                    return Err(invalid("point tracking cannot carry a homography"));
                }
                _ => (),
            }
        }
        if self.content_hash != self.computed_hash()? {
            return Err(invalid("tracking DataAsset hash mismatch"));
        }
        Ok(())
    }
    /// Derived expression input. Tracking rows can exceed the authored
    /// DataTable byte budget, so this bypasses `ExpressionDataAsset::new` while
    /// retaining the same `(version, table)` content-hash formula.
    pub fn expression_asset(&self) -> Result<crate::ExpressionDataAsset, ProjectError> {
        self.validate()?;
        let table = self.data_table();
        let bytes = serde_json::to_vec(&(crate::EXPRESSION_DATA_VERSION, &table))
            .map_err(|e| invalid(&e.to_string()))?;
        Ok(crate::ExpressionDataAsset {
            id: self.id,
            version: crate::EXPRESSION_DATA_VERSION,
            content_hash: format!("{:x}", Sha256::digest(bytes)),
            table,
        })
    }
}

impl crate::Project {
    /// Validated tracking assets with live sources, in document order.
    pub fn tracking_data_inputs(&self) -> Result<Vec<TrackingDataAsset>, ProjectError> {
        let mut result = Vec::new();
        for data in &self.tracking_data_assets {
            let crate::DocumentObject::Known(data) = data else {
                return Err(ProjectError::UnsupportedMeaning);
            };
            data.validate()?;
            if !self.assets.iter().any(|a| {
                matches!(a, crate::DocumentObject::Known(a)
                    if a.id == data.source.asset && a.content_hash == data.source.content_hash)
            }) {
                return Err(invalid("stale tracking source"));
            }
            result.push(data.clone());
        }
        Ok(result)
    }
}
