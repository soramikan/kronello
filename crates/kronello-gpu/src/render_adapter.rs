//! Adapter for the backend-free Render DAG contract. Compilation/layout remain
//! in kronello-render. CPU oracle selection is explicit; GPU never falls back.
use kronello_render::{BackendFrame, DagNode, MatteKind, RenderBackend, RenderDag, RenderError};

use crate::{
    Contour, DrawNode, DrawScene, Fill, FillRule, GpuContext, GpuError, InputSpace, OutputAlpha,
    OutputTransform, Paint, PathDraw, RenderSize, RoundStroke, WorkingSpace,
    convert_output_reference, render_scene_reference,
};

fn error(e: GpuError) -> RenderError {
    let code = match e {
        GpuError::AdapterUnavailable(_) => "ADAPTER_UNAVAILABLE",
        GpuError::DeviceUnavailable(_) => "DEVICE_UNAVAILABLE",
        GpuError::UnsupportedFeature(_) => "UNSUPPORTED_FEATURE",
        GpuError::InvalidInput(_) => "INVALID_INPUT",
        GpuError::Readback(_) => "READBACK_FAILED",
    };
    RenderError::Backend {
        code,
        message: e.to_string(),
    }
}
fn space(space: kronello_model::ColorSpace) -> WorkingSpace {
    match space {
        kronello_model::ColorSpace::LinearRec2020 => WorkingSpace::LinearRec2020,
        _ => WorkingSpace::LinearRec709,
    }
}
fn paint(color: kronello_model::Color) -> Paint {
    let c = color.components();
    Paint {
        rgba: [
            c.r.get() as f32,
            c.g.get() as f32,
            c.b.get() as f32,
            c.alpha.get() as f32,
        ],
        space: match color.space() {
            kronello_model::ColorSpace::Srgb => InputSpace::Srgb,
            kronello_model::ColorSpace::LinearRec709 => InputSpace::LinearRec709,
            kronello_model::ColorSpace::LinearRec2020 => InputSpace::LinearRec2020,
        },
    }
}
fn lower(dag: &RenderDag) -> Result<(RenderSize, DrawScene, WorkingSpace), RenderError> {
    let mut scene = DrawScene {
        nodes: vec![],
        roots: vec![],
    };
    let mut ids = vec![None; dag.nodes().len()];
    let image = |ids: &[Option<usize>], id: usize| {
        ids.get(id)
            .copied()
            .flatten()
            .ok_or_else(|| RenderError::InvalidInput("DAG expects an image input".into()))
    };
    for (index, node) in dag.nodes().iter().enumerate() {
        let draw = match node {
            DagNode::Geometry { .. } | DagNode::TextLayout { .. } => continue,
            DagNode::CoverageDraw { path, .. } => DrawNode::Path(PathDraw {
                contours: path
                    .contours
                    .subpaths
                    .iter()
                    .map(|c| Contour {
                        points: c.points.iter().map(|p| p.map(|v| v as f32)).collect(),
                        closed: c.closed,
                    })
                    .collect(),
                fill: path.fill.map(|(color, rule)| Fill {
                    paint: paint(color),
                    rule: match rule {
                        kronello_model::FillRule::Nonzero => FillRule::Nonzero,
                        kronello_model::FillRule::Evenodd => FillRule::Evenodd,
                    },
                }),
                stroke: path.stroke.map(|(color, width)| RoundStroke {
                    paint: paint(color),
                    width: width as f32,
                }),
            }),
            DagNode::IsolatedComposite { children, opacity } => DrawNode::Group {
                children: children
                    .iter()
                    .map(|id| image(&ids, *id))
                    .collect::<Result<_, _>>()?,
                opacity: *opacity as f32,
            },
            DagNode::Mask {
                source,
                matte,
                kind,
            } => DrawNode::Masked {
                source: image(&ids, *source)?,
                matte: image(&ids, *matte)?,
                kind: match kind {
                    MatteKind::Alpha => crate::MaskKind::Alpha,
                    MatteKind::Luminance => crate::MaskKind::Luminance,
                },
            },
            DagNode::OutputTransform { source, .. } => {
                scene.roots = vec![image(&ids, *source)?];
                continue;
            }
        };
        ids[index] = Some(scene.nodes.len());
        scene.nodes.push(draw);
    }
    scene.validate().map_err(error)?;
    let pixels = dag.region().pixels;
    let size = RenderSize {
        design_extent: pixels.map(|v| v as f32),
        output_resolution: pixels,
    };
    let working = space(dag.working_space());
    Ok((size, scene, working))
}
const DISPLAY: OutputTransform = OutputTransform {
    space: InputSpace::Srgb,
    alpha: OutputAlpha::Straight,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct CpuReferenceBackend;
impl RenderBackend for CpuReferenceBackend {
    fn name(&self) -> &str {
        "cpu_reference_float32"
    }
    fn execute(&self, dag: &RenderDag) -> Result<BackendFrame, RenderError> {
        let (size, scene, working) = lower(dag)?;
        let linear = render_scene_reference(size, &scene, working).map_err(error)?;
        let display = linear
            .iter()
            .map(|p| convert_output_reference(*p, working, DISPLAY).map_err(error))
            .collect::<Result<_, _>>()?;
        Ok(BackendFrame { linear, display })
    }
}
impl RenderBackend for GpuContext {
    fn name(&self) -> &str {
        "wgpu_rgba16f"
    }
    fn execute(&self, dag: &RenderDag) -> Result<BackendFrame, RenderError> {
        let (size, scene, working) = lower(dag)?;
        let linear = self
            .render_scene(size, &scene, working)
            .map_err(error)?
            .pixels;
        // External output conversion stays on GPU. Each call reports its own
        // explicit image/status readback; no CPU color transform is substituted.
        let display = self
            .render_scene_output(size, &scene, working, DISPLAY)
            .map_err(error)?
            .pixels;
        Ok(BackendFrame { linear, display })
    }
}
