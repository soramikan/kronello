use std::{collections::BTreeSet, fs::File, path::Path, time::Duration};

use kronello_model::{Project, ProjectError, PropertyId};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::{DetectedLocation, LocationDetector, OpenMode, OpenOptions, SystemLocationDetector};

pub type Revision = u64;
pub const HISTORY_WARNING_BYTES: u64 = 256 * 1024 * 1024;
const INTERNAL_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("revision conflict: expected {base}, current {current}")]
    RevisionConflict { base: Revision, current: Revision },
    #[error("project is locked")]
    ProjectLocked,
    #[error("migration failed: {0}")]
    MigrationFailed(String),
    #[error("unsupported schema version: {0}")]
    UnsupportedSchemaVersion(u32),
    #[error("unsupported meaning or opaque content")]
    UnsupportedFeature,
    #[error("invalid mutation: {0}")]
    InvalidMutation(String),
    #[error("snapshot not found: {0}")]
    SnapshotNotFound(Revision),
    #[error("idempotency key already recorded")]
    IdempotencyKeyExists,
    #[error("invalid location: {0}")]
    InvalidLocation(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
impl StoreError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::RevisionConflict { .. } => "REVISION_CONFLICT",
            Self::ProjectLocked => "PROJECT_LOCKED",
            Self::MigrationFailed(_) => "MIGRATION_FAILED",
            Self::UnsupportedSchemaVersion(_) => "UNSUPPORTED_SCHEMA_VERSION",
            Self::UnsupportedFeature => "UNSUPPORTED_FEATURE",
            Self::InvalidMutation(_) => "INVALID_MUTATION",
            Self::SnapshotNotFound(_) => "SNAPSHOT_NOT_FOUND",
            Self::IdempotencyKeyExists => "IDEMPOTENCY_KEY_EXISTS",
            Self::InvalidLocation(_) => "INVALID_LOCATION",
            Self::Io(_) => "IO_ERROR",
            Self::Sqlite(_) => "STORAGE_ERROR",
            Self::Json(_) => "INVALID_DOCUMENT",
        }
    }
}
impl From<ProjectError> for StoreError {
    fn from(value: ProjectError) -> Self {
        match value {
            ProjectError::UnsupportedSchemaVersion(version) => {
                Self::UnsupportedSchemaVersion(version)
            }
            ProjectError::UnsupportedMeaning => Self::UnsupportedFeature,
            ProjectError::InvalidDocument(reason) => Self::InvalidMutation(reason),
        }
    }
}

/// Paths use object-member segments, avoiding ambiguous JSON pointer escaping.
/// Arrays are replaced as a unit. A service can use stable IDs before preparing
/// these storage patches; array positions are never object identities.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Mutation {
    Set { path: Vec<String>, value: Value },
    Remove { path: Vec<String> },
}
impl Mutation {
    fn apply(&self, document: &mut Value) -> Result<Self, StoreError> {
        let path = match self {
            Self::Set { path, .. } | Self::Remove { path } => path,
        };
        if path.is_empty() {
            if let Self::Set { value, .. } = self {
                return Ok(Self::Set {
                    path: vec![],
                    value: std::mem::replace(document, value.clone()),
                });
            }
            return Err(StoreError::InvalidMutation(
                "cannot remove document root".into(),
            ));
        }
        let mut parent = document;
        for part in &path[..path.len() - 1] {
            parent = parent
                .as_object_mut()
                .and_then(|object| object.get_mut(part))
                .ok_or_else(|| StoreError::InvalidMutation("missing object parent".into()))?;
        }
        let object = parent
            .as_object_mut()
            .ok_or_else(|| StoreError::InvalidMutation("parent is not an object".into()))?;
        let key = path.last().unwrap();
        let previous = match self {
            Self::Set { value, .. } => object.insert(key.clone(), value.clone()),
            Self::Remove { .. } => Some(
                object
                    .remove(key)
                    .ok_or_else(|| StoreError::InvalidMutation("missing removal target".into()))?,
            ),
        };
        Ok(match previous {
            Some(value) => Self::Set {
                path: path.clone(),
                value,
            },
            None => Self::Remove { path: path.clone() },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChangedKey {
    Value {
        object_id: Uuid,
        property_id: PropertyId,
    },
    Structure {
        object_id: Uuid,
        parent_container_id: Uuid,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyRequest {
    pub base_revision: Revision,
    pub session_id: Uuid,
    pub mutations: Vec<Mutation>,
    pub changed_keys: BTreeSet<ChangedKey>,
    pub idempotency_key: Option<String>,
    pub undo_of: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub revision: Revision,
    pub document: Project,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: Uuid,
    pub revision: Revision,
    pub session_id: Uuid,
    pub mutations: Vec<Mutation>,
    pub inverse: Vec<Mutation>,
    pub changed_keys: BTreeSet<ChangedKey>,
    pub idempotency_key: Option<String>,
    pub undo_of: Option<Uuid>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistorySize {
    pub bytes: u64,
    pub warning: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdempotencyRecord {
    pub key: String,
    pub payload: ApplyRequest,
    pub result: Event,
}

pub struct ProjectStore {
    // Connection must close before the lifetime lock is released.
    connection: Connection,
    _lifetime_lock: File,
    safe_mode: bool,
    location: DetectedLocation,
}

const SCHEMA: &str = "
CREATE TABLE project (singleton INTEGER PRIMARY KEY CHECK(singleton=1), revision INTEGER NOT NULL CHECK(revision>=0), document TEXT NOT NULL);
CREATE TABLE events (revision INTEGER PRIMARY KEY, event_id TEXT NOT NULL UNIQUE, session_id TEXT NOT NULL, mutations TEXT NOT NULL, inverse TEXT NOT NULL, changed_keys TEXT NOT NULL, idempotency_key TEXT, undo_of TEXT);
CREATE TABLE snapshots (revision INTEGER PRIMARY KEY, document TEXT NOT NULL);
CREATE TABLE idempotency (key TEXT PRIMARY KEY, payload TEXT NOT NULL, event_id TEXT NOT NULL, revision INTEGER NOT NULL, result TEXT NOT NULL);
";

impl ProjectStore {
    pub fn open(path: impl AsRef<Path>, options: OpenOptions) -> Result<Self, StoreError> {
        Self::open_with_detector(path, options, &SystemLocationDetector)
    }
    pub fn open_with_detector(
        path: impl AsRef<Path>,
        options: OpenOptions,
        detector: &dyn LocationDetector,
    ) -> Result<Self, StoreError> {
        Self::open_impl(path.as_ref(), options, detector).map_err(|error| match error {
            StoreError::Sqlite(error) => map_open_error(error),
            other => other,
        })
    }
    fn open_impl(
        path: &Path,
        options: OpenOptions,
        detector: &dyn LocationDetector,
    ) -> Result<Self, StoreError> {
        if path
            .extension()
            .is_none_or(|extension| extension != "kronello")
        {
            return Err(StoreError::InvalidLocation(
                "project must use .kronello extension".into(),
            ));
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let canonical = if path.exists() {
            path.canonicalize()?
        } else {
            parent.canonicalize()?.join(
                path.file_name()
                    .ok_or_else(|| StoreError::InvalidLocation("missing filename".into()))?,
            )
        };
        let location = detector.detect(&canonical)?;
        let safe_mode = match options.mode {
            OpenMode::Auto => location != DetectedLocation::Local,
            OpenMode::ForceNormal => false,
            OpenMode::ForceSafe => true,
        };
        let project_file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&canonical)?;
        // Darwin whole-file locks conflict with SQLite locks on the same inode.
        // Coordinate modes on a separate OS-local file, never inside the project.
        let lock_directory = std::env::temp_dir().join("kronello-project-locks");
        std::fs::create_dir_all(&lock_directory)?;
        let identity = lock_identity(&project_file, &canonical)?;
        drop(project_file);
        let lifetime_lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_directory.join(&identity))?;
        let lock = if safe_mode {
            lifetime_lock.try_lock()
        } else {
            lifetime_lock.try_lock_shared()
        };
        lock.map_err(|error| match error {
            std::fs::TryLockError::WouldBlock => StoreError::ProjectLocked,
            std::fs::TryLockError::Error(error) => StoreError::Io(error),
        })?;
        // Serialize bootstrap/journal-mode changes too: SQLite may reject a
        // competing WAL switch immediately without invoking its busy handler.
        let bootstrap_lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_directory.join(format!("{identity}.open")))?;
        bootstrap_lock.lock()?;
        let mut connection = Connection::open(&canonical)?;
        if safe_mode {
            connection.pragma_update(None, "locking_mode", "EXCLUSIVE")?;
        }
        connection.busy_timeout(Duration::from_secs(5))?;
        let version: u32 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > INTERNAL_SCHEMA_VERSION {
            return Err(StoreError::UnsupportedSchemaVersion(version));
        }
        // Refuse unrecognized nonempty databases without adopting or rewriting them.
        if version == 0 {
            let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let locked_version: u32 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
            if locked_version == 0 {
                let count: u32 = tx.query_row(
                    "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
                    [],
                    |r| r.get(0),
                )?;
                if count != 0 {
                    return Err(StoreError::MigrationFailed(
                        "unrecognized unversioned database".into(),
                    ));
                }
                tx.execute_batch(SCHEMA)?;
                let document = serde_json::to_string(&Project::default())?;
                tx.execute("INSERT INTO project VALUES(1,0,?1)", [&document])?;
                tx.execute("INSERT INTO snapshots VALUES(0,?1)", [&document])?;
                tx.pragma_update(None, "user_version", INTERNAL_SCHEMA_VERSION)?;
            } else if locked_version != INTERNAL_SCHEMA_VERSION {
                return Err(StoreError::UnsupportedSchemaVersion(locked_version));
            }
            tx.commit()?;
        }
        let store = Self {
            connection,
            _lifetime_lock: lifetime_lock,
            safe_mode,
            location,
        };
        store.snapshot()?.document.validate_storage()?;
        let mode = if safe_mode { "DELETE" } else { "WAL" };
        let current: String = store
            .connection
            .pragma_query_value(None, "journal_mode", |r| r.get(0))?;
        let actual: String = if current.eq_ignore_ascii_case(mode) {
            current
        } else {
            store
                .connection
                .query_row(&format!("PRAGMA journal_mode={mode}"), [], |r| r.get(0))?
        };
        if !actual.eq_ignore_ascii_case(mode) {
            return Err(StoreError::ProjectLocked);
        }
        store
            .connection
            .pragma_update(None, "synchronous", "FULL")?;
        if safe_mode {
            store.connection.execute_batch("BEGIN EXCLUSIVE; COMMIT;")?;
        }
        Ok(store)
    }
    pub fn safe_mode(&self) -> bool {
        self.safe_mode
    }
    pub fn detected_location(&self) -> &DetectedLocation {
        &self.location
    }
    pub fn journal_mode(&self) -> Result<String, StoreError> {
        Ok(self
            .connection
            .pragma_query_value(None, "journal_mode", |r| r.get(0))?)
    }
    pub fn snapshot(&self) -> Result<Snapshot, StoreError> {
        read_snapshot(&self.connection)
    }
    pub fn snapshot_at(&self, revision: Revision) -> Result<Snapshot, StoreError> {
        let document: String = self
            .connection
            .query_row(
                "SELECT document FROM snapshots WHERE revision=?1",
                [sql_revision(revision)?],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(StoreError::SnapshotNotFound(revision))?;
        let document: Project = serde_json::from_str(&document)?;
        document.validate_storage()?;
        Ok(Snapshot { revision, document })
    }
    pub fn export_json(&self) -> Result<String, StoreError> {
        Ok(serde_json::to_string_pretty(&self.snapshot()?.document)?)
    }
    /// Imports the full public snapshot as a new event, never as another authority.
    /// Import is explicitly allowed to preserve unsupported meaning/opaque data.
    pub fn import_json(
        &mut self,
        base_revision: Revision,
        session_id: Uuid,
        input: &str,
    ) -> Result<Event, StoreError> {
        let project: Project = serde_json::from_str(input)?;
        project.validate_storage()?;
        let previous = self.snapshot()?.document;
        let mut changed_keys = BTreeSet::new();
        for value in [
            serde_json::to_value(&previous)?,
            serde_json::to_value(&project)?,
        ] {
            collect_object_keys(&value, previous.id, &mut changed_keys);
        }
        let request = ApplyRequest {
            base_revision,
            session_id,
            mutations: vec![Mutation::Set {
                path: vec![],
                value: serde_json::to_value(project)?,
            }],
            changed_keys,
            idempotency_key: None,
            undo_of: None,
        };
        self.apply_inner(request, true)
    }
    pub fn restore_snapshot(
        &mut self,
        base_revision: Revision,
        session_id: Uuid,
        revision: Revision,
    ) -> Result<Event, StoreError> {
        let document = self.snapshot_at(revision)?.document;
        self.import_json(
            base_revision,
            session_id,
            &serde_json::to_string(&document)?,
        )
    }
    pub fn apply(&mut self, request: ApplyRequest) -> Result<Event, StoreError> {
        self.apply_inner(request, false)
    }
    fn apply_inner(&mut self, request: ApplyRequest, import: bool) -> Result<Event, StoreError> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let snapshot = read_snapshot(&tx)?;
        if request.base_revision != snapshot.revision {
            return Err(StoreError::RevisionConflict {
                base: request.base_revision,
                current: snapshot.revision,
            });
        }
        if !import {
            snapshot.document.ensure_editable()?;
        }
        if let Some(key) = &request.idempotency_key
            && tx
                .query_row("SELECT 1 FROM idempotency WHERE key=?1", [key], |_| Ok(()))
                .optional()?
                .is_some()
        {
            return Err(StoreError::IdempotencyKeyExists);
        }
        let mut value = serde_json::to_value(&snapshot.document)?;
        let mut inverse = Vec::new();
        for mutation in &request.mutations {
            inverse.push(mutation.apply(&mut value)?);
        }
        inverse.reverse();
        let document: Project = serde_json::from_str(&value.to_string())?;
        document.validate_storage()?;
        if !import {
            document.ensure_editable()?;
        }
        let revision = snapshot
            .revision
            .checked_add(1)
            .ok_or_else(|| StoreError::InvalidMutation("revision overflow".into()))?;
        let event = Event {
            id: Uuid::new_v4(),
            revision,
            session_id: request.session_id,
            mutations: request.mutations.clone(),
            inverse,
            changed_keys: request.changed_keys.clone(),
            idempotency_key: request.idempotency_key.clone(),
            undo_of: request.undo_of,
        };
        let document = serde_json::to_string(&document)?;
        tx.execute(
            "UPDATE project SET revision=?1, document=?2 WHERE singleton=1",
            params![sql_revision(revision)?, document],
        )?;
        tx.execute(
            "INSERT INTO events VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                sql_revision(revision)?,
                event.id.to_string(),
                event.session_id.to_string(),
                serde_json::to_string(&event.mutations)?,
                serde_json::to_string(&event.inverse)?,
                serde_json::to_string(&event.changed_keys)?,
                event.idempotency_key,
                event.undo_of.map(|id| id.to_string())
            ],
        )?;
        tx.execute(
            "INSERT INTO snapshots VALUES(?1,?2)",
            params![sql_revision(revision)?, document],
        )?;
        if let Some(key) = &request.idempotency_key {
            tx.execute(
                "INSERT INTO idempotency VALUES(?1,?2,?3,?4,?5)",
                params![
                    key,
                    serde_json::to_string(&request)?,
                    event.id.to_string(),
                    sql_revision(revision)?,
                    serde_json::to_string(&event)?
                ],
            )?;
        }
        tx.commit()?;
        Ok(event)
    }
    /// Receipt lookup supplies material for SERVICE-001 payload/result policy.
    pub fn idempotency_record(&self, key: &str) -> Result<Option<IdempotencyRecord>, StoreError> {
        let record: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT payload,result FROM idempotency WHERE key=?1",
                [key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        record
            .map(|(payload, result)| {
                Ok(IdempotencyRecord {
                    key: key.to_owned(),
                    payload: serde_json::from_str(&payload)?,
                    result: serde_json::from_str(&result)?,
                })
            })
            .transpose()
    }
    pub fn events_since(&self, revision: Revision) -> Result<Vec<Event>, StoreError> {
        let mut statement = self.connection.prepare("SELECT revision,event_id,session_id,mutations,inverse,changed_keys,idempotency_key,undo_of FROM events WHERE revision>?1 ORDER BY revision")?;
        let mut rows = statement.query([sql_revision(revision)?])?;
        let mut result = Vec::new();
        while let Some(row) = rows.next()? {
            let id: String = row.get(1)?;
            let session: String = row.get(2)?;
            let undo: Option<String> = row.get(7)?;
            let parse_id = |value: &str| {
                Uuid::parse_str(value).map_err(|e| StoreError::InvalidMutation(e.to_string()))
            };
            result.push(Event {
                revision: read_u64(row, 0)?,
                id: parse_id(&id)?,
                session_id: parse_id(&session)?,
                mutations: serde_json::from_str(&row.get::<_, String>(3)?)?,
                inverse: serde_json::from_str(&row.get::<_, String>(4)?)?,
                changed_keys: serde_json::from_str(&row.get::<_, String>(5)?)?,
                idempotency_key: row.get(6)?,
                undo_of: undo.as_deref().map(parse_id).transpose()?,
            });
        }
        Ok(result)
    }
    /// Prunes strictly before the supplied revision; its full snapshot and event
    /// are retained. No replay of historical command semantics is necessary.
    pub fn compact(&mut self, revision: Revision) -> Result<(), StoreError> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists = tx
            .query_row(
                "SELECT 1 FROM snapshots WHERE revision=?1",
                [sql_revision(revision)?],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            return Err(StoreError::SnapshotNotFound(revision));
        }
        tx.execute(
            "DELETE FROM events WHERE revision<?1",
            [sql_revision(revision)?],
        )?;
        tx.execute(
            "DELETE FROM snapshots WHERE revision<?1",
            [sql_revision(revision)?],
        )?;
        // Idempotency receipts outlive history pruning, to avoid reapplying keys.
        tx.commit()?;
        Ok(())
    }
    pub fn history_size(&self) -> Result<HistorySize, StoreError> {
        let events: u64 = self.connection.query_row("SELECT coalesce(sum(length(CAST(mutations AS BLOB))+length(CAST(inverse AS BLOB))+length(CAST(changed_keys AS BLOB))),0) FROM events", [], |r| read_u64(r, 0))?;
        let snapshots: u64 = self.connection.query_row("SELECT coalesce(sum(length(CAST(document AS BLOB))),0) FROM snapshots WHERE revision != (SELECT revision FROM project)", [], |r| read_u64(r, 0))?;
        let bytes = events.saturating_add(snapshots);
        Ok(HistorySize::from_bytes(bytes))
    }
    /// Trusted internal migration hook, not exposed by Command/Query payloads.
    /// DDL and data changes roll back together on failure or invalid document.
    pub fn migrate_schema(
        &mut self,
        target: u32,
        migration: impl FnOnce(&Transaction<'_>) -> Result<(), StoreError>,
    ) -> Result<(), StoreError> {
        let result = (|| {
            let tx = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            migration(&tx)?;
            read_snapshot(&tx)?.document.validate_storage()?;
            tx.pragma_update(None, "user_version", target)?;
            tx.commit()?;
            Ok(())
        })();
        result.map_err(|error: StoreError| StoreError::MigrationFailed(error.to_string()))
    }
    /// Explicit close reports errors; SQLite performs the final WAL checkpoint
    /// and removes sidecars when the last connection closes. Drop does likewise.
    pub fn close(self) -> Result<(), StoreError> {
        self.connection
            .close()
            .map_err(|(_, error)| StoreError::Sqlite(error))?;
        Ok(())
    }
    /// Deterministic JSON object ordering (serde_json without preserve_order),
    /// UTF-8 compact serialization, SHA-256; includes all opaque content/versions.
    pub fn content_hash(&self) -> Result<String, StoreError> {
        let value = serde_json::to_value(self.snapshot()?.document)?;
        Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(&value)?)))
    }
}

fn sql_revision(revision: Revision) -> Result<i64, StoreError> {
    i64::try_from(revision)
        .map_err(|_| StoreError::InvalidMutation("revision exceeds SQLite integer range".into()))
}
fn read_snapshot(connection: &Connection) -> Result<Snapshot, StoreError> {
    let (revision, document): (Revision, String) = connection.query_row(
        "SELECT revision,document FROM project WHERE singleton=1",
        [],
        |r| Ok((read_u64(r, 0)?, r.get(1)?)),
    )?;
    let document: Project = serde_json::from_str(&document)?;
    document.validate_storage()?;
    Ok(Snapshot { revision, document })
}

fn read_u64(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    value.try_into().map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}

fn collect_object_keys(value: &Value, parent: Uuid, keys: &mut BTreeSet<ChangedKey>) {
    match value {
        Value::Object(object) => {
            let id = object
                .get("id")
                .and_then(Value::as_str)
                .and_then(|v| Uuid::parse_str(v).ok());
            if let Some(id) = id {
                keys.insert(ChangedKey::Structure {
                    object_id: id,
                    parent_container_id: parent,
                });
            }
            let current = id.unwrap_or(parent);
            for (name, value) in object {
                if name == "properties"
                    && let Value::Array(properties) = value
                {
                    for property in properties {
                        if let Some(id) = property
                            .get("id")
                            .and_then(Value::as_str)
                            .and_then(|v| Uuid::parse_str(v).ok())
                        {
                            keys.insert(ChangedKey::Value {
                                object_id: current,
                                property_id: PropertyId::from_uuid(id),
                            });
                        }
                    }
                }
                collect_object_keys(value, current, keys);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_object_keys(value, parent, keys);
            }
        }
        _ => (),
    }
}

fn lock_identity(file: &File, path: &Path) -> Result<String, StoreError> {
    #[cfg(unix)]
    let identity = {
        use std::os::unix::fs::MetadataExt;
        let metadata = file.metadata()?;
        let _ = path;
        format!("{}:{}", metadata.dev(), metadata.ino())
    };
    #[cfg(not(unix))]
    let identity = {
        let _ = file;
        path.to_string_lossy().into_owned()
    };
    Ok(format!("{:x}.lock", Sha256::digest(identity.as_bytes())))
}

fn map_open_error(error: rusqlite::Error) -> StoreError {
    match &error {
        rusqlite::Error::SqliteFailure(code, _)
            if matches!(
                code.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            ) =>
        {
            StoreError::ProjectLocked
        }
        _ => StoreError::Sqlite(error),
    }
}

impl HistorySize {
    pub const fn from_bytes(bytes: u64) -> Self {
        Self {
            bytes,
            warning: bytes >= HISTORY_WARNING_BYTES,
        }
    }
}
