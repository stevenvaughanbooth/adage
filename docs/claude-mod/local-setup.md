# Local setup: your own machine's Adage with the Claude Code mod

A guide for the repository's owner, on a macOS or Linux machine that has run Adage's command hooks before, from pulling the branch to the first prediction. Everything below is from the tree on `claude/mods-spike-doc-m98u3o`; where the five readers disagreed or left a question open, the guide says so in one line instead of guessing. The full evaluation protocol is `docs/claude-mod/evaluation-plan.md`; this is the shorter path to a working machine.

The short path is two commands after the build: `target/release/adage init` once (it installs the plugin into your Claude Code, wires every hook once and turns learning on), and `adage update` later. Sections 1 to 3 are that path; loading the plugin straight from the checkout with `--plugin-dir` is the developer's route, at the end of section 2.

What has run live so far (headless `claude -p`, Claude Code 2.1.292, in a container; `docs/claude-mod/evaluation-results.md`): the evaluation's §5 comparison twice and one finish-line probe, all under `--plugin-dir`, and one `adage init` install followed by a headless session. The opening line reaches the engine, every hook settles once, and the mod predicts; but the mod finds its binary only with `CLAUDE_PLUGIN_DATA` exported, under either route, until a fix lands (section 2). Nothing has been drawn in an interactive session yet. Section 10 lists what remains and what to write down.

## 0. Prerequisites and versions

1. Rust, current stable. The repository pins no toolchain (no `rust-toolchain` file, no `rust-version` in any `Cargo.toml`); CI builds with stable (`.github/workflows/ci.yml:61`). You also need a C compiler (`tree-sitter-bash` compiles C through the `cc` crate): Xcode Command Line Tools on macOS, `build-essential` on Linux. The embedding model is `include_bytes!`'d from `crates/adage-intent/bundled/`, so the build downloads nothing.
2. Claude Code 2.1.288 or later. The mod API was validated against 2.1.288 (`plugin/claude-code/hooks/plugin.ts:53-54`, `TESTED = '2.1.288'`); CI pins that version for `claude plugin validate` (`.github/workflows/ci.yml:264-267`). A newer CLI is fine for live use; only the `VALIDATE.txt` diff needs the pinned one (section 0, step 5).
3. Node 22 or later: every CI job that needs Node uses 22 (`.github/workflows/ci.yml:91-93,257-259,296-298,330-331`) and the evaluation plan asks for 22 or later (`docs/claude-mod/evaluation-plan.md:26`). Python 3.9 or later, which the evaluation plan asks for (`docs/claude-mod/evaluation-plan.md:27`) for its Python scripts: `demo/run.py`, `demo/report.py`, `demo/replay_hooks.py`, `evals/live/dev_v9.py` and the `cargo xtask test python` suites, none of which need third-party packages; CI also runs `python3` for the `evals/live` unittest suites and selfchecks (`.github/workflows/ci.yml:103-110`).
4. Check them:

```sh
rustup show
claude --version      # 2.1.288 or later
node --version        # 22 or later
python3 --version     # 3.9 or later
```

5. Optional, only for the `VALIDATE.txt` diff: install the pinned CLI beside your own. The pin is the one in `.github/workflows/ci.yml:267` (2.1.288; the comment at `ci.yml:264-265` says to bump it together with `plugin/claude-code/VALIDATE.txt`), so read that line rather than this guide for the version. A newer validator adds lines of its own (on the build machine, 2.1.292 added extra "gating hook without .catch" notes for the unchanged `register.ts`), so a diff against a newer CLI is noise, not a finding (`docs/claude-mod/evaluation-plan.md:73-79`).

```sh
npm install --prefix /tmp/cli288 @anthropic-ai/claude-code@2.1.288
```

## 1. Pull and build

1. Check out the PR branch. It carries the whole implementation and the latest `origin/main` (`63cbc77`, #31). Note: `docs/claude-mod/evaluation-plan.md:16,23` still names the older `claude/mods-poc-m98u3o`; use the spike-doc branch, which merges it.

```sh
git fetch origin claude/mods-spike-doc-m98u3o
git checkout claude/mods-spike-doc-m98u3o
```

2. Build the three binaries. The plugin ships all three (`adage`, `adage-exec`, `adage-train`), and `adage init` stages them from beside the `adage` you run.

```sh
cargo build --release --locked -p adage-cli -p adage-exec -p adage-train
```

3. `--locked` means `Cargo.lock` must match `Cargo.toml`; a `Cargo.toml` edit without a lock update fails the build.

4. Why release: the shim copies and SHA-256-hashes each staged binary at first start, inside SessionStart's 10 s hook timeout (`plugin/claude-code/hooks/hooks.json:4`); `[profile.release]` is opt-level 3, LTO, stripped (`Cargo.toml:30-39`), giving the ~10 MB binaries; debug builds are hundreds of MB and keep debug-only knobs live (`ADAGE_TEST_FOLLOW_POLL_MS`, `ADAGE_TEST_FAULT`).

5. Your terminal `adage`. The plugin's hooks never need `adage` on `PATH` (section 2), but you do for `condition`, `review`, `delegate`, `doctor`. Use the checkout's binary, `target/release/adage`, with `adage-exec` and `adage-train` beside it (a terminal `adage delegate on` needs `adage-exec` there, `crates/adage-cli/src/exec.rs:252-266`; `adage train` needs `adage-train`). The rest of this guide writes `adage` and assumes one of:

```sh
# a) an alias for this shell
alias adage="$PWD/target/release/adage"

# b) or install all three onto PATH (~/.cargo/bin), side by side
cargo install --locked --path crates/adage-cli
cargo install --locked --path crates/adage-exec
cargo install --locked --path crates/adage-train
```

`adage init` stages the plugin from whichever `adage` runs it, so run it, and later `adage update`, with the same one you use at the terminal; `adage doctor`'s `claude-code plugin binary` check warns when the plugin's staged `bin/adage` is another build (`crates/adage-cli/src/doctor/mod.rs`).

6. Pulling or switching branches never updates what the plugin runs: after a rebuild, run `adage init` again (section 2), which stages the new build for the next session.

7. `target/` is gitignored, and so is `plugin/claude-code/dist/` (`.gitignore`), which only the developer's route (section 2) writes.

8. Remove or upgrade any older `adage` that shares your Adage home. The new binary writes index format 1; an older binary cannot read a format-1 index that holds a `skill` step, serves nothing, and its `adage doctor` fails with `is newer than this binary … an adage on PATH and the Claude Code plugin's must match` (`crates/adage-tracks/src/index.rs:20-36`; `crates/adage-cli/src/doctor/mod.rs:362-370`).

## 2. Install the plugin into your Claude Code

**The short path: `adage init`.** At a terminal (not inside Claude Code), in the repository you will use:

```sh
adage init
```

It finds Claude Code (and Codex or OpenCode, if installed) and, on a yes, installs the plugin for you: it stages it from the `adage` that runs it, the module files carried inside the binary and the three binaries beside it with their `dist/SHA256SUMS`, as a one-plugin local marketplace named `adage-local` under `$ADAGE_HOME/plugin/`, then runs `claude plugin marketplace add <that directory>` and `claude plugin install adage@adage-local` (user scope) and checks `claude plugin list`, which then shows `adage@adage-local` enabled and read from `$ADAGE_HOME/plugin/adage` (`crates/adage-cli/src/claude_plugin.rs`). Beside it, it offers to take the settings-file registration you made before out (section 3). Then it turns learning on for this repository, binds its instincts, offers a first conditioning of its history, and ends on one screen with the line your next session opens with. `adage init --print` shows every step without doing any; `adage init --yes --hooks user` takes them all without a question. Running it again is safe: it stages the plugin again from the binary that runs it and rewrites nothing else in place. Later, `adage update` (from a checkout: `git pull --ff-only`, the same `cargo build`, then the plugin staged again) keeps all of it current, with `adage update --rollback` to go back. The plugin's manifest is `plugin/claude-code/.claude-plugin/plugin.json` (name `adage`, version 0.1.0, two `userConfig` rows: `band` boolean default `true`, `pane` string `auto|manual` default `manual`), hooks in `plugin/claude-code/hooks/hooks.json`, mod module `plugin/claude-code/hooks/register.ts`.

**Known, until a fix lands** (live, 2026-10-07, Claude Code 2.1.292, a fresh `CLAUDE_CONFIG_DIR`): after `adage init`, a headless session ran every command hook once and carried the opening line, and the shim linked `bin/adage`, `bin/adage-exec`, `bin/adage-train` and `bin/hook` in `$CLAUDE_CONFIG_DIR/plugins/data/adage-adage-local/`; but the debug log had no `$.process.run (adage)` line: the mod never ran its binary, so it is inert (no band, no `/adage`, no questions, no predictions). Claude Code reads a local-marketplace plugin from its source directory, not from `plugins/cache/…`, so the mod's derivation of its data directory from the plugin root finds nothing, and the variable is not in the engine's environment (below). With `CLAUDE_PLUGIN_DATA=$CLAUDE_CONFIG_DIR/plugins/data/adage-adage-local` exported in the shell that starts `claude`, the same session ran `bin/hook` and `bin/adage` from the mod. Export it (with `~/.claude` for `$CLAUDE_CONFIG_DIR` when that is unset) until the mod finds the directory itself.

**The developer's route: `--plugin-dir`.** To work on the plugin itself, load it straight from the checkout. `plugin/build.sh` runs the same `cargo build` (it sources `~/.cargo/env` if present; `--no-build` stages what `target/release` already holds; `--from`, `--dist` and `--target` are for its test) and stages the three binaries into `plugin/claude-code/dist/<triple>/`: it reads the host triple from `rustc -vV` (`aarch64-apple-darwin` on Apple Silicon, `x86_64-unknown-linux-gnu` on a Linux box), refuses (writing nothing) unless `adage`, `adage-exec` and `adage-train` are all in `target/release`, writes `plugin/claude-code/dist/SHA256SUMS` over every staged binary (`sha256sum` on Linux, `shasum -a 256` on macOS; lines are `<sha256>  <triple>/<name>`), checks it, copies `LICENSE` and `NOTICE` into `dist/`, and prints the sums. Re-run it after any change to the Rust code, or the plugin keeps linking the old (still checksum-valid) copy.

```sh
plugin/build.sh
claude plugin disable adage@adage-local      # if adage init installed it: never both, or every hook runs twice
CLAUDE_PLUGIN_DATA=${CLAUDE_CONFIG_DIR:-$HOME/.claude}/plugins/data/adage-inline claude --plugin-dir plugin/claude-code
```

Claude Code keys a `--plugin-dir` plugin `adage@inline`, so its data directory is `<config dir>/plugins/data/adage-inline` (seen live; `docs/claude-mod/evaluation-results.md`, defect 4, where the mod was inert without the variable). The mod's own lookup of that directory is in progress on this branch (`plugin/claude-code/hooks/binary.ts` `dataDir`); until it lands, export the variable as above.

**The published marketplace, from v0.1.0.** `/plugin marketplace add stevenvaughanbooth/adage`, then `/plugin install adage@adage`, in Claude Code. `.claude-plugin/marketplace.json` points the `adage` plugin at ref `marketplace` of `stevenvaughanbooth/adage`, a branch only `.github/workflows/publish-marketplace.yml` writes, after a tagged release (`docs/release.md`); until then that install cannot resolve. The marketplace lists one plugin, `adage`, carrying the three binaries.

What happens at every session start (`plugin/claude-code/scripts/session-start`):

1. Claude Code runs `${CLAUDE_PLUGIN_ROOT}/scripts/session-start` with args `[${CLAUDE_PLUGIN_ROOT}, ${CLAUDE_PLUGIN_DATA}]`, 10 s timeout (`hooks.json:4`). If either arg is empty the shim swallows stdin and exits 0 doing nothing (`session-start:46`).
2. It maps `uname -s`-`uname -m` to a triple: Darwin-arm64 to `aarch64-apple-darwin`, Darwin-x86_64 to `x86_64-apple-darwin`, Linux-x86_64 to `x86_64-unknown-linux-musl`, Linux-aarch64 to `aarch64-unknown-linux-musl` (`session-start:48-56`). On Linux, when `dist/<musl>/adage` is absent it falls back to the `-gnu` triple, which is what `plugin/build.sh` stages (`session-start:62`).
3. `bin/adage`: verifies `dist/<triple>/adage` against `dist/SHA256SUMS`, copies it to `$CLAUDE_PLUGIN_DATA/bin/adage-<sha>`, runs it once (`version`, to pay macOS's one-time code-signing check off the hot path), and links `bin/adage` to it. Warm starts skip the hash when `bin/adage-<sha>` already exists (`session-start:78-118`). Each rebuild stages a new sha, so a new copy appears and the old ones are not pruned.
4. `bin/adage-exec`: the same, else `adage-exec` on `PATH`, else beside the serving `adage` (`session-start:120-133`).
5. `bin/adage-train`: the same, else `adage-train` on `PATH`, else beside the serving `adage`; a missing or mismatched one only removes the link, and nothing is said, since no hook runs it (`session-start:135-147`). `adage train` finds it beside the running `adage`.
6. `bin/hook`: links to `adage-exec` when `$ADAGE_HOME/exec/enabled` exists and an adage-exec was found, else to `adage` (`session-start:149-160`). Re-made at every start, so a marker change takes effect in the next session.
7. Every other hook runs `${CLAUDE_PLUGIN_DATA}/bin/hook hook claude <Event>` by absolute path (`hooks.json:5-10`): UserPromptSubmit 5 s, PostToolBatch 30 s, PostToolUseFailure 5 s, PreToolUse (matcher `Bash`) 5 s, Stop 30 s, SessionEnd 5 s. `PATH` is only the shim's fallback when nothing bundled matches.
8. If nothing can serve, the shim links `bin/hook` and `bin/adage` to `scripts/quiet` (and removes `bin/adage-exec` and `bin/adage-train`), writes the reason to `$CLAUDE_PLUGIN_DATA/status`, prints one `{"systemMessage":"adage: …"}` line, and exits 0 (fail open; `session-start:25-45,115-118`).

The mod finds the same binaries from `CLAUDE_PLUGIN_DATA` (`plugin/claude-code/hooks/register.ts:99`; `bridge.ts:138-153`). It has no `PATH` fallback. Every child the mod runs carries `CLAUDECODE=1` and an allow-listed argv, through one of two guards (`plugin/claude-code/hooks/register.ts:94-97`): the bridge guard (`plugin/claude-code/hooks/bridge.ts:57-110`) for read-only/turn-down verbs — `version`; `hook claude <Event>`; `watch --follow --json --stdin ignore --repo <dir> --session <id> [--since <cursor>]`; `graph --json`; `doctor --json`; `status --json`; `--json --for-session`; `ledger note`; `off`; `map --port 0`; `condition`; `delegate status --json` — and the press guard (`plugin/claude-code/hooks/press.ts:44-95`) for person-only verbs that run only from a dialog answer or pane Button press with the press token on stdin: `review <approve|dismiss|strengthen|weaken|extinguish|pin> <track> <id>`; `review delegate <track> <id> --level e2 (--stage --json | --confirm | --predict)`; `delegate on`; `on`; `add <dir> --trust`. Without the variable it derives the data dir from the plugin root for a plugin Claude Code installed into its cache (`<config>/plugins/cache/<marketplace>/<plugin>/<version>` to `<config>/plugins/data/adage-adage`, `bridge.ts` `dataDirFromRoot`); for a `--plugin-dir` checkout the in-progress `bridge.ts` `inlineDataDirs` tries `<config>/plugins/data/adage-inline`; for the `adage@adage-local` plugin `adage init` installs, neither applies yet (above).

Answered live (`docs/claude-mod/evaluation-results.md`, defect 4, and the `adage init` session above): `CLAUDE_PLUGIN_DATA` is substituted into the command hooks (the shim gets it as its second argument, `hooks.json:4`) but is not in the engine's process environment, so the mod reads nothing from `$.env.get('CLAUDE_PLUGIN_DATA')` unless the shell that starts `claude` exports it. Under `--plugin-dir` the data directory is `<config dir>/plugins/data/adage-inline`; for `adage@adage-local`, `<config dir>/plugins/data/adage-adage-local`.

## 3. Hooks: wire every event exactly once

You have used `adage install claude-code --write` before, so a settings file on this machine registers Adage's hooks for the same seven events the plugin registers. Claude Code runs every registration, so plugin plus settings file means every hook runs twice and every notice appears twice (`crates/adage-cli/src/doctor/mod.rs:649-666`). The evaluation plan calls `--plugin-dir` together with the same hooks in `--settings` "the misuse the plan forbids" (`docs/claude-mod/evaluation-plan.md`, §2.1).

`adage init` handles this: beside the plugin it offers to take Adage's entries out of each settings file that has them (the diff first, `<file>.adage-bak` kept; `--yes` accepts), so after it the steps below are a check. By hand, they are the same commands. Live, after `adage init` in a fresh config dir, each of the plugin's hooks settled once per event (`hooks module adage@adage-local classic.SessionStart settled`, one `SessionStart:startup` success, one `SessionEnd` completion), as on all 60 sessions of each `--plugin-dir` comparison run (`evaluation-results.md`).

Where the old registrations can be (`crates/adage-cli/src/placement.rs:38-46`): user scope `~/.claude/settings.json` (or `$CLAUDE_CONFIG_DIR/settings.json`), project scope `<repo>/.claude/settings.local.json`; `adage` also detects, but never writes, `~/.claude/settings.local.json` and `<repo>/.claude/settings.json`.

1. See what is registered, at a terminal (not inside Claude Code), in the repository you will use:

```sh
adage doctor
```

Look at `hooks: claude-code`. Before the plugin has ever run it will say your settings file; once both are present it says `hooks: claude-code (twice)` with `registered by the Adage plugin and in <file>: every hook runs twice, and each notice appears twice`.

2. Remove the settings-file copies, one command per scope that has them. Each shows a diff, asks `Remove them? [y/N]`, and keeps `<file>.adage-bak` (`crates/adage-cli/src/install.rs:321-382`):

```sh
adage install claude-code --remove --scope user
adage install claude-code --remove --scope project
```

Both refuse inside an agent session (`CLAUDECODE` set) and need a terminal on stdin (`install.rs:489-503`). Entries in `<repo>/.claude/settings.json` are not written by `adage install`; doctor says `remove Adage's hook entries from <file> by hand`.

3. Order matters. `adage install claude-code --write` refuses when it sees the plugin (`Nothing written: a second registration in <file> would run every hook twice.`, `install.rs:437-464`), but it detects the plugin only via `<claude_dir>/plugins/data/*adage*/bin/adage`, which exists only after the plugin's SessionStart shim has run once (`crates/adage-cli/src/detect.rs:298-319`). Do not run `--write` again while using the plugin.

4. Nothing goes in `settings.json` for the plugin. `hooks.json` registers all seven hooks and the module; the two options (`band`, `pane`) are plugin `userConfig`, edited in `/config`.

5. Caveat for section 4: with a fresh `CLAUDE_CONFIG_DIR`, the user-scope `settings.json` is the fresh one and holds nothing; a project-scope `<repo>/.claude/settings.local.json` still applies, so step 2's `--scope project` matters even then. `detect.rs:25-27` honours `CLAUDE_CONFIG_DIR`, so export it in the terminal where you run `adage doctor` too, or doctor looks at `~/.claude`.

6. A `--plugin-dir` plugin's data lands in `plugins/data/adage-inline/` and the `adage init` one's in `plugins/data/adage-adage-local/` (both seen live), and both match what `detect.rs` scans (`plugins/data/*adage*/`), so doctor lists either as the registration once its shim has run.

## 4. First run in a fresh config dir, and what you should see

The evaluation plan isolates Claude Code's configuration so nothing in your daily setup leaks in or changes (`docs/claude-mod/evaluation-plan.md:42-47`). It does not isolate `ADAGE_HOME`: a first run writes to your real home (macOS `~/Library/Application Support/Adage`, Linux `${XDG_DATA_HOME:-~/.local/share}/adage`; `crates/adage-store/src/paths.rs:36-49`). Decide once: keep the real home (your existing reflexes are there; the first `adage condition` re-parses every transcript because the parse-cache stamp includes `SESSION_FORMAT`, now 6, and the executable's size and mtime, `crates/adage-ingest/src/cache.rs:68-83`), or export a throwaway `ADAGE_HOME` as the demo runner does (`demo/run.py:59-60`). Whichever you pick, export the same `ADAGE_HOME` in the terminal and in the shell that starts `claude`, or the shim and your terminal commands look at different `exec/enabled` markers (`session-start:57-61`).

1. Set up the scratch root and the fresh config dir (sign in once with `claude` if asked), then install the plugin into it with `adage init` (section 2), at a terminal in the repository:

```sh
export ADAGE_EVAL_ROOT=/tmp/adage-eval
mkdir -p $ADAGE_EVAL_ROOT
export CLAUDE_CONFIG_DIR=$ADAGE_EVAL_ROOT/claude-config
adage init
export CLAUDE_PLUGIN_DATA=$CLAUDE_CONFIG_DIR/plugins/data/adage-adage-local   # until the fix lands (section 2)
```

2. Start Claude Code with a debug log (`evaluation-plan.md` §2.1). The plugin is installed, so no `--plugin-dir`:

```sh
claude --debug --debug-file $ADAGE_EVAL_ROOT/q1.log
```

On the developer's route the same session is `claude --plugin-dir plugin/claude-code --debug --debug-file $ADAGE_EVAL_ROOT/q1.log`, with `CLAUDE_PLUGIN_DATA` set to `…/plugins/data/adage-inline` and the installed plugin disabled.

3. What you should see, in a repository where Adage already has reflexes or history:

- The opening line, once, redrawn by the mod at the `InfoNotice` site as `◠ adage · …` (`plugin/claude-code/hooks/draw/sites.ts:33-38`). The text is the binary's SessionStart `systemMessage`: `adage: N reflexes and M instincts active in this repo; it keeps learning as you work. Nothing runs without you.`, or in a repository with history but no reflexes yet `adage: learning from N past sessions in this repo; reflexes arrive within a minute. …` (`crates/adage-hook/src/session_start.rs:152-187`). Seen live, headless: the line reaches the engine as the SessionStart hook's `systemMessage` (the debug log's `Hook SessionStart:startup (SessionStart) success: {"systemMessage":"adage: …"}`). Not yet seen, because it needs an interactive session: whether the mod redraws it at the `InfoNotice` site (`docs/claude-mod/impl/B2-B3-notes.md:183`). If it stays Claude Code's plain notice, that is finding number one.
- The band under the conversation: `◠ adage  {n reflexes this session} · {n steps without a model request} · …` (`plugin/claude-code/hooks/view/band.ts:56-113`). If it reads `◠ adage ○ off · run adage doctor`, the mod is inert: see section 8.
- Possibly, once per Claude Code version per machine, the second band line `» Claude Code {v} is newer than this plugin was tested with (2.1.288) · run adage doctor`; it clears at the first prompt and never returns for that version.

4. Type `/adage status`. It prints the status card as text from `adage --json --for-session` (`plugin/claude-code/hooks/commands.ts:72,118-123`): header lamp (`● healthy`, or `check: <label>` / `problem: <label>` naming the worst doctor check), rows, and a `next` ending in `adage doctor`. `adage is not answering here; run adage doctor in a terminal.` means the read failed. Bare `/adage` prints the card and opens the pane on Live. `/adage` is registered only while the mod is active, that is, when the hook binary probe succeeded: `activate()` returns before `m.command.register(...)` when `m.rt.bin === undefined` (`plugin/claude-code/hooks/session.ts:41-42`; `COMMAND = 'adage'` is `plugin/claude-code/hooks/plugin.ts:51`); if Claude Code does not know the command, the mod is inert.

5. Type `/adage watch`. The pane opens with tabs Live (`l`), Reflexes (`r`), Health (`t`), footer `esc: close`, and `Explore in browser` at the end of each tab (`plugin/claude-code/hooks/draw/pane.tsx:92-128`). Live shows `Nothing yet in this repository. Reflexes appear here as Adage uses them.` until something fires. Health lists every `adage doctor --json` check as a lamp row.

6. In a second terminal, confirm the shim did its work, and that the mod runs its binary. The data dir is whatever Claude Code passed the shim as `CLAUDE_PLUGIN_DATA`: `$CLAUDE_CONFIG_DIR/plugins/data/adage-adage-local` for the plugin `adage init` installed, `…/adage-inline` under `--plugin-dir`, `…/adage-adage` for the published plugin (all three spellings follow Claude Code's `<plugin>@<marketplace>` key). No `$.process.run (adage)` line in the debug log means the mod is inert: export `CLAUDE_PLUGIN_DATA` (section 2) and start a new session.

```sh
grep -n 'CLAUDE_PLUGIN_DATA\|session-start' $ADAGE_EVAL_ROOT/q1.log | head
grep -c '\$.process.run (adage)' $ADAGE_EVAL_ROOT/q1.log     # more than 0: the mod asked its binary
ls -l $CLAUDE_CONFIG_DIR/plugins/data/*adage*/bin/
```

You want `adage -> adage-<sha>`, `adage-exec -> adage-exec-<sha>`, `adage-train -> adage-train-<sha>`, `hook -> adage`, and no `status` file (it exists only after a failed start, `session-start:161`).

7. Then, still at the terminal:

```sh
adage doctor
```

`hooks: claude-code` should pass with `registered by the Adage plugin`; `hook activity: claude-code` may still warn `Registered; no hook has been observed in this repository yet.` until a prompt has gone through (the receipt is refreshed at most once a minute, `crates/adage-hook/src/receipt.rs`); `claude-code plugin binary` should pass with `matches this binary` if you are running `target/release/adage`.

8. Count hook invocations. The plan's testable proxy is exactly one `session_start` record in the session's spool (`evaluation-plan.md:108-110`; `docs/claude-mod/test-plan.md:830-832`). Spools are binary-framed, not NDJSON (`docs/spool.md:9-14`), so read them through the follower (section 7) rather than `grep`.

## 5. Reflexes: getting a repository learned, and seeing what was learned

Minimal sequence, at a terminal inside the repository:

1. Condition once from the traces Claude Code already recorded for this repository (`docs/usage.md:337-341`). `adage sense` is the read-only preview; `--all` does every repository; `--demo` lowers thresholds.

```sh
adage condition
adage status
```

The first run after pulling re-parses every transcript (section 4), so expect it to be slow once. Output tracks and index are identical with or without the cache.

2. After that, learning is automatic (`docs/live-learning.md:123-131`): at `Stop` and `SessionEnd` the hook starts `adage condition --auto --repo-root <root>` detached when `learn.auto` is on (default `true`), `learn.repos` names the repository (default `all`), no `.adage/disabled` exists, and at most once per `learn.min_interval_secs` (default 120) per repository. At SessionStart in a repository with no reflexes the hook runs the learner with `--bootstrap` at once (`live-learning.md:138-143`).

3. Thresholds. A learned reflex needs `max(min_support, ceil(min_support_share * n), 1)` supporting traces, defaults `min_support = 5` and `min_support_share = 0.01` (`crates/adage-tracks/src/params.rs:145-173`). The only knob is `learn.thresholds = default|demo` (guarded; `demo` lowers `min_support` to 3) or `adage condition --demo`. Note: `docs/live-learning.md:238` names a `learn.min_support` key; it does not exist in `crates/adage-store/src/config.rs:183-360` (`KEYS` is authoritative). There is no number of sessions that guarantees a reflex (`docs/first-reflex.md:38-40`).

4. What waits for you. A `new` reflex goes into service directly; a `nascent` one (a context trigger, or an execution-only routine, or an environment-effect instinct step) waits for approval (`crates/adage-learn/src/lib.rs:579-583`):

```sh
adage review
adage review approve <track> <reflex>
```

Approval accepts evidence only; it grants no execution or prediction (`docs/first-reflex.md:50-51`). In the session the same decision is the `enable` answer of the dialog (section 6).

5. Seeing what was learned:

```sh
adage                      # the status card: is it working here?
adage status --json        # every track and reflex with strength and evidence; `candidates` below support
adage review --json        # what waits for a person
adage watch                # live pane: hook decisions, conditioning, health
tail -f "$ADAGE_HOME/learn.log"
```

`adage status --json` (`{intent_model, tracks[], candidates, …}`, `crates/adage-cli/src/main.rs:638-695`) and `adage --json` (the card: `health`, `off`, `rows`, `next`, `crates/adage-cli/src/status_card.rs:564-585`) are different shapes; the mod uses both.

6. `adage learn` and `adage survey` no longer exist; the error names `adage condition` and `adage sense` (`crates/adage-cli/src/cli.rs:820-839`).

## 6. Granting the mod the right to act

Two person verbs exist in the mod (`docs/claude-mod/ux-design.md:43-47`): **enable** (let Adage tell the agent this: `adage review approve`) and **allow** (let Adage do this for you: a predict grant in one act, or an execute grant in two). Only `adage-exec` predicts; `adage hook` has no grants to read (`docs/execution.md:566-570`). So three things must all hold before the first prediction: the machine is at E2, the reflex holds a predict grant, and `bin/hook` is `adage-exec` (the `exec/enabled` marker). Every one takes effect in new sessions only.

### From the terminal

1. Opt this machine in to E2. Writes `[exec] max_level = "E2"`, `auto_approve = "off"`, `require_sandbox = true` in `$ADAGE_HOME/policy.toml` (`crates/adage-cli/src/exec.rs:365-416`). Default `max_level` is E1, and a predict grant is refused below E2 (`policy caps execution at E1; a call made in the agent's place needs E2`, `crates/adage-exec/src/policy.rs:304-310`).

```sh
adage delegate policy --max-level e2
```

2. Turn execution on. Finds `adage-exec` beside the running `adage` (or `--bin <path>`), writes `$ADAGE_HOME/exec/enabled`, and prints `Claude Code plugin: no second install; with <marker> present its hooks call adage-exec` (`exec.rs:241-308`). From `target/release/adage` this works because `build.sh` built `adage-exec` beside it.

```sh
adage delegate on --repo .
```

3. Grant one reflex the predict right. A context-triggered reflex gets `--predict`; an after-edit reflex is offered as an E2 execute grant instead (two acts, section "From the dialog"). `--predict` conflicts with `--level`, `--effect` and the other execution flags, stages nothing, prints the argv, contracts, scripts with sha256 and evidence, then records the grant (`exec.rs:1033-1150`). Refused when the reflex already holds an E1/E2 grant: `adage review revoke <track> <reflex>` first.

```sh
adage review delegate <track> <reflex> --predict --repo .
```

4. See it:

```sh
adage delegate status --repo .          # enabled, predict on|off, grants per repository, lapsed grants
adage-exec status --repo .              # execution grants, then prediction grants
```

A valid predict grant waits for nothing, so `adage review` does not list it; `adage delegate status` does. One reader did not check whether bare `adage` or `adage status` show grants.

5. The grant is keyed on the command template, slot contracts, scripts (hashed every step), effect and pack digest; any change lapses it, nothing is predicted, and the band says `» 1 grant lapsed · /adage health` with the reapprove command in the Health tab (`docs/execution.md:576-590`).

### From the dialog

At `turn.complete` (interactive only, model idle) the mod may ask one question via `$.ui.ask`, at most one per turn end and one per reflex per day (`docs/claude-mod/impl/B4-D5-notes.md:59-76,119-127`):

- Enable: `Adage learned: {summary}. Tell the agent?` with `enable` / `not this one` / `why`.
- Allow, predict offer: `Adage can {summary}, instead of asking the agent to. Allow it?` with `allow` / `not now` / `why`; `allow` runs `adage review delegate <track> <id> --predict` with the press on stdin.
- Allow, execute offer: `see what it would do` / `not now`; the first runs `--level e2 --stage --json` and opens the pane with `it runs`, `it changes`, `a: allow · n: not now · d: details`; `allow` runs `--level e2 --confirm`. The first confirmed execute grant on an E1 machine writes policy, marker and grant together (`exec.rs:1259-1294`).

The answers run through a one-use press token issued in the window between the plugin's SessionStart and the agent's first act (`docs/execution.md:619-676`). With no press held, a one-act answer is kept in `$.store` and offered at the next session start (`Kept from your last session: … Run it now?`).

Pitfall, flagged by the learning reader as an open question: offers are computed as `permitted` against `trial_e2` (the policy as it would be raised, `crates/adage-app/src/graph.rs:122`), but `delegate_predict` checks the current policy (`exec.rs:1069-1082`). On a machine still at E1 the dialog can ask `Allow it?` for a predict offer and the `allow` press is then refused (`not recorded: …`). `delegate policy` is not in the mod's `PRESS_VERBS` (`B4-D5-notes.md:184-185`). Run step 1 above at the terminal first.

### Config keys (from `KEYS`, `crates/adage-store/src/config.rs:320-360`)

| key | default | guarded | what |
|---|---|---|---|
| `runtime.predict_cap` | `6` | yes | steps per session Adage takes in the model's place; whole number 0..50; 0 predicts none |
| `runtime.routine_model` | `""` | no | the model a step goes to, at low effort, when it only does what a delivered directive says; `""` sends every step to the session's model |
| `skills.invoke` | `false` | yes | invoke a skill at a `skill` reflex's step under a predict grant, when `adage train` recorded it inert in a pack you accepted |
| `runtime.control` | `0.1` | no | share of sessions held out as the control arm; env `ADAGE_CONTROL` |
| `runtime.rewrite` | `true` | no | PreToolUse interpreter rewrite and the mod's learned-pair substitution; `adage delegate rewrites on|off` |

Guarded keys refuse inside an agent session and refuse a press: set them at a terminal (`crates/adage-cli/src/config.rs:35-58`). Precedence is environment, then file, then default.

```sh
adage config                                       # every key, value and source
adage config set runtime.predict_cap 6
adage config set runtime.routine_model <model>
adage config set skills.invoke true
```

### The machine-wide off switch for prediction

```sh
adage delegate predict off     # writes $ADAGE_HOME/exec/predict-off; nothing predicted from the next step; grants kept; runs from anywhere
adage delegate predict on      # removes it; a person's decision (terminal or press)
```

Other ways it stops: `adage config set runtime.predict_cap 0`; `adage delegate off` (hooks call `adage`, which cannot predict); `adage review revoke <track> <reflex> --predict`; `adage off` per repository; `ADAGE_DISABLE=1` for every hook (`exec.rs:158-239,762-773`).

## 7. Verifying it acts

Three sources of proof, from a session started after section 6 (a new session, with the plugin, in the granted repository, `--debug --debug-file` on).

1. The follower, in a second terminal. Run it without `--session <id>`: with that flag the current session's rows are scrubbed of `would_predict`, `control` and withheld acts (`crates/adage-cli/src/ui/follow.rs:22-28,361-401`). Add `--stdin ignore` when stdin is not a terminal, or the follower exits at EOF (`crates/adage-cli/src/ui/mod.rs:117-122`).

```sh
adage watch --follow --json --repo . --stdin ignore
```

One JSON object per line, `kind` in `decision|learn|health|learner`, with a `cursor`. What proves what (`docs/live-learning.md:324-363`): a `decision` line with `event: step` and an act `predicted` proves a prediction happened; `would_predict` proves the TurnStep was asked and the prediction withheld (control arm or observe mode); a `pretool` row with act `answered` (`receipt`) or `judged {what: substituted}` proves a receipt or substitution; `health` lines carry the lamp.

2. The spool, `$ADAGE_HOME/sessions/<session>.spool`. Binary-framed (`magic | len | crc | payload`), so not `grep`-able; the follower above and `demo/adage_metrics.py` read it. What to expect (`docs/spool.md:48-77`): exactly one `session_start` record per session (hooks wired once; it carries `arm`, `bin` as `adage` or `adage-exec`); a `turn_step` record only for a step that predicted or routed, or would have (a step with nothing pending leaves no record), with `decision.predicted` and `predicted_at`, or `decision.would_predict`.

3. The debug log. Model requests are the lines matching `^\S+ \[DEBUG\] \[API REQUEST\] /v1/messages x-client-request-id=([\w-]+) source=(\w+)$` (`evals/reflex-proof/harness/heldout_analysis.py:67-68`); retries count. Around a predicted step there must be none (`evaluation-plan.md:258-261`); at a finish-line step, one fewer than the same task without the mod (`:284`).

```sh
grep -cE '^\S+ \[DEBUG\] \[API REQUEST\] /v1/messages x-client-request-id=[A-Za-z0-9_-]+ source=[A-Za-z0-9_]+$' $ADAGE_EVAL_ROOT/q1.log
```

The transcript on disk shows the yielded call as a `tool_use` whose id begins `adage_` (`plugin/claude-code/hooks/step.ts:65-66`).

4. In the session: `/adage why` prints the last directive as the model read it (`No reflex has spoken in this session yet.` when nothing fired). The band's `{n steps without a model request}` cell counts predicted steps.

5. Reasons for "nothing predicted" (`evaluation-plan.md:250-266`): `bin/hook` is `adage` (no marker; check `ls -l` in the plugin data dir), `exec/predict-off` exists, step index 0 for anything but a run reflex the prompt directed (a sequence, a chain, a finish line, or a directive from an earlier prompt), a subagent, two in a row, the `runtime.predict_cap`, the grant lapsed, or this is a control session (1 in 10 by default; nothing on screen says so; `ADAGE_ARM=treated` forces a treated session, section 9).

6. A packaged measurement exists if you train the demo sandbox (it runs real `claude -p` sessions and costs money): `python3 demo/run.py one W-2041 mod --debug` records `requests`, `requests_verified`, `hooks_wired_once`, `predicted_steps` in `$DEMO_ROOT/state.json` (`evaluation-plan.md:354-369`).

## 8. Degraded states and fixes

Start with `adage doctor`; it exits non-zero only on a FAIL, so read the WARN rows too (`crates/adage-cli/src/doctor/mod.rs:1252-1254`). `adage doctor --json` is what the pane's Health tab lists; its output is `{ "checks": [...], "summary": { "pass", "warn", "fail" } }` (`doctor/mod.rs:1244-1248`), and neither `--json` nor `--watch` is a clap flag: doctor parses its args by hand (`doctor/mod.rs:1235,1244`). For a non-passing check with a fix, the Health tab shows `enter: condition this repository now` only when the action is `run` with command exactly `adage condition` (`plugin/claude-code/hooks/view/pane.ts:197`); any other `run` action, or a `manual` fix whose prose contains a backticked command, gets a `Copy command` button (`view/pane.ts:198-200`, `plugin/claude-code/hooks/draw/pane.tsx:317-319`); a manual fix with no backticked command shows no button, and passing checks never show one.

| what you see | meaning | fix |
|---|---|---|
| band `◠ adage ○ off · run adage doctor`; `/adage` missing or `adage is not answering here` | no binary could serve: the mod is inert, every hook answers nothing (fail open) | `cat $CLAUDE_PLUGIN_DATA/status`: `no binary for <os-arch>; …` means no stage for the host triple (run `adage init` again, or `plugin/build.sh` on the developer's route; on Linux check the `-gnu` fallback); `does not match its checksum` means a stale `SHA256SUMS` (the same); `ls -l $CLAUDE_PLUGIN_DATA/bin` shows the links at `scripts/quiet`. Then a new session. If there is no `status` file, the links are fine and the debug log has no `$.process.run (adage)` line, the mod did not find its data directory: export `CLAUDE_PLUGIN_DATA` (section 2) and start a new session. |
| status line `hooks are not answering; run adage doctor` | three hook answers in a row failed or timed out (`plugin/claude-code/hooks/binary.ts:9-19`) | `adage doctor`; `fail-open errors` reads `$ADAGE_HOME/fail-open.log` |
| `hooks: claude-code (twice)` | plugin and a settings file both register | section 3, step 2 |
| `claude-code plugin binary … is a different build` / `is a broken link` | staged binary differs from the `adage` you run, or the link is dead | `adage init` again with the `adage` you run (or `adage update`; `plugin/build.sh` on the developer's route), then a new Claude Code session |
| `index … is newer than this binary` (FAIL) | an older `adage` shares the home | upgrade or remove it (section 1, step 8) |
| `index` missing / stale / invalid | | `adage condition` |
| `hook activity: claude-code` WARN | registered, no delivery receipt yet (refreshed at most once a minute) | send a prompt in a new session, refresh Health |
| `kill switches` WARN | `.adage/disabled` (`adage on`), `ADAGE_DISABLE=1`, `ADAGE_CONTROL>=1`, `ADAGE_ARM=control` | unset them; `adage on` is the runnable fix for the repository marker |
| `adage: execution is on, but no adage-exec binary was found for this machine; serving without execution` | marker set, no `adage-exec` staged | build it beside `adage` and run `adage init` again (`plugin/build.sh` on the developer's route); or `adage delegate off` |
| band `○ off in this repository · /adage on` | `.adage/disabled` | `adage on` at a terminal (`/adage on` opens an ask only when a press is bound) |
| Live tab `feed stopped · run adage doctor` | the follower child ended early three times (`plugin/claude-code/hooks/follower.ts:21-30`) | `/adage pause` twice (pause, resume) restarts it from the kept cursor with the count at 0; doctor has no check for the follower itself |
| `» 1 grant lapsed · /adage health` | a granted command changed | the Health tab holds `adage review delegate … --predict` |
| status `an action was interrupted; …` | an E2 action was interrupted | `adage-exec recover --id <op> --acknowledge` in a terminal |

Two notes. `docs/Adage — Architecture.md:702,709` still describes an older shim (stderr hint, "needs the adage binary on PATH", marketplace source `./plugin/claude-code`); trust the shim script and `marketplace.json`. And nothing is drawn or asked in `claude -p` or the SDK (the engine reports `isInteractive: false` there); in the VS Code panel nothing is drawn (the band and pane draw only on the terminal and desktop surfaces) and `/adage` answers as text; absence of a warning there is not health (`docs/usage.md:412-413`).

## 9. Observe mode and the control arm, for a side-by-side

Three environment knobs, all read by the binary or the mod from the environment Claude Code inherits; leave them unset for normal use.

- `ADAGE_MOD_OBSERVE=1`: the mod asks the binary at every step and Bash call with `observe: true`, records `would_predict` as the control arm does, but never yields a predicted call or replaces a command; drawing is the treated arm's (`plugin/claude-code/hooks/session.ts:22`; `step.ts:38-47`; `evaluation-plan.md:268`). Note: `docs/claude-mod/test-plan.md:634-636` says only the observers register under observe; `register.ts` registers every hook unconditionally and observe is a runtime flag (`docs/claude-mod/impl/B2-B3-notes.md:150-151`). Do not describe it as changing registrations.
- `ADAGE_ARM=treated|control`: forces the session's arm. In control Adage computes but says and predicts nothing, spooling `would_predict`; `adage doctor` warns `ADAGE_ARM=control: this shell's sessions receive nothing`.
- `ADAGE_CONTROL=<share>` (config `runtime.control`, default `0.1`): the held-out share; `>= 1` is shadow mode.

A by-hand side-by-side on one repository, same task, three sessions (with the installed plugin drop `--plugin-dir plugin/claude-code`; either way with `CLAUDE_PLUGIN_DATA` exported, section 2):

```sh
ADAGE_ARM=treated claude --plugin-dir plugin/claude-code --debug --debug-file $ADAGE_EVAL_ROOT/treated.log
ADAGE_ARM=treated ADAGE_MOD_OBSERVE=1 claude --plugin-dir plugin/claude-code --debug --debug-file $ADAGE_EVAL_ROOT/observe.log
ADAGE_ARM=control claude --plugin-dir plugin/claude-code --debug --debug-file $ADAGE_EVAL_ROOT/control.log
```

Then section 7: `[API REQUEST]` counts per log, `predicted` versus `would_predict` in the follower (run without `--session`). The demo runner's arms are the packaged form: `control` (hooks via `--settings`, `ADAGE_ARM=control`), `mod-observe` (plugin, `ADAGE_MOD_OBSERVE=1`), `mod` (plugin), never the plugin and `--settings` together (`demo/run.py:16-20,179-189`).

## 10. What has run live, and what has not

Everything offline is green: 134 suites / 1,401 tests in the workspace, 351 in `claude plugin test plugin/claude-code`; a different count on your machine is not itself a failure (`evaluation-plan.md` §1). Live, headless only so far (`evaluation-results.md`; the `adage init` session in section 2):

- Answered. `CLAUDE_PLUGIN_DATA` reaches the shim's arguments but not the engine's process: the mod needs it exported until the fix lands (section 2). The data directories are `plugins/data/adage-inline` (`--plugin-dir`) and `plugins/data/adage-adage-local` (`adage init`). The opening line reaches the engine as the SessionStart `systemMessage`. Every hook settles once per event (one `session_start` record per session, 60 of 60 in each comparison run). The shim links all three binaries and `bin/hook`, and `bin/hook -> adage-exec` once `adage delegate on` has run (every plugin-arm session was served by `adage-exec`). The mod asks `TurnStep` at every step, and a granted step is yielded in the model's place: an `adage_` tool_use in the transcript, a `turn_step` record with `predicted`, no `[API REQUEST]` for that step (one finish-line probe; 12 of 12 `mod` sessions in run 2, `read-linked-files`). The request after a yielded step is answered by the server's `404 … No thread state was found` and replayed once (12 of 12 in run 2): an engine and API interaction recorded there, not changed.
- Still open, in roughly this order:

1. Which fires first, the mod's `session.start` or the shim; the re-probe after `classic.SessionStart` covers either order at the cost of one extra `version` run (`P3-notes.md:95-99`). And whether `classic.SessionStart` reaches the mod before the first `UserPromptSubmit`, which is the press window (`B4-D5-notes.md:214-218`).
2. The first-start budget with three binaries hashed: `< 1,500 ms` first start, `< 200 ms` for ten warm starts, measured by `cargo test --release -p adage-cli --test zero_config_plugin release_plugin_startup -- --nocapture` (release only; `evaluation-plan.md` §3). On macOS each new binary pays 119-350 ms on first spawn (`spikes/S1-hook-latency/FINDINGS.md:75-95`).
3. Drawing, which needs an interactive session (`B2-B3-notes.md:180-191`): the `systemMessage` redrawn at `InfoNotice`; `UserMessage` text equalling the prompt; `classic.PreToolUse` carrying `updatedInput`; the follower (`adage watch --follow --json --stdin ignore`) surviving `$.process.spawn`'s stdin, restarting across a module reload with `--since`, gone after `/clear` and exit (`ps aux | grep 'adage watch --follow'`); `$.ui.copy`, `$.ui.scroll`, the pane across a reload; the map opening the browser; `ToolGroup` rows carrying `calls[].tool_use_id`.
4. Press and dialog (`B4-D5-notes.md:212-239`; `A7-A8-notes.md:304-308`): the `$.ui.ask` dialog drawing from a detached `turn.complete`; the press spent with `via: press` in `review.yaml`; whether `UserPromptSubmit` fires for `/adage …` commands (Q14, which decides whether `/adage on` typed first can ever be accepted); `see what it would do` against a real `--stage --json`; `$.store` surviving a restart.
5. Acting beyond what run 2 showed (`C3-C7-C8-D2-C9-E-notes.md:121-128,217-221`; `evaluation-plan.md` §4): ten of ten yielded calls on one route; the finish line rewriting `end_turn` into `tool_use`; whether `PostToolBatch` follows a receipt-answered call; `$.tool.check` agreeing with `keeps_permissions`; `routine_model` visible in the request's model; subagent `agent_id` reaching the command hooks.
6. Windows: whether Claude Code in Git Bash runs the exec-form SessionStart hook at all (`session-start:53`); not your machine, but it is on the list.

What to report back, per section, in `docs/claude-mod/evaluation-results.md` in the format of `evaluation-plan.md:413-429`: machine (CPU, OS, Claude Code version, commit), the command as run, the result (a number or yes/no per probe), pass/fail; attach the debug logs' request counts; a number not produced is listed as not produced with the reason, never estimated. On a "no", the chunk's notes name the assumption that failed and, where seen, the fallback; record it, fix the mod or binary, re-run the section. Fail-open on hook paths is never relaxed to pass a test. Also record the exact dialog text seen, whether `adage review` history shows `via: press`, and the spool's `press_issued` / `press_spent` counts.

Two doc fixes to flag rather than fold in: the plan's branch name (`evaluation-plan.md:16,23`, `docs/claude-mod/impl/README.md:3`), and `learn.min_support` in `docs/live-learning.md:238`.

## 11. Switching it off, and uninstalling

In order of how much you turn off:

1. This repository only, from anywhere (an agent session included):

```sh
adage off            # writes <root>/.adage/disabled; every hook here answers nothing
adage on             # removes it; a person's decision: terminal, or /adage on with a press
```

2. Predictions only, machine-wide, grants kept: `adage delegate predict off` (section 6).

3. Execution: `adage delegate off` removes `$ADAGE_HOME/exec/enabled`; the shim links `bin/hook` back to `adage` at the next start; grants are kept (`exec.rs:241-328`).

4. The kill switch: `ADAGE_DISABLE=1` in the environment Claude Code inherits. Every hook event prints nothing and does no work (after draining stdin and checking the harness name), except that a pending `/adage` press is still answered so a person can turn Adage back on from the session (`crates/adage-hook/src/lib.rs:138-147`; `crates/adage-hook/src/press.rs:275`). It applies wherever the variable is set, and `adage on` cannot override it: `adage on` only removes the repository's `.adage/disabled` marker and reports that `ADAGE_DISABLE=1` still keeps every hook off (`crates/adage-cli/src/switch.rs:21-23,81-85`).

5. The plugin. The one `adage init` installed: `claude plugin uninstall adage@adage-local` (it removes the plugin's data directory too), then `claude plugin marketplace remove adage-local`; the staged copy under `$ADAGE_HOME/plugin/` stays until you delete it. On the developer's route, stop passing `--plugin-dir plugin/claude-code`; its data directory (`$CLAUDE_CONFIG_DIR/plugins/data/adage-inline/`) can be deleted by hand. For the published plugin, from v0.1.0: `/plugin uninstall adage@adage` in Claude Code.

6. If you go back to command hooks without the plugin, re-register them at a terminal: `adage install claude-code --write` (`--scope user` optional). `adage doctor` then confirms a single registration.

7. Your data stays. `adage install --remove` and the plugin's removal leave `$ADAGE_HOME` (sessions, tracks, `approvals.json`, `policy.toml`, `learn.log`) in place (`install.rs:378`). Remove it yourself if you want a clean slate.
