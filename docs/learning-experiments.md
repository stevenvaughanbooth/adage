# Frozen learning experiments

`adage eval experiment` fixes the task cohort before measurement and binds results to the
training audit, model settings and compiled reflex set. It runs offline. An external harness
executes the scheduled jobs and supplies independently graded, complete workflow receipts.

Use this workflow for prospective savings studies. `adage eval paired` remains useful for
exploring submitted pairs, but cannot establish that the full task list was supplied. Neither
command proves a 20–30% saving without representative, independently graded live runs.

For Claude Code on Linux, [`evals/live/frozen.py`](../evals/live/README.md#frozen-studies)
prepares and executes this contract through the existing isolated live runner. It collects native
request accounting and independent grades automatically. The commands below also document the
provider-neutral contract for other collectors.

## Prepare and freeze

Split training, validation and final test task families before learning. Keep retries and related
variants in the same declared group, using the same repository/task registry throughout. Import
training, learn and review candidates, then freeze the compiled `index.json`. Disable automatic
learning and changes to the reflex set during the experiment. Retain the learner build,
parameters, training generation and learning report as provenance.

Export the verified audit with `adage corpus inspect --repo acme/api --evidence`. Every training
trace must have a declared identity; inferred/unknown identities prevent freezing because they
cannot establish separation from the test registry. Pin models, settings, tools, checkout,
context, grader, per-task budgets and the entire task list before collecting results. With the
default two-point quality margin, even zero discordant outcomes need about 183 tasks; the
30-task default minimum is only a floor.

Store the exact JSON files as immutable objects:

```sh
adage eval experiment object training-evidence.json --objects study/objects
adage eval experiment object baseline-settings.json --objects study/objects
adage eval experiment object intervention-settings.json --objects study/objects
adage eval experiment object frozen-index.json --objects study/objects
```

Each returns `{"sha256":"…"}` and publishes those bytes to `study/objects/<sha256>.json`.
Files are private on Unix. Identical delivery is idempotent; corruption at an existing address
fails without replacing it. Objects and protocol/submission inputs are bounded to 64 MiB each.
Raw evidence belongs outside git.

Create a protocol with this structure. Replace the illustrative strings and populate the full
cohort; the single task below is a format example and cannot satisfy its 200-task policy.

```json
{
  "schema": 1,
  "experiment": "api-reflex-savings-v1",
  "repo": "acme/api",
  "seed": 42,
  "kind": "same_model",
  "training_audit_sha256": "AUDIT_DIGEST",
  "learner_revision": "pinned-build-and-parameters",
  "policy": {"min_groups": 200, "max_quality_loss": 0.02, "min_savings": 0.20, "min_baseline_success": 0.80},
  "budget": {"max_model_calls": 30, "max_cost_microusd": 1000000, "timeout_seconds": 600},
  "baseline": {"model": "pinned-model", "configuration_sha256": "BASELINE_SETTINGS_DIGEST", "reflex_set_sha256": null},
  "intervention": {"model": "pinned-model", "configuration_sha256": "INTERVENTION_SETTINGS_DIGEST", "reflex_set_sha256": "INDEX_DIGEST"},
  "tasks": [
    {
      "identity": {"task_id": "TASK-1", "group_id": "INDEPENDENT-FAMILY-1"},
      "task_revision": "pinned-input-and-checkout",
      "environment_revision": "pinned-harness-tools-context",
      "evaluator_revision": "pinned-independent-grader"
    }
  ]
}
```

```sh
adage eval experiment freeze protocol.json --objects study/objects > study/frozen.json
adage eval experiment validate study/frozen.json --objects study/objects
```

Freezing verifies object checksums, training/test separation, unique declared groups, model
cohorts and positive budgets. The cohort must meet `min_groups` and contain at most 10,000 tasks.
The baseline disables reflex delivery; the intervention pins a reflex set. `same_model` requires
identical model revisions; `model_transfer` requires different ones. Common environment/grader
revisions apply to both arms. Differences in arm settings are explicit treatment choices that
must be considered when attributing a benefit to reflexes.

The output contains the normalized protocol, its digest, stable job ids and maximum scheduled
cost in millionths of a dollar. Task order is seeded; first-arm order is balanced across paired
blocks. Execute each job on a fresh equivalent checkout. Publish the digest to an independent,
timestamped record before execution. Changing tasks, thresholds or settings creates a new
experiment. A local hash alone does not prove prospective registration.

## Collect every scheduled result

The harness writes one terminal receipt per job, retaining failures, timeouts and cancellations.
Resume collection by immutable job id; do not replace unfavorable terminal results. Retries
inside the predeclared workflow contribute usage/cost to the same job, never another task pair.

```json
{
  "schema": 1,
  "protocol_sha256": "FROZEN_PROTOCOL_DIGEST",
  "job_id": "JOB_ID_FROM_SCHEDULE",
  "run_id": "unique-terminal-workflow-id",
  "observed": {
    "model": "pinned-model",
    "configuration_sha256": "ACTUAL_SETTINGS_DIGEST",
    "reflex_set_sha256": "ACTUAL_INDEX_DIGEST",
    "task_revision": "pinned-input-and-checkout",
    "environment_revision": "pinned-harness-tools-context",
    "evaluator_revision": "pinned-independent-grader",
    "budget": {"max_model_calls": 30, "max_cost_microusd": 1000000, "timeout_seconds": 600}
  },
  "status": "completed",
  "passed": true,
  "usage_complete": true,
  "calls": [
    {
      "id": "provider/account/request-id",
      "model": "pinned-model",
      "usage": {"input_tokens": 800, "output_tokens": 200, "cached_input_tokens": 400, "model_calls": 1, "cost_microusd": 2000}
    }
  ]
}
```

Use JSON `null` for the baseline's reflex-set digest. Status is `completed`, `failed`, `timeout`
or `cancelled`. Completed runs need an independent boolean grade to qualify; unknown grades
remain `null`. Other terminal statuses count as failures, and a success grade on one invalidates
the receipt. Grade the whole task; a passing intermediate check is insufficient.

Count every production workflow request, including planning, retries and intervention overhead.
Each call's `model_calls` is one, and input includes cached input once. Missing request usage is
`null`; incomplete collection sets `usage_complete` to false. Both block token qualification.
Missing billed cost blocks the cost claim without erasing complete token evidence. Empty calls
with complete accounting mean a verified zero-model-call workflow, not a missing log. Unexpected
fallback models invalidate this fixed-model comparison; routing policies need a separate design.

The adapter must derive `observed` from actual settings/artifact bytes, collect provider
accounting and independent grades, enforce timeout/budget, and reconcile the entire request
ledger. The evaluator cannot attest provider authenticity, omitted requests behind a false
completeness assertion, grader honesty, or that the artifact was trained on the claimed corpus.
The frozen live adapter retains immutable accounting, grader and execution objects in the
receipt's optional `evidence` map (names to object digests). Reporting verifies those objects too;
corrupt or missing sources prevent qualification. Native stream checksums retain the connection
to raw logs kept privately outside git. Claude assistant chunks can carry provisional output
counts: this adapter requires completed requests reconciled with terminal and per-model totals.
CLI list-price estimates are not billed costs, so cost stays unknown. Historical runs cannot
become preregistered studies.

## Qualify the complete cohort

Store each receipt with `experiment object`, then list every digest in a submission:

```json
{"schema":1,"protocol_sha256":"FROZEN_PROTOCOL_DIGEST","receipts":["RECEIPT_DIGEST"]}
```

```sh
adage eval experiment report study/frozen.json study/submission.json --objects study/objects
adage eval experiment report study/frozen.json study/submission.json --objects study/objects --require-token-target
adage eval experiment report study/frozen.json study/submission.json --objects study/objects --require-cost-target
```

Every scheduled task stays in the denominator. Missing jobs, duplicates, unscheduled jobs,
reused request/run ids and changed conditions invalidate qualification and suppress confidence
bounds. Missing/corrupt objects fail before a report is emitted. An unqualified required target
prints its report and exits unsuccessfully. `evidence_complete` means outcomes/token accounting
are complete and internally consistent; cost may still be missing and quality/savings may fail.
The nested `paired` report separates quality, token and cost decisions.

For cheaper-model studies, predeclare stronger and cheaper models with and without reflexes.
Compare cheaper-with to cheaper-without to isolate the contribution, and cheaper-with to
stronger-without for substitution. This schema freezes one comparison at a time; multiple
comparisons need an external multiplicity plan. Repeatability needs repeated within-task runs
and clustered analysis; this fixed-pair evaluator does not establish reduced outcome variance.

The synthetic fixtures in `adage-eval/tests/experiment.rs`, the CLI lifecycle test and
`evals/live/test_frozen.py` verify these contracts. The Python flow uses simulated agents and the
real Rust validator, plus actual local processes for deadline and grading failures. They are not
customer savings evidence.
