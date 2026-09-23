# S4 · Directive wording and delivery — FINDINGS

**Status: blocked before any model run. The harness is built, wired, and verified end-to-end
except for the one thing it cannot supply itself — a working `claude -p` login.**

## What happened

The spike's method needs `claude -p --model claude-sonnet-5` invoked as a subprocess, separate
from this session's own agent loop. The standalone CLI at `~/.local/bin/claude` (v2.1.221) has
no working credentials on this machine:

```
$ claude -p "say hi" --model claude-sonnet-5 ...
Failed to authenticate: OAuth session expired and could not be refreshed
```

`claude auth status` reports `"loggedIn": false, "authMethod": "none"`. There is no
`ANTHROPIC_API_KEY`, no `apiKeyHelper` in `~/.claude/settings.json`, and no per-project override.
This is distinct from — but functionally identical to — the "usage limit" stop condition in the
brief: no run can proceed. Per the brief's rule ("do not retry in a loop") I did not attempt to
fix this myself: logging the CLI in requires a browser OAuth flow that is the user's own account
action, not something a headless subagent should do on their behalf, and it isn't something I can
complete without their interaction anyway.

**One real invocation was attempted** (`arm A / T1 / repeat 1`), through the finished harness, to
get a reproducible, first-party confirmation rather than relying on an ad-hoc probe. Its own
reported cost and token counts: `"total_cost_usd": 0`, `input_tokens/output_tokens: 0`. The
harness detected the auth failure and stopped itself after that one attempt, as designed — it
did not loop. **Total runs used: 1 of the ~70 approved (all others: 0, none possible).**

That one attempt is also useful evidence the plumbing works: the transcript shows the
`SessionStart` hook actually fired inside a real Claude Code session (`hook_started` →
`hook_response`, exit 0, correct `{}` output for arm A, which emits nothing at `SessionStart`)
before the session died on the authentication call. The `--settings` file, the hook command line,
and the sandbox `cwd` were all live and correct — the only missing piece was the model call
itself.

## What's delivered and verified without a model call

| Piece | Path | Verified how |
| --- | --- | --- |
| Sandbox generator | `generate_sandbox.sh` | Ran it; `./scripts/wi-lookup W-4821 --json` prints fake JSON, `make test-auth` fails on the deliberately buggy `rotate_token` and passes once fixed, `npm test` is a decoy that fails with a pointer to the real command, `make gen` regenerates `src/generated.py` from `schema/service.proto` |
| Hook script (3 arms + baseline, 4 tasks) | `hooks/directive_hook.py` | Fed synthetic stdin for `SessionStart`/`UserPromptSubmit`/`PostToolUse`/`Stop` by hand: correct `additionalContext` per arm/task, nonce persisted per `session_id` and only added for arm C, the T3 finish-line nudge fires once and is suppressed on a second `Stop` (bounded, non-looping), and it was confirmed live inside one real `claude -p` session (above) |
| Run harness | `run_experiment.py` | `--dry-run` builds the full 68-run matrix (3 arms x 4 tasks x 5 repeats = 60, + 4 tasks x 2 no-directive baseline = 8) without spending budget; is resumable (skips combos already logged `ok`, so a rerun after a stop doesn't re-spend); the one live attempt above went through this exact code path, including sandbox reset via `git checkout`+`git clean -fdx`, per-run `--settings` generation, and stop-on-error classification |
| Run log with running count | `results/run_log.csv` | 1 row, `status=auth_error`, budget-spent counter after it: 1 |

Two things worth flagging about the harness itself, found while building it:

- The installed CLI's `--output-format stream-json` now **requires `--verbose`** (undocumented in
  `--help`); the harness sets both.
- `--max-turns` is not listed in `claude -p --help` on this build, but is still accepted (it
  reached the auth check rather than an "unknown option" error), so the brief's cap is honored.

## What I could not do

Everything the brief actually asks to be *measured* — follow rate per arm, injection-suspicion or
W3-violation responses, derail behaviour on the wrong-directive task (T4), and whether Stop
`additionalContext` gets the finish-line obligation met — requires live model turns. With zero
completed runs there is no data, and I'm not reporting invented numbers. This is the honest
result the spike's own conventions ask for: it disproves nothing about the hypothesis, it just
couldn't be tested this session.

## Recommendation

1. **Fix the CLI auth** (`claude` needs a valid login or `ANTHROPIC_API_KEY`/`apiKeyHelper` on
   this machine) — the user's own action, not something to script around.
2. **Then re-run exactly this harness**: `python3 run_experiment.py --budget 70 --max-turns 12`
   from `spikes/S4-directive-wording/`. It will pick up from run 1 (the log has one `auth_error`
   row; it does not count as a completed combo, so all 68 combos still run) and stop itself again
   if it hits a real usage-limit error, per the same rule.
3. **Wording recommendation stays provisional**, not measured: prefer **Arm B (factual,
   provenance-labelled)** per the design spec's own hypothesis and Claude Code's documented
   injection-defence guidance around imperative out-of-band text (§9, W1-W8) — but treat this as
   the pre-registered hypothesis, not a finding, until arm A/B/C follow rates and suspicion/W3
   counts actually come back. Do not update design spec §9.3 or architecture F3 from this spike
   until it produces real numbers.
4. Once data exists, the pass/fail bar from `SPIKE.md` still applies unchanged: pick the wording
   with the highest follow rate that has zero injection-suspicion responses, zero W3 violations,
   and correct derailing on T4; and treat Stop `additionalContext` as insufficient (recommend
   bounded `block`/`continue`) only if it fails to get the T3 obligation met in >=50% of runs.
