//! Intent routes.

use super::*;

fn intent_fixture(model: &str, threshold: f32) -> (tempfile::TempDir, Index, CommandTable) {
    let (dir, index, table) = fixture();
    let mut routes: Vec<Route> = index.routes.iter().map(|r| r.route.clone()).collect();
    let centroid = adage_intent::quantize(&adage_intent::unit(vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]));
    routes.push(route(
        "intent-tests",
        Trigger::Intent { family: "intent-tests".into(), model: model.into(), centroid, threshold },
        Step::Run { command: "make test".into() },
        vec![],
        Effect::Verify,
        Strength::Prime,
    ));
    let track = Track {
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
        routes,
    };
    adage_tracks::io::validate(&track).unwrap();
    (dir, Index::compile(&[track]), table)
}

#[test]
fn intent_route_fires_on_a_matching_prompt_only() {
    let (dir, index, table) = intent_fixture(MODEL, 0.8);
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
    let d = decide(&ctx, &mut st, &Event::Prompt { text: "the tests are failing again".into() });
    let hit = d.directives.iter().find(|x| x.route == "intent-tests").expect("intent directive");
    assert!(hit.text.contains("make test") && hit.text.contains("requests like this one"));
    assert_eq!(st.intent.as_deref(), Some("intent-tests"), "the family is carried in session state");
    // Following it is judged like any other directive.
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![bash("make test", "ok")] });
    assert!(d.judgements.iter().any(|j| matches!(j, Judgement::Followed { route } if route == "intent-tests")));

    let mut st = SessionState::default();
    let d = decide(&ctx, &mut st, &Event::Prompt { text: "please write the docs".into() });
    assert!(d.directives.iter().all(|x| x.route != "intent-tests"), "a different intent does not match");
    assert!(st.intent.is_none());
}

/// A card whose family's one prompt points along the first axis, at `threshold`.
fn card_fixture(threshold: f32) -> (tempfile::TempDir, Index, CommandTable) {
    use adage_tracks::card::{Card, CardFile};
    let (dir, index, table) = fixture();
    let mut routes: Vec<Route> = index.routes.iter().map(|r| r.route.clone()).collect();
    let axis = adage_intent::quantize(&adage_intent::unit(vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]));
    let card = Card {
        sessions: 2,
        passed: 2,
        files: vec![
            CardFile { path: "flag_<x>.go".into(), sessions: 2 },
            CardFile { path: "docs/flags.md".into(), sessions: 2 },
        ],
        exemplars: vec![],
        commands: vec![],
        avoid: vec![],
        finish: Some("go test ./...".into()),
        prompts: vec![axis.clone()],
    };
    routes.push(route(
        "card-1",
        Trigger::Intent { family: "card-1".into(), model: MODEL.into(), centroid: axis, threshold },
        Step::Card { card },
        vec![],
        Effect::Observe,
        Strength::Prime,
    ));
    let track = Track {
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
        routes,
    };
    adage_tracks::io::validate(&track).unwrap();
    (dir, Index::compile(&[track]), table)
}

#[test]
fn a_card_is_said_once_at_a_close_prompt_or_at_the_first_edit_of_its_files_when_nearly_close() {
    // "test" prompts score 0.97 against the family's prompt; "docs" prompts 0.
    let said = |d: &Decision| d.directives.iter().any(|x| x.route == "card-1");
    for (threshold, at_prompt) in [(0.9, true), (0.99, false)] {
        let (dir, index, table) = card_fixture(threshold);
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
        let d = decide(&ctx, &mut st, &Event::Prompt { text: "add a test flag".into() });
        assert_eq!(said(&d), at_prompt, "threshold {threshold}");
        assert_eq!(st.card_waiting.is_some(), !at_prompt, "in the grey zone the card waits");
        let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![edit_call(&dir, "src/main.go")] });
        assert!(!said(&d), "not one of the card's files");
        let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![edit_call(&dir, "flag_port.go")] });
        assert_eq!(said(&d), !at_prompt, "a file of the card's pattern: said now, unless it was said");
        let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![edit_call(&dir, "docs/flags.md")] });
        assert!(!said(&d), "once per session");
        // An unrelated prompt: nothing, and no edit brings the card.
        let mut st = SessionState::default();
        let d = decide(&ctx, &mut st, &Event::Prompt { text: "write the docs".into() });
        assert!(!said(&d) && st.card_waiting.is_none());
        let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![edit_call(&dir, "flag_port.go")] });
        assert!(!said(&d));
    }
}

#[test]
fn a_card_names_the_test_command_of_an_execution_certified_verification_unless_turned_off() {
    let (dir, index, table) = card_fixture(0.9);
    let mut index = index;
    let mut check = index.routes[0].clone();
    check.route = route(
        "check-1",
        Trigger::Context { items: vec![adage_tracks::CtxItem::Edit { glob: "flag_*.go".into() }], within: 3 },
        Step::Run { command: "go test ./...".into() },
        vec![],
        Effect::Verify,
        Strength::Prime,
    );
    check.route.evidence.execution =
        Some(adage_tracks::ExecutionEvidence { opportunities: 5, successful: 5, sessions: 5 });
    index.routes.push(check);
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
    let card = |on: bool| {
        let mut st = SessionState::default();
        let ctx = Ctx { policy: Policy { card_checks: on, ..Default::default() }, ..ctx.clone() };
        let d = decide(&ctx, &mut st, &Event::Prompt { text: "add a test flag".into() });
        d.directives.into_iter().find(|x| x.route == "card-1").unwrap().text
    };
    assert!(card(true).contains("\nTests in these sessions ran with: `go test ./...`."), "{}", card(true));
    assert!(!card(false).contains("Tests in these sessions"));
    // Without execution evidence the command is not named: only certified verifications are.
    let mut index2 = index.clone();
    index2.routes.last_mut().unwrap().route.evidence.execution = None;
    let ctx2 = Ctx {
        checkout: &crate::checkout::Disk,
        index: &index2,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: Some(&fake_embed),
        params: Default::default(),
        policy: Default::default(),
    };
    let mut st = SessionState::default();
    let d = decide(&ctx2, &mut st, &Event::Prompt { text: "add a test flag".into() });
    assert!(!d.directives.iter().any(|x| x.text.contains("Tests in these sessions")));
}

/// dev-v8: the exemplar ("copy the format") sent a third of card sessions paging and grepping for
/// its idioms. The card now says where its files stand in the checkout, read when it is said; the
/// earlier form stays available as an evaluation knob.
#[test]
fn a_card_says_where_its_files_stand_now_instead_of_quoting_an_exemplar_unless_turned_off() {
    let (dir, mut index, table) = card_fixture(0.9);
    std::fs::create_dir_all(dir.path().join("docs")).unwrap();
    std::fs::write(dir.path().join("docs/flags.md"), "# Flags\n\nSome text.\n").unwrap();
    let card = index.routes.iter_mut().find(|r| r.route.id == "card-1").unwrap();
    let Step::Card { card } = &mut card.route.step else { unreachable!() };
    card.exemplars =
        vec![adage_tracks::card::Exemplar { path: "docs/flags.md".into(), lines: vec!["## --hex".into()] }];
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
    let card = |on: bool| {
        let mut st = SessionState::default();
        let ctx = Ctx { policy: Policy { card_orientation: on, ..Default::default() }, ..ctx.clone() };
        let d = decide(&ctx, &mut st, &Event::Prompt { text: "add a test flag".into() });
        d.directives.into_iter().find(|x| x.route == "card-1").unwrap().text
    };
    let oriented = card(true);
    assert!(oriented.contains("Where those files stand in this checkout now"), "{oriented}");
    assert!(oriented.contains("  - docs/flags.md: 3 lines."), "{oriented}");
    assert!(!oriented.contains("copy the format") && !oriented.contains("## --hex"), "{oriented}");
    let earlier = card(false);
    assert!(earlier.contains("copy the format") && earlier.contains("    | ## --hex"), "{earlier}");
    assert!(!earlier.contains("Where those files stand"), "{earlier}");
}

/// The tests an edit adds to a test file are remembered by name, for the receipt to name them.
#[test]
fn edits_that_add_tests_are_remembered_for_the_receipt() {
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
    let path = |rel: &str| dir.path().join(rel).to_string_lossy().to_string();
    let call = |tool: &str, input: Value| Call {
        tool: tool.into(),
        input,
        output: String::new(),
        failed: false,
        by: By::Agent,
        tool_use_id: None,
        agent: None,
    };
    let mut st = SessionState::default();
    let batch = Event::Batch {
        calls: vec![
            call(
                "Edit",
                json!({ "file_path": path("tests/test_v.py"), "old_string": "def test_old():\n    pass\n",
                         "new_string": "def test_old():\n    pass\n\n\ndef test_finite_a():\n    pass\n\nasync def test_finite_b():\n    pass\n" }),
            ),
            call(
                "Write",
                json!({ "file_path": path("src/v.py"), "content": "def test_not_a_test_file():\n    pass\n" }),
            ),
            call(
                "MultiEdit",
                json!({ "file_path": path("tests/test_w.py"), "edits": [{ "old_string": "x", "new_string": "    def test_method(self):\n" }] }),
            ),
        ],
    };
    decide(&ctx, &mut st, &batch);
    assert_eq!(st.new_tests, ["test_finite_a", "test_finite_b", "test_method"]);
    // A failed edit adds nothing; the same name is kept once.
    let failed = Call {
        failed: true,
        ..call("Write", json!({ "file_path": path("tests/test_z.py"), "content": "def test_z(): pass\n" }))
    };
    decide(&ctx, &mut st, &Event::Batch { calls: vec![failed] });
    assert_eq!(st.new_tests.len(), 3);
    // The fast path never skips a batch that adds a test.
    let t = adage_tracks::triggers::Triggers::default();
    let quiet = || Some(SessionState::default());
    let adds = Event::Batch {
        calls: vec![call(
            "Write",
            json!({ "file_path": path("tests/test_q.py"), "content": "def test_q():\n    pass\n" }),
        )],
    };
    assert!(!crate::fast::skip(&t, &adds, &dir.path().to_string_lossy(), quiet));
}

/// Review S7: a grey-zone card waits only for the prompt that came close to it. Prompt 1 is close
/// and edits nothing of the card; prompt 2 is an unrelated task that edits a card file: no card.
#[test]
fn a_waiting_card_expires_at_the_next_prompt() {
    let said = |d: &Decision| d.directives.iter().any(|x| x.route == "card-1");
    let (dir, index, table) = card_fixture(0.99);
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
    decide(&ctx, &mut st, &Event::Prompt { text: "add a test flag".into() });
    assert!(st.card_waiting.is_some(), "grey zone: waits");
    decide(&ctx, &mut st, &Event::Batch { calls: vec![edit_call(&dir, "src/main.go")] });
    let d = decide(&ctx, &mut st, &Event::Prompt { text: "write the docs".into() });
    assert!(!said(&d) && st.card_waiting.is_none(), "an unrelated prompt ends the wait");
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![edit_call(&dir, "flag_port.go")] });
    assert!(!said(&d), "a later task's edit of a card file does not bring the card");
    // A close prompt again: it waits again, and its edit brings the card.
    decide(&ctx, &mut st, &Event::Prompt { text: "add a test flag".into() });
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![edit_call(&dir, "flag_port.go")] });
    assert!(said(&d));
}

#[test]
fn intent_routes_fail_open_without_a_model() {
    let (dir, index, table) = intent_fixture(MODEL, 0.8);
    let none = |_: &str| -> Option<(String, Vec<f32>)> { None };
    for embed in [None, Some(&none as Embed)] {
        let ctx = Ctx {
            checkout: &crate::checkout::Disk,
            index: &index,
            repo: "acme".into(),
            root: dir.path(),
            table: &table,
            embed,
            params: Default::default(),
            policy: Default::default(),
        };
        let mut st = SessionState::default();
        let d = decide(&ctx, &mut st, &Event::Prompt { text: "tests failing, W-2042".into() });
        assert!(d.directives.iter().all(|x| x.route != "intent-tests"));
        assert!(d.directives.iter().any(|x| x.route == "lookup"), "structured routes still fire");
    }
}

#[test]
fn intent_routes_need_the_same_model_and_the_threshold() {
    let (dir, index, table) = intent_fixture("other-model@000000000000", 0.8);
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
    let d = decide(&ctx, &mut SessionState::default(), &Event::Prompt { text: "tests failing".into() });
    assert!(d.directives.iter().all(|x| x.route != "intent-tests"), "a vector from another model is meaningless");
    let (dir, index, table) = intent_fixture(MODEL, 0.99);
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
    let d = decide(&ctx, &mut SessionState::default(), &Event::Prompt { text: "tests failing".into() });
    assert!(d.directives.iter().all(|x| x.route != "intent-tests"), "similarity 0.97 is below a 0.99 threshold");
}

#[test]
fn the_embedder_is_not_called_without_intent_routes() {
    let (dir, index, table) = fixture();
    let called = std::cell::Cell::new(false);
    let spy = |_: &str| -> Option<(String, Vec<f32>)> {
        called.set(true);
        None
    };
    let ctx = Ctx {
        checkout: &crate::checkout::Disk,
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: Some(&spy),
        params: Default::default(),
        policy: Default::default(),
    };
    decide(&ctx, &mut SessionState::default(), &Event::Prompt { text: "tests failing".into() });
    assert!(!called.get(), "no intent routes: the model is never loaded");
}

/// Design W2: every directive states an exit condition. A dead end's is on its own line after the
/// command; a route stored without one (older tracks) renders as before.
#[test]
fn dead_ends_and_linked_files_state_their_exit_condition() {
    let avoid = Step::Avoid { avoid: "python3 -m pytest".into(), instead: "make test".into(), signature: "x".into() };
    let mut r = route("d", Trigger::SessionStart, avoid, vec![], Effect::Verify, Strength::Inhibit);
    assert_eq!(
        render::route_text(&r, &[]).unwrap(),
        "In this repository, running python3 -m pytest fails (\"x\"). Use this instead:\n  make test\nIgnore this if the user is asking for something else."
    );
    r.exit.clear();
    assert!(render::route_text(&r, &[]).unwrap().ends_with("Use this instead:\n  make test"));
    let files = Slot {
        name: "files".into(),
        source: SlotSource::ResultField { field: "f".into() },
        contract: Contract::RepoPath,
    };
    let read = Step::ReadBatch { slot: "files".into(), max: 5 };
    let r = route("r", Trigger::SessionStart, read, vec![files], Effect::Observe, Strength::Prime);
    let text = render::route_text(&r, &[("files".into(), "a.py, b.py".into())]).unwrap();
    assert!(text.ends_with("one at a time. Ignore this if the user is asking for something else."), "{text}");
}

/// The fields a rebuild restores, as JSON (Obligation has no PartialEq).
fn restored(st: &SessionState) -> serde_json::Value {
    // `edited` and `fresh` describe the latest edit for an E2 action at that event; no later
    // decision reads them, so a rebuild need not restore them.
    let obligations: Vec<serde_json::Value> = st
        .obligations
        .iter()
        .map(|o| {
            let mut v = json!(o);
            if let Some(m) = v.as_object_mut() {
                m.remove("edited");
                m.remove("fresh");
            }
            v
        })
        .collect();
    json!({
        "arm": st.arm, "repo": st.repo, "prompts": st.prompts, "batches": st.batches,
        "emitted": st.emitted, "obligations": obligations, "history": st.history,
        "first_edit_at_batch": st.first_edit_at_batch, "stop_requests": st.stop_requests, "ctx": st.ctx,
    })
}

#[test]
fn state_rebuilt_from_the_spool_matches_the_live_state_after_every_event() {
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
    let edit = |f: &str| Call {
        tool: "Edit".into(),
        input: json!({ "file_path": dir.path().join(f).to_string_lossy() }),
        output: String::new(),
        failed: false,
        by: By::Agent,
        tool_use_id: None,
        agent: None,
    };
    let lookup_out = r#"{"id":"W-2042","linked_files":["acme/billing.py"]}"#;
    let failed = Call { failed: true, ..bash("python3 -m unittest", "RuntimeError: ACME_ENV is not set") };
    let events = vec![
        ("SessionStart", Event::SessionStart),
        ("UserPromptSubmit", Event::Prompt { text: "Pick up W-2042 please".into() }),
        ("PostToolBatch", Event::Batch { calls: vec![bash("./scripts/wi-lookup W-2042 --json", lookup_out)] }),
        ("PostToolBatch", Event::Batch { calls: vec![edit("schema/invoice.json"), bash("ls", "x")] }),
        ("PostToolUseFailure", Event::Failure { call: failed }),
        ("Stop", Event::Stop { stop_hook_active: false }),
        ("PostToolBatch", Event::Batch { calls: vec![bash("make gen", "ok")] }),
        ("Stop", Event::Stop { stop_hook_active: false }),
        ("UserPromptSubmit", Event::Prompt { text: "and W-2043".into() }),
    ];
    for control in [false, true] {
        let mut st = SessionState {
            arm: Some(if control { Arm::Control } else { Arm::Treated }),
            repo: Some("acme".into()),
            ..Default::default()
        };
        let mut records: Vec<serde_json::Value> = Vec::new();
        for (i, (name, ev)) in events.iter().enumerate() {
            let d = decide(&ctx, &mut st, ev);
            // What the hook spools; the second half of the session in the old format (no `kind`
            // or `repo`), which the rebuild maps by event name.
            let mut rec =
                json!({ "ts": i, "event": name, "arm": st.arm, "generation": index.generation, "decision": d });
            if i < 4 {
                rec["kind"] = json!(ev.name());
                rec["repo"] = json!("acme");
            }
            records.push(rec);
            let rebuilt = rebuild::rebuild(&records, &index, "acme", &table).unwrap();
            assert_eq!(restored(&rebuilt), restored(&st), "after event {i} ({name}), control {control}");
        }
        assert_eq!(st.emitted.len(), 4, "lookup, dead end, linked files, finish line");
        assert_eq!((st.stop_requests, st.obligations[0].met), (1, true));
    }
}

#[test]
fn a_rebuilt_state_never_requests_a_stop_the_live_one_would_not() {
    // A re-arming edit after the obligation was met is invisible in the spool (it keeps call keys,
    // not paths): the rebuilt obligation stays met, so it can miss a request, never add one.
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
    let edit = |f: &str| Call {
        tool: "Edit".into(),
        input: json!({ "file_path": dir.path().join(f).to_string_lossy() }),
        output: String::new(),
        failed: false,
        by: By::Agent,
        tool_use_id: None,
        agent: None,
    };
    let mut st = SessionState { arm: Some(Arm::Treated), ..Default::default() };
    let mut records = Vec::new();
    for ev in [
        Event::Batch { calls: vec![edit("schema/a.json")] },
        Event::Batch { calls: vec![bash("make gen", "ok")] },
        Event::Batch { calls: vec![edit("schema/b.json")] },
    ] {
        let d = decide(&ctx, &mut st, &ev);
        records.push(json!({ "kind": ev.name(), "arm": st.arm, "decision": d }));
    }
    let mut rebuilt = rebuild::rebuild(&records, &index, "acme", &table).unwrap();
    assert!(!st.obligations[0].met && rebuilt.obligations[0].met);
    let live = decide(&ctx, &mut st, &Event::Stop { stop_hook_active: false });
    let quiet = decide(&ctx, &mut rebuilt, &Event::Stop { stop_hook_active: false });
    assert!(live.continue_reason.is_some() && quiet.continue_reason.is_none());
}
