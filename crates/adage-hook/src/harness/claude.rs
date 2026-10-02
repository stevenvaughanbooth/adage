//! Claude Code: command hooks with the payload on stdin and a JSON response on stdout
//! (code.claude.com/docs/en/hooks). Directives go to `additionalContext`; the finish line uses
//! Stop `decision: block`.

use super::{Adapter, Event, Harness, Output};
use adage_runtime::Call;
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

fn call_from(v: &Value, failed: bool) -> Call {
    let mut output = response_text(v.get("tool_response").unwrap_or(&Value::Null));
    if let Some(e) = v.get("error").and_then(|e| e.as_str()) {
        output.push_str(e);
    }
    Call {
        tool: v.get("tool_name").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        input: v.get("tool_input").cloned().unwrap_or(Value::Null),
        output,
        failed,
    }
}

impl Adapter for Claude {
    fn harness(&self) -> Harness {
        Harness::ClaudeCode
    }

    fn event_identity(&self, event: &str, p: &Value) -> Option<String> {
        let ids = match event {
            "PostToolBatch" => p
                .get("tool_calls")?
                .as_array()?
                .iter()
                .map(|call| call.get("tool_use_id")?.as_str())
                .collect::<Option<Vec<_>>>()?,
            "PostToolUse" | "PostToolUseFailure" => vec![p.get("tool_use_id")?.as_str()?],
            _ => return None,
        };
        super::tool_event_identity(self.name(), event, ids)
    }

    fn translate(&self, event: &str, p: &Value, _cwd: &str) -> Option<Event> {
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
            "PostToolBatch" => {
                Event::Batch { calls: p.get("tool_calls")?.as_array()?.iter().map(|c| call_from(c, false)).collect() }
            }
            "PostToolUse" => Event::Batch { calls: vec![call_from(p, false)] },
            "PostToolUseFailure" => Event::Failure { call: call_from(p, true) },
            "Stop" => {
                Event::Stop { stop_hook_active: p.get("stop_hook_active").and_then(|x| x.as_bool()).unwrap_or(false) }
            }
            "SessionEnd" => Event::SessionEnd,
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
