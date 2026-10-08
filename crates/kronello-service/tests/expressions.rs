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
            luts: None,
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
        plan_hash: None,
        idempotency_key: None,
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
    assert_eq!(
        restored.semantic_versions().expression,
        EXPRESSION_SUPPORTED_VERSION
    );
    // Older persisted snapshots omit the optional expression pin: retain v1 meaning.
    let mut legacy_wire = serde_json::to_value(&frozen).unwrap();
    legacy_wire["semantic_versions"]
        .as_object_mut()
        .unwrap()
        .remove("expression");
    let legacy: RenderSnapshot = serde_json::from_value(legacy_wire).unwrap();
    assert_eq!(legacy.semantic_versions().expression, EXPRESSION_VERSION);
    assert_eq!(
        build_scene_ir(&legacy, Time::ZERO, &[]).unwrap().nodes[0].opacity,
        0.4
    );
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
        "noise_range",
        "data_missing",
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
            "noise_range" => {
                e.version = 3;
                e.nodes = vec![
                    ExpressionNode::Literal(scalar(1_000_000_001.0)),
                    ExpressionNode::ContinuousNoise {
                        seed: 1,
                        element: 0,
                        input: 0,
                    },
                ];
            }
            "data_missing" => {
                e.version = 3;
                e.nodes = vec![
                    ExpressionNode::Literal(scalar(0.0)),
                    ExpressionNode::DataAssetCell {
                        asset: AssetId::new(),
                        column: "v".into(),
                        row: 0,
                        value_type: ValueType::Scalar,
                    },
                ];
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
            plan_hash: None,
            idempotency_key: None,
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
        plan_hash: None,
        idempotency_key: None,
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

#[test]
fn v3_table_and_dynamic_past_sample_persist_render_and_fail_strictly() {
    let s = Service::new(BackendSelection::CpuReference);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v3.kronello");
    let mut p: Project =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    p.texts.clear();
    let table = ExpressionDataAsset::new(
        AssetId::new(),
        DataTable {
            columns: [("offset".into(), ValueType::Scalar)].into(),
            rows: vec![[("offset".into(), scalar(0.1))].into()],
        },
    )
    .unwrap();
    p.expression_data_assets
        .push(DocumentObject::Known(table.clone()));
    let source = Expression {
        id: ExpressionId::new(),
        version: 3,
        value_type: ValueType::Scalar,
        budget: Default::default(),
        nodes: vec![ExpressionNode::Time],
    };
    let registry = SchemaRegistry::with_builtin();
    let DocumentObject::Known(c) = &mut p.compositions[0] else {
        panic!()
    };
    c.nodes.truncate(1);
    c.root_nodes.truncate(1);
    c.nodes[0]
        .properties
        .sort_by_key(|p| p.descriptor().key.as_str() != "kronello.opacity");
    let mut upstream = c.nodes[0].clone();
    upstream.id = NodeId::new();
    upstream.kind = NodeKind::Null;
    let opacity = upstream
        .properties
        .iter()
        .find(|p| p.descriptor().key.as_str() == "kronello.opacity")
        .unwrap();
    upstream.properties = vec![
        Property::new(
            PropertyId::new(),
            opacity.descriptor().clone(),
            PropertySource::Expression(source.id),
            vec![],
            &registry,
        )
        .unwrap(),
    ];
    let mut e = expression(0.0);
    e.version = 3;
    e.nodes = vec![
        ExpressionNode::Time,
        ExpressionNode::Literal(scalar(0.5)),
        ExpressionNode::Multiply { left: 0, right: 1 },
        ExpressionNode::PropertySample {
            node: Some(upstream.id),
            property: upstream.properties[0].id(),
            value_type: ValueType::Scalar,
            lookback: 2,
        },
        ExpressionNode::Literal(scalar(0.0)),
        ExpressionNode::DataAssetCell {
            asset: table.id,
            column: "offset".into(),
            row: 4,
            value_type: ValueType::Scalar,
        },
        ExpressionNode::Add { left: 3, right: 5 },
    ];
    c.root_nodes.push(upstream.id);
    c.nodes.push(upstream);
    p.expressions.push(DocumentObject::Known(source.clone()));
    s.dispatch(Request::ProjectCreate(CreateRequest {
        project: path.clone(),
        document: p.clone(),
        plan_hash: None,
        idempotency_key: None,
    }))
    .unwrap();
    let plan = plan(
        &s,
        &path,
        "1",
        vec![
            EditCommand::ExpressionSet { expression: source },
            commands(&p, &e)[0].clone(),
            commands(&p, &e)[1].clone(),
        ],
    );
    let wire = serde_json::to_string(&Request::EditPlan(PlanRequest {
        project: path.clone(),
        base_revision: "1".into(),
        commands: plan.commands.clone(),
    }))
    .unwrap();
    assert!(matches!(s.execute_json(&wire), Response::Success { .. }));
    let applied = apply(&s, &path, &plan, "v3").unwrap();
    assert_eq!(apply(&s, &path, &plan, "v3").unwrap().id, applied.id);
    assert_eq!(
        sample(&s, &path, &p).unwrap().samples[0].values,
        vec![scalar(0.1), scalar(0.35)]
    );
    let snapshot =
        RenderSnapshot::new(&plan.candidate, comp(&p).id, 2, RenderProfile::default()).unwrap();
    assert_eq!(snapshot.semantic_versions().expression, 3);
    let saved: RenderSnapshot =
        serde_json::from_slice(&serde_json::to_vec(&snapshot).unwrap()).unwrap();
    let mut frames = std::collections::BTreeMap::new();
    for n in [2, 0, 1, 2] {
        let time = Time::new(n, 4).unwrap();
        let frame = kronello_render::render_frame(
            &saved,
            &[],
            &kronello_gpu::render_adapter::CpuReferenceBackend,
            kronello_render::FrameRequest {
                time,
                region: kronello_render::OutputRegion {
                    origin: [0., 0.],
                    extent: [64., 32.],
                    pixels: [64, 32],
                },
            },
        )
        .unwrap();
        let alpha = frame
            .pixels
            .linear
            .iter()
            .map(|p| p[3])
            .fold(0.0_f32, f32::max);
        assert!((alpha - (0.1 + n as f32 / 8.)).abs() < 1e-6);
        if let Some(old) = frames.insert(n, frame.pixels.linear.clone()) {
            assert_eq!(old, frame.pixels.linear);
        }
    }
    let mut noise_project = plan.candidate.clone();
    let mut noise = e.clone();
    noise.nodes = vec![
        ExpressionNode::Time,
        ExpressionNode::ContinuousNoise {
            seed: 19,
            element: 7,
            input: 0,
        },
        ExpressionNode::Literal(scalar(0.5)),
        ExpressionNode::Multiply { left: 1, right: 2 },
        ExpressionNode::Literal(scalar(0.5)),
        ExpressionNode::Add { left: 3, right: 4 },
    ];
    for expression in &mut noise_project.expressions {
        if matches!(expression,DocumentObject::Known(v) if v.id==e.id) {
            *expression = DocumentObject::Known(noise.clone());
        }
    }
    let noise_plan = self::plan(
        &s,
        &path,
        "2",
        vec![EditCommand::ExpressionSet {
            expression: noise.clone(),
        }],
    );
    let noise_event = apply(&s, &path, &noise_plan, "v3-noise").unwrap();
    assert_eq!(
        apply(&s, &path, &noise_plan, "v3-noise").unwrap().id,
        noise_event.id
    );
    let noise_snapshot =
        RenderSnapshot::new(&noise_project, comp(&p).id, 2, RenderProfile::default()).unwrap();
    let expected_query: Vec<_> = [Time::ZERO, Time::new(1, 2).unwrap()]
        .into_iter()
        .map(|time| scalar(build_scene_ir(&noise_snapshot, time, &[]).unwrap().nodes[0].opacity))
        .collect();
    assert_eq!(
        sample(&s, &path, &p).unwrap().samples[0].values,
        expected_query
    );
    let mut noise_frames = std::collections::BTreeMap::new();
    for n in [2, 0, 1, 2] {
        let time = Time::new(n, 4).unwrap();
        let expected = build_scene_ir(&noise_snapshot, time, &[]).unwrap().nodes[0].opacity as f32;
        let frame = kronello_render::render_frame(
            &noise_snapshot,
            &[],
            &kronello_gpu::render_adapter::CpuReferenceBackend,
            kronello_render::FrameRequest {
                time,
                region: kronello_render::OutputRegion {
                    origin: [0., 0.],
                    extent: [64., 32.],
                    pixels: [64, 32],
                },
            },
        )
        .unwrap();
        let alpha = frame
            .pixels
            .linear
            .iter()
            .map(|p| p[3])
            .fold(0.0_f32, f32::max);
        assert!((alpha - expected).abs() < 1e-6);
        if let Some(old) = noise_frames.insert(n, frame.pixels.linear.clone()) {
            assert_eq!(old, frame.pixels.linear);
        }
    }
    let mut older = serde_json::to_value(&saved).unwrap();
    older["semantic_versions"]["expression"] = serde_json::json!(2);
    assert!(
        build_scene_ir(
            &serde_json::from_value::<RenderSnapshot>(older).unwrap(),
            Time::ZERO,
            &[]
        )
        .is_err()
    );
    let mut bad = plan.candidate.clone();
    let DocumentObject::Known(data) = &mut bad.expression_data_assets[0] else {
        panic!()
    };
    data.table.rows[0].insert("offset".into(), scalar(0.9)); // Saved hash remains immutable.
    assert!(bad.ensure_editable().is_err());
    assert!(RenderSnapshot::new(&bad, comp(&p).id, 2, RenderProfile::default()).is_err());
}

#[test]
fn expression_text_edit_commits_complete_text_and_reports_typed_diagnostics() {
    let s = Service::new(BackendSelection::CpuReference);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("expr-text.kronello");
    let p = document();
    s.dispatch(Request::ProjectCreate(CreateRequest {
        plan_hash: None,
        idempotency_key: None,
        project: path.clone(),
        document: p.clone(),
    }))
    .unwrap();
    let object = comp(&p).nodes[0].id.as_uuid();
    let property = comp(&p).nodes[0].properties[0].id();
    let metadata = || ExpressionMetadata {
        id: ExpressionId::new(),
        version: EXPRESSION_SUPPORTED_VERSION,
        value_type: ValueType::Scalar,
        budget: Default::default(),
    };
    let text_set = |text: &str| {
        vec![EditCommand::PropertyExpressionTextSet {
            object,
            property,
            text: text.into(),
            metadata: metadata(),
        }]
    };
    // Malformed text fails planning with typed syntax diagnostics; nothing is
    // stored and the revision does not move.
    let err = s
        .dispatch(Request::EditPlan(PlanRequest {
            project: path.clone(),
            base_revision: "1".into(),
            commands: text_set("1 +"),
        }))
        .unwrap_err();
    assert_eq!(err.code, "EXPRESSION_SYNTAX");
    let diagnostics = &err.details.as_ref().unwrap()["diagnostics"];
    assert!(diagnostics[0]["line"].as_u64().unwrap() >= 1);
    assert!(diagnostics[0]["byte_end"].as_u64().unwrap() > 0);
    assert!(diagnostics[0]["expected"].is_array());
    // A declared type that the text cannot produce is a typed AST error.
    let err = s
        .dispatch(Request::EditPlan(PlanRequest {
            project: path.clone(),
            base_revision: "1".into(),
            commands: text_set("vec2(1, 2)"),
        }))
        .unwrap_err();
    assert_eq!(err.code, "EVALUATION_ERROR");
    // Only complete committed text is applied: the failed plans above left
    // neither expressions nor revisions behind.
    let plan = plan(&s, &path, "1", text_set("0.3 + 0.1"));
    let event = apply(&s, &path, &plan, "set-expression-text").unwrap();
    assert_eq!(event.revision, 2);
    assert_eq!(
        apply(&s, &path, &plan, "set-expression-text").unwrap().id,
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
    let DocumentObject::Known(stored) = plan.candidate.expressions[0].clone() else {
        panic!()
    };
    // expression.format returns the canonical text for a stored id and for a
    // supplied AST, and rejects unknown ids with a typed error.
    let ResultData::ExpressionText(result) = s
        .dispatch(Request::ExpressionFormat(ExpressionFormatRequest {
            project: path.clone(),
            expression_id: Some(stored.id),
            expression: None,
        }))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(result.text, "0.3 + 0.1");
    assert_eq!(result.revision.as_deref(), Some("2"));
    let ResultData::ExpressionText(result) = s
        .dispatch(Request::ExpressionFormat(ExpressionFormatRequest {
            project: path.clone(),
            expression_id: None,
            expression: Some(Box::new(expression(0.9))),
        }))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(result.text, "0.9");
    assert!(result.revision.is_none());
    assert_eq!(
        s.dispatch(Request::ExpressionFormat(ExpressionFormatRequest {
            project: path.clone(),
            expression_id: Some(ExpressionId::new()),
            expression: None,
        }))
        .unwrap_err()
        .code,
        "EXPRESSION_NOT_FOUND"
    );
    assert_eq!(
        s.dispatch(Request::ExpressionFormat(ExpressionFormatRequest {
            project: path.clone(),
            expression_id: None,
            expression: None,
        }))
        .unwrap_err()
        .code,
        "INVALID_REQUEST"
    );
    // Undo restores the previous source and removes the stored expression.
    let ResultData::Edit(undo) = s
        .dispatch(Request::EditUndo(UndoRequest {
            project: path.clone(),
            base_revision: "2".into(),
            session_id: Uuid::from_u128(123),
            idempotency_key: "undo-expression-text".into(),
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
        s.dispatch(Request::ExpressionFormat(ExpressionFormatRequest {
            project: path.clone(),
            expression_id: Some(stored.id),
            expression: None,
        }))
        .unwrap_err()
        .code,
        "EXPRESSION_NOT_FOUND"
    );
    // The exact same command payload goes through the public wire decoder.
    let wire = serde_json::json!({
        "operation": "edit.plan",
        "project": path,
        "base_revision": "3",
        "commands": text_set("0.5"),
    });
    let schema = serde_json::to_value(api_json_schema()).unwrap();
    jsonschema::validator_for(&schema)
        .unwrap()
        .validate(&wire)
        .unwrap();
}
