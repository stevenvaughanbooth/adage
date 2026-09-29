# Adage

**Agent traces become reflexes.**

Adage reads the traces your coding agents leave behind (Claude Code, OpenCode and Codex session
transcripts), finds the paths that actually get work done, and compiles them into **reflexes**:
small, typed, deterministic responses with their evidence attached. At each decision point, the
matching reflex tells the agent what to do next, so the agent spends turns on the task instead of
rediscovering the repository.

Reflexes come from three places:

- **Traces.** `adage condition` mines real traces in a repository.
- **Instincts.** Reflexes that ship with Adage and work on day 0, read from the repository's own
  Makefile, `package.json`, CI and lockfiles, with no history needed. A learned reflex always
  overrides the matching instinct.
- **Skills.** `adage train <SKILL.md>` compiles a skill's procedure (which command, after which
  edit, in which order, never which runner) into one reviewed pack of reflexes, deterministically
  and for free, and replays every compiled command on a clean checkout in a sandbox: a stale
  instruction is dropped and reported, and the review says which steps stay text and why. `adage
  train accept` installs it; the agent then follows the procedure without the skill in context.

Reflexes live in **tracks** on disk, and travel as signed, closed **packs** you can review, commit
and share. Adage never runs the commands its reflexes direct: the agent runs every command through
its own tools, under its harness's own permissions. The `adage` binary starts no process on the
agent's behalf (it starts only itself, to condition in the background, and `adage-train` when you
ask it to train; a test pins this). Execution is a separate, opt-in binary (`adage-exec`, turned
on with `adage delegate on`) that runs only reflexes you approved on that machine (`adage review
delegate`), inside an OS sandbox.

```text
  agent traces ──► adage condition ──► reflexes ──► index ──► hooks ──► directives
  (transcripts)     (mine)            (tracks,    (compiled) (Claude Code,  (text; typed data
  + instincts                          packs)                 OpenCode,     where you approved
  + adage train                                               Codex)        execution)
```

## Install

In Claude Code:

```text
/plugin marketplace add stevenvaughanbooth/adage
/plugin install adage@adage
```

Then start a session. That is all. The plugin carries verified binaries for macOS (arm64, x64),
Linux (x64, arm64) and Windows (x64), so nothing else is installed and nothing is downloaded. At
the first session in a repository you see one line, which the agent never sees:

```text
adage: learning from 23 past sessions in this repo; reflexes arrive within a minute. Nothing runs without you.
```

In the background, Adage binds its instincts from the repository's own files (Makefile,
`package.json`, CI) and learns from this repository's earlier Claude Code traces, and only this
repository's: other repositories' transcripts are never opened. From the next prompt on, the agent
gets its reflexes. Adage keeps learning after every session, and at most once a day, only when
something is new, tells you in one line (`adage: 3 new reflexes active.`, or `1 install step waits
for you: run adage review in a terminal.`).

From a terminal, `brew install stevenvaughanbooth/tap/adage` or `npm install -g @adage/cli`, then
connect your agent once (`adage` tells you how: `adage install claude-code --write --scope user`,
or `codex`, `opencode`) and start a session. Plain `adage`, anywhere (a terminal, a pipe, or
`!adage` inside Claude Code), prints where things stand:

```text
adage 0.1.0 · github.com/acme/billing                          ● healthy
  14 reflexes active: 4 instincts, 9 learned, 1 from your team
  learning after each session · last: 2 min ago, 1 change
  last 30 days: 61 sessions · 188 directives · 152 followed · 7 dead ends headed off
  savings: ~$0.034 and 3.1 turns per session, API-list equivalent (95% CI $0.011–$0.058)
           ≈ $1.87 over 55 sessions, vs 6 where Adage stayed quiet (priced as of 2026-06-24)
  ended asking you for help: 3% with Adage, 5% without
  1 install step waits for you → adage review
  adage ui · adage map · adage doctor
```

**What is measured, honestly.** Adage stays quiet in 1 session in 10 (the control), so its effect
can be measured. Until a repository has 10 quiet sessions and 30 with Adage, the card shows counts
of what happened, never dollars (`measuring savings: 4 of 10 quiet sessions so far`). After that
it shows the saving per session with a 95% interval, or "no measurable difference yet", or, if
that is what the sessions say, "Adage appears to add ~$x per session". Dollars are API-list
equivalents from a dated price table (subscription users do not pay per token); a model not in
the table is shown as tokens and turns only. `adage savings --json` has the whole estimate.

**What never happens without you:** nothing is ever run on the agent's behalf unless you turn on
delegation at a terminal; install steps (environment changes) wait for `adage review`; no hook or
learner touches the network; settings files change only when you install the plugin or run
`adage install … --write` (which shows the diff).

## What it does, concretely

From real traces in a repository, one learner, a graph engine
([Track G](docs/tracks/track-g-graph-engine.md)), finds the steps agents take together and
compiles them into reflexes. It has no list of kinds to look for: a reflex is whatever stimulus
reliably precedes a step, and how it is delivered follows from its shape. Reflexes it discovers
on real and generated repositories include:

| Reflex | Learned from | At runtime |
| --- | --- | --- |
| **Look the item up** | Agents passing a value from the prompt to the same command; the value's type (`W-2042`, `#482`, a UUID) is learned from the values themselves | 🟢/🟡 "Work item details are looked up with: `python3 scripts/wi-lookup W-2042`" |
| **Read what it lists** | Agents reading, one turn at a time, the files a lookup listed | 🟡 "That result lists these files … read them together in one turn" |
| **Avoid a dead end** | A command that fails with the same signature, followed by one that works | 🔴 "Running `python3 -m unittest` fails (…). Use this instead: `make test`" |
| **Regenerate after an edit** | A regenerate step that follows edits to a path pattern | 🟢 after the edit, and once at Stop if still unmet: "changes to `schema/*.json` must be followed by `make gen`" |
| **Steps in order** | Commands run one after another after edits to a path pattern | 🟢/🟡 "changes to `registry/*.csv` are followed by these steps, in order: `make gen`, `make check`" |
| **Check the part you edited** | A command naming the directory an edit touched | 🟢/🟡 after an edit under `packages/ledger/`: `make -C packages/ledger check` |
| **Run the repository's checks** | The checks agents run after editing a kind of file, in the repo-wide form some of them ran (their `cargo test -p <crate>` and `cargo test --workspace --locked -j4` runs are one `cargo test --workspace --locked`) | 🟡 after an edit to `crates/**/*.rs`: `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo fmt --all` owed as a finish line |
| **Sync the environment** | A dependency step after a dependency file changes | 🟡 after review, once `requirements.txt` changes: `make deps then make check` |
| **Start like others did** | Prompts that mean the same thing whose sessions ran the same command first | 🟢/🟡 "Requests like this one usually start by running: `make test`" (optional; needs `adage models fetch`) |

Every reflex carries its evidence (distinct traces, Wilson lower bound), an effect class that caps
how strongly it may be directed, and typed slot contracts: a value from the prompt reaches a directive
only if it matches the contract learned from real values (`^W-\d{4}$`), and file paths only if they
are safe, repository-relative, and exist.

## Results

Real **Claude Haiku 4.5** agents, headless, each in a fresh copy of a sandbox repository whose
conventions they cannot guess. Adage learned from 18 training sessions, then two learning-loop rounds
of held-out work items ran 18 sessions with Adage and 18 without (hooks active, directives withheld).
Hidden tests scored every run. Round 2 learned from every earlier session.

> **Caveat (2026-09-27 audit).** These runs predate stricter protocol: round 2 re-used the same six
> work items as round 1, arm order was fixed, both arms shared the operator's Claude Code
> auto-memory, and p-values treat 18 runs (6 tasks × 3 repeats) as independent. The effects are
> large enough to survive most of this, but treat the numbers as indicative until the runs are
> repeated under the corrected runner. See [docs/evals.md](docs/evals.md#known-limitations-of-the-published-numbers).

| Round 2, held-out | With Adage | Without | p (permutation) |
| --- | ---: | ---: | ---: |
| Turns before first edit (median) | **2** | 20 | 0.0002 |
| Seconds to first edit (median) | **12.0** | 42.5 | 0.0001 |
| Total turns (median) | **9** | 27.5 | < 0.001 |
| Cost per run (mean, USD) | **0.053** | 0.135 | 0.0001 |
| Hidden checks passed | **100%** | 78% | 0.11 (n.s. at n = 18) |

Round 1 showed the loop at work: removing exploration exposed a dead end (agents that no longer read
the Makefile tried `pytest` first: 1.44 failed runs per session). Round 2 conditioned it as an inhibit reflex
and it disappeared (0.00). Without Adage, 4 of 18 round-1 agents gave up asking what the ticket was.
Full data: [demo/results](demo/results/); interactive results page published as an artifact.

### A second demo: a stronger model, a different stack

Next, **Claude Sonnet 5** on **harbor-dispatch**, a TypeScript service on Node 26 whose conventions
differ from the first sandbox:

- incidents like `INC-94` resolve only through `bin/fetch-incident`;
- `npm test` fails and `npm run check` works;
- `registry/*.csv` edits need `npm run codegen`.

There were 84 sessions: 12 for training, then 36 with Adage and 36 without over two rounds. The run
cost $6.55 in total.

| Round 2, held-out | With Adage | Without | p (permutation) |
| --- | ---: | ---: | ---: |
| Turns before first edit (median) | **3** | 6 | 0.02 |
| Seconds to first edit (median) | **6.7** | 11.0 | < 0.001 |
| Exploration calls before first edit (median) | **2** | 4 | < 0.001 |
| Total turns (median) | **7** | 10 | 0.002 |
| Cost per run (mean, USD) | **0.065** | 0.086 | < 0.001 |
| Hidden checks passed | 100% | 100% | — |

Sonnet found every convention on its own:

- it ran the lookup unaided in every training session;
- it met the finish-line obligation 100% of the time in both arms;
- it never tried `npm test`, so no dead end was learned.

Adage's effect therefore moved from avoiding failures (Haiku: 78% → 100% hidden checks) to
**skipping exploration**: agents went to the lookup and the linked files directly. That halved the
turns before the first edit and cut cost by 24%, with quality unchanged. The weaker the agent is at
finding conventions, the more Adage adds.

Results are in [demo/results/20260923-1249-harbor-dispatch-claude-sonnet-5](demo/results/20260923-1249-harbor-dispatch-claude-sonnet-5/).


## Going further

### Other ways to install

From a tagged release, for macOS (arm64, x64), Linux (x64, arm64; static) and Windows (x64):

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/stevenvaughanbooth/adage/releases/latest/download/adage-cli-installer.sh | sh
powershell -ExecutionPolicy Bypass -c "irm https://github.com/stevenvaughanbooth/adage/releases/latest/download/adage-cli-installer.ps1 | iex"
brew install stevenvaughanbooth/tap/adage
npm install -g @adage/cli   # once the scope is claimed (npm/config.mjs)
cargo install --locked --git https://github.com/stevenvaughanbooth/adage adage-cli
```

Three binaries ship: `adage`; `adage-exec`, which runs the reflexes you let act (`adage delegate`;
Linux and macOS); and `adage-train`, which compiles a skill into reflexes (`adage train`). The npm
package carries all three. The other channels install each on its own: `adage-exec-installer.sh`
and `adage-train-installer.sh` beside `adage-cli-installer.sh`, `brew install
stevenvaughanbooth/tap/adage-exec` (and `adage-train`), or `cargo install --locked --git … adage-exec`
(and `adage-train`). Put them beside `adage` or on `PATH`.

Every release archive has a SHA-256 checksum, a Sigstore build-provenance attestation
(`gh attestation verify <file> --repo stevenvaughanbooth/adage`) and a CycloneDX SBOM. The release
workflow publishes the Claude Code plugins, with those binaries bundled and checksummed, to the
`marketplace` branch that `/plugin install` reads; the plugin verifies its binary against
`dist/SHA256SUMS` at every session start, falls back to `adage` on `PATH`, and otherwise says in one
line what to install.

### Commands

`adage --help` lists every command; `adage <command> --help` its options.

```bash
adage init             # guided setup at a terminal: hooks, model, first conditioning, auto-conditioning
adage sense            # read-only: which reflexes your traces would form, per repository
adage condition        # form them: read your agents' traces, bind instincts, compile the index
adage watch            # in a split pane: Adage's health, hook activity and conditioning, live
adage map              # the map of your reflexes at http://127.0.0.1:8765 (read-only, loopback only)
adage doctor           # check binary, data dir, index, tracks, model, hooks, kill switches
adage review           # reflexes waiting for your decision, with evidence and the exact directive text
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
it registers hooks (showing the exact file and diff), fetches the embedding model, conditions
the repository for the first time, and turns automatic conditioning on if it is off; it ends with the doctor's
summary. Piped, inside an agent session, or with `--print`, it prints the same report and writes
nothing; `--yes` downloads the embedding model and binds this repository's instincts. `sense` reads
the same traces `condition` does and lists the reflexes `condition` would form as candidates, with
prompts shown only as short redacted snippets. With no history yet, `adage condition
--instincts-only` binds Adage's instincts from the repository's files alone. `doctor` exits
non-zero only when a check fails. From a source checkout, `cargo build --release --locked -p
adage-cli -p adage-exec -p adage-train` builds all three into `target/release/`.

### Watch it live

`adage watch` (or `adage doctor --watch`) is a compact live pane for a split terminal. It works in
60×20 and grows with the pane:

```text
 adage watch · harbor-dispatch · gen 44876069                         LIVE
+ adage 0.1.0  + hooks cc  + 4 reflexes  + auto-learn on  - no model
+ fail-open 0
 activity · harbor-dispatch ────────────────────────────────────────────
12:04:00 ●d76f0f cc prompt  → lookup-incident [F]
12:04:01 ◌19b2cf cc prompt  withheld lookup-incident [F]
12:04:02 ●d76f0f cc tools   → read-linked-files [P]  ✓ followed lookup-i
12:04:05 ●bc0c7d cc tools   ✗ ignored lookup-incident
12:04:06 ●bc0c7d cc failure → dead-end-npm-test [I]
12:04:07 ●bc0c7d cc stop    ■ stop-request
12:04:08 ●bc0c7d cc tools   ✓ met codegen-after-registry
 conditioning ──────────────────────────────────────────────────────────
12:04 auto-learn: 27/28 sessions used, 730ms, 2 changes, gen 44876069
12:04   conditioned incidents: read-linked-files prime→fire (13 of 17 s
 q quit · p pause · f all repos · v hide quiet · ? help
```

- **Top:** health lamps, re-checked every few seconds: the binary, hooks that run for this
  repository, its reflexes in the index, auto-learn, the embedding model, recent fail-open errors.
- **Middle:** every hook call as it happens, from the session spools: time, session, harness, event,
  and what Adage did: a directive (→, coloured by strength), withheld in a control session (◌),
  followed ✓ or ignored ✗, a finish line met, or a stop request.
- **Bottom:** what conditioning changed, from `learn.log`, manual or automatic; its title says
  *conditioning now…* while a background learner holds this repository's lock.

Keys: `q` quits, `p` pauses, `f` switches between this repository and all of them, `v` hides events
where Adage said nothing, `↑`/`↓` scroll, `?` shows help. `NO_COLOR` is honoured: strengths are then
spelled out by initial (`[F]` fire, `[P]` prime, `[I]` inhibit), as above.

`adage ui` is the full-screen app, with four tabs:

| Tab | What it does |
| --- | --- |
| **Live** | the watch view, larger |
| **Reflexes** | every installed reflex by track: strength, trigger and step, evidence (traces, Wilson lower bound), live counts, and the exact directive text. Review decisions (`a`pprove, `e`xtinguish, `w`eaken, `s`trengthen, pi`n`, dismiss `x`) ask first, then run exactly what `adage review` runs, with the same ceilings |
| **Health** | every doctor check, live, including live conditioning's. `Enter` fixes the selected one where a key can: register hooks (confirming the exact file and diff), fetch the model (with progress), condition this repository now (what a background learner runs: `adage condition --auto --repo-root`), turn auto-learn on, open the settings, or open the fail-open log |
| **Settings** | every `adage config` key with its value and where it came from (default, file, or environment); `Enter` edits, `Space` toggles, and `adage config set`'s validation and agent-session refusal apply |

`adage watch --demo` and `adage ui --demo` replay a small scripted scenario (three sessions of the
harbor-dispatch demo, through real hook calls) into a temporary data directory, so you can see it
all without an agent running. `adage watch --once [--size 80x24]` prints a single frame as text.
Decisions, installs and settings refuse inside an agent session, as their commands do.

### Claude Code

Installed from the marketplace (above), the plugin needs nothing else. To try the plugin from a
source checkout instead (it then uses the binary `plugin/build.sh` stages, or `adage` on `PATH`):

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

### OpenCode and Codex

```bash
adage install opencode            # print the plugin; add --write to install it
adage install codex               # print a hooks.json; add --write to merge it
opencode export <session-id> > s.json && adage condition --transcripts s.json
adage condition --codex-sessions ~/.codex/sessions   # Codex rollouts
```

`--write` installs into the repository (`.opencode/plugins/adage.js`, `.codex/hooks.json`) or, with
`--scope user`, the harness's documented user location; it never runs inside an agent session.
`adage install claude-code` does the same for Claude Code settings. What each harness supports, and
what it cannot do yet (Codex reports no tool failures to hooks, so only reflexes that react to a
failed command stay silent there; learned dead ends are announced at the first prompt), is in
[docs/harnesses.md](docs/harnesses.md).

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
[docs/live-learning.md](docs/live-learning.md).

**Settings:** `$ADAGE_HOME/config.toml`, shown and changed with `adage config`:

```bash
adage config                                  # every key, its value, and where it came from
adage config set learn.auto false             # stop learning automatically (ADAGE_AUTO_LEARN=0 also works)
adage config set learn.thresholds demo        # lower evidence bars, for small histories
adage config set learn.min_interval_secs 300  # learn each repository at most every 5 minutes
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
| `learn.cards` | `true` | Procedure cards for recurring tasks, with the embedding model ([docs/training.md](docs/training.md#recurring-tasks-procedure-cards)) |
| `learn.card_threshold` | `0.8` | A prompt's similarity to a family that delivers its card |
| `instincts.enabled` | `true` | Bind Adage's instincts from each repository's files |
| `ui.notices` | `daily` | One-line notices at session start (Claude Code), at most daily and only when new; `off` silences them |
| `runtime.control` | `0.1` | Control share (`ADAGE_CONTROL` wins) |

Keys that change what Adage learns from or how much evidence it needs (`learn.thresholds`, the
three source directories, `instincts.packs`) are a person's decision: `config set` asks for a
terminal and refuses inside an agent session. Every other key only tunes Adage or turns something
down, and can be set from a script, CI or a sandboxed session.

**Intent families (optional):** `adage models fetch` downloads the pinned static embedding model
(potion-base-8M, 31 MB, SHA-256 verified; `adage models install <dir>` for offline machines) into
the data directory. `adage condition` then clusters each repository's prompts into intent families
(exact kNN + deterministic Louvain, split and merged by what the sessions did next — spike S7) and
forms an intent reflex per usable family. Without the model everything else works unchanged: learning
skips intent families and hooks never load a model. Behind a proxy that blocks huggingface.co, the
download is not retried by itself: `adage doctor` says why (`intent grouping off: huggingface.co
blocked by proxy (403)`) and how to fix it. `ADAGE_MODEL_BASE_URL=<mirror>` downloads the same
files from a mirror (still checked against the pinned SHA-256s).

**Sharing:** `adage share <track> --out <dir> [--key <path>]` converts a track to a **pack** (the
only format that travels; [docs/packs.md](docs/packs.md)), checks every byte for absolute paths,
home-directory paths and secret-looking strings, re-validates it as a recipient would, and signs its
digest (`sshsig`, namespace `adage-pack`; ssh-agent first, else the key file, whose passphrase is asked
for at the terminal). `adage add <dir>` verifies the
signature against `$ADAGE_HOME/trusted-signers` or the repository's `.adage/trusted-signers` (approved
once with `--trust` and pinned by hash in `$ADAGE_HOME/trust.json`) and installs the reflexes
forming, never fire, under `tracks/@imported/` where `adage condition` never writes; if you later condition
the same reflex yourself, your first-hand reflex wins. Adding a newer version keeps your own review
decisions for every reflex that survives. An unknown signer needs `--trust`. Both commands refuse to run inside an agent
session (`CLAUDECODE`) or without a terminal; `ADAGE_ALLOW_NONINTERACTIVE=1` lifts only the terminal check.

**Review:** `adage review [--json]` lists the reflexes that need a person's decision: new reflexes from
`adage condition`, reflexes whose strength or evidence changed materially since the last decision (shape,
strength, a Wilson lower bound moving by 0.10, or support doubling or halving), imports forming,
extinguished reflexes that came back stronger, and reflexes agents keep ignoring in treated traces. Each
entry shows its evidence, effect and ceiling, and the exact directive text with example slot values.
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

The map page lives with the binary (`crates/adage-cli/assets/brain.html`); `adage map` serves it
without the Python server.

See [demo/SANDBOX.md](demo/SANDBOX.md). `python3 demo/sandbox/selfcheck.py` proves every work item
fails untouched and passes with its reference solution.

## Repository

| Path | Contents |
| --- | --- |
| `crates/adage-core` | Canonical actions, shell normaliser (tree-sitter-bash + command table as data), effect classes, spans, repo identity |
| `crates/adage-store` | Per-session two-slot state files and CRC-framed spools — no daemon |
| `crates/adage-tracks` | Track format, validation, compiled index, interlocking, review decisions |
| `crates/adage-runtime` | Hook-time decisions and directive rendering from fixed templates |
| `crates/adage-hook` | The hook pipeline `adage` and `adage-exec` share: harness adapters, the no-match fast path, fail-open |
| `crates/adage-graph` | The graph engine: the only learner |
| `crates/adage-learn` | Transcript import (Claude Code and its sub-agents, Codex, OpenCode), steering by influence, and the learner's driver |
| `crates/adage-instincts` | Instincts: reflexes that ship with Adage, bound from a repository's own files |
| `crates/adage-intent` | Prompt normalisation, the static embedding model in plain Rust (WordPiece + safetensors rows read on demand), deterministic kNN + Louvain clustering, behavioural split/merge |
| `crates/adage-pack` | Packs (format `adage-pack/1`): validation, closure, digest, binding; the `adage-pack` inspect/verify/digest tool |
| `crates/adage-sign` | Native `sshsig` signing (ssh-agent or key file) and `allowed_signers` verification |
| `crates/adage-exec` | The `adage-exec` binary: approvals, policy, the OS sandbox, E1 prefetch and E2 actions |
| `crates/adage-train` | The `adage-train` binary: a skill → procedure graph → replayed, reviewed pack of reflexes (and optional skill-steered sessions) |
| `crates/adage-eval` | `adage eval`: offline replay of recorded sessions through the runtime, scored against what agents did; the L0 bench; safety oracle; property tests |
| `crates/adage-cli` | The `adage` binary |
| `crates/adage-sim` | Harness simulator: seeded mock histories (`sim/fixtures/`) and native hook payloads for Claude Code, OpenCode, and Codex |
| `plugin/claude-code` | The Claude Code plugin |
| `plugin/claude-code-exec` | The `adage-exec` plugin, which serves hooks once `adage delegate on` |
| `plugin/opencode` | The OpenCode plugin (installed by `adage install opencode`) |
| `npm/` | The npm launcher and per-platform packages |
| `demo/` | Sandbox, runner, live dashboard |
| `evals/` | Eval datasets (90 real demo sessions, synthetic, adversarial) and the live eval runner — see [docs/evals.md](docs/evals.md) |
| `docs/` | [Design spec](docs/Adage%20—%20Design%20Spec.md) · [Architecture](docs/Adage%20—%20Architecture.md) · [Implementation plan](docs/Adage%20—%20Implementation%20Plan.md) |
| `spikes/` | The experiments that settled the architecture — see [the board](spikes/README.md) |

`cargo test --workspace --locked` runs the suite, including the normaliser's golden fixtures, the
eval gates and the L0 bench against its baseline.

## Status

v0.1: Claude Code, plus OpenCode and Codex adapters and importers, built against their
documentation and source and verified end to end by the harness simulator (condition on each
harness's mock history, replay a held-out session through its native hooks in both arms), but not
yet run against a live installation — see [docs/harnesses.md](docs/harnesses.md#verification). Reflexes travel
as signed packs (`adage share` / `adage add`) and are reviewed with `adage review`. Intent families use static
embeddings (`adage models fetch`; everything else works without the model). Hardened against an
adversarial review (tracks are untrusted input; effects re-derived; bounded hot path; symlink-safe
paths). Team depots and lockfiles are designed but not yet built. See the implementation plan.

## License

Adage is licensed under the [Apache License 2.0](LICENSE). Third-party licenses for everything
compiled into the binary are in [NOTICE](NOTICE). Contributions are welcome under the same
license with a DCO sign-off (`git commit -s`); see [CONTRIBUTING.md](CONTRIBUTING.md).
