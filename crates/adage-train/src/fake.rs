//! A scripted stand-in for a real agent, so the pipeline (plan → run → mine → report) runs end to
//! end, deterministically, with nothing spent. It writes Claude Code transcripts (the format
//! `adage condition` reads) into the session's `CLAUDE_CONFIG_DIR/projects`, exactly where a real
//! session would, and reports a cost per turn.
//!
//! Its "agent" is a small policy over a [`FakeWorld`] (what each command prints and whether it
//! fails in this repository):
//!
//! - what it knows comes from its arm: the skill's claims (read with the same extractor, from the
//!   appended system prompt), or the reflexes in the index its hooks would read (`ADAGE_HOME`,
//!   unless `ADAGE_DISABLE=1`), or nothing (baseline: it tries the naive command first, and runs
//!   the regenerate step only some of the time);
//! - a task with a target file reads and edits it, then runs any regenerate step it knows for it;
//! - a trigger task that is not a lookup then verifies: the commands it knows, in order, until one
//!   passes, else the world's working command.

use crate::claims::{self, ClaimKind};
use crate::harness::{Harness, Outcome, SessionSpec};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FakeCommand {
    /// A command prefix (`python3 -m unittest`).
    pub run: String,
    pub exit: i32,
    pub output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FakeWorld {
    pub commands: Vec<FakeCommand>,
    /// What an agent that knows nothing tries first to verify (`pytest`).
    pub naive_verify: String,
    /// What works (`make test`).
    pub working_verify: String,
    /// The regenerate steps the repository really needs: (glob, command).
    pub regenerate: Vec<(String, String)>,
    /// The lookup command a baseline agent finds by exploring, with `{id}` for the id.
    pub lookup: Option<String>,
    /// How often (percent) a baseline agent runs a needed regenerate step on its own.
    pub baseline_regen_percent: u64,
    pub usd_per_turn: f64,
}

pub struct Fake {
    pub world: FakeWorld,
}

#[derive(Default)]
struct Knows {
    verify: Vec<String>,
    regen: Vec<(String, String)>,
    lookup: Option<String>,
}

fn stable(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

fn uuid(s: &str) -> String {
    let a = stable(s);
    let b = stable(&format!("{s}/2"));
    format!(
        "{:08x}-{:04x}-4{:03x}-8{:03x}-{:012x}",
        a >> 32,
        (a >> 16) & 0xffff,
        a & 0xfff,
        b >> 52,
        b & 0xffff_ffff_ffff
    )
}

fn glob_matches(glob: &str, rel: &str) -> bool {
    glob::Pattern::new(glob).is_ok_and(|p| {
        p.matches_with(rel, glob::MatchOptions { require_literal_separator: true, ..Default::default() })
    })
}

impl Fake {
    fn outcome(&self, cmd: &str) -> (i32, String) {
        self.world
            .commands
            .iter()
            .find(|c| cmd == c.run || cmd.starts_with(&format!("{} ", c.run)))
            .map(|c| (c.exit, c.output.clone()))
            .unwrap_or((0, "ok".into()))
    }

    fn knows(&self, spec: &SessionSpec, h: u64) -> Knows {
        let mut k = Knows::default();
        if let Some(text) = &spec.append_system_prompt {
            let table = adage_core::CommandTable::load();
            for c in claims::from_markdown(text, "skill", &table).into_iter().filter(|c| !c.is_unsafe()) {
                match c.kind {
                    ClaimKind::Verify { command } => k.verify.push(command),
                    ClaimKind::Avoid { instead, .. } => k.verify.insert(0, instead),
                    ClaimKind::RegenerateAfter { glob: Some(g), command } => k.regen.push((g, command)),
                    ClaimKind::Lookup { command, id_example: Some(id), .. } => {
                        k.lookup = Some(command.replace(&id, "{id}"))
                    }
                    _ => {}
                }
            }
            return k;
        }
        let hooks_on = spec.env.get("ADAGE_DISABLE").map(String::as_str) != Some("1");
        if let (true, Some(home)) = (hooks_on, spec.env.get("ADAGE_HOME")) {
            let index = adage_tracks::Index::load(&Path::new(home).join("index.json"));
            let key = adage_core::repo::repo_key(&spec.cwd);
            for r in index.for_repo(&key) {
                match (&r.route.trigger, &r.route.step) {
                    (_, adage_tracks::Step::Avoid { instead, .. }) => k.verify.insert(0, instead.clone()),
                    (adage_tracks::Trigger::AfterEdit { paths }, adage_tracks::Step::Run { command }) => {
                        k.regen.push((paths.clone(), command.clone()))
                    }
                    (adage_tracks::Trigger::Prompt { .. }, adage_tracks::Step::Run { command }) => {
                        k.lookup = Some(command.replace("{item}", "{id}"))
                    }
                    _ => {}
                }
            }
            return k;
        }
        // Baseline: the naive command first; the regenerate step only sometimes.
        k.verify.push(self.world.naive_verify.clone());
        if h % 100 < self.world.baseline_regen_percent {
            k.regen = self.world.regenerate.clone();
        }
        k
    }
}

/// One tool call: (tool name, input, result text, is_error).
type Call = (&'static str, Value, String, bool);

impl Harness for Fake {
    fn name(&self) -> &'static str {
        "fake"
    }
    fn spawns_agents(&self) -> bool {
        false
    }
    fn auth_env(&self) -> &'static [&'static str] {
        &[]
    }
    fn command(&self, s: &SessionSpec) -> Vec<String> {
        vec!["(fake agent)".into(), s.prompt.clone()]
    }
    fn settings(&self, hooks: Option<&Value>) -> Value {
        json!({ "fake": true, "hooks": hooks.and_then(|h| h.get("hooks")).cloned().unwrap_or(Value::Null) })
    }
    fn run(&self, s: &SessionSpec) -> Result<Outcome> {
        let h = stable(&s.job);
        let k = self.knows(s, h);
        let cwd = s.cwd.to_string_lossy().to_string();
        let mut calls: Vec<Call> = Vec::new();
        let bash = |cmd: &str, this: &Fake| -> Call {
            let (exit, out) = this.outcome(cmd);
            let text = if exit == 0 { out } else { format!("Exit code {exit}\n{out}") };
            ("Bash", json!({ "command": cmd, "description": "Run a command" }), text, exit != 0)
        };
        let ticket = regex::Regex::new(r"\b[A-Z][A-Z0-9]{0,9}-\d{1,7}\b").unwrap();
        let id = ticket.find(&s.prompt).map(|m| m.as_str().to_string());
        let task = &s.task;
        if let Some(id) = &id {
            match &k.lookup {
                Some(l) => calls.push(bash(&l.replace("{id}", id), self)),
                None => {
                    calls.push(bash("ls scripts", self));
                    if let Some(l) = &self.world.lookup {
                        calls.push(bash(&l.replace("{id}", id), self));
                    }
                }
            }
        }
        let mut edited = false;
        match (&task.target, task.control) {
            (Some(t), false) => {
                let path = format!("{cwd}/{t}");
                calls.push(("Read", json!({ "file_path": path }), "     1\t{}\n".into(), false));
                calls.push((
                    "Edit",
                    json!({ "file_path": path, "old_string": "{", "new_string": "{\n  \"x\": 1,", "replace_all": false }),
                    format!("The file {path} has been updated successfully."),
                    false,
                ));
                edited = true;
                for (g, c) in &k.regen {
                    if glob_matches(g, t) {
                        calls.push(bash(c, self));
                    }
                }
            }
            (Some(t), true) => {
                calls.push(("Read", json!({ "file_path": format!("{cwd}/{t}") }), "     1\t# file\n".into(), false))
            }
            (None, true) => calls.push(bash("ls", self)),
            (None, false) => {}
        }
        if !task.control && (edited || id.is_none()) {
            let mut passed = false;
            for c in &k.verify {
                let call = bash(c, self);
                let failed = call.3;
                calls.push(call);
                if !failed {
                    passed = true;
                    break;
                }
            }
            if !passed {
                calls.push(bash(&self.world.working_verify, self));
            }
        }
        let sid = uuid(&format!("{}/{}", s.job, s.model));
        let dir = s.config_dir.join("projects").join(project_dir(&cwd));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{sid}.jsonl"));
        std::fs::write(&path, transcript(&sid, &cwd, &s.model, &s.prompt, &calls))
            .with_context(|| format!("writing {}", path.display()))?;
        let turns = calls.len() as u32 + 1;
        std::fs::create_dir_all(&s.session_dir)?;
        let cost = turns as f64 * self.world.usd_per_turn;
        let result = json!({ "type": "result", "subtype": "success", "is_error": false, "num_turns": turns,
                             "total_cost_usd": cost, "session_id": sid });
        std::fs::write(s.session_dir.join("stream.jsonl"), format!("{result}\n"))?;
        Ok(Outcome {
            session_id: Some(sid),
            cost_usd: cost,
            turns,
            subtype: Some("success".into()),
            ok: true,
            error: None,
        })
    }
}

/// The project directory name Claude Code derives from a cwd.
pub fn project_dir(cwd: &str) -> String {
    cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

/// A Claude Code transcript: the prompt, one assistant record per tool call (one call per turn),
/// one user record per result, and a final answer.
pub fn transcript(sid: &str, cwd: &str, model: &str, prompt: &str, calls: &[Call]) -> String {
    let mut lines: Vec<Value> = Vec::new();
    let mut n = 0u32;
    let mut rec = |v: Value, lines: &mut Vec<Value>| {
        n += 1;
        let mut v = v;
        v["sessionId"] = json!(sid);
        v["cwd"] = json!(cwd);
        v["isSidechain"] = json!(false);
        v["uuid"] = json!(uuid(&format!("{sid}/{n}")));
        v["timestamp"] = json!(format!("2026-09-27T10:{:02}:{:02}.000Z", (n / 60) % 60, n % 60));
        lines.push(v);
    };
    rec(json!({ "type": "user", "message": { "role": "user", "content": prompt } }), &mut lines);
    for (i, (name, input, out, err)) in calls.iter().enumerate() {
        let id = format!("toolu_{:04}", i + 1);
        rec(
            json!({ "type": "assistant", "message": { "id": format!("msg_{:04}", i + 1), "role": "assistant", "model": model,
                    "content": [{ "type": "tool_use", "id": id, "name": name, "input": input }] } }),
            &mut lines,
        );
        rec(
            json!({ "type": "user", "message": { "role": "user",
                    "content": [{ "type": "tool_result", "tool_use_id": id, "content": out, "is_error": err }] } }),
            &mut lines,
        );
    }
    rec(
        json!({ "type": "assistant", "message": { "id": "msg_end", "role": "assistant", "model": model,
                "content": [{ "type": "text", "text": "Done." }] } }),
        &mut lines,
    );
    lines.iter().map(|l| format!("{l}\n")).collect()
}
