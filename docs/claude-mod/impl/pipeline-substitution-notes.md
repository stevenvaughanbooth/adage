# A dead end piped into output filters (C6, T-C6-06)

Run 3 (`evaluation-results.md`, "Run 3") had 33 dead-end hits across the treated arms, and none was
replaced: the learned substitution matched only a whole plain command, and every hit was piped.
The coordinator's decision: replace the dead end when it is the first stage of a pipeline whose
later stages only filter its output; keep everything else as it is. This note records what was
built, the contract, one departure, and the live proof.

## What was built

- **`adage_core::shell::plain_pipeline(command) -> Option<Vec<Stage>>`** (`crates/adage-core/src/shell.rs`).
  It parses with the same tree-sitter parser and the same bounds as `normalize_shell`. It answers
  only for one statement whose stages are joined by `|`. Each stage must be a simple command of
  literal words: `word`, single-quoted, double-quoted with nothing expanded, or a number. The only
  redirection allowed is the stage's own `2>&1`. `Stage { range, argv, merges_stderr }`: `range`
  is the command's byte range without its `2>&1`, and `argv` has its quotes removed. Everything
  else is `None`:
  - a list (`;`, `&&`, `||`, a newline) or a background `&`;
  - `|&`, any other redirection, or a heredoc;
  - a subshell or group;
  - `$…`, `` ` ``, or a backslash in a word;
  - an assignment prefix;
  - a comment, or a parse error.
- **`adage_runtime::substitution`** (`crates/adage-runtime/src/pretool_decide.rs`). The signature
  is unchanged, so `crates/adage-hook` needed no edit. A new `replaced_stage(command)` picks the
  part to replace:
  - a command with no shell syntax at all is replaced whole, by exactly the old test, so a plain
    command is decided byte for byte as before;
  - otherwise, the first stage of a `plain_pipeline` of at least two stages, when every later stage
    is an `OUTPUT_FILTERS` command with no `2>&1` of its own. `OUTPUT_FILTERS` is `tail`, `head`,
    `grep`, `wc`, `cat`, `tee`, `sort`, `uniq`, as a bare name and with any literal arguments.

  The route checks that follow are unchanged: the key, `directs_avoided`, `failure_matches`,
  `same_command`, and the one-action check. They run on that stage. The `Substitution`:
  - `command` is `before + instead + after`: the rest of the string is kept byte for byte,
    spacing, the stage's `2>&1` and the filters included;
  - `context` names the stage replaced: `adage ran make test in place of python3 -m unittest
    discover -s tests -t .: in this repository it fails ("…").`

  `substitute()` is unchanged: substituted and judged in the treated arm, `would_substitute` only
  in the control arm and in observe mode.
- **The mod** (`plugin/claude-code/hooks/call.ts`). Comments only. The branch already asks
  `$.tool.check` about `e.command` and `plan.substitute.command`: the whole original pipeline and
  the whole new one. It then runs `next({ ...e, command: made.command })`. The `substituted`
  marker shows the whole new pipeline.

## Contract (unchanged shape)

- The plan stays `{"plan":{"substitute":{command, route, track, context}}}` and the verdict answer
  stays `{"substitute":{…}}`. There is no `stage` field. `command` is the whole new pipeline, and
  the context line already names the stage replaced, so neither the mod nor the spool needs one.
  Adding a field would also have broken the struct literals in `crates/adage-hook/tests/wire.rs`,
  which this change does not own.
- The `pretool` spool record is as before: `substituted: true` with the `substituted` judgement,
  or `would_substitute: true, withheld: true`. `decision.substituted.command` is the whole new
  pipeline. `demo/adage_metrics.py` counts it in `dead_end_substituted` with no change.
- The command-hook path (`adage hook claude PreToolUse` without `via: mod`) never substitutes.
  Its output for every payload is unchanged, and no golden was re-blessed (`git status` shows
  none).

## Departure: the first stage's own `2>&1` is allowed

The brief said "no redirection of the first stage", and gave the run-3 command as `python3 -m
unittest discover -s tests | tail -n 20`. The run-3 streams (`/tmp/adage-live3/runs/*/stream.jsonl`,
every Bash `tool_use` that names unittest or pytest) show something else. Every piped dead end
there merges stderr:

| times | command |
| --- | --- |
| 11 | `python3 -m unittest discover -s tests -t . 2>&1 \| tail -5` |
| 6 | `python3 -m unittest discover -s tests -t . 2>&1 \| head -20; ls . tests` |
| 3 | `… 2>&1 \| head -20; ls tests` |
| 2 + 2 + 1 + 1 + 1 + 1 | `… -t . 2>&1 \| tail -15`, `tests.test_billing 2>&1 \| tail -15`, `discover 2>&1 \| tail -5`, `tests.test_billing 2>&1 \| tail -5`, `-s . -t . 2>&1 \| tail -5`, `… 2>&1 \| head -20` |
| 2 + 4 | `tests.test_billing 2>&1 \| tail -15; ls tests`, and other lists of two runs or a `git stash` |
| 8 | an edit (heredoc, `sed -i`, a Python heredoc), then `… 2>&1 \| tail -5` in the same call |

Under the literal rule, none of these would be replaced: the change would have no effect on
run 3's commands. `2>&1` sends the stage's stderr to the same pipe. It neither reads nor writes a
file, and `make test 2>&1` feeds the filters the way `python3 -m unittest 2>&1` would. So `2>&1`
is accepted on the first stage and only there, and kept with the replacement. Any other
redirection is refused, as is `2>&1` on a filter, `|&`, and a lone `cmd 2>&1` with no filter
(the old plain path refuses it as before).

With this rule, 19 of run 3's 42 piped unittest calls (all five arms) are replaced. This was
counted by sending each call's dry run to the debug binary over the live home below. Under the
literal rule the count is 0: every one of the 42 carries `2>&1`. The other 23 are lists (`;`, or
an edit before the run) and stay refused, as the coordinator decided. Replacing a stage
inside a list (live-fixes-2's design, items 1–4) would reach them. The coordinator has not taken
that up.

## Tests (T-C6-06, new in `test-plan.md`; T-C6-05 is the demo's)

- `crates/adage-core/src/shell.rs` `plain_pipeline_tests`: the stages of `… 2>&1 | tail -5`,
  `make test | grep -E 'FAIL|ERROR' | wc -l` (the quoted `|` stays in the argument), a quoted
  string, a single command and non-ASCII words. 22 refused shapes.
- `crates/adage-runtime/src/tests/pipeline_substitution.rs` (new; one `mod` line in
  `tests/mod.rs`):
  - accepted: `| tail -n 20`, `| head`, run 3's `2>&1 | tail -5`, `tail -15` and `head -20`
    forms, `grep | wc`, `tee out.log | sort | uniq -c | cat -n`, and odd spacing kept;
  - the exact context line, and the plain command still replaced whole;
  - 27 refused shapes: lists, every other redirection, `|&`, subshell, group, `$(…)`, `"$PAT"`,
    an assignment prefix, a filter not on the list (`xargs`, `sed`, `python3 -c`,
    `/usr/bin/tail`), the dead end not first, a quoted dead end, and a lone `cmd 2>&1`;
  - the decision: substituted and judged in the treated arm, `would_substitute` only in the
    control arm and in observe mode.
- `crates/adage-runtime/src/tests/dead_end.rs`: one assertion changed. `python3 -m unittest |
  tail` was asserted to give no substitution. It now gives `make test | tail`.
- `crates/adage-cli/tests/harness_sim.rs` `claude_code_substitutes_a_dead_end_piped_into_filters`
  (new). It runs through the binary on the sim's learned dead end, in both arms, with run 3's three
  shapes: `| tail -n 20`, `-t . 2>&1 | tail -5`, and `2>&1 | grep -E 'FAIL|ERROR' | head`. Each is
  checked by the existing `check_substitute`: the dry run plans the whole new pipeline with the
  stage-naming line, the treated verdict substitutes, and the control verdict spools
  `would_substitute, withheld`. Four refused shapes plan nothing and get nothing from the command
  hook. **The sim's own dead-end turn is not changed.** `scenario::replay` is shared by the
  Claude Code, Codex and OpenCode replays and by `check_graph`'s counts. Piping it would move
  other harnesses' expectations, and `crates/adage-sim/src/scenario.rs` is in another agent's
  diff. The plain dead-end turn still runs through `replay_claude_with`, and the piped forms
  through the new test.
- `plugin/claude-code/hooks/call.test.ts` `T-C6-06 · a dead end piped into filters`, four cases:
  - the same verdict on both pipelines runs `make test 2>&1 | tail -n 20`, with both whole
    pipelines checked and one context line and notice;
  - a different verdict for the new pipeline runs the call as written;
  - a withheld final word (the control arm) runs the dead end;
  - observe mode asks both phases with `observe` and replaces nothing.

## Live proof

Setup, all under the scratchpad (`$L = …/scratchpad/pipe-live`):

- **Binaries**: `cargo build -p adage-cli -p adage-exec` (debug), copied to `$L/bin`. They are
  built from the shared tree, so they include the other agent's in-progress prediction changes.
  None of those applies to a prompt that names no work item.
- **Plugin**: `git archive 037c945 plugin/claude-code`, with this change's `call.ts` laid over it.
  It was staged by `demo/run.py`'s own `stage_plugin` (`dist/x86_64-unknown-linux-gnu/{adage,
  adage-exec}` and a regenerated `dist/SHA256SUMS`), so the other agent's in-progress `step.ts`
  and `predict.ts` were not loaded.
- **Home**: a copy of `/tmp/adage-live3/adage-home`; the original was not touched. **That home
  has no learned dead end.** Every dead end in it is an instinct (`acme-billing/instincts`,
  prime, forming), and `substitution` never replaces an instinct (O0-live defect 6). As O0-live
  built `home-DE`, the copy gets the fixture track
  `crates/adage-pack/tests/fixtures/tracks/acme-billing/dead-ends` with the unittest route renamed
  `unittest-dead-end`: `session_start`, `avoid python3 -m unittest → make test`, firm. The index
  was rebuilt with `adage condition --instincts-only --repo acme-billing --repo-path .` (gen
  `eeb43d28`). This proves the mechanism; it is not learned behaviour.
- **Sessions**: a fresh `CLAUDE_CONFIG_DIR`, and `CLAUDE_PLUGIN_DATA` and the environment from
  `demo/run.py`'s `arm_invocation("mod" | "mod-observe")`. `ADAGE_ARM` is set per run. The flag is
  `--permission-mode default`. Each run has a sandbox from `demo/sandbox/generate.py` and run 3's
  model. The prompt: "Run exactly this one Bash command, as written, and nothing else: `<cmd>`.
  Then reply with one sentence …". Driver: `…/scratchpad/pipe_live.py`.

| run | command the model wrote | what ran | context line in transcript | spool `pretool` | `dead_end_hits` / `dead_end_substituted` |
| --- | --- | --- | --- | --- | --- |
| treated | `python3 -m unittest discover -s tests \| tail -n 20` | `make test \| tail -n 20` (the result ends with make's own echo, `ACME_ENV=test python3 -m unittest discover -s tests`; "Ran 3 tests … OK") | yes, once (`hook_additional_context`, "adage ran make test in place of python3 -m unittest discover -s tests: …") | `substituted: true`, `checked_by: engine`, judgements `[substituted unittest-dead-end]`, `decision.substituted.command` `make test \| tail -n 20` | 0 / 1 |
| treated | `python3 -m unittest discover -s tests -t . 2>&1 \| tail -5` | `make test 2>&1 \| tail -5` ("Ran 3 tests … OK") | yes, once (names `… -t .`) | `substituted: true`, command `make test 2>&1 \| tail -5` | 0 / 1 |
| `ADAGE_ARM=control` | `python3 -m unittest discover -s tests \| tail -n 20` | the dead end (`ImportError … ACME_ENV is not set`) | no | `would_substitute: true`, `withheld: true`, judgements `[]`, `would_substitute.command` `make test \| tail -n 20` | 1 / 0 |
| observe (`mod-observe`) | the same | the dead end | no | `would_substitute: true`, `withheld: true` | 1 / 0 |

Sessions: `582c215a…`, `8a17bb7e…`, `af7baea0…`, `e715f2cb…`. Each exited 0 after 2 turns.

**What the model made of the line.** In both treated runs the model read the context line and
quoted it. In the `2>&1 | tail -5` run it disbelieved it: "A hook note said the command was
swapped for `make test` … That doesn't match what happened. The output shows the unittest
discover command ran and passed." With `tail -5`, make's echo line is cut off. The transcript's
`tool_use` still shows the original command, and "in this repository it fails" reads as if the
replacement had failed. The same wording is used for a plain substitution, so this is not new
with pipelines. Whether the line should say which command's output follows (for example "the
output below is make test's") is a wording question for the line's owner. It is not changed here.

## Commands and results

| command | result |
| --- | --- |
| `cargo fmt --all`, then `cargo fmt --all -- --check` | clean |
| `cargo clippy -p adage-core -p adage-runtime -p adage-cli --all-targets -- -D warnings` | clean |
| `cargo test -p adage-runtime -p adage-core` | runtime lib 138 pass (6 substitution cases), core lib 74 pass (2 `plain_pipeline`), all other binaries pass |
| `cargo test -p adage-cli --test hooks` | 25 pass |
| `cargo test -p adage-cli --test harness_sim` | first run 10 pass, 1 fail (below); rerun after the other agent's update to the same file: 11 pass, 0 fail |
| `cargo test -p adage-exec --test parity` | 25 pass |
| `cargo test -p xtask` | 24 pass |
| `claude plugin validate --strict plugin/claude-code` and `.` (pinned 2.1.288) | passed; the output is identical to `VALIDATE.txt` (no regeneration needed) |
| `claude plugin test plugin/claude-code` (pinned 2.1.288) | 430 pass, 0 fail (the 4 new T-C6-06 cases included) |
| `node scripts/check-plugin.mjs` | `plugin adage 0.1.0: ok` |

The first run's one failure was `harness_sim::claude_code_predicts_under_adage_exec`: "claude
TurnStep 0: nothing in the model's place" at `harness_sim.rs:293`. It came from the other agent's
in-progress change in `crates/adage-runtime/src/prediction.rs`, which now predicts a
prompt-directed lookup at index 0. That agent then updated the replay in the same file, and the
rerun passes all 11 tests. This change's code is in neither.

## Files touched

- `crates/adage-core/src/shell.rs`: `plain_pipeline`, `Stage`, and their tests
- `crates/adage-runtime/src/pretool_decide.rs`: `substitution`, `replaced_stage`, `OUTPUT_FILTERS`
- `crates/adage-runtime/src/tests/pipeline_substitution.rs` (new)
- `crates/adage-runtime/src/tests/mod.rs`: one `mod pipeline_substitution;` line (shared file)
- `crates/adage-runtime/src/tests/dead_end.rs`: one assertion (shared file; the other agent's
  edits there are in other tests)
- `crates/adage-cli/tests/harness_sim.rs`: one new test (the other agent also edits this file's
  replay; the two diffs do not overlap)
- `plugin/claude-code/hooks/call.ts` (comments) and `call.test.ts`
- `docs/claude-mod/architecture.md` §3.4 (one paragraph), `docs/execution.md` ("The learned
  pair"), `docs/claude-mod/test-plan.md` (T-C6-06, and the index row "01 … 06")
- `CHANGELOG.md` (The Claude Code mod)
- these notes
