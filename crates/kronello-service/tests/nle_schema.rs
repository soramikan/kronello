//! Crate-scoped schema generator using the same functions as the existing examples.
#[test]
fn public_schemas_match_rust_generators() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (name, value) in [
        (
            "project-v1.schema.json",
            serde_json::to_string_pretty(&kronello_model::project_json_schema()).unwrap(),
        ),
        (
            "api-v1.schema.json",
            serde_json::to_string_pretty(&kronello_service::api_json_schema()).unwrap(),
        ),
    ] {
        let path = root.join("schemas").join(name);
        let generated = value + "\n";
        if std::env::var("KRONELLO_SCHEMA_UPDATE").as_deref() == Ok("1") {
            std::fs::write(&path, &generated).unwrap();
        }
        assert_eq!(std::fs::read_to_string(path).unwrap(), generated);
    }
}
