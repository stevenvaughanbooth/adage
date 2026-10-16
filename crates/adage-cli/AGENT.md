# adage-cli

The `adage` binary: the hook client, every command a person types, the status card, the TUI
(`adage watch`, `adage ui`) and the `adage map` HTTP server. It parses arguments and renders; the
rules live below it in `adage-app` and the engines. Front-end map: `../../docs/architecture.md`.

## Where it sits

- Layer 9 in `xtask/src/layers.rs` (binaries). Nothing depends on it.
- Links: `adage-core`, `adage-trace`, `adage-store`, `adage-tracks`, `adage-intent` (`bundled`),
  `adage-runtime`, `adage-learn`, `adage-graph`, `adage-instincts`, `adage-sign`, `adage-pack`,
  `adage-eval`, `adage-hook`, `adage-exec` (library only: it never runs a route), `adage-app`,
  `adage-term` (`ratatui`); plus `clap`, `clap_complete`, `ratatui`.
- Dev-dependencies: `adage-sim` (fixtures and replays), `adage-train` (`adage train accept`), `insta`.
- No `FORBIDDEN` edge names it. It must not spawn processes except the listed uses in
  `crates/adage-exec/tests/no_spawn.rs` (`ALLOWED`): a new `Command::new` fails that test.

## What's inside

- `main.rs`: dispatches `adage hook` first (fail open, before clap), then every command.
- `cli.rs`: the whole clap grammar; tests check every flag a module reads is declared.
- `helpers.rs`: the one copy of each small helper the commands share: `flag`, `flags`, `ago` (a
  card sentence), `ago_short` (doctor's terse form), `plural`. Use these rather than writing
  another; switch markers come from `Paths` (`delegate_marker`, `rewrite_off`, `predict_off`,
  `Paths::repo_disabled`), never a joined string.
- Commands, one module each: `init`, `doctor`, `install` (with `placement.rs`, one table per
  `Harness` for install and detection, and `detect.rs`), `condition` (`main.rs`, `autolearn`,
  `history`, `condition_out`), `sense`, `review` and `proposal`, `exec` (`delegate`, `review delegate|revoke`),
  `savings`, `graph`, `map`, `share`/`share_pack`/`add`/`packs`/`reimport`, `packs_cmd`,
  `marketplace_cmd`, `models`, `config`, `train` (execs the `adage-train` binary; `accept` installs
  the staged pack here), `update`, `status_card`, `corpus_cmd` (`adage corpus export|import|inspect|
  learn`, canonical trace corpora through `adage_learn::corpus`).
- `claude_plugin.rs`: the Claude Code plugin's own files, embedded at build time by `build.rs`
  (`plugin/claude-code` minus `dist/`, the mod's tests and fixtures), staged with this binary and
  its companions as the local marketplace `adage-local` under `$ADAGE_HOME/plugin/`, and installed
  through `claude plugin …` behind the `ClaudeCli` trait (`adage init`, `adage update`).
- `net.rs`: the one place `adage` reaches the network (curl, `https` only; `ADAGE_OFFLINE=1`), for
  `models fetch`, `init`'s verified release companions and `update --check`.
- `map/`: `mod.rs` (options, listener, and `Server`, a map `adage ui` starts on a free port and
  closes with the terminal), `http.rs` (`/api/v1/graph`, `/graph.json`, `/api/v1/events`,
  `/api/v1/health`, `/api/v1/trace?session=&repo=` (one whole recorded session),
  `/api/v1/reflex/<id>`, CSP), `events.rs` (SSE hub), `assets.rs` and the generated
  `assets_list.rs` (the embedded page).
- `ui/`: ratatui app, watch pane, wizard, tabs (Home among them), tailers; `forms.rs` and
  `tools.rs` are the palette's actions, most run as this binary in a child process: one only a
  person may take (`Tool::person_command`) checks for a person first and lifts only the child's
  terminal check (`ADAGE_ALLOW_NONINTERACTIVE=1`; the child keeps every agent-session check);
  snapshots under `ui/snapshots/`.
- `doctor/`: every check, each `warn`/`fail` with a typed `Fix` (`Fix::run` for one whole `adage`
  command, `Fix::manual` for the person's own step); `hook activity: <harness>` reads
  `adage_hook::receipt`. See `../../docs/claude-mod/impl/structure-cli.md`.
- `assets/map/`: the built web map, precompressed (`.br`, `.gz`), listed in `hygiene::ALLOWED`.

## How it connects

- `adage hook` calls `adage_hook::main` with no extension; `adage-exec hook` is the same pipeline
  with one, and the hook goldens in `tests/golden/` pin both.
- Review, graph, savings, evidence and words come from `adage_app`; `adage eval` is
  `adage_eval::cli::run`; approvals go through `adage_exec::approvals`.
- The map page is `web/map/`. `web/map/scripts/embed.mjs` builds `dist/` into `assets/map/` and
  regenerates `src/map/assets_list.rs`; `web/map/scripts/drift.mjs` fails CI when they differ.

## Changing it

- Tests: `cargo test -p adage-cli`. Most are end to end through the binary with an isolated
  `ADAGE_HOME` and `HOME`, built with the kit in `tests/common/mod.rs` (`scrub`, `hermetic`,
  `adage`, `TempWorld`, `text`); a new suite starts with `mod common;` and calls `common::scrub`
  before setting its own variables. `tests/hooks.rs` cannot use it: `adage-exec`'s `parity` test
  compiles that file by path. See `../../docs/claude-mod/impl/structure-cli.md`.
- Routes, tracks, indexes and learned homes come from the workspace test kit,
  `adage_tracks::testkit` (the `testkit` feature, a dev-dependency here): `route`, `track`,
  `IndexBuilder`, `kinds::*`, `TempWorld`, `LearnedHome`. A suite builds its world from it
  rather than writing a `Route { … }` literal; `tests/hooks.rs` uses it directly. See
  `../../docs/claude-mod/impl/testkit.md`.
- A suite that waits on `adage watch --follow` sets `ADAGE_TEST_FOLLOW_POLL_MS=<1..=250>`
  (`ui/follow.rs`): a debug build polls that often instead of every 250 ms, so the test is not
  paced by the poll. A release build never reads it.
- Goldens: `tests/golden/*.txt`, re-blessed with `ADAGE_BLESS=1`; a changed Claude Code golden is
  a behaviour change, and `adage-exec`'s `parity` test runs the same goldens.
  Snapshots: `src/snapshots/`, `src/ui/snapshots/` (`cargo insta review`).
- Preconditions: `/fixture/acme-billing` must not exist (`adage_sim::assume_no_checkout_at_cwd`);
  tests tagged `ADAGE_TEST_NO_SPAWN` are ignored in release builds; the OpenCode plugin session
  test skips without `node`.
- Fixtures read (`READS`): `evals/sandboxes/`, `evals/gen/`, `plugin/`; `sim/fixtures` through `adage-sim`.
- After changing `web/map`: in `web/map`, `pnpm install`, then `pnpm assets` (build + embed) and
  commit `assets/map/` and `src/map/assets_list.rs` together; `pnpm drift` checks. The page's
  brotli budget is 400 KB (`embed.mjs`). Node is never needed to build the binary.
- Changing `/api/v1/graph` fields means changing `adage_app::graph` and the web map fixtures together.
- Pitfall: `main.rs` strips `--plain` and handles `hook` before clap; keep the hook path free of
  clap, ratatui and anything that costs startup time.
