//! Harness adapters (Architecture §4.11). The decision core (`respond`) knows only the normalised
//! [`Event`] (prompt submitted, tool batch completed, tool failed, stop requested, session start and
//! end, a step boundary) and the normalised [`Output`]. Each harness contributes two pure functions
//! — translate a native payload into an event, and render an output as the native response — plus
//! its limits. What each native event name is comes from one table, adage-trace's
//! [`Harness::events`]; [`Adapter::classify`] turns it into the path `handle` takes ([`HookEvent`]).
//!
//! What each harness documents, and what is inferred, is recorded in `docs/harnesses.md`.

pub mod claude;
pub mod codex;
pub mod opencode;

use adage_runtime::{By, Call, Prediction};
pub use adage_trace::{Capabilities, EventKind, Harness};

/// The hook timeout, in seconds, of the events an E2 action runs at (the edit and Stop events)
/// when the registration runs `adage-exec`: an action's wall limit (20 s) stays well under it.
/// Every other event, and every `adage` registration, keeps 5 s. OpenCode's bridge waits 1 s, so
/// actions never run under OpenCode.
pub const ACTION_HOOK_TIMEOUT_S: u32 = 30;

/// The timeout for an action event of a registration running `bin`.
pub fn action_timeout(bin: &str) -> u32 {
    let name = bin.rsplit(['/', '\\']).next().unwrap_or(bin);
    if name == "adage-exec" || name == "adage-exec.exe" {
        ACTION_HOOK_TIMEOUT_S
    } else {
        5
    }
}
pub use adage_runtime::Event;
use serde_json::Value;
use std::path::PathBuf;

/// What the core decided, before harness rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
    /// Directive text (already wrapped and capped) for the model's next turn.
    Context(String),
    /// Context for the model with a separate person-only notice: the session start's contract,
    /// or an event's directives beside a lapsed-grant line (`Delivery::notice`).
    SessionContext { context: String, notice: Option<String> },
    /// A stop-hook request to continue, with its reason (fixed template).
    Continue(String),
    /// One line for the person, not the model (docs/zero-config.md §3.3): Claude Code shows a
    /// `systemMessage` in its UI and never sends it to the model. Harnesses without such a channel
    /// render nothing; their notices reach the status card and `adage watch` only.
    Notice(String),
    /// Before a tool call (`PreToolUse`): run it with this input instead (only the interpreter
    /// prefix of its command changed), telling the model why (`context`) and the person
    /// (`notice`). Harnesses that cannot rewrite a call render nothing (the post-hoc directive
    /// still follows the failure).
    Rewrite { input: Value, context: String, notice: String },
    /// At a step boundary (`TurnStep`): the calls Adage makes in the model's place (the Claude
    /// Code mod's `turn.step`). Harnesses without the event render nothing.
    Predict(Prediction),
    /// At a step boundary: the model is about to be asked to do what a delivered directive already
    /// says, so the step may go to the person's routine model (the mod's `routine_model`).
    /// Harnesses without the event render nothing.
    Routine,
}

/// What `handle` does with one invocation: one path each, matched exhaustively there. The native
/// name's [`EventKind`] comes from the event table ([`Harness::events`]); an event the runtime
/// decides is translated by the adapter ([`Adapter::translate`]) and carried as that [`Event`].
#[derive(Debug, Clone)]
pub enum HookEvent {
    /// The mod asks for a press token: answered by the press before anything else
    /// (`press::gate`), so it never reaches the dispatch.
    Press,
    /// The mod asks about a skill's text (`skill::prompt`).
    SkillPrompt,
    /// A shell call is about to run: the command hook, or the mod's dry run or verdict.
    PreToolUse,
    /// The mod asks for the standing text.
    Standing,
    /// A session starts.
    SessionStart,
    /// An event the runtime decides (`decide`), as the adapter translated it.
    Decide(Event),
    /// A name this harness does not send, or a payload its translation refuses: nothing to do.
    Ignored,
}

/// Who is calling: the session and working directory every native payload must carry.
pub struct Envelope {
    pub session: String,
    pub cwd: PathBuf,
}

pub trait Adapter: Sync {
    /// Which harness this adapts.
    fn harness(&self) -> Harness;
    /// The name in hook registrations and spools ([`Harness::hook_name`]).
    fn name(&self) -> &'static str {
        self.harness().hook_name()
    }
    /// The `harness` a spool record names, or `None` for a record that names none (read as Claude
    /// Code's, as every record was before there were other harnesses).
    fn recorded_name(&self) -> Option<&'static str> {
        Some(self.name())
    }
    /// Session and cwd from the native payload. `None` → do nothing.
    fn envelope(&self, p: &Value) -> Option<Envelope> {
        Some(Envelope {
            session: p.get("session_id")?.as_str()?.to_string(),
            cwd: PathBuf::from(p.get("cwd")?.as_str()?),
        })
    }
    /// What the harness can do for this payload ([`Capabilities`]): its command hooks' set
    /// ([`Harness::capabilities`]), unless its adapter knows a richer caller sent it (Claude Code's
    /// mod, [`claude::Claude`]). Behaviour past the adapter reads this, never which plugin asked.
    fn capabilities(&self, _event: &str, _p: &Value) -> Capabilities {
        self.harness().capabilities()
    }
    /// Native event name and payload → normalised event. `None` → do nothing.
    fn translate(&self, event: &str, p: &Value, cwd: &str) -> Option<Event>;
    /// What `handle` does with this native event and payload ([`HookEvent`]): the event table
    /// names it, and an event the runtime decides is translated.
    fn classify(&self, event: &str, p: &Value, cwd: &str) -> HookEvent {
        match self.harness().event_kind(event) {
            None => HookEvent::Ignored,
            Some(EventKind::Press) => HookEvent::Press,
            Some(EventKind::SkillPrompt) => HookEvent::SkillPrompt,
            Some(EventKind::PreToolUse) => HookEvent::PreToolUse,
            Some(EventKind::Standing) => HookEvent::Standing,
            Some(
                EventKind::SessionStart
                | EventKind::Prompt
                | EventKind::ToolBatch
                | EventKind::ToolUse
                | EventKind::Failure
                | EventKind::Stop
                | EventKind::SessionEnd
                | EventKind::TurnStep,
            ) => match self.translate(event, p, cwd) {
                Some(Event::SessionStart) => HookEvent::SessionStart,
                Some(ev) => HookEvent::Decide(ev),
                None => HookEvent::Ignored,
            },
        }
    }
    /// Stable native occurrence identity, when the harness supplies one. Never derive this
    /// from tool input/output contents: a later legitimate call can have identical contents.
    fn event_identity(&self, _event: &str, _p: &Value) -> Option<String> {
        None
    }
    /// Normalised output → the exact bytes the harness expects on stdout.
    fn render(&self, event: &str, out: Output) -> Option<String>;
    /// The largest directive text this harness accepts in one response.
    fn max_context(&self) -> usize {
        9_500
    }
    /// Does this native event end an agent turn or a session? Live learning is triggered there
    /// (`crate::autolearn`), whatever the decision.
    fn is_learn_point(&self, event: &str) -> bool {
        matches!(self.harness().event_kind(event), Some(EventKind::Stop | EventKind::SessionEnd))
    }
}

/// Canonical identity of a complete native tool-call set, independent of completion order.
/// Missing, duplicate or oversized IDs do not establish an occurrence identity.
pub fn tool_event_identity(harness: &str, event: &str, mut ids: Vec<&str>) -> Option<String> {
    if ids.is_empty() || ids.len() > 1024 || ids.iter().any(|id| id.is_empty() || id.len() > 256) {
        return None;
    }
    ids.sort_unstable();
    if ids.windows(2).any(|pair| pair[0] == pair[1]) {
        return None;
    }
    serde_json::to_string(&(harness, event, ids)).ok()
}

/// The adapter registered as `adage hook <name>` ([`Harness::hook_name`]).
pub fn adapter(name: &str) -> Option<&'static dyn Adapter> {
    Harness::ALL.into_iter().find(|h| h.hook_name() == name).map(adapter_for)
}

/// Every harness's adapter: one arm per harness, so a new [`Harness`] cannot go without one.
pub fn adapter_for(h: Harness) -> &'static dyn Adapter {
    match h {
        Harness::ClaudeCode => &claude::Claude,
        Harness::Codex => &codex::Codex,
        Harness::OpenCode => &opencode::OpenCode,
    }
}

/// Text Adage itself wrote: a stop-hook continuation (a learned obligation's request, the
/// finish-line gate's failure, a recovery warning: all begin `Adage finish-line check:` and end
/// with the "not from the user" sentence, `render::finish_line`) or a directive. On harnesses
/// where these re-enter as user messages (Codex Stop, OpenCode's context message), they must not
/// be mistaken for the user's prompt.
pub fn is_own_text(prompt: &str) -> bool {
    adage_runtime::render::is_finish_line(prompt) || prompt.trim_start().starts_with("<adage-reflex ")
}

/// Calls for translated `(tool, input)` pairs, with relative file paths anchored at the session's
/// working directory (patch tools name files relative to it).
pub fn calls(pairs: Vec<(String, Value)>, cwd: &str, output: &str, failed: bool) -> Vec<Call> {
    pairs
        .into_iter()
        .map(|(tool, mut input)| {
            if let Some(p) = input.get("file_path").and_then(|v| v.as_str()) {
                if !p.is_empty() && !p.starts_with('/') {
                    input["file_path"] = Value::String(adage_core::shell::resolve_path(cwd, p));
                }
            }
            Call { tool, input, output: output.to_string(), failed, by: By::Agent, tool_use_id: None, agent: None }
        })
        .collect()
}

#[cfg(test)]
mod dispatch_tests;

#[cfg(test)]
mod own_text_tests {
    use super::*;
    use adage_runtime::render;
    use serde_json::json;

    /// Review F3: on Codex a Stop `block` reason comes back as the next user prompt. Every Stop
    /// continuation Adage writes (a learned obligation's request, the finish-line gate's failure
    /// or unconfirmed run, both joined) is Adage's text there, never decided on as the person's
    /// prompt; on Claude Code too, should it ever arrive as one. Before, the gate's failure began
    /// "Adage ran …" and was taken for the user's prompt.
    #[test]
    fn stop_continuations_are_never_the_persons_prompt() {
        let gate = render::stop_gate_failed(".venv/bin/python -m pytest tests -q", Some(1), None);
        let unknown = render::stop_gate_unknown("make test", "the completion could not be recorded");
        let obligation = render::stop_reason("schema/*.json", "make gen");
        let joined = render::join_finish_lines(&gate, &obligation);
        for text in [&gate, &unknown, &obligation, &joined] {
            assert!(is_own_text(text), "{text}");
            for harness in ["codex", "claude"] {
                let a = adapter(harness).unwrap();
                assert!(
                    a.translate("UserPromptSubmit", &json!({ "prompt": text }), "/r").is_none(),
                    "{harness}: {text}"
                );
            }
        }
        let typed = json!({ "prompt": "Fix the rounding bug in the finish-line check" });
        for harness in ["codex", "claude"] {
            assert!(matches!(
                adapter(harness).unwrap().translate("UserPromptSubmit", &typed, "/r"),
                Some(Event::Prompt { .. })
            ));
        }
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn native_identity_requires_the_complete_call_set() {
        let a = adapter("claude").unwrap();
        let batch =
            |ids: &[&str]| json!({"tool_calls": ids.iter().map(|id| json!({"tool_use_id": id})).collect::<Vec<_>>()});
        let first = a.event_identity("PostToolBatch", &batch(&["one", "two"]));
        assert!(first.is_some());
        assert_eq!(first, a.event_identity("PostToolBatch", &batch(&["two", "one"])));
        assert_ne!(first, a.event_identity("PostToolBatch", &batch(&["one", "three"])));
        assert!(a.event_identity("PostToolBatch", &batch(&["one", "one"])).is_none());
        assert!(a.event_identity("PostToolBatch", &json!({"tool_calls": [{"tool_use_id": "one"}, {}]})).is_none());
        assert!(a.event_identity("PostToolBatch", &batch(&[])).is_none());
        assert!(adapter("codex").unwrap().event_identity("PostToolUse", &json!({"tool_use_id": "one"})).is_some());
        assert!(adapter("codex").unwrap().event_identity("PostToolUse", &json!({"id": "invented"})).is_none());
    }

    /// T-A1-01: a Bash call about to run has an identity of its own (one decision per call, the
    /// mod's and the command hook's alike); without a usable `tool_use_id` it has none.
    #[test]
    fn a_pretool_call_is_identified_by_its_tool_use_id() {
        let a = adapter("claude").unwrap();
        let one = a.event_identity("PreToolUse", &json!({"tool_use_id": "t1"}));
        assert!(one.is_some());
        assert_eq!(one, a.event_identity("PreToolUse", &json!({"tool_use_id": "t1", "dry_run": true})));
        assert_ne!(one, a.event_identity("PostToolUse", &json!({"tool_use_id": "t1"})), "another event");
        assert_ne!(one, a.event_identity("PreToolUse", &json!({"tool_use_id": "t2"})));
        assert!(a.event_identity("PreToolUse", &json!({})).is_none());
        assert!(a.event_identity("PreToolUse", &json!({"tool_use_id": ""})).is_none());
        assert!(a.event_identity("PreToolUse", &json!({"tool_use_id": "x".repeat(257)})).is_none());
        assert!(a.event_identity("PreToolUse", &json!({"tool_use_id": "x".repeat(256)})).is_some());
    }
}
