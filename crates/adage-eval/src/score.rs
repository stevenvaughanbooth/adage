//! Scoring would-have directives against what the agent actually did later in the session.
//!
//! Definitions (docs/evals.md has the long form):
//!
//! - A directive is **correct** if the agent later took its step unaided: ran the directed
//!   command (the runtime's follow judgement, `adage_core::pattern::follows`: the same command,
//!   with whatever the agent added to narrow it, not misfired), read
//!   at least half of the files a linked-files directive listed, or ran a finish-line route's
//!   command after the edit that triggered it. For a dead-end warning the step is the command
//!   it recommends instead.
//! - **Precision@session**: correct / given. **Precision@window**: correct within `window` model
//!   turns of being given. **False-directive rate** = 1 − precision@session.
//! - **Turns saved** (an optimistic estimate): model turns between the directive and the agent's
//!   first matching action, as if the agent had acted at once; for linked files, plus the extra
//!   turns spent reading them one at a time; for dead ends, failed runs of the avoided command.
//! - **Recall**: of the agent's key steps (the lookup of the prompt's ticket, reads of the files
//!   it listed, the first passing verify, a regenerate after edits, each failed verify), those
//!   for which a matching directive came before the agent got there.
//! - **Dead-end accuracy**: of the warnings whose avoided command the agent later ran, how often
//!   it failed as predicted.
//! - **Finish-line accuracy**: of the obligations given, how many were right — the agent ran the
//!   step, or the session's hidden check shows it was needed (stale generated code, or failure).

use crate::dataset::SessionMeta;
use crate::replay::{Action, Fired, Replayed};
use crate::safety::{self, Violation};
use adage_core::spans::{self, SpanKind};
use adage_core::{pattern, CommandTable, Effect};
use adage_runtime::command_key;
use adage_tracks::{Index, Route, Step, Trigger};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Where a route acts and how, for per-group metrics: its delivery shape
/// ([`adage_tracks::shape`]) at its trigger, `run@prompt`, `read-batch@after-command`,
/// `avoid@session-start`, `obligation@after-edit`, `run@intent`, ….
pub fn point(r: &Route) -> String {
    let at = match &r.trigger {
        Trigger::Prompt { .. } => "prompt",
        Trigger::Intent { .. } => "intent",
        Trigger::AfterCommand { .. } => "after-command",
        Trigger::AfterEdit { .. } => "after-edit",
        Trigger::AfterFailure { .. } => "after-failure",
        Trigger::SessionStart => "session-start",
        Trigger::Context { .. } => "context",
    };
    format!("{}@{at}", adage_tracks::shape(r))
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Metrics {
    pub directives: u32,
    pub sessions: u32,
    pub correct: u32,
    pub correct_in_window: u32,
    pub precision: f64,
    pub precision_in_window: f64,
    pub false_directive_rate: f64,
    pub turns_saved: u32,
    pub turns_saved_median: f64,
    /// Wilson 95% intervals `[low, high]` for the proportions above.
    pub precision_ci: [f64; 2],
    pub precision_in_window_ci: [f64; 2],
    pub false_directive_rate_ci: [f64; 2],
    #[serde(skip)]
    saved: Vec<u32>,
    #[serde(skip)]
    session_ids: BTreeSet<String>,
}

impl Metrics {
    fn add(&mut self, session: &str, correct: bool, in_window: bool, saved: u32) {
        self.directives += 1;
        self.session_ids.insert(session.to_string());
        if correct {
            self.correct += 1;
            self.saved.push(saved);
        }
        if in_window {
            self.correct_in_window += 1;
        }
    }

    fn finish(&mut self) {
        self.sessions = self.session_ids.len() as u32;
        self.precision = ratio(self.correct, self.directives);
        self.precision_in_window = ratio(self.correct_in_window, self.directives);
        self.false_directive_rate = if self.directives == 0 { 0.0 } else { round(1.0 - self.precision) };
        self.precision_ci = wilson(self.correct, self.directives);
        self.precision_in_window_ci = wilson(self.correct_in_window, self.directives);
        self.false_directive_rate_ci = wilson(self.directives - self.correct, self.directives);
        self.turns_saved = self.saved.iter().sum();
        self.saved.sort_unstable();
        self.turns_saved_median = match self.saved.len() {
            0 => 0.0,
            n if n % 2 == 1 => self.saved[n / 2] as f64,
            n => (self.saved[n / 2 - 1] + self.saved[n / 2]) as f64 / 2.0,
        };
    }
}

pub fn round(x: f64) -> f64 {
    (x * 10_000.0).round() / 10_000.0
}

pub fn ratio(a: u32, b: u32) -> f64 {
    if b == 0 {
        0.0
    } else {
        round(a as f64 / b as f64)
    }
}

/// The Wilson score 95% interval `[low, high]` for `k` successes in `n` trials, rounded like
/// every other metric. With no trials it is `[0, 1]`: nothing is known.
///
/// Plain IEEE arithmetic and `sqrt` (correctly rounded), so the bytes are the same everywhere.
pub fn wilson(k: u32, n: u32) -> [f64; 2] {
    if n == 0 {
        return [0.0, 1.0];
    }
    const Z: f64 = 1.959_963_984_540_054;
    let n = n as f64;
    let p = k as f64 / n;
    let z2 = Z * Z;
    let denom = 1.0 + z2 / n;
    let centre = (p + z2 / (2.0 * n)) / denom;
    let half = Z * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt() / denom;
    [round((centre - half).max(0.0)), round((centre + half).min(1.0))]
}

/// `[low–high]` in percent, for the text report.
pub fn ci_text(ci: [f64; 2]) -> String {
    format!("[{:.1}–{:.1}]", ci[0] * 100.0, ci[1] * 100.0)
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Recall {
    pub key_steps: u32,
    pub covered: u32,
    pub recall: f64,
    /// Wilson 95% interval for `recall`.
    pub recall_ci: [f64; 2],
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct DeadEnds {
    pub warnings: u32,
    /// Warnings after which the agent ran an avoided command.
    pub tested: u32,
    /// ... and it failed, as predicted.
    pub confirmed: u32,
    /// ... and it succeeded.
    pub refuted: u32,
    pub accuracy: f64,
    /// Wilson 95% interval for `accuracy` (confirmed of tested).
    pub accuracy_ci: [f64; 2],
    /// Failed verify runs in scored sessions, and those a warning had already covered.
    pub failed_verify_runs: u32,
    pub failed_runs_after_warning: u32,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct FinishLine {
    pub obligations: u32,
    /// The agent ran the step unaided.
    pub met: u32,
    /// Not run, and the hidden check shows it was needed.
    pub caught: u32,
    /// Not run, and the session passed with fresh generated code.
    pub false_alarms: u32,
    /// Not run, and the session has no outcome labels.
    pub unknown: u32,
    pub accuracy: f64,
    /// Wilson 95% interval for `accuracy` (met + caught of labelled obligations).
    pub accuracy_ci: [f64; 2],
    /// Sessions whose hidden check found stale generated code with no obligation given.
    pub missed: u32,
    pub stop_requests: u32,
    pub stop_requests_correct: u32,
}

/// Instinct directives (prebuilt routes, bound from the repository's files): what they said on day
/// 0 against what the agents did.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Instincts {
    /// Directives from instinct routes that direct a command (advice excluded).
    pub directives: u32,
    /// ... that the agent then took unaided (as for any directive).
    pub correct: u32,
    pub precision: f64,
    pub precision_ci: [f64; 2],
    /// Instinct dead ends (after a failure, the declared command): given, and correct.
    pub dead_end_directives: u32,
    pub dead_end_correct: u32,
    /// Instinct finish lines (after an edit, the declared generator): given, and correct.
    pub finish_line_directives: u32,
    pub finish_line_correct: u32,
    /// Advice given (generated files, lockfiles, the package manager); not scored.
    pub advice: u32,
    /// Failed verify runs in scored sessions, and those an instinct answered with a directive at
    /// that failure whose command the agent then ran successfully (day-0 recall of dead ends).
    pub failed_verify_runs: u32,
    pub failures_answered: u32,
    pub recall: f64,
    pub recall_ci: [f64; 2],
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Safety {
    pub checked: u32,
    pub unsafe_directives: u32,
    pub violations: Vec<Violation>,
}

/// One scored directive, for the per-session detail.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Scored {
    pub route: String,
    pub shape: String,
    pub event: String,
    pub turn: u32,
    pub correct: bool,
    /// Turn of the first matching action, if any.
    pub matched_turn: Option<u32>,
    pub turns_saved: u32,
    /// The directive's text, exactly as the runtime rendered it.
    pub text: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SessionScore {
    pub id: String,
    pub run: String,
    pub turns: u32,
    pub directives: Vec<Scored>,
    pub key_steps: u32,
    pub covered: u32,
    pub turns_saved: u32,
}

#[derive(Debug, Default)]
pub struct Tally {
    pub overall: Metrics,
    pub by_shape: BTreeMap<String, Metrics>,
    pub by_route: BTreeMap<String, Metrics>,
    pub recall: BTreeMap<String, Recall>,
    pub dead_ends: DeadEnds,
    pub finish_line: FinishLine,
    pub safety: Safety,
    pub sessions: Vec<SessionScore>,
    pub instincts: Instincts,
}

impl Tally {
    pub fn finish(&mut self) {
        self.overall.finish();
        self.by_shape.values_mut().for_each(Metrics::finish);
        self.by_route.values_mut().for_each(Metrics::finish);
        let mut total = Recall::default();
        for r in self.recall.values_mut() {
            r.recall = ratio(r.covered, r.key_steps);
            r.recall_ci = wilson(r.covered, r.key_steps);
            total.key_steps += r.key_steps;
            total.covered += r.covered;
        }
        total.recall = ratio(total.covered, total.key_steps);
        total.recall_ci = wilson(total.covered, total.key_steps);
        self.recall.insert("all".into(), total);
        let d = &mut self.dead_ends;
        d.accuracy = ratio(d.confirmed, d.tested);
        d.accuracy_ci = wilson(d.confirmed, d.tested);
        let f = &mut self.finish_line;
        f.accuracy = ratio(f.met + f.caught, f.met + f.caught + f.false_alarms);
        f.accuracy_ci = wilson(f.met + f.caught, f.met + f.caught + f.false_alarms);
        let i = &mut self.instincts;
        i.precision = ratio(i.correct, i.directives);
        i.precision_ci = wilson(i.correct, i.directives);
        i.recall = ratio(i.failures_answered, i.failed_verify_runs);
        i.recall_ci = wilson(i.failures_answered, i.failed_verify_runs);
        self.safety.violations.sort();
    }
}

/// The directed command a directive's text names (the indented line).
fn command_in(text: &str) -> Option<String> {
    // "A known next step here is:" carries its exit clause on the command's line.
    let cmd = |l: &str| l.split(" Ignore this if ").next().unwrap_or(l).trim().to_string();
    text.lines().find_map(|l| l.strip_prefix("  ")).map(cmd)
}

fn files_in(text: &str) -> Vec<String> {
    let first = text.lines().next().unwrap_or("");
    first
        .strip_prefix("That result lists these files: ")
        .and_then(|s| s.strip_suffix('.'))
        .map(|s| s.split(", ").map(str::to_string).collect())
        .unwrap_or_default()
}

/// A command as the scorer compares it: its canonical key.
pub(crate) struct Expect {
    pub(crate) key: String,
}

impl Expect {
    pub(crate) fn of(cmd: &str, table: &CommandTable) -> Expect {
        Expect { key: command_key(cmd, table) }
    }
    /// The same command, for route matching: either meets the other (`adage_core::pattern`), so
    /// `python3 scripts/wi-lookup W-1` and `./scripts/wi-lookup W-2 --json` are one command, and
    /// `npm run check` and `npm run codegen` are two.
    pub(crate) fn same(&self, other: &Expect) -> bool {
        !self.key.is_empty() && (pattern::meets(&self.key, &other.key) || pattern::meets(&other.key, &self.key))
    }
    /// The runtime's own follow judgement ([`pattern::follows`]): the directed command, with
    /// whatever the agent added to narrow it, that did not misfire (a test run that ran and
    /// reported failing tests was followed; one whose runner could not start was not).
    fn matches(&self, a: &Action) -> bool {
        a.argv.is_some()
            && !(a.failed && adage_core::outcome::misfired(&a.output))
            && pattern::follows(&self.key, &a.key)
    }
}

/// Does a failing runner (`python3 -m pytest`) describe this command? Any run of the same program
/// and subcommand does (`adage_core::pattern::CommandPattern::runner`).
fn runs_runner(a: &Action, runner: &str) -> bool {
    a.argv.is_some() && pattern::runs_avoided(runner, &a.key)
}

/// Does `argv` run `runner`: the same program and subcommand, or script (`node --test` is not
/// `node tools/codegen.mjs`)? A bare interpreter (`python3`) is any run of it.
pub(crate) fn argv_runs(argv: &[String], runner: &str) -> bool {
    let (Some(p), Some(o)) = (pattern::CommandPattern::runner(runner), pattern::Shape::of_argv(argv)) else {
        return false;
    };
    p.matches(&o)
}

fn after(a: &Action, f: &Fired) -> bool {
    a.turn > f.turn
}

/// Score one replayed session into the tally.
#[allow(clippy::too_many_arguments)]
pub fn score_session(
    tally: &mut Tally,
    rep: &Replayed,
    meta: Option<&SessionMeta>,
    index: &Index,
    repo_key: &str,
    root: &Path,
    table: &CommandTable,
    window: u32,
    canaries: &[String],
) {
    let routes: BTreeMap<&str, &Route> = index.for_repo(repo_key).map(|ir| (ir.route.id.as_str(), &ir.route)).collect();
    let acts = &rep.actions;
    let mut detail = Vec::new();
    let mut session_saved = 0;

    for f in &rep.fired {
        let Some(route) = routes.get(f.directive.route.as_str()) else { continue };
        let text = &f.directive.text;
        tally.safety.checked += 1;
        // The whole directive as the agent receives it: wrapper, origin line and body.
        let v =
            safety::check_wrapper(route, &adage_runtime::render::wrap_directive(&f.directive), Some(root), canaries);
        if !v.is_empty() {
            tally.safety.unsafe_directives += 1;
            tally.safety.violations.extend(v);
        }
        let k = point(route);
        let (correct, matched, saved) = match &route.step {
            // Advice directs no command: it is checked for safety, and not scored.
            Step::Advise { .. } => {
                tally.instincts.advice += u32::from(route.is_instinct());
                continue;
            }
            Step::ReadBatch { .. } => {
                let files = files_in(text);
                let reads: Vec<&Action> = acts
                    .iter()
                    .filter(|a| after(a, f) && a.read && a.rel_path.as_ref().is_some_and(|p| files.contains(p)))
                    .collect();
                let covered: BTreeSet<&String> = reads.iter().filter_map(|a| a.rel_path.as_ref()).collect();
                let turns: BTreeSet<u32> = reads.iter().map(|a| a.turn).collect();
                let correct = !files.is_empty() && covered.len() * 2 >= files.len();
                let first = turns.iter().next().copied();
                let saved = first.map(|t| (t - f.turn - 1) + turns.len().saturating_sub(1) as u32).unwrap_or(0);
                (correct, first, saved)
            }
            Step::Avoid { avoid, instead, .. } => {
                let e = Expect::of(instead, table);
                let m = acts.iter().find(|a| after(a, f) && e.matches(a)).map(|a| a.turn);
                let runners: Vec<&str> = avoid.split(" / ").collect();
                let tried: Vec<&Action> =
                    acts.iter().filter(|a| after(a, f) && runners.iter().any(|r| runs_runner(a, r))).collect();
                let d = &mut tally.dead_ends;
                d.warnings += 1;
                if !tried.is_empty() {
                    d.tested += 1;
                    if tried.iter().any(|a| a.failed) {
                        d.confirmed += 1;
                    } else {
                        d.refuted += 1;
                    }
                }
                let wasted = tried.iter().filter(|a| a.failed).count() as u32;
                (m.is_some(), m, wasted)
            }
            Step::Run { command } if matches!(route.trigger, Trigger::AfterEdit { .. }) => {
                let Trigger::AfterEdit { paths } = &route.trigger else { unreachable!() };
                let glob = glob_pattern(paths);
                let edit_call = acts
                    .iter()
                    .filter(|a| a.turn == f.turn && a.author)
                    .find(|a| a.rel_path.as_deref().is_some_and(|p| glob.as_ref().is_some_and(|g| g.matches(p))))
                    .map(|a| a.call);
                let key = command_key(command, table);
                let m = acts
                    .iter()
                    .find(|a| {
                        a.argv.is_some()
                            && !a.failed
                            && pattern::meets(&key, &a.key)
                            && (a.turn > f.turn || (a.turn == f.turn && edit_call.is_some_and(|c| a.call > c)))
                    })
                    .map(|a| a.turn);
                tally_obligation(&mut tally.finish_line, m.is_some(), meta);
                (m.is_some(), m, 0)
            }
            Step::Run { .. } => {
                let Some(cmd) = command_in(text) else { continue };
                let e = Expect::of(&cmd, table);
                let m = acts.iter().find(|a| after(a, f) && e.matches(a)).map(|a| a.turn);
                (m.is_some(), m, m.map(|t| t - f.turn - 1).unwrap_or(0))
            }
            // A sequence is judged on its first command (Track G §6.1). One that begins with a
            // finish line's command after the same edits carries that obligation (the runtime
            // delivers the sequence alone): it counts as the obligation given, too.
            Step::Sequence { commands } => {
                let Some(cmd) = commands.first() else { continue };
                let e = Expect::of(cmd, table);
                let m = acts.iter().find(|a| after(a, f) && e.matches(a)).map(|a| a.turn);
                let carries = matches!(route.trigger, Trigger::AfterEdit { .. })
                    && routes.values().any(|o| {
                        !o.is_instinct()
                            && matches!(o.trigger, Trigger::AfterEdit { .. })
                            && adage_tracks::interlock::same_point(&route.trigger, &o.trigger)
                            && matches!(&o.step, Step::Run { command } if pattern::same_command(cmd, command))
                    });
                if carries {
                    tally_obligation(&mut tally.finish_line, m.is_some(), meta);
                }
                (m.is_some(), m, m.map(|t| t - f.turn - 1).unwrap_or(0))
            }
        };
        let in_window = correct && matched.is_some_and(|t| t.saturating_sub(f.turn) <= window);
        if route.is_instinct() {
            let i = &mut tally.instincts;
            i.directives += 1;
            i.correct += u32::from(correct);
            match &route.trigger {
                Trigger::AfterFailure { .. } => {
                    i.dead_end_directives += 1;
                    i.dead_end_correct += u32::from(correct);
                }
                Trigger::AfterEdit { .. } => {
                    i.finish_line_directives += 1;
                    i.finish_line_correct += u32::from(correct);
                }
                _ => {}
            }
        }
        for m in [
            &mut tally.overall,
            tally.by_shape.entry(k.clone()).or_default(),
            tally.by_route.entry(route.id.clone()).or_default(),
        ] {
            m.add(&rep.id, correct, in_window, saved);
        }
        if correct {
            session_saved += saved;
        }
        detail.push(Scored {
            route: route.id.clone(),
            shape: k,
            event: f.event.into(),
            turn: f.turn,
            correct,
            matched_turn: matched,
            turns_saved: if correct { saved } else { 0 },
            text: text.clone(),
        });
    }

    for (route, reason) in &rep.stops {
        let v = safety::check_stop(route, reason, canaries);
        tally.safety.checked += 1;
        if !v.is_empty() {
            tally.safety.unsafe_directives += 1;
            tally.safety.violations.extend(v);
        }
        let fl = &mut tally.finish_line;
        fl.stop_requests += 1;
        if meta.is_some_and(|m| m.stale_generated == Some(true) || m.passed == Some(false)) {
            fl.stop_requests_correct += 1;
        }
    }
    let any_obligation = rep.fired.iter().any(|f| {
        routes.get(f.directive.route.as_str()).is_some_and(|r| matches!(r.trigger, Trigger::AfterEdit { .. }))
    });
    if meta.is_some_and(|m| m.stale_generated == Some(true)) && !any_obligation {
        tally.finish_line.missed += 1;
    }

    // Interlocking, over the whole session's decisions.
    let decisions: Vec<Vec<&Route>> = rep
        .trace
        .iter()
        .map(|(_, ids, _)| ids.iter().filter_map(|id| routes.get(id.as_str()).copied()).collect())
        .collect();
    let v = safety::check_interlock(&decisions);
    tally.safety.unsafe_directives += v.len() as u32;
    tally.safety.violations.extend(v);

    let (key_steps, covered) = recall(tally, rep, &routes, root, table);
    tally.sessions.push(SessionScore {
        id: rep.id.clone(),
        run: meta.map(|m| m.run.clone()).unwrap_or_default(),
        turns: rep.turns,
        directives: detail,
        key_steps,
        covered,
        turns_saved: session_saved,
    });
}

fn glob_pattern(p: &str) -> Option<glob::Pattern> {
    glob::Pattern::new(p).ok()
}

/// Key steps the agent took, and whether a matching directive came first.
fn recall(
    tally: &mut Tally,
    rep: &Replayed,
    routes: &BTreeMap<&str, &Route>,
    root: &Path,
    table: &CommandTable,
) -> (u32, u32) {
    let acts = &rep.actions;
    let fired: Vec<(&Fired, &Route)> =
        rep.fired.iter().filter_map(|f| routes.get(f.directive.route.as_str()).map(|r| (f, *r))).collect();
    // Commands the directives named, with the turn they were given.
    // (turn, command, given after an edit)
    let mut named: Vec<(u32, Expect, bool)> = Vec::new();
    for (f, r) in &fired {
        let by_edit = matches!(r.trigger, Trigger::AfterEdit { .. }) && !matches!(r.step, Step::Avoid { .. });
        match &r.step {
            Step::Avoid { instead, .. } => named.push((f.turn, Expect::of(instead, table), false)),
            Step::Run { .. } => {
                if let Some(c) = command_in(&f.directive.text) {
                    named.push((f.turn, Expect::of(&c, table), by_edit));
                }
            }
            Step::Sequence { commands } => {
                for c in commands {
                    named.push((f.turn, Expect::of(c, table), by_edit));
                }
            }
            Step::ReadBatch { .. } | Step::Advise { .. } => {}
        }
    }
    let covered_cmd = |a: &Action| named.iter().any(|(t, e, _)| *t < a.turn && e.matches(a));
    let mut steps: Vec<(&'static str, bool)> = Vec::new();

    // 1. The lookup of the prompt's ticket, and the files it listed.
    let tickets: Vec<String> =
        spans::extract(&rep.prompt).into_iter().filter(|s| s.kind == SpanKind::Ticket).map(|s| s.value).collect();
    let lookup =
        acts.iter().find(|a| !a.failed && a.argv.as_ref().is_some_and(|v| v.iter().any(|x| tickets.contains(x))));
    if let Some(l) = lookup {
        steps.push(("lookup", covered_cmd(l)));
        let listed: BTreeSet<String> = spans::extract(&l.output)
            .into_iter()
            .filter(|s| {
                s.kind == SpanKind::Path
                    && adage_core::repo::is_safe_relative(&s.value)
                    && root.join(&s.value).is_file()
            })
            .map(|s| s.value)
            .collect();
        let mut seen = BTreeSet::new();
        for a in acts.iter().filter(|a| a.turn > l.turn && a.read) {
            let Some(p) = &a.rel_path else { continue };
            if listed.contains(p) && seen.insert(p.clone()) {
                let hit = fired.iter().any(|(f, r)| {
                    matches!(r.step, Step::ReadBatch { .. })
                        && f.turn < a.turn
                        && files_in(&f.directive.text).contains(p)
                });
                steps.push(("linked-read", hit));
            }
        }
    }
    // 2. The first passing verify.
    if let Some(v) = acts.iter().find(|a| a.effect == Effect::Verify && a.argv.is_some() && !a.failed) {
        steps.push(("verify", covered_cmd(v)));
    }
    // 3. Each failed verify: a dead end, covered if a warning naming its runner came first.
    for a in acts.iter().filter(|a| a.effect == Effect::Verify && a.argv.is_some() && a.failed) {
        tally.dead_ends.failed_verify_runs += 1;
        // Instincts answer a failure as it happens: a directive at this turn's failure event whose
        // command the agent then ran successfully.
        tally.instincts.failed_verify_runs += 1;
        let answered = fired.iter().any(|(f, r)| {
            r.is_instinct()
                && matches!(r.trigger, Trigger::AfterFailure { .. })
                && f.turn == a.turn
                && f.event == "PostToolUseFailure"
                && command_in(&f.directive.text).is_some_and(|c| {
                    let e = Expect::of(&c, table);
                    acts.iter().any(|b| b.turn > a.turn && e.matches(b))
                })
        });
        tally.instincts.failures_answered += u32::from(answered);
        let hit = fired.iter().any(|(f, r)| match &r.step {
            Step::Avoid { avoid, .. } => f.turn < a.turn && avoid.split(" / ").any(|x| runs_runner(a, x)),
            _ => false,
        });
        if hit {
            tally.dead_ends.failed_runs_after_warning += 1;
        }
        steps.push(("dead-end", hit));
    }
    // 4. A regenerate after the first edit.
    if let Some(first_edit) = acts.iter().position(|a| a.author) {
        if let Some(g) =
            acts.iter().skip(first_edit + 1).find(|a| a.effect == Effect::Regenerate && a.argv.is_some() && !a.failed)
        {
            let hit = named.iter().any(|(t, e, by_edit)| *by_edit && *t <= g.turn && e.matches(g));
            steps.push(("regenerate", hit));
        }
    }
    let mut covered = 0;
    for (k, hit) in &steps {
        let r = tally.recall.entry((*k).into()).or_default();
        r.key_steps += 1;
        if *hit {
            r.covered += 1;
            covered += 1;
        }
    }
    (steps.len() as u32, covered)
}

/// One obligation given: met, or caught (the session needed it), a false alarm, or unknown.
fn tally_obligation(fl: &mut FinishLine, met: bool, meta: Option<&SessionMeta>) {
    fl.obligations += 1;
    let needed = meta.is_some_and(|m| m.stale_generated == Some(true) || m.passed == Some(false));
    let clean = meta.is_some_and(|m| m.passed == Some(true) && m.stale_generated != Some(true));
    if met {
        fl.met += 1;
    } else if needed {
        fl.caught += 1;
    } else if clean {
        fl.false_alarms += 1;
    } else {
        fl.unknown += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wilson_matches_known_values() {
        assert_eq!(wilson(5, 10), [0.2366, 0.7634]);
        assert_eq!(wilson(49, 66), [0.6257, 0.8325]);
        assert_eq!(wilson(50, 50), [0.9287, 1.0]);
        assert_eq!(wilson(0, 3), [0.0, 0.5615]);
        assert_eq!(wilson(0, 0), [0.0, 1.0]);
        assert_eq!(ci_text([0.6257, 0.8325]), "[62.6–83.2]");
    }
}
