# approx-subgraph: does similarity-tolerant pattern mining find reflexes worth delivering?

2026-09-29. Research sprint, slug `approx-subgraph`. Offline and $0. Python lives in this
directory, and no Rust crate was touched.

## So what

**Negative.** Similarity-tolerant mining finds real structure. Embedding-clustered nodes compress
held-out traces 2 to 10 times better than exact labels, and most of that structure transfers across
repositories. The structure it finds is the agent's habits: re-running the same test, re-reading a
file, `git commit` followed by `git log`, and repro-script loops. The agent already does these
without help.

Compiled into delivered steps and scored by the common harness, the policy **loses money** under
every protocol:

| protocol | $/task | calls/task |
|---|---|---|
| split | −0.1% [−0.3, −0.0] | −0.04 [−0.09, −0.00] |
| leave-task-out | −0.3% [−0.6, −0.0] | −0.07 [−0.16, +0.00] |
| leave-one-repo-out | −0.5% [−0.9, −0.1] | −0.16 [−0.34, −0.01] |

- All three rows use the harness test set `6a795a362ccb` (158 Haiku sessions, 38 tasks, baseline
  $0.260 and 32.3 calls per task). Brackets are 90% task-clustered CIs.
- The lever's own ceiling is small. Deleting every failed or repository-discovery call is worth
  3.0 calls per session (9.3% of calls). The harness oracle puts every failed call at 10.3% of $.
  A skip-ahead reflex cannot reach the 15–20% target.
- **Expected effect if shipped:** ≈ 0 $, ≈ 0 calls and no pass-rate change (CIs above). I am
  highly confident a live A/B would find nothing: the patterns describe what agents already do.
- **Don't build.**

## Question

The product vision is an event graph with embedding-bearing nodes. A learner crawls it for common
subgraphs, and those become reflexes. Exact-match mining found almost nothing, because agents rarely
repeat themselves exactly. This study tests the similarity-tolerant alternatives:

- node matching by embedding similarity, with a threshold τ;
- gap-tolerant sequential patterns;
- process mining over clustered activities (Heuristics Miner and Inductive Miner);
- MDL selection, in the SQS/Krimp family, where a pattern must pay for itself in bits.

Each was measured by held-out compression, coverage, usefulness (prefix → suffix), the value of
compiling it into a delivered sequence, leave-one-repo-out transfer, and comparison with the Rust G
engine.

## Method (nothing hand-written in the learner)

**Nodes** (`nodes.py`) are built in four steps:

1. **Abstraction by recurrence (learned).** A call's text is split into word pieces. A piece is
   kept only if it recurs in ≥ 2 training *tasks*, or ≥ 2 *repositories* under leave-one-repo-out.
   Otherwise it becomes `·`. For example, `tests/test_options.py::test_x -xvs` becomes
   `tests/·.py::· -xvs`. This is document frequency, the Drain/log-template idea.
2. **Embedding.** The vector is potion-base-8M of the abstracted text concatenated with a 256-d
   signed hash of its piece n-grams (Track G §1.2's structural half).
3. **Clustering.** Clusters are average-linkage agglomerative at cosine τ ∈ {0.9, 0.8, 0.7, 0.6}.
4. **Label.** A call's label is its cluster plus the harness `is_error` bit. A held-out call takes
   the nearest centroid if cos ≥ τ, otherwise it is unknown.

**Baselines** are exact labels:
- `literal`: the call's first line;
- `full` and `key`: the hand-written normaliser `evals/lab/normalize.py`. These are baselines only.

**Mining** (`mdl.py`):
- PrefixSpan with gap ≤ 8 between items and 2–5 items;
- support in distinct sessions ≥ max(3, 10%);
- a closure filter.

**Selection** (`mdl.py`) is an SQS-style code table with a greedy Krimp cover. It has a usage-coded
pattern stream, per-pattern gap/fill codes and universal integer codes in L(CT). Candidates are
added by estimated gain and kept only if L(CT)+L(D|CT) falls, then pruned.

**Held-out compression** codes the held-out sessions with the *training* code lengths, fixed and
Laplace-smoothed. The reference is the singleton-only code on the same alphabet. The reported figure
is bits saved per session and as a share of that alphabet's own baseline.

**Usefulness** (`evaluate.py`): at the first completion of a prefix, does the suffix's first item
follow within 8 calls? The report gives the rate, the lift over the item's base rate after a random
position, and the number of calls between.

**Delivery replay** (`evaluate.py`): the suffix is delivered at the prefix. It is useful if a later
successful call carries the suffix's label.
- Savings are counted over the union of windows in three tiers:
  - failed calls;
  - failed plus discovery calls (the embedding lab's hand-written instrument, used for measurement
    only);
  - all calls, which is the skip-ahead bound.
- Net = 0.8·saved − 0.1 per directive − 0.3 per misfire.
- Control: **the same suffixes delivered after the first call, ignoring the prefix.** This isolates
  what the pattern's context adds.
- Compilation follows Track G §2: a suffix item becomes a `run` only when one literal holds ≥ 80%
  of its training occurrences.

**Process mining** (`pm.py`, textbook implementations; pm4py was not available):
- Heuristics Miner: dependency ≥ 0.9 and support ≥ s_min. Its edges are scored as two-step
  patterns.
- Inductive Miner–infrequent: DFG cuts filtered at f = 0.2, with a flower fall-through.

**Settings** (`run.py`), all scored on held-out Haiku **control** sessions (176, 38 tasks):
- `chrono`: the RepoBench split, training on 7–10 sessions per repository.
- `grown`: 5 task-grouped folds over every unsteered session of the repository, 40–100 training
  sessions.
- `loro`: training on the other four repositories (about 250 sessions).
- Sonnet 5 control sessions (32) are reported in `results/tables.txt`.

CIs are 90% task-clustered bootstraps (2,000 draws), and Wilson intervals for rates.

**Final policy** (`harness_policy.py`) is scored by the common harness (`evals/lab/research/harness`)
through its `split`, `leave_task_out` and `loro` drivers, with its loader and cost model.

## Results (held-out; full tables in `results/tables.txt`)

### 1. Compression and coverage: similarity helps, and helps more with more data

Figures are the share of held-out bits saved, with the share of sessions where a selected pattern
occurs in brackets.

| alphabet | chrono (7–10 train) | grown (40–100 train, same repo) | leave-one-repo-out (~250 train) |
|---|---|---|---|
| literal (exact) | 0.0% [−0.0, 0.1] (cov 2%) | 0.5% [0.3, 0.8] (43%) | 0.1% [0.1, 0.2] (20%) |
| full (normaliser) | 0.0% [−0.0, 0.1] (4%) | 1.1% [0.8, 1.4] (56%) | 0.5% [0.4, 0.7] (59%) |
| key (normaliser, HeadSub-like) | 0.3% [−0.0, 0.7] (15%) | 0.5% [0.1, 0.9] (41%) | 0.9% [0.7, 1.1] (39%) |
| emb τ=0.8 | 0.2% [0.0, 0.4] (7%) | **2.3% [1.5, 3.2]** (59%) | 3.0% [2.5, 3.7] (67%) |
| emb τ=0.7 | 0.2% [−0.1, 0.5] (12%) | **2.6% [1.7, 3.4]** (69%) | **4.9% [4.0, 5.9]** (77%) |
| emb τ=0.6 | −0.1% [−0.3, 0.0] (6%) | 1.1% [0.4, 1.8] (55%) | 5.5% [4.7, 6.3] (65%) |

- **Chrono split.** On the RepoBench split (7–10 training sessions), MDL keeps 0–1 patterns per
  repository whatever the alphabet. There is not enough data for any pattern to pay for itself.
  This is the Rust engine's result (0 routes) reproduced with a similarity-tolerant alphabet.
- **Grown memory.** With more history, embedding clusters compress 2–5 times better than exact
  labels.
- **Leave-one-repo-out.** When the vocabulary is abstracted across repositories, patterns do
  transfer:
  - 7% on attrs, click and marshmallow;
  - 1.7–3.7% with 25–35% coverage on vue-core and urfave-cli.
- **Absolute size.** The largest gain is 13 bits out of about 250 per session.

### 2. What was found: habits, not procedures

These are selected patterns from `results/patterns.txt`. Each item shows its cluster's top
abstracted members.

- Leave-one-repo-out → click:
  - `{python -m pytest tests/·.py::· -xvs}` → the same. Re-running the same test: held out,
    22/28 sessions follow within 8, base rate 0.27.
  - `{read tests/·.py}` → `{read tests/·.py}` → `{edit tests/·.py}`: 19/26.
  - `{python -m pytest … FAILED}` → `{pip install pytest -q}`. This is the harm-class
    system-Python install the learner diagnosis warns about.
  - `{git commit -m …}` → `{git log -1}`: 17/19.
  - `{python3 << 'EOF' FAILED}` → `{python3 << 'EOF'}`. This is a repro-script retry.
- Grown → attrs:
  - `{git status}` → `{git diff src/attr/·.py}`;
  - `{find … validators}` → `{read src/attr/validators.py}` → `{read tests/test_validators.py}` →
    `{edit …}` → `{edit …}`. This is a task-area walk that does not generalise beyond that area.
- Transfer to urfave-cli (Go) and vue-core (TS): only the git housekeeping patterns match.

**The procedure the vision hoped for** (edit → scoped test → fix → full test) is never selected.
Measured with a hand-written instrument, it does occur in held-out sessions:
- edit → scoped test in 76%;
- edit → scoped test → edit → full test in 14%.

Its test commands vary in the part that carries meaning (the target), and agents already run it
unprompted, so there is nothing to direct. Median calls between prefix and suffix is **0**: the
patterns describe adjacency, not something far ahead to skip to.

### 3. Usefulness and the value of compiling a pattern into a delivered step

Grown memory, net calls per session; leave-one-repo-out is similar (`results/tables.txt`).

| alphabet | P(suffix follows prefix) [lift] | directives/session | useful | net: failed | net: failed + discovery | skip-ahead bound (all calls in window) | prefix ignored, same bound | compiles to one command |
|---|---|---:|---|---|---|---|---|---|
| full (exact) | 0.30 [2.9×] | 4.3 | 0.45 | −0.81 [−1.02, −0.58] | −0.80 [−1.01, −0.56] | +2.35 | +9.12 | 200/332 |
| emb τ=0.8 | 0.42 [2.5×] | 3.8 | 0.61 | −0.41 [−0.57, −0.27] | −0.38 [−0.55, −0.22] | +2.07 | +10.85 | 56/282 |
| emb τ=0.7 | 0.46 [1.8×] | 4.5 | 0.71 | −0.49 [−0.64, −0.34] | −0.44 [−0.60, −0.28] | +2.81 | +13.50 | 22/248 |
| emb τ=0.6 | 0.51 [1.35×] | 2.7 | 0.86 | +0.18 [−0.04, 0.42] | +0.23 [0.01, 0.48] | +3.41 | +10.79 | 9/129 |
| Heuristics Miner edges, τ=0.8 | 0.59 [2.3×] | – | – | – | −0.07 [−0.25, 0.11] | +4.64 | – | – |

- **The "skip-ahead value" is an artefact.** Delivering the same suffixes with the prefix
  ignored scores 4–5 times *more* on the all-calls bound. That bound measures only how far away
  the suffix is, not what the pattern knows. The honest tiers (failed and discovery calls that a
  directive would remove) are ≤ 0.
- **Similarity makes patterns uncompilable.** At τ=0.8 only 20% of the delivered suffixes have a
  dominant literal, and at τ=0.7 only 9%. A cluster of 200 `pytest` spellings cannot be sent as
  one command without the slot machinery that the grammar lab and the learner diagnosis already
  built by other means.

### 4. Process mining

| alphabet | HM edges per fold | P(b follows a) | net calls (failed + discovery) | Inductive Miner: activity mass under flower models |
|---|---:|---|---|---:|
| key | 10.9 | 0.65 [0.63, 0.67] | −0.24 [−0.44, −0.03] | 0.74 |
| emb τ=0.8 | 8.2 | 0.59 [0.56, 0.62] | −0.07 [−0.25, 0.11] | 0.63 |
| emb τ=0.7 | 8.8 | 0.59 [0.56, 0.62] | +0.02 [−0.19, 0.23] | 0.55 |

- **Heuristics Miner.** Its strong dependencies are self-loops (`read core.py → read core.py`,
  `pytest X → pytest X`) and `git status → git diff`.
- **Inductive Miner.** It returns a top-level choice among dozens of task-specific branches, with
  55–74% of activity mass inside flowers, which allow any order and so say nothing. The textbook
  "spaghetti process" outcome.

### 5. Common harness (the North-star metrics)

Test set `6a795a362ccb`: 158 Haiku held-out control sessions and 38 tasks, baseline $0.260 and
32.3 calls per task. Policy: τ=0.8, MDL patterns, and a `run` directive only where the suffix
compiles.

| protocol | directives | $ saved/task [90% CI] | share of $ | calls saved/task [90% CI] |
|---|---:|---|---|---|
| split | 10 | −$0.0003 [−0.0008, −0.0000] | −0.1% [−0.3, −0.0] | −0.04 [−0.09, −0.00] |
| leave-task-out | 62 | −$0.0007 [−0.0014, −0.0001] | −0.3% [−0.6, −0.0] | −0.07 [−0.16, +0.00] |
| leave-task-out, p_follow 0.5 | 62 | −$0.0004 [−0.0008, −0.0001] | −0.2% [−0.3, −0.0] | −0.04 [−0.08, +0.00] |
| **leave-one-repo-out** | 79 | −$0.0013 [−0.0023, −0.0003] | −0.5% [−0.9, −0.1] | −0.16 [−0.34, −0.01] |
| sensitivity: every pattern delivered, Sat = learned cluster (`custom`, flagged), split | 57 | −$0.0019 | −0.7% [−1.3, −0.3] | −0.23 |
| sensitivity: same, leave-task-out | 552 | −$0.0176 | **−6.8%** [−8.5, −5.1] | −2.17 [−2.61, −1.70] |
| sensitivity: same, leave-task-out, p_follow 0.5 | 552 | −$0.0092 | −3.5% [−4.4, −2.7] | −1.11 [−1.34, −0.88] |
| sensitivity: same, leave-one-repo-out | – | −$0.0154 | −5.9% [−7.7, −4.3] | −1.96 [−2.47, −1.49] |

- The `run` rows use the harness's uncalibrated `command` class. Their Ŝ(Rust) is +0.07
  (leave-task-out) and +0.15 (leave-one-repo-out) before the misfire charge.
- Delivering *every* pattern (the `custom` rows) makes it clearly worse: 1.1 directives per 10
  calls, 38% misfire.
- **U11 for reference:** +1.00 calls, +1.9% of $ post-hoc.
- **Leakage.** `harness.leakage.scan_memory` over the learner's stored texts and directives
  (click and vue-core split fits) finds nothing.

## Comparison with the Rust G engine (`crates/adage-graph`, read-only)

**Same family.** `mine/seq.rs` is target-rooted, gap-bounded (G = 8) PrefixSpan with BIDE closure,
and `select.rs` is SQS MDL. The Python here is the same algorithm class, so the comparison isolates
**the node alphabet**. Rust's prototypes (`proto.rs`) are embeddings plus a feature hash, but
clustering is *blocked* by effect class and program, and *cannot-linked* across HeadSub identities.
Similarity therefore only merges spellings of one command. Its gates (contrast, V > 0,
dominant/covering variant, ≥ 2 tasks) come after selection.

**Result.** On the RepoBench split both engines keep nothing:
- Rust: 0 routes at default and demo (`evals/lab/results/engine.json`);
- here: 0–1 MDL pattern per repository at every τ.

With more data, unblocked similarity (τ 0.7–0.8) raises compression 2–5 times over exact labels.
The extra patterns are habits that fail Rust's contrast and value gates by design, and the harness
confirms those gates are right (§3, §5). Making Rust's prototypes looser would add candidates, not
value.

**One transferable idea.** *Abstraction by recurrence* (mask tokens seen in < 2 tasks or
repositories) is a cheap, learned alternative to hand-written slot regexes for labels. It is the
reason leave-one-repo-out compression reaches 5–7% here. It is a labeling improvement for
diagnostics (`adage graph`), not a source of savings.

## Verdict and what to build

- **Reject** similarity-tolerant subgraph mining as the source of reflexes. The evidence holds on
  every protocol and alphabet:
  - common structure in agent traces is habit;
  - its delivery removes nothing;
  - its compiled form is either a literal (exact mining, which finds nothing) or a cluster that
    cannot be expressed as one command;
  - the only lever class it could serve (skip wasted calls) has a ~10% ceiling.
- **Build nothing new in Rust for this.** Keep `adage-graph`'s conservative gates.
- If `adage graph --learned` needs readable prototypes, port the recurrence mask (≈ 30 lines in
  `proto.rs` labeling). Expected value on $ and calls: 0.
- **Where the value is instead:** the productive path and the carried context (output caps,
  knowledge that changes *which* files and tests the agent touches). These are outside what
  frequent-structure mining can see.

## Hard-coded parts (honest list)

- `evals/lab/traces.py` parsing, and `evals/lab/normalize.py`'s `text` rendering (`run <cmd>`,
  sandbox root stripped) as the embedding input. Its key and slot levels are used only as the
  `full` and `key` baselines.
- The discovery tier of the replay uses the lab's hand-written `metrics.discovery` regex and
  `normalize.EXPLORE_*` lists, **measurement only**.
- The §2 procedure-frequency check used a hand-written test-command regex, **measurement only**.
- Parameters were fixed a priori: G = 8, maximum length 5, support ≥ max(3, 10%), the 80%
  dominant-variant rule, and the replay's 0.8/0.1/0.3 constants. τ was reported over a grid, and
  0.8 was used for the harness policy without tuning on the test set.

## Reproduce

Run from this directory with `OMP_NUM_THREADS=1 nice -n 10`:

```sh
python3 data.py                    # cache sessions -> /tmp/adage-research/approx-subgraph
python3 run.py chrono && python3 run.py grown && python3 run.py loro   # ~20 CPU-min total
python3 run_pm.py                  # process mining
python3 report.py                  # tables; results/tables.txt
python3 show_patterns.py           # results/patterns.txt
python3 harness_policy.py          # harness drivers -> results/harness-tau0.8.json
CUSTOM=1 python3 harness_policy.py # flagged sensitivity (results/harness-tau0.8-custom.json)
# ADVICE=1 (advice text for uncompilable suffixes) was stopped unfinished: advice is costed, never credited.
```
