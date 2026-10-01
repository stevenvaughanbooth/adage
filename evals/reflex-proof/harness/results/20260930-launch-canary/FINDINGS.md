# Launch canary (launch-canary-4): frozen, not dispatched

Status at 2026-10-01: frozen offline under protocol `launch-gate-1` revision 4. No canary agent
session and no grant exist. Nothing here is evidence of an effect yet.

- Cohort `cohorts/launch-gate-v3`. Its native-confinement session is prepared by the fixed runner:
  `native-confinement-manifest.json`, `native-confinement-settings.json`, and the canary program
  `native_canary.py`. The program checks files, the network, the session inbox socket (fixed at
  preparation) and the tool environment, and probes the sandbox proxy. The policy does not depend
  on the shell: the agent environment (60 names) and an `env -i` shell (3 names) give identical
  settings. The session must be run and reviewed from a terminal before canary preparation.
- Study `cohorts/launch-gate-v3/canary-v4`: 12 automatic sessions on the development task
  `mm-curve--finite`, two per condition (positive, failed, later-edit, config-change, broader,
  rerun), frozen at commit `a5a769a`.
- Binaries: release `adage` `41684cdb…`, `adage-exec` `2c511ee5…`; the production crates are
  unchanged since `b14ffd3`. Learned route: `run-175d85` from `cumulative-launch-10` (explicitly
  guided). `isolation-audit.json` passed.
- `approve-canary.sh` (sha256 `9b35534a…`) is the person-run grant script. It enables E2 only for
  the twelve assignment homes, keeps automatic approval off, requires the sandbox, declares the
  four caches and sets a 30 s timeout. Agents must not run it.

Superseded or aborted attempts, preserved:

- `canary-v1-superseded-manifest.json`: zero-spend freeze on training-pool tasks (revision 2).
- `canary-v2-aborted/`: environment denial depended on the shell. The `launch-gate-v1` native
  session ($0.0258378) is excluded as confinement evidence. The canary was never prepared.
- `canary-v3-aborted/`: the `launch-gate-v2` native session ($0.0278896) passed the file, network
  and sentinel checks. Its review failed closed on two names that Claude Code itself gives tool
  children (`CLOUDSDK_PROXY_PASSWORD`, `CLAUDE_CODE_MESSAGING_TOKEN`). That review is immutable and
  excluded, and the canary was never prepared. See `../20260930-environment-denial-audit/FINDINGS.md`.
