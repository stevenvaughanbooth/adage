//! Eval datasets on disk: `manifest.json`, transcripts, and a sandbox file list.
//!
//! ```text
//! <dataset>/manifest.json   name, repo, learn thresholds, sessions (+ labels), named splits
//! <dataset>/sessions/*.jsonl Claude Code transcripts (the format `adage learn` reads)
//! <dataset>/sandbox.txt      the repository's files, one per line; `a -> b` is a symlink, and a
//!                            path starting `../` is created *outside* the repository root
//! ```
//!
//! Replay needs the repository only for repository-path contracts (a bound file must exist and
//! resolve inside the root), so the sandbox is materialised as empty files in a temp directory.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
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

    /// Resolve a split name: a named split from the manifest, `hash:<share>[:<seed>]`
    /// (deterministic by session id), or `all` (train and test are every session; in-sample).
    pub fn split(&self, name: &str) -> Result<Split> {
        let ids: Vec<String> = self.manifest.sessions.iter().map(|s| s.id.clone()).collect();
        if name == "all" {
            return Ok(Split { train: ids.clone(), test: ids });
        }
        if let Some(rest) = name.strip_prefix("hash:") {
            let (share, seed) = rest.split_once(':').unwrap_or((rest, "adage-eval"));
            let share: f64 = share.parse().with_context(|| format!("split {name:?}: share is not a number"))?;
            if !(0.0..=1.0).contains(&share) {
                bail!("split {name:?}: share must be within 0..=1");
            }
            let (mut train, mut test) = (vec![], vec![]);
            for id in ids {
                if in_test(&id, seed, share) {
                    test.push(id)
                } else {
                    train.push(id)
                }
            }
            return Ok(Split { train, test });
        }
        self.manifest.splits.get(name).cloned().with_context(|| {
            let names: Vec<&String> = self.manifest.splits.keys().collect();
            format!("no split {name:?} (named splits: {names:?}; or hash:<share>, all)")
        })
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
