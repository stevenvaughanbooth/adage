# Adage in Claude Code: the user experience

> Status: **proposed 2026-10-02**, against Claude Code 2.1.287. The person-facing half of
> [architecture.md](architecture.md): every place Adage draws, every string it shows, every
> state, and the rules that keep it quiet. Built on `docs/zero-config.md` §0 (the principles),
> `docs/lexicon.md` (the words) and `docs/design/style.md` (colour, glyphs, the output grammar).
> Built by [implementation-plan.md](implementation-plan.md) B2, B3, D5; tested by
> [test-plan.md](test-plan.md) §6–§8. Nothing here has been drawn in a live session yet; S9 Q3,
> Q3b and Q12 are where it first is.

## 1. Principles

Zero-config's four principles stand, and this surface adds five of its own.

1. **Earn the first directive without a command.** The mod installs with the plugin and shows
   its first line at the first session start. Nothing is typed to see Adage work.
2. **Nothing fails silently.** A problem is one plain sentence where the person is looking: the
   status line for a warning, a toast for a lapse. Never a stack trace, never a silent blank.
3. **A concept is introduced only when a decision needs it.** The band says "3 reflexes this
   session", not "fire" or "firm". Strength words and lifecycle words appear in the pane's
   Reflexes tab, where a person has gone to look.
4. **Dollars only when measured, always with an interval.** The band and `/adage` show counts
   until the savings gate (10 control, 30 treated) is met; then the verdict with its interval,
   never a bare figure.
5. **Beside the conversation, never in it.** The model's text is untouched by anything in this
   document. Every line here is person-only: a `ui.render` tree, a toast, a status line, a pane.
6. **Quiet by default.** No line when nothing fired. No band cell for a zero. No toast for a
   routine learn. One line per event at most, one notice per candidate per session, one toast per
   learn.
7. **The arm is never inferable.** A control session draws what a treated session with no match
   draws. Nothing on screen says "control", counts withheld directives, or differs in layout.
8. **One accent, lamps mean strength, the terminal's colours win.** Text uses the engine's theme
   keys; Adage's accent marks the active thing; green, amber and red appear only on fire, prime
   and inhibit and on success, warning and failure marks (`docs/design/style.md`, "Colour").
9. **Every glyph is one column wide and has an ASCII twin** (`style.md`, "Glyphs"): `◠`/`~`
   reflex, `◉`/`(o)` instinct, `↪`/`=>` rewrite, `●`/`*` lamp, `○`/`o` off, `»`/`>>` notice,
   `◈`/`<>` from a skill, `◌`/`(.)` quiet. `ADAGE_ASCII=1` picks the twin.

## 2. Who it is for

| Persona | How they meet Adage | What this surface must do for them |
| --- | --- | --- |
| **(a) The plugin user** who never opens a terminal | `/plugin install adage@adage`, then sessions | See reflexes fire, see what was saved, know when something waits for them, never need `adage ui` |
| **(b) The CLI user** who ran `adage init` | Has the terminal programs too | Not be told twice; the pane is the same information as `adage watch`, one window instead of two |
| **(c) The team lead** who shares packs | Reviews, delegates, signs | See which reflexes are a teammate's, copy the exact command to approve or revoke at a terminal |
| **(d) Headless** (`claude -p`, the SDK, CI, the demo runner) | No one is looking | See nothing, change nothing: the hooks run, no site is drawn, no question is asked |

## 3. The screen map

Where Adage may draw in a terminal session, and in the Desktop app's Code tab. The permission
prompt is not a render site and is never touched.

```text
┌──────────────────────────────────────────────────────────────────┬──────────────────────┐
│ ✻ Claude Code 2.1.287                                            │ adage  Live Refl Health│  ← Pane (dock, ≥110 cols
│   ◠ adage · 4 instincts, 9 reflexes in acme-billing · learning … │ 04:22:53 bc0c7d …     │     fullscreen; inline above
│                                                                  │ 04:22:53 19b2cf …     │     the prompt otherwise)
│ > fix W-2042                                             ← UserMessage             │
│   ◠ lookup-work-item · fire · ./scripts/wi-lookup W-2042 --json  │                      │
│                                                                  │                      │
│ ● Bash(./scripts/wi-lookup W-2042 --json)              ← ToolUse │                      │
│   ⎿ {"id":"W-2042", …}                                 ← ToolResult                    │
│ ● Bash(make gen)                                                 │                      │
│   ◠ adage ran this step on the CPU: codegen-after-registry, no model request          │
│                                                                  │                      │
│ ✓ Worked for 1m 48s · adage: 2 requests saved          ← TurnDuration (terminal)      │
│                                                                  │                      │
│ ┌──────────────────────────────────────────────────────────────┐ │                      │
│ │ ◠ adage  3 reflexes this session · 2 requests saved · ● healthy · learning in 38 s  │  ← AbovePrompt band
│ └──────────────────────────────────────────────────────────────┘ │                      │
│ >                                                                │                      │
│ ⚠ adage: the bundled binary failed its checksum        ← $.ui.status (warnings only)  │
└──────────────────────────────────────────────────────────────────┴──────────────────────┘
                                                      ┌──────────────────────┐
                                                      │ adage                │ ← toast (top right,
                                                      │ ◠ codegen-after-regi…│    4 s)
                                                      └──────────────────────┘
```

| Site | Adage's use | Frequency cap |
| --- | --- | --- |
| `InfoNotice` (terminal; a toast on Desktop) | The session's opening line | Once per session |
| `UserMessage` | One line per reflex fired at that prompt | Per prompt, at most 3 lines (the decision cap) |
| `ToolUse` | A marker under a rewritten, predicted, or dead-end-avoided call; a skill invoked or verified | Per call it applies to |
| `ToolResult` | The receipt card in place of pytest output | Per receipt |
| `ToolGroup` / last `ToolUse` of a batch | The noticed line | Once per candidate per session |
| `Spinner` | `· adage ◠ n…` suffix | While a reflex was delivered this turn |
| `TurnDuration` (terminal) | `· adage: …` summary | Per turn with Adage activity |
| `AbovePrompt` | The band, one line (two while something needs attention) | Always, unless off |
| `Pane` | `/adage` with Live, Reflexes, Health | On demand; unasked only at ≥144 columns with `pane: auto` |
| `CommandOutput` | `/adage`, `/adage why` drawn as a card | Per command |
| Toast | The learner finished with a change; a reflex formed; a lapsed grant; the finish line continuing | One per learn; one per lapse per session; one per gate |
| `$.ui.status` | Warnings only (the engine prefixes it `⚠ adage:`) | Until cleared |
| `AskUserQuestion` | Never, except `hold_dead_ends` (§9.4), off by default | |

## 4. The session, state by state

### 4.1 First session in a repository with history (bootstrap)

```text
   ◠ adage · learning from 23 past sessions in this repo · reflexes arrive within a minute · nothing runs without you
```

The band shows `◠ adage  learning…` and nothing else until the learner lands; then the next
event's line appears as in §4.2. The learner's toast at the end of the bootstrap:

```text
 adage learned from 23 sessions
 ◠ 9 reflexes and 4 instincts are now active here
```

### 4.2 A treated session with reflexes (the common case)

The opening line, the prompt line, the tool markers, the turn line and the band from §3. The
prompt line's parts, in order, each dropped from the right when `bodyColumns` is short: glyph,
reflex id, strength word, the command or file it names (truncated with `…`).

### 4.3 A control session (1 in 10)

Opening line: identical to §4.2's. No prompt line (the binary printed nothing). No tool
markers. Turn line: the engine's own. Band: `◠ adage  ● healthy · learning in 38 s`, which is
also what a treated session shows before anything fires. Toasts for learning: identical. Nothing
distinguishes the two until a reflex would have fired, and then the control session simply does
not show it, exactly as a treated session with no matching reflex.

### 4.4 A session where Adage acts (E2 on, a `predict` grant)

```text
 ● Bash(make gen)
   ◠ adage ran this step on the CPU: codegen-after-registry, no model request
```

```text
 ● Bash(.venv/bin/python -m pytest tests/test_invoice.py -q)
   ↪ adage: pointed this test run at .venv/bin/python (the project's environment)
   ⎿ ◠ adage receipt a1f2 · 48 passed in 1.9 s · the tests added in this session: test_total_rounds_half_up
      pytest's own output is not shown; adage ran the same command 4 s ago on identical inputs
```

Under the permission dialog for the rewritten call, one line (`$.ui.notice`):
`adage ↪ pointed this test run at .venv/bin/python`.

At finish: `✓ Worked for 1m 48s · adage: 2 requests saved, 1 dead end headed off`. The counts
are what the session's spool holds for this session: predicted steps, receipts answered as
results, dead ends avoided. They are not savings claims.

### 4.5 A lapsed grant

Toast, once per session, when the grant would have run:

```text
 adage
 the make gen approval lapsed (Makefile changed) · adage review delegate acme-billing/learned codegen-after-registry --level e2 --effect regenerate
```

The band's second line while any grant is lapsed: `» 1 approval lapsed · /adage health`.

### 4.6 The learner finishing mid-session

Band cell: `learning in 38 s` → `learning…` → gone (or `learned` for 10 s). Toast when a change
matters (a strength change, a new reflex, a candidate that formed):

```text
 adage learned from this session
 ◠ codegen-after-registry  prime → fire  (9 of 9 sessions)
```

```text
 adage learned from this session
 ◠ test-after-acme-edit formed · this session was its 5th
```

No toast for a learn with no change.

### 4.7 Noticed

```text
 ● Edit(acme/invoice.py)
 ● Bash(make test)
   ◠ adage noticed · make test after editing acme/** · seen in 3 earlier sessions · a reflex forms at 5
```

```text
   ◠ adage noticed · ./scripts/wi-lookup then reading the files it names · waits for your review · adage review
```

Sequences only, once per candidate per session, in both arms, never for a single call. Off with
the `noticed` setting.

### 4.8 Skills

```text
 ● Skill(release-checklist)
   ◈ adage invoked this skill: release-after-bump, no model request
```

```text
 ● Skill(acme-work-item)
   ◈ adage verified 3 of 5 steps here · step 2 corrected · /adage why
```

`/adage why` after a verified skill lists each step: `s1 ./scripts/wi-lookup {id} · works ·
2026-09-30`, `s2 pytest · stale: command not found · corrected to .venv/bin/python -m pytest`.

### 4.9 `/clear`, `/resume`, a reload

`$.state` resets on `/clear`, `/resume` and `/branch`; the mod reloads the band's figures from
the spool at `classic.SessionStart` with `source` `clear`, `resume` or `fork`. The pane stays
open if it was open (the engine keeps panes across a module reload); its rows are refetched
through the feed's `--since`. Nothing is shown about the reload itself.

### 4.10 Headless, the VS Code panel, a cloud session

Hooks run; nothing draws; `$.ui.ask` is never called. `/adage` answers as plain text (the card's
rows). The evals see the same session as without the mod.

### 4.11 A narrow terminal

Below 110 columns in fullscreen, or on the main screen, the pane seats inline above the prompt,
20 rows asked; the band keeps its first cells and drops from the right: `◠ adage  3 reflexes · 2
saved · ●`. Below 60 columns the band shows `◠ adage ●` and the lamp alone.

### 4.12 The Desktop app

The same sites draw with the Desktop's elements: native buttons in the pane, `Markdown` cards,
no `InfoNotice` (the opening line becomes a toast), no `TurnDuration` (the turn's counts move to
the band for 10 s). No `Raster`. Theme keys map to the app's theme.

## 5. The band

One line above the prompt; a second line only while something needs attention.

```text
◠ adage  3 reflexes this session · 2 requests saved · ● healthy · learning in 38 s
» 1 approval lapsed · /adage health
```

| Cell | Source | Shown when | Narrow priority (1 kept longest) |
| --- | --- | --- | --- |
| `◠ adage` | fixed | always | 1 |
| health lamp `●`/`○` | `adage doctor --json` summary (fail → `○`, warn → `●` amber) | always | 2 |
| `n reflexes this session` | `$.state.fired.length` | n > 0 | 3 |
| `n requests saved` | predicted steps + receipts as results, this session | n > 0 | 4 |
| `learning in 38 s` / `learning…` | the feed's `learner` record (`spawn.json.after`, lock) | a learn is scheduled or running | 5 |
| `1 pattern noticed` | `$.state.noticed.length` | n > 0 and the pane is closed | 6 |
| `» …` second line | lapsed grant, binary problem, review waiting | any | shown whole or not at all |

Rules: the band returns `next(e)` when every optional cell is empty and health is green, so
other mods' bands show; it never contains a button (a digit hotkey on the band fires from an
empty prompt); the words follow the lexicon ("reflexes", "requests", "learning"); numbers are
plain, no colour; the lamp is the only coloured glyph. `e.props.hasSurvey` true → `next(e)` (the
engine's survey has the band).

## 6. The pane

Opened by `/adage` (and `/adage watch|reflexes|health`) with `focus: true, closeOnEscape: true,
rows: 20`; unasked only with `pane: auto` and a 144-column fullscreen terminal. Three tabs as
`plain` Buttons with hotkeys `l`, `r`, `h`; the open tab in the accent, the others dim.

### 6.1 Live

```text
 l: Live   r: Reflexes   h: Health                                          esc close
 Filter: ▁                                                                ⏎ apply
 04:22:53 bc0c7d cc tools   ✗ ignored lookup-incident
 04:22:53 19b2cf cc stop    ○ would ask the agent to continue (withheld)
 04:22:53 d76f0f cc tools   → ◠ codegen-after-regist… FIRE     npm run code…
 04:22:53 bc0c7d cc failure → ◠ dead-end-npm-test     INHIBIT  npm run check
 04:22:53 d76f0f cc tools   ✓ finish line met codegen-after-registry
 ─ conditioning ───────────────────────────────────────────────────────────
 ◆ 04:22 auto-condition · 27/28 traces used · 0.73 s · 2 changes
   ◠ read-linked-files   prime → fire       13/17 · lb 0.53
 ─ health ────────────────────────────────────────────────────────────────
 ● hooks claude  ● 4 reflexes  ● auto-condition  ● embedding model  ● no fail-open errors
 p pause · f scope · v quiet · G newest                                 (while focused)
```

Each row is a `plain` Button: Enter opens the inspect pane (`adage-inspect`, `focus`,
`closeOnEscape`, `rows: 12`) with the record's directive text and evidence as `Markdown`. Arrows
and page keys scroll (engine-owned); `p`, `f`, `v`, `G` are hotkeys on footer buttons, live
while the pane holds the keys. The filter is an `Input` (`onInput` filters as typed, Enter keeps
it). Rows older than 500 drop. The lamp cells use the lamp roles; `FIRE`/`PRIME`/`INHIBIT` are
spelled out when the theme has no colour (the engine's theme decides; `NO_COLOR` is honoured by
Claude Code, not by the mod).

### 6.2 Reflexes

From `adage graph --json`, read-only. A tree by track: instinct `◉`, learned `◠`, imported `◠`
with the signer dim; each row: id, strength lamp and word, lifecycle word, trigger → step, support
`13/17 · lb 0.53`. Enter expands to the directive text, evidence and uplift line (`Uplift::line`).
Filters: an `Input` (fuzzy on id, trigger, command, track), `F P I` toggles, `S` cycles
lifecycle. Each row has `Copy command` (`$.ui.copy`): the exact `adage review …` verb the
`review.verbs` field names, since review decisions are terminal-only (F11). A toast confirms the
copy: `copied: adage review strengthen acme-billing/learned read-linked-files`.

### 6.3 Health

From `adage doctor --json`: every check as a lamp row with its label and detail; a check with a
`fix` shows `Enter: condition this repository now` when the fix is allowed from a session, else
`Copy command`. A `Link` to the map when `adage map` is serving (`http://localhost:<port>`).

### 6.4 Empty states

Live with no rows: `No hook has run in this repository yet. Reflexes appear here as they
fire.` Reflexes with none: `No reflexes yet · learning from your sessions · 0 of ~5 needed`.
Health with the binary missing: the warning from §9.1.

## 7. Commands

| Command | Output (`CommandOutput`) | Context for the model |
| --- | --- | --- |
| `/adage` | The status card, drawn: header, key column, `next` (as `adage` prints it) | none (`{}` after drawing) |
| `/adage watch`, `/adage reflexes`, `/adage health` | Opens the pane on that tab; prints nothing | none |
| `/adage why` | The last directive this session: its text as the model read it, route, track, strength, evidence line, followed/ignored; for a skill, the verified steps | none |
| `/adage off` | `adage is off for this repository (.adage/disabled). /adage on turns it back on.` | none |
| `/adage on` | `adage is on for this repository.` | none |
| `/adage pause` | Pauses the feed and the band for this session; `/adage pause` again resumes | none |

`immediate: true` on `watch`, `reflexes`, `health` and `pause` so they run mid-turn. `off` and
`on` write `.adage/disabled` through the binary (`adage off` / `adage on`, allowed anywhere).
Nothing else is registered: no `review`, `delegate`, `install`, `config set` of a guarded key.

## 8. Settings

The manifest's `userConfig`, drawn by Claude Code's `/config` and `/plugin configure`, stored
under `pluginConfigs`; a change reloads the mod.

| Row | Type | Default | Description shown |
| --- | --- | --- | --- |
| Band above the prompt | boolean | on | One line: reflexes this session, requests saved, health, learning |
| Watch pane | `auto` / `manual` | `manual` | Open the live pane unasked in a wide terminal |
| Notices | `daily` / `off` | `daily` | The one-line notice at session start |
| Show what Adage noticed | boolean | on | A line when a sequence of calls matches one Adage is still learning |
| Invoke skills from reflexes | boolean | on | Load a skill at the step a fire reflex names, without a model request to choose it |
| Hold known dead ends | boolean | off | Ask before a command Adage has learned fails here (never in `claude -p`) |
| Route routine steps to | string | empty | A model alias for steps a fire reflex covers, or empty to route nothing |

Execution policy, grants, trust and learning sources are not here; they stay at the terminal
(`docs/live-learning.md`, the guarded keys).

## 9. Degraded and error states

| State | What the person sees | Where |
| --- | --- | --- |
| **9.1 No binary** (the shim failed, or `$DATA/bin/adage` is `quiet`) | `⚠ adage: no binary for darwin-arm64; install with brew install … or npm i -g …` (the shim's own `systemMessage` text) | `$.ui.status`, once; the band shows `◠ adage ○` |
| **9.2 Binary refused or timed out** on one event | Nothing for that event (fail open); after 3 in a row: `⚠ adage: hooks are not answering; adage doctor` | status, once per session |
| **9.3 A tree the engine refused** (a prop it does not take) | The engine draws its own row; a dim transcript line only while hot-reloading or under `--plugin-dir`; the debug log always | nothing for installed users; CI's `plugin test` prevents it |
| **9.4 `hold_dead_ends` on, a dead end matched** | The engine's question dialog: `adage: npm test fails here (ACME_ENV is not set). Run it anyway?` with `Run it` / `Use the learned command (make test)` | dialog; never in `-p` |
| **9.5 The feed child died** | The pane's Live tab shows `feed stopped · reconnecting…`, the mod restarts it with `--since`; after 3 failures `feed stopped · adage doctor` | pane |
| **9.6 Lapsed grant** | §4.5 | toast, band line 2 |
| **9.7 Harness newer than validated** | `» Claude Code 2.1.3xx is newer than this plugin was tested with (2.1.287) · adage doctor` on the band's second line, once per session | band |
| **9.8 Narrow terminal, unasked pane** | Nothing; the pane waits (`isPlaced: false`); `/adage` opens it inline | |
| **9.9 Review waiting** | `» 2 reflexes wait for your review · adage review` | band line 2, once per session |
| **9.10 Recovery required** (an interrupted E2 action) | `⚠ adage: an action was interrupted; inspect the worktree, then adage-exec recover --id … --acknowledge` | status until acknowledged |

Rules: a warning uses `$.ui.status` (the engine's `⚠` fits); an attention item uses the band's
second line with `»`; a fact uses a toast. Every degraded line names the command that fixes it,
as the status card does.

## 10. Copy deck

Every string the mod draws, by id. `{…}` are slots; lengths are caps before truncation with `…`.

| Id | Template | Max |
| --- | --- | --- |
| `open.ready` | `◠ adage · {instincts} instincts, {reflexes} reflexes in {repo} · learning after each session · nothing runs without you` | 120 |
| `open.bootstrap` | `◠ adage · learning from {n} past sessions in this repo · reflexes arrive within a minute · nothing runs without you` | 120 |
| `open.notice` | `◠ adage · {learner's notice text}` | 120 |
| `prompt.fired` | `◠ {route} · {strength} · {command or files}` | bodyColumns |
| `tool.rewrite` | `↪ adage: pointed this {test run\|Python run} at {interpreter} (the project's environment)` | bodyColumns |
| `tool.predicted` | `◠ adage ran this step on the CPU: {route}, no model request` | bodyColumns |
| `tool.avoided` | `◠ dead end headed off: {route}` | bodyColumns |
| `tool.skill.invoked` | `◈ adage invoked this skill: {route}, no model request` | bodyColumns |
| `tool.skill.verified` | `◈ adage verified {k} of {n} steps here{ · step {i} corrected} · /adage why` | bodyColumns |
| `result.receipt` | `◠ adage receipt {id} · {n} passed in {t} s{ · the tests added in this session: {names}}` then `pytest's own output is not shown; adage ran the same command {ago} on identical inputs` | 2 lines |
| `noticed.forming` | `◠ adage noticed · {sequence} · seen in {n} earlier sessions · a reflex forms at {s_min}` | bodyColumns |
| `noticed.review` | `◠ adage noticed · {sequence} · waits for your review · adage review` | bodyColumns |
| `spinner.suffix` | ` · adage ◠ {n}…` | 16 |
| `turn.summary` | ` · adage: {n} requests saved{, {m} dead ends headed off}` | 60 |
| `band.main` | `◠ adage  {n reflexes this session · }{n requests saved · }{lamp} {healthy\|needs attention}{ · learning in {s} s\|learning…}` | bodyColumns |
| `band.lapsed` | `» {n} approval{s} lapsed · /adage health` | bodyColumns |
| `band.review` | `» {n} reflex{es} wait for your review · adage review` | bodyColumns |
| `band.version` | `» Claude Code {v} is newer than this plugin was tested with ({tested}) · adage doctor` | bodyColumns |
| `toast.learned.strength` | `adage learned from this session` / `◠ {route}  {from} → {to}  ({support} of {opportunities} sessions)` | 2 lines |
| `toast.learned.formed` | `adage learned from this session` / `◠ {route} formed · this session was its {nth}` | 2 lines |
| `toast.lapsed` | `adage` / `the {command} approval lapsed ({field} changed) · {reapprove command}` | 2 lines |
| `toast.gate` | `adage: finish line — the agent continues once` | 1 line |
| `toast.copied` | `copied: {command}` | 1 line |
| `status.binary` | the shim's text, verbatim | 1 line |
| `status.unanswering` | `adage: hooks are not answering; adage doctor` | 1 line |
| `status.recovery` | `adage: an action was interrupted; inspect the worktree, then {recover command}` | 1 line |
| `pane.live.empty` | `No hook has run in this repository yet. Reflexes appear here as they fire.` | |
| `pane.reflexes.empty` | `No reflexes yet · learning from your sessions · {n} of ~{s_min} needed` | |
| `pane.feed.stopped` | `feed stopped · reconnecting…` / `feed stopped · adage doctor` | |
| `cmd.off` | `adage is off for this repository (.adage/disabled). /adage on turns it back on.` | |
| `cmd.on` | `adage is on for this repository.` | |
| `hold.question` | `adage: {avoid} fails here ({signature}). Run it anyway?` with options `Run it`, `Use the learned command ({instead})` | |

Ordinal words (`5th`) follow English rules in a tiny helper; plurals are explicit in the
templates. No string carries `fire`/`prime`/`inhibit` outside the pane's Reflexes tab, and
`forming`/`firm` never appear outside it.

## 11. Interaction details

- **Focus.** The pane takes the keyboard only when opened by `/adage …` (the engine grants
  `focus` with an empty prompt), by ctrl+x tab, or by a click. Esc returns it and closes the
  pane. While focused: Tab walks controls, arrows and page keys scroll, Enter presses or
  submits, hotkeys press footer buttons. The band has no controls.
- **Inspect** opens a second pane as a dialog over the first (`focus`, `closeOnEscape`,
  `holdToasts`, `rows: 12`); Esc closes it and returns to the Live tab with the row still
  selected.
- **Copy** uses `$.ui.copy` on the surface the press came from; a remote surface answers
  `isCopied: false` and the toast says `copy is not available here; the command is: …`.
- **Scrolling** is the engine's; the mod never binds arrows.
- **Redraw** is driven by `$.state` writes (each read site redraws) at the engine's cap (10 a
  second, 30 for the shown pane); the pulse on a new row is 1 s of `invalidate` at 60 ms.
- **Pointer.** Hover lightens a row (`hover` on a keyed `Box`); a click presses. No drag.

## 12. Accessibility

- Nothing is said by colour alone: every lamp has a word in the pane; the band's lamp sits beside
  `healthy`/`needs attention`.
- Every glyph has an ASCII twin under `ADAGE_ASCII=1` (read through `$.env.get`).
- Desktop buttons are native, with the label as the accessible name; `role="dismiss"` on close.
- Text never exceeds the engine's bounds (10,000 characters per `Text` child); cards are cut at
  2,400 bytes as the receipts are.
- No animation other than the 1 s pulse, and none when `prefers-reduced-motion` is reported by
  the Desktop app (read from the theme when exposed; otherwise the pulse is 300 ms).

## 13. What is deliberately not here

- Review, delegation, policy, trust, install and guarded settings: terminal only (F11). The pane
  copies the command instead.
- Dollar figures before the savings gate; GPU or energy words anywhere.
- Strength and lifecycle words outside the Reflexes tab.
- Any text to the model. The `<adage-reflex>` blocks are unchanged by this document.
- A status-line entry for normal operation (the engine's `⚠` prefix makes it a warning channel).
- Digit hotkeys, bound arrows, a takeover of the screen, a second window.

## 14. Usability check (part of the test plan)

Three readers who have not seen Adage watch the acme-billing recording with the mod (S9 Q3,
Q3b, Q12 produce it) and answer, unprompted, four questions: what did Adage do in this session;
what did it save; what, if anything, waits for you; which of the two sessions shown was the
control. Pass: 3 of 3 answer the first three correctly and 0 of 3 identify the control session
better than chance. Their verbatim confusions become copy-deck changes.
