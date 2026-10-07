//! NLE-004: persistent sequence/clip markers and the In/Out work area share
//! the typed TimelineCommand path, selective undo, and explicit render range.
use kronello_model::*;
use kronello_render::OutputRegion;
use kronello_service::*;
use kronello_time::{FrameRate, Rational, SampleRate, Time, TimeMap, TimeRange};
use std::path::{Path, PathBuf};
use uuid::Uuid;

fn t(n: i64, d: i64) -> Time {
    Time::new(n, d).unwrap()
}
fn range(a: Time, b: Time) -> TimeRange {
    TimeRange::new(a, b).unwrap()
}
fn clip(a: Time, b: Time) -> Clip {
    Clip {
        id: ClipId::new(),
        source_ref: SourceRef::Generator {
            generator: SOLID_GENERATOR_ID.into(),
            version: 1,
            color: Color::from_srgb8([90, 40, 120], None),
        },
        timeline_range: range(a, b),
        source_in: Time::ZERO,
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
    }
}
fn sequence(clips: Vec<Clip>) -> Sequence {
    Sequence {
        id: SequenceId::new(),
        extent: DesignExtent::new(16.0, 16.0).unwrap(),
        frame_rate: FrameRate::new(24, 1).unwrap(),
        audio_rate: SampleRate::HZ_48000,
        working_space: ColorSpace::LinearRec709,
        tracks: vec![Track {
            state: None,
            id: TrackId::new(),
            kind: TrackKind::Video,
            clips,
        }],
        transitions: vec![],
        markers: vec![],
        work_area: None,
        targets: None,
    }
}
fn marker(time: Time, color: MarkerColor, comment: Option<&str>) -> Marker {
    Marker {
        id: MarkerId::new(),
        time,
        color,
        comment: comment.map(str::to_owned),
    }
}
fn service() -> Service<'static> {
    Service::new(BackendSelection::CpuReference)
}
fn setup(p: Project) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nle4.kronello");
    service()
        .dispatch(Request::ProjectCreate(CreateRequest {
            plan_hash: None,
            idempotency_key: None,
            project: path.clone(),
            document: p,
        }))
        .unwrap();
    (dir, path)
}
fn project(sequence: Sequence) -> Project {
    let mut p = Project::default();
    p.sequences.push(DocumentObject::Known(sequence));
    p
}
fn export(path: &Path) -> ExportResult {
    let ResultData::Export(p) = service()
        .dispatch(Request::ProjectExport(ProjectRequest {
            project: path.into(),
        }))
        .unwrap()
    else {
        panic!()
    };
    *p
}
fn query(path: &Path, sequence: SequenceId) -> SequenceQueryResult {
    let ResultData::Timeline(result) = service()
        .dispatch(Request::SequenceQuery(SequenceQueryRequest {
            project: path.into(),
            sequence,
        }))
        .unwrap()
    else {
        panic!()
    };
    result
}
fn plan(path: &Path, commands: Vec<TimelineCommand>) -> Result<EditApplyRequest, ServiceError> {
    let commands = commands
        .into_iter()
        .map(|c| EditCommand::Timeline(Box::new(c)))
        .collect::<Vec<_>>();
    let revision = export(path).revision;
    let ResultData::Plan(plan) = service().dispatch(Request::EditPlan(PlanRequest {
        project: path.into(),
        base_revision: revision.clone(),
        commands: commands.clone(),
    }))?
    else {
        panic!()
    };
    Ok(EditApplyRequest {
        project: path.into(),
        base_revision: revision,
        plan_hash: plan.plan_hash,
        idempotency_key: Uuid::new_v4().to_string(),
        session_id: Uuid::new_v4(),
        commands,
    })
}
fn apply(path: &Path, commands: Vec<TimelineCommand>) -> kronello_store::Event {
    let ResultData::Edit(e) = service()
        .dispatch(Request::EditApply(plan(path, commands).unwrap()))
        .unwrap()
    else {
        panic!()
    };
    e
}
fn reject(path: &Path, commands: Vec<TimelineCommand>, code: &str) {
    let before = export(path);
    let e = plan(path, commands).unwrap_err();
    assert_eq!(e.code, code, "{e:?}");
    let after = export(path);
    assert_eq!(after.document, before.document);
    assert_eq!(after.revision, before.revision);
}
fn undo(path: &Path, event: Uuid) -> Result<ResultData, ServiceError> {
    service().dispatch(Request::EditUndo(UndoRequest {
        project: path.into(),
        base_revision: export(path).revision,
        session_id: Uuid::new_v4(),
        idempotency_key: Uuid::new_v4().to_string(),
        event_id: event,
    }))
}

#[test]
fn sequence_markers_set_move_edit_remove_and_roundtrip_through_query() {
    let s = sequence(vec![clip(t(0, 1), t(4, 1))]);
    let sid = s.id;
    let (_dir, path) = setup(project(s));
    let chapter = marker(t(1, 1), MarkerColor::Green, Some("chapter"));
    let set = apply(
        &path,
        vec![TimelineCommand::MarkerSet {
            sequence: sid,
            clip: None,
            marker: chapter.clone(),
        }],
    );
    let result = query(&path, sid);
    assert_eq!(result.sequence.markers, vec![chapter.clone()]);
    // marker_set with the same id updates color and comment in place.
    let recolored = Marker {
        color: MarkerColor::Orange,
        comment: Some("renamed".into()),
        ..chapter.clone()
    };
    let _recolor = apply(
        &path,
        vec![TimelineCommand::MarkerSet {
            sequence: sid,
            clip: None,
            marker: recolored.clone(),
        }],
    );
    assert_eq!(query(&path, sid).sequence.markers, vec![recolored.clone()]);
    let moved = apply(
        &path,
        vec![TimelineCommand::MarkerMove {
            sequence: sid,
            clip: None,
            marker: chapter.id,
            time: t(2, 1),
        }],
    );
    assert_eq!(query(&path, sid).sequence.markers[0].time, t(2, 1));
    // Markers persist across save/reload: export reads the stored document.
    let exported = export(&path);
    let DocumentObject::Known(s) = &exported.document.sequences[0] else {
        panic!()
    };
    assert_eq!(s.markers.len(), 1);
    // Selective undo rejects non-LIFO targets: the set event overlaps the keys
    // the later recolor and move already touched.
    let conflict = undo(&path, set.id).unwrap_err();
    assert_eq!(conflict.code, "UNDO_CONFLICT");
    // The newest event can be undone; the marker returns to the recolored
    // pre-move state.
    let ResultData::Edit(moved_undo) = undo(&path, moved.id).unwrap() else {
        panic!()
    };
    assert_eq!(query(&path, sid).sequence.markers[0].time, t(1, 1));
    assert_eq!(
        query(&path, sid).sequence.markers[0].color,
        MarkerColor::Orange
    );
    // Undoing the undo event redoes the move.
    undo(&path, moved_undo.id).unwrap();
    assert_eq!(query(&path, sid).sequence.markers[0].time, t(2, 1));
    // Undo events keep the same changed keys, so the earlier events on the
    // sequence stay conflicted; a fresh document proves the set inverse
    // removes the marker and redo restores it.
    let s2 = sequence(vec![clip(t(0, 1), t(4, 1))]);
    let sid2 = s2.id;
    let (_dir2, path2) = setup(project(s2));
    let set2 = apply(
        &path2,
        vec![TimelineCommand::MarkerSet {
            sequence: sid2,
            clip: None,
            marker: chapter.clone(),
        }],
    );
    let ResultData::Edit(set_undo) = undo(&path2, set2.id).unwrap() else {
        panic!()
    };
    assert!(query(&path2, sid2).sequence.markers.is_empty());
    undo(&path2, set_undo.id).unwrap();
    assert_eq!(query(&path2, sid2).sequence.markers, vec![chapter.clone()]);
    // Removal targets one id; a missing id is a typed miss.
    apply(
        &path,
        vec![TimelineCommand::MarkerRemove {
            sequence: sid,
            clip: None,
            marker: chapter.id,
        }],
    );
    assert!(query(&path, sid).sequence.markers.is_empty());
    reject(
        &path,
        vec![TimelineCommand::MarkerRemove {
            sequence: sid,
            clip: None,
            marker: MarkerId::new(),
        }],
        "SOURCE_MISSING",
    );
}

#[test]
fn marker_times_are_bounded_and_moves_validate_the_same_extent() {
    let s = sequence(vec![clip(t(0, 1), t(4, 1))]);
    let sid = s.id;
    let (_dir, path) = setup(project(s));
    // Beyond the content end and negative sequence times are out of extent.
    for time in [t(5, 1), t(-1, 1)] {
        reject(
            &path,
            vec![TimelineCommand::MarkerSet {
                sequence: sid,
                clip: None,
                marker: marker(time, MarkerColor::Red, None),
            }],
            "INVALID_CLIP",
        );
    }
    let id = MarkerId::new();
    apply(
        &path,
        vec![TimelineCommand::MarkerSet {
            sequence: sid,
            clip: None,
            marker: Marker {
                id,
                time: t(2, 1),
                color: MarkerColor::Blue,
                comment: None,
            },
        }],
    );
    // Moving a marker past the extent fails atomically.
    reject(
        &path,
        vec![TimelineCommand::MarkerMove {
            sequence: sid,
            clip: None,
            marker: id,
            time: t(9, 1),
        }],
        "INVALID_CLIP",
    );
    assert_eq!(query(&path, sid).sequence.markers[0].time, t(2, 1));
}

#[test]
fn clip_markers_stay_inside_the_placement_range_and_ride_its_edits() {
    let clip = clip(t(1, 1), t(3, 1));
    let clip_id = clip.id;
    let s = sequence(vec![clip]);
    let sid = s.id;
    let (_dir, path) = setup(project(s));
    let m = marker(t(2, 1), MarkerColor::Purple, None);
    apply(
        &path,
        vec![TimelineCommand::MarkerSet {
            sequence: sid,
            clip: Some(clip_id),
            marker: m.clone(),
        }],
    );
    let result = query(&path, sid);
    assert_eq!(result.clips[0].clip.markers, vec![m.clone()]);
    // Outside the clip's range is typed and atomic.
    reject(
        &path,
        vec![TimelineCommand::MarkerSet {
            sequence: sid,
            clip: Some(clip_id),
            marker: marker(t(4, 1), MarkerColor::Red, None),
        }],
        "INVALID_CLIP",
    );
    reject(
        &path,
        vec![TimelineCommand::MarkerMove {
            sequence: sid,
            clip: Some(clip_id),
            marker: m.id,
            time: t(0, 1),
        }],
        "INVALID_CLIP",
    );
    reject(
        &path,
        vec![TimelineCommand::MarkerSet {
            sequence: sid,
            clip: Some(ClipId::new()),
            marker: marker(t(1, 1), MarkerColor::Red, None),
        }],
        "SOURCE_MISSING",
    );
    // Clip markers follow the placement when the clip moves.
    apply(
        &path,
        vec![TimelineCommand::ClipMove {
            sequence: sid,
            clip: clip_id,
            delta: t(1, 1),
            linked: false,
        }],
    );
    let moved = query(&path, sid).clips[0].clip.clone();
    assert_eq!(moved.timeline_range, range(t(2, 1), t(4, 1)));
    assert_eq!(moved.markers[0].time, t(3, 1));
    // Trimming past a marker drops it with the content.
    apply(
        &path,
        vec![TimelineCommand::ClipTrim {
            sequence: sid,
            clip: clip_id,
            range: range(t(2, 1), t(3, 1)),
        }],
    );
    assert!(query(&path, sid).clips[0].clip.markers.is_empty());
}

#[test]
fn work_area_models_in_out_and_rejects_empty_or_out_of_extent_ranges() {
    let s = sequence(vec![clip(t(0, 1), t(6, 1))]);
    let sid = s.id;
    let (_dir, path) = setup(project(s));
    assert_eq!(query(&path, sid).sequence.work_area, None);
    let event = apply(
        &path,
        vec![TimelineCommand::WorkAreaSet {
            sequence: sid,
            work_area: Some(range(t(1, 1), t(4, 1))),
        }],
    );
    assert_eq!(
        query(&path, sid).sequence.work_area,
        Some(range(t(1, 1), t(4, 1)))
    );
    // Undo clears the work area again.
    undo(&path, event.id).unwrap();
    assert_eq!(query(&path, sid).sequence.work_area, None);
    apply(
        &path,
        vec![TimelineCommand::WorkAreaSet {
            sequence: sid,
            work_area: Some(range(t(1, 1), t(4, 1))),
        }],
    );
    // Clearing is explicit and stays cleared through reload.
    apply(
        &path,
        vec![TimelineCommand::WorkAreaSet {
            sequence: sid,
            work_area: None,
        }],
    );
    assert_eq!(query(&path, sid).sequence.work_area, None);
    for area in [range(t(2, 1), t(2, 1)), range(t(1, 1), t(7, 1))] {
        reject(
            &path,
            vec![TimelineCommand::WorkAreaSet {
                sequence: sid,
                work_area: Some(area),
            }],
            "INVALID_CLIP",
        );
    }
}

#[test]
fn work_area_maps_explicitly_into_the_render_job_range() {
    let s = sequence(vec![clip(t(0, 1), t(6, 1))]);
    let sid = s.id;
    let (dir, path) = setup(project(s));
    apply(
        &path,
        vec![TimelineCommand::WorkAreaSet {
            sequence: sid,
            work_area: Some(range(t(1, 1), t(3, 1))),
        }],
    );
    // The service never reads work_area: the caller copies it into the render
    // request, and the submitted job stores exactly that range.
    let work_area = query(&path, sid).sequence.work_area.unwrap();
    let job_state = tempfile::tempdir().unwrap();
    let engine = Service::new(BackendSelection::CpuReference)
        .with_job_config(kronello_jobs::JobConfig::at(job_state.path()))
        .with_worker_executable(PathBuf::from("/usr/bin/false"));
    let ResultData::Job(record) = engine
        .dispatch(Request::RenderSubmit(RenderSubmitRequest {
            expected_revision: None,
            render: SequenceRenderRequest {
                input: RenderInput {
                    project: path.clone(),
                    composition: None,
                    target: Some(RenderTarget::Sequence { sequence: sid }),
                    region: OutputRegion {
                        origin: [0.0; 2],
                        extent: [16.0; 2],
                        pixels: [16; 2],
                    },
                    profile: Default::default(),
                    fonts: vec![],
                    media_proxies: kronello_render::MediaProxyMode::Off,
                },
                range: work_area,
                frame_rate: FrameRate::new(24, 1).unwrap(),
                output_directory: dir.path().join("work-area-frames"),
            },
            output: JobOutput::ImageSequence,
            required_features: vec![],
        }))
        .unwrap()
    else {
        panic!()
    };
    // 2 seconds at 24 fps -> 48 frames scheduled from the explicit range.
    assert_eq!(record.total_frames, 48);
    let store =
        kronello_jobs::JobStore::open(kronello_jobs::JobConfig::at(job_state.path())).unwrap();
    let input: serde_json::Value = serde_json::from_slice(
        &std::fs::read(store.directory(&record.id).unwrap().join("input.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        serde_json::from_value::<TimeRange>(input["request"]["render"]["range"].clone()).unwrap(),
        work_area
    );
    // The fixed snapshot keeps the authored work_area for later inspection,
    // but nothing in the job derived it implicitly.
    let DocumentObject::Known(snapshot_sequence) =
        &serde_json::from_value::<Project>(input["snapshot"]["project"].clone())
            .unwrap()
            .sequences[0]
    else {
        panic!()
    };
    assert_eq!(snapshot_sequence.work_area, Some(work_area));
}

#[test]
fn markers_and_work_area_survive_serde_roundtrip_and_old_documents_default() {
    let mut s = sequence(vec![{
        let mut c = clip(t(0, 1), t(4, 1));
        c.markers = vec![marker(t(1, 1), MarkerColor::Cyan, Some("clip"))];
        c
    }]);
    s.markers = vec![marker(t(2, 1), MarkerColor::White, None)];
    s.work_area = Some(range(t(1, 1), t(3, 1)));
    let json = serde_json::to_string(&s).unwrap();
    let decoded: Sequence = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, s);
    // Documents written before NLE-004 omit the fields entirely.
    let mut legacy = serde_json::to_value(&s).unwrap();
    for field in ["markers", "work_area"] {
        legacy.as_object_mut().unwrap().remove(field);
    }
    let clip_json = serde_json::to_value(&legacy["tracks"][0]["clips"][0]).unwrap();
    let mut clip_legacy = clip_json.clone();
    clip_legacy.as_object_mut().unwrap().remove("markers");
    let decoded_clip: Clip = serde_json::from_value(clip_legacy).unwrap();
    assert!(decoded_clip.markers.is_empty());
    let decoded_legacy: Sequence = serde_json::from_value(legacy).unwrap();
    assert!(decoded_legacy.markers.is_empty());
    assert_eq!(decoded_legacy.work_area, None);
}

#[test]
fn trimming_a_clip_preserves_markers_still_inside_the_range() {
    let mut clip = clip(t(0, 1), t(4, 1));
    let keep = marker(t(2, 1), MarkerColor::Yellow, None);
    let drop = marker(t(3, 1), MarkerColor::Yellow, None);
    clip.markers = vec![keep.clone(), drop.clone()];
    let clip_id = clip.id;
    let s = sequence(vec![clip]);
    let sid = s.id;
    let (_dir, path) = setup(project(s));
    apply(
        &path,
        vec![TimelineCommand::ClipTrim {
            sequence: sid,
            clip: clip_id,
            range: range(t(0, 1), t(3, 1)),
        }],
    );
    let markers = &query(&path, sid).clips[0].clip.markers;
    assert_eq!(*markers, vec![keep]);
}
