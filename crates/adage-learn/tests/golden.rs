//! End-to-end learner test over synthetic Claude Code transcripts in the real on-disk format.

use adage_learn::{learn, Inputs, LearnConfig};
use adage_tracks::{io, Aspect, Index, Step, Trigger};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

struct Tx {
    lines: Vec<Value>,
    sid: String,
    cwd: String,
    n: usize,
    turn: usize,
}

impl Tx {
    fn new(sid: &str, cwd: &str, prompt: &str) -> Tx {
        let mut t = Tx { lines: vec![], sid: sid.into(), cwd: cwd.into(), n: 0, turn: 0 };
        t.lines.push(json!({"type":"user","sessionId":sid,"cwd":cwd,"timestamp":"2026-09-20T10:00:00Z","message":{"role":"user","content":prompt}}));
        t
    }
    /// One model turn issuing one tool call, and its result.
    fn call(&mut self, name: &str, input: Value, output: &str, is_error: bool) -> &mut Self {
        self.n += 1;
        self.turn += 1;
        let id = format!("toolu_{}", self.n);
        self.lines.push(json!({"type":"assistant","sessionId":self.sid,"cwd":self.cwd,"message":{"id":format!("msg_{}", self.turn),"model":"claude-test","content":[{"type":"tool_use","id":id,"name":name,"input":input}]}}));
        self.lines.push(json!({"type":"user","sessionId":self.sid,"cwd":self.cwd,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":id,"content":output,"is_error":is_error}]}}));
        self
    }
    fn bash(&mut self, cmd: &str, out: &str, err: bool) -> &mut Self {
        self.call("Bash", json!({"command": cmd}), out, err)
    }
    fn write(&self, dir: &Path) -> PathBuf {
        let p = dir.join(format!("{}.jsonl", self.sid));
        std::fs::write(&p, self.lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n")).unwrap();
        p
    }
}

fn corpus(dir: &Path) -> (Vec<PathBuf>, String) {
    let cwd = "/fixture/acme-billing";
    let mut files = vec![];
    for i in 0..6 {
        let id = format!("W-10{:02}", 40 + i);
        let schema = i < 4;
        let mut t = Tx::new(&format!("s{i}"), cwd, &format!("Pick up {id} and get it done."));
        t.bash("ls", "README.md\nMakefile\nacme\nscripts", false)
            .bash(&format!("python3 scripts/wi-lookup {id}"), &format!("ID: {id}\nLinked files: acme/billing.py\nAcceptance: ok"), false)
            .call("Read", json!({"file_path": format!("{cwd}/acme/billing.py")}), "code", false);
        if schema {
            t.call("Edit", json!({"file_path": format!("{cwd}/schema/invoice.json")}), "ok", false)
                .bash("make gen", "generated", false);
        } else {
            t.call("Edit", json!({"file_path": format!("{cwd}/acme/billing.py")}), "ok", false);
        }
        // Different failing runners, one working command: must be learned as one dead end.
        let runner = ["python3 -m unittest discover -s tests", "python3 -m pytest tests", "python -m pytest -v"][i % 3];
        t.bash(runner, "Traceback\nRuntimeError: ACME_ENV is not set — run tests with 'make test'", true)
            .bash("make test", "OK", false);
        files.push(t.write(dir));
    }
    (files, cwd.to_string())
}

fn run(dir: &Path, files: &[PathBuf], home: &Path) -> Index {
    let inputs = Inputs { transcripts: files.to_vec(), outcomes: BTreeMap::new(), repo_override: Some("acme-billing".into()), models: None };
    learn(&inputs, &home.join("tracks"), &home.join("index.json"), &LearnConfig::demo()).unwrap();
    let _ = dir;
    Index::load(&home.join("index.json"))
}

#[test]
fn mines_all_four_route_kinds() {
    let d = tempfile::tempdir().unwrap();
    let (files, _) = corpus(d.path());
    let home = d.path().join("home");
    let index = run(d.path(), &files, &home);
    let routes: Vec<_> = index.routes.iter().map(|r| &r.route).collect();

    let lookup = routes.iter().find(|r| matches!(r.trigger, Trigger::Prompt { .. })).expect("prompt route");
    assert_eq!(lookup.step, Step::Run { command: "python3 scripts/wi-lookup {item}".into() });
    assert_eq!(lookup.aspect, Aspect::Green, "6/6 sessions clears the demo green bar");
    assert_eq!(format!("{:?}", lookup.effect), "Observe");

    let linked = routes.iter().find(|r| matches!(r.step, Step::ReadBatch { .. })).expect("linked-files route");
    assert!(matches!(linked.trigger, Trigger::AfterCommand { .. }));

    let dead = routes.iter().find(|r| matches!(r.step, Step::Avoid { .. })).expect("dead-end route");
    if let Step::Avoid { avoid, instead, signature } = &dead.step {
        assert!(avoid.contains("unittest") || avoid.contains("pytest"));
        assert_eq!(instead, "make test");
        assert!(signature.contains("ACME_ENV is not set"));
    }
    assert_eq!(dead.aspect, Aspect::Red);

    let fin = routes.iter().find(|r| matches!(r.trigger, Trigger::AfterEdit { .. })).expect("finish-line route");
    assert_eq!(fin.trigger, Trigger::AfterEdit { paths: "schema/*.json".into() });
    assert_eq!(fin.step, Step::Run { command: "make gen".into() });
    // Code edits must NOT pick up a make-gen obligation: only schema sessions ran it.
    assert!(!routes.iter().any(|r| r.trigger == Trigger::AfterEdit { paths: "acme/*.py".into() }));
}

#[test]
fn learning_is_deterministic_and_versioned() {
    let d = tempfile::tempdir().unwrap();
    let (files, _) = corpus(d.path());
    let home = d.path().join("home");
    run(d.path(), &files, &home);
    let first = std::fs::read(home.join("index.json")).unwrap();
    let t1 = io::read(&home.join("tracks/acme-billing/work-items")).unwrap();
    run(d.path(), &files, &home);
    assert_eq!(first, std::fs::read(home.join("index.json")).unwrap(), "same inputs, same index bytes");
    let t2 = io::read(&home.join("tracks/acme-billing/work-items")).unwrap();
    assert_eq!(t1.manifest.track.version, t2.manifest.track.version, "no change, no version bump");
}

#[test]
fn steered_sessions_are_excluded_only_from_their_own_route_kind() {
    let d = tempfile::tempdir().unwrap();
    let cwd = "/fixture/acme-billing";
    let (mut files, _) = corpus(d.path());
    // Three sessions steered by the lookup route, which then hit a dead end the route never
    // mentioned: they must count as dead-end evidence but not as lookup evidence.
    for i in 0..3 {
        let mut t = Tx::new(&format!("steered{i}"), cwd, "Pick up W-2099");
        t.lines.push(json!({"type":"attachment","sessionId":format!("steered{i}"),"cwd":cwd,
            "content":"<adage-signal track=\"x\" route=\"lookup-work-item\" aspect=\"green\">…</adage-signal>"}));
        t.bash("./scripts/other-lookup W-2099", "ok", false)
            .call("Edit", json!({"file_path": format!("{cwd}/acme/billing.py")}), "ok", false)
            .bash("python3 -m pytest", "No module named pytest; error", true)
            .bash("make test", "OK", false);
        files.push(t.write(d.path()));
    }
    let p = files.last().unwrap();
    let s = adage_learn::import::parse_transcript(p, &adage_core::CommandTable::load(), None).unwrap();
    assert!(s.steered && s.directed.contains("lookup-work-item"));
    assert!(!s.steps.is_empty(), "steps are kept; exclusion is per route kind");
    let home = d.path().join("home");
    let index = run(d.path(), &files, &home);
    let lookup = index.routes.iter().find(|r| r.route.id == "lookup-work-item").unwrap();
    assert_eq!(lookup.route.evidence.opportunities, 6, "steered sessions are not lookup opportunities");
    let dead = index.routes.iter().find(|r| r.route.id.starts_with("dead-end")).unwrap();
    assert_eq!(dead.route.evidence.support, 9, "steered sessions still count as dead-end evidence");
}

#[test]
fn a_publishing_variant_is_never_merged_into_a_lookup() {
    let d = tempfile::tempdir().unwrap();
    let cwd = "/fixture/acme";
    let mut files = vec![];
    for i in 0..6 {
        let id = format!("GH-1{i}");
        let mut t = Tx::new(&format!("g{i}"), cwd, &format!("handle {id}"));
        let cmd = if i == 0 { format!("gh issue view {id}") } else { format!("gh issue close {id}") };
        t.bash(&cmd, "ok", false).call("Edit", json!({"file_path": format!("{cwd}/a.py")}), "ok", false);
        files.push(t.write(d.path()));
    }
    let home = d.path().join("home");
    let index = run(d.path(), &files, &home);
    assert!(
        !index.routes.iter().any(|r| matches!(&r.route.step, Step::Run { command } if command.contains("close"))),
        "a publish command must never be directed"
    );
}
