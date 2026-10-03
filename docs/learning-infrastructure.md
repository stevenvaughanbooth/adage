# Trace learning infrastructure

Working branch: `codex/trace-learning-infrastructure`. Base: `2534963`.

## Objective

Turn repository-scoped agent traces into reproducible, independently supported reflexes. Scale
collection and processing without mixing repository evidence, inflating confidence with retries,
or interpreting repeated behavior as demonstrated benefit. The business target is 20–30% lower
token spend at preserved task quality; it is a prospective evaluation target, not a measured
claim. Cheaper-model predictability requires a separate model-transfer comparison.

## Milestones and acceptance

| Milestone | Deliverable | Acceptance | Status |
| --- | --- | --- | --- |
| M1 Evidence independence | Canonical task/group identity, preserved in graph calibration and route qualification | Retries stay in one fold; explicit groups survive import; unknown identity cannot establish task diversity; old traces remain readable | Complete |
| M2 Corpus ingestion | Versioned, repository-scoped canonical corpus with bounded streaming and auditable rejection/deduplication | Input order does not change evidence; conflicting duplicates fail visibly; malformed and oversized records cannot silently become evidence | Complete |
| M3 Scalable learning | Reusable repository mining checkpoints with explicit invalidation | Repeated runs avoid unchanged mining; changed/deleted evidence and mining parameters invalidate correctly; full/warm artifacts agree | Complete |
| M4 Outcomes and value | Conservative completion evidence and reproducible paired token/quality evaluation | Stale checks do not label final work successful; quality regressions reject savings claims; unknown usage and contaminated comparisons are visible | Complete |
| M5 Verification and delivery | Required Rust checks, deterministic corpus/scale measurements, signed commits, remote branch and review | Results and limitations recorded against a commit; no unsupported savings or enterprise-capacity claim | Pending |

## Implementation boundaries

- Keep the existing ingestion, canonical trace, graph, learner, store, and evaluation layers.
- No new hosted service, mandatory database, online model call, or harness execution authority.
- Keep interventions distinguishable from independent demonstrations.
- Use the existing `Store` boundary for persistent learning state. Cache errors may cause a
  recomputation, never silently reuse incompatible evidence.
- Persist provenance and computation identities; keep raw trace text out of aggregate reports.
- Preserve user review decisions and effect ceilings.
- Use held-out task groups and quality constraints to assess benefit. Recorded trace replay
  measures opportunities, not causal token savings.

## Progress log

- 2026-10-03: Inspected the current source and research, cloned the complete repository, created
  the working branch, and installed a workspace-local Rust toolchain. Baseline verification is
  running. Confirmed the production bridge drops task identity before calibration and the route
  judge uses a separate truncated-prompt heuristic.
- 2026-10-03: Baseline required a new stable-Rust lint fix. The managed sandbox requires network
  permission for tests that bind a local socket, a UTF-8 terminal setting and a temporary root
  without its injected `.git` directory. Tests retain their assertions.
- 2026-10-03: M1 implemented. Task identity survives the canonical bridge, evaluation manifests,
  parse-cache reads and route qualification. Calibration refits prototypes and candidates on
  independent task representatives. Identical canonical deliveries are deduplicated; conflicting
  repository/session ids reject the run before evidence writes. Scoped canonical learning filters
  other repositories itself. Existing repository attribution is retained for removed subdirectories
  and worktrees, covered by regression assertions.

## Deterministic fixture strategy

### Reusable mining

Learning persists one G1/G2 mining checkpoint per repository under `$ADAGE_HOME/cache/mining`.
Keys cover sorted canonical sessions, mining parameters and the SHA-256 of the running executable;
worker count does not affect the key. The binary digest is computed once per process. Checkpoints
include the compiler-only patterns, unselected candidates and dormant-route counts omitted from
public mining reports. JSON float round trips preserve confidence values exactly. Entries are
private on Unix, checksum-verified, atomically replaced and capped at 64 MiB; incompatible,
missing, oversized or corrupt entries cause full recomputation.

Only pure mining is reused. Checkout facts, priors, compilation, qualification, current reviews,
effect ceilings and final index publication run each time. Dry `plan`/`sense` calls do not write
checkpoints. `condition --no-cache` and `corpus learn --no-cache` force recomputation; reports
show `engine.mining_cache_hit` per repository. The cache is optional for other `Store` adapters.

New or changed evidence still causes a full repository mine. The existing frozen-epoch negative
border algorithm remains tested but is not used across changing prototype epochs: that would
need a separate epoch/invalidation contract. This milestone provides exact computation reuse
and incremental corpus updates, not approximate online learning or distributed mining.

### Canonical corpora

```sh
adage corpus export trace-a.jsonl trace-b.jsonl --repo acme/api --identities identities.json --outcomes outcomes.json > corpus.jsonl
adage corpus import corpus.jsonl --repo acme/api
adage corpus inspect --repo acme/api --evidence > training-evidence.json
adage corpus learn --repo acme/api
```

Each JSONL record is `{"schema":1,"session":<canonical Session>}`. Import processes one record at
a time, limited to 8 MiB and 50,000 steps; at most 100 line/reason diagnostics are retained while
all rejection counts are reported. Invalid JSON, unknown schemas, malformed session structure,
oversized records and conflicting duplicate ids prevent manifest publication. Other repositories
are counted separately and never mixed into the selected repository. Identical re-delivery is
idempotent. Export uses the recorded checkout location rather than inspecting this machine.

`corpus import --replace` explicitly replaces one repository's snapshot, including corrections
and removals. Two conflicting records inside that same replacement still fail. An empty
replacement followed by learning retires unsupported learned routes; authored routes and review
records retain their normal protections. Selecting a repository absent from a nonempty input is
an error, so a selection typo cannot clear its snapshot.

Canonical objects are SHA-256 addressed and sharded by digest prefix under `$ADAGE_HOME/corpus`;
owner-only files on Unix are published before one atomic repository manifest. Imports hold the
normal writer lock. A failed import may leave unreferenced objects but cannot publish a partial
manifest. Reads verify object content and repository/session ownership. Corruption is an error,
not silently missing training data. Automatic garbage collection is not implemented.

Ingestion memory is one bounded record plus the selected repository's id/digest manifest. Mining
currently loads one repository's canonical sessions. This is a portable collection foundation,
not a claim of distributed execution or bounded memory for an arbitrarily large single repository.

### Evidence identity

`adage condition --transcripts trace.jsonl --identities identities.json` accepts a map such as
`{"session-1":{"task_id":"BUG-7","group_id":"retry-family-7","attempt_id":"2"}}`.
Canonical sessions carry the same optional `identity` object. Group id takes precedence over task
id; either is scoped to the repository. With neither, normalized full prompt text gives an
inferred fingerprint. Empty prompts are unknown, not one new task per session. Prompt inference
cannot detect semantic paraphrases: curated training/evaluation datasets should declare groups.

Calibration deterministically selects up to 400 groups by SHA-256 order and one session per group
by session-id order, without consulting outcomes. Every fold refits its prototypes, mines and
scores on its training side. It no longer reuses all-data prototypes or switches to optimistic
all-data candidate selection on large corpora. Sparse groups can yield no calibration pairs;
raw observational confidence is still not a causal benefit estimate or an execution approval.
Route admission still uses its existing support/strength calculation and a minimum task-diversity
gate; the cross-fitted calibration report does not replace that calculation. A future admission
change needs its own held-out reliability evidence.

### Outcomes and paired value

`outcomes.json` maps session ids to independently graded booleans, for example
`{"session-1":false}`; omit a session when its outcome is unknown. These recorded labels take
precedence over trace proxies. Without a recorded label, an interrupted/help-seeking ending is a
negative proxy. A check can supply a proxy only when it follows every mutation in a later model
turn and serves the latest prompt. A stale check or no check leaves the outcome unknown. A
passing check remains a proxy for completion, not an independent task grade.

`corpus inspect --evidence` verifies one snapshot and returns its training generation, canonical
group keys, identity coverage and outcome-source counts. `corpus learn` and `learn.log` carry the
generation actually read. Copy `training_generation`, `training_groups`,
`training_inferred_identity` and `training_unknown_identity` into a paired study. Qualification
requires declared training identities; inferred prompts cannot prove separation from declared
evaluation task ids. Use the same repository keys and task/group registry across both sets.

```sh
adage eval paired study.json
adage eval paired study.json --require-token-target
adage eval paired study.json --require-cost-target
```

The input schema is illustrated by
[`paired-study.json`](../crates/adage-eval/tests/fixtures/paired-study.json), a synthetic one-pair
format fixture that deliberately cannot qualify a commercial claim. Reports are JSON; a required
target exits unsuccessfully when evidence does not qualify. `same_model` compares one model with
and without the intervention; `model_transfer` compares a fixed baseline/intervention model pair.
Do not mix models within one study. Each run identifies its task, environment and evaluator
revisions. The environment revision must pin checkout, harness, tools, inputs and budget while
the chosen experimental treatment is recorded in the study protocol. Retries contribute no
additional independent pairs. Reused run ids, overlapping training groups, unknown outcomes or
mismatched conditions invalidate qualification.

Quality uses a conservative simultaneous 95% lower bound on paired improvement minus regression
probability (Clopper–Pearson bounds with Bonferroni adjustment). The default allows at most two
percentage points of quality loss, requires an observed baseline success rate of at least 80%,
and never accepts fewer than 30 independent tasks. Thirty is only a floor: with zero discordant
pairs, about 183 tasks are needed even to establish the two-point margin. Predeclare the margin,
task population and sample size; repeatedly stopping when a threshold passes is not covered by
these fixed-sample bounds.

Token, model-call and billed-cost totals are separate. Input tokens include cached input once;
cached tokens are not added again. Costs are recorded millionths of a US dollar, never guessed
from token counts. Missing usage/cost remains missing. Savings are the ratio of aggregate totals,
with a deterministic 2,000-resample task-pair bootstrap and an approximate one-sided 95% lower
percentile bound. The default target is 20%; acceptance needs both the quality gate and that
metric's savings bound. A bootstrap is an empirical estimate under representative independent
task sampling, not a distribution-free guarantee about unseen enterprises. An all-identical
synthetic fixture tests arithmetic; it provides no external validity.

### Training and promotion workflow

1. Collect native histories, task/group ids, model and repository provenance, intervention
   exposure and independent outcomes. Keep failures, timeouts and unknown outcomes. Respect the
   existing influence exclusion: behavior caused by Adage is not a new organic demonstration.
2. Split task families before fitting, including paraphrases and retries. Use repository and
   time holdouts for transfer claims. Keep a final test set untouched while tuning on validation.
3. Import the training snapshot, inspect identity/outcome coverage, learn candidates and review
   support, effects and uncertainty. A candidate is a hypothesis, not demonstrated savings.
4. Freeze the corpus generation, compiled artifacts and policy. Run baseline and intervention
   on fresh equivalent checkouts with randomized arm order, pinned model/settings and the same
   independent grader. Count all scheduled tasks, failures and fallbacks. Sum the whole workflow's
   model usage, including planning, hook/context overhead and retries.
5. Evaluate the complete predeclared paired cohort, then use the existing review/enable/compile
   controls for qualified candidates. Start narrow, measure live exposure and quality, and retain
   rollback and extinguish behavior. Test cheaper-model transfer in a separate experiment.

The next highest-value component is an experiment runner that freezes this protocol, collects
provider billing/usage and complete outcomes, and binds each arm to an artifact digest. This
evaluator checks submitted evidence; it cannot independently attest its completeness or grades.
After real benefit is established, prioritize exact changed-evidence mining and worker/store
adapters for large repositories, then task-level reliability and drift-driven requalification.
The defensible product asset is the accumulated, permissioned evidence linking a typed reflex
and its applicability to verified savings and quality across environments. Streaming storage,
cross-validation, checkpointing and paired experiments are established techniques; this change
does not establish patent novelty or freedom to operate.

Outcome evals establish whether an intervention helps a real agent; they are not the only test
of learning. Each milestone includes these lower-level contracts where relevant:

| Boundary | Fixture and assertion |
| --- | --- |
| Raw trace → canonical session | Equivalent Claude, Codex and OpenCode histories; preserve prompts, order, results, attribution and unknown metadata |
| Session → evidence | Explicit task groups, retries, paraphrases, empty identity, truncation, duplicate and conflicting records, stale checks and intervention provenance |
| Evidence → candidates | Small hand-authored sessions with expected routes, support, rejection reasons and abstentions; negative examples differ in the condition that matters |
| Candidate → artifact | Stable ids, typed bindings, effect ceilings, user review preservation and serialized round trips |
| Incremental → full | Exactly equal artifacts after insertion, reordering, replacement, removal and configuration changes; corrupt checkpoints trigger recomputation |
| Corpus → report | Repository isolation, bounded records, deterministic manifests, restart behavior, missing usage and rejected quality regressions |

Use table-driven canonical fixtures for behavior, property/metamorphic tests for invariants, and
small end-to-end CLI fixtures for wiring. Keep expected outputs explicit and reviewed; never
derive an expected answer by calling the same implementation under test. Every fixed learning
bug gets a fixture at the lowest boundary that reproduces it. Scale benchmarks report resource
cost separately from correctness and live outcome claims.

- 2026-10-03: M2 implemented: bounded canonical JSONL, repository snapshots, private content
  objects, transactional imports and export/import/inspect/learn CLI commands. Reviewed the one-line
  help snapshot addition. Corpus tests verify interruption, corruption, idempotence, conflicts,
  repository boundaries, exact learned candidate support and retirement after evidence removal.

## Verification record

Foundation: workspace formatting and all-target Clippy pass on Rust 1.99; all 18 architecture,
affected-path and hygiene guards pass. The network-enabled baseline passed the CLI's 108 unit
tests before reaching three execution integration failures: this container cannot create the
user namespace Bubblewrap needs (`setting up uid map: Read-only file system`). Those tests
remain unchanged. This is an environment limitation, not a passing full-suite result.

M1: all trace, ingestion, graph and learner suites pass, including the existing property tests
for input-order/thread determinism and incremental/full mining equality. The 28 evaluation unit
tests pass. Final duplicate/identity qualification changes also pass focused learner unit,
routine execution, remote-session and parse-cache equivalence tests. Workspace all-target Clippy
and architecture guards pass. No live model outcome was measured.

Live paid studies require an explicitly bounded run and available model credentials. Engineering
checks and offline measurements do not establish production savings.

M2: all ingestion, learner, store and architecture suites pass, as do the CLI corpus lifecycle,
all 11 CLI grammar/help tests and both no-spawn guards. All-target workspace Clippy passes.

M3: all learner tests, architecture guards, CLI corpus lifecycle and no-spawn checks pass.
Checkpoint fixtures compare complete tracks/why/reviews and index bytes to full computation
after each update, verify corrupt-cache recovery and confirm current human decisions still apply.
All-target workspace Clippy passes. Scale timings remain pending.

M4: all 28 evaluation unit tests and eight paired-outcome integration tests pass, alongside the
canonical outcome fixtures, learner tests and two CLI corpus/evaluation flows. All-target
workspace Clippy passes. Full-workspace verification and scale measurements are recorded below
when complete; no live outcome study has been run.
