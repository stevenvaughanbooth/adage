# S8 · Normaliser coverage — Findings

**Verdict: pass.** tree-sitter-bash plus a data-driven command table normalises **98.2%** of real
tool calls to a known canonical action (gate: ≥ 95%), with **315 / 315** golden fixtures passing.

> The spike agent was stopped by a usage limit after building and running everything but before
> writing this file; the coordinator wrote it from `results/corpus-summary.json` and a fresh fixture
> run. Reproduce: `cargo build --release && ./target/release/normaliser fixtures && ./target/release/normaliser corpus`.

## What was built

| File | Purpose |
| --- | --- |
| `src/shell.rs` (775 lines) | tree-sitter-bash walk: `bash -lc` unwrapping, `cd` tracking, `&&`/`||`/`;` sequences, pipelines with pure-filter tails, compound constructs, redirects, heredocs, mutation signals |
| `data/commands.toml` | Per-command flag grammars and effect classes **as data** |
| `src/table.rs` | Loads the command table |
| `src/tools.rs` | Claude Code native tools → canonical actions |
| `src/template.rs` | Argument templating with typed placeholders |
| `src/fixtures.rs`, `fixtures/golden.json` | 315 synthetic golden cases, including the proof of concept's cases |
| `src/corpus.rs` | Streams the local Claude Code transcripts (read-only); raw detail goes to git-ignored `local/` |

## Results on this machine's transcripts

62 sessions (including sub-agents), 11,280 lines, **0 quarantined**, 6,283 tool calls.

| Metric | Value |
| --- | --- |
| Normalised to non-`unknown` | **98.2%** |
| Low confidence | 11.8% |
| Distinct literal commands → templates | 4,416 → 1,691 |
| Repeat rate, literal → templated | **38.7% → 77.7%** (proof of concept: 37% → 73%) |
| Throughput | ~32,000 calls/s (single thread, machine under load) |

**Effect classes:** observe 4,153 · environment 1,485 · author 433 · verify 134 · publish 74 ·
regenerate 4.

**Operations:** exec 3,280 · fetch 1,387 · read 425 · search 388 · compound 241 · list 165 · write 149
· edit 133 · unknown 115.

## What remains

- **Unhandled commands** are tail cases: `claude` (56), `probe` (53), `run`, `date`, `uptime`,
  `lsof`, `du`, `ps`, `stat`. Most are system inspection commands that belong in the table as
  *observe*; several (`claude`, `probe`) are artefacts of this project's own spike runs.
- **Harness-control tools** have no canonical category: `ToolSearch` (62), `SendMessage` (23),
  `AskUserQuestion` (11), `Monitor`, `Skill`, `Artifact`. The design already calls for a `control`
  category (Design §11); these should map there rather than to `unknown`.
- **Environment is over-represented** (1,485) because unknown commands default to environment by
  design. Growing the table will move many of these to *observe*.
- The corpus is research-heavy (1,387 fetches) and small. The work corpus will look different.

## Recommendation

Adopt this normaliser as the product's normaliser crate: the command table as data, the fixture
suite as its regression gate, and a `control` category for harness-control tools.
