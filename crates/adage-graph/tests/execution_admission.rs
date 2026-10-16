//! Execution-only admission (`adage_graph::score`) on the committed datasets: a candidate kept
//! for execution judgement alone (absorbed by closure, or below the advisory contrast or value)
//! has a context of changes, no command, and a positive contrast. The datasets hold candidates
//! each rule turns away, so the check is not vacuous.
mod support;

use adage_graph::select::{candidates_shaped, Params};
use std::collections::BTreeMap;

#[test]
fn execution_only_candidates_are_changes_with_contrast() {
    let mut turned_away: BTreeMap<String, usize> = BTreeMap::new();
    for name in
        ["demo-haiku45", "adversarial", "synthetic", "live-20260928-py-clinic-s12", "live-20260928-acme-billing"]
    {
        let Some(sessions) = support::dataset(name) else { continue };
        for (text, _, sc) in candidates_shaped(&sessions, &Params::demo()) {
            if sc.gate.starts_with("execution-only") {
                let rule = sc.gate.split(|c: char| c.is_ascii_digit()).next().unwrap_or_default().trim().to_string();
                *turned_away.entry(rule).or_default() += 1;
            }
            if !(sc.passes && sc.execution_only) {
                continue;
            }
            let context = text.split(" || ").next().unwrap_or_default();
            assert!(
                !context.contains("[act/"),
                "{name}: a context with a command admitted for execution alone: {text}"
            );
            assert!(sc.contrast > 0.0, "{name}: a context with no contrast admitted for execution alone: {text}");
        }
    }
    for rule in [
        "execution-only context holds a command",
        "execution-only context edits one file twice",
        "execution-only contrast",
        "execution-only prefix contrast",
    ] {
        assert!(turned_away.get(rule).is_some_and(|n| *n > 0), "{rule} never applied: {turned_away:?}");
    }
}
