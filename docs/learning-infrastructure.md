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
| M5 Verification and delivery | Required Rust checks, deterministic corpus/scale measurements, signed commits, remote branch and review | Results and limitations recorded against a commit; no unsupported savings or enterprise-capacity claim | Complete locally; hosted CI blocked by account billing |
| M6 Frozen experiments | Immutable protocol, complete scheduled cohort, artifact integrity and workflow accounting | Missing jobs, retries, changed artifacts/settings, unknown usage and reused provider requests cannot qualify a savings claim | Complete locally |
| M7 Live evidence collection | Native provider accounting and independent grading connected to frozen jobs | Partial usage cannot look complete; no terminal attempt is rerun or discarded; observed provenance matches execution | Complete locally; live provider study remains unmeasured |
| M8 End-to-end product proof | Native evidence → learning → review → hook → independently correct fresh outcome; actionable study preflight | No inserted reflexes, actual delivered commands repair held-out state, negative controls abstain; inert artifacts and execution blockers fail before spend | Complete locally; live model uptake and savings remain unmeasured |
| M9 Local evaluation handoff | Train-only archive bridge, scenario observation, diagnostic fixtures, operator runbook and claim policies | Native identities/outcomes survive real export/import; template siblings cannot impersonate independent tasks; action observations cannot impersonate passing tests | Complete locally; paid execution belongs to the model-enabled operator |
| M10 Pilot launch and completion evidence | Runnable learned E0 pilot, disjoint task pack, canonical treatment lineage, durable optional required checks and restart-safe obligations | No model spend during preparation; every paid attempt retained; exact learned artifact reviewed before use; missing, failed or stale checks never become current passes | Complete locally; paid execution remains operator-controlled |

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
  the working branch, and installed a workspace-local Rust toolchain. Confirmed the production
  bridge dropped task identity before calibration and the route
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
- 2026-10-03: M2 implemented: bounded canonical JSONL, repository snapshots, private content
  objects, transactional imports and export/import/inspect/learn CLI commands. Corpus fixtures
  verify interruption, corruption, idempotence, conflicts, repository boundaries, exact candidate
  support and retirement after evidence removal.
- 2026-10-03: M3 implemented: checksummed, content/parameter/build-keyed mining checkpoints;
  full/warm artifact equivalence fixtures and current human decisions checked on cache hits.
- 2026-10-03: M4 implemented: conservative outcome proxies, independent outcome import, corpus
  evidence audits and paired quality/token/cost qualification. Fast evaluation contracts now run
  in PR CI. Published signed milestone commits and opened
  [draft PR #31](https://github.com/stevenvaughanbooth/adage/pull/31).
- 2026-10-03: M6 implemented. Experiments freeze the audited training snapshot, task cohort,
  model/settings, compiled reflex set, budget and grader. Content-addressed receipts retain
  failures and missing jobs in the denominator and charge every recorded workflow request.
  Provider execution stays in the live harness; M7 adds its automatic receipt adapter.
- 2026-10-04: M7 native accounting adapter implemented. Real archived CLI traces demonstrate that
  ordinary assistant chunks carry provisional output counts and `costBasis=list` costs. The
  adapter requires completed request counters reconciled to terminal/per-model totals, preserves
  incomplete accounting, and never labels list-price estimates as actual bills. Eleven
  deterministic adapter/grade tests cover these contracts.
- 2026-10-04: M7 frozen execution implemented in the existing Python live harness. Preparation
  pins actual training provenance, the complete held-out cohort, settings and source inputs;
  execution checks fresh checkouts, preserves immutable terminal results and stops interrupted
  attempts from spending again. The offline Rust validator verifies source-object integrity.
  Local process tests exercise deadlines and invalid grades; simulated agents cross the real
  Rust protocol/report boundary without model credentials or paid calls.
- 2026-10-04: M8 checked the latest remote `main` (`2534963`, already contained by this branch)
  and added a single-command native-trace-to-delivery proof. A learned, reviewed dependency
  sequence corrects a fresh independently graded task through the real hook; no authored reflex
  is inserted. Operator studies now reject inert artifacts and wrong repository routing and show
  actionable readiness before execution. Resume checks reconcile the immutable job journal with
  the original admission ledger, including costs retained in native source evidence.
- 2026-10-04: A bounded live Codex canary found a stored ChatGPT login but received HTTP 401:
  its token could not be refreshed after logout/account change. It failed before inference.
  Claude Code is not installed and no provider credentials are configured here. Live model
  uptake and token-savings measurement therefore remain blocked on restored provider access;
  the free proof establishes deterministic mechanism and outcome only.

## Implemented interfaces

### Run a model-enabled local evaluation

Start with [the packaged paid-pilot guide](paid-pilot.md). `evals/live/pilot.py` creates a private
pilot root, fixes one total admission allocation across 20 training runs and ten held-out arm
runs, collects real training traces, exports them through the canonical corpus, runs production
learning and stops for actual candidate review. It freezes validation and test before either
cohort is collected. The 15 semantic groups are synthetic programming and documentation tasks;
siblings share an identity and entire groups stay in one split. Unknown accounting or grading
blocks further spend. Original receipts and failed attempts survive resume.

`adage eval experiment sources <tracks-dir>` exports full typed learned candidates and a
recompiled index without writing files. The pilot checks that enabled behavior came from its
original production learning output, allowing only reviewed lifecycle/strength changes. This
does not turn a synthetic task pack into representative customer evidence. Small-group quality
bounds remain strict; a first paid pilot can produce a useful negative or inconclusive result.

Give the receiving agent [the local evaluation handoff](local-agent-evaluation.md). It includes
capability boundaries, all nine missed-action scenario specifications, free checks, a three-run
diagnostic config, independent task/group registry, train-only normalization bridge, normal
learning/review, frozen validation/final commands, budgets, controls and expected report contents.
The supported frozen runner remains Claude Code on Linux with E0 advice. Broader PR enforcement,
external-tool recovery, static-rule comparisons and context/waiting claims need the explicitly
listed implementation/instrumentation work before they can be advertised.

### Run the end-to-end proof

From the repository checkout on Unix, with Rust and `make` installed:

```sh
cargo test --locked -p adage-cli --test learning_delivery -- --nocapture
```

The test creates a disposable repository and runs actual local commands. Sixteen distinct
native-format rehearsal sessions enter through `corpus export`, `import` and `learn` with
declared task identities and independently checked outcomes. Eight demonstrate a dependency
update; eight unrelated documentation tasks provide controls. No reflex is inserted by the test,
and production mining thresholds are unchanged. Embeddings are disabled to isolate deterministic
procedure mining and keep this proof independent of model downloads.

The learned `make deps` → `make check` sequence is initially a candidate requiring review.
The isolated test acts as its reviewer through the normal CLI, checks the compiled artifact and
runtime repository, then edits an unseen dependency in a fresh held-out checkout. That checkout
starts with a stale lockfile and fails its check. The real hook supplies the learned continuation;
the fixture executes exactly those delivered commands and independently verifies both files.
Before review, with Adage disabled and on an unrelated edit, the hook abstains. Repeated delivery
is suppressed, repeat import/learning preserves evidence and review, and held-out work never
enters the training snapshot.

The final `learning_delivery_proof={...}` line is a machine-readable account of these assertions.
It explicitly reports `model_calls: 0` and `token_savings_measured: false`. This establishes that
the mechanism transfers a learned procedure to a new input and corrects its state. A real model's
decision to follow the reflex, net inference savings and population-level quality need a live
held-out experiment. The [frozen collector](../evals/live/README.md#frozen-studies) prepares that
next step and reports missing prerequisites in a read-only plan.

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

The [frozen experiment workflow](learning-experiments.md) implements protocol registration,
artifact/receipt integrity and complete cohort qualification. Use **reflex** for an individual
behavior, **reflex set** for its versioned collection, **candidate** for an unqualified mined
hypothesis and **pack** for a distributable bundle. Internal `Track` storage names remain as-is.

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

For cheaper-model claims, collect four arms: the stronger and cheaper model, each with and
without Adage. A same-model comparison on the cheaper model isolates Adage's contribution;
stronger-model baseline versus cheaper-model-plus-Adage tests substitution quality and cost.
Improved repeatability also needs repeated runs within tasks and a clustered variance analysis.
The current paired evaluator does not estimate that variance or count retries as independent tasks.

The [frozen live collector](../evals/live/README.md#frozen-studies) now consumes the fixed schedule,
checks execution against pinned inputs and emits independently graded, reconciled request
receipts. The next highest-value step is a representative live study using a reviewed reflex set,
an untouched task cohort and explicit budget. No model credentials are configured in this work
environment, so the current verification uses deterministic fixtures. The experiment layer checks
artifact/settings digests and the complete scheduled cohort. It cannot independently attest
provider completeness, grading, actual training lineage or registration time; those remain
collection/provenance obligations. CLI list-price costs do not qualify as billed savings.
After real benefit is established, prioritize exact changed-evidence mining and worker/store
adapters for large repositories, then task-level reliability and drift-driven requalification.
The defensible product asset is the accumulated, permissioned evidence linking a typed reflex
and its applicability to verified savings and quality across environments. Streaming storage,
cross-validation, checkpointing and paired experiments are established techniques; this change
does not establish patent novelty or freedom to operate.

## Deterministic fixture strategy

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

## Verification record

Foundation: workspace formatting and all-target Clippy pass on Rust 1.99; all 18 architecture,
affected-path and hygiene guards pass. The network-enabled baseline passed the CLI's 108 unit
tests before reaching execution integration failures under the tool sandbox's read-only process
filesystem. A direct namespace probe outside that sandbox succeeded. The final run uses system
Python, a UTF-8 terminal and `/var/tmp` outside any ancestor Git repository; the temporary root
also needs to remain visible inside Bubblewrap. Test assertions remain unchanged.

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
All-target workspace Clippy passes. The scale runner is `scripts/bench-corpus.py`; it uses a
literal canonical fixture, verifies duplicate-import idempotence and checks byte-identical cold,
warm and forced-full artifacts while recording wall time and per-process peak RSS. It disables
embeddings to isolate corpus and graph learning, and makes no production savings claim.

M4: all 28 evaluation unit tests and eight paired-outcome integration tests pass, alongside the
canonical outcome fixtures, learner tests and two CLI corpus/evaluation flows. All-target
workspace Clippy passes. No live outcome study has been run.

M5, code at `5186d81`: `cargo test --workspace --exclude adage-eval --locked --no-fail-fast`
finished with 1,117 passed, one failed and three existing ignored tests. The one failure was the
escape suite's ripgrep positive control: this environment's `/usr/local/bin/rg` is a symlink to
an executable outside the sandbox's toolchain roots. Adding its actual directory
(`/opt/codex/codex-path`) to `PATH` made the complete escape suite pass, 13/13. Across that full
run and the corrected rerun, all 1,118 exercised workspace tests passed; the initial full command
itself exited unsuccessfully. No test was disabled or weakened. Both Bubblewrap execution and
the attack fixtures actually ran. Reproduction here requires `TERM=xterm`, system Python first
on `PATH`, a temp root under `/var/tmp` rather than `/tmp` or `/dev/shm`, and working user
namespaces outside the tool sandbox's read-only process filesystem.

The separate evaluation unit/paired suite passes 36/36. Formatting, all-target workspace Clippy
with warnings denied, 18 architecture/hygiene guards, two no-spawn guards, plugin metadata,
npm launcher (including both companion binaries), session-start script and DCO sign-offs pass.
The release CLI builds with the normal optimized profile. Long evaluation gates are the existing
weekly/manual workflow; they were not run as part of these checks.

Remote CI at `5186d81` did not start: GitHub's job annotation reports failed account payments or
a spending limit that needs increasing. This blocks hosted verification, not local checks.
[CI run](https://github.com/stevenvaughanbooth/adage/actions/runs/37155984759).

M6: 47 evaluation tests pass (28 unit, eight paired, 11 frozen-experiment contracts). All 259
CLI tests pass, including the new object → freeze → report flow. The final accounting change
also passes a focused rerun of both corpus and experiment CLI flows. Formatting, all-target
workspace Clippy with warnings denied, 18 architecture/hygiene guards and both no-spawn guards
pass. Fixtures exercise omitted jobs, retry charges, timeouts, changed protocols/artifacts,
reused provider ids, incomplete accounting, overflow and budget overruns hidden by missing cost.
The local environment has no configured model credentials; no paid study or measured savings
claim is included.

M7: all 86 Python live-runner tests pass, including 11 native accounting/grade contracts and
14 frozen collector/process contracts. The latter run simulated agents against the actual Rust
validator; timeout tests execute local processes, including a child that outlives its parent.
All 259 CLI tests and 48 fast evaluation tests pass (28 unit, eight paired, 12 experiment).
Formatting, all-target workspace Clippy with warnings denied, 18 architecture/hygiene guards and
both no-spawn guards pass. Inputs changed after freezing, visible study directories, corrupted
source objects, grader errors and interrupted resume all fail conservatively. No paid model was
called. The native accounting milestone is `9689cbc`; its
[hosted CI job](https://github.com/stevenvaughanbooth/adage/actions/runs/37186652959/job/111389881517)
again reports that it did not start because account payments failed or the spending limit needs
increasing. The optimized binary used for M5 measurements predates M7; rebuild before preparing
a study with this collector.

M8: the complete `cargo test --workspace --exclude adage-eval --locked --no-fail-fast`
run passes with 1,121 passed, zero failed and three existing ignored tests, including the actual
learning-to-hook proof and Bubblewrap execution/escape suites. The separate fast evaluation
suite passes 51/51; all 101 Python live-runner tests pass with no skips. Resume fixtures reject
missing or rolled-back admission ledgers, duplicate attempts, changed reservation caps and
costs inconsistent with retained native evidence, including unknown costs rewritten as zero.
A valid partial study resumes without
repeating its completed job. Formatting, all-target workspace Clippy with warnings denied,
18 architecture/hygiene guards and both no-spawn guards pass.

The optimized CLI was rebuilt for M8 and its artifact command accepts the checked-in compiled
fixture. Binary SHA-256:
`e39786c0b63845d7477031134c3abecf9402b29fe44cace41713da920e288eda`.
The deterministic end-to-end proof uses 16 training tasks and one fresh held-out task; the latter
fails before receiving the learned procedure and passes its independent grade afterward. It
makes zero model calls and establishes neither a token-savings percentage nor population-level
quality. The live Codex canary failed before inference with expired credentials (HTTP 401).

M9: all 136 live Python tests pass with no skips, including 11 training-bridge contracts through
the actual Rust export/import/learn/audit path and 24 advisory action-order contracts. The nine
cardinal self-checks exercise all 42 synthetic variants, independent reference outcomes,
tampered checks, partial fixes, symlinks and unrelated controls. They do not count those variants
as independent learning/evaluation groups. All 18 architecture/hygiene guards and diff checks
pass. The diagnostic CLI plan schedules exactly three baseline sessions and creates no run root.
Cross-review caught and fixed same-record edit/publication ordering and a registry change during
export; regressions keep both cases conservative. Production Rust is unchanged from M8, whose
full workspace/evaluation results are recorded above. No live models were called for M9.

M10: the full workspace run, excluding the separately tested evaluation crate, passes with
1,154 passed, zero failed and three existing ignored tests. The final executor library includes
one further regression and passes 125/125 in three consecutive parallel runs with actual OS
sandbox staging. Seven required-completion integration cases require a live staging backend;
four binary CLI tests cover readiness JSON/exit codes and protected configuration/recovery.
Fast Rust evaluation contracts pass 55/55, including the canonical source export. All 148 live
Python contracts pass with no skips, and all 11 pilot fixture selfchecks pass across 30 variants.
Formatting, all-target workspace Clippy with warnings denied, 18 architecture/hygiene guards,
both no-spawn guards and diff checks pass.

The complete scripted pilot uses the real task generator and external grader, native canonical
export/import, production-default learning, explicitly simulated operator CLI review, full
canonical learned-artifact checks, frozen paired collection and resume. It records 20 training
and ten held-out arm runs. Equal scripted usage produces no accepted savings; no provider is
called. Review found and fixed older-pass resurrection across sessions, lost-journal linkage,
and a transient inherited-lock race. A malformed synthetic native trace correctly produced no
candidates until its missing message ids were repaired; learning thresholds were unchanged.

Both release binaries were rebuilt from the Rust sources in `977d353`. The release smoke accepts
canonical source export and the completion CLI, creates the actual 30-run/$30 private pilot
plan, reports all uncollected usage as incomplete, and makes zero model calls. This environment's
remaining paid-dispatch blockers are the agent-session boundary, missing Claude Code, and
missing Claude authentication; the real Bubblewrap probe passes. Binary SHA-256 values:

- `adage`: `7986d8c449d87719f381f9d55c4032ecf87d3e1cc49d73c49fdbf2a8eb56038a`
- `adage-exec`: `a8e07b3cda295fda694e37d56eeffa8f41d66395fd98715767e9bb78781c60da`

The branch contains current `main` (`2534963`). At `977d353`, GitHub again refused to start CI
because account payments failed or its spending limit needs increasing
([annotation](https://github.com/stevenvaughanbooth/adage/actions/runs/37225761512/job/111504957092)).
No paid model evaluation or commercial savings measurement has been performed for this milestone.

### Release scale smoke measurements

Measured on 2026-10-03 against code `5186d81`, normal release profile, Rust 1.99.0,
Linux 6.18.44 x86_64, five logical CPUs. Release binary SHA-256:
`e9e1945edc017232fcaac46e566364f512640e35642d32047765759202e8a9f3`.
Tests and compilation had finished before measurement. Each row is one sequential observation,
not a repeated statistical benchmark; the filesystem and binary pages can be warm.

| Sessions | Steps | JSONL bytes | Import s | Duplicate import s | Cold learn s | Warm learn s | Forced full s |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1,000 | 3,000 | 1,546,000 | 0.0303 | 0.0255 | 0.1270 | 0.0842 | 0.0895 |
| 10,000 | 30,000 | 15,460,000 | 0.2595 | 0.2083 | 0.8127 | 0.6037 | 0.7031 |
| 100,000 | 300,000 | 154,600,000 | 2.6495 | 2.1526 | 8.2684 | 5.6685 | 7.4976 |

At 100,000 sessions, per-process peak RSS was 55.11 MiB for import, 78.12 MiB for duplicate
import, 1,039.60 MiB for cold learning, 881.03 MiB for warm learning and 1,040.82 MiB for forced
full learning. This demonstrates the current repository-sized mining memory cost; it is not
constant-memory learning. The small manifest still grows with the number of session ids.

All three sizes retained the same generation on duplicate import, produced the one expected
routine candidate, reported a mining cache hit only on the warm run, and produced byte-identical
track, explanation, follow-weight and compiled-index artifacts across cold, warm and forced-full
runs. The 100,000-session corpus generation was
`47c31b713434d57f2458b612df4e6a8cd47c3fe243da2dfc91fa4cc8b5c0fd1e`.

```sh
cargo build --release --locked -p adage-cli
python3 scripts/bench-corpus.py --adage target/release/adage \
  --work-dir /tmp/adage-corpus-bench-100000 --sessions 100000
```

The work directory must be empty. The script retains raw outputs and a machine-readable summary
there; only this curated summary is committed. The workload copies a three-step hand-authored
routine with distinct synthetic task ids in one repository and disables embeddings. It exercises
storage, graph learning, qualification and compilation; it does not represent long, diverse
enterprise traces, measure embedding/procedure-card scale, establish real task independence,
or demonstrate saved model tokens. Real corpus diversity, long sessions and live value remain
the next evaluation workload.
