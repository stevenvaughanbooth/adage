//! A declared instinct's dead end replaced before it runs, as a learned firm one is (T-C6-07).

use super::*;

fn sub(index: &Index, cmd: &str) -> Option<Substitution> {
    substitution(index, "acme", &Policy::default(), CommandTable::load(), cmd)
}

/// The sandbox's dead end as `adage condition --instincts-only` binds it from its Makefile
/// (`test: ACME_ENV=test python3 -m unittest discover -s tests`): instinct U1, after the bare
/// runner fails for want of `ACME_ENV`, run `make test`; prime and forming, as every instinct is.
fn u1() -> Route {
    Route {
        id: "dead-end-d41b1e".into(),
        summary: "Instinct U1: a declared script runs this repository's test runner with the setup it needs".into(),
        trigger: Trigger::AfterFailure { command: "exec python3 -m unittest".into(), signature: "ACME_ENV".into() },
        step: Step::Run { command: "make test".into() },
        slots: vec![],
        effect: Effect::Verify,
        strength: Strength::Prime,
        state: RouteState::Forming,
        exit: vec![Exit::IntentChanges],
        evidence: Evidence {
            provenance: "instinct:adage/test-wrapper@0.1.0".into(),
            origin: Some(Origin::Instinct {
                pack: "adage/test-wrapper".into(),
                version: "0.1.0".into(),
                instinct: "U1".into(),
                ecosystem: Some("Python".into()),
                declared: Some("Makefile".into()),
            }),
            ..Default::default()
        },
        authored: false,
    }
}

fn one(r: Route) -> Index {
    unlocked(&[&track("acme/instincts", "instinct:adage/test-wrapper@0.1.0", vec![r])])
}

const BECAUSE: &str = "was replaced because this repository's Makefile declares make test in its place (a declared \
                       script runs this repository's test runner with the setup it needs). The output above is from \
                       make test.";

/// T-C6-07: the instinct's dead end, plain and at the head of a pipeline of output filters, is
/// replaced as a learned firm one is, and the line gives the instinct's declared reason (the file
/// that declares `instead`, the instinct's summary without its label) instead of a failure.
#[test]
fn an_instinct_dead_end_is_substituted_with_its_declared_reason() {
    let index = one(u1());
    let s = sub(&index, "python3 -m unittest discover -s tests").expect("the plain dead end");
    assert_eq!(
        (s.route.as_str(), s.track.as_str(), s.command.as_str()),
        ("dead-end-d41b1e", "acme/instincts", "make test")
    );
    assert_eq!(s.context, format!("adage ran make test for you; python3 -m unittest discover -s tests {BECAUSE}"));

    // Run 3's and the index-0 runs' command.
    let s = sub(&index, "python3 -m unittest discover -s tests -t . 2>&1 | tail -5").expect("the piped dead end");
    assert_eq!(s.command, "make test 2>&1 | tail -5");
    assert_eq!(
        s.context,
        format!(
            "adage ran make test for you, through the rest of your pipeline as written; python3 -m unittest discover \
             -s tests -t . {BECAUSE}"
        )
    );
    assert!(!s.context.contains('\n'));

    // The pipeline rule is the learned one's: a list or a redirection is never rewritten.
    for cmd in [
        "python3 -m unittest discover -s tests 2>&1 | head -20; ls . tests",
        "python3 -m unittest discover -s tests > out.txt",
        "python3 -m unittest | xargs echo",
        "make test",
        "python3 -m pytest tests",
    ] {
        assert!(sub(&index, cmd).is_none(), "{cmd}");
    }
}

/// T-C6-07: only an instinct in service is a replacement. Nascent (an environment step waiting
/// for approval), dormant and extinguished are not; neither is one with a slot, an `instead`
/// that is the call or the call's own tool in another environment, or one whose failure trigger
/// names another runner. An instinct that names
/// no declaring file says the build declares it; one in an `avoid` step is replaced the same way.
#[test]
fn only_an_instinct_in_service_with_a_fixed_instead_is_a_replacement() {
    let cmd = "python3 -m unittest discover -s tests";
    for state in [RouteState::Nascent, RouteState::Dormant, RouteState::Extinguished] {
        let mut r = u1();
        r.state = state;
        assert!(sub(&one(r), cmd).is_none(), "{state:?}");
    }
    let mut firm = u1();
    firm.state = RouteState::Firm;
    assert!(sub(&one(firm), cmd).is_some(), "firm is in service");

    let mut slotted = u1();
    slotted.step = Step::Run { command: "make test-{suite}".into() };
    assert!(sub(&one(slotted), cmd).is_none(), "slotted instead");
    let mut itself = u1();
    itself.step = Step::Run { command: cmd.into() };
    assert!(sub(&one(itself), cmd).is_none(), "the call itself");
    let mut other = u1();
    other.trigger = Trigger::AfterFailure { command: "exec npm test".into(), signature: "ACME_ENV".into() };
    assert!(sub(&one(other), cmd).is_none(), "another runner's failure");

    // U11's `instead` runs the call's own tool in the project's environment: the interpreter
    // rewrite's to make, with the call's arguments kept; a substitution would drop them.
    for venv in [".venv/bin/python -m pytest", "uv run pytest"] {
        let mut u11 = u1();
        u11.trigger = Trigger::AfterFailure { command: "exec python -m pytest".into(), signature: "No module".into() };
        u11.step = Step::Run { command: venv.into() };
        assert!(sub(&one(u11), "python -m pytest tests -q").is_none(), "{venv}");
    }

    let mut undeclared = u1();
    undeclared.evidence.origin = None;
    undeclared.summary = "Instinct U3: this repository declares one test command".into();
    let s = sub(&one(undeclared), cmd).unwrap();
    assert_eq!(
        s.context,
        format!(
            "adage ran make test for you; {cmd} was replaced because this repository's build declares make test in \
             its place (this repository declares one test command). The output above is from make test."
        )
    );

    let mut avoid = u1();
    avoid.trigger = Trigger::SessionStart;
    avoid.step =
        Step::Avoid { avoid: "python3 -m unittest".into(), instead: "make test".into(), signature: "ACME_ENV".into() };
    assert_eq!(sub(&one(avoid), cmd).map(|s| s.command), Some("make test".into()), "an instinct avoid step");
}

/// T-C6-07, the decision: the same as a learned route's, substituted and judged in the treated
/// arm, `would_substitute` only in the control arm and in observe mode.
#[test]
fn an_instinct_substitution_is_withheld_in_control_and_observe_mode() {
    let s = sub(&one(u1()), "python3 -m unittest discover -s tests -t . 2>&1 | tail -5").unwrap();
    let d = substitute(Some(Arm::Treated), false, s.clone());
    assert_eq!(d.substituted.as_ref(), Some(&s));
    assert!(matches!(d.judgements.as_slice(), [Judgement::Substituted { route }] if route == "dead-end-d41b1e"));
    for (arm, observe) in [(Some(Arm::Control), false), (Some(Arm::Treated), true)] {
        let d = substitute(arm, observe, s.clone());
        assert!(d.substituted.is_none() && d.judgements.is_empty());
        assert_eq!(d.would_substitute.as_ref(), Some(&s));
    }
}
