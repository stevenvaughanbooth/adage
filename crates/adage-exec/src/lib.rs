//! `adage-exec`: the optional component that may run a route, and only an approved one, in an OS
//! sandbox, returning its output as typed, fenced data (Track C §3, §4).
//!
//! The `adage` binary never spawns anything (Q4a, checked by `tests/no_spawn.rs`). This crate is
//! the only place that does, and every run goes through the same gates, in order:
//!
//! 1. the request is well formed: every slot bound to a value its contract accepts
//!    ([`request::ExecRequest::bind`]);
//! 2. the declared effect is not lower than the local classification ([`classify`]);
//! 3. policy permits it ([`policy`]);
//! 4. an approval matches on every field of the key, or the request is in the inert-observe class
//!    and policy turns automatic approval on ([`approvals`]);
//! 5. a sandbox is available and enforced ([`sandbox`]); otherwise nothing runs (fail closed);
//! 6. the run is bounded ([`limits`]) with an environment built from nothing ([`env`]) and git
//!    hardening ([`git`]);
//! 7. the output is projected to its declared typed fields, or, in raw mode, stripped and scanned
//!    ([`project`], [`scan`]); any failure withholds the whole block;
//! 8. the block is rendered from one fixed template ([`render`]).

pub mod approvals;
pub mod classify;
pub mod env;
pub mod git;
pub mod limits;
pub mod person;
pub mod policy;
pub mod project;
pub mod render;
pub mod request;
pub mod run;
pub mod sandbox;
pub mod scan;
pub mod time;

pub use request::{ExecRequest, Level, OutputMode};

/// Lower-case hex of a byte slice.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// SHA-256 of `bytes`, as lower-case hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex(&Sha256::digest(bytes))
}

/// Canonical JSON (object keys sorted at every depth, no whitespace), as
/// `EngineParams::canonical_json` does. Keys are sorted explicitly, so the result does not depend
/// on whether some crate in the build turns on `serde_json/preserve_order`.
pub fn canonical_json<T: serde::Serialize>(v: &T) -> String {
    fn write(v: &serde_json::Value, out: &mut String) {
        match v {
            serde_json::Value::Object(m) => {
                let mut keys: Vec<&String> = m.keys().collect();
                keys.sort();
                out.push('{');
                for (i, k) in keys.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::to_string(k).unwrap_or_default());
                    out.push(':');
                    write(&m[k.as_str()], out);
                }
                out.push('}');
            }
            serde_json::Value::Array(a) => {
                out.push('[');
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write(x, out);
                }
                out.push(']');
            }
            other => out.push_str(&serde_json::to_string(other).unwrap_or_default()),
        }
    }
    let value = serde_json::to_value(v).unwrap_or(serde_json::Value::Null);
    let mut out = String::new();
    write(&value, &mut out);
    out
}
