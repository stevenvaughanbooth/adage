# Learned CPU transfer: execution works; action-elimination gate fails

**All eight assigned sessions completed. The learned reflex executed successfully in all four
automatic arms, and all four automatic outputs passed independent grading. The frozen launch
gate failed: zero of four pairs established agent-action elimination.**

This connects ten earlier, explicitly guided native sessions to approved CPU execution on four
different coding tasks. Each execution used the production context matcher, the human's exact
grant, private staging and network-denied sandbox. No inference call selected or ran the pytest
action. This is evidence for that mechanism, not a measurement of GPU utilization or energy.

## Complete comparison

| Measure | Learned artifact, execution off | Same artifact, approved execution |
|---|---:|---:|
| Assigned/completed sessions | 4/4 | 4/4 |
| Outputs passing independent checks | 3/4 | 4/4 |
| Reported API cost | $0.9829461 | $0.8479369 |
| Observed model requests | 152 | 130 |
| Agent tool actions | 151 | 126 |
| Successful CPU executions | 0 | 4 |
| Confirmed action eliminations | — | 0/4 pairs |

The automatic arm's aggregate cost was 13.7% lower and request count 14.5% lower. These are
descriptive four-pair observations, **not established CPU-driven savings**. Two pairs cost more
with execution. The baseline's Odd session spent substantial effort retrying sandbox-denied Git
operations; unrelated agent behavior contributes to the difference. No outcomes were retried or
excluded. The failed baseline MaxUtf8Bytes output omitted the expected default error in `repr`:
1,204 tests passed and the independent `test_hidden_repr` failed. It remains in every total.

## What the real execution established

The same learned sequence—edit `tests/*.py`, then `CHANGELOG.rst` within eight events—ran
`.venv/bin/python -m pytest tests/test_validate.py -q` in each automatic session. Recorded action
durations were 1,221, 1,175, 1,168 and 1,213 ms (mean 1,194.25 ms). These are action durations,
not total task time or an independently isolated predictor latency.

All four receipts identify the approved route/session and an exit-0 applied action. A separate
post-hoc audit confirms that the source-state digest before and after each execution matches
that task's final source state; the only recorded file changes were four declared pytest cache
files. All four native transcripts contain both `hook_success` and `hook_additional_context`
attachments carrying the completion message. Delivery was real, not inferred from wrapper logs.

## Why the gate failed

1. **The agent still repeated the action.** After the successful CPU receipt, automatic sessions
   requested the exact command again: twice for Odd, once for each other task. The completion
   message explicitly said the action was complete and should not be repeated. These are actual
   duplicate requests, independent of the scoring limitation below.
2. **The frozen receipt filter was too narrow for approved caches.** It required an empty
   `outcome.changed`, so it rejected receipts containing even the explicitly authorized pytest
   cache files. Preserve the original score. The separate `runtime-diagnostic.json` explains this
   limitation; it is not a replacement success criterion.
3. **Baseline evidence and shell behavior did not meet the exact comparison contract.** Some
   baseline runs used different pytest flags/selectors, and additional verification commands
   remained. The fixed matcher admits only its predeclared command forms. The full per-pair
   reasons are retained, along with the baseline quality failure.

Fixing the cache filter alone would not establish action elimination: all automatic sessions
still requested the corresponding command after receiving its result.

## What to change before another paid study

First build a deterministic replay from these native events that verifies the complete
execution-to-agent-result path and distinguishes earlier tests, duplicates on unchanged source,
and genuinely necessary verification after a new edit. Examine how the harness presents a
completed action and its test scope/result; stronger prose alone is an unproven fix.

Then make completed verification reusable with explicit scope, source freshness and invalidation.
If an adapter can avoid only a repeated subprocess after the model already requested it, report
that as command reuse—not an avoided model request. Keep the CPU completion available before
the agent decides its next action if the goal is inference savings.

For a new, prospectively registered study, distinguish approved cache effects from source writes
in receipt scoring and predeclare justified pytest flag equivalences. Use new task instances and
both positive and negative controls. Do not rewrite this run's result or keep repeating the same
benchmark until it passes. Additional mining breadth and cross-model transfer remain separate work.

## Provenance, spend and limits

Training used five distinct tasks twice, with explicit workflow guidance. Transfer tasks were
excluded from that training but had already appeared in the cold-start pilots. One repository,
one model and four pairs do not establish general reliability or statistical significance.

The eight sessions cost **$1.8308830**. The shared ledger now contains 70 attempts: **$15.1229241
measured provider cost + $0.50 separately approved conservative charge = $15.6229241 admission
spend**, leaving **$44.3770759** under the $60 cap. Every attempt is settled or explicitly
cap-charged; no paid process remains active for this study.

`report.json` preserves all outcomes and reproduces the four frozen pair decisions. Only repeated
per-hook file maps were omitted; source digests remain and the complete original report is
hash-addressed in its serialization metadata. `archive-index.json` binds all native transcripts,
hook logs, runtime spools and original collections in the local archive. `delivery-audit.json`
and `runtime-diagnostic.json` are explicitly post-hoc diagnostics. Assignment manifests, native
provenance, final source manifests, patches, the budget snapshot and export script are adjacent.
Human approval stores are not published.
