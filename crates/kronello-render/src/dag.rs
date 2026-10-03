use std::collections::{BTreeMap, BTreeSet};

use kronello_eval::Affine2;
use kronello_model::{
    Color, ColorSpace, FillRule, PropertyId, ResolvedShape, Shape, ShapeGeometry, StrokeCap,
    StrokeJoin, Value,
};
use kronello_vector::{FlattenRequest, FlattenedPath};
use serde::{Deserialize, Serialize};

use crate::{MatteKind, RenderError, RenderProfile, SceneContent, SceneIr, SceneKey};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputRegion {
    pub origin: [f64; 2],
    pub extent: [f64; 2],
    pub pixels: [u32; 2],
}
impl OutputRegion {
    pub fn validate(self) -> Result<(), RenderError> {
        if self
            .origin
            .iter()
            .chain(&self.extent)
            .any(|v| !v.is_finite())
            || self.extent.iter().any(|v| *v <= 0.0)
            || self.pixels.contains(&0)
            || u64::from(self.pixels[0]) * u64::from(self.pixels[1]) > 16_777_216
        {
            return Err(RenderError::InvalidInput(
                "invalid output region or pixel budget".into(),
            ));
        }
        let m = self.design_to_pixel();
        if m.0.iter().flatten().any(|v| !v.is_finite()) {
            return Err(RenderError::InvalidInput(
                "non-finite region mapping".into(),
            ));
        }
        Ok(())
    }
    pub fn design_to_pixel(self) -> Affine2 {
        let x = f64::from(self.pixels[0]) / self.extent[0];
        let y = f64::from(self.pixels[1]) / self.extent[1];
        Affine2([[x, 0.0, -self.origin[0] * x], [0.0, y, -self.origin[1] * y]])
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoveragePath {
    /// Coordinates mapped to output pixels; +Y is down. Paint remains explicitly
    /// tagged straight RGB until coverage execution in the working space.
    pub contours: FlattenedPath,
    pub fill: Option<(Color, FillRule)>,
    pub stroke: Option<(Color, f64)>,
}
#[derive(Debug, Clone, PartialEq)]
pub enum DagNode {
    Geometry {
        key: SceneKey,
        resolved: ResolvedShape,
    },
    TextLayout {
        key: SceneKey,
        layout: kronello_text::LayoutResult,
    },
    CoverageDraw {
        geometry: usize,
        path: CoveragePath,
    },
    IsolatedComposite {
        children: Vec<usize>,
        opacity: f64,
    },
    Mask {
        source: usize,
        matte: usize,
        kind: MatteKind,
    },
    /// Produces both linear premultiplied truth and straight sRGB display.
    OutputTransform {
        source: usize,
        display_space: ColorSpace,
    },
}
impl DagNode {
    pub fn inputs(&self) -> Vec<usize> {
        match self {
            Self::Geometry { .. } | Self::TextLayout { .. } => vec![],
            Self::CoverageDraw { geometry, .. } => vec![*geometry],
            Self::IsolatedComposite { children, .. } => children.clone(),
            Self::Mask { source, matte, .. } => vec![*source, *matte],
            Self::OutputTransform { source, .. } => vec![*source],
        }
    }
}
/// Topological nodes: all input indices precede the consumer. Geometry/text
/// remain semantic plain data; only coverage contours depend on output scale.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderDag {
    nodes: Vec<DagNode>,
    region: OutputRegion,
    working_space: ColorSpace,
}
impl RenderDag {
    pub fn nodes(&self) -> &[DagNode] {
        &self.nodes
    }
    pub fn region(&self) -> OutputRegion {
        self.region
    }
    pub fn working_space(&self) -> ColorSpace {
        self.working_space
    }
    pub fn output(&self) -> usize {
        self.nodes.len() - 1
    }
}

struct Builder<'a> {
    scene: &'a SceneIr,
    profile: RenderProfile,
    region: OutputRegion,
    indices: BTreeMap<SceneKey, usize>,
    hidden: BTreeSet<SceneKey>,
    cache: BTreeMap<usize, usize>,
    visiting: BTreeSet<usize>,
    nodes: Vec<DagNode>,
}
impl Builder<'_> {
    fn push(&mut self, node: DagNode) -> Result<usize, RenderError> {
        if self.nodes.len() >= 4096 {
            return Err(RenderError::UnsupportedFeature(
                "render DAG node budget exceeded".into(),
            ));
        }
        let id = self.nodes.len();
        self.nodes.push(node);
        Ok(id)
    }
    fn node(&mut self, index: usize, depth: usize) -> Result<usize, RenderError> {
        if depth > 24 {
            return Err(RenderError::UnsupportedFeature(
                "scene nesting exceeds render budget".into(),
            ));
        }
        if let Some(id) = self.cache.get(&index) {
            return Ok(*id);
        }
        if !self.visiting.insert(index) {
            return Err(RenderError::InvalidInput(
                "cyclic containment/matte dependency".into(),
            ));
        }
        let n = &self.scene.nodes[index];
        if !n.opacity.is_finite() || !(0.0..=1.0).contains(&n.opacity) {
            return Err(RenderError::InvalidInput("invalid scene opacity".into()));
        }
        let transform = self.region.design_to_pixel().compose(n.world_transform);
        if transform.0.iter().flatten().any(|v| !v.is_finite()) {
            return Err(RenderError::InvalidInput(
                "non-finite scene transform".into(),
            ));
        }
        let [a, b] = transform.0;
        // Frobenius norm conservatively bounds maximum singular magnification.
        let magnification = (a[0] * a[0] + a[1] * a[1] + b[0] * b[0] + b[1] * b[1]).sqrt();
        let flatten =
            FlattenRequest::new(magnification.max(1.0), self.profile.flatten_tolerance_px)?;
        let mut children = vec![];
        match &n.content {
            SceneContent::Empty => (),
            SceneContent::Shape {
                definition,
                values,
                resolved,
            } => {
                if definition.resolve(values)? != *resolved {
                    return Err(RenderError::InvalidInput(
                        "inconsistent resolved shape IR".into(),
                    ));
                }
                let geometry = self.push(DagNode::Geometry {
                    key: n.key.clone(),
                    resolved: resolved.clone(),
                })?;
                let contours = map_contours(
                    kronello_vector::flatten(definition, values, flatten)?,
                    transform,
                )?;
                let stroke = if let Some(stroke) = &resolved.stroke {
                    if stroke.join != StrokeJoin::Round || stroke.cap != StrokeCap::Round {
                        return Err(RenderError::UnsupportedFeature(format!(
                            "stroke cap/join on {:?}",
                            n.key
                        )));
                    }
                    let x = a[0].hypot(b[0]);
                    let y = a[1].hypot(b[1]);
                    let dot = a[0] * a[1] + b[0] * b[1];
                    if (x - y).abs() > 1e-10 * x.max(y).max(1.0)
                        || dot.abs() > 1e-10 * (x * y).max(1.0)
                    {
                        return Err(RenderError::UnsupportedFeature(format!(
                            "nonuniform transformed stroke on {:?}",
                            n.key
                        )));
                    }
                    Some((stroke.color, stroke.width.get() * x))
                } else {
                    None
                };
                children.push(self.push(DagNode::CoverageDraw {
                    geometry,
                    path: CoveragePath {
                        contours,
                        fill: resolved.fill.as_ref().map(|f| (f.color, f.rule)),
                        stroke,
                    },
                })?);
            }
            SceneContent::Text(layout) => {
                let geometry = self.push(DagNode::TextLayout {
                    key: n.key.clone(),
                    layout: layout.clone(),
                })?;
                for glyph in &layout.glyphs {
                    let contours =
                        map_contours(flatten_outline(&glyph.outline, flatten)?, transform)?;
                    children.push(self.push(DagNode::CoverageDraw {
                        geometry,
                        path: CoveragePath {
                            contours,
                            fill: Some((glyph.fill, FillRule::Nonzero)),
                            stroke: None,
                        },
                    })?);
                }
            }
        }
        for child in 0..self.scene.nodes.len() {
            let c = &self.scene.nodes[child];
            if c.parent.as_ref() == Some(&n.key) && !self.hidden.contains(&c.key) {
                children.push(self.node(child, depth + 1)?);
            }
        }
        // Opacity applies once after fill/stroke/glyph/child compositing.
        let mut id = self.push(DagNode::IsolatedComposite {
            children,
            opacity: n.opacity,
        })?;
        if let Some(binding) = self.scene.mattes.iter().find(|m| m.source == n.key) {
            let matte = *self.indices.get(&binding.matte).ok_or_else(|| {
                RenderError::InvalidInput(format!("missing active matte {:?}", binding.matte))
            })?;
            let matte = self.node(matte, depth + 1)?;
            id = self.push(DagNode::Mask {
                source: id,
                matte,
                kind: binding.kind,
            })?;
        }
        self.visiting.remove(&index);
        self.cache.insert(index, id);
        Ok(id)
    }
}

fn map_contours(
    mut contours: FlattenedPath,
    transform: Affine2,
) -> Result<FlattenedPath, RenderError> {
    // One-point/empty subpaths have no coverage and are intentionally omitted.
    contours.subpaths.retain(|c| c.points.len() >= 2);
    for contour in &mut contours.subpaths {
        for p in &mut contour.points {
            *p = transform.transform_point(*p);
            if p.iter().any(|v| !v.is_finite() || v.abs() > 1_000_000.0) {
                return Err(RenderError::InvalidInput(
                    "coverage coordinate outside finite backend range".into(),
                ));
            }
        }
    }
    Ok(contours)
}
fn flatten_outline(
    path: &kronello_model::Path,
    request: FlattenRequest,
) -> Result<FlattenedPath, RenderError> {
    // Transient parameters, never document or instance identities. Fixed values
    // keep compilation deterministic, including equality of rebuilt DAGs.
    let id = PropertyId::from_uuid(uuid::Uuid::nil());
    let shape = Shape {
        id: kronello_model::ContentId::from_uuid(uuid::Uuid::nil()),
        geometry: ShapeGeometry::BezierPath { path: id },
        fill: None,
        stroke: None,
    };
    Ok(kronello_vector::flatten(
        &shape,
        &BTreeMap::from([(id, Value::Path(path.clone()))]),
        request,
    )?)
}

pub fn build_render_dag(
    scene: &SceneIr,
    profile: RenderProfile,
    region: OutputRegion,
) -> Result<RenderDag, RenderError> {
    region.validate()?;
    if profile.working_space == ColorSpace::Srgb
        || !profile.flatten_tolerance_px.is_finite()
        || profile.flatten_tolerance_px <= 0.0
    {
        return Err(RenderError::InvalidInput("invalid render profile".into()));
    }
    if scene.nodes.len() > 1024 {
        return Err(RenderError::UnsupportedFeature(
            "scene node budget exceeded".into(),
        ));
    }
    let indices: BTreeMap<_, _> = scene
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.key.clone(), i))
        .collect();
    if indices.len() != scene.nodes.len()
        || scene
            .nodes
            .iter()
            .any(|n| n.parent.as_ref().is_some_and(|p| !indices.contains_key(p)))
    {
        return Err(RenderError::InvalidInput(
            "duplicate scene key or missing parent".into(),
        ));
    }
    let mut sources = BTreeSet::new();
    let mut hidden = BTreeSet::new();
    for m in &scene.mattes {
        if !indices.contains_key(&m.source)
            || !indices.contains_key(&m.matte)
            || !sources.insert(m.source.clone())
        {
            return Err(RenderError::InvalidInput(
                "duplicate source or missing active matte binding".into(),
            ));
        }
        if !m.visible {
            hidden.insert(m.matte.clone());
        }
    }
    let mut b = Builder {
        scene,
        profile,
        region,
        indices,
        hidden,
        cache: BTreeMap::new(),
        visiting: BTreeSet::new(),
        nodes: vec![],
    };
    let mut roots = vec![];
    for (i, n) in scene.nodes.iter().enumerate() {
        if n.parent.is_none() && !b.hidden.contains(&n.key) {
            roots.push(b.node(i, 1)?);
        }
    }
    // Check all nodes, including a cycle consisting entirely of hidden mattes.
    for i in 0..scene.nodes.len() {
        b.node(i, 1)?;
    }
    let composite = b.push(DagNode::IsolatedComposite {
        children: roots,
        opacity: 1.0,
    })?;
    b.push(DagNode::OutputTransform {
        source: composite,
        display_space: ColorSpace::Srgb,
    })?;
    Ok(RenderDag {
        nodes: b.nodes,
        region,
        working_space: profile.working_space,
    })
}
