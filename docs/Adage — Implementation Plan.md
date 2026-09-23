# Adage — Implementation Plan

**Status:** Phase 3, draft 1 · **Date:** 2026-09-22 · **Builds:** *Adage — Architecture* draft 2 + spike results

## 1. What the spikes settled

| Decision | Settled by | Outcome |
| --- | --- | --- |
| A1 Daemon | S3, S1 | **No daemon.** Hooks are short-lived processes over per-session state files and spools |
| A2 Language | S1 | **Rust.** Exec-form spawn + work ≈ 2 ms p95; a Node client is 35 ms p50 and ruled out |
| A3 Transport | S1, S3 | **Command hooks in exec form**, no socket needed without a daemon |
| A5 Storage | S3 | Hooks write per-session **spools + two-slot state files**; `adage learn` is the only SQLite/index writer |
| A6 Embeddings | S2 | **Static embeddings first** (potion, in-process); transformer only if the credibility test shows the 8-point family gap matters |
| A7 Signing | S6 | **Native Rust sshsig**, signing through ssh-agent; pin `ssh-key = 0.6.7` |
| A8 Packaging | S5 | Binary in `${CLAUDE_PLUGIN_DATA}`, hooks call it by absolute path, pre-run at `SessionStart` (first-exec cost 119–350 ms, S1) |
| Normaliser | S8 | tree-sitter-bash + command table as data; 98.2% coverage, 315 fixtures |
| Batch decisions | S3 + S5 | Decide on the **synchronous `PostToolBatch` payload**; async observations are history only |

## 2. Repository layout

```text
Cargo.toml                 workspace
crates/
  adage-core/              canonical actions, effects, normaliser (from S8), templates, spans
  adage-store/             spool + state files (from S3), data directories
  adage-tracks/            track format, validation, canonical digest, signing (from S6)
  adage-learn/             transcript import, segmentation, miners, evidence, assembly, index
  adage-runtime/           index, decisions, directive rendering, experiment assignment
  adage-cli/               the `adage` binary
plugin/claude-code/        plugin manifest, hooks, install shim
demo/                      sandbox repo generator, tasks, runner, live dashboard
```

## 3. Slices

Each slice is vertical, testable, and leaves `main` working.

| # | Slice | Delivers | Done when |
| --- | --- | --- | --- |
| 1 | **Walking skeleton** | `adage hook claude <event>` reads a payload, appends to the spool, renders a directive from a hand-written index | A real Claude Code session receives a directive through the hook |
| 2 | **Normaliser crate** | S8 promoted to `adage-core`, with its fixture suite as a test | 315 fixtures pass in `cargo test` |
| 3 | **Learn: import + segment** | Claude Code transcripts → sessions of canonical, templated actions with prompts, spans, outcomes | Golden test on a recorded session |
| 4 | **Learn: miners** | Prompt routes with slot provenance, dead ends (red), finish-line obligations, batchable reads | Mines the expected routes from the demo's training transcripts |
| 5 | **Tracks + index** | Track directories (TOML + YAML + evidence), deterministic index compile, signing and verification | `adage learn` twice produces identical bytes; a tampered track is rejected |
| 6 | **Runtime decisions** | Decision points, effect ceilings, slot validation, interlocking, per-session caps, stop-hook bounds, follow/derail judgement | Unit tests for every rule; replay test over recorded sessions |
| 7 | **Claude Code plugin** | Plugin with exec-form hooks, data-dir install, `SessionStart` pre-run | Installs with `--plugin-dir`; fails open with the binary missing |
| 8 | **Demo** | Sandbox repo, training + held-out tasks, runner, live dashboard, published results | Real agents, both arms, results charted live |
| 9 | Intent families | Static embeddings, normalised prompts, per-family thresholds | S7 results applied |
| 10 | Sharing, review queue, OpenCode adapter | Per design | — |

Slices 1–8 are the demo path. 9–10 follow.

## 4. The demo

A **sandbox repository** with conventions an agent cannot guess and must otherwise discover:

- Work items are only available through `scripts/wi-lookup <ID>`.
- Running `pytest` directly fails; tests run through `make test` (a dead end → red signal).
- Editing `schema/*.json` requires `make gen`, and `make ci` fails if generated code is stale
  (a finish-line obligation).

**Protocol:**

1. **Learn.** Real agents (headless Claude Code) work training tasks with no Adage. `adage learn`
   mines their transcripts into tracks — nothing hand-written.
2. **Evaluate.** Held-out tasks, several repeats, two arms: **baseline** (no Adage) and **Adage**
   (tracks installed, hooks active). Each run starts from a fresh copy of the sandbox.
3. **Score.** Hidden check (`make ci`) after each run; turns and time to first edit, total turns,
   wall-clock, cost, success, obligation met, directives followed or derailed.
4. **Watch.** A live dashboard charts both arms as runs complete, with a per-agent timeline showing
   where directives landed.

Evidence thresholds are lowered to demo scale and the dashboard says so.

## 5. Test strategy

- Normaliser: golden fixtures (S8).
- Spool and state: property and crash tests (S3).
- Learn: golden transcripts → expected routes; determinism (same input → same bytes).
- Runtime: table-driven tests per decision rule; replay of recorded sessions; fail-open tests
  (missing index, corrupt state, garbage stdin).
- Signing: interoperability and tamper tests (S6).
- CI: `cargo test`, `cargo clippy -D warnings`, fixture gate.
