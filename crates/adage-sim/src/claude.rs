//! Claude Code: transcripts as `~/.claude/projects/<project>/<session>.jsonl`, and hook payloads
//! (code.claude.com/docs/en/hooks). One record per content block; the blocks of one model turn
//! share `message.id`; tool results come back as user records with `tool_result` blocks.

use crate::scenario::{uuid4, Act, Rng, Session, Turn};
use serde_json::{json, Value};

pub const MODEL: &str = "claude-sonnet-4-6";
const VERSION: &str = "2.1.87";

pub fn session_id(s: &Session) -> String {
    uuid4(&mut Rng::new(s.seed ^ 0xc1a0))
}

/// The project directory name Claude Code derives from a cwd.
pub fn project_dir(cwd: &str) -> String {
    cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

/// (tool name, input, result content, is_error) for one call.
pub fn native(act: &Act, cwd: &str) -> (&'static str, Value, String, bool) {
    match act {
        Act::Shell { cmd, out, exit } => {
            let input = json!({ "command": cmd, "description": describe(cmd) });
            if *exit != 0 {
                ("Bash", input, format!("Exit code {exit}\n{out}"), true)
            } else {
                ("Bash", input, out.trim_end().to_string(), false)
            }
        }
        Act::Read { path, body } => {
            let numbered: String = body.lines().enumerate().map(|(i, l)| format!("{:>6}\t{l}\n", i + 1)).collect();
            ("Read", json!({ "file_path": format!("{cwd}/{path}") }), numbered, false)
        }
        Act::Edit { path, old, new } => (
            "Edit",
            json!({ "file_path": format!("{cwd}/{path}"), "old_string": old, "new_string": new, "replace_all": false }),
            format!("The file {cwd}/{path} has been updated successfully."),
            false,
        ),
    }
}

fn describe(cmd: &str) -> String {
    let first = cmd.split_whitespace().next().unwrap_or("");
    match first {
        "make" => format!("Run {cmd}"),
        "ls" => "List repository files".into(),
        "rg" => "Search the repository".into(),
        _ => format!("Run {first}"),
    }
}

/// A transcript: the prompt, one assistant record per tool call (sharing the turn's message id),
/// one user record per result, and the final answer.
pub fn transcript(s: &Session, cwd: &str) -> String {
    let sid = session_id(s);
    let mut rng = Rng::new(s.seed ^ 0x7a11);
    let mut t = s.start_ms;
    let mut parent: Option<String> = None;
    let mut lines: Vec<Value> = Vec::new();
    let rec = |lines: &mut Vec<Value>, parent: &mut Option<String>, rng: &mut Rng, t: i64, mut v: Value| {
        let uuid = uuid4(rng);
        let base = json!({
            "parentUuid": parent.clone(), "isSidechain": false, "userType": "external", "cwd": cwd,
            "sessionId": sid, "version": VERSION, "gitBranch": "main", "uuid": uuid, "timestamp": crate::scenario::iso(t)
        });
        for (k, x) in base.as_object().unwrap() {
            v[k] = x.clone();
        }
        *parent = Some(uuid);
        lines.push(v);
    };
    rec(
        &mut lines,
        &mut parent,
        &mut rng,
        t,
        json!({ "type": "user", "message": { "role": "user", "content": s.prompt } }),
    );
    for turn in &s.turns {
        t += 4_000 + rng.below(20_000) as i64;
        let mid = format!("msg_01{}", rng.base62(22));
        let req = format!("req_011C{}", rng.base62(18));
        if let Some(say) = &turn.say {
            rec(&mut lines, &mut parent, &mut rng, t, assistant(&mid, &req, json!([{ "type": "text", "text": say }])));
        }
        let mut ids = Vec::new();
        for c in &turn.calls {
            let (name, input, _, _) = native(&c.act, cwd);
            let id = format!("toolu_01{}", rng.base62(22));
            rec(
                &mut lines,
                &mut parent,
                &mut rng,
                t,
                assistant(&mid, &req, json!([{ "type": "tool_use", "id": id, "name": name, "input": input }])),
            );
            ids.push(id);
        }
        for (c, id) in turn.calls.iter().zip(ids) {
            t += 300 + rng.below(2_000) as i64;
            let (_, _, content, is_error) = native(&c.act, cwd);
            let result = tool_use_result(&c.act);
            rec(
                &mut lines,
                &mut parent,
                &mut rng,
                t,
                json!({ "type": "user", "message": { "role": "user", "content": [{ "tool_use_id": id, "type": "tool_result", "content": content, "is_error": is_error }] }, "toolUseResult": result }),
            );
        }
    }
    t += 3_000;
    let mid = format!("msg_01{}", rng.base62(22));
    let req = format!("req_011C{}", rng.base62(18));
    rec(&mut lines, &mut parent, &mut rng, t, assistant(&mid, &req, json!([{ "type": "text", "text": s.answer }])));
    lines.iter().map(|l| format!("{l}\n")).collect()
}

fn assistant(mid: &str, req: &str, content: Value) -> Value {
    json!({
        "type": "assistant",
        "requestId": req,
        "message": { "id": mid, "type": "message", "role": "assistant", "model": MODEL, "content": content, "stop_reason": null,
            "usage": { "input_tokens": 4, "cache_read_input_tokens": 18_000, "output_tokens": 120 } }
    })
}

fn tool_use_result(act: &Act) -> Value {
    match act {
        Act::Shell { out, exit, .. } if *exit != 0 => json!(format!("Error: Exit code {exit}\n{out}")),
        Act::Shell { out, .. } => {
            json!({ "stdout": out.trim_end(), "stderr": "", "interrupted": false, "isImage": false })
        }
        Act::Read { path, body } => {
            json!({ "type": "text", "file": { "filePath": path, "content": body, "numLines": body.lines().count(), "startLine": 1 } })
        }
        Act::Edit { path, old, new } => {
            json!({ "filePath": path, "oldString": old, "newString": new, "userModified": false })
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Hooks

pub fn hook(sid: &str, cwd: &str, event: &str, extra: Value) -> Value {
    let mut b = json!({ "session_id": sid, "transcript_path": format!("/home/dev/.claude/projects/{}/{sid}.jsonl", project_dir(cwd)), "cwd": cwd, "hook_event_name": event, "permission_mode": "default" });
    for (k, v) in extra.as_object().into_iter().flatten() {
        b[k] = v.clone();
    }
    b
}

/// The hook calls one turn produces: a `PostToolUseFailure` per failed call, then one
/// `PostToolBatch` with the rest. Each comes with the route prefixes it should draw.
pub fn turn_hooks(turn: &Turn, sid: &str, cwd: &str, rng: &mut Rng) -> Vec<(&'static str, Value, Vec<&'static str>)> {
    let mut out = Vec::new();
    let mut batch = Vec::new();
    let mut expect = Vec::new();
    for c in &turn.calls {
        let (name, input, content, is_error) = native(&c.act, cwd);
        let id = format!("toolu_01{}", rng.base62(22));
        if is_error {
            out.push(("PostToolUseFailure", hook(sid, cwd, "PostToolUseFailure", json!({ "tool_name": name, "tool_input": input, "tool_use_id": id, "error": content, "is_interrupt": false })), c.expect.clone()));
        } else {
            batch.push(json!({ "tool_name": name, "tool_input": input, "tool_use_id": id, "tool_response": tool_use_result(&c.act) }));
            expect.extend(c.expect.iter().copied());
        }
    }
    if !batch.is_empty() {
        out.push(("PostToolBatch", hook(sid, cwd, "PostToolBatch", json!({ "tool_calls": batch })), expect));
    }
    out
}
