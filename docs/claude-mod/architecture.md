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
> [test-plan.md](test-plan.md).

A mod is a plugin whose TypeScript runs *inside* Claude Code: functions the engine calls on every
tool call, prompt, model request, transcript row and screen draw, with an API (`$`) to the screen,
the host's processes and files, the session's usage figures and a store. Adage's seven command
hooks run *outside*: a spawned `adage hook claude <event>` with JSON in and JSON out, which can
add context, rewrite a Bash command and block a Stop, and nothing else.

This document designs Adage as a mod. The Rust binary stays the decider, the learner and the
sandbox; the command hooks stay as the floor on every host; the mod is one more file in the same
plugin. What the mod adds falls in two halves, and both are in the title:

- **Capability.** The mod sits at the one point the command hooks cannot reach: the moment before
  a model request goes out. When a `fire` reflex with a person's grant predicts the next tool
  call, the mod makes that call and the request is never sent. It also answers a repeated test run
  from the receipt Adage already holds, checks a rewrite against the engine's own permission
  verdict, routes routine steps to a cheaper model, and delivers standing procedures once, from
  the prompt cache, rather than at every event.
- **Looks.** Adage becomes visible where the person is looking: a line under the prompt a reflex
  fired at, a marker on the tool row it rewrote or ran, a receipt card in place of a wall of
  pytest output, a one-line band above the prompt, `/adage` with a live pane, and toasts for the
  moments that matter. Today the person gets one `systemMessage` a day and a split terminal.

## 1. The session, before and after

The acme-billing sandbox (`demo/`), which has three conventions an agent cannot guess: work items
are looked up with `./scripts/wi-lookup`, tests run with `make test`, and a schema edit under
`registry/` needs `make gen`. Adage has learned all three. This is a treated session with E2
delegation on and the codegen reflex granted `predict` (§3.2).

**Today.** The person sees, at most, one line at session start:

```text
adage: 2 reflexes and 14 instincts active in this repo; it keeps learning as you work. Nothing runs without you.
```

Then nothing. The directives go to the model as `additionalContext`, the rewrite shows as one
`systemMessage` line, the finish-line result as another. To see Adage work, the person opens a
second terminal and runs `adage watch`.

**With the mod.** The same session, in the terminal (the Desktop app's Code tab draws the same
sites with its own elements):

```text
 ✻ Claude Code 2.1.287
   ◠ adage · 4 instincts, 9 reflexes in acme-billing · learning after each session · nothing runs without you

 > fix W-2042 and make sure the registry still generates
   ◠ lookup-work-item · fire · ./scripts/wi-lookup W-2042 --json

 ● Bash(./scripts/wi-lookup W-2042 --json)
   ⎿ {"id":"W-2042","title":"Rounding error in invoice totals", …}

 ● Edit(registry/invoice.csv)
   ⎿ Updated registry/invoice.csv with 1 addition

 ● Bash(make gen)
   ◠ adage ran this step on the CPU: codegen-after-registry, no model request
   ⎿ generated 3 files

 ● Bash(.venv/bin/python -m pytest tests/test_invoice.py -q)
   ↪ adage: pointed this test run at .venv/bin/python (the project's environment)
   ⎿ ◠ adage receipt a1f2 · 48 passed in 1.9 s · the tests added in this session: test_total_rounds_half_up
      pytest's own output is not shown; adage ran the same command 4 s ago on identical inputs

 ✓ Worked for 1m 48s · adage: 2 requests saved, 1 dead end headed off

┌──────────────────────────────────────────────────────────────────────────────────────┐
│ ◠ adage  3 reflexes this session · 2 requests saved · ● healthy · learning in 38 s   │
└──────────────────────────────────────────────────────────────────────────────────────┘
 >
```

Every line with `◠` or `↪` is the mod's. The person did not run a command, open a pane or read a
`systemMessage`. The model saw the same plain `<adage-reflex>` blocks it sees today, plus one tool
call it did not have to ask for.

In a **control session** (1 in 10), the mod draws the first line and the band in their neutral
form (name, counts of what is installed, health, when the learner runs) and no per-event line,
which is exactly what a treated session with no matching reflex draws. The arm is never
inferable from the screen.

In **`claude -p`**, the SDK, the VS Code chat panel and a cloud session, the hooks run and nothing
draws (the engine's rule); the mod falls back to nothing, so the evals in `demo/` see the same
session as today.

## 2. The shape: one plugin, two layers, one binary

```text
plugin/claude-code/
├── .claude-plugin/plugin.json      # + "types": "./types/index.d.ts", + userConfig (§5)
├── hooks/
│   ├── hooks.json                  # the seven command hooks (unchanged) + "modules": ["./register.ts"]
│   ├── register.ts                 # on(...) registrations; nothing else
│   ├── bridge.ts                   # ask(): one $.process.run of the binary per decision, fail open
│   ├── decide.ts                   # classic.*, tool.call, turn.step hooks (§3)
│   ├── draw/                       # ui.render hooks per site (§4), pure functions of $.state
│   │   ├── band.tsx  rows.tsx  pane.tsx  card.tsx
│   ├── feed.ts                     # the watch feed: $.process.spawn(adage watch --follow --json)
│   ├── commands.ts                 # /adage … (§5)
│   ├── tokens.ts                   # generated by `cargo xtask tokens`: glyphs, the accent, roles
│   └── *.test.ts                   # claude plugin test, over 'terminal' and 'desktop'
├── types/index.d.ts                # PluginState contract: every $.state value the mod keeps
├── scripts/session-start, quiet    # unchanged
└── dist/                           # unchanged
```

`hooks/hooks.json` "can also hold settings hooks under `hooks`" (reference, Files), so the
command hooks and the module ship in one file. The order is documented: a mod's `tool.call` hook
runs first; the plugin's own `PreToolUse` command hook runs "after the last mod calls `next`, as
part of Claude Code's own behavior", and for every classic event `next(e)` resolves to what the
command hooks answered.

Three rules keep the binary in charge:

1. **Every decision is `adage hook claude <Event>`**, with the payload on stdin as today. The mod
   never decides; it asks, through one helper:

   ```ts
   // hooks/bridge.ts
   export async function ask($: EngineInterface, event: string, payload: unknown, timeoutMs = 5000) {
     const bin = await binary($)                              // $DATA/bin/adage, as the shim linked it
     if (bin === undefined) return undefined                  // no binary: fail open, as the shim does
     try {
       const ran = await $.process.run([bin, 'hook', 'claude', event], {
         stdin: JSON.stringify(payload), cwd: await $.session.cwd(), timeoutMs })
       return ran.exitCode === 0 && ran.stdout.trim() !== '' ? JSON.parse(ran.stdout) as HookAnswer : undefined
     } catch { return undefined }                             // cannot start, timed out, not JSON: nothing
   }
   ```

   `$.process.run` "takes an argument list and uses no shell", the exec form S1 measured; the
   binary's own cost is unchanged (1.7–6 ms, Architecture §9). One new event, `TurnStep` (§3.2),
   is the only decision the command hooks cannot also make.

2. **A decision is made once per `tool_use_id`.** Where the mod and a command hook both see an
   event (the rewrite path), the binary's existing identity check (`event_identity` in
   `crates/adage-hook/src/harness/claude.rs`, today for `PostToolBatch`, `PostToolUse` and
   `PostToolUseFailure`) extends to `PreToolUse`, so the second delivery is a no-op with no spool
   record. Prompt and batch decisions stay with the command hooks entirely; the mod observes their
   results (`classic.UserPromptSubmit`, `classic.PostToolBatch`) and draws.

3. **Adage's state stays in `$ADAGE_HOME`.** `$.state` holds only what a drawing reads this
   session (a list of fired reflexes, counters), declared in `types/index.d.ts`; `$.store` holds
   only the person's UI preferences (which pane tab was open), because the store is "a key-value
   store that every session on the machine shares" with a documented read-then-write race. The
   feed is one child, `adage watch --follow --json`, started from `session.start` through
   `$.process.spawn` and ending with the module: every spool parser, lock probe and word table
   stays in Rust, and the mod holds rows and draws them. Daemonless (A1) holds: the child lives
   exactly as long as the session.

## 3. Capability: what the mod lets Adage do

Each subsection names the event, the exact shape the hook returns, the binary's part, what the
model reads, what the person sees, and how the control arm behaves. The letter in brackets is the
spike question that must pass first.

### 3.1 Observe the command hooks and draw [Q1, Q3]

```ts
for (const event of ['classic.UserPromptSubmit', 'classic.PostToolBatch', 'classic.PostToolUseFailure'] as const) {
  on(event, async ($, e, next) => {
    const t0 = await $.clock.now()
    const answered = await next(e)                         // the command hook beneath ran, once
    const fired = parseReflexes(answered.additionalContext ?? [])   // route, track, strength, kind per block
    await update($, session, s => ({ ...s, fired: [...s.fired, ...fired],
      lastMs: (await $.clock.now()) - t0 }))                // the hook's wall time: §3.10
    return answered                                        // unchanged
  })
}
on('classic.Stop', async ($, e, next) => {
  const answered = await next(e)
  if (answered.block) $.ui.toast('adage: finish line — the agent continues once')
  return answered
})
```

- **Binary:** unchanged. **Model:** unchanged. **Control:** the command hook prints nothing;
  `fired` stays empty; nothing draws. **Hosts:** every one.

### 3.2 The request Adage does not send [Q9, Q10]

The event the command hooks lack. `turn.step` "fires when Claude Code is about to send one
request to the model"; a hook on it is an async generator; "an answer without `next` sends no
request"; chunks the hook yields are recorded and run ("a block whose `tool` chunk never comes out
of the chain is no tool call: the engine records none and runs none", and the converse). A step
with no request carries `stopReason: null, usage: null`.

```ts
on('turn.step', async function* ($, e, next) {
  if (e.agentId !== undefined || e.index === 0) return yield* next(e)   // main loop; the first step is the prompt's
  const recent = (await $.session.messages()).slice(-12)               // { role, text, toolUses: [{ name, input, result? }] }
  const answer = await ask($, 'TurnStep', {
    session_id: await $.session.id(), cwd: await $.session.cwd(),
    turn_id: e.turnId, index: e.index, model: e.model, recent })
  const p = answer?.predict
  if (p === undefined) return yield* next(e)                           // nothing predicted: the request goes out
  const id = `adage_${p.route}_${e.turnId}_${e.index}`                 // provenance in the id (§3.10)
  yield { kind: 'tool', index: 0, id, name: p.tool }
  yield { kind: 'input', index: 0, json: JSON.stringify(p.input) }
  yield { kind: 'stop', stopReason: 'tool_use', usage: null }
  return { turnId: e.turnId, index: e.index, answer: '', toolUses: [{ name: p.tool, input: p.input }],
           stopReason: 'tool_use', usage: null }
})
```

- **Binary:** a new event, `adage hook claude TurnStep`. Input: the recent messages as the engine
  reports them (tool uses with their results, and the model's text between them, which
  `PostToolBatch` never carried). Output: `{}` or
  `{"predict": {"tool": "Bash", "input": {"command": "make gen"}, "route": "codegen-after-registry", "track": "…"}}`.
  The matcher is the one the runtime already has for context triggers (Track G G4); the gates are
  the E2 gates with one more effect, **`predict`**, granted per reflex by a person at a terminal
  (`adage review delegate <track> <reflex> --effect predict`), under the same policy, key and lapse
  rules as `verify`. Two bounds the binary enforces: never two predicted steps in a row (a model
  step must separate them, so Adage can never drive a turn alone), and at most `N` per session
  (default 6), like the Stop gate's two.
- **Model:** its next request carries an assistant turn with a tool call it did not write, and the
  tool's result. That is the API's prefill shape; models carry on from it. The call itself ran
  through every `tool.call` hook, the plugin's `PreToolUse` command hook (so the receipt path and
  the rewrite apply), the person's permission rules and dialog, and the tool. Adage's grant
  decides whether to predict; Claude Code's permissions decide whether it runs; nothing runs in
  the hook, and the E2 sandbox is not involved: this is the agent's own tool.
- **Person:** the tool row carries `◠ adage ran this step on the CPU: <reflex>, no model request`
  (§4), and the turn's closing line counts it.
- **Control:** the hook forwards `yield* next(e)` and asks nothing; the binary spools
  `would_have` from the `PostToolBatch` it does see, so saved requests are measured against a
  baseline, as directives are.
- **Cost accounting:** the step's `usage` is null on the engine's side; §3.10 records the step as
  Adage's with the model it replaced, so the ledger prices what was *not* spent at the same rate
  as what was.

### 3.3 The receipt as the result [Q9]

`tool.call` may "answer a call yourself, return an object with a `result` field … without calling
`next`. When you do that, no permission prompt appears and the tool doesn't run, so the result you
return is all Claude learns about what happened."

```ts
on('tool.call', { tool: 'Bash' }, async ($, e, next) => {
  const r = await ask($, 'PreToolUse', pretoolPayload(e, await $.session.id(), await $.session.cwd()))
  if (r?.receipt?.fresh) {                                             // identical inputs, a passing run seconds ago
    return { result: { stdout: r.receipt.text, stderr: '', interrupted: false },
             context: [r.receipt.note] }                               // the <adage-receipt> block, after the result
  }
  return rewrite($, e, next, r)                                        // §3.4
})
```

- **Binary:** the receipt check `docs/execution.md` already runs ("Receipts before a repeated test
  run") gains a `fresh` flag and the receipt's text as the result would read. It stays a fact:
  the model asked for a test run and received its result; nothing was refused. A receipt is
  answered as the result only for a run the journal holds as passed on byte-identical inputs
  within the freshness bound; otherwise the call runs and the note rides as context, as today.
- **Model:** a shorter result than pytest's output, the named tests, the session's own tests
  first; the duplicate 20 s run the pilot saw in every automatic session does not happen.
- **Person:** the `ToolResult` row draws the receipt card (§4).
- **Control:** never.

### 3.4 Rewrites checked by the engine [Q4]

Today the U11 planner reads the settings files itself to decide whether `.venv/bin/python -m pytest`
would still match a `Bash(pytest:*)` allow rule, and `--allowedTools` is invisible to it.
`$.tool.check({ tool, input })` "resolves to the decision the rules, the permission mode, and
those hooks reached" and "executes nothing".

```ts
async function rewrite($, e, next, r) {
  const command = r?.hookSpecificOutput?.updatedInput?.command
  if (command === undefined) return next(e)
  const { decision } = await $.tool.check({ tool: 'Bash', input: { ...e, command } })
  if (decision === 'deny') return next({ ...e }, /* advice only: */ withContext(r.advice))
  $.ui.notice(e.tool_use_id, `adage ↪ ${r.systemMessage}`)            // one line under the permission dialog
  await update($, rewrites, list => [...list, { id: e.tool_use_id, from: e.command, to: command }])
  return next({ ...e, command })
}
```

- **Binary:** the planner's own settings reading becomes the fallback for hosts without a mod.
- **Model:** as today (`additionalContext` names the word that changed). **Person:** the notice
  under the dialog and the `↪` on the tool row. **Control:** never, as today.

### 3.5 Hold a known dead end, if the person wants that [design decision]

`blast-radius`, one of Anthropic's sample mods, holds a risky Bash call with `$.ui.ask` and two
buttons. An `inhibit` reflex (`npm test` where `make test` is the way) could do the same: hold the
call, say why in the dialog, and let the person proceed or redirect. Adage's rule since the launch
review is advice, never a refusal, because "a hook `deny` beats the person's own allow rules, and
no measured harm justified overriding them". A hold is not a deny: the person decides, in the
dialog, and `$.ui.ask` "rejects … in a `claude -p` run", so headless runs are untouched.

```ts
if (r?.inhibit && options.hold_dead_ends === true) {
  const choice = await $.ui.ask(`adage: ${r.inhibit.reason}. Run it anyway?`, ['Run it', 'Use the learned command']).catch(() => 'Run it')
  if (choice === 'Use the learned command') return next({ ...e, command: r.inhibit.instead })
}
```

Off by default (`hold_dead_ends: false`, §5). It is listed because the API makes it a one-line
product decision rather than an architecture change.

### 3.6 Route the step [Q11]

`next({ ...e, model })` or `effort` "sends another". Where the binary marks a step **routine** (a
`fire` reflex applies, confidence below the predict threshold, or the predicted call needs its
arguments filled from the conversation), the step goes to a small model at low effort:

```ts
if (answer?.routine) return yield* next({ ...e, model: options.routine_model, effort: 'low' })
```

`agent.spawn` has the same rewrite for subagents (`{ model }`). Off by default; the quality rule
that kept execution mode experimental (hidden tests 17/20 against 19/20) is the gate, and Q11
reports numbers, not a verdict.

### 3.7 Standing procedures once, from the cache [Q2]

Today each applicable procedure card and instinct is `additionalContext`, injected again at each
matching event and kept in the history. `prompt.compose` returns "`{ sections }`, a list of
`{ id, text, scope }` in the order they're sent"; a section appended last is `session`-scoped, and
text that does not change within a session is served from the prompt cache. `tool.describe` fires
"once for each tool, when its description is first sent to Claude" and is cached for the session.

```ts
on('prompt.compose', async ($, e, next) => {
  const { sections } = await next(e)
  const standing = await read($, procedures)                           // from SessionStart's answer, fixed for the session
  return standing === '' ? { sections } : { sections: [...sections, { id: 'adage:procedures', text: standing, scope: 'session' }] }
})
on('tool.describe', { tool: 'Bash' }, async ($, e, next) => {
  const d = await next(e)
  const envNote = await read($, environment)                           // "tests run under .venv/bin/python here", when U11 is bound
  return envNote === '' ? d : { ...d, description: `${d.description}\n\n${envNote}` }
})
```

The per-event channel stays for what is situational (a lookup at this prompt, a dead end at this
failure). The standing text is one copy per session; `$.ui.invalidate('prompt.section')` is never
called mid-session, so the cache is never spent.

### 3.8 Survive compaction

`session.compact` hands the hook `messages` and `instructions`. The mod drops Adage's own rows
(`<adage-reflex>`, `<adage-receipt>`, the finish-line continuation) from the summariser's input,
so they are not relearned as the agent's own steps, and adds one instruction to keep the
procedures the session followed:

```ts
on('session.compact', async ($, e, next) => next({ ...e,
  messages: e.messages.filter(m => !isAdageRow(m)),
  instructions: [e.instructions, 'Keep the repository procedures that were followed (how tests run, what regenerates after a schema edit).'].filter(Boolean).join('\n') }))
```

### 3.9 Subagents

`tool.call` "fires … including calls a subagent makes", with `e.agentId`; `turn.step` carries it
too. An Explore agent that re-runs the lookup the main loop already did is the same waste one loop
down. The reflex and receipt paths above apply unchanged; predictions (§3.2) stay on the main loop
(`e.agentId === undefined`) until a subagent study says otherwise.

### 3.10 Measure and attribute with the engine's own figures [Q5]

- **Cost from the harness.** Every `turn.step` result carries `usage` (`input_tokens`,
  `output_tokens`, `cache_read_input_tokens`, `cache_creation_input_tokens`, `model`), and
  `$.session.usage()` returns `{ startedAt, context, rateLimits, cost }`. At `turn.complete` the
  mod writes one line through the binary, `adage ledger note` (to add): per-step usage, the steps
  Adage replaced with the model each would have used, and the engine's `cost` where the host keeps
  one. The ledger's `cost.source` becomes `harness` for Claude Code sessions, in both arms.
- **Latency on every machine.** The wall time around `next(e)` in §3.1 is the command hook's cost
  as the engine saw it, spawn included; it goes in the same ledger line, so `adage doctor` gets
  the p95 check zero-config P1 lists, measured where the person works.
- **The harness version.** `$.session.version()` answers `{ version, base, builtAt }`; recorded
  once per session, `doctor` says when the harness moved past the version the plugin was
  validated against (R7).
- **Provenance in the row.** A predicted call's `tool_use_id` carries Adage's prefix and the
  route; a row appended with `$.session.append` is stored under `origin: { kind: 'plugin', name }`.
  If Q5 finds the origin on disk, `crates/adage-learn` marks steering from the row's author and
  keeps the `<adage-reflex>` marker as the cross-harness fallback.
- **The learner's trigger.** Unchanged: the `Stop` and `SessionEnd` command hooks start the
  detached learner. The mod's `session.end` hook only flushes the ledger line; "all `session.end`
  hooks together" share 1.5 s, so nothing waits there.

### 3.11 Noticed: learning the person can watch [Q12]

Adage learns after the session, from the trace, and the person hears about it a day later in one
line. The mod can show the learning while it happens, at three moments, without turning any of
them into a directive.

**The pattern Adage has seen before.** The runtime already matches *context triggers*, "two or
three earlier events, in order" over the last sixteen (Architecture §4.3.2), for reflexes that are
in service. Two kinds of pattern are matched but never delivered: a **nascent** reflex (it waits
for review) and a **candidate** the learner has counted but that has not cleared `min_support`
(five unsteered sessions by default). Today neither leaves a trace in the session. The design adds
a `candidates` table to the index, compiled with the same no-match fast-path words as every other
trigger, so the common path stays at 1.8 ms; at `PostToolBatch` the binary matches them as it
matches a reflex, spools a `noticed` record (`kind: "noticed"`, the route or candidate id, its
support so far, never a directive), and prints nothing to the model. The mod reads the record off
the command hook's result (§3.1 extended with a `noticed` field in `additionalContext`'s sibling,
or off the feed) and draws one line under the batch:

```text
 ● Edit(acme/invoice.py)
 ● Bash(make test)
   ◠ adage noticed · make test after editing acme/** · seen in 3 earlier sessions · a reflex forms at 5
```

```text
   ◠ adage noticed · ./scripts/wi-lookup then reading the files it names · this reflex waits for your review (adage review)
```

The rules that keep it from becoming noise: only sequences (a context trigger of two or more
events, or a prompt family followed by its command), never a single call; once per candidate per
session; nothing for a pattern that is already a firm reflex (that is a directive, §3.1); plain
counts, never a prediction of what the agent should do next. Both arms draw it, because a note
about learning changes neither the model's path nor the person's view of which arm they are in.

**The learner, live.** The feed (§2) carries `learn.log` lines as they are written, and the
learner's state file says when the next run starts (`<slug>.spawn.json`, `after`). The band's
last cell counts down to it (`learning in 38 s`), turns to `learning…` while the lock is held, and
when the line lands the mod raises one toast with the change that matters most, in the lexicon's
words:

```text
 adage learned from this session
 ◠ codegen-after-registry  prime → fire  (9 of 9 sessions)
```

**The reflex that just formed from what you did.** When a `noticed` candidate reaches support in
the learn that follows this session, the toast says so and names the session as the one that
tipped it: `◠ test-after-acme-edit formed · this session was its 5th`. That is the moment the
product's promise ("your agent keeps relearning the same lessons; Adage remembers them") becomes
something the person has watched happen.

- **Binary:** the `candidates` table in the index, the `noticed` record, and a `--follow --json`
  feed that includes the learner's state. The "familiar-command computation" F14 lists as not
  built is the same count by another name (sessions and days a command has run in), so one
  implementation serves both.
- **Model:** nothing. **Control:** drawn in both arms. **Hosts:** drawn where the mod draws; in
  `-p` the record is spooled and nothing else.

### 3.12 Skills: select them deterministically, and verify them where they run [Q13]

Skill selection is the model's judgment today: it reads the skill list in the prompt and decides.
Adage's compiler (`adage train SKILL.md`) turns a skill's commands into verified reflexes, and
`training.md` lists what stays text: judgment, and "a command with no stimulus". F14 lists "skill
triggers" as not built. The mods API gives three handles on skills, and together they make a new
response kind, **`skill`**: stimulus → *use this skill now*.

| Handle | Documented shape | What Adage does with it |
| --- | --- | --- |
| `turn.step`, yielding a `Skill` tool call | The engine's Skill tool takes `{ skill: string, args?: string }`; a yielded `tool` + `input` + `stop` chunk is recorded and run (§3.2) | **Invoke.** At the step where a `fire` skill reflex matches, the mod yields `Skill { skill: 'release-checklist' }` and no request is sent. The engine runs the Skill tool; the skill's text enters the transcript as the tool's result; the model's next step proceeds with the skill loaded, at the moment the pattern says, every time |
| `skill.prompt` | "Fires when the engine expands a skill's prompt for the model (`/name`, the Skill tool, a preload)"; `e` is `{ skill, text }`; return `{ text }` | **Verify in place.** For a skill Adage has trained in this repository, the mod rewrites the text as the model reads it: each step the replay verified carries one plain line (`verified here 2026-09-30: make test passes; the lookup is ./scripts/wi-lookup {id}`), a step the replay found stale carries its correction, and the environment Adage bound (`.venv/bin/python`) replaces a bare `python`. The skill file is untouched; the model reads the repository's truth |
| `skill.prompt` as an observation | The same event, every invocation, with the skill's name | **Learn selection.** Each invocation is spooled with what preceded it (the prompt family, the last edits, the last commands), so the learner can form a `skill` reflex from how skills are actually used here: "in sessions whose prompt looks like *cut a release*, the release-checklist skill was invoked after the version bump, 7 of 8 times". Transcripts record Skill tool uses too, so history before the mod counts |
| `$.agent.register({ skills })`, `agent.spawn` | An agent definition's `skills` are "preloaded into the agent's context before its first turn"; a spawn's `prompt` is rewritable | **Equip subagents.** A skill reflex whose stimulus is a delegation (the `Agent` call's description, which `PreToolUse` on `Agent` carries, S5 Q7) prepends `Use the <skill> skill for this.` to the subagent's prompt |
| `tool.describe` on `Skill` | The Skill tool's description, cached for the session | **Advise, cached.** The standing list of skills that apply in this repository and when (from the `skill` reflexes), once, in the tool's own description |

Three delivery levels, like every other reflex, so the same `skill` reflex works on every harness:

1. **Advise** (every harness, today's channel): `<adage-reflex kind="skill">` in `additionalContext`
   at the stimulus: "In this repository the release-checklist skill is used at this point. Ignore
   this if the user is asking for something else." The model still chooses.
2. **Invoke** (Claude Code with the mod): the `turn.step` yield above. A skill invocation loads
   text; it runs no command and edits no file, so its ceiling is **Observe**, not Regenerate: it
   needs no `delegate` grant, only a `fire` reflex, the two bounds of §3.2 (never two predicted
   steps in a row, a per-session cap) and `skills.invoke` on (§5, default on). The model's next
   step can still ignore the skill; what is deterministic is *that it was offered, in full, at
   that moment*, which is the part that varied before.
3. **Verify in place** (Claude Code with the mod): the `skill.prompt` rewrite, for any skill
   `adage train` has compiled here, at every invocation however it was triggered.

What the person sees: the invoked skill's tool row carries `◠ adage invoked this skill: <reflex>,
no model request`; a verified skill's row carries `◠ adage verified 3 of 5 steps here · step 2
corrected`. In the control arm nothing is invoked or rewritten and the `would_have` is spooled.

- **Binary:** the `skill` response kind in tracks and the index (stimulus: intent family,
  after-edit glob, context trigger or delegation; response: a skill name and optional args); the
  learner's rule for forming one from Skill tool uses in traces; `adage train` recording per-step
  verification so `skill.prompt` has something to say. **Model:** at level 1 a plain block; at
  level 2 the skill's own text as a tool result; at level 3 the skill's text with plain,
  factual lines added. **Hosts:** level 1 everywhere; levels 2 and 3 wherever hooks run,
  `-p` included, which is the first place to measure them.

## 4. Looks: every render site, and what Adage draws there

`ui.render` fires for each render site; a hook can "change a detail" (`next({ ...e, props })`),
"replace the drawing" (return a tree, no `next`) or "leave it alone" (`next(e)`), and `next(e)`
"returns a reference to Claude Code's drawing, `{ type: 'engine', ref }`" that a tree may place
"in a `Box` beside elements of your own". The permission prompt is not a render site.

| Site | Adage draws | When | How |
| --- | --- | --- | --- |
| `InfoNotice` (terminal) | `◠ adage · 4 instincts, 9 reflexes in <repo> · learning after each session · nothing runs without you`, or the day's notice (`2 reflexes strengthened`, `1 install step waits: adage review`) | Session start | Replaces the `systemMessage` where the mod draws; the command hook still prints it for hosts that do not |
| `UserMessage` | One dim line under the prompt per reflex fired at it: `◠ lookup-work-item · fire · ./scripts/wi-lookup W-2042 --json` | A prompt-triggered reflex | `Box` column: `await next(e)` first, then the lines; `e.props.isExpanded` (ctrl+o) returns `next(e)` alone |
| `ToolUse` | A marker line under the row: `↪ adage: pointed this test run at .venv/bin/python` for a rewrite; `◠ adage ran this step on the CPU: <reflex>, no model request` for a predicted call; `◠ dead end headed off: <reflex>` for a followed `avoid` | Matched by `e.requestId` (the `tool_use_id`) against `$.state` | `Box` with the engine's row then one `Text dimColor` |
| `ToolResult` | The receipt card in place of pytest's output: receipt id, result, duration, the tests named, the session's own tests first, and the one line saying the output is not shown | A receipt answered as the result (§3.3) or delivered beside one | Return a tree, no `next`; `Markdown` for the test list |
| `ToolGroup` or the last `ToolUse` of a batch | `◠ adage noticed · <sequence> · seen in 3 earlier sessions · a reflex forms at 5`, or `· waits for your review` | A `noticed` record for this batch (§3.11), once per candidate per session | `Box` with the engine's row then one `Text dimColor` |
| `ToolUse` for `Skill` | `◠ adage invoked this skill: <reflex>, no model request`; `◠ adage verified 3 of 5 steps here · step 2 corrected` | A skill reflex invoked or a trained skill expanded (§3.12) | As the other `ToolUse` markers |
| `Spinner` | `Thinking · adage ◠ 1…` while a reflex was delivered this turn | `e.props.suffix` | `next({ ...e, props: { ...e.props, suffix } })` |
| `TurnDuration` (terminal) | `Worked for 1m 48s · adage: 2 requests saved, 1 dead end headed off` | A turn with Adage activity | `next({ ...e, props: { ...e.props, word } })` or a `Box` beside the reference |
| `AbovePrompt` | One line, like `token-weather`: `◠ adage  3 reflexes this session · 2 requests saved · ● healthy · learning in 38 s`; a second line while a learner runs or a grant lapsed | Always, unless `band: false` or the session is headless | Sized to `e.props.bodyColumns`; `next(e)` when nothing to say so other mods' bands show |
| `Pane` `adage` | The watch pane: tabs **Live**, **Reflexes**, **Health** (§4.1) | `/adage`, `/adage watch`, or unasked only when the terminal is 144 columns wide and `pane: "auto"` | Fed by `feed.ts`; rows as `plain` Buttons so Enter inspects |
| `CommandOutput` | `/adage` and `/adage why` draw the status card and the last directive's evidence as a tree, not as text | `{ component: 'CommandOutput', props: { command: 'adage' } }` | `Markdown` and `Code` |
| `AskUserQuestion` | Untouched | | |
| `$.ui.toast` | `adage: 2 reflexes strengthened`, `adage: the make gen approval lapsed (Makefile changed); re-approve with adage review delegate …`, `adage: finish line — the agent continues once` | Rare, important | `holdToasts` on the pane keeps them until it closes |
| `$.ui.status` | Problems only: `the bundled binary failed its checksum`, `hooks are off for this session` | The engine prefixes it `⚠ adage:`; the glyph is the engine's, so it is used only for warnings | |
| `$.ui.log` | Nothing by default; `adage: …` dim lines under `verbose: true` | | Starts with `●` and the mod's name, by the engine |

Rules that hold at every site:

- **Theme keys, not hex.** `Text` colour "is a theme key or a color"; the style guide says the
  terminal's own colours win, and in a pane the engine's theme keys are that rule. Adage's accent
  and the lamp roles come from `hooks/tokens.ts`, generated by `cargo xtask tokens` beside
  `tokens.rs`; no colour literal in the module. Under the Desktop app the same keys map to its
  theme.
- **The lexicon's glyphs** (`◠` reflex, `◉` instinct, `↪` rewrite, `●`/`○` lamps, `»` notice),
  each one column wide; the ASCII fallbacks under `ADAGE_ASCII=1` read through `$.env.get`.
- **No digit hotkeys.** A digit hotkey on the band "also fires when the user types that digit
  alone into an empty prompt and pauses"; Adage's band has no buttons.
- **Quiet in control, invisible when nothing fired, nothing headless.** `session.start`'s
  `isInteractive` is false in `-p` and the SDK; the mod registers no `ui.render` hook there.
- **Every tree is a pure function of `$.state`**, so `claude plugin test`'s `ui.mount` draws it on
  `terminal` and `desktop` from fixture state, as the Rust frames are drawn from fixtures today.

### 4.1 The pane

`/adage` opens the pane with `focus: true, closeOnEscape: true, rows: 20`. Three tabs, as three
`plain` Buttons with hotkeys `l`, `r`, `h`:

- **Live**: the `adage watch` feed (now, conditioning, health), one `Text` per row truncated to
  `bodyColumns`, `/` an `Input` filter, Enter on a row opens a second pane `adage-inspect`
  (`focus`, `closeOnEscape`, `rows: 12`) with the directive text and evidence as `Markdown`.
  Arrows and page keys scroll (the engine's window); `p` pauses.
- **Reflexes**: every reflex by track from `adage graph --json`, read-only: strength, lifecycle,
  trigger and step, evidence, the directive text. The review keys of `adage ui` are **not** here:
  `adage review` refuses inside an agent session (`AGENT_ENV` carries `CLAUDECODE` into any child
  the mod runs), by F11. Each row has a `Copy command` button (`$.ui.copy`) with the exact
  `adage review …` to run at a terminal.
- **Health**: `adage doctor --json`, lamps and the fixing command; `Enter` runs only what the
  binary allows from a session (`condition now`); the rest copies the command.
- A `Link` to the running map, `http://localhost:<port>`, when `adage map` is up.

The feed child, `adage watch --follow --json` (to add: the same `tail`, `learnlog` and `health`
modules as the terminal pane, one JSON line per change), is started once:

```ts
on('session.start', async ($, e, next) => {
  const started = await next(e)
  if (!e.isInteractive) return started
  void (async () => {
    const feed = $.process.spawn({ argv: [await binary($), 'watch', '--follow', '--json', '--repo', e.cwd] })
    for await (const chunk of feed) {
      if ('text' in chunk) for (const line of lines(chunk.text)) await update($, rows, r => [...r, JSON.parse(line)].slice(-500))
    }
  })()                                                                 // the loop is the child's life; it ends with the module
  return started
})
```

## 5. Commands and settings

`$.command.register` in `session.start`; `command.run` answers each. Names that the lexicon's
verbs already use, under one prefix so the typeahead groups them:

| Command | Does | Allowed from a session because |
| --- | --- | --- |
| `/adage` | The status card, drawn (`CommandOutput`) | Read-only |
| `/adage watch` | Opens the pane on Live | UI |
| `/adage reflexes`, `/adage health` | Opens the pane on that tab | UI |
| `/adage why` | The last directive this session: its text, route, evidence, and whether it was followed | Read-only (spool) |
| `/adage off`, `/adage on` | Writes or removes `.adage/disabled` for this repository | "Turns something down", allowed anywhere (live-learning.md) |
| `/adage pause` | Pauses the feed and the band for this session | UI |

Settings go in the manifest's `userConfig`, which Claude Code draws as `/config` rows and stores
under `pluginConfigs`; a change reloads the mod with the new `options`:

```json
"userConfig": {
  "band":           { "type": "boolean", "title": "Band above the prompt", "description": "One line: reflexes this session, requests saved, health", "default": true },
  "pane":           { "type": "string",  "title": "Watch pane", "description": "Open the live pane unasked in a wide terminal", "options": ["auto", "manual"], "default": "manual" },
  "notices":        { "type": "string",  "title": "Notices", "description": "The one-line notice at session start", "options": ["daily", "off"], "default": "daily" },
  "hold_dead_ends": { "type": "boolean", "title": "Hold known dead ends", "description": "Ask before a command Adage has learned fails here (never in claude -p)", "default": false },
  "routine_model":  { "type": "string",  "title": "Route routine steps to", "description": "A model alias for steps a fire reflex covers, or empty to route nothing", "default": "" },
  "noticed":        { "type": "boolean", "title": "Show what Adage noticed", "description": "A line when a sequence of calls matches one Adage is still learning", "default": true },
  "skills_invoke":  { "type": "boolean", "title": "Invoke skills from reflexes", "description": "Load a skill at the step a fire reflex names, without a model request to choose it", "default": true }
}
```

Only keys the binary allows from a session appear here. Execution policy, grants, trust and the
learning sources stay at the terminal, as `docs/live-learning.md` requires.

## 6. Invariants

1. **The binary decides.** No reflex, grant, arm or threshold is evaluated in TypeScript.
2. **Fail open, both ways.** A missing binary, a timeout, a non-JSON answer: `undefined`, and
   `next(e)`. A hook that throws is skipped by the engine; `register` adds no `.catch` that
   answers, so a failure never blocks a call. (The engine's own rule: "a hook that fails doesn't
   break the session".)
3. **Permissions are Claude Code's.** The mod never returns `{ decision: 'allow' }` from
   `tool.check`, never answers `tool.call` with a `deny`, never passes `asUser: true` to
   `$.prompt.submit`, and strips nothing from the environment of a child it runs.
4. **Nothing executes in a hook.** `$.process.run` runs the binary's decision paths, which read
   state and print JSON; E1 and E2 run where they run today.
5. **The control arm is silent and indistinguishable.** No per-event drawing, no prediction, no
   rewrite, no receipt; the neutral band and notice only.
6. **Provenance is marked structurally**: the tool_use_id prefix, the `origin` of appended rows,
   the ledger's per-step record; the text markers stay for other harnesses.
7. **The model's text stays plain.** Nothing in §4 reaches the model; nothing in §3 adds themed
   words to what it reads.
8. **Headless is untouched.** `isInteractive === false` registers no UI and asks nothing.
9. **The API is pinned.** CI runs `claude plugin validate --strict` and `claude plugin test`
   against a pinned Claude Code version; the public `claude-code.d.ts` in the Claude Code
   repository is diffed on each release bump (S9 Q8).

## 7. Where it runs

From the overview's table, with Adage's fallback in the last column:

| Where | Hooks run | Draws | Adage |
| --- | --- | --- | --- |
| `claude` in a terminal (editor terminals, JetBrains included) | Yes | Yes | Everything above |
| Desktop app, Code tab | Yes | Yes, except terminal-only elements (`Raster`, `Image`, `InfoNotice`, `TurnDuration`, `ToolProgress`) | Everything; the session notice goes to a toast, the turn line to the band |
| Desktop app, WSL session | No | No | The command hooks alone |
| VS Code extension's chat panel | Yes | No | §3 in full; `/adage` answers as text |
| `claude -p`, the Agent SDK | Yes | No | §3.1–3.4, 3.7–3.10; nothing asked, nothing drawn; the evals see today's session plus any prediction the grants allow |
| Remote Control | Yes, on the machine | In that terminal | As the terminal |
| Cloud session | Yes, if the plugin reaches it | No | As `-p` |
| Claude Code before 2.1.287 | The module is ignored | | The command hooks alone, as today |

`$.process` is "CLI only" in the declaration file and unrestricted on the API page; S9 Q7
measures the Desktop app before anything above is promised there.

## 8. Repository changes, in order

| Milestone | Ships | Needs from the spike |
| --- | --- | --- |
| **M0 · Spike S9** | Findings, the probe mod, numbers | |
| **M1 · See it** | `hooks/register.ts` with §3.1 and the `UserMessage`, `ToolUse`, `Spinner`, `TurnDuration`, `InfoNotice`, `AbovePrompt` sites; `tokens.ts` from `cargo xtask tokens`; `claude plugin test` in CI; `scripts/check-plugin.mjs` learns the module | Q1, Q2 (Layer 1), Q3, Q6 |
| **M2 · Open it** | The pane and `/adage …`; `adage watch --follow --json`; `adage graph --json` and `doctor --json` as the pane's data; `userConfig` rows | Q3b |
| **M3 · Trust it** | §3.4 rewrites through `$.tool.check`; §3.3 receipts as results; `PreToolUse` identity dedup in the binary; the `ToolResult` card | Q2 (Layer 2), Q4 |
| **M4 · Measure it** | §3.10: `adage ledger note`, harness-priced cost, latency and version in the spool and `doctor`; `session.append` provenance in the learner if Q5 finds it | Q5 |
| **M5 · Skip it** | §3.2 `TurnStep` in the binary with the `predict` effect, the two bounds, the control-arm `would_have`; §3.7 cached delivery; §3.8 compaction; §3.6 routing behind its option | Q9, Q10, Q11 |
| **M6 · Notice and learn in view** | §3.11: the `candidates` table, the `noticed` record, the learner's state in the feed, the toasts | Q12 |
| **M7 · Skills** | §3.12: the `skill` response kind, the learner's rule from Skill tool uses, `adage train`'s per-step verification record, the `turn.step` invoke, the `skill.prompt` rewrite | Q13 |

Documentation moves with each milestone, as `how-it-works.md` requires: `harnesses.md` gains a
"Claude Code mod" section marked documented/inferred/measured; `live-learning.md` records the
harness cost source; `execution.md` gains the `predict` effect beside `verify`; Architecture §15.2
F14's "status line" moves to built.

## 9. What "10×" would mean, and how it is measured

The claim is made in Adage's own units: model requests, tokens and API cost per task at preserved
quality (`how-it-works.md`, "What may be claimed"). Mods change two of the three inputs to that
number and leave quality to be proven:

| Metric | Today (transfer pilot, four held-out tasks, Haiku) | What the mod changes | Where measured |
| --- | --- | --- | --- |
| Model requests per task | Execution mode 0.67× the agent alone, with every session still asking for the test again | §3.2 removes the request that asks for a predicted step; §3.3 removes the duplicate run's request pair; §3.7 shortens every request | `demo/run.py all`, treated vs control, S9 Q10 |
| API cost per task | Priced from token counts after the fact | §3.6 routes routine steps to a cheaper model; §3.7 moves standing text into the cached prefix; §3.10 prices from the harness | Same runner, S9 Q11; the ledger's `cost.source: harness` |
| Hidden-test quality | 17/20 against the baseline's 19/20, which kept execution mode experimental | Must not move: the gate for every item above | Same runner |
| What the person sees of Adage per session | One line a day, or a second terminal | §4: every fired reflex, rewrite, receipt and saved request, where it happened; §3.11: the learning itself | Q3, Q3b, Q12; a usability pass on the demo recording |
| Skill selected at the moment the procedure says | The model's judgment, each time | §3.12: invoked by the reflex at that step, verified in place | Q13: invocations at the right step over the skill-steered sessions `adage train` already runs |

Whether the first row reaches ten times fewer requests on real tasks is what Q10 finds out; the
mechanism that could get there is documented, the authority model that keeps it safe is Adage's
own, and the control arm that keeps it honest already exists. The second half of the title needs
no spike to believe: a reflex the person can see fire is a different product from one they read
about in a daily line.

## 10. Open questions

All in [S9](../../spikes/S9-claude-mods/SPIKE.md): coexistence and dedup (Q1), latency (Q2), surfaces
and silence (Q3, Q3b), `$.tool.check` agreement (Q4), provenance and cost on disk (Q5), fail-open
and reload (Q6), hosts (Q7), API drift (Q8), the skipped request (Q9), the gate (Q10), routing
(Q11), noticed candidates on the fast path (Q12), skill invocation and rewrite (Q13). Two product decisions sit outside the spike: whether `hold_dead_ends` exists at all (§3.5),
and whether a person-verified approval inside the harness is ever designed, which would be the
only way review and delegation reach the pane.
