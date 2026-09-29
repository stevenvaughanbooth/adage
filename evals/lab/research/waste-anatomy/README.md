# Waste anatomy: where sessions spend calls, tokens and dollars, and what each lever could save

2026-09-29, slug `waste-anatomy`. Offline, $0. It covers the held-out **control** sessions of RepoBench
v1 and v2, both Haiku 4.5 and Sonnet 5, and every tool call priced from the measured `usage` of its
API request.

## Question

From first principles, where do agent sessions spend their calls, tokens and dollars? What is the
oracle ceiling of each possible lever, and what fraction of it can realistically be reached?

## Verdict (the "so what" first)

- **The cumulative-context cost model is real in tokens, but the 1-hour cache takes most of its bite
  in dollars.**
  - Every token a call appends is re-processed **36× (token volume)** over the rest of a session.
  - It is written to cache once at $2/MTok and then read ~22 times at $0.10, so its lifetime cost is
    **2.2× its cache-write cost**.
  - An early 5k-token output costs **$0.026 over a Haiku session, 8.6% of the session's $0.30**.
  - Where the dollars physically go (Haiku v2):
    - 31% re-reading carried content;
    - 29% cache writes;
    - 23% output tokens;
    - 16% re-reading the fixed 13k-token system/tool prefix on every request.
  - So even a call that appends nothing costs about $0.003.
- **The oracle ceiling is huge, but the waste isn't where a reflex can reach it.**
  - An oracle that removes every call not in the provenance slice of the final diff saves **74% of
    calls and 68% of $** (Haiku v2 [66, 70]; Sonnet 64% / 65%).
  - The ceiling survives loose definitions of "needed": 46–62% of $.
  - **Orientation is 49% (Haiku) / 56% (Sonnet) of all $.** But only **1.4 of its calls per session
    are searching** before the agent first opens the file it will change. That is a perfect-pointer
    ceiling of **5.8% of $** [4.3, 7.5].
  - The other ~7 unneeded orientation calls per session come after the agent has already found the
    right file: reading to understand. No natural experiment moves them. AGENTS.md changed
    orientation by **+0.3 calls [−0.7, +1.4]** (Haiku) and **0.0 [−1.5, +1.6]** (Sonnet).
- **Pre-loading file contents is net negative.**
  - A needed file averages 13.8k tokens.
  - Each 1k tokens pre-loaded costs $0.006 per session, because it is carried through all ~37 requests.
  - Even the task's own gold source file, pre-loaded, **loses 6% of $** [−9, −3].
  - BM25@3 loses 74%; trace-memory@1 loses 49%.
- **The only lever with a double-digit realistic value is output trimming, and only for Haiku.**
  - A learned per-tool budget (Read ≤ 1000 tokens, head-weighted; Bash ≤ 2000) was tuned on the train
    split and scored on held-out sessions with a harm model.
  - It nets **11.1% of $ [9.1, 13.4]** on Haiku v2 and 6–24% leave-one-repo-out.
  - It costs **+2.8 calls per session** to re-fetch trimmed lines.
  - On **Sonnet it nets −0.1% [−1.7, +1.6]**: Sonnet already reads line ranges.
- **Environment friction is 11% of $ for Haiku and 4% for Sonnet.** In the adage arm the current
  system already removes **−0.64 calls [−1.12, −0.18]** of it, which is 1.3% of $. A pre-emptive
  version might reach half the ceiling: about 5% for Haiku and 2% for Sonnet.
- **Failure is the biggest structural sink, but it can't be stopped early.**
  - **48% of Haiku $ and 72% of Sonnet $ is spent in sessions that fail.** Failed sessions run +14
    calls longer, driven by orientation (+8.2), tests (+2.8) and dead-end edits (+2.5).
  - An oracle that stopped doomed sessions early would save 22% (Haiku) / 38% (Sonnet) of $.
  - Doom is not predictable online. A leave-one-repo-out AUC of 0.42–0.56 means any stop policy
    costs 4–35 pass-rate points. **Don't build it.**
  - Verifying correlates with passing. Within a task, failed sessions verify **3.6 calls less**
    [−6.3, −1.0] (4 mixed tasks). So do not cut verification.

**So what.**

- **Haiku.** Output trimming (≈ −11% $, CI [−13, −9], +2.8 calls), pre-emptive friction removal
  (≈ −5% $) and a "don't commit unless asked" directive (≈ −3% $) together plausibly reach
  **≈ 15–19% of $**. Calls fall by only ~2–3, because trimming adds calls back. Confidence that it
  survives a live A/B is **moderate for trimming** (the harm model is the weak link) and **high for
  friction** (already measured live).
- **Sonnet.** No lever in this map reaches 5% realistically.
- **Pass rate.** None of these levers is a pass-rate lever. What drives $ per resolved task is the
  resolve rate: +5 points is worth −7% (Haiku) / −10% (Sonnet) of $ per resolved task. The segments
  that separate failing sessions are dead-end edits and test churn, and those belong to
  success-uplift, not to a cost reflex.

## Data

- **Sessions.** The held-out control-arm sessions, de-duplicated by session id and restricted to the
  run each scorer recorded (`state.json` `metrics.session_id`). That drops 18 v2 transcripts the
  narrative says were discarded and rerun.
- **Uncensored (60-turn cap, headline):**
  - **Haiku v2: 82 sessions**;
  - **Sonnet v2: 16**, on click and vue-core only; the Haiku comparison is restricted to the same
    two repositories (40 sessions).
- **Censored (30-turn cap), reported as a robustness check:**
  - Haiku v1: 76 (including urfave-cli);
  - Sonnet v1: 16.
- **Other arms.** Train (57 sessions) is used for everything learned. Adage and agents-md are used as
  natural experiments.
- **Prices** are list prices, checked against each run's `total_cost_usd`:
  - Haiku: $1 input, $2 cache write (1h), $0.10 cache read, $5 output.
  - Sonnet 5: exactly 2× Haiku.
  - The usage-based cost reproduces the reported cost (median ratio 1.000). The counterfactual
    re-pricer reproduces the measured cost to within 1% (median 0.99998).

## Method

- **Requests and tokens (`load.py`).** Each assistant API message carries its measured context C_k
  (input + cache write + cache read), its output and its cache write. Context growth
  D_k = C_{k+1} − C_k is split over that request's tool calls:
  - result tokens by characters (3.08 chars/token, fitted, R² 0.91);
  - the rest is per-request overhead: tool-use JSON, retained output, hook text.
- **Final diff by replay (hindsight, not hand-coded).**
  - Every successful Edit/Write/MultiEdit is replayed against the task's base commit, which is the
    repository's root commit. `HEAD` is not the base because agents `git commit` their work.
  - An edit **survives** if its added lines are among the final diff's added lines, in a file the
    scorer lists as changed or whose replayed content differs from base.
  - 13 of 190 sessions have no surviving tool edit (edits made through Bash).
- **Provenance and the needed slice (hindsight, minimal hand-coding).**
  - A later call *uses* an earlier result if its input or the visible text of its request contains a
    rare token that the result introduced first.
  - "Rare" is a **learned** stoplist: tokens in more than 2% of train-split results are excluded.
  - The **needed slice** is the backward closure from surviving edits, plus the Edit tool's own
    read-before-edit rule.
  - Thinking is redacted in the transcripts, so the slice is a lower bound on "needed" and the
    oracle is an upper bound on savings.
  - Instrument check: 94% of gold source files that were read have a needed read. Reads of gold
    source files are needed 73% of the time, other reads 53%.
- **Segments.**
  - Each call gets one primary segment, by precedence: env_friction > redundant > edit (final or
    dead-end) > dead-end runs > verify > test > repro_exec > orient > vcs.
  - The role underneath is a **hand-coded measurement instrument** (declared): tool name, the lab
    `normalize.py` explore flag, and three command regexes (test runner, setup/installer, `git`).
  - Everything else is hindsight or learned:
    - *Failure* comes from a **learned** detector: TF-IDF char n-grams plus logistic regression on
      train-split `is_error`, held-out AUC **0.99**. It finds 0.27 pipe-hidden failures per session.
    - *Friction* is a failed shell call that is later re-invoked in a similar form (char-3-gram
      cosine ≥ 0.35) with **no edit in between** and then succeeds. The failure was fixed by changing
      the invocation or the environment, not the code. The detour calls in between also count, as do
      setup-role calls.
    - *Redundant* is an exact re-read of a range already read with the file unchanged, or an
      identical re-run with no edit in between whose twin did not fail.
    - *Dead-end* is an edit that did not survive, plus the runs that only exercised it.
    - *Verify* is test or exec after the last surviving edit.
    - *Bloat*, used for trimming, is the result lines holding no token the agent ever used again.
- **Re-pricing (`ceilings.py`).**
  - Removing a call removes its tokens from every later context.
  - Removing every call of a request removes the request: its prefix read and its output.
  - Kept requests re-read at the cache-read price and write what is new at the cache-write price.
  - The rest of the session is held fixed. That is what makes these ceilings.
  - CIs are 90% bootstraps over sessions. Arm comparisons are task-clustered: per-task mean
    difference, bootstrapped over tasks.

## Results

### 1. Cost structure and the carry multiplier

| | Haiku v2 (82) | Sonnet v2 (16) |
| --- | ---: | ---: |
| calls / session | 36.0 [33.5, 38.6] | 31.6 [24.5, 38.9] |
| $ / session | 0.301 [0.272, 0.331] | 0.636 [0.447, 0.844] |
| fixed prefix (first request) | 13.2k tokens | 17.5k tokens |
| context at the last request | 52.5k [49.4, 55.9] | 54.6k [43.9, 65.8] |
| $ share: re-reading the fixed prefix | 16% [15, 17] | 18% [15, 21] |
| $ share: re-reading carried content | 31% [30, 33] | 28% [23, 32] |
| $ share: cache writes (1h) | 29% [28, 30] | 27% [26, 30] |
| $ share: output tokens | 23% [22, 24] | 27% [25, 30] |
| later re-reads of an appended token (token-weighted) | 21.7 [20.3, 22.9] | 17.8 [13.6, 21.9] |
| tokens processed per token appended | 36.2 [34.6, 37.8] | 37.1 [33.1, 41.1] |
| lifetime $ of a token / its cache-write $ | **2.21** [2.13, 2.28] | 2.20 [1.93, 2.41] |
| lifetime $ of 1k tokens appended at request 1 / 10 / 20 (median) | $0.0051 / 0.0042 / 0.0033 | $0.0090 / 0.0079 / 0.0072 |
| lifetime $ of a 5k-token output at request 3 | $0.026 (8.6% of a session) | $0.047 |

### 2. Anatomy (held-out control, v2; calls/session and share of $, lifetime-attributed)

| segment | Haiku calls | Haiku $ share | Sonnet calls | Sonnet $ share |
| --- | ---: | ---: | ---: | ---: |
| orient (reads, greps, listings) | 12.8 [11.1, 14.4] | **49%** [46, 51] | 15.2 [11.1, 19.8] | **56%** [49, 62] |
| env friction | 4.6 [4.0, 5.2] | 8% [6, 9] | 1.1 [0.2, 2.2] | 3% [0, 5] |
| test (before the last final edit) | 2.4 [1.7, 3.0] | 4% [3, 5] | 5.0 [3.1, 6.9] | 11% [8, 15] |
| repro / other exec | 1.9 [1.4, 2.5] | 4% [3, 5] | 1.5 [0.9, 2.1] | 3% [1, 5] |
| edits in the final diff | 3.4 [3.0, 3.8] | 12% [10, 13] | 3.9 [3.2, 4.7] | 11% [9, 13] |
| dead-end edits | 1.4 [0.9, 1.9] | 5% [3, 8] | 0.9 [0.2, 1.6] | 3% [1, 5] |
| runs of dead-end code | 1.2 [0.7, 1.7] | 2% [1, 2] | 0.1 [0.0, 0.4] | 0% |
| redundant (exact re-read / re-run) | 0.9 [0.6, 1.2] | 3% [2, 4] | 0.8 [0.5, 1.2] | 3% [2, 6] |
| verify (after the last final edit) | 4.0 [3.5, 4.5] | 6% [5, 7] | 2.2 [1.6, 3.1] | 3% [2, 4] |
| vcs (git log/add/diff/commit; unprompted in 98/98) | 3.6 [3.2, 3.9] | 4% [3, 5] | 0.8 [0.5, 1.1] | 2% [1, 4] |

Output bloat is an overlay on these segments.

- **Haiku:** 80% of tool results never supply a token the agent uses again, and 72% of
  result characters sit on lines that are never used.
- **Sonnet:** 75% and 63%.

### 3. Oracle ceilings (a, b): share saved if the lever were perfect (Haiku v2 | Sonnet v2)

| lever (oracle) | calls saved / session | calls | context tokens | **$** |
| --- | ---: | ---: | ---: | ---: |
| (a) remove every call not needed for the final diff | 26.7 [24.9, 28.6] \| 20.3 | 74% \| 64% | 81% \| 76% | **68% [66, 70] \| 65% [57, 71]** |
| (a) … keeping one final test run | 25.8 \| 19.3 | 72% \| 61% | 79% \| 74% | 66% \| 63% |
| (a) … only what the graded source diff needs | 30.8 \| 24.2 | 86% \| 77% | 90% \| 85% | 80% \| 76% |
| (a) sensitivity: keep all reads / all referenced / all exploration | 23.8 / 24.2 / 18.3 | | | 60% / 62% / 46% (Haiku) |
| know the answer from past sessions (edits + licensing reads + 1 test) | 29.6 \| 24.2 | 82% \| 76% | 89% \| 87% | 79% \| 79% |
| pre-load the right files (needed read tokens at the prefix; all exploration gone) | 14.1 \| 16.1 | 39% \| 51% | 44% \| 60% | 36% [33, 39] \| 54% [46, 61] |
| → perfect pointer (paths only; removes only search before localisation) | 1.4 \| 0.8 | | | 5.8% [4.3, 7.5] \| 2.6% [1.7, 3.9] |
| trim outputs to lines used later | 0 | 0 | 30% \| 16% | 25% [23, 27] \| 13% [10, 14] |
| post-edit tail (verify + vcs, keep 1 test) | 6.9 \| 2.3 | 19% \| 7% | 23% \| 10% | 16% [14, 18] \| 7% [5, 9] |
| skip env friction | 4.6 \| 1.1 | 13% \| 4% | 14% \| 5% | 11% [9, 13] \| 4% [0, 8] |
| avoid dead-end edits (and their runs) | 2.5 \| 1.0 | 7% \| 3% | 11% \| 5% | 10% [6, 13] \| 4% [1, 7] |
| skip exact redundancy | 0.9 \| 0.8 | 2% \| 3% | 4% \| 5% | 4% [2, 5] \| 4% [2, 7] |
| scoped tests (trim test output to used lines; drop redundant reruns) | 0.0 \| 0.0 | 0% | 2% \| 1% | 2% \| 1% |
| stop doomed sessions early (fails cut at the median pass length) | 6.5 \| 10.5 | | | 22% [16, 28] \| 38% [19, 53] |

- **Per repository (Haiku, $ ceiling):**

  | repository | oracle (a) | env friction | trim outputs | pre-load |
  | --- | ---: | ---: | ---: | ---: |
  | attrs | 68% | 18% | 21% | 31% |
  | click | 70% | 12% | 25% | 32% |
  | marshmallow | 71% | 20% | 36% | 31% |
  | vue-core | 64% | 0% | 20% | 47% |
  | urfave-cli (v1) | 58% | 2% | 27% | 50% |

  Friction is a Python-repository phenomenon.
- **v1 (30-turn cap, censored) gives the same picture.** Haiku: oracle 64%, trim 28%, pre-load 43%,
  friction 9%.

### 4. Realistic fractions (held-out, learned where possible)

| lever | realistic policy, evaluated | result | realistic share of $ |
| --- | --- | --- | ---: |
| trim outputs | per-tool token budget with head/tail split, tuned on train, harm = 1 extra call per result whose used line was trimmed | Haiku v2: gross 19.5%, harm 8.3%, **net 11.1% [9.1, 13.4]**, +2.8 calls [2.4, 3.2]. LORO: attrs 9.7, click 10.0, marshmallow 24.2, urfave-cli 6.3, vue-core 6.3. Sonnet v2: **−0.1% [−1.7, +1.6]** | **~11% (Haiku), 0 (Sonnet)** |
| env friction | the current system (adage arm vs control, v2, paired over 30 tasks) | −0.64 calls [−1.12, −0.18] of 4.21 (15%); −$0.0036 [−0.0064, −0.0010] = 1.2% of $. The live U11 A/B removed 56% of the dead-end calls, so a pre-emptive, general version might reach ~50% of the ceiling | 1.3% now → ~5% Haiku / ~2% Sonnet |
| pre-load contents | top-k files by trace memory, BM25 over the base commit, or hybrid; the first Read of an edited file is kept (tool rule) | recall of needed files @3: memory 0.24, **BM25 0.55**, hybrid 0.45. Net $: BM25@1 −19%, BM25@3 −74%, memory@1 −49%, gold file@1 −6% | **negative** |
| pointers (paths/ranges, no content) | a perfect pointer × BM25 recall@3 | ceiling 5.8% × 0.55 | ~3% (Haiku) |
| repository knowledge up front | AGENTS.md arm vs control (v1, paired over tasks) | orientation +0.3 calls [−0.7, +1.4] (Haiku), 0.0 [−1.5, +1.6] (Sonnet). Total $ −1% [−7, +4] / −7% [−16, +1] | ~0 |
| know the answer from past sessions | trace-memory recall of needed files | @5 = 0.32 [0.26, 0.37]; tasks don't repeat; other-repo memory gives 0 | ~0 on new tasks |
| post-edit tail | only the vcs habit is safe to cut; verifying correlates with passing | vcs is 4% of $ (Haiku), 2% (Sonnet) | ~3% / ~1% |
| dead-end edits | no offline mechanism; the segment tracks failure | — | unknown (pass-rate research) |
| stop doomed sessions | logistic regression on online counts, LORO, checkpoints 15/20/25 | AUC 0.42 / 0.56 / 0.50. At θ=0.9: 1.7–7.5% of $ for 3.7–6.1 pass points lost | **negative** |

### 5. (c) Pass/fail against the segments (Haiku v2: 53 pass, 29 fail)

| segment | pass | fail | fail − pass, calls [90% CI] | within task (4 mixed tasks) |
| --- | ---: | ---: | ---: | ---: |
| total calls | 31.1 | 45.1 | **+14.0 [+8.7, +18.8]**; +$0.16 [+0.10, +0.22] | +5.9 [0.0, +11.8] |
| orient | 9.9 | 18.0 | **+8.2 [+4.6, +11.7]** | +1.6 [−1.5, +4.8] |
| test | 1.4 | 4.2 | **+2.8 [+1.4, +4.2]** | +2.8 [−0.9, +7.4] |
| dead-end edits | 0.5 | 3.0 | **+2.5 [+1.3, +4.0]** | **+2.8 [0.0, +6.8]** |
| final edits | 2.7 | 4.6 | +1.9 [+1.1, +2.8] | +0.4 |
| redundant | 0.5 | 1.4 | +0.9 [+0.2, +1.7] | −0.3 |
| env friction | 5.1 | 3.7 | −1.4 [−2.6, −0.1] | 0.0 |
| verify | 4.5 | 3.2 | −1.3 [−2.2, −0.3] | **−3.6 [−6.3, −1.0]** |

- Failed sessions take 48% [37, 57] of Haiku $ and 72% [48, 90] of Sonnet $.
- Every failed session has a surviving source edit, so failures come from wrong fixes, not from never
  finding the file.
- Sonnet shows the same signs: tests +4.6 [+1.4, +7.8], dead-end edits +1.6 [+0.6, +2.7].
- These are associations, not effects.

### 6. Haiku vs Sonnet (same repositories: click and vue-core, v2)

| | Haiku (40) | Sonnet (16) |
| --- | ---: | ---: |
| calls / $ per session | 42.9 / $0.39 | 31.6 / $0.64 |
| orientation share of $ | 49% | 56% |
| env friction calls | 3.0 | 1.1 |
| dead-end edits + runs | 3.6 | 1.0 |
| vcs calls | 3.6 (all repos) | 0.8 |
| oracle (a) $ share | 67% | 65% |
| trim ceiling / realistic | 22% / ~11% | 13% / ~0 |
| friction ceiling | 6% | 4% |

Sonnet removes most of the procedural waste on its own: friction, dead ends, commit rituals and
whole-file reads. What is left is orientation and output, at twice the price per token. **The
stronger the model, the less there is for a procedural reflex to remove.**

## Ranked lever table (Haiku v2 unless noted)

| rank | lever | $ ceiling | calls ceiling | realistic fraction | realistic $ | priority |
| ---: | --- | ---: | ---: | ---: | ---: | --- |
| 1 | **output trimming / Read paging** (learned per-tool budget, PostToolUse) | 25% [23, 27] (Sonnet 13%) | 0 (+2.8 cost) | ~0.45 | **~11% [9, 13]**; Sonnet 0 | **build and A/B first on Haiku-class models** |
| 2 | **env friction, pre-emptive** (PreToolUse rewrite; generalise U11) | 11% [9, 13] (Sonnet 4%) | 4.6 | ~0.5 (now 0.12) | ~5% (now 1.3%) | continue (already in flight) |
| 3 | vcs habit ("don't commit unless asked") | 4% | 3.6 | ~0.8 | ~3% | cheap; it's a user preference, so ship it as an opt-in instinct |
| 4 | pointers (localisation hints, no content) | 5.8% [4.3, 7.5] | 1.4 | ~0.55 | ~3% | low |
| 5 | redundant re-reads/re-runs | 4% | 0.9 | ~0.5 | ~2% | low |
| 6 | scoped tests | 2% | 0 | – | <1% | drop (agents already scope) |
| 7 | dead-end edits | 10% [6, 13] | 2.5 | unknown | ? | pass-rate research, not a cost reflex |
| — | pre-loading contents | 36% (as an oracle) | 14.1 | **negative** | −6% to −95% | **don't build** |
| — | repository knowledge up front (AGENTS.md-like) | — | — | 0 (measured) | ~0 | don't build |
| — | know the answer from past sessions | 79% | 29.6 | ~0 (memory recall 0.24–0.32; tasks don't repeat) | ~0 | only for repeat-task workloads |
| — | early stop of doomed sessions | 22% (Sonnet 38%) | 6.5 | negative (AUC ≈ 0.5) | loses pass rate | don't build |

## vs baseline

- **Friction.** The current state is U11 plus the learned reflexes: about +1.0 Ŝ calls per session
  post-hoc and +2.11 pre-emptive. That sits inside the friction lever, whose ceiling is 4.6 calls per
  Haiku session. The adage arm measured −0.64 calls of friction under this instrument, so pre-emptive
  delivery at ~2.3 calls would take roughly half the ceiling. That is the realistic limit for this
  lever class, about 5% of $.
- **Trimming.** This is the one lever class the current system doesn't touch. It adds about 11% of $
  on Haiku, at +2.8 calls.

## What to build (Rust)

1. **A learned output budget in PostToolUse (`updatedToolOutput`) for Read and Bash.**
   - Per tool (and later per repository and per action cluster), learn a token budget and a head/tail
     split from the train traces. The objective is the hindsight "line used later" signal against the
     re-fetch cost, exactly as in `trim.py`.
   - The elision marker should carry the line range so the agent can page with `offset`/`limit`.
   - Expected: **−11% $ per Haiku session [−13, −9], +2.8 calls**; ≈0 on Sonnet. Gate it by the model
     family's own read behaviour (learned: the median share of Read results over budget).
   - A/B live before claiming it. The harm model (one re-fetch per lost line group) is the weak point.
2. **Pre-emptive friction rewrite (PreToolUse).** It is already on the roadmap. It is worth ~5% on
   Python repositories with Haiku, and ~0 on vue-core and urfave-cli.
3. **Don't build** content pre-loading, AGENTS.md-style knowledge dumps, trace-memory answers for new
   tasks, or doom-stopping. Each is measured here as zero or negative.
4. **Redirect research to pass rate.**
   - Orientation after localisation (~7 unneeded calls, ~40% of $) is the agent reading to understand.
     Nothing here reaches it.
   - Failure carries 48–72% of $.
   - Dead-end edits and test churn separate failed from passed sessions. Verification goes with
     passing.
   - The next lever is one that raises the resolve rate: +5 points is worth −7% (Haiku) / −10%
     (Sonnet) in $ per resolved task.

## Hard-coded parts (honest list)

- **Role instrument** (measurement only): tool names (Read/Grep/Glob/LS, Edit/Write/MultiEdit), the
  lab's `normalize.py` explore flag, and `TEST_RX`, `SETUP_RX` and the `git` prefix in `anatomy.py`.
  This determines the orient / test / setup / vcs labels. Friction also includes every setup-role
  call.
- **Test-path regex**, used to split the final diff into source and tests (the graded-source oracle
  only).
- **Claude Code's read-before-edit rule**, used in the needed slice and in the pre-load simulation.
- **Fixed thresholds:** a stoplist cut at 2% document frequency (the stoplist itself is learned);
  re-invocation cosine ≥ 0.35; token regex `[A-Za-z_]\w{3,}`; 3.08 chars/token (fitted).
- **Harm models:** trimming (one extra call per result that lost a used line) and pointers (40 tokens
  per file).
- **List prices**, verified against `total_cost_usd`.
- The doom predictor's features count role-instrument classes.
- **Learned, not hand-coded:** the failure detector (AUC 0.99 held-out), the stoplist, edit survival,
  provenance, and the trim budgets (train split, LORO).

## Caveats

- **Ceilings hold the rest of the session fixed.** Removing a call can change what the agent does next.
  That is why realistic fractions are measured separately.
- **Redacted thinking makes the slice a lower bound on "needed".** The sensitivity rows (46–62%) bound
  this effect.
- **Sonnet is only 16 sessions per version, on 2 repositories.** Its CIs are wide.
- **urfave-cli exists only in v1 (30-turn cap).**
- **Sessions of the same task share a task.** Session bootstraps understate between-task variance.
  The arm comparisons are task-clustered.

## Reproduce

Run from `evals/lab/research/waste-anatomy/`, with `nice -n 10`, `OMP_NUM_THREADS=1`, about 6 CPU-minutes
in total:

```
python3 load.py      # -> /tmp/adage-research/waste-anatomy/sessions.jsonl.gz (22 MB; deleted after use, the shared disk is full)
python3 anatomy.py   # -> calls.pkl (15 MB): segments, provenance, lifetime $
python3 ceilings.py  # -> ceilings.json: cost structure, anatomy, oracle ceilings, pass/fail
python3 preload.py   # -> preload.json: file prediction, pre-load translation, instrument check
python3 trim.py      # -> trim.json: learned output budget, held-out and LORO
python3 arms.py      # -> arms.json: adage and agents-md vs control, orientation split
python3 extras.py    # -> extras.json: sensitivity, early-stop ceiling, $ per resolved
python3 stop.py      # -> stop.json: doom predictor (negative)
```
