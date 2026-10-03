//! `PreToolUse` (Claude Code's Bash calls): the command hook's path here, the interpreter
//! planner in `interpreter`, and the mod's two phases in `phases`. Entry: [`pretool`], from
//! `handle`. Every path that decides a call marks it decided ([`crate::identity`]), so a call is
//! decided once however many times it is delivered.

mod interpreter;
mod phases;

pub use interpreter::rewrite_off;
pub(crate) use interpreter::u11_in_service;

use crate::extension::guarded;
use crate::failopen::Failure;
use crate::harness::{self, Output};
use crate::{identity, phase, record, site, Extension, PreCall, PreNote};
use adage_core::repo;
use adage_runtime::{Arm, SessionState};
use adage_store::{state, Paths};
use adage_tracks::Index;
use interpreter::{claude_allow_rules, interpreter_plan};
use serde_json::{json, Value};
use std::path::Path;
use std::time::Instant;

/// A Bash call about to run, as `pretool` and its helpers see it.
struct BashCall<'a> {
    adapter: &'a dyn harness::Adapter,
    p: &'a Value,
    input: &'a Value,
    command: &'a str,
    session: &'a str,
    cwd: &'a Path,
    root: &'a Path,
    paths: &'a Paths,
    extension: Option<&'static dyn Extension>,
    /// When the hook began: every record carries the time it took (`duration_us`).
    started: Instant,
    /// The key the decided list holds the call by ([`identity::decided_key`]), when the payload
    /// carries a usable `tool_use_id`: one decision per id.
    identity: Option<u64>,
}

/// The cheap no-match test: does the command name a Python program at all (words, not substrings:
/// `pipeline` is not `pip`)? Nothing is read before it.
fn names_python(command: &str) -> bool {
    let python_word = |w: &str| matches!(w, "pip" | "pip3" | "pytest" | "py.test") || w.starts_with("python");
    command.split(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '_')).any(python_word)
}

/// The session's arm is the control arm (read only; a session with no arm yet is not).
fn in_control(paths: &Paths, session: &str) -> bool {
    state::try_read::<SessionState>(&paths.state(session)).ok().flatten().is_some_and(|s| s.arm == Some(Arm::Control))
}

/// The call was decided already this session (by the mod, or by an earlier delivery of the same
/// event): one state read.
fn already_decided(call: &BashCall<'_>) -> bool {
    call.identity.is_some_and(|id| {
        state::try_read::<SessionState>(&call.paths.state(call.session))
            .ok()
            .flatten()
            .is_some_and(|s| s.has_seen_pretool(id))
    })
}

/// A receipt covering this call, from the extension (adage-exec), whatever the interpreter advice
/// decides: fresh or not ([`PreNote::fresh`]). Only treated sessions hold receipts; like every
/// other extension call, a panic loses only this note.
fn receipt(call: &BashCall<'_>, key: &str) -> Option<PreNote> {
    let x = call.extension.filter(|_| !in_control(call.paths, call.session))?;
    let index = Index::try_load(&call.paths.index()).unwrap_or_default();
    // The key the index (and adage-exec's journal) names this repository by: a checkout without
    // a remote is keyed by its path until an alias resolves it to the learned key.
    let key = site::routed_key(&index, call.root, call.paths, false).unwrap_or_else(|| key.to_string());
    let c = PreCall {
        repo_key: &key,
        root: call.root,
        cwd: call.cwd,
        session: call.session,
        paths: call.paths,
        index: &index,
        command: call.command,
    };
    guarded(|| x.pretool_context(&c))
}

/// The note a command hook delivers beside a call: a fresh receipt's block, and nothing else (a
/// receipt that is not fresh says nothing to the model, as before the mod).
fn fresh_note(note: Option<PreNote>) -> Option<String> {
    note.filter(|n| n.fresh && !n.text.is_empty()).map(|n| n.text)
}

/// Append one `pretool` record, naming the call's harness id (`tool_use_id`) when the payload
/// carries one: the feed line can then be drawn under that very call. The failure is returned:
/// a decision's record is part of the decision.
fn append_pretool(call: &BashCall<'_>, mut rec: Value) -> Result<(), Failure> {
    if let Some(id) = call.p["tool_use_id"].as_str().filter(|id| !id.is_empty()) {
        rec["tool_use_id"] = json!(id);
    }
    record::append(call.paths, call.session, &rec)
}

/// Append one `pretool` record with the hook's time: a lost record loses measurement, never the
/// delivery.
fn spool_pretool(call: &BashCall<'_>, mut rec: Value) {
    rec["duration_us"] = json!(record::duration_us(call.started));
    if let Err(f) = append_pretool(call, rec) {
        crate::failopen::record(&call.paths.root, call.adapter.name(), "PreToolUse", &f);
    }
}

/// A session's arm and its propensity, as a decision records them.
type Assigned = (Option<Arm>, Option<f64>);

/// Assign the session's arm if it has none (the control arm is left as it is) and mark the call
/// decided, in one state update: `None` when the call was decided already (nothing more is done).
fn claim(call: &BashCall<'_>, key: &str) -> Result<Option<Assigned>, Failure> {
    claim_if(call, key, |_| true)
}

/// [`claim`], marking the call decided only when `decides` says the session's arm makes the
/// decision: a decision withheld (the control arm, observe mode) leaves the call to whatever
/// decides it next, the command hook's own rewrite included, as it is without the mod.
fn claim_if(
    call: &BashCall<'_>,
    key: &str,
    decides: impl Fn(Option<Arm>) -> bool,
) -> Result<Option<Assigned>, Failure> {
    let arms = site::ArmInputs::load(call.paths);
    state::update::<SessionState, _>(&call.paths.state(call.session), |st| {
        if call.identity.is_some_and(|id| st.has_seen_pretool(id)) {
            return None;
        }
        arms.ensure(st, call.session, key);
        if decides(st.arm) {
            if let Some(id) = call.identity {
                st.see_pretool(id);
            }
        }
        Some((st.arm, st.propensity))
    })
    .map_err(|e| Failure::io("state", "assigning the session's arm", &e))
}

/// A receipt delivered with no rewrite or advice beside it: recorded too (with no `pretool`
/// action), so evaluation sees every delivery, and the call marked decided.
fn only_receipt(call: &BashCall<'_>, key: &str, receipt: Option<String>) -> Result<Option<String>, Failure> {
    let Some(text) = receipt else { return Ok(None) };
    if let Some(id) = call.identity {
        let fresh = state::update::<SessionState, _>(&call.paths.state(call.session), |st| st.see_pretool(id));
        if fresh.is_ok_and(|f| !f) {
            return Ok(None);
        }
    }
    let st = state::try_read::<SessionState>(&call.paths.state(call.session)).ok().flatten();
    spool_pretool(
        call,
        json!({ "ts": record::now_ms(), "event": "PreToolUse", "kind": "pretool", "arm": st.as_ref().and_then(|s| s.arm),
                "propensity": st.as_ref().and_then(|s| s.propensity), "repo": key, "receipt": true }),
    );
    Ok(call.adapter.render("PreToolUse", Output::Context(text)))
}

/// The final decision on an interpreter plan, once whether the rewrite keeps the person's
/// permission outcome is known (`pre` already says so): the arm assigned, the call marked decided,
/// one `pretool` record (`checked_by` says which check decided), and the rewrite or advice in the
/// treated arm; nothing in the control arm.
fn decide_pretool(
    call: &BashCall<'_>,
    key: &str,
    receipt: Option<String>,
    pre: adage_runtime::pretool::Pre,
    env: &adage_runtime::pretool::Env,
    checked_by: &str,
) -> Result<Option<String>, Failure> {
    use adage_runtime::pretool::{self, Pre};
    let Some((arm, propensity)) = claim(call, key)? else { return Ok(None) };
    let control = arm == Some(Arm::Control);
    // The receipt leads: a covered run is evidence already in hand, the interpreter is secondary.
    let lead = |text: String| match &receipt {
        Some(r) => format!("{r}\n{text}"),
        None => text,
    };
    let (what, out) = match &pre {
        Pre::Rewrite { command: new, kind, interpreter, .. } => {
            let mut updated = call.input.clone();
            updated["command"] = Value::String(new.clone());
            // Both notes name the interpreter word that was substituted, as it now reads in the command.
            let notice = format!(
                "adage: pointed this {} at {} (the project's environment).",
                if *kind == "test" { "test run" } else { "Python run" },
                interpreter
            );
            (
                json!({ "rewrite": kind, "interpreter": env.shown }),
                Output::Rewrite { input: updated, context: lead(pretool::rewrite_note(interpreter, kind)), notice },
            )
        }
        // Advice, never a refusal: the command runs as written, and the person's own permission
        // rules decide it (review B2).
        Pre::Advise { reason, kind } => {
            (json!({ "advise": kind, "interpreter": env.shown }), Output::Context(lead(reason.clone())))
        }
    };
    let mut rec = json!({ "ts": record::now_ms(), "event": "PreToolUse", "kind": "pretool", "arm": arm, "propensity": propensity, "repo": key, "pretool": what, "receipt": receipt.is_some(), "checked_by": checked_by });
    if control {
        rec["withheld"] = json!(true);
    }
    rec["duration_us"] = json!(record::duration_us(call.started));
    append_pretool(call, rec)?;
    if control {
        return Ok(None);
    }
    Ok(call.adapter.render("PreToolUse", out))
}

/// The planner's own reading of the person's permission rules (the settings files, the payload's
/// `permission_mode`): a rewrite they would treat differently from the command as written is
/// advice instead (review S4). The fallback where no engine verdict is at hand.
fn planner_checked(
    call: &BashCall<'_>,
    pre: adage_runtime::pretool::Pre,
    env: &adage_runtime::pretool::Env,
) -> adage_runtime::pretool::Pre {
    use adage_runtime::pretool::{self, Pre};
    if let Pre::Rewrite { command: new, kind, segments, .. } = &pre {
        let mode = call.p.get("permission_mode").and_then(Value::as_str);
        if !pretool::keeps_permissions(segments, &claude_allow_rules(call.root), mode) {
            return Pre::Advise { reason: pretool::permission_advice(env, kind, new), kind: "permissions" };
        }
    }
    pre
}

/// `PreToolUse` (Claude Code only; other harnesses keep the post-hoc directive): a Python test run
/// or scratch repro under the system Python runs with the project's verified environment instead
/// (only the interpreter prefix changes), and a `pip install` of the project or its test
/// dependencies into the system Python gets advice ([`adage_runtime::pretool`]). On by default for
/// a repository whose U11 instinct is in service and whose environment was verified when it was
/// bound; off with `adage delegate rewrites off`; never in the control arm, nor in a session
/// started in an activated environment.
///
/// **One decision per call** (the Claude Code mod's architecture §2 rule 2): a payload with a
/// usable `tool_use_id` ([`harness::Adapter::event_identity`]) is decided once per session. After
/// the word scan (so a command that names no Python program still costs no I/O) a call already
/// decided is answered with nothing and no record; every path that decides marks it decided. A
/// payload without an id is decided as it always was. The mod's own payloads ([`phase::Phase`])
/// take [`phases::pretool_mod`].
#[allow(clippy::too_many_arguments)]
pub(crate) fn pretool(
    adapter: &dyn harness::Adapter,
    p: &Value,
    session: &str,
    cwd: &Path,
    root: &Path,
    paths: &Paths,
    extension: Option<&'static dyn Extension>,
    started: Instant,
) -> Result<Option<String>, Failure> {
    if !adapter.harness().capabilities().pre_tool_rewrite || p.get("tool_name").and_then(Value::as_str) != Some("Bash")
    {
        return Ok(None);
    }
    let input = p.get("tool_input").cloned().unwrap_or(Value::Null);
    let Some(command) = input.get("command").and_then(Value::as_str) else { return Ok(None) };
    let phase = phase::Phase::of(p);
    // The cheap no-match path: no Python program named at all. A mod's dry run or verdict may
    // still plan a learned substitution for any command.
    if phase.is_none() && !names_python(command) {
        return Ok(None);
    }
    let identity = identity::decided_key(adapter, p);
    let call = BashCall { adapter, p, input: &input, command, session, cwd, root, paths, extension, started, identity };
    if let Some(phase) = phase {
        return phases::pretool_mod(&call, &phase);
    }
    if already_decided(&call) {
        return Ok(None);
    }
    let key = repo::repo_key_cached(root, &paths.repo_keys());
    let receipt = fresh_note(receipt(&call, &key));
    let Some((pre, env)) = interpreter_plan(&call, &key) else { return only_receipt(&call, &key, receipt) };
    let pre = planner_checked(&call, pre, &env);
    decide_pretool(&call, &key, receipt, pre, &env, "planner")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::*;
    use crate::{handle, Delivery, Options, Turn};
    use adage_runtime::Directive;
    use adage_store::spool;

    #[test]
    fn pretool_receipt_context_reaches_a_python_test_call_without_any_rewrite() {
        struct Receipts;
        impl Extension for Receipts {
            fn deliver(&self, _: &Turn<'_>, _: &[Directive]) -> Delivery {
                Delivery::default()
            }
            fn pretool_context(&self, call: &PreCall<'_>) -> Option<PreNote> {
                call.command.contains("pytest").then(|| PreNote {
                    text: format!("receipt for {}", call.session),
                    fresh: !call.command.contains("stale"),
                    result: Some("passed".into()),
                    covering: "pytest".into(),
                    card: None,
                    operation: String::new(),
                })
            }
        }
        static RECEIPTS: Receipts = Receipts;
        let temp = tempfile::tempdir().unwrap();
        let paths = Paths::at(temp.path().join("home"));
        let root = temp.path().join("repo");
        std::fs::create_dir_all(&root).unwrap();
        let call = |command: &str| {
            let p = json!({ "tool_name": "Bash", "tool_input": { "command": command } });
            pretool(&harness::claude::Claude, &p, "s-1", &root, &root, &paths, Some(&RECEIPTS), Instant::now()).unwrap()
        };
        let out: Value =
            serde_json::from_str(&call(".venv/bin/python -m pytest tests/test_x.py::t -q").unwrap()).unwrap();
        assert_eq!(out["hookSpecificOutput"]["additionalContext"], "receipt for s-1");
        assert!(out["hookSpecificOutput"].get("updatedInput").is_none(), "context only, never a rewrite or refusal");
        assert!(out["hookSpecificOutput"].get("permissionDecision").is_none());
        assert!(call(".venv/bin/python -c 'print(1)'").is_none());
        assert!(call("git status").is_none());
        // T-C3-02's floor: a receipt that is not fresh says nothing to the model, as before the mod.
        assert!(call(".venv/bin/python -m pytest tests/stale_test.py -q").is_none());
        // F8: a receipt delivered alone is recorded, with no rewrite or advice in the record.
        let recs: Vec<Value> = spool::read_all(&paths.spool("s-1")).unwrap();
        assert_eq!(recs.len(), 1, "{recs:?}");
        assert_eq!((recs[0]["kind"].as_str(), recs[0]["receipt"].as_bool()), (Some("pretool"), Some(true)));
        assert!(recs[0].get("pretool").is_none(), "{recs:?}");
        state::update::<SessionState, _>(&paths.state("s-1"), |st| st.arm = Some(Arm::Control)).unwrap();
        assert!(call(".venv/bin/python -m pytest tests/test_x.py -q").is_none(), "never in the control arm");
        let p = json!({ "tool_name": "Bash", "tool_input": { "command": "python -m pytest" } });
        assert!(pretool(&harness::claude::Claude, &p, "s-1", &root, &root, &paths, None, Instant::now())
            .unwrap()
            .is_none());
    }

    /// F8: `pretool_context` is wrapped like every other extension call: a panic there loses only
    /// the note, and the hook still answers.
    #[test]
    fn a_panicking_receipt_check_loses_only_the_note() {
        struct Panics;
        impl Extension for Panics {
            fn deliver(&self, _: &Turn<'_>, _: &[Directive]) -> Delivery {
                Delivery::default()
            }
            fn pretool_context(&self, _: &PreCall<'_>) -> Option<PreNote> {
                panic!("a receipt check that panics")
            }
        }
        static PANICS: Panics = Panics;
        let temp = tempfile::tempdir().unwrap();
        let paths = Paths::at(temp.path().join("home"));
        let root = temp.path().join("repo");
        std::fs::create_dir_all(&root).unwrap();
        let p = json!({ "tool_name": "Bash", "tool_input": { "command": ".venv/bin/python -m pytest tests -q" } });
        let out = pretool(&harness::claude::Claude, &p, "s-1", &root, &root, &paths, Some(&PANICS), Instant::now());
        assert!(matches!(out, Ok(None)), "{out:?}");
    }

    /// T-A1-07's L2 half: a command that names no Python program reads and writes nothing, the
    /// session state included, whatever its id.
    #[test]
    fn a_call_naming_no_python_program_touches_nothing() {
        let w = u11_world(Arm::Treated);
        let p = json!({ "session_id": "fresh", "cwd": w.root, "hook_event_name": "PreToolUse", "tool_name": "Bash",
                        "tool_use_id": "toolu_1", "tool_input": { "command": "ls -la src" } });
        let opts = Options { learner: None, extension: Some(&NO_EXTENSION) };
        assert!(handle("claude", "PreToolUse", &p.to_string(), opts, &w.paths, Instant::now()).unwrap().is_none());
        assert!(!w.paths.state("fresh").exists() && !w.paths.spool("fresh").exists());
    }
}
