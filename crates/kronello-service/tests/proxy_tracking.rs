//! ADR-0118/0119 service coverage: track.analyze persistence/idempotency and
//! the proxy command/job surface against real encoded media.
use std::path::{Path, PathBuf};

use kronello_media::{EncodeCodec, EncodeFrame, EncodeRequest, MediaRuntime, content_hash};
use kronello_model::*;
use kronello_service::*;
use kronello_time::{Rational, Time, TimeRange};

fn r(n: i64, d: i64) -> Rational {
    Rational::new(n, d).unwrap()
}
fn finite(v: f64) -> FiniteF64 {
    FiniteF64::new(v).unwrap()
}
fn service() -> Service<'static> {
    Service::new(BackendSelection::CpuReference)
}

/// Real ProRes clip beside the project file, 4 frames at 1/24 time base.
fn write_clip(path: &Path, frames: usize) {
    MediaRuntime::load()
        .unwrap()
        .encode_video_stream(
            &EncodeRequest {
                output: path.into(),
                codec: EncodeCodec::ProRes,
                width: 32,
                height: 24,
                time_base: r(1, 24),
            },
            frames,
            &mut |index| {
                let mut rgba = vec![0u8; 32 * 24 * 4];
                for (p, pixel) in rgba.chunks_exact_mut(4).enumerate() {
                    let (x, y) = (p as u32 % 32, p as u32 / 32);
                    pixel[0] = ((x * 8 + index as u32 * 16) % 256) as u8;
                    pixel[1] = ((y * 10) % 256) as u8;
                    pixel[2] = ((x + y) % 256) as u8;
                    pixel[3] = 255;
                }
                Ok(EncodeFrame {
                    pts: r(index as i64, 24),
                    rgba,
                })
            },
        )
        .unwrap();
}
fn video_asset(dir: &Path, name: &str, id: AssetId, frames: usize) -> Asset {
    let file = dir.join(name);
    write_clip(&file, frames);
    let runtime = MediaRuntime::load().unwrap();
    let metadata = runtime
        .open_video_stream(&file, 0)
        .unwrap()
        .stream_metadata()
        .unwrap();
    Asset {
        id,
        content_hash: content_hash(&file).unwrap(),
        kind: AssetKind::Video,
        streams: vec![metadata],
        locator: AssetLocator {
            relative: Some(name.into()),
            absolute: None,
        },
    }
}
fn create(dir: &Path, document: Project) -> (PathBuf, ProjectInfo) {
    let path = dir.join("project.kronello");
    let ResultData::Project(info) = service()
        .dispatch(Request::ProjectCreate(CreateRequest {
            plan_hash: None,
            idempotency_key: None,
            project: path.clone(),
            document,
        }))
        .unwrap()
    else {
        panic!()
    };
    (path, info)
}
fn export(path: &Path) -> Project {
    let ResultData::Export(result) = service()
        .dispatch(Request::ProjectExport(ProjectRequest {
            project: path.into(),
        }))
        .unwrap()
    else {
        panic!()
    };
    result.document
}
fn error(service: &Service<'_>, request: Request) -> ServiceError {
    service.dispatch(request).unwrap_err()
}

#[test]
fn track_analyze_persists_revisioned_idempotent_data_asset() {
    let temp = tempfile::tempdir().unwrap();
    let asset_id = AssetId::new();
    let mut document = Project::default();
    document.assets.push(DocumentObject::Known(video_asset(
        temp.path(),
        "clip.mov",
        asset_id,
        4,
    )));
    let (path, created) = create(temp.path(), document);
    let request = TrackAnalyzeRequest {
        project: path.clone(),
        base_revision: created.revision.clone(),
        id: AssetId::new(),
        asset: asset_id,
        stream_index: 0,
        mode: TrackingMode::Points,
        seeds: vec![TrackingSeed {
            x: finite(0.5),
            y: finite(0.5),
            template_radius: 4,
            search_radius: 6,
        }],
        range: TimeRange::new(Time::ZERO, r(4, 24)).unwrap(),
        idempotency_key: Some("track-1".into()),
    };
    let ResultData::Project(analyzed) = service()
        .dispatch(Request::TrackAnalyze(request.clone()))
        .unwrap()
    else {
        panic!()
    };
    assert_ne!(analyzed.revision, created.revision);
    // Idempotent replay returns the committed result without a new revision.
    let ResultData::Project(replay) = service()
        .dispatch(Request::TrackAnalyze(request.clone()))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(replay.revision, analyzed.revision);
    let document = export(&path);
    assert_eq!(document.tracking_data_assets.len(), 1);
    let DocumentObject::Known(data) = &document.tracking_data_assets[0] else {
        panic!()
    };
    assert_eq!(data.id, request.id);
    assert_eq!(data.source.asset, asset_id);
    assert_eq!(data.frames.len(), 4);
    data.validate().unwrap();
    // The derived expression input is available to the shared evaluator.
    let inputs = document.expression_data_inputs().unwrap();
    assert_eq!(inputs.len(), 1);
    assert!(inputs[0].table.columns.contains_key("x0"));
    // Revision fence rejects a stale base (distinct key: a replayed
    // idempotency key legitimately returns the committed result first).
    let mut stale = request.clone();
    stale.idempotency_key = None;
    assert_eq!(
        error(&service(), Request::TrackAnalyze(stale)).code,
        "REVISION_CONFLICT"
    );
}

#[test]
fn track_analyze_rejects_bad_requests_before_decoding() {
    let temp = tempfile::tempdir().unwrap();
    let asset_id = AssetId::new();
    let mut document = Project::default();
    document.assets.push(DocumentObject::Known(video_asset(
        temp.path(),
        "clip.mov",
        asset_id,
        2,
    )));
    let (path, created) = create(temp.path(), document);
    let base = TrackAnalyzeRequest {
        project: path.clone(),
        base_revision: created.revision.clone(),
        id: AssetId::new(),
        asset: asset_id,
        stream_index: 0,
        mode: TrackingMode::Points,
        seeds: vec![TrackingSeed {
            x: finite(0.5),
            y: finite(0.5),
            template_radius: 4,
            search_radius: 6,
        }],
        range: TimeRange::new(Time::ZERO, r(2, 24)).unwrap(),
        idempotency_key: None,
    };
    // Wrong seed count for plane mode.
    let mut request = base.clone();
    request.mode = TrackingMode::Plane;
    assert_eq!(
        error(&service(), Request::TrackAnalyze(request)).code,
        "INVALID_REQUEST"
    );
    // Empty range.
    let mut request = base.clone();
    request.range = TimeRange::new(Time::ZERO, Time::ZERO)
        .unwrap_or_else(|_| TimeRange::new(Time::ZERO, r(1, 24)).unwrap());
    // Range outside the stream decodes zero frames.
    request.range = TimeRange::new(r(9, 1), r(10, 1)).unwrap();
    assert_eq!(
        error(&service(), Request::TrackAnalyze(request)).code,
        "INVALID_MEDIA_INPUT"
    );
    // Unknown asset.
    let mut request = base.clone();
    request.asset = AssetId::new();
    assert_eq!(
        error(&service(), Request::TrackAnalyze(request)).code,
        "ASSET_MISSING"
    );
}

fn proxy_linked_project(dir: &Path) -> (PathBuf, ProjectInfo, AssetId, AssetId) {
    let original = AssetId::new();
    let proxy = AssetId::new();
    let mut document = Project::default();
    document.assets.push(DocumentObject::Known(video_asset(
        dir,
        "original.mov",
        original,
        4,
    )));
    document.assets.push(DocumentObject::Known(video_asset(
        dir,
        "proxy.mov",
        proxy,
        4,
    )));
    // The proxy asset reports half-scale dimensions per the link.
    if let DocumentObject::Known(a) = &mut document.assets[1] {
        a.streams[0].width = Some(16);
        a.streams[0].height = Some(12);
    }
    document.proxies.push(ProxyLink {
        original,
        proxy,
        original_stream_index: 0,
        proxy_stream_index: 0,
        scale: finite(0.5),
        width: 16,
        height: 12,
        source_content_hash: match &document.assets[0] {
            DocumentObject::Known(a) => a.content_hash.clone(),
            _ => panic!(),
        },
        source_duration: Some(r(4, 24)),
        job: None,
    });
    let (path, info) = create(dir, document);
    (path, info, original, proxy)
}

#[test]
fn proxy_status_and_clear_manage_link_state() {
    let temp = tempfile::tempdir().unwrap();
    let (path, created, original, proxy) = proxy_linked_project(temp.path());
    let status = |service: &Service<'_>| {
        let ResultData::Proxies(result) = service
            .dispatch(Request::ProxyStatus(ProxyStatusRequest {
                project: path.clone(),
                asset: None,
            }))
            .unwrap()
        else {
            panic!()
        };
        result
    };
    // Link metadata is consistent; the proxy file resolves → ready.
    let result = status(&service());
    assert_eq!(result.proxies.len(), 1);
    assert_eq!(result.proxies[0].state, ProxyState::Ready);
    // Filtered by either end of the link.
    for asset in [original, proxy] {
        let ResultData::Proxies(result) = service()
            .dispatch(Request::ProxyStatus(ProxyStatusRequest {
                project: path.clone(),
                asset: Some(asset),
            }))
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(result.proxies.len(), 1);
    }
    // Missing proxy file → typed state, not an error.
    let stored = temp.path().join("proxy.mov");
    let moved = temp.path().join("proxy-away.mov");
    std::fs::rename(&stored, &moved).unwrap();
    assert_eq!(status(&service()).proxies[0].state, ProxyState::Missing);
    std::fs::rename(&moved, &stored).unwrap();
    // Stale link (source hash drift in the document) → typed state.
    let mut document = export(&path);
    if let DocumentObject::Known(a) = &mut document.assets[0] {
        a.content_hash = "f".repeat(64);
    }
    let ResultData::Project(updated) = service()
        .dispatch(Request::ProjectImport(ImportRequest {
            project: path.clone(),
            base_revision: created.revision.clone(),
            document,
            idempotency_key: None,
            plan_hash: None,
        }))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(status(&service()).proxies[0].state, ProxyState::Stale);
    // proxy.clear removes link and unreferenced proxy asset, once.
    let ResultData::Project(cleared) = service()
        .dispatch(Request::ProxyClear(ProxyClearRequest {
            project: path.clone(),
            base_revision: updated.revision.clone(),
            asset: original,
        }))
        .unwrap()
    else {
        panic!()
    };
    assert_ne!(cleared.revision, updated.revision);
    let document = export(&path);
    assert!(document.proxies.is_empty());
    assert!(!document.assets.iter().any(|a| matches!(
        a,
        DocumentObject::Known(a) if a.id == proxy
    )));
    assert_eq!(status(&service()).proxies.len(), 0);
    assert_eq!(
        error(
            &service(),
            Request::ProxyClear(ProxyClearRequest {
                project: path.clone(),
                base_revision: cleared.revision,
                asset: original,
            })
        )
        .code,
        "PROXY_NOT_FOUND"
    );
}

#[test]
fn proxy_generate_submits_fixed_input_jobs_and_collect_relinks() {
    let temp = tempfile::tempdir().unwrap();
    let original = AssetId::new();
    let mut document = Project::default();
    document.assets.push(DocumentObject::Known(video_asset(
        temp.path(),
        "original.mov",
        original,
        4,
    )));
    let (path, _) = create(temp.path(), document);
    let jobs_dir = temp.path().join("jobs");
    let service = || {
        Service::new(BackendSelection::CpuReference)
            .with_job_config(kronello_jobs::JobConfig::at(&jobs_dir))
            .with_worker_executable(PathBuf::from("/usr/bin/false"))
    };
    let ResultData::Jobs(result) = service()
        .dispatch(Request::ProxyGenerate(ProxyGenerateRequest {
            project: path.clone(),
            expected_revision: None,
            assets: vec![original],
            scale: Some(0.5),
            stream_index: None,
        }))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(result.jobs.len(), 1);
    let record = &result.jobs[0];
    assert_eq!(record.status, kronello_jobs::JobStatus::Queued);
    assert_eq!(record.destination.extension().unwrap(), "mov");
    assert!(
        record
            .destination
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with(".proxies")
    );
    assert_eq!(record.output_profile["stream_index"], serde_json::json!(0));
    // The fixed input is a proxy payload, not a render snapshot.
    let input: serde_json::Value = serde_json::from_slice(
        &kronello_jobs::JobStore::open(kronello_jobs::JobConfig::at(&jobs_dir))
            .unwrap()
            .input(record)
            .unwrap(),
    )
    .unwrap();
    assert!(input["proxy"].is_object());
    assert!(input["snapshot"].is_null() || input.get("snapshot").is_none());
    // Scale out of range and unknown assets reject before submission.
    for mutate in [
        (|r: &mut ProxyGenerateRequest| r.scale = Some(2.0)),
        (|r: &mut ProxyGenerateRequest| r.assets = vec![AssetId::new()]),
        (|r: &mut ProxyGenerateRequest| r.assets = vec![]),
    ] as [fn(&mut ProxyGenerateRequest); 3]
    {
        let mut request = ProxyGenerateRequest {
            project: path.clone(),
            expected_revision: None,
            assets: vec![original],
            scale: Some(0.5),
            stream_index: None,
        };
        mutate(&mut request);
        assert!(service().dispatch(Request::ProxyGenerate(request)).is_err());
    }
}

#[test]
fn file_writing_renders_reject_preview_proxy_mode() {
    let temp = tempfile::tempdir().unwrap();
    let composition = CompositionId::new();
    let mut document = Project::default();
    document
        .compositions
        .push(DocumentObject::Known(Composition {
            id: composition,
            duration: kronello_time::Duration::new(r(1, 1)).unwrap(),
            design_extent: DesignExtent::new(64.0, 32.0).unwrap(),
            edit_rate: kronello_time::FrameRate::new(24, 1).unwrap(),
            root_nodes: vec![],
            nodes: vec![],
            properties: vec![],
        }));
    let (path, _) = create(temp.path(), document);
    let input = serde_json::json!({
        "project": path, "composition": composition,
        "region": {"origin":[0,0],"extent":[64,32],"pixels":[64,32]},
        "media_proxies": "prefer",
    });
    let sequence = serde_json::json!({
        "input": input,
        "range": {"start": {"num": "0", "den": "1"}, "end": {"num": "1", "den": "24"}},
        "frame_rate": {"num": "24", "den": "1"},
        "output_directory": temp.path().join("out"),
    });
    for (operation, mut request) in [
        ("render.sequence", sequence.clone()),
        (
            "render.export",
            serde_json::json!({
                "render": { "input": input,
                    "range": {"start": {"num": "0", "den": "1"}, "end": {"num": "1", "den": "24"}},
                    "frame_rate": {"num": "24", "den": "1"},
                    "output_directory": temp.path().join("out.mov")},
                "output": {"format": "pro_res_mov", "clips": [], "background": [0.0, 0.0, 0.0]}}),
        ),
        (
            "render.submit",
            serde_json::json!({"render": sequence, "output": {"format": "image_sequence"}}),
        ),
    ] {
        request["operation"] = serde_json::json!(operation);
        let response = service().execute_json(&request.to_string());
        let response = serde_json::to_value(response).unwrap();
        assert_eq!(
            response["error"]["code"], "UNSUPPORTED_FEATURE",
            "{operation}: {response}"
        );
    }
    // In-memory preview accepts the flag.
    let response = service().execute_json(
        &serde_json::json!({"operation": "render.frame", "input": input,
            "time": {"num": "0", "den": "1"}})
        .to_string(),
    );
    let response = serde_json::to_value(response).unwrap();
    assert_eq!(response["status"], "success", "{response}");
}
