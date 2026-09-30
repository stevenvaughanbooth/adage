# Open-source launch evidence

Status: **real-repository CPU action-elimination gate failed in the completed pilot**. The
[eight-session result](harness/results/20260930-live-cpu-transfer/FINDINGS.md) establishes 4/4
successful learned CPU executions and 4/4 correct automatic outputs versus 3/4 baseline outputs,
but 0/4 confirmed action eliminations. Every automatic session repeated the test command after
receiving its result. The frozen scorer also excludes approved cache changes; correcting that
alone does not resolve the duplicate-work finding. No CPU-driven savings claim is established.
The [frozen transfer preparation](harness/results/20260930-transfer-preparation/FINDINGS.md)
retains a zero-spend approval failure and its narrow pytest-cache fix. The replacement eight-session
comparison was superseded by the completed `transfer-v3` study linked above. Earlier preparation
records remain historical; the human issued all four grants and every assigned session completed.
The [launch-experience follow-up](harness/results/20260930-launch-experience/FINDINGS.md) adds the
first-reflex guide and TUI next steps, covers pytest's hidden cache metadata explicitly, and records
112 passing Rust/onboarding checks plus a fresh unchanged-history replay. The current study is
`transfer-v3`; the [continuation handoff](../../docs/launch-handoff.md) has its exact commands and scope.
This page is the launch evidence index. Preserve earlier reports, failed attempts and protocols
when a later experiment supersedes them.

The subsequent [edit-receipt diagnostic](harness/results/20260930-edit-receipt-ablation/summary.json)
is an authored fixture, not a launch benchmark. After fixing inherited-stdin contamination,
two runs per arm observed verifier requests in baseline 2/2, generic receipt 2/2, structured
receipt 1/2, and structured receipt plus startup responsibility 0/2; all eight fixes passed.
Pre-fix behavioral comparisons are excluded, with their raw records preserved. The candidate
production integration is implemented and locally tested, but needs a fresh real-repository
cohort and negative controls. The [handoff](../../docs/launch-handoff.md) is the current resume point.

The [cold-start pilot and fixed replication](harness/results/20260930-cold-start-replication/FINDINGS.md)
completed sixteen assignments on four distinct Marshmallow tasks, twice per arm. Shipped instincts
passed **8/8 outputs versus 6/8**, with **8.8% lower reported cost** ($1.7720458 versus $1.9429724)
and **10.5% fewer observed model requests** (272 versus 304). Three cost regressions and both
baseline failures remain in the totals. The reversed-order replication alone saved only 0.6%,
had equal request counts and passed all four tasks in both arms. The intervention is advisory
project-Python context; this is not learned automatic execution or a general savings estimate.

The [cross-session learning-curve pilot](harness/LEARNING-CURVE.md) completed 15 distinct native
training sessions and checkpoints at 0/3/5/10/15 sessions. **Every checkpoint produced zero executable
candidates.** Training cost $3.7549221; independent hidden checks and full-suite grading passed 12/15
tasks. All 19 offline base/gold controls pass after correcting one reference implementation.
The [complete native record](harness/results/20260930-learning-curve-native/report.json) preserves
every assignment, checkpoint and failure. A development replay with narrow command equivalence also
produced zero candidates. No held-out automatic arm ran, and there is no real-coding CPU savings
percentage to publish. The updated harness passes 72 tests; the equivalence change passes 188 focused
Rust tests. These test counts establish contracts, not product efficacy.

The separate [guided development check](harness/results/20260930-guided-demonstrations/FINDINGS.md)
then completed five explicitly instructed demonstrations: all eventually ran the prescribed test
command successfully, 4/5 passed independent grading, and zero executable candidates compiled.
Mining closure absorbs the test action into longer continuations; advisory contrast rejects many
of those before execution eligibility. Even after fixing that admission gap, the common last-edit
trigger has at most three qualifying sessions because intervening edits or verification stop the
recount. This is a concrete integration blocker, not evidence that more paid runs alone will solve it.
All 78 current harness tests pass. No grant or automatic held-out arm was created for this check.

The [execution-prefix integration](harness/results/20260930-execution-prefix-integration/FINDINGS.md)
now clears the mining gap: replaying the frozen guided histories compiles nine candidates instead
of zero. The best candidates still have only 4/5 qualifying continuations, so none receives an
execution certificate. The frozen organic replay also remains uncertified. Final validation passes
120 affected Rust tests and 81 harness tests. All original results remain unchanged.

The [cumulative guided follow-up](harness/results/20260930-cumulative-guided/FINDINGS.md) now
certifies four triggers for one pytest routine from all ten native demonstrations, with nine
supporting sessions and 9/10 qualifying continuations. Three triggers are learned sequence contexts.
New task quality is 5/5, cumulative quality 9/10. This clears real-history learning/certification,
but does not establish live automatic savings. The subsequent eight-assignment transfer comparison
is complete, with the failed action-elimination result at the top of this page. The person issued
all four exact execution grants. Current harness validation passes 84 tests.

| Historical gate | Evidence | Conclusion at that checkpoint |
| --- | --- | --- |
| Learn without authored routes or outcome labels | [Default onboarding](traces/results/onboarding.json) | Nine archived generated-repository training sessions produce learned routes using normal thresholds |
| Remove an agent action through CPU execution | [Paired lookup results](harness/results/20260929-learned-action-gate/FINDINGS.md) | Three held-out triage tasks: all six answers correct; three agent lookups removed; observed model dispatches 6 → 3 |
| Reliable complete result delivery | [Reliability](harness/results/20260929-learned-action-gate/reliability.json), [failed preflights](harness/results/20260929-learned-action-gate/preflight-failures.json) | 100/100 warm local repetitions after fixing interpreter launch; two earlier timeout failures retained |
| Preserve routine actions as execution candidates | [Learner and review validation](harness/results/20260929-routine-learning/validation.json) | Production-default synthetic transcript tests retain exact edit-to-command routines, including project Python; no unapproved advice or execution. Real training still pending |
| Connect cross-session sequence learning to CPU execution | [Native integration and regressions](harness/results/20260930-predictive-execution-integration/validation.json) | Five synthetic prior sessions learn a context that executes in a fresh session with an intervening read. Missing/stale grants remain silent; duplicates are withheld. 464 affected-crate tests and 229 CLI tests passed, with final focused checks recorded separately. Real-history development replay still has zero certificates |
| Run project Python checks on staged source | [Native Python integration](harness/results/20260929-project-python/validation.json) | Real uv/pytest on macOS arm64 passes through route planning, approval, later test/source edits, and private staged execution; post-merge release staging sample 2.75 s including an 873 ms child ([integrated record](harness/results/20260930-integrated-validation/validation.json)). Initial identity discovery is additional; no live savings claim |
| Full upstream suites through CPU staging | [Approved profile v3 result](harness/results/20260930-full-source-python-stage-profile3/validation.json), [initial failure](harness/results/20260930-full-source-python-stage/validation.json), [proposal canary](harness/results/20260930-proposed-temp-directory-policy/validation.json) | Marshmallow 1,190 passed; Click 2,240 passed and 26 skipped. Both checkouts unchanged. The approved macOS fix allows temporary-directory names, with child reads/writes still denied and old grants lapsed. Authored fixture routes: compatibility proven, live action elimination still pending |
| Useful automatic execution on real code | [Prospective protocol](harness/REAL-REPO-GATE.md), [preparation and first attempt](harness/results/20260929-real-repository-preparation/report.json) | Four held-out base/gold scorers validated; first live substrate attempt stalled during CLI startup; the [revised native probe passed](harness/results/20260930-native-probe-v4/validation.json). No real-repository coding savings claim |
| Faithful real-code pair measurement | [Paired protocol](harness/REAL-REPO-PAIRS.md), [offline validation](harness/results/20260929-real-repository-pairs-preparation/validation.json) | 33 harness tests pass; all four gold controls execute every expected hidden case. Stale receipts, mismatched assignments, alternate pytest reruns, and unknown cost fail the gate. Live pairs still pending |
| Plugin installation and continued learning | [Release startup and execution-plugin companion checks](harness/results/20260929-release-startup/validation.json) | Latest [integrated macOS arm64 check](harness/results/20260930-integrated-validation/validation.json): first start 828 ms, warm p95 92 ms over ten runs, 12 historical sessions learned; debug semantics and archive integrity also pass |
| Integrated regression and platform matrix | [Integrated validation](harness/results/20260930-integrated-validation/validation.json) | 894 native Rust tests and 53 map tests passed; all eight target lint/type checks passed using stub C compilers. GitHub Actions cannot start because of account billing/spending limits; native Linux/Windows remain unverified |

## Real-repository experiment eligibility

The next screen uses authored feature requests on pinned upstream Click and Marshmallow code.
These are full coding tasks with independent hidden tests, not historical upstream bug reports.
Five fresh unsteered variants per repository will supply training; held-out variants stay separate.
The original four-variant archive is insufficient for the default five-session support gate.
Do not mix newer feature
training traces with older historical fixes whose solutions may already appear in those traces.

The committed showcase archives contain four parsed sessions per family, but only two are
unsteered. The others received a card naming the test procedure. The production learner's
anti-feedback exclusion remains active: we do not count prior suggestions as independent evidence
of a naturally recurring action. The archived caches also lack their original raw transcript paths.
Fresh unsteered training sessions are needed before claiming a
learned automatic verification reflex. Current zero-candidate results are readiness findings,
not failed paid held-out tasks and not an excuse to lower the learning threshold.

Freeze the eligible history, production source/binaries, learned artifact, prompts, task matrix,
dependency environment and scorer before paid held-out dispatch. Record every attempt, including
budget cutoffs, infrastructure failures, unknown accounting and negative results. Any protocol
revision gets a reason and a timestamp before subsequent dispatch. Keep broad harness debug logs,
credentials and local approval grants out of public exports.

## Training policy correction

The first fresh training cohort stopped after three completed sessions. All three attempted
`python -m pytest`; the evaluation command allowlist denied those calls before a test process ran.
Inspection commands such as `find`, `grep`, and `wc` did succeed. The
[complete policy-friction record](harness/results/20260930-real-repository-training/v4-policy-stop.json)
retains all ten assignments, three costs and transcripts, and seven undispatched assignments.
No hidden coding quality was scored, and CLI success is not treated as code correctness.

The entire cohort is excluded from subsequent learning and efficacy analysis regardless of outcome.
The next prospective cohort repeats the same fixed matrix with generic Bash permission inside the
mandatory native sandbox. Filesystem, network, credential, and protected-write restrictions remain;
there is no task-specific command hint or environment activation. The [new native canary passed](harness/results/20260930-native-probe-v5/review.json);
[ten fresh assignments were frozen](harness/results/20260930-real-repository-training-v5/protocol.json) before dispatch. All ten attempts completed; one hit its turn limit. The [learning checkpoint](harness/results/20260930-real-repository-training-v5/learning-checkpoint.json) produced 25 routes per repository and zero narrow execution certificates. No held-out execution pair ran. This is an explicitly revised feasibility experiment, not
an unchanged execution of the first protocol.

## Prediction is the intended primitive

The original five-session checkpoints used an immediate edit-command execution certificate.
The integrated learner now recounts related continuations after compiled multi-event contexts;
the runtime connects those selections to the existing approved E2 executor. A synthetic native
integration test learns from five prior sessions and executes the resulting context in a fresh
session with an intervening read. This proves the connection, not real-repository savings.

The [first predictive development replay](harness/results/20260930-predictive-learning-v1/checkpoint.json)
reuses the frozen ten-session training cohort and still produces zero executable certificates.
The [release replay](harness/results/20260930-predictive-learning-v2/checkpoint.json), after
the integration and accounting fixes at `45b13c4`, confirms the same result with frozen
source/binary hashes. This snapshot precedes the subsequent merge of main `0c28814`.
The [post-merge checks](harness/results/20260930-predictive-main-refresh/validation.json) cover
the combined graph compiler, learner, native executor, review and harness adapters.
Each repository has only four sessions containing successful pytest observations, below the
five-session support requirement; activation compounds and command variation also limit the
current executable evidence. No extra paid calls or held-out pairs were used for this replay.
This is neither a mature-history evaluation nor a successful action-elimination result. Preserve
these negative records and the original thresholds. See the [behavior reference](../../docs/how-it-works.md#accumulated-prediction-and-current-execution-limits).

## Claims for launch

The measured claim today is narrow: **a learned, approved CPU lookup replaced one agent lookup
and one observed model request in each of three generated triage tasks**. The 21.4% lower reported
cost belongs to that six-session pilot. It is not an estimate of general developer savings.
GPU energy was not measured. Advisory benchmarks elsewhere in the repository remain advisory
benchmarks even when they use real repositories.

For real coding, report hidden-test quality and paired agent-action counts first, then model
dispatches, tokens/cost, wall time, CPU action count/duration and duplicate agent reruns. An
automatic check that the agent repeats, or one that runs needlessly after intermediate edits,
does not establish action elimination. Small task counts establish a mechanism, not a reliable
population savings percentage.

## Paid evaluation ledger

The user authorized **$60 for this task**. Measured spend so far is **$4.3605710**, plus an explicitly approved conservative charge of **$0.50** for an unknown earlier bill:

| Experiment | Paid sessions | Reported USD | Record |
| --- | ---: | ---: | --- |
| Hook-boundary substrate | 1 | 0.0152216 | [Summary](harness/results/20260929-claude-2.1.282/summary.json) |
| Learned lookup pairs | 6 | 0.0413526 | [All attempts and accounting](harness/results/20260929-learned-action-gate/report.json) |
| First native sandbox probe | 1 attempted | Actual bill unknown; $0.50 charged by human approval | [Original attempt](harness/results/20260929-real-repository-preparation/paid-state.json), [resolution](harness/results/20260930-native-probe-v4/prior-cost-resolution.json) |
| Revised native sandbox probe | 1 | 0.0133411 | [Passing native confinement](harness/results/20260930-native-probe-v4/validation.json) |
| Generic-Bash native confinement probe | 1 | 0.0124461 | [Passing native confinement](harness/results/20260930-native-probe-v5/review.json) |
| Training cohort v4, stopped for test-command permission mismatch | 3 of 10 assigned | 0.4952096 | [Every assignment and observed denial](harness/results/20260930-real-repository-training/v4-policy-stop.json) |
| Training cohort v5, two repository learning checkpoints | 10 | 3.7830000 | [Every attempt and learning result](harness/results/20260930-real-repository-training-v5/learning-checkpoint.json) |

One startup-censored attempt has no reported terminal cost. The human explicitly approved charging
its entire $0.50 reservation; its actual bill remains unknown. The ledger records this separately
from measured provider costs. The earlier planning snapshot reported **$4.8605710**, leaving
**$55.1394290** under the $60 task cap. The authoritative ledger's corresponding prior admission
total is $4.8039968, a $0.0565742 reconciliation difference; retain the earlier snapshot as history.
The [48-attempt accounting snapshot](harness/results/20260930-guided-demonstrations/budget-snapshot.json)
includes all 48 attempts through the natural, cold-start, diagnostic and guided runs: **$10.7000678
measured provider cost + $0.50 approved conservative charge = $11.2000678 admission spend**,
leaving **$48.7999322**. All attempts are settled or explicitly cap-charged; none are outstanding.
The [62-attempt snapshot](harness/results/20260930-cumulative-guided/budget-snapshot.json), after
the cold replication and cumulative guided study, records **$13.2920411 measured + $0.50 approved
conservative charge = $13.7920411 admission spend**, leaving **$46.2079589**. No paid transfer
sessions had started at this snapshot. This approval does not cover future unknown bills.
The [70-attempt snapshot](harness/results/20260930-live-cpu-transfer/budget-snapshot.json) includes
all eight completed transfer assignments: **$15.1229241 measured + $0.50 conservative charge =
$15.6229241 admission spend**, leaving **$44.3770759**. The transfer itself cost **$1.8308830**;
all assignments are settled, and no paid transfer process remains active.
Historical experiments elsewhere in the repository are separate from this task's ledger. Each new
experiment must reserve full session caps durably before dispatch; a new unknown cost blocks further
paid work. Human approval of a reflex is separate from evaluation spending authorization and applies
only to the reviewed command, repository, output contract and scope.
