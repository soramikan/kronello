//! FX-004 service contracts (ADR-0114): `clip_masks_set` edits a clip's mask
//! stack through the shared timeline command with typed validation, undo,
//! idempotency, and split-time property remapping.
use kronello_model::*;
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
fn scalar(v: f64) -> Value {
    Value::Scalar(FiniteF64::new(v).unwrap())
}
fn property(key: &str, value: Value) -> Property {
    let r = SchemaRegistry::with_builtin();
    Property::new(
        PropertyId::new(),
        DescriptorRef::new(r.lookup(&SchemaKey::new(key).unwrap()).unwrap()),
        PropertySource::Constant(value),
        vec![],
        &r,
    )
    .unwrap()
}
fn rect_path(max_x: f64, max_y: f64) -> Value {
    let f = |v: f64| FiniteF64::new(v).unwrap();
    Value::Path(kronello_model::Path {
        segments: vec![
            PathSegment::MoveTo([f(0.0), f(0.0)]),
            PathSegment::LineTo([f(max_x), f(0.0)]),
            PathSegment::LineTo([f(max_x), f(max_y)]),
            PathSegment::LineTo([f(0.0), f(max_y)]),
            PathSegment::Close,
        ],
    })
}
/// A valid left-half Add mask plus its four backing clip properties.
fn mask_parts() -> (Mask, Vec<Property>) {
    let path = property(MASK_PATH_KEY, rect_path(1.0, 2.0));
    let feather = property(MASK_FEATHER_KEY, scalar(0.0));
    let expansion = property(MASK_EXPANSION_KEY, scalar(0.0));
    let opacity = property(MASK_OPACITY_KEY, scalar(1.0));
    (
        Mask {
            id: MaskId::new(),
            path: path.id(),
            mode: MaskMode::Add,
            feather: feather.id(),
            expansion: expansion.id(),
            opacity: opacity.id(),
            invert: false,
            closed: true,
        },
        vec![path, feather, expansion, opacity],
    )
}
fn clip() -> Clip {
    Clip {
        id: ClipId::new(),
        source_ref: SourceRef::Generator {
            generator: SOLID_GENERATOR_ID.into(),
            version: 1,
            color: Color::from_srgb8([200, 40, 10], None),
        },
        timeline_range: range(Time::ZERO, t(4, 1)),
        source_in: Time::ZERO,
        time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
        audio_retime: AudioRetimePolicy::Reject,
        reverse_sampling: None,
        volume: None,
        links: vec![],
        enabled: true,
        effects: vec![],
        masks: vec![],
        markers: vec![],
        properties: vec![],
    }
}
fn sequence(track: Track) -> Sequence {
    Sequence {
        id: SequenceId::new(),
        extent: DesignExtent::new(2.0, 2.0).unwrap(),
        frame_rate: FrameRate::new(24, 1).unwrap(),
        audio_rate: SampleRate::HZ_48000,
        working_space: ColorSpace::LinearRec709,
        tracks: vec![track],
        transitions: vec![],
        markers: vec![],
        work_area: None,
        targets: None,
    }
}
fn video(clips: Vec<Clip>) -> Track {
    Track {
        state: None,
        id: TrackId::new(),
        kind: TrackKind::Video,
        clips,
    }
}
fn service() -> Service<'static> {
    Service::new(BackendSelection::CpuReference)
}
fn setup(p: Project) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fx004.kronello");
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
fn apply_request(
    path: &Path,
    command: TimelineCommand,
    key: &str,
    session: Uuid,
) -> EditApplyRequest {
    let commands = vec![EditCommand::Timeline(Box::new(command))];
    let revision = export(path).revision;
    let ResultData::Plan(plan) = service()
        .dispatch(Request::EditPlan(PlanRequest {
            project: path.into(),
            base_revision: revision.clone(),
            commands: commands.clone(),
        }))
        .unwrap()
    else {
        panic!()
    };
    EditApplyRequest {
        project: path.into(),
        base_revision: revision,
        commands,
        plan_hash: plan.plan_hash,
        idempotency_key: key.into(),
        session_id: session,
    }
}
fn apply(path: &Path, command: TimelineCommand, key: &str) -> kronello_store::Event {
    let ResultData::Edit(event) = service()
        .dispatch(Request::EditApply(apply_request(
            path,
            command,
            key,
            Uuid::new_v4(),
        )))
        .unwrap()
    else {
        panic!()
    };
    event
}
fn undo(path: &Path, event: kronello_store::Event) {
    service()
        .dispatch(Request::EditUndo(UndoRequest {
            project: path.into(),
            base_revision: export(path).revision,
            event_id: event.id,
            session_id: Uuid::new_v4(),
            idempotency_key: Uuid::new_v4().to_string(),
        }))
        .unwrap();
}
fn reject(path: &Path, command: TimelineCommand, code: &str) {
    let before = export(path);
    let error = service()
        .dispatch(Request::EditPlan(PlanRequest {
            project: path.into(),
            base_revision: before.revision.clone(),
            commands: vec![EditCommand::Timeline(Box::new(command))],
        }))
        .unwrap_err();
    assert_eq!(error.code, code, "{error:?}");
    assert_eq!(export(path).document, before.document);
    assert_eq!(export(path).revision, before.revision);
}
fn sequence_clip(path: &Path, sequence: SequenceId, clip: ClipId) -> Clip {
    let e = export(path);
    let DocumentObject::Known(s) = e
        .document
        .sequences
        .iter()
        .find(|s| matches!(s, DocumentObject::Known(s) if s.id == sequence))
        .unwrap()
    else {
        panic!()
    };
    s.tracks
        .iter()
        .flat_map(|t| &t.clips)
        .find(|c| c.id == clip)
        .expect("clip")
        .clone()
}
fn project(seq: Sequence) -> Project {
    let mut p = Project::default();
    p.sequences.push(DocumentObject::Known(seq));
    p
}

#[test]
fn clip_masks_set_roundtrips_undo_and_idempotent_replay() {
    let clip = clip();
    let clip_id = clip.id;
    let s = sequence(video(vec![clip]));
    let sequence_id = s.id;
    let (_dir, path) = setup(project(s));
    let (mask, properties) = mask_parts();
    let mask_id = mask.id;
    let request = apply_request(
        &path,
        TimelineCommand::ClipMasksSet {
            sequence: sequence_id,
            clip: clip_id,
            masks: vec![mask],
            properties,
        },
        "set-masks",
        Uuid::new_v4(),
    );
    let ResultData::Edit(event) = service()
        .dispatch(Request::EditApply(request.clone()))
        .unwrap()
    else {
        panic!()
    };
    let stored = sequence_clip(&path, sequence_id, clip_id);
    assert_eq!(stored.masks.len(), 1);
    assert_eq!(stored.masks[0].id, mask_id);
    assert_eq!(stored.properties.len(), 4);
    // Replaying the identical request returns the recorded event without
    // mutating the document again.
    let before = export(&path);
    let ResultData::Edit(replayed) = service().dispatch(Request::EditApply(request)).unwrap()
    else {
        panic!()
    };
    assert_eq!(replayed.id, event.id);
    assert_eq!(export(&path).revision, before.revision);
    undo(&path, event);
    assert!(sequence_clip(&path, sequence_id, clip_id).masks.is_empty());
}

#[test]
fn clip_masks_set_rejects_orphaned_references_and_nonvideo_tracks() {
    let base = clip();
    let clip_id = base.id;
    let s = sequence(video(vec![base]));
    let sequence_id = s.id;
    let (_dir, path) = setup(project(s));
    let (mask, properties) = mask_parts();
    // A mask referencing a property the command does not carry fails typed.
    reject(
        &path,
        TimelineCommand::ClipMasksSet {
            sequence: sequence_id,
            clip: clip_id,
            masks: vec![mask.clone()],
            properties: properties[1..].to_vec(),
        },
        "MASK_INVALID_PROPERTY",
    );
    // A path property carrying the feather descriptor fails typed.
    let mut swapped = mask.clone();
    swapped.path = properties[1].id();
    reject(
        &path,
        TimelineCommand::ClipMasksSet {
            sequence: sequence_id,
            clip: clip_id,
            masks: vec![swapped],
            properties: properties.clone(),
        },
        "MASK_INVALID_PROPERTY",
    );
    // Masks on an audio track are rejected before the edit lands.
    let mut audio_clip = clip();
    audio_clip.id = clip_id;
    let s = sequence(Track {
        state: None,
        id: TrackId::new(),
        kind: TrackKind::Audio,
        clips: vec![audio_clip],
    });
    let audio_sequence = s.id;
    let (_dir2, path2) = setup(project(s));
    let (mask, properties) = mask_parts();
    reject(
        &path2,
        TimelineCommand::ClipMasksSet {
            sequence: audio_sequence,
            clip: clip_id,
            masks: vec![mask],
            properties,
        },
        "INVALID_EDIT",
    );
}

#[test]
fn clip_split_remaps_mask_properties_into_the_right_piece() {
    let clip = clip();
    let clip_id = clip.id;
    let (mask, properties) = mask_parts();
    let mut masked = clip.clone();
    masked.masks = vec![mask.clone()];
    masked.properties = properties;
    let s = sequence(video(vec![masked]));
    let sequence_id = s.id;
    let (_dir, path) = setup(project(s));
    let right = ClipId::new();
    apply(
        &path,
        TimelineCommand::ClipSplit {
            sequence: sequence_id,
            clip: clip_id,
            time: t(2, 1),
            right_clip: right,
        },
        "split",
    );
    let e = export(&path);
    let DocumentObject::Known(s) = &e.document.sequences[0] else {
        panic!()
    };
    let clips: Vec<_> = s.tracks[0].clips.iter().collect();
    assert_eq!(clips.len(), 2);
    for c in &clips {
        assert_eq!(c.masks.len(), 1, "each piece keeps the mask");
        assert_eq!(c.properties.len(), 4, "each piece owns mask properties");
        for p in &c.properties {
            assert!(
                c.properties.iter().any(|q| q.id() == p.id()),
                "mask properties live on the piece"
            );
        }
        for reference in [
            c.masks[0].path,
            c.masks[0].feather,
            c.masks[0].expansion,
            c.masks[0].opacity,
        ] {
            assert!(
                c.properties.iter().any(|p| p.id() == reference),
                "mask references resolve inside the piece"
            );
        }
    }
    // The right piece is a fresh property space: ids differ from the left.
    let left_path = clips[0].masks[0].path;
    let right_path = clips[1].masks[0].path;
    assert_ne!(left_path, right_path);
    assert_ne!(clips[0].masks[0].id, clips[1].masks[0].id);
}
