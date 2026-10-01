# Launch canary (launch-canary-3): frozen, not dispatched

Status at 2026-10-01: frozen offline under protocol `launch-gate-1` revision 3. No canary agent
session, no grant. Nothing here is evidence of an effect yet.

- Cohort `cohorts/launch-gate-v2`, with its native-confinement session prepared by the fixed runner
  (`native-confinement-manifest.json`, `native-confinement-settings.json`). The policy is
  independent of the shell: recomputing it in the agent environment (60 names) and in an `env -i`
  shell (4 names) gives identical settings. The session must be run and reviewed from a terminal
  before canary preparation. The review now also requires environment denial (sentinel absent,
  no credential-like names visible to tools).
- Study `cohorts/launch-gate-v2/canary-v3`: 12 automatic sessions on the development task
  `mm-curve--finite`, two per condition (positive, failed, later-edit, config-change, broader,
  rerun). `manifest.json` / `manifest.sha256` freeze prompts, binaries, sources and inputs at
  commit `03de2b3`.
- Binaries: release `adage` `41684cdb…`, `adage-exec` `2c511ee5…` (production crates unchanged
  since `b14ffd3`). Learned route `run-175d85` from `cumulative-launch-10` (explicitly guided).
  `isolation-audit.json` passed.
- `approve-canary.sh` (sha256 `c288d6d1…`): the person-run grant script. E2 opt-in only for the
  twelve assignment homes, automatic approval off, sandbox required, the four declared caches,
  30 s timeout. Agents must not run it.

Superseded or aborted, preserved:

- `canary-v1-superseded-manifest.json`: zero-spend freeze on training-pool tasks (revision 2).
- `canary-v2-aborted/`: frozen under revision 2 in `cohorts/launch-gate-v1`. That cohort's native
  session was frozen in a Claude Code agent environment and run from the user's terminal
  ($0.0258378). Its review passed on files and network, but environment denial was
  shell-dependent and untested. It is **excluded as confinement evidence**
  (`EXCLUDED.json`; the ledger entry is marked excluded and keeps its cost). Canary preparation
  then failed the policy-equality check, so no assignment, grant or agent session was created
  (`ABORTED.jsonl`). See `../20260930-environment-denial-audit/FINDINGS.md`.
