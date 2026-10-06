//! The canonical trace model (docs/architecture.md, layer 1).
//!
//! Every harness speaks its own format: Claude Code transcripts and hook payloads, Codex rollouts
//! and hooks, OpenCode exports and plugin events. Adage translates each of them, once, into the
//! types here, and every component past that translation works on these types only:
//!
//! - [`Call`]: one tool call as the agent made it, with its output and whether it failed.
//! - [`Event`]: one live hook event (the live interaction's input).
//! - [`Session`]: one whole recorded session (ingestion's output, the learning engine's input).
//! - [`wire`]: how Adage's own text reads inside a trace.
//!
//! The crate depends on `adage-core` alone, and does no I/O.

pub mod harness;
pub mod identity;
pub mod session;
pub mod wire;

pub use harness::{Capabilities, Harness};
pub use identity::{EvidenceGroup, EvidenceIdentity, GroupBasis};
pub use session::{Directed, Ending, OutcomeBasis, OutcomeEvidence, PromptObs, Session, StepObs};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One tool call, in the canonical tool vocabulary (`adage_core::tools`): the tool, its input, its
/// output as the agent read it, and whether it failed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Call {
    pub tool: String,
    pub input: Value,
    pub output: String,
    pub failed: bool,
}

/// One live event, as every harness adapter translates its native hook payload.
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
