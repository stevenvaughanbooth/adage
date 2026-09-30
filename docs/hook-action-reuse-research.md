# Hook action reuse: research and next experiment

Research checkpoint: 2026-09-30. This is a diagnosis and prospective experiment plan,
not evidence that duplicate work has been fixed.

Subsequent live diagnostics, evaluator stdin isolation, candidate production integration, and
current validation are recorded in the [launch handoff](launch-handoff.md). The experiment plan
below is the original research checkpoint, not the latest execution status.

## The observed problem

The completed Marshmallow transfer study executed one learned, human-approved pytest
routine successfully in all four automatic sessions. Each native transcript contains
both `hook_success` and `hook_additional_context`. Each agent subsequently requested
the exact command again (2, 1, 1, 1 times). See the immutable
[delivery audit](../evals/reflex-proof/harness/results/20260930-live-cpu-transfer/delivery-audit.json)
and [findings](../evals/reflex-proof/harness/results/20260930-live-cpu-transfer/FINDINGS.md).

An additional read-only inspection found that the first post-receipt assistant tool
request in each session was a narrower pytest node with verbose flags and the bare
`python` interpreter. The exact approved command was repeated later. A narrow diagnostic
request is not automatically interchangeable with a quiet suite result. The old frozen
gate correctly remains unsuccessful; neither argument normalization nor cache accounting
can erase the later exact requests.

The emitted completion has the same `adage-reflex`, `strength="prime"` envelope as
advice. It says verification succeeded but carries no test count, structured result,
receipt identifier or explicit snapshot freshness. Its final sentence describes the
learned origin as not requested by the user. These are candidate causes, not established
causes. The transfer task prompt asks for tests, a docstring and changelog; it does not
explicitly require personally rerunning tests.

Native attachment evidence is stronger than capturing hook stdout, but it is not a
capture of the provider's model request. Do not state that the model understood or
trusted the result merely because the attachment exists.

## What other systems already provide

| Primary source | Verified mechanism | Implication for Adage |
| --- | --- | --- |
| [Claude hooks reference](https://code.claude.com/docs/en/hooks#posttooluse-decision-control) | `additionalContext` supplies context; `updatedToolOutput` replaces a completed tool's result, subject to that tool's output schema. | Result delivery is distinct from advice. Rewriting a result cannot undo an execution. Preserve the original tool's semantics. |
| [Claude changelog](https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md#21121) | Version 2.1.121 added result replacement for all tools. | Installed CLI reports 2.1.282; its binary contains the replacement field and schema-rejection diagnostic. That establishes a candidate capability, not a successful live integration. |
| [Gemini CLI hook specification](https://github.com/google-gemini/gemini-cli/blob/main/docs/hooks/reference.md#beforemodel) | `BeforeModel` can supply a synthetic response and skip inference; `AfterTool` supports a programmatic tail tool call. | Harnesses can expose deterministic continuation without asking a model to choose the action. Do not assume Claude has the same API. |
| [LangChain custom middleware](https://docs.langchain.com/oss/python/langchain/middleware/custom#wrap-style-hooks) | Model and tool wrappers may call the underlying handler zero times, supporting caching and interception. State updates are also supported. | Returning an existing tool result and avoiding a model call belong at different execution boundaries. |

There is directly related community experience, but not a proven drop-in fix for our
failure. [Claude issue #6416](https://github.com/anthropics/claude-code/issues/6416)
requests structured hook feedback and illustrates tests following edits; it is a proposal,
not measured proof. [Issue #19432](https://github.com/anthropics/claude-code/issues/19432)
reports hook context captured in logs without effective model delivery; it concerns
PreToolUse and cannot establish the cause of our PostToolUse result. Treat both as
firsthand reports with limited scope, not provider guarantees.

This research establishes prior art for deterministic routing, hook feedback and
execution caching. It does not establish whether Adage's complete learned, shareable,
approved reflex system is novel. Avoid claiming that nobody has done this before.

## First-principles contract

Prediction nominates useful work. Approval authorizes it. Execution produces a result.
The harness must then incorporate that result into the task's state before the next
decision. These are separate responsibilities. A confidence score cannot substitute for
proof that a prior result is still valid.

Use fuzzy sequence/embedding matching to nominate an action, as intended. Reusing a
completed result needs stricter validity: exact action identity, repository and working
directory, relevant source/configuration inputs, interpreter/dependencies, approved
environment, result status and declared test scope. Changes invalidate reuse. External,
time-dependent or nondeterministic inputs require abstention or a separate explicit policy.
That validity check does not turn the learner back into an exact-match predictor.

Three outcomes must be measured separately:

1. **CPU execution:** the approved routine ran without inference selecting its invocation.
2. **Execution reuse:** a repeated request returned a valid prior result without another
   subprocess. The model already spent inference requesting it.
3. **Agent action avoidance:** the agent proceeds without requesting the completed work.
   Count total provider requests/tokens/cost too: tool calls can share one model response.

No one of these metrics measures GPU utilization or energy directly.

## Smallest experiment before another real-repository cohort

First freeze a new diagnostic protocol and record it against the shared paid ledger.
Do not rerun or modify `transfer-v3`. Use an isolated fixture with a real deterministic
verifier and a random result marker that cannot be inferred from its source or task prompt.
Keep task wording neutral across arms. A visibility-only prompt may ask for the marker,
but that probe is not an action-avoidance evaluation.

1. **Delivery canary:** verify a supported native tool-output shape; confirm that the
   marker reaches the assistant, original edit/result semantics survive, and malformed
   replacement is rejected or falls back. Test current `additionalContext` too. Store
   CLI version, settings, hook output, native transcript and costs. Do not describe a
   parser unit test as successful provider delivery.
2. **Receipt ablation:** compare current completion prose with a typed completion in
   the same context channel. Include actual exit status, command/scope, receipt ID and
   validated freshness; add only approved, bounded result fields. Keep provenance tags
   so learning still excludes steered sessions. Do not dump arbitrary stdout into context.
3. **Channel ablation:** only after the canary passes, compare that same receipt in
   supported result delivery. Preserve the originating Edit response; never fabricate a
   Bash tool call/result pair or pretend a test result is the file-edit result.
4. **Negative cases:** changed source after verification, changed configuration/runtime,
   failed verification, broader requested scope and explicit rerun requests must remain
   actionable. A mechanism that prevents necessary testing fails even if repeats decrease.

Use a small fixed diagnostic matrix (for example three independent runs per arm), with
all attempts retained and no best-of selection. Predeclare the sample count and spend
ceiling in the protocol, stop on integration failure, and report rates without treating
this convenience sample as launch statistics. The next agent must implement/account for
the probe through the existing paid harness; this document is not a new runnable evaluator.

If a fresh completed result is reliably consumed, run a new preregistered real-repository
comparison with frozen learning and unseen transfer tasks. Include a no-execution control,
quality checks and failure/drift cases. Fix cache-only receipt scoring prospectively and
report both exact repeated commands and any justified semantic equivalences separately.

If delivery improvements still fail, use explicit harness state/continuation integration.
A tool wrapper can provide safe execution reuse, but mark that as execution reuse. A
pre-model continuation can eliminate a decision only where the host exposes that control.
Do not implement a denial/retry loop or conceal tool availability to manufacture savings.

## Code entry points and limits of this checkpoint

- `crates/adage-runtime/src/render.rs::action_predicted`: current completion text.
- `crates/adage-exec/src/hook.rs::act`: successful E2 receipt to delivery mapping.
- `crates/adage-exec/src/stage.rs::Staged`: typed projected fields; raw stdout is not
  retained for unrestricted model delivery. Preserve this boundary.
- Existing execution journal and approval identity should back receipts; do not create
  a second execution engine or weaken person-issued grants.

No production runtime change or new paid experiment was made during this research.
The CPU pilot was removed from public launch-site assets at the user's request; its full
negative evidence remains in the repository. The next meaningful gate is the delivery
canary and controlled receipt ablation, not a larger benchmark using unchanged behavior.
