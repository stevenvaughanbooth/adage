//! Property tests: `render::sanitise`, the last gate between track or output text and a directive.

use adage_runtime::render::sanitise;
use proptest::prelude::*;

fn cases() -> u32 {
    if cfg!(feature = "slow") {
        10_000
    } else {
        512
    }
}

fn forbidden(c: char) -> bool {
    matches!(c, '`' | '<' | '>' | '"' | '\\' | '\u{85}' | '\u{2028}' | '\u{2029}')
        || c.is_control()
        || adage_eval::safety::is_invisible(c)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(), ..ProptestConfig::default() })]

    #[test]
    fn sanitised_text_is_bounded_clean_and_stable(s in prop_oneof![
        "\\PC{0,400}",
        ".{0,400}",
        prop::collection::vec(prop::sample::select(vec!["a", " ", "`", "<", "\"", "\\", "\n", "\u{202e}", "\u{200b}", "\u{feff}", "\u{2028}", "é", "—", "$(", ";"]), 0..300).prop_map(|v| v.concat()),
    ]) {
        let out = sanitise(&s);
        prop_assert!(out.chars().count() <= 200);
        prop_assert!(!out.chars().any(forbidden), "{:?} -> {:?}", s, out);
        prop_assert_eq!(out.trim(), out.as_str());
        prop_assert_eq!(sanitise(&out), out.clone(), "idempotent");
    }
}

/// Regression (found by the adversarial eval set): right-to-left overrides and zero-width
/// characters passed through `sanitise` into directive text.
#[test]
fn invisible_characters_are_stripped() {
    assert_eq!(sanitise("not set\u{202e}tset ekam"), "not settset ekam");
    assert_eq!(sanitise("a\u{200b}b\u{feff}c\u{2066}d"), "abcd");
}
