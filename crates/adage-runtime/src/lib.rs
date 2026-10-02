//! Hook-time decisions (Design §9, §13; Architecture §15).
//!
//! Pure logic: given the compiled index, the session's state, and one canonical event, decide
//! which directives to emit. No I/O except checking that bound repository paths exist. The CLI
//! owns reading payloads, locking state, writing spools, and printing harness output.

pub mod checkout;
pub mod context;
pub mod fast;
pub mod orient;
pub mod pretool;
pub mod pytest_case;
pub mod rebuild;
pub mod render;

use adage_core::keys::{self, template_key};
use adage_core::spans::{self, SpanKind};
use adage_core::tools::normalize_tool;
use adage_core::{outcome, pattern};
use adage_core::{repo, CommandTable};
use adage_tracks::context::CtxNet;
use adage_tracks::{interlock, Contract, Index, IndexedRoute, Route, SlotSource, Step, Strength, Trigger};
pub use context::CtxState;
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
    Control,
}

/// Deterministic experiment assignment (Architecture A12): stable per session, reproducible.
pub fn assign_arm(session: &str, control_share: f64) -> Arm {
    let h = Sha256::digest(format!("adage-arm:{session}").as_bytes());
    let x = u32::from_le_bytes([h[0], h[1], h[2], h[3]]) as f64 / u32::MAX as f64;
    if x < control_share {
        Arm::Control
    } else {
        Arm::Treated
    }
}

/// The arm a session gets, and its **propensity**: the probability that assignment gave it that
/// arm, recorded in every spool record for inverse-propensity weighting later (Design §11, §14.1).
/// With the control share `h` (clamped to 0..=1): `h` for the control arm, `1 - h` for the
/// treated arm. An arm forced with `ADAGE_ARM` (`forced` = `treated` or `control`) was not drawn:
/// its propensity is 1. Any other `forced` value is ignored.
pub fn assign(session: &str, control_share: f64, forced: Option<&str>) -> (Arm, f64) {
    match forced {
        Some("control") => return (Arm::Control, 1.0),
        Some("treated") => return (Arm::Treated, 1.0),
        _ => {}
    }
    let h = if control_share.is_nan() { 0.0 } else { control_share.clamp(0.0, 1.0) };
    match assign_arm(session, control_share) {
        Arm::Control => (Arm::Control, h),
        Arm::Treated => (Arm::Treated, 1.0 - h),
    }
}

pub use adage_trace::{Call, Event};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Pending {
    pub route: String,
    pub expect_key: String,
    /// The directed command's identity ([`pattern`]), for reading the state; following is judged
    /// on `expect_key` with [`pattern::follows`] (added options and operands count).
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
    /// The sequence that carries it, when a sequence after the same edit begins with this
    /// obligation's command: the agent is told once (the sequence), and the obligation rides on
    /// the sequence's first step (met when it runs, a Stop request if it does not).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub via: String,
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
    /// The session's recent context symbols (Track G §7.1), what context triggers match on.
    #[serde(default, skip_serializing_if = "CtxState::is_empty")]
    pub ctx: CtxState,
    /// A procedure card whose family a prompt came close to (within the grey margin below its
    /// threshold): said at the session's first edit of one of the card's files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card_waiting: Option<String>,
    /// Stop-gate runs this session (adage-exec): at most two, and only the first may block.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub stop_gates: u32,
    /// Test functions this session's edits added to test files, by name, in order (bounded): a
    /// verification receipt names these as the agent's own tests (adage-exec).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub new_tests: Vec<String>,
    /// The person was told this session that a grant no longer matches (adage-exec): at most
    /// once per session.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub lapse_noticed: bool,
}

/// At most this many test names are kept per session ([`SessionState::new_tests`]).
pub const MAX_NEW_TESTS: usize = 64;

/// `def test…` names (sync or async, at any indentation) in a text an edit wrote.
fn test_names(text: &str) -> Vec<&str> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim_start();
            let r = l.strip_prefix("async def ").or_else(|| l.strip_prefix("def "))?;
            let end = r.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(r.len());
            let name = &r[..end];
            (name.starts_with("test") && name.len() <= 100).then_some(name)
        })
        .collect()
}

/// The test functions a successful edit adds to a test file ([`orient::is_test_file`]): names in
/// what it wrote that were not in what it replaced (an Edit's or MultiEdit's `new_string`s over
/// their `old_string`s; a Write's whole content).
pub(crate) fn added_tests(call: &Call, root: &std::path::Path) -> Vec<String> {
    if call.failed || !matches!(call.tool.as_str(), "Edit" | "MultiEdit" | "Write") {
        return Vec::new();
    }
    let Some(path) = call.input.get("file_path").and_then(Value::as_str) else { return Vec::new() };
    if !orient::is_test_file(&repo::relativize(root, path)) {
        return Vec::new();
    }
    let field = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    let mut pairs: Vec<(String, String)> = Vec::new();
    if let Some(t) = field(&call.input, "content") {
        pairs.push((t, String::new()));
    }
    if let Some(t) = field(&call.input, "new_string") {
        pairs.push((t, field(&call.input, "old_string").unwrap_or_default()));
    }
    for e in call.input.get("edits").and_then(Value::as_array).into_iter().flatten() {
        if let Some(t) = field(e, "new_string") {
            pairs.push((t, field(e, "old_string").unwrap_or_default()));
        }
    }
    let mut out: Vec<String> = Vec::new();
    for (new, old) in &pairs {
        let before = test_names(old);
        for name in test_names(new) {
            if !before.contains(&name) && !out.iter().any(|n| n == name) {
                out.push(name.to_string());
            }
        }
    }
    out
}

fn is_zero(n: &u32) -> bool {
    *n == 0
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
    pub strength: Strength,
    pub text: String,
    /// The route's delivery shape ([`adage_tracks::shape`]), rendered into the directive's
    /// wrapper (`kind="…"`). Empty in records written before it existed.
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
    Ignored {
        route: String,
    },
    ObligationMet {
        route: String,
    },
    /// The agent ran the directed command within the follow window, and it failed. Three of
    /// these (or an ignore rate whose Wilson lower bound reaches 0.5) extinguish an instinct route at
    /// the next learn (`adage_learn::instincts`).
    Misfired {
        route: String,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Decision {
    /// What the agent receives (empty in the control arm).
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
    /// The session's context state after this event, when the event changed it: what a state
    /// rebuilt from the spool takes it from ([`rebuild`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ctx: Option<CtxState>,
}

/// Embeds a raw prompt: `(model id, unit vector)`, or `None` when no model is available. Called
/// only at a prompt decision point, and only when the repository has intent routes.
pub type Embed<'a> = &'a dyn Fn(&str) -> Option<(String, Vec<f32>)>;

#[derive(Clone)]
pub struct Ctx<'a> {
    pub index: &'a Index,
    pub repo: String,
    pub root: &'a Path,
    /// What the decision may read of the checkout at `root` ([`checkout::Disk`] in the hook).
    pub checkout: &'a dyn checkout::Checkout,
    pub table: &'a CommandTable,
    pub embed: Option<Embed<'a>>,
    /// Limits: the follow window, directives per session and per decision, stop requests,
    /// history. The hook always uses [`RuntimeParams::default`]; the offline eval can tune them.
    pub params: RuntimeParams,
    /// What this deployment of the decision allows and knows beyond the index: everything that
    /// used to be set around `decide` instead of passed to it.
    pub policy: Policy,
}

/// The decision's switches and side inputs. Every field has the hook's default, so a `Ctx` built
/// with `Policy::default()` decides exactly as the plain `adage hook` does.
#[derive(Debug, Clone)]
pub struct Policy {
    /// Procedure cards are oriented: no exemplar; where the card's files stand in the checkout now
    /// ([`orient::block`]). `false` renders cards as they were before (the exemplar of the last
    /// edit, no orientation): an evaluation knob for the card's earlier form.
    pub card_orientation: bool,
    /// Procedure cards name the test command of the repository's execution-certified learned
    /// verification ([`render::card_checks_line`]).
    pub card_checks: bool,
    /// The repository's learned follow-judge weights (`adage_core::judge`, fitted at learn time from
    /// its sessions): a follow is then also judged by the learned judge, on the raw command the
    /// agent ran. Without them only the structural and action judges apply.
    pub follow_weights: Option<adage_core::judge::Weights>,
    /// Exec-only routes in play, `(track, route id)`: reflexes that run a vendored script. Only
    /// `adage-exec hook` names any, for the ones approved on this machine, because it carries them
    /// out itself (Track C §1.7); everywhere else they are never directed.
    pub exec_only: Vec<(String, String)>,
    /// Compatibility: decide with the matching the hooks used before the command-pattern algebra,
    /// where a trigger, an obligation or a follow judgement needed the observed key to equal the
    /// directed one exactly. Only for replaying decisions that hooks of that time recorded (the
    /// eval's fidelity check, `adage_eval::fidelity`).
    pub exact_keys: bool,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            card_orientation: true,
            card_checks: true,
            follow_weights: None,
            exec_only: Vec::new(),
            exact_keys: false,
        }
    }
}

impl Policy {
    /// Whether an observed key follows a directed one ([`pattern::follows`]): the follow judgement.
    fn follows(&self, directed: &str, observed: &str) -> bool {
        if self.exact_keys {
            directed == observed
        } else {
            pattern::follows(directed, observed)
        }
    }

    /// Whether an observed key meets a directed one ([`pattern::meets`]).
    fn meets(&self, directed: &str, observed: &str) -> bool {
        if self.exact_keys {
            directed == observed
        } else {
            pattern::meets(directed, observed)
        }
    }
}

/// A card route's text: oriented ([`Policy::card_orientation`]) or as before; any other route's text.
fn card_text(ctx: &Ctx<'_>, route: &adage_tracks::Route) -> Option<String> {
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

/// The learned follow judge ([`Policy::follow_weights`]) over a call's raw command line.
fn follows_learned(policy: &Policy, directed_key: &str, call: &Call) -> bool {
    if policy.exact_keys {
        return false;
    }
    let (Some(d), Some(o)) = (directed_key.strip_prefix("exec "), call_command(call)) else { return false };
    policy.follow_weights.as_ref().is_some_and(|w| w.follows(d, o))
}

/// Whether an index entry may be directed here: not exec-only, or an exec-only route in play.
fn in_play(policy: &Policy, r: &IndexedRoute) -> bool {
    !r.exec_only() || policy.exec_only.iter().any(|(t, id)| t == &r.track && id == &r.route.id)
}

/// The argv of each command a call ran, in order (an activation, the run, a filter), for the
/// action-level follow judgement ([`follows_call`]).
fn exec_segments(keyed: &[(String, adage_core::Normalized)]) -> Vec<Vec<String>> {
    keyed
        .iter()
        .filter_map(|(_, n)| match &n.action {
            adage_core::CanonicalAction::Exec { argv, .. } => Some(argv.clone()),
            _ => None,
        })
        .collect()
}

/// Whether a call as a whole follows a directed test run ([`pattern::follows_action`]: the
/// action classifier reads `source .venv/bin/activate && python -m pytest …` as one test run in
/// the project's environment, which no single key shows).
fn follows_call(policy: &Policy, directed: &str, segs: &[Vec<String>]) -> bool {
    if policy.exact_keys || segs.is_empty() {
        return false;
    }
    let r: Vec<&[String]> = segs.iter().map(Vec::as_slice).collect();
    pattern::follows_action(directed, &r)
}

/// The command line of a shell tool call (Claude Code's `Bash`, Codex's `shell`), if it has one.
fn call_command(call: &Call) -> Option<&str> {
    match call.input.get("command")? {
        Value::String(s) => Some(s.as_str()),
        // Codex: an argv, `["bash", "-lc", "…"]`.
        Value::Array(a) => a.last().and_then(Value::as_str),
        _ => None,
    }
}

/// Fire the after-failure triggers (dead ends and instincts) a failed run of `key` with this
/// output meets: [`failure_matches`] and the route's signature in the output. A run step, or a
/// sequence's first command, is then judged like any other directive.
fn fire_after_failure(
    ctx: &Ctx,
    routes: &[&IndexedRoute],
    st: &mut SessionState,
    out: &mut Vec<Directive>,
    key: &str,
    output: &str,
    emit_ok: &dyn Fn(&SessionState, &Vec<Directive>, &str) -> bool,
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
        if (answered && advice(ir)) || !emit_ok(st, out, &ir.route.id) {
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
    // own ([`Policy::exec_only`]).
    let routes: Vec<&IndexedRoute> = ctx.index.for_repo(&ctx.repo).filter(|r| in_play(&ctx.policy, r)).collect();
    // Context triggers (Track G §7): the repository's alphabet and net, and a symboliser for it,
    // only when it has any.
    let net = ctx.index.contexts(&ctx.repo);
    let syms = (!net.is_empty()).then(|| context::Symbols::new(&net).exact_keys(ctx.policy.exact_keys));
    let ctx_before = syms.as_ref().map(|_| st.ctx.clone());
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
            // A card waiting for an edit belongs to the prompt that came close to it: a new prompt
            // is a new task, so it waits no longer (it waits again below if this prompt is close).
            st.card_waiting = None;
            let spans = spans::extract(text);
            // 0. Inhibit first (Design §10.6): the dead ends to announce at the first prompt are chosen
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
            let inhibits: Vec<&IndexedRoute> = announce
                .iter()
                .copied()
                .filter(|r| r.route.strength == Strength::Inhibit && !st.emitted.iter().any(|e| e == &r.route.id))
                .filter(|r| render::route_text(&r.route, &[]).is_some())
                .take(room)
                .collect();
            let mut inhibits_left = inhibits.len();
            let ok = |st: &SessionState, out: &Vec<Directive>, ir: &IndexedRoute, reserved: usize| {
                emit_ok(st, out, &ir.route.id)
                    && st.emitted.len() + out.len() + reserved < per_session
                    && out.len() + reserved < per_decision
                    && !inhibits.iter().any(|inhibit| interlock::avoids(&inhibit.route, &ir.route))
            };
            // 1. Prompt-triggered routes, strongest strength first.
            let mut prompt_routes: Vec<&&IndexedRoute> =
                routes.iter().filter(|r| matches!(r.route.trigger, Trigger::Prompt { .. })).collect();
            prompt_routes
                .sort_by_key(|r| (adage_tracks::delivered_strength(&r.route), -(r.route.evidence.support as i64)));
            for ir in prompt_routes {
                if !ok(st, &out, ir, inhibits_left) {
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
            //    if this repository has such routes (or intent context items); no model, no
            //    match (fail open). It embeds the prompt once.
            let mut emb: Option<Option<(String, Vec<f32>)>> = None;
            // A family's procedure card is matched on its own (it does not compete with a run),
            // said once, and directs no command of its own.
            let (cards, runs): (Vec<&IndexedRoute>, Vec<&IndexedRoute>) =
                routes.iter().copied().partition(|r| matches!(r.route.step, Step::Card { .. }));
            match match_card(ctx, &cards, text, &mut emb) {
                Some((ir, true)) => {
                    if ok(st, &out, ir, inhibits_left) {
                        if let Some(mut t) = card_text(ctx, &ir.route) {
                            // Execution-only verifications are out of play as advice; the card may still
                            // name their command as a fact about the repository.
                            let all: Vec<&IndexedRoute> = ctx.index.for_repo(&ctx.repo).collect();
                            let checks = certified_checks(&all);
                            if let Some(line) =
                                ctx.policy.card_checks.then(|| render::card_checks_line(&checks)).flatten()
                            {
                                t.push('\n');
                                t.push_str(&line);
                            }
                            out.push(directive(ir, t));
                        }
                    }
                }
                Some((ir, false)) if emit_ok(st, &out, &ir.route.id) => st.card_waiting = Some(ir.route.id.clone()),
                _ => {}
            }
            if let Some((family, ir)) = match_intent(ctx, &runs, text, &mut emb) {
                st.intent = Some(family);
                if ok(st, &out, ir, inhibits_left) {
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
            // 2b. Context triggers ending in a prompt item (a learned value type, an intent).
            if let Some(sy) = syms.as_ref().filter(|sy| sy.has_prompt_items()) {
                let got = sy.prompt(text, || embedding(ctx, text, &mut emb));
                if st.ctx.push(&net, &got) {
                    fire_contexts(ctx, &routes, &net, st, &mut out, |st, out, ir| ok(st, out, ir, inhibits_left));
                }
            }
            // 3. Known dead ends, once, at the first prompt: the inhibits chosen above, and any other
            //    session-start route there is room for.
            for ir in announce {
                let inhibit = inhibits.iter().any(|r| r.route.id == ir.route.id);
                if inhibit {
                    inhibits_left -= 1;
                }
                if inhibit || ok(st, &out, ir, inhibits_left) {
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
            // A failed call the failure hook just reported, listed again in this batch.
            let mut open_failure = syms.as_ref().and_then(|_| st.ctx.open_failure(&net));
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
                let keyed = call_keys(call, ctx);
                let segs = exec_segments(&keyed);
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
                            d.judgements.push(if misfire {
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
                        fire_after_failure(ctx, &routes, st, &mut out, &key, &call.output, &emit_ok);
                    }
                    // Obligations met.
                    for o in st.obligations.iter_mut().filter(|o| !o.met && ctx.policy.meets(&o.expect_key, &key)) {
                        o.met = true;
                        d.judgements.push(Judgement::ObligationMet { route: o.route.clone() });
                    }
                    // Edits trigger finish-line obligations.
                    if keys::is_author(&n.action) {
                        st.first_edit_at_batch.get_or_insert(st.batches);
                        if let Some(p) = keys::action_path(&n.action) {
                            let rel = repo::relativize(ctx.root, p);
                            // A procedure card a prompt came close to: said at the first edit of
                            // one of its files.
                            if let Some(id) = st.card_waiting.clone() {
                                let hit = routes.iter().find(|r| r.route.id == id).filter(|r| {
                                    matches!(&r.route.step, Step::Card { card } if adage_tracks::card::touches(card, &rel))
                                });
                                if let Some(ir) = hit {
                                    st.card_waiting = None;
                                    if emit_ok(st, &out, &ir.route.id) {
                                        if let Some(t) = card_text(ctx, &ir.route) {
                                            out.push(directive(ir, t));
                                        }
                                    }
                                }
                            }
                            let hits = |paths: &str| glob::Pattern::new(paths).is_ok_and(|g| g.matches(&rel));
                            // One edit, one directive: a sequence after this edit with a step that
                            // is a finish line's command subsumes that finish line (a compiled
                            // skill's `make test`, `make ci` carries its "before you finish: make
                            // ci", which holds after any edit). Returns the sequence and the
                            // finish line's bound command.
                            let subsumer = |o: &IndexedRoute| -> Option<(String, String)> {
                                let (Trigger::AfterEdit { paths }, Step::Run { command }) =
                                    (&o.route.trigger, &o.route.step)
                                else {
                                    return None;
                                };
                                if !hits(paths) {
                                    return None;
                                }
                                let command = render::bind(command, &bind_edit_slots(&o.route, &rel, ctx)?);
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
                                // first command. On its own it is not an obligation, but it carries
                                // the obligations of the finish lines it subsumes (their command is
                                // its first step), which then say nothing themselves.
                                if let (Trigger::AfterEdit { paths }, Step::Sequence { commands }) =
                                    (&ir.route.trigger, &ir.route.step)
                                {
                                    if !hits(paths) {
                                        continue;
                                    }
                                    let announced = st.emitted.iter().any(|e| e == &ir.route.id);
                                    let emitted_now = !announced && emit_ok(st, &out, &ir.route.id) && {
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
                                            if via != ir.route.id
                                                || st.obligations.iter().any(|x| x.route == o.route.id)
                                            {
                                                continue;
                                            }
                                            st.obligations.push(Obligation {
                                                route: o.route.id.clone(),
                                                expect_key: command_key(&command, ctx.table),
                                                command,
                                                paths: paths.clone(),
                                                met: false,
                                                stop_requested: false,
                                                edited: rel.clone(),
                                                fresh: true,
                                                via,
                                            });
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
                                    let subsumed = subsumer(ir).is_some();
                                    // An instinct's finish line is advice, never an obligation: no
                                    // Stop request can come of it. Its directive is judged like
                                    // any other (followed, ignored, or misfired). A sequence that
                                    // begins with its command says it instead.
                                    if ir.route.is_instinct() {
                                        if subsumed {
                                            continue;
                                        }
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
                                    // Carried by the sequence that subsumes it (armed there).
                                    if subsumed {
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
                                            via: String::new(),
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
                                if ctx.policy.meets(command, &key) && emit_ok(st, &out, &ir.route.id) {
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
                                        _ => bind_result_route(&ir.route, &call.output, ctx),
                                    };
                                    if let Some(t) = text {
                                        out.push(directive(ir, t));
                                    }
                                }
                            }
                        }
                    }
                    // Context triggers: symbolise the call, push it if it is an alphabet event,
                    // and walk the net from it.
                    if let Some(sy) = &syms {
                        let mut got = Vec::new();
                        if keys::is_author(&n.action) {
                            if let Some(p) = keys::action_path(&n.action) {
                                got.extend(sy.edit(&repo::relativize(ctx.root, p)));
                            }
                        }
                        if call.failed {
                            got.extend(sy.fail(&key, call));
                        } else {
                            let acts = sy.act(&key);
                            if !acts.is_empty() && open_failure == Some(context::key_hash(&key)) {
                                // Reported by the failure hook already: not a successful command.
                                open_failure = None;
                                st.ctx.last_fail = None;
                            } else {
                                got.extend(acts);
                            }
                        }
                        got.sort_unstable();
                        got.dedup();
                        if st.ctx.push(&net, &got) {
                            fire_contexts(ctx, &routes, &net, st, &mut out, |st, out, ir| {
                                emit_ok(st, out, &ir.route.id)
                            });
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
        }
        Event::Failure { call } => {
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
                        d.judgements.push(if outcome::misfired(&call.output) {
                            Judgement::Misfired { route }
                        } else {
                            Judgement::Followed { route }
                        });
                        false
                    } else {
                        true
                    }
                });
                fire_after_failure(ctx, &routes, st, &mut out, &key, &call.output, &emit_ok);
                // Context triggers: a failure item, and the record that keeps the batch after it
                // from counting this call as a successful command.
                if let Some(sy) = &syms {
                    let pushed = st.ctx.push(&net, &sy.fail(&key, call));
                    if !sy.act(&key).is_empty() {
                        st.ctx.failed(&net, &key);
                    }
                    if pushed {
                        fire_contexts(ctx, &routes, &net, st, &mut out, |st, out, ir| emit_ok(st, out, &ir.route.id));
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
                    if st.arm != Some(Arm::Control) {
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
    if ctx_before.is_some_and(|b| b != st.ctx) {
        d.ctx = Some(st.ctx.clone());
    }
    d.would_have = out.clone();
    if st.arm != Some(Arm::Control) {
        d.directives = out;
    }
    d
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
        .emitted
        .iter()
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
        st.obligations.retain(|o| !gone(&o.route) && !gone(&o.via));
    }
    keep
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

/// The context routes the newest alphabet event completes ([`CtxNet::walk`]), in index order,
/// each emitted if `ok` allows: a run step, or a sequence, from its fixed template, with a follow
/// judgement on its (first) command. No slot is bound and no obligation is made: a context
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
        let first = match &ir.route.step {
            Step::Run { command } => command,
            Step::Sequence { commands } => match commands.first() {
                Some(c) => c,
                None => continue,
            },
            _ => continue,
        };
        let Some(t) = render::route_text(&ir.route, &[]) else { continue };
        st.pending.push(pending_for(ir, first, ctx));
        out.push(directive(ir, t));
    }
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
/// binds the bare spellings agents reach for, and what fails is the runner, not the arguments. The
/// same holds for a learned route whose trigger names a runner with no operands (the value model's
/// `exec python -m pytest`, `adage_learn::value`).
fn failure_matches(policy: &Policy, r: &Route, command: &str, observed: &str) -> bool {
    let bare = || pattern::Shape::of_key(command).is_some_and(|s| s.positionals.is_empty());
    policy.meets(command, observed)
        || ((r.is_instinct() || bare())
            && !policy.exact_keys
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
        // A route forming or dormant is never delivered fire, whatever its track says.
        strength: adage_tracks::delivered_strength(&ir.route),
        text,
        kind: adage_tracks::shape(&ir.route).to_string(),
        // An instinct's and a compiled skill's wrapper say what they are based on (not sessions).
        origin: (ir.route.is_instinct() || ir.route.evidence.origin == Some(adage_tracks::Origin::Authored))
            .then(|| ir.route.origin()),
    }
}

/// Belt and braces: whatever a contract says, a bound value is a plain token.
fn plain_token(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 200
        && !v.starts_with('-')
        && v.chars().all(|c| c.is_ascii_alphanumeric() || "_./:@=+,-".contains(c))
}

fn contract_ok(c: &Contract, v: &str, ctx: &Ctx) -> bool {
    contract_ok_typed(c, None, v, ctx)
}

/// [`contract_ok`], where a slot with a learned value type is checked by the O(len) matcher
/// instead of a compiled regex: only when the type's runs write exactly the contract's regex
/// (`io::validate` refuses a track where they do not), so the two are the same language and no
/// regex is compiled in the hook (Track G §4.3, §7.4).
fn contract_ok_typed(c: &Contract, t: Option<&adage_tracks::ValueType>, v: &str, ctx: &Ctx) -> bool {
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
            // Symlinks resolved: the real file must still be inside the repository.
            repo::is_safe_relative(v) && ctx.checkout.is_file(ctx.root, v)
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
                spans.iter().find(|x| x.kind == *span && contract_ok(&s.contract, &x.value, ctx))?.value.clone()
            }
            SlotSource::PromptValue { vtype, mentions } => {
                let toks = toks
                    .get_or_insert_with(|| adage_core::tokens::with_mentions(text, adage_core::tokens::PROMPT_TOKENS));
                toks.iter()
                    .find(|(t, m)| {
                        (mentions.is_empty() || mentions.contains(m))
                            && adage_core::vtype::matches(&vtype.runs, &t.text)
                            && contract_ok_typed(&s.contract, Some(vtype), &t.text, ctx)
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
            adage_core::vtype::matches(&vtype.runs, v) && contract_ok_typed(&s.contract, Some(vtype), v, ctx)
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
        match &s.source {
            SlotSource::EditSegment { glob, depth } => {
                if !glob::Pattern::new(glob).is_ok_and(|g| g.matches(rel)) {
                    return None;
                }
                let seg = rel.split('/').filter(|x| !x.is_empty()).nth(*depth as usize)?;
                if !contract_ok(&s.contract, seg, ctx) {
                    return None;
                }
                bound.push((s.name.clone(), seg.to_string()));
            }
            // The edited file's tests from the learned map: each must exist in the repository.
            SlotSource::EditTests { glob, k, map } => {
                if !glob::Pattern::new(glob).is_ok_and(|g| g.matches(rel)) {
                    return None;
                }
                let tests: Vec<&str> = map
                    .get(rel)?
                    .iter()
                    .filter(|t| contract_ok(&s.contract, t, ctx))
                    .take(*k as usize)
                    .map(String::as_str)
                    .collect();
                if tests.is_empty() {
                    return None;
                }
                bound.push((s.name.clone(), tests.join(" ")));
            }
            _ => return None,
        }
    }
    Some(bound)
}

/// Bind a route's result-field slot from the triggering command's JSON output.
fn bind_result_route(r: &Route, output: &str, ctx: &Ctx) -> Option<String> {
    let Step::ReadBatch { slot, max } = &r.step else { return None };
    let s = r.slots.iter().find(|s| &s.name == slot)?;
    let SlotSource::ResultField { field } = &s.source else { return None };
    let candidates: Vec<String> =
        match extract_json(output).and_then(|j| j.get(field).and_then(|v| v.as_array()).cloned()) {
            Some(list) => list.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
            // Plain-text output, or the "*paths" pseudo-field: repository paths it mentions.
            None => spans::extract(output).into_iter().filter(|x| x.kind == SpanKind::Path).map(|x| x.value).collect(),
        };
    let files: Vec<String> = candidates.into_iter().filter(|p| contract_ok(&s.contract, p, ctx)).take(*max).collect();
    if files.is_empty() {
        return None;
    }
    render::route_text(r, &[(slot.clone(), files.join(", "))])
}

pub use adage_core::json::extract_json;

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
