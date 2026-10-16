import unittest
from unittest.mock import patch

from curve_replay import validated_inputs


class FrozenReplay(unittest.TestCase):
    def checkpoint(self, count=5, kind='instruction-guided-training'):
        return dict(status='completed', count=count, inputs=[dict(session_id=str(i), sha256=str(i),
            copy=str(i), kind=kind, workflow_guidance='repo workflow' if kind.startswith('instruction') else None,
            synthetic=False, hooks_enabled=False) for i in range(count)])

    @patch('curve_replay.gate.sha', side_effect=lambda p: str(p))
    def test_complete_guided_and_organic_checkpoints_preserve_regime(self, sha):
        for count, kind in ((5, 'instruction-guided-training'), (15, 'organic-training')):
            original = self.checkpoint(count, kind)
            self.assertEqual(validated_inputs(original), original['inputs'])

    @patch('curve_replay.gate.sha', side_effect=lambda p: str(p))
    def test_rejects_incomplete_mixed_exposed_or_duplicate_history(self, sha):
        variants = []
        for key, value in (('count', 15), ('count', True), ('status', 'started')):
            original = self.checkpoint(); original[key] = value; variants.append(original)
        for key, value in (('kind', 'organic-training'), ('workflow_guidance', None),
                           ('session_id', '1'), ('sha256', '1'), ('synthetic', True),
                           ('hooks_enabled', True), ('copy', 'changed')):
            original = self.checkpoint(); original['inputs'][0][key] = value; variants.append(original)
        for original in variants:
            with self.subTest(original=original), self.assertRaises(ValueError):
                validated_inputs(original)


if __name__ == '__main__':
    unittest.main()
