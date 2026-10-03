//! The record of how a repository's instincts were bound: against which checkout, its declared
//! files' fingerprint, its verified environment, and the packs selected. The learner writes it when
//! it binds; the live hook reads it to decide whether to bind again. It lives beside the
//! repository's tracks (`adage_tracks::layout`).

use adage_tracks::layout::slug;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The packs a binding was made with, as a short fingerprint: the built-in marketplace this build
/// ships (`adage_pack::builtin::fingerprint`) and the choice of packs (`instincts.packs`,
/// `instincts.disabled`). Either changing binds each repository's instincts again. Cheap: the
/// session-start hook computes it from the settings it has read.
pub fn selection<A: AsRef<str>, B: AsRef<str>>(packs: &[A], disabled: impl IntoIterator<Item = B>) -> String {
    let mut on: Vec<&str> = packs.iter().map(AsRef::as_ref).collect();
    on.sort_unstable();
    on.dedup();
    let mut off: Vec<String> = disabled.into_iter().map(|d| d.as_ref().to_string()).collect();
    off.sort_unstable();
    off.dedup();
    adage_core::repo::short_hash(&format!("{}\n{}\n{}", crate::builtin::fingerprint(), on.join(","), off.join(",")))
}

/// Where the checkout a repository's instincts were bound against is recorded, beside its tracks
/// (a hidden file: never a track).
pub fn binding_path(tracks_root: &Path, repo: &str) -> PathBuf {
    tracks_root.join(slug(repo)).join(".instincts-binding.json")
}

/// What a binding was made from: the checkout and its declared files' fingerprint
/// (`adage_instincts::fingerprint`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub root: String,
    pub declared: String,
    /// The checkout's project environment, verified from its files at bind time
    /// (`adage_instincts::verify_env`): what a `PreToolUse` interpreter rewrite may use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<adage_instincts::VerifiedEnv>,
    /// The packs the instincts were bound with ([`selection`]): a build that ships other base
    /// packs, or another choice of packs, binds them again.
    #[serde(default)]
    pub packs: String,
}

/// The recorded binding of a repository's instincts, if any.
pub fn binding(tracks_root: &Path, repo: &str) -> Option<Binding> {
    std::fs::read(binding_path(tracks_root, repo)).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

/// Whether the repository's instincts should be bound again against the live checkout `root` (the
/// session's): never bound, bound against another checkout (paths named in old traces, another
/// worktree), its declared files changed since (a `.venv` made, a Makefile edited), or other packs
/// are selected now (`selection`: [`selection`] of the current settings).
pub fn stale_binding(tracks_root: &Path, repo: &str, root: &Path, selection: &str) -> bool {
    is_stale(binding(tracks_root, repo).as_ref(), root, selection)
}

/// [`stale_binding`], for the binding recorded (`None`: never bound).
pub fn is_stale(recorded: Option<&Binding>, root: &Path, selection: &str) -> bool {
    recorded.is_none_or(|b| {
        b.root != root.to_string_lossy() || b.declared != adage_instincts::fingerprint(root) || b.packs != selection
    })
}
