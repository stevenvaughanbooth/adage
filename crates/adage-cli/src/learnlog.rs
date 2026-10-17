//! `$ADAGE_HOME/learn.log`: one JSON object per line, appended by every learn, manual or
//! automatic. `adage ui` / `adage watch` tail it (with `sessions/*.spool`) to show learning live;
//! `adage doctor` reads it for the last automatic learn per repository.
//!
//! **A learn** — one line per repository it learned (a manual run over several repositories writes
//! several lines with the same `ts`; one that found no usable session writes one with `repo` null):
//!
//! ```json
//! {"ts":1758620000000,"kind":"condition","trigger":"auto","repo":"github.com/acme/billing",
//!  "sessions_read":212,"sessions_used":198,"duration_ms":431,"index_generation":"3f1c…",
//!  "changes":[{"route":"finish-line-6a01b2","track":"github-com-acme-billing/finish-line",
//!              "change":"strength","from":"prime","to":"fire",
//!              "summary":"reflex prime → fire (9 of 9 traces, lower bound 0.70)"}],
//!  "version":"0.1.0","sessions_steered":3,"thresholds":"min_support=5 …","intent_model":null,
//!  "repos":{"github.com/acme/billing":{"sessions":198,"tracks":[{"name":"…","version":"1.2.0",
//!           "change":"reflex added","routes":4}]}}}
//! ```
//!
//! - `trigger`: `auto` (the background learner a hook started) or `manual` (`adage condition`).
//! - `sessions_read`: transcripts the run read (an automatic run counts only its repository's);
//!   `sessions_used`: this repository's sessions with steps.
//! - `input_scope`: `repository` when input counts belong to the one reported repository,
//!   otherwise `run`; a manual run can repeat global input counts on several repository lines.
//! - `changes`: [`adage_learn::diff`] — `change` is `new`, `extinguished`, `strength`, `evidence`, `nascent`
//!   or `proposal`; `from`/`to` are strengths (`fire`, `prime`, `inhibit`) or null; `summary` is a
//!   fixed template (route kind, strength, session counts).
//! - `version` … `repos` are the fields every line had before live learning, kept as they were.
//!
//! **A failed conditioning run**: `{"ts", "kind":"condition_error", "trigger", "repo", "class", "error"}`, where
//! `class` is one of `repo` (the repository's directory is gone), `lock` (the tracks lock could not
//! be taken), `learn` (learning failed) or `panic`, and `error` a fixed message or an I/O error
//! kind.
//!
//! No line ever holds prompt text, a command, tool output, or a path: repository keys, track names
//! and route ids are derived identifiers. Readers must ignore unknown kinds and fields. The file is
//! rotated to `learn.log.1` at [`ROTATE_BYTES`], like `fail-open.log`; each line is one write, so
//! concurrent learners append whole lines.

use adage_learn::diff::Change;
use adage_learn::Report;
use adage_store::Paths;
use serde_json::{json, Value};
use std::io::Write;

/// Rotated (to `learn.log.1`) when it reaches this size.
pub const ROTATE_BYTES: u64 = 256 << 10;

pub use adage_core::time::now_ms;

/// Append one line, rotating first if the log is full.
pub fn append(paths: &Paths, line: &Value) -> std::io::Result<()> {
    let path = paths.learn_log();
    if std::fs::metadata(&path).is_ok_and(|m| m.len() >= ROTATE_BYTES) {
        std::fs::rename(&path, path.with_extension("log.1"))?;
    }
    std::fs::create_dir_all(&paths.root)?;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&path)?;
    f.write_all(format!("{line}\n").as_bytes())
}

/// What one learn did for one repository.
pub struct Learned<'a> {
    pub trigger: &'a str,
    pub repo: Option<&'a str>,
    /// The repository the input was scoped or explicitly assigned to, not inferred from outputs.
    pub input_repo: Option<&'a str>,
    pub sessions_read: usize,
    pub duration_ms: u64,
    pub changes: &'a [Change],
    pub report: &'a Report,
}

/// The `learn` line for one repository.
pub fn learn_line(ts: u64, l: &Learned) -> Value {
    let rep = l.report;
    let rr = l.repo.and_then(|r| rep.repos.get(r));
    let repos: serde_json::Map<String, Value> = match (l.repo, rr) {
        (Some(repo), Some(rr)) => {
            let tracks: Vec<Value> = rr
                .tracks
                .iter()
                .map(|t| json!({ "name": t.name, "version": t.version, "change": t.change, "routes": t.routes.len() }))
                .collect();
            let mut entry = json!({ "sessions": rr.sessions, "tracks": tracks });
            // Instinct counts only: ids and reasons stay in `adage condition --json`.
            if let Some(i) = &rr.instincts {
                entry["instincts"] = json!({
                    "bound": i.bound,
                    "advice": i.advice,
                    "nascent": i.nascent,
                    "extinguished": i.extinguished.len(),
                    "confirmed": i.confirmed.len(),
                });
            }
            [(repo.to_string(), entry)].into_iter().collect()
        }
        _ => Default::default(),
    };
    let mut line = json!({
        "ts": ts,
        "kind": "condition",
        "trigger": l.trigger,
        "repo": l.repo,
        "sessions_read": l.sessions_read,
        "input_scope": if l.repo.is_some() && l.repo == l.input_repo { "repository" } else { "run" },
        "sessions_used": rr.map(|r| r.sessions).unwrap_or(0),
        "duration_ms": l.duration_ms,
        "index_generation": rep.index_generation,
        "changes": l.changes,
        "version": env!("CARGO_PKG_VERSION"),
        "sessions_steered": rep.sessions_steered,
        "thresholds": rep.thresholds,
        "intent_model": rep.intent_model,
        "repos": repos,
    });
    if let Some(generation) = &rep.corpus_generation {
        line["corpus_generation"] = json!(generation);
    }
    line
}

/// The `condition_error` line.
pub fn error_line(ts: u64, trigger: &str, repo: Option<&str>, class: &str, error: &str) -> Value {
    let error: String = error.chars().filter(|c| !c.is_control()).take(200).collect();
    json!({ "ts": ts, "kind": "condition_error", "trigger": trigger, "repo": repo, "class": class, "error": error })
}

/// A fixed description of a learn failure: the I/O error kind if one caused it, never its text
/// (which can name a path).
pub fn describe(e: &anyhow::Error) -> String {
    match e.chain().find_map(|c| c.downcast_ref::<std::io::Error>()) {
        Some(io) => format!("I/O error ({:?})", io.kind()),
        None => "conditioning failed; run `adage condition --json` to see why".into(),
    }
}

/// Every line of `learn.log.1` then `learn.log` that parses, oldest first.
pub fn read(paths: &Paths) -> Vec<Value> {
    let log = paths.learn_log();
    [log.with_extension("log.1"), log]
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .flat_map(|t| t.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).collect::<Vec<_>>())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_usable_repository_does_not_make_global_input_counts_repository_scoped() {
        let mut report = Report { sessions_read: 20, ..Default::default() };
        report.repos.insert("acme".into(), adage_learn::RepoReport { sessions: 3, ..Default::default() });
        for (input_repo, expected) in [(None, "run"), (Some("other"), "run"), (Some("acme"), "repository")] {
            let line = learn_line(
                1,
                &Learned {
                    trigger: "manual",
                    repo: Some("acme"),
                    input_repo,
                    sessions_read: 20,
                    duration_ms: 0,
                    changes: &[],
                    report: &report,
                },
            );
            assert_eq!(line["input_scope"], expected);
            assert_eq!(line["sessions_used"], 3);
        }
    }
}
