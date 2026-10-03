//! What the binary answers the Claude Code mod: every answer shape in one enum ([`ModAnswer`]) and
//! one function that writes it ([`answer`]). A command hook's answer is Claude Code's own format
//! (`hookSpecificOutput`, `systemMessage`, `decision`), written by the adapter's `render`; these are
//! the answers only the mod reads, to its own asks (`PreToolUse` dry runs and verdicts, `TurnStep`,
//! `Standing`, `SkillPrompt`, `Press`).
//!
//! One JSON file per answer shape, `crates/adage-hook/tests/wire/*.json`, pins the wire: the
//! crate's `wire` test writes each from a [`ModAnswer`] and compares, and the mod's own tests read
//! the same files (docs/claude-mod/impl/wire.md). An answer is a JSON object whose keys serialise
//! sorted, whatever the order of the fields here.

use crate::ReceiptCard;
use adage_runtime::{Prediction, Substitution};
use serde::{Deserialize, Serialize};

/// One answer to the mod. Each variant is one JSON object, keyed as the mod reads it (the variant
/// is not named on the wire: its keys are the shape).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ModAnswer {
    /// The dry run's plan for a Bash call (`{"plan": {…}}`): what the verdict may decide.
    Plan { plan: Plan },
    /// A learned dead end replaced by its working command, on the engine's `same` verdict.
    Substitute { substitute: Substitution },
    /// The call answered by a fresh receipt instead of running it.
    Receipt { receipt: ReceiptAnswer },
    /// The standing text, its environment sentence, what compaction keeps, and, for a subagent's
    /// spawn, what the subagent is told.
    Standing {
        standing: String,
        environment: String,
        compact: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subagent: Option<String>,
    },
    /// A skill's text with what Adage verified appended.
    Text { text: String },
    /// A press token (64 hex characters).
    Press { press: String },
    /// The calls a step makes in the model's place.
    Predict { predict: Prediction },
    /// The step goes to the person's routine model (always `true`).
    Routine { routine: bool },
}

/// The dry run's plan: whichever apply. Empty, there is no answer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    /// A learned dead end and the command that replaces it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub substitute: Option<Substitution>,
    /// The interpreter rewrite, before the engine compares the two commands' permissions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rewrite: Option<PlanRewrite>,
    /// Advice instead of a rewrite.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advice: Option<PlanAdvice>,
    /// A receipt of a run this session made that covers the call (treated sessions only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<PlanReceipt>,
}

impl Plan {
    /// Nothing to plan.
    pub fn is_empty(&self) -> bool {
        self.substitute.is_none() && self.rewrite.is_none() && self.advice.is_none() && self.receipt.is_none()
    }
}

/// The interpreter rewrite in a plan: the command as it would run, and why (`test` or `repro`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanRewrite {
    pub command: String,
    pub kind: String,
}

/// Advice in a plan: its kind (`permissions`, `pip-install`, …) and its text for the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanAdvice {
    pub kind: String,
    pub text: String,
}

/// A covering receipt in a plan: `fresh` and the command that ran, always; the run's operation id
/// when it has one; the note and the result the call would be answered with only when fresh (the
/// result only where a call can be answered).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanReceipt {
    pub fresh: bool,
    pub covering: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
}

/// A call answered by a receipt: the result the model reads, the receipt block, the run that
/// covered it, and the card the mod draws (`null` when there is none).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptAnswer {
    pub result: Option<String>,
    pub note: String,
    pub covering: String,
    pub card: Option<ReceiptCard>,
}

/// The bytes the binary prints for an answer: one JSON object, its keys sorted.
pub fn answer(a: &ModAnswer) -> String {
    // Through a `Value` (a sorted map): the bytes are those the answers had before they were typed.
    serde_json::to_value(a).map(|v| v.to_string()).unwrap_or_default()
}
