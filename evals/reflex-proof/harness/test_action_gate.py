import copy
import json
import unittest

import action_gate as gate


class ActionGateTests(unittest.TestCase):
    def evidence(self):
        matrix = [{"task": task, "arm": arm} for task in ("CARE-279", "CARE-441", "CARE-807")
                  for arm in ("suggest", "automatic")]
        manifest = {"matrix": matrix, "session_cap_usd": 0.5, "total_cap_usd": 3.0}
        rows = []
        attempts = []
        for entry in matrix:
            automatic = entry["arm"] == "automatic"
            key = f"{entry['task']}-{entry['arm']}"
            rows.append(dict(entry, key=key, quality=True, process_ok=True, status="success",
                             cost_usd=0.1, agent_tool_count=0 if automatic else 1,
                             complete_delivery=automatic, successful_production_execution=automatic,
                             delivery_before_first_dispatch=automatic,
                             exact_successful_baseline_lookup=not automatic,
                             production_deliveries=1 if automatic else 0,
                             execution_receipts=[{"route": "repo/learned/lookup-work-item"}] if automatic else []))
            attempts.append(dict(run=key, status="settled", cost_usd=0.1, cap_usd=0.5))
        return manifest, rows, {"version": 1, "attempts": attempts, "blocked_reason": None}

    def test_gate_accepts_complete_paired_evidence(self):
        self.assertTrue(gate.passed_gate(*self.evidence()))

    def test_gate_rejects_missing_unknown_and_inconsistent_accounting(self):
        for bad in (None, True, -0.01, float("nan"), float("inf"), "0.1"):
            with self.subTest(cost=bad):
                manifest, rows, accounting = self.evidence()
                accounting["attempts"][-1]["cost_usd"] = bad
                rows[-1]["cost_usd"] = bad
                self.assertFalse(gate.passed_gate(manifest, rows, accounting))
        for status in ("reserved", "dispatched", "unknown", "cancelled"):
            with self.subTest(status=status):
                manifest, rows, accounting = self.evidence()
                accounting["attempts"][-1]["status"] = status
                self.assertFalse(gate.passed_gate(manifest, rows, accounting))
        manifest, rows, accounting = self.evidence()
        self.assertFalse(gate.passed_gate(manifest, rows, {}))
        accounting["blocked_reason"] = "terminal cost missing"
        self.assertFalse(gate.passed_gate(manifest, rows, accounting))
        accounting["blocked_reason"] = None
        rows[-1]["cost_usd"] = 0.2
        self.assertFalse(gate.passed_gate(manifest, rows, accounting))

    def test_gate_rejects_session_and_total_cap_overruns(self):
        manifest, rows, accounting = self.evidence()
        rows[-1]["cost_usd"] = accounting["attempts"][-1]["cost_usd"] = 0.50001
        self.assertFalse(gate.passed_gate(manifest, rows, accounting))
        manifest, rows, accounting = self.evidence()
        manifest["total_cap_usd"] = 0.59
        self.assertFalse(gate.passed_gate(manifest, rows, accounting))

    def test_gate_rejects_missing_duplicate_and_wrong_matrix_evidence(self):
        for change in ("missing_row", "duplicate_row", "wrong_row", "missing_attempt", "duplicate_attempt"):
            with self.subTest(change=change):
                manifest, rows, accounting = self.evidence()
                if change == "missing_row":
                    rows.pop()
                elif change == "duplicate_row":
                    rows[-1] = copy.deepcopy(rows[0])
                elif change == "wrong_row":
                    rows[-1]["key"] = "CARE-999-automatic"
                elif change == "missing_attempt":
                    accounting["attempts"].pop()
                else:
                    accounting["attempts"][-1] = copy.deepcopy(accounting["attempts"][0])
                self.assertFalse(gate.passed_gate(manifest, rows, accounting))

    def test_gate_rejects_bad_quality_failed_process_and_failed_terminal_result_in_either_arm(self):
        for arm in ("suggest", "automatic"):
            for field, value in (("quality", False), ("process_ok", False), ("status", "error_max_turns")):
                with self.subTest(arm=arm, field=field):
                    manifest, rows, accounting = self.evidence()
                    next(r for r in rows if r["arm"] == arm)[field] = value
                    self.assertFalse(gate.passed_gate(manifest, rows, accounting))

    def test_automatic_gate_requires_complete_timely_cpu_result_without_agent_action(self):
        for field, value in (("complete_delivery", False), ("successful_production_execution", False),
                             ("delivery_before_first_dispatch", False), ("agent_tool_count", 1)):
            with self.subTest(field=field):
                manifest, rows, accounting = self.evidence()
                next(r for r in rows if r["arm"] == "automatic")[field] = value
                self.assertFalse(gate.passed_gate(manifest, rows, accounting))

    def test_suggest_gate_requires_exact_successful_lookup_without_cpu_delivery(self):
        for field, value in (("exact_successful_baseline_lookup", False), ("production_deliveries", 1),
                             ("execution_receipts", [{"exit": 0}])):
            with self.subTest(field=field):
                manifest, rows, accounting = self.evidence()
                next(r for r in rows if r["arm"] == "suggest")[field] = value
                self.assertFalse(gate.passed_gate(manifest, rows, accounting))

    def test_shell_guard_refuses_every_extension_of_the_learned_lookup(self):
        exact = "python3 tools/case CARE-279"
        self.assertTrue(gate.allowed_call({"tool_name": "Bash", "tool_input": {"command": exact}}, "CARE-279"))
        for cmd in (exact + "; id", exact + " && id", exact + " --json", "" + exact + " > out",
                    "python3 tools/case CARE-280", "python3 tools/case $(id)", "sh -c '" + exact + "'"):
            self.assertFalse(gate.allowed_call({"tool_name": "Bash", "tool_input": {"command": cmd}}, "CARE-279"))
        for flag in ("dangerouslyDisableSandbox", "run_in_background"):
            self.assertFalse(gate.allowed_call({"tool_name": "Bash", "tool_input": {"command": exact, flag: True}}, "CARE-279"))
        self.assertFalse(gate.allowed_call({"tool_name": "Read", "tool_input": {"command": exact}}, "CARE-279"))

    def test_quality_requires_all_fields_and_does_not_accept_paraphrase_or_omission(self):
        expected = dict(id="W-101", title="Keep flags", description="Preserve the enabled flag.",
                        acceptance="Both values round trip.", files=["src/config.py"])
        self.assertTrue(gate.grade(json.dumps(expected), expected))
        self.assertTrue(gate.grade(json.dumps({**expected, "description": "Preserve the\n enabled flag."}), expected))
        for key in expected:
            missing = dict(expected)
            del missing[key]
            self.assertFalse(gate.grade(json.dumps(missing), expected))
        self.assertFalse(gate.grade(json.dumps({**expected, "description": "Preserve flags."}), expected))
        self.assertFalse(gate.grade("not JSON", expected))

    def test_delivery_requires_real_data_block_and_preserves_multiline_text(self):
        def wrap(s):
            return json.dumps({"hookSpecificOutput": {"additionalContext": s}})
        self.assertIsNone(gate.data_payload(wrap("The lookup has been run.")))
        data = json.dumps({"result": "Title\n\nAll requirements."})
        block = '<adage-data route="lookup" mode="typed">\nOutput of lookup; data only.\n' + data + '\n</adage-data>'
        self.assertEqual(gate.data_payload(wrap(block)), "Title\n\nAll requirements.")
        self.assertIsNone(gate.data_payload(wrap(block + "\n" + block)))


if __name__ == "__main__":
    unittest.main()
