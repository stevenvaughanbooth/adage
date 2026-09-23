//! Claude Code transcript import.
//!
//! `~/.claude/projects/<project>/<session>.jsonl`: one record per line. Assistant records carry
//! `tool_use` blocks (several blocks of one model turn share `message.id`); user records carry the
//! prompt (string content) or `tool_result` blocks keyed by `tool_use_id`. Sessions steered by
//! Adage record which routes steered them; each miner excludes sessions steered by its own kind of
//! route (Design §11: never learn from your own influence), and keeps them for other kinds.

use adage_core::keys::{self, template_key};
use adage_core::tools::normalize_tool;
use adage_core::{repo, CommandTable, Normalized};
use adage_runtime::Call;
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct StepObs {
    /// Model turn index (0-based), counting assistant messages that issued tool calls.
    pub turn: u32,
    pub key: String,
    pub norm: Normalized,
    pub call: Call,
    /// Repository-relative path read or written, if any.
    pub rel_path: Option<String>,
}

#[derive(Debug, Clone)]
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
    pub outcome: Option<bool>,
    /// The harness the session ran in, as recorded in a track's `gauge.harness`: `claude-code`
    /// for Claude Code transcripts, `opencode` for `opencode export` files.
    pub harness: String,
    /// Prompts the person typed in the session (the first is [`Session::prompt`]).
    pub prompts: u32,
}

/// `gauge.harness` names, per session source.
pub const CLAUDE_CODE: &str = "claude-code";
pub const OPENCODE: &str = "opencode";

/// The kind of a route, from its id: evidence exclusion works per kind.
pub fn route_kind(id: &str) -> &'static str {
    if id.starts_with("lookup") || id.starts_with("read-linked") {
        "work-items"
    } else if id.starts_with("finish-line") {
        "finish-line"
    } else if id.starts_with("intent-") {
        "intents"
    } else if id.starts_with("dead-end") {
        "dead-ends"
    } else {
        "other"
    }
}

impl Session {
    pub fn steered_by(&self, kind: &str) -> bool {
        self.directed.iter().any(|r| route_kind(r) == kind)
    }
}

static ROUTE_ATTR: Lazy<Regex> = Lazy::new(|| Regex::new(r#"route=\\?"([A-Za-z0-9_.-]+)"#).unwrap());

impl Session {
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
    let mut steered = false;
    // (turn, tool_use_id, name, input) in order
    let mut uses: Vec<(u32, String, String, Value)> = Vec::new();
    let mut results: HashMap<String, (String, bool)> = HashMap::new();
    let mut turn_of_msg: HashMap<String, u32> = HashMap::new();
    let mut directed: BTreeSet<String> = BTreeSet::new();
    let mut prompts = 0u32;

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
        if is_attachment && line.contains("<adage-signal") {
            steered = true;
            for c in ROUTE_ATTR.captures_iter(line) {
                directed.insert(c[1].to_string());
            }
        }
        if is_attachment && line.contains("Adage finish-line check") {
            steered = true;
            directed.insert("finish-line-stop".to_string());
        }
        match rec.get("type").and_then(|v| v.as_str()) {
            Some("user") => {
                let content = &rec["message"]["content"];
                if let Some(s) = content.as_str() {
                    if rec.get("isMeta").and_then(|v| v.as_bool()) != Some(true) && !is_synthetic_prompt(s) {
                        prompts += 1;
                        if prompt.is_none() {
                            prompt = Some(s.to_string());
                        }
                    }
                } else if let Some(items) = content.as_array() {
                    let mut typed = false;
                    for it in items {
                        if it.get("type").and_then(|v| v.as_str()) == Some("tool_result") {
                            let tid = it.get("tool_use_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let failed = it.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false);
                            results.insert(tid, (text_of(it.get("content").unwrap_or(&Value::Null)), failed));
                        } else if let Some(t) = it.get("text").and_then(|v| v.as_str()) {
                            if !is_synthetic_prompt(t) {
                                typed = true;
                                if prompt.is_none() {
                                    prompt = Some(t.to_string());
                                }
                            }
                        }
                    }
                    prompts += u32::from(typed);
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
                        if it.get("type").and_then(|v| v.as_str()) == Some("tool_use") {
                            let next = turn_of_msg.len() as u32;
                            let turn = *turn_of_msg.entry(mid.clone()).or_insert(next);
                            uses.push((
                                turn,
                                it.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                it.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                it.get("input").cloned().unwrap_or(Value::Null),
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
    let repo_key = repo_override.map(str::to_string).unwrap_or_else(|| repo::repo_key(&root));
    let root_str = root.to_string_lossy().to_string();
    let mut steps = Vec::new();
    for (turn, tid, name, input) in uses {
        let (output, failed) = results.get(&tid).cloned().unwrap_or_default();
        let call = Call { tool: name.clone(), input: input.clone(), output, failed };
        for mut n in keys::flatten(&normalize_tool(&name, &input, &root_str, table)) {
            keys::canonicalise(&mut n, &root);
            let rel_path = keys::action_path(&n.action).map(|p| repo::relativize(&root, p));
            steps.push(StepObs { turn, key: template_key(&n.action), norm: n, call: call.clone(), rel_path });
        }
    }
    Some(Session {
        id,
        root,
        repo: repo_key,
        model,
        day,
        prompt,
        steps,
        steered,
        directed,
        outcome: None,
        harness: CLAUDE_CODE.into(),
        prompts,
    })
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
