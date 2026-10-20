//! Single-instance lock: a file in the app data dir, locked for the whole
//! process lifetime. The OS releases it when the process ends, even on a
//! crash, so a stale file never blocks the next start.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::Path;

pub const LOCK_FILE: &str = "private-context.lock";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockError {
    /// Another process holds the lock.
    AlreadyRunning,
    Io(String),
}

/// Held for as long as this process may touch the database.
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
}

impl InstanceLock {
    pub fn acquire(dir: &Path) -> Result<Self, LockError> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join(LOCK_FILE))
            .map_err(|err| LockError::Io(err.to_string()))?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => Err(LockError::AlreadyRunning),
            Err(TryLockError::Error(err)) => Err(LockError::Io(err.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_lock_holder_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let _first = InstanceLock::acquire(dir.path()).unwrap();
        assert_eq!(
            InstanceLock::acquire(dir.path()).unwrap_err(),
            LockError::AlreadyRunning
        );
    }

    #[test]
    fn lock_released_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        drop(InstanceLock::acquire(dir.path()).unwrap());
        assert!(InstanceLock::acquire(dir.path()).is_ok());
    }
}
