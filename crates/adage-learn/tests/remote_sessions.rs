//! Traces collected elsewhere (a team's trace store): decoded from bytes with no checkout on this
//! machine, then learned from through the same entry point as local history.

use adage_ingest::{source::Memory, AsRecorded, Decode};
use adage_learn::store::Store;
use adage_learn::{learn_sessions, LearnConfig};
use adage_tracks::proposals::Proposals;
use adage_tracks::review::{Decision, Review, Reviews};
use adage_tracks::why::Why;
use adage_tracks::{RouteState, Track};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Mutex;

fn transcript(sid: &str) -> Vec<u8> {
    let cwd = "/nowhere/on/this/machine/acme";
    [
        json!({"type": "user", "sessionId": sid, "cwd": cwd, "timestamp": "2026-09-01T10:00:00Z",
               "message": {"role": "user", "content": "fix the failing test"}}),
        json!({"type": "assistant", "sessionId": sid, "cwd": cwd,
               "message": {"id": "m1", "content": [{"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "make test"}}]}}),
        json!({"type": "user", "sessionId": sid, "cwd": cwd,
               "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1", "content": "ok", "is_error": false}]}}),
    ]
    .iter()
    .map(|l| format!("{l}\n"))
    .collect::<String>()
    .into_bytes()
}

#[test]
fn sessions_from_another_machine_are_learned_like_local_ones() {
    let source = Memory((0..4).map(|i| (format!("s{i}"), transcript(&format!("s{i}")))).collect());
    let table = adage_core::CommandTable::load();
    let cx = Decode { table, locator: &AsRecorded, repo_override: Some("github.com/acme/app"), subagent: false };
    let mut batch = adage_ingest::batch::Batch::default();
    adage_ingest::ingest(&source, &cx, |_, s| {
        batch.read += 1;
        batch.sessions.extend(s);
    });
    assert_eq!(batch.sessions.len(), 4);

    let d = tempfile::tempdir().unwrap();
    let (tracks, index) = (d.path().join("tracks"), d.path().join("index.json"));
    let cfg = LearnConfig { instincts: None, ..LearnConfig::default() };
    let report = learn_sessions(batch, None, None, &tracks, &index, &cfg).unwrap();
    assert_eq!((report.sessions_read, report.sessions_used), (4, 4));
    assert_eq!(report.repos.keys().collect::<Vec<_>>(), ["github.com/acme/app"]);
    assert!(index.exists(), "the index is compiled");
}

/// A session that tries a runner the repository does not have, then the one it does: a dead end
/// learning turns into a reflex.
fn dead_end(sid: &str) -> Vec<u8> {
    let cwd = "/nowhere/on/this/machine/acme";
    let call = |id: &str, cmd: &str| {
        json!({"type": "assistant", "sessionId": sid, "cwd": cwd,
               "message": {"id": format!("m-{id}"), "content": [{"type": "tool_use", "id": id, "name": "Bash", "input": {"command": cmd}}]}})
    };
    let result = |id: &str, out: &str, failed: bool| {
        json!({"type": "user", "sessionId": sid, "cwd": cwd,
               "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": id, "content": out, "is_error": failed}]}})
    };
    [
        json!({"type": "user", "sessionId": sid, "cwd": cwd, "timestamp": "2026-09-01T10:00:00Z",
               "message": {"role": "user", "content": "fix the failing test"}}),
        call("t1", "python3 -m pytest"),
        result("t1", "/usr/bin/python3: No module named pytest", true),
        call("t2", "make test"),
        result("t2", "ok", false),
    ]
    .iter()
    .map(|l| format!("{l}\n"))
    .collect::<String>()
    .into_bytes()
}

/// A store that keeps everything in memory (a team service's rows, here): what earlier runs
/// learned and a person decided, read back by the next run, and a log of every write.
#[derive(Default)]
struct Rows {
    tracks: Mutex<BTreeMap<String, Track>>,
    reviews: Mutex<BTreeMap<String, Reviews>>,
    proposals: Mutex<BTreeMap<String, Proposals>>,
    why: Mutex<BTreeMap<String, BTreeMap<String, Why>>>,
    candidates: Mutex<BTreeMap<String, adage_tracks::candidates::CandidateFile>>,
    log: Mutex<Vec<String>>,
}

impl Rows {
    fn push(&self, row: String) {
        self.log.lock().unwrap().push(row);
    }
    fn log(&self) -> Vec<String> {
        std::mem::take(&mut *self.log.lock().unwrap())
    }
}

impl Store for Rows {
    fn track(&self, name: &str) -> Option<Track> {
        self.tracks.lock().unwrap().get(name).cloned()
    }
    fn holds(&self, name: &str) -> bool {
        self.tracks.lock().unwrap().contains_key(name) || self.reviews.lock().unwrap().contains_key(name)
    }
    fn reviews(&self, name: &str) -> anyhow::Result<Reviews> {
        Ok(self.reviews.lock().unwrap().get(name).cloned().unwrap_or_default())
    }
    fn proposals(&self, name: &str) -> anyhow::Result<Proposals> {
        Ok(self.proposals.lock().unwrap().get(name).cloned().unwrap_or_default())
    }
    fn why(&self, name: &str) -> BTreeMap<String, Why> {
        self.why.lock().unwrap().get(name).cloned().unwrap_or_default()
    }
    fn names_under(&self, prefix: &str) -> Vec<String> {
        let under = |n: &String| n.strip_prefix(prefix).and_then(|r| r.strip_prefix('/')).is_some();
        self.tracks.lock().unwrap().keys().filter(|n| under(n)).cloned().collect()
    }
    fn tracks_under(&self, prefix: &str) -> Vec<Track> {
        self.names_under(prefix).iter().filter_map(|n| self.track(n)).collect()
    }
    fn binding(&self, _: &str) -> Option<adage_pack::binding::Binding> {
        None
    }
    fn candidates(&self, repo: &str) -> Option<adage_tracks::candidates::CandidateFile> {
        self.candidates.lock().unwrap().get(repo).cloned()
    }
    fn packs(&self) -> (Vec<adage_pack::Pack>, Vec<(String, String)>) {
        (Vec::new(), Vec::new())
    }
    fn install_base_packs(&self) {
        self.push("base packs".into());
    }
    fn write_track(&self, p: &adage_learn::Planned) -> anyhow::Result<()> {
        self.tracks.lock().unwrap().insert(p.name.clone(), p.track.clone());
        self.proposals.lock().unwrap().insert(p.name.clone(), p.proposals.clone());
        if let Some(r) = &p.reviews {
            self.reviews.lock().unwrap().insert(p.name.clone(), r.clone());
        }
        if let Some(w) = &p.why {
            self.why.lock().unwrap().insert(p.name.clone(), w.clone());
        }
        self.push(format!("track {} {} ({} routes)", p.name, p.track.manifest.track.version, p.track.routes.len()));
        Ok(())
    }
    fn write_binding(&self, repo: &str, _: &adage_pack::binding::Binding) {
        self.push(format!("binding {repo}"));
    }
    fn write_follow_weights(&self, repo: &str, _: &adage_core::judge::Weights) {
        self.push(format!("follow weights {repo}"));
    }
    fn write_candidates(&self, c: &adage_tracks::candidates::CandidateFile) -> anyhow::Result<()> {
        self.candidates.lock().unwrap().insert(c.repo.clone(), c.clone());
        self.push(format!("candidates {} ({})", c.repo, c.candidates.len()));
        Ok(())
    }
    fn compile_index(&self) -> anyhow::Result<adage_learn::store::Compiled> {
        self.push("index".into());
        let generation = format!("g{}", self.tracks.lock().unwrap().len());
        Ok(adage_learn::store::Compiled { generation, conflicts: vec![] })
    }
    fn index_generation(&self) -> String {
        "g0".into()
    }
}

/// Eight sessions from another machine, each hitting the dead end.
fn dead_ends() -> adage_ingest::batch::Batch {
    let source = Memory((0..8).map(|i| (format!("s{i}"), dead_end(&format!("s{i}")))).collect());
    let table = adage_core::CommandTable::load();
    let cx = Decode { table, locator: &AsRecorded, repo_override: Some("github.com/acme/app"), subagent: false };
    let mut batch = adage_ingest::batch::Batch::default();
    adage_ingest::ingest(&source, &cx, |_, s| {
        batch.read += 1;
        batch.sessions.extend(s);
    });
    batch
}

const TRACK: &str = "github-com-acme-app/learned";

/// Everything a learn run decides reaches state only through its store: with a store that keeps
/// rows in memory, nothing is written to disk, and the index is compiled once, after every row.
#[test]
fn a_learn_run_materializes_only_through_its_store() {
    let d = tempfile::tempdir().unwrap();
    let rows = Rows::default();
    let cfg = LearnConfig { instincts: None, ..LearnConfig::demo() };
    let report = adage_learn::learn_sessions_into(dead_ends(), None, None, &rows, &cfg).unwrap();
    assert_eq!(report.index_generation, "g1", "the store's index");
    let log = rows.log();
    assert!(log.iter().any(|r| r.starts_with(&format!("track {TRACK}"))), "{log:?}");
    assert!(log.contains(&"follow weights github.com/acme/app".to_string()), "{log:?}");
    // Candidates too (plan D1): nothing writes `candidates.json` but the store.
    assert!(log.iter().any(|r| r.starts_with("candidates github.com/acme/app")), "{log:?}");
    assert_eq!(log.iter().filter(|r| *r == "index").count(), 1, "{log:?}");
    assert_eq!(log.last().map(String::as_str), Some("index"), "compiled after every row: {log:?}");
    assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 0, "nothing on disk");
}

/// What earlier runs learned and a person decided is read back from the store, not from disk: a
/// second run over a store held in memory keeps the route it learned unchanged, and a person's
/// decision to extinguish it holds across re-learning.
#[test]
fn a_learn_run_reads_what_earlier_runs_left_from_its_store() {
    let rows = Rows::default();
    let cfg = LearnConfig { instincts: None, ..LearnConfig::demo() };
    adage_learn::learn_sessions_into(dead_ends(), None, None, &rows, &cfg).unwrap();
    let first = rows.track(TRACK).expect("learned");
    let route = first.routes.iter().find(|r| r.state.is_live()).expect("a live reflex").clone();
    rows.log();

    // The same traces again: the same track, read back from the store, so the same version.
    adage_learn::learn_sessions_into(dead_ends(), None, None, &rows, &cfg).unwrap();
    assert_eq!(rows.track(TRACK).unwrap().manifest.track.version, first.manifest.track.version, "{:?}", rows.log());

    // A person extinguishes it, in the store alone: learning does not bring it back.
    let decision = Review {
        decision: Decision::Extinguished,
        on: "2026-10-02".into(),
        by: None,
        basis: adage_tracks::review::basis(&route, 0, 0),
        via: None,
    };
    rows.reviews.lock().unwrap().entry(TRACK.into()).or_default().routes.insert(route.id.clone(), decision);
    adage_learn::learn_sessions_into(dead_ends(), None, None, &rows, &cfg).unwrap();
    let after = rows.track(TRACK).unwrap();
    let kept = after.routes.iter().find(|r| r.id == route.id);
    assert!(kept.is_none_or(|r| r.state == RouteState::Extinguished), "the decision holds: {kept:?}");
    assert_ne!(after.manifest.track.version, first.manifest.track.version, "a change, so a new version");
}
