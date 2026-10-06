# P4a: arms and the requests metric in the demo runner (chunk E1, POC scope)

Chunk E1 of `docs/claude-mod/implementation-plan.md`, cases T-E1-01 to T-E1-04 of
`docs/claude-mod/test-plan.md` §7.3. No live session was run: this environment has no API key.
Every number below comes from offline unit tests, a synthetic report, or a replay through a
locally built binary.

## What was built

| File | What |
| --- | --- |
| `demo/run.py` | Five eval arms in one table (`ARMS`): `a0`, `control`, `adage`, `mod-observe`, `mod`. `arm_invocation()` returns each arm's flags and environment. `stage_plugin()` / `plugin_for_runs()` stage the plugin with its own binary. Every session runs under `--debug --debug-file runs/<id>/claude-debug.log` with `--include-partial-messages`, and stderr goes to a file instead of an unread pipe. `derived_metrics()` computes the E1 keys, and all of `score.py`'s keys are recorded, `hidden_passed` included. The CLI is now argparse: `one <task> <arm> [--debug]`, `eval --arms …`, `--plugin-dir`, `--plugin-bin`, `--no-stage`. |
| `demo/adage_metrics.py` (new) | `requests()`: the same regexes and cross-check as `heldout_analysis.requests()`, a test pins it to the harness. A CRC-framed spool reader with a JSON-lines fallback. Fail-soft spool and stream counters. |
| `demo/replay_hooks.py` (new) | Replays a recorded payload stream through `adage hook claude <event>` and records stdout, the exit code and the new spool records for each event. Paths are normalised and `ts`/`duration_us` dropped. `--compare A B` diffs two recordings. `--identity` replays twice, without and then with `ADAGE_MOD_OBSERVE=1`, and diffs the two. `--seed-home` starts from a learned home. |
| `demo/fixtures/` (new) | `hook-stream.jsonl` (nine Claude payloads, SessionStart through SessionEnd) and two identical hand-written recordings, `recording-adage.jsonl` and `recording-mod-observe.jsonl`. |
| `demo/report.py` | New `METRICS` entries: `requests` (missing when `requests_verified` is false), `predicted_steps`, `receipts_as_results`, `dead_end_substituted`, `directives`, `subagent_tool_calls`. `compare()` already took arm names, and `main` gains `--a/--b`. The mechanism counts are kept out of the Holm family, so the adjustment of the existing outcome metrics only changes when `requests` data exists. |
| `demo/test_run_arms.py`, `demo/test_replay_hooks.py` (new) | 34 tests (below). |
| `demo/SANDBOX.md` | The arms, the plugin staging and every new key. |
| `xtask/src/suites.rs` | Registers the `demo` suite (root `demo`, tests `demo`). |

### How the plugin arms get their binary

`plugin/build.sh` runs `cargo build --release` and copies `target/release/adage` to
`plugin/claude-code/dist/<rustc host triple>/adage`, then writes `dist/SHA256SUMS`
(`<sha256>  <triple>/adage`). The plugin's `scripts/session-start` maps `uname` to a triple. On
Linux it tries `-musl` first and falls back to `-gnu` when no musl binary is staged. It checks the
digest and links `${CLAUDE_PLUGIN_DATA}/bin/adage` to a verified copy, falling back to `adage` on
`PATH`. Every other hook in `hooks/hooks.json` runs that link.

The runner never writes to `plugin/`. Instead, `stage_plugin()` copies the plugin directory to
`$DEMO_ROOT/plugins/<name>/` without its `dist/`, so a stale musl stage cannot win. It then stages
the chosen binary as `dist/<host triple>/adage` with a matching `SHA256SUMS` line. The binary is
`--plugin-bin`, else `ADAGE_BIN`, else `target/release/adage`. The real shim is exercised offline
against a stub binary in `test_the_plugin_shim_links_the_staged_binary`. `--no-stage` loads
`--plugin-dir` as it is, after `plugin/build.sh`.

## How to run the arms once a key is available

```sh
cargo build --release -p adage-cli                 # or: plugin/build.sh (then --no-stage works too)
export ANTHROPIC_API_KEY=…                         # or a logged-in `claude`
export DEMO_ROOT=/tmp/adage-demo-e1

# Train and learn once, so the hook arms have routes (control, adage, mod-observe and mod share ADAGE_HOME).
python3 demo/run.py train
python3 demo/run.py learn

# T-E1-01 / T-E1-02 smoke: one held-out run per arm; check requests, requests_verified, hooks_wired_once.
for arm in a0 adage mod-observe mod; do python3 demo/run.py one W-2041 $arm --debug; done
jq '.runs | to_entries[] | {id: .key, arm: .value.arm, requests: .value.metrics.requests,
    verified: .value.metrics.requests_verified, wired: .value.metrics.hooks_wired_once,
    starts: .value.metrics.session_starts, hidden: .value.metrics.hidden_passed}' $DEMO_ROOT/state.json

# The comparison: all five arms interleaved per task and repeat.
DEMO_REPEATS=3 python3 demo/run.py eval --arms a0,control,adage,mod-observe,mod
python3 demo/report.py $DEMO_ROOT --a mod --b a0 --out $DEMO_ROOT/results-mod-a0       # T-E1-04
python3 demo/report.py $DEMO_ROOT --a mod-observe --b adage --out $DEMO_ROOT/results-observe

# T-E1-03, offline and free: observation changes nothing for a recorded stream (seed a learned home).
python3 demo/replay_hooks.py demo/fixtures/hook-stream.jsonl --identity \
    --adage target/release/adage --seed-home $DEMO_ROOT/adage-home --keep $DEMO_ROOT/replay
```

Use `--plugin-dir plugin/claude-code-exec` for the `mod` arm once the predict grant exists (see
open issues).

## Test results (commands run here)

- `cd demo && python3 -m unittest discover -s . -p 'test_*.py'`: 35 tests, OK, 4 skipped. The
  skipped tests are the replay tests, which need `ADAGE_BIN`.
- `cargo xtask test python demo`: `== demo (2 files, from demo)`, 35 tests, OK, 4 skipped.
- `ADAGE_BIN=<scratch build>/adage cargo xtask test python demo`: 35 tests, OK, none skipped. The
  binary was built with `cargo build --release -p adage-cli` from the unmodified main checkout at
  `cc0240a` into a scratch target directory, because `crates/` in this worktree was being edited
  concurrently. The replay tests include one that learns a home offline
  (`adage condition --demo --claude-projects sim/fixtures/claude/projects`) and replays the
  fixture stream into a checkout named `acme-billing`. That replay delivers a real
  `<adage-reflex>` directive and real spool records. `spool_metrics` reads them (one
  `session_start`, at least one directive), and the observe and non-observe recordings are
  identical.
- `cargo test -p xtask`: 18 passed, including `every_python_test_belongs_to_a_suite` and
  `every_suite_has_tests_and_unique_name`.
- `cargo xtask test python live`: 61 tests, OK. This suite shares `demo/report.py`.
- `python3 demo/report.py <synthetic state> --a mod --b a0`: prints the table with `requests`
  (diff −12, CI [−12, −12], p 0.0001, p_holm 0.0007 on the synthetic data).
- `ruff check --select F demo/*.py`: all checks passed.

Covered offline:
- **T-E1-01.** A retry seen in the stream, a retry explained by `[ERROR] API error` lines,
  duplicate dispatch ids, an unexplained surplus or deficit, and message ids that are not counted.
  `test_mirrors_the_harness` compares the regexes and the results with `heldout_analysis.requests()`.
- **T-E1-02.** The arm table: `a0` gets no `--settings`, no plugin and no `ADAGE_*`, and the plugin
  arms never call `settings_file`. `hooks_wired_once` is checked on synthetic spools.
- **T-E1-03.** `--compare` over the fixture recordings, including one-byte stdout and spool
  mutations.
- **T-E1-04.** `compare(runs, a="mod", b="a0")` returns p, CI and `n_tasks` for `requests`,
  `cost_usd`, `pass_rate`, `hidden_pass_rate` and `stale_generated_rate`.

## Open issues

1. **No live run.** T-E1-01's acceptance (`run.py one W-2041 mod` records `requests` equal to the
   distinct explicit request-id count) and T-E1-02's live check are untested here. These are also
   unverified on Claude Code 2.1.288:
   - whether `--include-partial-messages` streams carry `request_id`;
   - whether `--debug` together with `--debug-file` logs the same `[API REQUEST]` lines the
     harness saw;
   - whether `--plugin-dir` plugins load under `--setting-sources project,local`.
2. **`ADAGE_MOD_OBSERVE` is read by nothing yet.** A search of `crates/` and `plugin/` in this
   worktree found no reader. The mod's `plugin/claude-code/hooks/register.ts`, being written in
   parallel, registers observers but does not check the variable yet. Until it exists, the `--identity` replay compares the binary with itself plus an
   ignored variable, so it is a harness check, not yet the T-E1-03 proof. The replay also drives
   `adage hook` directly and does not exercise a JS mod. Once the mod has an entry point, the
   replay must go through it.
3. **The plugin directory for `mod`.** The test plan (§7.2) names `plugin/claude-code-exec` for
   `mod` and the brief named `plugin/claude-code`. The default is `plugin/claude-code` for both
   plugin arms, selectable with `--plugin-dir`, and staging stages only `adage`, not `adage-exec`.
   Staging `plugin/claude-code-exec` would also need `dist/<t>/adage-exec`, which is a small
   extension of `stage_plugin`.
4. **The `mod` arm's grants.** "The study's plus `--predict` on the finish-line route" is not
   issued by the runner: the grant (C2) does not exist yet. Until then `mod` and `mod-observe`
   differ only by the observe variable.
5. **Spool field names are guesses for chunks not yet built.** These come from the test plan, not
   from code: `turn_step` with `predicted` (C1), `answered: receipt` (C3) and the `substituted`
   judgement (C6). The counters fail soft, so a different name reads as 0 rather than an error.
   They should be re-pinned when those chunks land.
6. **Unverified `requests` are dropped per run, not per block.** The harness drops the whole
   paired block, and `report.py` drops only the run.
7. **Subagents are disallowed.** The runner's `--disallowedTools … Agent Task` means
   `subagent_tool_calls` is 0 by construction until that list changes for a subagent study.
8. The runner's smoke run id is now `smoke-<task>-<arm>`, previously `smoke-<task>`, so one root
   can hold one smoke run per arm.
