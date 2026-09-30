import copy
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import curve_tasks
import learning_curve as curve
import real_repo_gate as gate


class HistoryAdmission(unittest.TestCase):
    def setUp(self):
        self.manifest = {"families": [{"train": curve_tasks.TRAIN}]}
        self.records = [dict(task=t, session_id=str(i), sha256=str(i), synthetic=False,
                             hooks_enabled=False, outcome="failed" if i == 1 else "passed")
                        for i, t in enumerate(curve_tasks.TRAIN)]

    def test_failures_remain_in_prefix(self):
        rows = curve.history_inputs(self.manifest, 3, self.records)
        self.assertEqual(len(rows), 3)
        self.assertEqual(rows[1]["outcome"], "failed")
        self.assertEqual(curve.history_inputs(self.manifest, 0, []), [])

    def test_rejects_leakage_duplicates_and_exposure(self):
        for field, value in [("task", curve_tasks.HELDOUT[0]), ("session_id", "0"),
                             ("sha256", "0"), ("synthetic", True), ("hooks_enabled", True)]:
            with self.subTest(field=field):
                records = copy.deepcopy(self.records)
                records[1][field] = value
                with self.assertRaises(ValueError):
                    curve.history_inputs(self.manifest, 3, records)

    def test_incomplete_prefix_cannot_silently_shrink(self):
        with self.assertRaises(ValueError):
            curve.history_inputs(self.manifest, 5, self.records[:4])

    def test_task_matrix_is_disjoint(self):
        self.assertEqual(len(set(curve_tasks.TRAIN)), 15)
        self.assertEqual(len(set(curve_tasks.HELDOUT)), 4)
        self.assertFalse(set(curve_tasks.TRAIN) & set(curve_tasks.HELDOUT))

    def test_guided_history_cannot_be_organic_or_use_other_checkpoints(self):
        manifest = dict(self.manifest, training_kind=curve.GUIDED, workflow_guidance=curve.GUIDANCE,
                        checkpoints=[0, 5])
        records = [dict(r, kind=curve.GUIDED, workflow_guidance=curve.GUIDANCE) for r in self.records]
        self.assertEqual(len(curve.history_inputs(manifest, 5, records)), 5)
        for m, count, rows in [(manifest, 3, records), (self.manifest, 5, records),
                               (manifest, 5, self.records)]:
            with self.assertRaises(ValueError):
                curve.history_inputs(m, count, rows)


class GuidedFreeze(unittest.TestCase):
    def test_frozen_scope_prompt_and_provenance(self):
        class Families:
            @staticmethod
            def prompt(task):
                return "Implement " + task
            @staticmethod
            def parse_task(task):
                return {"hidden": lambda variant: "hidden " + variant}, task
        with tempfile.TemporaryDirectory() as d, patch.object(curve, "sources", return_value={}), \
                patch.object(gate, "modules", return_value=(Families, None, None, None)), \
                patch.object(curve_tasks, "install"), patch.object(curve.subprocess, "check_output", return_value="revision"):
            authority = Path(d)
            gate.write(authority / "manifest.json", dict(families=[dict(family="mm-validator")],
                preparation_python="python", python_requested="3.12"))
            organic = curve.freeze(authority / "organic", authority)
            self.assertEqual(organic["protocol"], "learning-curve-1")
            self.assertEqual(organic["checkpoints"], list(curve.CHECKPOINTS))
            self.assertEqual(len(organic["matrix"]), 16)
            root = authority / "guided"
            guided = curve.freeze(root, authority, curve.GUIDED)
            self.assertEqual(guided["families"][0]["train"], curve_tasks.TRAIN[:5])
            self.assertEqual(guided["families"][0]["heldout"], [])
            self.assertEqual(guided["matrix"], [])
            self.assertEqual(guided["checkpoints"], [0, 5])
            self.assertEqual(guided["budget"]["reservation_usd"], 7.5)
            task = curve_tasks.TRAIN[0]
            frozen = root / "private/tasks" / task / "prompt.txt"
            self.assertEqual(frozen.read_text(), Families.prompt(task) + curve.GUIDANCE)
            value = dict(experiment_root=str(root), kind=curve.GUIDED, task=task,
                         workflow_guidance=curve.GUIDANCE, prompt_sha256=gate.sha(frozen), argv=[frozen.read_text()])
            gate.validate_training_recipe(value)
            curve.verify(root)
            for field, bad in [("kind", "organic-training"), ("argv", [Families.prompt(task)]),
                               ("workflow_guidance", None), ("prompt_sha256", "bad")]:
                with self.subTest(field=field), self.assertRaises(ValueError):
                    gate.validate_training_recipe(dict(value, **{field: bad}))
            with patch.object(curve, "sources", return_value={"changed": "source"}), self.assertRaises(ValueError):
                curve.verify(root)
            guided["training_kind"] = "organic-training"
            gate.write(root / "manifest.json", guided)
            with self.assertRaises(ValueError):
                curve.verify(root)


if __name__ == "__main__":
    unittest.main()
