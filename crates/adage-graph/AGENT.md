# adage-graph

The graph engine (Track G): mines reflexes from canonical sessions and compiles them to routes.
Value-flow induction and learned value types (G1), session facts and embeddings (G0), event mining,
scoring and MDL selection (G2), and the compiler from reflexes to delivery shapes (G3). Mining is a
pure function of the set of sessions; the only filesystem code is the facts cache (`cache`) and
`Epoch::write`/`read` in `proto`.

## Where it sits

- Layer 3 ("engines") in `xtask/src/layers.rs`. Engines never link each other: this crate links
  none of `adage-ingest`, `adage-runtime`, `adage-instincts`, `adage-safety`.
- Links: `adage-core`, `adage-intent`, `adage-trace`, `adage-tracks`. Dev-dependencies add
  `adage-ingest` (tests read traces as the learner does), `proptest`, `regex`, `tempfile`.
- Depended on by: `adage-learn` (the production caller: `adage_learn::engine` and
  `adage_learn::graph`), `adage-eval`, `adage-cli`.
- Must never link `adage-learn` (layer 5) or `adage-runtime` (a sibling); `Cargo.toml` says so.
  No `FORBIDDEN` row names this crate.

## What's inside

- `lib.rs`: `Params`, `learn(sessions, params) -> Learned` (G1: value slots with a `Contract`:
  `Pattern(Induced)`, `RepoPath`, `None(Reject)`), `SlotType`, `literal_prefix`.
- `input`: the engine's own session (`Session`, `Prompt`, `Step::{Exec, Read, Edit}`,
  `Annotated`, `StepMeta`; bounds `MAX_STEPS`, `MAX_OUTPUT`, `MAX_SOURCE_TOKENS`).
- `canonical`: the bridge from `adage_trace::Session`: `to_input`, `annotate`, `annotate_prefix`,
  `influence` (directives the session received, which mining discounts), `events`; traits
  `DirectiveIdentities`, `Exposure`.
- `flow`: value-flow detection (`observe`, `observe_all`, `fold`, `Slot`, `SinkKey`,
  `FlowParams`). `types`: grammar induction (`induce`, `ValueType`, `Induced`) with `anti`,
  `lattice`, `skeleton`, `safety` (`Reject`, `check_shape`, `plain_value`).
- `facts`: `SessionFacts`, `facts`, `facts_all`; `embed`: `Embedder` (prompt, act and art vectors;
  SHA-256 feature hashing, `SEED`, `EMBED_VERSION`); `cache`: `FactsCache` under
  `$ADAGE_HOME/cache/facts`.
- `events`: `EventSession`, `Event`, `Influence`, `from_input`, `LabelTable`, `Corpus`,
  `build_corpus`; `proto`: `Epoch`, `ProtoParams`, `build` (prototypes and the label taxonomy).
- `mine`: `MineParams`, `Db`, `Pattern`, `mine`; `mine::seq` (target-rooted growth),
  `mine::closed` (closure), `mine::border` (`Incremental`, `full`).
- `score`: `ScoreParams`, `Scored`, `score_all`, and the next-action models (`NextModel`,
  `Order1`, `Order2`); `stats`: fixed-order beta and Wilson arithmetic (`posterior`,
  `wilson_lower`, `advantage`, `advantage_mix`).
- `select`: `Params` (`demo()`, `with_threads`), `Reflex`, `Learned`, `learn(&[EventSession],
  &Params)` (the G2 pipeline), `learn_split`, `candidates_shaped`, `matches_convention`.
- `compile`: `CompileParams`, `Corpus`, `Input`, `Prior`, `Output`, `compile`, `outcome_gate`;
  `compile::shapes` (shape choice, `housekeeping`), `compile::ids` (`route_id`), `compile::stable`
  (`LOOKUP_ID`, `dead_end_id`, `finish_line_id`, `runner`, `failure_signature`), `compile::glob`
  (`lattice`, `widen`), `compile::intent` (`compile`, `IntentParams`), `compile::variants`
  (`covering`, `covers`).

## How it connects

- In: `adage_trace::Session`s through `canonical` (the learner gets them from `adage-ingest`);
  instinct routes as priors (`compile::Prior`); an optional `adage_intent::model::StaticModel`
  (`Embedder::new(Option<&StaticModel>)`) for prompt embeddings.
- Out: `select::Learned` (reflexes with their `why`), `compile::Output` (routes as
  `adage_tracks::Route` plus `adage_tracks::why::Why`), and G1 `Learned` (slot contracts).
  `adage_learn::engine::run` calls `select::learn`, `compile::compile`, `compile::intent` and
  `adage_graph::learn`.
- Clustering, purity and quantisation come from `adage-intent` (`cluster`, `purity`, `quantize`).
- Design: `../../docs/architecture.md` ("Learning engine") and
  `../../docs/tracks/track-g-graph-engine.md`.

## Changing it

- Determinism is the contract: the same sessions give byte-identical output for any input order
  and any `threads` value (`tests/determinism.rs`; `tests/mining.rs`
  `thread_count_and_input_order_change_nothing`; `tests/border.rs`: incremental mining equals a
  full re-mine). Keep `BTreeMap` and sorted orders, content-derived label ids, fixed-order float
  sums (`stats.rs`), SHA-256 hashing in anything that reaches output. `Params::threads` and
  `select::Params::with_threads` must never change a result.
- Route ids are content-derived and must not move when traces or clustering change
  (`compile::ids`, `compile::stable`): `review.yaml` decisions key on them and instinct ids line up
  with them.
- There are two `learn`s: `adage_graph::learn` (G1, over `input::Session`) and
  `adage_graph::select::learn` (G2, over `events::EventSession`).
- Tests: `cargo test -p adage-graph`. `tests/common/` builds scripted sessions in memory;
  `tests/support/` reads committed datasets from `evals/datasets/` through `adage-ingest`
  (declared for this crate in `READS`, `xtask/src/affected.rs`). `tests/kinds.rs` also runs
  `evals/gen/gen.py` and `rehearse.py` into a temporary directory when `python3` is present
  (skipped otherwise); `evals/gen/` is not declared for this crate in `READS`. The proptest
  files (`tests/agreement.rs`, `tests/border.rs`) run in the default test run; the scale test in
  `tests/mining.rs` and `tests/profile.rs` are `#[ignore]` and meant for `--release`.
- Then run `cargo test -p adage-learn` (its goldens and end-to-end tests move with any scoring,
  selection or compiling change) and `cargo test -p adage-eval`.
