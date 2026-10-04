//! Small derived draw-list boundary; no document, font, or store types.
use crate::{GpuError, InputSpace, RenderSize, WorkingSpace, color, pixel_count};

pub const COVERAGE_SAMPLES: u32 = 16;
pub const SCENE_SHADER: &str = include_str!("scene.wgsl");
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillRule {
    Nonzero,
    Evenodd,
}
#[derive(Debug, Clone, Copy)]
pub struct Paint {
    pub rgba: [f32; 4],
    pub space: InputSpace,
}
#[derive(Debug, Clone)]
pub struct Contour {
    pub points: Vec<[f32; 2]>,
    pub closed: bool,
}
#[derive(Debug, Clone, Copy)]
pub struct Fill {
    pub paint: Paint,
    pub rule: FillRule,
}
/// Centered stroke geometry expanded once for CPU and GPU point sampling.
#[derive(Debug, Clone, Copy)]
pub struct RoundStroke {
    pub join: StrokeJoin,
    pub cap: StrokeCap,
    pub miter_limit: f32,
    pub paint: Paint,
    pub width: f32,
}
pub use kronello_model::{StrokeCap, StrokeJoin};
#[derive(Debug, Clone)]
pub struct GradientPaint {
    pub geometry: GradientGeometry,
    pub stops: Vec<GradientStop>,
}
#[derive(Debug, Clone, Copy)]
pub enum GradientGeometry {
    Linear { start: [f32; 2], end: [f32; 2] },
    Radial { center: [f32; 2], radius: f32 },
}
#[derive(Debug, Clone, Copy)]
pub struct GradientStop {
    pub offset: f32,
    pub paint: Paint,
}
/// Flattened, transformed design-space geometry. Fill implicitly closes contours;
/// stroke closes only contours marked closed. Fill is drawn before stroke.
#[derive(Debug, Clone)]
pub struct PathDraw {
    pub fill_gradient: Option<GradientPaint>,
    pub stroke_gradient: Option<GradientPaint>,
    /// Output design coordinates to local paint coordinates.
    pub paint_transform: [[f32; 3]; 2],
    pub contours: Vec<Contour>,
    pub fill: Option<Fill>,
    pub stroke: Option<RoundStroke>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskKind {
    Alpha,
    Luminance,
}
#[derive(Debug, Clone)]
pub enum DrawNode {
    Path(PathDraw),
    Group {
        children: Vec<usize>,
        opacity: f32,
    },
    /// Matte is an input reference. It is visible only if explicitly listed in
    /// roots/children. Luma is linear working-space Y times alpha, clamped to [0,1].
    Effect {
        source: usize,
        effect: crate::PixelEffect,
    },
    Masked {
        source: usize,
        matte: usize,
        kind: MaskKind,
    },
}
#[derive(Debug, Clone)]
pub struct DrawScene {
    pub nodes: Vec<DrawNode>,
    pub roots: Vec<usize>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputAlpha {
    Straight,
    Premultiplied,
}
/// Explicit external association space. No tone mapping, gamut clipping, or
/// implicit alpha discard. PQ/HLG are outside this SDR boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputTransform {
    pub space: InputSpace,
    pub alpha: OutputAlpha,
}

impl DrawScene {
    pub fn validate(&self) -> Result<(), GpuError> {
        if self.nodes.len() > 1024 || self.roots.len() > 1024 {
            return Err(GpuError::UnsupportedFeature("scene node budget exceeded"));
        }
        let mut edges = 0;
        for node in &self.nodes {
            match node {
                DrawNode::Path(path) => {
                    for g in path.fill_gradient.iter().chain(&path.stroke_gradient) {
                        g.validate()?;
                    }
                    if path
                        .paint_transform
                        .iter()
                        .flatten()
                        .any(|v| !v.is_finite())
                    {
                        return Err(GpuError::InvalidInput("invalid paint mapping"));
                    }
                    for c in &path.contours {
                        if c.points.len() < 2
                            || c.points
                                .iter()
                                .flatten()
                                .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
                        {
                            return Err(GpuError::InvalidInput(
                                "invalid contour coordinates or length",
                            ));
                        }
                        edges += c.points.len();
                    }
                    for paint in path
                        .fill
                        .map(|f| f.paint)
                        .into_iter()
                        .chain(path.stroke.map(|s| s.paint))
                    {
                        color::validate_straight(paint.rgba, paint.space)?;
                    }
                    if let Some(s) = path.stroke
                        && (!s.width.is_finite()
                            || !(0.0..=1_000_000.0).contains(&s.width)
                            || !s.miter_limit.is_finite()
                            || s.miter_limit < 1.0)
                    {
                        return Err(GpuError::InvalidInput(
                            "invalid stroke width or miter limit",
                        ));
                    }
                }
                DrawNode::Group { children, opacity } => {
                    if children.len() > 1024 {
                        return Err(GpuError::UnsupportedFeature("group child budget exceeded"));
                    }
                    if !opacity.is_finite() || !(0.0..=1.0).contains(opacity) {
                        return Err(GpuError::InvalidInput("invalid group opacity"));
                    }
                }
                DrawNode::Effect { effect, .. } => effect
                    .validate()
                    .map_err(|_| GpuError::InvalidInput("invalid effect parameters"))?,
                DrawNode::Masked { .. } => {}
            }
        }
        if edges > 65536 {
            return Err(GpuError::UnsupportedFeature("scene edge budget exceeded"));
        }
        // Validate all nodes, including unreachable inputs. Memoized heights keep
        // shared DAGs linear and enforce the depth bound on every reference path.
        fn visit(
            scene: &DrawScene,
            id: usize,
            states: &mut [u8],
            heights: &mut [usize],
            depth: usize,
        ) -> Result<usize, GpuError> {
            if id >= scene.nodes.len() {
                return Err(GpuError::InvalidInput("missing scene input reference"));
            }
            if depth > 32 {
                return Err(GpuError::UnsupportedFeature("scene nesting exceeds 32"));
            }
            if states[id] == 1 {
                return Err(GpuError::InvalidInput("cyclic scene input reference"));
            }
            if states[id] == 2 {
                return Ok(heights[id]);
            }
            states[id] = 1;
            let inputs = inputs(&scene.nodes[id]);
            let mut height = 1;
            for child in inputs {
                height = height.max(visit(scene, child, states, heights, depth + 1)? + 1);
            }
            if height > 32 {
                return Err(GpuError::UnsupportedFeature("scene nesting exceeds 32"));
            }
            states[id] = 2;
            heights[id] = height;
            Ok(height)
        }
        let mut states = vec![0; self.nodes.len()];
        let mut heights = vec![0; self.nodes.len()];
        for id in 0..self.nodes.len() {
            visit(self, id, &mut states, &mut heights, 1)?;
        }
        for &id in &self.roots {
            if id >= self.nodes.len() {
                return Err(GpuError::InvalidInput("missing scene root"));
            }
        }
        Ok(())
    }
}
pub(crate) fn inputs(node: &DrawNode) -> Vec<usize> {
    match node {
        DrawNode::Path(_) => vec![],
        DrawNode::Group { children, .. } => children.clone(),
        DrawNode::Effect { source, .. } => vec![*source],
        DrawNode::Masked { source, matte, .. } => vec![*source, *matte],
    }
}
/// Edges retain whether the closing edge participates in stroke.
pub(crate) fn edges(path: &PathDraw) -> Vec<([f32; 2], [f32; 2], bool)> {
    let mut edges = Vec::new();
    for c in &path.contours {
        for pair in c.points.windows(2) {
            edges.push((pair[0], pair[1], true));
        }
        edges.push((*c.points.last().unwrap(), c.points[0], c.closed));
    }
    edges
}
#[derive(Debug, Clone, Copy)]
pub(crate) enum StrokePrimitive {
    Triangle([[f32; 2]; 3]),
    Circle([f32; 2], f32),
}
fn sub(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}
fn add(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] + b[0], a[1] + b[1]]
}
fn mul(a: [f32; 2], s: f32) -> [f32; 2] {
    a.map(|v| v * s)
}
fn cross(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[1] - a[1] * b[0]
}
fn dot(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}
fn unit(a: [f32; 2]) -> [f32; 2] {
    mul(a, 1.0 / dot(a, a).sqrt())
}
pub(crate) fn stroke_primitives(path: &PathDraw) -> Vec<StrokePrimitive> {
    let mut out = Vec::new();
    let Some(s) = path.stroke else { return out };
    let r = s.width / 2.0;
    if r == 0.0 {
        return out;
    }
    for contour in &path.contours {
        // Consecutive coincident points have no direction and do not create joins.
        let mut points = Vec::new();
        for &p in &contour.points {
            if points.last() != Some(&p) {
                points.push(p);
            }
        }
        if contour.closed && points.first() == points.last() {
            points.pop();
        }
        let n = points.len();
        if n < 2 {
            if s.cap == StrokeCap::Round && !contour.closed && n == 1 {
                out.push(StrokePrimitive::Circle(points[0], r));
            }
            continue;
        }
        let segments = if contour.closed { n } else { n - 1 };
        for i in 0..segments {
            let mut a = points[i];
            let mut b = points[(i + 1) % n];
            let d = unit(sub(b, a));
            let normal = mul([-d[1], d[0]], r);
            if !contour.closed && s.cap == StrokeCap::Square {
                if i == 0 {
                    a = sub(a, mul(d, r));
                }
                if i == segments - 1 {
                    b = add(b, mul(d, r));
                }
            }
            let q = [
                add(a, normal),
                sub(a, normal),
                sub(b, normal),
                add(b, normal),
            ];
            out.push(StrokePrimitive::Triangle([q[0], q[1], q[2]]));
            out.push(StrokePrimitive::Triangle([q[0], q[2], q[3]]));
        }
        for i in 0..n {
            if !contour.closed && (i == 0 || i == n - 1) {
                if s.cap == StrokeCap::Round {
                    out.push(StrokePrimitive::Circle(points[i], r));
                }
                continue;
            }
            let v = points[i];
            let d = unit(sub(v, points[(i + n - 1) % n]));
            let e = unit(sub(points[(i + 1) % n], v));
            if s.join == StrokeJoin::Round {
                out.push(StrokePrimitive::Circle(v, r));
                continue;
            }
            let turn = cross(d, e);
            if turn == 0.0 {
                continue;
            }
            let sign = if turn > 0.0 { -1.0 } else { 1.0 };
            let a = add(v, mul([-d[1], d[0]], r * sign));
            let b = add(v, mul([-e[1], e[0]], r * sign));
            out.push(StrokePrimitive::Triangle([v, a, b]));
            if s.join == StrokeJoin::Miter {
                let m = add(a, mul(d, cross(sub(b, a), e) / turn));
                if (m[0] - v[0]).hypot(m[1] - v[1]) / r <= s.miter_limit {
                    out.push(StrokePrimitive::Triangle([a, m, b]));
                }
            }
        }
    }
    out
}
pub(crate) fn primitive_hit(p: [f32; 2], primitive: &StrokePrimitive) -> bool {
    match *primitive {
        StrokePrimitive::Circle(c, r) => dot(sub(p, c), sub(p, c)) <= r * r,
        StrokePrimitive::Triangle([a, b, c]) => {
            if cross(sub(b, a), sub(c, a)) == 0.0 {
                return false;
            }
            let x = cross(sub(b, a), sub(p, a));
            let y = cross(sub(c, b), sub(p, b));
            let z = cross(sub(a, c), sub(p, c));
            (x >= 0.0 && y >= 0.0 && z >= 0.0) || (x <= 0.0 && y <= 0.0 && z <= 0.0)
        }
    }
}
fn fill_hit(point: [f32; 2], edges: &[([f32; 2], [f32; 2], bool)], rule: FillRule) -> bool {
    let mut winding = 0;
    for &(a, b, _) in edges {
        let c = cross(sub(b, a), sub(point, a));
        if a[1] <= point[1] && b[1] > point[1] && c > 0.0 {
            winding += 1;
        }
        if b[1] <= point[1] && a[1] > point[1] && c < 0.0 {
            winding -= 1;
        }
    }
    match rule {
        FillRule::Nonzero => winding != 0,
        FillRule::Evenodd => winding % 2 != 0,
    }
}
impl GradientPaint {
    pub fn validate(&self) -> Result<(), GpuError> {
        let valid = match self.geometry {
            GradientGeometry::Linear { start, end } => {
                start != end
                    && start
                        .into_iter()
                        .chain(end)
                        .all(|v| v.is_finite() && v.abs() <= 1_000_000.0)
                    && dot(sub(end, start), sub(end, start)) > 0.0
            }
            GradientGeometry::Radial { center, radius } => {
                center
                    .into_iter()
                    .all(|v| v.is_finite() && v.abs() <= 1_000_000.0)
                    && radius.is_finite()
                    && radius > 0.0
            }
        };
        if !valid || !(2..=256).contains(&self.stops.len()) {
            return Err(GpuError::InvalidInput(
                "invalid gradient geometry or stop count",
            ));
        }
        let mut previous = -1.0;
        for s in &self.stops {
            if !s.offset.is_finite() || !(0.0..=1.0).contains(&s.offset) || s.offset < previous {
                return Err(GpuError::InvalidInput("invalid gradient stop offset"));
            }
            previous = s.offset;
            color::validate_straight(s.paint.rgba, s.paint.space)?;
        }
        Ok(())
    }
    /// Pad and right-continuous equal offsets: last stop at that offset wins.
    pub fn sample(&self, p: [f32; 2], working: WorkingSpace) -> [f32; 4] {
        let t = match self.geometry {
            GradientGeometry::Linear { start, end } => {
                let d = sub(end, start);
                dot(sub(p, start), d) / dot(d, d)
            }
            GradientGeometry::Radial { center, radius } => {
                dot(sub(p, center), sub(p, center)).sqrt() / radius
            }
        };
        let convert = |s: &GradientStop| color::to_working(s.paint.rgba, s.paint.space, working);
        let mut previous = &self.stops[0];
        if t < previous.offset {
            return convert(previous);
        }
        for stop in &self.stops[1..] {
            if t < stop.offset {
                let f = (t - previous.offset) / (stop.offset - previous.offset);
                let a = convert(previous);
                let b = convert(stop);
                return std::array::from_fn(|i| a[i] + (b[i] - a[i]) * f);
            }
            previous = stop;
        }
        convert(previous)
    }
}
pub(crate) fn luma_weights(working: WorkingSpace) -> [f32; 3] {
    match working {
        WorkingSpace::LinearRec709 => [0.2126, 0.7152, 0.0722],
        WorkingSpace::LinearRec2020 => [0.2627, 0.6780, 0.0593],
    }
}
pub(crate) fn check_scene_budget(
    size: RenderSize,
    scene: &DrawScene,
    pixel_bytes: u64,
) -> Result<(), GpuError> {
    let surfaces = scene.nodes.len()
        + scene.roots.len()
        + scene
            .nodes
            .iter()
            .map(|n| match n {
                DrawNode::Group { children, .. } => children.len() + 1,
                DrawNode::Effect { .. } => 3,
                _ => 0,
            })
            .sum::<usize>()
        + 3;
    let bytes = u64::from(size.output_resolution[0])
        .checked_mul(u64::from(size.output_resolution[1]))
        .and_then(|b| b.checked_mul(pixel_bytes))
        .and_then(|b| b.checked_mul(surfaces as u64));
    if bytes.is_none_or(|b| b > 512 * 1024 * 1024) {
        return Err(GpuError::UnsupportedFeature(
            "scene surface budget exceeds 512 MiB",
        ));
    }
    Ok(())
}
/// Validate values at an RGBA16F surface boundary before implementation-defined
/// overflow conversion (some backends saturate instead of producing infinity).
pub(crate) fn validate_surface_pixels(pixels: &[[f32; 4]]) -> Result<(), GpuError> {
    if pixels.iter().any(|p| {
        p.iter().any(|v| !v.is_finite())
            || !(0.0..=1.0).contains(&p[3])
            || p[..3].iter().any(|v| v.abs() > 65504.0)
    }) {
        return Err(GpuError::InvalidInput(
            "RGBA16F surface value outside finite representable range",
        ));
    }
    Ok(())
}
pub(crate) fn raster_path_reference(
    size: RenderSize,
    path: &PathDraw,
    working: WorkingSpace,
) -> Result<Vec<[f32; 4]>, GpuError> {
    let [w, h] = size.output_resolution;
    let mut pixels = vec![[0.0; 4]; (w as usize) * (h as usize)];
    let e = edges(path);
    let scale = size.pixel_scale();
    let primitives = stroke_primitives(path);
    let fill_solid = path
        .fill
        .map(|f| color::to_working(f.paint.rgba, f.paint.space, working))
        .unwrap_or([0.0; 4]);
    let stroke_solid = path
        .stroke
        .map(|s| color::to_working(s.paint.rgba, s.paint.space, working))
        .unwrap_or([0.0; 4]);
    let paint = |solid: [f32; 4], g: &Option<GradientPaint>, p: [f32; 2]| {
        g.as_ref().map_or(solid, |g| {
            let local = path
                .paint_transform
                .map(|row| row[0] * p[0] + row[1] * p[1] + row[2]);
            g.sample(local, working)
        })
    };
    for y in 0..h {
        for x in 0..w {
            let mut fill = [0.0; 4];
            let mut stroke = [0.0; 4];
            for sy in 0..4 {
                for sx in 0..4 {
                    let p = [
                        (x as f32 + (sx as f32 + 0.5) / 4.0) * scale[0],
                        (y as f32 + (sy as f32 + 0.5) / 4.0) * scale[1],
                    ];
                    if let Some(f) = path.fill
                        && fill_hit(p, &e, f.rule)
                    {
                        let c = paint(fill_solid, &path.fill_gradient, p);
                        for i in 0..4 {
                            fill[i] += c[i] / 16.0;
                        }
                    }
                    if path.stroke.is_some() && primitives.iter().any(|v| primitive_hit(p, v)) {
                        let c = paint(stroke_solid, &path.stroke_gradient, p);
                        for i in 0..4 {
                            stroke[i] += c[i] / 16.0;
                        }
                    }
                }
            }
            pixels[(y * w + x) as usize] = color::source_over(stroke, fill);
        }
    }
    validate_surface_pixels(&pixels)?;
    Ok(pixels)
}

/// CPU oracle: same 4x4 point sampling as WGSL. Coverage/groups use float32;
/// effects explicitly round their surface boundaries to binary16 RNE.
/// Offsets are ((sx+0.5)/4,(sy+0.5)/4). Coverage is the fraction of hits.
pub fn render_scene_reference(
    size: RenderSize,
    scene: &DrawScene,
    working: WorkingSpace,
) -> Result<Vec<[f32; 4]>, GpuError> {
    render_scene_reference_with_raster(size, scene, working, &mut |_, path| {
        raster_path_reference(size, path, working)
    })
}

type RasterResolver<'a> = dyn FnMut(usize, &PathDraw) -> Result<Vec<[f32; 4]>, GpuError> + 'a;
pub(crate) fn render_scene_reference_with_raster(
    size: RenderSize,
    scene: &DrawScene,
    working: WorkingSpace,
    raster: &mut RasterResolver<'_>,
) -> Result<Vec<[f32; 4]>, GpuError> {
    render_scene_reference_with_resolvers(size, scene, working, raster, &mut |_, source, effect| {
        crate::effect::apply_reference(source, size.output_resolution, effect, working)
    })
}
type EffectResolver<'a> =
    dyn FnMut(usize, &[[f32; 4]], &crate::PixelEffect) -> Result<Vec<[f32; 4]>, GpuError> + 'a;
pub(crate) fn render_scene_reference_with_resolvers(
    size: RenderSize,
    scene: &DrawScene,
    working: WorkingSpace,
    raster: &mut RasterResolver<'_>,
    effects: &mut EffectResolver<'_>,
) -> Result<Vec<[f32; 4]>, GpuError> {
    size.validate()?;
    scene.validate()?;
    check_scene_budget(size, scene, 16)?;
    fn node(
        size: RenderSize,
        scene: &DrawScene,
        id: usize,
        working: WorkingSpace,
        cache: &mut [Option<Vec<[f32; 4]>>],
        raster: &mut RasterResolver<'_>,
        effects: &mut EffectResolver<'_>,
    ) -> Result<Vec<[f32; 4]>, GpuError> {
        if let Some(p) = &cache[id] {
            return Ok(p.clone());
        }
        let [w, h] = size.output_resolution;
        let mut pixels = vec![[0.0; 4]; (w as usize) * (h as usize)];
        match &scene.nodes[id] {
            DrawNode::Path(path) => {
                pixels = raster(id, path)?;
            }
            DrawNode::Group { children, opacity } => {
                for &child in children {
                    let src = node(size, scene, child, working, cache, raster, effects)?;
                    for (d, s) in pixels.iter_mut().zip(src) {
                        *d = color::source_over(s, *d);
                    }
                    validate_surface_pixels(&pixels)?;
                }
                for p in &mut pixels {
                    *p = p.map(|v| v * opacity);
                }
            }
            DrawNode::Effect { source, effect } => {
                let source = node(size, scene, *source, working, cache, raster, effects)?;
                pixels = effects(id, &source, effect)?;
            }
            DrawNode::Masked {
                source,
                matte,
                kind,
            } => {
                pixels = node(size, scene, *source, working, cache, raster, effects)?;
                let mask = node(size, scene, *matte, working, cache, raster, effects)?;
                let weights = luma_weights(working);
                for (p, m) in pixels.iter_mut().zip(mask) {
                    let coverage = match kind {
                        MaskKind::Alpha => m[3],
                        MaskKind::Luminance => {
                            (m[0] * weights[0] + m[1] * weights[1] + m[2] * weights[2])
                                .clamp(0.0, 1.0)
                        }
                    };
                    *p = p.map(|v| v * coverage);
                }
            }
        }
        validate_surface_pixels(&pixels)?;
        cache[id] = Some(pixels.clone());
        Ok(pixels)
    }
    let mut cache = vec![None; scene.nodes.len()];
    let mut pixels =
        vec![[0.0; 4]; pixel_count(size.output_resolution[0], size.output_resolution[1])?];
    for &id in &scene.roots {
        let src = node(size, scene, id, working, &mut cache, raster, effects)?;
        for (d, s) in pixels.iter_mut().zip(src) {
            *d = color::source_over(s, *d);
        }
        validate_surface_pixels(&pixels)?;
    }
    Ok(pixels)
}
/// CPU reference for the explicit output boundary; alpha is unchanged.
pub fn convert_output_reference(
    p: [f32; 4],
    working: WorkingSpace,
    transform: OutputTransform,
) -> Result<[f32; 4], GpuError> {
    if p.iter().any(|v| !v.is_finite())
        || !(0.0..=1.0).contains(&p[3])
        || (p[3] == 0.0 && p[..3].iter().any(|v| *v != 0.0))
    {
        return Err(GpuError::InvalidInput("invalid internal output pixel"));
    }
    let straight = color::unpremultiply_external(p);
    let to = if transform.space == InputSpace::LinearRec2020 {
        WorkingSpace::LinearRec2020
    } else {
        WorkingSpace::LinearRec709
    };
    let mut rgb = color::convert_primaries([straight[0], straight[1], straight[2]], working, to);
    if transform.space == InputSpace::Srgb {
        rgb = rgb.map(color::srgb_encode);
    }
    let result = [rgb[0], rgb[1], rgb[2], p[3]];
    let result = if transform.alpha == OutputAlpha::Premultiplied {
        color::premultiply(result)
    } else {
        result
    };
    validate_surface_pixels(&[result])?;
    Ok(result)
}

#[cfg(test)]
mod stroke_tests {
    use super::*;
    fn path(points: Vec<[f32; 2]>, join: StrokeJoin, cap: StrokeCap, limit: f32) -> PathDraw {
        PathDraw {
            fill_gradient: None,
            stroke_gradient: None,
            paint_transform: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            contours: vec![Contour {
                points,
                closed: false,
            }],
            fill: None,
            stroke: Some(RoundStroke {
                paint: Paint {
                    rgba: [1.0; 4],
                    space: InputSpace::LinearRec709,
                },
                width: 2.0,
                join,
                cap,
                miter_limit: limit,
            }),
        }
    }
    fn hit(path: &PathDraw, p: [f32; 2]) -> bool {
        stroke_primitives(path).iter().any(|v| primitive_hit(p, v))
    }
    #[test]
    fn cpu_caps_have_analytic_extent_and_corner_definition() {
        let points = vec![[2.0, 2.0], [6.0, 2.0]];
        for cap in [StrokeCap::Butt, StrokeCap::Square, StrokeCap::Round] {
            let p = path(points.clone(), StrokeJoin::Miter, cap, 4.0);
            assert!(hit(&p, [4.0, 3.0]));
            assert!(!hit(&p, [4.0, 3.01]));
            assert_eq!(hit(&p, [1.5, 2.0]), cap != StrokeCap::Butt);
            assert_eq!(hit(&p, [6.5, 2.9]), cap == StrokeCap::Square);
            assert!(!hit(&p, [7.01, 2.0]));
            let mut closed = p.clone();
            closed.contours[0].closed = true;
            assert!(!hit(&closed, [1.5, 2.0]));
        }
    }
    #[test]
    fn cpu_joins_miter_limit_and_reversed_winding_are_analytic() {
        for points in [
            vec![[2.0, 4.0], [4.0, 4.0], [4.0, 6.0]],
            vec![[4.0, 6.0], [4.0, 4.0], [2.0, 4.0]],
        ] {
            for join in [StrokeJoin::Miter, StrokeJoin::Bevel, StrokeJoin::Round] {
                let p = path(points.clone(), join, StrokeCap::Butt, 4.0);
                assert_eq!(hit(&p, [4.9, 3.1]), join == StrokeJoin::Miter);
                assert_eq!(hit(&p, [4.6, 3.4]), join != StrokeJoin::Bevel);
                assert!(hit(&p, [4.2, 3.8]));
                if join == StrokeJoin::Miter {
                    let fallback = path(points.clone(), join, StrokeCap::Butt, 1.0);
                    let bevel = path(points.clone(), StrokeJoin::Bevel, StrokeCap::Butt, 4.0);
                    for y in 0..80 {
                        for x in 0..80 {
                            let q = [x as f32 / 10.0, y as f32 / 10.0];
                            assert_eq!(hit(&fallback, q), hit(&bevel, q));
                        }
                    }
                    assert!(!hit(
                        &path(points.clone(), join, StrokeCap::Butt, 1.4),
                        [4.9, 3.1]
                    ));
                    assert!(hit(
                        &path(points.clone(), join, StrokeCap::Butt, 2.0_f32.sqrt()),
                        [4.9, 3.1]
                    ));
                }
            }
        }
    }
    #[test]
    fn cpu_degenerate_contours_and_zero_width_do_not_create_spurious_regions() {
        let mut p = path(
            vec![[2.0, 2.0], [2.0, 2.0]],
            StrokeJoin::Miter,
            StrokeCap::Round,
            4.0,
        );
        assert!(hit(&p, [2.5, 2.0]));
        p.stroke.as_mut().unwrap().cap = StrokeCap::Butt;
        assert!(!hit(&p, [2.0, 2.0]));
        p.stroke.as_mut().unwrap().width = 0.0;
        assert!(stroke_primitives(&p).is_empty());
    }
}
