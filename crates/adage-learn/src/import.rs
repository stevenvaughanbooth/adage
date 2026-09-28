//! Claude Code transcript import.
//!
//! `~/.claude/projects/<project>/<session>.jsonl`: one record per line. Assistant records carry
//! `tool_use` blocks (several blocks of one model turn share `message.id`); user records carry the
//! prompt (string content) or `tool_result` blocks keyed by `tool_use_id`. Sessions steered by
//! Adage record which routes steered them; each miner excludes sessions steered by its own kind of
//! route (Design §11: never learn from your own influence), and keeps them for other kinds.

use adage_core::keys::{self, template_key};
use adage_core::tools::normalize_tool;
use adage_core::{repo, tokens, CommandTable, Normalized};
use adage_runtime::Call;
use adage_tracks::{
    KIND_ADVICE, KIND_DEAD_ENDS, KIND_FINISH_LINE, KIND_INTENTS, KIND_OTHER, KIND_WORK_ITEMS, MINER_KINDS,
};
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
    /// Adage routes whose directives this session received. Evidence for a route kind is never
    /// taken from a session steered by that kind (Design §11: never learn from your own influence).
    pub directed: BTreeSet<String>,
    /// The miner kinds those directives could have influenced ([`steers`]). Each miner ignores
    /// the sessions steered on its kind ([`Session::steered_by`]).
    #[serde(default)]
    pub steered_kinds: BTreeSet<String>,
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

/// The kind of a route, from its id alone: the fallback for directives rendered before the
/// wrapper carried a `kind` attribute. An id no miner produces is `other`.
pub fn route_kind(id: &str) -> &'static str {
    if id.starts_with("lookup") || id.starts_with("read-linked") {
        KIND_WORK_ITEMS
    } else if id.starts_with("finish-line") {
        KIND_FINISH_LINE
    } else if id.starts_with("intent-") {
        KIND_INTENTS
    } else if id.starts_with("dead-end") {
        KIND_DEAD_ENDS
    } else {
        KIND_OTHER
    }
}

/// The miners a directive of route kind `kind` can influence. No miner takes evidence from a
/// session that received a directive steering it (Design §11: never learn from your own
/// influence).
///
/// | Directive kind | Miners it steers | Why |
/// | --- | --- | --- |
/// | `work-items` | `work-items` | The lookup and the linked-file reads it directs |
/// | `finish-line` | `finish-line` | The step after an edit (and the stop request) |
/// | `dead-ends` | `dead-ends`, `intents` | Announced at the first prompt, it changes which verify command the agent runs first, which is what the intent miner learns from; an instinct's dead end (`after_failure` + `run`, the same route ids) steers the same miners |
/// | `intents` | `intents` | The first command after the prompt |
/// | `advice` | none but its own | Fixed advice (a generated file, a lockfile, the package manager) directs no command any miner learns |
/// | `other`, unknown, or unreadable | every kind | A shape no miner produces (an import, a hand-written route) could have influenced anything |
pub fn steers(kind: &str) -> &'static [&'static str] {
    match kind {
        KIND_WORK_ITEMS => &[KIND_WORK_ITEMS],
        KIND_FINISH_LINE => &[KIND_FINISH_LINE],
        KIND_DEAD_ENDS => &[KIND_DEAD_ENDS, KIND_INTENTS],
        KIND_INTENTS => &[KIND_INTENTS],
        KIND_ADVICE => &[KIND_ADVICE],
        _ => &MINER_KINDS,
    }
}

/// One `<adage-signal ...>` opening tag, and its `route` and `kind` attributes. Both forms a
/// transcript holds are read: plain text, and JSON-escaped (`route=\"...\"`).
static SIGNAL_TAG: Lazy<Regex> = Lazy::new(|| Regex::new(r"<adage-signal\b([^>]*)>").unwrap());
static ROUTE_ATTR: Lazy<Regex> = Lazy::new(|| Regex::new(r#"\broute=\\?"([A-Za-z0-9_.-]+)"#).unwrap());
static KIND_ATTR: Lazy<Regex> = Lazy::new(|| Regex::new(r#"\bkind=\\?"([a-z-]*)"#).unwrap());
/// `exec="e1"`: Adage ran the directed route itself and delivered its output (Track C §3.4).
static EXEC_ATTR: Lazy<Regex> = Lazy::new(|| Regex::new(r#"\bexec=\\?""#).unwrap());

/// The miner kinds one directive steers, from its wrapper's attributes: its `kind` (rendered by
/// `adage_runtime::render::wrap_kind`); in a wrapper from before `kind`, the kind its route id
/// implies ([`route_kind`]); with neither, every kind. A directive Adage also ran (an `exec`
/// attribute, E1) steers the intent miner too, whatever its kind: the agent's first command after
/// the prompt, which that miner learns from, is no longer its own (Track C D11).
pub fn steered_kinds(attrs: &str) -> Vec<&'static str> {
    let mut kinds: Vec<&'static str> = if let Some(k) = KIND_ATTR.captures(attrs) {
        steers(&k[1]).to_vec()
    } else {
        match ROUTE_ATTR.captures(attrs) {
            Some(r) => steers(route_kind(&r[1])).to_vec(),
            None => MINER_KINDS.to_vec(),
        }
    };
    if EXEC_ATTR.is_match(attrs) && !kinds.contains(&KIND_INTENTS) {
        kinds.push(KIND_INTENTS);
    }
    kinds
}

/// What a transcript shows Adage delivered: the directives' routes, and the miners they steered.
/// Fed each piece of text the harness recorded as coming from Adage's hooks.
#[derive(Debug, Default)]
pub struct Signals {
    pub steered: bool,
    pub directed: BTreeSet<String>,
    pub kinds: BTreeSet<String>,
}

impl Signals {
    pub fn note(&mut self, text: &str) {
        if text.contains("<adage-signal") {
            self.steered = true;
            let mut tags = 0;
            for t in SIGNAL_TAG.captures_iter(text) {
                tags += 1;
                if let Some(r) = ROUTE_ATTR.captures(&t[1]) {
                    self.directed.insert(r[1].to_string());
                }
                self.kinds.extend(steered_kinds(&t[1]).iter().map(|k| k.to_string()));
            }
            if tags == 0 {
                // A wrapper cut short: it could have been anything.
                self.kinds.extend(MINER_KINDS.iter().map(|k| k.to_string()));
            }
        }
        if text.contains("<adage-data") && !SIGNAL_TAG.captures_iter(text).any(|t| EXEC_ATTR.is_match(&t[1])) {
            // Execution output with no whole E1 wrapper beside it: it could have steered anything.
            self.steered = true;
            self.kinds.extend(MINER_KINDS.iter().map(|k| k.to_string()));
        }
        if text.contains("Adage finish-line check") {
            self.steered = true;
            self.directed.insert(FINISH_LINE_STOP.to_string());
            self.kinds.insert(KIND_FINISH_LINE.to_string());
        }
    }
}

/// The entry in [`Session::directed`] for a finish-line stop request.
pub const FINISH_LINE_STOP: &str = "finish-line-stop";

impl Session {
    /// Whether a directive this session received could have influenced the miner of `kind`
    /// ([`steers`]).
    pub fn steered_by(&self, kind: &str) -> bool {
        if !self.steered_kinds.is_empty() {
            return self.steered_kinds.contains(kind);
        }
        // Built without kinds (a caller's own `Session`): judge by the route ids.
        self.directed.iter().any(|r| {
            let k = if r == FINISH_LINE_STOP { KIND_FINISH_LINE } else { route_kind(r) };
            steers(k).contains(&kind)
        })
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
    for (turn, tid, name, input, at) in uses {
        let (output, failed) = results.get(&tid).cloned().unwrap_or_default();
        let call = Call { tool: name.clone(), input: input.clone(), output, failed };
        steps.extend(call_steps(turn, at, &call, normalize_tool(&name, &input, &root_str, table), &root));
    }
    let prompts = prompts.anchor(&steps);
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
        steered_kinds: signals.kinds,
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

    fn kinds(text: &str) -> (Vec<String>, Vec<String>) {
        let mut s = Signals::default();
        s.note(text);
        assert!(s.steered);
        (s.directed.into_iter().collect(), s.kinds.into_iter().collect())
    }

    fn v(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn the_wrapper_kind_decides_which_miners_a_directive_steered() {
        // The kind attribute wins over the id: an imported lookup-shaped id with a dead-end kind.
        let (d, k) = kinds(r#"<adage-signal track="t@1.0.0" route="lookup-x" aspect="red" kind="dead-ends">…"#);
        assert_eq!((d, k), (v(&["lookup-x"]), v(&["dead-ends", "intents"])));
        // JSON-escaped, as a Claude Code transcript line holds it.
        let (_, k) = kinds(
            r#"{"content":"<adage-signal track=\"t@1.0.0\" route=\"r\" aspect=\"green\" kind=\"finish-line\">…"}"#,
        );
        assert_eq!(k, v(&["finish-line"]));
        for (kind, want) in [
            ("work-items", v(&["work-items"])),
            ("finish-line", v(&["finish-line"])),
            ("intents", v(&["intents"])),
            ("dead-ends", v(&["dead-ends", "intents"])),
            // Advice steers no miner but its own.
            ("advice", v(&["advice"])),
            // Other and unknown kinds steer every miner.
            ("other", v(&["dead-ends", "finish-line", "intents", "work-items"])),
            ("mystery", v(&["dead-ends", "finish-line", "intents", "work-items"])),
            ("", v(&["dead-ends", "finish-line", "intents", "work-items"])),
        ] {
            let (_, k) = kinds(&format!(r#"<adage-signal track="t@1.0.0" route="r" aspect="yellow" kind="{kind}">"#));
            assert_eq!(k, want, "{kind}");
        }
        // An instinct's wrapper carries its origin after the kind; the kind still decides.
        let (d, k) = kinds(
            r#"<adage-signal track="acme/instincts@0.1.0" route="dead-end-d41b1e" aspect="yellow" kind="dead-ends" origin="instinct">"#,
        );
        assert_eq!((d, k), (v(&["dead-end-d41b1e"]), v(&["dead-ends", "intents"])));
    }

    #[test]
    fn a_directive_adage_ran_also_steers_the_intent_miner() {
        // E1: the wrapper's exec marker, plain and JSON-escaped.
        let (d, k) = kinds(
            "<adage-signal track=\"t@1.0.0\" route=\"lookup-work-item\" aspect=\"green\" kind=\"work-items\" exec=\"e1\">x</adage-signal>\n<adage-data track=\"t@1.0.0\" route=\"lookup-work-item\">{}</adage-data>",
        );
        assert_eq!((d, k), (v(&["lookup-work-item"]), v(&["intents", "work-items"])));
        let (_, k) = kinds(
            r#"{"content":"<adage-signal track=\"t@1.0.0\" route=\"r\" aspect=\"green\" kind=\"finish-line\" exec=\"e1\">…"}"#,
        );
        assert_eq!(k, v(&["finish-line", "intents"]));
        // A data block with no E1 wrapper (cut short): every miner.
        assert_eq!(kinds("<adage-data track=\"t@1.0.0\" route=\"r\">{}").1.len(), 4);
    }

    #[test]
    fn old_wrappers_fall_back_to_the_route_id_and_unknown_ids_steer_everything() {
        let old = |id: &str| kinds(&format!(r#"<adage-signal track="t@1.0.0" route="{id}" aspect="green">…"#)).1;
        assert_eq!(old("lookup-work-item"), v(&["work-items"]));
        assert_eq!(old("read-linked-files"), v(&["work-items"]));
        assert_eq!(old("finish-line-6a01b2"), v(&["finish-line"]));
        assert_eq!(old("intent-1a2b3c"), v(&["intents"]));
        assert_eq!(old("dead-end-d41b1e"), v(&["dead-ends", "intents"]));
        // An imported or authored id no miner produces: excluded from every miner.
        assert_eq!(old("gen"), v(&["dead-ends", "finish-line", "intents", "work-items"]));
        // No route at all, or a wrapper cut short: everything.
        assert_eq!(kinds(r#"<adage-signal aspect="green">"#).1.len(), 4);
        assert_eq!(kinds("…<adage-signal track=").1.len(), 4);
        // Several directives in one attachment: the union.
        let (d, k) = kinds(
            "<adage-signal track=\"a@1.0.0\" route=\"lookup-work-item\" aspect=\"green\">x</adage-signal>\n\n\
             <adage-signal track=\"a@1.0.0\" route=\"dead-end-1\" aspect=\"red\" kind=\"dead-ends\">y</adage-signal>",
        );
        assert_eq!((d, k), (v(&["dead-end-1", "lookup-work-item"]), v(&["dead-ends", "intents", "work-items"])));
        // The finish-line stop request steers the finish-line miner.
        let (d, k) = kinds("Adage finish-line check: this session changed files matching x");
        assert_eq!((d, k), (v(&[FINISH_LINE_STOP]), v(&["finish-line"])));
    }

    #[test]
    fn a_session_is_steered_on_the_kinds_its_directives_steer() {
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
            steered_kinds: BTreeSet::new(),
            outcome: None,
            harness: CLAUDE_CODE.into(),
            prompts: vec![PromptObs { text: "p".into(), at_step: 0 }],
            ending: Default::default(),
        };
        // Built without kinds: judged by route ids.
        assert!(s.steered_by("dead-ends") && s.steered_by("intents"));
        assert!(!s.steered_by("work-items") && !s.steered_by("finish-line"));
        s.directed = ["gen".to_string()].into();
        assert!(MINER_KINDS.iter().all(|k| s.steered_by(k)), "an unknown route steers every miner");
        // Recorded kinds are authoritative.
        s.steered_kinds = ["work-items".to_string()].into();
        assert!(s.steered_by("work-items") && !s.steered_by("intents"));
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
