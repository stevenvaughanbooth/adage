//! Harness simulator, end to end through the binary. For each harness: learn from that harness's
//! mock history (`sim/fixtures`, written by `adage-sim`), then replay a held-out session
//! (`adage_sim::scenario::replay`) through `adage hook <harness> <event>` in that harness's native
//! payloads — once in the treated arm, once in the holdout arm — and check:
//!
//! - the treated arm gets each directive at its moment, in the harness's output shape: the lookup
//!   and the known dead end at the prompt, the linked files after the lookup, the finish line
//!   after the schema edit, a request to continue at the first stop, and nothing at the second;
//! - the holdout arm gets nothing at all;
//! - `adage graph --json` then counts both sessions, split by arm.
//!
//! Also: learning from Codex history and delivering in OpenCode, learning from all three
//! histories at once, and the OpenCode plugin itself driven through a whole session in node.
//! Every run uses an isolated `ADAGE_HOME` and `HOME`.

use adage_sim::scenario::{self, Rng, Turn};
use adage_sim::{claude, codex, opencode};
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

struct Sandbox {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Sandbox {
    /// A checkout named `acme-billing` (so its repository key matches the mock history's) with the
    /// files the linked-files route may name.
    fn new() -> Sandbox {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for d in ["home", "adage", "acme-billing/.git", "acme-billing/acme", "acme-billing/schema", "acme-billing/tests"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        for f in ["acme/billing.py", "acme/customers.py", "schema/invoice.json", "schema/customer.json", "README.md", "Makefile"] {
            std::fs::write(root.join("acme-billing").join(f), scenario::body(f)).unwrap();
        }
        Sandbox { _dir: dir, root }
    }
    fn repo(&self) -> String {
        self.root.join("acme-billing").to_string_lossy().to_string()
    }
    fn command(&self, args: &[&str], arm: &str) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_adage"));
        c.args(args)
            .current_dir(&self.root)
            .env("ADAGE_HOME", self.root.join("adage"))
            .env("HOME", self.root.join("home"))
            .env("XDG_CONFIG_HOME", self.root.join("home/.config"))
            .env("CODEX_HOME", self.root.join("home/.codex"))
            .env("ADAGE_ARM", arm)
            .env_remove("ADAGE_DISABLE")
            .env_remove("CLAUDECODE");
        c
    }
    fn run(&self, args: &[&str], stdin: &[u8], arm: &str) -> (String, String, i32) {
        let mut child = self.command(args, arm).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let _ = child.stdin.take().unwrap().write_all(stdin);
        let o = child.wait_with_output().unwrap();
        (String::from_utf8_lossy(&o.stdout).to_string(), String::from_utf8_lossy(&o.stderr).to_string(), o.status.code().unwrap_or(-1))
    }
    fn learn(&self, args: &[&str]) -> Value {
        let mut a = vec!["learn", "--demo", "--json"];
        a.extend_from_slice(args);
        let (out, err, code) = self.run(&a, b"", "treated");
        assert_eq!(code, 0, "learn failed: {err}");
        serde_json::from_str(&out).unwrap()
    }
    fn learn_files(&self, files: &[PathBuf]) -> Value {
        let paths: Vec<String> = files.iter().map(|p| p.to_string_lossy().to_string()).collect();
        let mut a = vec!["--transcripts"];
        a.extend(paths.iter().map(String::as_str));
        self.learn(&a)
    }
    fn hook(&self, harness: &str, event: &str, payload: &Value, arm: &str) -> String {
        let (out, err, code) = self.run(&["hook", harness, event], payload.to_string().as_bytes(), arm);
        assert_eq!(code, 0, "hooks always exit 0");
        assert!(err.is_empty(), "hooks never write to stderr: {err}");
        out
    }
    fn graph(&self) -> Value {
        let (out, err, code) = self.run(&["graph", "--json"], b"", "treated");
        assert_eq!(code, 0, "{err}");
        serde_json::from_str(&out).unwrap()
    }
}

fn learned_routes(report: &Value) -> Vec<String> {
    let mut v: Vec<String> = report["repos"]["acme-billing"]["tracks"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|t| t["routes"].as_array().cloned().unwrap_or_default())
        .filter_map(|r| r.as_str().map(|s| s.split(' ').next().unwrap().to_string()))
        .collect();
    v.sort();
    v
}

/// The directive text in a native hook response, whatever the harness.
fn context_of(out: &str) -> String {
    let Ok(v) = serde_json::from_str::<Value>(out) else { return String::new() };
    v["hookSpecificOutput"]["additionalContext"].as_str().or_else(|| v["context"].as_str()).unwrap_or("").to_string()
}

/// Route ids in a response, in order.
fn routes_in(text: &str) -> Vec<String> {
    let re = regex::Regex::new(r#"<adage-signal [^>]*route="([^"]+)""#).unwrap();
    re.captures_iter(text).map(|c| c[1].to_string()).collect()
}

/// The response carries exactly the expected routes (by id prefix), and nothing else.
fn expect_routes(out: &str, want: &[&str], what: &str) {
    let got = routes_in(&context_of(out));
    assert_eq!(got.len(), want.len(), "{what}: got {got:?}, want {want:?}\n{out}");
    for w in want {
        assert!(got.iter().any(|g| g.starts_with(w)), "{what}: no {w} in {got:?}");
    }
    if want.is_empty() {
        assert_eq!(out, "", "{what}: silence");
    }
}

fn silent_or(treated: bool, v: &[&'static str]) -> Vec<&'static str> {
    if treated {
        v.to_vec()
    } else {
        vec![]
    }
}

// ---------------------------------------------------------------------------------------------
// Replays, one per harness, in native payloads

/// Claude Code: `PostToolBatch` per turn, `PostToolUseFailure` per failed call, Stop `block`.
fn replay_claude(sb: &Sandbox, sid: &str, arm: &str) {
    let r = scenario::replay();
    let cwd = sb.repo();
    let treated = arm == "treated";
    let mut rng = Rng::new(7);
    let h = |ev: &str, extra: Value| sb.hook("claude", ev, &claude::hook(sid, &cwd, ev, extra), arm);
    assert_eq!(h("SessionStart", json!({ "source": "startup" })), "");
    let out = h("UserPromptSubmit", json!({ "prompt": r.prompt }));
    expect_routes(&out, &silent_or(treated, &r.prompt_expect), "claude prompt");
    if treated {
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["hookSpecificOutput"]["hookEventName"], "UserPromptSubmit");
        assert!(context_of(&out).contains(&format!("python3 scripts/wi-lookup {}", r.item.id)), "the prompt's work item is bound into the command");
    }
    let turns = |turns: &[Turn], rng: &mut Rng| {
        for t in turns {
            for (ev, p, want) in claude::turn_hooks(t, sid, &cwd, rng) {
                let out = sb.hook("claude", ev, &p, arm);
                expect_routes(&out, &silent_or(treated, &want), &format!("claude {ev}"));
                if !out.is_empty() {
                    assert_eq!(serde_json::from_str::<Value>(&out).unwrap()["hookSpecificOutput"]["hookEventName"], ev);
                }
            }
        }
    };
    turns(&r.work, &mut rng);
    let stop = h("Stop", json!({ "stop_hook_active": false }));
    if treated {
        let v: Value = serde_json::from_str(&stop).unwrap();
        assert_eq!(v["decision"], "block");
        assert!(v["reason"].as_str().unwrap().starts_with("Adage finish-line check:") && v["reason"].as_str().unwrap().contains("make gen"));
    } else {
        assert_eq!(stop, "", "holdout: no stop request");
    }
    turns(&r.finish, &mut rng);
    assert_eq!(h("Stop", json!({ "stop_hook_active": treated })), "", "obligation met: nothing more");
    assert_eq!(h("SessionEnd", json!({ "reason": "other" })), "");
}

/// OpenCode: the payloads `plugin/opencode/adage.js` builds, one `tool.execute.after` per call;
/// the finish line is a `continue` at `session.idle`, whose prompt comes back via `chat.message`.
fn replay_opencode(sb: &Sandbox, sid: &str, arm: &str) {
    let r = scenario::replay();
    let cwd = sb.repo();
    let treated = arm == "treated";
    let h = |ev: &str, extra: Value| sb.hook("opencode", ev, &opencode::payload(sid, &cwd, extra), arm);
    assert_eq!(h("session.created", json!({})), "");
    let out = h("chat.message", json!({ "prompt": r.prompt }));
    expect_routes(&out, &silent_or(treated, &r.prompt_expect), "opencode prompt");
    let turns = |turns: &[Turn]| {
        for t in turns {
            for (ev, p, want) in opencode::turn_hooks(t, sid, &cwd) {
                let out = sb.hook("opencode", ev, &p, arm);
                expect_routes(&out, &silent_or(treated, &want), &format!("opencode {ev} {}", p["tool"]));
                if !out.is_empty() {
                    assert!(serde_json::from_str::<Value>(&out).unwrap()["context"].is_string());
                }
            }
        }
    };
    turns(&r.work);
    let idle = h("session.idle", json!({ "stop_hook_active": false }));
    if treated {
        let reason = serde_json::from_str::<Value>(&idle).unwrap()["continue"].as_str().unwrap().to_string();
        assert!(reason.starts_with("Adage finish-line check:"));
        assert_eq!(h("chat.message", json!({ "prompt": reason })), "", "Adage's own continuation is not a prompt");
    } else {
        assert_eq!(idle, "");
    }
    turns(&r.finish);
    assert_eq!(h("session.idle", json!({ "stop_hook_active": treated })), "");
    assert_eq!(h("session.deleted", json!({})), "");
}

/// Codex: `PostToolUse` per call with no exit code; Stop `block` becomes a new `UserPromptSubmit`.
fn replay_codex(sb: &Sandbox, sid: &str, arm: &str) {
    let r = scenario::replay();
    let cwd = sb.repo();
    let treated = arm == "treated";
    let mut rng = Rng::new(9);
    let h = |ev: &str, turn: &str, extra: Value| sb.hook("codex", ev, &codex::hook(sid, &cwd, ev, turn, extra), arm);
    assert_eq!(h("SessionStart", "", json!({ "source": "startup" })), "");
    let out = h("UserPromptSubmit", "1", json!({ "prompt": r.prompt }));
    expect_routes(&out, &silent_or(treated, &r.prompt_expect), "codex prompt");
    let turns = |turns: &[Turn], turn_id: &str, rng: &mut Rng| {
        for t in turns {
            for (ev, p, want) in codex::turn_hooks(t, sid, &cwd, turn_id, rng) {
                let out = sb.hook("codex", ev, &p, arm);
                expect_routes(&out, &silent_or(treated, &want), &format!("codex {ev} {}", p["tool_input"]));
                if !out.is_empty() {
                    assert_eq!(serde_json::from_str::<Value>(&out).unwrap()["hookSpecificOutput"]["hookEventName"], "PostToolUse");
                }
            }
        }
    };
    turns(&r.work, "1", &mut rng);
    let stop = h("Stop", "1", json!({ "stop_hook_active": false, "last_assistant_message": r.answer }));
    if treated {
        let v: Value = serde_json::from_str(&stop).unwrap();
        assert_eq!(v["decision"], "block");
        // Codex turns the reason into a new user prompt: Adage must not match prompt routes on it.
        assert_eq!(h("UserPromptSubmit", "2", json!({ "prompt": v["reason"] })), "");
    } else {
        assert_eq!(stop, "");
    }
    turns(&r.finish, "2", &mut rng);
    assert_eq!(h("Stop", "2", json!({ "stop_hook_active": treated, "last_assistant_message": r.answer })), "");
    assert_eq!(h("SessionEnd", "", json!({ "reason": "other" })), "");
}

// ---------------------------------------------------------------------------------------------
// Graph

fn route<'a>(g: &'a Value, prefix: &str) -> &'a Value {
    g["routes"].as_array().unwrap().iter().find(|r| r["id"].as_str().unwrap().starts_with(prefix)).unwrap_or_else(|| panic!("no route {prefix}"))
}

/// One treated and one holdout replay of the same session: every route applied once per arm,
/// delivered only in the treated arm.
fn check_graph(sb: &Sandbox, harness: &str) {
    let g = sb.graph();
    assert_eq!(g["sessions"], json!({ "total": 2, "treated": 1, "holdout": 1 }), "{harness}: both sessions, split by arm");
    for id in ["lookup-work-item", "read-linked-files", "finish-line-", "dead-end-"] {
        let r = route(&g, id);
        for arm in ["treated", "holdout"] {
            assert_eq!(r["arms"][arm]["sessions"], 1, "{harness} {id} {arm}");
            assert_eq!(r["arms"][arm]["applied"], 1, "{harness} {id} {arm}: applied (delivered or withheld) once");
        }
        assert_eq!(r["live"]["emitted"], 1, "{harness} {id}: delivered once, in the treated arm only");
        assert_eq!(r["live"]["derailed"], 0, "{harness} {id}");
    }
    let lookup = route(&g, "lookup-work-item");
    assert_eq!(lookup["arms"]["treated"]["followed"], 1, "{harness}");
    assert_eq!(lookup["arms"]["holdout"]["followed"], 1, "{harness}: the holdout session looked it up unaided");
    assert_eq!(lookup["live"]["followed"], 1);
    assert_eq!(lookup["live"]["holdout"]["unaided"], 1);
    let fin = route(&g, "finish-line-");
    assert_eq!((fin["arms"]["treated"]["met"].clone(), fin["arms"]["holdout"]["met"].clone()), (json!(1), json!(1)), "{harness}: make gen ran in both");
    let dead = route(&g, "dead-end-");
    assert_eq!((dead["arms"]["treated"]["hit_avoided"].clone(), dead["arms"]["holdout"]["hit_avoided"].clone()), (json!(1), json!(1)), "{harness}: both replays ran unittest");
    let stops = g["events"].as_array().unwrap().iter().filter(|e| e["continue"] == true).count();
    assert_eq!(stops, 1, "{harness}: one stop request, in the treated arm");
}

const ALL_FOUR: [&str; 4] = ["dead-end-", "finish-line-", "lookup-work-item", "read-linked-files"];

fn assert_all_four(report: &Value, harness: &str) {
    let got = learned_routes(report);
    assert_eq!(got.len(), 4, "{harness}: {got:?}");
    for (g, w) in got.iter().zip(ALL_FOUR) {
        assert!(g.starts_with(w), "{harness}: {got:?}");
    }
    assert_eq!(report["sessions_used"], 12, "{harness}");
}

/// Two sessions' ids as the harness would mint them, two seconds apart (two agents started together).
fn ids(harness: &str) -> (String, String) {
    let t = scenario::EPOCH_MS + 21 * 86_400_000 + 9 * 3_600_000;
    let mut rng = Rng::new(42);
    match harness {
        "claude" => (scenario::uuid4(&mut rng), scenario::uuid4(&mut rng)),
        "opencode" => (opencode::ident("ses", t, 1, true, &mut rng), opencode::ident("ses", t + 2_000, 1, true, &mut rng)),
        _ => (scenario::uuid7(&mut rng, t), scenario::uuid7(&mut rng, t + 2_000)),
    }
}

// ---------------------------------------------------------------------------------------------

#[test]
fn claude_code_learn_then_replay() {
    let sb = Sandbox::new();
    let projects = adage_sim::fixtures_dir().join("claude/projects");
    let rep = sb.learn(&["--claude-projects", projects.to_str().unwrap()]);
    assert_all_four(&rep, "claude");
    let (a, b) = ids("claude");
    replay_claude(&sb, &a, "treated");
    replay_claude(&sb, &b, "holdout");
    check_graph(&sb, "claude");
}

#[test]
fn opencode_learn_then_replay() {
    let sb = Sandbox::new();
    let rep = sb.learn_files(&adage_sim::fixture_files("opencode"));
    assert_all_four(&rep, "opencode");
    let (a, b) = ids("opencode");
    replay_opencode(&sb, &a, "treated");
    replay_opencode(&sb, &b, "holdout");
    check_graph(&sb, "opencode");
}

#[test]
fn codex_learn_then_replay() {
    let sb = Sandbox::new();
    let sessions = adage_sim::fixtures_dir().join("codex/sessions");
    let rep = sb.learn(&["--codex-sessions", sessions.to_str().unwrap()]);
    assert_all_four(&rep, "codex");
    let (a, b) = ids("codex");
    replay_codex(&sb, &a, "treated");
    replay_codex(&sb, &b, "holdout");
    check_graph(&sb, "codex");
}

/// Dead ends learned from Codex's rollouts (which record exit codes) are delivered in another
/// harness — and in Codex itself, at the first prompt (above), without any failure signal.
#[test]
fn learn_from_codex_history_deliver_in_opencode() {
    let sb = Sandbox::new();
    let rep = sb.learn_files(&adage_sim::fixture_files("codex"));
    assert_all_four(&rep, "codex");
    let (a, b) = ids("opencode");
    replay_opencode(&sb, &a, "treated");
    replay_opencode(&sb, &b, "holdout");
    check_graph(&sb, "codex → opencode");
}

/// All three histories at once: one repository, the same four routes, evidence from every harness.
#[test]
fn learn_from_all_three_histories_together() {
    let sb = Sandbox::new();
    let mut files = adage_sim::fixture_files("claude");
    files.extend(adage_sim::fixture_files("opencode"));
    files.extend(adage_sim::fixture_files("codex"));
    let rep = sb.learn_files(&files);
    assert_eq!((rep["sessions_read"].clone(), rep["sessions_used"].clone()), (json!(36), json!(36)));
    assert_eq!(rep["repos"].as_object().unwrap().len(), 1);
    let got = learned_routes(&rep);
    assert_eq!(got.len(), 4, "{got:?}");
    let g = sb.graph();
    let lookup = route(&g, "lookup-work-item");
    // Nine sessions per harness look the work item up; ten per harness name one.
    assert_eq!((lookup["evidence"]["support"].clone(), lookup["evidence"]["opportunities"].clone()), (json!(27), json!(30)));
    assert!(route(&g, "dead-end-")["evidence"]["support"].as_u64().unwrap() >= 12);
    // Replaying in each harness against the combined index draws the same directives.
    let (a, _) = ids("claude");
    replay_claude(&sb, &a, "treated");
    let (a, _) = ids("opencode");
    replay_opencode(&sb, &a, "treated");
    let (a, _) = ids("codex");
    replay_codex(&sb, &a, "treated");
    assert_eq!(sb.graph()["sessions"], json!({ "total": 3, "treated": 3, "holdout": 0 }));
}

/// The OpenCode plugin (plugin/opencode/adage.js) driven through the whole held-out session by
/// plugin/opencode/test/session.test.mjs with a fake OpenCode client, against the real binary,
/// in both arms. Skipped without node.
#[test]
fn opencode_plugin_full_session_in_node() {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("node not found: skipping the OpenCode plugin session test");
        return;
    }
    let sb = Sandbox::new();
    assert_all_four(&sb.learn_files(&adage_sim::fixture_files("opencode")), "opencode");
    let (src, _, code) = sb.run(&["install", "opencode", "--print", "--bin", env!("CARGO_BIN_EXE_adage")], b"", "treated");
    assert_eq!(code, 0);
    let plugin = sb.root.join("adage-plugin.mjs");
    std::fs::write(&plugin, src).unwrap();
    let r = scenario::replay();
    let (a, b) = ids("opencode");
    for (sid, arm) in [(a, "treated"), (b, "holdout")] {
        let script = sb.root.join(format!("script-{arm}.json"));
        std::fs::write(&script, opencode::plugin_script(&r, &sid, arm == "treated").to_string()).unwrap();
        let driver = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugin/opencode/test/session.test.mjs");
        let o = Command::new("node")
            .arg(driver)
            .env("PLUGIN", &plugin)
            .env("SCRIPT", &script)
            .env("REPO", sb.repo())
            .env("ADAGE_HOME", sb.root.join("adage"))
            .env("HOME", sb.root.join("home"))
            .env("ADAGE_ARM", arm)
            .env_remove("ADAGE_DISABLE")
            .output()
            .unwrap();
        let text = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
        assert!(o.status.success() && text.contains("session ok"), "{arm}: {text}");
    }
    check_graph(&sb, "opencode plugin");
}

/// OpenCode session ids begin with 12 hex digits of the (inverted) creation time and Codex thread
/// ids are UUIDv7s, so sessions started close together share a long prefix. `adage graph` must
/// still count them as different sessions.
#[test]
fn graph_counts_sessions_whose_ids_share_a_prefix() {
    let sb = Sandbox::new();
    sb.learn_files(&adage_sim::fixture_files("opencode"));
    let (a, b) = ids("opencode");
    assert_eq!(a[..8], b[..8], "the ids share a prefix: {a} {b}");
    replay_opencode(&sb, &a, "treated");
    replay_opencode(&sb, &b, "treated");
    let (c, d) = ids("codex");
    assert_eq!(c[..8], d[..8], "{c} {d}");
    replay_codex(&sb, &c, "treated");
    replay_codex(&sb, &d, "treated");
    let g = sb.graph();
    assert_eq!(g["sessions"], json!({ "total": 4, "treated": 4, "holdout": 0 }));
    let lookup = route(&g, "lookup-work-item");
    assert_eq!(lookup["arms"]["treated"]["sessions"], 4);
    assert_eq!(lookup["arms"]["treated"]["applied"], 4);
    assert_eq!(lookup["arms"]["treated"]["followed"], 4);
    assert_eq!(lookup["live"]["sessions"], 4);
    let sessions: std::collections::BTreeSet<&str> = g["events"].as_array().unwrap().iter().filter_map(|e| e["session"].as_str()).collect();
    assert_eq!(sessions.len(), 4, "events name four sessions: {sessions:?}");
}
