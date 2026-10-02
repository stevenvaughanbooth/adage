# S9 · Claude Code mods: an in-process plugin beside the command hooks

**Informs:** A3 (transport), A8 (packaging), A9 (harness layer), F3 (delivery channel per decision
point), F14 (the status line and `suggest` mode, listed as not built) · **Wave:** 3 ·
**Budget:** two days for Q1–Q8, ≤ 20 short `claude -p` runs on Haiku, one interactive terminal
session per question that needs a person at the prompt; Q9–Q11 reuse the demo runner and its
budget (two arms on the four held-out tasks, Haiku), one more day

Research date: **2026-10-02**, against Claude Code **2.1.287**, the release that turned mods on by
default. Every claim about mods below is marked **documented** (stated in the public docs,
[overview](https://code.claude.com/docs/en/plugins/mods/overview),
[events](https://code.claude.com/docs/en/plugins/mods/events),
[API](https://code.claude.com/docs/en/plugins/mods/api),
[interface](https://code.claude.com/docs/en/plugins/mods/interface) and
[reference](https://code.claude.com/docs/en/plugins/mods/reference), or in the `claude-code`
declaration file that build writes for a mod, `types/claude-code.d.ts`, about 20,000 lines, which
the reference calls the authority), **inferred** (our reading of those sources, not run), or **to
verify** (what this spike measures). The design that builds on this brief is
[docs/claude-mod/](../../docs/claude-mod/README.md): the architecture, the user experience, the
implementation plan and the test plan. Nothing here was run against a live Claude Code yet; that
is the point of the spike.
The API is early access: the skill says it "moves between releases: the declaration file is the
authority". Treat every type name below as true of 2.1.287 and re-read the file before building.

## Question

Claude Code now loads **mods**: plugins of *function hooks*, a TypeScript module the engine runs
in-process, with an event chain over every tool call, prompt, system-prompt render, transcript
row and screen draw, and an interface (`$`) to the screen, the host's processes and files, the
session's usage figures and a key-value store. Adage integrates with Claude Code today through
seven exec-form **command hooks** (`plugin/claude-code/hooks/hooks.json`), each a spawned
`adage hook claude <event>` with the payload on stdin and JSON on stdout.

Would an Adage mod, placed **beside** those command hooks and above the same Rust binary, give
the person and the learner things the command hooks cannot, without touching the decision core,
its latency budget, its fail-open contract, or its security posture? And what would it cost in
coupling to an API that changes between releases?

## Why it matters

Adage's decision core is deliberate about what it does not do in Claude Code, and the gaps are
all on the *person's* side of the hook, not the model's:

| Gap today | Where it is recorded |
| --- | --- |
| **No status line.** The person learns that a reflex fired from a one-line `systemMessage` at most once a day, or by running `adage watch` in a split terminal. The Design Spec's own rule for study participants, "a status line shows when a directive fired", is listed under F14 as not built. | Design Spec §(opt-in participants); Architecture §15.2 F14; `docs/live-learning.md` (`ui.notices = "daily"`) |
| **Rewrites are checked against a copy of the rules.** The U11 `PreToolUse` rewrite reads the user, project and local settings files itself to decide whether the rewritten command would still match the person's `Bash(…)` allow rules; rules given on the command line (`--allowedTools`) "are invisible to a hook", so a headless run should turn rewrites off. | `docs/execution.md`, "Rewrites to the project's environment" |
| **Cost comes from pricing tables.** The ledger records a trace's cost as `{usd?, source: harness\|priced@<date>\|none}`; for Claude Code it is priced after the fact from token counts in the transcript. | `docs/live-learning.md`, "Files" |
| **Provenance is a text marker.** The learner tells Adage's own context from the agent's by the `<adage-reflex>` wrapper and fixed template sentences ("This reminder is from Adage, not from the user."). It works, and it is the only signal a command hook can leave. | `docs/how-it-works.md` ("Preserve result and intervention provenance"), `docs/harnesses.md` |
| **The shim is a shell script.** `scripts/session-start` (120 lines of `sh`) finds, verifies and pre-runs the binary. Whether Claude Code on Windows runs an exec-form `sh` hook at all is marked unverified in that script. | `plugin/claude-code/scripts/session-start` |
| **Latency is not recorded at run time.** The §9 figures are from one M3 Pro; "Hook latency has not been measured on Linux or Windows", and a `duration_us` per spool record with a `doctor` p95 check is listed as later work. | Architecture §9–§10; `docs/zero-config.md` (P1) |
| **The agent re-runs what Adage already ran.** After an E2 receipt arrived as hook context, agents requested the same test command again (2/1/1/1 times in the pilot); replacing the tool result (`updatedToolOutput`) is the candidate fix. | `docs/hook-action-reuse-research.md`, `docs/launch-handoff.md` |
| **No in-session commands.** `!adage` prints the status card through the Bash prefix; nothing answers `/adage`. The Design Spec imagined `/adage off` (J6), and zero-config lists a review slash command as later, "once there is a design for a person-verified approval inside the harness". | Design Spec J6; `docs/zero-config.md` ("Later"); Architecture §14 |
| **Harness version drift has no alarm** (R7 open); the plugin cannot tell which Claude Code it runs under. | Architecture §14 |

A mod reaches every one of these: `$.ui.status`, `$.tool.check`, `$.session.usage()`,
`$.session.append` (rows stored with `origin: { kind: 'plugin', name }`), `$.process.run` and
`$.fs` from a module the engine loads on every platform it runs on, and `$.command.register`.
The question is whether it can do so **additively**: the command hooks stay as the floor on every
host, the binary stays the sole decider, and a mod that fails to load changes nothing.

## What a mod is (documented, 2.1.287)

Three files in the plugin folder: `.claude-plugin/plugin.json` (the manifest Adage already has),
`hooks/hooks.json` naming one module (`{ "modules": ["./register.ts"] }`), and that module,
`export const register: Register = (on, options) => { … }`. The module runs "in an environment
of its own, with no DOM and no Node": no `require`, no `fetch`, no `process`; everything outside
is reached through `$`. The engine compiles the TypeScript itself, so no build step and no Node
at build time (Adage's "building never needs Node" rule holds; `tsc -p <folder>` is for the editor
and CI only).

`on(event, matcher?, hook)` adds a hook; every hook is `($, e, next)`. `next(e)` runs the hooks
beneath and then the engine's own behaviour and resolves to the event's result; a hook that
returns without `next` answers for itself; `next({ ...e, x })` rewrites what the rest sees. A
hook that throws, overruns its budget (`HookBudget.ms = 10_000` per dispatch) or answers a wrong
shape "is skipped: the hooks beneath and core run in its place". That is fail-open by
construction, and it matches Adage's own rule ("exit 0 and print nothing on any error").

The chain has tiers: managed `prepend`, `user`, `builtin`, `core`, managed `append`. For the
classic settings-hook events the chain is "[managed settings hooks, ...hooks modules, the other
settings hooks as core]": **a mod runs above the plugin's own command hooks, and `next(e)`
resolves to what those command hooks answered**, folded into one `ClassicResult`
(`additionalContext: string[]`, one entry per hook; `block`; the event's own fields). That one
fact is what makes an additive design possible.

Events that matter for Adage:

| Event | What it carries | What it allows |
| --- | --- | --- |
| `classic.<Event>` for every settings hook event: `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PostToolBatch`, `PostToolUseFailure`, `Stop`, `SessionEnd`, `SubagentStart`, … | `e` is the hook's whole stdin payload (`session_id`, `cwd`, `transcript_path`, `tool_calls`, …), except `classic.PreToolUse`, whose `e` is the tool call alone | Observe or replace the command hooks, event for event, with the payload Adage already parses |
| `tool.call`, matcher `{ tool: 'Bash' }` | `e.command`, `e.tool_use_id`; `await next(e)` resolves `{ result, text, isError }` | Rewrite the command in place (`next({ ...e, command })`), see the result and its error flag (the failure path) in the same hook |
| `tool.check` and `$.tool.check({ tool, input })` | the engine's own permission verdict: "rules, mode, the tool's own check, PreToolUse's decision" | Ask whether a rewritten command is still allowed, without reading settings files |
| `prompt.submit` | `e.text`; `next({ ...e, context: [...] })` attaches "what the model reads beside the prompt and the user never sees" | The directive channel at the prompt, hidden from the transcript view |
| `prompt.compose` | the system prompt's sections, each `{ id, text, scope }` | Add a `session`-scoped section (the E2 session contract) that stays stable for the prompt cache |
| `session.start` | `cwd`, `surface`, `isInteractive` | Register commands and tools; know whether a person is at the prompt (`-p` and the SDK are `false`) |
| `session.end` | `reason` (`prompt_input_exit`, `clear`, `resume`, `other`); a short shared budget | What `SessionEnd` gives today, plus `/clear` |
| `turn.complete`, `session.measure`, `$.session.usage()` | per-turn `usage` (the four token counts), the context fill, rate limits, and `cost` "as the status line has them" | A ledger line from the harness's own figures |
| `session.append` and `$.session.append({ message })` | every row the conversation keeps, with `door` and `origin`; a plugin's row is stored under `origin: { kind: 'plugin', name }` | Deliver a directive as a row whose author is recorded, not inferred |
| `ui.render` on `AbovePrompt`, `Pane`, `UserMessage`, `CommandOutput` | a tree of `Box`, `Text`, `Button`, `Markdown` per surface (`terminal`, `desktop`, `vscode`, `mobile`) | A band, a pane, a drawn command output |
| `command.run` with `$.command.register` | `/adage …` typed by the person | In-session commands |

Interface nouns that matter: `$.ui.status(text)` ("this plugin's status line under the prompt,
beside the engine's own pinned notices"), `$.ui.toast`, `$.ui.log` (a dim transcript line the
model never reads), `$.ui.notice(tool_use_id, text)` (one line under an open permission dialog),
`$.ui.ask` (the engine's AskUserQuestion dialog), `$.process.run(argv, { stdin, cwd, env,
timeoutMs })` ("no shell"; `{ exitCode, stdout, stderr }`; **"CLI only"**), `$.fs` (read, write,
list, stat, 4 MiB per read), `$.store` (JSON, kept between sessions, 4 MiB in all), `$.state` and
`atom`/`read`/`update` (values that survive a hot reload and redraw their readers),
`$.clock.every`, `$.settings.read()`, `$.env.get`.

Development: `claude plugin validate <folder>` reads the manifest and the module's source "the way
the engine will" (CONTRIBUTING already runs `claude plugin validate --strict plugin/claude-code`);
`claude plugin test <folder>` runs `*.test.ts` files against the engine itself, with a `ui.mount`
that draws a component on a named surface; `claude --plugin-dir <folder>` loads from disk and
hot-reloads on save. `$.state` values need a `types/index.d.ts` contract named in the manifest.

## Where it fits

The thesis: **the binary decides, the mod shows, measures, and, where a reflex is strong enough
and a person has granted it, acts in the model's place.** Four layers, each optional, each
leaving the one beneath untouched. The first three are plumbing and visibility. The fourth is the
one that touches Adage's actual claim, fewer model requests, and it is the reason to run this
spike rather than file it.

### Layer 0 · What stays exactly as it is

`adage hook claude <event>`, the seven command hooks in exec form, the `session-start` shim, the
`adage-exec` plugin's replacement of those hooks when delegation is on, the control arm, the
session state slots and spools, the golden output (`crates/adage-cli/tests/golden/claude-code.txt`),
the harness simulator. Codex and OpenCode are untouched; the `Event`/`Output` adapter layer (A9)
needs no new variant. A mod is Claude Code polish over an unchanged core.

### Layer 1 · Observe: the mod reads what the command hooks answered

The mod hooks the classic events the plugin registers, calls `next(e)` **first**, and draws from
the result. It adds no decision, no spawn and no I/O to the hot path: the command hook beneath
runs exactly as today, and the mod's hook costs one in-process function call after it.

```ts
// plugin/claude-code/hooks/register.ts (sketch; the names are 2.1.287's)
import type { Register } from 'claude-code'

const MARK = '<adage-reflex'

export const register: Register = on => {
  for (const event of ['classic.UserPromptSubmit', 'classic.PostToolBatch',
                       'classic.PostToolUseFailure'] as const) {
    on(event, async ($, e, next) => {
      const answered = await next(e)                     // the command hook ran beneath
      const fired = (answered.additionalContext ?? []).filter(t => t.includes(MARK))
      if (fired.length > 0) $.ui.status(`adage ◠ ${fired.length} reflex${fired.length > 1 ? 'es' : ''} at ${e.hook_event_name}`)
      return answered                                     // unchanged
    })
  }
  on('classic.Stop', async ($, e, next) => {
    const answered = await next(e)
    if (answered.block) $.ui.toast('adage: finish line — the agent continues once')
    return answered
  })
}
```

What this layer gives, with nothing else changed:

- **The line F14 asks for, as a band.** The API page says `$.ui.status` "starts with `⚠` and the
  mod's name", so the engine's warning glyph would sit on every Adage line; the band above the
  prompt (`AbovePrompt`, the site the `token-weather` sample draws in) is Adage's own to draw,
  one line replaced at each decision: which event, how many reflexes, a rewrite (`↪`), a receipt,
  the finish line. Neutral when the session is in the control arm or no route matched (the binary
  prints nothing; the mod sees an empty `additionalContext`). `$.ui.status` is kept for warnings,
  where the glyph fits. Words and glyphs follow the lexicon and the style guide, because the text
  is for the person; the directive text the model reads is never touched.
- **`adage watch` inside Claude Code.** A `Pane` (docked beside a fullscreen transcript, inline
  above the prompt otherwise, or opened by `/adage watch`) that draws what `adage watch` draws,
  fed by one long-lived child, `adage watch --follow --json` (to add), through `$.process.spawn`.
  No split terminal, no second program. On the desktop app and VS Code the same tree draws with
  their element tables. The fit is worked through in [TUI fit](#tui-fit-adage-watch-yes-adage-ui-no).
- **Toasts for the rare, important moments**: a learner finished and a reflex changed strength
  (`learn.log` tail), a lapsed grant, a finish-line continuation. These replace nothing: the
  `systemMessage` notice stays for hosts without a mod.
- **Run-time latency, on every machine.** `$.clock.now()` before and after `next(e)` is the
  command hook's wall time as the engine saw it, spawn included. Written through
  `adage ledger note` (or a spool field) it gives `doctor` the p95 check zero-config P1 asks for,
  measured on the person's Linux or Windows machine rather than on one Mac in a spike.
- **The harness version, and a drift alarm.** `$.session.version()` answers the engine's
  `version`, `base` and `builtAt`; recorded once per session, `doctor` can say when the harness
  moved past the version the plugin was validated against (R7).
- **`/adage off` and `/adage status`.** Turning Adage down is allowed from anywhere
  (`.adage/disabled`, `ADAGE_DISABLE`; "every other key only tunes Adage or turns something
  down, and can be set from anywhere"), so the J6 kill switch fits a registered command. Status
  prints what `!adage` prints, drawn as a tree in the `CommandOutput` row.
- **Nothing in `-p` and the SDK.** `session.start` reports `isInteractive: false` there; the mod
  registers no UI and the evals (`demo/`, headless Haiku runs) see the same session as today.
  This must be measured, not assumed (Q3): the mod loads in `-p` too.

### Layer 2 · Decide through the mod: the binary's answer, the engine's permission check

The mod calls the binary itself from `tool.call` and `prompt.submit` and answers for the event, so
the equivalent command hook is suppressed (how, exactly, is Q1), or those two command hooks are
removed from `hooks.json` while the mod serves them. The payload goes on stdin as today, so
`crates/adage-hook` is unchanged; only the caller differs:

```ts
on('tool.call', { tool: 'Bash' }, async ($, e, next) => {
  const payload = JSON.stringify({ hook_event_name: 'PreToolUse', tool_name: 'Bash',
    tool_input: { command: e.command }, tool_use_id: e.tool_use_id,
    session_id: await $.session.id(), cwd: await $.session.cwd() })
  const ran = await $.process.run([BIN, 'hook', 'claude', 'PreToolUse'], { stdin: payload, timeoutMs: 5000 })
    .catch(() => undefined)                              // missing binary: fail open
  const out = ran?.exitCode === 0 && ran.stdout ? JSON.parse(ran.stdout) : undefined
  const rewritten = out?.hookSpecificOutput?.updatedInput?.command
  if (rewritten === undefined) return next(e)
  const { decision } = await $.tool.check({ tool: 'Bash', input: { command: rewritten } })
  if (decision === 'deny') return next(e)                // the rewrite would lose an allow rule: advice instead
  $.ui.notice(e.tool_use_id, `adage ↪ ${out.systemMessage}`)
  return next({ ...e, command: rewritten })
})
```

What this layer gives:

- **The permission check that `docs/execution.md` says a hook cannot make.** `$.tool.check` runs
  "the same chain and executes nothing": rules from every settings source, `--allowedTools`, the
  permission mode, the tool's own check. The U11 planner's own reading of the settings files
  becomes a fallback for hosts without a mod, and the "headless run allowed only `Bash(pytest:*)`
  should turn rewrites off" caveat goes away where the mod serves.
- **The rewrite is shown where the person is looking.** `$.ui.notice` puts one line under the
  permission dialog for that very call, instead of a `systemMessage` in the transcript.
- **The failure path in one place.** `await next(e)` on `tool.call` resolves `isError`, so the
  `PostToolUseFailure` event's work can be done by the same hook that saw the call, with the
  call's input and output together (today the batch and the failure arrive as two payloads).
- **A receipt where the result goes.** The hook-action-reuse research found agents re-running
  the test Adage had just run, because the receipt arrived as context beside the result. A
  `tool.call` hook can answer the result itself (`{ result, context }`), which is what the
  `updatedToolOutput` candidate fix does from a `PostToolUse` command hook, in the same place and
  with the receipt's provenance. Whether a repeated identical call should get the fresh receipt
  *as its result* rather than run again is a product decision this spike only makes possible;
  `docs/execution.md` is firm that a receipt "is a fact, never a refusal", and the mod must keep
  that.
- **Hidden directive text at the prompt.** `prompt.submit`'s `context` is read by the model and
  never shown; the transcript view stays the person's own words. `additionalContext` stays as the
  channel for every other host.
- **The session contract as a prompt section.** `prompt.compose` can append the E2 contract as
  one `session`-scoped section with a stable id, so it is present on every request rather than
  only in the first message's context, and cache-stable because its text does not vary within a
  session.

Layer 2 moves the spawn from the harness to the mod. The binary's own cost is unchanged (1.7–6 ms
per event, §9); what changes is the engine's dispatch around it, which is Q2.

### Layer 3 · Measure and attribute with the engine's own figures

- **Cost from the harness.** `turn.complete` carries `usage` (the four token counts "as the API
  spells them"), `session.measure` fires after each main-thread turn, and `$.session.usage()`
  returns `cost` where the host keeps a ledger. A `session.end` hook can write one line to
  `$ADAGE_HOME/ledger/<slug>.jsonl` with `cost.source = "harness"` through the binary
  (`adage ledger note …`, to add), replacing `priced@<date>` for Claude Code sessions. It also
  gives the control arm's cost directly, so the savings estimate's two arms are priced the same
  way.
- **Provenance in the row, not in the text.** A directive delivered with `$.session.append` is
  stored "under `origin: { kind: 'plugin', name }`", and "a later reader of the transcript knows
  who appended it". If the transcript file keeps that origin (Q5), `crates/adage-learn` can mark
  steering from the row's author and keep the `<adage-reflex>` marker as the cross-harness
  fallback. The rule in `how-it-works.md`, that Adage's own actions must never masquerade as
  independent agent evidence, gets a structural guarantee on this harness.
- **Learning trigger with the reason.** `session.end` reports `clear`, `resume`, `logout` and
  `prompt_input_exit`; today's `SessionEnd` payload reaches the binary with the same `reason`
  field, so this is parity, not gain, except that `/clear` is a `session.end` with no
  `session.start` after it, which the mod must handle (its `register` does not run again).

### Layer 4 · Act in the model's place: the request Adage does not send

Everything a command hook can do happens *around* a model request: context before it, a rewrite
of the tool call after it, a result beside it. The request itself is still made, and the model
still spends a turn deciding to do the thing Adage already knew. That is the gap the launch
evidence names: in the transfer pilot "every automatic session nevertheless requested the test
command again after receiving its completion context", and the action-elimination gate "remains
unmet" (`docs/how-it-works.md`). The command-hook API has no event between "the tool results are
in" and "the next request goes out". Mods have one.

**`turn.step` (documented).** It "fires when the engine is about to send a model request of a
turn", carrying `turnId`, `index`, `model`, `effort` and `messageCount`; "`next(e)` resolves to
the whole response", and **"an answer without `next` sends no request"**. A hook on it is an
async generator over the response's chunks: text, thinking, a tool call's start (`tool`: name and
`tool_use_id`) and its arguments (`input`: JSON pieces), and the `stop`. "A hook that yields its
own response has none [of the engine's chunks] to yield, and the engine supplies what the
response needs around the chunks it does yield"; "what leaves the outermost hook is what is shown
and recorded"; a chunk a hook builds "has none [no `ref`] and is taken at its word". A result with
no request made carries `stopReason: null` and `usage: null`, so the cost ledger records zero.
The transcript is not on `e`; `$.session.messages()` reads it in-process.

So a mod can do what no command hook can: at the step after an edit's tool result, when the
binary's context matcher says a `fire` reflex with a current grant predicts `pytest tests/x.py -q`
next, the mod yields a `tool` chunk, its `input` and a `stop` of `tool_use`, and calls no model.
The engine records the call as the turn's, runs it through every `tool.call` hook, the
`PreToolUse` command hook, the person's permission rules and dialog, and then the tool; the next
step's request carries the result, and the model picks up from there. One model request, the
largest single cost in a long session (the whole context as input tokens), is not made.

```ts
on('turn.step', async function* ($, e, next) {
  if (e.agentId !== undefined || e.index === 0) return yield* next(e)   // main loop, not the first step
  const predicted = await predictNext($, e)   // adage hook claude TurnStep on stdin: the recent
                                              // messages' tool uses and results; answers a fire
                                              // reflex's call with a current grant, or nothing
  if (predicted === undefined) return yield* next(e)
  const id = `adage_${e.turnId}_${e.index}`
  yield { kind: 'tool', index: 0, id, name: predicted.tool }
  yield { kind: 'input', index: 0, json: JSON.stringify(predicted.input) }
  yield { kind: 'stop', stopReason: 'tool_use', usage: null }
  return { turnId: e.turnId, index: e.index, answer: '', stopReason: 'tool_use', usage: null,
           toolUses: [{ name: predicted.tool, input: predicted.input }] }
})
```

Three facts make this fit Adage rather than fight it:

- **The authority model is unchanged.** The yielded call runs through the same permission path
  the model's own call would, so a `Bash(pytest:*)` rule, `dontAsk`, the dialog and Adage's own
  `PreToolUse` all still apply. Adage's grant decides *whether to predict*; Claude Code's
  permissions decide *whether it runs*. Nothing executes in the hook, and the E2 sandbox is not
  involved: this is the agent's own tool, as the rewrite path already is.
- **Provenance is structural.** The step's `usage` is null and its `stopReason` is null on the
  engine's side; the tool_use id can carry Adage's prefix; and the `tool.call` hook sees
  `e.tool_use_id`, so the result can be tagged and the learner can read the step as Adage's,
  never as the agent's own evidence (the rule in `how-it-works.md`).
- **The control arm still measures it.** A control session's hook returns `yield* next(e)` and
  nothing else; the spool records `would_have` as it does for a withheld directive, so the saved
  requests are counted against a baseline, never assumed.

What it costs: the model's next request sees an assistant turn it did not write. That is how the
API's own prefill works and models tolerate it, but it is a quality question the demo must
answer, not a safety one. And the matcher's inputs change: today the runtime tracks "bounded
recent context" from hook payloads; `$.session.messages()` gives it the whole conversation,
including the model's text between tool calls, which `PostToolBatch` never carried.

**The lesser levers, same layer.**

- **Route the step, not just skip it.** `next({ ...e, model, effort })` sends the request to
  another model or at another effort. Where a `fire` reflex applies but confidence is below the
  skip threshold, or the predicted call needs its arguments filled from the conversation, the
  step can go to a small model at low effort rather than the session's. `agent.spawn` has the
  same `model` rewrite for subagents. Cost is stated in API dollars, which is Adage's unit; the
  quality rule (hidden tests, the 17/20 against 19/20 that kept execution mode experimental) is
  the gate.
- **Answer the repeated call from the receipt.** `tool.call` may return `{ result }` without
  `next`. When the model does ask for the test Adage just ran on byte-identical inputs, the hook
  answers with the receipt as the tool's result: no 20 s run, a shorter result for the model to
  read, and the "fact, never a refusal" rule kept because the model asked and got an answer. This
  is the `updatedToolOutput` candidate fix from `docs/hook-action-reuse-research.md`, in the
  place that owns the result.
- **Deliver the standing procedures once, cached.** `prompt.compose` (a `session`-scoped section
  with a stable id) and `tool.describe` (the Bash tool's description, "cached for the session")
  carry what does not change within a session: the repository's procedure cards and instincts.
  Today each is `additionalContext` injected again at each matching event and kept in the
  history. A section is one copy, read from the prompt cache; the per-event channel stays for
  what is situational.
- **Survive compaction.** `session.compact` hands the hook the messages about to be summarised
  and the `instructions`; the mod can drop Adage's own `<adage-reflex>` rows from the summariser's
  input (they are re-derivable and would otherwise be relearned as the agent's), and add one
  instruction to keep the procedures the session followed.

### What a mod should not do

- **Hold or grant execution authority.** F11 ("Refuse trust changes inside an agent session") and
  the grant key design stand. `$.ui.ask` is tempting for "approve this reflex?", and the
  `tool.call` input even carries `consent` ("the person's own words for the press that raised the
  call"), but a press in a Claude Code dialog is not the terminal the design requires, and the
  binary's environment checks (`crate::install::AGENT_ENV`) would refuse it anyway. A `/adage
  review` command should print what `adage review` prints and tell the person where to decide.
- **Run the learner or anything long.** `$.process.run` is one shot and waits (30 s default,
  ten minutes at most); a background child belongs to `$.process.spawn`, whose "loop is the
  child's life" and ends with the module. The binary already starts the learner detached from
  `Stop`; the mod leaves that alone.
- **Reveal the arm.** The status line must read the same in a control session as in a treated
  session with no match (both: nothing fired). The model never sees the UI, but the person's
  behaviour is part of what the control measures.
- **Replace the command hooks.** They are the floor: documented, stable across releases, the only
  path on hosts where `$.process` is unavailable, and what Codex's adapter mirrors.

## TUI fit: `adage watch` yes, `adage ui` no

Adage has two terminal programs (`crates/adage-cli/src/ui`, about 10,000 lines of Rust with
tests): `adage watch`, a compact live pane for a split terminal (now, conditioning, health; eleven
keys), and `adage ui`, a full-screen app with five tabs (Live, Reflexes, Map, Health, Settings), a
command palette, review decisions on the Reflexes tab and one-key fixes on the Health tab. Both are
drawn by pure functions from state to frame, with state read from files only (spools, `learn.log`,
`doctor`'s checks, `config.toml`). The question is which of the two a mod can be, and should be.

### What the drawing model is (documented)

A mod draws a **tree of plain data**, not a screen: `Box` (flex layout, borders, padding, absolute
placement), `Text` (colour "a theme key or a raw color", `dimColor`, `bold`, `inverse`, wrap or
truncate), `Button`, `Input`, `Select`, `Link` (https, or `http://localhost`), `Code`, `Markdown`,
and on the terminal `Raster` (a cell grid up to 512 × 256, repainted by `$.ui.blit` at up to 60
frames a second) and `Image`. The engine validates the tree; one it refuses is replaced by its own
drawing and named in the debug log.

A **`Pane`** is the one site a plugin opens. It is seated by the surface: `dock`, floor to ceiling
beside a fullscreen transcript (the terminal from 110 columns once asked, 144 unasked), or
`inline`, a block above the prompt as tall as it asks for ("up to what the layout spares, not a
third" by default). Several open panes are tabs behind one shown pane. The person's size wins over
the plugin's. The hook is given `bodyColumns` and `placement`, and is re-run on a width change, a
props change or a `$.state` write that it read; `$.ui.invalidate` redraws at most ten times a
second, thirty for the shown pane. There is no alternate screen, no takeover, and no second
window: a mod is always *beside* the conversation.

The **keyboard** is a focus ring, not a keymap. Keys reach a pane only while it holds the focus
(opened with `focus`, ctrl+x tab, or a click); Tab and the arrows walk its `Button`s, `Input`s and
`Select`s, Enter presses, Esc returns the keys to the prompt (and closes the pane when opened
`closeOnEscape`). Wheel and page keys move an engine-owned window over the tree (`ui.scroll`). A
`Button` may carry a `hotkey`, one digit or lowercase letter that presses it while the site holds
the focus, and the engine's own keybinding chords can press one from the prompt (`action`). Full
key control exists in one place: a `Client` element, a surface module of the plugin's that draws
its region with its own `onKey`, `onPointer`, local state and frame clock, "a second per call at
most", talking to the hooks module by `post`. That is a small immediate-mode renderer in
TypeScript, which is what ratatui is in Rust.

### Mapping `adage watch`

| `adage watch` today | In a pane | Fit |
| --- | --- | --- |
| Three sections (now, conditioning, health), 60 × 20 and up | `Box` columns with a dim rule each, sized to `bodyColumns`; `rows: 20` asked for inline, the dock as the person sized it | Direct |
| Feed rows: time, session, harness, event, glyph, strength lamp, reflex, command, truncated to width | One `Text` per row, `wrap="truncate-end"`; strength as colour or spelled out; glyphs from the lexicon table | Direct. `NO_COLOR` has no equivalent in the mod; theme keys follow the person's Claude Code theme instead |
| Health lamps re-checked every few seconds | `$.clock.every` or the feed child's own health lines; `●`/`○` with `color` | Direct |
| `p` pause, `f` scope, `v` quiet, `m` map | Four `plain` `Button`s with `hotkey`s `p`, `f`, `v`, `m` in a footer row, pressed while the pane has focus | Close. Hotkeys work only while the pane holds the focus (ctrl+x tab first); from the prompt, `/adage pause` |
| `/` filter, 60 characters | An `Input` with `onInput`, filtering the drawn rows | Direct |
| `j`/`k`, arrows, PgUp/PgDn, `G` | Arrows and page keys scroll the engine's window for free; `j`/`k` as hotkeys on two plain Buttons, or dropped | Partial; `G` becomes `$.ui.scroll({ to: 'end' })` on a Button |
| `Enter` inspects the selected row (the exact directive and its evidence) | Each row a `plain` `Button` whose `onPress` opens a second pane (a tab) drawing the record as `Markdown` and `Code`, or a popup pane `focus`, `closeOnEscape`, `rows: 12` | Direct, and better: the inspected text is selectable in the dock and the popup is a real dialog |
| `m` shows how to open the map | A `Link` to the running `adage map` on `http://localhost:<port>`, one click | Better |
| Pulse animation on a new row | `$.ui.invalidate` every 60 ms for a second, within the 30 a second cap | Direct |
| Follows the spools and `learn.log` by offset, probes the learners' locks twice a second | Not in the mod: `$.fs.read` returns at most 4 MiB and the spool is CRC-framed. The binary does it, as now, and emits one JSON line per change: `adage watch --follow --json`, a session-long child of `$.process.spawn` whose loop ends with the module | The right seam: every parser, lock probe and word table stays in Rust; the mod holds a list of records and draws |

Everything on the watch screen maps, most of it directly. Two things get better (inspect and the
map link), two get worse (single-key bindings need the pane focused first; no `NO_COLOR` or
`--plain` mode, since the engine owns the palette), and nothing is lost that the person cannot
also get from the terminal program, which stays.

### Mapping `adage ui`

| `adage ui` tab | In a mod | Fit |
| --- | --- | --- |
| Live | The watch pane above | Direct |
| Reflexes: every reflex by track, fuzzy filter, strength and lifecycle filters, evidence, directive text | A second pane fed by `adage graph --json`: a tree of `Button` rows, an `Input` filter, `Markdown` detail. Read-only | Good |
| Reflexes: `a`pprove, `e`xtinguish, `w`eaken, `s`trengthen, pi`n`, dismiss, `d`elegate, which "run exactly what `adage review` runs" after asking | **Blocked by Adage's own rule, not by the API.** A `$.process.run` child inherits the engine's environment, and `CLAUDECODE` is set there: `adage review`, `delegate`, `install --write` and `config set` for the sensitive keys refuse under `AGENT_ENV` (F11, `crates/adage-cli/src/install.rs`). A mod could pass `env` without it; doing so is the trust change inside an agent session the design refuses, so the pane shows the equivalent command and `$.ui.copy` puts it on the clipboard | Out, until there is "a design for a person-verified approval inside the harness" (`docs/zero-config.md`). Mods supply the primitive that design would need (a `ui.press` only a person can make, never the model; a mod must then register no tool that acts) but the design is a separate decision |
| Map: stimulus → reflex → response as a layered graph; `o` opens it in the browser | A `Raster` can draw the graph; a `Link` opens `adage map` | Possible; low value against the browser map one click away |
| Health: every doctor check; `Enter` fixes the selected one (install hooks after confirming the diff, fetch the model, condition now, open the settings, open the fail-open log) | Checks draw fine from `adage doctor --json`. Of the fixes, `condition now` is allowed from anywhere; `install hooks` and the settings that need a person are refused as above; `fetch the model` is a download the person should start themselves | Half: show every check, fix only what the binary allows from a session |
| Settings: toggle the keys that "only tune Adage or turn something down" | `userConfig` rows in Claude Code's own `/config` menu (`notices`, `rewrites`, `control`), each change reloading the mod; `config set` from the mod for keys allowed from a session | Good, and native |
| Command palette over every action and reflex | `/adage <verb>` commands registered with `$.command.register`, the typeahead as the palette | Different, native |
| The `init` wizard | Stays in the terminal | Out |

The full-screen app does not map, for two reasons that are independent of each other. The
drawing model is a sidebar, not an application: one shown pane, a focus ring, no keymap, a third
of the height inline. And the actions that make `adage ui` more than a viewer are person-only by
Adage's design, so a pane that offered them would either refuse at the press or quietly weaken
F11. Neither reason argues against the watch pane; both argue for keeping `adage ui` as the
place decisions are made.

### The verdict, and what makes the port cheap

A mod is a good fit for **the watch pane and the status line**, a fair fit for a **read-only
reflexes browser**, a native fit for **settings and commands**, and the wrong fit for **review,
delegation and setup**. The thing that makes the port cheap is already in the repository: the
view is a pure function of state the binary reads from files, so the split is state in Rust
(`adage watch --follow --json`), view in TypeScript, and the Rust frame tests have a twin in
`claude plugin test`'s `ui.mount` over `terminal` and `desktop`. The thing to decide first is the
colour rule: the style guide says the terminal's own colours win, and in a pane the engine's
theme keys are that rule; Adage's one accent would come from `cargo xtask tokens` writing a
`tokens.ts` beside `tokens.rs`, never a hex literal in the module.

Two deeper hooks are worth one experiment each, since they put Adage's output *on the row it is
about* rather than beside it, which no command hook can do:

- `ui.render` on `ToolUse` for a Bash call Adage rewrote: wrap `next(e)` and add one dim `↪` line
  under the tool row naming the interpreter word that changed. The person sees the rewrite where
  the command is, and the transcript keeps it.
- `ui.render` on `UserMessage` for a prompt a reflex fired at: one dim line under the person's
  prompt, `◠ lookup-work-item · fire`, drawn from what the `classic.UserPromptSubmit` hook saw
  and held in `$.state` under the prompt's id. The directive text itself stays hidden, as now.

## Method

Isolated as S5 was: `--plugin-dir` on a copy of `plugin/claude-code` with the mod added, a
throwaway `CLAUDE_CONFIG_DIR` where it can authenticate, otherwise the real config with nothing
installed. Measurements use `crates/adage-cli/examples/hook_latency.rs`'s method (release binary,
500 runs after 20 warm-ups) where latency is the question. Every number goes in `results/`, every
script in the folder, per the spikes README.

| # | Question | How | Pass |
| --- | --- | --- | --- |
| Q1 | **Coexistence.** The docs settle the shape: `hooks/hooks.json` "can also hold settings hooks under `hooks`" beside `modules` (reference, Files), the plugin's own `PreToolUse` command hook runs "after the last mod calls `next`, as part of Claude Code's own behavior", and a mod that answers `tool.call` without `next` "keeps them from running" (events, order). To measure: that `next(e)` on each `classic.*` event resolves to the command hook's `additionalContext` once; that a `tool.call` rewrite followed by the `PreToolUse` command hook is deduplicated by the binary's `tool_use_id` identity (to add for `PreToolUse`), with one spool record; whether the result exposes `systemMessage` | A probe mod that logs `e` and `next(e)`'s result per event to a file under the spike folder; one `-p` run exercising every event (S5's `probe-prompt.txt`) | No event delivered twice to the model and none lost; one spool record per `tool_use_id`; each event's result shape recorded verbatim |
| Q2 | **Latency.** Layer 1: the engine's dispatch cost of an observing hook over the command hook, at no-match and at a match. Layer 2: `$.process.run` of the binary from `tool.call` and `prompt.submit` against the command hook, end to end as the model sees it | `hook_latency`-style runs; `claude --debug` timestamps for the engine side | Layer 1 adds ≤ 1 ms p95; Layer 2 stays inside §9's budget (≤ 10 ms p95 no-match, ≤ 25 ms with intent), else Layer 2 is dropped |
| Q3 | **Surfaces and silence.** `$.ui.status` and a `Pane` draw on the terminal (main screen and fullscreen) and the desktop app; `claude plugin test` mounts the pane on `terminal` and `desktop`; a `-p` run and a control session show nothing and change no output | An interactive session per surface; `plugin test`; a `-p` run diffed against one without the mod | Identical model-facing output with and without the mod in `-p`; a pane and status line on both surfaces |
| Q3b | **The watch pane's feed and keys.** A `$.process.spawn` child (`adage watch --follow --json`, a stub for the spike) lives the session and ends with the module; the pane draws 200 rows at 2 Hz under the redraw cap; hotkeys `p`/`f`/`v`, an `Input` filter and Enter-to-inspect behave as the table in [TUI fit](#tui-fit-adage-watch-yes-adage-ui-no) says; the `ToolUse` and `UserMessage` row decorations draw and the transcript keeps them | The probe mod with the demo spools (`adage watch --demo`) as its feed; `plugin test` for the keys | No dropped rows at 2 Hz; the child is gone after `/clear` and exit; every row of the mapping table measured as Direct, Close or Out |
| Q4 | **`$.tool.check` for rewrites.** With `Bash(pytest:*)` allowed in settings, and separately with `--allowedTools "Bash(pytest:*)"` on the command line, does `$.tool.check` on the rewritten `.venv/bin/python -m pytest …` answer `deny`/`ask` where the U11 planner's own reading answers "would lose the rule", and `allow` where a rule for `.venv/bin/python` exists? | Four settings fixtures × the probe mod; compare with `adage_runtime::pretool`'s verdict on the same inputs | `$.tool.check` agrees with the planner on every settings-file case and answers the command-line case the planner cannot see |
| Q5 | **Provenance and cost.** A row appended with `$.session.append` carries `origin.plugin` in the transcript `.jsonl` the learner reads; `turn.complete.usage` and `$.session.usage().cost` match the transcript's own token counts and the ledger's priced estimate within pricing error | Append one row, read the transcript file; compare figures for three sessions | `origin` is on disk in a stable field; usage agrees within 1 % of the transcript's counts |
| Q6 | **Fail-open and reload.** A hook that throws, one that sleeps past 10 s, a missing binary, a syntax error in the module: the session continues and the command hooks still serve; a hot reload re-runs `register` without losing `$.state`; `claude plugin validate --strict` passes on the real plugin folder with the module added | Fault injection in the probe mod; `--debug` log lines | Every fault leaves the command hooks' output unchanged and leaves one debug line naming the mod |
| Q7 | **Hosts.** The overview's table is documented: hooks run in the terminal, the Desktop app's Code tab (not WSL), the VS Code chat panel, `-p`, the SDK, Remote Control and cloud sessions; drawing only in the terminal and the Desktop app. Left to measure: `$.process` on the Desktop app ("CLI only" in the declaration, unrestricted on the API page); on Windows, whether the mod loads and can run `adage.exe`, where the `sh` shim is unverified | One session per host, the probe mod logging `$.session.surfaces()` and a `$.process.run(['adage','version'])` result | A table of hosts with yes/no per noun; Windows and the Desktop app measured, not inferred |
| Q8 | **Drift.** The reference points at the public copy of the declaration file, `mods/types/claude-code.d.ts` in the Claude Code repository, so churn is its git history: count the renamed or removed events, props and nouns the sketches here use between the releases since mods shipped, and against 2.1.221 (the spikes' pinned environment) | `git log -p` on that file, grep for each name | A churn figure per release that sets how the CI gate (Q6's `validate --strict` against a pinned CLI version) must be run |
| Q9 | **The request not sent.** A `turn.step` hook that yields a `tool`, `input` and `stop` chunk and returns without `next`: is the call recorded in the transcript, run through `tool.call`, `PreToolUse` and the permission dialog, and does the loop continue with its result? Does the result carry `usage: null`, and does the session's cost stay flat for that step? Does the model's next step read the fabricated assistant turn as its own and carry on? | The probe mod on the acme-billing sandbox (`demo/`), predicting the `make gen` after a `registry/*.csv` edit and the `make test` after a source edit from a fixed table; `claude --debug`; the transcript `.jsonl` | The call runs under the person's rules, the step costs zero, and the next model step proceeds in 10 of 10 runs; otherwise Layer 4 is out |
| Q10 | **Does it move the gate?** The transfer pilot's measure: model requests per task and hidden-test quality, treated against control, with Layer 4 predicting only what today's E2 grants cover | `demo/run.py all` with the probe mod in the treated arm, Haiku, the four held-out tasks | Fewer model requests than the 0.67× execution mode reached, with hidden tests at the baseline's 19/20; a quality drop is a fail whatever the saving |
| Q11 | **Routing.** `next({ ...e, model: 'haiku', effort: 'low' })` on steps the matcher marks routine: cost per task and quality against the unrouted arm | The same runner, a second treated arm | Reported as numbers; no pass bar, since the quality rule decides |
| Q12 | **Noticed, on the fast path.** A `candidates` table of sub-support sequences in the index: what it adds to the no-match path (the 1.76 ms p95 of §9) and to a `PostToolBatch` with a context match; how often a `noticed` line would draw per session over the demo's seeded histories, and whether a person reads it as learning or as noise | `hook_latency` with candidates compiled for the acme-billing fixture; the twelve seeded sessions replayed; one person's read of the recording | ≤ 0.3 ms added at no match; at most one line per candidate per session; nothing for a single call |
| Q13 | **Skills.** A `turn.step` hook that yields a `Skill` tool call: does the engine run the Skill tool, does the skill's text enter the transcript as its result, and does the model's next step proceed with it loaded? Does a `skill.prompt` rewrite reach the model for a `/name`, a Skill tool call and a subagent preload alike? Is a Skill tool use recorded in the transcript `.jsonl` so the learner can count it? | The probe mod with a fixture skill in `.claude/skills/`, in `-p` and interactive; `claude --debug`; the transcript file | The skill's text appears once as the tool result and the next step cites it in 10 of 10 runs; the rewrite is what the model reads at all three entry points; Skill tool uses are on disk with their inputs |

## Pass / fail

The spike passes if Q1, Q2 (Layer 1), Q3 and Q6 pass: an observing mod can be shipped in
`plugin/claude-code` as an additive file, costing nothing measurable, silent in headless and
control sessions, failing open, and validated in CI. The watch pane ships only if Q3b passes.
Layer 2 ships only if Q2 (Layer 2) and Q4 pass. Layer 3 ships only if Q5 finds the origin on
disk. Layer 4 is the spike's real bet: Q9 says whether the mechanism exists as documented, and Q10
whether it moves the launch gate that execution mode could not. It ships only behind the same E2
grants, policy and control arm as any action, and only with Q10's quality at the baseline.
Review, delegation and setup do not ship in a pane whatever the spike finds; that needs the
approval design named above, which is not this spike's question.

The spike fails, and the command hooks remain the whole integration, if a mod cannot coexist
with the plugin's command hooks without double delivery (Q1), or if `$.process` is unavailable in
the terminal CLI (Q7), or if the dispatch cost breaks §9's budget (Q2).

## What it would change in the repository, if it passes

- `plugin/claude-code/hooks/register.ts`, `types/index.d.ts` and `*.test.ts` beside
  `hooks.json`, which gains `"modules": ["./register.ts"]`; `.claude-plugin/plugin.json` gains
  `"types"`. `adage-exec`'s plugin gets the same module (it replaces the hooks, not the UI).
- `scripts/check-plugin.mjs` learns the module path; CI adds `claude plugin test
  plugin/claude-code` and keeps `claude plugin validate --strict`, both against a pinned CLI
  version (Q8 says how often it must move).
- `adage watch --follow --json` (the spools, `learn.log` and the health lamps as JSON lines, from
  the same `tail`, `learnlog` and `health` modules the terminal pane uses) is the pane's feed;
  `adage graph --json` and `adage doctor --json` the reflexes and health views; a `adage ledger
  note` subcommand takes a harness-priced line (Layer 3). `cargo xtask tokens` writes a
  `tokens.ts` beside `tokens.rs`. No new crate; nothing on the hook path changes.
- `docs/harnesses.md` gets a "Claude Code mod" section marked documented/inferred/measured like
  the others; `docs/live-learning.md` records the harness cost source; Architecture §15.2 F14's
  "status line" moves to built.
- The lexicon and the style guide apply to every string the mod draws (`◠`, `↪`, `»`, one accent,
  lamps only for strength). The directive text the model reads stays plain and factual, as today.

## Deliverables

`FINDINGS.md` with each question answered from the probe logs and timings, the measured recipe
for coexistence (Q1), the host table (Q7), the drift figure (Q8), and a recommendation per layer.
The probe mod, its tests and `rerun.sh` stay in this folder; nothing is installed and nothing is
committed from inside a session.
