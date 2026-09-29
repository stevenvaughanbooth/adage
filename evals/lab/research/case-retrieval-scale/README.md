# Does case-based retrieval become the lever as memory grows? (`case-retrieval-scale`)

2026-09-29. Offline, $0, over RepoBench v1 + v2 traces already on disk.

## Question

The embedding lab (`evals/lab/README.md`) rejected kNN at 10 training sessions per repository but
noted that grown memory was the one place it beat the prior. Real users will accumulate hundreds of
sessions. Does outcome-weighted case-based retrieval (ExpRAG / ReasoningBank style) become *the*
lever once memory is large? And does a cheap variant, which retrieves whole past sessions and
delivers a 3-line "what worked last time", do better?

## Verdict: no. The ceiling is small and retrieval does not climb toward it

- **The learning curve is flat.** Harm-adjusted net calls saved per held-out session, for the best
  kNN policy (skip-ahead plus avoid-what-failed):
  - −0.04 at 5 sessions of same-repository memory;
  - +0.10 at 50;
  - +0.01 to +0.03 at all ~100–130.
  - Fitted slope: +0.014 calls per doubling of memory.
  - Skip-ahead alone, next-action and the whole-session summary are flat or falling.
  - None of them reaches the U11 baseline (+1.00 post-hoc) at any memory size, and the log-linear
    extrapolation crosses it only beyond 10⁶ sessions, which means never.
- **The trivial prior is still better at every memory size.** The lab's "most-often-useful command,
  once per session" prior scores +0.07 → +0.36 [+0.21, +0.51]. It is a counting pass and needs no
  embeddings. Retrieval adds nothing on top of it that survives the harm of wrong directives.
- **Memory from other repositories:**
  - On its own it is harmful: skip −0.24, summary −0.32, prior −0.32.
  - Added to same-repository memory it lifts the summary variant from −0.27 to +0.09
    [−0.07, +0.24]. That is not significant, and even the no-harm number is only +0.51.
  - Once there is enough of it, it helps a little, but not enough to matter.
- **The ceiling is small.** A hindsight oracle over this lever's directives (run or avoid) removes
  **1.90 failed commands per session: 6.0% of calls and 4.0% of $.** Even perfect retrieval stays
  below the north star's 15–20%. The "loose" bound (51%) counts every exploratory read, most of
  which the agent needed.
- **Pass rate:** not estimable offline. These are command-discovery hints, and nothing here suggests
  they would change hidden-test outcomes.

**So what.** Shipped, case retrieval would save about 0.0–0.1 calls per session (≤ 0.3% of calls and
$; at full memory the 90% CI includes 0), with no visible pass-rate effect. We are highly confident a
live A/B would show nothing meaningful. **Do not build it.** Keep the lab's per-repository counting
reflex, which improves with memory: +0.29 to +0.36 at around 100 sessions, and +0.73 [+0.50, +0.97]
on the v2 Haiku subset. At most, add the commands-only session summary as a SessionStart advise:
+0.15 [+0.05, +0.26], precision 0.73, about 0.5% of calls, so it is low priority.

## Data and protocol

- **Sessions:** 554 de-duplicated sessions (by session id; v1 + v2, all arms, 7 slices, 5
  repositories) (`data.py`). `click-sonnet5` and `vue-core-sonnet5` count as the same repository
  and the same tasks as their Haiku slices.
- **Test set:** every held-out **control-arm** session, 208 in total (`attrs` 49, `click` 47 + 16
  Sonnet, `marshmallow` 30, `urfave-cli` 16, `vue-core` 34 + 16 Sonnet).
- **Memory:** sessions of **other tasks** only (leave-task-out), from any arm (train, control, adage,
  agents-md).
- **Memory sizes:** N ∈ {5, 10, 20, 50, 100, all} sessions sampled from the same repository (3, 3,
  3, 2, 1 and 1 seeds, averaged per session).
  - N=100 exists only for attrs, click and vue-core, so table B reports that family at every N.
  - "all" means 50–150 sessions, about 99 on average.
- **Cross-repository memory:** all ~442 sessions of the other repositories, alone and added to
  same-repository memory of 5, 20 or all sessions.
- **Thresholds:** the agreement θa and value θv are chosen **leave-one-repository-out**. For each
  repository, we take the grid point with the most harm-adjusted net calls on the *other*
  repositories' test sessions, and abstain if none is positive.
- **Uncertainty:** 90% bootstrap over sessions, and Wilson 90% intervals for precision.

## Method (`casebank.py`)

- **Situation.** A decision point comes after the prompt and after each tool batch. It is
  represented by three potion-base-8M vectors:
  - the prompt;
  - the last 5 actions as issued, with "[failed]" from the harness's `is_error`;
  - the last tool output, masked Drain-style (digits and hex runs become `<*>`), which serves as
    the output template.

  Similarity is the mean of the three cosines. Retrieval takes the k = 20 nearest situations in
  memory, and each neighbour is weighted by sim² × (1 if its session passed hidden tests, else 0.5).
- **Learned from memory, not hand-written:**
  - **Failure** comes from `is_error`.
  - **Action equivalence:** complete-linkage clusters of the action keys' own potion vectors at
    cosine ≥ 0.95, within a tool. This merges pairs such as `python`/`python3 -m pytest` and
    `python3`/`python <<heredoc`.
  - **Achieving actions (landmarks):** action classes that passed sessions in memory take
    successfully in ≥ 20% of sessions (outcome-weighted), with a success rate ≥ 0.5 and ≥ 2
    sessions. No runner or command is named anywhere.
- **Successor targets** of a memory situation:
  - **next:** the action taken next, if it succeeded, weighted by exp(−steps to the next
    landmark / 10). This is "what the agent did next that led to progress fastest".
  - **skip:** every achieving action within 20 calls ahead. Its value is the gap, i.e. the calls a
    skip-ahead directive would jump over.
  - **avoid:** commands that failed in the next 3 calls, with the command's memory failure rate as
    the value.

  A candidate is delivered when its weighted agreement ≥ θa and its value ≥ θv, with support from
  ≥ 2 sessions, at most once per session. The learner never proposes an action the session already
  ran successfully.
- **Whole-session variant:**
  - Retrieve the 5 memory sessions with the most similar prompts, weighted sim² × outcome.
  - Vote over their achieving actions.
  - Keep the top 3 with ≥ 2 supporting sessions, and deliver them at the prompt as one advise.
  - Examples at full memory, commands only:
    - vue-core: `npm test {test_file}` (agreement 1.0, 5 sessions);
    - urfave-cli: `go test` (0.8);
    - attrs: `pip install -e .`, `python -m pytest {test_file}`, `pip install` (1.0 / 1.0 / 0.8);
    - click: `python -m pytest {test_file}`, `python3 <<heredoc`, `python {test_file}`.
- **Baselines:**
  - (i) the same landmark target voted by all memory sessions with no similarity;
  - (ii) the lab's "most-often-useful command", which uses the lab's hand-written usefulness label
    on memory sessions and is included as a baseline to beat.

## Scoring (`score.py`, the measurement instrument)

**Run directive** "do `a`", delivered before call i:

- t₁ is the first later call of class `a`. If `a` never succeeds from t₁ on, the directive is
  **wrong**.
- **Calls saved** are the failed *commands* in [i, t₁), which is the Ŝ "other command" rule
  tightened. Failed searches and failed Edits are not counted, because a run directive doesn't
  head them off.
- The directive is **useful** if its window holds a failure or ≥ 2 exploratory steps (the lab's
  definition). Precision is useful ÷ issued.

**Avoid directive** "`a` fails here":

- It is **useful** if the agent's next `a` failed. The calls saved are the failed `a`s until one
  succeeds.
- It is **harmful** if the agent's next `a` succeeded.

**Session totals:**

- Windows are unioned per session, and superseded directives are dropped.
- **Cost** C(d) = tokens × the cache-read rate × API turns left.
- **Harm:** each wrong or harmful directive, if followed, costs 1 call. **The headline is net calls
  after harm**, and the no-harm number is reported next to it.
- **$:** each call's marginal cost under the cumulative-context model. That is the call's share of
  its API turn plus its tokens carried by every later turn at the cache-read rate, rescaled so a
  session's marginals sum to its total cost, priced with `pricing.toml`.

## Results

All numbers below are held-out, leave-task-out, with leave-one-repo-out thresholds.

### Learning curve (attrs + click + vue-core, where every size exists; 162 sessions; harm-adjusted net calls/session [90% CI])

| policy | N=5 | 10 | 20 | 50 | 100 | all (~130) | slope/doubling |
|---|---|---|---|---|---|---|---:|
| kNN skip-ahead | −0.01 [−0.03, −0.01] | −0.03 | −0.01 | −0.01 | −0.03 | −0.03 [−0.06, −0.01] | −0.002 |
| kNN next action | −0.00 | +0.00 | +0.00 | −0.00 | −0.00 | −0.01 | −0.001 |
| kNN avoid-what-failed | −0.03 | +0.00 | +0.08 | +0.12 [+0.05, +0.19] | +0.05 | +0.04 [−0.02, +0.12] | +0.017 |
| kNN skip + avoid | −0.04 | −0.03 | +0.06 | +0.10 [+0.03, +0.18] | +0.02 | +0.01 [−0.06, +0.09] | +0.014 |
| whole-session summary (3 lines) | −0.04 | −0.23 | −0.26 | −0.35 | 0.00 (abstains) | −0.35 [−0.46, −0.23] | −0.022 |
| prior: landmark, no similarity | +0.01 | 0.00 | 0.00 | 0.00 | −0.17 | 0.00 | −0.019 |
| **prior: lab's most-often-useful command** | +0.07 [−0.05, +0.18] | +0.15 | +0.22 | +0.32 | +0.20 | **+0.29 [+0.11, +0.48]** | +0.039 |

### All 208 test sessions, all memory (same repository)

| policy | directives/session | precision [Wilson 90] | useful/session | failed cmds saved | net calls, no harm | **net, harm-adjusted** | net $ |
|---|---:|---|---:|---:|---|---|---:|
| kNN skip-ahead | 0.07 | 0.57 [0.36, 0.76] | 0.04 | 0.00 | −0.00 | **−0.03 [−0.04, −0.01]** | −0.1% |
| kNN next action | 0.10 | 0.10 [0.03, 0.26] | 0.01 | 0.00 | −0.00 | **−0.01 [−0.02, −0.00]** | −0.0% |
| kNN avoid-what-failed | 0.30 | 0.33 [0.24, 0.44] | 0.10 | 0.13 | +0.12 [+0.08, +0.18] | **+0.05 [−0.00, +0.11]** | +0.1% |
| kNN skip + avoid | 0.37 | 0.38 [0.29, 0.47] | 0.14 | 0.13 | +0.12 | **+0.03 [−0.03, +0.09]** | −0.1% |
| whole-session summary | 0.64 | 0.45 [0.38, 0.52] | 0.29 | 0.08 | +0.07 | **−0.27 [−0.36, −0.17]** | −1.3% |
| whole-session summary, commands only | 0.68 | 0.73 [0.66, 0.78] | 0.50 | 0.35 | +0.34 | **+0.15 [+0.05, +0.26]** | +0.1% |
| prior: lab's most-often-useful command | 1.00 | 0.72 [0.66, 0.76] | 0.72 | 0.66 | +0.64 [+0.53, +0.77] | **+0.36 [+0.21, +0.51]** | +0.3% |

Sessions average 31.8 calls, so +0.36 calls is 1.1% of calls.

### Memory from other repositories (harm-adjusted net calls/session, 208 sessions)

| memory | skip | avoid | skip + avoid | summary | lab prior |
|---|---|---|---|---|---|
| same-5 | −0.01 | −0.03 | −0.04 | +0.02 | +0.13 |
| other repositories only (~442 sessions) | −0.24 | +0.06 | −0.19 | −0.32 | −0.32 |
| same-5 + other | −0.08 | +0.10 | −0.02 | −0.18 | −0.32 |
| same-20 + other | +0.03 | +0.06 | +0.07 | +0.08 | −0.32 |
| same-all + other (~541 sessions) | −0.02 | +0.06 | +0.03 [−0.08, +0.15] | +0.09 [−0.07, +0.24] | −0.32 |

Cross-repository memory adds candidates that are right on the other repositories: skip-ahead
precision 0.24 on its own, and a pooled prior that is wrong on every repository. It helps only the
summary variant, and only once there is same-repository memory to anchor it.

### U11-comparable subset (98 v2 Haiku control sessions of click / attrs / marshmallow / vue-core)

| memory | skip + avoid | summary | lab prior |
|---|---|---|---|
| same-5 | −0.03 | +0.12 [+0.02, +0.21] | +0.31 [+0.16, +0.46] |
| same-all | +0.09 [−0.01, +0.20] | −0.07 | **+0.73 [+0.50, +0.97]** |
| same-all + other | +0.17 [−0.02, +0.37] | +0.31 [+0.06, +0.53] | −0.17 |

For comparison, U11 + learned reflexes give **+1.00 [+0.56, +1.46]** post-hoc and **+2.11**
pre-emptive (Rust Ŝ, 82 sessions). No retrieval policy reaches the post-hoc number. The directives
it does find (`python -m pytest`, `pip install -e .`, `npm test`, `go test`) are the same knowledge
as U11 and the prior, so their additive value is at most what is shown here.

### Ablations (same repository, all memory, harm-adjusted)

| policy | skip | avoid | skip + avoid | summary |
|---|---|---|---|---|
| full (prompt + history + output, outcome-weighted) | −0.03 | +0.05 | +0.03 | −0.27 |
| no outcome weighting | +0.00 | +0.06 | +0.06 | −0.16 |
| no prompt | −0.09 | +0.07 | −0.03 | −0.27 |
| no output template | −0.09 | +0.09 | −0.01 | −0.27 |
| history only | +0.02 | +0.10 | +0.10 [+0.02, +0.19] | −0.27 |
| k = 50 | −0.02 | +0.05 | +0.02 | −0.27 |
| commands only | −0.08 | +0.05 | −0.02 | **+0.15 [+0.05, +0.26]** |

Outcome weighting, the prompt vector and the output template add nothing measurable, which is the
lab's finding again at 10× the memory. Restricting candidates to commands is the one change that
helps: file reads and edits from other tasks are wrong most of the time.

### Latency and size

| memory | sessions | situations | stored (i16 potion-8M) | query per decision point (Python, brute force) |
|---|---:|---:|---:|---:|
| one repository, all | 109 | 3,085 | 3.3 MB | 0.87 ms |
| every repository | 540 | 16,323 | 17.3 MB | 4.36 ms |

potion-8M embeds one text in 0.07 ms. Cost is not the problem, since both sizes are well inside the
hook's 25 ms budget. Value is the problem.

## Why it doesn't scale

1. **What transfers between tasks is repository know-how, not situations.** Across tasks, what
   repeats is "how to run the tests / install here". That is one or two commands per repository,
   and a count finds them once there are about 10 sessions. Situation-level successors, such as the
   next file to read or the fix path, are task-specific, and more memory from *other* tasks doesn't
   make them transfer. Top-k neighbours become more numerous, not more relevant.
2. **Harm grows with recall.** Lowering θ to deliver more roughly doubles "calls saved" (skip-ahead
   saves 1.1 failed commands/session at θ = 0.1), but 70% of those directives name a command the
   session never ran successfully. At one call each, that wipes out the gain. The
   leave-one-repo-out threshold therefore settles near abstention.
3. **The ceiling is small.** Failed commands are 2.45 per 32-call session, and 1.90 of them can be
   reached by any run or avoid directive (6% of calls, 4% of $). Most of the "waste" in a window is
   reading code the agent needed, which no skip-ahead can remove.

## Hard-coded parts (honest list)

- **Action identity:** the lab's `normalize.py` `directive_key` (shlex rules, slot regexes,
  explore and destructive lists). A learned embedding equivalence at cosine ≥ 0.95 sits on top of
  it.
- **Candidate safety filter:** `directive_eligible`. Avoid directives are shell commands only.
- **Measurement only:** the lab's usefulness definition, its discovery regex and "edit tool", plus
  a harm model of 1 call per wrong or harmful directive with uptake assumed to be 1. The lab-prior
  baseline uses the usefulness label on memory sessions. The kNN learner does not use it.
- **Generic digit and hex masking** of tool output.

## Caveats

- A single model family (Haiku 4.5, plus a Sonnet 5 slice of 80 sessions, 32 of them held out), bug-fix tasks, and 5
  repositories.
- "All" memory is about 100–130 sessions per repository, not the several hundred of a heavy user,
  so beyond that point the extrapolation is from the fit. Every slope is ≤ 0.04 calls per doubling.
- Exact-class matching is conservative. Some "wrong" directives name an equivalent command the
  learned equivalence missed. The no-harm column is the optimistic bound, and even it stays below
  +0.64.
- This is offline replay. Uptake of retrieved hints is unknown, and U11 was followed 41 of 41 times
  live.

## Reproduce

```sh
cd evals/lab/research/case-retrieval-scale
nice -n 10 env OMP_NUM_THREADS=1 python3 data.py     # sessions -> /tmp/adage-research/case-retrieval-scale
nice -n 10 env OMP_NUM_THREADS=1 python3 run.py      # ~11 min -> result_raw.json
python3 report.py > tables.md                        # every table -> tables.md, extrapolation.json
python3 diag.py all                                  # the full θ grid, in-sample (diagnostic only)
```

Files:

- `data.py`: loading and marginal $.
- `casebank.py`: situations, embeddings, learned equivalence and landmarks, memory, the three
  targets and the summary.
- `score.py`: the instrument.
- `run.py`: the protocol.
- `report.py`: the tables.
- `diag.py`: threshold grid diagnostics.
- `tables.md`: the full output, including the per-size rows for every policy.
