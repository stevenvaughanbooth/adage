# Reflex execution: evidence before expansion

Status: evidence and implementation gates, 2026-09-29, updated against main `acdb0a4acd875be56821469aaeadeb29b8d6d493`. Execution exists; broad incremental task value remains the question.

## Objective

Show that a procedure learned from agent traces can execute at a harness boundary without an intervening model request, preserve task correctness, and reduce total task cost or latency. Separate four claims: execution works; learning generalizes; automatic execution helps; shared reflexes transfer. No result in one category substitutes for another.

## Implementation checkpoint

This plan includes future experiments. The authoritative current behavior is
[How Adage works](how-it-works.md); test outcomes and limitations live in the
[proof ledger](../evals/reflex-proof/README.md).

| Gate | Current evidence | Still required |
| --- | --- | --- |
| Harness boundary | Real fixture actions precede the next observed model request | Production automatic-versus-suggest pair with complete results |
| Learning inputs | Checksummed source audit; default onboarding learns and delivers on archived clinic history | Held-out executable prediction/binding accuracy across real repositories |
| Execution reliability | Canonical durable journal, recovery, real-hook duplicate tests and 500 staged safety runs | Linux platform checks and failure testing beyond the covered fault model |
| Developer experience | Effective authority/outcome states, narrow/wide snapshots and source-discovery checks | First-user walkthrough; execution approval/recovery remain CLI controls |
| Packaging | Vendored-script and accepted-skill-pack integration tests pass | Benefit and compatibility in a genuinely different recipient environment |
| Net value | Budget accounting tested; prospective screen preregistered | Authenticated confined Linux runner, missing real-repository training inputs, then paid comparisons |

## Worktree and integration boundary

Work in the isolated `codex/reflex-proof` worktree at `/Users/stevenbooth/.codex/worktrees/adage-reflex-proof/skill-compile`. It was fast-forwarded from `d3f8905` to the newly published main `acdb0a4`. The primary checkout has not been edited by this experiment.

The earlier five-miner assessment and intermediate branch `f8e7d29` do not describe current main. Current main already has a generic graph learner, local CPU embeddings, shared hook pipeline, E1 read prefetch, E2 staged execution, exact local approvals, signed packs with vendored scripts, skill compilation, and measured savings. Preserve these components. Separate `adage` and `adage-exec` entry points share the hook pipeline; that is not a reason to merge or replace them.

Do not port the intermediate branch's storage architecture by assumption. Advice evidence, approval authority, completed execution and measured utility are distinct facts. Main applied E2 changes before best-effort telemetry append. This work replaces that write path with durable preparation/completion in the existing actions spool and explicit unknown-outcome recovery; it does not introduce another store. See the [current behavior reference](how-it-works.md) and [evidence ledger](../evals/reflex-proof/README.md) for implementation status.

## Evidence already available

- **New live boundary probe:** installed Claude Code 2.1.282, Haiku 4.5, $0.0152216 billed. Prompt and completed-batch hooks ran local fixture actions and supplied receipts before the next debug-recorded API dispatch. This proves the synchronous substrate, not learned production-task savings. See `evals/reflex-proof/harness/results/20260929-claude-2.1.282`.
- **New source audit:** 540 traces / 10,846 calls: 444 live sandbox traces, 36 synthetic traces and 60 supported sampled public traces. Curated live traces retain arguments/results/cwd/model/timestamps, but lack numeric exit codes, model request IDs and original record IDs; 218 outputs are explicitly trimmed. One additional 12-trace public format is unsupported. These are source-presence counts, not usable-reflex counts. See `evals/reflex-proof/traces/results/source-audit.json`.
- **New actual CPU probe on acdb0a4:** installed verified Potion base 8m, 100 measurements; warm-cache lazy open+normalize+embed p95 2.506 ms; loaded normalize+embed p95 0.0187 ms. A separate build was active, so these are diagnostic timings, not an isolated benchmark. Reversed procedural order gives exactly equal vector components. This is one configured static model and authored diagnostic examples, not a comparison of every supported embedding backend or held-out accuracy.
- **Existing repository evidence:** launch E1 delivered data but all three agents repeated the lookup; two E2 generated-file runs applied changes and avoided agent regeneration. These historical samples motivate the next experiment. Replay on nine archived clinic training sessions, with success labels supplied using `--outcomes`, produced two intent cards and one lookup route, but no regeneration route, even with `--demo`. This is outcome-assisted learning, not default onboarding. Do not manually insert the missing route and call that learned execution.
- **Receipt-loss defect and repair:** the original `judge → finish/apply → record` characterization showed applied changes surviving with no receipt. The alternate writer and characterization were replaced by canonical journal regression tests: refuse before apply when durable preparation fails; report Unknown when completion is lost; preserve the recovery block across fresh reads; require explicit acknowledgement. Fault injection is not power-loss validation.
- **Production startup defect fixed on this Mac:** sandboxed replay originally aborted `/bin/sh` with signal 6 before output. A one-clause SBPL fix grants read access to the root directory node only, never its descendants. Replay now passes, including explicit successful script startup, denied external file reads/writes and a loopback connection blocked only inside the sandbox. The older escape suite enumerates Linux backends only; its green count is still not macOS containment evidence. The full coding-agent evaluation runner has separate Linux confinement/auth limitations.
- **Process-budget repair:** the full executor run exposed legitimate staged regeneration failing with exit 128, `fork: Resource temporarily unavailable`. macOS now adds existing same-user processes to its bounded allowance and checks limit installation; the focused legitimate-generation control passed. Integrated and containment results belong in the evidence ledger. Resource limits remain enforced.
- **Existing broader evaluations:** recurring procedure-card prototype savings and compiled-skill sandbox savings do not isolate automatic execution. The shipped-card fresh-family validation is preregistered, not completed. Do not relabel prototype/advice evidence as proof of the CPU execution thesis.

## What gets built first

1. Reusable hook-boundary probes with offline selftests and an explicit paid-run manifest.
2. A reproducible trace sufficiency audit and opportunity analysis. It never executes commands contained in traces.
3. A frozen experimental protocol and cost budget.

These are evaluation infrastructure, not a second product runtime. The eventual production executor and learner will be tested through the same fixtures. A full storage rewrite, schema rename, new predictor, and TUI redesign are not prerequisites for discovering whether the proposal is valuable.

## Six stages and decision rules

### 1. Establish the real execution boundary

Hypothesis: a synchronous hook can complete an approved procedure before the harness's next model request.

Use an isolated temporary repository and session-only hook settings. Probe the installed Claude version, record its exact version and flags, and test both a pre-prompt lookup and a completed-batch action. Do not install global hooks or invoke commands from untrusted traces. Capture native event and tool IDs, process and monotonic timestamps, script start/end, result publication, cancellation, and the model-request observations actually exposed by the harness.

Pass conditions:

- The expected fixture effect exists before the next model request starts.
- There is no model request between the trigger and the completed procedure.
- The next already-needed request sees the receipt/result.
- A missing/revoked grant launches nothing; duplicate deliveries have one claimed execution.
- Timeout/cancellation and a crash around receipt persistence produce an honest outcome.

Offline fixture tests establish probe correctness only. They do not establish real harness timing. Assistant-message IDs are not automatically API-request IDs; if the real request boundary cannot be measured, report that claim as unverified and improve instrumentation before claiming zero inference.

If this gate fails, change the adapter or choose a different verified boundary. Do not start the learner rewrite to compensate for an execution-substrate failure.

### 2. Verify and harden the existing production execution path

Hypothesis: current `adage-hook` + `adage-exec` can remove one real agent action without compromising correctness or replay safety.

Reuse `request.rs`, `route.rs`, `approvals.rs`, `hook.rs`, `stage.rs` and the OS sandbox. Freeze one learned lookup and one learned generated-file action; compare the same match/binding in advisory and automatic modes. Verify the delivered typed result is sufficient for the next task step: a successful prefetch that makes the agent fetch again is a failed utility gate. Only extend to two dependent steps after a single step demonstrates benefit. No new model call is part of planning or execution.

Preserve exact executable/argv/cwd/stdin semantics. Explicitly classify host execution versus an actually enforced sandbox. A script's effect label is not confinement. Bind approval to executable behavior and local scope. Recheck authorization before dispatch; a changed executable definition cannot inherit its predecessor's grant.

Before expanding automatic side effects, add a durable attempt reservation to the existing execution path, keyed by repository/session/source event/executable identity/bound inputs. A reservation failure must prevent dispatch. After a crash, an effect with an absent receipt is unknown, not safe to replay. Reconcile run state with E2's existing staged hashes and partial-apply reporting. Replace best-effort execution accounting with one authoritative journal rather than mirroring it. Keep learning storage and the existing planner/executor intact. Read-only execution can be tested while this durability gate remains pending.

Tests: duplicate and concurrent events; grant revocation and executable changes; quoted/leading-dash/Unicode arguments; wrong output with exit zero; disk-full before dispatch; crash after effect; changing inputs; timeout and descendants. No automatic retry of an uncertain non-idempotent action.

Pass condition: a real harness executes both a read-oriented procedure and a bounded repository procedure through that one path. Failure means fix the execution semantics, not add workflow categories.

### 3. Establish learnability and the value of embeddings

Hypothesis: available traces contain enough pre-action information to recover useful programs and inputs, and contextual features improve precision or coverage.

First audit source coverage. Record whether each source actually supplies call/result IDs, request/turn IDs, timestamps, parent/subagent relations, task IDs, all user prompts, original arguments, per-call cwd, relevant environment references, structured results, exit/cancellation status, repository revision, and intervention origin. Unknown remains unknown. Timestamp order is not proof that concurrent actions depend on one another.

Extend existing import/capture types, not a parallel event pipeline. The minimum retained envelope is: source schema/version and content digest; native event/call/result/parent IDs when supplied; session/task/repository/revision; harness/model/request identifiers; event kind; observed wall time and same-host monotonic time with clock domain; raw structured arguments and results or content-addressed references; explicit exit/signal/timeout/partial-result status; cwd and the origin of that value; environment/toolchain **fingerprints and allowed variable names**, not secret values; intervention provenance (`agent`, `reflex`, `human`, `unknown`). A missing field has an availability reason. Preserve all prompts with their causal positions, including later corrections, but never make those corrections visible to earlier predictions.

An outcome such as “agent eventually succeeded” is a label attached after the decision; it cannot enter trigger features. An asynchronous tool response is not process completion. A duplicate native call ID with conflicting results is ambiguous. Keep original record order and explicit causal links instead of sorting everything into a fabricated total order. Feed the same audit manifest through the existing graph bridge to measure fields lost during projection.

Represent every observation as immutable source facts plus a derived feature view. Store vector references with model ID/digest, dimension, tokenizer/normalization version, source event/window digest, and causal cutoff. Embeddings of a context window must not include the future command, its result, final task outcome, or later user corrections.

Current `adage-intent` supports multiple configured local models, bundled quantized weights and a separately identified hashed fallback. The measured installed Potion base model is a pinned static 256-dimensional token lookup model. Reuse verified loading and CPU embedding code, preserve the actual model/representation ID, and never mix incompatible vectors. None of these vectors automatically represents execution dependencies. Keep exact argument/dataflow and repository-state features alongside vectors. A fallback must be explicitly labeled and calibrated independently.

Freeze four ablations over the SAME opportunities and held-out groups:

| Variant | Features | Question |
| --- | --- | --- |
| A | Event and exact command/argument structure | What is available without sequence or semantic context? |
| B | A + prior event sequence + typed dataflow + repository facts | Does causal/structural history provide the gain? |
| C | B + existing CPU prompt embedding | Do paraphrases and task intent add coverage at fixed precision? |
| D | B + CPU embedding of a bounded prior context window | Does broader context improve over the prompt alone? |

Do not select a semantic model based on final-test performance. Select thresholds and feature choice on calibration data; report the untouched test once. Measure precision/coverage curves, binding errors, abstentions, match latency, model-load time, memory, artifact size, and CPU learning time. If C/D add no useful coverage at the chosen error rate, ship B and retain vectors as optional metadata rather than forcing an embedding dependency.

Learning initially generalizes recurring invocation structure and source-to-argument bindings. It does not infer arbitrary program intent from repetition. Require unfamiliar command names, multiple distinct parameter values, hard negatives, and held-out task instances. No per-workflow detector may be added to make the benchmark pass.

The first audit illustrates why: 474 of 683 simple-shell held-out calls matched repeated training shapes, but only 75 of 289 variable operands had a unique exact occurrence in earlier context; 10 were ambiguous and 204 had no prior exact literal. This narrow, descriptive screen is not a production learner score. It makes binding provenance a first-class evaluation, instead of equating repeated command shape with executable readiness.

Report the funnel: all decision boundaries -> observable/supported boundaries -> trigger matches -> unambiguous bindings -> valid execution plans -> locally approved activations -> verified results -> measured task benefit. Every percentage names its denominator. A small high-precision subset does not establish broad utility.

If exact bindings or evidence provenance are missing, improve capture first. If coverage is low despite sufficient capture, refine the candidate representation. Do not lower the execution bar merely to produce more matches.

### 4. Validate the developer experience

Hypothesis: a developer can understand, approve, supervise, and stop automation without reading trace internals or trusting a vague confidence color.

Reuse the current TUI layout and component system, and incorporate any subsequent UX update before editing it. Existing route views show evidence, strength, effect, state and follow metrics; delegation/revocation currently live in the CLI. The narrow change is to join that read model with exact grant status and the last authoritative execution receipt. Add candidate, approved, running, succeeded, failed, stale, disabled and unknown states. Keep the executable decision separate from evidence strength. Do not reuse evidence approval as permission to run scripts.

Task-based checks, using the same trace/receipt fixtures:

1. Find the first useful candidate and explain its trigger, inputs, commands, and limits.
2. Enable automatic mode for the intended repository and identify what authority was granted.
3. Locate a run, see why it fired, and distinguish observed results from estimated savings.
4. Notice a script update requiring renewed approval.
5. Disable/revoke the reflex and verify future dispatch stops.
6. Diagnose a failed or unknown run and return control to the agent.

Show one clear action at each stage, progressive detail, bounded output, keyboard navigation, and explicit non-color status labels. Capture small/large terminal snapshots and a real terminal walkthrough after the incoming UX is integrated. A snapshot test verifies layout; a person completing these tasks verifies usability. Until that walkthrough occurs, label usability unvalidated.

Proposed usability target: a new user can correctly explain and enable the first candidate in under two minutes, without accidentally granting more scope; all participants can revoke it. This is a design target, not a measured finding. Do not display hypothetical token savings as measured benefit.

The Reflexes detail panel should lead with a plain-language name and effective mode: **Suggest**, **Automatic**, or **Disabled**. Beneath it, show “When”, “Runs”, “May change”, “Last result”, and “Measured benefit”. Put evidence counts and exact argv/script hashes behind details. Automatic mode opens the existing approval preview, showing repository scope, output paths, network limits, timeout and executable changes. Approval creates the existing local grant; it does not rewrite the learned program. A changed script shows “Needs approval” while preserving the user's requested mode. Running and unknown outcomes never look like success. The live feed links to the same receipt detail rather than introducing another execution screen.

Example intended summary (illustrative, not measured): “Refresh generated client · Automatic / When API schema changes / Runs repository generator / May change generated/client/** / Last result: 3 files updated, verification passed / Measured benefit: not enough paired sessions yet.” The default view should answer what happened and what the person controls, without XML directives or statistical terminology taking the primary visual space.

### 5. Prove packaging and local transfer

Hypothesis: a signed package transfers executable behavior without transferring local authority or private history.

Export the learned definition, exact scripts/assets, typed contracts, runtime requirements, verification fixtures, and redacted aggregate evidence. Keep installed signed bytes immutable. Keep local environment bindings, modes, grants, credentials, and raw receipts outside the package. Evidence-only revisions may retain execution identity; executable changes cannot.

Test on an independent clean data directory and checkout: verify signature -> install without executing -> resolve local requirements -> observe -> approve locally -> execute held-out parameter values. Include tampered assets, missing tools, changed script dependencies, wrong repo scope, unknown signer, sticky disabled state, and differing local conventions.

Compare recipient cold start using a pack versus learning from scratch. Report recipient approval effort, compatibility failures, time to first useful reflex, and held-out task outcomes. Imported source evidence does not establish recipient benefit. An identical sandbox clone establishes transport only; a second genuine repository or environment establishes a separate transfer claim.

### 6. Demonstrate net task value

Hypothesis: automatic learned reflexes reduce total billed model work or elapsed time while preserving task outcomes, including on a cheaper student model.

Use isolated copies of each task, ordinary documentation intact, frozen packages/configuration, exact model/harness versions, and blocked randomized arm ordering by repository/task/model. Keep failed, timed-out, and abstaining runs in the intention-to-treat analysis. No post hoc exclusion of tasks where Adage did not fire.

| Arm | Purpose |
| --- | --- |
| Normal setup | Practical baseline |
| Equivalent static instructions/skills | Separate repository knowledge from runtime timing |
| Handwritten automation using the same executor | Separate learning value from simply automating a script |
| Learned reflex in suggest mode | Inference-mediated version of the same learned plan |
| Same learned reflex in automatic mode | Effect of bypassing model action selection |

Run the relevant arms on both teacher and student models. Teacher traces may train the frozen package; held-out tasks, repeats, forks, and teacher/student variants of one task must remain in the same split. Separate known-workflow/new-instance, repository drift, new-repository transfer, and online adaptation experiments. Current round-two demo adaptation must not be relabeled unseen-task generalization.

Primary metrics: external task score and total billed model cost per assigned task. Secondary: actual model requests, input/output/cache tokens, time to verified completion, execution/learning CPU time, script costs, added result-context tokens, unnecessary actions, binding errors, and recovery rate. Record compilation and teacher costs, and show amortization versus number of future tasks. Do not infer energy savings from request counts.

For paired task comparisons report absolute and relative differences, task-cluster confidence intervals, per-repository results, and the fraction of all tasks helped. Repeats are not independent new tasks. A pilot estimates variance; it does not establish quality noninferiority. Predeclare the quality margin and power calculation before the confirmatory run. A candidate two-percentage-point margin requires user acceptance and a sufficiently powered sample; 18 tasks per arm cannot establish it by default.

Proposed pilot target: >=15% lower total billed cost for automatic versus equivalent suggestions, with no major regression observed, alongside the actual confidence interval. This is a decision target, not promised savings. Compare automatic against static guidance and handwritten automation separately. If only weak baselines improve, narrow the claim. If output ingestion or unnecessary executions erase savings, reduce the program/output scope or stop expansion.

## Budget and evidence status

The user authorized a **$60 total cap** for the initial paid evaluation. Reserve at most $3 for initial live harness-boundary probes (at most three sequential runs with a $1 per-run harness budget). The remaining $57 is reserved for paired value runs after the execution gate passes; it is not permission to silently exceed the total if a harness budget is soft. Generate the complete run matrix and cost reservations first. Enforce per-run limits and reserve worst-case concurrent commitments before dispatch; do not rely only on completed-cost totals. Record budget-stopped runs and stop if accounting is unavailable. The initial pilot is a screening experiment, not the confirmatory trial.

Live barrier timing now has direct evidence and current main already implements production execution. Incremental automatic-versus-advisory savings, embedding ablation gains, developer usability and cross-repository transfer remain separate unproven claims. Paid spend is $0.0152216. The additional $15 pilot reservation was unused and released: no further paid run is outstanding, leaving $59.9847784. The offline candidate does not yet justify a live automatic run: its result contract omits task information, the desired regeneration route was not learned, and the existing confined live runner requires a supported platform/auth setup. Prepare and approve a fresh bounded run matrix after those gates pass. Every result must include command/configuration, code revision, source manifest hashes, and its limits. Offline reports must identify themselves as offline.

## Stop/change decisions

- No reliable hook barrier: fix transport before modifying learning.
- Insufficient trace fields: fix capture before selecting a more complex predictor.
- No embedding lift: retain exact/contextual features and avoid compulsory embeddings.
- No useful coverage: improve representation or narrow the supported workflow; do not claim generality.
- No advantage over equivalent suggestions: do not expand automatic execution on efficiency grounds.
- Handwritten automation performs equally but takes little effort: the learning product must prove reduced setup/maintenance effort.
- Script authority or crash recovery cannot be represented honestly: keep that procedure nonautomatic.
- Existing unpublished infrastructure solves a requirement: integrate and test it rather than rebuild it.
