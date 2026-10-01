//! Harness adapters (Architecture §4.11). The decision core in `hook.rs` knows only the normalised
//! [`Event`] (prompt submitted, tool batch completed, tool failed, stop requested, session start and
//! end) and the normalised [`Output`]. Each harness contributes two pure functions — translate a
//! native payload into an event, and render an output as the native response — plus its limits.
//!
//! What each harness documents, and what is inferred, is recorded in `docs/harnesses.md`.

pub mod claude;
pub mod codex;
pub mod opencode;

use adage_runtime::Call;

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
    /// Session responsibility context, with an optional separate person-only notice.
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
}

/// Who is calling: the session and working directory every native payload must carry.
pub struct Envelope {
    pub session: String,
    pub cwd: PathBuf,
}

pub trait Adapter: Sync {
    /// The name recorded in spools.
    fn name(&self) -> &'static str;
    /// Session and cwd from the native payload. `None` → do nothing.
    fn envelope(&self, p: &Value) -> Option<Envelope> {
        Some(Envelope {
            session: p.get("session_id")?.as_str()?.to_string(),
            cwd: PathBuf::from(p.get("cwd")?.as_str()?),
        })
    }
    /// Native event name and payload → normalised event. `None` → do nothing.
    fn translate(&self, event: &str, p: &Value, cwd: &str) -> Option<Event>;
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
        matches!(event, "Stop" | "SessionEnd")
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

pub fn adapter(harness: &str) -> Option<&'static dyn Adapter> {
    match harness {
        "claude" => Some(&claude::Claude),
        "opencode" => Some(&opencode::OpenCode),
        "codex" => Some(&codex::Codex),
        _ => None,
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
            Call { tool, input, output: output.to_string(), failed }
        })
        .collect()
}

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
}
