import json
from pathlib import Path
import tempfile
import unittest

import curve_cold as cold


class ReplicationAdmission(unittest.TestCase):
    def test_requires_complete_bills_but_preserves_quality_failures(self):
        with tempfile.TemporaryDirectory() as d:
            authority = Path(d).resolve(); previous = authority / 'first'; previous.mkdir()
            original = authority / 'cohort'
            entries = [dict(task='one', arm=a) for a in ('agent', 'instincts')]
            m = dict(protocol=cold.PROTOCOL, native_cohort_root=str(original),
                budget_authority=str(authority), families=[dict(heldout=['one'])], matrix=entries)
            cold.pairs.write(previous / 'manifest.json', m)
            (previous / 'manifest.sha256').write_text(cold.pairs.sha(previous / 'manifest.json'))
            ledger = dict(runs={})
            paths = []
            for e in entries:
                session = previous / 'heldout' / e['task'] / e['arm']; session.mkdir(parents=True)
                path = session / 'collection.json'; paths.append(path)
                cold.pairs.write(path, dict(cost_usd=0.2, collection_errors=[], quality=dict(passed=False)))
                ledger['runs'][str(session.relative_to(authority))] = dict(status='complete')
            self.assertEqual(len(cold.replication_inputs(previous, original, ['one'], ledger)), 3)
            c = cold.pairs.read(paths[0]); c['cost_usd'] = None; cold.pairs.write(paths[0], c)
            with self.assertRaises(ValueError):
                cold.replication_inputs(previous, original, ['one'], ledger)
            c['cost_usd'] = .2; cold.pairs.write(paths[0], c)
            ledger['runs'].pop(next(iter(ledger['runs'])))
            with self.assertRaises(ValueError):
                cold.replication_inputs(previous, original, ['one'], ledger)


if __name__ == '__main__':
    unittest.main()
