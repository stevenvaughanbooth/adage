//! The graph engine's own session input (Track G §1.3): what value-flow induction needs from a
//! session, independent of any importer. `adage-learn` will convert its `Session` into this (G1
//! integration: `adage-learn/src/bridge.rs`); `adage eval types` has its own adapter meanwhile.

use serde::{Deserialize, Serialize};

/// One session: its prompts and its ordered tool steps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// Bounds on one session (Track G §1.3, S2): later steps and longer outputs are ignored.
pub const MAX_STEPS: usize = 2_000;
pub const MAX_OUTPUT: usize = 64 * 1024;
/// At most this many tokens are read from one output or prompt.
pub const MAX_SOURCE_TOKENS: usize = 512;
