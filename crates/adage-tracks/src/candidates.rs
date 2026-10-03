//! Candidates: patterns the learner has counted that are not reflexes (plan D1, architecture
//! §3.10). Two kinds:
//!
//! - a **border** pattern: a sequence (a context trigger of two or more events, never a single
//!   call) seen in at least `s_min − border` traces but fewer than `s_min`, mined in a pass of its
//!   own (`adage_graph::mine::mine_candidates`), so the reflexes' mining is never touched;
//! - a **nascent** reflex with a context trigger: learned, but waiting for a person's review
//!   (`review: true`).
//!
//! A candidate is counted, never delivered. It carries the stable id the same pattern gets as a
//! reflex (`adage_graph::compile::ids`), so the day it crosses `s_min` a person sees one name.
//!
//! **On disk**, one file per repository beside its learned track, owner-only like the index:
//! `<tracks>/<slug>/learned/candidates.json`, written only through the learner's store port
//! (`adage_learn::store::Store::write_candidates`):
//!
//! ```json
//! { "schema": "adage-candidates/1", "repo": "acme-billing", "s_min": 3,
//!   "candidates": [{ "id": "run-1a2b3c", "trigger": {"on": "context", …}, "step": {"do": "run", …},
//!                    "support": 2, "days": 2, "last_seen": "2026-09-09", "review": false,
//!                    "sessions": ["…", "…"] }],
//!   "formed": [{ "id": "run-4d5e6f", "sessions": ["…"] }] }
//! ```
//!
//! `sessions` are the supporting trace ids (what tells the next learn which traces tipped a
//! candidate into a reflex); `formed` lists the candidates of the previous learn that this learn
//! made reflexes, with the traces that tipped each. Neither reaches the index.
//!
//! **In the index** ([`crate::Index::candidates`], [`crate::Index::candidate_contexts`]) each
//! repository's valid candidates are [`Candidate`]s, with `s_min` on each, never a route: the
//! compiled index drops any whose id is a route of the repository.

use crate::io;
use crate::model::*;
use adage_core::Effect;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The `schema` of `candidates.json`.
pub const SCHEMA: &str = "adage-candidates/1";
/// The file's name, in the repository's learned track directory.
pub const FILE: &str = "candidates.json";
/// The learned track's directory name (`adage_learn::TRACK`), where the file lives.
pub const LEARNED_TRACK: &str = "learned";
/// At most this many candidates per repository, in the file and in the index: the hook walks
/// them on every event that could notice one.
pub const MAX_CANDIDATES: usize = 128;
/// At most this many session ids recorded per candidate (or per formed reflex).
pub const MAX_SESSIONS: usize = 256;
/// The largest `candidates.json` read, in bytes: well above what [`MAX_CANDIDATES`] entries of
/// [`MAX_SESSIONS`] ids each take, so only a file not written by the learner is refused.
pub const MAX_FILE_BYTES: u64 = 8 << 20;

/// A candidate as the index holds it (and as the hook reads it).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    /// The id the same pattern gets as a reflex.
    pub id: String,
    /// When it would apply: always a [`Trigger::Context`] of 2 to [`CONTEXT_MAX_ITEMS`] items.
    pub trigger: Trigger,
    /// What it would direct: a [`Step::Run`] or a [`Step::Sequence`].
    pub step: Step,
    /// Distinct traces it was seen in, unsteered.
    pub support: u32,
    /// The support a reflex needs in this repository (`learn.min_support`, or more for a large
    /// history). Necessary, not sufficient: forming also needs the rate bar.
    pub s_min: u32,
    /// Distinct calendar days of those traces.
    pub days: u32,
    /// The UTC date (`YYYY-MM-DD`) of the latest of them, when the traces carry dates.
    pub last_seen: Option<String>,
    /// A nascent reflex: it formed, and waits for a person's review.
    pub review: bool,
}

/// One candidate in `candidates.json`: [`Candidate`] without `s_min` (the file's), with its
/// supporting trace ids.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub trigger: Trigger,
    pub step: Step,
    pub support: u32,
    pub days: u32,
    pub last_seen: Option<String>,
    pub review: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sessions: Vec<String>,
}

/// A candidate of the previous learn that is a reflex now, with the traces that tipped it (its
/// supporting traces that the candidate did not have).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Formed {
    pub id: String,
    #[serde(default)]
    pub sessions: Vec<String>,
}

/// `candidates.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateFile {
    pub schema: String,
    pub repo: String,
    pub s_min: u32,
    pub candidates: Vec<Entry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub formed: Vec<Formed>,
}

impl CandidateFile {
    pub fn new(repo: &str, s_min: u32) -> CandidateFile {
        CandidateFile { schema: SCHEMA.into(), repo: repo.into(), s_min, candidates: Vec::new(), formed: Vec::new() }
    }

    /// Sorted by id, one entry per id (the most support wins, then a review), at most
    /// [`MAX_CANDIDATES`] (the most support first), each with sorted, bounded session ids: the same
    /// learn always writes the same bytes.
    pub fn normalize(&mut self) {
        let mut by_id: BTreeMap<String, Entry> = BTreeMap::new();
        for mut e in std::mem::take(&mut self.candidates) {
            e.sessions.sort();
            e.sessions.dedup();
            e.sessions.truncate(MAX_SESSIONS);
            let better = |k: &Entry| (e.review, e.support) > (k.review, k.support);
            if by_id.get(&e.id).is_none_or(better) {
                by_id.insert(e.id.clone(), e);
            }
        }
        let mut all: Vec<Entry> = by_id.into_values().collect();
        if all.len() > MAX_CANDIDATES {
            all.sort_by(|a, b| (b.review, b.support).cmp(&(a.review, a.support)).then(a.id.cmp(&b.id)));
            all.truncate(MAX_CANDIDATES);
            all.sort_by(|a, b| a.id.cmp(&b.id));
        }
        self.candidates = all;
        for f in &mut self.formed {
            f.sessions.sort();
            f.sessions.dedup();
            f.sessions.truncate(MAX_SESSIONS);
        }
        self.formed.sort_by(|a, b| a.id.cmp(&b.id));
        self.formed.dedup_by(|a, b| a.id == b.id);
    }

    /// The file's valid candidates, as the index holds them; an invalid one is left out alone
    /// (the file is the learner's output from untrusted traces).
    pub fn valid(&self) -> Vec<Candidate> {
        self.candidates
            .iter()
            .filter(|e| validate_entry(e, self.s_min).is_ok())
            .map(|e| Candidate {
                id: e.id.clone(),
                trigger: e.trigger.clone(),
                step: e.step.clone(),
                support: e.support,
                s_min: self.s_min,
                days: e.days,
                last_seen: e.last_seen.clone(),
                review: e.review,
            })
            .collect()
    }

    /// The entry with this id.
    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.candidates.iter().find(|e| e.id == id)
    }
}

impl Candidate {
    /// The candidate as a route would carry it: what [`io::validate`] checks and what
    /// [`Route::gist`] reads. Its effect is the strongest class of the commands it would direct
    /// (classified here, never declared), its strength prime and its state nascent: it is never
    /// delivered.
    pub fn route(&self) -> Route {
        probe(&self.id, &self.trigger, &self.step, self.support, self.s_min)
    }

    /// What it would tell the agent, in one sentence (`Route::gist`).
    pub fn gist(&self) -> String {
        self.route().gist()
    }
}

fn probe(id: &str, trigger: &Trigger, step: &Step, support: u32, opportunities: u32) -> Route {
    let table = adage_core::CommandTable::load();
    let effect = commands(step).iter().map(|c| io::classify(c, table)).max().unwrap_or(Effect::Observe);
    Route {
        id: id.to_string(),
        summary: "A pattern counted in this repository's traces, not yet a reflex".into(),
        trigger: trigger.clone(),
        step: step.clone(),
        slots: vec![],
        effect,
        strength: Strength::Prime,
        state: RouteState::Nascent,
        exit: vec![Exit::IntentChanges],
        evidence: Evidence { support, opportunities, ..Default::default() },
        authored: false,
    }
}

fn commands(step: &Step) -> Vec<&str> {
    match step {
        Step::Run { command } => vec![command],
        Step::Sequence { commands } => commands.iter().map(String::as_str).collect(),
        _ => vec![],
    }
}

/// Whether one entry may be a candidate: a safe id, a context of 2 to [`CONTEXT_MAX_ITEMS`]
/// command, failure or edit items within [`CONTEXT_MAX_WITHIN`], a run or a sequence that passes
/// track validation (safe, fixed, directable commands), and support below `s_min`.
pub fn validate_entry(e: &Entry, s_min: u32) -> Result<()> {
    if !io::safe_id(&e.id) {
        bail!("candidate id {:?} is not allowed", e.id);
    }
    let Trigger::Context { items, within } = &e.trigger else { bail!("candidate {}: not a context", e.id) };
    if !(2..=CONTEXT_MAX_ITEMS).contains(&items.len()) || *within > CONTEXT_MAX_WITHIN {
        bail!("candidate {}: a context of 2 to {CONTEXT_MAX_ITEMS} items within {CONTEXT_MAX_WITHIN}", e.id);
    }
    for it in items {
        match it {
            CtxItem::Act { keys } | CtxItem::Fail { keys, .. } if !keys.is_empty() => {}
            CtxItem::Edit { glob } if !glob.is_empty() => {}
            _ => bail!("candidate {}: a context item that is not a command, a failure or an edit", e.id),
        }
    }
    if !matches!(e.step, Step::Run { .. } | Step::Sequence { .. }) {
        bail!("candidate {}: only a run or a sequence", e.id);
    }
    if commands(&e.step).iter().any(|c| !io::slot_refs(c).is_empty()) {
        bail!("candidate {}: a command with slots", e.id);
    }
    // A nascent reflex has formed, so it has the support; a border candidate has not.
    if s_min == 0 || (!e.review && e.support >= s_min) || e.support == 0 {
        bail!("candidate {}: support {} against s_min {s_min}", e.id, e.support);
    }
    let r = probe(&e.id, &e.trigger, &e.step, e.support, e.support);
    let t = Track {
        manifest: Manifest {
            track: TrackInfo {
                name: "candidates/learned".into(),
                version: "0.0.0".into(),
                description: String::new(),
                provenance: String::new(),
            },
            scope: Scope { repo: String::new() },
            gauge: Gauge::default(),
        },
        routes: vec![r],
    };
    io::validate(&t).with_context(|| format!("candidate {}", e.id))
}

/// Where `repo`'s candidates are kept under `tracks_root`.
pub fn path(tracks_root: &Path, repo: &str) -> PathBuf {
    tracks_root.join(crate::layout::slug(repo)).join(LEARNED_TRACK).join(FILE)
}

/// Read one `candidates.json`.
pub fn read(path: &Path) -> Result<CandidateFile> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes))
        .with_context(|| path.display().to_string())?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        bail!("{}: larger than {MAX_FILE_BYTES} bytes", path.display());
    }
    let f: CandidateFile = serde_json::from_slice(&bytes).with_context(|| path.display().to_string())?;
    if f.schema != SCHEMA {
        bail!("{}: schema {:?}, not {SCHEMA:?}", path.display(), f.schema);
    }
    Ok(f)
}

/// `repo`'s candidates file, if it has one that reads.
pub fn load(tracks_root: &Path, repo: &str) -> Option<CandidateFile> {
    read(&path(tracks_root, repo)).ok().filter(|f| f.repo == repo)
}

/// Write `f` (normalised) for its repository, owner-only, atomically (a temporary file renamed
/// over the old one, so a hook never reads half a file).
pub fn write(tracks_root: &Path, f: &CandidateFile) -> Result<()> {
    let mut f = f.clone();
    f.normalize();
    let p = path(tracks_root, &f.repo);
    let dir = p.parent().expect("a file in a directory");
    std::fs::create_dir_all(dir)?;
    let tmp = p.with_extension(format!("json.tmp{}", std::process::id()));
    io::write_private(&tmp, &serde_json::to_vec_pretty(&f)?)?;
    std::fs::rename(&tmp, &p)?;
    Ok(())
}

/// Every repository's valid candidates under `tracks_root`, by repository key: one file per
/// repository directory (`<slug>/learned/candidates.json`, whose `repo` must slug to the
/// directory it is in). Imports (`@imported`) and hidden directories never hold any. Unreadable
/// files and invalid entries are skipped (fail open: no candidate).
pub fn read_all(tracks_root: &Path) -> BTreeMap<String, Vec<Candidate>> {
    let mut out: BTreeMap<String, Vec<Candidate>> = BTreeMap::new();
    let Ok(dirs) = std::fs::read_dir(tracks_root) else { return out };
    let mut names: Vec<String> = dirs
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.') && !n.starts_with('@'))
        .collect();
    names.sort();
    for name in names {
        let Ok(f) = read(&tracks_root.join(&name).join(LEARNED_TRACK).join(FILE)) else { continue };
        if crate::layout::slug(&f.repo) != name {
            continue;
        }
        let mut cs = f.valid();
        if cs.is_empty() {
            continue;
        }
        // One per id (the strongest), and past the bound the strongest kept, as `normalize` keeps
        // them: a file the learner did not write is held to the same rule.
        let strength = |c: &Candidate| (c.review, c.support);
        cs.sort_by(|a, b| a.id.cmp(&b.id).then(strength(b).cmp(&strength(a))));
        cs.dedup_by(|a, b| a.id == b.id);
        if cs.len() > MAX_CANDIDATES {
            cs.sort_by(|a, b| strength(b).cmp(&strength(a)).then(a.id.cmp(&b.id)));
            cs.truncate(MAX_CANDIDATES);
            cs.sort_by(|a, b| a.id.cmp(&b.id));
        }
        out.entry(f.repo.clone()).or_default().extend(cs);
    }
    out
}

/// The ids of `routes`, for leaving out a candidate that is a reflex already.
pub fn route_ids<'a>(routes: impl IntoIterator<Item = &'a Route>) -> BTreeSet<&'a str> {
    routes.into_iter().map(|r| r.id.as_str()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, support: u32) -> Entry {
        Entry {
            id: id.into(),
            trigger: Trigger::Context {
                items: vec![
                    CtxItem::Edit { glob: "acme/*.py".into() },
                    CtxItem::Act { keys: vec!["exec make test".into()] },
                ],
                within: 8,
            },
            step: Step::Run { command: "make lint".into() },
            support,
            days: support,
            last_seen: Some("2026-09-09".into()),
            review: false,
            sessions: vec!["b".into(), "a".into(), "a".into()],
        }
    }

    #[test]
    fn a_file_round_trips_and_keeps_only_valid_candidates() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = CandidateFile::new("acme-billing", 3);
        f.candidates = vec![entry("run-bbbbbb", 2), entry("run-aaaaaa", 2), entry("run-cccccc", 3)];
        let mut unsafe_cmd = entry("run-dddddd", 2);
        unsafe_cmd.step = Step::Run { command: "make lint; rm -rf /".into() };
        f.candidates.push(unsafe_cmd);
        let mut single = entry("run-eeeeee", 2);
        single.trigger =
            Trigger::Context { items: vec![CtxItem::Act { keys: vec!["exec make test".into()] }], within: 8 };
        f.candidates.push(single);
        write(dir.path(), &f).unwrap();
        let p = path(dir.path(), "acme-billing");
        assert!(p.ends_with("acme-billing/learned/candidates.json"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600, "owner-only");
        }
        let back = read(&p).unwrap();
        assert_eq!(
            back.candidates.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            ["run-aaaaaa", "run-bbbbbb", "run-cccccc", "run-dddddd", "run-eeeeee"]
        );
        assert_eq!(back.candidates[0].sessions, ["a", "b"], "sorted, once each");
        let valid = back.valid();
        // At s_min, a border pattern is a reflex, not a candidate; an unsafe command and a single
        // event are never candidates.
        assert_eq!(valid.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["run-aaaaaa", "run-bbbbbb"]);
        assert!(valid.iter().all(|c| c.s_min == 3));
        assert_eq!(read_all(dir.path()).get("acme-billing").map(Vec::len), Some(2));
        // The same candidates always write the same bytes.
        let bytes = std::fs::read(&p).unwrap();
        write(dir.path(), &back).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), bytes);
    }

    #[test]
    fn a_file_in_the_wrong_directory_or_of_another_schema_is_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = CandidateFile::new("acme-billing", 3);
        f.candidates = vec![entry("run-aaaaaa", 2)];
        write(dir.path(), &f).unwrap();
        let moved = dir.path().join("other").join(LEARNED_TRACK);
        std::fs::create_dir_all(&moved).unwrap();
        std::fs::copy(path(dir.path(), "acme-billing"), moved.join(FILE)).unwrap();
        assert_eq!(read_all(dir.path()).keys().collect::<Vec<_>>(), ["acme-billing"]);
        let mut v: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path(dir.path(), "acme-billing")).unwrap()).unwrap();
        v["schema"] = "adage-candidates/9".into();
        std::fs::write(path(dir.path(), "acme-billing"), v.to_string()).unwrap();
        assert!(read_all(dir.path()).is_empty());
    }

    /// Review L5: a file over the bound (one the learner did not write) is read with the
    /// strongest candidates kept, not the first ids; a file over the byte bound is not read.
    #[test]
    fn read_all_keeps_the_strongest_past_the_bound() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = CandidateFile::new("acme-billing", 5);
        // The weakest have the lowest ids: an id-order cut would keep them.
        f.candidates =
            (0..MAX_CANDIDATES + 10).map(|i| entry(&format!("run-{i:06}"), if i < 10 { 1 } else { 3 })).collect();
        let p = path(dir.path(), "acme-billing");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, serde_json::to_vec(&f).unwrap()).unwrap();
        let all = read_all(dir.path());
        let cs = &all["acme-billing"];
        assert_eq!(cs.len(), MAX_CANDIDATES);
        assert!(cs.iter().all(|c| c.support == 3), "the least seen are cut");
        assert!(cs.windows(2).all(|w| w[0].id < w[1].id));
        let big = std::fs::File::options().append(true).open(&p).unwrap();
        big.set_len(MAX_FILE_BYTES + 1).unwrap();
        assert!(read(&p).is_err());
    }

    #[test]
    fn at_most_the_bound_with_the_most_support_kept() {
        let mut f = CandidateFile::new("r", 5);
        f.candidates = (0..MAX_CANDIDATES + 10).map(|i| entry(&format!("run-{i:06}"), 1 + (i % 4) as u32)).collect();
        f.normalize();
        assert_eq!(f.candidates.len(), MAX_CANDIDATES);
        assert!(f.candidates.windows(2).all(|w| w[0].id < w[1].id));
        let ones = (0..MAX_CANDIDATES + 10).filter(|i| i % 4 == 0).count();
        assert_eq!(f.candidates.iter().filter(|e| e.support == 1).count(), ones - 10, "the least seen go first");
    }
}
