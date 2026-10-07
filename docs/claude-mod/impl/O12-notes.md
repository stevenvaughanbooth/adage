# O12 notes: the words match the code and the bar

Written on `claude/mods-poc-m98u3o` at `d29f5ce`, uncommitted, beside the agents that own the
plugin's hooks, `crates/` and `cli.rs`. The bar (the owner's words): a newcomer installs once, runs
`adage init` once and `adage update` later, and nothing else. The plan is
`docs/claude-mod/onboarding-plan.md` (§1, §5, §7, chunk O12). This pass resumed an agent cut off
by a container restart; its partial edits were read, kept where sound, and finished.

## What changed, per file

- **`README.md`.** The banner says no release is tagged and the install is from source (no list of
  unpublished channels). Install is three numbered steps: (1) `cargo install --locked --git
  https://github.com/stevenvaughanbooth/adage adage-cli adage-exec adage-train`, with what the two
  companions are; under "Once v0.1.0 is tagged", the two routes exactly as `docs/release.md` names
  them (`/plugin marketplace add stevenvaughanbooth/adage`, `/plugin install adage@adage`; the
  curl installer line, then `adage init`), npm and Homebrew not named; (2) `adage init` in one
  paragraph in the person's words (plugin installed through `claude plugin`, an older registration
  taken out, Codex and OpenCode files, every file shown first and kept as `.adage-bak`, learning on,
  one closing screen; `--yes --hooks user`, `--print`); (3) `adage update` in one paragraph (where
  it is announced, what it updates, kept binaries, `--rollback`; before v0.1.0 `--branch main`).
  "Quickstart" became "What you see", with the card's `update` row. "Safe by default" says the one
  network request (the learner's daily check, `update.check off`) and that no hook touches the
  network. No "both binaries", "not published yet: brew" or `plugin/claude-code-exec` sentence
  remains.
- **`docs/usage.md`.** A new first section, "Installing and updating": Install (source today; the
  two v0.1.0 routes; npm only if switched on; from a checkout), "First run: `adage init`" (its seven
  steps, the closing screen, `--yes --hooks user|project`, `--print`, re-runs), a **Known, until a
  fix lands** paragraph (below), "If you prefer to do it by hand" (the slash commands, the
  settings file, Codex and OpenCode, the `--plugin-dir` route with its `CLAUDE_PLUGIN_DATA`
  caveat, `adage condition`), "Updating" (the check, the transaction, before the first release, the
  published plugin, version drift) and "Removing Adage". The old "Other ways to install" (curl,
  PowerShell, brew, npm block) and the old Updating section are folded in; Homebrew is gone from
  the channel list. The Commands list has `init` and `update`; the Claude Code section drops the
  `plugin/build.sh` steps for end users; `update.check` is in the settings table; the "never
  without you" paragraph has the true network sentence.
- **`docs/zero-config.md`.** The status block and the distribution note (2026-10-07: source today,
  the marketplace and curl installer with v0.1.0, npm if switched on, Homebrew off); persona (b) is
  `cargo install` today; three binaries in the plugin; the network sentence (one daily check from
  the learner, `update.check = off`, never from a hook); the settings-file rule names `init`,
  `install` and `update`; the audit's "steps today" marked as superseded; the no-binary message is
  the shim's actual one; the README rewrite item marked built.
- **`docs/harnesses.md`.** The install row and each harness's install paragraph lead with `adage
  init`; the Claude Code paragraph describes the one plugin with three binaries and the stable
  `${CLAUDE_PLUGIN_DATA}/bin/adage` link a Codex or OpenCode registration written by the plugin's
  own binary names; `adage install …` kept as the by-hand route.
- **`docs/first-reflex.md`.** The install line is the three-crate `cargo install`; what `init`
  does (plugin installed for you); `adage-exec` comes with the install; removal names
  `claude plugin uninstall adage@adage-local` and links Removing Adage.
- **`plugin/claude-code/README.md`.** "Installing and updating": from a terminal today (`adage
  init`, the local marketplace `adage-local`), the known mod defect under it (below), in Claude Code
  from v0.1.0, the developer's `--plugin-dir` route with its caveat, removal; the stable `bin/adage`
  link; one plugin, no alias.
- **`npm/cli/README.md`.** What the binary does (advises; execution opt-in by `adage delegate on`
  or an answer in the session), the three binaries in the platform package, `npm install -g`, then
  `adage init` and `adage update` (and `--rollback`).
- **`evals/launch/README.md`.** Limitation 6 and the release-gate rows (distribution, npm name,
  marketplace branch, plugin users and the CLI) gain dated "Since" notes: Homebrew off the release,
  `@adage-sh/cli`, one plugin with three binaries, `adage@adage-local` before the release, the
  stable link. The original assessments stay as written.
- **`docs/claude-mod/README.md`.** `local-setup.md` described as build, `adage init`, learn,
  grant, verify; the onboarding plan named; the live runs and where their results are; the bar
  from a terminal is two commands.
- **`docs/claude-mod/local-setup.md`.** The short path first: build the three binaries, then
  `adage init` (what it stages and runs, `--print`, `--yes --hooks user`, re-runs, `adage update`
  and `--rollback`); the known defect with the export that works; `plugin/build.sh` + `--plugin-dir`
  as the developer's route, with the `adage-inline` data directory and the export; the published
  marketplace from v0.1.0. The open question about `CLAUDE_PLUGIN_DATA` is now "Answered live".
  Section 3 says `adage init` offers the settings-file removal and what the live runs showed (each
  hook settled once). Section 4 installs with `adage init` into the fresh config dir and starts
  `claude` without `--plugin-dir`; the opening line is marked seen at the engine, not yet at
  `InfoNotice`; the data directory names for the three routes and a `grep -c '$.process.run
  (adage)'` check that the mod runs its binary. Section 8's fixes name `adage init` again (or
  `plugin/build.sh` on the developer's route) and the export. Section 10 is now "What has run live,
  and what has not" (answered: the variable, the data directories, the opening line, hooks once,
  the links, `TurnStep` and yielded steps, the thread-state 404; open: ordering, startup budget,
  drawing, press, acting beyond run 2, Windows). Section 11 removes the `adage init` plugin with
  `claude plugin uninstall adage@adage-local` and `claude plugin marketplace remove adage-local`.
- **`docs/claude-mod/evaluation-plan.md`.** The opening says what has run live (§5 twice, one §4.3
  probe, §2.3) and links the results. §0 builds the three binaries, exports `CLAUDE_CONFIG_DIR`, and
  runs `target/release/adage init` (the plugin as `adage@adage-local`), with `plugin/build.sh` +
  `--plugin-dir` as the developer's and the demo runner's route, never both, and the
  `CLAUDE_PLUGIN_DATA` export per route. §2.1 starts `claude` with the installed plugin, then the
  developer's route, and records what live runs have shown. §3 says three binaries. §5 says the
  runner uses its own `--plugin-dir` copy (disable `adage@adage-local` or give the runner its own
  `CLAUDE_CONFIG_DIR`) and summarises the two runs.

## The two `--help` sentences (for the owner of `crates/adage-cli/src/cli.rs`)

Not edited in this pass. `cli.rs` in the tree (uncommitted, lines 47–50 and 125–128) already carries
these; if they are kept, nothing is left to do:

- `init`: "Set Adage up once: connect your coding agents (Claude Code through the Adage plugin,
  Codex, OpenCode) and start learning from their past sessions." After help: "At a terminal it asks
  before each step; `--yes --hooks user` takes them all without a question, and `--print` only shows
  them. Later, `adage update` keeps everything current."
- `update`: "Update Adage when a newer version is out: its binaries, hooks and Claude Code plugin
  together, the way you installed it, with a way back." After help (unchanged): "Checks for a newer
  release first (--check only checks). Before anything is replaced the running binaries are kept in
  $ADAGE_HOME/previous/; --rollback puts them back."

One more for that owner: `UpdateArgs.from` still accepts `brew` (`value_parser = ["source", "npm",
"brew", "release"]`) while Homebrew is off the release; the docs no longer name it.

## A live finding: the mod is inert under the plugin `adage init` installs

Checked in this pass because the docs were about to promise the band after `adage init`. In a
fresh world (temporary `HOME`, `ADAGE_HOME`, `CLAUDE_CONFIG_DIR`; the host session's variables
removed as `demo/run.py` does; Claude Code 2.1.292; the tree's `target/debug` binaries):

1. `adage init --yes --hooks user` in a new git repository: `hooks registered: claude-code (the
   Adage plugin)`, the closing screen as documented. `installed_plugins.json` records
   `adage@adage-local` with `installPath` `<config>/plugins/cache/adage-local/adage/0.1.0`, but
   `claude plugin list` says `Read from: <ADAGE_HOME>/plugin/adage`; a probe plugin from a local
   marketplace showed the same, and its cache directory held only `.claude-plugin/plugin.json`.
   So Claude Code runs a local-marketplace plugin from its source directory.
2. `claude -p 'Reply OK' --debug-file q1.log`: `OK`; the SessionStart hook's `systemMessage` is the
   opening line (`adage: learning from this repo as you work; …`); every module hook settles once
   (`hooks module adage@adage-local classic.SessionStart settled`, …); the shim linked
   `plugins/data/adage-adage-local/bin/{adage,adage-exec,adage-train,hook}`. But there is **no
   `$.process.run (adage)` line**: the mod never ran its binary, so it is inert (no band, no
   `/adage`, no `Standing`, no `TurnStep`).
3. The same with `CLAUDE_PLUGIN_DATA=<config>/plugins/data/adage-adage-local` exported: 8
   `$.process.run (adage)` lines (`bin/hook` probes and asks, `bin/adage` reads).

Cause (reading `plugin/claude-code/hooks/binary.ts` `dataDir` and `bridge.ts`
`dataDirCandidates`): the variable is not in the engine's environment; the root is not under
`plugins/cache/…`, so `dataDirFromRoot` gives nothing, and the fallback candidates are only
`<name>-inline`. The fix belongs to the mod's owner: also try `<config>/plugins/data/adage-adage-local`
(the marketplace name is `claude_plugin::MARKETPLACE`, and the root's parent holds the local
marketplace's `.claude-plugin/marketplace.json`), checked the same way (the shim's `bin/hook`
exists there). Until then the docs say so and give the export (`docs/usage.md` "Known, until a fix
lands", `plugin/claude-code/README.md`, `local-setup.md` §2 and §4, `evaluation-plan.md` §0). The
two debug logs are in the session scratchpad (`o12-q1-noenv.log`, `o12-q2-withenv.log`); the
worlds were deleted. Spend: two one-line headless sessions.

## Sweep

`grep -rn "claude-code-exec\|both binaries\|brew install\|not published yet" README.md docs/
plugin/claude-code/README.md npm/ evals/launch/`: no hit left in the files this pass owns (the
last, in `evals/launch/README.md`, was the substring of "Homebrew installers"; reworded). Hits left
for others, by file:

- History and plans, which describe what was true or planned then (leave, or annotate): `docs/Adage
  — Architecture.md:969`, `docs/Adage — Implementation Plan.md:57,349`,
  `docs/claude-mod/architecture.md:228,895,904,1137`, `docs/claude-mod/implementation-plan.md`
  (nine, A8's history), `docs/claude-mod/onboarding-plan.md` (six, its own tests and history),
  `docs/claude-mod/test-plan.md` (six), `docs/tracks/track-c-compiled-skills.md:230,437`,
  `docs/release.md:112` (the checklist's own wording), every `docs/claude-mod/impl/*-notes.md` and
  `docs/claude-mod/poc/*.md` hit (records of runs).
- Worth fixing: `docs/claude-mod/ux-design.md:581` quotes the no-binary message as `install with brew
  install … or npm i -g …`; the shim says `reinstall with /plugin install adage@adage, or npm i -g
  @adage-sh/cli` (`plugin/claude-code/scripts/session-start:117`).
  `docs/claude-mod/evaluation-results.md:32` "(both binaries, digests checked)" is a record of what
  run 1 staged (true then: `adage-train` was not staged by the runner) and can stay.
  `npm/test/launcher.test.mjs:99` is a comment ("one plugin, both binaries").

## Checks run

- Relative links and anchors in the eleven files: a script over every `](…)` link (file exists;
  for a `.md` target, the anchor is one of its headings' slugs): 0 bad. Anchors other files use into
  these pages (`usage.md#installing-and-updating`, `#updating`, `#removing-adage`, `#claude-code`,
  `#commands`, `README.md#install`, …) all resolve.
- Commands: every backquoted `adage <verb>` in the eleven files is a verb of `target/debug/adage
  --help` or hidden but present (`hook`, `on`), except `connect`, `learn`, `survey` (named as
  removed or struck) and `adage is not answering` (the mod's own message). The flags written
  (`init --yes --hooks user|project`, `--print`; `update --check --now --json --rollback --branch
  --from --print`; `install --write --remove --scope`) are in `cli.rs` and the binary's `--help`;
  `update.check` and `ADAGE_UPDATE_CHECK` are in `crates/adage-store/src/config.rs`;
  `claude plugin disable`, `uninstall` (removes the data directory unless `--keep-data`) and
  `marketplace remove|update` are in Claude Code 2.1.292's help; `plugin/build.sh`,
  `scripts/check-plugin.mjs` exist.
- Not run: cargo (as briefed); no other test touches these pages except `cargo test -p xtask`'s
  hygiene, whose rules this pass did not change.
