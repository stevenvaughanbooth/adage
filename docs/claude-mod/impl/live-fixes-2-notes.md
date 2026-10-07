# Live fixes 2: what live run 2 found, closed

Live run 2 ("Run 2: the study's grants" in [`../evaluation-results.md`](../evaluation-results.md))
found four faults. These notes cover each one: what was wrong, the fix, the contract other agents
can rely on, and the proof. Built on `claude/mods-poc-m98u3o` at `d29f5ce`. Nothing is committed.
An earlier agent was cut off by a container restart while working on this; its partial edits
(`predict.ts`, `step.ts`, `fixtures/kit.ts`, the live probes under
`$S/live3`) were read, kept and finished here. `$S` is the session scratchpad.

## 1. The 404 after a step the mod made

**What happened.** In all 12 `mod` sessions, the API call right after the predicted read failed
with `404 … No thread state was found for the requested previous_message_id`
(`thread_not_found`). Claude Code then logged `[tether] forcing create for retry:
replay_not_found` and sent the request again. That retry spent the request the prediction saved.

**Why: the engine's rule (Claude Code 2.1.292, read from the installed CLI).**

- *The anchor.* A request continues a server-side thread from an anchor:
  `thread: {type: "continue", previous_message_id}`. The anchor is the last assistant message
  for which `$Ke` holds: it has a `requestId`, or its `message.id` starts `msg_` and its model is
  not `<synthetic>` (`UKe`).
- *The made-up response.* A `turn.step` hook that answers without `next()` gets a message the
  engine builds itself. Its id is minted as `` `msg_hook_${randomUUID()}` `` and it has no
  request id. Its `stop_reason` is set from the hook's stop chunk (`vs`:
  `e.message.stop_reason = s.stopReason`). Its id starts `msg_`, so it becomes the anchor.
- *The thread plan (`ctn`).*
  - It starts a new thread (`create`) when there is no anchor (`anchor_missing`).
  - It starts a new thread when nothing follows the anchor (`anchor_is_tail`).
  - It starts a new thread when the anchor's `stop_reason` is null (`anchor_incomplete`).
  - Otherwise it continues from the anchor's id.
- *The 404.* The mod's stop chunk said `stopReason: 'tool_use'`, so the next request continued
  from `msg_hook_…`. The server never issued that id, which is the 404.

The 2.1.292 declarations (`plugin-authoring/types/claude-code.d.ts`, `TurnStepResult.stopReason`)
define the stop reason as "null when no response arrived (… or no request was made)". The API has
no other way to mark a step's thread state.

**Fix (`hooks/predict.ts`, `hooks/step.ts`).** A response made wholly of predicted calls now
ends with `PREDICTED_STOP = { kind: 'stop', stopReason: null, usage: null }`, and
`predictedResult` returns `stopReason: null`. This is the documented value for a step that made
no request. With it the engine plans `create` (`anchor_incomplete`) for the next request and
sends the whole conversation. That is what the retry used to send, minus the failed dispatch. The
predicted calls still run: the engine runs a response's `tool_use` blocks whatever its stop
reason says, as the live runs below show. The request after that one continues from the next real
response again.

**Pinned by a kit test.** `fixtures/kit.ts` adds `engineThread(messages)` and
`madeUpResponse(chunks, model)`, which model the engine's rule above. In `step.test.ts`, "a
predicted step keeps the server thread whole" checks three things:

- what `turnStep` yields for a prediction leads to `create / anchor_incomplete`;
- the old `tool_use` stop leads to `continue` from `msg_hook_…` (the 404);
- the next real response is a sound anchor again.

`predict.test.ts` and the engine test "onStep through the engine" were updated to the null stop.

**Live proof (W-2041 by hand, `mod` configuration).** `$S/live3/one.py` runs one session the
way `demo/run.py one` does, in a directory of its own:

- a copy of `/tmp/adage-live2/adage-home` as `ADAGE_HOME`;
- a fresh `CLAUDE_CONFIG_DIR`;
- a plugin copy with the change, plus `plugin/claude-code/dist` (the release binaries);
- `CLAUDE_PLUGIN_DATA` set by `run.arm_invocation`;
- a generated sandbox;
- `OTEL_LOG_RAW_API_BODIES`, to read each request's `thread`.

`$S/live3/count.py` counts the runs. Nothing under `/tmp/adage-live*` was written.

| run | `[API REQUEST]` (all `sdk`) | model responses | predicted | 404 after the prediction | threads |
| --- | --- | --- | --- | --- | --- |
| before | 4 | 3 | 1 | 1 | (not logged) |
| before2 | 4 | 3 | 1 | 1 | create, continue:msg_hook, create, continue |
| before3 | 6 | 5 | 1 | 1 | create, continue:msg_hook, create, continue, continue, continue |
| after | 5 | 5 | 1 | 0 | (not logged) |
| after2 | 5 | 5 | 1 | 0 | create, create, continue, continue, continue |
| after3 | 6 | 5 | 1 | 0 | create, create, continue, continue, create, continue |
| after4 | 4 | 4 | 1 | 0 | create, create, continue, continue |
| after5 (this tree) | 3 | 3 | 1 | 0 | create, create, continue |
| after6 (this tree, with §5's line) | 4 | 4 | 1 | 0 | create, create, continue, continue |

Before the fix, every prediction was followed by a 404 and a replay: dispatches = responses + 1.
After it, no request after a prediction failed, and dispatches = responses. The one 404 in
`after3` came later in the session, after a real response. That is the background rate run 2
measured in every arm (6–22% per response). How many responses a session needs is the model's
choice (3 to 5 here), so the claim is per session: the predicted step costs no dispatch at all,
where before it cost one. All 8 sessions passed.

**Open (live-unverified), since closed in §6.** The finish line's *ending* prediction (`step.ts`,
calls appended to a real response after an `end_turn` stop) still rewrites the stop to `tool_use`. That response
has a real `msg_` id, so the next request continues from it. But the server's copy of it has no
appended calls, so the tool results that follow name a `tool_use` the server never saw. By the
rule above this would also be refused and replayed. Run 2 never reached an ending prediction, so
this is untested live. The smallest fix is the same null stop on that chunk and on the result.
That changes T-C8-02's contract (the prediction "replaces the stop" with `tool_use`), so it is left for a
decision and a live probe (`demo/run.py one W-2045 mod` with the finish-line grant).

## 5. Whose call it was (the coordinator's addition)

**What happened.** In a live session the mod ran `make gen` in the model's place. The model then
apologised for having "run" it, because it read a call in its own turn that it did not remember
making. The design says Adage's act is never counted as the agent's and the model is told plainly
(architecture §3.1–3.2).

**Fix.** The line rides on the call's result as context, the mechanism the receipt and
substitution branches use (`ToolCallResult.context`, which the engine delivers after the result
as a PostToolUse-style reminder).

- `predict.ts` has two new functions:
  - `predictedRoute(id)` reads the route from `toolUseId`'s exact shape
    (`adage_<route>_<turn>_<index>_<k>`, every other id undefined).
  - `madeForAgentLine(id)` returns: "Adage made this call in your place (learned reflex
    `<route>`); the result above is its output. You did not make it, and it does not need
    repeating."
- `call.ts` `bashCall`: when the mod is active, it decides a predicted Bash call as before
  (receipt, replacement, rewrite) and appends the line to whatever it ran or answered. A deny gets
  no line.
- `call.ts` `onPredictedCall(m, e, next)` does the same for every other tool: the chain's `Read`,
  a predicted `Skill`. It skips Bash, which `bashCall` answers, so the line is added once.
- **`register.ts` (the O6/O9 agent's file): one added line**, after the Bash `tool.call` hook:
  `on('tool.call', async ($, e, next) => call.onPredictedCall(mod($, options), e, next))`.
  A predicted `Read` has no other hook to carry the line, and every hook must be written inline
  at `on(...)` in that file. `VALIDATE.txt` was regenerated for it.

**Tests.** `call.test.ts`, "T-C4-01 · a call Adage made in the model's place says so in its
result", runs through the engine:

- a predicted Bash `make gen` runs and carries the line;
- a predicted `Read` carries it;
- the model's own `Read` and Bash calls carry nothing;
- only the exact id shape names a route.

**Live (after6, W-2041, `mod`).**

- The transcript has `attachment {type: hook_additional_context, hookName: tool.call, hookEvent:
  PostToolUse, toolUseID: adage_read-linked-files_…_1_0-context}` carrying the line, right after
  the predicted read's `tool_result`.
- The model's next response is an Edit with no text. Its closing text has no apology: no
  "apolog"/"sorry" anywhere in the transcript.
- 4 dispatches for 4 responses, no 404.

The `make gen` case is covered by the kit test. Run 2 offers no finish-line opportunity in `mod`
on W-2041.

## 2. The lookup classed as changing the environment

**What happened.** `adage-learn/src/effects.rs` `infer` stopped as soon as one run's output
contained a change word. W-1046's lookup prints "…generated models include it", so the learner
kept `scripts/wi-lookup` as `environment`, and `adage-exec` refuses predict above `regenerate`.
Two further false negatives sat behind it in the same traces:

- **A change the agent made itself.** W-1045's session ran `git status` after its own Bash
  heredoc rewrote the schema. Only Edit and Write tools counted as the agent's edit, so the
  change was laid on the script.
- **An interpreter flag.** `python3 -I scripts/wi-lookup` was not counted as a run of the script.

**Fix.** Classification now comes from the command and the script, not from prose:

- **The script's text decides.** Where a trace shows the script's text (a Read, or
  `cat`/`head`/`sed`/`less` of it), unsafe text still settles the effect at `environment`. A
  safe sight of it means the runs' output is not read for change words at all.
- **Without the text, only a line that opens with a change word counts.** Past any indent, list
  marker or `[tag]` (`Wrote cache.json`, `[cache] Updated …`). A change word inside a sentence is
  not the script speaking.
- **Only a change the script could have made counts.** A `git status`/`git diff` change counts
  against the script only if nothing that could have made it ran in between: no agent edit, and
  no command classified above `observe` (a heredoc, `make gen`). The same rule decides whether a
  run came "after an edit".
- **`runs()` skips interpreter flags**, so `python3 -I scripts/x` is a run of `scripts/x`.

**Contract.** `effects::infer` and `effect_of` keep their signatures and note format. The note
for the lookup is now `scripts/wi-lookup is not in the command table; classified observe from 11
traces: no run changed a file, and its output was read next in 11 of them; its text uses no
network or process library and writes no file`.

**Tests.**

- `crates/adage-learn/tests/effects.rs` runs over the recorded excerpt
  (`tests/fixtures/lookup-traces-live2.json`, the 12 sessions as imported). The lookup is
  observe from 11 traces, and the prose is still in the excerpt. With the script's text removed
  and one run that opens a line `Wrote …`, the lookup stays environment.
- Unit tests in `effects.rs` cover:
  - a change word in looked-up prose;
  - a seen safe text making a leading change word moot;
  - a tagged change line;
  - a `git status` after the agent's own writing command.

**Live confirmation (release binary).** Rebuilt `target/release/adage` with
`cargo build --release -p adage-cli --bin adage`. Then, in `$S/lf2`:

- a copy of `/tmp/adage-live2/adage-home`;
- `adage condition --demo --json --repo acme-billing` over run 1's 12 training transcripts and
  outcomes, as `demo/run.py learn` does (tracks kept);
- the route became `effect: observe`, `state: firm`, with the note above;
- `adage review delegate acme-billing/learned lookup-work-item --predict --repo .` gave exit 0,
  `effect observe (the classifier says environment)`, `evidence 11 of 12 traces over 1 days`;
- `adage delegate status` lists three predict grants: `finish-line-e7e809`, `lookup-work-item`
  and `read-linked-files`.

## 3. `delegate … --predict` on a nascent reflex

`crates/adage-cli/src/exec.rs` `indexed()`: when the index has no entry, the code checks whether
the track installed under that reference holds the reflex as `nascent`
(`adage_app::review::discover`). If it does, the error is:

```text
error  reflex <id> in track <track> is nascent: it waits for a person's review, and only a reviewed reflex can be delegated
  ╰─ help   `adage review approve <track> <id>` makes it eligible
```

Every other refusal is unchanged. In particular, `no reflex <id> in track <track> for <key> (see
`adage status`)` has the same text and exit code 1.

- **Test:** `delegating_a_nascent_reflex_names_the_review_that_makes_it_eligible` in
  `crates/adage-cli/tests/exec.rs`.
- **Live check:** the release binary on a copy of run 1's home (`$S/lf3`, lookup nascent) prints
  the text above and exits 1. An unknown reflex still prints the old line.

## 4. Dead ends piped through `| tail`

**What happened.** The runner counted a dead end only from its exit status (the timeline's
`verify-fail`, set from `is_error`). `ls; python3 -m pytest -q 2>&1 | tail -5` exits with
`tail`'s status (0), so all 11 pytest attempts in run 2 escaped `dead_end_hits`.

**Fix (`demo/adage_metrics.py`).** `dead_end_hits(stream)` returns
`[{id, command, failure}]`, one entry per Bash call where both hold:

- the command runs a verification (`pytest|unittest|make test|make ci`, as `classify` names
  them);
- the result is an error, or carries failure text: `No module named …`, `command not found`,
  `No rule to make target`, `make: *** `, a Python traceback, `FAILED`, or `N failed`.

`run.derived_metrics` counts the union of these ids and the timeline's `verify-fail` events, so
each tool call counts once.

**Tests.** `demo/test_run_arms.py`, `DeadEndTest`:

- The captured excerpt `demo/fixtures/dead-end-piped-live2.jsonl` holds the pytest attempt and
  the `make test` after it, verbatim from `eval-W-2043-r1-mod`'s stream (`is_error: false`). It
  scores one hit, `No module named pytest`.
- Unit cases cover each failure text, a passing run, a non-verification command, and an
  `is_error` already counted by the timeline.

**Run 2 rescored (read-only).** Dead-end hits per arm (12 sessions each):

| arm | hits |
| --- | --- |
| a0 | 0 |
| control | 0 |
| adage | 4 |
| mod-observe | 5 |
| mod | 3 |

These hits are the pytest attempts plus two real test failures during edits. They match the
dead-end directive deliveries recorded in the spool. The published run-2 tables are not edited.

### Design: substituting a dead end inside a pipeline (runtime, another agent's area)

The runtime's dead-end substitution (`pretool`) matches the whole Bash command against the
learned dead end (`python3 -m pytest`). A compound or piped command never matches, so run 2 has
no `pretool` record and `dead_end_substituted` is 0. The design:

1. **Split at the top level only.** Split the command into segments at `;`, `&&`, `||` and `|`,
   using the shell lexer `adage_core::shell` already has. Do not split inside quotes, `$(…)`, or
   heredocs.
2. **Match each segment that heads a pipeline**, so `python3 -m pytest -q 2>&1` is matched and
   `tail -5` is not. Match against the dead end's key exactly as for a whole command, with
   redirections like `2>&1` stripped before matching.
3. **Substitute in place.** Replace the matched segment with the learned alternative
   (`make test`), with its own redirections kept, and leave every other segment byte-identical:
   `ls; ls tests 2>/dev/null; make test 2>&1 | tail -5`.
4. **Only when it is safe.**
   - Substitute only when the alternative's effect is no higher than the dead end's.
   - Substitute only when the segment is not inside a subshell or function body.
   - Substitute only when no earlier segment changes directory or the environment (`cd`,
     `export`, `source`). Otherwise the alternative might run elsewhere.
5. **Record it.** Write a `pretool` record with the original and the rewritten command, and the
   segment index, so `dead_end_substituted` counts it and the timeline can show what changed.
6. **Tests.** Cover the run-2 commands as fixtures:
   - `ls; python3 -m pytest -q 2>&1 | tail -5`;
   - `ls; ls tests 2>/dev/null; python3 -m pytest -q 2>&1 | tail -5`;
   - a quoted `"pytest"` that must not match;
   - a `cd x && pytest` that must not be rewritten.

## Files touched

- `plugin/claude-code/hooks/predict.ts`
- `plugin/claude-code/hooks/call.ts`, `bashCall` wrapper and `onPredictedCall`
- `plugin/claude-code/hooks/call.test.ts`
- `plugin/claude-code/hooks/register.ts`, one `on('tool.call', …)` line (O6/O9's file; see §5)
- `plugin/claude-code/VALIDATE.txt`, regenerated
- `plugin/claude-code/hooks/step.ts`
- `plugin/claude-code/hooks/step.test.ts`
- `plugin/claude-code/hooks/predict.test.ts`
- `plugin/claude-code/hooks/fixtures/kit.ts`, the thread model, `engineThread` and
  `madeUpResponse`. The `fs.exists`, `env` and `plugin.name` additions there came from the
  live-fixes agent's `binary.ts` work, kept.
- `plugin/claude-code/hooks/token.ts`, `hookBinary(await dataDir(m), …)`: the restarted agent's
  edit, which follows `binary.ts`/`bridge.ts`'s new signature (kept).
- `crates/adage-learn/src/effects.rs`
- `crates/adage-learn/tests/effects.rs` (new)
- `crates/adage-learn/tests/fixtures/lookup-traces-live2.json` (new)
- `crates/adage-cli/src/exec.rs`, `indexed()` and the new `nascent()`, refusal only
- `crates/adage-cli/tests/exec.rs`
- `demo/adage_metrics.py`
- `demo/run.py`, `derived_metrics`
- `demo/test_run_arms.py`
- `demo/fixtures/dead-end-piped-live2.jsonl` (new)
- `CHANGELOG.md`
- these notes

## Commands and results

The results for each command are in the report that accompanies these notes. VALIDATE.txt was
regenerated with the pinned CLI (2.1.288) right before reporting.

## 6. The finish line's stop (the coordinator's decision: the thread stays whole there too)

**What was wrong.** At the finish line the mod forwards the model's real response (text,
`end_turn`), appends the obligation's call to it, and rewrote the stop to `tool_use`. The engine
records that response under the id the server gave it, with its request id, so it is a sound
anchor, and its stated reason makes the next request continue from it (`continue`,
`previous_message_id` = that id) with only the call's result after it. The server holds the
response as the model sent it: text and `end_turn`, no call. Live (below), the API refuses that
request with a 400, `unexpected tool_use_id found in tool_result blocks: adage_regen-after-schema_…
Each tool_result block must have a corresponding tool_use block in the previous message`. Claude
Code then logs `[tether] forcing create for retry: replay_other_400` and sends it again whole. That
is one dispatch spent for nothing at every finish line. Had the server accepted it, the thread
would have diverged: the server's thread would carry a result of a call it never saw.

**The three shapes, against the engine (2.1.292).**

- *A null stop on the rewritten response.* `ctn` (the thread plan) starts afresh when the anchor's
  `stop_reason` is null (`anchor_incomplete`). The next request is then sent whole, the
  appended call with it, and the server's copy is never continued. The request after the next
  real response continues from that response again. This is the shape `PREDICTED_STOP` already
  uses, and the engine still runs the response's `tool_use` blocks.
- *A hook-minted id.* A hook cannot set a forwarded response's id. The `message_start` envelope is
  an opaque `engine` chunk, and the engine names the message from it. Dropping the envelope makes
  the engine mint `msg_hook_…`, which the rule anchors exactly as before: with a stated reason
  the next request continues from an id the server never issued (the 404 of §1). With a null
  reason it is the first shape again, minus the response's real id and envelope.
- *Something the API offers for appended calls.* There is nothing. The 2.1.292 declarations have
  no chunk, field or result for it. The engine's own rewrite check (`anchor_rewritten`, from the
  `tengu_tether_echo_audit` classes such as `added_tool_use_other`) compares the transcript with
  the request's messages, not with what the server holds. Both carry the appended call, so it
  never fires.

**Fix (`hooks/predict.ts`, `hooks/step.ts`).** `predict.ts` has a new `appendedStop(stop)`,
which returns the model's own stop chunk with `stopReason: null`. It keeps the chunk's `usage` and
its `ref`, the engine's handle, so the usage the API reported still reaches the message. At the
finish line `step.ts` yields `appendedStop(chunk)` in place of `{ ...chunk, stopReason:
'tool_use' }`, and the step's result says `stopReason: null` as well. The text and the appended
calls are unchanged.

**T-C8-02's contract, changed.** On an `end_turn` stop the hook keeps every text chunk, yields
the call after the model's last block, and ends the response with the model's stop chunk with no
reason (`stopReason: null`, its usage kept; the result says the same). It used to be a `tool_use`
stop. On `tool_use` stops it forwards unchanged, as before. The contract is restated in
`test-plan.md` (T-C8-02), `C3-C9-mod-notes.md` (chains and the finish line, and live item 6),
`C3-C7-C8-D2-C9-E-notes.md` (the `ending` rule and its live-only line), and `architecture.md`
§3.2's finish-line paragraph.

**Pinned by a kit test.** `fixtures/kit.ts` extends the thread model:

- `ThreadMessage` carries the tool_use ids an assistant message holds (`calls`) and those a user
  message carries results of (`results`).
- `forwardedResponse(chunks, {id, requestId, model})` is the engine's record of a response that
  came from a request and passed through the hooks.
- `unseenResults(messages, held)` lists the results the next request carries whose call the
  server's thread does not hold. `held` is the server's copy of each real response.

`step.test.ts`, "T-C8-02 · the finish line keeps the server thread whole", mirrors §1's test:

- what `turnStep` yields at the finish line leads to `create / anchor_incomplete` with nothing
  unseen;
- the old `tool_use` stop leads to `continue` from the server's copy, with the call's result
  unseen (the 400);
- the next real response is a sound anchor again.

The first T-C8-02 test now expects the null stop on the chunk and the result.

**Live proof (finish line, `mod` configuration).** `$S/fl/one.py` runs one session the way
`demo/run.py one` does, in a directory of its own:

- a copy of `$S/o0n/home-FL` as `ADAGE_HOME`. This is the O0 agent's home: the learned finish line
  `acme-billing/finish-line/regen-after-schema` (`after_edit schema/*.json → make gen`) with a
  `predict` grant made at a terminal as a person, and `exec/enabled`;
- a fresh `CLAUDE_CONFIG_DIR`;
- the plugin staged by `run.stage_plugin` with `target/release/adage` and
  `target/release/adage-exec` and their `SHA256SUMS`;
- `CLAUDE_PLUGIN_DATA` set by `run.arm_invocation`;
- a generated sandbox;
- `OTEL_LOG_RAW_API_BODIES`, to read each request's `thread`.

The prompt is O0's FL prompt: add `po_number` to `schema/invoice.json` "with the Edit tool, then
stop: do not run anything". *before* is the plugin with this change reverted in `step.ts` only
(`$S/fl/src-before`), *after* is this tree (`$S/fl/src-after`). `$S/fl/count.py` counts. Nothing
under `/tmp/adage-live*` was read or written.

| run | `[API REQUEST]` | model responses | refused after the finish line | threads |
| --- | --- | --- | --- | --- |
| before | 5 | 4 | 1 (400, `replay_other_400`) | create, continue, continue, continue:finish-line id, create |
| before2 | 5 | 4 | 1 (400, `replay_other_400`) | create, continue, continue, continue:finish-line id, create |
| after | 4 | 4 | 0 | create, continue, continue, create |
| after2 | 4 | 4 | 0 | create, continue, continue, create |

In all four runs:

- the transcript keeps the model's text and the `adage_regen-after-schema_…_2_0` call (`make
  gen`) in one message, under its real `msg_` id and request id. Its stop is `tool_use` before
  and null after;
- `make gen` ran as that call (`is_error: false`, output `python3 tools/gen.py`);
- the Stop hook answered nothing (exit 0, empty output; no `block` in any stream);
- the session ended after one more model response.

The request after the finish line was a `continue` from the server's copy before, with 2 messages
(the call's result and a system line). After, it was a `create` with all 11, the call included.

**Observed in passing, not changed.** The first *before* attempt (`$S/fl/runs/before-auto`) ran
without `--permission-mode`. This host's session then starts in `auto` mode, and Claude Code denied
the predicted `make gen`: "The server-side auto mode classifier gave no verdict for Bash: the
request that produced this action did not ask for one". A hook-made Bash call has no classifier
verdict in auto mode, whichever stop it carries. The four runs above pass `--permission-mode
default`, as O0's FL2 ran. Whether a predicted call can ever run in auto mode is a question for
the grant's design.

**The model's reading is unchanged.** In all four runs the model's closing text says `make gen`
ran though it did not run it. §5's line ("Adage made this call in your place …") is delivered.
The model still flags it, because the prompt said "do not run anything". The design wants exactly
that: the act is never counted as the agent's.

**Files touched (this section).**

- `plugin/claude-code/hooks/predict.ts`: `appendedStop`.
- `plugin/claude-code/hooks/step.ts`: the finish line's stop and result, and the header comment.
- `plugin/claude-code/hooks/step.test.ts`: the T-C8-02 expectations and the new describe.
- `plugin/claude-code/hooks/fixtures/kit.ts`: `calls`/`results`, `forwardedResponse`,
  `unseenResults`.
- `plugin/claude-code/VALIDATE.txt`: regenerated with the pinned CLI as CI diffs it; identical, so
  unchanged.
- `docs/claude-mod/test-plan.md`, `docs/claude-mod/architecture.md`,
  `docs/claude-mod/impl/C3-C9-mod-notes.md`, `docs/claude-mod/impl/C3-C7-C8-D2-C9-E-notes.md`, and
  these notes.

**Commands.**

- `claude plugin test plugin/claude-code` with the pinned 2.1.288 CLI: 386 pass, 0 fail.
- `claude plugin validate --strict plugin/claude-code`: passed.
- `node scripts/check-plugin.mjs`: ok.
- `tsc` strict (`noUncheckedIndexedAccess`) over `hooks/**/*.ts`, the fixtures included, against
  both the 2.1.288 and 2.1.292 declarations: clean.

## 7. A predicted call only under the person's permissions (the coordinator's decision)

**What was wrong.** §6's first *before* run started in `auto` mode, and Claude Code denied the
predicted `make gen`: "The server-side auto mode classifier gave no verdict for Bash: the request
that produced this action did not ask for one". The mod yielded every prediction the binary
answered and left the permission decision to whatever met the call. Under a mode whose decider
cannot judge a call no model request produced, that is a denial in the transcript, a model told to
"issue the action again", and a finish line counted as Adage's step.

**What the engine reports (2.1.292, probed live).** A probe plugin called `$.tool.check` at the
first step of a `-p` session with `--allowedTools Bash Read Edit Write Glob Grep LS MultiEdit`, as
`demo/run.py` passes them:

| call | `--permission-mode default` | `--permission-mode auto` |
| --- | --- | --- |
| Bash `make gen` | `{ decision: 'allow', rule: 'Bash' }` | `{ decision: 'ask', reason: 'This command requires approval' }` |
| Bash `ls` | `{ decision: 'allow', rule: 'Bash' }` | `{ decision: 'ask', reason: "Claude requested permissions to use Bash, but you haven't granted it yet." }` |
| Bash `python3 scripts/wi-lookup W-2041` | `{ decision: 'allow', rule: 'Bash' }` | `{ decision: 'ask', reason: 'This command requires approval' }` |
| Read a file in the sandbox | `{ decision: 'allow', rule: 'Read' }` | `{ decision: 'allow', rule: 'Read' }` |
| Skill `x` | `{ decision: 'ask', reason: 'Execute skill: x' }` | the same |

So auto mode's "no verdict" is, as the check reports it, an `ask` with the engine's sentence and no
rule: auto mode sets the broad `Bash` allow rule aside and puts every Bash call to its classifier,
which `$.tool.check` never asks. The kit carries these shapes as `DEFAULT_MODE_BASH`,
`AUTO_MODE_BASH`, `AUTO_MODE_BASH_UNLISTED`, `READ_ALLOWED` and `SKILL_ASKED`.

**Fix (`hooks/predict.ts`, `hooks/step.ts`).**

- `predict.ts`: `permissionOf(answer)` reads a check's answer into `{ decision: 'allow' | 'ask' |
  'deny' | 'none', reason }` (anything malformed, or a check that fails, is `none`).
  `heldBy(calls, check)` checks every call of a prediction at once and returns the first call, in
  order, not allowed (`{ tool, decision, reason }`), or undefined when every verdict is `allow`.
- `step.ts`: after the binary's answer and the skill check, before the request and at the finish
  line alike, `turnStep` calls `heldBy` with `deps.check`. Only `undefined` yields. Otherwise the
  step is forwarded exactly as when nothing is predicted: the request goes out (on the routine
  model if the binary marked it routine), the stream passes unchanged, the finish line's `end_turn`
  stop is the model's own, nothing is recorded as predicted, the ledger's step is the model's. The
  held step is handed to `deps.withheld` as `Withheld = { turnId, index, route, kind, isEnding,
  tool, decision, reason }`.
- `onStep` binds `check` to the new `Mod.checkTool` (`register.ts`: `$.tool.check({ tool, input })`,
  the same call `checkBash` makes) and `withheld` to `keepWithheld`, which keeps the latest 20 in
  `rt.withheld` and raises the band's line once per session id (`rt.withheldSaid = { session,
  submits }`).
- The band (`view/band.ts`, `draw/sites.ts`): `band.withheld`, `» adage left a step to the agent:
  your permissions do not allow it unasked`, the attention line after a lapsed grant and a problem
  and before an answer kept, review waiting, an update and the version. It shows from the held step
  until the next prompt, in the session that held it. No `grant`, `predict` or strength word.

The check runs only when the binary predicted something (a prediction is rare; the check runs in
the engine and executes nothing), so a step with nothing predicted costs nothing more.

**Contract changes.** T-C4-01 now states the null stop and the `_<k>` id; T-C4-07 is new
(`test-plan.md`); plan C4 names the check and T-C4-07; `C3-C9-mod-notes.md` (chains and the finish
line, the tests table); `architecture.md` §3.2's **Model** bullet (a call the rules or mode would
put to a decider is left to the model); `ux-design.md`'s string table (`band.withheld`);
`docs/how-it-works.md`'s predicted-call paragraph.

**What the binary records today, and the request to the runtime owner.** The `TurnStep` path has no
verdict phase: the binary commits the prediction when it answers (`predict_step`: `predictions += 1`,
`last_step_predicted`, `chained`, `predicted_routes`, `predicted_at`) and spools `turn_step` with
`decision.predicted`. When the mod then holds the step back, that record says predicted, the step
counts against `runtime.predict_cap`, the route is not offered again this session, and the step is
not routed. Live (below), `fl-auto`'s spool holds `turn_step {predicted: regen-after-schema,
predicted_at: [turn, 2]}` though nothing was yielded. The mod keeps the reason locally
(`rt.withheld`) until the binary can take it. The exact request, mirroring `PreToolUse`'s phases:

1. `TurnStep` with `"dry_run": true`: decide as now under the state lock, but commit nothing and
   spool nothing; answer `{ "predict": … }`, `{ "routine": true }` or `{}` exactly as today.
2. Only after a `predict` answer, one more `TurnStep` with the same fields and `"verdict"`:
   - `{ "permission": "allow" }`: decide again and commit as today; spool `turn_step` with
     `predicted`; answer `{ "predict": … }`. The mod yields only if it names the same route and
     calls as the dry run.
   - `{ "permission": "ask" | "deny" | "none", "tool": "Bash", "reason": "This command requires
     approval" }`: decide again; spool `turn_step` with `would_predict`, `"withheld": "permission"`
     and `"permission": { "decision", "tool", "reason" }`; leave `predictions`,
     `last_step_predicted`, `chained` and `predicted_at` untouched (the step does not count against
     the cap and the next step is not "in a row"); add the route to a new
     `SessionState::withheld_routes` (rebuilt from the `withheld` records) so it is not offered again
     this session; route the step as when nothing is predicted (`would_route`, `routine`); answer
     `{}` or `{ "routine": true }`.
3. A payload with neither field behaves as today, so a mod without the phases is recorded as
   before. The control arm and observe mode answer the dry run `{}` and never see a verdict.

With that, `step.ts` sends the dry run in place of today's ask and the verdict after `heldBy`;
`StepDeps.withheld` stays the local record for the band.

**Kit tests (`claude plugin test`, 2.1.288).**

- `step.test.ts` "T-C4-07 · a predicted call is made only under the person's permissions":
  allow on every call (each call checked once, yielded, recorded); ask, deny, a throwing check, a
  malformed answer and no answer (forwarded, the model's stream unchanged, nothing recorded, the
  `Withheld` kept with the verdict and sentence); the finish line held (the model's `end_turn` stop
  as it came); one read of a chain denied holds the whole step and names that read; a listed skill
  the engine asks about is held; auto mode's shapes (Bash held, a Read chain yielded, default
  mode's allow yields); a check that throws synchronously; a keeper that throws changes nothing.
- `step.test.ts` "T-C4-07 · onStep through the engine": default mode yields after one check of
  `make gen`; auto mode yields nothing, keeps nothing as predicted, makes the request and runs no
  call; a check the engine cannot answer forwards; the band's line once per session, gone after
  the next prompt and not raised again by a second held step.
- `predict.test.ts` (`permissionOf`, `heldBy`) and `view/band.test.ts` (`band.withheld`'s place,
  its ASCII form, no banned words).
- The kit's `tool.check` fake takes `verdict(tool, input)` (any tool, a whole verdict, or
  `FAIL.exit` to throw) before `rules`; `checked` lists a non-Bash call as `<tool> <input JSON>`.
  The fake `Mod` has `checkTool`, with the same option.

**Live proof.** `$S/wd-live/one.py <out> <plugin-src> <home> <mode> <w2041|fl>` runs one session
the way `demo/run.py one … mod` does, in a directory of its own, with:

- a COPY of the home: `/tmp/adage-live2/adage-home` for W-2041, `$S/o0n/home-FL` for the finish
  line;
- a fresh `CLAUDE_CONFIG_DIR`;
- a copy of the plugin source taken after this change (`$S/wd-live/src2`), plus the repository's
  `plugin/claude-code/dist`, with its two binaries replaced by `target/release`'s and
  `SHA256SUMS` rewritten;
- `CLAUDE_PLUGIN_DATA` set by `run.arm_invocation`;
- a generated sandbox;
- `--permission-mode <mode>`, `--allowedTools` as `demo/run.py` passes them.

Claude Code was 2.1.292. `$S/wd-live/count.py` counts, and `$S/wd-live/spool.py` reads the spool's
`turn_step` and `stop` records. Nothing under `/tmp/adage-live*` was written. The runs with the
whole change (`*2`):

| run | mode | requests | predicted (yielded) | `adage_` calls, result | denials / classifier lines | Stop blocks | result | `turn_step` record |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| w-default2 | default | 5 | 1 | Read `acme/billing.py`, ok | 0 / 0 | 0 | success, W-2041 passed | predicted `read-linked-files` |
| w-auto2 | auto | 3 | 1 | Read `acme/billing.py`, ok | 0 / 0 | 0 | success, W-2041 passed | predicted `read-linked-files` |
| fl-default2 | default | 4 | 1 (finish line) | Bash `make gen`, ok | 0 / 0 | 0 | success | predicted `regen-after-schema` |
| fl-auto2 | auto | 4 | 0 | none | 0 / 0 | 1 | success | predicted `regen-after-schema` (the binary's record; nothing was yielded) |

A first round on the plugin with the permission gate alone (`w-default`, `w-auto`, `fl-default`,
`fl-auto`) gave the same picture: W-2041 passed in both modes with the read predicted, `make gen`
predicted in default, held in auto with no denial.

- **W-2041.** The chain the binary predicts for W-2041 here is the lookup's read
  (`read-linked-files`, one Read of `acme/billing.py`). The lookup itself is the model's call, as in
  live run 2. A Read is allowed by its rule in both modes, so the step is yielded in auto mode too,
  and it ran (as in `live3/after*`, which ran in auto). The brief expected nothing yielded in auto
  mode for W-2041. That holds only for a Bash step: the check allows this one, so the gate yields
  it, by design. The step held in auto mode is the finish line's `make gen` (`fl-auto`).
- **Finish line in auto mode.** Nothing was yielded. The model's response and its `end_turn` passed
  as they came, and no transcript row denies anything (0 `is_error` results, 0 classifier lines).
  The Stop gate then asked once, as it does without a prediction (the obligation is unmet). The
  model declined, quoting the prompt's "do not run anything", and the session ended `success`. In
  default mode `make gen` was predicted at the finish line and ran (`is_error: false`), as in §6.
- **The binary's record.** In `fl-auto` the spool says `predicted` for a step the mod held back (the
  request above).

**Files touched (this section).**

- `plugin/claude-code/hooks/predict.ts`: `Permission`, `Held`, `CheckCall`, `permissionOf`,
  `heldBy`.
- `plugin/claude-code/hooks/step.ts`: `StepDeps.check` and `withheld`, `Withheld`, the gate in
  `predicted`, `keepWithheld`, `onStep`'s wiring.
- `plugin/claude-code/hooks/mod.ts`: `Caps.checkTool`, `Runtime.withheld` and `withheldSaid`.
- `plugin/claude-code/hooks/register.ts`: `checkTool`, one line. VALIDATE.txt is unchanged, since
  `tool.check` was already called.
- `plugin/claude-code/hooks/view/band.ts`: `withheld`, `withheldLine`.
- `plugin/claude-code/hooks/draw/sites.ts`: the band's `withheld` input.
- `plugin/claude-code/hooks/fixtures/kit.ts`: `verdict` on the world and the fake, the
  engine-reported shapes, `checkTool`.
- `step.test.ts`, `predict.test.ts`, `view/band.test.ts`.
- `docs/claude-mod/test-plan.md`, `implementation-plan.md`, `architecture.md`, `ux-design.md`,
  `impl/C3-C9-mod-notes.md`; `docs/how-it-works.md`; `CHANGELOG.md`.

## 8. The ledger's tool calls and the lookup conventions (the mod halves of O0-fixes-notes.md §8, §6)

**`tool_calls` (O0 §8).** `ledger.ts`:

- `Noted` carries `tool_calls`.
- `accrue(noted, session, turn, calls = 0)` adds the turn's calls. Another session starts again; a
  count that is not a non-negative safe integer adds nothing; the total stops at `MAX_CALLS`
  (u32::MAX, the binary's bound).
- `Note` carries `tool_calls` and `noteOf` sets it.
- `keepCall(m, e, origin)` counts a call when it is on the main loop (`agentId` absent) and the
  engine raised it (`next.origin.plugin === 'engine'`: the model's calls and the yielded predicted
  ones; never the mod's own `$.ui.ask`).

`call.ts` calls `keepCall` in `onBashCall`, before anything decides, so a call a receipt answers is
counted, and in `onPredictedCall` for every other tool. `noteLedger` takes and clears
`rt.turnCalls` (`mod.ts`) at each main-loop turn end.

The note's stdin gains `"tool_calls": n`, which a63bc2b's `harness_ledger.rs` reads.

Tests (`ledger.test.ts`):

- `accrue`'s counts across turns, a /clear, nonsense counts and the bound;
- the note's `tool_calls`;
- through the engine: three main-loop calls, one of them an `adage_` call, and one subagent call
  give `3`; a call in the next turn gives `4`.

Live (the `*2` runs above), the ledger's line for each session says `tool_calls` 6, 4, 3 and 2.
Those are the calls each transcript lists, predicted ones included. In the first round, without this
change, the W-2041 default session's line read 0.

**Lookup conventions (O0 §6).** `attach.ts`:

- The session's `Standing` ask carries `conventions: true`. A subagent's ask does not.
- `Standing` gains `conventions`, which `standingOf` reads.
- The Bash description's note is `describedNote`: the environment note, then the conventions,
  joined by a blank line, appended once.

The wire case `standing-conventions` was added to `crates/adage-hook/tests/wire.rs` as the notes
give it. It was blessed into `crates/adage-hook/tests/wire/standing-conventions.json`, and
`node scripts/check-plugin.mjs --wire` regenerated `hooks/fixtures/wire.ts`.

`attach.test.ts`:

- the payloads now carry `conventions: true`;
- "the lookup conventions follow the environment note on the Bash tool, once; the session asks for
  them, a subagent does not";
- "standing-conventions.json alone";
- `standingOf`'s fourth part.

Live, the release binary answers the mod's ask with `"conventions": ""` for these homes. Asked by
hand, both release and debug answer it, but neither home holds a learned lookup prompt route at
full strength. So the Bash description was unchanged in these runs; the kit tests cover the
placement.

**Files touched (this section).**

- `plugin/claude-code/hooks/ledger.ts`, `call.ts`, `mod.ts`, `attach.ts`.
- `ledger.test.ts`, `attach.test.ts`, `fixtures/wire.ts`.
- `crates/adage-hook/tests/wire.rs`: one case, in another agent's crate, at the coordinator's
  request.
- `crates/adage-hook/tests/wire/standing-conventions.json`: new.

**Commands (§7 and §8).**

- `claude plugin test plugin/claude-code` with the pinned 2.1.288 CLI: 409 pass, 0 fail.
- `claude plugin validate --strict plugin/claude-code` and `claude plugin validate --strict .`:
  passed.
- `plugin/claude-code/VALIDATE.txt` was regenerated as CI diffs it. It is identical, so unchanged.
- `node scripts/check-plugin.mjs`: ok. Its `--wire` mode regenerated `wire.ts`.
- `ADAGE_BLESS=1 cargo test -p adage-hook --test wire`, then without the variable: 2 pass.
- `tsc` strict (`noUncheckedIndexedAccess`) over `hooks/**/*.ts(x)`, the fixtures included, against
  both the 2.1.288 and 2.1.292 declarations: clean.
- `cargo build -p adage-cli -p adage-exec --bins` (debug) was run only to ask `Standing` by hand.
