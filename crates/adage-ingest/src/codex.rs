//! Codex CLI session import, from rollout files
//! (`$CODEX_HOME/sessions/YYYY/MM/DD/rollout-<time>-<thread id>.jsonl`).
//!
//! The format is not a documented interface. It is read from openai/codex source at tag
//! rust-v0.156.1 (2026-09-23), with the older shapes checked at rust-v0.40.0; see
//! docs/harnesses.md. Each line is `{ timestamp, ordinal?, type, payload }`:
//!
//! | `type` | Used for |
//! | --- | --- |
//! | `session_meta` | session id, cwd, day; sub-agent threads (`source.subagent`, `parent_thread_id`) are skipped |
//! | `turn_context` | model, cwd |
//! | `event_msg` `user_message` | the prompt (legacy history mode, the default) |
//! | `response_item` `message` | the prompt (fallback); `developer` messages carry hook context, so Adage's directives |
//! | `response_item` `function_call` | `exec_command` (`{cmd, workdir}`), `shell_command`, `shell` (argv), `apply_patch` (`{input}`), others |
//! | `response_item` `custom_tool_call` | freeform `apply_patch` (`input` is the patch) |
//! | `response_item` `local_shell_call` | `action.command` argv |
//! | `response_item` `*_output` | the result, by `call_id`; the exit code from its header or JSON metadata |
//!
//! Unlike Codex's hooks, rollouts record exit codes (in the tool output the model reads), so dead
//! ends are learned from Codex history like any other. A model turn is a run of calls with no
//! output, message, or prompt between them. Compressed rollouts (`.jsonl.zst`, written only with
//! the `local_thread_store_compression` feature) are not read.

use crate::format::{Decode, Format};
use crate::locate::Located;
use crate::signals::{call_steps, Prompts, Signals};
use adage_core::tools::{from_codex, normalize_tool};
use adage_core::CommandTable;
use adage_trace::session::CODEX;
use adage_trace::{wire, Call, Session};
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, Read};
use std::path::{Path, PathBuf};

static EXIT_LINE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(?:Process exited with code|Exit code:) (-?\d+)$").unwrap());
static CONTEXT_TAG: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?s)^<([a-z_]+)(?:\s[^>]*)?>.*</([a-z_]+)>$").unwrap());

/// Lines longer than this are skipped (a rollout line is one item; outputs are truncated by Codex
/// long before this).
const MAX_LINE: usize = 4 << 20;
/// Calls kept per session.
const MAX_CALLS: usize = 4096;

fn s<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
    path.iter().try_fold(v, |v, k| v.get(*k))?.as_str()
}

/// Is this the first line of a rollout? `session_meta` is always written first.
pub fn is_rollout_head(v: &Value) -> bool {
    v.get("type").and_then(|t| t.as_str()) == Some("session_meta")
        && v.get("payload").is_some_and(|p| p.is_object())
        && v.get("timestamp").is_some()
}

/// Read a rollout's lines if `path` is one (recognised by its first line), else `None`.
pub fn read_rollout(path: &Path) -> Option<Vec<Value>> {
    read_lines(std::io::BufReader::new(std::fs::File::open(path).ok()?))
}

/// A rollout's lines from any reader, if it is one (recognised by its first line), else `None`.
pub fn read_lines(mut r: impl BufRead) -> Option<Vec<Value>> {
    let mut first = String::new();
    // The first line is small (session metadata); a huge or non-JSON first line is not a rollout.
    (&mut r).take(1 << 20).read_line(&mut first).ok()?;
    let head: Value = serde_json::from_str(first.trim_end()).ok()?;
    if !is_rollout_head(&head) {
        return None;
    }
    let mut lines = vec![head];
    for line in r.lines() {
        let Ok(line) = line else { break };
        if line.len() > MAX_LINE {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(&line) {
            lines.push(v);
        }
    }
    Some(lines)
}

/// Rollout files under a Codex sessions directory (`$CODEX_HOME/sessions`), sorted, and the number
/// of compressed rollouts (`.jsonl.zst`) that were skipped.
pub fn discover(dir: &Path) -> (Vec<PathBuf>, usize) {
    let mut out = Vec::new();
    let mut compressed = 0;
    let mut stack = vec![(dir.to_path_buf(), 0)];
    while let Some((d, depth)) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            match e.file_type() {
                // sessions/YYYY/MM/DD: never follow symlinks, never go deeper than the layout.
                Ok(t) if t.is_dir() && depth < 3 => stack.push((p, depth + 1)),
                Ok(t) if t.is_file() && name.starts_with("rollout-") => {
                    if name.ends_with(".jsonl") {
                        out.push(p);
                    } else if name.ends_with(".jsonl.zst") {
                        compressed += 1;
                    }
                }
                _ => {}
            }
        }
    }
    out.sort();
    (out, compressed)
}

/// Text of a message's content items, or of a tool output (a string, or content items).
fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(items) => {
            items.iter().filter_map(|i| i.get("text").and_then(|t| t.as_str())).collect::<Vec<_>>().join("\n")
        }
        _ => String::new(),
    }
}

/// Context Codex adds as user messages (`<environment_context>`, AGENTS.md, …): never the prompt.
fn contextual(t: &str) -> bool {
    let t = t.trim();
    if t.starts_with("# AGENTS.md instructions") {
        return true;
    }
    CONTEXT_TAG.captures(t).is_some_and(|c| c[1] == c[2])
}

/// Text Adage wrote (a directive, or a Stop-hook continuation: `render::is_finish_line`), never a
/// prompt the person typed.
fn own_text(t: &str) -> bool {
    t.trim_start().starts_with("<adage-reflex ") || wire::is_finish_line(t)
}

/// The shell command an argv runs: `["bash", "-lc", cmd]` → `cmd`; anything else is joined.
fn argv_command(argv: &[Value]) -> String {
    let a: Vec<&str> = argv.iter().filter_map(|x| x.as_str()).collect();
    let shell = |x: &str| matches!(x.rsplit('/').next().unwrap_or(""), "bash" | "sh" | "zsh" | "dash");
    if a.len() == 3 && shell(a[0]) && (a[1] == "-lc" || a[1] == "-c") {
        return a[2].to_string();
    }
    a.iter()
        .map(|w| {
            if !w.is_empty() && w.chars().all(|c| c.is_ascii_alphanumeric() || "-_./=:,@+".contains(c)) {
                w.to_string()
            } else {
                format!("'{}'", w.replace('\'', r"'\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A call as recorded: the canonical tool pairs it stands for, and the directory it ran in.
struct Recorded {
    turn: u32,
    /// The prompt it served, counted in `user_message` events and in user messages.
    prompt: (u16, u16),
    pairs: Vec<(String, Value)>,
    cwd: Option<String>,
}

/// Canonical `(tool, input)` pairs for a Codex call.
fn translate(name: &str, args: &Value, input: Option<&str>) -> (Vec<(String, Value)>, Option<String>) {
    let workdir = s(args, &["workdir"]).map(str::to_string);
    let patch = |p: &str| from_codex("apply_patch", &json!({ "command": p }));
    let shell = |cmd: String| -> Vec<(String, Value)> {
        // `apply_patch` run through the shell (older models did this).
        if cmd.trim_start().starts_with("apply_patch") && cmd.contains("*** Begin Patch") {
            return patch(&cmd);
        }
        vec![("Bash".to_string(), json!({ "command": cmd }))]
    };
    let pairs = match name {
        "exec_command" | "shell_command" => {
            shell(s(args, &["cmd"]).or_else(|| s(args, &["command"])).unwrap_or("").to_string())
        }
        "shell" | "container.exec" | "local_shell" => match args.get("command") {
            Some(Value::Array(argv)) if argv.first().and_then(|x| x.as_str()) == Some("apply_patch") => {
                patch(argv.get(1).and_then(|x| x.as_str()).unwrap_or(""))
            }
            Some(Value::Array(argv)) => shell(argv_command(argv)),
            Some(Value::String(c)) => shell(c.clone()),
            _ => vec![],
        },
        "apply_patch" => patch(input.or_else(|| s(args, &["input"])).or_else(|| s(args, &["patch"])).unwrap_or("")),
        "update_plan" => vec![("TodoWrite".to_string(), Value::Null)],
        "view_image" => vec![("Read".to_string(), json!({ "file_path": s(args, &["path"]).unwrap_or("") }))],
        // MCP tools already carry `mcp__server__tool` names; others normalise as unknown.
        other => vec![(other.to_string(), args.clone())],
    };
    (pairs, workdir)
}

/// The program output and whether the command failed, from a tool output's text: the unified
/// exec header (`Process exited with code N` … `Output:`), the older `Exit code: N` header, or the
/// older JSON `{output, metadata: {exit_code}}`.
fn exec_result(text: &str) -> (String, bool) {
    if text.starts_with('{') {
        if let Ok(v) = serde_json::from_str::<Value>(text) {
            if let Some(code) = v.pointer("/metadata/exit_code").and_then(|c| c.as_i64()) {
                return (s(&v, &["output"]).unwrap_or("").to_string(), code != 0);
            }
        }
    }
    let mut failed = false;
    for (i, line) in text.lines().enumerate().take(8) {
        if let Some(c) = EXIT_LINE.captures(line) {
            failed = c[1].parse::<i64>().is_ok_and(|c| c != 0);
        }
        if line == "Output:" {
            let body: String = text.lines().skip(i + 1).collect::<Vec<_>>().join("\n");
            return (body, failed);
        }
    }
    (text.to_string(), failed || text.starts_with("apply_patch verification failed"))
}

/// Directives Adage delivered (hook context, or its finish-line continuation prompt).
fn note(text: &str, signals: &mut Signals, calls: usize) {
    signals.at(calls);
    signals.note(text);
}

/// Parse one rollout. `None` for sub-agent threads and sessions without a usable prompt.
pub fn parse_rollout(lines: &[Value], table: &CommandTable, repo_override: Option<&str>) -> Option<Session> {
    decode(lines, &Decode::local(table, repo_override))
}

/// Codex rollouts as a [`crate::format::Format`]: recognised by a `session_meta` first line.
pub struct Rollout;

impl Format for Rollout {
    fn harness(&self) -> &'static str {
        CODEX
    }

    fn sniff(&self, head: &[u8]) -> bool {
        let first = head.split(|b| *b == b'\n').next().unwrap_or_default();
        std::str::from_utf8(first)
            .ok()
            .and_then(|l| serde_json::from_str::<Value>(l.trim_end()).ok())
            .is_some_and(|v| is_rollout_head(&v))
    }

    fn decode(&self, trace: &mut dyn BufRead, cx: &Decode<'_>) -> Option<Session> {
        decode(&read_lines(trace)?, cx)
    }
}

/// Decode one rollout's lines. `None` for sub-agent threads and sessions without a usable prompt.
/// The repository is found by `cx.locator`; nothing else here touches the filesystem.
pub fn decode(lines: &[Value], cx: &Decode<'_>) -> Option<Session> {
    let (table, repo_override) = (cx.table, cx.repo_override);
    let meta = lines.first().filter(|l| is_rollout_head(l))?.get("payload")?;
    if meta.get("parent_thread_id").is_some_and(|p| !p.is_null()) || meta.pointer("/source/subagent").is_some() {
        return None;
    }
    let id = s(meta, &["id"]).or_else(|| s(meta, &["session_id"]))?.to_string();
    let mut cwd: Option<String> = s(meta, &["cwd"]).map(str::to_string);
    let day: String =
        s(meta, &["timestamp"]).or_else(|| s(&lines[0], &["timestamp"])).unwrap_or("").chars().take(10).collect();
    let mut model: Option<String> = None;
    let mut event_prompt: Option<String> = None;
    let mut item_prompt: Option<String> = None;
    // Prompts the person typed: counted from `user_message` events when present, else from user
    // messages (the same precedence as the first prompt).
    let (mut event_prompts, mut item_prompts) = (Prompts::default(), Prompts::default());
    let mut signals = Signals::default();
    let mut calls: Vec<(String, Recorded)> = Vec::new();
    let mut outputs: HashMap<String, String> = HashMap::new();
    let mut turns = 0u32;
    let mut in_calls = false;
    for line in &lines[1..] {
        let p = line.get("payload").unwrap_or(&Value::Null);
        match (s(line, &["type"]), s(p, &["type"])) {
            (Some("turn_context"), _) => {
                if model.is_none() {
                    model = s(p, &["model"]).map(str::to_string);
                }
                if cwd.is_none() {
                    cwd = s(p, &["cwd"]).map(str::to_string);
                }
            }
            (Some("event_msg"), Some("user_message")) => {
                let t = s(p, &["message"]).unwrap_or("");
                in_calls = false;
                note(t, &mut signals, calls.len());
                if !t.trim().is_empty() && !own_text(t) {
                    event_prompts.push(t);
                    if event_prompt.is_none() {
                        event_prompt = Some(t.to_string());
                    }
                }
            }
            (Some("response_item"), Some(kind)) => {
                let call_id = s(p, &["call_id"]).or_else(|| s(p, &["id"])).unwrap_or("").to_string();
                let prompt = (event_prompts.current(), item_prompts.current());
                let mut call = |pairs_cwd: (Vec<(String, Value)>, Option<String>)| {
                    if !in_calls {
                        turns += 1;
                        in_calls = true;
                    }
                    if calls.len() < MAX_CALLS {
                        calls.push((
                            call_id.clone(),
                            Recorded { turn: turns - 1, prompt, pairs: pairs_cwd.0, cwd: pairs_cwd.1 },
                        ));
                    }
                };
                match kind {
                    "message" => {
                        let t = text_of(p.get("content").unwrap_or(&Value::Null));
                        match s(p, &["role"]) {
                            Some("user") => {
                                in_calls = false;
                                note(&t, &mut signals, calls.len());
                                if !t.trim().is_empty() && !own_text(&t) && !contextual(&t) {
                                    item_prompts.push(&t);
                                    if item_prompt.is_none() {
                                        item_prompt = Some(t);
                                    }
                                }
                            }
                            // Hook `additionalContext` is recorded as a developer message.
                            Some("developer") => note(&t, &mut signals, calls.len()),
                            _ => in_calls = false,
                        }
                    }
                    "function_call" => {
                        let name = s(p, &["name"]).unwrap_or("");
                        let args: Value =
                            s(p, &["arguments"]).and_then(|a| serde_json::from_str(a).ok()).unwrap_or(Value::Null);
                        call(translate(name, &args, None));
                    }
                    "custom_tool_call" => {
                        call(translate(s(p, &["name"]).unwrap_or(""), &Value::Null, s(p, &["input"])))
                    }
                    "local_shell_call" => {
                        let args = json!({ "command": p.pointer("/action/command").cloned().unwrap_or(Value::Null), "workdir": p.pointer("/action/working_directory").cloned().unwrap_or(Value::Null) });
                        call(translate("local_shell", &args, None));
                    }
                    "function_call_output" | "custom_tool_call_output" => {
                        in_calls = false;
                        if outputs.len() < MAX_CALLS {
                            outputs.insert(call_id, text_of(p.get("output").unwrap_or(&Value::Null)));
                        }
                    }
                    "reasoning" => {}
                    _ => in_calls = false,
                }
            }
            _ => {}
        }
    }

    let cwd = cwd?;
    let prompt = event_prompt.or(item_prompt)?;
    let Located { root, repo: repo_key } = cx.locator.locate(Path::new(&cwd), repo_override);
    let root_str = root.to_string_lossy().to_string();
    // Prompts from `user_message` events when there are any, else from user messages (the same
    // precedence as the first prompt).
    let by_event = !event_prompts.is_empty();
    let mut steps = Vec::new();
    let mut first_step: Vec<u32> = Vec::with_capacity(calls.len());
    for (call_id, rec) in calls {
        first_step.push(steps.len() as u32);
        // A call with no recorded output never finished (interrupted, or the file was cut off).
        let Some(raw) = outputs.get(&call_id) else { continue };
        let (output, failed) = exec_result(raw);
        let run_in = rec.cwd.filter(|w| w.starts_with('/')).unwrap_or_else(|| cwd.clone());
        for (tool, mut input) in rec.pairs {
            if let Some(p) = input.get("file_path").and_then(|v| v.as_str()) {
                if !p.is_empty() && !p.starts_with('/') {
                    input["file_path"] = Value::String(adage_core::shell::resolve_path(&run_in, p));
                }
            }
            let call = Call { tool: tool.clone(), input: input.clone(), output: output.clone(), failed };
            let at = if tool == "Bash" { run_in.as_str() } else { root_str.as_str() };
            let prompt = if by_event { rec.prompt.0 } else { rec.prompt.1 };
            steps.extend(call_steps(rec.turn, prompt, &call, normalize_tool(&tool, &input, at, table), &root));
        }
    }
    let prompts = if by_event { event_prompts } else { item_prompts }.anchor(&steps);
    let influence = signals.anchor(&first_step, steps.len());
    Some(Session {
        id,
        root,
        repo: repo_key,
        model,
        day,
        prompt,
        steps,
        steered: signals.steered,
        directed: signals.directed,
        influence,
        outcome: None,
        harness: CODEX.into(),
        prompts,
        ending: Default::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_from_every_output_shape() {
        let unified = "Chunk ID: 1a2b3c\nWall time: 0.0500 seconds\nProcess exited with code 2\nOriginal token count: 3\nOutput:\nboom\nmore";
        assert_eq!(exec_result(unified), ("boom\nmore".to_string(), true));
        assert_eq!(exec_result("Exit code: 0\nWall time: 0.1 seconds\nOutput:\nok"), ("ok".to_string(), false));
        assert_eq!(
            exec_result(r#"{"output":"No module named pytest\n","metadata":{"exit_code":1,"duration_seconds":0.1}}"#),
            ("No module named pytest\n".to_string(), true)
        );
        assert!(!exec_result("Success. Updated the following files:\nM a.json\n").1);
        assert!(exec_result("apply_patch verification failed: no such file").1);
        assert!(
            !exec_result("Wall time: 1.0 seconds\nProcess running with session ID 7\nOutput:\n").1,
            "still running: unknown, not failed"
        );
    }

    #[test]
    fn argv_and_context() {
        assert_eq!(argv_command(&[json!("bash"), json!("-lc"), json!("make test")]), "make test");
        assert_eq!(argv_command(&[json!("/bin/zsh"), json!("-c"), json!("ls -la")]), "ls -la");
        assert_eq!(argv_command(&[json!("rg"), json!("-n"), json!("a b")]), "rg -n 'a b'");
        assert!(contextual("<environment_context>\n  <cwd>/x</cwd>\n</environment_context>"));
        assert!(contextual("# AGENTS.md instructions for /x\n\n<INSTRUCTIONS>…</INSTRUCTIONS>"));
        assert!(!contextual("<b>Fix</b> the bug in W-1"));
        assert!(!contextual("Pick up W-1041"));
    }
}
