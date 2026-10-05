# Execution offload: can a hook remove model turns by doing deterministic work itself?

2026-09-29, research sprint wave 2, slug `execution-offload`. Offline, $0, nothing in `crates/`
touched. The code is in this directory, and small caches are in `/tmp/adage-research/execution-offload/`.

## Question

Wave 1 found that every "tell the agent X" lever lands under 5% of $, because the agent was
mostly going to do X anyway. Each model turn re-reads the whole context, so a large saving has to
**remove model turns**. Adage already has hook *actions*: E2 (staged, verified, applied) and E1
prefetch. The question: how many turns could a hook remove by *doing* the deterministic work itself
(formatting, env setup, running the tests after an edit, git bookkeeping, "run tests → read the
failure → open the file" chains), net of what its results add to the context and of the harm when
its work was unwanted? Is that the missing lever?

## Verdict: no. A removed turn is worth a lot, but few turns can be offloaded.

- **One removed turn is worth about 2% of session $.** When the hook runs the call and hands over
  exactly the result the agent would have got, the saving is the response's own full-context
  re-read, its output (thinking and tool-use JSON) and that output's carry. That is about
  **$0.005 per turn on Haiku (≈ 2.1% of a $0.26 session)**. That is 2–3× what the harness's
  flat $/call suggests for a call whose output must still be delivered. The first-principles
  premise holds.
- **But the turns a hook can take over without doing the model's reasoning are few.**
  - **Exact prediction** of the next call's arguments from the context (a repeat of an earlier
    call, a repository habit, or a Read of a path the previous call printed) covers only
    **11.9% of Haiku calls and 4.2% of Sonnet calls**. Even a perfect predictor with no misses
    would be worth **6.3% / 1.9% of $**.
  - **The learned predictor** (gradient-boosted, held out) gets **+0.1% of $** on the Haiku
    split and −0.5% on the Sonnet split. Leave-one-repo-out, it delivers nothing on either model.
    Its confident predictions are mostly early whole-file Reads and `find`/`ls` listings. When it
    misses, it injects a whole file the agent never asked for, and that cancels its hits.
  - **Test runs** are the largest offloadable class: 19% of calls and a **13.1% of $ ceiling**
    (16% if passing output is compacted). However, **64% of Haiku test runs follow no edit.** They
    are re-runs with another selector, other flags, a pipe or a broader scope, or retries after
    environment friction. A hook can't predict those. Only **1.3 runs per session** (Haiku) come
    right after an edit.
  - **Formatting doesn't happen:** 0.03 formatter or linter calls per Haiku session and 0.6 per
    Sonnet session. The **test → read failure → open file** chain happens 0.06 times per session:
    after a failing run the agent edits or re-runs; it doesn't open a printed path.
- **The addressable ceiling is below the North star even for an oracle.** Take the union of
  exactly reachable calls, every after-edit test run (with compacted passing output) and all
  environment friction, with perfect prediction and no misses. That is **14.6% of $ on Haiku
  [13.6, 15.8]** and **9.8% on Sonnet [8.2, 11.7]**. Without the friction class, which U11
  already works on, it is 10.2% and 7.3%.
- **The realistic lever is a learned "verify after edit" E2 action, and it is small.**
  - After an edit turn, the hook re-runs the agent's own last verifier command. "Verifier" is a
    learned template score. The hook delivers the result compacted on a pass and in full on a fail.
  - Held-out, Haiku: **+2.8% of $ [2.0, 3.7], +1.18 calls** (split) and **+1.2% [0.7, 1.7],
    +0.55 calls** (leave-one-repo-out).
  - Sonnet: **+0.6% [−0.1, 1.3]** (split) and **+1.0% [0.3, 1.7]** (leave-one-repo-out).
  - It breaks even or goes negative if an unwanted result costs the agent one call
    (Haiku −0.9%, Sonnet −9%), and it halves at p_skip 0.5.
- **Earlier failure detection barely changes the trajectory.**
  - 79% of Haiku's failing test runs (135 of 170) come right after a single edit turn. The agent
    already tests immediately.
  - Where it doesn't, the dead-end edits made during the detection lag are worth **+1.3% of $
    [0.6, 2.0]** (conservative) to +5.6% (every lag call, upper bound) on Haiku. On Sonnet they
    are worth +0.4% to +6.4%.
- **Environment setup by the hook adds ≈ 0 over U11 when learned conservatively.** Repair commands
  learned from the traces fix friction in held-out sessions with no measurable credit; the upper
  bound is the friction class itself, 2.5% (split) to 7.1% (LORO). That is the ground U11's
  pre-emptive directive already covers (+2 calls, 3.9%).

**So what.** Shipped as the best variant here (E2 "verify after edit", compact on pass, gated),
execution offload would save **≈ +1–3% of $ and +0.5–1.2 calls per Haiku task**
(split +2.8% [2.0, 3.7], LORO +1.2% [0.7, 1.7]) and **≈ +1% on Sonnet** (LORO +1.0% [0.3, 1.7]).
Pass rate is unmeasurable offline; the worst case is unknown, because an unrequested mid-change
test result could mislead. Even a perfect offload hook is capped at 14.6% (Haiku) / 9.8% (Sonnet)
of $. **This is not the missing lever.** Confidence that the realistic +1–3% survives a live A/B is
**low to moderate**. It hinges on p_skip and on the harm from about 1.2 unused deliveries per
session, and both need the micro-slice below.

## Method

### Data

- The data is the common harness corpus (`harness.load()`).
- The held-out sets are the control arm of test tasks: **158 Haiku 4.5 sessions / 38 tasks**
  (test set `6a795a362ccb`, v1 + v2) and **32 Sonnet 5 sessions / 16 tasks**.
- The pools are:
  - split: the 47 training-phase sessions of every repository (train tasks only);
  - **leave one repository out**: every session, any arm, of the other repositories, subsampled
    to 120 (stratified by repository) for the CPU budget.
- `assert_clean` runs for every pool/test pair.
- Almost every API response issues exactly one tool call (5,104 of 5,199 Haiku responses). So
  removing a call removes a model turn.

### Scoring: an extension of the harness semantics (`offload.py`)

`score()`'s directive semantics don't fit an offload. There, S(d) counts waste calls *before* the
goal t\*, and the goal call itself is never removed. An offload removes exactly the goal call: the
hook ran it. So I scored with the harness's own cost model (`harness.costmodel`: `removal`,
`injection`) and its task-clustered bootstrap (`harness.stats`), with these semantics:

- **Hit.** The hook runs X at the hook event that ends the turn with call t. The agent's next
  occurrence of X falls in (t, t+3] and comes before any edit.
  - `$ = p_skip · removal({k}) − injection(result, t)`.
  - The removal takes out response k: its full-context read, its output, and the carry of its
    growth.
  - The injection puts X's result back: the result is still needed, only earlier. Its size is
    exactly the agent's own result's growth (response growth minus its output), plus the command
    and a 40-token wrapper.
- **Miss.** The hook ran X and the agent did not take it: `$ = −injection(estimated result, t) −
  harm_calls · $/call`. For a Read the hook proposed but the agent never made, the size is that
  file's mean Read size in the pool.
- **Test-run "cover"** (verifier only). The agent's own next test run counts as served if it
  has V's runner and its test files ⊆ V's (selectors ignored). Deciding what is a test run and
  what its targets are uses the harness's **hand-written instrument** (`features.Act.run/targets`),
  for measurement only. A hit injects max(V's result, the agent's result). With compaction, a
  passing run is delivered as about 60 tokens.
- **Sensitivities** reported:
  - `p_skip = 0.5`: the agent re-runs anyway half the time. This is the E1 analogue of p_follow,
    which nobody has measured.
  - `harm = 1 call per miss`: an unwanted result costs a call.
- **Check.** With zero injected tokens, `session_value` equals `harness.oracle.ceiling` by
  construction: same `removal`, same bootstrap.

### 1. Learning deterministic stretches (`learner.py`, `evaluate.py`, `stretches.py`)

- **Candidates at each decision point** are generated without hand lists:
  - **repeat**: every earlier non-authoring call of the session;
  - **habit**: keys in at least 2 pool sessions of the repository, or in at least 2 other
    repositories;
  - **pointer**: a Read of one of the first 5 paths printed by the previous call.
- **Scoring.** A `HistGradientBoostingClassifier` scores each candidate on generic features:
  source, tool identity, position, recency, run count, edits since the candidate's last run, its
  last outcome and output size, template statistics learned from the pool (P(repeat) and
  P(within 3 calls after an edit)), the key's frequency in the pool, overlap with the
  last-edited file's stem, and inline-script syntax.
- **Label.** The key is taken within the next 3 calls, before an edit.
- **Delivery rule.**
  - Deliver the top candidate if p ≥ θ, with θ cross-fitted on the pool by task and never on
    held-out data.
  - Also require p·V_turn > (1−p)·C_inject. The hook knows the result size, because it ran the
    command, and the context size.
  - Never deliver the same key twice between edits.

### 2. Verify after edit (`verifier.py`)

- **Verifier score.** A template's verifier score is P(a call of this template comes within 3
  calls after an edit), learned on the pool.
- **The session's verifier V** is the agent's own most recent non-inline command with a score
  ≥ τ (τ cross-fitted on the pool).
- **Trigger.** After every edit turn, the hook runs V and delivers the result.
- **Optional gate.** A logistic P(the agent re-runs V's template before its next edit), on
  generic features.
- **Variants:** exact or cover; full output or compact on a pass.

### 3. Environment setup (`setup_offload.py`)

- **What is learned.** A repair is a non-authoring command the agent ran between a failed call of
  template T and the next success of T. It must recur in at least 2 pool sessions of the
  repository (split), or in at least 2 other repositories (LORO). It must also be specific: it
  sits in a repair window in at least half of the sessions that run it.
- **What the hook does.** It runs the learned repairs at SessionStart.
- **Credit (conservative).**
  - The agent's own calls with those keys are credited.
  - The friction calls before the agent's own first repair are credited, but only if its next test
    run executed tests.
- **Upper bound:** all friction calls.

### 4. Trajectory (`trajectory.py`)

- For each failing test run, the window runs back to the previous test run. Failure is `is_error`
  or, as a measurement instrument, failure text, for runs whose exit status a pipe hid.
- If the agent edited in more than one turn in that window, a hook test after the first edit turn
  would have exposed the failure earlier.
- Three levels are credited:
  - **U**: every call in the detection lag;
  - **D**: lag edits later revised (a later edit of the same file undoes at least 50% of their new
    lines, a string test), plus runs of that dead-end code;
  - **C**: D, only when the first edit turn itself was later revised.

## Results (held-out control, 90% task-clustered bootstrap CIs)

### Cost of a turn, and ceilings (`ceilings.py`, `union.py`; perfect prediction, no misses, results re-injected)

| class offloaded (instrument, measurement only) | Haiku calls/task | Haiku $ share | Sonnet calls/task | Sonnet $ share |
| --- | ---: | ---: | ---: | ---: |
| every call that authors nothing (would need the model's reasoning) | 23.6 | 47.6% [44.9, 50.1] | 21.9 | 61.3% [55.6, 65.6] |
| test runs | 6.16 | 13.1% [12.1, 14.1] | 5.44 | 13.8% [10.7, 17.5] |
| … with passing output compacted | 6.16 | 16.2% [15.0, 17.3] | 5.44 | 16.9% [13.3, 21.3] |
| test runs that follow an edit (compacted) | 2.08 | 5.2% [4.3, 6.1] | 2.44 | 6.3% [5.6, 7.2] |
| git commands | 2.93 | 6.5% [5.5, 7.5] | 0.75 | 2.8% [1.4, 4.2] |
| environment friction (U11 class) | 3.25 | 6.0% [4.8, 7.4] | 1.59 | 3.2% [1.6, 5.5] |
| env probes / installs | 1.53 | 2.4% [1.8, 3.1] | 0.72 | 1.7% [0.6, 3.4] |
| lint / format / type-check | 0.03 | 0.1% | 0.62 | 1.4% [0.7, 2.2] |
| **exactly reachable** (repeat / habit / printed path) | 3.85 | **6.3% [5.6, 7.0]** | 1.19 | **1.9% [1.3, 2.5]** |
| **union: reachable + after-edit tests + friction** | 7.80 | **14.6% [13.6, 15.8]** | 4.47 | **9.8% [8.2, 11.7]** |
| union without friction | 5.45 | 10.2% [9.3, 11.1] | 3.34 | 7.3% [6.4, 8.3] |

- Net $ per offloaded turn is about **$0.005 on Haiku**, about 2% of a session. It is higher for
  Edits, but those can't be offloaded.
- Searches and Reads have large ceilings (11.9% and 14.4%), but their arguments are the model's
  decision. Only the "reachable" slice of them is proposable.

### Where test runs sit (`diag_test_positions.py`, `diag_reruns.py`, `diag_chains.py`; instrument)

| Haiku test runs per session: 6.16 | per session |
| --- | ---: |
| first run of the session (no verifier known yet) | 0.94 |
| after an edit (reachable by "verify after edit") | 1.27 (1.09 covered by the previous run) |
| **no edit since the previous run** | **3.95** |
| … the previous run was friction or ran nothing | 1.17 |
| … same files, another selector, flags or pipe | 1.46 |
| … different or broader files (for example, the full suite after a targeted run) | 1.21 |

- Sonnet: 1.0 / 1.62 / 2.81 out of 5.44.
- After a failing run, the next call is a Read of a printed path 0.06 times per session (both
  models).
- After a passing run, the next call is `git` 0.35 times per session (Haiku).

### Learned exact offload (`evaluate.py` → `results/learned.json`)

| protocol | θ (pool CV) | $ share [90% CI] | calls/task [90% CI] | hits / misses per session |
| --- | --- | ---: | ---: | ---: |
| Haiku split | 0.8 | +0.1% [−0.0, +0.2] | +0.28 [+0.17, +0.39] | 0.28 / 0.35 |
| Haiku split, p_skip 0.5 | | −2.1% [−3.9, −0.5] | +0.14 | |
| Haiku split, harm 1 call/miss | | −1.0% [−1.5, −0.6] | −0.07 | |
| Haiku LORO | 0.3–0.8 | 0.0% (delivers nothing at any θ: the value rule never clears on a new repo) | 0.00 | 0 / 0 |
| Sonnet split | 0.8 | −0.5% [−1.1, −0.0] | 0.00 | 0 / 0.28 |
| Sonnet LORO | 0.3–0.7 | 0.0% (delivers nothing) | 0.00 | 0 / 0 |

- **Calibration** (Haiku split, `results/stretches.json`):
  - only 1.2% of turns reach p ≥ 0.9, and their precision is 0.55;
  - at p ≥ 0.8, 0.85 deliveries per session with precision 0.41;
  - at p ≥ 0.5, precision is 0.24.
- **What it finds** at θ 0.8: first Reads of a file a grep just printed (0.23 per session),
  session-opening `find`/`ls` listings (0.10), and `pip install pytest` (0.03).
- It found **0 chains** of two or more consecutive predictable turns at θ ≥ 0.8.
- The θ sweep is negative for every θ ≤ 0.7.

### Verify after edit (`verifier.py` → `results/verifier.json`)

| variant | Haiku split | Haiku LORO | Sonnet split | Sonnet LORO |
| --- | ---: | ---: | ---: | ---: |
| cover, full output | +1.8% [1.2, 2.5], +1.18 calls | +0.7% [0.4, 1.1], +0.55 | −1.2% [−2.4, −0.2] | −0.5% [−1.7, +0.6] |
| **cover, compact on pass** | **+2.8% [2.0, 3.7], +1.18 calls** | **+1.2% [0.7, 1.7], +0.55** | **+0.6% [−0.1, 1.3], +0.38** | **+1.0% [0.3, 1.7], +0.44** |
| cover, compact, gated p ≥ 0.5 | +2.9% [2.0, 3.7] | +1.1% [0.7, 1.6] | +0.6% [−0.1, 1.3] | +1.0% [0.4, 1.7] |
| … p_skip 0.5 | +1.1% [0.7, 1.5] | +0.4% [0.2, 0.6] | +0.1% | +0.4% [0.1, 0.7] |
| … harm 1 call per miss | −0.9% [−1.6, −0.2] | −0.6% [−1.3, −0.1] | −9.1% | −3.4% |
| cover, compact, gated p ≥ 0.7 | +0.7% [0.4, 0.9] | +0.3% [0.1, 0.4] | +0.2% | +0.1% |
| exact (the agent's next run is V verbatim), compact | +0.9% [0.5, 1.3] | +0.4% [0.2, 0.8] | −0.4% | −0.2% |

- **Compact on pass is a small trim of output.** It assumes the lines of a passing run are never needed again (token-cost found trimmed lines tend to come back). The no-trim row, "cover, full output", is the conservative number: Haiku +1.8% (split), +0.7% (LORO).
- Misses run at about 1.15 per session (Haiku split) and 1.4–2.6 (Sonnet): the edit turn is
  followed by more edits, or by a differently scoped run.
- τ, cross-fitted on the pool, came out at 0.2 (split) and 0.3–0.4 (LORO).

### Environment setup (`setup_offload.py` → `results/setup.json`)

| | Haiku split | Haiku LORO | Sonnet LORO |
| --- | ---: | ---: | ---: |
| conservative | +0.1% [−0.0, +0.3] | −0.1% [−0.3, +0.2] | −0.5% |
| upper bound: all friction removed | +2.5% [1.1, 4.2] | +7.1% [5.7, 8.8] | +3.3% [1.5, 5.9] |

- The learned setup is `pip install pytest -q`, plus environment probes (`ls -la | grep
  setup|pyproject`) that occur in repair windows.
- The split pool is too small: repairs recur only in marshmallow.

### Trajectory: earlier failure detection (`trajectory.py` → `results/trajectory.json`)

| | Haiku | Sonnet |
| --- | ---: | ---: |
| failing test runs per session | 1.08 | 1.59 |
| … preceded by edits in more than one turn since the last test run | 35 of 170 (21%) | 9 of 51 |
| U: every lag call removed (upper bound) | +5.6% [3.6, 7.7], +1.44 calls | +6.4% [2.3, 10.4] |
| D: dead-end lag edits and their runs | +1.8% [1.0, 2.7], +0.39 calls | +1.2% [0.1, 2.7] |
| **C: conservative** | **+1.3% [0.6, 2.0], +0.23 calls** | **+0.4% [0.0, 1.2]** |

This is a pass-rate-neutral, cost-only reading. It assumes the earlier signal would have been acted
on, and ignores the opposite risk: mid-change red results sending the agent off course.

## vs baseline

- **The current state** is U11 plus the learned reflexes: +1.0 Ŝ calls per session post-hoc and
  +2.11 pre-emptive (≈ 3.9% of $).
- **Verify-after-edit** is the same size or smaller: +1.18 calls / +2.8% (split), +0.55 /
  +1.2% (LORO). It acts on different calls (post-edit test runs, not friction), so the two would
  roughly add.
- **Setup offload** overlaps U11 and adds ≈ 0 when measured conservatively.

## Hard-coded parts (honest list)

- **Authoring-tool set** (`Edit`, `Write`, `MultiEdit`, `NotebookEdit`). It defines "an edit"
  (the product's existing E2 edit stimulus) and what a hook may not author.
- **Shell syntax:** the first word after `VAR=` assignments is the command template; a
  here-document or `-c`/`-e` marks an inline script; a path regex extracts printed paths. The
  benchmark sandbox root is stripped (data preparation).
- **Measurement instruments only, never a learner input:**
  - harness `features.Act` (`run`, `targets`, `executed`, `env_friction`, `env_probe`): test-run
    coverage, the trajectory windows and the ceiling classes;
  - `ceilings.py` / `stretches.py` category regexes (git, lint/format, search);
  - `trajectory.FAIL_TEXT`.
- **Fixed constants:**
  - window W = 3;
  - 40-token wrapper; 60-token compact pass summary;
  - tokens/char 0.324 (Haiku) and 0.387 (Sonnet), only for misses;
  - LORO pool of 120 sessions;
  - "revised" = at least 50% of lines undone; repair specificity 0.5; value-rule output of 150
    tokens.
- **Learned:** the candidate scorer, θ and τ (pool cross-fitting), template verifier scores, the
  gate, and the repair set.

## What to build

- **Don't build a general offload engine.** Its oracle is 6.3% (Haiku) / 1.9% (Sonnet) for
  exactly predictable calls, and the learned held-out result is ≈ 0.
- **Maybe build E2 "verify after edit"** as an experiment, not as a product bet:
  - after an edit turn, re-run the agent's own last verifier command (learned template score);
  - deliver `exit 0 · N passed` on a pass and the full output on a fail;
  - expected: **+1–3% of $ and +0.5–1.2 calls per Haiku task, ≈ +1% on Sonnet**.
- **The micro-slice that would confirm it.** Take 5 click/attrs tasks where it fires most, and run
  10 pairs (about $5.4). Measure:
  - **p_skip**: the share of deliveries after which the agent does not run a covered test before
    its next edit;
  - **harm**: whole-trajectory calls after a *miss* delivery, against the matched control.
  - Pass if p_skip ≥ 0.6 and the local saving ≥ 0.6 calls per firing session.
- **For the sprint.** Turn removal is the right *currency*: each turn is worth about 2% of $. But
  the turns that can be removed without the model's reasoning add up to at most 10–15% of $, and a
  hook realistically reaches a tenth to a fifth of that. A 15–20% lever still has to change what
  the agent *decides* (fewer investigative re-runs and reads), not execute its decisions for it.

## Reproduce

Run from `evals/lab/research/execution-offload/`. Prefix each command with
`OMP_NUM_THREADS=1 PYTHONHASHSEED=0 nice -n 10`.

```
python3 ceilings.py        # results/ceilings.json   (~1 min)
python3 union.py           # results/union.json
python3 stretches.py       # results/stretches.json  (~2 min)
python3 verifier.py        # results/verifier.json   (~5 CPU-min)
python3 setup_offload.py   # results/setup.json
python3 trajectory.py      # results/trajectory.json
python3 evaluate.py        # results/learned.json    (~15 CPU-min, LORO pool subsampled to 120)
python3 diag_test_positions.py; python3 diag_reruns.py; python3 diag_chains.py   # results/diag_*.txt
```
