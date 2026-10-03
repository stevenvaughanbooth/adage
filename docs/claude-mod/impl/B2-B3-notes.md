# B2 · B3 · The rows, the band, the pane and `/adage`

Chunks B2 and B3 of [implementation-plan.md](../implementation-plan.md) (workstream B, §2.6),
drawn as [ux-design.md](../ux-design.md) specifies, cases T-B2-01 … T-B2-11, T-B3-01 … T-B3-10,
T-X-02, T-X-03, T-X-04, T-X-06, T-X-07 of [test-plan.md](../test-plan.md). Built against Claude
Code 2.1.288 (CI's pin) and its declaration file; nothing here ran in a live interactive session.
The departures from the copy deck are listed, with the reason for each, in ux-design.md's status
note; the departures from the plan are below.

## Module map

The module map is [mod-structure.md](mod-structure.md#the-module-map), which also says where each
kind of change goes and what `claude plugin validate` allows. B2 and B3 built what is now
`session.ts`, `feed.ts` and `follower.ts`, `refresh.ts` and `reads.ts`, `observe.ts`, `commands.ts`,
`actions.ts`, `view/*.ts`, `draw/*` and `fixtures/kit.ts`; the tests named in the table below
moved with their module (the case ids are in each `describe`; mod-structure.md's last table
says where each file went).

## The contracts

### What the module runs (`bridge.ts`, T-X-04)

`ALLOWED_VERBS = ['hook', 'version', 'watch', 'graph', 'doctor', 'status', 'ledger', 'off',
'map', 'condition', 'delegate']`, each in exactly one shape, every child with `CLAUDECODE=1`:

| argv after the binary | From | Why |
| --- | --- | --- |
| `version` | `bin/hook` | the probe |
| `hook claude <Event>` | `bin/hook` | decisions (C4, C6) |
| `watch --follow --json --repo <cwd> --session <id> [--since <cursor>]` | `bin/adage`, spawned | the feed |
| `graph --json`, `doctor --json`, `status --json`, `delegate status --json`, `--json --for-session` | `bin/adage` | the reads the pane and the band draw from |
| `ledger note` | | allowed for §2.1.4's `turn.complete` (not run by B2/B3) |
| `off` | `bin/adage` | `/adage off` |
| `map --port 0` | `bin/adage`, spawned | `Explore in browser` (the map opens the browser itself) |
| `condition` | `bin/adage` | the Health tab's one action (doctor's `action: {kind: run, command: "adage condition"}`) |

`scripts/check-plugin.mjs` now requires this literal exactly (a change fails until the script and
`bridge.test.ts` change with it) and requires `userConfig` to be exactly `band` and `pane`.

### The state (`types/index.d.ts`, plugin `adage`)

`fired`, `predicted` (as before; `fired` gains `prompt`, the prompt's place, and `promptText`),
`marks` (per tool_use_id: `rewrite {interpreter, what}`, `substituted {command}`, `avoided
{route}`, `receipt {receipt}`, `answered {receipt, summary, tests, ago, command}`), `batches`
(`{ids, from, to}` per tool batch), `feed` (`{status, cursor, failures}`), `rows` (Live rows;
`mine` marks this session's), `ticks`, `health`, `learner`, `reflexes` (from `graph --json`,
keeping no `arms`, `live` or control count), `checks`, `lapsed`, `packs`, `mode` (`active`,
`interactive`, `surface`, `nobinary`, `ascii`, `reducedMotion`, `spinner`, `turnLine`,
`versionLine`, `warned`), `view` (the pane: tab, filters, selection, pause), `map` (`{url,
starting}`), `turn` (`{submits, fired, predicted, marks}`: where the current turn's entries
begin). `$.store` keeps `adage.tab` (the person's last tab) and `adage.harness` (the Claude Code
version the version line was last shown for).

### The follower

Started from `session.start` when the mod is active and the session interactive, again from
`classic.SessionStart` when the session id changed (a `/clear`: the old child is ended, the
session's counts start again, the old rows stop being "mine"). Lines are applied per chunk; a
child that ends is started again `--since` the last cursor; three ends in a row each under 60 s
set `feed.status = 'stopped'` (a child that lived a minute counts as the first of a new run).
`/adage pause` and `p` end the child and keep the cursor; resuming starts a new one from it. A
learn line older than the follower's start (the backfill) earns no toast.

### `/adage`

One registration at activation: `{ name: 'adage', argumentHint: '[review]', immediate: true }`.
Verbs: bare (the card as text, and the pane opens on Live), `status` (the card), `savings` (the
card's activity, rewrites and savings rows), `watch`, `reflexes`, `review`, `health`, `doctor`
(open the pane on that tab, print nothing), `why`, `off` (runs `adage off`), `on` (runs nothing,
copies `adage on`, says so: until B4), `pause`, `map`; anything else prints the card. The
`CommandOutput` row for `adage` draws the card or `why` as a tree from the text the command
answered, so a headless session or the VS Code panel reads the same words as text.

## The seam for B4

- **What to ask about** is `view.ts` `waiting(reflexes, packs)`: a reflex whose review reasons
  include `approval` or `quiet` (`kind: 'enable'`), each `review.offers` entry (`'allow'`), each
  `packs_awaiting_trust` (`'trust'`), with the terminal command. The band's `» n need review`
  counts it today; the Reflexes tab lists it under **waiting for you** with `Copy command`.
- **Which, and in what words** is `ask.ts`: `nextAsk(items, asked, dismissed, now)` (the first
  item not asked in 24 h and not dismissed) and `question(item)` (§4.17's question and answers).
- **Where it goes**: a `turn.complete` hook in `register.ts` (none is registered now) calls
  `nextAsk` over `waiting(await read($, REFLEXES), await read($, PACKS))`, then `$.ui.ask(q.question,
  q.answers)`, and runs the answer through a `press()` that does not exist yet. `press()` belongs
  in `bridge.ts` beside `guard`, with its own allow-list (the person-only verbs) so `isAllowed`
  stays as it is; `check-plugin.mjs`'s exact-list rule then needs a second literal.
- **What to replace**: `draw/pane.tsx` `reflexRow`'s `copy:<verb>:<key>` Buttons (one per
  `review.verbs`) become press Buttons with lowercase hotkeys (keep the copy beside them for
  persona (b)); the **waiting for you** rows gain `e enable` / `n not this one`; `commands.ts`
  `case 'on'` opens the ask instead of copying; `ON_TEXT` goes. `PaneActions` is the place for the
  new handlers.

## Tests (L7, `claude plugin test plugin/claude-code`, 158 pass)

| Case | Where |
| --- | --- |
| T-B2-01 | `draw.test.ts` "the engine's notice redrawn with the glyph"; `view.test.ts` "the opening line" |
| T-B2-02 | `draw.test.ts` "three reflexes, three lines…", "a prompt nothing fired at…" (both surfaces) |
| T-B2-03 | `draw.test.ts` "at 40 columns…" (`wrap: truncate-end`); `view.test.ts` T-B2-03 |
| T-B2-04 | `draw.test.ts` "a rewrite, a predicted step and a dead end headed off…"; `view.test.ts` markers |
| T-B2-05 | `draw.test.ts` "a receipt delivered beside a run…"; `view.test.ts` "the receipt card… 2,400 bytes" (the card's tree is drawn from an `answered` mark, which C3 writes: see below) |
| T-B2-06 | `draw.test.ts` "off by default; on, a suffix…", "without the binary reporting the lines on…"; `view.test.ts` |
| T-B2-07 | `draw.test.ts` the band: cells and 60 columns, quiet, survey, no repository, off, paused, no binary |
| T-B2-08 | `draw.test.ts` lapsed over review, problem over review, the version line until the first prompt |
| T-B2-09 | `draw.test.ts` "the lamp in the fire, prime and inhibit roles"; `view.test.ts` |
| T-B2-10 | `draw.test.ts` "every Text colour is a role…" (every site's `Text` colour in `ROLES`, no banned word, no `adage:` status) |
| T-B2-11 | `draw.test.ts` "ADAGE_ASCII picks every twin; reduced motion schedules no redraw"; `view.test.ts` |
| T-B3-01 … T-B3-07 | `pane.test.ts` "B3 · the pane" (T-B3-07 also the Desktop without a binary) |
| T-B3-08 | `feed.test.ts` (all of it: argv, buffering, bad lines, the stem, folding, the cap, reconnecting, pause) |
| T-B3-09 | `pane.test.ts` "B3 · /adage"; `commands.test.ts` |
| T-B3-10 | `pane.test.ts` "band: false…", "pane: auto…", and manual opening nothing |
| T-X-02 | `pane.test.ts` "T-X-02 · fail open" (a binary that answers `{`, reads that fail); `bridge.test.ts` (rejects, timeouts, exit 1, non-JSON, `{}`) |
| T-X-03 | `pane.test.ts` "T-X-03 · headless" |
| T-X-04 | `pane.test.ts` "T-X-04 · authority" (every hook, command and pane press: heads ⊆ the list, never `install`, `config`, `review`, `on`, `add`; `CLAUDECODE=1` on every run); `bridge.test.ts` shapes; `check-plugin.mjs` |
| T-X-06 | `pane.test.ts` "T-X-06": a control session (this session's rows holding `withheld`, `would_predict`, `unaided`, `not_reached`, `stop_withheld`, a withheld pre-tool record, `control: true`, `would_have`) and a treated session with no match draw byte-identical bands, prompt rows, tool rows, Live tabs (both surfaces) and `/adage why`; `feed.test.ts` the scrub |
| T-X-07 | every mount above loops `['terminal', 'desktop']` (InfoNotice and TurnDuration are terminal-only sites and are mounted there only) |
| T-X-10 | retired by A8 (one plugin, one module) |
| §4.9 `/clear` | `pane.test.ts` "a new session id: the follower follows it…" |
| B4 seam | `ask.test.ts` |
| T-U-01 | the reading and doing parts need people and a live session; offline, `pane.test.ts` drives each doing task that B3 owns through the mounted tree: turn Adage off (`/adage off`), open the inspect view of the last fired reflex (`row:` press), find the health problem (Health tab, the band's `» problem`), open the map (`Explore in browser`). Enabling and allowing are B4's |

The world beneath the plugin answers `process.run` and `process.spawn` by argv; the follower
child prints its lines and ends (a resumed one has nothing new), so the reconnect loop settles,
which is why most fixtures end with the feed `stopped`. A held child or a serving map keeps work
pending, and the kit then waits about half a second per act: the two tests that need one have a
30 s timeout. `$.ui.scroll` has no answer in the test engine, so `n: newest` is checked to settle
and change nothing else.

## Departures from the plan

1. **Tabs and verbs follow ux-design** (Live, Reflexes, Health; `/adage watch|reflexes|health|
   review|why|off|on|pause|map`). The brief's status, doctor and savings are there as `/adage
   status` (the card), `/adage doctor` (the Health tab, from `doctor --json` with its actions) and
   `/adage savings` (the card's own activity and savings rows, worded by the binary).
2. **Two verbs beyond architecture §2's list**: `condition` (ux-design §6.3 and T-B3-06 need the
   Health tab to run it; T-X-04's set did not name it) and `delegate status --json` (the lapsed
   grants of §4.5 and the band's second line; nothing else reports them). Both read-only or
   allowed from a session; `check-plugin.mjs` holds the list.
3. **Explore in browser is built here** (ux-design §6 puts it at every tab's end; the plan lists
   it under B4): the press spawns `adage map --port 0` once, which opens the browser, and the
   footer then links to `http://localhost:<port>/` (the map answers `localhost` and `127.0.0.1`).
   `/adage map` does the same. B4's T-B4-07 asks for `--no-open --port <free>`: the engine has no
   call that opens a URL, so `--no-open` would leave the first press opening nothing.
4. **The arm scrub keeps an emptied row as a quiet row** where A3's binary drops it: a treated
   session with no match spools the same event as a quiet row, so dropping the control one makes
   the two Live tabs differ by that row. The mod scrubs again either way (defence in depth).
5. **The prompt line is keyed by the prompt's text**, not by order: `UserMessage`'s `requestId`
   cannot be tied to the hook payload without `session.append`, which a test cannot raise.
6. **The Desktop opening-line toast is not built** (same reason; ux-design status note 3).
7. **Observe mode is unchanged** (P5's: the binary is asked with `observe` and nothing is
   yielded or replaced); T-B1-04's "only the classic observers and ui.render register" is B1's.
8. **No pulse on a new row**: nothing animates, so `ADAGE_REDUCED_MOTION` has nothing to stop;
   the learner's countdown redraws once a second while it waits, and that is a countdown.
9. **`noticed` (`ui.noticed`, the `ToolGroup` line, the band's `1 pattern noticed`) and
   `skills_invoke`** are D5's and C4's; T-B3-10's rows for them are not built. A `noticed` act in
   the feed (D2, in another agent's tree this wave) is shown in the Live tab as `○ noticed …`.

## What the binary does not give the mod yet

1. **The follower ends when the engine closes its stdin.** `adage watch --follow` exits on stdin
   EOF when stdin is a pipe (A3). `$.process.spawn` without `input` "closes standard input from
   the start" (the declaration file); if that is a closed pipe, every child ends before its first
   line (measured: `printf '' | adage watch --follow --json …` prints nothing and exits 0 in
   16 ms), the reconnect loop stops after three, and the Live tab says `feed stopped · run adage
   doctor`. The follower already ends on SIGTERM and on its parent's death; it needs a way to
   ignore stdin, e.g. `--stdin ignore` (then added to the allow-list's `watch` shape), or to treat
   a stdin closed before anything was read as "no stdin". This is the first thing to check live.
2. **`ui.spinner`, `ui.turn_line` (and `ui.noticed`) are not reported** in `adage --json`; the mod
   reads `ui: { spinner, turn_line }` from the card when it appears and draws nothing until then.
3. **The receipt answered as the result** (`answered` marks) has no writer until C3 (its
   `tool.call` path writes `{ id, kind: 'answered', receipt, summary, tests, ago, command }`).
4. **A feed judgement names no `tool_use_id`**, so `tool.avoided` is drawn under the last call of
   the tool batch whose hooks ran when the judgement's record was written (matched by time,
   `from − 50 ms ≤ ts ≤ to + 2 s`). A `tool_use_id` (or the batch's ids) on the decision line would
   make it exact.
5. **A scrubbed control row is dropped, not sent quiet** (departure 4).
6. **Recovery (ux-design §9.9)** needs `adage-exec`'s journal; nothing in the allow-list reports
   an interrupted action, so the status line for it is not built.

## Checked only offline; live checks owed

- Everything drawn: no tree has been seen in a terminal or the Desktop app (S9 Q3, Q3b, Q12).
- Whether the hook's `systemMessage` arrives as the `InfoNotice` site (the redraw assumes it).
- Whether `UserMessage`'s `props.text` equals `UserPromptSubmit`'s `prompt` for every prompt.
- Whether the engine dispatches `classic.PreToolUse` for the plugin's own command hook with
  `updatedInput` as the hook printed it (the rewrite marker reads it there).
- The follower's stdin (above), its restart across a module reload (`--since` the kept cursor),
  and that the child is gone after `/clear` and exit (Q3b's `ps`).
- `$.ui.copy` on a remote surface, `$.ui.scroll` on the pane, the pane surviving a reload.
- The map opening the browser from a child of the engine, and the `Link` opening it after.
- T-U-01, all three parts.

## Files touched

```text
plugin/claude-code/.claude-plugin/plugin.json      (userConfig: band, pane)
plugin/claude-code/types/index.d.ts                (the state contract)
plugin/claude-code/VALIDATE.txt                    (regenerated as CI does)
plugin/claude-code/hooks/register.ts, bridge.ts, plugin.ts, substitute.ts (a `made` dep for the marker)
plugin/claude-code/hooks/feed.ts, view.ts, commands.ts, ask.ts                     (new)
plugin/claude-code/hooks/draw/els.ts, rows.tsx, band.tsx, pane.tsx, card.tsx        (new)
plugin/claude-code/hooks/fixtures/kit.ts                                           (new)
plugin/claude-code/hooks/bridge.test.ts, register.test.ts                          (updated)
plugin/claude-code/hooks/feed.test.ts, view.test.ts, commands.test.ts, ask.test.ts, draw.test.ts, pane.test.ts (new)
scripts/check-plugin.mjs                           (the exact allow-list; the two settings rows)
docs/claude-mod/ux-design.md                       (the build's status note)
docs/usage.md                                      ("What the plugin shows in the session")
docs/how-it-works.md                               (one paragraph)
CHANGELOG.md                                       (Seeing it: one bullet)
docs/claude-mod/impl/B2-B3-notes.md                (this file)
```

No file under `crates/` was edited.

## Commands run (in /home/user/adage-poc)

| Command | Result |
| --- | --- |
| `claude plugin validate --strict plugin/claude-code` | `√ Validation passed`; output committed as `VALIDATE.txt` (paths stripped as CI does) and a fresh run diffs clean |
| `claude plugin validate --strict plugin/claude-code-exec`, `claude plugin validate --strict .` | `√ Validation passed` both |
| `claude plugin test plugin/claude-code` | `158 pass, 0 fail` across 12 files (about 11 s) |
| `claude plugin test plugin/claude-code-exec` | exits 1: "no hooks module to load" (the retired alias has no module since A8; CI no longer runs it) |
| `tsc -p <scratch tsconfig>` over `hooks/` and `types/` with 2.1.288's `claude-code.d.ts` (strict, `noUncheckedIndexedAccess`) | no errors |
| `node scripts/check-plugin.mjs` | `plugins adage, adage-exec 0.1.0: ok`; fails as intended when the allow-list literal changes |
| `node scripts/test-stage-plugin.mjs` | ok |
| `sh plugin/test/session-start.test.sh` | `session-start ok` |
| `cargo test -p xtask` | 18 passed (`the_committed_tokens_are_current` included; `tokens.ts` unchanged) |
| `adage watch --follow --json …` on a seeded sandbox, parsed with `feed.ts` under `node --experimental-strip-types` | every line kind parsed; rows, health and learner as drawn |
| `printf '' \| adage watch --follow --json …` | exits at once with no line (binary issue 1) |
| `adage map --port 0 --no-open > file` | prints `http://127.0.0.1:<port>/` on its first line; `mapUrl` reads it |
