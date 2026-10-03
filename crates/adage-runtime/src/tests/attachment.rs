//! Attachment by capability (C9).

use super::*;

/// T-C9-02: the standing reflexes go to a cached section only where there is one, the environment
/// sentence to the tool's description only where it can carry one, and a subagent is told the
/// conventions its prompt binds only where its prompt can be written; never in control.
#[test]
fn what_is_attached_follows_the_harness_capabilities() {
    let (dir, index, table) = fixture();
    let ctx = predict_ctx(&dir, &index, &table, &[]);
    let s = Standing { standing: "reflexes".into(), environment: "env".into(), routes: vec!["dead-unittest".into()] };
    let hooks = adage_trace::Harness::ClaudeCode.capabilities();
    let with_mod = adage_trace::Harness::ClaudeCode.capabilities_with_mod();
    assert_eq!(standing_for(&hooks, s.clone()), Standing::default());
    assert_eq!(standing_for(&with_mod, s.clone()), s);
    let describe_only = adage_trace::Capabilities { tool_description: true, ..hooks };
    assert_eq!(standing_for(&describe_only, s.clone()), Standing { environment: "env".into(), ..Default::default() });
    let st = SessionState { arm: Some(Arm::Treated), ..Default::default() };
    let prompt = "Find out what W-2042 asks for and which files it names.";
    let told = delegation(&ctx, &st, prompt, &with_mod);
    assert!(told.contains("./scripts/wi-lookup W-2042 --json"), "the lookup bound to the delegation's item: {told}");
    assert!(told.contains("<adage-reflex") && told.contains("make test"), "and the standing dead end: {told}");
    assert!(told.len() <= render::MAX_STANDING);
    assert_eq!(delegation(&ctx, &st, prompt, &hooks), "", "no subagent prompt to write");
    let control = SessionState { arm: Some(Arm::Control), ..Default::default() };
    assert_eq!(delegation(&ctx, &control, prompt, &with_mod), "", "never in control");
    let unbound = delegation(&ctx, &st, "Summarise the billing module.", &with_mod);
    assert!(!unbound.contains("wi-lookup") && unbound.contains("make test"), "{unbound}");
}
