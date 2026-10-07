# O4 · O10 notes: `adage init` wires every harness; one front door

Built 2026-10-06/07 on `claude/mods-poc-m98u3o` (from `b6548a2`), beside the O1–O2 and O5/O7/O8
agents. O4 follows the coordinator's amendment (the plugin route first is *automatic*), which the
plan's O4 text now carries (`docs/claude-mod/onboarding-plan.md`: §1.2 step 2, §2.4, the O4 chunk).

## For the agents working beside this one (read first)

- **`crates/adage-cli/src/net.rs`** (new; O5/O7 use it). The curl runner lifted from `models.rs`:
  `download(url, out, max, report)`, `get(url, timeout, max)` (one small body, for `update
  --check`), `classify` (moved here; `models` keeps its own `explain`), `Failed { class, host,
  detail, code }` with `not_found()` (a 404, or a missing `file://`), `release_base()`
  (`ADAGE_RELEASE_URL`, else GitHub's release downloads), `offline()` (`ADAGE_OFFLINE=1` stops
  every fetch before curl starts), `release_target()` (the five targets of `npm/config.mjs`),
  `sha256_file`, `make_executable`, and `fetch_companion(name, version, into, scratch) ->
  Companion::{Placed, NoTarget, NotPublished, Failed, Mismatch}`. Only `https` is followed
  (redirects too); a `file://` URL and `http` to 127.0.0.1/localhost are allowed only for the URL
  asked, for tests and local mirrors. `User-Agent: adage/<version>`.
- **`crates/adage-cli/src/claude_plugin.rs`** (new; O7 calls it). `stage(paths, exe)`,
  `ClaudeCli` (the trait) and `System::on_path()`, `list`, `connect`, `verify`, `refresh(cli,
  paths, exe, cwd)`; constants `MARKETPLACE = "adage-local"`, `PLUGIN_ID = "adage@adage-local"`,
  `SLASH_ADD`, `SLASH_INSTALL`, `BINARIES`, `COMPANIONS`; `plugin_dir(paths)`, `target()`.
- **The staging entry the new binary runs** (for `adage update`): `<new adage> init --restage`
  (hidden). It stages `$ADAGE_HOME/plugin` from *that* binary (its embedded module files and its
  companions), and when Claude Code lists `adage@adage-local` runs `claude plugin marketplace
  update adage-local` and verifies `claude plugin list`; prints `adage init --restage · adage
  <version>` and exits 0; with no local marketplace staged it says so and exits 0. It is not refused
  inside an agent session (a press runs `adage update` there) and writes only under
  `$ADAGE_HOME/plugin`.
- **`adage_store::Paths::plugin()`** → `$ADAGE_HOME/plugin` (one method added after
  `tracks_lock()`; O5 owns the crate). `channel.json` uses O5's `Paths::channel_file()`; the
  Binaries step writes `{ "channel": "release", "version", "at", "bin_dir" }` only when it placed a
  companion.
- **`detect::registrations`** also reports an installed, enabled Claude Code plugin that has not
  started a session yet (from `plugins/installed_plugins.json` and `enabledPlugins`, local over
  project over user settings), as kind `plugin-data` with `binary: None` and location
  `plugins/data/adage-<marketplace>`, so doctor and the card count it as connected. New in
  `detect.rs`: `stable_link(exe)`, `in_plugin_path(exe)`, `plugin_companions(data)` (O1's request,
  used by init's Binaries step), `claude_plugins(repo)`, `ClaudePlugin::data_dir_name()`.
- Edits outside the files I own, each the smallest that works: `main.rs` (`mod claude_plugin; mod
  net;`), `cli.rs` (`InitArgs`: `--hooks <user|project|none>`, hidden `--restage`),
  `crates/adage-exec/tests/no_spawn.rs` (`net.rs`: 2, curl and tar; `claude_plugin.rs`: 1;
  `models.rs`'s entry removed, it spawns nothing now), `crates/adage-cli/src/graph.rs` (`adage` in
  the graph JSON), `crates/adage-cli/src/ui/app.rs` (`m` in Live opens the map), `ui/keys.rs` (the
  `m` row), `crates/adage-cli/tests/watch.rs` (step 5 → 6), `demo/dashboard/serve.py` (the port),
  `crates/adage-cli/AGENT.md` (`claude_plugin.rs`, `net.rs`), `docs/how-it-works.md` (one paragraph
  on what `init` connects).
  `cargo fmt --all` was run once while the other agents were mid-change; it only reformats.
- O8's owner made the requested doctor edit (the unhooked fix is `adage init --yes --hooks user`;
  the optional line names `adage init`); `tests/first_run.rs` follows its words.

## What was built

### O4 · `adage init` wires every harness

**The plugin route first, automatic.** When `adage init` finds Claude Code and no Adage plugin is
installed, it stages the plugin from the binary that runs it and installs it with Claude Code's own
command line (proven headless on Claude Code 2.1.292):

1. `claude_plugin::stage` writes the local marketplace (below). The module files come from the
   binary itself: `crates/adage-cli/build.rs` embeds `plugin/claude-code` minus `dist/`, the mod's
   `*.test.ts(x)`, `fixtures/` and `VALIDATE.txt` (one `include_bytes!` per file, the executable bit
   recorded; about 0.3 MB), so every install channel carries its plugin (a checkout build, `cargo
   install --git`, the release installers, npm's platform binary). Built without the plugin
   directory beside it, the binary carries none and `stage` says so. `ADAGE_BUILD_TARGET` (the
   build's `TARGET`) names `dist/<target>/`; the shim's Linux musl→gnu fallback finds a gnu build.
   `build.rs` re-runs when anything in the plugin but `dist/` changes, so staging binaries there
   never rebuilds `adage`; the unit test `the_binary_carries_the_plugins_files_and_not_its_tests`
   fails if the embedded set and the directory disagree.
2. `claude plugin marketplace add <$ADAGE_HOME/plugin> --scope user --json`, then `claude plugin
   install adage@adage-local --scope user --json` (`--scope local` with `--hooks project`), each
   required to answer `outcome: ok` with exit 0.
3. `claude plugin list --json` must list `adage@adage-local` enabled with `readFromFolder` the
   staged plugin directory; anything else is an error naming what it saw.
4. Every Claude Code settings-file registration `adage install` wrote that the plugin makes
   redundant (all of them with the plugin at user scope; the repository's with `local`) is taken
   out with `install --remove`'s diff and `<file>.adage-bak` (`install::plan_removal` +
   `remove_backed_up`, lifted from `uninstall`, which uses them too); a file `adage install` did not
   write is named for the person to edit.

An installed `adage@adage-local` is staged again in place (a re-run, or `--restage`) and its
marketplace refreshed; an installed `adage@<other>` (the published `adage@adage`) is left to Claude
Code (`Claude Code updates it from the adage marketplace`); a disabled one is named with `/plugin`.
Without `claude` on `PATH`, or when a step refuses (Claude Code's words are quoted), the settings
file is the route (`--hooks user|project`, diffed, backed up) and the typed `/plugin marketplace add
stevenvaughanbooth/adage` + `/plugin install adage@adage` are named. Without `claude` and with a
settings registration already there, init says `registered … · nothing to write` and names the
slash commands for the band and the pane.

**The local marketplace** (`$ADAGE_HOME/plugin/`, `~/.local/share/adage/plugin/` on Linux):

```text
$ADAGE_HOME/plugin/
  .claude-plugin/marketplace.json     {"name":"adage-local","owner":{"name":"Adage"},
                                       "metadata":{"description":…,"version":"0.1.0"},
                                       "plugins":[{"name":"adage","source":"./adage",
                                         "description":<plugin.json's>,"version":"0.1.0",
                                         "repository":…,"homepage":…}]}
  .staged.json                        per binary {from, size, modified_ns, sha256}: no re-hashing
  adage/                              the plugin, adage@adage-local
    .claude-plugin/plugin.json        \
    README.md                          |
    hooks/hooks.json                   | embedded in the adage binary (build.rs)
    hooks/**/*.ts, hooks/draw/*.tsx    |
    scripts/session-start, quiet       | (0755)
    types/index.d.ts                  /
    dist/<build target>/adage         this binary
    dist/<build target>/adage-exec    beside it (else on PATH), when there is one
    dist/<build target>/adage-train   beside it (else on PATH), when there is one
    dist/SHA256SUMS                   "<sha256>  <target>/<name>" per binary staged
```

`claude plugin validate --strict` passes on it (the live test runs it). Claude Code reads a
directory marketplace's plugin in place (its own words on a second install: "loads in place from
…, so edits there take effect at the next session start"), so staging again needs no `claude
plugin update`. It also copies the plugin into `plugins/cache/adage-local/adage/<version>/` at
install, binaries included, and the shim copies each binary into the plugin's data directory
(`plugins/data/adage-adage-local/bin/<name>-<sha>`): three copies of each binary in all, about 1.1
GB for a debug build, tens of MB for a release one.

Staging is idempotent: each file is written beside a temporary name and renamed only when its bytes
differ; a binary whose source size and modification time match `.staged.json` (and whose staged
copy has that size) is not hashed again; files a newer set lacks are removed. A second `adage init`
on an unchanged binary writes nothing (`changed == 0`).

**The trait.**

```rust
pub(crate) trait ClaudeCli {
    /// Run `claude <args>` in `cwd`. `Err` when it could not be started or did not finish in time.
    fn run(&self, args: &[&str], cwd: &Path) -> std::io::Result<Ran>;   // Ran { code, stdout, stderr }
}
pub(crate) struct System { program: PathBuf }   // `claude` on PATH (claude.exe / claude.cmd on Windows); 120 s timeout
```

`list`, `connect`, `verify` and `refresh` take `&dyn ClaudeCli`; the unit tests drive them through
`Stub` (a closure per call line, recording the calls). Through the binary, integration tests put a
fake `claude` shell script on `PATH` that answers as 2.1.292 does and records each call.

**`--hooks user|project|none`.** `adage init --yes --hooks user` connects every harness found for
every repository (`project`: this one; the Claude Code plugin at `local` scope); Codex and OpenCode
through `install::plan` + `apply_backed_up`, each diffed (OpenCode's plugin file named, not dumped),
the Codex `/hooks` approval said once. A harness registered anywhere that applies here is never
registered a second time (`registered … · nothing to write`, or `… · nothing written: a second
registration … would run every hook twice`). `--yes` without `--hooks`, or `--hooks none`, registers
nothing and says so (first_run's wording kept; its `next` is now `adage init --yes --hooks user`).
`--print`, `--hooks` without `--yes`, a pipe, or an agent session print what would happen and run
no `claude`; `--yes` refuses inside an agent session; `--hooks project` outside a repository is an
error.

**The Binaries step** (step 2, before the hooks so the plugin is staged with all three): skipped
when `adage-exec` and `adage-train` sit beside `adage`; the plugin's own adage reports what its
shim staged (`detect::plugin_companions`); a checkout build (O7's `update::channel::checkout`) is
pointed at `cargo build --locked --manifest-path <root>/Cargo.toml [--release] -p <missing>`;
anything else, with `--yes` (or the wizard's yes), fetches each missing one: first
`<ADAGE_RELEASE_URL or GitHub>/v<version>/<name>-<target>.tar.xz.sha256`, then the archive; a
mismatch is refused (`<name>: mismatch: … nothing written`); `tar` unpacks it; the binary is
renamed into place 0755; `channel.json` is written. `offline`/`blocked`/`tls`/a missing release
continue with one line and name `cargo install --locked --git
https://github.com/stevenvaughanbooth/adage adage-exec adage-train`.

**Stable paths.** `install::hook_binary` resolves the binary, then names the plugin's stable link
when `<dir>/adage` is a symlink to its file name in the same directory (`detect::stable_link`), so
a Codex or OpenCode registration written by the plugin's `<data>/bin/adage-<sha>` names
`<data>/bin/adage`.

**The closing screen.** `hooks` (where each harness is registered), `binaries`, `learning` (the
repository, and `conditioning N past sessions in the background` when `--yes` started the first
condition: `autolearn::on_turn_end(…, bootstrap)` when the repository has history), `first line`
(the line the next Claude Code session here opens with, computed as the session-start hook words it
and pinned by a test against the hook's own output), a `Later: adage update …; adage doctor …` note,
and at most two `next` lines (`claude · start a session…`, `codex · open /hooks…`).

The exact screen, from `target/debug/adage init --yes --hooks user` in a fresh git repository with a
fresh `CLAUDE_CONFIG_DIR` and `HOME`, the real `claude` 2.1.292 on `PATH`, no Codex or OpenCode (`$S`
is the scratch directory; `NO_COLOR=1`):

```text
adage init · repo@path-ab8f20026785 · setting up
Adage ships with instincts and forms reflexes from your traces. Each step says what it does as it runs.

It will:
  1. detect the agent harnesses installed here (read-only);
  2. check that adage-exec and adage-train sit beside adage, and fetch any that are missing from this version's release;
  3. connect each harness for every repository: Claude Code through the Adage plugin, Codex and OpenCode through their hook files;
  4. download the optional embedding model for intent families (`adage models fetch`);
  5. list the repositories your Claude Code sessions were in, and condition on this one;
  6. sense this repository's existing traces: which reflexes they would form (read-only);
  7. show which instincts (Adage's built-in reflexes) would bind in this repository, from its files alone, and bind them.

1  Harnesses
  claude-code  found: `claude` on PATH (/opt/node22/bin/claude); $S/cfg exists
  opencode     not found
  codex        not found

2  Binaries
  adage, adage-exec and adage-train are in /home/user/adage-poc/target/debug · nothing to do

3  Hooks
  claude-code  the Adage plugin, staged from this adage in $S/data/plugin
               ran claude plugin marketplace add $S/data/plugin
               ran claude plugin install adage@adage-local --scope user
               ✓ installed and enabled · Claude Code reads it from $S/data/plugin/adage

4  Embedding model
  model        potion-base-8m-i8@b9c0e329a2ce (bundled in this binary) · nothing to do
               adage models fetch installs it at full precision; adage models fetch potion-code-16m-v2 adds the code model

5  Repositories to condition on
  [x] $S/repo  no Claude Code sessions yet · this repository
  Adage conditions on the one ticked (learn.repos); adage init at a terminal picks others.
  Sessions in other repositories are recorded, not read.

6  Sense this repository's traces

adage sense · 0 traces · 0 repositories · read-only
source      Claude Code history in $S/cfg/projects, recorded in $S/repo (--all reads every repository's)

No traces to sense in that source.

next  adage sense --transcripts <file>… · or --claude-projects <dir>, --codex-sessions <dir>

7  Instincts (day 0)
  8 instinct packs on, from the adage marketplace built into Adage; none applies here (adage packs to see or turn them off)
  Nothing in this repository's files matches an instinct.

hooks       registered: claude-code (the Adage plugin)
binaries    adage, adage-exec, adage-train in /home/user/adage-poc/target/debug
learning    repo@path-ab8f20026785 · after each session
first line  your next Claude Code session here opens with
            adage: learning from this repo as you work; its first reflexes form in a few sessions. Nothing runs without you.

Later: `adage update` updates Adage, its hooks and the plugin together; `adage doctor` if anything looks wrong.

next  claude · start a session; Adage says what it is doing on its first line
```

Then, in the same world: `claude plugin list` prints `adage@adage-local · Version: 0.1.0 · Read
from: $S/data/plugin/adage · Scope: user · Status: √ enabled`; `claude plugin validate --strict
$S/data/plugin` prints `√ Validation passed`; `claude -p 'Reply OK' --debug-file …` answers `OK`,
and its debug log has each of the plugin's hooks once (`Hook SessionStart:startup (SessionStart)
success`, `adage@adage-local classic.UserPromptSubmit settled`, `adage@adage-local classic.Stop
settled`, `SessionEnd:other [${CLAUDE_PLUGIN_DATA}/bin/hook hook claude SessionEnd] completed with
status 0`: 1 each) with the opening line as its `systemMessage` (`adage: learning from this repo as
you work; its first reflexes form in a few sessions. Nothing runs without you.`, the line init
quoted); the shim linked `plugins/data/adage-adage-local/bin/{adage,adage-exec,adage-train,hook}`;
`adage doctor` passes every check (`hooks: claude-code  registered by the Adage plugin`, `claude-code
plugin binary  … matches this binary`). Running init again: `claude-code  the Adage plugin
(adage@adage-local) is installed and up to date with this adage` (or `… staged again from this
adage, for your next session` when a binary beside it was rebuilt in between, which another agent's
build did once here); `adage init --restage` printed `adage init --restage · adage 0.1.0`, `staged
… · already up to date`, `binaries  adage, adage-exec, adage-train`.

**The wizard** (`ui/wizard.rs`): Harnesses, **Binaries** (new; `Fetch them · each verified against
the release's checksums` as a job, or Continue), Hooks per harness. For Claude Code: `The Adage
plugin (recommended) · adage installs it with claude plugin` (a job running
`claude_plugin::connect`, then the settings-file removal), or without `claude` on `PATH` `The Adage
plugin (recommended) · in Claude Code: /plugin marketplace add …, then /plugin install adage@adage`
(records the step, writes nothing); then the settings file (this repository, every repository) with
its diff; beside an installed plugin, `Take Adage's hooks out of <file> · the plugin runs each one
already` (its diff) or `Keep both`; `Skip claude-code`. A failed plugin route shows Claude Code's
words and offers what is left. Then the model, repositories, the first condition, auto-condition,
and the closing screen with at most two `next` lines and the `Later:` note. Codex and OpenCode keep
their steps as they were.

### O10 · One front door

- **The card** (`status_card.rs`): `next` is at most two lines, each one command or `/adage` verb:
  `adage update` first when the release cache knows a newer version (O5's `update::card_json`),
  `adage on` where Adage is off, `adage review` (`/adage review` with `--for-session`) when something
  waits, else `adage ui` (`/adage watch`); `adage map` and `adage doctor` left the list. A newer
  release is an `update` row (`0.2.0 is out (you have 0.1.0) → adage update`) and `--json`'s
  `update` object (the line O5 asked for), never the lamp. Each harness found and not connected gets
  one row, `found · adage init connects it`. Before any agent is connected: `claude-code  in Claude
  Code: /plugin marketplace add stevenvaughanbooth/adage then /plugin install adage@adage` / `or,
  from a terminal: adage init`, a row per other harness, and `next  adage init` + `adage doctor`.
- **The wizard's steps and one screen at the end:** above.
- **The map** (`web/map`): the graph JSON gains `adage: { version, latest, channel }` (added where
  the binary serves it, `crate::graph::build`; `channel` is O7's `update::channel::detect`).
  `src/lib/setup.ts` words every next step by channel: `channel: "plugin"` gets `In Claude Code:
  /adage review, /adage health; the pane's Explore in browser opened this page.` and four `/adage`
  steps, never `adage ui` or "in your terminal"; anyone else (or an older binary) `In a terminal:
  adage ui, adage review, adage update.` with the steps `adage init`, `adage ui`, `adage review`,
  `adage update`. The Setup view, `CommandButton` (Health's and the Inspector's command buttons),
  the palette's hints, the first-run line and the Inspector's execution-review button read it; the
  top bar shows `adage <version>` and, when a newer one is known, `0.2.0 is out · adage update`
  (`/adage update` for a plugin user) in plain ink. `web/map/README.md` is new. The embedded page
  is rebuilt (`pnpm assets`; deterministic: a second build gives the same bytes).
- **The ports:** `adage map` keeps 8765; the demo dashboard (`demo/dashboard/serve.py`) moves to
  8766 (`DEMO_PORT` still overrides); `docs/usage.md` follows.
- **`adage ui`:** Live's `m` opens the map in the browser (`Cmd::OpenWeb`), as `adage watch`'s does;
  Home's "Connect your agent" names `adage init`, and when a newer Adage is known Home's first line
  is `adage 0.1.0 · 0.2.0 is out · adage update updates it, its hooks and the plugin` (plain ink).

## Tests (ids from the plan)

| Id | Where |
| --- | --- |
| T-O4-01, T-O4-02 | `tests/onboarding.rs::init_yes_hooks_user_connects_every_harness` (the three user files, `.adage-bak`, doctor green for each; a second run writes nothing and says where) |
| T-O4-03 | `tests/onboarding.rs::a_plugin_registration_is_not_registered_again` |
| T-O4-04 | `tests/onboarding.rs::init_yes_hooks_refuses_inside_an_agent_session`; `tests/first_run.rs::init_prints_inside_an_agent_session_but_yes_refuses_there` |
| T-O4-05 | `src/ui/tests.rs::wizard_offers_the_plugin_first` (snapshot `wizard-plugin-route`, new), `wizard_walks_every_step`, `wizard_next_commands_keep_their_columns` |
| T-O4-06 | `tests/onboarding.rs::the_binaries_step_fetches_verified_companions` (a `file://` release built with `tar -cJf`; one altered byte refused; offline) |
| T-O4-07 | `src/init.rs::tests::the_binaries_step_is_skipped_with_both_companions_beside` |
| T-O4-08 | `tests/onboarding.rs::a_registration_written_by_the_plugins_binary_names_its_stable_link` |
| T-O4-09 | `tests/onboarding.rs::the_card_before_anything_is_connected` (snapshot `card-not-connected`, new) |
| T-O4-10 | `tests/onboarding.rs::the_plugin_route_installs_verifies_and_leaves_one_registration`; `src/claude_plugin.rs::tests::{connecting_adds_installs_and_verifies, staging_writes_the_marketplace_and_is_idempotent}` |
| T-O4-11 | `tests/onboarding.rs::when_claude_refuses_the_settings_file_is_the_route`; `claude_plugin::tests::a_refusal_names_the_step_and_claudes_words` |
| T-O4-12 | `tests/onboarding.rs::print_runs_nothing_and_writes_nothing` |
| T-O4-13 (live) | `tests/onboarding.rs::live_init_installs_the_plugin_into_claude_code` (`#[ignore]`; `-- --ignored live_`) |
| — | `claude_plugin::tests::the_binary_carries_the_plugins_files_and_not_its_tests`; `net::tests::*`; `init::tests::*`; `tests/onboarding.rs::{the_closing_screen_quotes_the_next_sessions_first_line, init_yes_without_hooks_connects_nothing_and_says_so}` |
| T-O10-01 | `tests/mod_surfaces.rs::the_card_as_json` (snapshots `card-text`, `card-json` re-accepted: only the `next` rows O10 trims, and `"update": null`); `tests/onboarding.rs::the_card_before_anything_is_connected` |
| T-O10-02 | `web/map/src/lib/setup.test.ts` (`channel: plugin` has no `adage ui`, no "in your terminal"; `npm` and absent get the terminal commands) |
| T-O10-03 | `web/map/e2e/map.spec.ts` "the served graph names the binary's version, and the top bar shows it"; the first-run and palette Setup tests read the terminal copy |
| T-O10-04 | `web/map/src/lib/setup.test.ts` "ports" (8765 vs 8766, read from both sources) |
| T-O10-05 | not run: T-U-01's reading part needs eight readers (live-only) |

Snapshots re-accepted, by name: `mod_surfaces__card-text` and `mod_surfaces__card-json` (the `next`
rows O10 trims, and `"update": null`; `git diff` shows only those lines). New:
`onboarding__card-not-connected`, `adage__ui__tests__wizard-plugin-route`. No golden changed.
`adage__cli__tests__top_level_help` fails on O7's new `update` description; it is O7's to accept.

## Departures from the plan, and why

- **The amendment** (the plugin route automatic; module files embedded; `adage-local`) replaces the
  plan's "first choice reads `/plugin …`". The plan's O4 text says so.
- **Binaries before Hooks.** The plan lists the Binaries step after Hooks; it runs first so the plugin
  is staged with the companions it fetched. `init`'s printed steps are renumbered (1 Harnesses, 2
  Binaries, 3 Hooks, 4 model, 5 repositories, 6 sense, 7 instincts); `first_run.rs` and `watch.rs`
  follow.
- **`adage` in the graph JSON is added in `adage-cli`**, where the binary knows its path and channel,
  not in `adage_app::graph::build`: the app layer's contract test and the e2e fixtures stay as they
  are, and the web map treats the field as optional.
- **`init --yes` starts the first condition in the background** when the repository has history
  (PASS3's "start the first `condition` in the background"), through the hooks' own trigger.
- **The plugin's install scope for `--hooks project` is `local`** (`.claude/settings.local.json`,
  never committed), matching `adage install`'s project scope; a local-path marketplace in a committed
  `.claude/settings.json` would break teammates.
- **`init::origin` reuses O7's `update::channel::checkout`** instead of a second checkout test.
- **TUI Home** (O6's row) shows the newer version as text with the command; running `adage update` as
  a Job from Home and the palette's `Update Adage` are O6/O9's and not built here. The `adage ui`
  palette's "Connect Claude Code" still writes the settings file (`palette.rs` is not mine).
- **The map's `CommandButton`** for a plugin user says "Do this from Claude Code with /adage" rather
  than a verb per command: the button gets an arbitrary `adage …` command and the map does not
  translate commands into verbs.
- **`docs/harnesses.md`** (named by O4) is not edited: the brief limited my documents to
  `docs/usage.md`, `docs/zero-config.md` and CHANGELOG; its `adage install` paragraphs stay true.

## Live-only, or not verified here

- The release fetch against a real GitHub release (none exists: `git tag` is empty); the code runs
  against a `file://` release laid out as dist lays it out.
- macOS (the build target `aarch64-apple-darwin` names the shim's directory directly) and Windows
  (`claude.cmd`, `.zip` archives, `adage.exe`): compiled for, not run.
- T-O10-05 (reading test).
- The wizard's interactive run end to end (its state machine and frames are unit-tested; its jobs
  call the functions the `--yes` path runs, which the integration and live tests cover).

## Commands run and results

All in `/home/user/adage-poc`, debug builds only (`plugin/build.sh` was not run).

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy -p adage-cli -p adage-app --all-targets -- -D warnings` | clean |
| `cargo test -p adage-cli --no-fail-fast` | 33 test binaries, 371 passed, 0 failed, 2 ignored (the live test, and mod_surfaces' existing ignored case) |
| `cargo test -p adage-cli --test onboarding -- --ignored live_` (real `claude` 2.1.292, fresh `CLAUDE_CONFIG_DIR`) | 1 passed (23 s): plugin listed enabled and read from `$ADAGE_HOME/plugin/adage`, `validate --strict` passes, each hook once in a headless session |
| `cargo test -p adage-exec --test no_spawn` | 2 passed |
| `cargo test -p xtask` | 24 passed |
| `cargo test -p adage-store` | 27 passed |
| `pnpm typecheck` (web/map) | clean |
| `pnpm test` (web/map, vitest) | 16 files, 71 tests passed (setup.test.ts included) |
| `pnpm assets`, twice (web/map) | page brotli 248.3 KB of 400 KB; the second build byte-identical to the first |
| `pnpm test:e2e` (web/map, Playwright + axe, against `target/debug/adage map`) | 51 of 52 passed in the full run; the failure ("how it works steps through every chapter", the lazy three.js chunk not loaded in 15 s while other agents' builds loaded the machine) passed alone (`-g "how it works steps"`) and in the first full run |
| `pnpm drift` | not run green here: it compares against git, and the rebuilt `crates/adage-cli/assets/map/` and `src/map/assets_list.rs` are uncommitted (the coordinator commits them); the build is deterministic, so it passes once they are |
| manual `adage init --yes --hooks user` with the real `claude` | the screen above; second runs idempotent; `--restage` as above |
