use super::*;
use adage_core::Effect;
use adage_tracks::*;
use serde_json::json;

fn route(id: &str, trigger: Trigger, step: Step, slots: Vec<Slot>, effect: Effect, strength: Strength) -> Route {
    Route {
        id: id.into(),
        summary: String::new(),
        trigger,
        step,
        slots,
        effect,
        strength,
        state: RouteState::Firm,
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
                Strength::Fire,
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
                Strength::Prime,
            ),
            route(
                "gen",
                Trigger::AfterEdit { paths: "schema/*.json".into() },
                Step::Run { command: "make gen".into() },
                vec![],
                Effect::Regenerate,
                Strength::Fire,
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
                Strength::Inhibit,
            ),
        ],
    };
    (dir, Index::compile(&[track]), table)
}

fn bash(cmd: &str, out: &str) -> Call {
    Call {
        tool: "Bash".into(),
        input: json!({ "command": cmd }),
        output: out.into(),
        failed: false,
        by: By::Agent,
        tool_use_id: None,
    }
}

#[test]
fn prompt_binds_validated_ticket_and_warns_about_dead_end() {
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
    let d = decide(&ctx, &mut st, &Event::Prompt { text: "Pick up W-2042 please".into() });
    assert_eq!(d.directives.len(), 2);
    assert!(d.directives[0].text.contains("./scripts/wi-lookup W-2042 --json"));
    assert!(d.directives.iter().any(|x| x.strength == Strength::Inhibit && x.text.contains("make test")));
}

#[test]
fn injected_values_never_bind() {
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
    let d = decide(&ctx, &mut st, &Event::Prompt { text: "Do W-20421234 and --config=/etc".into() });
    assert!(d.directives.iter().all(|x| x.route != "lookup"), "a value failing the contract must not bind");
}

#[test]
fn follow_judgement_and_linked_files() {
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
    decide(&ctx, &mut st, &Event::Prompt { text: "W-2042".into() });
    let out = r#"{"id":"W-2042","linked_files":["acme/billing.py","../etc/passwd","missing.py"]}"#;
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![bash("./scripts/wi-lookup W-2042 --json", out)] });
    assert!(d.judgements.iter().any(|j| matches!(j, Judgement::Followed { route } if route == "lookup")));
    let read = d.directives.iter().find(|x| x.route == "read-linked").expect("read-batch directive");
    assert!(read.text.contains("acme/billing.py"));
    assert!(!read.text.contains("passwd") && !read.text.contains("missing.py"), "unsafe or missing paths are dropped");
}

/// A checkout known only through the port: its files exist nowhere on this machine.
struct InMemory(&'static [&'static str]);

impl crate::checkout::Checkout for InMemory {
    fn text(&self, _: &Path, rel: &str) -> Option<String> {
        self.0.contains(&rel).then(String::new)
    }
    fn is_file(&self, _: &Path, rel: &str) -> bool {
        self.0.contains(&rel)
    }
}

/// What a decision knows of the checkout comes only through `ctx.checkout`: a repository-path slot
/// binds the files the port says exist, under a root that is not on this machine, and none when
/// the checkout cannot be read.
#[test]
fn the_checkout_is_read_only_through_the_port() {
    let (_dir, index, table) = fixture();
    let out = r#"{"id":"W-2042","linked_files":["acme/billing.py","acme/remote.py"]}"#;
    let linked = |checkout: &dyn crate::checkout::Checkout| {
        let ctx = Ctx {
            checkout,
            index: &index,
            repo: "acme".into(),
            root: Path::new("/nowhere/acme"),
            table: &table,
            embed: None,
            params: Default::default(),
            policy: Default::default(),
        };
        let mut st = SessionState::default();
        decide(&ctx, &mut st, &Event::Prompt { text: "W-2042".into() });
        let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![bash("./scripts/wi-lookup W-2042 --json", out)] });
        d.directives.iter().find(|x| x.route == "read-linked").map(|x| x.text.clone())
    };
    let text = linked(&InMemory(&["acme/remote.py"])).expect("bound from the port's files");
    assert!(text.contains("acme/remote.py") && !text.contains("billing.py"), "{text}");
    assert_eq!(linked(&crate::checkout::Unread), None, "nothing readable: the slot never binds");
}

#[test]
fn obligation_triggers_on_edit_and_is_enforced_once_at_stop() {
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
    let edit = Call {
        tool: "Edit".into(),
        input: json!({ "file_path": dir.path().join("schema/invoice.json").to_string_lossy() }),
        output: String::new(),
        failed: false,
        by: By::Agent,
        tool_use_id: None,
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
    let edit = Call {
        tool: "Write".into(),
        input: json!({ "file_path": dir.path().join("schema/a.json").to_string_lossy() }),
        output: String::new(),
        failed: false,
        by: By::Agent,
        tool_use_id: None,
    };
    decide(&ctx, &mut st, &Event::Batch { calls: vec![edit] });
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![bash("make gen", "ok")] });
    assert!(d.judgements.iter().any(|j| matches!(j, Judgement::ObligationMet { .. })));
    assert!(decide(&ctx, &mut st, &Event::Stop { stop_hook_active: false }).continue_reason.is_none());
}

#[test]
fn control_logs_exposure_but_emits_nothing() {
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
    let mut st = SessionState { arm: Some(Arm::Control), ..Default::default() };
    let d = decide(&ctx, &mut st, &Event::Prompt { text: "W-2042".into() });
    assert!(d.directives.is_empty());
    assert!(!d.would_have.is_empty());
}

#[test]
fn other_repos_get_nothing() {
    let (dir, index, table) = fixture();
    let ctx = Ctx {
        checkout: &crate::checkout::Disk,
        index: &index,
        repo: "other".into(),
        root: dir.path(),
        table: &table,
        embed: None,
        params: Default::default(),
        policy: Default::default(),
    };
    let mut st = SessionState::default();
    assert!(decide(&ctx, &mut st, &Event::Prompt { text: "W-2042".into() }).directives.is_empty());
}

#[test]
fn caps_hold() {
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
    assert_eq!(assign_arm("abc", 1.0), Arm::Control);
}

#[test]
fn linked_files_from_plain_text_output() {
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
    let out = r#"{"linked_files":["acme/notes.md","acme/billing.py"]}"#;
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![bash("./scripts/wi-lookup W-2042 --json", out)] });
    let read = d.directives.iter().find(|x| x.route == "read-linked").unwrap();
    assert!(!read.text.contains("notes.md") && read.text.contains("acme/billing.py"));
}

#[test]
fn obligations_rearm_after_a_new_edit_and_stop_requests_are_bounded() {
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
    let edit = |f: &str| Call {
        tool: "Edit".into(),
        input: json!({ "file_path": dir.path().join(f).to_string_lossy() }),
        output: String::new(),
        failed: false,
        by: By::Agent,
        tool_use_id: None,
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
fn control_records_would_continue() {
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
    let edit = Call {
        tool: "Edit".into(),
        input: json!({ "file_path": dir.path().join("schema/a.json").to_string_lossy() }),
        output: String::new(),
        failed: false,
        by: By::Agent,
        tool_use_id: None,
    };
    decide(&ctx, &mut st, &Event::Batch { calls: vec![edit] });
    st.arm = Some(Arm::Control);
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
        by: By::Agent,
        tool_use_id: None,
    }
}

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

fn intent_route(id: &str, family: &str, command: &str, strength: Strength) -> Route {
    let centroid = adage_intent::quantize(&adage_intent::unit(vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]));
    route(
        id,
        Trigger::Intent { family: family.into(), model: MODEL.into(), centroid, threshold: 0.8 },
        Step::Run { command: command.into() },
        vec![],
        Effect::Verify,
        strength,
    )
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

// ---------------------------------------------------------------- interlocking

fn npm_test_dead_end() -> Route {
    route(
        "dead-end-npm",
        Trigger::SessionStart,
        Step::Avoid { avoid: "npm test".into(), instead: "make test".into(), signature: "ACME_ENV is not set".into() },
        vec![],
        Effect::Verify,
        Strength::Inhibit,
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
                Strength::Fire,
            ),
            route(
                "show-owner",
                Trigger::AfterCommand { command: "exec scripts/wi-lookup {arg}".into() },
                Step::Run { command: "./scripts/wi-lookup {id} --json".into() },
                vec![slot("id", SlotSource::OutputValue { field: Some("owner.id".into()), vtype: t.clone() }, &t)],
                Effect::Observe,
                Strength::Prime,
            ),
            route(
                "test-crate",
                Trigger::AfterEdit { paths: "crates/*/src/*.rs".into() },
                Step::Run { command: "cargo test -p {p}".into() },
                vec![slot("p", SlotSource::EditSegment { glob: "crates/*/src/*.rs".into(), depth: 1 }, &c)],
                Effect::Verify,
                Strength::Fire,
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
        Ctx {
            checkout: &crate::checkout::Disk,
            index,
            repo: "acme".into(),
            root,
            table,
            embed: None,
            params: Default::default(),
            policy: Default::default(),
        }
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
            by: By::Agent,
            tool_use_id: None,
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

// ---------------------------------------------------------------- context triggers (Track G G4)

fn ctx_route(id: &str, items: Vec<CtxItem>, within: u8, step: Step, effect: Effect) -> Route {
    route(id, Trigger::Context { items, within }, step, vec![], effect, Strength::Prime)
}

fn act_item(k: &str) -> CtxItem {
    CtxItem::Act { keys: vec![k.into()] }
}

/// A repository with a sequence after a registry edit, and three context routes: `make check`
/// after a registry edit then `make gen` (within 2 alphabet events), the steps after `make lint`
/// then `make fmt`, and `make gen` again after it failed with its signature and the registry was
/// edited.
fn context_index(within: u8) -> Index {
    let routes = vec![
        route(
            "sequence-1",
            Trigger::AfterEdit { paths: "registry/*.csv".into() },
            Step::Sequence { commands: vec!["make gen".into(), "make check".into()] },
            vec![],
            Effect::Regenerate,
            Strength::Fire,
        ),
        ctx_route(
            "run-ctx",
            vec![CtxItem::Edit { glob: "registry/*.csv".into() }, act_item("exec make gen")],
            within,
            Step::Run { command: "make check".into() },
            Effect::Verify,
        ),
        ctx_route(
            "sequence-ctx",
            vec![act_item("exec make lint"), act_item("exec make fmt")],
            8,
            Step::Sequence { commands: vec!["make gen".into(), "make check".into()] },
            Effect::Regenerate,
        ),
        ctx_route(
            "retry-ctx",
            vec![
                CtxItem::Fail { keys: vec!["exec make gen".into()], signature: "out of date".into() },
                CtxItem::Edit { glob: "registry/*.csv".into() },
            ],
            8,
            Step::Run { command: "make gen".into() },
            Effect::Regenerate,
        ),
    ];
    let t = track("acme/graph", "desire-path", routes);
    adage_tracks::io::validate(&t).unwrap();
    Index::compile(&[t])
}

fn failed_bash(cmd: &str, out: &str) -> Call {
    Call { failed: true, ..bash(cmd, out) }
}

/// Decide every event on the full path and, as the hook does, on the fast path (an event it
/// skips is not decided). A skipped event's full decision must be empty, and the state later
/// decisions read, context symbols included, must be the same on both paths.
fn decide_both(ctx: &Ctx, full: &mut SessionState, fast_st: &mut SessionState, ev: &Event) -> Decision {
    let root = ctx.root.to_string_lossy().to_string();
    let t = ctx.index.triggers(&ctx.repo);
    let skip = fast::skip(&t, ev, &root, || Some(fast_st.clone()));
    let d = decide(ctx, full, ev);
    if skip {
        assert!(d.would_have.is_empty() && d.judgements.is_empty(), "skipped {ev:?}, decided {d:?}");
    } else {
        let f = decide(ctx, fast_st, ev);
        assert_eq!(serde_json::to_value(&f).unwrap(), serde_json::to_value(&d).unwrap(), "{ev:?}");
    }
    let state = |s: &SessionState| json!([s.emitted, s.pending, s.obligations, s.stop_requests, s.ctx]);
    assert_eq!(state(fast_st), state(full), "after {ev:?}");
    d
}

/// Track G G3 and G4: a sequence after an edit is said once, numbered, judged on its first
/// command and never a Stop request; a context route fires on the event of its last item, when
/// its earlier items came before it within its gap, and is judged like any other directive.
#[test]
fn a_sequence_follows_an_edit_and_a_context_fires_after_its_items() {
    let dir = tempfile::tempdir().unwrap();
    let table = CommandTable::load();
    let index = context_index(2);
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
    let (mut st, mut fast_st) = (SessionState::default(), SessionState::default());
    let mut go = |ev: Event| decide_both(&ctx, &mut st, &mut fast_st, &ev);
    let edit = || Event::Batch { calls: vec![edit_call(&dir, "registry/ports.csv")] };
    let d = go(edit());
    assert_eq!(d.directives.len(), 1, "{:?}", d.directives);
    assert_eq!(d.directives[0].route, "sequence-1");
    assert!(d.directives[0].text.contains("  1. make gen\n  2. make check"), "{}", d.directives[0].text);
    assert!(d.ctx.is_some(), "an alphabet event is recorded");
    // Events outside the alphabet change nothing, however many (in one batch: the sequence's
    // follow window is still open after it).
    let noise: Vec<Call> = (0..30).map(|i| bash(&format!("git log -n {i}"), "x")).collect();
    let d = go(Event::Batch { calls: noise });
    assert!(d.directives.is_empty() && d.ctx.is_none());
    // `make gen` completes the context: the sequence is followed and the context route fires.
    let d = go(Event::Batch { calls: vec![bash("make gen -j4", "ok")] });
    assert!(d.judgements.iter().any(|j| matches!(j, Judgement::Followed { route } if route == "sequence-1")));
    assert_eq!(d.directives.iter().map(|x| x.route.as_str()).collect::<Vec<_>>(), ["run-ctx"]);
    assert_eq!(
        d.directives[0].text,
        "A known next step here is:\n  make check\nIgnore this if the user is asking for something else."
    );
    // Judged like any directive, and never a Stop request.
    let d = go(Event::Batch { calls: vec![bash("make check", "ok")] });
    assert!(d.judgements.iter().any(|j| matches!(j, Judgement::Followed { route } if route == "run-ctx")));
    assert!(go(Event::Stop { stop_hook_active: false }).continue_reason.is_none());
    // Said once: another edit, and make gen after it, say nothing again.
    assert!(go(edit()).directives.is_empty());
    assert!(go(Event::Batch { calls: vec![bash("make gen", "ok")] }).directives.is_empty());
    // The fast path's triggers carry every item's words and globs.
    let t = index.triggers("acme");
    assert_eq!(t.edit_globs, ["registry/*.csv"]);
    assert_eq!(t.words, ["make"]);
}

/// One edit, one directive: a sequence whose first step is a finish line's command subsumes the
/// finish line. The agent is told once (the sequence), and the obligation rides on the sequence's
/// first step: met when it runs, a Stop request when it does not, re-armed by a later edit.
#[test]
fn a_sequence_subsumes_the_finish_line_it_begins_with() {
    let dir = tempfile::tempdir().unwrap();
    let table = CommandTable::load();
    let routes = vec![
        route(
            "finish-line",
            Trigger::AfterEdit { paths: "schema/*.json".into() },
            Step::Run { command: "make gen".into() },
            vec![],
            Effect::Regenerate,
            Strength::Fire,
        ),
        route(
            "gen-then-test",
            Trigger::AfterEdit { paths: "schema/*.json".into() },
            Step::Sequence { commands: vec!["make gen".into(), "make test".into()] },
            vec![],
            Effect::Regenerate,
            Strength::Fire,
        ),
        route(
            "docs-line",
            Trigger::AfterEdit { paths: "docs/*.md".into() },
            Step::Run { command: "make docs".into() },
            vec![],
            Effect::Regenerate,
            Strength::Fire,
        ),
    ];
    let index = Index::compile(&[track("acme/t", "desire-path", routes)]);
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
    let (mut st, mut fast_st) = (SessionState::default(), SessionState::default());
    let mut go = |ev: Event| decide_both(&ctx, &mut st, &mut fast_st, &ev);
    let edit = |f: &str| Event::Batch { calls: vec![edit_call(&dir, f)] };
    let stop = || Event::Stop { stop_hook_active: false };

    let d = go(edit("schema/users.json"));
    let said: Vec<&str> = d.directives.iter().map(|x| x.route.as_str()).collect();
    assert_eq!(said, ["gen-then-test"], "one directive for one edit");
    // Not run: the carried obligation asks at Stop, once.
    let s = go(stop());
    assert!(s.continue_reason.as_deref().is_some_and(|r| r.contains("make gen")), "{s:?}");
    assert!(go(stop()).continue_reason.is_none());
    // Its first step meets the obligation and follows the sequence.
    go(edit("schema/users.json"));
    let d = go(Event::Batch { calls: vec![bash("make gen", "ok")] });
    assert!(d.judgements.iter().any(|j| matches!(j, Judgement::ObligationMet { route } if route == "finish-line")));
    assert!(go(stop()).continue_reason.is_none(), "met");
    // A finish line no sequence begins with is unchanged.
    let d = go(edit("docs/index.md"));
    assert_eq!(d.directives.iter().map(|x| x.route.as_str()).collect::<Vec<_>>(), ["docs-line"]);
}

/// A compiled skill's "before you finish, run `make ci`" is a finish line after any edit (`*`). A
/// code edit is told once, by the sequence `make test`, `make ci` that carries it; a schema edit,
/// which no sequence covers, is told by the finish line itself, and Stop asks for it either way.
#[test]
fn a_finish_line_after_any_edit_rides_on_a_sequence_that_includes_it() {
    let dir = tempfile::tempdir().unwrap();
    let table = CommandTable::load();
    let routes = vec![
        route(
            "before-finish",
            Trigger::AfterEdit { paths: "*".into() },
            Step::Run { command: "make ci".into() },
            vec![],
            Effect::Verify,
            Strength::Fire,
        ),
        route(
            "test-then-ci",
            Trigger::AfterEdit { paths: "acme/*.py".into() },
            Step::Sequence { commands: vec!["make test".into(), "make ci".into()] },
            vec![],
            Effect::Verify,
            Strength::Fire,
        ),
    ];
    let index = Index::compile(&[track("acme/t", "desire-path", routes)]);
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
    let (mut st, mut fast_st) = (SessionState::default(), SessionState::default());
    let mut go = |ev: Event| decide_both(&ctx, &mut st, &mut fast_st, &ev);
    let edit = |f: &str| Event::Batch { calls: vec![edit_call(&dir, f)] };
    let stop = || Event::Stop { stop_hook_active: false };
    let d = go(edit("acme/billing.py"));
    assert_eq!(d.directives.iter().map(|x| x.route.as_str()).collect::<Vec<_>>(), ["test-then-ci"]);
    let s = go(stop());
    assert!(s.continue_reason.as_deref().is_some_and(|r| r.contains("make ci")), "{s:?}");

    let (mut st, mut fast_st) = (SessionState::default(), SessionState::default());
    let mut go = |ev: Event| decide_both(&ctx, &mut st, &mut fast_st, &ev);
    let d = go(edit("schema/customer.json"));
    assert_eq!(d.directives.iter().map(|x| x.route.as_str()).collect::<Vec<_>>(), ["before-finish"]);
    let s = go(stop());
    assert!(s.continue_reason.as_deref().is_some_and(|r| r.contains("make ci")), "{s:?}");
    go(Event::Batch { calls: vec![bash("make ci", "ok")] });
    assert!(go(stop()).continue_reason.is_none(), "met");
}

#[test]
fn a_context_needs_its_items_in_order_within_its_gap_of_alphabet_events() {
    let dir = tempfile::tempdir().unwrap();
    let table = CommandTable::load();
    let index = context_index(2);
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
    let fired = |events: Vec<Event>| -> Vec<String> {
        let (mut st, mut fast_st) = (SessionState::default(), SessionState::default());
        let mut out = Vec::new();
        for ev in events {
            out.extend(decide_both(&ctx, &mut st, &mut fast_st, &ev).would_have.into_iter().map(|d| d.route));
        }
        out.retain(|r| r.ends_with("-ctx"));
        out
    };
    let b = |cmd: &str| Event::Batch { calls: vec![bash(cmd, "ok")] };
    let edit = || Event::Batch { calls: vec![edit_call(&dir, "registry/ports.csv")] };
    // Two alphabet events between the edit and make gen: a gap of 3 > 2.
    assert!(fired(vec![edit(), b("make lint"), b("make lint"), b("make gen")]).is_empty());
    assert_eq!(fired(vec![edit(), b("make lint"), b("make gen")]), ["run-ctx"]);
    // Out of order, or another directory: nothing.
    assert!(fired(vec![b("make gen"), edit()]).is_empty());
    let other = Event::Batch { calls: vec![edit_call(&dir, "docs/ports.csv")] };
    assert!(fired(vec![other, b("make gen")]).is_empty());
    // Two items in one batch are two events, in the batch's order.
    let both = Event::Batch { calls: vec![bash("make lint", "ok"), bash("make fmt", "ok")] };
    assert_eq!(fired(vec![both]), ["sequence-ctx"]);
    // A context route is said once per session.
    assert_eq!(fired(vec![b("make lint"), b("make fmt"), b("make lint"), b("make fmt")]), ["sequence-ctx"]);
}

/// A failed command is a failure item, never a successful command item, even when the harness
/// lists it again in the tool batch after its failure hook (Claude Code does).
#[test]
fn a_failed_command_is_a_failure_item_and_not_a_success() {
    let dir = tempfile::tempdir().unwrap();
    let table = CommandTable::load();
    let index = context_index(8);
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
    let (mut st, mut fast_st) = (SessionState::default(), SessionState::default());
    let mut go = |ev: Event| decide_both(&ctx, &mut st, &mut fast_st, &ev);
    go(Event::Batch { calls: vec![edit_call(&dir, "registry/ports.csv")] });
    let fail = failed_bash("make gen", "error: registry.py is out of date");
    let d = go(Event::Failure { call: fail });
    assert!(d.directives.is_empty());
    // The same call in the batch that follows is not `make gen` succeeding: no `run-ctx`.
    let d = go(Event::Batch { calls: vec![bash("make gen", "error: registry.py is out of date")] });
    assert!(d.would_have.iter().all(|x| x.route != "run-ctx"), "{:?}", d.would_have);
    // The failure, then an edit of the registry: run it again.
    let d = go(Event::Batch { calls: vec![edit_call(&dir, "registry/ports.csv")] });
    assert_eq!(d.would_have.iter().map(|x| x.route.as_str()).collect::<Vec<_>>(), ["retry-ctx"]);
    // A later successful `make gen` is one.
    let d = go(Event::Batch { calls: vec![bash("make gen", "ok")] });
    assert_eq!(d.would_have.iter().map(|x| x.route.as_str()).collect::<Vec<_>>(), ["run-ctx"]);
}

#[test]
fn context_state_is_bounded_rebuilt_from_the_spool_and_reset_by_a_new_alphabet() {
    let dir = tempfile::tempdir().unwrap();
    let table = CommandTable::load();
    let index = context_index(8);
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
    let mut st = SessionState { arm: Some(Arm::Treated), ..Default::default() };
    let mut records = Vec::new();
    let mut events = vec![Event::Batch { calls: vec![edit_call(&dir, "registry/a.csv")] }];
    for i in 0..30 {
        events.push(Event::Batch { calls: vec![bash(if i % 2 == 0 { "make lint" } else { "ls" }, "ok")] });
    }
    events.push(Event::Failure { call: failed_bash("make gen", "out of date") });
    events.push(Event::Batch { calls: vec![bash("make gen", "out of date")] });
    for (i, ev) in events.iter().enumerate() {
        let d = decide(&ctx, &mut st, ev);
        records.push(json!({ "kind": ev.name(), "arm": st.arm, "decision": d }));
        let rebuilt = rebuild::rebuild(&records, &index, "acme", &table).unwrap();
        assert_eq!(rebuilt.ctx, st.ctx, "after event {i}");
        assert!(st.ctx.ring.len() <= crate::context::RING && st.ctx.since.iter().all(|s| *s <= crate::context::SPAN));
    }
    assert!(serde_json::to_vec(&st).unwrap().len() < 2048, "the state stays small");
    // A relearned index with another alphabet: the recorded symbols mean nothing there.
    let other = context_index(8);
    let mut t = track("acme/graph", "desire-path", other.routes.iter().map(|r| r.route.clone()).collect());
    t.routes.retain(|r| r.id != "retry-ctx");
    let relearned = Index::compile(&[t]);
    assert_ne!(relearned.contexts("acme").digest, index.contexts("acme").digest);
    assert!(rebuild::rebuild(&records, &relearned, "acme", &table).unwrap().ctx.is_empty());
    // And a live state from the old alphabet starts over at its next alphabet event.
    let ctx2 = Ctx { index: &relearned, ..ctx };
    decide(&ctx2, &mut st, &Event::Batch { calls: vec![bash("make lint", "ok")] });
    assert_eq!((st.ctx.alphabet, st.ctx.ring.len()), (relearned.contexts("acme").digest, 1));
}

/// A context ending in a prompt item: a learned value type in the prompt, after a command.
#[test]
fn a_context_can_end_at_a_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let table = CommandTable::load();
    let runs = adage_core::vtype::Runs {
        branches: vec![vec![
            adage_core::vtype::Run::Lit { text: "OPS".into() },
            adage_core::vtype::Run::Lit { text: "-".into() },
            adage_core::vtype::Run::Class { class: adage_core::vtype::Class::D, lo: 2, hi: 3 },
        ]],
    };
    let regex = adage_core::vtype::to_regex(&runs);
    let vtype = ValueType { id: "vt-0123456789abcdef".into(), runs, regex, samples_hash: String::new() };
    let t = track(
        "acme/graph",
        "desire-path",
        vec![ctx_route(
            "prompt-ctx",
            vec![act_item("exec make deps"), CtxItem::PromptValue { vtype }],
            8,
            Step::Run { command: "make check".into() },
            Effect::Verify,
        )],
    );
    adage_tracks::io::validate(&t).unwrap();
    let index = Index::compile(&[t]);
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
    let (mut st, mut fast_st) = (SessionState::default(), SessionState::default());
    let mut go = |ev: Event| decide_both(&ctx, &mut st, &mut fast_st, &ev);
    assert!(go(Event::Prompt { text: "Look at OPS-12".into() }).directives.is_empty(), "no command before it");
    go(Event::Batch { calls: vec![bash("make deps", "ok")] });
    assert!(go(Event::Prompt { text: "Now the README".into() }).directives.is_empty());
    let d = go(Event::Prompt { text: "Now OPS-77 please".into() });
    assert_eq!(d.directives.iter().map(|x| x.route.as_str()).collect::<Vec<_>>(), ["prompt-ctx"]);
}

/// U11 as the core instinct pack binds it in click (RepoBench v2): after `python -m pytest` fails
/// with `No module named pytest`, run `.venv/bin/python -m pytest`.
fn u11_routes() -> Vec<Route> {
    let table = CommandTable::load();
    let mut r = route(
        "dead-end-0cdcfb",
        Trigger::AfterFailure {
            command: command_key("python -m pytest", &table),
            signature: "No module named pytest".into(),
        },
        Step::Run { command: ".venv/bin/python -m pytest".into() },
        vec![],
        Effect::Verify,
        Strength::Prime,
    );
    r.state = RouteState::Forming;
    r.evidence.provenance = "instinct:core@1.0.0".into();
    vec![r]
}

fn failed(cmd: &str, out: &str) -> Call {
    Call { failed: true, ..bash(cmd, out) }
}

fn kinds(d: &Decision) -> Vec<String> {
    d.judgements.iter().map(|j| serde_json::to_value(j).unwrap()["kind"].as_str().unwrap().to_string()).collect()
}

/// Bug 1 (launch report): RepoBench v2 session `eval-click-0cd28d65-r2-adage`. The agent's next
/// test command after the U11 directive ran `.venv/bin/python -m pytest` with its own test id and
/// flags. `meets` judged all 41 such follows "ignored"; they are followed. A followed run whose
/// tests fail is followed too (the tests failing is the task); only a run where the runner could
/// not start is a misfire.
#[test]
fn a_directed_runner_narrowed_to_the_task_is_followed() {
    let dir = tempfile::tempdir().unwrap();
    let table = CommandTable::load();
    let index = Index::compile(&[track("github-com-pallets-click/instincts", "instinct:core@1.0.0", u11_routes())]);
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
    let first = "python -m pytest tests/test_formatting.py::test_formatting_usage_custom_help -xvs";
    let missing = "Exit code 1\n/usr/bin/python: No module named pytest";
    let followed = ".venv/bin/python -m pytest tests/test_formatting.py::test_formatting_usage_custom_help -xvs";
    let piped = format!("{followed} 2>&1 | head -100");
    for (outcome, want) in [
        (bash(followed, "test_formatting_usage_custom_help PASSED\n==== 1 passed in 0.05s ===="), "followed"),
        (failed(followed, "FAILED tests/test_formatting.py::test_x\n==== 1 failed in 0.09s ===="), "followed"),
        (failed(followed, "bash: .venv/bin/python: No such file or directory"), "misfired"),
        (bash(&piped, "==== 1 failed, 2 passed in 0.2s ===="), "followed"),
        (bash(&piped, "bash: .venv/bin/python: No such file or directory"), "misfired"),
        // The action classifier (adage_core::action): the environment activated, then its python.
        (
            bash(
                "source .venv/bin/activate && python -m pytest tests/test_formatting.py -q 2>&1 | tail -20",
                "==== 3 passed in 0.2s ====",
            ),
            "followed",
        ),
    ] {
        let (mut st, mut fast_st) = (SessionState::default(), SessionState::default());
        let mut go = |ev: Event| decide_both(&ctx, &mut st, &mut fast_st, &ev);
        let d = go(Event::Failure { call: failed(first, missing) });
        assert_eq!(d.directives.iter().map(|x| x.route.as_str()).collect::<Vec<_>>(), ["dead-end-0cdcfb"]);
        // Claude Code lists the failed call again in the batch after it: not a judgement.
        assert!(go(Event::Batch { calls: vec![failed(first, missing)] }).judgements.is_empty());
        let d = if outcome.failed {
            go(Event::Failure { call: outcome.clone() })
        } else {
            go(Event::Batch { calls: vec![outcome.clone()] })
        };
        assert_eq!(kinds(&d), [want], "{:?}", outcome.input);
    }
    // Another interpreter is not the directed one: the window runs out, ignored.
    let (mut st, mut fast_st) = (SessionState::default(), SessionState::default());
    let mut go = |ev: Event| decide_both(&ctx, &mut st, &mut fast_st, &ev);
    go(Event::Failure { call: failed(first, missing) });
    let mut all = Vec::new();
    for _ in 0..10 {
        all.extend(kinds(&go(Event::Batch {
            calls: vec![bash("python3 -m pytest tests -x", "no tests ran in 0.01s")],
        })));
    }
    assert_eq!(all, ["ignored"]);
}

/// The learned follow judge (adage_core::judge, evals/lab/research/uptake-delivery): with the
/// repository's weights, a follow no hand-written rule reads (the runner behind a wrapper the
/// multiplexer table does not know) is judged followed; the system Python still is not.
#[test]
fn the_learned_judge_reads_follows_by_the_repository_s_tokens() {
    let dir = tempfile::tempdir().unwrap();
    let table = CommandTable::load();
    let index = Index::compile(&[track("github-com-pallets-click/instincts", "instinct:core@1.0.0", u11_routes())]);
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
    let pairs: Vec<(String, String)> = (0..8)
        .flat_map(|i| {
            [
                (format!("s{i}"), format!(".venv/bin/python -m pytest tests/test_{i}.py -x")),
                (format!("s{i}"), "python -m pytest tests".to_string()),
            ]
        })
        .collect();
    let w = adage_core::judge::Weights::fit(pairs.iter().map(|(a, b)| (a.as_str(), b.as_str())));
    let first = "python -m pytest tests/test_basic.py -x";
    let missing = "/usr/bin/python: No module named pytest";
    let wrapped = "xvfb-run -a .venv/bin/python -m pytest tests/test_basic.py -x";
    let judge = |weights: Option<adage_core::judge::Weights>, then: &str| {
        let mut st = SessionState::default();
        let ctx = Ctx { policy: Policy { follow_weights: weights, ..Default::default() }, ..ctx.clone() };
        decide(&ctx, &mut st, &Event::Failure { call: failed(first, missing) });
        kinds(&decide(&ctx, &mut st, &Event::Batch { calls: vec![bash(then, "==== 1 passed in 0.1s ====")] }))
    };
    assert!(judge(None, wrapped).is_empty(), "no hand-written rule reads it");
    assert_eq!(judge(Some(w.clone()), wrapped), ["followed"]);
    assert!(judge(Some(w), "python -m pytest tests/test_basic.py -x").is_empty());
}

/// Bug 5 (launch report): in the zero-config E2E, 4 of 6 sessions began with `python -c "import
/// click"` or `python repro.py`, which failed with `No module named 'click'` and got nothing. U11's
/// advice names the project's interpreter; a pytest run failing so gets U11's run step alone.
#[test]
fn a_python_run_outside_the_project_environment_is_answered() {
    let dir = tempfile::tempdir().unwrap();
    let table = CommandTable::load();
    let mut routes = u11_routes();
    let mut click = u11_routes().remove(0);
    click.id = "dead-end-click".into();
    click.trigger = Trigger::AfterFailure {
        command: command_key("python -m pytest", &table),
        signature: "No module named 'click'".into(),
    };
    routes.push(click);
    for probe in ["python", "python3"] {
        let mut a = route(
            &format!("instinct-u11-{probe}-click"),
            Trigger::AfterFailure { command: command_key(probe, &table), signature: "No module named 'click'".into() },
            Step::Advise {
                template: "project-python".into(),
                slots: [("interpreter".to_string(), ".venv/bin/python".to_string())].into(),
            },
            vec![],
            Effect::Observe,
            Strength::Prime,
        );
        a.state = RouteState::Forming;
        a.exit = vec![];
        a.evidence.provenance = "instinct:core@1.0.0".into();
        routes.push(a);
    }
    let t = track("github-com-pallets-click/instincts", "instinct:core@1.0.0", routes);
    adage_tracks::io::validate(&t).unwrap();
    let index = Index::compile(&[t]);
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
    let first = |call: Call| {
        let (mut st, mut fast_st) = (SessionState::default(), SessionState::default());
        decide_both(&ctx, &mut st, &mut fast_st, &Event::Failure { call })
    };
    let missing = "Traceback (most recent call last):\n  File \"<string>\", line 1, in <module>\nModuleNotFoundError: No module named 'click'";
    for cmd in ["python -c \"import click; print(click.__version__)\"", "python repro.py", "python3 repro_issue.py --x"]
    {
        let d = first(failed(cmd, missing));
        assert_eq!(d.directives.len(), 1, "{cmd}: {:?}", d.directives);
        assert_eq!(
            d.directives[0].text,
            "That Python is not this repository's environment, which has the project and its test dependencies installed: run Python here as .venv/bin/python."
        );
    }
    // A pytest run that cannot import the project: the run step answers it, without the advice.
    let d = first(failed("python -m pytest tests/test_basic.py -x", missing));
    assert_eq!(d.directives.iter().map(|x| x.route.as_str()).collect::<Vec<_>>(), ["dead-end-click"]);
    // The project's own interpreter, or another module missing: nothing.
    assert!(first(failed(".venv/bin/python repro.py", missing)).directives.is_empty());
    assert!(first(failed("python repro.py", "ModuleNotFoundError: No module named 'requests'")).directives.is_empty());
}

/// Bug 2 (launch report): `python -m pytest … 2>&1 | head -100` exits 0 even when pytest could not
/// start, so no failure event fired and U11 never saw the dead end (22 of 63 Haiku sessions that
/// hit it; the zero-config E2E's only opportunity). A piped run of the monitored runner whose
/// output holds the signature fires the instinct; a piped run that works, an unpiped exit 0, or
/// the signature in the output of a command that is not the runner fires nothing.
#[test]
fn a_piped_failure_fires_its_dead_end() {
    let dir = tempfile::tempdir().unwrap();
    let table = CommandTable::load();
    let mut routes = u11_routes();
    routes.push(route(
        "gen",
        Trigger::AfterEdit { paths: "schema/*.json".into() },
        Step::Run { command: "make gen".into() },
        vec![],
        Effect::Regenerate,
        Strength::Fire,
    ));
    let index = Index::compile(&[track("acme/t", "instinct:core@1.0.0", routes)]);
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
    let run = |call: Call| {
        let (mut st, mut fast_st) = (SessionState::default(), SessionState::default());
        let d = decide_both(&ctx, &mut st, &mut fast_st, &Event::Batch { calls: vec![call] });
        d.directives.iter().map(|x| x.route.clone()).collect::<Vec<_>>()
    };
    // The zero-config E2E's confined session 2, as the agent ran it.
    let piped = bash(
        "cd /tmp/click && python -m pytest tests/test_basic.py -x 2>&1 | head -100",
        "/usr/bin/python: No module named pytest\n",
    );
    assert_eq!(run(piped), ["dead-end-0cdcfb"]);
    assert!(run(bash("python -m pytest tests -x 2>&1 | tail -5", "==== 12 passed in 0.4s ====")).is_empty());
    assert!(run(bash("python -m pytest tests -x", "No module named pytest")).is_empty());
    assert!(run(bash("cat notes.txt | head", "No module named pytest")).is_empty());
}

// ---------------------------------------------------------------- TurnStep: predicted steps

/// The lookup, its linked files and the finish line of [`fixture`], plus `extra`, in one track
/// `acme/t`.
fn predict_fixture(extra: Vec<Route>) -> (tempfile::TempDir, Index, CommandTable) {
    let (dir, base, table) = fixture();
    let mut routes: Vec<Route> =
        base.routes.iter().filter(|r| r.route.id != "dead-unittest").map(|r| r.route.clone()).collect();
    routes.extend(extra);
    (dir, Index::compile(&[track("acme/t", "desire-path", routes)]), table)
}

fn predict_ctx<'a>(dir: &'a tempfile::TempDir, index: &'a Index, table: &'a CommandTable, grants: &[&str]) -> Ctx<'a> {
    Ctx {
        checkout: &crate::checkout::Disk,
        index,
        repo: "acme".into(),
        root: dir.path(),
        table,
        embed: None,
        params: Default::default(),
        policy: Policy {
            predict_grants: grants.iter().map(|r| ("acme/t".to_string(), r.to_string())).collect(),
            ..Default::default()
        },
    }
}

fn step(turn: &str, index: u32, ending: bool) -> Event {
    Event::TurnStep { turn_id: turn.into(), index, model: Some("claude-x".into()), agent: None, ending }
}

fn by_adage(call: Call, id: &str) -> Call {
    Call { by: By::Adage, tool_use_id: Some(id.into()), ..call }
}

fn pending(route: &str, command: &str, table: &CommandTable) -> Pending {
    Pending {
        route: route.into(),
        expect_key: command_key(command, table),
        expect_head: String::new(),
        batches_left: 3,
        command: command.into(),
    }
}

fn armed(route: &str, command: &str, table: &CommandTable) -> Obligation {
    Obligation {
        route: route.into(),
        expect_key: command_key(command, table),
        command: command.into(),
        paths: "schema/*.json".into(),
        ..Default::default()
    }
}

/// T-C1-01: a delivered directive is predicted with its rendered command, under a grant; the
/// call Adage then makes is judged `predicted`, never `followed`, and retires the follow
/// judgement. An armed obligation is predicted at the step that ends the turn.
#[test]
fn turn_step_predicts_a_delivered_directive() {
    let (dir, index, table) = predict_fixture(vec![]);
    let ctx = predict_ctx(&dir, &index, &table, &["lookup", "gen"]);
    let mut st = SessionState { arm: Some(Arm::Treated), ..Default::default() };
    decide(&ctx, &mut st, &Event::Prompt { text: "Pick up W-2042 please".into() });
    assert_eq!(st.pending[0].command, "./scripts/wi-lookup W-2042 --json", "the rendered command is kept");
    let d = decide(&ctx, &mut st, &step("t", 1, false));
    let p = d.predicted.clone().expect("the lookup is predicted");
    assert_eq!((p.kind, p.route.as_str(), p.track.as_str()), (PredictionKind::Run, "lookup", "acme/t"));
    assert_eq!(
        p.calls,
        vec![PredictedCall { tool: "Bash".into(), input: json!({ "command": "./scripts/wi-lookup W-2042 --json" }) }]
    );
    assert_eq!(d.would_predict, d.predicted);
    assert_eq!((st.last_step_predicted.clone(), st.predictions), (Some(("t".into(), 1)), 1));
    assert!(d.directives.is_empty() && d.judgements.is_empty());
    // The engine runs the call Adage yielded; the batch carries it with Adage's id.
    let out = r#"{"id":"W-2042","linked_files":["acme/billing.py"]}"#;
    let call = by_adage(bash("./scripts/wi-lookup W-2042 --json", out), "adage_lookup_t_1");
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![call] });
    assert!(
        d.judgements.iter().any(|j| matches!(j, Judgement::Predicted { route } if route == "lookup")),
        "{:?}",
        d.judgements
    );
    assert!(!d.judgements.iter().any(|j| matches!(j, Judgement::Followed { .. })));
    assert!(st.pending.is_empty(), "the follow judgement is retired");
    // An armed obligation: only at the step that ends the turn.
    let mut st = SessionState {
        arm: Some(Arm::Treated),
        obligations: vec![armed("gen", "make gen", &table)],
        ..Default::default()
    };
    assert!(decide(&ctx, &mut st, &step("u", 1, false)).predicted.is_none());
    let p = decide(&ctx, &mut st, &step("u", 2, true)).predicted.expect("the finish line");
    assert_eq!((p.kind, p.calls[0].input["command"].as_str()), (PredictionKind::Obligation, Some("make gen")));
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![by_adage(bash("make gen", "ok"), "adage_gen_u_2_0")] });
    assert!(st.obligations[0].met);
    assert!(d.judgements.iter().any(|j| matches!(j, Judgement::ObligationMet { route } if route == "gen")));
    assert!(d.judgements.iter().any(|j| matches!(j, Judgement::Predicted { route } if route == "gen")));
}

/// T-C1-02: never at the turn's first step, never right after a predicted step.
#[test]
fn turn_step_never_at_index_zero_nor_twice_in_a_row() {
    let (dir, index, table) = predict_fixture(vec![]);
    let ctx = predict_ctx(&dir, &index, &table, &["lookup", "gen"]);
    let mut st = SessionState {
        arm: Some(Arm::Treated),
        pending: vec![pending("lookup", "./scripts/wi-lookup W-2042 --json", &table)],
        obligations: vec![armed("gen", "make gen", &table)],
        ..Default::default()
    };
    let d = decide(&ctx, &mut st, &step("t", 0, true));
    assert!(d.predicted.is_none() && d.would_predict.is_none(), "index 0 is the prompt's request");
    assert_eq!(decide(&ctx, &mut st, &step("t", 1, true)).predicted.unwrap().route, "lookup");
    let d = decide(&ctx, &mut st, &step("t", 2, false));
    assert!(d.predicted.is_none() && d.would_predict.is_none(), "a model step separates two predictions");
    // A model step later, the next thing due (the lookup is never predicted twice).
    assert_eq!(decide(&ctx, &mut st, &step("t", 3, true)).predicted.unwrap().route, "gen");
    assert_eq!(st.predictions, 2);
    // At a turn's end the mod asks with the index of the request the model just answered: a
    // prediction before that request (index - 1) is not "in a row" with it, one at it would be.
    let mut st = SessionState { obligations: vec![armed("gen", "make gen", &table)], ..Default::default() };
    st.last_step_predicted = Some(("t".into(), 4));
    assert!(decide(&ctx, &mut st, &step("t", 5, true)).predicted.is_some(), "model step 5 came between");
    st.obligations[0].met = false;
    st.predicted_routes.clear();
    assert!(decide(&ctx, &mut st, &step("t", 5, true)).predicted.is_none(), "predicted at 5 already");
    // A new turn's step 1 after the last step of another turn is not "in a row".
    let mut st = SessionState { obligations: vec![armed("gen", "make gen", &table)], ..Default::default() };
    st.last_step_predicted = Some(("t".into(), 0));
    assert!(decide(&ctx, &mut st, &step("u", 1, true)).predicted.is_some());
}

/// T-C1-03: at most `predict_cap` per session; past it, recorded as `would_predict` only.
#[test]
fn turn_step_holds_the_session_cap() {
    let (dir, index, table) = predict_fixture(vec![]);
    let mut ctx = predict_ctx(&dir, &index, &table, &["lookup", "gen"]);
    ctx.policy.predict_cap = 1;
    let mut st = SessionState {
        pending: vec![pending("lookup", "./scripts/wi-lookup W-2042 --json", &table)],
        obligations: vec![armed("gen", "make gen", &table)],
        ..Default::default()
    };
    assert!(decide(&ctx, &mut st, &step("t", 1, true)).predicted.is_some());
    let d = decide(&ctx, &mut st, &step("t", 3, true));
    assert!(d.predicted.is_none() && d.predicted_at.is_none(), "the cap holds");
    assert_eq!(d.would_predict.map(|p| p.route), Some("gen".into()));
    assert_eq!(st.predictions, 1);
    assert_eq!(Policy::default().predict_cap, 6);
}

/// T-C1-04: the control arm predicts nothing and records what it would have; the bounds count
/// in both arms.
#[test]
fn turn_step_in_the_control_arm_records_would_predict() {
    let (dir, index, table) = predict_fixture(vec![]);
    let ctx = predict_ctx(&dir, &index, &table, &["lookup"]);
    let mut st = SessionState {
        arm: Some(Arm::Control),
        pending: vec![pending("lookup", "./scripts/wi-lookup W-2042 --json", &table)],
        ..Default::default()
    };
    let d = decide(&ctx, &mut st, &step("t", 1, false));
    assert!(d.predicted.is_none());
    assert_eq!(d.would_predict.map(|p| p.route), Some("lookup".into()));
    assert_eq!((d.predicted_at, st.predictions), (Some(("t".into(), 1)), 1));
}

/// T-C1-05: no grant, no command, an inhibited route, a route already carried out by CPU
/// prediction, a subagent's step: nothing.
#[test]
fn turn_step_predicts_nothing_without_a_grant_a_command_or_past_the_interlock() {
    let dead_gen = route(
        "dead-gen",
        Trigger::SessionStart,
        Step::Avoid { avoid: "make gen".into(), instead: "make generate".into(), signature: "boom".into() },
        vec![],
        Effect::Regenerate,
        Strength::Inhibit,
    );
    let (dir, index, table) = predict_fixture(vec![dead_gen]);
    let state = || SessionState {
        pending: vec![pending("lookup", "./scripts/wi-lookup W-2042 --json", &table)],
        obligations: vec![armed("gen", "make gen", &table)],
        ..Default::default()
    };
    let nothing = |ctx: &Ctx, st: &mut SessionState, ev: &Event| {
        let d = decide(ctx, st, ev);
        d.predicted.is_none() && d.would_predict.is_none() && st.predictions == 0
    };
    // No grant (the plain `adage hook`).
    assert!(nothing(&predict_ctx(&dir, &index, &table, &[]), &mut state(), &step("t", 1, true)));
    // A grant on another route only.
    assert!(nothing(&predict_ctx(&dir, &index, &table, &["read-linked"]), &mut state(), &step("t", 1, true)));
    let ctx = predict_ctx(&dir, &index, &table, &["lookup", "gen"]);
    // A pending entry from an older state: no command kept.
    let mut st = SessionState {
        pending: vec![Pending { command: String::new(), ..pending("lookup", "x", &table) }],
        ..Default::default()
    };
    assert!(nothing(&ctx, &mut st, &step("t", 1, true)));
    // The dead end announced this session says not to run `make gen` (without it, it would be).
    let mut st = SessionState { obligations: vec![armed("gen", "make gen", &table)], ..Default::default() };
    assert!(!nothing(&ctx, &mut st, &step("t", 1, true)));
    let mut st = SessionState {
        obligations: vec![armed("gen", "make gen", &table)],
        emitted: vec!["dead-gen".into()],
        ..Default::default()
    };
    assert!(nothing(&ctx, &mut st, &step("t", 1, true)));
    // Carried out by adage-exec's CPU prediction at the batch before.
    let mut st = SessionState { predicted_routes: vec!["lookup".into(), "gen".into()], ..state() };
    assert!(nothing(&ctx, &mut st, &step("t", 1, true)));
    // A subagent's step.
    let sub = Event::TurnStep { turn_id: "t".into(), index: 1, model: None, agent: Some("a1".into()), ending: false };
    assert!(nothing(&ctx, &mut state(), &sub));
}

/// T-C1-06 and the sequence: a skill step yields the Skill tool; a sequence its first command.
#[test]
fn turn_step_predicts_a_skill_and_a_sequence_first_command() {
    // A skill step (D3), delivered after an edit; invoked only as the policy allows.
    let release = route(
        "release",
        Trigger::AfterEdit { paths: "CHANGELOG.md".into() },
        Step::Skill { skill: "release-checklist".into(), args: None },
        vec![],
        Effect::Observe,
        Strength::Prime,
    );
    let seq = route(
        "lint-then-test",
        Trigger::AfterEdit { paths: "docs/*.md".into() },
        Step::Sequence { commands: vec!["make lint".into(), "make test".into()] },
        vec![],
        Effect::Verify,
        Strength::Prime,
    );
    let (dir, index, table) = predict_fixture(vec![release, seq]);
    let mut ctx = predict_ctx(&dir, &index, &table, &["release", "lint-then-test"]);
    ctx.policy.invoke_skills = vec!["release-checklist".into()];
    let mut st = SessionState::default();
    decide(&ctx, &mut st, &Event::Batch { calls: vec![edit_call(&dir, "CHANGELOG.md")] });
    assert_eq!(st.pending[0].expect_key, "control Skill release-checklist");
    let p = decide(&ctx, &mut st, &step("t", 1, false)).predicted.unwrap();
    assert_eq!(p.kind, PredictionKind::Skill);
    assert_eq!(p.calls, vec![PredictedCall { tool: "Skill".into(), input: json!({ "skill": "release-checklist" }) }]);
    // A sequence after an edit: its first command, as delivered.
    let mut st = SessionState::default();
    let edit = Call {
        tool: "Edit".into(),
        input: json!({ "file_path": dir.path().join("docs/a.md").to_string_lossy() }),
        output: String::new(),
        failed: false,
        by: By::Agent,
        tool_use_id: None,
    };
    decide(&ctx, &mut st, &Event::Batch { calls: vec![edit] });
    assert_eq!(st.pending[0].command, "make lint");
    let p = decide(&ctx, &mut st, &step("t", 1, false)).predicted.unwrap();
    assert_eq!((p.kind, p.calls[0].input["command"].as_str()), (PredictionKind::Sequence, Some("make lint")));
}

/// A chain: the lookup's result binds the read batch's files; the next step reads them, one call
/// per file; Adage's reads retire it, judged `predicted`.
#[test]
fn turn_step_predicts_a_chain_of_reads_bound_from_the_lookup() {
    let (dir, index, table) = predict_fixture(vec![]);
    std::fs::write(dir.path().join("acme/rates.py"), "x").unwrap();
    let ctx = predict_ctx(&dir, &index, &table, &["read-linked"]);
    let mut st = SessionState::default();
    decide(&ctx, &mut st, &Event::Prompt { text: "Pick up W-2042 please".into() });
    let out = r#"{"id":"W-2042","linked_files":["acme/billing.py","../etc/passwd","acme/rates.py"]}"#;
    decide(&ctx, &mut st, &Event::Batch { calls: vec![bash("./scripts/wi-lookup W-2042 --json", out)] });
    assert_eq!(st.reads[0].files, vec!["acme/billing.py".to_string(), "acme/rates.py".into()]);
    let p = decide(&ctx, &mut st, &step("t", 1, false)).predicted.expect("the reads");
    assert_eq!((p.kind, p.route.as_str()), (PredictionKind::Chain, "read-linked"));
    let files: Vec<&str> = p.calls.iter().map(|c| c.input["file_path"].as_str().unwrap()).collect();
    let abs = |f: &str| dir.path().join(f).to_string_lossy().to_string();
    assert_eq!(files, vec![abs("acme/billing.py"), abs("acme/rates.py")]);
    assert!(p.calls.iter().all(|c| c.tool == "Read"));
    let read = |f: &str| {
        by_adage(
            Call { tool: "Read".into(), input: json!({ "file_path": abs(f) }), ..bash("", "") },
            "adage_read-linked_t_1",
        )
    };
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![read("acme/billing.py"), read("acme/rates.py")] });
    assert_eq!(
        d.judgements.iter().filter(|j| matches!(j, Judgement::Predicted { route } if route == "read-linked")).count(),
        1
    );
    assert!(st.reads.is_empty());
    // The agent's own read retires it too, with no judgement; so does the follow window.
    let mut st = SessionState::default();
    decide(&ctx, &mut st, &Event::Prompt { text: "Pick up W-2042 please".into() });
    decide(&ctx, &mut st, &Event::Batch { calls: vec![bash("./scripts/wi-lookup W-2042 --json", out)] });
    let mine = Call { tool: "Read".into(), input: json!({ "file_path": abs("acme/rates.py") }), ..bash("", "") };
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![mine] });
    assert!(st.reads.is_empty() && !d.judgements.iter().any(|j| matches!(j, Judgement::Predicted { .. })));
}

/// T-C1-07: the bounds survive a rebuild from the spool.
#[test]
fn turn_step_bounds_survive_a_rebuild() {
    let (dir, index, table) = predict_fixture(vec![]);
    let ctx = predict_ctx(&dir, &index, &table, &["lookup"]);
    let mut st = SessionState { arm: Some(Arm::Treated), repo: Some("acme".into()), ..Default::default() };
    let mut records = Vec::new();
    for ev in [Event::Prompt { text: "Pick up W-2042 please".into() }, step("t", 1, false)] {
        let d = decide(&ctx, &mut st, &ev);
        records.push(json!({ "event": "x", "kind": ev.name(), "arm": st.arm, "repo": "acme", "decision": d }));
    }
    assert_eq!(records[1]["kind"], "turn_step");
    let rebuilt = rebuild::rebuild(&records, &index, "acme", &table).unwrap();
    assert_eq!(rebuilt.predictions, 1);
    assert_eq!(rebuilt.last_step_predicted, Some(("t".into(), 1)));
    assert_eq!(rebuilt.predicted_routes, vec!["lookup".to_string()]);
    assert_eq!(rebuild::kind_of(&json!({ "event": "TurnStep" })), "turn_step");
}

/// T-C1-08: the fast path skips a step with nothing directed, never one with something due.
#[test]
fn turn_step_fast_path() {
    let (_dir, _index, table) = predict_fixture(vec![]);
    let t = adage_tracks::triggers::Triggers::default();
    let ev = step("t", 1, true);
    assert!(fast::skip(&t, &ev, "/r", || Some(SessionState::default())));
    let met = Obligation { met: true, ..armed("gen", "make gen", &table) };
    assert!(fast::skip(&t, &ev, "/r", || Some(SessionState { obligations: vec![met.clone()], ..Default::default() })));
    let due = [
        SessionState { pending: vec![pending("lookup", "x", &table)], ..Default::default() },
        SessionState { obligations: vec![armed("gen", "make gen", &table)], ..Default::default() },
        SessionState {
            reads: vec![PendingReads { route: "r".into(), files: vec!["a".into()], batches_left: 1 }],
            ..Default::default()
        },
    ];
    for st in due {
        assert!(!fast::skip(&t, &ev, "/r", || Some(st.clone())), "{st:?}");
    }
    assert!(!fast::skip(&t, &ev, "/r", || None), "an unreadable state is rebuilt, never skipped past");
}

// ---------------------------------------------------------------- the dead end that never runs

/// The mod's observe mode records a step as the control arm does, in a treated session.
#[test]
fn turn_step_in_observe_mode_records_would_predict_only() {
    let (dir, index, table) = predict_fixture(vec![]);
    let mut ctx = predict_ctx(&dir, &index, &table, &["lookup"]);
    ctx.policy.observe = true;
    let mut st = SessionState {
        arm: Some(Arm::Treated),
        pending: vec![pending("lookup", "./scripts/wi-lookup W-2042 --json", &table)],
        ..Default::default()
    };
    let d = decide(&ctx, &mut st, &step("t", 1, false));
    assert!(d.predicted.is_none());
    assert_eq!(d.would_predict.map(|p| p.route), Some("lookup".into()));
    assert_eq!((st.arm, st.predictions), (Some(Arm::Treated), 1));
}

fn after_failure_pair(strength: Strength) -> Route {
    route(
        "after-pytest",
        Trigger::AfterFailure { command: "exec python3 -m pytest".into(), signature: "No module named pytest".into() },
        Step::Run { command: "make test".into() },
        vec![],
        Effect::Verify,
        strength,
    )
}

/// T-C6-01: a firm learned `avoid` route with a fixed `instead`, or a run after a failure at fire,
/// answers a substitution; a prime pair, an instinct, a slotted or missing `instead`, a list, or
/// the `instead` itself answer none.
#[test]
fn a_learned_dead_end_is_substituted_only_from_a_firm_fixed_pair() {
    let (_dir, index, table) = fixture();
    let policy = Policy::default();
    let sub = |index: &Index, cmd: &str| substitution(index, "acme", &policy, &table, cmd);
    let s = sub(&index, "python3 -m unittest tests.test_billing -v").expect("the dead end");
    assert_eq!((s.route.as_str(), s.track.as_str(), s.command.as_str()), ("dead-unittest", "acme/t", "make test"));
    assert_eq!(
        s.context,
        "adage ran make test in place of python3 -m unittest tests.test_billing -v: in this repository it fails (ACME_ENV is not set)."
            .replace("(ACME", "(\"ACME")
            .replace("set)", "set\")")
    );
    assert!(sub(&index, "make test").is_none(), "the working command");
    assert!(sub(&index, "cd sub && python3 -m unittest").is_none(), "a list keeps what else it runs");
    assert!(sub(&index, "python3 -m unittest | tail").is_none(), "a pipeline too");
    assert!(sub(&index, "ls").is_none());

    let one = |r: Route| unlocked(&[&track("acme/x", "desire-path", vec![r])]);
    let fire = one(after_failure_pair(Strength::Fire));
    assert_eq!(sub(&fire, "python3 -m pytest tests").map(|s| s.command), Some("make test".into()));
    assert!(sub(&one(after_failure_pair(Strength::Prime)), "python3 -m pytest tests").is_none(), "prime");
    let mut forming = after_failure_pair(Strength::Fire);
    forming.state = RouteState::Forming;
    assert!(sub(&one(forming), "python3 -m pytest tests").is_none(), "forming is delivered prime");
    let mut dormant = npm_test_dead_end();
    dormant.state = RouteState::Dormant;
    assert!(sub(&one(dormant), "npm test").is_none(), "dormant");
    assert_eq!(sub(&one(npm_test_dead_end()), "npm test").map(|s| s.route), Some("dead-end-npm".into()));
    let slotted = route(
        "slotted",
        Trigger::SessionStart,
        Step::Avoid { avoid: "npm test".into(), instead: "make test-{suite}".into(), signature: "x".into() },
        vec![],
        Effect::Verify,
        Strength::Inhibit,
    );
    assert!(sub(&one(slotted), "npm test").is_none(), "slotted instead");
    let empty = route(
        "empty",
        Trigger::SessionStart,
        Step::Avoid { avoid: "npm test".into(), instead: " ".into(), signature: "x".into() },
        vec![],
        Effect::Verify,
        Strength::Inhibit,
    );
    assert!(sub(&one(empty), "npm test").is_none(), "no instead");
    let mut instinct = npm_test_dead_end();
    instinct.evidence.provenance = "instinct:core@0.1.0".into();
    assert!(sub(&one(instinct), "npm test").is_none(), "an instinct is advice only");
}

/// T-C6-02, the runtime half: treated substitutes and judges it `substituted`, never a follow;
/// control and observe mode record `would_substitute` only.
#[test]
fn a_substitution_is_its_own_judgement_and_never_in_control() {
    let (_dir, index, table) = fixture();
    let s = substitution(&index, "acme", &Policy::default(), &table, "python3 -m unittest").unwrap();
    let d = substitute(Some(Arm::Treated), false, s.clone());
    assert_eq!(d.substituted.as_ref(), Some(&s));
    assert!(matches!(d.judgements.as_slice(), [Judgement::Substituted { route }] if route == "dead-unittest"));
    let v = serde_json::to_value(&d).unwrap();
    assert_eq!(v["judgements"], json!([{ "kind": "substituted", "route": "dead-unittest" }]));
    for (arm, observe) in [(Some(Arm::Control), false), (Some(Arm::Treated), true)] {
        let d = substitute(arm, observe, s.clone());
        assert!(d.substituted.is_none() && d.judgements.is_empty());
        assert_eq!(d.would_substitute.as_ref(), Some(&s));
    }
    // A decision without either keeps its old bytes.
    assert!(serde_json::to_string(&Decision::default()).unwrap().find("substitut").is_none());
}

/// T-A1-03 (the ring itself): one decision per `PreToolUse` identity, at most 64 kept, the oldest
/// dropped first; a state written before the ring existed reads with it empty.
#[test]
fn the_pretool_ring_is_bounded_and_exact() {
    let mut st = SessionState::default();
    for id in 0..SEEN_PRETOOL as u64 {
        assert!(st.see_pretool(id), "{id} is new");
    }
    assert!(!st.see_pretool(0), "decided already");
    assert_eq!(st.seen_pretool.len(), SEEN_PRETOOL);
    assert!(st.see_pretool(64), "a 65th identity is decided");
    assert_eq!(st.seen_pretool.len(), SEEN_PRETOOL);
    assert!(!st.has_seen_pretool(0), "the oldest made room");
    assert!(st.see_pretool(0), "so it decides again");
    assert!(st.has_seen_pretool(64) && !st.has_seen_pretool(1));
    let old: SessionState = serde_json::from_str(r#"{"arm":null,"repo":null,"prompts":0,"batches":0,"emitted":[],"pending":[],"obligations":[],"history":[],"first_edit_at_batch":null}"#).unwrap();
    assert!(old.seen_pretool.is_empty() && old.standing.is_none());
    let json = serde_json::to_string(&SessionState::default()).unwrap();
    assert!(!json.contains("seen_pretool") && !json.contains("standing"), "{json}");
}

fn card_route(id: &str, files: &[&str]) -> Route {
    let card = card::Card {
        sessions: 4,
        passed: 3,
        files: files.iter().map(|f| card::CardFile { path: f.to_string(), sessions: 4 }).collect(),
        exemplars: vec![],
        commands: vec![],
        avoid: vec![],
        finish: None,
        prompts: vec![],
    };
    let trigger = Trigger::Intent { family: id.into(), model: "m".into(), centroid: vec![1], threshold: 0.9 };
    route(id, trigger, Step::Card { card }, vec![], Effect::Observe, Strength::Prime)
}

/// T-C5-01 (the text): the standing text is the repository's session-start reflexes in their
/// wrappers, then one line per procedure card; deterministic, whole items only, at most 2,000
/// bytes; the routes it carries are named.
#[test]
fn standing_text_carries_the_session_start_reflexes_and_the_cards_within_its_bound() {
    let (_dir, base, _table) = fixture();
    let mut routes: Vec<Route> = base.routes.iter().map(|r| r.route.clone()).collect();
    routes.push(card_route("card-currency", &["src/currency.py", "tests/test_<x>.py", "docs/a.md", "docs/b.md"]));
    let index = Index::compile(&[track("acme/t", "desire-path", routes.clone())]);
    let s = standing(&index, "acme", &Policy::default(), "env".into());
    assert_eq!(s.routes, ["dead-unittest"], "only what a session start announces is suppressed later");
    assert!(s.standing.starts_with("<adage-reflex track=\"acme/t@"), "{}", s.standing);
    assert!(s.standing.contains("route=\"dead-unittest\" strength=\"inhibit\" kind=\"avoid\">"), "{}", s.standing);
    assert!(s.standing.contains("running python3 -m unittest fails"), "{}", s.standing);
    assert!(
        s.standing.contains("changes that touch src/currency.py, tests/test_*.py, docs/a.md follow a known procedure, learned from 4 earlier sessions."),
        "{}",
        s.standing
    );
    assert_eq!(s.environment, "env");
    assert_eq!(standing(&index, "acme", &Policy::default(), "env".into()), s, "deterministic");
    assert!(standing(&index, "other", &Policy::default(), String::new()).standing.is_empty());
    // Many session-start reflexes: whole items only, within the bound.
    for i in 0..12 {
        routes.push(route(
            &format!("dead-{i}"),
            Trigger::SessionStart,
            Step::Avoid {
                avoid: format!("tool-{i} run"),
                instead: "make test".into(),
                signature: "no such tool".into(),
            },
            vec![],
            Effect::Verify,
            Strength::Inhibit,
        ));
    }
    let index = Index::compile(&[track("acme/t", "desire-path", routes)]);
    let s = standing(&index, "acme", &Policy::default(), String::new());
    assert!(s.standing.len() <= render::MAX_STANDING, "{}", s.standing.len());
    assert_eq!(s.standing.matches("<adage-reflex ").count(), s.standing.matches("</adage-reflex>").count());
    assert_eq!(
        s.standing.matches("<adage-reflex ").count(),
        s.routes.len() + usize::from(s.standing.contains("known procedure"))
    );
    assert!(s.routes.len() < 13, "not every reflex fits: {:?}", s.routes);
}

/// T-C5-01 (the suppression): once the standing text carries a reflex, the first prompt does not
/// announce it again, and the dead end it states still holds back what it avoids.
#[test]
fn a_reflex_in_the_standing_text_is_not_announced_again() {
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
    let prompt = Event::Prompt { text: "Pick up W-2042 please".into() };
    let mut plain = SessionState::default();
    let d = decide(&ctx, &mut plain, &prompt);
    assert!(d.directives.iter().any(|x| x.route == "dead-unittest"));
    let mut st = SessionState {
        standing: Some(standing(&index, "acme", &Policy::default(), String::new())),
        ..Default::default()
    };
    let d = decide(&ctx, &mut st, &prompt);
    assert!(!d.directives.iter().any(|x| x.route == "dead-unittest"), "{:?}", d.directives);
    assert!(d.directives.iter().any(|x| x.route == "lookup"), "the situational directive stays");
    assert!(!st.emitted.contains(&"dead-unittest".to_string()));
    assert_eq!(st.told().filter(|r| *r == "dead-unittest").count(), 1, "told through the standing text");
}

/// T-C5-04 (the binary): a step is routine only when the mod says a routine model is set, a
/// delivered `fire` directive is pending without a `predict` grant, and the arm is treated; the
/// control arm and observe mode record `would_route` only; a predicted step is never routed.
#[test]
fn routing_is_consented_and_never_in_control() {
    let prime = route(
        "prime-run",
        Trigger::SessionStart,
        Step::Run { command: "make check".into() },
        vec![],
        Effect::Verify,
        Strength::Prime,
    );
    let (dir, index, table) = predict_fixture(vec![prime]);
    let at = |grants: &[&str], routine: bool, observe: bool| {
        let mut c = predict_ctx(&dir, &index, &table, grants);
        c.policy.routine = routine;
        c.policy.observe = observe;
        c
    };
    let with = |arm: Arm, pending_routes: &[(&str, &str)]| SessionState {
        arm: Some(arm),
        pending: pending_routes.iter().map(|(r, c)| pending(r, c, &table)).collect(),
        ..Default::default()
    };
    let lookup = [("lookup", "./scripts/wi-lookup W-2042 --json")];
    let mut st = with(Arm::Treated, &lookup);
    let d = decide(&at(&[], true, false), &mut st, &step("t", 1, false));
    let want = Routing { route: "lookup".into(), track: "acme/t".into() };
    assert_eq!((d.routine.clone(), d.would_route.clone()), (Some(want.clone()), Some(want.clone())));
    assert!(d.predicted.is_none() && d.would_predict.is_none());
    // Not without the person's setting, not at the turn's first request, not for a subagent.
    assert!(decide(&at(&[], false, false), &mut with(Arm::Treated, &lookup), &step("t", 1, false))
        .would_route
        .is_none());
    assert!(decide(&at(&[], true, false), &mut with(Arm::Treated, &lookup), &step("t", 0, false))
        .would_route
        .is_none());
    let sub = Event::TurnStep { turn_id: "t".into(), index: 1, model: None, agent: Some("a".into()), ending: false };
    assert!(decide(&at(&[], true, false), &mut with(Arm::Treated, &lookup), &sub).would_route.is_none());
    // Control and observe mode: recorded, never routed.
    for (arm, observe) in [(Arm::Control, false), (Arm::Treated, true)] {
        let d = decide(&at(&[], true, observe), &mut with(arm, &lookup), &step("t", 1, false));
        assert_eq!((d.routine.clone(), d.would_route.clone()), (None, Some(want.clone())), "{arm:?} {observe}");
    }
    // A granted directive is predicted, not routed.
    let d = decide(&at(&["lookup"], true, false), &mut with(Arm::Treated, &lookup), &step("t", 1, false));
    assert!(d.predicted.is_some() && d.would_route.is_none());
    // A directive below full strength, or nothing pending: no routing.
    let d =
        decide(&at(&[], true, false), &mut with(Arm::Treated, &[("prime-run", "make check")]), &step("t", 1, false));
    assert!(d.would_route.is_none());
    assert!(decide(&at(&[], true, false), &mut with(Arm::Treated, &[]), &step("t", 1, false)).would_route.is_none());
}
