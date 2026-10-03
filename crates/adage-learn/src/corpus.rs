//! Canonical evidence snapshots. Import is streaming and publishes one manifest only after all
//! records pass. An interrupted or rejected import cannot partially replace training evidence.

use crate::store::Store;
use adage_ingest::corpus::{self, Limits, ReadReport};
use adage_trace::Session;
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};

pub const SCHEMA: u32 = 1;

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub repo: String,
    /// Session id → SHA-256 of the complete canonical record. Sorted, and independent of arrival order.
    pub sessions: BTreeMap<String, String>,
    pub generation: String,
}

impl Manifest {
    pub fn new(repo: &str, sessions: BTreeMap<String, String>) -> Result<Self> {
        let generation = digest(&serde_json::to_vec(&(SCHEMA, repo, &sessions))?);
        Ok(Self { schema: SCHEMA, repo: repo.into(), sessions, generation })
    }

    pub fn validate(&self, repo: &str) -> Result<()> {
        ensure!(self.schema == SCHEMA && self.repo == repo, "incompatible corpus manifest");
        ensure!(self == &Self::new(repo, self.sessions.clone())?, "corpus manifest digest mismatch");
        ensure!(self.sessions.values().all(|s| valid_digest(s)), "invalid corpus object digest");
        Ok(())
    }
}

pub(crate) fn valid_digest(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Merge new sessions; existing ids must have exactly the same record.
    Append,
    /// Replace this repository's snapshot, allowing explicit corrections and removals.
    Replace,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImportReport {
    pub read: ReadReport,
    pub repo: String,
    pub elsewhere: u64,
    pub duplicates: u64,
    pub conflicts: u64,
    pub added: usize,
    pub replaced: usize,
    pub removed: usize,
    pub sessions: usize,
    pub committed: bool,
    pub generation: String,
}

/// One repository per transaction; a service can feed this reader from any stream. The caller
/// holds the store's writer lock across import. Content objects written before a failure are
/// unreachable until a manifest references them; the prior manifest remains the committed input.
pub fn import(reader: impl BufRead, repo: &str, mode: Mode, limits: Limits, store: &dyn Store) -> Result<ImportReport> {
    ensure!(!repo.trim().is_empty(), "a corpus import requires a repository key");
    let previous = store.corpus_manifest(repo)?.unwrap_or(Manifest::new(repo, BTreeMap::new())?);
    previous.validate(repo)?;
    let mut incoming = BTreeMap::<String, String>::new();
    let mut report = ImportReport { repo: repo.into(), generation: previous.generation.clone(), ..Default::default() };
    report.read = corpus::read(reader, limits, |session| {
        if session.repo != repo {
            report.elsewhere += 1;
            return Ok(());
        }
        let body = serde_json::to_vec(&session)?;
        let hash = digest(&body);
        if let Some(found) = incoming.get(&session.id) {
            if found == &hash {
                report.duplicates += 1;
            } else {
                report.conflicts += 1;
            }
            return Ok(());
        }
        if mode == Mode::Append {
            if let Some(found) = previous.sessions.get(&session.id) {
                if found == &hash {
                    report.duplicates += 1;
                } else {
                    report.conflicts += 1;
                }
            }
        }
        store.write_corpus_object(&hash, &body).map_err(std::io::Error::other)?;
        incoming.insert(session.id, hash);
        Ok(())
    })?;
    report.sessions = previous.sessions.len();
    if report.read.rejected_count() > 0 || report.conflicts > 0 {
        return Ok(report);
    }
    // A wrong repository selection must never erase an existing snapshot.
    ensure!(
        !incoming.is_empty() || report.elsewhere == 0,
        "the input contains no sessions for the selected repository"
    );
    let mut sessions = match mode {
        Mode::Append => previous.sessions.clone(),
        Mode::Replace => BTreeMap::new(),
    };
    for (id, hash) in incoming {
        match previous.sessions.get(&id) {
            None => report.added += 1,
            Some(old) if old != &hash => report.replaced += 1,
            _ => {}
        }
        sessions.insert(id, hash);
    }
    report.removed = previous.sessions.keys().filter(|id| !sessions.contains_key(*id)).count();
    let next = Manifest::new(repo, sessions)?;
    report.sessions = next.sessions.len();
    report.generation = next.generation.clone();
    store.write_corpus_manifest(&next)?;
    report.committed = true;
    Ok(report)
}

/// Read exactly the snapshot named by its manifest, verifying content hashes and ownership.
/// Learning's current memory bound is one repository, not the entire organization's corpus.
pub fn load(repo: &str, store: &dyn Store) -> Result<adage_ingest::batch::Batch> {
    let manifest = store.corpus_manifest(repo)?.context("no canonical corpus for this repository")?;
    manifest.validate(repo)?;
    let mut batch = adage_ingest::batch::Batch::default();
    for (id, hash) in manifest.sessions {
        let body = store.corpus_object(&hash)?;
        ensure!(digest(&body) == hash, "corpus object digest mismatch");
        let session: Session = serde_json::from_slice(&body).context("invalid corpus object")?;
        ensure!(session.id == id && session.repo == repo, "corpus object belongs to another session or repository");
        ensure!(corpus::valid_session(&session, Limits::default()), "invalid canonical session in corpus");
        batch.read += 1;
        batch.steered += usize::from(session.steered);
        batch.sessions.push(session);
    }
    Ok(batch)
}

/// Normalize native histories into a portable canonical stream without probing local checkouts.
pub fn export(
    paths: &[PathBuf],
    repo: Option<&str>,
    identities: &BTreeMap<String, adage_trace::EvidenceIdentity>,
    mut writer: impl std::io::Write,
) -> Result<usize> {
    let cx = adage_ingest::Decode {
        table: adage_core::CommandTable::load(),
        locator: &adage_ingest::AsRecorded,
        repo_override: repo,
        subagent: false,
    };
    let mut count = 0;
    for path in paths {
        let mut session = adage_ingest::read_trace(path, &cx)
            .with_context(|| format!("no usable canonical session in {}", path.display()))?;
        if let Some(identity) = identities.get(&session.id) {
            session.identity = identity.clone();
        }
        corpus::write(&mut writer, &session)?;
        count += 1;
    }
    Ok(count)
}

/// Learn a committed snapshot with the normal review and execution-admission boundaries.
pub fn learn(repo: &str, models: Option<&Path>, store: &dyn Store, cfg: &crate::LearnConfig) -> Result<crate::Report> {
    let batch = load(repo, store)?;
    if !batch.sessions.is_empty() {
        return crate::learn_sessions_into(batch, Some(repo), models, store, cfg);
    }
    let (mut report, planned) = crate::plan_repository(Vec::new(), Some(repo), models, store, cfg, false)?;
    for p in &planned {
        store.write_track(p)?;
    }
    store.write_follow_weights(repo, &Default::default());
    let index = store.compile_index()?;
    report.index_generation = index.generation;
    report.conflicts = index.conflicts;
    Ok(report)
}
