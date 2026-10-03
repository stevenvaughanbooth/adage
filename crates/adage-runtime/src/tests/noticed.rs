//! Noticed (D2).

use super::*;

/// A repository with one context route (`make check` after `git status` then an edit of
/// acme/*.py) and one candidate (`make lint` after an edit of acme/*.py then `make test`, seen in
/// 2 traces where a reflex needs 3), or the given candidates.
fn noticing_index(candidates: Vec<adage_tracks::candidates::Candidate>) -> Index {
    let routes = vec![ctx_route(
        "check-ctx",
        vec![act_item("exec git status"), CtxItem::Edit { glob: "acme/*.py".into() }],
        3,
        Step::Run { command: "make check".into() },
        Effect::Verify,
    )];
    let t = track("acme/graph", "desire-path", routes);
    adage_tracks::io::validate(&t).unwrap();
    let mut index = Index::compile(&[t]);
    index.attach_candidates([("acme".to_string(), candidates)].into_iter().collect());
    index
}

fn lint_candidate(id: &str, last: &str, support: u32) -> adage_tracks::candidates::Candidate {
    adage_tracks::candidates::Candidate {
        id: id.into(),
        trigger: Trigger::Context {
            items: vec![CtxItem::Edit { glob: "acme/*.py".into() }, act_item(last)],
            within: 3,
        },
        step: Step::Run { command: "make lint".into() },
        support,
        s_min: 3,
        days: support,
        last_seen: Some("2026-09-09".into()),
        review: false,
    }
}

fn noticing_ctx<'a>(dir: &'a tempfile::TempDir, index: &'a Index, table: &'a CommandTable) -> Ctx<'a> {
    Ctx {
        checkout: &crate::checkout::Disk,
        index,
        repo: "acme".into(),
        root: dir.path(),
        table,
        embed: None,
        params: Default::default(),
        policy: Default::default(),
    }
}

/// T-D2-01, T-D2-02: a candidate's context notices it once per session, in both arms alike, and
/// never as a directive; a live route with the same events fires its directive and notices
/// nothing.
#[test]
fn a_candidate_is_noticed_once_never_directed_and_alike_in_both_arms() {
    let (dir, _, table) = fixture();
    let index = noticing_index(vec![lint_candidate("run-lint", "exec make test", 2)]);
    let ctx = noticing_ctx(&dir, &index, &table);
    let events = [
        Event::Batch { calls: vec![edit_call(&dir, "acme/billing.py")] },
        Event::Batch { calls: vec![bash("make test", "OK")] },
    ];
    let mut seen = Vec::new();
    for arm in [Arm::Treated, Arm::Control] {
        let (mut st, mut fast_st) = (SessionState { arm: Some(arm), ..Default::default() }, SessionState::default());
        fast_st.arm = Some(arm);
        let mut noticed = Vec::new();
        for _ in 0..2 {
            for ev in &events {
                let d = decide_both(&ctx, &mut st, &mut fast_st, ev);
                assert!(d.directives.is_empty() && d.would_have.is_empty(), "{d:?}");
                noticed.extend(d.noticed);
            }
        }
        assert_eq!(
            noticed,
            vec![Noticed { candidate: "run-lint".into(), support: 2, s_min: 3, review: false }],
            "once per session"
        );
        assert_eq!(st.noticed, vec!["run-lint".to_string()]);
        seen.push(noticed);
    }
    assert_eq!(seen[0], seen[1], "both arms record it");
    // The same pattern as a live reflex: its directive, and nothing noticed (a candidate is never
    // a route's id).
    let routes = vec![ctx_route(
        "run-lint",
        vec![CtxItem::Edit { glob: "acme/*.py".into() }, act_item("exec make test")],
        3,
        Step::Run { command: "make lint".into() },
        Effect::Verify,
    )];
    let mut live = Index::compile(&[track("acme/graph", "desire-path", routes)]);
    live.attach_candidates(
        [("acme".to_string(), vec![lint_candidate("run-lint", "exec make test", 2)])].into_iter().collect(),
    );
    assert!(live.candidates("acme").is_empty());
    let ctx = noticing_ctx(&dir, &live, &table);
    let mut st = SessionState { arm: Some(Arm::Treated), ..Default::default() };
    let mut directed = Vec::new();
    for ev in &events {
        let d = decide(&ctx, &mut st, ev);
        assert!(d.noticed.is_empty());
        directed.extend(d.directives.into_iter().map(|x| x.route));
    }
    assert_eq!(directed, vec!["run-lint".to_string()]);
}

/// T-D2-03: candidates keep a context ring of their own. A relearn that changes only candidates
/// changes their net's digest and starts their ring over, and the routes' ring is untouched (a
/// route's context still fires on the next event); at most 64 candidates are noticed per session.
#[test]
fn candidates_have_their_own_bounded_ring() {
    let (dir, _, table) = fixture();
    let before = noticing_index(vec![lint_candidate("run-lint", "exec make test", 2)]);
    let after = noticing_index(vec![lint_candidate("run-lint-2", "exec make build", 2)]);
    assert_eq!(before.contexts("acme").digest, after.contexts("acme").digest, "the routes did not change");
    assert_ne!(before.candidate_contexts("acme").digest, after.candidate_contexts("acme").digest);
    assert_ne!(before.candidate_contexts("acme").digest, before.contexts("acme").digest);
    let mut st = SessionState { arm: Some(Arm::Treated), ..Default::default() };
    decide(&noticing_ctx(&dir, &before, &table), &mut st, &Event::Batch { calls: vec![bash("git status", "")] });
    let d = decide(
        &noticing_ctx(&dir, &before, &table),
        &mut st,
        &Event::Batch { calls: vec![edit_call(&dir, "acme/a.py")] },
    );
    assert_eq!(d.directives.iter().map(|x| x.route.as_str()).collect::<Vec<_>>(), vec!["check-ctx"]);
    assert_eq!(st.cand_ctx.alphabet, before.candidate_contexts("acme").digest);
    assert!(d.cand_ctx.is_some(), "the decision records the candidate ring it changed");
    let ctx_before = st.ctx.clone();
    // Relearned: the next edit pushes onto a fresh candidate ring, the routes' ring goes on.
    let ctx = noticing_ctx(&dir, &after, &table);
    decide(&ctx, &mut st, &Event::Batch { calls: vec![bash("git status", "")] });
    assert_eq!(st.ctx.alphabet, ctx_before.alphabet);
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![edit_call(&dir, "acme/b.py")] });
    assert_eq!(st.cand_ctx.alphabet, after.candidate_contexts("acme").digest, "started over");
    assert_eq!(st.cand_ctx.ring.len(), 1);
    assert!(st.emitted.contains(&"check-ctx".to_string()) && d.directives.is_empty(), "said once already");
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![bash("make build", "")] });
    assert_eq!(d.noticed.iter().map(|n| n.candidate.as_str()).collect::<Vec<_>>(), vec!["run-lint-2"]);
    // Bounded.
    let mut full = SessionState { noticed: (0..MAX_NOTICED).map(|i| format!("c{i}")).collect(), ..Default::default() };
    for ev in [
        Event::Batch { calls: vec![edit_call(&dir, "acme/c.py")] },
        Event::Batch { calls: vec![bash("make build", "")] },
    ] {
        assert!(decide(&ctx, &mut full, &ev).noticed.is_empty());
    }
    assert_eq!(full.noticed.len(), MAX_NOTICED);
}

/// T-D2-04 (the runtime's half): the candidate ring and what was noticed are restored from the
/// spool, and a ring recorded under another candidate alphabet is dropped.
#[test]
fn noticed_and_the_candidate_ring_are_rebuilt_from_the_spool() {
    let (dir, _, table) = fixture();
    let index = noticing_index(vec![lint_candidate("run-lint", "exec make test", 2)]);
    let ctx = noticing_ctx(&dir, &index, &table);
    let mut st = SessionState { arm: Some(Arm::Treated), repo: Some("acme".into()), ..Default::default() };
    let mut records = Vec::new();
    for ev in [
        Event::Batch { calls: vec![edit_call(&dir, "acme/billing.py")] },
        Event::Batch { calls: vec![bash("make test", "OK")] },
    ] {
        let d = decide(&ctx, &mut st, &ev);
        records.push(
            json!({ "event": "PostToolBatch", "kind": "batch", "arm": "treated", "repo": "acme", "decision": d }),
        );
    }
    assert_eq!(records[1]["decision"]["noticed"][0]["candidate"], "run-lint");
    let rebuilt = rebuild::rebuild(&records, &index, "acme", &table).unwrap();
    assert_eq!((rebuilt.cand_ctx.clone(), rebuilt.noticed.clone()), (st.cand_ctx.clone(), st.noticed.clone()));
    // The rebuilt session does not notice it again.
    let mut again = rebuilt;
    for ev in [
        Event::Batch { calls: vec![edit_call(&dir, "acme/billing.py")] },
        Event::Batch { calls: vec![bash("make test", "OK")] },
    ] {
        assert!(decide(&ctx, &mut again, &ev).noticed.is_empty());
    }
    let relearned = noticing_index(vec![lint_candidate("run-lint-2", "exec make build", 2)]);
    let rebuilt = rebuild::rebuild(&records, &relearned, "acme", &table).unwrap();
    assert!(rebuilt.cand_ctx.is_empty() && rebuilt.noticed == vec!["run-lint".to_string()]);
}

/// The control arm records the same chain: the agent's own lookup, where Adage would have made
/// it, binds the reads Adage would read at once, so `would_predict` counts the steps the treated
/// arm makes.
#[test]
fn the_control_arm_records_the_chain_the_treated_arm_makes() {
    let (dir, index, table) = predict_fixture(vec![]);
    let ctx = predict_ctx(&dir, &index, &table, &["lookup", "read-linked"]);
    let mut st = SessionState { arm: Some(Arm::Control), ..Default::default() };
    decide(&ctx, &mut st, &Event::Prompt { text: "Pick up W-2042 please".into() });
    let d = decide(&ctx, &mut st, &step("t", 1, false));
    assert_eq!((d.predicted.is_none(), d.would_predict.map(|p| p.route)), (true, Some("lookup".into())));
    let out = r#"{"id":"W-2042","linked_files":["acme/billing.py"]}"#;
    decide(&ctx, &mut st, &Event::Batch { calls: vec![bash("./scripts/wi-lookup W-2042 --json", out)] });
    assert!(st.reads[0].after_adage, "the agent's call stood where Adage's would have");
    let d = decide(&ctx, &mut st, &step("t", 2, false));
    assert_eq!(d.would_predict.map(|p| p.kind), Some(PredictionKind::Chain));
    assert!(d.predicted.is_none() && st.chained && st.predictions == 2);
}
