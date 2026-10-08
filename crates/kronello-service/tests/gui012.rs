//! GUI-012 (ADR-0138) service coverage: the shared `clip_set_pan` timeline
//! command and the `track` loudness input used by the mixer strips.
use kronello_model::*;
use kronello_service::{
    AudioLoudnessInput, AudioLoudnessRequest, BackendSelection, CreateRequest, EditApplyRequest,
    EditCommand, PlanRequest, ProjectRequest, Request, Response, ResultData, Service,
    TimelineCommand, UndoRequest,
};
use kronello_time::{FrameRate, SampleRate, Time, TimeMap, TimeRange};
use std::path::{Path, PathBuf};
use uuid::Uuid;

fn t(n: i64, d: i64) -> Time {
    Time::new(n, d).unwrap()
}
fn scalar(v: f64) -> Value {
    Value::Scalar(FiniteF64::new(v).unwrap())
}
fn property(key: &str, value: f64) -> Property {
    let registry = SchemaRegistry::with_builtin();
    Property::new(
        PropertyId::new(),
        DescriptorRef::new(registry.lookup(&SchemaKey::new(key).unwrap()).unwrap()),
        PropertySource::Constant(scalar(value)),
        vec![],
        &registry,
    )
    .unwrap()
}
fn clip(generator: &str, range: TimeRange) -> Clip {
    Clip {
        id: ClipId::new(),
        source_ref: SourceRef::Generator {
            generator: generator.into(),
            version: 1,
            color: Color::from_srgb8([0; 3], None),
        },
        timeline_range: range,
        source_in: Time::ZERO,
        time_map: TimeMap::linear(Time::ZERO, Time::ONE).unwrap(),
        enabled: true,
        audio_retime: AudioRetimePolicy::ResampleV1,
        reverse_sampling: None,
        volume: None,
        pan: None,
        links: vec![],
        effects: vec![],
        masks: vec![],
        properties: vec![],
        markers: vec![],
    }
}
fn fixture() -> (Project, SequenceId, TrackId, TrackId) {
    let project: Project =
        serde_json::from_str(include_str!("../../../examples/ffi-preview.project.json")).unwrap();
    let (sequence, tone_track, silent_track) = (SequenceId::new(), TrackId::new(), TrackId::new());
    let mut project = project;
    project.sequences.push(DocumentObject::Known(Sequence {
        id: sequence,
        extent: DesignExtent::new(64.0, 32.0).unwrap(),
        frame_rate: FrameRate::new(30000, 1001).unwrap(),
        audio_rate: SampleRate::HZ_48000,
        working_space: ColorSpace::LinearRec709,
        tracks: vec![
            Track {
                state: None,
                id: tone_track,
                kind: TrackKind::Audio,
                clips: vec![clip(
                    "kronello.audio.tone440",
                    TimeRange::new(Time::ZERO, t(4, 1)).unwrap(),
                )],
            },
            Track {
                state: None,
                id: silent_track,
                kind: TrackKind::Audio,
                clips: vec![clip(
                    "kronello.audio.silence",
                    TimeRange::new(Time::ZERO, t(4, 1)).unwrap(),
                )],
            },
        ],
        transitions: vec![],
        markers: vec![],
        work_area: None,
        targets: None,
    }));
    (project, sequence, tone_track, silent_track)
}
fn engine() -> Service<'static> {
    Service::new(BackendSelection::CpuReference)
}
fn setup(p: Project) -> (tempfile::TempDir, PathBuf, String) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gui012.kronello");
    let ResultData::Project(info) = engine()
        .dispatch(Request::ProjectCreate(CreateRequest {
            project: path.clone(),
            document: p,
            plan_hash: None,
            idempotency_key: None,
        }))
        .unwrap()
    else {
        panic!("project.create failed")
    };
    (dir, path, info.revision)
}
fn export(path: &Path) -> kronello_service::ExportResult {
    let ResultData::Export(value) = engine()
        .dispatch(Request::ProjectExport(ProjectRequest {
            project: path.into(),
        }))
        .unwrap()
    else {
        panic!()
    };
    *value
}
fn apply(path: &Path, command: TimelineCommand, key: &str) -> kronello_store::Event {
    let commands = vec![EditCommand::Timeline(Box::new(command))];
    let revision = export(path).revision;
    let ResultData::Plan(plan) = engine()
        .dispatch(Request::EditPlan(PlanRequest {
            project: path.into(),
            base_revision: revision.clone(),
            commands: commands.clone(),
        }))
        .unwrap()
    else {
        panic!()
    };
    let ResultData::Edit(event) = engine()
        .dispatch(Request::EditApply(EditApplyRequest {
            project: path.into(),
            base_revision: revision,
            commands,
            plan_hash: plan.plan_hash,
            idempotency_key: key.into(),
            session_id: Uuid::new_v4(),
        }))
        .unwrap()
    else {
        panic!()
    };
    event
}
fn reject(path: &Path, command: TimelineCommand, code: &str) {
    let error = engine()
        .dispatch(Request::EditPlan(PlanRequest {
            project: path.into(),
            base_revision: export(path).revision,
            commands: vec![EditCommand::Timeline(Box::new(command))],
        }))
        .unwrap_err();
    assert_eq!(error.code, code, "{error:?}");
}
fn loudness(
    path: &Path,
    revision: &str,
    input: AudioLoudnessInput,
) -> Result<kronello_service::AudioLoudnessResult, String> {
    match engine().execute(Request::AudioLoudness(AudioLoudnessRequest {
        project: path.into(),
        base_revision: revision.into(),
        input,
    })) {
        Response::Success {
            result: ResultData::Loudness(report),
        } => Ok(report),
        Response::Error { error } => Err(error.code),
        other => panic!("unexpected {other:?}"),
    }
}
fn clip_at(document: &Project, sequence: SequenceId, track: TrackId) -> Clip {
    document
        .sequences
        .iter()
        .find_map(|s| match s {
            DocumentObject::Known(s) if s.id == sequence => Some(s),
            _ => None,
        })
        .unwrap()
        .tracks
        .iter()
        .find(|t| t.id == track)
        .unwrap()
        .clips[0]
        .clone()
}

#[test]
fn clip_set_pan_is_revisioned_undoable_and_validated() {
    let (project, sequence, track, _) = fixture();
    let clip = clip_at(&project, sequence, track).id;
    let (_dir, path, _) = setup(project);
    let pan = property("kronello.audio.pan", -0.5);
    apply(
        &path,
        TimelineCommand::ClipSetPan {
            sequence,
            clip,
            pan: Some(pan.clone()),
        },
        "pan-set",
    );
    let document = export(&path).document;
    assert_eq!(
        clip_at(&document, sequence, track).pan.as_deref(),
        Some(&pan)
    );
    // Clearing restores the absent-pan (centered) contract.
    let clear = apply(
        &path,
        TimelineCommand::ClipSetPan {
            sequence,
            clip,
            pan: None,
        },
        "pan-clear",
    );
    let document = export(&path).document;
    assert!(clip_at(&document, sequence, track).pan.is_none());
    // Undo of the most recent event restores the authored pan value.
    engine()
        .dispatch(Request::EditUndo(UndoRequest {
            project: path.clone(),
            base_revision: export(&path).revision,
            event_id: clear.id,
            session_id: Uuid::new_v4(),
            idempotency_key: Uuid::new_v4().to_string(),
        }))
        .unwrap();
    let document = export(&path).document;
    assert_eq!(
        clip_at(&document, sequence, track).pan.as_deref(),
        Some(&pan)
    );
    // The wrong descriptor key and missing clips are typed rejections.
    reject(
        &path,
        TimelineCommand::ClipSetPan {
            sequence,
            clip,
            pan: Some(property("kronello.audio.volume", 0.5)),
        },
        "INVALID_AUDIO_INPUT",
    );
    reject(
        &path,
        TimelineCommand::ClipSetPan {
            sequence,
            clip: ClipId::new(),
            pan: Some(property("kronello.audio.pan", 0.0)),
        },
        "SOURCE_MISSING",
    );
}

#[test]
fn track_loudness_isolates_one_track() {
    let (project, sequence, tone_track, silent_track) = fixture();
    let (_dir, path, revision) = setup(project);
    let sequence_report = loudness(
        &path,
        &revision,
        AudioLoudnessInput::Sequence {
            sequence,
            range: None,
        },
    )
    .unwrap();
    let track_report = loudness(
        &path,
        &revision,
        AudioLoudnessInput::Track {
            sequence,
            track: tone_track,
        },
    )
    .unwrap();
    // One audible track plus silence: the track measurement equals the mix.
    assert_eq!(
        track_report.integrated_lufs,
        sequence_report.integrated_lufs
    );
    assert_eq!(track_report.frames, sequence_report.frames);
    // A silence-only track reports no integrated loudness (never an error).
    let quiet = loudness(
        &path,
        &revision,
        AudioLoudnessInput::Track {
            sequence,
            track: silent_track,
        },
    )
    .unwrap();
    assert!(quiet.integrated_lufs.is_none());
    // Missing and non-audio tracks are typed errors.
    assert_eq!(
        loudness(
            &path,
            &revision,
            AudioLoudnessInput::Track {
                sequence,
                track: TrackId::new(),
            },
        )
        .unwrap_err(),
        "ASSET_MISSING"
    );
    let video_track = TrackId::new();
    let mut project = export(&path).document;
    let s = project
        .sequences
        .iter_mut()
        .find(|s| matches!(s, DocumentObject::Known(s) if s.id == sequence))
        .unwrap();
    let DocumentObject::Known(s) = s else {
        panic!()
    };
    s.tracks.push(Track {
        state: None,
        id: video_track,
        kind: TrackKind::Video,
        clips: vec![],
    });
    let (_dir2, path2, revision2) = setup(project);
    assert_eq!(
        loudness(
            &path2,
            &revision2,
            AudioLoudnessInput::Track {
                sequence,
                track: video_track,
            },
        )
        .unwrap_err(),
        "INVALID_AUDIO_INPUT"
    );
}
