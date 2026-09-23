#!/usr/bin/env python3
"""Smoke test of the live runner's dry run. Runs no agent and spends nothing.

  python3 -m unittest evals/live/test_runner.py        (or: python3 evals/live/test_runner.py)
"""

import contextlib
import io
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import runner  # noqa: E402

CONFIG = str(HERE / "configs" / "acme-billing.json")
HARBOR = str(HERE / "configs" / "harbor-dispatch.json")


def dry_with(*args):
    out = io.StringIO()
    with contextlib.redirect_stdout(out):
        assert runner.main(list(args)) == 0
    return out.getvalue()


def dry(*args):
    return dry_with(CONFIG, *args)


class DryRun(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name) / "live"

    def tearDown(self):
        self.tmp.cleanup()

    def plan(self, *args):
        return json.loads(dry("--json", "--root", str(self.root), *args))

    def test_the_demo_plan(self):
        p = self.plan()
        self.assertEqual(p["totals"]["sessions"], 54)
        self.assertEqual(p["totals"]["by_arm"], {"adage": 18, "holdout": 18, "train": 18})
        for s in p["sessions"]:
            self.assertEqual(s["command"][:3], ["claude", "-p", s["prompt"]])
            self.assertIn("claude-haiku-4-5-20251001", s["command"])
            self.assertIn(s["task"], s["prompt"])
            self.assertTrue(s["cwd"].startswith(str(self.root.resolve())))
        by_arm = {s["arm"]: s["env"] for s in p["sessions"]}
        self.assertEqual(by_arm["train"], {}, "training sessions run without hooks")
        self.assertEqual(by_arm["holdout"]["ADAGE_ARM"], "holdout")
        self.assertEqual(by_arm["adage"]["ADAGE_ARM"], "treated")
        self.assertIn("learn", p["learn"])

    def test_a_dry_run_writes_nothing(self):
        text = dry("--root", str(self.root))
        self.assertIn("DRY RUN", text)
        self.assertFalse(self.root.exists())

    def test_overrides_and_later_rounds(self):
        p = self.plan("--model", "claude-sonnet-5", "--round", "2", "eval")
        self.assertEqual(p["totals"]["sessions"], 36, "round 2 runs no training sessions")
        self.assertTrue(all(s["run"].startswith("eval2-") for s in p["sessions"]))
        self.assertTrue(all("claude-sonnet-5" in s["command"] for s in p["sessions"]))
        self.assertIsNone(p["learn"])

    def test_execute_is_refused_inside_an_agent_session(self):
        old = os.environ.get("CLAUDECODE")
        os.environ["CLAUDECODE"] = "1"
        try:
            with self.assertRaises(SystemExit):
                runner.main([CONFIG, "--execute", "--root", str(self.root)])
        finally:
            if old is None:
                del os.environ["CLAUDECODE"]
            else:
                os.environ["CLAUDECODE"] = old
        self.assertFalse(self.root.exists())

    def test_the_second_demo_plan(self):
        p = json.loads(dry_with(HARBOR, "--json", "--root", str(self.root)))
        self.assertEqual(p["rounds"], [1, 2])
        self.assertEqual(p["estimate"]["total_runs"], 84)
        self.assertEqual(p["estimate"]["train"]["runs"], 12)
        self.assertEqual(p["estimate"]["eval"]["runs"], 72)
        self.assertEqual(p["totals"]["by_arm"], {"adage": 36, "holdout": 36, "train": 12})
        self.assertEqual(p["totals"]["by_round"], {"1": 48, "2": 36})
        self.assertTrue(all("claude-sonnet-5" in s["command"] for s in p["sessions"]))
        self.assertTrue(all(s["run"].startswith("eval2-") for s in p["sessions"] if s["round"] == 2))
        self.assertFalse(any(s["phase"] == "train" for s in p["sessions"] if s["round"] == 2))
        self.assertTrue(all(s["task"].startswith("INC-") and s["task"] in s["prompt"] for s in p["sessions"]))

    def test_config_flag_and_explicit_dry_run(self):
        text = dry_with("--config", HARBOR, "--dry-run", "--root", str(self.root))
        self.assertIn("DRY RUN", text)
        self.assertIn("total 84 agent runs", text)
        self.assertIn("6 tasks x 3 repeats x 2 arms (holdout, adage) x 2 rounds = 72", text)
        self.assertFalse(self.root.exists())
        with self.assertRaises(SystemExit):
            runner.main(["--config", HARBOR, "--dry-run", "--execute"])

    def test_one_round_of_a_multi_round_config(self):
        p = json.loads(dry_with(HARBOR, "--json", "--root", str(self.root), "--round", "2", "learn", "eval"))
        self.assertEqual(p["totals"]["sessions"], 36)
        self.assertEqual(p["learn_rounds"], [2])

    def test_classification_of_the_second_sandbox(self):
        cfg = runner.load_config(HARBOR, {})
        c = lambda cmd, failed=False: runner.classify(cfg, "Bash", {"command": cmd}, failed)
        self.assertEqual(c("./bin/fetch-incident INC-86"), "lookup")
        self.assertEqual(c("npm test", failed=True), "verify-fail")
        self.assertEqual(c("npm run check"), "verify")
        self.assertEqual(c("npm run codegen"), "regen")
        self.assertEqual(c("node tools/codegen.mjs --verify"), "verify")
        self.assertEqual(c("ls src"), "explore")
        self.assertEqual(runner.classify(cfg, "Edit", {"file_path": "/x/registry/ports.csv"}), "edit")
        # The first demo keeps its own categories.
        acme = runner.load_config(CONFIG, {})
        self.assertEqual(runner.classify(acme, "Bash", {"command": "python3 scripts/wi-lookup W-2042"}), "lookup")
        self.assertEqual(runner.classify(acme, "Bash", {"command": "make gen"}), "regen")

    def test_metrics_follow_the_obligation(self):
        cfg = runner.load_config(HARBOR, {})
        wd = "/w/harbor-dispatch"
        ev = lambda turn, tool, cat, cmd, failed=False: {"t": turn * 2.0, "turn": turn, "tool": tool, "id": str(turn),
                                                         "cat": cat, "cmd": cmd, "failed": failed}
        events = [ev(1, "Bash", "lookup", "./bin/fetch-incident INC-245"), ev(2, "Read", "explore", f"{wd}/registry/ports.csv"),
                  ev(3, "Edit", "edit", f"{wd}/registry/ports.csv"), ev(4, "Bash", "verify-fail", "npm test", True),
                  ev(5, "Bash", "regen", "npm run codegen"), ev(6, "Bash", "verify", "npm run check")]
        spool = [{"kind": "would_have", "route": "lookup-work-item"}, {"kind": "followed", "route": "lookup-work-item"},
                 {"kind": "would_have", "route": "dead-end-x"}]
        m = runner.run_metrics(cfg, events, {"num_turns": 7, "total_cost_usd": 0.1}, {"passed": True, "hidden_passed": True},
                               wd, 12.0, "sid", spool)
        self.assertEqual((m["turns_to_first_edit"], m["dead_end_hits"], m["lookup_turn"]), (2, 1, 1))
        self.assertEqual((m["obligation_applies"], m["obligation_met"]), (True, True))
        self.assertEqual(m["follow"], {"exposed": 2, "followed": 1, "derailed": 0})
        m2 = runner.run_metrics(cfg, events[:4], {}, {"passed": False, "stale_generated": True}, wd, 1.0, "sid", [])
        self.assertEqual((m2["obligation_applies"], m2["obligation_met"]), (True, False))

    def test_regeneration_inside_the_editing_command_counts(self):
        # Found in the Sonnet 5 run: `sed … registry/ports.csv && npm run codegen && npm run check` is one
        # event classified "edit"; the regeneration after the edit must still count.
        cfg = runner.load_config(HARBOR, {})
        wd = "/w/harbor-dispatch"
        ev = lambda turn, cat, cmd: {"t": turn * 2.0, "turn": turn, "tool": "Bash", "id": str(turn), "cat": cat, "cmd": cmd}
        same = [ev(1, "edit", "sed -i '' 's/22.6/24/' registry/ports.csv && npm run codegen && npm run check 2>&1 | tail")]
        self.assertTrue(runner.regen_after_obligation(cfg, same, wd))
        before = [ev(1, "edit", "npm run codegen && sed -i '' 's/22.6/24/' registry/ports.csv")]
        self.assertFalse(runner.regen_after_obligation(cfg, before, wd), "regeneration before the edit does not count")
        reads_after = [ev(1, "edit", "sed -i '' '4a\\ Neo' registry/vessels.csv && cat registry/vessels.csv && npm run codegen; "
                                     "cat registry/ports.csv | grep x")]
        self.assertTrue(runner.regen_after_obligation(cfg, reads_after, wd), "reading the file after codegen is not an edit")
        sed_append = [ev(1, "edit", "sed -i '' '4a\\\n Neo,1,2,3,4' registry/vessels.csv && npm run codegen 2>&1 | tail -5")]
        self.assertTrue(runner.regen_after_obligation(cfg, sed_append, wd), "a newline inside a quoted sed script is not a separator")
        appended = [ev(1, "edit", "echo 'Neo,1,2,3,4' >> registry/vessels.csv && npm run codegen 2>&1 | tail -5")]
        self.assertTrue(runner.regen_after_obligation(cfg, appended, wd))
        verify_only = [ev(1, "edit", "sed -i '' 's/a/b/' registry/ports.csv && node tools/codegen.mjs --verify")]
        self.assertFalse(runner.regen_after_obligation(cfg, verify_only, wd), "a --verify check is not a regeneration")

    def test_report_only_needs_no_execute_and_runs_no_agents(self):
        root = Path(self.tmp.name) / "done"
        root.mkdir()
        (root / "state.json").write_text(json.dumps({"config": "harbor-dispatch", "runs": {}, "rounds": {}}))
        env = {k: v for k, v in os.environ.items()}
        os.environ["CLAUDECODE"] = "1"  # reporting spends nothing, so it is allowed inside a session
        try:
            out = io.StringIO()
            with contextlib.redirect_stdout(out):
                self.assertEqual(runner.main([HARBOR, "report", "--root", str(root)]), 0)
            self.assertTrue((root / "summary.json").exists())
        finally:
            os.environ.clear()
            os.environ.update(env)

    def test_a_bad_config_is_refused(self):
        bad = Path(self.tmp.name) / "bad.json"
        cfg = json.loads(Path(CONFIG).read_text())
        cfg["eval"]["arms"] = ["adage", "sometimes"]
        bad.write_text(json.dumps(cfg))
        with self.assertRaises(SystemExit):
            runner.main([str(bad), "--root", str(self.root)])


if __name__ == "__main__":
    unittest.main()
