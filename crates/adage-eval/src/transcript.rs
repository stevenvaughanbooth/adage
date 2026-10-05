//! A Claude Code transcript as the hooks saw it, event by event.
//!
//! `adage_learn::import` reads a transcript into flattened steps for mining. Replay needs more:
//! the prompt, then each model turn's tool calls grouped as Claude Code groups them for
//! `PostToolBatch`, each call's failure for `PostToolUseFailure`, and the tool response text the
//! hook payload carried (`toolUseResult`, which for `Read` has no text at all). This module keeps
//! that structure, and converts it into the runtime's [`Event`]s exactly as the Claude Code
//! adapter does (`crates/adage-hook/src/harness/claude.rs`).

use adage_runtime::{Call, Event};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub tool: String,
    pub input: Value,
    /// The tool result as the transcript records it (what the model saw).
    pub output: String,
    /// The tool response as a hook payload carries it (`tool_response` → text).
    pub hook_output: String,
    pub failed: bool,
}

#[derive(Debug, Clone)]
pub enum Item {
    Prompt(String),
    /// One model turn's tool calls, in order.
    Turn(Vec<ToolCall>),
}

#[derive(Debug, Clone)]
pub struct Transcript {
    pub id: String,
    pub cwd: String,
    pub items: Vec<Item>,
    /// The session received Adage directives (hook attachments carrying `<adage-reflex`).
    pub steered: bool,
    pub directed: BTreeSet<String>,
}

impl Transcript {
    pub fn prompt(&self) -> Option<&str> {
        self.items.iter().find_map(|i| match i {
            Item::Prompt(p) => Some(p.as_str()),
            _ => None,
        })
    }

    pub fn turns(&self) -> impl Iterator<Item = &Vec<ToolCall>> {
        self.items.iter().filter_map(|i| match i {
            Item::Turn(t) => Some(t),
            _ => None,
        })
    }
}

/// The text of a tool response, whatever its shape (mirrors `harness::claude::response_text`).
pub fn response_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Object(o) => {
            let mut s = String::new();
            for k in ["stdout", "stderr", "error", "content", "output"] {
                match o.get(k) {
                    Some(Value::String(x)) if !x.is_empty() => {
                        s.push_str(x);
                        s.push('\n');
                    }
                    Some(Value::Array(items)) => {
                        for it in items {
                            if let Some(t) = it.get("text").and_then(|t| t.as_str()) {
                                s.push_str(t);
                                s.push('\n');
                            }
                        }
                    }
                    _ => {}
                }
            }
            s
        }
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn text_of(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|i| i.get("text").and_then(|t| t.as_str()).map(str::to_string))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn is_synthetic_prompt(s: &str) -> bool {
    let t = s.trim_start();
    t.starts_with("<local-command") || t.starts_with("<command-") || t.starts_with("<system-reminder") || t.is_empty()
}

fn route_ids(line: &str, out: &mut BTreeSet<String>) {
    static RE: once_cell::sync::Lazy<regex::Regex> =
        once_cell::sync::Lazy::new(|| regex::Regex::new(r#"route=\\?"([A-Za-z0-9_.-]+)"#).unwrap());
    for c in RE.captures_iter(line) {
        out.insert(c[1].to_string());
    }
}

pub fn read(path: &Path) -> anyhow::Result<Transcript> {
    parse(&std::fs::read_to_string(path)?)
        .ok_or_else(|| anyhow::anyhow!("{}: no session id, cwd, or prompt", path.display()))
}

/// Parse transcript text. `None` without a session id, a cwd, or a prompt.
pub fn parse(text: &str) -> Option<Transcript> {
    let mut id = String::new();
    let mut cwd: Option<String> = None;
    let mut steered = false;
    let mut directed = BTreeSet::new();
    // Items in order; turns hold tool_use ids until results are attached.
    enum Raw {
        Prompt(String),
        Turn(String, Vec<(String, String, Value)>),
    }
    let mut raw: Vec<Raw> = Vec::new();
    let mut results: HashMap<String, (String, bool)> = HashMap::new();
    let mut hook_results: HashMap<String, String> = HashMap::new();

    for line in text.lines() {
        let Ok(rec) = serde_json::from_str::<Value>(line) else { continue };
        if rec.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        if id.is_empty() {
            if let Some(s) = rec.get("sessionId").and_then(Value::as_str) {
                id = s.to_string();
            }
        }
        if cwd.is_none() {
            cwd = rec.get("cwd").and_then(Value::as_str).map(str::to_string);
        }
        let typ = rec.get("type").and_then(Value::as_str).unwrap_or("");
        if matches!(typ, "attachment" | "system") {
            // Only hook output steers (`adage_learn::import`): an edited file's snippet that
            // quotes the wrapper does not.
            let hook_output = match typ {
                "attachment" => rec["attachment"]["type"].as_str().is_some_and(|t| t.starts_with("hook")),
                _ => rec.get("subtype").and_then(Value::as_str).is_some_and(|t| t.contains("hook")),
            };
            if !hook_output {
                continue;
            }
            if line.contains("<adage-reflex") {
                steered = true;
                route_ids(line, &mut directed);
            }
            if line.contains("Adage finish-line check") {
                steered = true;
                directed.insert("finish-line-stop".into());
            }
            // A receipt for work Adage already ran (PreToolUse): Adage's text too.
            if line.contains("<adage-receipt") {
                steered = true;
            }
            continue;
        }
        match typ {
            "user" => {
                let content = &rec["message"]["content"];
                let meta = rec.get("isMeta").and_then(Value::as_bool) == Some(true);
                if let Some(s) = content.as_str() {
                    if !meta && !is_synthetic_prompt(s) {
                        raw.push(Raw::Prompt(s.to_string()));
                    }
                } else if let Some(items) = content.as_array() {
                    let mut tids = Vec::new();
                    let mut texts = Vec::new();
                    for it in items {
                        if it.get("type").and_then(Value::as_str) == Some("tool_result") {
                            let tid = it.get("tool_use_id").and_then(Value::as_str).unwrap_or("").to_string();
                            let failed = it.get("is_error").and_then(Value::as_bool).unwrap_or(false);
                            results.insert(tid.clone(), (text_of(it.get("content").unwrap_or(&Value::Null)), failed));
                            tids.push(tid);
                        } else if let Some(t) = it.get("text").and_then(Value::as_str) {
                            texts.push(t.to_string());
                        }
                    }
                    if let (Some(tur), [tid]) = (rec.get("toolUseResult"), tids.as_slice()) {
                        hook_results.insert(tid.clone(), response_text(tur));
                    }
                    if tids.is_empty() && !meta {
                        let t = texts.join("\n");
                        if !is_synthetic_prompt(&t) {
                            raw.push(Raw::Prompt(t));
                        }
                    }
                }
            }
            "assistant" => {
                let msg = &rec["message"];
                let mid = msg.get("id").and_then(Value::as_str).unwrap_or("").to_string();
                let uses: Vec<(String, String, Value)> = msg
                    .get("content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter(|it| it.get("type").and_then(Value::as_str) == Some("tool_use"))
                    .map(|it| {
                        (
                            it.get("id").and_then(Value::as_str).unwrap_or("").to_string(),
                            it.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                            it.get("input").cloned().unwrap_or(Value::Null),
                        )
                    })
                    .collect();
                if uses.is_empty() {
                    continue;
                }
                match raw.last_mut() {
                    Some(Raw::Turn(m, calls)) if *m == mid && !mid.is_empty() => calls.extend(uses),
                    _ => raw.push(Raw::Turn(mid, uses)),
                }
            }
            _ => {}
        }
    }
    let cwd = cwd?;
    if id.is_empty() || !raw.iter().any(|r| matches!(r, Raw::Prompt(_))) {
        return None;
    }
    let items = raw
        .into_iter()
        .map(|r| match r {
            Raw::Prompt(p) => Item::Prompt(p),
            Raw::Turn(_, uses) => Item::Turn(
                uses.into_iter()
                    .map(|(tid, tool, input)| {
                        let (output, failed) = results.get(&tid).cloned().unwrap_or_default();
                        let hook_output = hook_results.get(&tid).cloned().unwrap_or_else(|| output.clone());
                        ToolCall { tool, input, output, hook_output, failed }
                    })
                    .collect(),
            ),
        })
        .collect();
    Some(Transcript { id, cwd, items, steered, directed })
}

/// Replace every occurrence of `from` with `to` in every string of a JSON value.
pub fn rebase_value(v: &Value, from: &str, to: &str) -> Value {
    match v {
        Value::String(s) => Value::String(s.replace(from, to)),
        Value::Array(a) => Value::Array(a.iter().map(|x| rebase_value(x, from, to)).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), rebase_value(x, from, to))).collect()),
        other => other.clone(),
    }
}

/// A step of the replay: the hook event, and which model turn it belongs to (0 = before any tool
/// call; turn `n` = the n-th model turn that called tools).
#[derive(Debug, Clone)]
pub struct Step {
    pub turn: u32,
    pub event: Event,
}

/// The hook events Claude Code would have sent for this session, in order, with paths rebased
/// from the recorded working directory to `root`. Mirrors the Claude Code adapter: each failed
/// call first raises `PostToolUseFailure`, then the whole turn arrives as one `PostToolBatch`
/// whose calls are all marked not failed (the adapter does not carry failure into a batch).
/// `stop`: the session ended with a Stop hook (not when the harness cut the agent off).
pub fn events(t: &Transcript, root: &str, stop: bool) -> Vec<Step> {
    let mut out = vec![Step { turn: 0, event: Event::SessionStart }];
    let mut turn = 0u32;
    for item in &t.items {
        match item {
            Item::Prompt(p) => out.push(Step { turn, event: Event::Prompt { text: p.clone() } }),
            Item::Turn(calls) => {
                turn += 1;
                let rebased: Vec<(ToolCall, Value)> =
                    calls.iter().map(|c| (c.clone(), rebase_value(&c.input, &t.cwd, root))).collect();
                for (c, input) in &rebased {
                    // A call rejected before it ran (invalid input) raises no failure hook.
                    if c.failed && !c.output.trim_start().starts_with("<tool_use_error>") {
                        let call = Call {
                            tool: c.tool.clone(),
                            input: input.clone(),
                            output: c.output.replace(&t.cwd, root),
                            failed: true,
                        };
                        out.push(Step { turn, event: Event::Failure { call } });
                    }
                }
                let calls = rebased
                    .into_iter()
                    .map(|(c, input)| Call {
                        tool: c.tool,
                        input,
                        output: c.hook_output.replace(&t.cwd, root),
                        failed: false,
                    })
                    .collect();
                out.push(Step { turn, event: Event::Batch { calls } });
            }
        }
    }
    if stop {
        out.push(Step { turn, event: Event::Stop { stop_hook_active: false } });
    }
    out.push(Step { turn, event: Event::SessionEnd });
    out
}

/// The hook event name for a runtime event, as spools record it.
pub fn hook_name(e: &Event) -> &'static str {
    match e {
        Event::SessionStart => "SessionStart",
        Event::Prompt { .. } => "UserPromptSubmit",
        Event::Batch { .. } => "PostToolBatch",
        Event::Failure { .. } => "PostToolUseFailure",
        Event::Stop { .. } => "Stop",
        Event::SessionEnd => "SessionEnd",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_calls_by_message_and_raises_failures_first() {
        let text = [
            r#"{"type":"user","sessionId":"s","cwd":"/w/r","message":{"content":"Fix W-1"}}"#,
            r#"{"type":"assistant","sessionId":"s","cwd":"/w/r","message":{"id":"m1","content":[{"type":"tool_use","id":"a","name":"Bash","input":{"command":"pytest"}}]}}"#,
            r#"{"type":"assistant","sessionId":"s","cwd":"/w/r","message":{"id":"m1","content":[{"type":"tool_use","id":"b","name":"Read","input":{"file_path":"/w/r/x.py"}}]}}"#,
            r#"{"type":"user","sessionId":"s","cwd":"/w/r","message":{"content":[{"type":"tool_result","tool_use_id":"a","content":"boom","is_error":true}]}}"#,
            r#"{"type":"user","sessionId":"s","cwd":"/w/r","message":{"content":[{"type":"tool_result","tool_use_id":"b","content":"1\tcode"}]},"toolUseResult":{"type":"text","file":{}}}"#,
        ]
        .join("\n");
        let t = parse(&text).unwrap();
        assert_eq!(t.turns().count(), 1);
        let ev = events(&t, "/tmp/root", true);
        let names: Vec<_> = ev.iter().map(|s| hook_name(&s.event)).collect();
        assert_eq!(
            names,
            ["SessionStart", "UserPromptSubmit", "PostToolUseFailure", "PostToolBatch", "Stop", "SessionEnd"]
        );
        let Event::Batch { calls } = &ev[3].event else { panic!() };
        assert_eq!(calls[1].input["file_path"], "/tmp/root/x.py");
        assert_eq!(calls[1].output, "", "a Read's hook payload carries no text");
        assert!(!calls[0].failed);
    }
}
