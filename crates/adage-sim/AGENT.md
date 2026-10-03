# adage-sim

The harness simulator: one deterministic scenario (the acme-billing sandbox, `demo/SANDBOX.md`)
rendered as the session history each harness writes to disk (Claude Code transcripts, OpenCode
exports, Codex rollouts) and as the hook payloads each harness sends. It is how the rest of the
workspace tests against every harness without installing one. Formats: `../../docs/harnesses.md`.

## Where it sits

- Layer 0 in `xtask/src/layers.rs` (kernel primitives): it links no workspace crate, only `serde_json`.
- Depended on, as a dev-dependency only, by `adage-cli` (`tests/harness_sim.rs`, `sources.rs`,
  `sense.rs`, `live_learn.rs`, `fresh_install.rs`, `zero_config_plugin.rs`, `examples/learn_bench.rs`)
  and `adage-learn` (`tests/cache.rs`, `tests/codex.rs`, `src/ledger.rs`).
- `publish = false`. As a layer-0 crate it must never link `adage-trace`, `adage-ingest` or any
  other Adage crate; the generators write native formats from scratch.

## What's inside

- `lib.rs`: `fixtures_dir` (`sim/fixtures`), `render_all` (every fixture file as `(path, contents)`),
  `write_all`, `fixture_files(harness)`, the seeds `CLAUDE_SEED`, `OPENCODE_SEED`, `CODEX_SEED`, and
  `assume_no_checkout_at_cwd`.
- `main.rs`: `adage-sim [dir]` rewrites the fixtures (default `sim/fixtures`).
- `scenario`: the harness-neutral sessions. `CWD` (`/fixture/acme-billing`), `EPOCH_MS`, `Rng`,
  `ITEMS`, `Act`, `Call`, `Turn`, `Session`, `DEAD_ENDS`, `history(seed)`, `replay()` (the held-out session).
- `claude`: `transcript`, `project_dir`, `session_id`, `hook`, `turn_hooks`.
- `codex`: `rollout`, `rel_path`, `hook`, `turn_hooks` (shapes from openai/codex `rust-v0.156.1`).
- `opencode`: `export`, `file_name`, `payload`, `turn_hooks`, `plugin_script` (drives `plugin/opencode/adage.js`).

## How it connects

- Out: bytes in each harness's native history format, read back by `adage-ingest`'s decoders
  through the learner in dependents' tests; hook payloads fed to `adage hook <harness> <event>` on stdin.
- In: nothing at run time. The scenario is the same one `evals/sandboxes/acme-billing` and
  `demo/sandbox` describe, so routes learned from these fixtures match that repository's conventions.
- `assume_no_checkout_at_cwd` is the one precondition end-to-end tests state: the fixtures were
  recorded at `scenario::CWD`, and a real checkout there would change repository keys.

## Changing it

- Invariants: byte-for-byte determinism from the seeds; twelve sessions per harness
  (`tests/fixtures.rs::three_harnesses_twelve_sessions_each`); the fixture set under 600 KB.
- After any change to `scenario`, `claude`, `codex` or `opencode`: `cargo run -p adage-sim`, review
  the diff under `sim/fixtures/`, and commit it. `cargo test -p adage-sim` fails while they are stale.
- Fixtures read (`READS`): `sim/` is declared for this crate; a change there also tests
  `adage-cli` and `adage-learn` through the reverse-dependency closure.
- Dependents to run: `cargo test -p adage-learn -p adage-cli` (the hook goldens in
  `crates/adage-cli/tests/golden/` and the learned keys depend on these bytes).
- Pitfall: a harness format change (a new Codex record shape, an OpenCode schema bump) belongs
  here and in `adage-ingest`'s decoder together; record the source version in the module docs as
  `codex.rs` and `opencode.rs` do.
