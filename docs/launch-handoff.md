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

The user authorized the remaining Codex allowance; the final check reports **99% used,
1% remaining**. This is the budget handoff. The independent paid evaluation ledger retains
its $60 ceiling: **$15.6474504 measured + $0.50 conservative charge = $16.1474504**, across
**100 attempts**, with **$43.8525496 remaining**. All attempts are settled or cap-charged;
accounting is unblocked and no paid process remains active. Read the live ledger before any
new dispatch; never reset it, double-dispatch, or treat an unknown bill as zero.

## Latest checkpoint: launch gate protocol (resume here)

Branch `codex/launch-first-reflex`, PR #16 (draft).

Ledger: 163 attempts, all settled or cap-charged.
- Admitted: $24.6504369 ($24.1504369 measured plus the $0.50 conservative charge).
- Remaining: $35.3495631.

New charges in this checkpoint:
- the development canary-v8 `dev-v8b` (34 sessions, $4.3539376);
- the passing `launch-gate-v5` native session ($0.0281286);
- the development canary-v7 ($1.3425845);
- three excluded native-confinement sessions: `launch-gate-v1` ($0.0258378), `launch-gate-v2`
  ($0.0278896) and `launch-gate-v3` ($0.0140024);
- the passing `launch-gate-v4` native session ($0.0143733);
- the development canary-v5 ($2.6962327). The frozen cohort and
canary manifests bind absolute harness paths in the agent worktree
`/Users/stevenbooth/Documents/skill-compile/.claude/worktrees/agent-ac9ae5e561a996d9d`; run the
commands below from its `evals/reflex-proof/harness`, or refreeze new roots from another checkout.

- [Launch gate protocol](launch-gate-protocol.md) (`launch-gate-1`, revision 8) and its governing
  JSON (`evals/reflex-proof/harness/launch_gate_protocol.json`) were registered before any dispatch:
  task splits with near-duplicate groups, arms, paired blocks, budgets and the GO thresholds G1-G9.
  `launch_gate_score.py` applies those thresholds; its tests pin every failure mode.
- `receipt_replay.py` rebuilds receipt consumption from native hook records. On the frozen
  transfer-v3 sessions it reproduces 0/4 eliminations: 6 repeats and 9 narrower node runs on
  unchanged source, the same routine as the no-execution arm
  ([findings](../evals/reflex-proof/harness/results/20260930-receipt-replay/FINDINGS.md)).
- transfer-v3's tasks were excluded from training by task id only. They had run in the cold
  pilots, and three are near-duplicates of training tasks. Its result is same-repository transfer
  on familiar task shapes; the protocol states that caveat.
- `split_audit.py` checks training isolation before dispatch (corpus, fitted state, examples,
  foreign state, forbidden session ids from every non-training record, development/evaluation
  markers). The protocol lists what the existing pipeline already enforced and what is new.
  `cumulative-launch-10` passes it.
- **canary-v5 ran (development).** It passed integration and safety: the contract reached all
  12 native transcripts, all 7 successful receipts were structured, every realized failed, edit,
  config, broader and rerun control kept necessary work, and quality was 12/12. **It gives no
  elimination evidence.**
  - Both positives got failing receipts. The context fired at the natural end of the work, but
    the agents' own first-draft tests were wrong, so the follow-up work was necessary.
  - After successful receipts, agents still repeated. A bare `python` attempt drew Adage's
    interpreter advice, and the agents ran its runnable form: a node run, then an exact repeat
    (`later-edit-2`), or an equivalent repeat (`broader-1`).
  - The report's `passed` is the integration and safety flag, not a hypothesis result
    ([findings](../evals/reflex-proof/harness/results/20260930-launch-canary/canary-v5/FINDINGS.md)).
- **Revision 6 (development).**
  - Test calls covered by a fresh, successful receipt from the same session now get a receipt
    note ahead of any interpreter advice. Freshness means inputs byte-identical by the executor's
    recomputed snapshot. The note is context only and never a refusal.
  - Pytest receipts state their scope plainly.
  - `launch_canary.py` (`launch-canary-7`) is frozen at `cohorts/launch-gate-v4/canary-v7`: six
    positives plus controls, with a fixed, validated test block in every prompt, so a successful
    receipt is likely by construction. Its report separates `integration_safety_passed` from the
    positive rows ([status](../evals/reflex-proof/harness/results/20260930-launch-canary/FINDINGS.md)).
- **Confinement fix (revision 3).** The tool-environment denial list used to be computed from
  the shell that prepared a session, while Claude inherited the shell that launched it. No canary
  tested environment denial. The first canary attempt exposed this: `launch-gate-v1`'s native
  session was frozen in an agent environment and run from the user's terminal. That session is
  excluded as confinement evidence, and `canary-v2` aborted before preparing anything. Claude
  now starts from an allowlisted environment plus a random sentinel. The denial list is fixed,
  launch refuses any passed name that is neither plain nor denied, and the native canary checks
  environment denial. A names-only [audit](../evals/reflex-proof/harness/results/20260930-environment-denial-audit/FINDINGS.md)
  of the 101 launched archived sessions found no environment dump or Python environment access,
  but it cannot verify launch-time denial. Earlier confinement claims cover the filesystem and
  network only.
- **Runtime-injected names (revision 4).** The fixed runner's `launch-gate-v2` native review
  failed closed. Claude Code itself gives tool children the sandbox proxy credential
  (`CLOUDSDK_PROXY_PASSWORD`) and the session inbox child token (`CLAUDE_CODE_MESSAGING_TOKEN`).
  A tool able to reach the inbox could inject messages into the agent's session. Both names are
  now on the denial list. The canary requires that a tool cannot connect to the inbox socket, and
  it probes whether the proxy refuses external requests. A surviving name is tolerated only by
  exact name and only with its proof passing in the same run. `launch-gate-v2` is excluded.
- **Revision 5.** The `launch-gate-v3` session confirmed the inbox was unreachable, the messaging
  names were removed and the proxy refused external requests. It also exposed a review parser that
  crashed on Claude Code's appended `<sandbox_violations>` block, and a canary program whose own
  denied-name check contradicted the exemption. Both are fixed under a single rule,
  `probe.proven_harmless`. `launch-gate-v3` is excluded and never reviewed. An offline dry run of
  the fixed review on its archived output passes at the content level (a fixture, not evidence).

- **canary-v7 ran (development). Elimination hypothesis: not supported.** All six positives had
  a fresh successful receipt. In every one, the model then requested covered verification of its
  new tests by name. The original report's 4 "eliminations" were bare `python` calls that failed
  to launch in the sandbox; the post hoc scorer-v2 re-score finds 0
  ([findings](../evals/reflex-proof/harness/results/20260930-launch-canary/canary-v7/FINDINGS.md)).
  The PreToolUse receipt note arrives with the call, so it cannot save that turn. Both later-edit
  controls were unrealized, and no necessary work was skipped.
- **Revision 7**: scorer version 2, a realistic project environment, test node ids and counts in
  pytest receipts (from the declared pytest cache record, bound to the journal), and the later-edit
  control by construction. PreToolUse result substitution is out of scope: the user's goal is
  skipping inference, not intercepting CPU. canary-v8 is designed in `launch_canary.py`
  (`launch-canary-8`) but **not frozen**, awaiting the next design direction. Freezing it needs a
  new native cohort, because the runner sources changed.

- **Revision 8: fewer model requests.** No hook can skip a mid-turn model request, so savings
  can only come from the agent needing fewer requests.
  - **Lever 2 (production code):**
    - a short factual contract naming the approved command and that Adage runs it again at finish;
    - compact receipts (command, result, exit, time, short id, tests, freshness), with no header,
      digests or origin footer;
    - failed learned verifications delivered with their failing tests;
    - a Stop gate that runs a stale or never-passed approved verification when the agent finishes
      and continues the agent once on failure (at most two runs).
  - **Lever 1:** the card names the certified test command.
  - **Not done:**
    - discovery prefetch is skipped, because no learned lookup routes exist for this family;
    - firing at the tests edit is left out, because it would change the approval key's input
      scope.
  - **Development canary-v8** (`dev_arms.py`, `cohorts/launch-gate-v5/dev-v8b`) compares A0 (no
    hooks), A1 (revision-7 binaries), A2 (Lever 2) and A3 (Levers 1+2) in six paired blocks, plus
    A2/A3 safety controls, using the production hook registration and thresholds D1-D4.

- **dev-v8b ran, and it is invalid for its execution arms from session 09. It is not a valid
  negative result** ([findings](../evals/reflex-proof/harness/results/20261001-dev-arms/FINDINGS.md)).
  All 34 sessions were collected, for $4.3539376.
  - **What went wrong.** From session 09, every A1-A3 session ran with a lapsed grant: the spool
    shows `withheld: approval` for `route_exec_digest`. Those sessions had no receipts and no Stop
    gate, and from session 10 they had no contract. Only 7 of the 18 hook-arm positives (01-06, 08)
    ran the design.
  - **Cause.** A concurrent PR review ran the base environment's pytest unsandboxed and without
    `-B`. That wrote bytecode into the shared runtime's standard library at 05:58:05Z, and the
    approved interpreter identity includes that library.
  - **What the report computed.** Mean requests per positive: A0 16.8, A1 22.5, A2 17.8, A3 18.7.
    No arm met D1, but these means mix working and card-only sessions, so they test neither lever.
    No arm qualified, so held-out does not run on this design. A1 was notably worse than A0. The
    Stop gate never ran.
  - **No safety claim.** All ten controls ran after the lapse, and none realized its condition. D3
    was never tested.
  - **Post hoc audit** (`dev-v8b/posthoc-audit.json`). Requests equal the debug-log dispatches in
    34/34 sessions, and no session was censored.
  - **Harness, prospective.** Reports now fail loudly on:
    - a hook-arm session with no contract, a withheld approval or no receipt;
    - unverified or censored counts;
    - unrealized controls.

    `prepare` makes the shared runtime read-only. Before every dispatch, `live` re-checks it, and
    checks each grant by running the session's own binary at SessionStart. Any lapse stops the
    study. The product-side digest fix and a lapse notice a person can see are on
    `codex/ship-fixes`.
  - **No new design has started.**

Blocked on three things, none of which an agent may bypass:

1. **Paid runs need a normal terminal.** `probe.execute` refuses nested Claude sessions.
2. **Grants need the person.** The person issued the dev-v8b grants (`approve-dev-arms.sh`,
   sha256 `2290b18c…`); no grant is pending.
3. **Learning needs the learner fix.** About 22 learning-quality tests fail on main since PR #14.
   Cohort training replay, checkpoints and frozen learned homes wait for that fix and a rebuild.

Development canary-v8 commands (all run on 2026-10-01 and kept for the record; never rerun an outcome), from `evals/reflex-proof/harness` in a normal terminal:

```sh
A=/private/tmp/adage-real-repo-gate-v1; C=$A/cohorts/launch-gate-v5; R=$C/dev-v8b
python3 real_repo_gate.py run-session $C/sessions/native-confinement/manifest.json   # paid, <= $0.50 cap
python3 curve_sessions.py review-native $C                                           # offline; must pass
python3 dev_arms.py prepare $R                                                       # offline, ~15 minutes (34 checkouts)
less $R/approve-dev-arms.sh && sh $R/approve-dev-arms.sh                             # the person issues 28 grants
python3 dev_arms.py verify-grants $R                                                 # read-only
python3 dev_arms.py live $R                                                          # paid, 34 sessions, $6.50 study ceiling
python3 dev_arms.py report $R
```

After the canary: if it passes, prepare the cohort (author and validate the new Marshmallow,
Click and more-itertools tasks, then learning once the fix lands). If it fails, diagnose on its
records, fix the integration, and freeze a new canary root; never rerun an outcome.

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
  a separate open gate. The completed real-coding study ran the learned CPU reflex successfully
  in 4/4 automatic sessions, with 4/4 correct outputs versus 3/4 baseline. **0/4 confirmed action
  eliminations:** every automatic session requested the same command again after its receipt.
  GPU utilization and energy were never measured.

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

**Completed:** all eight `transfer-v3` assignments are collected and settled. Executor session
58750 has exited; `/private/tmp/adage-transfer-v3-run.json` contains the run output. **Do not
dispatch this study again.** The result and diagnosis are in
`evals/reflex-proof/harness/results/20260930-live-cpu-transfer/FINDINGS.md`.

To inspect the existing completed study, from the worktree run:

```sh
python3 evals/reflex-proof/harness/curve_transfer.py report \
  /private/tmp/adage-real-repo-gate-v1/cohorts/curve-guided-v2/transfer-v3
```

The archived report and export script preserve the original decision even after future harness
changes. Do not overwrite frozen roots, their task material, histories or binaries. Any new paid
study needs its own prospective protocol and assignments; do not reuse these outcomes as unseen.

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

### Latest checkpoint: receipt integration and evaluator isolation

PR #16 remains draft. There are no paid processes running. Shared ledger now has **100
attempts**, $15.6474504 measured plus the previous $0.50 conservative charge =
**$16.1474504 admission spend**, **$43.8525496 remaining**. Accounting is unblocked.
The latest diagnostic work cost $0.5245263 including failed/excluded attempts.
Final Codex allowance check: 99% used (1% remaining); the user authorized the remainder.

Research: [hook action reuse](hook-action-reuse-research.md). Native Read delivery canaries
all passed ($0.0288169), archived in `results/20260930-delivery-canary` under the harness.
Both context and valid result replacement delivered random markers; malformed replacement
and no-result controls did not. This verifies native delivery, not production action avoidance.

A **real evaluator bug** was found during the edit diagnostic: Claude inherited launcher
stdin and one agent reproduced the launcher Python program as an attempted command.
`probe.execute` now uses `stdin=DEVNULL`; the intended prompt is already in argv. A real
child-process regression fails on the old implementation and passes after the fix. All 85
harness tests pass. Preserve `receipt-edit-v1` as contaminated; the earlier heredoc-launched
read comparison `receipt-v3` is also excluded from behavioral conclusions. Do not promote
its apparently positive rates. `receipt-v2` previously stopped on a navigation/permission
confound and remains excluded. All outcomes and frozen code are archived.

Clean fresh `receipt-edit-v2` ran eight assignments after stdin isolation, $0.1513985 total:

| Arm | Agent requested verifier | Independent quality | Successful CPU runs |
| --- | --- | --- | --- |
| Baseline | 2/2 | 2/2 | 0 |
| Current-style receipt | 2/2 | 2/2 | 2 |
| Structured receipt | 1/2 | 2/2 | 2 |
| Structured receipt + startup responsibility | 0/2 | 2/2 | 2 |

Archive: `evals/reflex-proof/harness/results/20260930-edit-receipt-ablation`. This is an
authored function-fix/changelog fixture, not learned real-repository evidence. Two repetitions
per arm are a diagnostic signal, not a reliable effect estimate. Current-style text was labeled
simulated; acknowledge that difference. No new launch-site statistics were added.

The candidate production integration is now implemented in the existing pipeline:

- `Extension::session_context` supplies a bounded model-facing responsibility contract only
  in treated supported sessions with a scoped E2 verification approval and E2 policy enabled.
  A stored grant is not a promise of execution; execution still rechecks the actual request.
- SessionStart keeps model context separate from the person's notice; muting notices does not
  mute the contract. Control, damaged-index, no-extension and no-grant paths remain silent.
- Successful learned CPU actions deliver journal operation ID, declared-input digest, exit
  status and duration, with explicit scope and later-change caveats. Cache-only writes are
  no longer described as no repository changes. No raw stdout is newly exposed.
- The contract uses existing provenance tags. Learning conservatively treats the continuation
  as guided from session start, rather than learning its own instructions as organic evidence.
  This reduces eligible organic training while approved verification is enabled; control
  sessions remain important. Do not silently relax this contamination boundary.

Local validation: all 24 CLI tests across `first_run`, `zero_config_plugin` and `exec` pass.
Execution/runtime/hook library suites pass; startup treatment/control,
notice separation, no-grant/E1 policy, real journal receipt identity and learning provenance
regressions pass. These tests do not establish a live effect from this exact production text.
The diagnostic supplied fixture-specific test counts; production receipts intentionally do not
expose undeclared stdout or invent test counts. That distinction needs prospective validation.

The next agent should:

1. Run a production-binary SessionStart/receipt canary, then stale-input, failed verification,
   changed environment, broader scope and explicit-rerun controls. Do not implement command
   denial to disguise repeats; no permission or execution grant may be bypassed.
2. Prepare a NEW real-repository cohort with fresh frozen binaries, replayed original training,
   updated native-confinement verification where source identity requires it, and corrected
   prospective cache-only scoring. The old binary hash allowlist must remain a check: replace
   it only with measured new hashes in the new reviewed protocol. Never overwrite old studies.
3. Obtain new person-issued grants for new evaluation homes after preparation. Do not copy old
   approvals. Freeze the whole matrix before the first paid agent. Keep the shared ledger.
4. Measure repeated tool requests, model requests/tokens/cost, quality and necessary reruns.
   Include a no-execution control. A small authored diagnostic is not a replacement for this gate.

The four old real-repository repeats occur in new assistant messages after the hook attachment,
not in the same message as the triggering edit. Message IDs are not API request IDs. A scan of
all 65 available historical cohort native transcripts (including the eight transfer runs) found no known launcher-code markers; that negative scan does
not prove full historical prompt isolation. Preserve the frozen failure and use the fixed runner
for all new evidence. Research and diagnostics identify a promising integration direction;
open-source launch readiness is still not proven.

The follow-up adds `docs/first-reflex.md`, first-run links to review and execution status, clearer
cache errors, and regression coverage for pytest's hidden metadata. It preserves the current TUI.
Do not create another learner or parallel onboarding implementation.

1. Address the observed duplicate-work failure. Native transcripts confirm that the CPU completion
   reached the model as `hook_additional_context`, yet every automatic arm requested the exact test
   again (2/1/1/1 times). Build an offline regression from these events, investigate how to present
   completed verification with explicit scope and freshness, and measure action avoidance separately
   from merely suppressing a subprocess after the model has already requested it.
2. Before a new paid study, fix receipt scoring prospectively: approved cache-only changes currently
   fail the no-changes receipt filter despite unchanged final source digests. Predeclare appropriate
   pytest flag equivalences. Preserve this failed result; a scoring correction alone cannot erase
   the actual duplicate agent requests. See the post-hoc delivery and runtime diagnostics.
3. Walk the first-reflex guide on a fresh supported installation. Verify hook registration, history
   discovery, empty-history behavior, candidate review, policy/grant setup, result delivery and revoke.
4. Improve naturally occurring history coverage; current organic-versus-guided gap is material.
   Measure unnecessary executions, duplicates, abstention and drift along with saved actions.
5. Run release platform CI before claiming all advertised binaries and harnesses work. Local macOS
   success is not Linux or Windows evidence; Windows automatic execution is unsupported.
6. Replicate on another repository/model before treating the pilot as a general savings estimate.
   Frontier-to-cheaper-model transfer and a marketplace remain future hypotheses.

Do not promise a universal session count for learning or guaranteed savings from `init`. The launch
pitch should show a concrete demonstrated workflow, publish its limits, and invite developers to
contribute histories and reproducible reflex cases.
