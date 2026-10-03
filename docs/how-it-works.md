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
task), and the held-out comparison (four new tasks, 2026-10-01) found execution mode at 0.67× the
agent alone's model requests but with hidden tests 17/20 against 19/20, so it did not pass the
registered quality rule and stays experimental. See the
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

A failed verification names its failing tests from pytest's declared failure record, and describes
each from the test's own source, read from the bytes the run used: which parametrized case the id
names, with its value, and the test's one check. When this session added the test, the receipt
says so and asks the agent to check it against the request before changing the implementation.
pytest's own messages are output and are not passed on
([execution](execution.md#the-finish-line-check-stop-gate)).

The launch evaluation must measure accumulated learning on chronological histories and
prediction on later unseen sessions: avoided failures, eliminated agent actions, task quality,
unnecessary executions, abstentions, and net cost/time. A five-session zero-candidate checkpoint
neither proves mature learning ineffective nor supports a savings claim. Execution grants remain
specific to reviewed actions; the pattern that nominates an action need not be an exact replay.

## Six distinct behaviors

| Behavior | What happens at the hook | Model involvement | Authority |
| --- | --- | --- | --- |
| Context or suggestion | Deliver a matching procedure card, command suggestion or skill-related guidance | The model still decides or performs the step | Ordinary installed hook and route policy |
| Pre-call instinct | Rewrite a supported command to the repository's declared environment before its tool call executes | No additional model request for the rewrite | Narrow supported rewrite rules and runtime setting |
| Read-only prefetch (E1) | Run an approved invocation in the execution sandbox and deliver its projected result | No model request to select or run that invocation; the next request can consume its result | Exact local execution grant or explicitly configured inert-observe policy |
| Staged action (E2) | Run an approved verification/generation action in a disposable stage, inspect changes, then apply permitted outputs | No model request to perform the action; exceptional outcomes return to the agent | Exact local grant, execution policy, declared outputs and enforced sandbox |
| Predicted call (the Claude Code mod) | At the boundary before a model request, make the call a directive Adage already delivered names (a command, a sequence's first step, the reads a lookup listed, an armed finish line at the turn's end) as the agent's own tool call, under Claude Code's own permission checks | No model request for that step; the model reads the call's result | A person's `predict` grant on that reflex (`adage-exec`); never in the control arm, never at a turn's first request, never twice in a row but for a chain's second step (the reads Adage's own lookup named), at most six per session |
| Skill at its step (`skill` reflex) | Say that the repository uses a skill at this point (everywhere); with the Claude Code mod, invoke an inert skill as the agent's own Skill tool call, and add to a trained skill's text, as the model reads it, what the replay in this repository verified or found stale | The model still decides whether to follow the skill; invoking it costs no request, and the added lines are plain facts | The reflex's evidence (prime at most); invoking needs a person's `predict` grant, `skills.invoke` on, and a skill `adage train` recorded as inert in a pack the person accepted |

A **chain** is two steps without a model request between them: Adage makes the lookup the prompt
directed, the lookup's result binds the reads its directive names, and at the very next step
Adage reads them, one response of several Read calls. That is the only time a predicted step
follows another at once; a model step always comes before the third. The **finish line** at the
turn's end is predicted once per arming: met by Adage's call, a later edit that arms it again may
have it predicted again; if Adage's call failed, it is not predicted again and the Stop request
asks for it as without the mod. With the mod, a Bash call whose test run a fresh receipt already
covers can be **answered by the receipt** instead of running ([execution](execution.md#receipts-before-a-repeated-test-run)),
and the finish line it covers is met.

A predicted call is not a CPU execution: the harness runs it exactly as if the model had asked
for it, and Adage records it as its own (`adage_` tool-use ids, `by: adage` in the trace), so it is
never counted as the agent following a reflex, never strengthens one, and is read by the learner
as Adage's step, not the agent's. Without the mod, or without a grant, nothing is predicted and
the hooks behave exactly as before. With the mod, each Bash call is decided once whoever asks (the
mod's plan and verdict, then the command hook): the mod asks Claude Code's own permission check
before it acts, so a receipt answers only a call the rules would have let run unasked, and a
rewrite or a learned replacement runs only when the rules treat both commands alike (otherwise
the binary's advice reaches the model beside the call as written). The repository's standing
reflexes sit once in the session's cached system prompt instead of arriving at the first prompt,
the project's environment note rides on the Bash tool's description, compaction is told to keep
the procedures the session followed, a subagent is told the conventions its task binds before its
first request, and a step the model takes only to do what a delivered directive says can go to a
smaller model the person chose (`runtime.routine_model`). Every hook record carries the hook's own
time, and `adage doctor` reports the p95.

A `skill` reflex says "In this repository the `release-checklist` skill is used at this point"
after the prompt, edit or learned sequence where sessions here used it, and is followed when the
agent uses that skill. It is never said as an instruction (prime at most), because a skill's text
can carry commands of its own. It is learned from traces as a command is: a Skill tool use is a
step the learner may direct (never the context of another reflex), after a family of similar
requests (the skill used before the first edit), after an edit, or after a learned sequence, with
the same support, rate and uplift bars as a command; a trace that a skill directive steered is no
evidence for that skill, and only for it. With the mod it can go further, in two separate ways. Invoking: at
the next step the Skill tool is called in the model's place, only for a skill `adage train`
recorded as running nothing when it expands (no dynamic-context block, no `context: fork`), in a
pack the person accepted, with `adage config set skills.invoke true` and a `predict` grant on the
reflex. Verifying in place: whenever any trained skill expands, however it was invoked, its text
reaches the model with a dated list of the steps the replay verified here or found stale
(`verified.json`, [training](training.md#verified-in-place)). Both read the skill file again
first: a skill edited since it was trained and accepted is neither invoked nor annotated. The control arm does neither and
records what it would have done.

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

**Candidates are counted, never delivered.** A sequence (two or three events, then a command)
seen in fewer traces than a reflex needs, but in at least `min_support − 2` and never fewer than
two, is kept as a *candidate*, from a mining pass of its own that leaves the reflexes' mining
untouched; so is a nascent reflex with a context trigger, which waits for review. Candidates live
beside the learned track (`candidates.json`), carry the id the same pattern gets as a reflex, and
are compiled into the index apart from the routes, with their own context net, so the hook can
notice one without directing anything. The day a candidate reaches support it becomes a reflex
under the same name, and the learn records which traces tipped it.

**Noticed at hook time.** The hook keeps a second context ring per session over the candidates'
own alphabet, fed by the same events as the reflexes' ring. When an event completes a candidate's
context, the decision records `noticed` (the candidate, its support so far, the support a reflex
needs, whether it waits for review), once per candidate per session and at most 64. Nothing
reaches the model; both arms record it alike; `adage watch` and the mod draw it as a line under
the batch. A repository with candidates and no reflexes yet takes the hook's match path, and an
event outside both alphabets is still skipped by the fast path.

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
stay silent. A repository kill switch stops hook interventions: `adage off` writes
`.adage/disabled` from anywhere, an agent session included (it only turns something down), and
`adage on` removes it, a person's decision that refuses inside an agent session. See [execution and recovery](execution.md)
for the exact controls and limits, including direct system-Python resolution on macOS.

Every such decision is a person's, made at a terminal or, in Claude Code, with a **press**: an
answer in the session's own dialog or pane, which the binary accepts in place of a terminal only
through a single-use token it issued to the mod before the agent's first act, read on stdin and
replaced at every use. A decision made that way is recorded `via: press` (a terminal's, `via:
terminal`), and a grant for an action is two presses, stage then confirm, with what the action
would change shown between. The press is a record and a guard against the model being steered,
not a boundary against an agent with a shell; installing, settings that change what Adage learns
from, sharing and every `adage-exec` verb stay a terminal's. One Claude Code plugin ships both
binaries: its hooks run `adage-exec` while the execution marker exists (written by `delegate on`,
or by the first confirmed action), else `adage`, from the next session; they are registered once.

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
the CLI's person-gated controls (or, for grants, a press from the Claude Code session). A predict
grant reads `Predicted` in the execution column while it holds in the open checkout. “Automatic” means eligible under the displayed grant and
configuration, not that the trigger fired or a control session will execute it.

Programs that draw Adage beside a session (the Claude Code mod's pane and band) read the same
state as the terminal screens, as JSON: `adage watch --follow --json --session <id>` is the live
feed, one line per hook decision, `learn.log` line, health change and learner state, each with a
cursor a restarted reader resumes from, and it never names the arm of the session it runs in;
bare `adage --json` is the status card's rows one to one, plus which of the mod's own lines the
person turned on (`ui`) and an interrupted action waiting for them (`recovery`). A harness that knows a session's usage
records it with `adage ledger note` (the Claude Code mod does at every turn's end), accepted only
for a session the spool shows started in that repository; its figures replace the transcript's in the savings estimate, where a step Adage took
in the model's place counts as a model turn not spent ([live learning](live-learning.md)).

In Claude Code the plugin's module draws from those reads and from the command hooks' own
answers, which it observes and returns unchanged: a dim line under a prompt a reflex fired at,
worded as what the reflex tells the agent; a marker under a tool row Adage rewrote, predicted,
replaced or headed off; one band line above the prompt that counts what happened and names the
health; a toast when learning changed something; and `/adage`, a pane with the live feed, the
reflexes and the health checks. It also shows learning as it happens: a dim `○ adage noticed`
line under the tool batch that completed a sequence Adage has counted in earlier sessions (never a
single call, never a forecast), and under a Skill tool row what Adage did with the skill (loaded it
in the model's place, or verified its steps here and said which are stale, with `/adage why` for
the list). The one thing it decides is who decides: every decision a person alone may make (enable
a reflex that waits for review, let Adage run a step, use a team's reflexes, turn Adage back on) is
asked in the dialog Claude Code itself uses to ask a question, at the end of a turn, or pressed in
the pane, and runs through the press, a single-use token the binary issues at the session's start
and the mod holds in memory; the token travels on stdin, so the model, which can only call tools,
cannot press, and a Bash command it is talked into running gets none. An execution grant is two
acts with the facts between (what it runs, what it changes, the diff). The session's own rows
reach the module with every record that would name the arm removed, by the binary and again by the
module ([usage](usage.md#claude-code)).

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
scaffolding) are not launch statistics. Adage saves model calls by handling learned reflexes and
instincts on the CPU; claims are stated in model requests, tokens and API cost. GPU use and energy
are not measured, so no GPU or energy savings may be claimed.
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
| The Claude Code mod's decisions: predicted steps, a Bash call's two phases, standing text, routing | `crates/adage-runtime` (`predict_step`, `route_step`, `standing`), `crates/adage-hook/src/phase.rs`, [the mod's notes](claude-mod/impl/A1-A2-C5-notes.md) |
| The mod's chains, finish line, receipt as the result, noticed candidates and attachment by capability | `crates/adage-runtime` (`predict_step`, `answered_by_receipt`, `notice`, `standing_for`, `delegation`), `crates/adage-hook/src/harness/claude.rs` (`capabilities`), [the notes](claude-mod/impl/C3-C7-C8-D2-C9-E-notes.md) |
| Hook latency on this machine | `duration_us` on every spool record, `crates/adage-cli/src/doctor/latency.rs` |
| Grants, sandbox, projected results and staged actions | `crates/adage-exec`, [execution](execution.md) |
| Signed reusable programs | `crates/adage-pack`, `crates/adage-sign`, [packs](packs.md) |
| User-visible state and controls | `crates/adage-cli/src/ui` |
| The feed and the card as JSON, the repository switch | `crates/adage-cli/src/ui/follow.rs`, `status_card.rs`, `switch.rs`, [live learning](live-learning.md#the-live-feed-adage-watch---follow---json) |
| Harness-reported usage and the savings estimate | `crates/adage-app/src/harness_ledger.rs`, `savings.rs`, `crates/adage-cli/src/ledger.rs` |
| Paired live experiments and cost accounting | `evals/live`, [evaluation guide](evals.md) |
