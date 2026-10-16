# O6 · O9 (and O8's T-O8-04) notes: telling the person, and the in-session update

Built on `claude/mods-poc-m98u3o` at `d29f5ce` (resumed after a container restart; the first
session's edits were kept). The plan is `docs/claude-mod/onboarding-plan.md` §1.3, O6, O9 and
O8's shim item; O5/O7/O8 (`impl/O5-O7-O8-notes.md`) built the cache, `update::newer`,
`update::card_json` and the transaction this chunk tells about and presses.

## For the agents working beside this one (read first)

- **The fourteenth press site.** `adage_app::person::PersonVerb::Update` (`cmd()` = `update`, its
  refusal says `it replaces Adage's own binaries on this machine`). `adage update` calls
  `review::person_or_press(PersonVerb::Update, …)` only when it is **bare**: any flag
  (`--yes`, `--from`, `--branch`, `--rollback`/`--back`) takes `person::require` alone, which
  never reads a press. `--check` and `--print` decide nothing and need no one. `--finish` (hidden)
  is unchanged.
- **The mod's press list** is `['review', 'delegate', 'on', 'add', 'update']`;
  `isPressAllowed([cli, 'update'])` and nothing else of `update`. `scripts/check-plugin.mjs` holds
  the literal (`review,delegate,on,add,update`); no other edit there.
- **The tail.** Under a press, `adage update`'s stdout is: line 1 `{"next": "<token>"}`, then
  Adage's own lines, then one JSON object as the **last** line:

  ```json
  {"updated": {"from": "0.1.0", "to": "0.2.0", "effect": "next session"}}
  {"latest": {"version": "0.2.0"}}
  {"manual": {"command": "adage update", "say": "this adage was installed from source: run adage update in a terminal"}}
  {"manual": {"command": "/plugin", "say": "In Claude Code: /plugin → adage → Update"}}
  ```

  Nothing of it is printed at a terminal. The package manager's own output goes to standard error
  under a press. The mod reads it with `reads.ts` `updatedOf(lines)`.
- **The mod's state.** `AdageMode.update: AdageUpdate | null` (`types/index.d.ts`, `plugin.ts`
  `MODE0`), `{ have, latest, next, notes, band }`, set by `refresh.ts` from the card's `update`
  object (`reads.ts` `updateOf`). One `$.store` key, `latest` (`plugin.ts` `STORE_KEYS`, one cell
  in `register.ts`): the newest release the band's line was shown for.
- **`Card`** (`reads.ts`) gained `version` and `update?` (`Offer` = `Update` without `band`).
- **`PressHandlers`** (`draw/pane.tsx`) gained `update()` and `askUpdate(offer)`;
  **`PaneData`** gained `update`. **`CommandDeps`** gained `askUpdate`. **`BandInput`** gained
  `update: { latest, next } | null`.

## What was built

### O6 · telling the person (the binary decides; every surface reads the cache, no request)

| Surface | As built |
| --- | --- |
| Card row (O10 drew it; this chunk adds it to the not-connected card too) | `update      0.2.0 is out (you have 0.1.0) → adage update` (`→ /adage update` with `--for-session`; `->` in ASCII); connected, `next  adage update · 0.2.0 is out` first; not connected, `adage init` stays first |
| `adage --json` | `update` object (O5's `card_json`): `null` unless the cache names a release newer than this binary |
| `adage version` (`update::version_line`) | `adage 0.1.0`, or `adage 0.1.0 · 0.2.0 is out (adage update)`; the first two words stay what every `version` reader parses |
| doctor `binary` | stays `Pass`; detail `adage 0.1.0 at <path> · 0.2.0 is out`; `Fix::run("adage update")` (`--json` `action: {kind: run, command: adage update}`); text shows `update adage update` under it; `next adage update · a newer Adage is out; everything else checks out` only when nothing fails or warns; counts and exit code unchanged |
| Notice (`autolearn::notice::compose`, fifth and lowest part, kind `update` only alone) | settings-file or `PATH` binary: `adage: 0.2.0 is out (you have 0.1.0); run adage update.`; the plugin's binary: `adage: 0.2.0 is out (you have 0.1.0); /adage update brings it.`; after one higher part it is the second; dropped when two higher parts fill the line |
| `adage ui` Home | first line ` adage 0.1.0 · 0.2.0 is out · u update (adage update: Adage, its hooks and the plugin together)` in plain ink, the `u` in the accent; `u` prepares `Tool::Update` |
| `adage ui` palette | `Update Adage` · `adage update` → `Tool::Update` |
| `Tool::Update` | runs `adage update` as a job after the confirm: `Update Adage 0.1.0 to 0.2.0, the way it was installed: its binaries, its hooks and the plugin.` / `The binaries it replaces are kept; adage update --rollback puts them back.` (`Check for a newer Adage than 0.1.0, and update to it if there is one: …` when none is known) |
| `adage watch` header | `… · 0.2.0 is out · adage update` as a fact, the first to go when the width runs short |
| Map | O10 built the graph's `adage: { version, latest, channel }` and the TopBar |

The notice is said **once per machine per release**: the notice basis records `latest`; a
repository whose own notice, or another repository's notice on this machine (`said_elsewhere`),
said it is not told again; one crowded out by two higher parts says it at its next notice. The
learner recomposes the notice right after a release check that found a new version.

Learner notices from the plugin's binary (`Voice::Plugin`, the running binary under
`plugins/cache` or `plugins/data`) say `/adage review` and `/adage health`; none says `in a
terminal` (T-O9-10).

### O9 · the in-session update

- **Press lifecycle for `update`.** `/adage update` (typed by the person) → `commands.ts` reads the
  card (`--json --for-session`) → no card: `adage is not answering here; run adage doctor in a
  terminal.`; no `update`: `adage 0.1.0 is the latest.`; no press can be bound (headless, inert):
  `Adage 0.2.0 is out; run adage update in a terminal.`; else `dialog.ts` `askUpdate` opens
  `Adage 0.2.0 is out (you have 0.1.0). Update now? It takes effect in your next session.` with
  `update` / `not now` / `what changes`. `update` → `pressUpdate(ANSWER)`: with a press held, the
  guarded runner runs exactly `[cli, 'update']`, `{ press, session }` on stdin, `CLAUDECODE=1`;
  the binary spends the token (first line `{next}`), updates a fetch channel or answers `manual`,
  and prints the tail; the mod holds the next token exactly as `token.ts` `pressCommand` does
  (a refusal before the spend keeps the token) and toasts from the tail: `adage updated 0.1.0 →
  0.2.0 · takes effect in your next session`, `adage 0.2.0 is the latest.`, or the `manual`
  `say`; a failure after the spend: `adage could not update ({last stderr line}) · run adage
  update in a terminal`. With no press held the answer is kept (`command: adage update`, `say:
  update Adage to 0.2.0`, `key: update:0.2.0`) and at the next start in that repository asked as
  `Kept from your last session: update Adage to 0.2.0 (adage update). Run it now?`, pressed only on
  `run it`. `what changes` opens the pane on Health; `not now` records the ask (`update:<v>`).
  Health's version row Button `u: update` is the same press (`BUTTON`).
- **The binary under a press** (`update.rs`): `By::Press`; runs only `plugin` (`claude plugin
  update`), `npm` and `release`; `source`, `checkout`, `brew` answer `manual` and run nothing
  (`This adage was installed from {channel}; its update runs in a terminal.`); `previous.json`
  `via: press`; the finish writes no registration (`repoint_or_say`): `hooks  {n} registrations
  still run {have} · run adage update in a terminal` / `… still need this version's hook events …`
  (also said by a terminal-less "nothing to do" run); a terminal's later `adage update` (even with
  nothing newer) re-points them.
- **The band** (`view/band.ts` `updateLine`): `» adage 0.2.0 is out · /adage update` (the command
  as the card words it; `>> … - …` in ASCII), after `review` and before the harness-version line;
  shown while `mode.update.band` and before the first prompt; `refresh.ts` sets `band` once per
  release per machine (`$.store` `latest` written when a session first takes it) and drops it once
  a prompt was submitted.
- **Health's `version` row** (`draw/pane.tsx` `versionRow`): `version  adage 0.1.0 · 0.2.0 is out`
  in plain ink, the notes dim, `u: update` (only when presses are bound) and `Copy: adage update`.

### O8 · T-O8-04

`plugin/claude-code/scripts/session-start` `superseded <name> <sha>`, called after each bundled
link: removes `bin/<name>-<64 hex>` files other than the linked one that no link in `bin/` names;
`adage-exec-*`/`adage-train-*` are never matched by `adage`'s sweep (exact length), and a non-digest
name stays.

## Tests (ids from the plan)

| Id | Where |
| --- | --- |
| T-O6-01 | `tests/update.rs::every_surface_says_a_newer_release_and_none_turns_amber` (plain, ASCII, colour rows; header lamp unchanged; connected and not) — assertions, not insta snapshots (the rows carry the build's version) |
| T-O6-02 | `tests/update.rs::the_card_carries_the_update` (present, `/adage update` with `--for-session`, absent when equal, older, malformed version, a cache that is not JSON); `reads.test.ts` `updateOf` |
| T-O6-03 | `every_surface_…` (doctor `binary` pass, action, summary counts and exit code unchanged) |
| T-O6-04 | `autolearn::notice::tests::{a_newer_release_is_said_last_and_dropped_first, a_release_is_said_once_per_machine}` |
| T-O6-05 | `ui::tests::home_says_a_newer_release` (new snapshot `app-home-update`, version redacted; colour never the warning colour; `u`; palette; watch header); `app-home` unchanged |
| T-O6-06 | O10's (graph JSON, TopBar) |
| T-O6-07 | `every_surface_…`, `the_card_carries_the_update` (`version` line) |
| T-O9-01 | `adage-app` `person::tests::{the_press_sites_are_the_listed_commands, refusals_say_what_each_command_changes}` |
| T-O9-02 | `tests/update.rs::a_press_updates_a_fetch_channel_only`, `only_bare_update_takes_a_press`; `tests/press.rs::a_press_is_a_person_at_the_listed_sites` (the fourteenth site) |
| T-O9-03 | `tests/update.rs::dead_registrations_are_repointed_at_a_terminal_only` (agent branch refuses), `tests/press.rs::the_model_cannot_press`, `a_press_is_refused_everywhere_else` (`update --rollback`, `--yes`, `--from npm`) |
| T-O9-04 | `press.test.ts` (accepts `[cli,'update']`; refuses `--back`, `--rollback`, `--check`, `--yes`, `--from`); `register.test.ts` `T-X-04, T-O9-04 · authority, with presses` |
| T-O9-05 | `check-plugin.mjs`'s existing literal equality (now `review,delegate,on,add,update`) and its `install`… refusal |
| T-O9-06 | `view/band.test.ts` (table-driven order lapsed > problem > kept > review > update > version); `refresh.test.ts` (once per release per machine, gone after the first prompt) |
| T-O9-07 | `dialog.test.ts` (`T-O9-07 · /adage update …`), `commands.test.ts` (through the engine), `draw/pane.test.ts` (Health's version row and its press), `ask.test.ts`, `reads.test.ts` (`updatedOf`) |
| T-O9-08 | `commands.test.ts` (`T-O9-08 · update …`) |
| T-O9-09 | `dialog.test.ts` (no press held → kept; kept → asked again, run only on `run it`) |
| T-O9-10 | `autolearn::notice::tests::a_plugin_binary_says_the_slash_verbs` |
| T-X-04 | `register.test.ts` (`update` only inside `press()`), `press.test.ts` |
| T-X-13 | `tests/press.rs::the_model_cannot_press` (`update` without a press refuses, nothing kept) |
| T-O8-04 | `plugin/test/session-start.test.sh` §2b |

Snapshots: one new, `crates/adage-cli/src/ui/snapshots/adage__ui__tests__app-home-update.snap`;
none re-accepted; no golden touched.

## Departures, and why

- **`/adage update`'s press is `dialog.ts` `pressUpdate`, not `token.ts` `pressCommand`.**
  `pressCommand` toasts `not recorded: …` on a failure and words success from `doneToast`; the
  update's toasts come from the binary's last line and its failure names the terminal. `token.ts`
  belongs to another agent, so the token handling (tamper check, held/none, the next token, kept
  when none) is repeated in `pressUpdate` line for line; folding the two (a `toast` callback on
  `pressCommand`) is a small follow-up for `token.ts`'s owner.
- **The tail is not a wire file.** It is a verb's output under a press, not a hook answer; no
  `ModAnswer` variant or `tests/wire/*.json` was added (adage-hook's wire is another chunk's).
- **The band's harness-version line still says `run adage doctor`.** The plan (O9) asks for `/adage
  health`, but the words are pinned by ux-design §9.6/§10 (`band.version`), which this chunk may
  not edit; left for the copy deck's owner. The `nobinary` band keeps `run adage doctor` (no
  `/adage` command exists there).
- **The not-connected card shows the `update` row** but keeps `adage init` as its first `next`.
- **`adage watch`'s header** names the pair as a fact (the plan's contract lists it; the plan's
  file list does not).
- **The kit world does not answer `update` as a person-only verb** (`fixtures/kit.ts` is another
  agent's), so engine-level tests assert the argv, stdin and environment; the toast from each tail
  is tested over `fake`.
- **ux-design §9.13 and the §10 copy deck** (`band.update`, `ask.update`, `toast.updated`,
  `toast.update.failed`, `cmd.update.headless`) are not edited (docs other than these notes were
  out of scope); the strings above are the ones built.

## Live-only, not verified here

- A real `claude plugin update adage@adage` under a press from a live session (stubbed `claude`
  only); the toast and band in a real Claude Code 2.1.288 terminal and Desktop.
- npm and the release installer run for real under a press.

## Commands run and results

- `cargo fmt --all -- --check`: clean.
- `cargo clippy -p adage-cli -p adage-app -p adage-hook --all-targets -- -D warnings`: clean.
- `cargo test -p adage-cli -p adage-app -p adage-hook --no-fail-fast`: 38 suites, all green
  (adage-app 49, adage-cli unit 159, `update` 16, `press` 12, `mod_surfaces` 11 + 1 ignored,
  `hooks` 25, `zero_config_plugin` 12, adage-hook 62 + wire 2, the rest unchanged).
- `cargo test -p xtask`: 24 passed.
- Pinned CLI 2.1.288: `claude plugin validate --strict plugin/claude-code` passed;
  `claude plugin test plugin/claude-code` 383 pass, 0 fail; strict `tsc` over the hooks and tests
  clean.
- `node scripts/check-plugin.mjs`: `plugin adage 0.1.0: ok` (after VALIDATE.txt was regenerated;
  before, it named the `tool.call` hook another agent added to `register.ts` meanwhile).
- `bash plugin/test/session-start.test.sh`: `session-start ok` (the seven `superseded:` checks).
- `plugin/claude-code/VALIDATE.txt` regenerated last with the pinned CLI, the way `ci.yml` diffs
  it; its one changed line is the hook list, which now includes that `tool.call` hook (not this
  chunk's).
