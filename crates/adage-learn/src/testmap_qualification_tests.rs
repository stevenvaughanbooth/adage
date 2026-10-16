use super::*;
use serde_json::json;

fn inventory(paths: &[&str]) -> BTreeSet<String> {
    paths.iter().map(|p| (*p).into()).collect()
}

fn session(source: &str, command: &str, output: &str) -> Session {
    let root = Path::new("/repo");
    let calls = [("Edit", json!({"file_path": source}), ""), ("Bash", json!({"command": command}), output)];
    let mut steps = Vec::new();
    for (i, (tool, input, output)) in calls.into_iter().enumerate() {
        let actions = adage_core::tools::normalize_tool(tool, &input, "/repo", adage_core::CommandTable::load());
        for mut norm in adage_core::keys::flatten(&actions) {
            adage_core::keys::canonicalise(&mut norm, root);
            let rel_path = adage_core::keys::action_path(&norm.action).map(|p| adage_core::repo::relativize(root, p));
            steps.push(json!({
                "turn": i, "key": adage_core::keys::template_key(&norm.action), "norm": norm,
                "rel_path": rel_path, "call": {
                    "tool":tool,"input": input,"output":output,"failed":false,
                    "tool_use_id":format!("call-{i}")
                }
            }));
        }
    }
    serde_json::from_value(json!({
        "id":"one", "root":"/repo", "repo":"example", "model":null, "day":"2026-01-01",
        "prompt":"Change the parser", "steps":steps, "steered":false, "directed":[],
        "outcome":null,"harness":"claude-code"
    }))
    .unwrap()
}

#[test]
fn names_cannot_cross_languages_or_declared_package_boundaries() {
    let files = inventory(&[
        "package.json",
        "packages/a/package.json",
        "packages/a/src/cli.ts",
        "packages/b/package.json",
        "packages/b/test/cli.test.ts",
        "tests/cli.rs",
        "tests/test_cli.py",
    ]);
    assert!(tests_of(&files, &BTreeMap::new(), &BTreeMap::new()).is_empty());
    let history = BTreeMap::from([(("packages/a/src/cli.ts".into(), "tests/cli.rs".into()), 10)]);
    assert!(tests_of(&files, &history, &BTreeMap::new()).is_empty());
    // An explicitly observed integration relation can cross a package/language boundary.
    let confirmed = BTreeMap::from([(("packages/a/src/cli.ts".into(), "tests/cli.rs".into()), 1)]);
    assert_eq!(tests_of(&files, &history, &confirmed)["packages/a/src/cli.ts"], ["tests/cli.rs"]);
}

#[test]
fn a_map_uses_only_targets_observed_under_its_exact_runner() {
    let files = inventory(&[
        "package.json",
        "src/cli.ts",
        "tests/cli.test.ts",
        "work/test/cli.test.mjs",
        "copied/tests/cli.rs",
    ]);
    let vitest = session(
        "src/cli.ts",
        "pnpm exec vitest run tests/cli.test.ts",
        " ✓ tests/cli.test.ts (2 tests) 12ms\n Test Files  1 passed (1)\n Tests  2 passed (2)\n",
    );
    let node = session("src/cli.ts", "node --test work/test/cli.test.mjs", "# tests 2\n# pass 2\n# fail 0\n");
    let map = qualified_tests_of(&files, &BTreeMap::new(), &[&vitest, &node], "pnpm exec vitest run");
    assert_eq!(map["src/cli.ts"], ["tests/cli.test.ts"]);
    assert!(qualified_tests_of(&files, &BTreeMap::new(), &[&vitest], "npx vitest run").is_empty());
    assert!(qualified_tests_of(&files, &BTreeMap::new(), &[&vitest], "npx jest").is_empty());
}

#[test]
fn static_or_dynamic_configuration_alone_cannot_qualify_a_target() {
    let files = inventory(&["package.json", "vitest.config.ts", "src/a.ts", "tests/a.test.ts"]);
    let history = BTreeMap::from([(("src/a.ts".into(), "tests/a.test.ts".into()), 20)]);
    assert!(!tests_of(&files, &history, &BTreeMap::new()).is_empty());
    assert!(qualified_tests_of(&files, &history, &[], "pnpm exec vitest run").is_empty());
}

#[test]
fn a_recorded_absolute_project_interpreter_keeps_its_canonical_spelling() {
    let files = inventory(&["src/core.py", "tests/test_core.py"]);
    let s = session("src/core.py", "/repo/.venv/bin/python -m pytest /repo/tests/test_core.py", "1 passed in 0.01s\n");
    assert_eq!(
        qualified_tests_of(&files, &BTreeMap::new(), &[&s], ".venv/bin/python -m pytest")["src/core.py"],
        ["tests/test_core.py"]
    );
}

#[test]
fn a_javascript_filename_filter_needs_a_matching_file_report() {
    let files = inventory(&["src/a.ts", "tests/a.test.ts"]);
    for output in [
        "Tests 1 passed (1)\n",
        "✓ tests/a.test.ts.other (1 test) 1ms\nTests 1 passed (1)\n",
        "✓ tests/a.test.ts (1 test | 1 skipped) 1ms\nTests 1 passed | 1 skipped (2)\n",
    ] {
        let s = session("src/a.ts", "pnpm exec vitest run tests/a.test.ts", output);
        assert!(qualified_tests_of(&files, &BTreeMap::new(), &[&s], "pnpm exec vitest run").is_empty());
    }
    let s = session("src/a.ts", "npm test -- tests/a.test.ts", "PASS tests/a.test.ts\nTests: 1 passed, 1 total\n");
    assert_eq!(qualified_tests_of(&files, &BTreeMap::new(), &[&s], "npm test --")["src/a.ts"], ["tests/a.test.ts"]);
}

#[test]
fn discovery_zero_tests_and_setup_errors_do_not_confirm_execution() {
    let files = inventory(&["src/core.py", "tests/test_core.py"]);
    for (command, output) in [
        (".venv/bin/python -m pytest tests/test_core.py", "collected 3 items\n"),
        (".venv/bin/python -m pytest tests/test_core.py --collect-only", "3 tests collected in 0.02s\n"),
        (".venv/bin/python -m pytest tests/test_core.py", "no tests ran in 0.01s\n"),
        (".venv/bin/python -m pytest tests/test_core.py", "0 passed in 0.01s\n"),
        (".venv/bin/python -m pytest tests/test_core.py", "Interrupted: 1 error during collection\n1 error in 0.01s\n"),
        (".venv/bin/python -m pytest tests/test_core.py --help", "1 passed in 0.01s\n"),
    ] {
        let s = session("src/core.py", command, output);
        assert!(
            qualified_tests_of(&files, &BTreeMap::new(), &[&s], ".venv/bin/python -m pytest").is_empty(),
            "{command}: {output}"
        );
    }
    for output in ["1 passed in 0.01s\n", "1 failed in 0.01s\n"] {
        let s = session("src/core.py", ".venv/bin/python -m pytest tests/test_core.py -q", output);
        assert_eq!(
            qualified_tests_of(&files, &BTreeMap::new(), &[&s], ".venv/bin/python -m pytest")["src/core.py"],
            ["tests/test_core.py"]
        );
    }
}

#[test]
fn one_files_summary_does_not_validate_every_argument() {
    let files = inventory(&["src/core.py", "tests/test_core.py", "tests/test_other.py"]);
    let command = ".venv/bin/python -m pytest tests/test_core.py tests/test_other.py";
    let aggregate = session("src/core.py", command, "1 passed in 0.01s\n");
    assert!(qualified_tests_of(&files, &BTreeMap::new(), &[&aggregate], ".venv/bin/python -m pytest").is_empty());
    let attributed = session("src/core.py", command, "tests/test_core.py::test_one PASSED\n1 passed in 0.01s\n");
    assert_eq!(
        qualified_tests_of(&files, &BTreeMap::new(), &[&attributed], ".venv/bin/python -m pytest")["src/core.py"],
        ["tests/test_core.py"]
    );
    let echoed = session("src/core.py", command, "Running tests/test_core.py tests/test_other.py\n1 passed in 0.01s\n");
    assert!(qualified_tests_of(&files, &BTreeMap::new(), &[&echoed], ".venv/bin/python -m pytest").is_empty());
}

#[test]
fn options_compounds_and_exposure_cannot_transfer_execution_evidence() {
    let files = inventory(&["src/core.py", "tests/test_core.py"]);
    for command in [
        ".venv/bin/python -m pytest --config other.ini tests/test_core.py",
        ".venv/bin/python -m pytest tests/test_core.py --rootdir=elsewhere",
        "PYTHONPATH=other .venv/bin/python -m pytest tests/test_core.py",
        ".venv/bin/python -m pytest tests/test_core.py && .venv/bin/python -m pytest tests/test_other.py",
        "echo '1 passed in 0.01s' ; .venv/bin/python -m pytest tests/test_core.py",
        ".venv/bin/python -m pytest tests/test_core.py | tail -20",
    ] {
        let s = session("src/core.py", command, "1 passed in 0.01s\n");
        assert!(
            qualified_tests_of(&files, &BTreeMap::new(), &[&s], ".venv/bin/python -m pytest").is_empty(),
            "{command}"
        );
    }
    let mut s = session("src/core.py", ".venv/bin/python -m pytest tests/test_core.py", "1 passed in 0.01s\n");
    s.influence.push(adage_trace::Directed { all: true, ..Default::default() });
    assert!(qualified_tests_of(&files, &BTreeMap::new(), &[&s], ".venv/bin/python -m pytest").is_empty());
}

#[test]
fn per_file_reports_require_exact_target_boundaries() {
    assert!(file_reported("pytest", "tests/test_a.py", "tests/test_a.py::test_x PASSED"));
    assert!(!file_reported("pytest", "tests/test_a.py", "tests/test_a.py.extra::test_x PASSED"));
    assert!(file_reported("vitest", "tests/a.test.ts", " ✓ tests/a.test.ts (2 tests) 12ms"));
    assert!(!file_reported("vitest", "tests/a.test.ts", " ✓ tests/a.test.ts (0 tests) 12ms"));
    assert!(!file_reported("vitest", "tests/a.test.ts", " ✓ tests/a.test.ts (2 tests | 2 skipped) 12ms"));
    assert!(!file_reported("vitest", "tests/a.test.ts", " ✓ tests/a.test.ts.extra (2 tests) 12ms"));
    assert!(file_reported("jest", "tests/a.test.ts", "PASS tests/a.test.ts"));
    assert!(!file_reported("jest", "tests/a.test.ts", "FAIL tests/a.test.ts"));
}
