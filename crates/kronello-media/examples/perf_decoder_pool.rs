//! Release software media-resolution probe; DAG preparation is outside timing.
use kronello_media::{MediaRuntime, MediaSession, content_hash, resolve_dag_media_in};
use kronello_model::*;
use kronello_render::*;
use kronello_time::{FrameRate, Rational, SampleRate, Time, TimeMap, TimeRange};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{path::PathBuf, time::Instant};

fn main() {
    if cfg!(debug_assertions) {
        panic!("use --release");
    }
    let movie = PathBuf::from(std::env::args().nth(1).expect("local movie"))
        .canonicalize()
        .unwrap();
    let directory = PathBuf::from(std::env::args().nth(2).expect("output directory"));
    std::fs::create_dir_all(&directory).unwrap();
    let runtime = MediaRuntime::load().unwrap();
    let stream = runtime
        .open_video(&movie)
        .unwrap()
        .stream_metadata()
        .unwrap();
    let origin = stream.start_time.unwrap_or(Time::ZERO);
    let source_hash = content_hash(&movie).unwrap();
    let mut cases = Vec::new();
    for shared in [false, true] {
        for count in [1, 2, 3, 4, 8] {
            let mut assets = Vec::new();
            for index in 0..if shared { 1 } else { count } {
                let path = directory.join(format!("source-{index}.mov"));
                std::fs::copy(&movie, &path).unwrap();
                assets.push(Asset {
                    id: AssetId::new(),
                    kind: AssetKind::Video,
                    content_hash: source_hash.clone(),
                    locator: AssetLocator {
                        relative: None,
                        absolute: Some(path.canonicalize().unwrap().to_str().unwrap().into()),
                    },
                    streams: vec![stream.clone()],
                });
            }
            let sequence = Sequence {
                id: SequenceId::new(),
                extent: DesignExtent::new(64.0, 36.0).unwrap(),
                frame_rate: FrameRate::new(30, 1).unwrap(),
                audio_rate: SampleRate::HZ_48000,
                working_space: ColorSpace::LinearRec709,
                tracks: (0..count)
                    .map(|index| Track {
                        id: TrackId::new(),
                        state: None,
                        kind: TrackKind::Video,
                        clips: vec![Clip {
                            id: ClipId::new(),
                            source_ref: SourceRef::Asset {
                                asset: assets[if shared { 0 } else { index }].id,
                                stream_index: stream.index,
                            },
                            timeline_range: TimeRange::new(Time::ZERO, Time::ONE).unwrap(),
                            source_in: origin
                                .checked_add(Time::new(index as i64, 4).unwrap())
                                .unwrap(),
                            time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
                            enabled: true,
                            audio_retime: AudioRetimePolicy::Reject,
                            reverse_sampling: None,
                            volume: None,
                            links: vec![],
                            properties: vec![],
                            effects: vec![],
                            masks: vec![],
                            markers: vec![],
                            pan: None,
                        }],
                    })
                    .collect(),
                transitions: vec![],
                markers: vec![],
                work_area: None,
                targets: None,
            };
            let target = sequence.id;
            let project = Project {
                assets: assets.into_iter().map(DocumentObject::Known).collect(),
                sequences: vec![DocumentObject::Known(sequence)],
                ..Project::default()
            };
            let snapshot = RenderSnapshot::for_target(
                &project,
                RenderTarget::Sequence { sequence: target },
                1,
                RenderProfile::default(),
            )
            .unwrap();
            let mut session = MediaSession::sharing_runtime(Ok(&runtime));
            let mut samples = Vec::new();
            let mut raster_hashes = Vec::new();
            for frame in 0..24 {
                let scene = build_scene_ir(&snapshot, Time::new(frame, 30).unwrap(), &[]).unwrap();
                let dag = build_render_dag(
                    &scene,
                    RenderProfile::default(),
                    OutputRegion {
                        origin: [0.0; 2],
                        extent: [64.0, 36.0],
                        pixels: [64, 36],
                    },
                )
                .unwrap();
                let start = Instant::now();
                let resolved = resolve_dag_media_in(&dag, &directory, &mut session).unwrap();
                samples.push(start.elapsed().as_secs_f64() * 1000.0);
                let mut digest = Sha256::new();
                for node in resolved.dag.nodes() {
                    if let DagNode::RasterInput { pixels } = node {
                        for channel in pixels.iter().flatten() {
                            digest.update(channel.to_bits().to_le_bytes());
                        }
                    }
                }
                raster_hashes.push(format!("{:x}", digest.finalize()));
            }
            let mut warm = samples[1..].to_vec();
            warm.sort_by(f64::total_cmp);
            cases.push(json!({"clips":count,"shared_asset":shared,
                "cold_ms":samples[0],"warm_p50_ms":warm[11],"warm_p95_ms":warm[20],
                "samples_ms":samples,"raster_hashes":raster_hashes,
                "pool":session.decoder_pool_stats(),"decoders":session.decoder_stats(),
                "decoder_totals":session.decoder_stats_total()}));
        }
    }
    println!(
        "{}",
        json!({"source_hash":source_hash,"stream":stream,
        "path":"software decode/convert/sample; no GPU or DAG preparation", "cases":cases})
    );
}
