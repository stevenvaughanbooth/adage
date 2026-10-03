# The proof of concept

What was built on the branch `claude/mods-poc-m98u3o` against Claude Code 2.1.288, to show that
the mod can hook into the harness and send different kinds of predictions, and what the offline
evaluation of it says. Each chunk's own notes hold the exact contracts, what differs from the
plan, and the open issues; this page is the index and the honest summary.

## Scope

The POC covers the critical path of the plan (`../implementation-plan.md` §4): C1 (`TurnStep` in
the binary), C2 (the `predict` grant in adage-exec), B1 and C4 (the mod and its `turn.step`), C6
(the dead end that never runs), the simulator and demo arms (E1), and an offline evaluation gate.
It does not cover drawing (B2–B4, D5), the press token (A7), standing text and compaction (C5),
attachment capabilities (C9), or the learner's candidates and skills (D1–D4).

## Chunks

| Chunk | Delivered | Notes | Commit |
| --- | --- | --- | --- |
| C1 (+ C7 chain, C8 `ending`) | `Event::TurnStep`, `Call.by`, `Pending.command`, `Prediction{kind,route,track,calls}` for run, sequence, skill, chain and obligation; bounds; `Judgement::Predicted`; `would_predict` in control; the `turn_step` spool record; `fast::skip`; adapters; feed, savings, graph, transcript readers | [P1-notes.md](P1-notes.md) | `45bc903` |
| B1, C4 (mod half), C8 | One mod module per plugin: probe and activation, classic observers, `turn.step` generator yielding predicted calls, the finish line after an `end_turn` stop, allow-list and `CLAUDECODE=1` in the bridge; 41 plugin tests each; CI pins the CLI and diffs `VALIDATE.txt` | [P3-notes.md](P3-notes.md) | `4086596` |
| E1 (runner) | Five arms in `demo/run.py`, the `requests` metric and its verification, `replay_hooks.py` with `--identity` for the mod-observe identity check, the `demo` Python suite | [P4a-notes.md](P4a-notes.md) | `13b2495` |
| C2 | the `predict` grant in adage-exec and `adage review delegate --predict`; `adage-exec version` | [P2-notes.md](P2-notes.md) | pending |
| C4 (sim half), E2 | `TurnStep` and two-phase `PreToolUse` payloads in adage-sim, `predict` and `pretool` expectations on `Replay`, the both-arm replay in `harness_sim` (a prediction under `adage-exec` with the grant, `would_predict` in control, nothing without the grant or under `adage hook`), the offline prediction gate `predict_gate.rs` | [P4b-notes.md](P4b-notes.md) | pending |
| C6, observe mode | `substitute` in the `dry_run` plan, the `verdict` phase, `Judgement::Substituted` and `would_substitute`, the mod's branch with two `$.tool.check`s; `ADAGE_MOD_OBSERVE` sends `observe: true` so the binary records without acting; a skill prediction only when the engine lists the skill; 53 plugin tests each | [P5-notes.md](P5-notes.md) | `9f6eceb` |

## The contract the mod and the binary share

One line in, one line out, on the CPU (P1-notes.md has the field table):

```text
stdin   {"session_id","cwd","hook_event_name":"TurnStep","turn_id","index","model"?,"agent_id"?,"ending"?}
stdout  nothing, or {"predict":{"kind":"run|sequence|skill|chain|obligation","route","track","calls":[{"tool","input"}]}}
```

The calls the mod makes come back in the next `PostToolBatch` with a `tool_use_id` beginning
`adage_`, which makes the canonical call `by: adage` and the judgement `Predicted`, never a
follow. The grant is a `predict` approval on the route in adage-exec; `adage hook` has no
extension and never predicts.

## Evaluation

No API key is available in the environment the POC was built in, so no live session ran. What
ran, and what each run can and cannot show:

| Run | Shows | Cannot show |
| --- | --- | --- |
| adage-runtime unit tests (`turn_step_*`) | every bound and every kind on `decide` alone | timing, the engine |
| adage-hook in-process tests | the exact stdout, the spool record, a predicted call meeting its obligation and no Stop request following | the mod's side |
| `claude plugin test` (53 per plugin) | the generator with the binary and the engine stubbed, the finish line, the substitution branch, observe mode, inertness | what the engine records |
| the Claude golden and parity | one silent `TurnStep` step changes nothing else; `adage-exec hook` agrees | a prediction (no grant in that fixture) |
| `hook_latency` | an empty step costs what an empty batch costs (p50 2.56 ms vs 2.64 ms, one local run) | a predicting step (needs C2) |
| the sim in both arms (`harness_sim`) | under `adage-exec` with a `predict` grant the `make gen` obligation is predicted at the work turn's end, judged `predicted` and `obligation_met`, and no Stop request follows; control spools `would_predict` and prints nothing; without the grant, and under `adage hook`, every step is silent; the dead end becomes `make test` in the treated arm | a live engine |
| the offline prediction gate (`predict_gate.rs`, both arms over the recorded datasets) | the numbers below | what the agent would have done next once Adage acted early |
| `demo/run.py` dry run | the arms are wired and the metrics computed from fixtures | any outcome |

The gate's numbers, from `cargo test -p adage-eval --test predict_gate -- --nocapture` (P4b-notes.md
has the method; every learned route except exec-only ones is granted):

| Dataset | Sessions | Opportunities | Requests saved, treated | Control | Next action predicted | Agent made the call later |
| --- | --- | --- | --- | --- | --- | --- |
| `demo-haiku45` round 2 | 18 | 34 | 34 | 0 | 6 of 34 | 23 of 34 |
| `synthetic` | 18 | 25 | 25 | 0 | 12 of 24 | 20 of 24 |

Read the accuracy column with its caveat: the scored sessions are agents that never got a
directive, so they found the lookup script themselves several steps after the step where Adage
would have run it. A correct early prediction does not count as the agent's "next" action, and
once Adage acts early the rest of the recording is no longer what that agent would have done.
"Made later" is the fairer offline bound; the live `mod` arm (`../test-plan.md` §7, T-C4-06, E2)
is where the real figure comes from. The gate pins the measured values as floors so a regression
in the runtime's bounds or kinds fails it.

The live protocol (`../test-plan.md` §7) is unchanged and still the exit criterion for M4; the
demo runner is ready to run it once a key is set.
