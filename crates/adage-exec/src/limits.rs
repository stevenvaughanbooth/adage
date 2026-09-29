//! Resource limits per level (Track C §3.3): wall time, captured and returned bytes, and the
//! rlimits every sandboxed process gets.

use serde::{Deserialize, Serialize};

use crate::request::Level;

pub const KIB: u64 = 1024;
pub const MIB: u64 = 1024 * KIB;
pub const GIB: u64 = 1024 * MIB;

/// Bounds on one run. Part of the sandbox profile, so part of its id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Limits {
    /// Wall-clock timeout; the whole process group is killed when it passes.
    pub wall_ms: u64,
    /// Bytes of stdout kept; more kills the process group and marks the run truncated.
    pub capture_bytes: u64,
    /// Bytes of the rendered payload that may reach the agent.
    pub return_bytes: u64,
    /// `RLIMIT_AS`.
    pub address_space: u64,
    /// `RLIMIT_NPROC` (per user; see `sandbox::linux` for why root runs as nobody).
    pub nproc: u64,
    /// `RLIMIT_FSIZE`: the largest file the run may write (in scratch).
    pub file_size: u64,
    /// `RLIMIT_NOFILE`.
    pub open_files: u64,
}

/// E1's wall timeout may be raised to at most this (§3.3 table).
pub const E1_MAX_WALL_MS: u64 = 3_000;
/// An E2 action runs inside a hook, so it stays well within the hook timeout its registration
/// sets ([`adage_hook::harness::ACTION_HOOK_TIMEOUT_S`], 30 s): at most 20 s.
pub const E2_MAX_WALL_MS: u64 = 20_000;

impl Limits {
    pub fn for_level(level: Level) -> Limits {
        let (wall_ms, capture_bytes, return_bytes) = match level {
            Level::E1 => (1_500, 64 * KIB, 4 * KIB),
            Level::E2 => (E2_MAX_WALL_MS, 256 * KIB, 4 * KIB),
        };
        Limits {
            wall_ms,
            capture_bytes,
            return_bytes,
            address_space: 2 * GIB,
            nproc: 256,
            file_size: 64 * MIB,
            open_files: 1024,
        }
    }

    /// A route may ask for a different wall timeout, within the level's bounds.
    pub fn with_timeout(mut self, level: Level, ms: u64) -> Limits {
        let max = match level {
            Level::E1 => E1_MAX_WALL_MS,
            Level::E2 => E2_MAX_WALL_MS,
        };
        self.wall_ms = ms.clamp(1, max);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_match_the_spec_table() {
        let e1 = Limits::for_level(Level::E1);
        assert_eq!((e1.wall_ms, e1.capture_bytes, e1.return_bytes), (1_500, 65_536, 4_096));
        let e2 = Limits::for_level(Level::E2);
        assert_eq!((e2.wall_ms, e2.capture_bytes, e2.return_bytes), (20_000, 262_144, 4_096));
        assert_eq!(e1.with_timeout(Level::E1, 60_000).wall_ms, 3_000);
        assert_eq!(e2.with_timeout(Level::E2, 60_000).wall_ms, 20_000, "an action stays within its hook's timeout");
        assert_eq!((e1.address_space, e1.nproc, e1.file_size), (2 << 30, 256, 64 << 20));
    }
}
