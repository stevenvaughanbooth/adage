//! Codex CLI: command hooks in `hooks.json`, modelled on Claude Code's contract
//! (developers.openai.com/codex/hooks). `additionalContext` from SessionStart, UserPromptSubmit,
//! and PostToolUse becomes developer context; Stop `decision: block` continues the turn with the
//! reason as a new *user* prompt, so the reason must say plainly that it is not from the user
//! (it does: `render::stop_reason`), and Adage must not treat it as a prompt when it comes back.
//!
//! Gaps (docs/harnesses.md): there are no batches (PostToolUse fires per call), and PostToolUse
//! carries no failure signal, so failure-triggered routes cannot fire on Codex today.

use super::claude::response_text;
use super::{calls, is_own_text, Adapter, Event, Output};
use adage_core::tools::from_codex;
use serde_json::{json, Value};

pub struct Codex;

/// A structured exit code, if a Codex version supplies one. None documented today.
fn failed(resp: &Value) -> bool {
    ["exit_code", "exitCode"].iter().any(|k| resp.get(*k).and_then(|v| v.as_i64()).is_some_and(|c| c != 0))
}

impl Adapter for Codex {
    fn name(&self) -> &'static str {
        "codex"
    }

    fn translate(&self, event: &str, p: &Value, cwd: &str) -> Option<Event> {
        let tool_call = |failure: bool| -> Option<Event> {
            let tool = p.get("tool_name")?.as_str()?;
            let input = p.get("tool_input").cloned().unwrap_or(Value::Null);
            let resp = p.get("tool_response").unwrap_or(&Value::Null);
            let mut output = response_text(resp);
            if let Some(e) = p.get("error").and_then(|e| e.as_str()) {
                output.push_str(e);
            }
            let failure = failure || failed(resp);
            let mut cs = calls(from_codex(tool, &input), cwd, &output, failure);
            Some(if failure && cs.len() == 1 {
                Event::Failure { call: cs.remove(0) }
            } else {
                Event::Batch { calls: cs }
            })
        };
        Some(match event {
            "SessionStart" => Event::SessionStart,
            "UserPromptSubmit" => {
                let text = p.get("prompt")?.as_str()?;
                if is_own_text(text) {
                    return None;
                }
                Event::Prompt { text: text.to_string() }
            }
            "PostToolUse" => tool_call(false)?,
            // Named in Codex's hook vocabulary but not documented, and reported never to fire
            // (openai/codex#34289). Accepted so that it works if it ever does.
            "PostToolUseFailure" => tool_call(true)?,
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
            // Codex documents no person-only channel: notices go to the card and `adage watch`.
            Output::Notice(_) | Output::Rewrite { .. } => return None,
        })
    }
}

/// Shell-quote one word for a Codex hook `command` (a shell string; no exec form is documented).
pub fn sh_quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "/._-+:@".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r#"'\''"#))
    }
}

/// A Codex `hooks.json` document. `SessionEnd` is capped at 3 seconds by Codex.
pub fn settings(bin: &str) -> Value {
    let h = |ev: &str, timeout: u32| json!([{ "hooks": [{ "type": "command", "command": format!("{} hook codex {ev}", sh_quote(bin)), "timeout": timeout }] }]);
    json!({
        "hooks": {
            "SessionStart": h("SessionStart", 5),
            "UserPromptSubmit": h("UserPromptSubmit", 5),
            "PostToolUse": h("PostToolUse", super::action_timeout(bin)),
            "Stop": h("Stop", super::action_timeout(bin)),
            "SessionEnd": h("SessionEnd", 2)
        }
    })
}
