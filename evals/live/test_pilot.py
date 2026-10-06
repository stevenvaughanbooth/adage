"""Paid-pilot coordination using actual checkouts, graders and Rust learning, with no models."""
import copy
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import frozen
import pilot
import runner
from budget import Budget

ADAGE = runner.REPO / "target" / "debug" / "adage"
VERSION = "fixture-1 (Claude Code)"


def gold_actions(work, task):
    """A disclosed test oracle; these are actual edits/commands, never paid evidence."""
    spec = pilot.subprocess_json([sys.executable, "-c",
        "import json,sys; from cases import task; print(json.dumps(task(sys.argv[1])))", task], cwd=pilot.PACK)
    name = "docs/guide.md" if spec["kind"] == "documentation" else "src/solution.py"
    path = work / name
    before, after = path.read_text(), spec["doc_after"] if spec["kind"] == "documentation" else spec["reference"]
    path.write_text(after)
    actions = [("Edit", {"file_path": str(path), "old_string": before, "new_string": after}, "File updated.", False)]
    if spec["kind"] == "code":
        for command in ("make generate", "make check"):
            result = subprocess.run(command.split(), cwd=work, capture_output=True, text=True, check=True)
            actions.append(("Bash", {"command": command}, result.stdout, False))
    return actions


class TrainingLive:
    """Reads README without repairing the task; real external grading records the failure."""
    runs = []
    mode = "complete"

    def __init__(self, cfg, root, adage, round_, isolate, resume, observer):
        self.cfg, self.root, self.adage, self.observer, self.isolate = cfg, root, adage, observer, isolate
        self.state = frozen.read_json(root / "state.json") if (root / "state.json").exists() else {
            "runs": {}, "harness_version": VERSION}
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
        subprocess.run(runner.expand(self.cfg["sandbox"], dir=str(work), task=task, arm=arm),
                       check=True, capture_output=True)
        settings = directory / "settings.json"
        settings.write_text("{}")
        env = runner.session_env(arm, directory / "home", getattr(self, "home", directory / "adage-home"), True, {})
        self.observer.prepared(self, run_id, task, arm, work, settings, env)
        assert env["ADAGE_AUTO_LEARN"] == "0" and env["ADAGE_DISABLE"] == ("0" if arm == "adage" else "1")
        assert self.budget.dispatch(attempt)
        if self.mode == "interrupted":
            self.budget.settle(attempt)
            raise RuntimeError("synthetic interruption after dispatch")
        actions = gold_actions(work, task) if self.mode == "gold" else []
        events = [json.loads(row) for row in (HERE / "fixtures" / "claude-usage.jsonl").read_text().splitlines()]
        for row in events:
            row["session_id"] = run_id
            message = row.get("message") or row.get("event", {}).get("message")
            if message:
                message["id"] += "-" + run_id
                message["model"] = self.cfg["model"]
            if "request_id" in row:
                row["request_id"] += "-" + run_id
            if "modelUsage" in row:
                row["modelUsage"] = {self.cfg["model"]: next(iter(row["modelUsage"].values()))}
            if self.mode == "unknown-cost" and row.get("type") == "result":
                row.pop("total_cost_usd")
            self.observer.event(row)
        if self.mode == "unknown-usage":
            events = [row for row in events if row.get("type") in ("system", "result")]
        (directory / "stream.jsonl").write_text("\n".join(json.dumps(row) for row in events) + "\n")
        grade = subprocess.run(runner.expand(self.cfg["score"], dir=str(work), task=task, arm=arm),
                               capture_output=True, text=True, check=False)
        self.observer.graded(grade.stdout, 1 if self.mode == "unknown-grade" else grade.returncode, False)
        archive = self.root / "private" / "transcripts" / "project"
        archive.mkdir(parents=True, exist_ok=True)
        native = [
            {"type": "user", "sessionId": run_id, "cwd": str(work), "timestamp": "2026-10-04T10:00:00Z",
             "message": {"role": "user", "content": (work / "TASK.md").read_text()}},
            {"type": "assistant", "sessionId": run_id, "cwd": str(work), "message": {
                "id": "message-read", "role": "assistant", "model": self.cfg["model"], "content": [
                    {"type": "tool_use", "id": "read1", "name": "Read", "input": {"file_path": "README.md"}}]}},
            {"type": "user", "sessionId": run_id, "cwd": str(work), "message": {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "read1", "content": (work / "README.md").read_text()}]}},
            {"type": "assistant", "sessionId": run_id, "cwd": str(work), "message": {
                "id": "message-final", "role": "assistant", "content": [{"type": "text", "text":
                    "Synthetic gold repair completed." if self.mode == "gold" else "Synthetic collector left task unfinished."}]}},
        ]
        for i, (tool, input_, output, failed) in enumerate(actions):
            native[-1:-1] = [
                {"type": "assistant", "sessionId": run_id, "cwd": str(work), "message": {
                    "id": f"message-gold-{i}", "role": "assistant", "model": self.cfg["model"], "content": [
                        {"type": "tool_use", "id": f"gold-{i}", "name": tool, "input": input_}]}},
                {"type": "user", "sessionId": run_id, "cwd": str(work), "message": {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": f"gold-{i}", "content": output, "is_error": failed}]}}
            ]
        if self.mode != "missing-history":
            body = "malformed native history\n" if self.mode == "malformed-history" else "\n".join(json.dumps(row) for row in native) + "\n"
            (archive / f"{run_id}.jsonl").write_text(body)
        self.state["runs"][run_id] = {"phase": phase, "arm": arm, "task": task,
            "status": "passed" if json.loads(grade.stdout)["passed"] else "failed",
            "run_status": "scorer_error" if self.mode == "unknown-grade" else "ok",
            "metrics": {"session_id": run_id, "exit_code": 0, "timed_out": False},
            "score": {} if self.mode == "unknown-grade" else json.loads(grade.stdout)}
        self.budget.settle(attempt, events[-1])
        self.save()


class PilotFlow(unittest.TestCase):
    def setUp(self):
        self.assertTrue(ADAGE.is_file(), "build first: cargo build --locked -p adage-cli")
        for owner, name, value in ((runner, "harness_version", VERSION),
                                   (runner, "auth_source", ("env", ["fixture-auth"])),
                                   (frozen, "confinement_probe", None)):
            probe = patch.object(owner, name, return_value=value)
            probe.start()
            self.addCleanup(probe.stop)
        env = patch.dict(os.environ, {key: "" for key in pilot.AGENT_MARKERS})
        env.start()
        self.addCleanup(env.stop)
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "pilot"
        self.manifest = pilot.initialize(self.root, ADAGE, "fixture-model-v1", "30", "1")
        TrainingLive.runs, TrainingLive.mode = [], "complete"

    def collect_one(self):
        def stop_between_jobs(*args, **kwargs):
            if TrainingLive.runs:
                raise RuntimeError("fixture stop between jobs")
            return TrainingLive(*args, **kwargs)
        with self.assertRaisesRegex(RuntimeError, "fixture stop between jobs"):
            pilot.train(self.root, execute=True, live_factory=stop_between_jobs)

    def test_generated_schedule_uses_honest_groups_fixed_total_and_correct_commands(self):
        self.assertEqual(self.manifest["counts"], {"train": 20, "validation": 4, "test": 6})
        self.assertEqual(pilot.plan(self.root)["maximum_reserved_usd"], 30)
        self.assertEqual(len(self.manifest["selected"]["validation"]), 2)
        groups = {stage: {self.manifest["registry"]["tasks"][task]["group_id"]
                          for task in tasks} for stage, tasks in self.manifest["selected"].items()}
        self.assertEqual([len(groups[s]) for s in pilot.STAGES], [10, 2, 3])
        self.assertFalse(groups["train"] & groups["test"])
        cfg = self.manifest["configs"]["train"]
        self.assertEqual(runner.classify(cfg, "Bash", {"command": "make check"}), "verify")
        self.assertEqual(runner.classify(cfg, "Bash", {"command": "make generate"}), "regen")
        self.assertEqual(cfg["obligation"]["paths"], "src/*.py")
        other = self.root.parent / "too-small"
        with self.assertRaisesRegex(ValueError, "30 scheduled runs"):
            pilot.initialize(other, ADAGE, "fixture-model-v1", "29", "1")
        self.assertFalse(other.exists())

    def test_plan_and_default_train_are_read_only_and_no_receipts_are_not_complete_usage(self):
        before = sorted((str(p.relative_to(self.root)), p.read_bytes()) for p in self.root.rglob("*") if p.is_file())
        with patch.object(runner, "auth_source", return_value=None):
            result = pilot.train(self.root, live_factory=lambda *a, **kw: self.fail("no execution authorized"))
        self.assertFalse(result["ready_for_paid_dispatch"])
        self.assertTrue(any("authentication" in reason for reason in result["blockers"]))
        after = sorted((str(p.relative_to(self.root)), p.read_bytes()) for p in self.root.rglob("*") if p.is_file())
        self.assertEqual(before, after)
        self.assertTrue(all(not stage["request_usage_complete"] for stage in pilot.report(self.root)["usage"].values()))

    def test_native_training_resume_real_export_and_production_learning_stop_without_candidates(self):
        self.collect_one()
        first = TrainingLive.runs[0]
        saved = (self.root / "train" / "receipts" / f"{first}.json").read_bytes()
        result = pilot.train(self.root, execute=True, live_factory=TrainingLive)
        self.assertEqual(result["returncode"], 0, result)
        self.assertEqual(len(TrainingLive.runs), 20)
        self.assertEqual((self.root / "train" / "receipts" / f"{first}.json").read_bytes(), saved)
        pilot.train(self.root, execute=True, live_factory=TrainingLive)
        self.assertEqual(len(TrainingLive.runs), 20)
        learned = pilot.learn(self.root)
        self.assertEqual(learned, pilot.learn(self.root))
        exported = frozen.read_json(self.root / "training-bundle" / "report.json")
        self.assertEqual((exported["sessions"], exported["training_groups"]), (20, 10))
        self.assertEqual(exported["recorded_failures"], 20)
        self.assertEqual(learned["candidate_count"], 0, "unfinished tasks must not fabricate learned value")
        with self.assertRaisesRegex(ValueError, "no enabled reflexes"):
            pilot.freeze(self.root, "fixture reviewer")
        self.assertFalse((self.root / "validation-frozen.json").exists())
        usage = pilot.report(self.root)["usage"]["train"]
        self.assertEqual(usage["recorded_model_calls"], 40)
        self.assertTrue(usage["request_usage_complete"])
        self.assertAlmostEqual(usage["known_cli_estimate_usd"], 20 * 0.123)
        self.assertEqual(usage["unknown_cli_estimates"], 0)

    def test_scripted_positive_trace_learning_review_freeze_and_paired_report(self):
        TrainingLive.mode = "gold"
        self.assertEqual(pilot.train(self.root, execute=True, live_factory=TrainingLive)["returncode"], 0)
        learned = pilot.learn(self.root)
        self.assertGreater(learned["candidate_count"], 0, learned)
        objects = frozen.Objects(self.root / "objects")
        routes = objects.get(learned["routes_sha256"])
        # Explicitly simulate the operator at the review boundary inside this isolated test.
        # Production pilot.py never invokes approve and never clears agent-session markers.
        for route in routes:
            if route["state"] == "nascent":
                approved = subprocess.run([str(ADAGE), "review", "approve", route["reference"], route["id"], "--by", "fixture-reviewer"],
                    env=dict(os.environ, ADAGE_HOME=str(self.root / "learner-home"), ADAGE_ALLOW_NONINTERACTIVE="1"),
                    capture_output=True, text=True, check=False)
                self.assertEqual(approved.returncode, 0, approved.stderr)
        readiness = pilot.freeze(self.root, "synthetic test operator")
        self.assertTrue(readiness["ready_for_paid_dispatch"], readiness)
        real_execute = frozen.execute
        def scripted_execute(*args, **kwargs):
            return real_execute(*args, **kwargs, live_factory=TrainingLive)
        with patch.object(frozen, "execute", side_effect=scripted_execute):
            validation = pilot.run(self.root, "validation", execute=True)
            self.assertEqual(validation["returncode"], 0, validation)
            test = pilot.run(self.root, "test", execute=True)
            self.assertEqual(test["returncode"], 0, test)
            pilot.run(self.root, "test", execute=True)
        self.assertEqual(len(TrainingLive.runs), 30, "resume cannot buy a better result")
        result = pilot.report(self.root)
        self.assertFalse(result["customer_savings_claim"])
        self.assertIsNone(result["billed_cost_usd"])
        for stage in pilot.STAGES[1:]:
            report = result["stages"][stage]["report"]
            self.assertTrue(report["evidence_complete"], report)
            self.assertFalse(report["paired"]["token_target_accepted"], "scripted equal usage proves no savings")
            self.assertTrue(result["usage"][stage]["request_usage_complete"])

    def test_interruption_preserves_reservation_and_never_redispatches(self):
        TrainingLive.mode = "interrupted"
        with self.assertRaisesRegex(RuntimeError, "synthetic interruption"):
            pilot.train(self.root, execute=True, live_factory=TrainingLive)
        result = pilot.plan(self.root)
        self.assertFalse(result["ready_for_paid_dispatch"])
        self.assertTrue(any("without a receipt" in reason for reason in result["blockers"]))
        with self.assertRaisesRegex(ValueError, "not ready"):
            pilot.train(self.root, execute=True, live_factory=TrainingLive)
        self.assertEqual(len(TrainingLive.runs), 1)
        state = frozen.read_json(self.root / "train" / "state.json")
        self.assertEqual(state["accounting"]["attempts"][0]["status"], "unknown")

    def test_unknown_grade_or_usage_or_cost_retains_attempt_and_stops_spending(self):
        for mode in ("unknown-grade", "unknown-usage", "unknown-cost"):
            with self.subTest(mode=mode):
                root = self.root.parent / mode
                pilot.initialize(root, ADAGE, "fixture-model-v1", "30", "1")
                TrainingLive.runs, TrainingLive.mode = [], mode
                result = pilot.train(root, execute=True, live_factory=TrainingLive)
                self.assertEqual(result["returncode"], 2)
                self.assertEqual(len(TrainingLive.runs), 1)
                self.assertEqual(len(list((root / "train" / "receipts").glob("*.json"))), 1)
                self.assertFalse(result["ready_for_paid_dispatch"])
                with self.assertRaisesRegex(ValueError, "not ready"):
                    pilot.train(root, execute=True, live_factory=TrainingLive)

    def test_missing_or_invalid_native_history_retains_receipt_and_stops_spending(self):
        for mode in ("missing-history", "malformed-history"):
            with self.subTest(mode=mode):
                root = self.root.parent / mode
                pilot.initialize(root, ADAGE, "fixture-model-v1", "30", "1")
                TrainingLive.runs, TrainingLive.mode = [], mode
                result = pilot.train(root, execute=True, live_factory=TrainingLive)
                self.assertEqual(result["returncode"], 2)
                self.assertEqual(len(TrainingLive.runs), 1)
                self.assertTrue(any("incomplete native histories" in reason for reason in result["blockers"]))
                pointers = list((root / "train" / "receipts").glob("*.json"))
                self.assertEqual(len(pointers), 1, "preserve the failed original evidence receipt")
                objects = frozen.Objects(root / "objects")
                receipt = objects.get(frozen.read_json(pointers[0])["sha256"])
                sources = objects.get(receipt["evidence"]["sources"])
                self.assertFalse(sources["complete"])
                before = pointers[0].read_bytes()
                TrainingLive.mode = "complete"
                with self.assertRaisesRegex(ValueError, "incomplete native histories"):
                    pilot.train(root, execute=True, live_factory=TrainingLive)
                self.assertEqual(len(TrainingLive.runs), 1)
                self.assertEqual(pointers[0].read_bytes(), before)

    def test_native_history_and_stream_are_pinned_and_must_survive_resume(self):
        self.collect_one()
        objects = frozen.Objects(self.root / "objects")
        pointer = next((self.root / "train" / "receipts").glob("*.json"))
        receipt = objects.get(frozen.read_json(pointer)["sha256"])
        sources = objects.get(receipt["evidence"]["sources"])
        self.assertTrue(sources["complete"])
        self.assertEqual(len(sources["sources"]), 2)
        original_receipt = pointer.read_bytes()
        for name, sha in sources["sources"].items():
            path = Path(name)
            original = path.read_bytes()
            self.assertEqual(frozen.file_digest(path), sha)
            for mutation in ("change", "remove"):
                with self.subTest(path=path.name, mutation=mutation):
                    if mutation == "change":
                        path.write_bytes(original + b"\n")
                    else:
                        path.unlink()
                    try:
                        readiness = pilot.plan(self.root)
                        self.assertFalse(readiness["ready_for_paid_dispatch"])
                        self.assertTrue(any("immutable receipt" in reason for reason in readiness["blockers"]))
                        with self.assertRaisesRegex(ValueError, "immutable receipt"):
                            pilot.train(self.root, execute=True, live_factory=TrainingLive)
                        self.assertEqual(len(TrainingLive.runs), 1)
                        self.assertEqual(pointer.read_bytes(), original_receipt)
                    finally:
                        path.write_bytes(original)
            self.assertTrue(pilot.plan(self.root)["ready_for_paid_dispatch"])

    def test_journal_rejects_budget_rollback_and_forged_zero_cost(self):
        self.collect_one()
        state_path = self.root / "train" / "state.json"
        original = frozen.read_json(state_path)
        for mode in ("missing-ledger", "missing-attempt", "duplicate", "cost"):
            with self.subTest(mode=mode):
                state = copy.deepcopy(original)
                if mode == "missing-ledger":
                    del state["accounting"]
                elif mode == "missing-attempt":
                    state["accounting"]["attempts"] = []
                elif mode == "duplicate":
                    state["accounting"]["attempts"] *= 2
                else:
                    state["accounting"]["attempts"][0]["cost_usd"] = 0
                state_path.write_text(json.dumps(state))
                self.assertFalse(pilot.plan(self.root)["ready_for_paid_dispatch"])
        state_path.write_text(json.dumps(original))
        self.assertTrue(pilot.plan(self.root)["ready_for_paid_dispatch"])

    def test_cross_split_group_alias_is_rejected(self):
        registry = copy.deepcopy(self.manifest["registry"])
        train_group = next(row["group_id"] for row in registry["tasks"].values() if row["split"] == "train")
        next(row for row in registry["tasks"].values() if row["split"] == "test")["group_id"] = train_group
        with self.assertRaisesRegex(ValueError, "disjoint"):
            pilot.selected_tasks(registry)

    def test_untouched_test_requires_complete_validation_evidence_not_savings_acceptance(self):
        for stage in pilot.STAGES[1:]:
            (self.root / f"{stage}-frozen.json").write_text("{}")
        readiness = {"blockers": [], "stages": {"validation": {"pending_runs": 0}}}
        with patch.object(pilot, "plan", return_value=readiness), patch.object(frozen, "adage_json", return_value={}), \
                patch.object(frozen, "execute") as execute, patch.object(frozen, "report") as measured:
            measured.return_value = subprocess.CompletedProcess([], 0, '{"evidence_complete":false}', "")
            with self.assertRaisesRegex(ValueError, "validation evidence is incomplete"):
                pilot.run(self.root, "test", execute=True)
            execute.assert_not_called()
            measured.return_value = subprocess.CompletedProcess([], 0,
                '{"evidence_complete":true,"paired":{"token_target_accepted":false}}', "")
            execute.return_value = subprocess.CompletedProcess([], 0, '{"evidence_complete":true}', "")
            self.assertEqual(pilot.run(self.root, "test", execute=True)["returncode"], 0)
            execute.assert_called_once()


class Lineage(unittest.TestCase):
    def setUp(self):
        self.repo = "github.com/adage-evals/pilot"
        route = {"id": "verify", "authored": False, "state": "nascent", "strength": "prime",
                 "step": {"run": "make check"}, "slots": {"path": "src/*.py"}, "evidence": {"support": 8}}
        self.original = {"tracks": [{"manifest": {"scope": {"repo": self.repo}, "track": {"name": "pilot/check"}},
                                     "routes": [route]}], "index": {"routes": []}}
        self.current = copy.deepcopy(self.original)
        self.current["tracks"][0]["routes"][0]["state"] = "firm"
        self.index = {"routes": [{"repo": self.repo, "track": "pilot/check",
                                   "route": self.current["tracks"][0]["routes"][0]}]}
        self.current["index"] = copy.deepcopy(self.index)

    def test_normal_review_preserves_full_learned_payload(self):
        pilot.check_lineage(self.original, self.current, self.index, self.repo)

    def test_same_id_tampered_compiled_step_is_rejected(self):
        self.index["routes"][0]["route"]["step"] = {"run": "echo authored"}
        with self.assertRaisesRegex(ValueError, "canonical reviewed source"):
            pilot.check_lineage(self.original, self.current, self.index, self.repo)

    def test_changed_source_slots_or_evidence_cannot_be_recompiled_into_learned_treatment(self):
        for key, value in (("slots", {"path": "private/*.json"}), ("evidence", {"support": 99}), ("authored", True)):
            with self.subTest(key=key):
                current = copy.deepcopy(self.current)
                current["tracks"][0]["routes"][0][key] = value
                current["index"]["routes"][0]["route"][key] = value
                with self.assertRaisesRegex(ValueError, "outside this pilot"):
                    pilot.check_lineage(self.original, current, current["index"], self.repo)


if __name__ == "__main__":
    unittest.main()
