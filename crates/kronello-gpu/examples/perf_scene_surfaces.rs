//! Real GPU full-scene payload and time probe for coverage/shared surfaces.
use kronello_gpu::*;
use kronello_model::{Color, ColorSpace};
use kronello_render::PixelEffect;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{io::Write, time::Instant};
fn path(index: usize, offset: f32, size: [u32; 2]) -> DrawNode {
    let x = ((index * 43) % size[0] as usize) as f32 + offset - 2.375;
    let y = ((index * 29) % size[1] as usize) as f32 + offset - 1.125;
    DrawNode::Path(PathDraw {
        stroke_geometry: None,
        fill_gradient: None,
        stroke_gradient: None,
        paint_transform: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        contours: vec![Contour {
            points: vec![
                [x, y],
                [x + 17.625, y + 1.375],
                [x + 14.25, y + 13.875],
                [x - 1.625, y + 11.125],
            ],
            closed: true,
        }],
        fill: Some(Fill {
            paint: Paint {
                rgba: [0.2 + (index % 4) as f32 * 0.35, 0.4, 0.8, 0.625],
                space: InputSpace::LinearRec709,
            },
            rule: FillRule::Nonzero,
        }),
        stroke: None,
    })
}
fn scene(size: [u32; 2], shared: bool, offset: f32) -> DrawScene {
    let count = if shared { 32 } else { 200 };
    let mut nodes: Vec<_> = (0..count).map(|i| path(i, offset, size)).collect();
    let mut roots: Vec<_> = (0..count).collect();
    if shared {
        for i in 0..count {
            let id = nodes.len();
            nodes.push(DrawNode::Masked {
                source: i,
                matte: (i + 7) % count,
                kind: if i % 2 == 0 {
                    MaskKind::Alpha
                } else {
                    MaskKind::AlphaInverted
                },
            });
            roots.push(id);
        }
    } else {
        for (i, root) in roots.iter_mut().enumerate().take(10) {
            let id = nodes.len();
            nodes.push(DrawNode::Effect {
                source: i,
                effect: if i % 2 == 0 {
                    PixelEffect::GaussianBlur { sigma: [2.25, 1.5] }
                } else {
                    PixelEffect::DropShadow {
                        sigma: [1.75, 2.5],
                        offset: [3.375, -1.625],
                        color: Color::new(ColorSpace::LinearRec709, [0.25, 0.125, 0.5], 0.5)
                            .unwrap(),
                        opacity: 0.625,
                    }
                },
            });
            *root = id;
        }
        let mut groups = Vec::new();
        for chunk in roots.chunks(20) {
            let id = nodes.len();
            nodes.push(DrawNode::Group {
                children: chunk.to_vec(),
                opacity: 0.875,
            });
            groups.push(id);
        }
        roots = groups;
    }
    DrawScene { nodes, roots }
}
fn main() {
    if cfg!(debug_assertions) {
        panic!("use --release");
    }
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 3, "OUTPUT_JSON before|full|optimized");
    let mode = &args[2];
    assert!(matches!(
        mode.as_str(),
        "before" | "full" | "optimized" | "paired"
    ));
    let mut cases = Vec::new();
    for (size, shared) in [([480, 270], false), ([960, 540], false), ([960, 540], true)] {
        let mut gpu = GpuContext::new()
            .unwrap()
            .with_scene_surface_mode(if mode == "optimized" {
                SceneSurfaceMode::LocalCoverageAndLastUse
            } else {
                SceneSurfaceMode::FullCanvas
            });
        let mut config = GpuCacheConfig::default();
        config.textures.entries = 0;
        config.textures.bytes = 0;
        config.disk = None;
        gpu.configure_cache(config).unwrap();
        let mut trials = Vec::new();
        for iteration in 0..if mode == "paired" { 22 } else { 9 } {
            let order = if mode == "paired" {
                if iteration % 2 == 0 {
                    vec![
                        SceneSurfaceMode::FullCanvas,
                        SceneSurfaceMode::LocalCoverageAndLastUse,
                    ]
                } else {
                    vec![
                        SceneSurfaceMode::LocalCoverageAndLastUse,
                        SceneSurfaceMode::FullCanvas,
                    ]
                }
            } else {
                vec![if mode == "optimized" {
                    SceneSurfaceMode::LocalCoverageAndLastUse
                } else {
                    SceneSurfaceMode::FullCanvas
                }]
            };
            for selected in order {
                gpu = gpu.with_scene_surface_mode(selected);
                let scene = scene(size, shared, iteration as f32 / 8.0);
                gpu.reset_allocation_peaks().unwrap();
                let begin = Instant::now();
                let frame = gpu
                    .render_scene_pair(
                        RenderSize::pixels(size[0], size[1]),
                        &scene,
                        WorkingSpace::LinearRec709,
                        OutputTransform {
                            space: InputSpace::Srgb,
                            alpha: OutputAlpha::Straight,
                        },
                    )
                    .unwrap();
                let elapsed_ms = begin.elapsed().as_secs_f64() * 1000.0;
                let mut digest = Sha256::new();
                for plane in [&frame.linear, &frame.display] {
                    for pixel in plane {
                        for value in pixel {
                            digest.update(value.to_bits().to_le_bytes());
                        }
                    }
                }
                trials.push(json!({"iteration":iteration,"selected":format!("{selected:?}"),"elapsed_ms":elapsed_ms,"linear_and_display_f32_sha256":format!("{:x}",digest.finalize()),"allocation":gpu.allocation_stats(),"transfers":frame.transfers.render_stats(),"resource_cache":gpu.render_cache_stats(),"nodes":scene.nodes.len(),"roots":scene.roots.len()}));
            }
        }
        let mut warm: Vec<_> = trials[1..]
            .iter()
            .map(|t| t["elapsed_ms"].as_f64().unwrap())
            .collect();
        warm.sort_by(f64::total_cmp);
        let mut by_mode = serde_json::Map::new();
        for selected in ["FullCanvas", "LocalCoverageAndLastUse"] {
            let mut times: Vec<_> = trials
                .iter()
                .filter(|trial| {
                    trial["iteration"].as_u64().unwrap() > 0 && trial["selected"] == selected
                })
                .map(|trial| trial["elapsed_ms"].as_f64().unwrap())
                .collect();
            if !times.is_empty() {
                times.sort_by(f64::total_cmp);
                by_mode.insert(selected.into(),json!({"warm_p50_ms":times[times.len()/2],"warm_p95_ms":times[(times.len()*95).div_ceil(100)-1],"samples":times.len()}));
            }
        }
        cases.push(json!({"size":size,"shared":shared,"by_mode":by_mode,"warm_p50_ms":warm[warm.len()/2],"warm_p95_ms":warm[(warm.len()*95).div_ceil(100)-1],"trials":trials}));
    }
    let report = json!({"mode":mode,"quiet_host":false,"scope":"paired mode alternates order each iteration within one device/context, 1 cold pair/21 warm pairs; other modes 1 cold/8 warm; full DrawScene pair; includes upload/compute/sticky status and final linear/display readback; scene construction/full-pixel hashing outside timings; 1 cold/8 warm; animation changes each trial, no semantic texture cache; bounded default pool",
        "source_sha256":{"scene_gpu.rs":format!("{:x}",Sha256::digest(include_bytes!("../src/scene_gpu.rs"))),"scene.wgsl":format!("{:x}",Sha256::digest(include_bytes!("../src/scene.wgsl"))),"scene.rs":format!("{:x}",Sha256::digest(include_bytes!("../src/scene.rs"))),"surface.rs":format!("{:x}",Sha256::digest(include_bytes!("../../kronello-render/src/surface.rs")))},"cases":cases});
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[1])
        .unwrap();
    output
        .write_all(&serde_json::to_vec_pretty(&report).unwrap())
        .unwrap();
}
