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
    for (open, close, n) in [("! ", "", 6000), ("if true; then ", " fi;", 800), ("a | ", "", 3500), ("a || ", "", 3000)] {
        let cmd = format!("{}ls{}", open.repeat(n), close.repeat(n));
        assert!(on_small_stack(cmd) >= 1);
    }
}
