//! Claude Code transcript import.
//!
//! `~/.claude/projects/<project>/<session>.jsonl`: one record per line. Assistant records carry
//! `tool_use` blocks (several blocks of one model turn share `message.id`); user records carry the
//! prompt (string content) or `tool_result` blocks keyed by `tool_use_id`. A session steered by
//! Adage records every directive it received, where, and what the directive named ([`Directed`]):
//! learning never counts what a directive named, from its delivery on, as evidence (Design §11:
//! never learn from your own influence; Track G §3.2, steering by influence).

use adage_core::keys::{self, template_key};
use adage_core::tools::normalize_tool;
use adage_core::{repo, tokens, CommandTable, Normalized};
use adage_runtime::Call;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepObs {
    /// Model turn index (0-based), counting assistant messages that issued tool calls.
    pub turn: u32,
    pub key: String,
    pub norm: Normalized,
    pub call: Call,
    /// Repository-relative path read or written, if any.
    pub rel_path: Option<String>,
    /// Index into [`Session::prompts`] of the prompt this step served: the latest prompt typed
    /// before its tool call (0 for a call before any prompt). Saturates at `u16::MAX`.
    #[serde(default)]
    pub prompt: u16,
}

/// One prompt the person typed, and where it falls among the session's steps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptObs {
    /// The prompt's text, at most [`MAX_PROMPT_BYTES`] (cut on a character boundary).
    pub text: String,
    /// The index of the first step that served this prompt or a later one: the prompt was
    /// delivered before `steps[at_step]` (`steps.len()` when no step followed it).
    pub at_step: u32,
}

/// A stored prompt's text is cut to this many bytes (what the shared tokeniser reads).
pub const MAX_PROMPT_BYTES: usize = 64 * 1024;
/// At most this many prompts are kept per session.
pub const MAX_PROMPTS: usize = u16::MAX as usize;

/// Collects a session's prompts while a transcript is read, and anchors them to its steps
/// afterwards ([`Prompts::anchor`]). Every importer uses it, so `StepObs::prompt` and
/// `PromptObs::at_step` mean the same thing whatever the harness.
#[derive(Debug, Default)]
pub(crate) struct Prompts {
    texts: Vec<String>,
}

impl Prompts {
    pub(crate) fn push(&mut self, text: &str) {
        if self.texts.len() < MAX_PROMPTS {
            self.texts.push(adage_core::tokens::bounded(text, MAX_PROMPT_BYTES).to_string());
        }
    }

    /// The index of the prompt a call recorded now serves.
    pub(crate) fn current(&self) -> u16 {
        self.texts.len().saturating_sub(1).min(u16::MAX as usize) as u16
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.texts.is_empty()
    }

    /// The prompts, each anchored at the first step that served it or a later prompt.
    pub(crate) fn anchor(self, steps: &[StepObs]) -> Vec<PromptObs> {
        self.texts
            .into_iter()
            .enumerate()
            .map(|(i, text)| {
                let at = steps.iter().position(|s| s.prompt as usize >= i).unwrap_or(steps.len());
                PromptObs { text, at_step: at as u32 }
            })
            .collect()
    }
}

/// Serializable so that a parse can be cached (`crate::cache`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub root: PathBuf,
    pub repo: String,
    pub model: Option<String>,
    pub day: String,
    pub prompt: String,
    pub steps: Vec<StepObs>,
    pub steered: bool,
    /// Adage routes whose directives this session received.
    pub directed: BTreeSet<String>,
    /// Every directive the session received: where, and what it named ([`Directed`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub influence: Vec<Directed>,
    pub outcome: Option<bool>,
    /// The harness the session ran in, as recorded in a track's `gauge.harness`: `claude-code`
    /// for Claude Code transcripts, `opencode` for `opencode export` files.
    pub harness: String,
    /// Prompts the person typed in the session, in order, each anchored to the steps (Track G
    /// §1.1). The first is also [`Session::prompt`].
    #[serde(default)]
    pub prompts: Vec<PromptObs>,
    /// How the session ended, as far as the transcript shows (outcome proxies,
    /// [`Session::proxy_outcome`]).
    #[serde(default)]
    pub ending: Ending,
}

/// How a session ended, read from its transcript: the evidence outcome proxies use when no
/// outcome was recorded.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ending {
    /// The agent's last message asks the person for help or for what to do (`Could you tell me
    /// what W-1043 involves?`) instead of reporting work done.
    #[serde(default)]
    pub asked_for_help: bool,
    /// The transcript ends on a tool call or its result with no final reply: the session was cut
    /// off (a turn limit, a kill).
    #[serde(default)]
    pub cut_off: bool,
}

/// Phrases of a final message that asks the person for help, in its last question.
const HELP_PHRASES: [&str; 14] = [
    "could you",
    "can you",
    "would you",
    "please provide",
    "please share",
    "please clarify",
    "please let me know",
    "do you have",
    "what would you like",
    "what should i",
    "should i",
    "which ",
    "tell me",
    "more details",
];

/// Whether a final reply asks the person for help: its last sentence that ends in `?` holds a
/// help phrase, and it is near the end. "Would you like me to also …?" after a report of done work
/// counts too; that is why this is only a proxy, used when no outcome was recorded.
pub fn asks_for_help(text: &str) -> bool {
    let t = text.trim_end();
    let tail: String = t.chars().rev().take(600).collect::<Vec<_>>().into_iter().rev().collect();
    let Some(q) = tail.rfind('?') else { return false };
    let start = tail[..q].rfind(['.', '!', '\n', '?']).map(|i| i + 1).unwrap_or(0);
    let question = tail[start..=q].to_ascii_lowercase();
    // Only when the question is the last thing said (a list of questions counts).
    let after = tail[q + 1..].trim();
    let trailing_ok = after.is_empty() || after.len() < 200;
    trailing_ok && HELP_PHRASES.iter().any(|p| question.contains(p))
}

/// `gauge.harness` names, per session source.
pub const CLAUDE_CODE: &str = "claude-code";
pub const OPENCODE: &str = "opencode";

/// One `<adage-reflex ...>` opening tag, and its `route` attribute. Text is read plain; a JSON
/// record (a Claude Code attachment line) is read through its string values.
static SIGNAL_TAG: Lazy<Regex> = Lazy::new(|| Regex::new(r"<adage-reflex\b([^>]*)>").unwrap());
static ROUTE_ATTR: Lazy<Regex> = Lazy::new(|| Regex::new(r#"\broute=\\?"([A-Za-z0-9_.-]+)"#).unwrap());
/// `exec="e1"`: Adage ran the directed route itself and delivered its output (Track C §3.4).
static EXEC_ATTR: Lazy<Regex> = Lazy::new(|| Regex::new(r#"\bexec=\\?""#).unwrap());

/// A directive a session received (Track G §3.2, steering by influence): delivered before step
/// `at_step`, it named these commands, avoided runners and files
/// ([`adage_runtime::render::named`]). `all`: it named nothing readable (a wrapper cut short, a
/// data block without its directive, a template the reader does not know) or Adage ran it itself
/// (an `exec` attribute: from there on the agent's actions are not its own), so it counts as
/// naming everything from there on.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Directed {
    pub at_step: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub route: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub avoided: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub all: bool,
}

impl Directed {
    /// HeadSub identities of every command and runner it named.
    pub fn identities(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .commands
            .iter()
            .chain(&self.avoided)
            .map(|c| adage_graph::events::act_identity(&c.split_whitespace().map(str::to_string).collect::<Vec<_>>()))
            .filter(|x| !x.is_empty())
            .collect();
        v.sort();
        v.dedup();
        v
    }

    /// As the graph engine reads it.
    pub fn influence(&self) -> adage_graph::events::Influence {
        adage_graph::events::Influence {
            at_step: self.at_step,
            identities: self.identities(),
            paths: self.files.clone(),
            all: self.all,
        }
    }
}

/// What a transcript shows Adage delivered: the directives, where each arrived, and what it
/// named. An importer sets [`Signals::at`] to the number of tool calls recorded so far before
/// each [`Signals::note`], and [`Signals::anchor`] maps those call counts to steps.
#[derive(Debug, Default)]
pub struct Signals {
    pub steered: bool,
    pub directed: BTreeSet<String>,
    /// Directives, each at the index of the first tool call after it.
    pub found: Vec<Directed>,
    at: u32,
}

/// The string values of a JSON record, or the text itself.
fn texts(text: &str) -> Vec<String> {
    fn walk(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::String(s) if s.contains("adage") || s.contains("Adage") => out.push(s.clone()),
            Value::Array(xs) => xs.iter().for_each(|x| walk(x, out)),
            Value::Object(m) => m.values().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    match serde_json::from_str::<Value>(text) {
        Ok(v) if v.is_object() || v.is_array() => {
            let mut out = Vec::new();
            walk(&v, &mut out);
            out
        }
        _ => vec![text.to_string()],
    }
}

impl Signals {
    /// The number of tool calls the transcript has recorded so far.
    pub fn at(&mut self, calls: usize) {
        self.at = calls.min(u32::MAX as usize) as u32;
    }

    pub fn note(&mut self, text: &str) {
        if !(text.contains("<adage-reflex") || text.contains("<adage-data") || text.contains("Adage finish-line check"))
        {
            return;
        }
        for t in texts(text) {
            self.note_text(&t);
        }
    }

    fn push(&mut self, route: String, named: Option<adage_runtime::render::Named>, all: bool) {
        let named = named.unwrap_or_default();
        let all = all || named.is_empty();
        self.found.push(Directed {
            at_step: self.at,
            route,
            commands: named.commands,
            avoided: named.avoided,
            files: named.files,
            all,
        });
    }

    fn note_text(&mut self, text: &str) {
        if text.contains("<adage-reflex") {
            self.steered = true;
            let tags: Vec<(usize, usize, String)> = SIGNAL_TAG
                .captures_iter(text)
                .map(|c| {
                    let m = c.get(0).expect("match");
                    (m.start(), m.end(), c[1].to_string())
                })
                .collect();
            if tags.is_empty() {
                // A wrapper cut short: it could have named anything.
                self.push(String::new(), None, true);
            }
            for (_, end, attrs) in tags {
                let route = ROUTE_ATTR.captures(&attrs).map(|r| r[1].to_string()).unwrap_or_default();
                if !route.is_empty() {
                    self.directed.insert(route.clone());
                }
                let body =
                    text[end..].split("</adage-reflex>").next().filter(|_| text[end..].contains("</adage-reflex>"));
                let named = body.map(adage_runtime::render::named);
                self.push(route, named, EXEC_ATTR.is_match(&attrs) || body.is_none());
            }
        }
        if text.contains("<adage-data") && !SIGNAL_TAG.captures_iter(text).any(|t| EXEC_ATTR.is_match(&t[1])) {
            // Execution output with no whole E1 wrapper beside it: it could have steered anything.
            self.steered = true;
            self.push(String::new(), None, true);
        }
        if text.contains("Adage finish-line check") {
            self.steered = true;
            self.directed.insert(FINISH_LINE_STOP.to_string());
            self.push(FINISH_LINE_STOP.to_string(), Some(adage_runtime::render::named(text)), false);
        }
    }

    /// The directives with `at_step` a step index: `first_step[k]` is the first step of tool call
    /// `k` (a directive after the last call is at `steps`).
    pub fn anchor(&mut self, first_step: &[u32], steps: usize) -> Vec<Directed> {
        let mut out = std::mem::take(&mut self.found);
        for d in &mut out {
            d.at_step = first_step.get(d.at_step as usize).copied().unwrap_or(steps as u32);
        }
        out
    }
}

/// The entry in [`Session::directed`] for a finish-line stop request.
pub const FINISH_LINE_STOP: &str = "finish-line-stop";

impl Session {
    /// Whether a directive this session received named any of `commands` (by HeadSub identity)
    /// or, with `reads`, any file: the session is then no evidence for a route directing them
    /// (Track G §3.2).
    pub fn exposed_to(&self, commands: &[&str], reads: bool) -> bool {
        let ids: Vec<String> = commands
            .iter()
            .map(|c| adage_graph::events::act_identity(&c.split_whitespace().map(str::to_string).collect::<Vec<_>>()))
            .collect();
        self.influence.iter().any(|d| {
            d.all || (reads && !d.files.is_empty()) || d.identities().iter().any(|i| ids.iter().any(|x| x == i))
        })
    }

    /// Whether a directive this session received named a command of HeadSub identity `identity`.
    pub fn exposed_identity(&self, identity: &str) -> bool {
        self.influence.iter().any(|d| d.all || d.identities().iter().any(|i| i == identity))
    }

    /// The directives as the graph engine reads them.
    pub fn graph_influence(&self) -> Vec<adage_graph::events::Influence> {
        self.influence.iter().map(Directed::influence).collect()
    }
}

impl Session {
    /// The session's outcome as its transcript suggests it, for when none was recorded: failed if
    /// the agent ended by asking the person for help or was cut off, or if its last check failed;
    /// succeeded if its last check passed; unknown otherwise (no check ran).
    pub fn proxy_outcome(&self) -> Option<bool> {
        if self.ending.asked_for_help || self.ending.cut_off {
            return Some(false);
        }
        let last_check = self.steps.iter().rev().find(|s| {
            s.norm.effect == adage_core::Effect::Verify
                && matches!(s.norm.action, adage_core::CanonicalAction::Exec { .. })
        })?;
        Some(!last_check.call.failed)
    }

    /// How many prompts the person typed.
    pub fn prompt_count(&self) -> u32 {
        self.prompts.len() as u32
    }

    /// Index of the first step that edits or writes a file.
    pub fn first_edit(&self) -> Option<usize> {
        self.steps.iter().position(|s| keys::is_author(&s.norm.action))
    }
    /// Steps before the first edit: the discovery window.
    pub fn discovery(&self) -> &[StepObs] {
        &self.steps[..self.first_edit().unwrap_or(self.steps.len())]
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

/// Parse one transcript file. Returns `None` for sidechains or sessions without a usable prompt.
///
/// The file is streamed line by line, so memory is bounded by the session's own tool calls, not
/// by the file (a long transcript is mostly tool output, kept only once per call).
pub fn parse_transcript(path: &Path, table: &CommandTable, repo_override: Option<&str>) -> Option<Session> {
    use std::io::BufRead;
    let reader = std::io::BufReader::new(std::fs::File::open(path).ok()?);
    let mut id = String::new();
    let mut cwd: Option<String> = None;
    let mut model: Option<String> = None;
    let mut day = String::new();
    let mut prompt: Option<String> = None;
    // (turn, tool_use_id, name, input, prompt index) in order
    let mut uses: Vec<(u32, String, String, Value, u16)> = Vec::new();
    let mut results: HashMap<String, (String, bool)> = HashMap::new();
    let mut turn_of_msg: HashMap<String, u32> = HashMap::new();
    let mut signals = Signals::default();
    let mut prompts = Prompts::default();
    // The last thing the transcript records: the agent's final reply (its text), or a tool call or
    // result with no reply after it.
    let mut last_reply: Option<String> = None;
    let mut ends_on_tool = false;

    for line in reader.lines() {
        let Ok(line) = line else { break };
        let line = line.as_str();
        let Ok(rec) = serde_json::from_str::<Value>(line) else { continue };
        if rec.get("isSidechain").and_then(|v| v.as_bool()) == Some(true) {
            continue;
        }
        if id.is_empty() {
            if let Some(s) = rec.get("sessionId").and_then(|v| v.as_str()) {
                id = s.to_string();
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
        // Directives are delivered as hook attachments; text elsewhere (a tool printing Adage's
        // own docs, say) is not steering.
        let is_attachment = matches!(rec.get("type").and_then(|v| v.as_str()), Some("attachment") | Some("system"));
        if is_attachment {
            signals.at(uses.len());
            signals.note(line);
        }
        match rec.get("type").and_then(|v| v.as_str()) {
            Some("user") => {
                let content = &rec["message"]["content"];
                if let Some(s) = content.as_str() {
                    if rec.get("isMeta").and_then(|v| v.as_bool()) != Some(true) && !is_synthetic_prompt(s) {
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
                            let failed = it.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false);
                            results.insert(tid, (text_of(it.get("content").unwrap_or(&Value::Null)), failed));
                        } else if let Some(t) = it.get("text").and_then(|v| v.as_str()) {
                            if !is_synthetic_prompt(t) {
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
                                last_reply = Some(tokens::bounded(t, 4096).to_string());
                                ends_on_tool = false;
                            }
                        }
                        if it.get("type").and_then(|v| v.as_str()) == Some("tool_use") {
                            ends_on_tool = true;
                            last_reply = None;
                            let next = turn_of_msg.len() as u32;
                            let turn = *turn_of_msg.entry(mid.clone()).or_insert(next);
                            uses.push((
                                turn,
                                it.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                it.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                it.get("input").cloned().unwrap_or(Value::Null),
                                prompts.current(),
                            ));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    let cwd = cwd?;
    let prompt = prompt?;
    let cwd_path = PathBuf::from(&cwd);
    let root = repo::find_root(&cwd_path).unwrap_or(cwd_path);
    let repo_key = repo::key_for(&root, repo_override);
    let root_str = root.to_string_lossy().to_string();
    let mut steps = Vec::new();
    let mut first_step: Vec<u32> = Vec::with_capacity(uses.len());
    for (turn, tid, name, input, at) in uses {
        first_step.push(steps.len() as u32);
        let (output, failed) = results.get(&tid).cloned().unwrap_or_default();
        let call = Call { tool: name.clone(), input: input.clone(), output, failed };
        steps.extend(call_steps(turn, at, &call, normalize_tool(&name, &input, &root_str, table), &root));
    }
    let prompts = prompts.anchor(&steps);
    let influence = signals.anchor(&first_step, steps.len());
    let ending = Ending {
        asked_for_help: last_reply.as_deref().is_some_and(asks_for_help),
        cut_off: ends_on_tool && !steps.is_empty(),
    };
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
        harness: CLAUDE_CODE.into(),
        prompts,
        ending,
    })
}

/// The steps one tool call contributes: its canonical actions, flattened and canonicalised
/// against the repository root. A failed call's failure is attributed to its last sub-action only,
/// unless the command has an `||` ([`keys::sub_action_failed`]): `make gen && pytest` failing
/// does not make `make gen` a failed run. Every importer (Claude Code, OpenCode, Codex) builds its
/// steps here.
pub(crate) fn call_steps(turn: u32, prompt: u16, call: &Call, actions: Vec<Normalized>, root: &Path) -> Vec<StepObs> {
    let flat = keys::flatten(&actions);
    let count = flat.len();
    let or_chain = keys::has_or_chain(&call.input);
    flat.into_iter()
        .enumerate()
        .map(|(i, mut n)| {
            keys::canonicalise(&mut n, root);
            let rel_path = keys::action_path(&n.action).map(|p| repo::relativize(root, p));
            let mut call = call.clone();
            call.failed = keys::sub_action_failed(i, count, call.failed, or_chain);
            StepObs { turn, key: template_key(&n.action), norm: n, call, rel_path, prompt }
        })
        .collect()
}

/// Every top-level (non-sidechain) transcript under a Claude Code projects directory.
pub fn discover(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(projects) = std::fs::read_dir(root) else { return out };
    for p in projects.flatten() {
        let Ok(files) = std::fs::read_dir(p.path()) else { continue };
        for f in files.flatten() {
            let path = f.path();
            if path.extension().is_some_and(|e| e == "jsonl") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steps(command: &str, failed: bool) -> Vec<(String, bool)> {
        let table = CommandTable::load();
        let input = serde_json::json!({ "command": command });
        let call = Call { tool: "Bash".into(), input: input.clone(), output: String::new(), failed };
        call_steps(0, 0, &call, normalize_tool("Bash", &input, "/r", &table), Path::new("/r"))
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
        let s = parse_transcript(&p, &CommandTable::load(), Some("r")).unwrap();
        assert_eq!(s.prompt, "Fix BIL-1041");
        let got: Vec<(&str, u32)> = s.prompts.iter().map(|p| (p.text.as_str(), p.at_step)).collect();
        // `a && ls` is two steps, both serving the first prompt.
        assert_eq!(got, [("Fix BIL-1041", 0), ("now run the checks", 2), ("thanks", 3)]);
        assert_eq!(s.steps.iter().map(|x| x.prompt).collect::<Vec<_>>(), [0, 0, 1]);
        assert_eq!(s.prompt_count(), 3);
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
        let (d, f) = read(
            "<adage-reflex track=\"t@1.0.0\" route=\"finish-line-1\" strength=\"fire\" kind=\"obligation\">\nIn this repository, changes to files matching schema/*.json must be followed by running:\n  make gen\nx\n</adage-reflex>",
        );
        assert_eq!(d, v(&["finish-line-1"]));
        assert_eq!(f.len(), 1);
        assert_eq!((f[0].at_step, f[0].commands.clone(), f[0].all), (5, v(&["make gen"]), false));
        assert_eq!(f[0].identities(), v(&["make gen"]));
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

    #[test]
    fn a_session_is_exposed_only_to_what_its_directives_named() {
        let mut s = Session {
            id: "s".into(),
            root: PathBuf::from("/r"),
            repo: "r".into(),
            model: None,
            day: String::new(),
            prompt: "p".into(),
            steps: vec![],
            steered: true,
            directed: ["dead-end-1".to_string()].into(),
            influence: vec![Directed {
                at_step: 0,
                route: "dead-end-1".into(),
                commands: v(&["make test"]),
                avoided: v(&["python3 -m pytest"]),
                ..Default::default()
            }],
            outcome: None,
            harness: CLAUDE_CODE.into(),
            prompts: vec![PromptObs { text: "p".into(), at_step: 0 }],
            ending: Default::default(),
        };
        assert!(s.exposed_to(&["make test"], false) && s.exposed_to(&["python3 -m pytest tests"], false));
        assert!(!s.exposed_to(&["make gen"], false) && !s.exposed_to(&["./scripts/wi-lookup {item}"], true));
        s.influence[0].all = true;
        assert!(s.exposed_to(&["make gen"], false));
    }

    fn transcript(lines: &[serde_json::Value]) -> Session {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("t.jsonl");
        let text: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
        std::fs::write(&p, text.join("\n")).unwrap();
        parse_transcript(&p, &CommandTable::load(), Some("acme")).unwrap()
    }

    fn user(text: &str) -> serde_json::Value {
        serde_json::json!({"type": "user", "sessionId": "s1", "cwd": "/tmp/acme", "message": {"role": "user", "content": text}})
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

    #[test]
    fn a_question_is_a_request_for_help_only_with_a_help_phrase() {
        assert!(asks_for_help("I looked around.\n\nCould you tell me what W-1043 involves?"));
        assert!(asks_for_help("Which file should I change?"));
        assert!(!asks_for_help("Done. All tests pass."));
        assert!(!asks_for_help("Is it done? Yes: all tests pass, and the change is committed."));
    }
}
