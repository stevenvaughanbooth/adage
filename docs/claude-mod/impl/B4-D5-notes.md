# B4 · D5 · Asking in the dialog, the press, noticed, learning and skills

Chunks B4 and D5 of [implementation-plan.md](../implementation-plan.md) (§2.6; §3 B4, D5),
designed in [architecture.md](../architecture.md) §3.10–§3.12, worded by
[ux-design.md](../ux-design.md) §4.6–§4.8, §4.17, §6, §7, §9.12, §10; cases T-B4-01 … T-B4-09,
T-D5-01 … T-D5-03, T-D4-04, T-X-13 (L7), T-A7 (mod side), T-U-01's doing part (offline) of
[test-plan.md](../test-plan.md). Built against Claude Code 2.1.288 and its declaration file;
nothing here ran in a live interactive session (the live-only checks are listed at the end, with
their probes).

## For the agent working in register.ts, bridge.ts, plugin.ts and types/ at the same time

- **Allow-list (`bridge.ts` `ALLOWED_VERBS`): nothing new is needed.** B4/D5 read with the shapes
  already there (`hook claude Press`, `hook claude SkillPrompt`, `graph --json`, `status --json`,
  `delegate status --json`, `--json --for-session`, `map --port 0`).
- **The person-only verbs have their own list**, `PRESS_VERBS` in `hooks/press.ts`:
  `['review', 'delegate', 'on', 'add']`, each in exact shapes (below). Run only by `press()` with
  `{ press, session }` on stdin. `scripts/check-plugin.mjs` is not edited by me; a rule holding
  this second literal (`export const PRESS_VERBS: readonly string[] = [...]`) is the
  regenerating agent's call.
- **register.ts:** the registrations are one marked block at the end of `register`'s body
  (`// === B4 · D5`), the helpers one marked block after it (`// === B4 · D5 helpers`), with their
  imports there too (import declarations are hoisted). Nothing above the first block was changed.
  Registrations nest first outermost (measured in a scratch plugin): each of mine runs inside
  yours, so each calls `next(e)` where yours does. Two of mine are reached by yours through
  `late.ts` instead, since your `Pane`/`AbovePrompt`/`command.run` hooks answer without `next`.
- **types/index.d.ts:** the B4 · D5 section (`AdagePress`, `AdageAsked`, `AdageStage`,
  `AdageAllow`, `AdageSkill`, `AdageCandidate`, the five `PluginState.adage` keys) plus two
  one-line additions in your types: `AdageRow.ids?: string[]` (a feed row's `tool_use_ids`) and
  `AdageStage.inputs: string[]`. `feed.ts` `rowOf` keeps `tool_use_ids` as `ids` (one small edit in
  that function). `kinds.ts` and `late.ts` are B4's by content (the previous B4 agent's); I kept
  them and extended `late.ts` with `paneOpen`.
- **VALIDATE.txt** changes with this block (the diff is in "Commands run"); per the brief I did
  not regenerate it.

## Module map (new or changed by B4/D5)

| File | What it is |
| --- | --- |
| `hooks/register.ts` (end) | The B4 · D5 hooks: `session.start{isInteractive=true\|false}` (binds the pane's presses), `classic.SessionStart{hook_event_name=SessionStart}` (the press), `turn.complete{reason=answer}` (the ask), `skill.prompt`, `ui.render{ToolGroup}` (noticed), `ui.render{ToolUse, props.tool=Skill}` (skill markers), `ui.render{Pane, requestId=adage-allow}` (the grant pane), `ui.open{id=adage}` / `ui.close{id=adage}` (whether the pane is open). Helpers: `issuePress`, `checkTamper`, `pressCommand` (the one path to `press()`), `applyKept`/`keep`, `turnEnded`/`act`/`askOn`, `stageAllow`/`confirmAllow`/`closeAllow`, `verifiedSkill`, `recountNoticed`/`wordCandidates` |
| `hooks/press.ts` | `PRESS_VERBS`, `REVIEW_VERBS`, `isPressAllowed` (exact shapes), `pressArgv` (a terminal command → argv), `guardPress`, the gestures `ANSWER` / `BUTTON` / `KEPT` (the only values `press()` accepts), `press()` (stdin `{ …extra, press, session }`, reads line 1 `{ next }`), `objectOf` (line 2), `isToken`, `tampered` |
| `hooks/ask.ts` | `nextAsk`, `question` (the dialog's words), `ANSWERS`, `ON_QUESTION`/`ON_ANSWERS`, `answerOf`, `record` (dismissed / why / answered), `askedOf`, `keptOf`, `whatOf`, `doneToast` |
| `hooks/grant.ts` | `stageOf` (`--stage --json` → `AdageStage`), `confirmOf`, `confirmCommand`, `routeOf`, `allowLines` (the three plain lines), `diffText`, `allowDetails`, `FIRST_LINE`, `TRUST_LINE` |
| `hooks/notice.ts` | `candidatesOf` (`status --json` → words), `noticedLine`, `noticedLines` (per tool group), `noticedIds`, `skillOf` (a `SkillPrompt` answer → steps), `withSkill`, `verifiedMark`, `invokedMark` |
| `hooks/late.ts` | `late`: the press handlers the pane and `/adage on` use, `kept`, `noticed`, `paneOpen`, the last verified `skill` |
| `hooks/kinds.ts` | The B4/D5 state types and `PRESS0`, `ALLOW_PANE`, `Kept`, `KEPT_MAX_AGE_MS`, `MAX_ASKED`, `MAX_SKILLS` |
| `hooks/draw/allow.tsx` | `allowTree`: the grant pane |
| `hooks/draw/pane.tsx` | The **waiting for you** rows with `yes`/`no` presses, the expanded row's `e enable` / `n not this one` / `d details` (the four more verbs, the dim command with `c Copy`), `late.press` fallback to Copy |
| `hooks/draw/rows.tsx` | `skillRow`, and `under` dropping `tool.predicted` beneath a skill row |
| `hooks/view.ts` | `Waiting` gains `no`, `offer`, `reflex`; `VERB_WORDS` (`pane.verb.*`), `ROW_VERBS`, `VERB_HOTKEYS`; the band's `kept` line and `n patterns noticed` cell; `whySkill`, `whyText` with the skill |
| `hooks/commands.ts` | `/adage on` opens the ask through `late.press.askOn()`; `ON_TEXT` is the headless/inert answer |
| `hooks/fixtures/kit.ts` | `$.ui.ask` answered as the `AskUserQuestion` tool (`ask` option, `w.asked`), the press chain (`TOKEN` → `NEXT_TOKEN` → `THIRD_TOKEN`), person-only verbs answered as the binary does (`refuse`, `stage`, `confirm`), `ui.close`, the store (`w.store`), `LINE.pressIssued`/`pressIssuedAgain`/`noticed`, `GRAPH_OFFER`/`GRAPH_PREDICT`, `CANDIDATE*`, `STAGE`/`STAGE_MOVED`/`CONFIRMED`, `SKILL_ANNOTATED`, `GROUP`, `SKILL_TOOL`, `TURN_END` |
| `hooks/{dialog,grant,notice,ask}.test.ts`, `pane.test.ts`, `commands.test.ts` | The tests (table below) |

## The dialog strings, as built

| Where | String |
| --- | --- |
| `ask.enable` | `Adage learned: {summary}. Tell the agent?` · `enable` / `not this one` / `why` |
| `ask.allow` (an execution offer, `offers[].kind = execute`) | `Adage can {summary}, instead of asking the agent to. Allow it?` · `see what it would do` / `not now` |
| a prediction offer (`offers[].kind = predict`) | the same question · `allow` / `not now` / `why` (mod architecture §3.12's row; one act) |
| `ask.trust` | `Reflexes from {signer} are in this repository. Use them?` · `use them` / `not now` / `why` (the count is not in `status --json`'s `packs_awaiting_trust`, so the question names the pack) |
| `/adage on` | `Adage is off in this repository. Turn it on?` · `turn it on` / `leave it off` |
| `ask.enable.done` | toast `on: {summary}` |
| `ask.allow.done` | toast `allowed: {summary} · from your next session` (confirm recorded, or `--predict`) |
| `cmd.on` | toast `adage is on for this repository.` |
| a pack trusted | toast `using them: {signer}` |
| a pane verb landed (dismiss, strengthen, weaken, extinguish, pin) | toast `done: {lexicon word} · {summary}` |
| a press the binary refused or a verb that failed | toast `not recorded: {the binary's last stderr line, error: stripped}` |
| `ask.allow.pane` | `{summary}` · `it runs    {argv}` · `it changes {paths}  (+n −m, shown below)` (or `it changes {outputs}  (nothing differs now)`, or `it reads   {input contracts}`) · `it never   touches anything else, or the network` · the diff as `Code format="diff"` under `─ what it would do now` · `a allow` · `n not now` · `d details` · `esc: later` |
| `ask.allow.first` | `this is the first time Adage would run something on this machine; it only ever runs steps you allow one by one` (when the stage's `first` is true) |
| a refused stage | `it would also touch {paths}, which the grant would not cover · not allowed as it is`, and the binary's `why` as the note; no `allow` button |
| `ask.allow.details` | `argv …`, `script {path}` with its text, `sandbox …`, `effect …`, `outputs …`, `caches …`, `hash profile …` / `hash route …` / `hash {script} {sha256}`, `takes effect in your next session`, the terminal command with `c Copy` |
| the repository moved between the acts | note `the repository moved meanwhile; this is what it would do now`, the fresh diff, asked again |
| a term changed | note `not recorded: {label} changed since it was staged ({fields}); nothing was recorded` (the binary's words) |
| no press held at a stage/confirm | note `no press is held in this session; the command below runs it at a terminal`, details open |
| `band.kept` | `» kept for your next session · /adage review` |
| the tamper signal | status `something else claimed this session's press; presses are off for this session · run adage doctor` |
| `pane.waiting` | `waiting for you`; each row `{text}` then `enable` / `not this one`, `see what it would do` / `not now`, `allow` / `not now`, `use them` / `not now`; expanded: the command with `Copy`, and for a pack `packs {name}` and `this is the same trust you give CLAUDE.md` |
| `pane.verb.*` | `enable` (`e`), `not this one` (`n`), `details` (`d`); behind details `more:` `make it a direct instruction` (`s`), `make it a suggestion` (`w`), `retire it` (`x`), `pin it` (`i`), the command dim with `Copy` (`c`) |
| `noticed.forming` | `○ adage noticed · {gist} · seen in {n} earlier session(s)` |
| `noticed.review` | `○ adage noticed · {gist} · waits for your review · /adage review` |
| `band.main` | `… · {n} pattern(s) noticed` (rank 7, while the pane is closed) |
| `tool.skill.invoked` | `◈ adage loaded the {skill} skill · no model request` |
| `tool.skill.verified` | `◈ adage verified {k} of {n} steps here{ · step {i} stale}… · /adage why` |
| `/adage why` after a verified skill | `◈ adage verified {k} of {n} steps of the {skill} skill here`, blank, `s{i} {command} · works \| stale: {signature} · {date}` |

## The press, from the mod's side

1. **Issue.** `classic.SessionStart` (mine, inner; after `await next(e)`, so the command hooks'
   session start opened the window), only when a person is at the prompt and `source` is not
   `compact`: `<bin/hook> hook claude Press` with stdin
   `{ session_id, cwd, hook_event_name: "Press", source, via: "mod" }` (the payload's own
   `session_id` and `cwd`, else `$.session`'s), 2 s. The binary path is the one activation found,
   or, for a first session whose link the shim made during this very hook, `hookBinary(...)`
   derived the same way. `{ press }` (64 hex) → `$.state.adage.press = { session, token, state:
   'held', issued, chain }`; nothing → `state: 'none'`, token empty. `issued` counts the tokens
   handed to the mod for this session id (a `resume` reissue is counted as 2, the most the binary
   issues for one id). The token lives in `$.state` only: never a file, never argv, never a toast.
2. **Spend.** `pressCommand($, gesture, command, …)` is the only caller of `press()`; it is reached
   from a `$.ui.ask` continuation (`ANSWER`), a Button's `onPress` in the pane or the grant pane
   (`BUTTON`), or a kept answer at a start (`KEPT`), and from nothing else (no hook body, timer,
   feed line or `command.run` calls it: `/adage on` only opens the ask). `press()` runs
   `guardPress`'s runner with `{ …extra, press, session }` on stdin (`extra` is `{ expect }` for a
   confirm), reads stdout line 1 as `{ next }` and keeps it as the token; line 2 is the stage or
   confirm object. Refused (no `next` line): `state: 'none'`, token cleared, nothing spent. The
   verb failed after the spend: the next token (if printed) is kept.
3. **Hold.** `checkTamper` before every press and at every turn end: this session's feed rows
   with `event: "press_issued"` (the record's `kind`, as `watch --follow --json` names it) counted
   against `issued`; more means something else claimed the window → `state: 'off'`, token
   cleared, one status warning, and every later answer is kept for the next session (below).
4. **Kept answers** (ux-design §9.12). With no token held (`none`, `off`, or the session id
   changed), a one-act answer (`approve`, `dismiss`, the four more verbs, `--predict`, `on`,
   `add --trust`) is kept in `$.store` `adage.kept` as `{ command, say, key, session, at }` and
   the band says `» kept for your next session`; a stage or confirm cannot be kept (the facts
   must be seen) and the grant pane shows the command to copy instead. At the next
   `classic.SessionStart` that issues a token, each kept answer at most 7 days old runs through
   `press(KEPT, …)`; one the binary refuses stays kept.
5. **Once a day.** `$.state.adage.asked` (mirrored to `$.store` `adage.asked`, 200 at most) records
   per waiting key `{ at, times, done }`: a dismissed dialog counts (`times`), re-asked a day later
   at most once more; `why` leaves it open; any other answer or any pane press on the item closes
   it (`done`).
6. **At `turn.complete`** (`reason: answer`, main loop, interactive, active): the ask runs off the
   hook (`quietly`), one at a time (`asking`), after `checkTamper` and the rate-limited `refresh`;
   `nextAsk(waiting(reflexes, packs), asked, now)` picks the item; `$.ui.ask(question, answers)`;
   the answer is recorded, then acted on. Nothing is asked headless (`-p`, the SDK).

### `PRESS_VERBS` shapes (`isPressAllowed`)

`review <approve|dismiss|strengthen|weaken|extinguish|pin> <track> <id>` · `review delegate
<track> <id> --level e2 --stage --json` · `… --level e2 --confirm` · `… --predict` · `delegate on`
(listed; nothing in the mod runs it yet) · `on` · `add <dir> --trust` (`<dir>` relative, may start
with `.`, never `..`, `.` or a flag). `install`, `config`, `share`, `train`, `packs`, `init` and
every `adage-exec` verb are not in the list.

## Noticed, learning and skills (D5)

- **Noticed.** The feed's `noticed` acts (`candidate` id, `support`, `s_min`, `review`) are drawn
  under the `ToolGroup` whose calls the row names (`tool_use_ids` → `Row.ids`), else the batch
  whose hooks ran when the row was written (the `avoided` rule). The words come from `status
  --json`'s `candidates[].gist` (`AdageCandidate { id, say, sequence }`, `sequence` = `trigger.kind
  === 'context'`), read when a noticed id is unknown (at most once per 5 s) and the drawing is
  invalidated; a candidate that is not a sequence, or not yet worded, draws nothing; the same
  candidate noticed again draws nothing. Only with `ui.noticed` on (the card's `ui.noticed`).
  The band's `n patterns noticed` counts the distinct candidates in this session's rows, while
  the pane is closed (`ui.open`/`ui.close` on `adage`).
- **Learning.** The toasts and the band's learner cell are B3's (`learnToast`, `learnerCell`);
  T-D5-02 now has an engine test over `learnStrength` with and without this session's id,
  `learnNew` with `sessions` (`toast.learned.formed`: `new, from this session`), `learnEvidence`
  (nothing) and `learnFailed` (once).
- **Skills.** `skill.prompt` asks `SkillPrompt` with `{ session_id, cwd, hook_event_name:
  "SkillPrompt", skill, text, via: "mod", observe? }`, 2 s; `{ text }` is returned as the prompt
  and parsed (`skillOf`: the heading `Adage verified these steps in this repository on
  YYYY-MM-DD:` and `- {command}: works` / `- {command}: stale ({signature})` lines) into
  `$.state.adage.skills` and `late.skill`; nothing → `next(e)`. The Skill tool row shows
  `tool.skill.invoked` when a predicted step of kind `skill` carries its id (the row's own
  `tool.predicted` line is dropped beneath it), and `tool.skill.verified` for a skill in
  `skills`; `/adage why` after it lists the steps (a skill verified after the last directive wins).

## Tests (L7, `claude plugin test plugin/claude-code`: 252 pass, 0 fail, 17 files)

| Case | Where |
| --- | --- |
| T-B4-01 | `dialog.test.ts` "T-B4-01 · the ask" (one ask at the turn end with the question and labels, nothing mid-turn, two waiting → one ask and `» 2 need review`; the press asked after the command hooks with the payload; headless nothing) |
| T-B4-02 | `dialog.test.ts` "T-B4-02 · the answers" (`enable` → `review approve` with `{ press, session }` on stdin, the next token spent next, the toast; `not this one` → `review dismiss`, never again; `why` → the pane focused with `e enable` / `n not this one`, still open a day later, settled by the pane press; a dismissed dialog returns once after 24 h); `ask.test.ts` |
| T-B4-03 | `dialog.test.ts` "T-B4-03 · two acts with the facts" (the stage argv, the focused `adage-allow` pane with every fact, `Code format=diff`, `a`/`n`/`d`, details with `c Copy`; `allow` → `--confirm` with `{ expect, press, session }`, the toast, the pane closed; a fresh diff redraws and records nothing, then confirms against the new digest; `not now` at either act runs nothing; the first-run line only when the fixture says so; a prediction offer is one act); `grant.test.ts` |
| T-B4-04, T-X-13 (L7) | `dialog.test.ts` "T-B4-04" (`/adage on` asks and runs nothing; `turn it on` → `[cli, on]` through the press; `leave it off`; a feed/learn line naming a verb or the press changes nothing); `dialog.test.ts` "T-X-04 · authority, with presses" and `pane.test.ts` "T-X-04" (person-only verbs only with `"press"` on stdin, never a token on argv, never outside); `commands.test.ts` |
| T-B4-05 | `dialog.test.ts` "T-B4-05" (the payload; a `press_issued` the mod did not cause → `off`, one warning, the answer kept and the band line; no press held → asked, kept in `$.store`, the command in the pane, applied at the next start that issues; a refused press says so) |
| T-B4-06 | `dialog.test.ts` "T-B4-06" (one Button per verb, `pane.verb.*` words, hotkeys, the dim command with Copy, the four more behind details; waiting for you lists enable, offer and pack; each press through the press) and `pane.test.ts` T-B3-05 |
| T-B4-07 | `pane.test.ts` "T-B4-07, Explore in browser" (B3's) |
| T-B4-08 | `pane.test.ts` T-B3-09 |
| T-B4-09 | `dialog.test.ts` "T-B4-09"; `ask.test.ts` |
| T-D5-01 | `notice.test.ts` (pure and through the engine, both surfaces; off by default) |
| T-D5-02 | `notice.test.ts` "T-D5-02"; `draw.test.ts` and `view.test.ts` (B3's) |
| T-D5-03, T-D4-04 | `notice.test.ts` "T-D5-03 … T-D4-04" |
| T-A7 (mod side) | `dialog.test.ts` T-B4-02/03/05 (the chain, stdin, the refusal) |
| T-U-01 doing (offline) | enable (T-B4-02), allow with the diff (T-B4-03), off and on (`pane.test.ts` T-B3-09, `dialog.test.ts` T-B4-04), inspect (T-B3-02), the health problem (T-B3-06), the map (T-B4-07): each through the mounted tree without a terminal |

## Departures from the plan, and why

1. **`press()` lives in `press.ts`, not `bridge.ts`**, with its own `PRESS_VERBS`; `bridge.ts`'s
   `ALLOWED_VERBS` is unchanged (and `check-plugin.mjs` holds it exactly), so the read-only list
   and the person-only list can never be confused. The plan's `delegate policy` and `delegate
   predict on` are not in `PRESS_VERBS`: the first execution grant's confirm writes the policy and
   the marker with the grant (A7), and a prediction grant is `review delegate … --predict`.
2. **The hooks are inner, not outer.** Every B4/D5 registration nests inside the B2/B3/C hooks
   (first outermost), so the pane's presses, the band's `kept`/`noticed` and `/adage on` reach
   their drawings through `late.ts` (handlers bound at `session.start`, values mirrored from
   `$.state`), and the Skill row's second marker is drawn inside the tool-row hook and the
   `tool.predicted` line it adds is dropped by `under`.
3. **The press is asked with `bin ?? hookBinary(...)`**, since the shim links the binary during
   the very command hook this registration wraps and activation runs after it.
4. **`» kept for your next session` outranks `» n need review`**: the reflex a kept answer
   decided still waits in the graph until the answer runs.
5. **`not now` on an offer or a pack records nothing through the press** (there is no verb for it);
   it is recorded as answered, so the ask does not return.
6. **The team-pack `why`** shows the pack name, the command and `this is the same trust you give
   CLAUDE.md`; the commit, author, date and key fingerprint are not in `status --json`'s
   `packs_awaiting_trust` (name, path, command), and the question says `Reflexes from {signer}`
   without the count for the same reason.
7. **`tool.skill.verified` says `step {i} stale`**, not `corrected`: `SkillPrompt` carries the
   stale step's signature, not a corrected command.
8. **A `ToolGroup` noticed line needs the candidate's words**, so it may appear one redraw after
   the batch (the `status --json` read lands and invalidates); the id is never drawn in its place.
9. **`Explore in browser`** stays as B3 built it (`map --port 0`, the map opens the browser);
   T-B4-07's `--no-open --port <free>` would leave the first press opening nothing.
10. **The grant pane's `d: details` and `c: copy` are both there** (T-B4-03 names `c copy`,
    ux-design `d: details`): copy sits inside details.
11. **The expanded row's details are kept in the one `expanded` key with a `!` suffix**, so the
    B3 `View` type needs no new field.

## Not verified here (live only), with the probes

- **The press window in a real session**: that `classic.SessionStart` reaches the mod after the
  plugin's own command hook (so `Press` finds the window open) and before the first
  `UserPromptSubmit`. Probe: a session in a repository with a nascent reflex; `/adage review` then
  `enable`; `adage review` history shows `via: press`; the spool holds one `press_issued` and one
  `press_spent`.
- **The dialog at `turn.complete`**: that `$.ui.ask` from a detached continuation after the hook
  returned draws the engine's dialog, that the answer's press runs with the session idle, and that
  a dismissed dialog rejects (the kit does). Probe: the same session, answer with the dialog's
  digits and with its dismiss key.
- **The model cannot press** (Q14): tell the model to enable the reflex; its `adage review
  approve` refuses and `adage hook claude Press` prints nothing; `ps` and the debug log never
  show the token.
- **Two acts live**: `see what it would do` against a real `--stage --json` (the sandbox), the
  diff as `Code format="diff"` in the terminal and the Desktop, `allow`'s `--confirm`, the policy
  and marker written on the first grant, and `make gen` running from the next session.
- **The grant pane and the inspect pane together** (two panes with `focus`), and `closeOnEscape`
  returning the keys.
- **`ui.open` / `ui.close` hooks on the mod's own pane**: that the engine dispatches them to the
  plugin that opened it, so the band's `n patterns noticed` cell hides while the pane is open.
- **`skill.prompt`** at the three entry points (`/name`, the Skill tool, a subagent preload)
  returning the annotated text, and the `◈` row under the Skill tool's own row (T-D4-05, Q13).
- **`ToolGroup` rows**: that `calls[].tool_use_id` is present on the terminal and the Desktop
  (2.1.288 declares it; "absent on a desktop host that predates it"), and that the feed's
  `tool_use_ids` match them.
- **A kept answer across a real restart**: `$.store` persistence and the toast at the next start.
- T-U-01's doing part with people; the recordings S9 Q12 and Q14 produce.

## Commands run (in /home/user/adage-poc)

| Command | Result |
| --- | --- |
| `claude plugin validate --strict plugin/claude-code` | `√ Validation passed`; hooks gain `session.start{isInteractive=true\|false}, classic.SessionStart{hook_event_name=SessionStart}, turn.complete{reason=answer}, skill.prompt, ui.render{component=ToolGroup}, ui.render{component=ToolUse, props has {tool=Skill}}, ui.render{component=Pane, requestId=adage-allow}, ui.open{id=adage}, ui.close{id=adage}`; calls gain `$.ui.ask (via askOn, turnEnded)`, `$.ui.close (via closeAllow)`, `$.process.run (via pressRunner, …)`, `$.store.get (via … applyKept, keep, loadStore)`, `$.ui.invalidate (via recountNoticed, …, wordCandidates)`; state reads/writes gain `adage.allow, adage.asked, adage.candidates, adage.press, adage.skills` |
| `claude plugin validate --strict plugin/claude-code-exec`, `claude plugin validate --strict .` | `√ Validation passed` |
| `claude plugin test plugin/claude-code` | `252 pass, 0 fail` across 17 files (about 17 s) |
| `/opt/node22/bin/tsc -p <scratch tsconfig>` over `hooks/` and `types/` with 2.1.288's `claude-code.d.ts` (strict, `noUncheckedIndexedAccess`) | no errors |
| `node scripts/check-plugin.mjs` | `plugins adage, adage-exec 0.1.0: ok` |
| `cargo test -p xtask` | 18 passed |
| the CI diff (`validate --strict … \| sed "s#$PWD/##g"` vs `VALIDATE.txt`) | differs by the lines above (hooks, calls, state reads/writes); not regenerated by me per the brief |

No file under `crates/` was edited (no `cargo fmt`/`clippy` to run); the goldens and
`adage-exec/tests/parity.rs` are untouched.

## Files touched

```text
plugin/claude-code/hooks/register.ts            (the B4 · D5 block at the end, and its helpers)
plugin/claude-code/hooks/press.ts               (B4's; the pack-directory name rule)
plugin/claude-code/hooks/ask.ts, grant.ts, notice.ts, late.ts, kinds.ts, commands.ts, view.ts, feed.ts (rowOf: ids)
plugin/claude-code/hooks/draw/allow.tsx (new), pane.tsx, rows.tsx
plugin/claude-code/types/index.d.ts             (AdageRow.ids, AdageStage.inputs)
plugin/claude-code/hooks/fixtures/kit.ts
plugin/claude-code/hooks/dialog.test.ts, grant.test.ts, notice.test.ts (new), ask.test.ts, pane.test.ts, commands.test.ts
docs/how-it-works.md, docs/usage.md, CHANGELOG.md ("Seeing it")
docs/claude-mod/impl/B4-D5-notes.md             (this file)
```

## Review fixes

The mod's (TypeScript) findings of the branch review, each with its test. Nothing under
`crates/` was touched.

| Finding | What changed | Test |
| --- | --- | --- |
| **H1** A kept answer was pressed with no person present, from `$.store` (a file the agent's tools can write) | `applyKept` is gone, and so is the `KEPT` gesture (`press.ts` `GESTURES` is `[ANSWER, BUTTON]`, and `scripts/check-plugin.mjs` now pins that literal). At the next start that holds a press, `askKept` puts each kept answer to the person in the engine's dialog: `ask.kept`, `Kept from your last session: {summary} ({command}). Run it now?` with `run it` / `drop it` (new in ux-design §10; §4.17 and §9.12 reworded). The question names the exact command, since the entry may not be the person's. `run it` presses with `ANSWER`, which spends the token then. `drop it` forgets the answer. A dismissed dialog keeps it for the next start. An entry no press could run (`pressArgv` refuses it) is dropped without being asked | `dialog.test.ts`: `no press held …` (asked again, lands on `run it`); `H1: a kept answer is never pressed unasked …`; `H1: an entry written into the store by something else …`; `ask.test.ts` `H1: … the exact command` |
| **H2** Kept answers were not tied to a repository | `Kept` gains `root` (`$.session.repo()?.root`, else the working directory), and `keptOf` drops an entry without one. `askKept` offers an answer only at a start in its own root and leaves the others untouched. `keep` deduplicates by command and root. The band's `kept` count covers this root only | `dialog.test.ts` `H2: an answer kept in one repository is not offered in another …`; `ask.test.ts` `H2: …` |
| **M1** A refused press threw away an unspent token | `pressCommand` leaves the press state alone (token, `held`, chain) when the binary refused before the spend (`!ok && refused`: no next line, as `adage on` prints where Adage is already on). A kept answer refused that way stays kept, which is what the old comment promised and the code did not do. `askOn` still opens its ask unconditionally (the review's third suggestion; with the token now kept, a refused `on` costs nothing) | `dialog.test.ts` `M1: a press the binary refuses … presses with the same token`; `M1: a kept answer the binary refuses before the spend stays kept …` |
| **L4** The bridge's binary paths had no `.exe` on Windows | `bridge.ts` `exeSuffix(OS)` (`.exe` when `OS=Windows_NT`). `hookBinary` and `cliBinary` take the suffix, so the mod probes `bin/hook.exe` and reads `bin/adage.exe`, the entries the shim copies there (`place` with `X=.exe`). The mod never runs `adage-exec` by name: `hook.exe` is whichever binary the shim chose. `activate` and `issuePress` read `OS` | `bridge.test.ts` `on Windows …`; `register.test.ts` `L4 · …` |
| **L8** `$.store.set('adage.harness', …)` had no `.catch` | `.catch(() => undefined)`, as every other store write has | `draw.test.ts` `L8: …` (the kit's `storeFails` denies the write; without the fix the version line is never cleared) |

`VALIDATE.txt` was regenerated the way CI diffs it. It changed only in the hooks line (`turn.complete` added, and the line is now long enough that the validator truncates it), the calls line (`$.session.repo (via repoRoot)`, `$.session.usage (via noteLedger)`, `askKept` in place of `applyKept`) and the env reads (`OS`). M6, the review's M2 (named M4 in the brief) and the ledger note are written up in C3-C9-mod-notes.md.

### Commands run (in /home/user/adage-poc)

| Command | Result |
| --- | --- |
| `claude plugin validate --strict plugin/claude-code`, `… plugin/claude-code-exec`, `… .` | `√ Validation passed`, all three |
| `claude plugin validate --strict plugin/claude-code 2>&1 \| sed "s#$PWD/##g"` vs `VALIDATE.txt` | regenerated; a fresh run diffs clean |
| `claude plugin test plugin/claude-code` | 272 pass, 0 fail across 18 files (252 before; 20 added) |
| `/opt/node22/bin/tsc -p <scratch tsconfig>` (strict, `noUncheckedIndexedAccess`, 2.1.288 declarations) | no errors |
| `node scripts/check-plugin.mjs` | `plugins adage, adage-exec 0.1.0: ok`. With a third gesture put back in `GESTURES`, it fails with the new message |

No cargo command was run (a Rust test run was in progress in the tree).
