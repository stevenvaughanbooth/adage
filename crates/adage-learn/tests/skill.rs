//! T-D3-05: a `skill` reflex learned from traces (plan §2.1.2). Synthetic Claude Code
//! transcripts in the real on-disk format: requests of one family ("cut a release"), a version
//! bump, then the Skill tool; a small fixture embedding model in the installed layout.

use adage_intent::model;
use adage_learn::{learn, Inputs, LearnConfig};
use adage_tracks::{Index, Step, Strength, Trigger};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const CWD: &str = "/fixture/acme-release";

struct Tx {
    lines: Vec<Value>,
    sid: String,
    n: usize,
}

impl Tx {
    fn new(sid: &str, prompt: &str) -> Tx {
        let mut t = Tx { lines: vec![], sid: sid.into(), n: 0 };
        t.lines.push(json!({"type":"user","sessionId":sid,"cwd":CWD,"timestamp":format!("2026-09-{:02}T10:00:00Z", 1 + t.sid.len() % 20),"message":{"role":"user","content":prompt}}));
        t
    }
    fn call(&mut self, name: &str, input: Value, output: &str, is_error: bool) -> &mut Self {
        self.n += 1;
        let id = format!("toolu_{}", self.n);
        self.lines.push(json!({"type":"assistant","sessionId":self.sid,"cwd":CWD,"message":{"id":format!("msg_{}", self.n),"model":"claude-test","content":[{"type":"tool_use","id":id,"name":name,"input":input}]}}));
        self.lines.push(json!({"type":"user","sessionId":self.sid,"cwd":CWD,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":id,"content":output,"is_error":is_error}]}}));
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

/// Release words along one axis, test words along another, function words faint.
fn install_fixture_model(models: &Path) {
    let dim = 8;
    let axis = |i: usize, w: f32, jitter: usize| {
        let mut v = vec![0f32; dim];
        v[i] = w;
        v[(i + 1 + jitter) % dim] += 0.15;
        v
    };
    let mut vocab: Vec<&str> = vec!["[UNK]"];
    let mut rows: Vec<Vec<f32>> = vec![vec![0.0; dim]];
    for (j, w) in ["release", "cut", "ship", "version", "publish", "tag"].iter().enumerate() {
        vocab.push(w);
        rows.push(axis(0, 1.0, j % 2));
    }
    for (j, w) in ["tests", "test", "failing", "fix", "broken", "red"].iter().enumerate() {
        vocab.push(w);
        rows.push(axis(3, 1.0, j % 2));
    }
    for w in ["the", "a", "new", "please", "it", "again", "are", "is", "now"] {
        vocab.push(w);
        rows.push(axis(6, 0.05, 0));
    }
    model::write_installed(
        &model::dir_in(models),
        "fixture@000000000000",
        dim,
        0,
        100,
        &vocab.join("\n"),
        &model::safetensors_bytes(&rows),
    )
    .unwrap();
}

const RELEASE: [&str; 8] = [
    "cut a release",
    "cut a new release please",
    "ship a release",
    "release it now",
    "tag and ship the release",
    "cut the release again",
    "publish a new version",
    "release a new version please",
];
const TESTS: [&str; 6] = [
    "the tests are failing",
    "fix the failing tests",
    "tests are red again",
    "fix the broken test",
    "failing tests",
    "fix it, tests are red",
];

/// A directive that said to use the skill, as a hook delivered it.
fn steer(t: &mut Tx) {
    t.lines.push(json!({"type":"attachment","sessionId":t.sid,"cwd":CWD,
        "attachment":{"type":"hook_additional_context","content":[format!(
            "<adage-reflex track=\"x\" route=\"intent-x\" strength=\"prime\" kind=\"skill\">\n{}\n</adage-reflex>",
            adage_trace::wire::skill_sentence("release-checklist"))]}}));
}

/// `releases` release traces (the first `steered` of them steered by the skill directive), and
/// the test traces.
fn corpus(dir: &Path, releases: usize, steered: usize) -> Vec<PathBuf> {
    let mut files = vec![];
    for (i, p) in RELEASE.iter().take(releases).enumerate() {
        let mut t = Tx::new(&format!("r{i}"), p);
        if i < steered {
            steer(&mut t);
        }
        t.bash("git status", "clean", false)
            .bash("npm version minor", "v1.3.0", false)
            .call("Skill", json!({"skill": "release-checklist"}), "Launching skill: release-checklist", false)
            .call("Edit", json!({"file_path": format!("{CWD}/CHANGELOG.md")}), "ok", false);
        files.push(t.write(dir));
    }
    for (i, p) in TESTS.iter().enumerate() {
        let mut t = Tx::new(&format!("t{i}"), p);
        t.bash("git status", "clean", false)
            .bash("make test", "FAILED", true)
            .call("Edit", json!({"file_path": format!("{CWD}/src/a.py")}), "ok", false)
            .bash("make test", "ok", false);
        files.push(t.write(dir));
    }
    files
}

fn run(files: &[PathBuf], home: &Path, models: &Path) -> Index {
    let inputs = Inputs {
        identities: Default::default(),
        transcripts: files.to_vec(),
        outcomes: BTreeMap::new(),
        repo_override: Some("acme-release".into()),
        models: Some(models.to_path_buf()),
        cache: None,
        only_repo: None,
    };
    // Uplift gating off: every trace of the family took the skill unaided, so with the default bar
    // the reflex is learned quiet (nascent until a person approves it), as any reflex agents
    // already follow; what is under test here is that it is learned, and how.
    let cfg = LearnConfig { uplift_min: 0.0, ..LearnConfig::default() };
    learn(&inputs, &home.join("tracks"), &home.join("index.json"), &cfg).unwrap();
    Index::load(&home.join("index.json"))
}

fn skill_routes(index: &Index) -> Vec<&adage_tracks::Route> {
    index.routes.iter().map(|r| &r.route).filter(|r| matches!(r.step, Step::Skill { .. })).collect()
}

#[test]
fn a_skill_reflex_is_learned_from_six_traces_and_not_from_four() {
    let d = tempfile::tempdir().unwrap();
    let models = d.path().join("models");
    install_fixture_model(&models);

    let index = run(&corpus(d.path(), 6, 0), &d.path().join("six"), &models);
    let skills = skill_routes(&index);
    assert_eq!(skills.len(), 1, "{:#?}", index.routes.iter().map(|r| (&r.route.id, &r.route.step)).collect::<Vec<_>>());
    let r = skills[0];
    assert_eq!(r.step, Step::Skill { skill: "release-checklist".into(), args: None });
    assert!(matches!(r.trigger, Trigger::Intent { .. }), "the family's intent trigger: {:?}", r.trigger);
    assert_eq!(r.strength, Strength::Prime, "never above the skill ceiling");
    assert_eq!(r.effect, adage_core::Effect::Observe);
    assert_eq!(r.evidence.support, 6);
    assert_eq!(adage_tracks::shape(r), "skill");

    let d4 = d.path().join("four-traces");
    std::fs::create_dir_all(&d4).unwrap();
    let index = run(&corpus(&d4, 4, 0), &d.path().join("four"), &models);
    assert!(skill_routes(&index).is_empty(), "four traces are fewer than a reflex needs");
}

/// A trace steered by the skill directive is no evidence for that skill (six traces, two
/// steered: four of its own), and the directive steered that skill alone: the trace's commands
/// are still its own.
#[test]
fn a_trace_steered_to_the_skill_is_no_evidence_for_it() {
    let d = tempfile::tempdir().unwrap();
    let models = d.path().join("models");
    install_fixture_model(&models);
    let files = corpus(d.path(), 6, 2);
    let s = adage_learn::import::parse_transcript(&files[0], adage_core::CommandTable::load(), None).unwrap();
    assert!(s.steered);
    assert_eq!(s.influence[0].skills, ["release-checklist"]);
    assert!(s.influence[0].commands.is_empty() && !s.influence[0].all, "{:?}", s.influence[0]);
    let index = run(&files, &d.path().join("home"), &models);
    assert!(skill_routes(&index).is_empty(), "{:#?}", index.routes.iter().map(|r| &r.route.id).collect::<Vec<_>>());
}
