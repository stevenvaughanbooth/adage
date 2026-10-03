# Structure: adage-cli, adage-store, adage-app

The maintainability plan's smaller items that fall in these three crates (S8–S11 and W9's test
part): one copy of each small CLI helper, the switch markers on `Paths`, one source for every
configuration default, one rule for a fix's command, and a test kit for the CLI's integration
suites. Behaviour is unchanged apart from the one disagreement this closes (the status card,
below); no golden or snapshot was re-blessed.

## Shared helpers (`crates/adage-cli/src/helpers.rs`)

| Helper | Was | Used by |
| --- | --- | --- |
| `flag(args, name)` | 5 copies: `main.rs`, `exec.rs`, `ui/mod.rs` (a wrapper of `main.rs`'s), `update.rs`, `install.rs` | `main.rs` (`use helpers::flag`), `exec`, `ui`, `update`, `install`; `crate::helpers::flag` in `init`, `sense`, `graph`, `share`, `savings`, `review`, `share_pack` |
| `flags(args, name)` | `exec.rs` | `exec` |
| `ago(now_ms, ts_ms)`: `just now`, `4 min ago`, `2 h ago`, `3 days ago` | `status_card.rs` | the card |
| `ago_short(now_ms, ts_ms)`: `12s ago`, `4m ago`, `2h ago`, `3d ago` | `doctor.rs` | doctor (imported `as ago`) |
| `plural(n, one, many)` | `status_card.rs`, `autolearn.rs` (`notice`) | both |

The two `ago` forms differ by design (a card sentence, a terse check line) and are kept. The third
`ago` the plan counts is `adage_exec::time::ago`, in seconds, in a crate this work does not own:
it stays. `ui/follow.rs`'s private `now_ms` and doctor's inline `SystemTime` arithmetic are now
`adage_core::time::now_ms` (the same expression).

## Single sources

**Switch markers** (`crates/adage-store/src/paths.rs`):

| Marker | Definition | Callers moved to it |
| --- | --- | --- |
| `<root>/.adage/disabled` | `Paths::repo_disabled(root)`; `paths::REPO_DISABLED` is the relative text | `switch::MARKER`/`switch::marker` (now aliases), `doctor::kill_switches`, `ui/routes.rs` |
| `exec/enabled` | `paths.delegate_marker()` | `exec.rs` (its `marker()` is gone), `ui/routes.rs` |
| `exec/rewrite-off` | `paths.rewrite_off()` | `exec.rs`, `doctor::rewrites_check`, `status_card`, `ui/app.rs`, `map/http.rs`, `adage_app::graph` |
| `exec/predict-off` | `paths.predict_off()` | `exec.rs`, `ui/routes.rs` |

`adage_hook::rewrite_off` and `adage_exec::hook::predict_off` (and the hook's own `.adage/disabled`
literal) belong to other owners and keep their public paths; the existing tests that write the
marker through those functions and read it through `Paths` (`doctor::rewrites_say_their_state…`,
`ui::routes::…predict…`) now also assert the two paths are equal, so a drift fails there. Their
owners can switch to `Paths` with no change in behaviour.
`paths::tests::switch_markers_keep_their_paths` pins the strings.

**Configuration defaults** (`crates/adage-store/src/config.rs`): each default is written once, in
`KEYS`. `load_from` builds `Config` through `from_values`, whose `bool_of`/`number_of` fall back to
`KEYS[name].default` (never reached: every value is validated first), so the eleven
`unwrap_or(<literal>)` copies are gone. `impl Default for Config` is the same function over the
defaults. `every_default_is_valid_and_is_the_default_config` checks that each default passes its
key's validation and that `Config::default()` is what a missing file gives. Not done:
`adage_runtime::PREDICT_CAP = 6` (adage-runtime may not link adage-store, `FORBIDDEN` in
`xtask/src/layers.rs`; its owner can pass the configured value in).

**A fix's command** (`crates/adage-cli/src/doctor.rs`): `doctor::fix_command(fix)` is the one
rule (the whole fix when it is one bare `adage` command; else a backticked `adage` command it
begins with, after an optional `run `; never when it sends the person to a terminal).
`Check::action` (`adage doctor --json`) and the card's `brief` both call it. The card used to read
fixes with its own `single_command`, which did not strip `run `: for an index or embedding-model
problem the card said `next adage doctor` while `--json` offered `adage condition` /
`adage models fetch`. The card now names the same command (CHANGELOG, "Seeing it").
`status_card::tests::a_problem_is_one_line_and_its_command` feeds every case to both and asserts
they agree; it gained the `run `…`` and "in a terminal" cases.

`adage doctor`'s kill-switch fix (A3-A6 notes, open issue) already names `adage on` on this
branch (`a_repository_turned_off_is_fixed_with_adage_on`); nothing was left to change.

## The CLI test kit (`crates/adage-cli/tests/common/mod.rs`)

- `scrub(&mut Command)`: removes the agent markers (`adage_app::person::AGENT_ENV`), every
  `ADAGE_*` variable the test process inherited, and `SCRUB` (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`,
  `SSH_AUTH_SOCK`, `VIRTUAL_ENV`, `CONDA_PREFIX`). Call it before setting anything: a suite's own
  `.env(…)` calls come after. The list only grows; it can hide a dependence on the machine, never
  a regression.
- `hermetic(bin, adage_home, cwd)`: `scrub`, then `ADAGE_HOME`, `ADAGE_AUTO_LEARN=0` and the
  working directory. `adage(adage_home, cwd)` is the same for this crate's binary.
- `TempWorld::new(&["home", "data", "work/.git"])`: a canonicalised temporary directory with those
  subdirectories; `p(rel)`.
- `text(&Output)`: stdout then stderr.

A new suite:

```rust
mod common;
use common::text;

struct World {
    w: common::TempWorld,
}

impl World {
    fn new() -> World {
        World { w: common::TempWorld::new(&["home", "data", "work/.git"]) }
    }
    fn cmd(&self, args: &[&str]) -> std::process::Command {
        let mut c = common::adage(&self.w.p("data"), &self.w.p("work"));
        c.args(args).env("HOME", self.w.p("home")).env("ADAGE_ARM", "treated");
        c
    }
}
```

Migrated: `mod_surfaces.rs`, `press.rs` and `skill_prompt.rs` use `TempWorld`/`hermetic`/`adage`
(their three drifted scrub lists are now one; `skill_prompt` also stops inheriting
`ADAGE_ALLOW_NONINTERACTIVE`). The older suites' builders call `common::scrub` in place of their
own `env_remove` lists (variables outside the kit's list, such as `XDG_DATA_HOME`, stay; a removal
that undoes an earlier `.env` in the same builder, `review.rs`'s `ADAGE_ASCII`, stays):
`actions`, `candidates`, `exec`, `first_run`, `fresh_install`, `g4_hook`, `gaps`, `graph`,
`harness_sim`, `hook_intent`, `instincts`, `live_learn`, `packs`, `repo_keys`, `review`, `sense`,
`serve`, `share`, `sources`, `state_rebuild`, `train_accept`, `watch`, `zero_config_plugin`.
Not migrated: `hooks.rs`, which `crates/adage-exec/tests/parity.rs` compiles by path, so a `mod
common;` in it would not resolve there. Each suite keeps its own `World` (W10 is the workspace
builder for routes and indexes). Every test keeps its name.

## Slow tests made deterministic

- `mod_surfaces.rs`:
  - `the_feed_ends_with_its_parent` slept 1.5 s before checking the follower was alive. It now
    waits for the follower's first line, writes a spool record, waits for that record's row, then
    checks: the follower has been round its loop (which asks first whether it should end) at
    least once after starting, which is what the sleep stood for.
  - `a_follower_told_to_ignore_stdin…` slept 0.5 s after closing stdin. It now writes a record
    after the close, waits for its row, then checks the follower is still running.
  - `the_feed_streams` measured `t0.elapsed() < 2 s` around a 5 s wait; the 2 s (the test plan's
    bound) is now the wait itself, as the learn line's already was.
  - Not changed: the remaining waits are bounds that return as soon as the line arrives. The
    suite's wall time is set by `the_card_as_json` (about 6 s of status-card runs, each running
    doctor's checks) and `the_feed_says_where…` (health refreshes every `HEALTH_EVERY` = 3 s); the
    plan's follow-poll knob would be a new test-only variable in `follow.rs` and was left out
    (behaviour stays exactly as it was).
- `skill_prompt.rs`: `adage train` on the acme skill (with its sandbox replay capped at 60 s) ran
  once per test. It now runs once for every test alive at the time (`Compiled::shared`, a
  `Weak` behind a `Mutex`, so the directory goes when the last test ends), and each test works on
  a copy. The copy's `review.json` and `verified.json` are rewritten to name the copy's paths, and
  `World::new` asserts nothing else under `home` names the original. The assertions are
  unchanged.
- `press.rs::a_committed_pack_can_be_trusted_with_a_press` slept 1.1 s so the session opened after
  the commit's whole second. The commit is now dated a minute earlier (`GIT_AUTHOR_DATE`,
  `GIT_COMMITTER_DATE`); the on-disk check, which the date does not affect, is unchanged.

## Carried from main

`origin/main` (after fff4fa7) changed `crates/adage-cli/src/exec.rs` and
`crates/adage-store/src/config.rs`:

- `config.rs`: the `learn_parse_cache` doc now names `adage_ingest::cache`. Carried verbatim.
- `exec.rs` `review()`: main's `CommandTable::load()` returns `&'static` (adage-core, 6583b66, not
  in this tree). Here `let table = &CommandTable::load();`, and the `route::plan`, `AutoContext {
  table, … }` and `preview(…, table)` lines are now byte-identical to main's, so they merge
  cleanly. The `predict_classified(&req, table)` line still conflicts (this branch calls
  `route::predict_classified`, main `adage_exec::classify::effect(&req.argv, table)`); the merge
  keeps this branch's call with main's argument. With main's `&'static` table, the `let` reads
  `&&CommandTable` and coerces where it is passed; `&` can be dropped after the merge.
  `lapsed_here`'s `plan_grants(…, &CommandTable::load())` is left as it was: main's form does not
  compile against this tree's adage-core, and this branch does not touch the line, so main's
  version merges in.

Main's new `crates/adage-store/AGENT.md` lists `Paths`' methods; after the merge it should add
`repo_disabled`, `delegate_marker`, `rewrite_off`, `predict_off` and `REPO_DISABLED`.
`crates/adage-app/AGENT.md`'s "the rewrites-off marker (`adage_hook::rewrite_off`, in `graph`)"
becomes `Paths::rewrite_off`. `crates/adage-cli/AGENT.md` could name `helpers.rs` and
`tests/common/mod.rs`.

## Commands and results

| Command | Result |
| --- | --- |
| `cargo test -p adage-cli -p adage-store -p adage-app --no-fail-fast` | all green (adage-cli: 122 unit tests and every integration suite, including `hooks.rs`'s goldens unchanged; adage-store 22; adage-app 31) |
| `cargo test -p adage-cli --test fresh_install --test zero_config_plugin` (after the clippy fix below) | green |
| `cargo test -p adage-cli -- --list`, before and after, with each binary's header | identical (`diff` empty) |
| `cargo clippy -p adage-cli -p adage-store -p adage-app --all-targets -- -D warnings` | clean (it first caught two one-element `for` loops the migration left; now plain `env_remove`) |
| `cargo fmt -p adage-cli -p adage-store -p adage-app -- --check` | clean |
| `cargo test -p xtask` | 18 passed |
| `cargo test -p adage-cli --test mod_surfaces --test skill_prompt`, wall time | before 9.5 s (harness: 7.5 s + 1.8 s); after 9.7 s (7.5 s + 1.9 s), twice; the machine's load average was about 6 during the after runs |
| per test, prebuilt binary, after | `the_feed_ends_with_its_parent` 1.52 s → 0.33 s; `a_follower_told_to_ignore_stdin…` 0.58 s → 0.34 s; `press::a_committed_pack_can_be_trusted_with_a_press` 0.42 s (it slept 1.1 s) |

The suite's wall time does not move: `the_card_as_json` (about 6.4 s) sets it, and on this
machine `skill_prompt` has no sandbox (`adage_exec::sandbox::detect()` is unavailable), so its
replay is skipped and sharing it saves nothing here; where a sandbox replays the steps, the suite
runs that replay once instead of twice.
