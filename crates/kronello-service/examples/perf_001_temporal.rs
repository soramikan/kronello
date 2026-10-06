//! Release resource observation of shared whole-region temporal rendering.
use kronello_gpu::{GpuCacheConfig, GpuContext};
use kronello_render::RenderBackend;
use kronello_service::{
    BackendSelection, FrameRenderRequest, Request, Response, ResultData, Service,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
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
fn gpu() -> GpuContext {
    let context = GpuContext::new().unwrap();
    context.configure_cache(GpuCacheConfig::default()).unwrap();
    context
}
fn hash(pixels: &[[f32; 4]]) -> String {
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 65536];
    for chunk in pixels.chunks(4096) {
        for (index, pixel) in chunk.iter().enumerate() {
            for (component, value) in pixel.iter().enumerate() {
                let offset = index * 16 + component * 4;
                buffer[offset..offset + 4].copy_from_slice(&value.to_bits().to_le_bytes());
            }
        }
        hash.update(&buffer[..chunk.len() * 16]);
    }
    format!("{:x}", hash.finalize())
}
fn render(service: &Service<'_>, request: FrameRenderRequest) -> kronello_service::FrameResult {
    let ResultData::Frame(frame) = service.dispatch(Request::RenderFrame(request)).unwrap() else {
        panic!("expected shared frame result");
    };
    *frame
}
fn main() {
    if cfg!(debug_assertions) {
        panic!("use cargo build --release");
    }
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 3, "source.project.json output-directory");
    let source = PathBuf::from(&args[1]);
    let directory = PathBuf::from(&args[2]);
    std::fs::create_dir_all(&directory).unwrap();
    let project = directory.join("temporal.kronello");
    let document: Value = serde_json::from_slice(&std::fs::read(&source).unwrap()).unwrap();
    let response = Service::new(BackendSelection::CpuReference).execute_json(
        &json!({"operation":"project.create","project":project,"document":document}).to_string(),
    );
    assert!(matches!(response, Response::Success { .. }), "{response:?}");
    let settings = json!({"frame_rate":{"num":"24","den":"1"},"shutter_angle":{"num":"180","den":"1"},"shutter_phase":{"num":"-1","den":"4"},"samples":4,"cut_policy":"avoid_crossing"});
    let request: FrameRenderRequest = serde_json::from_value(json!({"input":{
        "project":project,"composition":document["compositions"][0]["id"],
        "region":{"origin":[0,0],"extent":[64,32],"pixels":[1920,1080]},
        "profile":{"working_space":"linear_rec709","flatten_tolerance_px":0.02,"temporal":settings}},
        "time":{"num":"1","den":"2"}})).unwrap();
    let tiles = kronello_render::frame_tiles(request.input.region);
    let maximum_backend_tile_pixels = tiles
        .iter()
        .map(|(_, region)| u64::from(region.pixels[0]) * u64::from(region.pixels[1]))
        .max()
        .unwrap();
    assert_eq!(tiles.len(), 12);
    let accumulation_pixels =
        u64::from(request.input.region.pixels[0]) * u64::from(request.input.region.pixels[1]);
    let context = gpu();
    let mut rejected_4k = request.clone();
    rejected_4k.input.region.pixels = [3840, 2160];
    let rejected_4k = Service::with_backend(&context)
        .dispatch(Request::RenderFrame(rejected_4k))
        .unwrap_err();
    assert_eq!(rejected_4k.code, "UNSUPPORTED_FEATURE");
    assert!(
        rejected_4k
            .message
            .contains("temporal accumulation budget exceeded")
    );
    context.reset_allocation_peaks().unwrap();
    handshake("phase_start", "temporal_1080_shared_four_samples");
    let start = Instant::now();
    let frame = render(&Service::with_backend(&context), request.clone());
    let duration_ns = u64::try_from(start.elapsed().as_nanos()).unwrap();
    let allocation = context.allocation_stats();
    let transfers = frame.metadata.transfer_stats;
    let cache = frame.metadata.resource_cache_stats;
    handshake("phase_end", "temporal_1080_shared_four_samples");
    let temporal = frame.metadata.temporal.as_ref().unwrap();
    assert_eq!(temporal.samples.len(), 4);
    let times: BTreeSet<_> = temporal.samples.iter().map(|s| s.time).collect();
    assert_eq!(times.len(), 4);
    let measured_linear_hash = hash(&frame.linear);
    let measured_display_hash = hash(&frame.display);
    let output_capacity_bytes =
        (frame.linear.capacity() + frame.display.capacity()) * std::mem::size_of::<[f32; 4]>();
    let fresh = gpu();
    let service = Service::with_backend(&fresh);
    let mut sums = vec![[0.0_f64; 4]; frame.linear.len()];
    let mut reference_hashes = BTreeSet::new();
    let mut instant = request.clone();
    instant.input.profile.temporal = None;
    for sample in &temporal.samples {
        let mut reference_request = instant.clone();
        reference_request.time = sample.time;
        fresh.clear_texture_cache().unwrap();
        let reference = render(&service, reference_request);
        reference_hashes.insert(hash(&reference.linear));
        let weight = sample.weight.numerator() as f64 / sample.weight.denominator() as f64;
        for (sum, pixel) in sums.iter_mut().zip(reference.linear) {
            for channel in 0..4 {
                sum[channel] += f64::from(pixel[channel]) * weight;
            }
        }
    }
    assert_eq!(
        reference_hashes.len(),
        4,
        "all shutter samples must change actual pixels"
    );
    let expected: Vec<_> = sums.into_iter().map(|p| p.map(|v| v as f32)).collect();
    assert_eq!(
        hash(&expected),
        measured_linear_hash,
        "strict all-pixel independent subtime mean"
    );
    let display = fresh
        .display_from_linear(&expected, request.input.profile.working_space)
        .unwrap();
    assert_eq!(
        hash(&display),
        measured_display_hash,
        "strict display conversion after accumulation"
    );
    let instantaneous = render(&service, instant);
    assert_ne!(
        hash(&instantaneous.linear),
        measured_linear_hash,
        "temporal result must differ from instantaneous frame"
    );
    fresh.clear_texture_cache().unwrap();
    let rerender = render(&service, request.clone());
    assert_eq!(
        hash(&rerender.linear),
        measured_linear_hash,
        "fresh shared temporal path"
    );
    assert_eq!(hash(&rerender.display), measured_display_hash);
    println!(
        "{}",
        json!({"event":"report","schema_version":1,"build_profile":"release",
        "compiled_source_id":option_env!("KRONELLO_PERF_SOURCE_ID").unwrap_or("unrecorded"),
        "source":source,"source_document_sha256":format!("{:x}",Sha256::digest(serde_json::to_vec(&document).unwrap())),
        "request":request,"n":1,"duration_ns":duration_ns,"timing_scope":"one resource coverage run, no percentile estimate; GPU context constructed before measured phase",
        "actual_temporal":temporal,"reference_distinct_linear_hashes":reference_hashes,"linear_sha256":measured_linear_hash,"display_sha256":measured_display_hash,
        "oracle":"untimed fresh-context shared instantaneous samples, independent f64 weighted mean, strict full-plane SHA256, post-average display conversion, fresh shared temporal re-render, differs from instant",
        "gpu_allocation_stats":allocation,"transfer_stats":transfers,"resource_cache_stats":cache,
        "cpu_accumulation_regions":1,"actual_sample_requests":temporal.samples.len(),"backend_tiles_per_sample":tiles.len(),"planned_backend_sample_tiles":tiles.len()*temporal.samples.len(),"backend_tile_edge_pixels":512,"maximum_backend_tile_pixels":maximum_backend_tile_pixels,
        "tile_plan":tiles,
        "cpu_accumulator_requested_payload_bytes":accumulation_pixels*std::mem::size_of::<[f64;4]>() as u64,
        "cpu_accumulator_scope":"32B/pixel Vec<f64x4> requested payload for full1920x1080 accumulator; source formula, not observed allocator/physical peak/capacity; sequential4samples reuse accumulator",
        "output_vec_capacity_bytes":output_capacity_bytes,"entry":"shared Service::render.frame; whole-region CPU accumulation, each instantaneous subframe uses512px GPU tiles",
        "unsupported_whole_4k":rejected_4k,"whole_4k_admission_estimate_bytes":3840_u64*2160*96,"whole_region_limit_bytes":512*1024*1024,
        "unknown_memory":["allocator overhead/capacity of internal accumulator","driver private memory","native codec pools (not used)"],
        "adapter":format!("{:?}",context.adapter_info)})
    );
    io::stdout().flush().unwrap();
    handshake("complete", "complete");
}
