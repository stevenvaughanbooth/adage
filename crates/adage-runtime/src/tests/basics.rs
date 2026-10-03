//! The decision's basics: prompt routes and their slots, follow judgements, linked files,
//! finish-line obligations and the Stop request, the arms, the caps, and the checkout port.

use super::*;

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
        agent: None,
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
        agent: None,
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
        agent: None,
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
        agent: None,
    };
    decide(&ctx, &mut st, &Event::Batch { calls: vec![edit] });
    st.arm = Some(Arm::Control);
    let d = decide(&ctx, &mut st, &Event::Stop { stop_hook_active: false });
    assert!(d.continue_reason.is_none() && d.would_continue.is_some());
}
