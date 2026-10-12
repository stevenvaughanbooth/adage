//! A batch of local traces read for one learn run: through the parse cache when there is one,
//! scoped to one repository when asked. What day-0 conditioning and the live learner both hand to
//! the learning engine.

use crate::cache;
use crate::format::{read_trace, Decode};
use crate::locate::{LocalCheckout, Locator};
use adage_core::CommandTable;
use adage_trace::Session;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// How to read a batch.
#[derive(Clone)]
pub struct Options {
    /// `--repo <key>`: the repository key to use instead of the derived one.
    pub repo_override: Option<String>,
    /// Read only this repository's sessions: others are neither loaded nor counted.
    pub only_repo: Option<String>,
    /// The parse cache directory (`$ADAGE_HOME/cache/sessions`); `None` parses every trace.
    pub cache: Option<PathBuf>,
    /// Which repository each trace belongs to: [`LocalCheckout`] for this machine's history,
    /// [`crate::AsRecorded`] for traces recorded elsewhere (and for tests, which must not depend
    /// on what is checked out where they run).
    pub locator: &'static dyn Locator,
    /// Told after each trace is read: how many of how many (for a person watching a long run).
    pub progress: Option<std::sync::Arc<dyn Fn(usize, usize) + Send + Sync>>,
}

impl Default for Options {
    fn default() -> Self {
        Options { repo_override: None, only_repo: None, cache: None, locator: &LocalCheckout, progress: None }
    }
}

impl std::fmt::Debug for Options {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Options")
            .field("repo_override", &self.repo_override)
            .field("only_repo", &self.only_repo)
            .field("cache", &self.cache)
            .field("locator", &self.locator.id())
            .field("progress", &self.progress.is_some())
            .finish()
    }
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
    let locator = opts.locator;
    let mut cache = opts.cache.as_deref().map(|d| cache::Cache::open(d, locator));
    // Repository keys by root, found once per run.
    let mut keys: BTreeMap<PathBuf, String> = BTreeMap::new();
    let mut repo_of = |root: &Path| -> String {
        keys.entry(root.to_path_buf()).or_insert_with(|| locator.locate(root, None).repo).clone()
    };
    let (over, only) = (opts.repo_override.as_deref(), opts.only_repo.as_deref());
    let cx = Decode { table, locator, repo_override: over, subagent: false };
    for (i, p) in traces.iter().enumerate() {
        if let Some(f) = &opts.progress {
            f(i, traces.len());
        }
        let got = match cache.as_mut() {
            Some(c) => c.read(p, &cx, only, &mut repo_of),
            None => match read_trace(p, &cx) {
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
    if let Some(f) = &opts.progress {
        f(traces.len(), traces.len());
    }
    // Which repository each checkout is, for when it is deleted (a removed worktree). Not under an
    // override, which is not the checkout's own key.
    if over.is_none() {
        let checkouts: BTreeMap<&Path, &str> =
            batch.sessions.iter().map(|s| (s.root.as_path(), s.repo.as_str())).collect();
        for (root, repo) in checkouts {
            locator.remember(root, repo);
        }
    }
    if let Some(c) = cache {
        (batch.cache_hits, batch.cache_misses) = (c.hits, c.misses);
        c.save();
    }
    batch
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AsRecorded;
    use serde_json::json;

    /// One transcript recorded in a checkout whose `origin` names its repository.
    fn transcript_in_checkout(d: &Path) -> PathBuf {
        let repo = d.join("work/app");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::write(repo.join(".git/config"), "[remote \"origin\"]\n\turl = git@github.com:acme/app.git\n").unwrap();
        let cwd = repo.to_str().unwrap();
        let lines = [
            json!({"type": "user", "sessionId": "s1", "cwd": cwd, "message": {"role": "user", "content": "run the tests"}}),
            json!({"type": "assistant", "sessionId": "s1", "cwd": cwd, "message": {"id": "m1", "content": [{"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "make test"}}]}}),
        ];
        let path = d.join("s1.jsonl");
        std::fs::write(&path, lines.iter().map(|l| format!("{l}\n")).collect::<String>()).unwrap();
        path
    }

    /// The locator decides the repository, through the parse cache too: a parse cached under this
    /// machine's checkout is never served to a run that reads traces as recorded, or back (a cache
    /// directory holds one locator's parses; switching re-parses).
    #[test]
    fn the_locator_decides_the_repository_and_the_cache_keeps_them_apart() {
        let d = tempfile::tempdir().unwrap();
        let path = transcript_in_checkout(d.path());
        let cache = Some(d.path().join("cache"));
        let local = Options { cache: cache.clone(), ..Default::default() };
        let recorded = Options { cache, locator: &AsRecorded, ..Default::default() };
        let repo = |o: &Options| {
            let b = read(std::slice::from_ref(&path), o);
            assert_eq!(b.sessions.len(), 1, "{o:?}");
            (b.sessions[0].repo.clone(), b.cache_hits)
        };
        assert_eq!(repo(&local), ("github.com/acme/app".into(), 0), "this machine's checkout names it");
        assert_eq!(repo(&local), ("github.com/acme/app".into(), 1), "and its parse is cached");
        assert_eq!(repo(&recorded), ("app".into(), 0), "as recorded: the directory's name, not the cached parse");
        assert_eq!(repo(&recorded), ("app".into(), 1), "and its own parse is cached");
        assert_eq!(repo(&local), ("github.com/acme/app".into(), 0), "never the other locator's");
    }
}
