# P1 · `TurnStep` in the binary (POC of chunk C1, with C7's chain and C8's `ending`)

The binary half of the mod's `turn.step` (mod architecture §3.2, plan §2.2 items 2 and 4, chunk C1):
`adage hook claude TurnStep` decides "make this call in the agent's place now, or nothing", on
the CPU, from what Adage already directed. Nothing is ever predicted by `adage hook` alone (it
has no extension, so no `predict` grants); the grant itself is chunk C2 (adage-exec).

## The JSON contract (shared with the mod)

**Invocation.** `<bin> hook claude TurnStep`, the payload on stdin, exit code always 0.

**stdin**

```json
{ "session_id": "…", "cwd": "/abs/path", "hook_event_name": "TurnStep",
  "turn_id": "…", "index": 3, "model": "claude-…", "agent_id": "…", "ending": false }
```

| Field | Required | Meaning |
| --- | --- | --- |
| `session_id`, `cwd` | yes | as every hook payload (no session or cwd → nothing printed) |
| `turn_id` | yes, non-empty string | the turn |
| `index` | yes, integer `0..=u32::MAX` | the step's position in the turn; 0 is the prompt's own request |
| `model` | no | recorded only |
| `agent_id` | no | set for a subagent's step: **nothing is ever predicted** for it |
| `ending` | no, default `false` | `true`: the model's response to request `index` just ended the turn (`end_turn`, no tool call); only then is an armed finish-line obligation predicted |

A payload missing `turn_id` or `index` (or with a negative or non-integer index) is not a
`TurnStep`: nothing is printed.

**stdout:** either nothing at all (the mod treats empty stdout as "no prediction"), or one line:

```json
{"predict":{"kind":"run","route":"lookup","track":"acme/work","calls":[{"tool":"Bash","input":{"command":"./scripts/wi-lookup W-2042 --json"}}]}}
```

| `kind` | When | `calls` |
| --- | --- | --- |
| `run` | a pending run directive, slots bound | one `{"tool":"Bash","input":{"command":"<rendered command>"}}` |
| `sequence` | a pending sequence directive | one Bash call: the sequence's **first** command |
| `skill` | a pending skill step (`expect_key` `control Skill <name>`) | one `{"tool":"Skill","input":{"skill":"<name>"}}` |
| `chain` | a read-batch directive whose files were bound from the preceding call's result (the lookup's `linked_files`) | one `{"tool":"Read","input":{"file_path":"<absolute path>"}}` per file, in the directive's order (paths are the repository root joined with the repository-relative path the directive names) |
| `obligation` | `ending: true` and an armed, unmet finish-line obligation | one Bash call: the obligation's bound command |

Key order inside the JSON is not significant (serde's map is sorted: `calls, kind, route, track`).
On Codex and OpenCode the event does not exist: their adapters do not translate it and an
`Output::Predict` renders as nothing.

**The calls the mod then makes** come back in the next `PostToolBatch` / `PostToolUseFailure`
with the mod's `tool_use_id`. Any id beginning `adage_` (the mod mints
`adage_<route>_<turnId>_<index>_<k>`) makes the canonical call `by: adage`; only the prefix is
read, so the binary does not care how the rest is spelled.

## What was built

- **`adage-trace`.** `Event::TurnStep { turn_id, index, model, agent, ending }`
  (`name() == "turn_step"`); `By { Agent, Adage }` (default `Agent`, serde lowercase,
  `By::of_tool_use_id`); `Call.by: By` and `Call.tool_use_id: Option<String>`, both
  `#[serde(default)]` and omitted when default, so every trace and cache written before reads and
  writes the same bytes. Every `Call` literal in the workspace gained `by: By::Agent,
  tool_use_id: None` (the ingest decoders included; setting `by` there from the transcript's
  ids is plan §2.1 item 3, not done here).
- **`adage-runtime`.**
  - `Pending.command` (`#[serde(default)]`): the rendered command, filled at every place a
    pending entry is made (`pending_for`, `bind_prompt_route`, `bind_output_route` through
    `bound_run`); empty when the command still holds a `{slot}`.
  - `PendingReads { route, files, batches_left }` and `SessionState.reads`: a read-batch
    directive's bound files (from `bind_result_route`, which now also returns the files). It
    carries no follow judgement (so no existing judgement or golden changes); it is retired when
    any of its files is read, after the follow window, or when the interlock drops its directive.
  - `SessionState.last_step_predicted`, `predictions`, `predicted_routes` (all serde default,
    omitted when empty).
  - `Prediction { kind, route, track, calls: Vec<PredictedCall { tool, input }> }`,
    `PredictionKind { Obligation, Run, Sequence, Skill, Chain }` (lowercase);
    `Decision.predicted`, `Decision.would_predict`, `Decision.predicted_at: Option<(turn, index)>`
    (all omitted when `None`); `Judgement::Predicted { route }`.
  - `Policy.predict_grants: Vec<(track, route)>` (default empty), `Policy.predict_cap`
    (default `PREDICT_CAP` = 6).
  - `decide`'s `TurnStep` arm (`predict_step`). Candidates in delivery order: pending directives
    (in the order delivered), then bound read batches, then (only with `ending`) armed unmet
    obligations. The first candidate that is in play, not exec-only, granted, not in
    `predicted_routes`, and not avoided by a dead end announced this session
    (`interlock::avoids` on the route, and `pattern::directs_avoided` on the bound command) is
    the prediction. Bounds: nothing at `index == 0`; nothing for a subagent; nothing without a
    grant; nothing when the step right before was predicted (before request `index`: a
    prediction at `(turn, index-1)`; at `ending`, which the mod asks with the index of the
    request the model just answered: a prediction at `(turn, index)`); past `predict_cap`,
    `would_predict` is recorded and nothing else. When it counts, both arms advance
    `predictions`, `last_step_predicted`, `predicted_routes` and set `predicted_at` and
    `would_predict`; only the treated arm sets `predicted`.
  - `Batch` and `Failure`: a call with `by == Adage` that follows a pending directive is judged
    `Predicted { route }`, never `Followed`/`Misfired`, and retires the pending entry. A Read by
    Adage of a pending read batch's file is judged `Predicted` once for the batch. An obligation
    met by Adage's call is judged `ObligationMet` (it is met: the Stop gate and a rebuild read
    that) and also `Predicted { route }` (the step was Adage's), as the plan's prediction-kinds
    table (§2.2 item 2b) says.
  - `fast::skip`: `TurnStep` is skipped when the state has no pending entry, no bound read batch
    and no unmet obligation. `Batch` is no longer skipped while a read batch is bound (so a read
    of its files retires it).
  - `rebuild`: `kind_of` maps `TurnStep` → `turn_step`; each record whose decision has
    `predicted_at` restores `predictions`, `last_step_predicted` and the route in
    `predicted_routes`. Pending entries and read batches are not rebuilt (as before for
    `pending`), so a rebuilt state predicts less, never more.
- **`adage-hook`.**
  - Claude adapter: `TurnStep` translation (above); `call_from` keeps `tool_use_id` and sets
    `by` from it (`PostToolBatch`, `PostToolUse`, `PostToolUseFailure`); `Output::Predict`
    renders `{"predict": …}`. Codex and OpenCode: no translation; `Output::Predict` → `None`.
  - `Extension::predict_grants(&self, repo_key, &Index, &Paths) -> Vec<(String, String)>`,
    default empty, asked only at a `TurnStep` (panic → empty).
  - `respond()`: after the fast path, a `TurnStep` with no grants returns nothing before the
    command table, the state lock and the spool; grants go on `Policy.predict_grants`. After
    `decide`, a step with no `would_predict` returns nothing and writes no spool record (an
    empty step is not worth a line per request). Otherwise one record `kind: "turn_step"` is
    spooled and the treated arm prints `Output::Predict`. Routes in
    `Delivery.cpu_predictions` are added to `predicted_routes` in the same state update that
    retires their pending entries, so a route adage-exec handled at a batch is never predicted
    at the next step. The no-routes path records no `seen` line for a `TurnStep`.
- **Consumers of spool kinds.** `rebuild::kind_of`; `ui/feed.rs` (`TurnStep` → `turn_step`,
  labelled `step`; a decision's `predicted` → act `chose`, `would_predict` → `would choose`,
  through `Act::Other`); `adage-app/src/savings.rs` (`turn_step` neither starts nor ends a
  session; `predicted` judgements are not follows); `adage-app/src/graph.rs` (a timeline event
  for a record with `predicted`/`would_predict`, the keys added only when present);
  `adage-eval/src/transcript.rs::hook_name` (`TurnStep`); `tests/g4_hook.rs` (payload for the
  arm). String-matching consumers (`instincts.rs`, `evidence.rs`, `review.rs`, `graph.rs`)
  count only `followed`/`ignored`/`misfired`, so `predicted` is never follow or extinction
  evidence there without further change.

## Files touched

```
crates/adage-trace/src/lib.rs
crates/adage-runtime/src/{lib.rs,fast.rs,rebuild.rs,tests.rs}
crates/adage-hook/src/lib.rs
crates/adage-hook/src/harness/{mod.rs,claude.rs,codex.rs,opencode.rs}
crates/adage-cli/src/ui/feed.rs
crates/adage-cli/examples/hook_latency.rs
crates/adage-cli/tests/{hooks.rs,g4_hook.rs}
crates/adage-cli/tests/golden/claude-code.txt
crates/adage-app/src/{savings.rs,graph.rs}
crates/adage-eval/src/{transcript.rs,savings_replay.rs}
crates/adage-eval/tests/prop_tracks.rs
crates/adage-ingest/src/{claude.rs,codex.rs,opencode.rs}      (Call literals only)
crates/adage-learn/src/{claims.rs,effects.rs,graph.rs}        (Call literals only)
crates/adage-learn/tests/routine_execution.rs                (Call literal only)
crates/adage-graph/src/canonical.rs                          (Call literal only)
crates/adage-tracks/src/matching.rs                          (Call literal only)
crates/adage-exec/src/hook.rs                                (Call literals only)
docs/claude-mod/poc/P1-notes.md
```

## Tests

New unit tests, `crates/adage-runtime/src/tests.rs`: `turn_step_predicts_a_delivered_directive`
(T-C1-01, incl. the `by: Adage` batch judged `Predicted` and an obligation at `ending`),
`turn_step_never_at_index_zero_nor_twice_in_a_row` (T-C1-02, incl. the `ending` index rule),
`turn_step_holds_the_session_cap` (T-C1-03), `turn_step_in_the_control_arm_records_would_predict`
(T-C1-04), `turn_step_predicts_nothing_without_a_grant_a_command_or_past_the_interlock`
(T-C1-05, plus CPU-predicted routes and a subagent step),
`turn_step_predicts_a_skill_and_a_sequence_first_command` (T-C1-06),
`turn_step_predicts_a_chain_of_reads_bound_from_the_lookup` (chain),
`turn_step_bounds_survive_a_rebuild` (T-C1-07), `turn_step_fast_path` (T-C1-08).

New in-process hook tests, `crates/adage-hook/src/lib.rs`: `turn_step_predicts_the_granted_finish_line`
(a test `Extension` granting `predict`: fast path before anything is armed; nothing and no
record without the grant; the exact stdout JSON; the `turn_step` record; the mod's `adage_` call
meets the obligation and no Stop request follows), `turn_step_in_the_control_arm_prints_nothing`
(`would_predict` spooled), `turn_step_on_each_adapter` (Claude translation and rendering, `by`
from `tool_use_id` on batch and failure, Codex/OpenCode print nothing; T-C1-09's adapter half).

Golden: `claude_scenario` gained one `TurnStep` step (`index: 2, ending: true`, after the edit
that arms `make gen`); the re-blessed `claude-code.txt` diff is exactly

```
+### TurnStep
+(nothing)
+
```

and `crates/adage-exec/tests/parity.rs` (the same scenario through `adage-exec hook`) passes.

`hook_latency` gained `turn_step_nothing_pending`. One local run (release build, 300 runs, `-`
for the model): `batch_no_match` p50 2.64 ms / p95 3.49 ms; `turn_step_nothing_pending` p50
2.56 ms / p95 3.11 ms. Not the paired two-build comparison the acceptance asks for.

### Results (as run; `cargo test … --locked`)

- `cargo test -p adage-trace -p adage-runtime -p adage-hook -p adage-cli -p adage-exec --locked`,
  before the last two edits (the `ending` "in a row" rule and the obligation's `Predicted`):
  42 suites, **518 passed, 0 failed** (hooks golden and `adage-exec/tests/parity.rs` included).
- The same command on the committed tree (45bc903): 36 suites reported, **514 passed, 1 failed**:
  `adage-runtime` `tests::turn_step_never_at_index_zero_nor_twice_in_a_row`
  (`tests.rs:2468`). The test is wrong, not the code: under the `ending` rule, a step
  `(t, 2, ending: true)` after a prediction at `(t, 1)` is *not* in a row (the model answered
  request 2). Fix: make that step `step("t", 2, false)`. Cargo stops at the first failing
  binary, so the suites after `adage-runtime`'s lib tests in that run were not re-run on 45bc903.
- `cargo test -p adage-eval --locked`: lib `28 passed`; `tests/bench.rs` `4 passed` (1921 s);
  `tests/g4.rs` **1 failed**: `contexts_fire_in_replay_and_the_fast_path_stays_exact_on_newkinds`
  (`g4.rs:182`) expects `"\n  make check "` (trailing space) where `render.rs:939` renders
  `"…is:\n  make check\nIgnore…"`. Neither file is in this change; not confirmed against a base
  build (the disk filled). The run stopped there, so the remaining adage-eval test binaries did
  not run.
- `cargo test -p adage-eval --locked --test gates`: **15 passed, 0 failed** (545 s).
- `cargo fmt --all --check`: clean (at the time of the last edit). `cargo test -p xtask` and
  `cargo clippy --workspace --all-targets -- -D warnings` were run by the coordinator on 45bc903
  (reported clean), not by this agent.

## Deferred, and open issues

1. **`turn_step_predicts` latency scenario: pending C2.** `hook_latency` spawns `adage`, which
   has no extension; a prediction needs adage-exec's `predict_grants` (C2: `GrantMode::Predict`,
   `review delegate --predict`). The in-process test above covers the behaviour, not the timing.
2. **adage-exec does not implement `predict_grants` yet** (C2), so neither shipped binary predicts.
3. **`Capabilities.predict_step` and friends** (mod architecture §1b.3) are not added; the event is
   simply not translated by the Codex and OpenCode adapters.
4. **Spool `duration_us`** (plan §2.2 item 9, chunk A2) is not in this POC.
5. **Ingest does not set `by`** from transcript ids, and the learner does not exclude `by: adage`
   steps from support (plan §2.1 item 3). `savings.rs` does not yet count a predicted step as a
   model turn not spent (needs the harness ledger line, plan §2.1 item 4).
6. **Once per route per session.** A route predicted once (or carried out by CPU prediction) is
   never predicted again in the session, even if its obligation is re-armed by a later edit or
   the mod's call was denied. Simple and safe; C8 may want "once per arming".
7. **Past the cap**, `would_predict` is recorded with `predicted_at` unset (test plan T-C1-03's
   wording); a rebuild therefore does not count it, which is what the bound needs.
8. **`decide` for a step whose candidate's route has no grant spools nothing**, in both arms: the
   control arm's `would_predict` exists only for granted routes (the grant is read whatever the
   arm), which is what the arm comparison needs.
9. **Chain only from the existing result binding.** `PendingReads` is made where the runtime
   already binds a `ReadBatch` from the triggering command's result; nothing new is parsed. The
   `file_path` sent is absolute (the Read tool's requirement); the directive's text names the
   repository-relative path.
10. **Skill steps** have no `Step::Skill` yet (D3). The runtime predicts a `Skill` call for any
    pending entry whose `expect_key` is `control Skill <name>` with `command = <name>`, which is
    how D3's pending entries will look; today no route creates one, so only the unit test
    exercises it.
11. **Docs** named by the plan for C1 (`docs/how-it-works.md` behaviour row, CHANGELOG) are not
    updated in this POC.
