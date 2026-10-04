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
            message = row.get("message") or row.get("event", {}).get("message")
            if message:
                message["id"] += "-" + run_id
                message["model"] = self.cfg["model"]
            if "request_id" in row:
                row["request_id"] += "-" + run_id
            if "modelUsage" in row:
                row["modelUsage"] = {self.cfg["model"]: next(iter(row["modelUsage"].values()))}
            if arm == "adage":
                usages = [row.get("usage"), (message or {}).get("usage"), row.get("event", {}).get("usage")]
                usages += list(row.get("modelUsage", {}).values())
                for usage in filter(None, usages):
                    for key in (*frozen.evidence.COUNTERS, "inputTokens", "outputTokens", "cacheReadInputTokens", "cacheCreationInputTokens"):
                        if key in usage:
                            usage[key] = usage[key] * 3 // 4
            self.observer.event(row)
        (directory / "stream.jsonl").write_text("\n".join(json.dumps(r) for r in events) + "\n")
        self.observer.graded('{"passed":true}', 1 if self.mode == "grader-error" else 0, False)
        self.state["runs"][run_id] = {"metrics": {"exit_code": 0, "timed_out": False}}
        self.budget.settle(attempt, events[-1])
        self.save()


@unittest.skipUnless(sys.platform == "linux", "frozen execution uses the Linux confinement profile")
class FrozenFlow(unittest.TestCase):
    def setUp(self):
        self.assertTrue(ADAGE.is_file(), "build the CLI first: cargo build --locked -p adage-cli")
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.objects = frozen.Objects(self.root / "objects")
        self.generator = self.root / "generate.py"
        self.generator.write_text("import pathlib,sys\np=pathlib.Path(sys.argv[1]);p.mkdir()\n(p/'source.txt').write_text('fixed task input')\n")
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
        self.index.write_text('{ "synthetic_fixture": true, "reflexes": [] }\n')
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

    def test_grader_errors_are_unknown_and_are_never_replaced_by_a_better_retry(self):
        FakeLive.mode = "grader-error"
        result = self.execute()
        self.assertEqual(result.returncode, 2)
        report = json.loads(result.stdout)
        self.assertEqual(report["paired"]["unknown_outcomes"], 2)
        self.assertFalse(report["paired"]["token_target_accepted"])
        FakeLive.mode = "complete"
        self.execute()
        self.assertEqual(len(FakeLive.runs), 4)
        self.assertEqual(frozen.read_json(self.run_root / "report.json")["paired"]["unknown_outcomes"], 2)

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
