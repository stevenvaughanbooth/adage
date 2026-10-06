# The workspace test kit (W10)

One place to build the routes, tracks, indexes and learned homes that tests across the workspace
used to write out by hand (MAINT-RUST §3.2: 19 `World` structs in adage-cli's suites, a `Route { … }`
literal in nearly every one, the hook's `ObligationWorld`, adage-exec's `Fixture`, and
`hook_latency`'s scenario builders). It lives in `adage-tracks`, behind a `testkit` feature, at
`crates/adage-tracks/src/testkit/`. Wave A built it and moved adage-cli's tests and
`examples/hook_latency.rs` onto it; wave B moved adage-hook's `testkit.rs`, adage-exec's
`hook/testkit.rs` and adage-cli's `tests/hooks.rs` (below, "Wave B"). No fixture in the workspace
writes a `Route`, `Track` or `Index` literal of its own any more.

## What it builds

| Item | Builds | Defaults (what most fixtures wrote) |
| --- | --- | --- |
| `route(id, trigger, step)` → `RouteBuilder` | a `Route` | summary empty, no slots, `Effect::Verify`, `Strength::Fire`, `RouteState::Firm`, no exit, `Evidence::default()`, not authored |
| `RouteBuilder` methods | | `id`, `summary`, `trigger`, `step`, `slot` (add), `slots` (replace), `effect`, `strength`, `state`, `exit`, `until_intent_changes` (`[Exit::IntentChanges]`), `evidence` (replace), `support(s, o)` (counts only), `measured(s, o)` (counts, rate `s/o`, `wilson_lower(s, o)`), `days`, `provenance`, `origin`, `authored`, `with(|r| …)` for anything else, `build()`; `Into<Route>` |
| `track(name, repo)` → `TrackBuilder` | a `Track` | version `0.1.0`, description empty, provenance `desire-path`, default gauge, no routes |
| `TrackBuilder` methods | | `version`, `description`, `provenance`, `route(impl Into<Route>)`, `routes(iter)`, `build()`, `validated()` (panics with the reason when `io::validate` refuses it: a fixture cannot test what no installed track could be) |
| `IndexBuilder::new(generation)` | a hand-written `Index` (not compiled: routes in the order given, the generation it is told, triggers and context nets derived on load) | `route(repo, track, route)` at `0.1.0`, `versioned(repo, track, version, route)`, `exec(binding)` on the last route, `indexed(IndexedRoute)`, `build()`, `write(path)` (atomic, returns the index); `IndexBuilder::from(index)` goes on from an index already built (its routes, in order, and its generation) |
| `kinds::lookup(id, command)` | a prompt naming a work item runs `command`; every `{slot}` in it is a `ticket_slot` | `Effect::Observe` |
| `kinds::finish_line(id, paths, command)` | after an edit matching `paths`, run `command` | `Effect::Regenerate` (an obligation) |
| `kinds::dead_end(id, avoid, instead, signature)` | from the session's start, `Step::Avoid` | `Strength::Inhibit` |
| `kinds::context(id, items, within, command)`, `kinds::act(key)`, `kinds::edit(glob)` | a context route and its items | |
| `kinds::sequence(id, paths, commands)` | after an edit, commands in order | `Effect::Regenerate` |
| `kinds::intent(id, model, centroid, threshold, command)` | an intent route whose family is its id | the centroid must be a unit vector in units of 1e-4 (validation) |
| `kinds::ticket_slot(name)`, `kinds::WORK_ITEM` | the `^W-\d{4}$` slot bound from a ticket span | |
| `kinds::every_kind()` | one of each kind above, valid together in one track (ids and globs do not collide) | |
| `TempWorld::new(&dirs)` | a canonicalised temporary directory with those subdirectories, removed on drop | `root` (also `path()`, the name `tempfile::TempDir` gives it, so a fixture that held a `TempDir` keeps its callers), `p(rel)`, `repo(rel)` (a directory with `.git`), `write(rel, body)` (parents created), `executable(rel, body)` (mode 755 on Unix: a fake tool on a private `PATH`), `home(rel)` |
| `LearnedHome::at(root)` | an `ADAGE_HOME`, created; every path through `adage_store::Paths` | `paths`, `root()`, `without_auto_learn()` (`[learn] auto = false`), `write_index(&Index)`, `compile(&[Track])` (`Index::compile`), `compile_root(&[Track])` (`Index::compile_root`, as the learner: candidates and conflicts), `install(reference, &Track)` (`io::write` under `tracks/`), `recompile()` (every track under `tracks/`), `loaded()` (`Index::load`) |
| the session-state seeder | `LearnedHome::seed::<T>(session, |s| …)`, `state::<T>(session)` | `T` is the runtime's `SessionState`, which layer 2 cannot name; the state file is written through `adage_store::state::update`, as the hook writes it |
| the spool reader | `LearnedHome::spool(session, &record)`, `spooled(session)` (none when there is no spool), `spooled_kind(session, kind)` | |

Every builder returns the plain model types, so a test that needs a field the kit does not name
sets it on the result (`t.routes[0].state = RouteState::Nascent`), and struct-update literals
(`Route { id: …, ..base.clone() }`) still work on what the kit built.

## A new test

```rust
use adage_tracks::testkit::{self, kinds, LearnedHome, TempWorld};
use adage_tracks::{Strength, Exit};

let w = TempWorld::new(&["home"]);
let repo = w.repo("acme");
let home = w.home("adage").without_auto_learn();
home.compile(&[testkit::track("acme/learned", "acme")
    .route(kinds::finish_line("gen", "schema/*.json", "make gen").until_intent_changes().support(5, 5))
    .route(kinds::dead_end("no-pytest", "pytest", "make test", "No module named pytest"))
    .validated()]);
home.seed::<adage_runtime::SessionState>("s", |st| st.arm = Some(adage_runtime::Arm::Treated));
// … run the hook in `repo` with ADAGE_HOME = home.root() …
assert_eq!(home.spooled_kind("s", "pretool").len(), 1);
```

A hand-written index, as the hook's unit tests use one:

```rust
let index = testkit::IndexBuilder::new("obligation-world")
    .route("acme", "acme/finish-lines", kinds::finish_line("regenerate-registry", "registry/*.csv", "make gen").support(5, 5).days(3))
    .write(&home.paths.index());
```

In adage-cli's suites, `tests/common/mod.rs` re-exports `TempWorld` and `LearnedHome` (`common::TempWorld`),
beside its scrub and hermetic `Command`; routes, tracks and indexes come from
`adage_tracks::testkit` directly.

## Enabling it in a crate

```toml
[dev-dependencies]
adage-tracks = { path = "../adage-tracks", features = ["testkit"] }
```

A crate that already links `adage-tracks` keeps its normal entry and adds this one: cargo merges
the two, and with resolver 2 the feature is on only when tests, examples or benches are built, never
in the binary a plain `cargo build` produces. adage-tracks' own unit tests compile the kit without
the feature (`#[cfg(any(test, feature = "testkit"))]`, with `adage-store` as a dev-dependency), so
`cargo test -p adage-tracks` runs the kit's tests (`src/testkit/tests.rs`) on its own.

The crates that enable it, each in `[dev-dependencies]`: **adage-cli** (its integration suites and
`examples/hook_latency.rs`), **adage-hook** (`src/testkit.rs`, its unit tests' worlds) and
**adage-exec** (`src/hook/testkit.rs`, and `tests/parity.rs`, which compiles adage-cli's
`tests/hooks.rs` by path). adage-tracks compiles it for its own tests without the feature.

**Layering.** The feature adds `adage-store` (layer 0) and `tempfile` as optional dependencies of
`adage-tracks` (layer 2). `xtask`'s check reads `cargo metadata`, which lists an optional
dependency as a normal one, so the edge `adage-tracks -> adage-store` is checked like any other:
layer 0 is below layer 2, and no `FORBIDDEN` rule names it (`adage-runtime -> adage-store` is
forbidden, but a dev-dependency is exempt, and a crate's tests reaching the store through the kit
is not the runtime linking it). `cargo test -p xtask` passes with it. `adage-ingest` must never
link `adage-tracks` at all, so it cannot use the kit. The kit never spawns a process, so
`adage-exec`'s `no_spawn` scan of `adage-tracks/src` is unaffected.

## What moved onto it (wave A)

- `crates/adage-cli/tests/common/mod.rs`: its `TempWorld` is now the kit's (re-exported; the kit's
  adds `repo`, `write`, `executable`, `home`), and it re-exports `LearnedHome`.
- Every remaining `World` (and `harness_sim`'s `Sandbox`) builds its directories with `TempWorld`:
  `exec`, `first_run`, `fresh_install`, `gaps`, `graph`, `harness_sim`, `hook_intent`, `instincts`,
  `live_learn`, `packs`, `review`, `sense`, `share`, `state_rebuild`, `zero_config_plugin` (and the
  three the CLI's kit had already moved: `mod_surfaces`, `press`, `skill_prompt`).
- Every hand-written `Route`/`Track` in a fixture builder is a kit builder: `actions`
  (`finish_line_track`), `exec`, `fresh_install` (`route`, `track`), `g4_hook` (`context_track`),
  `gaps` (`route`, `track`), `graph` (`track`), `hook_intent`, `press` (`track`, and the two finish
  lines of `a_committed_pack…`), `review` (`route`, `make_track`), `share` (`track`),
  `state_rebuild`. Routes a single test changes from a base with struct-update syntax stay in the test.
- Index writes and reads go through `LearnedHome` (`compile`, `compile_root`, `install`,
  `recompile`, `loaded`); session-state seeds (`review::World::session`) through `seed`; spool reads
  and appends (`actions::exec_records`, `press::World::spool`, `skill_prompt::World::spooled`,
  `mod_surfaces::World::spool`) through `spooled`/`spool`.
- `examples/hook_latency.rs`: `route`, `lookup`, `with_contexts`, `intent` and `index` are the kit's
  kinds and `LearnedHome::compile`; the scratch directory is a `TempWorld`; `turn_step_predicts`
  seeds its pending directive with `LearnedHome::seed`; `batch_notices` reads the probe's spool with
  `spooled`. The candidates (`with_candidates`) are not routes and stay as they were.

Every test keeps its name and its assertions; `cargo test -p adage-cli -- --list` is unchanged.
The values built are the same: where a suite's evidence differs from what the kit derives, it says
so in place (`fresh_install`'s bound of −0.0004 and `press`'s 0.6 are set with `with`, not the
Wilson bound of the counts).

## Wave B: the remaining worlds (done)

- **adage-hook `src/testkit.rs`, `ObligationWorld`**: a `TempWorld` with `acme/registry`, the
  repository `w.repo("acme")`, and `home: LearnedHome` (`w.home("home").without_auto_learn()`;
  the world keeps `paths` as `home.paths.clone()`, the name its tests use). The index is
  `IndexBuilder::new("obligation-world").route("acme", "acme/finish-lines",
  kinds::finish_line("regenerate-registry", "registry/*.csv", "make gen").support(5, 5).days(3))`
  written to `home.paths.index()`; the arm is `home.seed::<SessionState>("s", …)` (a private
  `arm(arm)` the derived worlds share); `spooled`, `pretool_records` and `state()` are
  `home.spooled("s")`, `home.spooled_kind("s", "pretool")` and `home.state::<SessionState>("s")`.
  `dead_end_world` is `kinds::dead_end(…).support(5, 5).days(3)` (effect `Verify`, the kit's
  default), `u11_world` `testkit::route(…).strength(Strength::Prime).support(1, 1).origin(…)`,
  `receipt_world` `kinds::finish_line(…).effect(Effect::Verify).support(5, 5).days(3)`.
- **adage-exec `src/hook/testkit.rs`, `Fixture`**: `_tmp` is a `TempWorld` (`act.rs` reads
  `f._tmp.path()`, hence the kit's `path()`), the repository `tmp.repo("fixture")` with
  `tools/check.sh` and `src/feature.rs` written through `tmp.write`, and `home: LearnedHome`
  (`tmp.home("adage")`, `paths` its clone). The index is `IndexBuilder::new("fixture").route(
  "fixture", "fixture/context", kinds::context("predict-check", vec![kinds::act("exec git
  status"), kinds::edit("src/*.rs")], 3, "sh tools/check.sh").summary(…).with(|r|
  r.evidence.execution = Some(…)))`, held in memory as before. `with_obligation` goes on from it
  with `IndexBuilder::from(index).route("fixture", "fixture/finish", kinds::finish_line(…)
  .support(5, 5).days(3)).write(…)` and turns learning off with `without_auto_learn`; `treated`
  and the predicted state are `home.seed`.
- **adage-cli `tests/hooks.rs`**: its three literals are the kit's. `route(…)` is
  `testkit::route(…).summary(…).slots(…).effect(…).strength(…).until_intent_changes()
  .support(7, 8).provenance("desire-path")`, `track()` is `testkit::track("acme/work",
  "acme").version("0.3.0").routes([…])`, and `bind_u11`'s index is `IndexBuilder::new("u11")`.
  `World` holds a `TempWorld` and compiles its index with `world.home("adage").compile(…)`;
  `pretool_records` is `spooled_kind`. It still compiles in adage-exec's parity build, which now
  enables the feature.

The values built are the same, with two differences no test can see: adage-exec's data directory
is now created with the fixture (a `LearnedHome` is; the hook created it on first write before),
and adage-hook's `spooled(w)` reads a missing spool as empty rather than panicking (as
adage-cli's suites already read one; no test reached the panic). Kit additions for wave B, each with a test in
`src/testkit/tests.rs`: `IndexBuilder::from(Index)` (`a_built_index_takes_more_routes_after_its_own`)
and `TempWorld::path()` (`a_temp_world_writes_repositories_files_and_tools`).

## Commands and results

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy -p adage-tracks -p adage-cli --all-targets --features adage-tracks/testkit -- -D warnings` | clean (a first run stopped on an unused `Check::action` in `adage-cli/src/doctor`, another agent's work in progress; clean on retry) |
| `cargo test -p adage-tracks -p adage-cli --no-fail-fast` | all green: adage-tracks 62 unit tests (9 of them the kit's) and its 4 integration suites; adage-cli 123 unit tests and all 27 integration suites; no golden or snapshot re-blessed (`git status` shows none changed) |
| `cargo test -p adage-exec --test parity` | 23 passed (`tests/hooks.rs`, unchanged) |
| `cargo test -p xtask` | 18 passed (layers accept `adage-tracks -> adage-store`) |
| `cargo test -p adage-cli -- --list`, before and after | every integration test identical; the one added line is `doctor::tests::every_fix_names_its_command_once`, a unit test another agent added in `src/` |
| `cargo build -p adage-cli --examples` | ok |
| `target/debug/examples/hook_latency target/debug/adage - 30` | exit 0; every scenario for this configuration printed (`no_routes_for_repo`, `structured_routes_only`, `structured_routes_ticket_prompt`, `intent_routes_model_missing`, `intent_routes_bundled_model` (fired), `intent_routes_no_model`, `batch_no_match`, `turn_step_nothing_pending`, `pretool_no_match`, `batch_context_fires` (fired), `batch_no_match_with_candidates`, `batch_notices` (noticed)); `intent_routes_model_installed*` need a model directory and `turn_step_predicts` an `adage-exec` binary, as before |

`Cargo.lock` changes with the manifest: `adage-tracks` gains `adage-store` and `tempfile` (both
already in the lock).

Wave B:

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy -p adage-hook -p adage-exec -p adage-tracks -p adage-cli --all-targets -- -D warnings` | clean (a first run stopped in `adage-cli/src/ui/feed.rs`, another agent's work in progress; clean on retry) |
| `cargo test -p adage-hook -p adage-exec -p adage-tracks --no-fail-fast` | all green: adage-hook 57 unit tests, adage-exec 111 unit tests and its integration suites, adage-tracks 63 unit tests (10 of them the kit's) |
| `cargo test -p adage-cli --test hooks` | 23 passed; `git diff --stat -- crates/adage-cli/tests/golden` empty |
| `cargo test -p adage-exec --test parity` | 23 passed (`tests/hooks.rs`, on the kit) |
| `cargo test -p xtask` | 18 passed |
| `cargo test -p adage-hook -- --list`, `cargo test -p adage-exec -- --list` | unchanged: no test was added, removed or renamed in either crate (the change is in their fixture modules only) |
