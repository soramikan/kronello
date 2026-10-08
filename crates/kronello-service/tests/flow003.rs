//! FLOW-003 (ADR-0130): versioned export presets in the document and ordered,
//! keyed `export.batch` submission shared by every transport.
use kronello_jobs::{JobConfig, JobStore};
use kronello_model::*;
use kronello_service::*;
use kronello_time::{FrameRate, Rational, TimeRange};
use serde_json::{Value as Json, json};
use std::path::PathBuf;
use uuid::Uuid;

fn fixture() -> Project {
    serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap()
}
fn composition(document: &Project) -> CompositionId {
    let DocumentObject::Known(c) = &document.compositions[0] else {
        panic!()
    };
    c.id
}
fn engine(state: &std::path::Path) -> Service<'static> {
    Service::new(BackendSelection::CpuReference)
        .with_job_config(JobConfig::at(state))
        // The stub worker exits instantly; submission records still commit.
        .with_worker_executable(PathBuf::from("/usr/bin/false"))
}
fn setup(document: Project, state: &std::path::Path) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("batch.kronello");
    let ResultData::Project(_) = engine(state)
        .dispatch(Request::ProjectCreate(CreateRequest {
            plan_hash: None,
            idempotency_key: None,
            project: project.clone(),
            document,
        }))
        .unwrap()
    else {
        panic!()
    };
    (dir, project)
}
fn apply(
    engine: &Service,
    project: &std::path::Path,
    base_revision: &str,
    commands: Vec<EditCommand>,
) -> Result<Json, ServiceError> {
    let ResultData::Plan(plan) = engine.dispatch(Request::EditPlan(PlanRequest {
        project: project.to_path_buf(),
        base_revision: base_revision.into(),
        commands: commands.clone(),
    }))?
    else {
        panic!()
    };
    match engine.dispatch(Request::EditApply(EditApplyRequest {
        project: project.to_path_buf(),
        base_revision: base_revision.into(),
        plan_hash: plan.plan_hash,
        session_id: Uuid::new_v4(),
        idempotency_key: Uuid::new_v4().to_string(),
        commands,
    }))? {
        ResultData::Edit(event) => Ok(serde_json::to_value(event).unwrap()),
        other => panic!("{other:?}"),
    }
}
fn preset(id: ExportPresetId, composition: CompositionId) -> ExportPreset {
    ExportPreset {
        version: EXPORT_PRESET_VERSION,
        id,
        name: "配信".into(),
        composition: Some(composition),
        target: None,
        range: TimeRange::new(Rational::ZERO, Rational::new(1, 24).unwrap()).unwrap(),
        frame_rate: FrameRate::new(24, 1).unwrap(),
        region: ExportRegion {
            origin: [0.0, 0.0],
            extent: [64.0, 32.0],
            pixels: [8, 4],
        },
        profile: ExportProfile::default(),
        output: ExportOutput::ImageSequence,
        required_features: vec![],
    }
}
fn submission(
    project: &PathBuf,
    composition: CompositionId,
    destination: PathBuf,
) -> RenderSubmitRequest {
    serde_json::from_value(json!({"render":{"input":{"project":project,
        "composition":composition,
        "region":{"origin":[0.0,0.0],"extent":[64.0,32.0],"pixels":[8,4]}},
        "range":{"start":{"num":"0","den":"1"},"end":{"num":"1","den":"24"}},
        "frame_rate":{"num":"24","den":"1"},"output_directory":destination},
        "output":{"format":"image_sequence"}}))
    .unwrap()
}
fn batch(engine: &Service, items: Vec<ExportBatchItem>) -> Result<ExportBatchResult, ServiceError> {
    batch_with(engine, items, BatchFailurePolicy::Stop)
}
fn batch_with(
    engine: &Service,
    items: Vec<ExportBatchItem>,
    failure_policy: BatchFailurePolicy,
) -> Result<ExportBatchResult, ServiceError> {
    match engine.dispatch(Request::ExportBatch(ExportBatchRequest {
        items,
        failure_policy,
    }))? {
        ResultData::Batch(result) => Ok(*result),
        other => panic!("{other:?}"),
    }
}
fn item(submission: RenderSubmitRequest) -> ExportBatchItem {
    ExportBatchItem {
        idempotency_key: None,
        submission: Some(Box::new(submission)),
        preset: None,
        project: None,
        destination: None,
    }
}

#[test]
fn export_presets_roundtrip_through_shared_edits() {
    let state = tempfile::tempdir().unwrap();
    let engine = engine(state.path());
    let document = fixture();
    let composition = composition(&document);
    let (_dir, project) = setup(document, state.path());
    let preset_id = ExportPresetId::new();
    apply(
        &engine,
        &project,
        "1",
        vec![EditCommand::ExportPresetSave {
            preset: preset(preset_id, composition),
        }],
    )
    .unwrap();
    // Upsert by id: same id, changed name stays a single preset.
    let mut renamed = preset(preset_id, composition);
    renamed.name = "ProRes 配信".into();
    apply(
        &engine,
        &project,
        "2",
        vec![EditCommand::ExportPresetSave { preset: renamed }],
    )
    .unwrap();
    let ResultData::Export(export) = engine
        .dispatch(Request::ProjectExport(ProjectRequest {
            project: project.clone(),
        }))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(export.document.export_presets.len(), 1);
    assert_eq!(export.document.export_presets[0].name, "ProRes 配信");
    assert_eq!(export.document.export_presets[0].id, preset_id);
    // Unknown targets never reach the document.
    let mut bad = preset(ExportPresetId::new(), CompositionId::new());
    bad.name = "missing".into();
    let error = apply(
        &engine,
        &project,
        "3",
        vec![EditCommand::ExportPresetSave { preset: bad }],
    )
    .unwrap_err();
    assert_eq!(error.code, "INVALID_MUTATION");
    let mut bad_version = preset(ExportPresetId::new(), composition);
    bad_version.version = 2;
    assert!(
        apply(
            &engine,
            &project,
            "3",
            vec![EditCommand::ExportPresetSave {
                preset: bad_version
            }],
        )
        .is_err()
    );
    apply(
        &engine,
        &project,
        "3",
        vec![EditCommand::ExportPresetDelete { preset: preset_id }],
    )
    .unwrap();
    let ResultData::Export(export) = engine
        .dispatch(Request::ProjectExport(ProjectRequest {
            project: project.clone(),
        }))
        .unwrap()
    else {
        panic!()
    };
    assert!(export.document.export_presets.is_empty());
}

#[test]
fn export_batch_orders_outcomes_replays_and_reports_item_failures() {
    let state = tempfile::tempdir().unwrap();
    let engine = engine(state.path());
    let document = fixture();
    let composition = composition(&document);
    let (dir, project) = setup(document, state.path());
    let submit = |name: &str| item(submission(&project, composition, dir.path().join(name)));
    let first = batch(&engine, vec![submit("a"), submit("b")]).unwrap();
    assert_eq!(
        first.items.iter().map(|i| i.outcome).collect::<Vec<_>>(),
        vec![ExportBatchOutcome::Submitted, ExportBatchOutcome::Submitted]
    );
    assert!(first.items.iter().all(|i| i.job.is_some()));
    assert!(
        first
            .items
            .iter()
            .all(|i| i.idempotency_key.starts_with("auto:"))
    );
    // An identical batch replays the recorded jobs without duplicating them.
    let second = batch(&engine, vec![submit("a"), submit("b")]).unwrap();
    assert!(
        second
            .items
            .iter()
            .all(|i| i.outcome == ExportBatchOutcome::Replayed)
    );
    assert_eq!(
        second.items[0].job.as_ref().unwrap().id,
        first.items[0].job.as_ref().unwrap().id
    );
    let jobs = JobStore::open(JobConfig::at(state.path()))
        .unwrap()
        .list()
        .unwrap();
    assert_eq!(jobs.len(), 2);
    // Duplicate keys and destinations fail the whole request deterministically.
    let mut dup_key = submit("c");
    dup_key.idempotency_key = Some("k".into());
    let mut dup_key2 = submit("d");
    dup_key2.idempotency_key = Some("k".into());
    assert_eq!(
        batch(&engine, vec![dup_key, dup_key2]).unwrap_err().code,
        "INVALID_REQUEST"
    );
    let same_destination = vec![
        submit("same"),
        item(submission(&project, composition, dir.path().join("same"))),
    ];
    assert_eq!(
        batch(&engine, same_destination).unwrap_err().code,
        "INVALID_REQUEST"
    );
    for shape in [
        ExportBatchItem {
            idempotency_key: None,
            submission: None,
            preset: None,
            project: None,
            destination: None,
        },
        ExportBatchItem {
            idempotency_key: None,
            submission: Some(Box::new(submission(
                &project,
                composition,
                dir.path().join("both"),
            ))),
            preset: Some(ExportPresetId::new()),
            project: Some(project.clone()),
            destination: Some(dir.path().join("both")),
        },
    ] {
        assert_eq!(
            batch(&engine, vec![shape]).unwrap_err().code,
            "INVALID_REQUEST"
        );
    }
    assert_eq!(batch(&engine, vec![]).unwrap_err().code, "INVALID_REQUEST");
    // An existing destination fails that item; stop skips the remainder.
    let occupied = dir.path().join("occupied");
    std::fs::create_dir(&occupied).unwrap();
    let stopped = batch(
        &engine,
        vec![
            item(submission(&project, composition, occupied.clone())),
            submit("after-stop"),
        ],
    )
    .unwrap();
    assert_eq!(stopped.items[0].outcome, ExportBatchOutcome::Failed);
    assert_eq!(
        stopped.items[0].error.as_ref().unwrap().code,
        "OUTPUT_EXISTS"
    );
    assert_eq!(stopped.items[1].outcome, ExportBatchOutcome::Skipped);
    assert_eq!(
        stopped.items[1].error.as_ref().unwrap().code,
        "BATCH_STOPPED"
    );
    // Continue still attempts every remaining item.
    let continued = batch_with(
        &engine,
        vec![
            item(submission(&project, composition, occupied)),
            submit("after-continue"),
        ],
        BatchFailurePolicy::Continue,
    )
    .unwrap();
    assert_eq!(continued.items[0].outcome, ExportBatchOutcome::Failed);
    assert_eq!(continued.items[1].outcome, ExportBatchOutcome::Submitted);
    // A key recorded for one payload rejects a different payload.
    let mut keyed = submit("keyed-a");
    keyed.idempotency_key = Some("shared".into());
    batch(&engine, vec![keyed]).unwrap();
    let mut reused = submit("keyed-b");
    reused.idempotency_key = Some("shared".into());
    let reused = batch(&engine, vec![reused]).unwrap();
    assert_eq!(reused.items[0].outcome, ExportBatchOutcome::Failed);
    assert_eq!(
        reused.items[0].error.as_ref().unwrap().code,
        "IDEMPOTENCY_KEY_REUSED"
    );
    // The same key with the identical payload replays instead.
    let mut same = submit("keyed-a");
    same.idempotency_key = Some("shared".into());
    let same = batch(&engine, vec![same]).unwrap();
    assert_eq!(same.items[0].outcome, ExportBatchOutcome::Replayed);
}

#[test]
fn export_batch_resolves_stored_presets_server_side() {
    let state = tempfile::tempdir().unwrap();
    let engine = engine(state.path());
    let document = fixture();
    let composition = composition(&document);
    let (dir, project) = setup(document, state.path());
    let preset_id = ExportPresetId::new();
    apply(
        &engine,
        &project,
        "1",
        vec![EditCommand::ExportPresetSave {
            preset: preset(preset_id, composition),
        }],
    )
    .unwrap();
    let result = batch(
        &engine,
        vec![ExportBatchItem {
            idempotency_key: Some("preset-run".into()),
            submission: None,
            preset: Some(preset_id),
            project: Some(project.clone()),
            destination: Some(dir.path().join("preset-frames")),
        }],
    )
    .unwrap();
    assert_eq!(result.items[0].outcome, ExportBatchOutcome::Submitted);
    let job = result.items[0].job.as_ref().unwrap();
    assert!(job.destination.ends_with("preset-frames"));
    assert_eq!(job.total_frames, 1);
    // Watch/CLI naming relies on the shared extension helper.
    let ResultData::Export(export) = engine
        .dispatch(Request::ProjectExport(ProjectRequest {
            project: project.clone(),
        }))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(
        preset_output_extension(&export.document.export_presets[0]).unwrap(),
        None
    );
    // Unknown presets fail the whole request during normalization, before any
    // item is queued (deterministic, no half-submitted batch).
    let error = batch(
        &engine,
        vec![ExportBatchItem {
            idempotency_key: None,
            submission: None,
            preset: Some(ExportPresetId::new()),
            project: Some(project),
            destination: Some(dir.path().join("x")),
        }],
    )
    .unwrap_err();
    assert_eq!(error.code, "PRESET_MISSING");
}
