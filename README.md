# Adage

**Agentic DAG Engine** — put your agent on rails.

Adage learns the routes coding agents take to get work done, packages the good ones as shareable
**tracks**, and at each decision point tells the agent — deterministically — what to do next. It
never executes anything itself: the agent runs every command through its own tools, under its
harness's own permissions.

```text
  your agents' sessions ──► adage learn ──► tracks ──► index ──► hooks ──► directives
        (transcripts)          (mine)       (files)   (compiled)  (Claude Code,  (text only)
                                                                   OpenCode, Codex)
```

## What it does, concretely

From real sessions in a repository, Adage learns five kinds of route:

| Route | Learned from | At runtime |
| --- | --- | --- |
| **Prompt → lookup** | Agents passing an ID from the prompt to the same command | 🟢/🟡 "Work item details are looked up with: `python3 scripts/wi-lookup W-2042`" |
| **Linked files** | Agents reading, one turn at a time, the files a lookup listed | 🟡 "That result lists these files … read them together in one turn" |
| **Dead ends** | A verify command that fails, followed by one that works | 🔴 "Running `python3 -m unittest` fails (…). Use this instead: `make test`" |
| **Finish line** | A regenerate step that follows edits to a path pattern | 🟢 after the edit, and once at Stop if still unmet: "changes to `schema/*.json` must be followed by `make gen`" |
| **Intent** | Prompts that mean the same thing (an *intent family*) whose sessions ran the same command first | 🟢/🟡 "Requests like this one usually start by running: `make test`" (optional; needs `adage models fetch`) |

Every route carries its evidence (distinct sessions, Wilson lower bound), an effect class that caps
how strongly it may be directed, and typed slot contracts: a value from the prompt reaches a directive
only if it matches the contract learned from real values (`^W-\d{4}$`), and file paths only if they
are safe, repository-relative, and exist.

## Results

Real **Claude Haiku 4.5** agents, headless, each in a fresh copy of a sandbox repository whose
conventions they cannot guess. Adage learned from 18 training sessions, then two learning-loop rounds
of held-out work items ran 18 sessions with Adage and 18 without (hooks active, directives withheld).
Hidden tests scored every run. Round 2 learned from every earlier session.

| Round 2, held-out | With Adage | Without | p (permutation) |
| --- | ---: | ---: | ---: |
| Turns before first edit (median) | **2** | 20 | 0.0002 |
| Seconds to first edit (median) | **12.0** | 42.5 | 0.0001 |
| Total turns (median) | **9** | 27.5 | < 0.001 |
| Cost per run (mean, USD) | **0.053** | 0.135 | 0.0001 |
| Hidden checks passed | **100%** | 78% | 0.11 (n.s. at n = 18) |

Round 1 showed the loop at work: removing exploration exposed a dead end (agents that no longer read
the Makefile tried `pytest` first: 1.44 failed runs per session). Round 2 learned it as a red route
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


## Install

From a tagged release (none published yet), for macOS (arm64, x64), Linux (x64, arm64; static) and
Windows (x64):

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/stevenvaughanbooth/adage/releases/latest/download/adage-cli-installer.sh | sh
powershell -ExecutionPolicy Bypass -c "irm https://github.com/stevenvaughanbooth/adage/releases/latest/download/adage-cli-installer.ps1 | iex"
brew install stevenvaughanbooth/tap/adage
npm install -g <package>    # name not decided yet (npm/config.mjs)
cargo install --locked --git https://github.com/stevenvaughanbooth/adage adage-cli
```

Every release archive has a SHA-256 checksum, a Sigstore build-provenance attestation
(`gh attestation verify <file> --repo stevenvaughanbooth/adage`) and a CycloneDX SBOM. For Claude
Code, add the plugin marketplace with `/plugin marketplace add stevenvaughanbooth/adage` and install
`adage`; the plugin runs the `adage` it finds on `PATH` and never downloads anything.

## Quick start

### First run

```bash
cargo build --release -p adage-cli
target/release/adage init             # guided setup at a terminal: hooks, model, first learn, auto-learn
target/release/adage watch            # run in a split pane: Adage's health, hook activity, and learning, live
target/release/adage survey           # what your agents actually do, per repository (read-only)
target/release/adage doctor           # check binary, data dir, index, tracks, model, hooks, kill switches
target/release/adage graph --serve    # the signal box at http://127.0.0.1:8765 (read-only, loopback only)
```

At a terminal, `init` is a wizard: it detects your harnesses and, one step at a time, asks before
it registers hooks (showing the exact file and diff), fetches the embedding model, runs a first
`adage learn`, and turns auto-learning on; it ends with the doctor's summary. Piped, inside an agent
session, or with `--print`, it prints the same report as before and writes nothing unless you pass
`--yes` (which only downloads the embedding model). `survey` reads the same history `learn` does and
lists the routes `learn` would build as candidates, with prompts shown only as short redacted
snippets. `doctor` exits non-zero only when a check fails.

### Watch it live

`adage watch` (or `adage doctor --watch`) is a compact live pane for a split terminal. It works in
60×20 and grows with the pane:

```text
 adage watch · harbor-dispatch · gen 8216e29f                         LIVE
+ adage 0.1.0  + hooks cc  + 4 routes  + auto-learn on  - no model
+ fail-open 0
 activity · harbor-dispatch ────────────────────────────────────────────
12:04:00 ●d76f0f cc prompt  → lookup-incident [G]
12:04:01 ◌19b2cf cc prompt  withheld lookup-incident [G]
12:04:02 ●d76f0f cc tools   → read-linked-files [Y]  ✓ followed lookup-i
12:04:05 ●bc0c7d cc tools   ✗ derailed lookup-incident
12:04:06 ●bc0c7d cc failure → dead-end-npm-test [R]
12:04:07 ●bc0c7d cc stop    ■ stop-request
12:04:08 ●bc0c7d cc tools   ✓ met codegen-after-registry
 learning ──────────────────────────────────────────────────────────────
12:04 auto-learn: 27/28 sessions used, 730ms, 2 changes, gen 8216e29f
12:04   learned incidents: read-linked-files yellow→green (13/17 sessions)
 q quit · p pause · f all repos · v hide quiet · ? help
```

- **Top:** health lamps, re-checked every few seconds: the binary, hooks that run for this
  repository, its routes in the index, auto-learn, the embedding model, recent fail-open errors.
- **Middle:** every hook call as it happens, from the session spools: time, session, harness, event,
  and what Adage did: a directive (→, coloured by aspect), withheld in a holdout session (◌),
  followed ✓ or derailed ✗, a finish line met, or a stop request.
- **Bottom:** what learning changed, from `learn.log`.

Keys: `q` quits, `p` pauses, `f` switches between this repository and all of them, `v` hides events
where Adage said nothing, `↑`/`↓` scroll, `?` shows help. `NO_COLOR` is honoured: aspects are then
spelled out (`[G]`, `[Y]`, `[R]`), as above.

`adage ui` (or plain `adage` at a terminal) is the full-screen app, with four tabs:

| Tab | What it does |
| --- | --- |
| **Live** | the watch view, larger |
| **Routes** | every installed route by track: aspect, trigger and step, evidence (sessions, Wilson lower bound), live counts, and the exact directive text. Review decisions (`a`pprove, `r`eject, `d`emote, `p`romote, pi`n`, dismiss `x`) ask first, then run exactly what `adage review` runs, with the same ceilings |
| **Health** | every doctor check, live. `Enter` fixes the selected one where a key can: register hooks (confirming the exact file and diff), fetch the model (with progress), learn now, clear a stale learning lock, or open the fail-open log |
| **Settings** | `learn.auto`, `learn.min_interval_secs`, `learn.thresholds` and `runtime.holdout` in `$ADAGE_HOME/config.toml`, validated, with the file's comments kept |

`adage watch --demo` and `adage ui --demo` replay a small scripted scenario (three sessions of the
harbor-dispatch demo, through real hook calls) into a temporary data directory, so you can see it
all without an agent running. `adage watch --once [--size 80x24]` prints a single frame as text.
Decisions, installs and settings refuse inside an agent session, as their commands do.

### Claude Code

```bash
# 1. Build the binary and stage it into the plugin
plugin/build.sh

# 2. Learn from your existing Claude Code history (reads ~/.claude/projects)
target/release/adage learn
target/release/adage status

# 3. Run Claude Code with the Adage plugin
claude --plugin-dir plugin/claude-code
```

### OpenCode and Codex

```bash
cargo build --release -p adage-cli
target/release/adage install opencode            # print the plugin; add --write to install it
target/release/adage install codex               # print a hooks.json; add --write to merge it
opencode export <session-id> > s.json && target/release/adage learn --transcripts s.json
target/release/adage learn --codex-sessions ~/.codex/sessions   # Codex rollouts
```

`--write` installs into the repository (`.opencode/plugins/adage.js`, `.codex/hooks.json`) or, with
`--scope user`, the harness's documented user location; it never runs inside an agent session.
`adage install claude-code` does the same for Claude Code settings. What each harness supports, and
what it cannot do yet (Codex reports no tool failures to hooks, so only routes that react to a
failed command stay silent there; learned dead ends are announced at the first prompt), is in
[docs/harnesses.md](docs/harnesses.md).

`adage status` shows every track and route with its aspect and evidence; `adage report` summarises
directives emitted, followed, and derailed. Data lives in `~/Library/Application Support/Adage`
(macOS), `~/.local/share/adage` (Linux) or `%APPDATA%\Adage` (Windows); `ADAGE_HOME` overrides it.

**Kill switches:** `ADAGE_DISABLE=1`, or a `.adage/disabled` file in a repository.

**Intent families (optional):** `adage models fetch` downloads the pinned static embedding model
(potion-base-8M, 31 MB, SHA-256 verified; `adage models install <dir>` for offline machines) into
the data directory. `adage learn` then clusters each repository's prompts into intent families
(exact kNN + deterministic Louvain, split and merged by what the sessions did next — spike S7) and
learns an intent route per usable family. Without the model everything else works unchanged: learning
skips intent families and hooks never load a model.

**Sharing:** `adage share <track> --out <dir> [--key <path>]` checks every file for absolute paths,
home-directory paths and secret-looking strings, re-validates the track, and signs its digest
(`sshsig`, namespace `adage-track`; ssh-agent first, else the key file, whose passphrase is asked for at
the terminal). `adage add <dir>` verifies the
signature against `$ADAGE_HOME/trusted-signers` or the repository's `.adage/trusted-signers` (approved
once with `--trust` and pinned by hash in `$ADAGE_HOME/trust.json`) and installs the routes at
probation, never green, under `tracks/@imported/` where `adage learn` never writes; if you later learn
the same route yourself, your first-hand route wins. Adding a newer version keeps your own review
decisions for every route that survives. An unknown signer needs `--trust`. Both commands refuse to run inside an agent
session (`CLAUDECODE`) or without a terminal; `ADAGE_ALLOW_NONINTERACTIVE=1` lifts only the terminal check.

**Review:** `adage review [--json]` lists the routes that need a person's decision: new routes from
`adage learn`, routes whose aspect or evidence changed materially since the last decision (shape,
aspect, a Wilson lower bound moving by 0.10, or support doubling or halving), imports at probation,
rejected routes that came back stronger, and routes agents keep leaving in treated sessions. Each
entry shows its evidence, effect and ceiling, and the exact directive text with example slot values.
A pinned route is never overwritten: when learning (or a re-import) finds a materially different
version, it waits as *proposed*, with a diff; `approve` applies it (still pinned) and `dismiss` keeps
yours. `adage review <approve|reject|demote|promote|pin|dismiss> <track> <route> [--by <name>]` records the decision
in the track's `review.yaml`, re-validates the track, and recompiles the index. Promotion never exceeds
the effect ceiling, and an import cannot be promoted to green without first-hand evidence learned
locally. `adage learn` keeps rejected routes retired until they strengthen materially, and holds
demotions and promotions. A rejected route also keeps a teammate's import of the same route out of
the index. Decisions refuse to run inside an agent session. Listing is allowed there.

**Experiment:** `ADAGE_HOLDOUT=0.1` (default) holds out 10% of sessions — Adage computes what it would
have said and logs it, but says nothing — so every claimed effect is measured against a control.

## The demo

`demo/` runs real headless Claude Code agents on a sandbox repository whose conventions an agent
cannot guess (work items only via a tracker CLI; tests only via `make test`; `make gen` required
after schema edits), with twelve work items and hidden checks.

```bash
python3 demo/run.py all                  # train (no Adage) → learn → held-out eval, two arms
python3 demo/dashboard/serve.py          # live dashboard at http://127.0.0.1:8765
```

The signal box page lives with the binary (`crates/adage-cli/assets/brain.html`); `adage graph
--serve` serves it without the Python server.

See [demo/SANDBOX.md](demo/SANDBOX.md). `python3 demo/sandbox/selfcheck.py` proves every work item
fails untouched and passes with its reference solution.

## Repository

| Path | Contents |
| --- | --- |
| `crates/adage-core` | Canonical actions, shell normaliser (tree-sitter-bash + command table as data), effect classes, spans, repo identity |
| `crates/adage-store` | Per-session two-slot state files and CRC-framed spools — no daemon |
| `crates/adage-tracks` | Track format, validation, canonical digest, compiled index |
| `crates/adage-runtime` | Hook-time decisions and directive rendering from fixed templates |
| `crates/adage-learn` | Transcript import and the route miners |
| `crates/adage-sign` | Native `sshsig` signing (ssh-agent or key file) and `allowed_signers` verification |
| `crates/adage-sign` | Native `sshsig` signing and `allowed_signers` verification for shared tracks |
| `crates/adage-intent` | Prompt normalisation, the static embedding model in plain Rust (WordPiece + safetensors rows read on demand), deterministic kNN + Louvain clustering, behavioural split/merge |
| `crates/adage-eval` | `adage eval`: offline replay of recorded sessions through the runtime, scored against what agents did; safety oracle; property tests |
| `crates/adage-cli` | The `adage` binary; harness adapters in `src/harness/` |
| `crates/adage-sim` | Harness simulator: seeded mock histories (`sim/fixtures/`) and native hook payloads for Claude Code, OpenCode, and Codex |
| `plugin/claude-code` | The Claude Code plugin |
| `plugin/opencode` | The OpenCode plugin (installed by `adage install opencode`) |
| `demo/` | Sandbox, runner, live dashboard |
| `evals/` | Eval datasets (90 real demo sessions, synthetic, adversarial) and the live proving-ground runner — see [docs/evals.md](docs/evals.md) |
| `docs/` | [Design spec](docs/Adage%20—%20Design%20Spec.md) · [Architecture](docs/Adage%20—%20Architecture.md) · [Implementation plan](docs/Adage%20—%20Implementation%20Plan.md) |
| `spikes/` | The experiments that settled the architecture — see [the board](spikes/README.md) |

`cargo test --workspace` runs the suite, including the normaliser's 315 golden fixtures.

## Status

v0.1: Claude Code, plus OpenCode and Codex adapters and importers, built against their
documentation and source and verified end to end by the harness simulator (learn from each
harness's mock history, replay a held-out session through its native hooks in both arms), but not
yet run against a live installation — see [docs/harnesses.md](docs/harnesses.md#verification). Tracks are shared
directly (`adage share` / `adage add`) and reviewed with `adage review`. Intent families use static
embeddings (`adage models fetch`; everything else works without the model). Hardened against an
adversarial review (tracks are untrusted input; effects re-derived; bounded hot path; symlink-safe
paths). Team depots and lockfiles are designed but not yet built. See the implementation plan.

## License

Adage is licensed under the [Apache License 2.0](LICENSE). Third-party licenses for everything
compiled into the binary are in [NOTICE](NOTICE). Contributions are welcome under the same
license with a DCO sign-off (`git commit -s`); see [CONTRIBUTING.md](CONTRIBUTING.md).
