//! Intent families end to end: synthetic Claude Code transcripts in the real on-disk format, a
//! small fixture embedding model in the installed layout, `adage condition`, then the runtime.

use adage_core::CommandTable;
use adage_intent::model::{self, StaticModel};
use adage_learn::{learn, Inputs, LearnConfig, Report};
use adage_runtime::{decide, Ctx, Event, SessionState};
use adage_tracks::{io, Aspect, Index, Step, Trigger};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const CWD: &str = "/fixture/acme-docs";

struct Tx {
    lines: Vec<Value>,
    sid: String,
    n: usize,
}

impl Tx {
    fn new(sid: &str, prompt: &str) -> Tx {
        let mut t = Tx { lines: vec![], sid: sid.into(), n: 0 };
        t.lines.push(json!({"type":"user","sessionId":sid,"cwd":CWD,"timestamp":"2026-09-21T10:00:00Z","message":{"role":"user","content":prompt}}));
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

/// A fixture model in the installed layout: test words along one axis, docs words along
/// another, chit-chat elsewhere, function words faint.
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
    for (j, w) in ["tests", "test", "failing", "fix", "broken", "red", "suite"].iter().enumerate() {
        vocab.push(w);
        rows.push(axis(0, 1.0, j % 2));
    }
    for (j, w) in ["docs", "documentation", "readme", "write", "update", "document"].iter().enumerate() {
        vocab.push(w);
        rows.push(axis(3, 1.0, j % 2));
    }
    for (j, w) in ["hello", "thanks", "coffee", "joke", "lunch", "weather"].iter().enumerate() {
        vocab.push(w);
        rows.push(axis(5 + j % 3, 1.0, 0));
    }
    for w in ["the", "are", "is", "please", "them", "again", "a", "for", "can", "you", "it", ",", "."] {
        vocab.push(w);
        rows.push(axis(2, 0.05, 0));
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

const TEST_PROMPTS: [&str; 8] = [
    "the tests are failing",
    "fix the failing tests please",
    "tests are red again",
    "the test suite is broken, fix it",
    "can you fix the broken tests",
    "failing tests, fix them",
    "fix the red test suite",
    "tests broken again",
];
const DOC_PROMPTS: [&str; 8] = [
    "write the docs",
    "update the readme",
    "please update the documentation",
    "document it",
    "write the readme",
    "update the docs please",
    "docs are broken again, update them",
    "write documentation for it",
];
const CHAT: [&str; 4] = ["hello", "thanks for the coffee", "a joke please", "lunch?"];

fn corpus(dir: &Path) -> Vec<PathBuf> {
    let mut files = vec![];
    for (i, p) in TEST_PROMPTS.iter().enumerate() {
        let mut t = Tx::new(&format!("t{i}"), p);
        t.bash("git status", "clean", false)
            .bash("make test", "FAILED tests/test_x.py::test_a", true)
            .call("Edit", json!({"file_path": format!("{CWD}/src/a.py")}), "ok", false)
            .bash("make test", "ok", false);
        files.push(t.write(dir));
    }
    for (i, p) in DOC_PROMPTS.iter().enumerate() {
        let mut t = Tx::new(&format!("d{i}"), p);
        t.bash("git status", "clean", false).bash("./scripts/show-docs", "docs/index.md\ndocs/api.md", false).call(
            "Edit",
            json!({"file_path": format!("{CWD}/docs/api.md")}),
            "ok",
            false,
        );
        files.push(t.write(dir));
    }
    for (i, p) in CHAT.iter().enumerate() {
        let mut t = Tx::new(&format!("c{i}"), p);
        t.bash("git status", "clean", false);
        files.push(t.write(dir));
    }
    files
}

fn run(files: &[PathBuf], home: &Path, models: Option<PathBuf>) -> (Report, Index) {
    let inputs = Inputs {
        transcripts: files.to_vec(),
        outcomes: BTreeMap::new(),
        repo_override: Some("acme-docs".into()),
        models,
        cache: None,
        only_repo: None,
    };
    let rep = learn(&inputs, &home.join("tracks"), &home.join("index.json"), &LearnConfig::demo()).unwrap();
    (rep, Index::load(&home.join("index.json")))
}

fn intents(index: &Index) -> Vec<&adage_tracks::Route> {
    index.routes.iter().map(|r| &r.route).filter(|r| matches!(r.trigger, Trigger::Intent { .. })).collect()
}

#[test]
fn mines_intent_routes_and_the_runtime_matches_new_phrasings() {
    let d = tempfile::tempdir().unwrap();
    let files = corpus(d.path());
    let models = d.path().join("models");
    install_fixture_model(&models);
    let home = d.path().join("home");
    let (rep, index) = run(&files, &home, Some(models.clone()));
    assert_eq!(rep.intent_model.as_deref(), Some("fixture@000000000000"));
    assert_eq!(rep.repos["acme-docs"].intent_families, 2, "chit-chat never becomes a usable family");

    let found = intents(&index);
    let cmds: Vec<String> = found
        .iter()
        .map(|r| match &r.step {
            Step::Run { command } => command.clone(),
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(found.len(), 2, "{cmds:?}");
    assert!(cmds.contains(&"make test".to_string()), "{cmds:?}");
    assert!(cmds.contains(&"scripts/show-docs".to_string()), "{cmds:?}");
    assert!(!cmds.iter().any(|c| c.contains("git status")), "a command every session runs is not an intent's route");
    for r in &found {
        let Trigger::Intent { model, threshold, centroid, family } = &r.trigger else { unreachable!() };
        assert_eq!(model, "fixture@000000000000");
        assert!((0.5..=0.99).contains(threshold));
        assert_eq!(centroid.len(), 8);
        assert_eq!(family, &r.id);
        assert!(r.slots.is_empty());
        assert_eq!(r.evidence.support, 8);
        assert_eq!(r.aspect, Aspect::Green, "8/8 sessions clears the demo green bar");
    }
    // The track on disk validates as untrusted input.
    io::read(&home.join("tracks/acme-docs/intents")).unwrap();

    // Runtime: a phrasing never seen in training finds its family; chit-chat finds nothing.
    let m = StaticModel::open(&model::dir_in(&models)).unwrap();
    let embed = |t: &str| -> Option<(String, Vec<f32>)> {
        Some((m.id.clone(), m.embed(&adage_intent::normalize::normalize(t))?))
    };
    let root = tempfile::tempdir().unwrap();
    let table = CommandTable::load();
    let ctx = Ctx {
        index: &index,
        repo: "acme-docs".into(),
        root: root.path(),
        table: &table,
        embed: Some(&embed),
        params: Default::default(),
    };
    let d1 = decide(&ctx, &mut SessionState::default(), &Event::Prompt { text: "Broken tests again! Fix?".into() });
    assert!(d1.directives.iter().any(|x| x.text.contains("make test")), "{:?}", d1.directives);
    let d2 =
        decide(&ctx, &mut SessionState::default(), &Event::Prompt { text: "Could you update the README docs".into() });
    assert!(d2.directives.iter().any(|x| x.text.contains("scripts/show-docs")), "{:?}", d2.directives);
    let d3 = decide(&ctx, &mut SessionState::default(), &Event::Prompt { text: "thanks, coffee?".into() });
    assert!(d3.directives.is_empty());
}

#[test]
fn intent_learning_is_deterministic_and_order_independent() {
    let d = tempfile::tempdir().unwrap();
    let files = corpus(d.path());
    let models = d.path().join("models");
    install_fixture_model(&models);
    let (a, b, c) = (d.path().join("a"), d.path().join("b"), d.path().join("c"));
    run(&files, &a, Some(models.clone()));
    run(&files, &b, Some(models.clone()));
    let mut reversed = files.clone();
    reversed.reverse();
    run(&reversed, &c, Some(models.clone()));
    let bytes = |h: &Path| std::fs::read(h.join("index.json")).unwrap();
    assert_eq!(bytes(&a), bytes(&b), "same input, same bytes");
    assert_eq!(bytes(&a), bytes(&c), "reordered input, same families and bytes");
    // Relearning the same sessions changes nothing, not even the version.
    run(&files, &a, Some(models));
    assert_eq!(bytes(&a), bytes(&b));
}

#[test]
fn without_a_model_learning_falls_back_and_keeps_existing_intent_tracks() {
    let d = tempfile::tempdir().unwrap();
    let files = corpus(d.path());
    // No model directory, and a damaged one: both mean no intent families, and nothing else changes.
    let broken = d.path().join("broken-models");
    std::fs::create_dir_all(model::dir_in(&broken)).unwrap();
    std::fs::write(model::dir_in(&broken).join("manifest.json"), "{not json").unwrap();
    for models in [None, Some(broken)] {
        let home = d.path().join(format!("home-{}", models.is_some()));
        let (rep, index) = run(&files, &home, models);
        assert!(rep.intent_model.is_none());
        assert!(intents(&index).is_empty());
        assert!(!home.join("tracks/acme-docs/intents").exists());
    }
    // Learned with a model, then relearned without one: the intent track is left as it was.
    let models = d.path().join("models");
    install_fixture_model(&models);
    let home = d.path().join("home");
    let (_, with) = run(&files, &home, Some(models));
    let (_, without) = run(&files, &home, None);
    assert_eq!(intents(&with).len(), 2);
    assert_eq!(with.routes, without.routes);
}
