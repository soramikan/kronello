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
            DagNode::VideoDraw { .. } => {
                return Err(RenderError::UnsupportedFeature(
                    "video requires explicit media backend".into(),
                ));
            }
            DagNode::RasterInput { pixels } => DrawNode::Raster(pixels.clone()),
            DagNode::Geometry { .. } | DagNode::TextLayout { .. } => continue,
            DagNode::CoverageDraw { path, .. } => DrawNode::Path(PathDraw {
                stroke_geometry: path.stroke_geometry.as_ref().map(|g| {
                    crate::LocalStrokeGeometry {
                        version: g.version.clone(),
                        contours: g
                            .contours
                            .subpaths
                            .iter()
                            .map(|c| Contour {
                                points: c.points.iter().map(|p| p.map(|x| x as f32)).collect(),
                                closed: c.closed,
                            })
                            .collect(),
                        output_to_local: g.output_to_local.map(|r| r.map(|x| x as f32)),
                        alignment: g.alignment,
                        fill_rule: match g.fill_rule {
                            kronello_model::FillRule::Nonzero => FillRule::Nonzero,
                            kronello_model::FillRule::Evenodd => FillRule::Evenodd,
                        },
                        dash_array: g.dash_array.clone(),
                        dash_offset: g.dash_offset,
                    }
                }),
                fill_gradient: path.fill_gradient.as_deref().map(gradient).map(Box::new),
                stroke_gradient: path.stroke_gradient.as_deref().map(gradient).map(Box::new),
                paint_transform: path.paint_transform.map(|r| r.map(|v| v as f32)),
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
                stroke: path
                    .stroke
                    .map(|(color, width, join, cap, miter_limit)| RoundStroke {
                        join,
                        cap,
                        miter_limit: miter_limit as f32,
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
            DagNode::Effect { source, effect } => DrawNode::Effect {
                source: image(&ids, *source)?,
                effect: effect.clone(),
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
    let pixels = dag.execution_region().pixels;
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

impl GpuContext {
    /// Uses the same DAG lowering as export without image readback.
    pub fn preview_texture(&self, dag: &RenderDag) -> Result<wgpu::Texture, RenderError> {
        let (size, scene, working) = lower(dag)?;
        self.render_scene_texture(
            size,
            &scene,
            working,
            OutputTransform {
                space: InputSpace::LinearRec709,
                alpha: OutputAlpha::Premultiplied,
            },
        )
        .map_err(error)
    }
}

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
        Ok(crop(dag, BackendFrame { linear, display }))
    }
    fn execute_with_cache(
        &self,
        dag: &RenderDag,
        cache: &mut kronello_render::RenderCache,
    ) -> Result<BackendFrame, RenderError> {
        let (size, scene, working) = lower(dag)?;
        // Keep the exact lowering order; DAG indices are never cache identities.
        let keys = kronello_render::RasterCacheKey::for_dag(dag, "cpu-reference-f32-v1")?;
        let keys: Vec<_> = dag
            .nodes()
            .iter()
            .zip(keys)
            .filter_map(|(node, key)| match node {
                DagNode::Geometry { .. }
                | DagNode::TextLayout { .. }
                | DagNode::OutputTransform { .. } => None,
                _ => Some(key),
            })
            .collect();
        let cache = std::cell::RefCell::new(cache);
        let linear = crate::scene::render_scene_reference_with_resolvers(
            size,
            &scene,
            working,
            &mut |id, path| {
                cache
                    .borrow_mut()
                    .rasterize(keys[id].expect("path key"), || {
                        crate::scene::raster_path_reference(size, path, working)
                    })
            },
            &mut |id, source, effect| {
                cache
                    .borrow_mut()
                    .rasterize(keys[id].expect("effect key"), || {
                        crate::effect::apply_reference(
                            source,
                            size.output_resolution,
                            effect,
                            working,
                        )
                    })
            },
        )
        .map_err(error)?;
        let display = linear
            .iter()
            .map(|p| convert_output_reference(*p, working, DISPLAY).map_err(error))
            .collect::<Result<_, _>>()?;
        Ok(crop(dag, BackendFrame { linear, display }))
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
        Ok(crop(dag, BackendFrame { linear, display }))
    }
}

fn gradient(g: &kronello_model::ResolvedGradient) -> crate::GradientPaint {
    let m = g.options.transform.map(|r| r.map(|v| v.get() as f32));
    let determinant = m[0][0] * m[1][1] - m[0][1] * m[1][0];
    let transform = [
        [
            m[1][1] / determinant,
            -m[0][1] / determinant,
            (m[0][1] * m[1][2] - m[1][1] * m[0][2]) / determinant,
        ],
        [
            -m[1][0] / determinant,
            m[0][0] / determinant,
            (m[1][0] * m[0][2] - m[0][0] * m[1][2]) / determinant,
        ],
    ];
    crate::GradientPaint {
        spread: g.options.spread,
        interpolation: g.options.interpolation,
        interpolation_version: g.options.interpolation_version,
        transform,
        geometry: match g.geometry {
            kronello_model::GradientGeometry::Linear { start, end } => {
                crate::GradientGeometry::Linear {
                    start: start.map(|v| v as f32),
                    end: end.map(|v| v as f32),
                }
            }
            kronello_model::GradientGeometry::Radial { center, radius } => {
                crate::GradientGeometry::Radial {
                    center: center.map(|v| v as f32),
                    radius: radius as f32,
                }
            }
            kronello_model::GradientGeometry::FocalRadial {
                center,
                radius,
                focal,
                focal_radius,
            } => crate::GradientGeometry::FocalRadial {
                center: center.map(|v| v as f32),
                radius: radius as f32,
                focal: focal.map(|v| v as f32),
                focal_radius: focal_radius as f32,
            },
            kronello_model::GradientGeometry::Conic {
                center,
                start_angle,
                sweep_angle,
            } => crate::GradientGeometry::Conic {
                center: center.map(|v| v as f32),
                start_angle: start_angle as f32,
                sweep_angle: sweep_angle as f32,
            },
        },
        stops: g
            .stops
            .iter()
            .map(|s| crate::GradientStop {
                offset: s.offset as f32,
                paint: paint(s.color),
            })
            .collect(),
    }
}

fn crop(dag: &RenderDag, frame: BackendFrame) -> BackendFrame {
    let [x, y] = dag.crop_origin();
    let [w, h] = dag.region().pixels;
    let stride = dag.execution_region().pixels[0] as usize;
    let extract = |pixels: Vec<[f32; 4]>| {
        (0..h as usize)
            .flat_map(|row| {
                pixels[(row + y) * stride + x..(row + y) * stride + x + w as usize]
                    .iter()
                    .copied()
            })
            .collect()
    };
    BackendFrame {
        linear: extract(frame.linear),
        display: extract(frame.display),
    }
}
