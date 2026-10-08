//! ADR-0118/0119 model contracts: proxy links and tracking data assets.
use kronello_model::*;
use kronello_time::{Rational, Time, TimeRange};

fn r(n: i64, d: i64) -> Rational {
    Rational::new(n, d).unwrap()
}
fn finite(v: f64) -> FiniteF64 {
    FiniteF64::new(v).unwrap()
}
fn video(id: AssetId, width: u32, height: u32, hash: char) -> Asset {
    Asset {
        id,
        content_hash: hash.to_string().repeat(64),
        kind: AssetKind::Video,
        streams: vec![StreamMetadata {
            index: 0,
            codec: "prores".into(),
            time_base: r(1, 24),
            duration: Some(r(4, 1)),
            start_time: None,
            width: Some(width),
            height: Some(height),
            pixel_format: None,
            color_primaries: None,
            color_transfer: None,
            color_matrix: None,
            color_range: None,
        }],
        locator: AssetLocator {
            relative: Some(format!("{id}.mov")),
            absolute: None,
        },
    }
}
fn link(original: AssetId, proxy: AssetId, hash: char) -> ProxyLink {
    ProxyLink {
        original,
        proxy,
        original_stream_index: 0,
        proxy_stream_index: 0,
        scale: finite(0.5),
        width: 160,
        height: 90,
        source_content_hash: hash.to_string().repeat(64),
        source_duration: Some(r(4, 1)),
        job: None,
    }
}
fn tracking_asset(id: AssetId, source_asset: AssetId, hash: char) -> TrackingDataAsset {
    let mut asset = TrackingDataAsset {
        id,
        version: TRACKING_VERSION,
        source: TrackingSource {
            asset: source_asset,
            stream_index: 0,
            content_hash: hash.to_string().repeat(64),
        },
        mode: TrackingMode::Points,
        seeds: vec![TrackingSeed {
            x: finite(0.5),
            y: finite(0.5),
            template_radius: 8,
            search_radius: 8,
        }],
        range: TimeRange::new(Time::ZERO, r(2, 1)).unwrap(),
        sample_rate: r(24, 1),
        frames: vec![
            TrackedFrame {
                time: Time::ZERO,
                points: vec![TrackedPoint {
                    x: finite(0.5),
                    y: finite(0.5),
                    confidence: finite(1.0),
                }],
                homography: None,
            },
            TrackedFrame {
                time: r(1, 24),
                points: vec![TrackedPoint {
                    x: finite(0.6),
                    y: finite(0.4),
                    confidence: finite(0.5),
                }],
                homography: None,
            },
        ],
        content_hash: String::new(),
    };
    asset.content_hash = asset.computed_hash().unwrap();
    asset
}

#[test]
fn proxy_dimensions_are_even_bounded_and_sane() {
    assert_eq!(proxy_dimensions(320, 180, 0.5), Some((160, 90)));
    assert_eq!(proxy_dimensions(321, 181, 0.5), Some((160, 90)));
    assert_eq!(proxy_dimensions(2, 2, 0.1), Some((2, 2)));
    assert_eq!(proxy_dimensions(3, 3, 1.0), Some((2, 2)));
    for (w, h, s) in [
        (0, 100, 0.5),
        (100, 1, 0.5),
        (100, 100, 0.0),
        (100, 100, -0.5),
        (100, 100, 1.5),
        (100, 100, f64::NAN),
        (100, 100, f64::INFINITY),
    ] {
        assert_eq!(proxy_dimensions(w, h, s), None, "{w}x{h}@{s}");
    }
}

#[test]
fn proxy_link_validation_and_document_state() {
    let original_id = AssetId::new();
    let proxy_id = AssetId::new();
    let mut document = Project::default();
    document
        .assets
        .push(DocumentObject::Known(video(original_id, 320, 180, 'a')));
    document
        .assets
        .push(DocumentObject::Known(video(proxy_id, 160, 90, 'b')));
    let valid = link(original_id, proxy_id, 'a');
    valid.validate().unwrap();
    document.proxies.push(valid);
    document.validate_storage().unwrap();

    // Structural failures.
    for mutate in [
        (|l: &mut ProxyLink| l.proxy = l.original),
        (|l: &mut ProxyLink| l.scale = finite(1.5)),
        (|l: &mut ProxyLink| l.width = 0),
        (|l: &mut ProxyLink| l.height = 33),
        (|l: &mut ProxyLink| l.source_duration = Some(Rational::ZERO)),
        (|l: &mut ProxyLink| l.source_content_hash = "xyz".into()),
        (|l: &mut ProxyLink| l.job = Some("not-a-uuid".into())),
    ] as [fn(&mut ProxyLink); 7]
    {
        let mut bad = link(original_id, proxy_id, 'a');
        mutate(&mut bad);
        assert!(bad.validate().is_err());
    }
    let mut ok_job = link(original_id, proxy_id, 'a');
    ok_job.job = Some(uuid::Uuid::new_v4().to_string());
    ok_job.validate().unwrap();

    // Stale states: hash drift, missing stream, wrong proxy dimensions.
    let mut stale_hash = link(original_id, proxy_id, 'c');
    assert!(document.proxy_link_state(&stale_hash).is_err());
    stale_hash.source_content_hash = 'a'.to_string().repeat(64);
    stale_hash.original_stream_index = 9;
    assert!(document.proxy_link_state(&stale_hash).is_err());
    let mut stale_dims = link(original_id, proxy_id, 'a');
    stale_dims.width = 320;
    assert!(document.proxy_link_state(&stale_dims).is_err());
    let mut missing = link(AssetId::new(), proxy_id, 'a');
    assert!(document.proxy_link_state(&missing).is_err());
    missing.original = original_id;
    missing.proxy = AssetId::new();
    assert!(document.proxy_link_state(&missing).is_err());
    assert_eq!(document.proxy_link(original_id).unwrap().proxy, proxy_id);

    // Duplicate membership is rejected at document level.
    let mut dup = document.clone();
    dup.proxies.push(link(original_id, AssetId::new(), 'a'));
    assert!(dup.validate_storage().is_err());
    let mut dup_proxy = document.clone();
    let extra_original = AssetId::new();
    dup_proxy
        .assets
        .push(DocumentObject::Known(video(extra_original, 320, 180, 'd')));
    let mut dup_link = link(extra_original, proxy_id, 'd');
    dup_link.proxy = proxy_id;
    dup_proxy.proxies.push(dup_link);
    assert!(dup_proxy.validate_storage().is_err());
}

#[test]
fn asset_in_use_covers_media_nodes_clips_and_bindings() {
    let asset_id = AssetId::new();
    let mut document = Project::default();
    document
        .assets
        .push(DocumentObject::Known(video(asset_id, 64, 64, 'a')));
    assert!(!document.asset_in_use(asset_id));
    // Sequence clip SourceRef::Asset usage.
    let sequence: Sequence = serde_json::from_value(serde_json::json!({
        "id": SequenceId::new(),
        "extent": {"width": 64.0, "height": 32.0},
        "frame_rate": {"num": "24", "den": "1"},
        "audio_rate": 48000,
        "working_space": "linear_rec709",
        "tracks": [{"id": TrackId::new(), "kind": "video", "clips": [{
            "id": ClipId::new(),
            "source_ref": {"kind": "asset", "asset": asset_id, "stream_index": 0},
            "timeline_range": {"start": {"num": "0", "den": "1"}, "end": {"num": "1", "den": "1"}},
            "source_in": {"num": "0", "den": "1"},
            "time_map": {"kind": "linear", "offset": {"num": "0", "den": "1"}, "speed": {"num": "1", "den": "1"}},
            "audio_retime": "reject", "links": [], "effects": []}]}]
    }))
    .unwrap();
    document.sequences.push(DocumentObject::Known(sequence));
    assert!(document.asset_in_use(asset_id));
}

#[test]
fn tracking_asset_hash_table_and_expression_view() {
    let source_id = AssetId::new();
    let asset = tracking_asset(AssetId::new(), source_id, 'a');
    asset.validate().unwrap();
    // Serialization round-trip preserves the exact object.
    let bytes = serde_json::to_vec(&asset).unwrap();
    let restored: TrackingDataAsset = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(restored, asset);
    restored.validate().unwrap();
    // Tampering breaks the content hash.
    let mut bad = asset.clone();
    bad.frames[0].points[0].x = finite(0.1);
    assert!(bad.validate().is_err());
    bad.frames[0].points[0].x = finite(0.5);
    bad.content_hash = "b".repeat(64);
    assert!(bad.validate().is_err());
    // Derived table columns and row values.
    let table = asset.data_table();
    for name in ["frame", "confidence", "x0", "y0", "score0"] {
        assert!(table.columns.contains_key(name), "{name}");
    }
    assert_eq!(table.rows.len(), 2);
    assert_eq!(table.rows[1]["x0"], Value::Scalar(finite(0.6)));
    assert_eq!(table.rows[1]["confidence"], Value::Scalar(finite(0.5)));
    // ExpressionDataAsset view shares the id and validates.
    let expression = asset.expression_asset().unwrap();
    assert_eq!(expression.id, asset.id);
    expression.validate().unwrap();

    // Project wiring: live source required, opaque rejection, expression input.
    let mut document = Project::default();
    document
        .assets
        .push(DocumentObject::Known(video(source_id, 64, 64, 'a')));
    document
        .tracking_data_assets
        .push(DocumentObject::Known(asset.clone()));
    let inputs = document.expression_data_inputs().unwrap();
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].id, asset.id);
    document.validate_storage().unwrap();
    // Stale source hash makes evaluation inputs fail; storage still holds the
    // versioned result until the source asset is repaired or removed.
    if let DocumentObject::Known(source) = &mut document.assets[0] {
        source.content_hash = "f".repeat(64);
    }
    assert!(document.expression_data_inputs().is_err());
}

#[test]
fn legacy_documents_without_new_fields_deserialize() {
    let document = Project::default();
    let mut json = serde_json::to_value(&document).unwrap();
    let object = json.as_object_mut().unwrap();
    assert!(!object.contains_key("proxies"));
    assert!(!object.contains_key("tracking_data_assets"));
    object.insert("proxies".into(), serde_json::Value::Null); // serde(default) still wins
    object.remove("proxies");
    let restored: Project = serde_json::from_value(json).unwrap();
    assert!(restored.proxies.is_empty() && restored.tracking_data_assets.is_empty());
    assert_eq!(
        serde_json::to_value(&restored).unwrap(),
        serde_json::to_value(&document).unwrap()
    );
}
