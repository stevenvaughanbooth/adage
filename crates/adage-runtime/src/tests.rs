use super::*;
use adage_core::Effect;
use adage_tracks::*;
use serde_json::json;

fn route(id: &str, trigger: Trigger, step: Step, slots: Vec<Slot>, effect: Effect, aspect: Aspect) -> Route {
    Route {
        id: id.into(),
        summary: String::new(),
        trigger,
        step,
        slots,
        effect,
        aspect,
        state: RouteState::Active,
        exit: vec![Exit::IntentChanges],
        evidence: Evidence { support: 5, ..Default::default() },
        authored: false,
    }
}

fn fixture() -> (tempfile::TempDir, Index, CommandTable) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("acme")).unwrap();
    std::fs::write(dir.path().join("acme/billing.py"), "x").unwrap();
    let table = CommandTable::load();
    let lookup_key = command_key("./scripts/wi-lookup W-1 --json", &table);
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
        routes: vec![
            route(
                "lookup",
                Trigger::Prompt { span: SpanKind::Ticket },
                Step::Run { command: "./scripts/wi-lookup {item} --json".into() },
                vec![Slot {
                    name: "item".into(),
                    source: SlotSource::Prompt { span: SpanKind::Ticket },
                    contract: Contract::Pattern { regex: r"^W-\d{4}$".into() },
                }],
                Effect::Observe,
                Aspect::Green,
            ),
            route(
                "read-linked",
                Trigger::AfterCommand { command: lookup_key },
                Step::ReadBatch { slot: "files".into(), max: 5 },
                vec![Slot {
                    name: "files".into(),
                    source: SlotSource::ResultField { field: "linked_files".into() },
                    contract: Contract::RepoPath,
                }],
                Effect::Observe,
                Aspect::Yellow,
            ),
            route(
                "gen",
                Trigger::AfterEdit { paths: "schema/*.json".into() },
                Step::Run { command: "make gen".into() },
                vec![],
                Effect::Regenerate,
                Aspect::Green,
            ),
            route(
                "dead-unittest",
                Trigger::SessionStart,
                Step::Avoid {
                    avoid: "python3 -m unittest".into(),
                    instead: "make test".into(),
                    signature: "ACME_ENV is not set".into(),
                },
                vec![],
                Effect::Verify,
                Aspect::Red,
            ),
        ],
    };
    (dir, Index::compile(&[track]), table)
}

fn bash(cmd: &str, out: &str) -> Call {
    Call { tool: "Bash".into(), input: json!({ "command": cmd }), output: out.into(), failed: false }
}

#[test]
fn prompt_binds_validated_ticket_and_warns_about_dead_end() {
    let (dir, index, table) = fixture();
    let ctx = Ctx {
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: None,
        params: Default::default(),
    };
    let mut st = SessionState::default();
    let d = decide(&ctx, &mut st, &Event::Prompt { text: "Pick up W-2042 please".into() });
    assert_eq!(d.directives.len(), 2);
    assert!(d.directives[0].text.contains("./scripts/wi-lookup W-2042 --json"));
    assert!(d.directives.iter().any(|x| x.aspect == Aspect::Red && x.text.contains("make test")));
}

#[test]
fn injected_values_never_bind() {
    let (dir, index, table) = fixture();
    let ctx = Ctx {
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: None,
        params: Default::default(),
    };
    let mut st = SessionState::default();
    let d = decide(&ctx, &mut st, &Event::Prompt { text: "Do W-20421234 and --config=/etc".into() });
    assert!(d.directives.iter().all(|x| x.route != "lookup"), "a value failing the contract must not bind");
}

#[test]
fn follow_judgement_and_linked_files() {
    let (dir, index, table) = fixture();
    let ctx = Ctx {
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: None,
        params: Default::default(),
    };
    let mut st = SessionState::default();
    decide(&ctx, &mut st, &Event::Prompt { text: "W-2042".into() });
    let out = r#"{"id":"W-2042","linked_files":["acme/billing.py","../etc/passwd","missing.py"]}"#;
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![bash("./scripts/wi-lookup W-2042 --json", out)] });
    assert!(d.judgements.iter().any(|j| matches!(j, Judgement::Followed { route } if route == "lookup")));
    let read = d.directives.iter().find(|x| x.route == "read-linked").expect("read-batch directive");
    assert!(read.text.contains("acme/billing.py"));
    assert!(!read.text.contains("passwd") && !read.text.contains("missing.py"), "unsafe or missing paths are dropped");
}

#[test]
fn obligation_triggers_on_edit_and_is_enforced_once_at_stop() {
    let (dir, index, table) = fixture();
    let ctx = Ctx {
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: None,
        params: Default::default(),
    };
    let mut st = SessionState::default();
    let edit = Call {
        tool: "Edit".into(),
        input: json!({ "file_path": dir.path().join("schema/invoice.json").to_string_lossy() }),
        output: String::new(),
        failed: false,
    };
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![edit] });
    assert!(d.directives.iter().any(|x| x.route == "gen" && x.text.contains("make gen")));
    let s1 = decide(&ctx, &mut st, &Event::Stop { stop_hook_active: false });
    assert!(s1.continue_reason.unwrap().contains("make gen"));
    let s2 = decide(&ctx, &mut st, &Event::Stop { stop_hook_active: false });
    assert!(s2.continue_reason.is_none(), "at most once per obligation");
}

#[test]
fn obligation_met_means_no_stop_request() {
    let (dir, index, table) = fixture();
    let ctx = Ctx {
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: None,
        params: Default::default(),
    };
    let mut st = SessionState::default();
    let edit = Call {
        tool: "Write".into(),
        input: json!({ "file_path": dir.path().join("schema/a.json").to_string_lossy() }),
        output: String::new(),
        failed: false,
    };
    decide(&ctx, &mut st, &Event::Batch { calls: vec![edit] });
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![bash("make gen", "ok")] });
    assert!(d.judgements.iter().any(|j| matches!(j, Judgement::ObligationMet { .. })));
    assert!(decide(&ctx, &mut st, &Event::Stop { stop_hook_active: false }).continue_reason.is_none());
}

#[test]
fn holdout_logs_exposure_but_emits_nothing() {
    let (dir, index, table) = fixture();
    let ctx = Ctx {
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: None,
        params: Default::default(),
    };
    let mut st = SessionState { arm: Some(Arm::Holdout), ..Default::default() };
    let d = decide(&ctx, &mut st, &Event::Prompt { text: "W-2042".into() });
    assert!(d.directives.is_empty());
    assert!(!d.would_have.is_empty());
}

#[test]
fn other_repos_get_nothing() {
    let (dir, index, table) = fixture();
    let ctx = Ctx {
        index: &index,
        repo: "other".into(),
        root: dir.path(),
        table: &table,
        embed: None,
        params: Default::default(),
    };
    let mut st = SessionState::default();
    assert!(decide(&ctx, &mut st, &Event::Prompt { text: "W-2042".into() }).directives.is_empty());
}

#[test]
fn caps_hold() {
    let (dir, index, table) = fixture();
    let ctx = Ctx {
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: None,
        params: Default::default(),
    };
    let mut st = SessionState::default();
    for _ in 0..20 {
        decide(&ctx, &mut st, &Event::Prompt { text: "W-2042".into() });
    }
    assert!(st.emitted.len() <= MAX_DIRECTIVES_PER_SESSION);
    assert_eq!(st.emitted.iter().filter(|e| *e == "lookup").count(), 1, "each route at most once per session");
}

#[test]
fn arm_assignment_is_deterministic() {
    assert_eq!(assign_arm("abc", 0.5), assign_arm("abc", 0.5));
    assert_eq!(assign_arm("abc", 0.0), Arm::Treated);
    assert_eq!(assign_arm("abc", 1.0), Arm::Holdout);
}

#[test]
fn linked_files_from_plain_text_output() {
    let (dir, index, table) = fixture();
    let ctx = Ctx {
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: None,
        params: Default::default(),
    };
    let mut st = SessionState::default();
    let out = "ID: W-2042\nLinked files: acme/billing.py, ../etc/passwd\nAcceptance: ok";
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![bash("./scripts/wi-lookup W-2042 --json", out)] });
    let read = d.directives.iter().find(|x| x.route == "read-linked").expect("read-batch from text output");
    assert!(read.text.contains("acme/billing.py") && !read.text.contains("passwd"));
}

#[cfg(unix)] // Windows symlinks need a privilege CI runners do not grant.
#[test]
fn symlinks_out_of_the_repo_are_rejected() {
    let (dir, index, table) = fixture();
    std::os::unix::fs::symlink("/etc/hosts", dir.path().join("acme/notes.md")).unwrap();
    let ctx = Ctx {
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: None,
        params: Default::default(),
    };
    let mut st = SessionState::default();
    let out = r#"{"linked_files":["acme/notes.md","acme/billing.py"]}"#;
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![bash("./scripts/wi-lookup W-2042 --json", out)] });
    let read = d.directives.iter().find(|x| x.route == "read-linked").unwrap();
    assert!(!read.text.contains("notes.md") && read.text.contains("acme/billing.py"));
}

#[test]
fn obligations_rearm_after_a_new_edit_and_stop_requests_are_bounded() {
    let (dir, index, table) = fixture();
    let ctx = Ctx {
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: None,
        params: Default::default(),
    };
    let mut st = SessionState::default();
    let edit = |f: &str| Call {
        tool: "Edit".into(),
        input: json!({ "file_path": dir.path().join(f).to_string_lossy() }),
        output: String::new(),
        failed: false,
    };
    decide(&ctx, &mut st, &Event::Batch { calls: vec![edit("schema/a.json")] });
    decide(&ctx, &mut st, &Event::Batch { calls: vec![bash("make gen", "ok")] });
    decide(&ctx, &mut st, &Event::Batch { calls: vec![edit("schema/b.json")] });
    assert!(decide(&ctx, &mut st, &Event::Stop { stop_hook_active: false }).continue_reason.is_some(), "re-armed");
    for _ in 0..5 {
        decide(&ctx, &mut st, &Event::Batch { calls: vec![bash("make gen", "ok")] });
        decide(&ctx, &mut st, &Event::Batch { calls: vec![edit("schema/c.json")] });
        decide(&ctx, &mut st, &Event::Stop { stop_hook_active: false });
    }
    assert!(st.stop_requests <= MAX_STOP_REQUESTS);
}

#[test]
fn hostile_output_does_not_panic() {
    let (dir, index, table) = fixture();
    let ctx = Ctx {
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: None,
        params: Default::default(),
    };
    let mut st = SessionState::default();
    let calls = vec![
        bash("./scripts/wi-lookup W-2042 --json", "} {"),
        bash("sed -n 99999999999999999999,2p a.py", ""),
        bash(&"$(echo ".repeat(20_000), ""),
        bash(&"x".repeat(100_000), ""),
    ];
    let _ = decide(&ctx, &mut st, &Event::Batch { calls });
    assert!(serde_json::to_vec(&st).unwrap().len() < 64 * 1024, "state stays within its slot");
}

#[test]
fn holdout_records_would_continue() {
    let (dir, index, table) = fixture();
    let ctx = Ctx {
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: None,
        params: Default::default(),
    };
    let mut st = SessionState::default();
    let edit = Call {
        tool: "Edit".into(),
        input: json!({ "file_path": dir.path().join("schema/a.json").to_string_lossy() }),
        output: String::new(),
        failed: false,
    };
    decide(&ctx, &mut st, &Event::Batch { calls: vec![edit] });
    st.arm = Some(Arm::Holdout);
    let d = decide(&ctx, &mut st, &Event::Stop { stop_hook_active: false });
    assert!(d.continue_reason.is_none() && d.would_continue.is_some());
}

// ---------------------------------------------------------------- intent routes

const MODEL: &str = "fixture@000000000000";

/// A stand-in embedder: "test" prompts point one way, "docs" prompts another, the rest a third.
fn fake_embed(text: &str) -> Option<(String, Vec<f32>)> {
    let t = text.to_lowercase();
    let v = if t.contains("test") {
        vec![0.8, 0.2, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
    } else if t.contains("docs") {
        vec![0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0]
    } else if t.is_empty() {
        return None;
    } else {
        vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]
    };
    Some((MODEL.to_string(), adage_intent::unit(v)))
}

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
        Aspect::Yellow,
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
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: Some(&fake_embed),
        params: Default::default(),
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

#[test]
fn intent_routes_fail_open_without_a_model() {
    let (dir, index, table) = intent_fixture(MODEL, 0.8);
    let none = |_: &str| -> Option<(String, Vec<f32>)> { None };
    for embed in [None, Some(&none as Embed)] {
        let ctx = Ctx {
            index: &index,
            repo: "acme".into(),
            root: dir.path(),
            table: &table,
            embed,
            params: Default::default(),
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
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: Some(&fake_embed),
        params: Default::default(),
    };
    let d = decide(&ctx, &mut SessionState::default(), &Event::Prompt { text: "tests failing".into() });
    assert!(d.directives.iter().all(|x| x.route != "intent-tests"), "a vector from another model is meaningless");
    let (dir, index, table) = intent_fixture(MODEL, 0.99);
    let ctx = Ctx {
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: Some(&fake_embed),
        params: Default::default(),
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
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: Some(&spy),
        params: Default::default(),
    };
    decide(&ctx, &mut SessionState::default(), &Event::Prompt { text: "tests failing".into() });
    assert!(!called.get(), "no intent routes: the model is never loaded");
}

/// Design W2: every directive states an exit condition. A dead end's is on its own line after the
/// command; a route stored without one (older tracks) renders as before.
#[test]
fn dead_ends_and_linked_files_state_their_exit_condition() {
    let avoid = Step::Avoid { avoid: "python3 -m pytest".into(), instead: "make test".into(), signature: "x".into() };
    let mut r = route("d", Trigger::SessionStart, avoid, vec![], Effect::Verify, Aspect::Red);
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
    let r = route("r", Trigger::SessionStart, read, vec![files], Effect::Observe, Aspect::Yellow);
    let text = render::route_text(&r, &[("files".into(), "a.py, b.py".into())]).unwrap();
    assert!(text.ends_with("one at a time. Ignore this if the user is asking for something else."), "{text}");
}

/// The fields a rebuild restores, as JSON (Obligation has no PartialEq).
fn restored(st: &SessionState) -> serde_json::Value {
    json!({
        "arm": st.arm, "repo": st.repo, "prompts": st.prompts, "batches": st.batches,
        "emitted": st.emitted, "obligations": st.obligations, "history": st.history,
        "first_edit_at_batch": st.first_edit_at_batch, "stop_requests": st.stop_requests,
    })
}

#[test]
fn state_rebuilt_from_the_spool_matches_the_live_state_after_every_event() {
    let (dir, index, table) = fixture();
    let ctx = Ctx {
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: None,
        params: Default::default(),
    };
    let edit = |f: &str| Call {
        tool: "Edit".into(),
        input: json!({ "file_path": dir.path().join(f).to_string_lossy() }),
        output: String::new(),
        failed: false,
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
    for holdout in [false, true] {
        let mut st = SessionState {
            arm: Some(if holdout { Arm::Holdout } else { Arm::Treated }),
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
            assert_eq!(restored(&rebuilt), restored(&st), "after event {i} ({name}), holdout {holdout}");
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
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: None,
        params: Default::default(),
    };
    let edit = |f: &str| Call {
        tool: "Edit".into(),
        input: json!({ "file_path": dir.path().join(f).to_string_lossy() }),
        output: String::new(),
        failed: false,
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

// ---------------------------------------------------------------- one command-pattern algebra

fn track(name: &str, provenance: &str, routes: Vec<Route>) -> Track {
    Track {
        manifest: Manifest {
            track: TrackInfo {
                name: name.into(),
                version: "1.0.0".into(),
                description: String::new(),
                provenance: provenance.into(),
            },
            scope: Scope { repo: "acme".into() },
            gauge: Gauge::default(),
        },
        routes,
    }
}

fn edit_call(dir: &tempfile::TempDir, f: &str) -> Call {
    Call {
        tool: "Edit".into(),
        input: json!({ "file_path": dir.path().join(f).to_string_lossy() }),
        output: String::new(),
        failed: false,
    }
}

#[test]
fn an_obligation_is_met_by_the_step_with_options_added() {
    let (dir, index, table) = fixture();
    let ctx = Ctx {
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: None,
        params: Default::default(),
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
        Aspect::Yellow,
    );
    let index = Index::compile(&[track("acme/t", "desire-path", vec![linked])]);
    let ctx = Ctx {
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: None,
        params: Default::default(),
    };
    let out = r#"{"linked_files":["acme/billing.py"]}"#;
    for cmd in ["./scripts/wi-lookup W-2042 --json", "python3 scripts/wi-lookup W-2042", "scripts/wi-lookup W-2042"] {
        let d = decide(&ctx, &mut SessionState::default(), &Event::Batch { calls: vec![bash(cmd, out)] });
        assert!(d.directives.iter().any(|x| x.route == "read-linked"), "{cmd}");
    }
    let other = Event::Batch { calls: vec![bash("./scripts/wi-other W-2042", out)] };
    assert!(decide(&ctx, &mut SessionState::default(), &other).directives.is_empty());
}

fn intent_route(id: &str, family: &str, command: &str, aspect: Aspect) -> Route {
    let centroid = adage_intent::quantize(&adage_intent::unit(vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]));
    route(
        id,
        Trigger::Intent { family: family.into(), model: MODEL.into(), centroid, threshold: 0.8 },
        Step::Run { command: command.into() },
        vec![],
        Effect::Verify,
        aspect,
    )
}

#[test]
fn a_different_script_is_not_following() {
    let dir = tempfile::tempdir().unwrap();
    let table = CommandTable::load();
    let routes = vec![intent_route("intent-check", "intent-check", "npm run check", Aspect::Green)];
    let index = Index::compile(&[track("acme/intents", "desire-path", routes)]);
    let ctx = Ctx {
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: Some(&fake_embed),
        params: Default::default(),
    };
    let mut st = SessionState::default();
    let d = decide(&ctx, &mut st, &Event::Prompt { text: "run the tests".into() });
    assert_eq!(d.directives.len(), 1);
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![bash("npm run build", "ok")] });
    assert!(d.judgements.is_empty(), "npm run build does not follow npm run check: {:?}", d.judgements);
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![bash("npm run check --silent", "ok")] });
    assert!(d.judgements.iter().any(|j| matches!(j, Judgement::Followed { route } if route == "intent-check")));
}

// ---------------------------------------------------------------- interlocking

fn npm_test_dead_end() -> Route {
    route(
        "dead-end-npm",
        Trigger::SessionStart,
        Step::Avoid { avoid: "npm test".into(), instead: "make test".into(), signature: "ACME_ENV is not set".into() },
        vec![],
        Effect::Verify,
        Aspect::Red,
    )
}

/// An index of these routes with no interlocking applied (as an index compiled before it was).
fn unlocked(tracks: &[&Track]) -> Index {
    Index {
        routes: tracks
            .iter()
            .flat_map(|t| {
                t.routes.iter().map(|r| IndexedRoute {
                    repo: "acme".into(),
                    track: t.manifest.track.name.clone(),
                    version: "1.0.0".into(),
                    route: r.clone(),
                    exec: None,
                })
            })
            .collect(),
        ..Default::default()
    }
}

/// The hostile case: a teammate's imported intent route says "start with npm test", and this
/// repository's own sessions learned that `npm test` fails. Only the warning is given.
#[test]
fn an_imported_route_a_learned_dead_end_contradicts_is_never_emitted() {
    let dir = tempfile::tempdir().unwrap();
    let table = CommandTable::load();
    let imported = track(
        "acme/intents",
        "imported:teammate",
        vec![intent_route("intent-tests", "intent-tests", "npm test", Aspect::Green)],
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
            index,
            repo: "acme".into(),
            root: dir.path(),
            table: &table,
            embed: Some(&fake_embed),
            params: Default::default(),
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
        Aspect::Green,
    );
    let t = track("acme/t", "desire-path", vec![npm_test_dead_end(), finish]);
    let (index, conflicts) = Index::compile_with_conflicts(&[t]);
    assert_eq!(conflicts[0].resolution, interlock::Resolution::RuntimeFilter);
    let ctx = Ctx {
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: None,
        params: Default::default(),
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
    // A red and what it contradicts in one decision: only the red.
    let after = || Trigger::AfterFailure { command: "exec make lint".into(), signature: "boom".into() };
    let avoid = Step::Avoid { avoid: "npm test".into(), instead: "make test".into(), signature: "x".into() };
    let red = route("red-after", after(), avoid, vec![], Effect::Verify, Aspect::Red);
    let run = Step::Run { command: "npm test".into() };
    let green = route("green-after", after(), run, vec![], Effect::Verify, Aspect::Green);
    let index = Index::compile(&[track("acme/t", "desire-path", vec![red, green])]);
    let ctx = Ctx {
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: None,
        params: Default::default(),
    };
    let failed = Call { failed: true, ..bash("make lint -q", "boom") };
    let d = decide(&ctx, &mut SessionState::default(), &Event::Failure { call: failed });
    let ids: Vec<&str> = d.directives.iter().map(|x| x.route.as_str()).collect();
    assert_eq!(ids, ["red-after"]);
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
        Aspect::Green,
    );
    let index = Index::compile(&[track("acme/t", "desire-path", vec![gen])]);
    let ctx = Ctx {
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: None,
        params: Default::default(),
    };
    for (f, hit) in [("proto/a.proto", true), ("proto/v1/deep/b.proto", true), ("other/proto/c.proto", false)] {
        let d = decide(&ctx, &mut SessionState::default(), &Event::Batch { calls: vec![edit_call(&dir, f)] });
        assert_eq!(d.directives.len(), usize::from(hit), "{f}");
    }
    let t = index.triggers("acme");
    assert!(t.call_might_match(&format!("{} Edit {{\"file_path\":\"proto/v1/deep/b.proto\"}}", dir.path().display())));
}

#[test]
fn red_comes_first_under_the_caps() {
    let dir = tempfile::tempdir().unwrap();
    let table = CommandTable::load();
    let slot = || Slot {
        name: "item".into(),
        source: SlotSource::Prompt { span: SpanKind::Ticket },
        contract: Contract::Pattern { regex: r"^W-\d{4}$".into() },
    };
    let lookup = |id: &str, cmd: &str| {
        let step = Step::Run { command: cmd.into() };
        route(id, Trigger::Prompt { span: SpanKind::Ticket }, step, vec![slot()], Effect::Observe, Aspect::Green)
    };
    let routes = vec![
        lookup("a", "scripts/a {item}"),
        lookup("b", "scripts/b {item}"),
        lookup("c", "scripts/c {item}"),
        npm_test_dead_end(),
    ];
    // Without interlocking the three lookups stay green and compete for the cap.
    let index = unlocked(&[&track("acme/t", "desire-path", routes)]);
    let ctx = Ctx {
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: None,
        params: Default::default(),
    };
    let d = decide(&ctx, &mut SessionState::default(), &Event::Prompt { text: "W-2042".into() });
    assert_eq!(d.directives.len(), MAX_DIRECTIVES_PER_DECISION);
    assert_eq!(d.directives.last().unwrap().route, "dead-end-npm", "the warning is kept, and still rendered last");
}
#[test]
fn propensity_is_the_probability_of_the_arm_given() {
    let (mut holdout, mut treated) = (0, 0);
    for i in 0..400 {
        let sid = format!("s{i}");
        let (arm, p) = assign(&sid, 0.1, None);
        assert_eq!(arm, assign_arm(&sid, 0.1), "the same draw as before");
        match arm {
            Arm::Holdout => {
                holdout += 1;
                assert_eq!(p, 0.1);
            }
            Arm::Treated => {
                treated += 1;
                assert!((p - 0.9).abs() < 1e-12);
            }
        }
    }
    assert!(holdout > 10 && treated > 300, "{holdout} held out of 400");
    // Forced arms were not drawn; other override values are ignored.
    assert_eq!(assign("s1", 0.1, Some("holdout")), (Arm::Holdout, 1.0));
    assert_eq!(assign("s1", 0.1, Some("treated")), (Arm::Treated, 1.0));
    assert_eq!(assign("s1", 0.1, Some("bogus")), assign("s1", 0.1, None));
    // Shadow mode (a share of 1 or more) holds every session out, certainly.
    assert_eq!(assign("s1", 1.5, None), (Arm::Holdout, 1.0));
    assert_eq!(assign("s1", 0.0, None), (Arm::Treated, 1.0));
    // A state rebuilt from the spool keeps it.
    let recs = vec![json!({ "kind": "prompt", "arm": "holdout", "propensity": 0.1, "decision": Decision::default() })];
    let st = rebuild::rebuild(&recs, &Index::default(), "acme", &CommandTable::load()).unwrap();
    assert_eq!((st.arm, st.propensity), (Some(Arm::Holdout), Some(0.1)));
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
        Aspect::Green,
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
fn directives_carry_their_kind_and_stale_routes_are_never_green() {
    let (dir, index, table) = fixture();
    let ctx = Ctx {
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: None,
        params: Default::default(),
    };
    let mut st = SessionState::default();
    let d = decide(&ctx, &mut st, &Event::Prompt { text: "Pick up W-2042".into() });
    let kinds: Vec<(&str, &str)> = d.directives.iter().map(|x| (x.route.as_str(), x.kind.as_str())).collect();
    assert_eq!(kinds, vec![("lookup", "work-items"), ("dead-unittest", "dead-ends")]);
    let wrapped = render::wrap_directive(&d.directives[0]);
    assert!(
        wrapped.starts_with(
            "<adage-signal track=\"acme/t@0.1.0\" route=\"lookup\" aspect=\"green\" kind=\"work-items\">\n"
        ),
        "{wrapped}"
    );
    // Without a kind (a preview, an old record), the wrapper is as it always was.
    assert!(render::wrap("acme/t", "0.1.0", "lookup", Aspect::Green, "x")
        .starts_with("<adage-signal track=\"acme/t@0.1.0\" route=\"lookup\" aspect=\"green\">\n"));
    let old: Directive = serde_json::from_value(
        json!({ "route": "lookup", "track": "acme/t", "version": "0.1.0", "aspect": "green", "text": "x" }),
    )
    .unwrap();
    assert_eq!(old.kind, "");

    // A stale green route (and one on probation) is delivered yellow; red stays red.
    for state in [RouteState::Stale, RouteState::Probation] {
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
            index: &index,
            repo: "acme".into(),
            root: dir.path(),
            table: &table,
            embed: None,
            params: Default::default(),
        };
        let mut st = SessionState::default();
        let d = decide(&ctx, &mut st, &Event::Prompt { text: "Pick up W-2042".into() });
        let aspects: Vec<Aspect> = d.directives.iter().map(|x| x.aspect).collect();
        assert_eq!(aspects, vec![Aspect::Yellow, Aspect::Red], "{state:?}");
    }
}

/// Learned value types (Track G §4.3): prompt values, output values and edit segments.
mod typed_slots {
    use super::*;
    use adage_core::vtype::{Class, Run, Runs};

    /// `#482`-style trackers: three digits.
    fn digits3() -> ValueType {
        let runs = Runs { branches: vec![vec![Run::Class { class: Class::D, lo: 3, hi: 3 }]] };
        ValueType {
            id: "vt-0123456789abcdef".into(),
            regex: adage_core::vtype::to_regex(&runs),
            runs,
            samples_hash: String::new(),
        }
    }

    fn crate_name() -> ValueType {
        let runs = Runs {
            branches: vec![vec![Run::Lit { text: "pkg".into() }, Run::Class { class: Class::D, lo: 1, hi: 2 }]],
        };
        ValueType {
            id: "vt-00000000000000aa".into(),
            regex: adage_core::vtype::to_regex(&runs),
            runs,
            samples_hash: String::new(),
        }
    }

    fn slot(name: &str, source: SlotSource, t: &ValueType) -> Slot {
        Slot { name: name.into(), source, contract: Contract::Pattern { regex: t.regex.clone() } }
    }

    fn track(routes: Vec<Route>) -> Track {
        Track {
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
        }
    }

    fn routes() -> Vec<Route> {
        let t = digits3();
        let c = crate_name();
        vec![
            route(
                "lookup-work-item",
                Trigger::Prompt { span: SpanKind::Ticket },
                Step::Run { command: "./scripts/wi-lookup {item}".into() },
                vec![slot("item", SlotSource::PromptValue { vtype: t.clone(), mentions: vec![] }, &t)],
                Effect::Observe,
                Aspect::Green,
            ),
            route(
                "show-owner",
                Trigger::AfterCommand { command: "exec scripts/wi-lookup {arg}".into() },
                Step::Run { command: "./scripts/wi-lookup {id} --json".into() },
                vec![slot("id", SlotSource::OutputValue { field: Some("owner.id".into()), vtype: t.clone() }, &t)],
                Effect::Observe,
                Aspect::Yellow,
            ),
            route(
                "test-crate",
                Trigger::AfterEdit { paths: "crates/*/src/*.rs".into() },
                Step::Run { command: "cargo test -p {p}".into() },
                vec![slot("p", SlotSource::EditSegment { glob: "crates/*/src/*.rs".into(), depth: 1 }, &c)],
                Effect::Verify,
                Aspect::Green,
            ),
        ]
    }

    #[test]
    fn typed_tracks_validate_and_bad_ones_do_not() {
        adage_tracks::io::validate(&track(routes())).unwrap();
        // Runs and regex that disagree.
        let mut bad = routes();
        if let SlotSource::PromptValue { vtype, .. } = &mut bad[0].slots[0].source {
            vtype.regex = "^(?:[0-9]{4})$".into();
        }
        bad[0].slots[0].contract = Contract::Pattern { regex: "^(?:[0-9]{4})$".into() };
        assert!(adage_tracks::io::validate(&track(bad)).unwrap_err().to_string().contains("disagree"));
        // A contract that is not the type's regex.
        let mut bad = routes();
        bad[0].slots[0].contract = Contract::Pattern { regex: "^[0-9]{3}$".into() };
        assert!(adage_tracks::io::validate(&track(bad)).is_err());
        // A prompt value is not bound by an after-command trigger, nor an edit segment by a prompt.
        let mut bad = routes();
        bad[1].slots[0].source = SlotSource::PromptValue { vtype: digits3(), mentions: vec![] };
        assert!(adage_tracks::io::validate(&track(bad)).unwrap_err().to_string().contains("cannot bind"));
        let mut bad = routes();
        bad[0].slots[0].source = SlotSource::EditSegment { glob: "a/*".into(), depth: 0 };
        assert!(adage_tracks::io::validate(&track(bad)).is_err());
        // A typed route round-trips through YAML.
        let y = serde_json::to_string(&routes()[0]).unwrap();
        let back: Route = serde_json::from_str(&y).unwrap();
        assert_eq!(back, routes()[0]);
    }

    fn ctx<'a>(index: &'a Index, root: &'a Path, table: &'a CommandTable) -> Ctx<'a> {
        Ctx { index, repo: "acme".into(), root, table, embed: None, params: Default::default() }
    }

    #[test]
    fn prompt_values_bind_the_first_accepted_token() {
        let dir = tempfile::tempdir().unwrap();
        let table = CommandTable::load();
        let index = Index::compile(&[track(routes())]);
        let c = ctx(&index, dir.path(), &table);
        let mut st = SessionState::default();
        let d = decide(&c, &mut st, &Event::Prompt { text: "Please close out 12 things, starting with #482.".into() });
        assert_eq!(d.directives.len(), 1, "{d:?}");
        assert!(d.directives[0].text.contains("./scripts/wi-lookup 482"), "{}", d.directives[0].text);
        assert!(d.directives[0].text.contains("work item details"));
        // Hostile neighbours never bind: only a whole token the type accepts does.
        for text in ["run -482 now", "run 482;id", "try 4821 or $(id)", "x`482`y", "a 48 b"] {
            let mut st = SessionState::default();
            let d = decide(&c, &mut st, &Event::Prompt { text: text.into() });
            for x in &d.directives {
                assert!(x.text.contains("wi-lookup 482\n"), "{text}: {}", x.text);
            }
        }
        // Followed like any directed command.
        let d = decide(&c, &mut st, &Event::Batch { calls: vec![bash("./scripts/wi-lookup 482", "{}")] });
        assert!(d.judgements.iter().any(|j| matches!(j, Judgement::Followed { .. })), "{d:?}");
        // Learned mentions: only `#482` names an item, not a stray number.
        let mut rs = routes();
        if let SlotSource::PromptValue { mentions, .. } = &mut rs[0].slots[0].source {
            mentions.push(adage_core::tokens::Mention { prefix: "#".into(), suffix: String::new() });
        }
        adage_tracks::io::validate(&track(rs.clone())).unwrap();
        let index = Index::compile(&[track(rs)]);
        let c = ctx(&index, dir.path(), &table);
        let mut st = SessionState::default();
        let d = decide(&c, &mut st, &Event::Prompt { text: "must return 120, see #482".into() });
        assert!(d.directives[0].text.contains("wi-lookup 482\n"), "{d:?}");
        let mut st = SessionState::default();
        assert!(decide(&c, &mut st, &Event::Prompt { text: "must return 120".into() }).directives.is_empty());
    }

    #[test]
    fn output_values_and_edit_segments_bind() {
        let dir = tempfile::tempdir().unwrap();
        let table = CommandTable::load();
        let index = Index::compile(&[track(routes())]);
        let c = ctx(&index, dir.path(), &table);
        let mut st = SessionState::default();
        let out = r#"{"id": "482", "owner": {"id": 731, "name": "x"}}"#;
        let d = decide(&c, &mut st, &Event::Batch { calls: vec![bash("./scripts/wi-lookup 482", out)] });
        let texts: Vec<&str> = d.directives.iter().map(|x| x.text.as_str()).collect();
        assert!(texts.iter().any(|t| t.contains("./scripts/wi-lookup 731 --json")), "{texts:?}");
        // No value of the type in the field: no directive.
        let mut st = SessionState::default();
        let bad = r#"{"owner": {"id": "-1"}}"#;
        let d = decide(&c, &mut st, &Event::Batch { calls: vec![bash("./scripts/wi-lookup 482", bad)] });
        assert!(d.directives.iter().all(|x| x.route != "show-owner"));
        // An edit in crates/pkg7 directs and obliges `cargo test -p pkg7`.
        let edit = |p: &str| Call {
            tool: "Edit".into(),
            input: json!({ "file_path": dir.path().join(p).to_string_lossy(), "old_string": "a", "new_string": "b" }),
            output: String::new(),
            failed: false,
        };
        let mut st = SessionState::default();
        let d = decide(&c, &mut st, &Event::Batch { calls: vec![edit("crates/pkg7/src/lib.rs")] });
        assert!(d.directives.iter().any(|x| x.text.contains("cargo test -p pkg7")), "{d:?}");
        assert_eq!(st.obligations[0].command, "cargo test -p pkg7");
        let d = decide(&c, &mut st, &Event::Batch { calls: vec![bash("cargo test -p pkg7", "ok")] });
        assert!(d.judgements.iter().any(|j| matches!(j, Judgement::ObligationMet { .. })), "{d:?}");
        // A segment the contract refuses binds nothing.
        let mut st = SessionState::default();
        let d = decide(&c, &mut st, &Event::Batch { calls: vec![edit("crates/evil/src/lib.rs")] });
        assert!(d.directives.is_empty() && st.obligations.is_empty(), "{d:?}");
    }

    #[test]
    fn the_fast_path_skips_only_prompts_no_type_accepts() {
        let dir = tempfile::tempdir().unwrap();
        let table = CommandTable::load();
        let index = Index::compile(&[track(routes())]);
        let t = index.triggers("acme");
        assert_eq!(t.prompt_types.len(), 1);
        assert!(t.prompt_spans.is_empty(), "a typed slot does not need the span kind");
        let c = ctx(&index, dir.path(), &table);
        let prompted = || Some(SessionState { prompts: 1, ..Default::default() });
        for (text, skip) in
            [("look at #482", false), ("look at ops/482", false), ("look at W-2042", true), ("hi", true)]
        {
            assert_eq!(fast::skip(&t, &Event::Prompt { text: text.into() }, "/r", prompted), skip, "{text}");
            // Skipping changes no decision.
            let mut st = SessionState { prompts: 1, ..Default::default() };
            let d = decide(&c, &mut st, &Event::Prompt { text: text.into() });
            assert_eq!(d.directives.is_empty(), skip, "{text}");
        }
    }
}

/// Track G G3: a sequence after an edit is said once, numbered, judged on its first command and
/// never a Stop request; a context trigger is compiled but inert until G4 (it never matches, and
/// the fast path adds nothing for it).
#[test]
fn a_sequence_follows_an_edit_and_a_context_never_fires() {
    let dir = tempfile::tempdir().unwrap();
    let table = CommandTable::load();
    let track = Track {
        manifest: Manifest {
            track: TrackInfo {
                name: "acme/graph".into(),
                version: "0.1.0".into(),
                description: String::new(),
                provenance: "desire-path".into(),
            },
            scope: Scope { repo: "acme".into() },
            gauge: Gauge::default(),
        },
        routes: vec![
            route(
                "sequence-1",
                Trigger::AfterEdit { paths: "registry/*.csv".into() },
                Step::Sequence { commands: vec!["make gen".into(), "make check".into()] },
                vec![],
                Effect::Regenerate,
                Aspect::Green,
            ),
            route(
                "run-ctx",
                Trigger::Context {
                    items: vec![
                        CtxItem::Edit { glob: "registry/*.csv".into() },
                        CtxItem::Act { keys: vec!["exec make gen".into()] },
                    ],
                    within: 8,
                },
                Step::Run { command: "make check".into() },
                vec![],
                Effect::Verify,
                Aspect::Green,
            ),
        ],
    };
    adage_tracks::io::validate(&track).unwrap();
    let index = Index::compile(&[track]);
    let ctx = Ctx {
        index: &index,
        repo: "acme".into(),
        root: dir.path(),
        table: &table,
        embed: None,
        params: Default::default(),
    };
    let mut st = SessionState::default();
    let edit = Call {
        tool: "Edit".into(),
        input: json!({ "file_path": dir.path().join("registry/ports.csv").to_string_lossy() }),
        output: String::new(),
        failed: false,
    };
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![edit.clone()] });
    assert_eq!(d.directives.len(), 1, "{:?}", d.directives);
    assert_eq!(d.directives[0].route, "sequence-1");
    assert!(d.directives[0].text.contains("  1. make gen\n  2. make check"), "{}", d.directives[0].text);
    // Said once: a second edit says nothing again.
    assert!(decide(&ctx, &mut st, &Event::Batch { calls: vec![edit] }).directives.is_empty());
    // Followed on its first command; the context route never speaks, whatever happens.
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![bash("make gen", "ok")] });
    assert!(d.judgements.iter().any(|j| matches!(j, Judgement::Followed { route } if route == "sequence-1")));
    assert!(d.directives.is_empty(), "{:?}", d.directives);
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![bash("make check", "ok")] });
    assert!(d.directives.is_empty());
    // A sequence is not an obligation: no Stop request.
    assert!(decide(&ctx, &mut st, &Event::Stop { stop_hook_active: false }).continue_reason.is_none());
    // The fast path's triggers carry nothing for the context route.
    let t = adage_tracks::triggers::Triggers::of(index.routes.iter().map(|r| &r.route));
    assert_eq!(t.edit_globs, ["registry/*.csv"]);
    assert!(t.words.is_empty() && !t.any_call);
}
