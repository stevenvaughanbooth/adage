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

/// Serializable so that a parse can be cached (`adage_ingest::cache`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
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
}

/// `gauge.harness` names, per session source.
pub const CLAUDE_CODE: &str = crate::Harness::ClaudeCode.id();
pub const OPENCODE: &str = crate::Harness::OpenCode.id();
pub const CODEX: &str = crate::Harness::Codex.id();

/// A directive a session received (Track G §3.2, steering by influence): delivered before step
/// `at_step`, it named these commands, avoided runners and files
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
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub all: bool,
}

/// The entry in [`Session::directed`] for a finish-line stop request.
pub const FINISH_LINE_STOP: &str = "finish-line-stop";

impl Session {
    /// The session's outcome as its transcript suggests it, for when none was recorded: failed if
    /// the agent ended by asking the person for help or was cut off, or if its last check failed;
    /// succeeded if its last check passed; unknown otherwise (no check ran).
    pub fn proxy_outcome(&self) -> Option<bool> {
        if self.ending.asked_for_help || self.ending.cut_off {
            return Some(false);
        }
        let last_check = self.steps.iter().rev().find(|s| {
            s.norm.effect == adage_core::Effect::Verify
                && matches!(s.norm.action, adage_core::CanonicalAction::Exec { .. })
        })?;
        Some(!last_check.call.failed)
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
