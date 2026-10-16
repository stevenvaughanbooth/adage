use super::*;

fn discovery(command: &str, signature: &str) -> Route {
    let table = CommandTable::load();
    let mut r = route(
        "empty-selection",
        Trigger::AfterFailure { command: command_key(command, table), signature: signature.into() },
        Step::Advise { template: "verification-discovery".into(), slots: [("command".into(), command.into())].into() },
        vec![],
        Effect::Observe,
        Strength::Prime,
    );
    // The binder puts instinct provenance on each route, not only its enclosing track. It
    // enables runner matching with extra test paths while retaining the discovery-specific gate.
    r.evidence = Evidence { provenance: "instinct:adage/verification-discovery@0.1.0".into(), ..Default::default() };
    r.state = RouteState::Forming;
    r
}

#[test]
fn discovery_advice_requires_the_declared_runner_and_its_own_empty_report() {
    let dir = tempfile::tempdir().unwrap();
    let index = Index::compile(&[track(
        "acme/verification",
        "instinct:adage/verification-discovery@0.1.0",
        vec![discovery("vitest run", "No test files found")],
    )]);
    assert!(index.routes[0].route.is_instinct());
    let ctx = Ctx {
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        checkout: &crate::checkout::Disk,
        table: CommandTable::load(),
        embed: None,
        params: Default::default(),
        policy: Default::default(),
    };
    let no_tests = "No test files found, exiting with code 1\ninclude: **/*.{test,spec}.js\n";
    for (command, output, want) in [
        ("vitest run", no_tests, true),
        ("vitest run tests/missing.test.js", no_tests, true),
        ("jest", no_tests, false),
        ("vitest run", "AssertionError: expected 'No test files found'\nTests  1 failed (1)\n", false),
        ("vitest run", "No test files found, exiting with code 1\nTests  1 passed (1)\n", false),
        ("vitest run", "No test files found, exiting with code 1\nTests  1 failed (1)\n", false),
        ("vitest run", "Tests  1 failed (1)\n", false),
        ("vitest run --help", no_tests, false),
        ("vitest run --config other.js", no_tests, false),
        ("vitest run --workspace other", no_tests, false),
        ("vitest run ../other/tests.js", no_tests, false),
        ("cd packages/other && vitest run", no_tests, false),
        ("echo fixture; vitest run", no_tests, false),
        ("cat test.log", no_tests, false),
    ] {
        let (mut state, mut fast_state) = (SessionState::default(), SessionState::default());
        let event = Event::Failure { call: failed_bash(command, output) };
        let d = decide_both(&ctx, &mut state, &mut fast_state, &event);
        assert_eq!(!d.directives.is_empty(), want, "{command}: {output}");
        assert!(state.pending.is_empty() && state.obligations.is_empty(), "advice does not run or require a command");
        if want {
            assert!(d.directives[0].text.contains("does not verify the change"));
            let second = decide(&ctx, &mut state, &event);
            assert!(second.directives.is_empty(), "one contextual reminder, not a retry loop");
        }
    }
    for event in [
        Event::Batch { calls: vec![bash("vitest run", "Tests  4 passed (4)\n")] },
        Event::Batch { calls: vec![bash("vitest run", no_tests)] },
        Event::Batch { calls: vec![edit_call(&dir, "src/app.js")] },
    ] {
        assert!(decide(&ctx, &mut SessionState::default(), &event).directives.is_empty());
    }
    let mut control = SessionState { arm: Some(Arm::Control), ..Default::default() };
    let d = decide(&ctx, &mut control, &Event::Failure { call: failed_bash("vitest run", no_tests) });
    assert!(d.directives.is_empty());
    assert_eq!(d.would_have.len(), 1);
}

#[test]
fn pytest_and_declared_javascript_wrapper_reports_are_supported() {
    let dir = tempfile::tempdir().unwrap();
    for (command, signature, output) in [
        ("python3 -m pytest", "no tests ran", "no tests ran in 0.01s"),
        ("npm test", "No tests found", "No tests found, exiting with code 1"),
    ] {
        let index = Index::compile(&[track(
            "acme/verification",
            "instinct:adage/verification-discovery@0.1.0",
            vec![discovery(command, signature)],
        )]);
        let ctx = Ctx {
            index: &index,
            repo: "acme".into(),
            root: dir.path(),
            checkout: &crate::checkout::Disk,
            table: CommandTable::load(),
            embed: None,
            params: Default::default(),
            policy: Default::default(),
        };
        let d = decide(&ctx, &mut SessionState::default(), &Event::Failure { call: failed_bash(command, output) });
        assert_eq!(d.directives.len(), 1, "{command}: {d:?}");
    }
}
