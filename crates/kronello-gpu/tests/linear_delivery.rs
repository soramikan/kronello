use kronello_gpu::{GpuContext, render_adapter::CpuReferenceBackend};
use kronello_model::{ColorSpace, DocumentObject, Project, ResolvedEffect};
use kronello_render::*;
use kronello_time::{FrameRate, Time};

fn snapshot(hdr: bool, temporal: bool) -> RenderSnapshot {
    let mut document: Project =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    let DocumentObject::Known(composition) = &mut document.compositions[0] else {
        panic!()
    };
    composition.nodes.truncate(1);
    composition.root_nodes.truncate(1);
    let id = composition.id;
    document.texts.clear();
    let profile = RenderProfile {
        working_space: if hdr {
            ColorSpace::LinearRec2020
        } else {
            ColorSpace::LinearRec709
        },
        hdr: hdr.then_some(HdrSettings {
            transfer: HdrTransfer::Pq,
        }),
        temporal: temporal.then_some(TemporalSettings {
            frame_rate: FrameRate::new(24, 1).unwrap(),
            shutter_angle: Time::new(180, 1).unwrap(),
            shutter_phase: Time::new(-1, 4).unwrap(),
            samples: 3,
            cut_policy: CutPolicy::AvoidCrossing,
        }),
        ..RenderProfile::default()
    };
    RenderSnapshot::new(&document, id, 1, profile).unwrap()
}
fn request() -> FrameRequest {
    FrameRequest {
        time: Time::new(1, 2).unwrap(),
        region: OutputRegion {
            origin: [-1.0, 0.0],
            extent: [66.0, 32.0],
            pixels: [513, 18],
        },
    }
}
fn assemble(
    snapshot: &RenderSnapshot,
    backend: &dyn RenderBackend,
    request: FrameRequest,
) -> (Vec<[f32; 4]>, FrameMetadata) {
    let mut pixels = vec![[0.0; 4]; (request.region.pixels[0] * request.region.pixels[1]) as usize];
    let metadata = render_frame_linear_tiles(
        snapshot,
        &[],
        backend,
        request,
        &mut |[x, y], tile, linear| {
            for row in 0..tile.pixels[1] as usize {
                let start = (y as usize + row) * request.region.pixels[0] as usize + x as usize;
                let source = row * tile.pixels[0] as usize;
                let width = tile.pixels[0] as usize;
                pixels[start..start + width].copy_from_slice(&linear[source..source + width]);
            }
            Ok(())
        },
    )
    .unwrap();
    (pixels, metadata)
}
struct LinearOnly;
impl RenderBackend for LinearOnly {
    fn name(&self) -> &str {
        "linear_only_contract"
    }
    fn execute(&self, _: &RenderDag) -> Result<BackendFrame, RenderError> {
        panic!("linear export must not execute both planes")
    }
    fn display_from_linear(
        &self,
        _: &[[f32; 4]],
        _: ColorSpace,
    ) -> Result<Vec<[f32; 4]>, RenderError> {
        panic!("linear export must not transform display, including shutter samples")
    }
    fn execute_linear_with_cache(
        &self,
        dag: &RenderDag,
        cache: &mut RenderCache,
    ) -> Result<Vec<[f32; 4]>, RenderError> {
        CpuReferenceBackend.execute_linear_with_cache(dag, cache)
    }
}

#[test]
fn linear_tiles_preserve_sdr_hdr_temporal_and_complete_frame_cache() {
    for hdr in [false, true] {
        for temporal in [false, true] {
            let snapshot = snapshot(hdr, temporal);
            let request = request();
            let mut cache = RenderCache::new(CacheConfig::default());
            let both =
                render_frame_with_cache(&snapshot, &[], &CpuReferenceBackend, request, &mut cache)
                    .unwrap();
            let (linear, metadata) = assemble(&snapshot, &LinearOnly, request);
            assert_eq!(linear, both.pixels.linear);
            assert_eq!(metadata.temporal, both.metadata.temporal);
            assert_eq!(
                metadata.snapshot_content_hash,
                both.metadata.snapshot_content_hash
            );
            assert_eq!(metadata.hdr, both.metadata.hdr);
            let warm =
                render_frame_with_cache(&snapshot, &[], &CpuReferenceBackend, request, &mut cache)
                    .unwrap();
            assert_eq!(warm.pixels, both.pixels);
            assert_eq!(warm.pixels.display.len(), linear.len());
        }
    }
}

struct BadLinear(bool);
impl RenderBackend for BadLinear {
    fn name(&self) -> &str {
        "invalid_linear_contract"
    }
    fn execute(&self, _: &RenderDag) -> Result<BackendFrame, RenderError> {
        panic!("unused display")
    }
    fn execute_linear_with_cache(
        &self,
        dag: &RenderDag,
        _: &mut RenderCache,
    ) -> Result<Vec<[f32; 4]>, RenderError> {
        Ok(if self.0 {
            vec![]
        } else {
            vec![
                [f32::NAN, 0.0, 0.0, 1.0];
                (dag.region().pixels[0] * dag.region().pixels[1]) as usize
            ]
        })
    }
}
#[test]
fn linear_boundary_rejects_bad_counts_pixels_and_sink_failure() {
    let snapshot = snapshot(false, false);
    for bad_count in [false, true] {
        let error = render_frame_linear_tiles(
            &snapshot,
            &[],
            &BadLinear(bad_count),
            request(),
            &mut |_, _, _| panic!("invalid tile must not reach sink"),
        )
        .unwrap_err();
        assert!(matches!(error, RenderError::InvalidInput(_)));
    }
    let error =
        render_frame_linear_tiles(&snapshot, &[], &LinearOnly, request(), &mut |_, _, _| {
            Err(RenderError::UnsupportedFeature("sink failure".into()))
        })
        .unwrap_err();
    assert!(matches!(error, RenderError::UnsupportedFeature(message) if message == "sink failure"));
}

#[test]
#[ignore = "actual Metal linear boundary and transfer accounting"]
fn actual_gpu_linear_matches_pair_with_halo_and_retains_validation() {
    let reference = snapshot(false, false);
    let mut scene = build_scene_ir(&reference, request().time, &[]).unwrap();
    scene.nodes[0]
        .effects
        .push(ResolvedEffect::AffineGaussianBlur {
            sigma: 3.0,
            linear: [[1.0, 0.0], [0.0, 1.0]],
        });
    let tile = OutputRegion {
        origin: [0.0, 0.0],
        extent: [32.0, 32.0],
        pixels: [32, 32],
    };
    let dag = build_render_dag(&scene, reference.profile(), tile).unwrap();
    assert_ne!(dag.execution_region(), dag.region());
    let gpu = GpuContext::new().expect("actual GPU required; no fallback");
    let mut cache = RenderCache::new(CacheConfig::default());
    let both = gpu.execute(&dag).unwrap();
    let linear = gpu.execute_linear_with_cache(&dag, &mut cache).unwrap();
    assert_eq!(linear, both.linear);
    let transfers = gpu.transfer_stats().unwrap();
    let size = dag.execution_region().pixels;
    // wgpu readback copies include the 256-byte padded row pitch.
    let readback_bytes = (u64::from(size[0]) * 8).div_ceil(256) * 256 * u64::from(size[1]);
    assert_eq!(transfers.gpu_readback_operations, 2);
    assert_eq!(transfers.gpu_wait_operations, 1);
    assert_eq!(transfers.gpu_readback_bytes, readback_bytes + 4);
    let warm_both = gpu.execute(&dag).unwrap();
    let both_transfers = gpu.transfer_stats().unwrap();
    assert_eq!(warm_both.linear, linear);
    assert_eq!(both_transfers.gpu_readback_operations, 3);
    assert_eq!(both_transfers.gpu_wait_operations, 3);
    assert_eq!(both_transfers.gpu_readback_bytes, readback_bytes * 2 + 4);
    assert_eq!(
        both_transfers.gpu_compute_dispatches,
        transfers.gpu_compute_dispatches + 1
    );
    for temporal in [false, true] {
        let snapshot = snapshot(false, temporal);
        let request = request();
        let both = render_frame(&snapshot, &[], &gpu, request).unwrap();
        let (linear, metadata) = assemble(&snapshot, &gpu, request);
        assert_eq!(linear, both.pixels.linear);
        let transfers = metadata.transfer_stats.unwrap();
        let samples = metadata
            .temporal
            .as_ref()
            .map_or(1, |m| m.samples.len() as u64);
        assert_eq!(transfers.gpu_readback_operations, 2 * 2 * samples);
        assert_eq!(transfers.gpu_wait_operations, 2 * samples);
    }
}

#[test]
#[ignore = "actual Metal staging reuse and detached CPU consumer"]
fn completed_staging_is_reused_without_mutating_retained_cpu_pixels() {
    let reference = snapshot(false, false);
    let scene = build_scene_ir(&reference, request().time, &[]).unwrap();
    let tile = OutputRegion {
        origin: [0.0; 2],
        extent: [32.0; 2],
        pixels: [32, 32],
    };
    let dag = build_render_dag(&scene, reference.profile(), tile).unwrap();
    let gpu = GpuContext::new().unwrap();
    let diagnostics = ExecutionDiagnosticsCollector::enabled();
    let _scope = diagnostics.activate();
    let retained = gpu
        .execute_linear_with_cache(&dag, &mut RenderCache::new(CacheConfig::default()))
        .unwrap();
    let original = retained.clone();
    let allocation_count = diagnostics.snapshot().unwrap().counts["gpu_staging_buffer_allocations"];
    let mut different_scene = scene;
    different_scene.nodes[0].opacity = 0.25;
    let different = build_render_dag(&different_scene, reference.profile(), tile).unwrap();
    let second = gpu
        .execute_linear_with_cache(&different, &mut RenderCache::new(CacheConfig::default()))
        .unwrap();
    assert_eq!(retained, original);
    assert_ne!(second, original);
    let measurement = diagnostics.snapshot().unwrap();
    assert_eq!(
        measurement.counts["gpu_staging_buffer_allocations"],
        allocation_count
    );
    assert_eq!(measurement.counts["gpu_staging_buffer_reuses"], 2);
    assert!(measurement.peak_payload_bytes["gpu_staging_pool"] <= 16 * 1024 * 1024);
    gpu.clear_texture_cache().unwrap();
    assert_eq!(
        gpu.allocation_stats().readback_buffers.live_payload_bytes,
        0
    );
}
