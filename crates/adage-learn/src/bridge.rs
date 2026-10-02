//! The bridge from an imported [`Session`] to the graph engine's input (Track G §1.3:
//! `adage-graph` never depends on `adage-learn`; this module converts).
//!
//! - Each step's canonical action becomes an [`input::Step`]: an Exec keeps its literal argv, its
//!   output and whether it failed; a read, list or search of a repository path becomes a
//!   `Read`; an edit or write becomes an `Edit`; anything else (a tool with no path) is left out.
//!   A compound action contributes each of its parts.
//! - Every prompt the person typed ([`Session::prompts`], G0) becomes an [`input::Prompt`],
//!   anchored at the first bridged step of the import step it was anchored at. A session built
//!   without prompts (by hand, or read from a cache of an older format) has its first prompt at
//!   step 0.
//! - [`annotate`] also keeps each step's turn, prompt, effect class and template key
//!   ([`input::StepMeta`]) and the session's day.

use crate::import::Exposure as _;
use crate::import::{Session, StepObs};
use adage_core::keys::template_key;
use adage_core::types::{CanonicalAction, Effect, Normalized};
use adage_core::CommandTable;
use adage_graph::input;

/// The effect class of a command as the graph engine labels it: the command's own. Each part of a
/// compound keeps its own class (in `cargo fmt --all && git commit -m …` the format step
/// regenerates; it is no author step), and a command whose output was redirected to a file is
/// still the command it ran (`cargo test --workspace > log` verifies; the log is the agent's).
pub(crate) fn effect_of(n: &Normalized) -> Effect {
    match &n.action {
        CanonicalAction::Exec { argv, .. } if n.note.starts_with("redirected to ") => {
            adage_graph::events::classify(argv, table())
        }
        _ => n.effect,
    }
}

fn table() -> &'static CommandTable {
    static T: std::sync::OnceLock<CommandTable> = std::sync::OnceLock::new();
    T.get_or_init(CommandTable::load)
}

fn push(n: &Normalized, step: &StepObs, out: &mut Vec<(input::Step, input::StepMeta)>) {
    let action = &n.action;
    let effect = effect_of(n);
    let meta = |key: String| input::StepMeta {
        turn: step.turn,
        prompt: step.prompt,
        effect: effect.as_str().to_string(),
        key,
        project_pytest_root_cwd: step.project_pytest_root_cwd,
    };
    match action {
        CanonicalAction::Exec { argv, .. } => out.push((
            input::Step::Exec { argv: argv.clone(), output: step.call.output.clone(), failed: step.call.failed },
            meta(template_key(action)),
        )),
        CanonicalAction::Read { .. } | CanonicalAction::List { .. } | CanonicalAction::Search { .. } => {
            if let Some(p) = &step.rel_path {
                out.push((input::Step::Read { path: p.clone() }, meta(template_key(action))));
            }
        }
        CanonicalAction::Edit { .. } | CanonicalAction::Write { .. } => {
            if let Some(p) = &step.rel_path {
                out.push((input::Step::Edit { path: p.clone() }, meta(template_key(action))));
            }
        }
        CanonicalAction::Compound { actions, .. } => {
            for a in actions {
                push(a, step, out);
            }
        }
        _ => {}
    }
}

/// The session as the graph engine reads it, with each step's metadata and the day.
pub fn annotate(s: &Session) -> input::Annotated {
    annotate_prefix(s, s.steps.len())
}

/// [`annotate`] of the session's first `n` steps (and the prompts typed before step `n`).
pub fn annotate_prefix(s: &Session, n: usize) -> input::Annotated {
    let steps = &s.steps[..n.min(s.steps.len())];
    let mut out: Vec<(input::Step, input::StepMeta)> = Vec::new();
    // The first bridged step of each import step (and one past the end).
    let mut start: Vec<u32> = Vec::with_capacity(steps.len() + 1);
    for st in steps {
        start.push(out.len() as u32);
        push(&st.norm, st, &mut out);
    }
    start.push(out.len() as u32);
    let prompts: Vec<input::Prompt> = if s.prompts.is_empty() {
        vec![input::Prompt { text: s.prompt.clone(), at_step: 0 }]
    } else {
        s.prompts
            .iter()
            .filter(|p| p.at_step as usize <= steps.len())
            .map(|p| input::Prompt {
                text: p.text.clone(),
                at_step: start.get(p.at_step as usize).copied().unwrap_or(out.len() as u32),
            })
            .collect()
    };
    let (steps, meta): (Vec<_>, Vec<_>) = out.into_iter().unzip();
    input::Annotated { session: input::Session { id: s.id.clone(), prompts, steps }, meta, day: s.day.clone() }
}

/// The first bridged step of each import step of `s` (and one past the end).
fn starts(s: &Session) -> Vec<u32> {
    let mut out: Vec<(input::Step, input::StepMeta)> = Vec::new();
    let mut start = Vec::with_capacity(s.steps.len() + 1);
    for st in &s.steps {
        start.push(out.len() as u32);
        push(&st.norm, st, &mut out);
    }
    start.push(out.len() as u32);
    start
}

/// The directives `s` received, as the graph engine reads them: anchored at bridged steps.
pub fn influence(s: &Session) -> Vec<adage_graph::events::Influence> {
    if s.influence.is_empty() {
        return Vec::new();
    }
    let start = starts(s);
    let last = start.last().copied().unwrap_or(0);
    s.graph_influence()
        .into_iter()
        .map(|mut d| {
            d.at_step = start.get(d.at_step as usize).copied().unwrap_or(last);
            d
        })
        .collect()
}

/// The session as the graph engine reads it (what value-flow induction needs).
pub fn to_input(s: &Session) -> input::Session {
    annotate(s).session
}

/// The session as the graph engine's miner reads it (Track G §1.1): [`annotate`], then
/// `adage_graph::events::from_input` with each step's model turn as its batch and its effect
/// class, the day, the outcome and the directives it received ([`influence`]).
pub fn events(a: &input::Annotated, s: &Session) -> adage_graph::events::EventSession {
    let effect = |e: &str| -> Option<adage_core::Effect> {
        Some(match e {
            "observe" => adage_core::Effect::Observe,
            "verify" => adage_core::Effect::Verify,
            "regenerate" => adage_core::Effect::Regenerate,
            "environment" => adage_core::Effect::Environment,
            "author" => adage_core::Effect::Author,
            "publish" => adage_core::Effect::Publish,
            _ => return None,
        })
    };
    let meta = adage_graph::events::Meta {
        day: Some(a.day.clone()),
        task: None,
        outcome: s.outcome,
        turns: Some(a.meta.iter().map(|m| m.turn).collect()),
        effects: Some(a.meta.iter().map(|m| effect(&m.effect)).collect()),
        influence: influence(s),
    };
    adage_graph::events::from_input(&a.session, &meta, &adage_core::CommandTable::load())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::PromptObs;
    use adage_core::{Effect, Normalized};
    use std::collections::BTreeSet;

    fn step(turn: u32, prompt: u16, action: CanonicalAction, rel: Option<&str>, failed: bool) -> StepObs {
        StepObs {
            project_pytest_root_cwd: false,
            turn,
            key: template_key(&action),
            norm: Normalized { action, effect: Effect::Observe, mutating: false, confidence: 1.0, note: String::new() },
            call: adage_trace::Call {
                tool: "Bash".into(),
                input: serde_json::Value::Null,
                output: "out".into(),
                failed,
            },
            rel_path: rel.map(str::to_string),
            prompt,
        }
    }

    #[test]
    fn prompts_are_anchored_to_bridged_steps() {
        let exec =
            |a: &str| CanonicalAction::Exec { argv: a.split(' ').map(str::to_string).collect(), cwd: "/r".into() };
        let s = Session {
            id: "s".into(),
            root: "/r".into(),
            repo: "r".into(),
            model: None,
            day: "2026-09-01".into(),
            prompt: "first".into(),
            steps: vec![
                step(0, 0, exec("ls"), None, false),
                // A read with no repository path is left out.
                step(0, 0, CanonicalAction::Read { path: "/etc/x".into(), range: None }, None, false),
                step(1, 1, exec("make test"), None, true),
                step(1, 1, CanonicalAction::Edit { path: "/r/a.py".into() }, Some("a.py"), false),
            ],
            steered: false,
            directed: BTreeSet::new(),
            influence: vec![],
            outcome: None,
            harness: "claude-code".into(),
            prompts: vec![
                PromptObs { text: "first".into(), at_step: 0 },
                PromptObs { text: "second".into(), at_step: 2 },
                PromptObs { text: "after".into(), at_step: 4 },
            ],
            ending: Default::default(),
        };
        let a = annotate(&s);
        assert_eq!(a.session.steps.len(), 3);
        let at: Vec<u32> = a.session.prompts.iter().map(|p| p.at_step).collect();
        assert_eq!(at, [0, 1, 3]);
        assert_eq!(a.meta[1].key, "exec make test");
        assert_eq!((a.meta[2].turn, a.meta[2].prompt), (1, 1));
        assert_eq!(a.day, "2026-09-01");
        let mut old = s.clone();
        old.prompts.clear();
        assert_eq!(to_input(&old).prompts, vec![input::Prompt { text: "first".into(), at_step: 0 }]);
    }
}
