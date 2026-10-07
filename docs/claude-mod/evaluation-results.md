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
