//! Decisions the Claude Code mod asks for at `PreToolUse`, outside [`crate::decide`]: a Bash call
//! answered by a fresh receipt instead of running (observed as made), and a known dead end (a
//! learned one, or a declared instinct) replaced before it runs. The shell-command rewrite for the
//! project's environment is [`crate::pretool`].

use crate::observe::{call_keys, cap, exec_segments};
use crate::policy::{failure_matches, follows_call, follows_learned, in_play};
use crate::state::push_history_capped;
use crate::{command_key, render, Arm, By, Call, Ctx, Decision, Judgement, Policy, SessionState, Substitution};
use adage_core::{keys, pattern, CommandTable};
use adage_tracks::{Index, RouteState, Step, Strength, Trigger};

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

/// The replacement for a Bash call about to run, if it is a known dead end here: the first route
/// in index order, in play, with no slots, that is
///
/// - an `avoid` step whose runners the replaced command runs ([`pattern::directs_avoided`]), or
/// - a run step after a failure of the replaced command's key (the after-failure trigger's own
///   match),
///
/// whose `instead` is a fixed command that is not itself avoided, not the call, and not the
/// call's own tool in another environment ([`same_tool`]: the interpreter rewrite's to make), and
/// that is either
///
/// - **learned** (or imported, or authored): firm, and a run after a failure also delivered at
///   fire; a forming, dormant or nascent route is never a replacement, or
/// - **an instinct** ([`adage_tracks::Route::is_instinct`]): in service, forming or firm. An
///   instinct is bound forming and kept forming (approval does not firm it) and is always prime,
///   so neither `firm` nor "at fire" can apply to it: its standing is that the repository's own
///   files declare `instead` (the declared fact it was bound on), and the engine's verdict on
///   both commands is the guard. A nascent instinct (an environment step not yet approved), a
///   dormant or an extinguished one is never a replacement.
///
/// What is replaced is one plain command, or the first stage of a pipeline whose other stages
/// only filter its output ([`OUTPUT_FILTERS`]; `make test 2>&1 | tail -5` for
/// `python3 -m unittest 2>&1 | tail -5`): the filters and the stage's own `2>&1` are kept
/// verbatim, and the [`Substitution`]'s `command` is the whole new pipeline. A list, any other
/// redirection, a subshell, an expansion, or a later stage that does more than read its input is
/// never rewritten: it also runs what the replacement would drop, or changes what the
/// replacement's output feeds.
///
/// The context line says why the call was replaced: for a learned route, that the command fails
/// here, quoting the failure learning found; for an instinct, which file of the repository
/// declares `instead` and the instinct's reason ([`instinct_reason`]).
pub fn substitution(
    index: &Index,
    repo: &str,
    policy: &Policy,
    table: &CommandTable,
    command: &str,
) -> Option<Substitution> {
    let stage = replaced_stage(command)?;
    let (before, dead, after) = (&command[..stage.start], &command[stage.clone()], &command[stage.end..]);
    if keys::flatten(&adage_core::shell::normalize_shell(dead, "/", table)).len() != 1 {
        return None;
    }
    let key = command_key(dead, table);
    if key.is_empty() {
        return None;
    }
    index.for_repo(repo).filter(|r| in_play(policy, r)).find_map(|ir| {
        let r = &ir.route;
        let instinct = r.is_instinct();
        let in_service = if instinct {
            matches!(r.state, RouteState::Forming | RouteState::Firm)
        } else {
            r.state == RouteState::Firm
        };
        if !in_service || !r.slots.is_empty() {
            return None;
        }
        let (instead, signature) = match (&r.trigger, &r.step) {
            (_, Step::Avoid { avoid, instead, signature })
                if pattern::directs_avoided(avoid, dead) && !pattern::directs_avoided(avoid, instead) =>
            {
                (instead, signature)
            }
            (Trigger::AfterFailure { command: failing, signature }, Step::Run { command: instead })
                if (instinct || adage_tracks::delivered_strength(r) == Strength::Fire)
                    && failure_matches(policy, r, failing, &key) =>
            {
                (instead, signature)
            }
            _ => return None,
        };
        let instead = instead.trim();
        if instead.is_empty() || instead.contains('{') || pattern::same_command(instead, dead) || same_tool(instead, dead) {
            return None;
        }
        // The line names what ran, what it replaced, and which command fails, so it reads the
        // same whatever output follows it: live, "in place of …: it fails" over output cut by a
        // filter was read as the replacement failing. A pipeline's filters ran as written and are
        // not quoted (sanitising would mangle `2>&1`).
        let through = if before.is_empty() && after.is_empty() { "" } else { ", through the rest of your pipeline as written" };
        let instead_shown = render::sanitise(instead);
        let because = if instinct {
            instinct_reason(r, &instead_shown)
        } else {
            format!("that command fails in this repository (\"{}\")", render::sanitise(signature))
        };
        let context = format!(
            "adage ran {instead_shown} for you{through}; {} was replaced because {because}. The output above is from {instead_shown}.",
            render::sanitise(dead),
        );
        let ran = format!("{before}{instead}{after}");
        Some(Substitution { route: r.id.clone(), track: ir.track.clone(), command: ran, context })
    })
}

/// Why an instinct replaced a call, in the repository's terms: the file that declares `instead`
/// (`Makefile`), when the instinct names one, and the instinct's own reason, its summary without
/// the `Instinct U1: ` label (the label is for people reading `adage routes`, not the agent). An
/// instinct claims no failure seen here, so the line quotes none.
fn instinct_reason(r: &adage_tracks::Route, instead_shown: &str) -> String {
    let declarer = match r.origin() {
        adage_tracks::Origin::Instinct { declared: Some(d), .. } if render::declared_ok(&d) => {
            format!("this repository's {}", render::sanitise(&d))
        }
        _ => "this repository's build".to_string(),
    };
    let summary = r.summary.trim();
    let reason = match summary.split_once(": ") {
        Some((label, rest)) if label.starts_with("Instinct ") && !rest.trim().is_empty() => rest.trim(),
        _ => summary,
    };
    if reason.is_empty() {
        format!("{declarer} declares {instead_shown} in its place")
    } else {
        format!("{declarer} declares {instead_shown} in its place ({})", render::sanitise(reason))
    }
}

/// Whether `instead` runs the same tool as the call, only in another environment
/// (`.venv/bin/python -m pytest` or `uv run pytest` for `python -m pytest tests -q`): replacing
/// the call would drop its arguments (which tests), and the interpreter rewrite
/// ([`crate::pretool`]) is what points that run at the project's environment with them kept.
fn same_tool(instead: &str, dead: &str) -> bool {
    let argv = |c: &str| c.split_whitespace().map(str::to_string).collect::<Vec<_>>();
    let (i, d) = (adage_core::action::classify_argv(&argv(instead)), adage_core::action::classify_argv(&argv(dead)));
    !i.runner.is_empty() && i.runner == d.runner
}

/// The commands a pipeline may pass a replaced stage's output through: each only reads its input
/// and prints some of it (`tee` also keeps a copy in the file it names), so the pipeline does with
/// the replacement's output what it would have done with the dead end's.
pub const OUTPUT_FILTERS: &[&str] = &["tail", "head", "grep", "wc", "cat", "tee", "sort", "uniq"];

/// The part of `command` a substitution may replace: the whole of a plain command (no shell
/// syntax at all), or the first stage of a plain pipeline ([`adage_core::shell::plain_pipeline`])
/// whose later stages are all [`OUTPUT_FILTERS`] without a redirection of their own.
fn replaced_stage(command: &str) -> Option<std::ops::Range<usize>> {
    if !command.contains(|c: char| "|;&<>$`()\n".contains(c)) {
        return Some(0..command.len());
    }
    let stages = adage_core::shell::plain_pipeline(command)?;
    let (first, filters) = stages.split_first()?;
    let only_filters =
        !filters.is_empty() && filters.iter().all(|f| !f.merges_stderr && OUTPUT_FILTERS.contains(&f.argv[0].as_str()));
    only_filters.then(|| first.range.clone())
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
