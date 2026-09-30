//! Command variants (Track G §6.1): which one concrete command a continuation item directs when
//! agents ran several spellings of it.
//!
//! An act item stands for a **family**: its exact members, which share one HeadSub identity
//! (`cargo test`), with how often each ran. Agents narrow a repository's check to what they are
//! working on (`cargo test -p adage-cli`, `cargo test --workspace --locked -- ingest`) or add
//! options that change nothing it checks (`-j4`, `--no-fail-fast`). The directed command is the
//! family's **covering variant** W: a member that
//!
//! - agents actually ran successfully, with `min_sessions` supporting traces (only reviewed
//!   output-verbosity variants of direct project-Python pytest may union distinct session IDs);
//! - is a fixed, plain command;
//! - covers at least `dominant` of the family's runs, where W covers a variant V when V
//!   [meets](adage_core::pattern::meets) W (V is W with options added: `cargo fmt --all --check`
//!   meets `cargo fmt --all`, as the runtime's follow judgement reads it), or W is **repo-wide**
//!   and V a narrowing of it: the same identity and effect class, W's operands a prefix of V's
//!   (V selects tests or targets W runs all of), and W has no option that selects a part of the
//!   repository ([`repo_wide`]).
//!
//! The winner covers the most runs, then ran most itself, then sorts first. A family whose
//! variants disagree (`cargo test -p a` and `cargo test -p b` with no repo-wide form anyone ran)
//! has no covering variant, and directs nothing. [`covers`] is also how the judge
//! (`adage_learn::engine`) counts a trace's run of the family as evidence for W.

use adage_core::pattern::Shape;
use std::collections::BTreeSet;

/// Options that select part of the repository or of its targets, without a value (`--lib`), and
/// flags whose value selects a part (`-p <package>`). Data: extend it here.
const NARROWING_FLAGS: &[&str] = &["--lib", "--bins", "--tests", "--benches", "--examples", "--doc"];

/// Flags whose value selects a part of the repository (`-p adage-cli`, `-C packages/a`,
/// `--test gates`, `-k name`, `-run Name`).
const NARROWING_VALUE_FLAGS: &[&str] = &[
    "-p",
    "--package",
    "--test",
    "--bin",
    "--example",
    "--bench",
    "--manifest-path",
    "-C",
    "--dir",
    "--directory",
    "--prefix",
    "-w",
    "--workspace",
    "--filter",
    "-F",
    "-k",
    "-m",
    "-run",
    "--project",
    "--cwd",
];

fn is_path_like(t: &str) -> bool {
    t.contains('/') || (t.contains('.') && !t.starts_with('-'))
}

/// Whether an option selects part of the repository (`-p adage-cli`, `--lib`, `-C packages/a`).
fn narrowing(o: &str) -> bool {
    let mut parts = o.splitn(2, [' ', '=']);
    let flag = parts.next().unwrap_or_default();
    match parts.next() {
        // `--workspace` alone widens; `-w <name>` (npm) selects.
        Some(v) => NARROWING_VALUE_FLAGS.contains(&flag) && !v.chars().all(|c| c.is_ascii_digit()) && v != "{n}",
        None => NARROWING_FLAGS.contains(&flag),
    }
}

/// Whether a command, as a [`Shape`], runs its identity over the whole repository: no option
/// selects a part of it (a package, a directory, a test target), and no operand names a path.
pub fn repo_wide(s: &Shape) -> bool {
    !s.options.iter().any(|o| narrowing(o)) && !s.positionals.iter().any(|p| is_path_like(p) || p.starts_with('{'))
}

/// Whether operand `w` selects at least what `v` does: the same operand (a slot matches any), or
/// a directory `v` lies under (`tests` or `tests/` for `tests/test_core.py::test_x`).
fn contains(w: &str, v: &str) -> bool {
    w == v || slot(w) || {
        let dir = w.trim_end_matches('/');
        !dir.is_empty()
            && !dir.starts_with('-')
            && v.strip_prefix(dir).is_some_and(|rest| rest.starts_with('/') || rest.starts_with("::"))
    }
}

fn slot(t: &str) -> bool {
    t.len() >= 2 && t.starts_with('{') && t.ends_with('}')
}

/// Whether two options are the same, a slot matching any value (`-p {part}` and `-p adage-cli`).
fn same_option(a: &str, b: &str) -> bool {
    let (fa, va) = a.split_once([' ', '=']).unwrap_or((a, ""));
    let (fb, vb) = b.split_once([' ', '=']).unwrap_or((b, ""));
    fa == fb && (va == vb || slot(va) || slot(vb))
}

/// Whether directed command `w` covers observed command `v` (see the module documentation).
/// `same_effect` says whether the two have the same effect class (a narrowing must).
///
/// A narrowing keeps `w`'s identity and every option of `w` that selects a part (so
/// `cargo test -p {part}` covers `cargo test -p {part} --test cli`, and never another package's
/// run), and each of `w`'s operands selects at least what `v`'s does (`pytest tests` covers
/// `pytest tests/test_core.py::test_x -xvs`); options that select nothing (`-j4`, `-q`,
/// `--locked`) may differ.
pub fn covers(w: &Shape, v: &Shape, same_effect: bool) -> bool {
    if w.met_by(v) {
        return true;
    }
    same_effect
        && w.same_identity(v)
        && w.options.iter().filter(|o| narrowing(o)).all(|o| v.options.iter().any(|x| same_option(o, x)))
        && v.positionals.len() >= w.positionals.len()
        && w.positionals.iter().zip(&v.positionals).all(|(a, b)| contains(a, b))
}

/// A command line read as it was typed, operands untemplated (`tests/test_x.py` stays itself, so
/// one test file's run is not another's).
pub fn shape(command: &str) -> Option<Shape> {
    let toks: Vec<String> = command.split_whitespace().map(str::to_string).collect();
    Shape::of_tokens(&toks)
}

/// Whether the directed command line `directed` covers the observed command line `observed`
/// (both plain command lines; effect classes compared by the caller).
pub fn covers_command(directed: &str, observed: &str, same_effect: bool) -> bool {
    match (shape(directed), shape(observed)) {
        (Some(w), Some(v)) => covers(&w, &v, same_effect),
        _ => directed == observed,
    }
}

/// One member of a family: a command line agents ran (a literal of one of the item's keys), how
/// often, in how many traces it succeeded, and its effect class.
#[derive(Debug, Clone)]
pub struct Variant {
    /// The command key it was run under (`exec python -m pytest {path} -x`).
    pub key: String,
    /// The command line itself (`python -m pytest tests/test_core.py -x`).
    pub command: String,
    pub runs: u32,
    pub ok_sessions: u32,
    /// Identity-bearing evidence is required to union equivalent literals without double counting.
    pub ok_session_ids: BTreeSet<String>,
    pub effect: adage_core::Effect,
    /// A fixed, plain command (no placeholder, nothing quoted, safe to direct).
    pub directable: bool,
}

/// The covering variant of a family (see the module documentation): its key and command line,
/// and the share of the family's runs it covers.
pub fn covering(family: &[Variant], min_sessions: u32, dominant: f64) -> Option<(String, String, f64)> {
    let total: u32 = family.iter().map(|v| v.runs).sum();
    if total == 0 {
        return None;
    }
    let shapes: Vec<Option<Shape>> = family.iter().map(|v| shape(&v.command)).collect();
    let mut best: Option<(u32, u32, &Variant)> = None;
    for (i, w) in family.iter().enumerate() {
        let Some(ws) = &shapes[i] else { continue };
        let equivalent_sessions: BTreeSet<&String> = family
            .iter()
            .filter(|v| {
                v.effect == w.effect
                    && adage_core::execution_equivalence::project_pytest_equivalent(&w.command, &v.command)
            })
            .flat_map(|v| v.ok_session_ids.iter())
            .collect();
        if !w.directable
            || w.ok_sessions == 0
            || (w.ok_sessions < min_sessions && equivalent_sessions.len() < min_sessions as usize)
        {
            continue;
        }
        let covered: u32 = family
            .iter()
            .zip(&shapes)
            .filter(|(v, vs)| vs.as_ref().is_some_and(|vs| covers(ws, vs, v.effect == w.effect)))
            .map(|(v, _)| v.runs)
            .sum();
        if (covered as f64) < dominant * total as f64 - 1e-9 {
            continue;
        }
        let better = match best {
            None => true,
            Some((c, r, b)) => {
                (covered, w.runs, std::cmp::Reverse(w.command.as_str())) > (c, r, std::cmp::Reverse(b.command.as_str()))
            }
        };
        if better {
            best = Some((covered, w.runs, w));
        }
    }
    best.map(|(c, _, w)| (w.key.clone(), w.command.clone(), c as f64 / total as f64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use adage_core::Effect;

    fn v(cmd: &str, runs: u32, ok: u32, effect: Effect) -> Variant {
        Variant {
            key: format!("exec {cmd}"),
            command: cmd.into(),
            runs,
            ok_sessions: ok,
            ok_session_ids: BTreeSet::new(),
            effect,
            directable: true,
        }
    }

    fn shape(c: &str) -> Shape {
        super::shape(c).unwrap()
    }

    #[test]
    fn repo_wide_forms() {
        assert!(repo_wide(&shape("cargo test --workspace --locked")));
        assert!(repo_wide(&shape("cargo clippy --workspace --all-targets --locked -- -D warnings")));
        assert!(repo_wide(&shape("cargo test -j 4")));
        assert!(!repo_wide(&shape("cargo test -p adage-cli")));
        assert!(!repo_wide(&shape("cargo test --lib")));
        assert!(!repo_wide(&shape("make -C packages/a check")));
        assert!(!repo_wide(&shape("python3 -m pytest tests/a.py")));
        assert!(repo_wide(&shape("make test")));
    }

    #[test]
    fn narrowings_are_covered_by_the_repo_wide_form() {
        let w = shape("cargo test --workspace --locked");
        assert!(covers(&w, &shape("cargo test -p adage-cli"), true));
        assert!(covers(&w, &shape("cargo test --workspace --locked -- ingest"), true));
        assert!(covers(&w, &shape("cargo test -j4 --workspace --locked --no-fail-fast"), true));
        assert!(!covers(&w, &shape("cargo build --workspace"), true));
        // A narrowing of another effect class is not the same step.
        assert!(!covers(&w, &shape("cargo test -p adage-cli"), false));
        // A narrow form covers only its own extensions.
        let n = shape("cargo test -p adage-cli");
        assert!(!covers(&n, &shape("cargo test --workspace"), true));
        assert!(covers(&n, &shape("cargo test -p adage-cli --lib"), true));
        // A check meets the rewrite it checks (the runtime's follow judgement agrees).
        assert!(covers(&shape("cargo fmt --all"), &shape("cargo fmt --all --check"), false));
        assert!(!covers(&shape("cargo fmt --all --check"), &shape("cargo fmt --all"), false));
        // Lint arguments after `--` are kept: a prefix of the variant's.
        let c = shape("cargo clippy --workspace --all-targets --locked -- -D warnings");
        assert!(covers(&c, &shape("cargo clippy -p adage-graph --all-targets --locked -- -D warnings"), true));
        assert!(!covers(&c, &shape("cargo clippy -p adage-graph"), true));
    }

    #[test]
    fn the_covering_variant_is_one_agents_ran() {
        let fam = vec![
            v("cargo test --workspace --locked", 30, 12, Effect::Verify),
            v("cargo test -p adage-cli", 10, 4, Effect::Verify),
            v("cargo test -p adage-graph --test mining", 6, 3, Effect::Verify),
            v("cargo test --workspace --locked --no-fail-fast", 4, 3, Effect::Verify),
        ];
        assert_eq!(covering(&fam, 5, 0.8).map(|x| x.1).as_deref(), Some("cargo test --workspace --locked"));
        // Too few traces ran the repo-wide form successfully: nothing is directed.
        assert_eq!(covering(&fam, 13, 0.8), None);
        // Only narrow forms, none covering the rest.
        let narrow = vec![v("cargo test -p a", 10, 6, Effect::Verify), v("cargo test -p b", 10, 6, Effect::Verify)];
        assert_eq!(covering(&narrow, 5, 0.8), None);
        // A dominant variant covers itself.
        let one = vec![v("make gen", 9, 9, Effect::Regenerate), v("make gen -j4", 1, 1, Effect::Regenerate)];
        assert_eq!(covering(&one, 5, 0.8).map(|x| x.1).as_deref(), Some("make gen"));
    }

    #[test]
    fn equivalent_pytest_support_unions_identities_not_literal_counts() {
        let variant = |flags: &str, ids: &[&str]| {
            let mut v = v(
                &format!(".venv/bin/python -m pytest tests/test_a.py {flags}"),
                ids.len() as u32,
                ids.len() as u32,
                Effect::Verify,
            );
            v.ok_session_ids = ids.iter().map(|s| s.to_string()).collect();
            v
        };
        let mut family = vec![variant("-q", &["a", "b", "c"]), variant("-v", &["c", "d", "e"])];
        let chosen = covering(&family, 5, 0.8).expect("five unique successful sessions");
        assert!(family.iter().any(|v| v.command == chosen.1));
        family[1] = variant("-v", &["a", "b", "c"]);
        assert!(covering(&family, 5, 0.8).is_none(), "six counts in three sessions cannot pass");
        for flags in ["-x", "-s", "-q -k one", "-q tests/other.py"] {
            family[1] = variant(flags, &["c", "d", "e"]);
            assert!(covering(&family, 5, 0.8).is_none(), "behavior/scope changes: {flags}");
        }
        family[1] = variant("-v", &["c", "d", "e"]);
        family[1].command = family[1].command.replace(".venv/", "other/");
        assert!(covering(&family, 5, 0.8).is_none(), "different environment");
        family[1] = variant("-v", &["c", "d", "e"]);
        family.iter_mut().for_each(|v| v.ok_session_ids.clear());
        assert!(covering(&family, 5, 0.8).is_none(), "legacy counts cannot be summed");
    }
}
