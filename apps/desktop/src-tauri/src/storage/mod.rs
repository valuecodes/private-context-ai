//! Encrypted chat storage: one SQLCipher database whose key lives in the OS
//! keychain and never reaches the webview.

pub mod audit;
pub mod compare;
pub mod conversations;
pub mod db;
pub mod export;
pub mod key;
pub mod lock;
pub mod messages;
mod migrations;
pub mod settings;

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use rusqlite::Connection;
use serde::Serialize;

use self::key::{KeySource, is_valid_key, new_key};
use self::lock::{InstanceLock, LockError};
use crate::error::AppError;

pub const DB_FILE: &str = "private-context.db";

/// What `storage_status` returns; mirrored by `StorageStatus` in
/// `src/lib/types.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum StorageStatus {
    Ok,
    /// No usable OS keychain, so there is nowhere safe to keep the key.
    KeychainUnavailable {
        detail: String,
    },
    /// The database exists but its key is gone from the keychain. A new key
    /// is never generated, since that would orphan the existing data.
    KeyMissing {
        db_path: String,
    },
    /// The keychain holds a key that does not open the database.
    WrongKey {
        db_path: String,
    },
    Io {
        detail: String,
    },
    /// Another instance holds the lock; this one never touches the database.
    AlreadyRunning,
}

impl StorageStatus {
    /// For log fields: the state name only, never a path.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::KeychainUnavailable { .. } => "keychainUnavailable",
            Self::KeyMissing { .. } => "keyMissing",
            Self::WrongKey { .. } => "wrongKey",
            Self::Io { .. } => "io",
            Self::AlreadyRunning => "alreadyRunning",
        }
    }
}

/// The open database. One connection behind a mutex, so a pool can replace
/// it later without changing callers.
pub struct Storage {
    conn: Mutex<Connection>,
}

impl Storage {
    fn conn(&self) -> MutexGuard<'_, Connection> {
        // Every critical section is one statement or one transaction, which
        // rolls back when dropped, so a poisoned lock is still consistent.
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Finds or creates the key, then opens the database in `dir`.
pub fn open_storage(dir: &Path, keys: &dyn KeySource) -> Result<Storage, StorageStatus> {
    let path = dir.join(DB_FILE);
    let db_path = path.display().to_string();
    let db_exists = path.try_exists().map_err(|err| StorageStatus::Io {
        detail: err.to_string(),
    })?;
    let key = match keys.get() {
        Err(err) => return Err(StorageStatus::KeychainUnavailable { detail: err.0 }),
        Ok(Some(key)) => key,
        Ok(None) if db_exists => return Err(StorageStatus::KeyMissing { db_path }),
        Ok(None) => {
            let key = new_key();
            keys.set(&key)
                .map_err(|err| StorageStatus::KeychainUnavailable { detail: err.0 })?;
            key
        }
    };
    if !is_valid_key(&key) {
        return Err(StorageStatus::WrongKey { db_path });
    }
    let conn = db::open(&path, &key).map_err(|err| match err {
        AppError::WrongKey(_) => StorageStatus::WrongKey { db_path },
        other => StorageStatus::Io {
            detail: other.to_string(),
        },
    })?;
    Ok(Storage {
        conn: Mutex::new(conn),
    })
}

/// Managed by Tauri. Holds the instance lock for the process lifetime.
pub struct StorageState {
    status: StorageStatus,
    storage: Option<Arc<Storage>>,
    _lock: Option<InstanceLock>,
}

impl StorageState {
    /// Creates `dir`, takes the instance lock, then opens the database. Never
    /// fails: a problem becomes the status the UI explains.
    pub fn open(dir: &Path, keys: &dyn KeySource) -> Self {
        let closed = |status| Self {
            status,
            storage: None,
            _lock: None,
        };
        if let Err(err) = std::fs::create_dir_all(dir) {
            return closed(StorageStatus::Io {
                detail: err.to_string(),
            });
        }
        let lock = match InstanceLock::acquire(dir) {
            Ok(lock) => lock,
            Err(LockError::AlreadyRunning) => return closed(StorageStatus::AlreadyRunning),
            Err(LockError::Io(detail)) => return closed(StorageStatus::Io { detail }),
        };
        match open_storage(dir, keys) {
            Ok(storage) => Self {
                status: StorageStatus::Ok,
                storage: Some(Arc::new(storage)),
                _lock: Some(lock),
            },
            Err(status) => Self {
                status,
                storage: None,
                _lock: Some(lock),
            },
        }
    }

    pub fn status(&self) -> StorageStatus {
        self.status.clone()
    }

    pub fn get(&self) -> Result<&Arc<Storage>, AppError> {
        self.storage
            .as_ref()
            .ok_or_else(|| AppError::StorageUnavailable(self.status.kind().to_owned()))
    }
}

#[cfg(test)]
pub mod testing {
    use super::*;
    use crate::storage::key::testing::MemoryKeySource;

    /// A fresh database in a temp dir, plus the dir that keeps it alive.
    pub fn temp_storage() -> (Arc<Storage>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let storage = open_storage(dir.path(), &MemoryKeySource::default())
            .unwrap_or_else(|status| panic!("storage did not open: {status:?}"));
        (Arc::new(storage), dir)
    }

    /// Makes every later write fail, as a full disk would.
    pub fn break_writes(storage: &Storage) {
        storage
            .conn()
            .pragma_update(None, "query_only", "ON")
            .unwrap();
    }

    /// How many rows `table` holds.
    pub fn count_rows(storage: &Storage, table: &str) -> i64 {
        storage
            .conn()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::key::testing::MemoryKeySource;

    #[test]
    fn first_run_generates_and_stores_key() {
        let dir = tempfile::tempdir().unwrap();
        let keys = MemoryKeySource::default();

        assert!(open_storage(dir.path(), &keys).is_ok());
        assert_eq!(keys.sets(), 1);
        assert!(is_valid_key(&keys.key().unwrap()));
        assert!(dir.path().join(DB_FILE).exists());
    }

    #[test]
    fn reopen_uses_stored_key() {
        let dir = tempfile::tempdir().unwrap();
        let keys = MemoryKeySource::default();
        drop(open_storage(dir.path(), &keys).unwrap());

        assert!(open_storage(dir.path(), &keys).is_ok());
        assert_eq!(keys.sets(), 1);
    }

    #[test]
    fn key_missing_with_existing_db_is_key_missing_and_sets_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let keys = MemoryKeySource::default();
        drop(open_storage(dir.path(), &keys).unwrap());
        let path = dir.path().join(DB_FILE);
        let before = std::fs::read(&path).unwrap();
        keys.clear();

        let status = open_storage(dir.path(), &keys).err().unwrap();
        assert_eq!(
            status,
            StorageStatus::KeyMissing {
                db_path: path.display().to_string()
            }
        );
        assert_eq!(keys.sets(), 1);
        assert_eq!(keys.key(), None);
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn existing_key_without_db_creates_db() {
        let dir = tempfile::tempdir().unwrap();
        let keys = MemoryKeySource::with_key(&new_key());

        assert!(open_storage(dir.path(), &keys).is_ok());
        assert_eq!(keys.sets(), 0);
        assert!(dir.path().join(DB_FILE).exists());
    }

    #[test]
    fn keychain_unavailable_status() {
        let dir = tempfile::tempdir().unwrap();

        let status = open_storage(dir.path(), &MemoryKeySource::unavailable())
            .err()
            .unwrap();
        assert!(matches!(status, StorageStatus::KeychainUnavailable { .. }));
        assert!(!dir.path().join(DB_FILE).exists());
    }

    #[test]
    fn malformed_key_is_wrong_key() {
        let dir = tempfile::tempdir().unwrap();
        let keys = MemoryKeySource::with_key("not hex");

        let status = open_storage(dir.path(), &keys).err().unwrap();
        assert!(matches!(status, StorageStatus::WrongKey { .. }));
        assert!(!dir.path().join(DB_FILE).exists());
    }

    #[test]
    fn second_instance_is_already_running() {
        let dir = tempfile::tempdir().unwrap();
        let keys = MemoryKeySource::default();
        let first = StorageState::open(dir.path(), &keys);
        assert_eq!(first.status(), StorageStatus::Ok);

        let second = StorageState::open(dir.path(), &keys);
        assert_eq!(second.status(), StorageStatus::AlreadyRunning);
        assert!(matches!(second.get(), Err(AppError::StorageUnavailable(_))));
    }

    #[test]
    fn status_serializes_with_state_tag() {
        assert_eq!(
            serde_json::to_value(StorageStatus::KeyMissing {
                db_path: "/x/private-context.db".into()
            })
            .unwrap(),
            serde_json::json!({ "state": "keyMissing", "dbPath": "/x/private-context.db" })
        );
        assert_eq!(
            serde_json::to_value(StorageStatus::Ok).unwrap(),
            serde_json::json!({ "state": "ok" })
        );
    }
}
