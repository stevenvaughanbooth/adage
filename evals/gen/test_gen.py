#!/usr/bin/env python3
"""Tests for the procedural sandbox generator. Runs no agent and spends nothing.

  python3 -m unittest discover evals/gen        (or: python3 evals/gen/test_gen.py)

Node-stack bundles are checked when node >= 20.6 and npm are on PATH, and skipped otherwise; the
python-make stack is always checked in full. The rehearsal's learn check runs when an adage binary
is built (target/release/adage or target/debug/adage) and is skipped otherwise.
"""

import contextlib
import io
import json
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(REPO / "evals" / "live"))
import gen  # noqa: E402
import grammar  # noqa: E402
import rehearse  # noqa: E402
import runner  # noqa: E402
import selfcheck  # noqa: E402
from stacks import STACKS  # noqa: E402

NODE_OK, NODE_WHY = selfcheck.node_ok()
SUITE = None
TMP = None


def setUpModule():
    global SUITE, TMP
    TMP = tempfile.TemporaryDirectory(prefix="adage-gen-test-")
    with contextlib.redirect_stdout(io.StringIO()):
        SUITE = gen.build_suite("smoke", Path(TMP.name) / "suite")


def tearDownModule():
    TMP.cleanup()


def bundles(stack=None):
    out = selfcheck.bundles(SUITE)
    if stack:
        out = [b for b in out if json.loads((b / "tasks.json").read_text())["stack"] == stack]
    return out


def usable(b):
    return json.loads((b / "tasks.json").read_text())["stack"] != "node-npm" or NODE_OK


class Grammar(unittest.TestCase):
    def bad(self, spec, fragment):
        with self.assertRaises(grammar.SpecError) as cm:
            grammar.resolve(spec, 1, STACKS)
        self.assertIn(fragment, str(cm.exception))

    def test_invalid_specs_are_refused(self):
        base = {"stack": "python-make", "skin": "billing"}
        self.bad({**base, "stack": "cobol"}, "spec.stack")
        self.bad({**base, "skin": "space"}, "spec.skin")
        self.bad({**base, "colour": 1}, "unknown keys")
        self.bad({**base, "conventions": {"dead_end": "path_alias"}}, "supports modes")
        self.bad({**base, "conventions": {"scope": True}}, "unknown keys")
        self.bad({**base, "conventions": {"finish_line": False}, "tasks": {"registry": 1, "control": 2}}, "finish_line")
        self.bad({**base, "conventions": {"lookup": False, "linked_files": False}}, "need conventions.lookup")
        self.bad({**base, "tasks": {"bugfix": 6, "registry": 2, "control": 1}}, "at least 25%")
        self.bad({**base, "conventions": {"decoys": ["nope"]}}, "conventions.decoys")
        self.bad({**base, "id_digits": [3, 2]}, "id_digits")

    def test_defaults_and_auto_mode(self):
        plan = grammar.resolve({"stack": "node-npm", "skin": "clinic"}, 7, STACKS)
        self.assertIn(plan.conv.dead_end, STACKS["node-npm"].DEAD_END_MODES)
        kinds = [t.kind for t in plan.tasks]
        self.assertEqual((kinds.count("bugfix"), kinds.count("registry"), kinds.count("control")), (4, 2, 2))
        self.assertEqual(grammar.resolve({"stack": "node-npm", "skin": "clinic"}, 7, STACKS).tasks[0].id, plan.tasks[0].id)

    def test_every_stack_skin_and_mode_generates(self):
        from skins import SKINS
        with tempfile.TemporaryDirectory() as t:
            for stack, cls in STACKS.items():
                for skin in SKINS:
                    for mode in cls.DEAD_END_MODES:
                        spec = {"stack": stack, "skin": skin, "conventions": {"dead_end": mode, "decoys": 4},
                                "tasks": {"bugfix": 6, "registry": 3, "control": 3}}
                        out = gen.build(spec, 3, Path(t) / f"{stack}-{skin}-{mode}", repo=False)
                        self.assertTrue((out / "conventions.json").exists())


class Determinism(unittest.TestCase):
    def test_same_seed_same_bytes(self):
        with tempfile.TemporaryDirectory() as t, contextlib.redirect_stdout(io.StringIO()):
            again = gen.build_suite("smoke", Path(t) / "again")
            for b in bundles():
                other = again / b.name
                self.assertEqual(selfcheck.tree_digest(b), selfcheck.tree_digest(other), b.name)
                self.assertEqual(gen.git(b / "repo", "rev-parse", "HEAD"), gen.git(other / "repo", "rev-parse", "HEAD"))
            self.assertEqual((SUITE / "suite.json").read_bytes(), (again / "suite.json").read_bytes())

    def test_other_seed_other_sandbox(self):
        spec = json.loads((bundles()[0] / "spec.json").read_text())
        with tempfile.TemporaryDirectory() as t:
            a = gen.build(spec["spec"], spec["seed"] + 1, Path(t) / "a")
            self.assertNotEqual(selfcheck.tree_digest(a), selfcheck.tree_digest(bundles()[0]))

    def test_repo_only_matches_the_bundle(self):
        b = bundles("python-make")[0]
        spec = json.loads((b / "spec.json").read_text())
        with tempfile.TemporaryDirectory() as t, contextlib.redirect_stdout(io.StringIO()):
            # From the bundle's spec.json, and from the canonical inline JSON the live configs embed.
            self.assertEqual(gen.main(["--spec", str(b / "spec.json"), "--seed", str(spec["seed"]),
                                       "--repo-only", str(Path(t) / "r")]), 0)
            self.assertEqual(gen.main(["--spec-json", grammar.canonical(spec["spec"]), "--seed", str(spec["seed"]),
                                       "--repo-only", str(Path(t) / "s")]), 0)
            self.assertEqual(gen.git(Path(t) / "r", "rev-parse", "HEAD"), spec["commit"])
            self.assertEqual(gen.git(Path(t) / "s", "rev-parse", "HEAD"), spec["commit"])


class Schema(unittest.TestCase):
    CONV_KEYS = {"id", "kind", "command", "avoided", "signature", "glob", "id_pattern", "notes", "mode"}

    def test_conventions(self):
        for b in bundles():
            conv = json.loads((b / "conventions.json").read_text())
            ids = [c["id"] for c in conv]
            self.assertEqual(len(ids), len(set(ids)))
            for c in conv:
                self.assertLessEqual(set(c), self.CONV_KEYS, c)
                self.assertIn(c["kind"], grammar.KINDS)
                self.assertTrue(c["command"] and c["notes"])
                if c["kind"] in ("dead_end", "instinct_violation"):
                    self.assertTrue(c["avoided"] and c["signature"])
                if c["kind"] == "finish_line":
                    self.assertTrue(c["glob"] and c["signature"])
                if c["kind"] == "lookup":
                    re.compile(c["id_pattern"])
                    self.assertIn("{item}", c["command"])
                if c["kind"] == "dead_end":
                    self.assertIn(c["mode"], grammar.DEAD_END_MODES)

    def test_tasks(self):
        for b in bundles():
            tasks = json.loads((b / "tasks.json").read_text())
            conv_ids = {c["id"] for c in json.loads((b / "conventions.json").read_text())}
            ts = tasks["tasks"]
            control = [t for t in ts if t["kind"] == "control"]
            self.assertGreaterEqual(len(control) * 4, len(ts), "at least 25% control tasks")
            for t in ts:
                self.assertIn(t["kind"], grammar.TASK_KINDS)
                self.assertIn(t["split"], ("train", "eval"))
                self.assertGreaterEqual(len(t["prompts"]), 2)
                self.assertLessEqual(set(t["exercises"]), conv_ids)
                self.assertTrue((b / t["hidden"]).exists())
                self.assertGreater(t["hidden_tests"], 0)
                self.assertFalse(str(b / t["hidden"]).startswith(str(b / "repo")))
                if t["kind"] == "control":
                    self.assertEqual(t["exercises"], [])
                if t["kind"] == "registry":
                    self.assertIn("finish-line", t["exercises"])
            self.assertLessEqual(set(tasks["session_conventions"]), conv_ids)

    def test_live_configs_plan_with_the_runner(self):
        for b in bundles():
            for name in ("config.json", "config-control.json"):
                cfg = runner.load_config(b / name, {})
                p = runner.describe(cfg, ["train", "learn", "eval", "report"], [1], Path("/nonexistent/root"),
                                    Path("adage"))
                self.assertEqual(p["totals"]["sessions"], p["estimate"]["total_runs"])
                s = p["sessions"][0]
                self.assertEqual(s["sandbox"][1], str(REPO / "evals" / "gen" / "gen.py"))
                json.loads(s["sandbox"][3])  # the inline spec survives the runner's formatting

    def test_live_config_commands_run(self):
        b = bundles("python-make")[0]
        cfg = runner.load_config(b / "config.json", {})
        task = cfg["train"]["tasks"][0]
        with tempfile.TemporaryDirectory() as t:
            d = Path(t) / "runs" / "x" / cfg["workdir"]
            d.parent.mkdir(parents=True)
            subprocess.run(runner.expand(cfg["sandbox"], dir=str(d), task=task), check=True, capture_output=True)
            self.assertEqual(gen.git(d, "rev-parse", "HEAD"), json.loads((b / "spec.json").read_text())["commit"])
            out = subprocess.run(runner.expand(cfg["score"], dir=str(d), task=task), capture_output=True, text=True)
            r = json.loads(out.stdout)
            self.assertFalse(r["passed"])
            self.assertGreater(r["hidden_tests_run"], 0)
            ctl = runner.load_config(b / "config-control.json", {})
            c = Path(t) / "runs" / "y" / cfg["workdir"]
            c.parent.mkdir(parents=True)
            subprocess.run(runner.expand(ctl["sandbox"], dir=str(c), task=ctl["train"]["tasks"][0]), check=True,
                           capture_output=True)
            self.assertIn("Brief", (c / "BRIEF.md").read_text())

    def test_bundle_scorer_is_self_contained(self):
        b = bundles("python-make")[0]
        with tempfile.TemporaryDirectory() as t:
            moved = Path(t) / "moved"
            shutil.copytree(b, moved, symlinks=True)
            tid = json.loads((moved / "tasks.json").read_text())["tasks"][0]["id"]
            out = subprocess.run([sys.executable, str(moved / "score.py"), str(moved / "repo"), tid],
                                 capture_output=True, text=True, cwd=t)
            self.assertEqual(out.returncode, 0, out.stderr)
            self.assertFalse(json.loads(out.stdout)["passed"])


class SelfCheck(unittest.TestCase):
    def test_smoke_suite_selfcheck(self):
        ran = 0
        for b in bundles():
            if not usable(b):
                continue
            c = selfcheck.Checker(b)
            try:
                with contextlib.redirect_stdout(io.StringIO()):
                    status = c.run_all()
            finally:
                c.tmp.cleanup()
            self.assertEqual(status, "ok", f"{b.name}: {c.failed}\n" + "\n".join(c.lines))
            ran += 1
        self.assertGreaterEqual(ran, len(bundles("python-make")))
        if not NODE_OK:
            self.skipTest(f"node-npm bundles not checked: {NODE_WHY}")


class Rehearsal(unittest.TestCase):
    def test_transcripts_are_written(self):
        b = bundles("python-make")[0]
        with tempfile.TemporaryDirectory() as t:
            rep = rehearse.rehearse_bundle(b, Path(t) / "r", 1, None, learn=False)
            self.assertTrue(rep["all_passed"])
            files = list((Path(t) / "r" / "transcripts").glob("*/*.jsonl"))
            self.assertEqual(len(files), len(json.loads((b / "tasks.json").read_text())["tasks"]))
            for f in files:
                recs = [json.loads(line) for line in f.read_text().splitlines()]
                self.assertEqual(recs[0]["type"], "user")
                self.assertTrue(all(r["sessionId"] == recs[0]["sessionId"] for r in recs))
                uses = [c for r in recs if r["type"] == "assistant" for c in r["message"]["content"]]
                self.assertTrue(all(u["type"] == "tool_use" for u in uses))

    def test_yaml_subset(self):
        text = ("id: x\nstep:\n  do: avoid\n  avoid: node\n  signature: 'error: ''test failed'''\nslots: []\n"
                "exit:\n- when: intent_changes\nevidence:\n  rate: 1.0\n  models:\n  - a\n")
        self.assertEqual(rehearse.yaml_load(text), {
            "id": "x", "step": {"do": "avoid", "avoid": "node", "signature": "error: 'test failed'"}, "slots": [],
            "exit": [{"when": "intent_changes"}], "evidence": {"rate": 1.0, "models": ["a"]}})

    def test_learn_recovers_every_convention(self):
        exe = rehearse.find_adage(None)
        if not exe:
            self.skipTest("no adage binary (cargo build -p adage-cli)")
        for b in bundles():
            if not usable(b):
                continue
            with tempfile.TemporaryDirectory() as t:
                rep = rehearse.rehearse_bundle(b, Path(t) / "r", 2, exe, learn=True)
            self.assertTrue(rep["ok"], json.dumps({k: rep.get(k) for k in
                                                   ("comparison", "unexpected_routes", "learn_error")}, indent=1)[:4000])
            self.assertEqual(rep["recovered"], rep["expected"])
            self.assertEqual(rep["decoys_learned"], 0)


if __name__ == "__main__":
    unittest.main()
