# The mod's structure

How `plugin/claude-code/hooks` is laid out, why, and where each kind of change goes. The design
is [architecture.md](../architecture.md) (the mod bridges and draws; the binary decides); the
words are [ux-design.md](../ux-design.md)'s copy deck. `scripts/check-plugin.mjs` enforces every
rule below that can be read from the source; the page says which.

## How to add X

Each change is one place in the code plus a test; the third column is what checks it.

| To add | Where | Test, and what holds it |
| --- | --- | --- |
| A hook event | An exported `on<Name>(m: Mod, e, next)` in the module that does the work (typed with `EventAt`, `NextAt`, `AnswerAt` from `mod.ts`), and one line in `register.ts`: `on('<event>'[, matcher], async ($, e, next) => <module>.on<Name>(mod($, options), e, next))`, placed by nesting (registrations nest first outermost), then `VALIDATE.txt` regenerated | `<module>.test.ts`, through the engine (`fixtures/kit.ts` `world`). check-plugin: every `on(...)` is one such line, its function exists and is exported, and the order is `VALIDATE.txt`'s, which CI diffs |
| An act on the feed | A case in `feed.ts` `parseLine` (and `Line`), what it changes in `fold`, and what it earns in `follower.ts` `apply` (a toast, a marker); its words in `view/pane.ts` `actText` | `feed.test.ts` (the line, from a fixture in `kit.ts` `LINE`), `follower.test.ts` |
| A render site | An exported `on<Component>` in `draw/sites.ts` (the words in `view/*.ts`, the tree in `draw/*.tsx`), and its `on('ui.render', { component: … }, …)` line in `register.ts` | `draw/<tree>.test.ts` mounted on both surfaces (`SURFACES`, T-X-07); `draw/sites.test.ts`'s T-B2-10 sweep for roles and banned words. check-plugin as for a hook event |
| An ask kind | Its `Waiting['kind']` and item in `view/pane.ts` `waiting`, its question and answers in `ask.ts` `question` (`ANSWERS`), and what its first answer does in `dialog.ts` `answerYes` (a press through `token.ts` `pressCommand`) | `ask.test.ts` (the words), `dialog.test.ts` (the ask through the engine, T-B4-01, T-B4-02) |
| An allow-listed verb | Its head in `bridge.ts` `ALLOWED_VERBS` and its one exact shape in `isAllowed`; a person-only verb instead goes in `press.ts` `PRESS_VERBS` and `isPressAllowed` | `bridge.test.ts` (T-X-04's shapes), `press.test.ts`; check-plugin holds both literals to architecture §2's lists, so a change there is a change to the script too |
| A `$.state` key | Its type in `types/index.d.ts` (`PluginState.adage`), its initial value in `plugin.ts` `STATE0`, and in `register.ts` one atom and one cell, in the same order | `plugin.test.ts`; check-plugin holds the four to the same keys in the same order |
| A `$.store` key | Its name in `plugin.ts` `STORE_KEYS` and one cell in `register.ts` `store` | `kept.test.ts` or the test of the module that keeps it; check-plugin holds the two together and refuses any other `$.store` call |
| An engine call | One closure in `mod.ts` `Caps` and in `register.ts` `mod($, options)` (a `$.env.get` name must be a literal there) | The `fake` `Mod` in `kit.ts` answers it; the validator lists it in `VALIDATE.txt`'s `calls` line |
| A module | `hooks/<name>.ts` taking a `Mod` (or nothing of the engine at all), and `hooks/<name>.test.ts` beside it | check-plugin: every module with a value export has its test file; no module but `register.ts` calls `$` or imports the state library |
| A test | In the file named by the module it tests, its `describe` (or top-level `test`) title carrying the test plan's id (`T-B2-07 · …`), its fixtures from `fixtures/kit.ts` | check-plugin: every title carries an id; no test file stubs `process.run`, `process.spawn` or `env.get` itself, or builds its own world |

## What `claude plugin validate` allows

`claude plugin validate --strict` (2.1.288) reads the hooks module's source and refuses a module
whose engine use it cannot list. Its rule was stated as "every `$` call lives in `register.ts`,
and hooks are written inline at `on(...)`" (P3-notes.md); experiments on scratch plugins (a
`register.ts` and a module beside it, validated one shape at a time) show the rule precisely:

| Shape | Result |
| --- | --- |
| A top-level function in `register.ts` (a declaration or a const arrow) returning an object of closures over `$`, each calling `$.noun.event(...)`, passed into an imported module | passes; the calls are listed `(via <function>)` |
| The same object built inline at the hook, memoised in a module variable, or built by a top-level function that calls another top-level function taking `$` | passes |
| A state cell as two closures, `() => read($, ATOM)` and `f => update($, ATOM, f)`, with `ATOM` a const of `register.ts` | passes; the state reads and writes are listed by key |
| `$.store.get(key)` with a key passed in as a parameter | passes (store keys are not listed) |
| `$.ui.resolve(e)` inside a closure, `e` passed in | passes |
| A generator hook delegating with `yield*` to an imported generator that takes closures | passes |
| `on(...)` called in a loop or in a helper function of `register.ts` | passes (a loop is listed as one matcher with alternatives) |
| A hook named at `on(...)` and imported from one of the module's own files | passes, and that file is then scanned by the same rules (its own atoms, its own top-level functions) |
| `$` passed to a function imported from another file (from `register.ts`, or from an imported hook's file) | fails: "`$` is followed only into a function declared in this same file, never across an import" |
| `$.ui` (a noun) or `$.ui.toast` (an event) used as a value | fails: "is used as a value … `$` is always spelled `$.noun.event(...)` at the call site" |
| A generic accessor: `ref => $.state.get(ref)`, or `read($, ref)` / `update($, ref, f)` with `ref` a parameter | fails: "takes a reference whose plugin and key are string literals … written there or in a const of this file" |
| An atom imported from another file, or reached as a property of a table object (`STATE.fired`) | fails, for the same reason |
| `$.env.get(name)` with a name passed in | fails: "takes a literal name" |
| An imported function registered as a hook whose first parameter is not `$` | fails: its file is scanned as a hook, and the parameter is read as `$` |
| Two registrations of one event without a matcher | fails: "registered twice without a matcher" (hence `{ isInteractive: [true, false] }` and `{ hook_event_name: 'SessionStart' }` on the second ones) |

So: `$` may appear only in functions of the file that registers the hooks (or a file reached as a
hook by name), every state reference is a literal atom of that file, and every environment
variable a literal name. A typed object of closures over `$`, built in `register.ts`, may be
handed to any module. The mod uses exactly that: `register.ts` holds the atoms, `mod($, options)`
and the `on(...)` lines, and every module is a function of a `Mod` that never sees `$`. Hooks
imported by name were not used: their files would hold `$` again and need the engine to test.

## The capability object

`mod.ts` declares what every hook function takes first, `Mod = Caps & { rt, options }`:

- `Caps`: `state` (one `Cell { read, update }` per `STATE0` key), `store` (one `{ get, set }` per
  `STORE_KEYS` key), the three runners `run`, `spawn` and `pressRun`, each already behind its
  allow-list and `CLAUDECODE=1` (bridge.ts `guard`, `guardSpawn`; press.ts `guardPress`), `env`
  (the five variables the mod reads, by name), `plugin.root`, `fs.read`, `command.register` and
  `list`, `session.id`, `cwd`, `repo`, `usage`, `version`, `clock.now`, `every`, `after`,
  `checkBash` (the person's rules on a Bash command), and `ui` (`status`, `toast`, `notice`,
  `open`, `close`, `ask`, `copy`, `scroll`, `invalidate`, `resolve`).
- `rt`, the `Runtime`: what one load of the module keeps between hooks and never draws (the
  binary that answered the probe, the follower's generation and child, the timers, the failures in
  a row, the routine model, the standing answer for the session, the steps for the ledger note, the
  presses bound, the counts the band reads). A reload starts it again (`runtime()`), losing nothing
  the binary cannot say again.
- `options`: the plugin's `userConfig` (`band`, `pane`).

`register.ts` builds the `Mod` per hook call (`mod($, options)`: plain closures, cheap); the
runtime is one object for the load. A test builds a `Mod` with `fixtures/kit.ts` `fake(...)`:
state and store in memory, every call recorded, nothing guarded.

## The module map

Before: `register.ts` held every `$` call and all the logic around it (1,446 lines: activation,
the follower, the reads, every observer and site, `/adage`, the pane's handlers, the press, the
asks, the grant, skills, noticed), and `late.ts` passed the B4 handlers to the drawings through a
module-level object. After, by what each file does (lines):

| File | Lines | What it is |
| --- | --- | --- |
| `register.ts` | 161 | The state table's atoms, `mod($, options)`, and the 34 `on(...)` lines |
| `mod.ts` | 187 | `Caps`, `Runtime`, `Mod`, `runtime()`, `quietly`, the hook typing helpers |
| `plugin.ts` | 185 | Constants, the state's value types, `STATE0`, `STORE_KEYS`, `Kept` (was `plugin.ts` and `kinds.ts`) |
| `shape.ts` | 18 | `isRecord`, `str`, `num`, `strings`, `records` (each was copied into up to eight files) |
| `bridge.ts` | 255 | The allow-list, `guard`, `guardSpawn`, the binaries' paths, `probe`, `ask`, `read`, `say`, `note` |
| `binary.ts` | 19 | `askBinary`: three failures in a row warn once |
| `notify.ts` | 23 | `warnOnce`, `toastOnce` |
| `session.ts` | 113 | Activation, `session.start`, the re-probe after the shim and a `/clear` |
| `feed.ts` | 182 | The feed's lines read and folded |
| `follower.ts` | 203 | The child's life (`follow`, reconnecting), `startFeed`, `stopFeed`, `togglePause`, what a chunk changes, the avoid markers, the learner's timers |
| `refresh.ts` | 57 | The `--json` reads, at most once per 5 s; `settings` for a headless session |
| `reads.ts` | 122 | Readers of the binary's `--json` (card, reflexes, checks, lapsed, packs, candidates, map, routine model, opt-in lines) |
| `observe.ts` | 75 | The classic observers and `PreToolUse`'s markers |
| `marks.ts` | 44 | What is kept beside the rows, capped; `batchAt` |
| `reflex.ts` | 26 | `<adage-reflex>` tags read |
| `step.ts`, `predict.ts` | 184, 92 | `turnStep` and `onStep`; the prediction read and its chunks |
| `ledger.ts` | 113 | The ledger note: steps kept, the note sent at the turn's end |
| `call.ts`, `receipt.ts`, `substitute.ts` | 240, 56, 17 | The Bash call (`bashCall`, `onBashCall`); the receipt's readers; the replacement's reader |
| `attach.ts` | 166 | The standing text: asked once per session, placed in the prompt, the Bash description, compaction, a subagent |
| `commands.ts` | 136 | `/adage …` |
| `actions.ts` | 112 | The pane's actions, opening it, copying, `Explore in browser`, the pane open or closed |
| `token.ts` | 123 | The press token: issued, held, tampered with, spent (`pressCommand`) |
| `press.ts` | 168 | The person-only verbs, `guardPress`, `press()`, the gestures |
| `kept.ts` | 47 | The asks and the kept answers in `$.store` |
| `dialog.ts` | 185 | The ask at a turn's end, `/adage on`, the kept answers asked again, the pane's presses |
| `ask.ts` | 157 | Which item to ask about, and the dialog's words |
| `allow.ts`, `grant.ts` | 99, 135 | The grant's two acts; the stage and confirm read and worded |
| `notice.ts`, `skill.ts` | 79, 85 | Noticed (words, count, candidates' words); verified skills (`skill.prompt`, markers) |
| `view/text.ts`, `lines.ts`, `band.ts`, `pane.ts`, `card.ts` | 56, 173, 139, 209, 83 | Every drawn word (was `view.ts`, 744 lines) |
| `draw/sites.ts` | 222 | Every `ui.render` hook body, one per site |
| `draw/els.ts`, `rows.tsx`, `band.tsx`, `card.tsx`, `pane.tsx`, `allow.tsx` | 7, 75, 30, 48, 350, 67 | The trees (`rule`, once, in `rows.tsx`) |
| `tokens.ts` | 72 | Generated by `cargo xtask tokens` |
| `fixtures/kit.ts` | 828 | Every shared fixture (below) |

Removed: `view.ts`, `kinds.ts`, `late.ts`. `late`'s fields are in the runtime and reach the
drawings as inputs: `BandInput.kept`, `noticed`, `paneOpen`; `whyText`'s `skill`; `PaneActions.press`;
`CommandDeps.askOn`.

## Tests

Laid out as [docs/testing.md](../../testing.md) lays out every suite: a module's tests beside it
in a file named by it (`<module>.test.ts`, as a crate's unit tests sit in the module), shared
helpers in one place (`fixtures/kit.ts`, as `tests/common/mod.rs` is for a crate), inputs as
fixtures shaped as the binary prints them, and the one expected output reviewed like code,
`VALIDATE.txt`, regenerated and diffed by CI. `claude plugin test plugin/claude-code` runs every
`*.test.ts` under `hooks/`, each file in its own child of the CLI.

- Two worlds in `kit.ts`: `world(on, options)`, beneath the plugin for a test that loads the
  module as the engine does (a binary answering by argv and by payload, the follower, the model,
  the session's nouns, the engine's own drawing of each site, and recorders: `ran`, `payloads`,
  `sets`, `writes`, `bash`, `checked`, `notices`, `agents`, `asked`, `toasts`, …); and
  `fake(options)`, a `Mod` in memory for a test of one module without the engine.
- The flows (`begin`, `turnEnd`, `prompt`, `mountPane`, `bandText`, `drain`, `texts`, `shown`,
  `personOnly`, `presses`, `stdin`, `rowsOf`) and every fixture object (`LINE`, `GRAPH`,
  `DOCTOR`, `CARD`, `STAGE`, the receipt, the skill) are there and nowhere else.
- `register.test.ts` holds the cross-cutting cases that need the whole module (T-X-02, T-X-03,
  T-X-04, T-X-06); each other file tests its module, through the engine where a hook is the unit.
- 339 tests in 45 files (274 before, in 18; every earlier test is kept, its describe now naming
  its test-plan id).

Where the earlier test files went (the notes of B2-B3, B4-D5 and C3-C9 name the old files):

| Before | After |
| --- | --- |
| `register.test.ts` | `session.test.ts` (activation, Windows), `observe.test.ts`, `step.test.ts` (through the engine), `ledger.test.ts` (M6) |
| `draw.test.ts` | `draw/sites.test.ts`, `draw/rows.test.ts`, `draw/band.test.ts`; the finish-line toast in `observe.test.ts` |
| `view.test.ts` | `view/{lines,band,pane,card,text}.test.ts`, `reads.test.ts` |
| `pane.test.ts` | `draw/pane.test.ts`, `actions.test.ts`, `commands.test.ts`, `draw/card.test.ts`, `follower.test.ts` (§9.4), `session.test.ts` (§4.9), `register.test.ts` (T-X-02, T-X-03, T-X-04, T-X-06) |
| `dialog.test.ts` | `dialog.test.ts`, `token.test.ts`, `allow.test.ts`, `draw/allow.test.ts`, `register.test.ts` (T-X-04 with presses) |
| `feed.test.ts` | `feed.test.ts`, `follower.test.ts` (reconnecting) |
| `notice.test.ts` | `notice.test.ts`, `skill.test.ts`, `reads.test.ts`, `view/card.test.ts` |
| `call.test.ts`, `substitute.test.ts` | `call.test.ts` (with C6 through the engine), `receipt.test.ts`, `substitute.test.ts` |

