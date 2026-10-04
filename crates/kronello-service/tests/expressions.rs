use kronello_model::*;
use kronello_render::{RenderProfile, RenderSnapshot, build_scene_ir};
use kronello_service::*;
use kronello_time::Time;
use uuid::Uuid;

fn scalar(v: f64) -> Value {
    Value::Scalar(FiniteF64::new(v).unwrap())
}
fn expression(value: f64) -> Expression {
    Expression {
        id: ExpressionId::new(),
        version: EXPRESSION_VERSION,
        value_type: ValueType::Scalar,
        budget: Default::default(),
        nodes: vec![ExpressionNode::Literal(scalar(value))],
    }
}
fn document() -> Project {
    let mut p: Project =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    let DocumentObject::Known(c) = &mut p.compositions[0] else {
        panic!()
    };
    c.nodes.truncate(1);
    c.root_nodes.truncate(1);
    c.nodes[0].kind = NodeKind::Null;
    c.nodes[0]
        .properties
        .retain(|p| p.descriptor().key.as_str() == "kronello.opacity");
    p.shapes.clear();
    p.texts.clear();
    p.curves.clear();
    p
}
fn comp(p: &Project) -> &Composition {
    let DocumentObject::Known(c) = &p.compositions[0] else {
        panic!()
    };
    c
}
fn commands(p: &Project, e: &Expression) -> Vec<EditCommand> {
    vec![
        EditCommand::ExpressionSet {
            expression: e.clone(),
        },
        EditCommand::PropertySourceSet {
            object: comp(p).nodes[0].id.as_uuid(),
            property: comp(p).nodes[0].properties[0].id(),
            source: PropertySource::Expression(e.id),
            curve: None,
        },
    ]
}
fn sample(
    service: &Service<'_>,
    path: &std::path::Path,
    p: &Project,
) -> Result<PropertySampleResult, ServiceError> {
    let c = comp(p);
    let ResultData::Samples(r) =
        service.dispatch(Request::PropertySample(PropertySampleRequest {
            project: path.into(),
            composition: c.id,
            keys: vec![SampleKey::Node {
                instance_path: InstancePath::root(),
                node: c.nodes[0].id,
                property: c.nodes[0].properties[0].id(),
            }],
            times: vec![Time::ZERO, Time::new(1, 2).unwrap()],
            fonts: None,
        }))?
    else {
        panic!()
    };
    Ok(r)
}
fn plan(
    service: &Service<'_>,
    path: &std::path::Path,
    revision: &str,
    commands: Vec<EditCommand>,
) -> EditPlan {
    let ResultData::Plan(p) = service
        .dispatch(Request::EditPlan(PlanRequest {
            project: path.into(),
            base_revision: revision.into(),
            commands,
        }))
        .unwrap()
    else {
        panic!()
    };
    *p
}
fn apply(
    service: &Service<'_>,
    path: &std::path::Path,
    plan: &EditPlan,
    key: &str,
) -> Result<kronello_store::Event, ServiceError> {
    let ResultData::Edit(e) = service.dispatch(Request::EditApply(EditApplyRequest {
        project: path.into(),
        base_revision: plan.base_revision.clone(),
        plan_hash: plan.plan_hash.clone(),
        idempotency_key: key.into(),
        session_id: Uuid::from_u128(123),
        commands: plan.commands.clone(),
    }))?
    else {
        panic!()
    };
    Ok(e)
}

#[test]
fn shared_edit_schema_revision_idempotency_undo_and_fixed_render_snapshot() {
    let s = Service::new(BackendSelection::CpuReference);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("expr.kronello");
    let p = document();
    let e = expression(0.4);
    s.dispatch(Request::ProjectCreate(CreateRequest {
        project: path.clone(),
        document: p.clone(),
    }))
    .unwrap();
    let cmd = commands(&p, &e);
    // The exact same command payload goes through the public wire decoder.
    let wire = serde_json::json!({"operation":"edit.plan", "project":path, "base_revision":"1", "commands":cmd});
    assert!(serde_json::from_str::<Request>(&wire.to_string()).is_ok());
    let plan = plan(&s, &path, "1", cmd);
    let event = apply(&s, &path, &plan, "set-expression").unwrap();
    assert_eq!(event.revision, 2);
    assert_eq!(
        apply(&s, &path, &plan, "set-expression").unwrap().id,
        event.id
    );
    assert_eq!(
        apply(&s, &path, &plan, "stale-key").unwrap_err().code,
        "REVISION_CONFLICT"
    );
    assert_eq!(
        sample(&s, &path, &p).unwrap().samples[0].values,
        vec![scalar(0.4); 2]
    );
    let frozen =
        RenderSnapshot::new(&plan.candidate, comp(&p).id, 2, RenderProfile::default()).unwrap();
    let ir = build_scene_ir(&frozen, Time::ZERO, &[]).unwrap();
    assert_eq!(ir.nodes[0].opacity, 0.4);
    let hash = frozen.content_hash().unwrap();
    let ResultData::Edit(undo) = s
        .dispatch(Request::EditUndo(UndoRequest {
            project: path.clone(),
            base_revision: "2".into(),
            session_id: Uuid::from_u128(123),
            idempotency_key: "undo-expression".into(),
            event_id: event.id,
        }))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(undo.revision, 3);
    assert_eq!(
        sample(&s, &path, &p).unwrap().samples[0].values,
        vec![scalar(0.75); 2]
    );
    assert_eq!(
        build_scene_ir(&frozen, Time::ZERO, &[]).unwrap().nodes[0].opacity,
        0.4
    );
    assert_eq!(frozen.content_hash().unwrap(), hash);
    let store =
        kronello_store::ProjectStore::open(&path, kronello_store::OpenOptions::default()).unwrap();
    let historical = store.snapshot_at(2).unwrap();
    assert_eq!(historical.document.expressions.len(), 1);
    assert!(store.snapshot().unwrap().document.expressions.is_empty());
    store.close().unwrap();
    let historical_render = RenderSnapshot::new(
        &historical.document,
        comp(&p).id,
        2,
        RenderProfile::default(),
    )
    .unwrap();
    assert_eq!(
        build_scene_ir(&historical_render, Time::ZERO, &[])
            .unwrap()
            .nodes[0]
            .opacity,
        0.4
    );
    let restored: RenderSnapshot =
        serde_json::from_slice(&serde_json::to_vec(&frozen).unwrap()).unwrap();
    assert_eq!(
        build_scene_ir(&restored, Time::ZERO, &[]).unwrap().nodes[0].opacity,
        0.4
    );
    assert_eq!(restored.semantic_versions().expression, EXPRESSION_VERSION);
    let schema = serde_json::to_value(api_json_schema()).unwrap();
    jsonschema::validator_for(&schema)
        .unwrap()
        .validate(&wire)
        .unwrap();
    assert!(schema["$defs"]["Expression"]["properties"]["nodes"].is_object());
    assert!(schema.to_string().contains("expression_set"));
    assert!(
        CapabilitiesResult::current(None)
            .features
            .contains(&"expression".into())
    );
    let mut forbidden = wire;
    forbidden["commands"][0]["expression_set"]["expression"]["nodes"] =
        serde_json::json!([{"clock":{}}]);
    assert!(
        jsonschema::validator_for(&schema)
            .unwrap()
            .validate(&forbidden)
            .is_err()
    );
    let Response::Error { error } = s.execute_json(&forbidden.to_string()) else {
        panic!()
    };
    assert_eq!(error.code, "INVALID_REQUEST");
}

#[test]
fn sample_and_final_render_share_budget_arithmetic_and_cycle_diagnostics() {
    for failure in [
        "budget",
        "arithmetic",
        "cycle",
        "missing_curve",
        "missing_expression",
        "missing_property",
    ] {
        let s = Service::new(BackendSelection::CpuReference);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("error.kronello");
        let mut p = document();
        let mut e = expression(0.4);
        match failure {
            "budget" => e.budget.instructions = 0,
            "arithmetic" => {
                e.nodes = vec![
                    ExpressionNode::Literal(scalar(1.0)),
                    ExpressionNode::Literal(scalar(0.0)),
                    ExpressionNode::Divide { left: 0, right: 1 },
                ]
            }
            "cycle" => {
                e.nodes = vec![ExpressionNode::Property {
                    node: Some(comp(&p).nodes[0].id),
                    property: comp(&p).nodes[0].properties[0].id(),
                    value_type: ValueType::Scalar,
                }]
            }
            "missing_curve" => {
                e.nodes = vec![ExpressionNode::CurveSample {
                    curve: CurveId::new(),
                    offset: Time::ZERO,
                    value_type: ValueType::Scalar,
                }]
            }
            "missing_property" => {
                e.nodes = vec![ExpressionNode::Property {
                    node: None,
                    property: PropertyId::new(),
                    value_type: ValueType::Scalar,
                }]
            }
            "missing_expression" => (),
            _ => unreachable!(),
        }
        let DocumentObject::Known(c) = &mut p.compositions[0] else {
            panic!()
        };
        c.nodes[0].properties[0]
            .set_source(
                PropertySource::Expression(e.id),
                &SchemaRegistry::with_builtin(),
            )
            .unwrap();
        if failure != "missing_expression" {
            p.expressions.push(DocumentObject::Known(e));
        }
        s.dispatch(Request::ProjectCreate(CreateRequest {
            project: path.clone(),
            document: p.clone(),
        }))
        .unwrap();
        let query = sample(&s, &path, &p).unwrap_err();
        let snapshot = RenderSnapshot::new(&p, comp(&p).id, 1, RenderProfile::default()).unwrap();
        let render = build_scene_ir(&snapshot, Time::ZERO, &[]).unwrap_err();
        assert_eq!(query.code, render.code(), "{failure}");
        assert_eq!(
            query.code,
            match failure {
                "budget" => "EXPRESSION_BUDGET_EXCEEDED",
                "cycle" => "PROPERTY_DEPENDENCY_CYCLE",
                _ => "EVALUATION_ERROR",
            }
        );
        assert_eq!(query.message, render.to_string());
        let frame = kronello_render::render_frame(
            &snapshot,
            &[],
            &kronello_gpu::render_adapter::CpuReferenceBackend,
            kronello_render::FrameRequest {
                time: Time::ZERO,
                region: kronello_render::OutputRegion {
                    origin: [0.0, 0.0],
                    extent: [16.0, 16.0],
                    pixels: [16, 16],
                },
            },
        )
        .unwrap_err();
        assert_eq!(frame.code(), query.code);
    }
}

#[test]
fn expression_edits_validate_dependencies_atomically_and_conflict_on_undo() {
    let s = Service::new(BackendSelection::CpuReference);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("edit.kronello");
    let p = document();
    let e = expression(0.4);
    s.dispatch(Request::ProjectCreate(CreateRequest {
        project: path.clone(),
        document: p.clone(),
    }))
    .unwrap();
    let first = plan(&s, &path, "1", commands(&p, &e));
    let event = apply(&s, &path, &first, "first").unwrap();
    let mut edited = e.clone();
    edited.nodes = vec![ExpressionNode::Literal(scalar(0.7))];
    let second = plan(
        &s,
        &path,
        "2",
        vec![EditCommand::ExpressionSet { expression: edited }],
    );
    apply(&s, &path, &second, "second").unwrap();
    let error = s
        .dispatch(Request::EditUndo(UndoRequest {
            project: path.clone(),
            base_revision: "3".into(),
            session_id: Uuid::new_v4(),
            idempotency_key: "conflict".into(),
            event_id: event.id,
        }))
        .unwrap_err();
    assert_eq!(error.code, "UNDO_CONFLICT");
    let mut cycle = e.clone();
    cycle.nodes = vec![ExpressionNode::Property {
        node: Some(comp(&p).nodes[0].id),
        property: comp(&p).nodes[0].properties[0].id(),
        value_type: ValueType::Scalar,
    }];
    let error = s
        .dispatch(Request::EditPlan(PlanRequest {
            project: path.clone(),
            base_revision: "3".into(),
            commands: vec![EditCommand::ExpressionSet { expression: cycle }],
        }))
        .unwrap_err();
    assert_eq!(error.code, "PROPERTY_DEPENDENCY_CYCLE");
    assert_eq!(sample(&s, &path, &p).unwrap().revision, "3");
    assert_eq!(
        sample(&s, &path, &p).unwrap().samples[0].values,
        vec![scalar(0.7); 2]
    );
}
