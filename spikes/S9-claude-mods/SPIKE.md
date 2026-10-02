# S9 · Claude Code mods: an in-process plugin beside the command hooks

**Informs:** A3 (transport), A8 (packaging), A9 (harness layer), F3 (delivery channel per decision
point), F14 (the status line and `suggest` mode, listed as not built) · **Wave:** 3 ·
**Budget:** two days, ≤ 20 short `claude -p` runs on Haiku, one interactive terminal session per
question that needs a person at the prompt

Research date: **2026-10-02**, against Claude Code **2.1.287**. Every claim about mods below is
marked **documented** (stated in the `claude-code` declaration file that build writes for a mod,
`types/claude-code.d.ts`, about 20,000 lines, or in the `plugin-authoring` skill that ships with
the CLI), **inferred** (our reading of those sources, not run), or **to verify** (what this spike
measures). Nothing here was run against a live Claude Code yet; that is the point of the spike.
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

The thesis: **the binary decides, the mod shows and measures.** Three layers, each optional, each
leaving the one beneath untouched.

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

- **The status line F14 asks for.** One pinned line under the prompt, replaced at each decision:
  which event, how many reflexes, a rewrite (`↪`), a receipt, the finish line. Cleared when the
  session is in the control arm or no route matched (the binary prints nothing; the mod sees an
  empty `additionalContext` and clears the line). Words and glyphs follow the lexicon and the
  style guide, because the text is for the person; the directive text the model reads is never
  touched.
- **`adage watch` inside Claude Code.** A `Pane` (docked from 144 columns in the fullscreen
  terminal layout, inline below that, or opened by `/adage watch`) that draws what `adage watch
  --once` draws, from `adage status --json` and `adage savings --json` on a `$.clock.every`
  timer, read through `$.process.run`. No split terminal, no second program. On the desktop app
  and VS Code the same tree draws with their element tables.
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

## Method

Isolated as S5 was: `--plugin-dir` on a copy of `plugin/claude-code` with the mod added, a
throwaway `CLAUDE_CONFIG_DIR` where it can authenticate, otherwise the real config with nothing
installed. Measurements use `crates/adage-cli/examples/hook_latency.rs`'s method (release binary,
500 runs after 20 warm-ups) where latency is the question. Every number goes in `results/`, every
script in the folder, per the spikes README.

| # | Question | How | Pass |
| --- | --- | --- | --- |
| Q1 | **Coexistence.** Can `hooks/hooks.json` carry both `hooks` and `modules`? When a mod hooks `classic.UserPromptSubmit` and calls `next(e)`, does the plugin's own command hook run once beneath and its `additionalContext` arrive in the result? Can a mod answer without `next` and suppress it, so Layer 2 never double-delivers? Does the result expose `systemMessage`? | A probe mod that logs `e` and `next(e)`'s result per event to a file under the spike folder; one `-p` run exercising every event (S5's `probe-prompt.txt`) | A written recipe with no event delivered twice and no event lost, each hook event's result shape recorded verbatim |
| Q2 | **Latency.** Layer 1: the engine's dispatch cost of an observing hook over the command hook, at no-match and at a match. Layer 2: `$.process.run` of the binary from `tool.call` and `prompt.submit` against the command hook, end to end as the model sees it | `hook_latency`-style runs; `claude --debug` timestamps for the engine side | Layer 1 adds ≤ 1 ms p95; Layer 2 stays inside §9's budget (≤ 10 ms p95 no-match, ≤ 25 ms with intent), else Layer 2 is dropped |
| Q3 | **Surfaces and silence.** `$.ui.status` and a `Pane` draw on the terminal (main screen and fullscreen) and the desktop app; `claude plugin test` mounts the pane on `terminal` and `desktop`; a `-p` run and a control session show nothing and change no output | An interactive session per surface; `plugin test`; a `-p` run diffed against one without the mod | Identical model-facing output with and without the mod in `-p`; a pane and status line on both surfaces |
| Q4 | **`$.tool.check` for rewrites.** With `Bash(pytest:*)` allowed in settings, and separately with `--allowedTools "Bash(pytest:*)"` on the command line, does `$.tool.check` on the rewritten `.venv/bin/python -m pytest …` answer `deny`/`ask` where the U11 planner's own reading answers "would lose the rule", and `allow` where a rule for `.venv/bin/python` exists? | Four settings fixtures × the probe mod; compare with `adage_runtime::pretool`'s verdict on the same inputs | `$.tool.check` agrees with the planner on every settings-file case and answers the command-line case the planner cannot see |
| Q5 | **Provenance and cost.** A row appended with `$.session.append` carries `origin.plugin` in the transcript `.jsonl` the learner reads; `turn.complete.usage` and `$.session.usage().cost` match the transcript's own token counts and the ledger's priced estimate within pricing error | Append one row, read the transcript file; compare figures for three sessions | `origin` is on disk in a stable field; usage agrees within 1 % of the transcript's counts |
| Q6 | **Fail-open and reload.** A hook that throws, one that sleeps past 10 s, a missing binary, a syntax error in the module: the session continues and the command hooks still serve; a hot reload re-runs `register` without losing `$.state`; `claude plugin validate --strict` passes on the real plugin folder with the module added | Fault injection in the probe mod; `--debug` log lines | Every fault leaves the command hooks' output unchanged and leaves one debug line naming the mod |
| Q7 | **Hosts.** Where is `$.process` available ("CLI only" in the declaration): the terminal, the desktop app's Code tab, VS Code, `-p`, the SDK. On Windows, does the mod load and can it run `adage.exe`, where the `sh` shim is unverified | One session per host, the probe mod logging `$.session.surfaces()` and a `$.process.run(['adage','version'])` result | A table of hosts with yes/no per noun; Windows measured, not inferred |
| Q8 | **Drift.** Diff `types/claude-code.d.ts` as written by 2.1.221 (the spikes' pinned environment) and 2.1.287; count the renamed or removed events and nouns the sketch above uses | `diff`, grep for each name | A churn figure per release that sets how the CI gate (Q6's `validate --strict` against a pinned CLI version) must be run |

## Pass / fail

The spike passes if Q1, Q2 (Layer 1), Q3 and Q6 pass: an observing mod can be shipped in
`plugin/claude-code` as an additive file, costing nothing measurable, silent in headless and
control sessions, failing open, and validated in CI. Layer 2 ships only if Q2 (Layer 2) and Q4
pass. Layer 3 ships only if Q5 finds the origin on disk.

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
- `adage status --json` and `adage savings --json` are the pane's data; a `adage ledger note`
  subcommand takes a harness-priced line (Layer 3). No new crate; nothing on the hook path
  changes.
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
