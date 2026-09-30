import json
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch

import prepare_training as training


class TrainingPreparation(unittest.TestCase):
    def test_revision_preserves_prompt_authority_and_original_and_blocks_reuse(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            original = root / "manifest.json"
            value = dict(root=str(root), kind="organic-training", training_hooks_enabled=False,
                         argv=["claude", "--restricted", "--max-budget-usd", "1.5", "original task"],
                         budget_usd=1.5, max_turns=60, shared_sources={})
            original.write_text(json.dumps(value))
            before = original.read_bytes()
            recipe = json.loads(training.revise(original).read_text())
            try:
                self.assertEqual(original.read_bytes(), before)
                self.assertEqual(recipe["argv"][-1], "original task")
                self.assertEqual(recipe["argv"][2:-3], value["argv"][:-1])
                self.assertEqual(recipe["budget_usd"], 1.5)
                self.assertFalse(recipe["training_hooks_enabled"])
                self.assertEqual(recipe["shared_sources"][str(original.resolve())], training.gate.sha(original))
                with self.assertRaises(ValueError):
                    training.revise(original)
            finally:
                socket = Path(recipe["argv"][-2])
                shutil.rmtree(socket.parent)

    def test_attempted_assignment_and_heldout_preparation_are_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            original = root / "manifest.json"
            original.write_text(json.dumps(dict(root=str(root), kind="organic-training", training_hooks_enabled=False)))
            (root / "run.json").write_text("{}")
            with self.assertRaises(ValueError):
                training.revise(original)
            (root / "native-confinement-reviewed.json").write_text('{"passed":true}')
            original.write_text('{"families":[{"train":["train"],"heldout":["test"]}]}')
            with patch.object(training.gate, "prepare_session") as dispatch:
                with self.assertRaises(ValueError):
                    training.prepare(root, "test", "model")
                dispatch.assert_not_called()


if __name__ == "__main__":
    unittest.main()
