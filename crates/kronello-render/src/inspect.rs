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
    pub transfers: Vec<TransferEstimate>,
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
    for (tile_id, (_, tile)) in crate::frame_tiles(region).into_iter().enumerate() {
        let dag = crate::build_render_dag_with_cache(scene, profile, tile, cache)?;
        let mut stages = Vec::new();
        let mut surfaces = 4; // one root + backend reserve
        for (index, node) in dag.nodes().iter().enumerate() {
            let (code, key, image) = match node {
                DagNode::Geometry { key, .. } => ("GEOMETRY", Some(key.clone()), false),
                DagNode::TextLayout { key, .. } => ("TEXT_LAYOUT", Some(key.clone()), false),
                DagNode::CoverageDraw { .. } => ("COVERAGE_DRAW", None, true),
                DagNode::IsolatedComposite { children, .. } => {
                    surfaces += children.len() as u64 + 1;
                    ("ISOLATED_COMPOSITE", None, true)
                }
                DagNode::Effect { .. } => {
                    surfaces += 3;
                    ("EFFECT", None, true)
                }
                DagNode::Mask { .. } => ("MASK", None, true),
                DagNode::OutputTransform { .. } => ("OUTPUT_TRANSFORM", None, false),
                DagNode::VideoDraw { bounds, .. } => {
                    // Estimate: one RGBA f32 upload of the drawn pixel bounds per frame.
                    let w = (bounds.max[0] - bounds.min[0]).max(0.0).ceil() as u64;
                    let h = (bounds.max[1] - bounds.min[1]).max(0.0).ceil() as u64;
                    if backend == ExplainBackend::Gpu {
                        image_upload_bytes += w * h * 16;
                        image_uploads += 1;
                    }
                    ("VIDEO_DRAW", None, true)
                }
                DagNode::RasterInput { pixels } => {
                    image_upload_bytes += pixels.len() as u64 * 16;
                    image_uploads += 1;
                    ("RASTER_INPUT", None, true)
                }
            };
            if image {
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
        // Built-in frame export renders twice, with one padded image and one
        // four-byte shader validation status readback per execution.
        readback_bytes +=
            (u64::from(pixels[0]) * 8).div_ceil(256) * 256 * u64::from(pixels[1]) * 2 + 8;
        result.tiles.push(RenderTilePlan {
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
            code: "DUPLICATE_LINEAR_DISPLAY_RENDER".into(),
            tile: None,
            actual_estimate: Some(2),
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
            Some(image_upload_bytes)
        } else {
            known.then_some(0)
        },
        if gpu {
            Some(image_uploads)
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
            result.tiles.len() as u64 * 4
        } else {
            0
        }),
    );
    result.compilation_cache = cache.stats();
    Ok(result)
}
