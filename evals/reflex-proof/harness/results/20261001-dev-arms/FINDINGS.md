# Development canary-v8 (dev-arms-1): invalid for its execution arms from session 09

Development-split work under `launch-gate-1` revision 8, on the development task `mm-curve--finite`
with a guided learned route. Nothing here is held-out evidence or a launch statistic. Recorded
2026-10-01, after the run. No threshold or rule changed.

## Status

**This is not a valid negative result.** From session 09 on, every hook-arm session (A1-A3) ran
with a grant that had lapsed. Those sessions had no receipts and no Stop gate, and, from session
10, no contract. They were card-only sessions under execution-arm labels. Only 7 of the 18
hook-arm positives (01-06 and 08) ran the design.

The `dev-arms-1` report averaged the 7 working sessions with the 11 card-only ones, so its arm
means test neither Lever 2 nor Levers 1+2. The report had no check that would have caught this.

**No arm qualified for held-out**, and held-out does not run on this design. The run is not
evidence that the design fails.

**v8 makes no safety claim.** All ten controls ran after the lapse, none realized its condition,
and none ran with execution. D3 was never tested.

## What the report computed (not evidence for any arm)

All 34 assigned sessions ran once and were collected. The report is `dev-v8b/report.json`; the
three files in the root's `reports/` are byte-identical. Known cost was $4.3539376, with no unknown
cost, and the ledger records all 34 attempts as settled.

| Arm | Requests per positive, blocks 1-6 | Mean | Ratio to A0 | Blocks with fewer requests than A0 | Sessions that ran the design |
| --- | --- | ---: | ---: | ---: | --- |
| A0: no hooks | 17, 16, 15, 24, 15, 14 | 16.8 | | | (no hooks) |
| A1: revision-7 binaries | 16, 14, 30, 25, 30, 20 | 22.5 | 1.34 | 2/6 | blocks 1-2 |
| A2: Lever 2 | 13, 14, 14, 17, 22, 27 | 17.8 | 1.06 | 4/6 | blocks 1-3 |
| A3: Levers 1+2 | 23, 12, 24, 14, 22, 17 | 18.7 | 1.11 | 2/6 | blocks 1-2 |

- **D1 held for no arm.** It needs at most 0.85 times A0's mean with fewer requests in at least 4
  of 6 blocks.
- **D2 and D4 held as computed.** Quality was 34/34, and covered repeat attempts appeared in A1
  2/6, A2 1/6 and A3 2/6 positives.
- **D3 was reported as held, but it was vacuous** (see Status).
- **The Stop gate never ran.** It needs a context run in the session, and none happened after the
  lapse.
- **A1 was notably worse**: a mean of 22.5 against A0's 16.8. Its four highest counts (30, 25, 30,
  20) are blocks 3-6, all card-only after the lapse.

## The lapse: what happened

From session 09 on, Adage withheld the learned verification in every hook-arm session that
triggered it: 20 sessions, all `withheld: approval`, all with the spool reason `exec-changed: the
approval for this route no longer matches (route_exec_digest)`. Only seven sessions ran it (01-06
and 08). All seven applied and covered the final sources.

The contract needs a matching grant, so no session from 10 on received it; session 09 got it at
startup, before the lapse. One control, `29-A3-config-change`, never triggered the route at all.

**Cause.** The approved identity of the project interpreter includes `runtime_sha256` (see
`crates/adage-exec/src/project_python.rs`), a tree hash of the shared runtime's library, and that
hash includes `__pycache__` bytecode. Here is what happened, in order:
- The last applied run was at 05:57:00Z (session 08).
- At 05:58:05-06Z, 126 `.pyc` files were created in the shared runtime's standard library
  (`runtime/python/cpython-3.12.14-macos-aarch64-none/lib`), and another 112 in the Marshmallow
  base environment's site-packages.
- The first withheld run was at 05:58:19Z (session 09).

Before this, the standard library held only three bytecode files, all from 2026-09-30T15:29:40Z,
which is before the grants. Nine sessions and seven receipts had added none.

**Writer.** A concurrent, read-only code review of PR #16 was running on the same machine. At
05:58:03.9Z its tool log shows it running the base environment's `.venv/bin/python -m pytest` on a
scratch directory, outside any sandbox and without `-B`. The study did not cause the writes, and
nothing in the study prevented them.

**Detection.** Failing closed was correct, but it was silent. Neither the agent nor the harness
noticed: the harness checked grants once, before the first dispatch, and never between sessions.
The study ran 25 more sessions in that state.

## Eval-audit checks applied post hoc

`dev-v8b/posthoc-audit.json` was produced by `dev-v8b/posthoc_audit.py`. It is read-only and reads
its rules from the study's own frozen source copy.

- **Dispatch cross-check: 34/34.** In every session, model requests (distinct assistant message ids)
  equal the debug log's dispatched requests, all `source=sdk`.
- **Censoring: none.** Every session ended `success`/`completed` under the 60-turn and $1.50 caps.
- **Control realization: 0/10**, by the frozen `launch_canary.realized`.
- **Frozen sources.** All 23 source hashes in `manifest.json` match commit `5b7fc3a`.

## Harness changes made after this run

These are prospective. dev-v8b is not re-reported with them.

**Reports fail loudly.** `dev_arms.report` and `launch_canary.report` now record `valid` and
`problems`, and exit non-zero when a study is invalid. Any of the following makes a study invalid
and passes no arm:
- a hook-arm session with no contract, a `withheld: approval` (or other grant, policy or sandbox
  stage) or no receipt;
- requests that differ from the debug log's dispatches;
- a censored session;
- an unrealized control.

D3 now needs every control in the arm realized. Collection records `exec_decisions` from the spool,
`observed_dispatches` and `censored`. A test replays dev-v8b's archived rows: the problems fall on
exactly the hook-arm sessions from 09 on.

**Runtimes are protected.** `prepare` makes the shared runtime library read-only before any grant
and records its digest in `runtime-state.json`. Before every dispatch, `live`:
- stops the study (`ABORTED.jsonl`) if that library changed or became writable;
- runs each hook-arm session's own binary at SessionStart on a throwaway copy of its home, and stops
  the study if the execution contract does not appear;
- stops the study if a collected session shows a lapse.

**Product fix.** The product-side fix (bytecode and the digest, and a lapse notice a person can
see) is on `codex/ship-fixes`, not here.

## Files

- `dev-v8b/report.json`: the `dev-arms-1` report.
- `dev-v8b/collections/`: the 34 collected rows; the audit lists each row's sha256.
- `dev-v8b/posthoc-audit.json` and `dev-v8b/posthoc_audit.py`: the post hoc audit. Run it with
  `python3 posthoc_audit.py <dev-v8b root>`.
- `native-confinement-reviewed.json`: the passing `launch-gate-v5` native review (names only,
  $0.0281286).
- Raw sessions (transcripts, debug logs, hook records, spools, Adage homes) stay under
  `/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b`.

## Freeze records

The study ran 2026-10-01 from 05:46Z to 06:30Z.

- **Cohort:** `cohorts/launch-gate-v5`. Its native-confinement session, prepared by the revision-7
  runner (`native-confinement-*.json`), passed review before preparation.
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
- **Grants:** the person ran `approve-dev-arms.sh` (sha256 `2290b18c…`). It covers 28 homes:
  A1-A3, each granted with its own arm's binary; A0 holds none.
- **Isolation:** `isolation-audit.json` passed.
- **Offline dry run** (scratch directory, no grants, no ledger): the card reached A1-A3. Only A3's
  card named the test command (`.venv/bin/python -m pytest tests/test_validate.py -q`, Python
  `.venv/bin/python`). Without a grant there was no contract and the Stop gate stayed silent.
- **Superseded:** `dev-v8-superseded/` was frozen minutes earlier. That dry run found that one
  arm's card-checks setting could carry over within a recorder process, so the root was superseded
  before preparation, with no spend.
