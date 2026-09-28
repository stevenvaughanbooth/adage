//! Attestations (Track C §1.8): `attestations/<id>.json`, optional records in the bench result
//! schema (`evals/bench/result.schema.json`, versions 1 and 2), carried by a pack and covered by
//! its signature.
//!
//! They are **informational only**. Nothing reads them to decide anything: they never grant a
//! strength (strength), a level or an approval. `adage review` shows them marked "self-reported by
//! signer", because a signature proves who wrote the numbers, not that they are true.
//!
//! [`check`] reads one as untrusted input against the schema's envelope, strictly: every key
//! known, every string bounded and printable, every number finite. [`summary`] renders the fields
//! a person needs (tier, suite, run, params hash, execution level, arm deltas, harm counts) from
//! fixed text.

use anyhow::{bail, Result};
use serde_json::{Map, Value};

/// Most attestations a pack may carry.
pub const MAX_ATTESTATIONS: usize = 16;
/// Most entries in `arms`, `deltas` or `harm`.
const MAX_ENTRIES: usize = 32;

/// Record kinds of the bench schema (`arms` is version 2).
pub const RECORDS: [&str; 6] = ["run", "trial", "directive", "aggregate", "scorecard", "arms"];
/// Tiers: version 1 knows `l0` only; version 2 adds the security, simulator and live tiers.
pub const TIERS: [&str; 4] = ["l0", "l0-sec", "l1", "l2"];
/// Execution levels an attested run may have used (version 2).
pub const EXEC_LEVELS: [&str; 3] = ["e0", "e1", "e2"];
const SPLIT_KINDS: [&str; 8] = ["group", "hash", "temporal", "all", "named", "cv", "final", "suite"];

/// Keys of the envelope.
const ENVELOPE: [&str; 13] = [
    "schema_version",
    "record",
    "run_id",
    "tier",
    "suite",
    "git_sha",
    "adage_version",
    "params",
    "params_hash",
    "dataset",
    "split",
    "reason",
    "exec_level",
];

/// Keys each record kind's body may hold.
fn body_keys(record: &str) -> &'static [&'static str] {
    match record {
        "run" => &["label", "window", "preset", "objective", "units"],
        "trial" => &[
            "role",
            "session",
            "task",
            "turns",
            "directives",
            "correct",
            "correct_in_window",
            "false_directives",
            "turns_saved",
            "turns_saved_at_window",
            "key_steps",
            "covered",
        ],
        "directive" => &[
            "role",
            "session",
            "route",
            "kind",
            "event",
            "turn",
            "correct",
            "in_window",
            "matched_turn",
            "turns_saved",
        ],
        "aggregate" => &["role", "metric", "estimate", "ci_low", "ci_high", "ci_method", "n"],
        "scorecard" => &["label", "objective", "feasible", "terms", "guardrails", "final"],
        "arms" => &["arms", "deltas", "harm", "sessions"],
        _ => &[],
    }
}

fn hex(s: &str, lo: usize, hi: usize) -> bool {
    (lo..=hi).contains(&s.len()) && s.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

/// A string a person may be shown: bounded, printable, nothing invisible.
fn text(v: &Value, max: usize) -> Option<&str> {
    v.as_str().filter(|s| crate::validate::people_text_ok(s, max) && !s.contains('\n'))
}

/// Every value in a record: bounded depth, strings printable, numbers finite.
fn tame(v: &Value, depth: usize) -> bool {
    if depth > 6 {
        return false;
    }
    match v {
        Value::String(s) => crate::validate::people_text_ok(s, 1000),
        Value::Number(n) => n.as_f64().is_some_and(f64::is_finite),
        Value::Array(a) => a.len() <= 256 && a.iter().all(|x| tame(x, depth + 1)),
        Value::Object(m) => m.len() <= 64 && m.iter().all(|(k, x)| k.len() <= 64 && safe_key(k) && tame(x, depth + 1)),
        Value::Bool(_) | Value::Null => true,
    }
}

fn safe_key(k: &str) -> bool {
    !k.is_empty() && k.len() <= 64 && k.bytes().all(|c| c.is_ascii_alphanumeric() || b"_-.".contains(&c))
}

fn named_numbers(v: &Value, what: &str, integers: bool) -> Result<()> {
    let Some(m) = v.as_object() else { bail!("{what} is not an object") };
    if m.len() > MAX_ENTRIES {
        bail!("{what} has more than {MAX_ENTRIES} entries");
    }
    for (k, x) in m {
        if !safe_key(k) {
            bail!("{what}: key {k:?} is not allowed");
        }
        let ok = if integers { x.as_u64().is_some() } else { x.as_f64().is_some_and(f64::is_finite) };
        if !ok {
            bail!("{what}.{k} is not {}", if integers { "a non-negative integer" } else { "a finite number" });
        }
    }
    Ok(())
}

/// Check one attestation against the bench result schema's envelope (versions 1 and 2), strictly.
pub fn check(v: &Value) -> Result<()> {
    let Some(m) = v.as_object() else { bail!("not a JSON object") };
    let version = m.get("schema_version").and_then(Value::as_u64);
    if !matches!(version, Some(1 | 2)) {
        bail!("schema_version is not 1 or 2");
    }
    let Some(record) = m.get("record").and_then(Value::as_str).filter(|r| RECORDS.contains(r)) else {
        bail!("record is not one of {RECORDS:?}")
    };
    if record == "arms" && version != Some(2) {
        bail!("an arms record is schema version 2");
    }
    for k in m.keys() {
        if !ENVELOPE.contains(&k.as_str()) && !body_keys(record).contains(&k.as_str()) {
            bail!("unknown key {:?}", k.chars().take(40).collect::<String>());
        }
    }
    for req in ["run_id", "tier", "suite", "adage_version", "params", "params_hash", "split"] {
        if !m.contains_key(req) {
            bail!("{req} is required");
        }
    }
    if !m["run_id"].as_str().is_some_and(|s| hex(s, 16, 16)) {
        bail!("run_id is not 16 hex digits");
    }
    let tier = m["tier"].as_str().unwrap_or_default();
    let tier_ok = if version == Some(1) { tier == "l0" } else { TIERS.contains(&tier) };
    if !tier_ok {
        bail!("tier {tier:?} is not a tier of schema version {}", version.unwrap_or(0));
    }
    if text(&m["suite"], 128).is_none() {
        bail!("suite is not a short printable string");
    }
    if text(&m["adage_version"], 64).is_none() {
        bail!("adage_version is not a short printable string");
    }
    if !m["params_hash"].as_str().is_some_and(|s| hex(s, 64, 64)) {
        bail!("params_hash is not a SHA-256");
    }
    let params = m["params"].as_object();
    if !params
        .is_some_and(|p| p.get("runtime").is_some_and(Value::is_object) && p.get("learn").is_some_and(Value::is_object))
    {
        bail!("params is not an EngineParams object (runtime, learn)");
    }
    let split = m["split"].as_object();
    let split_ok = split.is_some_and(|s| {
        s.keys().all(|k| ["kind", "name", "seed"].contains(&k.as_str()))
            && s.get("kind").and_then(Value::as_str).is_some_and(|k| SPLIT_KINDS.contains(&k))
            && s.get("name").is_some_and(|n| text(n, 128).is_some())
            && s.get("seed").is_some_and(|x| x.is_null() || text(x, 64).is_some())
    });
    if !split_ok {
        bail!("split is not {{kind, name, seed}}");
    }
    if let Some(g) = m.get("git_sha") {
        if !g.as_str().is_some_and(|s| hex(s, 7, 40)) {
            bail!("git_sha is not a git revision");
        }
    }
    if let Some(d) = m.get("dataset") {
        let ok = d.as_object().is_some_and(|d| {
            d.len() == 2
                && d.get("id").is_some_and(|x| text(x, 128).is_some())
                && d.get("content_sha256").and_then(Value::as_str).is_some_and(|s| hex(s, 64, 64))
        });
        if !ok {
            bail!("dataset is not {{id, content_sha256}}");
        }
    }
    if let Some(e) = m.get("exec_level") {
        if version != Some(2) || !e.as_str().is_some_and(|s| EXEC_LEVELS.contains(&s)) {
            bail!("exec_level is not one of {EXEC_LEVELS:?} (schema version 2)");
        }
    }
    if record == "arms" {
        let Some(arms) = m.get("arms").and_then(Value::as_object) else { bail!("an arms record has arms") };
        if arms.is_empty() || arms.len() > MAX_ENTRIES || !arms.keys().all(|k| safe_key(k)) {
            bail!("arms names 1 to {MAX_ENTRIES} arms by safe names");
        }
        for (k, a) in arms {
            named_numbers(a, &format!("arms.{k}"), false)?;
        }
        named_numbers(m.get("deltas").unwrap_or(&Value::Null), "deltas", false)?;
        if let Some(h) = m.get("harm") {
            named_numbers(h, "harm", true)?;
        }
    }
    if !m.values().all(|x| tame(x, 0)) {
        bail!("a value is too deep, too long, not printable or not finite");
    }
    Ok(())
}

fn num(v: &Value) -> String {
    match v.as_f64() {
        Some(f) if f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", f as i64),
        Some(f) => format!("{f:.3}"),
        None => "?".into(),
    }
}

fn pairs(m: Option<&Map<String, Value>>, signed: bool) -> String {
    m.map(|m| {
        m.iter()
            .map(|(k, v)| {
                let n = num(v);
                let n = if signed && !n.starts_with(['+', '-']) && n != "?" { format!("+{n}") } else { n };
                format!("{k} {n}")
            })
            .collect::<Vec<_>>()
            .join(", ")
    })
    .unwrap_or_default()
}

/// One line for people, from fixed text (the attestation has passed [`check`]).
pub fn summary(v: &Value) -> String {
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("?").to_string();
    let mut out = format!(
        "{} record · tier {} · suite {} · run {} · params {}",
        s("record"),
        s("tier"),
        s("suite"),
        s("run_id"),
        s("params_hash").chars().take(12).collect::<String>()
    );
    if let Some(e) = v.get("exec_level").and_then(Value::as_str) {
        out.push_str(&format!(" · exec {}", e.to_uppercase()));
    }
    match s("record").as_str() {
        "aggregate" => out.push_str(&format!(
            " · {} {} (n {})",
            s("metric"),
            num(v.get("estimate").unwrap_or(&Value::Null)),
            num(v.get("n").unwrap_or(&Value::Null))
        )),
        "scorecard" => out.push_str(&format!(
            " · objective {} · feasible {}",
            num(v.get("objective").unwrap_or(&Value::Null)),
            v.get("feasible").and_then(Value::as_bool).map(|b| if b { "yes" } else { "no" }).unwrap_or("?")
        )),
        "arms" => {
            let arms: Vec<&str> = v
                .get("arms")
                .and_then(Value::as_object)
                .map(|a| a.keys().map(String::as_str).collect())
                .unwrap_or_default();
            out.push_str(&format!(" · arms {}", arms.join(", ")));
            let d = pairs(v.get("deltas").and_then(Value::as_object), true);
            if !d.is_empty() {
                out.push_str(&format!(" · deltas {d}"));
            }
            let h = pairs(v.get("harm").and_then(Value::as_object), false);
            if !h.is_empty() {
                out.push_str(&format!(" · harm {h}"));
            }
        }
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn arms() -> Value {
        json!({
            "schema_version": 2, "record": "arms", "run_id": "0123456789abcdef", "tier": "l2",
            "suite": "gen-smoke", "adage_version": "0.5.0",
            "params": { "runtime": {}, "learn": {} }, "params_hash": "ab".repeat(32),
            "split": { "kind": "suite", "name": "all", "seed": null }, "exec_level": "e1",
            "arms": { "e0": { "sessions": 40 }, "e1": { "sessions": 40 } },
            "deltas": { "turns_saved": 0.62, "cost_usd": -0.01 },
            "harm": { "canary_actions": 0 }
        })
    }

    #[test]
    fn a_v2_arms_record_passes_and_reads_as_one_line() {
        check(&arms()).unwrap();
        assert_eq!(
            summary(&arms()),
            "arms record · tier l2 · suite gen-smoke · run 0123456789abcdef · params abababababab · exec E1 · arms e0, e1 \
             · deltas cost_usd -0.010, turns_saved +0.620 · harm canary_actions 0"
        );
    }

    #[test]
    fn hostile_or_malformed_records_are_refused() {
        type Mutation = Box<dyn Fn(&mut Value)>;
        let cases: Vec<(&str, Mutation)> = vec![
            ("version", Box::new(|v| v["schema_version"] = json!(3))),
            ("record", Box::new(|v| v["record"] = json!("praise"))),
            ("run id", Box::new(|v| v["run_id"] = json!("xyz"))),
            ("tier", Box::new(|v| v["tier"] = json!("l9"))),
            ("v1 tier", Box::new(|v| v["schema_version"] = json!(1))),
            ("unknown key", Box::new(|v| v["grants"] = json!("fire"))),
            ("invisible", Box::new(|v| v["suite"] = json!("a\u{202e}b"))),
            ("newline", Box::new(|v| v["suite"] = json!("a\nb"))),
            ("params hash", Box::new(|v| v["params_hash"] = json!("00"))),
            ("params", Box::new(|v| v["params"] = json!({}))),
            ("split", Box::new(|v| v["split"] = json!({ "kind": "x", "name": "a", "seed": null }))),
            ("delta", Box::new(|v| v["deltas"] = json!({ "a": "big" }))),
            ("harm", Box::new(|v| v["harm"] = json!({ "a": -1 }))),
            ("exec", Box::new(|v| v["exec_level"] = json!("e9"))),
        ];
        for (what, m) in &cases {
            let mut v = arms();
            m(&mut v);
            assert!(check(&v).is_err(), "{what}");
        }
        let mut v1 = arms();
        v1["schema_version"] = json!(1);
        v1["record"] = json!("aggregate");
        v1["tier"] = json!("l0");
        for k in ["exec_level", "arms", "deltas", "harm"] {
            v1.as_object_mut().unwrap().remove(k);
        }
        for (k, x) in
            [("role", json!("final")), ("metric", json!("precision")), ("estimate", json!(0.9)), ("n", json!(12))]
        {
            v1[k] = x;
        }
        check(&v1).unwrap();
        assert!(summary(&v1).ends_with("· precision 0.900 (n 12)"), "{}", summary(&v1));
    }
}
