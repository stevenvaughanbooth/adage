//! Codex CLI session import, from rollout files
//! (`$CODEX_HOME/sessions/YYYY/MM/DD/rollout-<time>-<thread id>.jsonl`).
//!
//! The format is not a documented interface. It is read from openai/codex source at tag
//! rust-v0.156.1 (2026-09-23), with the older shapes checked at rust-v0.40.0; see
//! docs/harnesses.md. Each line is `{ timestamp, ordinal?, type, payload }`:
//!
//! | `type` | Used for |
//! | --- | --- |
//! | `session_meta` | session id, cwd, day; task sub-agents require a native own-history ordinal boundary |
//! | `turn_context` | model, cwd |
//! | `event_msg` `user_message` | the prompt (legacy history mode, the default) |
//! | `response_item` `message` | the prompt (fallback); `developer` messages carry hook context, so Adage's directives |
//! | `response_item` `function_call` | `exec_command` (`{cmd, workdir}`), `shell_command`, `shell` (argv), `apply_patch` (`{input}`), others |
//! | `response_item` `custom_tool_call` | freeform `apply_patch`, or a bounded literal single-command `exec` forwarder |
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
use adage_trace::{wire, By, Call, Session};
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
    /// The native call translated to no known actions and must remain an opaque boundary.
    opaque: bool,
    /// Only native custom exec calls are eligible for literal wrapper decoding.
    wrapper: bool,
    position: usize,
}

struct Output {
    text: String,
    native: Value,
    occurrences: usize,
    custom: bool,
    position: usize,
}

/// Canonical `(tool, input)` pairs for a Codex call.
fn translate(name: &str, args: &Value, input: Option<&str>) -> (Vec<(String, Value)>, Option<String>, bool) {
    let workdir = s(args, &["workdir"]).map(str::to_string);
    let patch = |p: &str| from_codex("apply_patch", &json!({ "command": p }));
    let shell = |cmd: String| -> Vec<(String, Value)> {
        // `apply_patch` run through the shell (older models did this).
        if cmd.trim_start().starts_with("apply_patch") && cmd.contains("*** Begin Patch") {
            return patch(&cmd);
        }
        vec![("Bash".to_string(), json!({ "command": cmd }))]
    };
    let mut pairs = match name {
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
        other => vec![(other.to_string(), input.map(|text| json!({ "input": text })).unwrap_or_else(|| args.clone()))],
    };
    let opaque = pairs.is_empty();
    if opaque {
        // A markerless patch or malformed shell payload is still a recorded native call.
        // Keep its original name/input rather than inventing a shell command or silently
        // making its neighbors adjacent. The decoder supplies an Unknown action below.
        pairs.push((name.to_string(), input.map(|raw| json!({ "input": raw })).unwrap_or_else(|| args.clone())));
    }
    (pairs, workdir, opaque)
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

/// Parse one rollout. Unsupported child kinds and ambiguous copied-history boundaries abstain.
pub fn parse_rollout(lines: &[Value], table: &CommandTable, repo_override: Option<&str>) -> Option<Session> {
    decode(lines, &Decode::local(table, repo_override))
}

/// Codex rollouts as a [`crate::format::Format`]: recognised by a `session_meta` first line.
pub struct Rollout;

impl Format for Rollout {
    fn harness(&self) -> adage_trace::Harness {
        adage_trace::Harness::Codex
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

struct Child<'a> {
    parent: &'a str,
    path: Option<&'a str>,
    start: usize,
}

/// The source's ordinal, never a timestamp or a guessed first prompt, separates inherited
/// context from the child's own actions. Special workers (for example approval reviewers) are
/// not coding-task demonstrations. Missing or contradictory provenance is not repaired by guess.
fn child<'a>(lines: &'a [Value], meta: &'a Value) -> Option<Option<Child<'a>>> {
    let source = meta.pointer("/source/subagent");
    let parent = meta.get("parent_thread_id").filter(|p| !p.is_null());
    if source.is_none() && parent.is_none() {
        return Some(None);
    }
    let spawn = source?.get("thread_spawn")?.as_object()?;
    if meta.get("thread_source").is_some_and(|s| s.as_str() != Some("subagent")) {
        return None;
    }
    let parent_id = spawn.get("parent_thread_id")?.as_str()?;
    let valid_id =
        |id: &str| !id.is_empty() && id.len() <= 128 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    let own_id = s(meta, &["id"]).or_else(|| s(meta, &["session_id"]))?;
    if !valid_id(parent_id)
        || !valid_id(own_id)
        || parent.is_some_and(|p| p.as_str() != Some(parent_id))
        || own_id == parent_id
    {
        return None;
    }
    let boundary = meta.get("subagent_history_start_ordinal")?.as_u64()?;
    if boundary == 0 {
        return None;
    }
    let mut previous = lines.first()?.get("ordinal")?.as_u64()?;
    let mut start = None;
    for (i, line) in lines.iter().enumerate().skip(1) {
        let ordinal = line.get("ordinal")?.as_u64()?;
        if ordinal <= previous {
            return None;
        }
        previous = ordinal;
        if ordinal == boundary {
            start = Some(i);
        }
        if ordinal >= boundary && line.get("type").and_then(Value::as_str) == Some("session_meta") {
            return None;
        }
    }
    Some(Some(Child { parent: parent_id, path: spawn.get("agent_path").and_then(Value::as_str), start: start? }))
}

/// Decode one rollout's lines. Task children may have an unknown (encrypted) assignment; their
/// own completed calls are still retained, without treating the copied parent task as theirs.
/// The repository is found by `cx.locator`; nothing else here touches the filesystem.
pub fn decode(lines: &[Value], cx: &Decode<'_>) -> Option<Session> {
    let (table, repo_override) = (cx.table, cx.repo_override);
    let meta = lines.first().filter(|l| is_rollout_head(l))?.get("payload")?;
    let child = child(lines, meta)?;
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
    let mut outputs: HashMap<String, Output> = HashMap::new();
    let mut requests: HashMap<String, usize> = HashMap::new();
    let mut turns = 0u32;
    let mut in_calls = false;
    let start = child.as_ref().map_or(1, |c| c.start);
    // Known inherited directives continue to apply. Copied actions and task prompts never become
    // fresh evidence; there is no parent-tool sequence in the child's canonical session.
    for line in &lines[1..start] {
        let p = line.get("payload").unwrap_or(&Value::Null);
        if s(line, &["type"]) == Some("response_item") && s(p, &["type"]) == Some("message") {
            let t = text_of(p.get("content").unwrap_or(&Value::Null));
            if s(p, &["role"]) == Some("developer") || own_text(&t) {
                note(&t, &mut signals, 0);
            }
        }
    }
    for (position, line) in lines[start..].iter().enumerate() {
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
                let mut call = |pairs_cwd: (Vec<(String, Value)>, Option<String>, bool)| {
                    if !in_calls {
                        turns += 1;
                        in_calls = true;
                    }
                    if calls.len() < MAX_CALLS {
                        *requests.entry(call_id.clone()).or_default() += 1;
                        calls.push((
                            call_id.clone(),
                            Recorded {
                                turn: turns - 1,
                                prompt,
                                pairs: pairs_cwd.0,
                                cwd: pairs_cwd.1,
                                opaque: pairs_cwd.2,
                                wrapper: kind == "custom_tool_call"
                                    && matches!(s(p, &["name"]), Some("exec" | "functions.exec")),
                                position,
                            },
                        ));
                    } else if let Some(count) = requests.get_mut(&call_id) {
                        *count += 1;
                    }
                };
                match kind {
                    "agent_message" => {
                        // A delegated message is not a new person-authored task. Its plaintext
                        // envelope is especially not the assignment when content is encrypted.
                        // Preserve receipt boundaries without manufacturing prompt evidence.
                        if child.as_ref().is_some_and(|c| c.path == s(p, &["recipient"])) {
                            in_calls = false;
                        }
                    }
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
                        let args: Value = s(p, &["arguments"])
                            .map(|a| serde_json::from_str(a).unwrap_or_else(|_| Value::String(a.to_string())))
                            .unwrap_or(Value::Null);
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
                        if outputs.len() < MAX_CALLS || outputs.contains_key(&call_id) {
                            let occurrences = outputs.get(&call_id).map_or(1, |o| o.occurrences + 1);
                            outputs.insert(
                                call_id,
                                Output {
                                    text: text_of(p.get("output").unwrap_or(&Value::Null)),
                                    native: p.get("output").cloned().unwrap_or(Value::Null),
                                    occurrences,
                                    custom: kind == "custom_tool_call_output",
                                    position,
                                },
                            );
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
    let prompt = event_prompt.or(item_prompt).or_else(|| child.as_ref().map(|_| String::new()))?;
    let Located { root, repo: repo_key } = cx.locator.locate(Path::new(&cwd), repo_override);
    let root_str = root.to_string_lossy().to_string();
    // Prompts from `user_message` events when there are any, else from user messages (the same
    // precedence as the first prompt).
    let by_event = !event_prompts.is_empty();
    let mut steps = Vec::new();
    let mut first_step: Vec<u32> = Vec::with_capacity(calls.len());
    for (call_id, mut rec) in calls {
        first_step.push(steps.len() as u32);
        // A call with no recorded output never finished (interrupted, or the file was cut off).
        let Some(raw) = outputs.get(&call_id) else { continue };
        let forwarded = if rec.wrapper
            && !call_id.is_empty()
            && requests.get(&call_id) == Some(&1)
            && raw.occurrences == 1
            && raw.custom
            && raw.position > rec.position
        {
            rec.pairs
                .first()
                .and_then(|(tool, input)| crate::codex_exec::decode(tool, input.get("input")?.as_str()?, &raw.native))
        } else {
            None
        };
        let unwrapped = forwarded.is_some();
        let (output, failed) = if let Some(call) = forwarded {
            rec.pairs = vec![("Bash".to_string(), call.input)];
            rec.cwd = Some(call.cwd);
            // The printed body is data. Never parse it again as a transport exit-code header.
            (call.output, call.failed)
        } else {
            exec_result(&raw.text)
        };
        let run_in = rec.cwd.filter(|w| w.starts_with('/')).unwrap_or_else(|| cwd.clone());
        for (tool, mut input) in rec.pairs {
            if let Some(p) = input.get("file_path").and_then(|v| v.as_str()) {
                if !p.is_empty() && !p.starts_with('/') {
                    input["file_path"] = Value::String(adage_core::shell::resolve_path(&run_in, p));
                }
            }
            let call = Call {
                tool: tool.clone(),
                input: input.clone(),
                output: output.clone(),
                failed,
                by: By::Agent,
                tool_use_id: ((child.is_some() || rec.opaque || unwrapped) && !call_id.is_empty())
                    .then(|| call_id.clone()),
                agent: child.as_ref().map(|_| id.clone()),
            };
            let at = if tool == "Bash" { run_in.as_str() } else { root_str.as_str() };
            let prompt = if by_event { rec.prompt.0 } else { rec.prompt.1 };
            let actions = if rec.opaque { Vec::new() } else { normalize_tool(&tool, &input, at, table) };
            steps.extend(call_steps(rec.turn, prompt, &call, actions, &root));
        }
    }
    let prompts = if by_event { event_prompts } else { item_prompts }.anchor(&steps);
    let influence = signals.anchor(&first_step, steps.len());
    Some(Session {
        id: id.clone(),
        delegation: child.as_ref().map(|c| adage_trace::session::Delegation {
            parent_id: c.parent.to_string(),
            agent_id: id.clone(),
            task: Some(prompt.clone()),
            instructions: None,
        }),
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
        identity: child.as_ref().map_or_else(Default::default, |c| adage_trace::EvidenceIdentity {
            group_id: Some(format!("codex-parent:{}", c.parent)),
            attempt_id: Some(id.clone()),
            ..Default::default()
        }),
        ending: Default::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn child_lines() -> Vec<Value> {
        let mut rows = vec![
            json!({"type":"session_meta","payload":{"id":"child","cwd":"/repo","parent_thread_id":"parent",
                "thread_source":"subagent","subagent_history_start_ordinal":5,
                "source":{"subagent":{"thread_spawn":{"parent_thread_id":"parent","agent_path":"/root/worker"}}}}}),
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"The copied parent task"}]}}),
            json!({"type":"response_item","payload":{"type":"function_call","name":"exec_command","call_id":"parent-call","arguments":"{\"cmd\":\"make parent-check\"}"}}),
            json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"parent-call","output":"Exit code: 0\nOutput:\nparent"}}),
            json!({"type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"<adage-reflex track=\"t@1\" route=\"check\" strength=\"fire\" kind=\"run\">make child-check</adage-reflex>"}]}}),
            json!({"type":"event_msg","payload":{"type":"task_started"}}),
            json!({"type":"response_item","payload":{"type":"agent_message","author":"/root","recipient":"/root/worker","content":[{"type":"input_text","text":"Message Type: NEW_TASK\nPayload:\n"},{"type":"encrypted_content","encrypted_content":"opaque"}]}}),
            json!({"type":"response_item","payload":{"type":"function_call","name":"exec_command","call_id":"own-call","arguments":"{\"cmd\":\"make child-check\"}"}}),
            json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"own-call","output":"Exit code: 0\nOutput:\nchild"}}),
        ];
        for (i, row) in rows.iter_mut().enumerate() {
            row["ordinal"] = json!(i);
            row["timestamp"] = json!("2026-10-15T00:00:00Z");
        }
        rows
    }

    fn recorded(lines: &[Value]) -> Option<Session> {
        decode(
            lines,
            &Decode {
                table: CommandTable::load(),
                locator: &crate::AsRecorded,
                repo_override: Some("r"),
                subagent: false,
            },
        )
    }

    #[test]
    fn child_keeps_only_own_calls_without_inventing_an_encrypted_task() {
        let s = recorded(&child_lines()).unwrap();
        assert_eq!(s.id, "child");
        assert!(s.prompt.is_empty() && s.prompts.is_empty());
        assert_eq!(s.steps.len(), 1);
        assert_eq!(s.steps[0].key, "exec make child-check");
        assert_eq!(s.steps[0].call.tool_use_id.as_deref(), Some("own-call"));
        assert_eq!(s.steps[0].call.agent.as_deref(), Some("child"));
        assert_eq!(s.identity.group_id.as_deref(), Some("codex-parent:parent"));
        assert!(s.steered && s.influence.iter().any(|d| d.at_step == 0), "inherited known steering still applies");
    }

    #[test]
    fn opaque_custom_calls_keep_input_but_are_not_shell_execution_evidence() {
        let mut rows = child_lines();
        rows[7]["payload"] = json!({"type":"custom_tool_call","name":"exec","call_id":"own-call","input":"const result = await tools.exec_command({cmd: 'make child-check'}); text(result);"});
        rows[8]["payload"]["type"] = json!("custom_tool_call_output");
        let s = recorded(&rows).unwrap();
        assert_eq!(s.steps.len(), 1);
        assert_eq!(s.steps[0].call.input["input"], rows[7]["payload"]["input"]);
        assert!(!matches!(s.steps[0].norm.action, adage_core::CanonicalAction::Exec { .. }));
    }

    fn wrapper_lines(command: &str, body: &str, exit: i32) -> Vec<Value> {
        let mut rows = child_lines();
        rows[7]["payload"] = json!({"type":"custom_tool_call","name":"exec","call_id":"own-call",
            "input":format!("text(await tools.exec_command({}));", json!({"cmd":command,"workdir":"/repo/child"}))});
        rows[8]["payload"] = json!({"type":"custom_tool_call_output","call_id":"own-call",
            "output":json!({"exit_code":exit,"output":body}).to_string()});
        rows
    }

    #[test]
    fn wrapper_preserves_child_provenance_cwd_and_authoritative_failure() {
        let body = "Exit code: 0\nOutput:\nNo module named pytest";
        let rows = wrapper_lines("python3.11 -m pytest", body, 1);
        let s = recorded(&rows).unwrap();
        assert_eq!(s.steps.len(), 1);
        let st = &s.steps[0];
        assert!(matches!(&st.norm.action, adage_core::CanonicalAction::Exec { cwd, .. } if cwd == "/repo/child"));
        assert_eq!(st.call.input["workdir"], "/repo/child");
        assert_eq!(st.call.input["native_wrapper"]["input"], rows[7]["payload"]["input"]);
        assert_eq!(st.call.input["native_wrapper"]["output"], rows[8]["payload"]["output"]);
        assert_eq!(st.call.output, body, "printed headers are body data, not transport metadata");
        assert!(st.call.failed);
        assert_eq!(st.call.tool_use_id.as_deref(), Some("own-call"));
        assert_eq!(st.call.agent.as_deref(), Some("child"));
        assert_eq!(s.identity.group_id.as_deref(), Some("codex-parent:parent"));
        assert!(s.prompt.is_empty() && s.steered);
    }

    #[test]
    fn native_two_block_wrapper_records_inner_exit_not_script_completion() {
        let mut rows = wrapper_lines("cargo test --offline", "Operation not permitted (os error 1)", 101);
        let envelope = rows[8]["payload"]["output"].clone();
        rows[8]["payload"]["output"] = json!([
            {"type":"input_text","text":"Script completed\nWall time 0.3 seconds\nOutput:\n"},
            {"type":"input_text","text":envelope},
        ]);
        let s = recorded(&rows).unwrap();
        let call = &s.steps[0].call;
        assert_eq!(call.tool, "Bash");
        assert!(call.failed);
        assert_eq!(call.output, "Operation not permitted (os error 1)");
        assert_eq!(call.input["native_wrapper"]["output"], rows[8]["payload"]["output"]);
        assert_eq!(call.tool_use_id.as_deref(), Some("own-call"));
    }

    #[test]
    fn wrapper_compounds_keep_one_native_call_and_shared_output() {
        let body = "1 passed in 0.1s";
        let s =
            recorded(&wrapper_lines("python -m pytest tests/a.py && python -m pytest tests/b.py", body, 0)).unwrap();
        assert_eq!(s.steps.len(), 2);
        assert!(s.steps[0].same_native_call(&s.steps[1]));
        for step in &s.steps {
            assert_eq!(step.call.output, body);
            let report = step.verification().unwrap();
            assert_eq!(report.status(), None, "one shared report cannot certify both test commands");
        }
    }

    #[test]
    fn ambiguous_native_wrapper_pairs_remain_opaque() {
        for mutation in 0..7 {
            let mut rows = wrapper_lines("cargo test", "test result: ok. 1 passed; 0 failed", 0);
            match mutation {
                0 => rows.push(rows[7].clone()), // reused request id
                1 => rows.push(rows[8].clone()), // repeated/streamed result
                2 => rows.swap(7, 8), // result precedes request
                3 => rows[8]["payload"]["type"] = json!("function_call_output"),
                4 => rows[8]["payload"]["output"] = json!([{"text":rows[8]["payload"]["output"]}]),
                5 => rows[7]["payload"]["input"] = json!("text(await tools.exec_command({cmd:\"cargo test\",workdir:\"/repo\",sandbox_permissions:\"require_escalated\"}));"),
                6 => {
                    rows[7]["payload"]["call_id"] = json!("");
                    rows[8]["payload"]["call_id"] = json!("");
                }
                _ => unreachable!(),
            }
            for (i, row) in rows.iter_mut().enumerate() {
                row["ordinal"] = json!(i);
            }
            let s = recorded(&rows).unwrap();
            assert!(!s.steps.is_empty());
            assert!(s.steps.iter().all(|st| st.norm.contains_unknown()), "mutation {mutation}");
            assert!(s.steps.iter().all(|st| st.call.input.get("native_wrapper").is_none()));
        }
    }

    #[test]
    fn root_wrapper_keeps_native_id_too() {
        let child = wrapper_lines("cargo test", "test result: ok. 1 passed; 0 failed", 0);
        let rows = vec![
            json!({"type":"session_meta","timestamp":"2026-10-15T00:00:00Z","payload":{"id":"main","cwd":"/repo"}}),
            json!({"type":"event_msg","payload":{"type":"user_message","message":"check the project"}}),
            child[7].clone(),
            child[8].clone(),
        ];
        let s = recorded(&rows).unwrap();
        assert_eq!(s.steps[0].call.tool, "Bash");
        assert_eq!(s.steps[0].call.tool_use_id.as_deref(), Some("own-call"));
        assert!(s.steps[0].call.agent.is_none() && s.delegation.is_none());
    }

    #[test]
    fn completed_zero_mapping_calls_keep_native_identity_and_uninterpreted_input() {
        let cases = [
            (
                json!({"type":"function_call","name":"apply_patch","call_id":"own-call",
                    "arguments":"{\"input\":\"not a patch\"}"}),
                json!({"input":"not a patch"}),
            ),
            (
                json!({"type":"custom_tool_call","name":"apply_patch","call_id":"own-call",
                    "input":"unrecognized patch syntax\nDo not execute this text"}),
                json!({"input":"unrecognized patch syntax\nDo not execute this text"}),
            ),
            (
                json!({"type":"function_call","name":"shell","call_id":"own-call",
                    "arguments":"{\"command\":42,\"workdir\":\"/repo\"}"}),
                json!({"command":42,"workdir":"/repo"}),
            ),
            (
                json!({"type":"function_call","name":"shell","call_id":"own-call",
                    "arguments":"{invalid json"}),
                json!("{invalid json"),
            ),
        ];
        for (payload, expected_input) in cases {
            for child in [false, true] {
                let mut rows = child_lines();
                rows[7]["payload"] = payload.clone();
                rows[8]["payload"] = json!({"type":if payload["type"] == "custom_tool_call" {
                    "custom_tool_call_output"
                } else {
                    "function_call_output"
                },"call_id":"own-call","output":"opaque result"});
                if !child {
                    rows = vec![
                        json!({"type":"session_meta","timestamp":"2026-10-15T00:00:00Z",
                            "payload":{"id":"main","cwd":"/repo"}}),
                        json!({"type":"event_msg","payload":{"type":"user_message","message":"inspect the project"}}),
                        rows[7].clone(),
                        rows[8].clone(),
                    ];
                }
                let s = recorded(&rows).unwrap();
                assert_eq!(s.steps.len(), 1, "completed call must not disappear: {payload}");
                let st = &s.steps[0];
                assert_eq!(st.call.tool, payload["name"].as_str().unwrap());
                assert_eq!(st.call.input, expected_input);
                assert_eq!(st.call.output, "opaque result");
                assert!(!st.call.failed);
                assert_eq!(st.call.tool_use_id.as_deref(), Some("own-call"));
                assert_eq!(st.call.agent.as_deref(), child.then_some("child"));
                assert_eq!(st.call.by, By::Agent);
                assert!(st.norm.contains_unknown());
                assert_eq!(st.norm.effect, adage_core::Effect::Environment);
                assert!(st.verification().is_none());
            }
        }
    }

    #[test]
    fn ambiguous_child_boundaries_and_special_workers_abstain() {
        let valid = child_lines();
        for mutation in 0..9 {
            let mut rows = valid.clone();
            match mutation {
                0 => {
                    rows[0]["payload"].as_object_mut().unwrap().remove("subagent_history_start_ordinal");
                }
                1 => rows[0]["payload"]["subagent_history_start_ordinal"] = json!(0),
                2 => rows[0]["payload"]["subagent_history_start_ordinal"] = json!(99),
                3 => {
                    rows[3].as_object_mut().unwrap().remove("ordinal");
                }
                4 => rows[3]["ordinal"] = json!(2),
                5 => rows[0]["payload"]["parent_thread_id"] = json!("different"),
                6 => rows[0]["payload"]["thread_source"] = json!("guardian_review"),
                7 => rows[0]["payload"]["source"]["subagent"] = json!({"guardian_review":{}}),
                8 => rows[6]["type"] = json!("session_meta"),
                _ => unreachable!(),
            }
            assert!(recorded(&rows).is_none(), "mutation {mutation}");
        }
    }

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
