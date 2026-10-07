# The prompt's run reflex at the turn's first step (index 0)

Run 3 (`evaluation-results.md`, "Run 3") found the design's first lever, the lookup (architecture
§1b, §3.2 "the request Adage does not send"), granted and never predicted in 36 of 36 treated
sessions: the lookup's directive arrives with the prompt, the model makes the call in its very first
response, and both the binary (`predict_step`, `index == 0`) and the mod (`step.ts`,
`e.index === 0`) never predicted at index 0 (T-C1-02, plan C1: "index 0 is the prompt's request").
The coordinator moved the bound. This note is what was built, the contract, and the live check.

## The rule

At index 0 of a main-loop turn Adage may make one step: a `Run` reflex **this prompt** directed,
every slot bound from the prompt's own words, so its command is fixed (`scripts/wi-lookup {item}`
with the work item the prompt names). Under the same conditions as every prediction: a `predict`
grant, the engine's `allow` through the two phases, never for a subagent, the session cap, once per
route per session, the dead-end interlock. Everything else at index 0 stays forbidden: a sequence,
a skill, a chain, a finish line, a directive from an earlier prompt, anything whose slots are not
bound from the prompt; the step's end (`ending: true` at index 0) is never asked or answered;
nothing is routed at index 0.

## The engine (why the thread stays whole)

`turn.step` at index 0 fires after `UserPromptSubmit`: the command hook has delivered and recorded
the prompt's directives when the mod asks. A yielded call makes the first response a made-up one
(`msg_hook_…`, `PREDICTED_STOP`: no stop reason), which the engine takes as an incomplete anchor,
so the model's first real request starts the thread afresh (`create`, `anchor_incomplete`) with the
prompt, the call and its result in it. Checked with the kit's `engineThread` and `unseenResults`
after a session's first prompt and after a later one (an earlier real anchor), with the chain read
following at index 1 (`step.test.ts`, "the thread: the first real request starts it afresh"), and
live: the first request body of `w2041-b` names `thread: {type: "create"}` and carries the
`adage_lookup-work-item_…_0_0` call and its result.

## The directive's text

Unchanged. The directive is rendered by the `UserPromptSubmit` command hook, before the step's dry
run and verdict exist, so it cannot truthfully say the call ran (the verdict may hold it back), and
the command-hook goldens stay byte-identical. The model learns the call was made from the line on
its result instead, which the mod adds for an `adage_` id whose step index is 0 (`predict.ts`
`madeForAgentLine`):

> Adage made this call in your place before your first response: it is the step the learned reflex
> lookup-work-item, delivered with the request, names. The result above is its output. You did not
> make it, and it does not need repeating.

Every later step keeps its line ("Adage made this call in your place (learned reflex …)"); the
phrase the evaluation counts is kept.

## Contracts

- **State.** `SessionState.at_prompt: Vec<String>` (`#[serde(default, skip_serializing_if =
  "Vec::is_empty")]`): the run routes the latest prompt directed with every slot bound from it
  (`decide::prompt::at_prompt`: a `Step::Run` from `bind_prompt_route`, prompt or intent trigger,
  command non-empty with no `{`). Set at the prompt; cleared at the start of every `Prompt`,
  `Batch`, `Failure` and `Stop` decision, so it never outlives the first step. Not rebuilt from the
  spool (as `pending` is not): a rebuilt state predicts nothing at index 0. A context trigger
  firing on a prompt item is not included (it binds no slot from the prompt).
- **Runtime** (`prediction.rs` `predict_step`). At `index == 0`: nothing with `ending`, nothing
  when `at_prompt` is empty; only pending entries whose route is in `at_prompt` are candidates, and
  only as `(None, Step::Run)` (`kind: run`); read batches and obligations are not looked at. The
  in-a-row rule uses `index.checked_sub(1)` (no underflow); a prediction at `(t, 0)` makes the
  reads bound from its result (`after_adage`) the chain's second step at `(t, 1)`, and
  `chained` then stops a third. `on_turn_step` still routes only at `index > 0`.
- **Wire.** Unchanged: `TurnStep` with `index: 0` answers `{"predict": {"kind": "run", "route",
  "track", "calls": [{"tool": "Bash", "input": {"command": "scripts/wi-lookup W-2042"}}]}}`, in the
  dry run and on an `allow` verdict; the record is `turn_step` with `predicted_at: [turn, 0]`.
- **Mod** (`step.ts` `turnStep`). Index 0 is no longer forwarded unasked: the dry run is asked; an
  answer whose kind is not `run` is refused before the verdict (so nothing is committed or
  recorded), and the step's end is not asked at index 0. A subagent's step is still forwarded
  unasked at every index.

## Departures and touches outside the brief's files

- `crates/adage-sim/src/scenario.rs` (no owner named): the held-out replay's lookup is expected at
  step 0 and the reads at step 1 in the treated arm; `Predict` gains `control_index` (the control
  session's agent looks the item up itself after its look around, so the reads it binds are due at
  step 2 there). `the_replays_predictions_stand_for_the_agents_turns` follows.
- `crates/adage-cli/tests/harness_sim.rs` (the pipeline-substitution agent's file, edited after its
  work landed at `739879c`): `replay_claude_with` asks step 0 like any step and removes the turn a
  predicted step stands for wherever it is in the queue; the pretool expectations are matched by
  command (the lookup's call now runs before the look around's `ls`); `check_turn_steps` reads
  `control_index` in the control arm; the substitution's expected line prefix.
- The substitution's context line (the coordinator's mid-task request; `pretool_decide.rs`, after
  `739879c` landed): live, "adage ran make test in place of …: in this repository it fails" over
  output cut by `| tail -5` was read as the replacement failing. It now names what ran, what it
  replaced and which command fails, and where the output above comes from:
  `adage ran make test for you; python3 -m pytest tests was replaced because that command fails in
  this repository ("No module named pytest"). The output above is from make test.` For a pipeline,
  `adage ran make test for you, through the rest of your pipeline as written; python3 -m unittest
  discover -s tests -t . was replaced because …`: the filters are not quoted, since `sanitise`
  drops `>` and would print `2&1`. This line reaches only the mod's phases: the claude-code golden
  changed in exactly one line, the `### PreToolUse` mod dry run (`"dry_run": true, "via": "mod"`),
  re-blessed with `ADAGE_BLESS=1`; the wire files (`plan.json`, `substitute.json`) re-blessed and
  `hooks/fixtures/wire.ts` regenerated with `node scripts/check-plugin.mjs --wire`; the record
  snapshot re-accepted (its two `substituted`/`would_substitute` lines); the tests that pin it
  (`dead_end.rs`, `pipeline_substitution.rs`, `pretool/phases.rs`, `scenario.rs`, `harness_sim.rs`)
  and `docs/execution.md`'s two examples. `ux-design.md` does not quote it.

## Tests

- Runtime (`tests/turn_step.rs`): `turn_step_never_at_index_zero_nor_twice_in_a_row` (T-C1-02, now
  an earlier turn's pending and an armed obligation at index 0, with and without `ending`);
  `turn_step_at_index_zero_makes_the_prompts_own_run_reflex` (the lookup at `(t, 0)`, still
  delivered, not routed; Adage's batch judged `predicted`, `at_prompt` cleared, the reads bound
  `after_adage` and predicted at `(t, 1)` as a chain, nothing at `(t, 2)`; the control arm's
  `would_predict` at the same step); `turn_step_at_index_zero_refuses_everything_else` (no grant;
  the step's end; a batch, a failure, a Stop since the prompt; a subagent; the cap; predicted
  before; a dead end told (an after-failure one: a session-start dead end leaves the lookup out of
  the index altogether); the dry run commits nothing, `ask` withholds and holds the route, `allow`
  commits; a skill the prompt directed (then taken at index 1); a sequence, a finish line and a read
  batch even when named in `at_prompt`).
- Hook (`respond/step_phase_tests.rs`): `the_prompts_run_reflex_is_made_at_the_first_step_under_the_verdict`
  (the real `UserPromptSubmit` then `TurnStep` index 0 through both phases; `ask` withholds; a
  subagent and a later step 0 after a batch make nothing).
- Mod (`step.test.ts`): "the turn's first step: only the prompt's run reflex" (a run in two phases,
  no request, the null stop, the `_0_0` id; every other kind refused before the verdict; the end
  never asked; the permissions hold it; the thread), the engine tests at index 0 in default and
  auto mode, the forwarded cases updated (a subagent's first step); `predict.test.ts` the index-0
  line. `fixtures/kit.ts`: `FIRST_STEP`, `PREDICT_LOOKUP`, `PREDICTED_LOOKUP_ID`.

## Commands and results

- `cargo fmt --all --check`: clean. `cargo clippy -p adage-runtime -p adage-hook -p adage-sim
  --all-targets -- -D warnings` and `cargo clippy -p adage-cli --tests -- -D warnings`: clean.
- `cargo test -p adage-runtime -p adage-hook -p adage-sim`: runtime 138 + 1, hook 71 + wire 2, sim 5.
- `cargo test -p adage-cli --test hooks`: 25 (golden: the one mod dry-run line above);
  `--test harness_sim`: 11; `--test mod_surfaces`: 11 (1 ignored). `cargo test -p adage-exec`
  (whole, before the line change): 142 lib and every integration target; `--test parity`: 25.
  `cargo test -p xtask`: 24.
- Pinned CLI (`$S/cli288/node_modules/.bin/claude`, 2.1.288): `plugin validate --strict
  plugin/claude-code` and `.` pass; the validate output made as CI makes it is identical to
  `VALIDATE.txt` (no new `$` API); `plugin test plugin/claude-code`: 430 pass, 0 fail.
  `node scripts/check-plugin.mjs`: `plugin adage 0.1.0: ok`.

## Live (Claude Code 2.1.292 on PATH)

Driven by `$S/i0-live/one.py` (`$S/tsp-live/one.py` unchanged): the debug build of this change
(`cargo build -p adage-exec -p adage-cli`, `target/debug/adage` and `adage-exec` replacing `dist/`'s,
`SHA256SUMS` rewritten), a copy of `plugin/claude-code` from the tree, a fresh `CLAUDE_CONFIG_DIR`,
a COPY of `/tmp/adage-live3/adage-home` (the run-3 grants: lookup, chain of reads, finish line; the
original was only read), `CLAUDE_PLUGIN_DATA` as `demo/run.py` sets it, `--permission-mode
default`, a generated sandbox, W-2041 with run 3's prompt. Run `a` used the build before the
substitution line change and before `739879c`; `b`–`d` the build after both.

| run | `[API REQUEST]` | first assistant message | then | 404 / 400 / API error | dead-end hits | passed |
| --- | --- | --- | --- | --- | --- | --- |
| `w2041-a` | 5 | `adage_lookup-work-item_…_0_0` Bash `scripts/wi-lookup W-2041` | `adage_read-linked-files_…_1_0` Read `acme/billing.py`; the model's first response: two Edits | none | 2 | yes |
| `w2041-b` | 5 | the same | the same; first response: Edit + Read of the test | none | 2 | yes |
| `w2041-c` | 5 | the same | the same; first response: two Edits | none | 2 | yes |
| `w2041-d` | 5 | the same | the same; first response: two Edits | none | 2 | yes |

- In every run the transcript's first assistant message is the predicted lookup (`msg_hook_…`, no
  stop reason) carrying the new line, then the chain read at index 1 with the usual line; the debug
  log has `answered turn.step without next()` twice before the first request and no `API error`,
  `thread_not_found` or tether line; the model's first real response works from the result (it
  edits `acme/billing.py` at once, no lookup or read of its own); no apology anywhere.
- **The request count did not fall below run 3's `mod` on W-2041 (5): it is 5 in all four runs.**
  The response saved (the model's own lookup; the reads were already predicted in run 3) was spent,
  with one more, in every run on the `python3 -m unittest discover -s tests -t .
  2>&1 | tail -5` dead end and a retry with `| head -20; ls . tests` before `make test`. That dead
  end is an instinct here (the run-3 home's U3/U1 routes), and substitution replaces only a learned,
  firm, non-instinct route, so `739879c` does not apply to it in this home; the second command is a
  list, which is never rewritten. Without the two dead-end responses each run is 3 requests
  (first response edits, `make test`, the answer). Run 3's `mod` W-2041 sessions had 1 dead-end
  hit; these 4 had 2 each.
- Spend: about $0.26 for the four runs.

## Open

- The live criterion "fewer `[API REQUEST]` lines than run 3's `mod` on W-2041" is not met (5 = 5);
  the saving at index 0 is visible in the transcripts and was spent on dead-end hits. The next lever
  is the instinct dead end (substituting instincts, or a learned firm route for it), not this rule.
- `evaluation-plan.md` §M4 item 3 ("never at index 0") is the pre-specified plan of run 3 and was
  left as written.
