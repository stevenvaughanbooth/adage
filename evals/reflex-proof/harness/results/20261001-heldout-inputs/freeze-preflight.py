"""Read-only preflight of heldout_cohort.freeze on the real inputs: every check freeze makes before it
writes, with the runner's own functions. Creates no root and writes nothing."""
import json
import sys
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, '/private/tmp/adage-heldout-src/evals/reflex-proof/harness')
import heldout_cohort as runner  # noqa: E402
from heldout_cohort import analysis, canary, gate, heldout_tasks, pairs  # noqa: E402

A = Path('/private/tmp/adage-real-repo-gate-v1')
C = A / 'cohorts/launch-gate-v7'
CUM = A / 'cohorts/curve-guided-v2/cumulative-launch-10'
DEVC = A / 'cohorts/curve-v2'
HELD = A / 'heldout-controls-1'
DEV = Path('/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9b/reports/dev-v9-1790870521952346000.json')
BIN = Path('/private/tmp/adage-heldout-src/target/release')
COMMIT = sys.argv[1]
SHAS = {'adage': sys.argv[2], 'adage-exec': sys.argv[3]}
AUDIT = [Path('/private/tmp/adage-learning-curve-v2')]

canary.require_local([gate.REPO, BIN, CUM, DEVC, HELD, DEV])
best = runner.design(DEV)
print('design', {k: best[k] for k in ('arm', 'card_orientation', 'card_checks', 'dev_protocol', 'report_sha256')})
commit, digests = runner.bind_binaries(BIN, COMMIT, SHAS)
print('binaries bound', commit, digests)
parent = pairs.read(C / 'manifest.json')
authority = Path(parent['budget_authority'])
record = pairs.read(CUM / 'attempt.json')
home = CUM / 'adage-home'
assert all(pairs.sha(pairs.safe_file(home, n)) == d for n, d in record['artifact_sha256'].items()), 'learned artifact changed'
routes = {r['track'] + '/' + r['route']['id']: r for r in pairs.read(home / 'index.json')['routes']}
assert routes.get(canary.ROUTE, {}).get('route', {}).get('step', {}).get('command') == canary.COMMAND, 'route missing'
guided = pairs.read(CUM.parent / 'manifest.json')
plan = dict(guided['families'][0], train=[], heldout=list(heldout_tasks.EVALUATION))
old, old_validation = pairs.read(DEVC / 'manifest.json'), pairs.read(DEVC / 'validation.json')
assert old_validation.get('complete') and old_validation.get('valid'), 'dev controls'
held, _ = runner.check_heldout_controls(HELD)
hp = held['families'][0]
assert hp['base_full'] == plan['base_full'] and hp['dependencies'] == plan['dependencies'], 'held-out base or deps differ'
print('held-out controls match the frozen task manifest, base and dependencies')
for task in heldout_tasks.EVALUATION:
    v = pairs.read(HELD / 'private/hidden-identities' / (task + '.json'))
    assert v['hidden_sha256'] == pairs.sha(HELD / 'private/tasks' / task / 'hidden.py') and v['identities'] and v['control_exit'] == 0, task
ctrl = {n: pairs.sha(DEVC / 'private/tasks' / analysis.CONTROL_TASK / n) for n in ('prompt.txt', 'hidden.py')}
assert ctrl == old['task_sha256'][analysis.CONTROL_TASK], 'control task material'
authorities = sorted({str(authority), *(str(a.resolve(strict=True)) for a in AUDIT)})
audit = canary.isolation(dict(cumulative_root=str(CUM), audit_authorities=authorities))
print('isolation audit passed', audit['passed'])
rows = analysis.matrix(heldout_tasks.EVALUATION)
budget = analysis.budget(len(rows))
print('matrix', len(rows), 'BEST rows', sum(r['arm'] == 'BEST' for r in rows), 'budget', budget)
print('sources', len(runner.sources()))
print('PREFLIGHT PASSED (nothing written)')
