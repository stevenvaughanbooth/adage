//! Daemonless session storage, as validated by spike S3.
//!
//! * **State** — `<session>.state`: two fixed-size slots, each `magic | seq | len | crc | payload`.
//!   A write goes to the *older* slot with one `pwrite`, under an exclusive `flock` on the file.
//!   The newest slot with a valid CRC wins, so a crash mid-write leaves the previous state intact.
//!   State is a cache: it can always be rebuilt from the spool.
//! * **Spool** — `<session>.spool`: append-only, one `O_APPEND` write per CRC-framed record, so
//!   concurrent writers never interleave. Readers stop at the first incomplete or corrupt frame
//!   and resynchronise on the next magic.

mod crc;
pub mod paths;
pub mod spool;
pub mod state;

pub use paths::Paths;
