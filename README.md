# Adage

**Agent traces become reflexes.**

Adage reads the traces your coding agents leave behind (Claude Code, OpenCode and Codex session
transcripts), finds the paths that actually get work done, and compiles them into **reflexes**:
small, typed, deterministic rules with their evidence attached. At each decision point, the
matching reflex tells the agent what to do next, so the agent spends turns on the task instead of
rediscovering the repository.

Reflexes come from three places:

- **Traces.** `adage condition` mines real sessions in a repository.
- **Instincts.** Reflexes that ship with Adage and work on day 0, read from the repository's own
  Makefile, `package.json`, CI and lockfiles, with no history needed. A learned reflex always
  overrides the matching instinct.
- **Skills.** `adage-train` runs your agent headlessly on a skill (a `SKILL.md`, `CONTRIBUTING.md`
  or runbook), in a sandboxed clone, and compiles the sessions into reflexes. It also tells you
  which claims in the skill are stale.

Reflexes live in **tracks** on disk, and travel as signed, closed **packs** you can review, commit
and share. The `adage` binary never executes anything: the agent runs every command through its
own tools, under its harness's own permissions. Execution is a separate, opt-in component
(`adage-exec`) that runs only reflexes you approved on that machine, inside an OS sandbox.

```text
  agent traces ──► adage condition ──► reflexes ──► index ──► hooks ──► directives
  (transcripts)     (mine)        (tracks,    (compiled) (Claude Code,  (text; typed data
  + instincts                      packs)                 OpenCode,     where you approved
  + adage-train                                           Codex)        execution)
```

## What it does, concretely

From real sessions in a repository, Adage learns five kinds of reflex today (a general
subgraph-mining engine that learns them without hand-written kinds is in progress:
[Track G](docs/tracks/track-g-graph-engine.md)):

| Reflex | Learned from | At runtime |
| --- | --- | --- |
| **Prompt → lookup** | Agents passing an ID from the prompt to the same command | fire/prime: "Work item details are looked up with: `python3 scripts/wi-lookup W-2042`" |
| **Linked files** | Agents reading, one turn at a time, the files a lookup listed | prime: "That result lists these files … read them together in one turn" |
| **Dead ends** | A verify command that fails, followed by one that works | inhibit: "Running `python3 -m unittest` fails (…). Use this instead: `make test`" |
| **Finish line** | A regenerate step that follows edits to a path pattern | fire: after the edit, and once at Stop if still unmet: "changes to `schema/*.json` must be followed by `make gen`" |
| **Intent** | Prompts that mean the same thing (an *intent family*) whose sessions ran the same command first | fire/prime: "Requests like this one usually start by running: `make test`" (optional; needs `adage models fetch`) |

Every reflex carries its evidence (distinct sessions, Wilson lower bound), an effect class that caps
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
the Makefile tried `pytest` first: 1.44 failed runs per session). Round 2 learned it as an inhibit reflex
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
target/release/adage init      # guided setup: hooks, embedding model, first condition, auto-condition
target/release/adage watch     # live pane for a split terminal: health, hook activity, conditioning
target/release/adage ui        # the full terminal app: reflexes, map, health, settings
target/release/adage map       # the live map of instincts and reflexes in your browser (read-only, loopback)
target/release/adage sense     # what your agents actually do, per repository (read-only)
target/release/adage doctor    # check binary, data dir, index, tracks, model, hooks, kill switches
```

At a terminal, `init` is a guided setup that stays in your scrollback. It detects your harnesses
and, one step at a time, asks before it registers hooks (showing the exact file and diff), fetches
the embedding model, conditions from your agents' past traces, and turns auto-condition on if it is
off. It ends with the doctor's findings and what to run next. Piped, inside an agent session, or
with `--print`, it prints the same steps as text and writes nothing unless you pass `--yes` (which
only downloads the embedding model). `sense` reads the same history `condition` does and lists the
reflexes it would form, with prompts shown only as short redacted snippets. `doctor` exits non-zero
only when a check fails. Every command ends with the next one to run; `--plain` drops colour and
glyphs, and `NO_COLOR` is honoured.

### Watch it live

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
 - intent model not fetched · adage models fetch
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
  in the index, auto-condition, the embedding model, recent fail-open errors; each problem names
  the command that fixes it.

Keys: `q` quits, `p` pauses, `f` switches between this repository and all of them, `/` filters by
trace, reflex or command, `Enter` inspects, `m` shows how to open the map, `v` hides events where
Adage said nothing, `↑`/`↓` scroll (`G` back to the newest), `?` lists every key. `NO_COLOR` is honoured: strengths are then
spelled out (`FIRE`, `PRIME`, `INHIBIT`), and `--plain` drops glyphs too.

`adage ui` (or plain `adage` at a terminal) is the full-screen app. `:` or `ctrl-k` opens a command
palette over every action and reflex; `?` lists the keys. Its tabs:

| Tab | What it does |
| --- | --- |
| **Live** | the watch view, larger |
| **Reflexes** | every instinct and reflex by track, with a fuzzy `/` filter: strength, stage, trigger and step, evidence (traces, Wilson lower bound), live counts, and the exact directive text. Review decisions ask first, then run exactly what `adage review` runs, with the same ceilings, and show the equivalent command |
| **Map** | stimulus → reflex → response as a layered graph in the terminal; `adage map` opens the same in the browser |
| **Health** | every doctor check, live, including auto-condition's. `Enter` fixes the selected one where a key can: register hooks (confirming the exact file and diff), fetch the model (with progress), condition this repository now (what a background conditioner runs: `adage condition --auto --repo-root`), turn auto-condition on, open the settings, or open the fail-open log |
| **Settings** | every `adage config` key with its value and where it came from (default, file, or environment); `Enter` edits, `Space` toggles, and `adage config set`'s validation and agent-session refusal apply |

`adage watch --demo` and `adage ui --demo` replay a small scripted scenario (three sessions of the
harbor-dispatch demo, through real hook calls) into a temporary data directory, so you can see it
all without an agent running. `adage watch --once [--size 80x24]` prints a single frame as text.
Decisions, installs and settings refuse inside an agent session, as their commands do.

### Claude Code

```bash
# 1. Build the binary and stage it into the plugin
plugin/build.sh

# 2. Condition on your existing Claude Code traces (reads ~/.claude/projects). After this,
#    Adage keeps conditioning by itself as sessions end (see "Auto-condition" below).
target/release/adage condition
target/release/adage status

# 3. Run Claude Code with the Adage plugin
claude --plugin-dir plugin/claude-code
```

### OpenCode and Codex

```bash
cargo build --release -p adage-cli
target/release/adage install opencode            # print the plugin; add --write to install it
target/release/adage install codex               # print a hooks.json; add --write to merge it
opencode export <session-id> > s.json && target/release/adage condition --transcripts s.json
target/release/adage condition --codex-sessions ~/.codex/sessions   # Codex rollouts
```

`--write` installs into the repository (`.opencode/plugins/adage.js`, `.codex/hooks.json`) or, with
`--scope user`, the harness's documented user location; it never runs inside an agent session.
`adage install claude-code` does the same for Claude Code settings. What each harness supports, and
what it cannot do yet (Codex reports no tool failures to hooks, so only reflexes that react to a
failed command stay silent there; learned dead ends are announced at the first prompt), is in
[docs/harnesses.md](docs/harnesses.md).

`adage status` shows every track, instinct and reflex with its strength and evidence; `adage report` summarises
directives emitted, followed, and derailed. Data lives in `~/Library/Application Support/Adage`
(macOS), `~/.local/share/adage` (Linux) or `%APPDATA%\Adage` (Windows); `ADAGE_HOME` overrides it.

**Kill switches:** `ADAGE_DISABLE=1`, or a `.adage/disabled` file in a repository.

**Auto-condition:** you do not have to run `adage condition` again. When an agent's session stops or
ends, the hook starts a background conditioner for that repository (detached; the hook never waits,
and never says anything different). It runs at most once per `learn.min_interval_secs` per
repository, one at a time, and appends each reflex that changed (formed, extinguished, prime →
fire, evidence, awaiting review) to `$ADAGE_HOME/learn.log`. `adage doctor` shows the last run
per repository. The file contract for tools that follow it live is
[docs/live-learning.md](docs/live-learning.md).

**Settings:** `$ADAGE_HOME/config.toml`, shown and changed with `adage config`:

```bash
adage config                                  # every key, its value, and where it came from
adage config set learn.auto false             # stop auto-condition (ADAGE_AUTO_LEARN=0 also works)
adage config set learn.thresholds demo        # lower evidence bars, for small histories
adage config set learn.min_interval_secs 300  # condition each repository at most every 5 minutes
```

| Key | Default | |
| --- | --- | --- |
| `learn.auto` | `true` | Condition automatically when sessions stop or end (`ADAGE_AUTO_LEARN` wins) |
| `learn.min_interval_secs` | `120` | At most one automatic conditioning per repository per interval |
| `learn.thresholds` | `default` | `default` or `demo`, for `adage condition` too (`--demo` wins) |
| `learn.claude_projects` | `""` | Claude Code history (`""` = `~/.claude/projects`) |
| `learn.codex_sessions` | `""` | Codex rollouts auto-condition reads (e.g. `~/.codex/sessions`) |
| `learn.opencode_exports` | `""` | A directory of `opencode export` files auto-condition reads |
| `learn.parse_cache` | `true` | Keep parsed transcripts in `cache/sessions` (owner-only), so re-conditioning parses only what changed |
| `runtime.holdout` | `0.1` | Control share: sessions where Adage stays silent (`ADAGE_HOLDOUT` wins) |

`config set` refuses inside an agent session.

**Intent families (optional):** `adage models fetch` downloads the pinned static embedding model
(potion-base-8M, 31 MB, SHA-256 verified; `adage models install <dir>` for offline machines) into
the data directory. `adage condition` then clusters each repository's prompts into intent families
(exact kNN + deterministic Louvain, split and merged by what the sessions did next — spike S7) and
forms an intent reflex per usable family. Without the model everything else works unchanged:
conditioning skips intent families and hooks never load a model.

**Sharing:** `adage share <track> --out <dir> [--key <path>]` checks every file for absolute paths,
home-directory paths and secret-looking strings, re-validates the track, and signs its digest
(`sshsig`, namespace `adage-track`; ssh-agent first, else the key file, whose passphrase is asked for at
the terminal). `adage add <dir>` verifies the
signature against `$ADAGE_HOME/trusted-signers` or the repository's `.adage/trusted-signers` (approved
once with `--trust` and pinned by hash in `$ADAGE_HOME/trust.json`) and installs its reflexes as
forming, never fire, under `tracks/@imported/` where `adage condition` never writes; if you later
condition the same reflex yourself, your first-hand reflex wins. Adding a newer version keeps your
own review decisions for every reflex that survives. An unknown signer needs `--trust`. Both commands refuse to run inside an agent
session (`CLAUDECODE`) or without a terminal; `ADAGE_ALLOW_NONINTERACTIVE=1` lifts only the terminal check.

**Review:** `adage review [--json]` lists the reflexes that need a person's decision: nascent reflexes
from `adage condition`, reflexes whose strength or evidence changed materially since the last decision
(shape, strength, a Wilson lower bound moving by 0.10, or support doubling or halving), imports still
forming, extinguished reflexes that came back stronger, and reflexes agents keep leaving in treated
sessions. Each entry shows its evidence, effect and ceiling, and the exact directive text with example
slot values. A pinned reflex is never overwritten: when conditioning (or a re-import) finds a
materially different version, it waits as *proposed*, with a diff; `approve` applies it (still pinned)
and `dismiss` keeps yours. `adage review <approve|strengthen|weaken|extinguish|pin|dismiss> <track>
<reflex> [--by <name>]` records the decision in the track's `review.yaml`, re-validates the track, and
recompiles the index. Strengthening never exceeds the effect ceiling, and an import cannot fire
without first-hand evidence conditioned locally. `adage condition` keeps extinguished reflexes retired
until they strengthen materially, and holds weakenings and strengthenings. An extinguished reflex also
keeps a teammate's import of the same reflex out of the index. Decisions refuse to run inside an agent
session. Listing is allowed there.

**Experiment:** `runtime.holdout = 0.1` (default; `ADAGE_HOLDOUT` overrides) puts 10% of sessions in the control arm — Adage computes what it would
have said and logs it, but says nothing — so every claimed effect is measured against a control.

## The demo

`demo/` runs real headless Claude Code agents on a sandbox repository whose conventions an agent
cannot guess (work items only via a tracker CLI; tests only via `make test`; `make gen` required
after schema edits), with twelve work items and hidden checks.

```bash
python3 demo/run.py all                  # train (no Adage) → learn → held-out eval, two arms
python3 demo/dashboard/serve.py          # live dashboard at http://127.0.0.1:8765
```

For your own repositories, `adage map` opens the live map of instincts and reflexes in the
browser; it is served by the binary, no Python needed.

See [demo/SANDBOX.md](demo/SANDBOX.md). `python3 demo/sandbox/selfcheck.py` proves every work item
fails untouched and passes with its reference solution.

## Repository

| Path | Contents |
| --- | --- |
| `crates/adage-core` | Canonical actions, shell normaliser (tree-sitter-bash + command table as data), effect classes, spans, repo identity |
| `crates/adage-store` | Per-session two-slot state files and CRC-framed spools — no daemon |
| `crates/adage-tracks` | Track format, validation, canonical digest, compiled index |
| `crates/adage-runtime` | Hook-time decisions and directive rendering from fixed templates |
| `crates/adage-learn` | Transcript import and the reflex miners |
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
