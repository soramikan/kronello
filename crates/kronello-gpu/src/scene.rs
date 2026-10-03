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
/// Basic centered stroke: union of segment capsules, round caps and round joins.
/// Other cap/join styles must be expanded upstream; they are not inferred here.
#[derive(Debug, Clone, Copy)]
pub struct RoundStroke {
    pub paint: Paint,
    pub width: f32,
}
/// Flattened, transformed design-space geometry. Fill implicitly closes contours;
/// stroke closes only contours marked closed. Fill is drawn before stroke.
#[derive(Debug, Clone)]
pub struct PathDraw {
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
                        && (!s.width.is_finite() || !(0.0..=1_000_000.0).contains(&s.width))
                    {
                        return Err(GpuError::InvalidInput("invalid stroke width"));
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
fn hit(
    point: [f32; 2],
    edges: &[([f32; 2], [f32; 2], bool)],
    rule: FillRule,
    radius: f32,
) -> (bool, bool) {
    let mut winding = 0i32;
    let mut stroke = false;
    for &(a, b, stroked) in edges {
        let d = [b[0] - a[0], b[1] - a[1]];
        let q = [point[0] - a[0], point[1] - a[1]];
        let cross = d[0] * q[1] - d[1] * q[0];
        if a[1] <= point[1] && b[1] > point[1] && cross > 0.0 {
            winding += 1;
        }
        if b[1] <= point[1] && a[1] > point[1] && cross < 0.0 {
            winding -= 1;
        }
        if stroked && radius > 0.0 {
            let length = d[0] * d[0] + d[1] * d[1];
            let t = if length > 0.0 {
                ((q[0] * d[0] + q[1] * d[1]) / length).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let v = [q[0] - t * d[0], q[1] - t * d[1]];
            stroke |= v[0] * v[0] + v[1] * v[1] <= radius * radius;
        }
    }
    (
        match rule {
            FillRule::Nonzero => winding != 0,
            FillRule::Evenodd => winding % 2 != 0,
        },
        stroke,
    )
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
/// CPU oracle: same 4x4 point-sampling contract as WGSL, no binary16 rounding.
/// Offsets are ((sx+0.5)/4,(sy+0.5)/4). Coverage is the fraction of hits.
pub fn render_scene_reference(
    size: RenderSize,
    scene: &DrawScene,
    working: WorkingSpace,
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
    ) -> Result<Vec<[f32; 4]>, GpuError> {
        if let Some(p) = &cache[id] {
            return Ok(p.clone());
        }
        let [w, h] = size.output_resolution;
        let mut pixels = vec![[0.0; 4]; (w as usize) * (h as usize)];
        match &scene.nodes[id] {
            DrawNode::Path(path) => {
                let e = edges(path);
                let scale = size.pixel_scale();
                let fill = path
                    .fill
                    .map(|f| color::to_working(f.paint.rgba, f.paint.space, working))
                    .unwrap_or([0.0; 4]);
                let stroke = path
                    .stroke
                    .map(|s| color::to_working(s.paint.rgba, s.paint.space, working))
                    .unwrap_or([0.0; 4]);
                for y in 0..h {
                    for x in 0..w {
                        let mut fc = 0;
                        let mut sc = 0;
                        for sy in 0..4 {
                            for sx in 0..4 {
                                let p = [
                                    (x as f32 + (sx as f32 + 0.5) / 4.0) * scale[0],
                                    (y as f32 + (sy as f32 + 0.5) / 4.0) * scale[1],
                                ];
                                let (f, s) = hit(
                                    p,
                                    &e,
                                    path.fill.map_or(FillRule::Nonzero, |f| f.rule),
                                    path.stroke.map_or(0.0, |s| s.width / 2.0),
                                );
                                fc += u32::from(f);
                                sc += u32::from(s);
                            }
                        }
                        pixels[(y * w + x) as usize] = color::source_over(
                            stroke.map(|v| v * sc as f32 / 16.0),
                            fill.map(|v| v * fc as f32 / 16.0),
                        );
                    }
                }
            }
            DrawNode::Group { children, opacity } => {
                for &child in children {
                    let src = node(size, scene, child, working, cache)?;
                    for (d, s) in pixels.iter_mut().zip(src) {
                        *d = color::source_over(s, *d);
                    }
                    validate_surface_pixels(&pixels)?;
                }
                for p in &mut pixels {
                    *p = p.map(|v| v * opacity);
                }
            }
            DrawNode::Masked {
                source,
                matte,
                kind,
            } => {
                pixels = node(size, scene, *source, working, cache)?;
                let mask = node(size, scene, *matte, working, cache)?;
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
        let src = node(size, scene, id, working, &mut cache)?;
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
