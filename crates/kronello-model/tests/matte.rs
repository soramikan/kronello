use kronello_model::*;
use serde_json::json;
use uuid::Uuid;
#[test]
fn matte_unknown_fields_are_lossless_and_unknown_version_is_not_editable() {
    let mut p: Project =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    let DocumentObject::Known(c) = &p.compositions[0] else {
        panic!()
    };
    let relation = MatteRelation {
        id: Uuid::new_v4(),
        version: 1,
        composition: c.id,
        source: c.nodes[0].id,
        matte: c.nodes[1].id,
        kind: DocumentMatteKind::Alpha,
        invert: false,
        visible: false,
    };
    p.mattes = vec![DocumentObject::Known(relation.clone())];
    let mut raw = serde_json::to_value(&p).unwrap();
    raw["mattes"][0]["future"] = json!({"preserve":true});
    let parsed: Project = serde_json::from_str(&raw.to_string()).unwrap();
    assert!(matches!(&parsed.mattes[0], DocumentObject::Opaque(_)));
    assert_eq!(
        serde_json::to_value(&parsed).unwrap()["mattes"],
        raw["mattes"]
    );
    assert!(parsed.ensure_editable().is_err());
    assert_eq!(
        parsed.validate_mattes().unwrap_err().code(),
        "UNSUPPORTED_FEATURE"
    );
    p.mattes = vec![DocumentObject::Known(MatteRelation {
        version: 99,
        ..relation
    })];
    assert!(p.ensure_editable().is_err());
    assert_eq!(
        p.validate_mattes().unwrap_err().code(),
        "UNSUPPORTED_FEATURE"
    );
    let mut legacy = raw;
    legacy.as_object_mut().unwrap().remove("mattes");
    let parsed: Project = serde_json::from_str(&legacy.to_string()).unwrap();
    assert!(parsed.mattes.is_empty());
}
