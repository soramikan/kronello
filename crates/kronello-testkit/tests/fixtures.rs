use kronello_testkit::{EXTERNAL_FIXTURE_DIR_ENV, FixtureError, FixtureResolver, resolve_fixture};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

const ABC_HASH: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

fn manifest(root: &Path, storage: &str) -> Value {
    fs::create_dir_all(root.join("tests/fixtures/data")).unwrap();
    json!({"schema_version": 1, "fixtures": [{
        "id": "test", "storage": storage,
        "path": if storage == "bundled" { "tests/fixtures/data/test.bin" } else { "external/test.bin" },
        "bytes": 3, "sha256": ABC_HASH
    }]})
}

fn write_manifest(root: &Path, value: &Value) {
    fs::write(root.join("tests/fixtures/manifest.json"), value.to_string()).unwrap();
}

fn setup(storage: &str) -> TempDir {
    let temp = tempfile::tempdir().unwrap();
    let value = manifest(temp.path(), storage);
    write_manifest(temp.path(), &value);
    fs::create_dir_all(temp.path().join("target/fixtures/external")).unwrap();
    temp
}

fn resolver(root: &Path) -> FixtureResolver {
    FixtureResolver::with_external_dir(root, root.join("target/fixtures/external")).unwrap()
}

#[test]
fn resolves_noto_sans_cjk_jp_from_actual_manifest() {
    // Required download: intentionally fails if the fetch step has not run.
    let path = resolve_fixture("noto-sans-cjk-jp").unwrap();
    assert!(path.is_absolute());
    assert_eq!(path.file_name().unwrap(), "NotoSansCJKjp-Regular.otf");
    assert_eq!(fs::metadata(path).unwrap().len(), 16_467_736);
}

#[test]
fn resolves_bundled_fixture_from_actual_manifest() {
    let path = resolve_fixture("timing").unwrap();
    assert!(path.is_absolute());
    assert_eq!(path.file_name().unwrap(), "timing.json");
}

#[test]
fn resolves_verified_bundled_fixture() {
    let temp = setup("bundled");
    let path = temp.path().join("tests/fixtures/data/test.bin");
    fs::write(&path, b"abc").unwrap();
    assert_eq!(
        resolver(temp.path()).resolve("test").unwrap(),
        path.canonicalize().unwrap()
    );
}

#[test]
fn missing_fixture_returns_not_found() {
    let temp = setup("external");
    assert!(
        matches!(resolver(temp.path()).resolve("test"), Err(FixtureError::NotFound { id, path })
        if id == "test" && path == temp.path().join("target/fixtures/external/test.bin"))
    );
}

#[test]
fn wrong_byte_count_returns_size_mismatch() {
    let temp = setup("external");
    fs::write(
        temp.path().join("target/fixtures/external/test.bin"),
        b"short",
    )
    .unwrap();
    assert!(matches!(
        resolver(temp.path()).resolve("test"),
        Err(FixtureError::SizeMismatch {
            expected: 3,
            actual: 5,
            ..
        })
    ));
}

#[test]
fn wrong_hash_returns_hash_mismatch() {
    let temp = setup("external");
    fs::write(
        temp.path().join("target/fixtures/external/test.bin"),
        b"xyz",
    )
    .unwrap();
    assert!(matches!(resolver(temp.path()).resolve("test"),
        Err(FixtureError::HashMismatch { expected, actual, .. }) if expected == ABC_HASH && actual != expected));
}

#[test]
fn unknown_id_returns_unknown_id() {
    let temp = setup("external");
    assert!(matches!(resolver(temp.path()).resolve("missing"),
        Err(FixtureError::UnknownId { id }) if id == "missing"));
}

#[test]
fn malformed_manifest_returns_invalid_manifest() {
    let temp = setup("external");
    fs::write(temp.path().join("tests/fixtures/manifest.json"), b"{").unwrap();
    assert!(matches!(
        FixtureResolver::new(temp.path()),
        Err(FixtureError::InvalidManifest { .. })
    ));
}

#[test]
fn missing_manifest_returns_invalid_manifest() {
    let temp = tempfile::tempdir().unwrap();
    assert!(matches!(
        FixtureResolver::new(temp.path()),
        Err(FixtureError::InvalidManifest { .. })
    ));
}

#[test]
fn unreadable_fixture_returns_io_error() {
    let temp = setup("external");
    // A directory cannot be read as fixture bytes on macOS, Linux, or Windows.
    fs::create_dir(temp.path().join("target/fixtures/external/test.bin")).unwrap();
    assert!(matches!(
        resolver(temp.path()).resolve("test"),
        Err(FixtureError::Io { .. })
    ));
}

#[test]
fn invalid_manifest_fields_are_rejected() {
    let temp = setup("bundled");
    let original = manifest(temp.path(), "bundled");
    let mut invalid = vec![json!({}), json!({"schema_version": 2, "fixtures": []})];
    for (key, value) in [
        ("id", json!(null)),
        ("bytes", json!(0)),
        ("bytes", json!(-1)),
        ("sha256", json!("bad")),
        ("path", json!("../escape")),
        ("path", json!("/absolute")),
        ("path", json!("outside/test.bin")),
    ] {
        let mut changed = original.clone();
        changed["fixtures"][0][key] = value;
        invalid.push(changed);
    }
    let mut duplicate = original.clone();
    duplicate["fixtures"]
        .as_array_mut()
        .unwrap()
        .push(original["fixtures"][0].clone());
    invalid.push(duplicate);
    for value in invalid {
        write_manifest(temp.path(), &value);
        assert!(
            matches!(
                FixtureResolver::new(temp.path()),
                Err(FixtureError::InvalidManifest { .. })
            ),
            "{value}"
        );
    }
}

#[test]
fn generated_and_unknown_storage_return_unsupported_storage() {
    for storage in ["generated", "unknown"] {
        let temp = setup(storage);
        assert!(matches!(resolver(temp.path()).resolve("test"),
            Err(FixtureError::UnsupportedStorage { storage: actual, .. }) if actual == storage));
    }
}

#[test]
fn external_directory_default_and_environment_override() {
    // A child process tests env selection without unsafe mutation or parallel-test races.
    if let Some(root) = std::env::var_os("KRONELLO_TEST_FIXTURE_ROOT") {
        let expected = std::env::var_os("KRONELLO_TEST_FIXTURE_EXPECTED").unwrap();
        assert_eq!(
            FixtureResolver::new(root).unwrap().resolve("test").unwrap(),
            Path::new(&expected).canonicalize().unwrap()
        );
        return;
    }
    let temp = setup("external");
    let default = temp.path().join("target/fixtures/external/test.bin");
    fs::write(&default, b"abc").unwrap();
    let override_dir = tempfile::tempdir().unwrap();
    let override_file = override_dir.path().join("test.bin");
    fs::write(&override_file, b"abc").unwrap();
    for external in [None, Some(override_dir.path())] {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "external_directory_default_and_environment_override",
            ])
            .env_remove(EXTERNAL_FIXTURE_DIR_ENV)
            .env("KRONELLO_TEST_FIXTURE_ROOT", temp.path())
            .env(
                "KRONELLO_TEST_FIXTURE_EXPECTED",
                if external.is_some() {
                    &override_file
                } else {
                    &default
                },
            );
        if let Some(path) = external {
            fs::remove_file(&default).unwrap();
            command.env(EXTERNAL_FIXTURE_DIR_ENV, path);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
