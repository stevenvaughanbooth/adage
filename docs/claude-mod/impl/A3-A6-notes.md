# A3 · A4 · A5 · A6: implementation notes

What the binary now gives the mod to read and run: the live feed (`adage watch --follow --json`),
the harness ledger (`adage ledger note`) and its merge into the savings estimate, the status card
as JSON with `tokens.ts`, and the repository switch (`adage off`, `adage on`). Every contract
below is what the code does; the operational guide is `docs/live-learning.md` ("Turning it off
in a repository", "Files", "The live feed").

## A3 · `adage watch --follow --json`

### What was built

`crates/adage-cli/src/ui/follow.rs` (new), wired from `ui/mod.rs::watch` and declared in `cli.rs`
(`WatchArgs.follow | json | session | since`). It bypasses `Live` and ratatui entirely: its own
tail over the same files (`sessions/*.spool`, `learn.log`), `jobs::Loader` for health
(`health::gather` every 3 s on a worker thread) and the learner's queue files probed every
500 ms. One poll every 250 ms; each line is written and flushed whole.

### Command

```text
adage watch --follow --json [--repo <path>] [--session <id>] [--since <cursor>]
```

`--follow` and `--json` go together (either alone is a usage error); `--session` and `--since`
need `--follow`; `--follow` conflicts with `--once`, `--demo`, `--size`. The architecture's feed
loop passes `--repo e.cwd`: records and `learn.log` lines are those of that repository's key
(outside a repository, all). A fresh follower first sends the recent backfill `adage watch` uses
(the last 256 KiB of spools changed within 24 h, the last 64 KiB of `learn.log`).

### Line shapes (one JSON object per line; every line has `kind` and `cursor`)

```json
{"kind":"decision","cursor":"1~<at>~<log>~<stem>:<off>:<n>,…","ts":1790000000000,"session":"<spool stem>",
 "harness":"claude","event":"prompt|tools|failure|stop|start|end|pretool|step|<other>","repo":"<key>",
 "control":true,
 "acts":[ … ],"observed":2,"unrouted":false,"texts":[{"route":"…","text":"<the directive as the agent read it>"}]}
```

`control` is present (and `true`) only on another session's row in the control arm. `acts[]`:

| `act` | Fields | From |
| --- | --- | --- |
| `directed` | `route`, `strength` (`fire`/`prime`/`inhibit`), `detail` (the command or first sentence), `instinct` | a delivered directive |
| `withheld` | `route`, `strength` | control: a would-have directive |
| `followed`, `ignored`, `misfired`, `met` | `route` | judgements (`met` = finish line) |
| `unaided`, `not_reached` | `route` | control judgements |
| `stop_request` / `stop_withheld` | | the finish line asked to continue / withheld |
| `pretool` | `rewrite` (bool), `what` (`test`, `pip-install`, …), `interpreter`, `withheld` | a `PreToolUse` record |
| `predicted` | `route` | a step Adage took in the model's place (`turn_step` with `decision.predicted`) |
| `would_predict` | `route` | the control arm's (or past-cap) prediction |
| `judged` | `what`, `route` | any other judgement kind, `_` as space (`substituted`) |

```json
{"kind":"learn","cursor":"…","ts":…,"repo":"<key>|null","line":{ …the learn.log object as written… },
 "ticks":[{"text":"…","short":"…","tone":"run|change|error","strength":"fire|prime|inhibit|null","repo":"…"}]}
{"kind":"health","cursor":"…","lights":[{"label":"hooks claude","lamp":"ok|warn|fail|off","hint":"adage doctor"|null}],
 "state":{"word":"healthy|check|problem","lamp":"fire|prime|inhibit","check":"<label>"},"off":false}
{"kind":"learner","cursor":"…","repo":"<key>","state":"waiting|running|idle","next_at":1790000038000|null}
```

`health` is sent at start and whenever the object changes; `state` is the worst doctor check
worded as the status card words it (`check` absent when healthy); `off` is `.adage/disabled` or
`ADAGE_DISABLE=1`. `learner` is sent at start and on change: `waiting` while `<slug>.spawn.json`'s
`after` is in the future (then `next_at = after`), else `running` while `<slug>.lock` is held,
else `idle`. No `learner` line outside a repository.

### Cursors

`1~<at ms>~<learn.log offset>~<stem>:<offset>:<passed>,…`, opaque to the mod. Spool records are
CRC-framed and one `read_from` returns many, so a spool's position is "the offset a read began at
and how many of its records were passed"; records of all spools are merged by `ts` keeping each
spool's own order, so the count is always a prefix. Only spools that held a record for the
followed repository are named; on resume a spool the cursor does not name and that changed within
5 s of (or after) `<at>` is read from 0 (it held nothing of this repository then, so nothing is
repeated), any other one starts at its end. `health` and `learner` lines carry the current
position (they move none). A cursor of another format exits 2 (`--since takes a cursor …`).

### The current session (`--session <id>`, compared with the spool's stem)

From its rows: `control` is never written; `withheld`, `unaided`, `not_reached`,
`stop_withheld`, `would_predict` and a withheld `pretool` are removed; in the control arm every
judgement is removed too (each exists only because a directive was withheld); the `texts` keep
only delivered directives; a row that had something removed and is left with no act is not sent
(a treated session would not have spooled it). A quiet row with nothing removed (a start, a
batch with observed calls) is sent as is. Other sessions: as `adage watch` shows them.

### Lifetime

Exit 0 on: stdin EOF **when stdin is a pipe** (a follower started with stdin `/dev/null`
follows until stopped: otherwise a spawn with no stdin would end at once), SIGTERM (a handler
calls `_exit(0)`), the parent's death (`PR_SET_PDEATHSIG` → SIGTERM on Linux; `getppid()` changing
on every Unix), stdout closed (EPIPE). No escape sequence is ever written.

### Tests

| Id | Test |
| --- | --- |
| T-A3-01 | `mod_surfaces::the_feed_streams` |
| T-A3-02 | `mod_surfaces::the_feed_resumes_across_processes` (two spools, resume from row 3, then follows; a bad cursor exits 2) |
| T-A3-03 | `mod_surfaces::the_feed_says_where_the_learner_and_health_stand` (idle → waiting with `next_at` → running; health again after `adage off`) |
| T-A3-04 | `mod_surfaces::the_current_session_never_names_the_arm`; unit: `ui::follow::tests::{the_current_sessions_rows_never_name_the_arm, a_withheld_pretool_record_is_not_sent_for_the_current_session}` |
| T-A3-05 | `mod_surfaces::the_feed_ends_with_its_parent` (`/bin/sh` parent killed, follower gone within 2 s; no escape byte); `--once` and UI snapshots unchanged (`watch.rs`, `ui::tests`) |
| T-A3-06 | `mod_surfaces::the_feed_is_steady`, `#[ignore]` (100 s): 200 rows at 2 Hz across seven spools with a forced restart at row 100: rows 1..200 exactly once, in order; RSS spread ≤ 10 MiB. Run once: **passed** (102 s) |
| unit | `ui::follow::tests::{cursors_round_trip_and_refuse_what_they_did_not_write, merging_keeps_each_spools_order}` |

## A4 · `adage ledger note` and the harness ledger

### What was built

- `crates/adage-app/src/harness_ledger.rs` (new): `Note`, `StepUsage`, `parse` (shape checks),
  `check_session` (spool integrity), `record` (one line per session, under `.harness.lock`),
  `overlay` (in memory) and `merge` (on disk, under the ledger lock), `ledger_lock`.
- `crates/adage-app/src/savings.rs`: `gather` lays the notes over the ledger; `refresh` merges
  them into the ledger file first and fills `Summary.harness`; `read` recomputes when the notes
  file is newer than the summary; `Counts.predicted_steps` from spool `turn_step` records with
  `decision.predicted` (the POC's open issue in P1-notes item 5, savings half).
- `crates/adage-cli/src/ledger.rs` (new) and `cli.rs` `Ledger { Note }`; `main.rs` dispatch.
- `crates/adage-cli/src/autolearn.rs`: the learner holds the same ledger lock around
  `Ledger::update`, so a transcript pass and a merge never write at once.
- `adage savings` prints `n steps without a model request` in its counts line when non-zero.

### stdin contract

```json
{ "session_id": "…", "repo": "<cwd or key>", "harness": "claude-code", "harness_version": "2.1.288",
  "started_at": 1790000000000 | "2026-10-03T10:00:00Z", "ended_at": …,
  "steps": [{ "index": 0, "model": "claude-sonnet-4-6", "input": 12, "output": 340,
              "cache_read": 51000, "cache_write": 900, "predicted": false }],
  "cost_usd": 0.0412, "hook_ms": [3, 2, 4] }
```

Required: `session_id` (≤ 256 bytes), `repo`, `harness` (only `claude-code`), `harness_version`
(digits-and-dots, 2–4 parts, optional `-`/`+` suffix), `steps[].index`. Optional: times (0),
token counts (0), `model` (empty), `predicted` (false), `cost_usd` (finite, 0 ≤ x < 1e6),
`hook_ms` (u32 each). Limits: 4 MiB, 20,000 steps, 100,000 `hook_ms`. `repo` that is an absolute
existing directory is resolved to its repository key the way hooks resolve it; anything else is
taken as the key. Accepted only when `sessions/<sanitised id>.spool` holds a `kind:
"session_start"` record whose `repo` equals that key. Exit 0, stdout empty; refused: exit 2,
`error  note not recorded: <reason>` on stderr, nothing written. Allowed with `CLAUDECODE` set.

### The file: `$ADAGE_HOME/ledger/.harness.jsonl`

One line per session (the note above plus `"noted_at": <ms>`, `repo` always the key), replaced
by session id, rewritten atomically under `ledger/.harness.lock`; notes whose `max(ended_at,
noted_at)` is over 180 days old are dropped when the file is written.

### The merge

`<slug>.jsonl`'s line for the session becomes: `harness`, `harness_version`, `models` (of the
answered steps), `started_at` (the note's, else the transcript's), `ended_at` (the later),
`model_turns` = steps with `predicted: false`, `tokens` = the four counts summed over every step,
`cost` = `{usd: cost_usd, source: "harness"}` when given, else the answered steps priced from the
pinned table (`priced@<date>`, or `none` for an unlisted model); `tool_calls`, `first_edit_turn`
and `asked_for_help` stay the transcript's. `SessionSummary` is unchanged (no adage-learn edit):
per-step data and `hook_ms` stay in the notes file, read again at every refresh.

### `adage savings --json` additions

```json
"counts": { …, "predicted_steps": 1 },
"harness": { "sessions": 8, "predicted_steps": 16,
             "recent": [{ "session_id": "…", "source": "harness", "model_turns": 12, "predicted_steps": 2 }] }
```

`harness` is absent when no note exists for the repository; `recent` holds at most 20, newest
first.

### Tests

| Id | Test |
| --- | --- |
| T-A4-01 | `adage_app::harness_ledger::tests::the_merge` |
| T-A4-02 | `adage_app::harness_ledger::tests::steps_and_predictions` |
| T-A4-03 | `mod_surfaces::ledger_note_records_a_session_its_spool_knows` |
| T-A4-04 | `zero_config_plugin::savings_gate` with `population()` extended (every fifth treated session noted with two predicted steps, its transcript line counting them; asserts 8 sessions, 16 predicted steps, `source: harness`, merged `model_turns` = answered steps); `pricing_honesty` unchanged and passing |
| T-A4-05 | `mod_surfaces::concurrent_merges_leave_one_line_per_session` (8 notes and 8 `adage savings` processes at once) |
| unit | `harness_ledger::tests::{a_note_needs_its_session_and_a_version, old_notes_are_dropped}` |

## A5 · The status card as JSON, `tokens.ts`

### Status card

`status_card.rs` now builds a `Card { header, repo, connected, off, health: Option<Lamp>, rows:
Vec<Row>, next }`; text and JSON both render from it, so the rows match one to one. Bare `adage
--json` (root flag `--json`; hidden `--for-session`):

```json
{ "version": "0.1.0", "header": "adage 0.1.0 · <key> · ● healthy", "repo": "<key>"|null,
  "connected": true, "off": false,
  "health": { "word": "healthy|check|problem", "lamp": "fire|prime|inhibit", "check": "<label>"|null } | null,
  "rows": [{ "key": "reflexes", "text": "none active yet" }, { "key": "", "text": "<continuation>" },
           { "text": "<a line of its own, e.g. a repository outside one>" }],
  "next": [{ "command": "adage ui", "why": "review, health and settings" }, …] }
```

Texts are the card's own, uncoloured (UTF-8 glyphs). `health` is null before an agent is
connected. `--for-session` replaces `adage ui` with `/adage watch` ("live rows, reflexes and
health") and, when off, `adage on` with `/adage on`. When the repository is off the header ends
`○ off in this repository` in place of the lamp and `next` leads with `adage on · Adage is off in
this repository (.adage/disabled)`.

### `tokens.ts`

`cargo xtask tokens` writes a third output, `plugin/claude-code/hooks/tokens.ts`:
`GLYPHS` (`{ name: [unicode, ascii] }` for every glyph in the tokens' `glyph` group, camel-cased
names), `GlyphName`, `glyph(name, ascii)`, `ROLES` (`ink: "text"`, `ink2: "inactive"`, `ink3:
"subtle"`, `accent: "suggestion"`, `command: "permission"`, `fire: "success"`, `prime:
"warning"`, `inhibit: "error"`, `cost: "claude"`) and `Role`. The theme keys were read from the
2.1.288 binary's theme table. The mapping lives in `xtask/src/main.rs` `ROLES`.

### Tests

| Id | Test |
| --- | --- |
| T-A5-01 | `xtask::tests::the_committed_tokens_are_current` (now also `tokens.ts`, every glyph with its twin, every role with a key) |
| T-A5-02 | `mod_surfaces::the_card_as_json` (rows one to one with the printed card; insta `card-text` and `card-json`; `--for-session`) |

## A6 · `adage off`, `adage on`

`crates/adage-cli/src/switch.rs` (new), `cli.rs` (`Off`, `On`, listed under "When something is
off"), `main.rs` dispatch.

| Command | Inside a repository | Outside |
| --- | --- | --- |
| `adage off` | writes `<root>/.adage/disabled` (idempotent), anywhere, `CLAUDECODE` included; prints `adage is off for this repository (.adage/disabled). adage on turns it back on.` | `not inside a repository: \`adage off\` turns Adage off for the repository it runs in`, exit 0 |
| `adage on` | if the file exists: a person's decision, then removes it (and `.adage/` if empty); prints `adage is on for this repository.` (plus ` ADAGE_DISABLE=1 still keeps every hook off where it is set.` when set) | `not inside a repository: \`adage on\` turns Adage on for the repository it runs in`, exit 0 |

The person check in `on` is `crate::review::person_or_press("on", …)`: the exec agent (A7)
replaced my `adage_app::person::require("on")` with it in this file during the wave, so a press
token on stdin also passes. Its refusal under `AGENT_ENV` is the person-only message (`… is a
decision for a person (it changes what is shared, trusted or configured) …`; `person.rs` has no
`on` arm of its own: the exec agent may want to add one).

`adage-exec version` existed from the POC (P2) and is tested there
(`adage-exec/tests/cli.rs::version_is_one_line_and_reads_nothing`); nothing changed in adage-exec.

| Id | Test |
| --- | --- |
| T-A6-01 | `mod_surfaces::off_from_anywhere` (twice under `CLAUDECODE=1`; a `SessionStart` hook then prints nothing and spools nothing; the card's first line and `next`; `--for-session` `/adage on`; `adage doctor` names the switch) |
| T-A6-02 | `mod_surfaces::on_is_a_persons` (refused under each of the four markers, file kept; removed at a terminal with `ADAGE_ARM` unset; both verbs one line outside a repository) |
| T-A6-03 | binary half in `adage-exec/tests/cli.rs` (POC); the paired latency is not run here (below) |

## Departures from the plan

- **The notes file is `ledger/.harness.jsonl`**, not `ledger/harness.jsonl`: a repository keyed
  `harness` has the slug `harness`, whose ledger is `ledger/harness.jsonl`. Slugs never begin
  with a dot.
- **No change to `adage-learn`.** The plan put `StepUsage`, `hook_ms` and `merge_harness` on
  `SessionSummary`/`Ledger`; the brief put the merge in `adage-app/src/savings.rs`. The ledger
  schema is untouched; the per-step data stays in the notes file and is laid over the ledger at
  every refresh (and written into `<slug>.jsonl` under its lock), so "a later transcript pass
  never replaces it" holds at every refresh rather than inside `Ledger::update`. Between the
  learner's `update` and its `refresh` (same process, milliseconds) the file briefly holds the
  transcript line.
- **`cost.source` is `harness` only when the note carries `cost_usd`**; without it the note's
  tokens are priced from the table (`priced@…`), so a dollar figure the engine did not report is
  never labelled as the harness's.
- **Transcript-only sessions' turns are not reduced by spool predictions.** A transcript's
  `model_turns` counts the model's own messages; whether a predicted step appears there as one is
  not known offline, so only a harness note changes a unit's turns (conservative: never
  overstates a saving). The spool count is reported as `counts.predicted_steps`.
- **`xtask/src/main.rs` was edited** (not in the brief's list, but the plan's A5 generates
  `tokens.ts` there and T-A5-01 tests it there): one new output and its test lines.
- **The status card's `--json` is a root flag** (`adage --json`); `--for-session` is hidden.
- **The learner state `waiting` wins over `running`** while `after` is in the future (a learner
  holds its lock while it waits); the test plan's sequence is met by writing a past `after` before
  taking the lock.

## Not verified here (live only)

- Whether `$.process.spawn` hands the child a stdin pipe that stays open. If the engine closes it
  at once, the follower ends at once: the mod should spawn it with an open stdin, or rely on the
  parent-death path (stdin `/dev/null` follows until the parent goes).
- The theme-key names were read from the 2.1.288 binary, not from a declaration file; `ROLES`
  should be re-checked against `types/index.d.ts` once the engine ships one.
- T-A6-03's paired `hook_latency` (release builds of base and branch): not run (disk). A6 did not
  touch `adage-exec` or the hook path.

## Open issues for other owners

- `scripts/check-plugin.mjs` fails until A8 merges the plugins: it requires both plugins' hooks
  modules to list the same files, and `tokens.ts` exists only in `plugin/claude-code` (the brief
  forbids writing elsewhere in `plugin/`). The restructuring agent carries it over, or
  `cargo xtask tokens` gains `plugin/claude-code-exec` until A8.
- `adage doctor`'s kill-switch fix text still says "remove the .adage/disabled file"; `adage on`
  is now the verb (doctor belongs to the A2 agent).

## Files touched

```text
crates/adage-cli/src/ui/follow.rs           (new)
crates/adage-cli/src/ui/mod.rs              (watch: --follow --json --session --since)
crates/adage-cli/src/switch.rs              (new; A7 later swapped in person_or_press)
crates/adage-cli/src/ledger.rs              (new)
crates/adage-cli/src/status_card.rs         (Card model, --json, --for-session, off line)
crates/adage-cli/src/cli.rs                 (WatchArgs, Off, On, Ledger, root --json/--for-session, overview, flag table)
crates/adage-cli/src/main.rs                (mods, dispatch)
crates/adage-cli/src/savings.rs             (predicted steps in the counts line; test Counts)
crates/adage-cli/src/autolearn.rs           (ledger lock around Ledger::update)
crates/adage-cli/src/snapshots/adage__cli__tests__top_level_help.snap
crates/adage-cli/tests/mod_surfaces.rs      (new)
crates/adage-cli/tests/snapshots/mod_surfaces__card-text.snap, mod_surfaces__card-json.snap (new)
crates/adage-cli/tests/zero_config_plugin.rs (population(): harness notes; savings_gate asserts)
crates/adage-app/src/harness_ledger.rs      (new)
crates/adage-app/src/savings.rs, lib.rs
xtask/src/main.rs                           (tokens.ts output and test)
plugin/claude-code/hooks/tokens.ts          (generated)
docs/live-learning.md, docs/how-it-works.md, docs/usage.md, docs/design/style.md, CHANGELOG.md
```

## Commands run (in /home/user/adage-poc)

- `cargo fmt --all` then `cargo fmt --all --check`: clean.
- `cargo clippy -p adage-cli -p adage-app --all-targets -- -D warnings`: clean (an earlier run
  stopped on another agent's `doctor_latency.rs`, since fixed by its owner).
- `cargo test -p adage-app`: 30 passed.
- `cargo test -p adage-cli --test mod_surfaces`: 10 passed, 1 ignored;
  `-- --ignored the_feed_is_steady`: passed (102 s).
- `cargo test -p adage-cli --test zero_config_plugin -- savings_gate pricing_honesty`: 2 passed.
- `CARGO_INCREMENTAL=0 cargo test -j 2 --no-fail-fast -p adage-cli -p adage-app` (disk is
  shared and tight): every target passes except, all outside these chunks and in files I did not
  touch: `ui::tests::settings_show_rewrites_off_by_the_older_marker_and_notices` (a new
  `skills.invoke` config key changes the settings snapshot), `live_learn::learn_auto_logs_what_changed`
  (D1's `change: "candidate"` lines), and, in the first run only, three `zero_config_plugin`
  tests that failed on a full disk / an in-progress A1 rewrite and passed on rerun.
- `cargo test -p xtask`: 18 passed. `cargo run -p xtask -- tokens`: wrote `tokens.ts`.
- `claude plugin validate --strict plugin/claude-code` and `plugin/claude-code-exec`: pass;
  output identical to `VALIDATE.txt` but for absolute paths (CI's form). `claude plugin test`
  both plugins: 53 passed each. `node scripts/check-plugin.mjs`: fails on the module-file parity
  check above (expected until A8).
