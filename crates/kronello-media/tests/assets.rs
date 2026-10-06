use kronello_media::*;
use kronello_model::*;
use std::path::Path;
fn asset(path: &Path) -> Asset {
    Asset {
        id: AssetId::new(),
        content_hash: content_hash(path).unwrap(),
        kind: AssetKind::Video,
        streams: vec![],
        locator: AssetLocator {
            relative: Some("media.bin".into()),
            absolute: Some(path.to_str().unwrap().into()),
        },
    }
}
#[test]
fn relative_first_absolute_fallback_mismatch_missing_and_reverify() {
    let temp = tempfile::tempdir().unwrap();
    let original = temp.path().join("original.bin");
    std::fs::write(&original, b"original").unwrap();
    let mut a = asset(&original);
    let project = temp.path().join("project.kronello");
    assert_eq!(
        resolve_asset(&a, &project).unwrap(),
        original.canonicalize().unwrap()
    );
    let relative = temp.path().join("media.bin");
    std::fs::write(&relative, b"original").unwrap();
    assert_eq!(
        resolve_asset(&a, &project).unwrap(),
        relative.canonicalize().unwrap()
    );
    std::fs::write(&relative, b"changed!").unwrap();
    assert_eq!(
        resolve_asset(&a, &project).unwrap_err().code(),
        "ASSET_HASH_MISMATCH"
    );
    std::fs::remove_file(relative).unwrap();
    std::fs::remove_file(original).unwrap();
    assert_eq!(
        resolve_asset(&a, &project).unwrap_err().code(),
        "ASSET_MISSING"
    );
    a.locator.relative = Some("../escape".into());
    assert_eq!(
        resolve_asset(&a, &project).unwrap_err().code(),
        "INVALID_MEDIA_INPUT"
    );
}

#[test]
fn locate_uses_the_same_candidates_but_does_not_verify_content() {
    let temp = tempfile::tempdir().unwrap();
    let original = temp.path().join("original.bin");
    std::fs::write(&original, b"original").unwrap();
    let a = asset(&original);
    let relative = temp.path().join("media.bin");
    std::fs::write(&relative, b"different content").unwrap();
    let project = temp.path().join("project.kronello");
    let located = locate_asset(&a, &project).unwrap();
    assert_eq!(located.path, relative.canonicalize().unwrap());
    assert_eq!(located.size_bytes, 17);
    assert_eq!(
        resolve_asset(&a, &project).unwrap_err().code(),
        "ASSET_HASH_MISMATCH"
    );
}
#[test]
fn relink_matches_hash_and_never_changes_original_on_failure() {
    let temp = tempfile::tempdir().unwrap();
    let original = temp.path().join("original");
    std::fs::write(&original, b"content").unwrap();
    let a = asset(&original);
    std::fs::remove_file(original).unwrap();
    let search = temp.path().join("search");
    std::fs::create_dir(&search).unwrap();
    std::fs::write(search.join("wrong"), b"wrong").unwrap();
    assert_eq!(
        relink_asset(&a, &temp.path().join("project.kronello"), &search)
            .unwrap_err()
            .code(),
        "ASSET_MISSING"
    );
    std::fs::write(search.join("different-name"), b"content").unwrap();
    let linked = relink_asset(&a, &temp.path().join("project.kronello"), &search).unwrap();
    assert_eq!(linked.id, a.id);
    assert_eq!(linked.content_hash, a.content_hash);
    assert_eq!(
        linked.locator.relative.as_deref(),
        Some("search/different-name")
    );
    assert_eq!(a.locator.relative.as_deref(), Some("media.bin"));
}
#[test]
fn collect_relative_paths_is_portable_and_failures_leave_no_output() {
    let temp = tempfile::tempdir().unwrap();
    let original = temp.path().join("original.bin");
    std::fs::write(&original, b"content").unwrap();
    let a = asset(&original);
    let mut project = Project::default();
    project.assets.push(DocumentObject::Known(a));
    let output = temp.path().join("collected");
    let write = |path: &Path, project: &Project| -> Result<(), MediaError> {
        std::fs::write(path, serde_json::to_vec(project)?)?;
        Ok(())
    };
    let result = collect_project(
        &project,
        &temp.path().join("project.kronello"),
        &output,
        write,
    )
    .unwrap();
    assert_eq!(result.asset_count, 1);
    let moved = temp.path().join("moved");
    std::fs::rename(output, &moved).unwrap();
    std::fs::remove_file(&original).unwrap();
    let copy: Project =
        serde_json::from_slice(&std::fs::read(moved.join("project.kronello")).unwrap()).unwrap();
    let DocumentObject::Known(a) = &copy.assets[0] else {
        panic!()
    };
    assert!(a.locator.absolute.is_none());
    assert!(resolve_asset(a, &moved.join("project.kronello")).is_ok());
    assert_eq!(
        collect_project(&copy, &moved.join("project.kronello"), &moved, write)
            .unwrap_err()
            .code(),
        "OUTPUT_EXISTS"
    );
    let failed = temp.path().join("failed");
    assert!(
        collect_project(
            &project,
            &temp.path().join("project.kronello"),
            &failed,
            write
        )
        .is_err()
    );
    assert!(!failed.exists());
    let failed = temp.path().join("writer-failed");
    let result =
        collect_project::<MediaError>(&copy, &moved.join("project.kronello"), &failed, |_, _| {
            Err(MediaError::InvalidInput("injected writer failure".into()))
        });
    assert!(result.is_err());
    assert!(!failed.exists());
}
#[test]
fn asset_schema_roundtrip_preserves_future_fields_and_duplicate_ids_fail() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("asset");
    std::fs::write(&path, b"content").unwrap();
    let mut p = Project::default();
    p.assets.push(DocumentObject::Known(asset(&path)));
    p.validate_storage().unwrap();
    let json = serde_json::to_value(&p).unwrap();
    let copy: Project = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(copy, p);
    let mut future = json;
    future["assets"][0]["future"] = serde_json::json!({"unknown":true});
    let copy: Project = serde_json::from_value(future.clone()).unwrap();
    assert!(matches!(copy.assets[0], DocumentObject::Opaque(_)));
    assert_eq!(serde_json::to_value(&copy).unwrap(), future);
    assert!(copy.ensure_editable().is_err());
    p.assets.push(p.assets[0].clone());
    assert!(p.validate_storage().is_err());
}
