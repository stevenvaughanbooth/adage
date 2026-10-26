//! Claude Code transcripts (`~/.claude/projects/<project>/<session>.jsonl`), decoded into the
//! canonical [`Session`].
//!
//! One record per line. Assistant records carry `tool_use` blocks (several blocks of one model turn
//! share `message.id`); user records carry the prompt (string content) or `tool_result` blocks keyed
//! by `tool_use_id`. A session steered by Adage records every directive it received, where, and what
//! the directive named ([`Directed`]): learning never counts what a directive named, from its
//! delivery on, as evidence (Design §11: never learn from your own influence; Track G §3.2,
//! steering by influence).
//!
//! A resumed session gets a transcript of its own that begins with a copy of the earlier session's
//! records (each still carrying the earlier `sessionId`), followed by its own. The copies are the
//! earlier session's evidence, read from its own transcript; the file decodes to the session its
//! last records belong to, from those records alone ([`decode`]).

use crate::format::{Decode, Format};
use crate::locate::{LocalCheckout, Located};
use crate::signals::{asks_for_help, call_steps, Prompts, Signals};
use adage_core::tools::normalize_tool;
use adage_core::{tokens, CommandTable};
use adage_trace::session::{Ending, CLAUDE_CODE};
use adage_trace::{wire, By, Call, Session};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::io::BufRead;
use std::path::{Path, PathBuf};

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

/// Completion delivery uses a user-shaped record, but its native envelope identifies the
/// system as the source. XML in an ordinary user message is still the user's task text.
fn is_task_notification(rec: &Value) -> bool {
    rec["origin"]["kind"].as_str() == Some("task-notification")
        && rec["promptSource"].as_str() == Some("system")
        && rec["turnOrigin"].as_str() == Some("task_notification")
}

/// Unwrap only a native workflow assignment in a worker's own history. It remains delegated
/// data, never human consent. A quoted frame in a tool result is not a prompt.
fn computed_task(s: &str) -> Option<String> {
    if !s.starts_with("[Workflow harness — computed task]") {
        return None;
    }
    let (_, body) = s.split_once("The computed task text follows:\n")?;
    let mut out = Vec::new();
    for line in body.lines() {
        out.push(if line.is_empty() { "" } else { line.strip_prefix("  ")? });
    }
    let body = out.join("\n");
    (!body.trim().is_empty()).then(|| tokens::bounded(&body, adage_trace::session::MAX_PROMPT_BYTES).to_string())
}

/// A workflow's handback is complete only after the harness accepts this exact tool call.
/// Its reported verdict is deliberately not interpreted as a task outcome.
struct Handback {
    id: String,
    data: Option<Value>,
    attached: bool,
}

impl Handback {
    fn from_tool(tool: &Value) -> Option<Self> {
        let id = tool.get("id")?.as_str()?.to_string();
        if id.is_empty() {
            return None;
        }
        let data = match tool.get("name")?.as_str()? {
            "StructuredOutput" if tool.get("input")?.is_object() => Some(tool["input"].clone()),
            "SubagentHandback" if tool["input"]["message"].as_str().is_some() => None,
            _ => return None,
        };
        Some(Self { id, data, attached: false })
    }

    fn accepts(&self, result: &Value) -> bool {
        if result.get("tool_use_id").and_then(Value::as_str) != Some(self.id.as_str())
            || !matches!(result.get("is_error"), None | Some(Value::Bool(false)))
        {
            return false;
        }
        let text = text_of(&result["content"]);
        match &self.data {
            Some(_) => self.attached && text == "Structured output provided successfully",
            None => serde_json::from_str::<Value>(&text).ok().is_some_and(|value| {
                value == serde_json::json!({"success":true,"message":"Report delivered to your caller."})
            }),
        }
    }
}

/// Parse one transcript file. Returns `None` for sessions without a usable prompt. Sidechain
/// records inside a top-level transcript (an older Claude Code's inline sub-agent turns) are
/// skipped; a sub-agent's own transcript (`<session>/subagents/agent-*.jsonl`, every record a
/// sidechain) is a trace of its own.
///
/// The file is streamed line by line, so memory is bounded by the session's own tool calls, not
/// by the file (a long transcript is mostly tool output, kept only once per call).
pub fn parse_transcript(path: &Path, table: &CommandTable, repo_override: Option<&str>) -> Option<Session> {
    let reader = std::io::BufReader::new(std::fs::File::open(path).ok()?);
    let cx = Decode { table, locator: &LocalCheckout, repo_override, subagent: is_subagent(path) };
    decode(reader, &cx)
}

/// Workflow sub-agents may be nested below `subagents/`. Keep discovery and classification on
/// the same bounded layout; journals and unrelated JSONL files are not agent transcripts.
const SUBAGENT_DEPTH: usize = 8;

fn agent_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("agent-") && n.len() > "agent-.jsonl".len() && n.ends_with(".jsonl"))
}

/// Whether a transcript path is a sub-agent's own trace, including workflow descendants.
pub fn is_subagent(path: &Path) -> bool {
    agent_file(path)
        && path.ancestors().skip(1).take(SUBAGENT_DEPTH + 1).any(|p| p.file_name().is_some_and(|n| n == "subagents"))
}

/// Decode one Claude Code transcript from any reader: a local file, a remote store, memory. The
/// repository is found by `cx.locator`; nothing else here touches the filesystem.
///
/// Records are read in segments of one `sessionId` each. A record whose `sessionId` differs from
/// the segment's starts a new one, and the file is the session of its last segment: the records
/// before it are copies a resume made of an earlier session, whose own transcript is their
/// evidence. What that earlier context delivered (its directives) still steers the resumed
/// session, so the directives found before the boundary are kept, at its first step.
pub fn decode(reader: impl BufRead, cx: &Decode<'_>) -> Option<Session> {
    let mut lines = reader.lines().map_while(Result::ok);
    let mut first: Option<String> = None;
    let mut prior = Signals::default();
    loop {
        match decode_segment(first.take(), &mut lines, cx, prior) {
            Segment::Done(session) => return session.map(|s| *s),
            Segment::Resumed { line, mut signals } => {
                for d in &mut signals.found {
                    d.at_step = 0;
                }
                first = Some(line);
                prior = signals;
            }
        }
    }
}

/// What reading one segment of a transcript ends in.
enum Segment {
    /// The transcript ended: its session, if it holds a usable one.
    Done(Option<Box<Session>>),
    /// A record of another session began: the record itself, to start the next segment with, and
    /// the directives delivered so far.
    Resumed { line: String, signals: Signals },
}

/// Decode records from `first` and then `lines` until they end or a record of another session
/// begins. `prior` carries the directives delivered before this segment.
fn decode_segment(
    first: Option<String>,
    lines: &mut impl Iterator<Item = String>,
    cx: &Decode<'_>,
    prior: Signals,
) -> Segment {
    let (table, repo_override, subagent) = (cx.table, cx.repo_override, cx.subagent);
    let mut id = String::new();
    // The `sessionId` this segment's records carry (`id` adds the agent for a sub-agent's).
    let mut session_id = String::new();
    let mut cwd: Option<String> = None;
    let mut model: Option<String> = None;
    let mut day = String::new();
    let mut prompt: Option<String> = None;
    // (turn, tool_use_id, name, input, prompt index) in order
    let mut uses: Vec<(u32, String, String, Value, u16, Option<String>)> = Vec::new();
    let mut issued_ids = BTreeSet::new();
    let mut results: HashMap<String, (String, bool)> = HashMap::new();
    let mut turn_of_msg: HashMap<String, u32> = HashMap::new();
    let mut signals = prior;
    let mut prompts = Prompts::default();
    // The last thing the transcript records: the agent's final reply (its text), or a tool call or
    // result with no reply after it.
    let mut last_reply: Option<String> = None;
    let mut ends_on_tool = false;
    let mut handback: Option<Handback> = None;
    let mut structured_handback = false;
    let mut delegation: Option<adage_trace::session::Delegation> = None;

    let mut pending = first;
    while let Some(owned) = pending.take().or_else(|| lines.next()) {
        let line = owned.as_str();
        let Ok(rec) = serde_json::from_str::<Value>(line) else { continue };
        if !subagent && rec.get("isSidechain").and_then(|v| v.as_bool()) == Some(true) {
            continue;
        }
        if let Some(s) = rec.get("sessionId").and_then(|v| v.as_str()) {
            if !session_id.is_empty() && session_id != s {
                return Segment::Resumed { line: owned, signals };
            }
        }
        if id.is_empty() {
            // A sub-agent shares its parent's session id; its agent id tells the traces apart.
            let agent = rec.get("agentId").and_then(|v| v.as_str()).filter(|_| subagent);
            if let Some(s) = rec.get("sessionId").and_then(|v| v.as_str()) {
                session_id = s.to_string();
                id = match agent {
                    Some(a) => {
                        delegation = Some(adage_trace::session::Delegation {
                            parent_id: s.to_string(),
                            agent_id: a.to_string(),
                            ..Default::default()
                        });
                        format!("{s}/{a}")
                    }
                    None => s.to_string(),
                };
            }
        }
        if cwd.is_none() {
            cwd = rec.get("cwd").and_then(|v| v.as_str()).map(str::to_string);
        }
        if day.is_empty() {
            if let Some(ts) = rec.get("timestamp").and_then(|v| v.as_str()) {
                day = ts.chars().take(10).collect();
            }
        }
        // Directives are delivered as hook output (`hook_additional_context` attachments, hook
        // system records); text elsewhere (a tool printing Adage's own docs, an edited file's
        // snippet that quotes the wrapper) is not steering.
        let hook_output = match rec.get("type").and_then(|v| v.as_str()) {
            Some("attachment") => rec["attachment"]["type"].as_str().is_some_and(|t| t.starts_with("hook")),
            Some("system") => rec.get("subtype").and_then(|v| v.as_str()).is_some_and(|t| t.contains("hook")),
            _ => false,
        };
        if hook_output {
            signals.at(uses.len());
            signals.note(line);
        }
        match rec.get("type").and_then(|v| v.as_str()) {
            Some("user") => {
                let content = &rec["message"]["content"];
                let notification = is_task_notification(&rec);
                if !notification && content.is_string() && (structured_handback || handback.is_some()) {
                    // A new instruction/continuation is work after the earlier report.
                    structured_handback = false;
                    handback = None;
                    ends_on_tool = true;
                    last_reply = None;
                }
                // A Stop-hook continuation Adage wrote, should Claude Code record it as a user
                // message: Adage's direction, never the person's prompt.
                let own = |t: &str| wire::is_finish_line(t);
                if let Some(s) = content.as_str().filter(|s| !notification && own(s)) {
                    signals.at(uses.len());
                    signals.note(s);
                } else if let Some(s) = content.as_str() {
                    if !notification
                        && rec.get("isMeta").and_then(|v| v.as_bool()) != Some(true)
                        && !is_synthetic_prompt(s)
                    {
                        if let Some(d) = delegation.as_mut().filter(|d| d.task.is_none()) {
                            d.task = computed_task(s);
                        }
                        prompts.push(s);
                        if prompt.is_none() {
                            prompt = Some(s.to_string());
                        }
                    }
                } else if let Some(items) = content.as_array() {
                    let mut typed: Option<&str> = None;
                    for it in items {
                        if it.get("type").and_then(|v| v.as_str()) == Some("tool_result") {
                            ends_on_tool = true;
                            last_reply = None;
                            let tid = it.get("tool_use_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            // Earlier calls may finish between the handback call, its accepted
                            // attachment and acknowledgment. Their first result is not new work.
                            // Unknown/duplicate results cannot preserve a pending handback.
                            let earlier_result = !tid.is_empty()
                                && !results.contains_key(&tid)
                                && uses.last().is_some_and(|(_, last, ..)| *last != tid)
                                && issued_ids.contains(&tid);
                            if !earlier_result {
                                structured_handback = handback.take().is_some_and(|h| h.accepts(it));
                            }
                            let failed = it.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false);
                            results.insert(tid, (text_of(it.get("content").unwrap_or(&Value::Null)), failed));
                        } else if let Some(t) = it.get("text").and_then(|v| v.as_str()).filter(|_| !notification) {
                            if structured_handback || handback.is_some() {
                                structured_handback = false;
                                handback = None;
                                ends_on_tool = true;
                                last_reply = None;
                            }
                            if own(t) {
                                signals.at(uses.len());
                                signals.note(t);
                            } else if !is_synthetic_prompt(t) {
                                typed.get_or_insert(t);
                                if prompt.is_none() {
                                    prompt = Some(t.to_string());
                                }
                            }
                        }
                    }
                    if let Some(t) = typed {
                        prompts.push(t);
                    }
                }
            }
            Some("assistant") => {
                let msg = &rec["message"];
                if model.is_none() {
                    model = msg.get("model").and_then(|v| v.as_str()).map(str::to_string);
                }
                let mid = msg.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if let Some(items) = msg.get("content").and_then(|c| c.as_array()) {
                    for it in items {
                        if it.get("type").and_then(|v| v.as_str()) == Some("text") {
                            if let Some(t) = it.get("text").and_then(|v| v.as_str()).filter(|t| !t.trim().is_empty()) {
                                handback = None;
                                if msg.get("model").and_then(Value::as_str) == Some("<synthetic>") {
                                    // Harness error text (for example an interrupted response)
                                    // is not the agent's completed reply.
                                    last_reply = None;
                                    ends_on_tool = true;
                                    structured_handback = false;
                                } else {
                                    last_reply = Some(tokens::bounded(t, 4096).to_string());
                                    ends_on_tool = false;
                                }
                            }
                        }
                        if it.get("type").and_then(|v| v.as_str()) == Some("tool_use") {
                            ends_on_tool = true;
                            last_reply = None;
                            structured_handback = false;
                            handback = Handback::from_tool(it);
                            let next = turn_of_msg.len() as u32;
                            let turn = *turn_of_msg.entry(mid.clone()).or_insert(next);
                            let tid = it.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            issued_ids.insert(tid.clone());
                            uses.push((
                                turn,
                                tid,
                                it.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                it.get("input").cloned().unwrap_or(Value::Null),
                                prompts.current(),
                                rec.get("cwd").and_then(Value::as_str).map(str::to_string),
                            ));
                        }
                    }
                }
            }
            Some("attachment") if rec["attachment"]["type"].as_str() == Some("structured_output") => {
                let attachment = &rec["attachment"];
                let matches = handback.as_ref().is_some_and(|h| {
                    attachment["toolUseID"].as_str() == Some(h.id.as_str())
                        && h.data.as_ref().is_some_and(|data| attachment.get("data") == Some(data))
                });
                if matches {
                    handback.as_mut().expect("matched pending handback").attached = true;
                } else {
                    handback = None;
                    structured_handback = false;
                }
            }
            Some("attachment") if rec["attachment"]["type"].as_str() == Some("prompt_snapshot") => {
                if let Some(d) = delegation.as_mut().filter(|d| d.instructions.is_none()) {
                    // Only the worker role, not inherited repository text or reported verdicts.
                    d.instructions = rec["attachment"]["systemPrompt"]
                        .as_array()
                        .and_then(|parts| parts.first())
                        .and_then(Value::as_str)
                        .map(|s| tokens::bounded(s, adage_trace::session::MAX_PROMPT_BYTES).to_string());
                }
            }
            _ => {}
        }
    }

    let (Some(cwd), Some(prompt)) = (cwd, prompt) else { return Segment::Done(None) };
    let Located { root, repo: repo_key } = cx.locator.locate(Path::new(&cwd), repo_override);
    let root_str = root.to_string_lossy().to_string();
    let mut steps = Vec::new();
    let mut first_step: Vec<u32> = Vec::with_capacity(uses.len());
    let unsafe_shell_turns: BTreeSet<u32> = uses
        .iter()
        .filter_map(|(turn, _, name, input, _, recorded)| {
            let direct = input
                .get("command")
                .and_then(Value::as_str)
                .is_some_and(|c| adage_core::execution_equivalence::project_pytest_key(c).is_some());
            let at_root = recorded.as_deref().is_some_and(|p| Path::new(p) == root);
            (name == "Bash" && (!direct || !at_root)).then_some(*turn)
        })
        .collect();
    let mut shell_cwd_known = true;
    for (turn, tid, name, input, at, recorded_cwd) in uses {
        // Tool calls in one assistant turn may run concurrently; ordering in JSON is not proof.
        if unsafe_shell_turns.contains(&turn) {
            shell_cwd_known = false;
        }
        first_step.push(steps.len() as u32);
        let (output, failed) = results.get(&tid).cloned().unwrap_or_default();
        // A call the mod made in the agent's place carries Adage's id (`adage_…`): never organic
        // evidence of what the agent does. Retain native ids to join procedure evidence back to
        // exact calls, including several normalized actions from the same shell invocation.
        let by = By::of_tool_use_id(&tid);
        let call = Call {
            tool: name.clone(),
            input: input.clone(),
            output,
            failed,
            by,
            tool_use_id: (!tid.is_empty()).then_some(tid),
            agent: delegation.as_ref().map(|d| d.agent_id.clone()),
        };
        let direct = input
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(|c| adage_core::execution_equivalence::project_pytest_key(c).is_some());
        let root_cwd = recorded_cwd.as_deref().is_some_and(|p| Path::new(p) == root);
        let eligible_cwd = name == "Bash" && shell_cwd_known && root_cwd && direct;
        if name == "Bash" && (!root_cwd || !direct) {
            // A recorded root may be session metadata, not a fresh shell pwd. Never use it to
            // undo an earlier cd, source, compound or unknown command's cwd uncertainty.
            shell_cwd_known = false;
        }
        let mut imported = call_steps(turn, at, &call, normalize_tool(&name, &input, &root_str, table), &root);
        for step in &mut imported {
            step.project_pytest_root_cwd = eligible_cwd;
        }
        steps.extend(imported);
    }
    let prompts = prompts.anchor(&steps);
    let influence = signals.anchor(&first_step, steps.len());
    let ending = Ending {
        asked_for_help: last_reply.as_deref().is_some_and(asks_for_help),
        cut_off: ends_on_tool && !structured_handback && !steps.is_empty(),
        structured_handback,
    };
    Segment::Done(Some(Box::new(Session {
        id,
        delegation,
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
        harness: CLAUDE_CODE.into(),
        prompts,
        identity: Default::default(),
        ending,
    })))
}

/// Claude Code transcripts as a [`Format`]: the fallback, tried last (anything else is not a
/// transcript, and decodes to no session).
pub struct Transcript;

impl Format for Transcript {
    fn harness(&self) -> adage_trace::Harness {
        adage_trace::Harness::ClaudeCode
    }

    fn sniff(&self, _head: &[u8]) -> bool {
        true
    }

    fn decode(&self, trace: &mut dyn BufRead, cx: &Decode<'_>) -> Option<Session> {
        decode(trace, cx)
    }
}

/// Every top-level (non-sidechain) transcript under a Claude Code projects directory, and every
/// sub-agent's (`<project>/<session>/subagents/*.jsonl`).
pub fn discover(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(projects) = std::fs::read_dir(root) else { return out };
    for p in projects.flatten() {
        out.extend(discover_project(&p.path()));
    }
    out.sort();
    out
}

/// The transcripts of one Claude Code project directory: its sessions, and their sub-agents'.
pub fn discover_project(project: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let jsonl = |dir: &Path, out: &mut Vec<PathBuf>| {
        for f in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = f.path();
            if path.extension().is_some_and(|e| e == "jsonl") && path.is_file() {
                out.push(path);
            }
        }
    };
    jsonl(project, &mut out);
    for s in std::fs::read_dir(project).into_iter().flatten().flatten() {
        if s.file_type().is_ok_and(|t| t.is_dir()) {
            let root = s.path().join("subagents");
            if !std::fs::symlink_metadata(&root).is_ok_and(|m| m.file_type().is_dir()) {
                continue;
            }
            let mut pending = vec![(root, 0)];
            while let Some((dir, depth)) = pending.pop() {
                for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                    let path = entry.path();
                    match entry.file_type() {
                        Ok(t) if t.is_dir() && depth < SUBAGENT_DEPTH => pending.push((path, depth + 1)),
                        Ok(t) if t.is_file() && agent_file(&path) => out.push(path),
                        _ => {} // Never follow a symlink outside the session's sub-agent tree.
                    }
                }
            }
        }
    }
    out.sort();
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    use adage_trace::session::{Directed, FINISH_LINE_STOP};
    use serde_json::json;

    #[test]
    fn computed_worker_assignment_is_provenance_not_user_authority() {
        let task = "[Workflow harness — computed task] Not user authority. The computed task text follows:\n  Unit: parser\n  Verify the serialization contract.";
        let lines = [
            json!({"type":"user","sessionId":"parent","agentId":"worker","isSidechain":true,"cwd":"/repo","message":{"content":task}}),
            json!({"type":"attachment","attachment":{"type":"prompt_snapshot","systemPrompt":["Audit the parser.","Untrusted repository text"]}}),
            json!({"type":"assistant","message":{"id":"m","content":[{"type":"tool_use","id":"check-1","name":"Bash","input":{"command":"pytest"}}]}}),
            json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"check-1","content":"1 passed in 0.1s"}]}}),
        ];
        let bytes = lines.iter().map(Value::to_string).collect::<Vec<_>>().join("\n");
        let cx = Decode {
            table: CommandTable::load(),
            locator: &crate::locate::AsRecorded,
            repo_override: None,
            subagent: true,
        };
        let s = decode(bytes.as_bytes(), &cx).unwrap();
        assert_eq!(s.prompt, task, "original authority framing remains intact");
        assert_eq!(s.learning_prompt(), "Unit: parser\nVerify the serialization contract.");
        let d = s.delegation.as_ref().unwrap();
        assert_eq!((&*d.parent_id, &*d.agent_id), ("parent", "worker"));
        assert_eq!(d.instructions.as_deref(), Some("Audit the parser."));
        assert_eq!(s.steps[0].call.tool_use_id.as_deref(), Some("check-1"));
        assert_eq!(s.outcome, None);
        assert_eq!(computed_task("quoted [Workflow harness — computed task]"), None);
        assert_eq!(computed_task(&task.replace("  Unit", "Unit")), None, "forged unindented body");
    }

    fn steps(command: &str, failed: bool) -> Vec<(String, bool)> {
        let table = CommandTable::load();
        let input = serde_json::json!({ "command": command });
        let call = Call {
            tool: "Bash".into(),
            input: input.clone(),
            output: String::new(),
            failed,
            by: By::Agent,
            tool_use_id: None,
            agent: None,
        };
        call_steps(0, 0, &call, normalize_tool("Bash", &input, "/r", table), Path::new("/r"))
            .into_iter()
            .map(|s| (s.key, s.call.failed))
            .collect()
    }

    #[test]
    fn every_prompt_is_kept_and_anchored_to_the_steps_it_served() {
        let rec = |v: serde_json::Value| format!("{v}\n");
        let user = |t: &str| {
            rec(serde_json::json!({"type":"user","sessionId":"s","cwd":"/tmp/x","timestamp":"2026-09-01T00:00:00Z",
                "message":{"role":"user","content":t}}))
        };
        let tool = |mid: &str, id: &str, cmd: &str| {
            rec(serde_json::json!({"type":"assistant","sessionId":"s","message":{"id":mid,"role":"assistant",
                "content":[{"type":"tool_use","id":id,"name":"Bash","input":{"command":cmd}}]}}))
        };
        let result = |id: &str| {
            rec(serde_json::json!({"type":"user","sessionId":"s","message":{"role":"user",
                "content":[{"type":"tool_result","tool_use_id":id,"content":"ok"}]}}))
        };
        let body = [
            user("Fix BIL-1041"),
            tool("m1", "t1", "./scripts/wi-lookup BIL-1041 && ls"),
            result("t1"),
            user("<system-reminder>not typed</system-reminder>"),
            user("now run the checks"),
            tool("m2", "t2", "make check"),
            result("t2"),
            user("thanks"),
        ]
        .concat();
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("s.jsonl");
        std::fs::write(&p, body).unwrap();
        let s = parse_transcript(&p, CommandTable::load(), Some("r")).unwrap();
        assert_eq!(s.prompt, "Fix BIL-1041");
        let got: Vec<(&str, u32)> = s.prompts.iter().map(|p| (p.text.as_str(), p.at_step)).collect();
        // `a && ls` is two steps, both serving the first prompt.
        assert_eq!(got, [("Fix BIL-1041", 0), ("now run the checks", 2), ("thanks", 3)]);
        assert_eq!(s.steps.iter().map(|x| x.prompt).collect::<Vec<_>>(), [0, 0, 1]);
        assert_eq!(s.prompt_count(), 3);
    }

    /// Plan §2.1 item 3: a call whose tool-use id Adage minted (`adage_…`, a step the Claude Code
    /// mod made in the agent's place) is Adage's in the trace; every other call stays the agent's,
    /// and no call keeps its id.
    #[test]
    fn calls_with_adages_ids_are_adages() {
        let rec = |v: serde_json::Value| format!("{v}\n");
        let tool = |mid: &str, id: &str, cmd: &str| {
            rec(serde_json::json!({"type":"assistant","sessionId":"s","message":{"id":mid,"role":"assistant",
                "content":[{"type":"tool_use","id":id,"name":"Bash","input":{"command":cmd}}]}}))
        };
        let result = |id: &str| {
            rec(serde_json::json!({"type":"user","sessionId":"s","message":{"role":"user",
                "content":[{"type":"tool_result","tool_use_id":id,"content":"ok"}]}}))
        };
        let body = [
            rec(serde_json::json!({"type":"user","sessionId":"s","cwd":"/tmp/x","timestamp":"2026-09-01T00:00:00Z",
                "message":{"role":"user","content":"Change the schema"}})),
            tool("m1", "toolu_01", "ls"),
            result("toolu_01"),
            tool("m2", "adage_gen_turn-1_2_0", "make gen"),
            result("adage_gen_turn-1_2_0"),
            tool("m3", "toolu_adage_x", "make check"),
            result("toolu_adage_x"),
        ]
        .concat();
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("s.jsonl");
        std::fs::write(&p, body).unwrap();
        let s = parse_transcript(&p, CommandTable::load(), Some("r")).unwrap();
        let by: Vec<(String, By)> = s.steps.iter().map(|x| (x.key.clone(), x.call.by)).collect();
        assert_eq!(
            by,
            [
                ("list {path}".to_string(), By::Agent),
                ("exec make gen".into(), By::Adage),
                ("exec make check".into(), By::Agent)
            ]
        );
        assert_eq!(
            s.steps.iter().map(|x| x.call.tool_use_id.as_deref()).collect::<Vec<_>>(),
            vec![Some("toolu_01"), Some("adage_gen_turn-1_2_0"), Some("toolu_adage_x")],
            "native receipt joins are retained for agent and Adage calls"
        );
        let json = serde_json::to_string(&s.steps[0]).unwrap();
        assert!(!json.contains("\"by\""), "an agent's step reads as before: {json}");
    }

    /// A resumed session's transcript begins with copies of the earlier session's records (each
    /// keeping its `sessionId`) before its own. It is the resumed session, from its own records
    /// alone: never a second, conflicting record of the earlier session, whose own transcript is
    /// its evidence. A directive the earlier context delivered still steers the resumed session.
    #[test]
    fn a_resumed_sessions_transcript_is_its_own_records_alone() {
        let rec = |v: serde_json::Value| format!("{v}\n");
        let user = |sid: &str, day: &str, t: &str| {
            rec(serde_json::json!({"type":"user","sessionId":sid,"uuid":format!("{sid}-{t}"),"cwd":"/tmp/x",
                "timestamp":format!("{day}T10:00:00Z"),"message":{"role":"user","content":t}}))
        };
        let tool = |sid: &str, mid: &str, id: &str, cmd: &str| {
            rec(serde_json::json!({"type":"assistant","sessionId":sid,"uuid":format!("{sid}-{id}"),"cwd":"/tmp/x",
                "message":{"id":mid,"role":"assistant","model":"claude-x",
                "content":[{"type":"tool_use","id":id,"name":"Bash","input":{"command":cmd}}]}}))
        };
        let result = |sid: &str, id: &str| {
            rec(serde_json::json!({"type":"user","sessionId":sid,"uuid":format!("{sid}-{id}-r"),"cwd":"/tmp/x",
                "message":{"role":"user","content":[{"type":"tool_result","tool_use_id":id,"content":"ok"}]}}))
        };
        let wrapper =
            "<adage-reflex track=\"t@1\" route=\"r\" strength=\"fire\" kind=\"run\">\n  make gen\n</adage-reflex>";
        let hook = |sid: &str| {
            rec(serde_json::json!({"type":"attachment","sessionId":sid,
                "attachment":{"type":"hook_additional_context","hookEvent":"UserPromptSubmit","content":[wrapper]}}))
        };
        let earlier = [user("a", "2026-09-01", "Fix the schema"), tool("a", "m1", "t1", "make gen"), result("a", "t1")];
        let own =
            [user("b", "2026-09-03", "now run the checks"), tool("b", "m2", "t2", "make check"), result("b", "t2")];
        let cx = Decode {
            table: CommandTable::load(),
            locator: &crate::locate::AsRecorded,
            repo_override: Some("r"),
            subagent: false,
        };
        let keys = |s: &Session| s.steps.iter().map(|x| x.key.clone()).collect::<Vec<_>>();

        let original = decode(earlier.concat().as_bytes(), &cx).unwrap();
        assert_eq!((original.id.as_str(), original.day.as_str()), ("a", "2026-09-01"));
        assert_eq!(keys(&original), ["exec make gen"]);

        let resumed = decode((earlier.concat() + &own.concat()).as_bytes(), &cx).unwrap();
        assert_eq!((resumed.id.as_str(), resumed.day.as_str()), ("b", "2026-09-03"));
        assert_eq!(resumed.prompt, "now run the checks");
        assert_eq!(resumed.prompt_count(), 1);
        assert_eq!(keys(&resumed), ["exec make check"], "the copies are the earlier session's evidence");
        assert!(!resumed.steered && resumed.influence.is_empty());

        // Resumed twice: the last session's records.
        let again = [user("c", "2026-09-04", "and lint"), tool("c", "m3", "t3", "make lint")];
        let twice = decode((earlier.concat() + &own.concat() + &again.concat()).as_bytes(), &cx).unwrap();
        assert_eq!((twice.id.as_str(), twice.prompt.as_str()), ("c", "and lint"));
        assert_eq!(keys(&twice), ["exec make lint"]);

        // A directive delivered in the earlier context steers the resumed session from its start.
        let steered = decode((earlier.concat() + &hook("a") + &own.concat()).as_bytes(), &cx).unwrap();
        assert_eq!((steered.id.as_str(), keys(&steered)), ("b", vec!["exec make check".to_string()]));
        assert!(steered.steered);
        assert_eq!(steered.directed, BTreeSet::from(["r".to_string()]));
        assert_eq!(steered.influence.iter().map(|d| (d.route.as_str(), d.at_step)).collect::<Vec<_>>(), [("r", 0)]);

        // Only the resumed session's own records: none with a prompt, no session.
        let only_copies = earlier.concat() + &tool("b", "m2", "t2", "make check") + &result("b", "t2");
        assert!(decode(only_copies.as_bytes(), &cx).is_none());
    }

    /// Only hook output steers: an edited file's snippet that quotes the wrapper (an edit to
    /// Adage's own docs, say) does not.
    #[test]
    fn only_hook_output_marks_a_session_steered() {
        let wrapper =
            "<adage-reflex track=\"t@1\" route=\"r\" strength=\"fire\" kind=\"run\">\n  make gen\n</adage-reflex>";
        let rec = |v: serde_json::Value| format!("{v}\n");
        let head =
            rec(serde_json::json!({"type":"user","sessionId":"s","cwd":"/tmp/x","timestamp":"2026-09-01T00:00:00Z",
            "message":{"role":"user","content":"Fix the docs"}}));
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("s.jsonl");
        for (attachment, steered) in [
            (serde_json::json!({"type":"edited_text_file","filename":"docs/lexicon.md","snippet":wrapper}), false),
            (
                serde_json::json!({"type":"hook_additional_context","hookEvent":"UserPromptSubmit","content":[wrapper]}),
                true,
            ),
        ] {
            let body =
                head.clone() + &rec(serde_json::json!({"type":"attachment","sessionId":"s","attachment":attachment}));
            std::fs::write(&p, body).unwrap();
            let s = parse_transcript(&p, CommandTable::load(), Some("r")).unwrap();
            assert_eq!(s.steered, steered, "{attachment}");
        }
    }

    /// Sub-agents' transcripts are traces of their own (every record a sidechain), found beside
    /// their parent's; a removed Claude Code worktree keeps its repository's key, with paths
    /// relative to the worktree.
    #[test]
    fn sub_agent_traces_are_found_and_removed_worktrees_keep_their_repository() {
        let d = tempfile::tempdir().unwrap();
        let main = d.path().join("repo");
        std::fs::create_dir_all(main.join(".git")).unwrap();
        std::fs::write(main.join(".git/config"), "[remote \"origin\"]\n\turl = git@github.com:acme/billing.git\n")
            .unwrap();
        let gone = main.join(".claude/worktrees/agent-1");
        let rec = |v: serde_json::Value| format!("{v}\n");
        let body = [
            rec(serde_json::json!({"type":"user","isSidechain":true,"agentId":"a1","sessionId":"s","cwd":gone,
                "timestamp":"2026-09-01T00:00:00Z","message":{"role":"user","content":"Fix the schema"}})),
            rec(serde_json::json!({"type":"assistant","isSidechain":true,"agentId":"a1","sessionId":"s",
                "message":{"id":"m1","role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Edit",
                "input":{"file_path":gone.join("schema/a.json"),"old_string":"a","new_string":"b"}}]}})),
        ]
        .concat();
        let projects = d.path().join("projects");
        let sub = projects.join("-repo/s/subagents");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(projects.join("-repo/s.jsonl"), "").unwrap();
        std::fs::write(sub.join("agent-a1.jsonl"), &body).unwrap();
        std::fs::write(sub.join("agent-a1.meta.json"), "{}").unwrap();
        let found = discover(&projects);
        assert_eq!(found, [sub.join("agent-a1.jsonl"), projects.join("-repo/s.jsonl")]);
        let s = parse_transcript(&sub.join("agent-a1.jsonl"), CommandTable::load(), None).unwrap();
        assert_eq!((s.id.as_str(), s.repo.as_str()), ("s/a1", "github.com/acme/billing"));
        assert_eq!(s.root, gone);
        assert_eq!(s.steps[0].rel_path.as_deref(), Some("schema/a.json"), "relative to the worktree");
        // In a top-level transcript, sidechain records are another agent's turns: skipped.
        std::fs::write(projects.join("-repo/s.jsonl"), &body).unwrap();
        assert!(parse_transcript(&projects.join("-repo/s.jsonl"), CommandTable::load(), None).is_none());
    }

    #[test]
    fn workflow_subagents_are_discovered_and_keep_their_own_identity() {
        let d = tempfile::tempdir().unwrap();
        let project = d.path().join("projects/-repo");
        let direct = project.join("parent/subagents/agent-direct.jsonl");
        let nested = project.join("parent/subagents/workflows/work-1/agent-worker.jsonl");
        let body = |agent: &str| {
            [
                json!({"type":"user","isSidechain":true,"agentId":agent,"sessionId":"parent","cwd":"/repo",
                    "message":{"role":"user","content":"Implement the assigned change"}}),
                json!({"type":"assistant","isSidechain":true,"agentId":agent,"sessionId":"parent",
                    "message":{"id":"m1","content":[{"type":"tool_use","id":"t1","name":"Read",
                        "input":{"file_path":"/repo/src/lib.rs"}}]}}),
            ]
            .into_iter()
            .map(|v| format!("{v}\n"))
            .collect::<String>()
        };
        for (path, agent) in [(&direct, "direct"), (&nested, "worker")] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body(agent)).unwrap();
        }
        std::fs::write(nested.parent().unwrap().join("journal.jsonl"), body("journal")).unwrap();
        std::fs::write(nested.parent().unwrap().join("notes.jsonl"), body("notes")).unwrap();
        let found = discover_project(&project);
        assert_eq!(found, vec![direct, nested.clone()]);
        let cx = Decode {
            table: CommandTable::load(),
            locator: &crate::AsRecorded,
            repo_override: Some("r"),
            subagent: false,
        };
        let session = crate::read_trace(&nested, &cx).unwrap();
        assert_eq!(session.id, "parent/worker");
        assert_eq!(session.steps.len(), 1);
        assert!(!is_subagent(&nested.parent().unwrap().join("journal.jsonl")));
        let too_deep =
            project.join("parent/subagents").join("deep/".repeat(SUBAGENT_DEPTH + 1)).join("agent-deep.jsonl");
        std::fs::create_dir_all(too_deep.parent().unwrap()).unwrap();
        std::fs::write(&too_deep, body("deep")).unwrap();
        assert!(!is_subagent(&too_deep));
        assert!(!discover_project(&project).contains(&too_deep));
    }

    #[cfg(unix)]
    #[test]
    fn subagent_discovery_does_not_follow_symlinks() {
        let d = tempfile::tempdir().unwrap();
        let project = d.path().join("project");
        let agents = project.join("parent/subagents");
        let elsewhere = d.path().join("elsewhere");
        std::fs::create_dir_all(&agents).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("agent-other.jsonl"), "{}").unwrap();
        std::os::unix::fs::symlink(&elsewhere, agents.join("linked")).unwrap();
        std::os::unix::fs::symlink(elsewhere.join("agent-other.jsonl"), agents.join("agent-link.jsonl")).unwrap();
        assert!(discover_project(&project).is_empty());
    }

    #[test]
    fn a_failed_chain_fails_at_its_last_step_only() {
        assert_eq!(
            steps("make gen && python3 -m pytest", true),
            [("exec make gen".to_string(), false), ("exec python3 -m pytest".to_string(), true)]
        );
        assert_eq!(steps("make gen; make test", true).iter().map(|s| s.1).collect::<Vec<_>>(), [false, true]);
        assert!(steps("make gen && python3 -m pytest", false).iter().all(|s| !s.1));
        assert_eq!(steps("python3 -m pytest", true), [("exec python3 -m pytest".to_string(), true)]);
        // `a || b` failed: `a` failed, then `b` ran and failed too.
        assert!(steps("python3 -m pytest -q || python3 -m unittest", true).iter().all(|s| s.1));
    }

    fn read(text: &str) -> (Vec<String>, Vec<Directed>) {
        let mut s = Signals::default();
        s.at(2);
        s.note(text);
        assert!(s.steered);
        let found = s.anchor(&[0, 1, 5], 9);
        (s.directed.into_iter().collect(), found)
    }

    fn v(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn a_directive_is_read_for_what_it_named_and_where() {
        // A session-wide execution contract steers the entire continuation even before
        // an action runs; never relearn it as an organic demonstration.
        let (_, contract) = read("<adage-reflex track=\"adage/execution@1\" route=\"session-contract\" strength=\"prime\" kind=\"execution-contract\">Use successful current receipts as evidence.</adage-reflex>");
        assert!(contract[0].all);
        let (d, f) = read(
            "<adage-reflex track=\"t@1.0.0\" route=\"finish-line-1\" strength=\"fire\" kind=\"obligation\">\nIn this repository, changes to files matching schema/*.json must be followed by running:\n  make gen\nx\n</adage-reflex>",
        );
        assert_eq!(d, v(&["finish-line-1"]));
        assert_eq!(f.len(), 1);
        assert_eq!((f[0].at_step, f[0].commands.clone(), f[0].all), (5, v(&["make gen"]), false));
        // JSON-escaped, as a Claude Code attachment line holds it: read through its strings.
        let line = serde_json::json!({"type": "attachment", "content": "<adage-reflex track=\"t@1.0.0\" route=\"dead-end-1\" strength=\"inhibit\" kind=\"avoid\">\nIn this repository, running python3 -m pytest fails (\"No module\"). Use this instead:\n  make test\n</adage-reflex>"}).to_string();
        let (_, f) = read(&line);
        assert_eq!((f[0].avoided.clone(), f[0].commands.clone()), (v(&["python3 -m pytest"]), v(&["make test"])));
        // A wrapper cut short, or a data block without its directive: it named everything.
        assert!(read("…<adage-reflex track=").1[0].all);
        assert!(read("<adage-reflex track=\"t@1\" route=\"r\" strength=\"fire\">no end").1[0].all);
        assert!(read("<adage-data track=\"t@1.0.0\" route=\"r\">{}").1.iter().any(|d| d.all));
        // A directive Adage ran itself: everything, from there on.
        let (_, f) = read("<adage-reflex track=\"t@1.0.0\" route=\"lookup-work-item\" strength=\"fire\" kind=\"run\" exec=\"e1\">\nIn this repository, work item details are looked up with:\n  ./scripts/wi-lookup W-1\n</adage-reflex>\n<adage-data track=\"t@1.0.0\" route=\"lookup-work-item\">{}</adage-data>");
        assert!(f[0].all);
        // The finish-line stop request names its step.
        let (d, f) = read("Adage finish-line check: this session changed files matching x, and this repository requires running `make gen` after such changes");
        assert_eq!((d, f[0].commands.clone()), (v(&[FINISH_LINE_STOP]), v(&["make gen"])));
    }

    /// F3 and F8: what Adage writes at a Stop (the finish-line gate's failure) and before a tool
    /// call (a receipt) is its own text in a Claude Code transcript, whether it is recorded as hook
    /// output or as a user message: the session is steered, and the text is never a prompt.
    #[test]
    fn stop_gate_text_and_receipts_are_adages_own_in_claude_transcripts() {
        // The runtime's own gate text (`adage_runtime::render::stop_gate_failed` writes this).
        let gate = adage_trace::wire::stop_gate_failed(
            ".venv/bin/python -m pytest tests -q",
            Some(1),
            &["1 test failed: tests/test_schema.py::test_invoice.".to_string()],
        );
        let receipt = "<adage-receipt operation=\"0123456789ab\">\nAdage's run of `make test` in this session passed on files identical to the current ones.\n</adage-receipt>";
        let rec = |v: serde_json::Value| format!("{v}\n");
        let head =
            rec(serde_json::json!({"type":"user","sessionId":"s","cwd":"/tmp/x","timestamp":"2026-09-01T00:00:00Z",
            "message":{"role":"user","content":"Fix the schema"}}));
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("s.jsonl");
        for (shape, finish_line) in [
            (serde_json::json!({"type":"user","sessionId":"s","message":{"role":"user","content":gate}}), true),
            (
                serde_json::json!({"type":"user","sessionId":"s","message":{"role":"user",
                    "content":[{"type":"text","text":format!("Stop hook feedback:\n{gate}")}]}}),
                true,
            ),
            (serde_json::json!({"type":"system","subtype":"stop_hook_summary","sessionId":"s","content":gate}), true),
            (
                serde_json::json!({"type":"attachment","sessionId":"s","attachment":{"type":"hook_additional_context",
                    "hookEvent":"PreToolUse","content":[receipt]}}),
                false,
            ),
        ] {
            std::fs::write(&p, head.clone() + &rec(shape.clone())).unwrap();
            let s = parse_transcript(&p, CommandTable::load(), Some("r")).unwrap();
            assert!(s.steered, "{shape}");
            assert_eq!((s.prompt.as_str(), s.prompt_count()), ("Fix the schema", 1), "{shape}");
            assert_eq!(s.directed.contains(FINISH_LINE_STOP), finish_line, "{shape}");
            // What the directive named is recorded, so learning never counts the rerun as the
            // agent's own (`adage_learn::import::Exposure`).
            let pytest = ".venv/bin/python -m pytest tests -q";
            assert!(s.influence.iter().any(|d| d.all || d.commands.iter().any(|c| c == pytest)), "{shape}");
        }
    }

    fn transcript(lines: &[serde_json::Value]) -> Session {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("t.jsonl");
        let text: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
        std::fs::write(&p, text.join("\n")).unwrap();
        parse_transcript(&p, CommandTable::load(), Some("acme")).unwrap()
    }

    fn user(text: &str) -> serde_json::Value {
        serde_json::json!({"type": "user", "sessionId": "s1", "cwd": "/tmp/acme", "message": {"role": "user", "content": text}})
    }

    fn task_notification(text: &str) -> Value {
        let mut record = user(text);
        record["origin"] = json!({"kind":"task-notification"});
        record["promptSource"] = "system".into();
        record["turnOrigin"] = "task_notification".into();
        record
    }

    #[test]
    fn native_task_notifications_do_not_become_task_instructions() {
        let text = "<task-notification><task-id>bg1</task-id><status>completed</status></task-notification>";
        let notification = task_notification(text);
        let mut array_notification = notification.clone();
        array_notification["message"]["content"] = json!([{"type":"text","text":text}]);
        let s = transcript(&[
            notification.clone(),
            user("Fix the parser"),
            tool("a", "ls"),
            result("a", "src", false),
            array_notification,
            tool("b", "make test"),
            result("b", "ok", false),
            user("Now check the docs"),
            tool("c", "ls docs"),
            result("c", "guide.md", false),
        ]);
        assert_eq!(s.prompt, "Fix the parser");
        assert_eq!(
            s.prompts.iter().map(|p| (p.text.as_str(), p.at_step)).collect::<Vec<_>>(),
            [("Fix the parser", 0), ("Now check the docs", 2)]
        );
        assert_eq!(s.steps.iter().map(|s| s.prompt).collect::<Vec<_>>(), [0, 0, 1]);
        let bytes = notification.to_string();
        let cx = Decode {
            table: CommandTable::load(),
            locator: &crate::locate::AsRecorded,
            repo_override: None,
            subagent: false,
        };
        assert!(decode(bytes.as_bytes(), &cx).is_none(), "a notification alone is not a task");
    }

    #[test]
    fn quoted_or_incomplete_notification_metadata_keeps_the_user_prompt() {
        let text = "<task-notification><status>completed</status></task-notification>";
        assert_eq!(transcript(&[user(text)]).prompt, text);
        let mut array_user = user(text);
        array_user["message"]["content"] = json!([{"type":"text","text":text}]);
        assert_eq!(transcript(&[array_user]).prompt, text);
        for field in ["origin", "promptSource", "turnOrigin"] {
            let mut missing = task_notification(text);
            missing.as_object_mut().unwrap().remove(field);
            assert_eq!(transcript(&[missing]).prompt, text, "{field}");
            let mut conflicting = task_notification(text);
            conflicting[field] = json!("user");
            assert_eq!(transcript(&[conflicting]).prompt, text, "{field}");
        }
    }

    fn tool(id: &str, cmd: &str) -> serde_json::Value {
        serde_json::json!({"type": "assistant", "sessionId": "s1", "cwd": "/tmp/acme",
            "message": {"id": format!("m{id}"), "content": [{"type": "tool_use", "id": id, "name": "Bash", "input": {"command": cmd}}]}})
    }

    fn result(id: &str, out: &str, error: bool) -> serde_json::Value {
        serde_json::json!({"type": "user", "sessionId": "s1", "cwd": "/tmp/acme",
            "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": id, "content": out, "is_error": error}]}})
    }

    fn reply(text: &str) -> serde_json::Value {
        serde_json::json!({"type": "assistant", "sessionId": "s1", "cwd": "/tmp/acme",
            "message": {"id": "mz", "content": [{"type": "text", "text": text}]}})
    }

    #[test]
    fn outcome_proxies_are_read_from_how_the_transcript_ends() {
        // Asked for help instead of acting.
        let s = transcript(&[
            user("Pick up W-1043"),
            tool("a", "ls"),
            result("a", "Makefile", false),
            reply("I don't have details about W-1043. Could you tell me what W-1043 involves?"),
        ]);
        assert!(s.ending.asked_for_help && !s.ending.cut_off);
        assert_eq!(s.proxy_outcome(), Some(false));
        // Cut off after a tool call (a turn limit).
        let s = transcript(&[user("Pick up W-1043"), tool("a", "make test"), result("a", "ok", false)]);
        assert!(s.ending.cut_off);
        assert_eq!(s.proxy_outcome(), Some(false));
        // Finished, with the last check passing; then failing.
        let done = |failed: bool| {
            transcript(&[
                user("Pick up W-1043"),
                tool("a", "make test"),
                result("a", "FAILED", true),
                tool("b", "make test"),
                result("b", if failed { "FAILED" } else { "OK" }, failed),
                reply("Done: the fix is in acme/billing.py and the tests pass."),
            ])
        };
        assert_eq!(done(false).proxy_outcome(), Some(true));
        assert_eq!(done(true).proxy_outcome(), Some(false));
        // No check ran: unknown.
        let s = transcript(&[user("Pick up W-1043"), tool("a", "ls"), result("a", "x", false), reply("Done.")]);
        assert_eq!(s.proxy_outcome(), None);
    }

    fn structured_report(id: &str, data: Value) -> Vec<Value> {
        vec![
            serde_json::json!({"type":"assistant","message":{"id":format!("m{id}"),"content":[
                {"type":"tool_use","id":id,"name":"StructuredOutput","input":data}]}}),
            serde_json::json!({"type":"attachment","attachment":{
                "type":"structured_output","toolUseID":id,"data":data}}),
            result(id, "Structured output provided successfully", false),
        ]
    }

    fn checked_then(records: &[Value]) -> Session {
        let mut body = vec![user("Review the change"), tool("check", "make test"), result("check", "ok", false)];
        body.extend_from_slice(records);
        transcript(&body)
    }

    #[test]
    fn accepted_structured_reports_complete_without_recording_task_success() {
        for data in [
            serde_json::json!({"status":"done"}),
            serde_json::json!({"status":"blocked"}),
            serde_json::json!({"status":"refused"}),
            serde_json::json!({"status":"failed"}),
            serde_json::json!({"verdict":"pass"}),
            serde_json::json!({"verdict":"fail"}),
            serde_json::json!({"arbitrary":"report"}),
        ] {
            let report = structured_report("report", data.clone());
            let s = checked_then(&report);
            assert!(s.ending.structured_handback, "{data}");
            assert!(!s.ending.cut_off && !s.ending.asked_for_help);
            assert_eq!(s.outcome, None);
            assert_eq!(s.proxy_outcome(), None, "a handback's status is not a quality oracle: {data}");
            assert_eq!(s.outcome_evidence().basis, adage_trace::OutcomeBasis::Unknown);
            let mut with_text = report;
            with_text.push(reply("Report delivered."));
            assert_eq!(checked_then(&with_text).proxy_outcome(), None);
        }
    }

    #[test]
    fn structured_report_needs_the_same_accepted_call_attachment_and_result() {
        let valid = structured_report("report", serde_json::json!({"verdict":"pass"}));
        let mut wrong_id = valid.clone();
        wrong_id[1]["attachment"]["toolUseID"] = "other".into();
        let mut wrong_data = valid.clone();
        wrong_data[1]["attachment"]["data"] = serde_json::json!({"verdict":"fail"});
        let mut failed = valid.clone();
        failed[2] = result("report", "Output does not match required schema", true);
        let mut wrong_result = valid.clone();
        wrong_result[2] = result("other", "Structured output provided successfully", false);
        let mut wrong_tool = valid.clone();
        wrong_tool[0]["message"]["content"][0]["name"] = "OtherTool".into();
        let mut unknown_error = valid.clone();
        unknown_error[2]["message"]["content"][0]["is_error"] = "false".into();
        for records in [
            valid[..2].to_vec(),
            vec![valid[0].clone(), valid[2].clone()],
            valid[1..].to_vec(),
            wrong_id,
            wrong_data,
            failed,
            wrong_result,
            wrong_tool,
            unknown_error,
        ] {
            let s = checked_then(&records);
            assert!(!s.ending.structured_handback, "{records:?}");
            assert!(s.ending.cut_off, "{records:?}");
        }
        let mut corrected = vec![valid[0].clone(), result("report", "Invalid schema", true)];
        corrected.extend(structured_report("corrected", serde_json::json!({"verdict":"fail"})));
        assert!(checked_then(&corrected).ending.structured_handback);
    }

    #[test]
    fn later_work_does_not_inherit_an_earlier_handback() {
        let valid = structured_report("report", serde_json::json!({"status":"done"}));
        for suffix in [
            vec![tool("later", "make test")],
            vec![tool("later", "make test"), result("later", "ok", false)],
            vec![user("Now inspect another change")],
        ] {
            let mut records = valid.clone();
            records.extend(suffix);
            let s = checked_then(&records);
            assert!(!s.ending.structured_handback);
            assert!(s.ending.cut_off);
        }
        let mut late_result = valid[..2].to_vec();
        late_result.extend([tool("later", "make test"), result("later", "ok", false), valid[2].clone()]);
        assert!(!checked_then(&late_result).ending.structured_handback);
    }

    #[test]
    fn system_completion_delivery_is_not_new_work_after_a_handback() {
        let text = "<task-notification><status>completed</status></task-notification>";
        let native = task_notification(text);
        let mut array_native = native.clone();
        array_native["message"]["content"] = json!([{"type":"text","text":text}]);
        for notification in [native, array_native] {
            let mut records = structured_report("report", json!({"status":"done"}));
            records.push(notification);
            let s = checked_then(&records);
            assert!(s.ending.structured_handback && !s.ending.cut_off);
            assert_eq!(s.prompt_count(), 1);
            assert_eq!(s.outcome, None, "a notification is not a task verdict");
            records.push(user(text));
            assert!(checked_then(&records).ending.cut_off, "the same text from the user is new work");
        }
        // Native tool results are still processed if a notification envelope carries one.
        let mut notification = task_notification(text);
        notification["message"]["content"] = json!([
            {"type":"text","text":text},
            {"type":"tool_result","tool_use_id":"a","content":"failed","is_error":true}
        ]);
        let s = transcript(&[user("Inspect"), tool("a", "ls"), notification]);
        assert!(s.steps[0].call.failed);
        assert_eq!(s.steps[0].call.output, "failed");
        assert_eq!(s.prompt_count(), 1);
    }

    #[test]
    fn earlier_inflight_results_do_not_erase_an_accepted_handback() {
        let report = structured_report("report", serde_json::json!({"status":"blocked"}));
        for failed in [false, true] {
            for result_position in 1..=3 {
                let mut tail = report.clone();
                tail.insert(result_position, result("earlier", "earlier command ended", failed));
                let mut records = vec![tool("earlier", "make test")];
                records.extend(tail);
                let s = checked_then(&records);
                assert!(s.ending.structured_handback && !s.ending.cut_off, "{result_position}, {failed}");
                assert_eq!(s.proxy_outcome(), None);
            }
        }
        // A call actually issued after the report invalidates its ending, even when its result
        // arrives before the report's acknowledgment.
        let records = [
            report[0].clone(),
            tool("later", "make test"),
            result("later", "ok", false),
            report[1].clone(),
            report[2].clone(),
        ];
        let s = checked_then(&records);
        assert!(s.ending.cut_off && !s.ending.structured_handback);
        // An unexplained result or a duplicate is not evidence of an earlier in-flight call.
        for id in ["unknown", "check"] {
            let mut records = report.clone();
            records.insert(1, result(id, "ok", false));
            assert!(!checked_then(&records).ending.structured_handback);
        }
    }

    #[test]
    fn subagent_handback_acknowledges_delivery_not_report_truth() {
        let call = serde_json::json!({"type":"assistant","message":{"id":"handoff","content":[
            {"type":"tool_use","id":"handoff","name":"SubagentHandback","input":{"message":"All tests pass."}}]}});
        let mut delivered = result("handoff", "", false);
        delivered["message"]["content"][0]["content"] = serde_json::json!([
            {"type":"text","text":"{\"success\":true,\"message\":\"Report delivered to your caller.\"}"}]);
        let s = checked_then(&[call.clone(), delivered.clone()]);
        assert!(s.ending.structured_handback && !s.ending.cut_off);
        assert_eq!(s.proxy_outcome(), None);
        delivered["message"]["content"][0]["is_error"] = true.into();
        assert!(!checked_then(&[call.clone(), delivered]).ending.structured_handback);
        let incomplete = checked_then(&[call, result("handoff", "{\"success\":true}", false)]);
        assert!(incomplete.ending.cut_off && !incomplete.ending.structured_handback);
    }

    #[test]
    fn synthetic_api_error_is_not_a_completed_reply() {
        let mut error = reply("API Error: Response interrupted.");
        error["message"]["model"] = "<synthetic>".into();
        let s = checked_then(&[error]);
        assert!(s.ending.cut_off);
        assert_eq!(s.outcome_evidence().basis, adage_trace::OutcomeBasis::IncompleteEnding);
    }

    #[test]
    fn a_question_is_a_request_for_help_only_with_a_help_phrase() {
        assert!(asks_for_help("I looked around.\n\nCould you tell me what W-1043 involves?"));
        assert!(asks_for_help("Which file should I change?"));
        assert!(!asks_for_help("Done. All tests pass."));
        assert!(!asks_for_help("Is it done? Yes: all tests pass, and the change is committed."));
    }
}
