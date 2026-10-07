use kronello_gpu::render_adapter::CpuReferenceBackend;
use kronello_media::{MediaRuntime, VideoRenderBackend};
use kronello_model::*;
use kronello_render::{
    FrameRequest, OutputRegion, RenderSnapshot, RenderTarget, build_render_dag, build_scene_ir,
    render_frame,
};
use kronello_service::*;
use kronello_time::{FrameRate, Rational, SampleRate, Time, TimeMap, TimeMapPoint, TimeRange};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
use uuid::Uuid;

fn t(n: i64, d: i64) -> Time {
    Time::new(n, d).unwrap()
}

#[test]
fn split_retime_mapping_owned_properties_receipt_and_one_undo() {
    let mut c = clip([60, 80, 120], 1, 5);
    c.time_map = TimeMap::piecewise_linear(vec![
        TimeMapPoint {
            parent: t(0, 1),
            local: t(0, 1),
        },
        TimeMapPoint {
            parent: t(1, 1),
            local: t(1, 2),
        },
        TimeMapPoint {
            parent: t(4, 1),
            local: t(3, 1),
        },
    ])
    .unwrap();
    let sigma = property(
        "kronello.effect.sigma",
        Value::Scalar(FiniteF64::new(2.0).unwrap()),
    );
    c.effects = vec![Effect::Known(EffectDefinition {
        effect_id: GAUSSIAN_BLUR_ID.into(),
        version: 1,
        parameters: EffectParameters::GaussianBlur { sigma: sigma.id() },
    })];
    let mut opacity = property(
        "kronello.opacity",
        Value::Scalar(FiniteF64::new(0.75).unwrap()),
    );
    let modifier = Modifier {
        id: ModifierId::new(),
        key: SchemaKey::new("example.unsupported").unwrap(),
        version: 1,
        enabled: false,
        parameters: Default::default(),
    };
    opacity
        .set_modifiers(vec![modifier.clone()], &kronello_render::render_registry())
        .unwrap();
    c.properties = vec![sigma.clone(), opacity.clone()];
    let volume = property(
        "kronello.audio.volume",
        Value::Scalar(FiniteF64::new(0.8).unwrap()),
    );
    c.volume = Some(Box::new(volume.clone()));
    let original = c.clone();
    let s = sequence(vec![c]);
    let sequence_id = s.id;
    let (_dir, path) = setup(project(s));
    let before = export(&path);
    let right_id = ClipId::new();
    let command = TimelineCommand::ClipSplit {
        sequence: sequence_id,
        clip: original.id,
        time: t(5, 2),
        right_clip: right_id,
    };
    let request = apply_request(&path, vec![command.clone()], "split-receipt");
    let second_plan = apply_request(&path, vec![command], "split-receipt");
    assert_eq!(
        request.plan_hash, second_plan.plan_hash,
        "owned IDs are deterministic across plans"
    );
    let ResultData::Edit(event) = service()
        .dispatch(Request::EditApply(request.clone()))
        .unwrap()
    else {
        panic!()
    };
    let ResultData::Edit(replay) = service().dispatch(Request::EditApply(request)).unwrap() else {
        panic!()
    };
    assert_eq!(event, replay);
    assert_eq!(
        export(&path).revision.parse::<u64>().unwrap(),
        before.revision.parse::<u64>().unwrap() + 1
    );
    let after = export(&path);
    let DocumentObject::Known(s) = &after.document.sequences[0] else {
        panic!()
    };
    let [left, right] = s.tracks[0].clips.as_slice() else {
        panic!()
    };
    assert_eq!(left.id, original.id);
    assert_eq!(right.id, right_id);
    assert_eq!(left.timeline_range, range(t(1, 1), t(5, 2)));
    assert_eq!(right.timeline_range, range(t(5, 2), t(5, 1)));
    for (part, times) in [
        (left, vec![t(1, 1), t(2, 1), t(5, 2)]),
        (right, vec![t(5, 2), t(3, 1), t(5, 1)]),
    ] {
        for time in times {
            assert_eq!(
                part.local_time(time).unwrap(),
                original.local_time(time).unwrap()
            );
        }
    }
    assert_eq!(left.properties, original.properties);
    assert_ne!(right.properties[0].id(), sigma.id());
    assert_eq!(right.properties[0].source(), sigma.source());
    assert_ne!(right.properties[1].id(), opacity.id());
    assert_ne!(right.properties[1].modifiers()[0].id, modifier.id);
    assert_eq!(
        right.properties[1].modifiers()[0].parameters,
        modifier.parameters
    );
    assert_ne!(right.volume.as_ref().unwrap().id(), volume.id());
    assert_eq!(right.volume.as_ref().unwrap().source(), volume.source());
    assert!(
        matches!(&right.effects[0], Effect::Known(e) if e.parameters == EffectParameters::GaussianBlur { sigma: right.properties[0].id() })
    );
    undo(&path, event.id).unwrap();
    assert_eq!(
        export(&path).document,
        before.document,
        "one inverse restores all split objects"
    );
}

#[test]
fn split_rejects_boundaries_duplicate_links_and_transition_atomically() {
    let c = clip([20, 40, 80], 0, 4);
    let s = sequence(vec![c.clone()]);
    let sequence_id = s.id;
    let (_dir, path) = setup(project(s));
    for time in [t(-1, 1), t(0, 1), t(4, 1), t(5, 1)] {
        reject(
            &path,
            vec![TimelineCommand::ClipSplit {
                sequence: sequence_id,
                clip: c.id,
                time,
                right_clip: ClipId::new(),
            }],
            "INVALID_EDIT",
        );
    }
    reject(
        &path,
        vec![TimelineCommand::ClipSplit {
            sequence: sequence_id,
            clip: c.id,
            time: t(2, 1),
            right_clip: c.id,
        }],
        "INVALID_EDIT",
    );
    reject(
        &path,
        vec![TimelineCommand::ClipSplit {
            sequence: sequence_id,
            clip: ClipId::new(),
            time: t(2, 1),
            right_clip: ClipId::new(),
        }],
        "SOURCE_MISSING",
    );
    let mut a = c.clone();
    let mut b = clip([10, 10, 10], 4, 6);
    a.links = vec![b.id];
    b.links = vec![a.id];
    let s = sequence(vec![a, b]);
    let id = s.id;
    let (_linked_dir, linked_path) = setup(project(s));
    reject(
        &linked_path,
        vec![TimelineCommand::ClipSplit {
            sequence: id,
            clip: c.id,
            time: t(2, 1),
            right_clip: ClipId::new(),
        }],
        "LINKED_EDIT_REQUIRED",
    );
    let incoming = clip([10, 10, 10], 3, 6);
    let mut s = sequence(vec![c.clone(), incoming.clone()]);
    let id = s.id;
    s.transitions = vec![Transition {
        params: None,
        version: 1,
        kind: TransitionKind::Crossfade,
        outgoing: c.id,
        incoming: incoming.id,
        range: range(t(3, 1), t(4, 1)),
    }];
    let (_transition_dir, transition_path) = setup(project(s));
    reject(
        &transition_path,
        vec![TimelineCommand::ClipSplit {
            sequence: id,
            clip: c.id,
            time: t(2, 1),
            right_clip: ClipId::new(),
        }],
        "TRANSITION_EDIT_CONFLICT",
    );
}

#[test]
fn split_undo_preserves_other_sequences_and_rejects_later_right_clip_edits() {
    let left = clip([60, 80, 120], 0, 4);
    let a = sequence(vec![left.clone()]);
    let other = clip([20, 30, 40], 0, 2);
    let b = sequence(vec![other.clone()]);
    let mut p = project(a.clone());
    p.sequences.push(DocumentObject::Known(b.clone()));
    let (_dir, path) = setup(p.clone());
    let right = ClipId::new();
    let command = TimelineCommand::ClipSplit {
        sequence: a.id,
        clip: left.id,
        time: t(2, 1),
        right_clip: right,
    };
    let event = apply(&path, vec![command.clone()], "split-selective");
    apply(
        &path,
        vec![TimelineCommand::ClipMove {
            sequence: b.id,
            clip: other.id,
            delta: t(1, 1),
            linked: false,
        }],
        "other-sequence",
    );
    let before_undo = export(&path);
    undo(&path, event.id).unwrap();
    let after = export(&path);
    assert_eq!(after.document.sequences[0], DocumentObject::Known(a));
    assert_eq!(
        after.document.sequences[1],
        before_undo.document.sequences[1]
    );
    let (_conflict_dir, conflict_path) = setup(p);
    let split = apply(&conflict_path, vec![command], "split-conflict");
    apply(
        &conflict_path,
        vec![TimelineCommand::ClipMove {
            sequence: b.id,
            clip: other.id,
            delta: t(1, 1),
            linked: false,
        }],
        "independent",
    );
    let sequence = match &export(&conflict_path).document.sequences[0] {
        DocumentObject::Known(s) => s.id,
        _ => panic!(),
    };
    apply(
        &conflict_path,
        vec![TimelineCommand::ClipMove {
            sequence,
            clip: right,
            delta: t(1, 1),
            linked: false,
        }],
        "right-edit",
    );
    let before = export(&conflict_path);
    assert_eq!(
        undo(&conflict_path, split.id).unwrap_err().code,
        "UNDO_CONFLICT"
    );
    assert_eq!(export(&conflict_path).document, before.document);
    assert_eq!(export(&conflict_path).revision, before.revision);
}

#[test]
fn sequence_asset_availability_is_batched_and_never_claims_hash_verification() {
    let s = sequence(vec![clip([0, 0, 0], 0, 2)]);
    let id = s.id;
    let mut p = project(s);
    let assets_dir = tempfile::tempdir().unwrap();
    let present = assets_dir.path().join("present.bin");
    std::fs::write(
        &present,
        b"content whose hash is intentionally not recorded",
    )
    .unwrap();
    let asset = |path: &Path| Asset {
        id: AssetId::new(),
        content_hash: "0".repeat(64),
        kind: AssetKind::Video,
        streams: vec![],
        locator: AssetLocator {
            relative: None,
            absolute: Some(path.display().to_string()),
        },
    };
    let present_asset = asset(&present);
    let missing_asset = asset(&assets_dir.path().join("missing.bin"));
    p.assets = vec![
        DocumentObject::Known(present_asset.clone()),
        DocumentObject::Known(missing_asset.clone()),
    ];
    let (_dir, path) = setup(p);
    let ResultData::Timeline(result) = service()
        .dispatch(Request::SequenceQuery(SequenceQueryRequest {
            project: path.clone(),
            sequence: id,
        }))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(result.asset_status.len(), 2);
    let present_status = result
        .asset_status
        .iter()
        .find(|s| s.asset == present_asset.id)
        .unwrap();
    assert_eq!(
        present_status.availability,
        AssetAvailability::PresentUnverified
    );
    assert_eq!(
        present_status.size_bytes,
        Some(std::fs::metadata(&present).unwrap().len())
    );
    assert!(
        present_status.error.is_none(),
        "a deliberately incorrect hash is not read by this query"
    );
    let missing_status = result
        .asset_status
        .iter()
        .find(|s| s.asset == missing_asset.id)
        .unwrap();
    assert_eq!(missing_status.availability, AssetAvailability::Missing);
    assert_eq!(missing_status.error.as_ref().unwrap().code, "ASSET_MISSING");
    assert_eq!(
        kronello_media::resolve_asset(&present_asset, &path)
            .unwrap_err()
            .code(),
        "ASSET_HASH_MISMATCH",
        "render resolver still verifies all content"
    );
}
fn range(a: Time, b: Time) -> TimeRange {
    TimeRange::new(a, b).unwrap()
}
fn clip(color: [u8; 3], a: i64, b: i64) -> Clip {
    Clip {
        id: ClipId::new(),
        source_ref: SourceRef::Generator {
            generator: SOLID_GENERATOR_ID.into(),
            version: 1,
            color: Color::from_srgb8(color, None),
        },
        timeline_range: range(t(a, 1), t(b, 1)),
        source_in: Time::ZERO,
        time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
        audio_retime: AudioRetimePolicy::Reject,
        reverse_sampling: None,
        volume: None,
        links: vec![],
        properties: vec![],
        effects: vec![],
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
    }
}
fn project(sequence: Sequence) -> Project {
    let mut p = Project::default();
    p.sequences.push(DocumentObject::Known(sequence));
    p
}
fn snapshot(p: &Project) -> RenderSnapshot {
    let DocumentObject::Known(s) = &p.sequences[0] else {
        panic!()
    };
    RenderSnapshot::for_target(
        p,
        RenderTarget::Sequence { sequence: s.id },
        0,
        Default::default(),
    )
    .unwrap()
}
fn region() -> OutputRegion {
    OutputRegion {
        origin: [0.0; 2],
        extent: [16.0; 2],
        pixels: [16; 2],
    }
}
fn cpu(s: &RenderSnapshot, time: Time) -> kronello_render::RenderedFrame {
    render_frame(
        s,
        &[],
        &CpuReferenceBackend,
        FrameRequest {
            time,
            region: region(),
        },
    )
    .unwrap()
}
fn service() -> Service<'static> {
    Service::new(BackendSelection::CpuReference)
}
fn setup(p: Project) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nle2.kronello");
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
fn apply_request(path: &Path, commands: Vec<TimelineCommand>, key: &str) -> EditApplyRequest {
    let commands = commands
        .into_iter()
        .map(|c| EditCommand::Timeline(Box::new(c)))
        .collect::<Vec<_>>();
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
        plan_hash: plan.plan_hash,
        idempotency_key: key.into(),
        session_id: Uuid::new_v4(),
        commands,
    }
}
fn apply(path: &Path, commands: Vec<TimelineCommand>, key: &str) -> kronello_store::Event {
    let ResultData::Edit(e) = service()
        .dispatch(Request::EditApply(apply_request(path, commands, key)))
        .unwrap()
    else {
        panic!()
    };
    e
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
fn reject(path: &Path, commands: Vec<TimelineCommand>, code: &str) {
    let before = export(path);
    let e = service()
        .dispatch(Request::EditPlan(PlanRequest {
            project: path.into(),
            base_revision: before.revision.clone(),
            commands: commands
                .into_iter()
                .map(|c| EditCommand::Timeline(Box::new(c)))
                .collect(),
        }))
        .unwrap_err();
    assert_eq!(e.code, code, "{e:?}");
    assert_eq!(export(path).document, before.document);
    assert_eq!(export(path).revision, before.revision);
}
fn property(key: &str, value: Value) -> Property {
    let r = kronello_render::render_registry();
    Property::new(
        PropertyId::new(),
        DescriptorRef::new(r.lookup(&SchemaKey::new(key).unwrap()).unwrap()),
        PropertySource::Constant(value),
        vec![],
        &r,
    )
    .unwrap()
}
fn asset(runtime: &MediaRuntime, path: &Path, stream: u32) -> Asset {
    let mut decoder = runtime.open_video_stream(path, stream).unwrap();
    Asset {
        id: AssetId::new(),
        content_hash: kronello_media::content_hash(path).unwrap(),
        kind: AssetKind::Video,
        streams: vec![decoder.stream_metadata().unwrap()],
        locator: AssetLocator {
            relative: None,
            absolute: Some(path.to_str().unwrap().into()),
        },
    }
}
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures/generated/media")
}

#[test]
fn generator_fixed_snapshot_versions_order_and_half_open_boundaries() {
    let s = sequence(vec![clip([255, 0, 0], 2, 4), clip([0, 0, 255], 4, 6)]);
    let mut p = project(s.clone());
    let snap = snapshot(&p);
    assert_eq!(snap.semantic_versions().generators[SOLID_GENERATOR_ID], 1);
    let restored: RenderSnapshot =
        serde_json::from_str(&serde_json::to_string(&snap).unwrap()).unwrap();
    let times = [t(5, 1), t(2, 1), t(6, 1), t(4, 1), t(1, 1), t(7, 2)];
    let frames: Vec<_> = times.iter().map(|time| cpu(&snap, *time)).collect();
    p.sequences.clear();
    for (time, expected) in times.iter().zip(&frames).rev() {
        assert_eq!(cpu(&restored, *time), *expected);
    }
    assert_eq!(frames[1].pixels.linear[0], [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(frames[3].pixels.linear[0], [0.0, 0.0, 1.0, 1.0]);
    assert!(frames[2].pixels.linear.iter().all(|p| *p == [0.0; 4]));
    for source in [
        SourceRef::Generator {
            generator: "unknown".into(),
            version: 1,
            color: Color::from_srgb8([0; 3], None),
        },
        SourceRef::Generator {
            generator: SOLID_GENERATOR_ID.into(),
            version: 2,
            color: Color::from_srgb8([0; 3], None),
        },
    ] {
        let mut bad = project(s.clone());
        let DocumentObject::Known(s) = &mut bad.sequences[0] else {
            panic!()
        };
        s.tracks[0].clips[0].source_ref = source;
        let id = s.id;
        bad.validate_storage().unwrap();
        assert_eq!(
            RenderSnapshot::for_target(
                &bad,
                RenderTarget::Sequence { sequence: id },
                0,
                Default::default()
            )
            .unwrap_err()
            .code(),
            "UNSUPPORTED_FEATURE"
        );
    }
}

#[test]
fn video_placements_cfr_vfr_bframes_exact_seeks_retime_and_independence() {
    let runtime = MediaRuntime::load().unwrap();
    for file in [
        "cfr-24-1.nut",
        "cfr-25-1.nut",
        "cfr-30-1.nut",
        "cfr-30000-1001.nut",
        "cfr-60000-1001.nut",
        "vfr.nut",
        "bframes.nut",
    ] {
        let path = fixtures().join(file);
        let asset = asset(&runtime, &path, 0);
        let origin = asset.streams[0].start_time.unwrap();
        let mut a = clip([0; 3], 2, 3);
        a.source_ref = SourceRef::Asset {
            asset: asset.id,
            stream_index: 0,
        };
        a.source_in = origin;
        a.time_map = TimeMap::linear(Time::ZERO, t(1, 24)).unwrap();
        let mut b = a.clone();
        b.id = ClipId::new();
        b.source_in = origin.checked_add(t(1, 24)).unwrap();
        b.time_map = TimeMap::piecewise_linear(vec![
            TimeMapPoint {
                parent: Time::ZERO,
                local: Time::ZERO,
            },
            TimeMapPoint {
                parent: t(1, 2),
                local: t(1, 96),
            },
            TimeMapPoint {
                parent: t(1, 1),
                local: t(1, 24),
            },
        ])
        .unwrap();
        let mut s = sequence(vec![a.clone()]);
        s.tracks.push(Track {
            state: None,
            id: TrackId::new(),
            kind: TrackKind::Video,
            clips: vec![b.clone()],
        });
        let mut p = project(s);
        p.assets.push(DocumentObject::Known(asset.clone()));
        let snap = snapshot(&p);
        let backend = VideoRenderBackend {
            backend: &CpuReferenceBackend,
            project_path: &path,
        };
        for time in [t(11, 4), t(2, 1), t(5, 2), t(9, 4), t(11, 4)] {
            let scene = build_scene_ir(&snap, time, &[]).unwrap();
            let locals: Vec<_> = scene
                .nodes
                .iter()
                .filter_map(|n| match &n.content {
                    kronello_render::SceneContent::Video { time, .. } => Some(*time),
                    _ => None,
                })
                .collect();
            assert_eq!(
                locals,
                vec![a.local_time(time).unwrap(), b.local_time(time).unwrap()]
            );
            let expected = runtime
                .decode_video_image(
                    &asset,
                    &path,
                    0,
                    b.local_time(time).unwrap(),
                    ColorSpace::LinearRec709,
                )
                .unwrap();
            let frame = render_frame(
                &snap,
                &[],
                &backend,
                FrameRequest {
                    time,
                    region: region(),
                },
            )
            .unwrap();
            assert_eq!(frame.pixels.linear, expected.pixels, "{file} {time:?}");
            assert!(frame.metadata.input_path.contains("software_video_decode"));
        }
        let mut bottom = p.clone();
        let DocumentObject::Known(s) = &mut bottom.sequences[0] else {
            panic!()
        };
        s.tracks.truncate(1);
        let actual = render_frame(
            &snapshot(&bottom),
            &[],
            &backend,
            FrameRequest {
                time: t(11, 4),
                region: region(),
            },
        )
        .unwrap();
        let expected = runtime
            .decode_video_image(
                &asset,
                &path,
                0,
                a.local_time(t(11, 4)).unwrap(),
                ColorSpace::LinearRec709,
            )
            .unwrap();
        assert_eq!(
            actual.pixels.linear, expected.pixels,
            "independent bottom {file}"
        );
        for time in [t(1, 1), t(3, 1)] {
            assert!(
                render_frame(
                    &snap,
                    &[],
                    &backend,
                    FrameRequest {
                        time,
                        region: region()
                    }
                )
                .unwrap()
                .pixels
                .linear
                .iter()
                .all(|p| *p == [0.0; 4])
            );
        }
        let restored: RenderSnapshot =
            serde_json::from_str(&serde_json::to_string(&snap).unwrap()).unwrap();
        assert_eq!(
            render_frame(
                &snap,
                &[],
                &backend,
                FrameRequest {
                    time: t(5, 2),
                    region: region()
                }
            )
            .unwrap(),
            render_frame(
                &restored,
                &[],
                &backend,
                FrameRequest {
                    time: t(5, 2),
                    region: region()
                }
            )
            .unwrap()
        );
    }
}

#[test]
fn transition_set_and_clip_effect_authoring_use_atomic_plans_and_undo() {
    let a = clip([255, 0, 0], 0, 3);
    let b = clip([0, 0, 255], 2, 5);
    let s = sequence(vec![a.clone()]);
    let id = s.id;
    let track = s.tracks[0].id;
    let (_dir, path) = setup(project(s));
    let initial = export(&path).document;
    let transition = Transition {
        outgoing: a.id,
        incoming: b.id,
        range: range(t(2, 1), t(3, 1)),
        kind: TransitionKind::Crossfade,
        params: None,
        version: 1,
    };
    let e = apply(
        &path,
        vec![
            TimelineCommand::ClipPlace {
                sequence: id,
                track,
                clip: Box::new(b.clone()),
            },
            TimelineCommand::TransitionSet {
                sequence: id,
                transition: transition.clone(),
            },
        ],
        "place-transition",
    );
    assert_eq!(
        cpu(&snapshot(&export(&path).document), t(5, 2))
            .pixels
            .linear[0],
        [0.5, 0.0, 0.5, 1.0]
    );
    undo(&path, e.id).unwrap();
    assert_eq!(export(&path).document, initial);
    let sigma = property(
        "kronello.effect.sigma",
        Value::Scalar(FiniteF64::new(1.0).unwrap()),
    );
    let e = apply(
        &path,
        vec![TimelineCommand::ClipSetEffects {
            sequence: id,
            clip: a.id,
            properties: vec![sigma.clone()],
            effects: vec![Effect::Known(EffectDefinition {
                effect_id: GAUSSIAN_BLUR_ID.into(),
                version: 1,
                parameters: EffectParameters::GaussianBlur { sigma: sigma.id() },
            })],
        }],
        "clip-effect",
    );
    assert!(
        cpu(&snapshot(&export(&path).document), t(1, 1))
            .pixels
            .linear[0][3]
            < 1.0
    );
    undo(&path, e.id).unwrap();
    assert_eq!(export(&path).document, initial);
    // Clip properties use the same value-edit command and conflict keys as
    // composition properties, including structural Undo conflicts.
    let (_property_dir, property_path) = setup(effects_fixture());
    let before = export(&property_path);
    let DocumentObject::Known(s) = &before.document.sequences[0] else {
        panic!()
    };
    let target = &s.tracks[1].clips[0];
    let property = target.properties.last().unwrap().id();
    let commands = vec![EditCommand::PropertySourceSet {
        object: target.id.as_uuid(),
        property,
        source: PropertySource::Constant(Value::Scalar(FiniteF64::new(2.0).unwrap())),
        curve: None,
    }];
    let ResultData::Plan(plan) = service()
        .dispatch(Request::EditPlan(PlanRequest {
            project: property_path.clone(),
            base_revision: before.revision.clone(),
            commands: commands.clone(),
        }))
        .unwrap()
    else {
        panic!()
    };
    let ResultData::Edit(value_event) = service()
        .dispatch(Request::EditApply(EditApplyRequest {
            project: property_path.clone(),
            base_revision: before.revision,
            commands,
            plan_hash: plan.plan_hash,
            idempotency_key: "clip-property".into(),
            session_id: Uuid::new_v4(),
        }))
        .unwrap()
    else {
        panic!()
    };
    assert_ne!(
        cpu(&snapshot(&before.document), t(2, 1)),
        cpu(&snapshot(&export(&property_path).document), t(2, 1))
    );
    apply(
        &property_path,
        vec![TimelineCommand::ClipMove {
            sequence: s.id,
            clip: target.id,
            delta: t(1, 1),
            linked: false,
        }],
        "clip-property-structure",
    );
    assert_eq!(
        undo(&property_path, value_event.id).unwrap_err().code,
        "UNDO_CONFLICT"
    );
    let mut bad = initial.clone();
    let DocumentObject::Known(s) = &mut bad.sequences[0] else {
        panic!()
    };
    s.tracks[0].clips.push(b);
    let mut future = transition;
    future.version = 2;
    s.transitions.push(future);
    bad.validate_storage().unwrap();
    assert_eq!(
        RenderSnapshot::for_target(
            &bad,
            RenderTarget::Sequence { sequence: id },
            0,
            Default::default()
        )
        .unwrap_err()
        .code(),
        "UNSUPPORTED_FEATURE"
    );
}

#[test]
fn linked_move_traverses_an_imported_reciprocal_chain() {
    let mut a = clip([255, 0, 0], 0, 2);
    let mut b = clip([0, 0, 255], 0, 2);
    let mut c = clip([0, 255, 0], 0, 2);
    a.links = vec![b.id];
    b.links = vec![a.id, c.id];
    c.links = vec![b.id];
    let mut s = sequence(vec![a.clone()]);
    for clip in [b.clone(), c.clone()] {
        s.tracks.push(Track {
            state: None,
            id: TrackId::new(),
            kind: TrackKind::Video,
            clips: vec![clip],
        });
    }
    let id = s.id;
    let mut unreciprocated = s.clone();
    unreciprocated.tracks[1].clips[0]
        .links
        .retain(|link| *link != a.id);
    assert_eq!(
        unreciprocated
            .validate(&project(s.clone()))
            .unwrap_err()
            .code(),
        "INVALID_CLIP"
    );
    let (_dir, path) = setup(project(s));
    let initial = export(&path).document;
    let request = apply_request(
        &path,
        vec![TimelineCommand::ClipMove {
            sequence: id,
            clip: a.id,
            delta: t(3, 1),
            linked: true,
        }],
        "chain",
    );
    let ResultData::Edit(e) = service()
        .dispatch(Request::EditApply(request.clone()))
        .unwrap()
    else {
        panic!()
    };
    let ResultData::Edit(retry) = service().dispatch(Request::EditApply(request)).unwrap() else {
        panic!()
    };
    assert_eq!(e, retry);
    let p = export(&path).document;
    let DocumentObject::Known(s) = &p.sequences[0] else {
        panic!()
    };
    for clip in s.tracks.iter().flat_map(|t| &t.clips) {
        assert_eq!(clip.timeline_range, range(t(3, 1), t(5, 1)));
    }
    undo(&path, e.id).unwrap();
    assert_eq!(export(&path).document, initial);
    let e = apply(
        &path,
        vec![TimelineCommand::ClipLink {
            sequence: id,
            clips: vec![b.id],
        }],
        "unlink-middle",
    );
    let p = export(&path).document;
    let DocumentObject::Known(s) = &p.sequences[0] else {
        panic!()
    };
    assert!(
        s.tracks
            .iter()
            .flat_map(|t| &t.clips)
            .all(|c| c.links.is_empty())
    );
    undo(&path, e.id).unwrap();
    assert_eq!(export(&path).document, initial);
}

#[test]
fn video_color_defaults_tagged_sdr_and_unknown_tags_are_explicit_in_query() {
    let runtime = MediaRuntime::load().unwrap();
    let path = fixtures().join("cfr-24-1.nut");
    let original = asset(&runtime, &path, 0);
    let query = |metadata: StreamMetadata| {
        let mut a = original.clone();
        a.streams = vec![metadata];
        let mut c = clip([0; 3], 0, 1);
        c.source_ref = SourceRef::Asset {
            asset: a.id,
            stream_index: 0,
        };
        c.time_map = TimeMap::linear(Time::ZERO, t(1, 24)).unwrap();
        let s = sequence(vec![c]);
        let id = s.id;
        let mut p = project(s);
        p.assets.push(DocumentObject::Known(a));
        let (_dir, project) = setup(p);
        let ResultData::Timeline(q) = service()
            .dispatch(Request::SequenceQuery(SequenceQueryRequest {
                project,
                sequence: id,
            }))
            .unwrap()
        else {
            panic!()
        };
        q.clips.into_iter().next().unwrap()
    };
    for format in ["yuv420p", "rgb24"] {
        let mut metadata = original.streams[0].clone();
        metadata.pixel_format = Some(format.into());
        metadata.color_primaries = None;
        metadata.color_transfer = None;
        metadata.color_matrix = None;
        metadata.color_range = None;
        let q = query(metadata.clone());
        let policy = q.video_color.unwrap();
        assert!(q.unsupported_reason.is_none());
        let rgb = format == "rgb24";
        let transfer = if rgb { "iec61966-2-1" } else { "bt709" };
        let matrix = if rgb { "gbr" } else { "bt709" };
        let range = if rgb { "pc" } else { "tv" };
        assert_eq!(
            (
                &*policy.primaries,
                &*policy.transfer,
                &*policy.matrix,
                &*policy.range
            ),
            ("bt709", transfer, matrix, range)
        );
        assert_eq!(
            policy.assumptions,
            vec![
                "untagged primaries: bt709".to_string(),
                format!("untagged transfer: {transfer}"),
                format!("untagged matrix: {matrix}"),
                format!("untagged range: {range}")
            ]
        );
        // FFmpeg's unknown/unspecified markers mean absent metadata, distinct
        // from an explicit unrecognized tag such as future-transfer-v2.
        metadata.color_primaries = Some("unknown".into());
        metadata.color_transfer = Some("unspecified".into());
        metadata.color_matrix = Some("N/A".into());
        metadata.color_range = Some(String::new());
        assert_eq!(query(metadata.clone()).video_color.unwrap(), policy);
        for transfer in ["bt709", "iec61966-2-1"] {
            for range in if rgb { vec!["pc"] } else { vec!["tv", "pc"] } {
                metadata.color_primaries = Some("bt709".into());
                metadata.color_transfer = Some(transfer.into());
                metadata.color_matrix = Some(matrix.into());
                metadata.color_range = Some(range.into());
                let q = query(metadata.clone());
                let policy = q.video_color.unwrap();
                assert!(policy.assumptions.is_empty());
                assert!(q.unsupported_reason.is_none());
                assert_eq!((&*policy.transfer, &*policy.range), (transfer, range));
            }
        }
        for field in [
            "primaries",
            "transfer",
            "matrix",
            "range",
            "format",
            "pq",
            "hlg",
            "wide_primaries",
            "wrong_matrix",
            "rgb_limited",
            "missing_format",
        ] {
            if field == "rgb_limited" && !rgb {
                continue;
            }
            let mut bad = metadata.clone();
            match field {
                "primaries" => bad.color_primaries = Some("future-primaries-v2".into()),
                "transfer" => bad.color_transfer = Some("future-transfer-v2".into()),
                "matrix" => bad.color_matrix = Some("future-matrix-v2".into()),
                "range" => bad.color_range = Some("future-range-v2".into()),
                "format" => bad.pixel_format = Some("future-format-v2".into()),
                "pq" => bad.color_transfer = Some("smpte2084".into()),
                "hlg" => bad.color_transfer = Some("arib-std-b67".into()),
                "wide_primaries" => bad.color_primaries = Some("bt2020".into()),
                "wrong_matrix" => bad.color_matrix = Some(if rgb { "bt709" } else { "gbr" }.into()),
                "rgb_limited" => bad.color_range = Some("tv".into()),
                _ => bad.pixel_format = None,
            }
            assert_eq!(
                kronello_media::video_color_policy(&bad).unwrap_err().code(),
                "UNSUPPORTED_FEATURE"
            );
            let q = query(bad);
            assert!(q.video_color.is_none());
            assert!(
                q.unsupported_reason
                    .unwrap()
                    .starts_with("UNSUPPORTED_FEATURE:")
            );
        }
    }
}

#[test]
fn untagged_rgb_video_uses_srgb_inverse_transfer_and_linear_rec2020_matrix() {
    let dir = tempfile::tempdir().unwrap();
    let raw = dir.path().join("rgb.raw");
    let path = dir.path().join("rgb.nut");
    std::fs::write(&raw, [128_u8, 64, 32].repeat(16 * 16 * 2)).unwrap();
    let status = std::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "rawvideo",
            "-pixel_format",
            "rgb24",
            "-video_size",
            "16x16",
            "-framerate",
            "24",
            "-i",
        ])
        .arg(&raw)
        .args([
            "-frames:v",
            "2",
            "-threads",
            "1",
            "-c:v",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "-f",
            "nut",
        ])
        .arg(&path)
        .status()
        .unwrap();
    assert!(status.success());
    let runtime = MediaRuntime::load().unwrap();
    let a = asset(&runtime, &path, 0);
    let policy = kronello_media::video_color_policy(&a.streams[0]).unwrap();
    assert_eq!(policy.transfer, "iec61966-2-1");
    assert!(
        policy
            .assumptions
            .contains(&"untagged transfer: iec61966-2-1".into())
    );
    let expected = [128_u8, 64, 32].map(|v| {
        let v = f64::from(v) / 255.0;
        ((v + 0.055) / 1.055).powf(2.4)
    });
    let rec709 = runtime
        .decode_video_image(&a, &path, 0, Time::ZERO, ColorSpace::LinearRec709)
        .unwrap();
    for pixel in rec709.pixels {
        for i in 0..3 {
            assert!((f64::from(pixel[i]) - expected[i]).abs() < 1e-6);
        }
        assert_eq!(pixel[3], 1.0);
    }
    let matrix = [
        [0.6274039, 0.3292830, 0.0433131],
        [0.0690973, 0.9195404, 0.0113623],
        [0.0163914, 0.0880133, 0.8955953],
    ];
    let rec2020 = runtime
        .decode_video_image(&a, &path, 0, Time::ZERO, ColorSpace::LinearRec2020)
        .unwrap();
    for pixel in rec2020.pixels {
        for i in 0..3 {
            let expected = (0..3).map(|j| matrix[i][j] * expected[j]).sum::<f64>();
            assert!((f64::from(pixel[i]) - expected).abs() < 1e-6);
        }
        assert_eq!(pixel[3], 1.0);
    }
}

#[test]
fn video_explicit_stream_color_assumptions_and_typed_failures() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("two.nut");
    let status = std::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=red:size=16x16:rate=24",
            "-f",
            "lavfi",
            "-i",
            "color=c=blue:size=16x16:rate=24",
            "-map",
            "0:v",
            "-map",
            "1:v",
            "-frames:v",
            "2",
            "-threads",
            "1",
            "-c:v",
            "rawvideo",
            "-pix_fmt",
            "yuv420p",
            "-f",
            "nut",
        ])
        .arg(&path)
        .status()
        .unwrap();
    assert!(status.success());
    let runtime = MediaRuntime::load().unwrap();
    let a = asset(&runtime, &path, 1);
    let mut c = clip([0; 3], 0, 1);
    c.source_ref = SourceRef::Asset {
        asset: a.id,
        stream_index: 1,
    };
    c.time_map = TimeMap::linear(Time::ZERO, t(1, 24)).unwrap();
    let mut p = project(sequence(vec![c]));
    p.assets.push(DocumentObject::Known(a.clone()));
    let snap = snapshot(&p);
    let backend = VideoRenderBackend {
        backend: &CpuReferenceBackend,
        project_path: &path,
    };
    let frame = render_frame(
        &snap,
        &[],
        &backend,
        FrameRequest {
            time: Time::ZERO,
            region: region(),
        },
    )
    .unwrap();
    assert!(frame.pixels.linear[0][2] > 0.8 && frame.pixels.linear[0][0] < 0.01);
    assert!(runtime.open_video_stream(&path, 99).is_err());
    let (_project_dir, project_path) = setup(p.clone());
    let ResultData::Timeline(query) = service()
        .dispatch(Request::SequenceQuery(SequenceQueryRequest {
            project: project_path,
            sequence: match &p.sequences[0] {
                DocumentObject::Known(s) => s.id,
                _ => panic!(),
            },
        }))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(query.clips[0].kind, ClipKind::Video);
    let policy = query.clips[0].video_color.as_ref().unwrap();
    assert!(!policy.assumptions.is_empty());
    assert_eq!(policy.transfer, "bt709");
    assert_eq!(policy.range, "tv");
    std::fs::write(&path, b"changed source").unwrap();
    assert_eq!(
        render_frame(
            &snap,
            &[],
            &backend,
            FrameRequest {
                time: Time::ZERO,
                region: region()
            }
        )
        .unwrap_err()
        .code(),
        "ASSET_HASH_MISMATCH"
    );
    std::fs::remove_file(&path).unwrap();
    assert_eq!(
        render_frame(
            &snap,
            &[],
            &backend,
            FrameRequest {
                time: Time::ZERO,
                region: region()
            }
        )
        .unwrap_err()
        .code(),
        "ASSET_MISSING"
    );
    for file in ["hdr-pq.mkv", "hdr-hlg.mkv"] {
        let path = fixtures().join(file);
        let a = asset(&runtime, &path, 0);
        assert_eq!(
            runtime
                .decode_video_image(&a, &path, 0, Time::ZERO, ColorSpace::LinearRec709)
                .unwrap_err()
                .code(),
            "UNSUPPORTED_FEATURE"
        );
    }
}

fn effects_fixture() -> Project {
    let mut c = clip([255, 0, 0], 1, 4);
    let f = |v| FiniteF64::new(v).unwrap();
    let position = property("kronello.transform.position", Value::Vec2([f(4.0), f(3.0)]));
    let scale = property("kronello.transform.scale", Value::Vec2([f(0.5), f(0.5)]));
    let sigma = property("kronello.effect.sigma", Value::Scalar(f(1.0)));
    c.effects = vec![Effect::Known(EffectDefinition {
        effect_id: GAUSSIAN_BLUR_ID.into(),
        version: 1,
        parameters: EffectParameters::GaussianBlur { sigma: sigma.id() },
    })];
    c.properties = vec![position, scale, sigma];
    let mut s = sequence(vec![clip([0, 0, 255], 0, 5)]);
    s.tracks.push(Track {
        state: None,
        id: TrackId::new(),
        kind: TrackKind::Video,
        clips: vec![c],
    });
    project(s)
}
fn video_effects_fixture(runtime: &MediaRuntime, path: &Path) -> Project {
    let a = asset(runtime, path, 0);
    let mut p = effects_fixture();
    let DocumentObject::Known(s) = &mut p.sequences[0] else {
        panic!()
    };
    s.tracks[1].clips[0].source_ref = SourceRef::Asset {
        asset: a.id,
        stream_index: 0,
    };
    s.tracks[1].clips[0].time_map = TimeMap::linear(Time::ZERO, t(1, 24)).unwrap();
    p.assets.push(DocumentObject::Known(a));
    p
}
#[test]
fn explicit_frame_cpu_backend_matches_session_cpu_video_pixels_and_is_strict() {
    let runtime = MediaRuntime::load().unwrap();
    let p = video_effects_fixture(&runtime, &fixtures().join("cfr-24-1.nut"));
    let DocumentObject::Known(s) = &p.sequences[0] else {
        panic!()
    };
    let id = s.id;
    let (_dir, path) = setup(p);
    let request = FrameRenderRequest {
        backend: Some(BackendSelection::CpuReference),
        input: RenderInput {
            project: path,
            composition: None,
            target: Some(RenderTarget::Sequence { sequence: id }),
            region: crop_region(),
            profile: Default::default(),
            fonts: vec![],
            media_proxies: kronello_render::MediaProxyMode::Off,
        },
        time: t(2, 1),
    };
    let explicit = Service::new(BackendSelection::Gpu)
        .render_requested_frame(&request)
        .unwrap();
    let mut omitted = request.clone();
    omitted.backend = None;
    let session = Service::new(BackendSelection::CpuReference)
        .render_requested_frame(&omitted)
        .unwrap();
    assert_eq!(explicit, session);
    assert_eq!(explicit.metadata.backend, "cpu_reference_float32");
    assert!(
        explicit
            .metadata
            .input_path
            .contains("software_video_decode")
    );
    let mut value = serde_json::to_value(Request::RenderFrame(request)).unwrap();
    value["backend"] = serde_json::json!("automatic");
    assert!(serde_json::from_value::<Request>(value).is_err());
}
fn crop_region() -> OutputRegion {
    OutputRegion {
        origin: [5.0, 2.0],
        extent: [7.0, 8.0],
        pixels: [7, 8],
    }
}
#[test]
fn video_clip_effects_transform_halo_roi_and_order_cpu() {
    let runtime = MediaRuntime::load().unwrap();
    let path = fixtures().join("cfr-24-1.nut");
    let p = video_effects_fixture(&runtime, &path);
    let snap = snapshot(&p);
    let backend = VideoRenderBackend {
        backend: &CpuReferenceBackend,
        project_path: &path,
    };
    let render = |snap: &RenderSnapshot, region| {
        render_frame(
            snap,
            &[],
            &backend,
            FrameRequest {
                time: t(2, 1),
                region,
            },
        )
        .unwrap()
    };
    let frame = render(&snap, region());
    let crop = render(&snap, crop_region());
    for y in 0..8 {
        for x in 0..7 {
            assert_eq!(
                crop.pixels.linear[y * 7 + x],
                frame.pixels.linear[(y + 2) * 16 + x + 5]
            );
        }
    }
    let scene = build_scene_ir(&snap, t(2, 1), &[]).unwrap();
    assert_eq!(
        scene.nodes[1].world_transform.0,
        [[0.5, 0.0, 4.0], [0.0, 0.5, 3.0]]
    );
    assert!(
        build_render_dag(&scene, snap.profile(), region())
            .unwrap()
            .execution_region()
            .pixels[0]
            > 16
    );
    assert!(
        frame
            .pixels
            .linear
            .iter()
            .any(|v| *v != [0.0, 0.0, 1.0, 1.0])
    );
    let mut reversed = p;
    let DocumentObject::Known(s) = &mut reversed.sequences[0] else {
        panic!()
    };
    s.tracks.reverse();
    assert!(
        render(&snapshot(&reversed), region())
            .pixels
            .linear
            .iter()
            .all(|v| *v == [0.0, 0.0, 1.0, 1.0])
    );
}
#[test]
fn clip_effects_transform_halo_roi_and_compositing_cpu() {
    let snap = snapshot(&effects_fixture());
    let scene = build_scene_ir(&snap, t(2, 1), &[]).unwrap();
    let dag = build_render_dag(&scene, snap.profile(), region()).unwrap();
    assert!(
        dag.nodes()
            .iter()
            .any(|n| matches!(n, kronello_render::DagNode::Effect { .. }))
    );
    assert!(dag.execution_region().pixels[0] > 16);
    assert_eq!(
        scene.nodes[1].world_transform.0,
        [[0.5, 0.0, 4.0], [0.0, 0.5, 3.0]]
    );
    let frame = cpu(&snap, t(2, 1));
    assert!(frame.pixels.linear[7 * 16 + 8][0] > 0.99);
    assert!(frame.pixels.linear[2 * 16 + 8][0] > 0.0);
    assert!(frame.pixels.linear[0][2] > 0.99);
    let crop = render_frame(
        &snap,
        &[],
        &CpuReferenceBackend,
        FrameRequest {
            time: t(2, 1),
            region: OutputRegion {
                origin: [5.0, 2.0],
                extent: [7.0, 8.0],
                pixels: [7, 8],
            },
        },
    )
    .unwrap();
    for y in 0..8 {
        for x in 0..7 {
            assert_eq!(
                crop.pixels.linear[y * 7 + x],
                frame.pixels.linear[(y + 2) * 16 + x + 5]
            );
        }
    }
    let mut swapped = effects_fixture();
    let DocumentObject::Known(s) = &mut swapped.sequences[0] else {
        panic!()
    };
    s.tracks.reverse();
    assert!(
        cpu(&snapshot(&swapped), t(2, 1))
            .pixels
            .linear
            .iter()
            .all(|p| *p == [0.0, 0.0, 1.0, 1.0])
    );
    let mut opaque = effects_fixture();
    let DocumentObject::Known(s) = &mut opaque.sequences[0] else {
        panic!()
    };
    s.tracks[1].clips[0].effects = vec![Effect::Opaque(serde_json::json!({"future":"effect"}))];
    assert_eq!(
        render_frame(
            &snapshot(&opaque),
            &[],
            &CpuReferenceBackend,
            FrameRequest {
                time: t(2, 1),
                region: region()
            }
        )
        .unwrap_err()
        .code(),
        "UNSUPPORTED_FEATURE"
    );
}

#[test]
fn gpu_nle2_clip_effects_and_explicit_video_upload_match_cpu() {
    let gpu = kronello_gpu::GpuContext::new().unwrap();
    let snap = snapshot(&effects_fixture());
    let expected = cpu(&snap, t(2, 1));
    let actual = render_frame(
        &snap,
        &[],
        &gpu,
        FrameRequest {
            time: t(2, 1),
            region: region(),
        },
    )
    .unwrap();
    assert_eq!(actual.metadata.backend, "wgpu_rgba16f");
    for (a, b) in actual.pixels.linear.iter().zip(expected.pixels.linear) {
        for i in 0..4 {
            assert!((a[i] - b[i]).abs() < 0.003);
        }
    }
    let runtime = MediaRuntime::load().unwrap();
    let path = fixtures().join("cfr-24-1.nut");
    let p = video_effects_fixture(&runtime, &path);
    let snap = snapshot(&p);
    let cpu_backend = VideoRenderBackend {
        backend: &CpuReferenceBackend,
        project_path: &path,
    };
    let gpu_backend = VideoRenderBackend {
        backend: &gpu,
        project_path: &path,
    };
    let expected = render_frame(
        &snap,
        &[],
        &cpu_backend,
        FrameRequest {
            time: t(2, 1),
            region: region(),
        },
    )
    .unwrap();
    let actual = render_frame(
        &snap,
        &[],
        &gpu_backend,
        FrameRequest {
            time: t(2, 1),
            region: region(),
        },
    )
    .unwrap();
    for (a, b) in actual.pixels.linear.iter().zip(expected.pixels.linear) {
        for i in 0..4 {
            assert!((a[i] - b[i]).abs() < 0.003);
        }
    }
    let crop = render_frame(
        &snap,
        &[],
        &gpu_backend,
        FrameRequest {
            time: t(2, 1),
            region: crop_region(),
        },
    )
    .unwrap();
    for y in 0..8 {
        for x in 0..7 {
            for i in 0..4 {
                assert!(
                    (crop.pixels.linear[y * 7 + x][i]
                        - actual.pixels.linear[(y + 2) * 16 + x + 5][i])
                        .abs()
                        < 0.003
                );
            }
        }
    }
    let scene = build_scene_ir(&snap, t(2, 1), &[]).unwrap();
    let dag = build_render_dag(&scene, snap.profile(), region())
        .unwrap()
        .resolve_video(|a, i, t, w, reverse| {
            assert!(!reverse);
            runtime.decode_video_image(a, &path, i, t, w).map_err(|e| {
                kronello_render::RenderError::Backend {
                    code: e.code(),
                    message: e.to_string(),
                }
            })
        })
        .unwrap();
    let pixels = dag.execution_region().pixels;
    let raster = dag
        .nodes()
        .iter()
        .find_map(|n| match n {
            kronello_render::DagNode::RasterInput { pixels } => Some(pixels.clone()),
            _ => None,
        })
        .unwrap();
    let report = gpu
        .render_scene(
            kronello_gpu::RenderSize {
                design_extent: pixels.map(|v| v as f32),
                output_resolution: pixels,
            },
            &kronello_gpu::DrawScene {
                nodes: vec![kronello_gpu::DrawNode::Raster(raster)],
                roots: vec![0],
            },
            kronello_gpu::WorkingSpace::LinearRec709,
        )
        .unwrap();
    assert_eq!(report.transfers.cpu_upload_pixel_operations, 1);
    assert_eq!(
        report.transfers.cpu_upload_pixel_bytes,
        u64::from(pixels[0]) * u64::from(pixels[1]) * 8
    );
}

#[test]
fn translucent_crossfade_is_same_track_source_over_with_explicit_overlap() {
    let mut a = clip([255, 0, 0], 0, 3);
    let mut b = clip([0, 0, 255], 2, 5);
    for (c, rgb, alpha) in [
        (&mut a, [1.0, 0.0, 0.0], 0.5),
        (&mut b, [0.0, 0.0, 1.0], 0.25),
    ] {
        c.source_ref = SourceRef::Generator {
            generator: SOLID_GENERATOR_ID.into(),
            version: 1,
            color: Color::new(ColorSpace::LinearRec709, rgb, alpha).unwrap(),
        };
    }
    // Storage order cannot override outgoing/incoming timeline order.
    let mut s = sequence(vec![b.clone(), a.clone()]);
    s.transitions.push(Transition {
        outgoing: a.id,
        incoming: b.id,
        range: range(t(2, 1), t(3, 1)),
        kind: TransitionKind::Crossfade,
        params: None,
        version: 1,
    });
    let p = project(s.clone());
    let snap = snapshot(&p);
    assert_eq!(cpu(&snap, t(2, 1)).pixels.linear[0], [0.5, 0.0, 0.0, 0.5]);
    assert_eq!(
        cpu(&snap, t(5, 2)).pixels.linear[0],
        [0.4375, 0.0, 0.125, 0.5625]
    );
    assert_eq!(cpu(&snap, t(3, 1)).pixels.linear[0], [0.0, 0.0, 0.25, 0.25]);
    let mut bad = s.clone();
    bad.transitions[0].range = range(t(2, 1), t(5, 2));
    assert_eq!(bad.validate(&p).unwrap_err().code(), "INVALID_CLIP");
    let mut bad = s.clone();
    let outgoing = bad.tracks[0].clips.pop().unwrap();
    bad.tracks.push(Track {
        state: None,
        id: TrackId::new(),
        kind: TrackKind::Video,
        clips: vec![outgoing],
    });
    assert_eq!(bad.validate(&p).unwrap_err().code(), "INVALID_CLIP");
    let mut bad = s;
    bad.tracks[0].clips.push(clip([0, 255, 0], 2, 3));
    assert_eq!(bad.validate(&p).unwrap_err().code(), "INVALID_CLIP");
}

#[test]
fn transitions_plan_apply_idempotency_selective_undo_and_reload() {
    let a = clip([255, 0, 0], 0, 3);
    let b = clip([0, 0, 255], 2, 5);
    let mut s = sequence(vec![a.clone(), b.clone()]);
    let tr = Transition {
        outgoing: a.id,
        incoming: b.id,
        range: range(t(2, 1), t(3, 1)),
        kind: TransitionKind::Crossfade,
        params: None,
        version: 1,
    };
    s.transitions.push(tr.clone());
    let id = s.id;
    let (_dir, path) = setup(project(s));
    let snap = snapshot(&export(&path).document);
    assert_eq!(cpu(&snap, t(2, 1)).pixels.linear[0], [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(cpu(&snap, t(5, 2)).pixels.linear[0], [0.5, 0.0, 0.5, 1.0]);
    assert_eq!(cpu(&snap, t(3, 1)).pixels.linear[0], [0.0, 0.0, 1.0, 1.0]);
    reject(
        &path,
        vec![TimelineCommand::TransitionRemove {
            sequence: id,
            outgoing: a.id,
            incoming: b.id,
        }],
        "CLIP_OVERLAP",
    );
    reject(
        &path,
        vec![TimelineCommand::ClipMove {
            sequence: id,
            clip: b.id,
            delta: t(1, 1),
            linked: false,
        }],
        "TRANSITION_EDIT_CONFLICT",
    );
    let initial = export(&path).document;
    let request = apply_request(
        &path,
        vec![
            TimelineCommand::TransitionRemove {
                sequence: id,
                outgoing: a.id,
                incoming: b.id,
            },
            TimelineCommand::ClipMove {
                sequence: id,
                clip: b.id,
                delta: t(1, 1),
                linked: false,
            },
        ],
        "remove-and-move",
    );
    let ResultData::Edit(event) = service()
        .dispatch(Request::EditApply(request.clone()))
        .unwrap()
    else {
        panic!()
    };
    let ResultData::Edit(repeated) = service().dispatch(Request::EditApply(request)).unwrap()
    else {
        panic!()
    };
    assert_eq!(event.id, repeated.id);
    assert_eq!(export(&path).revision, event.revision.to_string());
    assert!(event.changed_keys.iter().any(|k|matches!(k,kronello_store::ChangedKey::Structure{object_id,parent_container_id} if *object_id==id.as_uuid() && *parent_container_id==id.as_uuid())));
    let after = export(&path).document;
    let changed = apply(
        &path,
        vec![TimelineCommand::ClipMove {
            sequence: id,
            clip: a.id,
            delta: t(-1, 1),
            linked: false,
        }],
        "later-move",
    );
    assert_eq!(undo(&path, event.id).unwrap_err().code, "UNDO_CONFLICT");
    undo(&path, changed.id).unwrap();
    assert_eq!(export(&path).document, after);
    // ADR-0026 also treats a still-active inverse as a conflicting event.
    assert_eq!(undo(&path, event.id).unwrap_err().code, "UNDO_CONFLICT");
    let (_fresh_dir, path) = setup(initial.clone());
    let event = apply(
        &path,
        vec![
            TimelineCommand::TransitionRemove {
                sequence: id,
                outgoing: a.id,
                incoming: b.id,
            },
            TimelineCommand::ClipMove {
                sequence: id,
                clip: b.id,
                delta: t(1, 1),
                linked: false,
            },
        ],
        "fresh-transition-edit",
    );
    // An independent sequence can change after this edit without blocking Undo.
    let independent = sequence(vec![clip([0, 255, 0], 0, 2)]);
    apply(
        &path,
        vec![TimelineCommand::SequenceCreate {
            sequence: independent.clone(),
        }],
        "other-sequence",
    );
    undo(&path, event.id).unwrap();
    let mut expected = initial;
    expected.sequences.push(DocumentObject::Known(independent));
    assert_eq!(export(&path).document, expected);
    // Render the restored overlap after independent reopen/export calls.
    assert_eq!(
        cpu(&snapshot(&export(&path).document), t(5, 2))
            .pixels
            .linear[0],
        [0.5, 0.0, 0.5, 1.0]
    );
}

#[test]
fn ripple_and_transitive_linked_move_are_atomic_and_keep_source_time() {
    let a = clip([255, 0, 0], 0, 2);
    let b = clip([0, 0, 255], 3, 5);
    let c = clip([0, 255, 0], 3, 5);
    let mut s = sequence(vec![a.clone(), b.clone()]);
    s.tracks.push(Track {
        state: None,
        id: TrackId::new(),
        kind: TrackKind::Video,
        clips: vec![c.clone()],
    });
    let id = s.id;
    let track = s.tracks[0].id;
    let (_dir, path) = setup(project(s));
    let initial = export(&path).document;
    for (tracks, pivot) in [
        (vec![], t(3, 1)),
        (vec![track], t(6, 1)),
        (vec![TrackId::new()], t(3, 1)),
    ] {
        reject(
            &path,
            vec![TimelineCommand::Ripple {
                sequence: id,
                tracks,
                pivot,
                delta: t(1, 1),
                linked: true,
            }],
            "INVALID_CLIP",
        );
    }
    let link = apply(
        &path,
        vec![TimelineCommand::ClipLink {
            sequence: id,
            clips: vec![b.id, c.id],
        }],
        "link",
    );
    reject(
        &path,
        vec![TimelineCommand::ClipMove {
            sequence: id,
            clip: b.id,
            delta: t(1, 1),
            linked: false,
        }],
        "LINKED_EDIT_REQUIRED",
    );
    reject(
        &path,
        vec![TimelineCommand::Ripple {
            sequence: id,
            tracks: vec![track],
            pivot: t(1, 1),
            delta: t(1, 1),
            linked: true,
        }],
        "INVALID_CLIP",
    );
    reject(
        &path,
        vec![TimelineCommand::Ripple {
            sequence: id,
            tracks: vec![track],
            pivot: t(3, 1),
            delta: t(1, 1),
            linked: false,
        }],
        "LINKED_EDIT_REQUIRED",
    );
    reject(
        &path,
        vec![TimelineCommand::Ripple {
            sequence: id,
            tracks: vec![track],
            pivot: t(3, 1),
            delta: t(-2, 1),
            linked: true,
        }],
        "CLIP_OVERLAP",
    );
    let linked = export(&path).document;
    let ripple = apply(
        &path,
        vec![TimelineCommand::Ripple {
            sequence: id,
            tracks: vec![track],
            pivot: t(3, 1),
            delta: t(1, 1),
            linked: true,
        }],
        "ripple",
    );
    let p = export(&path).document;
    let DocumentObject::Known(s) = &p.sequences[0] else {
        panic!()
    };
    let selected = BTreeSet::from([b.id, c.id]);
    let shifted: Vec<_> = s
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .filter(|clip| selected.contains(&clip.id))
        .collect();
    assert_eq!(shifted.len(), 2);
    for shifted in shifted {
        assert_eq!(shifted.timeline_range, range(t(4, 1), t(6, 1)));
        assert_eq!(shifted.source_in, Time::ZERO);
        assert_eq!(shifted.local_time(t(9, 2)).unwrap(), t(1, 2));
    }
    let moved: BTreeSet<_> = ripple
        .changed_keys
        .iter()
        .filter_map(|k| match k {
            kronello_store::ChangedKey::Structure { object_id, .. } => Some(*object_id),
            _ => None,
        })
        .collect();
    assert!(moved.contains(&b.id.as_uuid()) && moved.contains(&c.id.as_uuid()));
    assert_eq!(undo(&path, link.id).unwrap_err().code, "UNDO_CONFLICT");
    undo(&path, ripple.id).unwrap();
    assert_eq!(export(&path).document, linked);
    let moved = apply(
        &path,
        vec![TimelineCommand::ClipMove {
            sequence: id,
            clip: b.id,
            delta: t(-1, 1),
            linked: true,
        }],
        "move-component",
    );
    undo(&path, moved.id).unwrap();
    assert_eq!(export(&path).document, linked);
    assert_eq!(undo(&path, link.id).unwrap_err().code, "UNDO_CONFLICT");
    let (_fresh_dir, path) = setup(initial.clone());
    let link = apply(
        &path,
        vec![TimelineCommand::ClipLink {
            sequence: id,
            clips: vec![b.id, c.id],
        }],
        "fresh-link",
    );
    undo(&path, link.id).unwrap();
    assert_eq!(export(&path).document, initial);
}

#[test]
fn gui007_track_output_is_atomic_rendered_and_undoable() {
    let seq = sequence(vec![clip([255, 0, 0], 0, 2)]);
    let id = seq.id;
    let track = seq.tracks[0].id;
    let (_dir, path) = setup(project(seq));
    let initial = export(&path);
    assert!(cpu(&snapshot(&initial.document), Time::ZERO).pixels.linear[0][0] > 0.99);
    let hidden = apply(
        &path,
        vec![TimelineCommand::TrackStateSet {
            sequence: id,
            track,
            state: TrackState {
                visible: false,
                muted: false,
            },
        }],
        "hide-track",
    );
    let after = export(&path);
    assert_eq!(
        cpu(&snapshot(&after.document), Time::ZERO).pixels.linear[0],
        [0.0; 4]
    );
    undo(&path, hidden.id).unwrap();
    assert!(
        cpu(&snapshot(&export(&path).document), Time::ZERO)
            .pixels
            .linear[0][0]
            > 0.99
    );
    let pending = apply_request(
        &path,
        vec![TimelineCommand::TrackStateSet {
            sequence: id,
            track,
            state: TrackState {
                visible: false,
                muted: true,
            },
        }],
        "stale-track",
    );
    apply(
        &path,
        vec![TimelineCommand::TrackStateSet {
            sequence: id,
            track,
            state: TrackState {
                visible: true,
                muted: true,
            },
        }],
        "track-other",
    );
    assert_eq!(
        service()
            .dispatch(Request::EditApply(pending))
            .unwrap_err()
            .code,
        "REVISION_CONFLICT"
    );
}

#[test]
fn gui007_time_set_preserves_rationals_and_rejects_invalid_candidate_atomically() {
    let seq = sequence(vec![clip([0, 255, 0], 0, 2)]);
    let id = seq.id;
    let clip_id = seq.tracks[0].clips[0].id;
    let (_dir, path) = setup(project(seq));
    let initial = export(&path);
    let event = apply(
        &path,
        vec![TimelineCommand::ClipTimeSet {
            sequence: id,
            clip: clip_id,
            source_in: t(1001, 30000),
            time_map: TimeMap::linear(Time::ZERO, t(1, 2)).unwrap(),
            audio_retime: AudioRetimePolicy::ResampleV1,
            reverse_sampling: None,
        }],
        "time-exact",
    );
    let current = export(&path);
    let DocumentObject::Known(seq) = &current.document.sequences[0] else {
        panic!()
    };
    assert_eq!(seq.tracks[0].clips[0].source_in, t(1001, 30000));
    assert_eq!(
        seq.tracks[0].clips[0].timeline_range,
        TimeRange::new(Time::ZERO, t(2, 1)).unwrap()
    );
    reject(
        &path,
        vec![TimelineCommand::ClipTimeSet {
            sequence: id,
            clip: clip_id,
            source_in: t(-1, 30000),
            time_map: TimeMap::linear(Time::ZERO, Time::ONE).unwrap(),
            audio_retime: AudioRetimePolicy::Reject,
            reverse_sampling: None,
        }],
        "INVALID_CLIP",
    );
    undo(&path, event.id).unwrap();
    assert_eq!(export(&path).document, initial.document);
}

#[test]
fn gui007_reverse_video_first_last_pixels_shared_edit_undo_and_bounds() {
    let runtime = MediaRuntime::load().unwrap();
    for (file, upper, last_start) in [
        ("cfr-30000-1001.nut", t(6006, 30000), t(5005, 30000)),
        ("vfr.nut", t(16, 30), t(15, 30)),
        ("bframes.nut", t(7, 24), t(6, 24)),
    ] {
        let source_path = fixtures().join(file);
        let mut asset = asset(&runtime, &source_path, 0);
        let origin = asset.streams[0].start_time.unwrap_or(Time::ZERO);
        let duration = upper.checked_sub(origin).unwrap();
        // NUT fixtures omit container duration; explicitly lock the tested decoder endpoint.
        assert_eq!(
            runtime
                .open_video(&source_path)
                .unwrap()
                .decode_at(last_start)
                .unwrap()
                .end,
            upper
        );
        asset.streams[0].duration = Some(duration);
        let mut authored = clip([0; 3], 0, 1);
        authored.source_ref = SourceRef::Asset {
            asset: asset.id,
            stream_index: 0,
        };
        authored.timeline_range = TimeRange::new(Time::ZERO, duration).unwrap();
        authored.source_in = origin;
        let seq = sequence(vec![authored.clone()]);
        let id = seq.id;
        let mut project = project(seq);
        project.assets.push(DocumentObject::Known(asset.clone()));
        let (_dir, path) = setup(project.clone());
        let event = apply(
            &path,
            vec![TimelineCommand::ClipTimeSet {
                sequence: id,
                clip: authored.id,
                source_in: upper,
                time_map: TimeMap::linear(Time::ZERO, Time::ONE).unwrap(),
                audio_retime: AudioRetimePolicy::ReverseResampleV1,
                reverse_sampling: Some(ReverseSampling::ReverseGridV1),
            }],
            "reverse-video",
        );
        let snap = snapshot(&export(&path).document);
        let backend = VideoRenderBackend {
            backend: &CpuReferenceBackend,
            project_path: &path,
        };
        for (time, source_time) in [
            (Time::ZERO, last_start),
            (duration.checked_sub(t(1, 30000)).unwrap(), origin),
        ] {
            let frame = render_frame(
                &snap,
                &[],
                &backend,
                FrameRequest {
                    time,
                    region: region(),
                },
            )
            .unwrap();
            let expected = runtime
                .decode_video_image(&asset, &path, 0, source_time, ColorSpace::LinearRec709)
                .unwrap();
            for (actual, expected) in frame.pixels.linear.iter().zip(expected.pixels) {
                for channel in 0..4 {
                    assert!(
                        (actual[channel] - expected[channel]).abs() < 1e-5,
                        "{file} {time:?}"
                    );
                }
            }
        }
        reject(
            &path,
            vec![TimelineCommand::ClipTimeSet {
                sequence: id,
                clip: authored.id,
                source_in: upper.checked_add(t(1, 1)).unwrap(),
                time_map: TimeMap::linear(Time::ZERO, Time::ONE).unwrap(),
                audio_retime: AudioRetimePolicy::ReverseResampleV1,
                reverse_sampling: Some(ReverseSampling::ReverseGridV1),
            }],
            "INVALID_CLIP",
        );
        undo(&path, event.id).unwrap();
        assert_eq!(export(&path).document, project);
    }
}

#[test]
fn transition_set_round_trips_wipe_slide_dip_and_rejects_param_mismatch() {
    let outgoing = clip([255, 0, 0], 0, 3);
    let incoming = clip([0, 0, 255], 1, 5);
    let mut s = sequence(vec![outgoing.clone(), incoming.clone()]);
    // Overlapping clips are only valid when a transition covers the overlap.
    let overlap = range(t(1, 1), t(3, 1));
    s.transitions = vec![Transition {
        outgoing: outgoing.id,
        incoming: incoming.id,
        range: overlap,
        kind: TransitionKind::Crossfade,
        params: None,
        version: 1,
    }];
    let id = s.id;
    let (_dir, path) = setup(project(s));
    let set =
        |kind: TransitionKind, params: Option<TransitionParams>| TimelineCommand::TransitionSet {
            sequence: id,
            transition: Transition {
                outgoing: outgoing.id,
                incoming: incoming.id,
                range: overlap,
                kind,
                params,
                version: 1,
            },
        };
    for (key, kind, params) in [
        (
            "wipe-left",
            TransitionKind::Wipe,
            Some(TransitionParams::Wipe(WipeParams {
                direction: TransitionDirection::Left,
            })),
        ),
        (
            "slide-up",
            TransitionKind::Slide,
            Some(TransitionParams::Slide(SlideParams {
                direction: TransitionDirection::Up,
            })),
        ),
        (
            "dip-black",
            TransitionKind::Dip,
            Some(TransitionParams::Dip(DipParams {
                color: Color::from_srgb8([0, 0, 0], None),
            })),
        ),
    ] {
        apply(&path, vec![set(kind, params)], key);
        let exported = export(&path);
        let DocumentObject::Known(s) = &exported.document.sequences[0] else {
            panic!()
        };
        assert_eq!(s.transitions.len(), 1);
        assert_eq!(s.transitions[0].kind, kind);
        assert_eq!(s.transitions[0].params, params);
    }
    // Kind/params mismatch and a foreign payload are INVALID_CLIP failures.
    reject(&path, vec![set(TransitionKind::Wipe, None)], "INVALID_CLIP");
    reject(
        &path,
        vec![set(
            TransitionKind::Crossfade,
            Some(TransitionParams::Dip(DipParams {
                color: Color::from_srgb8([0, 0, 0], None),
            })),
        )],
        "INVALID_CLIP",
    );
    // An unsupported transition version is a typed rejection, not a fallback.
    let mut unsupported = set(TransitionKind::Crossfade, None);
    let TimelineCommand::TransitionSet { transition, .. } = &mut unsupported else {
        panic!()
    };
    transition.version = 2;
    reject(&path, vec![unsupported], "UNSUPPORTED_FEATURE");
    // Re-set a wipe and confirm the rendered midpoint has a spatially split
    // frame: left columns show the incoming clip, right columns the outgoing.
    apply(
        &path,
        vec![set(
            TransitionKind::Wipe,
            Some(TransitionParams::Wipe(WipeParams {
                direction: TransitionDirection::Left,
            })),
        )],
        "wipe-final",
    );
    let exported = export(&path);
    let snapshot = snapshot(&exported.document);
    let frame = cpu(&snapshot, t(2, 1)).pixels.linear;
    let (left, right) = (frame[0], frame[15]);
    assert!(
        left[2] > 0.9 && left[0] < 0.1,
        "left column is incoming: {left:?}"
    );
    assert!(
        right[0] > 0.9 && right[2] < 0.1,
        "right column is outgoing: {right:?}"
    );
}
