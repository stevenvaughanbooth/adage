//! Verify, don't count: replay every compiled command on a clean checkout, in adage-exec's
//! sandbox ([`adage_exec::replay`]), and act on what it did.
//!
//! - A directed command (a lookup with the skill's example value, a finish line, a regenerate
//!   step, each step of a sequence) must exit 0 and show what the step says it shows (`Expect OK`).
//!   One that fails on a clean checkout is **stale**: its route is dropped, and the review says
//!   so ("your skill is stale here").
//! - A forbidden command must fail. When it fails with the skill's signature, the route stands;
//!   when it fails differently, it gets a route of its own with the signature observed (`pytest`:
//!   `command not found`, not the skill's `ACME_ENV is not set`); when it works, the prohibition is
//!   stale and the runner is dropped.
//! - Each route is replayed in a fresh checkout, so no replay sees another's leftovers.
//!
//! A route that could not be replayed (no sandbox here, a tool not installed, no example value)
//! is kept, reported as not replayed, and delivered at prime at most: only a verified route may
//! fire (docs/training.md, "Strength").

use crate::compile::{Candidate, Check, Compiled, Verified};
use adage_exec::replay::{Replay, ReplayLimits, Replayed};
use adage_pack::{Shape, Strength};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;
use std::path::Path;

/// Environment directories a clean checkout lacks, lent read-only when the repository has them.
pub const LEND: [&str; 3] = [".venv", "venv", "node_modules"];

/// How replays run.
#[derive(Debug, Clone)]
pub struct Options {
    /// False: nothing is replayed (`--no-replay`); every route is reported as not replayed.
    pub enabled: bool,
    pub wall_ms: u64,
}

impl Default for Options {
    fn default() -> Self {
        Options { enabled: true, wall_ms: 300_000 }
    }
}

/// What the replays had to work with.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    /// The sandbox, in one line, or why there was none.
    pub sandbox: String,
    /// Environment directories lent to the checkouts.
    pub lent: Vec<String>,
    /// Commands run.
    pub runs: usize,
}

static SIGNATURES: Lazy<[Regex; 5]> = Lazy::new(|| {
    [
        Regex::new(r"No module named [A-Za-z0-9_.]+").unwrap(),
        Regex::new(r"RuntimeError: [^\n]{1,80}").unwrap(),
        Regex::new(r"[A-Za-z0-9_.-]+: (?:command )?not found").unwrap(),
        Regex::new(r"[A-Za-z]+Error: [^\n]{1,80}").unwrap(),
        Regex::new(r"(?i)\berror: [^\n]{1,80}").unwrap(),
    ]
});

/// A short, renderable failure signature from a command's output.
pub fn signature_of(output: &str) -> Option<String> {
    for re in SIGNATURES.iter() {
        if let Some(m) = re.find(output) {
            let s: String = m.as_str().chars().filter(|c| !"`<>\"\\{}".contains(*c)).collect();
            let s = s.trim().to_string();
            if !s.is_empty() && adage_tracks::io::safe_text(&s) {
                return Some(s);
            }
        }
    }
    None
}

/// The last non-empty line of an output, trimmed and bounded, for a report.
fn last_line(output: &str) -> String {
    let l = output.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    adage_pack::procedure::clean_text(&l.chars().take(120).collect::<String>())
}

enum Ran {
    Did(Replayed),
    /// The program is not on this machine's PATH: an agent's shell would say `command not found`.
    Missing(String),
    /// A path the command names is not in a clean checkout.
    NotInCheckout(String),
    /// The sandbox refused (a reason that says nothing about the command).
    Refused(String),
}

fn run(rp: &Replay, argv: &[String], report: &mut Report) -> Ran {
    report.runs += 1;
    match rp.run(argv) {
        Ok(r) => Ran::Did(r),
        Err(e) if e.reason.contains("not on the sanitised PATH") => Ran::Missing(argv[0].clone()),
        Err(e) if e.reason.contains("does not exist in a clean checkout") => Ran::NotInCheckout(e.reason),
        Err(e) => Ran::Refused(e.to_string()),
    }
}

/// Replay one candidate in a fresh checkout. Returns the candidates it becomes (an avoid route
/// may split by signature; a stale one becomes none) and, for a stale one, why.
fn replay_one(c: Candidate, root: &Path, o: &Options, report: &mut Report) -> (Vec<Candidate>, Option<String>) {
    let mut c = c;
    if c.checks.is_empty() {
        c.verified = Verified::NotReplayed(match c.route.shape {
            Shape::Read => "nothing to run: it reads what the lookup lists".into(),
            Shape::Advise => "nothing to run: fixed advice".into(),
            _ => "nothing to run with: the skill gives no example value".into(),
        });
        return (vec![c], None);
    }
    let mut rp = match Replay::new(root, ReplayLimits { wall_ms: o.wall_ms, ..Default::default() }) {
        Ok(r) => r,
        Err(e) => {
            c.verified = Verified::NotReplayed(format!("the checkout could not be replayed: {e}"));
            return (vec![c], None);
        }
    };
    for d in LEND {
        if rp.lend(d) && !report.lent.iter().any(|x| x == d) {
            report.lent.push(d.to_string());
        }
    }
    let mut not_replayed: Option<String> = None;
    let mut stale: Option<String> = None;
    // Avoid runners: (argv, the signature it is kept with) — or dropped.
    let mut kept: Vec<(Vec<String>, String)> = Vec::new();
    for check in c.checks.clone() {
        match check {
            Check::Works { argv, expect, regenerate } => {
                let cmd = argv.join(" ");
                match run(&rp, &argv, report) {
                    Ran::Did(r) if r.ok() => {
                        let missing: Vec<&String> = expect.iter().filter(|e| !r.output.contains(e.as_str())).collect();
                        if let Some(m) = missing.first() {
                            let why = format!("`{cmd}` exits 0 on a clean checkout, but does not show `{m}` as the skill says");
                            c.replay.push(why.clone());
                            stale.get_or_insert(why);
                            continue;
                        }
                        let mut line = format!("`{cmd}`: {}", r.summary());
                        if !expect.is_empty() {
                            line.push_str(&format!(", shows {}", expect.join(", ")));
                        }
                        if regenerate {
                            match rp.changed() {
                                Ok(ch) if ch.is_empty() => line.push_str(", leaves a clean checkout unchanged"),
                                Ok(ch) => line.push_str(&format!(
                                    ", changes {} file(s) of a clean checkout (the committed output is out of date)",
                                    ch.len()
                                )),
                                Err(_) => {}
                            }
                        }
                        c.replay.push(line);
                    }
                    Ran::Did(r) if r.timed_out => {
                        let why = format!("`{cmd}` did not finish within {} s", o.wall_ms / 1000);
                        c.replay.push(why.clone());
                        not_replayed.get_or_insert(why);
                    }
                    Ran::Did(r) => {
                        let seen = signature_of(&r.output).unwrap_or_else(|| last_line(&r.output));
                        let why = format!("`{cmd}` fails on a clean checkout ({}: {seen})", r.summary());
                        c.replay.push(why.clone());
                        stale.get_or_insert(why);
                    }
                    Ran::Missing(p) => {
                        let why = format!("`{cmd}`: {p} is not on the replay's PATH");
                        c.replay.push(why.clone());
                        not_replayed.get_or_insert(why);
                    }
                    Ran::NotInCheckout(e) => {
                        let why = format!("`{cmd}`: {e}");
                        c.replay.push(why.clone());
                        stale.get_or_insert(why);
                    }
                    Ran::Refused(e) => {
                        let why = format!("`{cmd}` could not be replayed: {e}");
                        c.replay.push(why.clone());
                        not_replayed.get_or_insert(why);
                    }
                }
            }
            Check::Fails { argv, signature } => {
                let cmd = argv.join(" ");
                match run(&rp, &argv, report) {
                    Ran::Did(r) if r.failed() => {
                        if !signature.is_empty() && r.output.contains(&signature) {
                            c.replay.push(format!("`{cmd}`: {}, with `{signature}` as the skill says", r.summary()));
                            kept.push((argv, signature));
                        } else if let Some(seen) = signature_of(&r.output) {
                            c.replay.push(format!("`{cmd}`: {}, with `{seen}` (not the skill's signature)", r.summary()));
                            kept.push((argv, seen));
                        } else {
                            c.replay.push(format!(
                                "`{cmd}`: {}, with no signature Adage can name ({}); dropped",
                                r.summary(),
                                last_line(&r.output)
                            ));
                        }
                    }
                    Ran::Did(r) if r.ok() => {
                        c.replay.push(format!(
                            "`{cmd}` works on a clean checkout ({}): the skill's prohibition is stale; dropped",
                            r.summary()
                        ));
                    }
                    Ran::Did(r) => {
                        c.replay.push(format!("`{cmd}`: {}; not verified", r.summary()));
                        not_replayed.get_or_insert_with(|| format!("`{cmd}` did not run to an exit"));
                        if !signature.is_empty() {
                            kept.push((argv, signature));
                        }
                    }
                    Ran::Missing(p) => {
                        let seen = format!("{p}: command not found");
                        c.replay.push(format!("`{cmd}`: {p} is not on the replay's PATH, so it fails with `{seen}`"));
                        kept.push((argv, seen));
                    }
                    Ran::NotInCheckout(e) => {
                        let seen = format!("{}: No such file or directory", argv[0]);
                        c.replay.push(format!("`{cmd}`: {e}"));
                        kept.push((argv, seen));
                    }
                    Ran::Refused(e) => {
                        c.replay.push(format!("`{cmd}` could not be replayed: {e}"));
                        not_replayed.get_or_insert(e);
                        if !signature.is_empty() {
                            kept.push((argv, signature));
                        }
                    }
                }
            }
            Check::Instead { argv } => {
                let cmd = argv.join(" ");
                match run(&rp, &argv, report) {
                    Ran::Did(r) => c.replay.push(format!("instead, `{cmd}`: {}", r.summary())),
                    Ran::Missing(p) => c.replay.push(format!("instead, `{cmd}`: {p} is not on the replay's PATH")),
                    Ran::NotInCheckout(e) | Ran::Refused(e) => c.replay.push(format!("instead, `{cmd}`: {e}")),
                }
            }
        }
    }
    if c.route.shape == Shape::Avoid {
        return split_avoid(c, kept, not_replayed);
    }
    if let Some(why) = stale {
        c.verified = Verified::Stale(why.clone());
        return (Vec::new(), Some(why));
    }
    c.verified = match not_replayed {
        Some(why) => Verified::NotReplayed(why),
        None => Verified::Works,
    };
    (vec![c], None)
}

/// An avoid route after its replay: runners kept with the same signature stay together; each
/// other signature gets a route of its own (`…-avoid-2`); none kept, and it is stale.
fn split_avoid(c: Candidate, kept: Vec<(Vec<String>, String)>, not_replayed: Option<String>) -> (Vec<Candidate>, Option<String>) {
    if kept.is_empty() {
        let why = "every command it forbids works, or fails with nothing Adage can name, on a clean checkout".to_string();
        return (Vec::new(), Some(why));
    }
    let mut groups: Vec<(String, Vec<Vec<String>>)> = Vec::new();
    for (argv, sig) in kept {
        match groups.iter_mut().find(|(s, _)| *s == sig) {
            Some((_, v)) => v.push(argv),
            None => groups.push((sig, vec![argv])),
        }
    }
    let mut out = Vec::new();
    for (i, (sig, runners)) in groups.into_iter().enumerate() {
        let mut n = c.clone();
        if i > 0 {
            n.route.id = format!("{}-{}", c.route.id, i + 1);
        }
        // Each route keeps the replay lines of its own runners (and of the instead command).
        let mine: Vec<String> = runners.iter().map(|r| format!("`{}`", r.join(" "))).collect();
        n.replay.retain(|l| l.starts_with("instead,") || mine.iter().any(|m| l.starts_with(m.as_str())));
        if let Some(a) = n.route.steps[0].avoid.as_mut() {
            a.runners = runners;
            a.signature = sig;
        }
        n.verified = match &not_replayed {
            Some(w) => Verified::NotReplayed(w.clone()),
            None => Verified::Works,
        };
        out.push(n);
    }
    (out, None)
}

/// Replay every candidate of `compiled` against `root`'s `HEAD` and settle each route's strength:
/// verified routes keep their shape's strength, the rest are prime at most (an avoid route stays
/// inhibit only with a signature). Stale commands leave their routes and are reported per step.
pub fn verify(compiled: &mut Compiled, root: &Path, o: &Options) -> Report {
    let mut report = Report::default();
    let candidates = std::mem::take(&mut compiled.candidates);
    if !o.enabled {
        report.sandbox = "not replayed (--no-replay)".into();
        compiled.candidates = candidates
            .into_iter()
            .map(|mut c| {
                c.verified = Verified::NotReplayed("replay was turned off (--no-replay)".into());
                c
            })
            .collect();
    } else {
        let cap = adage_exec::sandbox::detect();
        report.sandbox = cap.line();
        if !cap.available() {
            compiled.candidates = candidates
                .into_iter()
                .map(|mut c| {
                    c.verified = Verified::NotReplayed(format!("no sandbox on this machine ({})", cap.line()));
                    c
                })
                .collect();
        } else {
            for c in candidates {
                let steps = c.steps.clone();
                let (made, stale) = replay_one(c, root, o, &mut report);
                if let Some(why) = stale {
                    for s in steps {
                        compiled.stale.entry(s).or_default().push(why.clone());
                    }
                }
                compiled.candidates.extend(made);
            }
        }
    }
    settle(compiled);
    report
}

/// Strength by verification: only a verified route keeps its shape's strength.
pub fn settle(compiled: &mut Compiled) {
    // An avoid route with no signature cannot render; without a replay it has none.
    let unsigned = |c: &Candidate| {
        c.route.shape == Shape::Avoid && c.route.steps[0].avoid.as_ref().is_some_and(|a| a.signature.is_empty())
    };
    for c in compiled.candidates.iter().filter(|c| unsigned(c)) {
        for s in &c.steps {
            let v = compiled.text.entry(s.clone()).or_default();
            v.push("it forbids a command without the failure it causes, and no replay observed one".into());
        }
    }
    compiled.candidates.retain(|c| !unsigned(c));
    for c in &mut compiled.candidates {
        let verified = c.verified == Verified::Works;
        if !verified && c.route.strength == Strength::Fire {
            c.route.strength = Strength::Prime;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_are_short_and_renderable() {
        assert_eq!(signature_of("E   ModuleNotFoundError: No module named pytest\n").as_deref(), Some("No module named pytest"));
        assert_eq!(
            signature_of("Traceback\nRuntimeError: ACME_ENV is not set\n").as_deref(),
            Some("RuntimeError: ACME_ENV is not set")
        );
        assert_eq!(signature_of("sh: 1: pytest: not found").as_deref(), Some("pytest: not found"));
        assert_eq!(signature_of("all good"), None);
    }
}
