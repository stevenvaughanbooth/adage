//! The dead end that never runs.

use super::*;

/// The mod's observe mode records a step as the control arm does, in a treated session.
#[test]
fn turn_step_in_observe_mode_records_would_predict_only() {
    let (dir, index, table) = predict_fixture(vec![]);
    let mut ctx = predict_ctx(&dir, &index, table, &["lookup"]);
    ctx.policy.observe = true;
    let mut st = SessionState {
        arm: Some(Arm::Treated),
        pending: vec![pending("lookup", "./scripts/wi-lookup W-2042 --json", table)],
        ..Default::default()
    };
    let d = decide(&ctx, &mut st, &step("t", 1, false));
    assert!(d.predicted.is_none());
    assert_eq!(d.would_predict.map(|p| p.route), Some("lookup".into()));
    assert_eq!((st.arm, st.predictions), (Some(Arm::Treated), 1));
}

fn after_failure_pair(strength: Strength) -> Route {
    route(
        "after-pytest",
        Trigger::AfterFailure { command: "exec python3 -m pytest".into(), signature: "No module named pytest".into() },
        Step::Run { command: "make test".into() },
        vec![],
        Effect::Verify,
        strength,
    )
}

/// T-C6-01: a firm learned `avoid` route with a fixed `instead`, or a run after a failure at fire,
/// answers a substitution; a prime pair, an instinct, a slotted or missing `instead`, a list, or
/// the `instead` itself answer none.
#[test]
fn a_learned_dead_end_is_substituted_only_from_a_firm_fixed_pair() {
    let (_dir, index, table) = fixture();
    let policy = Policy::default();
    let sub = |index: &Index, cmd: &str| substitution(index, "acme", &policy, table, cmd);
    let s = sub(&index, "python3 -m unittest tests.test_billing -v").expect("the dead end");
    assert_eq!((s.route.as_str(), s.track.as_str(), s.command.as_str()), ("dead-unittest", "acme/t", "make test"));
    assert_eq!(
        s.context,
        "adage ran make test in place of python3 -m unittest tests.test_billing -v: in this repository it fails (ACME_ENV is not set)."
            .replace("(ACME", "(\"ACME")
            .replace("set)", "set\")")
    );
    assert!(sub(&index, "make test").is_none(), "the working command");
    assert!(sub(&index, "cd sub && python3 -m unittest").is_none(), "a list keeps what else it runs");
    assert_eq!(
        sub(&index, "python3 -m unittest | tail").map(|s| s.command),
        Some("make test | tail".into()),
        "filters are kept"
    );
    assert!(sub(&index, "ls").is_none());

    let one = |r: Route| unlocked(&[&track("acme/x", "desire-path", vec![r])]);
    let fire = one(after_failure_pair(Strength::Fire));
    assert_eq!(sub(&fire, "python3 -m pytest tests").map(|s| s.command), Some("make test".into()));
    assert!(sub(&one(after_failure_pair(Strength::Prime)), "python3 -m pytest tests").is_none(), "prime");
    let mut forming = after_failure_pair(Strength::Fire);
    forming.state = RouteState::Forming;
    assert!(sub(&one(forming), "python3 -m pytest tests").is_none(), "forming is delivered prime");
    let mut dormant = npm_test_dead_end();
    dormant.state = RouteState::Dormant;
    assert!(sub(&one(dormant), "npm test").is_none(), "dormant");
    assert_eq!(sub(&one(npm_test_dead_end()), "npm test").map(|s| s.route), Some("dead-end-npm".into()));
    let slotted = route(
        "slotted",
        Trigger::SessionStart,
        Step::Avoid { avoid: "npm test".into(), instead: "make test-{suite}".into(), signature: "x".into() },
        vec![],
        Effect::Verify,
        Strength::Inhibit,
    );
    assert!(sub(&one(slotted), "npm test").is_none(), "slotted instead");
    let empty = route(
        "empty",
        Trigger::SessionStart,
        Step::Avoid { avoid: "npm test".into(), instead: " ".into(), signature: "x".into() },
        vec![],
        Effect::Verify,
        Strength::Inhibit,
    );
    assert!(sub(&one(empty), "npm test").is_none(), "no instead");
    let mut instinct = npm_test_dead_end();
    instinct.evidence.provenance = "instinct:core@0.1.0".into();
    assert!(sub(&one(instinct), "npm test").is_none(), "an instinct is advice only");
}

/// T-C6-02, the runtime half: treated substitutes and judges it `substituted`, never a follow;
/// control and observe mode record `would_substitute` only.
#[test]
fn a_substitution_is_its_own_judgement_and_never_in_control() {
    let (_dir, index, table) = fixture();
    let s = substitution(&index, "acme", &Policy::default(), table, "python3 -m unittest").unwrap();
    let d = substitute(Some(Arm::Treated), false, s.clone());
    assert_eq!(d.substituted.as_ref(), Some(&s));
    assert!(matches!(d.judgements.as_slice(), [Judgement::Substituted { route }] if route == "dead-unittest"));
    let v = serde_json::to_value(&d).unwrap();
    assert_eq!(v["judgements"], json!([{ "kind": "substituted", "route": "dead-unittest" }]));
    for (arm, observe) in [(Some(Arm::Control), false), (Some(Arm::Treated), true)] {
        let d = substitute(arm, observe, s.clone());
        assert!(d.substituted.is_none() && d.judgements.is_empty());
        assert_eq!(d.would_substitute.as_ref(), Some(&s));
    }
    // A decision without either keeps its old bytes.
    assert!(serde_json::to_string(&Decision::default()).unwrap().find("substitut").is_none());
}

/// T-A1-03 (the ring itself): one decision per `PreToolUse` identity, at most 64 kept, the oldest
/// dropped first; a state written before the ring existed reads with it empty.
#[test]
fn the_pretool_ring_is_bounded_and_exact() {
    let mut st = SessionState::default();
    for id in 0..SEEN_PRETOOL as u64 {
        assert!(st.see_pretool(id), "{id} is new");
    }
    assert!(!st.see_pretool(0), "decided already");
    assert_eq!(st.seen_pretool.len(), SEEN_PRETOOL);
    assert!(st.see_pretool(64), "a 65th identity is decided");
    assert_eq!(st.seen_pretool.len(), SEEN_PRETOOL);
    assert!(!st.has_seen_pretool(0), "the oldest made room");
    assert!(st.see_pretool(0), "so it decides again");
    assert!(st.has_seen_pretool(64) && !st.has_seen_pretool(1));
    let old: SessionState = serde_json::from_str(r#"{"arm":null,"repo":null,"prompts":0,"batches":0,"emitted":[],"pending":[],"obligations":[],"history":[],"first_edit_at_batch":null}"#).unwrap();
    assert!(old.seen_pretool.is_empty() && old.standing.is_none());
    let json = serde_json::to_string(&SessionState::default()).unwrap();
    assert!(!json.contains("seen_pretool") && !json.contains("standing"), "{json}");
}

fn card_route(id: &str, files: &[&str]) -> Route {
    let card = card::Card {
        sessions: 4,
        passed: 3,
        files: files.iter().map(|f| card::CardFile { path: f.to_string(), sessions: 4 }).collect(),
        exemplars: vec![],
        commands: vec![],
        avoid: vec![],
        finish: None,
        prompts: vec![],
    };
    let trigger = Trigger::Intent { family: id.into(), model: "m".into(), centroid: vec![1], threshold: 0.9 };
    route(id, trigger, Step::Card { card }, vec![], Effect::Observe, Strength::Prime)
}

/// T-C5-01 (the text): the standing text is the repository's session-start reflexes in their
/// wrappers, then one line per procedure card; deterministic, whole items only, at most 2,000
/// bytes; the routes it carries are named.
#[test]
fn standing_text_carries_the_session_start_reflexes_and_the_cards_within_its_bound() {
    let (_dir, base, _table) = fixture();
    let mut routes: Vec<Route> = base.routes.iter().map(|r| r.route.clone()).collect();
    routes.push(card_route("card-currency", &["src/currency.py", "tests/test_<x>.py", "docs/a.md", "docs/b.md"]));
    let index = Index::compile(&[track("acme/t", "desire-path", routes.clone())]);
    let s = standing(&index, "acme", &Policy::default(), "env".into());
    assert_eq!(s.routes, ["dead-unittest"], "only what a session start announces is suppressed later");
    assert!(s.standing.starts_with("<adage-reflex track=\"acme/t@"), "{}", s.standing);
    assert!(s.standing.contains("route=\"dead-unittest\" strength=\"inhibit\" kind=\"avoid\">"), "{}", s.standing);
    assert!(s.standing.contains("running python3 -m unittest fails"), "{}", s.standing);
    assert!(
        s.standing.contains("changes that touch src/currency.py, tests/test_*.py, docs/a.md follow a known procedure, learned from 4 earlier sessions."),
        "{}",
        s.standing
    );
    assert_eq!(s.environment, "env");
    assert_eq!(standing(&index, "acme", &Policy::default(), "env".into()), s, "deterministic");
    assert!(standing(&index, "other", &Policy::default(), String::new()).standing.is_empty());
    // Many session-start reflexes: whole items only, within the bound.
    for i in 0..12 {
        routes.push(route(
            &format!("dead-{i}"),
            Trigger::SessionStart,
            Step::Avoid {
                avoid: format!("tool-{i} run"),
                instead: "make test".into(),
                signature: "no such tool".into(),
            },
            vec![],
            Effect::Verify,
            Strength::Inhibit,
        ));
    }
    let index = Index::compile(&[track("acme/t", "desire-path", routes)]);
    let s = standing(&index, "acme", &Policy::default(), String::new());
    assert!(s.standing.len() <= render::MAX_STANDING, "{}", s.standing.len());
    assert_eq!(s.standing.matches("<adage-reflex ").count(), s.standing.matches("</adage-reflex>").count());
    assert_eq!(
        s.standing.matches("<adage-reflex ").count(),
        s.routes.len() + usize::from(s.standing.contains("known procedure"))
    );
    assert!(s.routes.len() < 13, "not every reflex fits: {:?}", s.routes);
}

/// T-C5-01 (the suppression): once the standing text carries a reflex, the first prompt does not
/// announce it again, and the dead end it states still holds back what it avoids.
#[test]
fn a_reflex_in_the_standing_text_is_not_announced_again() {
    let (dir, index, table) = fixture();
    let ctx = Ctx {
        checkout: &crate::checkout::Disk,
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table,
        embed: None,
        params: Default::default(),
        policy: Default::default(),
    };
    let prompt = Event::Prompt { text: "Pick up W-2042 please".into() };
    let mut plain = SessionState::default();
    let d = decide(&ctx, &mut plain, &prompt);
    assert!(d.directives.iter().any(|x| x.route == "dead-unittest"));
    let mut st = SessionState {
        standing: Some(standing(&index, "acme", &Policy::default(), String::new())),
        ..Default::default()
    };
    let d = decide(&ctx, &mut st, &prompt);
    assert!(!d.directives.iter().any(|x| x.route == "dead-unittest"), "{:?}", d.directives);
    assert!(d.directives.iter().any(|x| x.route == "lookup"), "the situational directive stays");
    assert!(!st.emitted.contains(&"dead-unittest".to_string()));
    assert_eq!(st.told().filter(|r| *r == "dead-unittest").count(), 1, "told through the standing text");
}

/// T-C5-04 (the binary): a step is routine only when the mod says a routine model is set, a
/// delivered `fire` directive is pending without a `predict` grant, and the arm is treated; the
/// control arm and observe mode record `would_route` only; a predicted step is never routed.
#[test]
fn routing_is_consented_and_never_in_control() {
    let prime = route(
        "prime-run",
        Trigger::SessionStart,
        Step::Run { command: "make check".into() },
        vec![],
        Effect::Verify,
        Strength::Prime,
    );
    let (dir, index, table) = predict_fixture(vec![prime]);
    let at = |grants: &[&str], routine: bool, observe: bool| {
        let mut c = predict_ctx(&dir, &index, table, grants);
        c.policy.routine = routine;
        c.policy.observe = observe;
        c
    };
    let with = |arm: Arm, pending_routes: &[(&str, &str)]| SessionState {
        arm: Some(arm),
        pending: pending_routes.iter().map(|(r, c)| pending(r, c, table)).collect(),
        ..Default::default()
    };
    let lookup = [("lookup", "./scripts/wi-lookup W-2042 --json")];
    let mut st = with(Arm::Treated, &lookup);
    let d = decide(&at(&[], true, false), &mut st, &step("t", 1, false));
    let want = Routing { route: "lookup".into(), track: "acme/t".into() };
    assert_eq!((d.routine.clone(), d.would_route.clone()), (Some(want.clone()), Some(want.clone())));
    assert!(d.predicted.is_none() && d.would_predict.is_none());
    // Not without the person's setting, not at the turn's first request, not for a subagent.
    assert!(decide(&at(&[], false, false), &mut with(Arm::Treated, &lookup), &step("t", 1, false))
        .would_route
        .is_none());
    assert!(decide(&at(&[], true, false), &mut with(Arm::Treated, &lookup), &step("t", 0, false))
        .would_route
        .is_none());
    let sub = Event::TurnStep { turn_id: "t".into(), index: 1, model: None, agent: Some("a".into()), ending: false };
    assert!(decide(&at(&[], true, false), &mut with(Arm::Treated, &lookup), &sub).would_route.is_none());
    // Control and observe mode: recorded, never routed.
    for (arm, observe) in [(Arm::Control, false), (Arm::Treated, true)] {
        let d = decide(&at(&[], true, observe), &mut with(arm, &lookup), &step("t", 1, false));
        assert_eq!((d.routine.clone(), d.would_route.clone()), (None, Some(want.clone())), "{arm:?} {observe}");
    }
    // A granted directive is predicted, not routed.
    let d = decide(&at(&["lookup"], true, false), &mut with(Arm::Treated, &lookup), &step("t", 1, false));
    assert!(d.predicted.is_some() && d.would_route.is_none());
    // A directive below full strength, or nothing pending: no routing.
    let d =
        decide(&at(&[], true, false), &mut with(Arm::Treated, &[("prime-run", "make check")]), &step("t", 1, false));
    assert!(d.would_route.is_none());
    assert!(decide(&at(&[], true, false), &mut with(Arm::Treated, &[]), &step("t", 1, false)).would_route.is_none());
}
