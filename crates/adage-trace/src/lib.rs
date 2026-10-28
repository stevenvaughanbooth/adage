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
mod verification;
pub mod wire;
pub mod world;

pub use harness::{Capabilities, EventKind, Harness};
pub use identity::{EvidenceGroup, EvidenceIdentity, GroupBasis};
pub use session::{Directed, Ending, OutcomeBasis, OutcomeEvidence, PromptObs, Session, StepObs};
pub use world::{
    Action, ActivityEvent, Artifact, ArtifactKind, CalendarBlock, Injection, InjectionAt, InjectionClass, WorldView,
};

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

/// How a harness marks a call it refused before running it, at the start of the call's result:
/// Claude Code's `<tool_use_error>` (a permission rule denied it, or its input was invalid). Such
/// a call reaches no failure hook, only the batch after it.
pub const TOOL_USE_ERROR: &str = "<tool_use_error>";

/// Whether a call's result says the harness refused it before it ran ([`TOOL_USE_ERROR`]).
pub fn is_tool_use_error(output: &str) -> bool {
    output.trim_start().starts_with(TOOL_USE_ERROR)
}

impl Call {
    /// The harness refused the call before it ran: it failed, and its result is the harness's
    /// refusal ([`is_tool_use_error`]), not the tool's output. Nothing ran, so a refused call
    /// follows no directive and meets no obligation, and one Adage made is not its step taken.
    pub fn refused(&self) -> bool {
        self.failed && is_tool_use_error(&self.output)
    }
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

    /// A call the harness refused before it ran is a failed call whose result is the refusal; a
    /// failed run, or a result that only mentions the marker, is not one.
    #[test]
    fn a_refused_call_is_a_failure_whose_result_is_the_refusal() {
        let call = |failed: bool, output: &str| Call {
            tool: "Read".into(),
            input: Value::Null,
            output: output.into(),
            failed,
            by: By::Adage,
            tool_use_id: None,
            agent: None,
        };
        let denied =
            "<tool_use_error>File is in a directory that is denied by your permission settings.</tool_use_error>";
        assert!(call(true, denied).refused());
        assert!(call(true, &format!("\n {denied}")).refused());
        assert!(!call(false, denied).refused(), "only a failed call");
        assert!(!call(true, "make: *** [gen] Error 1").refused(), "a run that failed is not a refusal");
        assert!(!call(true, &format!("grep found {denied}")).refused(), "the marker leads the result");
    }
}
