# canary-v5 (development): delivery and safety pass; no elimination evidence

Development-split material: twelve automatic sessions on `mm-curve--finite`, using the explicitly
guided learned route `run-175d85`. All twelve were collected; the known cost was $2.6962327 and no
cost is unknown. Report: `report.json`. Timelines: `timelines/`, bound to their native hook logs
by `timeline-sources.json`. Every attempt is kept.

## What it shows

The report's `passed: true` is the **integration and safety criterion only**. It is not support for
the elimination hypothesis.

- **Delivery (evidence item 1).** The SessionStart contract reached all 12 native transcripts.
  All 7 successful CPU receipts arrived as structured receipts.
- **Safety (evidence item 2).** Every realized control kept necessary work: failed (2/2),
  later-edit (1), config-change (1), broader (2/2) and rerun (2/2). No necessary verification was
  skipped. No receipt was foreign or unapproved. Hidden-test quality passed 12/12.
- **Elimination: none.** Both positive sessions went unrealized: their receipts failed, and they
  have no successful receipt. `later-edit-2` and `config-change-2` also did not realize their
  conditions. Where a successful receipt was current, agents still repeated:
  - `later-edit-2`: one exact repeat and four narrower node runs (`covered_subset`);
  - `broader-1`: one equivalent repeat before the required wider run.

  There were 0 confirmed eliminations.

## Mechanism (a): failed receipts in the positives

The learned context fired at the natural end of the work, not mid-implementation. In both
positives and in `config-change-2`, the agent edited `src/marshmallow/validate.py`, then
`tests/test_validate.py`, then `CHANGELOG.rst`, and the receipt followed the changelog edit. This
is also the order of the guided training trajectories and of the context (`tests/*.py`, then
`CHANGELOG.rst`).

The receipts failed because the agents' own first-draft tests were wrong. One example: a
custom-message test expected `1.8e308 is not finite.` where the validator reported
`inf is not finite.`. Each agent then fixed its tests and verified, so the failed receipt led to
necessary work: a correct outcome. That happened in 3 of the 10 sessions that were not designed to
fail.

No prediction change is warranted. Firing later would not have made those tests pass, and the
timing evidence gives nothing principled to tune. The positive condition was simply not
guaranteed by the task. Separately, a failed receipt delivers nothing to the agent (an unexecuted
prediction gives no advice). A typed failure notice is a possible future change; it would not
eliminate work, because the agent needs the failure details.

## Mechanism (b): repeats after a successful receipt

In 10 of the 12 sessions, the first test request after the receipt used the bare system `python`
(`command not found` here). Adage's own interpreter advice (PreToolUse) then offered the
project-environment form of the same command, and the agent ran it next. In `later-edit-2` that was
a node run inside the receipt's file (`covered_subset`), followed later by the receipt's exact
command. In `broader-1` it was the receipt's command with `-v` (`equivalent_repeat`).

The receipt's scope covered what the agents checked (the whole file, which contains their new
tests), but nothing in it said that their new tests had run. The advice turned a failed bare
attempt into a runnable rerun. `test_receipt_replay.CanaryV5` pins both mechanisms on these
timelines.

## Changes (development, before canary-v7)

1. **Receipt note for covered test calls** (`adage-exec` `covering_receipt`, `adage-hook` `pretool`).
   A test call gets a note when this session holds a completed, applied learned verification receipt
   whose inputs are byte-identical now and whose command selects every test the call selects. The
   comparison uses the executor's snapshot, recomputed, with declared caches excluded. The note
   leads any interpreter advice. It is context only: the call still runs, nothing is refused or
   rewritten differently. The control arm and failed, refused, stale or other-session receipts
   never produce it.
2. **Pytest receipts now say plainly** that they ran every selected test, on the sources as edited
   so far, and that narrower runs inside their targets are covered.
3. **canary-v7** supplies one fixed test block (`canary-test-block-check.*`): a gold `Finite`
   passes it, an implementation accepting infinities fails it, and one with the wrong message fails
   it. A successful receipt in the positives is therefore likely by construction. canary-v7 runs
   six positives, two later-edits and one each of the other controls, and reports
   `integration_safety_passed` separately from the positive rows.
