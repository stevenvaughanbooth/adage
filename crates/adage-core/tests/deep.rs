//! Regression: deeply nested shell must not exhaust the stack (found by the adage-eval property
//! tests). A 15 KB `a && a && … && ls` — within `MAX_COMMAND`, and not bracket nesting, so
//! `too_deep` let it through — overflowed even an 8 MiB stack: the hook process aborted, and so
//! did `adage learn` over any history containing such a command.

use adage_core::shell::normalize_shell;
use adage_core::CommandTable;

fn on_small_stack(cmd: String) -> usize {
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || normalize_shell(&cmd, "/repo", &CommandTable::load()).len())
        .unwrap()
        .join()
        .expect("normalize_shell must not panic or overflow")
}

#[test]
fn a_long_and_chain_is_walked_iteratively() {
    let cmd = format!("{}ls", "make test && ".repeat(1200));
    assert!(cmd.len() < adage_core::shell::MAX_COMMAND);
    assert_eq!(on_small_stack(cmd), 1201, "every command in the chain is still normalised");
}

#[test]
fn other_deep_nesting_is_cut_off_not_crashed() {
    for (open, close, n) in [("! ", "", 6000), ("if true; then ", " fi;", 800), ("a | ", "", 3500), ("a || ", "", 3000)]
    {
        let cmd = format!("{}ls{}", open.repeat(n), close.repeat(n));
        assert!(on_small_stack(cmd) >= 1);
    }
}

/// Regression (found by the adage-eval property tests): tree-sitter's error recovery is quadratic
/// on some malformed input. 16 KB of `a|a|…|` took 0.6 s to normalise in a release build (5 s in
/// a debug one), and `)))…` 0.7 s: a hook has 25 ms. A pipe-count bound and a parse timeout now
/// keep every input to about the timeout (50 ms in release).
#[test]
fn pathological_input_is_normalised_quickly() {
    for cmd in ["a|".repeat(8000), ")".repeat(16000), "|".repeat(16000), ">".repeat(15000)] {
        let t = std::time::Instant::now();
        assert!(on_small_stack(cmd) >= 1);
        // Generous: debug builds on shared CI runners. Before the fix the first case took ~5 s.
        assert!(t.elapsed() < std::time::Duration::from_millis(1500), "took {:?}", t.elapsed());
    }
    // A long alternation in quotes is one argument, not a pipeline.
    let grep = format!("grep -E \"{}\" x.txt", "ab|".repeat(1000));
    assert_eq!(normalize_shell(&grep, "/repo", &CommandTable::load()).len(), 1);
}
