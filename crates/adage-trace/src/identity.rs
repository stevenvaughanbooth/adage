//! Evidence grouping is shared by calibration, qualification and corpus import.
//! A prompt fingerprint is a fallback, not proof that two differently worded tasks are independent.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Source-assigned identity. Retries retain their task/group and may have different attempt ids.
/// All ids are scoped to the session's repository; none authorizes execution.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceIdentity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    /// A broader leakage group (for example, paraphrases or variants of the same work item).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupBasis {
    DeclaredGroup,
    DeclaredTask,
    PromptFingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceGroup {
    /// A versioned, repository-scoped digest; raw prompts never appear in aggregate group ids.
    pub key: String,
    pub basis: GroupBasis,
}

impl EvidenceIdentity {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }

    pub fn group(&self, repo: &str, prompt: &str) -> Option<EvidenceGroup> {
        let nonempty = |s: &Option<String>| s.as_deref().is_some_and(|s| !s.trim().is_empty());
        let (basis, kind, value) = if nonempty(&self.group_id) {
            (GroupBasis::DeclaredGroup, "group", self.group_id.as_ref()?.trim().to_string())
        } else if nonempty(&self.task_id) {
            (GroupBasis::DeclaredTask, "task", self.task_id.as_ref()?.trim().to_string())
        } else {
            let text = prompt.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
            if text.is_empty() {
                return None;
            }
            (GroupBasis::PromptFingerprint, "prompt", text)
        };
        // Length framing prevents ambiguous concatenations and distinguishes declared/inferred ids.
        let mut hash = Sha256::new();
        for part in ["adage-evidence-group-v1", repo, kind, &value] {
            hash.update((part.len() as u64).to_le_bytes());
            hash.update(part.as_bytes());
        }
        Some(EvidenceGroup { key: format!("group-v1:{:x}", hash.finalize()), basis })
    }
}

impl crate::Session {
    /// The independent evidence group, or unknown. A session id is never a substitute for a task.
    pub fn evidence_group(&self) -> Option<EvidenceGroup> {
        self.identity.group(&self.repo, &self.prompt)
    }
}
