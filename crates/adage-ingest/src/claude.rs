//! Claude Code transcripts (`~/.claude/projects/<project>/<session>.jsonl`), decoded into the
//! canonical [`Session`].
//!
//! One record per line. Assistant records carry `tool_use` blocks (several blocks of one model turn
//! share `message.id`); user records carry the prompt (string content) or `tool_result` blocks keyed
//! by `tool_use_id`. A session steered by Adage records every directive it received, where, and what
//! the directive named ([`Directed`]): learning never counts what a directive named, from its
//! delivery on, as evidence (Design §11: never learn from your own influence; Track G §3.2,
//! steering by influence).

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

/// Whether a transcript path is a sub-agent's own trace (`<session>/subagents/agent-*.jsonl`).
pub fn is_subagent(path: &Path) -> bool {
    path.parent().and_then(|p| p.file_name()).is_some_and(|n| n == "subagents")
}

/// Decode one Claude Code transcript from any reader: a local file, a remote store, memory. The
/// repository is found by `cx.locator`; nothing else here touches the filesystem.
pub fn decode(reader: impl BufRead, cx: &Decode<'_>) -> Option<Session> {
    let (table, repo_override, subagent) = (cx.table, cx.repo_override, cx.subagent);
    let mut id = String::new();
    let mut cwd: Option<String> = None;
    let mut model: Option<String> = None;
    let mut day = String::new();
    let mut prompt: Option<String> = None;
    // (turn, tool_use_id, name, input, prompt index) in order
    let mut uses: Vec<(u32, String, String, Value, u16, Option<String>)> = Vec::new();
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
        if !subagent && rec.get("isSidechain").and_then(|v| v.as_bool()) == Some(true) {
            continue;
        }
        if id.is_empty() {
            // A sub-agent shares its parent's session id; its agent id tells the traces apart.
            let agent = rec.get("agentId").and_then(|v| v.as_str()).filter(|_| subagent);
            if let Some(s) = rec.get("sessionId").and_then(|v| v.as_str()) {
                id = match agent {
                    Some(a) => format!("{s}/{a}"),
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
                // A Stop-hook continuation Adage wrote, should Claude Code record it as a user
                // message: Adage's direction, never the person's prompt.
                let own = |t: &str| wire::is_finish_line(t);
                if let Some(s) = content.as_str().filter(|s| own(s)) {
                    signals.at(uses.len());
                    signals.note(s);
                } else if let Some(s) = content.as_str() {
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
                                rec.get("cwd").and_then(Value::as_str).map(str::to_string),
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
        // evidence of what the agent does. The id itself is not kept, so an agent's calls read
        // and cache exactly as before.
        let by = By::of_tool_use_id(&tid);
        let call =
            Call { tool: name.clone(), input: input.clone(), output, failed, by, tool_use_id: None, agent: None };
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
        identity: Default::default(),
        ending,
    })
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
            jsonl(&s.path().join("subagents"), &mut out);
        }
    }
    out.sort();
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    use adage_trace::session::{Directed, FINISH_LINE_STOP};

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
        assert!(s.steps.iter().all(|x| x.call.tool_use_id.is_none()));
        let json = serde_json::to_string(&s.steps[0]).unwrap();
        assert!(!json.contains("\"by\""), "an agent's step reads as before: {json}");
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
