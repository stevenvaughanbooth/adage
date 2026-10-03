//! End-to-end learner test over synthetic Claude Code transcripts in the real on-disk format.

use adage_learn::{learn, Inputs, LearnConfig};
use adage_tracks::{io, Exit, Index, Step, Strength, Trigger};
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
            .bash(
                &format!("python3 scripts/wi-lookup {id}"),
                &format!("ID: {id}\nLinked files: acme/billing.py\nAcceptance: ok"),
                false,
            )
            .call("Read", json!({"file_path": format!("{cwd}/acme/billing.py")}), "code", false);
        if schema {
            t.call("Edit", json!({"file_path": format!("{cwd}/schema/invoice.json")}), "ok", false).bash(
                "make gen",
                "generated",
                false,
            );
        } else {
            t.call("Edit", json!({"file_path": format!("{cwd}/acme/billing.py")}), "ok", false);
        }
        // Different failing runners, one working command: must be learned as one dead end.
        let runner = ["python3 -m unittest discover -s tests", "python3 -m pytest tests", "python -m pytest -v"][i % 3];
        t.bash(runner, "Traceback\nRuntimeError: ACME_ENV is not set — run tests with 'make test'", true).bash(
            "make test",
            "OK",
            false,
        );
        files.push(t.write(dir));
    }
    (files, cwd.to_string())
}

fn run(dir: &Path, files: &[PathBuf], home: &Path) -> Index {
    let inputs = Inputs {
        transcripts: files.to_vec(),
        outcomes: BTreeMap::new(),
        repo_override: Some("acme-billing".into()),
        models: None,
        cache: None,
        only_repo: None,
    };
    learn(&inputs, &home.join("tracks"), &home.join("index.json"), &LearnConfig::demo()).unwrap();
    let _ = dir;
    Index::load(&home.join("index.json"))
}

#[test]
fn learns_the_lookup_linked_files_an_avoid_and_an_obligation() {
    let d = tempfile::tempdir().unwrap();
    let (files, _) = corpus(d.path());
    let home = d.path().join("home");
    let index = run(d.path(), &files, &home);
    let routes: Vec<_> = index.routes.iter().map(|r| &r.route).collect();

    let lookup = routes.iter().find(|r| matches!(r.trigger, Trigger::Prompt { .. })).expect("prompt route");
    assert_eq!(lookup.step, Step::Run { command: "python3 scripts/wi-lookup {item}".into() });
    assert_eq!(lookup.strength, Strength::Fire, "6/6 sessions clears the demo fire bar");
    assert_eq!(format!("{:?}", lookup.effect), "Observe");

    let linked = routes.iter().find(|r| matches!(r.step, Step::ReadBatch { .. })).expect("linked-files route");
    assert!(matches!(linked.trigger, Trigger::AfterCommand { .. }));
    // Design W2: every learned route states an exit condition.
    assert!(routes.iter().all(|r| !r.exit.is_empty()), "{routes:#?}");
    assert_eq!(linked.exit, vec![Exit::IntentChanges]);

    let dead = routes.iter().find(|r| matches!(r.step, Step::Avoid { .. })).expect("dead-end route");
    if let Step::Avoid { avoid, instead, signature } = &dead.step {
        assert!(avoid.contains("unittest") || avoid.contains("pytest"));
        assert_eq!(instead, "make test");
        assert!(signature.contains("ACME_ENV is not set"));
    }
    assert_eq!(dead.strength, Strength::Inhibit);
    assert_eq!(dead.exit, vec![Exit::IntentChanges]);

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
    let t1 = io::read(&home.join("tracks/acme-billing/learned")).unwrap();
    run(d.path(), &files, &home);
    assert_eq!(first, std::fs::read(home.join("index.json")).unwrap(), "same inputs, same index bytes");
    let t2 = io::read(&home.join("tracks/acme-billing/learned")).unwrap();
    assert_eq!(t1.manifest.track.version, t2.manifest.track.version, "no change, no version bump");
}

#[test]
fn steered_traces_are_exposed_only_to_what_their_directives_named() {
    let d = tempfile::tempdir().unwrap();
    let cwd = "/fixture/acme-billing";
    let (mut files, _) = corpus(d.path());
    // Three traces steered by the lookup route, which then hit a dead end the directive never
    // named: they must count as dead-end evidence but not as lookup evidence (Track G §3.2). (The
    // dead end is the corpus's, same failure signature: evidence for a dead end shares it.)
    for i in 0..3 {
        let mut t = Tx::new(&format!("steered{i}"), cwd, "Pick up W-2099");
        t.lines.push(json!({"type":"attachment","sessionId":format!("steered{i}"),"cwd":cwd,
            "attachment":{"type":"hook_additional_context","content":["<adage-reflex track=\"x\" route=\"lookup-work-item\" strength=\"fire\" kind=\"run\">\nIn this repository, work item details are looked up with:\n  python3 scripts/wi-lookup W-2099\n</adage-reflex>"]}}));
        t.bash("./scripts/other-lookup W-2099", "ok", false)
            .call("Edit", json!({"file_path": format!("{cwd}/acme/billing.py")}), "ok", false)
            .bash("python3 -m pytest", "RuntimeError: ACME_ENV is not set — run tests with 'make test'", true)
            .bash("make test", "OK", false);
        files.push(t.write(d.path()));
    }
    let p = files.last().unwrap();
    let s = adage_learn::import::parse_transcript(p, &adage_core::CommandTable::load(), None).unwrap();
    assert!(s.steered && s.directed.contains("lookup-work-item"));
    assert!(!s.steps.is_empty(), "steps are kept; exposure is per command named");
    assert_eq!(s.influence[0].commands, ["python3 scripts/wi-lookup W-2099"]);
    let home = d.path().join("home");
    let index = run(d.path(), &files, &home);
    let lookup = index.routes.iter().find(|r| r.route.id == "lookup-work-item").unwrap();
    assert_eq!(lookup.route.evidence.opportunities, 6, "steered traces are not lookup opportunities");
    let dead = index.routes.iter().find(|r| r.route.id.starts_with("dead-end")).unwrap();
    assert_eq!(dead.route.evidence.support, 9, "steered traces still count as dead-end evidence");
}

const NO_PYTEST: &str = "/usr/bin/python3: No module named pytest";
const NO_ENV: &str = "Traceback\nRuntimeError: ACME_ENV is not set — run tests with 'make test'";
const ENV_FIXED: &str = "ACME_ENV=test python3 -m unittest discover -s tests";

/// A run: command, output, failed.
type Run<'a> = (&'a str, &'a str, bool);

/// For each (count, runs): `count` sessions that each run `runs` and then `make test`.
fn dead_end_corpus(dir: &Path, groups: &[(usize, &[Run])]) -> Vec<PathBuf> {
    let mut files = vec![];
    let mut n = 0;
    for (count, runs) in groups {
        for _ in 0..*count {
            let mut t = Tx::new(&format!("d{n:02}"), "/fixture/acme-billing", "Fix the rounding bug.");
            n += 1;
            for (cmd, out, failed) in *runs {
                t.bash(cmd, out, *failed);
            }
            t.bash("make test", "OK", false);
            files.push(t.write(dir));
        }
    }
    files
}

/// (id, avoid, signature, instead, support, opportunities) of each dead end, in index order.
type Dead = (String, String, String, String, u32, u32);

fn dead_ends(index: &Index) -> Vec<Dead> {
    index
        .routes
        .iter()
        .filter(|r| r.route.id.starts_with("dead-end"))
        .map(|r| {
            let Step::Avoid { avoid, instead, signature } = &r.route.step else { panic!("not an avoid step") };
            let e = &r.route.evidence;
            (r.route.id.clone(), avoid.clone(), signature.clone(), instead.clone(), e.support, e.opportunities)
        })
        .collect()
}

fn only_dead_end(index: &Index) -> (String, String, String, u32, u32) {
    let dead = dead_ends(index);
    assert_eq!(dead.len(), 1, "one failure signature, one dead end: {dead:?}");
    let (_, avoid, signature, instead, support, opportunities) = dead[0].clone();
    (avoid, signature, instead, support, opportunities)
}

/// The dead end whose signature is `sig`.
fn dead_end_for(index: &Index, sig: &str) -> Dead {
    dead_ends(index).into_iter().find(|d| d.2 == sig).unwrap_or_else(|| panic!("no dead end for {sig}"))
}

const NOT_FOUND: &str = "zsh: command not found: python";

#[test]
fn a_dead_end_names_only_runners_that_share_its_signature() {
    // docs/evals.md: the route named every runner that failed before `make test`, including
    // `python3 -m unittest`, which passes once `ACME_ENV` is set, and `python -m pytest`, which
    // fails for an unrelated reason. That reason is now a dead end of its own.
    let d = tempfile::tempdir().unwrap();
    let files = dead_end_corpus(
        d.path(),
        &[
            (
                6,
                &[
                    ("python3 -m pytest tests", NO_PYTEST, true),
                    ("python3 -m unittest discover -s tests", NO_ENV, true),
                ],
            ),
            (3, &[("python -m pytest -v", NOT_FOUND, true)]),
            (2, &[(ENV_FIXED, "OK", false)]),
        ],
    );
    let index = run(d.path(), &files, &d.path().join("home"));
    let dead = dead_ends(&index);
    assert_eq!(dead.len(), 2, "one dead end per (working command, signature): {dead:?}");
    let first = dead_end_for(&index, "No module named pytest");
    assert_eq!((first.1.as_str(), first.3.as_str(), first.4, first.5), ("python3 -m pytest", "make test", 6, 6));
    let second = dead_end_for(&index, NOT_FOUND);
    assert_eq!((second.1.as_str(), second.3.as_str(), second.4, second.5), ("python -m pytest", "make test", 3, 3));
    // `python3 -m unittest` is still named by neither: it passes once `ACME_ENV` is set.
    assert!(dead.iter().all(|d| !d.1.contains("unittest")), "{dead:?}");
}

#[test]
fn the_most_common_signature_keeps_the_working_commands_id() {
    // Adding a second signature must not rename the first route: its id is derived from the
    // working command alone (the demo's `dead-end-d41b1e`), and the new route's from (working
    // command, signature). Both ids are stable across re-learning.
    let d = tempfile::tempdir().unwrap();
    let one: &[(usize, &[Run])] = &[(5, &[("python3 -m pytest tests", NO_PYTEST, true)])];
    let index = run(d.path(), &dead_end_corpus(d.path(), one), &d.path().join("home"));
    let alone = dead_ends(&index);
    assert_eq!(alone.len(), 1);

    let d = tempfile::tempdir().unwrap();
    let two: &[(usize, &[Run])] =
        &[(5, &[("python3 -m pytest tests", NO_PYTEST, true)]), (4, &[("python -m pytest -v", NOT_FOUND, true)])];
    let index = run(d.path(), &dead_end_corpus(d.path(), two), &d.path().join("home"));
    let first = dead_end_for(&index, "No module named pytest");
    let second = dead_end_for(&index, NOT_FOUND);
    assert_eq!(first.0, alone[0].0, "the most common signature keeps its id");
    assert_ne!(second.0, first.0);
    assert!(second.0.starts_with("dead-end-") && second.0.len() == "dead-end-".len() + 6, "{}", second.0);

    // The same corpus again: the same ids.
    let d = tempfile::tempdir().unwrap();
    let index = run(d.path(), &dead_end_corpus(d.path(), two), &d.path().join("home"));
    assert_eq!(dead_end_for(&index, NOT_FOUND).0, second.0);
}

#[test]
fn a_second_signature_needs_its_own_support_and_names_each_runner_once() {
    // Two sessions are below the demo's minimum support (3): no second route.
    let d = tempfile::tempdir().unwrap();
    let files = dead_end_corpus(
        d.path(),
        &[(5, &[("python3 -m pytest tests", NO_PYTEST, true)]), (2, &[("python -m pytest -v", NOT_FOUND, true)])],
    );
    let index = run(d.path(), &files, &d.path().join("home"));
    assert_eq!(only_dead_end(&index).1, "No module named pytest");

    // A runner that fails with two signatures is named once, under its more common one.
    let d = tempfile::tempdir().unwrap();
    let files = dead_end_corpus(
        d.path(),
        &[
            (5, &[("python3 -m pytest tests", NO_PYTEST, true)]),
            (3, &[("python3 -m pytest -x", NO_ENV, true)]),
            (3, &[("python -m pytest -v", NOT_FOUND, true)]),
        ],
    );
    let index = run(d.path(), &files, &d.path().join("home"));
    let dead = dead_ends(&index);
    let named: Vec<&str> = dead.iter().flat_map(|d| d.1.split(" / ")).collect();
    assert_eq!(named.iter().filter(|r| **r == "python3 -m pytest").count(), 1, "{dead:?}");
    assert_eq!(dead_end_for(&index, "No module named pytest").1, "python3 -m pytest");
    assert_eq!(dead_end_for(&index, NOT_FOUND).1, "python -m pytest");
    assert!(dead.iter().all(|d| d.2 != "RuntimeError: ACME_ENV is not set — run tests with 'make test'"), "{dead:?}");
}

#[test]
fn a_runner_that_passes_once_the_environment_is_fixed_is_never_named() {
    // `python3 -m unittest` fails with the route's own signature, but passes in two sessions that
    // set `ACME_ENV` (normalised to the same argv): it failed in 5 of the 7 sessions that ran it,
    // under the 9-in-10 bar. `python3 -m pytest` failed every time it ran.
    let d = tempfile::tempdir().unwrap();
    let files = dead_end_corpus(
        d.path(),
        &[
            (5, &[("python3 -m unittest discover -s tests", NO_ENV, true)]),
            (3, &[("python3 -m pytest tests", NO_ENV, true)]),
            (2, &[(ENV_FIXED, "OK", false)]),
        ],
    );
    let index = run(d.path(), &files, &d.path().join("home"));
    let (avoid, signature, _, support, opportunities) = only_dead_end(&index);
    assert_eq!(avoid, "python3 -m pytest");
    assert_eq!(signature, "RuntimeError: ACME_ENV is not set — run tests with 'make test'");
    assert_eq!((support, opportunities), (3, 3));

    // Without the environment-fixed sessions, unittest fails every time it runs and is named,
    // first, because more sessions show it.
    let d = tempfile::tempdir().unwrap();
    let files = dead_end_corpus(
        d.path(),
        &[
            (5, &[("python3 -m unittest discover -s tests", NO_ENV, true)]),
            (3, &[("python3 -m pytest tests", NO_ENV, true)]),
        ],
    );
    let index = run(d.path(), &files, &d.path().join("home"));
    assert_eq!(only_dead_end(&index).0, "python3 -m unittest / python3 -m pytest");
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

#[test]
fn the_dry_run_plans_exactly_what_learn_writes_and_writes_nothing() {
    let d = tempfile::tempdir().unwrap();
    let (files, _) = corpus(d.path());
    let home = d.path().join("home");
    let table = adage_core::CommandTable::load();
    let sessions: Vec<_> =
        files.iter().filter_map(|p| adage_learn::read_session(p, &table, Some("acme-billing"))).collect();
    assert_eq!(sessions[0].harness, "claude-code");
    assert_eq!(sessions[0].prompt_count(), 1);
    let store = adage_learn::store::Files::new(&home.join("tracks"), &home.join("index.json"));
    let (rep, planned) = adage_learn::plan(sessions, None, &store, &LearnConfig::demo()).unwrap();
    assert_eq!(rep.sessions_used, 6);
    assert!(!home.exists(), "a plan writes nothing");
    assert_eq!(planned.len(), 1, "one track per repository: <repo>/learned");
    run(d.path(), &files, &home);
    for p in &planned {
        assert_eq!(io::read(&home.join("tracks").join(&p.name)).unwrap(), p.track, "learn wrote what the plan said");
        assert_eq!(p.track.manifest.gauge.harness, vec!["claude-code".to_string()]);
    }
}

#[test]
fn a_pinned_route_that_drifts_is_proposed_not_overwritten() {
    let d = tempfile::tempdir().unwrap();
    let (files, _) = corpus(d.path());
    let home = d.path().join("home");
    run(d.path(), &files, &home);
    let dir = home.join("tracks/acme-billing/learned");
    let mut t = io::read(&dir).unwrap();
    let r = t.routes.iter_mut().find(|r| r.id == "lookup-work-item").unwrap();
    r.authored = true;
    r.step = Step::Run { command: "python3 scripts/wi-lookup {item} --json".into() };
    io::write(&dir, &t).unwrap();
    run(d.path(), &files, &home);
    let after = io::read(&dir).unwrap();
    let pinned = after.routes.iter().find(|r| r.id == "lookup-work-item").unwrap();
    assert_eq!(
        pinned.step,
        Step::Run { command: "python3 scripts/wi-lookup {item} --json".into() },
        "never overwritten"
    );
    let p = adage_tracks::proposals::load(&dir).unwrap();
    let prop = &p.routes["lookup-work-item"];
    assert_eq!(prop.route.step, Step::Run { command: "python3 scripts/wi-lookup {item}".into() });
    assert!(prop.reason.contains("step"), "{}", prop.reason);
    // Re-learning the same sessions leaves identical bytes.
    let bytes = std::fs::read(dir.join(adage_tracks::proposals::FILE)).unwrap();
    run(d.path(), &files, &home);
    assert_eq!(std::fs::read(dir.join(adage_tracks::proposals::FILE)).unwrap(), bytes);
}
