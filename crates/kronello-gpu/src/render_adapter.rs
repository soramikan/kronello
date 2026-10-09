//! Adapter for the backend-free Render DAG contract. Compilation/layout remain
//! in kronello-render. CPU oracle selection is explicit; GPU never falls back.
use kronello_render::{BackendFrame, DagNode, MatteKind, RenderBackend, RenderDag, RenderError};

use crate::{
    Contour, DrawNode, DrawScene, Fill, FillRule, GpuContext, GpuError, InputSpace, OutputAlpha,
    OutputTransform, Paint, PathDraw, RenderSize, RoundStroke, WorkingSpace,
    convert_output_reference, render_scene_reference,
};

fn error(e: GpuError) -> RenderError {
    RenderError::Backend {
        code: e.code(),
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
// Only the compiler's final synthetic output root may alias its single child.
// The final scene composite still performs normalization and numeric validation.
fn elided_output_root(dag: &RenderDag) -> Option<usize> {
    let root = dag.nodes().len().checked_sub(2)?;
    match (&dag.nodes()[root], dag.nodes().last()?) {
        (
            DagNode::IsolatedComposite { children, opacity },
            DagNode::OutputTransform { source, .. },
        ) if *source == root && children.len() == 1 && *opacity == 1.0 => Some(root),
        _ => None,
    }
}
fn lower(dag: &RenderDag) -> Result<(RenderSize, DrawScene, WorkingSpace), RenderError> {
    lower_impl(dag, Some(&std::collections::BTreeMap::new()))
}
fn lower_with_resident(
    dag: &RenderDag,
    resident: &std::collections::BTreeMap<usize, crate::ResidentImage>,
) -> Result<(RenderSize, DrawScene, WorkingSpace), RenderError> {
    lower_impl(dag, Some(resident))
}
/// Lowering used only for surface-count estimates before media resolution:
/// unresolved `VideoDraw` nodes count as one raster surface each.
fn lower_estimate(dag: &RenderDag) -> Result<(RenderSize, DrawScene, WorkingSpace), RenderError> {
    lower_impl(dag, None)
}
fn lower_impl(
    dag: &RenderDag,
    resident: Option<&std::collections::BTreeMap<usize, crate::ResidentImage>>,
) -> Result<(RenderSize, DrawScene, WorkingSpace), RenderError> {
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
    let elided_root = elided_output_root(dag);
    for (index, node) in dag.nodes().iter().enumerate() {
        if Some(index) == elided_root
            && let DagNode::IsolatedComposite { children, .. } = node
        {
            ids[index] = Some(image(&ids, children[0])?);
            continue;
        }
        let draw = match node {
            DagNode::VideoDraw { .. } => match resident {
                Some(resident) => {
                    DrawNode::GpuRaster(resident.get(&index).cloned().ok_or_else(|| {
                        RenderError::UnsupportedFeature(
                            "video requires explicit media backend".into(),
                        )
                    })?)
                }
                None => DrawNode::Raster(Vec::new()),
            },
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
            // Transition solid rectangles execute through the existing
            // supersampled path coverage stage (wipe matte, dip underlay).
            DagNode::SolidRect { color, rect } => DrawNode::Path(PathDraw {
                stroke_geometry: None,
                fill_gradient: None,
                stroke_gradient: None,
                paint_transform: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                contours: vec![Contour {
                    points: [
                        [rect[0], rect[1]],
                        [rect[2], rect[1]],
                        [rect[2], rect[3]],
                        [rect[0], rect[3]],
                    ]
                    .iter()
                    .map(|p| p.map(|v| v as f32))
                    .collect(),
                    closed: true,
                }],
                fill: Some(Fill {
                    paint: paint(*color),
                    rule: FillRule::Nonzero,
                }),
                stroke: None,
            }),
            DagNode::IsolatedComposite { children, opacity } => DrawNode::Group {
                children: children
                    .iter()
                    .map(|id| image(&ids, *id))
                    .collect::<Result<_, _>>()?,
                opacity: *opacity as f32,
            },
            DagNode::Blend {
                source,
                backdrop,
                mode,
            } => DrawNode::Blend {
                source: image(&ids, *source)?,
                backdrop: image(&ids, *backdrop)?,
                mode: *mode,
            },
            DagNode::Effect { source, effect } => DrawNode::Effect {
                source: image(&ids, *source)?,
                effect: effect.clone(),
            },
            // FX-008 (ADR-0137): the displacement binding is a real DAG input
            // lowered like every other image surface.
            DagNode::EffectMap {
                source,
                map,
                effect,
            } => DrawNode::EffectMap {
                source: image(&ids, *source)?,
                map: image(&ids, *map)?,
                effect: effect.clone(),
            },
            DagNode::Generate { effect, .. } => DrawNode::Generate {
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
                    MatteKind::AlphaInverted => crate::MaskKind::AlphaInverted,
                    MatteKind::LuminanceInverted => crate::MaskKind::LuminanceInverted,
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
    /// Explicit nonblocking execution for a caller that will retry a busy
    /// shared context. Normal frame requests serialize instead.
    pub fn try_execute(&self, dag: &RenderDag) -> Result<BackendFrame, RenderError> {
        let _scope = self.try_render_scope().map_err(error)?;
        self.execute(dag)
    }

    /// Execute the original DAG with device-bound video inputs. The only image
    /// readbacks are the requested final linear/display output boundaries.
    pub fn execute_resident_video(
        &self,
        dag: &RenderDag,
        resident: &std::collections::BTreeMap<usize, crate::ResidentImage>,
    ) -> Result<BackendFrame, RenderError> {
        Ok(self.execute_resident_video_with_stats(dag, resident)?.0)
    }
    pub fn execute_resident_video_with_stats(
        &self,
        dag: &RenderDag,
        resident: &std::collections::BTreeMap<usize, crate::ResidentImage>,
    ) -> Result<(BackendFrame, crate::TransferStats), RenderError> {
        let (size, scene, working) = lower_with_resident(dag, resident)?;
        let pair = self
            .render_scene_pair(size, &scene, working, DISPLAY)
            .map_err(error)?;
        Ok((
            crop(
                dag,
                BackendFrame {
                    linear: pair.linear,
                    display: pair.display,
                },
            ),
            pair.transfers,
        ))
    }
    /// Cached native inputs remain GPU-resident. Disk lookup/persistence is
    /// explicitly disabled because it would introduce intermediate CPU pixels.
    pub fn execute_resident_video_cached_with_stats(
        &self,
        dag: &RenderDag,
        resident: &std::collections::BTreeMap<usize, crate::ResidentImage>,
        inputs: &std::collections::BTreeMap<usize, kronello_render::RasterCacheKey>,
    ) -> Result<(BackendFrame, crate::TransferStats), RenderError> {
        let (size, scene, working) = lower_with_resident(dag, resident)?;
        let keys = kronello_render::RasterCacheKey::for_dag_with_inputs(
            dag,
            &self.cache_namespace().unwrap(),
            inputs,
        )?;
        let keys = map_scene_keys(dag, &keys);
        let pair = self
            .render_scene_pair_cached(size, &scene, working, Some(&keys), false, DISPLAY)
            .map_err(error)?;
        Ok((
            crop(
                dag,
                BackendFrame {
                    linear: pair.linear,
                    display: pair.display,
                },
            ),
            pair.transfers,
        ))
    }
    /// Conservative full-resolution surface count for a native preview DAG,
    /// before any texture is allocated. Native adapters shrink the requested
    /// region and rebuild when the estimate exceeds the surface budget.
    pub fn preview_surface_estimate(
        &self,
        dag: &RenderDag,
    ) -> Result<([u32; 2], u64), RenderError> {
        let (size, scene, _) = lower(dag)?;
        Ok((
            size.output_resolution,
            crate::scene::scene_surface_count(&scene) as u64,
        ))
    }
    /// Surface estimate on an unresolved DAG for the budget-fit loop; media
    /// resolution happens once at the accepted size, not per retry.
    pub fn preview_surface_estimate_unresolved(
        &self,
        dag: &RenderDag,
    ) -> Result<([u32; 2], u64), RenderError> {
        let (size, scene, _) = lower_estimate(dag)?;
        Ok((
            size.output_resolution,
            crate::scene::scene_surface_count(&scene) as u64,
        ))
    }
    /// `preview_texture_with_inputs` for DAGs whose `VideoDraw` nodes stay
    /// unresolved and render from device-resident images (`resident` keyed by
    /// DAG node index) produced by `sample_upload_to_working`. Mixed scenes
    /// are supported: `RasterInput` nodes keep the software-raster path.
    pub fn preview_texture_resident(
        &self,
        dag: &RenderDag,
        resident: &std::collections::BTreeMap<usize, crate::ResidentImage>,
        input_identities: &std::collections::BTreeMap<usize, String>,
    ) -> Result<wgpu::Texture, RenderError> {
        let namespace = self.cache_namespace().unwrap();
        let inputs = source_input_keys(dag, &namespace, input_identities)?;
        let (size, scene, working) = lower_with_resident(dag, resident)?;
        let keys = map_scene_keys(
            dag,
            &kronello_render::RasterCacheKey::for_dag_with_inputs(dag, &namespace, &inputs)?,
        );
        self.render_scene_texture_cached(
            size,
            &scene,
            working,
            &keys,
            OutputTransform {
                space: InputSpace::LinearRec709,
                alpha: OutputAlpha::Premultiplied,
            },
        )
        .map_err(error)
    }
    /// Uses the same DAG lowering as export without image readback.
    pub fn preview_texture(&self, dag: &RenderDag) -> Result<wgpu::Texture, RenderError> {
        let (size, scene, working) = lower(dag)?;
        let keys = scene_keys(dag, &self.cache_namespace().unwrap())?;
        self.render_scene_texture_cached(
            size,
            &scene,
            working,
            &keys,
            OutputTransform {
                space: InputSpace::LinearRec709,
                alpha: OutputAlpha::Premultiplied,
            },
        )
        .map_err(error)
    }
    /// `preview_texture` with content-addressed source identities from the
    /// media resolver (`DAG node index` → serialized identity). Identified
    /// `RasterInput` nodes key on the identity instead of hashing pixel data.
    pub fn preview_texture_with_inputs(
        &self,
        dag: &RenderDag,
        input_identities: &std::collections::BTreeMap<usize, String>,
    ) -> Result<wgpu::Texture, RenderError> {
        let namespace = self.cache_namespace().unwrap();
        let inputs = source_input_keys(dag, &namespace, input_identities)?;
        let (size, scene, working) = lower(dag)?;
        let keys = map_scene_keys(
            dag,
            &kronello_render::RasterCacheKey::for_dag_with_inputs(dag, &namespace, &inputs)?,
        );
        self.render_scene_texture_cached(
            size,
            &scene,
            working,
            &keys,
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
    fn cache_namespace(&self) -> Option<String> {
        Some("cpu-reference-f32-v1".into())
    }
    fn display_from_linear(
        &self,
        linear: &[[f32; 4]],
        working: kronello_model::ColorSpace,
    ) -> Result<Vec<[f32; 4]>, RenderError> {
        linear
            .iter()
            .map(|p| convert_output_reference(*p, space(working), DISPLAY).map_err(error))
            .collect()
    }
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
        self.execute_keyed(dag, cache, None)
    }
    fn execute_with_inputs(
        &self,
        dag: &RenderDag,
        cache: &mut kronello_render::RenderCache,
        input_identities: &std::collections::BTreeMap<usize, String>,
    ) -> Result<BackendFrame, RenderError> {
        self.execute_keyed(dag, cache, Some(input_identities))
    }
}
impl CpuReferenceBackend {
    fn execute_keyed(
        &self,
        dag: &RenderDag,
        cache: &mut kronello_render::RenderCache,
        input_identities: Option<&std::collections::BTreeMap<usize, String>>,
    ) -> Result<BackendFrame, RenderError> {
        let (size, scene, working) = lower(dag)?;
        // Keep the exact lowering order; DAG indices are never cache identities.
        let keys = match input_identities {
            Some(identities) => {
                let inputs = source_input_keys(dag, "cpu-reference-f32-v1", identities)?;
                kronello_render::RasterCacheKey::for_dag_with_inputs(
                    dag,
                    "cpu-reference-f32-v1",
                    &inputs,
                )?
            }
            None => kronello_render::RasterCacheKey::for_dag(dag, "cpu-reference-f32-v1")?,
        };
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
            &mut |id, source, map, effect| {
                cache
                    .borrow_mut()
                    .rasterize(keys[id].expect("effect key"), || {
                        crate::effect::apply_reference_mapped(
                            source,
                            map,
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
    fn begin_observation_scope(
        &self,
    ) -> Result<Option<Box<dyn kronello_render::RenderObservationScope + '_>>, RenderError> {
        Ok(Some(Box::new(self.render_scope().map_err(error)?)))
    }

    fn cache_namespace(&self) -> Option<String> {
        Some(format!(
            "wgpu-rgba16f-v1:{}:{:?}:{:?}:{:?}",
            self.strict_namespace(),
            self.adapter_info,
            self.device.features(),
            self.device.limits()
        ))
    }
    fn display_from_linear(
        &self,
        linear: &[[f32; 4]],
        working: kronello_model::ColorSpace,
    ) -> Result<Vec<[f32; 4]>, RenderError> {
        linear
            .iter()
            .map(|p| convert_output_reference(*p, space(working), DISPLAY).map_err(error))
            .collect()
    }
    fn name(&self) -> &str {
        "wgpu_rgba16f"
    }
    fn resource_cache_stats(&self) -> Option<kronello_render::RenderResourceCacheStats> {
        Some(self.render_cache_stats())
    }
    fn transfer_stats_total(&self) -> Option<kronello_render::RenderTransferStats> {
        Some(
            self.total_transfers
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .render_stats(),
        )
    }
    fn transfer_stats(&self) -> Option<kronello_render::RenderTransferStats> {
        Some(
            self.last_transfers
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .render_stats(),
        )
    }
    fn execute(&self, dag: &RenderDag) -> Result<BackendFrame, RenderError> {
        let _scope = self.render_scope().map_err(error)?;
        let (size, scene, working) = lower(dag)?;
        let keys = scene_keys(dag, &self.cache_namespace().unwrap())?;
        let pair = self
            .render_scene_pair_cached(size, &scene, working, Some(&keys), true, DISPLAY)
            .map_err(error)?;
        self.record_transfers(&pair.transfers);
        Ok(crop(
            dag,
            BackendFrame {
                linear: pair.linear,
                display: pair.display,
            },
        ))
    }
    /// Identified inputs key resolved media without hashing pixel buffers;
    /// the device-side resource cache is internal, so the caller cache stays
    /// unused exactly as in `execute`.
    fn execute_with_inputs(
        &self,
        dag: &RenderDag,
        _cache: &mut kronello_render::RenderCache,
        input_identities: &std::collections::BTreeMap<usize, String>,
    ) -> Result<BackendFrame, RenderError> {
        let _scope = self.render_scope().map_err(error)?;
        let (size, scene, working) = lower(dag)?;
        let keys = scene_keys_with_inputs(dag, &self.cache_namespace().unwrap(), input_identities)?;
        let pair = self
            .render_scene_pair_cached(size, &scene, working, Some(&keys), true, DISPLAY)
            .map_err(error)?;
        self.record_transfers(&pair.transfers);
        Ok(crop(
            dag,
            BackendFrame {
                linear: pair.linear,
                display: pair.display,
            },
        ))
    }
}

fn scene_keys(
    dag: &RenderDag,
    namespace: &str,
) -> Result<Vec<Option<kronello_render::RasterCacheKey>>, RenderError> {
    Ok(map_scene_keys(
        dag,
        &kronello_render::RasterCacheKey::for_dag(dag, namespace)?,
    ))
}
/// Convert resolver-supplied serialized identities into per-node source keys.
fn source_input_keys(
    dag: &RenderDag,
    namespace: &str,
    input_identities: &std::collections::BTreeMap<usize, String>,
) -> Result<std::collections::BTreeMap<usize, kronello_render::RasterCacheKey>, RenderError> {
    input_identities
        .iter()
        .map(|(index, identity)| {
            Ok((
                *index,
                kronello_render::RasterCacheKey::external_source(
                    identity,
                    dag.execution_region(),
                    dag.working_space(),
                    namespace,
                )?,
            ))
        })
        .collect()
}
fn scene_keys_with_inputs(
    dag: &RenderDag,
    namespace: &str,
    input_identities: &std::collections::BTreeMap<usize, String>,
) -> Result<Vec<Option<kronello_render::RasterCacheKey>>, RenderError> {
    let inputs = source_input_keys(dag, namespace, input_identities)?;
    Ok(map_scene_keys(
        dag,
        &kronello_render::RasterCacheKey::for_dag_with_inputs(dag, namespace, &inputs)?,
    ))
}
fn map_scene_keys(
    dag: &RenderDag,
    keys: &[Option<kronello_render::RasterCacheKey>],
) -> Vec<Option<kronello_render::RasterCacheKey>> {
    let elided_root = elided_output_root(dag);
    dag.nodes()
        .iter()
        .enumerate()
        .filter_map(|(i, node)| {
            (Some(i) != elided_root
                && !matches!(
                    node,
                    DagNode::Geometry { .. }
                        | DagNode::TextLayout { .. }
                        | DagNode::OutputTransform { .. }
                ))
            .then_some(keys[i])
        })
        .collect()
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

#[cfg(test)]
mod output_root_tests {
    use super::*;
    #[test]
    fn only_final_singleton_root_is_elided_and_cache_keys_stay_aligned() {
        let mut document: kronello_model::Project =
            serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
        let kronello_model::DocumentObject::Known(composition) = &mut document.compositions[0]
        else {
            panic!()
        };
        composition.nodes.truncate(1);
        composition.root_nodes.truncate(1);
        let id = composition.id;
        document.texts.clear();
        let profile = kronello_render::RenderProfile::default();
        let snapshot = kronello_render::RenderSnapshot::new(&document, id, 1, profile).unwrap();
        let scene =
            kronello_render::build_scene_ir(&snapshot, kronello_time::Time::ZERO, &[]).unwrap();
        let dag = kronello_render::build_render_dag(
            &scene,
            profile,
            kronello_render::OutputRegion {
                origin: [0.0; 2],
                extent: [1920.0, 1080.0],
                pixels: [32, 18],
            },
        )
        .unwrap();
        assert_eq!(elided_output_root(&dag), Some(dag.nodes().len() - 2));
        let (_, lowered, _) = lower(&dag).unwrap();
        let keys = scene_keys(&dag, "root-elision-test").unwrap();
        assert_eq!(keys.len(), lowered.nodes.len());
        // The actual composition child remains isolated; only synthetic root goes.
        assert!(
            lowered
                .nodes
                .iter()
                .any(|node| matches!(node, DrawNode::Group { .. }))
        );
        assert_eq!(lowered.roots.len(), 1);
        let mut empty = scene.clone();
        empty.nodes.clear();
        let empty = kronello_render::build_render_dag(&empty, profile, dag.region()).unwrap();
        assert_eq!(elided_output_root(&empty), None);
    }
    #[test]
    #[ignore = "requires an actual GPU adapter and 4K surface memory"]
    fn perf001_basic_4k_preview_admits_elided_root() {
        let mut document: kronello_model::Project =
            serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
        let kronello_model::DocumentObject::Known(composition) = &mut document.compositions[0]
        else {
            panic!()
        };
        composition.nodes.truncate(1);
        composition.root_nodes.truncate(1);
        let id = composition.id;
        document.texts.clear();
        let profile = kronello_render::RenderProfile::default();
        let snapshot = kronello_render::RenderSnapshot::new(&document, id, 1, profile).unwrap();
        let scene =
            kronello_render::build_scene_ir(&snapshot, kronello_time::Time::ZERO, &[]).unwrap();
        let dag = kronello_render::build_render_dag(
            &scene,
            profile,
            kronello_render::OutputRegion {
                origin: [0.0; 2],
                extent: [1920.0, 1080.0],
                pixels: [3840, 2160],
            },
        )
        .unwrap();
        let gpu = GpuContext::new().unwrap();
        let texture = gpu.preview_texture(&dag).unwrap();
        assert_eq!([texture.width(), texture.height()], [3840, 2160]);
        gpu.wait().unwrap();
    }
}
