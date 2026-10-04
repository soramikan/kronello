//! Deletable, bounded semantic caches. No document revision or device handles.
use std::collections::VecDeque;

use kronello_eval::{DependencyGraph, EvaluationError, RuntimePropertyKey};
use kronello_model::{Color, ColorSpace, ResolvedGeometry, ResolvedText, Value};
use kronello_text::{FontData, LayoutResult};
use kronello_time::Time;
use kronello_vector::{FlattenRequest, FlattenedPath};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{
    COLOR_VERSION, COVERAGE_VERSION, CoveragePath, OutputRegion, RenderError, VECTOR_VERSION,
};

const KEY_VERSION: &str = "cache001-json-sha256-v1";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheCapacity {
    pub entries: usize,
    pub bytes: usize,
}
impl Default for CacheCapacity {
    fn default() -> Self {
        Self {
            entries: 256,
            bytes: 64 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheConfig {
    pub values: CacheCapacity,
    pub layout: CacheCapacity,
    pub geometry: CacheCapacity,
    pub raster: CacheCapacity,
}
impl CacheConfig {
    /// Zero-capacity caches execute the same code without retaining entries.
    pub fn disabled() -> Self {
        let zero = CacheCapacity {
            entries: 0,
            bytes: 0,
        };
        Self {
            values: zero,
            layout: zero,
            geometry: zero,
            raster: zero,
        }
    }
}
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub inserts: u64,
    pub evictions: u64,
    pub entries: usize,
    /// Retained payload weight, excluding allocator overhead. Values use JSON byte size.
    pub bytes: usize,
}
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct RenderCacheStats {
    pub values: CacheStats,
    pub layout: CacheStats,
    pub geometry: CacheStats,
    pub raster: CacheStats,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Key([u8; 32]);
fn key(domain: &str, value: impl Serialize) -> Result<Key, RenderError> {
    let value = serde_json::to_value((KEY_VERSION, domain, value))?;
    Ok(Key(Sha256::digest(serde_json::to_vec(&value)?).into()))
}
struct Lru<T> {
    capacity: CacheCapacity,
    entries: VecDeque<(Key, T, usize)>,
    stats: CacheStats,
}
impl<T: Clone> Lru<T> {
    fn new(capacity: CacheCapacity) -> Self {
        Self {
            capacity,
            entries: VecDeque::new(),
            stats: CacheStats::default(),
        }
    }
    fn get(&mut self, key: Key) -> Option<T> {
        if let Some(i) = self.entries.iter().position(|(k, _, _)| *k == key) {
            let entry = self.entries.remove(i).expect("located entry");
            let value = entry.1.clone();
            self.entries.push_back(entry);
            self.stats.hits += 1;
            Some(value)
        } else {
            self.stats.misses += 1;
            None
        }
    }
    fn insert(&mut self, key: Key, value: T, bytes: usize) {
        if self.capacity.entries == 0 || bytes > self.capacity.bytes {
            return;
        }
        while self.entries.len() >= self.capacity.entries
            || self.stats.bytes > self.capacity.bytes - bytes
        {
            let (_, _, weight) = self.entries.pop_front().expect("full cache");
            self.stats.bytes -= weight;
            self.stats.evictions += 1;
        }
        self.entries.push_back((key, value, bytes));
        self.stats.bytes += bytes;
        self.stats.inserts += 1;
        self.stats.entries = self.entries.len();
    }
    fn clear(&mut self) {
        self.entries.clear();
        self.stats.entries = 0;
        self.stats.bytes = 0;
    }
    fn reset_stats(&mut self) {
        self.stats = CacheStats {
            entries: self.entries.len(),
            bytes: self.stats.bytes,
            ..CacheStats::default()
        };
    }
}
/// Owned by the caller and reusable across snapshots, frames, and output regions.
/// Failed computations are never retained; clearing preserves cumulative counters.
pub struct RenderCache {
    values: Lru<Value>,
    layout: Lru<LayoutResult>,
    geometry: Lru<FlattenedPath>,
    raster: Lru<Vec<[f32; 4]>>,
}
impl Default for RenderCache {
    fn default() -> Self {
        Self::new(CacheConfig::default())
    }
}
impl RenderCache {
    pub fn new(config: CacheConfig) -> Self {
        Self {
            values: Lru::new(config.values),
            layout: Lru::new(config.layout),
            geometry: Lru::new(config.geometry),
            raster: Lru::new(config.raster),
        }
    }
    pub fn stats(&self) -> RenderCacheStats {
        RenderCacheStats {
            values: self.values.stats,
            layout: self.layout.stats,
            geometry: self.geometry.stats,
            raster: self.raster.stats,
        }
    }
    pub fn clear(&mut self) {
        self.values.clear();
        self.layout.clear();
        self.geometry.clear();
        self.raster.clear();
    }
    pub fn reset_stats(&mut self) {
        self.values.reset_stats();
        self.layout.reset_stats();
        self.geometry.reset_stats();
        self.raster.reset_stats();
    }

    pub(crate) fn evaluate(
        &mut self,
        graph: &DependencyGraph<'_>,
        identity: &str,
        runtime: &RuntimePropertyKey,
        time: Time,
    ) -> Result<Value, EvaluationError> {
        // Serialize the complete typed runtime identity without relying on Debug.
        let runtime_json = match runtime {
            RuntimePropertyKey::LayoutValue {
                instance_path,
                text,
                consumer,
            } => json!(["layout", instance_path, text, consumer]),
            RuntimePropertyKey::Node(k) => json!(["node", k.instance_path, k.node, k.property]),
            RuntimePropertyKey::Composition {
                instance_path,
                composition,
                property,
            } => json!(["composition", instance_path, composition, property]),
        };
        let cache_key =
            key("values", (identity, runtime_json, time)).expect("validated runtime key JSON");
        if let Some(value) = self.values.get(cache_key) {
            return Ok(value);
        }
        let value = graph.evaluate_property(runtime, time)?;
        let bytes = serde_json::to_vec(&value)
            .expect("validated value JSON")
            .len();
        self.values.insert(cache_key, value.clone(), bytes);
        Ok(value)
    }

    pub fn layout(
        &mut self,
        text: &ResolvedText,
        fonts: &[FontData<'_>],
    ) -> Result<LayoutResult, RenderError> {
        // Validate bytes on every call: warm cache must not hide missing/corrupt locks.
        text.validate()?;
        let cache_key = layout_key(text)?;
        let mut layout = if let Some(layout) = self.layout.get(cache_key) {
            kronello_text::validate_fonts(text, fonts)?;
            layout
        } else {
            let mut canonical = text.clone();
            for style in &mut canonical.styles {
                style.fill = Color::from_srgb8([0, 0, 0], None);
            }
            let result = kronello_text::layout(&canonical, fonts)?;
            // Accounts for outlines and all source/cluster/line metadata.
            let bytes = std::mem::size_of::<LayoutResult>()
                + std::mem::size_of_val(result.lines.as_slice())
                + std::mem::size_of_val(result.glyphs.as_slice())
                + std::mem::size_of_val(result.shaping_clusters.as_slice())
                + std::mem::size_of_val(result.animation_units.as_slice())
                + std::mem::size_of_val(result.graphemes.as_slice())
                + result
                    .glyphs
                    .iter()
                    .map(|g| std::mem::size_of_val(g.outline.segments.as_slice()))
                    .sum::<usize>();
            self.layout.insert(cache_key, result.clone(), bytes);
            result
        };
        for glyph in &mut layout.glyphs {
            glyph.fill = text.styles[glyph.style_index].fill;
        }
        Ok(layout)
    }
    pub(crate) fn geometry(
        &mut self,
        geometry: &ResolvedGeometry,
        provenance: Option<&str>,
        request: FlattenRequest,
        compute: impl FnOnce() -> Result<FlattenedPath, RenderError>,
    ) -> Result<(String, FlattenedPath), RenderError> {
        let geometry = match geometry {
            ResolvedGeometry::Rectangle {
                size,
                corner_radius,
            } => json!(["rectangle", size, corner_radius]),
            ResolvedGeometry::Ellipse { size } => json!(["ellipse", size]),
            ResolvedGeometry::BezierPath(path) => json!(["path", path]),
        };
        // Exact tolerance/scale is the scale bucket: no quantization changes output.
        let cache_key = key(
            "geometry",
            (
                VECTOR_VERSION,
                provenance,
                geometry,
                request.tolerance_design(),
            ),
        )?;
        let identity = hex(cache_key);
        if let Some(path) = self.geometry.get(cache_key) {
            return Ok((identity, path));
        }
        let path = compute()?;
        let bytes = path.subpaths.iter().map(|p| 32 + p.points.len() * 16).sum();
        self.geometry.insert(cache_key, path.clone(), bytes);
        Ok((identity, path))
    }
    /// Backend-owned raster computation. Namespace must pin numeric execution
    /// semantics and, for strict GPU caching, the device/driver fingerprint.
    pub fn rasterize<E>(
        &mut self,
        key: RasterCacheKey,
        compute: impl FnOnce() -> Result<Vec<[f32; 4]>, E>,
    ) -> Result<Vec<[f32; 4]>, E> {
        if let Some(pixels) = self.raster.get(key.0) {
            return Ok(pixels);
        }
        let pixels = compute()?;
        self.raster.insert(key.0, pixels.clone(), pixels.len() * 16);
        Ok(pixels)
    }
}
/// Opaque content hash, never a DAG index, document revision, or GPU object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RasterCacheKey(Key);
impl RasterCacheKey {
    /// Effect identities include the ordered dependency image identity, exact
    /// evaluated parameters, semantic versions, ROI, color and backend namespace.
    pub fn for_dag(
        dag: &crate::RenderDag,
        backend_namespace: &str,
    ) -> Result<Vec<Option<Self>>, RenderError> {
        let mut keys: Vec<Option<Self>> = vec![];
        for node in dag.nodes() {
            let value = match node {
                crate::DagNode::VideoDraw { .. } => {
                    return Err(RenderError::UnsupportedFeature(
                        "unresolved video input".into(),
                    ));
                }
                crate::DagNode::RasterInput { pixels } => Some(Self(key(
                    "video-raster",
                    (
                        pixels,
                        dag.execution_region(),
                        dag.working_space(),
                        backend_namespace,
                    ),
                )?)),
                crate::DagNode::Geometry { .. } | crate::DagNode::TextLayout { .. } => None,
                crate::DagNode::CoverageDraw { path, .. } => Some(Self::new(
                    path,
                    dag.execution_region(),
                    dag.working_space(),
                    backend_namespace,
                )?),
                crate::DagNode::IsolatedComposite { children, opacity } => Some(Self(key(
                    "isolated-composite",
                    (
                        children
                            .iter()
                            .map(|id| keys[*id].map(|k| hex(k.0)))
                            .collect::<Vec<_>>(),
                        opacity,
                    ),
                )?)),
                crate::DagNode::Mask {
                    source,
                    matte,
                    kind,
                } => Some(Self(key(
                    "mask",
                    (
                        keys[*source].map(|k| hex(k.0)),
                        keys[*matte].map(|k| hex(k.0)),
                        kind,
                    ),
                )?)),
                crate::DagNode::Effect { source, effect } => Some(Self(key(
                    "effect",
                    (
                        crate::EFFECT_KERNEL_VERSION,
                        kronello_model::EFFECT_VERSION,
                        keys[*source].map(|k| hex(k.0)),
                        effect,
                        dag.execution_region(),
                        dag.working_space(),
                        backend_namespace,
                    ),
                )?)),
                crate::DagNode::OutputTransform { source, .. } => keys[*source],
            };
            keys.push(value);
        }
        Ok(keys)
    }
    pub fn new(
        path: &CoveragePath,
        region: OutputRegion,
        working: ColorSpace,
        backend_namespace: &str,
    ) -> Result<Self, RenderError> {
        let contours: Vec<_> = path
            .contours
            .subpaths
            .iter()
            .map(|p| (&p.points, p.closed))
            .collect();
        Ok(Self(key(
            "raster",
            (
                VECTOR_VERSION,
                COVERAGE_VERSION,
                COLOR_VERSION,
                backend_namespace,
                &path.geometry_content_hash,
                contours,
                path.fill,
                path.stroke,
                &path.fill_gradient,
                &path.stroke_gradient,
                path.paint_transform,
                crate::STROKE_GEOMETRY_VERSION,
                crate::GRADIENT_INTERPOLATION_VERSION,
                region,
                working,
            ),
        )?))
    }
}

fn hex(key: Key) -> String {
    key.0.iter().map(|b| format!("{b:02x}")).collect()
}
fn layout_key(text: &ResolvedText) -> Result<Key, RenderError> {
    let styles: Vec<_> = text
        .styles
        .iter()
        .map(|s| (s.range, &s.font, s.size))
        .collect();
    let alignment = match text.alignment {
        kronello_model::TextAlignment::Start => "start",
        kronello_model::TextAlignment::Center => "center",
        kronello_model::TextAlignment::End => "end",
    };
    key(
        "layout",
        (
            text.layout_version,
            &text.text,
            styles,
            text.direction,
            &text.ruby,
            text.wrap_width,
            text.line_height,
            alignment,
        ),
    )
}
/// The same paint-free identity used by layout and downstream glyph geometry.
pub fn layout_content_hash(text: &ResolvedText) -> Result<String, RenderError> {
    Ok(hex(layout_key(text)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lru_promotes_hits_evicts_oldest_and_enforces_byte_and_entry_limits() {
        let mut cache = Lru::new(CacheCapacity {
            entries: 2,
            bytes: 5,
        });
        let a = Key([1; 32]);
        let b = Key([2; 32]);
        let c = Key([3; 32]);
        cache.insert(a, 10, 2);
        cache.insert(b, 20, 2);
        assert_eq!(cache.get(a), Some(10));
        cache.insert(c, 30, 2);
        assert_eq!(cache.get(b), None);
        assert_eq!(cache.get(a), Some(10));
        assert_eq!(cache.get(c), Some(30));
        assert_eq!(
            (
                cache.stats.entries,
                cache.stats.bytes,
                cache.stats.evictions
            ),
            (2, 4, 1)
        );
        cache.insert(b, 20, 6);
        assert_eq!(cache.get(b), None);
        cache.clear();
        assert_eq!((cache.stats.entries, cache.stats.bytes), (0, 0));
    }
    #[test]
    fn failed_raster_computations_are_not_cached_and_namespaces_are_distinct() {
        let path = CoveragePath {
            geometry_content_hash: "shape-v1".into(),
            fill_gradient: None,
            stroke_gradient: None,
            paint_transform: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            contours: FlattenedPath { subpaths: vec![] },
            fill: None,
            stroke: None,
        };
        let region = OutputRegion {
            origin: [0.0; 2],
            extent: [1.0; 2],
            pixels: [1; 2],
        };
        let a = RasterCacheKey::new(&path, region, ColorSpace::LinearRec709, "cpu-v1").unwrap();
        let b = RasterCacheKey::new(&path, region, ColorSpace::LinearRec709, "cpu-v2").unwrap();
        assert_ne!(a, b);
        let mut cache = RenderCache::default();
        assert_eq!(
            cache.rasterize(a, || Err::<Vec<[f32; 4]>, _>("failure")),
            Err("failure")
        );
        let pixels = vec![[0.0; 4]];
        assert_eq!(
            cache
                .rasterize(a, || Ok::<_, &str>(pixels.clone()))
                .unwrap(),
            pixels
        );
        assert_eq!(
            cache
                .rasterize::<&str>(a, || panic!("hit must not compute"))
                .unwrap(),
            pixels
        );
        assert_eq!(
            cache
                .rasterize(b, || Ok::<_, &str>(pixels.clone()))
                .unwrap(),
            pixels
        );
        assert_eq!(
            (
                cache.stats().raster.hits,
                cache.stats().raster.misses,
                cache.stats().raster.inserts
            ),
            (1, 3, 2)
        );
    }
}
