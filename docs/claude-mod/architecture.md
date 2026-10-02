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
on, §2), with the codegen reflex granted `predict` by the person at a terminal (§3.2).

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
   ◠ lookup-work-item · scripts/wi-lookup W-2042 --json

 ● Bash(scripts/wi-lookup W-2042 --json)
   ⎿ {"id":"W-2042","title":"Rounding error in invoice totals", …}

 ● Edit(schema/invoice.json)
   ⎿ Updated schema/invoice.json with 1 addition

 ● Bash(make gen)
   ◠ adage chose this step: codegen-after-schema · no model request
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
`off` and `map` from any code path, and the person-only verbs (`review`, `delegate`, `trust`,
`on`) only from a `Button`'s `onPress` and only with `--press` (§3.12); `install` and `config` are
never in it. The list is checked by a unit test and by `scripts/check-plugin.mjs`. (With one
plugin, A8, the two-copy rule above becomes a presence probe and this paragraph's second sentence
falls away; it is kept here because the plan builds the one-plugin step last.)

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
  if (!active || e.agentId !== undefined || e.index === 0) return yield* next(e)   // main loop; step 0 is the prompt's
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
  never see each other's keys. Three bounds the binary enforces: never two predicted steps in a
  row (a model step must separate them, so Adage can never drive a turn alone); at most
  `runtime.predict_cap` per session (default 6), like the Stop gate's two; and never for a route
  that also holds an `Execute` grant for the same step, because that action already runs inside
  the `PostToolBatch` hook and a prediction would run it twice (`delegate --predict` on such a
  route says so and refuses). The grant is read in the hook's outer layer and handed to the pure
  `decide()` the way exec-only routes are today (a thread-local set), so the runtime crate stays
  free of adage-exec types.
- **Model:** its next request carries an assistant turn with a tool call it did not write, and the
  tool's result. That is the API's prefill shape; models carry on from it. The call itself ran
  through every `tool.call` hook, the plugin's `PreToolUse` command hook, the person's permission
  rules and dialog, and the tool. Adage's grant decides whether to predict; Claude Code's
  permissions decide whether it runs. **This is a third execution path**, beside E1 and E2:
  nothing runs in the hook, the E2 sandbox and journal are not involved, the call is the agent's
  own Bash under the person's permission mode. Q9 must show the engine applies permission rules
  and the dialog to a yielded call exactly as to a model's; if it does not, Layer 4 does not
  ship. Under `bypassPermissions`, `dontAsk`, an allow rule, or `-p` with `--allowedTools`, no
  dialog appears for the model's own calls either; the person's `predict` grant, per reflex, is
  the consent, and the notice under the dialog (where one appears) and the tool-row marker name
  the call as Adage's.
- **Person:** the tool row carries `◠ adage chose this step: <reflex> · no model request` (§4),
  `$.ui.notice` puts one line under the permission dialog when one is shown, and the turn's
  closing line counts it as a step without a model request.
- **Control:** the mod always asks (the arm is the binary's to know); the binary answers `{}`,
  spools `would_predict` on the `turn_step` record, and the request goes out. Saved requests are
  measured against a baseline, as directives are.
- **Headless:** runs (`-p` included); it is where Q9 and the evals measure it.
- **Cost accounting:** the step's `usage` is null on the engine's side; §3.10 records the step as
  Adage's with the model it replaced, so the ledger prices what was *not* spent at the same rate
  as what was.

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
down. The receipt and rewrite paths above apply unchanged; predictions (§3.2) stay on the main loop
(`e.agentId === undefined`) until a subagent study says otherwise.

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
   ○ adage noticed · scripts/wi-lookup then reading the files it names · waits for your review · run adage review in a terminal
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

Every decision Adage needs from a person (approve a reflex that waits for review, strengthen or
dismiss one, grant execution or prediction, turn delegation on, accept a team's signing key, turn
Adage back on) is a terminal command today, and persona (a) never opens a terminal. The mod can
ask in the session and act on the answer, because a mod `Button` is something the model cannot
press: it can only call tools. That is a stronger signal of a person's intent than the guard F11
relies on (`CLAUDECODE` in a shell's environment, which the code itself calls "a guard against
steering, not a sandbox"). What makes it sound is binding the press to the session in a way an
agent with a shell cannot forge.

**The press token.** At `session.start`, before the agent has run anything, the mod's `Standing`
call (§3.6, a mod-only channel: its stdout reaches the mod and no one else) returns a `press`
token, 32 random bytes. The binary stores only its SHA-256 in the session state and marks it
issued; the mod keeps the token in `$.state`, which is engine memory the agent cannot read. A
token is issued, or reissued after `/resume`, **only while the session's latest spool record is
`session_start`**: the window between the command hooks' session start and the first prompt, in
which no agent can run anything. A `Standing` call outside that window gets no token, so an agent
running `adage hook claude Standing` from Bash mid-session gets nothing, and the spool it can read
holds a hash. `/clear` is a new engine session with a new id and a new token. A person-only verb
run by the mod carries `--press <token>`; `require_person` accepts a token whose hash matches the
session's as a person's act, with `CLAUDECODE` set or not. Everything else about the verb is
unchanged: the same code path, the same record, the same notice.

**The ask row.** A one-line prompt, `adage asks`, seated as a small pane above the prompt and
answerable by one press, drawn when the learner's line lands (`turn.complete`, never inside a
hook's budget), and never a modal: `$.ui.ask` is not used for learning news. Each ask has three
answers: do it, not this one, and show me why (the inspect pane). `esc` dismisses; a dismissed
ask returns at most once more, a day later; "not this one" records `dismiss` so it never returns.
Asks of different weight ask differently:

| Decision | Today | In the row | Weight |
| --- | --- | --- | --- |
| Approve a reflex that waits for review (nascent), dismiss it, strengthen, weaken, pin | `adage review <verb> …` at a terminal | `◠ test-after-acme-edit waits for your review · enable · not this one · why` | one press |
| Grant execution (E1 or E2) for one reflex | `adage review delegate … --level e2 --outputs …`, which stages the action once and prints the diff | `◠ codegen-after-schema can run make gen for you (sandboxed) · see what it would change · not now`; the press opens the staged diff (`Code`) with `allow` and `not now`; `allow` records the grant | two presses, the diff between them |
| Grant prediction for one reflex | `adage review delegate … --predict` | `◠ codegen-after-schema can run its step without asking the model · allow · not now · why` | two presses (the second says what will and will not meet a dialog) |
| Turn execution on for this machine (policy `max_level e2`, `delegate on`) | `adage delegate policy --max-level e2`, `adage delegate on`, and the second plugin | The first execution ask carries it: `this turns on sandboxed execution for this machine · allow · not now`; the row says what is sandboxed and what never runs | once per machine, two presses |
| Accept a team's signing key | `adage trust …` at a terminal | `◠ 6 team reflexes signed by alice@acme.com are in this repository · use them · not now · why` | two presses (the second shows the key and the pack list) |
| Turn Adage back on in this repository | `adage on` at a terminal | `◠ adage is off here · turn it on` | one press |

The row's text follows ux-design §10; nothing in it is a strength or lifecycle word. The terminal
command stays as an equal path for persona (b), and the Reflexes tab lists every pending ask so a
dismissed one can be found again.

**What the press does not change.** The binary still decides: the mod runs the verb the person
pressed and nothing else, through the same allow-list (which gains the person-only verbs, each
required to carry `--press`); the verb's own refusals (policy, effect, exec-only, a double grant)
still apply; an E2 grant still stages the action before recording anything; the control arm is
unaffected (asks are about learning and grants, not delivery). A hostile agent with a shell could
still unset `CLAUDECODE` and run a verb, as it can today; the press changes what a *person* can
do, not what the guard against steering can prevent. What it closes is the gap the design had
left open: a person who installs the plugin and never opens a terminal now gets the whole
product.

- **Binary:** `Standing` issues the token under the window rule; `require_person` learns
  `--press`; `review delegate` learns `--stage --json` (the staged diff as data, for the row) and
  `--confirm --press`; `trust` and `on` learn `--press`; the spool records `by: press` on the
  decision so `adage review` history shows how it was made. **Model:** nothing. **Control:**
  unaffected. **Hosts:** the row draws where the mod draws; headless sessions issue no token and
  ask nothing.

**One plugin.** Track C's decision D2 chose a second plugin, `adage-exec`, because installing it
was "explicit consent in the harness's own UI", and listed the alternative: one plugin whose
hooks call `adage-exec` only when a marker that `adage delegate on` writes exists. With consent
now a press in the session, the marker is the explicit act and the second plugin is a second
install step with no purpose. The design therefore ships **both binaries in the one plugin**:
the shim links `$DATA/bin/adage` and `$DATA/bin/adage-exec`, links `$DATA/bin/hook` to whichever
serves (`adage-exec` when `$ADAGE_HOME/exec/enabled` exists, else `adage`), and the command hooks
call `$DATA/bin/hook`. Hooks are never registered twice because there is one plugin; the promise
Track C wanted checkable (no `Command::new` in the `adage` crates) is unchanged because the
binaries stay separate. The two-plugin activation probe of §2 becomes a one-plugin presence
probe, and `plugin/claude-code-exec` is retired after one release as an alias that installs
nothing. This reverses D2 and is called out as such in the plan (A8).

**The web map.** `adage map` serves a read-only page on localhost (GET and HEAD only, the host
checked). Every tab of the pane ends with one `Explore in browser` link, which starts `adage map` if it
is not running (a child like the feed, ended with the module) and opens a `Link` to
`http://localhost:<port>`. The map stays read-only: a browser page cannot make a decision, since
any page on the machine could send the same request; decisions stay in the pane, where a press is
a person's.

## 4. Looks: every render site, and what Adage draws there

`ui.render` fires for each render site; a hook can "change a detail" (`next({ ...e, props })`),
"replace the drawing" (return a tree, no `next`) or "leave it alone" (`next(e)`), and `next(e)`
"returns a reference to Claude Code's drawing, `{ type: 'engine', ref }`" that a tree may place
"in a `Box` beside elements of your own". The permission prompt is not a render site. The strings
and states are specified in [ux-design.md](ux-design.md); this table says where and how.

| Site | Adage draws | When | How |
| --- | --- | --- | --- |
| `InfoNotice` (terminal) | The engine's own notice for the hook's `systemMessage` (the first-session line, or the day's notice), redrawn with the glyph: `◠ adage · 3 reflexes and 4 instincts active in this repo · …` | Whenever the binary's notice gate printed one (first session, then at most daily) | `next({ ...e, props })` on the engine's notice; no second line, no new frequency |
| `UserMessage` | One dim line under the prompt per reflex fired at it: `◠ lookup-work-item · scripts/wi-lookup W-2042 --json` | A prompt-triggered reflex | Keyed by order: the nth `UserPromptSubmit` result ↔ the nth `UserMessage` (the payload carries no message id); `Box` column: `await next(e)` first, then the lines; `e.props.isExpanded` returns `next(e)` alone |
| `ToolUse` | A marker line under the row: `↪ adage: pointed this test run at .venv/bin/python` for a rewrite; `◠ adage chose this step: <reflex> · no model request` for a predicted call; `◠ dead end headed off: <reflex>` for a followed `avoid` | Rewrites and predictions matched by `e.requestId` (the `tool_use_id`) against `$.state`; the avoid marker from the feed's judgement record for that call | `Box` with the engine's row then one `Text dimColor` |
| `ToolResult` | The receipt card in place of pytest's output: receipt id, result, duration, the tests named, the session's own tests first, and the one line saying the output is not shown | A receipt answered as the result (§3.3) only; beside a run that happened, one dim line and `next(e)` | Return a tree, no `next`; `Markdown` for the test list |
| `ToolGroup` or the last `ToolUse` of a batch | `○ adage noticed · <sequence> · seen in 3 earlier sessions`, or `· waits for your review · run adage review in a terminal` | A `noticed` record for this batch (§3.10), once per candidate per session | `Box` with the engine's row then one `Text dimColor` |
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
- **Reflexes**: every reflex by track from `adage graph --json`, read-only: strength, lifecycle,
  trigger and step, evidence, the directive text. The review keys of `adage ui` are **not** here:
  `adage review` refuses inside an agent session (`require_person`; the bridge sets
  `CLAUDECODE=1` on every child), by F11. Each row offers `Copy command` (`$.ui.copy`) for each
  verb the graph's `review.verbs` lists, in `review_command`'s exact form, to run at a terminal.
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
| `/adage on` | Runs `adage on --press <token>`: a person's press (§3.12) | The press token |
| `/adage review` | Opens the pane on the pending asks (§3.12) | UI; each answer is a press |
| `/adage map` | `Explore in browser`: starts `adage map` if needed and opens the link | Read-only |
| `/adage pause` | Pauses the feed and the band for this session; the band shows `paused` | UI |

Settings go in the manifest's `userConfig`, which Claude Code draws as `/config` rows and stores
under `pluginConfigs`; a change reloads the mod with the new `options`:

```json
"userConfig": {
  "band":          { "type": "boolean", "title": "Band above the prompt", "description": "One line: reflexes this session, steps without a model request, health, learning", "default": true },
  "pane":          { "type": "string",  "title": "Watch pane", "description": "Open the live pane unasked in a wide terminal", "options": ["auto", "manual"], "default": "manual" },
  "noticed":       { "type": "boolean", "title": "Show what Adage noticed", "description": "A line when a sequence of calls matches one Adage is still learning", "default": true },
  "skills_invoke": { "type": "boolean", "title": "Invoke skills from reflexes", "description": "Load a skill at the step a granted reflex names, without a model request to choose it", "default": false },
  "routine_model": { "type": "string",  "title": "Route routine steps to", "description": "A model alias for steps a fire reflex covers, or empty to route nothing", "default": "" },
  "asks":          { "type": "boolean", "title": "Ask me in the session", "description": "One line when a reflex waits for your decision; off keeps every decision at the terminal", "default": true }
}
```

Only keys the binary allows from a session appear here. Execution policy, grants, trust and the
learning sources are decisions, not settings: they are made by a press in the ask row (§3.12) or
at a terminal, never by a `/config` row; the notice cadence is `ui.notices` in `config.toml`
already and is not duplicated here. `hold_dead_ends` (a `$.ui.ask`
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
   from the mod only inside a `Button`'s `onPress`, with the session's press token, which is
   issued once in the window before the first prompt and lives only in engine memory. No hook,
   timer or model output ever runs one.
4. **Nothing executes in a mod hook.** The mod's `$.process.run` runs the binary's decision paths,
   which read state and print JSON; E1 and E2 run where they run today, inside the command hooks
   and the sandbox; a predicted call is the agent's own tool under the engine's permissions.
5. **The control arm is silent and never named.** No prediction, rewrite, receipt or annotation;
   the binary prints nothing per event; nothing drawn, streamed or printed for the current
   session names the arm or counts what was withheld. (A `noticed` line and a learner toast appear
   in both arms.)
6. **A predicted call is a third execution path and is marked as such**: a per-reflex `predict`
   grant from a person at a terminal, the `adage_` id prefix, the notice under the dialog, the
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
| **M4 · Skip it** | The mod's `turn.step` and the simulator; §3.6 cached placement; §3.7 compaction; §3.5 routing behind its option; §3.9 the ledger line | Q5, Q9, Q10, Q11 |
| **M5 · Notice** | §3.10: the `candidates` table and net, `noticed` on the decision, the learner's state in the feed, the toasts | Q12 |
| **M6 · Skills** | §3.11: the `skill` response kind, the learner's rule from Skill tool uses, `adage train`'s `verified.json` with inertness, the `turn.step` invoke under the grant, the `SkillPrompt` event | Q13 |
| **M7 · Evidence** | The `requests` metric and the arms in the demo runner; the development gate with a `MOD` arm on the reflex-proof harness; the registered confirmation on new groups | Q10 |
| **M8 · Ask it** | §3.12: the press token and the window rule; `--press` on every person-only verb; the ask row and the pending-asks list; one plugin with both binaries; the map link | Q14 |

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
skill invocation, inertness and rewrite (Q13), the press token's window and secrecy (Q14: no
agent call can land before the first prompt; `$.process.run`'s stdout reaches the mod alone;
`/resume` reissues inside the window and nothing else does). Two product decisions sit outside
the spike: whether a `hold_dead_ends` ask ever exists (it needs a binary `PreToolUse` arm for
`avoid` reflexes and a measured answer on hooks that wait on a person), and the reversal of Track
C's D2 to one plugin (§3.12), which the owner has asked for and which the plan carries as A8.
