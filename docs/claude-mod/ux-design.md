# Adage in Claude Code: the user experience

> Status: **proposed 2026-10-02**, against Claude Code 2.1.287. The person-facing half of
> [architecture.md](architecture.md): every place Adage draws, every string it shows, every
> state, and the rules that keep it quiet. Built on `docs/zero-config.md` §0 (the principles),
> `docs/lexicon.md` (the words) and `docs/design/style.md` (colour, glyphs, the output grammar).
> Built by [implementation-plan.md](implementation-plan.md) B2, B3, D5; tested by
> [test-plan.md](test-plan.md) §6–§8. Nothing here has been drawn in a live session yet; S9 Q3,
> Q3b and Q12 are where it first is. Revised once after review: the first draft broke three of
> its own rules (strength words on default surfaces, the arm visible in the pane, counts stated as
> savings), and this version does not.

## 1. Principles

One paragraph every surface agrees with, from `docs/design/everyday-ux.md` §1:

> Adage reads the sessions your coding agent has already had in a repository, notices the steps
> that repository keeps needing, and reminds the agent at the right moment. Each reminder is a
> **reflex**. Adage never runs anything unless you allow it. You can always see what it learned,
> what it told your agent, and whether that helped.

Three nouns carry the model (repository, session, reflex) and **two verbs** carry every
decision: **enable** (let Adage tell the agent this) and **allow** (let Adage do this for you).
Everything else Adage has (instincts, packs, strength, lifecycle, grants, effects, sandboxes,
prediction, the control arm) is detail a person meets only if they go looking in the pane or
at `adage` itself. Zero-config's four principles stand, and this surface adds five of its own.

1. **Earn the first directive without a command.** The mod installs with the plugin and shows
   its first line at the first session start. Nothing is typed to see Adage work.
2. **Nothing fails silently.** A problem is one plain sentence where the person is looking: the
   status line for a warning, a toast for a lapse. Never a stack trace, never a silent blank.
3. **A concept is introduced only when a decision needs it.** The band says "2 reflexes this
   session", not "fire" or "firm". Strength words (`fire`, `prime`, `inhibit`) and lifecycle
   words (`forming`, `firm`, `dormant`) appear only **inside the pane**, in both its Live and
   Reflexes tabs, where a person has gone to look and where `adage watch` already uses them
   (`docs/zero-config.md` §0.3).
4. **Counts until measured, dollars only with an interval.** The band and `/adage` count what
   happened (reflexes fired, steps taken without a model request, test runs reused). The word
   "saved" belongs to the measured savings verdict and appears only once the savings gate (10
   control, 30 treated sessions) is met, with its interval, never as a bare figure
   (`docs/design/style.md`, "Numbers"; `docs/zero-config.md` §0.4).
5. **Beside the conversation, never in it.** The model's text is untouched by anything in this
   document. Every line here is person-only: a `ui.render` tree, a toast, a status line, a pane.
6. **Quiet by default.** No line when nothing fired. No band cell for a zero. No toast for a
   learn with no change. One line per event at most, one notice per candidate per session, one
   toast per learn.
7. **The arm is never stated and never counted.** Nothing on screen, in the pane or in
   `/adage why` names the control arm, lists a withheld directive, or shows a count that only a
   control session would have. A control session with nothing scheduled draws what a treated
   session with nothing fired draws.
8. **One accent, lamps carry roles, the terminal's colours win.** Text uses the engine's theme
   keys; Adage's accent marks the active thing; the fire, prime and inhibit roles colour lamps and
   marks only, and every lamp has a word or a distinct shape beside it (`style.md`, "Colour";
   `theme.rs` gives health lamps distinct shapes "so a colour-blind reader loses nothing").
9. **Every glyph is one column wide and has an ASCII twin** (`style.md`, "Glyphs";
   `design/tokens/adage.tokens.json`). `ADAGE_ASCII=1` picks the twin, read through `$.env.get`.
10. **A reflex is shown by what it tells the agent, never by its id** (`everyday-ux.md` §3
    rule 4): `run make gen after a schema edit`, not `codegen-after-schema`. Ids appear in
    `--json`, in the pane's details and where a terminal command needs one.
11. **Small by default.** Out of the box Adage draws the opening line, the line under a prompt a
    reflex fired at, the marker under a tool row it touched, the receipt card, the band, the learn
    toast, and the ask. The spinner suffix, the turn line and the noticed line are off until
    `adage config` turns them on. The pane is there for whoever opens it.

| Glyph | Twin | Means |
| --- | --- | --- |
| `◠` | `~` | a reflex, Adage's mark |
| `◉` | `(o)` | an instinct |
| `↪` | `=>` | a rewrite |
| `●` | `*` | a lamp (role gives the colour; a word gives the meaning) |
| `○` | `o` | off, nascent, or still being learned |
| `»` | `>>` | something that needs attention |
| `◈` | `<>` | from a skill |
| `◌` | `(.)` | quiet |
| `→` | `->` | leads to |
| `✓` `✗` | `OK` `X` | success, failure |
| `◆` | `#` | a conditioning run |
| `·` | `-` | separator |

Key names are words: `enter`, `esc`, `tab`; no `⏎` or `▁`.

## 2. Who it is for

| Persona | How they meet Adage | What this surface must do for them |
| --- | --- | --- |
| **(a) The plugin user** who never opens a terminal | `/plugin install adage@adage`, then sessions | See reflexes fire, see what Adage did instead of asking the model, and make every decision Adage needs (enable a reflex, let it run a step, use a team's reflexes) with one or two presses in the session; never be sent to a terminal |
| **(b) The CLI user** who ran `adage init` | Has the terminal programs too | Not be told twice; the pane is the same information as `adage watch`, one window instead of two |
| **(c) The team lead** who shares packs | Reviews, delegates, signs | See which reflexes are a teammate's, approve or revoke with a press in the pane, and still find the exact terminal command when they want it |
| **(d) Headless** (`claude -p`, the SDK, CI, the demo runner) | No one is looking | See nothing, be asked nothing: the hooks run, no site is drawn |

## 3. The screen map

Where Adage may draw in a terminal session, and in the Desktop app's Code tab (§4.12, pending
Q7). The permission prompt is not a render site and is never touched.

```text
┌──────────────────────────────────────────────────────────────────┬──────────────────────┐
│ ✻ Claude Code 2.1.287                                            │ adage  Live Refl Health│  ← Pane (dock, ≥110 cols
│   ◠ adage · 3 reflexes and 4 instincts active in this repo · …   │ 04:22:53 bc0c7d …     │     fullscreen; inline above
│                                                                  │ 04:22:53 19b2cf …     │     the prompt otherwise)
│ > fix W-2042 and add the currency field                 ← UserMessage             │
│   ◠ lookup-work-item · scripts/wi-lookup W-2042 --json           │                      │
│                                                                  │                      │
│ ● Bash(scripts/wi-lookup W-2042 --json)                ← ToolUse │                      │
│   ⎿ {"id":"W-2042", …}                                 ← ToolResult                    │
│ ● Bash(make gen)                                                 │                      │
│   ◠ adage chose this step: codegen-after-schema · no model request                     │
│                                                                  │                      │
│ ✓ Worked for 1m 48s · adage: 1 step without a model request     ← TurnDuration (terminal)│
│                                                                  │                      │
│ ┌──────────────────────────────────────────────────────────────┐ │                      │
│ │ ◠ adage  2 reflexes this session · 1 step without a model request · ● healthy       │  ← AbovePrompt band
│ └──────────────────────────────────────────────────────────────┘ │                      │
│ >                                                                │                      │
│ ⚠ adage: hooks are not answering; run adage doctor      ← $.ui.status (warnings only)  │
└──────────────────────────────────────────────────────────────────┴──────────────────────┘
                                                      ┌──────────────────────────────┐
                                                      │ adage learned from this session│ ← toast (top right,
                                                      │ ◠ codegen-after-schema is now…│    2 lines, ~40 cols, 4 s)
                                                      └──────────────────────────────┘
```

| Site | Adage's use | Frequency cap |
| --- | --- | --- |
| `InfoNotice` (terminal; a toast on Desktop) | The engine's own notice for the hook's `systemMessage`, redrawn with the glyph | Whenever the binary's notice gate printed one: the first session, then at most daily (`ui.notices`); never on `/clear` or `/resume` |
| `UserMessage` | One line per reflex fired at that prompt | Per prompt, at most 3 lines (the decision cap, `params.rs`) |
| `ToolUse` | A marker under a rewritten, predicted, or dead-end-avoided call; a skill loaded or verified | Per call it applies to |
| `ToolResult` | The receipt card, only when the receipt was the result | Per receipt answered |
| `ToolGroup` / last `ToolUse` of a batch | The noticed line (`ui.noticed`, off by default) | Once per candidate per session |
| `Spinner` | `· adage ◠ n…` suffix (`ui.spinner`, off by default) | While a reflex was delivered this turn |
| `TurnDuration` (terminal) | `· adage: …` summary (`ui.turn_line`, off by default) | Per turn with Adage activity |
| `AbovePrompt` | The band, one line (two while something needs attention) | When any cell has something to say; otherwise `next(e)` |
| `Pane` | `/adage` with Live, Reflexes, Health | On demand; unasked only at ≥144 columns with `pane: auto` |
| `CommandOutput` | `/adage`, `/adage why` drawn as a card | Per command |
| Toast | The learner finished with a change; a grant lapsed; the finish line continuing; a copy confirmation | One per learn; one per lapse per session; one per gate; one per copy |
| `$.ui.status` | Warnings only (the engine prefixes it `⚠ adage:`, so the text carries no prefix) | Until cleared |
| `AskUserQuestion` | Untouched (it is the model's question tool) | |
| `$.ui.ask` | A decision Adage needs (§4.17), at `turn.complete` only, when the model is idle | One per turn end; one per reflex per day; never inside a hook with a budget |

## 4. The session, state by state

### 4.1 First session in a repository with history (bootstrap)

```text
   ◠ adage · learning from 23 past sessions in this repo; reflexes arrive within a minute. Nothing runs without you.
```

The text is the binary's own `first_notice` (`crates/adage-hook/src/lib.rs`), which already has
its 0, 1 and n forms; the mod adds the glyph and nothing else. The band shows `◠ adage  learning…`
until the learner lands; then the next event's line appears as in §4.2. The learner's toast at
the end of the bootstrap:

```text
 adage learned from 23 sessions
 ◠ 9 reflexes and 4 instincts are now active here
```

### 4.1b First session in a repository with no history

```text
   ◠ adage · learning from this repo as you work; its first reflexes form in a few sessions. Nothing runs without you.
```

Again the binary's text. The band shows `◠ adage  ● healthy` and nothing else.

### 4.2 A treated session with reflexes (the common case)

The opening line (the binary's `ready_notice`: `◠ adage · 3 reflexes and 4 instincts active in
this repo; it keeps learning as you work. Nothing runs without you.`, with the singular forms the
binary already produces), the prompt line, the tool markers, the turn line and the band from §3.
The prompt line's parts, in order, each dropped from the right when `bodyColumns` is short: glyph,
reflex id, the command or files it names (truncated with `…`). No strength word.

### 4.3 A control session (1 in 10)

Opening line: identical to §4.2's. No prompt line (the binary printed nothing). No tool
markers. Turn line: the engine's own. Band: `◠ adage  ● healthy`, or `◠ adage  ● healthy ·
learning in 38 s` when a learn is scheduled, which is also what a treated session shows before
anything fires. Toasts for learning: identical. Noticed lines: identical (both arms). `/adage why`
prints `No reflex has spoken in this session yet.`, as it does in a treated session with nothing
fired. The pane's Live tab shows this session's rows without the records that would name the arm
(§6.1). Over a long session the two arms differ in what fires, as they must; nothing says which
is which.

### 4.4 A session where Adage acts (a `predict` grant, or E2 with receipts)

```text
 ● Bash(make gen)
   ◠ adage chose this step: codegen-after-schema · no model request
```

In a Python repository with an E2 `verify` grant:

```text
 ● Bash(.venv/bin/python -m pytest tests/test_invoice.py -q)
   ↪ adage: pointed this test run at .venv/bin/python
   ⎿ ✓ adage receipt a1f2 · 48 passed in 1.9 s · the tests added in this session: test_total_rounds_half_up
      pytest's own output is not shown; adage answered from its run 4 s ago on identical inputs (python -m pytest tests -q)
```

Under the permission dialog for the rewritten call, when one is shown, one line (`$.ui.notice`):
`↪ adage: pointed this test run at .venv/bin/python`. For a predicted call: `◠ adage chose this
step: codegen-after-schema`.

At finish: `✓ Worked for 1m 48s · adage: 1 step without a model request · 1 test run reused ·
1 dead end headed off`. The counts are what the session's spool holds for this session: predicted
steps, receipts answered as results, dead ends avoided. They are counts, not savings claims.

A receipt delivered beside a run that did happen (the receipt was not fresh, or the person's
rules would have asked) draws one dim line under the real output, not the card:
`◠ adage receipt a1f2 covers this run · /adage why`.

### 4.5 A lapsed grant

Toast, once per session, when the grant would have applied:

```text
 adage
 the make gen grant lapsed (Makefile changed) · /adage health
```

The band's second line while any grant is lapsed: `» 1 grant lapsed · /adage health`. The Health
tab lists the lapse with the exact command the binary names (`reapprove_command`, which ends in
`--predict` for a predict grant and `--level e2 --effect …` for an execute grant) behind
`Copy command`; the toast never carries the command (a 100-character command cannot be read or
copied from a 4-second toast). The word is **grant**: `adage review approve` does not grant
execution (`ui/words.rs`), and the lexicon's status is "needs review".

### 4.6 The learner finishing mid-session

Band cell: `learning in 38 s` → `learning…` → gone (or `learned` for 10 s). Toast when a change
matters (a strength change, a new reflex, a candidate that became a reflex):

```text
 adage learned from this session
 ◠ codegen-after-schema is now said as a direct instruction (9 of 9 sessions)
```

```text
 adage learned from this session
 ◠ test-after-acme-edit is now a reflex · this session was its 5th
```

```text
 adage learned from recent sessions
 ◠ lookup-then-read now waits for your review · /adage review
```

The header says `from this session` only when this session's id is among the learn line's
`sessions`; otherwise `from recent sessions` (several sessions may be open on one repository and
all see the toast). No toast for a learn with no change. A failed learn is a toast too, once:
`adage could not learn from this session (sandbox) · run adage doctor`.

### 4.7 Noticed

```text
 ● Edit(acme/invoice.py)
 ● Bash(make test)
   ○ adage noticed · make test after editing acme/** · seen in 3 earlier sessions
```

```text
   ○ adage noticed · scripts/wi-lookup then reading the files it names · waits for your review · /adage review
```

Sequences only, once per candidate per session, in both arms, never for a single call, never a
forecast of when a reflex forms. Off with the `noticed` setting. The glyph is `○`: not yet a
reflex.

### 4.8 Skills

```text
 ● Skill(release-checklist)
   ◈ adage loaded this skill: release-after-bump · no model request
```

```text
 ● Skill(acme-work-item)
   ◈ adage verified 3 of 5 steps here · step 2 corrected · /adage why
```

`/adage why` after a verified skill lists each step: `s1 scripts/wi-lookup {id} · works ·
2026-09-30`, `s2 pytest · stale: command not found · corrected to .venv/bin/python -m pytest`.

### 4.9 `/clear`, `/resume`, `/branch`, a reload

`/clear` starts a new engine session with a new id: every per-session cap and count starts
again, as it does for the binary. `/resume` and `/branch` keep or fork the id; `$.state` is reset
by the engine and the mod reloads this session's figures from the feed (`--since` the cursor the
spool gives for the session's start). The pane stays open if it was open (verify in Q3b: the
engine keeps panes across a module reload); its rows are refetched. Nothing is shown about the
reload itself.

### 4.10 Headless, the VS Code panel, a cloud session

Hooks run; nothing draws; nothing is asked. `/adage` answers as plain text (the card's rows). What
the grants allow still runs (architecture §7), so an eval that wants today's session runs without
the mod or with the mod in observe mode.

### 4.11 A narrow terminal

Below 110 columns in fullscreen, or on the main screen, the pane seats inline above the prompt,
20 rows asked; the band keeps its first cells and drops from the right by the priority in §5:
`◠ adage  2 reflexes · 1 step without a model request · ● healthy` → `◠ adage  2 reflexes ·
● healthy` → `◠ adage  ● healthy`. The lamp's word is never dropped (§1.8); at the narrowest the
band is `◠ adage ● healthy` or `◠ adage ● problem`.

### 4.12 The Desktop app (pending Q7)

The declaration file marks `$.process` "CLI only". If Q7 finds it works in the Desktop app's Code
tab, the same sites draw with the Desktop's elements: native buttons in the pane, `Markdown`
cards, no `InfoNotice` (the opening line becomes a toast), no `TurnDuration` (the turn's counts
move to the band for 10 s), no `Raster`; theme keys map to the app's theme. If `$.process` is
unavailable there, the mod has no bridge and no feed: nothing draws, the command hooks serve as
today, and the Live tab, if opened, says `live rows need the terminal · adage watch`.

### 4.13 Adage is off here

With `.adage/disabled` in the repository or `ADAGE_DISABLE` set, the hooks print nothing. The
band shows `◠ adage ○ off in this repository · /adage on` (the `○` is the off glyph), nothing
else draws, and `/adage` prints the card, whose first line says the same.

### 4.14 Automatic learning is off

When `learn.auto` is off, the opening line is the binary's own for that state (it does not say
"keeps learning"), the band has no learner cell, and `/adage` shows the card's `learning: off`
row.

### 4.15 Paused

`/adage pause` pauses the feed and the band; the band shows its one cell `◠ adage  paused ·
/adage pause`, so the pause is never invisible (§1.2). `/adage pause` again resumes.

### 4.16 Outside a repository

The binary answers nothing; the band returns `next(e)`; `/adage` prints the card, which says
`not in a repository`.

### 4.17 Adage asks (architecture §3.12)

When the learner's line lands and a reflex waits for a decision, Adage asks at the end of the
turn, in the dialog Claude Code itself uses to ask a question (`$.ui.ask`): the question above
a numbered list of answers, answered with the dialog's own digits or dismissed with the
dialog's own key. Nothing of Adage's is drawn around it; it is the engine's dialog with Adage's
words, and the words say what the reflex tells the agent, never its id.

**Enable** (let Adage tell the agent this):

```text
 Adage learned: after editing acme/**, run make test. Tell the agent?
   1. enable
   2. not this one
   3. why
```

**Allow** (let Adage do this for you):

```text
 Adage can run make gen for you after a schema edit, instead of asking the agent to. Allow it?
   1. see what it would do
   2. not now
```

`enable` records the decision through the press and a 2 s toast says `on: after editing
acme/**, run make test`. `not this one` records `dismiss`; the ask never returns. `why` opens
the pane, focused, on that reflex's evidence, with `e enable` and `n not this one`. Dismissing
the dialog is `later`: the ask returns at most once more, a day later, and is always under
**waiting for you** in the pane. At most one ask per turn end; when several wait, the band's
second line says `» 3 need review · /adage review` and the pane lists them. Whether an allow
runs the step in a sandbox inside Adage or as the agent's own call without a model request is
not the person's problem: the binary picks the path the grant and the evidence support, and
`why` says which.

`see what it would do` opens the pane, focused, with three plain lines and a button:

```text
 run make gen after a schema edit
 it runs    make gen
 it changes acme/generated/models.py            (+4 −0, shown below)
 it never   touches anything else, or the network
 this is the first time Adage would run something on this machine; it only ever runs steps you allow one by one
 ─ what it would do now ────────────────────────────────────────────────────────────
 +    currency: str = "USD"
 a: allow · n: not now · d: details                                          esc: later
```

`d: details` shows what the terminal shows a person today (the exact argv, the scripts and their
text, the sandbox profile, hashes, the effect, caches, the terminal command with `Copy`); nothing
there is needed to decide. `allow` confirms against the digest the stage produced; if the
repository moved meanwhile, the pane redraws the fresh diff and asks again. The first-run line
appears once per machine. An allow for a step with no diff (a lookup, a read) shows `it runs`,
`it reads` and `it never` and no diff. A team-pack ask reads `6 reflexes from alice@acme.com are
in this repository. Use them?` with `use them` / `not now` / `why`, and `why` shows the
commit, its author and date, and the sentence `this is the same trust you give CLAUDE.md`.

Rules: one ask per reflex per day at most; never during a turn; never inside a hook with a
budget; two verbs and plain words; the dialog's keys are the engine's, and Adage's own hotkeys
are lowercase letters in a focused pane only; a dismissed ask is a recorded `dismiss`, not a
hidden one; if no press is held (the window was missed, or the mod reloaded mid-session), the
dialog still asks and its answer is kept for the next session start, with the band saying
`» kept for your next session · /adage review`.

### 4.18 The first ten minutes with no terminal

`/plugin install adage@adage`. First session: `◠ adage · learning from 23 past sessions in this
repo…`, the band's `learning…`, then the toast `adage learned from 23 sessions`. First prompt: a
reflex fires and its line appears under the prompt. End of the first or second turn: the first
ask, usually a nascent reflex (`waits for your review · enable`). A few sessions in: the first
allow ask with its three lines and its diff, two acts, and from their next session `make gen` runs for them. Nothing in
this journey is a command, a copy, or a terminal; `Explore in browser` is there when they want
the whole picture.

## 5. The band

One line above the prompt; a second line only while something needs attention.

```text
◠ adage  2 reflexes this session · 1 step without a model request · ● healthy · learning in 38 s
» 1 grant lapsed · /adage health
```

| Cell | Source | Shown when | Narrow priority (1 kept longest) |
| --- | --- | --- | --- |
| `◠ adage` | fixed | always (when the band draws) | 1 |
| `● healthy` / `● check: <check>` / `● problem: <check>` / `○ off …` | `adage doctor --json`'s worst check, worded as the status card words it (`status_card.rs`): the lamp in the fire, prime or inhibit role, the word always present | always | 2 |
| `n reflexes this session` | distinct route ids fired this session | n > 0 | 3 |
| `n steps without a model request` | predicted steps this session | n > 0 | 4 |
| `n test runs reused` | receipts answered as results this session | n > 0 | 5 |
| `learning in 38 s` / `learning…` / `learned` | the feed's `learner` record (`spawn.json.after`, the lock) | a learn is scheduled, running or just done | 6 |
| `1 pattern noticed` | `$.state.noticed.length` | n > 0 and the pane is closed | 7 |
| `paused · /adage pause` | `/adage pause` | paused (the only cell then) | |
| `» …` second line | lapsed grant, binary problem, review waiting, harness newer than tested | one at a time, highest priority first: lapsed > problem > review > version | shown whole or not at all |

Rules: the band returns `next(e)` when every cell but the name is empty and health is green, so
other mods' bands show and a quiet session has no band; it never contains a button (a digit
hotkey on the band fires from an empty prompt); the words follow the lexicon's plain forms
("reflexes", "sessions", "learning": `lexicon.md`'s rule that default surfaces use the everyday
word); numbers are plain, no colour; the lamp is the only coloured glyph. `e.props.hasSurvey`
true → `next(e)` (the engine's survey has the band; verify the prop in Q3).

## 6. The pane

Opened by `/adage` (and `/adage watch|reflexes|health|review`) with `focus: true, closeOnEscape:
true, holdToasts: true, rows: 20`; unasked only with `pane: auto` and a 144-column fullscreen
terminal. Three tabs as `plain` Buttons with hotkeys `l` (Live), `r` (Reflexes), `t` (Health): a
`Button` hotkey is one digit or one lowercase letter, and `h` is `adage watch`'s help key, which
persona (b) knows. The open tab is in the accent, the others dim. Every tab's footer ends with one
`Link`, **`Explore in browser`**, which starts `adage map` if it is not running and opens
`http://localhost:<port>`: the whole graph, the live events and health, read-only, for when a
pane is too small. Decisions in the pane are presses (§4.17): a press runs the one verb it names
with the session's press token, and nothing in the pane runs `install` or a `config set`.

### 6.1 Live

```text
 l: Live   r: Reflexes   t: Health                                          esc: close
 Filter: lookup                                                            enter: apply
 04:22:53 bc0c7d cc tools   → ◠ codegen-after-schema   FIRE     make gen
 04:22:53 bc0c7d cc failure → ◠ dead-end-unittest      INHIBIT  python3 -m unittest
 04:22:53 bc0c7d cc tools   ✓ finish line met codegen-after-schema
 04:21:10 7a01e2 cc prompt  → ◠ lookup-work-item       FIRE     scripts/wi-lookup W-2041
 ─ conditioning ───────────────────────────────────────────────────────────
 ◆ 04:22 auto-condition · 27/28 traces used · 0.73 s · 2 changes
   ◠ read-linked-files   prime → fire       13/17 · lb 0.53
 ─ health ────────────────────────────────────────────────────────────────
 ✓ hooks claude  ✓ 4 reflexes  ✓ auto-condition  ✓ bundled model
 p: pause · v: quiet rows · n: newest                                  (while focused)
```

The feed is `adage watch --follow --json --session <id>`; **for the current session it omits
every record that would name the arm**: the `control` flag, `would_have`, `would_predict`,
`would_continue`, withheld pre-tool records and the `Unaided`, `NotReached`, `Withheld` and
`StopWithheld` acts (`ui/feed.rs`). Other sessions' rows appear as `adage watch` shows them. The
strength words (`FIRE`, `INHIBIT`) and the conditioning ticker's `prime → fire` are allowed here
by §1.3, and are spelled out so no colour is needed. The health strip quotes the real lights
(`health.rs`: `hooks <harness>`, `n reflexes`, `auto-condition`, `model` / `bundled model` /
`no model`, and a fail-open light only when the count is positive) with `theme.rs`'s shapes.

Each row is a `plain` Button: `enter` opens the inspect pane (`adage-inspect`, `focus`,
`closeOnEscape`, `rows: 12`) with the record's directive text and evidence as `Markdown`. Arrows
and page keys scroll (engine-owned); `p`, `v`, `n` are hotkeys on footer buttons, live while the
pane holds the keys. The filter is an `Input` (`onInput` filters as typed, `enter` keeps it, an
empty submit clears it). Rows older than 500 drop. The inspect pane never opens on a row the
filter above removed.

### 6.2 Reflexes

From `adage graph --json`. A **waiting for you** group first (every pending ask, §4.17), then a
tree by track: instinct `◉`, learned `◠`, imported `◠` with the signer dim; each row: id,
strength lamp and word, lifecycle word, trigger → step, support `13/17 · lb 0.53`. `enter`
expands to the directive text, evidence and uplift line (`Uplift::line`, emitted as
`evidence.uplift.line`). Filters: an `Input` (fuzzy on id, trigger, command, track), a `Select`
for strength (`all · fire · prime · inhibit`) and a `Select` for lifecycle. The graph's
`review.verbs` is a list of the verbs that apply to the row (`strengthen`, `weaken`, `extinguish`,
`approve`, `pin`, `dismiss`, as `ui/routes.rs` lists them); each is a `Button` in the expanded
row, worded as the lexicon words it (`make it a direct instruction`, `make it a suggestion`,
`retire it`, `enable it`, `pin it`, `dismiss it`), and a press records the decision through the
press token with a 2 s thanks in place. An execution or prediction grant is offered as in §4.17
(two acts, the facts between). For persona (b) the expanded row also shows the equivalent
terminal command, dim, with `Copy`; a toast confirms a copy: `copied: adage review strengthen
acme-billing/learned read-linked-files`.

### 6.3 Health

From `adage doctor --json`: every check as a lamp row with its label and detail; a check whose
`fix` the binary marks as allowed from a session (`condition now`; the JSON gains a structured
`action` beside the prose `fix`) shows `enter: condition this repository now`, every other shows
`Copy command`. Lapsed grants list here with their exact reapprove command behind `Copy command`.
A `Link` to the map when `adage map` is serving (`http://localhost:<port>`).

### 6.4 Empty states

Live with no rows: `Nothing yet in this repository. Reflexes appear here as Adage uses them.`
Reflexes with no learned reflex: `No learned reflexes yet · 0 of ~5 sessions needed · 4 instincts
active meanwhile` (the status card's wording and counts). Health with the binary missing: the
warning from §9.1. Live on a Desktop without `$.process`: §4.12's line.

## 7. Commands

| Command | Output (`CommandOutput`) | Context for the model |
| --- | --- | --- |
| `/adage` | The status card, drawn: header, key column, `next` (as `adage` prints it, with `adage ui …` swapped for `/adage watch` because `adage ui` refuses inside a session, and the terminal-only commands behind `Copy`) | none (`{}` after drawing) |
| `/adage watch`, `/adage reflexes`, `/adage health` | Opens the pane on that tab; prints nothing | none |
| `/adage why` | The last directive this session: its text as the model read it, route, track, evidence line, followed or not; for a skill, the verified steps; with nothing fired (either arm): `No reflex has spoken in this session yet.` | none |
| `/adage off` | Runs `adage off` (plan A6): `adage is off for this repository (.adage/disabled). /adage on turns it back on.` | none |
| `/adage on` | Opens the ask `adage is off in this repository · turn it on / leave it off`; the answer runs `adage on` through the press; then `adage is on for this repository.` | none |
| `/adage review` | Opens the pane on **waiting for you** | none |
| `/adage map` | `Explore in browser`: starts `adage map --no-open` on a free port if needed and opens the link | none |
| `/adage pause` | `adage is paused for this session · /adage pause resumes` / `adage resumed` | none |

One registration, `/adage`, with the argument hint `[review]` (the two verbs a person needs:
`/adage` for the card, `/adage review` for what waits); the others (`watch`, `reflexes`,
`health`, `why`, `off`, `on`, `map`, `pause`) are parsed from the arguments too and listed on
the card's last line, not in the hint. An unknown verb prints the card. `immediate: true` (they run mid-turn). No command handler runs
a person-only verb; `/adage on` opens the ask. Nothing else is registered: no `install`, no
`config set` of a guarded key.

## 8. Settings

The manifest's `userConfig`, drawn by Claude Code's `/config` and `/plugin configure`, stored
under `pluginConfigs`; a change reloads the mod.

| Key | Row | Type | Default | Description shown |
| --- | --- | --- | --- | --- |
| `band` | Band above the prompt | boolean | on | One line: reflexes this session, steps without a model request, health, learning |
| `pane` | Watch pane | `auto` / `manual` | `manual` | Open the live pane unasked in a wide terminal |

Two rows. Everything else is a `config.toml` key for people who go digging with `adage config`:
`ui.notices`, `ui.noticed`, `ui.spinner`, `ui.turn_line`, `skills.invoke`, `runtime.routine_model`,
`runtime.predict_cap`. Not a setting anywhere: execution policy, grants and trust (decisions
made by a press, §4.17, or at a terminal), a switch for asking (a decision is never hidden
behind a setting: dismiss it and it waits in the pane), and a hold on known dead ends (an open
question, architecture §10).

## 9. Degraded and error states

| State | What the person sees | Where |
| --- | --- | --- |
| **9.1 No binary** (the shim failed) | The shim's reason, read from `$DATA/status` so the engine's `⚠ adage:` prefix is not doubled: `⚠ adage: no binary for darwin-arm64; install with brew install … or npm i -g …` | `$.ui.status`, once; the band shows `◠ adage ○ off · run adage doctor` |
| **9.2 Binary refused or timed out** on one event | Nothing for that event (fail open). The bridge's answer is tagged, so a failure is distinct from "nothing to say"; after 3 failures in a row: `⚠ adage: hooks are not answering; run adage doctor` | status, once per session |
| **9.3 A tree the engine refused** (a prop it does not take) | The engine draws its own row; a dim transcript line only while hot-reloading or under `--plugin-dir`; the debug log always | nothing for installed users; CI's `plugin test` prevents it |
| **9.4 The feed child died** | The Live tab shows `feed stopped · reconnecting…`; the mod restarts it from the last cursor; after 3 failures `feed stopped · run adage doctor` | pane |
| **9.5 Lapsed grant** | §4.5 | toast, band line 2 |
| **9.6 Harness newer than validated** | `» Claude Code 2.1.3xx is newer than this plugin was tested with (2.1.287) · run adage doctor` on the band's second line, until the first prompt is submitted, once per new harness version per machine | band |
| **9.7 Narrow terminal, unasked pane** | Nothing; the pane waits; `/adage` opens it inline | |
| **9.8 Review waiting** | `» 2 need review · /adage review`, while any ask is pending and not shown | band line 2 |
| **9.9 Recovery required** (an interrupted E2 action) | `⚠ adage: an action was interrupted; inspect the worktree, then run adage-exec recover --id … --acknowledge in a terminal` (recovery is the one act that stays at a terminal: it needs a person looking at the worktree) | status until acknowledged |
| **9.12 No press token** (the window was missed, or the mod reloaded mid-session) | The dialog still asks; the answer is kept and applied at the next session start, and the band says `» kept for your next session · /adage review`; the pane shows the terminal command for anyone who wants it now | dialog, band |
| **9.10 Desktop without `$.process`** | §4.12 | pane empty state |
| **9.11 Off, learning off, paused, outside a repository** | §4.13–4.16 | band, card |

Rules: a warning uses `$.ui.status` (the engine's `⚠ adage:` fits, so no template starts with
`adage:`); an attention item uses the band's second line with `»`; a fact uses a toast. Every
degraded line names what fixes it: a `/adage` verb where one exists, and `in a terminal` only for
recovery, the one act that stays there.

## 10. Copy deck

Every string the mod draws, by id. `{…}` are slots; the binary's own notice texts are used
verbatim with their `adage: ` prefix stripped by one helper. Caps are in display columns; a
string over its cap drops cells from the right, or its last slot, before `…`. Toasts are two
lines of about 40 columns.

| Id | Template | Cap |
| --- | --- | --- |
| `open.notice` | `◠ adage · {the binary's notice text, prefix stripped}` (`ready_notice`, `first_notice`, or the day's notice) | bodyColumns |
| `prompt.fired` | `◠ {summary}` (`run make test after editing acme/**`); for an avoid reflex `◠ avoid {command} here · {instead}` | bodyColumns |
| `tool.rewrite` | `↪ adage: pointed this {test run\|Python run} at {interpreter}` (also the `$.ui.notice` line) | bodyColumns |
| `tool.predicted` | `◠ adage chose this step · no model request` (also the `$.ui.notice` line, without the tail) | bodyColumns |
| `tool.avoided` | `◠ adage: dead end headed off` | bodyColumns |
| `tool.substituted` | `↪ adage ran {instead} instead · {reason}` | bodyColumns |
| `tool.skill.invoked` | `◈ adage loaded the {skill} skill · no model request` | bodyColumns |
| `tool.skill.verified` | `◈ adage verified {k} of {n} steps here{ · step {i} corrected} · /adage why` | bodyColumns |
| `result.receipt` | `✓ adage receipt {id} · {receipt summary}{ · the tests added in this session: {names}}` / `pytest's own output is not shown; adage answered from its run {ago} on identical inputs ({command that ran})` | 2 lines |
| `result.receipt.beside` | `◠ adage receipt {id} covers this run · /adage why` | bodyColumns |
| `noticed.forming` | `○ adage noticed · {sequence} · seen in {n} earlier sessions` | bodyColumns |
| `noticed.review` | `○ adage noticed · {sequence} · waits for your review · /adage review` | bodyColumns |
| `spinner.suffix` | ` · adage ◠ {n}…` | 16 |
| `turn.summary` | ` · adage: {n} step{s} without a model request{ · {m} test run{s} reused}{ · {k} dead end{s} headed off}` | 72 |
| `band.main` | `◠ adage  {n reflexes this session · }{n steps without a model request · }{n test runs reused · }{lamp} {healthy\|check: {check}\|problem: {check}}{ · learning in {s} s\|learning…\|learned}{ · 1 pattern noticed}` | bodyColumns |
| `band.off` | `◠ adage ○ off in this repository · /adage on` | bodyColumns |
| `band.nobinary` | `◠ adage ○ off · run adage doctor` | bodyColumns |
| `band.paused` | `◠ adage  paused · /adage pause` | bodyColumns |
| `band.learning` | `◠ adage  learning…` (bootstrap) | bodyColumns |
| `band.lapsed` | `» {n} grant{s} lapsed · /adage health` | bodyColumns |
| `band.review` | `» {n} need review · /adage review` | bodyColumns |
| `band.kept` | `» kept for your next session · /adage review` | bodyColumns |
| `ask.enable` | question `Adage learned: {summary}. Tell the agent?` · answers `enable` / `not this one` / `why` | the dialog's |
| `ask.enable.done` | toast `on: {summary}` | 1 × 40 |
| `ask.allow` | question `Adage can {summary}, instead of asking the agent to. Allow it?` · answers `see what it would do` / `not now` | the dialog's |
| `ask.allow.pane` | `{summary}` · `it runs {argv}` · `it changes {outputs} ({+n −m}, shown below)` or `it reads {files}` · `it never touches anything else, or the network` · the diff as `Code` · `a: allow · n: not now · d: details` / `esc: later` | pane |
| `ask.allow.first` | `this is the first time Adage would run something on this machine; it only ever runs steps you allow one by one` | pane |
| `ask.allow.details` | the exact argv, scripts and their text, sandbox profile, hashes, effect, caches, `takes effect in your next session`, the terminal command with `Copy` | pane |
| `ask.allow.done` | toast `allowed: {summary} · from your next session` | 2 × 40 |
| `ask.trust` | question `{n} reflexes from {signer} are in this repository. Use them?` · answers `use them` / `not now` / `why` | the dialog's |
| `ask.trust.pane` | `added in {commit} by {author} on {date}` · `key {fingerprint}` · `packs {list}` · `this is the same trust you give CLAUDE.md` · `u: use them · n: not now` | pane |
| `pane.waiting` | `waiting for you` | |
| `pane.verb.{approve\|dismiss}` | `enable` / `not this one` (the row's two buttons) | |
| `pane.verb.more` | `more: make it a direct instruction · make it a suggestion · retire it · pin it` (behind `d: details`) | |
| `pane.explore` | `Explore in browser` | |
| `band.version` | `» Claude Code {v} is newer than this plugin was tested with ({tested}) · run adage doctor` | bodyColumns |
| `toast.learned.strength` | `adage learned from {this session\|recent sessions}` / `◠ {summary} · now a direct instruction` | 2 × 40 |
| `toast.learned.softened` | `adage learned from {this session\|recent sessions}` / `◠ {summary} · now a suggestion` | 2 × 40 |
| `toast.learned.formed` | `adage learned from {this session\|recent sessions}` / `◠ {summary} · new, from this session` | 2 × 40 |
| `toast.learned.review` | `adage learned from {this session\|recent sessions}` / `◠ {summary} · needs you · /adage review` | 2 × 40 |
| `toast.learned.bootstrap` | `adage learned from {n} sessions` / `◠ {reflexes} reflexes and {instincts} instincts are now active here` | 2 × 40 |
| `toast.learn.failed` | `adage could not learn from this session ({class}) · run adage doctor` | 2 × 40 |
| `toast.lapsed` | `adage` / `{command} is no longer allowed: {field} changed · /adage review` | 2 × 40 |
| `toast.gate` | `adage: finish line · the agent continues once` | 1 × 40 |
| `toast.copied` | `copied: {command}` | 1 × 40 |
| `toast.copy.unavailable` | `copy is not available here · the command is in /adage health` | 1 × 40 |
| `status.binary` | `{the shim's reason from $DATA/status}` | 1 line |
| `status.unanswering` | `hooks are not answering; run adage doctor` | 1 line |
| `status.recovery` | `an action was interrupted; inspect the worktree, then run {recover command} in a terminal` | 1 line |
| `pane.tabs` | `l: Live   r: Reflexes   t: Health` / `esc: close` | |
| `pane.footer` | `p: pause · v: quiet rows · n: newest` | |
| `pane.filter` | `Filter:` / `enter: apply` | |
| `pane.live.empty` | `Nothing yet in this repository. Reflexes appear here as Adage uses them.` | |
| `pane.live.desktop` | `live rows need the terminal · adage watch` | |
| `pane.reflexes.empty` | `No learned reflexes yet · {n} of ~{s_min} sessions needed · {k} instincts active meanwhile` | |
| `pane.feed.stopped` | `feed stopped · reconnecting…` / `feed stopped · run adage doctor` | |
| `pane.copy` | `Copy: adage review {verb} {track} {id}` / `Copy command` | |
| `pane.health.action` | `enter: condition this repository now` | |
| `cmd.off` | `adage is off for this repository (.adage/disabled). /adage on turns it back on.` | |
| `cmd.on` | `adage is on for this repository.` | |
| `cmd.pause` | `adage is paused for this session · /adage pause resumes` / `adage resumed` | |
| `cmd.why.none` | `No reflex has spoken in this session yet.` | |
| `cmd.why.step` | `s{i} {command} · {works\|stale: {signature}}{ · corrected to {command}} · {date}` | |
| `band.main` (the one-line form) | as above; `{summary}` wherever a reflex is named | |

`{summary}` is the reflex's own sentence (`adage_app::reflex::Reflex`'s summary, the one every
surface renders): never an id. Plurals are explicit in the templates and zero parts are omitted
as the binary's `ready_notice` omits them. Outside the pane's details no string carries `fire`,
`prime`, `inhibit`, `forming`, `firm`, `dormant`, `instinct`, `grant`, `effect`, `sandbox`,
`cache`, `stage`, `predict` or a hash; "approval" and "saved" are not used anywhere.

## 11. Interaction details

- **Focus.** The pane takes the keyboard only when opened by `/adage …` (the engine grants
  `focus` with an empty prompt), by a `why` or `see what it would do` answer, by ctrl+x tab, or
  by a click. `esc` returns it and closes the pane. While focused: `tab` walks controls, arrows
  and page keys scroll, `enter` presses or submits, lowercase hotkeys press buttons. The band has
  no controls. An ask is the engine's dialog and has the engine's keys.
- **Inspect** opens a second pane as a dialog over the first (`focus`, `closeOnEscape`,
  `rows: 12`); `esc` closes it and returns to the Live tab with the row still selected.
- **Copy** uses `$.ui.copy` on the surface the press came from; when the result says the copy
  did not happen (a remote surface), the toast is `toast.copy.unavailable` and the command stays
  visible in the Health tab.
- **Scrolling** is the engine's; the mod never binds arrows.
- **Redraw** is driven by `$.state` writes (each read site redraws) at the engine's cap (10 a
  second, 30 for the shown pane or band); the pulse on a new row is 1 s of `invalidate` at 60 ms.
- **Pointer.** Hover lightens a row; a click presses. No drag.
- The props and calls this section leans on beyond the documented facts (`hasSurvey`,
  `isExpanded`, the copy result's shape, hover, panes surviving a reload) are checked in S9 Q3
  and Q3b before B2 and B3 start; where one is absent, the behaviour degrades to `next(e)`.

## 12. Accessibility

- Nothing is said by colour alone: every lamp has a word or a distinct shape beside it; the band's
  lamp sits beside `healthy`, `check` or `problem` at every width.
- Every glyph has an ASCII twin under `ADAGE_ASCII=1` (§1.9).
- Desktop buttons are native, with the label as the accessible name.
- Text never exceeds the engine's bounds (10,000 characters per `Text` child); cards are cut at
  2,400 bytes as the receipts are.
- No animation other than the 1 s pulse, and none at all under `ADAGE_REDUCED_MOTION=1`
  (`style.md`, `theme.rs`), read through `$.env.get` like `ADAGE_ASCII`.

## 13. What is deliberately not here

- Install and guarded settings from the pane; recovery after an interrupted action (it needs a
  person at the worktree). Every other decision is a press.
- Any decision made from a web page: the map is read-only.
- Dollar figures before the savings gate; the word "saved" before the verdict; GPU, CPU or energy
  words anywhere.
- Strength and lifecycle words outside the pane.
- The arm, anywhere.
- Any text to the model. The `<adage-reflex>` blocks are unchanged by this document.
- A status-line entry for normal operation (the engine's `⚠` prefix makes it a warning channel).
- Digit hotkeys of Adage's own (the digits in an ask are the engine's dialog's), uppercase
  hotkeys, bound arrows, a takeover of the screen, a second window, a dialog during a turn.
- Reflex ids, lexicon words and the machinery (grants, effects, sandboxes, caches, hashes,
  prediction) on any default surface. They are behind `details` in the pane and at `adage`.
- More than two verbs for a person: enable and allow. The six review verbs are in the pane's
  details and at `adage review`.

## 14. Usability check (part of the test plan, T-U-01)

Two parts, run once per surface change on the acme-billing sandbox with the mod loaded.

**Reading.** Eight readers who have not seen Adage, four from persona (a) and four from (b), each
watch one recorded session (S9 Q3, Q3b, Q12 produce them) and answer, unprompted: what did Adage
do in this session; what did Adage do that the agent did not have to ask the model for; what, if
anything, waits for you, and where do you do it. Pass: 7 of 8 answer all three correctly; every
confusion is recorded verbatim and becomes a copy-deck change.

**The arm.** Twenty matched pairs of recordings (one control, one treated session with no
matching reflex, same task) are shown to the readers in random order with the question "in which
of these two did Adage hold back on purpose?". Pass when the pooled accuracy's 95% interval
includes 0.5 (binomial; at 160 answers, between 68 and 92 correct).

**Doing.** Each reader performs six tasks live in a session: enable the reflex that waits for
you; allow a reflex to run its step, reading what it would change; turn Adage off in this
repository and back on; open the inspect view of the last fired reflex; find the health problem;
open the map in the browser. Pass: every task completed by 7 of 8 without help and without a
terminal, under 60 s each; the two slowest are redesigned.
