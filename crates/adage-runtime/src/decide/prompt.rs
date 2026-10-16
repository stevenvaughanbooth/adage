//! The `Prompt` arm of [`crate::decide`]: the dead ends to announce chosen first, then prompt
//! routes, procedure cards and intent routes, context triggers that end in a prompt item, and the
//! first prompt's announcements.

use super::{emit_ok, fire_contexts, Walk};
use crate::binding::bind_prompt_route;
use crate::decision::directive;
use crate::{orient, render, Ctx, Directive, Pending, SessionState};
use adage_core::spans;
use adage_tracks::{interlock, IndexedRoute, Step, Strength, Trigger};

pub(super) fn on_prompt(ctx: &Ctx, w: &Walk, st: &mut SessionState, out: &mut Vec<Directive>, text: &str) {
    let routes = w.routes;
    st.prompts += 1;
    // A card waiting for an edit belongs to the prompt that came close to it: a new prompt
    // is a new task, so it waits no longer (it waits again below if this prompt is close).
    st.card_waiting = None;
    let spans = spans::extract(text);
    // 0. Inhibit first (Design §10.6): the dead ends to announce at the first prompt are chosen
    //    before anything else, so the caps never crowd a warning out, and nothing a
    //    warning contradicts is chosen beside it. They are still emitted last, as before.
    // Routes the standing text already carries (the mod's cached section) are not said
    // again.
    let standing: Vec<&str> = st.standing.iter().flat_map(|s| s.routes.iter().map(String::as_str)).collect();
    let announce: Vec<&IndexedRoute> = if st.prompts == 1 {
        routes
            .iter()
            .copied()
            .filter(|r| matches!(r.route.trigger, Trigger::SessionStart))
            .filter(|r| !standing.contains(&r.route.id.as_str()))
            .collect()
    } else {
        Vec::new()
    };
    let (per_session, per_decision) = (ctx.params.max_directives_per_session, ctx.params.max_directives_per_decision);
    let room = per_decision.min(per_session.saturating_sub(st.emitted.len()));
    let inhibits: Vec<&IndexedRoute> = announce
        .iter()
        .copied()
        .filter(|r| r.route.strength == Strength::Inhibit && !st.emitted.iter().any(|e| e == &r.route.id))
        .filter(|r| render::route_text(&r.route, &[]).is_some())
        .take(room)
        .collect();
    let mut inhibits_left = inhibits.len();
    let ok = |st: &SessionState, out: &Vec<Directive>, ir: &IndexedRoute, reserved: usize| {
        emit_ok(ctx, st, out, &ir.route.id)
            && st.emitted.len() + out.len() + reserved < per_session
            && out.len() + reserved < per_decision
            && !inhibits.iter().any(|inhibit| interlock::avoids(&inhibit.route, &ir.route))
    };
    // 1. Prompt-triggered routes, strongest strength first.
    let mut prompt_routes: Vec<&&IndexedRoute> =
        routes.iter().filter(|r| matches!(r.route.trigger, Trigger::Prompt { .. })).collect();
    prompt_routes.sort_by_key(|r| (adage_tracks::delivered_strength(&r.route), -(r.route.evidence.support as i64)));
    for ir in prompt_routes {
        if !ok(st, out, ir, inhibits_left) {
            continue;
        }
        if let Some((text, key, head, command)) = bind_prompt_route(&ir.route, &spans, text, ctx) {
            at_prompt(st, ir, &command);
            st.pending.push(Pending {
                route: ir.route.id.clone(),
                expect_key: key,
                expect_head: head,
                batches_left: ctx.params.follow_window,
                command,
            });
            out.push(directive(ir, text));
        }
    }
    // 2. Intent routes: the prompt means what a family means. The model is consulted only
    //    if this repository has such routes (or intent context items); no model, no
    //    match (fail open). It embeds the prompt once.
    let mut emb: Option<Option<(String, Vec<f32>)>> = None;
    // A family's procedure card is matched on its own (it does not compete with a run),
    // said once, and directs no command of its own.
    let (cards, runs): (Vec<&IndexedRoute>, Vec<&IndexedRoute>) =
        routes.iter().copied().partition(|r| matches!(r.route.step, Step::Card { .. }));
    match match_card(ctx, &cards, text, &mut emb) {
        Some((ir, true)) => {
            if ok(st, out, ir, inhibits_left) {
                if let Some(mut t) = card_text(ctx, &ir.route) {
                    // Execution-only verifications are out of play as advice; the card may still
                    // name their command as a fact about the repository.
                    let all: Vec<&IndexedRoute> = ctx.index.for_repo(&ctx.repo).collect();
                    let checks = certified_checks(&all);
                    if let Some(line) = ctx.policy.card_checks.then(|| render::card_checks_line(&checks)).flatten() {
                        t.push('\n');
                        t.push_str(&line);
                    }
                    out.push(directive(ir, t));
                }
            }
        }
        Some((ir, false)) if emit_ok(ctx, st, out, &ir.route.id) => st.card_waiting = Some(ir.route.id.clone()),
        _ => {}
    }
    if let Some((family, ir)) = match_intent(ctx, &runs, text, &mut emb) {
        st.intent = Some(family);
        if ok(st, out, ir, inhibits_left) {
            if let Some((text, key, head, command)) = bind_prompt_route(&ir.route, &spans, text, ctx) {
                at_prompt(st, ir, &command);
                st.pending.push(Pending {
                    route: ir.route.id.clone(),
                    expect_key: key,
                    expect_head: head,
                    batches_left: ctx.params.follow_window,
                    command,
                });
                out.push(directive(ir, text));
            }
        }
    }
    // 2b. Context triggers ending in a prompt item (a learned value type, an intent).
    if let Some(sy) = w.syms.filter(|sy| sy.has_prompt_items()) {
        let got = sy.prompt(text, || embedding(ctx, text, &mut emb));
        if st.ctx.push(w.net, &got) {
            fire_contexts(ctx, routes, w.net, st, out, |st, out, ir| ok(st, out, ir, inhibits_left));
        }
    }
    // 3. Known dead ends, once, at the first prompt: the inhibits chosen above, and any other
    //    session-start route there is room for.
    for ir in announce {
        let inhibit = inhibits.iter().any(|r| r.route.id == ir.route.id);
        if inhibit {
            inhibits_left -= 1;
        }
        if inhibit || ok(st, out, ir, inhibits_left) {
            if let Some(t) = render::route_text(&ir.route, &[]) {
                out.push(directive(ir, t));
            }
        }
    }
}

/// Keeps a run reflex this prompt directed for the turn's first step ([`SessionState::at_prompt`]):
/// a single fixed command whose slots the prompt's words bound (`command` is empty when one was
/// left unbound). A sequence, a skill or anything else waits for a model step, as before.
fn at_prompt(st: &mut SessionState, ir: &IndexedRoute, command: &str) {
    if matches!(ir.route.step, Step::Run { .. }) && !command.is_empty() && !command.contains('{') {
        st.at_prompt.push(ir.route.id.clone());
    }
}

/// A card route's text: oriented ([`crate::Policy::card_orientation`]) or as before; any other
/// route's text.
pub(super) fn card_text(ctx: &Ctx<'_>, route: &adage_tracks::Route) -> Option<String> {
    match &route.step {
        Step::Card { card } if ctx.policy.card_orientation => {
            render::oriented_card_text(route, orient::block(ctx.checkout, ctx.root, card).as_deref())
        }
        _ => render::route_text(route, &[]),
    }
}

/// The commands of this repository's learned verifications with execution evidence, most
/// supported first: what a card may name as the repository's test command.
fn certified_checks<'a>(routes: &[&'a IndexedRoute]) -> Vec<&'a str> {
    let mut v: Vec<(&'a str, u32)> = routes
        .iter()
        .filter(|r| r.route.effect == adage_core::Effect::Verify && r.route.evidence.execution.is_some())
        .filter_map(|r| match &r.route.step {
            Step::Run { command } if !command.contains('{') => Some((command.as_str(), r.route.evidence.support)),
            _ => None,
        })
        .collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    v.dedup_by(|a, b| a.0 == b.0);
    v.into_iter().map(|(c, _)| c).collect()
}

/// The most similar intent family whose threshold the prompt clears, if any. Ties keep the first
/// route in index order.
fn match_intent<'a>(
    ctx: &Ctx,
    routes: &[&'a IndexedRoute],
    text: &str,
    emb: &mut Option<Option<(String, Vec<f32>)>>,
) -> Option<(String, &'a IndexedRoute)> {
    let intents: Vec<&'a IndexedRoute> =
        routes.iter().copied().filter(|r| matches!(r.route.trigger, Trigger::Intent { .. })).collect();
    if intents.is_empty() {
        return None;
    }
    let (model_id, v) = embedding(ctx, text, emb)?;
    let mut best: Option<(f32, &'a String, &'a IndexedRoute)> = None;
    for ir in intents {
        let Trigger::Intent { family, model, centroid, threshold } = &ir.route.trigger else { continue };
        if *model != model_id || centroid.len() != v.len() {
            continue;
        }
        let s = adage_intent::cosine(&v, &adage_intent::dequantize(centroid));
        if s >= *threshold && best.is_none_or(|b| s > b.0) {
            best = Some((s, family, ir));
        }
    }
    best.map(|(_, f, ir)| (f.clone(), ir))
}

/// The procedure card whose family the prompt is closest to: its best cosine to the family's
/// prompts (the trigger's centroid when the card keeps none). `(card, true)` at or above the
/// threshold; `(card, false)` within the grey margin below it, when the card waits for an edit of
/// one of its files; nothing otherwise.
fn match_card<'a>(
    ctx: &Ctx,
    cards: &[&'a IndexedRoute],
    text: &str,
    emb: &mut Option<Option<(String, Vec<f32>)>>,
) -> Option<(&'a IndexedRoute, bool)> {
    if cards.is_empty() {
        return None;
    }
    let (model_id, v) = embedding(ctx, text, emb)?;
    let mut best: Option<(f32, f32, &'a IndexedRoute)> = None;
    for ir in cards.iter().copied() {
        let (Trigger::Intent { model, centroid, threshold, .. }, Step::Card { card }) =
            (&ir.route.trigger, &ir.route.step)
        else {
            continue;
        };
        if *model != model_id || centroid.len() != v.len() {
            continue;
        }
        let score = if card.prompts.is_empty() {
            adage_intent::cosine(&v, &adage_intent::dequantize(centroid))
        } else {
            card.prompts
                .iter()
                .filter(|p| p.len() == v.len())
                .map(|p| adage_intent::cosine(&v, &adage_intent::dequantize(p)))
                .fold(f32::NEG_INFINITY, f32::max)
        };
        if best.is_none_or(|b| score - *threshold > b.0 - b.1) {
            best = Some((score, *threshold, ir));
        }
    }
    let (score, threshold, ir) = best?;
    if score >= threshold {
        Some((ir, true))
    } else if score >= threshold - adage_tracks::card::GREY_MARGIN {
        Some((ir, false))
    } else {
        None
    }
}

/// The prompt's embedding, computed at most once per decision (`cache`).
fn embedding(ctx: &Ctx, text: &str, cache: &mut Option<Option<(String, Vec<f32>)>>) -> Option<(String, Vec<f32>)> {
    cache.get_or_insert_with(|| ctx.embed.and_then(|e| e(text))).clone()
}
