import unittest
import json
import tempfile
from pathlib import Path
from unittest.mock import patch
import curve_evidence
from curve_evidence import pytest_invocation


class InvocationAudit(unittest.TestCase):
    def test_guided_report_never_claims_autonomous_discovery_or_heldout_efficacy(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'manifest.json').write_text(json.dumps(dict(
                protocol='instruction-guided-development-1', budget_authority=str(root),
                families=[dict(train=['demo'])], checkpoints=[5],
                training_kind='instruction-guided-training', workflow_guidance='run the repo tests')))
            (root / 'paid-state.json').write_text('{}')
            checkpoint = root / 'checkpoints' / '5'
            checkpoint.mkdir(parents=True)
            (checkpoint / 'attempt.json').write_text(json.dumps(dict(
                status='completed', binary_sha256='test-binary', execution_candidates=[])))
            row = dict(cost_usd=0.25, successful_pytest_calls=1)
            with patch.object(curve_evidence, 'training_row', return_value=row), \
                    patch.object(curve_evidence.curve_heldout, 'report') as heldout:
                result = curve_evidence.report(root)
            heldout.assert_not_called()
        self.assertEqual(result['status'], 'training-complete-no-execution-candidate')
        self.assertEqual(result['heldout']['assignment_denominator'], 0)
        self.assertFalse(result['claims']['autonomous_discovery_tested'])
        self.assertFalse(result['claims']['cpu_action_elimination_established'])
        self.assertEqual(result['known_training_cost_usd'], 0.25)

    def test_recognizes_invocation_without_claiming_coverage(self):
        for command in ('python3 -m pytest tests/test_validate.py', '.venv/bin/python -m pytest -q',
                        'source .venv/bin/activate && python -m pytest tests', '.venv/bin/pytest --co'):
            self.assertTrue(pytest_invocation(command), command)

    def test_mentions_and_masked_exit_status_do_not_count(self):
        for command in ('python3 -m pip install pytest', 'echo pytest', 'cat > /tmp/x <<EOF\n# pytest\nEOF',
                        '.venv/bin/python -m pytest | tail -5', '.venv/bin/python -m pytest || true',
                        '.venv/bin/python -m pytest; echo done', 'source unknown && python -m pytest'):
            self.assertFalse(pytest_invocation(command), command)

    def test_cold_totals_keep_unknown_cost_and_missing_requests_unknown(self):
        rows = [dict(task=str(i), arm=arm, order=i, cost_usd=0.25, quality=dict(passed=i != 1),
            agent_action_count=3, observed_dispatches=[dict(request_id=str(i))], collection_errors=[])
            for arm in ('agent', 'instincts') for i in range(4)]
        rows[0]['cost_usd'] = None
        rows[0]['observed_dispatches'] = []
        source = dict(protocol='learning-curve-cold-1', rows=rows, collected=8, contrasts={}, limitations=[])
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'manifest.json').write_text('{}')
            with patch.object(curve_evidence.curve_heldout, 'report', return_value=source):
                result = curve_evidence.cold_report(root)
        self.assertEqual(result['assignment_denominator'], 8)
        self.assertIsNone(result['totals']['agent']['reported_cost_usd'])
        self.assertIsNone(result['totals']['agent']['observed_model_requests'])
        self.assertEqual(result['totals']['instincts']['reported_cost_usd'], 1)
        self.assertEqual(result['totals']['instincts']['quality_passed'], 3)
        self.assertEqual(result['cpu_savings_claim'], 'not tested')


if __name__ == '__main__':
    unittest.main()
