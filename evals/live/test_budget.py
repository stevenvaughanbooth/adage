"""No model calls: exercise admission, persistence and terminal-cost failures."""
import copy
import threading
import time
import unittest
import subprocess
import sys
import tempfile
from pathlib import Path
from concurrent.futures import ThreadPoolExecutor

from evals.live.budget import Budget, known_cost, execution_lock
from evals.live import runner


class Accounting(unittest.TestCase):
    def unknown(self, total=2, cap=.5, persist=lambda:None):
        state = {"runs":{}}
        ledger = Budget(state,total,cap,persist=persist)
        attempt,_ = ledger.reserve("startup-censored")
        ledger.dispatch(attempt)
        ledger.settle(attempt,{})
        return state,ledger,attempt

    def authorization(self, cap=.5, run="startup-censored"):
        return dict(kind="human_cap_charge",approved=True,run=run,cap_usd=cap,
                    statement="Approve charging the full reserved cap; actual cost remains unknown.",
                    source="explicit human reply in evaluation task")

    def test_authorized_cap_charge_is_distinct_from_measured_cost_and_survives_restart(self):
        state,ledger,attempt = self.unknown(total=1)
        receipt = ledger.resolve_unknown_at_cap("startup-censored",self.authorization())
        self.assertEqual(receipt["status"],"cap_charged")
        self.assertIsNone(receipt["cost_usd"])
        self.assertEqual(receipt["charged_usd"],.5)
        self.assertEqual(receipt["resolution"]["authorization"],self.authorization())
        self.assertEqual(ledger.measured_spent,0)
        self.assertEqual(ledger.conservative_charges,.5)
        restarted = Budget(copy.deepcopy(state),1,.5)
        self.assertEqual(restarted.spent,.5)
        next_attempt,reason = restarted.reserve("next")
        self.assertIsNone(reason)
        restarted.dispatch(next_attempt)
        restarted.settle(next_attempt,{"total_cost_usd":.1})
        self.assertAlmostEqual(restarted.spent,.6)
        self.assertIsNone(restarted.reserve("cannot-fit-full-cap")[0])
        ledger.settle(attempt,{"total_cost_usd":0})
        self.assertIsNone(attempt["cost_usd"],"ordinary settle cannot rewrite conservative resolution as free")

    def test_resolution_cannot_choose_smaller_charge_missing_human_record_or_known_attempt(self):
        mutations = [lambda a:a.update(approved=False),lambda a:a.update(cap_usd=.1),lambda a:a.update(cap_usd=True),
                     lambda a:a.update(run="another"),lambda a:a.update(statement=""),lambda a:a.update(source=""),
                     lambda a:a.update(kind="automatic")]
        for mutate in mutations:
            state,ledger,_ = self.unknown()
            before = copy.deepcopy(state)
            authorization = self.authorization()
            mutate(authorization)
            with self.assertRaises(ValueError):
                ledger.resolve_unknown_at_cap("startup-censored",authorization)
            self.assertEqual(state,before)
        _,ledger,_ = self.unknown()
        ledger.resolve_unknown_at_cap("startup-censored",self.authorization())
        with self.assertRaises(ValueError):
            ledger.resolve_unknown_at_cap("startup-censored",self.authorization())
        for status in ("reserved","dispatched","settled","cancelled"):
            _,ledger,attempt = self.unknown()
            attempt["status"] = status
            with self.assertRaises(ValueError):
                ledger.resolve_unknown_at_cap("startup-censored",self.authorization())

    def test_other_unknown_and_fatal_blocks_are_not_cleared(self):
        state,ledger,_ = self.unknown()
        state["accounting"]["attempts"].append(dict(run="other",status="unknown",cost_usd=None,cap_usd=.5))
        ledger.resolve_unknown_at_cap("startup-censored",self.authorization())
        self.assertIsNone(ledger.reserve("blocked")[0])
        for reason in ("budget accounting could not be persisted; dispatch is blocked",
                       "a reported cost exceeded its reserved cap; stop and reconcile","custom operator block"):
            state,ledger,_ = self.unknown()
            state["accounting"]["blocked_reason"] = reason
            restarted = Budget(state,2,.5)
            restarted.resolve_unknown_at_cap("startup-censored",self.authorization())
            self.assertEqual(restarted.data["blocked_reason"],reason)
            self.assertIsNone(restarted.reserve("blocked")[0])

    def test_failed_resolution_persistence_restores_unknown_and_keeps_dispatch_blocked(self):
        state,ledger,attempt = self.unknown()
        def fail():
            raise OSError("disk full")
        ledger.persist = fail
        with self.assertRaises(OSError):
            ledger.resolve_unknown_at_cap("startup-censored",self.authorization())
        self.assertEqual(attempt["status"],"unknown")
        self.assertNotIn("resolution",attempt)
        self.assertIn("could not be persisted",ledger.data["blocked_reason"])
        self.assertIsNone(ledger.reserve("blocked")[0])

    def test_invalid_or_tampered_cap_charge_and_total_overrun_block_restart(self):
        mutations = [lambda a:a.update(charged_usd=.1),lambda a:a.update(cost_usd=0),
                     lambda a:a["resolution"].pop("authorization"),lambda a:a.update(cap_usd=None)]
        for mutate in mutations:
            state,ledger,attempt = self.unknown()
            ledger.resolve_unknown_at_cap("startup-censored",self.authorization())
            mutate(attempt)
            restarted=Budget(copy.deepcopy(state),2,.5)
            self.assertIsNone(restarted.reserve("blocked")[0])
        state,ledger,_=self.unknown()
        ledger.resolve_unknown_at_cap("startup-censored",self.authorization())
        restarted=Budget(copy.deepcopy(state),.4,.1)
        self.assertIsNone(restarted.reserve("blocked")[0])

    def test_legacy_missing_cap_and_ambiguous_run_cannot_be_resolved(self):
        state,ledger,attempt=self.unknown()
        attempt.pop("cap_usd")
        with self.assertRaises(ValueError):
            ledger.resolve_unknown_at_cap("startup-censored",self.authorization())
        state,ledger,attempt=self.unknown()
        ledger.data["attempts"].append(copy.deepcopy(attempt))
        with self.assertRaises(ValueError):
            ledger.resolve_unknown_at_cap("startup-censored",self.authorization())

    def test_parallel_workers_reserve_full_caps_and_release_unused_cost(self):
        state = {"runs": {}}
        ledger = Budget(state, total=1, per_session=0.6)
        first, _ = ledger.reserve("first")
        ledger.dispatch(first)
        started = threading.Event()

        def second():
            started.set()
            return ledger.reserve("second")

        with ThreadPoolExecutor(max_workers=1) as pool:
            waiting = pool.submit(second)
            self.assertTrue(started.wait(1))
            time.sleep(0.02)
            self.assertFalse(waiting.done(), "cannot spend the first worker's reservation")
            ledger.settle(first, {"total_cost_usd": 0.2})
            other, reason = waiting.result(timeout=1)
        self.assertIsNone(reason)
        self.assertEqual(other["cap_usd"], 0.6)
        ledger.dispatch(other)
        ledger.settle(other, {"total_cost_usd": 0.3})
        self.assertEqual(ledger.spent, 0.5)
        self.assertIsNone(ledger.reserve("third")[0], "cannot reserve $0.60 with only $0.50 left")

    def test_unknown_cost_blocks_waiting_and_already_prepared_workers(self):
        ledger = Budget({"runs": {}}, 2, 1)
        first, _ = ledger.reserve("first")
        second, _ = ledger.reserve("second")
        ledger.dispatch(first)
        ledger.settle(first, {})
        self.assertFalse(ledger.dispatch(second))
        ledger.settle(second)
        self.assertEqual(second["status"], "cancelled")
        self.assertEqual(first["status"], "unknown")
        self.assertIsNone(first["cost_usd"])
        self.assertIsNone(ledger.reserve("third")[0])

    def test_zero_is_known_but_missing_and_invalid_values_are_not(self):
        for value in [None, True, "0.1", -1, float("nan"), float("inf")]:
            self.assertIsNone(known_cost(value))
        self.assertEqual(known_cost(0), 0.0)
        ledger = Budget({"runs": {}}, 1, 1)
        first, _ = ledger.reserve("zero")
        ledger.dispatch(first)
        ledger.settle(first, {"total_cost_usd": 0})
        self.assertIsNotNone(ledger.reserve("next")[0])

    def test_restarted_or_repeated_runs_preserve_every_attempt(self):
        state = {"runs": {}}
        ledger = Budget(state, 1, 0.5)
        first, _ = ledger.reserve("same-id")
        ledger.dispatch(first)
        ledger.settle(first, {"total_cost_usd": 0.4})
        restarted = Budget(copy.deepcopy(state), 1, 0.5)
        again, _ = restarted.reserve("same-id")
        restarted.dispatch(again)
        restarted.settle(again, {"total_cost_usd": 0.4})
        self.assertEqual(len(restarted.data["attempts"]), 2)
        self.assertAlmostEqual(restarted.spent, 0.8)
        self.assertIsNone(restarted.reserve("next")[0])

    def test_crash_after_reservation_or_dispatch_blocks_restart(self):
        for dispatch in (False, True):
            state = {"runs": {}}
            ledger = Budget(state, 1, 1)
            attempt, _ = ledger.reserve("crashed")
            if dispatch:
                ledger.dispatch(attempt)
            restarted = Budget(copy.deepcopy(state), 1, 1)
            self.assertIsNone(restarted.reserve("next")[0])

    def test_preparation_failure_is_free_but_cost_overrun_stops(self):
        ledger = Budget({"runs": {}}, 3, 1)
        attempt, _ = ledger.reserve("preparation-failed")
        ledger.settle(attempt)
        self.assertEqual(attempt["status"], "cancelled")
        attempt, _ = ledger.reserve("overrun")
        ledger.dispatch(attempt)
        ledger.settle(attempt, {"total_cost_usd": 1.1})
        self.assertIsNone(ledger.reserve("next")[0])

    def test_legacy_zero_or_missing_cost_cannot_be_treated_as_free(self):
        for cost in (None, 0):
            ledger = Budget({"runs": {"old": {"status": "failed", "metrics": {"cost_usd": cost}}}}, 3, 1)
            self.assertIsNone(ledger.reserve("next")[0])

    def test_persists_before_dispatch_and_passes_cap_to_claude(self):
        state, snapshots = {"runs": {}}, []
        ledger = Budget(state, 1, 1, persist=lambda: snapshots.append(copy.deepcopy(state)))
        attempt, _ = ledger.reserve("one")
        ledger.dispatch(attempt)
        self.assertEqual([s["accounting"]["attempts"][0]["status"] for s in snapshots], ["reserved", "dispatched"])
        cmd = runner.claude_command(runner.DEFAULTS, "prompt", "/tmp/settings.json")
        self.assertEqual(cmd[cmd.index("--max-budget-usd") + 1], "1.0")

    def test_failed_persistence_blocks_and_wakes_waiting_workers(self):
        fail = []

        def persist():
            if fail:
                raise OSError("disk full")

        ledger = Budget({"runs": {}}, 1, 1, persist=persist)
        first, _ = ledger.reserve("first")
        ledger.dispatch(first)
        with ThreadPoolExecutor(max_workers=1) as pool:
            waiting = pool.submit(ledger.reserve, "second")
            time.sleep(0.02)
            self.assertFalse(waiting.done())
            fail.append(True)
            with self.assertRaises(OSError):
                ledger.settle(first, {"total_cost_usd": 0.1})
            self.assertIsNone(waiting.result(timeout=1)[0])
        ledger = Budget({"runs": {}}, 1, 1, persist=persist)
        with self.assertRaises(OSError):
            ledger.reserve("never-dispatched")
        self.assertIsNone(ledger.reserve("next")[0])

    def test_second_process_is_refused_then_lock_releases(self):
        with tempfile.TemporaryDirectory() as directory:
            code = ("from pathlib import Path; from evals.live.budget import execution_lock; "
                    "lock=execution_lock(Path(" + repr(directory) + ")); lock.__enter__()")
            with execution_lock(Path(directory)):
                other = subprocess.run([sys.executable, "-B", "-c", code], capture_output=True, text=True)
                self.assertNotEqual(other.returncode, 0)
                self.assertIn("another runner owns", other.stderr)
            other = subprocess.run([sys.executable, "-B", "-c", code], capture_output=True, text=True)
            self.assertEqual(other.returncode, 0, other.stderr)

    def test_real_state_file_retains_a_reservation_across_restart(self):
        import json
        with tempfile.TemporaryDirectory() as directory:
            live = runner.Live.__new__(runner.Live)
            live.root, live.state = Path(directory), {"runs": {}}
            ledger = Budget(live.state, 1, 1, persist=live.save)
            attempt, _ = ledger.reserve("crash-window")
            self.assertEqual(json.loads((live.root / "state.json").read_text())["accounting"]["attempts"][0]["status"],
                             "reserved")
            ledger.dispatch(attempt)
            disk = json.loads((live.root / "state.json").read_text())
            restarted = Budget(disk, 1, 1)
            self.assertIsNone(restarted.reserve("next")[0])


if __name__ == "__main__":
    unittest.main()
