import copy
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import curve_heldout as heldout
import real_repo_pairs as pairs
from test_real_repo_pairs import passing_rows


def four_rows():
    learned, automatic = passing_rows()
    learned["arm"] = "learned"
    rows = []
    for arm in ("agent", "instincts"):
        row = copy.deepcopy(learned)
        row.update(arm=arm, learned_files={}, execution_attempts=[], collection_errors=[])
        rows.append(row)
    rows.extend([learned, automatic])
    return rows


class CurveHeldoutTests(unittest.TestCase):
    def test_literal_pair_success_remains_unconfirmed_without_semantic_proof(self):
        result = heldout.task_result(four_rows())
        self.assertTrue(result["literal_pair_evidence"]["verification_action_suppressed"])
        self.assertFalse(result["confirmed_elimination"])
        self.assertFalse(result["passed"])
        self.assertIn("unverified", result["reasons"][-1])
        self.assertEqual(set(result["contrasts"]), {
            "instincts-agent", "learned-instincts", "automatic-learned", "automatic-agent"})

    def test_fixed_test_file_on_final_generation_can_confirm_lower_bound(self):
        rows = four_rows()
        for row in rows:
            row["template"] = ".venv/bin/python -m pytest tests/test_validate.py -q -x"
        learned = rows[2]
        action = dict(id="test", name="Bash", input={"command": learned["template"]})
        learned["successful_matching_agent_actions"] = [action]
        learned["matching_agent_actions"] = [action]
        learned["agent_actions"][-1] = action
        learned["bash_action_audit"] = pairs.bash_audit.audit_bash_actions(learned["agent_actions"])
        learned.update(final_generation="final", provenance={"session_id": "native"}, hooks=[
            dict(event="PostToolUse", exit_code=0, payload={"session_id": "native", "tool_use_id": "test"},
                 before={"digest": "final"}, after={"digest": "final"})])
        self.assertTrue(heldout.task_result(rows)["confirmed_elimination"])
        transfer = heldout.task_result(rows[2:], ("learned", "automatic"))
        self.assertTrue(transfer["confirmed_elimination"])
        self.assertEqual(set(transfer["contrasts"]), {"automatic-learned"})
        self.assertFalse(heldout.task_result(rows[2:])["passed"], "four-arm default must remain strict")
        learned["hooks"][0]["after"]["digest"] = "stale"
        self.assertFalse(heldout.task_result(rows)["confirmed_elimination"])
        self.assertFalse(heldout.task_result(rows[2:], ("learned", "automatic"))["confirmed_elimination"])
        for command in ("pytest -q", ".venv/bin/python -m pytest -k one", ".venv/bin/python -m pytest {tests}"):
            learned["template"] = command
            self.assertFalse(heldout.fixed_pytest_evidence(learned))

    def test_fixed_pytest_scope_and_flags_are_strictly_bounded(self):
        row = dict(final_generation="final", provenance={"session_id": "native"}, hooks=[
            dict(event="PostToolUse", exit_code=0, payload={"session_id": "native", "tool_use_id": "test"},
                 before={"digest": "final"}, after={"digest": "final"})])
        def admitted(suffix):
            row["template"] = ".venv/bin/python -m pytest " + suffix
            row["successful_matching_agent_actions"] = [dict(id="test", name="Bash", input={"command": row["template"]})]
            return heldout.fixed_pytest_evidence(row)
        for suffix in ("-q", "tests/test_validate.py -x", "tests/ -v -vv -s --disable-warnings",
                       "tests/test_validate.py -p no:cacheprovider -q", "tests/test_a.py tests/test_b.py"):
            self.assertTrue(admitted(suffix), suffix)
        for suffix in ("../tests/test_a.py", "tests/../test_a.py", "/tmp/test_a.py", "-k one", "-m slow",
                       "tests/test_a.py::test_one", "tests/{name}.py", "tests/*.py", "-p", "-p arbitrary",
                       "-pno:cacheprovider", "--collect-only", "--override-ini x", "-- tests", "-qx",
                       "tests/test_a.py; true", "$(echo tests)", "tests/test_a.py > result"):
            self.assertFalse(admitted(suffix), suffix)
        admitted("tests/test_validate.py -x")
        row["successful_matching_agent_actions"][0]["input"]["command"] = ".venv/bin/python -m pytest tests/test_other.py -x"
        self.assertFalse(heldout.fixed_pytest_evidence(row))

    def test_unknown_or_foreign_execution_cannot_pass_approval_evidence(self):
        row = four_rows()[-1]
        row["all_journal_receipts"] = [dict(phase="prepared", approved_on="grant", route=row["route"])]
        self.assertFalse(heldout.approved_execution_evidence(row))

    def test_unknown_bash_is_not_absence_and_all_cpu_work_is_retained(self):
        rows = four_rows()
        automatic = rows[-1]
        automatic["agent_actions"].append(dict(id="unknown", name="Bash", input={"command": "./check.sh"}))
        automatic["agent_action_count"] += 1
        automatic["bash_action_audit"] = pairs.bash_audit.audit_bash_actions(automatic["agent_actions"])
        result = heldout.task_result(rows)
        self.assertFalse(result["literal_pair_evidence"]["verification_action_suppressed"])
        self.assertEqual(result["literal_pair_evidence"]["automatic_bash_blockers"][0]["tool_id"], "unknown")

    def test_quality_loss_and_missing_arm_are_explicit(self):
        rows = four_rows()
        rows[-1]["quality"]["passed"] = False
        result = heldout.task_result(rows)
        self.assertTrue(result["automatic_only_quality_failure"])
        self.assertFalse(heldout.task_result(rows[:3])["passed"])
        rows[0]["execution_attempts"] = [{"route": "unexpected"}]
        self.assertIn("agent: unexpected CPU execution", heldout.task_result(rows)["reasons"])

    def test_unknown_cost_or_requests_never_become_zero_savings(self):
        a, b = four_rows()[:2]
        a["cost_usd"] = None
        b["observed_dispatches"] = []
        result = heldout.contrast(a, b)
        self.assertIsNone(result["cost_usd_saved"])
        self.assertIsNone(result["observed_model_requests_saved"])

    def test_agent_hook_is_forbidden_without_invoking_production(self):
        with patch.object(pairs, "read", return_value={"arm": "agent"}), patch.object(pairs, "hook") as hook:
            with self.assertRaisesRegex(ValueError, "must never"):
                heldout.hook(Path("manifest.json"), {})
            hook.assert_not_called()

    def test_non_cpu_arms_use_unmodified_production_hook(self):
        with patch.object(pairs, "read", return_value={"arm": "learned"}), patch.object(pairs, "hook", return_value="") as hook:
            self.assertEqual(heldout.hook(Path("manifest.json"), {"event": "Stop"}), "")
            hook.assert_called_once()

    def test_grants_and_runtime_state_cannot_be_artifacts(self):
        with tempfile.TemporaryDirectory() as tmp:
            home = Path(tmp)
            for name in ("approvals.json", "sessions/one.spool", "exec/actions.spool"):
                path = home / name
                path.parent.mkdir(exist_ok=True)
                path.write_text("{}")
                with self.assertRaisesRegex(ValueError, "never copied"):
                    heldout.artifact_files(home, [name])

    def test_report_preserves_sixteen_assignments_and_billed_collection_failure(self):
        with tempfile.TemporaryDirectory() as tmp:
            authority = Path(tmp)
            root = authority / "cohorts/curve"
            root.mkdir(parents=True)
            tasks = ["t0", "t1", "t2", "t3"]
            matrix = [dict(task=t, arm=arm, order=i * 4 + j)
                      for i, t in enumerate(tasks) for j, arm in enumerate(heldout.ARMS)]
            base = dict(protocol="learning-curve-1", budget_authority=str(authority), matrix=matrix,
                        families=[dict(heldout=tasks)])
            pairs.write(root / "manifest.json", base)
            pairs.write(authority / "paid-state.json", {"runs": {"cohorts/curve/heldout/t0/agent":
                dict(status="failed", error="scoring failed", summary={"total_cost_usd": .3})}})
            with patch.object(heldout.curve, "verify", return_value=(base, None)):
                result = heldout.report(root)
            self.assertEqual(result["assignment_denominator"], 16)
            self.assertEqual(result["collected"], 0)
            self.assertEqual(result["unknown_cost_assignments"], 15)
            self.assertEqual(result["known_cost_usd"], .3)
            self.assertEqual(result["rows"][0]["ledger_attempt"]["error"], "scoring failed")
            self.assertFalse(result["passed"])

    def test_cold_report_keeps_eight_denominator_and_never_claims_cpu_savings(self):
        import curve_cold
        with tempfile.TemporaryDirectory() as tmp:
            authority = Path(tmp)
            root = authority / "cohort/cold"
            root.mkdir(parents=True)
            tasks = ["t0", "t1", "t2", "t3"]
            matrix = [dict(task=t, arm=arm, order=i * 2 + j) for i, t in enumerate(tasks)
                      for j, arm in enumerate(("agent", "instincts") if i % 2 == 0 else ("instincts", "agent"))]
            pairs.write(authority / "paid-state.json", {"runs": {}})
            base = dict(budget_authority=str(authority), matrix=matrix, families=[dict(heldout=tasks)])
            with patch.object(curve_cold, "verify", return_value=(base, None)):
                result = curve_cold.report(root)
            self.assertEqual(result["assignment_denominator"], 8)
            self.assertEqual(result["collected"], 0)
            self.assertEqual(result["cpu_savings_claim"], "not tested")
            self.assertEqual({r["arm"] for r in result["rows"]}, {"agent", "instincts"})

    def test_cold_native_review_cannot_be_reused_for_sibling_root(self):
        import curve_cold
        with tempfile.TemporaryDirectory() as tmp:
            original = Path(tmp) / "original"
            original.mkdir()
            with self.assertRaisesRegex(ValueError, "nested root"):
                curve_cold.freeze(Path(tmp) / "sibling", original)


if __name__ == "__main__":
    unittest.main()
