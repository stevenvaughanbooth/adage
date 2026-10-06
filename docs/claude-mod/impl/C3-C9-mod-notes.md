# C3 · C5 · C7 · C8 · C9 (the mod's side), and `noticed` on the view seam: implementation notes

What was built in `plugin/claude-code` for the mod's half of chunks C3, C5, C7, C8 and C9 of
[the implementation plan](../implementation-plan.md), against the binary contracts in
[C3-C7-C8-D2-C9-E-notes.md](C3-C7-C8-D2-C9-E-notes.md) and [A1-A2-C5-notes.md](A1-A2-C5-notes.md),
with the small binary additions the mod's side needed. Built against Claude Code 2.1.288 and its
declaration file; nothing here ran in a live session. The module map stays as
[B2-B3-notes.md](B2-B3-notes.md) lists it, plus the files below.

## Module map (new or changed)

| File | What it is |
| --- | --- |
| `hooks/call.ts` | The Bash call (`tool.call { tool: 'Bash' }`): the dry run, the receipt branch, the learned replacement, the rewrite checked by the engine, every verdict's answer; `PreToolPayload`, `Verdict`, `rewritePlanOf`, `verdictAnswerOf`, `bashCall(e, next, deps)`. Takes closures over `$` from register.ts |
| `hooks/receipt.ts` | Pure: the binary's `receipt` plan and the final answer (`receiptPlanOf`, `receiptAnswerOf`), the `answered` mark the card is drawn from, `shortId` |
| `hooks/substitute.ts` | Pure now: `substituteOf` only (its hook body moved into call.ts, one decision per call) |
| `hooks/attach.ts` | Pure: the `Standing` payload and answer (`standingOf`, `subagentOf`), where each part goes (`withSection`, `withNote`, `withInstruction`, `withPreamble`), and the structural attachment test for compaction (`isAdageAttachment`, `withoutAttachments`) |
| `hooks/step.ts` | `turn.step`: `via: 'mod'`, `routine_model: true` in the payload when the person set one, routing on `routine`; chains and the finish line as the POC built them |
| `hooks/register.ts` | The `$` side: `standing($)` (asked once per session id, kept for the session), the `prompt.compose`, `tool.describe { tool: 'Bash' }`, `session.compact` and `agent.spawn` hooks, the `tool.call` deps (`answered`, `beside`, `made`, `rewrote`, `delivered`), the routine model read from the card (`refresh` for a session that draws, `settings` once for a headless one) |
| `hooks/feed.ts` | The follower's argv carries `--stdin ignore`; a `noticed` act keeps `support`, `s_min` and `review` for the line D5 draws |
| `hooks/bridge.ts` | The `watch` shape is `watch --follow --json --stdin ignore --repo <dir> --session <id> [--since <cursor>]` |
| `types/index.d.ts` | `AdageAct` gains `support`, `s_min`, `review`; `AdageMode` gains `noticed` |
| `hooks/{call,attach}.test.ts`, `step.test.ts`, `register.test.ts`, `bridge.test.ts`, `feed.test.ts`, `substitute.test.ts`, `view.test.ts`, `pane.test.ts` | The tests (table below; the last two had one-line changes for the new argv and the Standing ask) |
| `scripts/check-plugin.mjs` | Two rules more: the `watch` shape in bridge.ts holds `--stdin ignore`; `hooks/press.ts`'s `PRESS_VERBS` is exactly `review, delegate, on, add` and never `install`, `config`, `share`, `train`, `packs`, `init` |

## What each branch does

### The Bash call (C3, C6; mod architecture §3.3, §3.4)

`on('tool.call', { tool: 'Bash' })` → `bashCall`. Inert, or no session context: `next(e)`. Else one
dry run (`PreToolUse` with `dry_run: true`, `via: 'mod'`, `agent_id` in a subagent, `observe` in
observe mode, a 2,000 ms budget); no plan, or any failure: `next(e)` (the command hook decides as
before). With a plan, in this order, each branch asking Claude Code's own rules (`$.tool.check`,
which runs nothing) before the binary's final word, and the original command checked at most once:

1. **Receipt** (`plan.receipt.fresh` with a `result`, not observing, the rules answer `allow` on
   the call as written): the final ask `verdict: { answered: 'receipt' }`; its `receipt` answers
   the call: `{ result: { stdout: receipt.result, stderr: '', interrupted: false }, context:
   [receipt.note] }`, `next` is never called, the `answered` mark is kept and the `ToolResult` row
   draws the receipt card. Nothing from the binary (control, no longer fresh): on to 2.
   A plan whose receipt did not answer (not fresh, the rules `ask`/`deny`, withheld) keeps a
   `receipt` mark from the plan's `operation`, so the row draws `◠ adage receipt a1f2 covers this
   run · /adage why` under the real run (T-C3-07); a fresh one's note still reaches the model from
   the command hook, as before.
2. **Learned replacement** (`plan.substitute`): both commands checked; on the same decision the
   final ask `verdict: { engine: 'same', substitute: route }`; a `substitute` in the answer runs
   in the call's place with its one line as the result's `context` and under the dialog
   (`$.ui.notice`), and the `substituted` mark. Different decisions, or nothing: on to 3.
3. **Rewrite** (`plan.rewrite`): both commands checked; the final ask `verdict: { engine: 'same' |
   'differs' }`. The answer reads as the command hook's own output: `updatedInput.command` runs in
   the call's place (the `rewrite` mark, `↪ adage: pointed this test run at …` under the dialog),
   and `additionalContext` (the rewrite's word, or on `differs` the advice) is the result's
   `context`, because the command hook that follows prints nothing for a decided call. Nothing
   usable: `next(e)`.

Advice alone, a receipt that only rides beside, or no plan: `next(e)` and the command hook
delivers it. A refused call (`deny`) is returned as it was. In observe mode only the dry run and a
replacement's verdict are asked (with `observe`); nothing answers, replaces or rewrites.

### Standing text, attachment and compaction (C5, C9; §3.6–§3.8)

`standing($)` asks `Standing` (`{ session_id, cwd, hook_event_name: 'Standing', via: 'mod',
observe? }`, 2,000 ms) once per session id, at `session.start` and again at `classic.SessionStart`
(a `/clear` is a new id), off the hook's path; the answer (or an empty one on any failure) is kept
for the session so every request reads the same text and the prompt cache holds it.

- `prompt.compose`: `next(e)`'s sections plus `{ id: 'adage:procedures', text: standing, scope:
  'session' }` at the end (a section of that id already there is replaced, never doubled);
  unchanged when `standing` is empty.
- `tool.describe { tool: 'Bash' }`: `description + '\n\n' + environment`, once (a description
  already carrying the note is left); unchanged when empty.
- `session.compact`: `instructions` gains the binary's `compact` line after the person's own
  words (once); the transcript passes through untouched unless a row's own `origin` names this
  plugin (`{ kind: 'plugin', plugin: 'adage' }`), which 2.1.288's `SessionMessage` never carries,
  so nothing is removed there and never by text (Q5). A transcript that is not a list is passed
  on as it came.
- `agent.spawn` (not a fork): after the session's own `Standing`, one more with `agent: { type,
  description, prompt }`; a non-empty `subagent` is prepended to the subagent's prompt
  (`subagent + '\n\n' + prompt`); else, or on any failure, `next(e)`.

### Routing (C5; §3.5)

The person's `runtime.routine_model` (new, below) is read from `adage --json --for-session`
(`runtime.routine_model`): with the other reads at `refresh` for a session that draws, once at the
first main-loop step for a headless one. When set, every `TurnStep` payload carries
`routine_model: true` (never the name); an answer `{ routine: true }` on the before-request ask
sends the request as `next({ ...e, model, effort: 'low' })`. Never in observe mode; a prediction
beside `routine` is the response and no request goes out; `routine` on the ending ask changes
nothing.

### Chains and the finish line (C7, C8; §3.2)

As the POC built them (poc/P3-notes.md), now under the binary's `ending` rule: before request
`n > 0`, `{ index: n, ending: false }`; a `chain` answer yields one `tool`/`input` pair per call at
block indexes `0..n-1` and one `tool_use` stop, and `toolUses` lists them all (T-C7-03). At an
`end_turn` stop with no tool chunk, `{ index: n, ending: true }`; a prediction keeps the model's
text chunks, yields the calls at block indexes after the model's last block, and the model's stop
as `tool_use` with its `usage` kept (T-C8-02).

### `noticed` on the view seam (D2 → D5)

`feed.ts` reads `support`, `s_min` and `review` off a `noticed` act (`types/index.d.ts`), and
`refresh` reads `ui.noticed` off the card into `mode.noticed`; drawing the line is D5's.

## The binary additions the mod's side needed

- **`runtime.routine_model`** (`crates/adage-store/src/config.rs`, kind `Model`: one word of
  `[A-Za-z0-9._:/@-]`, at most 128, `""` for none, not guarded) and `adage --json` reports
  `"runtime": {"routine_model": "…"}` (`crates/adage-cli/src/status_card.rs`). The settings tab
  lists it (snapshot re-accepted, one row added). Documented in docs/live-learning.md.
- **The plan's `receipt` carries `operation`** (the journal row's id, sanitised, 12 characters),
  fresh or not: `adage_hook::PreNote.operation` (every literal gained the field) and
  `receipt_plan` prints it when non-empty (`crates/adage-hook/src/lib.rs`,
  `crates/adage-exec/src/hook.rs`). Nothing else in the plan or the command hook's output moved.
- **T-X-01 for the mod's events**: `crates/adage-cli/tests/hooks.rs::the_mods_events_fail_open`
  (`TurnStep`, `Standing`, `SkillPrompt`, the two `PreToolUse` phases: a panic in debug builds, a
  9 MiB payload, no session id → exit 0, nothing printed).

## Tests, by test-plan id (L7 unless noted)

| Id | Where |
| --- | --- |
| T-C3-04 | `call.test.ts` "T-C3-04": allow (the receipt is the result, two asks, one check, `next` never called), ask/deny (one ask, the call runs), ask with a rewrite planned (the rewrite path), withheld, no result, observe; "drawn": the card on both surfaces |
| T-C3-05 | `call.test.ts` "T-C3-05": same (`engine: same`, the new command runs, one notice, the word as context), differs (`engine: differs`, as written, the advice as context, no notice), advice alone, a downgrade, observe |
| T-C3-07 | `call.test.ts` "drawn": not fresh and fresh-but-withheld draw the beside line, never the card |
| T-C3-06 | L9, not run (below) |
| T-C5-02 | `attach.test.ts` "T-C5-02": the section once as `session`, identical at three composes from one ask; empty standing unchanged; nothing/garbage unchanged and not asked again; inert; observe; `/clear` |
| T-C5-03 | `attach.test.ts` "T-C5-03": the instruction once; no row removed without a handle, never by text; with a handle only the plugin's rows. The engine-level compaction is live-only (below) |
| T-C5-04 | `step.test.ts` "routing a step": routed only on `routine` with a model set, `routine_model: true` and never the name, not observing, not beside a prediction, not on the ending ask; `register.test.ts` "T-C5-04": a headless session reads the card once and routes from the next step; no model on the card routes nothing |
| T-C5-05 | the offline half in T-C5-02 (three composes, one text); L9 below |
| T-C7-03 | `step.test.ts` "a prediction before the request" (`chain`: two Read calls, indexes 0..1, one stop, `toolUses` lists both); `register.test.ts` end to end |
| T-C8-02 | `step.test.ts` "the finish line (ending)" |
| T-C9-03 | `attach.test.ts` "T-C9-03": the note once, another tool untouched, no environment unchanged; the subagent asked after the session's own and prepended; nothing applies or the binary fails: `next(e)` |
| T-X-01 | L2: `hooks.rs::the_mods_events_fail_open` (new), `a_panic_in_the_hook_is_recorded_without_the_payload_and_doctor_warns`, `malformed_oversized_and_unknown_payloads_produce_nothing`; L7: `call.test.ts` "T-X-02" (garbage, exit 1, cannot start, rules unreadable, a malformed final answer, inert), `attach.test.ts` (nothing, garbage, inert), the drawing deps failing (`call.test.ts` "the hook body alone") |
| T-X-05 | L2: `hooks.rs::standing_is_answered_once_per_session_and_spools_nothing` (control: empty text), adage-runtime `routing_is_consented_and_never_in_control`, adage-hook `the_verdict_in_the_control_arm_is_withheld_and_decided_once`, `a_receipt_is_never_the_result_in_control`; L7: the withheld cases above (the mod does nothing on an empty answer) |
| T-X-12 | `cargo test -p adage-exec --test parity` |
| T-X-04 | `bridge.test.ts` (the `watch` shape with `--stdin ignore`, the forms without it refused), `check-plugin.mjs` (the shape and the press list) |

## Departures

1. **`permission_mode` is not in the mod's payload.** `tool.call`'s input carries no permission
   mode (2.1.288: `ToolCallInput` is the tool's arguments, `tool_use_id` and `agentId`); the
   engine's own verdict (`$.tool.check`) is what the mod reads the mode through.
2. **The not-fresh beside line names the run by the plan's `operation`** (added to the binary's
   plan): the UX deck's `result.receipt.beside` needs an id, and a stale plan carried none.
3. **A failed `Standing` is the empty answer for the whole session**, not retried per request:
   a section that appears mid-session would spend the prompt cache (§3.6).
4. **Routing is read from the card, not asked of the binary**: the binary is told only that a
   model is set (A1-A2-C5-notes); the name never leaves the mod. A headless session reads the
   card once, at its first main-loop step, so the first step of such a session is never routed.
5. **`agent.spawn` does not route a subagent's model** (mod architecture §3.5 "the same rewrite for
   subagents"): the binary's `Standing` with `agent` answers no `routine`, and a subagent's steps
   are not asked about (`turn.step` stays on the main loop, §3.8).
6. **Compaction removes nothing on 2.1.288** (Q5): `SessionMessage` has no origin, so
   `isAdageAttachment` is false for every row; the instruction alone is added.

## Live-only (what to verify in a session, with the probe)

1. **The receipt as the result (T-C3-04 live, binary live-only 1).** In the zero-config Python
   fixture with a journaled passing run: run the same pytest command twice; the second `ToolResult`
   row is the card and the transcript on disk holds the receipt text as the tool result. Check
   whether `PostToolBatch`/`PostToolUse` fire for the answered call (`adage watch --follow --json`
   shows one `pretool` record with `answered: receipt` and whether a batch for the same
   `tool_use_id` follows).
2. **T-C3-06 (`$.tool.check` agreement).** Over the five settings fixtures of
   `pretool::keeps_permissions`, and `--allowedTools "Bash(.venv/bin/python:*)"`: the notice
   `↪ adage: pointed this test run at …` appears exactly where the planner's own reading rewrote,
   and the advice where it did not; the spool's `checked_by: engine`.
3. **T-C5-05 (the section byte-identical).** `claude --plugin-dir plugin/claude-code --debug`
   with a repository holding standing reflexes: read `$.prompt.compose()`'s `adage:procedures` at
   three points (a plugin test hook, or the debug log of the system prompt); the Bash tool's
   description carries the environment note once; the first prompt does not repeat the standing
   reflexes.
4. **Compaction (T-C5-03's engine half).** `/compact keep the todo list` in a session with the
   mod: the summarizer's instructions carry the binary's `compact` line after the person's; no
   transcript row is dropped. The test engine raises `session.compact` with no transcript and
   refuses a `next` without a list, so the hook's whole could not run offline.
5. **Routing (T-C5-04 live, Q11).** `adage config set runtime.routine_model <alias>`; a turn where
   a `fire` directive without a grant is pending: the step's request names that model at low
   effort (`--debug` shows the request's model); the spool's `turn_step` record has
   `decision.routine`; `ADAGE_ARM=control` never routes and records `would_route`.
6. **C7, C8 (T-C7-03 live, T-C8-02/04, Q15).** As the binary's notes list them: several yielded
   Read calls of one response run; the engine records the rewritten `tool_use` stop and the
   `adage_` ids on disk; ten of ten runs keep the text, run the call, continue and end with no
   `block`.
7. **Subagents (T-C9-03 live, T-C9-04).** An Explore subagent spawned for a work item: its
   transcript opens with the `<adage-reflex>` block and it makes no lookup of its own; its Bash
   calls reach the mod's `tool.call` with `agentId`, and the spool's calls carry `agent`.
8. **The follower with `--stdin ignore`** (B2-B3 issue 1): the Live tab keeps following in a
   real session (`ps` shows one `adage watch` child; it ends with the session).

## Open issues

- The other agent's files in flight at the time of this run: `ask.test.ts` (four `tsc` errors
  and one failing test against the new `nextAsk` signature), `commands.test.ts` (imports
  `ON_TEXT_UNCOPIED`), `pane.test.ts` (`/adage on` expectations). Not touched here.
- `adage-exec`'s `journal::tests::durable_completion_deduplicates_occurrence_but_allows_later_obligations`
  failed once under the full parallel run and passed twice alone (not touched here).

## Commands run (in /home/user/adage-poc)

| Command | Result |
| --- | --- |
| `claude plugin validate --strict plugin/claude-code`, `… plugin/claude-code-exec`, `… .` | `√ Validation passed`, all three; `VALIDATE.txt` regenerated as CI does (`sed "s#$PWD/##g"`) and a fresh run diffs clean. The diff against the committed file: the four hooks added (`prompt.compose`, `tool.describe{tool=Bash}`, `session.compact`, `agent.spawn`) and the state keys the B4/D5 agent declares; the B4/D5 block at the end of register.ts will move it again |
| `claude plugin test plugin/claude-code` | every suite of mine passes (`attach`, `call`, `step`, `register`, `substitute`, `bridge`, `feed`, `predict`, `reflex`, `view`, `draw`); the failures in the run are the other agent's files in flight (`ask.test.ts`, `commands.test.ts`, `pane.test.ts`, listed under open issues) |
| `tsc -p <scratch tsconfig>` over `hooks/` and `types/` with 2.1.288's declarations (strict, `noUncheckedIndexedAccess`) | no errors in any file of mine; `ask.test.ts` (4) and `commands.test.ts` (1) are the other agent's |
| `node scripts/check-plugin.mjs` | `plugins adage, adage-exec 0.1.0: ok` |
| `node scripts/test-stage-plugin.mjs` | `plugin archive integrity: verified pair accepted …` |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy -p adage-store -p adage-hook -p adage-exec -p adage-cli --all-targets -- -D warnings` | clean |
| `cargo test -p adage-store`, `-p adage-hook` | 20 and 49 passed |
| `cargo test -p adage-exec --lib` | 109 passed, 2 failed in the full parallel run: `a_fresh_receipt_answers_the_repeated_test_run_and_meets_its_finish_line` (fixed: the fixture's journal id sanitises to nothing, the assertion now follows the note), and the journal test above (passes alone, twice); the receipt test passes after the fix |
| `cargo test -p adage-cli --test mod_surfaces --test hooks` | 23 passed; 11 passed, 1 ignored (`the_mods_events_fail_open` and `the_card_as_json`'s `runtime` among them) |
| `cargo test -p adage-cli --bin adage -- ui::tests` | 41 passed after re-accepting two snapshots (`INSTA_UPDATE=always`): the settings tab gains the `runtime.routine_model` row (the frame's last row, `skills.invoke`, moves below the fold), the palette's match count 17 → 18 |
| `cargo test -p adage-exec --test parity` | 23 passed (T-X-12) |
| `cargo test -p xtask` | 18 passed |

## Review fixes

### M2 in REVIEW.md (M4 in the fix brief): a withheld substitution must not decide the call, mod half

The binary half belongs to the Rust agent: in `crates/adage-hook/src/lib.rs` `pretool_mod`, a
substitution verdict that is withheld (the control arm, or `observe`) must spool
`would_substitute` without `claim` marking the call decided. The mod's half needed no change in
logic. After a substitution verdict that made nothing, `bashCall` already falls through to the
interpreter rewrite. In the control arm it asks the rewrite verdict (`engine: same|differs`), and
once the binary leaves the call undecided, that verdict decides it (recorded withheld there). In
observe mode it hands the call on as written with no further verdict, so the classic
`PreToolUse` command hook makes today's U11 rewrite. `call.ts` now says this at the branch, and
two tests in `substitute.test.ts` fix the contract the binary fix relies on:

- `the control arm: a withheld substitution goes on to the rewrite verdict, and its answer
  rewrites the call`: the asks are `dry_run`, `{engine:same, substitute}`, `{engine:same}`.
- `observe mode: after the withheld substitution the call goes on as written with no further
  verdict`: the asks are `dry_run` and the substitute verdict, both with `observe`.

Live, the rewrite in these two cases happens only once the binary half lands. Until then the
binary's claim leaves both the rewrite verdict and the command hook with nothing to say.

### M6: `adage ledger note` at `turn.complete`

- `turn.step` (register.ts): after `turnStep` returns, a main-loop step of an active mod is kept
  as `{model, input, output, cache_read, cache_write, predicted}` (`ledger.ts` `stepOf`). A
  predicted step is one `turnStep` recorded with `isEnding: false` (no request went out). It
  counts no tokens, under the model the step asked for. A request that reported no usage is no
  step.
- A new `turn.complete` hook (no filter; main loop only; active only) runs `noteLedger` off the
  hook's path. The turn's steps are appended to the session's (`accrue`: numbered on across
  turns, started again when the session id changed, capped at the binary's 20,000). The note is
  `ledger.ts` `noteOf`: `{session_id, repo: <cwd>, harness: "claude-code", harness_version:
  $.session.version().version, started_at: $.session.usage().startedAt (0 when unavailable),
  ended_at: now, steps, cost_usd?: $.session.usage().cost.usd}`. It goes on stdin to `[cli,
  'ledger', 'note']` through the guarded runner (`bridge.ts` `note`, 10 s, in the session's
  directory, `CLAUDECODE=1`). The binary keeps one line per session, so each turn's note carries
  every step so far. A refusal (exit 2), a failure or a missing binary is ignored (fail open).
  `hook_ms` is not sent (optional; the mod does not time its own dispatch).
- Tests: `register.test.ts` `M6 · adage ledger note at the turn end` (the exact note over two
  turns; a subagent's turn and steps excluded; a refused note leaves the turn's answer as
  beneath; inert sends nothing); `ledger.test.ts` (step usage, numbering across turns and a
  /clear, the cap, the note's optional cost); `bridge.test.ts` `note (…)`.
- Live only: that `$.session.usage()` answers in the CLI (the types say the CLI always keeps a
  cost ledger), and that the binary accepts the note (its spool must hold this session's
  `session_start` for the repository `cwd` resolves to).

Commands and results are in B4-D5-notes.md, "Review fixes".
