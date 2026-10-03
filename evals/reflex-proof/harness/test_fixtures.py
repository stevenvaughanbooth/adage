"""The tests read their own frozen inputs (fixtures/), never the archived runs (results/): an
archived run is evidence, and pruning or rewriting one must not change what a test sees."""
from pathlib import Path
import re
import unittest

HERE = Path(__file__).resolve().parent
ARCHIVED = re.compile(r"""["']results/|/\s*["']results["']""")


class FixturesTest(unittest.TestCase):
    def test_no_test_reads_an_archived_run(self):
        for test in sorted(HERE.glob("test_*.py")):
            for n, line in enumerate(test.read_text().splitlines(), 1):
                self.assertIsNone(ARCHIVED.search(line), f"{test.name}:{n} reads results/; use fixtures/: {line.strip()}")

    def test_every_fixture_is_read_by_a_test(self):
        tests = "\n".join(t.read_text() for t in HERE.glob("test_*.py"))
        for run in sorted(p.name for p in (HERE / "fixtures").iterdir() if p.is_dir()):
            self.assertIn(run, tests, f"fixtures/{run} is read by no test; remove it")


if __name__ == "__main__":
    unittest.main()
