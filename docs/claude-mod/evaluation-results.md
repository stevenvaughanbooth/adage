# Evaluation results: the Claude Code mod, live (headless comparison, §5 and §2.3)

The first live run of `docs/claude-mod/evaluation-plan.md` §5 (the five-arm headless comparison)
and §2.3 (observation changes nothing), with the defects it found on the way and their fixes. The
other sections of the plan (§2.1–§2.2, §2.4–§4 except one finish-line probe, §6–§7) were not run
here and are not reported.

## Verdict

**The M4 exit criterion is not met.** In 60 interleaved held-out sessions (6 tasks × 5 arms × 2) the
`mod` arm ran end to end, every request count verified and every hook wired once, but it never
predicted a step: `predicted_steps` 0 of 12 sessions. Its one predict grant (the finish line,
`make gen` after a schema edit) had no opportunity in the `mod` arm: no `mod` session edited a
schema file with an edit tool, and every one that changed a schema ran `make gen` itself in the
same Bash command. Against `adage`: requests 8.00 vs 7.83 (diff +0.17, 95% CI [−0.83, 1.00],
p 0.87); `dead_end_hits` 0 vs 0 (no arm ever hit the dead end); directives 0.83 vs 1.17 (diff
−0.33, CI [−0.67, −0.08], p 0.088 unadjusted), lower for a reason that is not the mod's (below);
`hidden_passed` 12/12 in both. The mechanism itself works live: in a probe that leaves the finish
line pending, the mod yielded `make gen` in the model's place (an `adage_` tool_use, a `turn_step`
record with `predicted`, no request for that step).

`mod-observe` against `adage`: no difference in any outcome metric (requests p 1.0, cost p 0.99,
pass and hidden pass 12/12 each); the offline identity check is byte-identical.

## Machine and inputs

- A Claude Code on the web container (Linux 6.18, 4 × Xeon 2.1 GHz), `claude` 2.1.292, headless,
  through the container's preconfigured proxy and host login.
- Binaries `target/release/adage` (sha256 `a63c492cdb3f…`) and `adage-exec` (`117561cb84d6…`),
  built 2026-10-06 23:22–23:27 UTC from the worktree at `b6548a2` (not rebuilt for this run).
- The plugin: a frozen copy of the branch's `plugin/claude-code` with `dist/` for this host,
  staged by the runner into `$DEMO_ROOT/plugins/claude-code` (both binaries, digests checked).
- `DEMO_ROOT=/tmp/adage-live`, `ADAGE_HOME=$DEMO_ROOT/adage-home`,
  `CLAUDE_CONFIG_DIR=$DEMO_ROOT/claude-config` (from the smoke on), `DEMO_MODEL` as in the
  first pass, `DEMO_CONCURRENCY=3`, `DEMO_MAX_TURNS=30`.
- Spend: first pass $1.84 (discarded, see below); this pass about $6.3 (training $0.82, learning
  free, smoke runs and probes about $1.3, the comparison $4.21).

## Results

`DEMO_REPEATS=2 python3 demo/run.py eval --arms a0,control,adage,mod-observe,mod`: 60 of 60
sessions finished; none left out, none censored; 60 distinct session ids.

| arm | n | requests (mean) | model responses (mean) | retries | cost USD (mean) | passed | hidden_passed | stale_generated | dead_end_hits hit / substituted | directives (mean) | predicted_steps | receipts_as_results | requests verified | hooks wired once |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| a0 | 12 | 7.58 | 6.75 | 10 | 0.0658 | 12/12 | 12/12 | 0/12 | 0 / 0 | 0.00 | 0 | 0 | 12/12 | 12/12 (no spool) |
| control | 12 | 8.67 | 7.17 | 18 | 0.0706 | 12/12 | 12/12 | 0/12 | 0 / 0 | 0.00 | 0 | 0 | 12/12 | 12/12 |
| adage | 12 | 7.83 | 7.17 | 8 | 0.0708 | 12/12 | 12/12 | 0/12 | 0 / 0 | 1.17 | 0 | 0 | 12/12 | 12/12 |
| mod-observe | 12 | 7.75 | 6.83 | 11 | 0.0707 | 12/12 | 12/12 | 0/12 | 0 / 0 | 0.75 | 0 | 0 | 12/12 | 12/12 |
| mod | 12 | 8.00 | 7.08 | 11 | 0.0730 | 12/12 | 12/12 | 0/12 | 0 / 0 | 0.83 | 0 | 0 | 12/12 | 12/12 |

`requests` is the debug log's dispatch count (the harness's definition, retries included); "model
responses" is the stream's distinct request ids; "retries" sums the `[ERROR] API error (attempt …)`
entries, almost all `404 … "No thread state was found for the requested previous_message_id"`, a
server-side miss retried once, in about half the sessions of every arm. `subagent_tool_calls` is 0
everywhere (subagents are disallowed). All 24 plugin-arm sessions were served by `adage-exec` (the
spool's `session_start.bin`); no spool has a `turn_step` record, so nothing was even
`would_predict`.

Requests per task (repeat 1, repeat 2):

| task | a0 | control | adage | mod-observe | mod |
| --- | --- | --- | --- | --- | --- |
| W-2041 | 8, 8 | 6, 12 | 8, 10 | 7, 9 | 8, 6 |
| W-2042 | 7, 8 | 9, 9 | 7, 8 | 7, 7 | 7, 7 |
| W-2043 | 8, 7 | 9, 9 | 8, 7 | 9, 7 | 10, 7 |
| W-2044 | 9, 7 | 8, 7 | 7, 8 | 6, 9 | 8, 9 |
| W-2045 | 8, 7 | 10, 6 | 9, 8 | 7, 6 | 9, 8 |
| W-2046 | 7, 7 | 8, 11 | 7, 7 | 12, 7 | 10, 7 |

`demo/report.py` (task-stratified permutation p, task bootstrap 95% interval, Holm across the
outcome metrics; the mechanism counts unadjusted). Medians for the turn and time metrics, means
otherwise; diff is a − b.

| metric | `mod` − `a0` | `mod` − `adage` | `mod-observe` − `adage` |
| --- | --- | --- | --- |
| requests | +0.42 [−0.33, 1.17] p 0.47, p_holm 1 | +0.17 [−0.83, 1.00] p 0.87, p_holm 1 | −0.08 [−1.17, 1.08] p 1.0, p_holm 1 |
| cost_usd | +0.0072 [0.0038, 0.0108] p 0.014, p_holm 0.10 | +0.0023 [−0.0020, 0.0075] p 0.39, p_holm 1 | −0.00002 [−0.0027, 0.0027] p 0.99, p_holm 1 |
| pass_rate | 0 (1 vs 1) | 0 | 0 |
| hidden_pass_rate | 0 (1 vs 1) | 0 | 0 |
| stale_generated_rate | 0 (0 vs 0) | 0 | 0 |
| dead_end_hits | 0 | 0 | 0 |
| total_turns | +1.5 [0.5, 2] p 0.035, p_holm 0.21 | −1 [−1.5, 0] p 0.27 | −1.5 [−2, 0] p 0.009, p_holm 0.10 |
| explore_calls_before_edit | +3 [2, 4] p 0.0015, p_holm 0.014 | +1 [0, 1.5] p 0.48 | −1 [−2, 0.5] p 0.30 |
| time_to_first_edit_s | +3.6 [0.7, 5.1] p 0.0002, p_holm 0.002 | +3.2 [−0.8, 6.9] p 0.12 | −0.04 [−3.4, 8.5] p 0.85 |
| elapsed_s | +3.2 [1.0, 5.1] p 0.0006, p_holm 0.006 | +2.4 [−1.0, 4.9] p 0.90 | +0.2 [−3.5, 5.5] p 0.66 |
| directives | +0.83 [0.67, 1] p 0.0007 | −0.33 [−0.67, −0.08] p 0.088 | −0.42 [−0.67, −0.17] p 0.044 |
| predicted_steps | 0 | 0 | 0 |
| receipts_as_results | 0 | 0 | 0 |
| dead_end_substituted | 0 | 0 | 0 |

For context, `adage` − `a0`: requests +0.25 [−0.17, 0.67] p 0.59; total_turns +2.5 [2, 3]
p_holm 0.003; explore_calls_before_edit +2 [1, 3] p_holm 0.011; cost +0.0049 p 0.046, p_holm 0.37.
The extra turns and exploration against `a0` are the hook arms' (the read-linked-files directive
arrives after the lookup and the agents read more), not the mod's.

Report directories: `/tmp/adage-live/results-mod-a0`, `results-mod-adage`, `results-observe`
(and `results-adage-a0`), each with `summary.json`, `state.json` and the dashboard page.

### Reading against the M4 exit criterion

`test-plan.md` §10: the `mod` arm runs end to end with `requests`, `dead_end_hits` and directives
lower than `adage`, at `hidden_passed` not lower.

- Runs end to end: **yes**, after the fixes below (60/60 sessions, verified counts, one
  `session_start` each, the mod active and asking the binary at every step).
- `requests` lower than `adage`: **no** (8.00 vs 7.83, CI spans 0). Nothing was predicted, so
  nothing could be saved.
- `dead_end_hits` lower: **no, and not testable here**: 0 in every arm. Every agent read the
  Makefile and ran `make test`/`make ci`; none ran `python3 -m unittest` directly, so the dead end
  (and its substitution, C6) never came up.
- Directives lower: **yes in number (0.83 vs 1.17), but not because of the mod.** Directive
  delivery conditional on its stimulus was identical in every treated arm: every batch that
  observed a `wi-lookup` run got `read-linked-files` (`adage` 10/10, `mod` 7/7, `mod-observe` 7/7;
  `control` 0/5 delivered, 5/5 `would_have`). The difference is the agents' path: in 3 `mod-observe`
  and 2 `mod` sessions the agent decoded `.tracker/cache.bin` itself instead of running
  `wi-lookup` (0 in `adage`), and the finish-line directive went out only in the two `adage`
  W-2046 sessions, the only ones that edited a schema with the Edit tool. `mod-observe`, which by
  design changes nothing, shows the same drop (0.75). The mod adds nothing to what the model reads
  in these sessions: the binary's `Standing` answer is `standing: ""`, `environment: ""` (only a
  compaction instruction, and no session compacted).
- `hidden_passed` not lower: **yes** (12/12 everywhere).

So the answer to the proposal's question from this run is "not shown": with the grant the brief
names, the mod had nothing to do on these tasks. Two engine facts bound the finish-line grant in
this sandbox (both in the binary, diagnosis only):

- An instinct's finish line is advice, never an obligation (`crates/adage-runtime/src/decide/batch.rs`:
  "An instinct's finish line is advice, never an obligation: no Stop request can come of it"). The
  `make gen` route here is instinct U5, so `obligation_state` stays empty, the Stop gate never
  blocks, and the end-of-turn prediction (C8, `ending: true`) has nothing to predict. The mod can
  predict it only as a pending directive at the step after the edit (`prediction.rs`,
  `predict_step`, `st.pending`).
- Most edits in this sandbox go through Bash (`python3 - <<'E' … E`), which the batch observes as
  `exec python3 -c {code}`, not as an edit of `schema/*.json`: the finish line does not arm for
  them. 20 of 60 eval sessions used an edit tool at all; 2 of the 20 schema-task sessions edited a
  schema with one (both `adage`).

The largest opportunity on these tasks is the lookup (`lookup-work-item`, 11 of 12 training
traces): it is nascent (an environment step waits for `adage review approve`), so no arm delivered
it, and it held no predict grant. The brief named only the finish-line grant; widening the grants
after seeing these numbers would be tuning the arms (§8), so it is listed as the next probe, not
done.

### The finish line, live (evaluation-plan §4.3, one probe pair)

`$S/probe_finish.py` runs `run_one` with phase `probe` (never compared) and the prompt "In
schema/invoice.json, add a field `po_number` (type string, default "") to the invoice record.
Change only that file, with the Edit tool, then stop: do not run anything."

| arm | requests (verified) | directives | predicted | evidence | generated models |
| --- | --- | --- | --- | --- | --- |
| adage | 4 (yes; 3 responses + 1 retry) | 1 (finish line, advice) | 0 | the model obeyed "do not run anything" | stale |
| mod | 4 (yes; 3 responses + 1 retry) | 1 | 1 | `turn_step` record: `predicted` `{kind: run, route: finish-line-e7e809, calls: [Bash make gen]}`, `predicted_at` index 2; transcript `tool_use` id `adage_finish-line-e7e809_<turn>_2_0`; debug log `[Stall] tool_dispatch_start tool=Bash toolUseId=adage_…`, no `[API REQUEST]` for that step | fresh |

The predicted step replaced a step the `adage` model did not take, so requests were equal; the
outcome differed (fresh generated models). A replay of the same `TurnStep` straight into
`adage-exec hook claude TurnStep` answers the prediction in 5 ms (`adage` answers nothing, as it
should).

## §2.3 Observation changes nothing

Offline: `python3 demo/replay_hooks.py demo/fixtures/hook-stream.jsonl --identity --adage
target/release/adage --seed-home /tmp/adage-live/adage-home --keep /tmp/adage-live/replay` →
`identical: 9 events, observe mode changed nothing`, the two recordings byte-identical. As
documented the replay runs in a fresh temporary directory, which is not the learned repository, so
nothing routes (no record, no directive): identical, but trivially. With `--cwd` a fresh sandbox
copy (`/tmp/adage-live/replay-cwd/acme-billing`) the replay delivers the dead-end directive at
`PostToolUseFailure` and writes 5 spool records; still byte-identical (`/tmp/adage-live/replay-sandbox`).
The replay drives `adage hook` directly, not the JS mod (P4a open issue 2 stands).

Live: `mod-observe` vs `adage` above: no outcome difference; per event class the delivered blocks
are the same given the same stimulus (all `read-linked-files` after an observed lookup in both
arms). The set of blocks per session differs only where the agents took different paths.

## Defects found and fixed

### 1. `learn` found no repositories (runner)

Every one of the first pass's 17 sessions had the same `session_id`: the id of the Claude Code
session this container runs (`CLAUDE_CODE_SESSION_ID` in its environment). The runner passed its
own environment to each `claude -p` minus `ADAGE_*`, and a child that inherits the host's session
variables takes that id as its own (removing only `CLAUDE_CODE_SESSION_ID` was not enough: the
remote-session variables carry it too). Consequences: `outcomes.json` had one key;
`transcript_for(sid)` globbed `~/.claude/projects/*/<sid>.jsonl` and returned the first match,
`~/.claude/projects/-tmp/<sid>.jsonl`, an unrelated session whose cwd was `/tmp`, for all 12
training runs; `adage condition --transcripts …` got that one file 12 times
(`sessions_read 1, sessions_used 0, repos {}`). Not the binary.

Fix (`demo/run.py`): no arm forwards the host session's variables (`host_session_variable()`, the
rule of `evals/live/runner.py`'s `forbidden_forward`: `CLAUDECODE`, `*SESSION_ID*`,
`CLAUDE_CODE_REMOTE*`, `CLAUDE_CODE_MESSAGING_*`, `CLAUDE_CODE_ENTRYPOINT`,
`CLAUDE_CODE_CHILD_SESSION`, `CLAUDE_PID`); every run passes its own `--session-id <uuid4>` (a
probe confirmed `claude -p --session-id` is honoured), stdin is empty (the CLI otherwise waits 3 s
for it), and records `session_id`, `session_id_as_asked` and its `transcript`.
`transcript_for(sid, cwd)` honours `CLAUDE_CONFIG_DIR` and looks in the sandbox's own project
directory first; `learn_inputs()` refuses runs that share a session id and names runs with no
transcript. With the host variables gone, auth still works (the host's token files), the billing
entrypoint becomes `sdk-cli`, and the host's remote tools are no longer offered to the agents.

Re-run: `run.py train` 12/12 passed, 12 distinct ids, $0.82; `run.py learn`: `learn: 12
transcripts from 12 runs`, `sessions_read 12, sessions_used 12`, `repos: {acme-billing: …}`.
`ADAGE_HOME=/tmp/adage-live/adage-home adage status`:

```text
adage status · 2 tracks · 2 reflexes · 18 instincts
acme-billing/instincts@0.1.0 · acme-billing · instincts
  PRIME ◉ U3   run `make test` · 15 triggers, e.g. `python -m pytest` fails with "make test" · forming
  PRIME ◉ U1   run `make test` · when `python3 -m unittest` fails with "ACME_ENV" · forming
  PRIME ◉ U5   run `make gen` · when an edit touches schema/*.json · forming
  PRIME ◉ U4   advise (generated-file: acme/generated/models.py) · when an edit touches acme/generated/models.py · forming
acme-billing/learned@0.1.0 · acme-billing
  PRIME ◠ lookup-work-item    11/12   lb 0.65 · nascent
  PRIME ◠ read-linked-files    9/9    lb 0.70
next  adage review · 1 needs you · 1 to look over
```

SANDBOX.md's three are there: the lookup script (`acme-billing/learned lookup-work-item`, run
`python3 scripts/wi-lookup {item}` when a prompt names a work item; `adage review`: "an
environment step: it is delivered only after a person approves it"), the finish line
(`acme-billing/instincts finish-line-e7e809`, `make gen` when an edit touches `schema/*.json`) and
the dead end (`acme-billing/instincts dead-end-d41b1e`, `python3 -m unittest` fails with "ACME_ENV"
→ `make test`, beside 15 U3 spellings). Learned besides: `read-linked-files` (read the files a
lookup lists in one turn).

### 2. `requests_verified=False` everywhere (cross-check, both copies)

The regexes match this CLI's lines. Every first-pass log had, beside N `source=sdk` dispatches,
one `source=generate_session_title` (at start) and one `source=side_query` (after the last turn,
`[dispatch] sent … (side query)`); the stream's explicit `request_id`s (on its `assistant` lines)
are exactly the N `sdk` ones. Dispatches − stream ids = 2 with no `API error` line to explain them:
unverified. (Those two sources came from the host's remote-session environment; with fix 1 they
are gone, `requests_by_source` is `sdk` only.)

Fix (`demo/adage_metrics.py` and `evals/reflex-proof/harness/heldout_analysis.py`, kept mirrors):
`STREAMED_SOURCES = ("sdk", "repl_main_thread")`; the cross-check compares the stream's ids with
the dispatches of those sources (retries still explained by `API error` lines, ids still unique).
`requests` still counts every dispatch. The harness's return shape is unchanged; the demo adds
`requests_streamed` and `requests_by_source`. Test over a captured excerpt
(`demo/fixtures/requests-2.1.292/`: the dispatch lines verbatim, the stream's assistant lines
reduced to their ids): `test_captured_claude_2_1_292_session` (9 requests, 7 streamed, 7 ids,
verified), plus `test_requests_the_stream_never_shows_explain_themselves` and the mirror test over
both. Recomputed over the 17 first-pass runs: all verified.

### 3. `hooks_wired_once` False on the hook arms (counted over a shared spool)

The first pass's one spool, `sessions/<host id>.spool`, held 40 records from four sessions (the
control, adage, mod-observe and mod smoke runs, in that order, each `session_start … session_end`
under its own `acme-billing@<root>`): exactly one `session_start` per run. Each stream has one
`hook_started SessionStart:startup`; each mod debug log one `classic.SessionStart settled`. The
engine fired SessionStart once and nothing was registered twice; control saw 1 because it ran
first, adage 2, mod-observe 3, mod 4. Fixed by 1 (one spool per session); the metric was right.
After the fix: wired once on 60/60 eval sessions.

### 4. The mod was inert under `--plugin-dir` (runner)

After the grant, the first smoke showed no prediction and no `turn_step` record. The grant,
`exec/enabled` (`bin/hook -> adage-exec`) and the bounds were all fine (a replayed `TurnStep`
predicts). The debug log had `$.process.run (cc-plugin-telemetry)` lines and not one
`$.process.run (adage)`: the mod never ran its binary, not even the presence probe, so it stayed
inert and only the command hooks of `hooks.json` ran. The mod finds `bin/hook` under
`$.env.get('CLAUDE_PLUGIN_DATA')` or a data directory derived from an installed plugin's root
(`plugins/cache/<marketplace>/<plugin>/<version>`). A `--plugin-dir` plugin has neither: the engine
substitutes `${CLAUDE_PLUGIN_DATA}` into command hooks only, it is not in the engine's process
environment. So every plugin-arm run before the fix (the first pass's included) measured the
command hooks through `adage-exec`, never the mod.

Fix (`demo/run.py`): the plugin arms set `CLAUDE_PLUGIN_DATA` in the session's environment to the
directory the engine gives an inline plugin, `<config dir>/plugins/data/<name>-inline`
(`plugin_data_dir()`; observed `~/.claude/plugins/data/adage-inline`). The mod then probes
`bin/hook version`, asks `Standing` once and `TurnStep` at every step. Also from then on
`CLAUDE_CONFIG_DIR=$DEMO_ROOT/claude-config` (evaluation-plan §0), so that data directory and its
`bin/hook` link, which every session start rewrites from its own `ADAGE_HOME`, is not shared with
other sessions on this machine.

### 5. Smaller runner and replay defects

- Smoke runs (`run.py one`) were recorded with phase `eval`, so `report.py` would have compared
  them with the interleaved runs; they are now phase `smoke` (the existing seven relabelled).
- `replay_hooks.py --adage target/release/adage` (the documented command) failed with
  `FileNotFoundError`: each hook runs in the replay's own directory. The path is now resolved.
- `replay_hooks.py` recorded a seeded home's existing spools (about 170 records of the live
  sessions) as the first event's records, so two copies of a live home could differ for that
  alone. It now records only what the replay writes.

## The `mod` arm's grant (as a person)

`$S/person.py` runs `adage` without the agent markers (`CLAUDECODE`, `OPENCODE`, `CODEX_*`,
`ADAGE_ARM`, `ADAGE_TRAIN_SESSION`) or the host-session variables, with
`ADAGE_ALLOW_NONINTERACTIVE=1` and `ADAGE_HOME=/tmp/adage-live/adage-home`, in a fresh sandbox copy
(`/tmp/adage-live/grant/acme-billing`):

```sh
adage delegate policy --max-level e2    # max level E2 · automatic off · sandbox required
adage delegate on --repo .              # writes $ADAGE_HOME/exec/enabled {bin: …/adage-exec}
adage review delegate acme-billing/instincts finish-line-e7e809 --predict --repo .
                                        # predict, for acme-billing · argv make gen · effect regenerate
adage delegate status --repo .          # on · 1 delegated reflex · finish-line-e7e809 · predict · make gen
```

No refusal; `approvals.json` holds `{repo: "acme-billing", mode: predict, level: e2, argv [make
gen]}`. One oddity (binary, diagnosis only): `adage-exec status --repo .` prints `Execution grants
for acme-billing@2911371e4cc7: 0` and no prediction grant, because it keys the repository with
`repo::repo_key` (`acme-billing@<root commit>`, `crates/adage-exec/src/main.rs:330-378`) while the
grant, like the index, is under `acme-billing` (`repo::lookup_key`, as `adage delegate status` and
the hooks use). Each sandbox copy has its own root commit (`demo/sandbox/generate.py` does not pin
the commit date), so the copies' own keys differ; the runner's `condition --repo acme-billing`
keeps one key for all of them.

## Smoke (`run.py one … --debug`, after the fixes)

| run | requests (verified) | directives | predicted | hidden |
| --- | --- | --- | --- | --- |
| W-2041 a0 | 7 (yes) | 0 | 0 | pass |
| W-2041 control | 6 (yes) | 0 (`would_have` read-linked-files) | 0 | pass |
| W-2041 adage | 8 (yes) | 1 (`read-linked-files`; `<adage-reflex` in the transcript) | 0 | pass |
| W-2041 mod-observe | 9 (yes) | 1 | 0 | pass |
| W-2041 mod | 8 (yes) | 1 | 0 | pass |
| W-2045 adage | 6 (yes) | 1 | 0 | pass |
| W-2045 mod | 6 (yes) | 1 | 0 | pass |

`hooks_wired_once` true on all; the plugin arms' `session_start.bin` is `adage-exec`. W-2041 is a
code fix in `acme/billing.py`: nothing granted applies, so `mod` < `adage` on it could not hold
(8 = 8). On W-2045 both agents wrote the schema change and ran `make gen` and `make ci` in one
Bash command, leaving nothing pending. The finish-line probe above is the live evidence that the
mod predicts.

## Not produced

- §2.1, §2.2, §2.4–§2.5, §3, §4 (but the one §4.3 probe pair), §6 and §7: not run here.
- Q9/Q15 "ten of ten": one finish-line prediction, not ten.
- The dead-end substitution (C6) and receipts as results (C3): no session reached them.
- A predict grant beyond the finish line: not issued (see the reading above).

## Files changed (repository)

- `demo/run.py`: host-session variables never forwarded; `--session-id` per run; stdin empty;
  `session_id_as_asked`, `transcript`; `claude_projects()`, `project_slug()`, `transcript_for(sid,
  cwd)`; `learn_inputs()`; `plugin_data_dir()` and `CLAUDE_PLUGIN_DATA` for the plugin arms; smoke
  runs in phase `smoke`; `requests_check` carries `requests_streamed`, `requests_by_source`.
- `demo/adage_metrics.py`, `evals/reflex-proof/harness/heldout_analysis.py`: `STREAMED_SOURCES` and
  the cross-check over them.
- `demo/replay_hooks.py`: absolute binary path; only the replay's own spool records.
- `demo/test_run_arms.py`, `demo/test_replay_hooks.py`: the tests named above;
  `demo/fixtures/requests-2.1.292/` (the captured excerpt).
- `demo/SANDBOX.md`: one session per run, the mod under `--plugin-dir`, smoke phase, the new keys.

Tests: `python3 -m unittest discover -s demo` 46 OK (6 skipped without `ADAGE_BIN`; 0 skipped with
`ADAGE_BIN=target/release/adage`); `evals/reflex-proof/harness`: `python3 -m unittest
test_heldout_analysis` 24 OK. `cargo xtask test python demo` was not run (no cargo in this task).

Scripts and logs outside the repository: `$S/live-env.sh` (the exports), `$S/person.py` (the
grant), `$S/probe_finish.py`, `$S/table.py`, `$S/live-eval.log`, `$S/report.out`; first pass in
`/tmp/adage-live-pass1`, this pass in `/tmp/adage-live` (`state.json`, `runs/<id>/claude-debug.log`,
`stream.jsonl`, `stderr.log`). `$S` is the session's scratchpad directory.

## Run 2: the study's grants (2026-10-07)

The second pre-specified live comparison: the same 60 sessions as run 1 (6 held-out tasks × 5
arms × 2, interleaved), with the grants the study names instead of the finish line alone.

### Verdict

**The M4 exit criterion is still not met, but the mod now acts.** It predicted a step in 12 of 12
`mod` sessions (the reads the work item links, yielded right after the model's own lookup: a
`turn_step` record with `predicted`, an `adage_` tool_use, no request for that step). It sent
fewer model responses than `adage` (4.08 vs 4.83, diff −0.75, 95% CI [−1.33, −0.08], p 0.027,
unadjusted and post hoc), but not fewer `requests` as the harness counts them (5.33 vs 5.25, diff
+0.08, CI [−0.67, 0.83], p 1.0): every predicted step was followed by a failed dispatch, the
server's 404 for a conversation thread it holds no state for, and a replay (12 of 12, below).
`dead_end_hits` 0 vs 0 (the dead end was hit in piped commands the metric cannot see),
directives 2.33 vs 2.42 (p 1.0), `hidden_passed` 12/12 in both, cost −0.0042 USD per session
(CI [−0.0068, −0.0013], p 0.013, p_holm 0.15).

The lookup itself could not be granted: the binary never predicts a step whose effect is
`environment`, and the learner classified the lookup script `environment` because one work item's
text contains the word "generated" (below). The study's chain was granted at its second step
(`read-linked-files`), which is what acted. Approving the lookup also made the hook arms deliver
it at the prompt, and that, not the mod, is the largest effect in run 2: `adage` − `a0` requests
−2.92 (p_holm 0.009), cost −0.019 USD (p_holm 0.002).

`mod-observe` against `adage`: no difference in any outcome metric (requests p 0.65, cost p 0.80,
pass and hidden pass 12/12 each).

### Pre-specification

Written to `$S/live-eval2-notes.md` at 00:08 UTC, before any run-2 number: the question (fewer
requests than `adage` at hidden-test quality not lower); the grants (the lookup reflex
`acme-billing/learned lookup-work-item` gets `predict`, made eligible the way the product does;
the finish-line grant kept); the arms, tasks, repeats and command; the four `report.py`
comparisons; the M4 reading; the mechanism counts per arm and the spool's step boundaries; the
inputs below; nothing about the arms or runner changed after numbers are seen. One amendment at
00:12, after the lookup's predict grant was refused and still before any run-2 number: the chain's
second step, `read-linked-files`, gets its own `predict` grant (the study's chain, architecture
§1b.1 and evaluation-plan §4.1–§4.2, keys the reads on that reflex, `plan_predict_reads`; the
binary checks the reads' route for the grant, `prediction.rs` `predict_step`).

### Machine and inputs

- As run 1: the same container, `claude` 2.1.292, the same binaries (`adage` sha256
  `a63c492cdb3f…`, `adage-exec` `117561cb84d6…`, built from `b6548a2`), the same frozen plugin
  copy, `DEMO_MODEL` `claude-sonnet-5-5`, `DEMO_CONCURRENCY=3`, `DEMO_MAX_TURNS=30`, the same
  prompts and runner (`demo/` unchanged in run 2).
- `DEMO_ROOT=/tmp/adage-live2`, `ADAGE_HOME=$DEMO_ROOT/adage-home` (a copy of run 1's learned home:
  no re-training; the copy's repository keys resolve in a fresh sandbox copy),
  `CLAUDE_CONFIG_DIR=$DEMO_ROOT/claude-config` (new).
- Spend: smoke $0.10, the comparison $3.52.

### The grants (as a person)

`$S/person2.py` (run 1's `person.py` with the new home): no agent markers, no host-session
variables, `ADAGE_ALLOW_NONINTERACTIVE=1`, `ADAGE_HOME=/tmp/adage-live2/adage-home`, in a fresh
sandbox copy (`/tmp/adage-live2/grant/acme-billing`). `adage review` listed the lookup as `why
approval: an environment step: it is delivered only after a person approves it`, `effect
environment · nascent`, `decide adage review <approve|extinguish|pin> acme-billing/learned
lookup-work-item`.

```sh
adage delegate policy --max-level e2    # max level E2 · updated
adage delegate on --repo .              # with …/exec/enabled present its hooks call adage-exec
adage review delegate acme-billing/learned lookup-work-item --predict --repo .
    # error  no reflex lookup-work-item in track acme-billing/learned for acme-billing (see `adage status`)
adage review approve acme-billing/learned lookup-work-item
    # lifecycle nascent → firm · track acme-billing/learned@0.2.0 · index gen 5653665d · 20 reflexes
adage review delegate acme-billing/learned lookup-work-item --predict --repo .
    # error  a environment step is never predicted (at most regenerate)
adage review delegate acme-billing/learned read-linked-files --predict --repo .
    # predict · argv Read {files} · input {files} must match ^[A-Za-z0-9_][A-Za-z0-9_./@+-]{0,199}$
    # effect observe (the classifier says observe) · evidence 9 of 9 traces over 1 days
adage delegate status --json --repo .
    # enabled true · predict "on" · lapsed [] · grants: instincts/finish-line-e7e809 (predict, make gen),
    #   learned/read-linked-files (predict, Read {files})
```

The two refusals and their rules (binary, diagnosis only):

- A nascent reflex is not in the index (formed in its track, delivered only after a person
  decides), and `review delegate` looks the reflex up in the index (`exec.rs` `indexed()`). The
  review verb that firms it is `approve`, which also puts it into service: from then on its
  directive is delivered at the prompt in every treated arm.
- `adage-exec/src/policy.rs` `permits_prediction`: `req.effect > Effect::Regenerate` is refused
  unconditionally, and `req.effect` is the route's learned effect, `environment` for the lookup. No
  policy key opts in (`exec.allow_environment_actions` covers only an unscripted command on a route
  whose own effect is at most regenerate), and no review verb changes a route's effect (`--effect`
  belongs to the E2 execute grant and conflicts with `--predict`). Nothing legitimate was left to
  try. Why `environment`: `adage-learn/src/effects.rs` classifies an unlisted repository script
  observe from the traces, but `infer` gives up when any run's output "reports a change", and
  W-1046's lookup prints `Acceptance: … generated models include it`; the word `generated ` in a
  work item's text is taken as the script saying it wrote something. A false negative of the
  learner on this sandbox.

The lookup lever is also bounded by "never at index 0": the lookup's directive arrives with the
prompt, and in all 36 treated sessions of the comparison the model's first call was the lookup
(in `a0` and `control` it was a `Grep` for the item), so a lookup grant could have acted only where
the model did something else first.

### Smoke (`run.py one W-2041 <arm> --debug`)

| run | requests (verified) | model responses + retries | directives | predicted | hidden |
| --- | --- | --- | --- | --- | --- |
| W-2041 adage | 8 (yes) | 6 + 2 | 3 (`lookup-work-item`, `read-linked-files`, `dead-end-305e5c`) | 0 | pass |
| W-2041 mod | 4 (yes) | 3 + 1 | 2 (`lookup-work-item`, `read-linked-files`) | 1 | pass |

The `mod` run: spool `turn_step` `predicted {kind: chain, route: read-linked-files, calls: [Read
acme/billing.py]}`, `predicted_at` index 1; transcript `tool_use` id
`adage_read-linked-files_<turn>_1_0`; debug log `adage (user) answered turn.step without next() in
14.0ms; nothing beneath it ran for this dispatch` and `turn.step: a hook answered without the
request`, no `[API REQUEST]` for that step; 4 `[API REQUEST]` lines against 8.

### Results

60 of 60 sessions finished; none left out, none censored; 60 distinct session ids, all as asked.

| arm | n | requests (mean) | model responses (mean) | retries | cost USD (mean) | passed | hidden_passed | stale_generated | dead_end_hits hit / substituted | directives (mean) | predicted_steps | receipts_as_results | requests verified | hooks wired once |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| a0 | 12 | 8.17 | 7.42 | 9 | 0.0707 | 12/12 | 12/12 | 0/12 | 0 / 0 | 0.00 | 0 | 0 | 12/12 | 12/12 (no spool) |
| control | 12 | 8.75 | 7.17 | 19 | 0.0718 | 12/12 | 12/12 | 0/12 | 0 / 0 | 0.00 | 0 | 0 | 12/12 | 12/12 |
| adage | 12 | 5.25 | 4.83 | 5 | 0.0515 | 12/12 | 12/12 | 0/12 | 0 / 0 | 2.42 | 0 | 0 | 12/12 | 12/12 |
| mod-observe | 12 | 5.58 | 4.92 | 8 | 0.0523 | 12/12 | 12/12 | 0/12 | 0 / 0 | 2.75 | 0 | 0 | 12/12 | 12/12 |
| mod | 12 | 5.33 | 4.08 | 15 | 0.0473 | 12/12 | 12/12 | 0/12 | 0 / 0 | 2.33 | 12 | 0 | 12/12 | 12/12 |

`dead_end_substituted` is 0 in every arm; `subagent_tool_calls` 0. All 24 plugin-arm sessions were
served by `adage-exec`.

Requests per task (repeat 1, repeat 2), and model responses in brackets:

| task | a0 | control | adage | mod-observe | mod |
| --- | --- | --- | --- | --- | --- |
| W-2041 | 9, 10 [8, 10] | 9, 12 [7, 8] | 4, 5 [4, 5] | 4, 5 [4, 4] | 4, 4 [3, 3] |
| W-2042 | 10, 7 [8, 7] | 7, 10 [6, 8] | 4, 4 [4, 4] | 4, 6 [4, 4] | 4, 4 [3, 3] |
| W-2043 | 9, 7 [9, 7] | 11, 10 [7, 8] | 9, 10 [7, 8] | 5, 8 [5, 6] | 9, 7 [6, 6] |
| W-2044 | 6, 8 [6, 7] | 7, 7 [7, 7] | 5, 4 [5, 4] | 8, 6 [8, 5] | 5, 4 [4, 3] |
| W-2045 | 7, 9 [6, 7] | 7, 8 [7, 7] | 5, 4 [4, 4] | 6, 5 [6, 4] | 5, 7 [4, 5] |
| W-2046 | 8, 8 [6, 8] | 7, 10 [6, 8] | 4, 5 [4, 5] | 5, 5 [5, 4] | 6, 5 [5, 4] |

`demo/report.py` (as run 1: task-stratified permutation p, task bootstrap 95% interval, Holm across
the outcome metrics, mechanism counts unadjusted; medians for the turn and time metrics, means
otherwise; diff is a − b):

| metric | `mod` − `a0` | `mod` − `adage` | `mod-observe` − `adage` | `adage` − `a0` |
| --- | --- | --- | --- | --- |
| requests | −2.83 [−4.25, −1.50] p 0.0009, p_holm 0.006 | +0.08 [−0.67, 0.83] p 1.0, p_holm 1 | +0.33 [−1.17, 1.50] p 0.65, p_holm 1 | −2.92 [−4.25, −1.08] p 0.0014, p_holm 0.009 |
| cost_usd | −0.0234 [−0.0298, −0.0170] p 0.0001, p_holm 0.001 | −0.0042 [−0.0068, −0.0013] p 0.013, p_holm 0.15 | +0.0008 [−0.0056, 0.0073] p 0.80, p_holm 1 | −0.0192 [−0.0248, −0.0127] p 0.0002, p_holm 0.002 |
| pass_rate | 0 (1 vs 1) | 0 | 0 | 0 |
| hidden_pass_rate | 0 (1 vs 1) | 0 | 0 | 0 |
| stale_generated_rate | 0 (0 vs 0) | 0 | 0 | 0 |
| dead_end_hits | 0 | 0 | 0 | 0 |
| total_turns | −2 [−4, 0] p 0.002, p_holm 0.011 | 0 [−1, 0.5] p 0.57 | +0.5 [−1, 1] p 1.0 | −2 [−4, −0.5] p 0.001, p_holm 0.009 |
| turns_to_first_edit | −4 [−4.5, −1] p 0.0001, p_holm 0.001 | 0 [0, 1] p 0.50 | 0 [−0.5, 0] p 0.67 | −4 [−4.5, −1.5] p 0.0001, p_holm 0.001 |
| explore_calls_before_edit | −2 [−2, 0] p 0.089, p_holm 0.44 | 0 [0, 1] p 0.50 | 0 [0, 1] p 0.67 | −2 [−2, 0] p 0.034, p_holm 0.17 |
| time_to_first_edit_s | −4.75 [−7.94, −2.82] p 0.0001, p_holm 0.001 | +0.78 [−0.90, 3.51] p 0.13 | +1.56 [−1.84, 2.05] p 0.26 | −5.53 [−8.79, −3.59] p 0.0001, p_holm 0.001 |
| elapsed_s | −5.65 [−6.84, −2.15] p 0.0004, p_holm 0.003 | +0.63 [−1.38, 2.77] p 0.57 | +2.02 [−1.46, 3.29] p 0.30 | −6.28 [−7.76, −3.69] p 0.0005, p_holm 0.004 |
| directives | +2.33 [2.08, 2.67] p 0.0001 | −0.08 [−0.33, 0.17] p 1.0 | +0.33 [−0.08, 0.75] p 0.22 | +2.42 [2.17, 2.67] p 0.0001 |
| predicted_steps | +1 [1, 1] p 0.0001 | +1 [1, 1] p 0.0001 | 0 | 0 |
| receipts_as_results | 0 | 0 | 0 | 0 |
| dead_end_substituted | 0 | 0 | 0 | 0 |

With six tasks the task-level sign-flip p (`p_tasks`) cannot fall below 0.031; it is 0.031 for
every `mod` − `a0` and `adage` − `a0` row above with p_holm < 0.05 except requests (0.0625), and
1.0 for `mod` − `adage` requests.

Post hoc, not pre-specified (`$S/supp2.py`, the same statistics, unadjusted): `requests` split into
model responses (the stream's distinct request ids) and retried dispatches.

| split | `mod` − `a0` | `mod` − `adage` | `mod-observe` − `adage` | `adage` − `a0` |
| --- | --- | --- | --- | --- |
| model responses | −3.33 [−4.58, −2.25] p 0.0001 | −0.75 [−1.33, −0.08] p 0.027 | +0.08 [−0.92, 1.08] p 1.0 | −2.58 [−3.50, −1.67] p 0.0003 |
| retried dispatches | +0.50 [0.08, 1.17] p 0.23 | +0.83 [0.50, 1.00] p 0.004 | +0.25 [−0.33, 0.67] p 0.58 | −0.33 [−0.92, 0.67] p 0.49 |

Report directories: `/tmp/adage-live2/results-mod-a0`, `results-mod-adage`, `results-observe`,
`results-adage-a0`.

### What the mod did at each step boundary (the spools)

- `mod`: one `turn_step` record in each of the 12 sessions, all `predicted {kind: chain, route:
  read-linked-files}` at request index 1, right after the model's own `wi-lookup` (index 0's
  response): `Read acme/billing.py` (W-2041, W-2043), `Read acme/customers.py` (W-2042, W-2044),
  `Read schema/invoice.json` + `Read acme/billing.py` in one response (W-2045), `Read
  schema/customer.json` + `Read acme/customers.py` (W-2046). Every transcript holds the
  `adage_read-linked-files_<turn>_1_<n>` ids; every debug log has one `answered turn.step without
  next()`. The following `PostToolBatch` records the judgement `predicted`, never `followed`. No
  other step was predicted: no `mod` session left the finish line pending at a step boundary (W-2045
  r2 edited the schema with the Edit tool and ran `make gen && make ci` in the same response; the
  other schema sessions edited through a Bash heredoc, which does not arm it). No session reached
  the cap, a second chain step, or a lookup prediction.
- `mod-observe`: 14 records, all `would_predict`: the same 12 chains at index 1, and `run
  finish-line-e7e809 [Bash make gen]` at index 4 (W-2045 r1) and 3 (W-2046 r1), after an Edit-tool
  schema edit alone in its batch; the model then ran `make gen && make ci …` itself.
- `adage`, `control`, `a0`: no `turn_step` (the hooks do not ask it).

Were the predictions the steps the model would have taken? In `adage` and `mod-observe` the
model's own response after the lookup was exactly the predicted read on W-2041..W-2044 (8 of 8
sessions in each arm): there the prediction replaced a whole response, and each of the 8 `mod`
sessions on those tasks used one or two model responses fewer than the `adage` session of the same
task and repeat. On W-2045 and
W-2046 that response also listed directories and read more files (4 of 4 in each arm): the
prediction replaced part of it, the model made the rest in its next response, and nothing was
saved (model responses 4, 5 and 5, 4 against `adage`'s 4, 4 and 4, 5).

Why `requests` did not fall: each of the 12 yielded responses was followed by `[ERROR] API error
(attempt 1/11): 404 … No thread state was found for the requested previous_message_id … Replay the
full conversation with thread: {"type": "create"}`, then `[tether] forcing create for retry:
replay_not_found` and the replayed dispatch about 0.1 s later. As the error says, the CLI's
request continues a server-side thread from the previous response (`previous_message_id`); a
response the mod yielded never came from the server, so the request after it names a thread state
that does not exist. Outside predicted steps the same miss occurs at roughly 8–22% of responses in
every arm (a0 9 of 89, control 19 of 86, adage 5 of 58, mod-observe 8 of 59, mod 3 of the 37
responses not right after a prediction), the server-side noise run 1 saw; right after a predicted
step it occurred 12 of 12 times. So under this CLI each prediction trades one model response for
one failed dispatch that generates nothing, and `requests`, which counts dispatches (the harness's
definition), cannot show the saving. This is an engine and API interaction (yielded `turn.step` responses under the CLI's
thread mode), recorded here, not changed.

### Reading against the M4 exit criterion

- Runs end to end: **yes** (60/60, verified counts, one `session_start` per hook-arm session, the
  plugin arms served by `adage-exec`, the mod predicting in every `mod` session).
- `requests` lower than `adage`: **no** (5.33 vs 5.25, p 1.0). Model responses are lower (4.08 vs
  4.83, p 0.027 unadjusted, post hoc), offset by the thread miss each prediction causes.
- `dead_end_hits` lower: **not testable by this metric**. It is 0 in every arm, yet the dead end
  was hit: the spool shows dead-end directives delivered after `python3 -m pytest` printed "No
  module named pytest" (`adage` 3 deliveries, `mod-observe` 5, `mod` 3). Every attempt was a
  compound, piped command (`ls; python3 -m pytest -q 2>&1 | tail -5`) whose exit status is
  `tail`'s, so the runner's `verify-fail` never counts it, and none was substituted (no `pretool`
  record in any spool). `a0` and `control` never tried pytest: every one of their sessions read
  the Makefile and ran `make test` or `make ci` (treated arms: 4–5 of 12 read the Makefile).
- Directives lower: **no** (2.33 vs 2.42, p 1.0). The prediction comes after `read-linked-files`
  is delivered at `PostToolBatch`, so it removes no directive.
- `hidden_passed` not lower: **yes** (12/12 everywhere).

### Differences from run 1

- **The lookup is approved**, so every treated arm (`adage`, `mod-observe`, `mod`) receives its
  directive with the prompt; `control` records it as `would_have`. This moved the hook arms far more
  than anything the mod did: `adage` requests 7.83 → 5.25, cost 0.0708 → 0.0515, turns to the first
  edit 7 → 2, while `a0` (7.58 → 8.17) and `control` (8.67 → 8.75) stayed put. Every session in every
  arm ran `wi-lookup` itself (run 1: 5 sessions decoded `.tracker/cache.bin` instead).
- **The grants**: run 1 had the finish line only; run 2 adds `read-linked-files` (predict) and
  tried, and was refused, the lookup. The learned track is 0.2.0 after the approval; nothing was
  re-learned.
- **The mod acted**: 12 predicted steps in 12 sessions (run 1: none, and no `turn_step` record in
  any spool). Run 1's directive drop in the plugin arms (a different path, not the mod) is gone:
  `mod` and `mod-observe` directives are within 0.33 of `adage`.
- **The thread miss** is new to run 2 because only run 2 yielded responses in the eval; run 1's one
  finish-line probe showed the same pattern, unremarked then: its one retry was the request right
  after the predicted `make gen` (`/tmp/adage-live/runs/probe-finish-mod/claude-debug.log`).
- **The dead end** came up in run 2 (treated agents went from the lookup straight to the files and
  guessed `python3 -m pytest`), never in run 1, and still escapes `dead_end_hits`.
- Same binaries, plugin copy, model, prompts, repeats, concurrency and runner; new `DEMO_ROOT` and
  `CLAUDE_CONFIG_DIR`.

### Not produced (run 2)

- A lookup prediction (refused by the binary, above) and therefore the two-step chain of §4.2
  (lookup then reads in a row) and its "requests fall by at least two".
- A finish-line prediction in the `mod` arm (no opportunity; two in `mod-observe` as `would_predict`).
- Receipts as results, dead-end substitution: none.

Scripts and logs (`$S` the session's scratchpad directory): `$S/live-env2.sh`, `$S/person2.py`,
`$S/grant2-a.txt`–`grant2-c.txt` (the grant commands' output), `$S/smoke2.log`,
`$S/live-eval2.log`, `$S/table2.out`, `$S/report2.out`, `$S/steps2.py` and `steps2.out` (the step
boundaries), `$S/supp2.py` and `supp2.out` (the split), `$S/live-eval2-notes.md` (the full log);
the run in `/tmp/adage-live2` (`state.json`, `runs/<id>/claude-debug.log`, `stream.jsonl`, the
spools under `adage-home/sessions/`).

## Run 3: the fixes since run 2 (2026-10-07)

The third pre-specified live comparison: the same 60 sessions (6 held-out tasks × 5 arms × 2,
interleaved), after the fixes run 2 led to (`impl/live-fixes-notes.md`, `impl/live-fixes-2-notes.md`,
`impl/O0-fixes-notes.md`), with fresh training and learning under the new learner.

### Verdict

**The M4 exit criterion is not met.** The fixes work as intended live: the lookup learned as
`effect: observe` and took its predict grant with no refusal; the mod predicted 14 steps in the 12
`mod` sessions (12 chain reads, 2 finish lines after a Bash heredoc edit of a schema); no request
failed after any of them (no 404, no 400, no retry anywhere in the 60 sessions); every predicted
call carried the "Adage made this call in your place" line and no model apologised for it. But
`mod` sent exactly as many requests as `adage` (5.58 vs 5.58, diff 0, 95% CI [−0.50, 0.50], p 1.0):
the predicted read replaced the whole of the model's next response in only 2 of 12 `adage` sessions
(run 2: 8 of 12), and where the model would have read more or listed the tree in that response, it
did so in the next one. `dead_end_hits` were higher, not lower (1.08 vs 0.50, p 0.12, p_holm 1),
and `mod-observe`, which predicts nothing, shows the same rise (1.17); directives 4.33 vs 3.67
(p 0.14), higher because more dead ends were hit; `hidden_passed` 12/12 in both. The lookup itself
was never predicted: the mod and the binary never predict at index 0, and in 36 of 36 treated
sessions the model's first call was the lookup.

`mod-observe` against `adage`: no difference in requests (p 0.39) or pass and hidden pass (12/12
each); dead-end hits +0.67 (p 0.042, p_holm 0.46), directives +0.83 (p 0.042), the same effect as in
`mod`. The identity replay is byte-identical.

### Pre-specification

Written to `$S/live-eval3-notes.md` at 01:59 UTC, before any run-3 number: the question (fewer
requests than `adage`, with fewer dead-end hits and directives, at `hidden_passed` not lower); the
inputs below; fresh `train` and `learn` (the lookup expected to learn as observe); the grants: the
lookup reflex (`lookup-work-item --predict`, after `adage review approve` if nascent), the chain of
reads (`read-linked-files --predict`) and the finish line (`finish-line-e7e809 --predict`), with
`adage delegate policy --max-level e2` and `adage delegate on --repo .`, as a person in a fresh
sandbox copy, `adage delegate status --json` listing the three; the smoke on W-2041 in `adage` and
`mod` (the `mod` run to show the lookup predicted before the model's first call, then the chain of
reads, `predicted` records, no 404 after a predicted step, fewer `[API REQUEST]` lines than
`adage`); the 60-session comparison; the four `report.py` comparisons; the identity replay;
nothing about the arms, prompts or runner changed after numbers are seen.

### Machine and inputs

- As runs 1 and 2: the same container, `claude` 2.1.292, `DEMO_MODEL` `claude-sonnet-5-5`,
  `DEMO_CONCURRENCY=3`, `DEMO_MAX_TURNS=30`, the same prompts.
- Binaries rebuilt at `abec2d2`: `adage` sha256 `9ba8b2b16f21…`, `adage-exec` `fe8f6937b716…`,
  `adage-train` `88e7972d2d34…`.
- The plugin frozen from `78d0934` (`git archive 78d0934 plugin/claude-code` plus the tree's `dist/`),
  outside the repository, as `DEMO_PLUGIN_DIR`.
- `DEMO_ROOT=/tmp/adage-live3` (new), `ADAGE_HOME=$DEMO_ROOT/adage-home` (trained and learned fresh),
  `CLAUDE_CONFIG_DIR=$DEMO_ROOT/claude-config`.
- **Runner change, before any run-3 session:** `demo/run.py` passed no `--permission-mode`; it now
  passes `--permission-mode default` to every `claude -p` (one argument pair and a comment). The mod
  yields a predicted call only when the person's permissions allow it unasked, and a host whose
  sessions start in auto mode would hold every predicted Bash step. Run 2's sessions had resolved to
  `default` anyway (their init lines); all 74 run-3 sessions carry the flag and `permissionMode:
  default`. `python3 -m unittest discover -s demo`: 48 OK, 6 skipped.
- Spend: training $0.85, smoke $0.18, the comparison $3.91; $4.94 in all.

### Training and learning

`python3 demo/run.py train`: 12/12 passed, 12 distinct session ids. `python3 demo/run.py learn`:
`learn: 12 transcripts from 12 runs`, `sessions_read 12, sessions_used 12`. `adage status`:

```text
acme-billing/learned@0.1.0 · acme-billing
  PRIME      ◠ card-e260afc3b1      5/5    lb 0.00 · forming
  FIRE       ◠ lookup-work-item    11/12   lb 0.65
                unaided: agents took this step unaided in 11 of 12 traces
  PRIME      ◠ read-linked-files    5/7    lb 0.36
                unaided: agents took this step unaided in 5 of 7 traces
```

The lookup's route is `effect: observe`, `strength: fire`, `state: firm`, step `scripts/wi-lookup
{item}`: not nascent, so it needs no review and is delivered at the prompt in every treated arm.
`read-linked-files` is firm, observe, at PRIME (5 of 7 traces; run 1's learning had 9 of 9). New
with this learner: a procedure card (`card-e260afc3b1`, forming, PRIME), delivered at the prompt in
every treated session. The instincts are run 1's (finish line U5, the U3/U1 dead ends, U4).

### The grants (as a person)

`$S/person3.py` (no agent markers, no host-session variables, `ADAGE_ALLOW_NONINTERACTIVE=1`,
`ADAGE_HOME=/tmp/adage-live3/adage-home`), in a fresh sandbox copy
(`/tmp/adage-live3/grant/acme-billing`). No refusal; no `review approve` was needed.

```sh
adage delegate policy --max-level e2    # max level E2 · updated · automatic off · sandbox required
adage delegate on --repo .              # with …/exec/enabled present its hooks call adage-exec
adage review delegate acme-billing/learned lookup-work-item --predict --repo .
    # predict · argv scripts/wi-lookup {item} · input {item} must match ^(?:W\-[0-9]{4})$
    # effect observe (the classifier says environment) · script scripts/wi-lookup sha256 72f9277f…
    # evidence 11 of 12 traces over 1 days
adage review delegate acme-billing/learned read-linked-files --predict --repo .
    # predict · argv Read {files} · effect observe (the classifier says observe) · evidence 5 of 7
adage review delegate acme-billing/instincts finish-line-e7e809 --predict --repo .
    # predict · argv make gen · effect regenerate · evidence 0 of 0 traces, by origin {"declared": 1}
adage delegate status --json --repo .
    # enabled true · predict "on" · lapsed [] · duplicates [] · grants under acme-billing:
    #   instincts/finish-line-e7e809 (make gen), learned/lookup-work-item (scripts/wi-lookup {item}),
    #   learned/read-linked-files (Read {files}), each mode predict, level e2
```

`adage-exec status --repo .` now lists the same three (`Prediction grants for acme-billing: 3`; the
repository-key oddity of runs 1 and 2 is gone). One note: at delegate time the command table says
`environment` for `scripts/wi-lookup`; the learned route's `observe` is what the grant records. The
script's digest is the same in every generated sandbox copy, so the grant did not lapse.

### Smoke (`run.py one W-2041 <arm> --debug`)

| run | requests (verified) | retries | directives | predicted | dead_end_hits | hidden |
| --- | --- | --- | --- | --- | --- | --- |
| W-2041 adage | 7 (yes) | 0 | 5 | 0 | 2 | pass |
| W-2041 mod | 5 (yes) | 0 | 5 | 1 | 1 | pass |

The `mod` run: spool `turn_step` `predicted {kind: chain, route: read-linked-files, calls: [Read
acme/billing.py]}` at index 1; transcript id `adage_read-linked-files_<turn>_1_0`; debug log
`adage (user) answered turn.step without next() in 23.7ms`; no `API error` or `[tether]` line; 5
`[API REQUEST]` lines against 7. **One pre-specified point failed:** the lookup was not predicted.
The model's own first call was `scripts/wi-lookup W-2041`, at index 0's response, and neither the
mod (`hooks/step.ts` `turnStep` forwards `e.index === 0`) nor the binary (`prediction.rs`
`predict_step` returns at `index == 0`) predicts at index 0 (T-C1-02, plan C1: "never at index 0").
The lookup's directive arrives with the prompt, the model takes it at once, and nothing is pending
at index 1. That is a design rule of these binaries and this plugin, not a fault of the run; the
comparison went ahead as pre-specified.

### Results

60 of 60 sessions finished; none left out, none censored; 60 distinct session ids, all as asked.

| arm | n | requests (mean) | model responses (mean) | retries | cost USD (mean) | passed | hidden_passed | stale_generated | dead_end_hits hit / substituted | directives (mean) | predicted_steps | receipts_as_results | requests verified | hooks wired once |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| a0 | 12 | 6.83 | 6.83 | 0 | 0.0672 | 12/12 | 12/12 | 0/12 | 0 / 0 | 0.00 | 0 | 0 | 12/12 | 12/12 (no spool) |
| control | 12 | 7.33 | 7.33 | 0 | 0.0714 | 12/12 | 12/12 | 0/12 | 0 / 0 | 0.00 | 0 | 0 | 12/12 | 12/12 |
| adage | 12 | 5.58 | 5.58 | 0 | 0.0608 | 12/12 | 12/12 | 0/12 | 6 / 0 | 3.67 | 0 | 0 | 12/12 | 12/12 |
| mod-observe | 12 | 5.92 | 5.92 | 0 | 0.0643 | 12/12 | 12/12 | 0/12 | 14 / 0 | 4.50 | 0 | 0 | 12/12 | 12/12 |
| mod | 12 | 5.58 | 5.58 | 0 | 0.0625 | 12/12 | 12/12 | 0/12 | 13 / 0 | 4.33 | 14 | 0 | 12/12 | 12/12 |

`dead_end_hits` is the new metric (live-fixes-2 §4: a verification whose result is an error or
carries failure text, so piped commands count); hits were in 4 `adage` sessions, 7 `mod-observe`, 9
`mod`. `dead_end_substituted` is 0 in every arm; `subagent_tool_calls` 0. Model responses are the
stream's request ids, so they exclude the 14 responses the mod made (the `mod` transcripts hold 6.75
responses per session, 5.58 of them from the model). All 24 plugin-arm sessions were served by
`adage-exec`.

Requests per task (repeat 1, repeat 2; no retries, so these are also model responses):

| task | a0 | control | adage | mod-observe | mod |
| --- | --- | --- | --- | --- | --- |
| W-2041 | 6, 7 | 7, 9 | 5, 7 | 6, 7 | 5, 6 |
| W-2042 | 9, 7 | 8, 8 | 6, 5 | 6, 7 | 5, 6 |
| W-2043 | 7, 7 | 7, 7 | 7, 7 | 6, 7 | 6, 6 |
| W-2044 | 8, 6 | 8, 8 | 5, 6 | 5, 7 | 7, 6 |
| W-2045 | 6, 6 | 7, 7 | 4, 5 | 5, 5 | 5, 5 |
| W-2046 | 7, 6 | 6, 6 | 5, 5 | 5, 5 | 5, 5 |

`demo/report.py` (as runs 1 and 2: task-stratified permutation p, task bootstrap 95% interval, Holm
across the outcome metrics, mechanism counts unadjusted; medians for the turn and time metrics,
means otherwise; diff is a − b):

| metric | `mod` − `a0` | `mod` − `adage` | `mod-observe` − `adage` | `adage` − `a0` |
| --- | --- | --- | --- | --- |
| requests | −1.25 [−1.83, −0.83] p 0.002, p_holm 0.017 | 0 [−0.50, 0.50] p 1.0, p_holm 1 | +0.33 [−0.08, 0.67] p 0.39, p_holm 1 | −1.25 [−1.83, −0.58] p 0.005, p_holm 0.043 |
| cost_usd | −0.0047 [−0.0089, −0.0018] p 0.036, p_holm 0.25 | +0.0018 [−0.0029, 0.0069] p 0.41, p_holm 1 | +0.0035 [0.0010, 0.0060] p 0.064, p_holm 0.51 | −0.0065 [−0.0112, −0.0012] p 0.015, p_holm 0.11 |
| pass_rate | 0 (1 vs 1) | 0 | 0 | 0 |
| hidden_pass_rate | 0 (1 vs 1) | 0 | 0 | 0 |
| stale_generated_rate | 0 (0 vs 0) | 0 | 0 | 0 |
| dead_end_hits | +1.08 [0.67, 1.50] p 0.001, p_holm 0.010 | +0.58 [0.17, 1.17] p 0.12, p_holm 1 | +0.67 [0.17, 1.08] p 0.042, p_holm 0.46 | +0.50 [0.08, 1.00] p 0.086, p_holm 0.51 |
| total_turns | 0 [−1, 1] p 1.0 | +1 [−1, 1] p 0.37 | +1 [0, 1.5] p 0.044, p_holm 0.46 | −1 [−1, 0.5] p 0.19 |
| turns_to_first_edit | −3.5 [−4, 0] p 0.002, p_holm 0.015 | −0.5 [−1, 2] p 0.47 | 0 [−0.5, 2] p 0.20 | −3 [−3, −2] p 0.0001, p_holm 0.001 |
| explore_calls_before_edit | −1 [−2, 0] p 0.066, p_holm 0.33 | −0.5 [−1, 0] p 0.085, p_holm 0.94 | 0 [−0.5, 1] p 0.62 | −0.5 [−1, 0.5] p 0.41 |
| time_to_first_edit_s | −5.95 [−8.43, −2.45] p 0.0005, p_holm 0.006 | −0.29 [−2.42, 3.60] p 0.19 | +0.52 [−1.11, 2.46] p 0.084 | −5.66 [−9.83, −4.23] p 0.0001, p_holm 0.001 |
| elapsed_s | −3.96 [−5.46, −0.96] p 0.044, p_holm 0.27 | +0.61 [−1.35, 2.32] p 0.23 | +2.23 [−0.10, 5.02] p 0.047, p_holm 0.46 | −4.57 [−6.36, −2.25] p 0.0006, p_holm 0.005 |
| directives | +4.33 [4.08, 4.67] p 0.0001 | +0.67 [0.17, 1.25] p 0.14 | +0.83 [0.33, 1.33] p 0.042 | +3.67 [3.33, 4.00] p 0.0001 |
| predicted_steps | +1.17 [1, 1.33] p 0.0001 | +1.17 [1, 1.33] p 0.0001 | 0 | 0 |
| receipts_as_results | 0 | 0 | 0 | 0 |
| dead_end_substituted | 0 | 0 | 0 | 0 |

`p_tasks` (the task-level sign flip, floor 0.031 with six tasks) is 1.0 for `mod` − `adage`
requests and 0.125 for its dead-end hits. Report directories: `/tmp/adage-live3/results-mod-a0`,
`results-mod-adage`, `results-observe`, `results-adage-a0`.

### What the mod did at each step boundary (the spools)

- `mod`: 14 `turn_step` records, all `predicted`, each yielded (`answered turn.step without next()`)
  and none followed by a failed request.
  - 12 × `predicted {kind: chain, route: read-linked-files}` at request index 1, one per session,
    right after the model's own `scripts/wi-lookup` (index 0): `Read acme/billing.py` (W-2041,
    W-2043), `Read acme/customers.py` (W-2042, W-2044), `Read schema/invoice.json` + `Read
    acme/billing.py` (W-2045), `Read schema/customer.json` + `Read acme/customers.py` (W-2046).
  - 2 × `predicted {kind: run, route: finish-line-e7e809, calls: [Bash make gen]}` at index 4
    (W-2045 r2, W-2046 r1), after a `cat Makefile; python3 - <<'E' … E` that rewrote the schema: the
    Bash edit now arms the finish line (live-fixes §3), `make gen` ran as
    `adage_finish-line-e7e809_<turn>_4_0` (`is_error: false`) under default mode's allow, and the
    model then ran `make ci` itself. Its other schema sessions ran `make gen` in the editing command.
  - No lookup prediction (index 0, above); no second chain step; no cap; no permission hold (no
    `withheld` in any debug log).
  - Every predicted call's result carried the context line "Adage made this call in your place …"
    (12 of 12 eval transcripts); no transcript has "apolog" or "sorry". The request after a
    predicted step read the prompt cache as usual (e.g. W-2041 r1: 16,805 tokens read, 999 written).
- `mod-observe`: 12 `would_predict` chain reads at index 1, the same calls; no finish-line
  opportunity.
- `adage`, `control`, `a0`: no `turn_step` (the hooks do not ask it).

Were the predictions the steps the model would have taken? Only in part. In `adage` the model's own
response after the lookup was exactly the predicted read in 2 of 12 sessions (W-2041 r2, W-2043 r1),
in `mod-observe` in 2 of 12 (W-2041 r1, r2). Elsewhere that response also read the test file or the
other module (`Read test_billing.py`, `Read billing.py`, `cat acme/billing.py && ls . tests`) or,
on the schema tasks, listed the tree (`ls -R`, `git ls-files`). In `mod` the model made that
remainder in its next response: each prediction removed a response's worth of calls the model
would have batched anyway, not a response. Model responses per session were equal (5.58 vs 5.58),
and on W-2044 r1 the `mod` model took 7 responses against `adage`'s 5 and 6 (it read
`tests/test_billing.py` before finding `tests/test_customers.py`). The finish-line predictions saved nothing either: in `adage` the
equivalent sessions ran `make gen && make ci` in one command after the edit.

### Reading against the M4 exit criterion

- Runs end to end: **yes** (60/60, verified counts, one `session_start` per hook-arm session, the
  plugin arms served by `adage-exec`, the mod predicting in every `mod` session, no failed request).
- `requests` lower than `adage`: **no** (5.58 vs 5.58, diff 0, p 1.0). The thread miss of run 2 is
  gone (0 retries after 14 predictions; 0 retries anywhere), so each prediction now costs no
  dispatch, but it saves none either on these tasks: the read it makes is part of a response the
  model still sends.
- `dead_end_hits` lower: **no** (1.08 vs 0.50, p 0.12, p_holm 1; higher). Every hit was
  `python3 -m unittest discover … | tail` (or `| head`) failing with "ACME_ENV", then sometimes a
  second unittest attempt after the dead-end directive, then `make test`. No prediction touches that
  path (no grant covers it, and the substitution matches a whole command, never a piped segment,
  so `dead_end_substituted` is 0), and `mod-observe`, which changes nothing, shows the same rise
  (1.17): this is a plugin-arm versus hook-arm difference in how often the agent tries unittest
  before `make test` (7–9 of 12 sessions against 4), not the mod's predictions.
- Directives lower: **no** (4.33 vs 3.67, p 0.14). Lookup, card and read directives are 12 each and
  the finish line 4 in every treated arm; the difference is dead-end deliveries (`adage` 4, `mod`
  12, `mod-observe` 14), following the hits.
- `hidden_passed` not lower: **yes** (12/12 everywhere).

### Differences from run 2

- **No failed request after a prediction**, and none at all: run 2 had a 404 thread miss after 12 of
  12 predicted steps plus 6–22% per response elsewhere (56 retries in 60 sessions); run 3 has 0
  retries in 60 sessions, 14 predictions included. The null stop is consistent with the smoke and
  every predicted step; the background rate going to zero is the server's, not ours to claim.
- **The lookup is learned as observe and granted predict** (run 2: environment, refused). It is
  never predicted: the model calls it at index 0, where nothing is ever predicted.
- **Fresh learning** with the new learner: the lookup is FIRE and firm (run 2: nascent until
  approved; delivered in both runs), its step `scripts/wi-lookup {item}` (run 2: `python3
  scripts/wi-lookup {item}`); `read-linked-files` from 5 of 7 traces (run 2: 9 of 9); a procedure
  card is delivered at every treated prompt (new).
- **The finish line acted in `mod`** (2 predictions, after Bash heredoc edits; run 2: none in `mod`).
- **The model's response after the lookup changed**: it was exactly the predicted read in 8 of 12
  `adage` sessions in run 2 and 2 of 12 in run 3, which is why model responses no longer fall
  (run 2: 4.08 vs 4.83; run 3: 5.58 vs 5.58).
- **`dead_end_hits` counts piped dead ends** (run 2 scored 0 everywhere; rescored, 4/5/3 in the
  treated arms); in run 3 they are hit more often in the plugin arms.
- **`--permission-mode default` is pinned** in the runner (run 2 resolved to it without the flag).
- `a0` and `control` also fell (8.17 → 6.83, 8.75 → 7.33), mostly the vanished retries (run 2: 9 and
  19). The hook arms' lead over `a0` is smaller (−1.25 requests, p_holm 0.043; run 2 −2.92).
- New binaries (`abec2d2`), plugin (`78d0934`), `DEMO_ROOT` and `CLAUDE_CONFIG_DIR`; same model,
  prompts, repeats, concurrency.

### Not produced (run 3)

- A lookup prediction, and so the two-step chain of evaluation-plan §4.2 (lookup then reads in a
  row) and its "requests fall by at least two": the index-0 rule leaves the lookup no step where it
  is due and the model has not already taken it.
- Receipts as results, dead-end substitution: none.

### §2.3 Observation changes nothing (replay)

`python3 demo/replay_hooks.py demo/fixtures/hook-stream.jsonl --identity --adage target/release/adage
--seed-home /tmp/adage-live3/adage-home --keep /tmp/adage-live3/replay` → `identical: 9 events,
observe mode changed nothing`; with `--cwd` a fresh sandbox copy (`/tmp/adage-live3/replay-sandbox`)
the replay delivers `dead-end-b8ac50` and `dead-end-d41b1e` at `PostToolUseFailure` and writes 5
spool records, and the two recordings are byte-identical (9,557 bytes each).

Scripts and logs (`$S` the session's scratchpad directory): `$S/live-env3.sh`, `$S/person3.py`,
`$S/grant3-a.txt`, `grant3-b.txt` (status, review and the grant commands' output), `$S/train3.log`,
`$S/learn3.log`, `$S/smoke3.log`, `$S/live-eval3.log`, `$S/table3.out`, `$S/report3.out`,
`$S/steps3.out` (the step boundaries), `$S/seq3.py` and `seq3.out` (each treated session's
responses), `$S/replay3.out`, `$S/live-eval3-notes.md` (the full log); the run in
`/tmp/adage-live3` (`state.json`, `runs/<id>/claude-debug.log`, `stream.jsonl`, the spools under
`adage-home/sessions/`).
