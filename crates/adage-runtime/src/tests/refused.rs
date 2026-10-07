//! A step Adage made in the agent's place that the harness refused before it ran (evaluation plan
//! §4.1.2; O0 live notes, defect 2).

use super::*;

const DENIED: &str =
    "<tool_use_error>File is in a directory that is denied by your permission settings.</tool_use_error>";

fn refused(call: Call) -> Call {
    Call { failed: true, output: DENIED.into(), ..call }
}

/// The live run's DENY: the predicted `Read` was refused by a deny rule, and the batch judged it
/// `predicted` and observed `read {path}`. Refused, it is judged `refused`, never `predicted`; it
/// is observed as refused; the read batch is retired and the route is not predicted again.
#[test]
fn a_refused_predicted_read_is_judged_refused_never_predicted() {
    let (dir, index, table) = predict_fixture(vec![]);
    let ctx = predict_ctx(&dir, &index, table, &["read-linked"]);
    let mut st = SessionState { arm: Some(Arm::Treated), ..Default::default() };
    let out = r#"{"id":"W-2041","linked_files":["acme/billing.py"]}"#;
    decide(&ctx, &mut st, &Event::Batch { calls: vec![bash("./scripts/wi-lookup W-2041 --json", out)] });
    let p = decide(&ctx, &mut st, &step("t", 1, false)).predicted.expect("the reads");
    let read = Call { tool: "Read".into(), input: p.calls[0].input.clone(), ..bash("", "") };
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![refused(by_adage(read, "adage_read-linked_t_1_0"))] });
    let routes = |f: fn(&Judgement) -> Option<&String>| d.judgements.iter().filter_map(f).cloned().collect::<Vec<_>>();
    assert_eq!(routes(|j| if let Judgement::Refused { route } = j { Some(route) } else { None }), vec!["read-linked"]);
    assert!(routes(|j| if let Judgement::Predicted { route } = j { Some(route) } else { None }).is_empty(), "{d:?}");
    assert!(d.observed.iter().all(|k| k.ends_with(" !refused")), "{:?}", d.observed);
    assert!(st.reads.is_empty());
    assert!(decide(&ctx, &mut st, &step("t", 3, false)).would_predict.is_none(), "not predicted again");
}

/// A refused finish line: Adage's `make gen` at the turn's end was refused, so the obligation is
/// not met (judged `refused`, never `obligation_met` or `predicted`) and the Stop gate asks for it
/// as without the mod. A refused pending command is judged the same and retired. The agent's own
/// refused call follows nothing and leaves its directive pending.
#[test]
fn a_refused_predicted_command_meets_no_obligation_and_the_stop_gate_asks() {
    let (dir, index, table) = predict_fixture(vec![]);
    std::fs::create_dir_all(dir.path().join("schema")).unwrap();
    let ctx = predict_ctx(&dir, &index, table, &["gen"]);
    let mut st = SessionState { arm: Some(Arm::Treated), ..Default::default() };
    decide(&ctx, &mut st, &Event::Batch { calls: vec![edit_call(&dir, "schema/invoice.json")] });
    assert!(decide(&ctx, &mut st, &step("t", 1, true)).predicted.is_some());
    let call = refused(by_adage(bash("make gen", ""), "adage_gen_t_1_0"));
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![call] });
    assert!(d.judgements.iter().any(|j| matches!(j, Judgement::Refused { route } if route == "gen")), "{d:?}");
    assert!(
        d.judgements.iter().all(|j| !matches!(j, Judgement::ObligationMet { .. } | Judgement::Predicted { .. })),
        "{d:?}"
    );
    assert!(!st.obligations[0].met);
    let stop = decide(&ctx, &mut st, &Event::Stop { stop_hook_active: false });
    assert!(stop.continue_reason.as_deref().is_some_and(|r| r.contains("make gen")), "{stop:?}");
    // A pending command Adage made and the harness refused: judged refused and retired.
    let mut st = SessionState { arm: Some(Arm::Treated), ..Default::default() };
    st.pending.push(pending("gen", "make gen", table));
    let d = decide(
        &ctx,
        &mut st,
        &Event::Batch { calls: vec![refused(by_adage(bash("make gen", ""), "adage_gen_t_2_0"))] },
    );
    assert!(d.judgements.iter().any(|j| matches!(j, Judgement::Refused { route } if route == "gen")), "{d:?}");
    assert!(st.pending.is_empty());
    // The agent's own refused call: no judgement, the directive stays pending.
    st.pending.push(pending("gen", "make gen", table));
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![refused(bash("make gen", ""))] });
    assert!(d.judgements.is_empty(), "{d:?}");
    assert!(st.pending.iter().any(|p| p.route == "gen"));
}
