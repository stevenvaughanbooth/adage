# adage-instincts

Instincts: prebuilt routes bound per repository from its declared files, working on day 0 with no
session history. This crate is the detectors: bounded, read-only access to a repository's files,
the declared facts read from them, and the binder that turns the catalogue's detectors into
concrete `adage_tracks::Route`s.

## Where it sits

- Layer 3 ("engines") in `xtask/src/layers.rs`. Engines never link each other; this crate links
  none of `adage-ingest`, `adage-graph`, `adage-runtime`, `adage-safety`.
- Links: `adage-core`, `adage-tracks`. External: `serde`, `serde_json`, `toml`, `regex`,
  `once_cell`.
- Depended on by: `adage-pack` (packs are templates naming detectors here;
  `adage_pack::instincts::bind` drives the binder), `adage-learn`, `adage-cli`, `adage-train`.
- No `FORBIDDEN` row names it. It sees only a repository root: it does not link `adage-trace`
  (no sessions) or `adage-store` (none of Adage's own state).

## What's inside

- `lib.rs`: re-exports `bind`, `bind_with`, `bind_facts`, `BindOptions`, `BindResult`, `Advice`,
  `catalogue`, `Catalogue`, `Instinct`, `extract`, `DeclaredFacts`; plus `is_instinct`,
  `DECLARED_FILES`, `fingerprint` (a stat-only fingerprint of the declared files that changes
  when what binds may change), `VerifiedEnv`, `verify_env`, `pack`.
- `catalogue`: `data/catalogue.toml`, embedded with `include_str!`; `Catalogue` (`get`,
  `provenance`, `lockfiles`), `Pack`, `Instinct`, `Probes`, `Generators`.
- `files`: `RepoFiles` (`open`, `read`, `head`, `read_lines_prefix`, `walk`, `is_file`, `is_dir`,
  `notes`); bounds `MAX_FILE_BYTES`, `HEAD_BYTES`, `MAX_READS`, `MAX_WALK_ENTRIES`,
  `MAX_WALK_DEPTH`; `safe_rel`. Never follows a symlink out of the repository, never executes.
- `declared`: `extract`, `extract_from`, `DeclaredFacts` (`Entry`, `Source`, `Verdict`,
  `PythonFacts`, `CargoFacts`, `GoFacts`, `PackageManager`, `Generated`, `GeneratorLink`),
  `run_steps` (CI `run:` steps), `generated_marker`, `glob_for`, `safe_glob`.
- `safety`: what a declared command and its body do, classified with adage-core's normaliser
  (`check_line`, `check_body`, `command_key`, `segments`, `repo_script`, `stable`,
  `INTERPRETERS`).
- `detect`: one detector per catalogue entry (`detect`, `BIND_ORDER`, `RouteCandidate`,
  `AdvisoryCandidate`, `Detected`, `Ctx`, `wrappers`, `runner_missing_signatures`,
  `CANONICAL_CHECKS`).
- `bind`: the binder's rules in order (one directive per trigger; ids the learner would derive;
  learned routes and review rejections win; capped; validated); `check_route`, `check_advisory`,
  `plain_command`.
- Example: `cargo run -p adage-instincts --example dry_run -- <repo-dir>` prints what binds and
  what is skipped; nothing is written.

## How it connects

- In: a repository root, the repository's other routes (`existing: &[Route]`: learned, imported,
  authored) and the route ids a person rejected (`BTreeSet<String>`). Out: `BindResult` with
  `routes` (`runs()`) and `advisories()` (`Step::Advise` from the fixed templates owned by
  `adage_tracks::advice`).
- In production the caller is `adage_pack::instincts::bind` (from `adage_learn::instincts::plan_repo`
  and `adage-cli`'s init), which calls `bind_with` with `BindOptions` and keeps only routes a
  turned-on pack's templates admit. The learner writes them as the track `<repo slug>/instincts`
  (`adage_tracks::layout::INSTINCTS_TRACK`). Every bound route passes
  `adage_tracks::io::validate`, and the index gives every other source precedence over instincts
  (`adage_tracks::interlock`, "Instincts last").
- Reference: `../../docs/instincts.md` (the catalogue for people) and
  `../../docs/architecture.md`.

## Changing it

- Determinism: the same repository files give byte-identical results wherever the checkout lives
  (`tests/bind.rs` `binding_is_deterministic_and_independent_of_where_the_repository_is`). Sort
  everything; no hash-map order may reach the output (`bind.rs` docs).
- Ids must be the ones the learner derives (`dead-end-<stable(...)>`, `finish-line-<stable(glob,
  command)>`; the learner's forms are in `adage_graph::compile::stable`), so the index's
  de-duplication works (`the_index_keeps_the_learned_route_over_an_instinct_with_the_same_id`).
- Caps: prime at most, forming (environment steps nascent), reactive triggers only, provenance
  `instinct:<pack>@<version>`, no evidence (`every_bound_route_is_a_valid_capped_instinct`). A
  command whose body is not plain, or classifies above `regenerate`, never binds
  (`hostile_repositories_never_bind_their_commands`, the `tests/fixtures/hostile-*` repositories).
- The catalogue and `docs/instincts.md` must agree: `catalogue_and_docs_agree` fails when a
  catalogue id has no row in the doc. A new instinct touches `data/catalogue.toml`, a detector in
  `detect.rs` (and `BIND_ORDER`), the doc row, and a fixture.
- Tests: `cargo test -p adage-instincts`. `tests/bind.rs` reads `tests/fixtures/*` (harbor, pnpm,
  rust, go, pytool, decoy, hostile-*) and, outside the crate, `evals/sandboxes/acme-billing` and
  `docs/instincts.md`; `tests/generated.rs` runs `evals/gen/gen.py` into a temporary directory
  (skipped without `python3` and `git`). All three outside paths are declared for
  `adage-instincts` in `READS` (`xtask/src/affected.rs`); declare any new one there, or
  `cargo test -p xtask` fails for an inert path. Then `cargo test -p adage-pack -p adage-learn`.
