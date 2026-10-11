//! Release original/proxy preparation and software decode probe; no GUI/GPU.
use kronello_media::{MediaRuntime, MediaSession, content_hash, resolve_dag_media_in};
use kronello_model::*;
use kronello_render::{DagNode, MediaProxyMode};
use kronello_service::{BackendSelection, FrameRenderRequest, ProjectSession, Service};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{path::PathBuf, time::Instant};

fn main() {
    if cfg!(debug_assertions) {
        panic!("use --release");
    }
    let movie = PathBuf::from(std::env::args().nth(1).expect("movie"))
        .canonicalize()
        .unwrap();
    let directory = PathBuf::from(std::env::args().nth(2).expect("new output directory"));
    std::fs::create_dir_all(&directory).unwrap();
    let runtime = MediaRuntime::load().unwrap();
    let stream = runtime
        .open_video(&movie)
        .unwrap()
        .stream_metadata()
        .unwrap();
    let [width, height] = [stream.width.unwrap(), stream.height.unwrap()];
    let proxy_file = directory.join("proxy.mov");
    let dimensions = proxy_dimensions(width, height, 0.5).unwrap();
    let encoded = runtime
        .encode_proxy(
            &movie,
            stream.index,
            dimensions.0,
            dimensions.1,
            &proxy_file,
            &mut |_| Ok(()),
        )
        .unwrap();
    let proxy_stream = runtime
        .open_video(&proxy_file)
        .unwrap()
        .stream_metadata()
        .unwrap();
    let original = Asset {
        id: AssetId::new(),
        kind: AssetKind::Video,
        content_hash: content_hash(&movie).unwrap(),
        streams: vec![stream.clone()],
        locator: AssetLocator {
            relative: None,
            absolute: Some(movie.to_str().unwrap().into()),
        },
    };
    let proxy = Asset {
        id: AssetId::new(),
        kind: AssetKind::Video,
        content_hash: encoded.content_hash.clone(),
        streams: vec![proxy_stream.clone()],
        locator: AssetLocator {
            relative: None,
            absolute: Some(proxy_file.canonicalize().unwrap().to_str().unwrap().into()),
        },
    };
    let link = ProxyLink {
        original: original.id,
        proxy: proxy.id,
        original_stream_index: stream.index,
        proxy_stream_index: proxy_stream.index,
        scale: FiniteF64::new(0.5).unwrap(),
        width: dimensions.0,
        height: dimensions.1,
        source_content_hash: original.content_hash.clone(),
        source_duration: stream.duration,
        job: None,
    };
    let mut document: serde_json::Value =
        serde_json::from_str(include_str!("../../../examples/nle-001.project.json")).unwrap();
    let sequence = document["sequences"][0]["id"].clone();
    document["assets"] = json!([original, proxy]);
    document["proxies"] = json!([link]);
    document["sequences"][0]["extent"] = json!({"width":width,"height":height});
    document["sequences"][0]["tracks"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    document["sequences"][0]["tracks"][0]["clips"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    let clip = &mut document["sequences"][0]["tracks"][0]["clips"][0];
    clip["source_ref"] = json!({"kind":"asset","asset":original.id,"stream_index":stream.index});
    clip["source_in"] = json!(stream.start_time.unwrap_or(kronello_time::Time::ZERO));
    clip["timeline_range"] = json!({"start":{"num":"0","den":"1"},"end":{"num":"1","den":"1"}});
    let path = directory.join("proxy-preview.kronello");
    let service = Service::new(BackendSelection::CpuReference);
    let result = serde_json::to_value(service.execute_json(
        &json!({"operation":"project.create","project":path,"document":document}).to_string(),
    ))
    .unwrap();
    assert_eq!(result["status"], "success", "{result}");
    let session = ProjectSession::open(&path).unwrap();
    let mut cases = Vec::new();
    for mode in [MediaProxyMode::Off, MediaProxyMode::Prefer] {
        let mut media = MediaSession::sharing_runtime(Ok(&runtime));
        let mut prepare = Vec::new();
        let mut resolve = Vec::new();
        let mut hashes = Vec::new();
        let mut source_sizes = Vec::new();
        for frame in 0..22 {
            let request: FrameRenderRequest =
                serde_json::from_value(json!({"input":{"project":path,
                "target":{"kind":"sequence","sequence":sequence},"media_proxies":mode,
                "region":{"origin":[0,0],"extent":[width,height],"pixels":[320,180]}},
                "time":{"num":frame.to_string(),"den":"30"}}))
                .unwrap();
            let start = Instant::now();
            let (_, dag) = session
                .scope(|| service.preview_dag_unresolved(&request))
                .unwrap();
            prepare.push(start.elapsed().as_secs_f64() * 1000.0);
            let sizes: Vec<_> = dag
                .nodes()
                .iter()
                .filter_map(|node| match node {
                    DagNode::VideoDraw {
                        asset,
                        stream_index,
                        ..
                    } => {
                        let s = asset
                            .streams
                            .iter()
                            .find(|s| s.index == *stream_index)
                            .unwrap();
                        Some([s.width.unwrap(), s.height.unwrap()])
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(
                sizes,
                vec![if mode == MediaProxyMode::Off {
                    [width, height]
                } else {
                    [dimensions.0, dimensions.1]
                }]
            );
            source_sizes.push(sizes);
            let start = Instant::now();
            let resolved = resolve_dag_media_in(&dag, &path, &mut media).unwrap();
            resolve.push(start.elapsed().as_secs_f64() * 1000.0);
            let mut digest = Sha256::new();
            for node in resolved.dag.nodes() {
                if let DagNode::RasterInput { pixels } = node {
                    for channel in pixels.iter().flatten() {
                        assert!(channel.is_finite());
                        digest.update(channel.to_bits().to_le_bytes());
                    }
                }
            }
            hashes.push(format!("{:x}", digest.finalize()));
        }
        let stats = session.preparation_stats();
        cases.push(json!({"mode":mode,"prepare":summary(&prepare),"decode_convert_sample":summary(&resolve),
            "raster_hashes":hashes,"source_sizes":source_sizes,"preparations_cumulative":stats.preparations,
            "hits_cumulative":stats.hits,"decoder_pool":media.decoder_pool_stats(),"decoder_totals":media.decoder_stats_total()}));
    }
    println!(
        "{}",
        json!({"path":"same shared service snapshot and 320x180 output; software resolution, no GPU/GUI",
        "original":original,"proxy":proxy,"encoded_proxy":encoded,"cases":cases})
    );
}
fn summary(samples: &[f64]) -> serde_json::Value {
    let mut warm = samples[1..].to_vec();
    warm.sort_by(f64::total_cmp);
    json!({"cold_ms":samples[0],"warm_p50_ms":warm[10],"warm_p95_ms":warm[19],"samples_ms":samples})
}
