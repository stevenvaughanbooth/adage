# Using Adage

Install channels, every command, the live views, settings, sharing and review. The [README](../README.md) has the quick path; [How Adage works](how-it-works.md) is the behavior reference.

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
and share. By default, reflexes advise the agent and supported pre-call instincts can rewrite its
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

**What is measured, honestly.** Adage stays quiet in 1 session in 10 (the control), so its effect
can be measured. Until a repository has 10 quiet sessions and 30 with Adage, the card shows counts
of what happened, never dollars (`○ measuring: 4 of 10 quiet sessions so far`). After that
it shows the saving per session with a 95% interval, or "no measurable difference yet", or, if
that is what the sessions say, "Adage appears to add ~$x per session". Dollars are API-list
equivalents from a dated price table (subscription users do not pay per token); a model not in
the table is shown as tokens and turns only. `adage savings --json` has the whole estimate.

**What never happens without you:** nothing is ever run on the agent's behalf unless you turn on
delegation at a terminal; install steps (environment changes) wait for `adage review`; no hook or
learner touches the network; settings files change only when you install the plugin or run
`adage install … --write` or `--remove` (which show the diff, ask before writing, keep the previous
file as `<file>.adage-bak`, and leave the rest of your settings as you wrote them).

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

`adage-exec` is a separate binary (`cargo install --locked --git … adage-exec`). Plain
`adage delegate policy` shows this machine's execution policy and the file it is kept in
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
it has not been tested on held-out tasks yet ([README](../README.md#evidence-so-far)). The full
mechanics are in [execution](execution.md#the-finish-line-check-stop-gate).

## Other ways to install

Today, only building from source works (it needs access to the private repository):

```bash
cargo install --locked --git https://github.com/stevenvaughanbooth/adage adage-cli
# or, from a checkout of this repository:
cargo install --locked --path crates/adage-cli
```

**Not published yet:** everything below waits for the first tagged release; none exists yet.
Once it is out, for macOS (arm64, x64), Linux (x64, arm64; static) and Windows (x64):

```bash
# not published yet: these URLs, the tap and the npm package do not exist until the first release
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/stevenvaughanbooth/adage/releases/latest/download/adage-cli-installer.sh | sh
powershell -ExecutionPolicy Bypass -c "irm https://github.com/stevenvaughanbooth/adage/releases/latest/download/adage-cli-installer.ps1 | iex"
brew install stevenvaughanbooth/tap/adage
npm install -g @adage-sh/cli
```

The release plugin is designed to carry checksummed binaries for macOS (arm64, x64), Linux (x64,
arm64) and Windows (x64). No release has been built yet, and native Linux and Windows runs remain
unverified. Native Windows hook invocation is untested, and automatic execution requires macOS or
Linux (including WSL).

Three binaries: `adage`; `adage-exec`, which runs the reflexes you let act (`adage delegate`;
Linux and macOS); and `adage-train`, which compiles a skill into reflexes (`adage train`). From
source, install each on its own: `cargo install --locked --git … adage-exec` (and `adage-train`).
After the first release, the npm package will carry all three, and the other channels will install
each on its own: `adage-exec-installer.sh` and `adage-train-installer.sh` beside
`adage-cli-installer.sh`, or `brew install stevenvaughanbooth/tap/adage-exec` (and `adage-train`).
Put them beside `adage` or on `PATH`.

The release workflow is built so that every release archive will have a SHA-256 checksum, a
Sigstore build-provenance attestation (`gh attestation verify <file> --repo
stevenvaughanbooth/adage`) and a CycloneDX SBOM, and so that it publishes the Claude Code plugins,
with those binaries bundled and checksummed, to the `marketplace` branch that `/plugin install`
reads. It has not run on a tag yet. The plugin checks its binary against `dist/SHA256SUMS` at every
session start, falls back to `adage` on `PATH`, and otherwise says in one line what to install.

## Commands

`adage --help` lists every command; `adage <command> --help` its options.

```bash
adage init             # guided setup at a terminal: hooks, model, first conditioning, live conditioning
adage sense            # read-only: which reflexes your traces would form, per repository
adage condition        # form them: read your agents' traces, bind instincts, compile the index
adage watch            # in a split pane: Adage's health, hook activity and conditioning, live
adage map              # the map of your reflexes in the browser (read-only, loopback only)
adage ui               # the full terminal app: reflexes, map, health, settings
adage doctor           # check binary, data dir, index, tracks, model, hooks, kill switches
adage review           # evidence, advice previews, and routine candidates awaiting execution approval
```

Then, when you want to:

```bash
adage share <track> --out <dir> --key ~/.ssh/id_ed25519   # a signed pack for your team
adage add <dir> --trust                                    # a teammate's pack, bound here at forming
adage delegate on                                          # let reflexes you approve act (adage-exec)
adage train .claude/skills/x/SKILL.md                      # compile a skill into reflexes, replayed and reviewed; installs nothing
adage train accept x                                       # install the reviewed pack
```

At a terminal, `init` is a wizard: it detects your harnesses and, one step at a time, asks before
it registers hooks (showing the exact file and diff), offers to fetch an embedding model only when
none is available (one is bundled in the binary, so usually there is nothing to fetch), conditions
the repository for the first time, and turns automatic conditioning on if it is off; it ends with the doctor's
summary. Piped, inside an agent session, or with `--print`, it prints the same report and writes
nothing; `--yes` binds this repository's instincts (and fetches the selected model only when none
is available), but registers no hooks (it ends by saying whether they are registered;
`adage install <harness> --write` registers them). `sense` reads
the same traces `condition` does and lists the reflexes `condition` would form as candidates, with
prompts shown only as short redacted snippets. With no history yet, plain `adage condition`
inside a repository binds available instincts from its files; `--instincts-only` requests that
explicitly. `doctor` exits
non-zero only when a check fails. From a source checkout, `cargo build --release --locked -p
adage-cli -p adage-exec -p adage-train` builds all three into `target/release/`. Every command
ends with the next one to run; `--plain` drops colour and glyphs, and `NO_COLOR` is honoured.

Manual `sense` and `condition` discover existing Claude Code history (`CLAUDE_CONFIG_DIR/projects`,
else `~/.claude/projects`), Codex history (`CODEX_HOME/sessions`, else `~/.codex/sessions`), and
configured OpenCode exports. The `learn.*` source settings apply to both; `learn.codex_sessions = ""`
disables Codex discovery. Explicit `--transcripts`, `--claude-projects`, or `--codex-sessions`
replace defaults. The wizard conditions only the selected repository; plain `condition` processes
all discovered repositories. Automatic Codex discovery requires registered Codex hooks or a Codex
hook request. History is read locally; conditioning never executes its commands.

See [How Adage works](how-it-works.md) for the current behavior and limits, and
[live learning](live-learning.md) for discovery and background learning. Conditioning and
advisory hooks are automatic after setup. CPU execution of learned actions requires the separate
execution policy and local approval described in [execution](execution.md).

## Watch it live

`adage watch` (or `adage doctor --watch`) is a compact live pane for a split terminal. It works in
60×20 and grows with the pane:

```text
 adage watch  harbor-dispatch · demo           LIVE  FIRE 4  PRIME 1  INHIBIT 1
─ now ──────────────────────────────────────────────────────────────────────────
 04:22:53 bc0c7d cc  tools    ✗ ignored lookup-incident
 04:22:53 19b2cf cc  stop     ○ would ask the agent to continue (withheld)
 04:22:53 d76f0f cc  tools    → ◠ codegen-after-regist… FIRE       npm run code…
 04:22:53 bc0c7d cc  failure  → ◠ dead-end-npm-test     INHIBIT    npm run check
 04:22:53 d76f0f cc  tools    ✓ finish line met codegen-after-registry
 04:22:53 bc0c7d cc  stop     ■ finish line: asked the agent to continue
─ conditioning ─────────────────────────────────────────────────────────────────
 ◆ 04:22 auto-condition · 27/28 traces used · 0.73 s · 2 changes
   ◠ read-linked-files   prime → fire       13/17 · lb 0.53
   ◠ lookup-incident     evidence           24/25 → 25/26
   ○ 4 reflexes wait for review · adage review
─ health ───────────────────────────────────────────────────────────────────────
 + hooks claude    + 4 reflexes    + auto-condition
 + embedding model bundled    + no fail-open errors
────────────────────────────────────────────────────────────────────────────────
 p pause · f scope · / filter · enter inspect · m open map · ? keys      q quit
```

(`NO_COLOR` output, as above; in colour the strengths are lamps.)

- **Now:** every hook call as it happens, from the trace spools: time, session, harness, event,
  and what Adage did: a directive (→, with its strength: fire, prime or inhibit), withheld in a
  control session (○), followed ✓ or ignored ✗, a finish line met, or a stop request. `Enter`
  inspects the selected row: the exact directive text and its evidence.
- **Conditioning:** what conditioning changed, from `learn.log`, manual or automatic, and how many
  reflexes wait for your review.
- **Health:** lamps re-checked every few seconds: hooks that run for this repository, its reflexes
  in the index, live conditioning (`auto-condition`), the embedding model, recent fail-open
  errors; each problem names the command that fixes it.

Keys: `q` quits, `p` pauses, `f` switches between this repository and all of them, `/` filters by
trace, reflex or command, `Enter` inspects, `m` shows how to open the map, `v` hides events where
Adage said nothing, `↑`/`↓` scroll (`G` back to the newest), `?` lists every key. `NO_COLOR` is
honoured: strengths are then spelled out (`FIRE`, `PRIME`, `INHIBIT`), and `--plain` drops glyphs
too.

`adage ui` is the full-screen app (plain `adage` prints the status card above). `:` or `ctrl-k`
opens a command palette over every action and reflex; `?` lists the keys. Its tabs:

| Tab | What it does |
| --- | --- |
| **Live** | the watch view, larger |
| **Reflexes** | every instinct and reflex by track, with a fuzzy `/` filter: strength, stage, trigger and step, evidence (traces, Wilson lower bound), live counts, and the exact directive text. Review decisions (`a`pprove, `e`xtinguish, `w`eaken, `s`trengthen, pi`n`, dismiss `x`) ask first, then run exactly what `adage review` runs, with the same ceilings, and show the equivalent command |
| **Map** | stimulus → reflex → response as a layered graph in the terminal; `adage map` opens the same in the browser |
| **Health** | every doctor check, live, including live conditioning's. `Enter` fixes the selected one where a key can: register hooks (confirming the exact file and diff), fetch the model (with progress), condition this repository now (what a background learner runs: `adage condition --auto --repo-root`), turn live conditioning on, open the settings, or open the fail-open log |
| **Settings** | every `adage config` key with its value and where it came from (default, file, or environment); `Enter` edits, `Space` toggles, and `adage config set`'s validation and agent-session refusal apply |

`adage watch --demo` and `adage ui --demo` replay a small scripted scenario (three sessions of the
harbor-dispatch demo, through real hook calls) into a temporary data directory, so you can see it
all without an agent running. `adage watch --once [--size 80x24]` prints a single frame as text.
Decisions, installs and settings refuse inside an agent session, as their commands do.

## Claude Code

Once the marketplace is published (not yet), the plugin installed from it will need nothing else.
Until then, try the plugin from a source checkout (it then uses the binary `plugin/build.sh`
stages, or `adage` on `PATH`):

```bash
# 1. Build the binaries and stage them into the plugins
plugin/build.sh

# 2. Condition on your existing Claude Code traces (reads ~/.claude/projects, sub-agents'
#    traces included). After this, Adage keeps conditioning by itself as sessions end
#    (see "Live conditioning" below).
target/release/adage condition
target/release/adage status

# 3. Run Claude Code with the Adage plugin
claude --plugin-dir plugin/claude-code
```

## OpenCode and Codex

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

**Kill switches:** `ADAGE_DISABLE=1`, or a `.adage/disabled` file in a repository.

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
| `learn.parse_cache` | `true` | Keep parsed transcripts in `cache/sessions` (owner-only), so re-learning parses only what changed |
| `learn.cards` | `true` | Procedure cards for recurring tasks (embedded with the installed or bundled model) ([docs/training.md](training.md#recurring-tasks-procedure-cards)) |
| `learn.card_threshold` | `0.811` | A prompt's best similarity to a family's prompts that delivers its card (strict: it recognises phrasing close to earlier prompts, not paraphrases) |
| `instincts.enabled` | `true` | Bind Adage's instincts from each repository's files |
| `ui.notices` | `daily` | One-line notices at session start (Claude Code), at most daily and only when new; `off` silences them |
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
forming, never fire, under `tracks/@imported/` where `adage condition` never writes; if you later condition
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

**Experiment:** `runtime.control = 0.1` (default; `ADAGE_CONTROL` overrides) makes 10% of sessions the control — Adage computes what it would
have said and logs it, but says nothing — so every claimed effect is measured against a control.

## The demo

`demo/` runs real headless Claude Code agents on a sandbox repository whose conventions an agent
cannot guess (work items only via a tracker CLI; tests only via `make test`; `make gen` required
after schema edits), with twelve work items and hidden checks.

```bash
python3 demo/run.py all                  # train (no Adage) → condition → held-out eval, two arms
python3 demo/dashboard/serve.py          # live dashboard at http://127.0.0.1:8765
```

For your own repositories, `adage map` opens the live map of instincts and reflexes in the
browser; it is served by the binary, no Python needed.

See [demo/SANDBOX.md](../demo/SANDBOX.md). `python3 demo/sandbox/selfcheck.py` proves every work item
fails untouched and passes with its reference solution.
