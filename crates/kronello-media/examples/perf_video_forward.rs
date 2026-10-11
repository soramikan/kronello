//! Same-binary native forward-decode cost of presentation certification.
use kronello_media::{MediaRuntime, VideoSeekIndexLimits, content_hash};
use kronello_time::Time;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{path::Path, time::Instant};

fn main() {
    if cfg!(debug_assertions) {
        panic!("use --release");
    }
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 4, "OUTPUT_JSON INPUT_MOV origin|indexed");
    let path = Path::new(&args[2]);
    let mode = &args[3];
    assert!(matches!(mode.as_str(), "origin" | "indexed"));
    let runtime = MediaRuntime::load().unwrap();
    let mut oracle = None;
    let mut intervals = Vec::new();
    let mut trials = Vec::new();
    for iteration in 0..5 {
        let mut decoder = runtime.open_video(path).unwrap();
        if mode == "origin" {
            decoder = decoder
                .with_seek_index_limits(VideoSeekIndexLimits {
                    max_entries: 0,
                    ..Default::default()
                })
                .unwrap();
        }
        let mut digest = Sha256::new();
        let mut requests = Vec::new();
        for index in 0..60 {
            let time = Time::new(index, 30).unwrap();
            let begin = Instant::now();
            let frame = decoder.decode_at(time).unwrap();
            let decode_ms = begin.elapsed().as_secs_f64() * 1000.0;
            let descriptor = json!({"pts":frame.pts,"end":frame.end,
                "width":frame.width,"height":frame.height,"format":frame.pixel_format,
                "primaries":frame.color_primaries,"transfer":frame.color_transfer,
                "matrix":frame.color_matrix,"range":frame.color_range});
            digest.update(serde_json::to_vec(&descriptor).unwrap());
            digest.update((frame.pixels.len() as u64).to_le_bytes());
            digest.update(&frame.pixels);
            if iteration == 0 {
                intervals.push(descriptor);
            }
            requests.push(json!({"frame":index,"decode_ms":decode_ms}));
        }
        let bits = format!("{:x}", digest.finalize());
        if let Some(expected) = &oracle {
            assert_eq!(&bits, expected);
        } else {
            oracle = Some(bits.clone());
        }
        trials.push(json!({"iteration":iteration,"requests":requests,
            "native_frame_stream_sha256":bits,"decode_stats":decoder.decode_stats(),
            "seek_index":decoder.seek_index_stats()}));
    }
    let mut warm: Vec<_> = trials[1..]
        .iter()
        .flat_map(|trial| trial["requests"].as_array().unwrap())
        .map(|request| request["decode_ms"].as_f64().unwrap())
        .collect();
    warm.sort_by(f64::total_cmp);
    let report = json!({"mode":mode,"input":path,"input_sha256":content_hash(path).unwrap(),
        "quiet_host":false,"scope":"60 sequential native presentations at 30fps; fresh decoder per batch; open/runtime/hash/equality excluded from request timings; cold 1/warm 4",
        "source_sha256":{"video.rs":format!("{:x}",Sha256::digest(include_bytes!("../src/video.rs"))),
            "video_seek.rs":format!("{:x}",Sha256::digest(include_bytes!("../src/video_seek.rs"))),
            "ffi.rs":format!("{:x}",Sha256::digest(include_bytes!("../src/ffi.rs")))},
        "warm_request_p50_ms":warm[warm.len()/2],
        "warm_request_p95_ms":warm[(warm.len()*95).div_ceil(100)-1],
        "oracle":oracle,"intervals":intervals,"trials":trials});
    use std::io::Write;
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[1])
        .unwrap();
    output
        .write_all(&serde_json::to_vec_pretty(&report).unwrap())
        .unwrap();
}
