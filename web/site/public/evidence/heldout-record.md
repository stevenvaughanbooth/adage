# Held-out decision record: Marshmallow stratum

Protocol `launch-gate-1`, revisions 9 to 11; analysis `heldout-analysis-1`. Stratum: same repository, same test file family, guided-learned route (run-175d85).
BEST: `A4` (verification ownership plus the orientation card), from the development report `/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v6/dev-v9b/reports/dev-v9-1790870521952346000.json`.
Build commit `8770f8cdd97728962e3ca25b3bf13a1352cc5dfe`; binaries {"adage": "63475c2ee97abf6c3982a37d6c77e4637c5fa52e92612b51a7ca3f3bec913903", "adage-exec": "c77b985973f218fd21eb5cc589f50a46fad0ec3840ac7027ef1d2b29dc12636c"}.
Assignments 65, collected 65.

## Outcome

FAIL: at least one registered criterion does not hold.


## Primary (registered)

BEST/A0 total model requests over the included blocks. Pass: the one-sided 95% bootstrap upper bound is at most 0.9.

| contrast | ratio | lower | upper | blocks | BEST fewer | sign-flip p | passed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| BEST / A0 | 0.6741 | 0.6258 | 0.7265 | 20 | 18 | 0.0001 | yes |

Included blocks: 20.

## Quality (registered)

BEST hidden-test passes 17; A0 19; margin 1; passed: no.

## G criteria

| criterion | applies | rule | passed | values |
| --- | --- | --- | --- | --- |
| G1 | yes | single repository: the frozen learned home holds the certified verification route run-175d85 (checked at freeze); the two-repository clause belongs to the cross-repository decision | yes | {"note": "checked at freeze (route present, isolation audit passed)"} |
| G2 | yes | BEST positive sessions: a learned verification ran in >= 50%, and >= 75% of those had a successful receipt | yes | {"fired": 20, "sessions": 20, "successful": 15} |
| G3 | no | confirmed eliminations were the r1-r7 primary; r8/r9 replaced it with model requests per session, which this stratum gates, and its two-repository clauses cannot apply to one repository; eliminations are reported | n/a |  |
| G4 | no | a repeat-elimination mechanism criterion superseded by the r9 primary; repeats were gated in development (D4); NOEXEC equivalent-command and BEST repeat fractions are reported | n/a |  |
| G5 | yes | BEST hidden-test failures <= min(A0 failures, NOEXEC failures) over all blocks; stricter than the r9 margin of one, so it governs quality | no | {"failures": {"A0": 1, "NOEXEC": 1, "BEST": 3}} |
| G6 | yes | zero skipped necessary verification (controls and positives), zero executions without a grant or in NOEXEC/A0, zero foreign or unapproved receipts, zero undeclared writes, zero unknown costs | yes | {"necessary_work_skipped": [], "executions_without_grant": [], "foreign_or_unapproved_receipts": [], "undeclared_writes": [], "unknown_costs": []} |
| G7 | yes | at most 25% of BEST CPU executions in positive sessions unnecessary (receipt stale before the session ended); controls are stale by construction and excluded | yes | {"executions": 15, "unnecessary": 0} |
| G8 | yes | BEST total requests <= NOEXEC total and BEST cost <= 105% of NOEXEC, over included blocks with the censoring rule | yes | {"requests": {"BEST": 391, "NOEXEC": 455}, "cost_usd": {"BEST": 2.701385, "NOEXEC": 2.699776}} |
| G9 | no | the pipeline-unseen repository (more-itertools) is not run in this stratum; G9 stays required for any claim that covers it | n/a |  |

## Sensitivity analyses (registered; reported, not gating)

| analysis | ratio | lower | upper | blocks | BEST fewer | sign-flip p | passes the bound |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| without censored blocks | 0.6741 | 0.6258 | 0.7265 | 20 | 18 | 0.0001 | yes |
| without has-keys | 0.6414 | 0.5902 | 0.6987 | 15 | 14 | 0.0003 | yes |

Blocks left out as censored: [].

## Censoring

A censored session counts as a quality failure. Only BEST is imputed upward, to 60 requests.

| arm | sessions | censored | observed requests | requests (included) | mean (included) | hidden-test passes | verification requests | cost USD |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| A0 | 20 | 0 | 580 | 580 | 29.0 | 19 | 177 | 4.019241 |
| NOEXEC | 20 | 0 | 455 | 455 | 22.75 | 19 | 138 | 2.699776 |
| BEST | 20 | 0 | 391 | 391 | 19.55 | 17 | 95 | 2.701385 |

## Request cross-check

Blocks excluded for an unverified request count: [] (at most 2 allowed).

## Controls (run first; all must be realized and skip no necessary work)

| condition | collected | realized | skipped necessary work |
| --- | --- | --- | --- |
| failed | yes | yes | no |
| later-edit | yes | yes | no |
| config-change | yes | yes | no |
| broader | yes | yes | no |
| rerun | yes | yes | no |

## Report only

| contrast | ratio | lower | upper | blocks | first fewer | sign-flip p | under the bound |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| NOEXEC / A0 | 0.7845 | 0.7374 | 0.833 | 20 | 18 | 0.0023 | yes |
| BEST / NOEXEC | 0.8593 | 0.8 | 0.9197 | 20 | 14 | 0.1858 | no |

## What this does not establish

- **No GPU savings.** Model requests are not GPU measurements, and CPU work is reported separately.
- **No other repositories.** This is a single stratum: same repository, same test-file family, and a guided learned route. G9 (the pipeline-unseen repository) is not run here.
- **No launch readiness.** That needs the cross-repository decision.

- note: GO for this stratum: the primary test and quality, plus every applicable G criterion.
- note: Model requests are never presented as GPU savings; CPU work is reported separately.
- note: Censoring: every censored session is a quality failure; only BEST is imputed upward (to 60 requests).
