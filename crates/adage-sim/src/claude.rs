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

/// What the mod's `turn.step` asks the binary (`adage hook claude TurnStep`) at a step boundary of
/// turn `turn_id`: before request `index` (0 is the prompt's own request, `k` follows the k-th tool
/// batch), or, with `ending`, after the model's response to request `index` ended the turn. The mod
/// builds it from `$.session`, not from a command hook's payload, so it carries no transcript path
/// or permission mode; like every payload the mod sends, it says `via: "mod"`.
pub fn turn_step_hook(sid: &str, cwd: &str, turn_id: &str, index: u32, ending: bool) -> Value {
    json!({ "session_id": sid, "cwd": cwd, "hook_event_name": "TurnStep", "turn_id": turn_id, "index": index, "model": MODEL, "ending": ending, "via": "mod" })
}

/// The `PreToolUse` for a Bash call before it runs: the mod's dry run (`dry_run`: the plan,
/// spooled nowhere, `via: "mod"`), or the command hook's own payload (the decision).
pub fn pretool_hook(sid: &str, cwd: &str, command: &str, tool_use_id: &str, dry_run: bool) -> Value {
    let mut p = hook(
        sid,
        cwd,
        "PreToolUse",
        json!({ "tool_name": "Bash", "tool_input": { "command": command, "description": describe(command) }, "tool_use_id": tool_use_id }),
    );
    if dry_run {
        p["dry_run"] = json!(true);
        p["via"] = json!("mod");
    }
    p
}

/// The mod's final ask for a Bash call (`verdict`: what the engine's permission check found, the
/// learned substitution it makes, or that it answered the call with a fresh receipt).
pub fn verdict_hook(sid: &str, cwd: &str, command: &str, tool_use_id: &str, verdict: Value) -> Value {
    let mut p = pretool_hook(sid, cwd, command, tool_use_id, false);
    p["verdict"] = verdict;
    p["via"] = json!("mod");
    p
}

/// What the mod asks once after a session starts (`adage hook claude Standing`): the standing text
/// for its cached system-prompt section, the environment sentence for the Bash tool's description
/// and the compaction instruction. With `agent` (the mod's `agent.spawn`: the subagent's `type`,
/// `description` and `prompt`), also what the subagent is told before it starts.
pub fn standing_hook(sid: &str, cwd: &str, agent: Option<Value>) -> Value {
    let mut p = json!({ "session_id": sid, "cwd": cwd, "hook_event_name": "Standing", "via": "mod" });
    if let Some(a) = agent {
        p["agent"] = a;
    }
    p
}

/// What the mod asks right after the command hooks' `SessionStart` (`adage hook claude Press`):
/// the session's first press token, if its window is still open.
pub fn press_hook(sid: &str, cwd: &str, source: &str) -> Value {
    json!({ "session_id": sid, "cwd": cwd, "hook_event_name": "Press", "source": source, "via": "mod" })
}

/// What the mod's `skill.prompt` asks as a skill's text is expanded for the model (`adage hook
/// claude SkillPrompt`).
pub fn skill_prompt_hook(sid: &str, cwd: &str, skill: &str, text: &str) -> Value {
    json!({ "session_id": sid, "cwd": cwd, "hook_event_name": "SkillPrompt", "skill": skill, "text": text, "via": "mod" })
}

/// A subagent's tool batch: Claude Code's payload for a subagent carries its `agent_id`.
pub fn subagent_batch(sid: &str, cwd: &str, agent_id: &str, acts: &[(String, Act)]) -> Value {
    let mut p = predicted_batch(sid, cwd, acts);
    p["agent_id"] = json!(agent_id);
    p
}

/// The id the mod gives the `k`-th call of a step it yields in the model's place
/// (`plugin/claude-code/hooks/predict.ts`): `_` inside a part is written `-`, so the binary can
/// tell an `adage_` call from the model's by its prefix alone.
pub fn predicted_tool_use_id(route: &str, turn_id: &str, index: u32, k: usize) -> String {
    let part = |s: &str| s.replace('_', "-");
    format!("adage_{}_{}_{index}_{k}", part(route), part(turn_id))
}

/// The `PostToolBatch` that brings back the calls of a predicted step, with the mod's ids.
pub fn predicted_batch(sid: &str, cwd: &str, ids_and_acts: &[(String, Act)]) -> Value {
    let calls: Vec<Value> = ids_and_acts
        .iter()
        .map(|(id, act)| {
            let (name, input, _, _) = native(act, cwd);
            json!({ "tool_name": name, "tool_input": input, "tool_use_id": id, "tool_response": tool_use_result(act) })
        })
        .collect();
    hook(sid, cwd, "PostToolBatch", json!({ "tool_calls": calls }))
}

/// The id Claude Code gives a tool call. [`turn_hooks`] draws one per call, in call order, so a
/// clone of its `rng` gives the ids its payloads will carry (a `PreToolUse` sent before them).
pub fn tool_use_id(rng: &mut Rng) -> String {
    format!("toolu_01{}", rng.base62(22))
}

/// The hook calls one turn produces: a `PostToolUseFailure` per failed call, then one
/// `PostToolBatch` with the rest. Each comes with the route prefixes it should draw.
pub fn turn_hooks(turn: &Turn, sid: &str, cwd: &str, rng: &mut Rng) -> Vec<(&'static str, Value, Vec<&'static str>)> {
    let mut out = Vec::new();
    let mut batch = Vec::new();
    let mut expect = Vec::new();
    for c in &turn.calls {
        let (name, input, content, is_error) = native(&c.act, cwd);
        let id = tool_use_id(rng);
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
