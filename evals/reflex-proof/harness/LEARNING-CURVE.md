# Cross-session learning-curve pilot

Status: the native runner, chronological checkpoints and four-arm collector are implemented.
The first 15-session pilot completed: **zero executable candidates** at every checkpoint.
Independent post-training grading passed 12/15 tasks; training cost $3.7549221. All sessions,
including failures, were retained. Per the registered stop rule, no automatic held-out arm ran.
See the [complete record](results/20260930-learning-curve-native/report.json).
A separate development replay with narrow pytest verbosity equivalence also produced zero
candidates. It does not replace the original checkpoints or constitute held-out efficacy.

## Question and causal comparisons

Does experience on related tasks let Adage predict useful work in a new session, execute an
approved action on the CPU, and reduce agent work while preserving completed-task quality?

Use Marshmallow at `7f0792bd7a06f72e393ec866ac1e89e1502ccfe1`. These are authored feature requests
against real upstream code, not historical bug fixes. The unit is a whole independent agent session.
Each starts from the same clean commit and dependencies; only Adage's assigned knowledge persists.

| Arm | Shipped instincts | Learned knowledge | Approved CPU execution |
|---|---|---|---|
| agent | No | No | No |
| instincts | Yes | No | No |
| learned | Yes | Checkpoint 15 | No |
| automatic | Yes | Identical checkpoint 15 | Yes |

`instincts − agent` estimates cold-start benefit. `learned − instincts` estimates learned guidance.
`automatic − learned` isolates execution. Execution-only candidates stay silent in `learned`; do not
manufacture a suggestion fallback. Also publish the total `automatic − agent` comparison.

## Six stages

1. **Freeze tasks and controls.** `curve_tasks.py` defines 15 different training requests and four new
   held-out requests. Freeze prompt, hidden-test and source hashes before dispatch. Task definition
   reuse from prior studies is disclosed; their transcripts never seed this study. For each task,
   pristine base must fail its hidden checks and gold must pass the full suite. Preserve all attempts.
   Gold is only a scorer control and never a learning trace. Private checks/gold stay outside agent
   sandboxes. Exact hidden test identities must survive collection in agent result scoring.
2. **Collect natural history.** Run the 15 training tasks in registered order, without Adage hooks,
   one fixed model/configuration, independent fresh sessions and working trees. Do not prescribe a
   test command, tool sequence or desired reflex. Retain failures, timeouts and bills. Never replace
   an unsuccessful task with an extra attempt. Duplicate transcripts do not increase support.
3. **Measure learning.** At 0, 3, 5, 10 and 15 sessions, condition a fresh isolated home from precisely
   that chronological prefix using production defaults. Checkpoint 0 is shipped instincts only.
   Record candidate count, support sessions, distinct tasks, context/embedding evidence, confidence,
   rejected evidence and reasons, conditioning CPU time, memory and learned artifact hashes.
   Preserve failed actions in input; the production learner decides their evidential weight.
   Compare routes across checkpoints without selecting a favorable checkpoint using held-out scores.
4. **Check prediction and approve.** Use training prefixes for causal shadow replay: an event's
   prediction may depend only on earlier events. Label future successful actions separately, never
   feed them into the trigger. Use same-repository documentation-only, read-only, unfinished-edit,
   unrelated-test and failed-command contexts as negative controls, plus stale and absent grants.
   Report false executions, abstentions and coverage. Execution eligibility is tied to the actual
   action's relevance, not merely whether the overall task resembles training. Use checkpoint 15;
   if it has no eligible learned action, stop and report that result. A human approves the bounded
   learned command after reviewing its evidence. Never create grants from an agent session.
5. **Run four held-out tasks in four arms.** Use a prospectively fixed Latin-square arm order so each
   arm occupies each order position once. Keep prompts, model, caps, base and installed dependencies
   identical. Clone knowledge but never state/journals or grants between sessions. Freeze learning
   during evaluation; test sessions must not train later test sessions. All 16 assignments remain
   in the denominator, including censored and failed runs. No tuning against their outcomes.
6. **Publish a complete scorecard.** Report per-task/arm outcomes, source/binary/model versions,
   action receipts, costs and the entire learning curve. Publish unsuccessful attempts and all
   exclusions alongside successes. A passing pilot justifies a separately frozen confirmation on
   more tasks and repositories; it does not establish a population-wide launch-site percentage.

## What counts as savings

Quality is independently scored hidden behavior, requested tests/docs/changelog and the whole
upstream suite. Report treatment-only failures explicitly. Do not average a quality loss away with
cost savings or report costs only among successful runs.

Count a replaced action only with (1) a semantically equivalent agent action in the paired learned
arm, (2) a successful CPU receipt in automatic, (3) adequate test scope and matching source generation,
and (4) no corresponding agent action repeated afterwards. Match runner/environment, targets,
selectors and effect using production action semantics. Different argv alone is not a different
operation. Unknown shell behavior remains unknown, not evidence of absence. Report conservative
confirmed eliminations separately from ambiguous cases and all CPU executions, including duplicates.

Record observed model requests separately from tool calls: eliminating a tool does not necessarily
eliminate a model request. Include input/output/cache tokens, billed USD, wall time, hook/CPU execution
time, result tokens, failures and extra work. Embedding generation belongs in the cost account; CPU
prediction does not make a remotely generated embedding free. Do not claim measured GPU utilization
or energy savings from API-token counts. Report learning cost and observed operating savings
separately; estimate amortization only when savings are positive, with the assumptions shown.

Screen passes only if a naturally learned action is confirmed replaced in at least three of four
held-out tasks, no automatic-only quality failure occurs against learned, all assignments are
accounted for, and no execution escapes its approval. Also report cold-start quality separately.
Any missing/ambiguous evidence is not a pass. Four tasks are a feasibility screen, not a powered
noninferiority test. Confidence claims and sample size for confirmation must be frozen after the
pilot estimates variability, before collecting confirmation results.

## Cost and operational gates

15 training + 16 held-out sessions at $1.50 maximum = $46.50 reserved. Reserve a further $0.50 for a
new native confinement probe after runner changes. Prior CPU-eval charges were $4.860571, so the
maximum total for this experiment would be $51.860571. The user subsequently approved paid work;
the explicit allocation uses the existing $60 CPU-evaluation ledger. The separate $47.64 showcase
study remains separately disclosed and is not silently charged to a fresh ledger.
All runs use the existing authority ledger, including previous/unknown charges. A prospective
allocation update must replace the old $27.50 admission ceiling explicitly; never reset the ledger.
An unknown bill blocks further dispatch. No automatic retries or hidden overrun allowance.

Codex allowance: the user's latest instruction makes **10% remaining a hard floor**.
Check the account allowance between meaningful phases and stop above it.

## Diagnostic conclusion and next experiment

Seven of 15 training sessions contain a recognized successful pytest invocation; only three use
direct project-Python commands. Others use activation chains or task-specific selectors. Successful
invocations are descriptive observations, not evidence that every selected test scope is equivalent.
The minimal equivalence change unions distinct sessions only for identical direct project-Python
commands differing in verbosity, with native root-working-directory provenance. Unknown cwd,
activation chains, different environments, selectors and behavioral flags remain separate.

Before another learning cohort, improve trustworthy execution-context capture and demonstrate that
the compiler can extract a supported routine from natural traces. Buying more sessions without
understanding eligibility would repeat the bottleneck. A stronger teacher is a separate prospective
experiment; do not quietly substitute it into this fixed-model result.

Cold-start instincts remain a separate causal question. Any two-arm cold-only continuation must
freeze its own protocol, sources and assignment order before dispatch. It must retain all four
original unattempted CPU comparisons and must never count advisory savings as CPU action elimination.

## Offline preparation

```sh
python3 evals/reflex-proof/harness/learning_curve.py freeze /private/tmp/adage-learning-curve-v1 \
  --authority /private/tmp/adage-real-repo-gate-v1
python3 evals/reflex-proof/harness/learning_curve.py validate /private/tmp/adage-learning-curve-v1
```

These commands reuse pinned cached dependencies, sandboxed scoring and existing gold generators;
they make no paid model calls and write no execution grants. Source drift rejects validation;
retain the old attempt and freeze a new root after a protocol change. `history_inputs` enforces
ordered, unique, unexposed prefixes but does not replace native-runner/ledger provenance checks.
