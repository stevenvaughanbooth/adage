//! Interlocking.

use super::*;

/// The hostile case: a teammate's imported intent route says "start with npm test", and this
/// repository's own sessions learned that `npm test` fails. Only the warning is given.
#[test]
fn an_imported_route_a_learned_dead_end_contradicts_is_never_emitted() {
    let dir = tempfile::tempdir().unwrap();
    let table = CommandTable::load();
    let imported = track(
        "acme/intents",
        "imported:teammate",
        vec![intent_route("intent-tests", "intent-tests", "npm test", Strength::Fire)],
    );
    let learned = track("acme/dead-ends", "desire-path", vec![npm_test_dead_end()]);
    let (index, conflicts) = Index::compile_with_conflicts(&[imported.clone(), learned.clone()]);
    assert_eq!(conflicts.len(), 1, "{conflicts:?}");
    assert_eq!(
        (conflicts[0].winner.as_str(), conflicts[0].loser.as_str(), conflicts[0].resolution),
        ("dead-end-npm", "intent-tests", interlock::Resolution::Suppressed)
    );
    // The runtime holds the line too, for an index compiled without interlocking.
    let unlocked = unlocked(&[&imported, &learned]);
    for index in [&index, &unlocked] {
        let ctx = Ctx {
            checkout: &crate::checkout::Disk,
            index,
            repo: "acme".into(),
            root: dir.path(),
            table: &table,
            embed: Some(&fake_embed),
            params: Default::default(),
            policy: Default::default(),
        };
        let mut st = SessionState::default();
        let d = decide(&ctx, &mut st, &Event::Prompt { text: "the tests are failing".into() });
        let ids: Vec<&str> = d.directives.iter().map(|x| x.route.as_str()).collect();
        assert_eq!(ids, ["dead-end-npm"]);
        assert!(st.pending.is_empty(), "no follow judgement for a directive never given");
        // Nor later in the session.
        let d = decide(&ctx, &mut st, &Event::Prompt { text: "tests again".into() });
        assert!(d.directives.is_empty());
    }
}

#[test]
fn approach_locking_filters_later_directives_and_their_obligations() {
    let dir = tempfile::tempdir().unwrap();
    let table = CommandTable::load();
    let finish = route(
        "finish-line-npm",
        Trigger::AfterEdit { paths: "src/*.ts".into() },
        Step::Run { command: "npm test".into() },
        vec![],
        Effect::Verify,
        Strength::Fire,
    );
    let t = track("acme/t", "desire-path", vec![npm_test_dead_end(), finish]);
    let (index, conflicts) = Index::compile_with_conflicts(&[t]);
    assert_eq!(conflicts[0].resolution, interlock::Resolution::RuntimeFilter);
    let ctx = Ctx {
        checkout: &crate::checkout::Disk,
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: None,
        params: Default::default(),
        policy: Default::default(),
    };
    let mut st = SessionState::default();
    let d = decide(&ctx, &mut st, &Event::Prompt { text: "hello".into() });
    assert_eq!(d.directives.len(), 1);
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![edit_call(&dir, "src/a.ts")] });
    assert!(d.directives.is_empty(), "the warning locks out the step it contradicts");
    assert!(st.obligations.is_empty());
    assert!(decide(&ctx, &mut st, &Event::Stop { stop_hook_active: false }).continue_reason.is_none());
    // Without the warning in the session, the finish line holds.
    let mut st = SessionState { prompts: 1, ..Default::default() };
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![edit_call(&dir, "src/a.ts")] });
    assert_eq!(d.directives.len(), 1);
    // An inhibit and what it contradicts in one decision: only the inhibit.
    let after = || Trigger::AfterFailure { command: "exec make lint".into(), signature: "boom".into() };
    let avoid = Step::Avoid { avoid: "npm test".into(), instead: "make test".into(), signature: "x".into() };
    let inhibit = route("inhibit-after", after(), avoid, vec![], Effect::Verify, Strength::Inhibit);
    let run = Step::Run { command: "npm test".into() };
    let fire = route("fire-after", after(), run, vec![], Effect::Verify, Strength::Fire);
    let index = Index::compile(&[track("acme/t", "desire-path", vec![inhibit, fire])]);
    let ctx = Ctx {
        checkout: &crate::checkout::Disk,
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: None,
        params: Default::default(),
        policy: Default::default(),
    };
    let failed = Call { failed: true, ..bash("make lint -q", "boom") };
    let d = decide(&ctx, &mut SessionState::default(), &Event::Failure { call: failed });
    let ids: Vec<&str> = d.directives.iter().map(|x| x.route.as_str()).collect();
    assert_eq!(ids, ["inhibit-after"]);
}

#[test]
fn a_generalised_glob_matches_any_depth() {
    let dir = tempfile::tempdir().unwrap();
    let table = CommandTable::load();
    let gen = route(
        "gen",
        Trigger::AfterEdit { paths: "proto/**/*.proto".into() },
        Step::Run { command: "make gen".into() },
        vec![],
        Effect::Regenerate,
        Strength::Fire,
    );
    let index = Index::compile(&[track("acme/t", "desire-path", vec![gen])]);
    let ctx = Ctx {
        checkout: &crate::checkout::Disk,
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: None,
        params: Default::default(),
        policy: Default::default(),
    };
    for (f, hit) in [("proto/a.proto", true), ("proto/v1/deep/b.proto", true), ("other/proto/c.proto", false)] {
        let d = decide(&ctx, &mut SessionState::default(), &Event::Batch { calls: vec![edit_call(&dir, f)] });
        assert_eq!(d.directives.len(), usize::from(hit), "{f}");
    }
    let t = index.triggers("acme");
    assert!(t.call_might_match(&format!("{} Edit {{\"file_path\":\"proto/v1/deep/b.proto\"}}", dir.path().display())));
}

#[test]
fn inhibit_comes_first_under_the_caps() {
    let dir = tempfile::tempdir().unwrap();
    let table = CommandTable::load();
    let slot = || Slot {
        name: "item".into(),
        source: SlotSource::Prompt { span: SpanKind::Ticket },
        contract: Contract::Pattern { regex: r"^W-\d{4}$".into() },
    };
    let lookup = |id: &str, cmd: &str| {
        let step = Step::Run { command: cmd.into() };
        route(id, Trigger::Prompt { span: SpanKind::Ticket }, step, vec![slot()], Effect::Observe, Strength::Fire)
    };
    let routes = vec![
        lookup("a", "scripts/a {item}"),
        lookup("b", "scripts/b {item}"),
        lookup("c", "scripts/c {item}"),
        npm_test_dead_end(),
    ];
    // Without interlocking the three lookups stay fire and compete for the cap.
    let index = unlocked(&[&track("acme/t", "desire-path", routes)]);
    let ctx = Ctx {
        checkout: &crate::checkout::Disk,
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: None,
        params: Default::default(),
        policy: Default::default(),
    };
    let d = decide(&ctx, &mut SessionState::default(), &Event::Prompt { text: "W-2042".into() });
    assert_eq!(d.directives.len(), MAX_DIRECTIVES_PER_DECISION);
    assert_eq!(d.directives.last().unwrap().route, "dead-end-npm", "the warning is kept, and still rendered last");
}

#[test]
fn propensity_is_the_probability_of_the_arm_given() {
    let (mut control, mut treated) = (0, 0);
    for i in 0..400 {
        let sid = format!("s{i}");
        let (arm, p) = assign(&sid, 0.1, None);
        assert_eq!(arm, assign_arm(&sid, 0.1), "the same draw as before");
        match arm {
            Arm::Control => {
                control += 1;
                assert_eq!(p, 0.1);
            }
            Arm::Treated => {
                treated += 1;
                assert!((p - 0.9).abs() < 1e-12);
            }
        }
    }
    assert!(control > 10 && treated > 300, "{control} held out of 400");
    // Forced arms were not drawn; other override values are ignored.
    assert_eq!(assign("s1", 0.1, Some("control")), (Arm::Control, 1.0));
    assert_eq!(assign("s1", 0.1, Some("treated")), (Arm::Treated, 1.0));
    assert_eq!(assign("s1", 0.1, Some("bogus")), assign("s1", 0.1, None));
    // Shadow mode (a share of 1 or more) holds every session out, certainly.
    assert_eq!(assign("s1", 1.5, None), (Arm::Control, 1.0));
    assert_eq!(assign("s1", 0.0, None), (Arm::Treated, 1.0));
    // A state rebuilt from the spool keeps it.
    let recs = vec![json!({ "kind": "prompt", "arm": "control", "propensity": 0.1, "decision": Decision::default() })];
    let st = rebuild::rebuild(&recs, &Index::default(), "acme", &CommandTable::load()).unwrap();
    assert_eq!((st.arm, st.propensity), (Some(Arm::Control), Some(0.1)));
    // Old spools and states, without it, still read.
    let old = vec![json!({ "kind": "prompt", "arm": "treated", "decision": Decision::default() })];
    assert_eq!(rebuild::rebuild(&old, &Index::default(), "acme", &CommandTable::load()).unwrap().propensity, None);
    let st: SessionState =
        serde_json::from_value(json!({ "arm": "treated", "repo": "acme", "prompts": 1, "batches": 0,
        "emitted": [], "pending": [], "obligations": [], "history": [], "first_edit_at_batch": null }))
        .unwrap();
    assert_eq!(st.propensity, None);
}

#[test]
fn finish_line_claims_ci_only_on_evidence() {
    let mut r = route(
        "gen",
        Trigger::AfterEdit { paths: "schema/*.json".into() },
        Step::Run { command: "make gen".into() },
        vec![],
        Effect::Regenerate,
        Strength::Fire,
    );
    r.exit = vec![Exit::ExploratoryChange];
    // No session is known to have failed for skipping it: say what agents do, nothing more.
    let t = render::route_text(&r, &[]).unwrap();
    assert_eq!(
        t,
        "In this repository, changes to files matching schema/*.json must be followed by running:\n  make gen\n\
         Agents in this repository run it after such changes. Ignore this if this is an exploratory or draft change."
    );
    assert!(!t.contains("CI fails"));
    // Sessions that skipped it failed: the claim is earned.
    r.evidence.deviations_failed = 2;
    let t = render::route_text(&r, &[]).unwrap();
    assert_eq!(
        t,
        "In this repository, changes to files matching schema/*.json must be followed by running:\n  make gen\n\
         CI fails if this is skipped. Ignore this if this is an exploratory or draft change."
    );
}

#[test]
fn directives_carry_their_shape_and_stale_routes_are_never_fire() {
    let (dir, index, table) = fixture();
    let ctx = Ctx {
        checkout: &crate::checkout::Disk,
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: None,
        params: Default::default(),
        policy: Default::default(),
    };
    let mut st = SessionState::default();
    let d = decide(&ctx, &mut st, &Event::Prompt { text: "Pick up W-2042".into() });
    let kinds: Vec<(&str, &str)> = d.directives.iter().map(|x| (x.route.as_str(), x.kind.as_str())).collect();
    assert_eq!(kinds, vec![("lookup", "run"), ("dead-unittest", "avoid")]);
    let wrapped = render::wrap_directive(&d.directives[0]);
    assert!(
        wrapped.starts_with("<adage-reflex track=\"acme/t@0.1.0\" route=\"lookup\" strength=\"fire\" kind=\"run\">\n"),
        "{wrapped}"
    );
    // Without a kind (a preview, an old record), the wrapper is as it always was.
    assert!(render::wrap("acme/t", "0.1.0", "lookup", Strength::Fire, "x")
        .starts_with("<adage-reflex track=\"acme/t@0.1.0\" route=\"lookup\" strength=\"fire\">\n"));
    let old: Directive = serde_json::from_value(
        json!({ "route": "lookup", "track": "acme/t", "version": "0.1.0", "strength": "fire", "text": "x" }),
    )
    .unwrap();
    assert_eq!(old.kind, "");

    // A dormant fire route (and one on forming) is delivered prime; inhibit stays inhibit.
    for state in [RouteState::Dormant, RouteState::Forming] {
        let t = Track {
            manifest: Manifest {
                track: TrackInfo {
                    name: "acme/t".into(),
                    version: "0.1.0".into(),
                    description: String::new(),
                    provenance: "desire-path".into(),
                },
                scope: Scope { repo: "acme".into() },
                gauge: Gauge::default(),
            },
            routes: index.routes.iter().map(|r| Route { state, ..r.route.clone() }).collect(),
        };
        let index = Index::compile(&[t]);
        assert_eq!(index.routes.len(), 4, "{state:?} routes are compiled");
        let ctx = Ctx {
            checkout: &crate::checkout::Disk,
            index: &index,
            repo: "acme".into(),
            root: dir.path(),
            table: &table,
            embed: None,
            params: Default::default(),
            policy: Default::default(),
        };
        let mut st = SessionState::default();
        let d = decide(&ctx, &mut st, &Event::Prompt { text: "Pick up W-2042".into() });
        let strengths: Vec<Strength> = d.directives.iter().map(|x| x.strength).collect();
        assert_eq!(strengths, vec![Strength::Prime, Strength::Inhibit], "{state:?}");
    }
}
