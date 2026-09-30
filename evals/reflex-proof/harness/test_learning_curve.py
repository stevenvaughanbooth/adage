import copy
import unittest

import curve_tasks
import learning_curve as curve


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


if __name__ == "__main__":
    unittest.main()
