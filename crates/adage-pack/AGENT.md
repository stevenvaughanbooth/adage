# adage-pack

Track packs (format `adage-pack/1`): compiled skills as signed, content-addressed, closed
directories; the marketplace format that pins them; how a pack is installed and bound against a
repository; and the record of how a repository's instincts were bound. A pack is inert: nothing
in it grants execution. The crate also builds the `adage-pack` debugging binary.

## Where it sits

- Layer 4 (distribution) in `xtask/src/layers.rs`.
- Links: `adage-core`, `adage-tracks` (the runtime route model a pack binds to), `adage-sign`
  (sshsig over the pack digest), `adage-instincts` (the detectors instinct templates name),
  `adage-intent`.
- Linked by: `adage-hook`, `adage-learn`, `adage-exec`, `adage-cli`, `adage-train`.
- No `FORBIDDEN` edge names this crate. The layer rule keeps it from linking anything at layer 4
  or above (`adage-learn`, `adage-hook`, `adage-exec`, the binaries). It does not link
  `adage-runtime`, so its tests cannot learn tracks: `tests/fixtures/tracks` is committed
  instead (a known gap in [`../../docs/testing.md`](../../docs/testing.md)).

## What's inside

- `lib.rs`: `Pack { manifest, routes, types, scripts, evidence, attestations, procedure }` with
  `load`, `from_files`, `seal`, `new`, `check`, `closure`, `files`, `digest`,
  `route_exec_digest`, `write`. Re-exports the `model`, `bind`, `closure`, `convert` and
  `digest` items.
- `model.rs`: `PackManifest`, `PackInfo`, `RouteV2`, `TriggerV2`, `StepV2`, `Input`, `Output`,
  `TypeDef`, `Shape`, `ClosureLevel`, `Requires`, `Provenance`, `RouteEvidence`,
  `PackEvidence`. Every struct refuses unknown fields.
- `validate.rs`: `read_dir_files`, `parse`, `validate`; the file names `MANIFEST`, `CLOSURE`,
  `EVIDENCE`, `SIG`; limits (`MAX_FILES`, `MAX_ROUTES`, `MAX_STEPS`, `MAX_SCRIPT_BYTES`);
  `safe_id`, `safe_path`, `safe_glob` and the other token predicates.
- `closure.rs`: `compute`, `Closure::render`, `Edge`, `MAX_AFTER_CHAIN`.
- `digest.rs`: `pack_digest`, `digest_dir`, `route_exec_digest`, `canonical_json`, `message`,
  `sign_dir`, `verify_dir`, `NAMESPACE`.
- `types.rs`: `BUILTINS`, `check_typedef`, `pattern_is_strict`, `PROBES`, `resolve`, `Resolved`.
- `bind.rs`: `bind`, `applies`, `Target`, `BindEnv`, `Binding`, `BoundRoute`, `Status`.
- `binding.rs`: the per-repository `Binding` record, `binding`, `binding_path`,
  `stale_binding`, `is_stale`, `selection`.
- `instincts.rs`: `bind`, `admits`, `bound_routes`, `available`, `InstinctBinding`, `Source`.
- `marketplace.rs`: `Marketplace::parse`, `load`, `load_dir`, `Loaded` and `Loaded::sync`,
  `check_entry`, `FORMAT`, `MANIFEST` (`.adage/marketplace.toml`).
- `builtin.rs`: `FILES` (the repository's `.adage/marketplace.toml` and `packs/*` embedded with
  `include_str!`), `marketplace()`, `install`, `fingerprint`, `reserved`.
- `install.rs`: `pack_dir`, `write_verified`, `Record`, `Via`, `records`, `record`, `uninstall`.
- `convert.rs`: `from_track_dir`, `from_track_dir_with`, `from_track`, `Converted`,
  `hash_repo_file`. `vendor.rs`: `vendor`, `script_path`. `attest.rs`: `check`, `summary`.
  `procedure.rs`: `Procedure::check`, `PROCEDURE`.
- `main.rs`: `adage-pack inspect | verify | digest | seal | marketplace [--pin]`.

## How it connects

- Signing: `digest::sign_dir` and `verify_dir` call `adage_sign::sign` and `verify` over
  `message(pack_digest)` in `adage_sign::PACK_NAMESPACE`.
- Binding to the runtime: `bind::bind` turns each `RouteV2` into a `BoundRoute` holding an
  `adage_tracks::Route` (and an `index::ExecBinding` for exec-only routes). It runs at index
  compile time in `adage-learn` and `adage add`, never in the hook.
- Instincts: `instincts::bind` runs `adage_instincts::bind` over a repository's declared files
  and keeps only routes a pack template `admits`. `builtin::install` syncs the embedded
  marketplace into `$ADAGE_HOME/tracks/@imported` before the learner binds.
- Readers: `adage-hook` reads `binding::binding` and `stale_binding` at session start and
  `PreToolUse`; `adage-learn` writes bindings; `adage-cli`'s `add`, `share` and `train` load,
  convert, digest and install packs through this crate (`add` copies vendored scripts into
  `$ADAGE_HOME/cas/` itself); `adage-exec` runs vendored scripts from that cas. Formats:
  [`../../docs/packs.md`](../../docs/packs.md),
  [`../../docs/marketplace.md`](../../docs/marketplace.md).

## Changing it

Invariants to keep:

- A pack is untrusted input: strict files, unknown fields refused, the closure re-derived and
  compared byte for byte with `CLOSURE`, every token through a `safe_*` predicate. A hostile
  pack must be refused with a specific reason (`tests/hostile.rs`).
- Canonical form: `Pack::write` re-reads what it wrote and must get the same `Pack` back.
- Digest stability: evidence and strength never change a route's exec digest; any semantic
  change does (`tests/digests.rs`, proptest).
- `bind` is a pure function of the pack, the repository's files and `BindEnv`; nothing is run
  (tools are found by PATH lookup only).
- `builtin::FILES` must equal the repository's `packs/` and `.adage/marketplace.toml`
  (`tests/marketplace.rs`). After editing a pack under `packs/`, re-seal it (`adage-pack seal`)
  and re-pin the marketplace (`adage-pack marketplace . --pin`), then add the new file to
  `FILES` if it is new.

Tests: `cargo test -p adage-pack` runs `tests/{convert,digests,hostile,marketplace,resolve,
roundtrip,trained}.rs` over the in-memory sample pack in `tests/common/mod.rs`. Fixtures read:
`tests/fixtures/tracks/acme-billing`, `evals/sandboxes/acme-billing`, `packs/` and
`.adage/marketplace.toml`. `READS` in `xtask/src/affected.rs` declares `evals/sandboxes/`,
`packs/` and `.claude-plugin/` for this crate. Then run the dependents: `cargo test -p
adage-learn -p adage-hook -p adage-exec -p adage-cli -p adage-train`.

Pitfalls: the `ssh-key` dev-dependency inherits the root workspace pin used by `adage-sign`;
keep fixture keys and production signature types on that same version. The property tests here have no feature gate and run on every `cargo test`.
Three things are called binding: `bind.rs` (a pack against a repository), `binding.rs` (the
instinct-binding record the hook reads) and `instincts::bind` (detectors over declared files).
`attest.rs` and `procedure.rs` are informational only; nothing may read them to grant a
strength, a level or an approval.
