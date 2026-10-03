//! Claude Code: command hooks with the payload on stdin and a JSON response on stdout
//! (code.claude.com/docs/en/hooks). Directives go to `additionalContext`; the finish line uses
//! Stop `decision: block`.

use super::{Adapter, Capabilities, Event, Harness, Output};
use adage_runtime::{By, Call};
use serde_json::{json, Value};

pub struct Claude;

/// The text of a tool response, whatever its shape: a string, or an object with stdout, stderr,
/// error, content, or output fields (content blocks contribute their `text`).
pub fn response_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Object(o) => {
            let mut s = String::new();
            for k in ["stdout", "stderr", "error", "content", "output"] {
                match o.get(k) {
                    Some(Value::String(x)) if !x.is_empty() => {
                        s.push_str(x);
                        s.push('\n');
                    }
                    Some(Value::Array(items)) => {
                        for it in items {
                            if let Some(t) = it.get("text").and_then(|t| t.as_str()) {
                                s.push_str(t);
                                s.push('\n');
                            }
                        }
                    }
                    _ => {}
                }
            }
            s
        }
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// One call from a payload (a `PostToolBatch` entry, or a `PostToolUse(Failure)` payload): its
/// `tool_use_id` is kept, and a call whose id Adage minted (`adage_…`, a predicted step the mod
/// made in the agent's place) is Adage's ([`By`]). `agent` is the payload's `agent_id`: set when a
/// subagent made the calls.
fn call_from(v: &Value, failed: bool, agent: Option<&str>) -> Call {
    let mut output = response_text(v.get("tool_response").unwrap_or(&Value::Null));
    if let Some(e) = v.get("error").and_then(|e| e.as_str()) {
        output.push_str(e);
    }
    let tool_use_id = v.get("tool_use_id").and_then(Value::as_str).filter(|id| !id.is_empty()).map(str::to_string);
    Call {
        tool: v.get("tool_name").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        input: v.get("tool_input").cloned().unwrap_or(Value::Null),
        output,
        failed,
        by: tool_use_id.as_deref().map_or(By::Agent, By::of_tool_use_id),
        tool_use_id,
        agent: v.get("agent_id").and_then(Value::as_str).or(agent).filter(|a| !a.is_empty()).map(str::to_string),
    }
}

/// The events only the Claude Code mod sends: a command hook is never registered for them.
pub const MOD_EVENTS: [&str; 3] = ["TurnStep", "Standing", crate::skill::EVENT];

/// Whether the Claude Code mod sent this payload ([`Claude::capabilities`]).
pub fn is_mods(event: &str, p: &Value) -> bool {
    p.get("via").and_then(Value::as_str) == Some("mod")
        || MOD_EVENTS.contains(&event)
        || (event == "PreToolUse" && crate::phase::Phase::of(p).is_some())
}

/// A `TurnStep` payload (the mod's `turn.step`): `turn_id` and `index` are required.
fn turn_step(p: &Value) -> Option<Event> {
    let s = |k: &str| p.get(k).and_then(Value::as_str).filter(|v| !v.is_empty()).map(str::to_string);
    Some(Event::TurnStep {
        turn_id: s("turn_id")?,
        index: u32::try_from(p.get("index")?.as_u64()?).ok()?,
        model: s("model"),
        agent: s("agent_id"),
        ending: p.get("ending").and_then(Value::as_bool).unwrap_or(false),
    })
}

impl Adapter for Claude {
    fn harness(&self) -> Harness {
        Harness::ClaudeCode
    }

    /// The mod's set ([`Harness::capabilities_with_mod`]) for a payload the mod sent: one that says
    /// so (`via: "mod"`), one of the events only the mod sends (`TurnStep`, `Standing`,
    /// `SkillPrompt`), or a `PreToolUse` carrying a phase only the mod asks for (`dry_run`,
    /// `verdict`). A command hook's payload has none of these: the command hooks' set.
    fn capabilities(&self, event: &str, p: &Value) -> Capabilities {
        if is_mods(event, p) {
            Harness::ClaudeCode.capabilities_with_mod()
        } else {
            Harness::ClaudeCode.capabilities()
        }
    }

    fn event_identity(&self, event: &str, p: &Value) -> Option<String> {
        let ids = match event {
            "PostToolBatch" => p
                .get("tool_calls")?
                .as_array()?
                .iter()
                .map(|call| call.get("tool_use_id")?.as_str())
                .collect::<Option<Vec<_>>>()?,
            // One decision per Bash call (`PreToolUse`), however many times it is delivered: the
            // mod's verdict and the command hook that follows it carry the same id.
            "PostToolUse" | "PostToolUseFailure" | "PreToolUse" => vec![p.get("tool_use_id")?.as_str()?],
            _ => return None,
        };
        super::tool_event_identity(self.name(), event, ids)
    }

    fn translate(&self, event: &str, p: &Value, _cwd: &str) -> Option<Event> {
        let agent = p.get("agent_id").and_then(Value::as_str);
        Some(match event {
            "SessionStart" => Event::SessionStart,
            "UserPromptSubmit" => {
                // How Claude Code carries a Stop `block` reason onward is not documented: should
                // Adage's own continuation ever come back as a prompt, it is not the person's.
                let text = p.get("prompt")?.as_str()?;
                if super::is_own_text(text) {
                    return None;
                }
                Event::Prompt { text: text.to_string() }
            }
            "PostToolBatch" => Event::Batch {
                calls: p.get("tool_calls")?.as_array()?.iter().map(|c| call_from(c, false, agent)).collect(),
            },
            "PostToolUse" => Event::Batch { calls: vec![call_from(p, false, None)] },
            "PostToolUseFailure" => Event::Failure { call: call_from(p, true, None) },
            "Stop" => {
                Event::Stop { stop_hook_active: p.get("stop_hook_active").and_then(|x| x.as_bool()).unwrap_or(false) }
            }
            "SessionEnd" => Event::SessionEnd,
            "TurnStep" => turn_step(p)?,
            _ => return None,
        })
    }

    fn render(&self, event: &str, out: Output) -> Option<String> {
        Some(match out {
            Output::Continue(reason) => json!({ "decision": "block", "reason": reason }).to_string(),
            Output::Context(text) => {
                json!({ "hookSpecificOutput": { "hookEventName": event, "additionalContext": text } }).to_string()
            }
            Output::SessionContext { context, notice } => {
                let mut value = json!({ "hookSpecificOutput": { "hookEventName": event, "additionalContext": context } });
                if let Some(notice) = notice {
                    value["systemMessage"] = json!(notice);
                }
                value.to_string()
            }
            // Shown to the person as a hook system message; the model never receives it (Claude
            // Code 2.1: `hook_system_message` attachments are not sent to the API).
            Output::Notice(text) => json!({ "systemMessage": text }).to_string(),
            // `updatedInput` with no `permissionDecision`: Claude Code applies the new input and
            // then runs its own permission checks on it, as for the original (2.1.28x).
            Output::Rewrite { input, context, notice } => json!({
                "systemMessage": notice,
                "hookSpecificOutput": { "hookEventName": "PreToolUse", "updatedInput": input, "additionalContext": context }
            })
            .to_string(),
            // The mod's `turn.step` answer: the calls it yields in the model's place.
            Output::Predict(p) => json!({ "predict": p }).to_string(),
            // The step goes to the person's routine model (the mod sends it there only when one is set).
            Output::Routine => json!({ "routine": true }).to_string(),
        })
    }

    /// Claude Code caps `additionalContext` at 10,000 characters.
    fn max_context(&self) -> usize {
        9_500
    }
}

/// A Claude Code `hooks` settings block. Exec form (`args`), per spike S1: no shell.
pub fn settings(bin: &str) -> Value {
    let h = |ev: &str, timeout: u32| json!([{ "hooks": [{ "type": "command", "command": bin, "args": ["hook", "claude", ev], "timeout": timeout }] }]);
    json!({
        "hooks": {
            "SessionStart": h("SessionStart", 5),
            "UserPromptSubmit": h("UserPromptSubmit", 5),
            "PostToolBatch": h("PostToolBatch", super::action_timeout(bin)),
            "PostToolUseFailure": h("PostToolUseFailure", 5),
            // Only shell calls: the project's environment for a Python test run or repro.
            "PreToolUse": [{ "matcher": "Bash", "hooks": [{ "type": "command", "command": bin, "args": ["hook", "claude", "PreToolUse"], "timeout": 5 }] }],
            "Stop": h("Stop", super::action_timeout(bin)),
            "SessionEnd": h("SessionEnd", 5)
        }
    })
}
