//! Spike S3 — daemonless session state for Adage.
//!
//! * `spool`    — append-only per-session observation spool (framing, scan/resync, reconstruct)
//! * `state`    — the §4.4 session state, its binary image, and the deterministic fold
//! * `store`    — the daemonless hot path: flock'd state files + spool appends
//! * `daemon`   — the comparison point: a minimal Unix-socket daemon with in-memory state
//! * `workload` — a model of Claude Code hook traffic and a concurrent driver
//! * `exec`     — executors (in-process files, process-per-hook, daemon client)
//! * `checks`   — correctness checks shared by tests and the `correctness` command

pub mod checks;
pub mod daemon;
pub mod exec;
pub mod spool;
pub mod state;
pub mod store;
pub mod util;
pub mod workload;
