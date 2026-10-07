//! AUDIO-008 service coverage: BS.1770-4 loudness queries and loudness
//! normalization through the shared plan + edit pipeline (ADR-0117).
use kronello_model::*;
use kronello_service::{
    AudioLoudnessInput, AudioLoudnessRequest, AudioNormalizeRequest, BackendSelection,
    CreateRequest, Request, Response, ResultData, Service, UndoRequest,
};
use kronello_time::{FrameRate, SampleRate, Time, TimeMap, TimeRange};
use uuid::Uuid;

fn t(n: i64, d: i64) -> Time {
    Time::new(n, d).unwrap()
}
fn fixture() -> (Project, SequenceId, TrackId, ClipId) {
    let project: Project =
        serde_json::from_str(include_str!("../../../examples/ffi-preview.project.json")).unwrap();
    let (sequence, track, clip) = (SequenceId::new(), TrackId::new(), ClipId::new());
    let mut project = project;
    project.sequences.push(DocumentObject::Known(Sequence {
        id: sequence,
        extent: DesignExtent::new(64.0, 32.0).unwrap(),
        frame_rate: FrameRate::new(30000, 1001).unwrap(),
        audio_rate: SampleRate::HZ_48000,
        working_space: ColorSpace::LinearRec709,
        tracks: vec![Track {
            state: None,
            id: track,
            kind: TrackKind::Audio,
            clips: vec![Clip {
                id: clip,
                source_ref: SourceRef::Generator {
                    generator: "kronello.audio.tone440".into(),
                    version: 1,
                    color: Color::from_srgb8([0; 3], None),
                },
                timeline_range: TimeRange::new(Time::ZERO, t(4, 1)).unwrap(),
                source_in: Time::ZERO,
                time_map: TimeMap::linear(Time::ZERO, Time::ONE).unwrap(),
                audio_retime: AudioRetimePolicy::ResampleV1,
                reverse_sampling: None,
                volume: None,
                links: vec![],
                enabled: true,
                properties: vec![],
                effects: vec![],
                markers: vec![],
            }],
        }],
        transitions: vec![],
        markers: vec![],
        work_area: None,
        targets: None,
    }));
    (project, sequence, track, clip)
}
fn service() -> Service<'static> {
    Service::new(BackendSelection::CpuReference)
}
fn create(
    service: &Service<'_>,
    project: Project,
) -> (tempfile::TempDir, std::path::PathBuf, String) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("loudness.kronello");
    let Response::Success {
        result: ResultData::Project(info),
    } = service.execute(Request::ProjectCreate(CreateRequest {
        project: path.clone(),
        document: project,
        plan_hash: None,
        idempotency_key: None,
    }))
    else {
        panic!("project.create failed")
    };
    (dir, path, info.revision)
}
fn loudness(
    service: &Service<'_>,
    path: &std::path::Path,
    revision: &str,
    input: AudioLoudnessInput,
) -> Result<kronello_service::AudioLoudnessResult, kronello_service::ServiceError> {
    let Response::Success {
        result: ResultData::Loudness(report),
    } = service.execute(Request::AudioLoudness(AudioLoudnessRequest {
        project: path.to_path_buf(),
        base_revision: revision.into(),
        input,
    }))
    else {
        panic!("audio.loudness failed")
    };
    Ok(report)
}
fn err_code(response: Response) -> String {
    let Response::Error { error } = response else {
        panic!("expected error, got {response:?}")
    };
    error.code
}
fn loudness_err(
    service: &Service<'_>,
    path: &std::path::Path,
    revision: &str,
    input: AudioLoudnessInput,
) -> String {
    err_code(
        service.execute(Request::AudioLoudness(AudioLoudnessRequest {
            project: path.to_path_buf(),
            base_revision: revision.into(),
            input,
        })),
    )
}
fn sequence_clip_effects(
    service: &Service<'_>,
    path: &std::path::Path,
    sequence: SequenceId,
) -> usize {
    let Response::Success {
        result: ResultData::Timeline(query),
    } = service.execute(Request::SequenceQuery(
        kronello_service::SequenceQueryRequest {
            project: path.to_path_buf(),
            sequence,
        },
    ))
    else {
        panic!("sequence.query failed")
    };
    query.sequence.tracks[0].clips[0].effects.len()
}
#[test]
fn loudness_reports_integrated_and_typed_errors() {
    let (project, sequence, _, clip) = fixture();
    let service = service();
    let (_dir, path, revision) = create(&service, project);
    // 0.25-amplitude stereo 440 Hz: unweighted -12.73 LUFS plus ~0.7 dB K weighting.
    let report = loudness(
        &service,
        &path,
        &revision,
        AudioLoudnessInput::Sequence {
            sequence,
            range: None,
        },
    )
    .unwrap();
    let integrated = report.integrated_lufs.expect("integrated");
    assert!(
        (-13.5..-11.5).contains(&integrated),
        "integrated {integrated}"
    );
    assert!(report.momentary_lufs.is_some() && report.short_term_lufs.is_some());
    assert!(report.true_peak_dbtp.unwrap() > -13.0);
    assert_eq!(report.frames, 192_000);
    let clip_report = loudness(
        &service,
        &path,
        &revision,
        AudioLoudnessInput::Clip { sequence, clip },
    )
    .unwrap();
    assert!((clip_report.integrated_lufs.unwrap() - integrated).abs() < 0.2);
    assert_eq!(
        loudness_err(
            &service,
            &path,
            "0",
            AudioLoudnessInput::Sequence {
                sequence,
                range: None
            }
        ),
        "REVISION_CONFLICT"
    );
    for input in [
        AudioLoudnessInput::Sequence {
            sequence: SequenceId::new(),
            range: None,
        },
        AudioLoudnessInput::Clip {
            sequence,
            clip: ClipId::new(),
        },
        AudioLoudnessInput::Asset {
            asset: AssetId::new(),
            stream_index: 0,
            range: None,
        },
    ] {
        assert_eq!(
            loudness_err(&service, &path, &revision, input),
            "ASSET_MISSING"
        );
    }
}
#[test]
fn normalize_appends_gain_then_undo_restores() {
    let (project, sequence, _, clip) = fixture();
    let service = service();
    let (_dir, path, revision) = create(&service, project);
    let session = Uuid::new_v4();
    let Response::Success {
        result: ResultData::Normalize(result),
    } = service.execute(Request::AudioNormalize(AudioNormalizeRequest {
        project: path.clone(),
        base_revision: revision.clone(),
        session_id: session,
        idempotency_key: "normalize-1".into(),
        sequence,
        clip,
        target_lufs: -20.0,
    }))
    else {
        panic!("audio.normalize failed")
    };
    assert!(result.gain_db < 0.0 && result.gain_linear < 1.0);
    assert!((-13.5..-11.5).contains(&result.measured_lufs));
    assert_eq!(sequence_clip_effects(&service, &path, sequence), 1);
    // The appended gain brings the measured integrated loudness to the target.
    let after = loudness(
        &service,
        &path,
        "2",
        AudioLoudnessInput::Clip { sequence, clip },
    )
    .unwrap();
    assert!(
        (after.integrated_lufs.unwrap() - -20.0).abs() < 0.3,
        "normalized {}",
        after.integrated_lufs.unwrap()
    );
    let Response::Success { .. } = service.execute(Request::EditUndo(UndoRequest {
        project: path.clone(),
        base_revision: "2".into(),
        session_id: session,
        idempotency_key: "undo-normalize".into(),
        event_id: result.event.id,
    })) else {
        panic!("edit.undo failed")
    };
    assert_eq!(sequence_clip_effects(&service, &path, sequence), 0);
    let restored = loudness(
        &service,
        &path,
        "3",
        AudioLoudnessInput::Clip { sequence, clip },
    )
    .unwrap();
    assert!(
        (restored.integrated_lufs.unwrap() - result.measured_lufs).abs() < 0.2,
        "restored {}",
        restored.integrated_lufs.unwrap()
    );
}
#[test]
fn normalize_and_loudness_fail_typed() {
    let (project, sequence, track, clip) = fixture();
    let service = service();
    let (_dir, path, revision) = create(&service, project.clone());
    let session = Uuid::new_v4();
    for target in [5.0, f64::NAN, -80.0] {
        assert_eq!(
            err_code(
                service.execute(Request::AudioNormalize(AudioNormalizeRequest {
                    project: path.clone(),
                    base_revision: revision.clone(),
                    session_id: session,
                    idempotency_key: "normalize-bad".into(),
                    sequence,
                    clip,
                    target_lufs: target,
                }))
            ),
            "INVALID_REQUEST",
            "{target}"
        );
    }
    // A clip that produces only silence cannot be normalized.
    let mut silent = project.clone();
    let DocumentObject::Known(s) = &mut silent.sequences[0] else {
        panic!()
    };
    s.tracks[0].clips[0].source_ref = SourceRef::Generator {
        generator: "kronello.audio.silence".into(),
        version: 1,
        color: Color::from_srgb8([0; 3], None),
    };
    let (_dir2, path2, revision2) = create(&service, silent);
    let code = err_code(
        service.execute(Request::AudioNormalize(AudioNormalizeRequest {
            project: path2.clone(),
            base_revision: revision2.clone(),
            session_id: session,
            idempotency_key: "normalize-silent".into(),
            sequence,
            clip,
            target_lufs: -20.0,
        })),
    );
    assert_eq!(code, "INVALID_AUDIO_INPUT");
    // Clips outside audio tracks are not loudness/normalize targets.
    assert_eq!(
        err_code(
            service.execute(Request::AudioNormalize(AudioNormalizeRequest {
                project: path.clone(),
                base_revision: revision.clone(),
                session_id: session,
                idempotency_key: "normalize-missing".into(),
                sequence,
                clip: ClipId::new(),
                target_lufs: -20.0,
            }))
        ),
        "ASSET_MISSING"
    );
    let _ = track;
}
