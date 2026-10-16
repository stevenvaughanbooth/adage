//! What a decision is given besides the event and the state: the index and the checkout
//! ([`Ctx`]), and the deployment's switches and side inputs ([`Policy`]), with the matching
//! rules that depend on them (follow, meet, in play, after a failure).

use crate::observe::call_command;
use crate::{checkout, Call, RuntimeParams};
use adage_core::{pattern, CommandTable};
use adage_tracks::{Index, IndexedRoute, Route};
use std::path::Path;

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
    /// Native event cwd when the harness supplies it. Recovery advice currently binds only
    /// root-relative declared invocations and abstains in a different working directory.
    pub observed_cwd: Option<std::path::PathBuf>,
    /// Procedure cards are oriented: no exemplar; where the card's files stand in the checkout now
    /// ([`crate::orient::block`]). `false` renders cards as they were before (the exemplar of the
    /// last edit, no orientation): an evaluation knob for the card's earlier form.
    pub card_orientation: bool,
    /// Procedure cards name the test command of the repository's execution-certified learned
    /// verification ([`crate::render::card_checks_line`]).
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
    /// Routes Adage may predict at a `TurnStep`, `(track, route id)`: those holding a person's
    /// `predict` grant on this machine. Only an extension that reads grants (adage-exec) names
    /// any, so the plain `adage hook` never predicts.
    pub predict_grants: Vec<(String, String)>,
    /// At most this many predicted steps per session.
    pub predict_cap: u32,
    /// Skills a `skill` step may invoke at a `TurnStep`, by name (mod architecture §3.11): each one
    /// `adage train` recorded as inert in a pack a person accepted here, and only while the person
    /// turned `skills.invoke` on. The hook fills it (`adage_hook::skill::invocable`); empty, no
    /// skill is ever invoked, whatever the grants.
    pub invoke_skills: Vec<String>,
    /// The mod's observe mode (`ADAGE_MOD_OBSERVE`): this decision is recorded as the control arm
    /// records it (what would have been predicted or substituted, and nothing done), while the
    /// session keeps its arm for every other event.
    pub observe: bool,
    /// The person set a routine model (the mod's `routine_model`, a `config.toml` key it reads):
    /// a `TurnStep` may answer `routine` ([`crate::Decision::routine`]). Unset, no step is ever
    /// routed.
    pub routine: bool,
    /// Which phase of the mod's two-phase step a `TurnStep` is ([`crate::StepPhase`]): the
    /// default, [`StepPhase::Commit`](crate::StepPhase::Commit), decides and commits at once.
    pub step: crate::StepPhase,
}

/// The default of [`Policy::predict_cap`]. The person's `runtime.predict_cap` (adage-store's
/// config) defaults to the same number; the hook passes that value in.
pub const PREDICT_CAP: u32 = 6;

impl Default for Policy {
    fn default() -> Self {
        Policy {
            observed_cwd: None,
            card_orientation: true,
            card_checks: true,
            follow_weights: None,
            exec_only: Vec::new(),
            exact_keys: false,
            predict_grants: Vec::new(),
            predict_cap: PREDICT_CAP,
            invoke_skills: Vec::new(),
            observe: false,
            routine: false,
            step: crate::StepPhase::Commit,
        }
    }
}

impl Policy {
    /// Whether an observed key follows a directed one ([`pattern::follows`]): the follow judgement.
    pub(crate) fn follows(&self, directed: &str, observed: &str) -> bool {
        if self.exact_keys {
            directed == observed
        } else {
            pattern::follows(directed, observed)
        }
    }

    /// Whether an observed key meets a directed one ([`pattern::meets`]).
    pub(crate) fn meets(&self, directed: &str, observed: &str) -> bool {
        if self.exact_keys {
            directed == observed
        } else {
            pattern::meets(directed, observed)
        }
    }
}

/// The learned follow judge ([`Policy::follow_weights`]) over a call's raw command line.
pub(crate) fn follows_learned(policy: &Policy, directed_key: &str, call: &Call) -> bool {
    if policy.exact_keys {
        return false;
    }
    let (Some(d), Some(o)) = (directed_key.strip_prefix("exec "), call_command(call)) else { return false };
    policy.follow_weights.as_ref().is_some_and(|w| w.follows(d, o))
}

/// Whether an index entry may be directed here: not exec-only, or an exec-only route in play.
pub(crate) fn in_play(policy: &Policy, r: &IndexedRoute) -> bool {
    !r.exec_only() || policy.exec_only.iter().any(|(t, id)| t == &r.track && id == &r.route.id)
}

/// Whether a call as a whole follows a directed test run ([`pattern::follows_action`]: the
/// action classifier reads `source .venv/bin/activate && python -m pytest …` as one test run in
/// the project's environment, which no single key shows).
pub(crate) fn follows_call(policy: &Policy, directed: &str, segs: &[Vec<String>]) -> bool {
    if policy.exact_keys || segs.is_empty() {
        return false;
    }
    let r: Vec<&[String]> = segs.iter().map(Vec::as_slice).collect();
    pattern::follows_action(directed, &r)
}

/// Whether a failed call (canonical key `observed`) fires an after-failure trigger on `command`.
/// The directed command, or it with options added ([`Policy::meets`]); for an instinct, any run
/// of the same runner (`python3 -m unittest tests.test_billing -v` for `python3 -m unittest`):
/// the binder binds the bare spellings agents reach for, and what fails is the runner, not the
/// arguments. The same holds for a learned route whose trigger names a runner with no operands
/// (the value model's `exec python -m pytest`, `adage_learn::value`).
pub(crate) fn failure_matches(policy: &Policy, r: &Route, command: &str, observed: &str) -> bool {
    let bare = || pattern::Shape::of_key(command).is_some_and(|s| s.positionals.is_empty());
    policy.meets(command, observed)
        || ((r.is_instinct() || bare())
            && !policy.exact_keys
            && command.strip_prefix("exec ").is_some_and(|runner| pattern::runs_avoided(runner, observed)))
}
