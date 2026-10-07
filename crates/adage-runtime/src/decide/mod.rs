//! [`decide`]: one canonical event, the session's state and the index in; the directives, the
//! judgements and the state's changes out. Each event has its own arm in its own module
//! (`prompt`, `batch`, `failure`, `stop`, and the `TurnStep` in [`crate::prediction`]); what
//! every arm shares is here: the routes in play and the context nets ([`Walk`]), the caps
//! ([`emit_ok`]), firing a context, and approach locking, which runs on every decision's
//! directives last.

mod batch;
mod failure;
mod prompt;
mod stop;

use crate::context::{self, Symbols};
use crate::decision::directive;
use crate::policy::in_play;
use crate::{
    command_key, prediction, render, Arm, Ctx, Decision, Directive, Event, Obligation, ObligationState, Pending,
    PendingReads, SessionState,
};
use adage_core::keys;
use adage_tracks::context::CtxNet;
use adage_tracks::{interlock, IndexedRoute, Route, Step, Trigger};

/// What the main loop's steps are predicted from ([`crate::prediction`]): the pending directives
/// with a bound command, the bound read batches, the finish-line obligations, and the routes
/// already predicted. A subagent's call (`Call::agent`) changes none of it (the Claude Code mod's
/// architecture §3.8: predictions stay on the main loop): it is observed and recorded, and a
/// directive it triggers is said to the subagent, but it binds nothing the main loop would then
/// make, follows no directive the main loop was given, and meets no obligation the main loop's
/// edits armed. Taken before such a call and put back after it.
struct MainThread {
    pending: Vec<Pending>,
    reads: Vec<PendingReads>,
    obligations: Vec<Obligation>,
    predicted_routes: Vec<String>,
}

impl MainThread {
    fn keep(st: &SessionState) -> Self {
        MainThread {
            pending: st.pending.clone(),
            reads: st.reads.clone(),
            obligations: st.obligations.clone(),
            predicted_routes: st.predicted_routes.clone(),
        }
    }

    fn restore(self, st: &mut SessionState) {
        st.pending = self.pending;
        st.reads = self.reads;
        st.obligations = self.obligations;
        st.predicted_routes = self.predicted_routes;
    }
}

/// What every arm reads besides the context and the state: the routes in play here, and the two
/// context nets (the routes', Track G §7, and the candidates', plan D2) with their symbolisers,
/// present only when the repository has such contexts.
struct Walk<'w, 'n> {
    routes: &'w [&'n IndexedRoute],
    net: &'w CtxNet,
    syms: Option<&'w Symbols<'n>>,
    cnet: &'w CtxNet,
    csyms: Option<&'w Symbols<'n>>,
}

pub fn decide(ctx: &Ctx, st: &mut SessionState, ev: &Event) -> Decision {
    let mut d = Decision::default();
    let obligations_before = ObligationState::capture(st);
    // Exec-only routes (vendored scripts) are carried out by adage-exec, never directed on their
    // own ([`Policy::exec_only`](crate::Policy::exec_only)).
    let routes: Vec<&IndexedRoute> = ctx.index.for_repo(&ctx.repo).filter(|r| in_play(&ctx.policy, r)).collect();
    // Context triggers (Track G §7): the repository's alphabet and net, and a symboliser for it,
    // only when it has any.
    let net = ctx.index.contexts(&ctx.repo);
    let syms = (!net.is_empty()).then(|| context::Symbols::new(&net).exact_keys(ctx.policy.exact_keys));
    let ctx_before = syms.as_ref().map(|_| st.ctx.clone());
    // Candidates (plan D2): their own alphabet and net, symbolised from the same events, walked
    // into `noticed` and never into a directive; only when the repository has any.
    let cnet = ctx.index.candidate_contexts(&ctx.repo);
    let csyms = (!cnet.is_empty()).then(|| context::Symbols::new(&cnet).exact_keys(ctx.policy.exact_keys));
    let cand_before = csyms.as_ref().map(|_| st.cand_ctx.clone());
    let mut out: Vec<Directive> = Vec::new();
    let w = Walk { routes: &routes, net: &net, syms: syms.as_ref(), cnet: &cnet, csyms: csyms.as_ref() };

    // What the prompt directed for its first step is for that step only: any call, failure or
    // turn's end since means the boundary has passed.
    if matches!(ev, Event::Prompt { .. } | Event::Batch { .. } | Event::Failure { .. } | Event::Stop { .. }) {
        st.at_prompt.clear();
    }
    match ev {
        Event::SessionStart | Event::SessionEnd => {}
        Event::Prompt { text } => prompt::on_prompt(ctx, &w, st, &mut out, text),
        Event::Batch { calls } => batch::on_batch(ctx, &w, st, &mut d, &mut out, calls),
        Event::Failure { call } => failure::on_failure(ctx, &w, st, &mut d, &mut out, call),
        Event::Stop { stop_hook_active } => stop::on_stop(ctx, st, &mut d, *stop_hook_active),
        // The main loop's steps only: a subagent's step is its own turn, not the session's.
        Event::TurnStep { agent: Some(_), .. } => {}
        Event::TurnStep { turn_id, index, ending, agent: None, .. } => {
            prediction::on_turn_step(ctx, &routes, st, &mut d, turn_id, *index, *ending)
        }
    }

    let out = approach_lock(&routes, st, out);
    for o in &out {
        st.emitted.push(o.route.clone());
    }
    if ctx_before.is_some_and(|b| b != st.ctx) {
        d.ctx = Some(st.ctx.clone());
    }
    if cand_before.is_some_and(|b| b != st.cand_ctx) {
        d.cand_ctx = Some(st.cand_ctx.clone());
    }
    d.would_have = out.clone();
    if st.arm != Some(Arm::Control) {
        d.directives = out;
    }
    if obligations_before.obligations != st.obligations
        || obligations_before.stop_requests != st.stop_requests
        || d.observed.iter().any(|key| key.starts_with("edit ") || key.starts_with("write "))
    {
        d.obligation_state = Some(ObligationState::capture(st));
    }
    d
}

/// Whether route `id` may be emitted now: not emitted before this session nor in this decision,
/// and within the per-session and per-decision caps.
fn emit_ok(ctx: &Ctx, st: &SessionState, out: &[Directive], id: &str) -> bool {
    !st.emitted.iter().any(|e| e == id)
        && !out.iter().any(|o| o.route == id)
        && st.emitted.len() + out.len() < ctx.params.max_directives_per_session
        && out.len() < ctx.params.max_directives_per_decision
}

/// The context routes the newest alphabet event completes ([`CtxNet::walk`]), in index order,
/// each emitted if `ok` allows: a run step, a sequence or a skill, from its fixed template, with a
/// follow judgement on its (first) command or on the skill's use. No slot is bound and no obligation is made: a context
/// directs fixed commands only (`io::validate`), and never leads to a Stop request.
fn fire_contexts(
    ctx: &Ctx,
    routes: &[&IndexedRoute],
    net: &CtxNet,
    st: &mut SessionState,
    out: &mut Vec<Directive>,
    ok: impl Fn(&SessionState, &Vec<Directive>, &IndexedRoute) -> bool,
) {
    let recent = st.ctx.recent();
    let fired = net.walk(&recent);
    if fired.is_empty() {
        return;
    }
    for ir in routes.iter().filter(|r| matches!(r.route.trigger, Trigger::Context { .. })) {
        if !fired.contains(ir.route.id.as_str()) || !ok(st, out, ir) {
            continue;
        }
        let pending = match &ir.route.step {
            Step::Run { command } => pending_for(ir, command, ctx),
            Step::Sequence { commands } => match commands.first() {
                Some(c) => pending_for(ir, c, ctx),
                None => continue,
            },
            Step::Skill { skill, .. } => pending_skill(ir, skill, ctx),
            _ => continue,
        };
        let Some(t) = render::route_text(&ir.route, &[]) else { continue };
        st.pending.push(pending);
        out.push(directive(ir, t));
    }
}

/// Approach locking (Design §10.6, "inhibit beats fire"; `adage_tracks::interlock`): once an inhibit
/// directive (a dead end) has been emitted in a session, or is emitted in this decision, no
/// directive whose command it avoids is emitted, in this decision or any later one. A directive
/// dropped here leaves nothing behind: no pending follow judgement and no finish-line obligation,
/// so there is never a stop request to run a command the session was warned fails. A fire
/// emitted before the warning stays as it was (an obligation it made is not withdrawn: a dead end
/// is announced at the first prompt, before any edit can make one).
fn approach_lock(routes: &[&IndexedRoute], st: &mut SessionState, out: Vec<Directive>) -> Vec<Directive> {
    let route_of = |id: &str| routes.iter().find(|r| r.route.id == id).map(|r| &r.route);
    let inhibits: Vec<&Route> = st
        .told()
        .chain(out.iter().map(|d| &d.route))
        .filter_map(|id| route_of(id))
        .filter(|r| matches!(r.step, Step::Avoid { .. }))
        .collect();
    if inhibits.is_empty() {
        return out;
    }
    let (keep, dropped): (Vec<Directive>, Vec<Directive>) = out.into_iter().partition(|d| {
        route_of(&d.route).is_none_or(|r| !inhibits.iter().any(|inhibit| interlock::avoids(inhibit, r)))
    });
    if !dropped.is_empty() {
        let gone = |id: &String| dropped.iter().any(|d| &d.route == id);
        st.pending.retain(|p| !gone(&p.route));
        st.reads.retain(|r| !gone(&r.route));
        st.obligations.retain(|o| !gone(&o.route) && !gone(&o.via));
    }
    keep
}

/// A follow judgement for a directed fixed command (no slots; a command with a slot left in it
/// is kept for the judgement but never predicted).
fn pending_for(ir: &IndexedRoute, command: &str, ctx: &Ctx) -> Pending {
    let argv: Vec<String> = command.split_whitespace().map(str::to_string).collect();
    Pending {
        route: ir.route.id.clone(),
        expect_key: command_key(command, ctx.table),
        expect_head: keys::command_head(&argv),
        batches_left: ctx.params.follow_window,
        command: if command.contains('{') { String::new() } else { command.to_string() },
    }
}

/// A skill step's follow judgement: the agent uses the skill (the canonical key of a Skill tool
/// call, [`adage_tracks::skill_key`]). `command` holds the skill's name, what a predicted step
/// invokes.
fn pending_skill(ir: &IndexedRoute, skill: &str, ctx: &Ctx) -> Pending {
    Pending {
        route: ir.route.id.clone(),
        expect_key: adage_tracks::skill_key(skill),
        expect_head: String::new(),
        batches_left: ctx.params.follow_window,
        command: skill.to_string(),
    }
}
