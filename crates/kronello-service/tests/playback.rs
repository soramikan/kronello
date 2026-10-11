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
                masks: vec![],
                markers: vec![],
                pan: None,
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
        None,
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
            &rev,
            None,
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
        None,
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
        None,
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
    let prepared = PreparedAudio::prepare(&path, composition.into(), &rev, None).unwrap();
    assert!(!prepared.has_audio());
    let empty_inheritance = PreparedAudio::prepare(
        &path,
        kronello_service::RenderTarget::Sequence { sequence },
        &rev,
        None,
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
fn param(key: &str, value: Value) -> Property {
    let mut registry = SchemaRegistry::with_builtin();
    for descriptor in effect_descriptors() {
        registry.register(descriptor).unwrap();
    }
    Property::new(
        PropertyId::new(),
        DescriptorRef::new(
            registry
                .lookup(&SchemaKey::new(format!("kronello.effect.{key}")).unwrap())
                .unwrap(),
        ),
        PropertySource::Constant(value),
        vec![],
        &registry,
    )
    .unwrap()
}
fn scalar(v: f64) -> Value {
    Value::Scalar(FiniteF64::new(v).unwrap())
}
fn audio_metadata(path: &std::path::Path) -> Vec<StreamMetadata> {
    kronello_media::MediaRuntime::load()
        .unwrap()
        .probe(path)
        .unwrap()
        .streams
        .into_iter()
        .map(|s| StreamMetadata {
            index: s.index,
            codec: s.codec,
            time_base: s.time_base,
            duration: s.duration,
            start_time: s.start,
            width: s.width,
            height: s.height,
            pixel_format: s.pixel_format,
            color_primaries: s.color_primaries,
            color_transfer: s.color_transfer,
            color_matrix: s.color_matrix,
            color_range: s.color_range,
        })
        .collect()
}
fn pcm_fixture() -> (Project, SequenceId) {
    let (mut project, sequence) = fixture();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/data/sine-48k-stereo.wav")
        .canonicalize()
        .unwrap();
    let asset = Asset {
        id: AssetId::new(),
        kind: AssetKind::Audio,
        content_hash: kronello_media::content_hash(&path).unwrap(),
        streams: audio_metadata(&path),
        locator: AssetLocator {
            relative: None,
            absolute: Some(path.to_string_lossy().into_owned()),
        },
    };
    let DocumentObject::Known(s) = &mut project.sequences[0] else {
        panic!()
    };
    let clip = &mut s.tracks[0].clips[0];
    clip.source_ref = SourceRef::Asset {
        asset: asset.id,
        stream_index: 0,
    };
    clip.timeline_range = TimeRange::new(Time::ZERO, Time::new(1, 10).unwrap()).unwrap();
    clip.source_in = Time::ZERO;
    clip.time_map = TimeMap::linear(Time::ZERO, Time::ONE).unwrap();
    let params = [
        param("threshold_db", scalar(-20.0)),
        param("ratio", scalar(4.0)),
        param("attack_ms", scalar(5.0)),
        param("release_ms", scalar(100.0)),
        param("makeup_db", scalar(0.0)),
    ];
    clip.effects.push(Effect::Known(EffectDefinition {
        effect_id: AUDIO_COMPRESSOR_ID.into(),
        version: 1,
        parameters: EffectParameters::AudioCompressor {
            threshold_db: params[0].id(),
            ratio: params[1].id(),
            attack_ms: params[2].id(),
            release_ms: params[3].id(),
            makeup_db: params[4].id(),
        },
    }));
    clip.properties.extend(params);
    project.assets.push(DocumentObject::Known(asset));
    (project, sequence)
}
fn audio_request(
    path: &std::path::Path,
    sequence: SequenceId,
    revision: &str,
) -> kronello_service::AudioPrepareRequest {
    kronello_service::AudioPrepareRequest {
        project: path.into(),
        target: kronello_service::RenderTarget::Sequence { sequence },
        expected_revision: revision.into(),
        resume_sample: Some(2048),
    }
}
#[test]
fn shared_sources_keep_each_prepared_dsp_independent_across_revisions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shared.kronello");
    let (mut project, sequence) = pcm_fixture();
    let service = Service::new(BackendSelection::CpuReference);
    let rev = revision(service.execute(Request::ProjectCreate(CreateRequest {
        project: path.clone(),
        document: project.clone(),
        plan_hash: None,
        idempotency_key: None,
    })));
    let first = service
        .capture_audio_input(audio_request(&path, sequence, &rev))
        .unwrap()
        .prepare()
        .unwrap();
    let second = service
        .capture_audio_input(audio_request(&path, sequence, &rev))
        .unwrap()
        .prepare()
        .unwrap();
    let fresh = PreparedAudio::prepare(
        &path,
        kronello_service::RenderTarget::Sequence { sequence },
        &rev,
        Some(2048),
    )
    .unwrap();
    if cfg!(unix) {
        assert_eq!(service.audio_source_cache_stats().decoded_sources, 1);
        assert_eq!(service.audio_source_cache_stats().hits, 1);
    }
    for start in [2048, 3072, 0, 1024, 4096, 0] {
        let mut a = [0.0; 2048];
        let mut b = [0.0; 2048];
        let mut c = [0.0; 2048];
        first.render_block(start, &mut a).unwrap();
        // The other session has not followed the first session's cursor.
        second.render_block_metered(0, &mut b).unwrap();
        second.render_block(start, &mut b).unwrap();
        fresh.render_block(start, &mut c).unwrap();
        assert_eq!(a.map(f32::to_bits), b.map(f32::to_bits));
        assert_eq!(a.map(f32::to_bits), c.map(f32::to_bits));
    }
    // A video-only edit changes the snapshot, not verified source PCM.
    let DocumentObject::Known(composition) = &mut project.compositions[0] else {
        panic!()
    };
    composition.design_extent = DesignExtent::new(128.0, 64.0).unwrap();
    let next = revision(service.execute(Request::ProjectImport(ImportRequest {
        project: path.clone(),
        base_revision: rev.clone(),
        document: project.clone(),
        plan_hash: None,
        idempotency_key: None,
    })));
    let third = service
        .capture_audio_input(audio_request(&path, sequence, &next))
        .unwrap()
        .prepare()
        .unwrap();
    assert_eq!(third.revision(), next);
    if cfg!(unix) {
        assert_eq!(service.audio_source_cache_stats().hits, 2);
    }
    assert_eq!(
        service
            .capture_audio_input(audio_request(&path, sequence, &rev))
            .err()
            .unwrap()
            .code,
        "REVISION_CONFLICT"
    );
    // Audio edits compile a new plan; old plans remain immutable.
    let DocumentObject::Known(s) = &mut project.sequences[0] else {
        panic!()
    };
    s.tracks[0].clips[0].enabled = false;
    let silent_rev = revision(service.execute(Request::ProjectImport(ImportRequest {
        project: path.clone(),
        base_revision: next,
        document: project,
        plan_hash: None,
        idempotency_key: None,
    })));
    let silent = service
        .capture_audio_input(audio_request(&path, sequence, &silent_rev))
        .unwrap()
        .prepare()
        .unwrap();
    let mut old = [0.0; 2048];
    let mut new = [1.0; 2048];
    first.render_block(0, &mut old).unwrap();
    silent.render_block(0, &mut new).unwrap();
    assert!(old.iter().any(|sample| *sample != 0.0));
    assert!(new.iter().all(|sample| *sample == 0.0));
}
#[test]
fn native_session_source_cache_outlives_capture_and_stays_off_render_thread() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("native.kronello");
    let (project, sequence) = pcm_fixture();
    let service = Service::new(BackendSelection::CpuReference);
    let rev = revision(service.execute(Request::ProjectCreate(CreateRequest {
        project: path.clone(),
        document: project,
        plan_hash: None,
        idempotency_key: None,
    })));
    let session = kronello_service::ProjectSession::open(&path).unwrap();
    // Native requests construct transient Services, but PCM belongs to the session.
    for index in 0..2 {
        let input = session
            .scope(|| {
                Service::new(BackendSelection::CpuReference)
                    .capture_audio_input(audio_request(&path, sequence, &rev))
            })
            .unwrap();
        let prepared = std::thread::spawn(move || input.prepare())
            .join()
            .unwrap()
            .unwrap();
        assert_eq!(prepared.revision(), rev);
        let mut output = [0.0; 2048];
        prepared.render_block(2048, &mut output).unwrap();
        assert!(output.iter().any(|sample| *sample != 0.0));
        if cfg!(unix) {
            assert_eq!(session.audio_source_cache_stats().hits, index);
        }
    }
    let input = session
        .scope(|| service.capture_audio_input(audio_request(&path, sequence, &rev)))
        .unwrap();
    drop(session);
    std::fs::remove_file(&path).unwrap();
    let prepared = std::thread::spawn(move || input.prepare())
        .join()
        .unwrap()
        .unwrap();
    let mut output = [0.0; 2048];
    prepared.render_block(0, &mut output).unwrap();
    assert!(output.iter().any(|sample| *sample != 0.0));
    assert_eq!(service.audio_source_cache_stats().decoded_sources, 0);
}
/// A stateful clip (generator + compressor): sequential blocks resume the
/// DSP chain instead of replaying the placement, and rewinds/jumps stay
/// bit-identical to the stateless evaluator (ADR-0076/ADR-0117).
#[test]
fn stateful_streaming_blocks_match_stateless_bits_and_warm_prepare() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audio.kronello");
    let (mut project, sequence) = fixture();
    let DocumentObject::Known(s) = &mut project.sequences[0] else {
        panic!()
    };
    let params = [
        param("threshold_db", scalar(-20.0)),
        param("ratio", scalar(4.0)),
        param("attack_ms", scalar(5.0)),
        param("release_ms", scalar(100.0)),
        param("makeup_db", scalar(0.0)),
    ];
    s.tracks[0].clips[0]
        .effects
        .push(Effect::Known(EffectDefinition {
            effect_id: AUDIO_COMPRESSOR_ID.into(),
            version: 1,
            parameters: EffectParameters::AudioCompressor {
                threshold_db: params[0].id(),
                ratio: params[1].id(),
                attack_ms: params[2].id(),
                release_ms: params[3].id(),
                makeup_db: params[4].id(),
            },
        }));
    s.tracks[0].clips[0].properties.extend(params);
    let service = Service::new(BackendSelection::CpuReference);
    let rev = revision(service.execute(Request::ProjectCreate(CreateRequest {
        project: path.clone(),
        document: project.clone(),
        plan_hash: None,
        idempotency_key: None,
    })));
    // Warming to the resume position keeps the first block identical.
    let prepared = PreparedAudio::prepare(
        &path,
        kronello_service::RenderTarget::Sequence { sequence },
        &rev,
        Some(24_000),
    )
    .unwrap();
    let plan =
        DocumentAudioPlan::compile_version(&project, AudioTarget::Sequence(sequence), 2).unwrap();
    let continuous = plan
        .mix(
            &AudioSources::new(),
            TimeRange::new(Time::ZERO, Time::new(40_960, 48_000).unwrap()).unwrap(),
        )
        .unwrap();
    // Scrambled block order: rewind resets the chain, forward jumps warm up.
    for start in [8_192_i64, 0, 4096, 36_864, 12_288, 16_384] {
        let mut block = vec![0.0; 8192];
        prepared.render_block(start, &mut block).unwrap();
        let expected: Vec<_> = continuous.buffer().frames()[start as usize..start as usize + 4096]
            .iter()
            .flatten()
            .map(|f| f.to_bits())
            .collect();
        assert_eq!(
            block.iter().map(|f| f.to_bits()).collect::<Vec<_>>(),
            expected,
            "block at {start}"
        );
    }
    // The metered producer path shares the same resumed cursor.
    let mut metered = vec![0.0; 8192];
    prepared.render_block_metered(20_480, &mut metered).unwrap();
    let mut plain = vec![0.0; 8192];
    prepared.render_block(20_480, &mut plain).unwrap();
    assert_eq!(metered, plain);
}

#[test]
fn exact_checkpoint_preparation_is_bounded_independent_and_revision_fenced() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("checkpoint.kronello");
    let (mut project, sequence) = pcm_fixture();
    let service = Service::new(BackendSelection::CpuReference)
        .with_audio_checkpoint_cache_limits(kronello_service::AudioCheckpointCacheLimits {
            entries: 2,
            state_bytes: 4096,
            interval_samples: 1024,
        })
        .unwrap();
    let rev = revision(service.execute(Request::ProjectCreate(CreateRequest {
        project: path.clone(),
        document: project.clone(),
        plan_hash: None,
        idempotency_key: None,
    })));
    let mut request = audio_request(&path, sequence, &rev);
    request.resume_sample = Some(3089);
    let first = service
        .capture_audio_input(request.clone())
        .unwrap()
        .prepare()
        .unwrap();
    assert_eq!(first.warm_start_sample(), 0);
    assert_eq!(first.warm_evaluated_frames(), 3089);
    let second = service
        .capture_audio_input(request.clone())
        .unwrap()
        .prepare()
        .unwrap();
    if cfg!(unix) {
        assert_eq!(second.warm_start_sample(), 3072);
        assert_eq!(second.warm_evaluated_frames(), 17);
        assert_eq!(service.audio_checkpoint_cache_stats().restores, 1);
    }
    let fresh = PreparedAudio::prepare(&path, request.target, &rev, Some(3089)).unwrap();
    for start in [3089, 0, 2048, 4096] {
        let mut a = [0.0; 1024];
        let mut b = [0.0; 1024];
        let mut c = [0.0; 1024];
        first.render_block(start, &mut a).unwrap();
        second.render_block_metered(0, &mut b).unwrap();
        let meters = second.render_block_metered(start, &mut b).unwrap();
        let expected = fresh.render_block_metered(start, &mut c).unwrap();
        assert_eq!(a.map(f32::to_bits), b.map(f32::to_bits));
        assert_eq!(a.map(f32::to_bits), c.map(f32::to_bits));
        assert_eq!(meters.master_peak, expected.master_peak);
        assert_eq!(meters.master_rms, expected.master_rms);
        assert!(service.audio_checkpoint_cache_stats().retained_checkpoints <= 2);
        assert!(service.audio_checkpoint_cache_stats().retained_state_bytes <= 4096);
    }
    assert!(service.audio_checkpoint_cache_stats().evictions > 0);
    assert_eq!(service.audio_checkpoint_cache_stats().retained_plans, 1);
    let DocumentObject::Known(sequence_data) = &mut project.sequences[0] else {
        panic!()
    };
    sequence_data.tracks[0].clips[0].enabled = false;
    let next = revision(service.execute(Request::ProjectImport(ImportRequest {
        project: path.clone(),
        base_revision: rev,
        document: project,
        plan_hash: None,
        idempotency_key: None,
    })));
    request.expected_revision = next;
    let silent = service
        .capture_audio_input(request)
        .unwrap()
        .prepare()
        .unwrap();
    let mut output = [1.0; 1024];
    silent.render_block(0, &mut output).unwrap();
    assert!(output.iter().all(|sample| *sample == 0.0));
    let saves = service.audio_checkpoint_cache_stats().saves;
    first.render_block(4096, &mut output).unwrap();
    assert!(output.iter().any(|sample| *sample != 0.0));
    assert_eq!(service.audio_checkpoint_cache_stats().saves, saves);
    assert_eq!(
        service.audio_checkpoint_cache_stats().retained_checkpoints,
        0
    );
    assert!(service.audio_checkpoint_cache_stats().invalidations >= 1);
}

#[cfg(unix)]
#[test]
fn checkpoint_restore_waits_for_source_revalidation_and_new_pcm_binding() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("fenced.kronello");
    let source = directory.path().join("source.wav");
    let (mut project, sequence) = pcm_fixture();
    let asset = project
        .assets
        .iter_mut()
        .find_map(|object| match object {
            DocumentObject::Known(asset) if asset.kind == AssetKind::Audio => Some(asset),
            _ => None,
        })
        .unwrap();
    std::fs::copy(asset.locator.absolute.as_ref().unwrap(), &source).unwrap();
    asset.locator.relative = Some("source.wav".into());
    asset.locator.absolute = None;
    let service = Service::new(BackendSelection::CpuReference)
        .with_audio_checkpoint_cache_limits(kronello_service::AudioCheckpointCacheLimits {
            entries: 8,
            state_bytes: 8192,
            interval_samples: 1024,
        })
        .unwrap();
    let rev = revision(service.execute(Request::ProjectCreate(CreateRequest {
        project: path.clone(),
        document: project,
        plan_hash: None,
        idempotency_key: None,
    })));
    let mut request = audio_request(&path, sequence, &rev);
    request.resume_sample = Some(3089);
    let old = service
        .capture_audio_input(request.clone())
        .unwrap()
        .prepare()
        .unwrap();
    assert_eq!(
        service
            .capture_audio_input(request.clone())
            .unwrap()
            .prepare()
            .unwrap()
            .warm_start_sample(),
        3072
    );
    let restores = service.audio_checkpoint_cache_stats().restores;
    let modified = std::fs::metadata(&source).unwrap().modified().unwrap();
    std::fs::File::options()
        .write(true)
        .open(&source)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(modified))
        .unwrap();
    let refreshed = service
        .capture_audio_input(request.clone())
        .unwrap()
        .prepare()
        .unwrap();
    assert_eq!(refreshed.warm_start_sample(), 0);
    assert_eq!(service.audio_checkpoint_cache_stats().restores, restores);
    assert!(service.audio_checkpoint_cache_stats().invalidations >= 1);
    let original = std::fs::read(&source).unwrap();
    let mut changed = original.clone();
    let end = changed.len() - 1;
    changed[end] ^= 1;
    std::fs::write(&source, &changed).unwrap();
    let failure = service
        .capture_audio_input(request.clone())
        .unwrap()
        .prepare()
        .err()
        .unwrap();
    assert_eq!(failure.code, "ASSET_HASH_MISMATCH");
    assert_eq!(service.audio_checkpoint_cache_stats().restores, restores);
    std::fs::write(&source, &original).unwrap();
    let recovered = service
        .capture_audio_input(request)
        .unwrap()
        .prepare()
        .unwrap();
    assert_eq!(recovered.warm_start_sample(), 0);
    let mut a = [0.0; 2048];
    let mut b = [0.0; 2048];
    old.render_block(3089, &mut a).unwrap();
    recovered.render_block(3089, &mut b).unwrap();
    assert_eq!(a.map(f32::to_bits), b.map(f32::to_bits));
    std::fs::remove_file(&source).unwrap();
    let failure = service
        .capture_audio_input(audio_request(&path, sequence, &rev))
        .unwrap()
        .prepare()
        .err()
        .unwrap();
    assert_eq!(failure.code, "ASSET_MISSING");
}

#[test]
fn native_checkpoint_capture_restores_on_producer_and_survives_project_close() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("native-checkpoint.kronello");
    let (mut project, sequence) = fixture();
    let (pcm, _) = pcm_fixture();
    let DocumentObject::Known(template) = &pcm.sequences[0] else {
        panic!()
    };
    let DocumentObject::Known(target) = &mut project.sequences[0] else {
        panic!()
    };
    target.tracks[0].clips[0].effects = template.tracks[0].clips[0].effects.clone();
    target.tracks[0].clips[0].properties = template.tracks[0].clips[0].properties.clone();
    let service = Service::new(BackendSelection::CpuReference);
    let rev = revision(service.execute(Request::ProjectCreate(CreateRequest {
        project: path.clone(),
        document: project.clone(),
        plan_hash: None,
        idempotency_key: None,
    })));
    let owner = kronello_service::ProjectSession::open(&path).unwrap();
    let capture = |resume| {
        owner
            .scope(|| {
                Service::new(BackendSelection::CpuReference).capture_audio_input(
                    kronello_service::AudioPrepareRequest {
                        project: path.clone(),
                        target: kronello_service::RenderTarget::Sequence { sequence },
                        expected_revision: rev.clone(),
                        resume_sample: Some(resume),
                    },
                )
            })
            .unwrap()
    };
    let input = capture(5 * 48_000 + 17);
    let first = std::thread::spawn(move || input.prepare())
        .join()
        .unwrap()
        .unwrap();
    assert_eq!(first.warm_start_sample(), 0);
    let input = capture(4 * 48_000 + 17);
    let second = std::thread::spawn(move || input.prepare())
        .join()
        .unwrap()
        .unwrap();
    assert_eq!(second.warm_start_sample(), 4 * 48_000);
    assert_eq!(second.warm_evaluated_frames(), 17);
    assert_eq!(owner.audio_checkpoint_cache_stats().restores, 1);
    let input = capture(4 * 48_000 + 33);
    drop(owner);
    std::fs::remove_file(&path).unwrap();
    let third = std::thread::spawn(move || input.prepare())
        .join()
        .unwrap()
        .unwrap();
    assert_eq!(third.warm_start_sample(), 4 * 48_000);
    assert_eq!(third.warm_evaluated_frames(), 33);
    let plan =
        DocumentAudioPlan::compile_version(&project, AudioTarget::Sequence(sequence), 2).unwrap();
    for prepared in [&first, &second, &third] {
        let start = 4 * 48_000 + 33;
        let mut output = [0.0; 2048];
        prepared.render_block(start, &mut output).unwrap();
        let expected = plan
            .mix(
                &AudioSources::new(),
                TimeRange::new(
                    Time::new(start, 48_000).unwrap(),
                    Time::new(start + 1024, 48_000).unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
        assert_eq!(
            output.map(f32::to_bits),
            expected
                .buffer()
                .frames()
                .iter()
                .flatten()
                .map(|sample| sample.to_bits())
                .collect::<Vec<_>>()
                .as_slice()
        );
    }
    assert_eq!(service.audio_checkpoint_cache_stats().retained_plans, 0);
}

#[test]
fn checkpoint_byte_admission_and_weak_sources_do_not_retain_uncached_pcm() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("bypassed.kronello");
    let (project, sequence) = pcm_fixture();
    let create = Service::new(BackendSelection::CpuReference);
    let rev = revision(create.execute(Request::ProjectCreate(CreateRequest {
        project: path.clone(),
        document: project,
        plan_hash: None,
        idempotency_key: None,
    })));
    let mut request = audio_request(&path, sequence, &rev);
    request.resume_sample = Some(3089);
    let tiny = Service::new(BackendSelection::CpuReference)
        .with_audio_checkpoint_cache_limits(kronello_service::AudioCheckpointCacheLimits {
            entries: 8,
            state_bytes: 1,
            interval_samples: 1024,
        })
        .unwrap();
    for _ in 0..2 {
        assert_eq!(
            tiny.capture_audio_input(request.clone())
                .unwrap()
                .prepare()
                .unwrap()
                .warm_start_sample(),
            0
        );
    }
    assert_eq!(tiny.audio_checkpoint_cache_stats().retained_state_bytes, 0);
    assert!(tiny.audio_checkpoint_cache_stats().bypasses > 0);
    let weak = Service::new(BackendSelection::CpuReference)
        .with_audio_source_cache_limits(kronello_media::AudioSourceCacheLimits {
            entries: 0,
            pcm_bytes: 0,
        })
        .with_audio_checkpoint_cache_limits(kronello_service::AudioCheckpointCacheLimits {
            entries: 8,
            state_bytes: 8192,
            interval_samples: 1024,
        })
        .unwrap();
    for _ in 0..2 {
        let prepared = weak
            .capture_audio_input(request.clone())
            .unwrap()
            .prepare()
            .unwrap();
        assert_eq!(prepared.warm_start_sample(), 0);
        assert_eq!(weak.audio_source_cache_stats().retained_sources, 0);
        assert!(weak.audio_checkpoint_cache_stats().retained_checkpoints > 0);
    }
    assert_eq!(weak.audio_checkpoint_cache_stats().restores, 0);
    assert_eq!(weak.audio_source_cache_stats().decoded_sources, 2);
    assert_eq!(weak.audio_checkpoint_cache_stats().invalidations, 1);
    let retained = weak.audio_checkpoint_cache_stats();
    let per_state = retained.retained_state_bytes / retained.retained_checkpoints;
    let bytes = Service::new(BackendSelection::CpuReference)
        .with_audio_checkpoint_cache_limits(kronello_service::AudioCheckpointCacheLimits {
            entries: 8,
            state_bytes: per_state * 2,
            interval_samples: 1024,
        })
        .unwrap();
    let prepared = bytes
        .capture_audio_input(request)
        .unwrap()
        .prepare()
        .unwrap();
    let retained = bytes.audio_checkpoint_cache_stats();
    // Three real compressor states exceed the byte cap while remaining
    // below the entry cap, so this exercises byte-triggered eviction.
    assert_eq!(retained.retained_checkpoints, 2);
    assert_eq!(retained.retained_state_bytes, per_state * 2);
    assert!(retained.evictions > 0);
    let mut actual = [0.0; 2048];
    prepared.render_block(3089, &mut actual).unwrap();
    let fresh = PreparedAudio::prepare(
        &path,
        kronello_service::RenderTarget::Sequence { sequence },
        &rev,
        Some(3089),
    )
    .unwrap();
    let mut expected = [0.0; 2048];
    fresh.render_block(3089, &mut expected).unwrap();
    assert_eq!(actual.map(f32::to_bits), expected.map(f32::to_bits));
}
