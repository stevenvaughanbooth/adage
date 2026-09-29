# Replay savings: the offline metric Adage's engine is optimised against

`adage eval replay-savings` implements the metric of the architecture review's §5.2
([evals/launch/architecture-review.md](../launch/architecture-review.md)): replay held-out agent
traces through the real runtime and count, in tool calls and dollars, what each directive would
have saved net of what it costs to carry. It spends nothing and runs nothing. The code is
`crates/adage-eval/src/savings_replay.rs` (tests: `savings_replay_tests.rs`). The helpers here
build its inputs from RepoBench runner roots, calibrate it against the live measurement, and
produce the baseline table.

```text
python3 evals/replay/manifest.py /tmp/adage-rb2/runs/{click,attrs,marshmallow,vue-core} --out /tmp/replay --cochange
adage eval replay-savings /tmp/replay/click.json --arm control --only U11 [--mode pre-emptive] [--json]
python3 evals/replay/calibrate.py --manifests /tmp/replay        # replay vs live (mandatory gate)
python3 evals/replay/baseline.py  --manifests /tmp/replay        # the lever table
python3 -m unittest discover evals/replay
```

`ADAGE_REPLAY_EXPLAIN=1` prints each directive's window call by call on stderr, marking which calls
are in Sat(d) (`S`), in W(d) (`W`), and failed (`F`).

## Definitions

A held-out trace τ = (a₁…aₙ) is one agent session. Each tool call is one action a_k. A policy π
is either Adage's runtime over an index (learned or bound), or a hypothetical oracle. At each hook
event t, π emits at most one directive d_t. For the runtime this is the first route that
`adage_runtime::decide` would have given, in the control arm, with the events Claude Code raises
(`transcript::events`, the same as `adage eval` replays). t is the index of the call the event
follows. The session's prompt and session start are t = −1.

**Features of an action** (from the call and its output, as the model saw it):

- **Test invocation**: interpreter × runner, parsed from the Bash command's simple commands.
  - Interpreters: `system` (bare `python`/`pytest`), `venv` (`.venv/bin/…`, an activated `.venv`,
    `uv run`), `tox`, `nox`, `node`, `go`, `cargo`.
  - Runners: `pytest`, `unittest`, `tox`, `vitest`, `jest`, `pkg-test`, `go-test`, `cargo-test`, …
- **Executed tests**: the output shows tests ran (`collected N items` with N > 0, `N passed`,
  `N failed`, `Tests:`, `Test Files`, `Ran N tests`, go's `ok`/`--- FAIL`, `test result:`) and
  carries no environment signature.
- **Environment signature**: `No module named …`, `ModuleNotFoundError`, `command not found`,
  `Cannot find module`, `ImportError while loading conftest`, and similar. Only a Bash call's output
  is checked. A call **failed** if the harness said so (`is_error`) or its output has a signature:
  a piped `… 2>&1 | head` exits 0.
- **Environment probe/repair**: `pip install|list|show`, `uv pip|sync`, `-m venv`, `which`,
  `--version`, `ls .venv…`, a root listing (`ls -la | head`), `find … pyproject/pytest`, reading
  `pyproject.toml`/`tox.ini`/`setup.cfg`/`requirements*.txt`, `npm|pnpm install`.
- **Test-discovery search**: Grep/Glob, or Bash `find`/`grep`/`rg`/`ls`, mentioning tests.
- **Test targets**: the test files a run names (`tests/test_x.py::t` → `tests/test_x.py`). For
  `go test`, the packages it names.

**Directive classes**, from the route's step:

| Class | Sat(d) | W(d) (the waste it targets) | Contradicting action (harm) |
| --- | --- | --- | --- |
| Test runner (`Run` / `Sequence` / `Avoid … instead` whose command is a test invocation; U11 `.venv/bin/python -m pytest`) | a run with d's **runner** that executed tests (interpreter free: see calibration) | environment friction: failed runner invocations that ran no tests, any call with an environment signature, environment probes and repairs | a different runner executed tests, and d's runner never did after t |
| Other command | a call starting with d's command that did not fail | failed calls | – |
| Test files (the co-change oracle) | a test run naming one of d's files that executed tests | test-discovery searches | a run of other test files executed tests, and d's files never ran after t |
| Advice, read batch | never (a trace cannot show it) | – | – |

For each directive d_t:

- **t\*** = min{t′ > t : a_t′ ∈ Sat(d)}.
- **Useful** if t\* exists.
- **Harmful** if it is not useful, the session passed, and a contradicting action happened after
  t. The spec's "sandbox replay of d" alternative is not implemented: nothing is executed.
- **S(d_t)** = |{a_k ∈ W(d) : t < k < t\*}| + 𝟙[d pre-emptive ∧ a_t ∉ Sat(d) ∧ a_t failed].
  It is 0 when d is not useful.
- **Superseded**: given inside an earlier useful directive's window [t, t\*). In the
  counterfactual the agent never got there, so it is neither scored nor costed.
- **C(d)** = tokens(d) × cache-read price × API calls left ÷ the session's mean $ per tool call:
  its cost in calls.
  - tokens(d): the delivered text, wrapper included (`render::wrap_directive`), at 4 characters
    per token.
  - Cache-read price: from the pinned table (`adage_core::pricing`), for the session's model.
  - API calls left: the tool calls after t × the session's model responses per tool call.
  - Mean $ per call: the harness's `total_cost_usd` ÷ tool calls (the ledger's priced tokens
    when absent).
- **Ŝ** = (1/|sessions|) Σ_τ Σ_t [S(d_t) − C(d_t)], in calls per session.
  - Reported with a 95% percentile bootstrap that resamples tasks (2000 draws, seeded), and in
    $ per session (each session's net × its $ per call).
  - Also reported: useful rate with its Wilson 90% lower bound, harmful rate, and directives per
    10 calls.
  - The §5.2 gates: LB ≥ 0.8, harmful ≤ 1%, ≤ 1 per 10 calls.

**Modes.** `post-hoc` delivers each directive after the event that triggered it, as the hooks do
today. `pre-emptive` also asks each failure-triggered route, at `PreToolUse` of every call that
would fail (known in hindsight from its output, piped or not), "had we intercepted this call".
This scores a `PreToolUse` rewrite before it is built.

## Calibration

The one live measurement (`evals/launch/README.md` §1, `mechanism.py`): in RepoBench v2 Haiku
sessions on click, attrs and marshmallow that hit U11's dead end, the adage arm needed **1.85**
fewer tool calls from the dead end to a test run that ran (task-clustered over 21 tasks,
**95% CI [1.17, 2.56]**).

The replay's prediction of the same quantity (`calibrate.py`):

1. Replay the **control** arm's traces with the index that was live
   (`private/adage-home/index.json`), restricted to U11 (`--only U11`), post-hoc as delivered.
2. Take Σ S per session that hit the dead end, the mean per task, then the mean over the same
   21 tasks (bootstrap as `mechanism.py`: 5000 draws, seed 7).

| Sat rule | arm replayed | mode | tasks | directives | useful | predicted saving (calls) | 95% CI | inside live CI |
| --- | --- | --- | ---: | ---: | ---: | ---: | --- | --- |
| strict | control | post-hoc | 21 | 35 | 9 | 0.16 | [0.03, 0.30] | no |
| strict | control | pre-emptive | 21 | 58 | 15 | 0.54 | [0.24, 0.87] | – |
| strict | adage | post-hoc | 21 | 41 | 41 | 0.08 | [0.03, 0.14] | – |
| goal | control | post-hoc | 21 | 35 | 35 | 1.40 | [0.87, 1.95] | **yes** |
| goal | control | pre-emptive | 21 | 58 | 57 | 2.95 | [2.39, 3.55] | – |
| goal | adage | post-hoc | 21 | 41 | 41 | 0.08 | [0.03, 0.14] | – |

| Repository | tasks | live saving [95% CI] | replay prediction (goal, post-hoc) [95% CI] |
| --- | ---: | --- | --- |
| click | 8 | 0.46 [0.08, 0.83] | 0.54 [0.25, 0.88] |
| attrs | 7 | 2.88 [1.83, 4.05] | 2.33 [1.48, 3.33] |
| marshmallow | 6 | 2.50 [1.33, 3.56] | 1.44 [0.47, 2.44] |
| all | 21 | 1.85 [1.17, 2.56] | 1.40 [0.87, 1.95] |

**What had to change.** The first definition of Sat for a test-runner directive required d's
interpreter as well as its runner (the spec's "interpreter and runner match d's"). It predicted
0.16 calls, an eighth of the live effect, and scored 26 of 35 U11 directives "not useful". The
reason: most control-arm agents recovered from the dead end by `pip install pytest` into the
system Python, then ran `python -m pytest` successfully. The trace never used `.venv`, so no t\*
existed, even though the calls between the dead end and that working run are exactly what the
directive removes: agents followed it 41 of 41 times live. Sat for a runner directive is now the
**goal class**: d's runner executed tests, in whatever environment the agent got working. The
interpreter-strict rule stays available as `--sat strict`. With the goal class, the prediction
lies inside the live CI, overall and in each repository. The ordering across repositories
matches: click low, attrs and marshmallow high.

**Checks that the replay reproduces what happened live:**

- Replaying the **adage** arm's traces fires exactly the 41 U11 directives that were delivered
  live, route by route (click 13 + 1, attrs 13 + 3, marshmallow 7 + 4) and in the same sessions.
- All 41 are useful even under the strict rule, since agents followed.
- What is left to save after them is 0.08 calls, as it should be.

**Where the replay stays below live** (1.40 vs 1.85, well inside the CI):

- The live measure counts every call to the working run; S counts only W's calls. Reads and edits
  in the window are not counted as saved.
- Live adage-arm sessions whose dead end went through a pipe (no directive, bug 2) were also
  faster than their control counterparts (2.14 vs 2.82 calls): noise the replay cannot reproduce.

The replay is therefore a slightly conservative estimator of the post-hoc effect.

## Baseline: which lever is worth building

All policies are scored on the **control** arm's held-out sessions, which no directive steered.
Haiku 4.5, RepoBench v2, default thresholds. `learned` learns from the repository's training
traces, then binds instincts against a fresh checkout with its `.venv`/`node_modules`
(`private/bind-sandbox/repo`), as `adage condition` does. `instincts` binds only. The co-change
oracle's table comes from each task's git history before its base commit (commits of fewer than
40 files, test files present at the base), via `manifest.py --cochange`.

| Repository | Policy | Sessions | Directives | per 10 calls | Useful (Wilson90 LB) | Harmful | Ŝ calls/session [95% CI] | Ŝ $/session | §5.2 gates |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- | ---: | --- |
| click | (a) learned reflexes + instincts | 24 | 11 | 0.12 | 1.00 (0.80) | 0.00 | **+0.49** [+0.20, +0.78] | +0.0043 | LB≥0.8 ✓, harm≤1% ✓, ≤1/10 ✓ |
| click | (b) instincts only | 24 | 11 | 0.12 | 1.00 (0.80) | 0.00 | **+0.49** [+0.20, +0.78] | +0.0043 | LB≥0.8 ✓, harm≤1% ✓, ≤1/10 ✓ |
| click | U11 post-hoc (as live) | 24 | 11 | 0.12 | 1.00 (0.80) | 0.00 | **+0.49** [+0.20, +0.78] | +0.0043 | LB≥0.8 ✓, harm≤1% ✓, ≤1/10 ✓ |
| click | (c) U11 pre-emptive (hypothetical) | 24 | 23 | 0.25 | 1.00 (0.89) | 0.00 | **+1.65** [+1.36, +1.98] | +0.0140 | LB≥0.8 ✓, harm≤1% ✓, ≤1/10 ✓ |
| click | (d) co-change file→test oracle (hypothetical) | 24 | 24 | 0.26 | 0.92 (0.78) | 0.04 | **+0.49** [+0.15, +0.82] | +0.0036 | LB≥0.8 ✗, harm≤1% ✗, ≤1/10 ✓ |
| attrs | (a) learned reflexes + instincts | 24 | 16 | 0.21 | 1.00 (0.86) | 0.00 | **+2.02** [+1.07, +3.02] | +0.0124 | LB≥0.8 ✓, harm≤1% ✓, ≤1/10 ✓ |
| attrs | (b) instincts only | 24 | 16 | 0.21 | 1.00 (0.86) | 0.00 | **+2.02** [+1.07, +3.02] | +0.0124 | LB≥0.8 ✓, harm≤1% ✓, ≤1/10 ✓ |
| attrs | U11 post-hoc (as live) | 24 | 16 | 0.21 | 1.00 (0.86) | 0.00 | **+2.02** [+1.07, +3.02] | +0.0124 | LB≥0.8 ✓, harm≤1% ✓, ≤1/10 ✓ |
| attrs | (c) U11 pre-emptive (hypothetical) | 24 | 21 | 0.27 | 0.95 (0.81) | 0.00 | **+3.31** [+2.11, +4.43] | +0.0208 | LB≥0.8 ✓, harm≤1% ✓, ≤1/10 ✓ |
| attrs | (d) co-change file→test oracle (hypothetical) | 24 | 28 | 0.37 | 0.68 (0.52) | 0.07 | **+0.44** [+0.06, +1.02] | +0.0029 | LB≥0.8 ✗, harm≤1% ✗, ≤1/10 ✓ |
| marshmallow | (a) learned reflexes + instincts | 18 | 8 | 0.17 | 1.00 (0.75) | 0.00 | **+1.21** [+0.39, +2.15] | +0.0102 | LB≥0.8 ✗, harm≤1% ✓, ≤1/10 ✓ |
| marshmallow | (b) instincts only | 18 | 8 | 0.17 | 1.00 (0.75) | 0.00 | **+1.21** [+0.39, +2.15] | +0.0102 | LB≥0.8 ✗, harm≤1% ✓, ≤1/10 ✓ |
| marshmallow | U11 post-hoc (as live) | 18 | 8 | 0.17 | 1.00 (0.75) | 0.00 | **+1.21** [+0.39, +2.15] | +0.0102 | LB≥0.8 ✗, harm≤1% ✓, ≤1/10 ✓ |
| marshmallow | (c) U11 pre-emptive (hypothetical) | 18 | 14 | 0.30 | 1.00 (0.84) | 0.00 | **+2.99** [+1.71, +4.14] | +0.0251 | LB≥0.8 ✓, harm≤1% ✓, ≤1/10 ✓ |
| marshmallow | (d) co-change file→test oracle (hypothetical) | 18 | 18 | 0.38 | 0.94 (0.79) | 0.06 | **+0.32** [-0.01, +0.88] | +0.0030 | LB≥0.8 ✗, harm≤1% ✗, ≤1/10 ✓ |
| vue-core | (a) learned reflexes + instincts | 16 | 0 | 0.00 | 0.00 (0.00) | 0.00 | **+0.00** [+0.00, +0.00] | +0.0000 | – |
| vue-core | (b) instincts only | 16 | 0 | 0.00 | 0.00 (0.00) | 0.00 | **+0.00** [+0.00, +0.00] | +0.0000 | – |
| vue-core | U11 post-hoc (as live) | 16 | 0 | 0.00 | 0.00 (0.00) | 0.00 | **+0.00** [+0.00, +0.00] | +0.0000 | – |
| vue-core | (c) U11 pre-emptive (hypothetical) | 16 | 0 | 0.00 | 0.00 (0.00) | 0.00 | **+0.00** [+0.00, +0.00] | +0.0000 | – |
| vue-core | (d) co-change file→test oracle (hypothetical) | 16 | 17 | 0.22 | 0.88 (0.70) | 0.00 | **+0.29** [-0.02, +0.79] | +0.0026 | LB≥0.8 ✗, harm≤1% ✓, ≤1/10 ✓ |
| **pooled: python** | (a) learned reflexes + instincts | 66 | 35 | 0.16 | 1.00 (0.93) | 0.00 | **+1.25** [+0.75, +1.79] | +0.0088 | LB≥0.8 ✓, harm≤1% ✓, ≤1/10 ✓ |
| **pooled: python** | (b) instincts only | 66 | 35 | 0.16 | 1.00 (0.93) | 0.00 | **+1.25** [+0.75, +1.79] | +0.0088 | LB≥0.8 ✓, harm≤1% ✓, ≤1/10 ✓ |
| **pooled: python** | U11 post-hoc (as live) | 66 | 35 | 0.16 | 1.00 (0.93) | 0.00 | **+1.25** [+0.75, +1.79] | +0.0088 | LB≥0.8 ✓, harm≤1% ✓, ≤1/10 ✓ |
| **pooled: python** | (c) U11 pre-emptive (hypothetical) | 66 | 58 | 0.27 | 0.98 (0.93) | 0.00 | **+2.62** [+2.00, +3.25] | +0.0195 | LB≥0.8 ✓, harm≤1% ✓, ≤1/10 ✓ |
| **pooled: python** | (d) co-change file→test oracle (hypothetical) | 66 | 70 | 0.32 | 0.83 (0.74) | 0.06 | **+0.43** [+0.18, +0.70] | +0.0032 | LB≥0.8 ✗, harm≤1% ✗, ≤1/10 ✓ |
| **pooled: haiku** | (a) learned reflexes + instincts | 82 | 35 | 0.12 | 1.00 (0.93) | 0.00 | **+1.00** [+0.56, +1.46] | +0.0071 | LB≥0.8 ✓, harm≤1% ✓, ≤1/10 ✓ |
| **pooled: haiku** | (b) instincts only | 82 | 35 | 0.12 | 1.00 (0.93) | 0.00 | **+1.00** [+0.56, +1.46] | +0.0071 | LB≥0.8 ✓, harm≤1% ✓, ≤1/10 ✓ |
| **pooled: haiku** | U11 post-hoc (as live) | 82 | 35 | 0.12 | 1.00 (0.93) | 0.00 | **+1.00** [+0.56, +1.46] | +0.0071 | LB≥0.8 ✓, harm≤1% ✓, ≤1/10 ✓ |
| **pooled: haiku** | (c) U11 pre-emptive (hypothetical) | 82 | 58 | 0.20 | 0.98 (0.93) | 0.00 | **+2.11** [+1.47, +2.70] | +0.0157 | LB≥0.8 ✓, harm≤1% ✓, ≤1/10 ✓ |
| **pooled: haiku** | (d) co-change file→test oracle (hypothetical) | 82 | 87 | 0.29 | 0.84 (0.76) | 0.05 | **+0.40** [+0.18, +0.64] | +0.0031 | LB≥0.8 ✗, harm≤1% ✗, ≤1/10 ✓ |

Supplementary (the Sonnet 5 slice's control arms; urfave/cli from RepoBench v1, 30-turn cap):

| Repository | Policy | Sessions | Directives | per 10 calls | Useful (Wilson90 LB) | Harmful | Ŝ calls/session [95% CI] | Ŝ $/session | §5.2 gates |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- | ---: | --- |
| click-sonnet5 | (a) learned reflexes + instincts | 8 | 0 | 0.00 | 0.00 (0.00) | 0.00 | **+0.00** [+0.00, +0.00] | +0.0000 | – |
| click-sonnet5 | (b) instincts only | 8 | 0 | 0.00 | 0.00 (0.00) | 0.00 | **+0.00** [+0.00, +0.00] | +0.0000 | – |
| click-sonnet5 | U11 post-hoc (as live) | 8 | 0 | 0.00 | 0.00 (0.00) | 0.00 | **+0.00** [+0.00, +0.00] | +0.0000 | – |
| click-sonnet5 | (c) U11 pre-emptive (hypothetical) | 8 | 8 | 0.36 | 0.75 (0.46) | 0.00 | **+1.36** [+0.74, +1.99] | +0.0191 | LB≥0.8 ✗, harm≤1% ✓, ≤1/10 ✓ |
| click-sonnet5 | (d) co-change file→test oracle (hypothetical) | 8 | 8 | 0.36 | 0.75 (0.46) | 0.12 | **+0.36** [-0.01, +0.87] | +0.0053 | LB≥0.8 ✗, harm≤1% ✗, ≤1/10 ✓ |
| vue-core-sonnet5 | (a) learned reflexes + instincts | 8 | 0 | 0.00 | 0.00 (0.00) | 0.00 | **+0.00** [+0.00, +0.00] | +0.0000 | – |
| vue-core-sonnet5 | (b) instincts only | 8 | 0 | 0.00 | 0.00 (0.00) | 0.00 | **+0.00** [+0.00, +0.00] | +0.0000 | – |
| vue-core-sonnet5 | U11 post-hoc (as live) | 8 | 0 | 0.00 | 0.00 (0.00) | 0.00 | **+0.00** [+0.00, +0.00] | +0.0000 | – |
| vue-core-sonnet5 | (c) U11 pre-emptive (hypothetical) | 8 | 0 | 0.00 | 0.00 (0.00) | 0.00 | **+0.00** [+0.00, +0.00] | +0.0000 | – |
| vue-core-sonnet5 | (d) co-change file→test oracle (hypothetical) | 8 | 8 | 0.28 | 0.88 (0.59) | 0.00 | **+0.11** [-0.02, +0.37] | +0.0015 | LB≥0.8 ✗, harm≤1% ✓, ≤1/10 ✓ |
| urfave-cli | (a) learned reflexes + instincts | 16 | 0 | 0.00 | 0.00 (0.00) | 0.00 | **+0.00** [+0.00, +0.00] | +0.0000 | – |
| urfave-cli | (b) instincts only | 16 | 0 | 0.00 | 0.00 (0.00) | 0.00 | **+0.00** [+0.00, +0.00] | +0.0000 | – |
| urfave-cli | U11 post-hoc (as live) | 16 | 0 | 0.00 | 0.00 (0.00) | 0.00 | **+0.00** [+0.00, +0.00] | +0.0000 | – |
| urfave-cli | (c) U11 pre-emptive (hypothetical) | 16 | 0 | 0.00 | 0.00 (0.00) | 0.00 | **+0.00** [+0.00, +0.00] | +0.0000 | – |
| urfave-cli | (d) co-change file→test oracle (hypothetical) | 16 | 16 | 0.34 | 0.56 (0.36) | 0.06 | **-0.01** [-0.01, -0.00] | -0.0001 | LB≥0.8 ✗, harm≤1% ✗, ≤1/10 ✓ |

**Reading.**

- **Learned reflexes add nothing.** (a) equals (b) in every repository: from 7–10 training traces
  the learner compiles no reflex, and of the bound instincts only U11 ever fires. Today's engine
  is worth **+1.00 calls per Haiku session** [0.56, 1.46] (+1.25 on the Python repositories), about
  $0.007, all of it U11. Nothing fires on vuejs/core or urfave/cli.
- **(c) Pre-emptive U11 is the lever to build first.** It doubles the value: **+2.11 calls per
  session** [1.47, 2.70] (+2.62 on the Python repositories), about $0.016. It passes every §5.2 gate
  (useful LB 0.93, harmful 0, 0.20 per 10 calls). In the sessions that hit the dead end it is
  worth 2.95 calls, matching the review's estimate of 2.5–3. Of the +1.1 over post-hoc:
  - Part is the intercepted call itself.
  - Most is the 23 of 58 dead ends that went through a pipe, which post-hoc delivery never sees
    (bug 2).
  - It is the only lever that reaches Sonnet 5, which pipes: post-hoc gives 0 directives on
    click-sonnet5, pre-emptive gives +1.36 calls.
- **(d) The co-change file→test oracle is worth ~0.4 calls and fails two gates.**
  - It scores +0.40 [0.18, 0.64] per Haiku session, useful LB 0.76 (< 0.8), harmful 5% (> 1%).
    Harm here means another test file ran and passed while the directed ones never ran.
  - It is the only lever that does anything on vuejs/core (+0.29, CI touching 0). It does nothing
    on urfave/cli, where there is one package and nothing to search for.
  - Most test discovery happens before the first source edit, so an after-edit prime arrives
    late. A file→test map would pay only as a pre-emptive or first-read trigger, or on larger
    monorepos.
  - Build it second and small, as the review's §5.4 #3 says, after re-scoring such a trigger
    variant here.
- **Directive cost is negligible.** C is 0.01–0.03 calls per directive (U11's delivered text is
  ~100 tokens, the co-change prime ~50, at the cache-read price over the rest of the session), so every lever here is limited by what it
  saves, not by what it costs.

## Files

- `manifest.py`: runner root → replay manifest (sessions, training traces, checkout, live index,
  co-change table).
- `calibrate.py`: the calibration table above.
- `baseline.py`: the baseline table above.
- `test_replay.py`: Python tests. Rust tests with synthetic traces of known Ŝ are in
  `crates/adage-eval/src/savings_replay_tests.rs`.
- `testdata/u11-route.json`: U11 as it was bound live on click.
- Raw inputs (`/tmp/adage-rb2`, `/tmp/adage-repobench`) are not committed. The manifests point
  into them.
