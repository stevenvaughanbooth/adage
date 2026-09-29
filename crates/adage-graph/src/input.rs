//! The graph engine's own session input (Track G §1.3): what value-flow induction needs from a
//! session, independent of any importer. `adage-learn/src/bridge.rs` converts its `Session` into
//! this ([`Annotated`] adds per-step metadata for session facts, G0).

use serde::{Deserialize, Serialize};

/// One session: its prompts and its ordered tool steps.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    /// A stable, content-derived id. Every reduction folds sessions in id order.
    pub id: String,
    pub prompts: Vec<Prompt>,
    pub steps: Vec<Step>,
}

/// A prompt the person typed, delivered before step `at_step`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prompt {
    pub text: String,
    pub at_step: u32,
}

/// One tool step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "lowercase")]
pub enum Step {
    /// A command: its literal argv (after shell parsing) and its output.
    Exec { argv: Vec<String>, output: String, failed: bool },
    /// A repository-relative path read (or listed, or searched).
    Read { path: String },
    /// A repository-relative path edited or written.
    Edit { path: String },
}

/// What an importer knows about one step beyond its [`Step`] (Track G §1.1, G0): the model turn
/// that issued it (calls of one turn form a batch), the prompt it served, its effect class and
/// its canonical template key. Kept beside a [`Session`] rather than inside it
/// ([`Annotated`]), so every existing `Session` stays as it is; each field defaults to empty.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepMeta {
    /// Model turn index: steps with the same turn were issued together (one batch).
    #[serde(default)]
    pub turn: u32,
    /// Index into [`Session::prompts`] of the prompt the step served.
    #[serde(default)]
    pub prompt: u16,
    /// Effect class (`observe`, `verify`, `regenerate`, `environment`, `author`, `publish`);
    /// empty when unknown.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub effect: String,
    /// Canonical template key (`exec make gen`, `read {path}`); empty when unknown.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub key: String,
}

/// A session with its per-step metadata (`meta[i]` describes `session.steps[i]`; shorter or
/// empty when unknown) and its calendar day.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Annotated {
    pub session: Session,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub meta: Vec<StepMeta>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub day: String,
}

impl Annotated {
    /// A session with no metadata.
    pub fn bare(session: Session) -> Annotated {
        Annotated { session, meta: Vec::new(), day: String::new() }
    }
}

/// Bounds on one session (Track G §1.3, S2): later steps and longer outputs are ignored.
pub const MAX_STEPS: usize = 2_000;
pub const MAX_OUTPUT: usize = 64 * 1024;
/// At most this many tokens are read from one output or prompt.
pub const MAX_SOURCE_TOKENS: usize = 512;
