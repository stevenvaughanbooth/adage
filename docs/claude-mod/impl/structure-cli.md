# Structure: adage-cli, adage-store, adage-app

The maintainability plan's smaller items that fall in these three crates (S8–S11 and W9's test
part): one copy of each small CLI helper, the switch markers on `Paths`, one source for every
configuration default, one rule for a fix's command, and a test kit for the CLI's integration
suites. Behaviour is unchanged apart from the one disagreement this closes (the status card,
below); no golden or snapshot was re-blessed.

Pass 2 (MAINT-RUST's W5, W8, W9's remainder and the CLI's part of W11) made the doctor's fixes
typed, listed the press sites as an enum, gave the follower a test-only poll knob and made
`doctor` a directory module: see [Typed doctor fixes](#typed-doctor-fixes-w5) onwards.

## Shared helpers (`crates/adage-cli/src/helpers.rs`)

| Helper | Was | Used by |
| --- | --- | --- |
| `flag(args, name)` | 5 copies: `main.rs`, `exec.rs`, `ui/mod.rs` (a wrapper of `main.rs`'s), `update.rs`, `install.rs` | `main.rs` (`use helpers::flag`), `exec`, `ui`, `update`, `install`; `crate::helpers::flag` in `init`, `sense`, `graph`, `share`, `savings`, `review`, `share_pack` |
| `flags(args, name)` | `exec.rs` | `exec` |
| `ago(now_ms, ts_ms)`: `just now`, `4 min ago`, `2 h ago`, `3 days ago` | `status_card.rs` | the card |
| `ago_short(now_ms, ts_ms)`: `12s ago`, `4m ago`, `2h ago`, `3d ago` | `doctor.rs` (now `doctor/mod.rs`) | doctor (imported `as ago`) |
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

**A fix's command** (`crates/adage-cli/src/doctor.rs`; pass 2 replaced this rule with a typed
fix, [below](#typed-doctor-fixes-w5)): `doctor::fix_command(fix)` was the one
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
    (behaviour stays exactly as it was). Pass 2 added it ([below](#the-followers-poll-knob-w9)).
- `skill_prompt.rs`: `adage train` on the acme skill (with its sandbox replay capped at 60 s) ran
  once per test. It now runs once for every test alive at the time (`Compiled::shared`, a
  `Weak` behind a `Mutex`, so the directory goes when the last test ends), and each test works on
  a copy. The copy's `review.json` and `verified.json` are rewritten to name the copy's paths, and
  `World::new` asserts nothing else under `home` names the original. The assertions are
  unchanged.
- `press.rs::a_committed_pack_can_be_trusted_with_a_press` slept 1.1 s so the session opened after
  the commit's whole second. The commit is now dated a minute earlier (`GIT_AUTHOR_DATE`,
  `GIT_COMMITTER_DATE`); the on-disk check, which the date does not affect, is unchanged.

## Typed doctor fixes (W5)

`crates/adage-cli/src/doctor/mod.rs` (was `doctor.rs`; `doctor_latency.rs` is now
`doctor/latency.rs`, a plain submodule with no `#[path]`):

```rust
pub enum Fix {
    Run { command: String, how: String }, // Fix::run(cmd), Fix::run_in("run {} to recompile it", cmd)
    Manual(String),                       // Fix::manual(text)
}
pub struct Check { …, pub fix: Option<Fix> }
```

- Every `warn`/`fail` takes a `Fix`, chosen where the check is made (a comment says why at each
  site where the choice is not obvious). `Fix::run` is a whole, runnable `adage` command (a
  placeholder such as `<key>` makes it `Manual`); `how` is doctor's sentence with `{}` where the
  command goes, in backticks. A debug assertion holds both rules.
- Everything else derives from it: `Fix::prose()` (doctor's `fix` line, the TUI's fix lines,
  `--json`'s `fix`), `Fix::command()` / `Check::command()` (the status card's command, `adage ui`'s
  one-key model fetch), `Fix::action()` (`--json`'s `action`: `{"kind":"run","command":…}` or
  `{"kind":"manual"}`, the shape `plugin/claude-code/hooks/reads.ts` reads), `Fix::next()`
  (doctor's `next`: the command, else the prose). `fix_command`, the prose parser, is gone, and so
  is the third rule `render` had (the first backticked `adage …` of any fix).
- The lexicon pass (`crate::words::prose`) runs over `how` and the manual text, never the command.
- `--json` is unchanged in shape; every fix's prose is byte-identical to before.

Edge outputs that change, each because the old rules read a command out of a sentence:

| Check | Before | Now | Why it is more correct |
| --- | --- | --- | --- |
| `hooks: <h>` whose registration runs a binary that is gone (`re-run `adage install <h> --write` with this binary (or --bin <path>)`) | `--json` `action: manual`; card `→ adage doctor` | `action: run, adage install <h> --write`; card `→ adage install <h> --write` | run from this binary, the command rewrites the registration to it: it is the fix (the old rule missed it only because the sentence starts `re-run`) |
| doctor's `next` for a manual fix with a backticked command in it | the first backticked `adage …` (`adage config set <key> <value>`, `adage review` for inert pack reflexes, `adage doctor --json` for slow hooks, `adage condition` for invalid tracks or a failed automatic run, `adage on` while a variable also keeps Adage off, one copy's `adage install … --remove …` for hooks registered twice, `adage models fetch` for a damaged binary) | the fix as written | none of those commands is the whole fix (a placeholder, a listing, the report itself, a step after the person's own, a choice of which copy to keep); `--json` and the card already called them manual |

No golden or snapshot changed: none pins those lines. The tests: `doctor::tests::a_fix_that_is_one_adage_command_is_an_action` (T-A2-03, now over typed fixes: prose, action, `next` and the JSON from one value, and the lexicon pass leaving the command alone), `every_fix_names_its_command_once` (a broken home's fixes: each command whole, named once in its prose), `status_card::tests::a_problem_is_one_line_and_its_command` (the card names `--json`'s action), `ui::tests` (the health fixture, typed).

### How to add a doctor check

1. Write `fn my_check(…) -> Check` in `doctor/mod.rs` (or a submodule like `latency.rs`) returning
   `pass(id, detail)`, or `warn`/`fail(id, detail, fix)` with the fix typed:
   `Fix::run("adage x")`, `Fix::run_in("run {} to …", "adage x")`, or `Fix::manual("…")`. Choose
   `Run` only when running that exact command, from this binary, fixes the check.
2. Push it in `checks()` in display order. If its id is not what people read, add it to `label`.
3. The card: add the id's one plain line to `status_card::brief` (else it says `<label> needs a
   look`); its command comes from the fix. A one-key fix in `adage ui`: `ui/health.rs::fix_for`,
   matching on `c.command()`, never on the prose.
4. Test it beside the check, asserting the `Fix` value (not a substring of prose).

## Press sites: `PersonVerb` (W8)

`crates/adage-app/src/person.rs`:

```rust
pub enum PersonVerb { Review(Verb), ReviewDelegate, ReviewRevoke, DelegateOn, DelegatePolicyE2, DelegatePredictOn, On, AddTrust }
impl PersonVerb { pub const ALL: [PersonVerb; 13]; pub fn cmd(self) -> String; pub fn what_it_changes(self) -> &'static str }
pub fn require_or_press(verb: PersonVerb, press: Option<&Press>, paths: &Paths) -> Result<Person>
```

`crate::review::person_or_press(verb, paths)` and `person_from(verb, paths, stdin)` in adage-cli
take the same enum: `review.rs` (`decide`, `decide_quietly`: `Review(verb)`, the verb now parsed
before the person check), `exec.rs` (`review delegate|revoke`, `delegate on`, `policy --max-level
e2`, `predict on`), `switch.rs` (`on`), `add.rs` (`add --trust`). A press can reach no other
command: there is no string to pass. `cmd()` gives the exact strings the free `cmd: &str` used to
be, so the refusals and the `press_spent` record's `verb` are byte-identical. `require(cmd: &str)`
stays for every other person-only command, which never reads a press.

The list is exactly the plan's (A7; `A7-A8-notes.md`, "Every press site"):
`person::tests::the_press_sites_are_the_listed_commands` pins `ALL`'s commands in order, checks
every review `Verb` is in it, and each site's refusal reason. The press suite
(`tests/press.rs`: `a_press_is_a_person_at_the_listed_sites`, `a_press_is_refused_everywhere_else`)
and `refusals_say_what_each_command_changes` are unchanged and green.

### How to add a press site

1. A variant of `PersonVerb`, with its doc line, and its entry in `ALL`.
2. Its arm in `PersonVerb::cmd` (the command as typed after `adage`). If its reason is new, an arm
   in `what_it_changes(cmd)`, which `require` shares.
3. At the verb, `crate::review::person_or_press(PersonVerb::X, paths)?` (or `person_from` when the
   verb has read stdin itself).
4. The design first: mod architecture §3.12's list, `A7-A8-notes.md`'s table, the expected list in
   `the_press_sites_are_the_listed_commands`, the TS `press.ts` allow-list, and the press suite's
   listed sites all change together.

## The follower's poll knob (W9)

`ui/follow.rs` polls every `POLL` = 250 ms and probes the learner every `LEARNER_POLLS` = 2 polls.
In a debug build, `ADAGE_TEST_FOLLOW_POLL_MS=<1..=250>` replaces the interval (`poll_every`); a
release build never reads it, and a value outside the range is ignored. `tests/mod_surfaces.rs`'s
`Feed::start` sets it to 20 (a one-line edit in the kit agent's file). Health still refreshes every
`HEALTH_EVERY` (the TUI's loader, shared). No test sleeps above 50 ms outside the `#[ignore]`d L5
run. `mod_surfaces` ran in 4.4 s here (the earlier note's harness time was 7.5 s).

## Where a new switch goes

Per-machine settings are configuration keys (`adage_store::config::KEYS`, `adage config set`):
`runtime.rewrite`, `runtime.predict_cap`, `ui.*`. Per-repository switches are marker files under
the repository (`Paths::repo_disabled`). Machine-wide switches an older verb wrote stay markers
on `Paths` (`delegate_marker`, `predict_off`, the legacy `rewrite_off` the hook still reads); a new
one is a configuration key.

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

## Merging main (`350ab0b`, #32)

Main's #32 ("setup, review and trace exploration from the app") changed `doctor.rs`, `review.rs`,
the TUI and the map server. On this branch:

- **Doctor.** Main's `tracks` wording ("reflex collection") is kept, with this branch's typed
  `Fix::manual` (recompiling alone does not mend a collection). Main's new
  `hook_delivery_checks` (`hook activity: <harness>`, one per registered harness, from
  `adage_hook::receipt`) passes when a delivery was seen and warns otherwise; both of its fixes are
  `Fix::manual` (start a session and send a prompt; for Codex, accept the hooks in `/hooks`): no
  `adage` command makes a hook fire, so `--json`'s `action` is `manual` and the card names
  `adage doctor`. `status_card::brief` gains the check's plain line ("no <harness> hook has
  reached Adage in this repository yet"), per step 3 of "How to add a doctor check". The Health
  tab's new viewer (Enter on a check with no one-key fix) shows `Fix::prose()`.
- **Review.** Main's `decide_reviewed_within` (refuse unless the route and its proposal still hash
  to the revision the person was shown) replaces `decide_within` in `review::decide_quietly`,
  `adage ui`'s decision. That is the same site, not a new one: it still goes through
  `PersonVerb::Review(verb)` with no press (`adage ui` owns the terminal), and
  `decide_reviewed_within` records the person's `via` like `decide` and `decide_within`.
  `PersonVerb::ALL` and the press suite are unchanged. Main's other person-only operations (the
  palette's `config set history`, `add --expected-digest` run as a child) call `require`, which
  never reads a press.
- **The map and the graph.** `/api/v1/graph` now reads the spools through
  `adage_app::graph::measurements` (attribution by installed reflex and version). This branch's
  additions moved with it: the mod's fields (`predicted`, `would_predict`, `substituted`,
  `would_substitute`, `noticed`, `answered`) are on every recorded event and keep an event in the
  recent window, and a route's `arms.<arm>.substituted` counts sessions where its dead end was
  replaced (a `substituted` judgement) or would have been (`would_substitute`, matched to the
  session's one exposure of that route, as a judgement is). `graph::event`, the live stream's row,
  carries both main's `directives`/`would_have` and the mod's fields. The web map reads only
  `/api/v1/graph`, `/api/v1/trace`, `/api/v1/health` and `/api/v1/events`; it calls no CLI
  surface (`adage --json`, `adage doctor --json`, `adage review`, the card), and the health
  check's `fix` stays the prose string its `HealthCheck` type declares (`action` is extra).
- **Tests.** `tests/review.rs`'s `session` writes main's exposure record before the judgements
  through the kit (`LearnedHome::seed`/`spool`). The palette snapshot was re-accepted: main's
  layout with this branch's two settings (`runtime.routine_model`, `runtime.predict_cap`), 33
  matches.

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
