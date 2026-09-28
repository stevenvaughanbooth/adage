//! Evidence integrity (E1): a route that steers the sessions which would confirm it goes dormant
//! instead of extinguishing itself, and directives of unknown kind are excluded from the learner.
//!
//! Synthetic Claude Code transcripts in the real on-disk format, as in `golden.rs`.

use adage_learn::{learn, Inputs, LearnConfig};
use adage_tracks::{io, Index, Route, RouteState, Strength, Track};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const CWD: &str = "/fixture/acme-billing";
const REPO: &str = "acme-billing";

struct Tx {
    lines: Vec<Value>,
    sid: String,
    n: usize,
}

impl Tx {
    fn new(sid: &str, prompt: &str) -> Tx {
        let first = json!({"type":"user","sessionId":sid,"cwd":CWD,"timestamp":"2026-09-20T10:00:00Z",
            "message":{"role":"user","content":prompt}});
        Tx { lines: vec![first], sid: sid.into(), n: 0 }
    }
    /// A directive delivered by the hook, as Claude Code records it: the lookup's text, naming
    /// the lookup of `item` (a wrapper of another route names nothing readable).
    fn directive(&mut self, wrapper: &str, item: &str) -> &mut Self {
        let body = if wrapper.contains("lookup-work-item") {
            format!("In this repository, work item details are looked up with:\n  python3 scripts/wi-lookup {item}")
        } else {
            "…".to_string()
        };
        self.lines.push(json!({"type":"attachment","sessionId":self.sid,"cwd":CWD,
            "attachment": {"type": "hook_additional_context",
                           "content": [format!("{wrapper}\n{body}\n</adage-reflex>")]}}));
        self
    }
    fn call(&mut self, name: &str, input: Value, output: &str) -> &mut Self {
        self.n += 1;
        let id = format!("toolu_{}", self.n);
        self.lines
            .push(json!({"type":"assistant","sessionId":self.sid,"cwd":CWD,"message":{"id":format!("msg_{}", self.n),
            "model":"claude-test","content":[{"type":"tool_use","id":id,"name":name,"input":input}]}}));
        self.lines.push(json!({"type":"user","sessionId":self.sid,"cwd":CWD,"message":{"role":"user",
            "content":[{"type":"tool_result","tool_use_id":id,"content":output,"is_error":false}]}}));
        self
    }
    fn bash(&mut self, cmd: &str, out: &str) -> &mut Self {
        self.call("Bash", json!({"command": cmd}), out)
    }
    fn write(&self, dir: &Path) -> PathBuf {
        let p = dir.join(format!("{}.jsonl", self.sid));
        std::fs::write(&p, self.lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n")).unwrap();
        p
    }
}

const LOOKUP: &str =
    r#"<adage-reflex track="acme-billing/learned@0.1.0" route="lookup-work-item" strength="fire" kind="run">"#;

/// A session whose prompt names a work item. `steered`: the lookup directive was delivered.
/// `looks_up`: the agent ran the lookup (as nearly every agent does once told to).
fn session(dir: &Path, sid: &str, item: usize, steered: Option<&str>, looks_up: bool) -> PathBuf {
    let id = format!("W-{:04}", 1000 + item);
    let mut t = Tx::new(sid, &format!("Pick up {id} and get it done."));
    if let Some(w) = steered {
        t.directive(w, &id);
    }
    t.bash("ls", "README.md\nMakefile\nacme");
    if looks_up {
        t.bash(&format!("python3 scripts/wi-lookup {id}"), &format!("ID: {id}\nAcceptance: ok"));
    }
    t.call("Edit", json!({"file_path": format!("{CWD}/acme/billing.py")}), "ok");
    t.write(dir)
}

struct World {
    dir: tempfile::TempDir,
}

impl World {
    fn new() -> World {
        World { dir: tempfile::tempdir().unwrap() }
    }
    fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }
    /// A fresh history directory: the transcripts a learn run can still see.
    fn history(&self, name: &str) -> PathBuf {
        let d = self.dir.path().join(name);
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    fn learn(&self, files: &[PathBuf]) -> Index {
        let inputs = Inputs {
            transcripts: files.to_vec(),
            outcomes: BTreeMap::new(),
            repo_override: Some(REPO.into()),
            models: None,
            cache: None,
            only_repo: None,
        };
        learn(&inputs, &self.home().join("tracks"), &self.home().join("index.json"), &LearnConfig::demo()).unwrap();
        Index::load(&self.home().join("index.json"))
    }
    fn track(&self) -> Track {
        io::read(&self.home().join("tracks").join(REPO).join("learned")).unwrap()
    }
    fn lookup(&self) -> Option<Route> {
        self.track().routes.into_iter().find(|r| r.id == "lookup-work-item")
    }
}

#[test]
fn a_route_that_steers_its_own_evidence_away_goes_stale_not_extinguished() {
    let w = World::new();

    // 1. Before Adage: eight unsteered sessions look the work item up. The route is learned.
    let h = w.history("before");
    let before: Vec<PathBuf> = (0..8).map(|i| session(&h, &format!("u{i}"), i, None, true)).collect();
    let index = w.learn(&before);
    let r = w.lookup().expect("learned");
    assert_eq!((r.state, r.strength), (RouteState::Firm, Strength::Fire));
    assert_eq!((r.evidence.support, r.evidence.opportunities, r.evidence.exposed), (8, 8, 0));
    assert!(index.routes.iter().any(|x| x.route.id == r.id));
    let learned = r.evidence.clone();

    // 2. The old transcripts expire. What is left: nine sessions the route steered (they looked
    //    the item up, as told) and one held-out session (~10%) that found it unaided. The learner
    //    sees one unsteered session: too little support. Before E1 the route extinguished itself.
    let h = w.history("after");
    let mut after: Vec<PathBuf> = (0..9).map(|i| session(&h, &format!("t{i}"), 20 + i, Some(LOOKUP), true)).collect();
    after.push(session(&h, "h0", 40, None, true));
    let index = w.learn(&after);
    let r = w.lookup().expect("kept, dormant");
    assert_eq!(r.state, RouteState::Dormant);
    assert_eq!(r.strength, Strength::Prime, "a dormant route is never fire");
    let today = adage_tracks::review::today();
    assert_eq!(r.evidence.dormant_since.as_deref(), Some(today.as_str()));
    assert_eq!(r.evidence.exposed, 9, "the steered sessions it could not learn from");
    // Its last evidence is kept as it was.
    assert_eq!((r.evidence.support, r.evidence.opportunities), (learned.support, learned.opportunities));
    assert_eq!(r.evidence.lower_bound, learned.lower_bound);
    // Still delivered (at prime), and the track still validates and re-reads.
    let ir = index.routes.iter().find(|x| x.route.id == r.id).expect("dormant routes are compiled");
    assert_eq!(adage_tracks::delivered_strength(&ir.route), Strength::Prime);
    io::validate(&w.track()).unwrap();
    // Learning the same history again changes nothing, and keeps the date it went dormant.
    let bytes = std::fs::read(w.home().join("index.json")).unwrap();
    let version = w.track().manifest.track.version;
    w.learn(&after);
    assert_eq!(bytes, std::fs::read(w.home().join("index.json")).unwrap());
    assert_eq!(version, w.track().manifest.track.version);

    // 3. Re-found (enough held-out sessions find it unaided): back to its normal state.
    let mut refound = after.clone();
    refound.extend((1..5).map(|i| session(&h, &format!("h{i}"), 40 + i, None, true)));
    w.learn(&refound);
    let r = w.lookup().unwrap();
    assert_eq!((r.state, r.evidence.dormant_since.clone()), (RouteState::Firm, None));
    assert_eq!((r.evidence.support, r.evidence.opportunities, r.evidence.exposed), (5, 5, 9));

    // 4. Dormant again, then contrary evidence: unsteered sessions that name a work item and do
    //    not look it up (1 of 6 did). That is not exposure; the route is dropped as before.
    w.learn(&after);
    assert_eq!(w.lookup().unwrap().state, RouteState::Dormant);
    let mut contrary = after.clone();
    contrary.extend((0..5).map(|i| session(&h, &format!("c{i}"), 60 + i, None, false)));
    let index = w.learn(&contrary);
    assert!(w.lookup().is_none(), "dropped on contrary evidence: {:?}", w.lookup());
    assert!(!index.routes.iter().any(|x| x.route.id == "lookup-work-item"));
}

#[test]
fn a_route_nothing_exposed_is_dropped_as_before() {
    let w = World::new();
    let h = w.history("before");
    let before: Vec<PathBuf> = (0..8).map(|i| session(&h, &format!("u{i}"), i, None, true)).collect();
    w.learn(&before);
    assert!(w.lookup().is_some());
    // Every transcript expired and nothing replaced them but sessions that never name a work
    // item: no exposure explains the absence.
    let h = w.history("after");
    let mut t = Tx::new("x0", "Tidy the README.");
    t.bash("ls", "README.md");
    let after = vec![t.write(&h)];
    w.learn(&after);
    assert!(w.lookup().is_none());
}

#[test]
fn directives_that_name_nothing_readable_steer_everything() {
    let w = World::new();
    let h = w.history("h");
    let mut files: Vec<PathBuf> = (0..6).map(|i| session(&h, &format!("u{i}"), i, None, true)).collect();
    // Steered by directives whose wrapper was cut short (no body to read what they named): either
    // could have steered the first command, so none of these traces is lookup evidence.
    let old = r#"<adage-reflex track="team/tricks@1.0.0" route="avoid-legacy-runner" strength="inhibit">"#;
    let other = r#"<adage-reflex track="team/tricks@1.0.0" route="dead-end-9" strength="inhibit" kind="other">"#;
    for i in 0..3 {
        files.push(session(&h, &format!("o{i}"), 10 + i, Some(old), true));
        files.push(session(&h, &format!("k{i}"), 20 + i, Some(other), true));
    }
    let p = files.last().unwrap();
    let s = adage_learn::import::parse_transcript(p, &adage_core::CommandTable::load(), None).unwrap();
    assert!(!s.influence.is_empty() && s.influence.iter().all(|d| d.all), "{:?}", s.influence);
    w.learn(&files);
    let r = w.lookup().unwrap();
    assert_eq!(r.evidence.opportunities, 6, "sessions steered by an unknown route are not lookup opportunities");
    assert_eq!(r.evidence.exposed, 6);
}
