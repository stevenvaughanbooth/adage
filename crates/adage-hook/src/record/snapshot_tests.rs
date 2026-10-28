//! Every kind of record the hook writes to a session's spool, as the spool holds it: the lines
//! themselves (so the key order is pinned too), with only what differs between runs redacted (the
//! wall-clock `ts`, the hook's `duration_us`, the test binary's name, the scratch directory and a
//! repository key derived from it). The record seam ([`super::Record`]) must write these bytes.

use crate::testkit::*;
use crate::{handle, press, skill, Delivery, Extension, Options, StopGate, Turn};
use adage_core::repo;
use adage_runtime::{Arm, Directive, SessionState};
use adage_store::{state, Paths};
use serde_json::{json, Value};
use std::path::Path;
use std::time::Instant;

/// The payload of every frame of a spool (`magic | len | crc | payload`, `adage_store::spool`):
/// the record's bytes exactly as they were serialized.
fn payloads(path: &Path) -> Vec<String> {
    let bytes = std::fs::read(path).unwrap_or_default();
    let mut out = Vec::new();
    let mut at = 0;
    while at + 12 <= bytes.len() {
        let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        out.push(String::from_utf8(bytes[at + 12..at + 12 + len].to_vec()).unwrap());
        at += 12 + len;
    }
    assert_eq!(at, bytes.len(), "{} ends in a whole frame", path.display());
    out
}

/// The spool of `session`, one record per line, redacted.
fn lines(paths: &Paths, session: &str, root: &Path) -> Vec<String> {
    let key = repo::repo_key_cached(root, &paths.repo_keys());
    let dir = root.parent().unwrap().to_string_lossy().into_owned();
    payloads(&paths.spool(session))
        .into_iter()
        .map(|l| {
            let mut out = String::new();
            let mut rest = l.replace(&key, "[key]").replace(&dir, "[dir]");
            for field in ["\"ts\":", "\"duration_us\":"] {
                while let Some(i) = rest.find(field) {
                    let start = i + field.len();
                    let end = start + rest[start..].find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len() - start);
                    assert!(end > start, "{field} is an integer: {l}");
                    out.push_str(&rest[..start]);
                    out.push_str("[n]");
                    rest = rest[end..].to_string();
                }
                rest = std::mem::take(&mut out) + &rest;
            }
            match rest.find("\"bin\":\"") {
                Some(i) => {
                    let start = i + "\"bin\":\"".len();
                    let end = start + rest[start..].find('"').unwrap();
                    format!("{}[bin]{}", &rest[..start], &rest[end..])
                }
                None => rest,
            }
        })
        .collect()
}

/// One section of the snapshot.
fn section(out: &mut String, title: &str, lines: Vec<String>) {
    assert!(!lines.is_empty(), "{title}: no record");
    out.push_str(&format!("## {title}\n"));
    for l in lines {
        out.push_str(&l);
        out.push('\n');
    }
    out.push('\n');
}

fn event(w: &ObligationWorld, harness: &str, ext: &'static dyn Extension, session: &str, name: &str, extra: Value) {
    let mut p = json!({ "session_id": session, "cwd": w.root });
    for (k, v) in extra.as_object().unwrap() {
        p[k] = v.clone();
    }
    let opts = Options { learner: None, extension: Some(ext) };
    handle(harness, name, &p.to_string(), opts, &w.paths, Instant::now()).unwrap();
}

/// A stand-in for adage-exec whose delivery records an `exec` entry and asks for recovery.
struct Recovers;
impl Extension for Recovers {
    fn deliver(&self, _: &Turn<'_>, _: &[Directive]) -> Delivery {
        Delivery {
            spool: Some(json!([{ "route": "regenerate-registry", "level": "E2", "withheld": "no sandbox" }])),
            recovery: Some("Adage cannot confirm an execution outcome.".into()),
            ..Delivery::default()
        }
    }
}
static RECOVERS: Recovers = Recovers;

/// A stand-in for adage-exec with a person's `predict` grant on the finish line's route.
struct Granted;
impl Extension for Granted {
    fn deliver(&self, _: &Turn<'_>, _: &[Directive]) -> Delivery {
        Delivery::default()
    }
    fn predict_grants(&self, _: &crate::Site<'_>) -> Vec<(String, String)> {
        vec![("acme/finish-lines".into(), "regenerate-registry".into())]
    }
}
static GRANTED: Granted = Granted;

/// A skill `adage train` compiled and replayed in `root`'s repository, accepted by a person.
fn accepted_skill(paths: &Paths, root: &Path) {
    use adage_pack::verified::{self, Outcome, Skill, Step, Verified};
    let text = "Run `make test`.\n";
    let file = root.join(".claude/skills/release-checklist/SKILL.md");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, text).unwrap();
    let step = |command: &str, result: Outcome| Step {
        id: "s1".into(),
        route: String::new(),
        command: command.into(),
        result,
        signature: None,
        date: "2026-10-03".into(),
        sandbox: "test".into(),
    };
    let v = Verified {
        schema: verified::SCHEMA.into(),
        skill: Skill {
            name: "release-checklist".into(),
            sha256: adage_pack::digest::sha256_hex(text.as_bytes()),
            inert: true,
            path: file.to_string_lossy().into_owned(),
        },
        repo: repo::repo_key(root),
        pack: "acme/skill-release-checklist".into(),
        digest: "b".repeat(64),
        steps: vec![step("make test", Outcome::Works), step("make lint", Outcome::Stale)],
        accepted_at: Some("2026-10-03".into()),
    };
    let dir = skill::skills_dir(paths).join("release-checklist");
    std::fs::create_dir_all(&dir).unwrap();
    verified::write(&dir, &v).unwrap();
}

/// T-A2-01's records, all of them: what each kind writes, byte for byte.
#[test]
fn every_record_kind_as_the_spool_holds_it() {
    let mut out = String::new();

    // A session's decided events in a repository with a finish line, and its Stop gate. Measuring
    // is on (it is off by default), so the spool holds both arms' records and their propensity.
    let w = ObligationWorld::new();
    std::fs::write(w.paths.config(), "[learn]\nauto = false\n\n[runtime]\ncontrol = 0.1\n").unwrap();
    let gate = scripted(vec![StopGate::Block("Adage finish-line check: `make test` failed.".into())]);
    event(&w, "claude", gate, "s", "SessionStart", json!({ "source": "startup" }));
    w.edit(gate);
    event(&w, "claude", gate, "s", "Stop", json!({ "stop_hook_active": false }));
    section(&mut out, "session_start, batch, stop, stop_gate (Claude Code)", lines(&w.paths, "s", &w.root));

    // The same from Codex: the records name their harness.
    let edit = json!({ "tool_name": "Edit", "tool_use_id": "c1", "tool_response": "ok",
                       "tool_input": { "file_path": w.root.join("registry/currencies.csv") } });
    event(&w, "codex", &NO_EXTENSION, "cx", "SessionStart", json!({}));
    event(&w, "codex", &NO_EXTENSION, "cx", "PostToolUse", edit);
    section(&mut out, "session_start, batch (Codex)", lines(&w.paths, "cx", &w.root));

    // An extension's `exec` entry and a recovery.
    event(
        &w,
        "claude",
        &RECOVERS,
        "x",
        "PostToolBatch",
        json!({ "tool_calls": [{ "tool_name": "Edit", "tool_use_id": "e1",
        "tool_response": "ok", "tool_input": { "file_path": w.root.join("registry/currencies.csv") } }] }),
    );
    section(&mut out, "batch with exec and recovery_required", lines(&w.paths, "x", &w.root));

    // A prompt decided where a dead end is learned.
    let d = dead_end_world(Arm::Treated);
    event(&d, "claude", &NO_EXTENSION, "s", "UserPromptSubmit", json!({ "prompt": "fix the invoice rounding" }));
    section(&mut out, "prompt", lines(&d.paths, "s", &d.root));

    // A step routed to the routine model.
    let r = ObligationWorld::new();
    state::update::<SessionState, _>(&r.paths.state("s"), |st| {
        st.pending = vec![adage_runtime::Pending {
            route: "regenerate-registry".into(),
            expect_key: adage_runtime::command_key("make gen", adage_core::CommandTable::load()),
            expect_head: String::new(),
            batches_left: 3,
            command: "make gen".into(),
        }];
    })
    .unwrap();
    event(
        &r,
        "claude",
        &NO_EXTENSION,
        "s",
        "TurnStep",
        json!({ "turn_id": "t1", "index": 1, "routine_model": "small" }),
    );
    section(&mut out, "turn_step", lines(&r.paths, "s", &r.root));

    // A predicted step the person's permissions held back (the mod's verdict).
    let h = ObligationWorld::new();
    state::update::<SessionState, _>(&h.paths.state("s"), |st| {
        st.pending = vec![adage_runtime::Pending {
            route: "regenerate-registry".into(),
            expect_key: adage_runtime::command_key("make gen", adage_core::CommandTable::load()),
            expect_head: String::new(),
            batches_left: 3,
            command: "make gen".into(),
        }];
    })
    .unwrap();
    let held = json!({ "turn_id": "t1", "index": 1, "via": "mod", "verdict": { "permission": "ask",
                       "route": "regenerate-registry", "tool": "Bash", "reason": "This command requires approval" } });
    event(&h, "claude", &GRANTED, "s", "TurnStep", held);
    section(&mut out, "turn_step: withheld", lines(&h.paths, "s", &h.root));

    // Sessions seen in a repository with no reflexes yet, from Claude Code and Codex.
    let plain = tempfile::tempdir().unwrap();
    let proot = plain.path().canonicalize().unwrap().join("plain");
    std::fs::create_dir_all(proot.join(".git")).unwrap();
    for (harness, session) in [("claude", "u"), ("codex", "uc")] {
        let p = |extra: Value| {
            let mut p = json!({ "session_id": session, "cwd": proot });
            for (k, v) in extra.as_object().unwrap() {
                p[k] = v.clone();
            }
            p.to_string()
        };
        let opts = Options { learner: None, extension: None };
        let start = if harness == "claude" { json!({ "source": "startup" }) } else { json!({}) };
        handle(harness, "SessionStart", &p(start), opts, &w.paths, Instant::now()).unwrap();
        handle(harness, "UserPromptSubmit", &p(json!({ "prompt": "hello" })), opts, &w.paths, Instant::now()).unwrap();
        let calls = json!({ "tool_name": "Bash", "tool_input": { "command": "ls" }, "tool_response": "a" });
        let (name, extra) = match harness {
            "claude" => ("PostToolBatch", json!({ "tool_calls": [calls.clone(), calls] })),
            _ => ("PostToolUse", calls),
        };
        handle(harness, name, &p(extra), opts, &w.paths, Instant::now()).unwrap();
        let failed = json!({ "tool_name": "Bash", "tool_input": { "command": "ls" }, "error": "x" });
        handle(harness, "PostToolUseFailure", &p(failed), opts, &w.paths, Instant::now()).unwrap();
        handle(harness, "SessionEnd", &p(json!({})), opts, &w.paths, Instant::now()).unwrap();
        section(&mut out, &format!("unrouted ({harness})"), lines(&w.paths, session, &proot));
    }

    // A failure an after-failure route is decided on, then `pretool`: the interpreter's rewrite and
    // advice, the control arm, a receipt alone.
    let u = u11_world(Arm::Treated);
    let failed = json!({ "tool_name": "Bash", "tool_use_id": "f1", "tool_input": { "command": "python -m pytest" },
                         "error": "No module named pytest" });
    u.event(&NO_EXTENSION, "PostToolUseFailure", failed);
    bash_call(&u, &NO_EXTENSION, "", PYTEST, json!({}));
    bash_call(&u, &NO_EXTENSION, "toolu_1", PYTEST, json!({ "verdict": { "engine": "same" } }));
    bash_call(&u, &NO_EXTENSION, "toolu_2", PYTEST, json!({ "verdict": { "engine": "differs" } }));
    section(
        &mut out,
        "failure, then pretool: rewrite, rewrite checked by the engine, advice",
        lines(&u.paths, "s", &u.root),
    );
    let c = u11_world(Arm::Control);
    bash_call(&c, &NO_EXTENSION, "toolu_1", PYTEST, json!({}));
    section(&mut out, "pretool: withheld", lines(&c.paths, "s", &c.root));
    let o = ObligationWorld::new();
    bash_call(&o, &FRESH_RECEIPTS, "toolu_1", PYTEST, json!({}));
    section(&mut out, "pretool: a receipt alone", lines(&o.paths, "s", &o.root));

    // `pretool`: a dead end substituted, and withheld in the control arm.
    let sub = json!({ "verdict": { "engine": "same", "substitute": "dead-end-pytest" } });
    for arm in [Arm::Treated, Arm::Control] {
        let d = dead_end_world(arm);
        bash_call(&d, &NO_EXTENSION, "toolu_1", "python3 -m pytest tests", sub.clone());
        section(&mut out, &format!("pretool: substitution ({arm:?})"), lines(&d.paths, "s", &d.root));
    }

    // `pretool`: a call answered by a fresh receipt.
    let rw = receipt_world(Arm::Treated);
    let edit = json!({ "tool_name": "Edit", "tool_use_id": "e1", "tool_response": "ok",
                       "tool_input": { "file_path": rw.root.join("tests/test_a.py") } });
    rw.event(&FRESH_RECEIPTS, "PostToolBatch", json!({ "tool_calls": [edit] }));
    bash_call(&rw, &FRESH_RECEIPTS, "toolu_1", PYTEST, json!({ "verdict": { "answered": "receipt" } }));
    section(&mut out, "batch, then pretool: answered by a receipt", lines(&rw.paths, "s", &rw.root));

    // `skill_prompt`: annotated, and in observe mode.
    let k = ObligationWorld::new();
    accepted_skill(&k.paths, &k.root);
    let ask = json!({ "skill": "release-checklist", "text": "Run `make test`." });
    event(&k, "claude", &NO_EXTENSION, "s", "SkillPrompt", ask.clone());
    let mut observe = ask;
    observe["observe"] = json!(true);
    event(&k, "claude", &NO_EXTENSION, "s", "SkillPrompt", observe);
    section(&mut out, "skill_prompt", lines(&k.paths, "s", &k.root));

    // The press: a token issued, spent, and the tripwire.
    let pw = ObligationWorld::new();
    let transcript = json!({ "source": "startup", "transcript_path": "/home/dev/.claude/projects/p/p.jsonl" });
    state::update::<SessionState, _>(&pw.paths.state("p"), |st| st.arm = Some(Arm::Treated)).unwrap();
    event(&pw, "claude", &NO_EXTENSION, "p", "SessionStart", transcript.clone());
    let mut p = json!({ "session_id": "p", "cwd": pw.root });
    p["source"] = json!("startup");
    let opts = Options { learner: None, extension: Some(&NO_EXTENSION) };
    let token = handle("claude", press::EVENT, &p.to_string(), opts, &pw.paths, Instant::now()).unwrap().unwrap();
    let token: Value = serde_json::from_str(&token).unwrap();
    press::spend(&pw.paths, token["press"].as_str().unwrap(), "p", &press::root_of(&pw.root), "review approve")
        .unwrap();
    bash_call_in(&pw, "p", "adage hook claude Press < p.json");
    section(&mut out, "session_start, press_issued, press_spent, tripwire", lines(&pw.paths, "p", &pw.root));

    // A subagent's calls (Claude Code's `agent_id`): its failure, the batch that lists the call
    // again, and its Bash call's `pretool` name it (`agent`).
    let sa = u11_world(Arm::Treated);
    let agent = json!({ "agent_id": "af2d43c8c64650e89", "agent_type": "Explore" });
    let call =
        json!({ "tool_name": "Bash", "tool_use_id": "toolu_s1", "tool_input": { "command": "python -m pytest" } });
    let mut failed = agent.clone();
    for (k, v) in call.as_object().unwrap() {
        failed[k] = v.clone();
    }
    failed["error"] = json!("No module named pytest");
    sa.event(&NO_EXTENSION, "PostToolUseFailure", failed);
    let mut listed = call;
    listed["tool_response"] = json!("No module named pytest");
    let mut batch = agent.clone();
    batch["tool_calls"] = json!([listed]);
    sa.event(&NO_EXTENSION, "PostToolBatch", batch);
    let mut verdict = agent;
    verdict["verdict"] = json!({ "engine": "same" });
    bash_call(&sa, &NO_EXTENSION, "toolu_s2", PYTEST, verdict);
    section(&mut out, "failure, batch, pretool: a subagent's calls", lines(&sa.paths, "s", &sa.root));

    // The world: a hint at a prompt (`injection`), its label at the batch that opens the pull
    // request it named (`injection_label`), and the hint withheld in the control arm.
    let ww = world_world(Arm::Treated);
    event(&ww, "claude", &NO_EXTENSION, "s", "UserPromptSubmit", json!({ "prompt": "review the PR" }));
    let open = json!({ "tool_calls": [{ "tool_name": "Bash", "tool_use_id": "w1", "tool_response": "",
                       "tool_input": { "command": "gh pr view #412" } }] });
    event(&ww, "claude", &NO_EXTENSION, "s", "PostToolBatch", open);
    section(&mut out, "injection, injection_label", world_lines(&ww.paths, "s", &ww.root));
    let wc = world_world(Arm::Control);
    event(&wc, "claude", &NO_EXTENSION, "s", "UserPromptSubmit", json!({ "prompt": "review the PR" }));
    section(&mut out, "injection: withheld", world_lines(&wc.paths, "s", &wc.root));

    insta::assert_snapshot!("every_record_kind", out);
}

/// A session in `arm` with the world on and one open pull request in its store.
fn world_world(arm: Arm) -> ObligationWorld {
    let w = dead_end_world(arm);
    std::fs::write(w.paths.config(), "[learn]\nauto = false\n\n[world]\nenabled = true\n").unwrap();
    state::update::<adage_world::ArtifactsState, _>(&w.paths.world_artifacts(), |s| {
        s.upsert(adage_trace::Artifact {
            id: "github:pr:412".into(),
            kind: adage_trace::ArtifactKind::Pr,
            title: "Schema evolution for invoices".into(),
            locator: "#412".into(),
            summary: None,
            embedding: None,
            first_seen: 1_792_000_000_000,
            last_touched: 1_792_000_000_000,
            related: vec![],
            state: Some("ci:green".into()),
            people: vec![],
        });
    })
    .unwrap();
    w
}

/// The world's records alone, with the fields that depend on the clock or the model made stable:
/// the hint's id and confidence, the scores, the recency-dependent text.
fn world_lines(paths: &Paths, session: &str, root: &Path) -> Vec<String> {
    lines(paths, session, root)
        .into_iter()
        .filter(|l| l.contains("\"kind\":\"injection\"") || l.contains("\"kind\":\"injection_label\""))
        .map(|l| {
            let mut l = l;
            for (field, stand_in) in
                [("\"id\":\"w", "\"id\":\"[id]"), ("\"injection_id\":\"w", "\"injection_id\":\"[id]")]
            {
                while let Some(i) = l.find(field) {
                    let end = i + field.len() + l[i + field.len()..].find('"').unwrap();
                    l.replace_range(i..end, stand_in);
                }
            }
            for field in ["\"confidence\":", "\"score\":", "\"sim\":", "\"recency\":", "\"entity\":", "\"kind\":"] {
                let mut from = 0;
                while let Some(i) = l[from..].find(field).map(|i| i + from) {
                    let start = i + field.len();
                    let end = start
                        + l[start..]
                            .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == 'e' || c == '-'))
                            .unwrap_or(l.len() - start);
                    if end > start {
                        l.replace_range(start..end, "[n]");
                        from = start + "[n]".len();
                    } else {
                        from = start;
                    }
                }
            }
            l
        })
        .collect()
}

/// A command hook's Bash call in `session`.
fn bash_call_in(w: &ObligationWorld, session: &str, command: &str) {
    let p = json!({ "tool_name": "Bash", "tool_use_id": "toolu_p", "tool_input": { "command": command } });
    event(w, "claude", &NO_EXTENSION, session, "PreToolUse", p);
}
