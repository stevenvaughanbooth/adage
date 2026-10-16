//! Text said once rather than at an event: a session's standing text (the Claude Code mod's
//! `Standing` event) and what a subagent is told before its first request (`agent.spawn`). Both
//! are read-only over the session state.

use crate::binding::bind_prompt_route;
use crate::decision::directive;
use crate::policy::in_play;
use crate::prediction::told_inhibits;
use crate::{render, Arm, Ctx, Policy, SessionState};
use adage_core::spans;
use adage_tracks::{interlock, Index, IndexedRoute, Step, Strength, Trigger};
use serde::{Deserialize, Serialize};

/// What a session's standing text is (the Claude Code mod's `Standing` event, its architecture
/// §3.6): fixed for the session once answered.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Standing {
    /// The repository's standing reflexes, each in its directive wrapper, at most
    /// [`render::MAX_STANDING`] bytes ([`standing()`]).
    pub standing: String,
    /// One sentence on the project's environment, when its instincts bound one.
    pub environment: String,
    /// The repository's lookup conventions, one sentence each ([`render::lookup_convention`]),
    /// for the shell tool's description: how a work item (a commit, a link) named in a request is
    /// looked up here, before the model's first call. The prompt that names one is still told
    /// the bound command (its directive carries the item, and the steps that follow it).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub conventions: String,
    /// The routes whose text `standing` carries: the first prompt does not announce them again.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub routes: Vec<String>,
}

/// A session's standing text (the Claude Code mod's `Standing` event, its architecture §3.6): the
/// repository's reflexes said once at a session's start, whatever the task (its dead ends and the
/// instincts' session-start advice), each in the wrapper and fixed template a directive has,
/// then one line per procedure card ([`render::card_line`]), in index order, whole items only and
/// at most [`render::MAX_STANDING`] bytes together. `environment` is the hook's sentence on the
/// project's environment ([`render::environment_note`]), when one is bound; `conventions` the
/// repository's lookup conventions ([`Standing::conventions`]). Deterministic for an
/// index: the hook keeps the first answer for the session ([`SessionState::standing`]).
pub fn standing(index: &Index, repo: &str, policy: &Policy, environment: String) -> Standing {
    let conventions = conventions(index, repo, policy);
    let (mut text, mut routes) = (String::new(), Vec::new());
    for (item, route) in standing_items(index, repo, policy) {
        let sep = if text.is_empty() { 0 } else { 2 };
        if text.len() + sep + item.len() > render::MAX_STANDING {
            continue;
        }
        if sep > 0 {
            text.push_str("\n\n");
        }
        text.push_str(&item);
        routes.extend(route);
    }
    Standing { standing: text, environment, conventions, routes }
}

/// At most this many lookup conventions are said in the shell tool's description.
const MAX_CONVENTIONS: usize = 3;

/// The repository's lookup conventions ([`Standing::conventions`]): each learned prompt route in
/// play at full strength whose step is a command with the item's slot, in index order, one
/// sentence each, at most [`MAX_CONVENTIONS`] and [`render::MAX_STANDING`] bytes together. Not an
/// instinct's (its evidence is not this repository's), and not one a session-start dead end says
/// not to run.
fn conventions(index: &Index, repo: &str, policy: &Policy) -> String {
    let routes: Vec<&IndexedRoute> = index.for_repo(repo).filter(|r| in_play(policy, r)).collect();
    let dead_ends: Vec<&IndexedRoute> = routes
        .iter()
        .copied()
        .filter(|r| matches!(r.route.trigger, Trigger::SessionStart) && matches!(r.route.step, Step::Avoid { .. }))
        .collect();
    let mut lines: Vec<String> = Vec::new();
    for ir in routes.iter().filter(|r| matches!(r.route.trigger, Trigger::Prompt { .. })) {
        if ir.route.is_instinct()
            || adage_tracks::delivered_strength(&ir.route) != Strength::Fire
            || dead_ends.iter().any(|d| interlock::avoids(&d.route, &ir.route))
        {
            continue;
        }
        let Some(line) = render::lookup_convention(&ir.route) else { continue };
        let total: usize = lines.iter().map(|l| l.len() + 1).sum();
        if lines.len() < MAX_CONVENTIONS && !lines.contains(&line) && total + line.len() <= render::MAX_STANDING {
            lines.push(line);
        }
    }
    lines.join("\n")
}

/// The standing text's items, in order, each with the route a directive carries (none for a
/// card's line): [`standing()`] and [`delegation`] bound them.
fn standing_items(index: &Index, repo: &str, policy: &Policy) -> Vec<(String, Option<String>)> {
    let mut items: Vec<(String, Option<String>)> = Vec::new();
    for ir in index.for_repo(repo).filter(|r| in_play(policy, r)) {
        if let (Trigger::SessionStart, Some(t)) = (&ir.route.trigger, render::route_text(&ir.route, &[])) {
            items.push((render::wrap_directive(&directive(ir, t)), Some(ir.route.id.clone())));
        }
    }
    for ir in index.for_repo(repo).filter(|r| in_play(policy, r)) {
        if let Step::Card { card } = &ir.route.step {
            if let Some(line) = render::card_line(card) {
                items.push((render::wrap_directive(&directive(ir, line)), None));
            }
        }
    }
    items
}

/// What of a session's standing text a harness can attach (the Claude Code mod's architecture
/// §1b.3), by capability: the standing reflexes go to a cached system-prompt section only where
/// there is one (`cached_section`), and the environment sentence and the lookup conventions to the
/// shell tool's description only where the description can carry them (`tool_description`).
/// Anything else is left empty.
pub fn standing_for(caps: &adage_trace::Capabilities, s: Standing) -> Standing {
    Standing {
        standing: if caps.cached_section { s.standing } else { String::new() },
        environment: if caps.tool_description { s.environment } else { String::new() },
        conventions: if caps.tool_description { s.conventions } else { String::new() },
        routes: if caps.cached_section { s.routes } else { Vec::new() },
    }
}

/// What a subagent is told before its first request (the Claude Code mod's `agent.spawn`, its
/// architecture §3.8): the prompt reflexes its own prompt binds (a work-item lookup for the item
/// the delegation names), then the repository's standing reflexes ([`standing()`]), each in its
/// directive wrapper and fixed template, whole items only, at most [`render::MAX_STANDING`] bytes
/// together. Nothing a dead end the session was told says not to run. Empty unless the harness
/// can attach text to a subagent's prompt (`subagent_prompt`), in the control arm, and when
/// nothing applies. Read-only: what a subagent is told is not what the session was told, so no
/// state changes.
pub fn delegation(ctx: &Ctx, st: &SessionState, prompt: &str, caps: &adage_trace::Capabilities) -> String {
    if !caps.subagent_prompt || st.arm == Some(Arm::Control) || ctx.policy.observe {
        return String::new();
    }
    let routes: Vec<&IndexedRoute> = ctx.index.for_repo(&ctx.repo).filter(|r| in_play(&ctx.policy, r)).collect();
    let inhibits = told_inhibits(&routes, st);
    let spans = spans::extract(prompt);
    let mut items: Vec<String> = Vec::new();
    for ir in routes.iter().filter(|r| matches!(r.route.trigger, Trigger::Prompt { .. })) {
        if inhibits.iter().any(|inhibit| interlock::avoids(inhibit, &ir.route)) {
            continue;
        }
        if let Some((text, ..)) = bind_prompt_route(&ir.route, &spans, prompt, ctx) {
            items.push(render::wrap_directive(&directive(ir, text)));
        }
    }
    items.extend(standing_items(ctx.index, &ctx.repo, &ctx.policy).into_iter().map(|(item, _)| item));
    let mut text = String::new();
    for item in items {
        let sep = if text.is_empty() { 0 } else { 2 };
        if text.len() + sep + item.len() > render::MAX_STANDING || text.contains(&item) {
            continue;
        }
        if sep > 0 {
            text.push_str("\n\n");
        }
        text.push_str(&item);
    }
    text
}
