//! Synthetic traces with a known Ŝ.

use super::*;
use serde_json::json;

const REPO: &str = "github.com/acme/app";

/// Instinct U11 as it was bound live (`private/adage-home/index.json` of RepoBench v2's click).
fn u11_index() -> Index {
    serde_json::from_value(json!({
        "generation": "test",
        "routes": [{
            "repo": REPO, "track": "acme/instincts", "version": "1.0.0",
            "route": {
                "id": "dead-end-u11", "summary": "Instinct U11: the project's environment has what this runner needs",
                "trigger": {"on": "after_failure", "command": "exec python -m pytest", "signature": "No module named pytest"},
                "step": {"do": "run", "command": ".venv/bin/python -m pytest"},
                "slots": [], "effect": "verify", "strength": "prime", "state": "forming",
                "exit": [{"when": "intent_changes"}],
                "evidence": {"opportunities": 0, "support": 0, "days": 0, "successes": 0, "deviations_failed": 0,
                    "rate": 0.0, "lower_bound": 0.0, "median_position": 0.0, "turns_saved": 0.0,
                    "provenance": "instinct:core@0.1.0", "models": [],
                    "origin": {"kind": "instinct", "pack": "core", "version": "0.1.0", "instinct": "U11",
                        "ecosystem": "Python", "declared": ".venv/pyvenv.cfg"}},
                "authored": false
            }
        }]
    }))
    .unwrap()
}

/// A Claude Code transcript: one tool call per turn, `(tool, input, output, is_error)`.
fn transcript(calls: &[(&str, serde_json::Value, &str, bool)]) -> String {
    let mut lines = vec![
        json!({"type": "user", "sessionId": "s", "cwd": "/w/r", "message": {"content": "Fix the bug"}}).to_string(),
    ];
    for (i, (tool, input, out, err)) in calls.iter().enumerate() {
        lines.push(
            json!({"type": "assistant", "sessionId": "s", "cwd": "/w/r", "message": {"id": format!("m{i}"),
                "content": [{"type": "tool_use", "id": format!("c{i}"), "name": tool, "input": input}]}})
            .to_string(),
        );
        lines.push(
            json!({"type": "user", "sessionId": "s", "cwd": "/w/r", "message": {"content": [
                {"type": "tool_result", "tool_use_id": format!("c{i}"), "content": out, "is_error": err}]}})
            .to_string(),
        );
    }
    lines.join("\n")
}

fn bash(cmd: &str, out: &str, err: bool) -> (&'static str, serde_json::Value, String, bool) {
    ("Bash", json!({"command": cmd}), out.to_string(), err)
}

struct Fixture {
    dir: tempfile::TempDir,
    root: PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("repo");
    std::fs::create_dir_all(root.join(".venv/bin")).unwrap();
    std::fs::write(root.join(".venv/pyvenv.cfg"), "home = /usr/bin\n").unwrap();
    std::fs::write(root.join(".venv/bin/python"), "").unwrap();
    Fixture { dir, root }
}

fn meta(
    f: &Fixture,
    name: &str,
    task: &str,
    calls: &[(&'static str, serde_json::Value, String, bool)],
    passed: bool,
) -> SessionMeta {
    let calls: Vec<(&str, serde_json::Value, &str, bool)> =
        calls.iter().map(|(t, i, o, e)| (*t, i.clone(), o.as_str(), *e)).collect();
    let p = f.dir.path().join(format!("{name}.jsonl"));
    std::fs::write(&p, transcript(&calls)).unwrap();
    SessionMeta {
        id: name.into(),
        task: task.into(),
        arm: "control".into(),
        transcript: p,
        passed: Some(passed),
        cost_usd: Some(0.0),
        stopped: false,
    }
}

const NO_PYTEST: &str = "/usr/bin/python: No module named pytest";
const RAN: &str =
    "============ test session starts ============\ncollected 3 items\n\ntests/test_x.py ...\n\n3 passed in 0.10s";

/// The dead end, then `ls -la`, `pip install pytest`, a read, and the run that works (system
/// Python, after the install): the recovery the control arm mostly took.
fn dead_end_session(piped: bool) -> Vec<(&'static str, serde_json::Value, String, bool)> {
    let first =
        if piped { "python -m pytest tests/test_x.py 2>&1 | head -50" } else { "python -m pytest tests/test_x.py" };
    vec![
        ("Read", json!({"file_path": "/w/r/src/app.py"}), "1\tcode".into(), false),
        bash(first, NO_PYTEST, !piped),
        bash("ls -la", "total 8\n.venv\nsrc\ntests", false),
        bash("pip install pytest", "Successfully installed pytest-8.0", false),
        ("Read", json!({"file_path": "/w/r/tests/test_x.py"}), "1\tdef test_x(): pass".into(), false),
        bash("python -m pytest tests/test_x.py", RAN, false),
    ]
}

fn runtime<'a>(ix: &'a Index, root: &'a Path, table: &'a CommandTable) -> Policy<'a> {
    Policy::Runtime { index: ix, repo: REPO, root, table, params: RuntimeParams::default() }
}

#[test]
fn invocations_are_classed_by_interpreter_and_runner() {
    let cases = [
        ("python -m pytest tests/test_x.py -xvs", Some(("system", "pytest"))),
        ("python3 -m pytest tests/ 2>&1 | head -100", Some(("system", "pytest"))),
        (".venv/bin/python -m pytest tests/test_x.py::t", Some(("venv", "pytest"))),
        ("/tmp/w/repo/.venv/bin/pytest -q", Some(("venv", "pytest"))),
        ("source .venv/bin/activate && python -m pytest tests/test_x.py", Some(("venv", "pytest"))),
        ("cd /w/r && . .venv/bin/activate && pytest -x", Some(("venv", "pytest"))),
        ("uv run pytest tests", Some(("venv", "pytest"))),
        ("uv run --frozen python -m pytest", Some(("venv", "pytest"))),
        ("PYTHONPATH=src python -m pytest tests", Some(("system", "pytest"))),
        ("pytest tests/test_x.py", Some(("system", "pytest"))),
        ("tox -e py", Some(("tox", "tox"))),
        ("python -m unittest discover", Some(("system", "unittest"))),
        ("pnpm vitest run packages/runtime-core/__tests__/x.spec.ts", Some(("node", "vitest"))),
        ("npx jest foo", Some(("node", "jest"))),
        ("npm test", Some(("node", "pkg-test"))),
        ("node_modules/.bin/vitest run --project unit x", Some(("node", "vitest"))),
        ("go test ./... -run TestX", Some(("go", "go-test"))),
        ("cargo test -p adage-eval", Some(("cargo", "cargo-test"))),
        ("python repro.py", None),
        ("python -c \"import click\"", None),
        ("grep -rn pytest pyproject.toml", None),
        ("echo 'python -m pytest && x'", None),
    ];
    for (cmd, want) in cases {
        let got = invocation(cmd).map(|(i, _)| (i.interp, i.runner));
        assert_eq!(got, want, "{cmd}");
    }
    let (_, args) = invocation("python -m pytest tests/test_x.py::test_a -xvs").unwrap();
    assert_eq!(targets(&args, "/w/r"), vec!["tests/test_x.py".to_string()]);
    assert_eq!(go_packages(&["-run".into(), "TestX".into(), "./...".into()], "/w/r"), vec!["pkg:...".to_string()]);
    assert!(
        same_file("pkg:...", "cmd/x_test.go")
            && same_file("pkg:", "flag_test.go")
            && !same_file("pkg:", "cmd/x_test.go")
    );
}

#[test]
fn features_of_calls() {
    let c = |cmd: &str, out: &str, err: bool| {
        act(
            &ToolCall {
                tool: "Bash".into(),
                input: json!({"command": cmd}),
                output: out.into(),
                hook_output: out.into(),
                failed: err,
            },
            "/w/r",
        )
    };
    let a = c("python -m pytest /w/r/tests/test_x.py 2>&1 | head", NO_PYTEST, false);
    assert!(a.env_sig && a.failed() && a.env_friction() && !a.executed, "a piped dead end fails too");
    assert_eq!(a.targets, vec!["tests/test_x.py".to_string()]);
    let a = c(".venv/bin/python -m pytest tests", RAN, false);
    assert!(a.executed && !a.failed());
    assert!(!c(".venv/bin/python -m pytest tests", "collected 0 items\n\nno tests ran in 0.01s", false).executed);
    for probe in [
        "ls -la | head -20",
        "pip install -e .",
        "which python",
        "ls .venv/bin",
        "cat pyproject.toml",
        "python --version",
    ] {
        assert!(c(probe, "", false).env_probe, "{probe}");
    }
    for not in ["ls src/click", "git status", "grep -n def src/app.py"] {
        assert!(!c(not, "", false).env_probe, "{not}");
    }
    assert!(c("find tests -name '*.py' | xargs grep -l Choice", "", false).discovery);
    assert!(!c("grep -n def src/app.py", "", false).discovery);
}

#[test]
fn u11_post_hoc_saves_the_friction_between_the_dead_end_and_the_working_run() {
    let f = fixture();
    let (ix, table) = (u11_index(), CommandTable::load());
    let m = meta(&f, "s1", "t1", &dead_end_session(false), true);
    let r = session(&runtime(&ix, &f.root, &table), &m, Mode::PostHoc, SatRule::Goal).unwrap();
    assert_eq!(r.scored.len(), 1, "{:#?}", r.scored);
    let s = &r.scored[0];
    assert_eq!((s.given.t, s.given.pre, s.t_star), (1, false, Some(5)));
    assert!(s.useful && !s.harmful && !s.superseded);
    // W between t=1 and t*=5: `ls -la` and `pip install` (the read is not friction).
    assert_eq!(s.saved, 2.0);
    assert_eq!(s.cost, 0.0, "no price for a session without a priced model");
    assert_eq!(r.net, 2.0);
    assert!(r.dead_end);

    // The strict Sat (interpreter must be d's): the trace never used `.venv`, so nothing counts.
    let r = session(&runtime(&ix, &f.root, &table), &m, Mode::PostHoc, SatRule::Strict).unwrap();
    assert!(!r.scored[0].useful && !r.scored[0].harmful);
    assert_eq!(r.net, 0.0);
}

#[test]
fn pre_emptive_also_saves_the_intercepted_call_and_sees_piped_dead_ends() {
    let f = fixture();
    let (ix, table) = (u11_index(), CommandTable::load());
    let m = meta(&f, "s1", "t1", &dead_end_session(false), true);
    let r = session(&runtime(&ix, &f.root, &table), &m, Mode::PreEmptive, SatRule::Goal).unwrap();
    assert_eq!(r.scored.len(), 1, "the post-hoc failure does not fire the route again");
    assert_eq!((r.scored[0].given.t, r.scored[0].given.pre), (1, true));
    assert_eq!(r.net, 3.0, "2 friction calls + the intercepted dead end");

    // Piped: exit 0. The runtime now reads a piped run's output for the dead end's signature
    // (launch bug 2), so post-hoc delivery sees it too; PreToolUse still saves the intercepted call.
    let m = meta(&f, "s2", "t1", &dead_end_session(true), true);
    let post = session(&runtime(&ix, &f.root, &table), &m, Mode::PostHoc, SatRule::Goal).unwrap();
    assert_eq!((post.scored.len(), post.net), (1, 2.0), "{:#?}", post.scored);
    let pre = session(&runtime(&ix, &f.root, &table), &m, Mode::PreEmptive, SatRule::Goal).unwrap();
    assert_eq!(pre.net, 3.0);
}

#[test]
fn a_different_runner_that_ran_the_tests_makes_the_directive_harmful() {
    let f = fixture();
    let (ix, table) = (u11_index(), CommandTable::load());
    let calls = vec![
        bash("python -m pytest tests", NO_PYTEST, true),
        bash("tox -e py", "py: commands[0]> pytest\n3 passed in 0.2s\n  py: OK", false),
    ];
    let m = meta(&f, "h", "t1", &calls, true);
    let r = session(&runtime(&ix, &f.root, &table), &m, Mode::PostHoc, SatRule::Goal).unwrap();
    assert!(!r.scored[0].useful && r.scored[0].harmful);
    assert_eq!(r.net, 0.0);
    // The same trace, failed: not harmful (nothing shows the other runner was right).
    let m = meta(&f, "h2", "t1", &calls, false);
    let r = session(&runtime(&ix, &f.root, &table), &m, Mode::PostHoc, SatRule::Goal).unwrap();
    assert!(!r.scored[0].harmful);
}

#[test]
fn a_directive_inside_an_earlier_recovery_window_is_superseded() {
    let acts: Vec<Act> = [
        (Some(("system", "pytest")), false, true), // 0 dead end
        (None, false, true),                       // 1 friction (env signature)
        (Some(("system", "pytest")), false, true), // 2 dead end again
        (Some(("system", "pytest")), true, false), // 3 works
    ]
    .iter()
    .map(|&(run, executed, env_sig)| Act {
        tool: "Bash".into(),
        run: run.map(|(i, r)| Invocation { interp: i, runner: r }),
        executed,
        env_sig,
        ..Default::default()
    })
    .collect();
    let class = Class::Runner { interp: "venv", runner: "pytest" };
    let g = |t: i64| Given {
        route: format!("r{t}"),
        t,
        pre: false,
        event: "PostToolUseFailure",
        tokens: 100,
        class: class.clone(),
    };
    let price = Price { cache_read: 0.1, usd_per_call: 0.01, api_per_call: 1.0 };
    let s = score(vec![g(0), g(2)], &acts, Some(true), &price, SatRule::Goal);
    assert_eq!((s[0].t_star, s[0].saved), (Some(3), 2.0));
    assert!(s[1].superseded && s[1].saved == 0.0 && s[1].cost == 0.0);
    // C = 100 tokens × $0.1/MTok × 3 remaining API calls / $0.01 per call = 0.003 calls.
    assert!((s[0].cost - 0.003).abs() < 1e-12, "{}", s[0].cost);
}

#[test]
fn the_cochange_oracle_saves_test_discovery_after_a_source_edit() {
    let f = fixture();
    let calls = vec![
        (
            "Edit",
            json!({"file_path": "/w/r/src/app.py", "old_string": "a", "new_string": "b"}),
            "ok".to_string(),
            false,
        ),
        ("Grep", json!({"pattern": "def test_", "path": "/w/r/tests"}), "tests/test_app.py".to_string(), false),
        bash("find tests -name 'test_*.py'", "tests/test_app.py\ntests/test_cli.py", false),
        ("Read", json!({"file_path": "/w/r/tests/test_app.py"}), "1\tdef test_a(): pass".to_string(), false),
        bash(".venv/bin/python -m pytest tests/test_app.py::test_a -q", RAN, false),
    ];
    let m = meta(&f, "c1", "t1", &calls, true);
    let mut map = BTreeMap::new();
    map.insert("src/app.py".to_string(), vec!["tests/test_app.py".to_string(), "tests/test_cli.py".to_string()]);
    let oracle = CoChange { tasks: BTreeMap::from([("t1".to_string(), map)]), k: 3 };
    let r = session(&Policy::CoChange(&oracle), &m, Mode::PostHoc, SatRule::Goal).unwrap();
    assert_eq!(r.scored.len(), 1);
    assert_eq!((r.scored[0].given.t, r.scored[0].t_star), (0, Some(4)));
    assert_eq!(r.net, 2.0, "the Grep and the find; the read is not a search");
    // Another task has no map: nothing is said.
    let m2 = meta(&f, "c2", "t2", &calls, true);
    assert!(session(&Policy::CoChange(&oracle), &m2, Mode::PostHoc, SatRule::Goal).unwrap().scored.is_empty());
}

#[test]
fn the_summary_is_a_task_clustered_mean_with_rates_and_gates() {
    let f = fixture();
    let (ix, table) = (u11_index(), CommandTable::load());
    let p = runtime(&ix, &f.root, &table);
    let quiet = vec![bash(".venv/bin/python -m pytest tests", RAN, false)];
    let rows = vec![
        session(&p, &meta(&f, "a", "t1", &dead_end_session(false), true), Mode::PostHoc, SatRule::Goal).unwrap(),
        session(&p, &meta(&f, "b", "t1", &quiet, true), Mode::PostHoc, SatRule::Goal).unwrap(),
        session(&p, &meta(&f, "c", "t2", &dead_end_session(false), true), Mode::PostHoc, SatRule::Goal).unwrap(),
        session(&p, &meta(&f, "d", "t2", &quiet, true), Mode::PostHoc, SatRule::Goal).unwrap(),
    ];
    let s = summarise(&rows);
    // Ŝ = (2 + 0 + 2 + 0) / 4 sessions.
    assert_eq!((s.sessions, s.tasks, s.directives, s.useful, s.harmful), (4, 2, 2, 2, 0));
    assert_eq!(s.s_hat, 1.0);
    assert_eq!(s.s_hat_ci, [1.0, 1.0], "both tasks have the same mean: no bootstrap spread");
    assert_eq!(s.useful_rate, 1.0);
    assert_eq!(s.useful_lb90, r4(adage_graph::stats::wilson_lower(2.0, 2.0, 1.644_853_626_951_472_2)));
    assert_eq!(s.per_10_calls, r4(10.0 * 2.0 / 14.0));
    assert_eq!((s.dead_end_sessions, s.s_hat_dead_end), (2, 2.0));
    // Deterministic.
    assert_eq!(serde_json::to_string(&summarise(&rows)).unwrap(), serde_json::to_string(&s).unwrap());
}

#[test]
fn only_instincts_keeps_the_named_ones() {
    let ix = u11_index();
    let ids: BTreeSet<String> = ["U11".to_string()].into();
    assert_eq!(only_instincts(ix.clone(), &ids).routes.len(), 1);
    let ids: BTreeSet<String> = ["U3".to_string()].into();
    assert!(only_instincts(ix, &ids).routes.is_empty());
}
