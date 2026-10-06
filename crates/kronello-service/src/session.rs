//! A native editing session retains the same store used by shared requests.
use crate::ServiceError;
use kronello_store::{OpenOptions, ProjectStore, StoreError};
use std::{
    cell::RefCell,
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
    rc::Rc,
};

struct Retained {
    path: PathBuf,
    safe: bool,
    store: RefCell<Option<ProjectStore>>,
}
thread_local! {
    static ACTIVE: RefCell<Option<Rc<Retained>>> = const { RefCell::new(None) };
}

/// Thread-confined native project lifetime. Requests still carry explicit paths.
/// Safe sessions keep exclusivity; normal sessions retain transient opens.
pub struct ProjectSession(Rc<Retained>);
impl ProjectSession {
    pub fn open(path: &Path) -> Result<Self, ServiceError> {
        Self::open_with_options(path, OpenOptions::default())
    }
    pub fn open_with_options(path: &Path, options: OpenOptions) -> Result<Self, ServiceError> {
        if !path.is_file() {
            return Err(ServiceError::new(
                "PROJECT_NOT_FOUND",
                "project file does not exist",
            ));
        }
        let path = path.canonicalize().map_err(StoreError::from)?;
        let store = ProjectStore::open(&path, options)?;
        let safe = store.safe_mode();
        let store = if safe {
            Some(store)
        } else {
            store.close()?;
            None
        };
        Ok(Self(Rc::new(Retained {
            path,
            safe,
            store: RefCell::new(store),
        })))
    }
    /// Execute the existing service on the session's owning thread. The guard
    /// restores previous routing on normal return and unwind alike.
    pub fn scope<T>(&self, operation: impl FnOnce() -> T) -> T {
        struct Restore(Option<Rc<Retained>>);
        impl Drop for Restore {
            fn drop(&mut self) {
                ACTIVE.with(|slot| *slot.borrow_mut() = self.0.take());
            }
        }
        let previous = ACTIVE.with(|slot| slot.replace(Some(self.0.clone())));
        let _restore = Restore(previous);
        operation()
    }
}

pub(crate) struct StoreLease {
    store: Option<ProjectStore>,
    retained: Option<Rc<Retained>>,
}
impl Deref for StoreLease {
    type Target = ProjectStore;
    fn deref(&self) -> &Self::Target {
        self.store.as_ref().expect("live store lease")
    }
}
impl DerefMut for StoreLease {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.store.as_mut().expect("live store lease")
    }
}
impl StoreLease {
    pub fn close(mut self) -> Result<(), StoreError> {
        if self.retained.is_none() {
            self.store.take().expect("live store lease").close()?;
        }
        Ok(())
    }
}
impl Drop for StoreLease {
    fn drop(&mut self) {
        if let Some(retained) = &self.retained {
            *retained.store.borrow_mut() = self.store.take();
        }
    }
}
pub(crate) fn open_existing(path: &Path) -> Result<StoreLease, ServiceError> {
    if !path.is_file() {
        return Err(ServiceError::new(
            "PROJECT_NOT_FOUND",
            "project file does not exist",
        ));
    }
    let canonical = path.canonicalize().map_err(StoreError::from)?;
    let retained = ACTIVE.with(|slot| {
        slot.borrow()
            .as_ref()
            .filter(|s| s.safe && s.path == canonical)
            .cloned()
    });
    let store =
        if let Some(session) = &retained {
            session.store.borrow_mut().take().ok_or_else(|| {
                ServiceError::new("PROJECT_LOCKED", "session store is already in use")
            })?
        } else {
            ProjectStore::open(path, OpenOptions::default())?
        };
    Ok(StoreLease {
        store: Some(store),
        retained,
    })
}

/// Preserve snapshot-only opening for stateless callers while a native safe
/// session reads its own retained connection rather than reopening its DB.
pub(crate) fn read_snapshot(path: &Path) -> Result<kronello_store::Snapshot, ServiceError> {
    let canonical = path.canonicalize().map_err(StoreError::from)?;
    let retained = ACTIVE.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|s| s.safe && s.path == canonical)
    });
    if retained {
        let store = open_existing(path)?;
        let snapshot = store.snapshot()?;
        store.close()?;
        Ok(snapshot)
    } else {
        Ok(ProjectStore::read_snapshot(path)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BackendSelection, Service};
    use kronello_store::OpenMode;
    use serde_json::json;
    #[test]
    fn safe_session_returns_store_after_unwind_and_preserves_exclusive_lifetime() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.kronello");
        let document: serde_json::Value =
            serde_json::from_str(include_str!("../../../examples/ffi-preview.project.json"))
                .unwrap();
        let service = Service::new(BackendSelection::CpuReference);
        assert_eq!(
            serde_json::to_value(
                service.execute_json(
                    &json!({"operation":"project.create","project":path,"document":document})
                        .to_string()
                )
            )
            .unwrap()["status"],
            "success"
        );
        let session = ProjectSession::open_with_options(
            &path,
            OpenOptions {
                mode: OpenMode::ForceSafe,
            },
        )
        .unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            session.scope(|| {
                let _lease = open_existing(&path).unwrap();
                panic!("injected request unwind");
            })
        }));
        assert!(result.is_err());
        assert!(matches!(
            ProjectStore::open(&path, OpenOptions::default()),
            Err(StoreError::ProjectLocked)
        ));
        session.scope(|| open_existing(&path).unwrap().close().unwrap());
        assert!(matches!(
            ProjectStore::open(&path, OpenOptions::default()),
            Err(StoreError::ProjectLocked)
        ));
        drop(session);
        ProjectStore::open(&path, OpenOptions::default())
            .unwrap()
            .close()
            .unwrap();
    }
}
