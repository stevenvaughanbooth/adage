# Hindsight shortcuts: relabel each trace with what an oracle could have said, and when

2026-09-29. Slug `hindsight-shortcuts`. Offline, $0. Python in this directory. Nothing in `crates/` is touched.

## Question

For every training trace, find the **achieving actions** (the command that finally worked, the file
that turned out to matter, the fact the fix depended on) and the **earliest point** at which an
oracle could have told the agent. The calls in between are waste that an oracle could remove.
Then learn a mapping from situation to shortcut that generalises, so that a held-out session in a
similar situation gets the shortcut delivered.

## Answer in brief: a negative result for this lever, with a precise reason

- **Hindsight waste is large.** Relabelled by data flow, 46% of held-out calls (48% of $) are
  non-achieving. A tight, per-kind oracle that names the achieving thing at its earliest point
  would remove **7.3 calls per session, 22.6% of calls and 24.9% of $** [21.7, 27.9]. That meets
  the North star's 15–20% bar.
- **Almost none of it recurs across tasks.** A **perfect** situation matcher, restricted to shortcuts
  seen in ≥ 2 training sessions of the repository, removes only **1.6 calls, 5.4% of $** [3.1, 7.8]
  with the 7–10 training traces a repository has. With 60–110 sessions of memory per repository it
  removes **3.6 calls, 12.1% of $** [9.2, 15.0].
  - Almost all of what recurs is command and procedure knowledge: `pip install -e .` before running
    anything, how to invoke the tests, and "`core.py` is too large to Read whole: use offset".
  - Files transfer a little: 0.15 calls with the training memory, 0.6 with grown memory.
  - Facts never transfer: 0.00.
- **The learned matcher recovers a fraction of that, at low precision.** kNN over potion-8M situation
  embeddings plus structure, on my data-flow instrument:
  - +1.09 calls per session [0.45, 1.94], 3.3% of $, with harmless misfires;
  - precision 0.15, far below the 0.8 gate;
  - with a 2-call misfire charge it is **+0.22 calls** (tuned for that charge) or negative.
- **On the common harness** (`evals/lab/research/harness`, test set `6a795a362ccb`, 158 Haiku held-out
  control sessions, 38 tasks, misfire 2 calls), the learner saves **+0.15 calls per task
  [0.01, 0.31], +$0.0010 [−0.0001, +0.0021], +0.4% of $ [−0.0, +0.8]**.
  - Leave-one-repo-out: 0.
  - U11 + shortcuts ≈ U11 alone: +0.97 vs +1.00 calls on the 82 v2 sessions.
- **So what.** Shipped as a learned situation→shortcut reflex, the expected effect is about
  **+0.15 calls and +0.4% of $ per task** (90% CI up to +0.8%), with no pass-rate effect
  (worst case −0.0 points).
  - I am confident (>90%) that a live A/B would not show ≥ 2% of $. The ceiling analysis shows why:
    the waste that shortcuts remove is **task-specific**, and the part that recurs is the part U11
    and the lab's "most useful command" already cover.
  - Revisit only once a repository has hundreds of sessions. The perfect-matcher ceiling doubles from
    10 to ~80 sessions, but the matcher's precision falls as it grows.

## Method

All code is in this directory. The inputs are RepoBench v1+v2 transcripts, de-duplicated by
session id and picked by the run's `state.json` session id (`load.py`). The harness adapter uses the
harness's own loader.

1. **Hindsight labeller** (`hindsight.py`). It is fully data-derived, with no tool-name, error-string
   or command lists.
   - **Tokens**: identifier or path tokens of each call's input and output, weighted by IDF over
     training sessions. Prompt tokens never count as flow.
   - **Locations**: tokens with a `/`, or names of tracked files in the repository's own checkout
     (`git ls-files`).
   - **Data flow**: output(k) → input(j) when the output's tokens reappear in the input (IDF mass ≥ 3).
   - **Writer tools** are learned from input/output size: median ratio ≥ 5, which selects Edit and
     Write.
   - **Retry chains**: same tool, similar inputs.
     - An attempt *failed* if its output fed nothing outside the chain and either the harness flagged
       it or its output differs from the final attempt's.
     - The call before each retry is its *repair*.
   - **Achieving calls**: writes that persist (they name a file in the final diff, or a path used
     again later), the command that finally worked, commands later repeated with the same result,
     repairs, and the backward data-flow closure of all of these.
   - **Shortcuts**, each with an earliest point t0 and a kind-specific removable set D:
     - **command / procedure**: D = failed attempts plus the waste between them; t0 = the first
       attempt. The repairs form a "first run X" procedure.
     - **file / fact**: the new location or identifier an achieving call got by data flow. D = the
       "finder" calls that fed only it, plus the waste from the first finder to the use.
     - **action**: an achieving call's own near-miss predecessors.
   - An example (attrs, held-out):
     - `python -m pytest` fails, `pip install pytest`, fails again (conftest import), 2 probes,
       `pip install hypothesis`, then it works. This is a procedure shortcut with t0 = the first
       attempt, D = 4 calls, content "first `pip install pytest`, `pip install hypothesis`".
     - `python3 <<heredoc` fails with ModuleNotFound, then `ls`, `ls | grep pyproject`,
       `pip install -e .`, then it works. This is a procedure "first `pip install -e .`", D = 3.
2. **Learned equivalence** (`shortcuts.py`). Shortcuts from different sessions are "the same" when:
   - commands: same tool, and the IDF-weighted Jaccard ≥ 0.6 of their *stable* tokens (seen in ≥ 2
     memory sessions), with an informative skeleton (IDF mass ≥ 1, so `Read file_path offset` alone
     never matches);
   - files: they share a path;
   - facts: they share a rare token.

   Greedy clustering gives a library of classes. A class is deliverable when its support is ≥ 2
   sessions of other tasks.
3. **Situations and matcher** (`matcher.py`).
   - There is a decision point before every tool batch. Its situation is the embedded prompt
     (potion-base-8M, the model Adage ships), the embedded recent history (the last 5 inputs, flags,
     and the first line of the last 2 outputs), and structure (log step, flagged share, whether a
     writer has run, the last tool).
   - Similarity = 0.3·cos(prompt) + 0.5·cos(history) + 0.2·structure.
   - Each memory decision point is labelled with the classes that would have saved calls *there*
     (the hindsight target).
   - Score(class | situation) = similarity²-weighted share of the 20 nearest memory points from other
     tasks. The best class is delivered once per session when score ≥ θ.
   - θ is tuned by leave-one-task-out on memory to maximise net calls − λ·misfires, with λ = 0, 1 or 2
     (the assumed misfire cost in calls).
4. **Scoring, two instruments.**
   - **Mine** (`experiment.py`) uses the held-out session's own hindsight labels. A delivery at i is
     useful if a matching instance has t\* ≥ i and D ∩ [i, t\*) ≠ ∅, and saves |∪ D ∩ window|. It is
     redundant if the agent took the matching action with no waste, and wrong otherwise.
   - $ and tokens use the cumulative-context cost model (`costs.py`), net of the delivered text.
     Its 5-minute cache-write rate prices sessions at 0.89× billed, so the shares of $ are the robust
     numbers.
   - **The common harness** (`harness_eval.py`) uses the sprint's hand-written Sat/W instrument:
     - `run` for command classes, `read_files` for file classes, `advice` for facts;
     - p_follow 1 (0.5 as sensitivity), misfire 2 calls, 1-hour cache-write pricing.
   - CIs: 90% bootstrap over tasks (clusters) and Wilson for precision.
   - Held-out sets are Haiku 4.5 control-arm sessions only. The adage arm and the Sonnet 5 slices are
     reported separately.

## Results

Held-out: 158 Haiku 4.5 control sessions, 38 tasks, averaging 32.3 calls and $0.23 (model) / $0.26
(billed) per session. Memory: the repository's 7–10 chronological training sessions.

### Where the hindsight waste is (the lever's absolute ceiling)

| | calls/session | share of calls | share of $ |
| --- | --- | --- | --- |
| every non-achieving call (hindsight waste) | 14.92 [13.74, 16.16] | 46.1% [43.8, 48.6] | 48.2% [45.4, 51.2] |
| command / procedure shortcuts | 5.58 [4.74, 6.45] | 17.2% [14.8, 19.5] | 18.9% [16.1, 21.5] |
| file shortcuts | 1.35 [0.95, 1.82] | 4.2% [3.0, 5.5] | 4.7% [3.3, 6.2] |
| fact shortcuts | 1.07 [0.65, 1.52] | 3.3% [2.1, 4.6] | 4.0% [2.4, 5.8] |
| **all shortcut kinds (absolute hindsight oracle)** | **7.32 [6.34, 8.36]** | **22.6% [20.0, 25.2]** | **24.9% [21.7, 27.9]** |

The part a library of past shortcuts covers (perfect situation matcher, support ≥ 2 sessions of
other tasks):

| memory | command | file | fact | all | share of $ |
| --- | --- | --- | --- | --- | --- |
| training split (7–10 sessions) | 1.55 [0.87, 2.39] | 0.15 [0.05, 0.26] | 0.00 | 1.63 [0.95, 2.46] | 5.4% [3.1, 7.8] |
| grown (training + adage/agents-md arms of the other tasks, 60–110 sessions) | 3.08 [2.26, 3.99] | 0.61 [0.42, 0.83] | 0.00 | 3.58 [2.70, 4.53] | 12.1% [9.2, 15.0] |

Only 22% (training) to 49% (grown) of the removable waste is of a kind that occurred in another task.
The rest is the bug's own knowledge: which function in which file, which test to extend. That is
the headline finding: **the lever is limited by transfer, not by the matcher.**

### Held-out results, my instrument (Haiku, v1+v2, 158 sessions)

| policy | calls saved/session (misfires free) | net calls, misfire = 2 calls | share of $ saved | precision [Wilson 90%] | per session |
| --- | --- | --- | --- | --- | ---: |
| learned kNN, θ tuned for λ=0 | +1.09 [+0.45, +1.94] | −3.83 [−4.89, −2.69] | +3.3% [+1.4, +5.6] | 0.15 [0.12, 0.18] (n=487) | 3.1 |
| learned kNN, θ tuned for λ=2 | +0.22 [+0.06, +0.42] | −0.52 [−0.82, −0.21] | +0.8% [+0.2, +1.5] | 0.19 [0.12, 0.27] (n=75) | 0.5 |
| frequency prior (at session start), λ=0 | +0.70 [+0.13, +1.55] | −1.25 [−2.18, −0.15] | +2.1% [+0.4, +4.4] | 0.15 [0.11, 0.20] (n=190) | 1.2 |
| kNN, fixed θ = 0.7 | +0.54 [+0.15, +1.08] | −0.43 [−0.92, +0.20] | +2.2% [+0.6, +4.0] | 0.25 [0.19, 0.33] (n=107) | 0.7 |
| **ceiling: perfect situation matcher** | +1.63 [+0.95, +2.46] | – | +5.4% [+3.1, +7.8] | – | – |
| **absolute hindsight oracle** | +7.32 [+6.34, +8.36] | – | +24.9% [+21.7, +27.9] | – | – |

Tokens saved (cumulative-context, processed tokens per session):

| policy | tokens saved per session |
| --- | --- |
| kNN, λ=0 | 51k |
| ceiling | 71k |
| oracle | 332k |

The 82 v2 sessions give the same picture:

| policy | calls saved | share of $ |
| --- | --- | --- |
| kNN, λ=0 | +1.17 [0.49, 2.10] | 3.4% |
| ceiling | +1.68 | 4.7% |
| oracle | +7.95 | 23.5% |

The **adage arm**, which received U11 live, leaves the same amount (+1.26 [0.71, 1.89], 3.7% of $).
The shortcuts my instrument credits are therefore mostly not U11's dead end.

Per repository (kNN λ=0 / ceiling):

| repository | kNN λ=0 | ceiling | ceiling share of $ |
| --- | --- | --- | --- |
| attrs | +0.46 | +0.88 | 3.6% |
| click | +3.03 [0.83, 5.98] | +3.33 | 8.7% |
| marshmallow | +1.10 | +1.87 | 6.5% |
| urfave-cli | 0 | +0.06 | 0.4% |
| vue-core | 0 | +1.00 | 4.2% |

The absolute oracle is 4.9–9.3 calls in every repository.

What the matcher said that paid off (useful / issued, held-out, λ=0):

- click "`src/click/core.py` is too large to read whole: `Read … offset=… limit=…`": 7/35, 86 calls
  in windows;
- "first `pip install -e .`": click 14/37, marshmallow 11/30, attrs 10/36;
- click "`python -m pytest tests/test_options.py -x`": 5/24;
- marshmallow "first `pip install pytest`; `pip install simplejson`": 4/24;
- "the file that matters: `src/click/core.py`": 3/23, and `src/attr/_make.py`: 6/40.

Across all deliveries:

| kind | useful | wrong |
| --- | ---: | ---: |
| command | 24 | 196 |
| procedure | 39 | 149 |
| file | 9 | 44 |

### Leave-one-repo-out (memory = the other four repositories' training sessions)

| held-out repo | kNN λ=0 | precision | kNN λ=2 | ceiling | ceiling share of $ |
| --- | --- | --- | --- | --- | --- |
| attrs | +1.64 [0.99, 2.30] | 0.13 | +0.07 | +2.27 [1.50, 3.05] | 8.5% |
| click | +0.51 [0.17, 0.91] | 0.10 | −0.01 | +1.80 [1.12, 2.48] | 5.7% |
| marshmallow | +1.61 [0.90, 2.38] | 0.16 | +0.10 | +2.57 [2.03, 3.07] | 8.7% |
| urfave-cli | −0.20 | 0.00 | −0.01 | 0.00 | 0.0% |
| vue-core | −0.27 | 0.00 | −0.01 | 0.00 | 0.0% |

Python environment procedures ("`pip install -e .` first", `python -m pytest <file>`) transfer between
Python repositories. Their LORO ceiling with ~37 foreign sessions is *higher* than the in-repo
ceiling with 10. Nothing transfers to Go or TypeScript. Facts and files never transfer across
repositories.

### Learning curve (Haiku, 158 sessions; n = 2 and 5 pool 5 random draws; "all" = grown memory, leave-task-out)

| memory per repo | kNN λ=0 | share of $ | precision | kNN λ=2 | frequency prior | ceiling | ceiling share of $ |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 2 | 0.00 | 0.0% | – | 0.00 | 0.00 | +0.65 [0.35, 0.93] | 1.9% |
| 5 | +0.86 [0.40, 1.37] | 2.6% | 0.16 | +0.62 | +0.59 | +1.37 [0.78, 2.05] | 4.2% |
| 10 | +1.09 [0.45, 1.94] | 3.3% | 0.15 | +0.22 | +0.70 | +1.63 [0.95, 2.46] | 5.4% |
| all (60–110) | +2.21 [1.56, 2.88] | 7.8% [5.6, 10.0] | 0.11 | +0.17 | +1.23 | +3.58 [2.70, 4.53] | 12.1% [9.2, 15.0] |

**Value scales with memory**: the ceiling grows 1.9% → 4.2% → 5.4% → 12.1% of $, but **precision
falls** (0.16 → 0.11). With more memory the matcher proposes more classes (10.7 per session at
"all"), and at a 2-call misfire charge the net stays at about 0.

### The common harness (`harness_eval.py`; test set `6a795a362ccb`, 158 Haiku sessions, 38 tasks; baseline $0.260 and 32.3 calls per task)

| policy | $ saved/task [90% CI] | share of $ | calls saved/task [90% CI] | Ŝ(Rust) proxy | directives | useful |
| --- | --- | --- | --- | --- | ---: | ---: |
| **learner, θ for misfire = 2 (headline)** | +$0.0010 [−0.0001, +0.0021] | +0.4% [−0.0, +0.8] | **+0.15 [+0.01, +0.31]** | +0.24 | 78 | 0.37 |
| … at p_follow = 0.5 | +$0.0004 [−0.0001, +0.0010] | +0.2% [−0.0, +0.4] | +0.07 [−0.00, +0.15] | | | |
| learner, θ for λ=0 | −$0.0023 [−0.0056, +0.0008] | −0.9% [−2.2, +0.3] | −0.17 [−0.67, +0.30] | +0.93 | 283 | 0.41 |
| … same, misfires free (upper bound) | +$0.0059 [+0.0041, +0.0076] | +2.3% [+1.6, +3.0] | +0.93 [+0.64, +1.21] | +0.93 | 283 | 0.41 |
| leave-one-repo-out (pool = all arms) | 0.0000 | 0.0% | −0.00 | 0.00 | 1 | – |
| grown memory (`leave_task_out`), λ=0 | −$0.0014 [−0.0100, +0.0069] | −0.6% [−3.8, +2.7] | +0.05 [−1.09, +1.21] | +3.32 | 713 | 0.39 |
| grown memory, θ for misfire = 2 | −$0.0001 | −0.0% | −0.01 | +0.01 | 9 | 0.22 |

Learning curve (`curve`, n = 2 / 5 / 10 training tasks): +0.00 / +0.00 / +0.15 calls.

On the 82 v2 sessions (test set `bbeaa02c4163`):

| policy | calls saved/task | share of $ |
| --- | --- | --- |
| learner | +0.07 [−0.10, +0.22] | +0.2% |
| U11 post-hoc | **+1.00 [0.65, 1.39]** | +1.9% |
| **U11 + shortcuts** | +0.97 [0.57, 1.38] | +2.0% [0.9, 3.1] |

The shortcuts add nothing to U11. `scan_memory` over the delivered texts finds no held-out task
statement.

### vs the replay Ŝ baseline

The current state is U11 plus learned reflexes: +1.00 Ŝ calls per session post-hoc and +2.11
pre-emptive. The learner's Ŝ(Rust) proxy on the harness is **+0.24** [0.08, 0.44], a quarter of U11
post-hoc. Even the λ=0 learner that sprays 1.8 directives per session reaches +0.93 gross, with a
useful rate of 0.41 against U11's 1.00. My own, more generous instrument gives +1.09 net of
delivery cost with free misfires. That is comparable to post-hoc U11 and below pre-emptive U11,
and it disappears once misfires cost anything.

### Sonnet 5 (control slices, 32 sessions; Haiku training memory)

| policy | calls saved | share of $ | precision |
| --- | --- | --- | --- |
| kNN λ=0 | +0.36 [−0.09, +1.19] | – | 0.06 |
| ceiling | +0.91 | 3.8% | – |
| oracle | +4.72 | 20.2% | – |

Haiku-learned shortcuts transfer to Sonnet even less.

## Hard-coded parts (honest list)

- **In the learner:** the token regex (`[A-Za-z_][\w./-]*`, length ≥ 3) and the thresholds
  (W_FLOW 3, T_IN 0.5, T_OUT 0.3, T_CMD 0.6, M_MIN 1.0, FACT_IDF 2.0, writer ratio 5,
  k 20, similarity weights 0.3/0.5/0.2). These are hyperparameters, not lists.
- **Structural features use the trace's own schema:** the tool name, `is_error`, and the input's
  field values.
- A token is a location if it contains `/` or is a tracked file of the checkout (read from the
  repository, not listed).
- The final-diff file list (`state.json` `changed_files`) is used to mark writes in *training*
  traces only.
- **As measurement only:** my instrument's usefulness rule (above). The harness's Sat/W classes are
  the sprint's hand-written instrument (`harness/features.py`); the learner never sees them. The
  adapter maps class families to harness kinds: Bash command → `run`, location → `read_files`,
  fact → `advice`.
- There are no tool-name lists, no error-string lists and no command vocabularies. The one exception
  is that the Directive kind is chosen from the tool name `Bash`, which is harness API plumbing.

## Verdict and what to build

- **Do not build a general situation→shortcut learner in Rust.** Its held-out value on the common
  harness is +0.15 calls and +0.4% of $ per task. Its ceiling with a perfect matcher over today's
  memory is 5.4% of $ even under the generous instrument. The North star's 15–20% is not reachable
  by delivering past shortcuts: the waste is real (25% of $ is shortcut-removable in hindsight),
  but it is **specific to the task**.
- **What is worth keeping** (small, cheap, and it feeds other levers):
  1. **The hindsight achievement labeller as an offline instrument**: data-flow closure, retry
     chains with learned failure (no error strings), and repairs. It finds U11's dead end without
     knowing about `.venv`, as a procedure. It also finds the repository-level procedures that
     transfer across Python repositories: "`pip install -e .` before running anything" and
     "install the test extras".
     - As a label source for the existing usefulness-target learner (the lab README's
       recommendation), it replaces the hand-written "exploratory step" definition with
       "non-achieving in hindsight". Expected value: equal to that learner (+0.3–1 calls), not more.
  2. **A repository-level "big file" fact**: in click, repeated failed whole-file Reads of `core.py`
     are the single most valuable recurring shortcut (86 calls saved in windows, 7/35 useful).
     It is better served by a context-size lever (an output cap or Read-with-offset rewrite, as the
     token-cost agent found) than by a delivered shortcut.
- **Revisit when** a repository has ≥ 100 sessions and a precision-first delivery. The grown-memory
  ceiling is 12% of $, but kNN precision at that size is 0.11. It would need a matcher that
  predicts *the task's own* files and facts, for example from the prompt and the repository's
  history (the co-change and context-prefetch levers), rather than from past sessions.

## Caveats

- My instrument credits the waste in D. It is kind-specific and tighter than "every wasted call in
  the window", but it is still hindsight: it assumes the agent follows, never compensates, and that
  telling it "the command that works" removes its own failed attempts. The harness numbers are
  the common, conservative currency. The two agree in order of magnitude: ≤ 1 call either way.
- "Grown" memory uses the adage and agents-md arms of *other* held-out tasks, which are
  chronologically closer to the test tasks than the training split. The harness's
  `leave_task_out` also includes other tasks' control sessions.
- Precision is low partly because many achieving calls appear only once. A "wrong" delivery here
  means the held-out session never took an equivalent achieving action. It is not direct evidence
  of harm; the harness charges 2 calls for it by prior.
- Data: 5 repositories, bug-fix tasks, Haiku 4.5 (Sonnet slice supplementary), 7–10 training
  sessions per repository.

## Reproduce

```sh
cd evals/lab/research/hindsight-shortcuts
OMP_NUM_THREADS=1 nice -n 10 python3 load.py            # parse + cache sessions (/tmp/adage-research/hindsight-shortcuts)
OMP_NUM_THREADS=1 nice -n 10 python3 experiment.py      # my instrument: main, LORO, curve -> results.json (~12 min)
OMP_NUM_THREADS=1 nice -n 10 python3 analysis.py        # waste anatomy + library ceilings -> breakdown.json
python3 report.py                                       # the tables above
cd .. && OMP_NUM_THREADS=1 nice -n 10 python3 hindsight-shortcuts/harness_eval.py   # common harness -> harness_results.json (~25 min)
```

Models: potion-base-8M from `evals/lab/fetch_models.sh` (`ADAGE_LAB_MODELS=/tmp/adage-lab-models`).

Files:

| file | what |
| --- | --- |
| `load.py` | transcripts with full I/O and usage |
| `hindsight.py` | the labeller |
| `shortcuts.py` | instances, learned equivalence, library |
| `matcher.py` | situations, embeddings, kNN |
| `costs.py` | cumulative-context $ |
| `experiment.py` | protocol and my instrument |
| `analysis.py` | breakdowns |
| `harness_eval.py` | the common harness adapter and drivers |
| `report.py` | tables |
| `results.json`, `breakdown.json`, `harness_results.json`, `result.json` | outputs |
