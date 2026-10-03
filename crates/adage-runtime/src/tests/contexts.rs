//! Context triggers (Track G G4).

use super::*;

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
