//! A learned dead end at the head of a pipeline of output filters (T-C6-06).

use super::*;

fn sub(index: &Index, cmd: &str) -> Option<Substitution> {
    substitution(index, "acme", &Policy::default(), CommandTable::load(), cmd)
}

/// T-C6-06: the first stage is replaced and the filters kept byte for byte, its own `2>&1` too;
/// the line names the stage replaced. These are run 3's commands.
#[test]
fn a_dead_end_piped_into_filters_is_replaced_and_the_filters_kept() {
    let (_dir, index, _table) = fixture();
    for (cmd, want) in [
        ("python3 -m unittest discover -s tests | tail -n 20", "make test | tail -n 20"),
        ("python3 -m unittest discover -s tests | head", "make test | head"),
        ("python3 -m unittest discover -s tests -t . 2>&1 | tail -5", "make test 2>&1 | tail -5"),
        ("python3 -m unittest tests.test_billing 2>&1 | tail -15", "make test 2>&1 | tail -15"),
        ("python3 -m unittest discover 2>&1 | head -20", "make test 2>&1 | head -20"),
        ("python3 -m unittest 2>&1 | grep -E 'FAIL|ERROR' | wc -l", "make test 2>&1 | grep -E 'FAIL|ERROR' | wc -l"),
        (
            "python3 -m unittest | tee out.log | sort | uniq -c | cat -n",
            "make test | tee out.log | sort | uniq -c | cat -n",
        ),
        ("python3  -m unittest   |  tail", "make test   |  tail"),
    ] {
        let s = sub(&index, cmd).unwrap_or_else(|| panic!("{cmd}"));
        assert_eq!(s.command, want, "{cmd}");
        assert_eq!(s.route, "dead-unittest");
    }
    let s = sub(&index, "python3 -m unittest discover -s tests -t . 2>&1 | tail -5").unwrap();
    assert_eq!(
        s.context,
        "adage ran make test for you, through the rest of your pipeline as written; python3 -m unittest discover -s \
         tests -t . was replaced because that command fails in this repository (\"ACME_ENV is not set\"). The output \
         above is from make test."
    );
    // A plain command is replaced as before: the whole call, and the line names it.
    let s = sub(&index, "python3 -m unittest discover -s tests").unwrap();
    assert_eq!(s.command, "make test");
    assert!(
        s.context.starts_with("adage ran make test for you; python3 -m unittest discover -s tests was replaced"),
        "{}",
        s.context
    );
}

/// T-C6-06: every other shape is left to run as written.
#[test]
fn a_pipeline_that_does_more_than_filter_is_never_rewritten() {
    let (_dir, index, _table) = fixture();
    for cmd in [
        // Lists: the replacement would also drop or reorder what else runs.
        "ls; python3 -m unittest discover -s tests -t . 2>&1 | tail -5",
        "python3 -m unittest discover -s tests -t . 2>&1 | head -20; ls tests",
        "cd sub && python3 -m unittest | tail",
        "python3 -m unittest | tail || true",
        "python3 -m unittest | tail &",
        "python3 -m unittest | tail\nls",
        // Redirections other than the first stage's `2>&1`.
        "python3 -m unittest > out.txt | tail",
        "python3 -m unittest 2>/dev/null | tail",
        "python3 -m unittest < in | tail",
        "python3 -m unittest | tail > out.txt",
        "python3 -m unittest | tail 2>&1",
        "python3 -m unittest |& tail",
        // Subshells, groups, expansions, assignments.
        "(python3 -m unittest) | tail",
        "{ python3 -m unittest; } | tail",
        "python3 -m unittest $(ls tests) | tail",
        "python3 -m unittest | grep \"$PAT\"",
        "ACME_ENV=1 python3 -m unittest | tail",
        // A later stage that is not an output filter, or a dead end not first.
        "python3 -m unittest | xargs echo",
        "python3 -m unittest | sed -n 1p",
        "python3 -m unittest | python3 -c 'import sys'",
        "python3 -m unittest | /usr/bin/tail",
        "echo x | python3 -m unittest",
        "ls | tail",
        "make test | tail",
        // A quoted dead end is an argument, not a command.
        "echo \"python3 -m unittest\" | tail",
        "grep 'python3 -m unittest' notes.txt | head",
        // A lone stage with `2>&1` is not a pipeline of filters.
        "python3 -m unittest 2>&1",
    ] {
        assert_eq!(sub(&index, cmd).map(|s| s.command), None, "{cmd:?}");
    }
}

/// T-C6-06: the decision a piped substitution records is the plain one's: substituted and judged
/// in the treated arm; `would_substitute` only in the control arm and in observe mode.
#[test]
fn a_piped_substitution_is_judged_as_a_plain_one() {
    let (_dir, index, _table) = fixture();
    let s = sub(&index, "python3 -m unittest discover -s tests | tail -n 20").unwrap();
    let d = substitute(Some(Arm::Treated), false, s.clone());
    assert_eq!(d.substituted.as_ref().map(|s| s.command.as_str()), Some("make test | tail -n 20"));
    assert!(matches!(d.judgements.as_slice(), [Judgement::Substituted { route }] if route == "dead-unittest"));
    for (arm, observe) in [(Some(Arm::Control), false), (Some(Arm::Treated), true)] {
        let d = substitute(arm, observe, s.clone());
        assert!(d.substituted.is_none() && d.judgements.is_empty());
        assert_eq!(d.would_substitute.as_ref(), Some(&s));
    }
}
