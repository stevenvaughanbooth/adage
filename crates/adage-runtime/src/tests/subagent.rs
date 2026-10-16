//! A subagent's calls and the main loop's predictions (the Claude Code mod's architecture §3.8;
//! O0 live notes, defect 1).

use super::*;

fn sub(call: Call) -> Call {
    Call { agent: Some("af2d43c8c64650e89".into()), ..call }
}

/// The live run's SUB: an Explore subagent ran the lookup; its batch bound the read batch, and
/// the main loop's turn end then predicted the reads into the main thread. A subagent's lookup
/// binds nothing the main loop predicts from: the read directive is said (to the subagent), but no
/// read batch, pending command or obligation is left behind, so the main loop's next step and
/// turn end predict nothing. The same lookup from the main loop binds them, as before.
#[test]
fn a_subagents_lookup_binds_no_prediction_in_the_main_thread() {
    let (dir, index, table) = predict_fixture(vec![]);
    let ctx = predict_ctx(&dir, &index, table, &["lookup", "read-linked", "gen"]);
    let out = r#"{"id":"W-2041","linked_files":["acme/billing.py"]}"#;
    let lookup = || bash("./scripts/wi-lookup W-2041 --json", out);
    let mut st = SessionState { arm: Some(Arm::Treated), ..Default::default() };
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![sub(lookup())] });
    assert!(d.would_have.iter().any(|w| w.route == "read-linked"), "said to the subagent: {d:?}");
    assert!(d.observed.iter().any(|k| k.contains("wi-lookup")), "observed: {d:?}");
    assert!(st.reads.is_empty() && st.pending.is_empty() && st.obligations.is_empty(), "{st:?}");
    for ending in [false, true] {
        let d = decide(&ctx, &mut st, &step("t", 1, ending));
        assert!(d.would_predict.is_none() && d.predicted.is_none(), "{d:?}");
    }
    // The main loop's own lookup binds the reads, and they are predicted.
    let mut st = SessionState { arm: Some(Arm::Treated), ..Default::default() };
    decide(&ctx, &mut st, &Event::Batch { calls: vec![lookup()] });
    assert_eq!(st.reads.len(), 1);
    assert_eq!(decide(&ctx, &mut st, &step("t", 1, false)).predicted.map(|p| p.route), Some("read-linked".into()));
}

/// A subagent's calls leave what the main loop was told alone: they follow none of its pending
/// directives, retire none of its read batches, meet none of its obligations and count none of
/// its follow windows down; a subagent's edit arms no obligation and its failure binds nothing.
/// Each is still observed.
#[test]
fn a_subagents_calls_leave_the_main_loops_directives_and_obligations_alone() {
    let (dir, index, table) = predict_fixture(vec![]);
    std::fs::create_dir_all(dir.path().join("schema")).unwrap();
    let ctx = predict_ctx(&dir, &index, table, &["gen"]);
    let mut st = SessionState { arm: Some(Arm::Treated), ..Default::default() };
    st.pending.push(pending("gen", "make gen", table));
    st.reads.push(PendingReads {
        route: "read-linked".into(),
        files: vec!["acme/billing.py".into()],
        batches_left: 3,
        after_adage: false,
    });
    st.obligations.push(armed("gen", "make gen", table));
    let before = (st.pending.clone(), st.reads.clone(), st.obligations.clone());
    let read = Call {
        tool: "Read".into(),
        input: json!({ "file_path": dir.path().join("acme/billing.py").to_string_lossy() }),
        ..bash("", "x")
    };
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![sub(bash("make gen", "ok")), sub(read)] });
    assert!(d.judgements.is_empty(), "nothing judged: {d:?}");
    assert!(d.observed.iter().any(|k| k.contains("make gen")), "{d:?}");
    assert_eq!(st.pending[0].batches_left, before.0[0].batches_left, "the window counts the main loop's batches");
    let routes: Vec<&str> = st.pending.iter().map(|p| p.route.as_str()).collect();
    assert_eq!((routes, &st.reads, &st.obligations), (vec!["gen"], &before.1, &before.2));
    // A subagent's edit arms nothing; the main loop's does.
    let mut st = SessionState { arm: Some(Arm::Treated), ..Default::default() };
    decide(&ctx, &mut st, &Event::Batch { calls: vec![sub(edit_call(&dir, "schema/invoice.json"))] });
    assert!(st.obligations.is_empty() && st.pending.is_empty(), "{st:?}");
    decide(&ctx, &mut st, &Event::Batch { calls: vec![edit_call(&dir, "schema/invoice.json")] });
    assert_eq!(st.obligations.len(), 1);
    // A subagent's failure judges none of the main loop's directives.
    st.pending.push(pending("gen", "make gen", table));
    let d = decide(&ctx, &mut st, &Event::Failure { call: sub(failed_bash("make gen", "Error 1")) });
    assert!(d.judgements.is_empty() && st.pending.iter().any(|p| p.route == "gen"), "{d:?}");
}
