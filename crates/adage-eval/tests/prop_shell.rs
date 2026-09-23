//! Property tests: the shell normaliser never panics and stays bounded on hostile input.
//!
//! The normaliser runs inside every hook on whatever command an agent wrote. It must return,
//! quickly, for any string — including ones built to be deep (nested substitutions, long `&&`
//! chains and pipelines, nested `if`s, `bash -c` inside `bash -c`) or huge. Each case runs on a
//! thread with a deliberately small stack, so unbounded recursion shows up as a failure here
//! instead of as a crash in someone's hook.

use adage_core::shell::normalize_shell;
use adage_core::CommandTable;
use proptest::prelude::*;
use std::time::{Duration, Instant};

/// The adage binary runs every command on an 8 MiB thread (on every platform); an optimised build
/// must fit in 1 MiB (Windows' main-thread default) to keep a wide margin. Unoptimised debug
/// frames are many times larger (measured on macOS: the worst generated case needs over 512 KiB
/// in debug and under 128 KiB in release, and Linux debug frames are larger still), so debug
/// builds get the production stack instead.
const STACK: usize = if cfg!(debug_assertions) { 8 << 20 } else { 1 << 20 };
const BUDGET: Duration = Duration::from_secs(2);

fn cases() -> u32 {
    if cfg!(feature = "slow") {
        2_000
    } else {
        128
    }
}

/// Normalise on a small-stack thread; fail on panic, stack overflow (the process aborts, which
/// fails the test run), or taking longer than the budget.
fn check(cmd: String) -> Result<(), TestCaseError> {
    let h = std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(move || {
            let t = Instant::now();
            let out = normalize_shell(&cmd, "/repo", &CommandTable::load());
            (t.elapsed(), out.len(), cmd.len())
        })
        .unwrap();
    match h.join() {
        Ok((took, _, len)) => {
            prop_assert!(took < BUDGET, "normalising {len} bytes took {took:?}");
            Ok(())
        }
        Err(_) => Err(TestCaseError::fail("normalize_shell panicked")),
    }
}

fn nested() -> impl Strategy<Value = String> {
    let atom = prop_oneof![
        Just("ls".to_string()),
        Just("cat a.txt".to_string()),
        Just("make test".to_string()),
        Just("echo $X".to_string()),
        "[a-z]{1,8}( -[a-z])?( [a-z./]{1,12})?",
    ];
    atom.prop_recursive(6, 256, 4, |inner| {
        prop_oneof![
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("{a} && {b}")),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("{a} | {b}")),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("{a}; {b}")),
            inner.clone().prop_map(|a| format!("echo $({a})")),
            inner.clone().prop_map(|a| format!("( {a} )")),
            inner.clone().prop_map(|a| format!("{{ {a}; }}")),
            inner.clone().prop_map(|a| format!("if true; then {a}; fi")),
            inner.clone().prop_map(|a| format!("for f in *; do {a}; done")),
            inner.clone().prop_map(|a| format!("bash -c \"{}\"", a.replace('\\', "\\\\").replace('"', "\\\""))),
            inner.clone().prop_map(|a| format!("{a} > out.txt 2>&1")),
            inner.prop_map(|a| format!("! {a}")),
        ]
    })
}

/// Constructs repeated many times: the shapes a recursive walk is most exposed to.
fn repeated() -> impl Strategy<Value = String> {
    let unit = prop_oneof![
        Just(("a && ", "")),
        Just(("a | ", "")),
        Just(("a; ", "")),
        Just(("a || ", "")),
        Just(("! ", "")),
        Just(("if true; then ", " fi;")),
        Just(("while true; do ", " done;")),
        Just(("case x in x) ", ";; esac;")),
        Just(("$(", ")")),
        Just(("(", ")")),
        Just(("{ ", "; }")),
        Just(("`", "`")),
        Just(("\"$(", ")\"")),
        Just(("x=$(", ")")),
        Just(("[[ ", " ]]")),
        Just(("$((1+", "))")),
        Just(("<(", ")")),
    ];
    (unit, 1usize..6000).prop_map(|((open, close), n)| {
        let mut s = open.repeat(n);
        s.push_str("ls");
        s.push_str(&close.repeat(n));
        s
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(), ..ProptestConfig::default() })]

    #[test]
    fn arbitrary_text_never_panics(s in "\\PC{0,400}") {
        check(s)?;
    }

    #[test]
    fn shell_shaped_text_never_panics(s in "[a-z0-9 ;|&<>()${}`'\"\\\\=./*~!#\\n-]{0,600}") {
        check(s)?;
    }

    #[test]
    fn nested_constructs_are_bounded(s in nested()) {
        check(s)?;
    }

    #[test]
    fn deep_repetition_is_bounded(s in repeated()) {
        check(s)?;
    }

    #[test]
    fn huge_input_is_bounded(n in 1usize..200_000, c in "[a-z &|;()]") {
        check(c.repeat(n))?;
    }
}
