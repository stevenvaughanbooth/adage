# Adage onboarding: install, update, and the app with every plugin

> Status: **proposed 2026-10-06**, for `docs/claude-mod/onboarding-plan.md`. Written against the
> branch `claude/mods-spike-doc-m98u3o` at `770171d` (the whole Claude Code mod implementation
> merged with `main`). Repository facts this plan rests on, checked in that tree: `git tag | wc -l`
> is 0; `origin` has no `marketplace` branch (`git ls-remote --heads origin`); the workspace
> version is 0.1.0 (`Cargo.toml:5-6`); the release workflow has never run on a tag
> (`CHANGELOG.md:741-759`); nothing on the mods branch has run in a live Claude Code session
> (`docs/claude-mod/impl/README.md:6-8`, `docs/claude-mod/README.md:7`). Every path and line below
> was read in that tree; a file that does not exist yet is marked **(new)**.
>
> The bar (the owner's words): *Adage should be SUPER easy to set up and use, so first-time users
> and returning users can easily use the app, get notified with updates, clearly update, and run
> the app with all of the rich plugins easily.* Read as five items: (1) install in one obvious step
> on macOS and Linux with no manual hook wiring; (2) the person is told when a new Adage exists, in
> the surfaces they already look at; (3) one obvious command or click updates the binaries, the
> plugin and the app together, safely; (4) "the app" (the mod's band, pane and dialog; `adage map`;
> `adage ui`; the terminal Home) runs with every plugin (Claude Code, Codex, OpenCode) with no extra
> steps; (5) a Claude Code user's first ten minutes need no terminal (ux-design §4.18).
>
> Where it comes from: three designs were scored by three judges. Two favoured the risk-first
> design ("Verified releases, one press to update"); the third favoured the MVP ("Ship what is
> built") and named the amendments that would close it. This plan starts from the risk-first
> design's skeleton (a release check off every hook path; `adage update` as a transaction with a
> way back; an available update as information, never a problem; one exact-argv press site for
> `update`; `init --yes --hooks`; the version-skew notice; the additive wire rule) and grafts what
> the judges named from the others: the live check *first* and the `/plugin` spelling quoted from
> its notes (MVP U0); Homebrew taken out of the first release so a missing token cannot fail it
> (MVP U2); the npm-registry-first update check (MVP U4); `install::repoint` lifted from the loop
> `delegate on` already runs (MVP U5); `cargo xtask bump` (all three); the `release` channel kept
> apart from `source` so a curl install never runs `cargo install` (user-first U1); the Binaries
> step in `adage init` that fetches `adage-exec` and `adage-train` from the matching release
> (user-first F1); the plugin channel updated through Claude Code's own `claude plugin update`,
> run by the binary, with the `/plugin` steps as the fallback (user-first U2). It drops what the
> judges found: no release-signing key and no hand patch to `release.yml` (risk-first R1); no
> second binary store under `$DATA/bin/staged` (risk-first R5); no `install` press (user-first P1,
> an owner decision kept apart in §5); no first release gated on new infrastructure; and it
> corrects the one claim every judge caught, that plugin-written Codex/OpenCode registrations pin
> a stable path (they pin the digest-named copy: `crates/adage-cli/src/install.rs:76-78`
> canonicalises `current_exe()`, and the shim links `bin/adage -> adage-<sha>`,
> `plugin/claude-code/scripts/session-start:89-94`).

## 0. How to work this plan

Every chunk is built on `claude/mods-spike-doc-m98u3o` (or `main` once that branch lands) and
follows the implementation plan's rules (`docs/claude-mod/implementation-plan.md:16-66`), repeated
here where they bind this work:

- **One chunk, one branch, one pull request.** A chunk is done when its acceptance criteria pass
  in CI and the documents it names are updated in the same change (`docs/how-it-works.md:6`:
  "Implementation changes must update this page and the relevant operational guide in the same
  change"). User-visible changes go under `## 0.1.0 - Unreleased` (after O3, `## 0.1.1 -
  Unreleased`) in `CHANGELOG.md`, in a new thematic subsection `### Installing and updating`
  beside `### Seeing it`. Commit with `git commit -s`.
- **The floor never moves.** `adage hook claude <Event>` stays byte-identical for every existing
  golden (`crates/adage-cli/tests/golden/*.txt`, six files) and the exec parity test
  (`crates/adage-exec/tests/parity.rs`). The only new hook output in this plan is one
  `SessionStart` notice for a condition no fixture has (an index written by a newer binary, O8).
- **Fail open, always; no hook touches the network.** `evals/launch/fresh-install.md:39` ("Hooks
  make no network calls") stays true. The one network site this plan adds runs in the detached
  learner, after a learn, at most daily, behind `update.check = daily | off`, and a counting
  fixture server proves every hook event makes zero requests (T-O5-05). `docs/zero-config.md:78`
  is amended to say exactly that.
- **The binary decides; the mod bridges and draws.** The mod never compares versions, never
  chooses a channel and never computes an update line: it draws the status card's `update` object
  verbatim (`plugin/claude-code/hooks/reads.ts:9-23` grows one field). The band's `newer()`
  (`plugin/claude-code/hooks/view/band.ts:123-132`) stays for the harness-version line only.
- **A person's decision is a person's press or a person's terminal** (architecture §6 invariant
  3a, `docs/claude-mod/architecture.md:1079-1085`). This plan adds **one** press site, `update`
  (`PersonVerb::Update`, the fourteenth; `crates/adage-app/src/person.rs:175` goes from 13 to 14),
  with the exact argv `[cli, 'update']` and no arguments, reached only from a dialog answer or a
  focused `Button`. Under a press `update` replaces Adage's own binaries through its channel and
  edits no settings file, no registration and no config. `install`, `config`, `share`, `train`,
  `packs` and `init` stay on the never-list (`scripts/check-plugin.mjs:135-137`,
  `plugin/claude-code/hooks/press.ts:23`); connecting Codex or OpenCode from the pane is an owner
  decision this plan does not make (§5).
- **An available update is information, never a problem.** Doctor stays Pass/Warn/Fail
  (`crates/adage-cli/src/doctor/mod.rs:42-48`); the `binary` check stays Pass with the newer version
  in its detail and `Fix::run("adage update")` as its action; the band's lamp never turns amber for
  it; no dialog opens unasked. Two *installed* Adage versions that disagree are a problem (Warn,
  O8).
- **Words.** `docs/lexicon.md` and `docs/design/style.md:91-104` (header, key column, `done`,
  `next`). Reflexes, instincts, condition, the map; never a strength or lifecycle word in an ask.
  Every `next` names one command or one `/adage` verb. No default surface tells a plugin-only
  person to open a terminal for something a `/adage` verb or a press does.
- **Tests.** Integration tests build their own `World` (`crates/adage-cli/tests/hooks.rs:35,
  129-157`; `zero_config_plugin.rs:1-8` with `ADAGE_TEST_NO_SPAWN=record`); a test that runs a
  person-only verb sets `ADAGE_ALLOW_NONINTERACTIVE=1` and removes `AGENT_ENV`. Test ids are
  `T-O<n>-<nn>`; cross-cutting ids from `docs/claude-mod/test-plan.md:548-612` (T-X-04 authority,
  T-X-12 parity, T-X-13 the model cannot press) and T-U-01 (`test-plan.md:794-798`) are reused by
  name. The layering rules (`xtask/src/layers.rs:17-62`) bind every chunk: the release cache type
  lives in `adage-store` (layer 0) so the hook (layer 6) may read it; the check itself lives in
  `adage-cli`; nothing here adds an exception.
- **Order.** O0 (the live check) runs first and in parallel with O1–O2; O3 (the first tag) waits
  for O0, O1 and O2 and nothing else. Every string that names a Claude Code command (`/plugin
  install adage@adage`, `claude plugin update adage@adage`) is quoted from
  `docs/claude-mod/impl/O0-live-notes.md` **(new)** once O0 has recorded it.

## 1. The stories, as the person experiences them

Strings in `code` are the exact command, click or dialog text. Where today's code already prints
the string, its line is given; where this plan introduces it, the chunk is.

### 1.1 First time, Claude Code user, macOS or Linux (no terminal)

1. In Claude Code: `/plugin marketplace add stevenvaughanbooth/adage`, then
   `/plugin install adage@adage`. The marketplace lists exactly one plugin, **adage** (the retired
   `adage-exec` alias at `.claude-plugin/marketplace.json:24-36` is gone, O1). Claude Code fetches
   the tip of the `marketplace` branch, written by the v0.1.0 release (O3):
   `plugin/claude-code` with `dist/<target>/{adage,adage-exec,adage-train}` for the five targets
   and `dist/SHA256SUMS` (`.github/workflows/publish-marketplace.yml:45-53`). No terminal, no Rust,
   nothing downloaded by Adage (`plugin/claude-code/scripts/session-start:19`).
2. `claude` in their repository. The shim verifies the bundled binaries against `SHA256SUMS`,
   links `bin/adage`, `bin/adage-exec`, `bin/adage-train` (O1) and `bin/hook`
   (`session-start:76-97, 99-131, 133-152`) and hands over `SessionStart`. One line appears, which
   the model never sees:
   `adage: learning from 23 past sessions in this repo; reflexes arrive within a minute. Nothing runs without you.`
   (`crates/adage-hook/src/session_start.rs:161-163`). The detached bootstrap learner starts
   (`crates/adage-cli/src/autolearn.rs:7-8`).
3. The band appears above the prompt: `◠ adage  ● healthy · learning…`
   (`plugin/claude-code/hooks/view/band.ts:62-100`), then `learned`.
4. First prompt: a reflex fires and its line is drawn under the prompt.
5. End of the first or second turn, in the engine's own dialog:
   `Adage learned: run make test after editing acme/**. Tell the agent?` with
   `enable` / `not this one` / `why` (`plugin/claude-code/hooks/ask.ts:52-53, 23-28`). The answer
   is a press (`dialog.ts:65-83`).
6. `/adage` prints the card and opens the pane (Live, Reflexes, Health); `Explore in browser`
   opens the map (`plugin/claude-code/hooks/actions.ts:37-65`).
7. If Codex or OpenCode is also installed, the card's row says `codex  found · adage init
   connects it` (O10) and the pane's Health tab shows the same with `Copy command`. That is the one
   terminal step left for those two harnesses, said once (§5: Connect-from-the-pane is the owner's
   call).

### 1.2 First time, terminal user, macOS or Linux (one step)

Two one-liners, whichever the person prefers; both end in `adage init`.

1. Without npm (needs no secret to publish, O3):
   `curl --proto '=https' --tlsv1.2 -LsSf https://github.com/stevenvaughanbooth/adage/releases/latest/download/adage-cli-installer.sh | sh && adage init`
   (the URL `docs/usage.md:165` already names; dist puts `adage` in `~/.cargo/bin`,
   `dist-workspace.toml:31`).
   With npm (once `NPM_TOKEN` and `ADAGE_NPM_PUBLISH=true` exist, §5):
   `npm install -g @adage-sh/cli && adage init` (all three binaries in the platform package,
   `npm/config.mjs:15-18`; nothing downloaded at install time, `npm/cli/bin/adage.js:1-6`).
2. `adage init` at a terminal is the wizard (`crates/adage-cli/src/ui/wizard.rs:1-26`). Its steps,
   after O4: **Harnesses** (read-only); **Binaries** (new, before the hooks so the plugin is staged
   with all three): `adage-exec and adage-train are not beside adage` with `Fetch them · each
   verified against the release's checksums` (skipped when both are there, as they are after npm;
   a checkout build is pointed at `cargo build`); **Hooks**, per harness found. When Claude Code is
   found and the plugin is not installed, the first choice is automatic (coordinator amendment,
   proven 2026-10-06 on Claude Code 2.1.292): `The Adage plugin (recommended) · adage installs it
   with claude plugin` stages the plugin from this binary as a local marketplace under
   `$ADAGE_HOME/plugin/` and runs `claude plugin marketplace add <that path>` and `claude plugin
   install adage@adage-local`, checked with `claude plugin list`; without `claude` on `PATH` the
   first choice names the typed spellings instead (`in Claude Code: /plugin marketplace add
   stevenvaughanbooth/adage, then /plugin install adage@adage`); the second is the settings file,
   the exact file and diff first (`~/.claude/settings.json would be created:` + the lines,
   `crates/adage-cli/src/install.rs:466-479`); a settings-file registration beside the plugin is
   offered for removal (`install --remove`'s diff, `.adage-bak` kept). Then the embedding model
   (bundled, usually nothing); repositories; a first `adage condition`; auto-condition; the doctor
   summary; and at most two `next` lines: `claude · start a session; Adage says what it is doing on
   its first line` and the harness's own remaining step (Codex: `open /hooks in Codex to approve
   the new hooks`), with `Later: adage update …; adage doctor if anything looks wrong.` under them.
3. Scripted: `adage init --yes --hooks user` does the same with no questions (a terminal or
   `ADAGE_ALLOW_NONINTERACTIVE=1`; refused inside an agent session as every write is,
   `crates/adage-cli/src/init.rs:50-58`), each file diffed in the output and backed up as
   `<file>.adage-bak` (`install.rs:27-31`).
4. `claude` (or `codex`, `opencode`). The first session says, once:
   `adage: 2 reflexes and 14 instincts active in this repo; it keeps learning as you work. Nothing runs without you.`
   (`session_start.rs:170-187`).

### 1.3 A new version is out: told, then updated

**Told.** A maintainer tags `v0.2.0`; `release.yml` builds, attests and publishes (§5). On the
person's machine nothing happens on any hook. After the next learn, the detached learner runs the
daily check (O5): one `curl` of `https://registry.npmjs.org/@adage-sh/cli/latest` (field
`version`), and only when that answers 404 (the package is not published) one of
`https://api.github.com/repos/stevenvaughanbooth/adage/releases/latest` (field `tag_name`, `v`
stripped); 3 s timeout; nothing sent but the request; the result in `$ADAGE_HOME/cache/latest.json`.
Then, in the surfaces they already look at (O6, O9):

1. Claude Code, the next session start, in the engine's UI and never to the model:
   `adage: 0.2.0 is out (you have 0.1.0); /adage update brings it.` (the notice's fifth and lowest
   kind, `update`; `crates/adage-cli/src/autolearn.rs:351-430`, ridden by the existing daily
   throttle `crates/adage-hook/src/autolearn.rs:93-110`). For a settings-file install the same line
   ends `run adage update.`
2. The band's second line, lowest tier, until the first prompt, once per new version per machine:
   `» adage 0.2.0 is out · /adage update` (`band.ts:102-113` gains the tier after `review`, before
   the harness-version line; `draw/sites.ts:131`'s cadence).
3. `/adage` (the card) shows the row `update      0.2.0 is out (you have 0.1.0) → /adage update`;
   in a terminal bare `adage` shows `→ adage update` and `next` begins with `adage update`
   (`crates/adage-cli/src/status_card.rs:565-585, 688-705`).
4. `adage version` prints `adage 0.1.0 · 0.2.0 is out (adage update)` (`crates/adage-cli/src/main.rs:267-270`).
5. `adage doctor`: `✓ binary   adage 0.1.0 at ~/.cargo/bin/adage · 0.2.0 is out` stays a pass, with
   `next adage update`; the lamp stays `healthy` (`doctor/mod.rs:211-215`).
6. `adage ui` Home's first row and the palette's `Update Adage`; the map's TopBar text
   `adage 0.1.0 · 0.2.0 is out`.

**Updated from Claude Code (plugin install).** The person types `/adage update` (or presses
`u: update` on the Health tab). The dialog asks
`Adage 0.2.0 is out (you have 0.1.0). Update now? It takes effect in your next session.` with
`update` / `not now` / `what changes` (O9). `what changes` opens Health on a `version` row with the
release notes' first lines and `Copy: adage update`. `update` is a press: the mod runs
`<data>/bin/adage update` with `{ press, session }` on stdin; the binary spends the token, prints
`{"next": …}`, finds its channel is `plugin` and runs Claude Code's own
`claude plugin update adage@adage` (the spelling O0 recorded) when `claude` is on `PATH`; its last
stdout line is `{"updated": {"from": "0.1.0", "to": "0.2.0", "effect": "next session"}}`. The toast:
`adage updated 0.1.0 → 0.2.0 · takes effect in your next session`. The running session keeps the
digest-named copy it started with (`session-start:84-95`), so nothing moves under it; the next
session's shim verifies and links 0.2.0, with `adage-exec` and `adage-train` beside it. When
`claude` is not on `PATH`, the binary answers `manual` and the pane shows
`In Claude Code: /plugin → adage → Update`, still no terminal. When Claude Code's own marketplace
auto-update has already refetched the plugin, the check finds `have == latest` after the next
start and nothing is asked.

**Updated from a terminal (npm, release, source, checkout).** `adage update` (O7) prints:

```text
adage update · 0.1.0 → 0.2.0 · npm

keeps    ~/.local/share/adage/previous/0.1.0/ (adage, adage-exec, adage-train)
runs     npm install --global @adage-sh/cli@0.2.0
…
version  adage 0.2.0 ✓
hooks    claude-code ~/.claude/settings.json runs this binary
         codex ~/.codex/hooks.json runs this binary
doctor   ● healthy
changed  · fixes hook latency on large repos · …

done     adage 0.2.0 · takes effect in new sessions
```

Already current: `adage 0.2.0 is the latest · nothing to do` (exit 0, nothing run). Anything
wrong after the package manager ran: `adage update --back` restores the kept binaries and
re-registers. A `release` (curl) install runs the installer line again with the version pinned and
refetches the two companions; a `source` install without `cargo` on `PATH` is offered the installer
line instead of `install Rust` (today: `update.rs:217`).

### 1.4 Running the app with every plugin

"The app" is one state drawn from five places, every one fed by `adage … --json` from the same
binary (architecture §2 rule 1):

1. **In Claude Code**: the band; the dialog at turn end; `/adage` (`status`, `watch`, `reflexes`,
   `health`, `doctor`, `review`, `why`, `off`, `on`, `pause`, `map`, `savings`,
   `plugin/claude-code/hooks/commands.ts:18`, plus `update`, O9); the pane; `Explore in browser`.
   Every binary from the one plugin: `adage` decides, `adage-exec` acts once `delegate on` is
   pressed (`session-start:133-144`), `adage-train` compiles skills (O1;
   `crates/adage-cli/src/train.rs:27-34` finds it beside the running `adage`, which under the plugin
   is `<data>/bin/`).
2. **The map** (`adage map`, read-only by design, `crates/adage-cli/src/map/mod.rs:10-14`): the
   TopBar shows `adage 0.1.0` and, when due, `0.2.0 is out · adage update` as text; the Setup view
   reads the channel from the graph JSON and says `In Claude Code: /adage review, /adage health;
   the pane's Explore in browser opened this page` for a plugin user and `In a terminal: adage ui,
   adage review, adage update` otherwise, never `start adage ui once in your terminal` for a plugin
   user (`web/map/src/views/Setup.tsx:61-66`, O10).
3. **`adage ui`**: opens on Home as today (`crates/adage-cli/src/ui/app.rs:213-215`) with a first
   row `adage 0.1.0 · 0.2.0 is out · u update` running `adage update` as a Job with its lines
   (`ui/jobs.rs:97-105`); the palette lists `Update Adage` (`ui/palette.rs:84-120`).
4. **Bare `adage`**: the card, `next` at most two lines (`adage review` when something waits, else
   `adage ui`; `adage update` first when a version is out).
5. **Codex and OpenCode**: wired once by `adage init` (user scope, the diff shown); they share the
   index, learner, reflexes and updates. Registrations written by the plugin's binary name the
   plugin's stable link `<data>/bin/adage` (O4), so a plugin update carries them; registrations
   written by a PATH binary are re-pointed by `adage update` when the path moved (O7). Their
   notices live in the card, `adage watch`'s header, TUI Home and `adage doctor`
   (`docs/zero-config.md:158-160`; Codex has no person channel,
   `crates/adage-hook/src/harness/codex.rs:78-86`); the fix named everywhere is `adage update`.

### 1.5 The no-terminal first ten minutes (the checklist O11 runs)

| t | What happens | Where it is built |
| --- | --- | --- |
| 0:00 | `/plugin marketplace add stevenvaughanbooth/adage`, `/plugin install adage@adage`; one plugin listed | O1, O3 |
| 0:30 | `claude`; the shim links three binaries; `adage: learning from N past sessions …` | O1; today |
| 0:35 | band `◠ adage  ● healthy · learning…`, then `learned` | today |
| 1:00 | first prompt, a reflex line under it | today |
| 2:00 | the turn-end ask `Adage learned: … Tell the agent?` → `enable` (a press) | today |
| 3:00 | `/adage` → card and pane; `Explore in browser` → the map, whose Setup names `/adage` | O10 |
| 5:00 | a Codex user sees `codex  found · adage init connects it` once | O10 |
| day 2 | `adage: 0.2.0 is out … /adage update brings it.`; `» adage 0.2.0 is out · /adage update` | O5, O6, O9 |
| day 2 | `/adage update` → the ask → `update` (a press) → `adage updated 0.1.0 → 0.2.0 · takes effect in your next session` | O9 |

Nothing in this journey is a command, a copy or a terminal (ux-design §4.18,
`docs/claude-mod/ux-design.md:425-433`).

## 2. What changes where

### 2.1 `adage-store` (layer 0)

- `Paths::update_cache()` → `$ADAGE_HOME/cache/latest.json`, beside `cache()`
  (`crates/adage-store/src/paths.rs:95-98`); `Paths::previous()` → `$ADAGE_HOME/previous/`.
- `config.rs`: `update_check: bool` on `Config` beside `ui_notices` (`:78-79`) and the key
  `update.check` (`daily` | `off`, default `daily`) beside `ui.notices` (`:288-295`); the environment
  override `ADAGE_UPDATE_CHECK=0`.
- `crates/adage-store/src/release.rs` **(new)**: `Latest { checked_at, have, latest: Option<{
  version, published_at, notes, url }>, failure: Option<{ class, host, at }> }`, read by the card and
  the hook's notice composer; written only by `adage update --check` and the learner.

### 2.2 `adage-app` (layer 8)

- `person.rs:153-171, 175-189, 193-204`: `PersonVerb::Update`, `ALL` becomes 14, `cmd() = "update"`,
  `what_it_changes` → `it replaces Adage's own binaries on this machine`.
- `words.rs`: the two install lines and the update line as constants, so README, the card and the
  wizard print one string (T-O12-02).

### 2.3 `adage-hook` (layer 6)

- `ctx.rs:72-78`: `Unloaded::Newer(format)` is kept (today folded into `unwrap_or_default()`;
  `crates/adage-tracks/src/index.rs:360-362, 383-388` already distinguish it).
- `session_start.rs:115-147`: one `systemMessage` for a newer-format index, once per data
  directory per day (the `unwritable_notice` marker pattern, `:189-201`); every other event prints
  nothing, as now.
- `autolearn.rs:74`: the `kind` doc gains `update` (lowest priority).
- `tests/wire/*.json`: the additive rule written into `docs/claude-mod/impl/wire.md` (fields are
  only ever added, so a 0.1.0 mod draws a 0.2.0 binary).

### 2.4 `adage-cli` (layer 9)

- `net.rs` **(new)**: the curl runner lifted from `models.rs:330-367` with `classify` (`:281-291`)
  and `explain`, shared by `models fetch`, `update --check` and the companion fetch;
  `ADAGE_RELEASE_URL` (the base URL for tests and mirrors) and `ADAGE_OFFLINE=1` (no fetch at all).
- `update.rs` (whole file): `Channel::Release`; detection by `cache|data` path segments for the
  plugin instead of the literal `.claude`/`plugins` pair (`:57-70`); `$ADAGE_HOME/channel.json` and
  dist's receipt for `release`; `--check [--now]`; `latest`/`cached`/`newer`; the transaction (keep,
  run with the version pinned, verify, repoint, doctor, `--back`); the press entry
  (`review::person_or_press(PersonVerb::Update)`, `review.rs:551-556`); `--json` progress lines;
  the plugin channel's `claude plugin update adage@adage` (a constant, quoted from O0's notes).
- `install.rs`: `hook_binary()` (`:72-88`) keeps the plugin's stable link `<data>/bin/adage` when
  `current_exe()` is a digest-named copy beside it (the same test `detect::adage_command` makes,
  `detect.rs:49-57`); `pub(crate) fn repoint(bin) -> Vec<(harness, path)>` lifted from the loop at
  `exec.rs:272-281` and called by both `delegate on` and `update`.
- `init.rs:15-21, 35-58, 77-80`: `--hooks user|project|none`; the Binaries step; the plugin route
  first, automatic: `claude_plugin.rs` **(new)** stages the plugin (module files embedded at build
  time by `crates/adage-cli/build.rs` **(new)**) as the local marketplace `adage-local` under
  `$ADAGE_HOME/plugin/` and installs it through `claude plugin`, behind the `ClaudeCli` trait.
- `ui/wizard.rs:18-26, 848-860`: the same steps in the wizard; two `next` lines.
- `autolearn.rs:287, 351-430, 371-373`: `update::check_if_due(paths)` after `notice::update`; the
  fifth kind; `/adage review` for a plugin binary.
- `status_card.rs:565-585, 596-626, 688-705`: the `update` row and JSON object; the not-connected
  rows name `adage init` and the two slash commands only; `next` trimmed.
- `doctor/mod.rs:211-215, 356-374, 667-709, 806-828`: `binary` detail and `Fix::run("adage
  update")`; the `index` newer fix; a `versions` check; the plugin check compares versions before
  digests and names user fixes.
- `detect.rs:150-160, 244-320`: a registration carries the binary's `version` when executable.
- `main.rs:267-270`: `adage version` with the update pair.
- `cli.rs:933-948`: `update` listed under "When something is off" beside `doctor`, described as
  `update Adage, its hooks and the plugin together`.
- `ui/tabs/home.rs:16-29`, `ui/palette.rs:84-120`, `ui/jobs.rs:97-105`, `ui/health.rs:18-30`
  (`Fix::Update`), `ui/keys.rs:28`, `ui/view.rs:296, 993-1002` (`m` opens the browser as `adage
  watch` does).
- `graph.rs` and `crates/adage-app/src/graph.rs:165-166`: the graph JSON gains `adage: { version,
  latest, channel }`; `map/mod.rs:29` keeps 8765 and the demo dashboard moves (`demo/dashboard/serve.py:17`).
- `train.rs:27-34`: unchanged in logic; a test pins that it finds `adage-train` in the plugin's
  `bin/`.
- Tests: `tests/update.rs` **(new)** with a local fixture server (the pattern of `tests/serve.rs`);
  `tests/first_run.rs:163-180` rewritten for `--hooks`; `tests/zero_config_plugin.rs:152-181`
  stages `adage-train`, `:970-979` the new notice words; `tests/hooks.rs` with the counting server.

### 2.5 The plugin (`plugin/claude-code`)

- `scripts/session-start`: a third `bundled adage-train` block after `:118-131`; the fallback text
  at `:113-116` names channels that exist; superseded `bin/<name>-<sha>` copies no link points at
  are removed after relinking (`:82-95`).
- `hooks/reads.ts:9-23`: `Card.update?: { have, latest, text, command, notes }`.
- `hooks/view/band.ts:11-35, 102-113`: `update: string` in `BandInput`; the tier after `review`,
  before `versionLine`.
- `hooks/draw/sites.ts:131`: passes the update text at the version line's cadence.
- `hooks/plugin.ts:155`, `hooks/register.ts`, `types/index.d.ts:159-175`: `STORE_KEYS` gains
  `latest` (once per new version per machine); `AdageMode` gains `update: string`.
- `hooks/press.ts:25, 44-63`: `PRESS_VERBS` gains `update`; `isPressAllowed` accepts exactly
  `[cli, 'update']`.
- `hooks/ask.ts:30-32`: `UPDATE_QUESTION`, `UPDATE_ANSWERS`; `hooks/dialog.ts:85-95`: `askUpdate`
  beside `askOn`, the `update` answer a press.
- `hooks/commands.ts:18, 49-52, 72, 102-107, 112`: the `update` verb opens the ask; headless text;
  `run adage doctor in a terminal` → `/adage health` where the mod is live.
- `hooks/draw/pane.tsx:303-332`: the Health `version` row with `u: update` and `Copy: adage update`.
- `hooks/refresh.ts:21-48`: keeps the card's `update` in `mode.update`; `notify.ts:12-16` unchanged
  (once per session), the once-per-machine toast keyed by the store.
- `README.md:1-8`: an end-user "Installing and updating" section.
- `plugin/claude-code-exec/` (the retired alias): **deleted**; `plugin/build.sh:9, 14-15, 17-18`
  builds and stages `adage-train`, drops the alias lines.
- `plugin/opencode/adage.js`, Codex `hooks.json`: no change; they follow the stable path O4 writes.

### 2.6 Release tooling, workflows, scripts

- `dist-workspace.toml:18, 23`: `installers = ["shell", "powershell"]`, `publish-jobs =
  ["./publish-npm", "./publish-marketplace"]`; `dist generate`; the SBOM patch re-applied (`:5-8`).
- `.github/workflows/release.yml:308-350`: the Homebrew job disappears with the regeneration.
- `scripts/stage-plugin.mjs:88-97, 122-125`: `adage-train` required for every target; the alias
  entry gone. `scripts/check-plugin.mjs:48-84, 134-137, 186-209, 334-345, 395-417`: one plugin;
  the press list literal `review,delegate,on,add,update`; three bins in `--staged`; `STORE_KEYS`
  table. `scripts/test-stage-plugin.mjs:20`: three apps.
- `xtask/src/main.rs:59-69` and `xtask/src/release.rs` **(new)**: `cargo xtask bump <version>`,
  `cargo xtask release-check`.
- `.github/workflows/ci.yml:139-140, 174-178, 266-280, 308-314`: `release-check` in the plugin
  job; the alias validate line dropped; `VALIDATE.txt` regenerated; map drift.

### 2.7 The web map (`web/map`)

- `src/api/types.ts:351-369`: `adage?: { version: string; latest?: string; channel?: string }`.
- `src/components/TopBar.tsx:93-172`: the version text beside the wordmark; `0.2.0 is out · adage update`.
- `src/views/Setup.tsx:5-30, 43-66`, `components/ui.tsx:181-201`, `views/Health.tsx:146-150`,
  `components/Inspector.tsx:595-607`, `components/Palette.tsx:184`: copy by channel; `README.md` **(new)**.

### 2.8 Documents

`README.md:17-23, 62-122`; `docs/usage.md:150-217, 327-345`; `docs/zero-config.md:11-25, 29, 42,
50-60, 76-82, 158-170, 285-300, 309-335`; `docs/harnesses.md:123-140, 221-226, 262-272`;
`docs/execution.md:619` ("Deciding from the session" gains `update`); `docs/live-learning.md`
(what reaches the network, when, and the off switch); `docs/claude-mod/architecture.md:663-665,
849-854`; `docs/claude-mod/ux-design.md:435-454, 580-597, 711-719`;
`docs/claude-mod/impl/A7-A8-notes.md:77-92`; `docs/claude-mod/implementation-plan.md:321-326` (the
planned major-version warn, now built as O8, cross-referenced); `docs/claude-mod/evaluation-plan.md`;
`docs/claude-mod/test-plan.md`; `evals/launch/fresh-install.md:30-56`; `npm/cli/README.md:5-18`;
`CONTRIBUTING.md:105-123`; `CHANGELOG.md`; `SECURITY.md` (what a release's checksums and
attestations promise).

## 3. The chunks

Each chunk: what it delivers, the files, the contract, the tests, the acceptance criteria, an
estimate in engineer-days, and what it depends on. Each is one pull request.

#### O0 · The live check of the staged plugin, and the `/plugin` spellings

- **Delivers.** The first live Claude Code session with the plugin `plugin/build.sh` builds and
  `scripts/stage-plugin.mjs --bins` stages, on one macOS (arm64) and one Linux (x86_64) machine
  with a Claude account: the shim links and the opening line; the band; the press window and the
  turn-end ask; `/adage` and the pane; `Explore in browser`; the Desktop app's `$.process`
  (ux-design Q7). And the exact spelling and behaviour of Claude Code's plugin update commands at
  the pinned version (`.github/workflows/ci.yml:267`, 2.1.288) and the latest: `/plugin update`,
  `claude plugin update adage@adage` from a shell and as a child of the running engine, whether a
  marketplace refresh must precede it, and what "restart required" means for a running session.
  Blockers are fixed in the same chunk; findings are recorded once so every later chunk quotes
  them.
- **Files.** `docs/claude-mod/impl/O0-live-notes.md` **(new)**: what ran, what broke, the verified
  commands; `docs/claude-mod/evaluation-plan.md:100-116` (§2.1 run); `docs/claude-mod/impl/README.md:6-8`
  (the status line); `plugin/build.sh` (run as is); `scripts/stage-plugin.mjs:9-13`
  (`--bins <dir> --allow-missing` for the host triple); `plugin/claude-code/scripts/session-start:46-60`
  (Linux stages the gnu triple; the fallback at `:60` is exercised); whatever the live run breaks
  under `plugin/claude-code/hooks/`.
- **Contract.** Every item of the evaluation plan's §2.1 and §2.4 smoke list is marked ran / not
  run with the Claude Code version and machine; the plugin update command is written once, in the
  notes, and later chunks' strings quote it. No golden changes.
- **Tests.** T-O0-01 the shim links `bin/adage`, `bin/adage-exec`, `bin/hook` on a real install;
  T-O0-02 the opening `systemMessage` is shown and absent from the model's turns; T-O0-03 a press
  issued at `classic.SessionStart` is spent by a dialog answer (`via: press` in the review record);
  T-O0-04 `/adage` draws the card in the terminal and the Desktop app; T-O0-05 `Explore in browser`
  opens the map from the pane; T-O0-06 `claude plugin update <plugin>` (the verified spelling)
  replaces the plugin while a session runs, the running session's digest-named copy is untouched,
  and the next session's shim links the new digest; T-X-13's L9 observation (Q14).
- **Acceptance.** The notes file exists with a date, machine, Claude Code version and outcome per
  line; a failed line opens a chunk, not a footnote; `claude plugin test plugin/claude-code` green.
- **Estimate.** 2.5 days. **Depends on.** Nothing (needs two machines with a Claude account).

#### O1 · One plugin, three binaries, no retired alias

- **Delivers.** The marketplace lists one plugin; the plugin bundles `adage`, `adage-exec` and
  `adage-train` for all five targets, so a plugin-only person can run `adage train` (G13); the
  retired `adage-exec` alias is deleted (nothing was ever published, so there is nothing to quiet;
  G20); the shim's fallback lines name channels that will exist (G21).
- **Files.** `scripts/stage-plugin.mjs:88-97` (add `{ name: "adage-train", app: "adage-train",
  required: () => true }`; drop the alias entry at `:96` and `:122-125`); `plugin/build.sh:9, 14-15,
  17-18`; `plugin/claude-code/scripts/session-start:113-116` (fallback: `no binary for $OSARCH;
  reinstall with /plugin install adage@adage, or npm i -g @adage-sh/cli`), a third `bundled
  adage-train` block after `:118-131` placing `bin/adage-train`, falling back to `PATH`, removing
  the link when neither; `.claude-plugin/marketplace.json:24-36` (remove), `:19` (`three binaries,
  five targets, verified; nothing downloaded`); `plugin/claude-code-exec/` (delete);
  `scripts/check-plugin.mjs:48-84, 186-209, 395-417` (one plugin; three bins); `scripts/test-stage-plugin.mjs:20`;
  `.github/workflows/ci.yml:271, 278`; `crates/adage-cli/tests/zero_config_plugin.rs:152-181`
  (`staged_plugin` stages `adage-train` when built beside); `crates/adage-cli/src/detect.rs:305-320`
  (`plugin-data` reports `bin/adage-train` too); `plugin/claude-code/README.md:12-14`;
  `docs/claude-mod/implementation-plan.md` (A8's Built line); `CHANGELOG.md`.
- **Contract.** `stage-plugin.mjs` fails unless `adage`, `adage-exec` (non-Windows) and
  `adage-train` are present for every target in `npm/config.mjs` `PLATFORMS`, and writes one
  `SHA256SUMS` covering all three. The shim exits 0 on every path; a missing or mismatched
  `adage-train` only removes `bin/adage-train` (adage keeps serving, as adage-exec's `:131` does).
  `train.rs::locate` finds `<data>/bin/adage-train` because the running `adage` is
  `<data>/bin/adage-<sha>` in the same directory.
- **Tests.** T-O1-01 stage-plugin with `--bins` lacking `adage-train` fails naming the target;
  T-O1-02 `plugin/test/session-start.test.sh` covers the third binary present, absent and tampered;
  T-O1-03 `adage train --help` from a staged plugin's `bin/adage` execs `bin/adage-train`
  (`zero_config_plugin.rs`, unix); T-O1-04 check-plugin passes with one plugin and fails if
  `marketplace.json` lists a second or the description says `on PATH`; T-O1-05 `claude plugin
  validate --strict .` passes with one entry and `VALIDATE.txt` is regenerated if a hook moved.
- **Acceptance.** `node scripts/check-plugin.mjs`, `node scripts/test-stage-plugin.mjs`, the shim
  test and `claude plugin test` green; `release_plugin_startup` budgets hold with three binaries
  staged.
- **Estimate.** 1 day. **Depends on.** Nothing.

#### O2 · Release tooling: one bump, one check, Homebrew off the first release

- **Delivers.** `cargo xtask bump <version>` writes the version into `Cargo.toml`
  `[workspace.package]`, `plugin/claude-code/.claude-plugin/plugin.json` and
  `.claude-plugin/marketplace.json` (metadata and the plugin), moves CHANGELOG's Unreleased under
  `## <version> - <date>`, and runs `check-plugin.mjs`; `cargo xtask release-check` fails when the
  versions disagree or `dist generate` would change `release.yml` beyond the documented SBOM patch.
  Homebrew leaves `installers` and `publish-jobs` so the tag cannot fail on a missing
  `HOMEBREW_TAP_TOKEN` (`release.yml:308-350` runs on every non-prerelease tag today; only npm is
  gated, `publish-npm.yml:27`).
- **Files.** `xtask/src/main.rs:59-69` (dispatch), `xtask/src/release.rs` **(new)**;
  `dist-workspace.toml:18, 23` and the regenerated `.github/workflows/release.yml` with the SBOM
  patch re-applied (`dist-workspace.toml:5-8`); `.github/workflows/ci.yml` (the plugin job runs
  `cargo xtask release-check`); `CONTRIBUTING.md:105-123` (three steps become `cargo xtask bump`,
  `git tag`, `git push`); `CHANGELOG.md:1-12` (the `### Installing and updating` subsection).
- **Contract.** `cargo xtask bump 0.1.1` on a scratch tree changes exactly `Cargo.toml`,
  `Cargo.lock`, the two JSON files and `CHANGELOG.md`; `release-check` is the only guard on the
  three versions (`check-plugin.mjs:41-45, 55, 65` keep their equality checks); a PR still plans
  the release (`pr-run-mode = "plan"`, `dist-workspace.toml:38`).
- **Tests.** T-O2-01 `bump` round-trips (bump, check, bump back) touching the named files only;
  T-O2-02 `release-check` fails on a drifted `plugin.json` version; T-O2-03 `release.yml` on a PR
  plans with no Homebrew job; T-O2-04 `publish-marketplace.yml`'s local smoke
  (`scripts/test-stage-plugin.mjs`) still passes.
- **Acceptance.** CI green; `grep -c homebrew .github/workflows/release.yml` is 0.
- **Estimate.** 1 day. **Depends on.** O1.

#### O3 · Ship v0.1.0: the tag, the marketplace branch, the two install lines

- **Delivers.** The first release that exists: a GitHub Release with every archive, `.sha256`,
  Sigstore attestation and SBOM; the `marketplace` branch with three binaries per target; npm when
  its two settings exist (§5). README and `docs/usage.md` say the two install lines that are true
  and nothing marked "not published yet".
- **Files.** `README.md:17-23, 62-91` (Install: `In Claude Code` the two slash commands; `In a
  terminal` the curl line or `npm install -g @adage-sh/cli`, then `adage init`); `docs/usage.md:150-189`
  (the checkout route moves under Contributing); `npm/cli/README.md:5-18`; `CHANGELOG.md:741-759`
  (rewritten as released); `.claude-plugin/marketplace.json:19`. Outside the repository: §5.
- **Contract.** `git ls-remote origin marketplace` resolves after the tag; `/plugin install
  adage@adage` on the O0 machines installs 0.1.0 with no `adage` on `PATH`
  (`zero_config_plugin.rs`'s story, live); the curl line installs `adage` into `~/.cargo/bin` and
  `adage version` answers `adage 0.1.0`; `npm view @adage-sh/cli version` prints 0.1.0 when npm is
  enabled.
- **Tests.** T-O3-01 (manual, once) the live install on macOS and Linux, recorded in
  `evals/launch/fresh-install.md:30-56` (B1 closed); T-O3-02 the curl installer on Linux x64 and
  macOS arm64, Gatekeeper behaviour recorded (`dist-workspace.toml:44-48`); T-O3-03 `release_plugin_startup`
  green on the tagged artifact; T-O3-04 README has no "not published yet" (an xtask hygiene rule
  gated by a repository variable once the tag exists).
- **Acceptance.** The release page, the branch and the two one-liners work on both machines.
- **Estimate.** 0.5 day plus the outside steps. **Depends on.** O0, O1, O2.

#### O4 · `adage init` wires every harness: `--hooks`, the plugin route first, the Binaries step, stable paths

> **Amended (coordinator, 2026-10-06; supersedes the text it replaces).** The plugin route first is
> *automatic*: `claude plugin marketplace add <local path>` and `claude plugin install
> adage@<marketplace>` were proven headless, exit 0, in a fresh `CLAUDE_CONFIG_DIR` on Claude Code
> 2.1.292, so `adage init` installs the plugin itself from the binary that runs it, with no
> published release. The typed `/plugin marketplace add stevenvaughanbooth/adage` + `/plugin install
> adage@adage` spellings stay as the fallback when `claude` is not on `PATH` or refuses, and as the
> published-release route. Built as described in `docs/claude-mod/impl/O4-O10-notes.md`.

- **Delivers.** One terminal step connects every harness (bar items 1 and 4; G3, G11). For Claude
  Code, when the plugin is not installed and `claude` is on `PATH`, `adage init` materializes a
  local marketplace under `$ADAGE_HOME/plugin/` (`Paths::plugin()`): `.claude-plugin/marketplace.json`
  naming one plugin, `adage`, with the relative source `./adage`; and the plugin directory itself
  (`hooks/hooks.json`, the mod's module files, `scripts/session-start`, `.claude-plugin/plugin.json`,
  `types/`, `README.md`, embedded into the `adage` binary at build time by `crates/adage-cli/build.rs`
  from `plugin/claude-code` minus `dist/`, the mod's tests and their fixtures, so every install
  channel carries its plugin; and `dist/<build target>/{adage,adage-exec,adage-train}` copied from
  the running binary and its siblings, with `dist/SHA256SUMS`). It then runs `claude plugin
  marketplace add <that path> --scope user` and `claude plugin install adage@adage-local --scope
  user` (`local` with `--hooks project`), verifies with `claude plugin list --json` that
  `adage@adage-local` is enabled and read from the staged directory, and reports. Every `claude`
  call goes through `claude_plugin::ClaudeCli`, so tests stub it. Claude Code reads a directory
  marketplace in place, so a re-run (and `adage update`, through `adage init --restage`) stages
  the new binaries and modules for the next session. After the plugin is in place, every Claude
  Code settings-file registration `adage install` wrote that applies here is taken out with
  `install --remove`'s diff and `.adage-bak` (`--yes` accepts; the wizard asks), so every event is
  wired once; a file `adage install` did not write is named for the person to edit. The settings
  file is the second route: when `claude` is not on `PATH` or refuses, `--hooks user|project`
  writes it (diffed, backed up) and names the two typed `/plugin` commands. `adage init --yes
  --hooks user|project|none` registers hooks non-interactively for every harness found (never where
  one already applies, `install.rs:17-22`; refused inside an agent session, `init.rs:50-58`), Codex
  and OpenCode through their files, each diffed and backed up; the wizard lists the plugin route
  first. The Binaries step (before the hooks, so the plugin is staged with all three) fetches
  `adage-exec` and `adage-train` from the release of the running version into the directory `adage`
  lives in, each archive verified against its `.sha256` as `stage-plugin.mjs:62-65` does, only on
  `y` or `--yes` (a download is a person's act, `docs/zero-config.md:29`), and writes
  `$ADAGE_HOME/channel.json { channel: "release", version, at, bin_dir }`; a checkout build is
  pointed at `cargo build`. Registrations written by the plugin's binary name the stable link
  `<data>/bin/adage` so plugin updates carry them. `init` ends with one screen (what is connected,
  the binaries, what Adage learns from, the next session's first line) and at most two `next`
  lines; with `--yes` and history, the first `adage condition` starts in the background.
- **Files.** `crates/adage-cli/build.rs` **(new)**; `crates/adage-cli/src/claude_plugin.rs` **(new)**
  (embedded files, `stage`, `ClaudeCli` + `System`, `list`, `connect`, `verify`, `refresh`);
  `crates/adage-store/src/paths.rs` (`plugin()`); `crates/adage-cli/src/init.rs:1-21, 35-58, 77-80`
  and the harness step (`install::plan` + `install::apply_backed_up`, `install.rs:101, 153`;
  `install::plan_removal` + `remove_backed_up` lifted from `uninstall`; the Codex `/hooks` note at
  `:480-483` once); `crates/adage-cli/src/install.rs:72-88` (`hook_binary`: keep `<dir>/adage` when
  it is a symlink to `current_exe()`'s file name), `:489-503` unchanged;
  `crates/adage-cli/src/detect.rs` (`stable_link`, `in_plugin_path`, `plugin_companions`,
  `claude_plugins`: an installed, enabled plugin before its first session counts as registered);
  `crates/adage-cli/src/ui/wizard.rs:18-26, 848-860`; `crates/adage-cli/src/net.rs` **(new)**: the
  curl runner lifted from `models.rs:330-367` with `classify` (`:281-291`), `get`, `download`,
  `fetch_companion(name, version, into, scratch)`, the target triple from `std::env::consts::{OS,
  ARCH}` mapped to `npm/config.mjs:20-26`'s five; `crates/adage-cli/src/models.rs` (uses `net`);
  `crates/adage-cli/src/status_card.rs:596-626` (not-connected rows: `in Claude Code: /plugin
  marketplace add … then /plugin install adage@adage`, `or, from a terminal: adage init`; other
  harnesses `adage init connects it`); `crates/adage-cli/src/doctor/mod.rs:667-685` (the unhooked
  fix becomes `adage init`); `crates/adage-cli/src/cli.rs` (`--hooks`, hidden `--restage`);
  `crates/adage-exec/tests/no_spawn.rs` (`net.rs`: curl and tar; `claude_plugin.rs`: `claude`);
  `crates/adage-cli/tests/first_run.rs:163-180`; `crates/adage-cli/tests/onboarding.rs` **(new)**;
  `docs/zero-config.md:16-17, 50-60, 332` (`adage connect` is `adage init`'s hooks step);
  `docs/harnesses.md:123-140, 221-226`; `docs/usage.md:191-217`.
- **Contract.** `init --yes --hooks user` at a terminal outside an agent session installs the
  Claude Code plugin from the local marketplace when `claude` is on `PATH` (the calls, in order:
  `plugin list --json`, `plugin marketplace add <$ADAGE_HOME/plugin> --scope user --json`, `plugin
  install adage@adage-local --scope user --json`, `plugin list --json`) and writes at most one
  registration per other detected harness, user scope, each preceded by its diff and followed by
  `wrote` and its backup; a refused `claude` step names the step and Claude Code's own words, then
  the settings file is written instead. `init --yes` without `--hooks` (or `--hooks none`) still
  registers nothing and says so (`first_run.rs:171-174`'s wording kept); `init --print` and `init`
  inside an agent session write nothing and run no `claude`. The local marketplace is idempotent:
  a file already holding its bytes is not rewritten, a binary whose size and modification time
  match `$ADAGE_HOME/plugin/.staged.json` is not hashed again, a file the new set lacks is
  removed. The Binaries step is skipped when both companions sit beside `adage`; a mismatch refuses
  the file and says `mismatch`; `offline`/`blocked`/`tls` continue with one line. A registration
  written by `<data>/bin/adage-<sha>` names `<data>/bin/adage`. `adage init --restage` stages the
  local plugin again from the binary that runs it, refreshes the marketplace and verifies; with
  none staged it says so and exits 0.
- **Tests.** T-O4-01 `init --yes --hooks user` with `ADAGE_ALLOW_NONINTERACTIVE=1` in a World with
  fake Claude Code, Codex and OpenCode homes and no `claude` on `PATH` writes the three user files
  with Adage's entries and a `.adage-bak` for an existing one; doctor is green for each; T-O4-02 a
  second run writes nothing and says where each is registered; T-O4-03 with a `plugin-data`
  registration present, Claude Code is not registered again; T-O4-04 `init --yes --hooks user` with
  `CLAUDECODE=1` refuses; T-O4-05 wizard snapshot: plugin route first when the plugin is absent
  (installed by adage with `claude` on `PATH`, typed without), no Claude Code offer when present,
  the settings file beside it offered for removal, two `next` lines; T-O4-06 `fetch_companion`
  against `ADAGE_RELEASE_URL` serving a `file://` fixture verifies the `.sha256` and refuses one
  altered byte; T-O4-07 the Binaries step is skipped with both companions beside; T-O4-08
  `hook_binary` under a staged plugin writes `<data>/bin/adage`, not the digest name; T-O4-09
  status card snapshots of the not-connected rows; T-O4-10 with a fake `claude` on `PATH` the
  plugin route makes exactly the calls above, stages the marketplace (`marketplace.json`, the module
  files, the binary and its `SHA256SUMS`), takes the earlier settings-file registration out with a
  backup, and a re-run installs nothing and refreshes the marketplace; T-O4-11 a refusing `claude`
  falls back to the settings file and names the `/plugin` commands; T-O4-12 `--print` runs no
  `claude` and writes nothing; T-O4-13 (live, ignored unless asked: `--ignored live_`) the real
  `claude` in a fresh `CLAUDE_CONFIG_DIR` lists `adage@adage-local` enabled, read from the staged
  directory, the staged marketplace passes `claude plugin validate --strict`, and a headless
  session runs each of the plugin's hooks once; the `claude_plugin` unit tests drive `connect`,
  `refresh` and refusals through a stub `ClaudeCli`.
- **Acceptance.** `cargo test -p adage-cli --test first_run --test onboarding --test
  zero_config_plugin` green; `adage doctor` is green for every harness after `init --yes --hooks
  user` in the World; T-O4-13 green on a machine with `claude`.
- **Estimate.** 3 days. **Depends on.** O1 (three binaries to fetch), O3 for the release fetch to
  find anything live (the code and tests do not wait).

#### O5 · The update check: one daily read in the learner, one cache file, zero hook network

- **Delivers.** The binary knows when a newer Adage exists, without any hook touching the network
  (bar item 2; G6, G18). `adage update --check [--now]` fetches, compares with `CARGO_PKG_VERSION`
  and writes the cache; the detached learner calls `update::check_if_due(paths)` once after
  `notice::update` (`autolearn.rs:287`), only with `update.check = daily` and a cache older than
  24 h; a failure is classified, cached and never retried before the next day (the `models fetch`
  rule, `docs/zero-config.md:300`).
- **Files.** `crates/adage-store/src/paths.rs:95-98` (`update_cache`), `crates/adage-store/src/release.rs`
  **(new)**, `crates/adage-store/src/config.rs:78-79, 288-295`; `crates/adage-cli/src/update.rs`
  (`latest::refresh`, `cached`, `newer`, `check_if_due`, `--check`); `crates/adage-cli/src/net.rs`
  (from O4); `crates/adage-cli/src/autolearn.rs:287`; `crates/adage-cli/src/config.rs` (`adage config
  set update.check off` listed); `crates/adage-cli/tests/update.rs` **(new)**: a counting fixture
  server; `crates/adage-cli/tests/hooks.rs:129-157` (the World sets `ADAGE_RELEASE_URL` to the
  counting server); `docs/zero-config.md:78, 158-170`; `docs/live-learning.md` (what reaches the
  network, when, the off switch, the privacy paragraph: one request, no identifier, `User-Agent:
  adage/<version>`).
- **Contract.** The endpoints: `registry.npmjs.org/@adage-sh/cli/latest` (`version`), then, only
  on a 404 from npm (unpublished, never on `offline`/`blocked`/`tls`),
  `api.github.com/repos/stevenvaughanbooth/adage/releases/latest` (`tag_name`); 3 s timeout each.
  No code under `crates/adage-hook`, `adage hook …`, `adage-exec hook` or the mod's bridge reads
  anything but the cache file; the cache is written only by `--check` and `check_if_due`; the
  learner fetches at most once per 24 h per data directory; `update.check = off` and
  `ADAGE_OFFLINE=1` fetch nothing; `--check` prints `adage 0.1.0 · 0.2.0 is out (released 3 days
  ago)` or `adage 0.1.0 is the latest` or `cannot check now: <class> (<host>)`.
- **Tests.** T-O5-01 `--check --now` against the fixture writes the cache with `latest.version`;
  T-O5-02 an unreachable host records `offline` and a second `--check` within 24 h makes no request
  (the server counts); T-O5-03 npm 404 falls through to GitHub, npm `blocked` does not; T-O5-04
  `adage condition --auto` in the World with `update.check = off` makes no request
  (`ADAGE_TEST_NO_SPAWN=record` pattern); T-O5-05 every event in `tests/hooks.rs` runs with the
  counting server and the count stays 0; T-O5-06 the goldens are byte-identical and
  `adage-exec --test no_spawn` green (T-X-12).
- **Acceptance.** `cargo xtask affected` green; `evals/launch/fresh-install.md:39` still true by
  T-O5-05.
- **Estimate.** 2.5 days. **Depends on.** O4 (`net.rs`); O3 for anything to return (the code and
  tests do not wait).

#### O6 · Telling the person: card, `version`, doctor, the notice, TUI Home, the map

- **Delivers.** Every surface reads the cache and says the same thing, and none turns amber for it
  (bar item 2; G6, G15, G16, G18). The card gains the row `update      0.2.0 is out (you have
  0.1.0) → adage update` (`→ /adage update` with `--for-session`) and `--json` gains `"update": {
  "have", "latest", "text", "command", "notes" }`, with `adage update` first in `next`; `adage
  version` prints the pair; doctor's `binary` stays Pass with ` · 0.2.0 is out` and
  `Fix::run("adage update")`; the notice composer gains the lowest kind `update` (`adage: 0.2.0 is
  out (you have 0.1.0); /adage update brings it.` for a plugin binary, `… run adage update.`
  otherwise); TUI Home's first row and the palette's `Update Adage` run `adage update` as a Job; the
  graph JSON gains `adage: { version, latest, channel }` and the TopBar shows the text.
- **Files.** `crates/adage-cli/src/status_card.rs:565-585, 688-705`; `crates/adage-cli/src/main.rs:267-270`;
  `crates/adage-cli/src/doctor/mod.rs:42-48` (the rule written where Status is defined), `:211-215`;
  `crates/adage-cli/src/autolearn.rs:351-430` (`compose` takes `update: Option<(&str, &str)>`),
  `crates/adage-cli/src/detect.rs:49-57` (`in_plugin()`); `crates/adage-hook/src/autolearn.rs:74`;
  `crates/adage-cli/src/ui/tabs/home.rs:16-29`, `ui/palette.rs:84-120`, `ui/jobs.rs:97-105`,
  `ui/health.rs:18-30` (`Fix::Update`), `ui/tabs/health.rs:50-55` (`u`); `crates/adage-cli/src/graph.rs`,
  `crates/adage-app/src/graph.rs:165-166`; `web/map/src/api/types.ts:351-369`,
  `web/map/src/components/TopBar.tsx:93-172`; `crates/adage-cli/src/map/assets.rs` (the committed
  bundle rebuilt; `ci.yml` `pnpm drift`); `docs/design/everyday-ux.md` (rule: version everywhere,
  update never amber).
- **Contract.** With no cache or an up-to-date one no surface changes; with `latest > have` the
  card, `version`, doctor, TUI Home, `adage watch`'s header and the map show the same version pair
  and the same command; the notice shows it at most daily and only after higher kinds (health,
  waiting, value, reflexes; `parts.truncate(2)` at `autolearn.rs:428` drops it first); doctor exits
  0 and the lamp word stays `healthy`; `ui.notices = off` silences the notice and nothing else.
- **Tests.** T-O6-01 card snapshots with and without a newer release (plain, ASCII, colour); T-O6-02
  `adage --json` carries `update` with `command: adage update`, `--for-session` with `/adage
  update`, absent when equal, older or malformed; T-O6-03 doctor `binary` Pass with action `run
  adage update`, overall status unchanged; T-O6-04 `compose`: `update` last, dropped when two
  higher parts exist, worded per channel; T-O6-05 TUI Home snapshot with and without the row
  (`crates/adage-cli/src/ui/snapshots`); T-O6-06 graph JSON carries `adage.version`, `update` null
  without a cache; TopBar vitest in both modes; T-O6-07 `adage version` line.
- **Acceptance.** Snapshots reviewed; `pnpm test`, `pnpm drift`, `pnpm test:e2e` green.
- **Estimate.** 2.5 days. **Depends on.** O5.

#### O7 · `adage update` as a transaction: check, keep, run pinned, verify, re-point, doctor, back

- **Delivers.** One terminal command updates binaries and registrations safely and can undo (bar
  item 3; G7, G9, G10). Steps: (1) check (the cache, or a fetch when stale); `adage 0.2.0 is the
  latest · nothing to do` ends it; (2) the plan printed as today (`update.rs:196-204`) with the
  version pinned: npm `npm install --global @adage-sh/cli@0.2.0` (never `@latest`, `:131`), source
  `cargo install --locked --git … --tag v0.2.0 adage-cli adage-exec adage-train` (not `--branch
  main`, `:112-113`), `release` the installer line for `v0.2.0` plus `fetch_companion` for the two
  companions, checkout and brew unchanged; (3) keep: the running `adage`, `adage-exec`,
  `adage-train` copied to `$ADAGE_HOME/previous/<have>/` with `previous.json { from, to, channel,
  paths, at }`; (4) run with the terminal attached (`:211-232`); (5) verify: the binary at the same
  path answers `version` with `latest`, and its SHA-256 equals the release's `.sha256` where the
  channel installs the archive's bytes (`release`, npm's platform package); (6) re-point: every
  registration whose binary path no longer exists or differs from the new binary, and every one
  lacking a wanted event (`doctor/mod.rs:667-709`), through `install::repoint` (lifted from
  `exec.rs:272-281`), with the diff and `.adage-bak`, at a terminal only; (7) `adage doctor`'s
  summary, the notes' first lines, `done adage 0.2.0 · takes effect in new sessions`. A failed step
  stops, leaves `previous/` in place and ends `next adage update --back`. `--back` restores the
  kept binaries and re-points. Channel detection: `plugin` by `plugins/cache` or `plugins/data`
  segments whatever `CLAUDE_CONFIG_DIR` is (`:57-70`); `release` by `$ADAGE_HOME/channel.json` or
  dist's receipt; `source` without `cargo` on `PATH` is offered the installer line. The plugin
  channel runs `claude plugin update adage@adage` (O0's spelling) when `claude` is on `PATH`, else
  prints the `/plugin` steps (today `:189-195`).
- **Files.** `crates/adage-cli/src/update.rs` (whole); `crates/adage-cli/src/install.rs` (`repoint`,
  `apply_backed_up` at `:153` reused); `crates/adage-cli/src/exec.rs:252-281` (`switch` calls
  `repoint`); `crates/adage-cli/src/doctor/mod.rs` (`brief_all` summary helper);
  `crates/adage-store/src/paths.rs` (`previous()`); `crates/adage-cli/src/cli.rs:933-948`;
  `crates/adage-cli/tests/update.rs` (a fake channel runner on `PATH`: `ADAGE_TEST_UPDATE_RUNNER`);
  `docs/usage.md:191-217` (`update` in the list; a new "Updating" section: check, update, back,
  what is kept, what is verified); `README.md:62-122` (Updating); `docs/execution.md:619`.
- **Contract.** `adage update` never runs a package manager when nothing is newer, never on a hook
  path, never from the learner (`update.rs:16-19` stays true); before any binary is replaced the
  previous set is on disk; a version or digest mismatch after the steps is an error naming both
  values and ending with `--back`; registrations are rewritten only at a terminal, each with its
  diff and backup, only when their path or events changed, and a failed re-point is a `warn` naming
  the exact `adage install <h> --write`, never a silent skip; `--print` runs nothing.
- **Tests.** T-O7-01 `detect`: `/x/cfg/plugins/cache/adage/adage/0.1.0/bin/adage` and
  `…/plugins/data/adage-adage/bin/adage-<sha>` → `plugin`; `channel.json` makes `~/.cargo/bin/adage`
  `release`; T-O7-02 `plan` per channel pins the version (extends `update.rs:270-292`); T-O7-03 up
  to date: no steps, no `previous/`, exit 0; T-O7-04 the fake runner installs a new `adage`:
  `previous/` holds the old bytes, verify passes, doctor runs, `done`; T-O7-05 the runner installs
  a binary answering a wrong version: the error names both, `previous/` intact, `--back` restores
  byte-identical; T-O7-06 a Codex and an OpenCode registration pinning a dead path are rewritten
  with `.adage-bak` at a terminal (`ADAGE_ALLOW_NONINTERACTIVE=1`) and only reported inside an agent
  session; T-O7-07 the runner fails: nothing else runs, `previous/` kept, exit non-zero; T-O7-08
  plugin channel with a stub `claude` on `PATH` runs it and prints `effect: next session`; without
  one prints the `/plugin` steps and runs nothing; T-O7-09 goldens and parity unchanged (T-X-12).
- **Acceptance.** `cargo test -p adage-cli --test update` green; `adage update --print` on each
  channel in the World prints the pinned plan.
- **Estimate.** 3 days. **Depends on.** O5.

#### O8 · Two versions never diverge silently

- **Delivers.** An older binary beside a newer index says so once and stays quiet otherwise;
  doctor's fixes are commands a person can run (bar item 3, safe; G8, G9). `Unloaded::Newer` makes
  `SessionStart` answer one `systemMessage`: `adage: the reflexes here were written by a newer
  adage; this one (0.1.0) stays quiet · adage update` (or `/adage update` for a plugin binary), at
  most once per data directory per day; every other event prints nothing. Doctor's `index` fail
  (`doctor/mod.rs:356-374`) carries `Fix::run("adage update")`. A new `versions` check (Warn)
  compares the plugin's staged binary, `PATH`'s `adage` and every registration's binary by
  `version`, with `Fix::run("adage update")`; the plugin check (`:806-828`) compares versions
  before digests and names user fixes: a broken link → `start a new Claude Code session (the plugin
  relinks its binary)`; a different version → `adage update`; the `plugin/build.sh` sentence only
  when the plugin root is not under `plugins/cache/` (a `--plugin-dir` checkout). The shim removes
  superseded `bin/<name>-<sha>` copies no link points at (safe now that registrations name the
  stable link, O4). The compatibility rule is written: binaries and the mod interoperate within one
  major version; wire answers only gain fields.
- **Files.** `crates/adage-hook/src/ctx.rs:72-78`; `crates/adage-hook/src/session_start.rs:115-147,
  189-201`; `crates/adage-cli/src/doctor/mod.rs:356-374, 806-828` and the new check;
  `crates/adage-cli/src/detect.rs:150-160` (`version` on a registration);
  `plugin/claude-code/scripts/session-start:82-95`; `plugin/test/session-start.test.sh`;
  `crates/adage-cli/tests/hooks.rs` (a newer-format index fixture); `docs/claude-mod/impl/wire.md`;
  `docs/claude-mod/implementation-plan.md:321-326`; `docs/live-learning.md`.
- **Contract.** For every existing golden `adage hook claude <Event>` is byte-identical; with an
  index of `INDEX_FORMAT + 1` only `SessionStart` says anything, once per day, and every hook still
  exits 0; `versions` is Pass when every binary found answers the same version, Warn otherwise with
  the list; the shim leaves exactly one `bin/<name>-<sha>` per name after a start.
- **Tests.** T-O8-01 a newer-format index: `SessionStart` prints the notice, `UserPromptSubmit`
  nothing, the second `SessionStart` that day nothing; T-O8-02 goldens and parity unchanged; T-O8-03
  doctor: `index` newer → run `adage update`; two stub binaries answering different versions →
  `versions` Warn naming `adage update`; plugin binary of a different version → `adage update`;
  `--plugin-dir` root → the build.sh sentence; T-O8-04 `session-start.test.sh`: a seeded stale
  `adage-<oldsha>` is removed, the linked one kept; T-O8-05 the wire mirror check
  (`check-plugin.mjs --wire`) passes after a field is added to a wire file.
- **Acceptance.** `cargo test -p adage-hook -p adage-cli --test hooks` green; the shim test green.
- **Estimate.** 1.5 days. **Depends on.** O7, O4.

#### O9 · The in-session update: the `update` press site, the band tier, `/adage update`, the Health row

- **Delivers.** A Claude Code user updates with one answer and never a terminal (bar items 3 and 5;
  G14, G17), within the authority model. `PersonVerb::Update` joins `ALL` (14); `adage update` calls
  `review::person_or_press(PersonVerb::Update, paths)` at entry, so without a press it is exactly
  today's terminal rule; `PRESS_VERBS` gains `update` and `isPressAllowed` accepts exactly `[cli,
  'update']`. Under a press, `update` skips re-pointing (prints `{n} registrations still run {have}
  · run adage update in a terminal` when any pinned path changed) and runs only channels whose
  update is a fetch (`plugin`, `npm`, `release`); `source`, `checkout` and `brew` answer `manual`
  with the terminal command (a minutes-long build never runs inside a press). The band's lowest tier
  `» adage 0.2.0 is out · /adage update`, until the first prompt, once per new `latest` per machine
  (store key `latest`); `/adage update` opens the ask `Adage 0.2.0 is out (you have 0.1.0). Update
  now? It takes effect in your next session.` with `update` / `not now` / `what changes`; `what
  changes` opens Health on a `version` row with the notes, a `u: update` Button and `Copy: adage
  update`; the toast `adage updated 0.1.0 → 0.2.0 · takes effect in your next session` or `adage
  could not update ({last stderr line}) · run adage update in a terminal`; no press held → kept
  like any other answer (ux-design §9.12) and asked again as `Kept from your last session: update
  Adage to 0.2.0 (adage update). Run it now?`; headless or inert: `Adage 0.2.0 is out; run adage
  update in a terminal.`; up to date: `adage 0.2.0 is the latest.` The ask is never opened unasked.
  Learner notices for plugin binaries say `/adage review` instead of `run <path> review in a
  terminal`; the band's `run adage doctor` and `/adage on`'s `run adage on in a terminal` become
  `/adage health` and the ask where the mod is live (`band.ts:64, 118`; `commands.ts:51, 72`).
- **Files.** `crates/adage-app/src/person.rs:153-171, 175-189, 193-204`; `crates/adage-cli/src/update.rs`
  (entry, the press branch, the JSON tail); `crates/adage-cli/src/autolearn.rs:371-373, 525-528`;
  `plugin/claude-code/hooks/press.ts:20-63`; `hooks/commands.ts:17-23, 49-52, 72, 97-115`;
  `hooks/dialog.ts:26-33, 85-95` (`askUpdate`); `hooks/ask.ts:30-32` (`UPDATE_QUESTION`,
  `UPDATE_ANSWERS`); `hooks/view/band.ts:11-35, 64, 102-120`; `hooks/draw/sites.ts:110-139`;
  `hooks/draw/pane.tsx:303-332`; `hooks/reads.ts:9-23`; `hooks/refresh.ts:21-48`;
  `hooks/plugin.ts:155`; `hooks/register.ts` (the store cell); `types/index.d.ts:159-175`;
  `hooks/*.test.ts`; `scripts/check-plugin.mjs:134` (`review,delegate,on,add,update`), `:334-345`;
  `plugin/claude-code/VALIDATE.txt` (regenerated if a hook moves); `crates/adage-cli/tests/zero_config_plugin.rs:970-979`;
  `docs/claude-mod/architecture.md:849-854` (the site list), `:1079-1085` (unchanged in substance);
  `docs/claude-mod/ux-design.md:435-454` (the tier), `:580-597` (§9.13 Update available), §10 copy
  deck (`band.update`, `ask.update`, `toast.updated`, `toast.update.failed`, `cmd.update.headless`),
  `:711-719` (§13: `update` is a press that only replaces binaries; install stays out);
  `docs/claude-mod/impl/A7-A8-notes.md:77-92`; `docs/execution.md:619`.
- **Contract.** A press runs `adage update` only as that exact argv, only from a dialog answer or
  a focused Button, with the token on stdin (`press.ts:89-95` unchanged); under a press the verb
  edits no settings file, no registration, no config, and never starts a build; a press beside
  `ADAGE_ALLOW_NONINTERACTIVE=1` refuses (`person.rs:221-229`); the band shows the update tier only
  below every other tier and never a strength or lifecycle word; no notice composed for a plugin
  binary contains `in a terminal`; `ALLOWED_VERBS` (`bridge.ts:23`) is unchanged.
- **Tests.** T-O9-01 `PersonVerb::ALL` is the thirteen plus `Update`, `cmd()` round-trips, the
  refusal names what it changes; T-O9-02 World: `adage update` with a valid press, `CLAUDECODE=1`,
  no TTY and a fake fetch-channel runner records `via: press` in `previous.json`, prints `{next}`
  first and `{updated}` last, rewrites no settings file even when one pins a dead path; the `source`
  channel under a press answers `manual` and runs nothing; the same with the variable set refuses;
  T-O9-03 without a press inside an agent session it refuses with the person-only message; T-O9-04
  `isPressAllowed` accepts `[cli,'update']` only (not `update --back`, not `update --check`);
  T-O9-05 check-plugin fails when `install` appears in `PRESS_VERBS` or the literal differs; T-O9-06
  band tier order lapsed > problem > kept > review > update > version, table-driven, and the tier
  drops at the first submit; T-O9-07 the dialog's `update` answer runs the press argv and toasts
  from the JSON tail, `what changes` opens Health with the notes, `not now` records `asked`; T-O9-08
  headless `/adage update` text; T-O9-09 no press held → kept, asked again next start, run only on
  `run it`; T-O9-10 `compose` for a plugin path yields `/adage review`, `adage` on `PATH` yields
  `adage review` (`zero_config_plugin.rs:970-979` updated); T-X-04 the argv recorder: `update`
  appears only inside `press()`; T-X-13 unchanged.
- **Acceptance.** `claude plugin test plugin/claude-code`, `check-plugin.mjs`, `cargo test -p
  adage-app -p adage-cli` green; `VALIDATE.txt` equal to a fresh `validate --strict`.
- **Estimate.** 3 days. **Depends on.** O6, O7, O0 (the spelling).

#### O10 · One front door: the card's `next`, the wizard, the map's Setup, the ports

- **Delivers.** Five entry points stop pointing at each other (G15, G16, G22). Bare `adage`'s
  `next` is at most two lines (`adage update` first when a version is out; `adage review` when
  something waits; else `adage ui`); the map's Setup view, `CommandButton`, Health, Inspector and
  Palette copy read `adage.channel` from the graph JSON and say `In Claude Code: /adage review,
  /adage health` for a plugin user, `adage init`/`adage ui`/`adage update` for a terminal user,
  never `start adage ui once in your terminal` for a plugin user; the demo dashboard moves to 8766;
  `adage ui` Live's `m` opens the browser as `adage watch` does; `docs/usage.md:292, 537` follow;
  `web/map/README.md` **(new)**.
- **Files.** `crates/adage-cli/src/status_card.rs:688-705`; `web/map/src/views/Setup.tsx:5-30,
  43-66`, `components/ui.tsx:181-201`, `views/Health.tsx:146-150`, `components/Inspector.tsx:595-607`,
  `components/Palette.tsx:184`; `demo/dashboard/serve.py:17`; `crates/adage-cli/src/ui/view.rs:296,
  993-1002`, `ui/keys.rs:28`; `crates/adage-cli/src/map/assets.rs` (rebuilt); `docs/usage.md:292, 537`.
- **Contract.** No default surface tells a plugin-only person to open a terminal for something a
  `/adage` verb or a press does; every `next` names one command or one `/adage` verb; the web app
  makes no request but to its own origin (`map/mod.rs:13-14`); the graph JSON stays readable by the
  current app when `adage` is absent.
- **Tests.** T-O10-01 card snapshots: connected, waiting, update out, plugin channel; T-O10-02
  Setup vitest for `channel: plugin` (no `adage ui` text) and `channel: npm`; T-O10-03 the e2e suite
  loads the page served by the binary with `adage.version` present; T-O10-04 `adage map`'s default
  port and the demo dashboard's differ; T-O10-05 T-U-01's reading part on the new words (8 readers,
  7 of 8).
- **Acceptance.** `pnpm typecheck`, `pnpm test`, `pnpm drift`, `pnpm test:e2e` green; snapshots reviewed.
- **Estimate.** 1.5 days. **Depends on.** O6.

#### O11 · The live gate: install, update and every surface on real machines

- **Delivers.** The whole story has run where the person runs it (G5, G19). On a clean macOS
  (arm64) and a clean Linux (x86_64) machine with Claude Code at the pinned version and the latest:
  `/plugin marketplace add` + `/plugin install adage@adage` from the published marketplace; the
  first-session line, band, first ask and press; `adage update --check` against the real release;
  a second tag (`v0.1.1`, notes only) published; the band's update tier after a learner run;
  `/adage update` → press → `claude plugin update adage@adage` as a child of the engine; the new
  session on 0.1.1; the curl installer + `adage init` (the Binaries step live) on Linux and macOS;
  `npm install -g` + `adage init --yes --hooks user` where npm is published; `adage update` on
  `release`, `npm` and `checkout` channels with `--back`; Codex and OpenCode `adage init` and one
  live session each (`docs/harnesses.md:262-272` closed or amended); Desktop app drawing
  (`$.process`, Q7); T-U-01 with the update journey added to the doing part; Gatekeeper behaviour of
  curl-installed binaries recorded; Windows explicitly out of scope and said so.
- **Files.** `docs/claude-mod/evaluation-plan.md` (a "first run and update" section with the
  checklist and results); `docs/claude-mod/test-plan.md` (T-U-01 grows the update journey; new live
  cases T-L-O1..O8); `evals/launch/fresh-install.md:30-56` (B1 closed; a new "Update" section);
  `docs/claude-mod/README.md:7`; `docs/claude-mod/impl/README.md:6-8, 50-56`; `docs/harnesses.md:262-272`;
  `docs/claude-mod/ux-design.md:13-22, 744-749` (verified marks); `docs/usage.md:171-174`;
  `plugin/claude-code/scripts/session-start:51-52` (the Windows note kept).
- **Contract.** Each checklist line carries a date, machine, Claude Code version and outcome; a
  failed line opens a chunk, not a footnote; `release_plugin_startup` holds its budgets on the
  release binary with three binaries bundled; the paired latency runs (test-plan §1.5) show no p95
  regression at session start.
- **Tests.** T-L-O1 marketplace install and the first ten minutes with no terminal, both platforms;
  T-L-O2 `update --check` reads the real latest; T-L-O3 the band tier appears after a learner run;
  T-L-O4 `/adage update` press runs the plugin update and the next session runs 0.1.1; T-L-O5
  curl installer + `adage init` with the Binaries step; T-L-O6 `adage update --back` on `release`
  and `npm`; T-L-O7 Codex and OpenCode `adage init` and one live session each; T-L-O8
  `release_plugin_startup` on the release binary; T-U-01 (update journey added).
- **Acceptance.** Every live case passes on both platforms before the release is announced.
- **Estimate.** 2 days. **Depends on.** O3, O4, O8, O9, O10.

#### O12 · Words that tell the truth: README, usage, zero-config, plugin README, help, CHANGELOG

- **Delivers.** Every page a first-time or returning person meets tells the same two stories
  (G21): README Install is the two one-liners and Quickstart leads with `/adage`, the band and the
  first ask; an `Updating` section (`/adage update` or `adage update`; what is kept; `--back`);
  `docs/usage.md`'s command list has `update` and its Claude Code section drops `plugin/build.sh` for
  end users; `docs/zero-config.md`'s status block, principle 2's network sentence (`:78`), `adage
  connect` and `run adage review in a terminal` are rewritten to what shipped;
  `plugin/claude-code/README.md` gains "Installing and updating" for people; `npm/cli/README.md`
  says what the binary does (advises; execution opt-in by a press or `adage delegate on`) and lists
  `init` and `update`; `docs/claude-mod/implementation-plan.md:321-326`'s promises are marked built
  (O8) or struck; `docs/claude-mod/architecture.md:663-665` says doctor reports version drift (true
  after O8); `docs/how-it-works.md` is updated; this plan is appended to the implementation plan's
  index as Workstream O.
- **Files.** `README.md:17-23, 62-122`; `docs/usage.md:150-217, 327-345`; `docs/zero-config.md:11-25,
  29, 42, 50-60, 76-82, 158-170, 285-300, 309-335`; `docs/harnesses.md`; `plugin/claude-code/README.md:1-8`;
  `npm/cli/README.md:5-18`; `.claude-plugin/marketplace.json:19`; `crates/adage-cli/src/cli.rs:933-948`;
  `crates/adage-app/src/words.rs` (the install and update strings); `docs/claude-mod/implementation-plan.md`;
  `docs/claude-mod/architecture.md:663-665`; `docs/how-it-works.md`; `SECURITY.md`; `CHANGELOG.md`;
  `xtask/src/hygiene.rs` (two rules below).
- **Contract.** No page names a channel that is not live at that release without saying so; every
  backquoted `adage <verb>` in `README.md` and `docs/usage.md` is a verb `cli.rs:920-981` lists or
  `/adage`'s `VERBS` (`commands.ts:18`) holds; README's install and update lines equal the strings
  in `words.rs`; `cargo test -p xtask` green.
- **Tests.** T-O12-01 hygiene: `not published yet` absent from `README.md`, `docs/usage.md`,
  `docs/zero-config.md` and `plugin/claude-code/README.md` (gated by the release variable); T-O12-02
  the install and update lines in README equal `words.rs` constants; T-O12-03 every backquoted
  `adage <verb>` in the two pages is a listed verb; T-O12-04 check-plugin's description rule (`on
  PATH`) still passes.
- **Acceptance.** `cargo test -p xtask` green; a reader following README alone installs and
  updates on each platform (O11's machines, re-run on the final words).
- **Estimate.** 1.5 days. **Depends on.** O11.

## 4. Sequencing and the critical path

```text
O0 ─────────────────────────┐
O1 ──▶ O2 ──▶ O3 ───────────┼──▶ O5 ──▶ O6 ──▶ O9 ──▶ O11 ──▶ O12
 │                          │     └───▶ O7 ──▶ O8 ─┘  ▲
 └──▶ O4 ───────────────────┘                    O10 ─┘
```

- O0 (2.5 d) runs in parallel with O1 → O2 (2 d); O3 waits for all three and is the first release.
- O4 needs only O1 and can start on day 2; its live Binaries step is exercised in O11.
- O5 → O6 → O9 is the notice-to-press line; O5 → O7 → O8 the update-and-skew line; O9 needs both.
- O10 hangs off O6 and runs beside O7/O8.
- O11 gates the announcement; O12 sweeps the words after the live run.

Estimates: O0 2.5, O1 1, O2 1, O3 0.5, O4 2.5, O5 2.5, O6 2.5, O7 3, O8 1.5, O9 3, O10 1.5,
O11 2, O12 1.5: **25 engineer-days**, about five weeks for one engineer. The critical path is
O0 → O3 → O5 → O7 → O9 → O11 → O12 (about 15 days of work); the first-time story (bar items 1,
4 for Claude Code, 5) is live on both platforms at O3 + O4, about day 7; the update story (bar
items 2, 3) at O9, about day 20.

## 5. What lies outside the repository, and the minimal version of each

| Item | What the plan needs | Minimal version | Named by |
| --- | --- | --- | --- |
| **The repository's visibility** | `/plugin marketplace add stevenvaughanbooth/adage` clones with `git-subdir` (`.claude-plugin/marketplace.json:13-18`); `releases/latest/download/…` and `api.github.com/…/releases/latest` need a readable repository; today it is private (`README.md:17-23`) | Make the repository public at the first tag. Alternative kept in reserve (`docs/zero-config.md:309`): push the `marketplace` branch to a public `stevenvaughanbooth/adage-marketplace` and point `marketplace.json`'s source at it; the release assets then also need a public mirror for the curl line | O3 |
| **The tag** | `release.yml` builds, attests, writes `.sha256` per archive and the SBOM, creates the GitHub Release, writes the `marketplace` branch | `cargo xtask bump 0.1.0 && git tag v0.1.0 && git push origin v0.1.0` (after O2) | O3 |
| **Release assets** | `adage-cli-installer.sh`, `adage-exec-<target>.tar.xz`, `adage-train-<target>.tar.xz` with `.sha256` beside each (`stage-plugin.mjs:62-65` pattern) for the five targets; the Release body's first bullets are the notes the surfaces show | dist produces them from `dist-workspace.toml:24-30`; no change | O3, O4, O7 |
| **npm** | `@adage-sh/cli` and the platform packages (`npm/config.mjs:7, 20-26`) for the terminal one-liner and the update check's first endpoint | Set `NPM_TOKEN` and the variable `ADAGE_NPM_PUBLISH=true` (`publish-npm.yml:7-10, 27`). Optional for the first tag: the plugin and the curl line cover both platforms, and the check falls through to GitHub when npm answers 404 | O3, O5 |
| **Homebrew** | Not on the bar's path | Deferred until macOS signing exists (`dist-workspace.toml:44-48`: bottles would be quarantined). Then: `macos-sign = true` and its three secrets, `HOMEBREW_TAP_TOKEN`, the tap repository, `dist generate`, the Homebrew installer and job back in `dist-workspace.toml:18, 23`, and `adage update`'s brew arm pins the version and re-points the Cellar path at a terminal | later |
| **Two machines with a Claude account** | O0 and O11; the build machine had no key (`docs/claude-mod/impl/README.md:6-8`) | macOS arm64 and Linux x86_64, Claude Code at 2.1.288 and the latest | O0, O11 |
| **Connect Codex and OpenCode from the pane** | Would fully close bar item 4 for a Claude Code user who also runs Codex or OpenCode | **An owner decision, not scheduled.** It makes `install <h> --write` a press, which three pinned guards forbid (`press.ts:23`, `check-plugin.mjs:135-137`, ux-design §13 `:713-714`) and `docs/zero-config.md:82` allows only "with a shown diff". The minimal version this plan ships: one terminal step, `adage init`, said once in the card and Health (O4, O10). If the owner decides for it, it is one chunk after O9: a fifteenth site `PersonVerb::Connect(Harness)` for Codex and OpenCode only, the diff from `adage install <h> --print` shown in a focused pane before the press | § |
| **A signed release manifest** | Authenticity beyond TLS and same-origin `.sha256` | Not needed for the bar: Sigstore attestations already exist (`release.yml`'s attest step) and `gh attestation verify` is advisory in `adage update` when `gh` is on `PATH`. A later hardening may add `release.json` signed with sshsig in a new namespace through `crates/adage-sign/src/lib.rs:1-9, 29`, at the cost of a release key to keep | later |
| **Windows** | `install --write` and the plugin on Windows | Out of scope for this release and said so (`session-start:51-52`, `docs/usage.md:171-174`) | O11 |

## 6. Risks, and what the plan does about them

- **The repository is private.** Nothing in §1.1 works for anyone without access until it is
  public (or mirrored). The plan makes this the first outside step (O3), and the update check
  degrades to a cached `unavailable` that `adage update --check` explains and no surface shows as a
  problem.
- **Nothing has run live.** O0 is first for this reason; every Claude Code command string later
  chunks print is quoted from its notes; `/adage update`'s plugin arm has the `/plugin` text as its
  fallback; O11 re-runs the whole story before announcing.
- **`claude plugin update adage@adage` as a child of the running engine.** Verified real in the
  installed CLI by one judge (2.1.292, "restart required to apply") but not in this tree. The
  running session is safe by construction: hooks run the digest-named copy in `$DATA/bin`
  (`session-start:84-95`), not the plugin cache. If the command needs a TTY or does not exist at the
  pinned version, O0 records it and the pane shows the `/plugin` steps; Claude Code's own
  marketplace auto-update may already refresh the plugin, in which case the check finds nothing to
  do.
- **A network request from the learner is new.** Today only `models fetch`, run by a person,
  reaches the network (`docs/zero-config.md:300`). This is one GET of a fixed URL, daily, no
  identifier, off every hook path, with `update.check = off`, `ADAGE_OFFLINE=1`, a privacy paragraph
  and T-O5-05 proving zero hook requests. A cautious owner may want opt-in: one default flip in
  `config.rs`, which would leave terminal users unnotified until `adage update --check`.
- **A fourteenth press site widens what a press can do** from reviewing and granting to replacing
  Adage's own binaries. Mitigations: exact argv with no arguments; the channel chosen by the binary
  from where it runs; the previous set kept; no settings-file edit and no build under a press; the
  notes shown before the answer; the ask opened only by the person. As architecture §3.12 says, the
  press is a provenance record and a steering guard, not a boundary against a same-user agent with a
  shell.
- **The `release` channel's detection** relies on `$ADAGE_HOME/channel.json` (written by the
  Binaries step) or dist's receipt (unverified in the tree). If neither exists, `release` collapses
  into `source`, which now offers the installer line when `cargo` is absent instead of failing; O11
  confirms the receipt on the first real artifact.
- **Same-origin verification.** The companion fetch and the npm/release verify step trust the
  `.sha256` beside the archive on the same release: integrity, not authenticity. Sigstore
  attestations are the independent check when `gh` is present; a signed manifest is listed in §5
  as later hardening.
- **Mod/binary skew.** `/plugin` moves both together; a PATH `adage update` beside an older plugin
  leaves two versions, which O8's `versions` check names with `adage update` as the fix and the
  additive wire rule keeps drawable. A breaking wire change would need a major version and a mod
  that refuses to activate across majors, which this plan does not build.
- **Codex and OpenCode still need `adage init` at a terminal** and Codex's `/hooks` approval;
  neither adapter has run against a live harness (`docs/harnesses.md:262-272`). O11 runs one
  session each; the pane route is the owner's decision (§5).
- **The release workflow has never run on a tag.** `publish-marketplace.yml` and `publish-npm.yml`
  are exercised only by local smoke tests. The first tag may need a second patch tag; `cargo xtask
  bump` makes that cheap, and O11's `v0.1.1` is that second tag anyway.
- **Deleting the alias plugin** is safe only because nothing shipped (0 tags); O1 lands strictly
  before O3's tag, and `ci.yml:271`'s validate line and `check-plugin.mjs`'s alias branch go in the
  same change.
- **Goldens and the control arm.** Nothing here touches `decide()`, the spool records or the arm;
  the only new hook output is O8's `SessionStart` notice for a newer-format index. CI's golden diff
  and the parity test guard against an accidental re-bless; each chunk's acceptance names them.
- **Estimates assume one engineer** and that Claude Code's plugin API at the pinned version holds
  (architecture §6 invariant 10). The outside steps are minutes of work but gate everything.

## 7. Acceptance, rolled up: the bar, item by item

| Bar item | Done when | Proved by |
| --- | --- | --- |
| **(1) Install in one obvious step on macOS and Linux, no manual hook wiring** | In Claude Code: `/plugin marketplace add stevenvaughanbooth/adage` + `/plugin install adage@adage` installs one plugin carrying three verified binaries for both platforms, and the first session's hooks run with no `adage` on `PATH`. In a terminal: `curl … adage-cli-installer.sh \| sh && adage init` (or `npm install -g @adage-sh/cli && adage init`) installs, fetches the companions on `y`, registers hooks for every harness found after showing each diff, and `adage init --yes --hooks user` does it with no questions | T-O1-01..05, T-O2-01..04, T-O3-01..04, T-O4-01..09, T-L-O1, T-L-O5 |
| **(2) Told when a new version exists, in the surfaces they already look at** | After a release, with no hook touching the network: the session-start notice, the band's second line, the card and `/adage`, `adage version`, `adage doctor` (lamp still green), TUI Home and the map's TopBar all say `0.2.0 is out` with the same command; `update.check = off` silences the check and `ui.notices = off` the notice | T-O5-01..06 (T-O5-05: zero hook requests), T-O6-01..07, T-O9-06, T-L-O2, T-L-O3 |
| **(3) One obvious command or click updates binaries, plugin and app together, safely** | In Claude Code: `/adage update` → the ask → `update` (a press) runs the plugin channel's update and toasts `adage updated 0.1.0 → 0.2.0 · takes effect in your next session`; the running session is untouched; the next session's shim links the new three binaries. In a terminal: `adage update` checks, keeps the previous set, runs the package manager with the version pinned, verifies `adage version` (and the digest where it can), re-points every registration with a diff and backup, runs doctor and prints what changed; `adage update --back` undoes it; two installed versions that disagree are named by doctor with `adage update` as the fix and an older hook says so once | T-O7-01..09, T-O8-01..05, T-O9-01..10, T-X-04, T-X-13, T-L-O4, T-L-O6 |
| **(4) Run the app with every plugin, no extra steps** | From the one plugin: `adage` decides, `adage-exec` acts after a press, `adage-train` compiles; the band, dialog, `/adage`, the pane and `Explore in browser` draw one state; the map and `adage ui` show the version and the update line and name the right next step by channel; Codex and OpenCode are wired by `adage init` once (the one terminal step left, said once), their registrations follow updates (the plugin's stable link; `repoint` for PATH binaries), and their notices reach the card, `adage watch`, Home and doctor | T-O1-03, T-O4-01..03, T-O4-08, T-O6-05..06, T-O7-06, T-O10-01..05, T-L-O7, T-L-O8 |
| **(5) The no-terminal first ten minutes** | §1.5's table runs on both platforms with no command, copy or terminal: install, the opening line, the band, the first reflex, the first ask as a press, `/adage`, the map, and on day two the update notice and `/adage update` as a press; no notice composed for a plugin binary says `in a terminal`; the map's Setup never sends a plugin user to `adage ui` | T-O9-10, T-O10-02, T-L-O1, T-L-O4, T-U-01 (reading, arm and doing parts, with the update journey; 7 of 8, under 60 s each) |
| **Words** | Every page tells the same two stories; no channel is named that is not live without saying so; every command named exists in `adage --help` or `/adage`'s verbs | T-O12-01..04 |
