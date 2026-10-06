//! Isolated release preview/final latency; no UI presentation is implied.
use kronello_gpu::{GpuCacheConfig, GpuContext};
use kronello_render::{DagNode, RenderBackend};
use kronello_service::{
    BackendSelection, FrameRenderRequest, Request, Response, ResultData, Service,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Write},
    path::PathBuf,
    time::Instant,
};

fn handshake(event: &str, phase: &str) {
    println!("{}", json!({"event":event,"phase":phase}));
    io::stdout().flush().unwrap();
    let mut line = String::new();
    io::stdin().read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "continue");
}
fn nanos(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_nanos()).unwrap()
}
fn summary(mut samples: Vec<u64>) -> Value {
    samples.sort_unstable();
    let n = samples.len();
    json!({"n":n,"percentile_method":"nearest_rank","p50_ns":samples[n.div_ceil(2)-1],"p95_ns":samples[(95*n).div_ceil(100)-1],"samples_ns_sorted":samples})
}
fn gpu() -> GpuContext {
    let context = GpuContext::new().unwrap();
    context.configure_cache(GpuCacheConfig::default()).unwrap();
    context
}
fn native_budget(dag: &kronello_render::RenderDag) -> Value {
    let elided_root = dag.nodes().len().checked_sub(2).filter(|&root| {
        matches!(
            (&dag.nodes()[root], dag.nodes().last()),
            (DagNode::IsolatedComposite { children, opacity }, Some(DagNode::OutputTransform { source, .. }))
                if *source == root && children.len() == 1 && *opacity == 1.0
        )
    });
    let mut nodes = 0_usize;
    let mut groups = 0_usize;
    let mut effects = 0_usize;
    for (index, node) in dag.nodes().iter().enumerate() {
        if Some(index) == elided_root {
            continue;
        }
        match node {
            DagNode::Geometry { .. }
            | DagNode::TextLayout { .. }
            | DagNode::OutputTransform { .. } => continue,
            DagNode::IsolatedComposite { children, .. } => groups += children.len() + 1,
            DagNode::Effect { .. } => effects += 3,
            _ => {}
        }
        nodes += 1;
    }
    let surfaces = nodes + 1 + groups + effects + 3;
    let pixels = dag.execution_region().pixels;
    json!({"dag_nodes":dag.nodes().len(),"draw_nodes":nodes,"elided_output_root":elided_root,"scene_roots":1,"group_extra_surfaces":groups,"effect_extra_surfaces":effects,"surface_count":surfaces,"execution_pixels":pixels,"rgba16f_bytes_per_pixel":8,"admission_surface_estimate_bytes":u64::from(pixels[0])*u64::from(pixels[1])*8*surfaces as u64,"limit_bytes":512*1024*1024})
}
fn report_case(name: &str, report: &mut Value, source_hash: &str, target: &Value) {
    report["source_document_sha256"] = json!(source_hash);
    report["compiled_source_id"] =
        json!(option_env!("KRONELLO_PERF_SOURCE_ID").unwrap_or("unrecorded"));
    report["selected_target"] = target.clone();
    println!(
        "{}",
        json!({"event":"case_report","name":name,"measurements":report})
    );
    io::stdout().flush().unwrap();
}
fn frame_hash(linear: &[[f32; 4]], display: &[[f32; 4]]) -> String {
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 65536];
    for plane in [linear, display] {
        for chunk in plane.chunks(4096) {
            for (index, pixel) in chunk.iter().enumerate() {
                for (component, value) in pixel.iter().enumerate() {
                    let offset = index * 16 + component * 4;
                    buffer[offset..offset + 4].copy_from_slice(&value.to_bits().to_le_bytes());
                }
            }
            hash.update(&buffer[..chunk.len() * 16]);
        }
    }
    format!("{:x}", hash.finalize())
}
fn main() {
    if cfg!(debug_assertions) {
        panic!("use cargo build --release");
    }
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 4, "source.kronello output-directory font.otf");
    let source = PathBuf::from(&args[1]);
    let basic =
        source.file_name().and_then(|name| name.to_str()) == Some("ffi-preview.project.json");
    let scene_kind = if basic {
        "basic FFI animated shape"
    } else {
        "actual lower-third complex sequence"
    };
    let project = if source
        .extension()
        .is_some_and(|extension| extension == "json")
    {
        let document: Value = serde_json::from_slice(&std::fs::read(&source).unwrap()).unwrap();
        let directory = PathBuf::from(&args[2]);
        std::fs::create_dir_all(&directory).unwrap();
        let project = directory.join("reference.kronello");
        let response = Service::new(BackendSelection::CpuReference).execute_json(
            &json!({"operation":"project.create","project":project,"document":document})
                .to_string(),
        );
        assert!(matches!(response, Response::Success { .. }), "{response:?}");
        project
    } else {
        source.clone()
    };
    let response = Service::new(BackendSelection::CpuReference)
        .execute_json(&json!({"operation":"project.export","project":project}).to_string());
    assert!(matches!(response, Response::Success { .. }), "{response:?}");
    let exported = serde_json::to_value(response).unwrap()["result"]["value"]["document"].clone();
    let source_document_sha256 = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&exported).unwrap())
    );
    let mut fonts = BTreeMap::new();
    for text in exported["texts"].as_array().into_iter().flatten() {
        for style in text["styles"].as_array().unwrap() {
            let font = &style["font"];
            fonts.insert(font.to_string(), json!({"identity":font,"path":args[3]}));
        }
    }
    let (target, extent) =
        if let Some(sequence) = exported["sequences"].as_array().and_then(|s| s.first()) {
            (
                json!({"target":{"kind":"sequence","sequence":sequence["id"]}}),
                sequence["extent"].clone(),
            )
        } else {
            (
                json!({"composition":exported["compositions"][0]["id"]}),
                exported["compositions"][0]["design_extent"].clone(),
            )
        };
    let mut cases = BTreeMap::new();
    let mut hashes = BTreeMap::new();
    let mut adapter = None;
    let validation_only = std::env::var_os("KRONELLO_PERF_PROXY_CHECK").is_some();
    let resolutions = if basic {
        vec![("proxy", [960, 540]), ("full_4k", [3840, 2160])]
    } else {
        vec![
            ("proxy", [960, 540]),
            ("full_1080", [1920, 1080]),
            ("final_4k", [3840, 2160]),
        ]
    };
    for (resolution, pixels) in resolutions {
        if validation_only && resolution != "proxy" {
            continue;
        }
        for preview in [true, false] {
            if resolution == "final_4k" && preview {
                continue;
            }
            for (mode, cold, animated) in [
                ("context_cold_static", true, false),
                ("warm_static", false, false),
                (
                    if basic {
                        "warm_animated_30fps"
                    } else {
                        "warm_distinct_times_30fps"
                    },
                    false,
                    true,
                ),
            ] {
                if validation_only && (animated || !cold || !preview) {
                    continue;
                }
                let name = format!(
                    "{resolution}_{}_{}",
                    if preview { "preview" } else { "final" },
                    mode
                );
                handshake("phase_start", &name);
                let shared = if cold { None } else { Some(gpu()) };
                let mut samples = vec![];
                let mut compiler = vec![];
                let mut drawing = vec![];
                let mut waits = vec![];
                let mut counters = vec![];
                let mut failure = None;
                let mut input = json!({"project":project,"region":{"origin":[0,0],"extent":[extent["width"],extent["height"]],"pixels":pixels},"fonts":fonts.values().collect::<Vec<_>>()});
                input
                    .as_object_mut()
                    .unwrap()
                    .extend(target.as_object().unwrap().clone());
                let mut request: FrameRenderRequest =
                    serde_json::from_value(json!({"input":input,"time":{"num":"3","den":"2"}}))
                        .unwrap();
                let n: usize = 21;
                // Excluded warm-up uses identical production API and time.
                let mut measured_requests = vec![];
                for iteration in 0..=n {
                    if animated {
                        request.time = kronello_time::Time::new(
                            i64::try_from(iteration.saturating_sub(1)).unwrap(),
                            30,
                        )
                        .unwrap();
                    }
                    if iteration > 0 {
                        measured_requests.push(request.clone());
                    }
                    let total = Instant::now();
                    let temporary = if cold { Some(gpu()) } else { None };
                    let context = temporary.as_ref().or(shared.as_ref()).unwrap();
                    if adapter.is_none() {
                        adapter = Some(
                            json!({"name":context.adapter_info.name,"vendor":context.adapter_info.vendor,"device":context.adapter_info.device,"backend":format!("{:?}",context.adapter_info.backend),"driver":context.adapter_info.driver,"driver_info":context.adapter_info.driver_info}),
                        );
                    }
                    context.reset_allocation_peaks().unwrap();
                    let service = Service::with_backend(context);
                    if preview {
                        let start = Instant::now();
                        let (_, dag) = service.preview_dag(&request).unwrap();
                        let compile_ns = nanos(start);
                        let start = Instant::now();
                        let texture = match context.preview_texture(&dag) {
                            Ok(texture) => texture,
                            Err(kronello_render::RenderError::Backend { code, message })
                                if code == "UNSUPPORTED_FEATURE" =>
                            {
                                failure = Some(
                                    json!({"status":"unsupported","n":0,"error":{"code":code,"message":message},"dag_budget":native_budget(&dag)}),
                                );
                                break;
                            }
                            Err(error) => panic!("native preview failed: {error}"),
                        };
                        let draw_ns = nanos(start);
                        let start = Instant::now();
                        context.wait().unwrap();
                        let wait_ns = nanos(start);
                        std::hint::black_box(&texture);
                        if iteration > 0 {
                            compiler.push(compile_ns);
                            drawing.push(draw_ns);
                            waits.push(wait_ns);
                        }
                    } else {
                        let ResultData::Frame(frame) = service
                            .dispatch(Request::RenderFrame(request.clone()))
                            .unwrap()
                        else {
                            panic!()
                        };
                        let time = nanos(total);
                        let hash = frame_hash(&frame.linear, &frame.display);
                        if let Some(previous) = hashes.insert(
                            format!(
                                "{resolution}:{}",
                                serde_json::to_string(&request.time).unwrap()
                            ),
                            hash.clone(),
                        ) {
                            assert_eq!(previous, hash, "cold/warm exact linear/display pixels");
                        }
                        if iteration > 0 {
                            samples.push(time);
                            counters.push(json!({"transfer_stats":frame.metadata.transfer_stats,"resource_cache_stats":frame.metadata.resource_cache_stats,"time":request.time,"gpu_allocation_stats":context.allocation_stats(),"output_vec_capacity_bytes":(frame.linear.capacity()+frame.display.capacity())*std::mem::size_of::<[f32;4]>()}));
                        }
                        continue;
                    }
                    if iteration > 0 {
                        samples.push(nanos(total));
                        counters.push(json!({"transfer_stats":context.transfer_stats(),"resource_cache_stats":context.resource_cache_stats(),"time":request.time,"gpu_allocation_stats":context.allocation_stats()}));
                    }
                }
                handshake("phase_end", &name);
                if let Some(mut report) = failure {
                    report_case(&name, &mut report, &source_document_sha256, &target);
                    cases.insert(name, report);
                    continue;
                }
                // Equality oracles execute after timings with a fresh default context.
                let oracle = gpu();
                let service = Service::with_backend(&oracle);
                let oracle_requests: BTreeMap<_, _> = measured_requests
                    .iter()
                    .map(|request| (serde_json::to_string(&request.time).unwrap(), request))
                    .collect();
                for request in oracle_requests.values() {
                    oracle.clear_texture_cache().unwrap();
                    let ResultData::Frame(frame) = service
                        .dispatch(Request::RenderFrame((**request).clone()))
                        .unwrap()
                    else {
                        panic!()
                    };
                    let key = format!(
                        "{resolution}:{}",
                        serde_json::to_string(&request.time).unwrap()
                    );
                    let hash = frame_hash(&frame.linear, &frame.display);
                    if let Some(previous) = hashes.insert(key, hash.clone()) {
                        assert_eq!(previous, hash, "timed final matches fresh context oracle");
                    }
                    if preview {
                        let (_, dag) = service.preview_dag(request).unwrap();
                        let texture = oracle.preview_texture(&dag).unwrap();
                        let raw = oracle
                            .read_texture(&texture, 8, &mut Default::default())
                            .unwrap();
                        let pixels = kronello_gpu::decode_rgba16f(&raw).unwrap();
                        assert_eq!(
                            frame.metadata.working_space,
                            kronello_model::ColorSpace::LinearRec709
                        );
                        let [x, y] = dag.crop_origin();
                        let [width, height] = dag.region().pixels.map(|value| value as usize);
                        let stride = dag.execution_region().pixels[0] as usize;
                        for row in 0..height {
                            let native =
                                &pixels[(row + y) * stride + x..(row + y) * stride + x + width];
                            let expected = &frame.linear[row * width..(row + 1) * width];
                            if let Some((column, (left, right))) = native
                                .iter()
                                .zip(expected)
                                .enumerate()
                                .find(|(_, (left, right))| left != right)
                            {
                                panic!(
                                    "native preview mismatch at [{column},{row}]: {left:?} != {right:?}"
                                );
                            }
                        }
                    }
                }
                let mut report = summary(samples);
                if preview {
                    report["compile_layout_ns"] = summary(compiler);
                    report["gpu_submission_ns"] = summary(drawing);
                    report["explicit_completion_wait_operations_per_sample"] = json!(1);
                    report["completion_wait_ns"] = summary(waits);
                }
                report["counters_per_sample"] = json!(counters);
                report["status"] = json!("verified");
                let unique: BTreeSet<_> = oracle_requests
                    .values()
                    .map(|request| {
                        hashes[&format!(
                            "{resolution}:{}",
                            serde_json::to_string(&request.time).unwrap()
                        )]
                            .clone()
                    })
                    .collect();
                if animated && basic {
                    assert!(
                        unique.len() > 1,
                        "animated benchmark must change actual pixels"
                    );
                }
                report["unique_output_hash_count"] = json!(unique.len());
                report["actual_pixel_variation"] = json!(unique.len() > 1);
                report["motion_target_evidence"] = json!(animated && unique.len() > 1);
                report_case(&name, &mut report, &source_document_sha256, &target);
                cases.insert(name, report);
            }
        }
    }
    if !validation_only && !basic {
        let mut input = json!({"project":project,"region":{"origin":[0,0],"extent":[extent["width"],extent["height"]],"pixels":[3840,2160]},"fonts":fonts.values().collect::<Vec<_>>()});
        input
            .as_object_mut()
            .unwrap()
            .extend(target.as_object().unwrap().clone());
        let request: FrameRenderRequest =
            serde_json::from_value(json!({"input":input,"time":{"num":"3","den":"2"}})).unwrap();
        let context = gpu();
        let service = Service::with_backend(&context);
        let (_, dag) = service.preview_dag(&request).unwrap();
        let mut boundary = match context.preview_texture(&dag) {
            Ok(_) => json!({"status":"supported_untimed","dag_budget":native_budget(&dag)}),
            Err(kronello_render::RenderError::Backend { code, message })
                if code == "UNSUPPORTED_FEATURE" =>
            {
                json!({"status":"unsupported","n":0,"error":{"code":code,"message":message},"dag_budget":native_budget(&dag)})
            }
            Err(error) => panic!("native preview boundary failed: {error}"),
        };
        report_case(
            "lower_third_complex_4k_native_preview_boundary",
            &mut boundary,
            &source_document_sha256,
            &target,
        );
        cases.insert(
            "lower_third_complex_4k_native_preview_boundary".into(),
            boundary,
        );
    }
    println!(
        "{}",
        json!({"event":"report","schema_version":1,"build_profile":"release",
        "compiled_source_id":option_env!("KRONELLO_PERF_SOURCE_ID").unwrap_or("unrecorded"),"project":project,"source":source,"scene_kind":scene_kind,
        "source_document_sha256":source_document_sha256,
        "cases":cases,"adapter":adapter,"validation_only":validation_only,"linear_display_hashes":hashes,"preview_scope":"sharedService DAG, native GPU texture and explicit completion wait; excludes GUI presentation",
        "cold_scope":"new GPU context/pipelines/resource caches within timing; OS disk caches not purged",
        "warm_scope":"persistent GPU context; excluded first render; production Service recompiles/loads snapshot/fonts on every call",
        "animated_scope":"21 distinct times 0..20/30 seconds; basic requires actual pixel variation, unchanged complex fixture reports measured hash count and may be static",
        "cache_capacity_bytes":{"textures":64*1024*1024,"pool":64*1024*1024},
        "selected_target":target,"selected_design_extent":extent,
        "preview_equality":"untimed native readback, cropped by DAG halo origin, exact against same shared final LinearRec709 plane",
        "unobserved_stage_timings":["final compile/layout subdivision","native decode wait","atlas layout subdivision"],
        "memory_categories":{"decode":"no video in this reference","atlas":"see GPU allocation harness","accumulation":"single temporal sample, GPU allocation harness","encoder":"not used by still-frame final"}})
    );
    io::stdout().flush().unwrap();
    handshake("complete", "complete");
}
