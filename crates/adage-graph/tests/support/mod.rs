//! Test support for the G2 miner: a Claude Code transcript reader built on adage-core only (the
//! graph crate never depends on adage-learn), and helpers to find datasets and generated bundles.
#![allow(dead_code)]

use adage_core::types::CanonicalAction;
use adage_core::{keys, repo, tools, CommandTable};
use adage_graph::events::{self, EventSession};
use adage_graph::input::{Prompt, Session, Step};
use serde_json::Value;
use std::collections::HashMap;
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

fn synthetic(s: &str) -> bool {
    let t = s.trim_start();
    t.is_empty() || t.starts_with("<local-command") || t.starts_with("<command-") || t.starts_with("<system-reminder")
}

/// A transcript read: the session's events and the repository root it ran in.
pub struct Read {
    pub session: EventSession,
    pub root: PathBuf,
}

/// Read one Claude Code transcript (every prompt, each tool call's canonical actions).
pub fn read_transcript(path: &Path, table: &CommandTable, task: Option<String>) -> Option<Read> {
    let f = std::fs::File::open(path).ok()?;
    let mut id = String::new();
    let mut cwd: Option<String> = None;
    let mut day = String::new();
    // (uses seen before it, text)
    let mut prompts: Vec<(usize, String)> = Vec::new();
    let mut uses: Vec<(u32, String, String, Value)> = Vec::new();
    let mut results: HashMap<String, (String, bool)> = HashMap::new();
    let mut turn_of: HashMap<String, u32> = HashMap::new();
    for line in std::io::BufReader::new(f).lines() {
        let Ok(line) = line else { break };
        let Ok(rec) = serde_json::from_str::<Value>(&line) else { continue };
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
        match rec.get("type").and_then(|v| v.as_str()) {
            Some("user") => {
                let content = &rec["message"]["content"];
                if let Some(s) = content.as_str() {
                    if rec.get("isMeta").and_then(|v| v.as_bool()) != Some(true) && !synthetic(s) {
                        prompts.push((uses.len(), s.to_string()));
                    }
                } else if let Some(items) = content.as_array() {
                    for it in items {
                        if it.get("type").and_then(|v| v.as_str()) == Some("tool_result") {
                            let tid = it.get("tool_use_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let failed = it.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false);
                            results.insert(tid, (text_of(it.get("content").unwrap_or(&Value::Null)), failed));
                        } else if let Some(t) = it.get("text").and_then(|v| v.as_str()) {
                            if !synthetic(t) {
                                prompts.push((uses.len(), t.to_string()));
                            }
                        }
                    }
                }
            }
            Some("assistant") => {
                let msg = &rec["message"];
                let mid = msg.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if let Some(items) = msg.get("content").and_then(|c| c.as_array()) {
                    for it in items {
                        if it.get("type").and_then(|v| v.as_str()) == Some("tool_use") {
                            let next = turn_of.len() as u32;
                            let turn = *turn_of.entry(mid.clone()).or_insert(next);
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
    if prompts.is_empty() {
        return None;
    }
    let cwd_path = PathBuf::from(&cwd);
    let root = repo::find_root(&cwd_path).unwrap_or(cwd_path);
    let root_str = root.to_string_lossy().to_string();
    let mut steps = Vec::new();
    let mut turns = Vec::new();
    let mut effects = Vec::new();
    let mut step_of_use = Vec::with_capacity(uses.len() + 1);
    for (turn, tid, name, input) in &uses {
        step_of_use.push(steps.len());
        let (output, failed) = results.get(tid).cloned().unwrap_or_default();
        let flat = keys::flatten(&tools::normalize_tool(name, input, &root_str, table));
        let count = flat.len();
        let or_chain = keys::has_or_chain(input);
        for (i, mut n) in flat.into_iter().enumerate() {
            keys::canonicalise(&mut n, &root);
            let rel = keys::action_path(&n.action).map(|p| repo::relativize(&root, p));
            let failed = keys::sub_action_failed(i, count, failed, or_chain);
            let step = match &n.action {
                CanonicalAction::Exec { argv, .. } => {
                    Some(Step::Exec { argv: argv.clone(), output: output.clone(), failed })
                }
                CanonicalAction::Read { .. } | CanonicalAction::List { .. } | CanonicalAction::Search { .. } => {
                    rel.map(|path| Step::Read { path })
                }
                CanonicalAction::Edit { .. } | CanonicalAction::Write { .. } => rel.map(|path| Step::Edit { path }),
                _ => None,
            };
            if let Some(step) = step {
                let eff = matches!(step, Step::Exec { .. }).then_some(n.effect);
                steps.push(step);
                turns.push(*turn);
                effects.push(eff);
            }
        }
    }
    step_of_use.push(steps.len());
    let prompts = prompts
        .into_iter()
        .map(|(u, text)| Prompt { text, at_step: step_of_use[u.min(step_of_use.len() - 1)] as u32 })
        .collect();
    let input = Session { id, prompts, steps };
    let meta = events::Meta { day: Some(day), task, turns: Some(turns), effects: Some(effects), ..Default::default() };
    Some(Read { session: events::from_input(&input, &meta, table), root })
}

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A dataset's sessions (every session in its manifest), with their tasks.
pub fn dataset(name: &str) -> Option<Vec<EventSession>> {
    let dir = repo_root().join("evals/datasets").join(name);
    let m: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).ok()?).ok()?;
    let table = CommandTable::load();
    let mut out = Vec::new();
    for s in m["sessions"].as_array()? {
        let file = dir.join(s["file"].as_str()?);
        let task = s["task"].as_str().map(str::to_string).or_else(|| s["id"].as_str().map(str::to_string));
        if let Some(r) = read_transcript(&file, &table, task) {
            out.push(r.session);
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Some(out)
}

/// A generated bundle's rehearsal sessions.
pub fn bundle(dir: &Path) -> Vec<EventSession> {
    let table = CommandTable::load();
    let mut files = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir.join("rehearsal/transcripts")) {
        for p in rd.flatten() {
            if let Ok(inner) = std::fs::read_dir(p.path()) {
                for f in inner.flatten() {
                    if f.path().extension().is_some_and(|e| e == "jsonl") {
                        files.push(f.path());
                    }
                }
            }
        }
    }
    files.sort();
    let mut out: Vec<EventSession> =
        files.iter().filter_map(|f| read_transcript(f, &table, None)).map(|r| r.session).collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// A tiny deterministic generator.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    pub fn chance(&mut self, pct: u64) -> bool {
        self.below(100) < pct
    }
}

fn exec(argv: &str, output: &str, failed: bool) -> Step {
    Step::Exec { argv: argv.split_whitespace().map(str::to_string).collect(), output: output.into(), failed }
}

/// A random repository history with planted conventions (a lookup whose id flows from the
/// prompt and whose output lists the files read next, a runner that always fails before the
/// working one, a regenerate step after a registry edit) amid noise. `events` is the approximate
/// length of each session. Deterministic in `seed`.
pub fn synthetic_sessions(n: usize, events: usize, seed: u64) -> Vec<EventSession> {
    use adage_core::types::Effect;
    let table = CommandTable::load();
    let mut rng = Rng(seed);
    let noise = [
        ("ls", Effect::Observe),
        ("git status", Effect::Observe),
        ("git diff", Effect::Observe),
        ("git log --oneline -5", Effect::Observe),
        ("make bench", Effect::Environment),
        ("npm run lint", Effect::Verify),
        ("pwd", Effect::Observe),
        ("grep -rn TODO src", Effect::Observe),
        ("cargo build", Effect::Verify),
        ("echo done", Effect::Observe),
    ];
    let files: Vec<String> = (0..60).map(|i| format!("src/mod{}/file{}.py", i % 7, i)).collect();
    let mut out = Vec::with_capacity(n);
    for si in 0..n {
        let mut steps: Vec<Step> = Vec::new();
        let mut effects: Vec<Option<Effect>> = Vec::new();
        let mut turns: Vec<u32> = Vec::new();
        let mut turn = 0u32;
        let tracker = rng.chance(70);
        let id = format!("BIL-{}", 1000 + rng.below(9000));
        let text = if tracker { format!("Please work on {id}.") } else { "Fix the rounding bug in the parser.".into() };
        let mut push = |s: Step, e: Option<Effect>, t: u32| {
            steps.push(s);
            effects.push(e);
            turns.push(t);
        };
        let mut len = 0usize;
        while len < events {
            turn += 1;
            let r = rng.below(100);
            // The lookup comes after a little exploring (right after the prompt it would have no
            // value: the agent does it anyway).
            if len == 2 && tracker {
                let a = files[rng.below(files.len() as u64) as usize].clone();
                let b = files[rng.below(files.len() as u64) as usize].clone();
                push(
                    exec(&format!("./scripts/wi-lookup {id}"), &format!("{id}\nFiles: {a} {b}"), false),
                    Some(Effect::Observe),
                    turn,
                );
                turn += 1;
                push(Step::Read { path: a }, None, turn);
                push(Step::Read { path: b }, None, turn);
                len += 3;
            } else if r < 8 {
                push(
                    exec("python3 -m pytest", "E ImportError: no module named acme", true),
                    Some(Effect::Verify),
                    turn,
                );
                turn += 1;
                push(exec("make test", "ok", false), Some(Effect::Verify), turn);
                len += 2;
            } else if r < 14 {
                push(Step::Edit { path: format!("registry/table{}.csv", rng.below(3)) }, None, turn);
                turn += 1;
                push(exec("make gen", "generated", false), Some(Effect::Regenerate), turn);
                len += 2;
            } else if r < 45 {
                let f = files[rng.below(files.len() as u64) as usize].clone();
                let st = if rng.chance(50) { Step::Read { path: f } } else { Step::Edit { path: f } };
                push(st, None, turn);
                len += 1;
            } else {
                let (c, e) = noise[rng.below(noise.len() as u64) as usize];
                let failed = rng.chance(3);
                push(exec(c, "", failed), Some(e), turn);
                len += 1;
            }
        }
        let input = Session { id: format!("s{si:05}"), prompts: vec![Prompt { text, at_step: 0 }], steps };
        let day = format!("2026-09-{:02}", 1 + rng.below(20));
        let meta = events::Meta { day: Some(day), turns: Some(turns), effects: Some(effects), ..Default::default() };
        out.push(events::from_input(&input, &meta, &table));
    }
    out
}
