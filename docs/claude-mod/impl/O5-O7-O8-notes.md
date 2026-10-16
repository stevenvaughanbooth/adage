# O5 · O7 · O8 notes: the update check, `adage update` as a transaction, two versions never diverge silently

Built on `claude/mods-poc-m98u3o` at `b6548a2`, beside the O1/O2 and O4/O10 agents. The plan is
`docs/claude-mod/onboarding-plan.md` (§1.3, §2.1–2.4, O5, O7, O8, §6).

## For the agents working beside this one (read first)

- **The status card's `update` field** (O4/O10 added the line in `status_card::run` and draw the
  row from it): `crate::update::card_json(paths, for_session)`, `null` unless the cache names a
  release newer than this binary; then

  ```json
  {"current": "0.1.0", "latest": "0.2.0", "channel": "npm", "next": "adage update",
   "text": "0.2.0 is out (you have 0.1.0)", "published_at": "2026-10-03T12:00:00Z",
   "url": "https://github.com/stevenvaughanbooth/adage/releases/tag/v0.2.0", "notes": "…"}
  ```

  `current` is the plan's `have`, `next` the plan's `command` (`/adage update` with
  `--for-session`, else `adage update`), named as the coordinator's brief asked. `channel` is
  `plugin`, `npm`, `release`, `source`, `checkout` or `brew`. `published_at`, `url` and `notes` are
  `null` when the source did not say (npm says only the version). Fields are only ever added.
- **For wave B (O6, O9)**: `crate::update::newer(paths) -> Option<Release>` (no request; the cache
  only), `adage_store::release::{Latest, Release, Failure, Version, is_newer, plugin_binary,
  VERSION}` (layer 0, so the hook may read it), `update::latest::sentence(&Latest, now)` (the
  `--check` line), `adage update --check --json` (`{current, latest, newer, checked_at,
  published_at, url, notes, failure}`). `previous.json` has `via` (`terminal` today; O9's press
  writes `press`). The notice composer's fifth kind, the band tier, `/adage update`, the press site,
  doctor's `binary` detail and `adage version`'s pair are not built here.
- **For O4**: `Paths::channel_file()` (`$ADAGE_HOME/channel.json`) is the one accessor (O4's
  Binaries step writes it, with `bin_dir`); O4 added `Paths::plugin()`. `adage update` reads
  `{channel: "release", bin_dir}` as "the adage in `bin_dir` is the release's". One consequence to
  weigh there: the Binaries step writes the file whenever it fetched a companion, so an `adage`
  from `cargo install adage-cli` whose `adage-train` was fetched is then updated as a release
  install (the installer for the new version, then the companions again), not with `cargo
  install`. `adage update`
  calls `claude_plugin::{System, list, refresh, stage, plugin_dir, target, PLUGIN_ID, MARKETPLACE}`
  as they are. Doctor's unhooked-harness fix is now `Fix::run_in("{} connects it", "adage init
  --yes --hooks user")` and its optional line names `adage init`, as O4 asked. `detect::in_plugin_path`
  duplicates `adage_store::release::plugin_binary` (one rule for the hook and the CLI); either may go.
- **For O1/O2**: `adage update` checks the installed binary against a release asset `SHA256SUMS`
  (the plugin's `dist/SHA256SUMS` format, `<sha256>  <target>/<name>`) at
  `…/releases/download/v<version>/SHA256SUMS` when the release publishes one; publishing the
  `SHA256SUMS` `publish-marketplace.yml` already computes as a release asset turns the check on.
  O8's shim half (remove superseded `bin/<name>-<sha>` copies after relinking; T-O8-04) lives in
  `plugin/claude-code/scripts/session-start`, which this chunk does not own: O1/O2's notes give the
  exact line; it is not built here.
- **`crates/adage-exec/tests/no_spawn.rs`**: one entry added, `adage-cli/src/update/probe.rs` (1:
  `version` of an installed Adage binary, and `update --finish`). `net.rs` and `claude_plugin.rs`
  are O4's entries.

## What was built

### O5 · the update check

- `adage_store::release` (new, layer 0): `Latest { checked_at, have, latest: Option<Release>,
  failure: Option<Failure> }`, `Release { version, published_at, notes, url, source }`, `Failure {
  class, host, at }`, `Latest::{read, write, due, newer_than}`, `CHECK_EVERY_MS` (a day),
  `Version` (semantic order: a pre-release before its release, numeric identifiers numerically,
  build metadata ignored, a tag's `v` dropped), `is_newer`, `plugin_binary` (`plugins/cache` or
  `plugins/data` segments, any `CLAUDE_CONFIG_DIR`), `VERSION`.
- `Paths::update_cache()` → `$ADAGE_HOME/cache/latest.json`, `Paths::previous()` →
  `$ADAGE_HOME/previous/`, `Paths::channel_file()` → `$ADAGE_HOME/channel.json`.
- Config `update.check` (`daily` | `off`, default `daily`; `Config::update_check`), environment
  `ADAGE_UPDATE_CHECK` (`0`/`1`, or `off`/`daily`). Listed by `adage config` (last, a new
  `[update]` section, so no existing settings snapshot moves).
- `crates/adage-cli/src/update/latest.rs`: `endpoints`, `refresh` (fetch, cache answer or failure,
  keep the last known release on a failure), `current` (the cache unless a day old, or `--now`),
  `check_if_due` (the learner's: `update.check` on, not `ADAGE_OFFLINE=1`, cache a day old, one
  checker per data directory under `cache/latest.lock`), `sentence`, `explain`. Requests go
  through O4's `net::get` (curl, `--proto =https`, `User-Agent: adage/<version>`, http only to
  127.0.0.1/localhost, 3 s `--max-time`).
- `autolearn::run_auto` (the learner) runs `release_check` last, with the repository's lock
  released, inside `catch_unwind`.
- `adage update --check [--now] [--json]`.

**The cache file** (`$ADAGE_HOME/cache/latest.json`):

```json
{"checked_at": 1759800000000, "have": "0.1.0",
 "latest": {"version": "0.2.0", "published_at": "2026-10-03T12:00:00Z", "notes": "first\nlines",
            "url": "https://github.com/stevenvaughanbooth/adage/releases/tag/v0.2.0", "source": "github"},
 "failure": null}
```

A failed check writes `failure: {class, host, at}` and keeps `latest`. Classes: `offline`,
`blocked`, `tls`, `timeout`, `http`, `error` (curl's, through `net::classify`), and `unavailable`
(npm 404 and then GitHub 404: no release this machine can see). Every string from the network is
checked: the version must parse, the URL must be `https://`, notes are the body's first three
bullets (else first lines), control characters removed, 120 characters a line.

**Endpoints.** `https://registry.npmjs.org/@adage-sh/cli/latest` (`version`); only on its 404,
`https://api.github.com/repos/stevenvaughanbooth/adage/releases/latest` (`tag_name`, `v` dropped;
`published_at`, `html_url`, `body`). With `ADAGE_RELEASE_URL=<base>` (O4's release-download base,
for mirrors and tests): `<base>/npm/@adage-sh/cli/latest` and
`<base>/github/repos/stevenvaughanbooth/adage/releases/latest`.

**Zero hook network (T-O5-05).** Every hook run in `tests/hooks.rs` (and so in adage-exec's parity
test, which compiles it) has `ADAGE_RELEASE_URL` and all six proxy variables pointing at a local
server that counts connections, `NO_PROXY` removed and the daily check on; each `World` asserts the
count is zero when dropped, and `no_hook_event_reaches_the_network` runs every Claude Code
scenario event plus Codex and OpenCode events with the learner allowed (`ADAGE_TEST_NO_SPAWN=record`)
and asserts the count and that no hook wrote the cache. Nothing under `crates/adage-hook` imports
the check; the hook reads only the cache's type.

### O7 · `adage update` as a transaction

`crates/adage-cli/src/update.rs` (entry, transaction, `--check`, `--rollback`, `--finish`, plugin
channel, `card_json`, `newer`) with `update/{channel, latest, keep, probe, repoint}.rs`.

Steps and files:

1. **Check** (`latest::current`): with nothing newer, `adage 0.1.0 is the latest · nothing to do`,
   exit 0, nothing kept or run (and a local plugin staged from another build is staged again from
   this one, so `adage update` is also the fix doctor names for a skewed local plugin). A channel
   that pins (npm, source, release) with no version known stops with `cannot check now: <class>
   (<host>)`; a checkout and a `--branch` build do not ask.
2. **Plan**, printed: `adage update · 0.1.0 → 0.2.0 · npm`, `installed`, `keeps
   …/previous/0.1.0/ (adage, adage-exec, …)`, `runs …`. npm `npm install --global
   @adage-sh/cli@0.2.0`; source `cargo --config net.git-fetch-with-cli=true install --locked --git
   … --tag v0.2.0 adage-cli adage-exec adage-train` (`--branch <name>` instead when asked); release
   the installer for `v0.2.0` (fetched with `net::download`, then run with `sh` / PowerShell, no
   pipe) plus `net::fetch_companion` for each companion the install had; checkout `git -C <root>
   pull --ff-only` + `cargo build` of the same profile; brew `brew upgrade` per formula. A source
   install with no `cargo` is refused with `adage update --from release` offered (`--print` only
   warns). `--print` stops here: nothing kept, run or changed.
3. **Keep** (`keep::keep`): `adage` and each companion beside it copied to
   `$ADAGE_HOME/previous/<have>/`, each copy checked against the original's SHA-256, then
   `previous.json {from, to, channel, at, via, bin, paths, sha256, restored_at}` (written in a
   temporary directory renamed into place).
4. **Run** each step with the terminal attached. A failure stops: the kept set stays, the error
   names the command to retry and `adage update --rollback`.
5. **Verify**: the binary the channel installed (the same path; Homebrew's link on `PATH`; cargo's
   bin directory for a source build) answers `version` with the version asked for (a checkout: its
   `Cargo.toml` version after the pull); for npm and release, its SHA-256 against the release's
   `SHA256SUMS` when published (`matches` / `the release publishes no SHA256SUMS; npm checks the
   package's integrity`). A mismatch is an error naming both values and `--rollback`.
   `previous.json` then records `to` and `bin`.
6. **Hand over**: `<new> update --finish <from>` with the terminal attached (`probe::hand_over`),
   so the rest runs as the new version. Exit 2 (a binary that does not know `--finish`) makes the
   old binary finish instead; any other failure names `--rollback`.
7. **Finish** (`update --finish`, hidden): re-point (`repoint::plan` + `repoint::apply`): every
   registration (settings file or OpenCode plugin file, user scope and this repository) whose
   binary is one of the old paths, in their directory, or gone, re-planned through
   `install::plan` for the binary of the same name the update installed; written only when the
   text changes, each diff shown, `install::apply_backed_up` keeping `<file>.adage-bak`; at a
   terminal after one `Re-point them? [y/N]` (`--yes` skips; `ADAGE_ALLOW_NONINTERACTIVE=1` with no
   terminal writes); inside an agent session only listed, each with `adage install <h> --write
   --scope <s> --bin <path>`; a file `adage install` does not write, or a failed write, is a warning
   naming what to do. Then `restage_plugin` (`claude_plugin::refresh`, or `stage` and the named
   `claude plugin marketplace update adage-local` without `claude`), doctor's summary (`doctor ●
   healthy` or `● 2 to look at · <check>: <fix>`), the release's first notes (`changed · …`),
   `done adage 0.2.0 · takes effect in new sessions`, `next adage update --rollback`; the kept
   sets of other versions are removed.
8. **`--rollback`** (alias `--back`): the newest kept set; every kept file checked against its
   recorded SHA-256 before anything is replaced (all or none), then each written beside its
   destination and renamed into place (moved aside first on Windows), checked again; the restored
   `adage` answers `version`, then finishes as above with `--rollback` (registrations follow the
   restored paths). `--print` lists what it would restore.

**Plugin channel** (the binary lives under `plugins/cache` or `plugins/data`): without `claude` on
`PATH`, the `/plugin` steps and nothing run; with it, `claude plugin list --json` finds the
installed id (default `adage@adage`), then `claude plugin marketplace update <marketplace> --json`
and `claude plugin update <id> --json`; `plugin 0.1.0 → 0.2.0`, `effect next session`, `done …
takes effect in your next session`; `up_to_date` says so. The local plugin (`adage@adage-local`)
is updated by updating the terminal install that staged it, which the output says.

### O8 · two versions never diverge silently

- `adage_hook::ctx`: the index load keeps `Unloaded::Newer(format)` (`HookCtx::index_newer`).
- `session_start`: with a newer index, records the session as seen (in a repository), starts no
  bootstrap or rebind learner, and returns one `systemMessage`: `adage: the reflexes here were
  written by a newer adage; this one (0.1.0) stays quiet · adage update` (`/adage update` from the
  plugin's binary), at most once a day per data directory (marker `$ADAGE_HOME/cache/skew-notice.json
  {at, format}`; said never again that day if it cannot be written), only with `ui.notices` on.
  Every other event says nothing, as with no index.
- The learner (`run_auto`) conditions nothing over a newer index (its index would replace the
  newer one); it still runs the daily release check.
- Doctor: `index` newer fails with `Fix::run_in("run {} (…)", "adage update")`; new `versions`
  check: this binary, `PATH`'s `adage`, the plugin's staged binary and every registration's binary
  asked their version (`probe::askable`: absolute, the file that runs is named `adage`,
  `adage-exec`, a digest copy of one, or npm's `adage.js`, and neither the path nor its target is
  inside the repository doctor runs in; answers cached per path, size and time for the process);
  Pass when all agree, Warn listing the others, `Fix::run("adage update")` when this binary is the
  older, otherwise what updates the older one (the local plugin: `adage update` stages it again;
  the published plugin: `/plugin`; another install: its own `update`). The plugin check compares
  versions before digests: broken link → `start a new Claude Code session (the plugin relinks its
  binary)`; another version → the same fix as `versions`; same version, another build → Pass,
  except a `--plugin-dir` checkout (no `adage@…` in Claude Code's `installed_plugins.json`) →
  the `plugin/build.sh` sentence. `Status`'s doc states the rule (a newer release is never a
  problem; two installed versions are).

## Tests (ids from the plan)

| Id | Where |
| --- | --- |
| T-O5-01 | `tests/update.rs::a_check_writes_the_cache` |
| T-O5-02 | `tests/update.rs::a_failed_check_is_cached_for_a_day` (also `ADAGE_OFFLINE=1`) |
| T-O5-03 | `tests/update.rs::npm_404_falls_through_to_github_and_a_block_does_not` (also `unavailable`) |
| T-O5-04 | `tests/update.rs::the_learner_checks_daily_and_never_when_off` (env and config) |
| T-O5-05 | `tests/hooks.rs`: every `World` (counter) and `no_hook_event_reaches_the_network`; the same through `adage-exec` in `parity` |
| T-O5-06 | goldens unchanged (`git status crates/adage-cli/tests/golden`: one new file only), `adage-exec --test parity` green |
| T-O7-01 | `update::channel::tests::the_channel_is_read_from_where_the_binary_lives` |
| T-O7-02 | `update::channel::tests::each_channel_pins_the_release`; `tests/update.rs::the_plan_is_pinned_per_channel_and_print_runs_nothing` (npm, source, release, checkout) |
| T-O7-03 | `tests/update.rs::up_to_date_runs_nothing` |
| T-O7-04 | `tests/update.rs::an_update_keeps_verifies_finishes_and_rolls_back` |
| T-O7-05 | `tests/update.rs::a_wrong_version_is_an_error_and_the_way_back_works`; `update::keep::tests` |
| T-O7-06 | `tests/update.rs::dead_registrations_are_repointed_at_a_terminal_only` |
| T-O7-07 | `tests/update.rs::a_failed_package_manager_stops_the_update` |
| T-O7-08 | `tests/update.rs::the_plugin_channel_asks_claude_code` |
| T-O7-09 | goldens and parity unchanged |
| T-O8-01 | `tests/hooks.rs::claude_code_newer_index_golden` (new golden `claude-code-newer-index.txt`, the version written `{VERSION}`), `session_start::tests::a_newer_index_is_said_once_a_day_and_starts_no_learner` |
| T-O8-02 | goldens and parity unchanged |
| T-O8-03 | `tests/update.rs::doctor_names_versions_that_disagree`, `doctor::tests::an_index_from_a_newer_binary_is_reported_as_newer` |
| T-O8-04 | not built (the shim is `plugin/`'s; see above) |
| T-O8-05 | not touched: no wire file changed (`card_json` is the card's JSON, not a mod answer) |

The fake package manager is a script on `PATH` (`npm`) that installs a script answering
`version` and handing every other command to the test binary; the "installed" adage is a hard
link to the test binary at the channel's place. Store unit tests: version order, the cache's
round trip, the daily rule, plugin paths, the config key and its environment variable.

## Departures from the plan, and why

- **`--rollback`** is the flag (the coordinator's brief and PASS3); the plan's `--back` is its
  visible alias.
- **The steps after the package manager run in the new binary** (`update --finish`), not the one
  that started the update: re-pointing then writes the new version's events, the plugin is staged
  with the new version's module files, and doctor is the new version's. Not in the plan; the
  fallback when the new binary does not know the flag keeps the plan's behaviour.
- **`install::repoint` lives in `update/repoint.rs`**, not `install.rs` (O4's file), and `adage
  delegate on`'s loop in `exec.rs` is unchanged: it rewrites without backups in place, by design
  of that verb. Moving it onto `repoint` (which keeps `.adage-bak`) is its owner's call.
- **`ADAGE_TEST_UPDATE_RUNNER`** was not added: fakes on `PATH` exercise the real code with no
  test-only switch in the binary.
- **Digest verification** compares against a per-binary `SHA256SUMS` release asset, which no
  release publishes yet (see O1/O2 above); until then the channel's own check stands and the output
  says which. Downloading and unpacking the release archive a second time only to compare was not
  worth the cost.
- **`ADAGE_RELEASE_URL` serves the check too**, under `npm/…` and `github/…` beside the downloads,
  rather than two more variables.
- **A failed check keeps the last known release**, so `--check` still says `0.2.0 is out` when
  today's request failed; the plan's `cannot check now` is said when nothing newer is known.
- **`unavailable`** is its own class (npm and GitHub both 404), as §6's private-repository risk
  asks.
- **Tests run with `ADAGE_UPDATE_CHECK=0`** by default (`tests/common::scrub`), so a learner a test
  starts never reaches the real registry; `hooks.rs` (which cannot use `common`) does the opposite
  on purpose, with everything pointed at the counter.
- **`adage update` with nothing newer stages a stale local plugin again**: the plan's "nothing to
  do" otherwise leaves `adage update` unable to be the fix doctor names for a skewed local plugin.

## Live-only, not verified here

- The real npm registry and GitHub API answers (no release exists; the private repository answers
  404, which the check records as `unavailable`).
- `claude plugin marketplace update` / `claude plugin update --json` against the published
  marketplace (checked by hand with Claude Code 2.1.292 against a local directory marketplace: a
  version change updates, the same version answers `up_to_date`; `uninstall` without `--keep-data`
  removes the plugin's data directory).
- dist's install receipt location (`$XDG_CONFIG_HOME/adage-cli/adage-cli-receipt.json`) and the
  release installer under `adage update` (O11).
- Windows: replacing a running `adage.exe` (moved aside, then renamed) is written but not run.

## Other files touched outside the brief's list (smallest edits)

- `crates/adage-hook/src/ctx.rs`: the index load keeps why it found none (`index_newer`); O8 needs
  it and the file is in the hook crate, which no other agent owns.
- `crates/adage-cli/src/main.rs`: `update::run(rest, &paths)` (one argument added).
- `crates/adage-cli/src/cli.rs`: `UpdateArgs` (`--check`, `--now`, `--json`, `--rollback` with
  alias `--back`, `--from … release`, `--branch` now optional, `--yes`, hidden `--finish`); `update`
  moved under "When something is off" beside `doctor` as `update Adage, its hooks and the plugin
  together` (plan `cli.rs:933-948`; the `top_level_help` snapshot changes those two lines); the
  flag-declaration test reads `update/channel.rs` and `update/probe.rs` too and counts aliases.
- `crates/adage-cli/tests/common/mod.rs`: `scrub` sets `ADAGE_UPDATE_CHECK=0`.
- `crates/adage-cli/tests/live_learn.rs`: `config --json`'s every-key-is-default assertion runs
  without that variable (one `env_remove`).
- `crates/adage-exec/tests/no_spawn.rs`: the one entry above.
- `docs/how-it-works.md` (one paragraph under "What is automatic after setup") and
  `docs/live-learning.md` (the `[update]` setting, step 7 "The release check", the two-binaries
  paragraph), as the plan's O5/O8 file lists and the docs rule ask.

## Commands run and results

- `cargo fmt --all -- --check`: clean (exit 0).
- `cargo clippy -p adage-cli -p adage-store -p adage-hook -p adage-app --all-targets -- -D
  warnings`: clean (exit 0).
- `cargo test -p adage-cli -p adage-store -p adage-hook --no-fail-fast`: every suite green
  (adage-cli unit 155, `update` 13, `hooks` 25, `live_learn` 9, `first_run` 11, `zero_config_plugin`
  12, `watch` 4, `onboarding` 11 + 1 ignored, `mod_surfaces` 11 + 1 ignored, and the rest;
  adage-hook 62 + wire 2; adage-store 27). A first run failed in `live_learn` (the
  every-key-is-default assertion meeting `scrub`'s new variable: fixed as above), in `update`
  (another agent's `cargo build` relinked the test binary mid-test: the tests now compare with the
  installed bytes read at their start), `cli::tests::top_level_help` (the moved `update` line:
  snapshot updated) and `watch.rs` (O4's init renumbering, since fixed by O4).
- `cargo test -p adage-exec --test parity --test no_spawn`: parity 25 passed (the new newer-index
  golden and the zero-network test through `adage-exec hook` included), no_spawn 2 passed.
- Goldens: `git status crates/adage-cli/tests/golden` shows one new file,
  `claude-code-newer-index.txt` (blessed alone with `ADAGE_BLESS=1 cargo test -p adage-cli --test
  hooks claude_code_newer_index_golden`); no existing golden changed (`git diff --stat --
  crates/adage-cli/tests/golden` is empty), so the diff is added lines only.
- `cargo test -p xtask`: 24 passed.
- Dry run against this checkout, temp `ADAGE_HOME`, `target/debug/adage update --print`:

  ```text
  adage update · 0.1.0 · checkout

  installed   /home/user/adage-poc/target/debug/adage
  keeps       …/scratchpad/dry/data/previous/0.1.0/ (adage, adage-exec, adage-train)
  runs        git -C /home/user/adage-poc pull --ff-only
  runs        cargo build --locked --manifest-path /home/user/adage-poc/Cargo.toml -p adage-cli -p adage-exec -p adage-train

  printed only: nothing was kept, run or changed
  next  adage update · runs it
  ```

  (exit 0; the data directory stayed empty.)
- A real round trip on copies (`scratchpad/roundtrip.sh`; temp `HOME`, `ADAGE_HOME`,
  `CARGO_HOME`; `target/` never written): the installed `~/.cargo/bin/adage` a copy of the debug
  build, a fake `cargo` on `PATH` that installs a copy of `target/release/adage` (built before this
  chunk's update code, so it answers `--finish` with a usage error: the old binary finished the
  update itself, the fallback path); `adage update --branch main` kept the debug bytes in
  `previous/0.1.0/` (sha256 `cd3a6ae5…` recorded), installed the release bytes (`a63c492c…`),
  verified `adage 0.1.0`, `doctor ● healthy`, `done`; then `adage update --rollback` restored
  `cd3a6ae5…`, the restored binary finished (hand-over exit 0), and `cmp` with the debug build is
  equal: byte for byte.
