//! Regression: deeply nested shell must not exhaust the stack (found by the adage-eval property
//! tests). A 15 KB `a && a && … && ls` — within `MAX_COMMAND`, and not bracket nesting, so
//! `too_deep` let it through — overflowed even an 8 MiB stack: the hook process aborted, and so
//! did `adage condition` over any history containing such a command.

use adage_core::shell::normalize_shell;
use adage_core::CommandTable;

fn on_small_stack(cmd: String) -> usize {
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || normalize_shell(&cmd, "/repo", CommandTable::load()).len())
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
/// a debug one), and `)))…` 0.7 s: a hook has 25 ms. A pipe-count bound and a parse budget (bytes
/// the lexer may read) now keep every input to a few milliseconds in release.
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
    assert_eq!(normalize_shell(&grep, "/repo", CommandTable::load()).len(), 1);
}

fn notes(cmd: &str) -> Vec<String> {
    normalize_shell(cmd, "/repo", CommandTable::load()).into_iter().map(|n| n.note).collect()
}

/// The cut-off is a property of the command, not of the machine (it used to be a 50 ms timeout).
/// Each pathological command is cut off by the parse budget, never by the wall-clock backstop,
/// and a command one step below each one's cut-off is not cut off at all, however slow the machine.
#[test]
fn the_parse_budget_not_the_clock_cuts_off_pathological_input() {
    use adage_core::shell::{NOTE_BUDGET_SPENT, NOTE_NO_TREE};
    // The lexer reads about n^2/2 bytes for n copies of `)`: the budget (2^20) is spent at n = 1448.
    for (unit, n) in [(")", 16000), ("|", 16000), (">", 15000), ("<", 16000)] {
        let cmd = unit.repeat(n);
        let first = notes(&cmd);
        assert_eq!(first, [NOTE_BUDGET_SPENT], "{unit} x {n}");
        // Deterministic: the same command, the same result, run after run (the parser is reset
        // after a cut-off, so nothing carries over).
        for _ in 0..3 {
            assert_eq!(notes(&cmd), first);
        }
    }
    // Just under the budget: parsed, whatever the time it takes.
    let under = ")".repeat(1400);
    assert!(!notes(&under).iter().any(|n| n == NOTE_BUDGET_SPENT || n == NOTE_NO_TREE), "{:?}", notes(&under));
    assert_eq!(notes(&")".repeat(1500)), [NOTE_BUDGET_SPENT]);
}

/// Real commands, up to `MAX_COMMAND`, read each byte a few times at most: none comes near the
/// budget. A 14 KB heredoc (the most re-read shape measured) and a 15 KB `&&` chain parse fully.
#[test]
fn large_real_commands_are_within_the_parse_budget() {
    let heredoc = format!("cat <<EOF > notes.txt\n{}EOF", "a line of text with $VAR and `ticks`\n".repeat(380));
    let chain = format!("{}ls", "make test && ".repeat(1200));
    for cmd in [heredoc, chain] {
        assert!(cmd.len() < adage_core::shell::MAX_COMMAND);
        let ns = notes(&cmd);
        assert!(!ns.iter().any(|n| n.contains("budget") || n.contains("no tree")), "{ns:?}");
    }
}
