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
                    if p.is_empty() {
                        ".".to_string()
                    } else {
                        p
                    }
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
                    if p.is_empty() {
                        "*".to_string()
                    } else {
                        p
                    }
                },
                paths: vec![resolve_path(cwd, ".")],
            },
            Effect::Observe,
            false,
        ),
        "LS" => full(CanonicalAction::List { path: resolve_path(cwd, ".") }, Effect::Observe, false),
        "Bash" => normalize_shell(&s(input, "command"), cwd, table),
        "Edit" | "MultiEdit" | "NotebookEdit" => {
            full(CanonicalAction::Edit { path: resolve_path(cwd, &s(input, "file_path")) }, Effect::Author, true)
        }
        "Write" => {
            full(CanonicalAction::Write { path: resolve_path(cwd, &s(input, "file_path")) }, Effect::Author, true)
        }
        "WebFetch" => full(CanonicalAction::Fetch { url: s(input, "url") }, Effect::Observe, false),
        "WebSearch" => {
            full(CanonicalAction::Fetch { url: format!("search:{}", s(input, "query")) }, Effect::Observe, false)
        }
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
        "ToolSearch" | "SendMessage" | "AskUserQuestion" | "TodoWrite" | "TaskCreate" | "TaskUpdate" | "TaskList"
        | "TaskStop" | "Monitor" | "ExitPlanMode" | "EnterPlanMode" | "SendUserFile" | "ReadNotifications"
        | "Artifact" | "ScheduleWakeup" | "CronCreate" | "CronDelete" | "CronList" => {
            full(CanonicalAction::Control { tool: tool.to_string() }, Effect::Observe, false)
        }
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

// ---------------------------------------------------------------------------------------------
// Other harnesses' native tools, translated into the vocabulary above (Architecture §4.11). Each
// native call becomes zero or more `(tool, input)` pairs that `normalize_tool` understands, so the
// runtime and the learner see one canonical stream whatever the harness. See docs/harnesses.md.

const MAX_PATCH_FILES: usize = 64;

/// Files a Codex-format patch (`*** Begin Patch` … `*** End Patch`) touches, as `(tool, path)`:
/// `Write` for an added file or a move target, `Edit` for an updated or deleted file. Codex's and
/// OpenCode's `apply_patch` tools share the format.
pub fn patch_files(patch: &str) -> Vec<(&'static str, String)> {
    const MARKERS: [(&str, &str); 4] = [
        ("*** Add File: ", "Write"),
        ("*** Update File: ", "Edit"),
        ("*** Delete File: ", "Edit"),
        ("*** Move to: ", "Write"),
    ];
    let mut out: Vec<(&'static str, String)> = Vec::new();
    for line in patch.lines() {
        let l = line.trim_end();
        let hit =
            MARKERS.iter().find_map(|(marker, tool)| l.strip_prefix(marker).map(|p| (*tool, p.trim().to_string())));
        if let Some((tool, p)) = hit {
            if !p.is_empty() && p.len() <= 4096 && !out.iter().any(|(t, q)| *t == tool && *q == p) {
                out.push((tool, p));
            }
        }
        if out.len() >= MAX_PATCH_FILES {
            break;
        }
    }
    out
}

fn patch_calls(patch: &str) -> Vec<(String, Value)> {
    patch_files(patch).into_iter().map(|(t, p)| (t.to_string(), serde_json::json!({ "file_path": p }))).collect()
}

fn str_of<'a>(v: &'a Value, keys: &[&str]) -> &'a str {
    keys.iter().find_map(|k| v.get(*k).and_then(|x| x.as_str())).unwrap_or("")
}

/// An OpenCode tool call (`tool` and `args` as in `tool.execute.after`, or an exported `ToolPart`'s
/// `tool` and `state.input`). Names from opencode.ai/docs/tools; argument names from the tool
/// sources (`filePath`, `patchText`).
pub fn from_opencode(tool: &str, args: &Value) -> Vec<(String, Value)> {
    use serde_json::json;
    let one = |t: &str, v: Value| vec![(t.to_string(), v)];
    let file = || json!({ "file_path": str_of(args, &["filePath", "file_path"]) });
    match tool {
        "bash" | "shell" => one("Bash", json!({ "command": str_of(args, &["command"]) })),
        "read" => one("Read", file()),
        "edit" | "multiedit" => one("Edit", file()),
        "write" => one("Write", file()),
        "grep" => one("Grep", json!({ "pattern": str_of(args, &["pattern"]), "path": str_of(args, &["path"]) })),
        "glob" => one("Glob", json!({ "pattern": str_of(args, &["pattern"]) })),
        "list" | "ls" => one("LS", json!({})),
        "apply_patch" | "patch" => patch_calls(str_of(args, &["patchText", "patch"])),
        "webfetch" => one("WebFetch", json!({ "url": str_of(args, &["url"]) })),
        "websearch" => one("WebSearch", json!({ "query": str_of(args, &["query"]) })),
        "task" => one("Task", json!({ "subagent_type": str_of(args, &["subagent_type", "subagentType"]) })),
        "skill" => one("Skill", json!({ "skill": str_of(args, &["name", "skill"]) })),
        "todowrite" | "todoread" => one("TodoWrite", Value::Null),
        "question" => one("AskUserQuestion", Value::Null),
        // MCP and plugin tools keep their native name and normalise as unknown.
        other => one(other, args.clone()),
    }
}

/// A Codex hook tool call. `Bash` and `mcp__…` already use Claude Code's names; `apply_patch`
/// carries the patch text in `tool_input.command` (developers.openai.com/codex/hooks).
pub fn from_codex(tool: &str, input: &Value) -> Vec<(String, Value)> {
    match tool {
        "apply_patch" => patch_calls(str_of(input, &["command", "patch"])),
        _ => vec![(tool.to_string(), input.clone())],
    }
}

#[cfg(test)]
mod harness_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn patches_name_their_files() {
        let p = "*** Begin Patch\n*** Update File: src/a.rs\n@@\n-x\n+y\n*** Add File: schema/b.json\n+{}\n*** Delete File: old.txt\n*** Update File: src/c.rs\n*** Move to: src/d.rs\n*** End Patch";
        let want: Vec<(&str, String)> = vec![
            ("Edit", "src/a.rs".into()),
            ("Write", "schema/b.json".into()),
            ("Edit", "old.txt".into()),
            ("Edit", "src/c.rs".into()),
            ("Write", "src/d.rs".into()),
        ];
        assert_eq!(patch_files(p), want);
        assert!(patch_files("no markers here").is_empty());
        let many: String = (0..500).map(|i| format!("*** Add File: f{i}\n")).collect();
        assert_eq!(patch_files(&many).len(), MAX_PATCH_FILES);
    }

    #[test]
    fn opencode_tools_map_to_the_canonical_vocabulary() {
        assert_eq!(
            from_opencode("bash", &json!({ "command": "make test" })),
            vec![("Bash".to_string(), json!({ "command": "make test" }))]
        );
        assert_eq!(
            from_opencode("read", &json!({ "filePath": "/r/a.py" })),
            vec![("Read".to_string(), json!({ "file_path": "/r/a.py" }))]
        );
        assert_eq!(from_opencode("apply_patch", &json!({ "patchText": "*** Add File: x.json\n" }))[0].0, "Write");
        assert_eq!(
            from_opencode("bash", &json!(42)),
            vec![("Bash".to_string(), json!({ "command": "" }))],
            "hostile args never panic"
        );
    }

    #[test]
    fn codex_apply_patch_is_split_per_file() {
        let calls = from_codex(
            "apply_patch",
            &json!({ "command": "*** Begin Patch\n*** Update File: schema/a.json\n*** End Patch" }),
        );
        assert_eq!(calls, vec![("Edit".to_string(), json!({ "file_path": "schema/a.json" }))]);
        assert_eq!(from_codex("Bash", &json!({ "command": "ls" }))[0].0, "Bash");
    }
}
