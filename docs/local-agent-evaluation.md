# Local agent evaluation handoff

Use this runbook to test whether learned Adage reflexes improve coding outcomes and reduce
avoidable work. The first deliverable is a trustworthy live pilot, not a savings claim. The
supported frozen collector is **Claude Code on Linux with Bubblewrap, using E0 advice**. Model
access alone does not supply independent tasks, a grader, or proof that a feature is implemented.

## Instruction to the receiving agent

> Work from `codex/trace-learning-infrastructure` in stevenvaughanbooth/adage. Read this file,
> `evals/live/README.md`, and `docs/learning-experiments.md`. Record the exact commit and audit
> the host before executing. Complete the free checks and prepare a concrete, bounded pilot.
> Use existing spending authorization; otherwise have the operator approve its task list,
> model, job count, and total reservation before paid execution. Do not infer a spending
> allowance from this example plan. Preserve raw evidence and every failed/interrupted attempt.
> Do not change learning thresholds, invent task independence, author a reflex to impersonate
> learning, bypass human review, remove agent markers, or relax isolation to get a passing study.
> Report unsupported capabilities separately. Stop expansion when accounting, grading,
> confinement, routing, or provenance fails. Deliver the artifacts and decision described below.

This file and the diagnostic assets are engineering documentation in the repository. Raw
transcripts, credentials, customer task data, study objects and reports belong outside Git.

## Capability and measurement boundaries

| Selling point | Current support | What the first evaluation can establish |
| --- | --- | --- |
| Complete companion changes | Learned after-edit sequences; deterministic learning-to-hook proof | Whether a real model follows a learned sequence and leaves independently correct final artifacts |
| Correct repository checks | Scoped verification advice and some environment/failed-verification recovery | Correct command selection, fewer failed attempts, and final correctness; not arbitrary external tool recovery |
| Evidence before finishing or publishing | E0 completion reminders; some approved E2 checks and a bounded Stop continuation exist outside this E0 collector | Observe omissions and ordering; universal must-test enforcement and a PR gate bound to the exact head are **not implemented by this collector** |
| External API/CLI recovery | Generic authenticated lookup routing is not established; executor networking is disabled | Scenario specification and a future adapter/implementation gate; do not claim this from a local shell substitution |
| Real user journey | A task-specific external grader can check final behavior | Outcome if the necessary browser/service fixture is provided; no shipped universal browser grader |
| Procedures after compaction | Contextual advice exists; context recovery benefit is unmeasured | Needs controlled long-context/compaction tasks with equivalent accepted requirements in every arm |
| No-progress loops and waiting | Some learned verification fallbacks; bounded runner deadlines | Descriptive recovery/polling counts; eliminating model polling needs harness work |
| Preserve verification integrity | Independent graders and fixture tamper checks | Final quality despite misleading visible checks; universal prevention of test weakening is not shipped |

E0 supplies advice. E1/E2 add separately approved execution authority. Do not describe an E0
result as automatic enforcement or compare it against E2 without declaring that extra treatment.
An E0 obligation can be satisfied by a matching command even when that command fails; the
independent grade must establish the outcome. A learned sequence alone is not a completion gate.
The frozen collector currently fixes `off` and `adage` arms. It does not run a static-rules arm,
an E2 arm, or a Codex agent. macOS/Windows users need a Linux execution host whose namespace
probe passes; a container alone does not guarantee Bubblewrap support. Do not use `--no-isolate`
as a substitute. A local Claude agent may prepare artifacts, but the runner refuses paid child
agents when `CLAUDECODE` is set; the operator must execute from a normal terminal in that case.

## Stages and example reservations

These are proposed limits, not authorization or predicted invoices. All model calls, including
training and failed attempts, consume the agreed program budget. Each stage has its own root;
the current runner does not enforce one shared ledger across separate studies.

| Stage | Sessions | Example session cap | Maximum reservation | Decision |
| --- | ---: | ---: | ---: | --- |
| Free contracts and fixture self-checks | 0 | $0 | $0 | Must pass before any model call |
| Diagnostic authentication/runner smoke | 3 | $1 | $3 | Stop on unknown accounting, bad isolation or grader failure |
| Initial real training corpus | 32 | $1 | $32 | Retain all attempts; inspect whether production learning yields useful candidates |
| Validation pilot | 12 independent task pairs, 24 sessions | $1 | $24 | Diagnose applicability, uptake and quality; no commercial claim |
| Final same-model study | At least 200 independent task pairs, 400 sessions | $1 | $400 | Separately authorize and freeze after validation; no tuning on final results |

The first three paid stages reserve at most $59 in this example. A task that genuinely needs
more than the cap must be redesigned or assigned a larger **prospective** budget; never replace
an unfavorable capped result. The CLI cost estimate controls admission, not provider invoices.
Use the frozen report for usage completeness and keep actual billed dollars unknown without
billing evidence. An unknown-cost stop requires reconciliation of the original attempt.

## Free setup and diagnostic fixtures

From the checkout root, with stable Rust, system Python 3, Git and make installed:

```sh
git status --short
git rev-parse HEAD
cargo build --release --locked -p adage-cli
cargo build --locked -p adage-cli
cargo test --locked -p adage-cli --test learning_delivery -- --nocapture
cargo test --locked -p adage-eval --lib --test paired --test experiment
python3 -m unittest discover -s evals/live -p 'test_*.py'
python3 evals/live/sandboxes/cardinal/selfcheck.py
python3 evals/live/sandboxes/cardinal/generate.py --list
```

The cardinal fixtures exercise generated catalog consistency, Python source-layout verification,
and an unrelated control. Their independent grader reads final state and protects fixture checks;
it does **not** prove a model ran verification. Template siblings share semantic groups even when
their IDs contain `train`, `dev` or `test`. They are diagnostic variants, not independent customer
tasks and not a disjoint training/test corpus. Do not relabel them to qualify a frozen claim.

Choose a private, initially unused directory under a supported hidden root. Set the exact model
revision supported by the installed Claude CLI; do not rely on an evolving alias.

```sh
export STUDY_ROOT=/var/tmp/adage-local-study-001
export ADAGE_BIN="$(pwd)/target/release/adage"
export EVAL_MODEL='REPLACE_WITH_RESOLVED_MODEL_REVISION'
mkdir -p "$STUDY_ROOT"
claude --version
bwrap --die-with-parent --unshare-pid --ro-bind / / --proc /proc --dev /dev -- /bin/true
python3 evals/live/runner.py evals/live/configs/cardinal-diagnostic.json train \
  --root "$STUDY_ROOT/diagnostic" --adage "$ADAGE_BIN" --model "$EVAL_MODEL" --json
```

After the stage's spending authorization, repeat the last command with `--execute` instead of
`--json`. This runs three baseline diagnostics with no Adage learning. Confirm current provider
authentication, native usage counters, successful grader execution, and no private grader/study
visibility. Merely finding a stored login does not establish that it is valid. Preserve this
root even if authentication fails. Do not feed diagnostic runs to the independent training bridge.

## Build the real task and grading assets

Prepare `inputs/train.json`, `inputs/validation.json`, `inputs/final.json`, `task-registry.json`,
the checkout generator, and the independent grader under `$STUDY_ROOT`, not inside an agent
checkout. Start with one real repository and two supported workflow families. Each generator
must create a fresh Git checkout with a stable origin and an identical initial state for both
arms. Tasks should be realistic requests, not instructions to deliberately forget testing or
use a broken tool. Include negative controls where the reflex should abstain.

The registry is the independent grouping authority, decided before learning:

```json
{
  "schema": 1,
  "repo": "github.com/your-org/your-repo",
  "tasks": {
    "TRAIN-001": {"split": "train", "group_id": "issue-family-a"},
    "TRAIN-002": {"split": "train", "group_id": "issue-family-a"},
    "VAL-001": {"split": "validation", "group_id": "issue-family-b"},
    "FINAL-001": {"split": "test", "group_id": "issue-family-c"}
  }
}
```

This illustrates the format only. Populate enough actual groups for the chosen stage. Retries,
paraphrases, cherry-picked variants and changes derived from one original issue stay together.
A group cannot span splits. Sharing a workflow, such as regenerating a client, is acceptable;
sharing the underlying issue/solution template is not evidence of independent generalization.

Config requirements (use `cardinal-diagnostic.json` as a field-format reference, not its data):

- Set the exact `repo_key`, model revision, seed, `rounds: 1`, `concurrency: 1`, `confine: true`,
  `capture_usage: true`, `setting_sources: ""`, `learn_args: []`, session cap and workflow timeout.
- `sandbox` is an argument array supporting `{dir}`, `{task}` and optionally `{arm}`. `score`
  receives the checkout and task id and prints JSON with a genuine boolean `passed`.
- Training config: only registry `train` tasks under `train.tasks`, one repeat initially.
  Validation/final configs: the respective task list under `eval.tasks`, repeats 1, arms
  `["off", "adage"]`. Always supply the required `train`, `eval` and `prompts` fields.
- Keep the same tool set, prompt, requirements, project instructions and environment in both
  arms. Baseline agents should have the normal repository documentation. Do not handicap them.
- Total budget must reserve every scheduled job at the session cap; frozen collection requires
  `budget_usd >= 2 * task_count * session_budget_usd`.
- Pin runtime dependencies before executing. Declare the complete private grader/generator/data
  import closure as frozen `--input` files. Do not expose it through PATH or credential mounts.

For every task, demonstrate that the untouched state fails for the intended reason, a reference
solution passes, partial/wrong solutions fail, and weakening visible tests cannot fool the hidden
grader. Negative controls need their own correct baseline expectation. Never use the agent's
claim of success as the grade. Preserve baseline failures rather than quietly deleting them.

## Collect and normalize training evidence

Print the plan before executing; use **only the `train` phase**, not the runner's default
train/learn/eval/report sequence or its legacy outcome-assisted `condition --demo` path.

```sh
python3 evals/live/runner.py "$STUDY_ROOT/inputs/train.json" train \
  --root "$STUDY_ROOT/training" --adage "$ADAGE_BIN" --json
# After authorization, repeat with --execute instead of --json.

python3 evals/live/training.py --root "$STUDY_ROOT/training" \
  --registry "$STUDY_ROOT/task-registry.json" --repo github.com/your-org/your-repo \
  --adage "$ADAGE_BIN" --out "$STUDY_ROOT/training-bundle"

export ADAGE_HOME="$STUDY_ROOT/learner-home"
"$ADAGE_BIN" corpus import "$STUDY_ROOT/training-bundle/corpus.jsonl" \
  --repo github.com/your-org/your-repo > "$STUDY_ROOT/import.json"
"$ADAGE_BIN" corpus learn --repo github.com/your-org/your-repo > "$STUDY_ROOT/learn.json"
"$ADAGE_BIN" corpus inspect --repo github.com/your-org/your-repo --evidence \
  > "$STUDY_ROOT/training-evidence.json"
"$ADAGE_BIN" review --json > "$STUDY_ROOT/review-queue.json"
```

The bridge supplies declared task/group/attempt identities and independently recorded outcomes
through the existing native normalizer. It retains negative/unknown outcomes and accounts for
unusable attempts in its manifest. It must not silently select only successful sessions, merge
evaluation traces, or lose an earlier retry overwritten in mutable runner state. Read its
provenance/omission report before accepting the corpus. Preserve original roots and streams.

Use production learning thresholds. Record the binary digest, commit, parameters, embedding
mode, corpus generation and learned report. If no useful candidate qualifies, report that finding
and improve/expand independent training evidence; do not lower thresholds or insert a route.

Have a person inspect the evidence and approve specific candidates through the normal CLI:

```sh
"$ADAGE_BIN" review approve TRACK_NAME REFLEX_ID --by REVIEWER_NAME
cp "$ADAGE_HOME/index.json" "$STUDY_ROOT/frozen-index.json"
"$ADAGE_BIN" eval experiment artifact "$STUDY_ROOT/frozen-index.json" \
  --repo github.com/your-org/your-repo --checkout /path/to/representative/fresh/checkout
```

Learning and review update the compiled index; there is no separate `adage compile` command.
Do not remove agent markers or use the isolated unit-test review bypass in a real study.

## Freeze the validation pilot, then a separate final study

Create `validation-identities.json` from the registry, keyed by execution task id, with exactly
`{"task_id": "VAL-001", "group_id": "issue-family-b"}` for each validation task. Do the same for
final tasks. Pick one representative per group in a frozen cohort. Task id must equal its key.
Every frozen study must be disjoint from the training audit. The registry must also keep final
groups separate from validation; the Rust validator cannot infer undeclared validation exposure.

```sh
python3 evals/live/frozen.py prepare "$STUDY_ROOT/inputs/validation.json" \
  --adage "$ADAGE_BIN" --training-audit "$STUDY_ROOT/training-evidence.json" \
  --reflex-set "$STUDY_ROOT/frozen-index.json" \
  --learner-revision 'REPLACE_WITH_ACTUAL_TRAINING_BUILD_AND_PARAMETERS' \
  --identities "$STUDY_ROOT/validation-identities.json" \
  --policy evals/live/policies/pilot.json --timeout-seconds 600 \
  --input "$STUDY_ROOT/inputs/generate.py" --input "$STUDY_ROOT/inputs/grade.py" \
  --input "$STUDY_ROOT/inputs/tasks.json" \
  --objects "$STUDY_ROOT/validation/objects" --out "$STUDY_ROOT/validation/frozen.json"

python3 evals/live/frozen.py run "$STUDY_ROOT/validation/frozen.json" \
  --adage "$ADAGE_BIN" --objects "$STUDY_ROOT/validation/objects" \
  --root "$STUDY_ROOT/validation/run"
```

Replace the illustrative input filenames with the entire real dependency closure. Preparation
runs generators and version checks but no models. Inspect `ready_to_execute` and all `blockers`;
a read-only plan can exit zero while blocked. Record the frozen digest before running. Once ready
and authorized, repeat the `run` command with `--execute`. Re-running that exact command resumes
only unfinished eligible jobs; terminal failures remain terminal. Never delete receipts, markers
or the budget ledger to retry a job. Do not change pinned source files during collection.

```sh
python3 evals/live/frozen.py report "$STUDY_ROOT/validation/frozen.json" \
  --adage "$ADAGE_BIN" --objects "$STUDY_ROOT/validation/objects" \
  --root "$STUDY_ROOT/validation/run"
```

The pilot policy only lowers the structural minimum to two groups so small diagnostics can be
collected. It keeps the 2-point quality margin and 20% savings target. A 12-pair pilot is generally
too small to establish those bounds. Do not weaken the policy until it passes, or call a complete
collection a qualified result. Use validation to inspect delivery, model uptake, abstentions,
independent grades, failure causes and accounting completeness. Revisions require a new frozen
protocol and fresh validation tasks when earlier results informed the change.

After validation, freeze a **new** final study using `inputs/final.json`, final identities,
`evals/live/policies/claim.json`, and a separate final object/run directory. Publish its digest
to a timestamped record before execution and do not inspect/tune on final outcomes mid-study.
Report with `--require-token-target`. The sample policy requires at least 200 independent groups,
baseline success of at least 80%, a quality-loss bound within 2 percentage points, and a token
savings lower confidence bound of at least 20%. Two hundred is a planning floor, not a universal
power calculation or guarantee: even zero discordant pairs need roughly 183 tasks for the
existing quality bound. Estimate the required sample size from a separate pilot and the actual
predeclared effect/variance before final registration.

## Observe the missed actions without inventing evidence

Copy `evals/live/scenario-spec.example.json` outside Git and replace its checkout root, relevant
paths and exact command lists **before** running the study. Use the actual per-job checkout path
when instantiating this fixed recipe; retain each resulting spec digest. Audit the retained raw
stream (the runner's `{t,line}` wrapper is also supported):

```sh
python3 evals/live/scenarios.py /path/to/job/stream.jsonl \
  --spec /path/to/job/scenario-spec.json > /path/to/job/scenario-audit.json
```

Keep these observations alongside the independent grader result.
A command start is not completion. A non-error tool result is not proof that tests ran, passed,
covered the change, or executed against the final PR head. The sidecar explicitly leaves those
claims unknown. Shell writes, background processes and mutations outside observed edit tools
also limit freshness inference. Do not promote a sidecar observation into a hidden quality grade.
Likewise, do not interpret the exploratory runner's legacy `ci_passed` aggregate as proof that
the model ran checks: it can fall back to final task correctness. Use the independent raw grade
for outcomes and the sidecar or trusted host instrumentation for action evidence.

For stronger verification/PR claims, first implement trusted host-side evidence: actual process
exit, tests discovered, relevant input digest before and after the check, current checkout/HEAD,
and publication target. Bind the result to immutable source state and invalidate it after edits.
An agent-writable `.tests-passed` file or matching command text is not acceptable evidence.
Use a local fake PR service for initial tests; do not create real public PRs as a grading side effect.

## Scenario acceptance tests and implementation queue

Use this matrix to build the remaining task fixtures. Rows marked instrumentation or product
work are not ready to advertise. The local agent should complete their free adversarial checks
before spending on their live efficacy comparison. The same matrix applies to baseline and
intervention; a treatment must not change the grader's definition of success.

| Scenario | Required independent evidence | Negative or adversarial controls | Readiness |
| --- | --- | --- | --- |
| S1 Finish without verification | Trusted check completion on the final relevant inputs, plus full-task correctness | No check; failed check; start-only/background check; check before final edit; unrelated suite; zero tests discovered | Sidecar ordering now; trusted process/input instrumentation and a universal gate still needed |
| S2 Publish an untested PR | S1 evidence bound to the actual publication head and worktree state | Edit after tests; publish a different head; simultaneous edit/publication; publish only after failure; stale remote checks | Sidecar timing now; fake PR endpoint, trusted revision journal and publication gate still needed |
| S3 Repeated external lookup failure | Correct requested record returned through an authorized operation, with all preceding attempts retained | Available tool changes; wrong repository/id; successful response with wrong data; exhausted credentials; access-policy denial with no allowed fallback | Controlled service/CLI fixture and general capability-aware recovery still needed; do not launch real external writes |
| S4 Wrong verification environment | Independent correct final code; host evidence of selected command and fewer environment failures | Wrong cwd/interpreter; watch mode; missing dependency; same command valid in another environment | Cardinal layout fixture is a narrow diagnostic; use real repository variants for efficacy |
| S5 Missed companion changes | Source and generated/lock/migration artifacts independently consistent | Source-only fix; generated-only fix; stale artifact; changed visible check; unrelated edit | Cardinal manifest fixture and learned-sequence proof ready for diagnostics |
| S6 Superficially passing feature | Relevant user journey succeeds against the evaluated revision | Unit-only pass with broken UI; wrong deployment; missing persistence; auth/role mismatch | Private browser/service grader and pinned environment still needed |
| S7 Lost obligation after context transition | Same acceptance criteria retained across a controlled long session, with final state/action evidence | Requirement at beginning/middle/end; compaction versus none; changed requirement; already-satisfied obligation | Controlled context/handoff fixture still needed; do not attribute every missed instruction to compaction |
| S8 Repeated failures or empty polling | Correct final result with complete call/wait history and fewer avoidable turns | Slow healthy process; transient error; genuine liveness failure; repeated deterministic error | Sidecar descriptive evidence/deadlines now; no-progress and event-driven waiting implementation still needed |
| S9 Weakened tests | Independent hidden outcome plus integrity of required assertions/checks | Skipped test; altered expectation; fake success output; ignored exit; unrelated pre-existing failure | Cardinal protects its own fixture checks; general repository integrity policy still needed |

Prioritize S4/S5 for the first live learned-reflex pilot. Implement trusted S1/S2 evidence next
before selling verified handoff. Build S3 against a deterministic local service before wiring
real credentials. Keep network authorization errors distinct from a broken invocation: a learned
fallback is not permission to bypass an access restriction. Do not broaden the first paid study
to all nine scenarios merely to populate a marketing checklist.

## Controls, metrics and claim decisions

The primary paired comparison uses the same pinned model and settings, Adage off versus the
frozen learned reflex set. Include tasks with applicable reflexes and tasks on which they should
abstain; do not condition the headline on successful delivery or baseline failure after the fact.

Keep three distinct result classes:

1. **Quality:** independent full-task pass rate, regressions, stale artifacts, wrong external
   state, and verified handoff coverage when trusted instrumentation exists.
2. **Mechanism:** applicable opportunities, advice delivered, followed/ignored advice, failed
   tool attempts, no-progress loops, observed check ordering, and unnecessary interventions.
3. **Economics:** complete workflow input/output/cache tokens, model calls, wall time and actual
   billed cost when available. Include failures, retries and hook overhead. Report offline
   learning/review cost separately and its amortization; session savings alone are not net ROI.

Retain the denominator of all predeclared tasks. Report subgroup results as exploratory unless
their tests and multiplicity handling were predeclared. Synthetic fixtures establish mechanism;
customer claims need representative independent work across the target environments.

To establish value beyond existing tools, add a **well-configured static rules/test-hook baseline**
using equivalent training knowledge and the same permissions. The exploratory runner supports
an `agents-md` arm, but the frozen two-arm collector does not qualify that comparison today.
Implement and validate a frozen matched multi-arm protocol before making superiority claims.
Do not compare a richly configured Adage arm against a deliberately underconfigured baseline.

For cheaper-model claims, predeclare strong-model/off, cheap-model/off and cheap-model/Adage
comparisons. `--intervention-model` supports a frozen strong/off versus cheap/Adage comparison,
but that alone cannot attribute the difference to Adage or establish the cheap-model uplift.
Use a separately frozen cheap/off versus cheap/Adage comparison with matched tasks/settings,
account for both protocols' full spend, and specify joint analysis before seeing results.

## Required output from the local agent

Return one report linking the pinned commit, binary/harness/model revisions, task registry,
training provenance, learning report, reviewed reflexes, frozen protocol digests and all report
objects. Keep raw/customer evidence private; share a redacted summary as appropriate.

The summary must contain:

- Readiness and capability matrix: tested, unsupported, blocked, or unmeasured.
- Scheduled/completed/missing jobs, independent groups, training/validation/test separation,
  unknown accounting and all budget reservations/charges across study roots.
- Per-arm quality and token totals with the evaluator's confidence bounds and rejection reasons.
- Applicable-reflex coverage, delivered advice, uptake, abstention controls, and concrete traces
  showing an avoided failed attempt or missed action caught. Include counterexamples.
- Total training/review cost and any extra checks caused by Adage, not only apparent savings.
- One decision: fix instrumentation, collect more independent evidence, improve an ineffective
  reflex, proceed to a separately approved final study, or reject the current value hypothesis.

Do not report 20–30% savings, universal test enforcement, exact-head PR verification, arbitrary
external-tool recovery, or cheaper-model reliability unless the corresponding evidence supports it.

## Evidence behind the scenarios

The [MAST study](https://arxiv.org/html/2503.13657v3) identifies premature termination, missing
verification and repeated steps; [SWE-agent](https://arxiv.org/html/2405.15793v3) evaluates workflow
interface changes. [Lost in the Middle](https://arxiv.org/abs/2307.03172) concerns context retrieval,
not measured coding-agent omission rates. [Evaluating AGENTS.md](https://arxiv.org/html/2602.11988v3)
warns that extra instructions can increase cost without a significant task-success gain.
Developer reports include [untested completion](https://github.com/anthropics/claude-code/issues/60177),
[tool capability mismatch](https://github.com/openai/codex/issues/37912), and
[watch-mode test execution](https://github.com/openai/codex/issues/3775). These motivate fixtures;
they do not supply Adage's effect size or prove current-market failure prevalence.
