//! The canonical session: what every trace decoder produces from a harness's native history, and
//! what the learning engine, the evals and `adage sense` consume. Nothing here knows a harness's
//! format or where a trace was read from.

use crate::Call;
use adage_core::keys;
use adage_core::Normalized;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepObs {
    /// Model turn index (0-based), counting assistant messages that issued tool calls.
    pub turn: u32,
    pub key: String,
    pub norm: Normalized,
    pub call: Call,
    /// Repository-relative path read or written, if any.
    pub rel_path: Option<String>,
    /// Index into [`Session::prompts`] of the prompt this step served: the latest prompt typed
    /// before its tool call (0 for a call before any prompt). Saturates at `u16::MAX`.
    #[serde(default)]
    pub prompt: u16,
    /// Native per-tool cwd was the repository root, with no prior unknown shell/cwd change.
    /// Only used to admit new direct-pytest verbosity pooling; old/cache/other-harness data fail closed.
    #[serde(default)]
    pub project_pytest_root_cwd: bool,
}

impl StepObs {
    /// Whether two normalized steps retain the same native invocation and result. Ingestion
    /// projects `Call::failed` onto individual shell subactions, so that derived flag is not
    /// invocation identity. Every other original call field, the prompt and the turn must agree.
    /// Consumers must still require adjacency and reject a reused id outside that one group.
    pub fn same_native_call(&self, other: &Self) -> bool {
        self.call.tool_use_id.as_deref().is_some_and(|id| !id.is_empty() && id.len() <= 256)
            && self.turn == other.turn
            && self.prompt == other.prompt
            && self.call.tool_use_id == other.call.tool_use_id
            && self.call.tool == other.call.tool
            && self.call.input == other.call.input
            && self.call.output == other.call.output
            && self.call.by == other.call.by
            && self.call.agent == other.call.agent
    }
}

/// One prompt the person typed, and where it falls among the session's steps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptObs {
    /// The prompt's text, at most [`MAX_PROMPT_BYTES`] (cut on a character boundary).
    pub text: String,
    /// The index of the first step that served this prompt or a later one: the prompt was
    /// delivered before `steps[at_step]` (`steps.len()` when no step followed it).
    pub at_step: u32,
}

/// A stored prompt's text is cut to this many bytes (what the shared tokeniser reads).
pub const MAX_PROMPT_BYTES: usize = 64 * 1024;
/// At most this many prompts are kept per session.
pub const MAX_PROMPTS: usize = u16::MAX as usize;

/// Native delegation context, retained as observations, never user authority or a task grade.
/// A shared parent is a leakage family, not evidence that its workers are independent trials.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delegation {
    pub parent_id: String,
    pub agent_id: String,
    /// The harness's computed assignment with its framing removed. The original prompt remains
    /// available in `Session::prompts`; this text must never be treated as human consent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    /// The worker's own recorded role instructions, not inherited tool output or a verdict.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

/// Serializable so that a parse can be cached (`adage_ingest::cache`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegation: Option<Delegation>,
    /// Task, leakage group and attempt supplied by a corpus or dataset manifest.
    #[serde(default, skip_serializing_if = "crate::EvidenceIdentity::is_empty")]
    pub identity: crate::EvidenceIdentity,
    pub root: PathBuf,
    pub repo: String,
    pub model: Option<String>,
    pub day: String,
    pub prompt: String,
    pub steps: Vec<StepObs>,
    pub steered: bool,
    /// Adage routes whose directives this session received.
    pub directed: BTreeSet<String>,
    /// Every directive the session received: where, and what it named ([`Directed`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub influence: Vec<Directed>,
    pub outcome: Option<bool>,
    /// The harness the session ran in, as recorded in a track's `gauge.harness`: `claude-code`
    /// for Claude Code transcripts, `opencode` for `opencode export` files.
    pub harness: String,
    /// Prompts the person typed in the session, in order, each anchored to the steps (Track G
    /// §1.1). The first is also [`Session::prompt`].
    #[serde(default)]
    pub prompts: Vec<PromptObs>,
    /// How the session ended, as far as the transcript shows (outcome proxies,
    /// [`Session::proxy_outcome`]).
    #[serde(default)]
    pub ending: Ending,
}

/// How a session ended, read from its transcript: the evidence outcome proxies use when no
/// outcome was recorded.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ending {
    /// The agent's last message asks the person for help or for what to do (`Could you tell me
    /// what W-1043 involves?`) instead of reporting work done.
    #[serde(default)]
    pub asked_for_help: bool,
    /// The transcript ends on a tool call or its result with no final reply: the session was cut
    /// off (a turn limit, a kill).
    #[serde(default)]
    pub cut_off: bool,
    /// The harness accepted a structured handback. This is a completed report, including a
    /// report of blocked or failed work, not evidence of task success or failure.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub structured_handback: bool,
}

/// `gauge.harness` names, per session source.
pub const CLAUDE_CODE: &str = crate::Harness::ClaudeCode.id();
pub const OPENCODE: &str = crate::Harness::OpenCode.id();
pub const CODEX: &str = crate::Harness::Codex.id();

/// A directive a session received (Track G §3.2, steering by influence): delivered before step
/// `at_step`, it named these commands, avoided runners, files and skills
/// ([`crate::wire::named`]). `all`: it named nothing readable (a wrapper cut short, a
/// data block without its directive, a template the reader does not know) or Adage ran it itself
/// (an `exec` attribute: from there on the agent's actions are not its own), so it counts as
/// naming everything from there on.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Directed {
    pub at_step: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub route: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub avoided: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    /// Skills it said to use (a `skill` directive): the session's use of one of them is steered.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub all: bool,
}

/// The entry in [`Session::directed`] for a finish-line stop request.
pub const FINISH_LINE_STOP: &str = "finish-line-stop";

impl Session {
    /// Meaning of the delegated assignment for learning, without the harness's repeated wrapper.
    /// This changes neither instruction authority nor the source-assigned evidence group.
    pub fn learning_prompt(&self) -> &str {
        self.delegation.as_ref().and_then(|d| d.task.as_deref()).unwrap_or(&self.prompt)
    }
    /// The session's outcome as its transcript suggests it, for when none was recorded: failed if
    /// the agent ended by asking the person for help or was cut off, or if its last check failed;
    /// a check only applies if it followed the final mutation in a later model turn and served
    /// the latest prompt. This is a completion proxy, never a substitute for a task evaluator.
    pub fn proxy_outcome(&self) -> Option<bool> {
        if self.ending.asked_for_help || self.ending.cut_off {
            return Some(false);
        }
        if self.ending.structured_handback {
            return None;
        }
        let last_check = self.steps.iter().rev().find(|s| {
            s.norm.effect == adage_core::Effect::Verify
                && matches!(s.norm.action, adage_core::CanonicalAction::Exec { .. })
        })?;
        if self.steps.iter().any(|s| {
            (s.norm.mutating
                || matches!(
                    s.norm.effect,
                    adage_core::Effect::Author
                        | adage_core::Effect::Regenerate
                        | adage_core::Effect::Environment
                        | adage_core::Effect::Publish
                ))
                && s.turn >= last_check.turn
        }) || usize::from(last_check.prompt) != self.prompts.len().saturating_sub(1)
        {
            return None;
        }
        last_check.verification().map_or(Some(!last_check.call.failed), |r| r.status())
    }

    /// Preserve whether an outcome was recorded or merely inferred from trace completion.
    pub fn outcome_evidence(&self) -> OutcomeEvidence {
        if let Some(value) = self.outcome {
            return OutcomeEvidence { value: Some(value), basis: OutcomeBasis::Recorded };
        }
        let value = self.proxy_outcome();
        let basis = if self.ending.asked_for_help || self.ending.cut_off {
            OutcomeBasis::IncompleteEnding
        } else if value.is_some() {
            OutcomeBasis::FinalCheck
        } else {
            OutcomeBasis::Unknown
        };
        OutcomeEvidence { value, basis }
    }

    /// How many prompts the person typed.
    pub fn prompt_count(&self) -> u32 {
        self.prompts.len() as u32
    }

    /// Index of the first step that edits or writes a file.
    pub fn first_edit(&self) -> Option<usize> {
        self.steps.iter().position(|s| keys::is_author(&s.norm.action))
    }
    /// Steps before the first edit: the discovery window.
    pub fn discovery(&self) -> &[StepObs] {
        &self.steps[..self.first_edit().unwrap_or(self.steps.len())]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeBasis {
    Recorded,
    IncompleteEnding,
    FinalCheck,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutcomeEvidence {
    pub value: Option<bool>,
    pub basis: OutcomeBasis,
}
