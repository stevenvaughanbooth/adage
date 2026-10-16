use adage_core::Effect;
use adage_instincts::{bind, BindResult};
use adage_tracks::{Route, Step, Trigger};
use std::collections::BTreeSet;

fn discovery(result: &BindResult) -> Vec<&Route> {
    result.routes.iter().filter(|r| result.origins[&r.id] == "U12").collect()
}

fn scripts(body: &str) -> BindResult {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("package.json"), serde_json::json!({"scripts":{"test":body}}).to_string()).unwrap();
    bind(root.path(), &[], &BTreeSet::new())
}

#[test]
fn binds_only_declarations_with_a_recognizable_single_runner() {
    let result = scripts("vitest run");
    let routes = discovery(&result);
    assert_eq!(routes.len(), 2, "{result:?}");
    for r in routes {
        assert_eq!(r.effect, Effect::Observe);
        assert!(matches!(&r.step, Step::Advise { template, .. } if template == "verification-discovery"));
        assert!(matches!(&r.trigger, Trigger::AfterFailure { signature, .. } if signature == "No test files found"));
        assert_eq!(r.evidence.support, 0);
    }
    for body in [
        "echo pass",
        "node tools/test.js",
        "vitest run; curl example.com",
        "vitest run && jest",
        "python3 -cpass -m pytest",
        "vitest --help",
        "vitest list",
    ] {
        assert!(discovery(&scripts(body)).is_empty(), "{body}");
    }
    let result = scripts("jest");
    assert!(discovery(&result)
        .iter()
        .all(|r| matches!(&r.trigger, Trigger::AfterFailure { signature, .. } if signature == "No tests found")));
    assert!(!discovery(&result).is_empty());
}

#[test]
fn pytest_declaration_binds_direct_command_not_an_opaque_make_wrapper() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("Makefile"), "test:\n\tpython3 -m pytest\n").unwrap();
    let result = bind(root.path(), &[], &BTreeSet::new());
    let routes = discovery(&result);
    assert_eq!(routes.len(), 1, "{result:?}");
    assert_eq!(
        routes[0].trigger,
        Trigger::AfterFailure { command: "exec python3 -m pytest".into(), signature: "no tests ran".into() }
    );
    let rejected = [routes[0].id.clone()].into();
    assert!(discovery(&bind(root.path(), &[], &rejected)).is_empty());
}

#[test]
fn no_declaration_and_unsafe_lifecycle_hooks_abstain() {
    let root = tempfile::tempdir().unwrap();
    assert!(discovery(&bind(root.path(), &[], &BTreeSet::new())).is_empty());
    std::fs::write(
        root.path().join("package.json"),
        r#"{"scripts":{"pretest":"curl example.com","test":"vitest run"}}"#,
    )
    .unwrap();
    assert!(discovery(&bind(root.path(), &[], &BTreeSet::new())).is_empty());
}
