use crate::*;
use kronello_render::{CacheCapacity, OutputRegion, RasterCacheKey};
fn scene(size: [u32; 2]) -> DrawScene {
    let pixels = (0..size[1])
        .flat_map(|y| {
            (0..size[0]).map(move |x| {
                [
                    ((x % 13) as f32 / 26.0),
                    ((y % 11) as f32 / 22.0),
                    0.125,
                    0.5,
                ]
            })
        })
        .collect();
    DrawScene {
        nodes: vec![
            DrawNode::Raster(pixels),
            DrawNode::Effect {
                source: 0,
                effect: PixelEffect::GaussianBlur { sigma: [2.0; 2] },
            },
            DrawNode::Group {
                children: vec![1],
                opacity: 0.75,
            },
        ],
        roots: vec![2],
    }
}
const DISPLAY: OutputTransform = OutputTransform {
    space: InputSpace::Srgb,
    alpha: OutputAlpha::Straight,
};
fn keys(gpu: &GpuContext, size: [u32; 2]) -> Vec<Option<RasterCacheKey>> {
    (0..3)
        .map(|node| {
            Some(
                RasterCacheKey::external_source(
                    &format!("perf001-pattern-blur2-opacity075-v1-node{node}"),
                    OutputRegion {
                        origin: [0.0; 2],
                        extent: size.map(f64::from),
                        pixels: size,
                    },
                    kronello_model::ColorSpace::LinearRec709,
                    &gpu.strict_namespace(),
                )
                .unwrap(),
            )
        })
        .collect()
}
fn legacy(
    gpu: &GpuContext,
    size: RenderSize,
    scene: &DrawScene,
    keys: Option<&[Option<RasterCacheKey>]>,
) -> SceneFramePair {
    let linear = if let Some(keys) = keys {
        gpu.render_scene_cached(size, scene, WorkingSpace::LinearRec709, keys, false, None)
            .unwrap()
    } else {
        gpu.render_scene(size, scene, WorkingSpace::LinearRec709)
            .unwrap()
    };
    let (display, display_transfers) = if let Some(keys) = keys {
        let output = gpu
            .render_scene_cached(
                size,
                scene,
                WorkingSpace::LinearRec709,
                keys,
                false,
                Some(DISPLAY),
            )
            .unwrap();
        (output.pixels, output.transfers)
    } else {
        let output = gpu
            .render_scene_output(size, scene, WorkingSpace::LinearRec709, DISPLAY)
            .unwrap();
        (output.pixels, output.transfers)
    };
    let mut transfers = linear.transfers;
    transfers.accumulate(&display_transfers);
    SceneFramePair {
        linear: linear.pixels,
        display,
        transfers,
    }
}
#[test]
#[ignore = "actual GPU fusion correctness and concrete ownership ledger"]
fn perf001_fused_graph_matches_doublepass_and_tracks_ownership() {
    let gpu = GpuContext::new().unwrap();
    gpu.configure_cache(GpuCacheConfig {
        textures: CacheCapacity {
            entries: 0,
            bytes: 0,
        },
        pool: CacheCapacity {
            entries: 0,
            bytes: 0,
        },
        disk: None,
    })
    .unwrap();
    let size = RenderSize::pixels(64, 36);
    let scene = scene(size.output_resolution);
    let before = legacy(&gpu, size, &scene, None);
    gpu.reset_allocation_peaks().unwrap();
    let after = gpu
        .render_scene_pair(size, &scene, WorkingSpace::LinearRec709, DISPLAY)
        .unwrap();
    assert_eq!(before.linear, after.linear);
    assert_eq!(before.display, after.display);
    assert_eq!(before.transfers.gpu_wait_operations, 4);
    assert_eq!(after.transfers.gpu_wait_operations, 3);
    assert_eq!(after.transfers.gpu_copy_bytes, 0);
    assert!(after.transfers.gpu_compute_dispatches < before.transfers.gpu_compute_dispatches);
    let allocation = gpu.allocation_stats();
    assert_eq!(allocation.live_owned_payload_bytes, 0);
    assert!(allocation.peak_owned_payload_bytes > 64 * 36 * 8);
    assert_eq!(allocation.output_copies.acquisitions, 0);
    assert_eq!(allocation.node_peaks.len(), 3);
    let lease = gpu.acquire_surface([8, 8]).unwrap();
    let copy = lease.clone();
    assert_eq!(
        gpu.allocation_stats()
            .graph_and_cache_surfaces
            .live_payload_bytes,
        512
    );
    drop(lease);
    assert_eq!(
        gpu.allocation_stats()
            .graph_and_cache_surfaces
            .live_payload_bytes,
        512
    );
    drop(copy);
    assert_eq!(
        gpu.allocation_stats()
            .graph_and_cache_surfaces
            .live_payload_bytes,
        0
    );
    eprintln!(
        "PERF001 correctness before={:?} after={:?} allocation={allocation:?}",
        before.transfers, after.transfers
    );
}
#[test]
#[ignore = "release actual GPU measurement; requires coordinated quiet host window"]
fn perf001_gpu_fusion_measurement() {
    assert!(
        !std::hint::black_box(cfg!(debug_assertions)),
        "use cargo test --release for accepted timing"
    );
    let gpu = GpuContext::new().unwrap();
    let mut results = Vec::new();
    for (name, dimensions) in [
        ("proxy", [320, 180]),
        ("preview", [640, 360]),
        ("full", [1920, 1080]),
    ] {
        let size = RenderSize::pixels(dimensions[0], dimensions[1]);
        let scene = scene(dimensions);
        let identity = keys(&gpu, dimensions);
        for warm in [false, true] {
            let config = if warm {
                GpuCacheConfig::default()
            } else {
                GpuCacheConfig {
                    textures: CacheCapacity {
                        entries: 0,
                        bytes: 0,
                    },
                    pool: CacheCapacity {
                        entries: 0,
                        bytes: 0,
                    },
                    disk: None,
                }
            };
            let keys = warm.then_some(identity.as_slice());
            let mut oracle = None;
            for fused in [false, true] {
                gpu.configure_cache(config.clone()).unwrap();
                if warm {
                    gpu.render_scene_pair_cached(
                        size,
                        &scene,
                        WorkingSpace::LinearRec709,
                        keys,
                        false,
                        DISPLAY,
                    )
                    .unwrap();
                }
                let mut samples = Vec::new();
                for _ in 0..21 {
                    gpu.reset_allocation_peaks().unwrap();
                    let start = std::time::Instant::now();
                    let frame = if fused {
                        gpu.render_scene_pair_cached(
                            size,
                            &scene,
                            WorkingSpace::LinearRec709,
                            keys,
                            false,
                            DISPLAY,
                        )
                        .unwrap()
                    } else {
                        legacy(&gpu, size, &scene, keys)
                    };
                    let elapsed = start.elapsed().as_secs_f64() * 1000.0;
                    let fingerprint = pixel_hash(&frame);
                    if let Some(expected) = &oracle {
                        assert_eq!(&fingerprint, expected);
                    } else {
                        oracle = Some(fingerprint.clone());
                    }
                    samples.push(serde_json::json!({"milliseconds": elapsed, "pixel_hash":fingerprint, "transfers":frame.transfers.render_stats(), "allocations":gpu.allocation_stats()}));
                }
                results.push(serde_json::json!({"size":name,"dimensions":dimensions,"warm":warm,"fused":fused,"samples":samples}));
            }
        }
    }
    use sha2::{Digest, Sha256};
    let mut source = Sha256::new();
    for bytes in [
        include_bytes!("renderer.rs").as_slice(),
        include_bytes!("scene_gpu.rs").as_slice(),
        include_bytes!("allocation.rs").as_slice(),
        include_bytes!("perf_tests.rs").as_slice(),
        include_bytes!("../../../Cargo.lock").as_slice(),
    ] {
        source.update(bytes);
    }
    let revision = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let status = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .unwrap();
    let binary = std::fs::read(std::env::current_exe().unwrap()).unwrap();
    let result = serde_json::json!({"revision":String::from_utf8(revision.stdout).unwrap().trim(),"dirty":!status.stdout.is_empty(),"source_sha256":format!("{:x}",source.finalize()),"binary_sha256":format!("{:x}",Sha256::digest(binary)),"schema_version":1,"adapter":format!("{:?}",gpu.adapter_info),"samples_per_case":21,"disk_cache":false,"driver_private_memory":"unknown","results":results});
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/evidence/perf-001");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("gpu-fusion.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
}
fn pixel_hash(frame: &SceneFramePair) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    for pixel in frame.linear.iter().chain(&frame.display) {
        for channel in pixel {
            hash.update(channel.to_bits().to_le_bytes());
        }
    }
    format!("{:x}", hash.finalize())
}

#[test]
#[ignore = "actual resident cached texture must retain the same ownership accounting guard"]
fn perf001_cached_resident_texture_retains_allocation_until_eviction() {
    let gpu = GpuContext::new().unwrap();
    let size = RenderSize::pixels(8, 8);
    let keys = vec![Some(
        RasterCacheKey::external_source(
            "resident-zero-v1",
            OutputRegion {
                origin: [0.0; 2],
                extent: [8.0; 2],
                pixels: [8; 2],
            },
            kronello_model::ColorSpace::LinearRec709,
            &gpu.strict_namespace(),
        )
        .unwrap(),
    )];
    {
        let image = ResidentImage::allocate(&gpu, [8, 8], WorkingSpace::LinearRec709).unwrap();
        let scene = DrawScene {
            nodes: vec![DrawNode::GpuRaster(image)],
            roots: vec![0],
        };
        gpu.render_scene_pair_cached(
            size,
            &scene,
            WorkingSpace::LinearRec709,
            Some(&keys),
            false,
            DISPLAY,
        )
        .unwrap();
        assert_eq!(
            gpu.allocation_stats().resident_inputs.live_payload_bytes,
            512
        );
    }
    assert_eq!(
        gpu.allocation_stats().resident_inputs.live_payload_bytes,
        512
    );
    gpu.clear_texture_cache().unwrap();
    assert_eq!(gpu.allocation_stats().resident_inputs.live_payload_bytes, 0);
}

#[test]
#[ignore = "requires an actual GPU adapter"]
fn perf001_singleton_output_root_elision_is_bit_exact() {
    let gpu = GpuContext::new().unwrap();
    let size = RenderSize::pixels(8, 1);
    let pixels = vec![
        [0.0, -0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 0.0],
        [-0.0, 0.0, -0.0, 1.0],
        [0.00000006, 0.0, 0.0, 0.00000006],
        [0.25, 0.125, 0.0625, 0.5],
        [2.0, 1.0, 0.5, 1.0],
        [-0.25, 0.25, 0.5, 1.0],
        [0.5, 0.5, 0.5, 1.0],
    ];
    for working in [WorkingSpace::LinearRec709, WorkingSpace::LinearRec2020] {
        for alpha in [OutputAlpha::Straight, OutputAlpha::Premultiplied] {
            for space in [
                InputSpace::Srgb,
                InputSpace::LinearRec709,
                InputSpace::LinearRec2020,
            ] {
                for with_effect in [false, true] {
                    let mut child = DrawScene {
                        nodes: vec![
                            DrawNode::Raster(pixels.clone()),
                            DrawNode::Raster(vec![[0.0, 0.0, 0.0, 0.5]; 8]),
                            DrawNode::Masked {
                                source: 0,
                                matte: 1,
                                kind: MaskKind::Alpha,
                            },
                            DrawNode::Effect {
                                source: 2,
                                effect: PixelEffect::GaussianBlur { sigma: [0.5; 2] },
                            },
                        ],
                        roots: vec![3],
                    };
                    if !with_effect {
                        child.nodes.truncate(1);
                        child.roots = vec![0];
                    }
                    let root = child.roots[0];
                    let mut old = child.clone();
                    old.nodes.push(DrawNode::Group {
                        children: vec![root],
                        opacity: 1.0,
                    });
                    old.roots = vec![old.nodes.len() - 1];
                    let transform = OutputTransform { space, alpha };
                    let before = gpu
                        .render_scene_pair(size, &old, working, transform)
                        .unwrap();
                    let after = gpu
                        .render_scene_pair(size, &child, working, transform)
                        .unwrap();
                    for (a, b) in before
                        .linear
                        .iter()
                        .chain(&before.display)
                        .zip(after.linear.iter().chain(&after.display))
                    {
                        assert_eq!(a.map(f32::to_bits), b.map(f32::to_bits));
                    }
                    assert_eq!(
                        before.transfers.gpu_compute_dispatches,
                        after.transfers.gpu_compute_dispatches + 2
                    );
                }
            }
        }
    }
    let hidden = DrawScene {
        nodes: vec![DrawNode::Raster(vec![[0.5, -0.25, 0.125, 0.0]; 8])],
        roots: vec![0],
    };
    let mut old = hidden.clone();
    old.nodes.push(DrawNode::Group {
        children: vec![0],
        opacity: 1.0,
    });
    old.roots = vec![1];
    for scene in [&old, &hidden] {
        assert!(matches!(
            gpu.render_scene_pair(size, scene, WorkingSpace::LinearRec709, DISPLAY),
            Err(GpuError::InvalidInput("invalid raster input"))
        ));
    }
}
