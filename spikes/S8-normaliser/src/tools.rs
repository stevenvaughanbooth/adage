//! Harness native tool call → canonical action. Mirrors `research/poc/src/normalize/tools.ts`.
//!
//! In production this table is per-harness-version data (Architecture §4.7: "mapping tables per
//! harness version as data"); here it is a small match, since only the Bash path is in scope
//! for this spike (native tools already carry structured args and normalise with confidence 1).

use serde_json::Value;

use crate::shell::{normalize_shell, resolve_path};
use crate::table::CommandTable;
use crate::types::{CanonicalAction, Effect, Normalized};

fn s(v: &Value, key: &str) -> String {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("").to_string()
}

pub fn normalize_tool(tool: &str, input: &Value, cwd: &str, table: &CommandTable) -> Vec<Normalized> {
    let full = |a: CanonicalAction, effect: Effect, mutating: bool| {
        vec![Normalized { action: a, effect, mutating, confidence: 1.0, note: String::new() }]
    };

    match tool {
        "Read" => full(
            CanonicalAction::Read { path: resolve_path(cwd, &s(input, "file_path")), range: None },
            Effect::Observe,
            false,
        ),
        "Grep" => full(
            CanonicalAction::Search {
                kind: "content".into(),
                pattern: s(input, "pattern"),
                paths: vec![resolve_path(cwd, &{
                    let p = s(input, "path");
                    if p.is_empty() { ".".to_string() } else { p }
                })],
            },
            Effect::Observe,
            false,
        ),
        "Glob" => full(
            CanonicalAction::Search {
                kind: "filename".into(),
                pattern: {
                    let p = s(input, "pattern");
                    if p.is_empty() { "*".to_string() } else { p }
                },
                paths: vec![resolve_path(cwd, ".")],
            },
            Effect::Observe,
            false,
        ),
        "LS" => full(CanonicalAction::List { path: resolve_path(cwd, ".") }, Effect::Observe, false),
        "Bash" => normalize_shell(&s(input, "command"), cwd, table),
        "Edit" | "MultiEdit" | "NotebookEdit" => full(
            CanonicalAction::Edit { path: resolve_path(cwd, &s(input, "file_path")) },
            Effect::Author,
            true,
        ),
        "Write" => full(
            CanonicalAction::Write { path: resolve_path(cwd, &s(input, "file_path")) },
            Effect::Author,
            true,
        ),
        "WebFetch" => full(CanonicalAction::Fetch { url: s(input, "url") }, Effect::Observe, false),
        "WebSearch" => full(
            CanonicalAction::Fetch { url: format!("search:{}", s(input, "query")) },
            Effect::Observe,
            false,
        ),
        "Skill" => full(
            CanonicalAction::Exec { argv: vec!["skill".into(), s(input, "skill")], cwd: cwd.to_string() },
            Effect::Environment,
            false,
        ),
        "Task" | "Agent" => full(
            CanonicalAction::Exec { argv: vec!["task".into(), s(input, "subagent_type")], cwd: cwd.to_string() },
            Effect::Environment,
            false,
        ),
        _ => {
            if let Some(rest) = tool.strip_prefix("mcp__") {
                return full(
                    CanonicalAction::Exec { argv: vec!["mcp".into(), rest.to_string()], cwd: cwd.to_string() },
                    Effect::Environment,
                    false,
                );
            }
            vec![Normalized {
                action: CanonicalAction::Unknown { raw: tool.to_string() },
                effect: Effect::Environment,
                mutating: false,
                confidence: 0.0,
                note: "no tool-table entry".to_string(),
            }]
        }
    }
}
