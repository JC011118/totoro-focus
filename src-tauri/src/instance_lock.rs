//! Process-lifetime exclusive lock on `<data>/state.lock` (advisory; `flock` on
//! Unix via `std::fs::File::try_lock`). Guarantees one Engine per data directory
//! even if two processes start together. Released automatically at exit.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::Path;

pub const LOCK_FILE: &str = "state.lock";

/// Held for the process lifetime (managed Tauri state).
#[derive(Debug)]
pub struct InstanceLock(#[allow(dead_code)] File);

#[derive(Debug)]
pub enum LockError {
    /// Another process holds the lock.
    Held,
    Io(std::io::Error),
}

pub fn acquire(data_dir: &Path) -> Result<InstanceLock, LockError> {
    std::fs::create_dir_all(data_dir).map_err(LockError::Io)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(data_dir.join(LOCK_FILE))
        .map_err(LockError::Io)?;
    match file.try_lock() {
        Ok(()) => Ok(InstanceLock(file)),
        Err(TryLockError::WouldBlock) => Err(LockError::Held),
        Err(TryLockError::Error(e)) => Err(LockError::Io(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_acquire_is_refused_until_the_first_is_dropped() {
        let dir = std::env::temp_dir().join(format!("focus-lock-test-{}", std::process::id()));
        let first = acquire(&dir).expect("first lock");
        assert!(matches!(acquire(&dir), Err(LockError::Held)));
        drop(first);
        let again = acquire(&dir).expect("lock is free again after drop");
        drop(again);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
