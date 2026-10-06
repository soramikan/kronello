use kronello_model::*;
use kronello_render::RenderSnapshot;
use kronello_time::{Duration, Time};
fn fixture() -> (Project, CompositionId) {
    let mut p: Project =
        serde_json::from_str(include_str!("../../../examples/ffi-preview.project.json")).unwrap();
    let DocumentObject::Known(source) = &p.compositions[0] else {
        panic!()
    };
    let source = source.clone();
    let mut main = source.clone();
    main.id = CompositionId::new();
    let mut node = source.nodes[0].clone();
    node.id = NodeId::new();
    node.properties = Vec::new();
    let registry = kronello_render::render_registry();
    for descriptor in simulation_descriptors() {
        let value = match descriptor.key().as_str() {
            "kronello.simulation.velocity" => {
                Value::Vec2([FiniteF64::new(20.).unwrap(), FiniteF64::new(0.).unwrap()])
            }
            _ => descriptor.definition().default.clone(),
        };
        node.properties.push(
            Property::new(
                PropertyId::new(),
                DescriptorRef::new(&descriptor),
                PropertySource::Constant(value),
                vec![],
                &registry,
            )
            .unwrap(),
        );
    }
    let ids: Vec<_> = node.properties.iter().map(Property::id).collect();
    let id = ContentId::new();
    node.kind = NodeKind::Simulation { content_ref: id };
    node.child_order = Vec::new();
    node.containment_parent = None;
    node.transform_parent = None;
    main.root_nodes = vec![node.id];
    main.nodes = vec![node];
    p.simulations
        .push(DocumentObject::Known(ParticleSimulation {
            id,
            version: 1,
            source: RepeatSource {
                composition: source.id,
                root: source.root_nodes[0],
            },
            start: Time::ZERO,
            step: Duration::new(Time::new(1, 10).unwrap()).unwrap(),
            lifetime: Duration::new(Time::ONE).unwrap(),
            emission_interval: Duration::new(Time::new(1, 5).unwrap()).unwrap(),
            seed: 7,
            inputs: ParticlePropertyInputs {
                origin: ids[0],
                velocity: ids[1],
                jitter: ids[2],
                acceleration: ids[3],
                enabled: ids[4],
                birth_count: ids[5],
            },
            source_bindings: Default::default(),
        }));
    let root = main.id;
    p.compositions.push(DocumentObject::Known(main));
    (p, root)
}

use kronello_service::*;
use serde_json::json;
use uuid::Uuid;
fn run(request: serde_json::Value) -> Result<ResultData, ServiceError> {
    Service::new(BackendSelection::CpuReference).dispatch(serde_json::from_value(request).unwrap())
}
fn export(path: &std::path::Path) -> ExportResult {
    let ResultData::Export(r) = run(json!({"operation":"project.export","project":path})).unwrap()
    else {
        panic!()
    };
    *r
}
fn apply(path: &std::path::Path, commands: Vec<EditCommand>, key: &str) -> kronello_store::Event {
    let base = export(path).revision;
    let ResultData::Plan(plan) = run(
        json!({"operation":"edit.plan","project":path,"base_revision":base,"commands":commands}),
    )
    .unwrap() else {
        panic!()
    };
    let request = json!({"operation":"edit.apply","project":path,"base_revision":base,"plan_hash":plan.plan_hash,"commands":plan.commands,"idempotency_key":key,"session_id":Uuid::from_u128(1)});
    let ResultData::Edit(event) = run(request.clone()).unwrap() else {
        panic!()
    };
    let ResultData::Edit(retry) = run(request).unwrap() else {
        panic!()
    };
    assert_eq!(event, retry);
    event
}
#[test]
fn simulation_shared_save_retry_conflict_remove_and_undo() {
    let (project, root) = fixture();
    if let Some(directory) = std::env::var_os("KRONELLO_WRITE_SIM_FIXTURE") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("simulation.project.json"),
            serde_json::to_vec_pretty(&project).unwrap(),
        )
        .unwrap();
        std::fs::write(
            directory.join("composition.json"),
            serde_json::to_vec(&root).unwrap(),
        )
        .unwrap();
    }
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("simulation.kronello");
    run(json!({"operation":"project.create","project":path,"document":project})).unwrap();
    let initial = export(&path);
    let DocumentObject::Known(original) = &initial.document.simulations[0] else {
        panic!()
    };
    let mut changed = original.clone();
    changed.seed = u64::MAX;
    let event = apply(
        &path,
        vec![EditCommand::SimulationSet {
            simulation: changed.clone(),
        }],
        "seed",
    );
    let reopened = export(&path);
    assert_eq!(reopened.revision, event.revision.to_string());
    assert_eq!(
        reopened.document.simulations,
        vec![DocumentObject::Known(changed.clone())]
    );
    let snapshot =
        RenderSnapshot::new(&reopened.document, root, event.revision, Default::default()).unwrap();
    let roundtrip: RenderSnapshot =
        serde_json::from_slice(&serde_json::to_vec(&snapshot).unwrap()).unwrap();
    roundtrip.validate().unwrap();
    let stale = run(json!({"operation":"edit.plan","project":path,"base_revision":initial.revision,"commands":[EditCommand::SimulationSet {simulation:original.clone()}]})).unwrap_err();
    assert_eq!(stale.code, "REVISION_CONFLICT");
    assert_eq!(export(&path).document, reopened.document);
    run(json!({"operation":"edit.undo","project":path,"base_revision":event.revision.to_string(),"event_id":event.id,"session_id":Uuid::from_u128(2),"idempotency_key":"undo-seed"})).unwrap();
    assert_eq!(export(&path).document, initial.document);
    let missing = run(json!({"operation":"edit.plan","project":path,"base_revision":export(&path).revision,"commands":[EditCommand::SimulationRemove{id:original.id}]})).unwrap_err();
    assert_eq!(missing.code, "SIMULATION_MISSING");
    let composition = export(&path)
        .document
        .compositions
        .into_iter()
        .find_map(|c| match c {
            DocumentObject::Known(c) if c.id == root => Some(c),
            _ => None,
        })
        .unwrap();
    let removed = apply(
        &path,
        vec![
            EditCommand::NodeRemove {
                composition: root,
                node: composition.nodes[0].id,
            },
            EditCommand::SimulationRemove { id: original.id },
        ],
        "remove",
    );
    assert!(export(&path).document.simulations.is_empty());
    run(json!({"operation":"edit.undo","project":path,"base_revision":removed.revision.to_string(),"event_id":removed.id,"session_id":Uuid::from_u128(3),"idempotency_key":"undo-remove"})).unwrap();
    assert_eq!(export(&path).document, initial.document);
}
#[test]
fn simulation_pin_and_budget_fail_explicitly_without_persisting() {
    let (mut project, root) = fixture();
    let snapshot = RenderSnapshot::new(&project, root, 0, Default::default()).unwrap();
    for pin in [serde_json::Value::Null, json!(999)] {
        let mut wire = serde_json::to_value(&snapshot).unwrap();
        wire["semantic_versions"]["simulation"] = pin;
        let unsupported: RenderSnapshot = serde_json::from_value(wire).unwrap();
        assert_eq!(
            unsupported.validate().unwrap_err().code(),
            "UNSUPPORTED_FEATURE"
        );
    }
    let DocumentObject::Known(sim) = &mut project.simulations[0] else {
        panic!()
    };
    sim.step = Duration::new(Time::new(1, 1_000_000).unwrap()).unwrap();
    sim.emission_interval = sim.step;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("budget.kronello");
    run(json!({"operation":"project.create","project":path,"document":project})).unwrap();
    let before = export(&path);
    let error = run(json!({"operation":"render.frame","input":{"project":path,"composition":root,"region":{"origin":[0.,0.],"extent":[32.,32.],"pixels":[32,32]}},"time":Time::ONE})).unwrap_err();
    assert_eq!(error.code, "SIMULATION_BUDGET");
    assert_eq!(export(&path).revision, before.revision);
    assert_eq!(export(&path).document, before.document);
}
