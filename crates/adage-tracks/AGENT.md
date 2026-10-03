# adage-tracks

The track format and the compiled index: routes with typed slots and contracts, effect ceilings,
evidence, reading and writing track directories, validation, canonical digests, and the matching
primitives the learner and the live runtime share. Every other engine and front end speaks this
crate's types.

## Where it sits

- Layer 2 in `xtask/src/layers.rs`: "the reflex model (tracks, the index) and the matching
  primitives over traces that the learner and the live runtime share".
- Links (normal dependencies): `adage-core`, `adage-trace`, `adage-intent`. Nothing else in the
  workspace.
- Depended on by: `adage-graph`, `adage-instincts`, `adage-runtime`, `adage-safety`, `adage-pack`,
  `adage-learn`, `adage-hook`, `adage-eval`, `adage-exec`, `adage-app`, `adage-cli`, `adage-train`.
- `adage-ingest` must never link this crate (`FORBIDDEN`: "ingestion produces sessions, not
  tracks"). The runtime and the learner meet here and must not depend on each other, which is why
  `params.rs` lives here (see its module docs).

## What's inside

- `model`: `Track`, `Manifest`, `TrackInfo`, `Scope`, `Gauge`, `Route`, `Trigger`, `Step`, `Slot`,
  `SlotSource`, `Contract`, `CtxItem`, `Exit`, `Evidence`, `ExecutionEvidence`, `Uplift`,
  `Strength`, `RouteState`, `Origin`; bounds `CONTEXT_MAX_ITEMS`, `CONTEXT_MAX_WITHIN`,
  `SEQUENCE_MAX_COMMANDS`, `MAX_INTENT_ROUTES`; `ceiling`, `delivered_strength`, `initial_state`,
  `shape`, `wilson_lower`. Re-exported at the crate root (`pub use model::*`).
- `io`: `write`, `read`, `read_all`, `read_all_dirs` (`track.toml` plus `routes/<id>.yaml`);
  `validate` (every track is untrusted input); `classify`, `safe_id`, `safe_name`, `safe_command`,
  `safe_text`, `contract_is_strict`.
- `index`: `Index` (`compile`, `compile_with_conflicts`, `compile_root`, `write_atomic`, `load`,
  `try_load`, `for_repo`, `triggers`, `contexts`), `IndexedRoute`, `ExecBinding`,
  `exec_bindings`, `is_imported`, `is_instinct_track`.
- `interlock`: the rules that stop installed routes conflicting (`Conflict`, `ConflictKind`,
  `Resolution`, `resolve`, `avoids`, `two_fires`, `same_point`).
- `triggers`: `Triggers` (`of`, `call_might_match`, `prompt_might_bind`, `calls_matter`), the
  coarse key set the hook asks before loading anything else.
- `context`: `CtxNet`, `DNode`: a repository's context alphabet and discrimination net.
- `matching`: `CtxState`, `Symbols`: recent alphabet symbols and how an event is symbolised. Used
  by `adage_runtime::context` and `adage_learn::engine`.
- `review` (`review.yaml`: `Reviews`, `Review`, `Decision`, `Basis`, `reconcile_with`,
  `keep_dormant`, `material_change`), `proposals` (`proposals.yaml`: `Proposals`, `propose_with`),
  `why` (`.why.json`: `Why`, `load`, `save`).
- `advice` (`Step::Advise` templates: `TEMPLATES`, `check`, `text`), `card` (`Step::Card`: `Card`,
  `check`, `text`, size bounds), `lift` (`OutcomeLift`, `diff_lower_bound`), `layout` (`slug`,
  `follow_weights_path`, `read_follow_weights`, `INSTINCTS_TRACK`), `params` (`EngineParams`,
  `RuntimeParams`, `LearnParams`; presets `default` and `demo`).

## How it connects

- No trait of its own: it is the shared data model and file formats. The learner writes tracks
  with `io::write` and the review, proposals and why files, then compiles with
  `Index::compile_root` and `write_atomic`. The hot path reads only the index (`Index::load`),
  never track directories (`index.rs` module docs).
- `Trigger::Intent` stores a centroid quantised with `adage_intent::quantize`; `matching.rs`
  compares prompts with `adage_intent::cosine` over `dequantize`.
- `io::classify` and `advice::check` take `adage_core::CommandTable`; `Effect` and `SpanKind` are
  `adage-core` types.
- Component map: `../../docs/architecture.md` ("Store" row and the layers table).

## Changing it

- The index is deterministic: the same tracks give the same bytes and the same `generation`
  (`index_is_deterministic` in `src/lib.rs`). Anything compiled from routes must use sorted,
  content-derived order. `Triggers` and `CtxNet` are derived from routes and are not part of the
  generation (`context.rs` docs).
- Floats must survive a write and a read: `tests/index_roundtrip.rs` relies on serde_json's
  `float_roundtrip` feature, set in the workspace `Cargo.toml`. Do not change serde_json settings
  per crate.
- `io::validate` is the safety boundary for every track from every source (learned, imported,
  instinct, authored). A new `Trigger`, `Step` or `CtxItem` variant needs validation rules and
  hostile cases (`src/lib.rs` tests, `tests/hostile.rs`); the `hostile.rs` cases came from
  `adage-eval`'s property tests (`prop_tracks.rs`).
- Advisory and card text is rendered from fixed templates here, and `advice.rs` states the safety
  oracle accepts exactly these sentences. Changing a template changes what is accepted downstream.
- Changing the route model changes the bytes of `routes/<id>.yaml` and `index.json`:
  `adage-learn`'s goldens (`tests/golden/`) and its version bumps (`adage_learn::bump`) move.
- Tests: `cargo test -p adage-tracks` (unit tests in `src/`, `tests/hostile.rs`,
  `tests/index_roundtrip.rs`). No fixtures outside the crate (`READS` in `xtask/src/affected.rs`
  has no entry for it). Then run at least `cargo test -p adage-runtime -p adage-learn
  -p adage-instincts -p adage-graph`; `cargo xtask affected` lists the full set.
