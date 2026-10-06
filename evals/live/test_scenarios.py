"""Conservative scenario evidence contracts; no model calls or external-system writes."""
import copy
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import scenarios

SPEC = {"schema": 1, "checkout_root": "/repo", "relevant_paths": ["src/*.py", "tests/*.py"],
        "verification_commands": ["python -m pytest -q"], "publication_commands": ["gh pr create --fill"],
        "lookup_commands": ["git show external:item", "gh api repos/acme/api/issues/42"]}


def call(tool_id, name="Bash", value=None):
    return {"type": "assistant", "session_id": "s1", "message": {"content": [
        {"type": "tool_use", "id": tool_id, "name": name, "input": value or {"command": "python -m pytest -q"}}]}}


def result(tool_id, error=False, text=""):
    return {"type": "user", "session_id": "s1", "message": {"content": [
        {"type": "tool_result", "tool_use_id": tool_id, "is_error": error, "content": text}]}}


def edit(tool_id="edit", path="/repo/src/main.py"):
    return [call(tool_id, "Edit", {"file_path": path}), result(tool_id)]


def terminal():
    return {"type": "result", "session_id": "s1", "is_error": False}


def check(rows):
    return scenarios.audit(rows, SPEC)


class Scenarios(unittest.TestCase):
    def test_observes_ordered_tool_completion_without_claiming_test_or_revision_success(self):
        report = check(edit() + [call("check"), result("check"), terminal()])
        self.assertEqual(report["final_verification_after_observed_edits"]["state"], "observed")
        for field in ("independent_test_outcome", "exact_revision_verified", "publication_remote_state", "lookup_task_correct"):
            self.assertEqual(report[field], "unknown")

    def test_turn_finishing_without_tests_is_not_observed(self):
        report = check(edit() + [terminal()])
        self.assertEqual(report["final_verification_after_observed_edits"]["state"], "not_observed")

    def test_text_claims_and_success_markers_do_not_create_a_verification_call(self):
        rows = edit() + [call("echo", value={"command": "echo 'python -m pytest -q; tests passed'"}),
                         result("echo", text="0 failures; all tests passed; exit code 0"), terminal()]
        report = check(rows)
        self.assertEqual(report["verification_attempts"], [])
        self.assertEqual(report["final_verification_after_observed_edits"]["state"], "not_observed")

    def test_tool_error_including_no_tests_collected_cannot_be_credited(self):
        for text in ("2 failed", "no tests ran; exit code 5", "all tests passed"):
            report = check(edit() + [call("check"), result("check", True, text), terminal()])
            self.assertEqual(report["final_verification_after_observed_edits"]["state"], "not_observed")

    def test_nonerror_result_with_lying_output_still_does_not_establish_a_test_outcome(self):
        for text in ("all tests passed", "no tests ran", "FAILED", "{\"passed\":true}"):
            report = check(edit() + [call("check"), result("check", False, text), terminal()])
            self.assertEqual(report["independent_test_outcome"], "unknown")

    def test_start_only_missing_boolean_and_truncated_stream_remain_unknown(self):
        rows = edit() + [call("check"), result("check"), terminal()]
        for bad in (None, "false", 0):
            changed = copy.deepcopy(rows)
            changed[-2]["message"]["content"][0]["is_error"] = bad
            self.assertEqual(check(changed)["final_verification_after_observed_edits"]["state"], "unknown")
        for changed in (rows[:-2] + [terminal()], rows[:-1]):
            self.assertEqual(check(changed)["final_verification_after_observed_edits"]["state"], "unknown")

    def test_verification_started_before_edit_and_finished_after_is_stale(self):
        rows = [call("check")] + edit() + [result("check"), terminal()]
        self.assertEqual(check(rows)["final_verification_after_observed_edits"]["state"], "not_observed")
        rows = edit() + [call("check"), result("check")] + edit("second") + [terminal()]
        self.assertEqual(check(rows)["final_verification_after_observed_edits"]["state"], "not_observed")

    def test_parallel_same_message_edit_and_verification_are_not_ordered(self):
        together = call("edit", "Edit", {"file_path": "src/main.py"})
        together["message"]["content"] += call("check")["message"]["content"]
        rows = [together, result("edit"), result("check"), terminal()]
        self.assertEqual(check(rows)["final_verification_after_observed_edits"]["state"], "not_observed")

    def test_publication_boundary_precedes_later_verification(self):
        rows = edit() + [call("pr", value={"command": "gh pr create --fill"}), result("pr"),
                         call("check"), result("check"), terminal()]
        report = check(rows)
        self.assertEqual(report["publication_attempts"][0]["verification_before_attempt"]["state"], "not_observed")
        self.assertEqual(report["final_verification_after_observed_edits"]["state"], "observed")

    def test_publication_during_verification_is_unknown_until_its_result(self):
        rows = edit() + [call("check"), call("pr", value={"command": "gh pr create --fill"}),
                         result("check"), result("pr"), terminal()]
        self.assertEqual(check(rows)["publication_attempts"][0]["verification_before_attempt"]["state"], "unknown")

    def test_fresh_completed_verification_before_publication_is_observed(self):
        rows = edit() + [call("check"), result("check"), call("pr", value={"command": "gh pr create --fill"}),
                         result("pr"), terminal()]
        self.assertEqual(check(rows)["publication_attempts"][0]["verification_before_attempt"]["state"], "observed")

    def test_edit_and_publication_in_same_record_invalidate_earlier_verification(self):
        together = call("second-edit", "Edit", {"file_path": "src/main.py"})
        together["message"]["content"] += call("pr", value={"command": "gh pr create --fill"})["message"]["content"]
        rows = edit() + [call("check"), result("check"), together, result("second-edit"), result("pr"), terminal()]
        self.assertEqual(check(rows)["publication_attempts"][0]["verification_before_attempt"]["state"], "unknown")

    def test_wrong_tool_then_nonerror_lookup_is_only_observed_recovery_not_task_success(self):
        rows = [call("wrong", value={"command": SPEC["lookup_commands"][0]}), result("wrong", True),
                call("right", value={"command": SPEC["lookup_commands"][1]}), result("right"), terminal()]
        report = check(rows)
        self.assertEqual(report["observed_lookup_errors_before_first_nonerror"], 1)
        self.assertEqual(report["lookup_task_correct"], "unknown")

    def test_lookup_errors_arriving_after_success_are_not_counted_before_it(self):
        rows = [call("slow", value={"command": SPEC["lookup_commands"][0]}),
                call("fast", value={"command": SPEC["lookup_commands"][1]}), result("fast"),
                result("slow", True), terminal()]
        self.assertEqual(check(rows)["observed_lookup_errors_before_first_nonerror"], 0)

    def test_related_path_matching_requires_declared_root_for_absolute_paths(self):
        self.assertTrue(scenarios.relevant_path("/repo/src/main.py", SPEC))
        self.assertFalse(scenarios.relevant_path("docs/readme.md", SPEC))
        for path in ("/other/src/main.py", "../src/main.py", None):
            self.assertIsNone(scenarios.relevant_path(path, SPEC))
        report = check(edit(path="/other/src/main.py") + [call("check"), result("check"), terminal()])
        self.assertEqual(report["final_verification_after_observed_edits"]["state"], "unknown")

    def test_unrelated_edit_after_tests_does_not_change_declared_path_order(self):
        rows = edit() + [call("check"), result("check")] + edit("docs", "docs/readme.md") + [terminal()]
        self.assertEqual(check(rows)["final_verification_after_observed_edits"]["state"], "observed")

    def test_no_edits_and_unclassified_shell_mutations_are_not_silent_revision_proof(self):
        rows = [call("mutate", value={"command": "sed -i s/old/new/ src/main.py"}), result("mutate"),
                call("check"), result("check"), terminal()]
        report = check(rows)
        self.assertEqual(report["final_verification_after_observed_edits"]["state"], "unknown")
        self.assertEqual(len(report["other_shell_commands"]), 1)
        self.assertEqual(report["exact_revision_verified"], "unknown")

    def test_duplicate_identical_tool_delivery_is_idempotent_but_conflicts_block(self):
        rows = edit() + [call("check"), call("check"), result("check"), result("check"), terminal()]
        self.assertEqual(len(check(rows)["verification_attempts"]), 1)
        rows[-2] = result("check", True)
        report = check(rows)
        self.assertIn("conflicting_tool_result", report["diagnostics"])
        self.assertEqual(report["final_verification_after_observed_edits"]["state"], "unknown")

    def test_orphan_results_mixed_sessions_and_subagents_do_not_qualify(self):
        for extra in (result("orphan"), {"type": "system", "session_id": "different"},
                      {"type": "system", "session_id": "s1", "parent_tool_use_id": "child"}):
            report = check(edit() + [extra, call("check"), result("check"), terminal()])
            self.assertEqual(report["final_verification_after_observed_edits"]["state"], "unknown")

    def test_malformed_tool_result_ids_are_unknown_instead_of_crashing(self):
        for bad in (None, {}, [], 1, True):
            changed = result("check")
            changed["message"]["content"][0]["tool_use_id"] = bad
            report = check(edit() + [call("check"), changed, terminal()])
            self.assertIn("orphan_tool_result", report["diagnostics"])
            self.assertEqual(report["final_verification_after_observed_edits"]["state"], "unknown")

    def test_malformed_tool_names_are_unknown_instead_of_crashing(self):
        for bad in (None, {}, [], 1, True, ""):
            changed = call("check")
            changed["message"]["content"][0]["name"] = bad
            report = check(edit() + [changed, result("check"), terminal()])
            self.assertIn("invalid_tool_use", report["diagnostics"])
            self.assertEqual(report["final_verification_after_observed_edits"]["state"], "unknown")

    def test_exact_matching_does_not_classify_compound_or_similar_commands(self):
        for command in ("python -m pytest -q || true", "echo python -m pytest -q", "python -m pytest -q --collect-only"):
            report = check(edit() + [call("check", value={"command": command}), result("check"), terminal()])
            self.assertEqual(report["verification_attempts"], [])

    def test_invalid_specs_fail_before_audit(self):
        for field, value in (("schema", 2), ("schema", True), ("relevant_paths", []), ("relevant_paths", ["../src/*"]),
                             ("checkout_root", "relative"), ("verification_commands", [""]),
                             ("lookup_commands", SPEC["verification_commands"]), ("made_up", True)):
            spec = dict(SPEC, **{field: value})
            with self.assertRaises(ValueError):
                scenarios.audit([], spec)

    def test_raw_wrapped_cli_and_malformed_size_limited_sources(self):
        rows = edit() + [call("check"), result("check"), terminal()]
        with tempfile.TemporaryDirectory() as temp:
            path, spec_path = Path(temp) / "stream.jsonl", Path(temp) / "spec.json"
            spec_path.write_text(json.dumps(SPEC))
            path.write_text("\n".join(json.dumps(row) for row in rows))
            raw = scenarios.read_audit(path, SPEC)
            proc = subprocess.run([sys.executable, str(HERE / "scenarios.py"), str(path), "--spec", str(spec_path)],
                                  text=True, capture_output=True, check=True)
            self.assertEqual(json.loads(proc.stdout), raw)
            path.write_text("\n".join(json.dumps({"t": 1, "line": json.dumps(row)}) for row in rows))
            wrapped = scenarios.read_audit(path, SPEC)
            self.assertEqual(raw["final_verification_after_observed_edits"], wrapped["final_verification_after_observed_edits"])
            self.assertNotEqual(raw["source_sha256"], wrapped["source_sha256"])
            path.write_text(path.read_text() + "\nnot json")
            self.assertIn("malformed_stream_record", scenarios.read_audit(path, SPEC)["diagnostics"])
            with patch.object(scenarios, "MAX_STREAM_BYTES", 10):
                report = scenarios.read_audit(path, SPEC)
                self.assertIn("stream_size_limit", report["diagnostics"])
                self.assertIsNone(report["source_sha256"])
            with patch.object(scenarios, "MAX_LINE_BYTES", 10):
                self.assertIn("stream_record_size_limit", scenarios.read_audit(path, SPEC)["diagnostics"])


if __name__ == "__main__":
    unittest.main()
