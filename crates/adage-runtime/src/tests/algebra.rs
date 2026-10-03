//! One command-pattern algebra.

use super::*;

#[test]
fn an_obligation_is_met_by_the_step_with_options_added() {
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
    decide(&ctx, &mut st, &Event::Batch { calls: vec![edit_call(&dir, "schema/a.json")] });
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![bash("make gen -j4", "ok")] });
    assert!(d.judgements.iter().any(|j| matches!(j, Judgement::ObligationMet { route } if route == "gen")));
    assert!(
        decide(&ctx, &mut st, &Event::Stop { stop_hook_active: false }).continue_reason.is_none(),
        "no spurious stop"
    );
    // Another target is not the step.
    let mut st = SessionState::default();
    decide(&ctx, &mut st, &Event::Batch { calls: vec![edit_call(&dir, "schema/a.json")] });
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![bash("make test", "ok")] });
    assert!(d.judgements.is_empty());
    assert!(decide(&ctx, &mut st, &Event::Stop { stop_hook_active: false }).continue_reason.is_some());
}

#[test]
fn an_after_command_trigger_fires_on_the_command_with_options_added() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("acme")).unwrap();
    std::fs::write(dir.path().join("acme/billing.py"), "x").unwrap();
    let table = CommandTable::load();
    // Mined from runs without `--json`.
    let key = command_key("./scripts/wi-lookup W-1", &table);
    let files = Slot {
        name: "files".into(),
        source: SlotSource::ResultField { field: "linked_files".into() },
        contract: Contract::RepoPath,
    };
    let linked = route(
        "read-linked",
        Trigger::AfterCommand { command: key },
        Step::ReadBatch { slot: "files".into(), max: 5 },
        vec![files],
        Effect::Observe,
        Strength::Prime,
    );
    let index = Index::compile(&[track("acme/t", "desire-path", vec![linked])]);
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
    let out = r#"{"linked_files":["acme/billing.py"]}"#;
    for cmd in ["./scripts/wi-lookup W-2042 --json", "python3 scripts/wi-lookup W-2042", "scripts/wi-lookup W-2042"] {
        let d = decide(&ctx, &mut SessionState::default(), &Event::Batch { calls: vec![bash(cmd, out)] });
        assert!(d.directives.iter().any(|x| x.route == "read-linked"), "{cmd}");
    }
    let other = Event::Batch { calls: vec![bash("./scripts/wi-other W-2042", out)] };
    assert!(decide(&ctx, &mut SessionState::default(), &other).directives.is_empty());
}

#[test]
fn a_different_script_is_not_following() {
    let dir = tempfile::tempdir().unwrap();
    let table = CommandTable::load();
    let routes = vec![intent_route("intent-check", "intent-check", "npm run check", Strength::Fire)];
    let index = Index::compile(&[track("acme/intents", "desire-path", routes)]);
    let ctx = Ctx {
        checkout: &crate::checkout::Disk,
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: Some(&fake_embed),
        params: Default::default(),
        policy: Default::default(),
    };
    let mut st = SessionState::default();
    let d = decide(&ctx, &mut st, &Event::Prompt { text: "run the tests".into() });
    assert_eq!(d.directives.len(), 1);
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![bash("npm run build", "ok")] });
    assert!(d.judgements.is_empty(), "npm run build does not follow npm run check: {:?}", d.judgements);
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![bash("npm run check --silent", "ok")] });
    assert!(d.judgements.iter().any(|j| matches!(j, Judgement::Followed { route } if route == "intent-check")));
}
