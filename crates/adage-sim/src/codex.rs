//! Codex CLI: rollout files (`$CODEX_HOME/sessions/YYYY/MM/DD/rollout-<time>-<thread id>.jsonl`)
//! and hook payloads. Record shapes follow openai/codex at tag rust-v0.156.1 (2026-09-23), read
//! from source, not a documented interface:
//!
//! - each line is `RolloutLine { timestamp, ordinal?, type, payload }`
//!   (`codex-rs/history/src/lib.rs`, `rollout_payload.rs`);
//! - `session_meta` (`SessionMetaLine`), `turn_context`, `response_item` (`ResponseItem`,
//!   `codex-rs/protocol/src/models.rs`), `event_msg` (`EventMsg`, `protocol.rs`), persisted per
//!   `codex-rs/rollout/src/policy.rs` (legacy history mode, the default);
//! - shell calls are `function_call` `exec_command` with JSON-string arguments `{cmd, workdir}`;
//!   the output text starts with a header that carries the exit code
//!   (`Process exited with code N`, `codex-rs/core/src/tools/context.rs`);
//! - `apply_patch` is a freeform tool: `custom_tool_call` with the patch as `input`;
//! - hook `additionalContext` is recorded as a `developer` message (`core/src/hook_runtime.rs`).
//!
//! Older CLIs (checked at rust-v0.40.0) used a `shell` function with an argv array
//! (`["bash", "-lc", cmd]`) and a JSON output `{output, metadata: {exit_code, duration_seconds}}`;
//! every third session (`is_legacy`) is rendered in that form.

use crate::opencode::patch_text;
use crate::scenario::{iso, uuid7, Act, Rng, Session, Turn};
use serde_json::{json, Value};

pub const MODEL: &str = "gpt-5.3-codex";

pub fn session_id(s: &Session) -> String {
    uuid7(&mut Rng::new(s.seed ^ 0xc0de), s.start_ms)
}

/// `sessions/YYYY/MM/DD/rollout-YYYY-MM-DDThh-mm-ss-<id>.jsonl` (`rollout/src/rollout_file_name.rs`).
pub fn rel_path(s: &Session) -> String {
    let ts = iso(s.start_ms);
    let (date, time) = (&ts[..10], ts[11..19].replace(':', "-"));
    format!("sessions/{}/{}/{}/rollout-{date}T{time}-{}.jsonl", &date[..4], &date[5..7], &date[8..10], session_id(s))
}

/// Legacy-format sessions: every third, by seed, so both shapes are always present.
pub fn is_legacy(s: &Session) -> bool {
    s.seed.is_multiple_of(3)
}

fn exec_output(out: &str, exit: i32, rng: &mut Rng, legacy: bool) -> String {
    if legacy {
        return json!({ "output": out, "metadata": { "exit_code": exit, "duration_seconds": 0.1 } }).to_string();
    }
    let tokens = out.len() / 4;
    format!(
        "Chunk ID: {}\nWall time: 0.{:04} seconds\nProcess exited with code {exit}\nOriginal token count: {tokens}\nOutput:\n{out}",
        rng.hex(6),
        rng.below(9000) + 1000
    )
}

/// The shell command a model would issue for an act (Codex has no read or edit tool besides
/// `apply_patch`; files are read through the shell).
pub fn shell_of(act: &Act, rng: &mut Rng) -> Option<(String, String, i32)> {
    match act {
        Act::Shell { cmd, out, exit } => Some((cmd.clone(), out.clone(), *exit)),
        Act::Read { path, body } => {
            let cmd = if rng.chance(50) { format!("sed -n '1,200p' {path}") } else { format!("cat {path}") };
            Some((cmd, body.clone(), 0))
        }
        Act::Edit { .. } => None,
    }
}

pub fn rollout(s: &Session, cwd: &str) -> String {
    let sid = session_id(s);
    let legacy = is_legacy(s);
    let mut rng = Rng::new(s.seed ^ 0xabcd);
    let mut t = s.start_ms;
    let mut lines: Vec<String> = Vec::new();
    let mut push = |t: i64, ty: &str, payload: Value| {
        lines.push(json!({ "timestamp": iso(t), "type": ty, "payload": payload }).to_string());
    };
    let msg = |role: &str, kind: &str, text: &str| json!({ "type": "message", "role": role, "content": [{ "type": kind, "text": text }] });
    let meta = if legacy {
        json!({ "id": sid, "timestamp": iso(t), "cwd": cwd, "originator": "codex_cli_rs", "cli_version": "0.46.0", "instructions": null,
            "source": "cli", "model_provider": "openai", "git": { "commit_hash": "4f1c2d0", "branch": "main" } })
    } else {
        json!({ "session_id": sid, "id": sid, "timestamp": iso(t), "cwd": cwd, "originator": "codex_cli_rs", "cli_version": "0.156.1",
            "source": "cli", "model_provider": "openai", "base_instructions": { "text": "You are Codex, a coding agent based on GPT-5." },
            "history_mode": "legacy", "git": { "commit_hash": "4f1c2d0", "branch": "main" } })
    };
    push(t, "session_meta", meta);
    push(t, "response_item", msg("developer", "input_text", "<permissions instructions>\nFilesystem sandboxing: workspace-write. Approvals: on-request.\n</permissions instructions>"));
    push(t, "response_item", msg("user", "input_text", &format!("<environment_context>\n  <cwd>{cwd}</cwd>\n  <shell>zsh</shell>\n</environment_context>")));
    let turn_id = format!("{}", 1 + rng.below(9));
    push(t, "event_msg", json!({ "type": "task_started", "turn_id": turn_id, "model_context_window": 272_000 }));
    push(t, "turn_context", json!({ "turn_id": turn_id, "cwd": cwd, "approval_policy": "on-request", "sandbox_policy": { "type": "workspace-write", "network_access": false }, "model": MODEL, "summary": "auto" }));
    push(t, "response_item", msg("user", "input_text", &s.prompt));
    push(t, "event_msg", json!({ "type": "user_message", "message": s.prompt, "images": [] }));
    for turn in &s.turns {
        t += 3_000 + rng.below(15_000) as i64;
        push(t, "response_item", json!({ "type": "reasoning", "summary": [{ "type": "summary_text", "text": format!("**{}**", turn.say.as_deref().unwrap_or("Next step")) }], "content": null, "encrypted_content": format!("gAAAAA{}", rng.base62(40)) }));
        if let Some(say) = &turn.say {
            push(t, "response_item", json!({ "type": "message", "role": "assistant", "content": [{ "type": "output_text", "text": say }], "phase": "commentary" }));
            push(t, "event_msg", json!({ "type": "agent_message", "message": say, "phase": "commentary" }));
        }
        let mut outs = Vec::new();
        for c in &turn.calls {
            let call_id = format!("call_{}", rng.base62(24));
            match shell_of(&c.act, &mut rng) {
                Some((cmd, out, exit)) if legacy => {
                    let args = json!({ "command": ["bash", "-lc", cmd], "workdir": cwd, "timeout_ms": 120_000 }).to_string();
                    push(t, "response_item", json!({ "type": "function_call", "name": "shell", "arguments": args, "call_id": call_id }));
                    outs.push(json!({ "type": "function_call_output", "call_id": call_id, "output": exec_output(&out, exit, &mut rng, true) }));
                }
                Some((cmd, out, exit)) => {
                    let args = json!({ "cmd": cmd, "workdir": cwd, "yield_time_ms": 10_000 }).to_string();
                    push(t, "response_item", json!({ "type": "function_call", "name": "exec_command", "arguments": args, "call_id": call_id }));
                    outs.push(json!({ "type": "function_call_output", "call_id": call_id, "output": exec_output(&out, exit, &mut rng, false) }));
                }
                None => {
                    let Act::Edit { path, old, new } = &c.act else { unreachable!() };
                    let patch = patch_text(path, old, new);
                    push(t, "response_item", json!({ "type": "custom_tool_call", "status": "completed", "call_id": call_id, "name": "apply_patch", "input": patch }));
                    outs.push(json!({ "type": "custom_tool_call_output", "call_id": call_id, "output": format!("Success. Updated the following files:\nM {path}\n") }));
                }
            }
        }
        t += 200 + rng.below(3_000) as i64;
        for o in outs {
            push(t, "response_item", o);
        }
        push(t, "event_msg", json!({ "type": "token_count", "info": null, "rate_limits": null }));
    }
    t += 2_000;
    push(t, "response_item", json!({ "type": "message", "role": "assistant", "content": [{ "type": "output_text", "text": s.answer }], "phase": "final_answer" }));
    push(t, "event_msg", json!({ "type": "agent_message", "message": s.answer, "phase": "final_answer" }));
    push(t, "event_msg", json!({ "type": "task_complete", "turn_id": turn_id, "last_agent_message": s.answer }));
    lines.iter().map(|l| format!("{l}\n")).collect()
}

// ---------------------------------------------------------------------------------------------
// Hooks (developers.openai.com/codex/hooks; `codex-rs/hooks/src/schema.rs` at rust-v0.156.1)

pub fn hook(sid: &str, cwd: &str, event: &str, turn_id: &str, extra: Value) -> Value {
    let mut b = json!({
        "session_id": sid, "transcript_path": format!("/home/dev/.codex/sessions/2026/09/22/rollout-2026-09-22T09-30-00-{sid}.jsonl"),
        "cwd": cwd, "hook_event_name": event, "model": MODEL, "permission_mode": "default"
    });
    if !turn_id.is_empty() {
        b["turn_id"] = json!(turn_id);
    }
    for (k, v) in extra.as_object().into_iter().flatten() {
        b[k] = v.clone();
    }
    b
}

/// One `PostToolUse` per call. Shell calls are reported as `Bash` with `{command}` and the output
/// alone as `tool_response` — no exit code (`ExecCommandToolOutput::post_tool_use_response`);
/// `apply_patch` carries the patch in `tool_input.command`.
pub fn turn_hooks(turn: &Turn, sid: &str, cwd: &str, turn_id: &str, rng: &mut Rng) -> Vec<(&'static str, Value, Vec<&'static str>)> {
    turn.calls
        .iter()
        .map(|c| {
            let id = format!("call_{}", rng.base62(24));
            let (name, input, resp) = match shell_of(&c.act, rng) {
                Some((cmd, out, _exit)) => ("Bash", json!({ "command": cmd }), out),
                None => {
                    let Act::Edit { path, old, new } = &c.act else { unreachable!() };
                    ("apply_patch", json!({ "command": patch_text(path, old, new) }), format!("Success. Updated the following files:\nM {path}\n"))
                }
            };
            ("PostToolUse", hook(sid, cwd, "PostToolUse", turn_id, json!({ "tool_name": name, "tool_input": input, "tool_response": resp, "tool_use_id": id })), c.expect.clone())
        })
        .collect()
}
