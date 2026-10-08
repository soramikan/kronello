use kronello_gpu::render_adapter::CpuReferenceBackend;
use kronello_gpu::{GpuCacheConfig, GpuContext};
use kronello_model::{DocumentObject, NodeKind, Project};
use kronello_render::*;
use kronello_time::{FrameRate, Rational, Time};

fn fixture(profile: RenderProfile) -> RenderSnapshot {
    let mut project: Project =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    project.texts.clear();
    let DocumentObject::Known(composition) = &mut project.compositions[0] else {
        panic!()
    };
    composition
        .nodes
        .retain(|n| !matches!(n.kind, NodeKind::Text { .. }));
    composition
        .root_nodes
        .retain(|id| composition.nodes.iter().any(|n| n.id == *id));
    let id = composition.id;
    RenderSnapshot::new(&project, id, 1, profile).unwrap()
}
fn region() -> OutputRegion {
    OutputRegion {
        origin: [0.0; 2],
        extent: [1920.0, 1080.0],
        pixels: [513, 3],
    }
}
fn cold_gpu() -> GpuContext {
    let gpu = GpuContext::new().unwrap();
    let mut config = GpuCacheConfig::default();
    config.textures.entries = 0;
    config.textures.bytes = 0;
    config.pool.entries = 0;
    config.pool.bytes = 0;
    config.disk = None;
    gpu.configure_cache(config).unwrap();
    gpu
}
fn readback(plan: &RenderPathPlan) -> &TransferEstimate {
    plan.transfers
        .iter()
        .find(|t| t.code == "IMAGE_AND_STATUS_READBACK")
        .unwrap()
}
#[test]
#[ignore = "requires an actual GPU adapter"]
fn single_graph_final_and_native_preview_estimates_match_actual_gpu_counters() {
    let snapshot = fixture(RenderProfile::default());
    let gpu = cold_gpu();
    for backend in [
        ExplainBackend::Gpu,
        ExplainBackend::GpuResidentBgra8,
        ExplainBackend::GpuResidentNv12,
    ] {
        let before = gpu.transfer_stats_total().unwrap();
        let plan = explain_snapshot_render_path(
            &snapshot,
            Time::ZERO,
            &[],
            region(),
            backend,
            &mut RenderCache::default(),
        )
        .unwrap();
        assert!(!plan.executed);
        assert_eq!(gpu.transfer_stats_total().unwrap(), before);
        assert_eq!(plan.graph_executions_estimate, Some(2));
        assert!(
            !plan
                .notices
                .iter()
                .any(|n| n.code == "DUPLICATE_LINEAR_DISPLAY_RENDER")
        );
        let frame = render_frame_with_cache(
            &snapshot,
            &[],
            &gpu,
            FrameRequest {
                time: Time::ZERO,
                region: region(),
            },
            &mut RenderCache::new(CacheConfig::disabled()),
        )
        .unwrap();
        let measured = frame.metadata.transfer_stats.unwrap();
        assert_eq!(
            readback(&plan).bytes_estimate,
            Some(measured.gpu_readback_bytes)
        );
        assert_eq!(
            readback(&plan).operations_estimate,
            Some(measured.gpu_readback_operations)
        );
        assert_eq!(measured.gpu_readback_operations, 6);
        assert_eq!(measured.gpu_copy_operations, 0);
        assert_eq!(measured.cpu_upload_pixel_operations, 0);
        // Native preview uses the same graph and only the sticky validation status.
        let scene = build_scene_ir(&snapshot, Time::ZERO, &[]).unwrap();
        let baseline = gpu.transfer_stats_total().unwrap();
        let dag = build_render_dag(&scene, snapshot.profile(), region()).unwrap();
        let _texture = gpu.preview_texture(&dag).unwrap();
        let after = gpu.transfer_stats_total().unwrap();
        let preview = plan
            .native_preview_transfers
            .iter()
            .find(|t| t.code == "STATUS_ONLY_READBACK")
            .unwrap();
        assert_eq!(
            preview.bytes_estimate,
            Some(after.gpu_readback_bytes - baseline.gpu_readback_bytes)
        );
        assert_eq!(
            preview.operations_estimate,
            Some(after.gpu_readback_operations - baseline.gpu_readback_operations)
        );
        assert_eq!(preview.bytes_estimate, Some(4));
        // Reset the planning baseline without asserting counters stay unchanged
        // after the explicit execution above.
    }
}
#[test]
#[ignore = "requires an actual GPU adapter"]
fn cpu_unknown_and_temporal_tile_plans_respect_execution_boundaries() {
    let settings = TemporalSettings {
        frame_rate: FrameRate::new(24, 1).unwrap(),
        shutter_angle: Rational::from_integer(180),
        shutter_phase: Rational::ZERO,
        samples: 3,
        cut_policy: CutPolicy::AllowCrossing,
    };
    let snapshot = fixture(RenderProfile {
        temporal: Some(settings),
        ..RenderProfile::default()
    });
    let gpu = cold_gpu();
    let plan = explain_snapshot_render_path(
        &snapshot,
        Time::ZERO,
        &[],
        region(),
        ExplainBackend::Gpu,
        &mut RenderCache::default(),
    )
    .unwrap();
    assert_eq!(
        plan.temporal_samples,
        temporal_samples(&snapshot, Time::ZERO, settings).unwrap()
    );
    assert_eq!(plan.tiles.len(), 6);
    assert_eq!(plan.graph_executions_estimate, Some(6));
    assert!(!plan.native_preview_supported);
    let measured = render_frame_with_cache(
        &snapshot,
        &[],
        &gpu,
        FrameRequest {
            time: Time::ZERO,
            region: region(),
        },
        &mut RenderCache::new(CacheConfig::disabled()),
    )
    .unwrap()
    .metadata
    .transfer_stats
    .unwrap();
    assert_eq!(
        readback(&plan).bytes_estimate,
        Some(measured.gpu_readback_bytes)
    );
    assert_eq!(
        readback(&plan).operations_estimate,
        Some(measured.gpu_readback_operations)
    );
    for backend in [
        ExplainBackend::GpuResidentBgra8,
        ExplainBackend::GpuResidentNv12,
    ] {
        assert!(matches!(
            explain_snapshot_render_path(
                &snapshot,
                Time::ZERO,
                &[],
                region(),
                backend,
                &mut RenderCache::default()
            ),
            Err(RenderError::UnsupportedFeature(_))
        ));
    }
    let cpu = explain_snapshot_render_path(
        &snapshot,
        Time::ZERO,
        &[],
        region(),
        ExplainBackend::CpuReference,
        &mut RenderCache::default(),
    )
    .unwrap();
    assert!(
        cpu.transfers
            .iter()
            .all(|t| t.bytes_estimate == Some(0) && t.operations_estimate == Some(0))
    );
    assert!(!cpu.native_preview_supported);
    render_frame_with_cache(
        &snapshot,
        &[],
        &CpuReferenceBackend,
        FrameRequest {
            time: Time::ZERO,
            region: region(),
        },
        &mut RenderCache::new(CacheConfig::disabled()),
    )
    .unwrap();
    let unknown = explain_snapshot_render_path(
        &snapshot,
        Time::ZERO,
        &[],
        region(),
        ExplainBackend::Unknown,
        &mut RenderCache::default(),
    )
    .unwrap();
    assert!(
        unknown
            .transfers
            .iter()
            .all(|t| t.bytes_estimate.is_none() && t.operations_estimate.is_none())
    );
    assert!(unknown.graph_executions_estimate.is_none());
}

#[test]
fn backend_free_temporal_plan_matches_actual_cpu_execution_count_and_warm_cache() {
    use std::sync::atomic::{AtomicU64, Ordering};
    struct CountingCpu(AtomicU64);
    impl RenderBackend for CountingCpu {
        fn name(&self) -> &str {
            "inspection-counting-cpu"
        }
        fn cache_namespace(&self) -> Option<String> {
            Some(self.name().into())
        }
        fn execute(&self, dag: &RenderDag) -> Result<BackendFrame, RenderError> {
            self.0.fetch_add(1, Ordering::Relaxed);
            CpuReferenceBackend.execute(dag)
        }
        fn display_from_linear(
            &self,
            linear: &[[f32; 4]],
            working: kronello_model::ColorSpace,
        ) -> Result<Vec<[f32; 4]>, RenderError> {
            CpuReferenceBackend.display_from_linear(linear, working)
        }
    }
    let settings = TemporalSettings {
        frame_rate: FrameRate::new(24, 1).unwrap(),
        shutter_angle: Rational::from_integer(180),
        shutter_phase: Rational::ZERO,
        samples: 3,
        cut_policy: CutPolicy::AllowCrossing,
    };
    let snapshot = fixture(RenderProfile {
        temporal: Some(settings),
        ..RenderProfile::default()
    });
    let backend = CountingCpu(AtomicU64::new(0));
    let plan = explain_snapshot_render_path(
        &snapshot,
        Time::ZERO,
        &[],
        region(),
        ExplainBackend::CpuReference,
        &mut RenderCache::default(),
    )
    .unwrap();
    assert_eq!(backend.0.load(Ordering::Relaxed), 0);
    assert_eq!(
        plan.temporal_samples,
        temporal_samples(&snapshot, Time::ZERO, settings).unwrap()
    );
    assert_eq!(plan.tiles.len(), 6);
    assert!(plan.tiles.iter().all(|t| t.sample_time.is_some()));
    assert!(
        plan.transfers
            .iter()
            .all(|t| t.bytes_estimate == Some(0) && t.operations_estimate == Some(0))
    );
    let mut live = RenderCache::default();
    let request = FrameRequest {
        time: Time::ZERO,
        region: region(),
    };
    let cold = render_frame_with_cache(&snapshot, &[], &backend, request, &mut live).unwrap();
    assert_eq!(
        Some(backend.0.load(Ordering::Relaxed)),
        plan.graph_executions_estimate
    );
    let warm = render_frame_with_cache(&snapshot, &[], &backend, request, &mut live).unwrap();
    assert_eq!(cold.pixels, warm.pixels);
    assert_eq!(backend.0.load(Ordering::Relaxed), 6);
    let unknown = explain_snapshot_render_path(
        &snapshot,
        Time::ZERO,
        &[],
        region(),
        ExplainBackend::Unknown,
        &mut RenderCache::default(),
    )
    .unwrap();
    assert!(
        unknown
            .transfers
            .iter()
            .all(|t| t.bytes_estimate.is_none() && t.operations_estimate.is_none())
    );
    assert!(unknown.graph_executions_estimate.is_none());
    for backend in [
        ExplainBackend::GpuResidentBgra8,
        ExplainBackend::GpuResidentNv12,
    ] {
        assert!(matches!(
            explain_snapshot_render_path(
                &snapshot,
                Time::ZERO,
                &[],
                region(),
                backend,
                &mut RenderCache::default()
            ),
            Err(RenderError::UnsupportedFeature(_))
        ));
    }
}

#[test]
#[ignore = "requires an actual GPU adapter"]
fn resident_graph_input_and_final_boundary_match_counters_without_cpu_image_upload() {
    use kronello_gpu::{ResidentImage, WorkingSpace};
    use kronello_model::{Asset, AssetId, AssetKind, AssetLocator};
    use std::collections::BTreeMap;
    let snapshot = fixture(RenderProfile::default());
    let mut scene = build_scene_ir(&snapshot, Time::ZERO, &[]).unwrap();
    scene.nodes[0].content = SceneContent::Video {
        asset: Asset {
            id: AssetId::new(),
            kind: AssetKind::Video,
            content_hash: "0".repeat(64),
            streams: vec![],
            locator: AssetLocator {
                relative: Some("diagnostic-only.mp4".into()),
                absolute: None,
            },
        },
        stream_index: 0,
        time: Time::ZERO,
        reverse_sampling: false,
        extent: scene.design_extent,
        crop: None,
    };
    let region = OutputRegion {
        origin: [0.0; 2],
        extent: scene.design_extent,
        pixels: [13, 5],
    };
    let gpu = cold_gpu();
    for backend in [
        ExplainBackend::GpuResidentBgra8,
        ExplainBackend::GpuResidentNv12,
    ] {
        let plan = explain_render_path(
            &scene,
            snapshot.profile(),
            region,
            backend,
            &mut RenderCache::default(),
        )
        .unwrap();
        let dag = build_render_dag(&scene, snapshot.profile(), region).unwrap();
        let inputs: BTreeMap<_, _> = dag
            .nodes()
            .iter()
            .enumerate()
            .filter(|(_, node)| matches!(node, DagNode::VideoDraw { .. }))
            .map(|(index, _)| {
                (
                    index,
                    ResidentImage::allocate(
                        &gpu,
                        dag.execution_region().pixels,
                        WorkingSpace::LinearRec709,
                    )
                    .unwrap(),
                )
            })
            .collect();
        let (_, measured) = gpu
            .execute_resident_video_with_stats(&dag, &inputs)
            .unwrap();
        assert_eq!(
            readback(&plan).bytes_estimate,
            Some(measured.gpu_readback_bytes)
        );
        assert_eq!(
            readback(&plan).operations_estimate,
            Some(measured.gpu_readback_operations)
        );
        assert_eq!(measured.cpu_upload_pixel_bytes, 0);
        assert_eq!(measured.gpu_copy_bytes, 0);
        assert_eq!(
            plan.transfers
                .iter()
                .find(|t| t.code == "IMAGE_UPLOAD")
                .unwrap()
                .bytes_estimate,
            Some(0)
        );
    }
    let decoded = explain_render_path(
        &scene,
        snapshot.profile(),
        region,
        ExplainBackend::Gpu,
        &mut RenderCache::default(),
    )
    .unwrap();
    assert!(
        decoded
            .transfers
            .iter()
            .find(|t| t.code == "IMAGE_UPLOAD")
            .unwrap()
            .bytes_estimate
            .is_none()
    );
}
