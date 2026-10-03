# adage-learn

`adage condition`: learning orchestration. Takes canonical sessions (an ingested batch), runs the
graph engine, judges and versions one track per repository (`<repo slug>/learned`), binds
instincts, and compiles the index, reading earlier results and writing everything through one
`Store` port.

## Where it sits

- Layer 5 ("learning orchestration: ingest → mine → write tracks and the index") in
  `xtask/src/layers.rs`.
- Links: `adage-core`, `adage-trace`, `adage-ingest`, `adage-tracks`, `adage-intent`,
  `adage-instincts`, `adage-pack`, `adage-store`, `adage-graph`. Dev-dependencies: `adage-runtime`
  (tests drive the runtime over what was learned), `adage-sim`, `tempfile`.
- Depended on by: `adage-app`, `adage-cli`, `adage-eval`, `adage-exec`, `adage-train`.
- Forbidden (`FORBIDDEN`): `adage-learn -> adage-runtime` ("the learner and the live runtime are
  siblings over the canonical model"). In the other direction `adage-hook -> adage-learn` and
  `adage-exec -> adage-learn` are forbidden. The runtime must stay a dev-dependency only.

## What's inside

- `lib.rs`: `Inputs`, `Report`, `RepoReport`, `TrackReport`, `Planned`, `TRACK` (`"learned"`),
  `LEGACY_TRACKS`; entry points `learn` (transcript paths in), `learn_sessions` (a `Batch` in,
  `store::Files`), `learn_sessions_into` (any `Store`), `plan` (the dry run: everything a run
  would write, nothing written), `route_line`, and the crate-private `bump` (semver rule).
- `store`: the `Store` trait, `Files` (`$ADAGE_HOME` layout), `Compiled`.
- `engine`: the one learner, which knows no route kinds. `run(sessions, cfg, priors, model) ->
  EngineRun`: mine (`adage_graph::select::learn`), compile (`adage_graph::compile`), then judge
  each route on the traces; `exposed`, `verdict`, `EngineReport`.
- `evidence`: `LearnConfig` (`min_support`, `fire_lower_bound`, `prime_rate`, `retain_rate`,
  `material_lower_bound`, `dormant_exposure`, `instincts: Option<instincts::Config>`, `priors`,
  `uplift_min`, and more; `demo()`, `from_params`, `for_thresholds`).
- `graph` (`lookup_routes`, `compute_facts`), `dormant` (exposure and dormant routes, E1), `value`
  (directive value and uplift: `assess`, `Priors`, `Statics`), `outcome` (local rewards: `Goal`,
  `Runner`, `situations`), `cases` (the case store), `testmap` (tests of the edited file), `cards`
  (procedure cards: `compile`, `CardParams`), `claims` (skill claims, for `adage-train`), `effects`
  (observe-from-traces for scripts the command table does not know).
- `instincts`: binding inside a learn run: `Config`, `plan_repo`, `learn_only_into`,
  `track_name`, `should_extinguish`, `judgements`, `binding_now`, `preinstall`.
- `ledger` (`$ADAGE_HOME/ledger/<slug>.jsonl` session summaries), `diff` (what a run changed, for
  `learn.log`), `import` and `bridge` (re-exports only: decoding is `adage_ingest`'s, the bridge
  is `adage_graph::canonical`).
- Re-exports: `adage_ingest::{cache, codex, opencode, scope, read_session}`,
  `adage_tracks::layout::{follow_weights_path, read_follow_weights, slug}`.

## How it connects

- In: an `adage_ingest::batch::Batch` of `adage_trace::Session`s. `learn` builds one with
  `adage_ingest::batch::read`; `learn_sessions*` accept one from anywhere, including traces
  decoded `AsRecorded` from another machine (`tests/remote_sessions.rs`). An optional models
  directory picks the embedding model with `adage_intent::model::resolve_static`; without one,
  intent routes are not learned and the existing ones are kept.
- The `Store` port (`src/store.rs`): reads earlier `Track`s, `Reviews`, `Proposals`, `Why`,
  instinct `Binding`s and packs, by track name; writes `Planned` tracks, bindings and follow-judge
  weights; `compile_index` once, after every write. `Files` is the production implementation;
  `tests/remote_sessions.rs` has `Rows`, a store held entirely in memory.
- Instincts: `instincts::plan_repo` calls `adage_pack::instincts::bind`, which runs
  `adage_instincts::bind_with` and keeps the routes a turned-on pack's templates admit; the result
  is the track `<repo slug>/instincts`.
- Out: tracks, `review.yaml`, `proposals.yaml`, `.why.json` (formats owned by `adage-tracks`),
  the compiled `index.json`, and a `Report`.
- Design: `../../docs/architecture.md`, "Learning engine (`adage-learn`, `adage-graph`)".

## Changing it

- Every write goes through `Store`; never touch the tracks root directly
  (`a_learn_run_materializes_only_through_its_store` asserts nothing on disk and exactly one
  `compile_index`, last). Every read of earlier results also goes through `Store`, by track name.
- A person's decisions win: when `reviews()` errors, `plan` leaves that repository's track as it
  is; authored routes are never overwritten (a proposal is recorded instead). Learned routes are
  untrusted content: one that fails `adage_tracks::io::validate` is dropped alone and never aborts
  the repository.
- Determinism and versions: `plan` sorts sessions by id; the same traces give the same track and
  version (`end_to_end.rs` `learning_is_deterministic_and_versioned`, `cache.rs`
  `cached_and_uncached_learns_write_identical_bytes`, `remote_sessions.rs`). `bump` ignores
  intent centroid and threshold drift when deciding a version.
- Steering by influence: a session that received a directive naming a command is never evidence
  for it (`dormant.rs`, `engine::exposed`, `end_to_end.rs`
  `steered_traces_are_exposed_only_to_what_their_directives_named`).
- Tests: `cargo test -p adage-learn`. Goldens in `tests/golden/` (`codex-session.txt`,
  `opencode-session.txt`), re-blessed with `ADAGE_BLESS=1`. Fixtures: `tests/fixtures/` in the
  crate; `evals/sandboxes/acme-billing` (declared for `adage-learn` in `READS`,
  `xtask/src/affected.rs`); `sim/fixtures/codex` through `adage_sim::fixtures_dir()` (covered by
  the `adage-sim` dev-dependency). `tests/intent.rs` and `tests/cards.rs` install a small fixture
  model into a temporary models directory. Then `cargo test -p adage-app -p adage-cli`.
- Pitfalls: `LearnConfig { instincts: None, .. }` is how tests skip binding and the base-pack
  install; `LearnConfig::demo()` lowers the evidence bars. `tests/remote_sessions.rs::Rows` is the
  reference for implementing `Store` elsewhere.
