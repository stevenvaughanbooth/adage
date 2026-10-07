//! The `Batch` arm of [`crate::decide`] (a `PostToolBatch`): per call, the follow judgements of
//! pending directives, failure triggers a piped failure meets, obligations met, what an edit
//! triggers, what a successful command triggers, and the context alphabets; then the follow
//! window closes on what is still pending.

use super::failure::fire_after_failure;
use super::prompt::card_text;
use super::{emit_ok, fire_contexts, pending_for, pending_skill, Walk};
use crate::binding::{bind_edit_slots, bind_output_route, bind_result_route, binds_output};
use crate::context::{self, Symbols};
use crate::decision::directive;
use crate::noticed::notice;
use crate::observe::{added_tests, batch_keys, call_command, cap, exec_segments};
use crate::policy::{follows_call, follows_learned};
use crate::state::push_history_capped;
use crate::{
    command_key, identity_hash, render, By, Call, Ctx, CtxState, Decision, Directive, Judgement, Obligation, Pending,
    PendingReads, SessionState, MAX_NEW_TESTS,
};
use adage_core::{keys, outcome, pattern, repo};
use adage_tracks::{IndexedRoute, Step, Trigger};
use serde_json::Value;
use std::path::Path;

pub(super) fn on_batch(
    ctx: &Ctx,
    w: &Walk,
    st: &mut SessionState,
    d: &mut Decision,
    out: &mut Vec<Directive>,
    calls: &[Call],
) {
    st.batches += 1;
    for o in st.obligations.iter_mut() {
        o.fresh = false;
    }
    // A failed call the failure hook just reported, listed again in this batch.
    let mut open_failure = w.syms.and_then(|_| st.ctx.open_failure(w.net));
    let mut cand_open_failure = w.csyms.and_then(|_| st.cand_ctx.open_failure(w.cnet));
    // A call the mod answered with a receipt was observed then: listed in the batch too,
    // it is not observed again. Keyed by the bare `tool_use_id` (`SessionState::answered`).
    let answered = |c: &&Call| c.tool_use_id.as_deref().is_some_and(|id| st.answered.contains(&identity_hash(id)));
    let calls: Vec<&Call> = calls.iter().filter(|c| !answered(c)).collect();
    for call in calls {
        for name in added_tests(call, ctx.root) {
            if st.new_tests.len() < MAX_NEW_TESTS && !st.new_tests.contains(&name) {
                st.new_tests.push(name);
            }
        }
        // A piped command exits with its last stage's status: `pytest … | head` exits 0
        // when pytest could not even start. Its output still says so (`outcome`).
        let piped = !call.failed && call_command(call).is_some_and(outcome::piped);
        let piped_failed = piped && outcome::piped_failure(&call.output);
        let misfire = (call.failed || piped) && outcome::misfired(&call.output);
        retire_reads(ctx, st, call, d);
        let keyed = batch_keys(call, ctx);
        let segs = exec_segments(&keyed);
        // The call is the step Adage made in the agent's place, or (in the arms that only
        // record) one it would have made: what a chain's second step may follow at once.
        let mut stood_in = call.by == By::Adage;
        for (key, n) in keyed {
            let key = cap(key);
            d.observed.push(key.clone());
            push_history_capped(st, key.clone(), ctx.params.history);
            // Follow / ignore judgement: the directed command, with whatever the agent
            // added to narrow it ([`follows`]). Followed and failed without the runner
            // reporting tests (or, piped, printing an environment failure) is a misfire.
            st.pending.retain(|p| {
                if ctx.policy.follows(&p.expect_key, &key)
                    || follows_call(&ctx.policy, &p.expect_key, &segs)
                    || follows_learned(&ctx.policy, &p.expect_key, call)
                {
                    let route = p.route.clone();
                    stood_in |= st.predicted_routes.contains(&route);
                    d.judgements.push(if call.by == By::Adage {
                        Judgement::Predicted { route }
                    } else if misfire {
                        Judgement::Misfired { route }
                    } else {
                        Judgement::Followed { route }
                    });
                    false
                } else {
                    true
                }
            });
            // Failure-triggered dead ends and instincts (never obligations) see a piped
            // failure too, by its output; each still needs its own signature there.
            if piped_failed {
                fire_after_failure(ctx, w.routes, st, out, &key, &call.output);
            }
            // Obligations met.
            for o in st.obligations.iter_mut().filter(|o| !o.met && ctx.policy.meets(&o.expect_key, &key)) {
                o.met = true;
                d.judgements.push(Judgement::ObligationMet { route: o.route.clone() });
                // Met by the call Adage made in the agent's place: its step, not the agent's.
                if call.by == By::Adage {
                    d.judgements.push(Judgement::Predicted { route: o.route.clone() });
                }
            }
            // Edits trigger finish-line obligations: an edit tool's, or a file a
            // Bash command wrote (`batch_keys`), right after the action that wrote it.
            if keys::is_author(&n.action) {
                st.first_edit_at_batch.get_or_insert(st.batches);
                if let Some(p) = keys::action_path(&n.action) {
                    after_edit(ctx, w.routes, st, out, &repo::relativize(ctx.root, p));
                }
            }
            // Routes triggered by a successful command (e.g. read the files it listed).
            if !call.failed {
                after_command(ctx, w.routes, st, out, call, &key, stood_in);
            }
            // Context triggers: symbolise the call, push it if it is an alphabet event,
            // and walk the net from it.
            if let Some(sy) = w.syms {
                let got = alphabet_items(sy, ctx.root, &n, &key, call, &mut open_failure, &mut st.ctx);
                if st.ctx.push(w.net, &got) {
                    fire_contexts(ctx, w.routes, w.net, st, out, |st, out, ir| emit_ok(ctx, st, out, &ir.route.id));
                }
            }
            // The same for the candidates' alphabet: noticed, never directed.
            if let Some(cs) = w.csyms {
                let got = alphabet_items(cs, ctx.root, &n, &key, call, &mut cand_open_failure, &mut st.cand_ctx);
                if st.cand_ctx.push(w.cnet, &got) {
                    notice(ctx, w.cnet, st, d);
                }
            }
        }
    }
    for p in st.pending.iter_mut() {
        p.batches_left = p.batches_left.saturating_sub(1);
        if p.batches_left == 0 {
            d.judgements.push(Judgement::Ignored { route: p.route.clone() });
        }
    }
    st.pending.retain(|p| p.batches_left > 0);
    for r in st.reads.iter_mut() {
        r.batches_left = r.batches_left.saturating_sub(1);
    }
    st.reads.retain(|r| r.batches_left > 0);
}

/// What an edit of the repository-relative path `rel` triggers: a procedure card waiting for it,
/// advice, a skill, a sequence (carrying the finish lines it subsumes), and a finish line, which
/// is an obligation once the agent was told it (an instinct's is advice only).
fn after_edit(ctx: &Ctx, routes: &[&IndexedRoute], st: &mut SessionState, out: &mut Vec<Directive>, rel: &str) {
    // A procedure card a prompt came close to: said at the first edit of
    // one of its files.
    if let Some(id) = st.card_waiting.clone() {
        let hit = routes
            .iter()
            .find(|r| r.route.id == id)
            .filter(|r| matches!(&r.route.step, Step::Card { card } if adage_tracks::card::touches(card, rel)));
        if let Some(ir) = hit {
            st.card_waiting = None;
            if emit_ok(ctx, st, out, &ir.route.id) {
                if let Some(t) = card_text(ctx, &ir.route) {
                    out.push(directive(ir, t));
                }
            }
        }
    }
    let hits = |paths: &str| glob::Pattern::new(paths).is_ok_and(|g| g.matches(rel));
    // One edit, one directive: a sequence after this edit with a step that
    // is a finish line's command subsumes that finish line (a compiled
    // skill's `make test`, `make ci` carries its "before you finish: make
    // ci", which holds after any edit). Returns the sequence and the
    // finish line's bound command.
    let subsumer = |o: &IndexedRoute| -> Option<(String, String)> {
        let (Trigger::AfterEdit { paths }, Step::Run { command }) = (&o.route.trigger, &o.route.step) else {
            return None;
        };
        if !hits(paths) {
            return None;
        }
        let command = render::bind(command, &bind_edit_slots(&o.route, rel, ctx)?);
        routes
            .iter()
            .find(|s| match (&s.route.trigger, &s.route.step) {
                (Trigger::AfterEdit { paths }, Step::Sequence { commands }) => {
                    hits(paths) && commands.iter().any(|f| pattern::same_command(f, &command))
                }
                _ => false,
            })
            .map(|s| (s.route.id.clone(), command))
    };
    for ir in routes.iter() {
        // Advice after an edit (a generated file, a lockfile): said once.
        if let (Trigger::AfterEdit { paths }, Step::Advise { .. }) = (&ir.route.trigger, &ir.route.step) {
            let hit = glob::Pattern::new(paths).map(|g| g.matches(rel)).unwrap_or(false);
            if hit && emit_ok(ctx, st, out, &ir.route.id) {
                if let Some(t) = render::route_text(&ir.route, &[]) {
                    out.push(directive(ir, t));
                }
            }
            continue;
        }
        // A skill after an edit (mod architecture §3.11): said once, judged on
        // the agent using it; never an obligation.
        if let (Trigger::AfterEdit { paths }, Step::Skill { skill, .. }) = (&ir.route.trigger, &ir.route.step) {
            if hits(paths) && emit_ok(ctx, st, out, &ir.route.id) {
                if let Some(t) = render::route_text(&ir.route, &[]) {
                    st.pending.push(pending_skill(ir, skill, ctx));
                    out.push(directive(ir, t));
                }
            }
            continue;
        }
        // A sequence after an edit (Track G §6.1): said once, and judged on its
        // first command. On its own it is not an obligation, but it carries
        // the obligations of the finish lines it subsumes (their command is
        // its first step), which then say nothing themselves.
        if let (Trigger::AfterEdit { paths }, Step::Sequence { commands }) = (&ir.route.trigger, &ir.route.step) {
            if !hits(paths) {
                continue;
            }
            let announced = st.emitted.iter().any(|e| e == &ir.route.id);
            let emitted_now = !announced && emit_ok(ctx, st, out, &ir.route.id) && {
                match (render::route_text(&ir.route, &[]), commands.first()) {
                    (Some(t), Some(first)) => {
                        st.pending.push(pending_for(ir, first, ctx));
                        out.push(directive(ir, t));
                        true
                    }
                    _ => false,
                }
            };
            if announced || emitted_now {
                for o in routes.iter().filter(|o| !o.route.is_instinct()) {
                    let Some((via, command)) = subsumer(o) else { continue };
                    let Trigger::AfterEdit { paths } = &o.route.trigger else { continue };
                    if via != ir.route.id || st.obligations.iter().any(|x| x.route == o.route.id) {
                        continue;
                    }
                    st.obligations.push(Obligation {
                        route: o.route.id.clone(),
                        expect_key: command_key(&command, ctx.table),
                        command,
                        paths: paths.clone(),
                        met: false,
                        stop_requested: false,
                        edited: rel.to_string(),
                        fresh: true,
                        via,
                    });
                }
            }
            continue;
        }
        if let (Trigger::AfterEdit { paths }, Step::Run { command }) = (&ir.route.trigger, &ir.route.step) {
            let hit = glob::Pattern::new(paths).map(|g| g.matches(rel)).unwrap_or(false);
            if !hit {
                continue;
            }
            // Slots bound from the edited path's segments (none for a
            // fixed command); a slot that cannot bind drops the route.
            let Some(bound) = bind_edit_slots(&ir.route, rel, ctx) else { continue };
            let command = &render::bind(command, &bound);
            let subsumed = subsumer(ir).is_some();
            // An instinct's finish line is advice, never an obligation: no
            // Stop request can come of it. Its directive is judged like
            // any other (followed, ignored, or misfired), and a predict
            // grant on it is used while it is pending (the step after the
            // edit), never at the turn's end, which only an obligation
            // reaches (plan C8). A sequence that begins with its command
            // says it instead.
            if ir.route.is_instinct() {
                if subsumed {
                    continue;
                }
                if emit_ok(ctx, st, out, &ir.route.id) {
                    if let Some(t) = render::route_text(&ir.route, &bound) {
                        st.pending.push(pending_for(ir, command, ctx));
                        out.push(directive(ir, t));
                    }
                }
                continue;
            }
            if let Some(o) = st.obligations.iter_mut().find(|o| o.route == ir.route.id) {
                // A new matching edit after the step ran re-arms the obligation.
                let rearmed = o.met;
                if o.met {
                    o.met = false;
                    o.stop_requested = false;
                }
                o.edited = rel.to_string();
                o.fresh = true;
                // A finish line is predicted once per arming (plan C8): met,
                // and armed again by a new edit, it may be predicted again.
                // One that is still unmet (its predicted call failed) is
                // not: the Stop gate asks for it, as without the mod.
                if rearmed {
                    st.predicted_routes.retain(|r| r != &ir.route.id);
                }
                continue;
            }
            // Carried by the sequence that subsumes it (armed there).
            if subsumed {
                continue;
            }
            // Only enforce a rule the agent was actually told about.
            let announced = st.emitted.iter().any(|e| e == &ir.route.id);
            let emitted_now = !announced && emit_ok(ctx, st, out, &ir.route.id) && {
                match render::route_text(&ir.route, &bound) {
                    Some(t) => {
                        out.push(directive(ir, t));
                        true
                    }
                    None => false,
                }
            };
            if announced || emitted_now {
                st.obligations.push(Obligation {
                    route: ir.route.id.clone(),
                    expect_key: command_key(command, ctx.table),
                    command: command.clone(),
                    paths: paths.clone(),
                    met: false,
                    stop_requested: false,
                    edited: rel.to_string(),
                    fresh: true,
                    via: String::new(),
                });
            }
        }
    }
}

/// What a successful command (canonical key `key`) triggers: advice, a sequence, a run step whose
/// slots bind from its output, or a read batch of the files its result names (bound right after a
/// step Adage made or would have made when `stood_in`).
fn after_command(
    ctx: &Ctx,
    routes: &[&IndexedRoute],
    st: &mut SessionState,
    out: &mut Vec<Directive>,
    call: &Call,
    key: &str,
    stood_in: bool,
) {
    for ir in routes.iter() {
        if let Trigger::AfterCommand { command } = &ir.route.trigger {
            if ctx.policy.meets(command, key) && emit_ok(ctx, st, out, &ir.route.id) {
                let text = match &ir.route.step {
                    Step::Advise { .. } => render::route_text(&ir.route, &[]),
                    Step::Sequence { commands } => {
                        let t = render::route_text(&ir.route, &[]);
                        if let (Some(_), Some(first)) = (&t, commands.first()) {
                            st.pending.push(pending_for(ir, first, ctx));
                        }
                        t
                    }
                    // A command with a slot bound from this command's output.
                    Step::Run { .. } if binds_output(&ir.route) => {
                        match bind_output_route(&ir.route, &call.output, ctx) {
                            Some((t, key, head, command)) => {
                                st.pending.push(Pending {
                                    route: ir.route.id.clone(),
                                    expect_key: key,
                                    expect_head: head,
                                    batches_left: ctx.params.follow_window,
                                    command,
                                });
                                Some(t)
                            }
                            None => None,
                        }
                    }
                    _ => bind_result_route(&ir.route, &call.output, ctx).map(|(t, files)| {
                        st.reads.push(PendingReads {
                            route: ir.route.id.clone(),
                            files,
                            batches_left: ctx.params.follow_window,
                            after_adage: stood_in,
                        });
                        t
                    }),
                };
                if let Some(t) = text {
                    out.push(directive(ir, t));
                }
            }
        }
    }
}

/// The items of one net's alphabet that a call's canonical action `n` (key `key`) is: an edit of
/// a path, a failure, or a successful command, sorted and deduplicated. A success of the command
/// whose failure the failure hook already reported (`open`, the net's open failure) is that
/// failure listed again in the batch, not a success: it adds nothing and closes the failure.
fn alphabet_items(
    sy: &Symbols,
    root: &Path,
    n: &adage_core::Normalized,
    key: &str,
    call: &Call,
    open: &mut Option<u32>,
    state: &mut CtxState,
) -> Vec<u32> {
    let mut got = Vec::new();
    if keys::is_author(&n.action) {
        if let Some(p) = keys::action_path(&n.action) {
            got.extend(sy.edit(&repo::relativize(root, p)));
        }
    }
    if call.failed {
        got.extend(sy.fail(key, call));
    } else {
        let acts = sy.act(key);
        if !acts.is_empty() && *open == Some(context::key_hash(key)) {
            *open = None;
            state.last_fail = None;
        } else {
            got.extend(acts);
        }
    }
    got.sort_unstable();
    got.dedup();
    got
}

/// A read of one of a pending read batch's files retires it ([`PendingReads`]); when Adage made
/// the read (a predicted chain step), it is judged [`Judgement::Predicted`].
fn retire_reads(ctx: &Ctx, st: &mut SessionState, call: &Call, d: &mut Decision) {
    if st.reads.is_empty() || call.failed || call.tool != "Read" {
        return;
    }
    let Some(path) = call.input.get("file_path").and_then(Value::as_str) else { return };
    let rel = repo::relativize(ctx.root, path);
    st.reads.retain(|r| {
        if !r.files.iter().any(|f| f == &rel) {
            return true;
        }
        if call.by == By::Adage
            && !d.judgements.iter().any(|j| matches!(j, Judgement::Predicted { route } if route == &r.route))
        {
            d.judgements.push(Judgement::Predicted { route: r.route.clone() });
        }
        false
    });
}
