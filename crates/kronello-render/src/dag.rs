use std::collections::{BTreeMap, BTreeSet};

use kronello_eval::Affine2;
use kronello_model::{
    Color, ColorSpace, FillRule, PropertyId, ResolvedGradient, ResolvedShape, Shape, ShapeGeometry,
    Value,
};
use kronello_vector::{FlattenRequest, FlattenedPath};
use serde::{Deserialize, Serialize};

use crate::{MatteKind, RenderError, RenderProfile, SceneContent, SceneIr, SceneKey};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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
    /// Semantic local geometry identity before output mapping or paint.
    pub geometry_content_hash: String,
    /// Coordinates mapped to output pixels; +Y is down. Paint remains explicitly
    /// tagged straight RGB until coverage execution in the working space.
    pub contours: FlattenedPath,
    pub fill: Option<(Color, FillRule)>,
    pub stroke: Option<(
        Color,
        f64,
        kronello_model::StrokeJoin,
        kronello_model::StrokeCap,
        f64,
    )>,
    pub fill_gradient: Option<ResolvedGradient>,
    pub stroke_gradient: Option<ResolvedGradient>,
    pub paint_transform: [[f64; 3]; 2],
}
#[derive(Debug, Clone, PartialEq)]
pub enum DagNode {
    VideoDraw {
        asset: kronello_model::Asset,
        stream_index: u32,
        time: kronello_time::Time,
        extent: [f64; 2],
        output_to_local: [[f64; 3]; 2],
        bounds: crate::PixelBounds,
    },
    /// Explicit CPU-prepared working-space premultiplied image input.
    RasterInput {
        pixels: Vec<[f32; 4]>,
    },
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
    Effect {
        source: usize,
        effect: crate::PixelEffect,
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
            Self::Geometry { .. }
            | Self::TextLayout { .. }
            | Self::VideoDraw { .. }
            | Self::RasterInput { .. } => vec![],
            Self::CoverageDraw { geometry, .. } => vec![*geometry],
            Self::IsolatedComposite { children, .. } => children.clone(),
            Self::Mask { source, matte, .. } => vec![*source, *matte],
            Self::Effect { source, .. } | Self::OutputTransform { source, .. } => vec![*source],
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
    execution_region: OutputRegion,
    requests: Vec<Option<crate::PixelBounds>>,
    bounds: Vec<crate::NodeBounds>,
}
impl RenderDag {
    /// Resolve external video only through a caller-selected media backend.
    /// Sampling is nearest, at the output pixel center, without frame interpolation.
    pub fn resolve_video(
        &self,
        mut decode: impl FnMut(
            &kronello_model::Asset,
            u32,
            kronello_time::Time,
            ColorSpace,
        ) -> Result<crate::VideoImage, RenderError>,
    ) -> Result<Self, RenderError> {
        let mut dag = self.clone();
        for node in &mut dag.nodes {
            if let DagNode::VideoDraw {
                asset,
                stream_index,
                time,
                extent,
                output_to_local,
                ..
            } = node
            {
                let image = decode(asset, *stream_index, *time, dag.working_space)?;
                if image.size.contains(&0)
                    || image.size[0] as usize * image.size[1] as usize != image.pixels.len()
                {
                    return Err(RenderError::InvalidInput("invalid video image size".into()));
                }
                let [w, h] = dag.execution_region.pixels;
                let mapping = kronello_eval::Affine2(*output_to_local);
                let mut pixels = Vec::with_capacity(w as usize * h as usize);
                for y in 0..h {
                    for x in 0..w {
                        let p = mapping.transform_point([f64::from(x) + 0.5, f64::from(y) + 0.5]);
                        pixels.push(
                            if p[0] >= 0.0 && p[1] >= 0.0 && p[0] < extent[0] && p[1] < extent[1] {
                                let sx =
                                    (p[0] * f64::from(image.size[0]) / extent[0]).floor() as usize;
                                let sy =
                                    (p[1] * f64::from(image.size[1]) / extent[1]).floor() as usize;
                                image.pixels[sy * image.size[0] as usize + sx]
                            } else {
                                [0.0; 4]
                            },
                        );
                    }
                }
                *node = DagNode::RasterInput { pixels };
            }
        }
        Ok(dag)
    }
    pub fn nodes(&self) -> &[DagNode] {
        &self.nodes
    }
    pub fn region(&self) -> OutputRegion {
        self.region
    }
    pub fn working_space(&self) -> ColorSpace {
        self.working_space
    }
    pub fn execution_region(&self) -> OutputRegion {
        self.execution_region
    }
    pub fn input_requests(&self) -> &[Option<crate::PixelBounds>] {
        &self.requests
    }
    pub fn bounds(&self) -> &[crate::NodeBounds] {
        &self.bounds
    }
    pub fn crop_origin(&self) -> [usize; 2] {
        std::array::from_fn(|i| {
            ((self.region.origin[i] - self.execution_region.origin[i])
                * f64::from(self.region.pixels[i])
                / self.region.extent[i])
                .round() as usize
        })
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
    semantic_cache: &'a mut crate::RenderCache,
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
            SceneContent::Video {
                asset,
                stream_index,
                time,
                extent,
            } => {
                let b = crate::DesignBounds::checked([0.0; 2], *extent)?.transform(transform)?;
                children.push(self.push(DagNode::VideoDraw {
                    asset: asset.clone(),
                    stream_index: *stream_index,
                    time: *time,
                    extent: *extent,
                    output_to_local: inverse(transform)?,
                    bounds: crate::PixelBounds {
                        min: b.min,
                        max: b.max,
                    },
                })?);
            }
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
                let (geometry_content_hash, contours) =
                    self.semantic_cache
                        .geometry(&resolved.geometry, None, flatten, || {
                            Ok(kronello_vector::flatten(definition, values, flatten)?)
                        })?;
                let contours = map_contours(contours, transform)?;
                let stroke = if let Some(stroke) = &resolved.stroke {
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
                    Some((
                        stroke.color,
                        stroke.width.get() * x,
                        stroke.join,
                        stroke.cap,
                        stroke.miter_limit.get(),
                    ))
                } else {
                    None
                };
                children.push(
                    self.push(DagNode::CoverageDraw {
                        geometry,
                        path: CoveragePath {
                            geometry_content_hash,
                            contours,
                            fill: resolved.fill.as_ref().map(|f| (f.color, f.rule)),
                            fill_gradient: resolved
                                .fill
                                .as_ref()
                                .and_then(|f| f.gradient.as_deref().cloned()),
                            stroke_gradient: resolved
                                .stroke
                                .as_ref()
                                .and_then(|s| s.gradient.as_deref().cloned()),
                            paint_transform: if resolved
                                .fill
                                .as_ref()
                                .is_some_and(|f| f.gradient.is_some())
                                || resolved
                                    .stroke
                                    .as_ref()
                                    .is_some_and(|s| s.gradient.is_some())
                            {
                                inverse(transform)?
                            } else {
                                Affine2::IDENTITY.0
                            },
                            stroke,
                        },
                    })?,
                );
            }
            SceneContent::Text(layout) => {
                let geometry = self.push(DagNode::TextLayout {
                    key: n.key.clone(),
                    layout: layout.clone(),
                })?;
                for glyph in &layout.glyphs {
                    let (geometry_content_hash, contours) = self.semantic_cache.geometry(
                        &kronello_model::ResolvedGeometry::BezierPath(glyph.outline.clone()),
                        n.layout_content_hash.as_deref(),
                        flatten,
                        || flatten_outline(&glyph.outline, flatten),
                    )?;
                    let contours = map_contours(contours, transform)?;
                    children.push(self.push(DagNode::CoverageDraw {
                        geometry,
                        path: CoveragePath {
                            geometry_content_hash,
                            contours,
                            fill: Some((glyph.fill, FillRule::Nonzero)),
                            fill_gradient: None,
                            stroke_gradient: None,
                            paint_transform: Affine2::IDENTITY.0,
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
        let scale =
            std::array::from_fn(|i| f64::from(self.region.pixels[i]) / self.region.extent[i]);
        for effect in &n.effects {
            id = self.push(DagNode::Effect {
                source: id,
                effect: crate::PixelEffect::from_design(
                    &map_effect(effect, n.world_transform)?,
                    scale,
                )?,
            })?;
        }
        if !n.post_effect_opacity.is_finite() || !(0.0..=1.0).contains(&n.post_effect_opacity) {
            return Err(RenderError::InvalidInput(
                "invalid transition opacity".into(),
            ));
        }
        if n.post_effect_opacity != 1.0 {
            id = self.push(DagNode::IsolatedComposite {
                children: vec![id],
                opacity: n.post_effect_opacity,
            })?;
        }
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
    build_render_dag_with_cache(
        scene,
        profile,
        region,
        &mut crate::RenderCache::new(crate::CacheConfig::disabled()),
    )
}

fn build_unpadded_dag(
    scene: &SceneIr,
    profile: RenderProfile,
    region: OutputRegion,
    semantic_cache: &mut crate::RenderCache,
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
        semantic_cache,
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
        execution_region: region,
        requests: vec![],
        bounds: vec![],
    })
}

fn inverse(transform: Affine2) -> Result<[[f64; 3]; 2], RenderError> {
    let [a, b] = transform.0;
    let det = a[0] * b[1] - a[1] * b[0];
    if det == 0.0 {
        return Err(RenderError::InvalidInput(
            "singular gradient mapping".into(),
        ));
    }
    let result = [
        [b[1] / det, -a[1] / det, (a[1] * b[2] - b[1] * a[2]) / det],
        [-b[0] / det, a[0] / det, (b[0] * a[2] - a[0] * b[2]) / det],
    ];
    if result.iter().flatten().any(|v| !v.is_finite()) {
        return Err(RenderError::InvalidInput(
            "non-finite gradient mapping".into(),
        ));
    }
    Ok(result)
}

/// Backward ROI propagation preserves the requested pixel lattice. The initial
/// executor uses one conservative union surface; per-node tiling is future work.
pub fn build_render_dag_with_cache(
    scene: &SceneIr,
    profile: RenderProfile,
    region: OutputRegion,
    cache: &mut crate::RenderCache,
) -> Result<RenderDag, RenderError> {
    use crate::PixelBounds;
    let mut dag = build_unpadded_dag(scene, profile, region, cache)?;
    let bounds = derive_bounds(&dag.nodes);
    let requested = PixelBounds {
        min: [0.0; 2],
        max: region.pixels.map(f64::from),
    };
    let mut requests = vec![None; dag.nodes.len()];
    requests[dag.output()] = Some(requested);
    let mut union = requested;
    for i in (0..dag.nodes.len()).rev() {
        let Some(output) = requests[i] else { continue };
        let input = match &dag.nodes[i] {
            DagNode::Effect { effect, .. } => effect.required_input(output),
            _ => output,
        };
        if input
            .min
            .iter()
            .chain(&input.max)
            .any(|v| !v.is_finite() || v.abs() > 16_777_216.0)
        {
            return Err(RenderError::InvalidInput(
                "effect ROI budget exceeded".into(),
            ));
        }
        for id in dag.nodes[i].inputs() {
            requests[id] = Some(requests[id].map_or(input, |old: PixelBounds| old.union(input)));
        }
        union = union.union(input);
    }
    let min = union.min.map(f64::floor);
    let max = union.max.map(f64::ceil);
    if min != requested.min || max != requested.max {
        let scale: [f64; 2] =
            std::array::from_fn(|i| f64::from(region.pixels[i]) / region.extent[i]);
        let execution = OutputRegion {
            origin: std::array::from_fn(|i| region.origin[i] + min[i] / scale[i]),
            extent: std::array::from_fn(|i| (max[i] - min[i]) / scale[i]),
            pixels: std::array::from_fn(|i| (max[i] - min[i]) as u32),
        };
        execution.validate()?;
        dag = build_unpadded_dag(scene, profile, execution, cache)?;
        dag.region = region;
        dag.execution_region = execution;
    }
    dag.requests = requests;
    dag.bounds = bounds;
    Ok(dag)
}
fn derive_bounds(nodes: &[DagNode]) -> Vec<crate::NodeBounds> {
    use crate::{NodeBounds, PixelBounds};
    let mut bounds: Vec<NodeBounds> = vec![];
    let union = |a: Option<PixelBounds>, b: Option<PixelBounds>| match (a, b) {
        (Some(a), Some(b)) => Some(a.union(b)),
        (a, b) => a.or(b),
    };
    for node in nodes {
        let value = match node {
            DagNode::VideoDraw { bounds, .. } => NodeBounds {
                ink_bounds: Some(*bounds),
                visual_bounds: Some(*bounds),
            },
            DagNode::CoverageDraw { path, .. } => {
                let points: Vec<_> = path
                    .contours
                    .subpaths
                    .iter()
                    .flat_map(|s| s.points.iter())
                    .collect();
                let ink = if points.is_empty() || (path.fill.is_none() && path.stroke.is_none()) {
                    None
                } else {
                    let b = PixelBounds {
                        min: std::array::from_fn(|i| {
                            points.iter().map(|p| p[i]).fold(f64::INFINITY, f64::min)
                        }),
                        max: std::array::from_fn(|i| {
                            points
                                .iter()
                                .map(|p| p[i])
                                .fold(f64::NEG_INFINITY, f64::max)
                        }),
                    };
                    Some(b.expand(
                        [path.stroke.map_or(0.0, |(_, w, join, cap, m)| {
                            crate::bounds::stroke_halo(w, join, cap, m)
                        }); 2],
                    ))
                };
                NodeBounds {
                    ink_bounds: ink,
                    visual_bounds: ink,
                }
            }
            DagNode::IsolatedComposite { children, .. } => {
                children
                    .iter()
                    .fold(NodeBounds::default(), |a, id| NodeBounds {
                        ink_bounds: union(a.ink_bounds, bounds[*id].ink_bounds),
                        visual_bounds: union(a.visual_bounds, bounds[*id].visual_bounds),
                    })
            }
            DagNode::Effect { source, effect } => NodeBounds {
                ink_bounds: bounds[*source].ink_bounds,
                visual_bounds: bounds[*source]
                    .visual_bounds
                    .map(|b| effect.output_bounds(b)),
            },
            DagNode::Mask { source, .. } | DagNode::OutputTransform { source, .. } => {
                bounds[*source]
            }
            _ => NodeBounds::default(),
        };
        bounds.push(value);
    }
    bounds
}

pub(crate) fn map_effect(
    effect: &kronello_model::ResolvedEffect,
    transform: Affine2,
) -> Result<kronello_model::ResolvedEffect, RenderError> {
    use kronello_model::ResolvedEffect;
    let [a, b] = transform.0;
    let x = a[0].hypot(b[0]);
    let y = a[1].hypot(b[1]);
    let dot = a[0] * a[1] + b[0] * b[1];
    let sigma = match effect {
        ResolvedEffect::GaussianBlur { sigma } | ResolvedEffect::DropShadow { sigma, .. } => *sigma,
    };
    if sigma > 0.0
        && ((x - y).abs() > 1e-10 * x.max(y).max(1.0) || dot.abs() > 1e-10 * (x * y).max(1.0))
    {
        return Err(RenderError::UnsupportedFeature(
            "nonuniform transformed Gaussian effect".into(),
        ));
    }
    Ok(match effect {
        ResolvedEffect::GaussianBlur { .. } => ResolvedEffect::GaussianBlur { sigma: sigma * x },
        ResolvedEffect::DropShadow {
            offset,
            color,
            opacity,
            ..
        } => ResolvedEffect::DropShadow {
            sigma: sigma * x,
            offset: [
                a[0] * offset[0] + a[1] * offset[1],
                b[0] * offset[0] + b[1] * offset[1],
            ],
            color: *color,
            opacity: *opacity,
        },
    })
}
