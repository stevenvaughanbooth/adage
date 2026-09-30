# Launch handoff — 2026-09-30

## Objective and current decision

Ship a useful open-source developer preview: learn recurring behavior across sessions, predict
when a bounded action helps, run approved actions on the CPU, and demonstrate that the agent
actually avoids work without losing correctness. Do not declare broad launch readiness from
compilation, a successful hook, or advisory benchmark gains alone.

PR #14 merged into main at `e5f5e33651df86a3b1dc89bafb6ba10e9193c2e1`.
This follow-up uses branch `codex/launch-first-reflex` in
`/Users/stevenbooth/.codex/worktrees/adage-reflex-proof/skill-compile`.
Follow-up PR: https://github.com/stevenvaughanbooth/adage/pull/16 (draft, mergeable when checked).
The primary checkout is `/Users/stevenbooth/Documents/skill-compile`; leave its local work alone.

The user lifted the Codex 5% reserve and authorized using the remaining allowance. The independent
paid evaluation ledger still has a $60 cap. At the last settled snapshot: $13.2920411 measured
provider cost plus a separately human-approved $0.50 conservative unknown charge, totaling
$13.7920411 across 62 attempts. Read the live ledger before scheduling anything; never reset it,
double-dispatch, or treat an unknown bill as zero.

## What is established

- Sixteen cold-start assignments on four Marshmallow tasks, two repetitions per arm: instincts
  8/8 correct versus 6/8 alone; $1.7720458 versus $1.9429724; 272 versus 304 model requests.
  Combined reductions: 8.8% cost, 10.5% requests. Three cost regressions remain. Replication alone
  saved only 0.6% with equal requests. These are advisory instincts, not automatic CPU savings.
- Fifteen organic training sessions produced no executable candidates. Preserve that negative.
- Ten explicitly guided sessions across five repeated tasks produced four certified triggers for
  **one** pytest routine, with 9/10 qualifying continuations. Task quality was 9/10. Three triggers
  are sequence contexts. This is real cross-session learning, not autonomous routine discovery.
- Production execution-prefix admission now retains routine actions without relaxing the
  execution evidence bar. Execution-only candidates cannot fall back into ordinary advice.
- The earlier generated lookup benchmark proved action elimination; real coding transfer remains
  a separate open gate. GPU utilization and energy were never measured.

Website evidence is in `web/site/index.html` and `web/site/public/evidence/cold-start.json`.
All study records are indexed by `evals/reflex-proof/LAUNCH-STATUS.md`. Do not cherry-pick or delete
earlier failures. GitHub deployment `6768511910` reports successful Production deployment of the
PR #14 merge commit. Its reported URL is `https://adage-site-r5gqonovm-adage3.vercel.app`;
public-domain accessibility was not independently verified. Recheck future deployments after merge.

## CPU transfer study and approval boundary

Local archive: `/private/tmp/adage-real-repo-gate-v1`.
Shared accounting: `paid-state.json` and `evaluation-allocation.json` there.
Cohort: `cohorts/curve-guided-v2`.
Original learned artifacts: `cumulative-10`; first cache-fix replay: `cumulative-cache-10`.
Current replay: `cumulative-launch-10`; current prepared study: `transfer-v3`.
Current human script: `/private/tmp/adage-real-repo-cpu-approve-v4.sh` (earlier scripts are superseded).

After all four human grants exist, from the worktree run:

```sh
python3 evals/reflex-proof/harness/curve_transfer.py run \
  /private/tmp/adage-real-repo-gate-v1/cohorts/curve-guided-v2/transfer-v3
python3 evals/reflex-proof/harness/curve_transfer.py report \
  /private/tmp/adage-real-repo-gate-v1/cohorts/curve-guided-v2/transfer-v3
```

First inspect the ledger and any live processes: never start a second copy of an attempted run.
If this run has already completed, report and export it rather than dispatching it again. The
eight sessions reserve at most $12 against the shared ledger. Do not edit the frozen harness,
`LEARNING-CURVE.md`, task material or binaries during the comparison.

The frozen transfer study compares `learned` (same artifact, no execution permission) against
`automatic` (one human-approved context). Eight sessions cover four tasks excluded from training,
but previously observed in cold pilots. Prompts contain no workflow guidance. Select the first
certified context deterministically: `run-175d85`, edits to `tests/*.py` then `CHANGELOG.rst` within
eight events; command `.venv/bin/python -m pytest tests/test_validate.py -q`.

Gate: at least three confirmed action eliminations, all eight complete, all quality checks passing,
known costs, exact source/receipt/approval checks and no unknown shell behavior. Alternate pytest
scopes do not qualify as the same avoided action. Preserve a negative result; do not loosen scoring
or rerun outcomes. Runtime bin hashes and source hashes are frozen; after a code change, preserve
the old root and prepare a new one. Never overwrite the training or replay history.

`transfer-v1` stopped before dispatch because `.pytest_cache/**` was rejected. `transfer-v2`
also stopped before dispatch: missing E2 policy, then the human's sandboxed approval run passed
pytest but refused `.pytest_cache/.gitignore`. No grants were saved. The new fix supports exactly
that additional cache file; it does not authorize repository `.gitignore` changes or hidden outputs.

**Agents must never issue grants.** `require_person` rejects agent sessions. Do not strip environment
markers, write `approvals.json`, or execute the human approval script yourself. User budget permission
does not grant product execution permission. A complete human script must explicitly opt only its
four temporary homes into E2, keep automatic approval off, require sandboxing, and declare both
`.pytest_cache/**` and `.pytest_cache/.gitignore` plus scoped bytecode caches. Check the resulting
four grant files before dispatch. Earlier scripts are historical; do not reuse their paths blindly.

## Launch experience and next priorities

The follow-up adds `docs/first-reflex.md`, first-run links to review and execution status, clearer
cache errors, and regression coverage for pytest's hidden metadata. It preserves the current TUI.
Do not create another learner or parallel onboarding implementation.

1. Finish the fixed real-repository CPU comparison after valid human grants. Export every result,
   updated ledger, quality grade, execution receipt and reproducible aggregate before making claims.
2. Walk the first-reflex guide on a fresh supported installation. Verify hook registration, history
   discovery, empty-history behavior, candidate review, policy/grant setup, result delivery and revoke.
3. Improve naturally occurring history coverage; current organic-versus-guided gap is material.
   Measure unnecessary executions, duplicates, abstention and drift along with saved actions.
4. Run release platform CI before claiming all advertised binaries and harnesses work. Local macOS
   success is not Linux or Windows evidence; Windows automatic execution is unsupported.
5. Replicate on another repository/model before treating the pilot as a general savings estimate.
   Frontier-to-cheaper-model transfer and a marketplace remain future hypotheses.

Do not promise a universal session count for learning or guaranteed savings from `init`. The launch
pitch should show a concrete demonstrated workflow, publish its limits, and invite developers to
contribute histories and reproducible reflex cases.
