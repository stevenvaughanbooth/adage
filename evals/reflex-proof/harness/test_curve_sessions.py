import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import curve_sessions
import real_repo_gate as gate


class Allocation(unittest.TestCase):
    def test_old_recipe_keeps_old_ceiling(self):
        self.assertEqual(gate.budget_limit({}), 27.5)

    def test_allocation_is_bound_to_authority_and_hash(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "evaluation-allocation.json"
            p.write_text(json.dumps(dict(kind="human_evaluation_budget", authority=d,
                                        ledger_ceiling_usd=60, statement="approved pilot")))
            value = dict(experiment_root=d, budget_allocation_sha256=gate.sha(p))
            self.assertEqual(gate.budget_limit(value), 60)
            p.write_text(p.read_text().replace('60', '600'))
            with self.assertRaises(ValueError):
                gate.budget_limit(value)
            value['budget_allocation_sha256'] = gate.sha(p)
            with self.assertRaises(ValueError):
                gate.budget_limit(value)

    def test_registry_does_not_leak_env_to_native_policy(self):
        before = dict(os.environ)
        class Fake:
            FAMILIES = {}
        def fake_modules(root):
            os.environ['REPOBENCH_CACHE'] = str(root)
            os.environ['GOCACHE'] = str(root)
            return Fake, None, None, None
        with patch.object(gate, 'modules', side_effect=fake_modules):
            with curve_sessions.registry(Path('/tmp/example')):
                self.assertEqual(dict(os.environ), before)
        self.assertEqual(dict(os.environ), before)


if __name__ == '__main__':
    unittest.main()
