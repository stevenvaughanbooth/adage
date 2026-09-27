//! Eval datasets on disk: `manifest.json`, transcripts, and a sandbox file list.
//!
//! ```text
//! <dataset>/manifest.json   name, repo, learn thresholds, sessions (+ labels), named splits
//! <dataset>/sessions/*.jsonl Claude Code transcripts (the format `adage learn` reads)
//! <dataset>/sandbox.txt      the repository's files, one per line; `a -> b` is a symlink, and a
//!                            path starting `../` is created *outside* the repository root
//! <dataset>/conventions.json the sandbox's hidden conventions (ground truth for `eval routes`)
//! ```
//!
//! Replay needs the repository only for repository-path contracts (a bound file must exist and
//! resolve inside the root), so the sandbox is materialised as empty files in a temp directory.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Repository key the sessions belong to (passed to `adage learn --repo`).
    pub repo: String,
    /// Learn thresholds: `demo` or `default` (`adage learn` vs `adage learn --demo`).
    #[serde(default = "default_learn")]
    pub learn: String,
    #[serde(default)]
    pub sandbox: Option<String>,
    pub sessions: Vec<SessionMeta>,
    #[serde(default)]
    pub splits: BTreeMap<String, Split>,
    /// The split used when none is named (default `hash:0.3`).
    #[serde(default)]
    pub default_split: Option<String>,
    /// Strings that must never appear in a directive (injected canaries, adversarial sets).
    #[serde(default)]
    pub canaries: Vec<String>,
    /// The sandbox's hidden conventions, the ground truth for `adage eval routes`
    /// (`conventions.json`; see [`crate::conventions`]). Defaults to `conventions.json` when
    /// that file exists in the dataset.
    #[serde(default)]
    pub conventions: Option<String>,
}

fn default_learn() -> String {
    "default".into()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionMeta {
    pub file: String,
    pub id: String,
    #[serde(default)]
    pub run: String,
    #[serde(default)]
    pub phase: String,
    #[serde(default)]
    pub arm: String,
    #[serde(default)]
    pub round: Option<u32>,
    /// The task (work item) the session worked on. Repeats of one task share it; splits and the
    /// leakage check group by it. A session without one is a task of its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    /// How the session ended, where known: `error_max_turns` means the harness cut the agent off
    /// and no Stop hook fired.
    #[serde(default)]
    pub result: Option<String>,
    /// Hidden checks passed (task success), where known.
    #[serde(default)]
    pub passed: Option<bool>,
    /// The hidden check found generated code stale (a finish-line obligation was missed).
    #[serde(default)]
    pub stale_generated: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Split {
    pub train: Vec<String>,
    pub test: Vec<String>,
    /// What the split measures, and any caveat; shown in the report.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

pub struct Dataset {
    pub dir: PathBuf,
    pub manifest: Manifest,
}

impl SessionMeta {
    /// Did the session end with a Stop hook? Not when the harness cut the agent off.
    pub fn stopped(&self) -> bool {
        self.result.as_deref() != Some("error_max_turns")
    }

    /// The session's task: its `task` label, or its own id when it has none.
    pub fn task_key(&self) -> &str {
        self.task.as_deref().filter(|t| !t.is_empty()).unwrap_or(&self.id)
    }
}

impl Dataset {
    pub fn load(dir: &Path) -> Result<Dataset> {
        let p = dir.join("manifest.json");
        let manifest: Manifest =
            serde_json::from_str(&std::fs::read_to_string(&p).with_context(|| p.display().to_string())?)
                .with_context(|| p.display().to_string())?;
        for s in &manifest.sessions {
            if !super::safe_rel(&s.file) {
                bail!("session file {:?} is not a relative path inside the dataset", s.file);
            }
        }
        Ok(Dataset { dir: dir.to_path_buf(), manifest })
    }

    pub fn session(&self, id: &str) -> Option<&SessionMeta> {
        self.manifest.sessions.iter().find(|s| s.id == id)
    }

    pub fn path(&self, s: &SessionMeta) -> PathBuf {
        self.dir.join(&s.file)
    }

    /// Resolve a split name:
    ///
    /// - a named split from the manifest;
    /// - `hash:<share>[:<seed>]`: a session is tested when SHA-256(seed:session id) falls below
    ///   `share`. Repeats of one task can land on both sides;
    /// - `group:task:<k>:<fold>`: task-grouped k-fold. A task is in fold SHA-256(task) mod `k`,
    ///   and every session of the fold's tasks is tested; no task is on both sides;
    /// - `temporal[:<share>]`: sessions in time order (manifest `round`, then `phase` with
    ///   `train` first, then manifest order). The last `share` of tasks by first appearance
    ///   (default 0.3) are tested; training uses only sessions of other tasks that came before
    ///   the first test session;
    /// - `all`: train and test are every session (in-sample).
    pub fn split(&self, name: &str) -> Result<Split> {
        let ids: Vec<String> = self.manifest.sessions.iter().map(|s| s.id.clone()).collect();
        if name == "all" {
            let note = Some("in-sample: every session is in both sets".to_string());
            return Ok(Split { train: ids.clone(), test: ids, note });
        }
        if let Some(rest) = name.strip_prefix("hash:") {
            let (share, seed) = rest.split_once(':').unwrap_or((rest, "adage-eval"));
            let share = parse_share(name, share)?;
            let (mut train, mut test) = (vec![], vec![]);
            for id in ids {
                if in_test(&id, seed, share) {
                    test.push(id)
                } else {
                    train.push(id)
                }
            }
            return Ok(Split { train, test, note: None });
        }
        if let Some(rest) = name.strip_prefix("group:task:") {
            let (k, fold) = rest.split_once(':').with_context(|| format!("split {name:?}: group:task:<k>:<fold>"))?;
            let k: u64 = k.parse().with_context(|| format!("split {name:?}: k is not a number"))?;
            let fold: u64 = fold.parse().with_context(|| format!("split {name:?}: fold is not a number"))?;
            if k < 2 || fold >= k {
                bail!("split {name:?}: needs k >= 2 and fold < k");
            }
            let (mut train, mut test) = (vec![], vec![]);
            for s in &self.manifest.sessions {
                if task_fold(s.task_key(), k) == fold {
                    test.push(s.id.clone())
                } else {
                    train.push(s.id.clone())
                }
            }
            return Ok(Split { train, test, note: Some(format!("task-grouped {k}-fold, fold {fold}")) });
        }
        if name == "temporal" || name.starts_with("temporal:") {
            let share = match name.strip_prefix("temporal:") {
                Some(x) => parse_share(name, x)?,
                None => 0.3,
            };
            return Ok(self.temporal(share));
        }
        self.manifest.splits.get(name).cloned().with_context(|| {
            let names: Vec<&String> = self.manifest.splits.keys().collect();
            format!(
                "no split {name:?} (named splits: {names:?}; or hash:<share>, group:task:<k>:<fold>, temporal[:<share>], all)"
            )
        })
    }

    /// Sessions in time order: round, then phase (`train` first), then manifest order.
    fn chronological(&self) -> Vec<&SessionMeta> {
        let mut v: Vec<(usize, &SessionMeta)> = self.manifest.sessions.iter().enumerate().collect();
        v.sort_by_key(|(i, s)| (s.round.unwrap_or(0), u8::from(s.phase != "train"), *i));
        v.into_iter().map(|(_, s)| s).collect()
    }

    fn temporal(&self, share: f64) -> Split {
        let order = self.chronological();
        let mut tasks: Vec<&str> = Vec::new();
        for s in &order {
            if !tasks.contains(&s.task_key()) {
                tasks.push(s.task_key());
            }
        }
        let n = tasks.len();
        let n_test = if n < 2 { n } else { ((share * n as f64).round() as usize).clamp(1, n - 1) };
        let test_tasks = &tasks[n - n_test..];
        let cutoff = order.iter().position(|s| test_tasks.contains(&s.task_key())).unwrap_or(order.len());
        let (mut train, mut test) = (vec![], vec![]);
        for (i, s) in order.iter().enumerate() {
            if test_tasks.contains(&s.task_key()) {
                test.push(s.id.clone());
            } else if i < cutoff {
                train.push(s.id.clone());
            }
        }
        let unused = order.len() - train.len() - test.len();
        let note = format!(
            "temporal: the last {n_test} of {n} tasks tested; trained only on sessions of other tasks before the first test session ({unused} later sessions unused)"
        );
        Split { train, test, note: Some(note) }
    }

    /// Tasks with sessions on both sides of a split, sorted.
    pub fn task_overlap(&self, split: &Split) -> Vec<String> {
        let tasks = |ids: &[String]| -> BTreeSet<String> {
            ids.iter().map(|id| self.session(id).map_or(id.as_str(), |s| s.task_key()).to_string()).collect()
        };
        tasks(&split.train).intersection(&tasks(&split.test)).cloned().collect()
    }

    /// The dataset's conventions file, if any: the manifest's `conventions`, else
    /// `conventions.json` when it exists in the dataset directory.
    pub fn conventions_file(&self) -> Result<Option<PathBuf>> {
        match &self.manifest.conventions {
            Some(f) if !super::safe_rel(f) => {
                bail!("conventions file {f:?} is not a relative path inside the dataset")
            }
            Some(f) => Ok(Some(self.dir.join(f))),
            None => Ok(Some(self.dir.join("conventions.json")).filter(|p| p.is_file())),
        }
    }

    pub fn default_split(&self) -> String {
        self.manifest.default_split.clone().unwrap_or_else(|| "hash:0.3".into())
    }

    /// The sandbox's entries, from `sandbox.txt`.
    pub fn sandbox(&self) -> Result<Vec<SandboxEntry>> {
        let Some(f) = &self.manifest.sandbox else { return Ok(vec![]) };
        if !super::safe_rel(f) {
            bail!("sandbox file {f:?} is not a relative path inside the dataset");
        }
        let text = std::fs::read_to_string(self.dir.join(f))?;
        Ok(text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(SandboxEntry::parse)
            .collect())
    }
}

fn parse_share(name: &str, share: &str) -> Result<f64> {
    let share: f64 = share.parse().with_context(|| format!("split {name:?}: share is not a number"))?;
    if !(0.0..=1.0).contains(&share) {
        bail!("split {name:?}: share must be within 0..=1");
    }
    Ok(share)
}

/// The fold of a task in a task-grouped k-fold split: SHA-256(task id) mod `k`.
pub fn task_fold(task: &str, k: u64) -> u64 {
    let h = Sha256::digest(task.as_bytes());
    u64::from_le_bytes([h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]]) % k
}

/// Deterministic assignment of a session to the test side of a hash split.
pub fn in_test(id: &str, seed: &str, share: f64) -> bool {
    let h = Sha256::digest(format!("{seed}:{id}").as_bytes());
    let x = u32::from_le_bytes([h[0], h[1], h[2], h[3]]) as f64 / u32::MAX as f64;
    x < share
}

#[derive(Debug, Clone, PartialEq)]
pub enum SandboxEntry {
    File(String),
    Symlink { path: String, target: String },
}

impl SandboxEntry {
    fn parse(l: &str) -> SandboxEntry {
        match l.split_once(" -> ") {
            Some((p, t)) => SandboxEntry::Symlink { path: p.trim().into(), target: t.trim().into() },
            None => SandboxEntry::File(l.into()),
        }
    }
}

/// A materialised sandbox: `<tmp>/outside/…` for escape targets, `<tmp>/repo` as the root.
pub struct Sandbox {
    _dir: tempfile::TempDir,
    pub root: PathBuf,
}

impl Sandbox {
    pub fn create(entries: &[SandboxEntry]) -> Result<Sandbox> {
        let dir = tempfile::tempdir()?;
        // Canonical, so the root string matches what contracts resolve (macOS: /private/var/…).
        let base = dir.path().canonicalize()?;
        let root = base.join("repo");
        std::fs::create_dir_all(&root)?;
        let under = |p: &str| -> Result<PathBuf> {
            // `../x` lands in <base>/x: outside the repository, still inside the temp directory.
            let q = crate::lexical(&root.join(p));
            if !q.starts_with(&base) {
                bail!("sandbox entry {p:?} escapes the temp directory");
            }
            Ok(q)
        };
        for e in entries {
            match e {
                SandboxEntry::File(p) => {
                    let q = under(p)?;
                    if let Some(d) = q.parent() {
                        std::fs::create_dir_all(d)?;
                    }
                    std::fs::write(&q, b"")?;
                }
                SandboxEntry::Symlink { path, target } => {
                    let q = under(path)?;
                    if let Some(d) = q.parent() {
                        std::fs::create_dir_all(d)?;
                    }
                    #[cfg(unix)]
                    std::os::unix::fs::symlink(target, &q)?;
                    #[cfg(not(unix))]
                    let _ = (q, target);
                }
            }
        }
        Ok(Sandbox { _dir: dir, root })
    }
}
