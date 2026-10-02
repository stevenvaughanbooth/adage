//! Repository identity for a trace: which repository, and which checkout root, a recorded working
//! directory belongs to. This is the one place a decoder may need the local machine, so it is a
//! port: [`LocalCheckout`] probes the checkout on this machine (what `adage condition` and the
//! live learner use); [`AsRecorded`] trusts the trace alone, for traces from somewhere else (a
//! team's trace store, an export from another machine, tests) where the checkout is not here.

use adage_core::repo;
use std::path::{Path, PathBuf};

/// The repository a trace was recorded in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    /// The checkout root that repository-relative paths are relative to.
    pub root: PathBuf,
    /// The repository key (`adage_core::repo::repo_key`), or the explicit override.
    pub repo: String,
}

/// Finds the repository a recorded working directory belongs to.
pub trait Locator: Sync {
    /// `explicit` (`--repo`) names the repository outright; the root is still found.
    fn locate(&self, cwd: &Path, explicit: Option<&str>) -> Located;
}

/// The checkout on this machine: a removed Claude Code agent worktree keeps its repository
/// ([`repo::removed_worktree`]), else the nearest directory holding `.git`, else the directory
/// itself; the key from its `origin` or root commit ([`repo::key_for`]).
#[derive(Debug, Clone, Copy, Default)]
pub struct LocalCheckout;

impl Locator for LocalCheckout {
    fn locate(&self, cwd: &Path, explicit: Option<&str>) -> Located {
        let root = match worktree_root(cwd) {
            // Paths are relative to the worktree the agent worked in; its key is its repository's
            // (`repo::repo_key`).
            Some(worktree) => worktree,
            None => repo::find_root(cwd).unwrap_or_else(|| cwd.to_path_buf()),
        };
        let repo = repo::key_for(&root, explicit);
        Located { root, repo }
    }
}

/// The trace alone, with no access to this machine: the recorded directory is the root, and the
/// repository is the explicit key, else the directory's name (`adage_core::repo::dir_name`).
/// Decoding through it reads nothing but the trace.
#[derive(Debug, Clone, Copy, Default)]
pub struct AsRecorded;

impl Locator for AsRecorded {
    fn locate(&self, cwd: &Path, explicit: Option<&str>) -> Located {
        let root = repo::lexical_normalise(cwd);
        let repo = repo::key_with(explicit, || repo::dir_name(&root));
        Located { root, repo }
    }
}

/// The root of a Claude Code agent worktree that no longer exists (`<repo>/.claude/worktrees/<name>`,
/// [`repo::removed_worktree`]), which the transcript's paths are under; `None` otherwise (a
/// worktree that still exists is found by [`repo::find_root`]).
fn worktree_root(cwd: &Path) -> Option<PathBuf> {
    let main = repo::removed_worktree(cwd)?;
    let rest = cwd.strip_prefix(main.join(".claude/worktrees")).ok()?;
    let name = rest.components().next()?;
    Some(main.join(".claude/worktrees").join(name))
}
