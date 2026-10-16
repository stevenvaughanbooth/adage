# xtask

Repository tasks, run as `cargo xtask <task>` (alias in `.cargo/config.toml`), and the
architecture guards that run as its tests: the layering rules, the fixture-read declarations CI
selects packages by, repository hygiene, and the Python suite registry. It is the one place those
tables live; `../docs/architecture.md` and `../docs/testing.md` describe them.

## Where it sits

- Layer 0 in `xtask/src/layers.rs`. Links only `serde_json` and `anyhow`; no workspace crate.
- Nothing depends on it. `publish = false`.
- As a layer-0 crate it must never link another Adage crate; it reads the workspace through
  `cargo metadata` and `git`, not through the crates' APIs.

## What's inside

- `main.rs`: dispatch, and `tokens [--check]`: reads `design/tokens/adage.tokens.json` (DTCG),
  writes `crates/adage-term/src/tokens.rs` and `design/build/tokens.css`, checks WCAG 4.5:1
  contrast for every text colour on every ground in both modes. `--check` writes nothing and fails
  when either output is stale.
- `affected.rs`: `cargo xtask affected [--base <ref>] [--files <path>...] [--json]`. `READS` (paths
  outside every package and the packages whose tests read them), `INERT` (paths that affect
  nothing), `members` (from `cargo metadata`), `affected` (the reverse-dependency closure), output
  `all`, `none` or one package per line.
- `suites.rs`: `cargo xtask test python [<suite>...]`. `PYTHON` lists every suite with its import
  root; `FIXTURE_DIRS` names `test_*.py` files that are fixtures, not Adage's tests.
- `release.rs`: `cargo xtask bump <version> [--date <YYYY-MM-DD>] [--no-changelog]` writes the
  version into `Cargo.toml`, the lockfile's members, the plugin and marketplace manifests and the
  npm templates (`VERSIONED`), dates CHANGELOG.md's Unreleased heading, then runs the version check
  and `node scripts/check-plugin.mjs`; `cargo xtask release-check [--require-dist] [--dist <path>]`
  fails while those versions disagree, while `release.yml` names Homebrew or lacks a custom publish
  job or the SBOM patch, and (with dist) while `release.yml` is not what dist generates in a
  scratch tree of links to this one. The release checklist is `docs/release.md`.
- `layers.rs` (test only): `LAYERS`, `FORBIDDEN`, `EXCEPTIONS`, `violations`.
- `hygiene.rs` (test only): `MAX_BYTES` (1 MiB), `RAW_EXTENSIONS`, `ALLOWED`, `violations` over `git ls-files`.

## How it connects

- CI (`.github/workflows/ci.yml`): on a pull request the test job runs `cargo xtask affected` and
  tests those packages (`adage-eval` excluded; it runs in `evals.yml`); `cargo test -p xtask` and
  `adage-exec`'s `no_spawn` test always run; the scheduled `design` job runs `cargo xtask tokens --check`;
  the `plugin` job installs the dist version `dist-workspace.toml` pins and runs
  `cargo xtask release-check --require-dist`.
- `evals.yml` runs `cargo xtask test python` for every suite.
- Every crate that reads a file outside its directory is named in `READS`; the test
  `every_path_a_crate_reads_is_declared` scans all crate sources for such paths and fails on an
  undeclared read into an inert directory. `the_workspace_respects_its_layers` reads the real
  dependency graph.

## Changing it

- `cargo test -p xtask` runs everything here (it needs `cargo` and `git` on `PATH`). It is fast
  and CI runs it on every change.
- Adding a crate: give it a layer in `LAYERS` (an unlisted member fails the test), and declare any
  external fixture it reads in `READS` with the crate's package name (`rules_name_real_packages`).
- Adding a Python test directory: add a `Suite` to `PYTHON`, or the file fails
  `every_python_test_belongs_to_a_suite`.
- Committing a file over 1 MiB, or a `.log`, `.gz`, `.tgz`, `.tar`, `.zip`: add an `ALLOWED` entry
  with the reason, and remove it when the file goes (an unused entry fails the test).
- `EXCEPTIONS` is empty and may only shrink; a listed violation that no longer exists fails the test.
- Changing a design token: edit `design/tokens/adage.tokens.json`, run `cargo xtask tokens`, commit
  both generated files; `the_committed_tokens_are_current` fails otherwise.
- Fixtures read (`READS`): `design/`; `plugin/` and `.claude-plugin/` (the release versions). The
  release tests also read `Cargo.toml`, `Cargo.lock`, `npm/`, `dist-workspace.toml`,
  `.github/workflows/` and every member manifest, which affect everything already.
- Pitfall: the paths `affected` sees are `/`-separated and relative to the workspace root; a
  `READS` prefix ending in `/` matches a directory, any other prefix matches one file exactly.
