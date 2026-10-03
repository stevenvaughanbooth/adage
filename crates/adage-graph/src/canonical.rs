//! The canonical model as the graph engine reads it: a recorded [`Session`] (`adage_trace`,
//! whatever harness and source it came from) becomes the engine's input, and the directives it
//! received become the influence the miner discounts (Track G §3.2, steering by influence).
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

use crate::input;
use adage_core::keys::template_key;
use adage_core::types::{CanonicalAction, Effect, Normalized};
use adage_core::CommandTable;
use adage_trace::session::{Directed, Session, StepObs};

/// The effect class of a command as the graph engine labels it: the command's own. Each part of a
/// compound keeps its own class (in `cargo fmt --all && git commit -m …` the format step
/// regenerates; it is no author step), and a command whose output was redirected to a file is
/// still the command it ran (`cargo test --workspace > log` verifies; the log is the agent's).
pub fn effect_of(n: &Normalized) -> Effect {
    match &n.action {
        CanonicalAction::Exec { argv, .. } if n.note.starts_with("redirected to ") => {
            crate::events::classify(argv, table())
        }
        _ => n.effect,
    }
}

fn table() -> &'static CommandTable {
    CommandTable::load()
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
        // A skill used (plan §2.1.2): the only control tool the learner reads. Every other
        // control tool (a todo list, a subagent, an MCP call) stays out, as before.
        CanonicalAction::Control { tool } => {
            if let Some(name) = tool.strip_prefix("Skill ").filter(|n| adage_tracks::safe_skill_name(n)) {
                out.push((input::Step::Skill { name: name.to_string() }, meta(adage_tracks::skill_key(name))));
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
pub fn influence(s: &Session) -> Vec<crate::events::Influence> {
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
/// `crate::events::from_input` with each step's model turn as its batch and its effect
/// class, the day, the outcome and the directives it received ([`influence`]).
pub fn events(a: &input::Annotated, s: &Session) -> crate::events::EventSession {
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
    let meta = crate::events::Meta {
        day: Some(a.day.clone()),
        task: None,
        outcome: s.outcome,
        turns: Some(a.meta.iter().map(|m| m.turn).collect()),
        effects: Some(a.meta.iter().map(|m| effect(&m.effect)).collect()),
        influence: influence(s),
    };
    crate::events::from_input(&a.session, &meta, adage_core::CommandTable::load())
}

/// The HeadSub identities a directive named, and the directive as the graph engine reads it.
pub trait DirectiveIdentities {
    /// HeadSub identities of every command and runner it named.
    fn identities(&self) -> Vec<String>;
    /// As the graph engine reads it.
    fn influence(&self) -> crate::events::Influence;
}

impl DirectiveIdentities for Directed {
    fn identities(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .commands
            .iter()
            .chain(&self.avoided)
            .map(|c| crate::events::act_identity(&c.split_whitespace().map(str::to_string).collect::<Vec<_>>()))
            .filter(|x| !x.is_empty())
            .collect();
        v.sort();
        v.dedup();
        v
    }

    fn influence(&self) -> crate::events::Influence {
        crate::events::Influence {
            at_step: self.at_step,
            identities: self.identities(),
            paths: self.files.clone(),
            skills: self.skills.clone(),
            all: self.all,
        }
    }
}

/// What a session was exposed to: the directives it received, as evidence rules read them.
pub trait Exposure {
    /// Whether a directive this session received named any of `commands` (by HeadSub identity)
    /// or, with `reads`, any file: the session is then no evidence for a route directing them
    /// (Track G §3.2).
    fn exposed_to(&self, commands: &[&str], reads: bool) -> bool;
    /// Whether a directive this session received named a command of HeadSub identity `identity`.
    fn exposed_identity(&self, identity: &str) -> bool;
    /// The directives as the graph engine reads them.
    fn graph_influence(&self) -> Vec<crate::events::Influence>;
}

impl Exposure for Session {
    fn exposed_to(&self, commands: &[&str], reads: bool) -> bool {
        let ids: Vec<String> = commands
            .iter()
            .map(|c| crate::events::act_identity(&c.split_whitespace().map(str::to_string).collect::<Vec<_>>()))
            .collect();
        self.influence.iter().any(|d| {
            d.all || (reads && !d.files.is_empty()) || d.identities().iter().any(|i| ids.iter().any(|x| x == i))
        })
    }

    fn exposed_identity(&self, identity: &str) -> bool {
        self.influence.iter().any(|d| d.all || d.identities().iter().any(|i| i == identity))
    }

    fn graph_influence(&self) -> Vec<crate::events::Influence> {
        self.influence.iter().map(DirectiveIdentities::influence).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use adage_core::{Effect, Normalized};
    use adage_trace::session::PromptObs;
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
                by: adage_trace::By::Agent,
                tool_use_id: None,
                agent: None,
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

    use adage_trace::session::CLAUDE_CODE;
    use std::path::PathBuf;

    fn v(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn a_session_is_exposed_only_to_what_its_directives_named() {
        let mut s = Session {
            id: "s".into(),
            root: PathBuf::from("/r"),
            repo: "r".into(),
            model: None,
            day: String::new(),
            prompt: "p".into(),
            steps: vec![],
            steered: true,
            directed: ["dead-end-1".to_string()].into(),
            influence: vec![Directed {
                at_step: 0,
                route: "dead-end-1".into(),
                commands: v(&["make test"]),
                avoided: v(&["python3 -m pytest"]),
                ..Default::default()
            }],
            outcome: None,
            harness: CLAUDE_CODE.into(),
            prompts: vec![PromptObs { text: "p".into(), at_step: 0 }],
            ending: Default::default(),
        };
        assert!(s.exposed_to(&["make test"], false) && s.exposed_to(&["python3 -m pytest tests"], false));
        assert!(!s.exposed_to(&["make gen"], false) && !s.exposed_to(&["./scripts/wi-lookup {item}"], true));
        s.influence[0].all = true;
        assert!(s.exposed_to(&["make gen"], false));
    }

    /// T-D3-04: a Skill tool use becomes a skill step, with its key; every other control tool is
    /// left out as before; a skill directive's influence names that skill alone.
    #[test]
    fn a_skill_used_is_a_skill_step_and_no_other_control_tool_is() {
        let control = |tool: &str| CanonicalAction::Control { tool: tool.into() };
        let s = Session {
            id: "s".into(),
            root: "/r".into(),
            repo: "r".into(),
            model: None,
            day: "2026-09-01".into(),
            prompt: "release it".into(),
            steps: vec![
                step(0, 0, control("TodoWrite"), None, false),
                step(1, 0, control("Skill release-checklist"), None, false),
                step(2, 0, control("Task general-purpose"), None, false),
                step(3, 0, control("Skill ../etc"), None, false),
            ],
            steered: false,
            directed: BTreeSet::new(),
            influence: vec![Directed {
                at_step: 1,
                route: "skill-1".into(),
                skills: v(&["release-checklist"]),
                ..Default::default()
            }],
            outcome: None,
            harness: CLAUDE_CODE.into(),
            prompts: vec![],
            ending: Default::default(),
        };
        let a = annotate(&s);
        assert_eq!(a.session.steps, vec![input::Step::Skill { name: "release-checklist".into() }]);
        assert_eq!(a.meta[0].key, "control Skill release-checklist");
        let e = events(&a, &s);
        assert_eq!(e.events.iter().filter(|x| x.ty == crate::events::NodeType::Skill).count(), 1);
        assert_eq!(e.exposed, vec![1], "the skill the directive named, from its delivery on");
        let inf = &influence(&s)[0];
        assert!(inf.names_skill("release-checklist") && !inf.names_skill("other") && !inf.names_act("make"));
    }
}
