//! A batch of local traces read for one learn run: through the parse cache when there is one,
//! scoped to one repository when asked. What day-0 conditioning and the live learner both hand to
//! the learning engine.

use crate::cache;
use crate::format::read_session;
use adage_core::CommandTable;
use adage_trace::Session;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// How to read a batch.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// `--repo <key>`: the repository key to use instead of the derived one.
    pub repo_override: Option<String>,
    /// Read only this repository's sessions: others are neither loaded nor counted.
    pub only_repo: Option<String>,
    /// The parse cache directory (`$ADAGE_HOME/cache/sessions`); `None` parses every trace.
    pub cache: Option<PathBuf>,
}

/// The sessions of a batch, and how they were read.
#[derive(Debug, Default)]
pub struct Batch {
    /// Sessions with at least one step, in the order their traces were given.
    pub sessions: Vec<Session>,
    /// Traces read: every one with a session of the repository asked for (or of any, in a full
    /// run), and in a full run those with none.
    pub read: usize,
    /// Of those, the sessions that received a directive.
    pub steered: usize,
    pub cache_hits: usize,
    pub cache_misses: usize,
}

/// Read `traces` into canonical sessions.
pub fn read(traces: &[PathBuf], opts: &Options) -> Batch {
    let table = CommandTable::load();
    let mut batch = Batch::default();
    let mut cache = opts.cache.as_deref().map(cache::Cache::open);
    // Repository keys by root, found once per run.
    let mut keys: BTreeMap<PathBuf, String> = BTreeMap::new();
    let mut repo_of = |root: &Path| -> String {
        keys.entry(root.to_path_buf()).or_insert_with(|| adage_core::repo::repo_key(root)).clone()
    };
    let (over, only) = (opts.repo_override.as_deref(), opts.only_repo.as_deref());
    for p in traces {
        let got = match cache.as_mut() {
            Some(c) => c.read(p, &table, over, only, &mut repo_of),
            None => match read_session(p, &table, over) {
                None => cache::Read::Nothing,
                Some(s) if only.is_some_and(|r| r != s.repo) => cache::Read::Elsewhere,
                Some(s) => cache::Read::Session(Box::new(s)),
            },
        };
        match got {
            cache::Read::Elsewhere => {}
            // A file with no session belongs to no repository: counted only in a full run.
            cache::Read::Nothing => batch.read += usize::from(only.is_none()),
            cache::Read::Session(s) => {
                batch.read += 1;
                batch.steered += usize::from(s.steered);
                if !s.steps.is_empty() {
                    batch.sessions.push(*s);
                }
            }
        }
    }
    // Which repository each checkout is, for when it is deleted (a removed worktree). Not under an
    // override, which is not the checkout's own key.
    if over.is_none() {
        let checkouts: BTreeMap<&Path, &str> =
            batch.sessions.iter().map(|s| (s.root.as_path(), s.repo.as_str())).collect();
        for (root, repo) in checkouts {
            adage_core::repo::remember_key(root, repo);
        }
    }
    if let Some(c) = cache {
        (batch.cache_hits, batch.cache_misses) = (c.hits, c.misses);
        c.save();
    }
    batch
}
