//! Backend-free frame plans. Estimates never claim device availability or timings.
use serde::{Deserialize, Serialize};

use crate::{
    DagNode, OutputRegion, RenderCache, RenderCacheStats, RenderError, RenderProfile, SceneIr,
    SceneKey,
};

/// The renderer's actual static layout edges, without layout work or cache access.
pub fn inspection_layout_dependencies(
    project: &kronello_model::Project,
    definitions: &[kronello_model::Composition],
    root: kronello_model::CompositionId,
) -> Result<kronello_eval::DependencyDeclarations, RenderError> {
    Ok(crate::template::TemplateRuntime::compile(project, definitions, root)?.dependencies)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExplainBackend {
    Gpu,
    GpuResidentBgra8,
    GpuResidentNv12,
    CpuReference,
    /// An injected backend has no known resource/transfer contract.
    Unknown,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderStage {
    /// Index local to this tile's DAG, not a document identity.
    pub index: usize,
    pub code: String,
    pub inputs: Vec<usize>,
    pub node: Option<SceneKey>,
    pub execution: String,
    pub image: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderTilePlan {
    /// Exact root sample time; absent for the scene-only planning entry.
    pub sample_time: Option<kronello_time::Time>,
    pub requested: OutputRegion,
    pub execution: OutputRegion,
    pub stages: Vec<RenderStage>,
    /// Conservative surface count matching the built-in backend budget formula.
    pub budget_surfaces_estimate: u64,
    pub surface_bytes_estimate: u64,
    pub intermediate_bytes_estimate: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransferEstimate {
    pub code: String,
    pub direction: String,
    /// None means not estimated, rather than zero or a measured value.
    pub bytes_estimate: Option<u64>,
    pub operations_estimate: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProcessingNotice {
    pub code: String,
    pub tile: Option<usize>,
    pub actual_estimate: Option<u64>,
    pub limit: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderPathPlan {
    pub backend: ExplainBackend,
    pub profile: RenderProfile,
    /// Always false: compilation does not initialize or execute a backend.
    pub executed: bool,
    pub tiles: Vec<RenderTilePlan>,
    /// Final linear/display CPU output boundary, excluding cache persistence.
    pub transfers: Vec<TransferEstimate>,
    pub output_boundary: String,
    /// Alternative native texture boundary; not added to final transfers.
    pub native_preview_transfers: Vec<TransferEstimate>,
    pub native_preview_supported: bool,
    pub temporal_samples: Vec<crate::TemporalSample>,
    /// Maximum executions without backend or temporal raster-cache hits.
    pub graph_executions_estimate: Option<u64>,
    pub estimate_scope: String,
    /// Two retained full-frame float32 RGBA host outputs, excluding allocator overhead.
    pub output_host_bytes_estimate: u64,
    pub peak_intermediate_bytes_estimate: Option<u64>,
    pub notices: Vec<ProcessingNotice>,
    /// Actual counters for isolated compilation only; raster is not executed.
    pub compilation_cache: RenderCacheStats,
    pub cache_scope: String,
    pub raster_cache_observed: bool,
}

/// Compiles into the supplied diagnostic cache; does not execute a backend or
/// probe raster entries. Service callers must supply a fresh isolated cache.
pub fn explain_render_path(
    scene: &SceneIr,
    profile: RenderProfile,
    region: OutputRegion,
    backend: ExplainBackend,
    cache: &mut RenderCache,
) -> Result<RenderPathPlan, RenderError> {
    region.validate()?;
    let mut result = RenderPathPlan {
        backend,
        profile,
        executed: false,
        tiles: vec![],
        transfers: vec![],
        output_boundary: "final_linear_display_cpu_readback".into(),
        native_preview_transfers: vec![],
        native_preview_supported: matches!(
            backend,
            ExplainBackend::Gpu
                | ExplainBackend::GpuResidentBgra8
                | ExplainBackend::GpuResidentNv12
        ) && profile.temporal.is_none(),
        temporal_samples: vec![],
        graph_executions_estimate: None,
        estimate_scope: "cold_execution_without_raster_cache_persistence_or_hits".into(),
        output_host_bytes_estimate: u64::from(region.pixels[0]) * u64::from(region.pixels[1]) * 32,
        peak_intermediate_bytes_estimate: match backend {
            ExplainBackend::Unknown => None,
            _ => Some(0),
        },
        notices: vec![],
        compilation_cache: Default::default(),
        cache_scope: "isolated_query_compilation".into(),
        raster_cache_observed: false,
    };
    let mut readback_bytes = 0;
    // CPU-decoded video frames and CPU-prepared rasters are uploaded explicitly on GPU (ADR-0008).
    let mut image_upload_bytes: u64 = 0;
    let mut image_uploads: u64 = 0;
    let mut decoded_upload_unknown = false;
    for (tile_id, (_, tile)) in crate::frame_tiles(region).into_iter().enumerate() {
        let dag = crate::build_render_dag_with_cache(scene, profile, tile, cache)?;
        let mut stages = Vec::new();
        let mut surfaces = 4; // one root + backend reserve
        let elided_root = if backend != ExplainBackend::CpuReference {
            dag.nodes().len().checked_sub(2).filter(|root| matches!((&dag.nodes()[*root], dag.nodes().last()), (DagNode::IsolatedComposite { children, opacity }, Some(DagNode::OutputTransform { source, .. })) if *source == *root && children.len() == 1 && *opacity == 1.0))
        } else {
            None
        };
        for (index, node) in dag.nodes().iter().enumerate() {
            let (code, key, image) = match node {
                DagNode::Geometry { key, .. } => ("GEOMETRY", Some(key.clone()), false),
                DagNode::TextLayout { key, .. } => ("TEXT_LAYOUT", Some(key.clone()), false),
                DagNode::CoverageDraw { .. } => ("COVERAGE_DRAW", None, true),
                DagNode::IsolatedComposite { children, .. } => {
                    if Some(index) != elided_root {
                        surfaces += children.len() as u64 + 1;
                    }
                    ("ISOLATED_COMPOSITE", None, true)
                }
                DagNode::Effect { .. } => {
                    surfaces += 3;
                    ("EFFECT", None, true)
                }
                DagNode::Blend { .. } => ("BLEND", None, true),
                DagNode::Mask { .. } => ("MASK", None, true),
                DagNode::SolidRect { .. } => ("SOLID_RECT", None, true),
                DagNode::OutputTransform { .. } => ("OUTPUT_TRANSFORM", None, false),
                DagNode::VideoDraw { .. } => {
                    // Decoded source dimensions, adapter conversion, and media-cache
                    // reuse are not known from the output-space draw rectangle.
                    if backend == ExplainBackend::Gpu {
                        decoded_upload_unknown = true;
                    }
                    ("VIDEO_DRAW", None, true)
                }
                DagNode::RasterInput { pixels } => {
                    image_upload_bytes += pixels.len() as u64 * 8;
                    image_uploads += 1;
                    ("RASTER_INPUT", None, true)
                }
            };
            if image && Some(index) != elided_root {
                surfaces += 1;
            }
            let execution = if !image && code != "OUTPUT_TRANSFORM" {
                "cpu"
            } else {
                match backend {
                    ExplainBackend::Gpu
                    | ExplainBackend::GpuResidentBgra8
                    | ExplainBackend::GpuResidentNv12 => "gpu",
                    ExplainBackend::CpuReference => "cpu",
                    ExplainBackend::Unknown => "unknown",
                }
            };
            stages.push(RenderStage {
                index,
                code: code.into(),
                inputs: node.inputs(),
                node: key,
                image,
                execution: execution.into(),
            });
        }
        let pixels = dag.execution_region().pixels;
        let area = u64::from(pixels[0]) * u64::from(pixels[1]);
        let surface_bytes = area * 8;
        let intermediate = match backend {
            ExplainBackend::Gpu
            | ExplainBackend::GpuResidentBgra8
            | ExplainBackend::GpuResidentNv12 => Some(surface_bytes * surfaces),
            ExplainBackend::CpuReference => Some(area * 16 * surfaces),
            ExplainBackend::Unknown => None,
        };
        if let Some(bytes) = intermediate {
            result.peak_intermediate_bytes_estimate = Some(
                result
                    .peak_intermediate_bytes_estimate
                    .unwrap_or(0)
                    .max(bytes),
            );
            if bytes > 512 * 1024 * 1024 {
                result.notices.push(ProcessingNotice {
                    code: "SURFACE_BUDGET_EXCEEDED".into(),
                    tile: Some(tile_id),
                    actual_estimate: Some(bytes),
                    limit: Some(512 * 1024 * 1024),
                });
            }
        }
        if pixels != tile.pixels {
            result.notices.push(ProcessingNotice {
                code: "EFFECT_HALO_EXPANSION".into(),
                tile: Some(tile_id),
                actual_estimate: Some(area),
                limit: None,
            });
        }
        // A single graph generates linear/display outputs. Each padded image
        // crosses its final boundary, then sticky status crosses once.
        readback_bytes +=
            (u64::from(pixels[0]) * 8).div_ceil(256) * 256 * u64::from(pixels[1]) * 2 + 4;
        result.tiles.push(RenderTilePlan {
            sample_time: None,
            requested: tile,
            execution: dag.execution_region(),
            stages,
            budget_surfaces_estimate: surfaces,
            surface_bytes_estimate: surface_bytes,
            intermediate_bytes_estimate: intermediate,
        });
    }
    if profile.hdr.is_some() {
        result.notices.push(ProcessingNotice {
            code: "HDR_LINEAR_REC2020_REFERENCE_WHITE_203_NITS".into(),
            tile: None,
            actual_estimate: Some(203),
            limit: None,
        });
        result.notices.push(ProcessingNotice {
            code: "HDR_DISPLAY_ONLY_REC709_REINHARD_SRGB_LINEAR_UNCHANGED".into(),
            tile: None,
            actual_estimate: None,
            limit: None,
        });
    }
    if matches!(
        backend,
        ExplainBackend::Gpu | ExplainBackend::GpuResidentBgra8 | ExplainBackend::GpuResidentNv12
    ) {
        result.notices.push(ProcessingNotice {
            code: "SINGLE_GRAPH_LINEAR_DISPLAY_OUTPUT".into(),
            tile: None,
            actual_estimate: Some(1),
            limit: None,
        });
    }
    if matches!(
        backend,
        ExplainBackend::GpuResidentBgra8 | ExplainBackend::GpuResidentNv12
    ) {
        result.notices.push(ProcessingNotice {
            code: "REQUIRE_GPU_RESIDENT_HARDWARE_DECODE_SAME_DEVICE_IMPORT".into(),
            tile: None,
            actual_estimate: None,
            limit: None,
        });
        if profile.temporal.is_some() {
            result.notices.push(ProcessingNotice {
                code: "REQUIRE_GPU_RESIDENT_REJECTS_CPU_TEMPORAL_ACCUMULATION".into(),
                tile: None,
                actual_estimate: None,
                limit: None,
            });
        }
    }
    let transparent = scene.nodes.iter().filter(|n| n.opacity == 0.0).count() as u64;
    if transparent > 0 {
        result.notices.push(ProcessingNotice {
            code: "ZERO_OPACITY_STILL_PROCESSED".into(),
            tile: None,
            actual_estimate: Some(transparent),
            limit: None,
        });
    }
    let gpu = matches!(
        backend,
        ExplainBackend::Gpu | ExplainBackend::GpuResidentBgra8 | ExplainBackend::GpuResidentNv12
    );
    let known = backend != ExplainBackend::Unknown;
    let mut transfer = |code: &str, direction: &str, bytes, operations| {
        result.transfers.push(TransferEstimate {
            code: code.into(),
            direction: direction.into(),
            bytes_estimate: bytes,
            operations_estimate: operations,
        });
    };
    transfer(
        "CONTROL_UPLOAD",
        "cpu_to_gpu",
        if gpu || !known { None } else { Some(0) },
        if gpu || !known { None } else { Some(0) },
    );
    transfer(
        "IMAGE_UPLOAD",
        "cpu_to_gpu",
        if gpu {
            (!decoded_upload_unknown).then_some(image_upload_bytes)
        } else {
            known.then_some(0)
        },
        if gpu {
            (!decoded_upload_unknown).then_some(image_uploads)
        } else {
            known.then_some(0)
        },
    );
    transfer(
        "GPU_IMAGE_COPY",
        "gpu_to_gpu",
        known.then_some(0),
        known.then_some(0),
    );
    transfer(
        "IMAGE_AND_STATUS_READBACK",
        "gpu_to_cpu",
        known.then_some(if gpu { readback_bytes } else { 0 }),
        known.then_some(if gpu {
            result.tiles.len() as u64 * 3
        } else {
            0
        }),
    );
    if result.native_preview_supported {
        // Native preview lowers one full-region DAG, independently of the
        // final CPU frame path's 512px tile traversal. Its input uploads are
        // not obtained by adding the final path's per-tile conversions.
        result.native_preview_transfers = vec![
            TransferEstimate {
                code: "CONTROL_UPLOAD".into(),
                direction: "cpu_to_gpu".into(),
                bytes_estimate: None,
                operations_estimate: None,
            },
            TransferEstimate {
                code: "IMAGE_UPLOAD".into(),
                direction: "cpu_to_gpu".into(),
                bytes_estimate: None,
                operations_estimate: None,
            },
            TransferEstimate {
                code: "GPU_IMAGE_COPY".into(),
                direction: "gpu_to_gpu".into(),
                bytes_estimate: Some(0),
                operations_estimate: Some(0),
            },
            TransferEstimate {
                code: "STATUS_ONLY_READBACK".into(),
                direction: "gpu_to_cpu".into(),
                bytes_estimate: Some(4),
                operations_estimate: Some(1),
            },
        ];
        result.notices.push(ProcessingNotice {
            code: "NATIVE_PREVIEW_ONE_FULL_REGION_GRAPH_NO_IMAGE_READBACK".into(),
            tile: None,
            actual_estimate: Some(1),
            limit: None,
        });
    }
    result.graph_executions_estimate = known.then_some(result.tiles.len() as u64);
    result.notices.push(ProcessingNotice {
        code: "COLD_ESTIMATE_CACHE_HITS_MAY_REDUCE_EXECUTION_TO_ZERO".into(),
        tile: None,
        actual_estimate: None,
        limit: None,
    });
    result.notices.push(ProcessingNotice {
        code: "CONTROL_UPLOAD_AND_CACHE_PERSISTENCE_NOT_ESTIMATED".into(),
        tile: None,
        actual_estimate: None,
        limit: None,
    });
    if profile.temporal.is_some() {
        result.notices.push(ProcessingNotice {
            code: "SCENE_ONLY_PLAN_SINGLE_SAMPLE_USE_SNAPSHOT_PLAN_FOR_TEMPORAL".into(),
            tile: None,
            actual_estimate: None,
            limit: None,
        });
    }
    result.compilation_cache = cache.stats();
    Ok(result)
}

/// Plans every actual rational shutter sample and tile without backend creation,
/// decode, raster-cache lookup, upload, dispatch, or readback.
pub fn explain_snapshot_render_path(
    snapshot: &crate::RenderSnapshot,
    time: kronello_time::Time,
    fonts: &[kronello_text::FontData<'_>],
    region: OutputRegion,
    backend: ExplainBackend,
    cache: &mut RenderCache,
) -> Result<RenderPathPlan, RenderError> {
    let profile = snapshot.profile();
    let samples = if let Some(settings) = profile.temporal {
        if matches!(
            backend,
            ExplainBackend::GpuResidentBgra8 | ExplainBackend::GpuResidentNv12
        ) {
            return Err(RenderError::UnsupportedFeature(
                "require_gpu_resident rejects CPU temporal accumulation".into(),
            ));
        }
        crate::temporal_samples(snapshot, time, settings)?
    } else {
        vec![crate::TemporalSample {
            time,
            weight: kronello_time::Rational::ONE,
        }]
    };
    let mut result: Option<RenderPathPlan> = None;
    for sample in &samples {
        let scene = crate::build_scene_ir_with_cache(snapshot, sample.time, fonts, cache)?;
        let mut plan = explain_render_path(&scene, profile, region, backend, cache)?;
        for tile in &mut plan.tiles {
            tile.sample_time = Some(sample.time);
        }
        plan.notices
            .retain(|n| n.code != "SCENE_ONLY_PLAN_SINGLE_SAMPLE_USE_SNAPSHOT_PLAN_FOR_TEMPORAL");
        if let Some(total) = &mut result {
            let offset = total.tiles.len();
            total.tiles.extend(plan.tiles);
            for notice in &mut plan.notices {
                if let Some(tile) = &mut notice.tile {
                    *tile += offset;
                }
            }
            total
                .notices
                .extend(plan.notices.into_iter().filter(|n| n.tile.is_some()));
            for (transfer, next) in total.transfers.iter_mut().zip(plan.transfers) {
                transfer.bytes_estimate = transfer
                    .bytes_estimate
                    .zip(next.bytes_estimate)
                    .and_then(|(a, b)| a.checked_add(b));
                transfer.operations_estimate = transfer
                    .operations_estimate
                    .zip(next.operations_estimate)
                    .and_then(|(a, b)| a.checked_add(b));
            }
            total.graph_executions_estimate = total
                .graph_executions_estimate
                .zip(plan.graph_executions_estimate)
                .and_then(|(a, b)| a.checked_add(b));
            total.peak_intermediate_bytes_estimate = total
                .peak_intermediate_bytes_estimate
                .zip(plan.peak_intermediate_bytes_estimate)
                .map(|(a, b)| a.max(b));
        } else {
            result = Some(plan);
        }
    }
    let mut result = result.expect("temporal samples always nonempty");
    if profile.temporal.is_some() {
        result.temporal_samples = samples;
        result.notices.push(ProcessingNotice {
            code: "TEMPORAL_ROOT_CPU_ACCUMULATION_AFTER_SAMPLE_READBACKS".into(),
            tile: None,
            actual_estimate: Some(u64::from(region.pixels[0]) * u64::from(region.pixels[1]) * 64),
            limit: None,
        });
        result.notices.push(ProcessingNotice {
            code: "TEMPORAL_WHOLE_FRAME_OR_TILE_STREAMING_HOST_MEMORY_DIFFERS".into(),
            tile: None,
            actual_estimate: Some(u64::from(region.pixels[0]) * u64::from(region.pixels[1]) * 96),
            limit: Some(512 * 1024 * 1024),
        });
    }
    result.compilation_cache = cache.stats();
    Ok(result)
}
