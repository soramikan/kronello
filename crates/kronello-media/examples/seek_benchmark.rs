//! Build before changing the seek algorithm, archive that binary, then compare
//! it with the optimized release binary in the same quiet host window.
use kronello_media::*;
use kronello_time::Rational;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{path::Path, process::Command, time::Instant};
fn t(n: i64, d: i64) -> Rational {
    Rational::new(n, d).unwrap()
}
fn command(program: &str, args: &[&str]) -> String {
    String::from_utf8_lossy(&Command::new(program).args(args).output().unwrap().stdout)
        .trim()
        .into()
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn percentile(samples: &[u64], percent: usize) -> u64 {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    sorted[(sorted.len() * percent).div_ceil(100).saturating_sub(1)]
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if cfg!(debug_assertions) {
        return Err("release build required".into());
    }
    let phase = std::env::args().nth(1).unwrap_or_else(|| "after".into());
    if !matches!(phase.as_str(), "before" | "after") {
        return Err("phase must be before or after".into());
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixtures = root.join("target/fixtures/generated/media");
    let runtime = MediaRuntime::load()?;
    let mut cases = Vec::<Value>::new();
    for (kind, name, times) in [
        (
            "CFR",
            "cfr-30000-1001.nut",
            (0..6).map(|i| t(i * 1001, 30000)).collect::<Vec<_>>(),
        ),
        (
            "VFR",
            "vfr.nut",
            [0, 1, 3, 6, 10, 15].into_iter().map(|i| t(i, 30)).collect(),
        ),
        ("Bframe", "bframes.nut", (1..7).map(|i| t(i, 24)).collect()),
    ] {
        let path = fixtures.join(name);
        let identity = content_hash(&path)?;
        for (direction, queries) in [
            ("forward", times.clone()),
            ("backward", times.iter().copied().rev().collect()),
            ("repeated", vec![times[4]; 20]),
        ] {
            let mut samples = Vec::new();
            let mut counters = VideoDecodeStats::default();
            let mut copied = 0_u64;
            // Fresh independent decoder is the history-free exact oracle.
            let expected: Vec<_> = queries
                .iter()
                .map(|q| runtime.open_video(&path).unwrap().decode_at(*q).unwrap())
                .collect();
            let mut decoder_name = None;
            for iteration in 0..65 {
                let mut decoder = runtime.open_video(&path)?;
                decoder_name.clone_from(&decoder.path_report().decoder);
                for (query, expected) in queries.iter().zip(&expected) {
                    let start = Instant::now();
                    let actual = decoder.decode_at(*query)?;
                    let elapsed = start.elapsed().as_nanos() as u64;
                    if iteration >= 5 {
                        samples.push(elapsed);
                    }
                    assert_eq!(
                        actual, *expected,
                        "exact planes/PTS/end/color mismatch {kind} {direction}"
                    );
                }
                if iteration >= 5 {
                    let stats = decoder.decode_stats();
                    counters.seeks += stats.seeks;
                    counters.decoded_frames += stats.decoded_frames;
                    counters.interval_hits += stats.interval_hits;
                    counters.cache_clone_bytes += stats.cache_clone_bytes;
                    counters.returned_clone_bytes += stats.returned_clone_bytes;
                    counters.peak_cached_frame_bytes = counters
                        .peak_cached_frame_bytes
                        .max(stats.peak_cached_frame_bytes);
                    copied += decoder.path_report().transfers.cpu_copy_bytes;
                }
            }
            cases.push(json!({"key":format!("{kind}/{direction}"),"fixture":name,"fixture_sha256":identity,"codec":decoder_name,"repetitions":60,"samples_ns":samples,"p50_ns":percentile(&samples,50),"p95_ns":percentile(&samples,95),"exact_pixel_pts_end_color":"pass","counters":counters,"cpu_copy_bytes":copied}));
        }
    }
    let report = json!({"schema_version":1,"phase":phase,"revision":command("git",&["rev-parse","HEAD"]),"dirty_files":command("git",&["diff","--name-only"]),"source_identities":{"video.rs":hash(include_bytes!("../src/video.rs")),"ffi.rs":hash(include_bytes!("../src/ffi.rs")),"media.c":hash(include_bytes!("../native/media.c"))},"host":command("uname",&["-n"]),"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"os_release":command("uname",&["-r"]),"build_profile":"release","runtime":{"version":runtime.capabilities().ffmpeg_version,"directory":runtime.capabilities().library_directory,"substituted":runtime.capabilities().substituted},"fixture_generation":"python3 scripts/fixtures.py generate; existing locked generated fixtures","warmup_repetitions":5,"timing_scope":"decode_at only; native runtime warm; fresh decoder per repetition; independent exact oracle excluded from timing; record only in quiet host window","cases":cases});
    let directory = root.join("target/perf-001-evidence");
    std::fs::create_dir_all(&directory)?;
    let output = directory.join(format!("media-seek-{phase}.json"));
    std::fs::write(&output, serde_json::to_vec_pretty(&report)?)?;
    println!("{}", output.display());
    Ok(())
}
