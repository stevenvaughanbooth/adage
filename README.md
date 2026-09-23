# Adage

**Agentic DAG Engine** — put your agent on rails.

Adage learns the routes coding agents take to get work done, packages the good ones as shareable
**tracks**, and at each decision point tells the agent — deterministically — what to do next. It
never executes anything itself: the agent runs every command through its own tools, under its
harness's own permissions.

```text
  your agents' sessions ──► adage learn ──► tracks ──► index ──► hooks ──► directives
        (transcripts)          (mine)       (files)   (compiled)  (Claude Code)  (text only)
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

## Quick start (Claude Code)

```bash
# 1. Build the binary and stage it into the plugin
plugin/build.sh

# 2. Learn from your existing Claude Code history (reads ~/.claude/projects)
target/release/adage learn
target/release/adage status

# 3. Run Claude Code with the Adage plugin
claude --plugin-dir plugin/claude-code
```

`adage status` shows every track and route with its aspect and evidence; `adage report` summarises
directives emitted, followed, and derailed. Data lives in `~/Library/Application Support/Adage`
(macOS) or `~/.local/share/adage` (Linux); `ADAGE_HOME` overrides it.

**Kill switches:** `ADAGE_DISABLE=1`, or a `.adage/disabled` file in a repository.
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
| `crates/adage-cli` | The `adage` binary |
| `plugin/claude-code` | The Claude Code plugin |
| `demo/` | Sandbox, runner, live dashboard |
| `docs/` | [Design spec](docs/Adage%20—%20Design%20Spec.md) · [Architecture](docs/Adage%20—%20Architecture.md) · [Implementation plan](docs/Adage%20—%20Implementation%20Plan.md) |
| `spikes/` | The experiments that settled the architecture — see [the board](spikes/README.md) |

`cargo test --workspace` runs the suite, including the normaliser's 315 golden fixtures.

## Status

v0.1: Claude Code only; tracks are local; sharing, signing enforcement, intent embeddings, and the
OpenCode and Codex adapters are designed and spiked but not yet built. See the implementation plan.
