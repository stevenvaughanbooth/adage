# P3 · The mod: bridge, observers, `turn.step` (POC of B1 + C4, with C7 and C8 in the generator)

Built against Claude Code **2.1.288** (`claude --version`), the version CI now pins. Every claim
below about the API is read from that build's own declaration file (written by the
`plugin-authoring` skill: `types/claude-code.d.ts`) or observed through `claude plugin validate`
and `claude plugin test`; nothing here ran in a live interactive session.

## What was built

| File (both plugins; the exec copy differs only where noted) | What it is |
| --- | --- |
| `hooks/hooks.json` | The seven command hooks unchanged, plus `"modules": ["./register.ts"]` |
| `hooks/register.ts` | The only file that touches `$`. `session.start` and `classic.SessionStart` probe the binary; four classic observers; the `turn.step` generator. Exec copy: the two `$.state` refs name `adage-exec` |
| `hooks/bridge.ts` | `ALLOWED_VERBS = ['hook', 'version']`, `isAllowed`, `guard(run)` (allow-list + `CLAUDECODE=1`), `dataDirFromRoot`, `hookBinary`, `probe`, `ask` (tagged `{ok:true, answer}` / `{ok:false}`) |
| `hooks/step.ts` | `turnStep(e, next, deps)`: the generator body (before-request prediction, forwarding, the finish line) |
| `hooks/predict.ts` | Pure: `predictionOf` (one dispatch table entry per kind: `run`, `obligation`, `sequence`, `skill`, `chain`), `toolUseId`, `callChunks`, `predictedResult` |
| `hooks/reflex.ts` | Pure: `parseReflexes` over `<adage-reflex …>` opening tags (route, track, kind, strength) |
| `hooks/plugin.ts` | `BIN = 'adage'` (exec copy: `'adage-exec'`) and the state value types |
| `types/index.d.ts` | `PluginState.adage = { fired: AdageFired[]; predicted: AdagePredicted[] }` (exec: key `'adage-exec'`) |
| `.claude-plugin/plugin.json` | `"types": "./types/index.d.ts"` |
| `hooks/*.test.ts` | 41 tests per plugin: bridge, predict, reflex, step (stubbed ask and stream), register (engine end to end) |
| `plugin/claude-code/VALIDATE.txt` | `claude plugin validate --strict plugin/claude-code`, the repository path stripped |
| `scripts/check-plugin.mjs` | hooks.json keys limited to `hooks` and `modules`; one module, `./…` inside hooks/, must exist; `plugin.json` `types` must exist; the `ALLOWED_VERBS` literal must hold only mod architecture §2's verbs; the CI install of the CLI must be pinned to an exact version; both plugins ship the same set of hooks/*.ts files. Every earlier rule kept |
| `.github/workflows/ci.yml` (plugin job) | `npm install -g @anthropic-ai/claude-code@2.1.288`; a `VALIDATE.txt` diff step; `claude plugin test` for both plugins |

### Behaviour

- **Activation.** `bin = <data>/bin/<BIN>` where `<data>` is `$.env.get('CLAUDE_PLUGIN_DATA')`,
  else derived from `$.plugin.root` (`<config>/plugins/cache/<mkt>/<plugin>/<ver>` →
  `<config>/plugins/data/<plugin@mkt sanitized>`). No PATH fallback at all. Probe:
  `[bin, 'version']`, 1000 ms, `CLAUDECODE=1`; active iff exit 0 and non-empty stdout
  (`scripts/quiet` prints nothing → inert). Probed at `session.start` **and again after
  `classic.SessionStart`'s command hooks ran** (the shim makes the link; see open issue 2).
- **Inert.** Every observer returns `await next(e)` unchanged and writes nothing; `turn.step`
  is `yield* next(e)` without asking.
- **Observers** (`classic.UserPromptSubmit`, `PostToolBatch`, `PostToolUseFailure`, `Stop`):
  `await next(e)`, parse `additionalContext[]` and `block` for `<adage-reflex>` tags, append to
  `$.state` `fired` with `update()`, return the answer object unchanged.
- **`turn.step`.** Skipped (`yield* next(e)`) when inert, `e.agentId` set, or `e.index === 0`.
  Otherwise asks `TurnStep` (250 ms) with
  `{session_id, cwd, hook_event_name:'TurnStep', turn_id, index, model, ending:false}`.
  A valid prediction → per call `{kind:'tool', index:k, id, name}` + `{kind:'input', index:k,
  json}` (k = 0..n-1, id `adage_<route>_<turnId>_<index>_<k>`), then `{kind:'stop',
  stopReason:'tool_use', usage:null}`, return `{turnId, index, answer:'', toolUses, stopReason:
  'tool_use', usage:null}`; `next` is never called (no request). Nothing / `{ok:false}` /
  malformed / unknown kind → the request goes out and every chunk is forwarded.
- **Finish line (`ending: true`).** While forwarding, the stop chunk is held. If it is
  `end_turn` and the response had no `tool` chunk, the binary is asked again with
  `ending: true`. A prediction then yields the calls at block indexes after the model's last
  block, then the model's stop with `stopReason: 'tool_use'` (its `usage` kept), and returns the
  model's result with the calls appended to `toolUses` and `stopReason: 'tool_use'`. The model's
  text chunks have already passed through unchanged. Nothing predicted → the stop is yielded as
  it came.
- Each yielded prediction is appended to `$.state` `predicted` (`turnId, index, route, kind,
  ids, isEnding`) for drawing later.

## What the API offered vs what the design assumed

| Design assumed | 2.1.288 actually has |
| --- | --- |
| `ask($, event, payload, timeoutMs)` in `bridge.ts`, `hookBinary($)` | **`claude plugin validate` refuses a module that passes `$` across an import** ("`$` is followed only into a function declared in this same file, never across an import"), or into a function not declared at the top level of the file ("not a function declared at the top of this file (a function declaration, or a const bound to one)"), and a hook that is not a function literal at the `on(...)` call ("the hook is not a function literal or the name of one"). So `bridge.ts` takes a runner: `ask(run, bin, event, payload, timeoutMs)`; `register.ts` has top-level `runner($)` (`guard((argv, init) => $.process.run(argv, init))`) and `askBinary($, event, payload, timeoutMs)`, the design's signature. `step.ts` gets closures over `$`, never `$` |
| `$.state.update(s => …)` | No such method. `$.state.get(ref)` / `$.state.set(ref, value, {ifVersion})`, and `update($, ref, fn)` imported from `'claude-code'` (read, apply, write with `ifVersion`, retry). Refs must be **literal consts in the module file** ("a reference whose plugin and key are string literals … written there or in a const of this file"), so the plugin name is spelled in `register.ts` and the exec copy differs there |
| `turn.step` payload from `e` incl. session and cwd | `TurnStepInput` is `{turnId, index, model, effort?, messageCount, agentId?}`, no session id, no cwd. **Session id: `$.session.id()`** ("the session's id (the transcript file's name)"); cwd: `$.session.cwd()`. Both read at each step (no `classic.SessionStart` bookkeeping needed) |
| `ending` known before the request | **Not observable before the request.** `TurnStepInput` says nothing about how the response will end. It is observable *after*: `next(e)` yields `{kind:'stop', stopReason, usage}` before the hook yields it, so the hook can hold it and replace it. Implemented: `ending:false` is asked before every request (index > 0); `ending:true` is asked only after an `end_turn` stop with no tool chunk, and its prediction keeps the model's text (C8's shape). A finish-line prediction therefore does not save the request that ended the turn; it saves the Stop gate's extra request |
| `toolUses: [{ id, name, input }]` | `TurnStepToolUse` is `{ name, input }` only; the id travels on the `tool` chunk |
| Chunks `tool`/`input`/`stop` | As assumed: `tool {kind, index, id, name}`, `input {kind, index, json}` (partial JSON pieces, concatenated), `stop {kind, stopReason, usage}`, plus `text`, `thinking`, and opaque `engine {kind:'engine', ref}`. Chunks read from the engine carry `ref`; a spread keeps it, a fresh chunk "is taken at its word" |
| `$.process.run(argv, {stdin, cwd, timeoutMs, env})` | As assumed: `ProcessRunInit = {cwd?, env? (over the host's), stdin?, timeoutMs? (30 s default, 10 min max)}`; result `{exitCode, stdout, stderr, isStdoutTruncated, isStderrTruncated}`; rejects when the command cannot start or times out. A truncated stdout is treated as `{ok:false}` |
| Classic answers | `classic.<Event>` results are `{additionalContext?: string[], block?: string, …}` (per-event subsets); `$.env.get` takes a literal name only |
| `CLAUDE_PLUGIN_DATA` in the engine's environment | **Unverified.** The CLI documents `${CLAUDE_PLUGIN_DATA}` as a hook-command substitution; nothing in the types says the engine process carries it. The fallback derives it from `$.plugin.root` using the layout the CLI binary's strings show (`~/.claude/plugins/data/{id}/`, ids sanitized with `[^a-zA-Z0-9\-_]` → `-`); the exact id form (`plugin@marketplace`, case) is inferred, not observed. A `--plugin-dir` checkout gets neither, so the mod is inert there unless the variable is set |
| Can `claude plugin test` stub `$.process.run`? | **Yes.** The test's `on` hooks sit beneath the plugin, and op events are hookable: `on('process.run', ($, e) => ({ value: ProcessRunResult }))` or `{ deny }`; likewise `session.id`, `session.cwd`, `env.get` (`mock.env`), and `state.set` (calling `next(e)` reaches the engine's store, so writes are observable). The test's `$` raises `$.session.start(…)`, `$.classic.<Event>(…)` and `$.turn.step(input)` (a `HookStream`); a test `on('turn.step', async function* …)` plays the model. Test files may import the plugin's own `.ts` files, so the pure modules are unit-tested directly |

## Commands run (in /home/user/adage-poc)

| Command | Result |
| --- | --- |
| `claude --version` | `2.1.288 (Claude Code)` |
| `claude plugin validate --strict plugin/claude-code` | `√ Validation passed`; hooks: session.start, classic.SessionStart, classic.UserPromptSubmit, classic.PostToolBatch, classic.PostToolUseFailure, classic.Stop, turn.step; calls `$.env.get (via activate)`, `$.process.run (via runner)`, `$.session.cwd`, `$.session.id`, `$.state.get`, `$.state.set`; env reads `CLAUDE_PLUGIN_DATA`; state `adage.fired`, `adage.predicted` (committed as `VALIDATE.txt`) |
| `claude plugin validate --strict plugin/claude-code-exec` | `√ Validation passed` (state `adage-exec.fired`, `adage-exec.predicted`) |
| `claude plugin validate --strict .` | `√ Validation passed` (marketplace) |
| `claude plugin test plugin/claude-code` | `41 pass, 0 fail` across 5 files |
| `claude plugin test plugin/claude-code-exec` | `41 pass, 0 fail` across 5 files |
| `tsc -p <scratch tsconfig>` over hooks/ and types/ of each plugin with the 2.1.288 declarations | no errors |
| `node scripts/check-plugin.mjs` | `plugins adage, adage-exec 0.1.0: ok` (before the CI pin it failed on the unpinned install, as intended; with the exec `register.ts` removed it reported the missing module and the file-set mismatch) |
| `node scripts/test-stage-plugin.mjs` | exit 0, `plugin archive integrity: verified pair accepted; …` |
| the CI diff step locally (`validate … \| sed "s#$PWD/##g"` vs `VALIDATE.txt`) | no difference |

Earlier iterations of `validate` failed on the three `$` rules quoted above; each was fixed by
restructuring, not by suppressing anything.

## Open issues

1. **`adage-exec version` does not exist yet** (mod architecture §2, plan A6). The exec copy probes
   `[adage-exec, 'version']`; until A6 lands, that copy is always inert (and its correctness, one
   active copy with both plugins installed, depends on that verb printing a line).
2. **Order of `session.start` and the SessionStart command hook is unknown.** If the mod's
   `session.start` runs before the shim, the first session has no link; the re-probe after
   `classic.SessionStart` covers it (tested), at the cost of one extra `version` run per start.
   A live session should confirm which fires first, and whether `classic.SessionStart` fires
   for plugin command hooks of this plugin in that order.
3. **`CLAUDE_PLUGIN_DATA` in the engine process, and the derived data path,** are unverified
   (above). B1's Q1 check should settle it; if neither holds, the mod needs another way to find
   the link (for instance the shim writing the path into `$ADAGE_HOME`).
4. **The finish line replaces the engine's own stop chunk** by spreading it with
   `stopReason: 'tool_use'` (its `ref` kept). Whether the engine records the message's
   stop_reason from that chunk or from an `engine` chunk it streamed (the API's `message_delta`)
   is not observable in the test kit; Q15 must check the transcript on disk.
5. **One extra binary call per turn end** (`ending: true` after every tool-free `end_turn` at
   index > 0), plus one per step before the request: both within the 250 ms budget, which the
   binary's nothing-pending path must meet (C1).
6. **Not in this POC:** `routine` model routing, drawing, the feed, `cliBinary`, the
   person-only `press()` path, `userConfig`. The allow-list is `hook`, `version` only. Closed in
   P5 (`P5-notes.md`): the `$.command.list()` check before yielding a `Skill` call, and
   `ADAGE_MOD_OBSERVE` (observe mode).
7. **Step 0 is never predicted, including its finish line.** A turn whose first response ends it
   with an armed obligation falls back to the Stop gate, as today.
8. The `adage_` id writes `_` inside a route or turn id as `-` so the four `_` separators stay
   unambiguous; the binary/sim side (C4's `rows.tsx` marker, the learner's `by`) must parse ids
   with that rule.
9. `plugin/claude-code/hooks/*.test.ts` ship in the staged plugin (stage-plugin copies the whole
   directory); harmless (only `register.ts` is a module), but they could be filtered.
