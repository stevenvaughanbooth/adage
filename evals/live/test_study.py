"""Free inventory admission checks; test controls are fixtures, never live efficacy evidence."""
import copy
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import study


class StudyRegistration(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.inputs = {}
        source = self.pin("source", "fixture source")
        prompt = self.pin("prompt", "Add the requested behavior and verify it.")
        controls = self.pin("controls.json", json.dumps({
            "task_id": "task-1", "base_commit": "a" * 40, "quality_authority": "external_parent",
            "kind": "semantic", "coverage_reviewed_by": "fixture reviewer", "remaining_omissions": [],
            "inputs": [source], "cases": [{"role": role, "accepted": role == "reference"}
                                          for role in sorted(study.CONTROL_ROLES)]}))
        self.spec = {"schema": 1, "name": "fixture", "scenario": "process", "repo": "github.com/example/repo",
                     "population": "fixture only", "sampling_rule": "all registered fixtures",
                     "treatment": "production_learned_e0", "policy": study.read(study.HERE / "policies/claim.json"),
                     "model_revision": "fixture-model", "harness_version": "fixture-cli",
                     "inputs": self.inputs, "prior_exposure_keys": [],
                     "tasks": [{"id": "task-1", "group": "feature-1", "related_keys": ["issue-1"],
                                "phase": "train", "exposed": True, "provenance": "historical",
                                "base_commit": "a" * 40, "prompt": prompt, "controls": controls}],
                     "targets": {"train": 1, "development": None, "confirmation": None},
                     "session_cap_microusd": 1_000_000,
                     "allocations_microusd": {"train": 1_000_000, "development": None, "confirmation": None},
                     "readiness": {gate: {"status": "pass", "artifact": source} for gate in (
                         "source_closure", "runtime", "parent_grader", "provider_only_egress", "group_review",
                         "learned_lineage", "operator_review", "delivery_controls", "development_decision",
                         "power_analysis", "multiplicity_policy")}}

    def pin(self, name, contents):
        path = self.root / name
        path.write_text(contents)
        self.inputs[str(path)] = study.digest(path)
        return str(path)

    def change_controls(self, change):
        path = self.spec["tasks"][0]["controls"]
        body = study.read(path)
        change(body)
        Path(path).write_bytes(study.encoded(body))
        self.inputs[path] = study.digest(path)

    def test_draft_is_read_only_and_never_authorizes_spend_or_claim(self):
        before = {p: p.read_bytes() for p in self.root.iterdir()}
        result = study.audit(self.spec, "train")
        self.assertTrue(result["ready_for_sealing"])
        self.assertEqual(result["reservation_microusd"], 1_000_000)
        self.assertFalse(result["paid_dispatch_authorized"])
        self.assertFalse(result["customer_savings_claim"])
        self.assertEqual(before, {p: p.read_bytes() for p in self.root.iterdir()})
        result = study.audit(self.spec, "confirmation")
        self.assertFalse(result["ready_for_sealing"])
        self.assertTrue(any("200 independent" in b for b in result["blockers"]))

    def test_related_followup_cannot_be_relabelled_as_independent(self):
        other = copy.deepcopy(self.spec["tasks"][0])
        other.update(id="followup", group="fake-independent", phase="confirmation")
        self.spec["tasks"].append(other)
        with self.assertRaisesRegex(ValueError, "related issues"):
            study.audit(self.spec, "train")
        other["group"] = "feature-1"
        with self.assertRaisesRegex(ValueError, "crosses phases"):
            study.audit(self.spec, "train")

    def test_inspected_issue_and_prior_alias_cannot_enter_confirmation(self):
        task = self.spec["tasks"][0]
        task["phase"] = "confirmation"
        for exposed, prior in ((True, []), (False, ["issue-1"]), (False, ["feature-1"])):
            task["exposed"], self.spec["prior_exposure_keys"] = exposed, prior
            self.assertTrue(any("previously exposed" in b for b in study.audit(self.spec, "confirmation")["blockers"]))

    def test_variants_do_not_inflate_paired_group_count(self):
        self.spec["tasks"][0]["phase"] = "development"
        other = copy.deepcopy(self.spec["tasks"][0])
        other["id"] = "variant"
        self.spec["tasks"].append(other)
        result = study.audit(self.spec, "development")
        self.assertEqual(result["groups"]["development"], 1)
        self.assertTrue(any("one representative" in b for b in result["blockers"]))

    def test_policy_and_integral_budgets_cannot_be_weakened(self):
        self.spec["policy"]["min_groups"] = 2
        with self.assertRaisesRegex(ValueError, "launch policy"):
            study.audit(self.spec, "train")
        self.spec["policy"] = study.read(study.HERE / "policies/claim.json")
        self.spec["allocations_microusd"]["train"] = 999_999
        self.assertFalse(study.audit(self.spec, "train")["ready_for_sealing"])
        self.spec["session_cap_microusd"] = True
        with self.assertRaisesRegex(ValueError, "session cap"):
            study.audit(self.spec, "train")

    def test_forged_report_or_structural_checks_cannot_qualify_semantic_grade(self):
        self.change_controls(lambda body: body["cases"].append({"role": "forged_report", "accepted": True}))
        self.assertTrue(any("unexpected grade" in b for b in study.audit(self.spec, "train")["blockers"]))
        self.change_controls(lambda body: body.update(kind="structural", remaining_omissions=["renewal policy"]))
        result = study.audit(self.spec, "train")
        self.assertTrue(any("semantic external-parent" in b for b in result["blockers"]))
        self.assertTrue(any("coverage review" in b for b in result["blockers"]))

    def test_synthetic_and_missing_runtime_evidence_block(self):
        self.spec["tasks"][0]["provenance"] = "synthetic"
        self.spec["readiness"]["provider_only_egress"]["status"] = "unverified"
        result = study.audit(self.spec, "train")
        self.assertTrue(any("synthetic" in b for b in result["blockers"]))
        self.assertTrue(any("provider_only_egress" in b for b in result["blockers"]))
        with self.assertRaisesRegex(ValueError, "not ready"):
            study.seal(self.spec, "train", self.root / "blocked")
        self.assertFalse((self.root / "blocked").exists())

    def test_changed_or_linked_dependencies_block(self):
        path = Path(self.spec["tasks"][0]["prompt"])
        path.write_text("Changed after registration")
        self.assertFalse(study.audit(self.spec, "train")["ready_for_sealing"])
        self.inputs[str(path)] = study.digest(path)
        original = path.with_name("original")
        path.rename(original)
        path.symlink_to(original)
        self.assertFalse(study.audit(self.spec, "train")["ready_for_sealing"])

    def test_seal_preserves_bytes_and_refuses_replacement(self):
        out = self.root / "seal"
        record = study.seal(self.spec, "train", out)
        self.assertEqual(study.verify(out)["sha256"], record["sha256"])
        Path(self.spec["tasks"][0]["prompt"]).write_text("Later development edit")
        self.assertTrue(study.verify(out)["integrity_verified"])
        self.assertFalse(study.audit(self.spec, "train")["ready_for_sealing"])
        object_path = out / "objects" / next(iter(self.inputs.values()))
        object_path.write_text("tampered")
        with self.assertRaisesRegex(ValueError, "missing or changed"):
            study.verify(out)

    def test_linked_destination_ancestor_cannot_bypass_git_boundary(self):
        repo = self.root / "repo"
        (repo / ".git").mkdir(parents=True)
        subdir = repo / "private-looking-subdir"
        subdir.mkdir()
        alias = self.root / "alias"
        alias.symlink_to(subdir, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, "linked ancestors"):
            study.seal(self.spec, "train", alias / "seal")
        self.assertEqual(list(subdir.iterdir()), [])

    def test_publication_does_not_replace_racing_empty_destination(self):
        out = self.root / "racing-seal"
        original_copy = study.shutil.copyfile
        inode = []

        def copy_and_race(source, target):
            result = original_copy(source, target)
            if not inode:
                out.mkdir()
                inode.append(out.stat().st_ino)
            return result

        with mock.patch.object(study.shutil, "copyfile", side_effect=copy_and_race):
            with self.assertRaises(FileExistsError):
                study.seal(self.spec, "train", out)
        self.assertEqual(out.stat().st_ino, inode[0])
        self.assertEqual(list(out.iterdir()), [])

    def test_sealed_audit_uses_frozen_reports_and_keeps_original_paths(self):
        controls = Path(self.spec["tasks"][0]["controls"])
        original_bytes = controls.read_bytes()
        original_copy = study.shutil.copyfile

        def copy_then_change_original(source, target):
            result = original_copy(source, target)
            if Path(source) == controls:
                controls.write_text('{"kind": "structural"}')
            return result

        out = self.root / "frozen-seal"
        with mock.patch.object(study.shutil, "copyfile", side_effect=copy_then_change_original):
            record = study.seal(self.spec, "train", out)
        self.assertTrue(study.verify(out)["integrity_verified"])
        self.assertTrue(record["registration"]["audit"]["ready_for_sealing"])
        self.assertEqual(record["registration"]["spec"]["tasks"][0]["controls"], str(controls))
        self.assertEqual((out / "objects" / self.inputs[str(controls)]).read_bytes(), original_bytes)
        self.assertFalse(study.audit(self.spec, "train")["ready_for_sealing"])

    def test_reopened_forged_report_cannot_qualify_bad_sealed_bytes(self):
        controls = Path(self.spec["tasks"][0]["controls"])
        claimed_good = study.read(controls)
        self.change_controls(lambda body: body.update(kind="structural"))
        original_read = study.read

        def forged_reopen(path):
            return claimed_good if Path(path) == controls else original_read(path)

        with mock.patch.object(study, "read", side_effect=forged_reopen):
            with self.assertRaisesRegex(ValueError, "not ready"):
                study.seal(self.spec, "train", self.root / "bad-seal")
        self.assertFalse((self.root / "bad-seal").exists())

    def test_interrupted_publication_preserves_pending_evidence_and_fails_closed(self):
        out = self.root / "interrupted-seal"
        original_rename = study.os.rename

        def interrupt_final_publish(source, target):
            if Path(source) == out / "registration.pending.json":
                raise OSError("simulated interruption before publication")
            return original_rename(source, target)

        with mock.patch.object(study.os, "rename", side_effect=interrupt_final_publish):
            with self.assertRaisesRegex(OSError, "simulated interruption"):
                study.seal(self.spec, "train", out)
        self.assertTrue((out / "registration.pending.json").is_file())
        self.assertTrue((out / "objects").is_dir())
        self.assertFalse((out / "registration.json").exists())
        with self.assertRaises(FileNotFoundError):
            study.verify(out)
        with self.assertRaisesRegex(ValueError, "already exists"):
            study.seal(self.spec, "train", out)

    def test_malformed_reports_and_whitespace_aliases_fail_closed(self):
        with self.assertRaisesRegex(ValueError, "study must be"):
            study.audit([], "train")
        self.spec["tasks"][0]["related_keys"] = ["issue-1 "]
        with self.assertRaisesRegex(ValueError, "relationship keys"):
            study.audit(self.spec, "train")
        self.spec["tasks"][0]["related_keys"] = ["issue-1"]
        path = self.spec["tasks"][0]["controls"]
        Path(path).write_text("[]")
        self.inputs[path] = study.digest(path)
        with self.assertRaisesRegex(ValueError, "control report must be"):
            study.audit(self.spec, "train")

    def test_registration_is_private_and_duplicate_keys_rejected(self):
        repo = self.root / "repo"
        (repo / ".git").mkdir(parents=True)
        with self.assertRaisesRegex(ValueError, "outside Git"):
            study.seal(self.spec, "train", repo / "study")
        out = self.root / "seal"
        study.seal(self.spec, "train", out)
        with self.assertRaisesRegex(ValueError, "already exists"):
            study.seal(self.spec, "train", out)
        path = self.root / "ambiguous.json"
        path.write_text('{"tasks": [], "tasks": [1]}')
        with self.assertRaisesRegex(ValueError, "duplicate JSON"):
            study.read(path)


if __name__ == "__main__":
    unittest.main()
