# adage-hook

The hook pipeline shared by `adage hook` (`adage-cli`) and `adage-exec hook`: read the harness
payload on stdin, translate it through a harness adapter, take the no-match fast path, decide
under the session's state lock, append to the spool, and print the adapter's rendering. It fails
open, always.

## Where it sits

- Layer 6 (the live hook pipeline) in `xtask/src/layers.rs`, beside `adage-eval`.
- Links: `adage-core`, `adage-trace`, `adage-store` (`Paths`, `state`, `spool`, `config`),
  `adage-tracks` (`Index`, `layout`), `adage-intent` (`model`, `normalize`), `adage-runtime`,
  `adage-ingest` (one call, `scope::count_claude_sessions`, for the first-session notice),
  `adage-pack` (only `binding`: is the instinct binding stale, and the verified environment).
- Linked by: `adage-exec`, `adage-app`, `adage-cli`.
- Forbidden: `adage-hook -> adage-learn` ("the live hook must not link the learner"). The hook
  runs on every agent event under the harness's timeout (5 s; 30 s for action events when the
  registration runs `adage-exec`, `harness::action_timeout`), and nothing in it may fail in a way
  the agent notices. The learner is slow and has its own failure modes, so live learning is a
  detached process (`autolearn::on_turn_end` spawns the adage binary with `condition --auto`),
  never a function call. See [`../../docs/architecture.md`](../../docs/architecture.md).

## What's inside

- `lib.rs`: `main(harness, event, opts) -> !` (panic hook, big stack, `catch_unwind`, always
  exit 0); `run` (stdin, `ADAGE_DISABLE`); `handle` (one event with its payload and `Paths`: the
  press first, then the `.adage/disabled` switch, then one exhaustive `match` over the adapter's
  `HookEvent`: `PreToolUse` to `pretool`, `Standing` to `standing`, `SkillPrompt` to `skill`, a
  session start to `session_start`, every decided event to `respond`, then the Stop gate and
  autolearn); `Options { learner, extension }`; `on_big_stack`; the re-exports (the extension
  contract, `rewrite_off`). Only reading, the kill switches and the dispatch live here; each
  event's handling is in its own module, and takes the event's `HookCtx`.
- `ctx.rs`: `HookCtx`, one event's context and the reads it shares (configuration, index,
  repository key, arm inputs), each made once, on first use.
- `extension.rs`: the contract `adage-exec` implements, re-exported at the root: the
  `Extension` trait (`deliver`, `session_context`, `session_texts`, `exec_only`,
  `predict_grants`, `pretool_context`, `stop_gate`) with `Site` (repository key, checkout, index,
  data directory: what the four session-level methods take), `Turn`, `Stimulus`, `stimuli`,
  `Delivery`, `PreCall`, `PreNote`, `ReceiptCard`, `SessionTexts`, `StopCall`, `StopGate`;
  `guarded` (an extension call whose panic loses only that answer).
- `answer.rs` (public): `ModAnswer`, every answer the binary sends the Claude Code mod (`plan`,
  `substitute`, `receipt`, `standing`, `text`, `press`, `predict`, `routine`), and `answer`, the
  one function that prints it. `tests/wire/*.json` pins each shape; the mod's tests read the same
  files (docs/claude-mod/impl/wire.md).
- `respond.rs`: decide and deliver every event but `PreToolUse` and `SessionStart` (the
  no-routes path, the fast path, `decide()` under the state lock, a candidates-only repository's
  noticing, `TurnStep` prediction and routing, the extension's delivery, the record).
- `pretool/mod.rs` (`pretool`: the word scan, one decision per call, the receipt note, the arm
  claim, the record), `pretool/interpreter.rs` (the interpreter plan and the public
  `rewrite_off`), `pretool/phases.rs` (the mod's dry run and verdict, `pretool_mod`).
- `session_start.rs` (`session_start`, `record_seen`), `stop.rs` (the finish-line gate's
  deadline and record, `gated`), `standing.rs` (the mod's standing text), `skill.rs`
  (`SkillPrompt`, the skills a `TurnStep` may invoke), `press.rs` (the press token).
- `phase.rs`: the mod's `PreToolUse` payload fields (`Phase::of`) and the public
  `identity_hash`; `identity.rs`: the keys of the session's two identity lists (`decided_key`,
  `answered_key`), which hold the same call under different hashes on purpose.
- Shared helpers every module uses: `record.rs`, the spool record seam (`RecordKind`, every kind
  a hook writes; the `Record` builder, which stamps `ts`, `event`, `kind`, `harness`,
  `tool_use_id(s)` and `duration_us`; docs/spool.md lists every kind and its readers), and
  `site.rs` (`routed_key`, `ArmInputs`, `observed`).
- `testkit.rs` (`#[cfg(test)]`): `ObligationWorld` and the other worlds, the stand-in
  extensions, `bash_call`, `spooled`.
- `harness/mod.rs`: the `Adapter` trait (`harness`, `name`, `recorded_name`, `envelope`,
  `capabilities`, `translate`, `classify`, `event_identity`, `render`, `max_context` = 9,500,
  `is_learn_point`), `HookEvent` (what `handle` matches on), `EventKind` (re-exported from
  adage-trace's event table), `Output` (`Context`, `SessionContext`, `Continue`, `Notice`,
  `Rewrite`, `Predict`, `Routine`), `Envelope`, `adapter(name)`,
  `adapter_for(Harness)`, `is_own_text`, `calls`, `tool_event_identity`,
  `ACTION_HOOK_TIMEOUT_S`, `action_timeout`.
- `harness/claude.rs` (`Claude`, `settings`, `response_text`), `harness/codex.rs` (`Codex`,
  `settings`, `sh_quote`), `harness/opencode.rs` (`OpenCode`, `plugin_source`, which embeds
  `plugin/opencode/adage.js` with `include_str!`).
- `autolearn.rs`: `on_turn_end`, `Queue` (request, lock, spawned, state, notice, shown files),
  `RunState`, `Notice`, `Shown`, `take_notice`, `TEST_SPAWN_ENV`.
- `failopen.rs`: `Failure`, `record` (one JSON line in `$ADAGE_HOME/fail-open.log`, rotated at
  `ROTATE_BYTES`), `install_panic_hook`, `panic_failure`.

## How it connects

- In: a native JSON payload on stdin. `harness::adapter` picks the adapter by
  `Harness::hook_name` (`claude`, `codex`, `opencode`); `envelope` gives session and cwd;
  `classify` names the event from adage-trace's event table (`Harness::events`), translating an
  event the runtime decides into an `adage_trace::Event` (`HookEvent::Ignored`: do nothing).
- Index: read once per event through `HookCtx::index` (`Index::try_load(paths.index())`); a
  damaged index is served empty and a bootstrap learner is asked for at the next session start.
- State and spool: `adage_store::state::update_or_rebuild` under the state lock, with
  `adage_runtime::rebuild` as the fallback; one spool record per decided event.
- Decision: `respond` (`respond.rs`) builds `adage_runtime::Ctx` with `checkout::Disk`, `CommandTable::load()`,
  an embed closure over `adage_intent::model::resolve_static`, and a `Policy` from
  `ADAGE_CARD_CHECKS`, `ADAGE_CARD_ORIENTATION`, `layout::read_follow_weights` and
  `Extension::exec_only`; then calls `decide`.
- Out: `render::wrap_directive` per directive, fitted to `max_context`, then
  `adapter.render(event, Output)` to stdout.
- `Extension`: `adage-exec` implements it (`Prefetch` in `adage-exec/src/hook.rs`, each method
  one call into a job module under `src/hook/`) for E1 data blocks, E2 actions, receipts and the
  finish-line gate. Plain `adage hook` passes none and
  behaves exactly as the pipeline above.

## Changing it

Invariants to keep:

- Fail open: on any `Err` or panic, print nothing, exit 0, record a `Failure` without payload
  content. Nothing but the harness's JSON may reach stdout. Every extension call is wrapped in
  `catch_unwind`; a panic in `deliver` becomes a recovery warning, never a retry instruction.
- Drain stdin first (`read_payload`, 8 MiB cap) so the harness never sees a broken pipe.
- Latency: `fast::skip` runs before the command table, the state lock and the spool; a
  repository with no reflexes returns from `respond` before any of them. Keep new work behind
  the same gates, and keep the Stop gate inside `stop_deadline(started)`.
- Never block the agent: `Output::Rewrite` changes only the interpreter prefix, advice is never
  a refusal, and the control arm receives no directives, rewrites or actions.
- Runtime purity: environment variables, config and disk reads belong here and flow into
  `Policy`/`Ctx`; do not add them to `adage-runtime`.
- Stop continuations must satisfy `harness::is_own_text`, or Codex and OpenCode re-read them as
  the user's prompt (`own_text_tests`).
- A new `Harness` variant needs an arm in `adapter_for` (the match is exhaustive).

Tests: `cargo test -p adage-hook` (unit tests in each module's `mod tests`, built on the
`testkit.rs` worlds; `harness/dispatch_tests.rs` characterizes the dispatch, and
`record/snapshot_tests.rs` with its insta snapshot under `src/record/snapshots/` every spool
record's bytes; `tests/wire.rs` checks the answers to the mod against `tests/wire/*.json`,
re-blessed with `ADAGE_BLESS=1`).
`docs/claude-mod/impl/structure-hook.md` has the module map and the steps for adding a hook
event, a spool record kind and a mod answer. Then `cargo test -p adage-exec -p adage-cli -p adage-app`;
`adage-cli`'s end-to-end tests run the binary, and `adage-exec`'s tests call `handle` with its
extension. `READS` in `xtask/src/affected.rs` declares `plugin/` for this crate (the OpenCode
plugin source). Before a release, `CONTRIBUTING.md`'s startup gate holds the plugin to its
latency budget.

Pitfalls: behaviour differs by `Capabilities` or by the event table, never by comparing a
`Harness` value outside `harness/`. `pretool` is Claude Code only (`pre_tool_rewrite`), and off in a
session started inside a virtualenv or conda environment. `ADAGE_TEST_FAULT` and
`ADAGE_TEST_NO_SPAWN` exist only in debug builds. Codex has no tool batches and no failure
signal (`harness/codex.rs`), so failure-triggered routes do not fire there. The decision core
is `respond.rs` (and `pretool/` for `PreToolUse`).
