# Launch experiments for learned process guidance

**Current decision: BLOCKED. No launch savings claim is supported.** The four scenarios below
are hypotheses about useful learned guidance, not demonstrated agent difficulty or measured
savings. The next Coder experiment will test the actual Claude mod with production-learned
reflexes, the same pinned model in both arms, and independently graded quality. The explicit
`native-mod-bundled-defaults-e0/v1` collector profile supports this advice-only comparison on
the verified Linux harness; see the [runtime requirements](../evals/live/README.md). Its free
fixture checks do not admit a paid study or establish savings. Optional execution or prediction
treatments require separate protocols and approvals. The experiment may establish no benefit.

Use the [commercial acceptance plan](commercial-evidence.md), the existing
[claim policy](../evals/live/policies/claim.json), and the
[frozen experiment contract](learning-experiments.md). The synthetic paid pilot remains a
separate exploratory study. Its unused allocation does not authorize these experiments.

## Four candidate use cases

| Scenario | Process worth learning | Independent task outcome | Boundary of the claim |
| --- | --- | --- | --- |
| Coder database changes | Migrations, old-data fixtures, SQL queries and generated Go must agree | Existing databases upgrade correctly; constraints, queries, API behavior and required downgrade behavior remain correct; regeneration is consistent | The registered database-change population, on the pinned PostgreSQL/runtime versions |
| cert-manager API changes | Public/internal types, CRDs, deepcopy and conversion have separate generation paths | Field survives conversion without aliasing; CRD accepts/rejects the right values; certificate issuance, matching and renewal follow the requested behavior | The registered API-change population; artifact consistency alone is insufficient |
| TypeScript/TSX grammar changes | Shared grammar must reach both generated parsers; Node binding tests cover a different layer | Both dialects produce the required trees and reject invalid syntax; generated parsers reproduce from grammar; unrelated corpus cases retain behavior | The registered parser-maintenance population and explicitly graded syntax/tree contract |
| dbt incremental changes | A successful fresh build can leave old materialized rows wrong | Historical rows, late updates, new rows and repeat runs produce correct stored data without duplicates or unauthorized full refresh | A real registered project and warehouse population; a DuckDB fixture cannot establish general warehouse savings |

Workflow references: [Coder backend development](https://github.com/coder/coder/blob/main/contributing/backend.md),
[cert-manager CRD development](https://cert-manager.io/docs/contributing/crds/),
[TypeScript parser package](https://github.com/tree-sitter/tree-sitter-typescript/blob/master/package.json),
[dbt incremental models](https://docs.getdbt.com/docs/build/incremental-models), and
[dbt incremental CI](https://docs.getdbt.com/best-practices/clone-incremental-models).
Pin historical instructions for the task's revision. For example, the inspected cert-manager
revision uses `make update-crds` and `make update-codegen`; current documentation is not its
immutable build recipe. Both arms get all normal repository instructions. Do not hide the
process to make the baseline fail. These descriptions are not reflexes to insert into Adage.

## Split before measuring

The sampling frame is a recorded list of eligible real issues, with inclusion/exclusion reasons
fixed before treatment outcomes. Record repository, base revision, original request, provenance,
related issue/feature keys, expected behavior and required runtime for every task. Reference
patches, future Git objects and private checks remain outside candidate checkouts. Starting
snapshots contain the full ordinary project, not just the files we expect an agent to edit.

The independence unit is a business issue or failure mechanism, not a PR number. Introduction
and follow-up correction of the same feature belong together. Paraphrases, seeded variants,
retries and repeated model runs do not increase the group count. A human must review semantic
relationships; a machine cannot discover omitted relationships from renamed identifiers.
Also inspect source chronology: a later training snapshot must not contain a held-out fix.
Public historical fixes may already be in model pretraining. An untouched-by-us split does not
establish model-unseen customer work; prospective customer tasks provide a stronger check.

Use three disjoint pools:

1. **Training:** untreated native sessions only. Preserve every original history, grade, request,
   failure and interrupted attempt. Export/import through the canonical production learner;
   preserve support groups, rejected candidates and reasons. No useful candidate is valid.
2. **Development:** paired baseline/treatment tasks used to diagnose delivery, quality and usage.
   Freeze each development comparison before running it. Any observed result used to select or
   change an implementation stays in development. Keep prior exposed groups and their aliases
   in the exposure ledger, including abandoned experiments.
3. **Confirmation:** untouched independent groups, selected under the preregistered sampling rule.
   Freeze N, task membership, model, code, learned artifact, grader, analysis, budgets and stopping
   rules after development. Do not tune, replace expensive failures or expand N until it passes.

Within development, validation precedes a further held-out development cohort. Release it only
after inspecting validation regressions, actual delivery, irrelevant hints and accounting.
Results used for this decision never become final confirmation evidence. The existing pilot
coordinator already has validation/test staging; those small cohorts remain development-sized.
All four current investigated examples are development material. No final cohort is allocated.

## Quality authority and source isolation

The trusted parent owns the grade. Candidate code executes in a separate, bounded, offline
worker. The parent selects private cases and compares typed observations or persisted database
state with expectations after the worker stops. It must not import candidate code into the
process that owns the answer key, grade files or acceptance decision. Candidate-produced JUnit,
JSON, success messages and exit status are supporting observations only.

For each admitted task, retain reference-pass, original-base-fail, plausible-partial-fail and
forged-report rejection controls. Include stale generated artifacts and unaffected behavior.
Review full requirement coverage explicitly. A narrow parser AST check, lexical schema check or
successful command does not prove missing spans, controllers or API behavior. Controls can
expose harness bugs: preserve the first failed attempt and version the repaired grader.

Pin source archives, reference material, graders, control observations, dependency locks,
compiler/generator binaries and container images. Use clean source-only candidate checkouts.
The current Bubblewrap profile confines files but shares networking with the model CLI. Disabling
WebFetch/WebSearch does not prevent Bash from downloading a public reference solution. A verified
provider-only egress path, with all build dependencies cached, remains a prerequisite for these
historical-code studies. Offline grader isolation alone does not satisfy that prerequisite.

A checksummed report records its author's assertion; it does not authenticate the author or
prove the grader is sufficient. Review the implementation and controls before marking readiness.

## Free inventory registration

`evals/live/study.py` audits preparation and captures declared inputs outside Git. It makes no
model calls and has no paid-dispatch command. `ready_for_sealing` refers only to the preparation
record; it is not spend authorization, a complete runtime protocol or launch qualification.

```sh
python3 evals/live/study.py audit /absolute/private/scenario-plan.json --phase train
python3 evals/live/study.py audit /absolute/private/scenario-plan.json --phase development
python3 evals/live/study.py audit /absolute/private/scenario-plan.json --phase confirmation

# Only after the requested phase has no preparation blockers:
python3 evals/live/study.py seal /absolute/private/scenario-plan.json \
  --phase development --out /absolute/private/development-registration-v1
python3 evals/live/study.py verify /absolute/private/development-registration-v1
```

Start with [the incomplete example plan](../evals/live/configs/launch-study.example.json).
An empty cohort, unknown budget or absent evidence remains blocked. `audit` is read-only and
returns blockers as JSON. `seal` refuses blockers and existing destinations, preserves every
declared input by SHA-256, and records the registration/tool digest. `verify` checks retained
bytes even if the working inputs later change. Keep the digest in an independent timestamped
record before execution; a local digest is not proof of prospective registration.

Each task requires `id`, `group`, `related_keys`, `phase`, explicit `exposed`, `provenance`
(`historical`, `prospective` or `synthetic`), full `base_commit`, and absolute pinned `prompt`
and `controls` paths. `inputs` maps absolute regular-file paths to SHA-256. A control report
contains matching `task_id`/`base_commit`, `quality_authority: "external_parent"`,
`kind: "semantic"`, a reviewed coverage name, `remaining_omissions: []`, its full dependency
`inputs`, and `cases` with `role` and boolean `accepted`. Required roles are `reference` (true),
`base`, `partial` and `forged_report` (false). Merely writing that metadata is not validation. The auditor checks declared relationships and
artifact integrity, not the truth of a coverage review, the contents of a power analysis, provider
authenticity or whether an omitted dependency exists. The operator must verify the selected N
against the power report and review the actual source/observation closure before sealing.

Readiness entries name a `status` and pinned `artifact`: source closure, runtime, parent grader,
provider-only egress and group review; paired phases additionally require learned lineage,
operator review and delivery controls. Confirmation additionally needs a development decision,
power analysis and multiplicity policy. Targets and dollar allocations in integer microdollars
are phase-specific. All remain unset until there is an executable, reviewable schedule.

This registration supplements the existing collector. Before paid work, prepare generator,
external grader, explicit prompts and task identities in a live configuration; bind the sealed
input closure into `frozen.py prepare` with `--input`. Check its actual runtime plan. Approval of
learned candidates and paid dispatch still require the normal operator terminal. Never remove
agent-session markers. An incomplete adapter is a blocker, not permission to use a looser runner.

## What the numbers must establish

The primary estimand is `1 - sum(treatment input + output) / sum(baseline input + output)` over
all scheduled paired groups. Cached input is counted once inside input; report cache reads and
creation separately. Use the existing paired bootstrap and exact conservative quality bound.
Require a one-sided 95% token-savings lower bound of at least 20%, a quality lower bound no worse
than -2 percentage points, and baseline success of at least 80%, under the unchanged claim policy.
Report calls, latency, CLI dollar estimates and actual billing separately. An input-token saving
with a different cache mix need not save the same percentage of dollars.

The 200-group floor applies **per separately claimed scenario**. It is not a power calculation.
Development estimates paired variance, expensive tails and quality discordance. Simulate the
joint acceptance gates to choose a fixed N >= 200 before confirmation. Record assumptions and
sensitivity; one regression can matter substantially at this size. The current evaluator gives
individual comparison bounds. Four selectively published positive results do not gain a joint
95% guarantee. Before confirmation, register and validate a multiple-comparison method or limit
claims explicitly to individual comparisons; do not label a pooled average as four proven cases.

Measure treatment assignment, applicability and actual delivery separately. Keep no-delivery
runs in the assigned-treatment analysis. Count hint overhead, unnecessary interventions,
duplicate verification, all retries and auxiliary calls. Unknown usage, ungraded outcomes,
missing native evidence or isolation failures stop subsequent paid dispatch. Resume only jobs
that never started. Complete negative studies remain results; incomplete studies cannot qualify.

Report training and setup cost separately from evaluation. Show cost per independently successful
task, and break-even workload only when there is a positive measured recurring margin. Billed
cost and billed ROI remain unknown without billing attribution. Record all earlier exploratory
spend too; do not count the same recovered history twice.

## Budget and publication

No new paid allocation is authorized for this portfolio. Four independent 200-group cohorts
would require at least 1,600 evaluation sessions. At an **illustrative**, unvalidated $1 session
cap, that alone reserves $1,600, before training/development; real task caps and powered N may be
higher. This arithmetic is neither a proposal to dispatch nor evidence that $1 can complete a
real Coder or cert-manager task. Prepare one exact staged proposal after free readiness, with
run counts, per-run caps, stage maxima, stopping gates and the decision each stage resolves.

Publish a per-scenario evidence card only after confirmation. It must name the task population,
model/date, N, success rates and regressions, ratio-of-totals savings and interval, calls,
input/output/cache totals, actual delivery, CLI versus billed cost, training cost, limitations,
protocol digest and reproduction link. Show failed or incomplete scenarios too. Until then the
website may explain the process problem and planned evaluation, with **savings unmeasured**.
Never convert grader-control counts or historical descriptive averages into a launch percentage.

## Free preparation results, 2026-10-06

These are grader controls, not agent evaluations or savings measurements. All four scenarios
remain blocked for paid admission and launch claims. No new model calls were made.

| Scenario | Observed control result | Remaining limitation |
| --- | --- | --- |
| Coder default organization | Independent base 3/9, reference 9/9; includes compiled Go Store behavior and artifact parity. Later cold-mod candidate 6/9, task failed | Broader API/UI checks and complete training admission remain incomplete; no savings comparison |
| TypeScript generic decorators | Reference passes 222/222 named-tree checks; base/stale parser 212/222; single-dialect fixes 217/222; six false-success/protocol controls rejected | Fields, unnamed tokens and spans omitted; strict generated metadata differs; other task graders incomplete |
| cert-manager OtherNames | Reference accepted; base and four stale artifacts rejected | Structural controls only; source/build closure and issuance/renewal semantic grader incomplete |
| dbt historical corrections | All nine dbt invocations exit zero; parent accepts gold and rejects base/forged-base in all three data phases | Synthetic fixture only; no real customer task cohort |

The first Coder wrapper error and TypeScript generator resource failures were retained before
corrected free controls. They were not paid attempts or silently replaced model outcomes.
Coder has five proposed training groups and one exposed development group; TypeScript has four
exposed candidate groups; cert-manager's introduction/follow-up form one exposed group; dbt has
zero real groups. There are zero untouched confirmation groups. These counts describe curation,
not admitted runnable tasks. Raw sources, outputs and reproduction scripts remain private.

## Claude mod integration after PR #27

The completed Coder development scout loaded the merged Claude Code mod itself, with Claude
Code 2.1.288 and both Adage binaries pinned. Its one authorized session used a $2 CLI cap,
60 turns and 1,800 seconds. It stopped at the turn cap, failed the task (6/9 checks), and cost
a CLI-estimated $0.3157685; billed cost remains unknown. No retry or expansion is authorized.
This exposed task began with an empty learned store, so the result is a cost/activation
diagnostic. The earlier baseline-only preparation is retained as superseded. See the
[complete scout audit](claude-mod/evaluation-results.md).

Before training or comparing learned mod treatment, register its complete plugin source, binary
hashes, model, runtime settings and operator grants. Keep the same model in both arms and disable
model routing. Use the independent semantic grader and verify native model requests, module
activation, reflex delivery, prediction/receipt provenance and duplicate-hook absence. The old
E0 frozen collector does not freeze these extra treatment inputs; do not label its output a mod
comparison. Failure or incomplete accounting stops paid expansion.

## Coder migration controls

`evals/live/postgres_grade.py` supplies one external-parent database component for real Coder
training tasks. A private contract pins the clean predecessor commit, old-data fixtures,
upgrade/fresh-schema/downgrade probes and expected JSON or SQLSTATE. The grader captures the
executed SQL bytes and hashes, rejects modified historical migrations and linked inputs, and
preserves commands, observations and errors in a new private output directory. It does not read
candidate-authored grade reports or execute candidate programs on the host. Candidate SQL runs
as a non-superuser inside an offline, bounded PostgreSQL container with no host bind mounts.
The parent alone supplies privileged fixture setup and decides whether observations pass.

On 2026-10-06, free controls for Coder PRs #7202 and #9699 accepted both historical reference
implementations and rejected all eight negative controls: unchanged base, plausible partial
fix, stale schema dump and forged success reports for each task. The checks distinguish active
case-insensitive uniqueness from soft-deleted duplicates, cover the historical experimental
proxy cleanup and downgrade, and detect UTC timestamp shifts when migration sessions use
America/Los_Angeles. Fresh schemas, old rows, defaults, offset timestamps and downgrade behavior
are checked separately. These are ten grader controls, not ten independent agent tasks.

Additional offline controls ran the historical proxy constraint-enum generator: its output
matched the reference Go constants and differed from stale base constants. SQLC 1.17.2 and
1.20.0, acquired from their official releases and pinned by local artifact hashes, reproduced
the committed model files byte for byte for both base and reference of their respective tasks
(four generation controls). These component checks do not cover the complete generator pipeline
or compilation and behavior of the full Coder database package.

```sh
python3 evals/live/postgres_grade.py \
  --base /absolute/private/pinned-clean-base \
  --candidate /absolute/private/candidate \
  --contract /absolute/private/contract.json \
  --postgres-profile 13.11 \
  --out /absolute/private/new-control-output
```

The current component reports `database_behavior_only`. The ten controls also passed their
expected positive/negative outcomes with the explicit pinned PostgreSQL 13.11 profile. Separate
private Go Store probes accepted both references and rejected both unchanged bases, with the
parent observing actual rows after the compiled Store methods ran. Complete requirement review
and one composed task grader remain necessary before paid admission. Neither these controls nor
the cold-mod cost scout establish learned-mod savings. See
[the scout results](claude-mod/evaluation-results.md) for the independently failed paid attempt.


A further free replay used complete, clean source-only reference snapshots with Go 1.20.14,
PostgreSQL/pg_dump 13.11, the historical SQLC versions above, and pinned goimports 0.19.0.
For both tasks, forced migration-to-schema generation and the complete historical
`coderd/database/generate.sh` succeeded with no tracked output differences. The proxy reference
passed 1,067 Go test/subtest events across `./coderd/database/...`. The timestamp reference
passed 1,034 events, with one failure: `TestPubsub_Disconnect` requires a Docker daemon unavailable
in this environment. Nothing was skipped or replaced to remove that failure. These event counts
include subtests and are not independent task counts.

The first replay's incompatible modern dump-client output and the remaining Docker-dependent
failure are retained. Successful generation does not replace the external behavioral grader.
The remaining proposed training references (#10152, #11069 and #11204) also reproduced the
forced schema and complete generated output without changes, using SQLC 1.20.0, 1.20.0 and
1.24.0 respectively. They passed 1,039, 1,068 and 1,071 test/subtest events; each retained the
same `TestPubsub_Disconnect` failure, with no skipped tests. These reference checks do not by themselves establish task-specific semantics.

Private PostgreSQL 13.11 contracts now cover those three tasks as well: all three references
passed, and twelve base, partial, stale-schema and forged-report controls were rejected.
Checks exercise OAuth JSON/null storage, old and new user preference defaults, independent
provisioner API versions, null constraints and downgrade behavior. Actual Go Store probes
also accepted these three references and rejected their bases. Three further negatives kept
correct schemas while breaking OAuth JSON updates, user preference reads or provisioner
conflict updates; all were rejected by the independent probes. The final case retained API
version `2.3` after an attempted update to `3.1`, which the parent database observation caught.

Four initial Go controls for the theme and provisioner cases ran out of bounded compiler
scratch space. Their outputs remain preserved as infrastructure failures, not quality
rejections. New output directories with larger bounded scratch space completed the controls;
assertions and task code were unchanged. These are component checks, not paid agent results.
A composed candidate grader, complete requirement scope (including broader OAuth behavior),
and frozen mod treatment support remain unfinished. No additional paid model calls were made.


The OAuth task's broader configuration check subsequently failed on its pinned historical
reference: extra-token storage worked when keys were supplied directly, but configured and
provider-default keys did not reach the runtime. A separately labeled corrected control
passed the identical offline probe. Both results and the original reference are preserved;
the corrected control is neither an agent trace nor a learned reflex. The historical reference
is therefore unsuitable as a full-scope positive oracle without resolving this gap. Database
passes do not justify narrowing the task or waiving the missing configuration behavior.
