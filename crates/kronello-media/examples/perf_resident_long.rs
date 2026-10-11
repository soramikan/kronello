//! Native cursor on a repeated 36-minute clip exceeding the old demux budget.
#[cfg(target_os = "macos")]
fn main() {
    use kronello_framebridge::resident::{
        ResidentFileDecoder, ResidentFormat, VideoTransfer, decode_file_with_interval,
    };
    use kronello_gpu::WorkingSpace;
    use kronello_media::content_hash;
    use serde_json::json;
    use sha2::{Digest, Sha256};
    use std::{path::Path, time::Instant};
    if cfg!(debug_assertions) {
        panic!("use --release");
    }
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 4, "OUTPUT_JSON LONG_MOV SOURCE_60S_MOV");
    let long = Path::new(&args[2]);
    let short = Path::new(&args[3]);
    let format = ResidentFormat::Nv12VideoRange;
    let begin = Instant::now();
    let legacy = decode_file_with_interval(long, 0, (0, 1), format, [(0, 1), (2160, 1)])
        .err()
        .expect("fixture must exceed old whole-clip compressed sample budget");
    let legacy_ms = begin.elapsed().as_secs_f64() * 1000.0;
    assert!(legacy.detail.contains("compressed sample budget exceeded"));
    let mut queries = Vec::new();
    let mut cursor = None;
    for (numerator, denominator) in [
        (0, 1),
        (2100, 1),
        (50401, 24),
        (50402, 24),
        (50403, 24),
        (30, 1),
        (721, 24),
    ] {
        let begin = Instant::now();
        if cursor.is_none() {
            cursor = Some(ResidentFileDecoder::open(long, 0, format, [(0, 1), (2160, 1)]).unwrap());
        }
        let frame = cursor
            .as_mut()
            .unwrap()
            .decode_at((numerator, denominator))
            .unwrap();
        let elapsed_ms = begin.elapsed().as_secs_f64() * 1000.0;
        // Packet-copy concat preserves the repeated source's exact native planes.
        // The short origin oracle remains under the old whole-clip budget.
        let relative = (numerator % (60 * denominator), denominator);
        let expected =
            decode_file_with_interval(short, 0, relative, format, [(0, 1), (60, 1)]).unwrap();
        let actual = frame
            .read_for_validation(WorkingSpace::LinearRec709, VideoTransfer::Bt709)
            .unwrap();
        let reference = expected
            .read_for_validation(WorkingSpace::LinearRec709, VideoTransfer::Bt709)
            .unwrap();
        assert_eq!(actual.planes, reference.planes);
        let pts = frame.presentation_time;
        let oracle_pts = expected.presentation_time;
        assert_eq!(
            i128::from(pts.value) * i128::from(denominator),
            i128::from(numerator) * i128::from(pts.timescale)
        );
        assert_eq!(
            i128::from(oracle_pts.value) * i128::from(relative.1),
            i128::from(relative.0) * i128::from(oracle_pts.timescale)
        );
        let mut digest = Sha256::new();
        for plane in actual.planes {
            digest.update((plane.len() as u64).to_le_bytes());
            digest.update(plane);
        }
        let presentation = [pts.value, i64::from(pts.timescale)];
        queries.push(json!({"time":[numerator,denominator],"oracle_time":relative,"elapsed_ms":elapsed_ms,"native_sha256":format!("{:x}",digest.finalize()),"size":frame.size(),"pts":presentation}));
    }
    let stats = cursor.unwrap().stats();
    let report = json!({"quiet_host":false,"scope":"one diagnostic batch, no percentile/causal memory claim; 36 packet-copy concatenations of 60s CFR; legacy returns its explicit compressed budget error; streaming checks all native planes against the repeated short source; source hashing and validation outside request timing",
        "long_input":long,"long_sha256":content_hash(long).unwrap(),"long_file_bytes":std::fs::metadata(long).unwrap().len(),"short_input":short,"short_sha256":content_hash(short).unwrap(),"legacy_error":legacy.to_string(),"legacy_ms":legacy_ms,"queries":queries,
        "cursor_stats":{"metadata_samples":stats.metadata_samples,"compressed_samples":stats.compressed_samples,"compressed_sample_peak_bytes":stats.compressed_sample_peak_bytes,"sessions_created":stats.sessions_created,"origin_restarts":stats.origin_restarts,"retained_outputs":stats.retained_outputs,"retained_output_bytes":stats.retained_output_bytes,"peak_output_bytes":stats.peak_output_bytes,"peak_outputs":stats.peak_outputs},
        "source_sha256":{"resident.rs":format!("{:x}",Sha256::digest(include_bytes!("../../kronello-framebridge/src/resident.rs"))),"resident_cursor.rs":format!("{:x}",Sha256::digest(include_bytes!("../../kronello-framebridge/src/resident_cursor.rs"))),"videotoolbox.rs":format!("{:x}",Sha256::digest(include_bytes!("../../kronello-framebridge/src/videotoolbox.rs"))),"demux.m":format!("{:x}",Sha256::digest(include_bytes!("../../kronello-framebridge/native/demux.m")))} });
    use std::io::Write;
    let mut out = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[1])
        .unwrap();
    out.write_all(&serde_json::to_vec_pretty(&report).unwrap())
        .unwrap();
}
#[cfg(not(target_os = "macos"))]
fn main() {
    panic!("strict VideoToolbox requires macOS");
}
