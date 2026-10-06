use kronello_gpu::{GpuCacheConfig, GpuContext};
use kronello_model::{DocumentObject, Project, ResolvedEffect};
use kronello_render::{
    OutputRegion, RasterCacheKey, RenderBackend, RenderProfile, RenderSnapshot, build_render_dag,
    build_scene_ir,
};
use kronello_time::Time;
fn fixture() -> (kronello_render::SceneIr, RenderProfile) {
    let mut document: Project =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    let DocumentObject::Known(composition) = &mut document.compositions[0] else {
        panic!()
    };
    composition.nodes.truncate(1);
    composition.root_nodes.truncate(1);
    let id = composition.id;
    document.texts.clear();
    let profile = RenderProfile::default();
    let snapshot = RenderSnapshot::new(&document, id, 1, profile).unwrap();
    let mut scene = build_scene_ir(&snapshot, Time::ZERO, &[]).unwrap();
    scene.nodes[0]
        .effects
        .push(ResolvedEffect::AffineGaussianBlur {
            sigma: 24.0,
            linear: [[1.0, 0.0], [0.0, 1.0]],
        });
    (scene, profile)
}
#[test]
fn semantic_empty_composite_keys_pin_region_color_and_backend() {
    let (mut scene, profile) = fixture();
    scene.nodes.clear();
    let region = OutputRegion {
        origin: [0.0; 2],
        extent: [1920.0, 1080.0],
        pixels: [32, 18],
    };
    let dag = build_render_dag(&scene, profile, region).unwrap();
    let first = RasterCacheKey::for_dag(&dag, "device-a").unwrap();
    assert_ne!(first, RasterCacheKey::for_dag(&dag, "device-b").unwrap());
    let other = build_render_dag(
        &scene,
        profile,
        OutputRegion {
            origin: [10.0, 0.0],
            ..region
        },
    )
    .unwrap();
    assert_ne!(first, RasterCacheKey::for_dag(&other, "device-a").unwrap());
    let other = build_render_dag(
        &scene,
        RenderProfile {
            working_space: kronello_model::ColorSpace::LinearRec2020,
            ..profile
        },
        region,
    )
    .unwrap();
    assert_ne!(first, RasterCacheKey::for_dag(&other, "device-a").unwrap());
}
#[test]
#[ignore = "actual GPU semantic DAG texture cache and effect tile/halo reuse"]
fn actual_gpu_dag_effect_tile_halo_and_preview_cache() {
    let gpu = GpuContext::new().unwrap();
    gpu.configure_cache(GpuCacheConfig::default()).unwrap();
    let (scene, profile) = fixture();
    let region = OutputRegion {
        origin: [0.0; 2],
        extent: [1920.0, 1080.0],
        pixels: [32, 18],
    };
    let full = build_render_dag(&scene, profile, region).unwrap();
    let cold = gpu.execute(&full).unwrap();
    let cold_stats = gpu.transfer_stats().unwrap();
    let warm = gpu.execute(&full).unwrap();
    let warm_stats = gpu.transfer_stats().unwrap();
    assert_eq!(cold.linear, warm.linear);
    assert_eq!(cold.display, warm.display);
    assert!(warm_stats.cpu_upload_control_bytes < cold_stats.cpu_upload_control_bytes);
    let mut tiled = vec![[0.0; 4]; 32 * 18];
    for half in 0..2 {
        let tile = OutputRegion {
            origin: [960.0 * f64::from(half), 0.0],
            extent: [960.0, 1080.0],
            pixels: [16, 18],
        };
        let dag = build_render_dag(&scene, profile, tile).unwrap();
        assert_ne!(
            dag.execution_region(),
            dag.region(),
            "effect must request a real halo"
        );
        let misses = gpu.cache_stats().textures.misses;
        let first = gpu.execute(&dag).unwrap();
        assert!(gpu.cache_stats().textures.misses > misses);
        let second = gpu.execute(&dag).unwrap();
        assert_eq!(first.linear, second.linear);
        for row in 0..18 {
            let start = row * 32 + half as usize * 16;
            tiled[start..start + 16].copy_from_slice(&first.linear[row * 16..row * 16 + 16]);
        }
    }
    let max = tiled
        .iter()
        .zip(&cold.linear)
        .flat_map(|(a, b)| a.iter().zip(b).map(|(a, b)| (a - b).abs()))
        .fold(0.0f32, f32::max);
    assert!(
        max <= 1.0 / 1024.0,
        "tile/full effect pixels mismatch {max}"
    );
    // Native preview uses this same persistent context but cannot read/write
    // intermediate disk pixels. A retained raw preview surface is detached from
    // pooling, so a later render cannot overwrite a consumer's output.
    let held = gpu.preview_texture(&full).unwrap();
    let hits = gpu.cache_stats().textures.hits;
    let another = gpu.preview_texture(&full).unwrap();
    assert!(gpu.cache_stats().textures.hits > hits);
    assert_eq!(held.width(), another.width());
    assert_ne!(
        held, another,
        "caller-owned preview texture must not be recycled"
    );
    let counters = gpu.transfer_stats().unwrap();
    assert_eq!(counters.gpu_readback_bytes, 4);
    assert_eq!(counters.cpu_upload_pixel_bytes, 0);
    eprintln!(
        "CACHE003 actual DAG adapter={:?} cold={cold_stats:?} warm={warm_stats:?} tileMax={max} stats={:?}",
        gpu.adapter_info,
        gpu.cache_stats()
    );
}

#[test]
#[ignore = "actual GPU request-local transfer totals across tiles and temporal cache"]
fn perf001_request_transfers_sum_tiles_samples_and_zero_work_hits() {
    use kronello_render::{
        CacheConfig, CutPolicy, FrameRequest, RenderCache, TemporalSettings, render_frame,
        render_frame_tiles, render_frame_with_cache,
    };
    let mut document: Project =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    let DocumentObject::Known(composition) = &mut document.compositions[0] else {
        panic!()
    };
    composition.nodes.truncate(1);
    composition.root_nodes.truncate(1);
    let id = composition.id;
    document.texts.clear();
    let gpu = GpuContext::new().unwrap();
    let snapshot = RenderSnapshot::new(&document, id, 1, RenderProfile::default()).unwrap();
    let request = FrameRequest {
        time: Time::new(1, 2).unwrap(),
        region: OutputRegion {
            origin: [0.0; 2],
            extent: [1920.0, 1080.0],
            pixels: [513, 1],
        },
    };
    let before = gpu.transfer_stats_total().unwrap();
    let frame = render_frame(&snapshot, &[], &gpu, request).unwrap();
    let transfers = frame.metadata.transfer_stats.unwrap();
    assert_eq!(
        transfers,
        gpu.transfer_stats_total().unwrap().since(&before)
    );
    assert_eq!(transfers.gpu_readback_operations, 6);
    assert_eq!(transfers.gpu_wait_operations, 6);
    assert_eq!(transfers.gpu_copy_bytes, 0);
    let mut tiles = 0;
    let streamed = render_frame_tiles(&snapshot, &[], &gpu, request, &mut |_, _, _| {
        tiles += 1;
        Ok(())
    })
    .unwrap();
    assert_eq!(tiles, 2);
    assert_eq!(streamed.transfer_stats.unwrap().gpu_readback_operations, 6);
    let profile = RenderProfile {
        temporal: Some(TemporalSettings {
            frame_rate: kronello_time::FrameRate::new(24, 1).unwrap(),
            shutter_angle: Time::new(180, 1).unwrap(),
            shutter_phase: Time::new(-1, 4).unwrap(),
            samples: 3,
            cut_policy: CutPolicy::AvoidCrossing,
        }),
        ..RenderProfile::default()
    };
    let snapshot = RenderSnapshot::new(&document, id, 1, profile).unwrap();
    let request = FrameRequest {
        region: OutputRegion {
            pixels: [32, 18],
            ..request.region
        },
        ..request
    };
    let mut cache = RenderCache::new(CacheConfig::default());
    let temporal = render_frame_with_cache(&snapshot, &[], &gpu, request, &mut cache).unwrap();
    assert_eq!(
        temporal.metadata.resource_cache_stats,
        gpu.resource_cache_stats()
    );
    let samples = temporal.metadata.temporal.as_ref().unwrap().samples.len() as u64;
    assert_eq!(
        temporal
            .metadata
            .transfer_stats
            .unwrap()
            .gpu_readback_operations,
        samples * 3
    );
    let warm = render_frame_with_cache(&snapshot, &[], &gpu, request, &mut cache).unwrap();
    assert_eq!(warm.pixels, temporal.pixels);
    assert_eq!(
        warm.metadata.transfer_stats.unwrap(),
        kronello_render::RenderTransferStats::default()
    );
}

#[test]
#[ignore = "actual GPU shared-context concurrent requests must serialize observed transfers"]
fn perf001_concurrent_shared_context_keeps_request_transfers_independent() {
    use kronello_render::{FrameRequest, render_frame};
    use std::sync::{Arc, mpsc};
    let mut document: Project =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    let DocumentObject::Known(composition) = &mut document.compositions[0] else {
        panic!()
    };
    composition.nodes.truncate(1);
    composition.root_nodes.truncate(1);
    let id = composition.id;
    document.texts.clear();
    let snapshot = RenderSnapshot::new(&document, id, 1, RenderProfile::default()).unwrap();
    let gpu = Arc::new(GpuContext::new().unwrap());
    let scope = gpu.begin_observation_scope().unwrap().unwrap();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();
    let peer_gpu = gpu.clone();
    let peer_snapshot = snapshot.clone();
    let request = FrameRequest {
        time: Time::new(1, 2).unwrap(),
        region: OutputRegion {
            origin: [0.0; 2],
            extent: [1920.0, 1080.0],
            pixels: [32, 18],
        },
    };
    let peer = std::thread::spawn(move || {
        ready_tx.send(()).unwrap();
        let result = render_frame(&peer_snapshot, &[], &*peer_gpu, request);
        result_tx.send(result).unwrap();
    });
    ready_rx.recv().unwrap();
    assert!(matches!(
        result_rx.recv_timeout(std::time::Duration::from_millis(50)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    let local = render_frame(
        &snapshot,
        &[],
        &*gpu,
        FrameRequest {
            region: OutputRegion {
                pixels: [513, 1],
                ..request.region
            },
            ..request
        },
    )
    .unwrap();
    assert_eq!(
        local
            .metadata
            .transfer_stats
            .unwrap()
            .gpu_readback_operations,
        6
    );
    drop(scope);
    let other = result_rx
        .recv_timeout(std::time::Duration::from_secs(30))
        .unwrap()
        .unwrap();
    peer.join().unwrap();
    assert_eq!(
        other
            .metadata
            .transfer_stats
            .unwrap()
            .gpu_readback_operations,
        3
    );
    assert_eq!(
        gpu.transfer_stats_total().unwrap().gpu_readback_operations,
        9
    );
}
