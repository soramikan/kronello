//! Full-document plans and durable responses shared by every transport.
use std::path::{Path, PathBuf};

use kronello_model::Project;
use kronello_store::{ProjectStore, ServiceReceipt, Snapshot, StoreError};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    CreateRequest, ImportRequest, ProjectInfo, ProjectOpenMode, ServiceError, open_existing,
    parse_revision, read_project_snapshot, snapshot_info,
};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreatePlanRequest {
    pub project: PathBuf,
    pub document: Project,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportPlanRequest {
    pub project: PathBuf,
    pub base_revision: String,
    pub document: Project,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProjectChangeKind {
    Create,
    Import,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectChangePlan {
    pub operation: ProjectChangeKind,
    pub project: PathBuf,
    pub expected_absent: bool,
    pub base_revision: Option<String>,
    pub base_content_hash: Option<String>,
    pub candidate: Project,
    pub plan_hash: String,
}
fn target(path: &Path) -> Result<PathBuf, ServiceError> {
    if path.extension().is_none_or(|ext| ext != "kronello") {
        return Err(ServiceError::invalid(
            "project must use .kronello extension",
        ));
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    Ok(parent.canonicalize()?.join(
        path.file_name()
            .ok_or_else(|| ServiceError::invalid("missing filename"))?,
    ))
}
fn exists(path: &Path) -> Result<bool, ServiceError> {
    match path.symlink_metadata() {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
fn validate(document: &Project) -> Result<(), ServiceError> {
    document.validate_storage().map_err(StoreError::from)?;
    kronello_template::validate_stored_project(document)?;
    Ok(())
}
fn hash(value: &impl Serialize) -> Result<String, ServiceError> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&serde_json::to_value(value)?)?)
    ))
}
fn build(
    project: PathBuf,
    document: Project,
    previous: Option<&Snapshot>,
) -> Result<ProjectChangePlan, ServiceError> {
    validate(&document)?;
    if let Some(previous) = previous {
        kronello_template::validate_stored_transition(&previous.document, &document)?;
    }
    let mut plan = ProjectChangePlan {
        operation: if previous.is_some() {
            ProjectChangeKind::Import
        } else {
            ProjectChangeKind::Create
        },
        project,
        expected_absent: previous.is_none(),
        base_revision: previous.map(|s| s.revision.to_string()),
        base_content_hash: previous.map(|s| hash(&s.document)).transpose()?,
        candidate: document,
        plan_hash: String::new(),
    };
    plan.plan_hash = hash(&plan)?;
    Ok(plan)
}
fn revision(base: u64, current: u64) -> Result<(), ServiceError> {
    if base == current {
        Ok(())
    } else {
        Err(StoreError::RevisionConflict { base, current }.into())
    }
}
fn check_hash(expected: Option<&str>, plan: &ProjectChangePlan) -> Result<(), ServiceError> {
    if expected.is_some_and(|hash| hash != plan.plan_hash) {
        return Err(ServiceError::new(
            "PLAN_HASH_MISMATCH",
            "project change differs from the planned hash",
        ));
    }
    Ok(())
}
fn key(value: Option<&str>) -> Result<(), ServiceError> {
    if value.is_some_and(|k| k.is_empty() || k.len() > 256) {
        return Err(ServiceError::invalid(
            "idempotency_key must contain 1..256 UTF-8 bytes",
        ));
    }
    Ok(())
}
fn receipt_result(
    record: kronello_store::IdempotencyRecord,
    payload: &Value,
) -> Result<ProjectInfo, ServiceError> {
    if record.service_payload.as_ref() != Some(payload) {
        return Err(StoreError::IdempotencyKeyReused.into());
    }
    let result = record
        .service_result
        .ok_or_else(|| ServiceError::new("STORAGE_ERROR", "project receipt has no result"))?;
    serde_json::from_value(result).map_err(|e| ServiceError::new("STORAGE_ERROR", e.to_string()))
}
fn retry(
    path: &Path,
    key: Option<&str>,
    payload: &Value,
) -> Result<Option<ProjectInfo>, ServiceError> {
    if let Some(key) = key {
        return ProjectStore::read_idempotency_record(path, key)?
            .map(|r| receipt_result(r, payload))
            .transpose();
    }
    Ok(None)
}
fn conflict() -> ServiceError {
    ServiceError::new(
        "PROJECT_EXISTS",
        "target exists without a matching create receipt",
    )
}
fn create_existing(
    path: &Path,
    key: Option<&str>,
    payload: &Value,
) -> Result<ProjectInfo, ServiceError> {
    // An existing arbitrary file is also a publication conflict, never adopted.
    let record = match key {
        Some(key) => ProjectStore::read_idempotency_record(path, key).map_err(|_| conflict())?,
        None => None,
    };
    match record {
        Some(r) => receipt_result(r, payload),
        None => Err(conflict()),
    }
}
pub(crate) fn create_plan(r: CreatePlanRequest) -> Result<ProjectChangePlan, ServiceError> {
    let path = target(&r.project)?;
    if exists(&path)? {
        return Err(conflict());
    }
    build(path, r.document, None)
}
pub(crate) fn import_plan(r: ImportPlanRequest) -> Result<ProjectChangePlan, ServiceError> {
    let path = target(&r.project)?;
    let previous = read_project_snapshot(&path)?;
    revision(parse_revision(&r.base_revision)?, previous.revision)?;
    build(path, r.document, Some(&previous))
}
fn mode(store: &ProjectStore) -> ProjectOpenMode {
    if store.safe_mode() {
        ProjectOpenMode::Safe
    } else {
        ProjectOpenMode::Normal
    }
}
fn receipt(
    key: Option<String>,
    payload: Value,
    result: &ProjectInfo,
) -> Result<Option<ServiceReceipt>, ServiceError> {
    key.map(|key| {
        Ok(ServiceReceipt {
            key,
            payload,
            result: serde_json::to_value(result)?,
        })
    })
    .transpose()
}
pub(crate) fn create(r: CreateRequest) -> Result<ProjectInfo, ServiceError> {
    create_before_publish(r, || {})
}
fn create_before_publish(
    r: CreateRequest,
    before_publish: impl FnOnce(),
) -> Result<ProjectInfo, ServiceError> {
    key(r.idempotency_key.as_deref())?;
    let path = target(&r.project)?;
    let payload = json!({"operation":"project.create", "project":path, "document":r.document, "plan_hash":r.plan_hash});
    if exists(&path)? {
        return create_existing(&path, r.idempotency_key.as_deref(), &payload);
    }
    let plan = build(path.clone(), r.document.clone(), None)?;
    check_hash(r.plan_hash.as_deref(), &plan)?;
    let staging = tempfile::Builder::new()
        .prefix(".kronello-create-")
        .suffix(".kronello")
        .tempfile_in(path.parent().unwrap())?;
    let mut store = ProjectStore::open(staging.path(), Default::default())?;
    let result = snapshot_info(
        Snapshot {
            revision: 1,
            document: r.document.clone(),
        },
        mode(&store),
    )?;
    store.import_json_with_receipt_checked::<ServiceError>(
        0,
        Uuid::new_v4(),
        &serde_json::to_string(&r.document)?,
        receipt(r.idempotency_key.clone(), payload.clone(), &result)?,
        |_| Ok(()),
    )?;
    store.close()?;
    staging.as_file().sync_all()?;
    before_publish();
    match staging.persist_noclobber(&path) {
        Ok(_) => Ok(result),
        Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
            // Dropping PersistError removes its unpublished staging file.
            drop(e);
            create_existing(&path, r.idempotency_key.as_deref(), &payload)
        }
        Err(e) => Err(ServiceError::new("IO_ERROR", e.error.to_string())),
    }
}
pub(crate) fn import(r: ImportRequest) -> Result<ProjectInfo, ServiceError> {
    key(r.idempotency_key.as_deref())?;
    let path = target(&r.project)?;
    let base = parse_revision(&r.base_revision)?;
    let payload = json!({"operation":"project.import", "project":path, "base_revision":base.to_string(), "document":r.document, "plan_hash":r.plan_hash});
    import_with_payload(r, payload)
}
pub(crate) fn import_with_payload(
    r: ImportRequest,
    payload: Value,
) -> Result<ProjectInfo, ServiceError> {
    key(r.idempotency_key.as_deref())?;
    let path = target(&r.project)?;
    let base = parse_revision(&r.base_revision)?;
    if path.is_file()
        && let Some(result) = retry(&path, r.idempotency_key.as_deref(), &payload)?
    {
        return Ok(result);
    }
    let mut store = open_existing(&path)?;
    let result = (|| {
        let previous = store.snapshot()?;
        if base != previous.revision {
            if let Some(result) = retry(&path, r.idempotency_key.as_deref(), &payload)? {
                return Ok(result);
            }
            revision(base, previous.revision)?;
        }
        let result = snapshot_info(
            Snapshot {
                revision: base
                    .checked_add(1)
                    .ok_or_else(|| ServiceError::invalid("revision overflow"))?,
                document: r.document.clone(),
            },
            mode(&store),
        )?;
        store.import_json_with_receipt_checked::<ServiceError>(
            base,
            Uuid::new_v4(),
            &serde_json::to_string(&r.document)?,
            receipt(r.idempotency_key.clone(), payload.clone(), &result)?,
            |previous| {
                let plan = build(path.clone(), r.document.clone(), Some(previous))?;
                check_hash(r.plan_hash.as_deref(), &plan)
            },
        )?;
        if let Some(key) = &r.idempotency_key {
            return receipt_result(
                store.idempotency_record(key)?.ok_or_else(|| {
                    ServiceError::new("STORAGE_ERROR", "committed receipt missing")
                })?,
                &payload,
            );
        }
        Ok(result)
    })();
    store.close()?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_publication_cleans_fully_initialized_staging_and_preserves_competitor() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("race.kronello");
        let document = Project::default();
        let error = create_before_publish(
            CreateRequest {
                project: path.clone(),
                document: document.clone(),
                plan_hash: None,
                idempotency_key: Some("create".into()),
            },
            || {
                assert!(!path.exists());
                let staging = std::fs::read_dir(dir.path())
                    .unwrap()
                    .map(|e| e.unwrap().path())
                    .find(|p| {
                        p.file_name()
                            .unwrap()
                            .to_string_lossy()
                            .starts_with(".kronello-create-")
                    })
                    .unwrap();
                let snapshot = ProjectStore::read_snapshot(&staging).unwrap();
                assert_eq!(snapshot.revision, 1);
                assert_eq!(snapshot.document, document);
                let receipt = ProjectStore::read_idempotency_record(&staging, "create")
                    .unwrap()
                    .unwrap();
                assert_eq!(receipt.service_result.unwrap()["revision"], "1");
                ProjectStore::open(&staging, Default::default())
                    .unwrap()
                    .close()
                    .unwrap();
                std::fs::write(&path, b"competitor").unwrap();
            },
        )
        .unwrap_err();
        assert_eq!(error.code, "PROJECT_EXISTS");
        assert_eq!(std::fs::read(&path).unwrap(), b"competitor");
        assert!(std::fs::read_dir(dir.path()).unwrap().all(|e| {
            !e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".kronello-create-")
        }));
    }
}
