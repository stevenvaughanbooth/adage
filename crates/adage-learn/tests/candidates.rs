//! Candidates (plan D1, test plan T-D1-02 … T-D1-05): sequences counted below the support a
//! reflex needs, mined in a pass of their own, written through the store beside the learned track
//! and compiled into the index apart from the routes.

use adage_learn::{diff, learn, Inputs, LearnConfig};
use adage_tracks::candidates::{self, CandidateFile};
use adage_tracks::{Index, Step, Trigger};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const CWD: &str = "/fixture/acme-billing";
const REPO: &str = "acme-billing";

/// A Claude Code transcript, one tool call per model turn.
struct Tx {
    lines: Vec<Value>,
    sid: String,
    n: usize,
}

impl Tx {
    fn new(sid: &str, day: u32, prompt: &str) -> Tx {
        let ts = format!("2026-09-{:02}T10:00:00Z", day);
        let first =
            json!({"type":"user","sessionId":sid,"cwd":CWD,"timestamp":ts,"message":{"role":"user","content":prompt}});
        Tx { lines: vec![first], sid: sid.into(), n: 0 }
    }
    fn call(&mut self, name: &str, input: Value, output: &str) -> &mut Self {
        self.n += 1;
        let id = format!("toolu_{}", self.n);
        self.lines.push(json!({"type":"assistant","sessionId":self.sid,"cwd":CWD,"message":{"id":format!("msg_{}", self.n),"model":"claude-test","content":[{"type":"tool_use","id":id,"name":name,"input":input}]}}));
        self.lines.push(json!({"type":"user","sessionId":self.sid,"cwd":CWD,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":id,"content":output,"is_error":false}]}}));
        self
    }
    fn bash(&mut self, cmd: &str) -> &mut Self {
        self.call("Bash", json!({ "command": cmd }), "ok")
    }
    fn edit(&mut self, path: &str) -> &mut Self {
        self.call("Edit", json!({ "file_path": format!("{CWD}/{path}") }), "ok")
    }
    fn write(&self, dir: &Path) -> PathBuf {
        let p = dir.join(format!("{}.jsonl", self.sid));
        std::fs::write(&p, self.lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n")).unwrap();
        p
    }
}

/// `n` traces: each edits a billing module and runs the tests; the first `lint` of them refresh the
/// dependencies before the tests and run the linter right after.
fn traces(dir: &Path, n: usize, lint: usize) -> Vec<PathBuf> {
    (0..n)
        .map(|i| {
            let mut t = Tx::new(&format!("s{i:02}"), 1 + i as u32, &format!("Fix the rounding in invoice {i}."));
            t.bash("git status --short").edit(&format!("acme/mod{}.py", i % 3));
            // The linter runs after the tests only when the dependencies were refreshed first:
            // the two-event context, not the tests alone, is what predicts it.
            if i < lint {
                t.bash("make deps").bash("make test").bash("git diff --stat").bash("ls acme").bash("make lint");
            } else {
                t.bash("make test");
            }
            t.write(dir)
        })
        .collect()
}

fn condition(files: &[PathBuf], home: &Path, cfg: &LearnConfig) -> Index {
    let inputs = Inputs {
        transcripts: files.to_vec(),
        outcomes: BTreeMap::new(),
        repo_override: Some(REPO.into()),
        models: None,
        cache: None,
        only_repo: None,
    };
    learn(&inputs, &home.join("tracks"), &home.join("index.json"), cfg).unwrap();
    Index::load(&home.join("index.json"))
}

fn cfg() -> LearnConfig {
    LearnConfig { min_support: 5, ..LearnConfig::default() }
}

fn file(home: &Path) -> CandidateFile {
    candidates::read(&candidates::path(&home.join("tracks"), REPO)).expect("candidates.json")
}

fn lint_candidates(f: &CandidateFile) -> Vec<&candidates::Entry> {
    f.candidates.iter().filter(|e| e.step == Step::Run { command: "make lint".into() }).collect()
}

fn directs_lint(index: &Index) -> Vec<String> {
    index
        .routes
        .iter()
        .filter(|r| r.route.step == Step::Run { command: "make lint".into() })
        .map(|r| r.route.id.clone())
        .collect()
}

/// T-D1-02: with `min_support 5, border 2`, a two-event context seen in 3 or 4 traces is a
/// candidate; in 2, nothing; in 5, a route and no candidate.
#[test]
fn the_candidate_pass_keeps_the_border_only() {
    for (lint, expect) in [(2, false), (3, true), (4, true)] {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().join("home");
        let files = traces(d.path(), 10, lint);
        let index = condition(&files, &home, &cfg());
        let f = file(&home);
        assert_eq!(f.s_min, 5);
        let found = lint_candidates(&f);
        assert_eq!(!found.is_empty(), expect, "{lint} traces: {:#?}", f.candidates);
        for e in &found {
            assert_eq!(e.support, lint as u32);
            assert!(!e.review);
            assert_eq!(e.sessions.len(), lint, "{e:?}");
            assert_eq!(e.last_seen.as_deref(), Some(format!("2026-09-{lint:02}").as_str()));
            let Trigger::Context { items, .. } = &e.trigger else { panic!("{e:?}") };
            assert!((2..=3).contains(&items.len()), "a sequence: {items:?}");
        }
        assert!(directs_lint(&index).is_empty(), "{lint} traces make no route");
        // The index carries what the file holds, apart from the routes.
        assert_eq!(index.candidates(REPO).len(), f.valid().len());
        if expect {
            assert!(index.candidate(REPO, &found[0].id).is_some());
            assert!(!index.candidate_contexts(REPO).is_empty());
            // Every word of its context reaches the full path (the fast path never skips it).
            let Trigger::Context { items, .. } = &found[0].trigger else { unreachable!() };
            let words = &index.triggers(REPO).words;
            for it in items {
                if let adage_tracks::CtxItem::Act { keys } = it {
                    let w = adage_tracks::triggers::key_word(&keys[0]).unwrap();
                    assert!(words.contains(&w), "{w} in {words:?}");
                }
            }
        }
    }
}

/// T-D1-03: the candidate's id is the route id the same pattern gets at `s_min`; no candidate id
/// is ever a route; one more supporting trace makes it a route, and the learn's changes say `new`
/// with the trace that tipped it.
#[test]
fn a_candidate_keeps_its_id_when_it_forms() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("home");
    let tracks = home.join("tracks");
    let files = traces(d.path(), 10, 4);
    let index = condition(&files, &home, &cfg());
    let f = file(&home);
    let before_ids: Vec<String> = lint_candidates(&f).iter().map(|e| e.id.clone()).collect();
    assert!(!before_ids.is_empty(), "{:#?}", f.candidates);
    let route_ids: Vec<&str> = index.routes.iter().map(|r| r.route.id.as_str()).collect();
    for c in index.candidates.values().flatten() {
        assert!(!route_ids.contains(&c.id.as_str()), "{} is a route", c.id);
    }
    let changes_before = diff::snapshot(&tracks, REPO);
    assert!(changes_before.candidates.is_some());

    // The fifth trace with the linter after the tests.
    let files = traces(d.path(), 10, 5);
    let index = condition(&files, &home, &cfg());
    let after = diff::snapshot(&tracks, REPO);
    let formed = directs_lint(&index);
    assert_eq!(before_ids.len(), 1, "one candidate per habit: {before_ids:?}");
    assert!(formed.contains(&before_ids[0]), "one name: {} formed, among {formed:?}", before_ids[0]);
    assert!(lint_candidates(&file(&home)).is_empty(), "a route is no longer a candidate");
    let changes = diff::diff(&changes_before, &after);
    let new = changes
        .iter()
        .find(|c| c.route == before_ids[0] && c.change == diff::ChangeKind::New)
        .unwrap_or_else(|| panic!("{changes:#?}"));
    assert_eq!(new.sessions.as_deref(), Some(&["s04".to_string()][..]), "the trace that tipped it");
    let line = serde_json::to_value(new).unwrap();
    assert_eq!(line["sessions"], json!(["s04"]));
}

/// `learn.log`'s changes gain `change: "candidate"` with `support` and `s_min`, once per new or
/// moved candidate, with no command or path in the summary.
#[test]
fn a_new_candidate_is_a_change() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("home");
    let tracks = home.join("tracks");
    let before = diff::snapshot(&tracks, REPO);
    condition(&traces(d.path(), 10, 3), &home, &cfg());
    let after = diff::snapshot(&tracks, REPO);
    let changes = diff::diff(&before, &after);
    let id = lint_candidates(&file(&home))[0].id.clone();
    let c = changes.iter().find(|c| c.route == id).unwrap_or_else(|| panic!("{changes:#?}"));
    assert_eq!(c.change, diff::ChangeKind::Candidate);
    assert_eq!((c.support, c.s_min), (Some(3), Some(5)));
    assert_eq!(c.track, "acme-billing/learned");
    assert!(!c.summary.contains("make") && !c.summary.contains("acme/"), "{}", c.summary);
    let v = serde_json::to_value(c).unwrap();
    assert_eq!(
        (v["change"].as_str(), v["support"].as_u64(), v["s_min"].as_u64()),
        (Some("candidate"), Some(3), Some(5))
    );
    // The same candidates again: no change.
    condition(&traces(d.path(), 10, 3), &home, &cfg());
    let again = diff::diff(&after, &diff::snapshot(&tracks, REPO));
    assert!(!again.iter().any(|c| c.change == diff::ChangeKind::Candidate), "{again:#?}");
}

/// Candidates off: no file, and the routes are the same (the pass never touches them).
#[test]
fn the_routes_are_the_same_with_candidates_on_or_off() {
    let d = tempfile::tempdir().unwrap();
    let files = traces(d.path(), 10, 4);
    let (on, off) = (d.path().join("on"), d.path().join("off"));
    let a = condition(&files, &on, &cfg());
    let b = condition(&files, &off, &LearnConfig { candidates: false, ..cfg() });
    assert_eq!(a.routes, b.routes);
    assert_eq!(a.generation, b.generation, "candidates are not part of the generation");
    assert!(!a.candidates.is_empty() && b.candidates.is_empty());
    assert!(!candidates::path(&off.join("tracks"), REPO).exists());
}

/// T-D1-02, the rest: a one-event after-edit pattern seen in 4 traces, and a prompt family's
/// command seen in 4, are not candidates (a sequence, never a single call); at 5 traces the
/// linter after the tests is a route, and no candidate.
#[test]
fn a_single_call_is_never_a_candidate() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("home");
    let mut files = Vec::new();
    for i in 0..10u32 {
        let (prompt, step): (&str, fn(&mut Tx)) = match i {
            0..=3 => ("Add the due date to the invoice schema.", |t| {
                t.edit("schema/invoice.json").bash("make gen");
            }),
            4..=7 => ("Regenerate the documentation site.", |t| {
                t.bash("make docs");
            }),
            _ => ("Fix the rounding.", |t| {
                t.edit("acme/mod0.py").bash("make test");
            }),
        };
        let mut t = Tx::new(&format!("e{i:02}"), 1 + i, prompt);
        step(&mut t);
        files.push(t.write(d.path()));
    }
    condition(&files, &home, &cfg());
    let f = file(&home);
    let directs = |c: &str| f.candidates.iter().any(|e| e.step == Step::Run { command: c.into() });
    assert!(!directs("make gen") && !directs("make docs"), "{:#?}", f.candidates);

    let files = traces(d.path(), 10, 5);
    let home = d.path().join("five");
    let index = condition(&files, &home, &cfg());
    assert!(!directs_lint(&index).is_empty(), "5 traces: a route");
    assert!(lint_candidates(&file(&home)).is_empty(), "5 traces: no candidate");
}

/// T-D1-01 on the simulator's history (all three harnesses' sessions of the scenario): the routes
/// and the index generation are the same with the candidate pass on or off.
#[test]
fn the_sims_routes_are_the_same_with_candidates_on_or_off() {
    use adage_sim::{claude, scenario};
    let d = tempfile::tempdir().unwrap();
    let mut files = Vec::new();
    for seed in [adage_sim::CLAUDE_SEED, adage_sim::OPENCODE_SEED, adage_sim::CODEX_SEED] {
        for s in scenario::history(seed) {
            let p = d.path().join(format!("{seed}-{}.jsonl", claude::session_id(&s)));
            std::fs::write(&p, claude::transcript(&s, scenario::CWD)).unwrap();
            files.push(p);
        }
    }
    let (on, off) = (d.path().join("on"), d.path().join("off"));
    let a = condition(&files, &on, &LearnConfig::demo());
    let b = condition(&files, &off, &LearnConfig { candidates: false, ..LearnConfig::demo() });
    assert!(!a.routes.is_empty());
    assert_eq!(a.routes, b.routes);
    assert_eq!(a.generation, b.generation);
    assert_eq!(a.contexts, b.contexts, "the routes' context net");
}
