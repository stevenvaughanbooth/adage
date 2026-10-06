//! Advisory file locks for single-flight work between processes: `flock` on Unix, `LockFileEx` on
//! Windows, both through `std`'s `File::lock`. A lock is released when its [`Lock`] is dropped, and
//! by the operating system when the holding process exits however it exits, so a crashed holder
//! never leaves a lock behind; the lock file itself stays (its presence means nothing).
//!
//! Used by live learning: `$ADAGE_HOME/learn-queue/<repo>.lock` (one learner per repository) and
//! `$ADAGE_HOME/tracks.lock` (one writer of the tracks directory and index at a time).

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

/// A held exclusive lock; released on drop.
#[derive(Debug)]
pub struct Lock {
    _file: File,
}

/// Released explicitly, not only by closing the file. `flock` belongs to the open file, and a
/// process another thread is starting holds a copy of every descriptor from its fork until its exec
/// closes them: closing ours alone leaves the lock held for that window and turns the next holder
/// away (an ordinary apply refused as busy). Unlocking releases it for every copy at once.
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = self._file.unlock();
    }
}

fn open(path: &Path) -> io::Result<File> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path)
}

/// Take the lock if no one holds it: `Ok(None)` when another holder has it.
pub fn try_exclusive(path: &Path) -> io::Result<Option<Lock>> {
    let file = open(path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(Lock { _file: file })),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(e)) => Err(e),
    }
}

/// Take the lock, retrying for up to `patience` while someone else holds it. `Ok(None)` if it is
/// still held then. A probe ([`is_held`]) holds a lock for microseconds; a short patience keeps it
/// from turning a new holder away.
pub fn exclusive_within(path: &Path, patience: Duration) -> io::Result<Option<Lock>> {
    let start = Instant::now();
    loop {
        if let Some(l) = try_exclusive(path)? {
            return Ok(Some(l));
        }
        if start.elapsed() >= patience {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Take the lock, waiting for as long as it is held.
pub fn exclusive(path: &Path) -> io::Result<Lock> {
    let file = open(path)?;
    file.lock()?;
    Ok(Lock { _file: file })
}

/// Is the lock held by someone? Probes with a shared lock, released at once; a missing lock file
/// is not held. Never creates the file.
pub fn is_held(path: &Path) -> io::Result<bool> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    match file.try_lock_shared() {
        Ok(()) => {
            file.unlock()?;
            Ok(false)
        }
        Err(std::fs::TryLockError::WouldBlock) => Ok(true),
        Err(std::fs::TryLockError::Error(e)) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_holder_at_a_time_and_released_on_drop() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("q/x.lock");
        assert!(!is_held(&p).unwrap(), "no file, not held");
        let a = try_exclusive(&p).unwrap().expect("free");
        assert!(is_held(&p).unwrap());
        assert!(try_exclusive(&p).unwrap().is_none(), "single flight");
        assert!(exclusive_within(&p, Duration::from_millis(60)).unwrap().is_none());
        drop(a);
        assert!(!is_held(&p).unwrap());
        let b = exclusive_within(&p, Duration::from_millis(60)).unwrap();
        assert!(b.is_some());
    }

    /// A copy of the descriptor (what a process being started holds until its exec) does not keep
    /// a dropped lock held.
    #[test]
    fn a_dropped_lock_is_free_while_a_copy_of_its_descriptor_lives() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("x.lock");
        let a = try_exclusive(&p).unwrap().unwrap();
        let copy = a._file.try_clone().unwrap();
        drop(a);
        assert!(!is_held(&p).unwrap());
        assert!(try_exclusive(&p).unwrap().is_some());
        drop(copy);
    }

    #[test]
    fn a_waiter_gets_the_lock_when_it_is_released() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("x.lock");
        let a = exclusive(&p).unwrap();
        let p2 = p.clone();
        let h = std::thread::spawn(move || exclusive_within(&p2, Duration::from_secs(5)).unwrap().is_some());
        std::thread::sleep(Duration::from_millis(100));
        drop(a);
        assert!(h.join().unwrap());
    }
}
