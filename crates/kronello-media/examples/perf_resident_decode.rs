//! Strict native decode: fresh sessions versus a bounded forward cursor.
#[cfg(target_os = "macos")]
fn main() {
    use kronello_framebridge::resident::{
        ResidentFileDecoder, ResidentFormat, VideoTransfer, decode_file_with_interval,
    };
    use kronello_gpu::WorkingSpace;
    use kronello_media::{MediaRuntime, content_hash};
    use kronello_time::Time;
    use serde_json::json;
    use sha2::{Digest, Sha256};
    use std::{path::Path, time::Instant};
    if cfg!(debug_assertions) {
        panic!("use --release");
    }
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(
        args.len(),
        5,
        "OUTPUT_JSON SEEK_FIXTURES INPUT_4K before|legacy|streaming"
    );
    let mode = &args[4];
    assert!(matches!(mode.as_str(), "before" | "legacy" | "streaming"));
    let runtime = MediaRuntime::load().unwrap();
    let paths = [
        Path::new(&args[2]).join("cfr.mov"),
        Path::new(&args[2]).join("vfr.mov"),
        Path::new(&args[2]).join("bframes.mov"),
        args[3].clone().into(),
    ];
    let mut cases = Vec::new();
    for (case_index, path) in paths.iter().enumerate() {
        let mut video = runtime.open_video(path).unwrap();
        let metadata = video.stream_metadata().unwrap();
        let start = metadata.start_time.unwrap_or(Time::ZERO);
        let end = start.checked_add(metadata.duration.unwrap()).unwrap();
        let interval = [
            (start.numerator(), start.denominator()),
            (end.numerator(), end.denominator()),
        ];
        let times: Vec<_> = [0, 1, 2, 3, 4, 5, 6, 7, 2, 3, 8, 9]
            .into_iter()
            .map(|i| {
                start
                    .checked_add(
                        Time::new(
                            if case_index == 3 { i } else { 720 + i },
                            if case_index == 3 { 30 } else { 24 },
                        )
                        .unwrap(),
                    )
                    .unwrap()
            })
            .collect();
        for format in [ResidentFormat::Bgra8, ResidentFormat::Nv12VideoRange] {
            let mut oracles = Vec::new();
            let mut trials = Vec::new();
            for iteration in 0..5 {
                let mut requests = Vec::new();
                let mut cursor: Option<ResidentFileDecoder> = None;
                for (i, time) in times.iter().enumerate() {
                    let begin = Instant::now();
                    let frame = if mode == "streaming" {
                        if cursor.is_none() {
                            cursor =
                                Some(ResidentFileDecoder::open(path, 0, format, interval).unwrap());
                        }
                        cursor
                            .as_mut()
                            .unwrap()
                            .decode_at((time.numerator(), time.denominator()))
                            .unwrap()
                    } else {
                        decode_file_with_interval(
                            path,
                            0,
                            (time.numerator(), time.denominator()),
                            format,
                            interval,
                        )
                        .unwrap()
                    };
                    let elapsed_ms = begin.elapsed().as_secs_f64() * 1000.0;
                    let validation = frame
                        .read_for_validation(WorkingSpace::LinearRec709, VideoTransfer::Bt709)
                        .unwrap();
                    let mut digest = Sha256::new();
                    for plane in &validation.planes {
                        digest.update((plane.len() as u64).to_le_bytes());
                        digest.update(plane);
                    }
                    let pts = frame.presentation_time;
                    let exact_pts = Time::new(pts.value, i64::from(pts.timescale)).unwrap();
                    digest.update(serde_json::to_vec(&json!({"pts":exact_pts,"size":frame.size(),"format":format!("{format:?}")})).unwrap());
                    let oracle = format!("{:x}", digest.finalize());
                    if iteration == 0 {
                        oracles.push(oracle.clone());
                    } else {
                        assert_eq!(oracle, oracles[i]);
                    }
                    requests.push(json!({"query":i,"time":time,"pts":exact_pts,"size":frame.size(),"elapsed_ms":elapsed_ms,"native_sha256":oracle}));
                }
                let stats=cursor.as_ref().map(|cursor| {
                    let stats=cursor.stats(); json!({"metadata_samples":stats.metadata_samples,"compressed_samples":stats.compressed_samples,"compressed_sample_peak_bytes":stats.compressed_sample_peak_bytes,"sessions_created":stats.sessions_created,"origin_restarts":stats.origin_restarts,"retained_outputs":stats.retained_outputs,"retained_output_bytes":stats.retained_output_bytes,"peak_output_bytes":stats.peak_output_bytes,"peak_outputs":stats.peak_outputs})
                });
                trials
                    .push(json!({"iteration":iteration,"requests":requests,"cursor_stats":stats}));
            }
            let mut warm: Vec<_> = trials[1..]
                .iter()
                .flat_map(|t| t["requests"].as_array().unwrap())
                .map(|r| r["elapsed_ms"].as_f64().unwrap())
                .collect();
            warm.sort_by(f64::total_cmp);
            cases.push(json!({"input":path,"input_sha256":content_hash(path).unwrap(),"metadata":metadata,"format":format!("{format:?}"),"warm_p50_ms":warm[warm.len()/2],"warm_p95_ms":warm[(warm.len()*95).div_ceil(100)-1],"trials":trials}));
        }
    }
    let report = json!({"mode":mode,"quiet_host":false,"scope":"includes streaming open/preflight in first query of each batch; strict hardware decoder and demux request; native full planes and PTS equality outside timing; 1 cold/4 warm batches; 12 forward/backward queries per case; runtime/metadata/source hash excluded", "source_sha256":{"resident.rs":format!("{:x}",Sha256::digest(include_bytes!("../../kronello-framebridge/src/resident.rs"))),"videotoolbox.rs":format!("{:x}",Sha256::digest(include_bytes!("../../kronello-framebridge/src/videotoolbox.rs"))),"resident_cursor.rs":format!("{:x}",Sha256::digest(include_bytes!("../../kronello-framebridge/src/resident_cursor.rs"))),"demux.m":format!("{:x}",Sha256::digest(include_bytes!("../../kronello-framebridge/native/demux.m")))},"cases":cases});
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
#[cfg(not(target_os = "macos"))]
fn main() {
    panic!("strict VideoToolbox/Metal requires macOS");
}
