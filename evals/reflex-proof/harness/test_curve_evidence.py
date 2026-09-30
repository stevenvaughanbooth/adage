import unittest
import tempfile
from pathlib import Path
from unittest.mock import patch
import curve_evidence
from curve_evidence import pytest_invocation


class InvocationAudit(unittest.TestCase):
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
