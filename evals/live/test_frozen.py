"""Frozen execution through the real Rust validator and a deterministic, free agent double.

Build first: cargo build --locked -p adage-cli. No API credentials or live agents are used.
"""
import contextlib
import io
import json
import os
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import frozen
import runner
from budget import Budget

ADAGE = runner.REPO / "target" / "debug" / ("adage.exe" if os.name == "nt" else "adage")
VERSION = "fixture-1 (Claude Code)"


class FakeLive:
    runs = []
    mode = "complete"

    def __init__(self, cfg, root, adage, round_, isolate, resume, observer):
        self.cfg, self.root, self.adage, self.observer, self.isolate = cfg, root, adage, observer, isolate
        path = root / "state.json"
        self.state = frozen.read_json(path) if path.exists() else {"runs": {}, "harness_version": VERSION}
        self.budget = Budget(self.state, cfg["budget_usd"], cfg["session_budget_usd"], persist=self.save)

    def save(self):
        frozen.publish(self.root / "state.json", self.state, replace=True)

    def run_one(self, run_id, phase, arm, task, prompt):
        self.runs.append(run_id)
        attempt, reason = self.budget.reserve(run_id)
        if reason:
            raise ValueError(reason)
        directory = self.root / "runs" / run_id
        work = directory / self.cfg["workdir"]
        work.parent.mkdir(parents=True)
        subprocess.run(runner.expand(self.cfg["sandbox"], dir=str(work), task=task, arm=arm), check=True)
        env = runner.session_env(arm, directory / "home", self.home, True, {})
        self.observer.prepared(self, run_id, task, arm, work, directory / "settings.json", env)
        assert env["ADAGE_DISABLE"] == ("1" if arm == "off" else "0")
        assert self.budget.dispatch(attempt)
        if self.mode == "interrupted":
            self.budget.settle(attempt)
            raise RuntimeError("synthetic crash after dispatch")
        events = [json.loads(row) for row in (HERE / "fixtures" / "claude-usage.jsonl").read_text().splitlines()]
        for row in events:
            row["session_id"] = run_id
            if self.mode == "unknown-cost" and row.get("type") == "result":
                row.pop("total_cost_usd", None)
            if self.mode == "last-overrun" and len(self.runs) == 4 and row.get("type") == "result":
                row["total_cost_usd"] = 1.01
            message = row.get("message") or row.get("event", {}).get("message")
            if message:
                message["id"] += "-" + run_id
                message["model"] = "unplanned-model" if self.mode == "wrong-model" else self.cfg["model"]
            if "request_id" in row:
                row["request_id"] += "-" + run_id
            if "modelUsage" in row:
                model = "unplanned-model" if self.mode == "wrong-model" else self.cfg["model"]
                row["modelUsage"] = {model: next(iter(row["modelUsage"].values()))}
            if arm == "adage":
                usages = [row.get("usage"), (message or {}).get("usage"), row.get("event", {}).get("usage")]
                usages += list(row.get("modelUsage", {}).values())
                for usage in filter(None, usages):
                    for key in (*frozen.evidence.COUNTERS, "inputTokens", "outputTokens", "cacheReadInputTokens", "cacheCreationInputTokens"):
                        if key in usage:
                            usage[key] = usage[key] * 3 // 4
            self.observer.event(row)
        if self.mode == "unknown-usage":
            events = [row for row in events if row.get("type") in ("system", "result")]
        (directory / "stream.jsonl").write_text("\n".join(json.dumps(r) for r in events) + "\n")
        self.observer.graded('{"passed":false}' if self.mode == "task-failure" else '{"passed":true}',
                             1 if self.mode == "grader-error" else 0, False)
        self.state["runs"][run_id] = {"metrics": {"exit_code": 0, "timed_out": False}}
        self.budget.settle(attempt, events[-1])
        self.save()


@unittest.skipUnless(sys.platform == "linux", "frozen execution uses the Linux confinement profile")
class FrozenFlow(unittest.TestCase):
    def setUp(self):
        self.assertTrue(ADAGE.is_file(), "build the CLI first: cargo build --locked -p adage-cli")
        for owner, name, result in ((runner, "harness_version", VERSION),
                                    (runner, "auth_source", ("env", ["fixture-auth"])),
                                    (frozen, "confinement_probe", None)):
            probe = patch.object(owner, name, return_value=result)
            probe.start()
            self.addCleanup(probe.stop)
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.objects = frozen.Objects(self.root / "objects")
        self.generator = self.root / "generate.py"
        self.generator.write_text("import pathlib,sys\np=pathlib.Path(sys.argv[1]);p.mkdir()\n"
                                  "(p/'source.txt').write_text('fixed task input')\n(p/'.git').mkdir()\n"
                                  "(p/'.git'/'config').write_text('[remote \\\"origin\\\"]\\n\\turl = https://fixture/repo\\n')\n")
        self.grader = self.root / "grade.py"
        self.grader.write_text("print('{\"passed\":true}')\n")
        self.config = self.root / "config.json"
        frozen.publish(self.config, {
            "name": "synthetic-frozen-flow", "repo_key": "fixture/repo", "workdir": "checkout",
            "sandbox": [sys.executable, str(self.generator), "{dir}"],
            "score": [sys.executable, str(self.grader)], "prompts": ["Work on {id}."],
            "train": {"tasks": ["TRAIN-0"]}, "eval": {"tasks": ["TEST-0", "TEST-1"]},
            "model": "fixture-model-v1", "max_turns": 4, "session_budget_usd": 1,
            "budget_usd": 4, "confine": True,
        })
        self.audit = self.root / "audit.json"
        frozen.publish(self.audit, {
            "repo": "fixture/repo", "training_generation": "0" * 64, "sessions": 1,
            "training_groups": [frozen.group_key("fixture/repo", {"task_id": "TRAIN-0"})],
            "training_declared_identity": 1, "training_inferred_identity": 0, "training_unknown_identity": 0,
            "recorded_outcomes": 1, "proxy_outcomes": 0, "unknown_outcomes": 0,
        })
        self.index = self.root / "index.json"
        self.index.write_bytes((HERE / "fixtures" / "reflex-index.json").read_bytes())
        self.path = self.root / "frozen.json"
        with patch.object(runner, "harness_version", return_value=VERSION):
            # Deliberately relaxed synthetic quality margin. Default production policy is unchanged.
            self.frozen = frozen.prepare(self.config, self.audit, self.index, self.objects, self.path, ADAGE,
                                          [self.generator, self.grader], learner_revision="synthetic-learner-v1",
                                          policy={"min_groups": 2, "max_quality_loss": 1,
                                          "min_savings": 0.2, "min_baseline_success": 0.8})
        self.run_root = self.root / "run"
        FakeLive.runs, FakeLive.mode = [], "complete"

    def execute(self):
        return frozen.execute(self.path, self.frozen, self.objects, self.run_root, ADAGE, live_factory=FakeLive)

    def collect_one_job(self):
        instances = 0

        def stop_between_jobs(*args, **kwargs):
            nonlocal instances
            instances += 1
            if instances > 1:
                raise RuntimeError("fixture stop between jobs")
            return FakeLive(*args, **kwargs)

        with self.assertRaisesRegex(RuntimeError, "fixture stop between jobs"):
            frozen.execute(self.path, self.frozen, self.objects, self.run_root, ADAGE, live_factory=stop_between_jobs)
        plan = frozen.preflight(self.frozen, self.objects, self.run_root, ADAGE)
        self.assertTrue(plan["ready_to_execute"], plan)
        self.assertEqual((plan["completed_runs"], plan["pending_runs"]), (1, 3))
        return frozen.read_json(self.run_root / "state.json")

    def test_prepare_rejects_an_unloadable_index_before_sandbox_generation(self):
        self.index.write_text("{}")
        with patch.object(frozen, "checkout_digest", side_effect=AssertionError("invalid index must fail before checkouts")):
            with self.assertRaisesRegex(ValueError, "not a compiled Adage index"):
                frozen.prepare(self.config, self.audit, self.index, self.objects, self.root / "invalid.json", ADAGE,
                               [self.generator, self.grader], learner_revision="synthetic-learner-v1")
        self.assertFalse((self.root / "invalid.json").exists())

    def test_prepare_rejects_a_checkout_that_cannot_receive_the_frozen_reflexes(self):
        self.generator.write_text(self.generator.read_text().replace("https://fixture/repo", "https://another/repository"))
        with self.assertRaisesRegex(ValueError, "checkout does not resolve to reflex repository"):
            frozen.prepare(self.config, self.audit, self.index, self.objects, self.root / "wrong-repo.json", ADAGE,
                           [self.generator, self.grader], learner_revision="synthetic-learner-v1")
        self.assertFalse((self.root / "wrong-repo.json").exists())

    def test_prepare_run_grade_report_and_resume_cross_the_real_rust_contract(self):
        validated = frozen.adage_json(ADAGE, "validate", self.path, "--objects", self.objects.root)
        self.assertEqual(validated, self.frozen)
        self.assertEqual(self.frozen["protocol"]["intervention"]["reflex_set_sha256"], frozen.file_digest(self.index))
        first = self.execute()
        self.assertEqual(first.returncode, 0, first.stderr)
        report = json.loads(first.stdout)
        self.assertEqual((report["scheduled_runs"], report["submitted_receipts"]), (4, 4))
        self.assertTrue(report["evidence_complete"])
        self.assertTrue(report["paired"]["token_target_accepted"])
        self.assertFalse(report["paired"]["cost_target_accepted"], "estimated CLI costs are not bills")
        self.assertEqual(report["paired"]["tokens"]["baseline"], 2460)
        self.assertEqual(report["paired"]["tokens"]["intervention"], 1844)
        self.assertEqual(len(FakeLive.runs), 4)
        second = self.execute()
        self.assertEqual(first.stdout, second.stdout)
        self.assertEqual(len(FakeLive.runs), 4, "a completed job is never dispatched again")
        for saved in (self.run_root / "receipts").glob("*.json"):
            receipt = self.objects.get(frozen.read_json(saved)["sha256"])
            self.assertEqual(set(receipt["evidence"]), {"accounting", "grade", "execution"})
            self.assertIs(self.objects.get(receipt["evidence"]["grade"])["returncode"], 0)

    def test_incomplete_evidence_stops_dispatch_and_blocks_resume_without_replacing_it(self):
        for mode, reason in (("grader-error", "unknown independent grade"),
                             ("unknown-usage", "incomplete request usage"),
                             ("wrong-model", "unplanned model call")):
            with self.subTest(mode=mode):
                self.run_root = self.root / mode
                FakeLive.runs, FakeLive.mode = [], mode
                result = self.execute()
                self.assertEqual(result.returncode, 2)
                self.assertIn(reason, result.stderr)
                self.assertEqual(len(FakeLive.runs), 1, "stop before buying the remaining jobs")
                report = json.loads(result.stdout)
                self.assertEqual(report["submitted_receipts"], 1)
                self.assertEqual(len(report["missing_jobs"]), 3)
                self.assertFalse(report["evidence_complete"])
                evidence_before = {p: p.read_bytes() for p in self.run_root.rglob("*") if p.is_file()}
                FakeLive.mode = "complete"
                plan = frozen.preflight(self.frozen, self.objects, self.run_root, ADAGE)
                self.assertFalse(plan["ready_to_execute"])
                self.assertTrue(any(item["code"] == "receipt_evidence" for item in plan["blockers"]))
                with self.assertRaisesRegex(ValueError, reason):
                    self.execute()
                self.assertEqual(len(FakeLive.runs), 1, "resume must not replace or skip past invalid evidence")
                self.assertEqual(evidence_before, {p: p.read_bytes() for p in self.run_root.rglob("*") if p.is_file()})

    def test_independently_graded_task_failures_remain_valid_evidence(self):
        FakeLive.mode = "task-failure"
        result = self.execute()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(FakeLive.runs), 4)
        report = json.loads(result.stdout)
        self.assertTrue(report["evidence_complete"])
        self.assertFalse(report["paired"]["token_target_accepted"])

    def test_final_job_overrun_blocks_completed_cohort_readiness(self):
        FakeLive.mode = "last-overrun"
        result = self.execute()
        self.assertEqual(len(FakeLive.runs), 4)
        self.assertEqual(result.returncode, 2)
        self.assertIn("exceeded its reserved cap", result.stderr)
        plan = frozen.preflight(self.frozen, self.objects, self.run_root, ADAGE)
        self.assertEqual(plan["pending_runs"], 0)
        self.assertFalse(plan["ready_to_execute"], "a completed cohort must still stop later paid stages")
        self.assertTrue(any(item["code"] == "accounting" for item in plan["blockers"]))
        original = frozen.read_json(self.run_root / "state.json")
        with self.assertRaisesRegex(ValueError, "exceeded its reserved cap"):
            self.execute()
        self.assertEqual(len(FakeLive.runs), 4)
        self.assertEqual(frozen.read_json(self.run_root / "state.json"), original)

    def test_interrupted_dispatch_is_journaled_and_cannot_spend_again_on_resume(self):
        FakeLive.mode = "interrupted"
        with self.assertRaisesRegex(RuntimeError, "synthetic crash"):
            self.execute()
        self.assertEqual(len(FakeLive.runs), 1)
        report = frozen.read_json(self.run_root / "report.json")
        self.assertEqual(len(report["missing_jobs"]), 4)
        FakeLive.mode = "complete"
        with self.assertRaisesRegex(ValueError, "without rerunning"):
            self.execute()
        self.assertEqual(len(FakeLive.runs), 1)

    def test_changed_grader_inputs_fail_before_any_dispatch(self):
        self.grader.write_text("print('{\"passed\":false}')\n")
        with self.assertRaisesRegex(ValueError, "frozen input changed"):
            self.execute()
        self.assertFalse(FakeLive.runs)
        self.assertFalse(self.run_root.exists())

    def test_changed_initial_checkout_is_rejected_before_agent_dispatch(self):
        original = frozen.checkout_digest
        with patch.object(frozen, "checkout_digest", side_effect=lambda path: "changed-" + original(path)):
            with self.assertRaisesRegex(ValueError, "checkout or grading inputs"):
                self.execute()
        state = frozen.read_json(self.run_root / "state.json")
        self.assertEqual(state["accounting"]["attempts"][0]["status"], "reserved")
        self.assertEqual(len(frozen.read_json(self.run_root / "report.json")["missing_jobs"]), 4)

    def test_origin_drift_after_generation_blocks_both_arms_before_dispatch(self):
        specs = frozen.bindings(self.frozen, self.objects, ADAGE)
        original = frozen.checkout_digest

        def drift(workdir):
            (Path(workdir) / ".git" / "config").write_text('[remote "origin"]\n\turl = https://another/repository\n')
            return original(workdir)  # Git metadata deliberately is not in the file-content digest.

        for side, arm in (("baseline", "off"), ("intervention", "adage")):
            with self.subTest(side=side):
                root = self.root / side
                root.mkdir()
                job = next(job for job in self.frozen["jobs"] if job["side"] == side)
                observer = frozen.Observer(self.frozen, job, specs[side], self.objects, ADAGE)
                live = FakeLive(specs[side]["config"], root, ADAGE, 1, True, False, observer)
                live.home = root / "private" / "reflexes"
                if side == "intervention":
                    frozen.publish_bytes(live.home / "index.json", self.index.read_bytes())
                task = observer.task["identity"]["task_id"]
                with patch.object(frozen, "checkout_digest", side_effect=drift), \
                        patch.object(Budget, "dispatch", side_effect=AssertionError("must not dispatch after origin drift")) as dispatch:
                    with self.assertRaisesRegex(ValueError, "checkout does not resolve to reflex repository"):
                        live.run_one(job["id"], "eval", arm, task, specs[side]["config"]["task_prompts"][task])
                dispatch.assert_not_called()
                self.assertIsNone(observer.observed)
                self.assertEqual(live.budget.data["attempts"][0]["status"], "reserved")

    def test_corrupt_native_or_grade_sources_invalidate_the_rust_report(self):
        self.execute()
        saved = frozen.read_json(next((self.run_root / "receipts").glob("*.json")))
        receipt = self.objects.get(saved["sha256"])
        source = self.objects.root / (receipt["evidence"]["grade"] + ".json")
        source.write_text("{}")
        result = frozen.report(self.path, self.frozen, self.objects, self.run_root, ADAGE)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("checksum mismatch", result.stderr)
        self.assertIs(frozen.read_json(self.run_root / "report.json")["report_valid"], False)

    def test_plan_is_read_only_and_execution_inside_an_agent_is_refused(self):
        args = ["run", str(self.path), "--objects", str(self.objects.root), "--root", str(self.run_root), "--adage", str(ADAGE)]
        with contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(frozen.main(args), 0)
        self.assertIs(json.loads(output.getvalue())["execute"], False)
        self.assertFalse(self.run_root.exists())
        with patch.dict(os.environ, {"CLAUDECODE": "fixture"}):
            with self.assertRaisesRegex(ValueError, "inside an agent"):
                frozen.main(args + ["--execute"])
        self.assertFalse(self.run_root.exists())

    def test_preflight_reports_missing_runtime_prerequisites_without_writes_or_dispatch(self):
        with patch.object(frozen, "confinement_probe", return_value="Bubblewrap confinement is unavailable; enable namespaces"), \
                patch.object(runner, "harness_version", return_value="different-version"), \
                patch.object(runner, "auth_source", return_value=None):
            plan = frozen.preflight(self.frozen, self.objects, self.run_root, ADAGE)
            self.assertFalse(plan["ready_to_execute"])
            self.assertEqual({b["code"] for b in plan["blockers"]}, {"confinement", "harness", "authentication"})
            self.assertEqual((plan["completed_runs"], plan["pending_runs"]), (0, 4))
            self.assertEqual(plan["admission"]["remaining_reservation_usd"], 4)
            self.assertEqual(plan["outputs"]["report"], str(self.run_root / "report.json"))
            args = ["run", str(self.path), "--objects", str(self.objects.root), "--root", str(self.run_root), "--adage", str(ADAGE)]
            with contextlib.redirect_stdout(io.StringIO()) as output:
                self.assertEqual(frozen.main(args), 0)
            self.assertFalse(json.loads(output.getvalue())["ready_to_execute"])
            with self.assertRaisesRegex(ValueError, "not ready"):
                frozen.execute(self.path, self.frozen, self.objects, self.run_root, ADAGE, live_factory=FakeLive)
        self.assertFalse(self.run_root.exists())
        self.assertFalse(FakeLive.runs)

    def test_preflight_detects_study_exposure_before_creating_start_markers(self):
        with patch.dict(os.environ, {"PATH": str(self.root)}):
            with self.assertRaisesRegex(ValueError, "would expose"):
                self.execute()
        self.assertFalse(self.run_root.exists())
        self.assertFalse(FakeLive.runs)

    def test_preflight_reads_journal_counts_and_completed_study_needs_no_auth(self):
        self.execute()
        before = {p: p.read_bytes() for p in self.run_root.rglob("*") if p.is_file()}
        with patch.object(runner, "auth_source", side_effect=AssertionError("offline resume needs no authentication")), \
                patch.object(frozen, "confinement_probe", side_effect=AssertionError("no unfinished jobs")):
            plan = frozen.preflight(self.frozen, self.objects, self.run_root, ADAGE)
        self.assertTrue(plan["ready_to_execute"], plan)
        self.assertEqual((plan["completed_runs"], plan["pending_runs"]), (4, 0))
        self.assertEqual(plan["reflex_set"]["directive_reflexes"], 1)
        self.assertEqual(before, {p: p.read_bytes() for p in self.run_root.rglob("*") if p.is_file()})

    def test_preflight_detects_unresolved_ledger_without_changing_it(self):
        self.run_root.mkdir()
        frozen.bind_root(self.frozen, self.run_root)
        job = self.frozen["jobs"][0]
        frozen.publish(self.run_root / "started" / f"{job['id']}.json",
                       {"protocol_sha256": self.frozen["protocol_sha256"], "job_id": job["id"]})
        state = {"accounting": {"version": 1, "blocked_reason": None, "attempts": [
            {"run": job["id"], "status": "dispatched", "cap_usd": 1, "cost_usd": None}]}}
        frozen.publish(self.run_root / "state.json", state)
        with self.assertRaisesRegex(ValueError, "unresolved accounting"):
            self.execute()
        self.assertEqual(frozen.read_json(self.run_root / "state.json"), state)
        self.assertEqual(len(list((self.run_root / "started").glob("*.json"))), 1)
        self.assertFalse(FakeLive.runs)

    def test_rolling_back_a_valid_ledger_cannot_refund_a_completed_attempt(self):
        state = self.collect_one_job()
        self.assertGreater(state["accounting"]["attempts"][0]["cost_usd"], 0)
        state["accounting"]["attempts"] = []
        frozen.publish(self.run_root / "state.json", state, replace=True)
        with patch.object(Budget, "dispatch", side_effect=AssertionError("rolled-back ledger must not dispatch")) as dispatch:
            with self.assertRaisesRegex(ValueError, "missing original accounting attempt"):
                self.execute()
        dispatch.assert_not_called()
        self.assertEqual(frozen.read_json(self.run_root / "state.json"), state)
        self.assertEqual(len(FakeLive.runs), 1)

    def test_valid_partial_collection_resumes_and_keeps_the_original_attempt(self):
        original = self.collect_one_job()["accounting"]["attempts"][0]
        result = self.execute()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(json.loads(result.stdout)["evidence_complete"])
        attempts = frozen.read_json(self.run_root / "state.json")["accounting"]["attempts"]
        self.assertEqual(len(attempts), 4)
        self.assertEqual(attempts[0], original)
        self.assertEqual(len(FakeLive.runs), 4)

    def test_reservations_and_settled_cost_must_match_the_immutable_receipt(self):
        original = self.collect_one_job()
        cases = (
            ("duplicate", None, "duplicate original accounting attempt"),
            ("run", "unknown-job", "unknown or unjournaled job"),
            ("cap_usd", 0.5, "reservation cap differs"),
            ("cost_usd", 0, "settled cost disagrees"),
            ("status", "cancelled", "nonterminal accounting attempt"),
            ("status", "reserved", "nonterminal accounting attempt"),
            ("status", "dispatched", "nonterminal accounting attempt"),
        )
        for key, value, message in cases:
            with self.subTest(key=key, value=value):
                state = json.loads(json.dumps(original))
                attempts = state["accounting"]["attempts"]
                if key == "duplicate":
                    attempts.append(dict(attempts[0]))
                else:
                    attempts[0][key] = value
                frozen.publish(self.run_root / "state.json", state, replace=True)
                with patch.object(Budget, "dispatch", side_effect=AssertionError("invalid ledger must not dispatch")) as dispatch:
                    with self.assertRaisesRegex(ValueError, message):
                        self.execute()
                dispatch.assert_not_called()
                self.assertEqual(frozen.read_json(self.run_root / "state.json"), state)
                self.assertEqual(len(FakeLive.runs), 1)

    def test_explicit_conservative_cap_resolution_preserves_the_original_reservation(self):
        state = self.collect_one_job()
        attempt = state["accounting"]["attempts"][0]
        attempt.update(status="unknown", cost_usd=None)
        ledger = Budget(state, 4, 1)
        ledger.resolve_unknown_at_cap(attempt["run"], {
            "kind": "human_cap_charge", "approved": True, "run": attempt["run"], "cap_usd": 1,
            "statement": "Synthetic fixture authorization to reserve the original full cap", "source": "unit-test",
        })
        frozen.publish(self.run_root / "state.json", state, replace=True)
        plan = frozen.preflight(self.frozen, self.objects, self.run_root, ADAGE)
        self.assertTrue(plan["ready_to_execute"], plan)
        self.assertEqual(plan["admission"]["accounted_usd"], 1)
        self.assertEqual(plan["admission"]["remaining_reservation_usd"], 3)
        attempt["resolution"]["authorization"]["approved"] = False
        frozen.publish(self.run_root / "state.json", state, replace=True)
        with self.assertRaisesRegex(ValueError, "invalid conservative cap charge"):
            self.execute()
        self.assertEqual(len(FakeLive.runs), 1)

    def test_missing_native_cost_cannot_be_rewritten_as_a_free_settled_attempt(self):
        FakeLive.mode = "unknown-cost"
        self.assertEqual(self.execute().returncode, 2)
        original = frozen.read_json(self.run_root / "state.json")
        self.assertEqual(len(original["accounting"]["attempts"]), 1)
        self.assertEqual(original["accounting"]["attempts"][0]["status"], "unknown")
        plan = frozen.preflight(self.frozen, self.objects, self.run_root, ADAGE)
        self.assertFalse(plan["ready_to_execute"])
        self.assertTrue(any(blocker["code"] == "accounting" for blocker in plan["blockers"]))

        forged = json.loads(json.dumps(original))
        forged["accounting"]["attempts"][0].update(status="settled", cost_usd=0)
        forged["accounting"]["blocked_reason"] = None
        frozen.publish(self.run_root / "state.json", forged, replace=True)
        with patch.object(Budget, "dispatch", side_effect=AssertionError("unknown native cost must not become free")) as dispatch:
            with self.assertRaisesRegex(ValueError, "requires a known immutable native accounting estimate"):
                self.execute()
        dispatch.assert_not_called()
        self.assertEqual(frozen.read_json(self.run_root / "state.json"), forged)
        self.assertEqual(len(FakeLive.runs), 1)

        ledger = Budget(original, 4, 1)
        attempt = original["accounting"]["attempts"][0]
        ledger.resolve_unknown_at_cap(attempt["run"], {
            "kind": "human_cap_charge", "approved": True, "run": attempt["run"], "cap_usd": 1,
            "statement": "Synthetic fixture authorization to charge the original full cap", "source": "unit-test",
        })
        frozen.publish(self.run_root / "state.json", original, replace=True)
        plan = frozen.preflight(self.frozen, self.objects, self.run_root, ADAGE)
        self.assertTrue(plan["ready_to_execute"], plan)
        self.assertEqual(plan["admission"]["accounted_usd"], 1)
        self.assertEqual(plan["admission"]["remaining_reservation_usd"], 3)
        self.assertEqual(len(FakeLive.runs), 1)

    def test_missing_ledger_cannot_reset_admission_after_a_job_was_started(self):
        frozen.bind_root(self.frozen, self.run_root)
        self.assertTrue(frozen.preflight(self.frozen, self.objects, self.run_root, ADAGE)["ready_to_execute"])
        job = self.frozen["jobs"][0]
        frozen.publish(self.run_root / "started" / f"{job['id']}.json",
                       {"protocol_sha256": self.frozen["protocol_sha256"], "job_id": job["id"]})
        plan = frozen.preflight(self.frozen, self.objects, self.run_root, ADAGE)
        self.assertFalse(plan["ready_to_execute"])
        self.assertIsNone(plan["admission"])
        self.assertTrue(any(b["code"] == "accounting" and "ledger is missing" in b["message"] for b in plan["blockers"]))
        with self.assertRaisesRegex(ValueError, "ledger is missing"):
            self.execute()
        self.assertFalse((self.run_root / "state.json").exists())
        self.assertFalse(FakeLive.runs)
        frozen.publish(self.run_root / "state.json", {})
        with self.assertRaisesRegex(ValueError, "ledger is missing"):
            self.execute()
        self.assertEqual(frozen.read_json(self.run_root / "state.json"), {})
        self.assertFalse(FakeLive.runs)

    def test_missing_ledger_with_receipts_blocks_resume_but_offline_report_remains_available(self):
        first = self.execute()
        (self.run_root / "state.json").unlink()
        # Receipts alone must retain the accounting requirement, even if start markers were lost.
        for path in (self.run_root / "started").glob("*.json"):
            path.unlink()
        with self.assertRaisesRegex(ValueError, "ledger is missing"):
            self.execute()
        self.assertEqual(len(FakeLive.runs), 4)
        self.assertFalse((self.run_root / "state.json").exists())
        report = frozen.report(self.path, self.frozen, self.objects, self.run_root, ADAGE)
        self.assertEqual(report.returncode, 0, report.stderr)
        self.assertEqual(report.stdout, first.stdout)

    def test_existing_historical_output_cannot_be_registered_as_a_new_study(self):
        self.run_root.mkdir()
        (self.run_root / "state.json").write_text('{"historical":true}')
        with self.assertRaisesRegex(ValueError, "historical runs"):
            self.execute()
        self.assertFalse(FakeLive.runs)

    def test_study_evidence_cannot_be_visible_outside_the_hidden_roots(self):
        exposed = Path("/usr/adage-frozen-fixture")
        for objects, root in ((self.objects, exposed), (frozen.Objects(exposed), self.run_root)):
            with self.subTest(root=root, objects=objects.root):
                with self.assertRaisesRegex(ValueError, "hidden directories"):
                    frozen.execute(self.path, self.frozen, objects, root, ADAGE, live_factory=FakeLive)
        self.assertFalse(FakeLive.runs)
        self.assertFalse(self.run_root.exists())


class LocalExecution(unittest.TestCase):
    def test_confinement_probe_checks_installation_and_namespace_failure_without_an_agent(self):
        with patch.object(runner.shutil, "which", return_value=None):
            self.assertIn("not installed", frozen.confinement_probe())
        with patch.object(runner.shutil, "which", return_value="/usr/bin/bwrap"), \
                patch.object(frozen.subprocess, "run", return_value=subprocess.CompletedProcess([], 1, "", "not permitted")) as probe:
            self.assertIn("not permitted", frozen.confinement_probe())
            self.assertEqual(probe.call_args.args[0][-1], "/bin/true")
        with patch.object(runner.shutil, "which", return_value="/usr/bin/bwrap"), \
                patch.object(frozen.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, "", "")):
            self.assertIsNone(frozen.confinement_probe())

    def test_grader_exit_status_type_and_timeout_are_checked_in_the_actual_runner(self):
        live = object.__new__(runner.Live)
        live.observer = None
        for program in ("print('{\"passed\":true}');raise SystemExit(1)",
                        "print('{\"passed\":\"false\"}')", "import time;time.sleep(30)"):
            live.cfg = {"score": [sys.executable, "-c", program.replace("{", "{{").replace("}", "}}") ]}
            score, usable = live.score(Path.cwd(), "fixture", timeout=0.02)
            self.assertFalse(usable)
            self.assertNotIn("passed", score)

    def test_native_usage_flag_and_explicit_settings_are_in_the_real_command(self):
        cfg = {**runner.DEFAULTS, "capture_usage": True, "setting_sources": ""}
        argv = runner.claude_command(cfg, "fixture", Path("settings.json"))
        self.assertIn("--include-partial-messages", argv)
        self.assertEqual(argv[argv.index("--setting-sources") + 1], "")

    @unittest.skipUnless(os.name == "posix", "runner deadlines terminate POSIX process groups")
    def test_workflow_deadline_stops_a_silent_process_and_keeps_unknown_spend(self):
        self.assert_deadline_stops("import time; time.sleep(30)")

    @unittest.skipUnless(os.name == "posix", "runner deadlines terminate POSIX process groups")
    def test_workflow_deadline_stops_children_after_the_harness_parent_exits(self):
        self.assert_deadline_stops("import subprocess,sys; subprocess.Popen([sys.executable,'-c','import time;time.sleep(30)'])")

    def assert_deadline_stops(self, program):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            generator = root / "generate.py"
            generator.write_text("import pathlib,sys\npathlib.Path(sys.argv[1]).mkdir()\n")
            grader = root / "grade.py"
            grader.write_text("print('{\"passed\":true}')\n")
            cfg = {**runner.DEFAULTS, "name": "deadline-fixture", "workdir": "checkout", "seed": 1,
                   "eval": {"tasks": ["TEST"]}, "budget_usd": 1, "session_budget_usd": 1,
                   "sandbox": [sys.executable, str(generator), "{dir}"], "score": [sys.executable, str(grader)],
                   "timeout_seconds": 0.3}
            command = [sys.executable, "-c", program]
            with patch.object(runner, "harness_version", return_value=VERSION), patch.object(runner, "auth_source", return_value=None):
                live = runner.Live(cfg, root, ADAGE, 1)
            began = time.monotonic()
            with patch.object(runner, "claude_command", return_value=command), contextlib.redirect_stdout(io.StringIO()):
                live.run_one("deadline-job", "eval", "off", "TEST", "fixture")
            self.assertLess(time.monotonic() - began, 10)
            self.assertTrue(live.state["runs"]["deadline-job"]["metrics"]["timed_out"])
            self.assertIsNotNone(live.budget.data["blocked_reason"])
            self.assertEqual(live.budget.data["attempts"][0]["status"], "unknown")

    def test_failed_preflight_never_copies_credentials_or_dispatches_the_agent(self):
        from unittest.mock import Mock
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            cfg = {**runner.DEFAULTS, "name": "preflight-fixture", "workdir": "checkout", "seed": 1,
                   "eval": {"tasks": ["TEST"]}, "budget_usd": 1, "session_budget_usd": 1,
                   "sandbox": [sys.executable, "-c", "import pathlib,sys;pathlib.Path(sys.argv[1]).mkdir()", "{dir}"]}
            observer = Mock()
            observer.prepared.side_effect = ValueError("changed frozen input")
            with patch.object(runner, "harness_version", return_value=VERSION), patch.object(runner, "auth_source", return_value=None):
                live = runner.Live(cfg, root, ADAGE, 1, observer=observer)
            with patch.object(live, "prepare_home") as prepare_home:
                with self.assertRaisesRegex(ValueError, "changed frozen input"):
                    live.run_one("preflight-job", "eval", "off", "TEST", "fixture")
            prepare_home.assert_not_called()
            self.assertEqual(live.budget.data["attempts"][0]["status"], "cancelled")


if __name__ == "__main__":
    unittest.main()
