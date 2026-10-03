//! E2: at most one approved action per event, for an armed obligation or a runtime-selected
//! learned context ([`candidates`]), staged, checked and applied through the journal ([`act`]).

use adage_hook::Turn;
use adage_runtime::{render, Directive};
use adage_tracks::index::IndexedRoute;
use adage_tracks::{Origin, Step, Trigger};
use serde_json::{json, Value};
use std::collections::BTreeMap;

use super::grants::{key_lapse_notice, unplanned_notice};
use super::pytest::{failure_record, pytest_nodeids, test_evidence};
use super::{executes_at, label, run_context};
use crate::approvals::{self, Approval, Store};
use crate::request::Level;
use crate::{policy, route, sandbox};
use adage_core::CommandTable;

/// What the E2 step decided for one event.
#[derive(Default)]
pub(super) struct Acted {
    /// Route → the text replacing its directive in this event.
    pub(super) replace: BTreeMap<String, String>,
    pub(super) extra: Vec<String>,
    pub(super) met: Vec<String>,
    pub(super) stop: Option<Option<String>>,
    pub(super) records: Vec<Value>,
    pub(super) recovery: Option<String>,
    /// An approved action was selected for a Stop request at this event ([`adage_hook::Delivery::at_stop_action`]).
    pub(super) at_stop_action: bool,
    /// For the person: the selected grant no longer matches ([`adage_hook::Delivery::notice`]).
    pub(super) notice: Option<String>,
}

impl Acted {
    /// `text` in place of `route`'s directive when that directive is delivered at this event, else
    /// after the directives (the obligation was announced earlier).
    fn place(&mut self, directives: &[Directive], route: &str, text: String) {
        if directives.iter().any(|d| d.route == route) {
            self.replace.insert(route.to_string(), text);
        } else {
            self.extra.push(text);
        }
    }
}

/// The directive for a route: the one about to be delivered, else rebuilt from the index entry
/// (the obligation was announced earlier and re-armed now).
fn directive_for(ir: &IndexedRoute, directives: &[Directive], command: &str) -> Option<Directive> {
    if let Some(d) = directives.iter().find(|d| d.route == ir.route.id && d.track == ir.track) {
        return Some(d.clone());
    }
    let Step::Run { command: template } = &ir.route.step else { return None };
    let bound: Vec<(String, String)> =
        route::bound_from_command(&template.split_whitespace().map(String::from).collect::<Vec<_>>(), command)?
            .into_iter()
            .collect();
    Some(Directive {
        route: ir.route.id.clone(),
        track: ir.track.clone(),
        version: ir.version.clone(),
        strength: ir.route.strength,
        text: render::route_text(&ir.route, &bound)?,
        kind: adage_tracks::shape(&ir.route).to_string(),
        origin: None,
    })
}

/// A runtime-selected action; a context prediction never creates an edit obligation.
pub(super) struct Candidate<'a> {
    pub(super) route: &'a str,
    pub(super) track: Option<&'a str>,
    pub(super) command: &'a str,
    paths: &'a str,
    edited: &'a str,
    at_stop: bool,
    pub(super) context: Option<&'a Trigger>,
}

/// This event's actions in the order [`act`] tries them: the runtime's stimuli (armed or due
/// obligations), then each delivered learned context with execution evidence and a fixed command.
pub(super) fn candidates<'a>(turn: &'a Turn<'_>, directives: &[Directive]) -> Vec<Candidate<'a>> {
    let mut out: Vec<_> = turn
        .stimuli
        .iter()
        .map(|s| Candidate {
            route: &s.route,
            track: None,
            command: &s.command,
            paths: &s.paths,
            edited: &s.edited,
            at_stop: s.at_stop,
            context: None,
        })
        .collect();
    // Only actual runtime selections qualify. Evidence is not authority: act() still requires
    // the exact local E2 grant, then every ordinary request/sandbox/journal gate.
    for d in directives {
        let Some(ir) = route::find(turn.index, turn.repo_key, &d.track, &d.route) else { continue };
        let r = &ir.route;
        if !matches!(r.trigger, Trigger::Context { .. })
            || r.origin() != Origin::Learned
            || r.evidence.execution.is_none()
            || !r.slots.is_empty()
            || !matches!(r.effect, adage_core::Effect::Verify | adage_core::Effect::Regenerate)
            || out.iter().any(|s| s.route == r.id)
        {
            continue;
        }
        let Step::Run { command } = &r.step else { continue };
        out.push(Candidate {
            route: &r.id,
            track: Some(&ir.track),
            command,
            paths: "",
            edited: "",
            at_stop: false,
            context: Some(&r.trigger),
        });
    }
    out
}

/// E2: run at most one approved obligation or selected learned-context action.
pub(super) fn act(turn: &Turn<'_>, directives: &[Directive], candidates: &[Candidate<'_>]) -> Acted {
    let mut out = Acted::default();
    let home = &turn.paths.root;
    // Recovery precedes grant selection: a revoked/missing grant must not hide an earlier
    // possibly applied action and restore a directive asking the agent to repeat it.
    match crate::journal::pending(home, turn.root) {
        Ok(Some(receipt)) => {
            out.recovery = Some(format!("Adage paused actions in this worktree because operation {} needs recovery. Inspect its changes before acknowledging it; do not retry automatically.", receipt.id));
            out.records.push(
                json!({ "route": receipt.route, "level": "e2", "outcome": "unknown", "operation_id": receipt.id }),
            );
            return out;
        }
        Err(e) => {
            out.recovery = Some(format!("Adage cannot verify the execution journal: {}. Earlier changes may be present. Inspect the worktree and repair the journal; do not retry automatically.", crate::reason_text(&e.to_string())));
            out.records.push(json!({ "route": "*", "level": "e2", "outcome": "unknown", "withheld": "journal" }));
            return out;
        }
        Ok(None) => {}
    }
    if turn.arm == Some(adage_runtime::Arm::Control) {
        return out;
    }
    let Ok(store) = Store::load(&approvals::path(home)) else { return out };
    let approved = |s: &Candidate<'_>| -> Option<(&Approval, &IndexedRoute)> {
        store.approvals.iter().filter(|a| executes_at(a, turn.repo_key, Level::E2)).find_map(|a| {
            let ir = turn.index.routes.iter().find(|r| {
                r.repo == turn.repo_key
                    && r.route.id == s.route
                    && s.track.is_none_or(|t| t == r.track)
                    && a.route == label(&r.track, &r.route.id)
            })?;
            Some((a, ir))
        })
    };
    // At most one action per event.
    let Some((s, a, ir)) = candidates.iter().find_map(|s| approved(s).map(|(a, ir)| (s, a, ir))) else { return out };
    let name = a.route.clone();
    let Some(dir) = directive_for(ir, directives, s.command) else {
        out.records.push(json!({ "route": name, "level": "e2", "withheld": "render",
                                 "reason": "the action's directive could not be rendered" }));
        return out;
    };
    let not_run = |out: &mut Acted, stage: &str, reason: &str| {
        out.records
            .push(json!({ "route": name, "level": "e2", "withheld": stage, "reason": crate::reason_text(reason), "context": s.context }));
        if s.context.is_some() {
            // The spool records the reason; an unexecuted prediction supplies no agent advice.
            out.replace.insert(dir.route.clone(), String::new());
        } else if s.at_stop && ir.exec_only() {
            // The agent cannot run a vendored script itself: asking it to would be unfollowable.
            out.stop = Some(None);
        } else if s.at_stop {
            out.stop = Some(Some(render::stop_reason_not_run(s.paths, s.command, reason)));
        } else {
            out.place(directives, &dir.route, render::action_not_run(&dir, reason));
        }
    };
    if !turn.harness.capabilities().actions {
        // Named from the harness: only a hook that can wait for a run carries actions out.
        let reason = format!("actions do not run under {}, whose hooks cannot wait for a run", turn.harness.display());
        not_run(&mut out, "harness", &reason);
        return out;
    }
    let table = CommandTable::load();
    let plan = match route::plan(ir, turn.index, turn.root, &turn.paths.tracks(), Level::E2, a.terms.as_ref(), &table) {
        Ok(p) => p,
        Err(e) => {
            out.notice = Some(unplanned_notice(a, &e));
            not_run(&mut out, "route", &e);
            return out;
        }
    };
    let mut req = plan.request;
    match route::bound_from_command(&req.argv, s.command) {
        Some(inputs) => req.inputs = inputs,
        None => {
            not_run(&mut out, "bind", "its command could not be bound from the selected action");
            return out;
        }
    }
    // A grant that no longer matches what would run is refused below (`run::check`); the person
    // is told which field changed and how to approve it again, not only the spool.
    let key = approvals::ApprovalKey::for_request(&req, turn.repo_key);
    if store.find(&key).is_none() {
        out.notice = Some(key_lapse_notice(a, &key));
    }
    let Ok(pol) = policy::load(home) else {
        not_run(&mut out, "policy", "the policy file is unreadable");
        return out;
    };
    let cap = sandbox::detect();
    let ctx = run_context(&pol, &store, &cap, &table, turn.repo_key, home);
    // Prefer native tool-call identity: a redelivery must not become a fresh apply just
    // because decide() incremented a batch counter. Counters remain a best-effort fallback
    // for Stop and older payloads without IDs; those cannot promise redelivery deduplication.
    let state = match adage_store::state::try_read::<adage_runtime::SessionState>(&turn.paths.state(turn.session)) {
        Ok(Some(s)) => s,
        _ => {
            not_run(&mut out, "state", "the action occurrence cannot be read safely");
            return out;
        }
    };
    // Runtime emits a context route once per session. Refuse replay before even spawning;
    // completed/failed receipts survive state reconstruction and native event redelivery.
    if s.context.is_some() {
        match crate::journal::list(home) {
            Ok(rows)
                if rows.iter().any(|r| r.repo == turn.repo_key && r.session == turn.session && r.route == name) =>
            {
                not_run(&mut out, "duplicate", "this context action was already attempted in this session");
                return out;
            }
            Err(_) => {
                not_run(&mut out, "journal", "the execution journal cannot be read safely");
                return out;
            }
            _ => {}
        }
    }
    let occurrence = match &turn.event_identity {
        Some(id) => format!("native:{id}:{}", s.route),
        None => format!("counter:{}:{}:{}:{}", turn.event, state.prompts, state.batches, s.route),
    };
    let nodeids_before = pytest_nodeids(turn.root, &req, s.command);
    // From here this Stop's time belongs to this action, whatever its outcome: no Stop gate too.
    out.at_stop_action = s.at_stop;
    let staged = match crate::stage::stage(&req, &ctx) {
        Ok(x) => x,
        Err(r) => {
            not_run(&mut out, r.stage, &r.reason);
            return out;
        }
    };
    let completed = crate::journal::finish(
        &staged,
        &crate::journal::Commit {
            home,
            repo: turn.repo_key,
            session: turn.session,
            occurrence: &occurrence,
            approved_on: &a.approved_on,
            request: &req,
            origin: if s.context.is_some() {
                crate::journal::RunOrigin::Context
            } else {
                crate::journal::RunOrigin::Obligation
            },
        },
    );
    let outcome = completed.outcome;
    let revoked = completed.revoked;
    let mut rec = json!({
        "route": name, "level": "e2", "outcome": outcome.name(), "exit": staged.exit,
        "duration_ms": staged.duration_ms,
        "operation_id": completed.id, "replayed_receipt": completed.replayed,
        "context": s.context,
    });
    if revoked {
        rec["revoked"] = json!(true);
    }
    // Refused or failed in the stage: the reason, as if the action had not run, with the run's
    // own record after it.
    let not_applied = |out: &mut Acted, stage: &str, reason: &str, rec: Value| {
        let records = std::mem::take(&mut out.records);
        not_run(out, stage, reason);
        out.records = records;
        out.records.push(rec);
    };
    match &outcome {
        crate::stage::Outcome::Unknown { reason } => {
            // Do not fall back to an instruction to repeat a potentially applied action.
            let text = format!("Adage cannot confirm this action's completion: {}. Do not retry it automatically; inspect the worktree and ask the person to acknowledge recovery.", crate::reason_text(reason));
            out.recovery = Some(text.clone());
            if s.at_stop {
                out.stop = Some(Some(text));
            } else {
                let block = render::wrap_origin(
                    &dir.track,
                    &dir.version,
                    &dir.route,
                    &dir.kind,
                    dir.strength,
                    &text,
                    dir.origin.as_ref(),
                );
                out.place(directives, &dir.route, block);
            }
            out.records.push(rec);
        }
        crate::stage::Outcome::Applied { changed, .. } => {
            out.records.push(rec);
            if s.context.is_none() {
                out.met.push(s.route.to_string());
            }
            if s.at_stop {
                // The Stop hook cannot carry context: the request is withdrawn, the record kept.
                out.stop = Some(None);
            } else {
                let paths: Vec<String> = changed.iter().map(|l| l.path.clone()).collect();
                let tests = (ir.route.effect == adage_core::Effect::Verify)
                    .then(|| {
                        let edited = &state.new_tests;
                        test_evidence(turn, &req, s.command, completed.id.as_deref(), nodeids_before.as_ref(), edited)
                    })
                    .flatten();
                let text = if s.context.is_some() {
                    render::action_predicted(
                        &dir,
                        s.command,
                        &paths,
                        ir.route.effect == adage_core::Effect::Verify,
                        &render::CpuReceipt {
                            tests: tests.as_ref(),
                            operation: completed.id.as_deref().unwrap_or_default(),
                            input_digest: &crate::sha256_hex(crate::canonical_json(&staged.inputs).as_bytes()),
                            exit: staged.exit,
                            duration_ms: staged.duration_ms,
                        },
                    )
                } else {
                    render::action_applied(&dir, s.edited, s.command, &paths)
                };
                out.place(directives, &dir.route, text);
            }
        }
        crate::stage::Outcome::Refused { reason, .. } => {
            let reason = if revoked { format!("{reason}; its approval is now revoked") } else { reason.clone() };
            not_applied(&mut out, "refused", &reason, rec);
        }
        crate::stage::Outcome::Failed { exit, .. }
            if s.context.is_some() && ir.route.effect == adage_core::Effect::Verify =>
        {
            // A failed run is a result the agent needs: what failed in this run, from the declared
            // record (never a stale list).
            let record = failure_record(&staged, s.command, &state.new_tests);
            out.records.push(rec);
            out.replace.insert(
                dir.route.clone(),
                render::action_failed(
                    &dir,
                    s.command,
                    *exit,
                    record.as_ref(),
                    completed.id.as_deref().unwrap_or_default(),
                ),
            );
        }
        crate::stage::Outcome::Failed { exit, .. } => {
            let reason = match exit {
                Some(e) => format!("it exited {e} in the staged copy, and nothing was applied"),
                None => "it was stopped in the staged copy, and nothing was applied".to_string(),
            };
            not_applied(&mut out, "failed", &reason, rec);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::testkit::*;
    use super::super::PREFETCH;
    use super::*;
    use adage_hook::Extension;
    use adage_runtime::{Arm, SessionState};
    use adage_tracks::Index;

    #[test]
    fn selected_context_uses_typed_command_across_related_paths_and_intervening_reads() {
        for path in ["src/feature.rs", "src/another.rs"] {
            let mut f = Fixture::new();
            let (event, mut dirs) = f.selected(path);
            assert_eq!(dirs.len(), 1);
            dirs[0].text = "untrusted prose suggesting another command".into();
            let turn = f.turn(&event);
            let c = candidates(&turn, &dirs);
            assert_eq!(c.len(), 1);
            assert_eq!(c[0].command, "sh tools/check.sh");
            assert!(c[0].context.is_some());
            assert!(candidates(&turn, &[]).is_empty(), "only actual selections nominate");
            f.index.routes[0].route.authored = true;
            assert!(candidates(&f.turn(&event), &dirs).is_empty(), "authored context is not learned");
            f.index.routes[0].route.authored = false;
            f.index.routes[0].route.evidence.execution = None;
            assert!(
                candidates(&f.turn(&event), &dirs).is_empty(),
                "advice without execution evidence is not nominated"
            );
        }
    }

    #[test]
    fn unapproved_context_does_not_execute_and_corrupt_journal_preserves_recovery() {
        let f = Fixture::new();
        let (event, dirs) = f.selected("src/feature.rs");
        let turn = f.turn(&event);
        let d = PREFETCH.deliver(&turn, &dirs);
        assert!(d.spool.is_none() && d.met.is_empty());
        assert!(d.texts.unwrap().iter().all(String::is_empty), "no unapproved fallback advice");
        assert!(crate::journal::list(&f.paths.root).unwrap().is_empty());
        std::fs::create_dir_all(f.paths.root.join("exec")).unwrap();
        std::fs::write(crate::stage::spool_path(&f.paths.root), b"corrupt journal").unwrap();
        let d = PREFETCH.deliver(&turn, &dirs);
        assert!(d.recovery.is_some(), "recovery precedes missing approval");
    }

    #[test]
    fn stale_context_grant_never_falls_back_to_agent_advice() {
        let f = Fixture::new();
        f.approve();
        std::fs::write(f.root.join("tools/check.sh"), "#!/bin/sh\necho changed\n").unwrap();
        let (event, dirs) = f.selected("src/feature.rs");
        let delivery = PREFETCH.deliver(&f.turn(&event), &dirs);
        assert!(delivery.texts.unwrap().iter().all(String::is_empty));
        assert_eq!(delivery.cpu_predictions, vec!["predict-check"], "abstention is not ignored agent advice");
        assert!(delivery.spool.unwrap()[0].get("withheld").is_some());
        assert!(crate::journal::list(&f.paths.root).unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn prior_sessions_learn_a_context_that_executes_in_a_fresh_session() {
        if !staging() {
            return;
        }
        let mut f = Fixture::new();
        std::fs::write(f.root.join("Makefile"), "test:\n\t@test -f src/feature.rs\n").unwrap();
        let history = f._tmp.path().join("history");
        std::fs::create_dir(&history).unwrap();
        let mut transcripts = vec![];
        for i in 0..5 {
            let sid = format!("past-task-{i}");
            let mut lines = vec![
                json!({"type":"user","sessionId":sid,"cwd":f.root,"timestamp":"2026-09-20T10:00:00Z","message":{"role":"user","content":format!("Implement past feature {i}")}}),
            ];
            let calls = [
                ("Edit", json!({"file_path":f.root.join("src/feature.rs")})),
                ("Bash", json!({"command":if i % 2 == 0 { "git diff --stat" } else { "git diff --name-only" }})),
                ("Bash", json!({"command":"git status --short"})),
                ("Bash", json!({"command":"make test"})),
            ];
            for (n, (tool, input)) in calls.into_iter().enumerate() {
                let id = format!("tool-{n}");
                lines.push(json!({"type":"assistant","sessionId":sid,"cwd":f.root,"message":{"id":format!("turn-{n}"),"model":"fixture","content":[{"type":"tool_use","id":id,"name":tool,"input":input}]}}));
                lines.push(json!({"type":"user","sessionId":sid,"cwd":f.root,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":id,"content":"done","is_error":false}]}}));
            }
            let path = history.join(format!("{sid}.jsonl"));
            std::fs::write(&path, lines.iter().map(Value::to_string).collect::<Vec<_>>().join("\n")).unwrap();
            transcripts.push(path);
        }
        let inputs = adage_learn::Inputs {
            transcripts,
            outcomes: BTreeMap::new(),
            repo_override: Some("fixture".into()),
            models: None,
            cache: None,
            only_repo: None,
        };
        adage_learn::learn(&inputs, &f.paths.tracks(), &f.paths.index(), &Default::default()).unwrap();
        let learned = Index::load(&f.paths.index());
        let ir = learned
            .routes
            .iter()
            .find(|r| {
                matches!(r.route.trigger, Trigger::Context { .. })
                    && r.route.evidence.execution.is_some()
                    && matches!(&r.route.step, Step::Run { command } if command == "make test")
            })
            .expect("production learning must produce the context and certificate")
            .clone();
        assert_eq!(ir.route.evidence.execution.as_ref().unwrap().sessions, 5);
        assert!(!ir.route.authored);
        // Isolate the learned candidate under test without altering its trigger, command or evidence.
        f.index = Index { routes: vec![ir.clone()], ..learned };
        f.approve(); // A test-only grant in this temporary home, never a real user's approval.
        let table = CommandTable::load();
        let ctx = f.ctx(&table, (ir.track.clone(), ir.route.id.clone()));
        let mut state = SessionState { arm: Some(Arm::Treated), ..Default::default() };
        let mut last = None;
        {
            for (tool, input) in [
                ("Edit", json!({"file_path":f.root.join("src/feature.rs")})),
                ("Bash", json!({"command":"git diff --stat"})),
                ("Read", json!({"file_path":f.root.join("README.md")})),
                ("Bash", json!({"command":"git status --short"})),
            ] {
                let event = batch(tool, input, "done");
                let decision = adage_runtime::decide(&ctx, &mut state, &event);
                if !decision.directives.is_empty() {
                    last = Some((event, decision.directives));
                }
            }
        };
        let (event, dirs) = last.expect("fresh related sequence predicts the learned command");
        assert!(state.obligations.is_empty());
        let delivery = PREFETCH.deliver(&f.turn(&event), &dirs);
        let records = delivery.spool.unwrap();
        assert_eq!(records[0]["outcome"], "applied", "{records}");
        assert_eq!(delivery.cpu_predictions, vec![ir.route.id]);
        assert!(delivery.texts.unwrap().join("\n").contains("after your edits: passed (exit 0"));
        assert_eq!(std::fs::read_to_string(f.root.join("src/feature.rs")).unwrap(), "fixture\n");
    }

    #[cfg(unix)]
    #[test]
    fn approved_context_runs_on_cpu_and_duplicate_never_stages_again() {
        if !staging() {
            return;
        }
        let f = Fixture::new();
        f.approve();
        let (event, dirs) = f.selected("src/feature.rs");
        let mut turn = f.turn(&event);
        turn.arm = Some(Arm::Control);
        assert!(PREFETCH.deliver(&turn, &dirs).spool.is_none());
        turn.arm = Some(Arm::Treated);
        let d = PREFETCH.deliver(&turn, &dirs);
        let records = d.spool.as_ref().unwrap();
        assert_eq!(records[0]["outcome"], "applied", "{records}");
        assert_eq!(records[0]["context"]["on"], "context");
        assert!(d.met.is_empty(), "no invented obligation completion");
        let text = d.texts.unwrap().join("\n");
        assert!(text.contains("after your edits: passed (exit 0") && text.contains("exec=\"e2\""), "{text}");
        let receipt = crate::journal::list(&f.paths.root).unwrap().pop().unwrap();
        assert!(text.contains(&receipt.id[..12]) && !text.contains(&receipt.input_digest), "{text}");
        assert_eq!(receipt.origin, Some(crate::journal::RunOrigin::Context), "the journal says what started it");
        assert!(text.contains("stays current until a file changes"), "{text}");
        assert!(!text.contains("COMPLETED CPU ACTION") && !text.contains("not requested by the user"), "{text}");
        assert!(!text.contains("after your edit to"));
        let repeat = PREFETCH.deliver(&turn, &dirs);
        assert_eq!(repeat.spool.unwrap()[0]["withheld"], "duplicate");
        assert_eq!(crate::journal::list(&f.paths.root).unwrap().len(), 1);
    }

    /// A lapsed grant at the moment its action would run: withheld as before (no agent advice),
    /// and now one line for the person with the field that changed and how to approve it again.
    #[test]
    fn a_lapsed_grant_is_told_to_the_person_when_its_action_is_withheld() {
        let f = Fixture::new();
        f.approve();
        let (event, dirs) = f.selected("src/feature.rs");
        std::fs::write(f.root.join("tools/check.sh"), "#!/bin/sh\ntest -f src/other.rs\n").unwrap();
        let d = PREFETCH.deliver(&f.turn(&event), &dirs);
        assert!(d.texts.unwrap().iter().all(String::is_empty), "no fallback advice");
        assert_eq!(d.spool.unwrap()[0]["withheld"], "approval");
        let notice = d.notice.expect("the person is told");
        assert!(notice.contains("fixture/context/predict-check") && notice.contains("route_exec_digest"), "{notice}");
        assert!(
            notice.contains("`adage review delegate fixture/context predict-check --level e2 --effect verify`"),
            "{notice}"
        );
        assert!(crate::journal::list(&f.paths.root).unwrap().is_empty(), "nothing ran");
    }
}
