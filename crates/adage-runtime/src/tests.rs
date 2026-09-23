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
            track: TrackInfo { name: "acme/t".into(), version: "0.1.0".into(), description: String::new(), provenance: "desire-path".into() },
            scope: Scope { repo: "acme".into() },
            gauge: Gauge::default(),
        },
        routes: vec![
            route(
                "lookup",
                Trigger::Prompt { span: SpanKind::Ticket },
                Step::Run { command: "./scripts/wi-lookup {item} --json".into() },
                vec![Slot { name: "item".into(), source: SlotSource::Prompt { span: SpanKind::Ticket }, contract: Contract::Pattern { regex: r"^W-\d{4}$".into() } }],
                Effect::Observe,
                Aspect::Green,
            ),
            route(
                "read-linked",
                Trigger::AfterCommand { command: lookup_key },
                Step::ReadBatch { slot: "files".into(), max: 5 },
                vec![Slot { name: "files".into(), source: SlotSource::ResultField { field: "linked_files".into() }, contract: Contract::RepoPath }],
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
                Step::Avoid { avoid: "python3 -m unittest".into(), instead: "make test".into(), signature: "ACME_ENV is not set".into() },
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
    let ctx = Ctx { index: &index, repo: "acme".into(), root: dir.path(), table: &table };
    let mut st = SessionState::default();
    let d = decide(&ctx, &mut st, &Event::Prompt { text: "Pick up W-2042 please".into() });
    assert_eq!(d.directives.len(), 2);
    assert!(d.directives[0].text.contains("./scripts/wi-lookup W-2042 --json"));
    assert!(d.directives.iter().any(|x| x.aspect == Aspect::Red && x.text.contains("make test")));
}

#[test]
fn injected_values_never_bind() {
    let (dir, index, table) = fixture();
    let ctx = Ctx { index: &index, repo: "acme".into(), root: dir.path(), table: &table };
    let mut st = SessionState::default();
    let d = decide(&ctx, &mut st, &Event::Prompt { text: "Do W-20421234 and --config=/etc".into() });
    assert!(d.directives.iter().all(|x| x.route != "lookup"), "a value failing the contract must not bind");
}

#[test]
fn follow_judgement_and_linked_files() {
    let (dir, index, table) = fixture();
    let ctx = Ctx { index: &index, repo: "acme".into(), root: dir.path(), table: &table };
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
    let ctx = Ctx { index: &index, repo: "acme".into(), root: dir.path(), table: &table };
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
    let ctx = Ctx { index: &index, repo: "acme".into(), root: dir.path(), table: &table };
    let mut st = SessionState::default();
    let edit = Call { tool: "Write".into(), input: json!({ "file_path": dir.path().join("schema/a.json").to_string_lossy() }), output: String::new(), failed: false };
    decide(&ctx, &mut st, &Event::Batch { calls: vec![edit] });
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![bash("make gen", "ok")] });
    assert!(d.judgements.iter().any(|j| matches!(j, Judgement::ObligationMet { .. })));
    assert!(decide(&ctx, &mut st, &Event::Stop { stop_hook_active: false }).continue_reason.is_none());
}

#[test]
fn holdout_logs_exposure_but_emits_nothing() {
    let (dir, index, table) = fixture();
    let ctx = Ctx { index: &index, repo: "acme".into(), root: dir.path(), table: &table };
    let mut st = SessionState { arm: Some(Arm::Holdout), ..Default::default() };
    let d = decide(&ctx, &mut st, &Event::Prompt { text: "W-2042".into() });
    assert!(d.directives.is_empty());
    assert!(!d.would_have.is_empty());
}

#[test]
fn other_repos_get_nothing() {
    let (dir, index, table) = fixture();
    let ctx = Ctx { index: &index, repo: "other".into(), root: dir.path(), table: &table };
    let mut st = SessionState::default();
    assert!(decide(&ctx, &mut st, &Event::Prompt { text: "W-2042".into() }).directives.is_empty());
}

#[test]
fn caps_hold() {
    let (dir, index, table) = fixture();
    let ctx = Ctx { index: &index, repo: "acme".into(), root: dir.path(), table: &table };
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
    let ctx = Ctx { index: &index, repo: "acme".into(), root: dir.path(), table: &table };
    let mut st = SessionState::default();
    let out = "ID: W-2042\nLinked files: acme/billing.py, ../etc/passwd\nAcceptance: ok";
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![bash("./scripts/wi-lookup W-2042 --json", out)] });
    let read = d.directives.iter().find(|x| x.route == "read-linked").expect("read-batch from text output");
    assert!(read.text.contains("acme/billing.py") && !read.text.contains("passwd"));
}

#[test]
fn symlinks_out_of_the_repo_are_rejected() {
    let (dir, index, table) = fixture();
    std::os::unix::fs::symlink("/etc/hosts", dir.path().join("acme/notes.md")).unwrap();
    let ctx = Ctx { index: &index, repo: "acme".into(), root: dir.path(), table: &table };
    let mut st = SessionState::default();
    let out = r#"{"linked_files":["acme/notes.md","acme/billing.py"]}"#;
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![bash("./scripts/wi-lookup W-2042 --json", out)] });
    let read = d.directives.iter().find(|x| x.route == "read-linked").unwrap();
    assert!(!read.text.contains("notes.md") && read.text.contains("acme/billing.py"));
}

#[test]
fn obligations_rearm_after_a_new_edit_and_stop_requests_are_bounded() {
    let (dir, index, table) = fixture();
    let ctx = Ctx { index: &index, repo: "acme".into(), root: dir.path(), table: &table };
    let mut st = SessionState::default();
    let edit = |f: &str| Call { tool: "Edit".into(), input: json!({ "file_path": dir.path().join(f).to_string_lossy() }), output: String::new(), failed: false };
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
    let ctx = Ctx { index: &index, repo: "acme".into(), root: dir.path(), table: &table };
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
    let ctx = Ctx { index: &index, repo: "acme".into(), root: dir.path(), table: &table };
    let mut st = SessionState::default();
    let edit = Call { tool: "Edit".into(), input: json!({ "file_path": dir.path().join("schema/a.json").to_string_lossy() }), output: String::new(), failed: false };
    decide(&ctx, &mut st, &Event::Batch { calls: vec![edit] });
    st.arm = Some(Arm::Holdout);
    let d = decide(&ctx, &mut st, &Event::Stop { stop_hook_active: false });
    assert!(d.continue_reason.is_none() && d.would_continue.is_some());
}
