//! Same-binary playback source sharing probe. Run each mode in its own process.
use kronello_media::{AudioSourceCacheLimits, MediaRuntime};
use kronello_model::*;
use kronello_service::*;
use kronello_time::{Time, TimeRange};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{io::Write, path::Path, time::Instant};

fn write_wave(path: &Path, seconds: u32, tone: u32) {
    let frames = seconds * 48000;
    let len = frames * 4;
    let mut file = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    file.write_all(b"RIFF").unwrap();
    file.write_all(&(36 + len).to_le_bytes()).unwrap();
    file.write_all(b"WAVEfmt ").unwrap();
    for bytes in [
        16u32.to_le_bytes().to_vec(),
        1u16.to_le_bytes().to_vec(),
        2u16.to_le_bytes().to_vec(),
        48000u32.to_le_bytes().to_vec(),
        192000u32.to_le_bytes().to_vec(),
        4u16.to_le_bytes().to_vec(),
        16u16.to_le_bytes().to_vec(),
    ] {
        file.write_all(&bytes).unwrap();
    }
    file.write_all(b"data").unwrap();
    file.write_all(&len.to_le_bytes()).unwrap();
    for i in 0..frames {
        let value = (0.5
            * (2.0 * std::f64::consts::PI * f64::from(tone) * f64::from(i) / 48000.0).sin()
            * 32767.0)
            .round() as i16;
        file.write_all(&value.to_le_bytes()).unwrap();
        file.write_all(&(-value).to_le_bytes()).unwrap();
    }
    file.flush().unwrap();
}
fn revision(response: Response) -> String {
    match response {
        Response::Success {
            result: ResultData::Project(info),
        } => info.revision,
        other => panic!("{other:?}"),
    }
}
fn summary(values: &[f64]) -> serde_json::Value {
    let mut warm = values[1..].to_vec();
    warm.sort_by(f64::total_cmp);
    json!({"cold_ms":values[0], "warm_p50_ms":warm[4], "warm_p95_ms":warm[7]})
}
fn main() {
    if cfg!(debug_assertions) {
        panic!("use --release");
    }
    let directory = std::path::PathBuf::from(std::env::args().nth(1).expect("output directory"));
    let mode = std::env::args().nth(2).expect("cached or uncached");
    assert!(matches!(mode.as_str(), "cached" | "uncached"));
    std::fs::create_dir_all(&directory).unwrap();
    let runtime = MediaRuntime::load().unwrap();
    let mut cases = vec![];
    for (count, seconds) in [(1, 30), (4, 15), (1, 300), (2, 300)] {
        let case_directory = directory.join(format!("{count}x{seconds}"));
        std::fs::create_dir(&case_directory).unwrap();
        let path = case_directory.join("audio.kronello");
        let mut document: Project =
            serde_json::from_str(include_str!("../../../examples/nle-001.project.json")).unwrap();
        let mut hashes = vec![];
        let DocumentObject::Known(sequence) = &mut document.sequences[0] else {
            panic!()
        };
        let sequence_id = sequence.id;
        sequence.tracks.truncate(1);
        sequence.tracks[0].kind = TrackKind::Audio;
        let original = sequence.tracks[0].clips[0].clone();
        sequence.tracks[0].clips.clear();
        for i in 0..count {
            let wave = case_directory.join(format!("{i}.wav"));
            write_wave(&wave, seconds, 440 + i * 37);
            let hash = kronello_media::content_hash(&wave).unwrap();
            hashes.push(hash.clone());
            let asset = Asset {
                id: AssetId::new(),
                kind: AssetKind::Audio,
                content_hash: hash,
                streams: runtime
                    .probe(&wave)
                    .unwrap()
                    .streams
                    .into_iter()
                    .map(|s| StreamMetadata {
                        index: s.index,
                        codec: s.codec,
                        time_base: s.time_base,
                        duration: s.duration,
                        start_time: s.start,
                        width: s.width,
                        height: s.height,
                        pixel_format: s.pixel_format,
                        color_primaries: s.color_primaries,
                        color_transfer: s.color_transfer,
                        color_matrix: s.color_matrix,
                        color_range: s.color_range,
                    })
                    .collect(),
                locator: AssetLocator {
                    relative: Some(format!("{i}.wav")),
                    absolute: None,
                },
            };
            let mut clip = original.clone();
            clip.id = ClipId::new();
            clip.source_ref = SourceRef::Asset {
                asset: asset.id,
                stream_index: 0,
            };
            clip.timeline_range = TimeRange::new(
                Time::from_integer(i64::from(i * seconds)),
                Time::from_integer(i64::from((i + 1) * seconds)),
            )
            .unwrap();
            clip.source_in = Time::ZERO;
            clip.time_map = kronello_time::TimeMap::linear(Time::ZERO, Time::ONE).unwrap();
            clip.audio_retime = AudioRetimePolicy::ResampleV1;
            sequence.tracks[0].clips.push(clip);
            document.assets.push(DocumentObject::Known(asset));
        }
        let service = Service::new(BackendSelection::CpuReference).with_audio_source_cache_limits(
            if mode == "cached" {
                AudioSourceCacheLimits::default()
            } else {
                AudioSourceCacheLimits {
                    entries: 0,
                    pcm_bytes: 0,
                }
            },
        );
        let mut rev = revision(service.execute(Request::ProjectCreate(CreateRequest {
            project: path.clone(),
            document: document.clone(),
            plan_hash: None,
            idempotency_key: None,
        })));
        let mut trials = vec![];
        let mut starts = vec![];
        let mut captures = vec![];
        let mut preparations = vec![];
        let mut blocks = vec![];
        for iteration in 0..9 {
            if iteration == 4 {
                let DocumentObject::Known(composition) = &mut document.compositions[0] else {
                    panic!()
                };
                composition.design_extent = DesignExtent::new(128.0, 64.0).unwrap();
                rev = revision(service.execute(Request::ProjectImport(ImportRequest {
                    project: path.clone(),
                    base_revision: rev,
                    document: document.clone(),
                    plan_hash: None,
                    idempotency_key: None,
                })));
            }
            let begin = Instant::now();
            let input = service
                .capture_audio_input(AudioPrepareRequest {
                    project: path.clone(),
                    target: RenderTarget::Sequence {
                        sequence: sequence_id,
                    },
                    expected_revision: rev.clone(),
                    resume_sample: Some(i64::from(iteration % 3) * 48000),
                })
                .unwrap();
            let captured = begin.elapsed().as_secs_f64() * 1000.0;
            let prepared = input.prepare().unwrap();
            let ready = begin.elapsed().as_secs_f64() * 1000.0;
            let mut buffer = [0.0f32; 8192];
            let block_begin = Instant::now();
            prepared
                .render_block(i64::from(iteration % 3) * 48000, &mut buffer)
                .unwrap();
            let block = block_begin.elapsed().as_secs_f64() * 1000.0;
            let first_audio = begin.elapsed().as_secs_f64() * 1000.0;
            let mut digest = Sha256::new();
            for start in [0, 4096, 48000, i64::from(count * seconds - 1) * 48000] {
                prepared.render_block(start, &mut buffer).unwrap();
                for value in buffer {
                    digest.update(value.to_bits().to_le_bytes());
                }
            }
            let output_hash = format!("{:x}", digest.finalize());
            captures.push(captured);
            preparations.push(ready - captured);
            starts.push(first_audio);
            blocks.push(block);
            trials.push(json!({"iteration":iteration,"revision":rev,"capture_ms":captured,"prepare_ms":ready-captured,"first_block_ms":block,"start_to_first_block_ms":first_audio,"rendered_block_bits_sha256":output_hash,"cache":service.audio_source_cache_stats()}));
        }
        cases.push(json!({"sources":count,"source_seconds_each":seconds,"decoded_pcm_bytes":u64::from(count)*u64::from(seconds)*48000*2*4,"source_sha256":hashes,"capture":summary(&captures),"prepare":summary(&preparations),"first_block":summary(&blocks),"start_to_first_block":summary(&starts),"trials":trials}));
    }
    println!(
        "{}",
        json!({"version":1,"mode":mode,"runtime":runtime.capabilities(),"quiet_host":false,"scope":"same binary and capture/prepare path; create/source generation/hash/import at iteration4 outside timing; first block included; device callback/presentation and DSP checkpoint optimization excluded","order":"all uncached cases then all cached cases in separate processes; cold1 warm8","cases":cases})
    );
}
