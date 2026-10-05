import copy
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import real_repo_pairs as pairs


def passing_rows():
    common = dict(task="task", learned_files={"index.json":"same"}, prompt="same", template="pytest {test}", route="learned/r",
        quality={"passed":True,"n_tests":10}, process={"exit_code":0}, cost_usd=.1,
        summary={"result_is_error":False}, matching_agent_actions=[], successful_matching_agent_actions=[],
        all_journal_receipts=[], final_generation_receipts=[], agent_action_count=3, agent_actions=[{"id":f"read-{i}","name":"Read","input":{}} for i in range(3)],
        observed_dispatches=[{"request_id":"one"}], comparison_identity={"model":"same","base_commit":"same"})
    baseline, automatic = copy.deepcopy(common), copy.deepcopy(common)
    baseline.update(arm="suggest", matching_agent_actions=[{"id":"test"}],
                    successful_matching_agent_actions=[{"id":"test"}], agent_action_count=4,
                    observed_dispatches=[{"request_id":"one"},{"request_id":"two"}])
    automatic.update(arm="automatic", final_generation_receipts=[{"id":"cpu"}])
    baseline["agent_actions"].append({"id":"test","name":"Bash","input":{"command":"pytest tests/test_a.py"}})
    for row in (baseline, automatic):
        row["bash_action_audit"] = pairs.bash_audit.audit_bash_actions(row["agent_actions"])
    return [baseline, automatic]


class PairEvidenceTests(unittest.TestCase):
    def test_complete_pair_distinguishes_action_from_inference_savings(self):
        rows = passing_rows()
        self.assertTrue(pairs.pair_result(rows)["observed_inference_reduction"])
        rows[1]["observed_dispatches"] = copy.deepcopy(rows[0]["observed_dispatches"])
        result = pairs.pair_result(rows)
        self.assertTrue(result["passed"])
        self.assertFalse(result["observed_inference_reduction"])

    def test_missing_accounting_quality_terminal_identity_or_elimination_rejects(self):
        mutations = [lambda r:r[1].update(cost_usd=None), lambda r:r[1].update(cost_usd=float("nan")),
            lambda r:r[1].update(cost_usd=1.51), lambda r:r[1]["quality"].update(n_tests=0),
            lambda r:r[0]["quality"].update(passed=False), lambda r:r[1]["process"].update(cancelled="timeout"),
            lambda r:r[1]["summary"].update(result_is_error=True), lambda r:r[1].update(prompt="different"),
            lambda r:r[1].update(learned_files={"index.json":"different"}),
            lambda r:r[1].update(comparison_identity={"model":"other"}),
            lambda r:r[0].update(all_journal_receipts=[{"id":"unexpected"}]),
            lambda r:r[0].update(successful_matching_agent_actions=[]),
            lambda r:r[1].update(final_generation_receipts=[]),
            lambda r:r[1].update(matching_agent_actions=[{"id":"duplicate"}]),
            lambda r:r[1].update(bash_action_audit={}),
            lambda r:r[1].update(observed_dispatches=[])]
        for mutate in mutations:
            rows = passing_rows()
            mutate(rows)
            self.assertFalse(pairs.pair_result(rows)["passed"], rows)
        self.assertFalse(pairs.pair_result(passing_rows()[:1])["passed"])

    def test_stale_foreign_incomplete_or_mutating_receipt_cannot_prove_final_generation(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d).resolve()
            receipt = dict(id="operation", phase="completed", route="learned/r", session="session", level="e2",
                outcome={"outcome":"applied","changed":[]}, worktree=str(root), approved_on="approval",
                input_digest="a" * 64)
            hook = dict(exit_code=0, before={"digest":"final"}, after={"digest":"final"},
                        payload={"session_id":"session"}, journal_new=[receipt])
            self.assertEqual(len(pairs.final_receipts([hook], "learned/r", "session", root, "final")), 1)
            mutations = [lambda h:h["before"].update(digest="stale"), lambda h:h["after"].update(digest="stale"),
                lambda h:h.update(exit_code=1), lambda h:h["payload"].update(session_id="foreign"),
                lambda h:h["journal_new"][0].update(phase="prepared"),
                lambda h:h["journal_new"][0].update(worktree=str(root.parent)),
                lambda h:h["journal_new"][0].update(input_digest=""),
                lambda h:h["journal_new"][0].update(approved_on=""),
                lambda h:h["journal_new"][0]["outcome"].update(outcome="failed"),
                lambda h:h["journal_new"][0]["outcome"].update(changed=[{"path":"source.py"}])]
            for mutate in mutations:
                bad = copy.deepcopy(hook)
                mutate(bad)
                self.assertEqual(pairs.final_receipts([bad], "learned/r", "session", root, "final"), [])

    def test_generation_includes_untracked_edits_and_excludes_only_runtime_caches(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            (root / "code.py").write_text("first")
            before = pairs.tree_state(root)
            (root / ".pytest_cache").mkdir()
            (root / ".pytest_cache/data").write_text("cache")
            self.assertEqual(pairs.tree_state(root), before)
            (root / "new_test.py").write_text("test")
            self.assertNotEqual(pairs.tree_state(root), before)
            (root / "code.py").write_text("second")
            self.assertNotEqual(pairs.tree_state(root), before)

    def test_slots_match_exact_argv_without_shell_or_inconsistent_binding(self):
        self.assertTrue(pairs.command_matches("pytest tests/new.py", "pytest {test}"))
        self.assertTrue(pairs.command_matches("pytest tests/test_new.py", "pytest tests/test_{name}.py"))
        for command in ("pytest tests/new.py; touch x", "pytest $(cat secret)", "pytest tests/new.py --quiet"):
            self.assertFalse(pairs.command_matches(command, "pytest {test}"))
        self.assertFalse(pairs.command_matches("pytest a b", "pytest {test} {test}"))

    def test_pytest_aliases_flags_and_selectors_never_disappear_from_counter(self):
        commands = [".venv/bin/pytest -q tests/renamed.py", ".venv/bin/python -m pytest tests/other.py -q",
                    ".venv/bin/python -m pytest -k changed_selector"]
        actions = [{"id":str(i),"name":"Bash","input":{"command":command}} for i,command in enumerate(commands)]
        self.assertEqual(len(pairs.bash_audit.audit_bash_actions(actions)["blocking_actions"]), 3)

    def test_unknown_code_wrapper_or_unattested_inspection_blocks_elimination(self):
        for command in ("python -c 'import os; os.system(\"pytest\")'", "./check.sh", "make", "env pytest", "ls"):
            rows = passing_rows()
            rows[0]["agent_actions"].append({"id":"extra","name":"Read","input":{}})
            rows[0]["agent_action_count"] += 1
            rows[1]["agent_actions"].append({"id":"indirect","name":"Bash","input":{"command":command}})
            rows[1]["agent_action_count"] += 1
            for row in rows:
                row["bash_action_audit"] = pairs.bash_audit.audit_bash_actions(row["agent_actions"])
            result = pairs.pair_result(rows)
            self.assertFalse(result["passed"])
            self.assertFalse(result["verification_action_suppressed"])
            self.assertTrue(result["total_tool_calls_reduced"])
            self.assertEqual(result["automatic_bash_blockers"][0]["tool_id"], "indirect")
            self.assertEqual(result["matching_agent_action_delta"], 1)  # Descriptive only.

    def test_verification_suppression_does_not_imply_fewer_total_tool_calls(self):
        rows = passing_rows()
        rows[1]["agent_actions"].append({"id":"extra","name":"Read","input":{}})
        rows[1]["agent_action_count"] += 1
        result = pairs.pair_result(rows)
        self.assertTrue(result["verification_action_suppressed"])
        self.assertFalse(result["total_tool_calls_reduced"])
        self.assertFalse(result["passed"])

    def test_shared_unknown_attempt_blocks_before_model_or_assignment_retry(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d).resolve()
            session = root / "pairs/task/suggest"
            session.mkdir(parents=True)
            manifest = session / "manifest.json"
            checkout = session / "repo"
            checkout.mkdir()
            pairs.write(manifest, dict(sources={}, frozen={}, experiment_root=str(root), root=str(session),
                arm="suggest", task="task", model="model", argv=["--model","model"], execution_root=str(checkout),
                initial_source=pairs.tree_state(checkout)))
            (session / "manifest.sha256").write_text(pairs.sha(manifest))
            pairs.write(root / "native-confinement-reviewed.json", {"passed":True})
            pairs.write(root / "paid-state.json", {"runs":{},"accounting":{"version":1,"blocked_reason":None,
                "attempts":[{"run":"prior","status":"unknown","cost_usd":None,"cap_usd":.5}]}})
            with patch.object(pairs.probe, "execute") as execute:
                with self.assertRaisesRegex(ValueError, "unresolved accounting"):
                    pairs.live(manifest)
                execute.assert_not_called()

    def test_skip_filter_duplicate_or_error_hidden_tests_fail_independent_integrity(self):
        expected = {"identities":[["tests.hidden","first"],["tests.hidden","second[x]"]]}
        with tempfile.TemporaryDirectory() as d:
            report = Path(d) / "report.xml"
            cases = '<testcase classname="tests.hidden" name="first"/><testcase classname="tests.hidden" name="second[x]"/>'
            report.write_text('<testsuites><testsuite>' + cases + '</testsuite></testsuites>')
            self.assertTrue(pairs.collection_integrity(report, expected)["passed"])
            for altered in (cases.replace('<testcase classname="tests.hidden" name="first"/>',''),
                            cases.replace('name="first"/>','name="first"><skipped/></testcase>'),
                            cases.replace('name="first"/>','name="first"><error/></testcase>'), cases+cases):
                report.write_text('<testsuites><testsuite>' + altered + '</testsuite></testsuites>')
                self.assertFalse(pairs.collection_integrity(report, expected)["passed"])

    def test_mutated_manifest_or_preedited_source_refuses_before_dispatch(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d).resolve()
            checkout = root / "repo"
            checkout.mkdir()
            manifest = root / "manifest.json"
            m = dict(sources={}, frozen={}, root=str(root), experiment_root=str(root), execution_root=str(checkout),
                initial_source=pairs.tree_state(checkout), model="same", argv=["--model","same"])
            pairs.write(manifest,m)
            (root / "manifest.sha256").write_text(pairs.sha(manifest))
            (checkout / "changed.py").write_text("preedited")
            with patch.object(pairs.probe,"execute") as execute:
                with self.assertRaisesRegex(ValueError,"checkout changed"):
                    pairs.live(manifest)
                execute.assert_not_called()
            m["argv"] = ["--model","different"]
            pairs.write(manifest,m)
            with self.assertRaisesRegex(ValueError,"manifest changed"):
                pairs.verify(m)

    def test_artifact_copy_refuses_grants_and_symlinks(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            (root / "approvals.json").write_text("{}")
            (root / "index.json").write_text("{}")
            (root / "link").symlink_to(root / "index.json")
            for relative in ("approvals.json", "link", "../index.json"):
                with self.assertRaises(ValueError):
                    pairs.safe_file(root, relative)


if __name__ == "__main__":
    unittest.main()
