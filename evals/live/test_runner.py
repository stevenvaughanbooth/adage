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


def dry(*args):
    out = io.StringIO()
    with contextlib.redirect_stdout(out):
        assert runner.main([CONFIG, *args]) == 0
    return out.getvalue()


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

    def test_a_bad_config_is_refused(self):
        bad = Path(self.tmp.name) / "bad.json"
        cfg = json.loads(Path(CONFIG).read_text())
        cfg["eval"]["arms"] = ["adage", "sometimes"]
        bad.write_text(json.dumps(cfg))
        with self.assertRaises(SystemExit):
            runner.main([str(bad), "--root", str(self.root)])


if __name__ == "__main__":
    unittest.main()
