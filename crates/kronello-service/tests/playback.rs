use kronello_audio::{AudioSources, AudioTarget, DocumentAudioPlan};
use kronello_model::*;
use kronello_service::{
    BackendSelection, CreateRequest, ImportRequest, PreparedAudio, ProjectRequest, Request,
    Response, ResultData, Service,
};
use kronello_time::{FrameRate, SampleRate, Time, TimeMap, TimeRange};

fn fixture() -> (Project, SequenceId) {
    let mut project: Project =
        serde_json::from_str(include_str!("../../../examples/ffi-preview.project.json")).unwrap();
    let sequence = SequenceId::new();
    project.sequences.push(DocumentObject::Known(Sequence {
        id: sequence,
        extent: DesignExtent::new(64.0, 32.0).unwrap(),
        frame_rate: FrameRate::new(30000, 1001).unwrap(),
        audio_rate: SampleRate::HZ_48000,
        working_space: ColorSpace::LinearRec709,
        tracks: vec![Track {
            state: None,
            id: TrackId::new(),
            kind: TrackKind::Audio,
            clips: vec![Clip {
                id: ClipId::new(),
                source_ref: SourceRef::Generator {
                    generator: "kronello.audio.tone440".into(),
                    version: 1,
                    color: Color::from_srgb8([0; 3], None),
                },
                timeline_range: TimeRange::new(Time::ZERO, Time::new(60, 1).unwrap()).unwrap(),
                source_in: Time::new(1, 48000).unwrap(),
                time_map: TimeMap::linear(Time::ZERO, Time::new(3, 2).unwrap()).unwrap(),
                enabled: true,
                audio_retime: AudioRetimePolicy::ResampleV1,
                reverse_sampling: None,
                volume: None,
                links: vec![],
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
    (project, sequence)
}
fn revision(response: Response) -> String {
    let Response::Success {
        result: ResultData::Project(info),
    } = response
    else {
        panic!("{response:?}")
    };
    info.revision
}
#[test]
fn prepared_blocks_match_export_evaluator_bits_and_remain_revision_pinned() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audio.kronello");
    let (mut project, sequence) = fixture();
    let service = Service::new(BackendSelection::CpuReference);
    let rev = revision(service.execute(Request::ProjectCreate(CreateRequest {
        project: path.clone(),
        document: project.clone(),
        plan_hash: None,
        idempotency_key: None,
    })));
    let prepared = PreparedAudio::prepare(
        &path,
        kronello_service::RenderTarget::Sequence { sequence },
        &rev,
    )
    .unwrap();
    assert!(prepared.has_audio());
    assert_eq!(prepared.revision(), rev);
    let plan =
        DocumentAudioPlan::compile_version(&project, AudioTarget::Sequence(sequence), 2).unwrap();
    let expected = plan
        .mix(
            &AudioSources::new(),
            TimeRange::new(Time::ZERO, Time::new(12288, 48000).unwrap()).unwrap(),
        )
        .unwrap();
    // Arbitrary render order, exact sample grid, before export's PCM24 quantisation.
    for start in [8192, 0, 4096] {
        let mut block = vec![0.0; 8192];
        prepared.render_block(start, &mut block).unwrap();
        let expected: Vec<_> = expected.buffer().frames()[start as usize..start as usize + 4096]
            .iter()
            .flatten()
            .map(|f| f.to_bits())
            .collect();
        assert_eq!(
            block.iter().map(|f| f.to_bits()).collect::<Vec<_>>(),
            expected
        );
    }
    let DocumentObject::Known(s) = &mut project.sequences[0] else {
        panic!()
    };
    s.tracks[0].clips[0].source_ref = SourceRef::Generator {
        generator: "kronello.audio.silence".into(),
        version: 1,
        color: Color::from_srgb8([0; 3], None),
    };
    let next = revision(service.execute(Request::ProjectImport(ImportRequest {
        project: path.clone(),
        base_revision: rev.clone(),
        document: project,
        plan_hash: None,
        idempotency_key: None,
    })));
    assert_eq!(
        PreparedAudio::prepare(
            &path,
            kronello_service::RenderTarget::Sequence { sequence },
            &rev
        )
        .err()
        .unwrap()
        .code,
        "REVISION_CONFLICT"
    );
    let updated = PreparedAudio::prepare(
        &path,
        kronello_service::RenderTarget::Sequence { sequence },
        &next,
    )
    .unwrap();
    let mut old = [0.0; 256];
    let mut new = [1.0; 256];
    prepared.render_block(1, &mut old).unwrap();
    updated.render_block(1, &mut new).unwrap();
    assert!(old.iter().any(|f| *f != 0.0));
    assert!(new.iter().all(|f| *f == 0.0));
    std::fs::remove_file(&path).unwrap();
    let mut repeated = [0.0; 256];
    prepared.render_block(1, &mut repeated).unwrap();
    assert_eq!(old.map(f32::to_bits), repeated.map(f32::to_bits));
}
#[test]
fn metered_render_reports_per_track_and_master_levels() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audio.kronello");
    let (project, sequence) = fixture();
    let service = Service::new(BackendSelection::CpuReference);
    let rev = revision(service.execute(Request::ProjectCreate(CreateRequest {
        project: path.clone(),
        document: project,
        plan_hash: None,
        idempotency_key: None,
    })));
    let prepared = PreparedAudio::prepare(
        &path,
        kronello_service::RenderTarget::Sequence { sequence },
        &rev,
    )
    .unwrap();
    let mut block = vec![0.0; 8192];
    let meters = prepared.render_block_metered(0, &mut block).unwrap();
    // The shared render path: output matches render_block exactly.
    let mut plain = vec![0.0; 8192];
    prepared.render_block(0, &mut plain).unwrap();
    assert_eq!(block, plain);
    // One tone440 track: steady-state 0.25 amplitude reads back as the
    // per-track and master levels for the same rendered block.
    assert_eq!(meters.tracks.len(), 1);
    for channel in 0..2 {
        assert!(meters.tracks[0].peak[channel] > 0.24);
        assert!(meters.tracks[0].peak[channel] <= 0.25 + 1e-6);
        assert!((meters.tracks[0].rms[channel] - 0.25 / 2_f32.sqrt()).abs() < 0.01);
        assert_eq!(meters.master_peak[channel], meters.tracks[0].peak[channel]);
        assert_eq!(meters.master_rms[channel], meters.tracks[0].rms[channel]);
    }
    for mut output in [vec![], vec![1.0], vec![1.0; 8194]] {
        assert!(prepared.render_block_metered(0, &mut output).is_err());
    }
}
#[test]
fn playback_bounds_errors_and_absent_audio_are_explicit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audio.kronello");
    let (mut project, sequence) = fixture();
    let composition = match &project.compositions[0] {
        DocumentObject::Known(c) => c.id,
        _ => panic!(),
    };
    let DocumentObject::Known(s) = &mut project.sequences[0] else {
        panic!()
    };
    s.tracks[0].clips[0].source_ref = SourceRef::Composition { composition };
    s.tracks[0].clips[0].timeline_range = TimeRange::new(Time::ZERO, Time::ONE).unwrap();
    s.tracks[0].clips[0].source_in = Time::ZERO;
    s.tracks[0].clips[0].time_map = TimeMap::linear(Time::ZERO, Time::ONE).unwrap();
    let service = Service::new(BackendSelection::CpuReference);
    let rev = revision(service.execute(Request::ProjectCreate(CreateRequest {
        project: path.clone(),
        document: project,
        plan_hash: None,
        idempotency_key: None,
    })));
    let prepared = PreparedAudio::prepare(&path, composition.into(), &rev).unwrap();
    assert!(!prepared.has_audio());
    let empty_inheritance = PreparedAudio::prepare(
        &path,
        kronello_service::RenderTarget::Sequence { sequence },
        &rev,
    )
    .unwrap();
    assert!(!empty_inheritance.has_audio());
    for mut output in [vec![], vec![1.0], vec![1.0; 8194]] {
        assert_eq!(
            prepared.render_block(0, &mut output).unwrap_err().code,
            "INVALID_AUDIO_INPUT"
        );
        assert!(output.iter().all(|f| *f == 1.0));
    }
    assert_eq!(
        prepared.render_block(-1, &mut [1.0; 2]).unwrap_err().code,
        "INVALID_AUDIO_INPUT"
    );
    assert_eq!(
        prepared
            .render_block(i64::MAX, &mut [1.0; 2])
            .unwrap_err()
            .code,
        "TIME_ERROR"
    );
    let mut silence = [1.0; 2];
    prepared.render_block(0, &mut silence).unwrap();
    assert_eq!(silence, [0.0; 2]);
    assert!(matches!(
        service.execute(Request::ProjectInfo(ProjectRequest { project: path })),
        Response::Success { .. }
    ));
}
