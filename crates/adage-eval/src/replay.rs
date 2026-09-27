//! Replay a recorded session through the real runtime, in the holdout arm.
//!
//! Every hook event the session would have raised goes through `adage_runtime::decide` with the
//! session state held in memory. The arm is `holdout`, so each decision records what Adage *would
//! have said* without saying it — this is history, and nothing in it can change.

use crate::transcript::{self, Transcript};
use adage_core::keys::{self, template_key};
use adage_core::tools::normalize_tool;
use adage_core::{repo, CanonicalAction, CommandTable, Effect};
use adage_runtime::{decide, fast, Arm, Ctx, Directive, Event, Judgement, RuntimeParams, SessionState};
use adage_tracks::Index;
use serde::Serialize;
use std::path::Path;

/// A directive the runtime would have given, and when.
#[derive(Debug, Clone)]
pub struct Fired {
    pub directive: Directive,
    pub event: &'static str,
    /// Model turns completed when it was given; the agent could act on it from turn `turn + 1`.
    pub turn: u32,
}

/// One canonical action the agent took.
#[derive(Debug, Clone)]
pub struct Action {
    pub turn: u32,
    /// Index of the tool call within its turn.
    pub call: usize,
    pub key: String,
    pub argv: Option<Vec<String>>,
    pub effect: Effect,
    pub read: bool,
    pub author: bool,
    pub rel_path: Option<String>,
    pub failed: bool,
    /// The call's result text, as the model saw it.
    pub output: String,
}

#[derive(Debug, Clone, Default)]
pub struct Replayed {
    pub id: String,
    pub prompt: String,
    pub fired: Vec<Fired>,
    /// Finish-line stop requests that would have been made: (obligation route, reason).
    pub stops: Vec<(String, String)>,
    pub judgements: Vec<(u32, Judgement)>,
    pub actions: Vec<Action>,
    pub turns: u32,
    /// Per event: hook name, would-have route ids, whether a stop request would have been made.
    pub trace: Vec<(&'static str, Vec<String>, bool)>,
    /// The hook's no-match fast path, checked against the full decision on every event.
    pub fast: FastCheck,
    /// Per event (as `trace`): whether the hook's fast path skips it, leaving no spool record.
    pub skipped: Vec<bool>,
}

/// The no-match fast path (`adage_runtime::fast`) against the full decision path. The replay
/// keeps a second session state, decided the way the hook decides: an event the fast path skips
/// is not decided at all. A skipped event's full decision must be empty, and every other event's
/// decision, and the state later decisions read, must be the same on both paths.
#[derive(Debug, Clone, Default, Serialize)]
pub struct FastCheck {
    pub events: u32,
    pub skipped: u32,
    /// `<session> event <n> (<hook>): <what differed>`
    pub mismatches: Vec<String>,
}

/// The state later decisions read (not the `batches` and `history` counters).
fn decision_state(st: &SessionState) -> serde_json::Value {
    serde_json::json!([st.arm, st.emitted, st.pending, st.obligations, st.stop_requests, st.prompts > 0])
}

pub fn actions(t: &Transcript, root: &Path, table: &CommandTable) -> Vec<Action> {
    let root_s = root.to_string_lossy().to_string();
    let mut out = Vec::new();
    for (i, calls) in t.turns().enumerate() {
        for (ci, c) in calls.iter().enumerate() {
            let input = transcript::rebase_value(&c.input, &t.cwd, &root_s);
            let flat = keys::flatten(&normalize_tool(&c.tool, &input, &root_s, table));
            let count = flat.len();
            for (si, mut n) in flat.into_iter().enumerate() {
                keys::canonicalise(&mut n, root);
                let argv = match &n.action {
                    CanonicalAction::Exec { argv, .. } => Some(argv.clone()),
                    _ => None,
                };
                out.push(Action {
                    turn: i as u32 + 1,
                    call: ci,
                    key: template_key(&n.action),
                    effect: n.effect,
                    read: matches!(n.action, CanonicalAction::Read { .. }),
                    author: keys::is_author(&n.action),
                    rel_path: keys::action_path(&n.action).map(|p| repo::relativize(root, p)),
                    argv,
                    // As the learner attributes it: a failed chain failed at its last step.
                    failed: keys::sub_action_failed(si, count, c.failed, keys::has_or_chain(&c.input)),
                    output: c.output.replace(&t.cwd, &root_s),
                });
            }
        }
    }
    out
}

/// `stop`: replay the session's final Stop hook (see [`transcript::events`]). `params`: the
/// runtime's limits (the hook's are `RuntimeParams::default()`).
pub fn replay(
    t: &Transcript,
    index: &Index,
    repo_key: &str,
    root: &Path,
    table: &CommandTable,
    stop: bool,
    params: RuntimeParams,
) -> Replayed {
    let ctx = Ctx { index, repo: repo_key.to_string(), root, table, embed: None, params };
    let mut st = SessionState { arm: Some(Arm::Holdout), repo: Some(repo_key.to_string()), ..Default::default() };
    let mut r = Replayed {
        id: t.id.clone(),
        prompt: t.prompt().unwrap_or_default().to_string(),
        actions: actions(t, root, table),
        ..Default::default()
    };
    let triggers = index.triggers(repo_key);
    let root_s = root.to_string_lossy().to_string();
    let mut fast_st = st.clone();
    for (n, step) in transcript::events(t, &root_s, stop).into_iter().enumerate() {
        let before: Vec<bool> = st.obligations.iter().map(|o| o.stop_requested).collect();
        let skip = fast::skip(&triggers, &step.event, &root_s, || Some(fast_st.clone()));
        let d = decide(&ctx, &mut st, &step.event);
        let name = transcript::hook_name(&step.event);
        r.fast.events += 1;
        r.skipped.push(skip);
        let mut differs = |what: &str| r.fast.mismatches.push(format!("{} event {n} ({name}): {what}", t.id));
        if skip {
            r.fast.skipped += 1;
            if !d.would_have.is_empty() || !d.judgements.is_empty() || d.would_continue.is_some() {
                differs("skipped, but the full path decided something");
            }
        } else {
            let fd = decide(&ctx, &mut fast_st, &step.event);
            if serde_json::to_value(&fd).ok() != serde_json::to_value(&d).ok() {
                differs("different decisions");
            }
        }
        if decision_state(&fast_st) != decision_state(&st) {
            differs("different state for later decisions");
            fast_st = st.clone();
        }
        debug_assert!(d.directives.is_empty(), "the holdout arm never delivers");
        for w in &d.would_have {
            r.fired.push(Fired { directive: w.clone(), event: name, turn: step.turn });
        }
        if let (Some(reason), Event::Stop { .. }) = (&d.would_continue, &step.event) {
            let route = st
                .obligations
                .iter()
                .enumerate()
                .find(|(i, o)| o.stop_requested && !before.get(*i).copied().unwrap_or(false))
                .map(|(_, o)| o.route.clone())
                .unwrap_or_default();
            r.stops.push((route, reason.clone()));
        }
        for j in d.judgements {
            r.judgements.push((step.turn, j));
        }
        r.trace.push((name, d.would_have.iter().map(|w| w.route.clone()).collect(), d.would_continue.is_some()));
        r.turns = step.turn;
    }
    r
}
