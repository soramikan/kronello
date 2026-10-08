//! NLE-003: slip / slide / roll / delete / ripple delete / insert / overwrite
//! share the typed TimelineCommand path with plan / apply / selective undo.
use kronello_model::*;
use kronello_service::*;
use kronello_time::{Duration, FrameRate, Rational, SampleRate, Time, TimeMap, TimeRange};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use uuid::Uuid;

fn t(n: i64, d: i64) -> Time {
    Time::new(n, d).unwrap()
}
fn range(a: Time, b: Time) -> TimeRange {
    TimeRange::new(a, b).unwrap()
}
/// Bounded composition source: slip / slide / roll exercise real remainder
/// limits against `duration`.
fn composition_source(p: &mut Project, seconds: i64) -> CompositionId {
    let id = CompositionId::new();
    p.compositions.push(DocumentObject::Known(Composition {
        id,
        duration: Duration::new(t(seconds, 1)).unwrap(),
        design_extent: DesignExtent::new(16.0, 16.0).unwrap(),
        edit_rate: FrameRate::new(24, 1).unwrap(),
        root_nodes: vec![],
        nodes: vec![],
        properties: vec![],
    }));
    id
}
fn comp_clip(composition: CompositionId, source_in: Time, a: Time, b: Time) -> Clip {
    Clip {
        id: ClipId::new(),
        source_ref: SourceRef::Composition { composition },
        timeline_range: range(a, b),
        source_in,
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
/// Unbounded solid generator source for pure placement semantics.
fn gen_clip(a: Time, b: Time) -> Clip {
    Clip {
        id: ClipId::new(),
        source_ref: SourceRef::Generator {
            generator: SOLID_GENERATOR_ID.into(),
            version: 1,
            color: Color::from_srgb8([40, 80, 160], None),
        },
        timeline_range: range(a, b),
        source_in: Time::ONE,
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
fn audio_clip(a: Time, b: Time) -> Clip {
    let mut clip = gen_clip(a, b);
    clip.source_ref = SourceRef::Generator {
        generator: "kronello.audio.tone440".into(),
        version: 1,
        color: Color::from_srgb8([0; 3], None),
    };
    clip.audio_retime = AudioRetimePolicy::ResampleV1;
    clip
}
fn sequence(tracks: Vec<(TrackKind, Vec<Clip>)>) -> Sequence {
    Sequence {
        id: SequenceId::new(),
        extent: DesignExtent::new(16.0, 16.0).unwrap(),
        frame_rate: FrameRate::new(24, 1).unwrap(),
        audio_rate: SampleRate::HZ_48000,
        working_space: ColorSpace::LinearRec709,
        tracks: tracks
            .into_iter()
            .map(|(kind, clips)| Track {
                state: None,
                id: TrackId::new(),
                kind,
                clips,
            })
            .collect(),
        transitions: vec![],
        markers: vec![],
        work_area: None,
        targets: None,
    }
}
fn setup(p: Project) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nle3.kronello");
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
fn service() -> Service<'static> {
    Service::new(BackendSelection::CpuReference)
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
fn stored(path: &Path, sequence: SequenceId) -> Sequence {
    let document = export(path).document;
    let DocumentObject::Known(s) = document
        .sequences
        .iter()
        .find(|s| matches!(s, DocumentObject::Known(s) if s.id == sequence))
        .expect("sequence")
    else {
        panic!()
    };
    s.clone()
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
/// A rejected plan leaves the document and revision byte-identical.
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
fn clips(s: &Sequence, track: usize) -> &[Clip] {
    &s.tracks[track].clips
}
fn ranges(s: &Sequence, track: usize) -> Vec<(Time, Time)> {
    s.tracks[track]
        .clips
        .iter()
        .map(|c| (c.timeline_range.start(), c.timeline_range.end()))
        .collect()
}

#[test]
fn slip_moves_the_source_window_inside_an_unchanged_placement() {
    let mut p = Project::default();
    let source = composition_source(&mut p, 6);
    let clip = comp_clip(source, t(1, 1), t(1, 1), t(3, 1));
    let clip_id = clip.id;
    let s = sequence(vec![(TrackKind::Video, vec![clip])]);
    let sid = s.id;
    p.sequences.push(DocumentObject::Known(s));
    let (_dir, path) = setup(p);
    let before = stored(&path, sid);
    // +1 shifts the displayed source window [1,3) -> [3,5).
    let event = apply(
        &path,
        vec![TimelineCommand::ClipSlip {
            sequence: sid,
            clip: clip_id,
            delta: t(2, 1),
            linked: false,
        }],
    );
    let after = stored(&path, sid);
    let c = &clips(&after, 0)[0];
    assert_eq!(c.timeline_range, range(t(1, 1), t(3, 1)));
    assert_eq!(c.source_in, t(3, 1));
    assert_eq!(c.local_time(t(1, 1)).unwrap(), t(3, 1));
    assert_eq!(c.local_time(t(3, 1)).unwrap(), t(5, 1));
    let ResultData::Edit(undo_event) = undo(&path, event.id).unwrap() else {
        panic!()
    };
    assert_eq!(stored(&path, sid), before);
    // Redo undoes the undo event and restores the slipped window.
    undo(&path, undo_event.id).unwrap();
    assert_eq!(clips(&stored(&path, sid), 0)[0].source_in, t(3, 1));
}

#[test]
fn slip_rejects_source_bounds_and_leaves_the_document_unchanged() {
    let mut p = Project::default();
    let source = composition_source(&mut p, 6);
    let clip = comp_clip(source, t(1, 1), t(1, 1), t(3, 1));
    let clip_id = clip.id;
    let s = sequence(vec![(TrackKind::Video, vec![clip])]);
    let sid = s.id;
    p.sequences.push(DocumentObject::Known(s));
    let (_dir, path) = setup(p);
    // Negative source_in.
    reject(
        &path,
        vec![TimelineCommand::ClipSlip {
            sequence: sid,
            clip: clip_id,
            delta: t(-2, 1),
            linked: false,
        }],
        "INVALID_CLIP",
    );
    // Window end 1+4+2 = 7 exceeds the 6-second source.
    reject(
        &path,
        vec![TimelineCommand::ClipSlip {
            sequence: sid,
            clip: clip_id,
            delta: t(4, 1),
            linked: false,
        }],
        "INVALID_CLIP",
    );
    reject(
        &path,
        vec![TimelineCommand::ClipSlip {
            sequence: sid,
            clip: ClipId::new(),
            delta: t(1, 1),
            linked: false,
        }],
        "SOURCE_MISSING",
    );
}

#[test]
fn slip_keeps_reverse_sampling_and_piecewise_maps_consistent() {
    let mut p = Project::default();
    let source = composition_source(&mut p, 8);
    let mut clip = comp_clip(source, t(5, 1), t(0, 1), t(2, 1));
    clip.reverse_sampling = Some(ReverseSampling::ReverseGridV1);
    clip.audio_retime = AudioRetimePolicy::ReverseResampleV1;
    let clip_id = clip.id;
    let s = sequence(vec![(TrackKind::Video, vec![clip])]);
    let sid = s.id;
    p.sequences.push(DocumentObject::Known(s));
    let (_dir, path) = setup(p);
    apply(
        &path,
        vec![TimelineCommand::ClipSlip {
            sequence: sid,
            clip: clip_id,
            delta: t(-1, 1),
            linked: false,
        }],
    );
    let s = stored(&path, sid);
    let c = &clips(&s, 0)[0];
    assert_eq!(c.source_in, t(4, 1));
    assert_eq!(c.reverse_sampling, Some(ReverseSampling::ReverseGridV1));
    assert_eq!(c.local_time(t(0, 1)).unwrap(), t(4, 1));
    assert_eq!(c.local_time(t(2, 1)).unwrap(), t(2, 1));
    // Slipping past the source head is typed and atomic.
    reject(
        &path,
        vec![TimelineCommand::ClipSlip {
            sequence: sid,
            clip: clip_id,
            delta: t(5, 1),
            linked: false,
        }],
        "INVALID_CLIP",
    );
}

#[test]
fn slip_expands_the_link_component_or_rejects_a_partial_edit() {
    let mut p = Project::default();
    let mut video = gen_clip(t(0, 1), t(2, 1));
    let mut audio = audio_clip(t(0, 1), t(2, 1));
    video.links = vec![audio.id];
    audio.links = vec![video.id];
    let video_id = video.id;
    let audio_id = audio.id;
    let s = sequence(vec![
        (TrackKind::Video, vec![video]),
        (TrackKind::Audio, vec![audio]),
    ]);
    let sid = s.id;
    p.sequences.push(DocumentObject::Known(s));
    let (_dir, path) = setup(p);
    reject(
        &path,
        vec![TimelineCommand::ClipSlip {
            sequence: sid,
            clip: video_id,
            delta: t(1, 1),
            linked: false,
        }],
        "LINKED_EDIT_REQUIRED",
    );
    apply(
        &path,
        vec![TimelineCommand::ClipSlip {
            sequence: sid,
            clip: video_id,
            delta: t(1, 1),
            linked: true,
        }],
    );
    let s = stored(&path, sid);
    assert_eq!(clips(&s, 0)[0].source_in, t(2, 1));
    assert_eq!(clips(&s, 1)[0].source_in, t(2, 1));
    assert_eq!(clips(&s, 1)[0].id, audio_id);
}

#[test]
fn slide_moves_the_clip_while_neighbours_absorb_the_delta() {
    let mut p = Project::default();
    let a = gen_clip(t(0, 1), t(2, 1));
    let b = gen_clip(t(2, 1), t(4, 1));
    let c = gen_clip(t(4, 1), t(6, 1));
    let (a_id, b_id, c_id) = (a.id, b.id, c.id);
    let s = sequence(vec![(TrackKind::Video, vec![a, b, c])]);
    let sid = s.id;
    p.sequences.push(DocumentObject::Known(s));
    let (_dir, path) = setup(p);
    apply(
        &path,
        vec![TimelineCommand::ClipSlide {
            sequence: sid,
            clip: b_id,
            delta: t(1, 1),
            linked: false,
        }],
    );
    let s = stored(&path, sid);
    assert_eq!(
        ranges(&s, 0),
        vec![(t(0, 1), t(3, 1)), (t(3, 1), t(5, 1)), (t(5, 1), t(6, 1))]
    );
    let [a, b, c] = clips(&s, 0) else { panic!() };
    assert_eq!((a.id, b.id, c.id), (a_id, b_id, c_id));
    // The absorbed neighbours keep their source continuity: A's tail shows
    // the following source content and C's new head is the trimmed source.
    assert_eq!(a.local_time(t(2, 1)).unwrap(), t(3, 1));
    assert_eq!(c.source_in, t(2, 1));
}

#[test]
fn slide_rejects_missing_adjacency_and_exhausted_source_handles() {
    let mut p = Project::default();
    let source = composition_source(&mut p, 4);
    // B's neighbours are composition clips with no handle left: A already
    // displays the last source second and C starts at the source origin.
    let a = comp_clip(source, t(3, 1), t(0, 1), t(1, 1));
    let b = gen_clip(t(1, 1), t(2, 1));
    let c = comp_clip(source, t(0, 1), t(2, 1), t(3, 1));
    let b_id = b.id;
    let s = sequence(vec![(TrackKind::Video, vec![a, b, c])]);
    let sid = s.id;
    p.sequences.push(DocumentObject::Known(s));
    let (_dir, path) = setup(p);
    // Sliding right needs tail handle on A (source ends at 3): +1 exhausts it.
    reject(
        &path,
        vec![TimelineCommand::ClipSlide {
            sequence: sid,
            clip: b_id,
            delta: t(1, 1),
            linked: false,
        }],
        "INVALID_CLIP",
    );
    // Sliding left needs head handle on C (source_in is 0 already).
    reject(
        &path,
        vec![TimelineCommand::ClipSlide {
            sequence: sid,
            clip: b_id,
            delta: t(-1, 2),
            linked: false,
        }],
        "INVALID_CLIP",
    );
    // A clip at the track head has no leading neighbour at all.
    let mut p = Project::default();
    let first = gen_clip(t(0, 1), t(2, 1));
    let first_id = first.id;
    let s = sequence(vec![(TrackKind::Video, vec![first])]);
    let sid2 = s.id;
    p.sequences.push(DocumentObject::Known(s));
    let (_dir, path) = setup(p);
    reject(
        &path,
        vec![TimelineCommand::ClipSlide {
            sequence: sid2,
            clip: first_id,
            delta: t(1, 1),
            linked: false,
        }],
        "INVALID_CLIP",
    );
}

#[test]
fn slide_moves_a_link_component_and_absorbs_on_both_tracks() {
    let mut p = Project::default();
    let va = gen_clip(t(0, 1), t(2, 1));
    let mut vb = gen_clip(t(2, 1), t(4, 1));
    let vc = gen_clip(t(4, 1), t(6, 1));
    let aa = audio_clip(t(0, 1), t(2, 1));
    let mut ab = audio_clip(t(2, 1), t(4, 1));
    let ac = audio_clip(t(4, 1), t(6, 1));
    vb.links = vec![ab.id];
    ab.links = vec![vb.id];
    let vb_id = vb.id;
    let s = sequence(vec![
        (TrackKind::Video, vec![va, vb, vc]),
        (TrackKind::Audio, vec![aa, ab, ac]),
    ]);
    let sid = s.id;
    p.sequences.push(DocumentObject::Known(s));
    let (_dir, path) = setup(p);
    apply(
        &path,
        vec![TimelineCommand::ClipSlide {
            sequence: sid,
            clip: vb_id,
            delta: t(1, 1),
            linked: true,
        }],
    );
    let s = stored(&path, sid);
    for track in 0..2 {
        assert_eq!(
            ranges(&s, track),
            vec![(t(0, 1), t(3, 1)), (t(3, 1), t(5, 1)), (t(5, 1), t(6, 1))]
        );
    }
}

#[test]
fn roll_moves_the_shared_edit_point_through_both_clips() {
    let mut p = Project::default();
    let source = composition_source(&mut p, 6);
    let left = comp_clip(source, t(1, 1), t(0, 1), t(2, 1));
    let right = comp_clip(source, t(2, 1), t(2, 1), t(4, 1));
    let (left_id, right_id) = (left.id, right.id);
    let s = sequence(vec![(TrackKind::Video, vec![left, right])]);
    let sid = s.id;
    p.sequences.push(DocumentObject::Known(s));
    let (_dir, path) = setup(p);
    apply(
        &path,
        vec![TimelineCommand::ClipRoll {
            sequence: sid,
            clip: left_id,
            delta: t(1, 1),
            linked: false,
        }],
    );
    let s = stored(&path, sid);
    assert_eq!(ranges(&s, 0), vec![(t(0, 1), t(3, 1)), (t(3, 1), t(4, 1))]);
    let [left, right] = clips(&s, 0) else {
        panic!()
    };
    assert_eq!(left.id, left_id);
    assert_eq!(right.id, right_id);
    assert_eq!(right.source_in, t(3, 1), "incoming head trims forward");
    // Rolling back re-extends the incoming head within the same command type.
    apply(
        &path,
        vec![TimelineCommand::ClipRoll {
            sequence: sid,
            clip: left_id,
            delta: t(-1, 1),
            linked: false,
        }],
    );
    let s = stored(&path, sid);
    assert_eq!(ranges(&s, 0), vec![(t(0, 1), t(2, 1)), (t(2, 1), t(4, 1))]);
}

#[test]
fn roll_rejects_atomically_when_either_side_cannot_absorb() {
    let mut p = Project::default();
    let source = composition_source(&mut p, 4);
    // Left clip has no tail handle: local end is already the source end.
    let left = comp_clip(source, t(0, 1), t(0, 1), t(4, 1));
    let right = comp_clip(source, t(1, 1), t(4, 1), t(5, 1));
    let left_id = left.id;
    let s = sequence(vec![(TrackKind::Video, vec![left, right])]);
    let sid = s.id;
    p.sequences.push(DocumentObject::Known(s));
    let (_dir, path) = setup(p);
    reject(
        &path,
        vec![TimelineCommand::ClipRoll {
            sequence: sid,
            clip: left_id,
            delta: t(1, 1),
            linked: false,
        }],
        "INVALID_CLIP",
    );
    // The last clip has no adjacent successor to roll against.
    let mut p = Project::default();
    let source = composition_source(&mut p, 8);
    let only = comp_clip(source, t(0, 1), t(0, 1), t(2, 1));
    let only_id = only.id;
    let s = sequence(vec![(TrackKind::Video, vec![only])]);
    let sid = s.id;
    p.sequences.push(DocumentObject::Known(s));
    let (_dir, path) = setup(p);
    reject(
        &path,
        vec![TimelineCommand::ClipRoll {
            sequence: sid,
            clip: only_id,
            delta: t(1, 1),
            linked: false,
        }],
        "INVALID_CLIP",
    );
}

#[test]
fn roll_rolls_linked_edit_points_together() {
    let mut p = Project::default();
    let v1 = gen_clip(t(0, 1), t(2, 1));
    let v2 = gen_clip(t(2, 1), t(4, 1));
    let mut a1 = audio_clip(t(0, 1), t(2, 1));
    let mut a2 = audio_clip(t(2, 1), t(4, 1));
    let mut v1 = v1;
    let mut v2 = v2;
    v1.links = vec![a1.id];
    a1.links = vec![v1.id];
    v2.links = vec![a2.id];
    a2.links = vec![v2.id];
    let v1_id = v1.id;
    let s = sequence(vec![
        (TrackKind::Video, vec![v1, v2]),
        (TrackKind::Audio, vec![a1, a2]),
    ]);
    let sid = s.id;
    p.sequences.push(DocumentObject::Known(s));
    let (_dir, path) = setup(p);
    apply(
        &path,
        vec![TimelineCommand::ClipRoll {
            sequence: sid,
            clip: v1_id,
            delta: t(1, 1),
            linked: true,
        }],
    );
    let s = stored(&path, sid);
    for track in 0..2 {
        assert_eq!(
            ranges(&s, track),
            vec![(t(0, 1), t(3, 1)), (t(3, 1), t(4, 1))]
        );
    }
}

#[test]
fn delete_leaves_a_gap_and_expands_the_link_component() {
    let mut p = Project::default();
    let a = gen_clip(t(0, 1), t(2, 1));
    let b = gen_clip(t(2, 1), t(4, 1));
    let c = gen_clip(t(4, 1), t(6, 1));
    let b_id = b.id;
    let s = sequence(vec![(TrackKind::Video, vec![a, b, c])]);
    let sid = s.id;
    p.sequences.push(DocumentObject::Known(s));
    let (_dir, path) = setup(p);
    let event = apply(
        &path,
        vec![TimelineCommand::ClipDelete {
            sequence: sid,
            clip: b_id,
            linked: false,
        }],
    );
    let s = stored(&path, sid);
    // Ordinary delete keeps the gap: [0,2) then [4,6).
    assert_eq!(ranges(&s, 0), vec![(t(0, 1), t(2, 1)), (t(4, 1), t(6, 1))]);
    undo(&path, event.id).unwrap();
    assert_eq!(
        ranges(&stored(&path, sid), 0),
        vec![(t(0, 1), t(2, 1)), (t(2, 1), t(4, 1)), (t(4, 1), t(6, 1))]
    );
    // Linked members delete together or the edit is rejected outright.
    let mut p = Project::default();
    let mut v = gen_clip(t(0, 1), t(2, 1));
    let mut a = audio_clip(t(0, 1), t(2, 1));
    v.links = vec![a.id];
    a.links = vec![v.id];
    let v_id = v.id;
    let s = sequence(vec![
        (TrackKind::Video, vec![v]),
        (TrackKind::Audio, vec![a]),
    ]);
    let sid = s.id;
    p.sequences.push(DocumentObject::Known(s));
    let (_dir, path) = setup(p);
    reject(
        &path,
        vec![TimelineCommand::ClipDelete {
            sequence: sid,
            clip: v_id,
            linked: false,
        }],
        "LINKED_EDIT_REQUIRED",
    );
    apply(
        &path,
        vec![TimelineCommand::ClipDelete {
            sequence: sid,
            clip: v_id,
            linked: true,
        }],
    );
    let s = stored(&path, sid);
    assert!(s.tracks.iter().all(|t| t.clips.is_empty()));
}

#[test]
fn ripple_delete_removes_the_range_and_closes_the_gap() {
    let mut p = Project::default();
    let a = gen_clip(t(0, 1), t(2, 1));
    let b = gen_clip(t(2, 1), t(4, 1));
    let c = gen_clip(t(4, 1), t(6, 1));
    let (a_id, b_id) = (a.id, b.id);
    let s = sequence(vec![(TrackKind::Video, vec![a, b, c])]);
    let sid = s.id;
    let track = s.tracks[0].id;
    p.sequences.push(DocumentObject::Known(s));
    let (_dir, path) = setup(p);
    // [1,3) cuts A's tail, removes the covered head of B, and pulls C forward.
    let event = apply(
        &path,
        vec![TimelineCommand::RippleDelete {
            sequence: sid,
            tracks: vec![track],
            range: range(t(1, 1), t(3, 1)),
            linked: false,
        }],
    );
    let s = stored(&path, sid);
    assert_eq!(
        ranges(&s, 0),
        vec![(t(0, 1), t(1, 1)), (t(1, 1), t(2, 1)), (t(2, 1), t(4, 1))]
    );
    assert_eq!(clips(&s, 0)[0].id, a_id);
    assert_eq!(
        clips(&s, 0)[1].id,
        b_id,
        "the surviving tail keeps identity"
    );
    undo(&path, event.id).unwrap();
    assert_eq!(
        ranges(&stored(&path, sid), 0),
        vec![(t(0, 1), t(2, 1)), (t(2, 1), t(4, 1)), (t(4, 1), t(6, 1))]
    );
    // A whole middle clip deletes and the gap closes completely.
    apply(
        &path,
        vec![TimelineCommand::RippleDelete {
            sequence: sid,
            tracks: vec![track],
            range: range(t(2, 1), t(4, 1)),
            linked: false,
        }],
    );
    assert_eq!(
        ranges(&stored(&path, sid), 0),
        vec![(t(0, 1), t(2, 1)), (t(2, 1), t(4, 1))]
    );
}

#[test]
fn ripple_delete_rejects_invalid_ranges_and_straddles() {
    let mut p = Project::default();
    let clip = gen_clip(t(0, 1), t(6, 1));
    let s = sequence(vec![(TrackKind::Video, vec![clip])]);
    let sid = s.id;
    let track = s.tracks[0].id;
    p.sequences.push(DocumentObject::Known(s));
    let (_dir, path) = setup(p);
    reject(
        &path,
        vec![TimelineCommand::RippleDelete {
            sequence: sid,
            tracks: vec![track],
            range: range(t(1, 1), t(2, 1)),
            linked: false,
        }],
        "INVALID_CLIP",
    );
    reject(
        &path,
        vec![TimelineCommand::RippleDelete {
            sequence: sid,
            tracks: vec![track],
            range: range(t(2, 1), t(2, 1)),
            linked: false,
        }],
        "INVALID_CLIP",
    );
    reject(
        &path,
        vec![TimelineCommand::RippleDelete {
            sequence: sid,
            tracks: vec![TrackId::new()],
            range: range(t(0, 1), t(1, 1)),
            linked: false,
        }],
        "INVALID_CLIP",
    );
}

#[test]
fn ripple_delete_pulls_linked_partners_from_unlisted_tracks() {
    let mut p = Project::default();
    let mut v = gen_clip(t(0, 1), t(4, 1));
    let mut a = audio_clip(t(0, 1), t(4, 1));
    v.links = vec![a.id];
    a.links = vec![v.id];
    let v_id = v.id;
    let s = sequence(vec![
        (TrackKind::Video, vec![v]),
        (TrackKind::Audio, vec![a]),
    ]);
    let sid = s.id;
    let video = s.tracks[0].id;
    p.sequences.push(DocumentObject::Known(s));
    let (_dir, path) = setup(p);
    reject(
        &path,
        vec![TimelineCommand::RippleDelete {
            sequence: sid,
            tracks: vec![video],
            range: range(t(2, 1), t(4, 1)),
            linked: false,
        }],
        "LINKED_EDIT_REQUIRED",
    );
    apply(
        &path,
        vec![TimelineCommand::RippleDelete {
            sequence: sid,
            tracks: vec![video],
            range: range(t(2, 1), t(4, 1)),
            linked: true,
        }],
    );
    let s = stored(&path, sid);
    // Both members trim to their own overlap with the range.
    assert_eq!(ranges(&s, 0), vec![(t(0, 1), t(2, 1))]);
    assert_eq!(ranges(&s, 1), vec![(t(0, 1), t(2, 1))]);
    assert_eq!(clips(&s, 0)[0].id, v_id);
}

#[test]
fn insert_opens_a_typed_gap_and_overwrite_replaces_in_place() {
    let mut p = Project::default();
    let a = gen_clip(t(0, 1), t(2, 1));
    let b = gen_clip(t(2, 1), t(4, 1));
    let a_id = a.id;
    let b_id = b.id;
    let s = sequence(vec![(TrackKind::Video, vec![a, b])]);
    let sid = s.id;
    let track = s.tracks[0].id;
    p.sequences.push(DocumentObject::Known(s));
    let (_dir, path) = setup(p);
    let inserted = gen_clip(t(2, 1), t(3, 1));
    apply(
        &path,
        vec![TimelineCommand::ClipInsert {
            sequence: sid,
            track,
            clip: Box::new(inserted.clone()),
            linked: false,
        }],
    );
    let s = stored(&path, sid);
    assert_eq!(
        ranges(&s, 0),
        vec![(t(0, 1), t(2, 1)), (t(2, 1), t(3, 1)), (t(3, 1), t(5, 1))]
    );
    assert_eq!(
        s.tracks[0]
            .clips
            .iter()
            .map(|c| c.id)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([a_id, inserted.id, b_id])
    );
    // Insertion into the middle of a clip fails instead of implicit splitting.
    reject(
        &path,
        vec![TimelineCommand::ClipInsert {
            sequence: sid,
            track,
            clip: Box::new(gen_clip(t(1, 1), t(2, 1))),
            linked: false,
        }],
        "INVALID_CLIP",
    );
    // Overwrite [4,6) covers the tail of the shifted B [3,5): B keeps [3,4).
    let overlay = gen_clip(t(4, 1), t(6, 1));
    apply(
        &path,
        vec![TimelineCommand::ClipOverwrite {
            sequence: sid,
            track,
            clip: Box::new(overlay.clone()),
        }],
    );
    let s = stored(&path, sid);
    assert_eq!(
        ranges(&s, 0),
        vec![
            (t(0, 1), t(2, 1)),
            (t(2, 1), t(3, 1)),
            (t(3, 1), t(4, 1)),
            (t(4, 1), t(6, 1))
        ]
    );
    let ids: BTreeSet<_> = s.tracks[0].clips.iter().map(|c| c.id).collect();
    assert!(ids.contains(&overlay.id));
    assert!(ids.contains(&b_id));
}

#[test]
fn overwrite_splits_straddling_clips_and_rejects_linked_targets() {
    let mut p = Project::default();
    let a = gen_clip(t(0, 1), t(4, 1));
    let b = gen_clip(t(4, 1), t(6, 1));
    let a_id = a.id;
    let b_id = b.id;
    let s = sequence(vec![(TrackKind::Video, vec![a, b])]);
    let sid = s.id;
    let track = s.tracks[0].id;
    p.sequences.push(DocumentObject::Known(s));
    let (_dir, path) = setup(p);
    // [2,5) splits A (keeps [0,2)) and B (keeps [5,6)).
    apply(
        &path,
        vec![TimelineCommand::ClipOverwrite {
            sequence: sid,
            track,
            clip: Box::new(gen_clip(t(2, 1), t(5, 1))),
        }],
    );
    let s = stored(&path, sid);
    assert_eq!(
        ranges(&s, 0),
        vec![(t(0, 1), t(2, 1)), (t(2, 1), t(5, 1)), (t(5, 1), t(6, 1))]
    );
    let ids: BTreeSet<_> = s.tracks[0].clips.iter().map(|c| c.id).collect();
    assert!(ids.contains(&a_id) && ids.contains(&b_id));
    // Overwrite cannot silently unlink: a covered linked clip is typed.
    let mut p = Project::default();
    let mut v = gen_clip(t(0, 1), t(2, 1));
    let mut a = audio_clip(t(0, 1), t(2, 1));
    v.links = vec![a.id];
    a.links = vec![v.id];
    let s = sequence(vec![
        (TrackKind::Video, vec![v]),
        (TrackKind::Audio, vec![a]),
    ]);
    let sid = s.id;
    let video = s.tracks[0].id;
    p.sequences.push(DocumentObject::Known(s));
    let (_dir, path) = setup(p);
    reject(
        &path,
        vec![TimelineCommand::ClipOverwrite {
            sequence: sid,
            track: video,
            clip: Box::new(gen_clip(t(0, 1), t(2, 1))),
        }],
        "LINKED_EDIT_REQUIRED",
    );
}

#[test]
fn plan_and_apply_report_the_same_typed_failure() {
    // edit.apply re-plans the batch, so both entry points reject a bounds
    // violation with the same code and no persisted change.
    let mut p = Project::default();
    let b = gen_clip(t(0, 1), t(2, 1));
    let b_id = b.id;
    let s = sequence(vec![(TrackKind::Video, vec![b])]);
    let sid = s.id;
    p.sequences.push(DocumentObject::Known(s));
    let (_dir, path) = setup(p);
    let bad = || TimelineCommand::ClipSlip {
        sequence: sid,
        clip: b_id,
        delta: t(-5, 1),
        linked: false,
    };
    let e = plan(&path, vec![bad()]).unwrap_err();
    assert_eq!(e.code, "INVALID_CLIP");
    let before = export(&path);
    let e = service()
        .dispatch(Request::EditApply(EditApplyRequest {
            project: path.clone(),
            base_revision: before.revision.clone(),
            plan_hash: "unused".into(),
            idempotency_key: Uuid::new_v4().to_string(),
            session_id: Uuid::new_v4(),
            commands: vec![EditCommand::Timeline(Box::new(bad()))],
        }))
        .unwrap_err();
    assert_eq!(e.code, "INVALID_CLIP");
    let after = export(&path);
    assert_eq!(after.document, before.document);
    assert_eq!(after.revision, before.revision);
}
