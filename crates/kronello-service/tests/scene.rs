//! AI-002 (ADR-0125) service coverage: `scene.detect` fixed-input job
//! submission and the `scene.apply` shared edit (markers and splits) with
//! revision/idempotency/undo semantics reused unchanged.
use std::path::{Path, PathBuf};

use kronello_media::{EncodeCodec, EncodeFrame, EncodeRequest, MediaRuntime, content_hash};
use kronello_model::*;
use kronello_service::*;
use kronello_time::{FrameRate, Rational, SampleRate, Time, TimeMap, TimeRange};

fn r(n: i64, d: i64) -> Rational {
    Rational::new(n, d).unwrap()
}
fn finite(v: f64) -> FiniteF64 {
    FiniteF64::new(v).unwrap()
}
fn service() -> Service<'static> {
    Service::new(BackendSelection::CpuReference)
}
fn error(service: &Service<'_>, request: Request) -> ServiceError {
    service.dispatch(request).unwrap_err()
}

/// Two-scene ProRes clip: frames 0..4 dark, frames 4..8 bright.
fn write_clip(path: &Path) {
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
            8,
            &mut |index| {
                let value = if index < 4 { 16u8 } else { 240u8 };
                let mut rgba = vec![0u8; 32 * 24 * 4];
                for pixel in rgba.chunks_exact_mut(4) {
                    pixel.copy_from_slice(&[value, value, value, 255]);
                }
                Ok(EncodeFrame {
                    pts: r(index as i64, 24),
                    rgba,
                })
            },
        )
        .unwrap();
}
fn video_asset(dir: &Path, name: &str, id: AssetId) -> Asset {
    let file = dir.join(name);
    write_clip(&file);
    let metadata = MediaRuntime::load()
        .unwrap()
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

/// A boundary asset over the locked source; the detect worker produces the
/// same shape, but apply tests need no decode at all.
fn boundary_asset(id: AssetId, asset: &Asset, boundaries: Vec<(i64, f64)>) -> SceneBoundaryAsset {
    let mut data = SceneBoundaryAsset {
        id,
        version: SCENE_BOUNDARY_VERSION,
        source: SceneSource {
            asset: asset.id,
            stream_index: 0,
            content_hash: asset.content_hash.clone(),
        },
        params: SceneDetectionParams::default(),
        range: TimeRange::new(Time::ZERO, r(8, 24)).unwrap(),
        frames_analyzed: 8,
        boundaries: boundaries
            .into_iter()
            .map(|(tick, confidence)| SceneBoundary {
                time: r(tick, 24),
                confidence: finite(confidence),
            })
            .collect(),
        content_hash: String::new(),
    };
    data.content_hash = data.computed_hash().unwrap();
    data
}

fn clip(asset: AssetId, source_in: Time, start: Time, end: Time) -> Clip {
    Clip {
        id: ClipId::new(),
        source_ref: SourceRef::Asset {
            asset,
            stream_index: 0,
        },
        timeline_range: TimeRange::new(start, end).unwrap(),
        source_in,
        time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
        enabled: true,
        audio_retime: AudioRetimePolicy::Reject,
        reverse_sampling: None,
        volume: None,
        links: vec![],
        effects: vec![],
        masks: vec![],
        properties: vec![],
        markers: vec![],
    }
}

/// Project with one video asset, one clip covering source times
/// `[2/24, 8/24)` on `[10/24, 16/24)`, and a boundary asset with cuts at
/// source 4/24 (conf 0.8) and 6/24 (conf 0.5), mapping to 12/24 and 14/24.
/// Ids are caller-provided so identical documents across directories share
/// every deterministic identity.
fn apply_fixture(
    dir: &Path,
    asset_id: AssetId,
    scene_id: AssetId,
) -> (PathBuf, ProjectInfo, SequenceId) {
    let sequence_id = SequenceId::new();
    let asset = video_asset(dir, "clip.mov", asset_id);
    let boundaries = boundary_asset(scene_id, &asset, vec![(4, 0.8), (6, 0.5)]);
    let mut document = Project::default();
    document.assets.push(DocumentObject::Known(asset));
    document
        .scene_boundary_assets
        .push(DocumentObject::Known(boundaries));
    document.sequences.push(DocumentObject::Known(Sequence {
        id: sequence_id,
        extent: DesignExtent::new(32.0, 24.0).unwrap(),
        frame_rate: FrameRate::new(24, 1).unwrap(),
        audio_rate: SampleRate::HZ_48000,
        working_space: ColorSpace::LinearRec709,
        tracks: vec![Track {
            state: None,
            id: TrackId::new(),
            kind: TrackKind::Video,
            clips: vec![clip(asset_id, r(2, 24), r(10, 24), r(16, 24))],
        }],
        transitions: vec![],
        markers: vec![],
        work_area: None,
        targets: None,
    }));
    let (path, info) = create(dir, document);
    (path, info, sequence_id)
}

fn apply_request(
    path: &Path,
    revision: &str,
    scene: AssetId,
    sequence: SequenceId,
    mode: SceneApplyMode,
) -> SceneApplyRequest {
    SceneApplyRequest {
        project: path.into(),
        base_revision: revision.into(),
        idempotency_key: uuid::Uuid::new_v4().to_string(),
        session_id: uuid::Uuid::new_v4(),
        scene_asset: scene,
        sequence,
        mode,
        track: None,
        clip: None,
        min_confidence: None,
    }
}

fn job_service(jobs_dir: &Path) -> Service<'static> {
    Service::new(BackendSelection::CpuReference)
        .with_job_config(kronello_jobs::JobConfig::at(jobs_dir))
        .with_worker_executable(PathBuf::from("/usr/bin/false"))
}

#[test]
fn scene_detect_submits_fixed_input_job_and_validates_input() {
    let temp = tempfile::tempdir().unwrap();
    let asset_id = AssetId::new();
    let mut document = Project::default();
    document.assets.push(DocumentObject::Known(video_asset(
        temp.path(),
        "clip.mov",
        asset_id,
    )));
    let (path, _) = create(temp.path(), document);
    let jobs_dir = temp.path().join("jobs");
    let request = |project: PathBuf| SceneDetectRequest {
        project,
        expected_revision: None,
        id: None,
        asset: asset_id,
        stream_index: 0,
        range: None,
        params: None,
    };
    let ResultData::Job(record) = job_service(&jobs_dir)
        .dispatch(Request::SceneDetect(request(path.clone())))
        .unwrap()
    else {
        panic!()
    };
    // One `<stem>.scene/<asset>.json` managed receipt destination.
    assert_eq!(record.destination.extension().unwrap(), "json");
    assert_eq!(
        record.destination.parent().unwrap().file_name().unwrap(),
        "project.scene"
    );
    // The scene asset id is preallocated into the fixed input profile.
    let scene_asset_id: AssetId =
        serde_json::from_value(record.output_profile["scene_asset_id"].clone()).unwrap();
    assert_eq!(
        record.destination.file_stem().unwrap().to_string_lossy(),
        scene_asset_id.to_string()
    );
    // The fixed input is a scene payload, not a render snapshot or proxy.
    let input: serde_json::Value = serde_json::from_slice(
        &kronello_jobs::JobStore::open(kronello_jobs::JobConfig::at(&jobs_dir))
            .unwrap()
            .input(&record)
            .unwrap(),
    )
    .unwrap();
    assert!(input["scene"].is_object());
    assert!(input.get("snapshot").is_none() || input["snapshot"].is_null());
    assert!(input.get("proxy").is_none() || input["proxy"].is_null());
    assert_eq!(input["scene"]["asset"]["id"], serde_json::json!(asset_id));
    // Typed rejections before any job exists.
    for (mutate, code) in [
        (
            (|r: &mut SceneDetectRequest| r.asset = AssetId::new()) as fn(&mut SceneDetectRequest),
            "ASSET_MISSING",
        ),
        (
            |q: &mut SceneDetectRequest| {
                q.range = Some(TimeRange::new(r(0, 1), r(400, 1)).unwrap())
            },
            "SCENE_BUDGET_EXCEEDED",
        ),
        (
            |q: &mut SceneDetectRequest| q.range = Some(TimeRange::new(r(1, 1), r(1, 1)).unwrap()),
            "INVALID_REQUEST",
        ),
        (
            |r: &mut SceneDetectRequest| r.stream_index = 9,
            "INVALID_MEDIA_INPUT",
        ),
        (
            |r: &mut SceneDetectRequest| r.expected_revision = Some("0".into()),
            "REVISION_CONFLICT",
        ),
    ] {
        let mut request = request(path.clone());
        mutate(&mut request);
        assert_eq!(
            error(&job_service(&jobs_dir), Request::SceneDetect(request)).code,
            code
        );
    }
    // An explicit id equal to an existing asset id rejects too.
    let mut duplicate = request(path.clone());
    duplicate.id = Some(asset_id);
    assert_eq!(
        error(&job_service(&jobs_dir), Request::SceneDetect(duplicate)).code,
        "INVALID_REQUEST"
    );
}

#[test]
fn scene_apply_markers_maps_boundaries_with_deterministic_ids() {
    let asset_id = AssetId::new();
    let scene_id = AssetId::new();
    let mut marker_ids: Vec<Vec<MarkerId>> = Vec::new();
    for dir in [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()] {
        let (path, created, sequence_id) = apply_fixture(dir.path(), asset_id, scene_id);
        let ResultData::Edit(event) = service()
            .dispatch(Request::SceneApply(apply_request(
                &path,
                &created.revision,
                scene_id,
                sequence_id,
                SceneApplyMode::Markers,
            )))
            .unwrap()
        else {
            panic!()
        };
        assert_ne!(event.revision.to_string(), created.revision);
        let document = export(&path);
        let DocumentObject::Known(sequence) = &document.sequences[0] else {
            panic!()
        };
        let mut times: Vec<_> = sequence.markers.iter().map(|m| m.time).collect();
        times.sort();
        // Source cuts 4/24, 6/24 map through source_in 2/24 to 12/24, 14/24.
        assert_eq!(times, vec![r(12, 24), r(14, 24)]);
        assert!(sequence.markers.iter().all(|m| {
            m.comment
                .as_deref()
                .unwrap_or("")
                .starts_with("scene boundary")
        }));
        let mut ids: Vec<_> = sequence.markers.iter().map(|m| m.id).collect();
        ids.sort();
        marker_ids.push(ids);
    }
    // Marker identity is a deterministic function of (asset, sequence time):
    // identical documents in different directories produce identical ids.
    assert_eq!(marker_ids[0], marker_ids[1]);
}

#[test]
fn scene_apply_markers_is_idempotent_and_undoable() {
    let temp = tempfile::tempdir().unwrap();
    let (path, created, sequence_id) = apply_fixture(temp.path(), AssetId::new(), AssetId::new());
    let scene_id = {
        let DocumentObject::Known(data) = &export(&path).scene_boundary_assets[0] else {
            panic!()
        };
        data.id
    };
    let request = apply_request(
        &path,
        &created.revision,
        scene_id,
        sequence_id,
        SceneApplyMode::Markers,
    );
    let ResultData::Edit(event) = service()
        .dispatch(Request::SceneApply(request.clone()))
        .unwrap()
    else {
        panic!()
    };
    // Replay with the same key returns the committed event, no new revision.
    let ResultData::Edit(replay) = service()
        .dispatch(Request::SceneApply(request.clone()))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(replay.id, event.id);
    assert_eq!(replay.revision, event.revision);
    // Same key with a different semantic request is a typed reuse error.
    let mut different = request.clone();
    different.min_confidence = Some(0.9);
    assert_eq!(
        error(&service(), Request::SceneApply(different)).code,
        "IDEMPOTENCY_KEY_REUSED"
    );
    // edit.undo removes every generated marker as one undo unit.
    service()
        .dispatch(Request::EditUndo(UndoRequest {
            project: path.clone(),
            base_revision: event.revision.to_string(),
            session_id: uuid::Uuid::new_v4(),
            idempotency_key: uuid::Uuid::new_v4().to_string(),
            event_id: event.id,
        }))
        .unwrap();
    let document = export(&path);
    let DocumentObject::Known(sequence) = &document.sequences[0] else {
        panic!()
    };
    assert!(sequence.markers.is_empty());
}

#[test]
fn scene_apply_split_creates_sequential_pieces_and_replays() {
    let temp = tempfile::tempdir().unwrap();
    let (path, created, sequence_id) = apply_fixture(temp.path(), AssetId::new(), AssetId::new());
    let scene_id = {
        let DocumentObject::Known(data) = &export(&path).scene_boundary_assets[0] else {
            panic!()
        };
        data.id
    };
    let request = apply_request(
        &path,
        &created.revision,
        scene_id,
        sequence_id,
        SceneApplyMode::Split,
    );
    let ResultData::Edit(event) = service()
        .dispatch(Request::SceneApply(request.clone()))
        .unwrap()
    else {
        panic!()
    };
    let document = export(&path);
    let DocumentObject::Known(sequence) = &document.sequences[0] else {
        panic!()
    };
    let clips = &sequence.tracks[0].clips;
    // Two mapped cuts produce three sequential pieces on the shared path.
    assert_eq!(clips.len(), 3);
    let mut ranges: Vec<_> = clips
        .iter()
        .map(|c| (c.timeline_range.start(), c.timeline_range.end()))
        .collect();
    ranges.sort();
    assert_eq!(
        ranges,
        vec![
            (r(10, 24), r(12, 24)),
            (r(12, 24), r(14, 24)),
            (r(14, 24), r(16, 24)),
        ]
    );
    // Middle and right pieces resume at the mapped source times: 2/24 source
    // in + 2/24,4/24 elapsed gives source_in 4/24 and 6/24.
    let mut middle_right: Vec<_> = clips
        .iter()
        .filter(|c| c.timeline_range.start() > r(10, 24))
        .collect();
    middle_right.sort_by_key(|c| c.timeline_range.start());
    assert_eq!(middle_right[0].source_in, r(4, 24));
    assert_eq!(middle_right[1].source_in, r(6, 24));
    // The original clip id stays on the left piece; right ids are derived.
    let original = clips
        .iter()
        .min_by_key(|c| c.timeline_range.start())
        .unwrap()
        .id;
    assert!(middle_right.iter().all(|c| c.id != original));
    // An identical retry replays the committed event without new edits.
    let ResultData::Edit(replay) = service().dispatch(Request::SceneApply(request)).unwrap() else {
        panic!()
    };
    assert_eq!(replay.id, event.id);
}

#[test]
fn scene_apply_filters_scope_and_rejects_empty_stale_or_locked() {
    let temp = tempfile::tempdir().unwrap();
    let (path, created, sequence_id) = apply_fixture(temp.path(), AssetId::new(), AssetId::new());
    let scene_id = {
        let DocumentObject::Known(data) = &export(&path).scene_boundary_assets[0] else {
            panic!()
        };
        data.id
    };
    // min_confidence keeps only the 0.8 boundary -> a single marker.
    let mut request = apply_request(
        &path,
        &created.revision,
        scene_id,
        sequence_id,
        SceneApplyMode::Markers,
    );
    request.min_confidence = Some(0.6);
    service().dispatch(Request::SceneApply(request)).unwrap();
    let DocumentObject::Known(sequence) = &export(&path).sequences[0] else {
        panic!()
    };
    assert_eq!(sequence.markers.len(), 1);
    assert_eq!(sequence.markers[0].time, r(12, 24));

    // Missing assets and sequences are typed errors.
    for (code, mutate) in [
        ("ASSET_MISSING", |r: &mut SceneApplyRequest| {
            r.scene_asset = AssetId::new();
        }),
        ("SOURCE_MISSING", |r: &mut SceneApplyRequest| {
            r.sequence = SequenceId::new();
        }),
    ] as [(&str, fn(&mut SceneApplyRequest)); 2]
    {
        let temp = tempfile::tempdir().unwrap();
        let (path, created, sequence_id) =
            apply_fixture(temp.path(), AssetId::new(), AssetId::new());
        let scene_id = {
            let DocumentObject::Known(data) = &export(&path).scene_boundary_assets[0] else {
                panic!()
            };
            data.id
        };
        let mut request = apply_request(
            &path,
            &created.revision,
            scene_id,
            sequence_id,
            SceneApplyMode::Markers,
        );
        mutate(&mut request);
        assert_eq!(error(&service(), Request::SceneApply(request)).code, code);
    }
    // A track scope with no mapped boundary rejects with INVALID_REQUEST.
    let temp = tempfile::tempdir().unwrap();
    let (path, created, sequence_id) = apply_fixture(temp.path(), AssetId::new(), AssetId::new());
    let scene_id = {
        let DocumentObject::Known(data) = &export(&path).scene_boundary_assets[0] else {
            panic!()
        };
        data.id
    };
    let mut request = apply_request(
        &path,
        &created.revision,
        scene_id,
        sequence_id,
        SceneApplyMode::Markers,
    );
    request.track = Some(TrackId::new());
    assert_eq!(
        error(&service(), Request::SceneApply(request)).code,
        "INVALID_REQUEST"
    );

    // Split on a locked track reuses the shared TRACK_LOCKED contract.
    let temp = tempfile::tempdir().unwrap();
    let (path, created, sequence_id) = apply_fixture(temp.path(), AssetId::new(), AssetId::new());
    let scene_id = {
        let DocumentObject::Known(data) = &export(&path).scene_boundary_assets[0] else {
            panic!()
        };
        data.id
    };
    let mut document = export(&path);
    let DocumentObject::Known(sequence) = &mut document.sequences[0] else {
        panic!()
    };
    sequence.tracks[0].state = Some(TrackState {
        visible: true,
        muted: false,
        locked: true,
    });
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
    let request = apply_request(
        &path,
        &updated.revision,
        scene_id,
        sequence_id,
        SceneApplyMode::Split,
    );
    assert_eq!(
        error(&service(), Request::SceneApply(request)).code,
        "TRACK_LOCKED"
    );

    // Stale boundary source (content drift) is a typed error, never applied.
    let temp = tempfile::tempdir().unwrap();
    let (path, created, sequence_id) = apply_fixture(temp.path(), AssetId::new(), AssetId::new());
    let scene_id = {
        let DocumentObject::Known(data) = &export(&path).scene_boundary_assets[0] else {
            panic!()
        };
        data.id
    };
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
    let request = apply_request(
        &path,
        &updated.revision,
        scene_id,
        sequence_id,
        SceneApplyMode::Markers,
    );
    assert_eq!(
        error(&service(), Request::SceneApply(request)).code,
        "INVALID_REQUEST"
    );
}

#[test]
fn scene_operations_roundtrip_through_tagged_wire_json() {
    let temp = tempfile::tempdir().unwrap();
    let asset_id = AssetId::new();
    let mut document = Project::default();
    document.assets.push(DocumentObject::Known(video_asset(
        temp.path(),
        "clip.mov",
        asset_id,
    )));
    let (path, created) = create(temp.path(), document);
    let jobs_dir = temp.path().join("jobs");
    let response = job_service(&jobs_dir).execute_json(
        &serde_json::json!({
            "operation": "scene.detect",
            "project": path,
            "asset": asset_id,
            "stream_index": 0,
        })
        .to_string(),
    );
    let response = serde_json::to_value(response).unwrap();
    assert_eq!(response["status"], "success", "{response}");
    assert_eq!(response["result"]["kind"], "job");
    let response = job_service(&jobs_dir).execute_json(
        &serde_json::json!({
            "operation": "scene.apply",
            "project": path,
            "base_revision": created.revision,
            "idempotency_key": "k",
            "session_id": uuid::Uuid::new_v4(),
            "scene_asset": AssetId::new(),
            "sequence": SequenceId::new(),
            "mode": "markers",
        })
        .to_string(),
    );
    let response = serde_json::to_value(response).unwrap();
    // Unknown boundary asset reaches typed dispatch, not a transport error.
    assert_eq!(response["status"], "error");
    assert_eq!(response["error"]["code"], "ASSET_MISSING");
}
