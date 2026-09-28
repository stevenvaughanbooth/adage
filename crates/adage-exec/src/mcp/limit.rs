//! Bounds on tool calls (Track C §3.5): at most 30 calls a minute per session (one server process
//! serves one harness session), and at most 4 runs at once across every `adage-exec mcp` on this
//! machine (lock files under `$ADAGE_HOME/exec/mcp-slots`, held with `flock` for the length of a
//! run and released by the kernel if the process dies).

use std::collections::VecDeque;
use std::path::Path;
use std::time::{Duration, Instant};

/// Calls a session may make in any 60 seconds.
pub const PER_MINUTE: usize = 30;
/// Runs at once on this machine.
pub const CONCURRENCY: usize = 4;
/// How long a call waits for a free run slot before it is refused.
pub const SLOT_WAIT: Duration = Duration::from_secs(10);

/// A sliding one-minute window.
#[derive(Debug, Default)]
pub struct RateLimit {
    calls: VecDeque<Instant>,
}

impl RateLimit {
    /// Count a call at `now`; `false` when the window is already full (the call is not counted).
    pub fn allow(&mut self, now: Instant) -> bool {
        while self.calls.front().is_some_and(|t| now.duration_since(*t) >= Duration::from_secs(60)) {
            self.calls.pop_front();
        }
        if self.calls.len() >= PER_MINUTE {
            return false;
        }
        self.calls.push_back(now);
        true
    }
}

/// A held run slot; dropping it releases the lock.
pub struct Slot {
    #[allow(dead_code)] // held for its lock
    file: std::fs::File,
}

#[cfg(unix)]
fn try_lock(f: &std::fs::File) -> bool {
    use std::os::unix::io::AsRawFd;
    // SAFETY: flock on a descriptor this function borrows; no memory is passed.
    unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) == 0 }
}

#[cfg(not(unix))]
fn try_lock(_f: &std::fs::File) -> bool {
    // No sandbox exists off Unix, so nothing ever runs there; a slot is never contended.
    true
}

/// Take one of `n` slots in `dir`, waiting up to `wait`. `None` when none frees in time or the
/// directory cannot be used (fail closed: the call is refused).
pub fn acquire(dir: &Path, n: usize, wait: Duration) -> Option<Slot> {
    std::fs::create_dir_all(dir).ok()?;
    let deadline = Instant::now() + wait;
    loop {
        for i in 0..n {
            let p = dir.join(format!("{i}.lock"));
            if std::fs::symlink_metadata(&p).is_ok_and(|m| !m.file_type().is_file()) {
                return None;
            }
            let Ok(f) = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&p) else {
                return None;
            };
            if try_lock(&f) {
                return Some(Slot { file: f });
            }
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thirty_a_minute() {
        let mut r = RateLimit::default();
        let t0 = Instant::now();
        for i in 0..PER_MINUTE {
            assert!(r.allow(t0 + Duration::from_millis(i as u64)), "call {i}");
        }
        assert!(!r.allow(t0 + Duration::from_secs(30)));
        assert!(r.allow(t0 + Duration::from_secs(61)), "the window slides");
    }

    #[cfg(unix)]
    #[test]
    fn at_most_n_slots_at_once() {
        let d = tempfile::tempdir().unwrap();
        let held: Vec<Slot> =
            (0..CONCURRENCY).map(|_| acquire(d.path(), CONCURRENCY, Duration::ZERO).unwrap()).collect();
        assert!(acquire(d.path(), CONCURRENCY, Duration::from_millis(50)).is_none());
        drop(held);
        assert!(acquire(d.path(), CONCURRENCY, Duration::ZERO).is_some());
    }
}
