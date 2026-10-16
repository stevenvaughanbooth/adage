//! A receipt as the result (C3).

use super::*;

/// The call the mod answered with a fresh receipt is observed as made and passed: a finish line
/// the call meets, or the covering run meets, is met (so the Stop gate does not ask for a run
/// whose result is in hand), and a directive the call follows is followed.
#[test]
fn a_call_answered_by_a_receipt_meets_what_it_covers() {
    let (dir, index, table) = predict_fixture(vec![]);
    let ctx = predict_ctx(&dir, &index, table, &[]);
    let test = ".venv/bin/python -m pytest tests/test_x.py -q";
    let mut st = SessionState {
        arm: Some(Arm::Treated),
        pending: vec![pending("tests", ".venv/bin/python -m pytest tests/test_x.py", table)],
        obligations: vec![armed("check", test, table), armed("gen", "make gen", table)],
        ..Default::default()
    };
    let d = answered_by_receipt(&ctx, &mut st, ".venv/bin/python -m pytest tests/test_x.py::test_one -xvs", test);
    assert_eq!(d.observed.len(), 1, "{d:?}");
    assert!(d.judgements.iter().any(|j| matches!(j, Judgement::ObligationMet { route } if route == "check")));
    assert!(d.judgements.iter().any(|j| matches!(j, Judgement::Followed { route } if route == "tests")));
    assert!(st.obligations[0].met && !st.obligations[1].met, "only what it covers");
    assert!(st.pending.is_empty());
    let d = decide(&ctx, &mut st, &Event::Stop { stop_hook_active: false });
    assert!(d.would_continue.as_deref().is_some_and(|r| r.contains("make gen")), "the other one is still due");
}

/// `SessionState::answered` is keyed by the hash of the bare `tool_use_id`, which is what a
/// batch's call carries, and not by the event-identity hash `seen_pretool` keeps: a batch leaves
/// out a call answered under its bare id and observes one recorded under any other key. The ring
/// keeps the newest [`SEEN_PRETOOL`], each once.
#[test]
fn the_answered_ring_is_keyed_by_the_bare_tool_use_id() {
    let (dir, index, table) = predict_fixture(vec![]);
    let ctx = predict_ctx(&dir, &index, table, &[]);
    let call = |id: &str| Call { tool_use_id: Some(id.into()), ..bash("make gen", "ok") };
    let alone = decide(&ctx, &mut SessionState::default(), &Event::Batch { calls: vec![call("toolu_2")] });
    assert!(!alone.observed.is_empty());
    let mut st = SessionState::default();
    st.see_answered(identity_hash("toolu_1"));
    st.see_answered(identity_hash(r#"["claude-code","PreToolUse",["toolu_2"]]"#));
    let d = decide(&ctx, &mut st, &Event::Batch { calls: vec![call("toolu_1"), call("toolu_2")] });
    assert_eq!(d.observed, alone.observed, "toolu_1 is left out, toolu_2 is observed");

    let mut st = SessionState::default();
    for id in 0..=SEEN_PRETOOL as u64 {
        st.see_answered(id);
    }
    assert_eq!(st.answered.len(), SEEN_PRETOOL);
    assert!(!st.answered.contains(&0) && st.answered.contains(&(SEEN_PRETOOL as u64)), "the oldest made room");
    st.see_answered(1);
    assert_eq!(st.answered.len(), SEEN_PRETOOL, "an id already kept is not kept twice");
}
