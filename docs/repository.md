# Repository layout and status

For setup, start with the [README](../README.md). For contributions, use
[CONTRIBUTING.md](../CONTRIBUTING.md) and the current [architecture](architecture.md). This page
maps the checkout; it is not an installation prerequisite.

## Layout

| Path | Contents |
| --- | --- |
| `crates/adage-core` | Canonical actions, shell normaliser (tree-sitter-bash + command table as data), effect classes, spans, repo identity |
| `crates/adage-store` | Per-session two-slot state files and CRC-framed spools — no daemon |
| `crates/adage-tracks` | Track format, validation, compiled index, interlocking, review decisions |
| `crates/adage-runtime` | Hook-time decisions and directive rendering from fixed templates |
| `crates/adage-hook` | The hook pipeline `adage` and `adage-exec` share: harness adapters, the no-match fast path, fail-open |
| `crates/adage-graph` | The graph engine: the only learner |
| `crates/adage-trace` | Canonical sessions, calls, events and harness capabilities |
| `crates/adage-ingest` | Decode Claude Code, Codex and OpenCode histories, including supported subagent records |
| `crates/adage-learn` | Orchestrate import, learning and writing results through the store |
| `crates/adage-instincts` | Instincts: reflexes that ship with Adage, bound from a repository's own files |
| `crates/adage-intent` | Prompt normalisation, the static embedding model in plain Rust (WordPiece + safetensors rows read on demand), deterministic kNN + Louvain clustering, behavioural split/merge |
| `crates/adage-pack` | Packs (format `adage-pack/1`): validation, closure, digest, binding; the `adage-pack` inspect/verify/digest tool |
| `crates/adage-sign` | Native `sshsig` signing (ssh-agent or key file) and `allowed_signers` verification |
| `crates/adage-exec` | The `adage-exec` binary: approvals, policy, the OS sandbox, E1 prefetch and E2 actions |
| `crates/adage-train` | The `adage-train` binary: a skill → procedure graph → replayed, reviewed pack of reflexes (and optional skill-steered sessions) |
| `crates/adage-eval` | `adage eval`: offline replay, benchmarks and evaluation gates |
| `crates/adage-safety` | Independent checks on possible actions |
| `crates/adage-app` | Shared application operations and views used by the CLI, TUI and map |
| `crates/adage-cli` | The `adage` binary |
| `crates/adage-sim` | Harness simulator: seeded mock histories (`sim/fixtures/`) and native hook payloads for Claude Code, OpenCode, and Codex |
| `crates/adage-term` | The terminal design system: tokens, capability detection, theme, glyphs and the output grammar every command prints through |
| `xtask` | `cargo xtask tokens [--check]`: generates the terminal palette and the web map's CSS variables from the design tokens; `cargo xtask bump <version>` and `release-check`: the release's versions and workflow ([releasing](release.md)) |
| `design/` | The design tokens (`design/tokens/adage.tokens.json`, W3C DTCG), the single source for every surface ([style guide](design/style.md)) |
| `web/map` | The map web app `adage map` serves, embedded in the binary |
| `web/site` | The public website |
| `plugin/claude-code` | The Claude Code plugin: its three binaries (`adage`, `adage-exec`, `adage-train`), the hooks, and the mod (`hooks/*.ts`) |
| `plugin/opencode` | The OpenCode plugin (installed by `adage install opencode`) |
| `npm/` | The `@adage-sh/cli` launcher and per-platform native packages |
| `packs/` | Built-in instincts; pinned in `.adage/marketplace.toml` |
| `scripts/` | Build, packaging and repository checks |
| `demo/` | Sandbox, runner, live dashboard |
| `evals/` | Evaluation code, fixtures and curated evidence; raw private runs stay out of Git — see [evals/README.md](../evals/README.md) |
| `docs/` | [Current guides and design history](README.md) |
| `spikes/` | The experiments that settled the architecture — see [the board](../spikes/README.md) |

`cargo test --workspace --locked` runs the suite, including the normaliser's table-driven cases, the
eval gates and the L0 bench against its baseline.

## Status

Adage is experimental. Check the [installation guide](usage.md#install) for current distribution
availability and [benchmarks](benchmarks.md) for measured results and limitations. The intended
user path is a published native package, followed by `adage init`; building this repository is
for development or trying an unpublished branch.

Claude Code has the most live evaluation coverage. Codex also has live experiments and real
history ingestion evidence, including subagent calls; OpenCode integration has simulator and
plugin coverage. These do not establish feature parity or reliability on every installed harness
version. See [harness support](harnesses.md).

Automatic execution uses `adage-exec` on macOS or Linux. Windows execution is unsupported and
Windows hook invocation remains unverified. Signed pack export and import are built; remote
marketplace discovery and team depots remain planned.
