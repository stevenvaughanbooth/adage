# O0 live notes: the mod, event by event, in a real `claude` (headless)

Date 2026-10-07 (00:45–01:05 UTC). Machine: a Claude Code on the web container, Linux 6.18 x86_64,
4 × Xeon 2.1 GHz. `claude` 2.1.292, headless only (`claude -p … --output-format stream-json
--include-hook-events --debug --debug-file`). Plugin: `git archive HEAD plugin/claude-code`
(HEAD `d29f5ce`; `plugin/claude-code/hooks/` unchanged since `cdf7c84`) plus the working tree's
`plugin/claude-code/dist` (x86_64-unknown-linux-gnu, `adage` sha256 `a63c492cdb3f…`, `adage-exec`
`117561cb84d6…`, identical to `target/release/`, built from `b6548a2`; no `adage-train` was built,
so `bin/adage-train` is never linked). Spend about $1.95 for 29 sessions.

This covers evaluation-plan §2.1, §2.2 items 1–4 and §4.1–§4.7 as far as a headless session
reaches them. Everything O0 lists that needs a person at a terminal (the band, the opening line as
drawn, the press window and the turn-end ask, `/adage` and the pane, `Explore in browser`, the
Desktop app, the `/plugin update` spellings, macOS) was **not run** here.

## How it was run

All scratch is under `$S/o0n/` (`$S` = `/tmp/claude-0/-home-user-adage/7d64b41e-29be-5754-8558-0639ccc77725/scratchpad`);
nothing under `/home/user/adage-poc` but this file was written, and nothing under `/tmp/adage-live*`.

- `run.py NAME PROMPT`: a fresh `CLAUDE_CONFIG_DIR` (`runs/NAME/cfg`), a scratch copy of the
  learned home `/tmp/adage-live2/adage-home` (`runs/NAME/home`), a fresh sandbox copy
  (`python3 demo/sandbox/generate.py`; the brief's `evals/sandboxes/acme-billing/generate.py` does
  not exist, `evals/sandboxes/acme-billing` is a static tree), `--plugin-dir $S/o0n/plugin`,
  `CLAUDE_PLUGIN_DATA=<cfg>/plugins/data/adage-inline` (as `demo/run.py` does), `ADAGE_ARM=treated`,
  `ADAGE_AUTO_LEARN=0`, the host session's variables removed (`demo/run.py`'s
  `host_session_variable`), `--setting-sources project,local --strict-mcp-config`,
  `--allowedTools Bash Read Edit Write Glob Grep LS MultiEdit`.
- The wrapper (`wrapper.sh`): the scratch copy's shim, after it links `bin/hook` and `bin/adage`,
  replaces each with a script that logs argv, rc, wall time, stdin (first 300 bytes; the whole
  payload kept in `runs/NAME/io/`), any `agent_id`/`agent_type`, and stdout (first 600 bytes), then
  hands the real binary's stdout and exit code back unchanged. Every command hook and every bridge
  call is in `runs/NAME/wrap.log`. Side effect: with `bin/adage` a script, the status card's doctor
  lamp reads `check: claude-code plugin binary`; without the wrapper (`STATUS2`) it reads
  `● healthy` and `adage doctor` passes `claude-code plugin binary … matches this binary`.
- The request recorder (`fwd.py`): `ANTHROPIC_BASE_URL=http://127.0.0.1:<port>`, a forwarder that
  saves each request body (`runs/NAME/req/*.req`) and streams the response back. **It changes the
  transport**: with a base URL that is not first party the engine does not use its server-side
  thread ("tether"), so the thread-state 404 after a predicted step does not happen through it
  (W1: 3 requests). Runs marked "no recorder" use the real path (W1nf: 4 requests, the 404).
- Analysis: `an.py NAME` (debug log, stream, wrapper log, spool via `demo/adage_metrics.py`'s
  `read_spool`), `timing.py` (below), `summary.txt` (every run's requests and cost).
- Homes. `live2`: the run-2 learned home (lookup approved, `read-linked-files` and the instinct
  finish line `finish-line-e7e809` granted predict, `exec/enabled`). `home-R`: the interrupted first
  attempt's Python repository `$S/o0/repoR` (a `.venv` on a standalone CPython, U11 bound, the
  learned E2 grant `tests-after-edit` = `.venv/bin/python -m pytest -q tests`), copied per run, the
  repository used in place and restored with `git checkout` after each run. Two **mechanism probe
  homes**, built because the learned home has no route that can reach C6 or C8 (see defects 6 and
  7): `home-DE` = `live2` + the test fixture track
  `crates/adage-pack/tests/fixtures/tracks/acme-billing/dead-ends` (route renamed
  `unittest-dead-end`; `session_start` + `avoid python3 -m unittest → make test`, firm), index
  rebuilt with `adage condition --instincts-only --repo acme-billing` (gen `77958d37`); `home-FL`
  = `live2` + the fixture `…/acme-billing/finish-line` (renamed `regen-after-schema`, firm, fire,
  `after_edit schema/*.json → make gen`), index rebuilt (gen `b0da69b2`; the instinct U5 is
  extinguished as subsumed), and `adage review delegate acme-billing/finish-line regen-after-schema
  --predict --repo .` as a person (`person.sh`: no agent or host markers,
  `ADAGE_ALLOW_NONINTERACTIVE=1`). These two are not learned behaviour; they prove the mechanism.

Runs (session id prefix, model requests, cost USD; all exit 0):

| run | what | home | requests | cost |
| --- | --- | --- | --- | --- |
| W1 | "Please work on W-2041." | live2 | 3 | 0.052 |
| W1nf | W1, no recorder | live2 | 4 (one 404 retry) | 0.044 |
| W1c / W1o | W1, `ADAGE_ARM=control` / `ADAGE_MOD_OBSERVE=1` | live2 | 7 / 4 | 0.093 / 0.060 |
| W45 | "Please work on W-2045." (two linked files) | live2 | 5 | 0.070 |
| FL / FLc / FLo | schema edit with the Edit tool, "then stop: do not run anything" | live2 | 3 / 3 / 3 | 0.05 each |
| FL2 / FL2c | the same, treated / control | home-FL | 4 / 3 | 0.058 / 0.046 |
| DE | `python3 -m unittest discover -s tests`, then the same `2>&1 \| tail -5` | live2 | 3 | 0.053 |
| DE2 / DE2c / DE2o | DE, treated / control / observe | home-DE | 3 / 3 / 3 | 0.05 each |
| PYN | `python3 -m pytest -q tests`, no venv (sandbox) | live2 | 2 | 0.037 |
| RV1 | `python3 -m pytest -q tests` (venv, rewrite) | home-R | 2 | 0.130 |
| RV2 | add a test with Edit, then `.venv/bin/python -m pytest -q tests` twice | home-R | 5 | 0.066 |
| RV3 | RV1 with `deny: Bash(.venv/bin/python:*)` in `.claude/settings.local.json` | home-R | 2 | 0.040 |
| SUB / SUB2 | "Use the Agent tool with subagent_type Explore to look up W-2041 …" | live2 | 6 / 5 | 0.213 / 0.070 |
| DENY | W1 with `deny: Read(**/billing.py)` | live2 | 2 | 0.038 |
| PREDOFF | W1 after `adage delegate predict off` | live2 | 4 | 0.061 |
| OFF | W1 after `adage off` | live2 | 9 | 0.113 |
| SET | W1 with `--plugin-dir` AND the same command hooks in `--settings` | live2 | 4 | 0.058 |
| STATUS2 | `/adage status` (no wrapper) | live2 | 0 | 0 |
| CMP / CMP2 | `--resume` W1's session: `/compact keep the todo list`, then "Please work on W-2042." | live2 | 1 / 6 | 0.078 / 0.167 |

## 1. Per event

From W1 unless another run is named. "Settled" is the debug log's `hooks module adage@inline
<event> settled in` (worker hop, `next()` included: for `tool.call` that includes running the tool,
for `turn.step stream` the model's whole response). "Wall" is the wrapper's process time for the
binary call. `duration_us` is the spool record's own. Distributions over all 22 fresh sessions
(`timing.txt`): n, median, p95.

| event | ran | what the binary answered | spool record | settled (median / p95 ms) | binary wall (median / p95 ms) | `duration_us` (median ms) |
| --- | --- | --- | --- | --- | --- | --- |
| SessionStart (shim → `bin/hook`) | yes, once per session start | acme: nothing (0 bytes). repoR: `additionalContext` with the session contract (`adage/execution@1 session-contract`, 527 bytes) | `session_start {arm treated, bin adage-exec-117561…, generation, propensity 1.0, repo acme-billing}` | 804 / 1542 (the shim hashes both bundled binaries in a fresh data dir every run) | 19 / 169; once 5,752 (RV1, see defect 9) | 1.1 |
| `session.start` (mod) | yes | probes `bin/hook version` → `adage-exec 0.1.0`, again after `classic.SessionStart` | none | 214 / 322 | 15 (`version`) | – |
| Standing (mod) | yes, once per session and once per `agent.spawn` | acme: `{"compact": "Keep the repository procedures …", "environment": "", "standing": ""}`; repoR: `environment` = "In this repository, Python runs in the project's environment, .venv/bin/python: …"; home-DE: `standing` = the `unittest-dead-end` block; SUB: also `subagent` = the lookup block | none (read-only) | – | 25.5 / 56 | – |
| `prompt.compose` | yes, once | (uses Standing) | – | 24 / 68 | – | – |
| `tool.describe` (Bash) | yes, once | (uses Standing) | – | 4.9 / 51 | – | – |
| UserPromptSubmit | yes | `additionalContext`: `lookup-work-item` block (452 bytes) | `prompt` with `directives [lookup-work-item]`, `would_have` | 117 / 210 | 21 / 48 | 4.2 |
| PreToolUse, dry run (mod, `via: mod, dry_run: true`) | yes, before every Bash call, main thread and subagent | W1: nothing. RV1 `{"plan":{"rewrite":{"command":".venv/bin/python -m pytest -q tests","kind":"test"}}}`; RV2 `{"plan":{"receipt":{"fresh":true,…}}}`; DE2 `{"plan":{"substitute":{"command":"make test",…}}}`; the piped command: nothing | none (as designed) | `tool.call` 490 / 1787 (tool included) | 18 / 62 (all PreToolUse) | – |
| PreToolUse, verdict (mod) | only when the plan was non-empty | RV1 `verdict {engine: same}` → `updatedInput` + `additionalContext` + `systemMessage`; RV3 `{engine: differs}` → advice; RV2 `answered: receipt` → `{"receipt":{"card":…}}`; DE2 → `{"substitute":{…}}` | `pretool` with `checked_by: engine` (`rewrite`, `advise: permissions`, `answered: receipt`, `substituted` / `would_substitute`) | (in `tool.call`) | 162 for a receipt answer | 4.2 (pretool), 162 (answered: freshness check) |
| PreToolUse, command hook | yes, whenever the mod passed the call on | nothing for every call the mod decided (RV1's carried the already rewritten command); never ran for a call the mod answered with a receipt | none | 44 / 183 | | |
| PostToolBatch | yes, after each batch (subagent batches with `agent_id`, `agent_type`) | W1: `read-linked-files` block (487 bytes); FL: `finish-line-e7e809` (528); RV2: the E2 result `tests-after-edit` (485, after adage-exec ran pytest) | `batch` with `directives`, `judgements` (`followed`, `predicted`, `obligation_met`), `observed`, `tool_use_ids` | 89 / 208; 4,007 with an E2 run | 16 / 51; 3,966 with an E2 run | 3.1; 3,949 with `exec` |
| PostToolUseFailure | yes (DE, PYN, RV3, CMP2) | DE: `dead-end-b8ac50` + `dead-end-d41b1e` (831 bytes); PYN: `dead-end-305e5c` | `failure` with directives | 128 / 171 | 14 / 41 | 3.2 |
| TurnStep (mod only, `via: mod`) | yes, at every main-thread step ≥ 1, and once more with `ending: true` | W1 index 1: `{"predict":{"calls":[{"tool":"Read",…acme/billing.py}],"kind":"chain","route":"read-linked-files"}}`; FL index 2: `kind: run` `make gen`; FL2 index 2 `ending: true`: `kind: obligation` `make gen`; W45: two `Read`s; else nothing | `turn_step` only when it predicted or would have: `predicted`, `predicted_at [turn, index]`, `would_predict` | `turn.step stream` 2,220 / 3,865 (the model's response); a yielded step: "adage (user) answered turn.step without next() in 104.5ms" | 17 / 30 | 3.1 |
| Stop | yes | nothing in every run (no block, FL2 included) | none | 80 / 284 | 19.5 / 223 | – |
| `turn.complete` → `adage ledger note` | yes, once per turn | ledger line written: `model_turns 3` (the predicted step excluded), `cost.usd 0.0521936` = the stream's cost, `tool_calls 0` (defect 10); the payload's `steps` carry `predicted: true` with zero tokens for index 1 | `$ADAGE_HOME/ledger/acme-billing.jsonl` | 5.1 / 42 | 28 / 53 | – |
| SessionEnd | yes (`reason: other`) | nothing | none | – | 18 / 47 | – |
| `agent.spawn` | yes (SUB, SUB2) | Standing with `agent` → `subagent` (the lookup block) | none | 141 / 162 | | |
| `session.compact` | yes (CMP) | (uses Standing `compact`) | none | 10,046 (the summarizer request included) | | |
| `command.run` (`/adage status`) | yes (STATUS2) | the card from `adage --json --for-session`, answered with no model request | none | | 125 (`--json --for-session`) | |
| Press | **needs a terminal**: `dialog.onStart` binds the press only when `isInteractive` (`dialog.ts:159`) | | | | | |
| SkillPrompt | **not reachable**: no Adage skill exists in these homes and `adage-train` is not built | | | | | |

Hooks wired once: every fresh session has exactly one `session_start` record and each command
hook once per event (wrapper). A `--resume` appends another `session_start` per SessionStart
(`source: resume`, `source: compact`) to the same spool: W1's spool holds four after CMP and CMP2
(defect 11).

## 2. The mechanisms

**Chain prediction (§4.1, §4.2).** W1: the model ran `python3 scripts/wi-lookup W-2041` (index 0),
PostToolBatch delivered `read-linked-files`, TurnStep index 1 answered the `Read`, the debug log has
`turn.step: a hook answered without the request; the loop read what it streamed` and `[Stall]
tool_dispatch_start tool=Read toolUseId=adage_read-linked-files_c3972013-…_1_0`; the next
PostToolBatch judged `predicted read-linked-files`; 3 requests for 4 steps. W45: one yielded step
with two calls (`adage_read-linked-files_dadb0778-…_1_0` and `_1_1`, `schema/invoice.json` and
`acme/billing.py`), both run. Without the recorder (W1nf) the request after the predicted step got
`404 … No thread state was found for the requested previous_message_id` and `[tether] forcing
create for retry: replay_not_found` (4 dispatches). `CLAUDE_CODE_STELLAR_DRIFT=false` (W1sd) did
not turn the thread off. So the retry is the engine's thread, not the mod: through a non-first-party
base URL it is absent.

**Receipt as the result (§4.4).** RV2: after the Edit, PostToolBatch ran the E2 grant (adage-exec,
3,966 ms) and said so; the model then made the same passing command twice, and both times the dry
run said `receipt {fresh: true}`, the verdict `answered: receipt` returned the card, the debug log
has `adage (user) answered tool.call without next() in 434.0ms`, the transcript's tool result is
"Adage answered this test run from its own run of `.venv/bin/python -m pytest -q tests` in this
session (2026-10-07T00:56:14Z, operation 392a79fe8781): it passed on files identical to the current
ones …" under the model's own ids (`toolu_01XhJUAB…`, `toolu_01CrLuFM…`), and the spool has two
`pretool {answered: receipt, checked_by: engine}`. A PostToolBatch follows for the same
`tool_use_id`; it records nothing new (no judgement, the obligation already met), so nothing
double-counts. The command hook never ran for those calls.

**Rewrite and advice (§4.4).** No venv (PYN, the sandbox): no plan, no rewrite; the run failed
"No module named pytest" and PostToolUseFailure delivered `dead-end-305e5c` (make test). Venv
(RV1): plan `rewrite`, verdict `{engine: same}`, the call ran as `.venv/bin/python -m pytest -q
tests` (1 passed), the model was told "Adage pointed this test run at the project's environment
(.venv/bin/python) …", spool `pretool {rewrite: test, interpreter: .venv/bin/python, checked_by:
engine}`. Deny on the rewritten form (RV3): verdict `{engine: differs}`, the command ran as written,
spool `pretool {advise: permissions, checked_by: engine}`, and the model quoted the advice
verbatim ("Adage: this test run uses the system Python … (Adage did not rewrite the command: your
permission rules allow it as written, not rewritten.)"), so `additionalContext` reaches it. The
other settings cases (ask, a mode, none) were not run.

**The dead end (§4.5).** With the learned home: never substituted (DE: no plan; R16 of the first
attempt the same): every dead end there is an instinct, and `substitution` skips instincts
(defect 6). With home-DE: DE2's dry run planned `substitute make test` with the context, the verdict
(`engine: same`) substituted, `make test` ran in the call's place ("Ran 3 tests … OK"), the spool has
`pretool {checked_by: engine, judgements [substituted unittest-dead-end]}`, and the model quoted
"adage ran make test in place of python3 -m unittest discover -s tests: in this repository it fails
(…)". The piped form `… 2>&1 | tail -5` got no plan and ran (refused as designed:
`pretool_decide.rs:79`, not a plain command). DE2c and DE2o: `would_substitute`, `withheld`, the dead
end ran and failed, nothing printed by the substitution.

**Standing and the Bash description (§4.6, §4.7).** DE2: the system prompt's cached block ends with
the `unittest-dead-end` block, once, and the whole block is byte-identical in all three requests
(sha256 `1f54b5c1…`); the first prompt carried no block (the standing already did, T-C5-05), while
DE2o (observe: standing empty) delivered it at the prompt. RV1, RV2: the Bash tool description ends
with the environment note, once, identical in every request (sha256 `b3522bc2…`). In the acme home
the standing and the environment are empty, so no section is added and the description is
unchanged.

**Subagent (§4.7).** SUB: `agent.spawn` asked Standing with the agent; the subagent's first message
opens with the `lookup-work-item` block; its Bash dry run (`via: mod`) carried `agent_id
af2d43c8c64650e89`, its command-hook PreToolUse and PostToolBatch `agent_id` and `agent_type:
Explore`; no TurnStep was sent for its steps (6 `turn.step stream` settles, 4 TurnStep calls). It
ran the lookup itself (`cat scripts/wi-lookup | head -50; python3 scripts/wi-lookup W-2041`).
The spool records carry no `agent` (defect 5), and its lookup armed a prediction in the main
thread (defect 1).

**The finish line (§4.3).** Learned home, instinct finish line (FL): predicted at index 2 with
`ending: false` as a `run` (the instinct's directive is a pending command), not at the end of the
turn; no Stop block. Learned finish line (FL2, home-FL): PostToolBatch delivered the `obligation`
(fire); the model answered with text and `end_turn`; TurnStep index 2 `ending: true` answered
`kind: obligation make gen`; the engine kept the text and appended `adage_regen-after-schema_5bc081c1-…_2_0`
(`Bash make gen`) to the same message; the batch judged `obligation_met` and `predicted`; Stop
answered nothing. In both, the model then wrote that it ran `make gen` against the person's
instruction (defect 4). FL2c: `would_predict {kind: obligation}`, nothing yielded.

**Control and observe (§4.1.4, §4.1.5).** Control (W1c, FLc, FL2c, DE2c): nothing printed by any
hook (no `hook_response` with output in the streams), Standing empty, spool `would_have`,
`would_predict`, `would_substitute`. Observe (W1o, FLo, DE2o): the treated arm's directives printed,
predictions and substitutions withheld (`would_predict`, `would_substitute`), nothing yielded.

**Others.** DENY: the predicted `Read` was refused by the engine ("File is in a directory that is
denied by your permission settings"), the session went on and ended normally (defect 2 for the
record). PREDOFF: no `turn_step` record at all, nothing predicted, directives as usual. OFF: every
hook still runs (36 invocations), every answer is empty, no spool records, and the model worked
unaided (9 requests). SET (`--plugin-dir` + the same hooks in `--settings`): every command-hook event
was delivered twice, the binary decided once (one `session_start`, one `prompt`, two `batch` records
for four batch deliveries, the directive once in the stream); the settings' own SessionStart failed
with ENOENT because it ran before the shim linked `bin/hook` (an artefact of pointing `--settings`
at the plugin's data dir). `/adage status` (STATUS2) answered the card with no model request, and
no UserPromptSubmit fired for it (Q14, headless: no). CMP: the summarizer's request reads
"Additional Instructions:\nkeep the todo list\nKeep the repository procedures this session
followed: …" (the person's words first). CMP2: after compaction the dead-end directives still fired;
the lookup was not announced again for W-2042 (said once per session, by design).

## 3. Evaluation plan §2 and §4, headless

| item | result | evidence |
| --- | --- | --- |
| §2.1 opening line once | needs a terminal (headless: SessionStart prints no `systemMessage` with `learn.auto` off in a learned repo, `session_start.rs:125`) | W1 wrapper: SessionStart stdout 0 bytes |
| §2.1 `/adage status` answers | pass | STATUS2: card, `● healthy`, 0 requests |
| §2.1 one invocation per event, one `session_start` | pass (fresh sessions); resumed sessions add records (defect 11) | 22 fresh spools; W1 after CMP/CMP2 |
| §2.1 nothing drawn when `adage off` | headless part pass (every answer empty, no records); drawing needs a terminal | OFF |
| §2.1 `--plugin-dir` + `--settings`: wires once | pass (binary dedupes) | SET |
| Q1 the mod loads, `turn.step` called | pass | every run: `loaded (worker …)`, TurnStep calls |
| Q6 allow-list; TurnStep only from the mod | pass live (every argv on the list: `version`, `hook claude <Event>`, `--json --for-session`, `ledger note`; TurnStep only via mod); a refused argv not provoked live | wrapper logs |
| §2.2.1 `systemMessage` at InfoNotice | needs a terminal (the rewrite's verdict does print `systemMessage`) | RV1 |
| §2.2.2 `UserMessage` text = prompt | needs a terminal | |
| §2.2.3 `classic.PreToolUse` carries `updatedInput` | not applicable while the mod decides: the command hook sees the rewritten command and prints nothing; the marker comes from `tool.call` (`call.ts:217`) | RV1 io |
| §2.2.4 follower | needs a terminal (no `watch` is started headless) | |
| §4.1.1 yielded call, `adage_` id, no request | pass (W1, W45, SUB, SET, DENY, FL, FL2); 10 of 10 not run here (run 2: 12/12) | |
| §4.1.2 under the engine's permission rules | engine refuses, session continues: pass; `permission` recorded: **fail** (defect 2) | DENY |
| §4.1.3 never index 0 / never for a subagent | pass / pass for the subagent's own steps, **fail** for its effect on the main thread (defect 1); twice in a row and the cap not provoked | SUB |
| §4.1.4 control | pass | W1c, FLc, FL2c |
| §4.1.5 observe | pass | W1o, FLo |
| §4.1.6 Skill prediction | not reachable | |
| `adage delegate predict off` | pass | PREDOFF |
| §4.2 several Reads in one response | pass (2 calls) | W45 |
| §4.2 requests −2 vs `adage` | not measured here (run 2: model responses −0.75, requests flat because of the thread 404) | |
| §4.3 finish line at `end_turn` (learned obligation) | pass, mechanism (crafted home) | FL2 |
| §4.3 with the sandbox's instinct finish line | predicted one step earlier as a `run`, never at the end (defect 7) | FL |
| §4.3 one fewer request than `adage` | not measured | |
| §4.4 receipt as the result | pass (2 of 2) | RV2 |
| §4.4 covering receipt that did not answer | not run | |
| §4.4 rewrite, `checked_by: engine` | pass (allow-all and deny-the-rewrite only) | RV1, RV3 |
| §4.4 advice reaches the model | pass | RV3 |
| §4.5 dead end substituted | pass with the crafted home; not reachable with the learned home (defect 6) | DE2, DE |
| §4.5 control `would_substitute` | pass | DE2c, DE2o |
| §4.5 piped form refused | pass | DE2 |
| §4.6 section once, byte-identical | pass (3 requests) | DE2 |
| §4.6 first prompt leaves out the standing's reflexes | pass | DE2 vs DE2o |
| §4.6 compaction instruction after the person's | pass | CMP |
| §4.6 reflexes after compaction | pass (dead-end directives fired) | CMP2 |
| §4.6 routing | not run (no `routine_model` set; no ungranted `fire` directive pending in these homes) | |
| §4.7 Bash description carries the note | pass for the U11 environment note; the lookup convention never reaches it (defect 8) | RV1, W1 |
| §4.7 subagent told, its calls attributed | block in its prompt: pass; `agentId` on `tool.call`: pass; spool `agent`: **fail** (defect 5) | SUB |

## 4. Defects (read-only; nothing in the repository was changed)

1. **A subagent's batch arms a prediction in the main thread.** SUB: the Explore subagent ran the
   lookup; its PostToolBatch (`agent_id af2d43c8…`) bound `read-linked-files`; the main agent, still
   waiting, ended its turn ("The Explore agent is still running …"), and TurnStep index 1
   `ending: true` (no agent) predicted `Read acme/billing.py` into the main thread
   (`adage_read-linked-files_4023bac8-…_1_0`), an extra main-thread step and request. Not seen in
   SUB2 (the timing differed). Cause: the runtime keeps one `SessionState` per session; batch
   decisions bind `st.reads` / `st.pending` from every call whatever its `agent`
   (`crates/adage-runtime/src/decide/batch.rs` `after_command`, `retire_reads`), and only a TurnStep
   *with* an agent is skipped (`decide/mod.rs:60`). Fix: bind reads, pending commands and
   obligations only from calls with `agent: None` (or key them by agent and have `predict_step`
   read only the main thread's); test: a batch with `agent_id` binding reads, then a TurnStep
   without one, predicts nothing.
2. **A predicted call the engine refuses is recorded as made.** DENY: the batch judged `predicted
   read-linked-files` and observed `read {path}` for a `Read` whose `tool_response` was
   `<tool_use_error>File is in a directory that is denied …</tool_use_error>`. The same path would
   mark an obligation met by a refused `make gen`. Cause: `crates/adage-hook/src/harness/claude.rs:142`
   builds every PostToolBatch call with `failed: false`; a permission refusal reaches no
   PostToolUseFailure. Fix: in `call_from`, a batch entry whose response begins `<tool_use_error>`
   is `failed` (the convention `adage-eval/src/transcript.rs:289` already reads), and a failed
   `By::Adage` call is judged as refused (§4.1.2's `permission`), never `Predicted` or
   `ObligationMet`.
3. **The E2 result of a verify action reads as a regeneration.** RV2: "Adage ran `.venv/bin/python
   -m pytest -q tests` after your edit to tests/test_calc.py: what it generates was already up to
   date. Nothing changed." It never says the tests passed. Cause:
   `crates/adage-runtime/src/render.rs:185-205` `action_applied` has one template for every kind.
   Fix: pass the action's kind and exit; for `verify`, "… : it passed." / "… : it failed (exit
   N)." with the receipt's tests when known.
4. **The model takes a yielded call for its own.** FL: "I added `po_number` …, but I also ran `make
   gen`, which you told me not to do."; FL2: "I ran `make gen` after telling you I wouldn't, and I
   shouldn't have." The transcript gives the model no sign that Adage made the call (the result is
   only the tool's output). Fix (proposed): the PostToolBatch answer for a batch with a `By::Adage`
   call adds one line of `additionalContext`, e.g. "Adage made `make gen` in your place; a person
   granted it." Related design question: a predict grant overrides the person's "do not run
   anything" in the prompt and the directive's own "Ignore this if this is an exploratory or draft
   change".
5. **Spool records carry no `agent`.** SUB: subagent batches' records have no agent field (§4.7 and
   T-C9-04 expect the calls attributed). The canonical `Call.agent` is set (`claude.rs:60`) but
   `Record` never writes it. Fix: `.with("agent", …)` on `batch` / `failure` / `pretool` records when
   the event's calls carry one.
6. **No learned dead end can be substituted in the study's sandbox.** Every dead end the learner
   formed for acme-billing is an instinct (`acme-billing/instincts dead-end-*`, prime, forming), and
   `crates/adage-runtime/src/pretool_decide.rs:89` returns for `r.is_instinct()` and for any state
   but `Firm`. So C6 never fires there; it does with a firm learned route (DE2). Either the plan's
   §4.5 needs a learned dead end, or a confirmed instinct dead end should be eligible.
7. **The sandbox's finish line never reaches the `ending` path.** The finish line there is the
   instinct U5, which `decide/batch.rs:256-270` keeps as advice (a pending `run`, never an
   obligation), so `predict_step`'s pending loop predicts it at the next step boundary
   (`prediction.rs:84`), before the model has answered; the `ending` obligation path
   (`prediction.rs:120`) needs a learned finish line (FL2). If an instinct's advice should be
   predicted at all, predicting it only at `ending` would leave the model its turn first.
8. **The lookup convention never reaches the Bash description.** Standing's `environment` is only
   U11's interpreter note (`crates/adage-hook/src/standing.rs` `environment_note`), and `standing`
   only carries `session_start` routes and cards (`adage-runtime/src/standing.rs:53-68`); a
   prompt-triggered convention like `lookup-work-item` is delivered at the prompt only. T-C9-05's
   premise ("the convention is in the tool description") has no producer.
9. **A slow SessionStart (observed once).** RV1: `adage-exec hook claude SessionStart` took 5,752 ms
   (budget 10 s); three direct repeats with a fresh copy of the same home took 149–166 ms, RV2 158
   ms, RV3 169 ms. Likely cause (not proven): verifying the grant's key hashes the Python runtime's
   `lib` tree (`crates/adage-exec/src/project_python.rs:289-292` `tree_hash`, ~100 MB here, from
   `route.rs:282/498`), slow on a cold page cache; a larger runtime could pass the timeout and drop
   the session contract silently. Fix: cache tree hashes by (path, size, mtime, inode), or verify
   the runtime at execution only.
10. **The ledger's `tool_calls` is 0.** The mod's `ledger note` payload carries no tool count and
    `crates/adage-app/src/harness_ledger.rs:167` falls back to 0. Minor: count the batch calls in
    the mod (`ledger.ts`) or leave the field out.
11. **`session_start` counts more than once per resumed session.** A `--resume` (and its
    compaction) appends `session_start` records to the same spool without the `source`; a
    `hooks_wired_once` count of exactly one then fails for any resumed session. Fix: record
    `source` and count `startup` only.
12. **Verify grants of a plain pytest run are refused at staging** (from the interrupted first
    attempt's scratch, `$S/o0/stage2.json`, not re-run here): the staged run wrote
    `.pytest_cache/.gitignore`, refused as "a verify action may not change sources" although the
    action declares `cache: [".pytest_cache/**"]`. Cause:
    `crates/adage-exec/src/stage.rs:154-155` matches with `require_literal_leading_dot: true`, so
    `**` never matches `.gitignore`. The attempt worked around it with `addopts = "-p
    no:cacheprovider"`. Fix: match cache globs with `require_literal_leading_dot: false` (cache
    directories hold dotfiles), with a test on `.pytest_cache/.gitignore`.

Smaller: `adage delegate status` still lists `acme-billing/instincts/finish-line-e7e809 · predict`
after U5 was extinguished (home-FL); PostToolBatch carries the call's original command after a
rewrite or substitution (`observed: exec python3 -m unittest discover -s tests` for the call that
ran `make test`), so anything reading transcripts or `observed` without the `pretool` record by
`tool_use_id` sees the dead end as having passed.

## Not run here

Everything that needs a person at a terminal or the Desktop app (O0's T-O0-02…05, §2.2's drawing
items, §2.4 the press, §2.5 skills), macOS, `claude plugin update` spellings (T-O0-06), the
`adage-train` link (T-O0-01's third binary: not built), `claude plugin test plugin/claude-code`,
the 10-of-10 repetitions, §3 latency at release with warm starts, routing (§4.6), §4.8 noticed.
