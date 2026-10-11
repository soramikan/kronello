//! Paired real project compilation/layout and GPU DAG probe.
use kronello_gpu::{GpuCacheConfig, GpuContext, SceneSurfaceMode};
use kronello_model::{DocumentObject, Project, ResolvedEffect};
use kronello_render::{
    OutputRegion, RenderBackend, RenderProfile, RenderSnapshot, build_render_dag, build_scene_ir,
};
use kronello_text::{FontData, pin_font};
use kronello_time::Time;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{io::Write, time::Instant};

fn main() {
    if cfg!(debug_assertions) {
        panic!("use --release");
    }
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 3, "PROJECT_JSON NEW_OUTPUT_JSON");
    let bytes = std::fs::read(&args[1]).unwrap();
    let document: Project = serde_json::from_slice(&bytes).unwrap();
    let DocumentObject::Known(composition) = &document.compositions[0] else {
        panic!()
    };
    let font =
        std::fs::read(kronello_testkit::resolve_fixture("noto-sans-cjk-jp").unwrap()).unwrap();
    let identity = pin_font(&font, 0).unwrap();
    let fonts = [FontData {
        identity: &identity,
        bytes: &font,
    }];
    let profile = RenderProfile::default();
    let snapshot = RenderSnapshot::new(&document, composition.id, 1, profile).unwrap();
    let mut gpu = GpuContext::new().unwrap();
    let mut config = GpuCacheConfig::default();
    config.textures.entries = 0;
    config.textures.bytes = 0;
    config.disk = None;
    gpu.configure_cache(config).unwrap();
    let mut trials = Vec::new();
    for iteration in 0..22 {
        let begin = Instant::now();
        let mut scene =
            build_scene_ir(&snapshot, Time::new(iteration, 24).unwrap(), &fonts).unwrap();
        // Explicit derived-scene prescription; the source project stays inspectable.
        for (i, node) in scene.nodes.iter_mut().enumerate() {
            node.world_transform.0[0][2] += iteration as f64 / 8.;
            if i % 8 == 0 {
                node.effects.push(ResolvedEffect::AffineGaussianBlur {
                    sigma: 2.25,
                    linear: [[1., 0.], [0., 1.]],
                });
            }
        }
        let dag = build_render_dag(
            &scene,
            profile,
            OutputRegion {
                origin: [0.; 2],
                extent: [960., 540.],
                pixels: [960, 540],
            },
        )
        .unwrap();
        let preparation_ms = begin.elapsed().as_secs_f64() * 1000.;
        let order = if iteration % 2 == 0 {
            [
                SceneSurfaceMode::FullCanvas,
                SceneSurfaceMode::LocalCoverageAndLastUse,
            ]
        } else {
            [
                SceneSurfaceMode::LocalCoverageAndLastUse,
                SceneSurfaceMode::FullCanvas,
            ]
        };
        let mut pair = Vec::new();
        for mode in order {
            gpu = gpu.with_scene_surface_mode(mode);
            gpu.reset_allocation_peaks().unwrap();
            let begin = Instant::now();
            let output = gpu.execute(&dag).unwrap();
            let elapsed_ms = begin.elapsed().as_secs_f64() * 1000.;
            let mut hash = Sha256::new();
            for pixel in output.linear.iter().chain(&output.display) {
                for value in pixel {
                    hash.update(value.to_bits().to_le_bytes());
                }
            }
            let digest = format!("{:x}", hash.finalize());
            pair.push(digest.clone());
            trials.push(json!({"iteration":iteration,"mode":format!("{mode:?}"),"elapsed_ms":elapsed_ms,"preparation_ms":preparation_ms,"sha256":digest,"dag_nodes":dag.nodes().len(),"allocation":gpu.allocation_stats(),"transfers":gpu.transfer_stats()}));
        }
        assert_eq!(pair[0], pair[1], "full planes at {iteration}");
    }
    let mut summary = serde_json::Map::new();
    for mode in ["FullCanvas", "LocalCoverageAndLastUse"] {
        let mut times: Vec<_> = trials
            .iter()
            .filter(|t| t["iteration"].as_i64().unwrap() > 0 && t["mode"] == mode)
            .map(|t| t["elapsed_ms"].as_f64().unwrap())
            .collect();
        times.sort_by(f64::total_cmp);
        summary.insert(
            mode.into(),
            json!({"warm_p50_ms":times[10],"warm_p95_ms":times[19],"samples":times.len()}),
        );
    }
    let report = json!({"scope":"64 authored nodes, 48 shapes/16 Japanese text; 8 derived Gaussian effects; integer output lattice with fractional moving positions; actual model/font/layout/DAG; GPU execute includes validation and full linear/display readback, preparation separately timed; one device alternates order; cold1/warm21; semantic textures disabled, bounded default pool","quiet_host":false,"project_sha256":format!("{:x}",Sha256::digest(bytes)),"font_sha256":format!("{:x}",Sha256::digest(font)),"summary":summary,"trials":trials});
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[2])
        .unwrap();
    file.write_all(serde_json::to_string_pretty(&report).unwrap().as_bytes())
        .unwrap();
}
