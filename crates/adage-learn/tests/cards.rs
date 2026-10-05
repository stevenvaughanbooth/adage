//! Procedure cards for recurring work end to end: synthetic Claude Code transcripts in the real
//! on-disk format, a small fixture embedding model in the installed layout, learning, then the
//! runtime's delivery at the prompt.

use adage_core::{CommandTable, Effect};
use adage_intent::model::{self, StaticModel};
use adage_learn::cards::CardParams;
use adage_learn::{learn, Inputs, LearnConfig, Report};
use adage_runtime::{decide, Ctx, Event, SessionState};
use adage_tracks::{io, Index, Step, Strength, Trigger};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const CWD: &str = "/fixture/acme-lib";

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
    fn edit(&mut self, rel: &str, old: &str, new: &str) -> &mut Self {
        self.call(
            "Edit",
            json!({"file_path": format!("{CWD}/{rel}"), "old_string": old, "new_string": new}),
            "ok",
            false,
        )
    }
    fn write(&self, dir: &Path) -> PathBuf {
        let p = dir.join(format!("{}.jsonl", self.sid));
        std::fs::write(&p, self.lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n")).unwrap();
        p
    }
}

/// Validator words along one axis, docs words along another, chit-chat elsewhere.
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
    for (j, w) in ["add", "validator", "new", "validate", "rule", "check"].iter().enumerate() {
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
    for w in ["the", "a", "for", "please", "that", "it", ",", "."] {
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

const VALIDATOR: [(&str, &str); 3] = [
    ("v0", "add a new validator for a rule"),
    ("v1", "add a validator that will check it"),
    ("v2", "please add a new validate rule"),
];

/// Three validator sessions (the same files, the same checks), two docs sessions, chit-chat.
fn corpus(dir: &Path) -> (Vec<PathBuf>, BTreeMap<String, bool>) {
    let mut files = vec![];
    let mut outcomes = BTreeMap::new();
    for (i, (sid, p)) in VALIDATOR.iter().enumerate() {
        let name = ["Even", "Odd", "Prime"][i];
        let mut t = Tx::new(sid, p);
        t.bash("grep -n __all__ src/validate.py", "12:__all__", false)
            .bash("python -m pytest tests/test_validate.py -q", "No module named pytest", true)
            .bash("python -c \"import sys; print(sys.path)\"", "[]", false)
            .edit(
                "src/validate.py",
                "",
                &format!("class {name}(Validator):\n    default_message = \"Must be {name}.\""),
            )
            .bash("make check", "1 failed", true)
            .edit("tests/test_validate.py", "", &format!("def test_{name}():\n    assert {name}()(2)"))
            .edit("CHANGELOG.rst", "", &format!("- Add `{name}` validator."))
            .bash("make check", "ok", false)
            .edit("src/validate.py", "x", "y")
            .bash("make check", "ok", false);
        files.push(t.write(dir));
        outcomes.insert(sid.to_string(), true);
    }
    for (sid, p) in [("d0", "update the readme docs"), ("d1", "write documentation for the readme")] {
        let mut t = Tx::new(sid, p);
        t.edit("README.md", "", "More docs.").bash("make docs", "ok", false);
        files.push(t.write(dir));
    }
    for (sid, p) in [("c0", "hello, thanks for the coffee"), ("c1", "a joke please")] {
        let mut t = Tx::new(sid, p);
        t.bash("git status", "clean", false);
        files.push(t.write(dir));
    }
    (files, outcomes)
}

fn run(files: &[PathBuf], outcomes: &BTreeMap<String, bool>, home: &Path, models: &Path) -> (Report, Index) {
    let inputs = Inputs {
        transcripts: files.to_vec(),
        outcomes: outcomes.clone(),
        repo_override: Some("acme-lib".into()),
        models: Some(models.to_path_buf()),
        cache: None,
        only_repo: None,
    };
    let cfg =
        LearnConfig { cards: Some(CardParams { threshold: 0.8, ..CardParams::default() }), ..LearnConfig::demo() };
    let rep = learn(&inputs, &home.join("tracks"), &home.join("index.json"), &cfg).unwrap();
    (rep, Index::load(&home.join("index.json")))
}

fn cards(index: &Index) -> Vec<&adage_tracks::Route> {
    index.routes.iter().map(|r| &r.route).filter(|r| matches!(r.step, Step::Card { .. })).collect()
}

#[test]
fn a_family_with_a_passing_trace_gets_a_card_delivered_once_on_its_own_kind_of_task() {
    let d = tempfile::tempdir().unwrap();
    let (files, outcomes) = corpus(d.path());
    let models = d.path().join("models");
    install_fixture_model(&models);
    let home = d.path().join("home");
    let (rep, index) = run(&files, &outcomes, &home, &models);
    let repo = &rep.repos["acme-lib"];
    assert_eq!(repo.engine.cards, 1, "the docs family has no passing trace, chit-chat edits nothing");
    assert!(repo.engine.card_families >= 2, "{}", repo.engine.card_families);

    let found = cards(&index);
    assert_eq!(found.len(), 1);
    let r = found[0];
    assert!(r.id.starts_with("card-"));
    assert!(matches!(r.trigger, Trigger::Intent { .. }));
    assert_eq!(r.strength, Strength::Prime, "a card is advice, never fired");
    assert_eq!(r.effect, Effect::Observe);
    let Step::Card { card } = &r.step else { unreachable!() };
    assert_eq!((card.sessions, card.passed), (3, 3));
    let paths: Vec<&str> = card.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["src/validate.py", "tests/test_validate.py", "CHANGELOG.rst"], "in the order first edited");
    let cmds: Vec<&str> = card.commands.iter().map(|c| c.command.as_str()).collect();
    assert_eq!(cmds, ["make check"], "productive commands only: no `grep` (fresh-install B9)");
    assert!(!cmds.iter().any(|c| c.starts_with("python -c")), "an inline script is not a command: {cmds:?}");
    assert_eq!(card.avoid.len(), 1);
    assert_eq!(card.avoid[0].command, "python -m pytest tests/test_validate.py -q");
    assert!(card.avoid[0].error.contains("No module named pytest"));
    assert_eq!(card.finish.as_deref(), Some("make check"));
    assert!(card.exemplars.iter().any(|e| e.lines.iter().any(|l| l.contains("class Prime"))), "the latest pass");
    // The track on disk validates as untrusted input.
    io::read(&home.join("tracks/acme-lib/learned")).unwrap();

    let m = StaticModel::open(&model::dir_in(&models)).unwrap();
    let embed = |t: &str| -> Option<(String, Vec<f32>)> {
        Some((m.id.clone(), m.embed(&adage_intent::normalize::normalize(t))?))
    };
    let root = tempfile::tempdir().unwrap();
    let table = CommandTable::load();
    let ctx = Ctx {
        checkout: &adage_runtime::checkout::Disk,
        index: &index,
        repo: "acme-lib".into(),
        root: root.path(),
        table,
        embed: Some(&embed),
        params: Default::default(),
        policy: Default::default(),
    };
    let said = |d: &adage_runtime::Decision| d.directives.iter().any(|x| x.route == r.id);
    // A new task of the family, phrased anew: the card, once.
    let mut st = SessionState::default();
    let d1 = decide(&ctx, &mut st, &Event::Prompt { text: "Add a validator: check rule".into() });
    assert!(said(&d1), "{:?}", d1.directives);
    let text = &d1.directives.iter().find(|x| x.route == r.id).unwrap().text;
    assert!(text.contains("src/validate.py") && text.contains("make check"), "{text}");
    let d2 = decide(&ctx, &mut st, &Event::Prompt { text: "add a new validator rule".into() });
    assert!(!said(&d2), "said once per session");
    // Unrelated tasks: nothing.
    for p in ["update the readme docs", "thanks, coffee?", "hello weather"] {
        let d = decide(&ctx, &mut SessionState::default(), &Event::Prompt { text: p.into() });
        assert!(!said(&d), "{p}: {:?}", d.directives);
    }
}

/// Review B3, the reviewer's scenario: the user pasted an API key and the agent wrote it into
/// `.env` and into a settings file the family edits every time, and ran `gh` with a token. The card
/// is learned, and nothing of it reaches the card, the track or the index. Fresh-install B9 too:
/// `git status` and `ls -la` are not commands, the test run that worked is listed (as one command,
/// though each trace named its own test), and exemplars are whole hunks.
#[test]
fn a_card_never_quotes_secrets_or_housekeeping() {
    let d = tempfile::tempdir().unwrap();
    let models = d.path().join("models");
    install_fixture_model(&models);
    let key = format!("sk-{}", "abcDEF123_".repeat(3));
    let token = format!("ghp_{}", "A1b2C3d4E5".repeat(4));
    let mut files = vec![];
    let mut outcomes = BTreeMap::new();
    for (i, (sid, p)) in VALIDATOR.iter().enumerate() {
        let name = ["Even", "Odd", "Prime"][i];
        let mut t = Tx::new(sid, p);
        t.bash("git status", "clean", false)
            .bash("ls -la", "total 0", false)
            .bash("python -m pytest tests/test_validate.py -q", "No module named pytest", true)
            .edit(".env", "", &format!("OPENAI_API_KEY={key}"))
            .edit(
                "src/validate.py",
                "",
                &format!(
                    "OPENAI_API_KEY = \"{key}\"\nclass {name}(Validator):\n    if strict:\n        default_message = \"Must be {name}.\""
                ),
            )
            .bash(&format!("echo \"OPENAI_API_KEY={key}\" > .env"), "", false)
            .bash(&format!("GITHUB_TOKEN={token} gh pr create"), "", false)
            .bash(&format!(".venv/bin/python -m pytest tests/test_{} -q", name.to_lowercase()), "1 passed in 0.1s", false)
            .edit("tests/test_validate.py", "", &format!("def test_{name}():\n    assert {name}()(2)"));
        files.push(t.write(d.path()));
        outcomes.insert(sid.to_string(), true);
    }
    let home = d.path().join("home");
    let (_, index) = run(&files, &outcomes, &home, &models);
    let found = cards(&index);
    assert_eq!(found.len(), 1);
    let Step::Card { card } = &found[0].step else { unreachable!() };
    let all = serde_json::to_string(card).unwrap();
    assert!(!all.contains(&key) && !all.contains(&token), "no secret in the card: {all}");
    assert!(!all.contains(".env"), "no secret-holding file: {all}");
    for dir in ["tracks", ""] {
        for e in walk(&home.join(dir)) {
            let text = std::fs::read_to_string(&e).unwrap_or_default();
            assert!(!text.contains(&key) && !text.contains(&token), "{}", e.display());
        }
    }
    let cmds: Vec<&str> = card.commands.iter().map(|c| c.command.as_str()).collect();
    assert_eq!(cmds, [".venv/bin/python -m pytest"], "{cmds:?}");
    assert_eq!(card.avoid[0].command, "python -m pytest tests/test_validate.py -q");
    let ex = card.exemplars.iter().find(|e| e.path == "src/validate.py").unwrap();
    assert_eq!(
        ex.lines,
        ["…", "class Prime(Validator):", "    if strict:", "        default_message = \"Must be Prime.\""]
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(home.join("index.json")).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "the index is the owner's only");
        for e in walk(&home.join("tracks")).into_iter().filter(|p| p.extension().is_some_and(|x| x == "yaml")) {
            assert_eq!(std::fs::metadata(&e).unwrap().permissions().mode() & 0o777, 0o600, "{}", e.display());
        }
    }
}

/// Review S8: one passing trace is any one-off bug fix; it makes no card, even one that created
/// no file (it did before, with a grey zone down to 0.653).
#[test]
fn one_trace_is_not_a_family() {
    let d = tempfile::tempdir().unwrap();
    let (files, outcomes) = corpus(d.path());
    let models = d.path().join("models");
    install_fixture_model(&models);
    let (rep, index) = run(&files[..1], &outcomes, &d.path().join("one"), &models);
    assert_eq!(rep.repos["acme-lib"].engine.cards, 0);
    assert!(cards(&index).is_empty());
    let (_, index) = run(&files[..2], &outcomes, &d.path().join("two"), &models);
    assert_eq!(cards(&index).len(), 1, "two traces of the family are");
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

#[test]
fn no_passing_trace_no_card_and_cards_are_off_without_the_setting() {
    let d = tempfile::tempdir().unwrap();
    let (files, outcomes) = corpus(d.path());
    let models = d.path().join("models");
    install_fixture_model(&models);
    let failed: BTreeMap<String, bool> = outcomes.keys().map(|k| (k.clone(), false)).collect();
    let (rep, index) = run(&files, &failed, &d.path().join("a"), &models);
    assert_eq!(rep.repos["acme-lib"].engine.cards, 0);
    assert!(cards(&index).is_empty());
    // The default configuration learns no cards (the eval baselines stay as they were).
    let inputs = Inputs {
        transcripts: files.clone(),
        outcomes,
        repo_override: Some("acme-lib".into()),
        models: Some(models),
        cache: None,
        only_repo: None,
    };
    let home = d.path().join("b");
    learn(&inputs, &home.join("tracks"), &home.join("index.json"), &LearnConfig::demo()).unwrap();
    assert!(cards(&Index::load(&home.join("index.json"))).is_empty());
}

#[test]
fn cards_are_deterministic_and_order_independent() {
    let d = tempfile::tempdir().unwrap();
    let (files, outcomes) = corpus(d.path());
    let models = d.path().join("models");
    install_fixture_model(&models);
    let (a, b) = (d.path().join("a"), d.path().join("b"));
    run(&files, &outcomes, &a, &models);
    let mut reversed = files.clone();
    reversed.reverse();
    run(&reversed, &outcomes, &b, &models);
    let bytes = |h: &Path| std::fs::read(h.join("index.json")).unwrap();
    assert_eq!(bytes(&a), bytes(&b));
}

/// Go-like recurring work: each task creates files named after it (`flag_port.go`), edits a
/// shared doc, and regenerates a file by redirection.
fn go_corpus(dir: &Path, names: &[&str]) -> (Vec<PathBuf>, BTreeMap<String, bool>) {
    let mut files = vec![];
    let mut outcomes = BTreeMap::new();
    for (i, name) in names.iter().enumerate() {
        let sid = format!("g{i}");
        let mut t = Tx::new(&sid, &format!("add a new check rule flag {name}"));
        let write = |rel: String, content: String| json!({"file_path": format!("{CWD}/{rel}"), "content": content});
        t.bash("go test ./...", "FAIL", true)
            .call(
                "Write",
                write(format!("flag_{}.go", name.to_lowercase()), format!("package cli\n\ntype {name}Flag = FlagBase")),
                "File created",
                false,
            )
            .call(
                "Write",
                write(
                    format!("flag_{}_test.go", name.to_lowercase()),
                    format!("package cli\n\nfunc Test{name}Flag(t *testing.T) {{}}"),
                ),
                "File created",
                false,
            )
            .bash("go test ./...", "FAIL", true)
            .edit("docs/basics.md", "", &format!("- {name}Flag"))
            .bash(&format!("cd {CWD} && go doc -all . > godoc-current.txt"), "", false)
            .bash("go test ./...", "ok", false);
        files.push(t.write(dir));
        outcomes.insert(sid, true);
    }
    (files, outcomes)
}

#[test]
fn names_that_vary_become_patterns_and_one_trace_that_created_files_is_not_enough() {
    let d = tempfile::tempdir().unwrap();
    let models = d.path().join("models");
    install_fixture_model(&models);
    let (files, outcomes) = go_corpus(d.path(), &["Port", "Url", "Regexp"]);
    let (_, index) = run(&files, &outcomes, &d.path().join("a"), &models);
    let found = cards(&index);
    assert_eq!(found.len(), 1);
    let Step::Card { card } = &found[0].step else { unreachable!() };
    let paths: Vec<&str> = card.files.iter().map(|f| f.path.as_str()).collect();
    for p in ["flag_<x>.go", "flag_<x>_test.go", "docs/basics.md", "godoc-current.txt"] {
        assert!(paths.contains(&p), "{p} in {paths:?}");
    }
    let ex = |p: &str| card.exemplars.iter().find(|e| e.path == p).map(|e| e.lines.join("\n")).unwrap_or_default();
    assert!(
        ex("flag_<x>.go").contains("type RegexpFlag"),
        "the latest trace, not its test file: {}",
        ex("flag_<x>.go")
    );
    assert!(ex("flag_<x>_test.go").contains("func TestRegexpFlag"));
    assert!(ex("godoc-current.txt").contains("go doc -all ."), "a redirect's text is its command");
    assert_eq!(card.finish.as_deref(), Some("go test ./..."));
    assert!(!card.prompts.is_empty(), "the family's prompts (identical ones kept once), to score a new one against");
    let Trigger::Intent { threshold, .. } = &found[0].trigger else { unreachable!() };
    assert!((threshold - 0.8).abs() < 1e-6);

    // One trace that created files: no card yet (which part of the name varies is unknown).
    let (_, index) = run(&files[..1], &outcomes, &d.path().join("b"), &models);
    assert!(cards(&index).is_empty());
}

/// chi-like recurring work: each task creates a middleware file named whole after itself
/// (`middleware/no_sniff.go`, `middleware/hsts.go`: no shared name shape), its test, and adds a
/// line to the README. The showcase dry run formed no card for this family.
#[test]
fn files_named_whole_after_each_task_group_by_role() {
    let d = tempfile::tempdir().unwrap();
    let models = d.path().join("models");
    install_fixture_model(&models);
    let mut files = vec![];
    let mut outcomes = BTreeMap::new();
    for (i, (name, file)) in
        [("NoSniff", "no_sniff"), ("Hsts", "hsts"), ("RealIPHeader", "real_ip_header")].iter().enumerate()
    {
        let sid = format!("c{i}");
        let mut t = Tx::new(&sid, &format!("add a new check rule flag {name}"));
        let write = |rel: String, content: String| json!({"file_path": format!("{CWD}/{rel}"), "content": content});
        t.call(
            "Write",
            write(
                format!("middleware/{file}.go"),
                format!("package middleware\n\nfunc {name}(next http.Handler) http.Handler"),
            ),
            "File created",
            false,
        )
        .call(
            "Write",
            write(
                format!("middleware/{file}_test.go"),
                format!("package middleware\n\nfunc Test{name}(t *testing.T) {{}}"),
            ),
            "File created",
            false,
        )
        .edit("README.md", "", &format!("| {name} | …"))
        .bash("go test ./...", "ok", false);
        files.push(t.write(d.path()));
        outcomes.insert(sid, true);
    }
    let (_, index) = run(&files, &outcomes, &d.path().join("a"), &models);
    let found = cards(&index);
    assert_eq!(found.len(), 1, "one family, one card");
    let Step::Card { card } = &found[0].step else { unreachable!() };
    let paths: Vec<&str> = card.files.iter().map(|f| f.path.as_str()).collect();
    for p in ["middleware/<x>.go", "middleware/<x>_test.go", "README.md"] {
        assert!(paths.contains(&p), "{p} in {paths:?}");
    }
    let ex = |p: &str| card.exemplars.iter().find(|e| e.path == p).map(|e| e.lines.join("\n")).unwrap_or_default();
    assert!(
        ex("middleware/<x>.go").contains("func RealIPHeader"),
        "the latest trace's file: {}",
        ex("middleware/<x>.go")
    );
    assert!(ex("middleware/<x>_test.go").contains("func TestRealIPHeader"));
    assert!(adage_tracks::card::touches(card, "middleware/strip_slashes.go"));
    assert!(!adage_tracks::card::touches(card, "other/strip_slashes.go"));
}
