//! The compiler's target filter refuses housekeeping (launch report, bug 6): in the zero-config
//! E2E, `run-3961d0` became a learned reflex directing `git log --oneline -5` after `source {path}`
//! and `cat`, from 8 of 9 traces. The filter is `adage_core::pattern::is_housekeeping`, which the
//! compiler's `shapes::housekeeping` calls.

use adage_graph::compile::shapes::housekeeping;

#[test]
fn housekeeping_is_never_a_target() {
    for c in [
        "git log --oneline -5",
        "git status",
        "git diff HEAD",
        "git show HEAD~1",
        "ls -la",
        "cat setup.py",
        "wc -l src/a.py",
        "head -20 x",
        "tail -n 5 y",
        "find . -name '*.py'",
        "grep -rn foo src",
        "echo ok",
        "pwd",
    ] {
        assert!(housekeeping(c), "{c:?} must never be directed");
    }
    for c in ["make gen", "git commit -m x", ".venv/bin/python -m pytest", "npm run codegen", "cargo fmt --all"] {
        assert!(!housekeeping(c), "{c:?} may be directed");
    }
}
