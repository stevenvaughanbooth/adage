"""Tests for the replay helpers: manifests, the co-change oracle's table, calibration, pooling.

  python3 -m unittest discover evals/replay

No network and no agent: a small git repository with a real history in a temporary directory,
and synthetic runner roots and dead-end rows. With `target/debug/adage` built, one test replays a
synthetic session end to end through `adage eval replay-savings` and checks its known Ŝ.
"""

import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import baseline  # noqa: E402
import calibrate  # noqa: E402
import manifest  # noqa: E402

REPO = HERE.parent.parent
ADAGE = REPO / "target" / "debug" / "adage"
CLICK = {"id": "click", "url": "https://github.com/pallets/click", "test_globs": ["tests/*", "tests/**"],
         "src_globs": ["src/click/*"]}


def git(d, *args):
    return subprocess.run(["git", "-C", str(d), *args], capture_output=True, text=True, check=True).stdout


class CoChange(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp)

    def commit(self, d, files, msg):
        for f in files:
            p = d / f
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(p.read_text() + "x\n" if p.exists() else "x\n")
        git(d, "add", "-A")
        git(d, "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", msg)
        return git(d, "rev-parse", "HEAD").strip()

    def test_ranks_tests_by_co_change_before_the_base_commit(self):
        d = self.tmp / "click"
        d.mkdir()
        git(d, "init", "-q")
        self.commit(d, ["src/click/core.py", "tests/test_core.py"], "a")
        self.commit(d, ["src/click/core.py", "tests/test_core.py", "tests/test_basic.py"], "b")
        self.commit(d, ["src/click/core.py", "tests/test_basic.py"], "c")
        self.commit(d, ["src/click/core.py", "tests/test_core.py"], "d")
        self.commit(d, ["src/click/types.py", "tests/test_types.py"], "e")
        # 40+ files: a sweeping change says nothing about co-change.
        self.commit(d, ["src/click/types.py"] + [f"tests/test_bulk{i}.py" for i in range(40)], "f")
        base = self.commit(d, ["README.md"], "g")
        # After the base: must not count.
        self.commit(d, ["src/click/core.py", "tests/test_late.py"], "h")
        mirrors = self.tmp / "mirrors"
        mirrors.mkdir()
        shutil.copytree(d / ".git", mirrors / "pallets__click")
        tasks = {"tasks": [{"id": "click-1", "base": base}]}
        rb = self.tmp / "rb"
        (rb / "tasks").mkdir(parents=True)
        (rb / "tasks" / "click.json").write_text(json.dumps(tasks))
        old = manifest.RB
        manifest.RB = rb
        try:
            table = manifest.cochange_table(CLICK, ["click-1"], mirrors)
        finally:
            manifest.RB = old
        self.assertEqual(table["click-1"]["src/click/core.py"], ["tests/test_core.py", "tests/test_basic.py"])
        self.assertEqual(table["click-1"]["src/click/types.py"], ["tests/test_types.py"])


class Manifest(unittest.TestCase):
    def test_builds_from_a_runner_root_picking_each_runs_own_transcript(self):
        tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, tmp)
        root = tmp / "runs" / "click"
        runs = {
            "train-click-a-r1": {"phase": "train", "arm": "train", "task": "click-a", "status": "passed",
                                 "metrics": {"session_id": "t1"}},
            "eval-click-b-r1-control": {"phase": "eval", "arm": "control", "task": "click-b", "status": "failed",
                                        "metrics": {"session_id": "s2", "passed": False, "cost_usd": 0.3,
                                                    "result_subtype": "success"}},
            "eval-click-b-r1-adage": {"phase": "eval", "arm": "adage", "task": "click-b", "status": "running"},
        }
        root.mkdir(parents=True)
        (root / "state.json").write_text(json.dumps({"config": "x", "runs": runs}))
        for rid, sids in (("train-click-a-r1", ["t1"]), ("eval-click-b-r1-control", ["s1", "s2"])):
            d = root / "transcripts" / f"-tmp-runs-click-runs-{rid}-repo"
            d.mkdir(parents=True)
            for s in sids:
                (d / f"{s}.jsonl").write_text("{}\n")
        (root / "private" / "bind-sandbox" / "repo").mkdir(parents=True)
        m = manifest.build(root)
        self.assertEqual(m["repo_key"], "github.com/pallets/click")
        self.assertEqual(len(m["train"]), 1)
        self.assertEqual([s["id"] for s in m["sessions"]], ["eval-click-b-r1-control"])
        s = m["sessions"][0]
        self.assertTrue(s["transcript"].endswith("s2.jsonl"), "the session id picks the rerun's transcript")
        self.assertEqual((s["task"], s["passed"], s["cost_usd"], s["stopped"]), ("click-b", False, 0.3, True))
        self.assertTrue(m["root"].endswith("private/bind-sandbox/repo"))


def dead_end_rows():
    """Two tasks: live control needs 4 and 2 calls, adage 1 and 1."""
    rows = []
    for task, ctrl in (("t1", 4), ("t2", 2)):
        for arm, calls in (("control", ctrl), ("adage", 1)):
            rows.append({"root": "adage-rb2/click", "task": task, "arm": arm, "u11_hits": 1, "recovery_calls": calls})
    rows.append({"root": "adage-rb2/vue-core", "task": "v", "arm": "control", "u11_hits": 1, "recovery_calls": 9})
    return rows


class Calibrate(unittest.TestCase):
    def test_live_pairs_and_the_clustered_statistic(self):
        tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, tmp)
        p = tmp / "d.jsonl"
        p.write_text("\n".join(json.dumps(r) for r in dead_end_rows()) + "\n")
        self.assertEqual(calibrate.live_tasks(p), {("click", "t1"), ("click", "t2")})
        live = calibrate.live_by_task(p)
        self.assertEqual(live, {("click", "t1"): 3.0, ("click", "t2"): 1.0})
        m, lo, hi, n = calibrate.clustered(live)
        self.assertEqual((m, n), (2.0, 2))
        self.assertTrue(1.0 <= lo <= m <= hi <= 3.0)
        reports = {"click": {"sessions": [
            {"task": "t1", "dead_end": True, "saved": 3.0}, {"task": "t1", "dead_end": True, "saved": 1.0},
            {"task": "t2", "dead_end": False, "saved": 5.0}, {"task": "t3", "dead_end": True, "saved": 7.0}]}}
        self.assertEqual(calibrate.task_means(reports, {("click", "t1"), ("click", "t2")}), {("click", "t1"): 2.0})


class Pool(unittest.TestCase):
    def test_pooled_is_a_session_mean_with_cluster_bootstrap(self):
        rows = [
            {"_repo": "a", "task": "t1", "net": 2.0, "net_usd": 0.02, "directives": 1, "useful": 1, "harmful": 0, "calls": 10},
            {"_repo": "a", "task": "t1", "net": 0.0, "net_usd": 0.0, "directives": 0, "useful": 0, "harmful": 0, "calls": 10},
            {"_repo": "b", "task": "t1", "net": 1.0, "net_usd": 0.01, "directives": 1, "useful": 0, "harmful": 1, "calls": 20},
        ]
        s = baseline.pooled(rows)
        self.assertEqual((s["sessions"], s["tasks"], s["directives"]), (3, 2, 2))
        self.assertAlmostEqual(s["s_hat"], 1.0)
        self.assertAlmostEqual(s["per_10_calls"], 0.5)
        self.assertEqual((s["useful_rate"], s["harmful_rate"]), (0.5, 0.5))
        self.assertTrue(s["s_hat_ci"][0] <= 1.0 <= s["s_hat_ci"][1])
        self.assertAlmostEqual(baseline.wilson_lb(2, 2), 0.4251, places=3)
        self.assertEqual(baseline.wilson_lb(0, 0), 0.0)


@unittest.skipUnless(ADAGE.exists(), "needs target/debug/adage (cargo build -p adage-cli)")
class EndToEnd(unittest.TestCase):
    """A synthetic control session with U11's dead end, replayed by the binary: Ŝ is known."""

    def test_known_s_hat(self):
        tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, tmp)
        root = tmp / "repo"
        (root / ".venv" / "bin").mkdir(parents=True)
        (root / ".venv" / "pyvenv.cfg").write_text("home = /usr/bin\n")
        (root / ".venv" / "bin" / "python").write_text("")
        key = "github.com/acme/app"
        route = json.loads((HERE / "testdata" / "u11-route.json").read_text())
        route["repo"] = key
        (tmp / "index.json").write_text(json.dumps({"generation": "t", "routes": [route]}))
        calls = [
            ("python -m pytest tests/test_x.py", "/usr/bin/python: No module named pytest", True),
            ("ls -la", ".venv\nsrc", False),
            ("pip install pytest", "Successfully installed pytest", False),
            ("python -m pytest tests/test_x.py", "collected 2 items\n\n2 passed in 0.1s", False),
        ]
        lines = [json.dumps({"type": "user", "sessionId": "s", "cwd": "/w/r", "message": {"content": "Fix it"}})]
        for i, (cmd, out, err) in enumerate(calls):
            lines.append(json.dumps({"type": "assistant", "sessionId": "s", "cwd": "/w/r", "message": {
                "id": f"m{i}", "content": [{"type": "tool_use", "id": f"c{i}", "name": "Bash", "input": {"command": cmd}}]}}))
            lines.append(json.dumps({"type": "user", "sessionId": "s", "cwd": "/w/r", "message": {"content": [
                {"type": "tool_result", "tool_use_id": f"c{i}", "content": out, "is_error": err}]}}))
        (tmp / "s.jsonl").write_text("\n".join(lines) + "\n")
        man = {"name": "acme", "repo_key": key, "root": str(root), "index": str(tmp / "index.json"),
               "sessions": [{"id": "s", "task": "t", "arm": "control", "transcript": str(tmp / "s.jsonl"),
                             "passed": True, "cost_usd": 0.0}]}
        (tmp / "m.json").write_text(json.dumps(man))

        def run(*extra):
            out = subprocess.run([str(ADAGE), "eval", "replay-savings", str(tmp / "m.json"), "--json", *extra],
                                 capture_output=True, text=True, check=True)
            return json.loads(out.stdout)["summary"]

        self.assertEqual(run()["s_hat"], 2.0)  # `ls -la` and `pip install` between the dead end and the run
        self.assertEqual(run("--mode", "pre-emptive")["s_hat"], 3.0)  # and the intercepted call
        self.assertEqual(run("--sat", "strict")["s_hat"], 0.0)  # the trace never used .venv


if __name__ == "__main__":
    unittest.main()
