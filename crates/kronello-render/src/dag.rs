use std::collections::{BTreeMap, BTreeSet};

use kronello_eval::Affine2;
use kronello_model::{
    BlendMode, Color, ColorSpace, FillRule, MaskMode, NodeId, PropertyId, ResolvedGradient,
    ResolvedMask, ResolvedShape, Shape, ShapeGeometry, Value,
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
            || u64::from(self.pixels[0]) * u64::from(self.pixels[1]) > 33_177_600
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
    pub stroke_geometry: Option<LocalStrokeGeometry>,
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
    pub fill_gradient: Option<Box<ResolvedGradient>>,
    pub stroke_gradient: Option<Box<ResolvedGradient>>,
    pub paint_transform: [[f64; 3]; 2],
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocalStrokeGeometry {
    pub version: String,
    pub contours: FlattenedPath,
    pub output_to_local: [[f64; 3]; 2],
    pub local_to_output: [[f64; 3]; 2],
    pub alignment: kronello_model::StrokeAlignment,
    pub fill_rule: FillRule,
    pub dash_array: Vec<f64>,
    pub dash_offset: f64,
}
#[derive(Debug, Clone, PartialEq)]
pub enum DagNode {
    VideoDraw {
        asset: kronello_model::Asset,
        stream_index: u32,
        time: kronello_time::Time,
        reverse_sampling: bool,
        extent: [f64; 2],
        /// AI-003 (ADR-0126): source-pixel window `[x, y, w, h]` sampled by
        /// the draw; `None` samples the full frame.
        crop: Option<[f64; 4]>,
        /// TRACK-003 (ADR-0123): authored intermediate-frame synthesis for
        /// this source sample; `None` decodes the single containing frame.
        interpolation: Option<kronello_time::FrameInterpolation>,
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
    Blend {
        source: usize,
        backdrop: usize,
        mode: BlendMode,
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
    /// FX-003 transition primitive: a filled output-pixel rectangle in the
    /// working space. Used as the wipe reveal matte and the dip underlay.
    SolidRect {
        color: Color,
        /// Half-open [min_x, min_y, max_x, max_y] in output pixels.
        rect: [f64; 4],
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
            | Self::RasterInput { .. }
            | Self::SolidRect { .. } => vec![],
            Self::CoverageDraw { geometry, .. } => vec![*geometry],
            Self::IsolatedComposite { children, .. } => children.clone(),
            Self::Mask { source, matte, .. } => vec![*source, *matte],
            Self::Blend {
                source, backdrop, ..
            } => vec![*source, *backdrop],
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
    hdr: Option<crate::HdrSettings>,
    execution_region: OutputRegion,
    requests: Vec<Option<crate::PixelBounds>>,
    bounds: Vec<crate::NodeBounds>,
}
impl RenderDag {
    /// Resolve external video only through a caller-selected media backend.
    /// Sampling is nearest, at the output pixel center; an authored
    /// `interpolation` mode lets the backend synthesize the exact source
    /// instant from the neighboring decoded frames (TRACK-003, ADR-0123).
    pub fn resolve_video(
        &self,
        mut decode: impl FnMut(
            &kronello_model::Asset,
            u32,
            kronello_time::Time,
            ColorSpace,
            bool,
            Option<kronello_time::FrameInterpolation>,
        ) -> Result<crate::VideoImage, RenderError>,
    ) -> Result<Self, RenderError> {
        let mut dag = self.clone();
        for node in &mut dag.nodes {
            if let DagNode::VideoDraw {
                asset,
                stream_index,
                time,
                reverse_sampling,
                extent,
                crop,
                interpolation,
                output_to_local,
                ..
            } = node
            {
                let image = decode(
                    asset,
                    *stream_index,
                    *time,
                    dag.working_space,
                    *reverse_sampling,
                    *interpolation,
                )?;
                if image.size.contains(&0)
                    || image.size[0] as usize * image.size[1] as usize != image.pixels.len()
                {
                    return Err(RenderError::InvalidInput("invalid video image size".into()));
                }
                // AI-003 (ADR-0126): the crop window is a source-pixel
                // rectangle; local design coordinates map onto the window,
                // not the full frame.
                let window =
                    crop.unwrap_or([0.0, 0.0, f64::from(image.size[0]), f64::from(image.size[1])]);
                if window[0] < 0.0
                    || window[1] < 0.0
                    || window[2] <= 0.0
                    || window[3] <= 0.0
                    || window[0] + window[2] > f64::from(image.size[0]) + 1e-6
                    || window[1] + window[3] > f64::from(image.size[1]) + 1e-6
                {
                    return Err(RenderError::InvalidInput(
                        "video crop window outside source bounds".into(),
                    ));
                }
                let [w, h] = dag.execution_region.pixels;
                let mapping = kronello_eval::Affine2(*output_to_local);
                let mut pixels = Vec::with_capacity(w as usize * h as usize);
                for y in 0..h {
                    for x in 0..w {
                        let p = mapping.transform_point([f64::from(x) + 0.5, f64::from(y) + 0.5]);
                        pixels.push(
                            if p[0] >= 0.0 && p[1] >= 0.0 && p[0] < extent[0] && p[1] < extent[1] {
                                let fx = window[0] + p[0] * window[2] / extent[0];
                                let fy = window[1] + p[1] * window[3] / extent[1];
                                let sx = (fx.floor() as i64).clamp(0, i64::from(image.size[0]) - 1)
                                    as usize;
                                let sy = (fy.floor() as i64).clamp(0, i64::from(image.size[1]) - 1)
                                    as usize;
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
    /// Conservative tile allocation including one surface per image stage,
    /// Group child/accumulator surfaces, three effect temporaries and reserves.
    /// Every stage currently uses the union execution ROI, including its halo.
    pub fn tile_surface_bytes(&self, pixel_bytes: u64) -> Result<u64, RenderError> {
        let mut surfaces = 4_u64;
        for node in &self.nodes {
            surfaces += match node {
                DagNode::IsolatedComposite { children, .. } => children.len() as u64 + 2,
                DagNode::Effect { .. } => 4,
                DagNode::CoverageDraw { .. }
                | DagNode::Blend { .. }
                | DagNode::Mask { .. }
                | DagNode::VideoDraw { .. }
                | DagNode::RasterInput { .. }
                | DagNode::SolidRect { .. } => 1,
                _ => 0,
            };
        }
        u64::from(self.execution_region.pixels[0])
            .checked_mul(u64::from(self.execution_region.pixels[1]))
            .and_then(|v| v.checked_mul(pixel_bytes))
            .and_then(|v| v.checked_mul(surfaces))
            .ok_or_else(|| RenderError::UnsupportedFeature("tile surface budget overflow".into()))
    }
    pub fn nodes(&self) -> &[DagNode] {
        &self.nodes
    }
    pub fn region(&self) -> OutputRegion {
        self.region
    }
    pub fn hdr(&self) -> Option<crate::HdrSettings> {
        self.hdr
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
    fn composite(
        &mut self,
        children: Vec<usize>,
        opacity: f64,
        modes: &BTreeMap<usize, BlendMode>,
    ) -> Result<usize, RenderError> {
        if modes.values().all(|m| *m == BlendMode::Normal) {
            return self.push(DagNode::IsolatedComposite { children, opacity });
        }
        let mut prefix = vec![];
        for child in children {
            let mode = modes.get(&child).copied().unwrap_or_default();
            if mode == BlendMode::Normal {
                prefix.push(child);
            } else {
                let backdrop = self.push(DagNode::IsolatedComposite {
                    children: prefix,
                    opacity: 1.0,
                })?;
                let blended = self.push(DagNode::Blend {
                    source: child,
                    backdrop,
                    mode,
                })?;
                prefix = vec![blended];
            }
        }
        self.push(DagNode::IsolatedComposite {
            children: prefix,
            opacity,
        })
    }
    /// FX-004: rasterize one resolved mask into a white premultiplied
    /// coverage raster (alpha = coverage). Expansion offsets the flattened
    /// path in node-local design_px; feather blurs coverage in output pixels.
    fn mask_coverage(
        &mut self,
        key: &SceneKey,
        world: Affine2,
        mask: &ResolvedMask,
        transform: Affine2,
        flatten: FlattenRequest,
        scale: [f64; 2],
    ) -> Result<usize, RenderError> {
        let property = PropertyId::from_uuid(uuid::Uuid::nil());
        let shape = Shape {
            id: kronello_model::ContentId::from_uuid(uuid::Uuid::nil()),
            geometry: ShapeGeometry::BezierPath { path: property },
            fill: None,
            stroke: None,
        };
        let values = BTreeMap::from([(property, Value::Path(mask.path.clone()))]);
        let resolved = shape.resolve(&values)?;
        let geometry = self.push(DagNode::Geometry {
            key: SceneKey {
                instance_path: key.instance_path.clone(),
                node: NodeId::from_uuid(mask.id.as_uuid()),
            },
            resolved: resolved.clone(),
        })?;
        let (geometry_content_hash, contours) =
            self.semantic_cache
                .geometry(&resolved.geometry, None, flatten, || {
                    flatten_outline(&mask.path, flatten)
                })?;
        // `closed` decides whether expansion offsets each subpath as a ring;
        // fill coverage always closes independently.
        let mut contours = contours;
        for subpath in &mut contours.subpaths {
            subpath.closed |= mask.closed;
        }
        let contours = if mask.expansion != 0.0 {
            kronello_vector::offset_path(&contours, mask.expansion)?
        } else {
            contours
        };
        let contours = map_contours(contours, transform)?;
        let white = Color::from_srgb8([255; 3], None);
        let mut id = self.push(DagNode::CoverageDraw {
            geometry,
            path: CoveragePath {
                stroke_geometry: None,
                geometry_content_hash,
                contours,
                fill: Some((white, FillRule::Nonzero)),
                stroke: None,
                fill_gradient: None,
                stroke_gradient: None,
                paint_transform: Affine2::IDENTITY.0,
            },
        })?;
        if mask.feather > 0.0 {
            let effect = map_effect(
                &kronello_model::ResolvedEffect::GaussianBlur {
                    sigma: mask.feather,
                },
                world,
            )?;
            id = self.push(DagNode::Effect {
                source: id,
                effect: crate::PixelEffect::from_design(&effect, scale)?,
            })?;
        }
        if mask.opacity != 1.0 {
            id = self.push(DagNode::IsolatedComposite {
                children: vec![id],
                opacity: mask.opacity,
            })?;
        }
        if mask.invert {
            id = self.inverted_coverage(id)?;
        }
        Ok(id)
    }
    fn inverted_coverage(&mut self, coverage: usize) -> Result<usize, RenderError> {
        let full = self.push(DagNode::SolidRect {
            color: Color::from_srgb8([255; 3], None),
            rect: [
                0.0,
                0.0,
                f64::from(self.region.pixels[0]),
                f64::from(self.region.pixels[1]),
            ],
        })?;
        self.push(DagNode::Mask {
            source: full,
            matte: coverage,
            kind: MatteKind::AlphaInverted,
        })
    }
    /// Combine mask coverages in authored order. The first mask initializes
    /// the accumulated coverage (a leading Subtract inverts from full, the
    /// conventional single-subtract behavior); each later mask applies its
    /// mode against the accumulator.
    fn mask_stack_coverage(
        &mut self,
        n: &crate::SceneNodeIr,
        transform: Affine2,
        flatten: FlattenRequest,
        scale: [f64; 2],
    ) -> Result<Option<usize>, RenderError> {
        let mut accumulated: Option<usize> = None;
        for mask in &n.masks {
            let coverage =
                self.mask_coverage(&n.key, n.world_transform, mask, transform, flatten, scale)?;
            accumulated = Some(match (accumulated, mask.mode) {
                (None, MaskMode::Subtract) => self.inverted_coverage(coverage)?,
                (None, _) => coverage,
                (Some(acc), MaskMode::Add) => self.push(DagNode::IsolatedComposite {
                    children: vec![acc, coverage],
                    opacity: 1.0,
                })?,
                (Some(acc), MaskMode::Subtract) => self.push(DagNode::Mask {
                    source: acc,
                    matte: coverage,
                    kind: MatteKind::AlphaInverted,
                })?,
                (Some(acc), MaskMode::Intersect) => self.push(DagNode::Mask {
                    source: acc,
                    matte: coverage,
                    kind: MatteKind::Alpha,
                })?,
                (Some(acc), MaskMode::Difference) => {
                    let a = self.push(DagNode::Mask {
                        source: acc,
                        matte: coverage,
                        kind: MatteKind::AlphaInverted,
                    })?;
                    let b = self.push(DagNode::Mask {
                        source: coverage,
                        matte: acc,
                        kind: MatteKind::AlphaInverted,
                    })?;
                    self.push(DagNode::IsolatedComposite {
                        children: vec![a, b],
                        opacity: 1.0,
                    })?
                }
            });
        }
        Ok(accumulated)
    }
    /// FX-004: multiply the clip's drawn alpha by its accumulated mask
    /// coverage (no-op when the stack is empty).
    fn apply_mask_stack(
        &mut self,
        source: usize,
        n: &crate::SceneNodeIr,
        transform: Affine2,
        flatten: FlattenRequest,
        scale: [f64; 2],
    ) -> Result<usize, RenderError> {
        if let Some(matte) = self.mask_stack_coverage(n, transform, flatten, scale)? {
            return self.push(DagNode::Mask {
                source,
                matte,
                kind: MatteKind::Alpha,
            });
        }
        Ok(source)
    }
    /// FX-007 (ADR-0116): rewrite the accumulated lower composite through the
    /// adjustment node's effect chain. Masks and node opacity bound where the
    /// adjustment applies: `kept` retains the untouched backdrop outside the
    /// coverage, `filtered` carries the effected backdrop inside it.
    fn apply_adjustment(
        &mut self,
        prefix: Vec<usize>,
        prefix_modes: &BTreeMap<usize, BlendMode>,
        n: &crate::SceneNodeIr,
        scale: [f64; 2],
    ) -> Result<usize, RenderError> {
        let backdrop = self.composite(prefix, 1.0, prefix_modes)?;
        let transform = self.region.design_to_pixel().compose(n.world_transform);
        let magnification = {
            let [a, b] = transform.0;
            (a[0] * a[0] + a[1] * a[1] + b[0] * b[0] + b[1] * b[1]).sqrt()
        };
        let flatten =
            FlattenRequest::new(magnification.max(1.0), self.profile.flatten_tolerance_px)?;
        let mut id = backdrop;
        for effect in &n.effects {
            id = self.push(DagNode::Effect {
                source: id,
                effect: crate::PixelEffect::from_design(
                    &map_effect(effect, n.world_transform)?,
                    scale,
                )?,
            })?;
        }
        let strength = n.opacity * n.post_effect_opacity;
        if !(strength.is_finite() && (0.0..=1.0).contains(&strength)) {
            return Err(RenderError::InvalidInput(
                "invalid adjustment opacity".into(),
            ));
        }
        if let Some(matte) = self.mask_stack_coverage(n, transform, flatten, scale)? {
            // Coverage scales by the adjustment strength so a faded or
            // half-strength adjustment lerps between backdrop and effect.
            let coverage = if strength != 1.0 {
                self.push(DagNode::IsolatedComposite {
                    children: vec![matte],
                    opacity: strength,
                })?
            } else {
                matte
            };
            let kept = self.push(DagNode::Mask {
                source: backdrop,
                matte: coverage,
                kind: MatteKind::AlphaInverted,
            })?;
            let filtered = self.push(DagNode::Mask {
                source: id,
                matte: coverage,
                kind: MatteKind::Alpha,
            })?;
            id = self.push(DagNode::IsolatedComposite {
                children: vec![kept, filtered],
                opacity: 1.0,
            })?;
        } else if strength != 1.0 {
            // Maskless half-strength adjustment: lerp over the whole frame.
            let full = self.push(DagNode::SolidRect {
                color: Color::from_srgb8([255; 3], None),
                rect: [
                    0.0,
                    0.0,
                    f64::from(self.region.pixels[0]),
                    f64::from(self.region.pixels[1]),
                ],
            })?;
            let coverage = self.push(DagNode::IsolatedComposite {
                children: vec![full],
                opacity: strength,
            })?;
            let kept = self.push(DagNode::Mask {
                source: backdrop,
                matte: coverage,
                kind: MatteKind::AlphaInverted,
            })?;
            let filtered = self.push(DagNode::Mask {
                source: id,
                matte: coverage,
                kind: MatteKind::Alpha,
            })?;
            id = self.push(DagNode::IsolatedComposite {
                children: vec![kept, filtered],
                opacity: 1.0,
            })?;
        }
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
            return Err(RenderError::Backend {
                code: "MATTE_CYCLE",
                message: "cyclic containment/matte dependency".into(),
            });
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
        if matches!(n.content, SceneContent::Adjustment) {
            // The root-loop rewrite performs the actual pass; standalone
            // lowering draws nothing (visited only by the dependency sweep).
            let id = self.composite(vec![], 1.0, &BTreeMap::new())?;
            self.visiting.remove(&index);
            self.cache.insert(index, id);
            return Ok(id);
        }
        let mut children = vec![];
        match &n.content {
            SceneContent::Empty | SceneContent::Adjustment => (),
            SceneContent::Video {
                asset,
                stream_index,
                time,
                extent,
                crop,
                reverse_sampling,
                interpolation,
            } => {
                let b = crate::DesignBounds::checked([0.0; 2], *extent)?.transform(transform)?;
                children.push(self.push(DagNode::VideoDraw {
                    asset: asset.clone(),
                    stream_index: *stream_index,
                    time: *time,
                    reverse_sampling: *reverse_sampling,
                    extent: *extent,
                    crop: *crop,
                    interpolation: *interpolation,
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
                let bounds = kronello_vector::geometry_bounds(&resolved.geometry)?;
                let fill_gradient = prepare_gradient(
                    resolved.fill.as_ref().and_then(|f| f.gradient.as_deref()),
                    bounds,
                )?;
                let stroke_gradient = prepare_gradient(
                    resolved.stroke.as_ref().and_then(|f| f.gradient.as_deref()),
                    bounds,
                )?;
                let stroke_geometry = resolved
                    .stroke
                    .as_ref()
                    .and_then(|s| s.options.as_ref())
                    .map(|o| -> Result<LocalStrokeGeometry, RenderError> {
                        if o.alignment != kronello_model::StrokeAlignment::Center
                            && contours.subpaths.iter().any(|c| !c.closed)
                        {
                            return Err(kronello_model::ShapeError::OpenStrokeAlignment.into());
                        }
                        Ok(LocalStrokeGeometry {
                            version: o.geometry_version.clone(),
                            contours: kronello_vector::dash_path(
                                &contours,
                                &o.dash_array,
                                o.dash_offset,
                            )?,
                            output_to_local: inverse(transform)?,
                            local_to_output: transform.0,
                            alignment: o.alignment,
                            fill_rule: o.fill_rule,
                            dash_array: o.dash_array.clone(),
                            dash_offset: o.dash_offset,
                        })
                    })
                    .transpose()?;
                let contours = map_contours(contours, transform)?;
                let stroke = if let Some(stroke) = &resolved.stroke {
                    let x = a[0].hypot(b[0]);
                    let y = a[1].hypot(b[1]);
                    let dot = a[0] * a[1] + b[0] * b[1];
                    if stroke_geometry.is_none()
                        && ((x - y).abs() > 1e-10 * x.max(y).max(1.0)
                            || dot.abs() > 1e-10 * (x * y).max(1.0))
                    {
                        return Err(RenderError::UnsupportedFeature(format!(
                            "nonuniform transformed stroke on {:?}",
                            n.key
                        )));
                    }
                    Some((
                        stroke.color,
                        stroke.width.get() * if stroke_geometry.is_some() { 1.0 } else { x },
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
                            stroke_geometry,
                            geometry_content_hash,
                            contours,
                            fill: resolved.fill.as_ref().map(|f| (f.color, f.rule)),
                            fill_gradient,
                            stroke_gradient,
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
                    let gradient = prepare_gradient(
                        glyph.gradient.as_deref(),
                        layout.ink_bounds.map(|b| (b.min, b.max)),
                    )?;
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
                            stroke_geometry: None,
                            geometry_content_hash,
                            contours,
                            fill: Some((glyph.fill, FillRule::Nonzero)),
                            paint_transform: if gradient.is_some() {
                                inverse(transform)?
                            } else {
                                Affine2::IDENTITY.0
                            },
                            fill_gradient: gradient,
                            stroke_gradient: None,
                            stroke: None,
                        },
                    })?);
                }
            }
            SceneContent::Caption(caption) => {
                let geometry = self.push(DagNode::TextLayout {
                    key: n.key.clone(),
                    layout: caption.layout.clone(),
                })?;
                // Caption glyphs anchor inside the sequence safe area: the
                // placement translation composes inside the node transform.
                let caption_transform = transform.compose(Affine2([
                    [1.0, 0.0, caption.origin[0]],
                    [0.0, 1.0, caption.origin[1]],
                ]));
                let italic_shear = Affine2([
                    [1.0, -kronello_model::CAPTION_ITALIC_SHEAR, 0.0],
                    [0.0, 1.0, 0.0],
                ]);
                let [a, b] = caption_transform.0;
                // Stroke widths are authored in design_px; scale them like
                // shape strokes by the output transform's x-axis norm.
                let stroke_scale = a[0].hypot(b[0]);
                if let Some(background) = caption.background {
                    let bounds = caption.layout.layout_bounds;
                    let contours = map_contours(
                        FlattenedPath {
                            subpaths: vec![kronello_vector::Polyline {
                                points: vec![
                                    bounds.min,
                                    [bounds.max[0], bounds.min[1]],
                                    bounds.max,
                                    [bounds.min[0], bounds.max[1]],
                                ],
                                closed: true,
                            }],
                        },
                        caption_transform,
                    )?;
                    children.push(self.push(DagNode::CoverageDraw {
                        geometry,
                        path: CoveragePath {
                            stroke_geometry: None,
                            geometry_content_hash:
                                n.layout_content_hash.clone().unwrap_or_default(),
                            contours,
                            fill: Some((background, FillRule::Nonzero)),
                            stroke: None,
                            fill_gradient: None,
                            stroke_gradient: None,
                            paint_transform: Affine2::IDENTITY.0,
                        },
                    })?);
                }
                for glyph in &caption.layout.glyphs {
                    let flags = caption
                        .span_flags
                        .get(glyph.style_index)
                        .copied()
                        .unwrap_or_default();
                    // Synthesized italic is an oblique shear in text-local
                    // space, applied before the placement translation.
                    let glyph_transform = if flags.italic {
                        caption_transform.compose(italic_shear)
                    } else {
                        caption_transform
                    };
                    let (geometry_content_hash, contours) = self.semantic_cache.geometry(
                        &kronello_model::ResolvedGeometry::BezierPath(glyph.outline.clone()),
                        n.layout_content_hash.as_deref(),
                        flatten,
                        || flatten_outline(&glyph.outline, flatten),
                    )?;
                    let contours = map_contours(contours, glyph_transform)?;
                    // Order: outline ring behind the fill, then the bold ring,
                    // then the glyph fill. A centered stroke at twice the
                    // authored width leaves exactly that width visible.
                    let mut draw = |stroke: Option<(
                        Color,
                        f64,
                        kronello_model::StrokeJoin,
                        kronello_model::StrokeCap,
                        f64,
                    )>,
                                    fill: Option<(Color, FillRule)>|
                     -> Result<usize, RenderError> {
                        self.push(DagNode::CoverageDraw {
                            geometry,
                            path: CoveragePath {
                                stroke_geometry: None,
                                geometry_content_hash: geometry_content_hash.clone(),
                                contours: contours.clone(),
                                fill,
                                stroke,
                                fill_gradient: None,
                                stroke_gradient: None,
                                paint_transform: Affine2::IDENTITY.0,
                            },
                        })
                    };
                    if let Some(outline) = &caption.outline
                        && outline.width.get() > 0.0
                    {
                        children.push(draw(
                            Some((
                                outline.color,
                                outline.width.get() * 2.0 * stroke_scale,
                                kronello_model::StrokeJoin::Round,
                                kronello_model::StrokeCap::Round,
                                4.0,
                            )),
                            None,
                        )?);
                    }
                    if flags.bold && caption.bold_width > 0.0 {
                        children.push(draw(
                            Some((
                                glyph.fill,
                                caption.bold_width * 2.0 * stroke_scale,
                                kronello_model::StrokeJoin::Round,
                                kronello_model::StrokeCap::Round,
                                4.0,
                            )),
                            None,
                        )?);
                    }
                    children.push(draw(None, Some((glyph.fill, FillRule::Nonzero)))?);
                }
            }
        }
        let mut blend_modes = BTreeMap::new();
        for child in 0..self.scene.nodes.len() {
            let c = &self.scene.nodes[child];
            if c.parent.as_ref() == Some(&n.key) && !self.hidden.contains(&c.key) {
                let mode = c.blend_mode;
                let id = self.node(child, depth + 1)?;
                children.push(id);
                blend_modes.insert(id, mode);
            }
        }
        // Opacity applies once after fill/stroke/glyph/child compositing.
        let mut id = self.composite(children, n.opacity, &blend_modes)?;
        let scale =
            std::array::from_fn(|i| f64::from(self.region.pixels[i]) / self.region.extent[i]);
        // FX-004 (ADR-0114): the mask stack multiplies drawn alpha after the
        // clip's own content and before its effect chain.
        id = self.apply_mask_stack(id, n, transform, flatten, scale)?;
        for effect in &n.effects {
            let mapped = map_effect(effect, n.world_transform)?;
            let pixel = if let kronello_model::ResolvedEffect::ColorLut {
                lut: asset_id,
                intensity,
            } = &mapped
            {
                // COLOR-003: the lattice was bound to this asset during scene
                // IR construction; absence here is a typed render failure.
                let lattice =
                    self.scene
                        .luts
                        .get(asset_id)
                        .ok_or_else(|| RenderError::Backend {
                            code: "LUT_INPUT_MISSING",
                            message: format!("lut data for asset {asset_id} was not resolved"),
                        })?;
                let effect = crate::PixelEffect::ColorLut {
                    lut: lattice.clone(),
                    intensity: *intensity as f32,
                };
                effect.validate()?;
                effect
            } else if let kronello_model::ResolvedEffect::Stabilize {
                inverse: tracked_inverse,
                border,
                fill_color,
                sampling,
                ..
            } = &mapped
            {
                // TRACK-002 (ADR-0122): the scene pass bound `inverse` — the
                // source-space inverse correction C^-1 — to this node's video
                // content. `frame` composes C^-1 behind output→local so the
                // kernel resolves corrected source-extent positions; `unmap`
                // is the draw transform re-embedding the border-resolved
                // position into the input raster where the frame was drawn.
                let crate::SceneContent::Video { extent, .. } = &n.content else {
                    return Err(RenderError::InvalidInput(
                        "stabilize requires video content".into(),
                    ));
                };
                let Some(frame_inverse) = tracked_inverse else {
                    return Err(RenderError::InvalidInput(
                        "stabilize tracking data unresolved".into(),
                    ));
                };
                let frame = kronello_eval::Affine2(*frame_inverse)
                    .compose(kronello_eval::Affine2(inverse(transform)?));
                crate::PixelEffect::stabilize(
                    frame,
                    transform,
                    *extent,
                    *border,
                    *fill_color,
                    *sampling,
                )?
            } else {
                crate::PixelEffect::from_design(&mapped, scale)?
            };
            id = self.push(DagNode::Effect {
                source: id,
                effect: pixel,
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
        // FX-003: wipe reveals through an opaque-rect alpha matte; dip inserts
        // a solid-color underlay below the faded incoming result (ADR-0109).
        for transition in &n.transitions {
            match transition {
                crate::SceneTransition::Reveal { min, max } => {
                    let to_pixel = self.region.design_to_pixel();
                    let lo = to_pixel.transform_point(*min);
                    let hi = to_pixel.transform_point(*max);
                    let rect = [
                        lo[0].min(hi[0]),
                        lo[1].min(hi[1]),
                        lo[0].max(hi[0]),
                        lo[1].max(hi[1]),
                    ];
                    if !rect.iter().all(|v| v.is_finite()) {
                        return Err(RenderError::InvalidInput(
                            "invalid wipe reveal bounds".into(),
                        ));
                    }
                    let matte = self.push(DagNode::SolidRect {
                        color: Color::from_srgb8([255; 3], None),
                        rect,
                    })?;
                    id = self.push(DagNode::Mask {
                        source: id,
                        matte,
                        kind: MatteKind::Alpha,
                    })?;
                }
                crate::SceneTransition::Dip { color, opacity } => {
                    let rect = [
                        0.0,
                        0.0,
                        f64::from(self.region.pixels[0]),
                        f64::from(self.region.pixels[1]),
                    ];
                    let underlay = self.push(DagNode::SolidRect {
                        color: *color,
                        rect,
                    })?;
                    let underlay = self.push(DagNode::IsolatedComposite {
                        children: vec![underlay],
                        opacity: *opacity,
                    })?;
                    id = self.push(DagNode::IsolatedComposite {
                        children: vec![underlay, id],
                        opacity: 1.0,
                    })?;
                }
            }
        }
        if let Some(binding) = self.scene.mattes.iter().find(|m| m.source == n.key) {
            let matte = *self
                .indices
                .get(&binding.matte)
                .ok_or_else(|| RenderError::Backend {
                    code: "MATTE_MISSING",
                    message: format!("missing active matte {:?}", binding.matte),
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
            return Err(RenderError::Backend {
                code: "MATTE_MISSING",
                message: "duplicate source or missing active matte binding".into(),
            });
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
    let scale: [f64; 2] = std::array::from_fn(|i| f64::from(region.pixels[i]) / region.extent[i]);
    let mut roots = vec![];
    let mut root_modes: BTreeMap<usize, BlendMode> = BTreeMap::new();
    for (i, n) in scene.nodes.iter().enumerate() {
        if n.parent.is_some() || b.hidden.contains(&n.key) {
            continue;
        }
        if matches!(n.content, SceneContent::Adjustment) {
            // FX-007: the adjustment rewrites everything composited below it;
            // stacking order falls out of the authored root order.
            if !roots.is_empty() {
                let adjusted =
                    b.apply_adjustment(std::mem::take(&mut roots), &root_modes, n, scale)?;
                root_modes.clear();
                roots.push(adjusted);
                root_modes.insert(adjusted, BlendMode::Normal);
            }
            continue;
        }
        let id = b.node(i, 1)?;
        roots.push(id);
        root_modes.insert(id, n.blend_mode);
    }
    // Check all nodes, including a cycle consisting entirely of hidden mattes.
    for i in 0..scene.nodes.len() {
        b.node(i, 1)?;
    }
    let composite = b.composite(roots, 1.0, &root_modes)?;
    b.push(DagNode::OutputTransform {
        source: composite,
        display_space: ColorSpace::Srgb,
    })?;
    Ok(RenderDag {
        nodes: b.nodes,
        region,
        working_space: profile.working_space,
        hdr: profile.hdr,
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
    // FX-006: a corner pin warps the incoming surface's own corner rectangle
    // into the authored quad, so the backward ROI walk needs the source quad
    // on each pin before it runs.
    patch_corner_pin_sources(&mut dag);
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
        patch_corner_pin_sources(&mut dag);
    }
    dag.requests = requests;
    dag.bounds = bounds;
    Ok(dag)
}
/// FX-006 corner pin: record the input node's visual bounds as the warp
/// source quad, on the DAG's current pixel lattice (ADR-0115).
fn patch_corner_pin_sources(dag: &mut RenderDag) {
    let bounds = derive_bounds(&dag.nodes);
    for node in &mut dag.nodes {
        let DagNode::Effect { source, effect } = node else {
            continue;
        };
        if let crate::PixelEffect::CornerPin { source: quad, .. } = effect {
            *quad = bounds[*source].visual_bounds;
        }
    }
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
                    let halo = path.stroke.map_or(0.0, |(_, w, join, cap, m)| {
                        crate::bounds::stroke_halo(w, join, cap, m)
                    });
                    let halo = path.stroke_geometry.as_ref().map_or([halo; 2], |g| {
                        let factor = match g.alignment {
                            kronello_model::StrokeAlignment::Center => 1.0,
                            kronello_model::StrokeAlignment::Inside => 0.0,
                            kronello_model::StrokeAlignment::Outside => 2.0,
                        };
                        g.local_to_output
                            .map(|r| halo * factor * (r[0].abs() + r[1].abs()))
                    });
                    Some(b.expand(halo))
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
            DagNode::Blend {
                source, backdrop, ..
            } => NodeBounds {
                ink_bounds: union(bounds[*source].ink_bounds, bounds[*backdrop].ink_bounds),
                visual_bounds: union(
                    bounds[*source].visual_bounds,
                    bounds[*backdrop].visual_bounds,
                ),
            },
            DagNode::Effect { source, effect } => NodeBounds {
                ink_bounds: bounds[*source].ink_bounds,
                visual_bounds: bounds[*source]
                    .visual_bounds
                    .map(|b| effect.output_bounds(b)),
            },
            DagNode::Mask { source, .. } | DagNode::OutputTransform { source, .. } => {
                bounds[*source]
            }
            DagNode::SolidRect { rect, .. } => {
                let ink = Some(PixelBounds {
                    min: [rect[0], rect[1]],
                    max: [rect[2], rect[3]],
                });
                NodeBounds {
                    ink_bounds: ink,
                    visual_bounds: ink,
                }
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
    // COLOR-002/COLOR-003 pointwise operations commute with any placement
    // transform.
    if matches!(
        effect,
        ResolvedEffect::ColorExposure { .. }
            | ResolvedEffect::ColorLevels { .. }
            | ResolvedEffect::ColorCurves { .. }
            | ResolvedEffect::ColorHsl { .. }
            | ResolvedEffect::ColorLut { .. }
    ) {
        return Ok(effect.clone());
    }
    let [a, b] = transform.0;
    if let ResolvedEffect::AffineGaussianBlur { sigma, linear }
    | ResolvedEffect::AffineDropShadow { sigma, linear, .. } = effect
    {
        let mapped =
            [a, b].map(|row| [0, 1].map(|j| row[0] * linear[0][j] + row[1] * linear[1][j]));
        crate::validate_affine_linear(mapped)?;
        return Ok(match effect {
            ResolvedEffect::AffineGaussianBlur { .. } => ResolvedEffect::AffineGaussianBlur {
                sigma: *sigma,
                linear: mapped,
            },
            ResolvedEffect::AffineDropShadow {
                offset,
                color,
                opacity,
                ..
            } => ResolvedEffect::AffineDropShadow {
                sigma: *sigma,
                linear: mapped,
                offset: [
                    a[0] * offset[0] + a[1] * offset[1],
                    b[0] * offset[0] + b[1] * offset[1],
                ],
                color: *color,
                opacity: *opacity,
            },
            _ => unreachable!(),
        });
    }
    let x = a[0].hypot(b[0]);
    let y = a[1].hypot(b[1]);
    let dot = a[0] * a[1] + b[0] * b[1];
    // Design-px lengths scale by the x-axis norm; a nonuniform or sheared
    // transform rejects positive widths exactly like the Gaussian effects.
    let length = |v: f64| -> Result<f64, RenderError> {
        if v > 0.0
            && ((x - y).abs() > 1e-10 * x.max(y).max(1.0) || dot.abs() > 1e-10 * (x * y).max(1.0))
        {
            return Err(RenderError::UnsupportedFeature(
                "nonuniform transformed Gaussian effect".into(),
            ));
        }
        Ok(v * x)
    };
    Ok(match effect {
        ResolvedEffect::GaussianBlur { sigma } => ResolvedEffect::GaussianBlur {
            sigma: length(*sigma)?,
        },
        ResolvedEffect::DropShadow {
            sigma,
            offset,
            color,
            opacity,
            ..
        } => ResolvedEffect::DropShadow {
            sigma: length(*sigma)?,
            offset: [
                a[0] * offset[0] + a[1] * offset[1],
                b[0] * offset[0] + b[1] * offset[1],
            ],
            color: *color,
            opacity: *opacity,
        },
        // Vignette is normalized-position pointwise and corner pins are
        // already absolute Composition design_px positions; both commute.
        // TRACK-002 stabilize limits live in source-pixel space and its
        // inverse transform is applied through the pixel-stage `unmap`; the
        // resolved value passes through untouched.
        ResolvedEffect::Vignette { .. }
        | ResolvedEffect::CornerPin { .. }
        | ResolvedEffect::Stabilize { .. } => effect.clone(),
        ResolvedEffect::ChromaKey {
            key_color,
            similarity,
            edge_shrink,
            edge_feather,
            spill,
        } => ResolvedEffect::ChromaKey {
            key_color: *key_color,
            similarity: *similarity,
            edge_shrink: length(*edge_shrink)?,
            edge_feather: length(*edge_feather)?,
            spill: *spill,
        },
        ResolvedEffect::LumaKey {
            key_luma,
            tolerance,
            edge_shrink,
            edge_feather,
        } => ResolvedEffect::LumaKey {
            key_luma: *key_luma,
            tolerance: *tolerance,
            edge_shrink: length(*edge_shrink)?,
            edge_feather: length(*edge_feather)?,
        },
        ResolvedEffect::Glow {
            threshold,
            radius,
            intensity,
        } => ResolvedEffect::Glow {
            threshold: *threshold,
            radius: length(*radius)?,
            intensity: *intensity,
        },
        ResolvedEffect::Sharpen { amount, radius } => ResolvedEffect::Sharpen {
            amount: *amount,
            radius: length(*radius)?,
        },
        _ => unreachable!("color and affine variants handled above"),
    })
}

// The object box is unstroked local geometry (text: complete positioned ink).
// Normalize the unit mapping before lowering; each fill/stroke keeps its own map.
fn prepare_gradient(
    g: Option<&ResolvedGradient>,
    bounds: Option<([f64; 2], [f64; 2])>,
) -> Result<Option<Box<ResolvedGradient>>, RenderError> {
    let Some(g) = g else {
        return Ok(None);
    };
    if g.options.interpolation_version != 1 {
        return Err(RenderError::UnsupportedFeature(
            "gradient interpolation version".into(),
        ));
    }
    let mut g = g.clone();
    let mut transform = Affine2(
        g.options
            .transform
            .map(|r| r.map(kronello_model::FiniteF64::get)),
    );
    if g.options.units == kronello_model::GradientUnits::ObjectBoundingBox {
        let (min, max) = bounds.ok_or_else(|| {
            RenderError::InvalidInput("empty gradient object bounding box".into())
        })?;
        if max[0] <= min[0] || max[1] <= min[1] {
            return Err(RenderError::InvalidInput(
                "degenerate gradient object bounding box".into(),
            ));
        }
        transform = Affine2([
            [max[0] - min[0], 0.0, min[0]],
            [0.0, max[1] - min[1], min[1]],
        ])
        .compose(transform);
    }
    inverse(transform)?;
    g.options.transform = transform
        .0
        .map(|r| r.map(|v| kronello_model::FiniteF64::new(v).expect("validated finite transform")));
    g.options.units = kronello_model::GradientUnits::LocalDesign;
    Ok(Some(Box::new(g)))
}
