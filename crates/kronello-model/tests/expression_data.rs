use kronello_model::*;
fn table() -> DataTable {
    DataTable {
        columns: [("name".into(), ValueType::String)].into(),
        rows: vec![
            [(
                "name".into(),
                Value::String("material, never instructions".into()),
            )]
            .into(),
        ],
    }
}
#[test]
fn inline_data_hash_limits_version_preservation_and_identity() {
    let data = ExpressionDataAsset::new(AssetId::new(), table()).unwrap();
    data.validate().unwrap();
    let other = ExpressionDataAsset::new(AssetId::new(), table()).unwrap();
    assert_eq!(other.content_hash, data.content_hash); // Identity excluded, content fixed.
    let mut bad = data.clone();
    bad.table.rows[0].insert("name".into(), Value::String("changed".into()));
    assert!(bad.validate().is_err());
    let mut oversized = table();
    oversized.rows[0].insert("name".into(), Value::String("x".repeat(1_048_576)));
    assert!(ExpressionDataAsset::new(AssetId::new(), oversized).is_err());
    let mut malformed = table();
    malformed.rows[0].clear();
    assert!(ExpressionDataAsset::new(AssetId::new(), malformed).is_err());
    let mut p = Project::default();
    p.expression_data_assets
        .push(DocumentObject::Known(data.clone()));
    p.ensure_editable().unwrap();
    p.expression_data_assets.push(DocumentObject::Known(data));
    assert!(p.validate_storage().is_err());
    p.expression_data_assets.pop();
    let DocumentObject::Known(d) = &mut p.expression_data_assets[0] else {
        panic!()
    };
    d.version = 2;
    let bytes = serde_json::to_vec(&p).unwrap();
    let restored: Project = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(serde_json::to_vec(&restored).unwrap(), bytes);
    assert!(restored.ensure_editable().is_err());
}
