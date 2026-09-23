//! Replay a recorded session through the real runtime, in the holdout arm.
//!
//! Every hook event the session would have raised goes through `adage_runtime::decide` with the
//! session state held in memory. The arm is `holdout`, so each decision records what Adage *would
//! have said* without saying it — this is history, and nothing in it can change.

use crate::transcript::{self, Transcript};
use adage_core::keys::{self, template_key};
use adage_core::tools::normalize_tool;
use adage_core::{repo, CanonicalAction, CommandTable, Effect};
use adage_runtime::{decide, Arm, Ctx, Directive, Event, Judgement, SessionState};
use adage_tracks::Index;
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
    pub head: String,
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
}

pub fn actions(t: &Transcript, root: &Path, table: &CommandTable) -> Vec<Action> {
    let root_s = root.to_string_lossy().to_string();
    let mut out = Vec::new();
    for (i, calls) in t.turns().enumerate() {
        for (ci, c) in calls.iter().enumerate() {
            let input = transcript::rebase_value(&c.input, &t.cwd, &root_s);
            for mut n in keys::flatten(&normalize_tool(&c.tool, &input, &root_s, table)) {
                keys::canonicalise(&mut n, root);
                let argv = match &n.action {
                    CanonicalAction::Exec { argv, .. } => Some(argv.clone()),
                    _ => None,
                };
                out.push(Action {
                    turn: i as u32 + 1,
                    call: ci,
                    key: template_key(&n.action),
                    head: argv.as_deref().map(keys::command_head).unwrap_or_default(),
                    effect: n.effect,
                    read: matches!(n.action, CanonicalAction::Read { .. }),
                    author: keys::is_author(&n.action),
                    rel_path: keys::action_path(&n.action).map(|p| repo::relativize(root, p)),
                    argv,
                    failed: c.failed,
                    output: c.output.replace(&t.cwd, &root_s),
                });
            }
        }
    }
    out
}

/// `stop`: replay the session's final Stop hook (see [`transcript::events`]).
pub fn replay(t: &Transcript, index: &Index, repo_key: &str, root: &Path, table: &CommandTable, stop: bool) -> Replayed {
    let ctx = Ctx { index, repo: repo_key.to_string(), root, table, embed: None };
    let mut st = SessionState { arm: Some(Arm::Holdout), repo: Some(repo_key.to_string()), ..Default::default() };
    let mut r = Replayed {
        id: t.id.clone(),
        prompt: t.prompt().unwrap_or_default().to_string(),
        actions: actions(t, root, table),
        ..Default::default()
    };
    for step in transcript::events(t, &root.to_string_lossy(), stop) {
        let before: Vec<bool> = st.obligations.iter().map(|o| o.stop_requested).collect();
        let d = decide(&ctx, &mut st, &step.event);
        let name = transcript::hook_name(&step.event);
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
