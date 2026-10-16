"""Read-only: would each recipe's reservation be admitted by the live ledger right now? Works on an
in-memory deep copy of paid-state.json; persist is a no-op, so nothing is written."""
import copy
import json
import sys
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, '/private/tmp/adage-heldout-src/evals/live')
sys.path.insert(0, '/private/tmp/adage-heldout-src/evals/reflex-proof/harness')
from budget import Budget  # noqa: E402
import real_repo_gate as gate  # noqa: E402

A = Path('/private/tmp/adage-real-repo-gate-v1')
state = json.loads((A / 'paid-state.json').read_text())
native = json.loads((A / 'cohorts/launch-gate-v7/sessions/native-confinement/manifest.json').read_text())
for label, total, cap in (('native v7 recipe (no allocation digest)', gate.budget_limit(native), native['budget_usd']),
                          ('allocation ceiling', 60, 0.5), ('held-out session', 60, 0.40)):
    b = Budget(copy.deepcopy(state), total, cap, persist=lambda: None)
    attempt, why = b.reserve('probe/' + label)
    print(label, 'total', total, 'spent', round(b.spent, 7), 'admitted' if attempt else 'REFUSED: %s' % why)
