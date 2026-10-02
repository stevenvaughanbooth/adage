//! Where a repository's learned artifacts live under the tracks root: its directory
//! ([`slug`]) and the files learning keeps beside its tracks. Read by the live hook and written by
//! the learner; both go through these paths, so neither has to link the other.

use std::path::{Path, PathBuf};

/// The track instincts are bound into, in each repository's directory.
pub const INSTINCTS_TRACK: &str = "instincts";

/// The directory name a repository's tracks are stored under.
pub fn slug(repo: &str) -> String {
    let old: String =
        repo.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
    // Preserve published track paths. Repository identities remain unchanged in Scope, but the
    // directory name must fit track validation, including the longest suffix this learner emits.
    if crate::io::safe_name(&format!("{old}/{}", INSTINCTS_TRACK)) {
        return old;
    }
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(repo.as_bytes());
    let hash: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
    let readable: String = old.to_ascii_lowercase().chars().take(64).collect();
    let readable = readable.trim_matches(['-', '_']);
    let readable = if readable.is_empty() { "unnamed" } else { readable };
    format!("repo-{readable}-{hash}")
}

/// Where a repository's learned follow-judge weights are kept (`adage_core::judge`), beside its
/// tracks (a hidden file: never a track). The hook hands them to the runtime.
pub fn follow_weights_path(tracks_root: &Path, repo: &str) -> PathBuf {
    tracks_root.join(slug(repo)).join(".follow-weights.json")
}

/// A repository's learned follow-judge weights, if it has any.
pub fn read_follow_weights(tracks_root: &Path, repo: &str) -> Option<adage_core::judge::Weights> {
    std::fs::read(follow_weights_path(tracks_root, repo)).ok().and_then(|b| serde_json::from_slice(&b).ok())
}
