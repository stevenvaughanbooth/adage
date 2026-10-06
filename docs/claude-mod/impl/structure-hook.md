# The structure of adage-hook

`crates/adage-hook` is the hook pipeline that `adage hook` and `adage-exec hook` share. Until the
maintainability pass, almost all of it was one 3,391-line `src/lib.rs`. The pass split that file
by job and moved the idioms it repeated into helpers (MAINT-RUST items S1 and S2). Behaviour is
unchanged: the Claude Code, Codex and OpenCode goldens, `adage-exec`'s parity test and every unit
test pass with no golden re-blessed. The public API is the same set of items at the same paths
(the rustdoc item index is identical). Line references to `lib.rs` in older notes and in
`../implementation-plan.md` are to commit `83b36bc`, before the split.

## Module map

Each module names the entry point `handle` (or another module) calls.

| Module | What it owns | Entry point |
| --- | --- | --- |
| `lib.rs` | Reading stdin, the kill switches, panics and fail-open (`main`, `run`), and the dispatch of one event to its module (`handle`: one exhaustive `match` over `harness::HookEvent`); the re-exports | `main`, `run`, `handle` |
| `ctx.rs` | `HookCtx`: one event (adapter, harness and event names, payload, session, `cwd`, `root`, `paths`, `started`, `Options`) and its shared reads, each made once on first use: the configuration, the index (and whether it was damaged), the repository key, the arm inputs | `HookCtx` |
| `answer.rs` (public) | Every answer the binary sends the mod: `ModAnswer` and the one function that prints it, `answer` | `answer::answer` |
| `extension.rs` | The extension contract adage-exec implements: `Extension`, `Site` (where it is asked: repository key, checkout, index, data directory), `Turn`, `Stimulus` and `stimuli`, `Delivery`, `PreCall`, `PreNote`, `ReceiptCard`, `SessionTexts`, `StopCall`, `StopGate`; all re-exported at the root | `Extension`, `guarded` |
| `pretool/mod.rs` | `PreToolUse` from a command hook: the word scan, one decision per call, the receipt note, the arm claim, the `pretool` record | `pretool::pretool` |
| `pretool/interpreter.rs` | The U11 interpreter plan: the bound environment, a worktree's own environment, the person's Bash allow rules, the `exec/rewrite-off` marker | `interpreter_plan`; `rewrite_off` (public) |
| `pretool/phases.rs` | The mod's two phases of a Bash call: the dry run's plan, the verdict (rewrite, advice, substitution, a call answered by a receipt) | `pretool_mod` |
| `phase.rs` | The mod's `PreToolUse` payload fields (`dry_run`, `verdict`) and `identity_hash` (public, unchanged) | `Phase::of` |
| `identity.rs` | The keys of the session's two identity lists: decided calls and calls answered by a receipt (see below) | `decided_key`, `answered_key` |
| `standing.rs` | The mod's `Standing`: the standing text, the environment sentence, what a subagent is told | `standing::standing` |
| `session_start.rs` | A session start (its record, the bootstrap or rebind learner, the session texts, the person's line) and the `seen` record of a session in a repository with no reflexes | `session_start`, `record_seen` |
| `respond.rs` | Decide and deliver every other event: the no-routes path, the fast path, `decide()` under the state lock, a candidates-only repository's noticing, `TurnStep` prediction and routing, the extension's delivery, the record | `respond` |
| `stop.rs` | The finish-line gate at a `Stop`: its deadline, its `stop_gate` record, joining its result to the decision's | `gated` |
| `record.rs` | The spool record seam: `RecordKind` (every kind a hook writes) and the `Record` builder, which stamps `ts`, `event`, `kind`, `harness`, `tool_use_id(s)` and `duration_us` and appends (failure returned or logged); `record/snapshot_tests.rs` pins every kind's bytes | `Record`, `RecordKind` |
| `site.rs` | What every path asks before deciding: the repository key the index routes this checkout by, the session's arm, observe mode | `routed_key`, `ArmInputs`, `observed` |
| `press.rs` | The press token; what it does at each invocation is decided from the event table (`act_of`) | `press::gate`, `press::before_kill_switch` |
| `skill.rs` | `SkillPrompt`, and the skills a `TurnStep` may invoke | `skill::prompt`, `skill::invocable` |
| `harness/` | The adapters: payload in, `HookEvent` out (`Adapter::classify`, from the event table and `translate`), rendering back; `dispatch_tests.rs` is the dispatch's characterization | `harness::adapter` |
| `receipt.rs` (public) | The delivery receipt (from main's #32): the first and the latest time a harness's hooks reached Adage in a repository, rewritten at most once a minute, in `$ADAGE_HOME/hook-receipts/`; `adage doctor`'s `hook activity: <harness>` reads it. Not the mod's test-run receipt (`ReceiptCard`, `identity::answered_key`) | `receipt::record` (from `handle`), `receipt::read` |
| `autolearn.rs`, `failopen.rs` | Live learning at a turn's end; the fail-open log | unchanged |
| `testkit.rs` | Test-only: `ObligationWorld` and the other worlds, the stand-in extensions, `bash_call`, `spooled` | `#[cfg(test)]` |

The TurnStep, candidates and noticed logic is in `adage-runtime` (`decide`, `predict_step`,
`notice`). The hook only gates it, in `respond`, so there is no hook module for it.

## Helpers

- `HookCtx` (`ctx.rs`): every path but `SkillPrompt` (whose entry, `skill::prompt`, is public)
  takes the event's context. `ctx.config()`, `ctx.index()` / `ctx.index_damaged()`,
  `ctx.repo_key()` (`repo::repo_key_cached`) and `ctx.arms()` (`site::ArmInputs` from the
  configuration) each read once, on first use; nothing is read before `pretool`'s word scan or
  `respond`'s fast path that was not read there before. `pretool::BashCall` derefs to it.
- `record::Record::new(kind, event)` (or `Record::at(kind, event, ts)`), `.with(key, value)` for
  the kind's own fields, `.harness(adapter)`, `.tool_use_id(p)` or `.calls(&ev)`, then
  `.timed(started)` and `.append(paths, session)` (a decision's record: the failure is returned)
  or `.append_or_log(paths, session, harness, event)` (logged to `fail-open.log`). The harness
  name logged is what each site logged before (the adapter's name, the CLI argument at the Stop
  gate, `claude` for the press).
- `answer::answer(&ModAnswer::…)`: every answer to the mod.
- `site::routed_key(index, root, paths, with_candidates)`: the key the index names this checkout
  by, or `None`. `respond` passes `with_candidates: true`.
- `site::ArmInputs` (through `ctx.arms()`; `ArmInputs::load(paths)` where there is no context),
  then `.ensure(st, session, key)` inside the state update: assigns an arm, propensity and
  repository once.
- `site::observed(p)`: the mod's observe mode.
- `extension::guarded(|| x.method(..))`: an extension call other than a delivery. A panic loses
  only that answer (`Default`). A delivery uses `respond::extension_delivery`, which turns a panic
  into the recovery warning.
- `identity::decided_key(adapter, p)` and `identity::answered_key(p)`: see below.

## The two identity lists

`SessionState::seen_pretool` (decided calls) and `SessionState::answered` (calls the mod answered
with a receipt) hold the same call under **different** hashes. `seen_pretool` holds the hash of
the adapter's event identity, for Claude Code the JSON triple
`["claude","PreToolUse",["<tool_use_id>"]]`. `answered` holds the hash of the bare `tool_use_id`,
because the `PostToolBatch` reader in `adage_runtime::decide` holds only that id. The keys are
left as they are, because running sessions' state files hold them.
`identity::tests::the_two_identity_lists_hold_a_call_by_different_keys` pins both.

The runtime's own docs of both fields said "by the same hash"; the runtime's split corrected them
(structure-runtime.md, "Documentation drift fixed").

## How to add a hook event

The native event names live in one place, the event table in adage-trace
(`adage_trace::Harness::events`, with `EventKind`). Everything that asks what an event name means
reads it: each adapter's `translate`, `Adapter::classify` (what `handle` matches on),
`Adapter::is_learn_point`, the press (`press::act_of`, `CLOSERS`), `claude::MOD_EVENTS` and
`is_mods`, and `adage_runtime::rebuild::kind_of`.

1. **The table.** Add the native name to the harness's row in `crates/adage-trace/src/harness.rs`
   (`CLAUDE_CODE_EVENTS`, `CODEX_EVENTS`, `OPENCODE_EVENTS`). A new kind is a new `EventKind`
   variant: say whether the runtime decides it (`EventKind::decided`, its record's `kind`) and
   whether only a mod sends it (`mod_only`). The compiler then lists every `match` that must
   decide it: `Adapter::classify` (which `HookEvent` it is), each adapter's `translate`, the
   press's `closes`, Claude Code's `event_identity`.
2. **The dispatch.** A decided event needs nothing more here: `classify` translates it and
   `handle` gives it to `respond`. An ask the hook answers itself is a new `HookEvent` variant,
   and `handle`'s `match` (exhaustive) names the module that answers it, taking `&HookCtx`.
3. **The module.** Put the handling in `src/<event>.rs`, with a `//!` header naming its entry
   point. Read through `HookCtx` (`ctx.index()`, `ctx.config()`, `ctx.arms()`, `ctx.repo_key()`),
   write its record through `record::Record` (and add the kind to `RecordKind`, below), answer the
   mod through `answer::ModAnswer` (below), and call the extension through `extension::guarded`.
   Fail open: return `Err(Failure)` or `Ok(None)`, and never print.
4. **The runtime.** For a decided event, add it to `adage_trace::Event`, `fast::skip`
   (exhaustive on `Event`) and the `decide` match. `rebuild::kind_of` reads the table already.
5. **Readers.** The feed's `event_label` (adage-cli `ui/feed.rs`) and any adage-app reader of
   its records (docs/spool.md lists them).
6. **Tests.** Add the event's rows to `harness/dispatch_tests.rs` (every harness that sends it,
   and one that does not), unit tests in the module using `crate::testkit` worlds, and a golden
   step in `adage-cli/tests/hooks.rs` for a command-hook event (re-blessed with `ADAGE_BLESS=1`;
   only added lines may change).

## How to add a spool record kind

One place: `RecordKind` in `src/record.rs`. Add the variant and its `kind` (`as_str`; a kind is
never renamed), build the record with `Record::new`, add a case to `record/snapshot_tests.rs`
and accept the new lines (`INSTA_UPDATE=always`, or `cargo insta review`), and describe it in
`docs/spool.md` with the readers that use it.

## How to add a mod answer

One place: `ModAnswer` in `src/answer.rs`. Add the variant (its keys are the wire shape), print
it with `answer::answer`, add a case to `tests/wire.rs` (its `key` match is exhaustive) and
write its file with `ADAGE_BLESS=1 cargo test -p adage-hook --test wire`. The mod's tests read
the same files; docs/claude-mod/impl/wire.md is their contract.

## The typed seams (MAINT-RUST W1, W2, W3, W4, W6, W7)

A second pass made the hook's extension points typed; the steps above describe the result.

- **W1, the event table.** `adage_trace::Harness::events` (with `EventKind`) is the one table of
  native event names. `handle` matches `Adapter::classify`'s `HookEvent` exhaustively; the
  adapters' `translate`, `is_learn_point`, the press's `act_of` and `CLOSERS`,
  `claude::MOD_EVENTS` and `is_mods`, and `rebuild::kind_of` read the table. The press still
  runs first, before the `.adage/disabled` switch and before `classify` (and before
  `ADAGE_DISABLE`, in `run`). `harness/dispatch_tests.rs` was written first, against the string
  dispatch, and keeps a frozen copy of it (`legacy`) as the oracle: every (harness, native event,
  payload class) row gives the same path, record kind, legacy `kind_of`, press action and
  live-learning request, from the old dispatch and the new; a second test runs every row
  through `handle` itself.
- **W2, capabilities.** Outside `harness/`, nothing compares a `Harness` value. `pretool` gates on
  `adapter.capabilities(..).pre_tool_rewrite`, `standing` on `cached_section`; `SkillPrompt`
  and the press on the harness sending that event at all (its table); the record's `harness`
  field on `Adapter::recorded_name`. One site keeps the command hooks' set on purpose, with a
  comment: the session start's contract (`session_start.rs`), since a session start is always a
  command hook's.
- **W3, the spool record seam.** Above. `record/snapshot_tests.rs` was written first; every
  record's bytes are unchanged but the press's, which gain `duration_us` (a fix: the CHANGELOG
  already said every record carries it). docs/spool.md documents every kind.
- **W4, mod answers.** `answer::ModAnswer` and `answer::answer`; the wire files in
  `tests/wire/`. Every answer's bytes are unchanged (they go through a sorted `Value`, as
  `json!(…).to_string()` did).
- **W6, `HookCtx`.** Above. The two `#[allow(clippy::too_many_arguments)]` left (on `pretool`
  and `stop::gated`) are gone, and so are the `Start` and `Here` structs it replaces.
- **W7, `Site`.** `Extension::session_context`, `session_texts`, `exec_only` and
  `predict_grants` take `&Site` instead of loose `(repo_key, root, index, paths)` arguments.
  adage-exec's `Prefetch` impl unpacks it into its unchanged `grants` functions.

Still open:

- **`Ctx::for_hook` and an `ensure_arm` in adage-runtime.** Those belong to adage-runtime's owner.
  The arm helper lives in the hook (`site::ArmInputs`) and needs no runtime change.
- **The feed's own name table.** adage-cli's `ui/feed.rs::event_label` maps native names for
  records without a `kind` by itself (and maps `PreToolUse` to `pretool`, which `kind_of` does
  not). It could read `adage_trace::Harness::events`; it is adage-cli's.
- **`skill::prompt` and `skill::invocable` are public** and keep their signatures, so the
  `SkillPrompt` path reads the configuration through `ArmInputs::load` rather than `HookCtx`
  (one read either way).

## Proof of no behaviour change

### The split (S1, S2)

Run in `/home/user/adage-poc` on top of `83b36bc`:

- `cargo test -p adage-hook`: 53 passed. That is the 52 from before plus the new identity pin.
- `cargo test -p adage-hook -- --list`, before and after. Sorted by test name, the two lists are
  identical apart from the added test. Full paths now carry the module (for example,
  `tests::the_dry_run_plans_and_the_verdict_decides_once` became
  `pretool::phases::tests::the_dry_run_plans_and_the_verdict_decides_once`).
- `cargo clippy -p adage-hook --all-targets -- -D warnings`: clean.
- `cargo doc -p adage-hook --no-deps`: no warnings, and none with `--document-private-items`.
  `target/doc/adage_hook/all.html` (every public item and its path) is identical before and
  after.
- `cargo test -p adage-cli --test hooks -p adage-exec --test parity`: 23 and 23 passed. No golden
  was re-blessed, and `crates/adage-cli/tests/golden/` is unchanged.
- `cargo test -p xtask`: 18 passed (the layering test included).
- `cargo fmt -p adage-hook -- --check`: clean.

### The typed seams (W1–W4, W6, W7)

Run in `/home/user/adage-poc` on top of `e4609ad`, after each item and at the end (other agents'
work in adage-cli and adage-tracks was in the tree at the same time):

- `cargo fmt --all -- --check`: clean.
- `cargo clippy -p adage-hook -p adage-trace -p adage-runtime -p adage-exec --all-targets -- -D
  warnings`: clean.
- `cargo test -p adage-hook -p adage-trace -p adage-runtime -p adage-exec`: adage-hook 57 unit + 2
  wire (53 before), adage-trace 6 (5), adage-runtime 113 + 1, adage-exec 111 unit and every
  integration binary (parity 23) pass. `journal::tests::durable_completion_deduplicates_…` in
  adage-exec failed once under load and passed 5 of 5 alone; it is in a file this pass did not
  touch.
- `cargo test -p adage-cli --test hooks --test press`: 23 and 12 pass. No golden was re-blessed:
  `git diff --stat -- crates/adage-cli/tests/golden` is empty.
- `cargo test -p xtask`: 18 pass.
- `cargo test -- --list` per crate, sorted, against the list at `e4609ad`: nothing removed; added
  are only the new tests (`harness::dispatch_tests::{the_legacy_dispatch_gives_every_row,
  handle_takes_each_rows_path}`, `record::snapshot_tests::every_record_kind_as_the_spool_holds_it`,
  `record::tests::a_decided_records_kind_is_its_events_name`, the wire tests
  `every_answer_to_the_mod_matches_its_wire_file` and `an_answer_prints_as_its_json_did`, and
  adage-trace's `harness::tests::a_native_event_name_means_one_thing`).
- `target/doc/<crate>/all.html`: nothing removed; added are `adage_trace::harness::EventKind`,
  `adage_hook::harness::{EventKind, HookEvent}`, `adage_hook::Site` and `adage_hook::answer::*`.
- The spool snapshot is byte-identical before and after W3 but for the three press lines, which
  gain `duration_us`.

`hook_latency` (debug build, 100 runs, each pair of binaries alternating in one run, `-` for the
model directory), ms. Before W6: `e4609ad` against the tree with W1 to W4. Across W6: the tree
before W6 against the tree after it.

| Scenario | `e4609ad` p50 / p95 | W1–W4 p50 / p95 | before W6 p50 / p95 | after W6 p50 / p95 |
| --- | --- | --- | --- | --- |
| `pretool_no_match` | 4.42 / 5.04 | 4.42 / 5.28 | 4.34 / 5.25 | 4.43 / 5.37 |
| `batch_no_match` | 5.29 / 6.36 | 5.35 / 6.20 | 5.45 / 7.41 | 5.52 / 7.20 |
| `turn_step_nothing_pending` | 5.48 / 7.17 | 5.47 / 6.68 | 5.17 / 6.84 | 5.31 / 6.71 |
| `no_routes_for_repo` | 5.45 / 6.39 | 5.38 / 6.50 | 5.32 / 7.17 | 5.30 / 6.59 |
| `batch_no_match_with_candidates` | 5.12 / 6.58 | 5.17 / 6.38 | 4.96 / 5.60 | 5.03 / 5.59 |

Within 0.15 ms at p50 on every fast-path scenario, in either direction: noise. Nothing reads
before `names_python` or `fast::skip` that did not before; the reads W6 saves (a second and
third index and configuration read on a `PreToolUse` that names Python, a second configuration
read in `respond`) are off these paths.

## Merging main (`350ab0b`, #32)

Main's #32 added `receipt.rs` and one hunk in the old monolithic `handle`: after the
`.adage/disabled` switch, in a repository, record a delivery receipt when the event translated
(`adapter.translate(…).is_some()`) or was `PreToolUse`. On this branch `handle` is still where
that happens (it is the dispatch), so the hunk stays in `lib.rs`, expressed over the classified
event: `HookEvent::PreToolUse | SessionStart | Decide(_)`, which is exactly what translated
before plus the mod's `TurnStep` (a decided event). The press is answered before the switch, and
the mod's asks for the standing text and a skill's text answer the mod rather than an agent's
event, so none of them writes a receipt. The receipt is a file beside the spools, never a spool
record: no golden, wire file or record snapshot changes. `receipt::tests` (main's) runs
`handle` on a Codex prompt with an empty index and checks the receipt, the one `unrouted`
record and that `.adage/disabled` writes none.
