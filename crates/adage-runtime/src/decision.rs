//! What one decision says: the directives the agent receives, the judgements of what it did,
//! and the steps Adage takes or would take in its place. Recorded in every spool record in both
//! arms, and read back by [`crate::rebuild`], the eval and the app, so a field is only ever added
//! (`#[serde(default)]`) and a `kind` is never renamed.

use crate::{CtxState, Noticed};
use adage_tracks::{IndexedRoute, Strength};
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    /// instead of claiming session evidence ([`crate::render::footer`]).
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
    /// The directed step was taken by Adage in the agent's place (a predicted call, `By::Adage`),
    /// not by the agent: never a follow, never evidence of what the agent does.
    Predicted {
        route: String,
    },
    /// The call was a learned dead end, and Adage ran the route's `instead` in its place before it
    /// ran ([`Substitution`]): never a follow, and the dead end was avoided, not hit.
    Substituted {
        route: String,
    },
}

/// What a predicted step is ([`Prediction::kind`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PredictionKind {
    /// An armed, unmet finish-line obligation, at the step that ends the turn.
    Obligation,
    /// A pending run directive's command.
    Run,
    /// A pending sequence directive's first command.
    Sequence,
    /// A pending skill step: the Skill tool with the skill's name.
    Skill,
    /// A read-batch directive's files, bound from the preceding call's result: one Read each.
    Chain,
}

/// One tool call Adage makes in the agent's place: the harness tool and its input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PredictedCall {
    pub tool: String,
    pub input: Value,
}

/// The step Adage makes in the agent's place at a `TurnStep`: only what it already directed
/// (architecture §3.2 of the Claude Code mod).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prediction {
    pub kind: PredictionKind,
    pub route: String,
    pub track: String,
    pub calls: Vec<PredictedCall>,
}

/// A step routed to the person's routine model (the Claude Code mod's `routine_model`, its
/// architecture §3.5): the model is about to be asked to do what a delivered `fire` directive
/// already says, and no `predict` grant lets Adage make the call itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Routing {
    pub route: String,
    pub track: String,
}

/// A learned dead end replaced before it runs (architecture §3.4 of the Claude Code mod): the
/// call meets an `avoid` route in service at full strength whose `instead` is a fixed command,
/// and the mod runs `command` in the call's place ([`substitution`](crate::substitution)).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Substitution {
    pub route: String,
    pub track: String,
    /// What runs instead: the route's `instead`.
    pub command: String,
    /// The one line the model reads after the result, and the person under the dialog.
    pub context: String,
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
    /// rebuilt from the spool takes it from ([`crate::rebuild`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ctx: Option<CtxState>,
    /// The step Adage makes in the agent's place (`TurnStep`; empty in the control arm).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predicted: Option<Prediction>,
    /// The step it would make, recorded in both arms, also when the session's cap holds it back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub would_predict: Option<Prediction>,
    /// `(turn id, index)` of the step whose prediction counted against the bounds (both arms):
    /// what a state rebuilt from the spool takes `last_step_predicted` and `predictions` from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predicted_at: Option<(String, u32)>,
    /// The dead end Adage replaced before it ran (`PreToolUse` with the mod's verdict; empty in
    /// the control arm and in observe mode).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub substituted: Option<Substitution>,
    /// The replacement it would make, recorded in both arms.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub would_substitute: Option<Substitution>,
    /// The step goes to the routine model (`TurnStep`, only when the mod says one is set; empty
    /// in the control arm and in observe mode).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routine: Option<Routing>,
    /// The routing it would make, recorded in both arms.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub would_route: Option<Routing>,
    /// Candidates this event's context completed, each once per session ([`Noticed`]), in both
    /// arms; never delivered.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub noticed: Vec<Noticed>,
    /// The session's candidate context state after this event, when the event changed it: what a
    /// state rebuilt from the spool takes it from, as [`Decision::ctx`] is for routes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cand_ctx: Option<CtxState>,
}

/// The directive an index entry's text becomes.
pub(crate) fn directive(ir: &IndexedRoute, text: String) -> Directive {
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
