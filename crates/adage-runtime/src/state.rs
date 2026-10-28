//! What a session carries from one event to the next: its experiment arm, the directives waiting
//! for a follow judgement, the finish-line obligations, the predicted-step bounds, and the rings
//! that keep an event from being decided twice. Serialised as the session's state file and
//! rebuilt from the spool ([`crate::rebuild`]); every field added since the first release is
//! `#[serde(default)]`, so a state written by an older hook still reads.

use crate::{CtxState, Standing};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Pending {
    pub route: String,
    pub expect_key: String,
    /// The directed command's identity ([`adage_core::pattern`]), for reading the state; following
    /// is judged on `expect_key` with [`adage_core::pattern::follows`] (added options and operands
    /// count).
    #[serde(default)]
    pub expect_head: String,
    pub batches_left: u32,
    /// The command the directive named, as rendered for the agent (slots bound): what a predicted
    /// step runs (`Event::TurnStep`). For a skill step, the skill's name. Empty where the
    /// directive has no fixed command (and in a state written before it was kept): such a
    /// directive is never predicted.
    #[serde(default)]
    pub command: String,
}

/// A read-batch directive delivered with its files bound from the preceding call's result (the
/// lookup's `linked_files`): what a predicted chain step reads (`Event::TurnStep`, kind
/// [`PredictionKind::Chain`](crate::PredictionKind::Chain)). It carries no follow judgement; it is
/// retired when any of its files is read, or after the follow window.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingReads {
    pub route: String,
    /// Repository-relative paths, validated by the route's slot contract, in the directive's order.
    pub files: Vec<String>,
    pub batches_left: u32,
    /// Bound from the result of a step Adage made in the agent's place (a predicted lookup), or,
    /// where a step is only recorded (the control arm), of the agent's call of a step Adage would
    /// have made: only such a batch may be read at the very next step, as the second step of a
    /// chain ([`PredictionKind::Chain`](crate::PredictionKind::Chain)), so both arms count the same
    /// steps.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub after_adage: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Obligation {
    pub route: String,
    pub expect_key: String,
    pub command: String,
    pub paths: String,
    /// The requested command was observed (or an approved action completed). This advisory
    /// bookkeeping is not evidence that tests passed or that the current checkout is verified.
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
    /// A bounded observed test failure, never proof of unchanged source or inputs. Lost state
    /// resets this watch rather than reconstructing outcomes from abbreviated command keys.
    #[serde(default, skip_serializing_if = "adage_tracks::recovery::RecoveryState::is_initial")]
    pub recovery: adage_tracks::recovery::RecoveryState,
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
    /// The latest step Adage predicted (or would have, in the control arm): `(turn id, index)`.
    /// No step right after it is predicted, so a model step always separates two predictions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_step_predicted: Option<(String, u32)>,
    /// Steps predicted this session (both arms), against [`Policy::predict_cap`](crate::Policy::predict_cap).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub predictions: u32,
    /// Routes never predicted again this session: predicted once already, or carried out by
    /// adage-exec's own CPU prediction (`Delivery.cpu_predictions` in adage-hook), which a
    /// prediction would run a second time.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub predicted_routes: Vec<String>,
    /// Routes whose predicted step the person's permissions held back this session (the mod's
    /// verdict, [`StepPhase::Verdict`](crate::StepPhase::Verdict)): never offered again this
    /// session, though another route still may be. Rebuilt from the `withheld` records.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub withheld_routes: Vec<String>,
    /// Read-batch directives whose files are bound ([`PendingReads`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reads: Vec<PendingReads>,
    /// The `PreToolUse` calls already decided this session, by [`identity_hash`] of the hook's
    /// event identity for the call (a JSON triple of the harness, `"PreToolUse"` and the call's
    /// ids; not the bare `tool_use_id`), oldest first and at most [`SEEN_PRETOOL`]: the Claude Code
    /// mod and the command hook both see a Bash call, and only the first decides it
    /// ([`SessionState::see_pretool`]). Never rebuilt from the spool: a rebuilt state starts
    /// empty, which can only decide again.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub seen_pretool: Vec<u64>,
    /// The calls the mod answered with a receipt's result
    /// ([`answered_by_receipt`](crate::answered_by_receipt), which observed them), by
    /// [`identity_hash`] of the bare `tool_use_id` (what a batch's call carries, so not the same
    /// hash as [`SessionState::seen_pretool`]'s), at most [`SEEN_PRETOOL`]: a `PostToolBatch` that
    /// lists one again does not observe it twice.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub answered: Vec<u64>,
    /// The standing text the Claude Code mod placed in the session's cached system-prompt
    /// section (the `Standing` event; the plan's `has_standing`): answered the same for the rest of
    /// the session, and the first prompt does not announce its routes again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub standing: Option<Standing>,
    /// The latest predicted step was the second of a chain (the reads bound from Adage's own
    /// lookup, at the step right after it): no third step follows without a model step between.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub chained: bool,
    /// The session's recent symbols in the candidates' own alphabet
    /// (`Index::candidate_contexts`): what a candidate's context is noticed on. Apart from
    /// [`SessionState::ctx`], so a learn that changes only candidates never resets a route's
    /// context.
    #[serde(default, skip_serializing_if = "CtxState::is_empty")]
    pub cand_ctx: CtxState,
    /// The candidates noticed this session, by id, in order: each is noticed once per session, at
    /// most [`MAX_NOTICED`](crate::MAX_NOTICED).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub noticed: Vec<String>,
    /// The run reflexes the latest prompt directed with every slot bound from its own words (a
    /// prompt or intent route whose step is one fixed command): the only steps a `TurnStep` may
    /// make at a turn's first step, before the model's first response
    /// ([`crate::prediction`]). Set at the prompt; cleared by the next tool batch, failure or
    /// Stop, so it never outlives the boundary it is for. Never rebuilt from the spool (as
    /// [`SessionState::pending`] is not): a rebuilt state predicts nothing at the first step.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub at_prompt: Vec<String>,
    /// The world as this session last saw it (`adage_trace::world`): the snapshot's hash, the
    /// states a delta is computed against, the interrupts sent, the hint waiting for its label.
    /// Never rebuilt from the spool: a rebuilt session takes a fresh snapshot.
    #[serde(default, skip_serializing_if = "adage_trace::WorldView::is_empty")]
    pub world: adage_trace::WorldView,
}

/// The 64-bit hash a call's harness identity is kept as in the session state
/// ([`SessionState::seen_pretool`], [`SessionState::answered`]; each says which identity it
/// hashes): the first eight bytes of its SHA-256, so the state stays small and the identity's
/// text is never stored.
pub fn identity_hash(identity: &str) -> u64 {
    let h = Sha256::digest(identity.as_bytes());
    u64::from_le_bytes([h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]])
}

/// At most this many `PreToolUse` identities are kept per session ([`SessionState::seen_pretool`]).
pub const SEEN_PRETOOL: usize = 64;

/// At most this many test names are kept per session ([`SessionState::new_tests`]).
pub const MAX_NEW_TESTS: usize = 64;

impl SessionState {
    /// Record that the call with this identity hash was answered by a receipt (and observed).
    pub fn see_answered(&mut self, id: u64) {
        if self.answered.contains(&id) {
            return;
        }
        if self.answered.len() >= SEEN_PRETOOL {
            self.answered.remove(0);
        }
        self.answered.push(id);
    }

    /// Whether the `PreToolUse` call with this identity hash was decided this session.
    pub fn has_seen_pretool(&self, id: u64) -> bool {
        self.seen_pretool.contains(&id)
    }

    /// Record that the `PreToolUse` call with this identity hash is decided now: `false` when it
    /// already was (the caller then decides nothing). The oldest identity makes room past
    /// [`SEEN_PRETOOL`].
    pub fn see_pretool(&mut self, id: u64) -> bool {
        if self.has_seen_pretool(id) {
            return false;
        }
        if self.seen_pretool.len() >= SEEN_PRETOOL {
            let excess = self.seen_pretool.len() + 1 - SEEN_PRETOOL;
            self.seen_pretool.drain(..excess);
        }
        self.seen_pretool.push(id);
        true
    }

    /// The routes the agent was told this session: the directives emitted, and the standing
    /// text's.
    pub(crate) fn told(&self) -> impl Iterator<Item = &String> {
        self.emitted.iter().chain(self.standing.iter().flat_map(|s| s.routes.iter()))
    }
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

pub(crate) fn push_history(st: &mut SessionState, key: String) {
    push_history_capped(st, key, crate::HISTORY)
}

pub(crate) fn push_history_capped(st: &mut SessionState, key: String, cap: usize) {
    st.history.push(key);
    if st.history.len() > cap {
        let extra = st.history.len() - cap;
        st.history.drain(..extra);
    }
}
