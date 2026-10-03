//! Decisions the Claude Code mod asks for at `PreToolUse`, outside [`crate::decide`]: a Bash call
//! answered by a fresh receipt instead of running (observed as made), and a learned dead end
//! replaced before it runs. The shell-command rewrite for the project's environment is
//! [`crate::pretool`].

use crate::observe::{call_keys, cap, exec_segments};
use crate::policy::{failure_matches, follows_call, follows_learned, in_play};
use crate::state::push_history_capped;
use crate::{command_key, render, Arm, By, Call, Ctx, Decision, Judgement, Policy, SessionState, Substitution};
use adage_core::{keys, pattern, CommandTable};
use adage_tracks::{Index, Step, Strength, Trigger};

/// A Bash call the mod answered with a fresh receipt instead of running it (the Claude Code mod's
/// architecture §3.3; `PreToolUse` with `verdict: {answered: "receipt"}`): the run the receipt
/// records passed on files identical to the current ones and covered every test the call
/// selects, so the call is observed as made and passed. A pending directive it follows is judged
/// followed (the agent asked for the run), and a finish-line obligation that either the call or
/// the covering run (`covering`, the command that ran) meets is met: the Stop gate does not ask
/// for a run whose result is already in hand.
pub fn answered_by_receipt(ctx: &Ctx, st: &mut SessionState, command: &str, covering: &str) -> Decision {
    let mut d = Decision::default();
    let bash = |command: &str| Call {
        tool: "Bash".into(),
        input: serde_json::json!({ "command": command }),
        output: String::new(),
        failed: false,
        by: By::Agent,
        tool_use_id: None,
        agent: None,
    };
    let call = bash(command);
    let keyed = call_keys(&call, ctx);
    let segs = exec_segments(&keyed);
    let mut meeting: Vec<String> = Vec::new();
    for (key, _) in keyed {
        let key = cap(key);
        d.observed.push(key.clone());
        push_history_capped(st, key.clone(), ctx.params.history);
        st.pending.retain(|p| {
            let follows = ctx.policy.follows(&p.expect_key, &key)
                || follows_call(&ctx.policy, &p.expect_key, &segs)
                || follows_learned(&ctx.policy, &p.expect_key, &call);
            if follows {
                d.judgements.push(Judgement::Followed { route: p.route.clone() });
            }
            !follows
        });
        meeting.push(key);
    }
    if !covering.is_empty() {
        meeting.extend(call_keys(&bash(covering), ctx).into_iter().map(|(k, _)| cap(k)));
    }
    for o in st.obligations.iter_mut().filter(|o| !o.met) {
        if meeting.iter().any(|k| ctx.policy.meets(&o.expect_key, k)) {
            o.met = true;
            d.judgements.push(Judgement::ObligationMet { route: o.route.clone() });
        }
    }
    d
}

/// The learned replacement for a Bash call about to run, if it is a known dead end here: the
/// first route in index order, in play, learned (an instinct is advice, never a replacement),
/// firm, with no slots, that is
///
/// - an `avoid` step whose runners `command` runs ([`pattern::directs_avoided`]), or
/// - a run step after a failure of `command`'s key (the after-failure trigger's own match)
///   delivered at fire,
///
/// and whose `instead` is a fixed command that is not itself avoided and not the call. Only one
/// plain command is replaced: a list or a pipeline also runs what the replacement would drop.
pub fn substitution(
    index: &Index,
    repo: &str,
    policy: &Policy,
    table: &CommandTable,
    command: &str,
) -> Option<Substitution> {
    let plain = !command.contains(|c: char| "|;&<>$`()\n".contains(c));
    if !plain || keys::flatten(&adage_core::shell::normalize_shell(command, "/", table)).len() != 1 {
        return None;
    }
    let key = command_key(command, table);
    if key.is_empty() {
        return None;
    }
    index.for_repo(repo).filter(|r| in_play(policy, r)).find_map(|ir| {
        let r = &ir.route;
        if r.is_instinct() || r.state != adage_tracks::RouteState::Firm || !r.slots.is_empty() {
            return None;
        }
        let (instead, signature) = match (&r.trigger, &r.step) {
            (_, Step::Avoid { avoid, instead, signature })
                if pattern::directs_avoided(avoid, command) && !pattern::directs_avoided(avoid, instead) =>
            {
                (instead, signature)
            }
            (Trigger::AfterFailure { command: failing, signature }, Step::Run { command: instead })
                if adage_tracks::delivered_strength(r) == Strength::Fire
                    && failure_matches(policy, r, failing, &key) =>
            {
                (instead, signature)
            }
            _ => return None,
        };
        let instead = instead.trim();
        if instead.is_empty() || instead.contains('{') || pattern::same_command(instead, command) {
            return None;
        }
        let context = format!(
            "adage ran {} in place of {}: in this repository it fails (\"{}\").",
            render::sanitise(instead),
            render::sanitise(command),
            render::sanitise(signature)
        );
        Some(Substitution { route: r.id.clone(), track: ir.track.clone(), command: instead.to_string(), context })
    })
}

/// The decision a substitution records once the mod found the person's rules treat both commands
/// alike: in the treated arm it is made and judged [`Judgement::Substituted`]; in the control arm,
/// and in observe mode ([`Policy::observe`]), it is only `would_substitute`.
pub fn substitute(arm: Option<Arm>, observe: bool, sub: Substitution) -> Decision {
    let mut d = Decision { would_substitute: Some(sub.clone()), ..Decision::default() };
    if arm != Some(Arm::Control) && !observe {
        d.judgements.push(Judgement::Substituted { route: sub.route.clone() });
        d.substituted = Some(sub);
    }
    d
}
