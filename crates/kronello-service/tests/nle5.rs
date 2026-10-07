//! NLE-005 (persisted track locks, clip enable, sequence targets) and
//! NLE-006 (piecewise retime holds and `clip_freeze`) through the shared
//! command path, including atomic rejection and selective undo.
use kronello_gpu::render_adapter::CpuReferenceBackend;
use kronello_model::*;
use kronello_render::{OutputRegion, RenderSnapshot, render_frame};
use kronello_service::*;
use kronello_time::{FrameRate, Rational, SampleRate, Time, TimeMap, TimeMapPoint, TimeRange};
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
        markers: vec![],
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
            duration: Some(t(8, 1)),
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
fn audio_clip(asset: AssetId, a: Time, b: Time, retime: AudioRetimePolicy) -> Clip {
    Clip {
        source_ref: SourceRef::Asset {
            asset,
            stream_index: 0,
        },
        timeline_range: range(a, b),
        audio_retime: retime,
        ..clip(a, b)
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
fn sequence(tracks: Vec<Track>) -> Sequence {
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
        targets: None,
    }
}
fn unlocked() -> TrackState {
    TrackState {
        visible: true,
        muted: false,
        locked: false,
    }
}
fn service() -> Service<'static> {
    Service::new(BackendSelection::CpuReference)
}
fn setup(p: Project) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nle5.kronello");
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
fn plan_edit(path: &Path, commands: Vec<EditCommand>) -> Result<EditApplyRequest, ServiceError> {
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
fn plan(path: &Path, commands: Vec<TimelineCommand>) -> Result<EditApplyRequest, ServiceError> {
    plan_edit(
        path,
        commands
            .into_iter()
            .map(|c| EditCommand::Timeline(Box::new(c)))
            .collect(),
    )
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
    // The failed plan is atomic: no event, no document or revision drift.
    let after = export(path);
    assert_eq!(after.document, before.document);
    assert_eq!(after.revision, before.revision);
}
fn reject_edit(path: &Path, command: EditCommand, code: &str) {
    let before = export(path);
    let e = plan_edit(path, vec![command]).unwrap_err();
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
fn snapshot(p: &Project, sequence: SequenceId) -> RenderSnapshot {
    RenderSnapshot::for_target(
        p,
        RenderTarget::Sequence { sequence },
        0,
        Default::default(),
    )
    .unwrap()
}
fn cpu(p: &Project, sequence: SequenceId, time: Time) -> kronello_render::RenderedFrame {
    render_frame(
        &snapshot(p, sequence),
        &[],
        &CpuReferenceBackend,
        kronello_render::FrameRequest {
            time,
            region: OutputRegion {
                origin: [0.0; 2],
                extent: [16.0; 2],
                pixels: [16; 2],
            },
        },
    )
    .unwrap()
}
fn piecewise(points: &[(i64, i64, i64, i64)]) -> TimeMap {
    TimeMap::piecewise_linear(
        points
            .iter()
            .map(|&(pn, pd, ln, ld)| TimeMapPoint {
                parent: t(pn, pd),
                local: t(ln, ld),
            })
            .collect(),
    )
    .unwrap()
}
fn marker(time: Time) -> Marker {
    Marker {
        id: MarkerId::new(),
        time,
        color: MarkerColor::Blue,
        comment: None,
    }
}

#[test]
fn legacy_documents_default_track_lock_clip_enable_and_targets() {
    let mut s = sequence(vec![track(TrackKind::Video, vec![clip(t(0, 1), t(4, 1))])]);
    s.tracks[0].state = Some(unlocked());
    s.targets = Some(TargetTracks {
        video: Some(s.tracks[0].id),
        audio: None,
    });
    let decoded: Sequence = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(decoded, s);
    // Documents written before NLE-005 omit every new field.
    let mut legacy = serde_json::to_value(&s).unwrap();
    legacy.as_object_mut().unwrap().remove("targets");
    legacy["tracks"][0]["state"]
        .as_object_mut()
        .unwrap()
        .remove("locked");
    let clip_json = serde_json::to_value(&legacy["tracks"][0]["clips"][0]).unwrap();
    // An enabled clip writes no `enabled` key at all.
    assert!(clip_json.get("enabled").is_none());
    let decoded: Sequence = serde_json::from_value(legacy).unwrap();
    assert_eq!(decoded.tracks[0].state, Some(unlocked()));
    assert!(decoded.tracks[0].clips[0].enabled);
    assert_eq!(decoded.targets, None);
    // A disabled clip serializes the flag and roundtrips.
    let mut disabled = clip_json.clone();
    disabled["enabled"] = serde_json::json!(false);
    let clip: Clip = serde_json::from_value(disabled).unwrap();
    assert!(!clip.enabled);
    assert_eq!(
        serde_json::to_value(clip).unwrap()["enabled"],
        serde_json::json!(false)
    );
}

#[test]
fn locked_track_rejects_every_clip_mutation_atomically() {
    // a and b overlap so their transition is authored; c and d are adjacent
    // so slide/roll gestures have absorbing neighbours.
    let a = clip(t(0, 1), t(3, 1));
    let b = clip(t(2, 1), t(5, 1));
    let c = clip(t(5, 1), t(8, 1));
    let d = clip(t(8, 1), t(10, 1));
    let partner = clip(t(0, 1), t(2, 1));
    let mut s = sequence(vec![
        track(
            TrackKind::Video,
            vec![a.clone(), b.clone(), c.clone(), d.clone()],
        ),
        track(TrackKind::Video, vec![partner.clone()]),
    ]);
    s.transitions = vec![Transition {
        outgoing: a.id,
        incoming: b.id,
        range: range(t(2, 1), t(3, 1)),
        kind: TransitionKind::Crossfade,
        params: None,
        version: 1,
    }];
    let sid = s.id;
    let (locked_track, free_track) = (s.tracks[0].id, s.tracks[1].id);
    let (_dir, path) = setup(project(s));
    // Author the cross-track link, a clip marker and an opacity property
    // while unlocked.
    let clip_marker = marker(t(6, 1));
    let registry = SchemaRegistry::with_builtin();
    let opacity = Property::new(
        PropertyId::new(),
        DescriptorRef::new(
            registry
                .lookup(&SchemaKey::new("kronello.opacity").unwrap())
                .unwrap(),
        ),
        PropertySource::Constant(Value::Scalar(FiniteF64::new(1.0).unwrap())),
        vec![],
        &registry,
    )
    .unwrap();
    let opacity_id = opacity.id();
    apply(
        &path,
        vec![
            TimelineCommand::ClipLink {
                sequence: sid,
                clips: vec![a.id, partner.id],
            },
            TimelineCommand::MarkerSet {
                sequence: sid,
                clip: Some(c.id),
                marker: clip_marker.clone(),
            },
            TimelineCommand::ClipSetEffects {
                sequence: sid,
                clip: c.id,
                properties: vec![opacity],
                effects: vec![],
            },
        ],
    );
    apply(
        &path,
        vec![TimelineCommand::TrackStateSet {
            sequence: sid,
            track: locked_track,
            state: TrackState {
                locked: true,
                ..unlocked()
            },
        }],
    );
    for command in [
        // Direct clip mutations on the locked track.
        TimelineCommand::ClipDelete {
            sequence: sid,
            clip: b.id,
            linked: false,
        },
        TimelineCommand::ClipMove {
            sequence: sid,
            clip: c.id,
            delta: t(1, 1),
            linked: false,
        },
        TimelineCommand::ClipTrim {
            sequence: sid,
            clip: c.id,
            range: range(t(4, 1), t(5, 1)),
        },
        TimelineCommand::ClipStretch {
            sequence: sid,
            clip: c.id,
            range: range(t(4, 1), t(5, 1)),
        },
        TimelineCommand::ClipSplit {
            sequence: sid,
            clip: c.id,
            time: t(5, 1),
            right_clip: ClipId::new(),
        },
        TimelineCommand::ClipSlip {
            sequence: sid,
            clip: c.id,
            delta: t(1, 2),
            linked: false,
        },
        // Slide and roll rewrite the adjacent absorbers too, so the lock
        // covers clips that are not the gesture target.
        TimelineCommand::ClipSlide {
            sequence: sid,
            clip: c.id,
            delta: t(1, 4),
            linked: false,
        },
        TimelineCommand::ClipRoll {
            sequence: sid,
            clip: c.id,
            delta: t(-1, 2),
            linked: false,
        },
        TimelineCommand::ClipTimeSet {
            sequence: sid,
            clip: c.id,
            source_in: Time::ZERO,
            time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
            audio_retime: AudioRetimePolicy::Reject,
            reverse_sampling: None,
        },
        TimelineCommand::ClipEnableSet {
            sequence: sid,
            clip: c.id,
            enabled: false,
        },
        TimelineCommand::ClipSetVolume {
            sequence: sid,
            clip: c.id,
            volume: None,
        },
        TimelineCommand::ClipSetEffects {
            sequence: sid,
            clip: c.id,
            properties: vec![],
            effects: vec![],
        },
        TimelineCommand::ClipFreeze {
            sequence: sid,
            clip: c.id,
            at: t(5, 1),
        },
        // Clip-scoped marker writes are clip mutations.
        TimelineCommand::MarkerSet {
            sequence: sid,
            clip: Some(c.id),
            marker: marker(t(9, 2)),
        },
        TimelineCommand::MarkerMove {
            sequence: sid,
            clip: Some(c.id),
            marker: clip_marker.id,
            time: t(11, 2),
        },
        TimelineCommand::MarkerRemove {
            sequence: sid,
            clip: Some(c.id),
            marker: clip_marker.id,
        },
        // Track-targeting structural edits.
        TimelineCommand::ClipPlace {
            sequence: sid,
            track: locked_track,
            clip: Box::new(clip(t(9, 1), t(10, 1))),
        },
        TimelineCommand::ClipInsert {
            sequence: sid,
            track: locked_track,
            clip: Box::new(clip(t(11, 1), t(12, 1))),
            linked: false,
        },
        TimelineCommand::ClipOverwrite {
            sequence: sid,
            track: locked_track,
            clip: Box::new(clip(t(9, 1), t(10, 1))),
        },
        TimelineCommand::Ripple {
            sequence: sid,
            tracks: vec![locked_track],
            pivot: t(5, 1),
            delta: t(1, 1),
            linked: false,
        },
        TimelineCommand::RippleDelete {
            sequence: sid,
            tracks: vec![locked_track],
            range: range(t(9, 1), t(10, 1)),
            linked: false,
        },
        // Transition edits touching locked-track clips.
        TimelineCommand::TransitionSet {
            sequence: sid,
            transition: Transition {
                outgoing: c.id,
                incoming: d.id,
                range: range(t(23, 4), t(6, 1)),
                kind: TransitionKind::Crossfade,
                params: None,
                version: 1,
            },
        },
        TimelineCommand::TransitionRemove {
            sequence: sid,
            outgoing: a.id,
            incoming: b.id,
        },
    ] {
        reject(&path, vec![command], "TRACK_LOCKED");
    }
    // Generic clip property commands funnel through the shared resolution
    // point and hit the same lock.
    reject_edit(
        &path,
        EditCommand::PropertySourceSet {
            object: c.id.as_uuid(),
            property: opacity_id,
            source: PropertySource::Constant(Value::Scalar(FiniteF64::new(0.5).unwrap())),
            curve: None,
        },
        "TRACK_LOCKED",
    );
    // A mutation on the unlocked track that pulls a locked clip into the
    // affected set rejects: linked delete and link-group rewrites.
    reject(
        &path,
        vec![TimelineCommand::ClipDelete {
            sequence: sid,
            clip: partner.id,
            linked: true,
        }],
        "TRACK_LOCKED",
    );
    reject(
        &path,
        vec![TimelineCommand::ClipMove {
            sequence: sid,
            clip: partner.id,
            delta: t(1, 1),
            linked: true,
        }],
        "TRACK_LOCKED",
    );
    reject(
        &path,
        vec![TimelineCommand::ClipLink {
            sequence: sid,
            clips: vec![partner.id],
        }],
        "TRACK_LOCKED",
    );
    // Sequence-level writes stay editable under the lock.
    apply(
        &path,
        vec![
            TimelineCommand::MarkerSet {
                sequence: sid,
                clip: None,
                marker: marker(t(6, 1)),
            },
            TimelineCommand::WorkAreaSet {
                sequence: sid,
                work_area: Some(range(t(0, 1), t(8, 1))),
            },
            TimelineCommand::SequenceTargetsSet {
                sequence: sid,
                targets: Some(TargetTracks {
                    video: Some(free_track),
                    audio: None,
                }),
            },
        ],
    );
    assert!(query(&path, sid).sequence.work_area.is_some());
    // The lock itself stays editable and unlocks the track again.
    apply(
        &path,
        vec![TimelineCommand::TrackStateSet {
            sequence: sid,
            track: locked_track,
            state: unlocked(),
        }],
    );
    apply(
        &path,
        vec![TimelineCommand::ClipDelete {
            sequence: sid,
            clip: d.id,
            linked: false,
        }],
    );
    assert_eq!(query(&path, sid).clips.len(), 4);
}

#[test]
fn clip_enable_set_disables_contribution_keeps_occupancy_and_undoes() {
    let a = clip(t(0, 1), t(2, 1));
    let s = sequence(vec![track(TrackKind::Video, vec![a.clone()])]);
    let sid = s.id;
    let tid = s.tracks[0].id;
    let (_dir, path) = setup(project(s));
    let enabled_frame = cpu(&export(&path).document, sid, t(1, 1));
    assert!(enabled_frame.pixels.linear.iter().all(|p| p[3] > 0.0));
    let event = apply(
        &path,
        vec![TimelineCommand::ClipEnableSet {
            sequence: sid,
            clip: a.id,
            enabled: false,
        }],
    );
    assert!(!query(&path, sid).clips[0].clip.enabled);
    // With no newer events the inverse restores `enabled` cleanly.
    undo(&path, event.id).unwrap();
    assert!(query(&path, sid).clips[0].clip.enabled);
    let event = apply(
        &path,
        vec![TimelineCommand::ClipEnableSet {
            sequence: sid,
            clip: a.id,
            enabled: false,
        }],
    );
    assert!(!query(&path, sid).clips[0].clip.enabled);
    // The disabled clip still occupies its range: overlapping placement
    // conflicts and content-end still covers it, but it renders nothing.
    reject(
        &path,
        vec![TimelineCommand::ClipPlace {
            sequence: sid,
            track: tid,
            clip: Box::new(clip(t(1, 1), t(3, 1))),
        }],
        "CLIP_OVERLAP",
    );
    let marker_event = apply(
        &path,
        vec![TimelineCommand::MarkerSet {
            sequence: sid,
            clip: None,
            marker: Marker {
                id: MarkerId::new(),
                time: t(2, 1),
                color: MarkerColor::White,
                comment: None,
            },
        }],
    );
    let disabled_frame = cpu(&export(&path).document, sid, t(1, 1));
    assert!(disabled_frame.pixels.linear.iter().all(|p| p[3] == 0.0));
    // Selective undo unwinds newest first: the marker shares the sequence
    // structure key, so the enable event cannot undo before it.
    assert_eq!(undo(&path, event.id).unwrap_err().code, "UNDO_CONFLICT");
    undo(&path, marker_event.id).unwrap();
    assert!(query(&path, sid).sequence.markers.is_empty());
}

#[test]
fn disabled_clip_keeps_transitions_out_of_rendering() {
    let a = clip(t(0, 1), t(2, 1));
    let mut b = clip(t(1, 1), t(3, 1));
    b.source_ref = SourceRef::Generator {
        generator: SOLID_GENERATOR_ID.into(),
        version: 1,
        color: Color::from_srgb8([0, 40, 120], None),
    };
    let mut s = sequence(vec![track(TrackKind::Video, vec![a.clone(), b.clone()])]);
    let sid = s.id;
    s.transitions = vec![Transition {
        outgoing: a.id,
        incoming: b.id,
        range: range(t(1, 1), t(2, 1)),
        kind: TransitionKind::Crossfade,
        params: None,
        version: 1,
    }];
    let (_dir, path) = setup(project(s));
    // Mid-crossfade the incoming clip blends at partial opacity over the
    // opaque outgoing clip: the composite is fully opaque but carries the
    // outgoing clip's red channel, which the pure incoming clip lacks.
    let mid = cpu(&export(&path).document, sid, t(3, 2));
    assert!(mid.pixels.linear.iter().all(|p| p[3] == 1.0 && p[0] > 0.0));
    // Disabling the outgoing endpoint suppresses the transition entirely:
    // the incoming clip renders opaque, not half-blended with a hole.
    apply(
        &path,
        vec![TimelineCommand::ClipEnableSet {
            sequence: sid,
            clip: a.id,
            enabled: false,
        }],
    );
    let disabled = cpu(&export(&path).document, sid, t(3, 2));
    assert!(
        disabled
            .pixels
            .linear
            .iter()
            .all(|p| p[3] == 1.0 && p[0] == 0.0)
    );
    // Disabling the incoming clip removes its contribution: the range shows
    // nothing where only the transition would have drawn it.
    apply(
        &path,
        vec![
            TimelineCommand::ClipEnableSet {
                sequence: sid,
                clip: a.id,
                enabled: true,
            },
            TimelineCommand::ClipEnableSet {
                sequence: sid,
                clip: b.id,
                enabled: false,
            },
        ],
    );
    let hole = cpu(&export(&path).document, sid, t(5, 2));
    assert!(hole.pixels.linear.iter().all(|p| p[3] == 0.0));
}

#[test]
fn sequence_targets_set_validates_persists_and_clears() {
    let audio = audio_asset();
    let asset = audio.id;
    let mut p = project(sequence(vec![
        track(TrackKind::Video, vec![clip(t(0, 1), t(2, 1))]),
        track(TrackKind::Video, vec![]),
        track(
            TrackKind::Audio,
            vec![audio_clip(
                asset,
                t(0, 1),
                t(2, 1),
                AudioRetimePolicy::Reject,
            )],
        ),
    ]));
    p.assets.push(DocumentObject::Known(audio));
    let DocumentObject::Known(s) = &p.sequences[0] else {
        panic!()
    };
    let sid = s.id;
    let (v1, v2, a1) = (s.tracks[0].id, s.tracks[1].id, s.tracks[2].id);
    let (_dir, path) = setup(p);
    // Kind mismatch and dangling references reject with typed codes.
    reject(
        &path,
        vec![TimelineCommand::SequenceTargetsSet {
            sequence: sid,
            targets: Some(TargetTracks {
                video: Some(a1),
                audio: None,
            }),
        }],
        "INVALID_CLIP",
    );
    reject(
        &path,
        vec![TimelineCommand::SequenceTargetsSet {
            sequence: sid,
            targets: Some(TargetTracks {
                video: Some(TrackId::new()),
                audio: None,
            }),
        }],
        "SOURCE_MISSING",
    );
    let event = apply(
        &path,
        vec![TimelineCommand::SequenceTargetsSet {
            sequence: sid,
            targets: Some(TargetTracks {
                video: Some(v2),
                audio: Some(a1),
            }),
        }],
    );
    let targets = query(&path, sid).sequence.targets.unwrap();
    assert_eq!(targets.video, Some(v2));
    assert_eq!(targets.audio, Some(a1));
    // The exported document persists the authored targets.
    let DocumentObject::Known(exported) = &export(&path).document.sequences[0] else {
        panic!()
    };
    assert_eq!(exported.targets.unwrap().video, Some(v2));
    // Re-targeting moves the entry; the audio entry stays.
    apply(
        &path,
        vec![TimelineCommand::SequenceTargetsSet {
            sequence: sid,
            targets: Some(TargetTracks {
                video: Some(v1),
                audio: Some(a1),
            }),
        }],
    );
    assert_eq!(query(&path, sid).sequence.targets.unwrap().video, Some(v1));
    // An all-absent payload normalizes to no targeting.
    apply(
        &path,
        vec![TimelineCommand::SequenceTargetsSet {
            sequence: sid,
            targets: Some(TargetTracks {
                video: None,
                audio: None,
            }),
        }],
    );
    assert_eq!(query(&path, sid).sequence.targets, None);
    // The earliest targeting event can no longer undo: later writes touched
    // the same sequence structure key.
    assert_eq!(undo(&path, event.id).unwrap_err().code, "UNDO_CONFLICT");
}

#[test]
fn clip_freeze_splits_into_hold_and_undoes_atomically() {
    let a = clip(t(0, 1), t(4, 1));
    let s = sequence(vec![track(TrackKind::Video, vec![a.clone()])]);
    let sid = s.id;
    let (_dir, path) = setup(project(s));
    let initial = export(&path).document;
    let event = apply(
        &path,
        vec![TimelineCommand::ClipFreeze {
            sequence: sid,
            clip: a.id,
            at: t(3, 2),
        }],
    );
    let clips = &query(&path, sid).clips;
    assert_eq!(clips.len(), 2);
    let left = &clips[0].clip;
    let right = &clips[1].clip;
    assert_eq!(left.id, a.id);
    assert_eq!(left.timeline_range, range(t(0, 1), t(3, 2)));
    assert_eq!(right.timeline_range, range(t(3, 2), t(4, 1)));
    // The right part pins the source time at the cut through its duration.
    let TimeMap::PiecewiseLinear(map) = &right.time_map else {
        panic!("freeze must produce a piecewise hold map")
    };
    assert_eq!(
        map.points(),
        &[
            TimeMapPoint {
                parent: Time::ZERO,
                local: Time::ZERO
            },
            TimeMapPoint {
                parent: t(5, 2),
                local: Time::ZERO
            },
        ]
    );
    assert!(map.is_hold(t(1, 1)));
    assert_eq!(right.source_in, t(3, 2));
    assert_eq!(right.local_time(t(2, 1)).unwrap(), t(3, 2));
    assert_eq!(right.local_time(t(7, 2)).unwrap(), t(3, 2));
    // Undo restores the original placement exactly; a repeated apply derives
    // the same deterministic right id.
    undo(&path, event.id).unwrap();
    assert_eq!(export(&path).document, initial);
    apply(
        &path,
        vec![TimelineCommand::ClipFreeze {
            sequence: sid,
            clip: a.id,
            at: t(3, 2),
        }],
    );
    assert_eq!(query(&path, sid).clips[1].clip.id, right.id);
}

#[test]
fn clip_freeze_rejects_boundaries_links_transitions_reverse_and_reject_audio() {
    let audio = audio_asset();
    let asset = audio.id;
    // a and b overlap [3, 4) so their transition is valid.
    let a = clip(t(0, 1), t(4, 1));
    let b = clip(t(3, 1), t(6, 1));
    let plain = clip(t(0, 1), t(4, 1));
    let linked_a = clip(t(0, 1), t(2, 1));
    let linked_b = clip(t(0, 1), t(2, 1));
    let mut reversed = clip(t(8, 1), t(10, 1));
    // A valid reverse_grid_v1 clip needs a positive-slope Linear map whose
    // mapped envelope stays inside [0, source_in].
    reversed.source_in = t(8, 1);
    reversed.time_map = TimeMap::linear(t(6, 1), Rational::ONE).unwrap();
    reversed.reverse_sampling = Some(ReverseSampling::ReverseGridV1);
    reversed.audio_retime = AudioRetimePolicy::ReverseResampleV1;
    let mut s = sequence(vec![
        track(TrackKind::Video, vec![a.clone(), b.clone()]),
        track(TrackKind::Video, vec![plain.clone(), reversed]),
        track(TrackKind::Video, vec![linked_a.clone()]),
        track(TrackKind::Video, vec![linked_b.clone()]),
        track(
            TrackKind::Audio,
            vec![audio_clip(
                asset,
                t(0, 1),
                t(2, 1),
                AudioRetimePolicy::Reject,
            )],
        ),
    ]);
    let sid = s.id;
    s.transitions = vec![Transition {
        outgoing: a.id,
        incoming: b.id,
        range: range(t(3, 1), t(4, 1)),
        kind: TransitionKind::Crossfade,
        params: None,
        version: 1,
    }];
    s.tracks[2].clips[0].links = vec![linked_b.id];
    s.tracks[3].clips[0].links = vec![linked_a.id];
    let mut p = project(s);
    p.assets.push(DocumentObject::Known(audio));
    let (_dir, path) = setup(p);
    for (command, code) in [
        // `at` must be strictly inside the placement.
        (
            TimelineCommand::ClipFreeze {
                sequence: sid,
                clip: plain.id,
                at: t(0, 1),
            },
            "INVALID_EDIT",
        ),
        (
            TimelineCommand::ClipFreeze {
                sequence: sid,
                clip: plain.id,
                at: t(4, 1),
            },
            "INVALID_EDIT",
        ),
        (
            TimelineCommand::ClipFreeze {
                sequence: sid,
                clip: plain.id,
                at: t(9, 1),
            },
            "INVALID_EDIT",
        ),
        (
            TimelineCommand::ClipFreeze {
                sequence: sid,
                clip: linked_a.id,
                at: t(1, 1),
            },
            "LINKED_EDIT_REQUIRED",
        ),
        (
            TimelineCommand::ClipFreeze {
                sequence: sid,
                clip: a.id,
                at: t(2, 1),
            },
            "TRANSITION_EDIT_CONFLICT",
        ),
        (
            TimelineCommand::ClipFreeze {
                sequence: sid,
                clip: b.id,
                at: t(5, 1),
            },
            "TRANSITION_EDIT_CONFLICT",
        ),
    ] {
        reject(&path, vec![command], code);
    }
    let reversed_id = query(&path, sid)
        .clips
        .iter()
        .find(|c| c.clip.timeline_range.start() == t(8, 1))
        .unwrap()
        .clip
        .id;
    reject(
        &path,
        vec![TimelineCommand::ClipFreeze {
            sequence: sid,
            clip: reversed_id,
            at: t(9, 1),
        }],
        "UNSUPPORTED_FEATURE",
    );
    // An audio clip under the Reject retime policy cannot hold: the hold map
    // is non-unit retime and validation rejects it at plan time.
    let rejecting_id = query(&path, sid)
        .clips
        .iter()
        .find(|c| c.kind == ClipKind::Audio)
        .unwrap()
        .clip
        .id;
    reject(
        &path,
        vec![TimelineCommand::ClipFreeze {
            sequence: sid,
            clip: rejecting_id,
            at: t(1, 1),
        }],
        "UNSUPPORTED_FEATURE",
    );
}

#[test]
fn clip_time_set_piecewise_ramp_and_hold_validate_domain() {
    let audio = audio_asset();
    let asset = audio.id;
    let a = clip(t(0, 1), t(4, 1));
    let resampled = audio_clip(asset, t(0, 1), t(2, 1), AudioRetimePolicy::ResampleV1);
    let rejecting = audio_clip(asset, t(2, 1), t(4, 1), AudioRetimePolicy::Reject);
    let s = sequence(vec![
        track(TrackKind::Video, vec![a.clone()]),
        track(TrackKind::Audio, vec![resampled.clone(), rejecting.clone()]),
    ]);
    let sid = s.id;
    let mut p = project(s);
    p.assets.push(DocumentObject::Known(audio));
    let (_dir, path) = setup(p);
    // A speed ramp covering the whole placement is accepted.
    let ramp = piecewise(&[(0, 1, 0, 1), (2, 1, 3, 1), (4, 1, 5, 1)]);
    apply(
        &path,
        vec![TimelineCommand::ClipTimeSet {
            sequence: sid,
            clip: a.id,
            source_in: Time::ZERO,
            time_map: ramp.clone(),
            audio_retime: AudioRetimePolicy::Reject,
            reverse_sampling: None,
        }],
    );
    let stored = &query(&path, sid).clips[0].clip;
    assert_eq!(stored.time_map, ramp);
    // A hold freezes the source across the plateau.
    let hold = piecewise(&[(0, 1, 0, 1), (1, 1, 1, 1), (3, 1, 1, 1), (4, 1, 3, 1)]);
    apply(
        &path,
        vec![TimelineCommand::ClipTimeSet {
            sequence: sid,
            clip: a.id,
            source_in: Time::ZERO,
            time_map: hold,
            audio_retime: AudioRetimePolicy::Reject,
            reverse_sampling: None,
        }],
    );
    let stored = &query(&path, sid).clips[0].clip;
    assert_eq!(stored.local_time(t(3, 2)).unwrap(), t(1, 1));
    assert_eq!(stored.local_time(t(5, 2)).unwrap(), t(1, 1));
    assert_eq!(stored.local_time(t(4, 1)).unwrap(), t(3, 1));
    // The map domain must cover the placement; a short domain is rejected.
    reject(
        &path,
        vec![TimelineCommand::ClipTimeSet {
            sequence: sid,
            clip: a.id,
            source_in: Time::ZERO,
            time_map: piecewise(&[(0, 1, 0, 1), (2, 1, 2, 1)]),
            audio_retime: AudioRetimePolicy::Reject,
            reverse_sampling: None,
        }],
        "TIME_MAP_OUT_OF_DOMAIN",
    );
    // Resampled audio accepts the same hold map; the Reject policy does not.
    let audio_hold = piecewise(&[(0, 1, 0, 1), (1, 1, 1, 1), (2, 1, 1, 1)]);
    apply(
        &path,
        vec![TimelineCommand::ClipTimeSet {
            sequence: sid,
            clip: resampled.id,
            source_in: Time::ZERO,
            time_map: audio_hold.clone(),
            audio_retime: AudioRetimePolicy::ResampleV1,
            reverse_sampling: None,
        }],
    );
    reject(
        &path,
        vec![TimelineCommand::ClipTimeSet {
            sequence: sid,
            clip: rejecting.id,
            source_in: Time::ZERO,
            time_map: audio_hold,
            audio_retime: AudioRetimePolicy::Reject,
            reverse_sampling: None,
        }],
        "UNSUPPORTED_FEATURE",
    );
}

#[test]
fn disabled_caption_clip_exports_no_cues_and_keeps_occupancy() {
    let caption = CaptionDocument {
        id: CaptionId::new(),
        version: 1,
        text: "きょうは晴れ".into(),
        style: CaptionStyle {
            font: FontRef {
                family: "TestSans".into(),
                postscript_name: "TestSans-Regular".into(),
                sha256: "a".repeat(64),
                face_index: 0,
            },
            size: FiniteF64::new(24.0).unwrap(),
            fill: Color::from_srgb8([255, 255, 255], None),
            outline: None,
            background: None,
        },
        spans: vec![],
        placement: CaptionPlacement::default(),
        format: Some(CaptionFormat::Srt),
    };
    let cue = clip(t(1, 1), t(2, 1));
    let mut cue = cue;
    cue.source_ref = SourceRef::Caption {
        caption: caption.id,
    };
    let s = sequence(vec![track(TrackKind::Caption, vec![cue.clone()])]);
    let sid = s.id;
    let mut p = project(s);
    p.captions.push(DocumentObject::Known(caption));
    let (_dir, path) = setup(p);
    let export_srt = |path: &Path| {
        let ResultData::Captions(result) = service()
            .dispatch(Request::CaptionsExport(CaptionsExportRequest {
                project: path.into(),
                sequence: sid,
                format: CaptionFormat::Srt,
            }))
            .unwrap()
        else {
            panic!()
        };
        result.content
    };
    assert!(export_srt(&path).contains("きょうは晴れ"));
    // Disabling the cue removes it from export without touching its slot.
    apply(
        &path,
        vec![TimelineCommand::ClipEnableSet {
            sequence: sid,
            clip: cue.id,
            enabled: false,
        }],
    );
    assert!(!export_srt(&path).contains("きょうは晴れ"));
    assert_eq!(
        query(&path, sid).clips[0].clip.timeline_range,
        range(t(1, 1), t(2, 1))
    );
    apply(
        &path,
        vec![TimelineCommand::ClipEnableSet {
            sequence: sid,
            clip: cue.id,
            enabled: true,
        }],
    );
    assert!(export_srt(&path).contains("きょうは晴れ"));
}
