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
pub mod completion;
pub mod env;
pub mod git;
pub mod hook;
pub mod journal;
pub mod limits;
pub mod person;
pub mod policy;
pub mod project;
pub mod project_python;
pub mod render;
pub mod replay;
pub mod request;
pub mod route;
pub mod run;
pub mod sandbox;
pub mod scan;
pub mod stage;
pub mod time;
pub mod verification;

pub use request::{ExecRequest, Level, OutputMode};

/// A refusal reason as a spool record keeps it: one line, no control characters, at most 300
/// characters. Reasons never hold output text; this only bounds them.
pub fn reason_text(s: &str) -> String {
    let one: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let one = one.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() > 300 {
        format!("{}…", one.chars().take(299).collect::<String>())
    } else {
        one
    }
}

/// The most recent withheld prefetch or action, from the session spools
/// (`$ADAGE_HOME/sessions/*.spool`): what `adage-exec doctor` and `adage review` show.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LastWithheld {
    pub route: String,
    pub level: String,
    pub stage: String,
    /// Why, as the refusing stage said it; empty in a record written before reasons were kept.
    pub reason: String,
    /// When (milliseconds since the Unix epoch), and in which session.
    pub ts: u64,
    pub session: String,
}

impl LastWithheld {
    /// One line: `withheld at sandbox: <reason> (acme/work-items/lookup, e1, session s1)`.
    pub fn line(&self) -> String {
        let reason = match self.reason.trim_start_matches("withheld: ") {
            "" => "no reason recorded",
            r => r,
        };
        format!("withheld at {}: {reason} ({}, {}, session {})", self.stage, self.route, self.level, self.session)
    }
}

/// The last withheld attempt recorded under `home` ([`LastWithheld`]), reading at most the 64
/// most recently changed spools.
pub fn last_withheld(home: &std::path::Path) -> Option<LastWithheld> {
    let dir = adage_store::Paths::at(home).sessions();
    let mut spools: Vec<(std::time::SystemTime, std::path::PathBuf)> = std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "spool"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    spools.sort_by_key(|a| std::cmp::Reverse(a.0));
    let mut best: Option<LastWithheld> = None;
    for (_, p) in spools.into_iter().take(64) {
        let session = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        for rec in adage_store::spool::read_all::<serde_json::Value>(&p).unwrap_or_default() {
            let ts = rec.get("ts").and_then(|t| t.as_u64()).unwrap_or(0);
            for x in rec.get("exec").and_then(|e| e.as_array()).into_iter().flatten() {
                let Some(stage) = x.get("withheld").and_then(|s| s.as_str()) else { continue };
                if best.as_ref().is_some_and(|b| b.ts > ts) {
                    continue;
                }
                let s = |k: &str| x.get(k).and_then(|v| v.as_str()).unwrap_or_default().to_string();
                best = Some(LastWithheld {
                    route: s("route"),
                    level: s("level"),
                    stage: stage.to_string(),
                    reason: reason_text(&s("reason")),
                    ts,
                    session: session.clone(),
                });
            }
        }
    }
    best
}

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
