//! One hook event's context ([`HookCtx`]): what it is about, and what it reads at most once, on
//! first use. Before it, a `PreToolUse` dry run read the index up to three times and the
//! configuration up to three times, and `respond` read the configuration twice.
//!
//! Every lazy read happens where its first use is, never earlier: nothing here reads anything
//! before `pretool`'s word scan (`names_python`) or before `respond`'s fast path (`fast::skip`)
//! that those paths did not read there already.

use crate::harness::Adapter;
use crate::site::ArmInputs;
use crate::Options;
use adage_core::repo;
use adage_store::config::Config;
use adage_store::Paths;
use adage_tracks::Index;
use serde_json::Value;
use std::cell::OnceCell;
use std::path::Path;
use std::time::Instant;

/// One hook event: its harness and native event, its payload, its session and where it runs, and
/// the reads it shares between the steps that handle it.
pub(crate) struct HookCtx<'a> {
    pub(crate) adapter: &'static dyn Adapter,
    /// The harness's hook name as the binary was called with it (`adage hook <harness> …`).
    pub(crate) harness: &'a str,
    /// The native event name.
    pub(crate) event: &'a str,
    pub(crate) p: &'a Value,
    pub(crate) session: &'a str,
    pub(crate) cwd: &'a Path,
    /// The repository root `cwd` is in, or `cwd` itself outside a repository.
    pub(crate) root: &'a Path,
    /// `root` is a repository (it has `.git`), not merely the session's working directory.
    pub(crate) in_repo: bool,
    pub(crate) paths: &'a Paths,
    /// When the hook began: the harness's timeout counts from then, and every record carries the
    /// time since (`duration_us`).
    pub(crate) started: Instant,
    pub(crate) opts: Options,
    /// What the event reads at most once ([`HookCtx::config`], [`HookCtx::index`],
    /// [`HookCtx::repo_key`], [`HookCtx::arms`]): `Reads::default()`, nothing read yet.
    pub(crate) reads: Reads,
}

/// The reads one event shares, each made on first use.
#[derive(Default)]
pub(crate) struct Reads {
    config: OnceCell<Config>,
    index: OnceCell<(bool, Index)>,
    repo_key: OnceCell<String>,
    arms: OnceCell<ArmInputs>,
}

impl HookCtx<'_> {
    /// The person's configuration (`config.toml`), read once.
    pub(crate) fn config(&self) -> &Config {
        self.reads.config.get_or_init(|| adage_store::config::load(self.paths).config)
    }

    /// The index, read once. A damaged index (a truncated write, a disk error) is served as empty
    /// and rebuilt: the session start asks the learner to recompile it (docs/zero-config.md §5).
    pub(crate) fn index(&self) -> &Index {
        &self.loaded().1
    }

    /// Whether the index on disk was damaged ([`HookCtx::index`]).
    pub(crate) fn index_damaged(&self) -> bool {
        self.loaded().0
    }

    fn loaded(&self) -> &(bool, Index) {
        self.reads.index.get_or_init(|| {
            let loaded = Index::try_load(&self.paths.index());
            let damaged = matches!(loaded, Err(adage_tracks::index::Unloaded::Damaged));
            (damaged, loaded.unwrap_or_default())
        })
    }

    /// The key this checkout's repository goes by (its remote, else its path, through the cache of
    /// keys), read once. Not the key the index routes it by: that is `site::routed_key`.
    pub(crate) fn repo_key(&self) -> &str {
        self.reads.repo_key.get_or_init(|| repo::repo_key_cached(self.root, &self.paths.repo_keys()))
    }

    /// What the session's arm is drawn with, from the configuration read once.
    pub(crate) fn arms(&self) -> &ArmInputs {
        self.reads.arms.get_or_init(|| ArmInputs::with_share(self.config().runtime_control))
    }
}
