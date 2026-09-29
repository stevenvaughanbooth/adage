# Learned action equivalence vs the hand-written taxonomy (`action.rs`)

Slug `action-equivalence`, 2026-09-29. Offline, $0, Python in this directory. No Rust was touched.

## Question

Can we **learn** when two actions are "roughly the same", instead of using the hand-written
taxonomy in `crates/adage-core/src/action.rs`? The test case is that `pytest tests/a.py -x` in
the project's environment is the same as `.venv/bin/python -m pytest tests/b.py -q`, but differs
from `python -m pytest` under the system Python, which fails.

## Answer in brief

**Negative, with one positive finding. The lever is also small, so don't prioritise it.**

1. **No learned representation matches `action.rs` at deciding equivalence.**
   - On a hand-labelled gold set, `action.rs`'s equivalence (role·runner·env class) reaches pair
     F1 **0.81 [0.76, 0.86]**. The best learned representation, action2vec, reaches **0.72
     [0.67, 0.79]**, and the others 0.37–0.69.
   - The gap widens on a new repository (leave-one-repo-out, pairs within the held-out repo):
     0.88 for `action.rs` against at most 0.61 for any learned representation.
   - The trivial baseline "same first word" does as well as the best learned embedding across
     repositories (0.64 / 0.65 within / cross).
   - Caveat: `action.rs` was written by people who had seen these five repositories' tools, and
     my gold rubric uses the same kind of categories (role × tool family × environment class).
     Both favour it.
2. **One learned signal does recover the environment distinction: outcomes observed before the
   agent repaired anything.**
   - Supervising a linear projection with *all* outcomes fails to separate project-env from
     system-env pytest. It splits only 40% of the cross-env pairs, because 60–70% of system
     `python -m pytest` runs succeed (after the agent's `pip install pytest`).
   - Supervising it with **first-in-session** outcomes only (`rrr-first`) splits **95%** of the
     cross-env pytest pairs and 76% of the cross-env repro pairs, with no hand-written feature.
     For comparison: `action.rs` splits 100% (by rule), action2vec 21%, potion-8M 0%.
   - Its overall pair F1 is only 0.55. It keeps the environment split but merges or scatters
     other classes.
3. **Downstream, no representation gives a reliable gain.**
   - I plugged each representation into the same outcome-optimising reflex learner and scored it
     with the common harness (Haiku, 158 held-out sessions, 38 tasks, test set `6a795a362ccb`).
   - On the brief's train split (7–10 sessions per repo), every representation, `action.rs`
     included, forms ~0 useful reflexes: `rrr-first` −0.07 calls/task [−0.26, +0.11];
     `action.rs` 0.
   - Leave-one-repo-out, `rrr-first` scores **+1.41 calls/task [0.61, 2.24] = +2.7% of $
     [0.6, 5.1]**, against +0.65 [0.36, 0.96] (+0.6% of $) for `action.rs` in the same learner.
   - But the learned directives are mostly `pip install -e .` (an uncalibrated Command class), or
     a system-Python pytest spelling carrying another task's test id. Replay credits these under
     its goal-class rule. Under the strict rule, or at p_follow 0.5, the gain collapses
     (grown-memory: +0.09 [−0.21, +0.42]). **I don't believe it survives live.**
4. **Ceiling: at most ~8–9% of calls, so this lever cannot reach 15–20%.**
   - A perfect recovery reflex, pre-empting every failed call with exactly the fix the agent
     later found, is worth +2.49 calls/session [2.19, 2.78] = 8.1% of calls (instrument; about
     $0.020/session).
   - A perfect "you already ran this, nothing changed" lever is worth 2.98 [2.59, 3.40] calls =
     8.7% with exact matching. Learned equivalence adds only ≤ 0.16 calls to that (3.14 with
     action2vec).
   - Action equivalence is an enabling primitive for levers whose ceilings are ~10% of $ (the
     harness: deleting every failed call = 10.3% of $). It is not the missing piece.

**So what.** If shipped, a learned `rrr-first` representation replacing `action.rs` would have
an expected effect of **≈ 0 calls and ≈ 0% of $** on repositories like these. The replay's
+2.7% of $ LORO number is an instrument artefact until a live micro-slice shows the `pip install
-e .` directive is followed and helps. It has no pass-rate effect. Confidence it survives a live
A/B as a gain: low (< 20%). Confidence it does no harm: moderate. The value is generality to
tools `action.rs` has no table for, bounded by the ~8% recovery ceiling.

## The gold set (MEASUREMENT INSTRUMENT, hand-built; I built it)

- **Sample.** 350 distinct Bash commands, 70 per repository, sampled uniformly (seed 11) from
  held-out **control-arm** calls (`gold_sample.py` → `gold_commands.json`).
- **Labels.** I labelled every command by hand into 18 functional classes (`gold_labels.tsv`).
  - Rubric: *would substituting one command for the other leave the local outcome unchanged?*
  - Each class is purpose × tool family × environment class, targets free:
    - tests: `test:py:{proj,sys}`, `test:go`, `test:node`;
    - running code: `run:py:{proj,sys}`, `run:go`, `run:node`;
    - search, read and VCS: `search:{text,files}`, `read:file`, `vcs:{inspect,mutate}`;
    - environment, checks and files: `env:{install,probe}`, `check:{lint,build}`, `fs:write`.
  - A compound takes the class of its outcome-determining part: `pip install pytest && python
    -m pytest …` is `test:py:sys`.
  - `.venv/bin/…`, `source .venv/bin/activate`, `uv run`, or an agent-made venv counts as
    `proj`. A bare `python`/`python3`/`pytest`, `PYTHONPATH=src python` or `~/.local/bin/pytest`
    counts as `sys`.
  - Note that in these sandboxes a bare `pytest` is the system one. So the question's
    `pytest tests/a.py -x` is `proj` only when a venv is active.
- **Pairs.**
  - All 61,075 pairs. Metrics use the 15,225 pairs among the 175 **test-half** items (odd ids);
    the even-id half tunes the similarity threshold τ.
  - A fixed stratified 300-pair sample (`gold_pairs.tsv`): 150 equivalent, 75 hard
    non-equivalent (same first word), 75 random non-equivalent.
- **Limits.** One labeller, no inter-annotator check, and a rubric close to `action.rs`'s own
  categories. Read the intrinsic numbers as "agreement with an `action.rs`-like notion of
  equivalence".

## Methods

Every learned method sees only the command text (the sandbox root made relative), the output as
the model saw it, the harness's `is_error` flag, and the order of calls in the session.

- **Fixed by hand: shell *syntax* only** (`feats.py`):
  - `&& || ; | &` separate simple commands, and a segment after `|` consumes output;
  - heredoc bodies and long quoted strings are data;
  - the first word is the head, and a leading `-` marks a flag;
  - `2>&1`-style redirections are removed.
- **Learned from the corpus: everything else.** The vocabulary keeps a feature only if it recurs
  in ≥ 2 sessions. BPE sub-words come from HF `tokenizers`, trained on the corpus. All weights
  are learned.
- **Fit set.** Representations are fitted on the non-control sessions (train, adage and
  agents-md arms; 5,835 Bash calls). They are never fitted on the held-out control arm. LORO
  refits without the held-out repository.
- **From embedding to equivalence.**
  - Average-linkage cosine clustering of the fit commands, cut at τ (τ tuned on the dev half).
  - A new command takes its nearest prototype's class if cos ≥ τ, and is its own class
    otherwise. This is exactly what a hook would do.

| # | method | what it is |
|---|---|---|
| 1 | `a2v` (action2vec) | PPMI-SVD (64-d) of feature × trace context. Contexts are the neighbouring actions (±2) and the action's own outcome (output words, a Drain-style first-line template, the error flag). A command vector is the TF-IDF-weighted mean of its feature vectors. |
| 2 | `tfidf` | corpus-vocabulary positional tokens + BPE pieces, TF-IDF → SVD-256 |
| 2 | `potion` | potion-base-8M static embedding of the normalised command line |
| 3 | `rrr` | outcome-supervised metric: reduced-rank ridge regression from command features to outcome features Y (output words, template, `is_error`, next action's head), rank 32. Two commands are close when they predict the same outcome distribution. |
| 3 | `triplet` | linear projection (32-d) trained in numpy with a cosine triplet loss. Positives and negatives are defined by outcome similarity. |
| 3 | **`rrr-first`** | `rrr` supervised only by each head word's **first call in the session**: the action's own outcome, before any repair |
| 2+3 | `hybrid`, `hybrid-first` | [tfidf-SVD, rrr(-first)] concatenated |
| 4 | `lgg-L`, `lgg-H` | anti-unification with **learned literals** (a token is literal if it recurs in ≥ 3 sessions of ≥ 2 tasks, otherwise a hole) and **learned slot types** (character shape + a recurring extension). Flags form a set, and output filters are dropped. `L` is the full skeleton; `H` is each segment's head + first literal operand. |

Baselines:
- `taxonomy`: `action.rs`'s follow equivalence, role|runner|env class, via its Python ancestor
  `evals/lab/abstraction/grammar.py`. **Hand-written.**
- `intent`: grammar.py's env-free intent.
- `lgg-loose`: the prior abstraction lab's LGG, with hand-typed holes from repo facts.
- `literal`, `head` (first word) and `headsub` (first two words).

## Results

All tables are in `results/tables.md` (`report.py`), and the raw numbers are in `results/*.json`.

### 1. Intrinsic: agreement with the gold set

In-repo means fitted on all five repositories' non-control sessions. CIs are 90% bootstraps over
gold commands. The two env columns are for Python test runs: the share of same-env pairs merged,
and the share of cross-env pairs split.

| representation | kind | pair P | pair R | **pair F1** [90% CI] | purity / inv. purity | 300-pair acc | same-env merged / cross-env split | LORO F1 within / cross |
|---|---|---:|---:|---|---|---:|---|---|
| taxonomy | hand-written (`action.rs`) | 0.68 | 0.99 | **0.81** [0.76, 0.86] | 0.88 / 0.97 | 0.97 | 1.00 / 1.00 | **0.88 / 0.77** |
| a2v | learned (1) | 0.63 | 0.85 | **0.72** [0.67, 0.79] | 0.76 / 0.90 | 0.76 | 0.93 / 0.21 | 0.55 / 0.56 |
| hybrid | learned (2+3) | 0.73 | 0.65 | **0.69** [0.61, 0.77] | 0.83 / 0.77 | 0.79 | 0.65 / 0.72 | 0.61 / 0.54 |
| head | trivial | 0.65 | 0.66 | **0.65** [0.58, 0.73] | 0.75 / 0.75 | 0.57 | 0.40 / 0.87 | 0.64 / 0.65 |
| hybrid-first | learned (2+3) | 0.71 | 0.58 | **0.64** [0.56, 0.71] | 0.83 / 0.74 | 0.77 | 0.84 / 0.71 | 0.58 / 0.51 |
| intent | hand-written | 0.47 | 0.84 | **0.61** [0.55, 0.66] | 0.66 / 0.86 | 0.84 | 0.77 / 0.22 | 0.65 / 0.59 |
| rrr | learned (3) | 0.59 | 0.59 | **0.59** | 0.78 / 0.74 | 0.76 | 0.69 / 0.40 | 0.62 / 0.49 |
| tfidf | learned (2) | 0.55 | 0.57 | **0.56** [0.49, 0.63] | 0.73 / 0.75 | 0.64 | 0.67 / 0.37 | 0.55 / 0.47 |
| potion | learned (2) | 0.55 | 0.58 | **0.56** | 0.78 / 0.74 | 0.70 | 1.00 / 0.00 | 0.50 / 0.48 |
| **rrr-first** | learned (3) | 0.50 | 0.60 | **0.55** [0.47, 0.62] | 0.75 / 0.77 | 0.73 | 0.62 / **0.95** | 0.44 / 0.39 |
| headsub | trivial | 0.90 | 0.38 | **0.54** [0.46, 0.61] | 0.96 / 0.58 | 0.69 | 0.38 / 1.00 | 0.59 / 0.47 |
| lgg-H | learned (4) | 0.93 | 0.29 | **0.45** [0.34, 0.54] | 0.99 / 0.49 | 0.63 | 0.35 / 1.00 | 0.66 / 0.37 |
| triplet | learned (3) | 0.53 | 0.28 | **0.37** | 0.71 / 0.51 | 0.56 | 0.18 / 0.89 | 0.46 / 0.31 |
| lgg-loose | prior lab | 1.00 | 0.08 | **0.15** [0.11, 0.21] | 1.00 / 0.29 | 0.57 | 0.05 / 1.00 | 0.18 / 0.12 |
| lgg-L | learned (4) | 1.00 | 0.05 | **0.10** | 1.00 / 0.23 | 0.53 | 0.01 / 1.00 | 0.15 / 0.05 |
| literal | trivial | 1.00 | 0.01 | **0.03** | 1.00 / 0.15 | 0.51 | 0.00 / 1.00 | 0.03 / 0.03 |

The LORO columns are fitted without the held-out repository, with τ tuned on the other
repositories' dev items. "Within" is pairs inside the held-out repository; "cross" is its items
against the other repositories'. Per repository (within / cross):

| representation | attrs | click | marshmallow | urfave-cli (Go) | vue-core (TS) |
|---|---:|---:|---:|---:|---:|
| taxonomy | 0.85 / 0.79 | 0.96 / 0.85 | 0.84 / 0.77 | 0.92 / 0.71 | 0.81 / 0.73 |
| head | 0.65 / 0.66 | 0.49 / 0.61 | 0.58 / 0.58 | 0.80 / 0.68 | 0.68 / 0.75 |
| a2v | 0.62 / 0.64 | 0.62 / 0.61 | 0.56 / 0.57 | 0.43 / 0.50 | 0.52 / 0.49 |
| hybrid | 0.65 / 0.66 | 0.56 / 0.55 | 0.69 / 0.62 | 0.61 / 0.31 | 0.52 / 0.56 |
| rrr-first | 0.49 / 0.52 | 0.55 / 0.52 | 0.55 / 0.50 | 0.36 / 0.12 | 0.26 / 0.30 |
| rrr-first, no BPE (`ablate_bpe.py`) | 0.62 | 0.60 | 0.52 | 0.83 | 0.43 |

**Reading.**
- The learned embeddings lose most on the two repositories whose tools no other repository uses:
  Go on urfave-cli and TypeScript on vue-core. A new repository with new tools is exactly where a
  learned representation was supposed to help. With ≤ 4 other repositories to learn from, it
  can't place `go test` next to `pytest`, and `action.rs` can because a person told it to.
- Dropping BPE sub-words makes `rrr-first` generalise better across repositories (LORO within
  0.60 against 0.44). Its env split stays at 0.93, and in-repo F1 drops only slightly (0.51
  against 0.55). A hook without a BPE tokenizer is simpler and no worse.
- action2vec (distributional context) is the best learned method at the gold rubric's
  granularity. It does so by *merging* environments: the same contexts surround a failing and a
  working pytest.

### 2. The question's own example: project env vs system env

`env_split.py`, all 350 gold items:

| | Python test runs: same-env pairs merged | cross-env pairs split | Python repros: merged | split |
|---|---:|---:|---:|---:|
| taxonomy (hand-written) | 1.00 | 1.00 | 1.00 | 1.00 |
| potion-8M | 0.98 | 0.04 | 0.67 | 0.98 |
| a2v | 0.93 | 0.21 | 0.94 | 0.15 |
| rrr (all outcomes) | 0.69 | 0.40 | 0.82 | 0.53 |
| **rrr-first (first-in-session outcomes)** | 0.62 | **0.95** | 0.93 | **0.76** |
| hybrid-first | 0.84 | 0.71 | 0.93 | 0.51 |

The environment distinction is *state-dependent*. The same `python -m pytest` fails before the
agent's `pip install pytest` and works after it. Outcome supervision that pools all calls
therefore sees two nearly identical outcome distributions. Restricting supervision to the first
call of each kind in a session removes the state, and the distinction appears. That is the
learnable version of `action.rs`'s `EnvClass`.

### 3. Downstream: the same reflex learner, one representation at a time

The learner (`downstream.py`) is identical for every representation R, with no hand-written
parts:
- **Fix.** A failed X call followed within 6 calls by a *successful* call of the same coarse
  class but a different fine class Y. Coarse is one level up: a lower cut of the same
  dendrogram, the next shorter anti-unifier, or role|runner for `action.rs`.
- **Reflex X → Y.** Kept if the fix was seen in ≥ 3 sessions and fail(Y) ≤ fail(X)/2, so it
  optimises outcomes rather than frequency.
- **Delivery.** Pre-emptively at the first X call when fail(X) ≥ 0.5, which needs no hindsight.
- **Failure.** Learned: `is_error`, or an output template that was ≥ 80% `is_error` in the fit
  corpus.
- **Scoring.**
  - First with the replay port (`instrument.py`, a hand-written measurement instrument; no misfire
    charge). "strict" requires the directed interpreter as well as the runner.
  - Then with the common harness, which adds a 2-call misfire charge and the
    cumulative-context $ model.

Instrument Ŝ, pre-emptive (learned), over 190 held-out control sessions of 31.7 calls on
average:

| representation | train split | grown memory (task 5-fold) [90% CI] | strict | LORO [90% CI] | strict |
|---|---:|---|---:|---|---:|
| taxonomy (`action.rs`) | 0.00 | +0.67 [0.48, 0.86] | +0.16 | +1.62 [1.40, 1.85] | +0.41 |
| rrr-first | +0.28 | +1.00 [0.85, 1.15] | +0.72 | +1.81 [1.51, 2.13] | +1.51 |
| hybrid-first | +0.29 | +0.64 [0.51, 0.77] | +0.64 | +1.54 [1.26, 1.83] | +0.40 |
| hybrid | +0.02 | +1.00 [0.76, 1.26] | +0.15 | +0.44 [0.30, 0.60] | +0.42 |
| lgg-H | 0.00 | +0.29 [0.19, 0.39] | +0.09 | +0.95 [0.76, 1.16] | +0.33 |
| a2v | +0.20 | +0.16 [0.12, 0.21] | +0.16 | +0.33 [0.24, 0.43] | +0.33 |
| potion | +0.04 | +0.65 [0.53, 0.79] | +0.65 | +0.68 [0.56, 0.82] | +0.68 |
| lgg-loose (prior lab) | 0.00 | +0.13 [0.05, 0.23] | +0.13 | +0.66 [0.49, 0.85] | +0.57 |

Common harness: Haiku held-out control, 158 sessions / 38 tasks, test set `6a795a362ccb`,
baseline $0.260 and 32.3 calls per task, p_follow 1, misfire 2 calls (`harness_eval.py`,
`results/harness.json`):

| policy | driver | $ saved/task [90% CI] | share of $ [90% CI] | calls saved/task [90% CI] | directives | useful |
|---|---|---|---|---|---:|---:|
| learner × taxonomy (`action.rs`) | split | 0 | 0 | 0 | 0 | – |
| learner × taxonomy | loro | +0.0016 [−0.0002, +0.0033] | +0.6% [−0.1, +1.3] | +0.65 [0.36, 0.96] | 185 | 0.52 |
| learner × taxonomy | grown (train + other tasks' control) | −0.0015 [−0.0049, +0.0017] | −0.6% [−1.9, +0.7] | +0.08 [−0.44, +0.60] | 108 | 0.43 |
| learner × **rrr-first** | split | −0.0010 [−0.0027, +0.0007] | −0.4% [−1.0, +0.3] | −0.07 [−0.26, +0.11] | 70 | 0.60 |
| learner × **rrr-first** | **loro** | **+0.0070 [+0.0017, +0.0127]** | **+2.7% [+0.6, +5.1]** | **+1.41 [0.61, 2.24]** | 137 | 0.59 |
| learner × rrr-first | grown | +0.0040 [+0.0016, +0.0065] | +1.5% [+0.6, +2.5] | +0.65 [0.28, 1.04] | 114 | 0.77 |
| … strict Sat | grown | −0.0003 [−0.0027, +0.0021] | −0.1% [−1.0, +0.8] | +0.09 [−0.21, +0.42] | 114 | 0.59 |
| … p_follow = 0.5 | grown | +0.0019 [+0.0007, +0.0031] | +0.7% [+0.3, +1.2] | +0.32 [0.13, 0.51] | 114 | 0.77 |
| learner × hybrid-first | loro | +0.0076 [+0.0022, +0.0130] | +2.9% [+0.8, +5.2] | +1.40 [0.62, 2.19] | 141 | 0.65 (harm 0.02) |
| learner × hybrid-first | grown | −0.0040 [−0.0073, −0.0006] | −1.6% [−2.8, −0.2] | −0.37 [−0.78, +0.07] | 180 | 0.52 |
| learner × lgg-loose (prior lab) | loro | +0.0034 [+0.0016, +0.0053] | +1.3% [+0.6, +2.1] | +0.71 [0.38, 1.08] | 57 | 0.88 |
| *U11 predictive, hand-written (harness README)* | *split / loro* | | *+3.5% / +1.9%* | *+1.63 / +0.88* | | |

**What the learned reflexes actually say.** The gains don't come from better equivalence of the
kind the question asks about.
- **`action.rs` in this learner** learns the right thing: "system `pytest` run failed → `source
  .venv/bin/activate && python3 -m pytest …`" (attrs, click and marshmallow, grown and LORO).
  But its first-call failure prior is 0.37–0.67, so it fires on many system runs that would have
  worked. In the harness its useful rate is 0.43–0.52, and misfires cancel the savings.
- **`rrr-first`** learns mostly "a system-Python call failed → `pip install -e . -q`". That is
  the recovery the agents themselves used most often, and it is valid in click and marshmallow.
  LORO transfers it to vue-core and urfave-cli, where it is nonsense but rarely fires usefully.
  - In attrs under LORO it learns "→ `python -m pytest tests/test_shell_completion.py::…`": a
    *click* test id, in the *system* Python. The harness's goal class credits it as soon as the
    agent gets pytest working by any route: attrs alone is +3.5 calls, 9.9% of $.
  - A person would reject that directive. The replay can't: Sat is a hand-written class, and its
    `run <other command>` class is uncalibrated.
- **With 7–10 training sessions** (the brief's split, what the product sees), no representation
  forms the `.venv` reflex. Only 3 of 23 training sessions in the Python repositories ever
  switched from a system to a project test run.

### 4. Ceilings (why this lever is small)

Held-out control sessions: 190 in total, 31.7 calls each on average. 90% bootstrap over sessions
(`downstream.oracle`, `ceiling.py`):

| ceiling | calls/session [90% CI] | share of calls |
|---|---|---:|
| failed calls + environment friction (all waste the recovery class could target) | 4.3 | 13.6% [12.5, 14.6] |
| **perfect recovery reflex**: every failed call pre-empted by exactly the fix the agent later found | **+2.49 [2.19, 2.78]** | **8.1%** [7.1, 9.0] (≈ $0.020) |
| same, v2 Haiku sessions only | +3.46 [2.97, 3.96] | – |
| perfect "already ran this, nothing edited since", exact command match | 2.98 [2.59, 3.40] | 8.7% [7.7, 9.7] |
| same, with learned equivalence (best: a2v) | 3.14 [2.74, 3.55] | 9.2% |
| same, with `action.rs` | 3.11 [2.70, 3.53] | 9.1% |

The harness's own oracle table agrees: deleting every failed call is worth 10.3% of $.
Equivalence decides *which* of these calls a reflex can match. It can't make the pool bigger.

## Recommendation (what to build)

**Keep `action.rs`. Don't replace it with a learned representation now.** Everything above says
a learned one is worse at equivalence (F1 0.55–0.72 against 0.81, LORO ≤ 0.61 against 0.88).
Downstream it is no better once directive quality is checked, and the lever's ceiling is ~8% of
calls.

If a learned representation is wanted for tools `action.rs` has no table for (it labels them
`other`), build `rrr-first` without BPE, as a **fallback labeller** and a **learned env-class
signal**, behind `action.rs`:

- **What it is.** Each command gets a shell-syntax lex, then sparse positional features (head
  word, head + second word, flags, operands, operand basenames). The vocabulary is a hash map of
  about 1.1k features. The sum of the matching rows of W (1.1k × 32, float32) is normalised, and
  the class is the nearest of about 20–30 prototypes (32-d) if cos ≥ τ (tuned on held-out labels: 0.6 for the no-BPE variant), else "new".
  - W is fitted offline by reduced-rank ridge regression from the features to first-in-session
    outcome features: output words, first-line template, error flag and next action.
  - Refit as traces accumulate. It is an `adage condition`-time job of 1–2 minutes in numpy for 6k
    calls on this loaded machine.
- **Size.**
  - About 140 KB of float32, or 35 KB as int8.
  - About 300 KB with the BPE variant (2.4k × 32), plus a 2k-merge BPE table. That variant
    isn't recommended: it generalises worse to new repositories.
- **Speed at hook time.**
  - The hook does a lex, ~17 hash lookups, 17 × 32 multiply-adds for the projection and
    ~25 × 32 for the nearest prototype. That is ~1.3k FLOPs, so in Rust it is a few µs (under
    10 µs), dominated by lexing.
  - Measured in Python: 0.34 ms lex+features, 0.14 ms projection and 1.4 µs nearest prototype
    per command (`bench_hook.py`, `results/bench_rrr-first.json`).
- **Expected value.**
  - ≈ 0 on repositories `action.rs` already covers.
  - On a repository with unknown tools, at most the recovery ceiling (≤ 2.5 calls/session,
    about 8% of calls). Realistically a fraction of it, since LORO gold agreement is 0.44–0.60.
- **Before any build, one check.** Run a live micro-slice (harness protocol, about $5) of the
  learned `pip install -e .` reflex on marshmallow and attrs, where the replay predicts the most.
  If p_follow < 0.6 or the local saving is < 50% of the prediction, drop the learned route
  entirely.

More useful for the sprint than this lever: the finding that **first-in-session outcome
supervision** recovers state-independent action properties without hand-written features. Any
learner that scores actions by their outcomes, such as a learned failure prior, a learned
"dead-end spelling" list, or a U11 generalisation, should condition on "before any repair" in
the same way.

## Hard-coded parts (honest list)

- **Measurement only.**
  - `instrument.py`: a port of the Rust replay's Sat/W features, with runner names,
    environment-signature regexes and probe regexes.
  - The harness's features.
  - The hand-labelled gold set and its rubric.
  - `ceiling.py`'s shell-write heuristic, used for the redundancy ceiling.
- **Baselines only.** `taxonomy` and `intent` (grammar.py, the ancestor of `action.rs`), and
  `lgg-loose` (the prior lab's typed holes and repo facts).
- **In the learned methods.**
  - Shell syntax (separators, pipes, heredocs, quoting, `2>&1`, flags start with `-`). This is
    the grammar of the language, not a taxonomy of tools.
  - The number thresholds: vocabulary ≥ 2–3 sessions, the reflex gates ≥ 3 sessions and
    fail(Y) ≤ fail(X)/2, the failure-template 80% threshold, window 6, and a coarse level of
    τ − 0.2.
  - The choice of outcome features (output words, first-line template, error flag, next head).
- **In the learner's data choice.** Reflexes are learned from `train` + `control` arms only,
  because the adage arm was steered by U11. Representations are fitted on train + adage +
  agents-md: command/outcome statistics only, never the held-out control arm.

## Caveats

- **Gold.** One labeller, and a rubric close to `action.rs`'s categories. The downstream columns
  don't depend on it.
- **The replay's Sat rules.**
  - Downstream numbers depend on them. The goal-class runner rule credits any pytest directive
    once the agent gets pytest working, and the `run <other command>` class is uncalibrated.
  - The strict column and the qualitative check are my guard against that. The harness flags the
    same (`uncalibrated classes: command`).
- **Learned failure misses piped dead ends.** `… | head` exits 0. The learned failure signal only
  saw `is_error` and one template ("Exit code N"). A logistic model on output words (tested)
  caught 0 of 78 piped environment-signature outputs.
- **LORO for `action.rs` is optimistic.** Its tables were written with these five repositories'
  tools in mind.
- **Machine load.** Load average was 25–32 on 4 cores, so the Python timings are inflated.

## Reproduce

```text
cd evals/lab/research/action-equivalence
OMP_NUM_THREADS=1 nice -n 10 python3 data.py            # session cache (needs /tmp/adage-abstraction-cache.pkl)
python3 gold_sample.py                                   # gold commands (labels: gold_labels.tsv, by hand)
python3 eval_gold.py                                     # intrinsic + LORO (writes results/gold_eval.json, taus.json)
python3 gold_ci.py; python3 env_split.py; python3 ablate_bpe.py
python3 downstream.py in; python3 downstream.py loro     # instrument Ŝ
python3 ceiling.py; python3 bench_hook.py rrr-first
python3 harness_eval.py taxonomy,rrr-first,hybrid-first,lgg-loose
python3 report.py                                        # results/tables.md
```

Caches are in `/tmp/adage-research/action-equivalence/`: the session and baseline pickles are
kept, and the fitted models are deleted (they refit in about 30 min under load).

## Files

| file | what |
|---|---|
| `data.py` | loader (via the abstraction lab's cache), de-duplication, run-root stripping, cost |
| `feats.py` | shell-syntax lexer, BPE, TF-IDF featuriser, outcome tokens and templates |
| `methods.py` | all representations, clustering and assignment |
| `instrument.py` | replay Sat/W/S port (measurement) |
| `gold_sample.py`, `gold_commands.json`, `gold_labels.tsv`, `gold_pairs.tsv` | the gold instrument |
| `eval_gold.py`, `gold_ci.py`, `env_split.py`, `ablate_bpe.py` | intrinsic evaluation |
| `downstream.py`, `harness_eval.py` | downstream reflex learner, instrument and harness scoring |
| `ceiling.py`, `bench_hook.py`, `report.py`, `common.py` | ceilings, hook cost, tables, shared helpers |
| `results/` | every number quoted here |
