//! `TurnStep`: the step Adage makes in the agent's place (the Claude Code mod's `turn.step`, its
//! architecture §3.2), or the step it routes to the person's routine model (§3.5). Only what
//! Adage already directed, only under a person's grant, and within bounds that count in both arms.

use crate::{
    Arm, Ctx, Decision, Permission, PredictedCall, Prediction, PredictionKind, Routing, SessionState, StepPhase,
    Withheld,
};
use adage_core::pattern;
use adage_tracks::{interlock, IndexedRoute, Route, Step, Strength};

/// A main-loop `TurnStep` ([`crate::decide`] leaves a subagent's step alone: it is its own turn,
/// not the session's): the step predicted, or else routed.
pub(crate) fn on_turn_step(
    ctx: &Ctx,
    routes: &[&IndexedRoute],
    st: &mut SessionState,
    d: &mut Decision,
    turn_id: &str,
    index: u32,
    ending: bool,
) {
    predict_step(ctx, routes, st, d, turn_id, index, ending);
    // A step Adage takes in the model's place sends no request to route (a dry run's prediction,
    // not yet counted, included: its verdict routes the step if the prediction is held back).
    if d.predicted_at.is_none() && d.predicted.is_none() && index > 0 {
        route_step(ctx, routes, st, d);
    }
}

/// A `TurnStep`: the step Adage makes in the agent's place, if any (the Claude Code mod's
/// `turn.step`; its architecture §3.2). Only what Adage already directed: a pending directive whose
/// command is bound (a run, a sequence's first command, a skill step), a read batch whose files are
/// bound (a chain), or, at the step that ends the turn, an armed and unmet finish-line obligation,
/// the first in that order whose route
///
/// - holds a `predict` grant ([`crate::Policy::predict_grants`]) and is in play, never an
///   exec-only one;
/// - was not predicted before this session, nor carried out by adage-exec's CPU prediction
///   ([`SessionState::predicted_routes`]), nor held back by the person's permissions
///   ([`SessionState::withheld_routes`]);
/// - is not one a dead end announced this session says not to run ([`interlock::avoids`]).
///
/// At the turn's first step (`index` 0, before the model's first response) only one thing: a run
/// reflex this prompt directed with every slot bound from its words ([`SessionState::at_prompt`]),
/// under the same grant, verdict, cap, once-per-route and interlock as any step; never a sequence,
/// a skill, a chain or a finish line there, never at that step's end, and nothing routed. Its
/// directive still went with the prompt, so the model reads the call's result knowing which reflex
/// it came from.
///
/// Never right after a predicted step (a model step always separates two: before request
/// `index`, a prediction at `index - 1`; at a turn's end, which the mod asks with the index of the
/// request the model just answered, a prediction at that index) but for one case, a chain's
/// second step (the step after the prompt's own lookup is one):
/// the reads bound from the result of the lookup Adage made at the step just before
/// ([`crate::PendingReads::after_adage`]), never followed by a third ([`SessionState::chained`]);
/// and at most [`crate::Policy::predict_cap`] per session (each yielded response counts once,
/// whatever its calls): past the cap the step is recorded as `would_predict` only. The bounds
/// count in both arms; in the control arm nothing is predicted and `would_predict` says what
/// would have been.
///
/// The mod's phases ([`crate::Policy::step`]): a dry run whose step would be yielded answers
/// `predicted` and changes nothing (no bound counts it, no `predicted_at`); a verdict other than
/// `allow` records `would_predict` with `withheld` and the verdict, counts against no bound and
/// holds the route for the session; `allow`, and a step with no phase, commit as described above.
/// A verdict naming another route than the one found decides no prediction at all.
fn predict_step(
    ctx: &Ctx,
    routes: &[&IndexedRoute],
    st: &mut SessionState,
    d: &mut Decision,
    turn_id: &str,
    index: u32,
    ending: bool,
) {
    // The turn's first step: the prompt's own run reflexes only, and none at its end.
    let first = index == 0;
    if ctx.policy.predict_grants.is_empty() || (first && (ending || st.at_prompt.is_empty())) {
        return;
    }
    // The step just before this request was predicted: no model step between them. At the end of
    // a turn (`ending`) the model has just answered request `index` itself, so only a prediction
    // at this very index would be "in a row". Nothing comes before a turn's first step.
    let previous = if ending { Some(index) } else { index.checked_sub(1) };
    // In a row, only the second step of a chain may follow (plan C7): the reads bound from the
    // result of the lookup Adage made at the step just before, never at a turn's end and never as
    // a third step. Every other step needs a model step between it and the last prediction.
    let in_a_row = st.last_step_predicted.as_ref().is_some_and(|(t, i)| t == turn_id && Some(*i) == previous);
    if in_a_row && (ending || st.chained) {
        return;
    }
    let inhibits = told_inhibits(routes, st);
    let eligible = |id: &str, command: Option<&str>| -> Option<&IndexedRoute> {
        let ir = routes.iter().copied().find(|r| r.route.id == id)?;
        let granted = ctx.policy.predict_grants.iter().any(|(t, r)| t == &ir.track && r == id);
        let inhibited = inhibits.iter().any(|inhibit| {
            interlock::avoids(inhibit, &ir.route)
                || matches!((&inhibit.step, command), (Step::Avoid { avoid, .. }, Some(c)) if pattern::directs_avoided(avoid, c))
        });
        let offered = !st.predicted_routes.iter().any(|r| r == id) && !st.withheld_routes.iter().any(|r| r == id);
        (granted && !ir.exec_only() && !inhibited && offered).then_some(ir)
    };
    let bash = |command: &str| PredictedCall { tool: "Bash".into(), input: serde_json::json!({ "command": command }) };
    let mut found: Option<Prediction> = None;
    let at_first = |route: &str| !first || st.at_prompt.iter().any(|r| r == route);
    for p in st.pending.iter().filter(|p| !p.command.is_empty() && !in_a_row && at_first(&p.route)) {
        let skill = p.expect_key.strip_prefix("control Skill ");
        let Some(ir) = eligible(&p.route, skill.is_none().then_some(p.command.as_str())) else { continue };
        let (kind, call) = match (skill, &ir.route.step) {
            // The prompt's own step: one fixed command, nothing else (`at_prompt` holds only
            // runs; the route may have been relearned since).
            (None, Step::Run { .. }) if first => (PredictionKind::Run, bash(&p.command)),
            _ if first => continue,
            (Some(_), Step::Skill { skill, args }) => match skill_call(ctx, skill, args.as_deref()) {
                Some(call) => (PredictionKind::Skill, call),
                None => continue,
            },
            // A skill's key on a route that is not a skill step: nothing to invoke.
            (Some(_), _) => continue,
            (None, Step::Sequence { .. }) => (PredictionKind::Sequence, bash(&p.command)),
            (None, _) => (PredictionKind::Run, bash(&p.command)),
        };
        found = Some(Prediction { kind, route: ir.route.id.clone(), track: ir.track.clone(), calls: vec![call] });
        break;
    }
    if found.is_none() && !first {
        for r in st.reads.iter().filter(|r| !r.files.is_empty() && (r.after_adage || !in_a_row)) {
            let Some(ir) = eligible(&r.route, None) else { continue };
            let calls = r
                .files
                .iter()
                .map(|f| PredictedCall {
                    tool: "Read".into(),
                    input: serde_json::json!({ "file_path": ctx.root.join(f).to_string_lossy() }),
                })
                .collect();
            found = Some(Prediction {
                kind: PredictionKind::Chain,
                route: ir.route.id.clone(),
                track: ir.track.clone(),
                calls,
            });
            break;
        }
    }
    if found.is_none() && ending {
        for o in st.obligations.iter().filter(|o| !o.met && !o.command.is_empty()) {
            let Some(ir) = eligible(&o.route, Some(&o.command)) else { continue };
            found = Some(Prediction {
                kind: PredictionKind::Obligation,
                route: ir.route.id.clone(),
                track: ir.track.clone(),
                calls: vec![bash(&o.command)],
            });
            break;
        }
    }
    let Some(p) = found else { return };
    if let StepPhase::Verdict { route: Some(r), .. } = &ctx.policy.step {
        if r != &p.route {
            return;
        }
    }
    d.would_predict = Some(p.clone());
    if st.predictions >= ctx.policy.predict_cap {
        return;
    }
    let yields = st.arm != Some(Arm::Control) && !ctx.policy.observe;
    match &ctx.policy.step {
        // Only a step that would be yielded waits for the person's permissions; the rest commit now.
        StepPhase::DryRun if yields => {
            d.predicted = Some(p);
            return;
        }
        StepPhase::Verdict { permission, .. } if permission.decision != Permission::Allow => {
            d.withheld = Some(Withheld::Permission);
            d.permission = Some(permission.clone());
            if !st.withheld_routes.contains(&p.route) {
                st.withheld_routes.push(p.route);
            }
            return;
        }
        _ => {}
    }
    st.predictions += 1;
    st.last_step_predicted = Some((turn_id.to_string(), index));
    st.chained = in_a_row;
    st.predicted_routes.push(p.route.clone());
    d.predicted_at = Some((turn_id.to_string(), index));
    if yields {
        d.predicted = Some(p);
    }
}

/// The Skill tool call that invokes `skill` (mod architecture §3.11): only a skill the policy lets a
/// step invoke ([`crate::Policy::invoke_skills`]: inert, accepted, `skills.invoke` on), with its
/// arguments when the step has them.
fn skill_call(ctx: &Ctx, skill: &str, args: Option<&str>) -> Option<PredictedCall> {
    if !adage_tracks::safe_skill_name(skill) || !ctx.policy.invoke_skills.iter().any(|s| s == skill) {
        return None;
    }
    let mut input = serde_json::json!({ "skill": skill });
    if let Some(a) = args {
        input["args"] = serde_json::json!(a);
    }
    Some(PredictedCall { tool: "Skill".into(), input })
}

/// The dead ends the agent was told this session ([`SessionState::told`]): nothing they say not to
/// run is predicted or routed.
pub(crate) fn told_inhibits<'a>(routes: &[&'a IndexedRoute], st: &SessionState) -> Vec<&'a Route> {
    st.told()
        .filter_map(|id| routes.iter().find(|r| &r.route.id == id))
        .map(|r| &r.route)
        .filter(|r| matches!(r.step, Step::Avoid { .. }))
        .collect()
}

/// A `TurnStep` the model will take ([`crate::Policy::routine`] set, nothing predicted): routine
/// when a delivered directive at full strength is pending, in play, not avoided by a dead end the
/// agent was told, and holding no `predict` grant (with one, Adage makes the call instead, or a
/// bound held it back), so the model is asked to do what the directive already says (the Claude
/// Code mod's architecture §3.5). The first such directive in delivery order is named. Recorded
/// as `would_route` in both arms; `routine` only in the treated arm outside observe mode.
fn route_step(ctx: &Ctx, routes: &[&IndexedRoute], st: &SessionState, d: &mut Decision) {
    if !ctx.policy.routine {
        return;
    }
    let inhibits = told_inhibits(routes, st);
    let found = st.pending.iter().find_map(|p| {
        let ir = routes.iter().copied().find(|r| r.route.id == p.route)?;
        let granted = ctx.policy.predict_grants.iter().any(|(t, r)| t == &ir.track && r == &p.route);
        let inhibited = inhibits.iter().any(|inhibit| {
            interlock::avoids(inhibit, &ir.route)
                || matches!(&inhibit.step, Step::Avoid { avoid, .. } if !p.command.is_empty() && pattern::directs_avoided(avoid, &p.command))
        });
        let fire = adage_tracks::delivered_strength(&ir.route) == Strength::Fire;
        (fire && !granted && !inhibited && !ir.exec_only())
            .then(|| Routing { route: ir.route.id.clone(), track: ir.track.clone() })
    });
    let Some(r) = found else { return };
    d.would_route = Some(r.clone());
    if st.arm != Some(Arm::Control) && !ctx.policy.observe {
        d.routine = Some(r);
    }
}
