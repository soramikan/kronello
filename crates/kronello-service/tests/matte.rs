use kronello_model::*;
use kronello_service::*;
use kronello_time::Time;
use serde_json::json;
use uuid::Uuid;
fn run(value: serde_json::Value) -> Result<ResultData, ServiceError> {
    Service::new(BackendSelection::CpuReference)
        .dispatch(serde_json::from_str(&value.to_string()).unwrap())
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
    let ResultData::Edit(e) = run(request.clone()).unwrap() else {
        panic!()
    };
    let ResultData::Edit(again) = run(request).unwrap() else {
        panic!()
    };
    assert_eq!(e, again);
    e
}
fn frame(path: &std::path::Path, c: CompositionId) -> FrameResult {
    let ResultData::Frame(r)=run(json!({"operation":"render.frame","input":{"project":path,"composition":c,"region":{"origin":[0.,0.],"extent":[64.,64.],"pixels":[64,64]}},"time":Time::ZERO})).unwrap()else{panic!()};
    *r
}
#[test]
fn persisted_shared_matte_alpha_luminance_invert_revision_undo_and_errors() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("matte.kronello");
    let mut p: Project =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    let DocumentObject::Known(c) = &mut p.compositions[0] else {
        panic!()
    };
    let mut prototype = c.nodes[0].clone();
    prototype.properties.clear();
    prototype.containment_parent = None;
    prototype.transform_parent = None;
    prototype.child_order.clear();
    c.nodes.clear();
    c.root_nodes.clear();
    c.design_extent = DesignExtent::new(64., 64.).unwrap();
    let comp = c.id;
    p.shapes.clear();
    p.texts.clear();
    run(json!({"operation":"project.create","project":path,"document":p})).unwrap();
    let mut source = prototype.clone();
    source.id = NodeId::new();
    let s = ContentId::new();
    source.kind = NodeKind::Shape { content_ref: s };
    let mut matte = prototype;
    matte.id = NodeId::new();
    let m = ContentId::new();
    matte.kind = NodeKind::Shape { content_ref: m };
    let targets = vec![
        SvgImportTarget {
            shape: s,
            path_property: PropertyId::new(),
            fill_property: PropertyId::new(),
            node: source.clone(),
            index: 0,
        },
        SvgImportTarget {
            shape: m,
            path_property: PropertyId::new(),
            fill_property: PropertyId::new(),
            node: matte.clone(),
            index: 1,
        },
    ];
    let ResultData::Plan(plan)=run(json!({"operation":"svg.import_plan","project":path,"base_revision":export(&path).revision,"composition":comp,"targets":targets,"svg":"<svg><path d='M10 10L50 10L50 50L10 50Z' fill='#00ff00'/><path d='M10 10L30 10L30 50L10 50Z' fill='#ff0000'/></svg>"})).unwrap()else{panic!()};
    apply(&path, plan.commands.clone(), "setup");
    let id = Uuid::new_v4();
    let mut relation = MatteRelation {
        id,
        version: DOCUMENT_MATTE_VERSION,
        composition: comp,
        source: source.id,
        matte: matte.id,
        kind: DocumentMatteKind::Alpha,
        invert: false,
        visible: false,
    };
    let before = export(&path).revision;
    let event = apply(
        &path,
        vec![EditCommand::MatteSet {
            matte: relation.clone(),
        }],
        "alpha",
    );
    let stored = export(&path);
    assert_eq!(
        stored.document.mattes,
        vec![DocumentObject::Known(relation.clone())]
    );
    let image = frame(&path, comp);
    assert_eq!(image.linear[20 * 64 + 20], [0., 1., 0., 1.]);
    assert_eq!(image.linear[20 * 64 + 40], [0.; 4]);
    let conflict=run(json!({"operation":"edit.plan","project":path,"base_revision":before,"commands":[EditCommand::MatteRemove{id}]})).unwrap_err();
    assert_eq!(conflict.code, "REVISION_CONFLICT");
    run(json!({"operation":"edit.undo","project":path,"base_revision":event.revision.to_string(),"event_id":event.id,"session_id":Uuid::from_u128(2),"idempotency_key":"undo-alpha"})).unwrap();
    assert!(export(&path).document.mattes.is_empty());
    for (kind, invert, inside, outside, key) in [
        (DocumentMatteKind::Alpha, true, 0., 1., "invert-alpha"),
        (DocumentMatteKind::Luminance, false, 0.2126, 0., "luma"),
        (
            DocumentMatteKind::Luminance,
            true,
            0.7874,
            1.,
            "invert-luma",
        ),
    ] {
        relation.kind = kind;
        relation.invert = invert;
        apply(
            &path,
            vec![EditCommand::MatteSet {
                matte: relation.clone(),
            }],
            key,
        );
        let image = frame(&path, comp);
        assert!((image.linear[20 * 64 + 20][3] - inside).abs() < 1e-5);
        assert!((image.linear[20 * 64 + 40][3] - outside).abs() < 1e-5);
    }
    let disabled = apply(
        &path,
        vec![EditCommand::NodeEnabledSet {
            composition: comp,
            node: matte.id,
            enabled: false,
        }],
        "disable-matte",
    );
    let missing=run(json!({"operation":"render.frame","input":{"project":path,"composition":comp,"region":{"origin":[0.,0.],"extent":[64.,64.],"pixels":[64,64]}},"time":Time::ZERO})).unwrap_err();
    assert_eq!(missing.code, "MATTE_MISSING");
    run(json!({"operation":"edit.undo","project":path,"base_revision":disabled.revision.to_string(),"event_id":disabled.id,"session_id":Uuid::from_u128(2),"idempotency_key":"undo-disable"})).unwrap();
    let revision = export(&path).revision;
    let reverse = MatteRelation {
        id: Uuid::new_v4(),
        source: matte.id,
        matte: source.id,
        ..relation.clone()
    };
    for (commands, code) in [
        (
            vec![EditCommand::MatteSet { matte: reverse }],
            "MATTE_CYCLE",
        ),
        (
            vec![EditCommand::MatteSet {
                matte: MatteRelation {
                    matte: NodeId::new(),
                    ..relation.clone()
                },
            }],
            "MATTE_MISSING",
        ),
        (
            vec![EditCommand::MatteSet {
                matte: MatteRelation {
                    version: 99,
                    ..relation.clone()
                },
            }],
            "UNSUPPORTED_FEATURE",
        ),
        (
            vec![EditCommand::NodeRemove {
                composition: comp,
                node: matte.id,
            }],
            "MATTE_MISSING",
        ),
    ] {
        let e=run(json!({"operation":"edit.plan","project":path,"base_revision":revision,"commands":commands})).unwrap_err();
        assert_eq!(e.code, code);
    }
    let document = export(&path).document;
    let snapshot =
        kronello_render::RenderSnapshot::new(&document, comp, 1, Default::default()).unwrap();
    let frozen = serde_json::to_string(&snapshot).unwrap();
    let parsed: kronello_render::RenderSnapshot = serde_json::from_str(&frozen).unwrap();
    assert_eq!(
        snapshot.content_hash().unwrap(),
        parsed.content_hash().unwrap()
    );
    let mut versions = kronello_render::SemanticVersions::current(document.semantic_version);
    versions.document_matte = None;
    assert_eq!(
        kronello_render::RenderSnapshot::with_contract(
            &document,
            comp,
            1,
            Default::default(),
            versions,
            vec![]
        )
        .unwrap_err()
        .code(),
        "UNSUPPORTED_FEATURE"
    );
}
