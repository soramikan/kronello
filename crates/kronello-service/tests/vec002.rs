use kronello_model::*;
use kronello_service::*;
use kronello_time::Time;
use serde_json::json;
use uuid::Uuid;
fn run(value: serde_json::Value) -> Result<ResultData, ServiceError> {
    Service::new(BackendSelection::CpuReference)
        .dispatch(serde_json::from_str(&value.to_string()).unwrap())
}
fn frame(path: &std::path::Path, composition: CompositionId) -> FrameResult {
    let ResultData::Frame(frame)=run(json!({"operation":"render.frame","input":{"project":path,"composition":composition,"region":{"origin":[0.0,0.0],"extent":[64.0,64.0],"pixels":[64,64]}},"time":Time::ZERO})).unwrap()else{panic!()};
    *frame
}
fn apply(path: &std::path::Path, plan: &EditPlan, key: &str) -> kronello_store::Event {
    let request = json!({"operation":"edit.apply","project":path,"base_revision":plan.base_revision,"plan_hash":plan.plan_hash,"commands":plan.commands,"session_id":Uuid::from_u128(22),"idempotency_key":key});
    let ResultData::Edit(event) = run(request.clone()).unwrap() else {
        panic!()
    };
    let ResultData::Edit(replay) = run(request).unwrap() else {
        panic!()
    };
    assert_eq!(event, replay);
    event
}
#[test]
fn shared_svg_import_plan_transaction_render_and_selective_undo() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("svg.kronello");
    let mut document: Project =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    let DocumentObject::Known(c) = &mut document.compositions[0] else {
        panic!()
    };
    let mut node = c.nodes[0].clone();
    node.id = NodeId::new();
    node.properties.clear();
    node.containment_parent = None;
    node.transform_parent = None;
    node.child_order.clear();
    c.nodes.clear();
    c.root_nodes.clear();
    c.design_extent = DesignExtent::new(64.0, 64.0).unwrap();
    let composition = c.id;
    document.shapes.clear();
    document.texts.clear();
    let ResultData::Project(created) =
        run(json!({"operation":"project.create","project":path,"document":document})).unwrap()
    else {
        panic!()
    };
    let shape = ContentId::new();
    node.kind = NodeKind::Shape { content_ref: shape };
    let path_id = PropertyId::new();
    let request = json!({"operation":"svg.import_plan","project":path,"base_revision":created.revision,"composition":composition,"svg":"<svg><path d='M10 10L30 10L30 30L10 30Z' fill='#ff0000'/></svg>","targets":[{"shape":shape,"path_property":path_id,"fill_property":PropertyId::new(),"node":node,"index":0}]});
    let ResultData::Plan(plan) = run(request).unwrap() else {
        panic!()
    };
    assert_eq!(plan.commands.len(), 2);
    let imported = apply(&path, &plan, "svg-import");
    let image = frame(&path, composition);
    assert!(image.linear.iter().map(|p| p[3]).sum::<f32>() > 390.0);
    assert!(image.linear[20 * 64 + 20][0] > 0.9);
    let start = PropertyId::new();
    let end = PropertyId::new();
    let offset = PropertyId::new();
    let registry = kronello_render::render_registry();
    let make = |id, key: &str, value| {
        Property::new(
            id,
            DescriptorRef::new(registry.lookup(&SchemaKey::new(key).unwrap()).unwrap()),
            PropertySource::Constant(Value::Scalar(FiniteF64::new(value).unwrap())),
            vec![],
            &registry,
        )
        .unwrap()
    };
    let mut trimmed = match &plan.candidate.shapes[0] {
        DocumentObject::Known(s) => s.clone(),
        _ => panic!(),
    };
    trimmed.geometry = ShapeGeometry::TrimmedPath {
        path: path_id,
        start,
        end,
        offset,
    };
    let commands = vec![
        EditCommand::NodePropertyInsert {
            composition,
            node: node.id,
            property: make(start, "kronello.shape.trim_start", 0.0),
        },
        EditCommand::NodePropertyInsert {
            composition,
            node: node.id,
            property: make(end, "kronello.shape.trim_end", 0.5),
        },
        EditCommand::NodePropertyInsert {
            composition,
            node: node.id,
            property: make(offset, "kronello.shape.trim_offset", 0.0),
        },
        EditCommand::ShapeSet { shape: trimmed },
    ];
    let ResultData::Plan(trim_plan)=run(json!({"operation":"edit.plan","project":path,"base_revision":imported.revision.to_string(),"commands":commands})).unwrap()else{panic!()};
    let trimmed_event = apply(&path, &trim_plan, "trim");
    let image = frame(&path, composition);
    let area = image.linear.iter().map(|p| p[3]).sum::<f32>();
    assert!((190.0..210.0).contains(&area), "{area}");
    run(json!({"operation":"edit.undo","project":path,"base_revision":trimmed_event.revision.to_string(),"event_id":trimmed_event.id,"session_id":Uuid::from_u128(23),"idempotency_key":"undo-trim"})).unwrap();
    let image = frame(&path, composition);
    assert!(image.linear.iter().map(|p| p[3]).sum::<f32>() > 390.0);
    let ResultData::Export(current) =
        run(json!({"operation":"project.export","project":path})).unwrap()
    else {
        panic!()
    };
    let to = PropertyId::new();
    let progress = PropertyId::new();
    let target_path =
        kronello_vector::inspect_svg("<svg><path d='M10 10L50 10L50 30L10 30Z'/></svg>")
            .unwrap()
            .paths
            .remove(0)
            .path;
    let target_property = Property::new(
        to,
        DescriptorRef::new(
            registry
                .lookup(&SchemaKey::new("kronello.shape.morph_target").unwrap())
                .unwrap(),
        ),
        PropertySource::Constant(Value::Path(target_path)),
        vec![],
        &registry,
    )
    .unwrap();
    let curve = AnimationCurve::new(
        CurveId::new(),
        ValueType::Scalar,
        vec![
            Keyframe {
                time: Time::ZERO,
                value: Value::Scalar(FiniteF64::new(0.0).unwrap()),
                interpolation: CurveInterpolation::Linear,
            },
            Keyframe {
                time: Time::new(1, 1).unwrap(),
                value: Value::Scalar(FiniteF64::new(1.0).unwrap()),
                interpolation: CurveInterpolation::Linear,
            },
        ],
    )
    .unwrap();
    let mut morph = match &plan.candidate.shapes[0] {
        DocumentObject::Known(s) => s.clone(),
        _ => panic!(),
    };
    morph.geometry = ShapeGeometry::MorphPath {
        from: path_id,
        to,
        progress,
    };
    let commands = vec![
        EditCommand::NodePropertyInsert {
            composition,
            node: node.id,
            property: target_property,
        },
        EditCommand::NodePropertyInsert {
            composition,
            node: node.id,
            property: make(progress, "kronello.shape.morph_progress", 0.0),
        },
        EditCommand::PropertySourceSet {
            object: node.id.as_uuid(),
            property: progress,
            source: PropertySource::Curve(curve.id()),
            curve: Some(curve),
        },
        EditCommand::ShapeSet { shape: morph },
    ];
    let ResultData::Plan(morph_plan)=run(json!({"operation":"edit.plan","project":path,"base_revision":current.revision,"commands":commands})).unwrap()else{panic!()};
    apply(&path, &morph_plan, "morph");
    let render_at = |time| {
        let ResultData::Frame(image)=run(json!({"operation":"render.frame","input":{"project":path,"composition":composition,"region":{"origin":[0.0,0.0],"extent":[64.0,64.0],"pixels":[64,64]}},"time":time})).unwrap()else{panic!()};
        image.linear.iter().map(|p| p[3]).sum::<f32>()
    };
    for (time, area) in [
        (Time::ZERO, 400.0),
        (Time::new(1, 2).unwrap(), 600.0),
        (Time::new(1, 1).unwrap(), 800.0),
        (Time::new(1, 2).unwrap(), 600.0),
    ] {
        assert!((render_at(time) - area).abs() < 2.0);
    }
    let ResultData::Export(current) =
        run(json!({"operation":"project.export","project":path})).unwrap()
    else {
        panic!()
    };
    let bad_path = kronello_vector::inspect_svg("<svg><path d='M0 0L1 1'/></svg>")
        .unwrap()
        .paths
        .remove(0)
        .path;
    let invalid=run(json!({"operation":"edit.plan","project":path,"base_revision":current.revision,"commands":[EditCommand::PropertySourceSet{object:node.id.as_uuid(),property:to,source:PropertySource::Constant(Value::Path(bad_path)),curve:None}]})).unwrap_err();
    assert_eq!(invalid.code, "PATH_MORPH_CORRESPONDENCE");
    // Legacy snapshots remain valid only for content not using the extension.
    let mut versions = kronello_render::SemanticVersions::current(plan.candidate.semantic_version);
    versions.path_operations = None;
    kronello_render::RenderSnapshot::with_contract(
        &plan.candidate,
        composition,
        0,
        Default::default(),
        versions.clone(),
        vec![],
    )
    .unwrap();
    let missing = kronello_render::RenderSnapshot::with_contract(
        &morph_plan.candidate,
        composition,
        0,
        Default::default(),
        versions.clone(),
        vec![],
    )
    .unwrap_err();
    assert_eq!(missing.code(), "UNSUPPORTED_FEATURE");
    versions.path_operations = Some(99);
    let unknown = kronello_render::RenderSnapshot::with_contract(
        &plan.candidate,
        composition,
        0,
        Default::default(),
        versions,
        vec![],
    )
    .unwrap_err();
    assert_eq!(unknown.code(), "UNSUPPORTED_FEATURE");
    let result=run(json!({"operation":"svg.import_plan","project":path,"base_revision":"0","composition":composition,"svg":"<svg><image href='file:///etc/passwd'/></svg>","targets":[]})).unwrap_err();
    assert_eq!(result.code, "UNSUPPORTED_FEATURE");
    assert!(
        result.details.unwrap()["external_references"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "file:///etc/passwd")
    );
}
#[test]
fn svg_queries_roundtrip_and_discover_through_shared_json_protocol() {
    let ResultData::SvgReport(report) =
        run(json!({"operation":"svg.inspect","svg":"<svg><path d='M0 0L1 1' fill='#abc'/></svg>"}))
            .unwrap()
    else {
        panic!()
    };
    let ResultData::SvgExport(export) =
        run(json!({"operation":"svg.export","paths":report.paths})).unwrap()
    else {
        panic!()
    };
    let ResultData::SvgReport(again) =
        run(json!({"operation":"svg.inspect","svg":export.svg})).unwrap()
    else {
        panic!()
    };
    assert_eq!(again, report);
    for name in ["svg.inspect", "svg.export", "svg.import_plan"] {
        assert!(command_registry().iter().any(|d| d.name == name));
    }
    let encoded = serde_json::to_string(&Response::Success {
        result: ResultData::SvgReport(report),
    })
    .unwrap();
    let decoded: Response = serde_json::from_str(&encoded).unwrap();
    assert!(matches!(
        decoded,
        Response::Success {
            result: ResultData::SvgReport(_)
        }
    ));
}
