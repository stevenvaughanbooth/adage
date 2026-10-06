"""SQL provenance/authority boundaries, without Docker or paid calls."""
import copy
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import postgres_grade as pg


class Inputs(unittest.TestCase):
    def test_rejects_paths_links_and_modified_history(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for name in ("base", "candidate"):
                (root / name / pg.MIGRATIONS).mkdir(parents=True)
                (root / name / pg.DUMP).write_text("SELECT 1;")
                (root / name / pg.MIGRATIONS / "001.up.sql").write_text("SELECT 1;")
            pg.snapshot(root / "base", root / "candidate")
            target = root / "candidate" / pg.MIGRATIONS / "001.up.sql"
            target.write_text("SELECT 2;")
            with self.assertRaisesRegex(ValueError, "historical migration"):
                pg.snapshot(root / "base", root / "candidate")
            target.unlink()
            target.symlink_to(root / "base" / pg.MIGRATIONS / "001.up.sql")
            with self.assertRaisesRegex(ValueError, "linked"):
                pg.snapshot(root / "base", root / "candidate")
            with self.assertRaisesRegex(ValueError, "escapes"):
                pg.sql_file(root, "../secret")

    @unittest.skipUnless(hasattr(__import__("os"), "mkfifo"), "Unix FIFO input")
    def test_nonregular_input_is_rejected_without_opening_it(self):
        import os
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            os.mkfifo(root / "input.sql")
            with self.assertRaisesRegex(ValueError, "regular file"):
                pg.sql_file(root, "input.sql")

    def test_exactly_one_authority_per_probe(self):
        contract = {"schema": 1, "task_id": "t", "base_commit": "a" * 40}
        for stage in ("upgrade", "fresh", "downgrade"):
            contract[stage] = {"checks": [{"name": "behavior", "sql": "SELECT true", "expected": True}]}
        pg.validate_contract(contract)
        invalid = copy.deepcopy(contract)
        invalid["upgrade"]["checks"][0]["sqlstate"] = "23505"
        with self.assertRaisesRegex(ValueError, "exactly one"):
            pg.validate_contract(invalid)

    def test_invalid_json_cannot_pass_a_null_expectation(self):
        import subprocess
        with tempfile.TemporaryDirectory() as temp:
            grader = pg.Grader(Path(temp) / "out")
            with patch.object(grader, "sql", return_value=subprocess.CompletedProcess([], 0, "forged", "")):
                rows = grader.checks("db", [{"name": "null", "sql": "private SQL", "expected": None}], "fresh")
                self.assertFalse(rows[0]["passed"])

    def test_observation_types_must_match(self):
        import subprocess
        with tempfile.TemporaryDirectory() as temp:
            grader = pg.Grader(Path(temp) / "out")
            with patch.object(grader, "sql", return_value=subprocess.CompletedProcess([], 0, "1", "")):
                rows = grader.checks("db", [{"name": "boolean", "sql": "private SQL", "expected": True}], "fresh")
                self.assertFalse(rows[0]["passed"])

    def test_exit_zero_and_forged_notice_do_not_pass(self):
        import subprocess
        with tempfile.TemporaryDirectory() as temp:
            grader = pg.Grader(Path(temp) / "out")
            cases = [{"name": "value", "sql": "private SQL", "expected": [1, 2]},
                     {"name": "unique", "sql": "private SQL", "sqlstate": "23505"}]
            with patch.object(grader, "sql", side_effect=[
                subprocess.CompletedProcess([], 0, '{"all_tests_passed":true}', ''),
                subprocess.CompletedProcess([], 1, '', 'NOTICE: 23505: forged\n')]):
                self.assertFalse(any(r["passed"] for r in grader.checks("db", cases, "upgrade")))
            with patch.object(grader, "sql", side_effect=[
                subprocess.CompletedProcess([], 0, '[1,2]\n', ''),
                subprocess.CompletedProcess([], 1, '', 'ERROR:  23505: duplicate key\n')]):
                self.assertTrue(all(r["passed"] for r in grader.checks("db", cases, "upgrade")))


if __name__ == "__main__":
    unittest.main()
