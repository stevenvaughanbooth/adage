//! The `Failure` arm of [`crate::decide`] (a `PostToolUseFailure`): the failed call's follow
//! judgements, the after-failure triggers it meets, and its failure item in the context
//! alphabets.

use super::{emit_ok, fire_contexts, pending_for, Walk};
use crate::decision::directive;
use crate::noticed::notice;
use crate::observe::{call_keys, exec_segments};
use crate::policy::{failure_matches, follows_call, follows_learned};
use crate::state::push_history_capped;
use crate::{render, By, Call, Ctx, Decision, Directive, Judgement, SessionState};
use adage_core::outcome;
use adage_tracks::{IndexedRoute, Step, Trigger};

pub(super) fn on_failure(
    ctx: &Ctx,
    w: &Walk,
    st: &mut SessionState,
    d: &mut Decision,
    out: &mut Vec<Directive>,
    call: &Call,
) {
    let keyed = call_keys(call, ctx);
    let segs = exec_segments(&keyed);
    for (key, _) in keyed {
        d.observed.push(format!("{key} !failed"));
        push_history_capped(st, format!("{key} !failed"), ctx.params.history);
        // The directed command, run within the follow window, failed: a misfire, unless
        // it is a test run that ran and reported failing tests (followed; the tests
        // failing is the task, not the directive).
        st.pending.retain(|p| {
            if ctx.policy.follows(&p.expect_key, &key)
                || follows_call(&ctx.policy, &p.expect_key, &segs)
                || follows_learned(&ctx.policy, &p.expect_key, call)
            {
                let route = p.route.clone();
                d.judgements.push(if call.by == By::Adage {
                    Judgement::Predicted { route }
                } else if outcome::misfired(&call.output) {
                    Judgement::Misfired { route }
                } else {
                    Judgement::Followed { route }
                });
                false
            } else {
                true
            }
        });
        fire_after_failure(ctx, w.routes, st, out, &key, &call.output);
        // Context triggers: a failure item, and the record that keeps the batch after it
        // from counting this call as a successful command.
        if let Some(sy) = w.syms {
            let pushed = st.ctx.push(w.net, &sy.fail(&key, call));
            if !sy.act(&key).is_empty() {
                st.ctx.failed(w.net, &key);
            }
            if pushed {
                fire_contexts(ctx, w.routes, w.net, st, out, |st, out, ir| emit_ok(ctx, st, out, &ir.route.id));
            }
        }
        if let Some(cs) = w.csyms {
            let pushed = st.cand_ctx.push(w.cnet, &cs.fail(&key, call));
            if !cs.act(&key).is_empty() {
                st.cand_ctx.failed(w.cnet, &key);
            }
            if pushed {
                notice(ctx, w.cnet, st, d);
            }
        }
    }
}

/// Fire the after-failure triggers (dead ends and instincts) a failed run of `key` with this
/// output meets: the trigger's command matches (`failure_matches`) and the route's signature is
/// in the output. A run step, or a sequence's first command, is then judged like any other
/// directive.
pub(super) fn fire_after_failure(
    ctx: &Ctx,
    routes: &[&IndexedRoute],
    st: &mut SessionState,
    out: &mut Vec<Directive>,
    key: &str,
    output: &str,
) {
    let hits = |ir: &&&IndexedRoute| match &ir.route.trigger {
        Trigger::AfterFailure { command, signature } => {
            failure_matches(&ctx.policy, &ir.route, command, key) && output.contains(signature.as_str())
        }
        _ => false,
    };
    let advice = |ir: &IndexedRoute| matches!(ir.route.step, Step::Advise { .. });
    // A failure a step answers (`run this instead`) gets that step, not advice as well: advice
    // after a failure (U11's "run Python here as .venv/bin/python") speaks only when no step
    // matches it, whether or not that step was said before.
    let answered = routes.iter().filter(hits).any(|ir| !advice(ir));
    for ir in routes.iter().filter(hits) {
        if (answered && advice(ir)) || !emit_ok(ctx, st, out, &ir.route.id) {
            continue;
        }
        if let Some(t) = render::route_text(&ir.route, &[]) {
            match &ir.route.step {
                Step::Run { command } => st.pending.push(pending_for(ir, command, ctx)),
                Step::Sequence { commands } => {
                    if let Some(first) = commands.first() {
                        st.pending.push(pending_for(ir, first, ctx));
                    }
                }
                _ => {}
            }
            out.push(directive(ir, t));
        }
    }
}
