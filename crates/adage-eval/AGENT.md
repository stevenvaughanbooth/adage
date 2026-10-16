# adage-eval

Offline evaluation: learn from a train split into a temporary home, replay held-out sessions
event by event through the real runtime in the control arm, and score what Adage would have said
against what the agent did next. Nothing runs, no model is called, and the same dataset gives the
same bytes. Long form: `../../docs/evals.md`.

## Where it sits

- Layer 6 in `xtask/src/layers.rs`, beside `adage-hook`.
- Links: `adage-core`, `adage-tracks`, `adage-safety` (re-exported as `adage_eval::safety`),
  `adage-runtime`, `adage-learn`, `adage-intent`, `adage-graph`; `tempfile` is a normal dependency.
- Dev-dependencies: `adage-store`, `proptest`.
- Depended on by `adage-cli` only: `adage eval …` is `adage_eval::cli::run`.
- `FORBIDDEN` has `adage-exec -> adage-eval`: execution must never link this crate.

## What's inside

- `lib.rs`: `Options`, `InstinctMode`, `prepare` (dataset, split, learned or loaded index),
  `evaluate`, `run`, `Report`, `render_text`, `learn_index`, `engine_params`.
- `cli`: `adage eval <dataset>`, and subcommands `routes`, `types`, `mine`, `fidelity`, `generate`,
  `bench`, `replay-savings`, `embeddings`.
- `dataset`: `manifest.json`, `sessions/*.jsonl`, `sandbox.txt`, `conventions.json`; `Sandbox::create`.
- `transcript`: a Claude Code transcript as the hook events the adapter would raise.
- `replay`: `adage_runtime::decide` over those events, control arm; `FastCheck` for the no-match fast path.
- `score`: `Metrics`, `Recall`, `DeadEnds`, `FinishLine`, `Safety`, `Tally`, `score_session`.
- `fidelity`, `conventions`, `generate` (synthetic and adversarial datasets), `graph_mine`,
  `value_types`, `value_types_e2e`, `exec_e1`, `savings_replay`, `embed_eval`.
- `paired`: held-out task quality/token/cost qualification. `experiment`: frozen protocols,
  content-addressed objects, scheduled jobs and complete workflow receipts. Both are offline;
  the live harness supplies independent grades and provider accounting.
- `bench/`: the L0 bench (`run`, `sweep`, `space`, `report`), JSON Lines to `evals/bench/result.schema.json`.

## How it connects

- In: `evals/datasets/<name>/` on disk; engine parameters from `adage_tracks::params::EngineParams`
  presets with `--params` overrides; the learner through `adage_learn::learn` and
  `adage_learn::instincts::learn_only`; the miner through `adage_graph`.
- Out: `Report` as JSON or text; bench results as JSON Lines compared with `evals/baselines/`.
- Replay builds an `adage_runtime::Ctx` with `checkout::Disk` over a temporary sandbox; every
  directive is held to `adage_safety`.

## Changing it

- Invariants: determinism (same dataset, same bytes; CI diffs two runs); replay never runs a
  command; a gate threshold moves only with a re-measurement and a note in `docs/evals.md`.
- Tests: `cargo test -p adage-eval` runs unit tests and the property tests at small case counts.
  `tests/gates.rs`, `bench.rs`, `parity.rs`, `g4.rs` are `#![cfg(unix)]` and take hours in a debug
  build; `ci.yml` excludes `adage-eval` from the affected set and `evals.yml` runs them weekly or
  on dispatch. `g4` needs `python3` (skips without it). `cargo test -p adage-eval --features slow`
  raises the proptest case counts (`prop_render`, `prop_shell`, `prop_store`, `prop_tracks`).
- Fast evidence contracts run on every PR: `cargo test --locked -p adage-eval --lib --test paired
  --test experiment`. Keep those suites independent of paid APIs and long replay gates.
- Re-blessing: `ADAGE_BLESS=1 cargo test -p adage-eval --test gates` rewrites the generated
  datasets; `adage eval bench run --suite core --deterministic --out evals/baselines/…` the baseline
  (`bench.rs` must reproduce `evals/baselines/l0-core.jsonl` exactly).
- Fixtures read (`READS`): `evals/datasets/`, `evals/lab/`, `demo/sandbox/`
  (`generate::ACME_CONVENTIONS` includes `demo/sandbox/conventions.json`).
- Dependents to run: `cargo test -p adage-cli` (the `eval` dispatch and `g4_hook.rs`).
- Pitfall: `instincts` is off by default so committed numbers are learned routes alone; binding
  them needs the sandbox's files (`--sandbox-dir`, default `evals/sandboxes/<repo>`).
