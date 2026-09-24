//! OpenCode: an in-process JS plugin (`plugin/opencode/adage.js`) spawns `adage hook opencode
//! <event>` with a small JSON payload of its own design, built from the documented plugin hooks
//! (opencode.ai/docs/plugins). The response is `{"context": …}` or `{"continue": …}`; the plugin
//! decides where the text goes. See docs/harnesses.md.
//!
//! | Event (plugin → adage) | From | Payload fields besides `session_id`, `cwd` |
//! | --- | --- | --- |
//! | `session.created` | `event` hook, `session.created` | — |
//! | `chat.message` | `chat.message` hook, user text parts | `prompt` |
//! | `tool.execute.after` | `tool.execute.after` hook | `tool`, `args`, `output`, `exit` |
//! | `tool.error` | `event` hook, `message.part.updated` with a tool part in state `error` | `tool`, `args`, `error` |
//! | `session.idle` | `event` hook, `session.idle` | `stop_hook_active` |
//! | `session.deleted` | `event` hook, `session.deleted` | — |

use super::{calls, is_own_text, Adapter, Event, Output};
use adage_core::tools::from_opencode;
use serde_json::{json, Value};

pub struct OpenCode;

impl Adapter for OpenCode {
    fn name(&self) -> &'static str {
        "opencode"
    }

    fn translate(&self, event: &str, p: &Value, cwd: &str) -> Option<Event> {
        let s = |k: &str| p.get(k).and_then(|v| v.as_str()).unwrap_or("");
        let native = || {
            from_opencode(p.get("tool").and_then(|v| v.as_str()).unwrap_or(""), p.get("args").unwrap_or(&Value::Null))
        };
        Some(match event {
            "session.created" => Event::SessionStart,
            "chat.message" => {
                let text = p.get("prompt")?.as_str()?;
                if is_own_text(text) {
                    return None;
                }
                Event::Prompt { text: text.to_string() }
            }
            "tool.execute.after" => {
                p.get("tool")?.as_str()?;
                // The shell tool reports a non-zero exit in `metadata.exit` and returns normally.
                let failed = p.get("exit").and_then(|v| v.as_i64()).is_some_and(|c| c != 0);
                let mut cs = calls(native(), cwd, s("output"), failed);
                if failed && cs.len() == 1 {
                    Event::Failure { call: cs.remove(0) }
                } else {
                    Event::Batch { calls: cs }
                }
            }
            "tool.error" => {
                p.get("tool")?.as_str()?;
                let mut cs = calls(native(), cwd, s("error"), true);
                if cs.len() != 1 {
                    return None;
                }
                Event::Failure { call: cs.remove(0) }
            }
            "session.idle" => {
                Event::Stop { stop_hook_active: p.get("stop_hook_active").and_then(|x| x.as_bool()).unwrap_or(false) }
            }
            "session.deleted" => Event::SessionEnd,
            _ => return None,
        })
    }

    fn is_learn_point(&self, event: &str) -> bool {
        matches!(event, "session.idle" | "session.deleted")
    }

    fn render(&self, _event: &str, out: Output) -> Option<String> {
        Some(match out {
            Output::Continue(reason) => json!({ "continue": reason }).to_string(),
            Output::Context(text) => json!({ "context": text }).to_string(),
        })
    }
}

/// The plugin source, with the adage binary's absolute path filled in.
pub fn plugin_source(bin: &str) -> String {
    const SRC: &str = include_str!("../../../../plugin/opencode/adage.js");
    // A JSON string literal is a valid JS string literal: no escaping surprises from the path.
    SRC.replace("\"__ADAGE_BIN__\"", &serde_json::to_string(bin).unwrap_or_else(|_| "\"adage\"".into()))
}
