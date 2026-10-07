# Commercial savings acceptance plan

The objective is measured whole-workflow savings on a specified customer workload, with
independently preserved task quality. Engineering verification, candidate formation and hook
delivery are prerequisites; none is an efficacy result. The synthetic paid pilot remains
exploratory. Keep study results and raw/customer evidence in private study roots outside Git.

## Fixed acceptance criteria

Use [`evals/live/policies/claim.json`](../evals/live/policies/claim.json) explicitly for final
confirmation. The generic evaluator default and the two-group pilot policy are not the claim
policy. Do not modify the policy after seeing results.

| Requirement | Prespecified decision |
| --- | --- |
| Primary savings metric | `1 - sum(treatment input + output tokens) / sum(baseline input + output tokens)` over every scheduled workflow |
| Token uncertainty | Existing deterministic paired bootstrap: 2,000 resamples of whole independent task pairs; approximate one-sided 95% lower percentile bound must be at least 20% |
| Quality | Independently graded complete-task pass/fail; treatment-minus-baseline success lower confidence bound must be at least -2 percentage points |
| Quality uncertainty | Existing conservative simultaneous 95% bound: lower improvement probability minus upper regression probability, using exact binomial bounds |
| Baseline floor | Observed baseline success at least 80% |
| Independence floor | At least 200 distinct declared task groups, disjoint from training and all development evidence |
| Evidence integrity | Complete scheduled cohort, original failed/interrupted attempts, unique requests, immutable settings/artifacts and complete native usage |
| Economic reporting | Model calls, input/output/cache components, CLI estimates, actual billed costs, cost per independent success, setup cost and break-even workload reported separately |

The implementation is in [`paired.rs`](../crates/adage-eval/src/paired.rs) and
[`experiment.rs`](../crates/adage-eval/src/experiment.rs). Cached input is included once in input,
not added to it. Sum every planning, retry and auxiliary model request, including intervention
messages and unsuccessful attempts. Model-call savings are a secondary metric; fewer calls
alone cannot pass the token gate. Twenty percent is a lower-bound requirement, not a requirement
that the point estimate fall between 20% and 30%.

The token and billed-cost decisions are separate. Cost estimates must retain their price basis;
CLI `total_cost_usd` is not an invoice. A billed-cost claim requires complete actual billing
attribution and the existing cost acceptance gate. Token qualification must not be presented as
proof of dollar savings. Without billing evidence, report billed cost and billed ROI as unknown;
any economic projection must say it is an estimate.

## Workload and fair comparison

Select one named real repository and its intended user workflow before choosing favorable
examples. The first frozen protocol supports one repository and two arms. Define the eligible
issue population, sampling window and workflow mix in the registry. Include discovery,
companion updates, verification and recoverable wrong-command opportunities where they occur
naturally, plus unrelated controls. Do not invent equal quotas without evidence that they
represent the intended workload. A claim applies only to that registered population.

Use distinct real issues as the initial grouping unit, then review related issues for shared
solution templates. Variants, retries, paraphrases and repeated stochastic runs stay in the same
group. Reserve training, development and final groups before learning. Existing inspected or
tuned evaluation tasks are development evidence and cannot be recycled into confirmation.

For each task, freeze its starting revision, requirements and independent private grader.
Demonstrate that the broken state fails for the intended reason, a reference solution passes,
partial/wrong solutions fail, and changes to visible checks cannot spoof the grade. Apply the
same grader and normal repository instructions to both arms. Neither an observed test command
nor a zero exit code establishes sufficient coverage.

Pin the same model revision, harness, tools, permissions, starting state and execution limits
in both arms. Use fresh checkouts and the evaluator's seeded order with balanced first-arm
assignment. Freeze the exact learned artifact; disable automatic learning during collection.
Record applicable/delivered reflexes, uptake or authorized execution, unnecessary hints,
repeated work and remaining verification omissions for every task, including negative controls.

E0 advice and approved E1/E2 execution are separate treatments. The current frozen live
collector supports E0. Execution requires its own declared adapter, lineage/usage capture and
operator approval; do not retrofit execution into an E0 study. Keep production thresholds and
human approval boundaries intact. A learned candidate that stays quiet is a valid result.

## Development and sample-size decision

First audit retained native evidence and prior experiments. Preserve negative results and do
not repeat completed attempts. A promising historical aggregate with missing native ledgers
can motivate a hypothesis but cannot be reconstructed into a qualified current study.

Collect a bounded, prospectively registered development comparison only after a useful
production-learned treatment and a validated task/grader registry exist. Keep ordinary baseline
instructions. Change one evidence-supported cause at a time: selection, timing, precision or
elimination of repeated work. Record the hypothesis, code/artifact hashes, full cost, quality,
measured savings and next decision in the private experiment log. Any results used to choose a
change are development data. Do not lower thresholds, author replacement reflexes or replace
unfavorable runs.

The minimum 200 groups is not a power calculation. Under the implemented quality bound, 200
pairs with no quality discordance yield a lower bound of about -1.83 points. A single regression
with no improvement fails at that size. Untreated training runs cannot estimate paired token
variance or quality discordance. Repeated variants cannot manufacture the missing sample size.

Once paired development evidence exists, choose a fixed final N >= 200 by simulating the joint
quality, baseline-floor and savings gates using the exact evaluator. Predeclare anticipated
effect above the 20% lower-bound threshold, desired probability of passing, tail/discordance
sensitivity analyses and the task sampling method. Small development samples leave these
assumptions uncertain; report the sensitivity rather than promising power. Register the final
N before outcomes are collected, and do not extend the cohort until a target passes.

Freeze code, learned artifacts, model/settings, registry, graders and analysis policy. Record
the digest in a separate timestamped record before execution. Keep all scheduled tasks in the
denominator. Stop for incomplete evidence, isolation or authorization failures; retain those
attempts and diagnose them. Do not silently replace them or tune the frozen treatment.

## Admission budget and release gates

The existing mechanism pilot has a $30 admission allocation: 20 untreated training sessions at
$1, four validation arm sessions at $1 and six test arm sessions at $1. Preserve that schedule
and its ledger. A low realized CLI estimate is not a new allocation or authorization to repeat
training. That cap does not authorize a larger commercial study.

For a new exact registry, price the following before dispatch:

| Stage | Scheduled jobs | Maximum reservation | Release condition |
| --- | ---: | ---: | --- |
| Free diagnosis, task/grader checks | 0 | $0 | Inputs and independent grouping reviewed |
| New untreated training | T | T × session cap | Named registry and authorized stage allocation |
| Paired development | 2D | 2D × session cap | Useful reviewed treatment; frozen development protocol |
| Untouched confirmation | 2N, N >= 200 | 2N × session cap | Prospective sample-size decision and exact authorization |

At the old $1 cap, the 200-pair floor alone reserves $400. That is an arithmetic lower planning
bound, not a powered proposal or proof that realistic tasks fit in $1. Determine the cap and
N from the actual workload and development evidence. Before requesting expansion, prepare one
reviewable package with exact task IDs/grouping, registry digest, run count, model, per-session
cap, maximum reservation, release gates and the decision it resolves. Missing workload or
paired data must stay explicit; do not disguise a structural example as an executable proposal.

Keep costs from separate historical programs separate until their ledgers are reconciled and
deduplicated. Admission reservations, conservative unknown-cost charges, CLI estimates and
provider invoices are different quantities. Cost per successful task includes all arm spending
in the numerator. Estimate break-even only when mean task-cost savings are positive; divide
setup cost by those savings and disclose excluded labor or infrastructure costs.

## Launch decision and reproducibility

Return commercial GO only when the untouched registered confirmation study meets the token and
quality criteria and supports an economically credible model-cost reduction for the stated
workload and cost basis. Token qualification alone does not satisfy that commercial objective.
Actual billed-savings claims additionally require the billed-cost gate; estimates must remain
explicitly estimates. Return ITERATE / NO-GO when trustworthy evidence misses the target, with the next
investment decision. Return BLOCKED when required workload, access, authorization or evidence
is missing, with the concrete prepared next action. Successful infrastructure checks are not
a commercial stopping point. State explicitly when no savings marketing claim is supported.

Retain exact commits, binary/model/harness hashes, registry and grader revisions, training and
learning provenance, review decisions, frozen protocols, every native history and usage ledger,
independent grades and analysis commands outside Git. Report training/setup separately from
evaluation. See the [local evaluation runbook](local-agent-evaluation.md) for the existing
prepare/run/report commands and operator-terminal boundary.

Hosted CI availability is separate from empirical efficacy. When unavailable, record exact
local source trees, commands, failures, corrections and reruns; state the unverified platform
matrix. A documented local engineering assessment does not create a savings result or remove
operator-only execution/approval boundaries. Merge still requires the user's authorization.

The concrete four-workflow preparation and publication gates are in [launch experiments](launch-experiments.md).
