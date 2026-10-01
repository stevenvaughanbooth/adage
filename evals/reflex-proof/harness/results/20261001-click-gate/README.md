# Click launch-gate study: ready to run (launch-gate-1 revision 13)

Prepared entirely offline on `codex/click-gate`. No paid model ran, no Claude session was launched,
no grant was written and nothing was written under the authority. The protocol is "Revision 13: the
Click stratum" in `docs/launch-gate-protocol.md`, and `click_heldout_analysis` in the protocol JSON.

| File | What it is |
| --- | --- |
| `run-click-gate.sh` | the one script the person runs (checks every input's hash first) |
| `INPUTS.json` | binaries, task controls, BEST's selection report, the ledger read at registration, the budget |
| `organic-replay-preview.json` | the organic arm's replay, run once offline after its expectation was written |
| `dry-run.json` | the offline dry run of the whole chain on a scratch authority (`click_dryrun.py`) |

The frozen tasks are in `../20261001-click-tasks`.

## The chain

`run-click-gate.sh` does the following, stopping on any error (a rerun resumes, and nothing already
attempted is dispatched again):

| Step | What | Spend |
| --- | --- | --- |
| 0 | budget check against the live ledger; you type `run` | none |
| 1 | cohort `click-gate-v1` (the authority manifest, the Click training plan, frozen prompts) and its native recipe | none |
| 2-3 | native-confinement session, then its review (must pass) | <= $0.50 |
| 4-5 | 10 guided training sessions (hex-color, port, duration, ip-address, semver, twice) | <= $6.00 |
| 6 | learning: the fixed learner on the guided histories; the organic replay; the isolation audit of both; route selection | none |
| 7 | the mode: execution if a route was selected, else advisory only (you type `advisory`); if execution mode does not fit the allocation, you may type `advisory-only` | none |
| 8-9 | freeze; prepare (checkouts, homes, settings; the shared runtime library made read-only) | none |
| 10 | execution mode only: `approve-click.sh` is written and its sha256 is printed and re-checked; you read it and type its first 12 characters; it issues 25 grants (BEST homes only) | none |
| 11 | verify grants | none |
| 12 | live: controls first in execution mode; a failed control or lapsed grant drops BEST, and A0 and NOEXEC continue | <= $39.00 (65 sessions) or <= $24.00 (40) |
| 13 | report: `reports/click-heldout-<time>.json` and `.md`, both co-primaries' outcomes | none |

## Budget

At registration the ledger has **$20.1084449** remaining of $60 (284 attempts), so **$17.1084449**
is available above the $3 floor.

| Chain | Worst case | Expected | Shortfall now | Ceiling needed |
| --- | ---: | ---: | ---: | ---: |
| Execution (native, training, 65 held-out) | $45.50 | $18.63 | $28.39 | $88.40 |
| Advisory only (native, training, 40 held-out) | $30.50 | $13.53 | $13.39 | $73.40 |

The caps that would fit $17.11 are $0.16 and $0.27 per session, below a Click session's expected cost
(planning basis: v5 Click sessions averaged $0.452 before the realism line and the project
environment; Marshmallow's no-Adage arm cost 0.66 of its v5 cost at held-out). **The script refuses
to start until the person raises the allocation.** Raising it is the person's decision: see the
handoff ("Click study: ready to run").

## Offline evidence

- **Tasks validated** (`../20261001-click-tasks/README.md`):
  - every base fails only hidden tests;
  - every gold passes Click's whole suite;
  - all 12 broken implementations fail hidden behaviour tests only;
  - the control blocks tell gold from broken implementations.
- **Organic replay** (`organic-replay-preview.json`): the five organic v5 histories gave 25 routes and
  0 certified, as expected. The isolation audit passed: 253 sessions scanned, 247 forbidden.
- **Dry run** (`dry-run.json`, `passed: true`). On a scratch copy of the authority it ran:
  - the cohort and its native recipe;
  - 10 training recipes (Haiku, $0.60 cap, 60 turns, the realism line and the instruction in every
    prompt);
  - learning on stand-in histories (copies of v5 transcripts with fresh ids): audit passed, mode
    advisory;
  - **advisory mode:** freeze, prepare (40 assignments), verify-grants (no grant anywhere), and a
    report that is `incomplete`, as expected with nothing run;
  - **execution mode on a simulated route:** freeze (five composed control prompts checked), prepare
    (65 assignments), the approval script (25 homes), and verify-grants failing only on the 25 missing
    person-issued grants;
  - `live` refused both dry-run roots.
- **Tests:** 268 harness tests pass, 50 of them new (`test_click_tasks.py`, `test_click_training.py`,
  `test_click_analysis.py`, `test_click_gate.py`).

## What this is not

Nothing here is a result. The organic replay is development of the learned home, and the dry run's
learning used stand-in histories. Model requests are never GPU measurements. Advisory and execution
mode are separate claims.
