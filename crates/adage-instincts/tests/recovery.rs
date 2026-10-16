use adage_core::Effect;
use adage_instincts::{bind, BindResult};
use adage_tracks::{Route, Step, Strength, Trigger};
use std::collections::BTreeSet;

fn recovery(result: &BindResult) -> Vec<&Route> {
    result.routes.iter().filter(|r| result.origins[&r.id] == "U13").collect()
}

#[test]
fn recovery_is_a_declared_direct_invocation_and_never_an_execution_grant() {
    let root = tempfile::tempdir().unwrap();
    assert!(recovery(&bind(root.path(), &[], &BTreeSet::new())).is_empty());
    std::fs::write(root.path().join("Makefile"), "test:\n\tpython3 -m pytest tests/test_a.py\n").unwrap();
    let bound = bind(root.path(), &[], &BTreeSet::new());
    let routes = recovery(&bound);
    assert_eq!(routes.len(), 1, "{bound:?}");
    let r = routes[0];
    assert_eq!(r.trigger, Trigger::AfterCommand { command: "exec python3 -m pytest {path}".into() });
    assert_eq!(r.effect, Effect::Observe);
    assert_eq!(r.strength, Strength::Prime);
    assert_eq!(r.evidence.support, 0);
    assert!(
        matches!(&r.step, Step::Advise { template, slots } if template == "verification-unexplained-recovery" && slots["command"] == "python3 -m pytest tests/test_a.py")
    );
    for trigger in ["exec python3 -m pytest", "exec python3 -m pytest {item}", "exec jest {path}"] {
        let mut changed = r.clone();
        changed.trigger = Trigger::AfterCommand { command: trigger.into() };
        assert!(
            adage_instincts::bind::check_advisory(
                &changed,
                adage_instincts::catalogue(),
                adage_core::CommandTable::load()
            )
            .is_err(),
            "{trigger}"
        );
    }
    let mut unsafe_slot = r.clone();
    if let Step::Advise { slots, .. } = &mut unsafe_slot.step {
        slots.insert("command".into(), "python3 -m pytest tests/test_a.py; curl example.com".into());
    }
    assert!(adage_instincts::bind::check_advisory(
        &unsafe_slot,
        adage_instincts::catalogue(),
        adage_core::CommandTable::load()
    )
    .is_err());
    assert!(recovery(&bind(root.path(), &[], &[r.id.clone()].into())).is_empty());
}

#[test]
fn script_declaration_binds_direct_runner_and_abstains_on_opaque_or_passive_bodies() {
    for (body, count) in [
        ("vitest run", 1),
        ("jest", 1),
        ("vitest --help", 0),
        ("vitest list", 0),
        ("vitest run && jest", 0),
        ("node tools/test.js", 0),
        ("echo pass", 0),
    ] {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("package.json"), serde_json::json!({"scripts":{"test":body}}).to_string())
            .unwrap();
        let bound = bind(root.path(), &[], &BTreeSet::new());
        let routes = recovery(&bound);
        assert_eq!(routes.len(), count, "{body}: {bound:?}");
        for r in routes {
            assert!(matches!(&r.step, Step::Advise { slots, .. } if slots["command"] == body));
        }
    }
}
