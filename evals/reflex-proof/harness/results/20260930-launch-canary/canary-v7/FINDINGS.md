# canary-v7 (development): the model still requested covered verification in every positive

Development-split material: 12 automatic sessions on `mm-curve--finite`. Known cost $1.3425845,
0 unknown. Files:
- `report.json`: the original report;
- `timelines/` and `timeline-sources.json`: the per-session timelines;
- `posthoc-rescore.json`: a **post hoc** re-score with `receipt_replay` version 2, written after
  the outcomes were seen.

## The original report is misleading on elimination

The original report (scorer version 1) shows:
- `positive_realized` 6/6, and a fresh successful receipt in every positive;
- `positive_eliminations` 4, `positive_redundant_repeats` 2, covered subsets 2;
- 16 receipt notes, quality 12/12, `necessary_work_skipped` empty;
- `integration_safety_passed` false, because both later-edit controls went unrealized: no agent
  edited after the receipt.

**The 4 "eliminations" are a scoring artifact and must not be presented as eliminations.** In
every one of those rows, the model's only post-receipt request was a covered test command run
with the bare `python`. A typical sequence:

1. The receipt is delivered after the CHANGELOG edit.
2. The model says "Let me verify the tests pass" and requests
   `python -m pytest tests/test_validate.py::test_finite_…`.
3. Adage's PreToolUse receipt note arrives with that call, and the call still runs.
4. The shell answers `command not found: python`.
5. The model then reads the note and stops.

The model did make the covered request, right after a fresh successful receipt. It failed only
because `python` does not resolve in the sandboxed shell. Where `python` resolves, it would have
run. The other two positives went on to run the `.venv` form anyway.

## Post hoc re-score (version 2: covered requests count whether or not they launched)

| Positive sessions | 6 |
| --- | ---: |
| With a fresh successful receipt | 6 |
| With at least one covered repeat attempt | 6 |
| Repeat attempts (of which never launched) | 10 (6) |
| Confirmed eliminations | **0** |

Safety is unchanged: `necessary_work_skipped` is empty.

**Development conclusion:** structured receipts plus the startup contract did not stop the model
from requesting covered verification in any positive. The PreToolUse note arrives with the call,
too late to save the model's turn; at most it stops a follow-up. The model verifies its own new
tests by name immediately after editing, and a file-level receipt did not register as covering them.

## Changes registered as revision 7 (before any held-out dispatch)

1. **Scorer version 2.** A request whose selection a fresh successful receipt covers is a repeat
   attempt, whatever its interpreter and whether or not it launched. Any repeat attempt blocks an
   elimination. Only completed requests satisfy necessary verification. Regressions:
   `test_receipt_replay.RepeatAttempts` and `CanaryV7`.
2. **Realistic environment.** Sessions launch with the checkout's own `.venv` first on PATH and as
   `VIRTUAL_ENV` (`probe.project_launch`), so bare `python`/`pytest` resolve and repeats execute
   and are measured. Confinement is unchanged. The native canary requires `python_on_path`. With an
   activated environment, Adage's interpreter advice stays out, and receipt notes still apply.
3. **Test evidence in the receipt.** A pytest verification receipt now lists the collected count
   and the node ids new since the last recorded run, from pytest's own `cache/nodeids` file. That
   file is a cache output the approval declares, and it is bound to the journal by its applied
   sha256. The list is bounded and restricted to the command's targets; no stdout is used. The
   receipt also says plainly: do not rerun these tests unless code or configuration changes again.
4. **The later-edit control is realized by construction.** After the CHANGELOG entry, the agent
   must make Finite reject booleans and add a test that fails without that change.
5. **Not implemented: PreToolUse result substitution.** It would save CPU only after the model has
   already spent its turn, and the goal is skipping inference.

canary-v8 is designed but deliberately **not frozen**, pending the next design direction.
