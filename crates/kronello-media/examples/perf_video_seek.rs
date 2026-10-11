//! Exact native-frame long scrub probe; archive the baseline before optimizing.
use kronello_media::{MediaRuntime, VideoDecodeStats, VideoSeekIndexLimits, content_hash};
use kronello_time::Time;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{path::Path, process::Command, time::Instant};

fn generate(directory: &Path) {
    std::fs::create_dir(directory).unwrap();
    for (name, bframes, vfr) in [
        ("cfr.mov", "0", false),
        ("vfr.mov", "0", true),
        ("bframes.mov", "2", false),
        ("negative.ts", "2", false),
    ] {
        let mut command = Command::new("ffmpeg");
        command.args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x180:rate=24",
            "-t",
            "60",
            "-threads",
            "2",
            "-pix_fmt",
            "yuv420p",
            "-color_primaries",
            "bt709",
            "-color_trc",
            "bt709",
            "-colorspace",
            "bt709",
            "-color_range",
            "tv",
        ]);
        if name == "negative.ts" {
            command.args([
                "-c:v",
                "mpeg2video",
                "-bf",
                bframes,
                "-g",
                "12",
                "-muxdelay",
                "0",
                "-output_ts_offset",
                "-0.125",
                "-avoid_negative_ts",
                "disabled",
            ]);
        } else {
            command.args([
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-g",
                "48",
                "-bf",
                bframes,
                "-sc_threshold",
                "0",
            ]);
            if vfr {
                command.args([
                    "-vf",
                    "select='not(eq(mod(n,6),2)+eq(mod(n,6),4)+eq(mod(n,6),5))'",
                    "-fps_mode",
                    "vfr",
                ]);
            }
        }
        command.arg(directory.join(name));
        assert!(command.status().unwrap().success(), "fixture {name}");
    }
}

fn main() {
    if cfg!(debug_assertions) {
        panic!("use --release");
    }
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(
        args.len(),
        4,
        "OUTPUT_DIRECTORY FIXTURE_DIRECTORY before|after|origin"
    );
    let output = Path::new(&args[1]);
    let fixtures = Path::new(&args[2]);
    let phase = &args[3];
    assert!(matches!(phase.as_str(), "before" | "after" | "origin"));
    std::fs::create_dir(output).unwrap();
    if phase == "before" {
        generate(fixtures);
    }
    let runtime = MediaRuntime::load().unwrap();
    let mut cases = Vec::new();
    let mut queries: Vec<_> = [54, 43, 55, 42, 57, 54, 44, 58, 45, 57, 53, 48, 54]
        .into_iter()
        .map(Time::from_integer)
        .collect();
    queries.push(Time::new(2593, 48).unwrap());
    for name in ["cfr.mov", "vfr.mov", "bframes.mov", "negative.ts"] {
        let path = fixtures.join(name);
        // A fresh empty decoder has no history-derived seek references.
        let expected: Vec<_> = queries
            .iter()
            .map(|time| runtime.open_video(&path).unwrap().decode_at(*time).unwrap())
            .collect();
        let oracle: Vec<_> = expected
            .iter()
            .map(|frame| {
                json!({"pts":frame.pts,"end":frame.end,"width":frame.width,
                    "height":frame.height,"pixel_format":frame.pixel_format,
                    "primaries":frame.color_primaries,"transfer":frame.color_transfer,
                    "matrix":frame.color_matrix,"range":frame.color_range,
                    "native_pixels_sha256":format!("{:x}",Sha256::digest(&frame.pixels))})
            })
            .collect();
        let mut decoder = runtime.open_video(&path).unwrap();
        if phase == "origin" {
            decoder = decoder
                .with_seek_index_limits(VideoSeekIndexLimits {
                    max_entries: 0,
                    ..Default::default()
                })
                .unwrap();
        }
        let metadata = decoder.stream_metadata().unwrap();
        let mut trials = Vec::new();
        let initial_counters: VideoDecodeStats = decoder.decode_stats();
        let mut previous = initial_counters;
        for iteration in 0..9 {
            let begin = Instant::now();
            let mut requests = Vec::new();
            for (time, expected) in queries.iter().zip(&expected) {
                let start = Instant::now();
                let actual = decoder.decode_at(*time).unwrap();
                let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                // Comparison is outside each request's measured decode interval.
                assert_eq!(
                    actual, *expected,
                    "native bits/interval/color {name} {time:?}"
                );
                requests.push(json!({"time":time,"decode_ms":elapsed_ms}));
            }
            let batch_wall_ms = begin.elapsed().as_secs_f64() * 1000.0;
            let counters = decoder.decode_stats();
            trials.push(json!({"iteration":iteration,"batch_wall_ms":batch_wall_ms,
                "requests":requests,"counters":counters,
                "decoded_frames_delta":counters.decoded_frames-previous.decoded_frames,
                "seeks_delta":counters.seeks-previous.seeks,"seek_index":decoder.seek_index_stats()}));
            previous = counters;
        }
        let mut warm: Vec<_> = trials[1..]
            .iter()
            .flat_map(|trial| trial["requests"].as_array().unwrap())
            .map(|request| request["decode_ms"].as_f64().unwrap())
            .collect();
        warm.sort_by(f64::total_cmp);
        cases.push(
            json!({"case":name,"path":path,"fixture_sha256":content_hash(&path).unwrap(),"initial_counters":initial_counters,
            "stream":metadata,"decoder":decoder.path_report().decoder,"oracle":oracle,
            "warm_request_p50_ms":warm[warm.len()/2],
            "warm_request_p95_ms":warm[(warm.len()*95).div_ceil(100)-1],"trials":trials}),
        );
    }
    let report = json!({"phase":phase,"quiet_host":false,"build":"release",
        "cold_batches":1,"warm_batches":8,"requests_per_batch":queries.len(),
        "scope":"decode_at only per request; runtime/open/fixture generation/oracle/hashes excluded; batch wall includes native frame equality and per-request recording; one retained decoder per case",
        "canonical_reference":"fresh decoder at every query; full native planes and exact PTS/end/dimensions/color compared outside decode timing",
        "source_sha256":{"video.rs":format!("{:x}",Sha256::digest(include_bytes!("../src/video.rs"))),
            "video_seek.rs":format!("{:x}",Sha256::digest(include_bytes!("../src/video_seek.rs"))),
            "ffi.rs":format!("{:x}",Sha256::digest(include_bytes!("../src/ffi.rs"))),
            "media.c":format!("{:x}",Sha256::digest(include_bytes!("../native/media.c")))},
        "capabilities":runtime.capabilities(),"cases":cases});
    std::fs::write(
        output.join("measurements.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("{}", output.join("measurements.json").display());
}
