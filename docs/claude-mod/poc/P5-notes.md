# P5 · The dead end that never runs (POC of chunk C6), observe mode, and the skill check

A learned `avoid` pair is substituted before the dead end runs (architecture §3.4, plan C6): when
a Bash call is a dead end this repository's sessions learned, the mod runs the learned command in
its place, only when Claude Code's permission engine decides both commands alike. The failure, the
model's reading of it and the recovery request do not happen. Two gaps in the P3 mod are closed
as well: observe mode (`ADAGE_MOD_OBSERVE`) and the `$.command.list()` check before a `Skill` call
is made in the model's place.

Built against Claude Code 2.1.288. Nothing here ran in a live interactive session.

## The JSON contract (shared with the mod)

C3 (receipts, rewrites checked by the engine) is not in this POC, so `pretool()` had no `dry_run`
or `verdict` phase. P5 builds the minimal two-phase shape the plan describes, for the
substitution only. A payload with either field takes the mod's path (`pretool_mod`) before the
Python word scan, so a command hook's payload, which never carries them, is decided exactly as
before (the `hooks` golden is unchanged).

**Phase 1, the plan.** `<bin> hook claude PreToolUse`, stdin:

```json
{ "session_id": "…", "cwd": "/abs/path", "hook_event_name": "PreToolUse", "tool_name": "Bash",
  "tool_input": { "command": "python3 -m unittest discover -s tests" }, "tool_use_id": "toolu_…",
  "dry_run": true, "observe": true }
```

`observe` is present only in observe mode. stdout is nothing, or one line:

```json
{"plan":{"substitute":{"command":"make test","context":"adage ran make test in place of python3 -m unittest discover -s tests: in this repository it fails (\"RuntimeError: ACME_ENV is not set — run tests with 'make test'\").","route":"dead-end-d41b1e","track":"acme-billing/learned"}}}
```

The dry run answers in either arm (the mod never knows the arm), changes no state and spools
nothing.

**Phase 2, the verdict.** Sent only when both `$.tool.check` decisions are equal:

```json
{ …the same envelope…, "verdict": { "engine": "same", "substitute": "dead-end-d41b1e" }, "observe": true }
```

The binary finds the substitution again; it must name the same route (the index may have moved
since the plan). Then it spools one `pretool` record:

| Arm | stdout | spool record |
| --- | --- | --- |
| treated | `{"substitute":{command, context, route, track}}` | `{ts, event: "PreToolUse", kind: "pretool", arm, propensity, repo, checked_by: "engine", substituted: true, decision}`, where `decision.judgements = [{kind: "substituted", route}]` and `decision.substituted` = `decision.would_substitute` = the substitution |
| control, or `observe: true` | nothing | the same, without `substituted`, with `would_substitute: true, withheld: true`, `decision.judgements = []` and only `decision.would_substitute` |

Any other `verdict` (`engine` not `same`, or another route) prints nothing and records nothing.
`demo/adage_metrics.py`'s `is_substitution` counts the treated record by its `substituted`
judgement or by `substituted: true`, and never a withheld record, as P4a wrote it.

**Which calls.** `adage_runtime::substitution(index, repo, policy, table, command)`:

- one plain command: no `| ; & < > $ \` ( )` or newline, and one canonical action;
- the first route in index order that is in play, learned (not an instinct), `firm`, with no
  slots, and either
  - an `avoid` step whose runners the command runs (`pattern::directs_avoided`), or
  - a run step after a failure of the command's key (`failure_matches`) delivered at `fire`;
- whose `instead` is non-empty, has no `{slot}`, is not itself avoided by the route, and is not
  the call (`pattern::same_command`);
- off with `adage delegate rewrites off` (`runtime.rewrite`), like the interpreter rewrites.

The context line: `adage ran <instead> in place of <command>: in this repository it fails
("<signature>").`, every field through `render::sanitise`.

## What was built

| File | What |
| --- | --- |
| `crates/adage-runtime/src/lib.rs` | `Substitution { route, track, command, context }`; `Judgement::Substituted { route }`; `Decision.substituted`, `Decision.would_substitute` (omitted when `None`, so every older record and golden keeps its bytes); `Policy.observe`; `substitution()` (above); `substitute(arm, observe, sub) -> Decision`; `predict_step` records only `would_predict` when `observe` |
| `crates/adage-runtime/src/tests.rs` | T-C6-01 (`a_learned_dead_end_is_substituted_only_from_a_firm_fixed_pair`: firm avoid, after-failure at fire, prime, forming, dormant, slotted, empty, instinct, list, pipeline), T-C6-02's runtime half, observe at a `TurnStep` |
| `crates/adage-hook/src/lib.rs` | `pretool_mod` (the two phases above), `observed(p)`; `respond()` sets `Policy.observe` from a `TurnStep` payload's `observe`; in-process tests of both phases in both arms and in observe mode, and of an observed `TurnStep` |
| `crates/adage-app/src/graph.rs` | `ArmSets.substituted` per route and arm (the treated judgement, or the control `would_substitute`), emitted as `arms.<arm>.substituted` only when non-zero (so every existing map reads as before); `event()` reads a `pretool` record that carries a decision and adds `substituted` / `would_substitute` to the timeline event |
| `crates/adage-app/src/savings.rs` | a `substituted` judgement counts the dead end as headed off (once per route per session), never as a follow |
| `plugin/claude-code{,-exec}/hooks/substitute.ts` (new) | `substituteOf`, `substituteCall(e, next, deps)`: dry run → two `$.tool.check`s → equal → verdict → `next({ ...e, command })`, `$.ui.notice(tool_use_id, context)`, and the context appended to the result's `context` for the model; never in observe mode; every failure runs the call as written |
| `plugin/claude-code{,-exec}/hooks/register.ts` | `on('tool.call', { tool: 'Bash' }, …)`; `checkBash($, command)` (`$.tool.check`), `hasSkill($, skill)` (`$.command.list()`); `activate()` reads `ADAGE_MOD_OBSERVE`. The exec copy still differs only in the two `$.state` refs |
| `plugin/claude-code{,-exec}/hooks/step.ts` | `StepDeps.isObserving` (asks carry `observe: true`; nothing yielded), `StepDeps.hasSkill` (a `skill` prediction the session does not list is dropped: the step is forwarded and nothing recorded) |
| `plugin/claude-code{,-exec}/hooks/{substitute,step,register}.test.ts` | substituted when the verdicts agree, untouched when they differ, untouched with no plan, with the final word withheld (control), inert, observe mode, an empty `ADAGE_MOD_OBSERVE`; observe and the skill check in the generator and through `$.command.list()` |
| `plugin/claude-code/VALIDATE.txt` | regenerated as CI diffs it |
| `crates/adage-sim/src/scenario.rs` | `Substitute` and `substitutions(&Replay)` (new functions only): the replay's dead-end turn becomes `make test` |
| `crates/adage-cli/tests/harness_sim.rs` | in P4b's two-phase replay: the dead end's plan, verdict and record are checked (`check_substitute`), the treated arm runs `make test` in its place (`substituted`), and `check_graph` expects the treated dead end avoided (`hit_avoided` 0, `substituted` 1, `followed` 0) for Claude Code |
| `docs/execution.md` | "Rewrites" gains "The learned pair" |
| `docs/claude-mod/poc/P3-notes.md` | open issue 6: the two gaps closed |

### Observe mode

`activate()` reads `$.env.get('ADAGE_MOD_OBSERVE')`; set and non-empty, the copy observes. It
still asks the binary at every step boundary and every Bash call, with `"observe": true` in the
payload, and never yields a predicted call or replaces a command. The binary then records what it
would have done as the control arm does (`would_predict` at a `TurnStep`, `would_substitute`
at a verdict) without touching the session's arm, so the command hooks of the same session
deliver as the treated arm. The bounds still count (`predictions`, `last_step_predicted`), as
they do in control. The binary reads the field, not the environment, so `demo/replay_hooks.py
--identity` (which sets the variable on command hooks) still compares like with like. `demo/run.py`
sets `ADAGE_MOD_OBSERVE=1` for `mod-observe`, the name the mod reads.

## How it differs from the plan

1. **No C3.** The plan builds C6 on C3's `dry_run` / `verdict` (receipts, the interpreter rewrite
   checked by the engine, the `seen_pretool` ring, `checked_by: planner`). Only the substitution
   is planned here: a dry run for a call with no dead end prints nothing, and the U11 rewrite
   still runs only from the command hook.
2. **`verdict` names the route** (`{engine: "same", substitute: <route>}`), so a plan that moved
   between the phases substitutes nothing.
3. **The mod does not ask the binary when the verdicts differ.** Nothing is recorded for a
   declined substitution; the command hook's post-hoc directive covers the failure as before.
4. **"At `fire`".** The pack validator makes every `avoid` step `inhibit`, so for an `avoid` step
   "at fire" is read as learned, `firm`, not dormant and not an instinct. A run step after a
   failure (the other shape `adage_tracks::shape` calls `avoid`) must be delivered at `fire`.
5. **The control arm goes through both phases.** It gets the plan (the mod cannot know the arm),
   checks both commands, and its verdict prints nothing, so `would_substitute` is measured on
   the same permission check as the treated arm's `substituted`.
6. **`next({ ...e, command })`**, not `next({ ...e, input: { ...e.input, command } })`: in 2.1.288
   a `tool.call` envelope spreads the tool's arguments beside `tool` and `tool_use_id`
   (`e.command` for Bash). The model's line rides on the result's `context` (what the model reads
   after the tool's result); the person's on `$.ui.notice`.
7. **The sim's dead-end turn is `python3 -m unittest discover -s tests`** (`DEAD_ENDS[0]`), not
   `python3 -m pytest tests`; the learned index has a route for each, and both are substituted
   by `make test` (checked by a dry run against a home learned from `sim/fixtures/claude`).
8. **The feed** (`crates/adage-cli/src/ui/feed.rs`, P2's tree) is not edited: a `substituted`
   judgement already reaches it as `Act::Other("substituted", route)`; `would_substitute` is not
   drawn.

## Open issues

1. **The model's context line rides on a copy of core's result** (`{ ...ran, context }`, `ref`
   kept). The types say a hook that returns the object it got makes core use its messages
   verbatim; whether core honours an added `context` beside a kept `ref` is not observable in the
   test kit. A live session must confirm the model reads the line.
2. **Order against the command hook.** After the mod replaces the command, the `PreToolUse`
   command hook sees (presumably) the new command. If it ran first on the dead end, the U11 path
   could still act on it. Not observable offline.
3. **A substituted call is the agent's call in the trace.** Its `tool_use_id` is the model's,
   so a later directive expecting `make test` could judge it `followed`. The dead end's own route
   is never judged followed (an `avoid` step makes no follow judgement); a state field marking
   substituted ids would close this.
4. **One substitution per call, no repeat ring.** A mod retry of the verdict for the same id would
   spool twice (C3's `seen_pretool` ring is not here).
5. **The skill check drops the prediction after the binary recorded it**: the `turn_step` record
   (and the session's prediction bounds) stand, though nothing was yielded.
6. **harness_sim.rs is P4b's file, edited in flight.** The substitution checks went into its
   pretool closure, its turn loop and `check_graph_predicting`. P4b's earlier assertion that the
   dry run answers what the command hook answers holds for every call but the dead end.
7. `cargo fmt --all` was not run as such: it would reformat `crates/adage-cli/tests/exec.rs`
   (P2) and `crates/adage-eval/tests/predict_gate.rs` (P4b), both in flight. Every file this
   chunk touched is formatted (`cargo fmt --all -- --check` lists only those two).

## Commands run (in /home/user/adage-poc)

| Command | Result |
| --- | --- |
| `rustfmt --edition 2021 crates/adage-hook/src/lib.rs crates/adage-runtime/src/lib.rs`, then `cargo fmt --all -- --check` | only `crates/adage-cli/tests/exec.rs` and `crates/adage-eval/tests/predict_gate.rs` differ (not this chunk's) |
| `cargo clippy -p adage-hook -p adage-runtime -p adage-sim --all-targets -- -D warnings` | clean |
| `cargo clippy -p adage-app --all-targets -- -D warnings` | clean |
| `cargo test -p adage-hook` | 22 passed |
| `cargo test -p adage-runtime` | 95 + 1 passed (during the work `turn_step_never_at_index_zero_nor_twice_in_a_row` failed once on code this chunk did not touch; it passed on the final run) |
| `cargo test -p adage-sim` | 3 + 2 passed |
| `cargo test -p adage-app` | 23 passed |
| `cargo test -p adage-cli --test hooks` | 19 passed; no golden changed, none re-blessed |
| `cargo test -p adage-cli --test graph` | 5 passed |
| `cargo test -p adage-cli --test harness_sim` | 8 passed (one warning in P4b's in-flight code: `Hook::Exec` never constructed) |
| `cargo test -p xtask` | 18 passed |
| `claude plugin validate --strict plugin/claude-code` / `plugin/claude-code-exec` | `√ Validation passed`; hooks add `tool.call{tool=Bash}`, calls add `$.command.list (via hasSkill)`, `$.tool.check (via checkBash)`, `$.ui.notice`, env reads add `ADAGE_MOD_OBSERVE` |
| `claude plugin test plugin/claude-code` / `plugin/claude-code-exec` | 53 pass, 0 fail each |
| `tsc -p <scratch tsconfig>` over each plugin's hooks with the 2.1.288 declarations | no errors |
| `node scripts/check-plugin.mjs` | `plugins adage, adage-exec 0.1.0: ok` |
| `claude plugin validate --strict plugin/claude-code 2>&1 \| sed "s#$PWD/##g"` vs `VALIDATE.txt` | no difference (regenerated) |
| a dry run of each sim dead end against a home learned with `adage condition --demo --claude-projects sim/fixtures/claude/projects` | both plan `make test` (routes `dead-end-0186eb`, `dead-end-d41b1e`); `make test` plans nothing |
