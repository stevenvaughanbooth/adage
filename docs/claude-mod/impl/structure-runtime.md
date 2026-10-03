# adage-runtime: its structure

How `crates/adage-runtime` is laid out after the split of `src/lib.rs` (2248 lines) and
`src/tests.rs` (3369 lines), items S4 and S5 of the Rust maintainability review. The split moved
code and changed no behaviour: every public item keeps its path through a re-export at the crate
root, and every test keeps its name. Line references in older notes are to the layout before
this change.

## Module map

| Module | What it holds |
| --- | --- |
| `lib.rs` | The module list, every public re-export, the `RuntimeParams` defaults (`MAX_DIRECTIVES_PER_*`, `MAX_STOP_REQUESTS`) and `span_kind_label`. |
| `decide/mod.rs` | `decide`: the prologue (routes in play, context nets), one `match` over `Event` that calls one function per arm, the epilogue (approach locking, `emitted`, context deltas, arms); plus what the arms share: `Walk`, `emit_ok`, `fire_contexts`, `approach_lock`, `pending_for`, `pending_skill`. |
| `decide/prompt.rs` | `on_prompt`: inhibits chosen first, prompt routes, cards, intent routes, prompt context items, first-prompt announcements; `match_intent`, `match_card`, `card_text`. |
| `decide/batch.rs` | `on_batch`: per call, follow judgements, piped failures, obligations met, `after_edit`, `after_command`, `alphabet_items` for both context nets; `retire_reads`; the follow window closing. |
| `decide/failure.rs` | `on_failure` and `fire_after_failure` (also used by a piped failure in a batch). |
| `decide/stop.rs` | `on_stop`: the Stop request for the first unmet obligation. |
| `prediction.rs` | The `TurnStep` arm: `on_turn_step`, `predict_step`, `route_step`, `skill_call`, `told_inhibits`. |
| `state.rs` | `SessionState`, `Pending`, `PendingReads`, `Obligation`, `Arm`, `assign`, `assign_arm`, `identity_hash`, the identity rings (`see_pretool`, `see_answered`), history. |
| `decision.rs` | What is recorded: `Decision`, `Directive`, `Judgement`, `Prediction`, `PredictionKind`, `PredictedCall`, `Routing`, `Substitution`; `directive()`. |
| `policy.rs` | `Ctx`, `Policy`, `PREDICT_CAP`, `Embed`; the policy-dependent matching rules `follows`, `meets`, `in_play`, `follows_call`, `follows_learned`, `failure_matches`. |
| `observe.rs` | How a call is read: `command_key`, `call_keys`, `exec_segments`, `call_command`, `cap`, `added_tests`. |
| `binding.rs` | Slot binding and contracts: `bind_prompt_route`, `bind_output_route`, `bind_edit_slots`, `bind_result_route`, `contract_ok_typed`. |
| `standing.rs` | Text said once: `Standing`, `standing`, `standing_for`, `delegation`. |
| `noticed.rs` | `Noticed`, `MAX_NOTICED`, `notice` (candidates, never directed). |
| `pretool_decide.rs` | The mod's `PreToolUse` decisions outside `decide`: `answered_by_receipt`, `substitution`, `substitute`. |
| `checkout.rs`, `context.rs`, `fast.rs`, `orient.rs`, `pretool.rs`, `pytest_case.rs`, `rebuild.rs`, `render.rs` | Unchanged public modules (the checkout port, the matching re-export, the fast path, card orientation, the U11 rewrite, pytest facts, the spool replay, every template). |
| `tests/mod.rs` | The shared fixtures (`route`, `fixture`, `bash`, `track`, `predict_fixture`, `predict_ctx`, ...) and one `mod` per area. |
| `tests/*.rs` | One file per former banner section: `basics`, `intent`, `algebra`, `interlocking`, `typed_slots`, `contexts`, `turn_step`, `dead_end`, `chains`, `finish_line`, `receipt`, `attachment`, `noticed`. |
| `skill_tests.rs` | The `skill` step (D3), unchanged. |

The private modules are never named outside the crate; callers keep writing
`adage_runtime::decide`, `adage_runtime::Policy`, and so on.

## How to add things, as it stands

**A prediction kind.** Add the variant to `PredictionKind` (`decision.rs`; it serialises
lowercase into the spool's `predicted`/`would_predict`). Find the candidate in `predict_step`
(`prediction.rs`) at its place in the order (pending, reads, obligations at a turn's end), going
through `eligible` so grants, exec-only, inhibits and `predicted_routes` hold. If it needs state
that a rebuilt session must restore, add the field to `SessionState` (`#[serde(default)]`) and to
`rebuild.rs`, and if `decide` reads a new state field, add it to `fast::skip` and its table
(otherwise a `TurnStep` the fast path drops would have predicted). Outside the crate: the mod's
`step.ts` and `register.ts`, and the feed in adage-cli (`Act::Other("chose")`, `feed.rs`).

**A judgement.** Add the variant to `Judgement` (`decision.rs`; `kind` is `snake_case` in the
spool and is never renamed). Push it at the site that decides it: an event arm in `decide/` or
a function in `pretool_decide.rs`. If it changes state that `rebuild` restores, handle it in
`rebuild.rs`. Outside the crate, the readers match on the string: the feed's `from_record` and
`follow.rs` in adage-cli, and `graph.rs`, `savings.rs`, `review.rs`, `evidence.rs` in adage-app.

**A step kind.** The `Step` enum, its validation and its shape live in adage-tracks
(`model.rs`, `io.rs`, `shape`, `interlock.rs`). In this crate: its fixed template in
`render.rs` (`route_text`, and the matching regex in `adage-safety/src/lib.rs` in the same
change); its binding in `binding.rs` if it has slots; where it is delivered, per trigger, in the
arm that handles that trigger (`decide/prompt.rs` for prompt and intent triggers,
`decide/batch.rs::after_edit` and `after_command`, `decide/failure.rs::fire_after_failure`,
`decide/mod.rs::fire_contexts` for contexts), each pushing a `Pending` (`pending_for`,
`pending_skill`) when it is judged on a follow; whether it can be predicted in
`prediction.rs::predict_step`; and whether it goes in the standing text in
`standing.rs::standing_items`. A trigger the fast path does not know about needs its words in
`Triggers` and a row in `fast.rs`'s table.

## Notes on this change

- **`decide`.** The 620-line function is now a 60-line `match` (`decide/mod.rs`); the arms are
  `on_prompt`, `on_batch`, `on_failure`, `on_stop` and `prediction::on_turn_step`. The `Batch`
  arm's 380 lines are split further into `after_edit`, `after_command` and `alphabet_items` (the
  two context nets' symbolisation, which were copies). The `emit_ok` closure is a function of
  `ctx`; `fire_after_failure` calls it instead of taking it as an argument. A subagent's
  `TurnStep` is its own `match` arm (`agent: Some(_)`) that does nothing, as before.
- **Documentation drift fixed.** `SessionState::answered` said it was keyed "by the same hash" as
  `seen_pretool`; it is keyed by the hash of the bare `tool_use_id` (what a batch's call
  carries), while `seen_pretool` holds the hash of the hook's event identity. The docs of both
  fields and of `identity_hash` now say which; the keys are unchanged. The doc line of
  `has_seen_pretool` that sat on `see_answered` is back on `has_seen_pretool`. A new test pins
  the current keying and the ring: `tests::receipt::the_answered_ring_is_keyed_by_the_bare_tool_use_id`.
  Three `cargo doc` warnings (private items linked from public docs, in `orient.rs`,
  `pretool.rs` and `substitution`) are fixed, and the crate doc no longer says a repository path
  check is its only I/O (it goes through `Checkout`).
- **Config defaults.** The only configurable default that lives here is `PREDICT_CAP` (6), and it
  is spelled once, in `policy.rs`. Its doc now says the person's `runtime.predict_cap` defaults to
  the same number. The other copies (`"6"` in `KEYS`, `unwrap_or(6)` in `load_from`) are in
  adage-store, which this crate may not link (`FORBIDDEN` in `xtask/src/layers.rs`); single-sourcing
  them is adage-store's item S10.
- **Test names.** Every test keeps its function name; the path gains the area
  (`tests::foo` is now `tests::<area>::foo`; `tests::typed_slots::*` is unchanged). Two areas are
  named apart from their banner because a test module shadows a glob-imported item of the same
  name: `interlocking` (not `interlock`) and `contexts` (not `context`).
- **`crates/adage-runtime/AGENT.md`** (new on main) is added here with main's text and its "What's
  inside", tests and pitfalls lines brought up to this layout, so the merge takes this copy.
- **Main's hunks.** `git diff fff4fa7 origin/main -- crates/adage-runtime` changes `rebuild.rs`
  (two test lines; this change does not touch `rebuild.rs`, so it merges as is) and `tests.rs`
  (63 hunks, all the `CommandTable::load()` to `&'static CommandTable` follow-up: `table: &table`
  to `table`, `&table` to `table`, the fixtures' return types). They cannot compile on this
  branch, whose `adage-core` still returns an owned table, so they are not applied in the tree.
  They are carried as a patch against the split files, which `git apply --check` accepts:
  `main-hunks-for-split.patch` (in the session scratchpad, `/tmp/claude-0/-home-user-adage/7d64b41e-29be-5754-8558-0639ccc77725/scratchpad/`; not committed), 63 hunks, 66 lines, into
  `tests/mod.rs` (the shared `fixture`), `tests/basics.rs`, `tests/intent.rs` (including
  `intent_fixture` and `card_fixture`), `tests/algebra.rs`, `tests/interlocking.rs`,
  `tests/typed_slots.rs` and `tests/contexts.rs`. After the merge, the tests this branch added
  after main's base still pass `&table` (which derefs) and `predict_fixture` (`tests/mod.rs`)
  still declares an owned `CommandTable`; it needs the same `&'static` change.

### Proof of no behaviour change

| Check | Result |
| --- | --- |
| `cargo test -p adage-runtime -p adage-hook` | runtime 113 unit + 1 integration, hook 53: all pass |
| `cargo test -p adage-cli --test hooks` | 23 pass, no golden changed, nothing re-blessed |
| `cargo test -p adage-exec --test parity` | 23 pass |
| `cargo test -p adage-runtime -- --list` before and after | 113 before, 114 after. With the area stripped from the path (`tests::<area>::x` to `tests::x`), the lists are identical but for the one new test. The raw lists differ only in those paths |
| `cargo clippy -p adage-runtime --all-targets -- -D warnings` | clean |
| `cargo fmt -p adage-runtime --check` | clean |
| `cargo test -p xtask` | 18 pass |
| `cargo test -p adage-eval --test parity --test predict_gate` | 3 and 1 pass (they replay decisions) |
| `cargo test -p adage-eval --test gates -- fast_path_changes_no_decision replay_reproduces_the_live_round2_decisions replay_with_the_learned_index_gives_every_recorded_directive` | 3 pass |
| `cargo doc -p adage-runtime --no-deps` | 0 warnings (3 before) |
| Public API | `target/doc/adage_runtime/all.html` lists the same items at the same paths before and after; every `pub` name in `src/` before is still there |
| `hook_latency` (debug, 30 runs, paired old,new) | within noise, see below |

`hook_latency target/debug/adage - 30` (debug build, `-` for the model directory), first each
build alone, then the two builds paired in one run (`<old>,<new>`, alternating spawns), in ms:

| Scenario | Old alone, p50 / p95 | New alone, p50 / p95 | Paired old, p50 / p95 | Paired new, p50 / p95 |
| --- | --- | --- | --- | --- |
| `batch_no_match` | 5.23 / 7.06 | 3.88 / 4.99 | 5.66 / 8.44 | 5.62 / 6.41 |
| `turn_step_nothing_pending` | 4.76 / 5.18 | 4.52 / 4.70 | 5.61 / 7.88 | 5.47 / 7.00 |
| `pretool_no_match` | 4.65 / 5.53 | 4.00 / 4.27 | 4.92 / 6.03 | 5.17 / 6.37 |
| `batch_no_match_with_candidates` | 5.29 / 8.27 | 7.45 / 13.48 | 5.61 / 6.81 | 5.71 / 6.50 |

The unpaired runs swing by more than a millisecond either way between scenarios that run the same
code; the paired run puts old and new within 0.25 ms at p50 on every fast-path scenario. The fast
path (`fast::skip`) is untouched, and `decide`'s prologue is the same code; the arms are now
function calls. The "new" binary also carries whatever the other agents had in the tree at the
time.
