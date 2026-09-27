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
use adage_tracks::{KIND_DEAD_ENDS, KIND_FINISH_LINE, KIND_INTENTS, KIND_OTHER, KIND_WORK_ITEMS, MINER_KINDS};
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
    /// Prompts the person typed in the session (the first is [`Session::prompt`]).
    pub prompts: u32,
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
/// | `dead-ends` | `dead-ends`, `intents` | Announced at the first prompt, it changes which verify command the agent runs first, which is what the intent miner learns from |
/// | `intents` | `intents` | The first command after the prompt |
/// | `other`, unknown, or unreadable | every kind | A shape no miner produces (an import, a hand-written route) could have influenced anything |
pub fn steers(kind: &str) -> &'static [&'static str] {
    match kind {
        KIND_WORK_ITEMS => &[KIND_WORK_ITEMS],
        KIND_FINISH_LINE => &[KIND_FINISH_LINE],
        KIND_DEAD_ENDS => &[KIND_DEAD_ENDS, KIND_INTENTS],
        KIND_INTENTS => &[KIND_INTENTS],
        _ => &MINER_KINDS,
    }
}

/// One `<adage-signal ...>` opening tag, and its `route` and `kind` attributes. Both forms a
/// transcript holds are read: plain text, and JSON-escaped (`route=\"...\"`).
static SIGNAL_TAG: Lazy<Regex> = Lazy::new(|| Regex::new(r"<adage-signal\b([^>]*)>").unwrap());
static ROUTE_ATTR: Lazy<Regex> = Lazy::new(|| Regex::new(r#"\broute=\\?"([A-Za-z0-9_.-]+)"#).unwrap());
static KIND_ATTR: Lazy<Regex> = Lazy::new(|| Regex::new(r#"\bkind=\\?"([a-z-]*)"#).unwrap());

/// The miner kinds one directive steers, from its wrapper's attributes: its `kind` (rendered by
/// `adage_runtime::render::wrap_kind`); in a wrapper from before `kind`, the kind its route id
/// implies ([`route_kind`]); with neither, every kind.
pub fn steered_kinds(attrs: &str) -> &'static [&'static str] {
    if let Some(k) = KIND_ATTR.captures(attrs) {
        return steers(&k[1]);
    }
    match ROUTE_ATTR.captures(attrs) {
        Some(r) => steers(route_kind(&r[1])),
        None => &MINER_KINDS,
    }
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
    // (turn, tool_use_id, name, input) in order
    let mut uses: Vec<(u32, String, String, Value)> = Vec::new();
    let mut results: HashMap<String, (String, bool)> = HashMap::new();
    let mut turn_of_msg: HashMap<String, u32> = HashMap::new();
    let mut signals = Signals::default();
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
        if is_attachment {
            signals.note(line);
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
        steps.extend(call_steps(turn, &call, normalize_tool(&name, &input, &root_str, table), &root));
    }
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
    })
}

/// The steps one tool call contributes: its canonical actions, flattened and canonicalised
/// against the repository root. A failed call's failure is attributed to its last sub-action only,
/// unless the command has an `||` ([`keys::sub_action_failed`]): `make gen && pytest` failing
/// does not make `make gen` a failed run. Every importer (Claude Code, OpenCode, Codex) builds its
/// steps here.
pub(crate) fn call_steps(turn: u32, call: &Call, actions: Vec<Normalized>, root: &Path) -> Vec<StepObs> {
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
            StepObs { turn, key: template_key(&n.action), norm: n, call, rel_path }
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
        call_steps(0, &call, normalize_tool("Bash", &input, "/r", &table), Path::new("/r"))
            .into_iter()
            .map(|s| (s.key, s.call.failed))
            .collect()
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
            // Other and unknown kinds steer every miner.
            ("other", v(&["dead-ends", "finish-line", "intents", "work-items"])),
            ("mystery", v(&["dead-ends", "finish-line", "intents", "work-items"])),
            ("", v(&["dead-ends", "finish-line", "intents", "work-items"])),
        ] {
            let (_, k) = kinds(&format!(r#"<adage-signal track="t@1.0.0" route="r" aspect="yellow" kind="{kind}">"#));
            assert_eq!(k, want, "{kind}");
        }
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
            prompts: 1,
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
}
