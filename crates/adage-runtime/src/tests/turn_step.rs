//! TurnStep: predicted steps.

use super::*;

/// T-C1-01: a delivered directive is predicted with its rendered command, under a grant; the
/// call Adage then makes is judged `predicted`, never `followed`, and retires the follow
/// judgement. An armed obligation is predicted at the step that ends the turn.
#[test]
fn turn_step_predicts_a_delivered_directive() {
    let (dir, index, table) = predict_fixture(vec![]);
    let ctx = predict_ctx(&dir, &index, table, &["lookup", "gen"]);
    let mut st = SessionState { arm: Some(Arm::Treated), ..Default::default() };
    decide(&ctx, &mut st, &Event::Prompt { text: "Pick up W-2042 please".into() });
    assert_eq!(st.pending[0].command, "./scripts/wi-lookup W-2042 --json", "the rendered command is kept");
    let d = decide(&ctx, &mut st, &step("t", 1, false));
    let p = d.predicted.clone().expect("the lookup is predicted");
    assert_eq!((p.kind, p.route.as_str(), p.track.as_str()), (PredictionKind::Run, "lookup", "acme/t"));
    assert_eq!(
        p.calls,
        vec![PredictedCall { tool: "Bash".into(), input: json!({ "command": "./scripts/wi-lookup W-2042 --json" }) }]
    );
    assert_eq!(d.would_predict, d.predicted);
    assert_eq!((st.last_step_predicted.clone(), st.predictions), (Some(("t".into(), 1)), 1));
    assert!(d.directives.is_empty() && d.judgements.is_empty());
    // The engine runs the call Adage yielded; the batch carries it with Adage's id.
    let out = r#"{"id":"W-2042","linked_files":["acme/billing.py"]}"#;
    let call = by_adage(bash("./scripts/wi-lookup W-2042 --json", out), "adage_lookup_t_1");
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![call] });
    assert!(
        d.judgements.iter().any(|j| matches!(j, Judgement::Predicted { route } if route == "lookup")),
        "{:?}",
        d.judgements
    );
    assert!(!d.judgements.iter().any(|j| matches!(j, Judgement::Followed { .. })));
    assert!(st.pending.is_empty(), "the follow judgement is retired");
    // An armed obligation: only at the step that ends the turn.
    let mut st = SessionState {
        arm: Some(Arm::Treated),
        obligations: vec![armed("gen", "make gen", table)],
        ..Default::default()
    };
    assert!(decide(&ctx, &mut st, &step("u", 1, false)).predicted.is_none());
    let p = decide(&ctx, &mut st, &step("u", 2, true)).predicted.expect("the finish line");
    assert_eq!((p.kind, p.calls[0].input["command"].as_str()), (PredictionKind::Obligation, Some("make gen")));
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![by_adage(bash("make gen", "ok"), "adage_gen_u_2_0")] });
    assert!(st.obligations[0].met);
    assert!(d.judgements.iter().any(|j| matches!(j, Judgement::ObligationMet { route } if route == "gen")));
    assert!(d.judgements.iter().any(|j| matches!(j, Judgement::Predicted { route } if route == "gen")));
}

/// T-C1-02: never at the turn's first step, never right after a predicted step.
#[test]
fn turn_step_never_at_index_zero_nor_twice_in_a_row() {
    let (dir, index, table) = predict_fixture(vec![]);
    let ctx = predict_ctx(&dir, &index, table, &["lookup", "gen"]);
    let mut st = SessionState {
        arm: Some(Arm::Treated),
        pending: vec![pending("lookup", "./scripts/wi-lookup W-2042 --json", table)],
        obligations: vec![armed("gen", "make gen", table)],
        ..Default::default()
    };
    let d = decide(&ctx, &mut st, &step("t", 0, true));
    assert!(d.predicted.is_none() && d.would_predict.is_none(), "index 0 is the prompt's request");
    assert_eq!(decide(&ctx, &mut st, &step("t", 1, true)).predicted.unwrap().route, "lookup");
    let d = decide(&ctx, &mut st, &step("t", 2, false));
    assert!(d.predicted.is_none() && d.would_predict.is_none(), "a model step separates two predictions");
    // A model step later, the next thing due (the lookup is never predicted twice).
    assert_eq!(decide(&ctx, &mut st, &step("t", 3, true)).predicted.unwrap().route, "gen");
    assert_eq!(st.predictions, 2);
    // At a turn's end the mod asks with the index of the request the model just answered: a
    // prediction before that request (index - 1) is not "in a row" with it, one at it would be.
    let mut st = SessionState { obligations: vec![armed("gen", "make gen", table)], ..Default::default() };
    st.last_step_predicted = Some(("t".into(), 4));
    assert!(decide(&ctx, &mut st, &step("t", 5, true)).predicted.is_some(), "model step 5 came between");
    st.obligations[0].met = false;
    st.predicted_routes.clear();
    assert!(decide(&ctx, &mut st, &step("t", 5, true)).predicted.is_none(), "predicted at 5 already");
    // A new turn's step 1 after the last step of another turn is not "in a row".
    let mut st = SessionState { obligations: vec![armed("gen", "make gen", table)], ..Default::default() };
    st.last_step_predicted = Some(("t".into(), 0));
    assert!(decide(&ctx, &mut st, &step("u", 1, true)).predicted.is_some());
}

/// T-C1-03: at most `predict_cap` per session; past it, recorded as `would_predict` only.
#[test]
fn turn_step_holds_the_session_cap() {
    let (dir, index, table) = predict_fixture(vec![]);
    let mut ctx = predict_ctx(&dir, &index, table, &["lookup", "gen"]);
    ctx.policy.predict_cap = 1;
    let mut st = SessionState {
        pending: vec![pending("lookup", "./scripts/wi-lookup W-2042 --json", table)],
        obligations: vec![armed("gen", "make gen", table)],
        ..Default::default()
    };
    assert!(decide(&ctx, &mut st, &step("t", 1, true)).predicted.is_some());
    let d = decide(&ctx, &mut st, &step("t", 3, true));
    assert!(d.predicted.is_none() && d.predicted_at.is_none(), "the cap holds");
    assert_eq!(d.would_predict.map(|p| p.route), Some("gen".into()));
    assert_eq!(st.predictions, 1);
    assert_eq!(Policy::default().predict_cap, 6);
}

/// T-C1-04: the control arm predicts nothing and records what it would have; the bounds count
/// in both arms.
#[test]
fn turn_step_in_the_control_arm_records_would_predict() {
    let (dir, index, table) = predict_fixture(vec![]);
    let ctx = predict_ctx(&dir, &index, table, &["lookup"]);
    let mut st = SessionState {
        arm: Some(Arm::Control),
        pending: vec![pending("lookup", "./scripts/wi-lookup W-2042 --json", table)],
        ..Default::default()
    };
    let d = decide(&ctx, &mut st, &step("t", 1, false));
    assert!(d.predicted.is_none());
    assert_eq!(d.would_predict.map(|p| p.route), Some("lookup".into()));
    assert_eq!((d.predicted_at, st.predictions), (Some(("t".into(), 1)), 1));
}

/// T-C1-05: no grant, no command, an inhibited route, a route already carried out by CPU
/// prediction, a subagent's step: nothing.
#[test]
fn turn_step_predicts_nothing_without_a_grant_a_command_or_past_the_interlock() {
    let dead_gen = route(
        "dead-gen",
        Trigger::SessionStart,
        Step::Avoid { avoid: "make gen".into(), instead: "make generate".into(), signature: "boom".into() },
        vec![],
        Effect::Regenerate,
        Strength::Inhibit,
    );
    let (dir, index, table) = predict_fixture(vec![dead_gen]);
    let state = || SessionState {
        pending: vec![pending("lookup", "./scripts/wi-lookup W-2042 --json", table)],
        obligations: vec![armed("gen", "make gen", table)],
        ..Default::default()
    };
    let nothing = |ctx: &Ctx, st: &mut SessionState, ev: &Event| {
        let d = decide(ctx, st, ev);
        d.predicted.is_none() && d.would_predict.is_none() && st.predictions == 0
    };
    // No grant (the plain `adage hook`).
    assert!(nothing(&predict_ctx(&dir, &index, table, &[]), &mut state(), &step("t", 1, true)));
    // A grant on another route only.
    assert!(nothing(&predict_ctx(&dir, &index, table, &["read-linked"]), &mut state(), &step("t", 1, true)));
    let ctx = predict_ctx(&dir, &index, table, &["lookup", "gen"]);
    // A pending entry from an older state: no command kept.
    let mut st = SessionState {
        pending: vec![Pending { command: String::new(), ..pending("lookup", "x", table) }],
        ..Default::default()
    };
    assert!(nothing(&ctx, &mut st, &step("t", 1, true)));
    // The dead end announced this session says not to run `make gen` (without it, it would be).
    let mut st = SessionState { obligations: vec![armed("gen", "make gen", table)], ..Default::default() };
    assert!(!nothing(&ctx, &mut st, &step("t", 1, true)));
    let mut st = SessionState {
        obligations: vec![armed("gen", "make gen", table)],
        emitted: vec!["dead-gen".into()],
        ..Default::default()
    };
    assert!(nothing(&ctx, &mut st, &step("t", 1, true)));
    // Carried out by adage-exec's CPU prediction at the batch before.
    let mut st = SessionState { predicted_routes: vec!["lookup".into(), "gen".into()], ..state() };
    assert!(nothing(&ctx, &mut st, &step("t", 1, true)));
    // A subagent's step.
    let sub = Event::TurnStep { turn_id: "t".into(), index: 1, model: None, agent: Some("a1".into()), ending: false };
    assert!(nothing(&ctx, &mut state(), &sub));
}

/// T-C1-06 and the sequence: a skill step yields the Skill tool; a sequence its first command.
#[test]
fn turn_step_predicts_a_skill_and_a_sequence_first_command() {
    // A skill step (D3), delivered after an edit; invoked only as the policy allows.
    let release = route(
        "release",
        Trigger::AfterEdit { paths: "CHANGELOG.md".into() },
        Step::Skill { skill: "release-checklist".into(), args: None },
        vec![],
        Effect::Observe,
        Strength::Prime,
    );
    let seq = route(
        "lint-then-test",
        Trigger::AfterEdit { paths: "docs/*.md".into() },
        Step::Sequence { commands: vec!["make lint".into(), "make test".into()] },
        vec![],
        Effect::Verify,
        Strength::Prime,
    );
    let (dir, index, table) = predict_fixture(vec![release, seq]);
    let mut ctx = predict_ctx(&dir, &index, table, &["release", "lint-then-test"]);
    ctx.policy.invoke_skills = vec!["release-checklist".into()];
    let mut st = SessionState::default();
    decide(&ctx, &mut st, &Event::Batch { calls: vec![edit_call(&dir, "CHANGELOG.md")] });
    assert_eq!(st.pending[0].expect_key, "control Skill release-checklist");
    let p = decide(&ctx, &mut st, &step("t", 1, false)).predicted.unwrap();
    assert_eq!(p.kind, PredictionKind::Skill);
    assert_eq!(p.calls, vec![PredictedCall { tool: "Skill".into(), input: json!({ "skill": "release-checklist" }) }]);
    // A sequence after an edit: its first command, as delivered.
    let mut st = SessionState::default();
    let edit = Call {
        tool: "Edit".into(),
        input: json!({ "file_path": dir.path().join("docs/a.md").to_string_lossy() }),
        output: String::new(),
        failed: false,
        by: By::Agent,
        tool_use_id: None,
        agent: None,
    };
    decide(&ctx, &mut st, &Event::Batch { calls: vec![edit] });
    assert_eq!(st.pending[0].command, "make lint");
    let p = decide(&ctx, &mut st, &step("t", 1, false)).predicted.unwrap();
    assert_eq!((p.kind, p.calls[0].input["command"].as_str()), (PredictionKind::Sequence, Some("make lint")));
}

/// A chain: the lookup's result binds the read batch's files; the next step reads them, one call
/// per file; Adage's reads retire it, judged `predicted`.
#[test]
fn turn_step_predicts_a_chain_of_reads_bound_from_the_lookup() {
    let (dir, index, table) = predict_fixture(vec![]);
    std::fs::write(dir.path().join("acme/rates.py"), "x").unwrap();
    let ctx = predict_ctx(&dir, &index, table, &["read-linked"]);
    let mut st = SessionState::default();
    decide(&ctx, &mut st, &Event::Prompt { text: "Pick up W-2042 please".into() });
    let out = r#"{"id":"W-2042","linked_files":["acme/billing.py","../etc/passwd","acme/rates.py"]}"#;
    decide(&ctx, &mut st, &Event::Batch { calls: vec![bash("./scripts/wi-lookup W-2042 --json", out)] });
    assert_eq!(st.reads[0].files, vec!["acme/billing.py".to_string(), "acme/rates.py".into()]);
    let p = decide(&ctx, &mut st, &step("t", 1, false)).predicted.expect("the reads");
    assert_eq!((p.kind, p.route.as_str()), (PredictionKind::Chain, "read-linked"));
    let files: Vec<&str> = p.calls.iter().map(|c| c.input["file_path"].as_str().unwrap()).collect();
    let abs = |f: &str| dir.path().join(f).to_string_lossy().to_string();
    assert_eq!(files, vec![abs("acme/billing.py"), abs("acme/rates.py")]);
    assert!(p.calls.iter().all(|c| c.tool == "Read"));
    let read = |f: &str| {
        by_adage(
            Call { tool: "Read".into(), input: json!({ "file_path": abs(f) }), ..bash("", "") },
            "adage_read-linked_t_1",
        )
    };
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![read("acme/billing.py"), read("acme/rates.py")] });
    assert_eq!(
        d.judgements.iter().filter(|j| matches!(j, Judgement::Predicted { route } if route == "read-linked")).count(),
        1
    );
    assert!(st.reads.is_empty());
    // The agent's own read retires it too, with no judgement; so does the follow window.
    let mut st = SessionState::default();
    decide(&ctx, &mut st, &Event::Prompt { text: "Pick up W-2042 please".into() });
    decide(&ctx, &mut st, &Event::Batch { calls: vec![bash("./scripts/wi-lookup W-2042 --json", out)] });
    let mine = Call { tool: "Read".into(), input: json!({ "file_path": abs("acme/rates.py") }), ..bash("", "") };
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![mine] });
    assert!(st.reads.is_empty() && !d.judgements.iter().any(|j| matches!(j, Judgement::Predicted { .. })));
}

/// T-C1-07: the bounds survive a rebuild from the spool.
#[test]
fn turn_step_bounds_survive_a_rebuild() {
    let (dir, index, table) = predict_fixture(vec![]);
    let ctx = predict_ctx(&dir, &index, table, &["lookup"]);
    let mut st = SessionState { arm: Some(Arm::Treated), repo: Some("acme".into()), ..Default::default() };
    let mut records = Vec::new();
    for ev in [Event::Prompt { text: "Pick up W-2042 please".into() }, step("t", 1, false)] {
        let d = decide(&ctx, &mut st, &ev);
        records.push(json!({ "event": "x", "kind": ev.name(), "arm": st.arm, "repo": "acme", "decision": d }));
    }
    assert_eq!(records[1]["kind"], "turn_step");
    let rebuilt = rebuild::rebuild(&records, &index, "acme", table).unwrap();
    assert_eq!(rebuilt.predictions, 1);
    assert_eq!(rebuilt.last_step_predicted, Some(("t".into(), 1)));
    assert_eq!(rebuilt.predicted_routes, vec!["lookup".to_string()]);
    assert_eq!(rebuild::kind_of(&json!({ "event": "TurnStep" })), "turn_step");
}

/// T-C1-08: the fast path skips a step with nothing directed, never one with something due.
#[test]
fn turn_step_fast_path() {
    let (_dir, _index, table) = predict_fixture(vec![]);
    let t = adage_tracks::triggers::Triggers::default();
    let ev = step("t", 1, true);
    assert!(fast::skip(&t, &ev, "/r", || Some(SessionState::default())));
    let met = Obligation { met: true, ..armed("gen", "make gen", table) };
    assert!(fast::skip(&t, &ev, "/r", || Some(SessionState { obligations: vec![met.clone()], ..Default::default() })));
    let due = [
        SessionState { pending: vec![pending("lookup", "x", table)], ..Default::default() },
        SessionState { obligations: vec![armed("gen", "make gen", table)], ..Default::default() },
        SessionState {
            reads: vec![PendingReads {
                route: "r".into(),
                files: vec!["a".into()],
                batches_left: 1,
                after_adage: false,
            }],
            ..Default::default()
        },
    ];
    for st in due {
        assert!(!fast::skip(&t, &ev, "/r", || Some(st.clone())), "{st:?}");
    }
    assert!(!fast::skip(&t, &ev, "/r", || None), "an unreadable state is rebuilt, never skipped past");
}

/// The mod's verdict on the calls of the step `step` found ([`StepPhase::Verdict`]).
fn verdict(permission: Permission, route: Option<&str>) -> StepPhase {
    StepPhase::Verdict {
        route: route.map(str::to_string),
        permission: PermissionVerdict {
            decision: permission,
            tool: Some("Bash".into()),
            reason: Some("This command requires approval".into()),
        },
    }
}

/// T-C4-08: a dry run answers the step it would yield and commits nothing: the state is as it
/// was (no bound counts it, the route stays offered), and nothing is routed while it waits.
#[test]
fn a_dry_run_answers_the_prediction_and_commits_nothing() {
    let (dir, index, table) = predict_fixture(vec![]);
    let mut ctx = predict_ctx(&dir, &index, table, &["gen"]);
    ctx.policy.step = StepPhase::DryRun;
    ctx.policy.routine = true;
    let mut st = SessionState {
        arm: Some(Arm::Treated),
        obligations: vec![armed("gen", "make gen", table)],
        ..Default::default()
    };
    let before = serde_json::to_value(&st).unwrap();
    let d = decide(&ctx, &mut st, &step("t", 2, true));
    assert_eq!(d.predicted.as_ref().map(|p| p.route.as_str()), Some("gen"));
    assert!(d.predicted_at.is_none() && d.routine.is_none() && d.would_route.is_none());
    assert_eq!(serde_json::to_value(&st).unwrap(), before, "nothing committed");
    // Asked again, the same answer: the route was not taken.
    assert_eq!(decide(&ctx, &mut st, &step("t", 2, true)).predicted, d.predicted);
}

/// T-C4-08: a dry run whose step would yield nothing has no verdict to wait for, so it decides
/// and commits as a step with no phase does: the control arm and observe mode count the bound
/// and record `would_predict`; past the cap only `would_predict`.
#[test]
fn a_dry_run_that_would_yield_nothing_commits_as_before() {
    let (dir, index, table) = predict_fixture(vec![]);
    let mut ctx = predict_ctx(&dir, &index, table, &["gen"]);
    for (arm, observe) in [(Arm::Control, false), (Arm::Treated, true)] {
        ctx.policy.observe = observe;
        let fresh = || SessionState {
            arm: Some(arm),
            obligations: vec![armed("gen", "make gen", table)],
            ..Default::default()
        };
        let (mut a, mut b) = (fresh(), fresh());
        ctx.policy.step = StepPhase::DryRun;
        let dry = decide(&ctx, &mut a, &step("t", 2, true));
        ctx.policy.step = StepPhase::Commit;
        let plain = decide(&ctx, &mut b, &step("t", 2, true));
        assert!(dry.predicted.is_none());
        assert_eq!(serde_json::to_value(&dry).unwrap(), serde_json::to_value(&plain).unwrap(), "{arm:?} {observe}");
        assert_eq!(serde_json::to_value(&a).unwrap(), serde_json::to_value(&b).unwrap());
        assert_eq!((a.predictions, a.predicted_routes.clone()), (1, vec!["gen".to_string()]));
    }
}

/// T-C4-08: `allow` commits exactly as a step with no phase: the same decision and state.
#[test]
fn an_allow_verdict_commits_as_before() {
    let (dir, index, table) = predict_fixture(vec![]);
    let mut ctx = predict_ctx(&dir, &index, table, &["gen"]);
    let fresh = || SessionState {
        arm: Some(Arm::Treated),
        obligations: vec![armed("gen", "make gen", table)],
        ..Default::default()
    };
    let (mut a, mut b) = (fresh(), fresh());
    ctx.policy.step = verdict(Permission::Allow, Some("gen"));
    let allowed = decide(&ctx, &mut a, &step("t", 2, true));
    ctx.policy.step = StepPhase::Commit;
    let plain = decide(&ctx, &mut b, &step("t", 2, true));
    assert_eq!(serde_json::to_value(&allowed).unwrap(), serde_json::to_value(&plain).unwrap());
    assert_eq!(serde_json::to_value(&a).unwrap(), serde_json::to_value(&b).unwrap());
    assert_eq!((allowed.predicted_at, a.predictions), (Some(("t".into(), 2)), 1));
    assert!(allowed.withheld.is_none() && allowed.permission.is_none());
}

/// T-C4-08: any other verdict records the step as withheld with the verdict, counts against no
/// bound (neither the cap nor the in-a-row rule), routes the step as when nothing is predicted,
/// and never offers the route again this session; a different route still may be.
#[test]
fn a_verdict_other_than_allow_withholds_the_step_and_its_route() {
    let (dir, index, table) = predict_fixture(vec![]);
    let mut ctx = predict_ctx(&dir, &index, table, &["lookup", "gen"]);
    ctx.policy.predict_cap = 1;
    for permission in [Permission::Ask, Permission::Deny, Permission::None] {
        let mut st = SessionState {
            arm: Some(Arm::Treated),
            pending: vec![pending("lookup", "./scripts/wi-lookup W-2042 --json", table)],
            obligations: vec![armed("gen", "make gen", table)],
            ..Default::default()
        };
        ctx.policy.step = verdict(permission, Some("lookup"));
        let d = decide(&ctx, &mut st, &step("t", 1, false));
        assert!(d.predicted.is_none() && d.predicted_at.is_none(), "{permission:?}");
        assert_eq!(d.would_predict.as_ref().map(|p| p.route.as_str()), Some("lookup"));
        assert_eq!(d.withheld, Some(Withheld::Permission));
        let v = d.permission.clone().unwrap();
        assert_eq!((v.decision, v.tool.as_deref()), (permission, Some("Bash")));
        assert_eq!(v.reason.as_deref(), Some("This command requires approval"));
        assert_eq!((st.predictions, st.last_step_predicted.clone(), st.chained), (0, None, false));
        assert!(st.predicted_routes.is_empty());
        assert_eq!(st.withheld_routes, vec!["lookup".to_string()]);
        let rec = serde_json::to_value(&d).unwrap();
        assert_eq!(rec["withheld"], "permission");
        assert_eq!(rec["permission"]["decision"], json!(permission));
        // The very next step is not "in a row", the cap of one is untouched, and the held route
        // is not offered again: the finish line is.
        ctx.policy.step = StepPhase::Commit;
        let d = decide(&ctx, &mut st, &step("t", 1, true));
        assert_eq!(d.predicted.map(|p| p.route), Some("gen".into()));
        st.obligations[0].met = false;
        st.predicted_routes.clear();
        st.last_step_predicted = None;
        st.predictions = 0;
        st.obligations.clear();
        let d = decide(&ctx, &mut st, &step("u", 3, false));
        assert!(d.would_predict.is_none(), "the held route is not offered again this session");
    }
    assert_eq!(serde_json::to_value(Permission::None).unwrap(), json!("none"));
}

/// T-C4-08: a held step goes to the routine model as a step with nothing predicted does.
#[test]
fn a_withheld_step_is_routed_as_when_nothing_is_predicted() {
    let (dir, index, table) = predict_fixture(vec![]);
    let mut ctx = predict_ctx(&dir, &index, table, &["gen"]);
    ctx.policy.routine = true;
    ctx.policy.step = verdict(Permission::Ask, Some("gen"));
    let mut st = SessionState {
        arm: Some(Arm::Treated),
        pending: vec![pending("lookup", "./scripts/wi-lookup W-2042 --json", table)],
        obligations: vec![armed("gen", "make gen", table)],
        ..Default::default()
    };
    let d = decide(&ctx, &mut st, &step("t", 2, true));
    assert_eq!(d.withheld, Some(Withheld::Permission));
    assert_eq!(d.routine.map(|r| r.route), Some("lookup".into()), "the delivered directive with no grant");
}

/// T-C4-08: a verdict naming another route than the one found now (the plan moved since the dry
/// run) decides no prediction: nothing is committed, held or recorded as predicted.
#[test]
fn a_verdict_for_a_moved_plan_decides_no_prediction() {
    let (dir, index, table) = predict_fixture(vec![]);
    let mut ctx = predict_ctx(&dir, &index, table, &["gen"]);
    for permission in [Permission::Allow, Permission::Deny] {
        ctx.policy.step = verdict(permission, Some("lookup"));
        let mut st = SessionState {
            arm: Some(Arm::Treated),
            obligations: vec![armed("gen", "make gen", table)],
            ..Default::default()
        };
        let before = serde_json::to_value(&st).unwrap();
        let d = decide(&ctx, &mut st, &step("t", 2, true));
        assert!(d.predicted.is_none() && d.would_predict.is_none() && d.withheld.is_none(), "{permission:?}");
        assert_eq!(serde_json::to_value(&st).unwrap(), before);
    }
    // No route named: the verdict is for whatever is found.
    ctx.policy.step = verdict(Permission::Allow, None);
    let mut st = SessionState { obligations: vec![armed("gen", "make gen", table)], ..Default::default() };
    assert!(decide(&ctx, &mut st, &step("t", 2, true)).predicted.is_some());
}

/// T-C4-08: the held routes survive a rebuild from the spool; a held step counts nothing.
#[test]
fn held_routes_survive_a_rebuild() {
    let (dir, index, table) = predict_fixture(vec![]);
    let mut ctx = predict_ctx(&dir, &index, table, &["lookup"]);
    ctx.policy.step = verdict(Permission::Ask, Some("lookup"));
    let mut st = SessionState { arm: Some(Arm::Treated), repo: Some("acme".into()), ..Default::default() };
    let mut records = Vec::new();
    for ev in [Event::Prompt { text: "Pick up W-2042 please".into() }, step("t", 1, false)] {
        let d = decide(&ctx, &mut st, &ev);
        records.push(json!({ "event": "x", "kind": ev.name(), "arm": st.arm, "repo": "acme", "decision": d }));
    }
    assert_eq!(records[1]["decision"]["withheld"], "permission");
    let rebuilt = rebuild::rebuild(&records, &index, "acme", table).unwrap();
    assert_eq!(rebuilt.withheld_routes, vec!["lookup".to_string()]);
    assert_eq!((rebuilt.predictions, rebuilt.last_step_predicted, rebuilt.predicted_routes.len()), (0, None, 0));
}
