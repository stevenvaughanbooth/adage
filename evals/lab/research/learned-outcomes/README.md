# Learned outcomes: success, failure and dead ends from tool outputs, with no hand-written lists

2026-09-29. Slug `learned-outcomes`. Offline, $0. The research sprint brief is in `../BRIEF.md`.

## Verdict

**The primitive works. The lever does not.**

- **Detection (the primitive).** A failure detector learned with no hand-written strings is
  better than the hand-coded signatures.
  - It uses Drain log templates over each output. A logistic regression over hashed templates is
    taught by the harness's own exit status (`is_error`), on sessions outside the test set.
  - On 277 hand-labelled held-out outputs, failure AUC is **0.961 [90% CI 0.943, 0.978]**. The
    hand-coded rule (`is_error` or an env signature) scores 0.917 [0.888, 0.946], and `is_error`
    alone scores 0.905.
  - **Leave-one-repo-out**, it is **0.951 [0.930, 0.970]**, so it transfers to Go and TypeScript.
  - On the failures the exit status hides (piped `… 2>&1 | tail`), it scores 0.85–0.90 AUC
    against 0.57 for the hand-coded rule.
  - Success is learned too. The last attempt of a retry chain predicts the hand label at 0.825 AUC,
    against 0.761 for the hand-coded rule (tests executed, then not failed).
- **Savings (the lever).** Turning the detector into dead-end reflexes saves little and can lose.
  - The reflexes save **+0.18 calls per task [−0.07, +0.42], or +0.2% of $ [−0.3, +0.8]**, on 158
    Haiku held-out sessions (split protocol).
  - No configuration, protocol or mode beats 0.5 calls or 0.9% of $.
  - U11 on the same test set saves +0.88 calls post-hoc and +1.63 predictive (+3.5% of $).
  - Adding the learned reflexes to U11 **lowers** it, from 1.63 to 1.10 calls per task. Their
    extra directives misfire (useful 0.57–0.77, against U11's 0.95), and the dead ends they catch
    are mostly U11's.
- **Credit assignment from session outcome fails.** TD(λ) and Monte-Carlo value estimation over
  trace states, rewarded by the hidden-test result, is **at chance**: failure AUC 0.51 and 0.48.
  Hidden-test pass/fail says nothing about which individual call was waste.
- **The ceiling is small.** Deleting every failed call is worth 9.6% of $ [8.3, 10.9] with the
  hand-coded definition. With the learned definition it is 13.1% [11.1, 15.4]. Both are below the
  15–20% target, and they assume perfect follow and zero cost.

**So what.** Shipped as reflexes, the expected effect is about +0.2 calls and +0.2% of $ per task
(90% CI −0.3% to +0.8%), with no pass-rate effect. Confidence that it survives a live A/B as a
positive effect is low. Shipped as a labeller, it is a better *instrument* for other learners.
It has no direct $ effect, and it should replace the hand-written signature lists only where
Adage labels outcomes. It is not a 15–20% lever. Nothing built on "skip failed calls" can be.

## Question and data

The question: can success, failure and dead ends be learned from tool outputs, replacing the
hand-coded signatures? Those signatures are `No module named`, `N passed`, `command not found`
and the rest in `adage-learn/src/outcome.rs` (`executed_tests`, `env_signature`) and in the
replay's `ENV_SIG`/`EXECUTED`.

**Corpus.** RepoBench v1 and v2 give 519 sessions, de-duplicated by session id (15,824 tool
calls). Only the transcript that `state.json` names is kept (`load.py`).

- **Model training pool:** every non-control session, which is 329 sessions and 9,806 calls
  (train, adage and agents-md arms). For leave-one-repo-out (LORO), the pool excludes every
  session of the held-out repository.
- **Hand labels:** held-out control-arm sessions only.
- **Downstream:** the common harness, `corpus.held_out(model="haiku")` on v1+v2 (158 sessions,
  38 tasks, test set `6a795a362ccb`). The data-expansion corpus is excluded, so the set is pinned.
  The Rust-parity check uses the 82 v2 sessions.

## The measurement instrument: 277 hand labels

`sample.py` draws 40 calls per slice across 7 slices (5 Haiku repositories, 2 Sonnet 5 slices),
from 133 held-out control sessions. The draw is stratified on the harness flag `is_error`, so
there are enough failures. AUC does not depend on the class mix.

**Labeller.** This agent read each output (`labels.txt`, `labels_todo.jsonl`). This is a known
limitation: one labeller, and no inter-rater check.

**Codes (counts: S 76, I 115, E 74, T 12):**

| code | meaning |
| --- | --- |
| **S** (success) | the call did its job: tests ran and passed, an edit applied, an install worked, a commit |
| **I** (informational) | reads, searches, listings, diffs, probes, and empty greps |
| **E** (environment or tooling dead end) | not found, missing module, bad tool input, "no tests to run", compile error in the agent's own scratch code, collection error |
| **T** (task failure) | it ran and reported a failure of the code under test: failing tests, an assertion in a repro |

Failure means E or T. Dead end means E.

## Methods (`features.py`, `models.py`, `value.py`, `eval2.py`)

Nothing here reads a hand-written string. The only inputs are:

- the exit status the agent saw;
- the tool name, as an opaque categorical;
- the action and output text through potion-8M, or through Drain templates;
- session order;
- the hidden-test outcome.

| method | what it learns from |
| --- | --- |
| **Drain → exit** | Drain3 (the stock algorithm, **no masking regexes**, sim 0.4, depth 4) mines line templates from the first and last 6 lines of every output in the pool. It gives 13.6k templates. They are hashed into first-line, last-line and any-line buckets, and a logistic regression is fitted with the exit status as the teacher. |
| potion → exit | potion-8M embedding of the output and of the action, plus tool and length, through a logistic regression with the same teacher |
| retry-excess (self-supervised) | whether a similar action (cos ≥ 0.9, same tool) follows within 3 calls. Score = logit P(retry \| output, action) − logit P(retry \| action): the part of the retry odds that the output explains. |
| k-means cluster | 64 clusters of output embeddings. A cluster's score is its pool exit-failure rate plus its retry excess. |
| **TD(λ)** / MC advantage | State = (action cluster of 16, output cluster of 32), tabular with 512 states. Reward is −1/30 per call and +1 if the hidden tests passed. λ = 0.8. Score = −(−c + V(s_i) − V(s_{i−1})). |
| success: recover | self-supervised target: the last attempt of a retry chain, after which the agent moved on |
| clean teacher (`eval2.py`) | Drops piped Bash calls from the teacher's training set. A pipeline's exit status is its last stage's, so for those calls `is_error` doesn't label the output. |

## Results: the instrument

Held out: trained on non-control sessions and scored on 277 labels from 133 control sessions.
The 90% CIs come from a bootstrap over sessions (`results/eval.json`, `results/eval2.json`).

| score | failure AUC (E+T) | dead-end AUC (E) | LORO failure AUC (pooled) | AUC on failures with `is_error` false (n=15) |
| --- | --- | --- | --- | --- |
| B0 `is_error` (harness) | 0.905 [0.874, 0.937] | 0.908 | 0.905 | 0.50 |
| B1 hand-coded (`is_error` or env signature) | 0.917 [0.888, 0.946] | 0.921 | 0.917 | 0.57 |
| **Drain → exit** | **0.961 [0.943, 0.978]** | 0.947 | **0.951 [0.930, 0.970]** | **0.83** |
| Drain → exit, clean teacher | 0.964 [0.947, 0.981] | – | 0.954 [0.934, 0.972] | 0.85 |
| potion → exit | 0.946 [0.921, 0.971] | 0.952 | 0.898 [0.863, 0.934] | 0.72 |
| potion → exit, clean teacher | 0.977 [0.964, 0.989] | – | 0.927 [0.900, 0.954] | 0.90 |
| Drain + potion → exit | 0.938 [0.913, 0.964] | 0.931 | 0.932 | 0.68 |
| retry-excess (self-supervised) | 0.811 [0.764, 0.862] | 0.828 | 0.790 | 0.70 |
| k-means cluster | 0.861 [0.812, 0.909] | 0.879 | – | 0.73 |
| combined (Drain+potion and retry) | 0.946 [0.916, 0.972] | 0.958 | 0.922 | 0.74 |
| **TD(λ) advantage** | **0.506 [0.443, 0.572]** | 0.510 | 0.509 | 0.49 |
| MC advantage | 0.482 [0.410, 0.557] | 0.479 | – | 0.40 |

**Per-repository LORO** (Drain → exit against hand-coded):

| repository | Drain → exit | hand-coded |
| --- | --- | --- |
| attrs | 1.00 | 1.00 |
| click | 0.93 | 0.89 |
| marshmallow | 1.00 | 1.00 |
| urfave-cli (Go) | 0.95 | 0.84 |
| vue-core (TypeScript) | 0.92 | 0.92 |

The learned detector is never worse, and it is better on the two repositories where the
hand-written list is incomplete for the language.

**Success, the 3-class result and F1:**

- **Success AUC.** Learned (recover × (1 − failure)) scores 0.825 [0.786, 0.861]. Hand-coded
  (tests executed > ok > failed) scores 0.761 [0.724, 0.796].
- **3-class macro F1**, at a pool-derived threshold with no label tuning: learned 0.70, hand-coded
  0.73. The learned model separates success from informational no better than the rule.
- **Failure F1 at the teacher's own 0.5:**

  | detector | failure F1 |
  | --- | --- |
  | Drain → exit | 0.88 |
  | potion → exit, clean teacher | 0.92 |
  | hand-coded | 0.90 |
  | `is_error` | 0.89 |

**Caveats on the instrument:**

- **Part of the text model's edge is the exit status in text form.** The top failure template is
  `Exit code <*>`, a line Claude Code writes into failed Bash outputs. The fair test of "beyond
  the flag" is the `is_error = false` column, and there are only 15 such failures in the labels.
- **The detector finds hidden dead ends only partly.** It ranks them above normal outputs, but at
  0.5 it flags only 12 of the 47 piped U11 dead ends in the control arm (clean teacher). With the
  unclean teacher it flags 0, because the pipe labels those outputs "success".
  (`results/deadend_recall.json`, `eval2.json`.)
- **Other learned "failure" templates:**
  - `<tool_use_error>String to replace not found`;
  - `InputValidationError`;
  - `/usr/local/bin/python3: No module named pytest`;
  - `EISDIR`;
  - `Errors 1 error`.
- **Learned "success" templates:** `The file <*> has been updated successfully`,
  `File created successfully`, `Found <*> files`, and source-code lines.
- **Hash collisions.** With 2,048 buckets per slot, some templates share a weight. A shipped
  version should key the exact template id.

## Results: downstream, in $ and calls (the target metrics)

**Learned reflexes** (`policy.py`, a harness learner) are built in four steps:

1. The outcome model above (potion → exit, clean teacher) marks a failure.
2. The output falls in a k-means situation cluster.
3. The reflex is what the agent did in training to get out of that dead end: the first later
   call of the same intent (cos ≥ 0.8) that the model scores a success.
4. The reflex is kept at ≥ 2 supporting sessions and a recover rate of ≥ 0.5 or ≥ 0.7. Both
   thresholds were declared before scoring.

The reflex is delivered post-hoc (PostToolUse, which reads the output, so piped failures count)
or predictive (PreToolUse, when the action resembles a training trigger; no hindsight). Scoring
uses the common harness on 158 Haiku held-out control sessions (38 tasks, test set `6a795a362ccb`).
The baseline is $0.260 and 32.3 calls per task, with 90% task-clustered CIs (`results/harness.json`,
`results/extras.json`).

| policy | protocol | calls saved/task [90% CI] | $ saved/task [90% CI] | share of $ | directives | useful (LB90) |
| --- | --- | --- | --- | --- | ---: | --- |
| U11 post-hoc (hand-written) | – | +0.88 [+0.58, +1.19] | +$0.0049 [0.0033, 0.0065] | +1.9% | 62 | 0.98 (0.93) |
| **U11 predictive (hand-written)** | split | **+1.63 [+1.22, +2.04]** | +$0.0091 [0.0068, 0.0114] | **+3.5%** | 98 | 0.95 (0.90) |
| learned post-hoc, recover ≥ 0.5 | split | +0.18 [−0.07, +0.42] | +$0.0006 [−0.0008, 0.0020] | +0.2% | 92 | 0.60 (0.51) |
| learned predictive, recover ≥ 0.5 | split | +0.25 [−0.01, +0.50] | +$0.0009 [−0.0006, 0.0023] | +0.3% | 94 | 0.60 (0.51) |
| learned post-hoc, recover ≥ 0.7 | split | −0.00 [−0.18, +0.17] | −$0.0004 | −0.1% | 73 | 0.58 (0.48) |
| learned post-hoc, recover ≥ 0.5 | LORO | −0.00 [−0.46, +0.43] | −$0.0023 [−0.0054, 0.0005] | −0.9% | 317 | 0.69 (0.65) |
| learned post-hoc, recover ≥ 0.7 | LORO | +0.42 [+0.07, +0.77] | +$0.0011 [−0.0012, 0.0033] | +0.4% | 223 | 0.73 (0.67) |
| learned predictive, recover ≥ 0.7 | LORO | +0.43 [+0.02, +0.83] | +$0.0009 [−0.0016, 0.0033] | +0.3% | 247 | 0.69 (0.64) |
| … post-hoc LORO ≥ 0.7 at p_follow 0.5 | LORO | +0.20 [+0.02, +0.37] | +$0.0003 | +0.1% | | |
| U11 predictive + learned post-hoc | split | +1.10 [+0.69, +1.49] | +$0.0058 [0.0035, 0.0081] | +2.2% | 146 | 0.74 (0.68) |
| *first configuration: unclean teacher, recover ≥ 0.5* | split | +0.48 [+0.26, +0.71] | +$0.0023 [0.0010, 0.0035] | +0.9% | 140 | 0.77 (0.71) |
| *same* | LORO | −0.25 [−0.59, +0.10] | −$0.0040 | −1.5% | 237 | 0.61 (0.55) |

The learned reflexes use the "command" class unless the response is a test runner, which is
uncalibrated. The harness flags this.

Cross-check: `replay.py` is a port of `savings_replay::score` that re-scores the Rust replay's own
U11 directives with **0 of 98 sessions differing** (`validate`). On it, the learned reflexes
score as follows (Rust Ŝ, no misfire charge, 82 v2 Haiku sessions; `results/downstream.json`):

| policy | Ŝ [95% CI] |
| --- | --- |
| U11 post-hoc | +1.00 |
| U11 pre-emptive (hindsight) | +2.11 |
| learned, own training sessions, post-hoc | +0.36 [0.09, 0.69] |
| learned, own training sessions, pre-emptive | +0.50 [0.13, 0.92] |
| learned, other repositories, post-hoc | +0.59 [0.20, 1.08] |
| learned, other repositories, pre-emptive | +0.79 [0.28, 1.38] |
| U11 pre-emptive + learned | +2.11, unchanged: every learned dead end is inside U11's windows |

**Why the lever fails** (from `by_route` and the mined reflexes):

1. **Most of the learned value is U11, rediscovered.**
   - From click's own training sessions the learner finds `.venv/bin/python -m pytest …`. From
     the other repositories it finds `.venv/bin/python -m pytest …` with 111 supporting sessions
     and `pip install simplejson && python -m pytest …`.
   - This is the right *behaviour*. It is also exactly what U11 already delivers, so the union
     adds nothing.
2. **Everything else misfires.** Other "ways out" are task-specific:
   - a repro heredoc (`PYTHONPATH=src python3 << EOF …`);
   - `python -m pytest tests/test_options.py -k …`;
   - another repository's test path.

   They carry concrete arguments from the training task, and their Sat rarely happens, so each
   is charged the 2-call misfire prior. Useful rates are 0.57–0.77 against U11's 0.95.
3. **Training data is thin.** Seven to ten training sessions per repository give 0–3 reflexes.
   Pooling across repositories gives many, and precision falls.
4. **Better detection doesn't fix a wrong response.**
   - Swapping the teacher changed the split result from +0.48 to +0.18 calls: the same order, and
     noise.
   - The limit is the response, not the trigger. Across all TD, MC and retry variants, knowing
     that a call failed doesn't say what to do instead.

## Ceiling (harness oracle, cumulative-context model, 158 sessions)

| class deleted | calls/task [90% CI] | share of calls | share of $ [90% CI] |
| --- | --- | ---: | --- |
| hand-coded failed (`is_error` or env signature) | 3.11 [2.73, 3.53] | 9.6% | 9.6% [8.3, 10.9] |
| hand-coded environment friction (U11's W) | 3.25 [2.68, 3.81] | 10.0% | 7.9% [6.4, 9.6] |
| **learned failure** (p > 0.5, or `is_error`) | 4.11 [3.48, 4.79] | 12.7% | **13.1% [11.1, 15.4]** |
| learned dead-end windows (a failure through to the next same-intent success) | 6.09 [5.19, 7.01] | 18.8% | 19.3% [16.4, 22.6] |

The windows row is a loose bound. It includes the productive edits between a failing and a
passing test run.

Any lever of the kind "avoid the dead end" is capped near 10–13% of $, with perfect follow and
zero cost. U11 predictive already takes 3.5%.

## How to ship it (only as a labeller)

This comes from `ship.py` and `results/ship.json`. The dictionary is fitted on 9,806 pool calls,
with 13,621 Drain templates.

- **Artefact.**
  - The Drain prefix tree is 1.3 MB pickled.
  - A logistic regression with about 4,900 non-zero weights over hashed templates, or about 5,400
    with potion features.
  - An optional potion-8M, which Adage already pins.
- **Latency per tool output**, end to end from raw text to a probability, single-threaded Python,
  on a shared 4-vCPU box at load ~30:
  - template only: median 12 ms, p95 75 ms;
  - with potion: median 16 ms, p95 85 ms.

  A Rust port of Drain matching (a tree walk plus token compare) and a dot product should be well
  under 1 ms, but that is not measured.
- **Build recommendation.**
  - In `adage-learn` (not in this sprint), replace `env_signature` / `executed_tests` for
    *labelling traces at learn time* with the template dictionary and classifier, taught by exit
    status on non-piped calls.
  - Keep exact template ids rather than hashes.
  - Expected value: better outcome labels for any learner (AUC 0.96 against 0.92, and about 0.85
    against 0.57 on piped failures), with **no direct $**.
  - **Don't** build learned dead-end reflexes.
  - Ship the U11 predictive `PreToolUse` rewrite first: +1.63 calls and +3.5% of $.

## Hard-coded parts (honest list)

- **Teacher:** the harness `is_error` flag. This is data, not a rule.
- **Clean teacher:** one shell-grammar regex for an unquoted pipe. This is not an error-string
  list.
- **Tool name**, as an opaque categorical.
- **Constants:**
  - retry window 3;
  - "same action" cos ≥ 0.9 and "same intent" cos ≥ 0.8;
  - horizon 15;
  - k = 64 clusters (TD: 16 × 32);
  - reflex support ≥ 2 and recover ≥ 0.5 / 0.7 (both reported);
  - θ = 0.5;
  - TD c = 1/30, λ = 0.8.
- **Measurement only**, never read by a learner:
  - the 277 hand labels;
  - `instrument.py`, a port of the replay's ENV_SIG / EXECUTED / ENV_PROBE / `invocation`, used
    for the hand-coded baseline and Sat/W;
  - the harness's `features` for Sat/W.

## Reproduce

Run from this directory, with `OMP_NUM_THREADS=1 nice -n 10`. Caches go to
`/tmp/adage-research/learned-outcomes` (37 MB). Drain3 is vendored there from the PyPI sdist,
because its wheel build fails here.

```sh
python3 load.py          # calls with full outputs
python3 features.py      # potion embeddings, self-supervised targets
python3 sample.py        # the label draw (labels.txt holds the labels)
python3 evaluate.py      # instrument table (Drain fits: ~10 min per fold under load)
python3 eval2.py         # clean teacher, dead-end recall
python3 downstream.py    # Rust-parity replay port (needs the replay manifests + rust-*.json in the cache)
python3 run_harness.py   # $ / calls in the common harness
python3 extras.py        # U11 + learned, oracle ceilings
python3 ship.py          # dictionary size and latency
python3 make_result.py   # result.json
```

The downstream port's inputs are:

- `python3 evals/replay/manifest.py /tmp/adage-rb2/runs/<slice> --out /tmp/adage-research/learned-outcomes/replay`;
- `target/debug/adage eval replay-savings … --only U11 --mode post-hoc|pre-emptive --json`, saved
  as `rust-<slice>-<mode>.json`.
