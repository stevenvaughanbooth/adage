# How Adage works

This is the product behavior reference for the README, CLI, TUI and launch copy.
Updated 2026-10-01 against the launch branch (`codex/launch-first-reflex`, with main's learner
fixes from PR #21 merged). Adage is an unreleased developer preview, built from source.
Implementation changes must update this page and the relevant operational guide in the same change.
The older numbered [architecture document](Adage%20%E2%80%94%20Architecture.md) is a historical
implementation snapshot, not the current feature checklist.

## The promise and its boundary

Adage learns recurring development procedures from a repository's agent sessions, recognizes
when they apply, and handles supported routine decisions at the agent harness boundary. Its
normal learner and runtime operate locally on the CPU. It does not replace the model's reasoning
about a new bug or turn arbitrary observed shell commands into trusted automation.

The first-use target is `adage init` in a repository, followed by learning existing sessions.
The interactive wizard connects a detected harness and bootstraps that repository. Subsequent
hooks can learn in the background. No history is a valid starting state: repository-derived
instincts can help where applicable, and Adage learns as sessions accumulate. A repository with
no applicable history or declared conventions must receive an honest “nothing learned yet.”

See [zero-config behavior](zero-config.md), [live learning](live-learning.md) and
[harness integration](harnesses.md) for exact commands, source locations and settings.
The operational [first-reflex guide](first-reflex.md) walks through setup, empty-history states,
the separate execution-policy and per-reflex decisions, and checking that an action replaced work.
Noninteractive previews remain read-only; installing hooks is an explicit setup operation.

## Accumulated prediction and current execution limits

The intended primitive learns patterns across many sessions and applies them to later, different
tasks. Related event sequences may support the same action even when filenames, test selectors,
intervening observations, or individual commands differ. Successful outcomes and failed paths
both inform which action is useful. An embedding similarity is evidence of relatedness, not by
itself a calibrated probability that execution will help.

The existing graph engine embeds events, forms prototypes, mines cross-session sequences, and
scores their continuations. The CPU runtime tracks bounded recent context and recognizes
multi-event patterns with gaps. Prompt intent also uses embeddings. The ordered context retains
information that a text embedding alone does not establish.

Automatic execution covers approved lookup prefetches, armed workflow obligations, and learned
fixed-command context predictions with execution evidence. A selected sequence prediction enters
the same E2 approval, staging, journal and result-delivery path without inventing an edit
obligation. Context execution is currently one slot-free verification or regeneration command,
once per route per session. Multi-command predictions and additional dynamic bindings remain
unsupported. The earlier immediate exact edit-command certificate has been generalized; the
original evaluation records retain its results. A native synthetic test now learns from five
prior sessions and executes that learned context on a fresh session with an intervening read.
Real-session predictive savings still require held-out evidence; the first replay of the frozen
real-repository training histories produced zero executable certificates.

### Verification ownership (opt-in E2)

When a person has opted this machine in to E2 (`adage delegate policy --max-level e2`), approved
one learned verification for the repository (`adage review delegate … --level e2 --effect verify`)
and turned on the execution companion (`adage delegate on`), Adage takes responsibility for that
verification in treated sessions on Claude Code and Codex:

- **The SessionStart contract.** The agent receives a short, factual note, separate from the
  person's startup notice: which command Adage runs (approved by a person, sandboxed), after
  which edits, and that it runs it again at finish if files changed since; its results arrive with
  the tool results, with the tests it ran, and a failure comes back with the failing tests. At
  most two commands are named. Only an approval whose key still matches the route is named: a
  lapsed or revoked one promises nothing. Control sessions receive no contract. It is a statement
  of fact, not a promise that a stored approval will run; every execution check still applies.
- **Receipts.** A completed learned-context action delivers a short receipt: the command, the
  result, the exit status, the duration and a short receipt id. A passing pytest run adds how many
  tests it collected and names tests from pytest's declared cache record (never from output),
  preferring the tests this session's edits added ("the tests added in this session: …"). The
  journal keeps the declared-input digest; it is not a freshness certificate for the whole
  repository, and raw stdout is not exposed. A later test call that a fresh passing receipt
  already covers gets a note saying so; it is never refused.
- **The finish-line check (Stop gate).** When the agent finishes, if that verification ran
  earlier in the session and has not passed on the current inputs, Adage runs it again through
  the same gates. Passed: the turn ends and the person sees one line. Failed: the agent continues
  once, with the failing tests, in a continuation marked as Adage's, not the user's. A second
  failure ends the turn with a line for the person; at most two gate runs per session. An
  unconfirmed run tells the agent not to retry and pauses actions in that worktree until a person
  acknowledges recovery. OpenCode never runs the gate.
- **Lapse notices.** A grant is bound to its approval key (repository, command definition,
  pinned scripts, effect, outputs, sandbox profile and, for project Python, the environment and
  interpreter). When the key stops matching, nothing runs, and the person is told: one line at
  most once per session (Claude Code; the agent never sees it) naming the grant, the field that
  changed and the `adage review delegate …` command that approves it again after checking the
  change. `adage delegate status` and `adage review` list every lapsed grant.

The contract marks the session as guided for learning, conservatively excluding its continuation
from organic demonstrations. This integration has local regression coverage. On real tasks it
has one development result (dev-v9b: one task, with development scaffolding, iterated on that
task), not held-out evidence; the held-out comparison has not run. See the
[launch evidence index](../evals/reflex-proof/LAUNCH-STATUS.md) and the exact mechanics in
[execution](execution.md#the-finish-line-check-stop-gate).

### The orientation in delivered procedure cards

A delivered procedure card no longer quotes the exemplar lines the latest passing session added.
In their place it says where the card's files stand in the checkout when the card is delivered:
each file's line count, a Python source's last top-level definitions with their lines, a test
file's last lines and a changelog's first headings. This is Adage's own read-only process on the
CPU: it runs no command, reads only the card's own files (never patterns, dotfiles, ignored or
secret-holding files, symlinks, files outside the checkout or files over 1 MiB), withholds any line
that looks like a secret, and is bounded at about 1,200 tokens. The learned card keeps its
exemplars; `ADAGE_CARD_ORIENTATION=0` delivers the earlier form. See
[training](training.md#recurring-tasks-procedure-cards).

The launch evaluation must measure accumulated learning on chronological histories and
prediction on later unseen sessions: avoided failures, eliminated agent actions, task quality,
unnecessary executions, abstentions, and net cost/time. A five-session zero-candidate checkpoint
neither proves mature learning ineffective nor supports a savings claim. Execution grants remain
specific to reviewed actions; the pattern that nominates an action need not be an exact replay.

## Four distinct behaviors

| Behavior | What happens at the hook | Model involvement | Authority |
| --- | --- | --- | --- |
| Context or suggestion | Deliver a matching procedure card, command suggestion or skill-related guidance | The model still decides or performs the step | Ordinary installed hook and route policy |
| Pre-call instinct | Rewrite a supported command to the repository's declared environment before its tool call executes | No additional model request for the rewrite | Narrow supported rewrite rules and runtime setting |
| Read-only prefetch (E1) | Run an approved invocation in the execution sandbox and deliver its projected result | No model request to select or run that invocation; the next request can consume its result | Exact local execution grant or explicitly configured inert-observe policy |
| Staged action (E2) | Run an approved verification/generation action in a disposable stage, inspect changes, then apply permitted outputs | No model request to perform the action; exceptional outcomes return to the agent | Exact local grant, execution policy, declared outputs and enforced sandbox |

A procedure card is useful learned guidance, but delivering it is not evidence of CPU execution.
An observed command running successfully is not evidence that it saved a model request. If the
agent repeats a prefetched lookup, the hook did work without necessarily removing any work.
Skill invocation can intentionally hand control to the agent when judgment is required.

The [learned lookup gate](../evals/reflex-proof/harness/results/20260929-learned-action-gate/FINDINGS.md)
now demonstrates the full E1 chain on three held-out triage tasks: default learning from nine
prior sessions, one human grant, complete CPU-produced results, zero agent lookup actions, and
correct answers. The same reflex in suggest mode required one lookup action per task. This
validates that mechanism; broader coding-task savings and platform reliability remain separate.

The [real-coding transfer pilot](../evals/reflex-proof/harness/results/20260930-live-cpu-transfer/FINDINGS.md)
now connects ten explicitly guided prior sessions to successful automatic pytest execution on
four different coding tasks. All four automatic outputs passed independent checks, versus three
of four baseline outputs. Every automatic session nevertheless requested the test command again
after receiving its completion context. Thus CPU execution is demonstrated in this pilot, while
the action-elimination launch gate remains unmet. No physical GPU savings were measured.

## From history to a reflex

1. **Discover and import.** Read supported Claude, Codex and OpenCode history sources. Retain
   repository/session attribution and the original tool arguments and outcomes that the source
   actually supplies. Missing metadata stays unknown.
2. **Build learning evidence.** The generic graph learner connects situations, operations,
   value flows and outcomes. It looks for recurring applicable actions, avoidable failed paths,
   edited-scope checks, sequences and families of similar work.
3. **Compile and qualify.** Produce typed routes, argument contracts and evidence. Repetition
   alone is insufficient: applicability, bindings, effect and outcome matter. A route can remain
   inactive or advisory when the evidence does not support stronger use.
4. **Publish locally.** Validated tracks compile into an index used by the shared hook runtime.
   Review decisions, imported package limits and repository scope still apply.
5. **Recognize at runtime.** Match the current event and bounded session context on the CPU.
   Decide whether to suggest, rewrite, prefetch or stage an approved action. Preserve result and
   intervention provenance so Adage's own actions do not masquerade as independent agent evidence.
6. **Observe outcomes.** Track delivery, following, completion, refusal and control-arm behavior.
   Update learning as evidence accumulates. “Followed” is not the same metric as “helped.”

`adage condition` is the existing learning command; setup and background learning use that
pipeline. `adage sense` previews available evidence without publishing learned routes.
`adage train <SKILL.md>` is a separate deterministic skill compiler: it makes no model calls
and replays compiled commands in a sandbox. Its optional `--execute` evidence workflow starts
paid agent sessions only with an explicit human invocation. Neither is background learning.

Routine actions have a different value test from reminders. If agents already run a check
reliably, reminding them can add context without saving work, while an approved CPU action can
still replace their next tool call. The graph learner can retain an **execution-only candidate**
for a single verification or regeneration command after an edit or learned sequence context.
It recounts the actual compiled trigger using the runtime's context matcher, requires the normal
support threshold (five unsteered sessions across multiple tasks by default), and requires at
least 90% of trigger opportunities to have a compatible successful continuation within eight
observed events. Inspection and failed checks may intervene. Another edit, setup, generation,
publish action, task boundary or a different successful check stops that search. A command
already queued in the triggering model turn receives no credit. Every trigger opportunity counts,
including those without a supporting outcome. Related selectors and options can support a typed
action family through the existing command-variant model; this is observational support, not a
claim that every broader compiled command has already succeeded. No repository-specific detector
is added. Compound shell commands currently cannot supply execution-certificate success: a final
exit code alone cannot establish each segment's outcome. Predictive contexts that fail the recount
are discarded rather than becoming advisory directives.

For direct project-Python pytest commands, the compiler can combine successful sessions that differ
only in output verbosity (`-q`, `-v`, or their repeated/long forms). It unions session identities,
retains a command that actually succeeded, and preserves interpreter, targets, selector values,
other flags and argument order. This new pooling requires native per-tool root-cwd evidence;
unknown cwd or an earlier/same-turn unknown shell command disables it. Activation chains and older
cached/importer data do not supply that provenance. This deliberately narrow action equivalence
does not turn related task embeddings into permission to run arbitrary commands.

The route records these observational counts in `evidence.execution`. They do not grant execution
or prove savings. These candidates stay silent in ordinary advisory mode and become eligible
only through the existing execution review and local grant. Actual avoided agent work still
requires a paired live evaluation; see the [launch evidence index](../evals/reflex-proof/LAUNCH-STATUS.md).

## What embeddings do

The intent subsystem supports versioned local static embedding models, including bundled
quantized weights and optional installed models. A separately identified hashed fallback is
not equivalent semantic evidence. Runtime vectors must match the representation used to learn
the route.

Embeddings help recognize related task descriptions. They do not encode execution permission,
exact argument identity or reliable causal order. The measured Potion model produces identical
vectors for two sentences with reversed procedural order. Original values, event ordering,
result links, repository state and typed contracts therefore remain necessary alongside vectors.

## What is automatic after setup

Automatic learning, matching, applicable installed instincts and advisory delivery are governed
by the normal configuration. Supported command rewrites can avoid a procedural mistake before
it occurs. These are the immediate-value path and must be tested without seeded benchmark rules.

Running repository scripts automatically has an additional authority boundary. Learning a
script does not approve it. Reviewing evidence does not approve execution. Enabling the executing
hook does not grant every route. A person's execution grant is local and bound to repository,
executable definition, pinned script bytes, effect, output contract and sandbox profile.

For text-only lookups, an explicit complete-stdout field can preserve the whole answer, including
line breaks, within its declared bound. It requires unstructured-output policy and an exact human
grant. Unsafe, empty or oversized output is withheld; it never becomes a silently truncated task.
Selected title/path fields alone do not establish that the lookup is complete. For automatic E1
execution, each prompt input must have exactly one distinct matching value within the full bounded
scan. Ambiguous prompts retain ordinary guidance instead of executing an arbitrary first match.

Changes to that bound definition or a pinned script invalidate the old grant. Evidence updates alone need not.
Disabling execution preserves learned knowledge and ordinary suggestions; execution-only candidates
stay silent. A repository kill switch stops hook interventions. See [execution and recovery](execution.md)
for the exact controls and limits, including direct system-Python resolution on macOS.

The script closure is not a complete dependency graph. An approved `make gen` can invoke
repository code that is not individually pinned in its grant. The staged input snapshot catches
source changes during a run; it does not make every earlier Makefile or transitive dependency
change require new approval. Treat that distinction as an execution limitation, not as complete
hermetic compilation.

## Developer-facing states

The Reflexes view separates **evidence strength** from **execution authority**. A strong reflex
may still be Suggest. A saved grant outside the currently open repository is not displayed as a
validated automatic action. Changed pinned scripts require review. Disabled and unavailable states have
plain-language reasons; color alone is never the status.

Execution history distinguishes completed, failed, refused and unknown outcomes. A durable
preparation record precedes E2 application; a missing completion after a possible side effect is
unknown and pauses E2 application in that worktree. Inspect before explicitly acknowledging it;
acknowledgement confirms neither success nor rollback and does not retry the action. See
[execution.md](execution.md) for recovery commands and the limits of this guarantee.

The redesigned Reflexes tab shows mode, trigger, action and latest E2 outcome before the evidence
details, using the shared terminal design system and the new tab layout. Its
evidence-review approval does not grant execution; execution grants and recovery currently use
the CLI's person-gated controls. “Automatic” means eligible under the displayed grant and
configuration, not that the trigger fired or a control session will execute it.

## Packages and future distribution

Signed packages can contain compiled routes, typed contracts and pinned scripts/assets. Package
integrity, signer trust, local compatibility and local execution approval are separate checks.
Installing or sharing a package does not transfer another developer's execution grants, private
history or credentials. Use [packs](packs.md) for the implemented format and transfer workflow.

A public marketplace is a future distribution layer over these primitives. It is not required
for local value and is not a currently validated launch capability.

## What may be claimed on the launch website

Each performance claim must identify the shipped revision, setup, model, task population,
sample size, comparison, task-quality result and uncertainty, with a reproducible evidence link.
Use paired held-out tasks; count tasks where no reflex fired and tasks that failed or hit limits.
Include result-context tokens and compilation/training costs when relevant.

Keep these claims separate:

- Hook work completed before the next model request: execution mechanism.
- A route predicted the correct next action and arguments on held-out history: learning accuracy.
- A user completed work more cheaply or quickly at preserved quality: developer value.
- A recipient benefited from an imported package: transfer value.

Prototype cards, synthetic repository sessions, historical replay estimates and current shipped
live runs are different evidence classes. A speedup measured in one must not be promoted into a
universal savings claim. Development results (such as dev-v9b, one task with development
scaffolding) are not launch statistics. Model requests and API cost are not GPU or energy
measurements: no GPU, energy or inference savings have been measured, so none may be claimed.
The launch studies used one model (Claude Haiku 4.5) on one repository (Marshmallow); they do
not establish generality across repositories or models, and transfer to cheaper models is
untested. Existing benchmark results and their limits are in [evals.md](evals.md);
the launch work's new measurements and outstanding gates are in the
[reflex proof ledger](../evals/reflex-proof/README.md).

## Source map

| Responsibility | Code / operational reference |
| --- | --- |
| Setup, discovery, background learning | `crates/adage-cli`, [live learning](live-learning.md) |
| Import and generic learning | `crates/adage-learn`, `crates/adage-graph` |
| CPU intent features | `crates/adage-intent` |
| Routes, evidence and compiled index | `crates/adage-tracks` |
| Shared hook adaptation and decisions | `crates/adage-hook`, `crates/adage-runtime` |
| Grants, sandbox, projected results and staged actions | `crates/adage-exec`, [execution](execution.md) |
| Signed reusable programs | `crates/adage-pack`, `crates/adage-sign`, [packs](packs.md) |
| User-visible state and controls | `crates/adage-cli/src/ui` |
| Paired live experiments and cost accounting | `evals/live`, [evaluation guide](evals.md) |
