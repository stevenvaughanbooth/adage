//! Claude Code hook client. Reads the payload on stdin, decides, prints the harness response.
//! Returns `None` for "no output" — including on every error path (fail open).

use adage_core::{repo, CommandTable};
use adage_runtime::{assign_arm, decide, render, Arm, Call, Ctx, Event, SessionState};
use adage_store::{spool, state, Paths};
use adage_tracks::Index;
use serde_json::{json, Value};
use std::io::Read;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_STDIN: u64 = 8 << 20;
const MAX_CONTEXT: usize = 9_500; // Claude Code caps additionalContext at 10,000 characters.

fn response_text(v: &Value) -> String {
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

fn to_event(name: &str, p: &Value) -> Option<Event> {
    Some(match name {
        "SessionStart" => Event::SessionStart,
        "UserPromptSubmit" => Event::Prompt { text: p.get("prompt")?.as_str()?.to_string() },
        "PostToolBatch" => Event::Batch {
            calls: p.get("tool_calls")?.as_array()?.iter().map(|c| call_from(c, false)).collect(),
        },
        "PostToolUse" => Event::Batch { calls: vec![call_from(p, false)] },
        "PostToolUseFailure" => Event::Failure { call: call_from(p, true) },
        "Stop" => Event::Stop { stop_hook_active: p.get("stop_hook_active").and_then(|x| x.as_bool()).unwrap_or(false) },
        "SessionEnd" => Event::SessionEnd,
        _ => return None,
    })
}

pub fn run(harness: &str, event: &str) -> Option<String> {
    if harness != "claude" || std::env::var("ADAGE_DISABLE").is_ok_and(|v| v == "1") {
        return None;
    }
    let mut buf = String::new();
    std::io::stdin().take(MAX_STDIN).read_to_string(&mut buf).ok()?;
    let p: Value = serde_json::from_str(&buf).ok()?;
    let session = p.get("session_id")?.as_str()?.to_string();
    let cwd = PathBuf::from(p.get("cwd")?.as_str()?);
    let root = repo::find_root(&cwd).unwrap_or(cwd);
    if root.join(".adage/disabled").exists() {
        return None;
    }
    let paths = Paths::from_env();
    let index = Index::load(&paths.index());
    let repo_key = repo::repo_key(&root);
    index.for_repo(&repo_key).next()?; // nothing installed for this repository: the common, cheapest path
    let ev = to_event(event, &p)?;
    let table = CommandTable::load();
    let ctx = Ctx { index: &index, repo: repo_key.clone(), root: &root, table: &table };
    let arm_env = std::env::var("ADAGE_ARM").ok();
    let share: f64 = std::env::var("ADAGE_HOLDOUT").ok().and_then(|v| v.parse().ok()).unwrap_or(0.1);
    let (decision, arm) = state::update::<SessionState, _>(&paths.state(&session), |st| {
        if st.arm.is_none() {
            st.arm = Some(match arm_env.as_deref() {
                Some("holdout") => Arm::Holdout,
                Some("treated") => Arm::Treated,
                _ => assign_arm(&session, share),
            });
            st.repo = Some(repo_key.clone());
        }
        (decide(&ctx, st, &ev), st.arm)
    })
    .ok()?;

    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let _ = spool::append(
        &paths.spool(&session),
        &json!({ "ts": ts, "event": event, "arm": arm, "generation": index.generation, "decision": decision }),
    );

    if let Some(reason) = decision.continue_reason {
        return Some(json!({ "decision": "block", "reason": reason }).to_string());
    }
    if decision.directives.is_empty() {
        return None;
    }
    let mut text = decision
        .directives
        .iter()
        .map(|d| render::wrap(&d.track, &d.version, &d.route, d.aspect, &d.text))
        .collect::<Vec<_>>()
        .join("\n\n");
    if text.len() > MAX_CONTEXT {
        text.truncate(text.char_indices().take_while(|(i, _)| *i < MAX_CONTEXT).last().map(|(i, _)| i).unwrap_or(0));
    }
    Some(json!({ "hookSpecificOutput": { "hookEventName": event, "additionalContext": text } }).to_string())
}

/// A Claude Code `hooks` settings block. Exec form (`args`), per spike S1: no shell.
pub fn settings(bin: &str) -> Value {
    let h = |ev: &str, timeout: u32| json!([{ "hooks": [{ "type": "command", "command": bin, "args": ["hook", "claude", ev], "timeout": timeout }] }]);
    json!({
        "hooks": {
            "SessionStart": h("SessionStart", 5),
            "UserPromptSubmit": h("UserPromptSubmit", 5),
            "PostToolBatch": h("PostToolBatch", 5),
            "PostToolUseFailure": h("PostToolUseFailure", 5),
            "Stop": h("Stop", 5),
            "SessionEnd": h("SessionEnd", 5)
        }
    })
}
