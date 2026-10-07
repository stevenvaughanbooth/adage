//! Daemonless session storage, as validated by spike S3.
//!
//! * **State** — `<session>.state`: two fixed-size slots, each `magic | seq | len | crc | payload`.
//!   A write goes to the *older* slot with one positional write (`pwrite`; `WriteFile` at an
//!   offset on Windows), under an exclusive lock on the file (`flock`; `LockFileEx` on Windows).
//!   The newest slot with a valid CRC wins, so a crash mid-write leaves the previous state intact.
//!   State is a cache: it can always be rebuilt from the spool.
//! * **Spool** — `<session>.spool`: append-only, one `O_APPEND` write per CRC-framed record, so
//!   concurrent writers never interleave (`FILE_APPEND_DATA` on Windows, likewise one write per
//!   record). Readers stop at the first incomplete or corrupt frame and resynchronise on the next
//!   magic.
//! * **Config** — `config.toml`: the person's settings ([`config`]).
//! * **Locks** — single-flight file locks for live learning ([`lock`]).
//! * **Release** — the last release check's answer, `cache/latest.json` ([`release`]).

pub mod config;
mod crc;
pub mod lock;
pub mod paths;
pub mod release;
pub mod spool;
pub mod state;

pub use paths::{home_dir, Paths};
