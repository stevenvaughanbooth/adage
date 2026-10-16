use super::*;

fn recovery_route() -> Route {
    route(
        "retry",
        Trigger::AfterCommand { command: command_key("pytest tests/test_a.py", CommandTable::load()) },
        Step::Advise {
            template: adage_tracks::recovery::TEMPLATE.into(),
            slots: [("command".into(), "pytest tests/test_a.py".into())].into(),
        },
        vec![],
        Effect::Observe,
        Strength::Prime,
    )
}

fn attempt(id: &str, failed: bool) -> Call {
    Call {
        tool_use_id: Some(id.into()),
        failed,
        ..bash("pytest tests/test_a.py", if failed { "1 failed, 1 passed in 0.1s" } else { "2 passed in 0.1s" })
    }
}

#[test]
fn recovery_is_evidence_gated_once_and_control_observes_without_delivering() {
    let dir = tempfile::tempdir().unwrap();
    let ix = Index::compile(&[track("acme/recovery", "desire-path", vec![recovery_route()])]);
    let ctx = Ctx {
        index: &ix,
        repo: "acme".into(),
        root: dir.path(),
        checkout: &crate::checkout::Disk,
        table: CommandTable::load(),
        embed: None,
        params: Default::default(),
        policy: Default::default(),
    };
    let red = Event::Failure { call: attempt("red", true) };
    let green = Event::Batch { calls: vec![attempt("green", false)] };
    for arm in [Arm::Treated, Arm::Control] {
        let mut st = SessionState { arm: Some(arm), ..Default::default() };
        let mut fast = st.clone();
        assert!(decide_both(&ctx, &mut st, &mut fast, &red).directives.is_empty());
        assert!(!st.recovery.is_empty());
        assert_eq!(st.recovery, fast.recovery);
        // Same failed native call can also arrive in a batch. It is not another attempt.
        decide_both(&ctx, &mut st, &mut fast, &Event::Batch { calls: vec![attempt("red", true)] });
        st = serde_json::from_str(&serde_json::to_string(&st).unwrap()).unwrap();
        let d = decide_both(&ctx, &mut st, &mut fast, &green);
        assert_eq!(d.would_have.len(), 1);
        assert_eq!(d.directives.len(), usize::from(arm == Arm::Treated));
        assert!(d.would_have[0].text.contains("without an observed repair"));
        assert!(d.would_have[0].text.contains("identity were not established"));
        assert!(st.pending.is_empty() && st.obligations.is_empty());
        assert!(decide_both(&ctx, &mut st, &mut fast, &green).directives.is_empty());
        assert!(decide_both(&ctx, &mut st, &mut fast, &Event::Stop { stop_hook_active: false })
            .continue_reason
            .is_none());
    }
    assert!(decide(&ctx, &mut SessionState::default(), &green).directives.is_empty());
}

#[test]
fn recovery_does_not_cross_repairs_unknown_activity_parallel_calls_or_task_boundaries() {
    let dir = tempfile::tempdir().unwrap();
    let ix = Index::compile(&[track("acme/recovery", "desire-path", vec![recovery_route()])]);
    let ctx = Ctx {
        index: &ix,
        repo: "acme".into(),
        root: dir.path(),
        checkout: &crate::checkout::Disk,
        table: CommandTable::load(),
        embed: None,
        params: Default::default(),
        policy: Default::default(),
    };
    for barrier in [
        Event::Batch { calls: vec![Call { tool_use_id: Some("edit".into()), ..edit_call(&dir, "src/a.py") }] },
        Event::Batch {
            calls: vec![Call { tool_use_id: Some("opaque".into()), ..bash("cat src/a.py > src/b.py", "") }],
        },
        Event::Batch { calls: vec![attempt("other-red", true), attempt("other-green", false)] },
        Event::Prompt { text: "A different task".into() },
        Event::SessionStart,
        Event::SessionEnd,
        Event::Stop { stop_hook_active: false },
    ] {
        let (mut st, mut fast) = (SessionState::default(), SessionState::default());
        decide_both(&ctx, &mut st, &mut fast, &Event::Failure { call: attempt("red", true) });
        assert!(decide_both(&ctx, &mut st, &mut fast, &barrier).directives.is_empty());
        assert!(st.recovery.is_empty());
        assert_eq!(st.recovery, fast.recovery);
        assert!(decide_both(&ctx, &mut st, &mut fast, &Event::Batch { calls: vec![attempt("green", false)] })
            .directives
            .is_empty());
    }
}
