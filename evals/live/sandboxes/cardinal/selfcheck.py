#!/usr/bin/env python3
"""No-model adversarial checks for the cardinal diagnostic fixtures."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from cases import REFERENCE_QUOTE, catalog, dump, files, task, tasks
from generate import generate
from grade import grade, quote_correct


def command(root, *argv):
    env = dict(os.environ, PYTHONDONTWRITEBYTECODE="1")
    env.pop("PYTHONPATH", None)
    return subprocess.run(argv, cwd=root, env=env, capture_output=True, text=True, timeout=15)


def solve(root, spec):
    if spec["family"] == "manifest":
        (root / "catalog/prices.json").write_text(dump(catalog(spec, solved=True)))
        result = command(root, "make", "generate")
        if result.returncode:
            raise AssertionError(result.stderr)
    elif spec["family"] == "layout":
        (root / "src/cardinal_quote.py").write_text(REFERENCE_QUOTE)


class CardinalFixtureTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="cardinal-selfcheck-")
        self.root = Path(self.tmp.name)
        self.counter = 0

    def tearDown(self):
        self.tmp.cleanup()

    def fresh(self, tid):
        self.counter += 1
        return generate(self.root / str(self.counter), tid)

    def test_every_task_fails_untouched_or_is_unchanged_control_and_reference_passes(self):
        for spec in tasks():
            with self.subTest(task=spec["id"]):
                root = self.fresh(spec["id"])
                self.assertEqual(grade(root, spec["id"])["passed"], spec["family"] == "control")
                solve(root, spec)
                result = grade(root, spec["id"])
                self.assertTrue(result["passed"], result)
                self.assertFalse(result["verification_execution_observed"])
                if spec["family"] != "control":
                    checks = command(root, "make", "check")
                    self.assertEqual(checks.returncode, 0, checks.stderr)

    def test_split_manifest_honestly_groups_template_siblings(self):
        manifest = tasks()
        self.assertEqual(len(manifest), 42)
        self.assertEqual(len({s["id"] for s in manifest}), 42)
        self.assertEqual({s["split"] for s in manifest}, {"diagnostic"})
        for family in ("manifest", "layout", "control"):
            members = [s for s in manifest if s["family"] == family]
            self.assertEqual(len({s["semantic_group"] for s in members}), 1)
            self.assertEqual({s["pool"] for s in members}, {"train", "dev", "test"})

    def test_generator_deterministic_and_clean_real_git_checkout(self):
        first = self.fresh("train-manifest-01")
        second = self.fresh("train-manifest-01")
        for name in files(task("train-manifest-01")):
            self.assertEqual((first / name).read_bytes(), (second / name).read_bytes())
        self.assertEqual(command(first, "git", "rev-parse", "HEAD").stdout,
                         command(second, "git", "rev-parse", "HEAD").stdout)
        self.assertEqual(command(first, "git", "status", "--porcelain").stdout, "")
        self.assertEqual(command(first, "git", "remote", "get-url", "origin").stdout.strip(),
                         "https://github.com/adage-evals/cardinal")
        with self.assertRaises(ValueError):
            generate(first, "train-manifest-01")

    def test_source_only_change_rejected_and_real_check_fails(self):
        tid = "test-manifest-01"
        root = self.fresh(tid)
        (root / "catalog/prices.json").write_text(dump(catalog(task(tid), solved=True)))
        result = grade(root, tid)
        self.assertTrue(result["source_correct"])
        self.assertTrue(result["stale_generated"])
        self.assertFalse(result["passed"])
        self.assertNotEqual(command(root, "make", "check").returncode, 0)

    def test_wrong_companion_and_tampered_checks_do_not_spoof_pass(self):
        tid = "dev-manifest-01"
        root = self.fresh(tid)
        (root / "tools/catalog.py").write_text("print('all checks passed')\n")
        (root / "Makefile").write_text("check:\n\t@true\n")
        self.assertEqual(command(root, "make", "check").returncode, 0)
        result = grade(root, tid)
        self.assertFalse(result["passed"])
        self.assertFalse(result["check_integrity_passed"])
        # Even correct task state cannot disguise weakened checks.
        spec = task(tid)
        (root / "catalog/prices.json").write_text(dump(catalog(spec, solved=True)))
        from cases import generated
        (root / "src/generated_catalog.py").write_text(generated(catalog(spec, solved=True)))
        self.assertFalse(grade(root, tid)["passed"])

    def test_layout_needs_project_command_and_hidden_behavior(self):
        tid = "dev-layout-01"
        root = self.fresh(tid)
        solve(root, task(tid))
        wrong = command(root, "python3", "-m", "unittest", "discover", "-s", "tests")
        self.assertNotEqual(wrong.returncode, 0)
        self.assertIn("No module named 'cardinal_quote'", wrong.stderr)
        self.assertEqual(command(root, "python3", "tools/check.py").returncode, 0)
        (root / "src/cardinal_quote.py").write_text("def quote(unit_cents, quantity):\n    return 3897\n")
        (root / "tests/test_quote.py").write_text("import unittest\nclass Fake(unittest.TestCase):\n    def test_always_pass(self):\n        self.assertTrue(True)\n")
        self.assertEqual(command(root, "python3", "tools/check.py").returncode, 0)
        self.assertFalse(grade(root, tid)["passed"])

    def test_control_rejects_unnecessary_edits_and_files(self):
        tid = "test-control-01"
        root = self.fresh(tid)
        self.assertTrue(grade(root, tid)["passed"])
        (root / "unrequested.txt").write_text("gratuitous change")
        self.assertFalse(grade(root, tid)["passed"])
        (root / "unrequested.txt").unlink()
        (root / "README.md").write_text("changed")
        self.assertFalse(grade(root, tid)["passed"])

    def test_grader_rejects_symlinks_oversize_and_arbitrary_python(self):
        tid = "test-layout-02"
        root = self.fresh(tid)
        source = root / "src/cardinal_quote.py"
        source.unlink()
        source.symlink_to(self.root / "nonexistent-secret")
        self.assertFalse(grade(root, tid)["passed"])
        source.unlink()
        source.write_text("x" * (128 * 1024 + 1))
        self.assertFalse(grade(root, tid)["passed"])
        for malicious in ("import os\n" + REFERENCE_QUOTE,
                          "def quote(unit_cents, quantity):\n    return __import__('os').system('echo bad')\n",
                          "def quote(unit_cents, quantity):\n    return unit_cents ** quantity\n"):
            self.assertFalse(quote_correct(malicious))
        self.assertTrue(quote_correct(REFERENCE_QUOTE))

    def test_cli_json_protocol(self):
        here = Path(__file__).resolve().parent
        manifest = command(here, "python3", str(here / "generate.py"), "--list")
        self.assertEqual(len(json.loads(manifest.stdout)), 42)
        root = self.root / "cli"
        generated = command(here, "python3", str(here / "generate.py"), "--out", str(root),
                            "--task", "test-manifest-01")
        self.assertEqual(generated.returncode, 0, generated.stderr)
        scored = command(here, "python3", str(here / "grade.py"), "--workdir", str(root),
                         "--task", "test-manifest-01")
        self.assertEqual(scored.returncode, 0)
        self.assertFalse(json.loads(scored.stdout)["passed"])


if __name__ == "__main__":
    unittest.main(verbosity=2)
