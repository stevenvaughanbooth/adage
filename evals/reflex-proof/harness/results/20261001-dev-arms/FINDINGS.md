# Development canary-v8 (dev-arms-1): frozen, not dispatched

Development-split work under `launch-gate-1` revision 8. Nothing here is held-out evidence, and
no statistic exists yet.

- **Cohort:** `cohorts/launch-gate-v5`. A new native-confinement session is prepared by the
  revision-7 runner (`native-confinement-*.json`). It must be run and reviewed before preparation.
- **Study:** `cohorts/launch-gate-v5/dev-v8b`, frozen at commit `651c142`. `manifest.json` and
  `manifest.sha256` freeze prompts, arms, binaries, sources and the learned home.
- **Arms**, run in six paired blocks on `mm-curve--finite` (a rotating arm order, each with the
  same prompt and the canary-v7 test block):
  - A0: no Adage hooks;
  - A1: revision-7 binaries (`c5f896c`: `adage` `b364f08a…`, `adage-exec` `e2b09160…`);
  - A2: Lever 2 (`651c142`: `adage` `1b78f10d…`, `adage-exec` `da266bc1…`, card checks off);
  - A3: Levers 1+2 (the same binaries, card checks on).

  Five safety controls each run in A2 and A3 (failed, later-edit, config-change, broader, rerun):
  34 sessions in total, with a $6.50 study ceiling.
- **Registration:** every hook arm uses Claude Code's production hook set: SessionStart,
  UserPromptSubmit, PostToolBatch, PostToolUseFailure, PreToolUse for Bash, Stop and SessionEnd.
- **Primary metric:** model requests per session. The development thresholds D1-D4 are registered
  in `launch_gate_protocol.json` (`dev_thresholds`).
- **Grants:** `approve-dev-arms.sh` (sha256 `2290b18c…`) is person-run only. It covers 28 homes:
  A1-A3, each granted with its own arm's binary; A0 holds none.
- **Isolation:** `isolation-audit.json` passed.
- **Offline dry run** (scratch directory, no grants, no ledger): the card reached A1-A3. Only A3's
  card named the test command (`.venv/bin/python -m pytest tests/test_validate.py -q`, Python
  `.venv/bin/python`). Without a grant there was no contract and the Stop gate stayed silent.
- **Superseded:** `dev-v8-superseded/` was frozen minutes earlier. That dry run found that one
  arm's card-checks setting could carry over within a recorder process, so the root was superseded
  before preparation, with no spend.
