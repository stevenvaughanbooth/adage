import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

import probe


class FixtureTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="reflex-offline-test-")
        self.root = Path(self.temp.name)
        (self.root / ".reflex-fixture").write_text("test")
        self.payload = {"hook_event_name": "UserPromptSubmit", "session_id": "offline", "prompt": "fixture"}

    def tearDown(self):
        self.temp.cleanup()

    def start(self, delay=0):
        child = subprocess.Popen([sys.executable, "-B", str(probe.HERE / "fixture_hook.py"), str(self.root), str(delay)],
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        child.stdin.write(json.dumps(self.payload))
        child.stdin.close()
        child.stdin = None
        return child

    def events(self):
        path = self.root / "events.jsonl"
        return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []

    def test_real_subprocess_order_and_receipt(self):
        child = self.start()
        out, err = child.communicate(timeout=5)
        self.assertEqual(child.returncode, 0, err)
        self.assertIn("REFLEX_FIXTURE_OK", out)
        events = self.events()
        times = {item["kind"]: item["monotonic_ns"] for item in events}
        for left, right in zip(["hook_enter", "script_start", "script_end", "receipt"],
                               ["script_start", "script_end", "receipt", "hook_exit"]):
            self.assertLess(times[left], times[right])
        self.assertEqual((self.root / "effects.txt").read_text(), "fixture-effect\n")

    def test_concurrent_duplicate_runs_once(self):
        a, b = self.start(.1), self.start(.1)
        for child in (a, b):
            _out, err = child.communicate(timeout=5)
            self.assertEqual(child.returncode, 0, err)
        self.assertEqual((self.root / "effects.txt").read_text(), "fixture-effect\n")
        self.assertEqual(sum(item["kind"] == "duplicate_suppressed" for item in self.events()), 1)

    def test_sigterm_kills_fixture_child_and_leaves_unknown(self):
        child = self.start(10)
        deadline = time.monotonic() + 3
        while not any(item["kind"] == "script_start" for item in self.events()):
            if time.monotonic() > deadline:
                child.kill()
                child.communicate()
                self.fail("fixture child did not start")
            time.sleep(.01)
        child.terminate()
        child.communicate(timeout=3)
        self.assertFalse((self.root / "effects.txt").exists())
        receipt = next(item for item in self.events() if item["kind"] == "receipt")
        self.assertEqual(receipt["state"], "unknown")
        retry = self.start()
        retry.communicate(timeout=3)
        self.assertFalse((self.root / "effects.txt").exists(), "unknown action must not retry")

    def test_message_ids_are_not_request_counts(self):
        value = {"type": "assistant", "message": {"id": "msg_123"}}
        self.assertEqual(probe.explicit_request_ids(value), [])
        self.assertEqual(probe.explicit_request_ids({"request_id": "req_real", "message": value}), ["req_real"])

    def test_missing_accounting_is_unknown_not_zero(self):
        self.assertIsNone(probe.summary(self.root)["total_cost_usd"])
        (self.root / "stream-observations.jsonl").write_text(json.dumps({"value": {
            "type": "result", "subtype": "success", "total_cost_usd": .02, "num_turns": 3,
            "usage": {"input_tokens": 123}}}) + "\n")
        result = probe.summary(self.root)
        self.assertEqual(result["total_cost_usd"], .02)
        self.assertEqual(result["num_turns"], 3)
        self.assertIsNone(result["api_request_count"])

    def test_live_refuses_nested_marker_before_reading_manifest(self):
        with patch.dict(os.environ, {"CLAUDECODE": "1"}):
            with self.assertRaisesRegex(ValueError, "Nested Claude"):
                probe.execute(self.root / "missing.json", 1, 10)


if __name__ == "__main__":
    unittest.main()
