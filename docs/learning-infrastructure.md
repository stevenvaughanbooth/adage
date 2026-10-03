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
| M3 Scalable learning | Reusable repository learning checkpoints and incremental computation with explicit invalidation | Repeated runs avoid unchanged work; changed/deleted evidence and parameter changes invalidate correctly; full/incremental outputs agree | Pending |
| M4 Outcomes and value | Conservative completion evidence and reproducible paired token/quality evaluation | Stale checks do not label final work successful; quality regressions reject savings claims; unknown usage and contaminated comparisons are visible | Pending |
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
- 2026-10-03: Baseline exposed an existing deleted-directory discovery bug and a new stable-Rust
  lint; fixed both without weakening tests. The managed sandbox requires network permission for
  tests that bind a local socket. The full suite is being rerun with that capability.
- 2026-10-03: M1 implemented. Task identity survives the canonical bridge, evaluation manifests,
  parse-cache reads and route qualification. Calibration refits prototypes and candidates on
  independent task representatives. Identical canonical deliveries are deduplicated; conflicting
  repository/session ids reject the run before evidence writes. Scoped canonical learning filters
  other repositories itself. Fixed deleted-directory attribution at ingestion as well.

## Deterministic fixture strategy

### Canonical corpora

```sh
adage corpus export trace-a.jsonl trace-b.jsonl --repo acme/api --identities identities.json > corpus.jsonl
adage corpus import corpus.jsonl --repo acme/api
adage corpus inspect --repo acme/api
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
