//! Hook-time decisions (Design §9, §13; Architecture §15).
//!
//! Pure logic: given the compiled index, the session's state, and one canonical event, decide
//! which directives to emit. No I/O except checking that bound repository paths exist. The CLI
//! owns reading payloads, locking state, writing spools, and printing harness output.

pub mod fast;
pub mod rebuild;
pub mod render;

use adage_core::keys::{self, template_key};
use adage_core::pattern;
use adage_core::spans::{self, SpanKind};
use adage_core::tools::normalize_tool;
use adage_core::{repo, CommandTable};
use adage_tracks::{interlock, Aspect, Contract, Index, IndexedRoute, Route, SlotSource, Step, Trigger};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;

// The defaults of `Ctx::params` (`adage_tracks::params::RuntimeParams`), which is what decides.
pub use adage_tracks::params::RuntimeParams;
pub const MAX_DIRECTIVES_PER_SESSION: usize = adage_tracks::params::MAX_DIRECTIVES_PER_SESSION;
pub const MAX_DIRECTIVES_PER_DECISION: usize = adage_tracks::params::MAX_DIRECTIVES_PER_DECISION;
const HISTORY: usize = adage_tracks::params::HISTORY;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Arm {
    #[default]
    Treated,
    Holdout,
}

/// Deterministic experiment assignment (Architecture A12): stable per session, reproducible.
pub fn assign_arm(session: &str, holdout_share: f64) -> Arm {
    let h = Sha256::digest(format!("adage-arm:{session}").as_bytes());
    let x = u32::from_le_bytes([h[0], h[1], h[2], h[3]]) as f64 / u32::MAX as f64;
    if x < holdout_share {
        Arm::Holdout
    } else {
        Arm::Treated
    }
}

/// The arm a session gets, and its **propensity**: the probability that assignment gave it that
/// arm, recorded in every spool record for inverse-propensity weighting later (Design §11, §14.1).
/// With the holdout share `h` (clamped to 0..=1): `h` for the holdout arm, `1 - h` for the
/// treated arm. An arm forced with `ADAGE_ARM` (`forced` = `treated` or `holdout`) was not drawn:
/// its propensity is 1. Any other `forced` value is ignored.
pub fn assign(session: &str, holdout_share: f64, forced: Option<&str>) -> (Arm, f64) {
    match forced {
        Some("holdout") => return (Arm::Holdout, 1.0),
        Some("treated") => return (Arm::Treated, 1.0),
        _ => {}
    }
    let h = if holdout_share.is_nan() { 0.0 } else { holdout_share.clamp(0.0, 1.0) };
    match assign_arm(session, holdout_share) {
        Arm::Holdout => (Arm::Holdout, h),
        Arm::Treated => (Arm::Treated, 1.0 - h),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Call {
    pub tool: String,
    pub input: Value,
    pub output: String,
    pub failed: bool,
}

#[derive(Debug, Clone)]
pub enum Event {
    SessionStart,
    Prompt { text: String },
    Batch { calls: Vec<Call> },
    Failure { call: Call },
    Stop { stop_hook_active: bool },
    SessionEnd,
}

impl Event {
    pub fn name(&self) -> &'static str {
        match self {
            Event::SessionStart => "session_start",
            Event::Prompt { .. } => "prompt",
            Event::Batch { .. } => "batch",
            Event::Failure { .. } => "failure",
            Event::Stop { .. } => "stop",
            Event::SessionEnd => "session_end",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Pending {
    pub route: String,
    pub expect_key: String,
    /// The directed command's identity ([`pattern`]), for reading the state; following is judged
    /// on `expect_key` with [`pattern::meets`] (added options and an interpreter prefix count).
    #[serde(default)]
    pub expect_head: String,
    pub batches_left: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Obligation {
    pub route: String,
    pub expect_key: String,
    pub command: String,
    pub paths: String,
    pub met: bool,
    pub stop_requested: bool,
    /// The repository-relative path whose edit last armed it (what an E2 action reports).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub edited: String,
    /// An edit in the latest tool batch matched it (cleared at the next batch): an E2 action
    /// may carry it out now.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fresh: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionState {
    pub arm: Option<Arm>,
    pub repo: Option<String>,
    pub prompts: u32,
    pub batches: u32,
    pub emitted: Vec<String>,
    pub pending: Vec<Pending>,
    pub obligations: Vec<Obligation>,
    pub history: Vec<String>,
    pub first_edit_at_batch: Option<u32>,
    /// Stop-hook requests made this session (bounded).
    #[serde(default)]
    pub stop_requests: u32,
    /// The intent family the latest prompt matched, carried through the session (Design §8.2.1).
    #[serde(default)]
    pub intent: Option<String>,
    /// The probability of `arm` at assignment ([`assign`]); `None` in a state written before it
    /// was recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub propensity: Option<f64>,
}

/// At most this many stop-hook requests per session, across all obligations (the default of
/// `RuntimeParams::max_stop_requests`).
pub const MAX_STOP_REQUESTS: u32 = adage_tracks::params::MAX_STOP_REQUESTS;
const MAX_KEY: usize = 256;

fn cap(mut s: String) -> String {
    if s.len() > MAX_KEY {
        let mut i = MAX_KEY;
        while !s.is_char_boundary(i) {
            i -= 1;
        }
        s.truncate(i);
    }
    s
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Directive {
    pub route: String,
    pub track: String,
    pub version: String,
    pub aspect: Aspect,
    pub text: String,
    /// The route's kind ([`adage_tracks::route_kind`]), rendered into the directive's wrapper so
    /// that learning can tell which miners a session's directives could influence. Empty in
    /// records written before it existed.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
    /// Where the route came from, when it is not learned (an instinct): its wrapper says so
    /// instead of claiming session evidence ([`render::footer`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<adage_tracks::Origin>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Judgement {
    Followed {
        route: String,
    },
    Derailed {
        route: String,
    },
    ObligationMet {
        route: String,
    },
    /// The agent ran the directed command within the follow window, and it failed. Three of
    /// these (or a derail rate whose Wilson lower bound reaches 0.5) retire an instinct route at
    /// the next learn (`adage_learn::instincts`).
    FollowedFailed {
        route: String,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Decision {
    /// What the agent receives (empty in the holdout arm).
    pub directives: Vec<Directive>,
    /// A stop-hook request to continue, with its reason.
    pub continue_reason: Option<String>,
    /// What would have been emitted — recorded in both arms for exposure logging.
    pub would_have: Vec<Directive>,
    /// A stop-hook request that would have been made (recorded in both arms).
    #[serde(default)]
    pub would_continue: Option<String>,
    pub judgements: Vec<Judgement>,
    /// Canonical template keys of the calls observed in this event.
    pub observed: Vec<String>,
}

/// Embeds a raw prompt: `(model id, unit vector)`, or `None` when no model is available. Called
/// only at a prompt decision point, and only when the repository has intent routes.
pub type Embed<'a> = &'a dyn Fn(&str) -> Option<(String, Vec<f32>)>;

pub struct Ctx<'a> {
    pub index: &'a Index,
    pub repo: String,
    pub root: &'a Path,
    pub table: &'a CommandTable,
    pub embed: Option<Embed<'a>>,
    /// Limits: the follow window, directives per session and per decision, stop requests,
    /// history. The hook always uses [`RuntimeParams::default`]; the offline eval can tune them.
    pub params: RuntimeParams,
}

thread_local! {
    static EXACT_KEYS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static EXEC_ONLY: std::cell::RefCell<Vec<(String, String)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Run `f` deciding with these exec-only routes (`(track, route id)`: reflexes that run a vendored
/// script) in play. Only `adage-exec hook` does, for the ones approved on this machine, because it
/// carries them out itself (Track C §1.7); everywhere else they are never directed. Scoped to
/// this thread.
pub fn with_exec_only<R>(allowed: Vec<(String, String)>, f: impl FnOnce() -> R) -> R {
    struct Reset(Vec<(String, String)>);
    impl Drop for Reset {
        fn drop(&mut self) {
            EXEC_ONLY.with(|e| *e.borrow_mut() = std::mem::take(&mut self.0));
        }
    }
    let _reset = Reset(EXEC_ONLY.with(|e| std::mem::replace(&mut *e.borrow_mut(), allowed)));
    f()
}

/// Whether an index entry may be directed here: not exec-only, or an exec-only route in play.
fn in_play(r: &IndexedRoute) -> bool {
    !r.exec_only() || EXEC_ONLY.with(|e| e.borrow().iter().any(|(t, id)| t == &r.track && id == &r.route.id))
}

/// Compatibility: run `f` deciding with the matching the hooks used before the command-pattern
/// algebra, where a trigger, an obligation or a follow judgement needed the observed key to equal
/// the directed one exactly. Only for replaying decisions that hooks of that time recorded (the
/// eval's fidelity check, `adage_eval::fidelity`); nothing else may use it. Scoped to this thread.
pub fn with_exact_key_matching<R>(f: impl FnOnce() -> R) -> R {
    struct Reset(bool);
    impl Drop for Reset {
        fn drop(&mut self) {
            EXACT_KEYS.set(self.0);
        }
    }
    let _reset = Reset(EXACT_KEYS.replace(true));
    f()
}

/// Whether an observed key meets a directed one ([`pattern::meets`]).
fn meets(directed: &str, observed: &str) -> bool {
    if EXACT_KEYS.get() {
        directed == observed
    } else {
        pattern::meets(directed, observed)
    }
}

/// The canonical template key of a command string, for matching the agent's calls.
pub fn command_key(command: &str, table: &CommandTable) -> String {
    let n = adage_core::shell::normalize_shell(command, "/", table);
    keys::flatten(&n)
        .first()
        .map(|x| {
            let mut x = x.clone();
            keys::canonicalise(&mut x, Path::new("/"));
            template_key(&x.action)
        })
        .unwrap_or_default()
}

fn call_keys(call: &Call, ctx: &Ctx) -> Vec<(String, adage_core::Normalized)> {
    let cwd = ctx.root.to_string_lossy();
    keys::flatten(&normalize_tool(&call.tool, &call.input, &cwd, ctx.table))
        .into_iter()
        .map(|mut n| {
            keys::canonicalise(&mut n, ctx.root);
            (template_key(&n.action), n)
        })
        .collect()
}

pub fn decide(ctx: &Ctx, st: &mut SessionState, ev: &Event) -> Decision {
    let mut d = Decision::default();
    // Exec-only routes (vendored scripts) are carried out by adage-exec, never directed on their
    // own ([`with_exec_only`]).
    let routes: Vec<&IndexedRoute> = ctx.index.for_repo(&ctx.repo).filter(|r| in_play(r)).collect();
    let mut out: Vec<Directive> = Vec::new();
    let emit_ok = |st: &SessionState, out: &Vec<Directive>, id: &str| {
        !st.emitted.iter().any(|e| e == id)
            && !out.iter().any(|o| o.route == id)
            && st.emitted.len() + out.len() < ctx.params.max_directives_per_session
            && out.len() < ctx.params.max_directives_per_decision
    };

    match ev {
        Event::SessionStart | Event::SessionEnd => {}
        Event::Prompt { text } => {
            st.prompts += 1;
            let spans = spans::extract(text);
            // 0. Red first (Design §10.6): the dead ends to announce at the first prompt are chosen
            //    before anything else, so the caps never crowd a warning out, and nothing a
            //    warning contradicts is chosen beside it. They are still emitted last, as before.
            let announce: Vec<&IndexedRoute> = if st.prompts == 1 {
                routes.iter().copied().filter(|r| matches!(r.route.trigger, Trigger::SessionStart)).collect()
            } else {
                Vec::new()
            };
            let (per_session, per_decision) =
                (ctx.params.max_directives_per_session, ctx.params.max_directives_per_decision);
            let room = per_decision.min(per_session.saturating_sub(st.emitted.len()));
            let reds: Vec<&IndexedRoute> = announce
                .iter()
                .copied()
                .filter(|r| r.route.aspect == Aspect::Red && !st.emitted.iter().any(|e| e == &r.route.id))
                .filter(|r| render::route_text(&r.route, &[]).is_some())
                .take(room)
                .collect();
            let mut reds_left = reds.len();
            let ok = |st: &SessionState, out: &Vec<Directive>, ir: &IndexedRoute, reserved: usize| {
                emit_ok(st, out, &ir.route.id)
                    && st.emitted.len() + out.len() + reserved < per_session
                    && out.len() + reserved < per_decision
                    && !reds.iter().any(|red| interlock::avoids(&red.route, &ir.route))
            };
            // 1. Prompt-triggered routes, strongest aspect first.
            let mut prompt_routes: Vec<&&IndexedRoute> =
                routes.iter().filter(|r| matches!(r.route.trigger, Trigger::Prompt { .. })).collect();
            prompt_routes
                .sort_by_key(|r| (adage_tracks::delivered_aspect(&r.route), -(r.route.evidence.support as i64)));
            for ir in prompt_routes {
                if !ok(st, &out, ir, reds_left) {
                    continue;
                }
                if let Some((text, key, head)) = bind_prompt_route(&ir.route, &spans, text, ctx) {
                    st.pending.push(Pending {
                        route: ir.route.id.clone(),
                        expect_key: key,
                        expect_head: head,
                        batches_left: ctx.params.follow_window,
                    });
                    out.push(directive(ir, text));
                }
            }
            // 2. Intent routes: the prompt means what a family means. The model is consulted only
            //    if this repository has such routes; no model, no match (fail open).
            if let Some((family, ir)) = match_intent(ctx, &routes, text) {
                st.intent = Some(family);
                if ok(st, &out, ir, reds_left) {
                    if let Some((text, key, head)) = bind_prompt_route(&ir.route, &spans, text, ctx) {
                        st.pending.push(Pending {
                            route: ir.route.id.clone(),
                            expect_key: key,
                            expect_head: head,
                            batches_left: ctx.params.follow_window,
                        });
                        out.push(directive(ir, text));
                    }
                }
            }
            // 3. Known dead ends, once, at the first prompt: the reds chosen above, and any other
            //    session-start route there is room for.
            for ir in announce {
                let red = reds.iter().any(|r| r.route.id == ir.route.id);
                if red {
                    reds_left -= 1;
                }
                if red || ok(st, &out, ir, reds_left) {
                    if let Some(t) = render::route_text(&ir.route, &[]) {
                        out.push(directive(ir, t));
                    }
                }
            }
        }
        Event::Batch { calls } => {
            st.batches += 1;
            for o in st.obligations.iter_mut() {
                o.fresh = false;
            }
            for call in calls {
                for (key, n) in call_keys(call, ctx) {
                    let key = cap(key);
                    d.observed.push(key.clone());
                    push_history_capped(st, key.clone(), ctx.params.history);
                    // Follow / derail judgement: the directed command, or it with options added.
                    st.pending.retain(|p| {
                        if meets(&p.expect_key, &key) {
                            let route = p.route.clone();
                            d.judgements.push(if call.failed {
                                Judgement::FollowedFailed { route }
                            } else {
                                Judgement::Followed { route }
                            });
                            false
                        } else {
                            true
                        }
                    });
                    // Obligations met.
                    for o in st.obligations.iter_mut().filter(|o| !o.met && meets(&o.expect_key, &key)) {
                        o.met = true;
                        d.judgements.push(Judgement::ObligationMet { route: o.route.clone() });
                    }
                    // Edits trigger finish-line obligations.
                    if keys::is_author(&n.action) {
                        st.first_edit_at_batch.get_or_insert(st.batches);
                        if let Some(p) = keys::action_path(&n.action) {
                            let rel = repo::relativize(ctx.root, p);
                            for ir in routes.iter() {
                                // Advice after an edit (a generated file, a lockfile): said once.
                                if let (Trigger::AfterEdit { paths }, Step::Advise { .. }) =
                                    (&ir.route.trigger, &ir.route.step)
                                {
                                    let hit = glob::Pattern::new(paths).map(|g| g.matches(&rel)).unwrap_or(false);
                                    if hit && emit_ok(st, &out, &ir.route.id) {
                                        if let Some(t) = render::route_text(&ir.route, &[]) {
                                            out.push(directive(ir, t));
                                        }
                                    }
                                    continue;
                                }
                                // A sequence after an edit (Track G §6.1): said once, and judged on its
                                // first command. It is not an obligation: no Stop request comes of it.
                                if let (Trigger::AfterEdit { paths }, Step::Sequence { commands }) =
                                    (&ir.route.trigger, &ir.route.step)
                                {
                                    let hit = glob::Pattern::new(paths).map(|g| g.matches(&rel)).unwrap_or(false);
                                    if hit && emit_ok(st, &out, &ir.route.id) {
                                        if let (Some(t), Some(first)) =
                                            (render::route_text(&ir.route, &[]), commands.first())
                                        {
                                            st.pending.push(pending_for(ir, first, ctx));
                                            out.push(directive(ir, t));
                                        }
                                    }
                                    continue;
                                }
                                if let (Trigger::AfterEdit { paths }, Step::Run { command }) =
                                    (&ir.route.trigger, &ir.route.step)
                                {
                                    let hit = glob::Pattern::new(paths).map(|g| g.matches(&rel)).unwrap_or(false);
                                    if !hit {
                                        continue;
                                    }
                                    // Slots bound from the edited path's segments (none for a
                                    // fixed command); a slot that cannot bind drops the route.
                                    let Some(bound) = bind_edit_slots(&ir.route, &rel, ctx) else { continue };
                                    let command = &render::bind(command, &bound);
                                    // An instinct's finish line is advice, never an obligation: no
                                    // Stop request can come of it. Its directive is judged like
                                    // any other (followed, derailed, or followed and failed).
                                    if ir.route.is_instinct() {
                                        if emit_ok(st, &out, &ir.route.id) {
                                            if let Some(t) = render::route_text(&ir.route, &bound) {
                                                st.pending.push(pending_for(ir, command, ctx));
                                                out.push(directive(ir, t));
                                            }
                                        }
                                        continue;
                                    }
                                    if let Some(o) = st.obligations.iter_mut().find(|o| o.route == ir.route.id) {
                                        // A new matching edit after the step ran re-arms the obligation.
                                        if o.met {
                                            o.met = false;
                                            o.stop_requested = false;
                                        }
                                        o.edited = rel.clone();
                                        o.fresh = true;
                                        continue;
                                    }
                                    // Only enforce a rule the agent was actually told about.
                                    let announced = st.emitted.iter().any(|e| e == &ir.route.id);
                                    let emitted_now = !announced && emit_ok(st, &out, &ir.route.id) && {
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
                                            edited: rel.clone(),
                                            fresh: true,
                                        });
                                    }
                                }
                            }
                        }
                    }
                    // Routes triggered by a successful command (e.g. read the files it listed).
                    if !call.failed {
                        for ir in routes.iter() {
                            if let Trigger::AfterCommand { command } = &ir.route.trigger {
                                if meets(command, &key) && emit_ok(st, &out, &ir.route.id) {
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
                                                Some((t, key, head)) => {
                                                    st.pending.push(Pending {
                                                        route: ir.route.id.clone(),
                                                        expect_key: key,
                                                        expect_head: head,
                                                        batches_left: ctx.params.follow_window,
                                                    });
                                                    Some(t)
                                                }
                                                None => None,
                                            }
                                        }
                                        _ => bind_result_route(&ir.route, &call.output, ctx.root),
                                    };
                                    if let Some(t) = text {
                                        out.push(directive(ir, t));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            for p in st.pending.iter_mut() {
                p.batches_left = p.batches_left.saturating_sub(1);
                if p.batches_left == 0 {
                    d.judgements.push(Judgement::Derailed { route: p.route.clone() });
                }
            }
            st.pending.retain(|p| p.batches_left > 0);
        }
        Event::Failure { call } => {
            for (key, _) in call_keys(call, ctx) {
                d.observed.push(format!("{key} !failed"));
                push_history_capped(st, format!("{key} !failed"), ctx.params.history);
                // The directed command, run within the follow window, failed.
                st.pending.retain(|p| {
                    if meets(&p.expect_key, &key) {
                        d.judgements.push(Judgement::FollowedFailed { route: p.route.clone() });
                        false
                    } else {
                        true
                    }
                });
                for ir in routes.iter() {
                    if let Trigger::AfterFailure { command, signature } = &ir.route.trigger {
                        if failure_matches(&ir.route, command, &key)
                            && call.output.contains(signature.as_str())
                            && emit_ok(st, &out, &ir.route.id)
                        {
                            if let Some(t) = render::route_text(&ir.route, &[]) {
                                // A run step after a failure is followed, or not, like any other (a
                                // sequence on its first command).
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
                }
            }
        }
        Event::Stop { stop_hook_active } => {
            if !*stop_hook_active && st.stop_requests < ctx.params.max_stop_requests {
                if let Some(o) = st.obligations.iter_mut().find(|o| !o.met && !o.stop_requested) {
                    o.stop_requested = true;
                    st.stop_requests += 1;
                    let reason = render::stop_reason(&o.paths, &o.command);
                    d.would_continue = Some(reason.clone());
                    if st.arm != Some(Arm::Holdout) {
                        d.continue_reason = Some(reason);
                    }
                }
            }
        }
    }

    let out = approach_lock(&routes, st, out);
    for o in &out {
        st.emitted.push(o.route.clone());
    }
    d.would_have = out.clone();
    if st.arm != Some(Arm::Holdout) {
        d.directives = out;
    }
    d
}

/// Approach locking (Design §10.6, "red beats green"; `adage_tracks::interlock`): once a red
/// directive (a dead end) has been emitted in a session, or is emitted in this decision, no
/// directive whose command it avoids is emitted, in this decision or any later one. A directive
/// dropped here leaves nothing behind: no pending follow judgement and no finish-line obligation,
/// so there is never a stop request to run a command the session was warned fails. A green
/// emitted before the warning stays as it was (an obligation it made is not withdrawn: a dead end
/// is announced at the first prompt, before any edit can make one).
fn approach_lock(routes: &[&IndexedRoute], st: &mut SessionState, out: Vec<Directive>) -> Vec<Directive> {
    let route_of = |id: &str| routes.iter().find(|r| r.route.id == id).map(|r| &r.route);
    let reds: Vec<&Route> = st
        .emitted
        .iter()
        .chain(out.iter().map(|d| &d.route))
        .filter_map(|id| route_of(id))
        .filter(|r| matches!(r.step, Step::Avoid { .. }))
        .collect();
    if reds.is_empty() {
        return out;
    }
    let (keep, dropped): (Vec<Directive>, Vec<Directive>) = out
        .into_iter()
        .partition(|d| route_of(&d.route).is_none_or(|r| !reds.iter().any(|red| interlock::avoids(red, r))));
    if !dropped.is_empty() {
        let gone = |id: &String| dropped.iter().any(|d| &d.route == id);
        st.pending.retain(|p| !gone(&p.route));
        st.obligations.retain(|o| !gone(&o.route));
    }
    keep
}

/// The most similar intent family whose threshold the prompt clears, if any. Ties keep the first
/// route in index order.
fn match_intent<'a>(ctx: &Ctx, routes: &[&'a IndexedRoute], text: &str) -> Option<(String, &'a IndexedRoute)> {
    let intents: Vec<&'a IndexedRoute> =
        routes.iter().copied().filter(|r| matches!(r.route.trigger, Trigger::Intent { .. })).collect();
    if intents.is_empty() {
        return None;
    }
    let (model_id, v) = (ctx.embed?)(text)?;
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

pub(crate) fn push_history(st: &mut SessionState, key: String) {
    push_history_capped(st, key, HISTORY)
}

fn push_history_capped(st: &mut SessionState, key: String, cap: usize) {
    st.history.push(key);
    if st.history.len() > cap {
        let extra = st.history.len() - cap;
        st.history.drain(..extra);
    }
}

/// Whether a failed call (canonical key `observed`) fires an after-failure trigger on `command`.
/// The directed command, or it with options added ([`meets`]); for an instinct, any run of the
/// same runner (`python3 -m unittest tests.test_billing -v` for `python3 -m unittest`): the binder
/// binds the bare spellings agents reach for, and what fails is the runner, not the arguments.
fn failure_matches(r: &Route, command: &str, observed: &str) -> bool {
    meets(command, observed)
        || (r.is_instinct()
            && !EXACT_KEYS.get()
            && command.strip_prefix("exec ").is_some_and(|runner| pattern::runs_avoided(runner, observed)))
}

/// A follow judgement for a directed fixed command (no slots).
fn pending_for(ir: &IndexedRoute, command: &str, ctx: &Ctx) -> Pending {
    let argv: Vec<String> = command.split_whitespace().map(str::to_string).collect();
    Pending {
        route: ir.route.id.clone(),
        expect_key: command_key(command, ctx.table),
        expect_head: keys::command_head(&argv),
        batches_left: ctx.params.follow_window,
    }
}

fn directive(ir: &IndexedRoute, text: String) -> Directive {
    Directive {
        route: ir.route.id.clone(),
        track: ir.track.clone(),
        version: ir.version.clone(),
        // A route on probation or stale is never delivered green, whatever its track says.
        aspect: adage_tracks::delivered_aspect(&ir.route),
        text,
        kind: adage_tracks::route_kind(&ir.route).to_string(),
        origin: ir.route.is_instinct().then(|| ir.route.origin()),
    }
}

/// Belt and braces: whatever a contract says, a bound value is a plain token.
fn plain_token(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 200
        && !v.starts_with('-')
        && v.chars().all(|c| c.is_ascii_alphanumeric() || "_./:@=+,-".contains(c))
}

fn contract_ok(c: &Contract, v: &str, root: &Path) -> bool {
    contract_ok_typed(c, None, v, root)
}

/// [`contract_ok`], where a slot with a learned value type is checked by the O(len) matcher
/// instead of a compiled regex: only when the type's runs write exactly the contract's regex
/// (`io::validate` refuses a track where they do not), so the two are the same language and no
/// regex is compiled in the hook (Track G §4.3, §7.4).
fn contract_ok_typed(c: &Contract, t: Option<&adage_tracks::ValueType>, v: &str, root: &Path) -> bool {
    if !plain_token(v) {
        return false;
    }
    match c {
        Contract::Pattern { regex } => match t {
            Some(t) if t.regex == *regex && adage_core::vtype::to_regex(&t.runs) == *regex => {
                adage_core::vtype::matches(&t.runs, v)
            }
            _ => Regex::new(regex).map(|r| r.is_match(v)).unwrap_or(false),
        },
        Contract::RepoPath => {
            if !repo::is_safe_relative(v) {
                return false;
            }
            // Resolve symlinks: the real file must still be inside the repository.
            match (root.canonicalize(), root.join(v).canonicalize()) {
                (Ok(r), Ok(p)) => p.starts_with(&r) && p.is_file(),
                _ => false,
            }
        }
    }
}

/// Bind a prompt route's slots from the prompt: a span slot from its typed spans, a learned
/// value type's slot from the first prompt token the type accepts (`adage_core::tokens`, at most
/// `PROMPT_TOKENS`, sub-tokens included). Every slot must validate ([`contract_ok_typed`], whose
/// `plain_token` check is the final gate).
fn bind_prompt_route(r: &Route, spans: &[spans::Span], text: &str, ctx: &Ctx) -> Option<(String, String, String)> {
    let Step::Run { command } = &r.step else { return None };
    let mut bound: Vec<(String, String)> = Vec::new();
    let mut toks: Option<Vec<(adage_core::tokens::Token, adage_core::tokens::Mention)>> = None;
    for s in &r.slots {
        let v = match &s.source {
            SlotSource::Prompt { span } => {
                spans.iter().find(|x| x.kind == *span && contract_ok(&s.contract, &x.value, ctx.root))?.value.clone()
            }
            SlotSource::PromptValue { vtype, mentions } => {
                let toks = toks
                    .get_or_insert_with(|| adage_core::tokens::with_mentions(text, adage_core::tokens::PROMPT_TOKENS));
                toks.iter()
                    .find(|(t, m)| {
                        (mentions.is_empty() || mentions.contains(m))
                            && adage_core::vtype::matches(&vtype.runs, &t.text)
                            && contract_ok_typed(&s.contract, Some(vtype), &t.text, ctx.root)
                    })?
                    .0
                    .text
                    .clone()
            }
            _ => return None,
        };
        bound.push((s.name.clone(), v));
    }
    bound_run(r, command, &bound, ctx)
}

/// A bound command: the directive's text, the key a follow is judged on, and its head.
fn bound_run(r: &Route, command: &str, bound: &[(String, String)], ctx: &Ctx) -> Option<(String, String, String)> {
    let text = render::route_text(r, bound)?;
    let rendered = render::bind(command, bound);
    let argv: Vec<String> = rendered.split_whitespace().map(str::to_string).collect();
    Some((text, command_key(&rendered, ctx.table), keys::command_head(&argv)))
}

/// Whether a route's slots are all bound from its triggering command's output (and it has one).
fn binds_output(r: &Route) -> bool {
    !r.slots.is_empty() && r.slots.iter().all(|s| matches!(s.source, SlotSource::OutputValue { .. }))
}

/// The candidate values of an output: the JSON leaves at a dotted `field` (through arrays), or
/// the tokens of the text (`adage_core::tokens`), bounded.
fn output_values(output: &str, field: Option<&str>) -> Vec<String> {
    let out = adage_core::tokens::bounded(output, 64 * 1024);
    let Some(field) = field else {
        return adage_core::tokens::tokens(out, 512).into_iter().map(|t| t.text).collect();
    };
    let Some(json) = extract_json(out) else { return Vec::new() };
    let mut cur = vec![&json];
    for part in field.split('.') {
        let (name, each) = match part.strip_suffix("[]") {
            Some(n) => (n, true),
            None => (part, false),
        };
        let mut next = Vec::new();
        for v in cur {
            let Some(x) = (if name.is_empty() { Some(v) } else { v.get(name) }) else { continue };
            match (each, x.as_array()) {
                (true, Some(a)) => next.extend(a.iter()),
                _ => next.push(x),
            }
        }
        cur = next;
    }
    cur.into_iter()
        .take(512)
        .filter_map(|v| match v {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .collect()
}

/// Bind a run step's slots from the triggering command's output; every slot must validate.
fn bind_output_route(r: &Route, output: &str, ctx: &Ctx) -> Option<(String, String, String)> {
    let Step::Run { command } = &r.step else { return None };
    let mut bound: Vec<(String, String)> = Vec::new();
    for s in &r.slots {
        let SlotSource::OutputValue { field, vtype } = &s.source else { return None };
        let v = output_values(output, field.as_deref()).into_iter().find(|v| {
            adage_core::vtype::matches(&vtype.runs, v) && contract_ok_typed(&s.contract, Some(vtype), v, ctx.root)
        })?;
        bound.push((s.name.clone(), v));
    }
    bound_run(r, command, &bound, ctx)
}

/// Bind a run step's slots from the edited path: segment `depth` of `rel`, which must match the
/// slot's glob and validate. `Some(empty)` for a route with no slots.
fn bind_edit_slots(r: &Route, rel: &str, ctx: &Ctx) -> Option<Vec<(String, String)>> {
    let mut bound = Vec::new();
    for s in &r.slots {
        let SlotSource::EditSegment { glob, depth } = &s.source else { return None };
        if !glob::Pattern::new(glob).is_ok_and(|g| g.matches(rel)) {
            return None;
        }
        let seg = rel.split('/').filter(|x| !x.is_empty()).nth(*depth as usize)?;
        if !contract_ok(&s.contract, seg, ctx.root) {
            return None;
        }
        bound.push((s.name.clone(), seg.to_string()));
    }
    Some(bound)
}

/// Bind a route's result-field slot from the triggering command's JSON output.
fn bind_result_route(r: &Route, output: &str, root: &Path) -> Option<String> {
    let Step::ReadBatch { slot, max } = &r.step else { return None };
    let s = r.slots.iter().find(|s| &s.name == slot)?;
    let SlotSource::ResultField { field } = &s.source else { return None };
    let candidates: Vec<String> =
        match extract_json(output).and_then(|j| j.get(field).and_then(|v| v.as_array()).cloned()) {
            Some(list) => list.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
            // Plain-text output, or the "*paths" pseudo-field: repository paths it mentions.
            None => spans::extract(output).into_iter().filter(|x| x.kind == SpanKind::Path).map(|x| x.value).collect(),
        };
    let files: Vec<String> = candidates.into_iter().filter(|p| contract_ok(&s.contract, p, root)).take(*max).collect();
    if files.is_empty() {
        return None;
    }
    render::route_text(r, &[(slot.clone(), files.join(", "))])
}

/// The first JSON object in a tool's text output.
pub fn extract_json(s: &str) -> Option<Value> {
    let start = s.find('{')?;
    let end = s.rfind('}')?;
    if end <= start {
        return None;
    }
    serde_json::from_str(&s[start..=end]).ok()
}

pub fn span_kind_label(k: SpanKind) -> &'static str {
    match k {
        SpanKind::Ticket => "work item",
        SpanKind::Path => "file",
        SpanKind::Sha => "commit",
        SpanKind::Url => "link",
    }
}

#[cfg(test)]
mod tests;
