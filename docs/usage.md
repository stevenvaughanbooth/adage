# Using Adage

Installing and updating, every command, the live views, settings, sharing and review. The [README](../README.md) has the quick path; [How Adage works](how-it-works.md) is the behavior reference.

## Installing and updating

One install step, then two commands: `adage init` once, and `adage update` whenever a newer Adage
is out. The first sets up the Claude Code plugin, every other harness's hooks and learning; the
second keeps all of them current.

### Install

With Node.js 18 or later, install the npm package:

```sh
npm install -g @adage-sh/cli
```

npm selects prebuilt binaries for macOS (Apple Silicon or Intel), Linux (arm64 or x64), or
Windows (x64). Keep optional dependencies enabled: they carry the platform binaries. There
are no install-time download scripts, and you do not need Rust or a checkout.

Then run this in your repository:

```sh
adage init
```

npm puts `adage` on your PATH for setup, learning and review. Its platform package also contains
`adage-train`, used by `adage train` to compile supported skill steps, and, on macOS and Linux,
`adage-exec` for separately approved execution. Adage finds these companions beside its native
binary; npm does not add separate commands for them to your PATH. Windows execution is unsupported.

**Standalone downloads:** get binaries from the
[latest GitHub release](https://github.com/stevenvaughanbooth/adage/releases/latest).
On macOS or Linux, the shell installer installs `adage`:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/stevenvaughanbooth/adage/releases/latest/download/adage-cli-installer.sh | sh
```

Follow its PATH instructions, then run `adage init`. It offers to fetch matching companions
and checks their published checksums.

The release's Claude Code plugin bundles the same platform binaries. Install it inside Claude
Code with:

```text
/plugin marketplace add stevenvaughanbooth/adage
/plugin install adage@adage
```

Windows release artifacts provide advisory operation only. Automatic execution is unsupported,
and Windows hook invocation has not been verified. See [harness support](harnesses.md).

**From source:** to build version 0.1.1 yourself, use Rust and a C compiler:

```sh
cargo install --locked --git https://github.com/stevenvaughanbooth/adage --tag v0.1.1 adage-cli adage-exec adage-train
```

For a Windows source install, omit `adage-exec`. Source installs place the binaries in `~/.cargo/bin`.

**From a checkout**, to work on Adage or try a branch:

```bash
cargo build --release --locked -p adage-cli -p adage-exec -p adage-train
target/release/adage init
```

`adage init` stages the Claude Code plugin from the build that runs it, so the plugin you get is
this checkout's. After a rebuild, run `target/release/adage init` again: it stages the new build,
and the next session runs it. `adage update` from a checkout runs `git pull --ff-only` and the same
`cargo build`, then stages the plugin again.

### First run: `adage init`

Run it at a terminal, in your repository. It works in seven steps, says what each does as it runs,
and asks before anything changes:

1. **Harnesses:** which of Claude Code, Codex and OpenCode are installed (read-only).
2. **Binaries:** that `adage-exec` and `adage-train` sit beside `adage`. One that is missing is
   fetched from this version's release on a yes, checked against the release's `.sha256` (nothing
   is written on a mismatch); a checkout build is pointed at the `cargo build` that makes it.
3. **Hooks,** per harness found. For Claude Code the Adage plugin comes first: `adage init`
   stages it from the `adage` you ran as a one-plugin local marketplace under `$ADAGE_HOME/plugin/`
   (its hooks and modules, carried inside the binary, and the three binaries with their
   `SHA256SUMS`), runs Claude Code's own `claude plugin marketplace add <that directory>` and
   `claude plugin install adage@adage-local`, and checks `claude plugin list`. A settings-file
   registration beside the plugin would run every hook twice, so it offers to take Adage's entries
   out of that file. Codex and OpenCode get their hook files. Every file is shown before it is
   written (a settings file as a diff, OpenCode's plugin file by name), and the previous version is
   kept as `<file>.adage-bak`.
4. **Embedding model:** one is bundled, so there is usually nothing to fetch.
5. **Repositories:** the ones Claude Code has sessions in, most recently used first, the current
   one ticked (space ticks, `a` all, `n` none). Adage reads the history of the ones ticked
   (`learn.repos`); sessions in other repositories are recorded, not read.
6. **Sense:** which reflexes this repository's traces would form (read-only).
7. **Instincts:** which of Adage's built-in reflexes bind here, from the repository's files.

At a terminal the guided flow also offers a first conditioning of each repository ticked and the
switch for automatic conditioning. It ends on one screen, with at most two `next` lines:

```text
hooks       registered: claude-code (the Adage plugin)
binaries    adage, adage-exec, adage-train in ~/.cargo/bin
learning    github.com/acme/billing · after each session
first line  your next Claude Code session here opens with
            adage: learning from 23 past sessions in this repo; reflexes arrive within a minute. Nothing runs without you.

Later: `adage update` updates Adage, its hooks and the plugin together; `adage doctor` if anything looks wrong.

next  claude · start a session; Adage says what it is doing on its first line
```

Codex adds one more `next` line: open `/hooks` in Codex to approve the new hooks.

`adage init --yes --hooks user` takes every step without a question: the plugin at user scope
(when `claude` is not on `PATH`, or refuses, the settings file `~/.claude/settings.json` instead,
and the two `/plugin` commands named), Codex's and OpenCode's user files, missing binaries fetched,
this repository added to `learn.repos` (unless that is `all` on purpose), its instincts bound, and
its history conditioned in the background. `--hooks project` does the same for this repository
alone (the plugin at Claude Code's `local` scope, `.claude/settings.local.json`, which is never
committed). `--yes` without `--hooks`, or `--hooks none`, connects nothing and says so. `--print`,
a pipe, or a run inside an agent session shows what would happen and changes nothing; `--yes`
refuses inside an agent session.

Running it again is safe. A harness already registered, in either scope or by the plugin, is never
registered a second time (`registered … · nothing to write`); the plugin is staged again from the
binary you ran, and nothing else that is in place is rewritten. In another repository,
`adage init` adds that one to the repositories Adage learns from.

The mod finds the plugin's data directory automatically after the session-start shim creates
its binary links. No `CLAUDE_PLUGIN_DATA` export is normally needed. If the binary is missing
or does not answer its version check, the mod stays inactive; run `adage doctor` to inspect setup.

### If you prefer to do it by hand

Each step `adage init` takes is also a command of its own.

- **The plugin, typed in Claude Code:** `/plugin marketplace add
  stevenvaughanbooth/adage`, then `/plugin install adage@adage`. `adage init` names these when
  `claude` is not on `PATH`.
- **Claude Code's settings file instead of the plugin:** `adage install claude-code --write`
  (`--scope user` for every repository). The hooks are the same; the band, the pane and `/adage`
  come only with the plugin.
- **Codex and OpenCode:** `adage install codex --write --scope user` and
  `adage install opencode --write --scope user` ([below](#opencode-and-codex)).
- **The plugin straight from a checkout**, for developing it: `plugin/build.sh` builds the three
  binaries and stages them into `plugin/claude-code/dist/`, and `claude --plugin-dir
  plugin/claude-code` loads the plugin for that session. Disable the installed one first
  (`claude plugin disable adage@adage-local`), or every hook runs twice. The mod also discovers
  the inline plugin's data directory after the session-start shim creates its binary links.
- **Learning now:** `adage condition` in a repository conditions on its history at once; after
  that the background learner keeps it current.

### Updating

```bash
adage update             # update Adage, its hooks and the plugin together, the way it was installed
adage update --print     # what it would run, keep and change; runs nothing
adage update --rollback  # put back the binaries the last update replaced
adage update --check     # only ask whether a newer Adage is out (--now asks again, --json as data)
```

**Being told.** When a newer Adage is out, the status card (bare `adage`) shows
`update      0.2.0 is out (you have 0.1.0) → adage update` and puts `adage update` first under
`next`; `adage ui`'s Home and the map's top bar say the same. A newer release is information,
never a problem: the health lamp stays as it was.

**The check.** No hook ever touches the network. The one request Adage makes on its own is this
check, by the background learner after it conditions a repository, at most once a day per data
directory: one request to the npm registry (`registry.npmjs.org/@adage-sh/cli/latest`) and, only
when the package is not published there, one to GitHub's latest release; three seconds each,
`User-Agent: adage/<version>`, nothing else sent. The answer (or why there was none: `offline`,
`blocked`, `tls`, `timeout`, `unavailable`) is kept in `$ADAGE_HOME/cache/latest.json`, which is
what every surface reads. `adage config set update.check off` (or `ADAGE_UPDATE_CHECK=0`) stops it,
and `ADAGE_OFFLINE=1` stops every request. `adage update` itself asks when the kept answer is a day
old.

**The update.** Even when the binary is current, `adage update` refreshes a stale locally managed
Claude Code plugin and repairs registrations belonging to that installation. When everything
matches, it reports that there is nothing to do. When a newer release is available, it prints
its plan, then, in order:

1. **Keeps** the running `adage`, and the `adage-exec` and `adage-train` beside it, in
   `$ADAGE_HOME/previous/<version>/` with their SHA-256 (`previous.json` says where each came from).
2. **Runs** the channel's own command with the version pinned, with your terminal attached: a
   source install `cargo install --locked --git … --tag v<version> adage-cli adage-exec
   adage-train` (`--branch <name>` builds a branch instead; without `cargo`, `--from release`
   switches to the release's binaries); the release installer for that version, then `adage-exec`
   and `adage-train` from the same release, each checked against its `.sha256`; npm
   `npm install --global @adage-sh/cli@<version>` (never `@latest`); a checkout `git pull
   --ff-only` and `cargo build` in place.
3. **Verifies** that the binary the channel installed answers `version` with the version asked for,
   and, for npm and the release installer, that its SHA-256 matches the release's `SHA256SUMS` when
   the release publishes one (npm checks its package's integrity, the installer each archive's).
4. **Re-points** every hook registration of this install whose binary moved or whose events changed
   (`adage install` would write it differently), each with its diff and the previous file kept as
   `<file>.adage-bak`, after one question (`--yes` skips it). Inside an agent session it only lists
   them, each with the `adage install` command that rewrites it.
5. **Stages again** the Claude Code plugin `adage init` installed from this machine, and refreshes
   Claude Code's copy of it (`claude plugin marketplace update adage-local`).
6. Prints doctor's summary, the release's first notes, and `done`. The new binaries take effect in
   new sessions; running ones keep the binary they started with.

These steps after the package manager are run by the new binary (`adage update --finish`, which
`adage update` runs itself), so its own registrations and checks apply. A failed step stops the
update, leaves the kept binaries where they are and names `adage update --rollback`, which copies
them back byte for byte (each checked against its recorded SHA-256 first) and re-points the
registrations to them.

**Switching installations.** Installing through npm does not replace an older Cargo binary or
hooks registered to it. Check `adage --version` and run `adage doctor` from the new installation;
use its full path if your terminal still selects the older one. Run `adage update` from that
installation to refresh its locally managed Claude Code plugin. To move Codex hooks from another
installation, run `adage install codex --write --scope user` from the new binary, review the diff,
then approve the changed entries in Codex's `/hooks`. Use `--scope project` for hooks registered
only in that repository. Start a new Claude Code session after refreshing its plugin.

**Development versions.** `adage update --branch main` builds the main branch instead of a
published release. A checkout build needs no release.

**The published plugin.** A plugin installed with `/plugin install adage@adage` is updated by
Claude Code: from its binary, `adage update` runs `claude plugin marketplace update` and
`claude plugin update adage@adage` when `claude` is on `PATH` (the update takes effect in the next
session), and otherwise names the steps (`/plugin`, choose adage, then Update).

**Two versions never diverge silently.** `adage doctor`'s `versions` check asks every Adage binary
installed here (`PATH`'s, the plugin's, and each one a hook registration runs) for its version,
and warns when they disagree, with `adage update` as the fix when this one is the older. An Adage
that finds the index written by a newer one says so once a day at a session start (`adage: the
reflexes here were written by a newer adage; this one (0.1.0) stays quiet · adage update`), stays
quiet otherwise, and never conditions over that index.

### Removing Adage

To pause it instead, `adage off` turns it off in one repository and `ADAGE_DISABLE=1` everywhere.
To remove it:

```bash
claude plugin uninstall adage@adage-local          # the plugin adage init installed, with its data directory
claude plugin marketplace remove adage-local       # the local marketplace it came from
adage install codex --remove --scope user          # each hook file adage init wrote: the diff first, a backup kept
adage install opencode --remove --scope user
adage doctor                                       # any registration left
cargo uninstall adage-cli adage-exec adage-train   # the binaries
```

The published plugin is `adage@adage` (`/plugin uninstall adage@adage` in Claude Code). A
settings-file registration comes out with `adage install claude-code --remove` (`--scope user` for
`~/.claude/settings.json`). Your data stays in Adage's data directory (`~/Library/Application
Support/Adage` on macOS, `~/.local/share/adage` on Linux, or `ADAGE_HOME`), the staged plugin
under its `plugin/`; delete it yourself for a clean slate.

## What Adage learns

From real traces in a repository, one learner, a graph engine
([Track G](tracks/track-g-graph-engine.md)), finds the steps agents take together and
compiles them into reflexes. It has no list of kinds to look for: a reflex is whatever stimulus
reliably precedes a step, and how it is delivered follows from its shape. Reflexes it discovers
on real and generated repositories include:

| Reflex | Learned from | At runtime |
| --- | --- | --- |
| **Look the item up** | Agents passing a value from the prompt to the same command; the value's type (`W-2042`, `#482`, a UUID) is learned from the values themselves | fire/prime: "Work item details are looked up with: `python3 scripts/wi-lookup W-2042`" |
| **Read what it lists** | Agents reading, one turn at a time, the files a lookup listed | prime: "That result lists these files … read them together in one turn" |
| **Avoid a dead end** | A command that fails with the same signature, followed by one that works | inhibit: "Running `python3 -m unittest` fails (…). Use this instead: `make test`" |
| **Regenerate after an edit** | A regenerate step that follows edits to a path pattern | fire: after the edit, and once at Stop if still unmet: "changes to `schema/*.json` must be followed by `make gen`" |
| **Steps in order** | Commands run one after another after edits to a path pattern | fire/prime: "changes to `registry/*.csv` are followed by these steps, in order: `make gen`, `make check`" |
| **Check the part you edited** | A command naming the directory an edit touched | fire/prime: after an edit under `packages/ledger/`: `make -C packages/ledger check` |
| **Run the repository's checks** | The checks agents run after editing a kind of file, in the repo-wide form some of them ran (their `cargo test -p <crate>` and `cargo test --workspace --locked -j4` runs are one `cargo test --workspace --locked`) | prime: after an edit to `crates/**/*.rs`: `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo fmt --all` owed as a finish line |
| **Sync the environment** | A dependency step after a dependency file changes | prime: after review, once `requirements.txt` changes: `make deps then make check` |
| **Start like others did** | Prompts that mean the same thing whose sessions ran the same command first | fire/prime: "Requests like this one usually start by running: `make test`" (the embedding model is bundled) |

Every reflex carries its evidence (distinct traces, Wilson lower bound), an effect class that caps
how strongly it may be directed, and typed slot contracts: a value from the prompt reaches a directive
only if it matches the contract learned from real values (`^W-\d{4}$`), and file paths only if they
are safe, repository-relative, and exist.

**Procedure cards and the orientation.** A task the repository keeps repeating gets a procedure
card ([benchmarks](benchmarks.md#recurring-work-the-procedure-a-repository-repeats)): the files to edit, in order,
the commands that worked and those that failed, and the finishing check. When a card is
delivered, it now also says where its files stand in your checkout at that moment: each file's
line count, the last top-level definitions of a Python source with their line numbers, a test
file's last lines and a changelog's first headings. Adage reads those files itself, read-only, on
the CPU; no command runs. In exchange, the delivered card no longer quotes the lines added last
time (the learned card keeps them). It skips dotfiles, ignored and secret-holding files,
symlinks and files over 1 MiB, withholds any line that looks like a secret, and adds at most about
1,200 tokens. `ADAGE_CARD_ORIENTATION=0` delivers the earlier form. The card benchmarks
measured that earlier form; the orientation has been measured only in the one-task development
study described in the [README](../README.md#evidence-so-far).

## Tracks, packs and execution

Reflexes live in **tracks** on disk, and travel as signed, closed **packs** you can review, commit
and share. Instincts ship in the same format: Adage's base instincts are packs of the marketplace in
its own repository, built into the binary ([marketplace.md](marketplace.md)). By default, reflexes advise the agent and supported pre-call instincts can rewrite its
tool arguments; the harness executes those calls under its own permissions. The `adage` binary starts no process on the
agent's behalf (it starts only itself, to condition in the background, and `adage-train` when you
ask it to train; a test pins this). Execution is a separate, opt-in binary (`adage-exec`, turned
on with `adage delegate on`) that runs only reflexes you approved on that machine (`adage review
delegate`), inside an OS sandbox: E1 prefetches read-only results, while E2 stages actions and
applies only permitted outputs. Selecting and running such an invocation needs no model request;
the next model request may still consume its result.

The learner also retains **predictive execution candidates**: commands supported by recurring
edit or multi-event contexts across sessions, even when reminding the agent would add no value. They appear as needing execution approval
in review and stay silent without a valid local grant. Accepting their evidence with ordinary
`adage review approve` does not authorize execution. Their evidence travels in packs; your local
execution permission does not.

## Defaults and measurement

**Measurement is optional.** Controls are off by default. To reserve roughly one session in ten
as a quiet comparison, run `adage config set runtime.control 0.1`. Until a repository has 10 quiet
sessions and 30 with Adage, the card shows counts
of what happened, never dollars (`○ measuring: 4 of 10 quiet sessions so far`). After that
it shows the saving per session with a 95% interval, or "no measurable difference yet", or, if
that is what the sessions say, "Adage appears to add ~$x per session". Dollars are API-list
equivalents from a dated price table (subscription users do not pay per token); a model not in
the table is shown as tokens and turns only. `adage savings --json` has the whole estimate.

**What never happens without you:** nothing is ever run on the agent's behalf unless you turn on
delegation at a terminal; install steps (environment changes) wait for `adage review`; no hook
touches the network, and the background learner's one request is the daily check for a newer Adage
([Updating](#updating); `update.check = off` stops it); hook files change only when you run
`adage init`, `adage install … --write` or `--remove`, or `adage update` re-points a registration
(each shows the diff, asks before writing, keeps the previous file as `<file>.adage-bak`, and
leaves the rest of your settings as you wrote them).

**The one change Adage makes by default** (Claude Code): when a Python repository's own `.venv` was
verified to have pytest and the project, a test run or scratch repro the agent starts under the
system Python runs with `.venv/bin/python` instead. Only the interpreter word of a top-level
command changes, never text in a heredoc, a quoted string or a `-c` payload; a command that
activates an environment, `cd`s elsewhere, or pins another Python version is left alone. If your
permission rules allow the command as written but not rewritten, Adage advises instead of
rewriting. You see each rewrite, and the card counts them. A `pip install` of the project or its
test dependencies into the system Python gets advice naming the command that reaches the project;
nothing is refused. `adage delegate rewrites off` (or `adage config set runtime.rewrite false`)
turns this off, from a script too, and `adage delegate rewrites on` back on
([docs/execution.md](execution.md#rewrites-to-the-projects-environment-pretooluse-u11)).

## When Adage runs your checks (opt-in)

By default Adage only advises. You can let it run one learned test command itself, on the CPU,
inside an OS sandbox, on macOS or Linux. That takes three decisions, all yours, made at a terminal
(each refuses to run inside an agent session):

```bash
adage delegate policy --max-level e2    # 1. allow approved actions on this machine
adage review                            #    find the learned verification's track and reflex
adage review delegate TRACK REFLEX --level e2 --effect verify \
  --cache '.pytest_cache/**' --cache '.pytest_cache/.gitignore' --timeout-ms 20000 --repo .
                                        # 2. approve that one command (it is staged once first)
adage delegate on --repo .              # 3. let the hooks call adage-exec
adage delegate status --repo .          # registrations, approvals, and any that lapsed
```

`adage-exec` is the companion every install puts beside `adage` (and the Claude Code plugin
carries). Plain `adage delegate policy` shows this machine's execution policy and the file it is kept in
(`$ADAGE_HOME/policy.toml`). `--max-level e2` writes `max_level = "E2"`, `auto_approve = "off"`
and `require_sandbox = true` under `[exec]`, keeping the rest of the file; `--max-level e1` turns
actions off again. A managed policy can still cap it. The opt-in alone runs nothing: each command
needs its own approval. Which caches a command may write depends on the command; the
[first-reflex guide](first-reflex.md#enable-one-verification-reflex) walks through a pytest
example.

In a session where such an approval is current, Adage owns that verification:

- **At session start**, the agent gets a short, factual note: which command Adage runs (approved
  by a person, sandboxed), after which edits, and that it runs it again when the agent finishes if
  files changed since. Results come back with the agent's tool results, with the tests that ran; a
  failure comes back with the failing tests. Control sessions get no note, and neither does an
  approval that no longer matches.
- **After the agent's edits**, Adage runs the command and hands the agent a receipt: the result,
  exit code, time, a receipt id, how many tests pytest collected, and, by name, the tests the agent
  added in this session. Names come from pytest's own cache record, never from the output. If the
  agent then starts a test run that a fresh passing receipt already covers, it gets a note saying
  so. The note is a fact, never a refusal: the call still runs.
- **When the agent finishes** (Claude Code and Codex; not OpenCode), if the check ran earlier in the
  session and has not passed on the files as they are now, Adage runs it again. If it passes, the
  turn ends, and you see one line such as ``adage: ran `make test` at finish: passed (exit 0,
  0.1 s).`` If it fails, the agent is asked once to continue, with the failing tests named, in a
  message that begins `Adage finish-line check:` and says it comes from Adage, not from you. A
  second failure ends the turn with one line for you; the check runs at most twice per session. If
  Adage cannot confirm that a run completed, the agent is told not to retry, and actions in that
  worktree pause until you inspect it (`adage-exec journal --repo .`) and acknowledge it
  (`adage-exec recover --id <id> --acknowledge`).
- **When an approval lapses.** An approval is bound to the exact command, its scripts and, for
  project Python, the environment and interpreter. When one of those changes, the approval stops
  matching and nothing runs. Adage tells you, in one line the agent never sees (Claude Code; not
  when notices are off), at most once per session: which approval, what changed, and the
  `adage review delegate …` command that approves it again once you have checked the change.
  `adage delegate status` and `adage review` list every lapsed approval.

To stop, `adage delegate off --repo .` (approvals are kept), or `adage review revoke TRACK REFLEX
--repo .` for one approval. What this has shown so far: in one development study on one task,
sessions with it and the orientation card used fewer model requests than sessions without Adage;
on held-out tasks it saved more calls still but missed the registered quality rule, so it stays
experimental ([README](../README.md#evidence-so-far)). The full
mechanics are in [execution](execution.md#the-learned-finish-line-check-stop-gate).

## Commands

`adage --help` lists every command; `adage <command> --help` its options.

```bash
adage init             # set Adage up once: binaries, the Claude Code plugin and other hooks, repositories, first conditioning
adage update           # later: a newer Adage, the way it was installed; --rollback goes back
adage sense            # read-only: which reflexes this repository's traces would form
adage condition        # form them: read this repository's traces, bind instincts, compile the index
adage watch            # in a split pane: Adage's health, hook activity and conditioning, live
adage map              # local browser view, with review actions when started from your terminal
adage ui               # the full terminal app: reflexes, traces, map, health, settings
adage doctor           # check binary, data dir, index, tracks, model, hooks, kill switches
adage review           # evidence, advice previews, and routine candidates awaiting execution approval
adage world            # the situational awareness layer: what you were doing, for the agent (off by default)
```

Then, when you want to:

```bash
adage marketplace                                          # where packs come from: the adage marketplace, built in
adage packs                                                # base instincts and added packs: which apply here
adage packs off adage/lockfiles                            # stop an instinct pack, here and everywhere
adage share <track> --out <dir> --key ~/.ssh/id_ed25519   # a signed pack for your team
adage add <dir> --trust                                    # a teammate's pack, bound here at forming
adage delegate on                                          # let reflexes you approve act (adage-exec)
adage train .claude/skills/x/SKILL.md                      # compile a skill into reflexes, replayed and reviewed; installs nothing
adage train accept x                                       # install the reviewed pack
```

`init` and `update` are described [above](#installing-and-updating). `sense` reads the same traces
`condition` does and lists the reflexes `condition` would form as candidates, with prompts shown
only as short redacted snippets. With no history yet, plain `adage condition` inside a repository
binds available instincts from its files; `--instincts-only` requests that explicitly. `doctor` exits
non-zero only when a check fails. Every command ends with the next one to run; `--plain` drops
colour and glyphs, and `NO_COLOR` is honoured.

Manual `sense` and `condition` read the traces recorded in the repository they run in, as the
background learner does: Claude Code history (`CLAUDE_CONFIG_DIR/projects`, else
`~/.claude/projects`), Codex history (`CODEX_HOME/sessions`, else `~/.codex/sessions`) once Codex
hooks are registered, and configured OpenCode exports. `--all` reads every repository's; outside a
repository they ask for it rather than read everything. The `learn.*` source settings apply to
both; `learn.codex_sessions = ""` disables Codex discovery. Explicit `--transcripts`,
`--claude-projects`, or `--codex-sessions` replace defaults. History is read locally;
conditioning never executes its commands. `adage condition --json` prints the run as data: the
learner's report, how long it took (`elapsed_ms`), what changed with each reflex's gist
(`changes`), and what `adage review` has waiting; `--progress` adds each stage on stderr as one
JSON line (`{"progress": "read", "done": 12, "total": 262}`), which is how `adage ui` follows a run.

See [How Adage works](how-it-works.md) for the current behavior and limits, and
[live learning](live-learning.md) for discovery and background learning. Conditioning and
advisory hooks are automatic after setup. CPU execution of learned actions requires the separate
execution policy and local approval described in [execution](execution.md).

## The world

The world is the situational awareness layer: a second time series beside the agent's trace,
what *you* were doing. With it on, Adage records the meetings on your calendar, the transcripts
your meeting tool writes, the files you edited, the pull requests and tickets open in your name,
and tells the agent what a prompt like "file the follow-ups from the marketing sync" refers to,
at the prompt, before the model spends tool calls finding out. It is off by default.

```bash
adage world on                                   # a person's decision: record what you do
adage config set world.transcripts ~/Meetings    # where Granola, Zoom or Gong write transcripts
adage config set world.roots ~/src,~/notes       # directories whose recent edits count
adage world                                      # sources, artifacts, the calendar, activity
adage world eval record "review the PR" --meant github:pr:412   # label what you meant, fifty times
adage world eval report                          # hit@1 and false attaches on your prompts
adage world pause                                # stop every source and every hint; resume starts them
```

**What arrives, and from where.** Each source is a descriptor, a TOML file (`packs/world/`, or
your own under `$ADAGE_HOME/packs/world/`), never code. Local sources (`meetings`, `files`,
`desktop`) run on this machine in `adage world follow`, which the Claude Code mod starts for a
session's life. Connector sources (`calendar`, `github`, `tickets`) run inside the Claude Code
mod through the MCP servers you have already connected to Claude Code (a claude.ai connector, a
`claude mcp add` server): Adage holds no token and signs in nowhere; disconnecting the server in
`/mcp` stops the source. Codex and OpenCode sessions read the same store and get the same hints,
from the local sources alone.

**What the agent reads.** One `<adage-world kind="hint">` block after the reflexes' text: the
likely reference (a meeting, its transcript's path, a pull request, a ticket), a summary of at
most 300 characters made locally, and a line saying it is from Adage. When two or three things
could be meant, a `disambiguation` lists them and the agent asks in one line. Nothing when the
prompt names its own file or URL, when nothing scores high enough, and in a control session.

**What is kept.** Titles, paths, keys, a short summary and timestamps, under `$ADAGE_HOME/world/`;
never pixels, keystrokes, audio or an artifact's content beyond its summary. `world.deny` drops
password managers, banking, HR and anything that looks like a secret before it is written.
Activity is kept `world.retention_days` (14), artifact summaries six times as long, and the
labels the resolver learns from (which hints the agent used) indefinitely; they hold no content.
`world.desktop` (off) adds the active window's title, sampled every five seconds.

## Watch it live

`adage watch` (or `adage doctor --watch`) is a compact live pane for a split terminal. It works in
60×20 and grows with the pane:

```text
 adage watch  harbor-dispatch · gen d103fe72 · demo   last call 06:50:35   LIVE
 ◠ 6 fired · ✓ 1 followed ━───── · ■ 2 finish lines · 13 calls     ▁▁▁▁▁▁▁▁▁▁▁█

  ┊ ↑ earlier: 2 sessions · 3 turns · 7 steps
  Claude Code  session, since 06:50 ────────────────────────────────────────────
  ├ List                                                                   06:50
  ├ Read                                                                   06:50
  ├ Run  npm test                                              ✗ 1 failed  06:50
  │  ◆ adage · INHIBIT · when npm test fails, run npm run check, not npm test
  │    ──▶ npm run check
  ├ Run  npm run check                                                     06:50
  ├ Edit                                                                   06:50
  │  ◆ adage · FIRE · after editing registry/*.csv, run npm run codegen
  │    ──▶ npm run codegen  ✓ finish line met
  ├ ◆ adage ■ finish line: asked the agent to go on before it stops        06:50
  ├ Run  npm run codegen                                                   06:50
  │  ◆ adage ✓ finish line met: npm run codegen
────────────────────────────────────────────────────────────────────────────────
 ◆ conditioned 06:50 · 27 traces · 2 changes                        l  all runs
 ○ 4 to look over · adage review
 + claude hook received    - codex not used here    + 4 reflexes
 + auto-condition    + bundled model
────────────────────────────────────────────────────────────────────────────────
  :  palette   p  pause   f  scope   /  filter   enter  inspect         q  quit
```

(`adage watch --demo --once --size 80x24` under `NO_COLOR`; in colour the strengths are lamps,
followed responses are green and the newest step glows.)

- **Pulse:** what this session (or all of them) came to: reflexes fired, by strength; how many
  the agent followed, as a meter; finish lines met and reminders sent; tool calls; and activity
  per minute, the minutes a reflex fired marked. A heartbeat lamp blinks on each hook call. From
  90×26 these are tiles, and from 120×40 their numbers are large.
- **Session flow:** the session drawn as it grows. Each turn is a box (a prompt, its steps and
  calls, its tool mix), its tool steps hang from a trunk in plain words (`Run ×3 · Read`, a single
  command spelled out, failures and subagents tagged), and each reflex Adage delivers branches off
  the step it came after as a `◆ adage · FIRE` box, pointing to the response it asked for: green
  once the agent follows it (`✓ followed`, `✓ headed off`, `✓ finish line met`), red when it
  does not. Finish lines, rewrites and answers from a receipt are `◆ adage` leaves. Three or more
  quiet steps fold into one line, and so does a turn where Adage had nothing to say. Where the
  flow is narrower than 88 columns each node is one line, as above. It follows the newest; `↑`/`↓` scroll, and `G` comes
  back.
- **Learning, review and health:** the last conditioning (traces used, what changed, the
  patterns forming and how close the nearest is to becoming a reflex), how many reflexes wait for
  your review, and lamps re-checked every few seconds: hooks that run for this repository (a
  harness set up but not used here is dimmed), its reflexes in the index, live conditioning
  (`auto-condition`), the embedding model, recent fail-open errors; each problem names the command
  that fixes it. From 120 columns these are a rail beside the flow, with the reflexes in play this
  session (fired and followed, as meters); `l` lists every conditioning run.

Keys: `q` quits, `p` pauses (a banner counts what arrives meanwhile), `f` switches between this
repository and all of them, `/` filters by trace, reflex or command, `Enter` inspects (the exact
directive text and its evidence), `m` opens the map in your browser, `v` hides events where Adage
said nothing, `l` lists the conditioning runs, `↑`/`↓` scroll (`G` back to the newest), `?` lists
every key. Motion (the heartbeat, a new step sliding in, a newly fired reflex flashing once) is
off under `ADAGE_REDUCED_MOTION=1`. `NO_COLOR` is honoured: strengths are then spelled out
(`FIRE`, `PRIME`, `INHIBIT`), and `--plain` drops glyphs too.

`adage ui` is the full-screen app (plain `adage` prints the status card above). It opens on
Adage's brain, the site's, turning in shaded text for about two seconds before dissolving into
the app; any key skips it (and any key but `Enter`, `Space` and `Esc` goes on to the app).
`ui.start_screen = false` (or `ADAGE_START_SCREEN=0`) turns it off, and it is never shown away
from a terminal, in an agent's session, below 60×20, with `ADAGE_REDUCED_MOTION=1`, or when
`adage watch` hands over to the app. `:` or `ctrl-k` opens a command palette over every action
and reflex; `?` lists the keys. Its tabs:

| Tab | What it does |
| --- | --- |
| **Home** | setup progress, repository selection, history sources, conditioning, and the next useful action; when a newer Adage is out, its version and `adage update` first |
| **Live** | the watch view, larger: the pulse tiles, the session flow, and a rail with the reflexes in play, learning and health; `m` opens the map in your browser, as in `adage watch` |
| **Reflexes** | every instinct and reflex by track, with a fuzzy `/` filter and a brief summary and trigger → response graph below the list. Press `Enter` to open a scrollable review dialog with evidence, the exact directive, and the available decisions. Each decision previews its effect before confirmation and shows the saved result |
| **Traces** | recorded Adage activity by session, with complete recorded session details and the latest conditioning time and counts shown separately. Press `c` to preview history and condition the selected repository |
| **Map** | stimulus → reflex → response as a layered graph in the terminal; `adage map` opens the same in the browser |
| **Health** | every doctor check, live, including live conditioning's. `Enter` fixes the selected one where a key can: register hooks (confirming the exact file and diff), fetch the model (with progress), condition this repository now (a card first shows the sources it reads and what they hold; then its stages tick as it runs, in the background if you press `esc`; then a result card says what it did in plain words, `v` for the details and `enter` for the next step; it runs `adage condition --repo-root` without changing the background allowlist), turn live conditioning on, open the settings, or open the fail-open log |
| **Settings** | every `adage config` key with its value and where it came from (default, file, or environment); `Enter` edits, `Space` toggles, and `adage config set`'s validation and agent-session refusal apply |

`adage watch --demo` and `adage ui --demo` replay a small scripted scenario (three sessions of the
harbor-dispatch demo, through real hook calls) into a temporary data directory, so you can see it
all without an agent running. `adage watch --once [--size 80x24]` prints a single frame as text.
Decisions, installs and settings refuse inside an agent session, as their commands do.

The palette also opens guided forms for local OpenCode history import, history directories,
reports, model management, and signed-pack import/export. Pack import and reflex review confirm
exactly the content previewed; a changed file requires a new review. The browser action starts
a local server owned by the TUI and opens its map, evidence, traces, or setup view. Trace links
open the complete recorded session beyond the recent activity window, with historical versions
identified separately. Closing the TUI stops its server.

The browser's **Review** actions use the same decisions and revision checks as the terminal.
Start `adage map` or `adage ui` yourself in a terminal to enable them. An agent-started or
noninteractive map remains read-only and shows the equivalent terminal command. Reviewing
advice never grants permission to execute commands.

**Why the session counts differ:** recorded activity contains decisions received by Adage's
hooks. Conditioning reads agent history, including supported subagent transcripts that may
have no hook recording. The latest conditioning count describes one run; repeated runs read
overlapping history, so their session counts are not added together. A missing count means
the older run did not record it. The recorded sessions list is not a list of every transcript
the learner can read.

In **Traces**, press `h` for **History sources** or `c` for **Condition**. If the selected
repository's checkout is not open, a form asks for its folder and checks that it matches your
selection. With all repositories selected, choose the one checkout to condition. A preview
shows the available files before you confirm learning. These are file counts, including
supported agent and subagent histories; the learner determines usable sessions and independent
evidence. A trace containing Adage advice may still contribute unrelated unsteered work.
After conditioning finishes, the view refreshes the latest run's time and counts.

Health distinguishes hooks registered from hooks actually received in the selected repository.
Codex's `/hooks` acceptance happens inside Codex; its guidance stays visible until a hook arrives.
History preview and conditioning use the same repository-scoped sources. With no usable history,
conditioning can still bind enabled instincts from repository files.


## Claude Code

`adage init` installs the Adage plugin into Claude Code for you ([above](#first-run-adage-init)):
`claude plugin list` then shows `adage@adage-local` at user scope, enabled, read from
`$ADAGE_HOME/plugin/adage`. Claude Code reads it in place, so `adage init` again, or `adage update`,
stages new binaries and modules there and the next session runs them.
`/plugin install adage@adage` installs the released plugin, which Claude Code updates. To develop
the plugin itself, load it from a checkout with `--plugin-dir`
([by hand](#if-you-prefer-to-do-it-by-hand)).

It is one plugin with three binaries. At every session start its shim checks each against
`dist/SHA256SUMS`, copies it into the plugin's data directory as `bin/<name>-<sha256>`, and links
`bin/adage`, `bin/adage-exec` and `bin/adage-train` to those copies and `bin/hook` to `adage-exec`
once `adage delegate on` has been set, else to `adage`. Every hook runs `bin/hook` by that absolute
path, so no `adage` on `PATH` is needed; with no bundled binary for the machine, the shim falls back
to the one on `PATH`. A Codex or OpenCode registration written by the plugin's own binary names
its stable link `bin/adage`, so a plugin update carries it.

### What the plugin shows in the session

The plugin carries a module that acts where the command hooks cannot, and draws beside the
conversation, never in it (the model reads nothing it draws). What it does: a test run a fresh
receipt already covers is answered with the receipt instead of running, only when Claude Code's
own permission rules would have let the call run unasked (the receipt card then stands in for
pytest's output); a rewrite to the project's interpreter, or a learned replacement for a dead end,
runs only when the rules treat both commands alike, and the binary's advice reaches the model
otherwise; the repository's standing reflexes sit once in the cached system prompt, the project's
environment note on the Bash tool's description, and a subagent is told the conventions its task
binds before its first request; a step the binary marks routine goes to the model
`runtime.routine_model` names, at low effort, when you set one. What it draws:

- the opening line, with Adage's glyph: `◠ adage · 3 reflexes and 4 instincts active in this repo; …`;
- under a prompt a reflex fired at, one dim line per reflex, worded as what it tells the agent;
- under a tool row, a marker when Adage pointed a test run at the project's interpreter, took the
  step without a model request, ran a learned replacement, or the agent headed off a dead end; a
  receipt that covered a run says so under its output;
- one band above the prompt while there is something to say: reflexes this session, steps without
  a model request, the health lamp and its word, learning; a second line only when something
  needs you (a grant that lapsed, a problem, reflexes waiting for review, a newer Claude Code);
- a toast when learning changed a reflex, and a status-line warning only when something is wrong
  (no binary for this machine, hooks not answering);
- with `ui.noticed` on, under the tool batch that completed a sequence Adage has counted in
  earlier sessions, `○ adage noticed · make test after editing acme/** · seen in 3 earlier
  sessions` (or `· waits for your review · /adage review`), once per sequence per session, never
  for a single call; the band counts them (`1 pattern noticed`) while the pane is closed;
- under a Skill tool row, `◈ adage loaded the release-checklist skill · no model request` when
  Adage made the call in the model's place, and `◈ adage verified 3 of 5 steps here · step 2
  stale · /adage why` when `adage train` verified the skill here: the skill's text reaches the
  model with those steps listed under a fixed heading, and `/adage why` lists each step.

**Adage asks.** When a reflex waits for a decision, Adage asks at the end of the turn, in the
dialog Claude Code uses to ask a question: `Adage learned: after editing acme/**, run make test.
Tell the agent?` with `enable` / `not this one` / `why`; `Adage can run make gen for you after a
schema edit, instead of asking the agent to. Allow it?` with `see what it would do` / `not now`;
`6 reflexes from alice@acme.com are in this repository. Use them?` with `use them` / `not now` /
`why`. `see what it would do` opens a pane with three plain lines (`it runs`, `it changes` with
the diff, `it never touches anything else, or the network`) and `a: allow`, `n: not now`,
`d: details`; `allow` confirms against the digest the stage produced, and if the repository moved
meanwhile the pane shows the fresh diff and asks again. At most one ask per turn end, one per
reflex per day; dismissing the dialog brings it back once, a day later; `not this one` records a
dismiss. Every answer runs through the press: a single-use token the binary issues once at the
session's start (`adage hook claude Press`, before the agent's first act) and the mod holds in
memory; the verb reads `{ press, session }` on stdin, records the decision as a person's (`via:
press`), and prints the next token. The model cannot press (it can only call tools) and a Bash
command it is talked into running gets no token. With no token held (the window was missed, the
mod reloaded mid-session), the dialog still asks and the answer is kept for the next session start
in the same repository; the band says `» kept for your next session · /adage review`. That start
asks again, naming the exact command (`Kept from your last session: … Run it now?`, `run it` /
`drop it`), and only that answer runs it.

`/adage` prints the status card and opens the pane: **Live** (`l`, the feed of `adage watch`, a
filter, `enter` to read a directive as the agent read it, `p` pause, `v` shows the rows where nothing was decided, hidden by default, `n` newest),
**Reflexes** (`r`, this repository's only: a **suggestions** group first, under a plain heading
per kind, each sentence whole with its two answers beneath; then every reflex by its step and
when, with its strength, stage and evidence; an expanded row has `e enable`, `n not
this one` and `d details`, where the four more verbs and the equivalent `adage review` command,
with `c Copy`, wait) and **Health** (`t`, every doctor check; `enter` conditions the repository
when a check asks for it, any other fix is a command to copy). Every tab ends with **Explore in
browser**, which starts `adage map` once and links to it. `/adage why` shows the last directive
of this session (or the verified skill's steps), `/adage off` turns Adage off in this repository,
`/adage on` opens the ask `Adage is off in this repository. Turn it on?`, `/adage pause` pauses the
feed and the band, and `/adage watch`, `reflexes`, `review` and `health` open the pane on that
tab. In `/config` the plugin has two rows: **Band above the prompt** (on) and **Watch pane**
(`manual`; `auto` opens the pane unasked in a terminal 144 columns wide); there is no switch for
asking: a decision is never hidden behind a setting. Nothing is drawn or asked in `claude -p`, the
SDK or the VS Code panel, where `/adage` answers as text. `ADAGE_ASCII=1` draws every glyph as
its ASCII twin.

## OpenCode and Codex

`adage init` writes both harnesses' hook files for you when it finds them, each shown as a diff
first, and says once for Codex to approve the new hooks in its `/hooks`. Codex keeps learning from
its own history once its hooks are registered. By hand:

```bash
adage install opencode            # print the plugin; add --write to install it
adage install codex               # print a hooks.json; add --write to merge it
opencode export <session-id> > s.json && adage condition --transcripts s.json
adage condition --codex-sessions ~/.codex/sessions   # Codex rollouts
```

`--write` installs into the repository (`.opencode/plugins/adage.js`, `.codex/hooks.json`) or, with
`--scope user`, the harness's documented user location; it never runs inside an agent session.
`adage install claude-code` does the same for Claude Code settings. A harness already registered for
the repository (in the other scope, or by the Claude Code plugin) is not registered again, since a
second registration runs every hook twice; `adage doctor` flags one made by hand. To remove Adage's
hooks, `adage install <harness> --remove [--scope user]` shows the diff, keeps `<file>.adage-bak`, and
leaves your own hooks and settings as they were. What each harness supports, and
what it cannot do yet (Codex reports no tool failures to hooks, so only reflexes that react to a
failed command stay silent there; learned dead ends are announced at the first prompt), is in
[docs/harnesses.md](harnesses.md).

`adage status` shows every track and reflex with its strength and evidence; `adage report` summarises
directives emitted, followed, and ignored. Data lives in `~/Library/Application Support/Adage`
(macOS), `~/.local/share/adage` (Linux) or `%APPDATA%\Adage` (Windows); `ADAGE_HOME` overrides it.

**Kill switches:** `ADAGE_DISABLE=1`, or a `.adage/disabled` file in a repository: `adage off`
writes it (allowed anywhere, inside an agent session too) and `adage on` removes it (a person's
decision, at a terminal).

**Live conditioning:** you do not have to run `adage condition` again. When an agent's session stops or
ends, the hook starts a background learner for that repository (detached; the hook never waits,
and never says anything different). It conditions at most once per `learn.min_interval_secs` per
repository, one learner at a time, and appends each reflex that changed (new, extinguished, prime →
fire, evidence, awaiting review) to `$ADAGE_HOME/learn.log`. `adage doctor` shows the last run
per repository. The file contract for tools that follow it live is
[docs/live-learning.md](live-learning.md).

**Settings:** `$ADAGE_HOME/config.toml`, shown and changed with `adage config`:

```bash
adage config                                  # every key, its value, and where it came from
adage config set learn.auto false             # stop live conditioning (ADAGE_AUTO_LEARN=0 also works)
adage config set learn.thresholds demo        # lower evidence bars, for small histories
adage config set learn.min_interval_secs 300  # condition each repository at most every 5 minutes
```

| Key | Default | |
| --- | --- | --- |
| `learn.auto` | `true` | Learn automatically when sessions stop or end (`ADAGE_AUTO_LEARN` wins) |
| `learn.min_interval_secs` | `120` | At most one automatic learn per repository per interval |
| `learn.thresholds` | `default` | `default` or `demo`, for `adage condition` too (`--demo` wins) |
| `learn.claude_projects` | `""` | Claude Code history (`""` = `~/.claude/projects`) |
| `learn.codex_sessions` | `auto` | Codex rollouts: `auto` reads `$CODEX_HOME/sessions` (this repository's only) once Codex hooks are registered; a directory; or `""` for none |
| `learn.opencode_exports` | `""` | A directory of `opencode export` files automatic learning reads |
| `learn.repos` | `all` | The repositories whose history is read, by key (`adage init` picks them); sessions elsewhere are recorded, not read |
| `learn.parse_cache` | `true` | Keep parsed transcripts in `cache/sessions` (owner-only), so re-learning parses only what changed |
| `learn.cards` | `true` | Procedure cards for recurring tasks (embedded with the installed or bundled model) ([docs/training.md](training.md#recurring-tasks-procedure-cards)) |
| `learn.card_threshold` | `0.811` | A prompt's best similarity to a family's prompts that delivers its card (strict: it recognises phrasing close to earlier prompts, not paraphrases) |
| `instincts.enabled` | `true` | Bind Adage's instincts from each repository's files |
| `instincts.packs` | `auto` | The instinct packs to bind: `auto` (default and added packs), explicit pack names, or both; optional built-ins need an explicit name |
| `instincts.disabled` | `""` | Packs (`adage/lockfiles`) or instinct ids (`U3`) never to bind; `adage packs off` writes it |
| `ui.notices` | `daily` | One-line notices at session start (Claude Code), at most daily and only when new; `off` silences them |
| `ui.start_screen` | `true` | `adage ui` opens on Adage's brain, turning, for about two seconds; any key skips it (`ADAGE_START_SCREEN` wins) |
| `update.check` | `daily` | After a background conditioning run, at most once a day, ask whether a newer Adage is out (one request, nothing sent but the request); `off` stops it (`ADAGE_UPDATE_CHECK` wins) |
| `runtime.control` | `0.1` | Control share (`ADAGE_CONTROL` wins) |

Keys that change what Adage learns from or how much evidence it needs (`learn.thresholds`, the
three source directories, `instincts.packs`) are a person's decision: `config set` asks for a
terminal and refuses inside an agent session. Every other key only tunes Adage or turns something
down, and can be set from a script, CI or a sandboxed session.

**Embeddings and intent families (nothing to do):** the `adage` binary bundles the static
embedding model potion-base-8M, quantised to int8 (7.9 MB, SHA-256 pinned), so `adage condition`
clusters each repository's prompts into intent families (exact kNN + deterministic Louvain, split
and merged by what the sessions did next — spike S7) and forms an intent reflex per usable family,
with no download. `adage models fetch [potion-base-8m | potion-code-16m-v2]` installs a model's
pinned upstream files instead (from Hugging Face, else the byte-identical npm package; every file is
checked against its SHA-256), `adage models use <key|auto|hashed>` chooses, and `adage models
status`, `adage doctor` and the status card (`embeddings: …`) say what is in use. With no model at
all the explicit hashed fallback is shown, intent reflexes cannot fire, and everything else works
unchanged. What embeddings are and are not worth on replay: [evals/launch/embeddings.md](../evals/launch/embeddings.md).

**Sharing:** `adage share <track> --out <dir> [--key <path>]` converts a track to a **pack** (the
only format that travels; [docs/packs.md](packs.md)), checks every byte for absolute paths,
home-directory paths and secret-looking strings, re-validates it as a recipient would, and signs its
digest (`sshsig`, namespace `adage-pack`; ssh-agent first, else the key file, whose passphrase is asked
for at the terminal). `adage add <dir>` verifies the
signature against `$ADAGE_HOME/trusted-signers` or the repository's `.adage/trusted-signers` (approved
once with `--trust` and pinned by hash in `$ADAGE_HOME/trust.json`) and installs the reflexes
forming, never fire, under `tracks/@imported/` where `adage condition` never writes (beside the base
instinct packs, which the marketplace built into Adage pins, so they need no signature); if you later condition
the same reflex yourself, your first-hand reflex wins. Adding a newer version keeps your own review
decisions for every reflex that survives. An unknown signer needs `--trust`. Both commands refuse to run inside an agent
session (`CLAUDECODE`) or without a terminal; `ADAGE_ALLOW_NONINTERACTIVE=1` lifts only the terminal check.

**Review:** `adage review [--json] [--verbose]` lists the reflexes that need a person's decision: new reflexes from
`adage condition`, reflexes whose strength or evidence changed materially since the last decision (shape,
strength, a Wilson lower bound moving by 0.10, or support doubling or halving), imports forming,
extinguished reflexes that came back stronger, and reflexes agents keep ignoring in treated traces. Each
entry shows its trigger, evidence (in plain counts; `--verbose` adds the bounds, uplift and the graph
engine's reasons), effect and ceiling, and the exact directive text with example slot values.
Instincts in effect are listed once per instinct and step, with what triggers each, and wait for no
decision; an execution-only routine waits once, for its evidence, and is then listed as ready to
delegate.
A pinned reflex is never overwritten: when learning (or a re-import) finds a materially different
version, it waits as *proposed*, with a diff; `approve` applies it (still pinned) and `dismiss` keeps
yours. `adage review <approve|extinguish|weaken|strengthen|pin|dismiss> <track> <reflex> [--by <name>]` records the decision
in the track's `review.yaml`, re-validates the track, and recompiles the index. Strengthening never exceeds
the effect ceiling, and an import cannot be strengthened to fire without first-hand evidence learned
locally. `adage condition` keeps extinguished reflexes out of service until they strengthen materially, and holds
weakenings and strengthenings. An extinguished reflex also keeps a teammate's import of the same
reflex out of the index. Decisions refuse to run inside an agent session. Listing is allowed there.

**Approval and color mean different things.** Approve accepts the reflex as it stands; it
does not add evidence or turn every suggestion green. Prime (yellow) means the agent receives
a suggestion, while Fire (green) recommends it more strongly. Some instincts, including
environment changes, stay at Prime because of their effect ceiling. The review view shows
whether advice is available and whether your approval still matches the reflex. Strengthen
appears only when that change is permitted. Automatic execution requires its separate local
grant even after a reflex is approved.

**Optional comparison:** `adage config set runtime.control 0.1` reserves roughly 10% of new
sessions as quiet controls. Adage records what it would have said but delivers no intervention.
The default is `0` (off); `ADAGE_CONTROL` overrides the setting. Observed delivery counts alone
are not evidence of savings.

## The demo

`demo/` runs real headless Claude Code agents on a sandbox repository whose conventions an agent
cannot guess (work items only via a tracker CLI; tests only via `make test`; `make gen` required
after schema edits), with twelve work items and hidden checks.

```bash
python3 demo/run.py all                  # train (no Adage) → condition → held-out eval, two arms
python3 demo/dashboard/serve.py          # live dashboard at http://127.0.0.1:8766 (adage map keeps 8765)
```

For your own repositories, `adage map` opens the live map of instincts and reflexes in the
browser; it is served by the binary, no Python needed.

See [demo/SANDBOX.md](../demo/SANDBOX.md). `python3 demo/sandbox/selfcheck.py` proves every work item
fails untouched and passes with its reference solution.
