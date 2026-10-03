# adage-intent

Intent families: prompt normalisation, a pinned static embedding model (potion-base-8M) loaded
in-process with no dependencies beyond std, and deterministic clustering. Same input, same bytes,
on every machine.

## Where it sits

- Layer 1 ("the canonical trace model; the intent model") in `xtask/src/layers.rs`, beside
  `adage-trace`.
- Links no workspace crate. External: `serde`, `serde_json`, `regex`, `once_cell`, `sha2`,
  `anyhow`, `miniz_oxide` (gunzip for npm tarballs).
- Depended on by: `adage-tracks` (quantised centroids and `cosine` in `matching`), `adage-graph`
  (`cluster`, `purity`, `quantize`, `model::StaticModel`), `adage-learn`, `adage-runtime`,
  `adage-hook`, `adage-pack`, `adage-eval`, `adage-cli` (the only one enabling `bundled`).
- No `FORBIDDEN` row names it. Keep it free of workspace dependencies: layer 0 holds only kernel
  crates and nothing here needs them.

## What's inside

- `lib.rs`: `SCALE`, `quantize`, `dequantize`, `unit`, `cosine`: centroids travel in tracks as
  `i16` in units of 1e-4.
- `normalize`: `normalize`, `normalize_full`, `Normalized`, `HEAD_TOKENS`, `MAX_INPUT`: typed
  placeholders for ids, paths, SHAs, versions and URLs; fenced code stripped; only the head kept.
- `tokenize`: `WordPiece` (`encode`), `pre_tokenize`: the BERT uncased WordPiece tokenizer in
  plain Rust.
- `model`: `Spec` (`BASE_8M`, `CODE_16M`, `SPECS`; every upstream file pinned by SHA-256),
  `StaticModel` (`open`, `load`, `embed`, `from_static`), `Manifest`, `resolve(models_root,
  load_all) -> Resolved`, `resolve_static`, `Resolved` and `Source` (which model is in use and
  why), `selection`, `install`, `install_spec`, `verify`, `derive_i8`, `status`; the bundled
  model (`BUNDLED_SPEC`, `BUNDLED_TABLE_SHA256`, `BUNDLED_VOCAB_SHA256`, `bundled`,
  `bundled_id`, `bundled_verified`) behind the `bundled` feature; `hashed_embed`, the
  always-available fallback.
- `npm`: `gunzip`, `entries`, `extract`: a model's files out of its npm tarball, each bounded by
  its pinned size.
- `cluster`: `knn`, `graph`, `louvain`, `finish`, `families`, `Params`, `dot`: exact cosine kNN,
  deterministic Louvain, minimum family size.
- `purity`: `Purity`, `split`, `merge`, `dominant`, `centroid`, `members`: behaviour decides what
  embeddings propose.
- Examples: `derive_bundle` (rebuilds `bundled/` from the pinned upstream files and prints the
  digests), `embed`, `reflex_embedding_probe`.

## How it connects

- No trait. Callers embed with `StaticModel::embed(&normalize(text))` and compare with `cosine`.
  `adage_tracks::Trigger::Intent` stores a `quantize`d centroid and a model id, and
  `adage_tracks::matching` matches a prompt with `cosine` over `dequantize`.
  `adage_graph::compile::intent` and `adage_graph::proto` cluster with `cluster` and `purity`.
  `adage_learn::plan` chooses the model with `model::resolve_static`.
- Model selection (`model::resolve`): installed under `$ADAGE_HOME/models/<key>/`, else bundled,
  else hashed; `ADAGE_EMBED_MODEL` or `$ADAGE_HOME/models/active` selects.
- Component map: `../../docs/architecture.md` (layers table).

## Changing it

- Determinism is the contract for every module (`lib.rs` docs): fixed summation order in
  `cluster::dot`, kNN ties by index, Louvain visiting nodes in index order and moving only on a
  strictly better gain, `quantize` rounding and clamping. Keep it so.
- Learning and matching both use this tokenizer and normaliser (`tokenize.rs` docs). Changing
  either moves every vector, while centroids already stored in tracks were computed with the old
  ones and are matched against the model id in `Trigger::Intent`.
- The bundled bytes are pinned: `bundled/potion-base-8m-i8.safetensors` and
  `bundled/potion-base-8m.vocab.txt` must hash to `BUNDLED_TABLE_SHA256` and
  `BUNDLED_VOCAB_SHA256` (`bundled_verified`). Regenerate with
  `cargo run -p adage-intent --example derive_bundle -- <upstream dir>` and update both constants.
- The `bundled` feature is off in this crate's `Cargo.toml` (no `default` features); `adage-cli`
  turns it on. `cargo test -p adage-intent` builds without it unless feature unification turns it
  on, so the `#[cfg(feature = "bundled")]` paths need `cargo test -p adage-intent --features
  bundled`. The `model.rs` docs say "on by default", which describes the binary, not this crate.
- Everything in `model` fails open: a truncated file or a wrong digest skips that source.
- Tests: unit tests only, in `src/` (`cargo test -p adage-intent`, and again with `--features
  bundled`). No `tests/` directory, no reads outside the crate. Then `cargo test -p adage-tracks
  -p adage-graph -p adage-learn -p adage-runtime`; `adage-learn/tests/intent.rs` installs a fixture
  model in the installed layout and exercises `resolve_static`.
