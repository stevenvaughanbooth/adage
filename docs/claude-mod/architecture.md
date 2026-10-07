# Adage in Claude Code: the mod

> Status: **proposed 2026-10-02**, against Claude Code 2.1.287, the first release with mods on by
> default. A design, not a report: nothing here has run. Spike [S9](../../spikes/S9-claude-mods/SPIKE.md)
> holds the questions that must pass before each part ships, and every claim about the mods API
> below is from the public docs ([overview](https://code.claude.com/docs/en/plugins/mods/overview),
> [events](https://code.claude.com/docs/en/plugins/mods/events),
> [API](https://code.claude.com/docs/en/plugins/mods/api),
> [interface](https://code.claude.com/docs/en/plugins/mods/interface),
> [reference](https://code.claude.com/docs/en/plugins/mods/reference)) or the declaration file
> Claude Code writes for a mod (`claude-code.d.ts`), which the reference calls the authority.
> Words follow the [lexicon](../lexicon.md); what the person sees follows the
> [style guide](../design/style.md); what the model reads stays plain and factual. The
> person-facing detail is in [ux-design.md](ux-design.md), the build order in
> [implementation-plan.md](implementation-plan.md), and how it is tested in
> [test-plan.md](test-plan.md). Revised once after review; the review's findings are folded in
> and the places where this design and the plan had disagreed now say the same thing.

A mod is a plugin whose TypeScript runs *inside* Claude Code: functions the engine calls on every
tool call, prompt, model request, transcript row and screen draw, with an API (`$`) to the screen,
the host's processes and files, the session's usage figures and a store. Adage's command hooks run
*outside*: a spawned `adage hook claude <event>` (or `adage-exec hook claude <event>` when
delegation is on) with JSON in and JSON out. Through that channel the binary adds context,
rewrites a Bash command's interpreter, blocks a Stop up to twice, and, under the adage-exec
plugin, runs a delegated E1 or E2 action inside the `PostToolBatch` and `Stop` hooks.

This document designs Adage as a mod. The Rust binary stays the decider, the learner and the
sandbox; the command hooks stay as the floor on every host; the mod is one more file in each of
the two plugins. What the mod adds falls in two halves, and both are in the title:

- **Capability.** The mod sits at the one point the command hooks cannot reach: the moment before
  a model request goes out. When a directive Adage already delivered names the next step and a
  person has granted that reflex `predict`, the mod makes that call and the request is never sent.
  It also answers a repeated test run from the receipt Adage already holds, checks a rewrite
  against the engine's own permission verdict, can route routine steps to a cheaper model, and
  places standing text where the prompt cache keeps it.
- **Looks.** Adage becomes visible where the person is looking: a line under the prompt a reflex
  fired at, a marker on the tool row it rewrote or chose, a receipt card in place of a wall of
  pytest output, a one-line band above the prompt, `/adage` with a live pane, and toasts for the
  moments that matter. Today the person gets one `systemMessage` line and a split terminal.

## 1. The session, before and after

The acme-billing sandbox (`demo/SANDBOX.md`) has three conventions an agent cannot guess: work
items are looked up with `scripts/wi-lookup`, tests run with `make test` (pytest is not installed
and a bare `unittest` fails on `ACME_ENV`), and a schema edit under `schema/` needs `make gen`.
Adage has learned all three. This is a treated session under the **adage-exec plugin** (delegation
on, §2), with the codegen reflex granted `predict` by the person, by a press in the session or at
a terminal (§3.2, §3.12).

**Today.** The person sees, at most, one line at session start:

```text
adage: 3 reflexes and 4 instincts active in this repo; it keeps learning as you work. Nothing runs without you.
```

Then nothing. The directives go to the model as `additionalContext`, the finish-line result as
one `systemMessage` line. To see Adage work, the person opens a second terminal and runs
`adage watch`.

**With the mod.** The same session, in the terminal:

```text
 ✻ Claude Code 2.1.287
   ◠ adage · 3 reflexes and 4 instincts active in this repo · it keeps learning as you work · nothing runs without you

 > fix W-2042 and add the currency field
   ◠ look up the work item · scripts/wi-lookup W-2042 --json

 ● Bash(scripts/wi-lookup W-2042 --json)
   ⎿ {"id":"W-2042","title":"Rounding error in invoice totals", …}

 ● Edit(schema/invoice.json)
   ⎿ Updated schema/invoice.json with 1 addition

 ● Bash(make gen)
   ◠ adage chose this step · no model request
   ⎿ generated acme/generated/models.py

 ● Bash(make test)
   ⎿ Ran 14 tests in 0.21s  OK

 ✓ Worked for 1m 48s · adage: 1 step without a model request · 1 dead end headed off

┌──────────────────────────────────────────────────────────────────────────────────────┐
│ ◠ adage  2 reflexes this session · 1 step without a model request · ● healthy        │
└──────────────────────────────────────────────────────────────────────────────────────┘
 >
```

Every line with `◠` is the mod's. The person did not run a command, open a pane or read a
`systemMessage`. The model saw the same plain `<adage-reflex>` blocks it sees today, plus one tool
call it did not have to ask for. The receipt path (§3.3) does not appear here because the sandbox
has no pytest; in a Python repository with an E2 `verify` grant, the repeated
`.venv/bin/python -m pytest` run would show the receipt card of ux-design §4.4 in place of the
output.

In a **control session** (1 in 10), the binary prints nothing, so the mod has nothing to draw per
event; the band shows the lamp and, when the learner is scheduled, the countdown, which is also
what a treated session shows before anything fires. Nothing on the screen names the arm or counts
what was withheld (§6, invariant 5).

In **`claude -p`**, the SDK, the VS Code chat panel and a cloud session, the hooks run and nothing
draws (the engine's rule). What runs headless is exactly what changes the model-facing session:
predictions under a grant, receipts as results, the engine-checked rewrite, the cached section and
compaction (§7). The evals therefore run three arms (§9): hooks alone, the mod observing only,
and the mod with grants.

## 1b. Where the value is: inference, prediction and attachment

The screen is half of it. The other half is what the mod does for the core loop, stated in the
primitives the architecture now has (`docs/architecture.md`): a canonical `Call`, `Event` and
`Session` (`adage-trace`), a `Harness` with `Capabilities`, `decide()` over an index and a
session state, and an `Extension` that may carry a decision out. Today the live loop sees a
session only through hook payloads: a prompt, a batch of tool calls, a failure, a stop. It can
add context, rewrite one interpreter word, block a stop, and, under adage-exec, run one approved
command inside the hook's budget. The mod sits inside the engine and sees, and can act at, every
point of the loop. That changes what Adage can infer, what it can predict, and where it can
attach what it knows.

### 1b.1 Prediction: from "run it in the hook" to "make the call the agent would have made"

Today's CPU prediction (`docs/execution.md`, "Learned sequence predictions"; `Delivery.cpu_predictions`
in `adage-hook`) already does the hard part: a learned context route with execution evidence
and a person's E2 grant nominates its command, adage-exec stages and runs it in the sandbox
inside the `PostToolBatch` hook, the result goes to the model as context, the directive is
suppressed, and pending follow judgements are retired. Its limits are the hook's: one slot-free
verification or regeneration command, once per route per session, inside 30 s, with declared
outputs and a sandbox profile, never a multi-command sequence, never an input slot, and the
agent still makes its next request to read the result.

The mod's `turn.step` moves the same decision to the request boundary and makes the predicted
step **the agent's own tool call** (§3.2). The grant, the evidence rule, the control arm and the
"predict only what Adage already directed" rule stay. What changes:

| Limit today | With `turn.step` | Why it is safe |
| --- | --- | --- |
| One slot-free command | Any directive step whose slots are bound by the directive itself (a lookup's work-item id, a `ReadBatch`'s file list) | The input is the rendered directive, never the conversation |
| Sandbox and declared outputs | None needed: the call runs as the agent's own tool under the person's permission rules, exactly as if the model had asked | Claude Code's permission system governs the call (Q9); the person's `predict` grant is per reflex |
| Inside the hook's 30 s | The tool runs as a tool, with the engine's own timeouts | Nothing runs in a hook |
| Once per route per session | Once per directive, within the session cap, never two in a row | The bounds of §3.2 |
| The result arrives as context in the next request the model makes anyway | The result arrives as a tool result and **that request is not made** | The API's prefill shape; Q9 shows the loop continues |

Three predictions the hooks cannot make at all follow from this:

- **A chain, step by step.** `lookup-work-item` then `read-linked-files` is two directives and
  two model requests today. The mod predicts the lookup, before the model's first response when
  the prompt directed it (§3.2, "The turn's first step"); the engine runs it; at the next step
  the binary sees the lookup's result in the batch, binds the `ReadBatch` from it (the directive
  already says "read the files it names"), and predicts the reads as one response with several
  `tool` chunks (a response may carry more than one tool call). Two requests not sent, each
  step verified by its result before the next, never two predictions without a model step
  between them per route.
- **The finish line without the Stop request.** Today an unmet obligation at `Stop` is a
  `block` with a reason, which costs the model at least one more request to run `make gen` and
  one to finish. At the last `turn.step` before the model's end-of-turn, a hook that holds a
  granted, unmet obligation can yield the obligation's call in place of the stop chunk: the step
  runs, the next request carries its result, the model ends. One request fewer per finish line,
  and the obligation is met by the agent's own call rather than the gate's sandbox run (Q15).
- **The dead end that never runs.** An `avoid` reflex is advice at failure time today: the
  failed run has already cost a tool run and a model request to recover. `tool.call` sees the
  call before it runs. Where the binary holds a learned pair (`python3 -m pytest` fails here;
  `make test` is the way) and `$.tool.check` gives the same verdict for both commands, the mod
  substitutes the learned command with one line of context saying so, exactly the discipline the
  U11 interpreter rewrite already has (§3.4). The failure and the recovery request do not happen.

### 1b.2 Inference: what the binary gets to see

The hooks deliver a prompt, batches, failures and stops. The engine delivers more, and each
item is a field on the canonical model, not a new format:

| Signal | Hook today | Mod | What it is for |
| --- | --- | --- | --- |
| Each tool call before it runs, with its `tool_use_id`, for every tool | `PreToolUse` for Bash only | `tool.call` for every tool, subagents included (`agentId`) | Substitution, receipts, dedup, subagent attribution on `Call` |
| The permission verdict | none | `tool.check` (observed, never answered) | A rewrite that would change the verdict is advice; a denied call is not a follow |
| The model's step boundary, model and usage | none | `turn.step`, `turn.complete` | Prediction; cost per step on the ledger line; the arm comparison priced from the harness |
| Compaction | none | `session.compact` | Procedures survive it (§3.7); a lookup repeated after compaction is counted as relearning, not as a new need |
| Skill expansion | none | `skill.prompt` | Verification in place; a `skill` reflex's follow judged on the Skill tool |
| Subagent spawn | `PreToolUse` on `Agent` | `agent.spawn`, with the prompt rewritable | A subagent told the convention before its first request |
| Adage's own acts | text markers | the `adage_` id, `Call.by`, `via: press`, `cpu_predictions` | The learner never counts Adage's act as the agent's |

The learner's inputs are unchanged in kind, which is the point: a `Session` from a mod-served
session decodes through the same `Format`, with `by`, `usage` and `agent` as optional fields on
`StepObs`. What the learner gains is evidence it cannot get from a hook-served trace: that a
predicted step's result was what the next step needed (prediction accuracy on live sessions,
not replay), that a substituted dead end was followed by the work the model meant to do, and the
tokens each directive saved, step by step, in both arms.

### 1b.3 Attachment: where a reflex lives in the model's context

A directive is `additionalContext` at an event today: it arrives late in the context, once per
session, and is relearned by the model after compaction. The engine offers three cheaper and
earlier places, each a capability the hooks lack:

- **The tool's own description** (`tool.describe`, cached for the session): the Bash tool says
  "tests here run with `make test`; `pytest` is not installed" before the model's first call. A
  convention attached to the tool is used on the first attempt, with no event, no directive and
  no tokens per event. This is the standing text of §3.6 in its best place.
- **A session-scoped system-prompt section** (`prompt.compose`): the repository's procedures and
  instincts once, in the cached prefix, instead of at the first event that needs them.
- **The subagent's prompt** (`agent.spawn`) and its preloaded skills (`$.agent.register`): an
  Explore agent that would repeat the lookup is told the convention before it starts.

Each is a `Capabilities` field on the harness (plan: `adage_trace::Capabilities` gains
`predict_step`, `answer_call`, `substitute_call`, `tool_description`, `cached_section`,
`subagent_prompt`, `compaction`), so behaviour is by capability as the architecture now
requires: Claude Code with the mod is the harness that has them all, Claude Code with the hooks
alone has today's four, and nothing past the adapter asks which plugin is installed.

### 1b.4 What each lever is worth, and how it is proven

| Lever | Unit it moves | Where it is measured |
| --- | --- | --- |
| A predicted step (§3.2) | model requests per task | `requests` in the demo runner, then the v9 rule (E1, E2) |
| A chain (lookup then reads) | model requests per task | the same, on the lookup items |
| The finish line at the last step | model requests per finish line | the same, on schema-edit items; Q15 |
| A dead end substituted | tool runs and model requests per dead end | `dead_end_hits`, `requests` |
| A receipt as the result (§3.3) | tool runs, tokens | `receipts_as_results`, tokens per step |
| Conventions in the tool description (§1b.3) | directives needed, tokens | directives per session, tokens, in the arm comparison |
| Procedures through compaction (§3.7) | repeated lookups after compaction | `dead_end_hits` and repeated lookups per session |
| A subagent told at spawn | subagent tool calls per delegation | subagent `tool_calls` in the arm comparison |

None of these is claimed in dollars or GPU time (`how-it-works.md`, "What may be claimed"); each
is a count the four arms of §9 compare, with hidden-test quality as the gate for all of them.

## 2. The shape: two plugins, one module, one active mod

```text
plugin/claude-code/                      plugin/claude-code-exec/        (identical module, other binary name)
├── .claude-plugin/plugin.json           # + "types": "./types/index.d.ts", + userConfig (§5)
├── hooks/
│   ├── hooks.json                       # the command hooks (unchanged) + "modules": ["./register.ts"]
│   ├── register.ts                      # on(...) registrations; every hook checks `active` first
│   ├── bridge.ts                        # ask(): one $.process.run of the binary per decision, fail open
│   ├── decide.ts                        # classic.*, tool.call, turn.step, skill.prompt, prompt.compose, session.compact
│   ├── draw/                            # ui.render hooks per site (§4), pure functions of $.state
│   │   ├── band.tsx  rows.tsx  pane.tsx  card.tsx
│   ├── feed.ts                          # the watch feed: $.process.spawn(<bin> watch --follow --json)
│   ├── commands.ts                      # /adage … (§5)
│   ├── tokens.ts                        # generated by `cargo xtask tokens`: glyphs, the accent, roles
│   └── *.test.ts                        # claude plugin test, over 'terminal' and 'desktop'
├── types/index.d.ts                     # PluginState contract: every $.state value the mod keeps
├── scripts/session-start, quiet         # unchanged
└── dist/                                # staged by plugin/build.sh at build time, as today
```

`hooks/hooks.json` "can also hold settings hooks under `hooks`" (reference, Files), so the
command hooks and the module ship in one file. The order is documented: a mod's `tool.call` hook
runs first; the plugin's own `PreToolUse` command hook runs "after the last mod calls `next`, as
part of Claude Code's own behavior", and for every classic event `next(e)` resolves to what the
command hooks answered.

**Which mod is active.** When delegation is on and the adage-exec plugin is installed, the adage
plugin's session-start shim links `$DATA/bin/adage` to `scripts/quiet` so that its hooks print
nothing and hooks never fire twice (`plugin/claude-code/scripts/session-start`, `docs/execution.md`
"Two plugins"). Receipts, the Stop gate's actions and every grant live only in adage-exec's
`Extension`. The module therefore ships in both plugins, and at `session.start` each copy probes
the binary the shim linked for it once:

```ts
// hooks/register.ts
let active = false
export function register(on: RegisterFn, options: Options) {
  on('session.start', async ($, e, next) => {
    const started = await next(e)
    const hookBin = await hookBinary($)                            // $DATA/bin/<adage|adage-exec>, as the shim linked it
    const probe = hookBin && await $.process.run([hookBin, 'version'], { timeoutMs: 1000, env: { CLAUDECODE: '1' } }).catch(() => undefined)
    active = probe?.exitCode === 0 && probe.stdout.trim() !== ''   // `quiet` prints nothing: this copy stays inert
    if (active) startFeed($, await cliBinary($), e)                // §4.1, only when drawing is possible
    return started
  })
  // every other registration is wrapped: inactive → next(e) unchanged, and ui.render → next(e)
}
```

Two binaries, two jobs. **Hook events** (`hook claude <Event>`) go to the plugin's own binary:
`adage` in the adage plugin, `adage-exec` in the exec plugin (the only binary that holds grants and
receipts). **CLI verbs** the mod needs for drawing and recording (`watch --follow --json`,
`graph --json`, `doctor --json`, `status --json`, `ledger note`, `off`) exist only in `adage`:
`adage-exec` has `hook`, `run`, `approve`, `revoke`, `status`, `doctor`, `journal` and `recover`
and nothing else. So `plugin/build.sh` stages `adage` beside `adage-exec` in the exec plugin's
`dist/<target>/` (its `SHA256SUMS` step already lists both names), the exec shim links both into
its `$DATA/bin`, `adage-exec` gains a `version` verb for the probe, and the mod never falls back to
`PATH` when its linked binary is `quiet` (a PATH binary would serve beside the other plugin's).
Exactly one copy is active in a session: under adage-exec the adage copy's hook binary is `quiet`;
without delegation the adage-exec plugin is not installed or its shim is not serving. `$.state`,
commands and panes are per plugin, so the inert copy registers no `/adage` command and draws
nothing. The test plan's T-X-11 covers both-installed. Every argv the bridge may run is an
allow-list in `bridge.ts`: `hook`, `version`, `watch`, `graph`, `doctor`, `status`, `ledger note`,
`off` and `map` from any code path, and the person-only verbs (`review …`, `delegate on`,
`delegate policy`, `delegate predict on`, `on`, `add … --trust`) only through `press()`, reached
only from a `$.ui.ask` answer or a focused `Button`'s `onPress`, with the token on stdin (§3.12);
`install` and `config` are never in it. The list is checked by a unit test and by
`scripts/check-plugin.mjs`. (With one plugin, A8, the two-copy rule above becomes a presence
probe and this paragraph's second sentence falls away; it is kept here because the plan builds
the one-plugin step last.)

Three rules keep the binary in charge:

1. **Every decision is `<bin> hook claude <Event>`**, with the payload on stdin as today. The mod
   never decides; it asks, through one helper, and the result is tagged so a failure is never
   mistaken for "nothing to say":

   ```ts
   // hooks/bridge.ts
   export type Asked = { ok: true, answer: HookAnswer | undefined } | { ok: false }
   export async function ask($: EngineInterface, event: string, payload: object, timeoutMs: number): Promise<Asked> {
     const bin = await hookBinary($)
     if (bin === undefined) return { ok: false }                   // no binary: fail open, as the shim does
     try {
       const ran = await $.process.run([bin, 'hook', 'claude', event], {
         stdin: JSON.stringify(payload), cwd: await $.session.cwd(), timeoutMs,
         env: { CLAUDECODE: '1' } })                                // F11, made structural: every child is an agent session
       if (ran.exitCode !== 0) return { ok: false }
       const text = ran.stdout.trim()
       return { ok: true, answer: text === '' ? undefined : JSON.parse(text) as HookAnswer }
     } catch { return { ok: false } }                               // cannot start, timed out, not JSON: nothing
   }
   ```

   Timeouts per event: 2,000 ms for `PreToolUse` and `SkillPrompt`, 250 ms for `TurnStep` (it
   runs before every model request; the binary's own budget for a nothing-pending step is the
   state read). `$.process.run` "takes an argument list and uses no shell", the exec form S1
   measured; `env` adds `CLAUDECODE=1` so `require_person` refuses a person-only verb in any child
   the mod runs, whatever the engine's own environment carries (`crates/adage-cli/src/share.rs`,
   `crates/adage-exec/src/person.rs`). Two new events, `TurnStep` (§3.2) and `SkillPrompt`
   (§3.12), are the decisions the command hooks cannot make.

2. **A decision is made once per `tool_use_id`.** Where the mod and a command hook both see an
   event (the Bash `tool.call` path, §3.3 and §3.4), the binary gains a bounded ring of seen
   `PreToolUse` ids on `SessionState` (`seen_pretool`, 64 entries; plan A1). The ring is checked
   *after* the existing word scan that ends the no-match path before any I/O, so a command that
   names no Python program costs what it costs today, and the command hook's repeat of a decided
   id returns nothing with no spool record. Nothing like this exists today: `event_identity`
   only hands an id to adage-exec's journal for `PostToolBatch`, `PostToolUse` and
   `PostToolUseFailure`, and `pretool()` never computes one. Prompt and batch decisions stay with
   the command hooks entirely; the mod observes their results (`classic.UserPromptSubmit`,
   `classic.PostToolBatch`) and draws.

3. **Adage's state stays in `$ADAGE_HOME`.** `$.state` holds only what a drawing reads this
   session (a list of fired reflexes, counters), declared in `types/index.d.ts`; `$.store` holds
   only the person's UI preferences (which pane tab was open), because the store is "a key-value
   store that every session on the machine shares" with a documented read-then-write race. The
   feed is one child, `<bin> watch --follow --json --session <id>`, started from `session.start`
   through `$.process.spawn` and ending with the module: every spool parser, lock probe and word
   table stays in Rust, and the mod holds rows and draws them. Daemonless (A1) holds: the child
   lives exactly as long as the session.

## 3. Capability: what the mod lets Adage do

Each subsection names the event, the exact shape the hook returns, the binary's part, what the
model reads, what the person sees, and how the control arm behaves. The letter in brackets is the
spike question that must pass first.

### 3.1 Observe the command hooks and draw [Q1, Q3]

```ts
for (const event of ['classic.UserPromptSubmit', 'classic.PostToolBatch', 'classic.PostToolUseFailure'] as const) {
  on(event, async ($, e, next) => {
    const t0 = await $.clock.now()
    const answered = await next(e)                                 // the command hooks beneath ran, once
    const t1 = await $.clock.now()
    const fired = parseReflexes(answered.additionalContext ?? [])  // route, track, kind per <adage-reflex> block
    await $.state.update(s => ({ ...s, fired: [...s.fired, ...fired.map(f => ({ ...f, prompt: s.prompts }))],
                                 hookMs: [...s.hookMs, t1 - t0] }))
    return answered                                                // unchanged
  })
}
on('classic.Stop', async ($, e, next) => {
  const answered = await next(e)
  if (active && answered.block) $.ui.toast('adage: finish line · the agent continues once')
  return answered
})
```

- **Binary:** unchanged. **Model:** unchanged. **Control:** the command hook prints nothing;
  `fired` stays empty; nothing draws. **Hosts:** every one. **Cost:** `hookMs` is the wall time
  of every settings hook under `next(e)`, Adage's included; it is recorded as "the hook layer's
  time as the engine saw it" (§3.10), and the binary's own time per record is `duration_us`
  (plan A2).

### 3.2 The request Adage does not send [Q9, Q10]

The event the command hooks lack. `turn.step` "fires when Claude Code is about to send one
request to the model"; a hook on it is an async generator; "an answer without `next` sends no
request"; chunks the hook yields are recorded and run ("a block whose `tool` chunk never comes out
of the chain is no tool call: the engine records none and runs none", and the converse).

```ts
on('turn.step', async function* ($, e, next) {
  if (!active || e.agentId !== undefined) return yield* next(e)                     // main loop only; step 0: below
  const asked = await ask($, 'TurnStep', {
    session_id: await $.session.id(), cwd: await $.session.cwd(), hook_event_name: 'TurnStep',
    turn_id: e.turnId, index: e.index, model: e.model }, 250)
  const p = asked.ok ? asked.answer?.predict : undefined
  if (p === undefined) return yield* next(e)                                       // nothing predicted: the request goes out
  const id = `adage_${p.route}_${e.turnId}_${e.index}`                             // provenance in the id (§3.10)
  yield { kind: 'tool', index: 0, id, name: p.tool }
  yield { kind: 'input', index: 0, json: JSON.stringify(p.input) }
  yield { kind: 'stop', stopReason: 'tool_use', usage: null }
  return { turnId: e.turnId, index: e.index, answer: '', toolUses: [{ id, name: p.tool, input: p.input }],
           stopReason: 'tool_use', usage: null }
})
```

- **Binary:** a new event, `<bin> hook claude TurnStep`, payload
  `{ session_id, cwd, hook_event_name, turn_id, index, model, agent_id? }` and nothing else: no
  messages, no tool output. It **predicts only what Adage has already directed**: a `pending`
  directive whose `expect_key` has not been observed, or an armed, unmet finish-line obligation,
  in delivery order, whose route holds a current `predict` grant and whose step is a `Run` with
  every slot bound, a `Sequence`'s first command, or a `Skill` (§3.11). The input is the
  directive's own rendered command, which the runtime now keeps on the `pending` entry (`Pending`
  holds only the canonical `expect_key` today; plan C1 adds `command`); nothing is filled from the
  conversation, and a step with no bound command predicts nothing. Before answering, the binary
  re-checks the session's `avoid` interlock (an obligation armed before an inhibit must not be
  predicted). Output: `{}` or
  `{"predict": {"tool": "Bash", "input": {"command": "make gen"}, "route": "codegen-after-schema", "track": "…", "kind": "obligation"}}`.
  The grant is a **mode** on the approval key, not a new effect: `ApprovalKey.mode: GrantMode
  { Execute, Predict }`, written by `adage review delegate <track> <reflex> --predict` at a
  terminal (person-only, like every delegate verb), under the same policy level (`max_level >=
  e2`), key and lapse rules as an E2 grant, with `profile_id: "harness"` because no sandbox is
  involved (plan C2). `Effect` is untouched: its order is security-relevant. What may be granted
  `predict` is narrower than what may be delegated: a route whose effect is Observe, Verify or
  Regenerate, not denied by the policy's `effects_deny`, not an environment action unless the
  policy allows them, and never an `exec_only` route (one that exists only to be executed in the
  sandbox). Because every approval lookup in adage-exec selects by repository, route and level
  today, the mode joins every selector, so a `predict` grant and an `Execute` grant on one route
  never see each other's keys. Four bounds the binary enforces: at the turn's first step (index
  0, before the model's first response) only the run reflex the prompt itself directed (below);
  never two predicted steps in a row (a model step must separate them, so Adage can never drive a
  turn alone, but for a chain's second step); at most
  `runtime.predict_cap` per session (default 6), like the Stop gate's two; and never for a route
  that also holds an `Execute` grant for the same step, because that action already runs inside
  the `PostToolBatch` hook and a prediction would run it twice (`delegate --predict` on such a
  route says so and refuses). The grant is read in the hook's outer layer and handed to the pure
  `decide()` the way exec-only routes are today (a thread-local set), so the runtime crate stays
  free of adage-exec types. A predicted step retires the directive's pending follow judgement
  the way `Delivery.cpu_predictions` does for today's in-hook prediction, and is marked
  `Judgement::Predicted`, never `Followed`; a route that today's path would run in the sandbox
  at `PostToolBatch` is not also predicted (§1b.1).
- **Model:** its next request carries an assistant turn with a tool call it did not write, and the
  tool's result. That is the API's prefill shape; models carry on from it. The call itself ran
  through every `tool.call` hook, the plugin's `PreToolUse` command hook, the person's permission
  rules, and the tool. Before yielding, the mod asks `$.tool.check` for each call and yields only
  when every verdict is `allow`: a call the rules or the mode would put to a decider (a dialog, a
  headless refusal, auto mode's classifier, which has no verdict for a call no model request
  produced) is left to the model, and the request goes out as if nothing were predicted. Adage's grant decides whether to predict; Claude Code's
  permissions decide whether it runs. **This is a third execution path**, beside E1 and E2:
  nothing runs in the hook, the E2 sandbox and journal are not involved, the call is the agent's
  own Bash under the person's permission mode. Q9 must show the engine applies permission rules
  and the dialog to a yielded call exactly as to a model's; if it does not, Layer 4 does not
  ship. Under `bypassPermissions`, `dontAsk`, an allow rule, or `-p` with `--allowedTools`, no
  dialog appears for the model's own calls either; the person's `predict` grant, per reflex, is
  the consent, and the notice under the dialog (where one appears) and the tool-row marker name
  the call as Adage's.
- **Person:** the tool row carries `◠ adage chose this step · no model request` (§4),
  `$.ui.notice` puts one line under the permission dialog when one is shown, and the turn's
  closing line counts it as a step without a model request.
- **Control:** the mod always asks (the arm is the binary's to know); the binary answers `{}`,
  spools `would_predict` on the `turn_step` record, and the request goes out. Saved requests are
  measured against a baseline, as directives are.
- **Headless:** runs (`-p` included); it is where Q9 and the evals measure it.
- **Cost accounting:** the step's `usage` is null on the engine's side; §3.9 records the step as
  Adage's with the model it replaced, so the ledger prices what was *not* spent at the same rate
  as what was.
**The turn's first step.** `turn.step` at index 0 fires after `UserPromptSubmit`, so the
prompt's directives are already delivered and recorded when it asks. There the binary predicts
one thing only: a `Run` reflex this prompt directed whose every slot the prompt's own words bound
and whose command is therefore fixed (the lookup, `scripts/wi-lookup {item}` with the work item
the prompt names), under the same grant, two-phase verdict, cap, once-per-route rule and
interlock as any step, never for a subagent. A sequence, a skill, a chain, a finish line, anything
directed before this prompt or with a slot bound from a result waits for a model step, as before;
nothing is routed at index 0, and its end is never asked (no finish line before a model step).
The made-up first response has no stop reason, so the model's first real request starts the
thread afresh with the prompt, the call and its result in it. The directive still goes with the
prompt, unchanged: it is rendered by the command hook before the step's verdict exists, so it
cannot truthfully say the call ran, and the command-hook path stays byte-identical. Instead the
call's result carries the line that says so ("Adage made this call in your place before your
first response: it is the step the learned reflex lookup-work-item, delivered with the request,
names"), so the model reads the directive, then the call it names already made, and works from
the result. Run 3 is why (evaluation-results.md): the model made the lookup in its very first
response in 36 of 36 treated sessions, so a rule of "never at index 0" left the design's first
lever, the lookup, never predicted.

**Chains and multi-call steps.** A `Sequence` or a lookup-then-read pair is predicted one step
at a time: the first call is yielded; the engine runs it; the batch that follows carries its
result; the binary binds the next directive from that result (a `ReadBatch`'s files from the
lookup's `linked_files`) and, at the next `turn.step`, yields it, as one response with several
`tool` chunks when the step is a batch of reads. A model step is never skipped twice in a row
for the same route, and the session cap counts every yielded response. **The finish line at the
last step** (Q15): when the model's response ends the turn (`stop` with `end_turn`) and the
session holds an armed, unmet obligation whose route has a `predict` grant, the hook keeps the
model's text, replaces the stop with the obligation's call, and the next request carries the
result; the Stop gate's `block` and its extra request do not happen. The response then ends with
no stop reason: the server holds it as the model sent it, without the call, so the next request
must start the server-side thread afresh rather than continue from that copy. If the obligation's call
fails, the gate behaves as today.


### 3.3 The receipt as the result [Q4, Q9]

`tool.call` may "answer a call yourself, return an object with a `result` field … without calling
`next`. When you do that, no permission prompt appears and the tool doesn't run, so the result you
return is all Claude learns about what happened." Because no permission prompt appears, the mod
first asks the engine whether the person's rules would have let the original call run unasked;
a receipt replaces only a run that needed no one's consent.

```ts
on('tool.call', { tool: 'Bash' }, async ($, e, next) => {
  if (!active) return next(e)
  const base = { session_id: await $.session.id(), cwd: await $.session.cwd(), hook_event_name: 'PreToolUse',
                 tool_name: 'Bash', tool_input: { command: e.command }, tool_use_id: e.toolUseId,
                 permission_mode: e.permissionMode }                 // the fields pretool() reads; Q4 confirms where the mode is read
  const planned = await ask($, 'PreToolUse', { ...base, dry_run: true }, 2000)
  const plan = planned.ok ? planned.answer?.plan : undefined
  if (plan === undefined) return next(e)                             // no Python word, nothing bound, or a failure: as today
  if (plan.receipt?.fresh) {
    const { decision } = await $.tool.check({ tool: 'Bash', input: { command: e.command } })
    if (decision === 'allow') {
      const final = await ask($, 'PreToolUse', { ...base, verdict: { answered: 'receipt' } }, 2000)
      if (final.ok && final.answer?.receipt?.result !== undefined)
        return { result: { stdout: final.answer.receipt.result, stderr: '', interrupted: false },
                 context: [final.answer.receipt.note] }               // the <adage-receipt> block, after the result
    }
  }
  return rewrite($, e, next, base, plan)                              // §3.4
})
```

- **Binary:** `PreToolUse` learns two optional fields a command hook never sends. `dry_run: true`
  returns the plan (`{ rewrite?: { command }, advice?: { text }, receipt?: { fresh, result, note } }`)
  and spools nothing. `verdict` carries what the mod found out and asks for the final decision,
  which is spooled once: `answered: 'receipt'` records the call as observed (the obligation it
  satisfies is met, and adage-exec treats the receipt's use as Adage's own run for the
  duplicate-run rule); `engine: 'same' | 'differs'` is §3.4. The receipt check `docs/execution.md`
  already runs ("Receipts before a repeated test run") gains `fresh` and `result`: fresh only for
  a completed, applied run the journal holds on byte-identical inputs covering the call; a
  *covering* run can be broader than the call (`covers()`), so the note says which command ran
  and when. The command hook that follows is deduplicated by the ring (§2 rule 2).
- **Model:** a shorter result than pytest's output, the named tests, the session's own tests
  first, and a note saying Adage answered from a run it made and when. It stays a fact: the model
  asked for a test run and received its result; nothing was refused.
- **Person:** the `ToolResult` row draws the receipt card (§4), only when the receipt was the
  result. A receipt delivered beside a run that did happen draws one dim line, not a card.
- **Control:** never (only treated sessions hold receipts, as today).
- **Headless:** runs.

### 3.4 Rewrites checked by the engine [Q4]

Today the U11 planner reads the settings files itself to decide whether `.venv/bin/python -m pytest`
would still be treated as `python -m pytest` is (`pretool::keeps_permissions`: a rewrite that
would change the permission outcome becomes advice), and `--allowedTools` is invisible to it.
`$.tool.check({ tool, input })` "resolves to the decision the rules, the permission mode, and
those hooks reached" and "executes nothing".

```ts
async function rewrite($, e, next, base, plan) {
  const to = plan.rewrite?.command
  if (to === undefined) return next(e)                               // advice only, or nothing: the command hook delivers it
  const [a, b] = await Promise.all([$.tool.check({ tool: 'Bash', input: { command: e.command } }),
                                    $.tool.check({ tool: 'Bash', input: { command: to } })])
  const verdict = a.decision === b.decision ? 'same' : 'differs'
  const final = await ask($, 'PreToolUse', { ...base, verdict: { engine: verdict } }, 2000)
  const r = final.ok ? final.answer : undefined
  const command = r?.hookSpecificOutput?.updatedInput?.command
  if (command === undefined) return next(e)                          // the binary downgraded to advice; its context rides with the command hook
  $.ui.notice(e.toolUseId, `↪ ${stripPrefix(r.systemMessage)}`)      // one line under the permission dialog, if one is shown
  await $.state.update(s => ({ ...s, rewrites: [...s.rewrites, { id: e.toolUseId, from: e.command, to: command }] }))
  return next({ ...e, command })
}
```

**The dead end that never runs.** The same discipline applies to a learned `avoid` pair. The
binary's `dry_run` plan may answer `substitute: { command: "make test", reason }` for a call
whose canonical key meets an `avoid` reflex at `fire` with a learned `instead`; the mod checks
both commands with `$.tool.check` and substitutes only on the same verdict, with one line of
context (`adage ran make test instead: pytest is not installed here`). The failure, the model's
reading of it and the recovery request do not happen. A substitution is recorded as its own
judgement (`Substituted`), never as a follow; in the control arm it is withheld and
`would_substitute` is spooled.

A dead end at the head of a pipeline is replaced the same way when every later stage only filters
its output (`tail`, `head`, `grep`, `wc`, `cat`, `tee`, `sort`, `uniq`, with their arguments): the
first stage is replaced, its own `2>&1` and the filters are kept byte for byte, and the plan's
`command` is the whole new pipeline (`python3 -m unittest discover -s tests 2>&1 | tail -5` →
`make test 2>&1 | tail -5`). The mod asks `$.tool.check` about both whole pipelines, and the
context line names the stage replaced. Nothing else is rewritten: a list, any other redirection,
a subshell or an expansion also runs or feeds something the replacement would change.

- **Binary:** with `verdict.engine` present, the engine's comparison replaces the planner's own
  settings reading for this decision; the planner's reading stays as the fallback for hosts
  without a mod. The spooled `pretool` record is written once, at the final call, and says which
  check decided (`checked_by: engine | planner`), so a downgrade is recorded as a downgrade.
- **Model:** as today (`additionalContext` names the word that changed). **Person:** the notice
  under the dialog and the `↪` on the tool row. **Control:** never, as today. **Headless:** runs.

### 3.5 Route the step [Q11]

`next({ ...e, model })` or `effort` "sends another". Where the binary marks a step **routine** (a
`fire` reflex is pending for this step and holds no `predict` grant, so the model will be asked
to do what the directive already says), the step can go to a small model at low effort:

```ts
if (asked.ok && asked.answer?.routine && options.routine_model !== '') return yield* next({ ...e, model: options.routine_model, effort: 'low' })
```

`agent.spawn` has the same rewrite for subagents (`{ model }`). Off by default (`routine_model`
empty): the person's setting is the consent for a changed model request, as the `predict` grant
is for a changed tool call, and the binary answers `routine` only when the mod's payload says a
routine model is configured. The quality rule that kept execution mode experimental (hidden tests
17/20 against 19/20) is the gate, and Q11 reports numbers, not a verdict. **Control:** `routine`
is never set; `would_route` is spooled.

### 3.6 Standing text once, from the cache [Q2]

Each directive is delivered once per session already (`emitted` in the runtime), so there is no
duplicate injection to remove. What a `prompt.compose` section changes is *placement*: the
standing text Adage has for a repository (the applicable procedure cards' titles, the instincts'
one-liners, the environment note) can sit in a `session`-scoped section at the end of the system
prompt, where the prompt cache keeps it, instead of arriving as `additionalContext` at the first
event that needs it. `prompt.compose` returns "`{ sections }`, a list of `{ id, text, scope }` in
the order they're sent"; text that does not change within a session is served from the cache.
`tool.describe` fires "once for each tool, when its description is first sent to Claude" and is
cached for the session.

```ts
on('prompt.compose', async ($, e, next) => {
  const { sections } = await next(e)
  const standing = active ? (await $.state.get()).standing : ''     // from SessionStart's answer, fixed for the session
  return standing === '' ? { sections } : { sections: [...sections, { id: 'adage:procedures', text: standing, scope: 'session' }] }
})
on('tool.describe', { tool: 'Bash' }, async ($, e, next) => {
  const d = await next(e)
  const envNote = active ? (await $.state.get()).environment : ''   // "tests run under .venv/bin/python here", when U11 is bound
  return envNote === '' ? d : { ...d, description: `${d.description}\n\n${envNote}` }
})
```

- **Binary:** a read-only event, `<bin> hook claude Standing`, asked once by the mod after
  `session.start` (the command hooks' `SessionStart` is not re-run: it records the start and may
  spawn the learner). It answers `{ standing, environment }`: the standing text is the
  repository's applicable procedure cards' titles and the instincts' one-liners in the fixed
  templates `render.rs` already uses, ≤ 2,000 characters, identical across the session; the
  environment note is the U11 binding's sentence when one is bound. It records `has_standing` on
  the session state so the per-event delivery of the same text is suppressed for the session.
  Today's per-event channel carries directives, which are situational and stay as they are (a
  lookup at this prompt, a dead end at this failure). `$.ui.invalidate('prompt.section')` is never
  called mid-session, so the cache is never spent. Q2 measures the token difference; the claim is
  cache placement, not fewer tokens per request.
- **Control:** `standing` and `environment` are empty.

### 3.7 Survive compaction [Q5]

`session.compact` hands the hook `messages` and `instructions`. Adage's text rides on the
person's prompt row and on tool-result rows as hook attachments, so a text filter would drop the
person's own rows. The mod removes nothing unless Q5 shows the attachment is addressable on its
own (`origin.kind === 'plugin'` or the hook-attachment type); until then it only adds the
instruction that keeps the procedures the session followed:

```ts
on('session.compact', async ($, e, next) => next({ ...e,
  messages: isAddressable ? e.messages.filter(m => !isAdageAttachment(m)) : e.messages,   // structural, never by text
  instructions: [e.instructions, 'Keep the repository procedures that were followed (how tests run, what regenerates after a schema edit).'].filter(Boolean).join('\n') }))
```

**Control:** the instruction is added in both arms (it names no directive).

### 3.8 Subagents

`tool.call` "fires … including calls a subagent makes", with `e.agentId`; `turn.step` carries it
too. An Explore agent that re-runs the lookup the main loop already did is the same waste one loop
down. The receipt, rewrite and substitution paths above apply unchanged; predictions (§3.2) stay
on the main loop (`e.agentId === undefined`) until a subagent study says otherwise. `agent.spawn`
lets the mod ask the binary for the standing text that applies to the delegation (`Standing`
with the subagent's description) and prepend it to the subagent's prompt as a marked
`<adage-reflex>` block, so the subagent is told the convention before its first request; a
subagent's calls are recorded with their `agent` on the canonical `Call`, so the learner can
count a delegation's waste on its own.

### 3.9 Measure and attribute with the engine's own figures [Q5]

- **Cost from the harness.** Every `turn.step` result carries `usage` (`input_tokens`,
  `output_tokens`, `cache_read_input_tokens`, `cache_creation_input_tokens`, `model`), and
  `$.session.usage()` returns `{ startedAt, context, rateLimits, cost }`. At `turn.complete` the
  mod writes one line through the binary, `adage ledger note` (plan A4): per-step usage, the
  steps Adage took in the model's place with the model each would have used, the hook layer's
  wall times, and the engine's `cost` where the host keeps one. The line goes to a file of its
  own (`ledger/harness.jsonl`) that the learner merges by session id, under the ledger's lock,
  when it writes the ledger, so the ledger's one-line-per-trace schema and its `.seen.json` dedupe
  are untouched; the merged line's `cost.source` is `harness` for Claude Code sessions, in both
  arms. Because the command is allowed inside a session, the binary accepts a note only for a
  session id whose spool holds a `session_start` in this repository, keeps one line per session,
  and never lets a transcript-priced line replace a harness line or the reverse; the agent cannot
  mint a session it did not have.
- **Latency on every machine.** `duration_us` on every spool record (plan A2) is the binary's own
  time; the mod's `hookMs` is the engine's dispatch time around `next(e)`. `adage doctor` gets the
  p95 check zero-config P1 lists from the first, measured where the person works.
- **The harness version.** `$.session.version()` answers `{ version, base, builtAt }`; recorded
  once per session in the ledger line, `doctor` says when the harness moved past the version the
  plugin was validated against (R7).
- **Provenance in the row.** A predicted call's `tool_use_id` carries the `adage_` prefix and the
  route. The Claude adapter reads `tool_use_id` on `PreToolUse`, `PostToolBatch` and
  `PostToolUseFailure` (today `call_from` drops it) and sets `Call.by = By::Adage`; the runtime
  judges such a call `Judgement::Predicted { route }`, never `Followed`, so a reflex cannot
  strengthen itself on its own predictions (plan §2.2.4, §2.1.3). Downstream, `Predicted` is not a
  follow (it is excluded from follow rates and from strength evidence), not extinction evidence,
  counted by the savings estimate as a model turn not spent, and drawn by the feed as a "chose"
  row. If Q5 finds the row's `origin` on disk, the learner reads that too and keeps the
  `<adage-reflex>` marker as the cross-harness fallback.
- **The learner's trigger.** Unchanged: the `Stop` and `SessionEnd` command hooks start the
  detached learner. The mod's `session.end` hook only flushes the ledger line; "all `session.end`
  hooks together" share 1.5 s, so nothing waits there.

### 3.10 Noticed: learning the person can watch [Q12]

Adage learns after the session, from the trace, and the person hears about it a day later in one
line. The mod can show the learning while it happens, at three moments, without turning any of
them into a directive.

**The pattern Adage has seen before.** Today nothing below a reflex leaves a trace in a session:
a **nascent** reflex (environment steps waiting for review) is filtered out of the index at
compile (`RouteState::is_live`), and a **candidate** the miner has counted but that has not
cleared `min_support` (five unsteered sessions by default) is thrown away by the judge. The design
adds a `candidates` table beside the index, mined in a **pass of its own** (plan D1): the miner
today drops every label below `min_support` from the alphabet and every pattern below it from the
result, and the alphabet is what `gap` and `within` count, so lowering the routes' floor would
change the meaning of reflexes already in service. The candidate pass builds a second alphabet
with the floor at `s_min − border`, keeps only the border patterns (support in
`[s_min − border, s_min)`) with a `Context` trigger of two or more events, and never touches the
routes' result. Candidates get a context net of their own for the same reason: the routes' net
digest is the alphabet's hash and a changed digest resets every session's context state, and
candidates change at nearly every learn. A candidate carries the stable id the same pattern gets
as a route, so the day it crosses `s_min` the person sees one name. Their words join the fast
path's `Triggers` so no event that could notice is skipped, and a repository with candidates but
no routes no longer takes the no-routes early exit. At `PostToolBatch` the binary walks the candidate net as it walks
the routes', records `noticed` on the decision (candidate id, support so far, whether it waits
for review), and prints nothing to the model. The mod reads it off the feed and draws one line
under the batch:

```text
 ● Edit(acme/invoice.py)
 ● Bash(make test)
   ○ adage noticed · make test after editing acme/** · seen in 3 earlier sessions
```

```text
   ○ adage noticed · scripts/wi-lookup then reading the files it names · waits for your review · /adage review
```

The rules that keep it from becoming noise: only sequences (a context trigger of two or more
events, or a prompt family followed by its command), never a single call; once per candidate per
session; nothing for a pattern that is already a live reflex (that is a directive, §3.1); plain
counts, never a forecast (forming needs the Wilson bound too, so "a reflex forms at 5" is not a
fact). Both arms record and draw it, because a note about learning changes neither the model's
path nor the person's view of which arm they are in. The common no-match path in a repository
with routes (1.9–2.0 ms p50, Architecture §9) is bounded at +0.3 ms p95 (plan D1).

**The learner, live.** The feed (§2) carries `learn.log` lines as they are written, and the
learner's state file says when the next run starts (`<slug>.spawn.json`, `after`). The band's
last cell counts down to it (`learning in 38 s`), turns to `learning…` while the lock is held, and
when the line lands the mod raises one toast with the change that matters most, in plain words
(no strength or lifecycle word on a default surface, zero-config §0):

```text
 adage learned from this session
 ◠ codegen-after-schema is now said as a direct instruction (9 of 9 sessions)
```

**The reflex that just formed from what you did.** When a `noticed` candidate reaches support in
the learn that follows this session, the toast says so and names the session as the one that
tipped it: `◠ test-after-acme-edit is now a reflex · this session was its 5th`; a candidate that
became a nascent reflex says `· now waits for your review`. That is the moment the product's
promise ("your agent keeps relearning the same lessons; Adage remembers them") becomes something
the person has watched happen.

- **Binary:** the `candidates` table and net, the `noticed` field on the decision, the feed's
  `learn` and `learner` records. The "familiar-command computation" F14 lists as not built is the
  same count by another name (sessions and days a command has run in), so one implementation
  serves both.
- **Model:** nothing. **Control:** recorded and drawn in both arms. **Hosts:** drawn where the
  mod draws; in `-p` the record is spooled and nothing else.

### 3.11 Skills: select them deterministically, and verify them where they run [Q13]

Skill selection is the model's judgment today: it reads the skill list in the prompt and decides.
Adage's compiler (`adage train SKILL.md`) turns a skill's commands into verified reflexes, and
`training.md` lists what stays text: judgment, and "a command with no stimulus". F14 lists "skill
triggers" as not built. The mods API gives three handles on skills, and together they make a new
response kind, **`skill`**: stimulus → *use this skill now*.

| Handle | Documented shape | What Adage does with it |
| --- | --- | --- |
| `turn.step`, yielding a `Skill` tool call | The engine's Skill tool takes `{ skill: string, args?: string }`; a yielded `tool` + `input` + `stop` chunk is recorded and run (§3.2) | **Invoke.** At the step where a skill reflex with a `predict` grant matches, the mod yields `Skill { skill: 'release-checklist' }` and no request is sent. The engine runs the Skill tool; the skill's text enters the transcript as the tool's result; the model's next step proceeds with the skill loaded, at the moment the pattern says, every time |
| `skill.prompt` | "Fires when the engine expands a skill's prompt for the model (`/name`, the Skill tool, a preload)"; `e` is `{ skill, text }`; return `{ text }` | **Verify in place.** For a skill Adage has trained in this repository, the mod rewrites the text as the model reads it: under a fixed heading, one plain line per step the replay verified (`make test passes; the lookup is scripts/wi-lookup {id}`), the correction for a step the replay found stale, and the environment Adage bound (`.venv/bin/python`) for a bare `python`. The skill file is untouched; the model reads the repository's truth |
| `skill.prompt` as an observation | The same event, every invocation, with the skill's name | **Learn selection.** Each invocation is spooled with what preceded it, so the learner can form a `skill` reflex from how skills are actually used here: "in sessions whose prompt looks like *cut a release*, the release-checklist skill was invoked after the version bump, 7 of 8 times". Transcripts record Skill tool uses too, so history before the mod counts |
| `$.agent.register({ skills })`, `agent.spawn` | An agent definition's `skills` are "preloaded into the agent's context before its first turn"; a spawn's `prompt` is rewritable | **Equip subagents.** A skill reflex whose stimulus is a delegation (the `Agent` call's description) prepends a marked block, `<adage-reflex kind="skill">…</adage-reflex>`, to the subagent's prompt, never bare text |
| `tool.describe` on `Skill` | The Skill tool's description, cached for the session | **Advise, cached.** The standing list of skills that apply in this repository and when (from the `skill` reflexes), once, in the tool's own description |

Three delivery levels, like every other reflex, so the same `skill` reflex works on every harness:

1. **Advise** (every harness, today's channel): `<adage-reflex kind="skill">` in `additionalContext`
   at the stimulus: "In this repository the release-checklist skill is used at this point. Ignore
   this if the user is asking for something else." The model still chooses.
2. **Invoke** (Claude Code with the mod): the `turn.step` yield above, **under the same `predict`
   grant as any predicted call** and the same three bounds. A skill is not inert text: a SKILL.md
   may hold dynamic-context blocks the engine executes when the skill expands, and a
   `context: fork` skill runs as a subagent, so invoking one is an action a person must grant.
   `adage train` records whether the skill it compiled is free of both (`verified.json`,
   `inert: true`, plan D4), and the binary never predicts a skill it has not recorded as inert in
   a pack the person has accepted. A `skill` route's strength ceiling is **prime** (its effect is
   not Observe in any useful sense, since the text it loads can carry commands), so it is never
   said as a direct instruction without the grant and the recorded inertness. The machine switch
   `skills_invoke` (§5) is off by default until Q13 reports. The model's next step can still
   ignore the skill; what is deterministic is *that it was offered, in full, at that moment*,
   which is the part that varied before.
3. **Verify in place** (Claude Code with the mod): the `skill.prompt` rewrite, for any skill
   `adage train` has compiled here, at every invocation however it was triggered.

What the person sees: the invoked skill's tool row carries `◈ adage loaded this skill: <reflex> ·
no model request`; a verified skill's row carries `◈ adage verified 3 of 5 steps here · step 2
corrected`. (`◈` is the style guide's "from a skill" glyph; it marks every skill line in this
design.) In the control arm nothing is invoked or rewritten and `would_predict` / `would_annotate`
are spooled.

- **Binary:** the `skill` response kind in tracks and the index (stimulus: intent family,
  after-edit glob, context trigger or delegation; response: a skill name and optional args); the
  learner's rule for forming one from Skill tool uses in traces; `adage train` recording per-step
  verification and inertness so `skill.prompt` and `TurnStep` have something to read; the
  `SkillPrompt` event (`<bin> hook claude SkillPrompt`, payload `{ session_id, cwd, skill, text }`,
  answer `{ text }` or `{}`). **Model:** at level 1 a plain block; at level 2 the skill's own text
  as a tool result; at level 3 the skill's text with plain, factual lines added. **Hosts:** level 1
  everywhere; levels 2 and 3 wherever hooks run, `-p` included, which is the first place to
  measure them.

### 3.12 Ask the person, in the line of work [Q14]

Every decision Adage needs from a person (enable a reflex that waits for review, strengthen or
dismiss one, grant execution or prediction, turn delegation on, use a team's signed reflexes,
turn Adage back on) is a terminal command today, and persona (a) never opens a terminal. The mod
can ask in the session, in the dialog Claude Code itself uses to ask a question, and act on the
answer. What the press is, exactly, matters: it is **not** a security boundary against an agent
that runs as the same user and has a shell (nothing in Adage is: the approval store is unsigned
by design, Track C D8, and `env -u CLAUDECODE ADAGE_ALLOW_NONINTERACTIVE=1 adage review approve`
passes today's guard). It is two things: a reliable record that a person, not the model, made
the decision (`via: press`), and a guard against *steering*, of the same class as F11: a model
that has been talked into enabling its own reflex cannot press a button, because it can only
call tools, and a Bash command it is talked into running gets no token.

**The press token.** A per-session capability the binary issues once and the mod holds in engine
memory.

- *Issue.* The mod asks `adage hook claude Press` from `classic.SessionStart`, **after**
  `await next(e)` (so the command hooks' session start has run) and with that payload's
  `session_id` and `source`. `Press` is a dedicated event that does nothing else: it runs before
  the kill switches (`.adage/disabled`, `ADAGE_DISABLE`), so a session in a repository where
  Adage is off can still be asked to turn it on; it records nothing to the model. It answers
  `{ press }`, 32 random bytes, only when the session's `press` state is open and no token has
  been issued for this start; it stores the SHA-256 and `issued: 1`. `source: startup` and
  `source: clear` (a new session id) issue; `source: resume` reissues once and replaces the hash;
  `source: compact`, which fires mid-turn, never issues.
- *Close.* The window is a state field, not an inference from records. `handle()` closes it on
  the first `UserPromptSubmit`, `PreToolUse` or `PostToolBatch` invocation for the session,
  before the fast path and before any record is or is not written (one small state write, once).
  A `Press` call after that returns nothing. So the only time a token can be claimed is between
  the hooks' session start and the first thing the agent does.
- *Hold.* The token lives in `$.state`, engine memory. The spool gets a `press_issued` record
  with no secret, and the mod counts: an issue it did not ask for means something else claimed
  the slot, and presses switch off for the session with a status warning (the tamper signal).
- *Spend.* A person-only verb run from the mod reads `{ "press", "session" }` on **stdin** (never
  argv, which `ps` shows) and, on success, prints the next token: tokens are single-use and
  chained, so a captured one is already spent. The binary clears the hash at `SessionEnd` and
  refuses a token older than 24 h, and refuses a verb whose repository is not the session's.
- *Check.* A new `require_person_or_press(cmd, press)` beside `require_person`, called **only**
  at the sites below. It passes when the token's hash equals the session's and the chain index
  matches, with any `AGENT_ENV` set and no terminal on stdin; it refuses when
  `ADAGE_ALLOW_NONINTERACTIVE` is also set (a press and a CI override never travel together).
  `require_person` itself is untouched, so `install`, guarded `config set`, `add` without a
  pack ask, `share`, `train accept`, `packs on`, `init` and every `adage-exec` verb (`approve`,
  `revoke`, `recover`) stay as they are.
- *Tripwire.* The binary's `PreToolUse` records and warns, once, when a Bash command names
  `hook claude Press` or carries a press on its stdin heredoc; it never blocks it (advice, never
  a refusal).

**Where a press is accepted.** `review strengthen|weaken|extinguish|approve|pin|dismiss`,
`review revoke`, `review delegate` (both levels), `delegate on` (under a press it writes the
marker only, and says which Codex, OpenCode or settings-file registrations still need a
terminal), `delegate policy --max-level e2`, `delegate predict on`, `on`, and `add <pack-dir>
--trust` for a pack committed in the repository. Turn-downs (`delegate off`, `policy
--max-level e1`, `predict off`, `off`) need no press, as today.

**How Adage asks.** At `turn.complete`, when the model is idle and the feed's learn line or
`graph --json` shows a decision waiting, the mod calls `$.ui.ask(question, labels)`: the engine's
own numbered dialog, the one Claude uses to ask the person something. Never inside a hook with a
budget, never mid-turn, at most one ask per turn end, one ask per reflex per day. The person
answers with the dialog's own digits or dismisses it; a dismissed ask returns at most once more,
a day later, and is always listed under **waiting for you** in the pane. "Not this one" records
`dismiss`, so it never returns. Asks of different weight ask differently:

| Decision | Today | The ask | Weight |
| --- | --- | --- | --- |
| Enable a reflex that waits for review (nascent), dismiss it, strengthen, weaken, pin | `adage review <verb> …` | `$.ui.ask('test-after-acme-edit waits for your review', ['enable', 'not this one', 'why'])` | one answer |
| Grant execution (E1 or E2) for one reflex | `adage review delegate … --level e2 --outputs …`, which stages once and prints the diff | `$.ui.ask('codegen-after-schema can run make gen for you, sandboxed', ['see what it would do', 'not now'])`; the first answer opens a **focused** pane with everything the terminal prints today (argv, the terms the binary proposes, scripts and their text, sandbox, hashes, the staged diff as `Code`) and two `Button`s, `a allow` and `n not now`; `allow` confirms | two acts, the facts between |
| Grant prediction for one reflex | `adage review delegate … --predict` | `$.ui.ask('codegen-after-schema can run its step without asking the model', ['allow', 'not now', 'why'])`; `why` opens the pane with what will and will not meet a permission dialog | one answer, or two |
| Turn execution on for this machine | `adage delegate policy --max-level e2`, `adage delegate on`, a second plugin | Folded into the first execution grant: the pane's first line says so, and `allow` writes the policy, the marker and the grant together; "takes effect in new sessions" is said | two acts |
| Use a team's signed reflexes | `adage add <pack-dir> --trust` | `$.ui.ask('6 team reflexes are in this repository, signed by alice@acme.com', ['use them', 'not now', 'why'])`; `why` shows the key's fingerprint, the pack list, and the commit that added them with its author and date; the same trust as `CLAUDE.md`, said in the pane | two acts |
| Turn Adage back on in this repository | `adage on` | `$.ui.ask('adage is off in this repository', ['turn it on', 'leave it off'])`, opened by `/adage on` | one answer |

A press is a `$.ui.ask` answer or a focused `Button`'s `onPress`; a `command.run` handler never
runs a person-only verb (`/adage on` opens the ask). Nothing in the ask or the pane is a strength
or lifecycle word. The terminal command stays as an equal path for persona (b), shown dim in
the pane with `Copy`.

**Stage, then confirm.** An execution grant is two acts because the terminal path already
stages the action once before recording anything. `review delegate … --stage --json` (no
record) answers the terms the binary proposes (E2: the outputs and caches observed on the
route's own evidence, the effect; E1: argv, the input contracts from the prompt slots, the
output mode) and the facts (argv, scripts and their full text, sandbox profile, runtime hashes,
attestations, the staged diff per declared output with before and after hashes), under a
`digest = H(approval key ‖ terms ‖ input snapshot)`. Staging may use a trial E2 policy when the
machine is at E1, for the trial store only. `--confirm` reads `{ expect: digest, press,
session }` on stdin, **re-plans and re-stages**, and refuses if the key or the terms changed; if
only the snapshot changed (the agent kept working), it answers the fresh diff and the pane shows
it again before `allow`. Nothing is persisted between the two acts. On the first execution grant
the confirm writes `policy max_level = e2`, the `exec/enabled` marker and the grant together.

**One plugin.** Track C's decision D2 chose a second plugin, `adage-exec`, because installing it
was "explicit consent in the harness's own UI", and listed the alternative: one plugin whose
hooks call `adage-exec` only when a marker that `adage delegate on` writes exists. With consent
now a press in the session, the marker is the explicit act and the second plugin is a second
install step with no purpose. The design therefore ships **both binaries in the one plugin**:
the shim links `$DATA/bin/adage` and `$DATA/bin/adage-exec` (copies on Windows), links
`$DATA/bin/hook` to whichever serves (`adage-exec` when `$ADAGE_HOME/exec/enabled` exists, else
`adage`; re-made at every start, and the shim itself execs it), and the command hooks call
`$DATA/bin/hook` with the exec plugin's timeouts (30 s on `PostToolBatch` and `Stop`, which the
Stop deadline assumes). Hooks are never registered twice because there is one plugin; the
promise Track C wanted checkable (no `Command::new` in the `adage` crates) is unchanged because
the binaries stay separate; the exec shim's `PATH` and npm fallbacks carry over. `delegate on`
keeps rewriting settings-file, Codex and OpenCode registrations as today (that path is
`install`'s and needs a terminal); only its plugin message changes. `plugin/claude-code-exec`
stays for one release as an alias whose `hooks.json` registers `SessionStart` alone and whose
shim relinks `bin/adage-exec` to `scripts/quiet`, so a stale link from a previous version can
never make hooks fire twice. The marker is read when the shim runs, so a grant takes effect in
new sessions, and the design says so. This reverses D2 and the plan carries it as A8.

**The web map.** `adage map` serves a read-only page on localhost (GET and HEAD only, the host
checked). Every tab of the pane ends with one `Explore in browser` link, which starts `adage map
--no-open` on a free port if it is not running (a child like the feed, ended with the module) and
opens a `Link` to `http://localhost:<port>`. The map stays read-only: a browser page cannot make
a decision, since any page on the machine could send the same request; decisions stay in the
dialog and the pane, where a press is a person's.

- **Binary:** the `Press` event and the window field; `press_issued` records; `require_person_or_press`
  at the listed sites, reading stdin; chained single-use tokens; `review delegate --stage --json`
  and `--confirm`; proposed terms from route evidence; `via: press | terminal` on the review
  judgement and on `Approval` (the existing `--by` is a person's name and is untouched); the
  tripwire; `graph --json` gains `offers` (which reflexes qualify for an execution or prediction
  ask: an after-edit or context route at `fire` with a Verify or Regenerate effect, policy
  permitting, no grant, not exec-only) and `status --json` gains the committed packs awaiting
  trust. **Model:** nothing. **Control:** unaffected; the token is issued in both arms.
  **Hosts:** the ask draws where the mod draws; headless sessions issue no token and ask
  nothing.

## 4. Looks: every render site, and what Adage draws there

`ui.render` fires for each render site; a hook can "change a detail" (`next({ ...e, props })`),
"replace the drawing" (return a tree, no `next`) or "leave it alone" (`next(e)`), and `next(e)`
"returns a reference to Claude Code's drawing, `{ type: 'engine', ref }`" that a tree may place
"in a `Box` beside elements of your own". The permission prompt is not a render site. The strings
and states are specified in [ux-design.md](ux-design.md); this table says where and how.

| Site | Adage draws | When | How |
| --- | --- | --- | --- |
| `InfoNotice` (terminal) | The engine's own notice for the hook's `systemMessage` (the first-session line, or the day's notice), redrawn with the glyph: `◠ adage · 3 reflexes and 4 instincts active in this repo · …` | Whenever the binary's notice gate printed one (first session, then at most daily) | `next({ ...e, props })` on the engine's notice; no second line, no new frequency |
| `UserMessage` | One dim line under the prompt per reflex fired at it, worded as what it tells the agent: `◠ look up the work item · scripts/wi-lookup W-2042 --json` | A prompt-triggered reflex | Keyed by order: the nth `UserPromptSubmit` result ↔ the nth `UserMessage` (the payload carries no message id); `Box` column: `await next(e)` first, then the lines; `e.props.isExpanded` returns `next(e)` alone |
| `ToolUse` | A marker line under the row: `↪ adage: pointed this test run at .venv/bin/python` for a rewrite; `◠ adage chose this step · no model request` for a predicted call; `↪ adage ran make test instead · pytest is not installed here` for a substitution; `◠ dead end headed off` for a followed `avoid` | Rewrites and predictions matched by `e.requestId` (the `tool_use_id`) against `$.state`; the avoid marker from the feed's judgement record for that call | `Box` with the engine's row then one `Text dimColor` |
| `ToolResult` | The receipt card in place of pytest's output: receipt id, result, duration, the tests named, the session's own tests first, and the one line saying the output is not shown | A receipt answered as the result (§3.3) only; beside a run that happened, one dim line and `next(e)` | Return a tree, no `next`; `Markdown` for the test list |
| `ToolGroup` or the last `ToolUse` of a batch | `○ adage noticed · <sequence> · seen in 3 earlier sessions`, or `· waits for your review · /adage review` | A `noticed` record for this batch (§3.10), once per candidate per session | `Box` with the engine's row then one `Text dimColor` |
| `ToolUse` for `Skill` | `◈ adage loaded this skill: <reflex> · no model request`; `◈ adage verified 3 of 5 steps here · step 2 corrected` | A skill reflex invoked or a trained skill expanded (§3.11) | As the other `ToolUse` markers |
| `Spinner` | `Thinking · adage ◠ 1…` while a reflex was delivered this turn | `e.props.suffix` | `next({ ...e, props: { ...e.props, suffix } })` |
| `TurnDuration` (terminal) | `Worked for 1m 48s · adage: 1 step without a model request · 1 dead end headed off` | A turn with Adage activity | `next({ ...e, props: { ...e.props, word } })` or a `Box` beside the reference |
| `AbovePrompt` | One line, like `token-weather`: `◠ adage  2 reflexes this session · 1 step without a model request · ● healthy · learning in 38 s`; a second line while something needs attention | When any cell has something to say; `next(e)` otherwise so other mods' bands show | Sized to `e.props.bodyColumns` |
| `Pane` `adage` | The watch pane: tabs **Live**, **Reflexes**, **Health** (§4.1) | `/adage`, `/adage watch`, or unasked only when the terminal is 144 columns wide and `pane: "auto"` | Fed by `feed.ts`; rows as `plain` Buttons so Enter inspects |
| `CommandOutput` | `/adage` and `/adage why` draw the status card and the last directive's evidence as a tree, not as text | `{ component: 'CommandOutput', props: { command: 'adage' } }` | `Markdown` and `Code` |
| `AskUserQuestion` | Untouched | | |
| `$.ui.toast` | The learner's change, a lapsed grant, the finish line continuing | Rare, important | `holdToasts` on the pane keeps them until it closes |
| `$.ui.status` | Problems only; the engine prefixes it `⚠ adage:`, so the text carries no prefix of its own | | |
| `$.ui.log` | Nothing by default; dim lines under `verbose: true` | | Starts with `●` and the mod's name, by the engine |

Rules that hold at every site:

- **Theme keys, not hex.** `Text` colour "is a theme key or a color"; the style guide says the
  terminal's own colours win, and in a pane the engine's theme keys are that rule. Adage's accent
  and the lamp roles come from `hooks/tokens.ts`, generated by `cargo xtask tokens` beside
  `tokens.rs`; no colour literal in the module. Under the Desktop app the same keys map to its
  theme.
- **The style guide's glyphs** (`◠` reflex, `◉` instinct, `↪` rewrite, `●`/`○` lamps, `»` notice,
  `◈` from a skill), each one column wide; the ASCII twins under `ADAGE_ASCII=1` read through
  `$.env.get`.
- **No digit hotkeys.** A digit hotkey on the band "also fires when the user types that digit
  alone into an empty prompt and pauses"; Adage's band has no buttons.
- **Quiet in control, invisible when nothing fired, nothing headless.** `register` runs before
  `session.start`, so the `ui.render` hooks are registered everywhere and return `next(e)` while
  `active` is false or `isInteractive` was false; nothing is drawn per event when the record is
  empty.
- **Every tree is a pure function of `$.state`**, so `claude plugin test`'s `ui.mount` draws it on
  `terminal` and `desktop` from fixture state, as the Rust frames are drawn from fixtures today.

### 4.1 The pane

`/adage` opens the pane with `focus: true, closeOnEscape: true, holdToasts: true, rows: 20`. Three
tabs, as three `plain` Buttons with lowercase hotkeys (a `Button` hotkey is one digit or one
lowercase letter):

- **Live**: the `adage watch` feed (now, conditioning, health) for this repository, one `Text` per
  row truncated to `bodyColumns`, an `Input` filter, Enter on a row opens a second pane
  `adage-inspect` (`focus`, `closeOnEscape`, `rows: 12`) with the directive text and evidence as
  `Markdown`. Arrows and page keys scroll (the engine's window). The feed is asked with
  `--session <id>` and **omits, for the current session, every record that would name the arm**:
  the `control` flag, `would_have`, `would_predict`, `would_continue`, withheld pre-tool records
  and the `Unaided`/`NotReached` judgements. Other sessions' rows are shown as `adage watch`
  shows them.
- **Reflexes**: a **waiting for you** group first (every pending ask, §3.12), then every reflex
  by track from `adage graph --json`: strength, lifecycle, trigger and step, evidence, the
  directive text. An expanded row carries one `Button` per verb the graph's `review.verbs` lists,
  worded in plain words, each pressing through the session's press token (§3.12), and the
  terminal command in `review_command`'s exact form, dim, with `Copy` for persona (b).
- **Health**: `adage doctor --json`, lamps and the fixing command; `Enter` runs only what the
  binary allows from a session (`condition now`); the rest copies the command.
- A `Link` to the running map, `http://localhost:<port>`, when `adage map` is up.

The feed child, `adage watch --follow --json --session <id>` (plan A3: the same `tail`, `learnlog`
and `health` modules as the terminal pane, one JSON line per change; every line carries an opaque
`cursor`, the spool and log positions, and `--since <cursor>` resumes a new child from it, since a
per-run sequence number means nothing to a new process), is started once from `session.start`
when the mod is active and the session is interactive:

```ts
function startFeed($: EngineInterface, bin: string, e: SessionStartEvent) {
  if (!e.isInteractive) return
  void (async () => {
    let since = '', failures = 0
    while (failures < 3) {
      const feed = $.process.spawn({ argv: [bin, 'watch', '--follow', '--json', '--repo', e.cwd, '--session', e.sessionId, ...(since ? ['--since', since] : [])],
                                     env: { CLAUDECODE: '1' } })
      let buffer = ''
      for await (const chunk of feed) {
        if (!('text' in chunk)) continue
        buffer += chunk.text
        const lines = buffer.split('\n'); buffer = lines.pop() ?? ''      // a chunk may end mid-line
        for (const line of lines) {
          const row = parseRow(line); if (row === undefined) continue       // one bad line is skipped, never fatal
          since = row.cursor
          await $.state.update(s => ({ ...s, rows: [...s.rows, row].slice(-500) }))
        }
      }
      failures += 1                                                         // the child ended: restart from `since`
    }
    await $.state.update(s => ({ ...s, feed: 'stopped' }))
  })()                                                                      // the loop is the child's life; it ends with the module
}
```

## 5. Commands and settings

`$.command.register` in `session.start`, when the mod is active; `command.run` answers each.
Names that the lexicon's verbs already use, under one prefix so the typeahead groups them:

| Command | Does | Allowed from a session because |
| --- | --- | --- |
| `/adage` | The status card, drawn (`CommandOutput`) | Read-only |
| `/adage watch` | Opens the pane on Live | UI |
| `/adage reflexes`, `/adage health` | Opens the pane on that tab | UI |
| `/adage why` | The last directive this session: its text, route, evidence, and whether it was followed; in a control session, the same text a treated session with nothing fired prints | Read-only (spool) |
| `/adage off` | Runs `adage off`, which writes `.adage/disabled` for this repository (a new command, plan A6) | "Turns something down", allowed anywhere (live-learning.md) |
| `/adage on` | Opens the ask (`$.ui.ask`); the answer runs `adage on` with the press on stdin (§3.12). A command handler never runs a person-only verb itself | The press token |
| `/adage review` | Opens the pane on **waiting for you** (§3.12) | UI; each answer is a press |
| `/adage map` | `Explore in browser`: starts `adage map --no-open` on a free port if needed and opens the link | Read-only |
| `/adage pause` | Pauses the feed and the band for this session; the band shows `paused` | UI |

Settings go in the manifest's `userConfig`, which Claude Code draws as `/config` rows and stores
under `pluginConfigs`; a change reloads the mod with the new `options`:

```json
"userConfig": {
  "band": { "type": "boolean", "title": "Band above the prompt", "description": "One line: reflexes this session, steps without a model request, health, learning", "default": true },
  "pane": { "type": "string",  "title": "Watch pane", "description": "Open the live pane unasked in a wide terminal", "options": ["auto", "manual"], "default": "manual" }
}
```

Two rows, because a person who installed a plugin should not have to understand Adage to
configure it. Everything else the mod can be tuned with (`ui.noticed`, `skills.invoke`,
`runtime.routine_model`, `runtime.predict_cap`, `ui.notices`) is a `config.toml` key set with
`adage config`, for people who go digging. Execution policy, grants, trust and the
learning sources are decisions, not settings: they are made by a press in the session (§3.12) or
at a terminal, never by a `/config` row, and asking is never behind a setting (a dismissed ask
waits in the pane); the notice cadence is `ui.notices` in `config.toml` already and is not
duplicated here. The commands above are one registration, `/adage`, with an argument hint; the
verb is parsed from the arguments, as the mods API has it. `hold_dead_ends` (a `$.ui.ask`
before a known dead end, as the `blast-radius` sample mod does) is an open question (§10), not a
setting: the binary has no `PreToolUse` arm that matches `avoid` reflexes, and whether a hook may
wait on a person inside its budget is unmeasured.

## 6. Invariants

1. **The binary decides.** No reflex, grant, arm, bound or threshold is evaluated in TypeScript;
   the mod asks, including in the control arm.
2. **Fail open, both ways.** A missing binary, a timeout, a non-zero exit, a non-JSON answer:
   `{ ok: false }`, and `next(e)`. A hook that throws is skipped by the engine; no hook of the mod
   waits on a person. (The engine's own rule: "a hook that fails doesn't break the session".)
3. **Permissions are Claude Code's.** The mod never returns a `decision` from `tool.check`, never
   answers `tool.call` with a `deny`, never calls `$.prompt.submit`, never strips anything from a
   child's environment, and a receipt replaces only a run `$.tool.check` would have allowed
   unasked.
3a. **A person's decision is a person's press or a person's terminal.** A person-only verb runs
   from the mod only through one function, reached only from a `$.ui.ask` answer or a focused
   `Button`'s `onPress`, with the session's single-use press token on stdin; the token is issued
   once, in the window before the agent's first action, and lives only in engine memory. No hook,
   timer, feed line, command handler or model output ever runs one. The press is a provenance
   record and a guard against steering, of F11's class; it is not claimed as a boundary against
   an agent that runs as the same user.
4. **Nothing executes in a mod hook.** The mod's `$.process.run` runs the binary's decision paths,
   which read state and print JSON; E1 and E2 run where they run today, inside the command hooks
   and the sandbox; a predicted call is the agent's own tool under the engine's permissions.
5. **The control arm is silent and never named.** No prediction, rewrite, receipt or annotation;
   the binary prints nothing per event; nothing drawn, streamed or printed for the current
   session names the arm or counts what was withheld. (A `noticed` line and a learner toast appear
   in both arms.)
6. **A predicted call is a third execution path and is marked as such**: a per-reflex `predict`
   grant from a person (a press or a terminal), the `adage_` id prefix, the notice under the dialog, the
   marker on the row, `Judgement::Predicted` in the trace, never an `Execute` grant's step.
7. **Provenance is marked structurally**: the tool_use_id prefix, `Call.by`, the `origin` of
   appended rows when Q5 finds it, the ledger's per-step record; the text markers stay for other
   harnesses.
8. **The model's text stays plain.** Nothing in §4 reaches the model; nothing in §3 adds themed
   words to what it reads.
9. **Nothing drawn, nothing asked, headless.** `isInteractive === false` draws nothing and starts
   no feed; what the grants allow still runs, so an eval that wants today's session runs the
   hooks without the mod, or the mod in observe mode (§9).
10. **The API is pinned.** CI installs the Claude Code version the plugin was validated against
    for `claude plugin validate --strict` and `claude plugin test`; the public `claude-code.d.ts`
    in the Claude Code repository is diffed on each version bump (S9 Q8).

## 7. Where it runs

From the overview's table, with Adage's fallback in the last column:

| Where | Hooks run | Draws | Adage |
| --- | --- | --- | --- |
| `claude` in a terminal (editor terminals, JetBrains included) | Yes | Yes | Everything above |
| Desktop app, Code tab | Yes | Yes, except terminal-only elements (`Raster`, `Image`, `InfoNotice`, `TurnDuration`, `ToolProgress`) | Everything, **if** `$.process` is available there (the declaration file says "CLI only"; Q7 measures); the session notice goes to a toast, the turn line to the band; without `$.process`, the command hooks alone |
| Desktop app, WSL session | No | No | The command hooks alone |
| VS Code extension's chat panel | Yes | No | §3.1–3.11 run; nothing drawn; `/adage` answers as text |
| `claude -p`, the Agent SDK | Yes | No | §3.2–3.7 and §3.9–3.11 run as the grants and options allow; nothing asked, nothing drawn |
| Remote Control | Yes, on the machine | In that terminal | As the terminal |
| Cloud session | Yes, if the plugin reaches it | No | As `-p` |
| Claude Code before 2.1.287 | Inferred: the module key is ignored | | The command hooks alone, as today. **Unverified**: Q6 runs the pinned 2.1.221 against a `hooks.json` with `modules` to show the seven command hooks still load; if they do not, the manifest pins a minimum Claude Code version before the module ships |

## 8. Repository changes, in order

One table for the three documents (the plan's §6 and the test plan's §10 use the same names).

| Milestone | Ships | Needs from the spike |
| --- | --- | --- |
| **M0 · Spike S9** | Findings, the probe mod, numbers | |
| **M1 · Foundation** (binary only) | `PreToolUse` dedup ring; `duration_us` and the doctor p95; `adage watch --follow --json --session`; `adage ledger note` and the harness ledger file; status card `--json` and `tokens.ts`; `adage off` and `adage on` | |
| **M2 · See it** | `hooks/register.ts` with §3.1 and the §4 sites; the pane and `/adage …`; `claude plugin test` in CI on a pinned CLI; `scripts/check-plugin.mjs` learns the module; the two-plugin activation probe | Q1, Q2 (Layer 1), Q3, Q3b, Q6 |
| **M3 · Trust it** | §3.2 `TurnStep` in the binary and the `predict` grant mode; §3.3 receipts as results and §3.4 the engine-checked rewrite through the two-phase `PreToolUse`; the `ToolResult` card | Q2 (Layer 2), Q4 |
| **M4 · Skip it** | The mod's `turn.step` and the simulator; chains and multi-call steps; the finish line at the last step; dead-end substitution; §3.6 cached placement and the tool description; §3.7 compaction; the subagent prompt; §3.5 routing behind its option; §3.9 the ledger line; the mod capabilities on `adage_trace::Capabilities` | Q5, Q9, Q10, Q11, Q15 |
| **M5 · Notice** | §3.10: the `candidates` table and net, `noticed` on the decision, the learner's state in the feed, the toasts | Q12 |
| **M6 · Skills** | §3.11: the `skill` response kind, the learner's rule from Skill tool uses, `adage train`'s `verified.json` with inertness, the `turn.step` invoke under the grant, the `SkillPrompt` event | Q13 |
| **M7 · Evidence** | The `requests` metric and the arms in the demo runner; the development gate with a `MOD` arm on the reflex-proof harness; the registered confirmation on new groups | Q10 |
| **M8 · Ask it** | §3.12: the `Press` event and the window field; `require_person_or_press` at specific sites, the token on stdin, single-use; asking in the engine's dialog at `turn.complete` and the **waiting for you** list; stage then confirm; one plugin with both binaries; `Explore in browser` | Q14 |

Documentation moves with each milestone, as `how-it-works.md` requires: `harnesses.md` gains a
"Claude Code mod" section marked documented/inferred/measured; `live-learning.md` records the
harness cost source and the feed; `execution.md` gains "Predicted calls" beside "Approvals";
Architecture §15.2 F14's "status line" moves to built.

## 9. What "10×" would mean, and how it is measured

The claim is made in Adage's own units: model requests, tokens and API cost per task at preserved
quality (`how-it-works.md`, "What may be claimed"). Mods change two of the three inputs to that
number and leave quality to be proven. Four arms, because headless runs load the mod:

| Arm | Hooks | Mod | Grants |
| --- | --- | --- | --- |
| `a0` | none | off | none (the registered protocol's baseline) |
| `adage` | on | off | as the study |
| `mod-observe` | on | on, `ADAGE_MOD_OBSERVE=1` (only §3.1 observers and `ui.render` register; every other hook returns `next(e)`) | as the study |
| `mod` | on | on | the study's plus `--predict` on the finish-line and skill routes |

`mod-observe` exists to prove the mod's observation changes nothing: its model-facing output
(the hook stdout per event and the `<adage-reflex>` blocks delivered per event class) must equal
the `adage` arm's for a recorded payload stream. `control` sessions (hooks with `ADAGE_ARM=control`)
remain the in-product savings baseline and are not `a0`.

| Metric | Today (transfer pilot, four held-out tasks) | What the mod changes | Where measured |
| --- | --- | --- | --- |
| Model requests per task | Execution mode 0.67× the agent alone, with every session still asking for the test again | §3.2 removes the request that asks for a predicted step | `demo/run.py`, then the reflex-proof harness with a `MOD` arm, S9 Q10 |
| Tool runs per task | A duplicate test run in every automatic session of the pilot | §3.3 answers the repeated run from the receipt (a run saved, not a request) | The same runner, `receipts_as_results` |
| API cost per task | Priced from token counts after the fact | §3.5 routes routine steps to a cheaper model; §3.6 moves standing text into the cached prefix; §3.9 prices from the harness | Same runner, S9 Q2, Q11; the ledger's `cost.source: harness` |
| Hidden-test quality | 17/20 against the baseline's 19/20, which kept execution mode experimental | Must not move: the gate for every item above | Same runner; the protocol's quality rule |
| What the person sees of Adage per session | One line a day, or a second terminal | §4: every fired reflex, rewrite, receipt and skipped request, where it happened; §3.10: the learning itself | Q3, Q3b, Q12; the usability check in ux-design §14 |
| Skill selected at the moment the procedure says | The model's judgment, each time | §3.11: invoked by the granted reflex at that step, verified in place | Q13: invocations at the right step over the skill-steered sessions `adage train` already runs |

Whether the first row reaches ten times fewer requests on real tasks is what Q10 and then the
registered run find out; the mechanism that could get there is documented, the authority model
that keeps it safe is Adage's own with one new grant mode, and the control arm that keeps it
honest already exists. The second half of the title needs no spike to believe: a reflex the
person can see fire is a different product from one they read about in a daily line.

## 10. Open questions

All in [S9](../../spikes/S9-claude-mods/SPIKE.md): coexistence and dedup (Q1), latency and whether
time inside `next(e)` counts toward the hook budget (Q2), surfaces and silence (Q3, Q3b),
`$.tool.check` agreement and where the mod reads the permission mode (Q4), provenance and cost on
disk (Q5), fail-open, reload and the pre-2.1.287 `modules` key (Q6), hosts and `$.process` on the
Desktop app (Q7), API drift (Q8), the skipped request and whether a yielded call meets the
permission dialog (Q9), the gate (Q10), routing (Q11), noticed candidates on the fast path (Q12),
skill invocation, inertness and rewrite (Q13), the press (Q14: the mod's `classic.SessionStart`
runs after the command hooks' session start so the token is claimed before any agent action;
`$.process.run`'s stdout and stdin appear in no transcript, debug log or file; `/clear` and
`/resume` arrive as `classic.SessionStart` with their `source`; a `$.ui.ask` from
`turn.complete` waits on the person without a budget; a model cannot invoke a registered
command; `$.state` survives what Q6 says it survives), and the finish line at the last step (Q15: a
`turn.step` hook that replaces an `end_turn` stop with a tool chunk keeps the model's text, runs
the call and continues the loop; the obligation is then met by the agent's own call). Two
product decisions sit outside
the spike: whether a `hold_dead_ends` ask ever exists (it needs a binary `PreToolUse` arm for
`avoid` reflexes and a measured answer on hooks that wait on a person), and the reversal of Track
C's D2 to one plugin (§3.12), which the owner has asked for and which the plan carries as A8.
