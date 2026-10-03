# C3 · C7 · C8 · D2 · C9 (binary side) · E2/E3 (offline) · the simulator: implementation notes

What was built for chunks C3, C7, C8 and C9 of [the implementation plan](../implementation-plan.md)
on the binary side, D2 in full, the offline parts of E2 and E3, and the simulator's share of all
of them. Wave 3 builds the mod's side of C3, C7, C8 and C9 **from this file alone**: every payload
the mod sends and every answer it reads is specified below, with what the mod must still verify
in a live session.

Everything the mod sends carries `"via": "mod"` (see "Capabilities"). Every event is
`<bin> hook claude <Event>` with the payload on stdin; exit code always 0; empty stdout means
"nothing"; any error is "nothing" (fail open).

## C3 · Receipt as the result, rewrites checked by the engine (binary side)

### What was built

- **`PreNote`** (`adage_hook::PreNote { text, fresh, result, covering }`) is what
  `Extension::pretool_context` returns now (it returned a string). adage-exec's
  `covering_receipt` (`crates/adage-exec/src/hook.rs`) finds the session's latest completed,
  applied learned pytest verification (an `Execute` E2 grant) whose run covers every test the call
  selects (`covers()`, unchanged), and reports it:
  - **fresh**: the call is made at the repository root and the run's input snapshot is
    byte-identical now (recomputed within the 1.5 s budget). `text` is the `<adage-receipt>` block
    (unchanged), `result` the receipt as the tool's output would read, `covering` the template
    that ran;
  - **not fresh**: a snapshotted file changed since, the snapshot could not be taken in time, or
    the call is made from another directory: only `covering`; `text` empty, `result` `None`;
  - **none**: no covering run (not a pytest call, wider scope, another session, a failed or refused
    run, an active virtualenv elsewhere).
- **`result`** (`receipt_result`): `Adage answered this test run from its own run of
  \`<template>\` in this session (<journal ts>[, operation <id>]): it passed on files identical to
  the current ones, and it ran every test this command selects. The command itself did not run
  again.` then, when the journal binds the run's pytest node-id record (its sha256 equals the file
  now), `Tests it ran:` and one `- <node id> passed` line per id inside the call's targets, the
  session's own new tests first (`SessionState.new_tests`), at most 12 and 2,000 bytes,
  `- and N more`. Every interpolated value through `render::sanitise`.
- **The command hook's output is unchanged**: it delivers `text` beside the call only when fresh
  (`fresh_note`); a stale receipt says nothing (as before: `covering_receipt` returned `None`).
- **The repository key**: `receipt()` now resolves the key the index (and the journal) uses
  (`repo::lookup_key` against the index's repositories) before asking the extension. Before, it
  passed the raw checkout key, which for a checkout without a remote is `<name>@path-<hash>` and
  never matched the journal's `repo`, so no receipt was ever found there (found by the new
  pipeline test; a real fix, not only C3's).
- **`verdict: { answered: "receipt" }`** (`pretool_mod` → `answer_with_receipt`): asks the
  extension again; only a receipt still fresh with a result is used. In one state update: the
  call is marked decided (the identity ring), the arm assigned if none, and
  `adage_runtime::answered_by_receipt(ctx, st, command, covering)` observes the call as made and
  passed: a pending directive it follows is judged `followed`; every unmet obligation that the
  call's key **or the covering command's key** meets is met (`obligation_met`). One `pretool`
  record is spooled. A session found in the control arm gets nothing and a `withheld` record.
- **Capabilities gate it**: the plan's `substitute` only with `substitute_call`, the receipt's
  `result` only with `answer_call` (the mod's payloads always have both).

### Contract (the mod builds on this)

**Dry run** (`dry_run: true`, plus the command hook's fields: `session_id`, `cwd`, `tool_name:
"Bash"`, `tool_input.command`, `tool_use_id`, `permission_mode`): unchanged from A1 except the
`receipt` key, now:

```json
{"plan":{"receipt":{"fresh":true,"covering":".venv/bin/python -m pytest tests/test_x.py -q",
                    "note":"<adage-receipt operation=\"…\">\n…\n</adage-receipt>",
                    "result":"Adage answered this test run from its own run of `…` in this session (…): …"}}}
{"plan":{"receipt":{"fresh":false,"covering":".venv/bin/python -m pytest tests/test_x.py -q"}}}
```

`note` and `result` appear only when `fresh`. The other plan keys (`substitute`, `rewrite`,
`advice`) are as in A1-A2-C5-notes.md and may appear beside `receipt`.

**Final ask** (`verdict: { "answered": "receipt" }`): only after the dry run said `fresh: true`
and `$.tool.check` on the **original** command answered `allow`. Answer:

```json
{"receipt":{"result":"Adage answered this test run …","note":"<adage-receipt …>…</adage-receipt>","covering":"…",
            "card":{"operation":"3f9c1a2b7d4e","ran_at":"2026-10-03T09:00:00Z","ago":"4 min ago",
                    "summary":"passed on files identical to the current ones · 2 tests this command selects",
                    "tests":["tests/test_x.py::test_zeta","tests/test_x.py::test_alpha"],"more":0}}}
```

`card` is the receipt card's data (the mod's `answered` mark `{receipt, summary, tests, ago,
command}` is `{card.operation, card.summary, card.tests, card.ago, covering}`): `tests` are the
ones the result lists (the session's own first, at most 12), `more` how many it does not list,
`ago` the time from the run to the answer, `ran_at` the journal's own time. Every value is
sanitised as the result is.

or nothing (no longer fresh, already decided, control arm, not a Python test command). On
nothing the mod continues to its rewrite path (A1's `verdict: { engine }`), which still decides
the call if it was not decided. The mod returns `{ result: { stdout: receipt.result, stderr: "",
interrupted: false }, context: [receipt.note] }` and never calls `next`.

**Spool record** (the final ask, once):

```json
{"ts":…,"event":"PreToolUse","kind":"pretool","arm":"treated","propensity":1.0,"repo":"<key>",
 "answered":"receipt","receipt":true,"checked_by":"engine","command":"<covering>","card":{…as above…},
 "tool_use_id":"toolu_…",
 "decision":{"directives":[],"would_have":[],"judgements":[{"kind":"obligation_met","route":"…"}],
             "observed":["exec .venv/bin/python -m pytest …"],"continue_reason":null},"duration_us":…}
```

(control: no `decision`, `withheld: true`). `rebuild` reads its `obligation_met`, so a rebuilt
state keeps the finish line met. `adage graph --json` events carry `"answered":"receipt"`;
`demo/adage_metrics.py`'s `receipts_as_results` counts these records.

**What the mod draws** (wave 3): the `ToolResult` receipt card only when it answered; a receipt
not fresh draws one dim line beside the real run (`result.receipt.beside`, T-C3-07) and `next(e)`.

### Departures

1. "adage-exec treats the receipt's use as Adage's own run for the duplicate-run rule": no new
   code. The Stop gate already never reruns a check whose passing run is on the current inputs,
   and an answered receipt is fresh by construction; the finish line it covers is met by
   `answered_by_receipt`.
2. The obligation is met by the covering command's key too, not only the call's (the call may be
   narrower than the run, e.g. one node id), because the run that passed is the covering one.
3. The acceptance's fixture is the in-process pipeline (`adage_hook::handle` with adage-exec's
   real extension) over a journaled receipt, not the `zero_config_plugin.rs` Python fixture with
   a sandboxed run: no sandbox backend runs in this environment (`bwrap` is absent), so a run
   cannot be journaled by executing it.

### Live-only (the mod must verify)

- Whether `PostToolBatch` / `PostToolUse` fire for a call the mod answered with `{ result }`
  (with the model's own `tool_use_id`). If they do, the batch observes the same command again: a
  pending directive is already retired and the obligation already met, so nothing double counts,
  but the call is judged as the agent's; if they do not, nothing is lost (the verdict recorded it).
- T-C3-06: `$.tool.check` against `pretool::keeps_permissions` on the five settings fixtures, and
  the `--allowedTools` case.

## C7 · Chains and multi-call steps

### What was built

- **A chain's second step may follow a prediction at once** (`predict_step`): the step after a
  predicted step (the same turn, `index - 1`) is refused unless the found prediction is a read
  batch bound from the result of the step Adage made (`PendingReads.after_adage`), it is not a
  turn's end, and the previous predicted step was not itself such a continuation
  (`SessionState.chained`). So: lookup at step k, the reads at k+1, then a model step before
  anything else (a third step in a row is never predicted, whatever is due).
- **`after_adage`** is set when the read batch is bound from a call that is Adage's (`by: Adage`),
  or, in an arm that only records, the agent's call of a route Adage would have predicted (a
  pending directive in `predicted_routes` that the call follows). So the control arm records
  `would_predict` for the chain at the same step as the treated arm predicts it.
- **One judgement per response**: the reads Adage made retire the batch with one `predicted`
  judgement, whatever the number of calls (unchanged from P1, now pinned by a test).
- **The bound counts responses**: a chain's response is one prediction against the session cap
  (`predictions`), whatever its calls.
- **A read batch takes a `predict` grant** (`adage-exec`): `route::plan_predict` keys a
  `Step::ReadBatch` as `argv = ["Read", "{<slot>}"]`, the slot under the repository-path
  contract, no scripts, the route's effect; `route::predict_classified` classifies it observe
  (no program runs), used by `adage review delegate … --predict` and `predict_grants`. Before, a
  read batch was refused ("only a route that runs a command can be predicted").
- A rebuilt state never chains (read batches are not rebuilt); `chained` is not rebuilt either.

### Contract

`TurnStep` stdin as in P1 (`session_id`, `cwd`, `hook_event_name`, `turn_id`, `index`, `model`?,
`agent_id`?, `ending`?, `observe`?, `routine_model`?), plus `"via": "mod"`. A chain's answer:

```json
{"predict":{"kind":"chain","route":"read-linked-files","track":"acme-billing/learned",
            "calls":[{"tool":"Read","input":{"file_path":"/abs/repo/schema/invoice.json"}},
                     {"tool":"Read","input":{"file_path":"/abs/repo/acme/billing.py"}}]}}
```

The mod yields, per call `k = 0..n-1`, `{kind:'tool', index:k, id, name}` and `{kind:'input',
index:k, json}`, then one `{kind:'stop', stopReason:'tool_use', usage:null}`; ids
`adage_<route>_<turnId>_<index>_<k>` with `_` inside a part written `-`. The calls come back in
one `PostToolBatch` with those ids. Spool record (`kind: "turn_step"`):

```json
{"ts":…,"event":"TurnStep","kind":"turn_step","arm":"treated","propensity":1.0,"repo":"…","generation":"…",
 "decision":{"predicted":{…the prediction…},"would_predict":{…},"predicted_at":["<turn>",2],…},"duration_us":…}
```

In control: no `predicted`, the same `would_predict` and `predicted_at`.

### Departures

1. **"Never two in a row" is relaxed for exactly one case.** The architecture says "never two
   predictions without a model step between them per route"; the plan's C1 rule was "never two
   predicted steps in a row", and T-C7-04 wants the lookup and the reads both predicted (requests
   on lookup items fall by two). The rule built: two in a row only for a chain's second step bound
   from Adage's own step, never three. Adage still cannot drive a turn alone.
2. "A `ReadBatch` as one response of several calls beyond that binding": a read batch is bound
   only from a command's result (`ResultField`, or the `*paths` pseudo-field of plain text, as
   before); the work beyond the POC is the in-a-row rule, the control arm's mirror, the predict
   grant for a read batch, and the sim. No new binding source was added (`io::validate` admits
   none).

### Live-only

T-C7-03 (several chunks in one response, `toolUses` listing them all) and that Claude Code runs
several yielded Read calls of one response; T-C7-04's demo half (`requests` falls by two).

## C8 · The finish line at the last step

### What was built

- **Once per arming**: when a met finish line is armed again by a new edit, its route leaves
  `predicted_routes`, so the next turn's end may predict it again. A finish line whose predicted
  call failed stays unmet and is not predicted again for that arming; the Stop request asks for it
  as without the mod (P1 open issue 6, closed).
- Everything else was built with P1: `ending: true` predicts an armed, unmet obligation under the
  grant; Adage's call (`adage_` id) meets it (`obligation_met` and `predicted`); the Stop gate
  then asks nothing.

### The `ending` index rule (what the mod sends)

Before request `n` (`n > 0`) the mod asks `{index: n, ending: false}`. When the model's response
to request `n` ends the turn (`end_turn`, no `tool` chunk), it asks again `{index: n, ending:
true}`: a prediction there is "in a row" only with a prediction at the same `n`, which cannot
exist (request `n` was the model's). The mod then keeps the model's text chunks, yields the calls
at block indexes after the model's last block, and the model's stop chunk with `stopReason:
'tool_use'` (its `usage` kept).

### Live-only (Q15)

That the engine records the rewritten stop (`tool_use`) from the mod's chunk, continues the loop,
and that the transcript on disk keeps the `adage_` id; 10 of 10 runs keep the text, run the call,
continue and end with no `block` (T-C8-02, T-C8-04's live half).

## D2 · `noticed` at hook time

### What was built

- `SessionState.cand_ctx: CtxState` (a second ring over `Index::candidate_contexts(repo)`, its own
  digest, `align`ed on push) and `SessionState.noticed: Vec<String>` (once per session, at most
  `MAX_NOTICED` = 64). Both `#[serde(default, skip_serializing_if = …)]`.
- `decide()` builds a second `Symbols` over the candidate net (only when non-empty, with the
  policy's `exact_keys`), symbolises every batch call (edit, act, fail, with the same
  "failure reported again" rule as the routes' ring) and every failure into `cand_ctx`, and after
  each push walks the candidate net (`notice`): each id not yet noticed becomes
  `Noticed { candidate, support, s_min, review }` on `Decision.noticed`, in both arms, never a
  directive, never in `would_have`.
- `Decision.cand_ctx: Option<CtxState>` when the event changed it; `rebuild` restores `cand_ctx`
  (dropped when the candidate digest moved) and `noticed`.
- The hook's no-routes path: `respond` resolves the repository against the routes' repositories
  **and** the candidates', so a repository with candidates and no reflexes enters the match path.
  The fast path stays exact: candidate words and globs are in the triggers (D1).
- Consumers: `ui/feed.rs` `Act::Noticed { candidate, support, s_min, review }` (drawn as `○ noticed
  <candidate> · seen in N earlier sessions[ · waits for your review]`); `watch --follow --json`
  emits `{"act":"noticed","candidate","support","s_min","review"}` and keeps it in a control
  session's own stream; `adage graph --json` events carry `"noticed":["<id>",…]`; savings ignore
  it. The replay gate (`adage_eval::replay`) requires a skipped event's `noticed` to be empty and
  compares `cand_ctx` and `noticed` in the state.

### Contract

In the spool record's `decision`: `"noticed": [{"candidate":"run-a54d7c","support":2,"s_min":3,"review":false}]`
(absent when empty), `"cand_ctx": {…}` (absent when unchanged). Nothing in `additionalContext`.
The mod reads it from the feed (`watch --follow --json`), never asks the binary for it.

### L5 (T-D2-06), on debug builds, reported as such

See "Commands run" for the numbers (`hook_latency` gained `batch_no_match_with_candidates` and
`batch_notices`). Debug builds on a shared machine; not the paired release comparison the
acceptance states.

## C9 · Attachment and the mod's capabilities (binary side)

### What was built

- **`Harness::capabilities_with_mod()`** (adage-trace): Claude Code's four plus `predict_step`,
  `answer_call`, `substitute_call`, `tool_description`, `cached_section`, `subagent_prompt`,
  `compaction`; Codex and OpenCode have no mod (their set unchanged).
- **`Adapter::capabilities(event, payload)`** (adage-hook): the harness's set by default; the
  Claude adapter reports the mod's set for a payload the mod sent: `via: "mod"`, an event only
  the mod sends (`TurnStep`, `Standing`, `SkillPrompt`), or a `PreToolUse` with `dry_run` or
  `verdict`. Behaviour reads it: a `TurnStep` is acted on only with `predict_step`; the plan's
  `substitute` needs `substitute_call`, a receipt's `result` `answer_call`; `Standing` answers
  `standing` only with `cached_section`, `environment` only with `tool_description`, `compact`
  only with `compaction`, `subagent` only with `subagent_prompt`
  (`adage_runtime::standing_for`, `adage_runtime::delegation`).
- **`Call.agent: Option<String>`** (adage-trace, `#[serde(default, skip_serializing_if)]`): the
  Claude adapter sets it from the payload's `agent_id` (a batch entry's own `agent_id`, else the
  payload's) on `PostToolBatch`, `PostToolUse` and `PostToolUseFailure`. Every `Call` literal in
  the workspace gained `agent: None`.
- **`Standing` with `agent`**: what a subagent is told before it starts
  (`adage_runtime::delegation`): the prompt reflexes the subagent's own description and prompt
  bind (a lookup bound to the work item it names), not those a told dead end avoids, then the
  standing reflexes, each in its `<adage-reflex>` wrapper, whole items only, at most 2,000 bytes.
  Read-only (no state, no spool); empty in the control arm and in observe mode.

### Contract

```json
stdin:  {"session_id":"…","cwd":"/abs","hook_event_name":"Standing","via":"mod","observe"?:true,
         "agent"?:{"type":"Explore","description":"Look up the work item","prompt":"Find what W-2045 asks for …"}}
stdout: {"standing":"<adage-reflex …>…","environment":"…","compact":"Keep the repository procedures …",
         "subagent":"<adage-reflex … route=\"lookup-work-item\" …>…</adage-reflex>\n\n<adage-reflex …>…"}
```

`subagent` appears only when the payload carries `agent` (so wave one's answer is unchanged
without it); `""` when nothing applies. The first `Standing` keeps the session's standing text
(as in C5), whether or not it carries `agent`, so the mod should ask its session `Standing` once
after `session.start` before any `agent.spawn`.

The mod: `tool.describe { tool: 'Bash' }` appends `environment` when non-empty; `prompt.compose`
appends `{ id: "adage:procedures", text: standing, scope: "session" }`; `agent.spawn` asks
`Standing` with `agent` and prepends `subagent` to the subagent's prompt when non-empty, else
`next(e)`; `session.compact` appends `compact`.

### Departures

1. `Standing` counts as the mod's by its event name, so its answer is the same with or without
   `via` (wave one's callers keep working); `via: "mod"` is the general rule for every other
   event.
2. "`decide()` answers `Standing` for a delegation": a function of its own,
   `adage_runtime::delegation`, not an arm of `decide` (it changes no state and is not an event).

### Live-only

T-C9-03 (the hooks), that `agent_id` reaches the command hooks for a subagent's tool events,
T-C9-04's "makes no lookup of its own", T-C9-05 (directives per session fall in the `mod` arm).

## E2 and E3 · the offline parts

- `evals/reflex-proof/harness/dev_v9.py`: `ARMS["MOD"]` (A4's hooks plus the mod, `mod: True`),
  `DESIGNS` (`dev-v9`: A0, A2f, A4, controls A4, protocol dev-arms-2, the registered design as it
  ran; `dev-v9-mod`: A0, A4, MOD, controls MOD, protocol dev-arms-3, 53 sessions, the same
  budget). `freeze --design dev-v9-mod --predict-route <track/reflex>…` refuses unless a protocol
  revision naming `dev-v9-mod` is registered (none is) and the predict routes do not include the
  E2 route; it stages the mod with its command hooks removed (`stage_mod_plugin`) and freezes its
  file digests. MOD sessions run with `--plugin-dir <study>/plugin`. `approval-script` adds, for
  each MOD home, `adage review delegate <track> <reflex> --predict --repo <checkout> --by
  launch-evaluation-human`; `verify-grants` requires exactly those predict grants in MOD homes
  and none elsewhere. **`dry-run`** prints, spending and writing nothing, the person's step (the
  grants) first, then every dispatch in order with its arm, `mod`, `plugin_dir` and the grants its
  home must hold, and `person_before_positives: true` (T-E2-01's dry run). `summarize(rows,
  thresholds, design)` judges every non-A0 arm with `D1_requests_max_ratio_to_A0`,
  `D1_blocks_fewer_min` and `D4_sessions_with_repeat_attempts_max` read from the frozen
  manifest's `dev_thresholds_v9` and reports them as `rule` (T-E2-02); only the design's control
  arm can be BEST.
- `heldout_cohort.py`: `design()` accepts a valid dev-arms-3 report and returns BEST = MOD with
  `mod: true` and the report's `predict_routes`; a MOD BEST's homes get the predict grants beside
  the E2 grant, its sessions `--plugin-dir`, NOEXEC neither. `heldout_analysis.py`:
  `select_best` defaults to the report's own `candidates`; `analyze(rows, "MOD")` takes rows that
  name BEST by its development arm (T-E3-01's script half).
- `evals/reflex-proof/LAUNCH-STATUS.md`: a "Layer 4: the Claude Code mod (MOD)" section saying no
  run has happened, with the slots E2's two runs and E3's outcome fill.
- No model was called. T-E2-03, T-E3-01's run and T-E3-02 need model runs.

## The simulator

- `scenario::history`: in the first two work-item sessions of every harness's history the agent
  runs `make lint` right after the last `make test` (`LINT_SESSIONS = 2`), drawn from no
  generator, so every other session is byte-identical. Fixtures regenerated (6 files changed).
  Claude's history learns the candidate `[edit acme/*.py, exec make test] → make lint` (support 2,
  `s_min` 3); Codex's learns it after the failed test run (`[fail …unittest…, exec make test]`).
- `scenario::replay`: a look around (`ls`) before the lookup, so the lookup can be predicted at
  step 1 (never at the prompt's own request); `Replay.predict` lists the lookup (`run`, 1), the
  reads (`chain`, 2) and the finish line (`obligation`, the turn's end), each made only for a
  granted route; `Predict.acts` replaces `Predict.act`; `Replay.noticed` (`Notice { step: "make
  lint", at: "make test" }`).
- `claude.rs`: `turn_step_hook` (now `via: "mod"`), `pretool_hook` (the dry run carries `via`),
  `verdict_hook`, `standing_hook` (with `agent`), `press_hook`, `skill_prompt_hook`,
  `subagent_batch` (`agent_id`).
- `harness_sim.rs`: every Claude replay asks `Press` (a 64-hex token once, nothing after the
  agent acted); a predicted step at any boundary replaces the agent's next turn; three grant sets
  under adage-exec (none; the finish line; the lookup, the reads and the finish line), both arms;
  `check_noticed` and `check_graph` count the noticed candidate per arm (Codex: none, it has no
  failure signal); `adage watch --once` shows the row; a new test for `Standing`, a subagent told
  at spawn, its tool batch, and `SkillPrompt` in both arms. The sandbox gains
  `scripts/wi-lookup`, so a predict grant on the lookup hashes the script, as in a real checkout.
- `crates/adage-cli/tests/candidates.rs` uses the history as it is now.
- The `claude-code` golden gains two steps (added lines only, below).

## A3 follow-ups: what the mod asked of the binary (B2-B3-notes, "What the binary does not give the mod yet")

All in crates/adage-cli (and the record writers in adage-hook / adage-exec); plugin/ untouched.

1. **`adage watch --follow --json --stdin ignore`** (the bug: a follower whose parent closes its
   stdin at spawn ended before its first line). `--stdin eof` (the default, as before) ends when a
   piped stdin closes; `--stdin ignore` never reads stdin. Both keep the other two exits: the
   parent's death (`PR_SET_PDEATHSIG`, and a check that its parent pid is unchanged) and SIGTERM (exit 0). Any other value is a
   usage error (`possible values: eof, ignore`). **The wave-3 mod passes `--stdin ignore`** in its
   `$.process.spawn` argv and adds it to the allow-list's `watch` shape:
   `adage watch --follow --json --stdin ignore --session <id> [--since <cursor>]`.
   Test: `mod_surfaces::a_follower_told_to_ignore_stdin_keeps_streaming_with_its_pipe_closed`
   (closed pipe, rows still arrive, SIGTERM ends it with 0; without the flag the closed pipe ends
   it at once).
2. **`adage --json` reports `ui`**: `{"ui":{"spinner":false,"turn_line":false,"noticed":false}}`,
   the config keys `ui.spinner`, `ui.turn_line`, `ui.noticed` (bool, default false, not guarded;
   `adage config set ui.spinner true`). Test: `mod_surfaces::the_card_as_json`. The settings tab
   lists the three keys (snapshot `app-settings-rewrite-marker` re-accepted; `app-palette`'s match
   count moved 15 → 17 with them).
3. **Feed lines name their calls**: a spool record carries `tool_use_id` (a `pretool` record, from
   the payload) or `tool_use_ids` (a tool batch's calls in order, a failure's one call), each only
   when the payload had ids; the follower's decision line carries `"tool_use_ids":[…]` (absent
   when none). A judgement in a batch of several calls still names the batch, not one call: the
   runtime's judgements are per route, not per call. Tests:
   `ui::follow::tests::a_receipt_answer_carries_the_card_and_judgements_name_their_calls`,
   adage-hook `a_call_answered_by_a_fresh_receipt_is_one_record_and_meets_its_finish_line`.
4. **An interrupted action** (ux-design §9.9 / `status.recovery`): the spool already marks it
   (`recovery_required: true`, and `exec: [{"outcome":"unknown","operation_id":"…",…}]`). The feed
   line gains `{"act":"recovery","operation":"op-7","command":"adage-exec recover --id op-7
   --acknowledge"}` (`operation` and `command` null when the journal itself could not be read),
   kept in either arm, drawn `⚠ an action was interrupted; inspect the worktree, then run … in a
   terminal`. The card: `adage --json` carries `"recovery":{"operation","route","command"}` from
   the execution journal for the current worktree (`null` when nothing waits; all three null
   when the journal is unreadable). Tests: `ui::follow::tests::an_interrupted_action_is_reported_in_either_arm`,
   `mod_surfaces::the_card_as_json`.
5. **The receipt card's feed line**: a call answered by a receipt is
   `{"act":"answered","receipt":"<operation>","command":"<covering>","summary":"…","tests":[…],
   "more":0,"ran_at":"…","ago":"4 min ago"}` beside its `met` acts, with `tool_use_ids:
   ["<the call>"]`; treated only (in control the call ran, and the record is withheld). Drawn
   `✓ answered from its run of <command> · ran 4 min ago`.

Not done: departure 5 of B2-B3 (a scrubbed control row is dropped, not sent quiet) is unchanged.

## Golden diff (`ADAGE_BLESS=1 cargo test -p adage-cli --test hooks claude_code_golden`)

```diff
@@ -28,6 +28,12 @@
 ### UserPromptSubmit
 {"hookSpecificOutput":{…"route=\"commit-review\"…}}
 
+### PreToolUse
+{"plan":{"substitute":{"command":"make test","context":"adage ran make test in place of python3 -m pytest tests: in this repository it fails (\"ACME_ENV is not set\").","route":"dead-pytest","track":"acme/work"}}}
+
+### Standing
+{"compact":"Keep the repository procedures …","environment":"","standing":"<adage-reflex … route=\"dead-unittest\" …>…</adage-reflex>","subagent":"<adage-reflex … route=\"lookup\" …>\nIn this repository, work item details are looked up with:\n  ./scripts/wi-lookup W-2042 --json\n…</adage-reflex>\n\n<adage-reflex … route=\"dead-unittest\" …>…</adage-reflex>"}
+
 ### SessionEnd
 (nothing)
```

## Tests, by test-plan id

| Id | Where |
| --- | --- |
| T-C3-01 | adage-exec `hook::prediction_tests::pretool_context_needs_a_fresh_successful_receipt_in_this_session_covering_the_call`, `a_fresh_receipts_result_names_the_tests_it_ran_the_sessions_own_first`, `a_receipt_never_covers_a_call_from_elsewhere` |
| T-C3-02, T-C3-03 | adage-exec `a_fresh_receipt_answers_the_repeated_test_run_and_meets_the_finish_line` (the real extension through `adage_hook::handle`); adage-hook `a_call_answered_by_a_fresh_receipt_is_one_record_and_meets_its_finish_line`, `a_receipt_is_never_the_result_in_control`, `pretool_receipt_context_reaches_a_python_test_call_without_any_rewrite`; adage-runtime `a_call_answered_by_a_receipt_meets_what_it_covers`; the golden (unchanged for command hooks) |
| T-C3-07 (L2 half) | the not-fresh plan and the silent command hook, in the tests above |
| T-C3-04, 05, 06 | the mod (L7) and live (L9): wave 3 |
| T-C7-01, T-C7-02 | adage-runtime `a_chain_reads_what_adages_lookup_bound_at_the_next_step_and_never_a_third`, `only_adages_own_lookup_chains_into_its_reads`, `the_control_arm_records_the_chain_the_treated_arm_makes`, `turn_step_predicts_a_chain_of_reads_bound_from_the_lookup` |
| T-C7-04 (sim) | harness_sim `claude_code_predicts_under_adage_exec` (the lookup, then the reads, granted) |
| T-C8-01 | adage-runtime `the_finish_line_is_predicted_once_per_arming_and_the_stop_gate_stays_the_fallback`, `turn_step_predicts_a_delivered_directive`; adage-hook `turn_step_predicts_the_granted_finish_line` |
| T-C8-03 | adage-hook `a_finish_line_whose_predicted_call_failed_falls_back_to_the_stop_request`; the runtime test above |
| T-C8-04 (sim) | harness_sim `claude_code_predicts_under_adage_exec` (no Stop request on the finish-line turn) |
| T-C9-01 | adage-trace `only_claude_code_has_a_mod_and_it_adds_the_seven`, `a_calls_agent_defaults_to_the_main_loop_and_is_omitted_there`, `the_mods_capabilities_are_unset_for_every_harness_and_round_trip`; adage-hook `the_mods_payloads_report_the_mods_capabilities` |
| T-C9-02 | adage-runtime `what_is_attached_follows_the_harness_capabilities` |
| T-C9-03 (binary half), T-C9-04 (sim) | adage-hook `a_subagent_is_told_what_its_prompt_binds_in_the_treated_arm_only`; harness_sim `claude_code_standing_and_a_subagent_told_at_spawn` |
| T-D2-01, T-D2-02 | adage-runtime `a_candidate_is_noticed_once_never_directed_and_alike_in_both_arms` (with the fast path checked on every event) |
| T-D2-03 | adage-runtime `candidates_have_their_own_bounded_ring` |
| T-D2-04 | adage-runtime `noticed_and_the_candidate_ring_are_rebuilt_from_the_spool`; adage-hook `a_repository_with_candidates_and_no_routes_notices_them` |
| T-D2-05 | harness_sim `check_noticed`, `check_graph_predicting`, `claude_code_learn_then_replay` (`adage watch --once`); adage-cli `ui::follow::tests::a_noticed_candidate_is_shown_alike_in_both_arms` |
| T-D2-06 | `hook_latency` (debug, below) |
| T-D2-07 | `cargo test -p adage-eval --test gates` (`fast_path_changes_no_decision` with `noticed`) |
| T-E2-01 (dry run), T-E2-02 | `evals/reflex-proof/harness/test_dev_v9.py` class `Mod` |
| T-E3-01 (script half) | `test_heldout_cohort.py` `test_a_dev_v9_mod_report_selects_mod_with_its_predict_routes`, `test_a_mod_best_gets_the_mod_and_its_predict_grants_and_noexec_neither`; `test_heldout_analysis.py` class `Mod` |
| T-X-12 | `cargo test -p adage-exec --test parity` |

## Files touched

```
crates/adage-trace/src/{lib.rs,harness.rs}
crates/adage-runtime/src/{lib.rs,fast.rs,rebuild.rs,tests.rs,skill_tests.rs}
crates/adage-hook/src/{lib.rs,harness/mod.rs,harness/claude.rs}
crates/adage-exec/src/{hook.rs,route.rs}
crates/adage-exec/src/time.rs
crates/adage-store/src/config.rs (ui.spinner, ui.turn_line, ui.noticed)
crates/adage-cli/src/{exec.rs,cli.rs,status_card.rs,ui/mod.rs,ui/feed.rs,ui/follow.rs,ui/view.rs}
crates/adage-cli/src/ui/snapshots/{adage__ui__tests__app-palette,adage__ui__tests__app-settings-rewrite-marker}.snap
crates/adage-cli/examples/hook_latency.rs
crates/adage-cli/tests/{harness_sim.rs,hooks.rs,candidates.rs,mod_surfaces.rs,golden/claude-code.txt}
crates/adage-cli/tests/snapshots/mod_surfaces__card-json.snap
crates/adage-learn/tests/golden/codex-session.txt (one added line: the sim's `make lint` turn)
crates/adage-app/src/graph.rs
crates/adage-eval/src/replay.rs
crates/adage-sim/src/{scenario.rs,claude.rs}
sim/fixtures/ (regenerated: 2 Claude transcripts, 2 Codex rollouts, 2 OpenCode exports)
Call literals only (`agent: None`): crates/adage-ingest/src/{claude,codex,opencode}.rs,
  crates/adage-learn/src/{claims,effects,graph}.rs, crates/adage-learn/tests/routine_execution.rs,
  crates/adage-graph/src/canonical.rs, crates/adage-tracks/src/matching.rs,
  crates/adage-eval/src/{transcript,savings_replay}.rs, crates/adage-eval/tests/{prop_tracks,predict_gate}.rs,
  crates/adage-hook/src/harness/mod.rs
evals/reflex-proof/harness/{dev_v9.py,heldout_cohort.py,heldout_analysis.py,test_dev_v9.py,
  test_heldout_cohort.py,test_heldout_analysis.py}
evals/reflex-proof/LAUNCH-STATUS.md
crates/adage-eval/tests/predict_gate.rs (a chain scored against the agent's next reads)
docs/how-it-works.md, docs/execution.md, docs/harnesses.md, docs/live-learning.md, CHANGELOG.md
docs/claude-mod/impl/C3-C7-C8-D2-C9-E-notes.md
```

## Commands run (in /home/user/adage-poc)

```text
cargo fmt --all
cargo clippy --workspace --all-targets                        clean
cargo test -p adage-trace -p adage-ingest -p adage-runtime -p adage-hook -p adage-exec -p adage-app \
           -p adage-sim -p adage-tracks -p adage-graph -p adage-learn -p adage-store --no-fail-fast
                                                              all pass (runtime 112, hook 49, exec 111)
cargo test -p adage-cli --no-fail-fast                        all pass (bin 122; harness_sim 10; hooks;
                                                              mod_surfaces 11 + 1 ignored L5)
cargo test -p adage-exec --test parity                        22 passed
cargo test -p xtask                                           18 passed
cargo xtask test python demo reflex-proof                     demo 36 OK (4 skipped), reflex-proof OK
cargo test -p adage-eval --test gates                         15 passed (517 s)
cargo test -p adage-eval --test parity                        3 passed (1,126 s)
cargo test -p adage-eval --test predict_gate                  passed, after a scoring fix (below)
ADAGE_BLESS=1 cargo test -p adage-cli --test hooks claude_code_golden     added lines only (above)
ADAGE_BLESS=1 cargo test -p adage-learn --test codex          one added line (`turn 6 exec make lint`)
target/debug/examples/hook_latency target/debug/adage - 100   (debug build, shared machine)
```

**predict_gate.** With C7 a chain of reads follows Adage's lookup at the very next step; the gate
scored a chain right only when the agent's *next batch* read all its files, and the recorded
agents read them one file per request, so synthetic fell to 12/29 (floor 0.48). The scoring now
takes the agent's next reads (its next batch with a Read and the all-Read batches right after it).
Floors unchanged; measured: demo 34 steps for 26 opportunities, 7/34 (floor 0.1565); synthetic 30
for 19, 22/29 = 0.7586 (floor 0.48). The gate's opportunity count still follows "never two steps
in a row", so it reads lower than before (25 → 19 on synthetic) now that a chain may follow at
once; the `predicted >= opportunities` check still holds.

**hook_latency (debug, p50 / p95 ms, 100 runs):** batch_no_match 4.85 / 5.79;
batch_no_match_with_candidates 5.21 / 6.21; batch_notices 27.64 / 36.10 (noticed: true);
batch_context_fires 27.72 / 35.00; pretool_no_match 4.07 / 4.88; turn_step_nothing_pending
5.08 / 6.03. The candidate net adds about 0.4 ms at p50 to the no-match path on a debug build.

