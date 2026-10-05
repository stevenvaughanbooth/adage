"""Exercise authorized file resolution against disposable ledgers; no model calls."""
import json
import tempfile
import unittest
from pathlib import Path

from evals.live.budget import Budget, execution_lock
from evals.live.resolve_cap import digest, resolve_file


class FileResolution(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.state_path = self.root / "paid-state.json"
        self.auth_path = self.root / "authorization.json"
        self.state = {"runs": {}}
        ledger = Budget(self.state, 2, .5)
        attempt, _ = ledger.reserve("probe")
        ledger.dispatch(attempt)
        ledger.settle(attempt, {})
        self.state["runs"]["probe"] = {"status": "failed"}
        self.before = json.dumps(self.state).encode()
        self.state_path.write_bytes(self.before)
        self.authorization = {
            "schema_version": 1,
            "scope": {"run": "probe", "original_cap_usd": .5,
                      "charged_usd": .5, "actual_cost_usd": None},
            "approved_question": "Charge this stopped probe's full reserved cap?",
            "user_reply": "Yes, charge the full reserved cap; keep actual cost unknown.",
            "original_paid_state_sha256": digest(self.before),
        }
        self.write_authorization()

    def write_authorization(self):
        self.auth_path.write_text(json.dumps(self.authorization))

    def test_preview_preserves_ledger_and_creates_no_backup(self):
        receipt = resolve_file(self.state_path, self.auth_path, 2, .5)
        self.assertFalse(receipt["applied"])
        self.assertEqual(receipt["admission_spent_usd"], .5)
        self.assertEqual(receipt["measured_spent_usd"], 0)
        self.assertIsNone(receipt["actual_cost_usd"])
        self.assertEqual(receipt["model_calls"], 0)
        self.assertEqual(self.state_path.read_bytes(), self.before)
        self.assertFalse((self.root / "accounting-resolutions").exists())

    def test_apply_preserves_original_and_binds_resolution_to_authorization_bytes(self):
        receipt = resolve_file(self.state_path, self.auth_path, 2, .5, apply=True)
        self.assertTrue(receipt["applied"])
        saved = json.loads(self.state_path.read_bytes())
        attempt = saved["accounting"]["attempts"][0]
        self.assertEqual(attempt["status"], "cap_charged")
        self.assertIsNone(attempt["cost_usd"])
        self.assertEqual(attempt["charged_usd"], .5)
        expected_source = str(self.auth_path.resolve()) + "#sha256=" + digest(self.auth_path.read_bytes())
        self.assertEqual(attempt["resolution"]["authorization"]["source"], expected_source)
        backup = self.root / "accounting-resolutions" / (digest(self.before) + ".before.json")
        self.assertEqual(backup.read_bytes(), self.before)
        self.assertIsNone(saved["accounting"]["blocked_reason"])
        after = self.state_path.read_bytes()
        with self.assertRaisesRegex(ValueError, "stale authorization"):
            resolve_file(self.state_path, self.auth_path, 2, .5, apply=True)
        self.assertEqual(self.state_path.read_bytes(), after)

    def test_stale_authorization_and_live_run_are_rejected(self):
        self.state_path.write_bytes(self.before + b"\n")
        with self.assertRaisesRegex(ValueError, "ledger changed"):
            resolve_file(self.state_path, self.auth_path, 2, .5, apply=True)
        self.state["runs"]["probe"]["status"] = "running"
        current = json.dumps(self.state).encode()
        self.state_path.write_bytes(current)
        self.authorization["original_paid_state_sha256"] = digest(current)
        self.write_authorization()
        with self.assertRaisesRegex(ValueError, "not terminal"):
            resolve_file(self.state_path, self.auth_path, 2, .5, apply=True)
        self.assertEqual(self.state_path.read_bytes(), current)

    def test_active_runner_lock_prevents_resolution(self):
        with execution_lock(self.root):
            with self.assertRaisesRegex(SystemExit, "another runner owns"):
                resolve_file(self.state_path, self.auth_path, 2, .5, apply=True)
        self.assertEqual(self.state_path.read_bytes(), self.before)

    def test_smaller_charge_and_missing_human_reply_are_rejected(self):
        self.authorization["scope"]["charged_usd"] = .1
        self.write_authorization()
        with self.assertRaises(ValueError):
            resolve_file(self.state_path, self.auth_path, 2, .5, apply=True)
        self.authorization["scope"]["charged_usd"] = .5
        self.authorization["user_reply"] = ""
        self.write_authorization()
        with self.assertRaises(ValueError):
            resolve_file(self.state_path, self.auth_path, 2, .5, apply=True)
        self.assertEqual(self.state_path.read_bytes(), self.before)


if __name__ == "__main__":
    unittest.main()
