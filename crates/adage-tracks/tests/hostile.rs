//! Regression tests for track validation gaps found by the adage-eval property tests
//! (crates/adage-eval/tests/prop_tracks.rs). Each accepted track rendered a directive outside the
//! fixed templates' promise.

use adage_core::spans::SpanKind;
use adage_core::Effect;
use adage_tracks::*;

fn track(trigger: Trigger, step: Step, slots: Vec<Slot>, effect: Effect, aspect: Aspect) -> Track {
    Track {
        manifest: Manifest {
            track: TrackInfo { name: "acme/t".into(), version: "1.0.0".into(), description: String::new(), provenance: "authored".into() },
            scope: Scope { repo: "acme".into() },
            gauge: Gauge::default(),
        },
        routes: vec![Route {
            id: "r".into(),
            summary: String::new(),
            trigger,
            step,
            slots,
            effect,
            aspect,
            state: RouteState::Active,
            exit: vec![],
            evidence: Evidence::default(),
            authored: false,
        }],
    }
}

fn run(command: &str) -> Track {
    track(Trigger::SessionStart, Step::Run { command: command.into() }, vec![], Effect::Environment, Aspect::Yellow)
}

#[test]
fn a_command_may_not_name_a_path_outside_the_repository() {
    // Validation checked only a command's characters, so `cat /etc/passwd` (observe, so even
    // green) or `cat ../../secret` could be directed by a shared track.
    for c in ["cat /etc/passwd", "cat ../../secret", "python3 /home/user/x.py", "cat acme/../../x", "make test --out=/tmp/x", "tool --in=a,../b"] {
        assert!(io::validate(&run(c)).is_err(), "{c} must be refused");
    }
    for c in ["make test", "python3 scripts/wi-lookup W-1", "cat acme/billing.py", "npm run build:prod", "curl http://localhost:8080/health"] {
        assert!(io::validate(&run(c)).is_ok(), "{c} must be allowed");
    }
}

#[test]
fn a_blank_or_padded_command_is_refused() {
    // `" "` passed validation and rendered as a directive telling the agent to run nothing.
    for c in [" ", "make  test", " make test", "make test "] {
        assert!(io::validate(&run(c)).is_err(), "{c:?} must be refused");
    }
}

#[test]
fn only_a_prompt_can_bind_a_run_steps_slots() {
    // `make {item}` after an edit rendered the placeholder itself as the directed command.
    let slot = Slot { name: "item".into(), source: SlotSource::Prompt { span: SpanKind::Ticket }, contract: Contract::Pattern { regex: r"^W-\d{4}$".into() } };
    let after_edit = track(Trigger::AfterEdit { paths: "*.py".into() }, Step::Run { command: "make {item}".into() }, vec![slot.clone()], Effect::Environment, Aspect::Yellow);
    assert!(io::validate(&after_edit).is_err());
    let prompt = track(Trigger::Prompt { span: SpanKind::Ticket }, Step::Run { command: "make {item}".into() }, vec![slot], Effect::Environment, Aspect::Yellow);
    assert!(io::validate(&prompt).is_ok());
    let avoid = track(
        Trigger::SessionStart,
        Step::Avoid { avoid: "pytest {item}".into(), instead: "make test".into(), signature: "fails".into() },
        vec![],
        Effect::Verify,
        Aspect::Red,
    );
    assert!(io::validate(&avoid).is_err(), "nothing binds an avoid step's slots");
}

#[test]
fn a_signature_may_not_carry_invisible_characters() {
    let t = |sig: &str| {
        track(
            Trigger::SessionStart,
            Step::Avoid { avoid: "python3 -m pytest".into(), instead: "make test".into(), signature: sig.into() },
            vec![],
            Effect::Verify,
            Aspect::Red,
        )
    };
    assert!(io::validate(&t("No module named pytest")).is_ok());
    // The miner names several failing runners as `a / b`; each must be safe on its own.
    let mut several = t("No module named pytest");
    several.routes[0].step = Step::Avoid { avoid: "python3 -m pytest / python -m pytest".into(), instead: "make test".into(), signature: "x".into() };
    assert!(io::validate(&several).is_ok());
    several.routes[0].step = Step::Avoid { avoid: "python3 -m pytest / cat /etc/passwd".into(), instead: "make test".into(), signature: "x".into() };
    assert!(io::validate(&several).is_err());
    for sig in ["not set\u{202e}tset ekam", "zero\u{200b}width", "bom\u{feff}"] {
        assert!(io::validate(&t(sig)).is_err(), "{sig:?} must be refused");
    }
}
