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
| `lib.rs` | Reading stdin, the kill switches, panics and fail-open (`main`, `run`), and the dispatch of one event to its module (`handle`); the re-exports | `main`, `run`, `handle` |
| `extension.rs` | The extension contract adage-exec implements: `Extension`, `Turn`, `Stimulus` and `stimuli`, `Delivery`, `PreCall`, `PreNote`, `ReceiptCard`, `SessionTexts`, `StopCall`, `StopGate`; all re-exported at the root | `Extension`, `guarded` |
| `pretool/mod.rs` | `PreToolUse` from a command hook: the word scan, one decision per call, the receipt note, the arm claim, the `pretool` record | `pretool::pretool` |
| `pretool/interpreter.rs` | The U11 interpreter plan: the bound environment, a worktree's own environment, the person's Bash allow rules, the `exec/rewrite-off` marker | `interpreter_plan`; `rewrite_off` (public) |
| `pretool/phases.rs` | The mod's two phases of a Bash call: the dry run's plan, the verdict (rewrite, advice, substitution, a call answered by a receipt) | `pretool_mod` |
| `phase.rs` | The mod's `PreToolUse` payload fields (`dry_run`, `verdict`) and `identity_hash` (public, unchanged) | `Phase::of` |
| `identity.rs` | The keys of the session's two identity lists: decided calls and calls answered by a receipt (see below) | `decided_key`, `answered_key` |
| `standing.rs` | The mod's `Standing`: the standing text, the environment sentence, what a subagent is told | `standing::standing` |
| `session_start.rs` | A session start (its record, the bootstrap or rebind learner, the session texts, the person's line) and the `seen` record of a session in a repository with no reflexes | `session_start`, `record_seen` |
| `respond.rs` | Decide and deliver every other event: the no-routes path, the fast path, `decide()` under the state lock, a candidates-only repository's noticing, `TurnStep` prediction and routing, the extension's delivery, the record | `respond` |
| `stop.rs` | The finish-line gate at a `Stop`: its deadline, its `stop_gate` record, joining its result to the decision's | `gated` |
| `record.rs` | The spool writer's shared steps: `ts`, `duration_us`, the harness's name, and the append (failure returned or logged) | `append`, `append_or_log` |
| `site.rs` | What every path asks before deciding: the repository key the index routes this checkout by, the session's arm, observe mode | `routed_key`, `ArmInputs`, `observed` |
| `press.rs` | The press token (unchanged apart from the shared append) | `press::gate`, `press::before_kill_switch` |
| `skill.rs` | `SkillPrompt`, and the skills a `TurnStep` may invoke | `skill::prompt`, `skill::invocable` |
| `harness/` | The adapters: payload in, normalised event out, rendering back | `harness::adapter` |
| `autolearn.rs`, `failopen.rs` | Live learning at a turn's end; the fail-open log | unchanged |
| `testkit.rs` | Test-only: `ObligationWorld` and the other worlds, the stand-in extensions, `bash_call`, `spooled` | `#[cfg(test)]` |

The TurnStep, candidates and noticed logic is in `adage-runtime` (`decide`, `predict_step`,
`notice`). The hook only gates it, in `respond`, so there is no hook module for it.

## Helpers

- `record::now_ms()`: every record's `ts` (`adage_core::time::now_ms`). The value and its JSON are
  the same as the old `SystemTime` expression.
- `record::duration_us(started)`: every record's `duration_us`.
- `record::name_harness(&mut rec, adapter)`: `harness` on records from harnesses other than
  Claude Code.
- `record::append(paths, session, &rec)`: returns the `spool` failure, for a decision's record
  (`pretool`, `session_start`).
- `record::append_or_log(paths, session, harness, event, &rec)`: logs the failure to
  `fail-open.log` and carries on. Every other writer uses it, the press and `SkillPrompt`
  included. The harness name logged is what each site logged before (the adapter's name, the CLI
  argument at the Stop gate, `claude` for the press).
- `site::routed_key(index, root, paths, with_candidates)`: the key the index names this checkout
  by, or `None`. `respond` passes `with_candidates: true`.
- `site::ArmInputs::load(paths)` (one config read) or `ArmInputs::with_share(share)` (config
  already read), then `.ensure(st, session, key)` inside the state update: assigns an arm,
  propensity and repository once.
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

The field doc on `SessionState::answered` in adage-runtime says "by the same hash", which is wrong.
The doc line "Whether the `PreToolUse` call with this identity hash was decided this session." sits
on `see_answered` but belongs to `has_seen_pretool`. adage-runtime is owned by the agent splitting
it, so both corrections are listed for them in this pass's report, not made here.

## How to add a hook event

As things stand after the split:

1. **The adapter.** Teach `harness/<harness>.rs` the native name. For a decided event, that is
   `translate` (and `adage_trace::Event` with `Event::name`, if the event is new to the runtime).
   For a mod-only event, add it to `MOD_EVENTS` and `is_mods` in `harness/claude.rs`, and to
   `capabilities` and `Capabilities` in adage-trace if it brings a new capability. A command hook
   also needs `settings()` in the adapter. If the answer has a new shape, add an `Output` variant
   and handle it in every adapter's `render`.
2. **The dispatch.** In `lib.rs::handle`, either let the event fall through to `respond`, the
   default for anything `translate` returns, or add one `if event == "<Name>"` branch that calls
   your module's entry point. Put the branch after the press gate and the `.adage/disabled`
   switch. Put it before the index load if the event needs no index, as `PreToolUse` and
   `SkillPrompt` do.
3. **The module.** Put the handling in `src/<event>.rs`, with a `//!` header naming its entry
   point. Use `site::routed_key` for the repository key, `site::ArmInputs` if the event assigns
   an arm, `record::*` for its spool record (`ts`, `duration_us`, `name_harness`), and
   `extension::guarded` for any extension call. Fail open: return `Err(Failure)` or `Ok(None)`,
   and never print.
4. **The runtime.** For a decided event, add it to `fast::skip` (exhaustive on `Event`), the
   `decide` match, and `rebuild::kind_of` if its records are replayed.
5. **The press.** If the agent's act closes the press window, add the event to `press::CLOSERS`
   and to `press::before_kill_switch` if needed.
6. **Readers.** Add the feed's `event_label` (adage-cli `ui/feed.rs`) and any adage-app reader of
   the new record kind.
7. **Tests.** Put unit tests in the module's `mod tests`, using `crate::testkit` worlds, and add a
   golden step in `adage-cli/tests/hooks.rs` for a command-hook event (re-blessed with
   `ADAGE_BLESS=1`; only added lines may change).

MAINT-RUST W1 (typed event dispatch) would turn steps 1 and 2 into one `enum HookEvent`. It waits
on a characterization of the press-before-kill-switch ordering.

## Not done in this pass, and why

- **A per-event context loading the index and config once (MAINT-RUST W6).** The plan marks it
  "should wait": it changes read timing (a `PreToolUse` dry run still loads the index up to three
  times and the config up to three times) and needs the paired `hook_latency` measurement first.
  Only the helpers were extracted. Every load happens where it did before.
- **`duration_us` on press records.** The plan files this under W3 as a visible change to the
  spool that needs a decision. Press records are unchanged.
- **Moving `phase.rs` under `pretool/`.** `adage_hook::phase` is a public module path, so it stays
  at the root. The mod's phases are handled in `pretool/phases.rs`.
- **`Ctx::for_hook` and an `ensure_arm` in adage-runtime.** Those belong to adage-runtime's owner.
  The arm helper lives in the hook (`site::ArmInputs`) and needs no runtime change.

## Proof of no behaviour change

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
