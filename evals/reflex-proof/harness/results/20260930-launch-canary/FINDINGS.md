# Launch canary (launch-canary-2): frozen, not dispatched

Status at 2026-09-30: frozen offline under protocol `launch-gate-1` revision 2. No paid call, no
preparation of assignment checkouts, no grant. Nothing here is evidence of an effect yet.

- Cohort `cohorts/launch-gate-v1` (new native-confinement cohort; the `curve-guided-v2` review no
  longer matches the current runner source). Its native-confinement session is prepared and must
  be run and reviewed before canary preparation (`native-confinement-manifest.json`).
- Study `cohorts/launch-gate-v1/canary-v2`: 12 automatic sessions on the development task
  `mm-curve--finite`, two per condition (positive, failed, later-edit, config-change, broader,
  rerun). `manifest.json` / `manifest.sha256` freeze prompts, binaries, sources and inputs.
- Binaries: release `adage` `41684cdb…`, `adage-exec` `2c511ee5…`, built at `b14ffd3`; production
  crates are unchanged through the recorded `binary_source_commit` (the freeze refuses a dirty tree).
- Learned route: `run-175d85` from `cumulative-launch-10` (explicitly guided provenance).
  `isolation-audit.json` passed: all ten inputs are training-split sessions; 35 non-training
  session ids across 69 scanned records and every development/evaluation marker are absent.
- `approve-canary.sh` (sha256 `bfa2feb1…`): the person-run grant script. E2 opt-in only for the
  twelve assignment homes, automatic approval off, sandbox required, the four declared caches,
  30 s timeout. Agents must not run it.
- `canary-v1-superseded-manifest.json`: an earlier zero-spend freeze on training-pool tasks,
  superseded by revision 2 before preparation.

An offline dry run in a scratch cohort prepared assignments, rendered the script, ran the
production hooks without a grant (silent, as designed) and scored fabricated collections built
from the archived transfer-v3 timeline; none of that is in the ledger.
