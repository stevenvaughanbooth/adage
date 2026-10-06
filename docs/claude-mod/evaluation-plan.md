# Evaluation and testing plan for the Claude Code mod

A brief for the person or agent running the evaluation on a machine with a real Claude Code
(2.1.288 or later), an Anthropic key or a logged-in `claude`, and a terminal. Everything on this
page that could run offline has already run on the build machine and is green (§1 says what, and
what was re-blessed); nothing here has run in a live Claude Code session, because the build
machine had no key. The purpose of the evaluation is to answer the question the proposal asks
(`architecture.md` §1b): does the mod make Adage better at its core job, measured as fewer model
requests, fewer tool runs and fewer directives per task at unchanged hidden-test quality, and
does it do so without changing what the command hooks already do.

Work the sections in order. Each stops on a failure; a failure is a defect to fix in the branch,
not a reason to skip the section (`test-plan.md` §9: flakes are not re-run to green). Record the
outcome of every section in `docs/claude-mod/evaluation-results.md` in the format of §9.

The branch is `claude/mods-poc-m98u3o` on `stevenvaughanbooth/adage`. The implementation notes
per chunk are under `docs/claude-mod/impl/`; each has a "Live-only" section that this plan
collects in §4.

## 0. Setup (once)

```sh
git fetch origin claude/mods-poc-m98u3o && git checkout claude/mods-poc-m98u3o
rustup show                      # current stable: the repository pins no toolchain, and CI builds with stable
claude --version                 # 2.1.288 or later; the mod API is the one in plugin/claude-code/types/index.d.ts
node --version                   # 22 or later (the plugin scripts and tests)
python3 --version                # 3.9 or later (the demo runner has no third-party dependencies)
plugin/build.sh                  # builds adage and adage-exec (release, --locked) and stages both, with
                                 # dist/SHA256SUMS, into plugin/claude-code/dist/<host triple>/
export ADAGE_EVAL_ROOT=/tmp/adage-eval   # scratch root used by every section below
mkdir -p $ADAGE_EVAL_ROOT
```

A base build is needed for the paired latency (§3) and the identity check (§2.3): build `origin/main`
once into a second directory and keep its binaries:

```sh
git worktree add $ADAGE_EVAL_ROOT/base origin/main
(cd $ADAGE_EVAL_ROOT/base && cargo build --release --locked -p adage-cli -p adage-exec --examples)
```

Every live section runs Claude Code with an isolated configuration directory so nothing on the
machine's own setup leaks in or is changed:

```sh
export CLAUDE_CONFIG_DIR=$ADAGE_EVAL_ROOT/claude-config   # fresh; sign in once with `claude` if needed
```

## 1. What already ran, and what to re-run first (offline, no key)

Run the repository's own checks once on your machine to confirm the branch builds and passes
there (platform differences are the only expected source of a difference):

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p xtask                                    # layers, affected, hygiene, suites
cargo build -p adage-exec -p adage-cli                 # harness_sim tests the built adage-exec; a stale one would test old code
cargo test --workspace --exclude adage-eval --no-fail-fast
cargo test -p adage-eval --no-fail-fast                # ~40 min; g4's one failure below is pre-existing on main
node scripts/check-plugin.mjs && node scripts/test-stage-plugin.mjs
bash plugin/test/session-start.test.sh && node npm/test/launcher.test.mjs
claude plugin validate --strict plugin/claude-code && claude plugin validate --strict . \
  && claude plugin validate --strict plugin/claude-code 2>&1 | sed "s#$PWD/##g" | diff plugin/claude-code/VALIDATE.txt -
claude plugin test plugin/claude-code
cargo xtask test python demo
```

Expected: everything green. On the build machine the final workspace run was 134 test suites and 1,401
tests, and `claude plugin test plugin/claude-code` ran 351 tests; a count that differs on
your machine names the suites to look at, it is not itself a failure.

The `VALIDATE.txt` diff and any regeneration of that file must use the CLI version CI pins
(`.github/workflows/ci.yml`, 2.1.288 at the time of writing): a newer validator adds lines of
its own (2.1.292 adds thirteen "gating hook without .catch" notes for the same, unchanged
`register.ts`), so a diff against a newer CLI is noise, not a finding. Install the pinned version
beside your own for that one check (`npm install --prefix /tmp/cli288 @anthropic-ai/claude-code@2.1.288`
and run `/tmp/cli288/node_modules/.bin/claude plugin validate …`). The live sections (§2 onward)
run with whichever 2.1.288-or-later CLI you have.

Two things these checks hold that a reviewer may not expect. The binary's answers to the mod are
pinned by one JSON file per answer shape, `crates/adage-hook/tests/wire/*.json`
(`impl/wire.md`); `node scripts/check-plugin.mjs` fails while their mirror in the plugin,
`plugin/claude-code/hooks/fixtures/wire.ts`, differs from them (`--wire` rewrites it), so a change
on either side fails a test on both. And the CLI suites that wait on `adage watch --follow` set
`ADAGE_TEST_FOLLOW_POLL_MS` (a debug build polls that often instead of every 250 ms; a release
build never reads it), so the follower's live pace is checked only in §2.2. One known exception: `cargo test -p adage-eval --test g4`
`contexts_fire_in_replay_and_the_fast_path_stays_exact_on_newkinds` fails on `origin/main` too
(a renderer trailing-space mismatch in the weekly suite); it is not this branch's.

What the build machine re-blessed, so a reviewer knows what to look at: the Claude golden
(`crates/adage-cli/tests/golden/claude-code.txt`) gained a silent `TurnStep` step, a silent
`PreToolUse` step and one prompt step delivering a skill directive; the Codex golden gained one
line; three snapshot files under `crates/adage-cli/src/ui/snapshots/` and
`crates/adage-cli/tests/snapshots/` changed for the new settings and card rows. Every other
golden is byte-identical to `origin/main`. `crates/adage-eval/tests/predict_gate.rs` scores a
predicted chain of reads against the agent's next run of read batches (the chain follows at once
since C7); its floors are the measured values.

## 2. The mod on its own, live (S9's questions, the plugin's install)

### 2.1 Install and activation (T-B1, T-X-11, Q1, Q6)

```sh
claude --plugin-dir plugin/claude-code --debug --debug-file $ADAGE_EVAL_ROOT/q1.log
```

In a repository with no Adage home: the opening line appears once; `/adage status` answers; the
debug log shows one hook invocation per event (the plugin's; count `session_start` spool records,
must be 1); nothing is drawn in a repository that is `adage off`. Then with `--plugin-dir` AND the
same hooks in `--settings` (the misuse the plan forbids): the mod must still wire once.

Record: Q1 (the mod loads and its `turn.step` is called), Q6 (the bridge's allow-list refuses an
argv outside it; `adage hook claude TurnStep` is the only hook argv with `TurnStep`).

### 2.2 Observe and draw (T-B2, T-B3, Q3, Q3b, Q12)

A repository with learned reflexes is needed. Use the demo sandbox:

```sh
export DEMO_ROOT=$ADAGE_EVAL_ROOT/demo
python3 demo/run.py train && python3 demo/run.py learn      # trains the pack, learns the acme-billing reflexes
```

Then open a session in the sandbox's working copy with the plugin and work a ticket by hand
(`demo/SANDBOX.md` lists the tasks). Check each render site against `ux-design.md`'s copy deck:

- the line under a prompt a reflex fired at, worded by what it told the agent;
- the marker under a tool row for a rewrite, a predicted step, a substitution, a dead end headed
  off; the receipt card under a run a receipt covered;
- the band, its narrow-width priority (resize the terminal), and the one attention line;
- the learn toast (make a learn happen: `adage condition` after a few sessions) and the lapsed
  toast (edit a granted command's script);
- the pane (`/adage watch`): Live, Reflexes, Health tabs; `Explore in browser` opens the map;
- the two settings rows in `/config`;
- the degraded states: no binary (rename `bin/adage` in the plugin data dir), `adage off`, the
  follower killed (`pkill -f 'adage watch --follow'`: the Live tab says so and `adage doctor`
  names it).

Live-only items the drawing agent listed (`impl/B2-B3-notes.md`), each a yes/no:

1. The hook's `systemMessage` arrives at the `InfoNotice` site (the redraw assumes it).
2. `UserMessage`'s `props.text` equals `UserPromptSubmit`'s `prompt` for every prompt (type a
   prompt twice: the later one's lines show).
3. The engine dispatches `classic.PreToolUse` for the plugin's own command hook with
   `updatedInput` as the hook printed it (the rewrite marker reads it there).
4. The follower survives: `ps` shows one `adage watch --follow --json --stdin ignore` per
   session, it restarts across a module reload with `--since` the kept cursor, and it is gone after
   `/clear` and after exit (Q3b).
5. `$.ui.copy` on a remote surface, `$.ui.scroll` on the pane, the pane surviving a reload.
6. The map opens the browser from a child of the engine, and the `Link` opens it after.
7. On the Desktop app: the opening line stays the engine's own notice; every site above renders.
8. `ToolGroup` rows carry `calls[].tool_use_id` on the terminal and the Desktop app, and the
   feed's `tool_use_ids` match them (the noticed line and the markers hang on them).

Record Q3 (drawing cost: the TUI stays responsive; `adage doctor` p95 unchanged with the mod
on), Q3b (no process left behind), Q12 (noticed rows draw when a candidate exists; make one with
the simulator's 2-of-12 `make lint` habit, or by repeating a habit in two sessions).

### 2.3 Observation changes nothing (T-E1-03, the identity check)

Offline first, free:

```sh
python3 demo/replay_hooks.py demo/fixtures/hook-stream.jsonl --identity \
    --adage target/release/adage --seed-home $DEMO_ROOT/adage-home --keep $ADAGE_EVAL_ROOT/replay
```

Expected: an empty diff between the run without and with `ADAGE_MOD_OBSERVE=1`. Then live: six
`mod-observe` sessions and six `adage` sessions on the same tasks (§5 runs them); the set of
`<adage-reflex>` blocks and `systemMessage`s per event class must be equal between the arms.

### 2.4 The press (T-A7, T-B4, T-X-13, Q14)

In a session with the plugin, with a learned reflex that can be delegated:

- At the end of a turn the dialog Claude Code uses to ask a question shows
  `Adage can run <command> for you after <trigger>. Allow it?` with `see what it would do / not
  now`; `see what it would do` shows three plain lines and the diff; `allow` records the grant
  with `via: press` (`adage delegate status --json` shows it on the grant, and the review
  decision's record carries it).
- The same press cannot be spent twice: the second verb in the same session is refused until the
  next token the first answer carried is used (the pane's buttons do this).
- A press pasted into a Bash command by the model (ask it to) trips the tripwire: a warning once,
  the command runs, `press_spent` is not recorded.
- `/adage on` typed first in a session in a repository where another session already acted is
  refused until something acts in this one (Q14: whether `UserPromptSubmit` fires for the mod's
  own commands; if it does not, record it and the fallback the UX names).
- Enable of a learned reflex from the `Adage learned: … Tell the agent?` dialog, and `not this
  one` leaving it disabled; the spool holds one `press_issued` and one `press_spent`, and the
  review decision carries `via: press`.
- The press window: `classic.SessionStart` reaches the mod after the plugin's own command hook
  (so the press finds its window open) and before the first `UserPromptSubmit`.
- The dialog from a detached `turn.complete` continuation draws the engine's dialog; the
  answer's verb runs with the session idle; a dismissed dialog leaves nothing behind. Answer once
  with the dialog's digits and once with its dismiss key.
- `see what it would do` against a real `--stage --json`: the three lines, the diff as a
  `Code format="diff"` block in the terminal and in the Desktop app, `allow`'s `--confirm`, the
  policy and marker written on the first grant, and the granted command running from the next
  session.
- The grant pane and the inspect pane open together (two panes with focus); Escape returns the
  keys; the band's `n patterns noticed` cell hides while the pane is open (the engine dispatches
  `ui.open`/`ui.close` to the plugin that opened it).
- A kept answer survives a restart (`$.store`): the toast shows at the next start.

### 2.5 Skills (T-D4-04, T-D4-05, Q13)

With a pack accepted by `adage train accept` whose `verified.json` says the skill is inert:

- `/acme-work-item` (the sandbox's skill) shows the dated verification heading and the step lines
  at the top of the skill's text; the Skill tool and a subagent preload each read the annotated
  text (three invocations, three annotated texts); the `◈` row draws under the Skill tool's own
  row (T-D4-05, Q13).
- Control arm (`ADAGE_ARM=control`): the text is unchanged and the spool has `would_annotate`.

## 3. Latency and startup (L5), paired, release builds

A quiet machine, named in the results. Five repetitions each.

```sh
cargo build --release -p adage-cli --examples
B=$ADAGE_EVAL_ROOT/base/target/release
target/release/examples/hook_latency $B/adage,target/release/adage - 500        # paired, both builds alternate
target/release/examples/hook_latency $B/adage-exec,target/release/adage-exec - 500
target/release/examples/learn_bench target/release/adage 300 2                  # the learner (unchanged by this branch)
cargo build --release -p adage-learn --examples && target/release/examples/candidates_bench 300 2 3
```

Pass bars (`test-plan.md` §5 and the chunks' acceptance): `batch_no_match` p95 of the branch
within 0.3 ms of the base (T-A1-07, T-A2-04, T-A6-03, T-D2-06); `turn_step_nothing_pending` p95
within 0.3 ms of `batch_no_match` (T-C1-12); `pretool_no_match` within 0.3 ms of the base
(T-A1-07); `turn_step_predicts` p95 ≤ 10 ms against `adage-exec` (T-C1-13);
`batch_no_match_with_candidates` within 0.3 ms of `batch_no_match` (T-D1-10);
`candidates_bench` on/off ≤ 1.3× (T-D1-08). Debug-build numbers from the build machine, for
orientation only: `batch_no_match` 4.85 ms median, 5.21 ms with candidates; `turn_step_predicts`
40 ms p50 on a debug build under load.

Startup (T-A8, `release_plugin_startup`): first start of the plugin with both binaries hashed and
installed < 1,500 ms, ten warm starts < 200 ms:

```sh
cargo test --release -p adage-cli --test zero_config_plugin release_plugin_startup -- --nocapture   # compiled only in release builds
```

## 4. Acting in the model's place, live (the mod's Layer 2 and 4; Q2, Q4, Q9, Q15)

Each item is a probe with a yes/no answer and a number where stated. Use the sandbox with its
learned reflexes, the plugin, `--debug --debug-file`, and `adage watch --follow --json` in a
second terminal. The grant: `adage review delegate acme-billing/<track> <reflex> --predict`
(a person's terminal, or the dialog of §2.4); `adage-exec status --repo <sandbox>` lists it, and
`adage-exec revoke --repo <sandbox> --track <track> --route <reflex> --mode predict` withdraws it
alone. `adage delegate predict off` stops every predict grant on the machine at once (the grants
are kept) and `adage delegate predict on` resumes them: one probe of §4.1 runs with it off and
expects nothing predicted in the treated arm.

### 4.1 `turn.step` (C1, C4; Q2 Layer 2, Q9)

1. At a step after the lookup reflex's directive is delivered, the mod yields the predicted call;
   the transcript on disk shows a `tool_use` with an id beginning `adage_`; the spool's
   `turn_step` record has `predicted`; the next model request is NOT made for that step (count
   `[API REQUEST]` in the debug log around it). Ten of ten sessions (Q9's bar).
2. The call runs under Claude Code's own permission rules: with the command denied in
   `settings.json`, the prediction is refused by the engine and the session continues
   (`permission` recorded, no error).
3. Never at index 0, never for a subagent (start a subagent task: its steps predict nothing),
   never twice in a row, never past the session cap (`runtime.predict_cap`, default 6).
4. Control arm: nothing yielded, `would_predict` spooled, the session otherwise identical.
5. `ADAGE_MOD_OBSERVE=1`: the same as control for predictions, with the treated arm's drawing.
6. A `Skill` prediction (with `skills.invoke = true` and an accepted inert skill): the engine
   records the yielded Skill call as the agent's; the skill runs (`impl/D3-D4-notes.md`).

### 4.2 Chains and multi-call steps (C7)

- After the lookup's result names files, one response carries several `Read` calls
  (`toolUses` lists them all, T-C7-03); Claude Code runs all of them; the agent's next request
  sees every file. `requests` for the task falls by at least two against `adage` (T-C7-04).

### 4.3 The finish line at the last step (C8, Q15)

- With the finish-line obligation armed (edit a schema file and let the model finish), the mod
  turns the `end_turn` stop into a `tool_use` of `make gen`: the engine records the rewritten stop,
  continues the loop, keeps the model's text, runs the call and ends with no Stop `block`; the
  transcript keeps the `adage_` id. Ten of ten (T-C8-02, T-C8-04).
- The Stop gate's request is not made (one fewer `[API REQUEST]` than `adage` on the same task).

### 4.4 Receipt as the result, rewrites checked by the engine (C3; Q2, Q4)

- A command a receipt covers (the zero-config Python fixture with a journaled passing run, or
  `make gen` twice without an edit between): the second `ToolResult` row is the card and the
  transcript on disk holds the receipt text as the tool result, with the model's own
  `tool_use_id`; `adage watch --follow --json` shows one `pretool` record with `answered:
  receipt`. Record whether a `PostToolBatch` for the same `tool_use_id` follows
  (`impl/C3-C7-C8-D2-C9-E-notes.md` says nothing double-counts either way).
- A covering receipt that did not answer (stale, or the rule is `ask`/`deny`): the dim beside
  line `◠ adage receipt <id> covers this run · /adage why` under the row, never the card
  (T-C3-07).
- A rewrite (`python3 -m pytest tests` in the sandbox): `$.tool.check` is consulted for both
  commands; for each settings case the unit tests of `keeps_permissions` in
  `crates/adage-runtime/src/pretool.rs` enumerate (allow, deny, ask, a mode, none), write that
  `settings.json` into the sandbox's `.claude/` and confirm the engine's verdict agrees with the
  planner's, then the `--allowedTools` case the planner cannot see (T-C3-06); the rewrite
  happens only when both verdicts agree: the notice `↪ adage: pointed this test run at …`
  appears exactly where the planner's own reading rewrote, the advice where it did not, and
  the spool record carries `checked_by: engine`.
- After `verdict {engine: differs}` the advice's `additionalContext` reaches the model (ask the
  model what it was told).

### 4.5 The dead end that never runs (C6)

- `python3 -m unittest discover -s tests` in the sandbox: `make test` runs in its place, one
  context line is shown under the permission dialog and reaches the model; `dead_end_hits`
  records `substituted`; the control arm runs the dead end and records `would_substitute`.
- Whether the engine shows the model a `context` added to a kept result (ask the model).

### 4.6 Standing text, compaction, routing (C5)

- The cached system-prompt section (`adage:procedures` from `$.prompt.compose()`) carries the
  standing text once per session and is byte-identical at three points of the session (the
  debug log's request bodies); the Bash tool's description carries the environment note once;
  the first prompt leaves out the reflexes the standing text carries (T-C5-05). A failed
  `Standing` leaves the section empty for the whole session by design (it is never retried
  mid-session, so the cache is not spent).
- At compaction (`/compact keep the todo list`), the summarizer's instructions carry the
  binary's `compact` line after the person's, no transcript row is dropped, and the reflexes
  survive compaction (the next directive still fires). On 2.1.288 the mod removes no rows
  (messages carry no origin), so only the instruction is added.
- Routing (Q11): `adage config set runtime.routine_model <alias>`; at a step where a `fire`
  directive without a grant is pending, the request names that model (the debug log shows the
  request's model) and the `turn_step` record has `decision.routine`; `ADAGE_ARM=control` never
  routes and records `would_route`. The first main-loop step of a headless session is never
  routed (the card is read there). A subagent's model is not routed.

### 4.7 Attachment (C9)

- The Bash tool's description carries the convention line; a fresh session uses the
  `scripts/wi-lookup` convention on its first attempt with no event and no directive
  (T-C9-04: "makes no lookup of its own": no directive is delivered and the agent still uses it).
- An Explore subagent spawned for a work item: its transcript opens with the `<adage-reflex>`
  block and it makes no lookup of its own; its Bash calls reach the mod's `tool.call` with
  `agentId`, and the spool's calls carry `agent` (T-C9-03, T-C9-04).
- Directives per session fall in the `mod` arm against `adage` (T-C9-05; §5 measures it).

### 4.8 Noticed (D2, D5; Q12)

- With a candidate present, the noticed row draws at hook time with its ask; the binary prints
  nothing for it in either arm and spools `noticed` in both (the mod draws it from the feed only
  in the treated arm, so the control session shows no row); the fast path shows no change (§3).

## 5. The headless comparison (L10, T-E1, the design's own question)

Five arms, interleaved per task and repeat, on the acme-billing sandbox. The demo runner sets no
spend cap per session (`DEMO_MAX_TURNS` bounds a session's turns); watch the spend as it runs.

```sh
export DEMO_ROOT=$ADAGE_EVAL_ROOT/demo          # the trained and learned home from §2.2
python3 demo/run.py one W-2041 adage --debug    # smoke, one per arm
for arm in a0 control mod-observe mod; do python3 demo/run.py one W-2041 $arm --debug; done
jq '.runs | to_entries[] | {id: .key, arm: .value.arm, requests: .value.metrics.requests,
    verified: .value.metrics.requests_verified, wired: .value.metrics.hooks_wired_once}' $DEMO_ROOT/state.json
# the grant the mod arm needs (a person's terminal), then the comparison
adage review delegate acme-billing/<track> <finish-line reflex> --predict
DEMO_REPEATS=3 python3 demo/run.py eval --arms a0,control,adage,mod-observe,mod
python3 demo/report.py $DEMO_ROOT --a mod --b a0 --out $DEMO_ROOT/results-mod-a0
python3 demo/report.py $DEMO_ROOT --a mod --b adage --out $DEMO_ROOT/results-mod-adage
python3 demo/report.py $DEMO_ROOT --a mod-observe --b adage --out $DEMO_ROOT/results-observe
```

Checks: `requests_verified` true on every run (the count agrees with the stream's distinct
request ids); `hooks_wired_once` true on the plugin arms; the `a0` arm has no spool.

Read the report: `requests`, `cost_usd`, `passed`, `hidden_passed`, `stale_generated`,
`dead_end_hits{hit,substituted}`, `directives`, `predicted_steps`, `receipts_as_results`,
`subagent_tool_calls`, with the task-stratified permutation p and the bootstrap interval.

The exit criterion for M4 (`test-plan.md` §10): the `mod` arm runs end to end with `requests`,
`dead_end_hits` and `directives` lower than `adage`, at `hidden_passed` not lower. `mod-observe`
against `adage`: no difference in any outcome metric (the identity check, live).

## 6. The development gate (L11, T-E2) and the registered run (L12, T-E3)

Only after §5 passes. The harness is `evals/reflex-proof/harness/dev_v9.py` with the MOD design
(`--design dev-v9-mod`: arms A0, A4 and MOD in 16 paired blocks, MOD with its five realized
controls). `freeze` refuses until the protocol revision naming `dev-v9-mod` is registered, and the
study directory must sit inside its native-confinement cohort. Every command but `live` spends
nothing; `dry-run` shows the order, the person's step (the predict grants) first.

```sh
cd evals/reflex-proof/harness
python3 dev_v9.py freeze <cohort>/dev-mod --design dev-v9-mod --plugin ../../../plugin/claude-code \
    --predict-route <track>/<reflex> --cohort <cohort> --cumulative <learned run> \
    --controls <validated task controls> --binaries <release binaries>
python3 dev_v9.py prepare <cohort>/dev-mod && python3 dev_v9.py dry-run <cohort>/dev-mod
python3 dev_v9.py approval-script <cohort>/dev-mod   # a person reviews the script it writes and runs it
python3 dev_v9.py verify-grants <cohort>/dev-mod
python3 dev_v9.py live <cohort>/dev-mod              # the paid step
python3 dev_v9.py collect <cohort>/dev-mod && python3 dev_v9.py report <cohort>/dev-mod
```

Rule: `D1_requests_max_ratio_to_A0`, `D1_blocks_fewer_min`, `D4_sessions_with_repeat_attempts_max`
from the frozen manifest's `dev_thresholds_v9`; the findings file states each as pass or fail. Two
runs on fresh seeds must pass before E3 (`STUDY_CEILING_USD` 8.50 each, `SESSION_CAP_USD` 0.50 a
session). E3 is the registered
protocol in `docs/launch-gate-protocol.md` (arms A0 / NOEXEC / BEST = MOD, 20 blocks); its outcome
goes in `evals/reflex-proof/LAUNCH-STATUS.md` with the scripts that produced every number. Model
requests are never presented as GPU savings.

## 7. Usability (L13, T-U-01)

Per `ux-design.md` §14 once the surfaces above are confirmed live: the reading part (eight
readers, the recordings), the doing part (five live tasks without a terminal), and the twenty
matched pairs of copy. Not before §2 passes.

## 8. What to do with a failure

- A failing offline check in §1 on your platform: fix it on the branch (fail-open on hook paths
  is never relaxed to pass a test; a golden is re-blessed only for added lines).
- A "no" in §2 or §4: the chunk's notes name the assumption that failed and, where the agent
  could see one, the fallback; record it, fix the mod or the binary, and re-run the section.
- A §5 result that is not better: that is the answer the proposal asked for; record it as such.
  Do not tune the arms, the tasks or the thresholds after seeing the numbers.

## 9. Reporting

Write `docs/claude-mod/evaluation-results.md` with, per section: the machine (CPU, OS, Claude
Code version, the commit evaluated), the command as run, the result (the number, or yes/no per
probe), and pass/fail against the bar above. Attach the demo report directories and the debug
logs' request counts. Numbers that could not be produced are listed as not produced with the
reason, never estimated.
