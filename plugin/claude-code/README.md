# The adage plugin for Claude Code

The plugin Claude Code installs: the command hooks (`hooks/hooks.json`, run through the
session-start shim `scripts/session-start`, which links `bin/adage`, `bin/adage-exec` and
`bin/adage-train`, and `bin/hook` to `adage` or `adage-exec`) and the mod, a hooks module of
function hooks (`hooks/register.ts`) that bridges Claude Code to the binary and draws what the
binary says. The binary decides; the mod never does
([architecture.md](../../docs/claude-mod/architecture.md)). Built and tested against Claude Code
2.1.288, CI's pin (`hooks/plugin.ts` `TESTED`).

## Installing and updating

- **From a terminal, today:** `adage init` installs this plugin for you. It stages it from the
  `adage` you run (these files are carried inside the binary) as a one-plugin local marketplace
  under `$ADAGE_HOME/plugin/`, with `adage`, `adage-exec` and `adage-train` and their
  `dist/SHA256SUMS`, then runs `claude plugin marketplace add <that directory>` and
  `claude plugin install adage@adage-local`. Claude Code reads it in place, so `adage update` (or
  `adage init` again) stages the new binaries and modules there and the next session runs them.
  The mod discovers the plugin's data directory automatically after the session-start shim
  creates its binary links; no `CLAUDE_PLUGIN_DATA` export is normally needed. It stays inactive
  if the binary is missing or does not answer its version check.
- **In Claude Code:** `/plugin marketplace add stevenvaughanbooth/adage`,
  then `/plugin install adage@adage`, with no terminal. Claude Code updates it from that
  marketplace; `adage update` run from the plugin's own binary asks it to
  (`claude plugin update adage@adage`), or names the steps: `/plugin`, choose adage, then Update.
- **To develop it:** `plugin/build.sh` stages this machine's three binaries into `dist/`, and
  `claude --plugin-dir plugin/claude-code` loads this folder for one session (disable an installed
  `adage@adage-local` first, or every hook runs twice). The mod discovers the inline plugin's
  data directory after the session-start shim creates its binary links.
- **To remove it:** `claude plugin uninstall adage@adage-local` (or `adage@adage`), which removes
  its data directory too, then `claude plugin marketplace remove adage-local`.

Whichever route, the shim links the binaries the same way: `${CLAUDE_PLUGIN_DATA}/bin/adage` is a
stable link to the verified copy `adage-<sha256>` beside it, and so are `bin/adage-exec` and
`bin/adage-train`. A Codex or OpenCode registration written by this plugin's `adage` names that
stable link, not the digest-named copy, so an update of the plugin carries it. There is one
plugin: the marketplace lists `adage` alone.

## Where it sits

- One plugin ships three binaries for each release target (plan A8, onboarding plan O1): `adage`
  (it decides; the mod runs its verbs), `adage-exec` (it acts once execution is on; not on
  Windows) and `adage-train` (`adage train` hands over to it). The marketplace lists this plugin
  alone. `plugin/build.sh` stages this machine's three into `dist/`; `scripts/stage-plugin.mjs`
  stages every target's, with this folder, onto the marketplace branch, and
  `scripts/test-stage-plugin.mjs` checks every binary and module file is staged.
- The mod reaches the binary only by a short list of exact argument vectors (`hooks/bridge.ts`
  `ALLOWED_VERBS`) and the person-only verbs only through a press (`hooks/press.ts`); every hook
  fails open (it answers what `next(e)` answered).
- `types/index.d.ts` is the `$.state` contract (`plugin.json` `types`); `userConfig` has two
  rows, `band` and `pane`.

## What's inside

`hooks/register.ts` is the only file that touches `$`: the state table's atoms, `mod($, options)`
(the capability object) and one `on(...)` line per hook, each delegating to the module that does
the work. Every other module takes a `Mod` and never sees `$`. The full map, with what each file
does, is in [mod-structure.md](../../docs/claude-mod/impl/mod-structure.md#the-module-map); in
short:

| Concern | Modules |
| --- | --- |
| The bridge to the binary | `bridge.ts`, `binary.ts`, `press.ts`, `reads.ts`, `refresh.ts` |
| The session and the feed | `session.ts`, `feed.ts`, `follower.ts`, `notify.ts` |
| Observing and deciding at the hooks | `observe.ts`, `marks.ts`, `dag.ts`, `reflex.ts`, `step.ts`, `predict.ts`, `ledger.ts`, `call.ts`, `receipt.ts`, `substitute.ts`, `attach.ts`, `skill.ts`, `notice.ts` |
| The person's decisions | `dialog.ts`, `ask.ts`, `token.ts`, `kept.ts`, `allow.ts`, `grant.ts`, `commands.ts`, `actions.ts` |
| What is drawn | `view/{text,lines,band,pane,card,graph}.ts` (the words; `graph.ts` lays out the Graph tab), `draw/sites.ts` (one function per site), `draw/{rows,band,card,pane,graph,allow}.tsx` (the trees), `tokens.ts` (generated) |
| Shared | `mod.ts` (the `Mod`), `plugin.ts` (constants and the tables), `shape.ts` (reading JSON) |

### The capability object

`mod.ts` `Mod = Caps & { rt, options }`. `Caps` is one closure per engine call the mod makes:
`state.<key>.read()` / `.update(f)`, `store.<key>.get()` / `.set(v)`, the guarded runners `run`,
`spawn`, `pressRun`, `env.<name>()`, `plugin.root()`, `fs.read`, `command.register` / `list`,
`session.id` / `cwd` / `repo` / `usage` / `version`, `clock.now` / `every` / `after`,
`checkBash`, and `ui.status` / `toast` / `notice` / `open` / `close` / `ask` / `copy` / `scroll` /
`invalidate` / `resolve`. `rt` is the runtime one load keeps between hooks (`runtime()`);
`options` the plugin's `userConfig`.

### The state keys

`$.state` (plugin `adage`), declared in `types/index.d.ts`, initialised in `plugin.ts` `STATE0`,
an atom and a cell each in `register.ts`: `fired`, `predicted`, `marks`, `batches`, `feed`,
`rows`, `ticks`, `health`, `learner`, `reflexes`, `checks`, `lapsed`, `packs`, `mode`, `view`,
`map`, `turn`, `press`, `asked`, `allow`, `skills`, `candidates`. Every value is a copy of what the
binary said or of what the person did in the pane.

`$.store` (across sessions), `plugin.ts` `STORE_KEYS`, as `adage.<key>`: `tab` (the last tab),
`harness` (the version the version line was last shown for), `asked` (the asks, one per reflex
per day), `kept` (the answers kept for the next session start; asked again, never pressed as
they stand).

## How it connects

- In: Claude Code raises an event; `register.ts`'s hook builds the `Mod` and calls
  `<module>.on<Name>(mod($, options), e, next)`.
- To the binary: `bin/hook hook claude <Event>` with the payload on stdin (`binary.ts`
  `askBinary`), `bin/adage <verb> --json` for what is drawn (`refresh.ts`), `adage watch --follow
  --json` for the feed (`follower.ts`), a person-only verb with the press token on stdin
  (`token.ts` `pressCommand`).
- Out: the hook's answer, unchanged unless the binary said otherwise; what to draw goes to
  `$.state`, and each site draws from it (`draw/sites.ts`).

## Changing it

Where each kind of change goes, one place and a test, is the table at the top of
[mod-structure.md](../../docs/claude-mod/impl/mod-structure.md#how-to-add-x).

The rules `claude plugin validate --strict` holds the module to (tested shape by shape in
mod-structure.md): `$` is followed only into functions declared in the file that registers the
hooks, never across an import, and is always spelled `$.noun.event(...)` at the call (never a
noun or an event as a value); a state reference is a literal atom of that file; an environment
variable a literal name; an event registered twice needs a matcher on the second. Hence
everything that needs `$` is in `register.ts`, and nothing else is.

Tests, from the repository root:

```bash
claude plugin test plugin/claude-code                     # every hooks/**/*.test.ts, each file in its own child
claude plugin validate --strict plugin/claude-code        # and . (the marketplace)
claude plugin validate --strict plugin/claude-code 2>&1 | sed "s#$PWD/##g" | diff plugin/claude-code/VALIDATE.txt -
node scripts/check-plugin.mjs                             # the tables, the hooks' order, the allow-lists, the layout
node scripts/test-check-plugin.mjs                        # check-plugin refuses a second plugin, an on-PATH description
node scripts/test-stage-plugin.mjs                        # the release staging: three binaries per target, checksums
plugin/test/session-start.test.sh                         # the shim (needs target/debug/{adage,adage-exec,adage-train})
```

A test sits beside its module as `<module>.test.ts`, names its test-plan id in its `describe` (or
top-level `test`) title, and takes every fixture from `hooks/fixtures/kit.ts`: `world` beneath
the plugin when the engine loads the module, `fake` for a `Mod` in memory. `scripts/check-plugin.mjs`
fails a module without its test file, a title without an id, and a test file that stubs the
engine itself. After a hook is added or moved, regenerate `VALIDATE.txt` with the `sed` line
above (CI diffs it). Typecheck with `tsc` (strict, `noUncheckedIndexedAccess`) over `hooks/` and
`types/` with the declaration file Claude Code writes for the pinned version.
