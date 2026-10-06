"""Real native decoding and canonical audits, without models or authored reflexes."""
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
import training

ADAGE = HERE.parent.parent / "target" / "debug" / ("adage.exe" if os.name == "nt" else "adage")


class TrainingExport(unittest.TestCase):
    def setUp(self):
        self.assertTrue(ADAGE.is_file(), "build first: cargo build --locked -p adage-cli")
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.root = self.base / "train"
        self.archive = self.root / "private" / "transcripts" / "project"
        self.archive.mkdir(parents=True)
        self.registry = self.base / "registry.json"
        self.tasks = {"T1": {"split": "train", "group_id": "family-one"},
                      "T2": {"split": "train", "group_id": "family-two"},
                      "T3": {"split": "train", "group_id": "family-three"},
                      "E1": {"split": "test", "group_id": "unseen-family"}}
        self.state = {"runs": {}}
        self.add_run("one", "T1", True)
        self.add_run("two", "T2", False)
        self.add_run("three", "T3", None)
        self.save()

    def add_run(self, sid, task, passed):
        self.state["runs"]["train-" + sid] = {
            "phase": "train", "arm": "train", "task": task,
            "status": "passed" if passed else "failed", "run_status": "scorer_error" if passed is None else "ok",
            "metrics": {"session_id": sid, "passed": False},
            "score": {} if passed is None else {"passed": passed}}
        rows = [
            {"type": "user", "sessionId": sid, "cwd": "/fixture/repo", "timestamp": "2026-10-04T10:00:00Z",
             "message": {"role": "user", "content": "Repair " + task}},
            {"type": "assistant", "sessionId": sid, "cwd": "/fixture/repo", "message": {
                "role": "assistant", "model": "scripted-test", "content": [
                    {"type": "tool_use", "id": "call1", "name": "Bash", "input": {"command": "make check"}}]}},
            {"type": "user", "sessionId": sid, "cwd": "/fixture/repo", "message": {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "call1", "content": "check finished", "is_error": passed is False}]}},
            {"type": "assistant", "sessionId": sid, "cwd": "/fixture/repo", "message": {
                "role": "assistant", "content": "Done."}},
        ]
        (self.archive / f"{sid}.jsonl").write_text("\n".join(json.dumps(row) for row in rows) + "\n")

    def save(self):
        training.write_json(self.root / "state.json", self.state)
        training.write_json(self.registry, {"schema": 1, "repo": "fixture/repo", "tasks": self.tasks})

    def export(self, roots=None):
        return training.export(roots or [self.root], self.registry, "fixture/repo", ADAGE, self.base / "bundle")

    def test_real_export_import_audit_preserves_negative_unknown_and_retries(self):
        self.add_run("retry", "T1", False)
        self.state["runs"]["never-started"] = {
            "phase": "train", "arm": "train", "task": "T2", "status": "skipped"}
        self.state["accounting"] = {"attempts": [{"run": run} for run in self.state["runs"] if run != "never-started"]}
        self.save()
        original = {p: p.read_bytes() for p in self.archive.glob("*.jsonl")}
        report = self.export()
        self.assertEqual((report["sessions"], report["training_groups"]), (4, 3))
        self.assertEqual((report["recorded_successes"], report["recorded_failures"], report["unknown_outcomes"]), (1, 2, 1))
        self.assertEqual(report["runs_without_native_session"], 1)
        self.assertEqual({p: p.read_bytes() for p in original}, original)
        identities = training.read_json(self.base / "bundle" / "identities.json")
        self.assertEqual(identities["one"]["group_id"], identities["retry"]["group_id"])
        self.assertNotEqual(identities["one"]["attempt_id"], identities["retry"]["attempt_id"])
        env = dict(os.environ, ADAGE_HOME=str(self.base / "home"), ADAGE_EMBED_MODEL="none")
        def cli(*args):
            result = subprocess.run([str(ADAGE), *args], capture_output=True, text=True, env=env, check=True)
            return json.loads(result.stdout)
        imported = cli("corpus", "import", str(self.base / "bundle" / "corpus.jsonl"), "--repo", "fixture/repo")
        self.assertEqual(imported["added"], 4)
        learned = cli("corpus", "learn", "--repo", "fixture/repo")
        self.assertEqual(learned["sessions_used"], 4)
        audit = cli("corpus", "inspect", "--repo", "fixture/repo", "--evidence")
        self.assertEqual(audit["training_declared_identity"], 4)
        self.assertEqual(len(audit["training_groups"]), 3)
        self.assertEqual(audit["recorded_outcomes"], 3)
        self.assertEqual(audit["training_inferred_identity"], 0)
        if os.name == "posix":
            self.assertEqual((self.base / "bundle").stat().st_mode & 0o077, 0)

    def test_held_out_task_or_related_group_cannot_enter_training(self):
        self.state["runs"]["train-one"]["task"] = "E1"
        self.save()
        with self.assertRaisesRegex(ValueError, "training split"):
            self.export()
        self.state["runs"]["train-one"]["task"] = "T1"
        self.tasks["E1"]["group_id"] = "family-one"
        self.save()
        with self.assertRaisesRegex(ValueError, "spans splits"):
            self.export()

    def test_mixed_or_incomplete_roots_are_rejected(self):
        for field, value, message in (("phase", "eval", "train-only"), ("arm", "adage", "train-only"),
                                      ("status", "running", "unfinished")):
            with self.subTest(field=field):
                prior = self.state["runs"]["train-one"][field]
                self.state["runs"]["train-one"][field] = value
                self.save()
                with self.assertRaisesRegex(ValueError, message):
                    self.export()
                self.state["runs"]["train-one"][field] = prior

    def test_missing_or_unaccounted_transcripts_fail_without_partial_bundle(self):
        path = self.archive / "one.jsonl"
        path.rename(self.archive / "unaccounted.jsonl")
        with self.assertRaisesRegex(ValueError, "exactly one"):
            self.export()
        (self.archive / "unaccounted.jsonl").rename(path)
        (self.archive / "extra.jsonl").write_bytes(path.read_bytes())
        with self.assertRaisesRegex(ValueError, "unaccounted"):
            self.export()
        self.assertFalse((self.base / "bundle").exists())

    def test_conflicting_native_session_identity_fails(self):
        path = self.archive / "one.jsonl"
        path.write_text(path.read_text().replace('"sessionId": "one"', '"sessionId": "other"'))
        with self.assertRaisesRegex(ValueError, "disagrees"):
            self.export()
        self.assertFalse((self.base / "bundle").exists())

    def test_mixed_or_truncated_native_records_cannot_be_silently_decoded(self):
        path = self.archive / "one.jsonl"
        original = path.read_bytes()
        for extra, message in ((b'{"sessionId":"other"}\n', "disagrees"), (b'{"partial":', None)):
            with self.subTest(extra=extra):
                path.write_bytes(original + extra)
                with self.assertRaises(ValueError) as raised:
                    self.export()
                if message:
                    self.assertIn(message, str(raised.exception))
                self.assertFalse((self.base / "bundle").exists())

    def test_source_changes_during_export_prevent_publication(self):
        original_run = subprocess.run
        path = self.archive / "one.jsonl"
        def change_after_export(*args, **kwargs):
            result = original_run(*args, **kwargs)
            path.write_bytes(path.read_bytes() + b"\n")
            return result
        with patch.object(training.subprocess, "run", side_effect=change_after_export):
            with self.assertRaisesRegex(ValueError, "changed during export"):
                self.export()
        self.assertFalse((self.base / "bundle").exists())

    def test_registry_changes_during_collection_prevent_publication(self):
        original_collect = training.collect
        def change_after_read(*args, **kwargs):
            result = original_collect(*args, **kwargs)
            changed = training.read_json(self.registry)
            changed["tasks"]["T1"]["group_id"] = "changed-family"
            training.write_json(self.registry, changed)
            return result
        with patch.object(training, "collect", side_effect=change_after_read):
            with self.assertRaisesRegex(ValueError, "changed during export"):
                self.export()
        self.assertFalse((self.base / "bundle").exists())

    def test_reused_session_and_overwritten_attempts_are_rejected(self):
        self.state["runs"]["train-two"]["metrics"]["session_id"] = "one"
        self.save()
        with self.assertRaisesRegex(ValueError, "reused"):
            self.export()
        self.state["runs"]["train-two"]["metrics"]["session_id"] = "two"
        self.state["accounting"] = {"attempts": [{"run": "train-one"}, {"run": "train-one"}]}
        self.save()
        with self.assertRaisesRegex(ValueError, "overwritten"):
            self.export()

    def test_unknown_grade_is_not_inferred_from_metrics_or_text(self):
        self.state["runs"]["train-three"]["score"] = {"passed": "true"}
        self.state["runs"]["train-three"]["metrics"]["passed"] = True
        self.save()
        self.export()
        self.assertNotIn("three", training.read_json(self.base / "bundle" / "outcomes.json"))

    def test_sources_and_output_are_not_overwritten(self):
        self.export()
        original = (self.base / "bundle" / "report.json").read_bytes()
        with self.assertRaisesRegex(ValueError, "already exists"):
            self.export()
        self.assertEqual(original, (self.base / "bundle" / "report.json").read_bytes())
        with self.assertRaisesRegex(ValueError, "duplicate JSON key"):
            path = self.base / "duplicate.json"
            path.write_text('{"a":1,"a":2}')
            training.read_json(path)


if __name__ == "__main__":
    unittest.main()
