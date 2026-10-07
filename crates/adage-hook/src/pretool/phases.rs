//! The Claude Code mod's two phases of a Bash call ([`phase::Phase`]): the dry run's plan and the
//! verdict's one decision, a call answered by a receipt included. Entry: [`pretool_mod`], from
//! `pretool`.

use super::{already_decided, claim_if, decide_pretool, fresh_note, only_receipt, planner_checked, receipt};
use super::{append_pretool, interpreter_plan, names_python, pretool_record, BashCall};
use crate::answer::{answer, ModAnswer, Plan, PlanAdvice, PlanReceipt, PlanRewrite, ReceiptAnswer};
use crate::failopen::Failure;
use crate::site::{self, observed};
use crate::{identity, phase, PreNote};
use adage_core::CommandTable;
use adage_runtime::{Arm, Ctx, SessionState};
use adage_store::state;
use adage_tracks::Index;

/// The mod's two phases of a Bash call ([`phase::Phase`]; architecture §3.3, §3.4 of the Claude
/// Code mod).
///
/// - **Dry run** answers `{"plan": {…}}`, or nothing when there is nothing to plan or the call
///   was decided already, in either arm; it changes no state and spools nothing. The plan holds
///   whichever apply: `substitute: {command, route, track, context}` (a learned dead end,
///   [`adage_runtime::substitution`]), `rewrite: {command, kind}` (the interpreter rewrite, not
///   yet checked against the person's permission rules: the mod asks the engine),
///   `advice: {kind, text}` (advice instead of a rewrite), and `receipt: {fresh, covering, note?,
///   result?}` (a covering receipt, treated sessions only; `note` and `result` only when fresh).
/// - **Verdict** decides once and marks the call decided; a call decided already gets nothing.
///   With `answered: receipt` (the mod answered the call with a fresh receipt's `result`, the
///   engine having allowed the call unasked) the call is observed as made and passed
///   ([`adage_runtime::answered_by_receipt`]: the obligation it covers is met), recorded with
///   `answered: "receipt"`, and answered `{"receipt": {result, note, covering}}`; a receipt no
///   longer fresh decides nothing (the mod then runs the call).
///   With `substitute` (and `engine: same`) the dead end is replaced: `{"substitute": {…}}`
///   judged `substituted`, or `would_substitute` and nothing printed in the control arm or in
///   observe mode; any other engine answer decides nothing. Otherwise the interpreter plan is
///   decided as the command hook decides it, the engine's comparison replacing the planner's
///   reading of the settings files when present (`engine: same` keeps the rewrite, `differs`
///   makes it advice, `checked_by: engine`), and answered in the command hook's own shape (the
///   mod reads `hookSpecificOutput.updatedInput`, or carries `additionalContext` itself, since
///   the command hook's repeat of the call is answered with nothing).
pub(super) fn pretool_mod(call: &BashCall<'_>, phase: &phase::Phase) -> Result<Option<String>, Failure> {
    if already_decided(call) {
        return Ok(None);
    }
    let (p, paths, root) = (call.p, call.paths, call.root);
    // What the mod can do here: always its full set in practice; read, never assumed.
    let caps = call.adapter.capabilities("PreToolUse", p);
    let index = call.index();
    let routed = site::routed_key(index, root, paths, false);
    // A replaced command is a rewrite: off with `adage delegate rewrites off` like the others.
    let rewrites_on = call.config().runtime_rewrite && !super::rewrite_off(paths).exists();
    let table = || CommandTable::load();
    let found =
        |key: &str| adage_runtime::substitution(index, key, &adage_runtime::Policy::default(), table(), call.command);
    match phase {
        phase::Phase::DryRun => {
            let mut plan = Plan {
                substitute: routed.as_deref().filter(|_| rewrites_on && caps.substitute_call).and_then(found),
                ..Plan::default()
            };
            if names_python(call.command) {
                let key = call.repo_key();
                plan.receipt = receipt(call, key).map(|note| receipt_plan(&note, caps.answer_call));
                match interpreter_plan(call, key) {
                    Some((adage_runtime::pretool::Pre::Rewrite { command, kind, .. }, _)) => {
                        plan.rewrite = Some(PlanRewrite { command, kind: kind.into() });
                    }
                    Some((adage_runtime::pretool::Pre::Advise { reason, kind }, _)) => {
                        plan.advice = Some(PlanAdvice { kind: kind.into(), text: reason });
                    }
                    None => {}
                }
            }
            Ok((!plan.is_empty()).then(|| answer(&ModAnswer::Plan { plan })))
        }
        phase::Phase::Verdict(v) if v.answered == Some(phase::Answered::Receipt) => {
            if !names_python(call.command) || !caps.answer_call {
                return Ok(None);
            }
            let key = routed.clone().unwrap_or_else(|| call.repo_key().to_string());
            match receipt(call, &key).filter(|n| n.fresh && n.result.is_some()) {
                Some(note) => answer_with_receipt(call, &key, index, note),
                None => Ok(None),
            }
        }
        phase::Phase::Verdict(v) if v.substitute.is_some() => {
            if v.engine != Some(phase::Engine::Same) || !rewrites_on {
                return Ok(None);
            }
            let Some(key) = routed else { return Ok(None) };
            let Some(sub) = found(&key).filter(|s| v.substitute.as_deref() == Some(s.route.as_str())) else {
                return Ok(None);
            };
            // Withheld (control arm, observe mode), the call stays undecided: the mod's rewrite
            // verdict, or the command hook after it, decides it as today's product does.
            let observe = observed(p);
            let Some((arm, propensity)) = claim_if(call, &key, |arm| arm != Some(Arm::Control) && !observe)? else {
                return Ok(None);
            };
            let decision = adage_runtime::substitute(arm, observe, sub);
            let mut rec =
                pretool_record(arm, propensity, &key).with("checked_by", "engine").with("decision", &decision);
            match &decision.substituted {
                Some(_) => rec.set("substituted", true),
                None => {
                    rec.set("would_substitute", true);
                    rec.set("withheld", true);
                }
            }
            append_pretool(call, rec)?;
            Ok(decision.substituted.map(|substitute| answer(&ModAnswer::Substitute { substitute })))
        }
        phase::Phase::Verdict(v) => {
            if !names_python(call.command) {
                return Ok(None);
            }
            let key = call.repo_key();
            let receipt = fresh_note(receipt(call, key));
            let Some((pre, env)) = interpreter_plan(call, key) else { return only_receipt(call, key, receipt) };
            let pre = match (v.engine, pre) {
                (None, pre) => planner_checked(call, pre, &env),
                (Some(phase::Engine::Same), pre) => pre,
                (Some(phase::Engine::Differs), adage_runtime::pretool::Pre::Rewrite { command, kind, .. }) => {
                    adage_runtime::pretool::Pre::Advise {
                        reason: adage_runtime::pretool::permission_advice(&env, kind, &command),
                        kind: "permissions",
                    }
                }
                (Some(phase::Engine::Differs), pre) => pre,
            };
            decide_pretool(call, key, receipt, pre, &env, phase.checked_by())
        }
    }
}

/// A covering receipt in the mod's dry-run plan: `fresh` and the command that ran, always; the
/// note and the result the call would be answered with only when fresh (and the result only where
/// a call can be answered).
fn receipt_plan(note: &PreNote, answers: bool) -> PlanReceipt {
    PlanReceipt {
        fresh: note.fresh,
        covering: note.covering.clone(),
        operation: Some(note.operation.clone()).filter(|o| !o.is_empty()),
        note: note.fresh.then(|| note.text.clone()),
        result: note.result.clone().filter(|_| note.fresh && answers),
    }
}

/// The mod answered a Bash call with a fresh receipt's result instead of running it: in one state
/// update the call is marked decided, the session's arm assigned if it has none, and the call
/// observed as made and passed ([`adage_runtime::answered_by_receipt`]); one `pretool` record with
/// `answered: "receipt"` and the decision (its `obligation_met` judgements are what a rebuilt
/// state reads). Treated sessions only: a session found in the control arm records the answer
/// withheld and gets nothing (the call then runs).
fn answer_with_receipt(
    call: &BashCall<'_>,
    key: &str,
    index: &Index,
    note: PreNote,
) -> Result<Option<String>, Failure> {
    let table = CommandTable::load();
    let ctx = Ctx {
        checkout: &adage_runtime::checkout::Disk,
        index,
        repo: key.to_string(),
        root: call.root,
        table,
        embed: None,
        params: Default::default(),
        policy: adage_runtime::Policy::default(),
    };
    let arms = call.arms();
    let decided = state::update::<SessionState, _>(&call.paths.state(call.session), |st| {
        if call.identity.is_some_and(|id| !st.see_pretool(id)) {
            return None;
        }
        arms.ensure(st, call.session, key);
        let d = (st.arm != Some(Arm::Control))
            .then(|| adage_runtime::answered_by_receipt(&ctx, st, call.command, &note.covering));
        // The batch that lists this call names it by its bare `tool_use_id`: the answered list's
        // key, not the decided list's ([`crate::identity`]).
        if let (Some(id), Some(_)) = (identity::answered_key(call.p), &d) {
            st.see_answered(id);
        }
        Some((st.arm, st.propensity, d))
    })
    .map_err(|e| Failure::io("state", "recording a call answered by a receipt", &e))?;
    let Some((arm, propensity, decision)) = decided else { return Ok(None) };
    let mut rec = pretool_record(arm, propensity, key)
        .with("answered", "receipt")
        .with("receipt", true)
        .with("checked_by", "engine")
        .with("command", &note.covering);
    if let Some(card) = &note.card {
        rec.set("card", card);
    }
    match &decision {
        Some(d) => rec.set("decision", d),
        None => rec.set("withheld", true),
    }
    append_pretool(call, rec)?;
    Ok(decision.map(|_| {
        let receipt = ReceiptAnswer { result: note.result, note: note.text, covering: note.covering, card: note.card };
        answer(&ModAnswer::Receipt { receipt })
    }))
}

#[cfg(test)]
mod tests {
    use crate::testkit::*;
    use crate::{harness, phase, Delivery, Extension, PreCall, PreNote, Turn};
    use adage_core::CommandTable;
    use adage_runtime::{rebuild, Arm, Directive, SessionState};
    use adage_store::state;
    use adage_tracks::Index;
    use serde_json::{json, Value};

    fn pre(w: &ObligationWorld, command: &str, extra: Value) -> Option<Value> {
        let mut p = json!({ "hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_use_id": "toolu_9",
                            "tool_input": { "command": command } });
        for (k, v) in extra.as_object().unwrap() {
            p[k] = v.clone();
        }
        w.event(&NO_EXTENSION, "PreToolUse", p)
    }

    /// T-C6-01 / T-C6-02, in process: the dry run answers the plan and records nothing; the
    /// verdict substitutes once, judged `substituted`; a command hook's payload is untouched.
    #[test]
    fn a_dead_end_is_planned_then_substituted_on_the_same_verdict() {
        let w = dead_end_world(Arm::Treated);
        let sub = json!({ "command": "make test", "route": "dead-end-pytest", "track": "acme/dead-ends",
            "context": "adage ran make test for you; python3 -m pytest tests was replaced because that command fails in this repository (\"No module named pytest\"). The output above is from make test." });
        assert_eq!(
            pre(&w, "python3 -m pytest tests", json!({ "dry_run": true })),
            Some(json!({ "plan": { "substitute": sub } }))
        );
        assert!(pre(&w, "make test", json!({ "dry_run": true })).is_none(), "no dead end: no plan");
        assert!(spooled(&w).is_empty(), "a dry run records nothing");
        assert!(pre(&w, "python3 -m pytest tests", json!({})).is_none(), "the command hook: as before");
        assert!(spooled(&w).is_empty());
        let differs = json!({ "verdict": { "engine": "differs", "substitute": "dead-end-pytest" } });
        assert!(pre(&w, "python3 -m pytest tests", differs).is_none(), "another permission verdict");
        let other = json!({ "verdict": { "engine": "same", "substitute": "another-route" } });
        assert!(pre(&w, "python3 -m pytest tests", other).is_none(), "the plan moved");
        assert!(spooled(&w).is_empty());
        let same = json!({ "verdict": { "engine": "same", "substitute": "dead-end-pytest" } });
        assert_eq!(pre(&w, "python3 -m pytest tests", same), Some(json!({ "substitute": sub })));
        let recs = spooled(&w);
        assert_eq!(recs.len(), 1);
        let rec = &recs[0];
        assert_eq!((rec["kind"].as_str(), rec["substituted"].as_bool()), (Some("pretool"), Some(true)));
        assert_eq!(rec["checked_by"], "engine");
        assert_eq!(rec["decision"]["judgements"], json!([{ "kind": "substituted", "route": "dead-end-pytest" }]));
        assert!(rec.get("withheld").is_none() && rec.get("would_substitute").is_none());
    }

    /// T-C6-02's control half, and observe mode: `would_substitute` spooled, nothing printed.
    #[test]
    fn a_dead_end_is_never_substituted_in_control_or_observe_mode() {
        let same = json!({ "verdict": { "engine": "same", "substitute": "dead-end-pytest" } });
        let observe = json!({ "verdict": { "engine": "same", "substitute": "dead-end-pytest" }, "observe": true });
        for (arm, payload) in [(Arm::Control, same), (Arm::Treated, observe)] {
            let w = dead_end_world(arm);
            assert!(
                pre(&w, "python3 -m pytest tests", json!({ "dry_run": true })).is_some(),
                "the plan, in either arm"
            );
            assert!(pre(&w, "python3 -m pytest tests", payload).is_none());
            let rec = spooled(&w).pop().unwrap();
            assert_eq!((rec["would_substitute"].as_bool(), rec["withheld"].as_bool()), (Some(true), Some(true)));
            assert!(rec.get("substituted").is_none());
            assert_eq!(rec["decision"]["judgements"], json!([]));
            assert_eq!(rec["decision"]["would_substitute"]["command"], "make test");
        }
    }

    /// T-A1-05: the mod's dry run answers the plan and touches neither the spool nor the ring; the
    /// verdict decides once, `checked_by: engine`; the engine's `differs` turns the rewrite into
    /// advice; the command hook's repeat of a decided call is answered with nothing.
    #[test]
    fn the_dry_run_plans_and_the_verdict_decides_once() {
        let w = u11_world(Arm::Treated);
        let plan = bash_call(&w, &NO_EXTENSION, "toolu_1", PYTEST, json!({ "dry_run": true })).expect("a plan");
        assert_eq!(
            plan,
            json!({ "plan": { "rewrite": { "command": ".venv/bin/python -m pytest tests -q", "kind": "test" } } })
        );
        assert!(pretool_records(&w).is_empty(), "a dry run records nothing");
        assert!(w.state().seen_pretool.is_empty(), "nor marks the call decided");
        let same = json!({ "verdict": { "engine": "same" } });
        let out = bash_call(&w, &NO_EXTENSION, "toolu_1", PYTEST, same.clone()).expect("the rewrite");
        assert_eq!(out["hookSpecificOutput"]["updatedInput"]["command"], ".venv/bin/python -m pytest tests -q");
        assert!(out["systemMessage"].as_str().unwrap().starts_with("adage: pointed this test run at"), "{out}");
        let recs = pretool_records(&w);
        assert_eq!(recs.len(), 1);
        assert_eq!(
            (recs[0]["checked_by"].as_str(), recs[0]["pretool"]["rewrite"].as_str()),
            (Some("engine"), Some("test"))
        );
        assert!(recs[0]["duration_us"].is_u64(), "{}", recs[0]);
        // The command hook that follows the mod, the verdict repeated, and a later dry run: nothing.
        assert!(bash_call(&w, &NO_EXTENSION, "toolu_1", ".venv/bin/python -m pytest tests -q", json!({})).is_none());
        assert!(bash_call(&w, &NO_EXTENSION, "toolu_1", PYTEST, json!({})).is_none());
        assert!(bash_call(&w, &NO_EXTENSION, "toolu_1", PYTEST, same).is_none());
        assert!(bash_call(&w, &NO_EXTENSION, "toolu_1", PYTEST, json!({ "dry_run": true })).is_none());
        assert_eq!(pretool_records(&w).len(), 1, "one record per call");
        // The engine decides the two commands differently: advice, never a rewrite.
        let differs = json!({ "verdict": { "engine": "differs" } });
        let out = bash_call(&w, &NO_EXTENSION, "toolu_2", PYTEST, differs).expect("advice");
        assert!(out["hookSpecificOutput"].get("updatedInput").is_none(), "{out}");
        let text = out["hookSpecificOutput"]["additionalContext"].as_str().unwrap();
        assert!(text.contains("Adage did not rewrite the command"), "{text}");
        let rec = pretool_records(&w).pop().unwrap();
        assert_eq!(
            (rec["checked_by"].as_str(), rec["pretool"]["advise"].as_str()),
            (Some("engine"), Some("permissions"))
        );
        // A verdict with no engine answer: the planner's own reading decides, and says so.
        assert!(bash_call(&w, &NO_EXTENSION, "toolu_3", PYTEST, json!({ "verdict": {} })).is_some());
        assert_eq!(pretool_records(&w).pop().unwrap()["checked_by"], "planner");
        // A command that names no Python program plans nothing.
        assert!(bash_call(&w, &NO_EXTENSION, "toolu_4", "make test", json!({ "dry_run": true })).is_none());
    }

    /// The control arm: the plan is the same (the mod never knows the arm), the verdict prints
    /// nothing, records the call withheld once, and the command hook's repeat stays silent.
    #[test]
    fn the_verdict_in_the_control_arm_is_withheld_and_decided_once() {
        let w = u11_world(Arm::Control);
        assert!(bash_call(&w, &NO_EXTENSION, "toolu_1", PYTEST, json!({ "dry_run": true })).is_some());
        assert!(bash_call(&w, &NO_EXTENSION, "toolu_1", PYTEST, json!({ "verdict": { "engine": "same" } })).is_none());
        assert!(bash_call(&w, &NO_EXTENSION, "toolu_1", PYTEST, json!({})).is_none());
        let recs = pretool_records(&w);
        assert_eq!(recs.len(), 1);
        assert_eq!((recs[0]["withheld"].as_bool(), recs[0]["checked_by"].as_str()), (Some(true), Some("engine")));
    }

    /// Review M2: a substitution withheld (the control arm, observe mode) records
    /// `would_substitute` and leaves the call undecided, so the interpreter rewrite still decides
    /// it as today's product does: the mod's rewrite verdict in the control arm (withheld there
    /// too), the command hook in observe mode (the rewrite made).
    #[test]
    fn a_withheld_substitution_leaves_the_call_to_the_rewrite() {
        use adage_tracks::{index::IndexedRoute, Evidence, Route, RouteState, Step, Strength, Trigger};
        let world = |arm: Arm| {
            let w = u11_world(arm);
            let mut index = Index::try_load(&w.paths.index()).unwrap();
            let repo = index.routes[0].repo.clone();
            index.routes.push(IndexedRoute {
                repo,
                track: "acme/dead-ends".into(),
                version: "0.1.0".into(),
                exec: None,
                route: Route {
                    id: "dead-end-pytest".into(),
                    summary: String::new(),
                    trigger: Trigger::SessionStart,
                    step: Step::Avoid {
                        avoid: "python3 -m pytest / python -m pytest".into(),
                        instead: "make test".into(),
                        signature: "No module named pytest".into(),
                    },
                    slots: vec![],
                    effect: adage_core::Effect::Verify,
                    strength: Strength::Inhibit,
                    state: RouteState::Firm,
                    exit: vec![],
                    evidence: Evidence { support: 5, opportunities: 5, days: 3, ..Default::default() },
                    authored: false,
                },
            });
            index.write_atomic(&w.paths.index()).unwrap();
            w
        };
        let sub = json!({ "verdict": { "engine": "same", "substitute": "dead-end-pytest" } });
        let plan = |w: &ObligationWorld| bash_call(w, &NO_EXTENSION, "toolu_1", PYTEST, json!({ "dry_run": true }));
        // The control arm: substitution withheld, then the rewrite verdict decides (withheld too).
        let w = world(Arm::Control);
        assert!(plan(&w).unwrap()["plan"].get("substitute").is_some());
        assert!(bash_call(&w, &NO_EXTENSION, "toolu_1", PYTEST, sub.clone()).is_none());
        assert!(w.state().seen_pretool.is_empty(), "a withheld substitution decides nothing");
        assert!(bash_call(&w, &NO_EXTENSION, "toolu_1", PYTEST, json!({ "verdict": { "engine": "same" } })).is_none());
        assert!(bash_call(&w, &NO_EXTENSION, "toolu_1", PYTEST, json!({})).is_none(), "decided once");
        let recs = pretool_records(&w);
        assert_eq!(recs.len(), 2, "{recs:?}");
        assert_eq!(recs[0]["would_substitute"].as_bool(), Some(true));
        assert_eq!((recs[1]["withheld"].as_bool(), recs[1]["pretool"]["rewrite"].as_str()), (Some(true), Some("test")));
        // Observe mode: substitution withheld, then the command hook makes today's rewrite.
        let w = world(Arm::Treated);
        let mut observe = sub.clone();
        observe["observe"] = json!(true);
        assert!(bash_call(&w, &NO_EXTENSION, "toolu_1", PYTEST, observe).is_none());
        let out = bash_call(&w, &NO_EXTENSION, "toolu_1", PYTEST, json!({})).expect("the command hook's rewrite");
        assert_eq!(out["hookSpecificOutput"]["updatedInput"]["command"], ".venv/bin/python -m pytest tests -q");
        let recs = pretool_records(&w);
        assert_eq!(recs.len(), 2, "{recs:?}");
        assert_eq!(recs[0]["would_substitute"].as_bool(), Some(true));
        assert!(recs[1].get("withheld").is_none(), "{}", recs[1]);
        // The treated arm still decides the call with the substitution.
        let w = world(Arm::Treated);
        assert!(bash_call(&w, &NO_EXTENSION, "toolu_1", PYTEST, sub).unwrap().get("substitute").is_some());
        assert!(bash_call(&w, &NO_EXTENSION, "toolu_1", PYTEST, json!({})).is_none());
    }

    /// A receipt delivered alone is one decision per call too, and the mod's dry run carries its
    /// note; the control arm gets none.
    #[test]
    fn a_receipt_alone_is_decided_once_and_planned_for_the_mod() {
        struct Receipts;
        impl Extension for Receipts {
            fn deliver(&self, _: &Turn<'_>, _: &[Directive]) -> Delivery {
                Delivery::default()
            }
            fn pretool_context(&self, call: &PreCall<'_>) -> Option<PreNote> {
                call.command.contains("pytest").then(|| PreNote {
                    text: "a receipt".into(),
                    fresh: true,
                    result: Some("it passed".into()),
                    covering: PYTEST.into(),
                    card: None,
                    operation: String::new(),
                })
            }
        }
        static RECEIPTS: Receipts = Receipts;
        let w = ObligationWorld::new();
        let first = bash_call(&w, &RECEIPTS, "toolu_1", PYTEST, json!({})).expect("the receipt");
        assert_eq!(first["hookSpecificOutput"]["additionalContext"], "a receipt");
        assert!(bash_call(&w, &RECEIPTS, "toolu_1", PYTEST, json!({})).is_none());
        let recs = pretool_records(&w);
        assert_eq!((recs.len(), recs[0]["receipt"].as_bool()), (1, Some(true)));
        assert!(recs[0]["duration_us"].is_u64());
        let plan = bash_call(&w, &RECEIPTS, "toolu_2", PYTEST, json!({ "dry_run": true })).expect("a plan");
        assert_eq!(
            plan,
            json!({ "plan": { "receipt": { "fresh": true, "covering": PYTEST, "note": "a receipt", "result": "it passed" } } })
        );
        state::update::<SessionState, _>(&w.paths.state("s"), |st| st.arm = Some(Arm::Control)).unwrap();
        assert!(bash_call(&w, &RECEIPTS, "toolu_3", PYTEST, json!({ "dry_run": true })).is_none());
    }

    /// P5's open issue 4: a substitution is one decision per call as well.
    #[test]
    fn a_substitution_is_decided_once_per_call() {
        let w = dead_end_world(Arm::Treated);
        let same = json!({ "verdict": { "engine": "same", "substitute": "dead-end-pytest" } });
        assert!(pre(&w, "python3 -m pytest tests", same.clone()).is_some());
        assert!(pre(&w, "python3 -m pytest tests", same).is_none(), "the mod's retry");
        assert!(pre(&w, "python3 -m pytest tests", json!({})).is_none(), "the command hook after it");
        let recs = spooled(&w);
        assert_eq!(recs.len(), 1);
        assert!(recs[0]["duration_us"].is_u64());
    }

    // ------------------------------------------------------------ a receipt as the result (C3)

    /// T-C3-02, T-C3-03 in process: the dry run carries the fresh receipt (`fresh`, `covering`,
    /// `note`, `result`); the verdict `answered: receipt` answers it, spools one `pretool` record
    /// with `answered: receipt` that meets the finish line (so the Stop gate asks nothing), and
    /// the command hook's repeat of the id prints nothing. A receipt no longer fresh decides
    /// nothing, so the call is still the command hook's; without a verdict the command hook's
    /// output is as before.
    #[test]
    fn a_call_answered_by_a_fresh_receipt_is_one_record_and_meets_its_finish_line() {
        let w = receipt_world(Arm::Treated);
        let edit = json!({ "tool_name": "Edit", "tool_use_id": "e1", "tool_response": "ok",
                           "tool_input": { "file_path": w.root.join("tests/test_a.py") } });
        let armed =
            w.event(&FRESH_RECEIPTS, "PostToolBatch", json!({ "tool_calls": [edit] })).expect("the finish line");
        assert!(armed.to_string().contains("python -m pytest tests -q"), "{armed}");
        let plan = bash_call(&w, &FRESH_RECEIPTS, "toolu_1", PYTEST, json!({ "dry_run": true })).expect("a plan");
        assert_eq!(
            plan["plan"]["receipt"],
            json!({ "fresh": true, "covering": PYTEST, "operation": "op", "note": "<adage-receipt operation=\"op\">\nAdage's run passed.\n</adage-receipt>", "result": "Adage answered this test run." })
        );
        assert!(pretool_records(&w).is_empty(), "the dry run records nothing");
        let answered = json!({ "verdict": { "answered": "receipt" } });
        let out = bash_call(&w, &FRESH_RECEIPTS, "toolu_1", PYTEST, answered.clone()).expect("the answer");
        assert_eq!(out["receipt"]["result"], "Adage answered this test run.");
        assert_eq!(out["receipt"]["covering"], PYTEST);
        assert!(out["receipt"]["note"].as_str().unwrap().starts_with("<adage-receipt"));
        let recs = pretool_records(&w);
        assert_eq!(recs.len(), 1, "{recs:?}");
        assert_eq!((recs[0]["answered"].as_str(), recs[0]["checked_by"].as_str()), (Some("receipt"), Some("engine")));
        assert_eq!(
            recs[0]["decision"]["judgements"],
            json!([{ "kind": "obligation_met", "route": "tests-after-edit" }]),
            "{}",
            recs[0]
        );
        assert!(recs[0]["duration_us"].is_u64());
        // What the receipt card needs, on the answer and on the record the feed reads.
        assert_eq!(out["receipt"]["card"]["operation"], "op");
        assert_eq!(recs[0]["card"], out["receipt"]["card"]);
        assert_eq!(recs[0]["card"]["tests"], json!(["tests/test_x.py::t"]));
        assert_eq!((recs[0]["command"].as_str(), recs[0]["tool_use_id"].as_str()), (Some(PYTEST), Some("toolu_1")));
        assert!(w.state().obligations[0].met);
        assert!(bash_call(&w, &FRESH_RECEIPTS, "toolu_1", PYTEST, json!({})).is_none(), "the command hook's repeat");
        assert!(bash_call(&w, &FRESH_RECEIPTS, "toolu_1", PYTEST, answered.clone()).is_none(), "the mod's retry");
        assert_eq!(pretool_records(&w).len(), 1);
        assert!(w.stop(&FRESH_RECEIPTS, false).is_none(), "met: the Stop gate asks nothing");
        // Review L7: the batch that lists the answered call does not observe it a second time.
        let history = w.state().history.len();
        let answered_call = json!({ "tool_name": "Bash", "tool_use_id": "toolu_1", "tool_input": { "command": PYTEST },
                                    "tool_response": "Adage answered this test run." });
        w.event(&FRESH_RECEIPTS, "PostToolBatch", json!({ "tool_calls": [answered_call] }));
        assert_eq!(w.state().history.len(), history);
        let rec = spooled(&w).pop().unwrap();
        assert_eq!(rec["decision"]["observed"], json!([]), "{rec}");
        let other = json!({ "tool_name": "Bash", "tool_use_id": "toolu_9", "tool_input": { "command": "ls" },
                            "tool_response": "a" });
        w.event(&FRESH_RECEIPTS, "PostToolBatch", json!({ "tool_calls": [other] }));
        assert!(w.state().history.len() > history, "another call is observed as ever");
        // A rebuilt state reads the met obligation off that record.
        let index = Index::try_load(&w.paths.index()).unwrap();
        let st = rebuild::rebuild(&spooled(&w), &index, "acme", CommandTable::load()).unwrap();
        assert!(st.obligations.iter().all(|o| o.met), "{:?}", st.obligations);
        // Not fresh: the plan says so without a note or a result, and the verdict decides nothing.
        let stale = "python -m pytest tests/stale_test.py -q";
        let plan = bash_call(&w, &FRESH_RECEIPTS, "toolu_2", stale, json!({ "dry_run": true })).expect("a plan");
        assert_eq!(plan["plan"]["receipt"], json!({ "fresh": false, "covering": PYTEST, "operation": "op" }));
        assert!(bash_call(&w, &FRESH_RECEIPTS, "toolu_2", stale, answered).is_none());
        assert_eq!(pretool_records(&w).len(), 1, "nothing decided");
        assert!(!w.state().has_seen_pretool(phase::identity_hash(
            &harness::Adapter::event_identity(
                &harness::claude::Claude,
                "PreToolUse",
                &json!({ "tool_use_id": "toolu_2" })
            )
            .unwrap()
        )));
        // The command hook's own payload: a stale receipt says nothing to the model.
        assert!(bash_call(&w, &FRESH_RECEIPTS, "toolu_3", stale, json!({})).is_none());
    }

    /// Control: a receipt is never the result. The plan has none (only treated sessions hold
    /// receipts), and a verdict asking for one prints nothing and records the answer withheld.
    #[test]
    fn a_receipt_is_never_the_result_in_control() {
        let w = receipt_world(Arm::Control);
        assert!(bash_call(&w, &FRESH_RECEIPTS, "toolu_1", PYTEST, json!({ "dry_run": true })).is_none());
        let answered = json!({ "verdict": { "answered": "receipt" } });
        assert!(bash_call(&w, &FRESH_RECEIPTS, "toolu_1", PYTEST, answered).is_none());
        assert!(pretool_records(&w).is_empty(), "no receipt here: nothing decided");
    }
}
