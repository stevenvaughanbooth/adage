import unittest
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


if __name__ == '__main__':
    unittest.main()
