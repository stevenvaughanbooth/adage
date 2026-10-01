"""Launch canary contracts: matrix, person-run approval script, grant checks, realized controls."""
import json
from pathlib import Path
import shlex
import tempfile
import unittest

import launch_canary as canary


def frozen(root):
    m = dict(protocol=canary.PROTOCOL, source_sha256=canary.sources(), frozen_inputs={}, matrix=canary.matrix(),
             route=canary.ROUTE, command=canary.COMMAND, caches=list(canary.CACHES),
             binaries={"adage": str(root / "bin/adage"), "adage-exec": str(root / "bin/adage-exec")})
    canary.pairs.write(root / "manifest.json", m)
    (root / "manifest.sha256").write_text(canary.pairs.sha(root / "manifest.json") + "\n")
    return m


def grant(**changes):
    g = dict(route=canary.ROUTE, by="launch-evaluation-human", auto=False,
             key=dict(level="e2", effect="verify", argv_templates=[shlex.split(canary.COMMAND)]),
             terms=dict(cache=list(canary.CACHES), timeout_ms=30000, effect="verify"))
    g.update(changes)
    return g


POLICY = '[exec]\nmax_level = "E2"\nauto_approve = "off"\nrequire_sandbox = true\n'


class Matrix(unittest.TestCase):
    def test_six_positives_and_every_control_on_development_split_tasks_only(self):
        rows = canary.matrix()
        self.assertEqual(len(rows), 12)
        table = canary.split_audit.splits(json.loads(canary.GATE.read_text()), "marshmallow")
        self.assertTrue(all(table[canary.split_audit.variant(r["task"])][0] == "development" for r in rows))
        self.assertEqual([r["order"] for r in rows], list(range(len(rows))))
        conditions = [r["condition"] for r in rows]
        self.assertEqual(conditions.count("positive"), 6)
        self.assertEqual(set(conditions), set(canary.CONDITIONS))
        self.assertEqual([r["repetition"] for r in rows if r["condition"] == "positive"], [1, 2, 3, 4, 5, 6])

    def test_every_condition_supplies_the_same_tests_and_only_negatives_change_requirements(self):
        self.assertEqual(canary.CONDITIONS["positive"][1:], (False, None))
        self.assertTrue(canary.CONDITIONS["rerun"][1])
        self.assertEqual(canary.CONDITIONS["broader"][2], [[], ["tests"]])
        self.assertTrue(all(canary.TESTS in c[0] for c in canary.CONDITIONS.values()))
        failed = canary.CONDITIONS["failed"][0]
        self.assertLess(failed.index("append the tests"), failed.index("implement the validator"))
        self.assertNotIn("Adage", "".join(c[0] for c in canary.CONDITIONS.values()))
        self.assertNotIn("pytest tests", canary.CONDITIONS["positive"][0], "no verification instruction in a positive")


class Approvals(unittest.TestCase):
    def test_script_is_scoped_to_assignment_homes_and_never_auto_approves(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            frozen(root)
            path = Path(canary.approval_script(root)["path"])
            text = path.read_text()
            self.assertEqual(text.count("review delegate"), 12)
            self.assertEqual(text.count("--level e2 --effect verify"), 12)
            self.assertIn('auto_approve = "off"', text)
            self.assertIn("require_sandbox = true", text)
            self.assertNotIn("approvals.json >", text)
            self.assertNotIn("--outputs", text)
            for home in (line.split()[0].split("=", 1)[1] for line in text.splitlines() if line.startswith("ADAGE_HOME=")):
                self.assertTrue(Path(home).is_relative_to(root / "sessions"))
            for cache in canary.CACHES:
                self.assertEqual(text.count("--cache " + shlex.quote(cache)), 12)
            self.assertEqual(canary.approval_script(root)["sha256"], canary.pairs.sha(path))  # Idempotent.

    def test_grant_check_rejects_anything_but_the_reviewed_terms(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            m = frozen(root)
            self.assertFalse(canary.verify_grants(root)["passed"])
            for row in m["matrix"]:
                home = canary.session_dir(root, row) / "adage-home"
                home.mkdir(parents=True)
                (home / "policy.toml").write_text(POLICY)
                canary.pairs.write(home / "approvals.json", dict(version=1, approvals=[grant()]))
            self.assertTrue(canary.verify_grants(root)["passed"])
            home = canary.session_dir(root, m["matrix"][3]) / "adage-home"
            for bad in (grant(auto=True), grant(terms=dict(cache=["**"], timeout_ms=30000)), grant(by="agent")):
                canary.pairs.write(home / "approvals.json", dict(version=1, approvals=[bad]))
                self.assertFalse(canary.verify_grants(root)["passed"])
            canary.pairs.write(home / "approvals.json", dict(version=1, approvals=[grant()]))
            (home / "policy.toml").write_text(POLICY.replace('"off"', '"on"'))
            self.assertFalse(canary.verify_grants(root)["passed"])

    def test_source_change_after_freeze_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            frozen(root)
            m = json.loads((root / "manifest.json").read_text())
            m["command"] = "pytest"
            (root / "manifest.json").write_text(json.dumps(m))
            with self.assertRaises(ValueError):
                canary.verify(root)


class Realized(unittest.TestCase):
    def row(self, condition, receipts, final="a"):
        return dict(condition=condition, replay={"x": 1}, timeline=dict(events=receipts, final_digest=final))

    def test_conditions_are_counted_only_when_they_occurred(self):
        ok = dict(kind="receipt", phase="completed", outcome="applied", digest="a")
        bad = dict(kind="receipt", phase="completed", outcome="failed", digest="a")
        self.assertTrue(canary.realized(self.row("failed", [bad])))
        self.assertFalse(canary.realized(self.row("failed", [ok])))
        self.assertFalse(canary.realized(self.row("failed", [])))
        self.assertTrue(canary.realized(self.row("later-edit", [ok], final="b")))
        self.assertFalse(canary.realized(self.row("later-edit", [ok], final="a")))
        self.assertTrue(canary.realized(self.row("rerun", [ok])))
        self.assertFalse(canary.realized(self.row("broader", [])))


if __name__ == "__main__":
    unittest.main()
