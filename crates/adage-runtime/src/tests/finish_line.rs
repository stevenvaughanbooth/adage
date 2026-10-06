//! The finish line (C8).

use super::*;

/// T-C8-01, T-C8-03: an armed, unmet finish line is answered at the step that ends the turn, under
/// a grant only; Adage's call meets it, so the Stop gate has nothing to ask. If the call fails,
/// it is not predicted again for that arming and the Stop gate asks as it always has; met, and
/// armed again by a new edit, it may be predicted again.
#[test]
fn the_finish_line_is_predicted_once_per_arming_and_the_stop_gate_stays_the_fallback() {
    let (dir, index, table) = predict_fixture(vec![]);
    std::fs::create_dir_all(dir.path().join("schema")).unwrap();
    let edit = || Event::Batch { calls: vec![edit_call(&dir, "schema/invoice.json")] };
    let stop = Event::Stop { stop_hook_active: false };
    // Without the grant: nothing at the turn's end, and the Stop gate asks.
    let ctx = predict_ctx(&dir, &index, table, &["lookup"]);
    let mut st = SessionState { arm: Some(Arm::Treated), ..Default::default() };
    decide(&ctx, &mut st, &edit());
    assert!(decide(&ctx, &mut st, &step("t", 1, true)).would_predict.is_none());
    assert!(decide(&ctx, &mut st, &stop).continue_reason.is_some());
    // With it: Adage's call meets the obligation and the Stop gate asks nothing.
    let ctx = predict_ctx(&dir, &index, table, &["gen"]);
    let mut st = SessionState { arm: Some(Arm::Treated), ..Default::default() };
    decide(&ctx, &mut st, &edit());
    let p = decide(&ctx, &mut st, &step("t", 1, true)).predicted.expect("the finish line");
    assert_eq!((p.kind, p.route.as_str()), (PredictionKind::Obligation, "gen"));
    decide(&ctx, &mut st, &Event::Batch { calls: vec![by_adage(bash("make gen", "ok"), "adage_gen_t_1_0")] });
    assert!(st.obligations[0].met);
    let d = decide(&ctx, &mut st, &stop);
    assert!(d.continue_reason.is_none() && d.would_continue.is_none(), "met by Adage's call: nothing to ask");
    // Met: nothing more at the next turn's end.
    assert!(decide(&ctx, &mut st, &step("u", 1, true)).would_predict.is_none());
    // A new edit arms it again: predicted again, once.
    decide(&ctx, &mut st, &edit());
    assert!(!st.obligations[0].met);
    assert_eq!(decide(&ctx, &mut st, &step("v", 1, true)).predicted.map(|p| p.route), Some("gen".into()));
    // The predicted call fails: the obligation stays unmet, is not predicted again, and the
    // Stop gate asks for it as without the mod.
    let failed = by_adage(failed_bash("make gen", "make: *** [gen] Error 1"), "adage_gen_v_1_0");
    let d = decide(&ctx, &mut st, &Event::Failure { call: failed });
    assert!(d.judgements.iter().all(|j| !matches!(j, Judgement::ObligationMet { .. })));
    assert!(!st.obligations[0].met);
    assert!(decide(&ctx, &mut st, &step("v", 2, true)).would_predict.is_none(), "once per arming");
    let d = decide(&ctx, &mut st, &stop);
    assert!(d.continue_reason.as_deref().is_some_and(|r| r.contains("make gen")), "{d:?}");
}
