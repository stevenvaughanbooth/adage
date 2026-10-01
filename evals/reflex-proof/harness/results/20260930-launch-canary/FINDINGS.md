# Launch canary: canary-v5 ran; canary-v6 frozen, not dispatched

All of this is development-split work: no held-out tasks are involved, and none of it is evidence
for the hypothesis.

- **canary-v5** (`cohorts/launch-gate-v4/canary-v5`, protocol `launch-canary-5`) ran all 12
  sessions for $2.6962327. Delivery and safety passed; both positives were unrealized; there were
  0 eliminations. See `canary-v5/FINDINGS.md`.
- **canary-v6** (`cohorts/launch-gate-v4/canary-v6`, protocol `launch-canary-6`) is frozen at
  commit `11eb615`:
  - binaries: `adage` `2b04bbf3…`, `adage-exec` `3895b20f…`, which include the receipt note;
  - 12 sessions on `mm-curve--finite`: six positives, two later-edits, and one each of failed,
    config-change, broader and rerun;
  - every prompt supplies the same validated test block (`canary-v6-test-block-check.*`);
  - `isolation-audit.json` passed;
  - `approve-canary.sh` sha256 `8878e159…` is person-run only.
- The cohort `cohorts/launch-gate-v4` native review passed. It remains valid for canary-v6: the
  runner, probe and helper sources are unchanged. Native files: `native-confinement-*.json`,
  `native_canary.py`.

Superseded or aborted attempts, preserved:

- `canary-v1-superseded-manifest.json`: zero-spend freeze on training-pool tasks (revision 2).
- `canary-v2-aborted/`: environment denial depended on the shell. The `launch-gate-v1` native
  session ($0.0258378) is excluded as confinement evidence. The canary was never prepared.
- `canary-v3-aborted/`: the `launch-gate-v2` native session ($0.0278896) passed the file, network
  and sentinel checks. Its review failed closed on two names that Claude Code itself gives tool
  children (`CLOUDSDK_PROXY_PASSWORD`, `CLAUDE_CODE_MESSAGING_TOKEN`). That review is immutable and
  excluded, and the canary was never prepared. See `../20260930-environment-denial-audit/FINDINGS.md`.
- `canary-v4-aborted/`: the `launch-gate-v3` native session ($0.0140024) never got a review. The
  review parser crashed on the appended `<sandbox_violations>` block, and the canary program
  exited 1 because its own denied-name check contradicted the proven exemption for the sandbox
  proxy credential. The session is excluded and the review now refuses that cohort. The archived
  tool result (`native-canary-tool-result.json`) is a test fixture. `dry-run-review.txt` shows the
  full review run offline under the fixed code, without writing: the content verdict passes, and
  only the old program's own flag and exit status fail.
