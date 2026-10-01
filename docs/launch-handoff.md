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

## Current decision, 2026-10-01 (resume here)

**The held-out Marshmallow study ran, and its outcome is FAIL on quality. The execution-mode
launch gate is NOT met.**
- **Record:** [`results/20261001-heldout-marshmallow`](../evals/reflex-proof/harness/results/20261001-heldout-marshmallow/RESULT.json).
  It holds the decision record `report.md`, `report.json`, all 65 collections, the run log, and the
  freeze records with frozen-source verification.
- **Protocol:** `heldout_results` in the protocol JSON, and "Held-out result: Marshmallow stratum" in
  [the protocol](launch-gate-protocol.md).

| Registered measure | Result | Holds |
| --- | --- | --- |
| Primary: BEST/A0 model requests (upper bound must be ≤ 0.90) | 0.6741, upper bound 0.7265; 18 of 20 blocks fewer; p = 0.0001 | yes |
| Quality: BEST ≥ A0 − 1 hidden-test passes | BEST 17/20, A0 19/20 | **no** |
| G5: BEST failures ≤ min(A0, NOEXEC) | BEST 3, A0 1, NOEXEC 1 | **no** |
| G1, G2, G6, G7, G8 | as registered | yes |
| Five safety controls | all realized; none skipped necessary verification | yes |

- **The study.** `cohorts/launch-gate-v8/heldout-1` ran 65 of 65 sessions, and the report is valid:
  - no lapses, censoring or excluded blocks;
  - the frozen sources equal the runner commit `f2a2a09`.

  BEST was A4: verification ownership, the orientation card and learned CPU execution.
- **Where quality failed.** b05 has-keys failed in all three arms. BEST alone also failed b08
  lat-long and b09 has-keys. A separate diagnosis is running.
- **Report only** (pre-registered, not gated, and not a pass for any arm):
  - NOEXEC/A0: 0.7845, upper bound 0.833, quality 19/20. NOEXEC is the learned card and instincts,
    with no execution.
  - BEST/NOEXEC: 0.8593, upper bound 0.9197.
- **Spent tasks.** has-keys, iso-date, json-object and lat-long are now development tasks. A
  confirmation needs new evaluation groups, registered before dispatch.

**Decision (the user's messaging decision):**
- Launch the developer preview leading with **advisory mode**: the card and instincts, no
  execution.
- **Execution mode is opt-in and labelled experimental and unproven.**
- **What advisory mode does not have.** Its contrast was report-only here, so it has no registered
  pass and no launch statistic.
- **Never present model requests as GPU savings.**

**Next, in order:**
1. **Diagnose the quality misses** on the archived records. Start with BEST's b08 lat-long and b09
   has-keys, against the same blocks in A0 and NOEXEC.
2. **Fix, and validate the fix in development** on development tasks only. The four spent tasks
   are development now.
3. **Run a fresh confirmation powered for quality.** It needs new evaluation groups and must be
   registered as a new protocol revision before any dispatch. Use a new native cohort from a
   fresh clone outside iCloud, and person-issued grants.
4. **Then the expensive-to-cheap transfer study.**

**Ledger after the held-out study:** 284 attempts (283 settled, 1 cap-charged), nothing open,
nothing blocked.
- Admitted: $39.8915551 ($39.3915551 measured plus the $0.50 conservative charge).
- Remaining of $60: $20.1084449.
- Held-out: $9.884438 for 65 sessions. The `launch-gate-v8` native session: $0.0276484.
- The ceiling stays $60, and the $3 floor applies. Any further allocation is set separately by
  the user.

**Where things are:**
- **Branch.** `codex/launch-first-reflex`, PR #16 (draft). Work and commit in the clean clone
  `/private/tmp/adage-heldout-src`, which is outside iCloud, then push.
- **The agent worktree** under `~/Documents` is iCloud-synced. Fast-forward it only.
- **Ledger and authority.** `/private/tmp/adage-real-repo-gate-v1`: `paid-state.json` and
  `evaluation-allocation.json`.
- **Study roots.** Never modify or rerun them:
  - `cohorts/launch-gate-v6/dev-v9b`, bound to `/private/tmp/adage-launch-src`, which must not be
    edited;
  - `cohorts/launch-gate-v8/heldout-1`, bound to `/private/tmp/adage-heldout-src` at `f2a2a09`;
  - `launch-gate-v7`: excluded, never dispatched.
- **Held-out freeze records:**
  - manifest `a508a425…`;
  - `approve-heldout.sh` `3213ff49…`;
  - `run-heldout.sh` `ff092c8f…`;
  - binaries `adage` `63475c2e…` and `adage-exec` `c77b9859…`, built at `8770f8c`.
- **Rules:**
  - agents never issue grants;
  - paid runs go from a normal terminal (the CLAUDECODE guard);
  - no Python from the shared runtime or bases outside the study runner;
  - kill only your own PIDs;
  - refuse dataless inputs;
  - never change thresholds, splits or reports after dispatch.

## Launch gate protocol checkpoints (history)

Branch `codex/launch-first-reflex`, PR #16 (draft).

Ledger before the held-out study: 218 attempts, all settled or cap-charged.
- Admitted: $29.9794687 ($29.4794687 measured plus the $0.50 conservative charge).
- Remaining: $30.0205313.

Charges, newest first:
- the held-out study `launch-gate-v8/heldout-1` (65 sessions, $9.884438) and the `launch-gate-v8`
  native session ($0.0276484);
- the development canary-v9b `dev-v9b` (53 sessions, $5.245021);
- the aborted development canary `dev-v9` (1 session, $0.0675211);
- the passing `launch-gate-v6` native session ($0.0164897);
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

### dev-v9b: ran, valid; A4 passed the development rule

**dev-v9b ran to completion and is valid** ([`RESULT.json`](../evals/reflex-proof/harness/results/20261001-dev-v9b/RESULT.json),
with the report and all 53 collected rows beside it). It collected 53 of 53 sessions at a known cost
of $5.245021, with no unknown cost. The report has no problems and skipped no necessary work. Its
26 frozen sources and its copy of the protocol JSON match `91a9714` byte for byte.

**What it is.** A development result on one task (`mm-curve--finite`), with development
scaffolding: every prompt carries a supplied test block and a work order. The design was iterated
on this same task (canary-v5, canary-v7, dev-v8b, dev-v9). It selects BEST for held-out under r9.
**It is not a launch statistic, held-out evidence or a savings estimate.**

| Arm | Mean requests per positive | Ratio to A0 | Blocks fewer than A0 | Verdict |
| --- | --- | --- | --- | --- |
| A0 | 18.0 | | | |
| A2f | 15.06 | 0.837 | 13/16 | not eligible (no controls) |
| A4 | 11.19 | 0.6215 | 15/16 | passed D1-D4 |

- **D2 to D4.**
  - D2: there were no hidden-test failures in any arm (16/16 each).
  - D3 was tested. All five A4 controls were realized, and none skipped necessary verification.
    The failed, later-edit and config-change controls were met by a passing Stop-gate run on the
    final sources; the broader and rerun controls by the agent's own runs.
  - D4: sessions with repeat attempts were A0 0, A2f 2, A4 1.
- **BEST = A4, with the orientation card on.**
- **Reported, not gated** (A0 against A4, per positive):
  - cost: $0.117 against $0.0825;
  - pytest requests: 2.81 against 0.19;
  - cache-read tokens: 445k against 245k;
  - wall time: 57.7 s against 50.5 s.

Model requests are not GPU measurements, and CPU runs are never counted as inference saved.

**dev-v9 was aborted after its first session.** The stop rule ended the study at order 0, the
failed control, which it reported as "not realized". That was a scoring bug, not a product
failure ([`results/20261001-dev-v9`](../evals/reflex-proof/harness/results/20261001-dev-v9/ABORTED.json)).
- **What the session did.** The context run failed and its failure was delivered. The agent fixed
  the code and stopped without testing, and the finish-line gate passed on the final sources.
- **The bug.** Version 2 of the scoring counted the gate's pass as a successful context run.
- **The fix (revision 11).** It was registered before any positive block ran, and the thresholds
  are unchanged:
  - journal receipts now record their origin;
  - `receipt_replay` version 3 takes necessity from context runs, and lets a gate pass on the
    final sources meet it;
  - realization is judged on context runs;
  - gate runs are reported apart.
- **Cost.** The dev-v9 session cost $0.0675211, and the `launch-gate-v6` native session cost
  $0.0164897. The ledger now stands at 165 attempts, $24.7344477 admitted and $35.2655523 remaining.

Development canary-v9b (`harness/dev_v9.py`, protocol `dev-arms-2`, revisions 10 and 11) is
frozen at `cohorts/launch-gate-v6/dev-v9b`. Records are in
[`results/20261001-dev-v9b`](../evals/reflex-proof/harness/results/20261001-dev-v9b/).
- **Source.** It was frozen from a fresh clone outside iCloud, **`/private/tmp/adage-launch-src`**,
  at `91a9714`, and the binaries were built there: `adage` `d7af0715…`, `adage-exec` `87e1835b…`.
  The build includes `codex/ship-fixes`, `codex/first-run-fixes`, `codex/heldout-tasks` (r9), the
  orientation card, the receipts that name the session's new tests, and receipt origins.
- **Running it.** The manifest binds the clone's paths, so run every command from
  `/private/tmp/adage-launch-src/evals/reflex-proof/harness`. Never edit, pull or rebuild in that
  clone.
- **Unchanged from dev-v9.** The arms, matrix, prompts, budget and `dev_thresholds_v9` are
  identical to dev-v9's frozen manifest:
  - A0: no hooks.
  - A2f: verification ownership, with the card as shipped before r10 (exemplar).
  - A4: A2f plus the orientation card.

  The five A4 controls run first, then 16 paired blocks, for 53 sessions. Each session is capped at
  $0.50, the study ceiling is $8.50 and the ledger floor $3. Every prompt carries "Do not create
  git commits."
- **The rule for A4:**
  - a valid study;
  - mean requests at most 0.80 times A0's, and fewer than A0 in at least 10 of 16 blocks;
  - D2, D3 (all five controls realized) and D4 (at most 5 of 16).

  A2f is reported only. BEST follows the r9 rule. If A4 does not pass, held-out does not run.
- **Native confinement is reused.** The `launch-gate-v6` native review passed and still applies.
  The fresh clone is at the same path, and the runner sources the review binds (`probe`,
  `real_repo_gate`, `real_repo_pairs`, `cohort_v5`, `budget`, and the cohort files) are
  byte-identical there. I checked this, and `require_native_review` passes for dev-v9b. No new
  native session is needed.
- **Guards.**
  - `prepare` makes the shared runtime library read-only.
  - Before every dispatch, `live` re-checks it and the session's grant at SessionStart.
  - `live` stops on a lapse or an unrealized control, now judged on context runs.
  - `report` exits non-zero if the study is invalid.

**Rule for every agent and every review:** nobody runs Python from the study's shared runtime
(`/private/tmp/adage-real-repo-gate-v1/runtime`) or its base environments (`bases/*/.venv`), except
the study runner itself (`prepare`, `live`, `collect`, `report`). Reviews and diagnoses read files.
They never run those interpreters, not even `-m pytest` on a scratch directory. dev-v8b lost 20
sessions this way.

**iCloud warning.** This repository lives in an iCloud-synced Documents folder with Optimize Storage
on. On 2026-10-01, 1,009 files in this worktree and one git pack (`pack-0eaf922f…`) had been evicted
to dataless placeholders. A sandboxed tool reads such a file as empty: a test file read that way
runs 0 tests, and an `include_str!` asset (the intent vocabulary) builds empty. Studies therefore
freeze and run from the clone in `/private/tmp`, and freeze and dispatch refuse dataless inputs.

### Held-out phase: frozen, run and reported (history; the result is at the top)

**This section records preparation only.** The outcome (FAIL on quality) is under "Current
decision" above.
- **The native-admission blocker was resolved by r12.** The user approved the change and applied
  it in `cohort_v5.py` (`f2a2a09`).
- `launch-gate-v7` is excluded and was never dispatched. `launch-gate-v8`'s native session passed
  review.
- `heldout-1` was frozen in v8 at `f2a2a09` with the binaries below.
- The person ran `run-heldout.sh` from their own terminal. It ran the native session, the review,
  prepare, the 25 grants, verify-grants, live and report. Its log is archived.

The text below is kept as it was written before the run.

dev-v9b's report is valid and A4 passed `dev_thresholds_v9`, so BEST is A4 with the orientation
card, by r9's rule.

**Done offline on 2026-10-01** (record: [`results/20261001-heldout-inputs/INPUTS.json`](../evals/reflex-proof/harness/results/20261001-heldout-inputs/INPUTS.json)):
- **Binaries** were built in `/private/tmp/adage-heldout-src` at `8770f8c` with
  `cargo build --release --locked -p adage-cli -p adage-exec`:
  - `adage`: `63475c2ee97abf6c3982a37d6c77e4637c5fa52e92612b51a7ca3f3bec913903`;
  - `adage-exec`: `c77b985973f218fd21eb5cc589f50a46fad0ec3840ac7027ef1d2b29dc12636c`.

  The production sources are unchanged since the main merge (`32e1cfa`). A later harness-only
  commit keeps them bindable, because `freeze` checks the crates and lockfile, not HEAD.
- **Held-out controls are validated on the real substrate** (`$A/heldout-controls-1`).
  - The validation is complete and valid, with 0 paid calls.
  - The validation ran only its own copies:
    - every venv home and the manifest interpreter are inside the copy;
    - no scoring profile allows the authority's runtime;
    - the shared runtime library is unchanged (`7ef262dc…`) and still read-only.
- **Native cohort `launch-gate-v7`** is prepared from this clone (`prepare-cohort` and
  `prepare-session`, offline). It has not been reviewed or run.
- **A read-only freeze preflight passed on the real inputs**
  (`results/20261001-heldout-inputs/freeze-preflight.py`). It wrote nothing:
  - design A4, with the orientation card;
  - binaries bound to `8770f8c`;
  - the controls match the frozen tasks, base and dependencies;
  - the training-isolation audit passed;
  - 65 sessions, 25 of them BEST, with a $26 worst case.

**Blocker: the native session cannot be admitted.**
- **Why.** `cohort_v5` native recipes carry no `budget_allocation_sha256`, so `run-session` admits
  them under the legacy $27.50 ceiling (`real_repo_gate.budget_limit`). The ledger has already
  admitted $29.9794687. Every study recipe binds the person's allocation and its $60 ceiling, but
  this recipe does not.
- **What happens if it is run.** The refusal happens before dispatch and writes nothing, but its
  message is "a reported cost exceeded its reserved cap; stop and reconcile". That message is
  misleading here. **Do not reconcile anything, and do not run v7's native session.**
- **Proposed remedy, which needs the user's approval.** New cohorts would bind
  `evaluation-allocation.json` into their native recipe, as every study recipe already does. The
  recipe would also list the allocation among its shared sources, so a changed allocation refuses
  dispatch. Tests would cover this.
  - The $0.50 session cap and the $3 floor are unchanged.
  - Old cohorts keep the $27.50 fallback.

  The auto-mode classifier stopped the agent from making this change on its own, as a weakening of
  a spending guard. With approval, an agent would:
  1. commit the change;
  2. mark `launch-gate-v7` superseded (its recipe binds the old `cohort_v5.py`; nothing was
     dispatched);
  3. prepare `launch-gate-v8`;
  4. freeze `heldout-1` in it;
  5. write `approve-heldout.sh`.

  Because `cohort_v5.py` is a held-out source, the held-out freeze waits for this decision.

**Done earlier:**
- **`origin/main` is merged** (PR #21's learner fixes, the bench re-bless, the `no_spawn`
  allowlist, and `adage update` from #17 and #18). The merge was made in a clean clone outside
  iCloud, `/private/tmp/adage-heldout-src`. The workspace suite and the harness suite were rerun
  there.
- **An offline dry run of the whole freeze pipeline passed** (`harness/heldout_dryrun.py`):
  - **Inputs.** It used a scratch copy of the authority, a scratch cohort with a synthetic review
    marked `dry_run`, held-out controls validated on a copy of the substrate, a copy of the learned
    home, and a simulated `dev-arms-2` report.
  - **Results.**
    - `freeze`: 65 sessions, BEST = A4 with the orientation card, $0.40 cap, $26 worst case.
    - `prepare`: 65 assignments; the scratch runtime was made read-only.
    - `approval-script`: 25 BEST homes.
    - `verify-grants`: it failed only on the 25 missing person-issued grants, as expected.
    - `report`: an INCOMPLETE decision record, as expected with no sessions.
  - **Two runner bugs fixed, with tests:**
    - `prepare` recomputed the held-out tasks' hidden-test identities by running each gold
      environment inside the study sandbox. That environment is bound to its validation's own
      runtime copy, which the sandbox cannot read, so it failed. `freeze` now copies the identities
      from that validation and checks them against the frozen hidden tests.
    - The report's lapse list overwrote the analysis's own `problems`. It is now kept apart, as
      `lapses`.

  The registered r9 to r11 rules are unchanged.
- **The decision record is scaffolded.** `heldout_cohort.py report` now also writes
  `reports/heldout-<time>.md` (`harness/heldout_writeup.py`). Its sections, in order:
  1. the outcome;
  2. the primary bound;
  3. quality;
  4. each G criterion;
  5. the sensitivity analyses;
  6. censoring;
  7. the request cross-check;
  8. the controls;
  9. the report-only contrasts;
  10. what the result does not establish.

  All numbers come from the report.

**Budget (live ledger, 2026-10-01 after dev-v9b).**
- **Ledger.** 218 attempts, all settled. $29.9794687 admitted, $30.0205313 remaining.
- **Worst case.** The native session ($0.50) plus held-out ($26.00: 65 sessions at $0.40) is $26.50.
  That leaves $3.5205313, which is $0.5205313 above the $3 floor. **It fits, and no remedy is
  needed.**
- **Checks before the 65th held-out session, in the worst case:**
  - floor: 60 − (56.0794687 + 0.40) = 3.5205313 ≥ 3;
  - study ceiling: 25.60 + 0.40 ≤ 26.00.
- **Expected held-out spend** is about $7 to $10.

The only budget problem is the native recipe's admission ceiling, described above.

Held-out commands, **after the native-admission remedy is approved and applied** and an agent has
frozen `heldout-1` in the new cohort (assumed here to be `launch-gate-v8`). Run them in a normal
terminal, one at a time, and stop on any error:

```sh
cd /private/tmp/adage-heldout-src/evals/reflex-proof/harness
A=/private/tmp/adage-real-repo-gate-v1; C=$A/cohorts/launch-gate-v8; H=$C/heldout-1
python3 real_repo_gate.py run-session $C/sessions/native-confinement/manifest.json   # paid, <= $0.50
python3 curve_sessions.py review-native $C             # offline; must pass
python3 heldout_cohort.py prepare $H                   # offline, ~30 min (65 checkouts); keeps the shared runtime read-only
less $H/approve-heldout.sh && sh $H/approve-heldout.sh  # the person issues 25 grants (BEST homes only)
python3 heldout_cohort.py verify-grants $H             # read-only
python3 heldout_cohort.py live $H                      # paid: 65 sessions, $0.40 cap, $26 ceiling; controls first; stops on any lapse
python3 heldout_cohort.py report $H                    # writes reports/heldout-<time>.json and .md; exits non-zero if invalid
```

`heldout_tasks.py validate` is done (`$A/heldout-controls-1`); do not run it again.

**Held-out result:** FAIL on quality (see "Current decision" at the top). The decision record is
`results/20261001-heldout-marshmallow/report.md`.

dev-v9b commands. Run them in a normal terminal, one at a time, and stop on any error. `prepare`
runs again for the new root (53 fresh checkouts). The grants must be issued again: the new root's
homes have none, and dev-v9's grants do not carry over.

```sh
cd /private/tmp/adage-launch-src/evals/reflex-proof/harness
A=/private/tmp/adage-real-repo-gate-v1; C=$A/cohorts/launch-gate-v6; R=$C/dev-v9b
python3 dev_v9.py prepare $R                           # offline, ~25 min (53 checkouts); keeps the shared runtime read-only
less $R/approve-dev-v9.sh && sh $R/approve-dev-v9.sh   # the person issues 37 new grants (sha256 ec674a11…)
python3 dev_v9.py verify-grants $R                     # read-only
python3 dev_v9.py live $R                              # paid: 53 sessions, $0.50 cap, $8.50 ceiling; stops on any lapse
python3 dev_v9.py report $R                            # exits non-zero if the study is invalid
```

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
   sha256 `2290b18c…`) and dev-v9b's (`approve-dev-v9.sh`, sha256 `ec674a11…`, 37 homes).
   They also issued held-out's grants (`approve-heldout.sh`, sha256 `3213ff49…`, 25 BEST homes),
   from their own terminal via `run-heldout.sh`.
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

**Marshmallow held-out tasks are done** (branch `codex/heldout-tasks`, revision 9 draft). They were
frozen at `260729a` before any v8 result was read:
- tasks: has-keys, iso-date, json-object and lat-long, with max-sum withdrawn as a size-limit
  near-duplicate;
- validated offline; see `evals/reflex-proof/harness/results/20261001-heldout-tasks/README.md`.

The r9 held-out analysis (`heldout_analysis.py`, A0/NOEXEC/BEST, 4 x 5 paired blocks) and the
runner (`heldout_cohort.py`) are drafted with tests. Both are registered only when merged.

`freeze` needs:
- the dev-v8b report (BEST by rule);
- a validated held-out controls root (`heldout_tasks.py validate`, under the authority);
- the 19-task development controls;
- the build commit and the two binary hashes.

BEST is built after the dev result and after `codex/ship-fixes` and `codex/first-run-fixes` land.
Then: `prepare`, review and run `approve-heldout.sh` (a person), `verify-grants`, `live`, `report`.
The held-out budget is $26 at a $0.40 cap per session.

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
