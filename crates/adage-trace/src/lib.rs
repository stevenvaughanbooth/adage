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
pub mod session;
pub mod wire;

pub use harness::{Capabilities, Harness};
pub use session::{Directed, Ending, PromptObs, Session, StepObs};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Who made a tool call: the agent (the model's own request), or Adage, which made the call the
/// agent would have made in its place (a predicted step, `adage_runtime`'s `TurnStep`). A call
/// Adage made is never organic evidence of what the agent does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum By {
    #[default]
    Agent,
    Adage,
}

impl By {
    pub fn is_agent(&self) -> bool {
        *self == By::Agent
    }

    /// Who made a call with this harness tool-use id: Adage mints `adage_<route>_<turn>_<index>`.
    pub fn of_tool_use_id(id: &str) -> By {
        if id.starts_with("adage_") {
            By::Adage
        } else {
            By::Agent
        }
    }
}

/// One tool call, in the canonical tool vocabulary (`adage_core::tools`): the tool, its input, its
/// output as the agent read it, and whether it failed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Call {
    pub tool: String,
    pub input: Value,
    pub output: String,
    pub failed: bool,
    /// Who made it ([`By`]). Omitted when it is the agent, so traces written before it read and
    /// write the same.
    #[serde(default, skip_serializing_if = "By::is_agent")]
    pub by: By,
    /// The harness's own id for the call, where its payload carries one (Claude Code's
    /// `tool_use_id`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    /// The subagent that made it (Claude Code's `agent_id` on a subagent's tool events), `None`
    /// for the main loop: a delegation's calls can then be counted on their own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
}

/// One live event, as every harness adapter translates its native hook payload.
#[derive(Debug, Clone)]
pub enum Event {
    SessionStart,
    Prompt {
        text: String,
    },
    Batch {
        calls: Vec<Call>,
    },
    Failure {
        call: Call,
    },
    Stop {
        stop_hook_active: bool,
    },
    SessionEnd,
    /// The harness is about to send one request to the model (Claude Code with the mod's
    /// `turn.step`): the point where Adage may make the call the agent would have made. `index` is
    /// the step's position in the turn (0 is the prompt's own request); `ending` is set when the
    /// model's response just ended the turn (the finish line at the last step).
    TurnStep {
        turn_id: String,
        index: u32,
        model: Option<String>,
        agent: Option<String>,
        ending: bool,
    },
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
            Event::TurnStep { .. } => "turn_step",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T-C9-01 (the call): a call written before `agent` existed reads as the main loop's, and a
    /// main-loop call writes the same bytes as before; a subagent's call keeps its agent.
    #[test]
    fn a_calls_agent_defaults_to_the_main_loop_and_is_omitted_there() {
        let before = r#"{"tool":"Bash","input":{"command":"ls"},"output":"","failed":false}"#;
        let c: Call = serde_json::from_str(before).unwrap();
        assert_eq!((c.agent.as_deref(), c.by), (None, By::Agent));
        assert_eq!(serde_json::to_string(&c).unwrap(), before);
        let sub = Call { agent: Some("explore-1".into()), ..c };
        let text = serde_json::to_string(&sub).unwrap();
        assert!(text.ends_with(r#","agent":"explore-1"}"#), "{text}");
        assert_eq!(serde_json::from_str::<Call>(&text).unwrap(), sub);
    }
}
