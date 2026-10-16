//! The characterization of the hook's dispatch: every (harness, native event, payload class) and
//! what `handle` does with it. Written against the string dispatch before the event table existed
//! (`legacy`, a frozen copy of what `handle`, `press::gate` and `rebuild::kind_of` decided), and
//! kept as the oracle the table-driven dispatch must agree with, row for row.

use super::*;
use serde_json::json;

/// One row: what a native event does.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Row {
    /// The path `handle` takes after the press gate and the kill switch: `press`, `skill`,
    /// `pretool`, `standing`, `session_start`, `decide`, or `none`.
    route: &'static str,
    /// The `kind` its spool record carries when it writes one (`""`: it writes none of its own).
    kind: &'static str,
    /// `rebuild::kind_of` of a record that carries only this event name (records before `kind`).
    legacy: &'static str,
    /// The press's part: `issue`, `open`, `end`, `close`, `close+tripwire` (a Bash call the
    /// tripwire reads), or `pass`.
    press: &'static str,
    /// Live learning is asked for after it (`Adapter::is_learn_point`).
    learn: bool,
}

const fn row(route: &'static str, kind: &'static str, legacy: &'static str, press: &'static str, learn: bool) -> Row {
    Row { route, kind, legacy, press, learn }
}

/// The table: (harness, native event, payload class, payload) → row.
#[rustfmt::skip]
fn table() -> Vec<(&'static str, &'static str, &'static str, Value, Row)> {
    let bash = json!({ "tool_name": "Bash", "tool_input": { "command": "ls" }, "tool_response": "a" });
    let calls = json!({ "tool_calls": [{ "tool_name": "Bash", "tool_use_id": "t1", "tool_input": { "command": "ls" } }] });
    let own = json!({ "prompt": "<adage-reflex route=\"r\">\nrun make gen\n</adage-reflex>" });
    let typed = json!({ "prompt": "fix the rounding" });
    let none = json!({});
    vec![
        // Claude Code: the command hooks and the mod.
        ("claude", "SessionStart", "a start", json!({ "source": "startup" }), row("session_start", "session_start", "session_start", "open", false)),
        ("claude", "UserPromptSubmit", "typed", typed.clone(), row("decide", "prompt", "prompt", "close", false)),
        ("claude", "UserPromptSubmit", "Adage's own text", own.clone(), row("none", "", "prompt", "close", false)),
        ("claude", "UserPromptSubmit", "no prompt", none.clone(), row("none", "", "prompt", "close", false)),
        ("claude", "PreToolUse", "command hook", bash.clone(), row("pretool", "pretool", "", "close+tripwire", false)),
        ("claude", "PreToolUse", "mod dry run", json!({ "dry_run": true }), row("pretool", "pretool", "", "close", false)),
        ("claude", "PreToolUse", "mod verdict", json!({ "verdict": { "engine": "same" } }), row("pretool", "pretool", "", "close", false)),
        ("claude", "PreToolUse", "via mod", json!({ "via": "mod" }), row("pretool", "pretool", "", "close", false)),
        ("claude", "PostToolUse", "a call", bash.clone(), row("decide", "batch", "batch", "close", false)),
        ("claude", "PostToolBatch", "calls", calls.clone(), row("decide", "batch", "batch", "close", false)),
        ("claude", "PostToolBatch", "no calls", none.clone(), row("none", "", "batch", "close", false)),
        ("claude", "PostToolUseFailure", "a call", json!({ "tool_name": "Bash", "error": "x" }), row("decide", "failure", "failure", "close", false)),
        ("claude", "Stop", "a stop", json!({ "stop_hook_active": false }), row("decide", "stop", "stop", "close", true)),
        ("claude", "SessionEnd", "an end", none.clone(), row("decide", "session_end", "session_end", "end", true)),
        ("claude", "TurnStep", "a step", json!({ "turn_id": "t", "index": 1 }), row("decide", "turn_step", "turn_step", "close", false)),
        ("claude", "TurnStep", "no index", json!({ "turn_id": "t" }), row("none", "", "turn_step", "close", false)),
        ("claude", "Standing", "the mod's ask", none.clone(), row("standing", "", "", "pass", false)),
        ("claude", "SkillPrompt", "the mod's ask", json!({ "skill": "s", "text": "t" }), row("skill", "skill_prompt", "", "pass", false)),
        ("claude", "Press", "the mod's ask", json!({ "source": "startup" }), row("press", "press_issued", "", "issue", false)),
        ("claude", "Mystery", "anything", none.clone(), row("none", "", "", "pass", false)),
        ("claude", "session.idle", "another harness's name", none.clone(), row("none", "", "stop", "pass", false)),
        // Codex: command hooks only.
        ("codex", "SessionStart", "a start", none.clone(), row("session_start", "session_start", "session_start", "pass", false)),
        ("codex", "UserPromptSubmit", "typed", typed.clone(), row("decide", "prompt", "prompt", "pass", false)),
        ("codex", "UserPromptSubmit", "Adage's own text", own.clone(), row("none", "", "prompt", "pass", false)),
        ("codex", "PostToolUse", "a call", bash.clone(), row("decide", "batch", "batch", "pass", false)),
        ("codex", "PostToolUse", "a failed call", json!({ "tool_name": "Bash", "tool_input": { "command": "ls" }, "tool_response": { "exit_code": 1 } }), row("decide", "failure", "batch", "pass", false)),
        ("codex", "PostToolUse", "no tool", none.clone(), row("none", "", "batch", "pass", false)),
        ("codex", "PostToolUseFailure", "a call", bash.clone(), row("decide", "failure", "failure", "pass", false)),
        ("codex", "Stop", "a stop", none.clone(), row("decide", "stop", "stop", "pass", true)),
        ("codex", "SessionEnd", "an end", none.clone(), row("decide", "session_end", "session_end", "pass", true)),
        ("codex", "PreToolUse", "a call", bash.clone(), row("none", "", "", "pass", false)),
        ("codex", "PostToolBatch", "calls", calls.clone(), row("none", "", "batch", "pass", false)),
        ("codex", "TurnStep", "a step", json!({ "turn_id": "t", "index": 1 }), row("none", "", "turn_step", "pass", false)),
        ("codex", "Standing", "an ask", none.clone(), row("none", "", "", "pass", false)),
        ("codex", "SkillPrompt", "an ask", json!({ "skill": "s", "text": "t" }), row("none", "", "", "pass", false)),
        ("codex", "Press", "an ask", json!({ "source": "startup" }), row("none", "", "", "pass", false)),
        // OpenCode: the bridge's own names.
        ("opencode", "session.created", "a start", none.clone(), row("session_start", "session_start", "session_start", "pass", false)),
        ("opencode", "chat.message", "typed", typed.clone(), row("decide", "prompt", "prompt", "pass", false)),
        ("opencode", "chat.message", "Adage's own text", own, row("none", "", "prompt", "pass", false)),
        ("opencode", "tool.execute.after", "a call", json!({ "tool": "bash", "args": { "command": "ls" }, "output": "a", "exit": 0 }), row("decide", "batch", "batch", "pass", false)),
        ("opencode", "tool.execute.after", "a failed call", json!({ "tool": "bash", "args": { "command": "ls" }, "exit": 2 }), row("decide", "failure", "batch", "pass", false)),
        ("opencode", "tool.execute.after", "no tool", none.clone(), row("none", "", "batch", "pass", false)),
        ("opencode", "tool.error", "a call", json!({ "tool": "bash", "args": { "command": "ls" }, "error": "x" }), row("decide", "failure", "failure", "pass", false)),
        ("opencode", "tool.error", "no tool", none.clone(), row("none", "", "failure", "pass", false)),
        ("opencode", "session.idle", "a stop", none.clone(), row("decide", "stop", "stop", "pass", true)),
        ("opencode", "session.deleted", "an end", none.clone(), row("decide", "session_end", "session_end", "pass", true)),
        ("opencode", "SessionStart", "Claude Code's name", none.clone(), row("none", "", "session_start", "pass", false)),
        ("opencode", "Stop", "Claude Code's name", none.clone(), row("none", "", "stop", "pass", false)),
        ("opencode", "PreToolUse", "a call", bash, row("none", "", "", "pass", false)),
        ("opencode", "Standing", "an ask", none, row("none", "", "", "pass", false)),
    ]
}

/// The dispatch as it was before the event table: `handle`'s string comparisons, `press::gate`'s
/// match and `CLOSERS`, `claude::is_mods` and `MOD_EVENTS`, and `rebuild::kind_of`'s match, copied
/// verbatim from commit `e4609ad`. Frozen: it is the reference, never edited to fit.
mod legacy {
    use super::super::{adapter, Event, Harness};
    use serde_json::Value;

    const CLOSERS: &[&str] =
        &["UserPromptSubmit", "PreToolUse", "PostToolUse", "PostToolBatch", "PostToolUseFailure", "Stop", "TurnStep"];
    const MOD_EVENTS: [&str; 3] = ["TurnStep", "Standing", "SkillPrompt"];

    fn is_mods(event: &str, p: &Value) -> bool {
        p.get("via").and_then(Value::as_str) == Some("mod")
            || MOD_EVENTS.contains(&event)
            || (event == "PreToolUse" && crate::phase::Phase::of(p).is_some())
    }

    fn kind_of(event: &str) -> &'static str {
        match event {
            "UserPromptSubmit" | "chat.message" => "prompt",
            "PostToolBatch" | "PostToolUse" | "tool.execute.after" => "batch",
            "PostToolUseFailure" | "tool.error" => "failure",
            "Stop" | "session.idle" => "stop",
            "SessionStart" | "session.created" => "session_start",
            "SessionEnd" | "session.deleted" => "session_end",
            "TurnStep" => "turn_step",
            _ => "",
        }
    }

    pub(super) fn row(harness: &str, event: &str, p: &Value) -> super::Row {
        let a = adapter(harness).unwrap();
        let claude = a.harness() == Harness::ClaudeCode;
        let press = match event {
            _ if !claude => "pass",
            "Press" => "issue",
            "SessionStart" => "open",
            "SessionEnd" => "end",
            "PreToolUse" if !is_mods(event, p) => "close+tripwire",
            e if CLOSERS.contains(&e) => "close",
            _ => "pass",
        };
        // `handle`: the press answers a `Press`; then `SkillPrompt` (only Claude Code's text is
        // rewritten), `PreToolUse` (only where the harness may rewrite a call), `Standing` (only
        // Claude Code), and everything else as `translate` names it.
        // Every branch but the last returns before live learning is asked for.
        let (route, kind, learn) = if claude && event == "Press" {
            ("press", "press_issued", false)
        } else if event == "SkillPrompt" {
            if claude {
                ("skill", "skill_prompt", false)
            } else {
                ("none", "", false)
            }
        } else if event == "PreToolUse" {
            if a.harness().capabilities().pre_tool_rewrite {
                ("pretool", "pretool", false)
            } else {
                ("none", "", false)
            }
        } else if event == "Standing" {
            if claude {
                ("standing", "", false)
            } else {
                ("none", "", false)
            }
        } else {
            match a.translate(event, p, "/r") {
                Some(Event::SessionStart) => ("session_start", "session_start", false),
                Some(ev) => ("decide", ev.name(), a.is_learn_point(event)),
                None => ("none", "", a.is_learn_point(event)),
            }
        };
        super::Row { route, kind, legacy: kind_of(event), press, learn }
    }
}

/// The dispatch as it is: the adapter's [`Adapter::classify`] (what `handle` matches on), the
/// press's [`crate::press::act_of`], and `rebuild::kind_of`, all read from the event table.
fn current(harness: &str, event: &str, p: &Value) -> Row {
    use crate::press::{act_of, Act};
    let a = adapter(harness).unwrap();
    // `handle` returns before live learning on every path but a decided event's.
    let (route, kind, learn) = match a.classify(event, p, "/r") {
        HookEvent::Press => ("press", "press_issued", false),
        HookEvent::SkillPrompt => ("skill", "skill_prompt", false),
        HookEvent::PreToolUse => ("pretool", "pretool", false),
        HookEvent::Standing => ("standing", "", false),
        HookEvent::SessionStart => ("session_start", "session_start", false),
        HookEvent::Decide(ev) => ("decide", ev.name(), a.is_learn_point(event)),
        HookEvent::Ignored => ("none", "", false),
    };
    let press = match act_of(harness, event, p) {
        Act::Pass => "pass",
        Act::Issue => "issue",
        Act::Open => "open",
        Act::End => "end",
        Act::Close { tripwire: true, .. } => "close+tripwire",
        Act::Close { tripwire: false, .. } => "close",
    };
    let record = json!({ "event": event });
    let legacy = adage_runtime::rebuild::kind_of(&record);
    let legacy = ["prompt", "batch", "failure", "stop", "session_start", "session_end", "turn_step"]
        .into_iter()
        .find(|k| *k == legacy)
        .unwrap_or("");
    Row { route, kind, legacy, press, learn }
}

/// The frozen dispatch from before the event table, and the table-driven one, each give every
/// row.
#[test]
fn the_legacy_dispatch_gives_every_row() {
    for (harness, event, class, p, want) in table() {
        assert_eq!(legacy::row(harness, event, &p), want, "{harness} {event} ({class})");
        assert_eq!(current(harness, event, &p), want, "{harness} {event} ({class}), from the event table");
    }
}

thread_local! {
    static LEARNER_ASKED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A learner that is never found: asking for it is counted, and the hook records the failure.
fn counted_learner() -> Option<PathBuf> {
    LEARNER_ASKED.with(|n| n.set(n.get() + 1));
    None
}

/// Every row through `handle` itself, in a repository with reflexes and a treated session: live
/// learning is asked for exactly at the rows that say so, a record it writes carries the row's
/// kind, and a `none` row prints nothing and writes nothing (no spool, no session state).
#[test]
fn handle_takes_each_rows_path() {
    use crate::testkit::{ObligationWorld, NO_EXTENSION};
    use crate::{handle, Options};
    for (harness, event, class, extra, want) in table() {
        let w = ObligationWorld::new();
        std::fs::remove_file(w.paths.state("s")).unwrap();
        let mut p = json!({ "session_id": "s", "cwd": w.root });
        for (k, v) in extra.as_object().unwrap() {
            p[k] = v.clone();
        }
        let opts = Options { learner: Some(counted_learner), extension: Some(&NO_EXTENSION) };
        let before = LEARNER_ASKED.with(|n| n.get());
        let out = handle(harness, event, &p.to_string(), opts, &w.paths, std::time::Instant::now()).unwrap();
        let learned = LEARNER_ASKED.with(|n| n.get()) > before;
        let at = format!("{harness} {event} ({class})");
        assert_eq!(learned, want.learn, "{at}");
        let recs: Vec<Value> = adage_store::spool::read_all(&w.paths.spool("s")).unwrap_or_default();
        for r in &recs {
            assert_eq!(r["kind"], want.kind, "{at}: {r}");
        }
        if want.route == "none" {
            assert!(out.is_none() && recs.is_empty() && !w.paths.state("s").exists(), "{at}: {out:?} {recs:?}");
        }
    }
}
