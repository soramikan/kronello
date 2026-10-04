use kronello_eval::{
    DependencyDeclarations, EvaluationSnapshot, ReferenceBindings, evaluate_sequence,
};
use kronello_model::*;
use kronello_render::{OutputRegion, RenderSnapshot};
use kronello_service::*;
use kronello_time::{
    Duration, FrameRate, Rational, SampleRate, Time, TimeMap, TimeMapPoint, TimeRange,
};
use serde_json::{Value as Json, json};
use std::path::{Path, PathBuf};
use uuid::Uuid;

fn t(n: i64, d: i64) -> Time {
    Time::new(n, d).unwrap()
}
fn range(a: Time, b: Time) -> TimeRange {
    TimeRange::new(a, b).unwrap()
}
fn fixture() -> Project {
    let mut p: Project =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    let DocumentObject::Known(c) = &mut p.compositions[0] else {
        panic!()
    };
    c.nodes.truncate(1);
    c.root_nodes.truncate(1);
    p.texts.clear();
    p
}
fn composition(p: &Project) -> &Composition {
    let DocumentObject::Known(c) = &p.compositions[0] else {
        panic!()
    };
    c
}
fn clip(composition: CompositionId, start: Time, end: Time, speed: Rational) -> Clip {
    Clip {
        id: ClipId::new(),
        source_ref: SourceRef::Composition { composition },
        timeline_range: range(start, end),
        source_in: Time::ZERO,
        time_map: TimeMap::linear(Time::ZERO, speed).unwrap(),
        audio_retime: AudioRetimePolicy::Reject,
        links: vec![],
        effects: vec![],
    }
}
fn sequence(p: &Project) -> Sequence {
    Sequence {
        id: SequenceId::new(),
        extent: composition(p).design_extent,
        frame_rate: FrameRate::new(24, 1).unwrap(),
        audio_rate: SampleRate::HZ_48000,
        working_space: ColorSpace::LinearRec709,
        tracks: vec![
            Track {
                id: TrackId::new(),
                kind: TrackKind::Video,
                clips: vec![clip(composition(p).id, t(2, 1), t(3, 1), Rational::ONE)],
            },
            Track {
                id: TrackId::new(),
                kind: TrackKind::Video,
                clips: vec![clip(composition(p).id, t(5, 2), t(9, 2), t(1, 2))],
            },
        ],
    }
}
fn engine() -> Service<'static> {
    Service::new(BackendSelection::CpuReference)
}
fn setup(p: Project) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nle.kronello");
    engine()
        .dispatch(Request::ProjectCreate(CreateRequest {
            project: path.clone(),
            document: p,
        }))
        .unwrap();
    (dir, path)
}
fn export(path: &Path) -> ExportResult {
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
fn undo(path: &Path, event: kronello_store::Event) {
    engine()
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
    let error = engine()
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
fn region() -> OutputRegion {
    OutputRegion {
        origin: [0.0, 0.0],
        extent: [64.0, 32.0],
        pixels: [64, 32],
    }
}
fn frame(path: &Path, target: RenderTarget, time: Time) -> FrameResult {
    let ResultData::Frame(value) = engine()
        .dispatch(Request::RenderFrame(FrameRenderRequest {
            input: RenderInput {
                project: path.into(),
                composition: None,
                target: Some(target),
                region: region(),
                profile: Default::default(),
                fonts: vec![],
            },
            time,
        }))
        .unwrap()
    else {
        panic!()
    };
    *value
}

#[test]
fn shared_composition_placements_evaluate_independently_and_render_cpu_pixels() {
    let mut p = fixture();
    let s = sequence(&p);
    let id = s.id;
    p.sequences.push(DocumentObject::Known(s.clone()));
    let comps = vec![composition(&p).clone()];
    let curves: Vec<_> = p
        .curves
        .iter()
        .filter_map(|c| match c {
            DocumentObject::Known(c) => Some(c.clone()),
            _ => None,
        })
        .collect();
    let mut registry = SchemaRegistry::with_builtin();
    for d in shape_descriptors() {
        registry.register(d).unwrap();
    }
    let refs = ReferenceBindings::new();
    let deps = DependencyDeclarations::new();
    let snapshot = EvaluationSnapshot {
        compositions: &comps,
        curves: &curves,
        registry: &registry,
        reference_bindings: &refs,
        dependencies: &deps,
        working_space: s.working_space,
    };
    let times = [
        t(11, 4),
        t(9, 4),
        t(7, 2),
        t(2, 1),
        t(9, 2),
        t(3, 1),
        t(1, 1),
    ];
    let expected: Vec<_> = times
        .iter()
        .map(|time| evaluate_sequence(&snapshot, &s, *time).unwrap())
        .collect();
    for (time, values) in times.iter().zip(&expected).rev() {
        assert_eq!(&evaluate_sequence(&snapshot, &s, *time).unwrap(), values);
    }
    assert_eq!(
        expected[0].iter().map(|v| v.local_time).collect::<Vec<_>>(),
        vec![t(3, 4), t(1, 8)]
    );
    assert_eq!(expected[0][0].scene.nodes[0].transform.position, [7.0, 1.0]);
    assert_eq!(expected[0][1].scene.nodes[0].transform.position, [2.0, 1.0]);
    assert_eq!(expected[0][0].clip, s.tracks[0].clips[0].id);
    assert_ne!(expected[0][0].clip, expected[0][1].clip);
    assert_eq!(expected[5].len(), 1);
    assert!(expected[4].is_empty());
    assert!(expected[6].is_empty());
    let (_dir, path) = setup(p.clone());
    let target = RenderTarget::Sequence { sequence: id };
    let single = frame(&path, target, t(9, 4));
    let direct = frame(&path, composition(&p).id.into(), t(1, 4));
    assert_eq!(single.linear, direct.linear);
    assert_eq!(single.display, direct.display);
    let bottom = frame(&path, composition(&p).id.into(), t(3, 4));
    let top = frame(&path, composition(&p).id.into(), t(1, 8));
    let stacked = frame(&path, target, t(11, 4));
    for ((actual, bottom), top) in stacked.linear.iter().zip(&bottom.linear).zip(&top.linear) {
        for channel in 0..4 {
            assert!(
                (actual[channel] - (top[channel] + bottom[channel] * (1.0 - top[3]))).abs() < 1e-6
            );
        }
    }
    assert!(stacked.linear.iter().any(|p| p[3] == 0.9375));
    assert!(single.linear.iter().any(|p| p[3] == 0.75));
    assert!(
        frame(&path, target, t(9, 2))
            .linear
            .iter()
            .all(|p| *p == [0.0; 4])
    );
    assert_eq!(stacked.metadata.target, target);
    assert_eq!(stacked.metadata.backend, "cpu_reference_float32");
    let fixed = RenderSnapshot::for_target(&p, target, 1, Default::default()).unwrap();
    let restored: RenderSnapshot =
        serde_json::from_str(&serde_json::to_string(&fixed).unwrap()).unwrap();
    assert_eq!(
        fixed.content_hash().unwrap(),
        restored.content_hash().unwrap()
    );
    restored.validate().unwrap();
    let output = path.parent().unwrap().join("frames");
    let ResultData::Sequence(result) = engine()
        .dispatch(Request::RenderSequence(SequenceRenderRequest {
            input: RenderInput {
                project: path.clone(),
                composition: None,
                target: Some(target),
                region: region(),
                profile: Default::default(),
                fonts: vec![],
            },
            range: range(t(2, 1), t(4, 1)),
            frame_rate: FrameRate::new(2, 1).unwrap(),
            output_directory: output.clone(),
        }))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(result.frames.len(), 4);
    assert!(result.frames.iter().all(|f| f.metadata.target == target));
    assert!(output.join("sequence.json").exists());
    let same = frame(&path, target, t(9, 4));
    assert_eq!(single.linear, same.linear);
}

#[test]
fn trim_preserves_speed_and_source_content_then_undo_restores_document() {
    let mut p = fixture();
    let mut s = sequence(&p);
    s.tracks.truncate(1);
    let id = s.id;
    let original = s.tracks[0].clips[0].clone();
    p.sequences.push(DocumentObject::Known(s));
    let (_dir, path) = setup(p.clone());
    let e = apply(
        &path,
        TimelineCommand::ClipTrim {
            sequence: id,
            clip: original.id,
            range: range(t(9, 4), t(11, 4)),
        },
        "trim",
    );
    let changed = export(&path).document;
    let DocumentObject::Known(s) = &changed.sequences[0] else {
        panic!()
    };
    let c = &s.tracks[0].clips[0];
    assert_eq!(c.source_in, t(1, 4));
    assert_eq!(c.time_map, original.time_map);
    assert_eq!(
        c.local_time(t(5, 2)).unwrap(),
        original.local_time(t(5, 2)).unwrap()
    );
    undo(&path, e);
    assert_eq!(export(&path).document, p);
    reject(
        &path,
        TimelineCommand::ClipTrim {
            sequence: id,
            clip: original.id,
            range: range(t(1, 1), t(3, 1)),
        },
        "INVALID_CLIP",
    );
    reject(
        &path,
        TimelineCommand::ClipTrim {
            sequence: id,
            clip: original.id,
            range: range(t(2, 1), t(2, 1)),
        },
        "INVALID_CLIP",
    );
}

#[test]
fn stretch_preserves_source_interval_changes_speed_and_undo_restores_document() {
    let mut p = fixture();
    let mut s = sequence(&p);
    s.tracks.truncate(1);
    let id = s.id;
    let original = s.tracks[0].clips[0].clone();
    p.sequences.push(DocumentObject::Known(s));
    let (_dir, path) = setup(p.clone());
    let e = apply(
        &path,
        TimelineCommand::ClipStretch {
            sequence: id,
            clip: original.id,
            range: range(t(4, 1), t(6, 1)),
        },
        "stretch",
    );
    let changed = export(&path).document;
    let DocumentObject::Known(s) = &changed.sequences[0] else {
        panic!()
    };
    let c = &s.tracks[0].clips[0];
    assert_eq!(c.source_in, original.source_in);
    assert_eq!(c.time_map, TimeMap::linear(Time::ZERO, t(1, 2)).unwrap());
    assert_eq!(
        c.local_time(t(6, 1)).unwrap(),
        original.local_time(t(3, 1)).unwrap()
    );
    assert_eq!(c.local_time(t(5, 1)).unwrap(), t(1, 2));
    undo(&path, e);
    assert_eq!(export(&path).document, p);
    reject(
        &path,
        TimelineCommand::ClipStretch {
            sequence: id,
            clip: original.id,
            range: range(t(2, 1), t(2, 1)),
        },
        "INVALID_CLIP",
    );
}

#[test]
fn instance_retime_changes_only_internal_map_with_undo_and_domain_rejections() {
    let mut p = fixture();
    let source = composition(&p).id;
    let node = NodeId::new();
    let root = CompositionId::new();
    p.compositions.push(DocumentObject::Known(Composition {
        id: root,
        duration: Duration::new(t(2, 1)).unwrap(),
        design_extent: composition(&p).design_extent,
        edit_rate: FrameRate::new(24, 1).unwrap(),
        root_nodes: vec![node],
        properties: vec![],
        nodes: vec![SceneNode {
            id: node,
            kind: NodeKind::CompositionInstance(CompositionInstance {
                id: CompositionInstanceId::new(),
                definition_ref: source,
                input_bindings: Default::default(),
                local_time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
                seed: 0,
            }),
            containment_parent: None,
            transform_parent: None,
            child_order: vec![],
            active_range: range(Time::ZERO, t(2, 1)),
            properties: vec![],
            effects: vec![],
        }],
    }));
    let (_dir, path) = setup(p.clone());
    let e = apply(
        &path,
        TimelineCommand::InstanceRetime {
            composition: root,
            node,
            time_map: TimeMap::linear(Time::ZERO, t(1, 2)).unwrap(),
        },
        "retime",
    );
    assert_eq!(
        frame(&path, root.into(), t(1, 1)).linear,
        frame(&path, source.into(), t(1, 2)).linear
    );
    undo(&path, e);
    assert_eq!(export(&path).document, p);
    reject(
        &path,
        TimelineCommand::InstanceRetime {
            composition: root,
            node,
            time_map: TimeMap::linear(Time::ZERO, t(2, 1)).unwrap(),
        },
        "INVALID_INSTANCE",
    );
    reject(
        &path,
        TimelineCommand::InstanceRetime {
            composition: root,
            node,
            time_map: TimeMap::piecewise_linear(vec![
                TimeMapPoint {
                    parent: Time::ZERO,
                    local: Time::ZERO,
                },
                TimeMapPoint {
                    parent: t(1, 1),
                    local: t(1, 1),
                },
            ])
            .unwrap(),
        },
        "TIME_MAP_OUT_OF_DOMAIN",
    );
}

#[test]
fn template_retime_preserves_intro_outro_rejects_short_duration_and_generic_override() {
    let p: Project =
        serde_json::from_str(include_str!("../../../examples/template-001.project.json")).unwrap();
    let root = composition(&p).id;
    let (_dir, path) = setup(p);
    let definition: TemplateDefinition = serde_json::from_str(include_str!(
        "../../../examples/template-001.definition.json"
    ))
    .unwrap();
    engine()
        .dispatch(Request::TemplateDefine(TemplateDefineRequest {
            project: path.clone(),
            base_revision: "1".into(),
            session_id: Uuid::new_v4(),
            idempotency_key: "define".into(),
            definition: definition.clone(),
        }))
        .unwrap();
    let instance = CompositionInstanceId::new();
    let node = NodeId::new();
    engine()
        .dispatch(Request::TemplateInstantiate(TemplateInstantiateRequest {
            project: path.clone(),
            base_revision: "2".into(),
            session_id: Uuid::new_v4(),
            idempotency_key: "instantiate".into(),
            composition: root,
            node,
            index: 0,
            instance: TemplateInstance {
                id: instance,
                definition_ref: definition.id,
                version: definition.version.clone(),
                duration: Duration::new(t(5, 1)).unwrap(),
                variant: None,
                inputs: Default::default(),
            },
        }))
        .unwrap();
    let before = export(&path).document;
    let e = apply(
        &path,
        TimelineCommand::TemplateInstanceRetime {
            instance,
            duration: Duration::new(t(8, 1)).unwrap(),
        },
        "protected-retime",
    );
    let after = export(&path).document;
    let DocumentObject::Known(c) = &after.compositions[0] else {
        panic!()
    };
    let NodeKind::CompositionInstance(i) = &c.nodes.iter().find(|n| n.id == node).unwrap().kind
    else {
        panic!()
    };
    let intro = definition.duration_policy.intro.as_time();
    let outro = definition.duration_policy.outro.as_time();
    assert_eq!(i.local_time_map.map(intro).unwrap(), intro);
    assert_eq!(
        i.local_time_map
            .map(t(8, 1).checked_sub(outro).unwrap())
            .unwrap(),
        t(5, 1).checked_sub(outro).unwrap()
    );
    assert_eq!(i.local_time_map.map(t(8, 1)).unwrap(), t(5, 1));
    undo(&path, e);
    assert_eq!(export(&path).document, before);
    reject(
        &path,
        TimelineCommand::TemplateInstanceRetime {
            instance,
            duration: Duration::new(t(1, 10)).unwrap(),
        },
        "DURATION_TOO_SHORT",
    );
    reject(
        &path,
        TimelineCommand::InstanceRetime {
            composition: root,
            node,
            time_map: TimeMap::linear(Time::ZERO, t(1, 2)).unwrap(),
        },
        "PROTECTED_INTERVAL",
    );
}

#[test]
fn creation_placement_overlap_missing_and_map_domain_are_transactional() {
    let p = fixture();
    let mut s = sequence(&p);
    s.tracks.truncate(1);
    let clip = s.tracks[0].clips.remove(0);
    let id = s.id;
    let track = s.tracks[0].id;
    let (_dir, path) = setup(p.clone());
    let e = apply(
        &path,
        TimelineCommand::SequenceCreate {
            sequence: s.clone(),
        },
        "create",
    );
    undo(&path, e);
    assert_eq!(export(&path).document, p);
    apply(
        &path,
        TimelineCommand::SequenceCreate {
            sequence: s.clone(),
        },
        "create-again",
    );
    let before = export(&path).document;
    let placed = apply(
        &path,
        TimelineCommand::ClipPlace {
            sequence: id,
            track,
            clip: clip.clone(),
        },
        "place",
    );
    let mut overlap = clip.clone();
    overlap.id = ClipId::new();
    reject(
        &path,
        TimelineCommand::ClipPlace {
            sequence: id,
            track,
            clip: overlap,
        },
        "CLIP_OVERLAP",
    );
    let mut missing = clip.clone();
    missing.id = ClipId::new();
    missing.timeline_range = range(t(4, 1), t(5, 1));
    missing.source_ref = SourceRef::Composition {
        composition: CompositionId::new(),
    };
    reject(
        &path,
        TimelineCommand::ClipPlace {
            sequence: id,
            track,
            clip: missing,
        },
        "SOURCE_MISSING",
    );
    let mut domain = clip.clone();
    domain.id = ClipId::new();
    domain.timeline_range = range(t(4, 1), t(6, 1));
    domain.time_map = TimeMap::piecewise_linear(vec![
        TimeMapPoint {
            parent: Time::ZERO,
            local: Time::ZERO,
        },
        TimeMapPoint {
            parent: t(1, 1),
            local: t(1, 1),
        },
    ])
    .unwrap();
    reject(
        &path,
        TimelineCommand::ClipPlace {
            sequence: id,
            track,
            clip: domain,
        },
        "TIME_MAP_OUT_OF_DOMAIN",
    );
    undo(&path, placed);
    assert_eq!(export(&path).document, before);
}

#[test]
fn new_documents_round_trip_validate_schema_and_preserve_opaque_extensions() {
    let mut p = fixture();
    p.sequences.push(DocumentObject::Known(sequence(&p)));
    p.validate_storage().unwrap();
    let value = serde_json::to_value(&p).unwrap();
    assert_eq!(
        serde_json::from_str::<Project>(&value.to_string()).unwrap(),
        p
    );
    let schema = serde_json::to_value(project_json_schema()).unwrap();
    assert!(jsonschema::validator_for(&schema).unwrap().is_valid(&value));
    let checked: Json =
        serde_json::from_str(include_str!("../../../schemas/project-v1.schema.json")).unwrap();
    assert_eq!(schema, checked);
    for field in ["future_sequence", "future_track", "future_clip"] {
        let mut future = value.clone();
        let target = match field {
            "future_track" => &mut future["sequences"][0]["tracks"][0],
            "future_clip" => &mut future["sequences"][0]["tracks"][0]["clips"][0],
            _ => &mut future["sequences"][0],
        };
        target[field] = json!({"unknown":"preserve"});
        let restored: Project = serde_json::from_str(&future.to_string()).unwrap();
        assert!(matches!(restored.sequences[0], DocumentObject::Opaque(_)));
        restored.validate_storage().unwrap();
        assert!(restored.ensure_editable().is_err());
        assert_eq!(serde_json::to_value(restored).unwrap(), future);
    }
    let mut future_source = value.clone();
    future_source["compositions"][0]["future_scene"] = json!(true);
    let restored: Project = serde_json::from_str(&future_source.to_string()).unwrap();
    restored.validate_storage().unwrap();
    assert_eq!(serde_json::to_value(&restored).unwrap(), future_source);
    let DocumentObject::Known(s) = &restored.sequences[0] else {
        panic!()
    };
    assert_eq!(
        RenderSnapshot::for_target(
            &restored,
            RenderTarget::Sequence { sequence: s.id },
            1,
            Default::default()
        )
        .unwrap_err()
        .code(),
        "UNSUPPORTED_FEATURE"
    );
    let mut duplicate = p.clone();
    let DocumentObject::Known(s) = &mut duplicate.sequences[0] else {
        panic!()
    };
    s.tracks[0].clips[0].id = ClipId::from_uuid(p.id);
    assert!(duplicate.validate_storage().is_err());
    let mut shadow = p.clone();
    shadow.unknown_fields.insert("sequences".into(), json!([]));
    assert!(shadow.validate_storage().is_err());
    let mut opaque = p.clone();
    opaque.sequences.push(DocumentObject::Opaque(OpaqueObject {
        id: Uuid::new_v4(),
        fields: std::collections::BTreeMap::from([("id".into(), json!(Uuid::new_v4()))]),
    }));
    assert!(opaque.validate_storage().is_err());
    assert!(
        serde_json::to_value(Project::default())
            .unwrap()
            .get("sequences")
            .is_none()
    );
}

#[test]
fn sequence_snapshot_rejects_opaque_target_and_mismatched_working_space() {
    let mut p = fixture();
    let s = sequence(&p);
    let target = RenderTarget::Sequence { sequence: s.id };
    p.sequences.push(DocumentObject::Known(s));
    let snapshot = RenderSnapshot::for_target(&p, target, 1, Default::default()).unwrap();
    let mut value = serde_json::to_value(snapshot).unwrap();
    value["profile"]["working_space"] = json!("linear_rec2020");
    let restored: RenderSnapshot = serde_json::from_str(&value.to_string()).unwrap();
    assert_eq!(restored.validate().unwrap_err().code(), "RENDER_ERROR");
    let mut value = serde_json::to_value(&p).unwrap();
    value["sequences"][0]["future"] = json!(true);
    let opaque: Project = serde_json::from_str(&value.to_string()).unwrap();
    opaque.validate_storage().unwrap();
    assert_eq!(
        RenderSnapshot::for_target(&opaque, target, 1, Default::default())
            .unwrap_err()
            .code(),
        "UNSUPPORTED_FEATURE"
    );
}

#[test]
fn piecewise_trim_and_stretch_keep_control_values_and_parent_domains_exact() {
    let mut c = clip(CompositionId::new(), t(2, 1), t(5, 1), Rational::ONE);
    c.source_in = t(1, 4);
    c.time_map = TimeMap::piecewise_linear(vec![
        TimeMapPoint {
            parent: Time::ZERO,
            local: t(1, 4),
        },
        TimeMapPoint {
            parent: t(1, 1),
            local: t(3, 4),
        },
        TimeMapPoint {
            parent: t(3, 1),
            local: t(7, 4),
        },
    ])
    .unwrap();
    let trimmed = c.trimmed(range(t(5, 2), t(9, 2))).unwrap();
    for time in [t(5, 2), t(3, 1), t(4, 1), t(9, 2)] {
        assert_eq!(
            trimmed.local_time(time).unwrap(),
            c.local_time(time).unwrap()
        );
    }
    let stretched = c.stretched(range(t(8, 1), t(14, 1))).unwrap();
    for delta in [Time::ZERO, t(1, 2), t(1, 1), t(3, 1)] {
        assert_eq!(
            stretched
                .local_time(
                    t(8, 1)
                        .checked_add(delta.checked_mul(t(2, 1)).unwrap())
                        .unwrap()
                )
                .unwrap(),
            c.local_time(t(2, 1).checked_add(delta).unwrap()).unwrap()
        );
    }
}

#[test]
fn asset_audio_tracks_mix_on_absolute_grid_and_reject_retime() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/data/sine-48k-stereo.wav")
        .canonicalize()
        .unwrap();
    let mut p = fixture_project_for_audio();
    let asset = Asset {
        id: AssetId::new(),
        kind: AssetKind::Audio,
        content_hash: kronello_media::content_hash(&fixture).unwrap(),
        locator: AssetLocator {
            relative: None,
            absolute: Some(fixture.to_string_lossy().into()),
        },
        streams: vec![StreamMetadata {
            index: 0,
            codec: "pcm_s16le".into(),
            time_base: t(1, 48000),
            duration: Some(t(1, 10)),
            width: None,
            height: None,
            pixel_format: None,
            color_primaries: None,
            color_transfer: None,
            color_matrix: None,
            color_range: None,
        }],
    };
    let mut s = sequence(&p);
    s.tracks.clear();
    let c = Clip {
        id: ClipId::new(),
        source_ref: SourceRef::Asset {
            asset: asset.id,
            stream_index: 0,
        },
        timeline_range: range(t(1, 30000), t(1, 20)),
        source_in: t(1, 100),
        time_map: TimeMap::linear(Time::ZERO, Rational::ONE).unwrap(),
        audio_retime: AudioRetimePolicy::Reject,
        links: vec![],
        effects: vec![],
    };
    s.tracks.push(Track {
        id: TrackId::new(),
        kind: TrackKind::Audio,
        clips: vec![c.clone()],
    });
    s.tracks.push(Track {
        id: TrackId::new(),
        kind: TrackKind::Audio,
        clips: vec![Clip {
            id: ClipId::new(),
            ..c.clone()
        }],
    });
    p.assets.push(DocumentObject::Known(asset.clone()));
    p.sequences.push(DocumentObject::Known(s.clone()));
    p.validate_storage().unwrap();
    let output = range(t(0, 1), t(3, 50));
    let bus = mix_sequence_audio(&p, s.id, Path::new("audio.kronello"), output).unwrap();
    let decoded = kronello_media::MediaRuntime::load()
        .unwrap()
        .decode_asset_audio(&asset, Path::new("audio.kronello"), 0)
        .unwrap();
    assert_eq!(bus.start_sample(), 0);
    assert_eq!(bus.buffer().frames().len(), 2880);
    assert_eq!(bus.buffer().frames()[0], [0.0; 2]);
    assert_eq!(bus.buffer().frames()[2400], [0.0; 2]);
    for index in 1..2400 {
        let expected = decoded.buffer.frames()[480 + index - 1].map(|v| v * 2.0);
        assert_eq!(bus.buffer().frames()[index], expected);
    }
    let cut = t(1001, 30000);
    let left = mix_sequence_audio(
        &p,
        s.id,
        Path::new("audio.kronello"),
        range(Time::ZERO, cut),
    )
    .unwrap();
    let right = mix_sequence_audio(
        &p,
        s.id,
        Path::new("audio.kronello"),
        range(cut, output.end()),
    )
    .unwrap();
    assert_eq!(
        left.buffer()
            .frames()
            .iter()
            .chain(right.buffer().frames())
            .copied()
            .collect::<Vec<_>>(),
        bus.buffer().frames()
    );
    let (_dir, path) = setup(p.clone());
    reject(
        &path,
        TimelineCommand::ClipStretch {
            sequence: s.id,
            clip: c.id,
            range: range(c.timeline_range.start(), t(1, 10)),
        },
        "UNSUPPORTED_FEATURE",
    );
    let mut missing = p.clone();
    missing.assets.clear();
    assert_eq!(
        mix_sequence_audio(&missing, s.id, Path::new("audio.kronello"), output)
            .unwrap_err()
            .code,
        "SOURCE_MISSING"
    );
}
fn fixture_project_for_audio() -> Project {
    fixture()
}

#[test]
fn render_target_wire_is_exclusive_strict_and_unsupported_video_has_typed_errors() {
    let mut p = fixture();
    let mut s = sequence(&p);
    s.tracks.truncate(1);
    s.tracks[0].clips[0].source_ref = SourceRef::Generator {
        generator: "future".into(),
    };
    let id = s.id;
    p.sequences.push(DocumentObject::Known(s));
    let (_dir, path) = setup(p.clone());
    assert_eq!(
        RenderSnapshot::for_target(
            &p,
            RenderTarget::Sequence { sequence: id },
            1,
            Default::default()
        )
        .unwrap_err()
        .code(),
        "UNSUPPORTED_FEATURE"
    );
    let schema = api_json_schema();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let valid = json!({"operation":"render.frame","input":{"project":path,"target":{"kind":"sequence","sequence":id},"region":region()},"time":Time::ZERO});
    assert!(validator.is_valid(&valid));
    assert!(serde_json::from_str::<Request>(&valid.to_string()).is_ok());
    let mut missing = valid.clone();
    missing["input"].as_object_mut().unwrap().remove("target");
    let mut duplicate = valid.clone();
    duplicate["input"]["composition"] = json!(composition(&p).id);
    let mut unknown = valid.clone();
    unknown["input"]["target"]["future"] = json!(true);
    let mut null = valid.clone();
    null["input"]["target"] = Json::Null;
    for bad in [missing, duplicate, unknown, null] {
        assert!(!validator.is_valid(&bad));
        assert!(serde_json::from_str::<Request>(&bad.to_string()).is_err());
    }
    let raw = format!(
        "{{\"operation\":\"render.frame\",\"input\":{{\"project\":\"local.kronello\",\"target\":{{\"kind\":\"sequence\",\"sequence\":\"{id}\"}},\"target\":{{\"kind\":\"sequence\",\"sequence\":\"{id}\"}},\"region\":{}}},\"time\":{}}}",
        serde_json::to_string(&region()).unwrap(),
        serde_json::to_string(&Time::ZERO).unwrap()
    );
    assert!(serde_json::from_str::<Request>(&raw).is_err());
}

#[test]
fn authored_track_order_changes_red_blue_source_over() {
    let mut p = fixture();
    let mut raw = serde_json::to_value(&p).unwrap();
    let mut copied = raw["compositions"][0].clone();
    let mut shape = raw["shapes"][0].clone();
    let curve = raw["curves"][0]["id"].as_str().unwrap().to_owned();
    let mut ids = std::collections::BTreeMap::new();
    fn remap(value: &mut Json, keep: &str, ids: &mut std::collections::BTreeMap<String, String>) {
        match value {
            Json::String(s) if s != keep && Uuid::parse_str(s).is_ok() => {
                *s = ids
                    .entry(s.clone())
                    .or_insert_with(|| Uuid::new_v4().to_string())
                    .clone();
            }
            Json::Array(a) => {
                for v in a {
                    remap(v, keep, ids);
                }
            }
            Json::Object(o) => {
                for v in o.values_mut() {
                    remap(v, keep, ids);
                }
            }
            _ => (),
        }
    }
    remap(&mut copied, &curve, &mut ids);
    remap(&mut shape, &curve, &mut ids);
    let color = copied["nodes"][0]["properties"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|v| v["descriptor"]["key"] == "kronello.fill_color")
        .unwrap();
    color["source"] = json!({"kind":"constant","value":{"kind":"color","value":{"space":"linear_rec709","components":{"r":0.0,"g":0.0,"b":1.0,"alpha":1.0}}}});
    raw["compositions"]
        .as_array_mut()
        .unwrap()
        .push(copied.clone());
    raw["shapes"].as_array_mut().unwrap().push(shape);
    p = serde_json::from_str(&raw.to_string()).unwrap();
    let mut s = sequence(&p);
    s.tracks[1].clips[0].source_ref = SourceRef::Composition {
        composition: serde_json::from_value(copied["id"].clone()).unwrap(),
    };
    let id = s.id;
    p.sequences.push(DocumentObject::Known(s));
    let (_dir, path) = setup(p.clone());
    let target = RenderTarget::Sequence { sequence: id };
    let blue_on_top = frame(&path, target, t(11, 4));
    let DocumentObject::Known(s) = &mut p.sequences[0] else {
        panic!()
    };
    s.tracks.reverse();
    let (_dir2, path2) = setup(p);
    let red_on_top = frame(&path2, target, t(11, 4));
    let overlap = 2 * 64 + 8;
    assert_eq!(blue_on_top.linear[overlap][3], 0.9375);
    assert_eq!(red_on_top.linear[overlap][3], 0.9375);
    assert!(blue_on_top.linear[overlap][2] > red_on_top.linear[overlap][2]);
    assert!(blue_on_top.linear[overlap][0] < red_on_top.linear[overlap][0]);
}

#[test]
fn same_track_adjacent_clips_ignore_storage_order_and_use_half_open_boundaries() {
    let mut p = fixture();
    let mut s = sequence(&p);
    s.tracks.truncate(1);
    let source = composition(&p).id;
    let mut next = clip(source, t(3, 1), t(4, 1), Rational::ONE);
    next.source_in = t(1, 1);
    s.tracks[0].clips.push(next);
    let target = RenderTarget::Sequence { sequence: s.id };
    p.sequences.push(DocumentObject::Known(s));
    let (_dir, path) = setup(p.clone());
    let DocumentObject::Known(s) = &mut p.sequences[0] else {
        panic!()
    };
    s.tracks[0].clips.reverse();
    let (_reordered_dir, reordered_path) = setup(p);
    for time in [t(2, 1), t(11, 4), t(3, 1), t(7, 2), t(4, 1)] {
        assert_eq!(
            frame(&path, target, time).linear,
            frame(&reordered_path, target, time).linear
        );
    }
    assert_eq!(
        frame(&path, target, t(3, 1)).linear,
        frame(&path, source.into(), t(1, 1)).linear
    );
    assert!(
        frame(&path, target, t(4, 1))
            .linear
            .iter()
            .all(|p| *p == [0.0; 4])
    );
}

#[test]
fn stretching_a_clip_containing_protected_templates_is_rejected_transactionally() {
    let p: Project =
        serde_json::from_str(include_str!("../../../examples/template-001.project.json")).unwrap();
    let root = composition(&p).id;
    let (_dir, path) = setup(p);
    let definition: TemplateDefinition = serde_json::from_str(include_str!(
        "../../../examples/template-001.definition.json"
    ))
    .unwrap();
    engine()
        .dispatch(Request::TemplateDefine(TemplateDefineRequest {
            project: path.clone(),
            base_revision: "1".into(),
            session_id: Uuid::new_v4(),
            idempotency_key: "define".into(),
            definition: definition.clone(),
        }))
        .unwrap();
    let p = export(&path).document;
    let mut s = sequence(&p);
    s.tracks.truncate(1);
    let c = &mut s.tracks[0].clips[0];
    c.source_ref = SourceRef::Composition {
        composition: definition.composition_ref,
    };
    c.timeline_range = range(Time::ZERO, t(1, 1));
    let clip = c.id;
    let id = s.id;
    apply(
        &path,
        TimelineCommand::SequenceCreate { sequence: s },
        "create",
    );
    reject(
        &path,
        TimelineCommand::ClipStretch {
            sequence: id,
            clip,
            range: range(Time::ZERO, t(2, 1)),
        },
        "PROTECTED_INTERVAL",
    );
    assert_ne!(root, definition.composition_ref);
}

#[test]
fn variant_clip_stretch_cannot_bypass_protected_duration_policy() {
    let p: Project =
        serde_json::from_str(include_str!("../../../examples/template-002.project.json")).unwrap();
    let (_dir, path) = setup(p);
    let definition: TemplateDefinition = serde_json::from_str(include_str!(
        "../../../examples/template-002.definition.json"
    ))
    .unwrap();
    engine()
        .dispatch(Request::TemplateDefine(TemplateDefineRequest {
            project: path.clone(),
            base_revision: "1".into(),
            session_id: Uuid::new_v4(),
            idempotency_key: "define".into(),
            definition: definition.clone(),
        }))
        .unwrap();
    let p = export(&path).document;
    let mut s = sequence(&p);
    s.tracks.truncate(1);
    let c = &mut s.tracks[0].clips[0];
    c.source_ref = SourceRef::Composition {
        composition: definition.variants["portrait"].composition_ref,
    };
    c.timeline_range = range(Time::ZERO, t(1, 1));
    let clip = c.id;
    let id = s.id;
    apply(
        &path,
        TimelineCommand::SequenceCreate { sequence: s },
        "create",
    );
    reject(
        &path,
        TimelineCommand::ClipStretch {
            sequence: id,
            clip,
            range: range(Time::ZERO, t(2, 1)),
        },
        "PROTECTED_INTERVAL",
    );
}
