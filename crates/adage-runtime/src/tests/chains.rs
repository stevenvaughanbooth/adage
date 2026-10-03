//! Chains and multi-call steps (C7).

use super::*;

/// T-C7-01, T-C7-02: Adage's lookup comes back with its result; the read batch it binds is
/// predicted at the very next step as one response with one Read per file (the only step that may
/// follow a prediction in a row); before the batch nothing is bound, so nothing is predicted; and
/// no third step follows without a model step between, whatever is due.
#[test]
fn a_chain_reads_what_adages_lookup_bound_at_the_next_step_and_never_a_third() {
    let (dir, index, table) = predict_fixture(vec![]);
    std::fs::write(dir.path().join("acme/rates.py"), "x").unwrap();
    let ctx = predict_ctx(&dir, &index, table, &["lookup", "read-linked", "gen"]);
    let mut st = SessionState { arm: Some(Arm::Treated), ..Default::default() };
    decide(&ctx, &mut st, &Event::Prompt { text: "Pick up W-2042 please".into() });
    assert_eq!(decide(&ctx, &mut st, &step("t", 1, false)).predicted.unwrap().route, "lookup");
    let d = decide(&ctx, &mut st, &step("t", 2, false));
    assert!(d.would_predict.is_none() && !st.chained, "no batch yet: the reads are not bound");
    let out = r#"{"id":"W-2042","linked_files":["acme/billing.py","acme/rates.py"]}"#;
    let lookup = by_adage(bash("./scripts/wi-lookup W-2042 --json", out), "adage_lookup_t_1_0");
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![lookup] });
    assert!(d.would_have.iter().any(|w| w.route == "read-linked"), "the read directive is delivered");
    assert!(st.reads[0].after_adage, "bound from Adage's own call");
    let d = decide(&ctx, &mut st, &step("t", 2, false));
    let p = d.predicted.expect("the reads, as the chain's second step");
    assert_eq!((p.kind, p.calls.len()), (PredictionKind::Chain, 2));
    assert!(p.calls.iter().all(|c| c.tool == "Read" && c.input["file_path"].as_str().unwrap().starts_with('/')));
    assert_eq!((st.predictions, st.chained, d.predicted_at), (2, true, Some(("t".into(), 2))));
    let reads: Vec<Call> = ["acme/billing.py", "acme/rates.py"]
        .iter()
        .enumerate()
        .map(|(k, f)| {
            let input = json!({ "file_path": dir.path().join(f).to_string_lossy() });
            by_adage(Call { tool: "Read".into(), input, ..bash("", "") }, &format!("adage_read-linked_t_2_{k}"))
        })
        .collect();
    let d = decide(&ctx, &mut st, &Event::Batch { calls: reads });
    let predicted: Vec<&Judgement> =
        d.judgements.iter().filter(|j| matches!(j, Judgement::Predicted { route } if route == "read-linked")).collect();
    assert_eq!(predicted.len(), 1, "one judgement for the response, whatever its calls: {:?}", d.judgements);
    // Something else is due, but a third step in a row is never predicted.
    st.pending.push(pending("gen", "make gen", table));
    let d = decide(&ctx, &mut st, &step("t", 3, false));
    assert!(d.would_predict.is_none(), "a model step first");
    let d = decide(&ctx, &mut st, &step("t", 4, false));
    assert_eq!(d.predicted.map(|p| p.route), Some("gen".into()), "after a model step");
    assert!(!st.chained);
}

/// A read batch the agent's own lookup bound waits for a model step after a prediction: only
/// Adage's own lookup chains into its reads.
#[test]
fn only_adages_own_lookup_chains_into_its_reads() {
    let (dir, index, table) = predict_fixture(vec![]);
    let ctx = predict_ctx(&dir, &index, table, &["read-linked"]);
    let reads = |after_adage| PendingReads {
        route: "read-linked".into(),
        files: vec!["acme/billing.py".into()],
        batches_left: 3,
        after_adage,
    };
    let mut st = SessionState { reads: vec![reads(false)], ..Default::default() };
    st.last_step_predicted = Some(("t".into(), 1));
    assert!(decide(&ctx, &mut st, &step("t", 2, false)).would_predict.is_none());
    assert!(decide(&ctx, &mut st, &step("t", 3, false)).would_predict.is_some(), "after a model step");
    let mut st = SessionState { reads: vec![reads(true)], ..Default::default() };
    st.last_step_predicted = Some(("t".into(), 1));
    assert!(decide(&ctx, &mut st, &step("t", 1, true)).would_predict.is_none(), "never as the same request's end");
    assert!(decide(&ctx, &mut st, &step("t", 2, false)).would_predict.is_some());
    // A rebuilt state never chains: its read batches are not rebuilt.
    let records = [json!({ "event": "x", "kind": "turn_step", "arm": "treated", "repo": "acme",
                           "decision": { "directives": [], "would_have": [], "judgements": [], "observed": [],
                                         "continue_reason": null, "predicted_at": ["t", 2],
                                         "would_predict": { "kind": "chain", "route": "read-linked", "track": "acme/t", "calls": [] } } })];
    let rebuilt = rebuild::rebuild(&records, &index, "acme", table).unwrap();
    assert!(rebuilt.reads.is_empty() && !rebuilt.chained && rebuilt.predictions == 1);
}
