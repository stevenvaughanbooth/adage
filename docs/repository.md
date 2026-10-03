# Repository layout and status

## Layout

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
| `crates/adage-term` | The terminal design system: tokens, capability detection, theme, glyphs and the output grammar every command prints through |
| `xtask` | `cargo xtask tokens [--check]`: generates the terminal palette and the web map's CSS variables from the design tokens |
| `design/` | The design tokens (`design/tokens/adage.tokens.json`, W3C DTCG), the single source for every surface ([style guide](design/style.md)) |
| `web/map` | The map web app `adage map` serves, embedded in the binary |
| `web/site` | The public website |
| `plugin/claude-code` | The Claude Code plugin: both binaries, the hooks, and the mod (`hooks/*.ts`) |
| `plugin/claude-code-exec` | The retired `adage-exec` plugin, kept one release as an alias that asks to be uninstalled |
| `plugin/opencode` | The OpenCode plugin (installed by `adage install opencode`) |
| `npm/` | The npm launcher and per-platform packages |
| `demo/` | Sandbox, runner, live dashboard |
| `evals/` | Eval datasets (90 real demo sessions, synthetic, adversarial) and the live eval runner — see [docs/evals.md](evals.md) |
| `docs/` | [Design spec](Adage%20—%20Design%20Spec.md) · [Architecture](Adage%20—%20Architecture.md) · [Implementation plan](Adage%20—%20Implementation%20Plan.md) |
| `spikes/` | The experiments that settled the architecture — see [the board](../spikes/README.md) |

`cargo test --workspace --locked` runs the suite, including the normaliser's table-driven cases, the
eval gates and the L0 bench against its baseline.

## Status

v0.1, unreleased (build from source; see the [README](../README.md#install)). Execution
(`adage-exec`) runs on macOS and Linux only; on Windows, Adage advises but executes nothing, and its
hook invocation there is untested.

Claude Code, plus OpenCode and Codex adapters and importers, built against their
documentation and source and verified end to end by the harness simulator (condition on each
harness's mock history, replay a held-out session through its native hooks in both arms), but not
yet run against a live installation — see [docs/harnesses.md](harnesses.md#verification). Reflexes travel
as signed packs (`adage share` / `adage add`) and are reviewed with `adage review`. Intent families use static
embeddings (a model bundled in the binary; everything else works without one). Hardened against an
adversarial review (tracks are untrusted input; effects re-derived; bounded hot path; symlink-safe
paths). Team depots and lockfiles are designed but not yet built. See the implementation plan.
