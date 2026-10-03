# P4b · `TurnStep` in the simulator, both arms in `harness_sim`, the offline prediction gate (POC of C4's sim half and E2's offline half)

The evaluation half of chunk C4 (`../implementation-plan.md` §2.7 items 1 and 3, T-C4-04) and an
offline gate for predicted steps. Nothing here runs a model. The sim drives the real `adage` and
`adage-exec` binaries. The gate drives `adage_runtime::decide` over recorded sessions.

## What was built

| File | What |
| --- | --- |
| `crates/adage-sim/src/claude.rs` | `turn_step_hook(sid, cwd, turn_id, index, ending)`: the payload the mod sends (P1's contract: `session_id, cwd, hook_event_name, turn_id, index, model, ending`, with no transcript path or permission mode, since the mod builds it from `$.session`). `pretool_hook(…, dry_run)`: the mod's two-phase `PreToolUse` for a Bash call. `predicted_tool_use_id`: the mod's `adage_<route>_<turn>_<index>_<k>`, with `_` written `-` inside a part (P3 issue 8). `predicted_batch`: the `PostToolBatch` that brings a predicted step back. `tool_use_id(rng)`: `turn_hooks` now draws its ids through it, so a clone of its `rng` gives the ids a `PreToolUse` sent before the batch carries. |
| `crates/adage-sim/src/scenario.rs` | `Replay.predict: Vec<Predict { index, ending, kind, route, act }>` and `Replay.pretool: Vec<Pretool { command, answer }>`, filled by `predict_expectations` and `pretool_expectations`. The held-out W-2045 session predicts one step: `obligation` on `finish-line-` with `make gen`, at the work turn's end (`index` = 6, `ending: true`). Every Bash call's `PreToolUse` answers nothing in both phases, except the dead end, which P5's `substitutions()` covers. |
| `crates/adage-cli/tests/harness_sim.rs` | `replay_claude` is now `replay_claude_with(sb, sid, arm, Hook)`, where `Hook` is `Adage` or `Exec { bin, granted }`. Every replay now sends `TurnStep` at each step boundary and the two-phase `PreToolUse` before each Bash call, and checks the session's `turn_step` spool records (`check_turn_steps`). `check_graph_predicting` also counts `predicted` and `would_predict` timeline events, the `predicted` judgement and stop requests. New test `claude_code_predicts_under_adage_exec` (T-C4-04): learn, then both arms without the grant, then both arms with it. `grant_predict` issues the grant with `adage review delegate <track> <route> --predict --repo …` under `ADAGE_ALLOW_NONINTERACTIVE=1` and `[exec] max_level = "E2"`. Hooks under `adage-exec` run with the checkout as their working directory (`hook_in_repo`), as a harness runs them, because adage-exec checks a predict grant against the checkout it is asked from. |
| `crates/adage-eval/tests/predict_gate.rs` (new) | The offline prediction gate (below). |
| `docs/testing.md` | `predict_gate` added to the eval-gates row. |

`demo/adage_metrics.py` needed no change. `is_predicted_step` reads `kind: "turn_step"` with
`decision.predicted`, which is what the binary spools and what `check_turn_steps` asserts. It
returns true for a treated record and false for a control record that has only `would_predict`
(checked by hand on both shapes). The `requests` metric comes from the debug log and is independent
of the spool.

### Step boundaries, as the replay sends them

After the prompt the replay sends `TurnStep 0`. After the k-th tool batch it sends `TurnStep k`.
When the model's response to request `n` ends the turn, it sends `TurnStep n, ending: true`, and
then the Stop hook. A Stop `block` makes request `n+1`, whose `TurnStep n+1` comes before the
continuation's first batch.

### The sim's arms (T-C4-04)

| Hook | Grant | Treated | Control |
| --- | --- | --- | --- |
| `adage` | (none possible) | every step silent; no `turn_step` record; Stop asks for `make gen` | every step silent; no record |
| `adage-exec` | none | the same as `adage` | the same as `adage` |
| `adage-exec` | `predict` on `finish-line-…` | `TurnStep 6 ending` prints `{"predict":{"kind":"obligation","route":"finish-line-…","track":…,"calls":[{"tool":"Bash","input":{"command":"make gen"}}]}}`; its `PreToolUse` checks pass with the `adage_` id; the `PostToolBatch` with that id prints nothing and is judged `predicted` and `obligation_met`; `TurnStep 7` is silent (never two in a row); the agent's own `make gen` is not replayed; Stop prints nothing (no request) | `TurnStep 6 ending` prints nothing; one `turn_step` record with `would_predict` and `predicted_at: [turn, 6]`, and no `predicted`; Stop is silent; the agent's `make gen` meets the obligation |

In the granted run `adage graph --json` shows 1 `predicted` event (treated), 1 `would_predict`
event in each arm, 1 `predicted` judgement and 0 stop requests. The ungranted runs show 0, 0, 0
and 1.

## The offline gate: `crates/adage-eval/tests/predict_gate.rs`

The gate replays each unsteered test session of a dataset through `decide` with a `TurnStep`
injected at every step boundary: 0 after a prompt, `k` after the turn's k-th batch, and `ending`
before the stop. It runs both arms, with every learned route that is not exec-only holding a
`predict` grant. In the treated arm, Adage's calls are put into the stream as a batch of `by:
adage` calls. The agent's own later call of the same thing is not replayed, and its result
becomes the result of Adage's call. A call the agent never made gets an empty result.

The opportunities are counted apart from the runtime. A step is an opportunity when all of these
hold:

- a directive was given with its command bound (pending run, sequence or skill; bound read batch;
  or, at `ending`, an unmet obligation);
- the step's index is greater than 0;
- the step does not come right after a counted one;
- fewer than 6 have been counted in the session;
- the route has not been counted before.

The gate asserts:

- **requests saved** (the treated arm's predicted steps) ≥ the opportunities in the treated
  stream;
- 0 predicted in the control arm, and no `predicted` judgement there;
- every step Adage made is judged `predicted` when it comes back;
- the hook's fast path never skips a step whose full decision would predict;
- accuracy is at least a floor set just under the measured value.

**Measured** (`cargo test -p adage-eval --test predict_gate -- --nocapture`):

| Dataset / split | Sessions | Steps | Opportunities (as recorded) | Requests saved, treated | Control | Accuracy: next action | Bash only | Agent made it later | Finish-line at turn end |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `demo-haiku45` round2 | 18 | 447 | 34 (35) | **34** | 0 (would 35) | **0.1765** (6/34) | 0.1364 (3/22) | 0.6765 (23/34) | 0 |
| `synthetic` hash:0.5 | 18 | 166 | 25 (25) | **25** | 0 (would 25) | **0.5000** (12/24) | 0.5333 (8/15) | 0.8333 (20/24) | 1 |

How to read the accuracy. The scored sessions are the unsteered ones: agents that never got a
directive and found the lookup script on their own, several steps after the step where Adage
would have run it. So strict next-action accuracy is low on `demo-haiku45`. Most of its steps
are the `lookup-work-item` run or the `read-linked-files` chain bound from it (a debug run listed
them), and both come earlier than the recorded agent's next action. "Made later" (0.68) measures how often
the step was one the agent did take. Once Adage acts early, the rest of the recording is no
longer what that agent would have done next. That counterfactual is outside what an offline
replay can show. The live `mod` arm (T-C4-06, E2) is where it gets measured. The floors are 0.1765
− 0.02 and 0.5 − 0.02.

There is one fewer opportunity in the treated `demo-haiku45` stream than as recorded. In session
`d9767f7f…` the sequence `make gen then make test then make ci` is predicted at step 28, and its
first call meets the finish line's obligation. The recorded stream would instead have had a
finish-line opportunity at the turn's end. "Judged predicted" (38) exceeds the 34 steps because a
call can stand for two routes: the obligation carried by the sequence's first step.

## How this differs from the plan

1. **The grant is issued through the CLI, not by writing `approvals.json` by hand.** The predict
   key carries the route's exec digest and script hashes (P2's `plan_predict`), and the hook
   rechecks it against the checkout. Hand-writing it would copy those internals into the test.
   `adage review delegate --predict` is the person's path, so the test also covers it. The test
   is not `#[ignore]`d because C2 had landed by the time it was written.
2. **The prediction in the sim is at the turn's end, not at the schema-edit turn.** The only
   predictable directive the W-2045 replay leaves pending after the schema edit is the finish
   line's obligation, and an obligation is predicted only at `ending`. The edit arms it, and the
   step comes when the model stops without `make gen`. No `run` directive is pending then: the
   lookup is followed at turn 1. Granting `read-linked-files` would add a `chain` at index 1. That
   was left out to keep the replay's later turns (the agent's own reads) as recorded.
3. **`dry_run`.** P5 added the two-phase path to adage-hook while this was being built. For every
   Bash call but the dead end, the replay asserts that the dry run's answer equals the final
   ask's, and that both are empty. The dead end's two phases are P5's `check_substitute`, inside
   the same closure. A predicted call goes through the same `PreToolUse` checks, with its
   `adage_` id.
4. **The plan's sub-support `make lint` sequence** in `scenario::history` (§2.7.1) is not added.
   It belongs to the learner's candidates (D1), and adding it would change every fixture.
5. **The gate is its own test file**, not an extension of `fast_path_changes_no_decision` in
   `gates.rs` (§2.7.3). It does check the fast path on every injected `TurnStep`, but only for the
   two datasets above, not all six runs that test covers.

## Open issues

1. **`adage-exec` must be built beside `adage`.** `cargo test -p adage-cli` builds only `adage`.
   `claude_code_predicts_under_adage_exec` uses `target/<profile>/adage-exec` and skips with a
   message when it is missing, as `tests/actions.rs` does. A stale binary would test old code: CI
   should run `cargo build -p adage-exec` first, or run the workspace tests.
2. **The `adage_` id parts.** `predicted_tool_use_id` mirrors `predict.ts::toolUseId`. If the mod's
   spelling changes, the sim's must follow (only the prefix matters to the binary).
3. **Turn id after a Stop block.** The replay keeps one `turn_id` and continues the index after a
   Stop `block` (request `n+1`). Whether Claude Code's `turn.step` starts a new turn there is
   unverified (P3: nothing ran live).
4. **The gate's counterfactual** (above): strict accuracy understates a live agent that was given
   the prediction's result. A wrong prediction's result is empty in the replay, though live it
   would be the command's real output.
5. **The ending index the mod sends** is the index of the request that just ended the turn (P3).
   The sim follows that. If the mod changes it, both `turn_step_hook` callers and the binary's
   "twice in a row" rule must move together.
6. `docs/claude-mod/poc/README.md` still lists the P4b rows as pending.

## Commands run (in /home/user/adage-poc)

| Command | Result |
| --- | --- |
| `cargo run -p adage-sim` | wrote 36 files, `git status sim/` clean (no fixture drift: hook payloads are built at test time, not committed) |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy -p adage-sim -p adage-cli -p adage-eval --all-targets -- -D warnings` | clean |
| `cargo test -p adage-sim` | 3 + 2 passed (lib incl. P5's `the_dead_end_turn_is_the_one_substitution`, fixtures) |
| `cargo build -p adage-exec -p adage-cli` then `cargo test -p adage-cli --test harness_sim` | 9 passed, including `claude_code_predicts_under_adage_exec` (ran, not skipped) and `claude_code_learn_then_replay` |
| `cargo test -p adage-eval --test predict_gate -- --nocapture` | 1 passed, about 81 s; numbers above |
| `cargo test -p xtask` | 18 passed |
| `cargo xtask test python demo` | 35 tests, OK, 4 skipped (the replay tests that need `ADAGE_BIN`) |

The rest of `cargo test -p adage-eval` was not run (g4 takes about 40 minutes). No golden was
re-blessed: no hooks golden step was added.
