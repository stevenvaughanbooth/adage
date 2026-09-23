//! Canonical action vocabulary and effect classes — the normalisation target.
//!
//! Mirrors `research/poc/src/types.ts` for the action shapes, and adds the Design Spec §8.6
//! effect classes, which the PoC did not have (it only had `ExecClass` for exec commands).

use serde::Serialize;

/// Design Spec §8.6. Unknown commands default to `Environment`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    Observe,
    Verify,
    Regenerate,
    Environment,
    Author,
    Publish,
}

impl Effect {
    pub fn as_str(&self) -> &'static str {
        match self {
            Effect::Observe => "observe",
            Effect::Verify => "verify",
            Effect::Regenerate => "regenerate",
            Effect::Environment => "environment",
            Effect::Author => "author",
            Effect::Publish => "publish",
        }
    }

    pub fn parse(s: &str) -> Option<Effect> {
        match s {
            "observe" => Some(Effect::Observe),
            "verify" => Some(Effect::Verify),
            "regenerate" => Some(Effect::Regenerate),
            "environment" => Some(Effect::Environment),
            "author" => Some(Effect::Author),
            "publish" => Some(Effect::Publish),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum CanonicalAction {
    Read { path: String, range: Option<(i64, i64)> },
    Search { kind: String, pattern: String, paths: Vec<String> },
    List { path: String },
    Exec { argv: Vec<String>, cwd: String },
    Edit { path: String },
    Write { path: String },
    Fetch { url: String },
    Unknown { raw: String },
    /// A compound construct (loop/conditional/case/subshell/function) whose *body* we
    /// successfully parsed into sub-actions, listed in `actions`. Distinct from PoC's opaque
    /// "…compound…" placeholder: the brief asks us to "decide what a loop over reads should
    /// become" rather than treat compounds as opaque.
    Compound { kind: String, actions: Vec<Normalized> },
}

#[derive(Debug, Clone, Serialize)]
pub struct Normalized {
    pub action: CanonicalAction,
    pub effect: Effect,
    pub mutating: bool,
    pub confidence: f64,
    /// Free-text note on why confidence is < 1 (e.g. "unrecognised command", "loop body assumed
    /// to run", "unresolved branch"). Empty when confidence is 1.
    pub note: String,
}

impl Normalized {
    pub fn is_unknown(&self) -> bool {
        matches!(self.action, CanonicalAction::Unknown { .. })
    }

    /// True if this or any nested compound action is unknown.
    pub fn contains_unknown(&self) -> bool {
        match &self.action {
            CanonicalAction::Unknown { .. } => true,
            CanonicalAction::Compound { actions, .. } => actions.iter().any(|a| a.contains_unknown()),
            _ => false,
        }
    }

    pub fn low_confidence(&self) -> bool {
        self.confidence < 0.7
    }
}
