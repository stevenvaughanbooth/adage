"""Tests for RepoBench-Local: task mining, validation, the scorer, prompts, the split, the report.

  python3 -m unittest discover evals/repobench

Nothing here spends tokens or touches the network: every test builds a small git repository with
a real history (a bug, then a fix commit with its test) in a temporary directory and points the
suite's cache at it.
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import agents_md  # noqa: E402
import build  # noqa: E402
import common  # noqa: E402
import gen_doc  # noqa: E402
import mine  # noqa: E402
import prompts  # noqa: E402
import make_report as report  # noqa: E402
import sandbox  # noqa: E402
import score  # noqa: E402
import validate  # noqa: E402

IDENT = ["-c", "user.name=t", "-c", "user.email=t@localhost", "-c", "commit.gpgsign=false"]
PY = sys.executable
# The toy repository's test runner: unittest over the given files, reporting as pytest does.
RUN_TESTS = """import importlib.util, sys, unittest
suite = unittest.TestSuite()
for i, f in enumerate(sys.argv[1:]):
    spec = importlib.util.spec_from_file_location(f"hidden{i}", f)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    for name in dir(mod):
        if name.startswith("test"):
            suite.addTest(unittest.FunctionTestCase(getattr(mod, name)))
res = unittest.TextTestRunner().run(suite)
bad = len(res.failures) + len(res.errors)
print(f"{res.testsRun - bad} passed, {bad} failed")
sys.exit(1 if bad or not res.testsRun else 0)
"""


def git(d, *a, date=None):
    env = dict(os.environ)
    if date:
        env.update(GIT_AUTHOR_DATE=date, GIT_COMMITTER_DATE=date)
    return subprocess.run(["git", "-C", str(d), *IDENT, *a], check=True, capture_output=True, text=True,
                          env=env).stdout.strip()


def write(d, rel, text):
    p = Path(d) / rel
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(text)


class Fixture(unittest.TestCase):
    """A toy upstream: calc.add is broken, then fixed with a test (the task), then a docs-only
    change, then a fix whose test passes at the base too (not a task)."""

    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="rb-test-"))
        self.cache = self.tmp / "cache"
        self.up = self.cache / "mirrors" / "example__calc"
        self.up.mkdir(parents=True)
        git(self.up, "init", "-q", "-b", "main")
        write(self.up, "src/calc.py", "def add(a, b):\n    return a - b\n\ndef mul(a, b):\n    return a * b\n")
        write(self.up, "tests/test_mul.py", "from calc import mul\n\ndef test_mul():\n    assert mul(2, 3) == 6\n")
        git(self.up, "add", "-A")
        git(self.up, "commit", "-q", "-m", "Initial", date="2025-01-01T00:00:00+00:00")
        write(self.up, "src/calc.py", "def add(a, b):\n    return a + b\n\ndef mul(a, b):\n    return a * b\n")
        write(self.up, "tests/test_add.py", "from calc import add\n\ndef test_add_positive_numbers():\n    assert add(2, 3) == 5\n")
        git(self.up, "add", "-A")
        git(self.up, "commit", "-q", "-m", "Fix add returning the difference", date="2025-02-01T00:00:00+00:00")
        self.fix = git(self.up, "rev-parse", "HEAD")
        write(self.up, "README.md", "calc\n")
        git(self.up, "add", "-A")
        git(self.up, "commit", "-q", "-m", "Fix typo in docs", date="2025-03-01T00:00:00+00:00")
        write(self.up, "src/calc.py", "def add(a, b):\n    return a + b\n\ndef mul(a, b):\n    return b * a\n")
        write(self.up, "tests/test_mul.py", "from calc import mul\n\ndef test_mul():\n    assert mul(2, 3) == 6\n\n"
                                             "def test_mul_zero():\n    assert mul(0, 3) == 0\n")
        git(self.up, "add", "-A")
        git(self.up, "commit", "-q", "-m", "Fix mul argument order", date="2025-04-01T00:00:00+00:00")
        self.repo = {"id": "calc", "url": "https://github.com/example/calc", "lang": "python", "since": "2024-12-01",
                     "until": "2026-01-01", "test_globs": ["tests/*"], "src_globs": ["src/*"], "max_src_files": 2,
                     "max_src_lines": 20, "setup": [], "rescue": [],
                     "hidden": f"PYTHONPATH=src {PY} {self.tmp}/run_tests.py {{tests}}",
                     "hidden_timeout": 60}
        (self.tmp / "run_tests.py").write_text(RUN_TESTS)
        self.tasks_dir = self.tmp / "tasks"
        self.patches = [
            mock.patch.dict(os.environ, {"REPOBENCH_CACHE": str(self.cache)}),
            mock.patch.object(common, "load_repos", lambda: {"calc": self.repo}),
            mock.patch.object(common, "TASKS_DIR", self.tasks_dir),
        ]
        for p in self.patches:
            p.start()

    def tearDown(self):
        for p in reversed(self.patches):
            p.stop()
        shutil.rmtree(self.tmp, ignore_errors=True)


class MineTest(Fixture):
    def test_candidates_need_a_fix_subject_source_and_tests(self):
        res = mine.mine("calc")
        subjects = [c["subject"] for c in res["candidates"]]
        self.assertIn("Fix add returning the difference", subjects)
        self.assertIn("Fix mul argument order", subjects)
        self.assertNotIn("Fix typo in docs", subjects)  # no source, no test; and a typo
        c = next(c for c in res["candidates"] if c["sha"] == self.fix)
        self.assertEqual(c["src_files"], ["src/calc.py"])
        self.assertEqual(c["test_files"], ["tests/test_add.py"])
        self.assertEqual(c["src_lines"], 2)

    def test_parse_log_reads_renames(self):
        text = "@@abc\x00p1\x002025-01-01\x00Fix x\x00\x00\n3\t1\tsrc/{a.py => b.py}\n"
        [c] = mine.parse_log(text)
        self.assertEqual(c["files"], [("src/b.py", 3, 1)])


class ValidateTest(Fixture):
    def test_a_real_fix_validates_and_a_test_that_passes_at_base_does_not(self):
        cands = mine.mine("calc")["candidates"]
        work = self.tmp / "work"
        work.mkdir()
        verdicts = {c["subject"]: validate.check(self.repo, c, work, 60) for c in cands}
        good = verdicts["Fix add returning the difference"]
        self.assertTrue(good["ok"], good)
        t = good["task"]
        self.assertEqual(t["base"], git(self.up, "rev-parse", f"{self.fix}^"))
        self.assertEqual(t["gold_files"], ["src/calc.py"])
        self.assertEqual(t["test_files"], ["tests/test_add.py"])
        bad = verdicts["Fix mul argument order"]
        self.assertFalse(bad["ok"])
        self.assertIn("pass at the base", bad["reason"])
        p = validate.merge("calc", [t])
        self.assertEqual(json.loads(p.read_text())["repo_key"], "github.com/example/calc")

    def test_manifest_changes_are_refused(self):
        c = mine.mine("calc")["candidates"][0]
        with mock.patch.object(common, "git", lambda *a, **k: "src/calc.py\npyproject.toml\ntests/test_add.py\n"):
            v = validate.check(self.repo, c, self.tmp, 60)
        self.assertFalse(v["ok"])
        self.assertIn("manifests", v["reason"])


class SandboxAndScoreTest(Fixture):
    def task(self):
        c = next(c for c in mine.mine("calc")["candidates"] if c["sha"] == self.fix)
        v = validate.check(self.repo, c, self.tmp / "w", 60)
        validate.merge("calc", [v["task"]])
        return v["task"]

    def test_the_sandbox_has_no_history_no_fix_and_the_upstream_origin(self):
        t = self.task()
        d = self.tmp / "sb" / "repo"
        sandbox.create(self.repo, t["base"], d)
        self.assertEqual(git(d, "rev-list", "--count", "HEAD"), "1")
        self.assertIn("a - b", (d / "src/calc.py").read_text())
        self.assertFalse((d / "tests/test_add.py").exists())
        self.assertEqual(git(d, "remote", "get-url", "origin"), "https://github.com/example/calc")
        self.assertEqual(common.repo_key(self.repo), "github.com/example/calc")

    def test_context_file_only_for_the_agents_md_arm(self):
        t = self.task()
        ctx = self.tmp / "ctx.md"
        ctx.write_text("# AGENTS.md\n- run tests with X\n")
        for arm, expect in (("control", False), ("adage", False), ("agents-md", True)):
            d = self.tmp / arm / "repo"
            sandbox.main(["calc", t["id"], str(d), "--arm", arm, "--context", str(ctx)])
            self.assertEqual((d / "CLAUDE.md").exists(), expect, arm)
            self.assertEqual(git(d, "status", "--porcelain"), "")  # committed, not left untracked

    def test_scorer_fails_the_base_passes_the_fix_and_ignores_the_agents_test_edits(self):
        t = self.task()
        d = self.tmp / "sb2" / "repo"
        sandbox.create(self.repo, t["base"], d)
        res = score.score("calc", t["id"], d)
        self.assertFalse(res["passed"])
        self.assertFalse(res["touched_gold"])
        # The agent fixes the source, commits it, and rewrites the hidden test to pass trivially.
        write(d, "src/calc.py", "def add(a, b):\n    return b + a\n\ndef mul(a, b):\n    return a * b\n")
        write(d, "tests/test_add.py", "def test_add_positive_numbers():\n    assert True\n")
        git(d, "add", "-A")
        git(d, "commit", "-q", "-m", "fix")
        res = score.score("calc", t["id"], d)
        self.assertTrue(res["passed"], res["tail"])
        self.assertTrue(res["touched_gold"])
        self.assertEqual(res["changed_files"], ["src/calc.py"])
        self.assertIn("assert add(2, 3) == 5", (d / "tests/test_add.py").read_text())  # the real test ran

    def test_a_wrong_fix_fails(self):
        t = self.task()
        d = self.tmp / "sb3" / "repo"
        sandbox.create(self.repo, t["base"], d)
        write(d, "src/calc.py", "def add(a, b):\n    return 5\n\ndef mul(a, b):\n    return a * b\n")
        self.assertTrue(score.score("calc", t["id"], d)["passed"])  # the test is weak, the scorer is honest
        write(d, "src/calc.py", "def add(a, b):\n    return a * b\n\ndef mul(a, b):\n    return a * b\n")
        self.assertFalse(score.score("calc", t["id"], d)["passed"])

    def test_runnable_and_counts(self):
        self.assertTrue(score.runnable("python", "tests/test_x.py"))
        self.assertFalse(score.runnable("python", "tests/conftest.py"))
        self.assertTrue(score.runnable("typescript", "packages/a/__tests__/b.spec.ts"))
        self.assertFalse(score.runnable("typescript", "packages/a/__tests__/__snapshots__/b.spec.ts.snap"))
        self.assertTrue(score.runnable("go", "flag_test.go"))
        self.assertEqual(score.count_tests("python", "3 passed, 1 failed in 0.2s"), 4)
        self.assertEqual(score.count_tests("typescript", " Tests  12 passed (12)"), 12)
        self.assertEqual(score.count_tests("python", "no tests ran"), 0)
        r = {"hidden": "go test -count=1 {pkgs}", "lang": "go"}
        self.assertEqual(score.hidden_command(r, {"test_files": ["a_test.go", "sub/b_test.go", "testdata/x"]}),
                         "go test -count=1 ./ ./sub")


class PromptTest(Fixture):
    def test_leaks_are_refused(self):
        t = {"id": "calc-x", "sha": self.fix, "base": git(self.up, "rev-parse", f"{self.fix}^"),
             "test_files": ["tests/test_add.py"]}
        ok = "add(2, 3) returns -1 instead of 5. Please fix add so it returns the sum."
        self.assertEqual(prompts.leaks(self.repo, t, ok), [])
        for bad in ("See #1234 for details. Fix add.", "The test tests/test_add.py fails; fix it.",
                    "test_add_positive_numbers fails, please fix.", "Fix add and add a regression test.",
                    "Upstream fixed this on GitHub.", ""):
            self.assertTrue(prompts.leaks(self.repo, t, bad), bad)

    def test_parse_json_tolerates_fences(self):
        self.assertEqual(prompts.parse_json('```json\n{"a": "b"}\n```'), {"a": "b"})


class SplitTest(Fixture):
    def test_chronological_split_with_buffer_and_digest(self):
        tasks = [{"id": f"calc-{i:02d}", "sha": f"{i:040x}", "date": f"2025-01-{i:02d}T00:00:00+00:00",
                  "test_files": [], "gold_files": []} for i in range(1, 15)]
        sp = build.split("calc", tasks, 0.5, 2)
        part = sp["repos"]["calc"]
        self.assertEqual(part["train"], [f"calc-{i:02d}" for i in range(1, 8)])
        self.assertEqual(part["buffer"], ["calc-08", "calc-09"])
        self.assertEqual(part["test"], [f"calc-{i:02d}" for i in range(10, 15)])
        self.assertTrue(all(part["dates"][x] > part["cutoff"] for x in part["test"]))
        self.assertEqual(len(sp["digest"]), 64)
        again = build.split("calc", list(reversed(tasks)), 0.5, 2)
        self.assertEqual(again["digest"], sp["digest"])

    def test_config_is_confined_host_auth_and_per_task_prompts(self):
        tasks = [{"id": f"calc-{i:02d}", "sha": f"{i:040x}", "date": f"2025-01-{i:02d}T00:00:00+00:00",
                  "test_files": [], "gold_files": [], "prompt": f"fix {{thing}} {i}"} for i in range(1, 15)]
        sp = build.split("calc", tasks, 0.5, 2)
        cfg = build.config("calc", sp, tasks, ["control", "adage", "agents-md"], 1, 2, "m", 30, 5.0)
        self.assertTrue(cfg["confine"] and cfg["host_auth"])
        self.assertEqual(cfg["repo_key"], "github.com/example/calc")
        self.assertIn("{arm}", cfg["sandbox"])
        sys.path.insert(0, str(common.REPO / "evals" / "live"))
        import runner
        p = self.tmp / "cfg.json"
        p.write_text(json.dumps(cfg))
        loaded = runner.load_config(p, {})
        rows = runner.plan(loaded, ["train", "eval"], 1)
        first = rows[0]
        self.assertEqual(first[4].format(id=first[3]), "fix {thing} 1")  # braces survive
        self.assertIn("agents-md", {r[2] for r in rows})
        self.assertEqual(runner.env_for("agents-md", "/h")["ADAGE_ARM"], "control")


class ReportTest(unittest.TestCase):
    def run_(self, task, arm, passed, cost, turns=10, repo="r"):
        return {"task": f"{repo}/{task}", "repo": repo, "arm": arm,
                "metrics": {"passed": passed, "cost_usd": cost, "turns": turns, "status": "ok", "tool_calls": 3,
                            "turns_to_first_edit": 2, "elapsed": 10.0, "explore_calls_before_edit": 1,
                            "dead_end_hits": 0, "hidden_passed": passed}}

    def test_per_resolved_and_harm(self):
        runs = [self.run_("t1", "control", True, 1.0), self.run_("t1", "adage", False, 0.5),
                self.run_("t2", "control", False, 1.0), self.run_("t2", "adage", True, 0.5),
                self.run_("t3", "control", True, 1.0), self.run_("t3", "adage", True, 0.5)]
        self.assertAlmostEqual(report.per_resolved([r for r in runs if r["arm"] == "control"]), 1.5)
        self.assertAlmostEqual(report.per_resolved([r for r in runs if r["arm"] == "adage"]), 0.75)
        self.assertEqual(report.harm(runs, "adage"), (1, 1, 3))
        c = report.comparison(runs, "adage", "control", 30)
        self.assertAlmostEqual(c["per_resolved"]["diff"], -0.75)
        rec = report.arms_record("repobench-local", "all", "suite", None, c, "0.1.0", 6)
        self.assertEqual(rec["tier"], "l3")
        self.assertEqual(rec["schema_version"], 2)
        self.assertRegex(rec["run_id"], r"^[0-9a-f]{16}$")
        self.assertLessEqual(len(rec["deltas"]), 32)
        self.assertEqual(rec["harm"], {"adage_worse_than_control": 1, "adage_better_than_control": 1})

    def test_params_hash_matches_the_l0_baseline(self):
        line = (common.REPO / "evals" / "baselines" / "l0-core.jsonl").read_text().splitlines()[0]
        base = json.loads(line)
        self.assertEqual(base["params"], report.DEMO_PARAMS)

    def test_record_satisfies_the_schema_envelope(self):
        schema = json.loads((common.REPO / "evals" / "bench" / "result.schema.json").read_text())
        self.assertIn("l3", schema["properties"]["tier"]["enum"])
        runs = [self.run_(f"t{i}", arm, i % 2 == 0, 0.1 * i) for i in range(4) for arm in ("adage", "control")]
        rec = report.arms_record("s", "all", "suite", None, report.comparison(runs, "adage", "control", 30), "0.1.0", 8)
        for k in schema["required"]:
            self.assertIn(k, rec)
        for k in rec:
            self.assertIn(k, set(schema["properties"]) | {"arms", "deltas", "harm", "sessions"})
        self.assertTrue(all(isinstance(v, (int, float)) for x in rec["arms"].values() for v in x.values()))

    def test_svgs_are_well_formed(self):
        import xml.dom.minidom
        s = report.svg_bars("t", ["a", "b"], ["control", "adage"], {("a", "control"): 1.0, ("b", "adage"): 2.0}, {},
                            lambda v: f"{v:.1f}", "")
        xml.dom.minidom.parseString(s)
        c = report.svg_curve("t", {"control": [1, 2, None, 3], "adage": [3, 2, 1, 1]}, "y", lambda v: f"{v:.0f}")
        xml.dom.minidom.parseString(c)

    def test_convention_matching(self):
        state = {"tracks": [{"route": {"id": "r1", "step": {"run": "source .venv/bin/activate && pytest tests/x.py"}}},
                            {"route": {"id": "r2", "step": {"run": "make lint"}}}]}
        conv = {"conventions": [{"id": "tests", "command": ".venv/bin/python -m pytest"},
                                {"id": "venv", "command": "pytest"}]}
        m = report.match_conventions(state, conv)
        self.assertEqual(m["found"], ["venv"])
        self.assertEqual(m["unmatched_routes"], ["r2"])


class ContextFilesTest(unittest.TestCase):
    def test_gen_doc_states_every_convention_and_no_decoy(self):
        conv = {"conventions": [
            {"id": "l", "kind": "lookup", "command": "./bin/fetch-ticket {item}", "samples": ["OPS-1"]},
            {"id": "lf", "kind": "linked_files", "command": "./bin/fetch-ticket {item}"},
            {"id": "d", "kind": "dead_end", "command": "npm run check", "avoided": ["node --test"]},
            {"id": "v", "kind": "dead_end", "command": "npm run check", "avoided": ["npm test"]},
            {"id": "f", "kind": "finish_line", "command": "npm run codegen", "glob": "registry/*.csv"}],
            "decoys": [{"id": "x", "kind": "decoy", "command": "npm run gen:docs"}]}
        text = gen_doc.doc_text(conv)
        self.assertIn("./bin/fetch-ticket OPS-1", text)
        self.assertIn("`node --test` and `npm test` do not work", text)
        self.assertEqual(text.count("npm run check"), 1)
        self.assertIn("npm run codegen", text)
        self.assertNotIn("gen:docs", text)

    def test_agents_md_reads_only_training_commands(self):
        state = {"runs": {
            "train-a": {"phase": "train", "metrics": {"passed": True},
                        "events": [{"tool": "Bash", "cmd": "pytest -x"}, {"tool": "Read", "cmd": "/x/y.py"},
                                   {"tool": "Bash", "cmd": "make test", "failed": True}]},
            "eval-b": {"phase": "eval", "metrics": {"passed": True}, "events": [{"tool": "Bash", "cmd": "SECRET"}]}}}
        lines = agents_md.session_lines(state)
        self.assertEqual(len(lines), 1)
        self.assertIn("- pytest -x", lines[0])
        self.assertIn("- FAILED make test", lines[0])
        self.assertNotIn("SECRET", lines[0])
        self.assertNotIn("y.py", lines[0])


class RealTaskFilesTest(unittest.TestCase):
    """The committed task definitions: well-formed, prompts clean, splits consistent."""

    def test_committed_tasks(self):
        repos = common.load_repos()
        for p in sorted(common.TASKS_DIR.glob("*.json")):
            data = json.loads(p.read_text())
            r = repos[data["repo"]]
            self.assertEqual(data["repo_key"], common.repo_key(r))
            for t in data["tasks"]:
                self.assertRegex(t["sha"], r"^[0-9a-f]{40}$")
                self.assertRegex(t["base"], r"^[0-9a-f]{40}$")
                self.assertTrue(t["test_files"] and t["gold_files"], t["id"])
                self.assertFalse(set(t["test_files"]) & set(t["gold_files"]), t["id"])
                self.assertLessEqual(t["fixed_seconds"], 60, t["id"])
                if t.get("prompt"):
                    self.assertNotIn(t["sha"][:7], t["prompt"])
                    for tf in t["test_files"]:
                        self.assertNotIn(tf.split("/")[-1], t["prompt"], t["id"])
            sp = common.HERE / "splits" / f"{data['repo']}.json"
            if sp.exists():
                split = json.loads(sp.read_text())["repos"][data["repo"]]
                ids = {t["id"] for t in data["tasks"]}
                self.assertTrue(set(split["train"]) | set(split["test"]) <= ids)
                self.assertFalse(set(split["train"]) & set(split["test"]))
                self.assertLess(max(split["dates"][x] for x in split["train"]),
                                min(split["dates"][x] for x in split["test"]) + "~")


if __name__ == "__main__":
    unittest.main()
