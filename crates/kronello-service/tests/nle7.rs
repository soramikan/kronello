//! NLE-007 multicam group creation/angle switching (ADR-0127) and GUI-011
//! shared three-point `edit.insert`/`edit.overwrite` (ADR-0128) through the
//! shared command path, including typed failures and selective undo.
use kronello_model::*;
use kronello_service::*;
use kronello_time::{FrameRate, Rational, SampleRate, Time, TimeMap, TimeRange};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use uuid::Uuid;

fn t(n: i64, d: i64) -> Time {
    Time::new(n, d).unwrap()
}
fn range(a: Time, b: Time) -> TimeRange {
    TimeRange::new(a, b).unwrap()
}
fn video_asset(start: Option<Rational>, duration: Option<Rational>) -> Asset {
    Asset {
        id: AssetId::new(),
        kind: AssetKind::Video,
        content_hash: "a".repeat(64),
        locator: AssetLocator {
            relative: Some("cam.mov".into()),
            absolute: None,
        },
        streams: vec![
            StreamMetadata {
                index: 0,
                codec: "prores".into(),
                time_base: t(1, 24),
                duration,
                start_time: start,
                width: Some(1920),
                height: Some(1080),
                pixel_format: None,
                color_primaries: None,
                color_transfer: None,
                color_matrix: None,
                color_range: None,
            },
            StreamMetadata {
                index: 1,
                codec: "pcm_s16le".into(),
                time_base: t(1, 48000),
                duration,
                start_time: start,
                width: None,
                height: None,
                pixel_format: None,
                color_primaries: None,
                color_transfer: None,
                color_matrix: None,
                color_range: None,
            },
        ],
    }
}
fn audio_asset() -> Asset {
    Asset {
        id: AssetId::new(),
        kind: AssetKind::Audio,
        content_hash: "b".repeat(64),
        locator: AssetLocator {
            relative: Some("voice.wav".into()),
            absolute: None,
        },
        streams: vec![StreamMetadata {
            index: 0,
            codec: "pcm_s16le".into(),
            time_base: t(1, 48000),
            duration: Some(t(30, 1)),
            start_time: None,
            width: None,
            height: None,
            pixel_format: None,
            color_primaries: None,
            color_transfer: None,
            color_matrix: None,
            color_range: None,
        }],
    }
}
fn clip(source: SourceRef, a: Time, b: Time) -> Clip {
    Clip {
        id: ClipId::new(),
        source_ref: source,
        timeline_range: range(a, b),
        source_in: Time::ZERO,
        time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
        enabled: true,
        audio_retime: AudioRetimePolicy::Reject,
        reverse_sampling: None,
        volume: None,
        links: vec![],
        effects: vec![],
        markers: vec![],
        masks: vec![],
        properties: vec![],
        pan: None,
    }
}
fn track(kind: TrackKind, clips: Vec<Clip>) -> Track {
    Track {
        state: None,
        id: TrackId::new(),
        kind,
        clips,
    }
}
fn sequence(tracks: Vec<Track>, targets: Option<TargetTracks>) -> Sequence {
    Sequence {
        id: SequenceId::new(),
        extent: DesignExtent::new(16.0, 16.0).unwrap(),
        frame_rate: FrameRate::new(24, 1).unwrap(),
        audio_rate: SampleRate::HZ_48000,
        working_space: ColorSpace::LinearRec709,
        tracks,
        transitions: vec![],
        markers: vec![],
        work_area: None,
        targets,
    }
}
fn service() -> Service<'static> {
    Service::new(BackendSelection::CpuReference)
}
fn setup(p: Project) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nle7.kronello");
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
fn document(path: &Path) -> Project {
    export(path).document
}
fn group_in(path: &Path, id: MulticamId) -> MulticamAsset {
    document(path)
        .multicams
        .into_iter()
        .find(|g| g.id == id)
        .expect("persisted multicam")
}
fn clips_in(path: &Path, sequence: SequenceId, track: TrackId) -> Vec<Clip> {
    let doc = document(path);
    let sequence = doc
        .sequences
        .iter()
        .find_map(|s| match s {
            DocumentObject::Known(s) if s.id == sequence => Some(s.clone()),
            _ => None,
        })
        .expect("sequence");
    sequence
        .tracks
        .into_iter()
        .find(|t| t.id == track)
        .expect("track")
        .clips
}
fn multicam_request(
    path: &Path,
    group: MulticamId,
    sync: MulticamSync,
    angles: Vec<MulticamAngleSpec>,
    offsets: BTreeMap<AngleId, Time>,
) -> MulticamCreateRequest {
    MulticamCreateRequest {
        project: path.into(),
        base_revision: export(path).revision,
        session_id: Uuid::new_v4(),
        idempotency_key: Uuid::new_v4().to_string(),
        multicam: group,
        name: "Interview".into(),
        sync,
        angles,
        reference: None,
        offsets,
    }
}
fn angle_spec(id: AngleId, asset: AssetId) -> MulticamAngleSpec {
    MulticamAngleSpec {
        id,
        asset,
        stream_index: 0,
        audio_stream_index: None,
        name: String::new(),
    }
}

#[test]
fn multicam_create_manual_persists_offsets_and_undoes() {
    let a = video_asset(None, Some(t(30, 1)));
    let b = video_asset(Some(t(2, 1)), Some(t(30, 1)));
    let mut p = Project::default();
    p.assets.push(DocumentObject::Known(a.clone()));
    p.assets.push(DocumentObject::Known(b.clone()));
    let (_dir, path) = setup(p);
    let group = MulticamId::new();
    let angle_a = AngleId::new();
    let angle_b = AngleId::new();
    let event = {
        let ResultData::Edit(event) = service()
            .dispatch(Request::MulticamCreate(multicam_request(
                &path,
                group,
                MulticamSync::Manual,
                vec![angle_spec(angle_a, a.id), angle_spec(angle_b, b.id)],
                BTreeMap::from([(angle_a, Time::ZERO), (angle_b, t(1, 2))]),
            )))
            .unwrap()
        else {
            panic!()
        };
        event
    };
    let stored = group_in(&path, group);
    assert_eq!(stored.name, "Interview");
    assert_eq!(stored.angles.len(), 2);
    assert_eq!(stored.angles[0].id, angle_a);
    assert_eq!(stored.angles[0].asset, a.id);
    assert_eq!(stored.angles[0].sync_offset, Time::ZERO);
    assert_eq!(stored.angles[1].id, angle_b);
    assert_eq!(stored.angles[1].sync_offset, t(1, 2));
    // Selective undo removes the group.
    let ResultData::Edit(_) = service()
        .dispatch(Request::EditUndo(UndoRequest {
            project: path.clone(),
            base_revision: export(&path).revision,
            session_id: Uuid::new_v4(),
            idempotency_key: Uuid::new_v4().to_string(),
            event_id: event.id,
        }))
        .unwrap()
    else {
        panic!()
    };
    assert!(document(&path).multicams.is_empty());
}

#[test]
fn multicam_create_timecode_aligns_stream_starts() {
    let a = video_asset(Some(t(4, 1)), Some(t(30, 1)));
    let b = video_asset(Some(t(10, 1)), Some(t(30, 1)));
    let mut p = Project::default();
    p.assets.push(DocumentObject::Known(a.clone()));
    p.assets.push(DocumentObject::Known(b.clone()));
    let (_dir, path) = setup(p);
    let group = MulticamId::new();
    let angle_a = AngleId::new();
    let angle_b = AngleId::new();
    service()
        .dispatch(Request::MulticamCreate(multicam_request(
            &path,
            group,
            MulticamSync::Timecode,
            vec![angle_spec(angle_a, a.id), angle_spec(angle_b, b.id)],
            BTreeMap::new(),
        )))
        .unwrap();
    let stored = group_in(&path, group);
    // start(B) - start(A) = 10 - 4 = +6s: B's media clock leads the
    // reference by six seconds, so sampling shifts forward by +6.
    assert_eq!(stored.angles[0].sync_offset, Time::ZERO);
    assert_eq!(stored.angles[1].sync_offset, t(6, 1));
}

#[test]
fn multicam_create_rejects_malformed_angles() {
    let a = video_asset(None, Some(t(30, 1)));
    let mut p = Project::default();
    p.assets.push(DocumentObject::Known(a.clone()));
    let (_dir, path) = setup(p);
    let group = MulticamId::new();
    // Missing asset (timecode mode exercises wiring without offsets).
    let error = service()
        .dispatch(Request::MulticamCreate(multicam_request(
            &path,
            group,
            MulticamSync::Timecode,
            vec![angle_spec(AngleId::new(), AssetId::new())],
            BTreeMap::new(),
        )))
        .unwrap_err();
    assert_eq!(error.code, "ASSET_MISSING");
    // Missing stream.
    let mut spec = angle_spec(AngleId::new(), a.id);
    spec.stream_index = 9;
    let error = service()
        .dispatch(Request::MulticamCreate(multicam_request(
            &path,
            group,
            MulticamSync::Timecode,
            vec![spec],
            BTreeMap::new(),
        )))
        .unwrap_err();
    assert_eq!(error.code, "SOURCE_MISSING");
    // Audio stream as the picture source.
    let mut spec = angle_spec(AngleId::new(), a.id);
    spec.stream_index = 1;
    let error = service()
        .dispatch(Request::MulticamCreate(multicam_request(
            &path,
            group,
            MulticamSync::Timecode,
            vec![spec],
            BTreeMap::new(),
        )))
        .unwrap_err();
    assert_eq!(error.code, "INVALID_MULTICAM");
    // Manual sync without exactly one offset per angle.
    let angle = AngleId::new();
    let error = service()
        .dispatch(Request::MulticamCreate(multicam_request(
            &path,
            group,
            MulticamSync::Manual,
            vec![angle_spec(angle, a.id)],
            BTreeMap::new(),
        )))
        .unwrap_err();
    assert_eq!(error.code, "INVALID_MULTICAM");
    // Extra offset keys naming no angle are rejected.
    let error = service()
        .dispatch(Request::MulticamCreate(multicam_request(
            &path,
            group,
            MulticamSync::Manual,
            vec![angle_spec(angle, a.id)],
            BTreeMap::from([(angle, Time::ZERO), (AngleId::new(), t(1, 1))]),
        )))
        .unwrap_err();
    assert_eq!(error.code, "INVALID_MULTICAM");
    // Stale base revision.
    let mut request = multicam_request(
        &path,
        group,
        MulticamSync::Timecode,
        vec![angle_spec(AngleId::new(), a.id)],
        BTreeMap::new(),
    );
    request.base_revision = "0".into();
    let error = service()
        .dispatch(Request::MulticamCreate(request))
        .unwrap_err();
    assert_eq!(error.code, "REVISION_CONFLICT");
}

#[test]
fn multicam_create_audio_sync_without_audio_fails_typed() {
    // A video-only angle cannot contribute audio to correlation, so sync
    // estimation must fail typed instead of decoding or guessing offsets.
    let mut silent = video_asset(None, Some(t(30, 1)));
    silent.streams.truncate(1);
    let voiced = video_asset(None, Some(t(30, 1)));
    let mut p = Project::default();
    p.assets.push(DocumentObject::Known(silent.clone()));
    p.assets.push(DocumentObject::Known(voiced.clone()));
    let (_dir, path) = setup(p);
    let error = service()
        .dispatch(Request::MulticamCreate(multicam_request(
            &path,
            MulticamId::new(),
            MulticamSync::Audio,
            vec![
                angle_spec(AngleId::new(), silent.id),
                angle_spec(AngleId::new(), voiced.id),
            ],
            BTreeMap::new(),
        )))
        .unwrap_err();
    assert_eq!(error.code, "MULTICAM_SYNC_FAILED");
    // The failed request must not persist a group or bump the revision.
    assert_eq!(export(&path).revision, "1");
    assert!(document(&path).multicams.is_empty());
}

fn multicam_clip(group: MulticamId, angle: AngleId, a: Time, b: Time) -> Clip {
    clip(
        SourceRef::Multicam {
            multicam: group,
            angle,
        },
        a,
        b,
    )
}
fn project_with_multicam(
    group: MulticamId,
    angles: [(AngleId, AssetId); 2],
    clip: Clip,
) -> (Project, SequenceId, TrackId) {
    let video = track(TrackKind::Video, vec![clip]);
    let track_id = video.id;
    let mut seq = sequence(vec![video], None);
    let sequence_id = seq.id;
    seq.targets = Some(TargetTracks {
        video: Some(track_id),
        audio: None,
    });
    let mut p = Project::default();
    for (angle, asset) in &angles {
        let _ = angle;
        p.assets
            .push(DocumentObject::Known(video_asset(None, Some(t(30, 1)))));
        let DocumentObject::Known(a) = p.assets.last_mut().unwrap() else {
            panic!()
        };
        a.id = *asset;
    }
    p.multicams.push(MulticamAsset {
        id: group,
        name: "Interview".into(),
        angles: angles
            .iter()
            .map(|(angle, asset)| MulticamAngle {
                id: *angle,
                asset: *asset,
                stream_index: 0,
                sync_offset: Time::ZERO,
                name: String::new(),
            })
            .collect(),
    });
    p.sequences.push(DocumentObject::Known(seq));
    (p, sequence_id, track_id)
}

#[test]
fn clip_angle_switch_repoins_only_the_selected_clip() {
    let group = MulticamId::new();
    let angle_a = AngleId::new();
    let angle_b = AngleId::new();
    let asset_a = AssetId::new();
    let asset_b = AssetId::new();
    let first = multicam_clip(group, angle_a, t(0, 1), t(2, 1));
    let second = multicam_clip(group, angle_a, t(2, 1), t(4, 1));
    let first_id = first.id;
    let (p, sequence_id, track_id) =
        project_with_multicam(group, [(angle_a, asset_a), (angle_b, asset_b)], first);
    let mut p = p;
    let DocumentObject::Known(seq) = &mut p.sequences[0] else {
        panic!()
    };
    seq.tracks[0].clips.push(second);
    let second_id = seq.tracks[0].clips[1].id;
    let (_dir, path) = setup(p);
    let revision = export(&path).revision;
    let ResultData::Edit(_) = service()
        .dispatch(Request::ClipAngleSwitch(ClipAngleSwitchRequest {
            project: path.clone(),
            base_revision: revision,
            session_id: Uuid::new_v4(),
            idempotency_key: "switch-1".into(),
            sequence: sequence_id,
            clip: first_id,
            angle: angle_b,
        }))
        .unwrap()
    else {
        panic!()
    };
    let clips = clips_in(&path, sequence_id, track_id);
    assert_eq!(
        clips.iter().find(|c| c.id == first_id).unwrap().source_ref,
        SourceRef::Multicam {
            multicam: group,
            angle: angle_b,
        }
    );
    // The later clip keeps its authored angle: no propagation.
    assert_eq!(
        clips.iter().find(|c| c.id == second_id).unwrap().source_ref,
        SourceRef::Multicam {
            multicam: group,
            angle: angle_a,
        }
    );
}

#[test]
fn clip_angle_switch_rejects_non_multicam_and_missing() {
    let asset = video_asset(None, Some(t(30, 1)));
    let media_clip = clip(
        SourceRef::Asset {
            asset: asset.id,
            stream_index: 0,
        },
        t(0, 1),
        t(2, 1),
    );
    let clip_id = media_clip.id;
    let video = track(TrackKind::Video, vec![media_clip]);
    let mut p = Project::default();
    p.assets.push(DocumentObject::Known(asset));
    p.sequences
        .push(DocumentObject::Known(sequence(vec![video], None)));
    let sequence_id = match &p.sequences[0] {
        DocumentObject::Known(s) => s.id,
        _ => panic!(),
    };
    let (_dir, path) = setup(p);
    let revision = export(&path).revision;
    // Non-multicam clip.
    let error = service()
        .dispatch(Request::ClipAngleSwitch(ClipAngleSwitchRequest {
            project: path.clone(),
            base_revision: revision.clone(),
            session_id: Uuid::new_v4(),
            idempotency_key: "bad-1".into(),
            sequence: sequence_id,
            clip: clip_id,
            angle: AngleId::new(),
        }))
        .unwrap_err();
    assert_eq!(error.code, "INVALID_CLIP");
    // Missing clip.
    let error = service()
        .dispatch(Request::ClipAngleSwitch(ClipAngleSwitchRequest {
            project: path.clone(),
            base_revision: revision.clone(),
            session_id: Uuid::new_v4(),
            idempotency_key: "bad-2".into(),
            sequence: sequence_id,
            clip: ClipId::new(),
            angle: AngleId::new(),
        }))
        .unwrap_err();
    assert_eq!(error.code, "SOURCE_MISSING");
    // Multicam clip + unknown angle.
    let group = MulticamId::new();
    let angle_a = AngleId::new();
    let asset_a = AssetId::new();
    let (p2, seq2, _) = project_with_multicam(
        group,
        [(angle_a, asset_a), (AngleId::new(), AssetId::new())],
        multicam_clip(group, angle_a, t(0, 1), t(2, 1)),
    );
    let clip_id = match &p2.sequences[0] {
        DocumentObject::Known(s) => s.tracks[0].clips[0].id,
        _ => panic!(),
    };
    let (_dir2, path2) = setup(p2);
    let error = service()
        .dispatch(Request::ClipAngleSwitch(ClipAngleSwitchRequest {
            project: path2.clone(),
            base_revision: export(&path2).revision,
            session_id: Uuid::new_v4(),
            idempotency_key: "bad-3".into(),
            sequence: seq2,
            clip: clip_id,
            angle: AngleId::new(),
        }))
        .unwrap_err();
    assert_eq!(error.code, "SOURCE_MISSING");
}

fn insert_request(
    path: &Path,
    sequence: SequenceId,
    source: SourceRef,
    at: Time,
) -> EditInsertRequest {
    EditInsertRequest {
        project: path.into(),
        base_revision: export(path).revision,
        session_id: Uuid::new_v4(),
        idempotency_key: Uuid::new_v4().to_string(),
        sequence,
        clip: ClipId::new(),
        source,
        source_range: range(Time::ZERO, t(1, 1)),
        at,
        track: None,
        linked: false,
    }
}
fn overwrite_request(
    path: &Path,
    sequence: SequenceId,
    source: SourceRef,
    at: Time,
) -> EditOverwriteRequest {
    EditOverwriteRequest {
        project: path.into(),
        base_revision: export(path).revision,
        session_id: Uuid::new_v4(),
        idempotency_key: Uuid::new_v4().to_string(),
        sequence,
        clip: ClipId::new(),
        source,
        source_range: range(Time::ZERO, t(1, 1)),
        at,
        track: None,
        split_tail: None,
    }
}

#[test]
fn edit_insert_ripples_later_clips_and_uses_targets() {
    let asset = video_asset(None, Some(t(30, 1)));
    let placed = clip(
        SourceRef::Asset {
            asset: asset.id,
            stream_index: 0,
        },
        t(2, 1),
        t(4, 1),
    );
    let placed_id = placed.id;
    let video = track(TrackKind::Video, vec![placed]);
    let video_id = video.id;
    let audio_track = track(TrackKind::Audio, vec![]);
    let audio_id = audio_track.id;
    let seq = sequence(
        vec![video, audio_track],
        Some(TargetTracks {
            video: Some(video_id),
            audio: Some(audio_id),
        }),
    );
    let mut p = Project::default();
    p.assets.push(DocumentObject::Known(asset.clone()));
    p.sequences.push(DocumentObject::Known(seq));
    let sequence_id = match &p.sequences[0] {
        DocumentObject::Known(s) => s.id,
        _ => panic!(),
    };
    let (_dir, path) = setup(p);
    // A one-second source insert at t=0 ripples the t=2 clip to t=3.
    let ResultData::Edit(_) = service()
        .dispatch(Request::EditInsert(insert_request(
            &path,
            sequence_id,
            SourceRef::Asset {
                asset: asset.id,
                stream_index: 0,
            },
            Time::ZERO,
        )))
        .unwrap()
    else {
        panic!()
    };
    let clips = clips_in(&path, sequence_id, video_id);
    assert_eq!(clips.len(), 2);
    let later = clips.iter().find(|c| c.id == placed_id).unwrap();
    assert_eq!(later.timeline_range.start(), t(3, 1));
    let inserted = clips.iter().find(|c| c.id != placed_id).unwrap();
    assert_eq!(inserted.timeline_range, range(Time::ZERO, t(1, 1)));
    assert_eq!(inserted.source_in, Time::ZERO);
    // Idempotent replay: same key returns the original event, no second clip.
    // (Covered by edit.apply idempotency; a fresh key would insert again.)
}

#[test]
fn edit_insert_resolves_audio_targets_and_rejects_mismatch() {
    let asset = audio_asset();
    let video = track(TrackKind::Video, vec![]);
    let audio_track = track(TrackKind::Audio, vec![]);
    let audio_id = audio_track.id;
    let seq = sequence(
        vec![video, audio_track],
        Some(TargetTracks {
            video: None,
            audio: Some(audio_id),
        }),
    );
    let mut p = Project::default();
    p.assets.push(DocumentObject::Known(asset.clone()));
    p.sequences.push(DocumentObject::Known(seq));
    let sequence_id = match &p.sequences[0] {
        DocumentObject::Known(s) => s.id,
        _ => panic!(),
    };
    let (_dir, path) = setup(p);
    service()
        .dispatch(Request::EditInsert(insert_request(
            &path,
            sequence_id,
            SourceRef::Asset {
                asset: asset.id,
                stream_index: 0,
            },
            Time::ZERO,
        )))
        .unwrap();
    assert_eq!(clips_in(&path, sequence_id, audio_id).len(), 1);
    // An audio source cannot target a video track explicitly.
    let mut bad = insert_request(
        &path,
        sequence_id,
        SourceRef::Asset {
            asset: asset.id,
            stream_index: 0,
        },
        Time::ZERO,
    );
    bad.track = match &document(&path).sequences[0] {
        DocumentObject::Known(s) => s
            .tracks
            .iter()
            .find(|t| t.kind == TrackKind::Video)
            .map(|t| t.id),
        _ => panic!(),
    };
    let error = service().dispatch(Request::EditInsert(bad)).unwrap_err();
    assert_eq!(error.code, "INVALID_CLIP");
}

#[test]
fn edit_overwrite_replaces_covered_content() {
    let asset = video_asset(None, Some(t(30, 1)));
    let covered = clip(
        SourceRef::Asset {
            asset: asset.id,
            stream_index: 0,
        },
        t(0, 1),
        t(3, 1),
    );
    let covered_id = covered.id;
    let video = track(TrackKind::Video, vec![covered]);
    let video_id = video.id;
    let seq = sequence(
        vec![video],
        Some(TargetTracks {
            video: Some(video_id),
            audio: None,
        }),
    );
    let mut p = Project::default();
    p.assets.push(DocumentObject::Known(asset.clone()));
    p.sequences.push(DocumentObject::Known(seq));
    let sequence_id = match &p.sequences[0] {
        DocumentObject::Known(s) => s.id,
        _ => panic!(),
    };
    let (_dir, path) = setup(p);
    // Overwriting [1, 2) splits the covering clip into [0,1) + [2,3); the
    // split tail needs a caller-supplied stable id.
    let mut request = overwrite_request(
        &path,
        sequence_id,
        SourceRef::Asset {
            asset: asset.id,
            stream_index: 0,
        },
        t(1, 1),
    );
    let error = service()
        .dispatch(Request::EditOverwrite(request.clone()))
        .unwrap_err();
    assert_eq!(error.code, "INVALID_CLIP");
    let tail = ClipId::new();
    request.split_tail = Some(tail);
    let ResultData::Edit(_) = service().dispatch(Request::EditOverwrite(request)).unwrap() else {
        panic!()
    };
    let clips = clips_in(&path, sequence_id, video_id);
    assert_eq!(clips.len(), 3);
    let mut ranges: Vec<_> = clips.iter().map(|c| c.timeline_range).collect();
    ranges.sort_by_key(|r| r.start());
    assert_eq!(
        ranges,
        vec![
            range(Time::ZERO, t(1, 1)),
            range(t(1, 1), t(2, 1)),
            range(t(2, 1), t(3, 1))
        ]
    );
    // The head keeps the covered clip's identity; the tail takes split_tail.
    let head = clips.iter().find(|c| c.id == covered_id).unwrap();
    assert_eq!(head.timeline_range, range(Time::ZERO, t(1, 1)));
    let tail_clip = clips.iter().find(|c| c.id == tail).unwrap();
    assert_eq!(tail_clip.timeline_range, range(t(2, 1), t(3, 1)));
    // The tail continues the covered clip's source window: source_in moved
    // forward by the covered head's duration.
    assert_eq!(tail_clip.source_in, t(2, 1));
}

#[test]
fn edit_insert_rejects_unresolved_destination_and_source() {
    let asset = video_asset(None, Some(t(30, 1)));
    let video = track(TrackKind::Video, vec![]);
    let seq = sequence(vec![video], None);
    let mut p = Project::default();
    p.assets.push(DocumentObject::Known(asset.clone()));
    p.sequences.push(DocumentObject::Known(seq));
    let sequence_id = match &p.sequences[0] {
        DocumentObject::Known(s) => s.id,
        _ => panic!(),
    };
    let (_dir, path) = setup(p);
    // No explicit track and no sequence targets.
    let error = service()
        .dispatch(Request::EditInsert(insert_request(
            &path,
            sequence_id,
            SourceRef::Asset {
                asset: asset.id,
                stream_index: 0,
            },
            Time::ZERO,
        )))
        .unwrap_err();
    assert_eq!(error.code, "TARGET_MISSING");
    // Missing asset.
    let error = service()
        .dispatch(Request::EditInsert(insert_request(
            &path,
            sequence_id,
            SourceRef::Asset {
                asset: AssetId::new(),
                stream_index: 0,
            },
            Time::ZERO,
        )))
        .unwrap_err();
    assert_eq!(error.code, "ASSET_MISSING");
    // Missing sequence.
    let error = service()
        .dispatch(Request::EditInsert(insert_request(
            &path,
            SequenceId::new(),
            SourceRef::Asset {
                asset: asset.id,
                stream_index: 0,
            },
            Time::ZERO,
        )))
        .unwrap_err();
    assert_eq!(error.code, "SOURCE_MISSING");
    // Empty source window.
    let mut bad = insert_request(
        &path,
        sequence_id,
        SourceRef::Asset {
            asset: asset.id,
            stream_index: 0,
        },
        Time::ZERO,
    );
    bad.source_range = range(Time::ZERO, Time::ZERO);
    bad.track = Some(match &document(&path).sequences[0] {
        DocumentObject::Known(s) => s.tracks[0].id,
        _ => panic!(),
    });
    let error = service().dispatch(Request::EditInsert(bad)).unwrap_err();
    assert_eq!(error.code, "INVALID_CLIP");
    // Caption/adjustment sources are not insertable media.
    let mut bad = insert_request(&path, sequence_id, SourceRef::Adjustment, Time::ZERO);
    bad.track = None;
    let error = service().dispatch(Request::EditInsert(bad)).unwrap_err();
    assert_eq!(error.code, "INVALID_CLIP");
}
