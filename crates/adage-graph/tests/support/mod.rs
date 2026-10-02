//! Test support for the G2 miner: traces read through ingestion and the engine's canonical bridge
//! (as the learner reads them), helpers to find datasets and generated bundles, and synthetic
//! sessions.
#![allow(dead_code)]

use adage_core::CommandTable;
use adage_graph::canonical;
use adage_graph::events::{self, EventSession};
use adage_graph::input::{Prompt, Session, Step};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// How a trace's directives from Adage are read.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Influence {
    /// As the learner reads them: what a directive named is exposure, never evidence.
    Kept,
    /// Not at all: every step counts as the agent's own. For measuring the engine's recall
    /// against ground truth labelled on what agents did, steered or not (the learner's discount
    /// of its own influence is tested in `adage-learn`).
    Ignored,
}

/// One trace, read as the learner reads it: decoded by ingestion (`adage_ingest`, its format
/// recognised by content) into the canonical session, then bridged to the engine's input
/// (`adage_graph::canonical`), with `task` as its ground-truth label.
pub fn read_transcript(path: &Path, table: &CommandTable, task: Option<String>) -> Option<EventSession> {
    read_trace(path, table, task, Influence::Kept)
}

fn read_trace(path: &Path, table: &CommandTable, task: Option<String>, influence: Influence) -> Option<EventSession> {
    let mut s = adage_ingest::read_session(path, table, None)?;
    if influence == Influence::Ignored {
        s.influence.clear();
    }
    let mut e = canonical::events(&canonical::annotate(&s), &s);
    e.task = task;
    Some(e)
}

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A dataset's sessions (every session in its manifest), with their tasks, as the learner reads
/// them.
pub fn dataset(name: &str) -> Option<Vec<EventSession>> {
    dataset_with(name, Influence::Kept)
}

/// A dataset's sessions, with their directives read as `influence` says.
pub fn dataset_with(name: &str, influence: Influence) -> Option<Vec<EventSession>> {
    let dir = repo_root().join("evals/datasets").join(name);
    let m: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).ok()?).ok()?;
    let table = CommandTable::load();
    let mut out = Vec::new();
    for s in m["sessions"].as_array()? {
        let file = dir.join(s["file"].as_str()?);
        let task = s["task"].as_str().map(str::to_string).or_else(|| s["id"].as_str().map(str::to_string));
        if let Some(s) = read_trace(&file, &table, task, influence) {
            out.push(s);
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
    let mut out: Vec<EventSession> = files.iter().filter_map(|f| read_transcript(f, &table, None)).collect();
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
