//! OpenCode: `opencode export <sessionID>` files (`{ info, messages: [{ info, parts }] }`, the
//! SessionV1 types of `packages/schema/src/v1/session.ts`, anomalyco/opencode v1.18.32), the
//! payloads Adage's plugin sends to `adage hook opencode <event>`, and a script that drives the
//! plugin itself (`plugin/opencode/test/session.test.mjs`) with a fake client.

use crate::scenario::{iso, Act, Replay, Rng, Session, Turn};
use serde_json::{json, Value};

pub const PROVIDER: &str = "anthropic";
pub const MODEL: &str = "claude-sonnet-4-6";
const VERSION: &str = "1.18.32";

/// OpenCode's identifier: prefix, 12 hex digits of (time × 4096 + counter), bitwise inverted for
/// descending ids (sessions), then 14 base62 characters (`packages/schema/src/identifier.ts`).
pub fn ident(prefix: &str, ms: i64, counter: u64, descending: bool, rng: &mut Rng) -> String {
    let cur = (ms as u64).wrapping_mul(0x1000).wrapping_add(counter);
    let v = if descending { !cur } else { cur } & 0xffff_ffff_ffff;
    format!("{prefix}_{v:012x}{}", rng.base62(14))
}

pub fn session_id(s: &Session) -> String {
    ident("ses", s.start_ms, 1, true, &mut Rng::new(s.seed ^ 0x0c0d))
}

/// (tool, input, output, metadata) for one call. Edits alternate between `edit` and `apply_patch`
/// (both are built-in tools; which one a model uses depends on the model).
pub fn native(act: &Act, cwd: &str, patch: bool) -> (&'static str, Value, String, Value) {
    match act {
        Act::Shell { cmd, out, exit } => {
            let output = if out.is_empty() { "(no output)".to_string() } else { out.clone() };
            (
                "bash",
                json!({ "command": cmd, "description": format!("Run {cmd}") }),
                output.clone(),
                json!({ "output": output.lines().last().unwrap_or(""), "exit": exit, "truncated": false }),
            )
        }
        Act::Read { path, body } => {
            let lines: String = body.lines().enumerate().map(|(i, l)| format!("{}: {l}\n", i + 1)).collect();
            let output = format!("<path>{cwd}/{path}</path>\n<type>file</type>\n<content>\n{lines}\n(End of file - total {} lines)\n</content>", body.lines().count());
            ("read", json!({ "filePath": format!("{cwd}/{path}") }), output, json!({ "truncated": false }))
        }
        Act::Edit { path, old, new } if patch => (
            "apply_patch",
            json!({ "patchText": patch_text(path, old, new) }),
            format!("Success. Updated the following files:\nM {path}"),
            json!({ "files": [{ "filePath": format!("{cwd}/{path}"), "type": "update" }], "diagnostics": {} }),
        ),
        Act::Edit { path, old, new } => (
            "edit",
            json!({ "filePath": format!("{cwd}/{path}"), "oldString": old, "newString": new }),
            "Edit applied successfully.".to_string(),
            json!({ "diagnostics": {}, "filediff": { "file": format!("{cwd}/{path}"), "additions": new.lines().count(), "deletions": old.lines().count() } }),
        ),
    }
}

/// A Codex-format patch (OpenCode's `apply_patch` and Codex share the format).
pub fn patch_text(path: &str, old: &str, new: &str) -> String {
    let minus: String = old.lines().map(|l| format!("-{l}\n")).collect();
    let plus: String = new.lines().map(|l| format!("+{l}\n")).collect();
    format!("*** Begin Patch\n*** Update File: {path}\n@@\n{minus}{plus}*** End Patch")
}

/// The export `opencode export <id>` would print for this session.
pub fn export(s: &Session, cwd: &str) -> Value {
    let sid = session_id(s);
    let mut rng = Rng::new(s.seed ^ 0x0e0e);
    let patch = s.seed.is_multiple_of(2);
    let mut t = s.start_ms;
    let mut n = 0u64;
    let mut id = |prefix: &str, t: i64, rng: &mut Rng| {
        n += 1;
        ident(prefix, t, n, false, rng)
    };
    let tokens = json!({ "input": 1200, "output": 90, "reasoning": 0, "cache": { "read": 18_000, "write": 0 } });
    let user_id = id("msg", t, &mut rng);
    let mut messages = vec![json!({
        "info": { "id": user_id, "sessionID": sid, "role": "user", "time": { "created": t }, "agent": "build", "model": { "providerID": PROVIDER, "modelID": MODEL } },
        "parts": [{ "id": id("prt", t, &mut rng), "sessionID": sid, "messageID": user_id, "type": "text", "text": s.prompt }]
    })];
    let assistant = |t0: i64, t1: i64, parts: Vec<Value>, mid: String, finish: &str| {
        json!({
            "info": { "id": mid, "sessionID": sid, "role": "assistant", "time": { "created": t0, "completed": t1 }, "parentID": user_id,
                "modelID": MODEL, "providerID": PROVIDER, "mode": "build", "agent": "build", "path": { "cwd": cwd, "root": cwd },
                "cost": 0.0123, "tokens": tokens, "finish": finish },
            "parts": parts
        })
    };
    for turn in &s.turns {
        let t0 = t + 1_000;
        t += 4_000 + rng.below(20_000) as i64;
        let mid = id("msg", t0, &mut rng);
        let mut parts =
            vec![json!({ "id": id("prt", t0, &mut rng), "sessionID": sid, "messageID": mid, "type": "step-start" })];
        if let Some(say) = &turn.say {
            parts.push(json!({ "id": id("prt", t0, &mut rng), "sessionID": sid, "messageID": mid, "type": "text", "text": say, "time": { "start": t0, "end": t0 + 400 } }));
        }
        for c in &turn.calls {
            let (tool, input, output, metadata) = native(&c.act, cwd, patch);
            let title = match &c.act {
                Act::Shell { cmd, .. } => cmd.clone(),
                Act::Read { path, .. } | Act::Edit { path, .. } => path.clone(),
            };
            parts.push(json!({
                "id": id("prt", t0, &mut rng), "sessionID": sid, "messageID": mid, "type": "tool", "callID": format!("toolu_{}", rng.base62(24)), "tool": tool,
                "state": { "status": "completed", "input": input, "output": output, "title": title, "metadata": metadata, "time": { "start": t0 + 500, "end": t - 200 } }
            }));
        }
        parts.push(json!({ "id": id("prt", t, &mut rng), "sessionID": sid, "messageID": mid, "type": "step-finish", "reason": "tool-calls", "cost": 0.0123, "tokens": tokens }));
        messages.push(assistant(t0, t, parts, mid, "tool-calls"));
    }
    let t0 = t + 1_000;
    t += 3_000;
    let mid = id("msg", t0, &mut rng);
    let parts = vec![
        json!({ "id": id("prt", t0, &mut rng), "sessionID": sid, "messageID": mid, "type": "step-start" }),
        json!({ "id": id("prt", t0, &mut rng), "sessionID": sid, "messageID": mid, "type": "text", "text": s.answer, "time": { "start": t0, "end": t } }),
        json!({ "id": id("prt", t, &mut rng), "sessionID": sid, "messageID": mid, "type": "step-finish", "reason": "stop", "cost": 0.0123, "tokens": tokens }),
    ];
    messages.push(assistant(t0, t, parts, mid, "stop"));
    let title: String = s.prompt.chars().take(40).collect();
    json!({
        "info": { "id": sid, "slug": format!("{}-{}", rng.base62(6).to_lowercase(), rng.base62(6).to_lowercase()), "projectID": "prj_acmebilling0000", "directory": cwd,
            "title": title, "version": VERSION, "time": { "created": s.start_ms, "updated": t } },
        "messages": messages
    })
}

/// The day an export was written, for fixture file names.
pub fn file_name(s: &Session) -> String {
    format!("{}-{}.json", &iso(s.start_ms)[..10], session_id(s))
}

// ---------------------------------------------------------------------------------------------
// Hooks: what plugin/opencode/adage.js sends to `adage hook opencode <event>`

pub fn payload(sid: &str, cwd: &str, extra: Value) -> Value {
    let mut b = json!({ "session_id": sid, "cwd": cwd });
    for (k, v) in extra.as_object().into_iter().flatten() {
        b[k] = v.clone();
    }
    b
}

/// One `tool.execute.after` per call (OpenCode has no batches), with the route prefixes each draws.
pub fn turn_hooks(turn: &Turn, sid: &str, cwd: &str) -> Vec<(&'static str, Value, Vec<&'static str>)> {
    turn.calls
        .iter()
        .map(|c| {
            let (tool, args, output, meta) = native(&c.act, cwd, true);
            ("tool.execute.after", payload(sid, cwd, json!({ "tool": tool, "args": args, "output": output, "exit": meta.get("exit").cloned().unwrap_or(Value::Null) })), c.expect.clone())
        })
        .collect()
}

/// A script for the node driver: each step calls one plugin hook, exactly as OpenCode would, and
/// says what the fake client should see afterwards. With `treated = false` (the holdout arm)
/// every expectation is silence.
pub fn plugin_script(r: &Replay, sid: &str, treated: bool) -> Value {
    let x = |v: &[&str]| if treated { json!(v) } else { json!([]) };
    let mut steps = vec![
        json!({ "hook": "event", "event": { "type": "session.created", "properties": { "info": { "id": sid } } } }),
        json!({ "hook": "chat.message", "text": r.prompt, "expect": { "context": x(&r.prompt_expect) } }),
    ];
    let mut n = 0;
    let mut tools = |turns: &[Turn], steps: &mut Vec<Value>| {
        for t in turns {
            for c in &t.calls {
                n += 1;
                let (tool, args, output, metadata) = native(&c.act, "{REPO}", true);
                steps.push(json!({ "hook": "tool.execute.after", "tool": tool, "callID": format!("call_{n}"), "args": args, "output": output, "metadata": metadata, "expect": { "appended": x(&c.expect) } }));
            }
        }
    };
    tools(&r.work, &mut steps);
    steps.push(json!({ "hook": "event", "event": { "type": "session.idle", "properties": { "sessionID": sid } }, "expect": { "continue": treated } }));
    tools(&r.finish, &mut steps);
    steps.push(json!({ "hook": "event", "event": { "type": "session.idle", "properties": { "sessionID": sid } }, "expect": { "continue": false } }));
    steps.push(
        json!({ "hook": "event", "event": { "type": "session.deleted", "properties": { "info": { "id": sid } } } }),
    );
    json!({ "session": sid, "steps": steps })
}
