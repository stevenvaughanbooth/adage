# Sequence models for skip-ahead delivery (slug `sequence-models`)

2026-09-29. Offline, $0. The code and results are in this directory. Caches are in
`/tmp/adage-research/sequence-models/` (14 MB).

## Question

Next-action prediction is low value. Top-1 accuracy was 0.18–0.32 in the lab, and the agent takes
that action anyway. The value would be in **skip-ahead**: predicting an action the agent will take
*eventually*, k steps later and after some waste, and delivering it early. Can a sequence model
trained on traces predict "the eventual useful action" precisely enough to be worth delivering?

## Verdict: no. Reject skip-ahead sequence models as a lever. The ceiling is small and the models don't reach it.

1. **The ceiling is small.** Assume perfect hindsight: any number of deliveries, p_follow = 1,
   and no harm or carry cost. A skip-ahead that removes only the waste *related to the target*
   saves **2.4 calls per session, or 6.0% of $**. That is 208 held-out control sessions of 31.8
   calls each, measured with the lab's waste instrument gated on relevance to the target. The
   learning-free measure "failed variants of the target" saves **0.5 calls (1.1%)**. That is
   below the North star's 15–20% before any model error. The loose measure counts every
   exploratory call that is related to the target (54% of $). Reaching it would need file
   localisation: knowing what this task's fix touches. A sequence of *other* tasks' actions does
   not carry that information, and the lab already showed that file directives rarely pay off.
2. **The models don't reach the ceiling. None beats "say nothing".**
   - I tried eight predictors: prior, repo prior, PPM order 1 and 3, PPM next-5, logistic, MLP,
     and a prefix tree of successful sessions.
   - I used three regimes: the in-repo split, leave-one-repo-out, and a learning curve up to 400
     sessions.
   - Every net saving is between −1.3 and +0.13 calls per session at the best threshold, and that
     threshold was chosen on the test set.
   - Precision on "eventual action after a related dead end" never reaches 0.5 on more than a
     handful of deliveries.
   - At the pre-declared operating point (calibrated p ≥ 0.5), every model delivers almost
     nothing.
   - The common harness agrees. On the Haiku held-out set (test set `6a795a362ccb`, 158 sessions
     over 38 tasks), the best variant saves **−0.004 calls per task [−0.057, +0.035], −0.0% of $
     [−0.3, +0.1]**. Its directives are "useful" (Sat reached) 90% of the time, yet they save
     nothing: they are correct and **redundant**. That is the next-action problem again, at
     longer range.
3. **More data doesn't help.**
   - The learning curve (leave-task-out, 25 → 100 → 400 training sessions from all repos and arms)
     is flat at 0: repo-prior −0.09, −0.01 and −0.02 calls per session. PPM and the MLP get
     *worse* with more data (−0.10 and −0.12 at n = 400).
   - Leave-one-repo-out is negative on 4 of 5 repos. The one positive is marshmallow, +0.5 to
     +0.7 calls per session (precision 0.52–0.57) from the prior and PPM-1. That is the
     `pip install … / pytest` pattern transferring from attrs and click: the same thing the U11
     instinct already covers.
4. **The structure the models discover is the structure we already hand-coded.** See the rules
   below: "install the package / test deps before running tests" (U11 and the lab's "install
   first" baseline) and "after editing module X, eventually run `tests/test_X`" (the replay's
   co-change `test_files` class). The rest is session phase ("eventually `git commit`"), which
   has no skip value.

**So what.** Shipped, a skip-ahead sequence-model reflex would change $ per task by **−0.0%
[−0.3, +0.1]** and calls per task by **−0.004 [−0.057, +0.035]**. That is the harness split,
Haiku, p_follow 1.0; at p_follow 0.5 it is also 0.0. Pass rate would not change (worst case
−0.0 points). Even a perfect predictor is capped at about 6% of $. I am confident, at about 90%,
that this survives a live A/B as "no effect". Don't build it.

## Method

- **Data** (`load.py`)
  - RepoBench v1 and v2, de-duplicated by session id: 47 training-arm sessions and 208 held-out
    control sessions (Haiku 4.5, plus Sonnet 5 on click and vue-core).
  - `adage` and `agents-md` arm sessions are used only as extra training data (LORO, curve,
    rules).
  - Each call is (tool name, its own argument text with the sandbox root stripped, the harness's
    `is_error`, the API turn). The sandbox-root strip is a benchmark artefact, not a feature.
  - Per-call $ uses the cumulative-context model: the call's share of its response's bill, plus
    its tokens re-read by every later response, rescaled to the session's true $.
- **Action clusters** (`space.py`). There are no hand categories.
  - Each call's text `"<tool> <args>"` is embedded with potion-base-8M, concatenated with a TF-IDF
    of argv tokens reduced by SVD to 64 dimensions, and clustered with k-means (K = 128) on
    training occurrences only.
  - Weighted intra-cluster cosine is 0.93. A held-out call goes to its nearest centroid.
- **Target** (`skip.labels`). At each decision point i (the prompt, then after each model turn)
  and for each cluster c, let t be the next occurrence of c.
  - **y = 1** iff t exists and the window [i, t) contains at least one failed call (`is_error`)
    that is *related* to the target step (cosine ≥ 0.35 in the learned space). This means "c is
    what the agent eventually does, after a dead end on the way to it".
  - A first version used "≥ 1 failed call of any kind". It rewarded far-off targets (the final
    commit) whose long windows collect unrelated failures. That was replaced; see the rules
    below.
- **Models** (`skip.py`)
  - `prior`: P(y | c, c already seen).
  - `repo-prior`: the same, per repository.
  - **PPM** / variable-order Markov over cluster contexts, orders 0..D, Witten–Bell blending,
    with the target's seen/unseen state in the context and an optional repo-prefixed context.
  - **PPM next-5** (next-k set prediction).
  - **Logistic** and a **1-hidden-layer MLP** (numpy, Adam, BCE, one sigmoid per cluster plus a
    learned "already seen" offset) over a decayed bag of recent clusters, the mean potion
    embedding of the last 5 calls, the prompt embedding, a recent failure rate, the step index
    and repo one-hot.
  - **Prefix tree of successful sessions**: follow the current collapsed cluster prefix as deep
    as it matches the passed training sessions, and score c by the share of matched sessions
    that took c later.
- **Calibration and delivery**
  - Isotonic regression is fitted on cross-fitted (by task) out-of-fold training scores of each
    point's top-2 candidates.
  - At each decision point, deliver the best not-yet-delivered cluster if its calibrated p ≥ θ.
  - The delivered text is the cluster's most frequent training member in this repo.
- **Outcome** (`skip.outcome`)
  - For each session, take the union of the windows' removed calls, times p_follow = 0.8.
  - Subtract carry: 80 tokens written once and re-read by every later response.
  - Subtract harm: a *wrong* delivery (the target is never taken) costs p_follow × 1 call.
  - Removed calls are measured five ways:
    - `fsim`: failed calls in the window with cosine ≥ 0.8 to the target (no hand-written parts);
    - `relfail`: failed calls with cosine ≥ 0.35;
    - `inst` (**primary**): the lab's hand-written waste instrument (`metrics.discovery`: failed,
      or env/manifest exploration), gated on cosine ≥ 0.35. It is a measurement instrument
      only;
    - `explore` (loose): any failed or exploratory call (the lab's taxonomy), gated on relevance;
    - `dist`: every call in the window. This is the question's "skip distance × p_follow", and it
      is a **loose upper bound, not a saving**. Its ceiling is 97% of $: it assumes a directive
      to "run the tests" at step 0 removes all the reading and editing before the tests.
- **Statistics.** Wilson 90% intervals for precision, and a 90% bootstrap over sessions for net
  calls and $ share. The harness re-score uses task-clustered 90% bootstraps.

## Results

### Ceiling (hindsight oracle, 208 held-out control sessions, 31.8 calls and $0.27 per session)

| measure of what a followed skip removes | unlimited deliveries | best 1 delivery | best 3 |
| --- | ---: | ---: | ---: |
| `fsim`: failed variants of the target (learning-free) | 0.54 calls / 1.1% $ | 0.41 / 0.8% | 0.54 / 1.1% |
| `relfail`: related failed calls | 2.20 / 5.3% | 1.75 / 4.1% | 2.19 / 5.3% |
| **`inst`: related waste (lab instrument)** | **2.43 / 6.0%** | 1.83 / 4.4% | 2.41 / 5.9% |
| `inst_any`: waste, not gated on relevance | 2.74 / 6.8% | 2.67 / 6.5% | 2.74 / 6.8% |
| `explore`: related exploration, including code reads (loose) | 15.6 / 54% | 9.7 / 35% | 14.0 / 50% |
| `dist`: skip distance (not a saving) | 30.6 / 97% | 28.9 / 91% | 30.5 / 96% |

Leave-one-repo-out ceilings (`inst`): attrs 7.4%, click 7.4%, marshmallow 8.3%, urfave-cli 6.6%
and vue-core 3.6% of $. These are consistent with the harness's class ceilings: every failed call
is 10.3%, and every search is 15.5%.

**A negative on the way: a purely learned waste measure fails.** I first learned "skippable"
from the order of actions alone: how much more often c′ occurs before the first c than after it.
It rated **23.6 of 31.8 calls per session skippable**. It confuses session *phase* (you read code
before you run tests) with *discovery*. Ordering statistics cannot separate the two. You need
failure or outcome information.

### In-repo split (train arm, 47 sessions → 208 held-out control; `results/split-K128.json`)

Instruments:

- next-cluster top-1: PPM-3 0.224, order-1 0.256 (the lab's 0.18–0.32);
- next-5 set: PPM-3 precision@5 0.253 and recall@5 0.368, against a frequency prior's 0.243 and
  0.353 (`results/nextk.json`).

Delivery (best θ chosen on the test set, which is optimistic; `pre` is the pre-declared
θ = 0.5):

| model | θ | deliveries/session | precision [Wilson 90%] | taken later | net calls, `inst` [90% CI] | net % $ `inst` [90% CI] | net `fsim` | net `explore` (loose) | skip dist × 0.8 |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| prior | 0.1 | 1.29 | 0.30 [0.25, 0.35] | 0.63 | +0.13 [−0.02, 0.28] | −0.1% [−0.6, 0.4] | −0.45 | +2.73 | 12.4 |
| repo-prior | 0.1 | 0.22 | 0.39 [0.28, 0.51] | 0.70 | +0.06 [0.01, 0.12] | 0.0% [−0.1, 0.2] | −0.06 | +0.29 | 2.5 |
| repo-prior | ≥0.2 | 0.08 | 0.44 | 0.88 | +0.02 | 0.0% | −0.01 | +0.09 | 1.1 |
| PPM-1 (repo) | 0.3 | 0.09 | 0.26 [0.13, 0.45] | 0.74 | +0.01 [−0.02, 0.04] | 0.0% | −0.02 | +0.05 | 0.6 |
| PPM-3 (repo) | 0.15 | 2.87 | 0.18 [0.15, 0.20] | 0.50 | −0.66 [−0.83, −0.48] | −2.2% [−2.8, −1.7] | −1.16 | +1.34 | 9.8 |
| PPM-3 (global) | 0.15 | 4.18 | 0.15 [0.13, 0.17] | 0.43 | −1.34 [−1.54, −1.14] | −4.2% [−4.8, −3.5] | −1.99 | +1.07 | 11.0 |
| PPM-3 next-5 (repo) | any | 0 | – | – | 0 | 0 | – | – | – |
| logistic | 0.15 | 0.26 | 0.15 [0.09, 0.24] | 0.41 | −0.07 [−0.13, −0.01] | −0.2% | −0.12 | +0.03 | 1.1 |
| MLP | pre 0.5 | 0.26 | 0.20 [0.13, 0.31] | 0.50 | −0.03 [−0.10, 0.03] | −0.1% [−0.3, 0.0] | −0.09 | +0.10 | 1.2 |
| prefix tree (successful sessions) | any | 0 | – | – | 0 | 0 | – | – | – |

- The prefix tree and next-5 PPM never reach a calibrated p of 0.05 for the eventual-useful
  target. What they predict is the obvious next step, which is almost never a useful skip.
- **The frontier is flat.** Raising θ lifts precision only to about 0.4 (repo-prior) while
  deliveries fall to under 0.1 per session. No operating point has both precision ≥ 0.7 and a
  positive net saving under any measure except the loose `explore`.
- Harm estimate: 30–60% of deliveries are never taken. Each such delivery costs about 0.8 calls
  when followed. That is why every higher-volume model is net negative.

### Leave-one-repo-out (train = every session of the other four repos, 394–496 sessions; `results/loro-K128.json`)

Best θ per model, net calls per session on `inst`:

| held-out repo (sessions) | prior | PPM-1 | PPM-3 | logistic | MLP | prefix tree |
|---|---:|---:|---:|---:|---:|---:|
| attrs (49) | −0.77 (p 0.20) | −1.67 (0.24) | −5.48 | −2.09 | +0.06 (0.08/session) | 0 |
| click (63) | −0.82 (0.23) | −3.44 | −2.07 | −0.03 | −1.32 | 0 |
| marshmallow (30) | **+0.54** (0.52) | **+0.71** (0.57) | −0.04 | −1.21 | +0.03 | −1.77 |
| urfave-cli (16) | −0.85 (0.00) | −1.73 (0.00) | −3.31 | −0.26 | −1.08 | 0 |
| vue-core (50) | −1.22 (0.08) | −2.42 (0.04) | −9.83 | −0.07 | −0.03 | 0 |

Python→Python transfer carries the env-setup pattern, and nothing transfers to Go or TypeScript:
urfave-cli precision is 0.00.

### Learning curve (leave-task-out, 5 task folds, training sampled from other tasks of every repo and arm; `results/curve-K128.json`)

| training sessions | repo-prior net calls [90% CI] (precision) | PPM-1 (repo) | MLP |
|---:|---:|---:|---:|
| 25 | −0.09 [−0.15, −0.03] (0.13) | −0.29 [−0.40, −0.17] | −1.07 [−1.25, −0.90] |
| 100 | −0.01 [−0.09, 0.07] (0.35) | 0.00 (nothing delivered) | −0.01 [−0.03, 0.00] |
| 400 | −0.02 [−0.06, 0.02] (0.29) | −0.10 [−0.16, −0.04] | −0.12 [−0.18, −0.07] |

The curve is flat: more history does not make "eventual" actions predictable.

### Common harness (`harness_policy.py`, `results/harness.json`)

- Directives: a Bash member of the cluster becomes `run`, and a file member becomes
  `read_files`.
- The Sat/W classes are the harness's own hand-written instruments. `read_files` and "other
  command" are uncalibrated.
- Haiku held-out, test set `6a795a362ccb` (158 sessions, 38 tasks, baseline $0.260 and 32.3
  calls per task):

| policy | driver | $ saved/task [90% CI] | share of $ | calls saved/task [90% CI] | directives | useful |
|---|---|---:|---:|---:|---:|---:|
| repo-prior θ 0.15 | split, p_follow 1.0 | −$0.0001 [−0.0007, +0.0003] | −0.0% [−0.3, +0.1] | −0.004 [−0.057, +0.035] | 31 | 0.90 |
| repo-prior θ 0.15 | split, p_follow 0.5 | −$0.0001 [−0.0004, +0.0001] | −0.0% | −0.004 [−0.030, +0.016] | 31 | 0.90 |
| repo-prior θ 0.15, and θ 0.5 of repo-prior, PPM-1 and MLP | loro, and split at θ 0.5 | 0 | 0 | 0 (nothing delivered) | 0 | – |
| U11 predictive (harness baseline, same drivers) | split / loro | | +3.5% / +1.9% | +1.63 / +0.88 | | |

## Discovered skip-ahead rules (`rules.py`, `results/rules.json`)

Rule a ⇒ b means that at a point whose last call is in cluster a, the agent eventually takes b
after a related dead end. Rules are fitted on the 380 non-control sessions and checked on the
held-out control sessions (Wilson 90%).

| rule (clusters shown by their members) | held-out precision | inst waste in window | already hand-coded? |
|---|---:|---:|---|
| exploring with `find . -name "*.py"` ⇒ `pip install -e .` / `pip install pytest` (first, any-failure target) | 0.68 [0.59, 0.76] | 1.35 | **yes**: U11 `.venv`/install instinct and the lab's "install first" baseline |
| Edit `src/attr/validators.py` ⇒ `python -m pytest tests/test_validators.py` | 0.46 [0.34, 0.58] | 3.1 | **yes**: the replay's co-change `test_files` class ("after editing X, run tests of X") |
| Edit `src/click/termui.py` ⇒ `pytest tests/test_termui.py` | 0.32 [0.23, 0.42] | 4.1 | same as above |
| Read `flag_*.go` ⇒ `go test -v -run …` (urfave) | 0.28 [0.16, 0.44] | 1.7 | the lab's "how to run the tests here" reflex |
| Read `src/marshmallow/validate.py` ⇒ `git add … && git commit` | 0.40 [0.24, 0.58] | 3.3 | no, and **no skip value**: session phase, not discovery |
| Edit `src/attr/validators.py` ⇒ `pip install -e ".[dev]" && python -m pytest …` | 0.30 [0.21, 0.42] | 1.6 | yes (U11 family) |

- **Yes, the model finds "after editing X, eventually run the tests of X" on its own.** The
  cluster pairs line up module with test file (validate ↔ test_validate, termui ↔ test_termui).
  So do the env-setup-before-tests rule and the per-repo test command.
- **All three are already hand-coded.** None is precise enough (≤ 0.7) to deliver as a skip.
- The first row comes from the first label variant ("any failed call in the window"). With the
  current label, which requires a related failure, that rule's precision is lower. It is quoted
  from that run's log.
- Everything else in the top 25 is session phase (`git status`, `git log`, `git commit`).

## Hard-coded parts (honest list)

- **Measurement only, never a model input or a training label:**
  - the `inst` and `explore` instruments: `metrics.DISCOVERY` regex plus the `normalize`
    exploratory taxonomy;
  - the harness Sat/W classes.
- **Delivery format:** a Bash member becomes `run`, and a Read/Edit/Write member becomes
  `read_files`. This keys on tool names, only to choose the directive kind.
- **Data preparation:** `normalize.strip_root` (the benchmark's sandbox path) and the argument
  fields chosen per tool call (`command`, or the string-valued inputs).
- **Fixed constants:** relevance cosine 0.35 and variant cosine 0.8 (learned space), p_follow
  0.8, 80-token directives, and 1-call harm per wrong delivery.

## Build recommendation

- **Don't build a skip-ahead sequence model in Rust.** Its expected value is 0 calls and 0% $
  [−0.3, +0.1], and it is capped at about 6% of $ even with perfect hindsight.
- **What to take from this:**
  1. "Eventual action" prediction is dominated by session phase. Any learned "useful later"
     target must be gated on a failure or outcome that is *related* to the target, or it rewards
     `git commit`.
  2. The only structures the models recover (env setup before tests; the module ↔ test-file
     link; the per-repo test command) are what U11, the learned test-runner reflex and the
     co-change oracle already deliver. The existing hand-coded parts could be replaced by this
     learned equivalent (potion + argv clusters, PPM-1 with a repo context), but that is a
     principle-1 clean-up, not a savings lever.
- The large pool in this area is the loose `explore` ceiling (about 50% of $). It is file
  localisation: knowing which files this task needs. That is a retrieval-from-the-prompt problem,
  not a sequence problem. It belongs with levers that shorten the productive path or shrink
  carried context.

## Reproduce

```
cd evals/lab/research/sequence-models
OMP_NUM_THREADS=1 nice -n 10 python3 load.py            # cache sessions
OMP_NUM_THREADS=1 nice -n 10 python3 run.py split       # ~5 min
OMP_NUM_THREADS=1 nice -n 10 python3 run.py loro --models prior,ppm1,ppm3,ppm3-next5,logistic,mlp,prefix-tree
OMP_NUM_THREADS=1 nice -n 10 python3 run.py curve --models repo-prior,ppm1-repo,mlp --sizes 25,100,400
python3 nextk.py; python3 rules.py; python3 harness_policy.py; python3 report.py
```
