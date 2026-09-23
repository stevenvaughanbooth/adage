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

From real sessions in a repository, Adage learns four kinds of route:

| Route | Learned from | At runtime |
| --- | --- | --- |
| **Prompt → lookup** | Agents passing an ID from the prompt to the same command | 🟢/🟡 "Work item details are looked up with: `python3 scripts/wi-lookup W-2042`" |
| **Linked files** | Agents reading, one turn at a time, the files a lookup listed | 🟡 "That result lists these files … read them together in one turn" |
| **Dead ends** | A verify command that fails, followed by one that works | 🔴 "Running `python3 -m unittest` fails (…). Use this instead: `make test`" |
| **Finish line** | A regenerate step that follows edits to a path pattern | 🟢 after the edit, and once at Stop if still unmet: "changes to `schema/*.json` must be followed by `make gen`" |

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

## Quick start

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
```

`--write` installs into the repository (`.opencode/plugins/adage.js`, `.codex/hooks.json`) or, with
`--scope user`, the harness's documented user location; it never runs inside an agent session.
`adage install claude-code` does the same for Claude Code settings. What each harness supports, and
what it cannot do yet (Codex reports no tool failures, so dead-end routes stay silent there), is in
[docs/harnesses.md](docs/harnesses.md).

`adage status` shows every track and route with its aspect and evidence; `adage report` summarises
directives emitted, followed, and derailed. Data lives in `~/Library/Application Support/Adage`
(macOS) or `~/.local/share/adage` (Linux); `ADAGE_HOME` overrides it.

**Kill switches:** `ADAGE_DISABLE=1`, or a `.adage/disabled` file in a repository.

**Sharing:** `adage share <track> --out <dir> [--key <path>]` checks every file for absolute paths,
home-directory paths and secret-looking strings, re-validates the track, and signs its digest
(`sshsig`, namespace `adage-track`; ssh-agent first, else the key file). `adage add <dir>` verifies the
signature against `$ADAGE_HOME/trusted-signers` or the repository's `.adage/trusted-signers` (approved
once with `--trust` and pinned by hash in `$ADAGE_HOME/trust.json`) and installs the routes at
probation, never green, under `tracks/@imported/` where `adage learn` never writes; if you later learn
the same route yourself, your first-hand route wins. An unknown signer needs `--trust`. Both commands refuse to run inside an agent
session (`CLAUDECODE`) or without a terminal; `ADAGE_ALLOW_NONINTERACTIVE=1` lifts only the terminal check.

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
| `crates/adage-cli` | The `adage` binary; harness adapters in `src/harness/` |
| `plugin/claude-code` | The Claude Code plugin |
| `plugin/opencode` | The OpenCode plugin (installed by `adage install opencode`) |
| `demo/` | Sandbox, runner, live dashboard |
| `docs/` | [Design spec](docs/Adage%20—%20Design%20Spec.md) · [Architecture](docs/Adage%20—%20Architecture.md) · [Implementation plan](docs/Adage%20—%20Implementation%20Plan.md) |
| `spikes/` | The experiments that settled the architecture — see [the board](spikes/README.md) |

`cargo test --workspace` runs the suite, including the normaliser's 315 golden fixtures.

## Status

v0.1: Claude Code, plus OpenCode and Codex adapters (built against their documentation, not yet
run against a live installation — see [docs/harnesses.md](docs/harnesses.md)); tracks are shared directly (`adage share` / `adage add`). Hardened against an adversarial review (tracks are
untrusted input; effects re-derived; bounded hot path; symlink-safe paths). Team depots, lockfiles,
intent embeddings, and the review queue are designed and spiked but not yet built. See the implementation plan.
