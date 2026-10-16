#!/usr/bin/env python3
"""No-model checks of pilot splits, all task outcomes, gold repairs and tamper resistance."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from cases import manifest, task
from generate import generate
from grade import grade
from project import equal
from safe_python import evaluate, validate

HERE = Path(__file__).resolve().parent


def command(root, *argv):
    env = dict(os.environ, PYTHONDONTWRITEBYTECODE="1")
    env.pop("PYTHONPATH", None)
    return subprocess.run(argv, cwd=root, env=env, capture_output=True, text=True, timeout=15)


def solve(root, spec):
    if spec["kind"] == "documentation":
        (root / "docs/guide.md").write_text(spec["doc_after"], encoding="utf-8")
    else:
        (root / "src/solution.py").write_text(spec["reference"], encoding="utf-8")
        result = command(root, "make", "generate")
        if result.returncode:
            raise AssertionError(result.stderr)


class PilotFixtureTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="adage-pilot-check-")
        self.root = Path(self.temp.name)
        self.counter = 0

    def tearDown(self):
        self.temp.cleanup()

    def fresh(self, tid):
        self.counter += 1
        return generate(self.root / str(self.counter), tid)

    def test_registry_has_truthful_disjoint_issue_groups(self):
        registry = manifest()
        self.assertEqual(registry["schema"], 1)
        self.assertEqual(registry["repo"], "github.com/adage-evals/pilot")
        self.assertEqual(len(registry["tasks"]), 30)
        groups = {}
        counts = {}
        for tid, entry in registry["tasks"].items():
            group = groups.setdefault(entry["group_id"], {"split": entry["split"], "variants": set()})
            self.assertEqual(group["split"], entry["split"])
            group["variants"].add(entry["variant"])
            counts[entry["split"]] = counts.get(entry["split"], 0) + 1
            self.assertEqual(task(tid)["group_id"], entry["group_id"])
        self.assertEqual(len(groups), 15)
        self.assertEqual(counts, {"train": 20, "validation": 4, "test": 6})
        self.assertTrue(all(group["variants"] == {1, 2} for group in groups.values()))

    def test_every_untouched_issue_fails_and_every_reference_repair_passes(self):
        for tid in manifest()["tasks"]:
            with self.subTest(task=tid):
                spec = task(tid)
                root = self.fresh(tid)
                before = grade(root, tid)
                self.assertFalse(before["passed"], before)
                if spec["kind"] == "code":
                    self.assertNotEqual(command(root, "make", "check").returncode, 0)
                else:
                    self.assertEqual(command(root, "make", "check").returncode, 0)
                solve(root, spec)
                result = grade(root, tid)
                self.assertTrue(result["passed"], result)
                self.assertFalse(result["verification_execution_observed"])
                check = command(root, "make", "check")
                self.assertEqual(check.returncode, 0, check.stderr)

    def test_correct_code_without_companion_artifact_fails_all_code_groups(self):
        for tid in manifest()["tasks"]:
            spec = task(tid)
            if spec["kind"] != "code" or spec["variant"] != 1:
                continue
            with self.subTest(task=tid):
                root = self.fresh(tid)
                (root / "src/solution.py").write_text(spec["reference"])
                result = grade(root, tid)
                self.assertTrue(result["hidden_passed"], result)
                self.assertFalse(result["generated_correct"])
                self.assertFalse(result["passed"])
                self.assertNotEqual(command(root, "make", "check").returncode, 0)

    def test_tampered_visible_checks_cannot_spoof_pass(self):
        tid = "train-stable-dedupe-01"
        root = self.fresh(tid)
        (root / "tools/project.py").write_text("print('all tests passed')\n")
        self.assertEqual(command(root, "make", "check").returncode, 0)
        result = grade(root, tid)
        self.assertFalse(result["passed"])
        self.assertFalse(result["check_integrity_passed"])
        # A fully correct implementation still does not excuse rewritten checks.
        spec = task(tid)
        (root / "src/solution.py").write_text(spec["reference"])
        from project import artifact
        expected = artifact(spec["reference"], spec["visible"], [x["expected"] for x in spec["visible"]])
        (root / "generated/client-examples.json").write_text(json.dumps(expected))
        result = grade(root, tid)
        self.assertTrue(result["hidden_passed"])
        self.assertTrue(result["generated_correct"])
        self.assertFalse(result["passed"])

    def test_hidden_cases_reject_example_specific_constant(self):
        tid = "train-clamp-range-01"
        root = self.fresh(tid)
        (root / "src/solution.py").write_text("def solve(value):\n    return 0\n")
        command(root, "make", "generate")
        result = grade(root, tid)
        self.assertFalse(result["hidden_passed"])
        self.assertGreater(result["behavior_failures"], 0)

    def test_documentation_controls_reject_source_churn(self):
        tid = "test-docs-support-01"
        root = self.fresh(tid)
        solve(root, task(tid))
        self.assertTrue(grade(root, tid)["passed"])
        (root / "src/solution.py").write_text("def solve(value):\n    return value\n\n")
        command(root, "make", "generate")
        result = grade(root, tid)
        self.assertFalse(result["passed"])
        self.assertIn("src/solution.py", result["changed_protected_files"])

    def test_generated_content_and_source_digest_are_both_graded(self):
        tid = "test-discounted-total-01"
        root = self.fresh(tid)
        solve(root, task(tid))
        path = root / "generated/client-examples.json"
        original = json.loads(path.read_text())
        for key in ("source_sha256", "examples"):
            corrupted = dict(original)
            corrupted[key] = "bad" if key == "source_sha256" else []
            path.write_text(json.dumps(corrupted))
            self.assertFalse(grade(root, tid)["passed"])

    def test_symlinks_missing_oversized_files_fail_closed(self):
        tid = "test-merge-intervals-01"
        root = self.fresh(tid)
        path = root / "src/solution.py"
        path.unlink()
        self.assertFalse(grade(root, tid)["passed"])
        path.symlink_to(self.root / "outside")
        self.assertFalse(grade(root, tid)["passed"])
        path.unlink()
        path.write_text("x" * (128 * 1024 + 1))
        self.assertFalse(grade(root, tid)["passed"])

    def test_candidate_execution_is_bounded_and_has_no_io_or_reflection(self):
        for source in (
            "import os\ndef solve(value):\n    return value\n",
            "def solve(value):\n    return open('/etc/passwd').read()\n",
            "def solve(value):\n    return value.__class__\n",
            "def solve(value):\n    return eval(value)\n",
            "def solve(value):\n    return 2 ** value\n",
            "def solve(value):\n    return (lambda: value)()\n",
        ):
            with self.subTest(source=source), self.assertRaises((ValueError, SyntaxError)):
                validate(source)
        with self.assertRaises((ValueError, subprocess.TimeoutExpired)):
            evaluate("def solve(value):\n    while True:\n        pass\n", [1])
        self.assertEqual(evaluate("def solve(value):\n    return value\n", [{"hello": [1, 2]}]), [{"hello": [1, 2]}])

    def test_generation_is_reproducible_clean_and_never_overwrites(self):
        a = self.fresh("train-csv-row-01")
        b = self.fresh("train-csv-row-01")
        self.assertEqual(command(a, "git", "rev-parse", "HEAD").stdout,
                         command(b, "git", "rev-parse", "HEAD").stdout)
        self.assertEqual(command(a, "git", "status", "--porcelain").stdout, "")
        self.assertEqual(command(a, "git", "remote", "get-url", "origin").stdout.strip(),
                         "https://github.com/adage-evals/pilot")
        with self.assertRaises(ValueError):
            generate(a, "train-csv-row-01")
        with self.assertRaises(ValueError):
            generate(self.root / "unknown", "not-a-task")
        self.assertFalse((self.root / "unknown").exists())

    def test_cli_contract_and_strict_boolean_numeric_comparison(self):
        listed = command(HERE, "python3", str(HERE / "generate.py"), "--list")
        self.assertEqual(json.loads(listed.stdout), manifest())
        work = self.root / "cli"
        generated = command(HERE, "python3", str(HERE / "generate.py"), "--out", str(work),
                            "--task", "validation-chunk-list-01")
        self.assertEqual(generated.returncode, 0, generated.stderr)
        scored = command(HERE, "python3", str(HERE / "grade.py"), "--workdir", str(work),
                         "--task", "validation-chunk-list-01")
        self.assertEqual(scored.returncode, 0, scored.stderr)
        self.assertFalse(json.loads(scored.stdout)["passed"])
        self.assertFalse(equal(True, 1))
        self.assertFalse(equal(1, True))
        self.assertTrue(equal(5.0, 5))


if __name__ == "__main__":
    unittest.main(verbosity=2)
