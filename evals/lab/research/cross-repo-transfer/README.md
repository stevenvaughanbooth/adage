# Cross-repository transfer: what a new repository gets on day one

Slug `cross-repo-transfer`, 2026-09-29. Offline, $0. Scored with the common harness
(`evals/lab/research/harness`).

## Question

Someone installs Adage on a repository with no history. What can we deliver on day one that was
learned from other repositories' traces, with no hand-written instinct? How quickly does learning
from the repository's own sessions overtake it? The key proof point is whether the learner can
rediscover the U11 `.venv` instinct without it being written by hand.

## Answer

- **Yes, U11 is rediscovered.** Leave-one-repository-out, with no hand rule, no tool-name list
  and no error-string list, the learner finds the same reflex in all three Python repositories:
  "`python -m pytest` / `python3 -m pytest` / `bin/pytest` dead end → `source .venv/bin/activate
  && python -m pytest`". That includes marshmallow, where **no training session ever used the
  `.venv`**. It learned the reflex from click and attrs agents who did.
- **Day-one value, Haiku held-out (158 sessions, 38 tasks, test set `6a795a362ccb`).** The learned
  predictive `PreToolUse` rewrite saves **+1.45 calls/task [90% CI 1.08, 1.81]** and
  **+$0.0078/task [0.0058, 0.0097]**, which is **+3.0% of $ [2.2, 3.9]**. For comparison:
  - The hand-written U11 post-hoc gets +0.88 calls / 1.9%. Learned beats it by **+0.57 calls
    [0.39, 0.74]** (paired, same sessions).
  - The hand-written U11 predictive, trained on the repository's own sessions, gets +1.63 calls
    / 3.5%. Learned is **−0.19 calls [−0.36, −0.02]** behind it (89% of U11) with zero own data.
  - The harness's U11 predictive learner under the same leave-one-repository-out protocol gets
    +0.88 calls.
- **Own sessions help once, then flatten.** With the hierarchical prior plus the repository's
  first n sessions: n = 0: 1.45; **n = 1: 1.62** [1.22, 2.01] (this equals U11 predictive); n = 3:
  1.60; n = 10: 1.61.
- **Repo-local learning never overtakes the prior.** A learner that uses only the repository's
  own sessions delivers **0.00 calls at n = 1, 3 and 10**. Ten sessions hold at most three `.venv`
  first attempts, split across two spellings. That is below any evidence gate, and in marshmallow
  there are none at all. Here the pooled prior *is* the product. Own data only calibrates when to
  fire.
- **What else transfers: almost nothing, and the data is too thin to say more.** urfave-cli (Go)
  and vue-core (pnpm monorepo) get **0**, because no other repository shares their ecosystem.
  Both synthetic repositories get 0 as well.
  - The learned dead-end detector finds 94–100% of the environment-signature calls in the Python
    repositories, including 75–100% of the exit-0 piped ones the harness cannot flag.
  - It finds 0 of 26 in vue-core, and its AUC is 0.50 on urfave-cli.
  - The repro-script variant (`python3 <<EOF` + `No module named 'click'`, launch bug 5) is not
    reliably transferred.
- **This is not the 15–20% lever.** Its class of calls, environment dead ends, has an oracle
  ceiling of about 8–10% of $ (harness `oracle.json`). What transfer changes is who pays for the
  reflex: it is learned from other repositories instead of being written by hand.

**So what.** Ship it as the mechanism that replaces hand-written instincts. It does not add
savings on top of them. On Python repositories with a `.venv` it is expected to be worth about
**+1.5 calls and 3% of $ per task** on day one (90% CI 2.2–3.9% of $). It reaches parity with U11
(3.4%) after one session in the repository. Pass rate is not estimable by replay, and the
worst-case bound is 0 harmful directives. Under p_follow = 0.5 it is +0.72 calls, 1.4% of $.

**Confidence that it survives a live A/B: moderate to high on the Python `.venv` slice.** The
mechanism and the directed action are the same as live-validated U11: 41/41 followed, and −1.85
calls from dead end to working test run. The replay already reproduces that. The confidence is
**low for any other ecosystem**: the value there depends entirely on having source repositories
in the pool.

## Method (principle 1: what is learned and what is not)

`data.py` loads the traces. `abstract.py` builds the action abstraction. `learn.py` holds the
learner. `run.py` runs the learned-instrument evaluation and the learning curve.
`harness_eval.py` scores with the harness. `diag.py` checks the instruments. `poolsize.py`
measures how the result depends on the pool.

1. **Repository fingerprint and slot binding.**
   - **Data.** Each real repository's file tree, to depth 3, comes from the zero-config bind
     checkout (`private/bind-sandbox/repo`). It includes `.venv/bin/*` and `node_modules/.bin/*`.
   - **Shared vocabulary.** A path component is shared vocabulary when it appears in the trees of
     at least 2 repositories: `.venv`, `bin`, `python`, `tests`, `src`, `node_modules`. Any other
     component becomes `*`. So `src/click/core.py` becomes `src/*/*.py`, and `.venv/bin/python`
     stays literal.
   - **Binding.** A candidate command binds in a new repository only if its literal paths exist in
     that repository's tree. `.venv/bin/activate` binds in attrs, click and marshmallow; it does
     not bind in vue-core.
   - **Fingerprint weighting.** Repositories were also weighted by TF-IDF similarity of their
     trees (`hier` vs `hier_u`). It made no difference with 5 repositories: binding does the
     work.
2. **Actions (learned).**
   - **Main command.** Commands are split into simple commands with a shell parser. The main
     command is the one whose program is most often run *alone* in the corpus, so `cd`, `source`,
     `head` and `tail` are learned to be wrappers.
   - **Head.** The head is the longest argv prefix whose next token is a constant across at least
     2 repositories. This is a variable-order prefix tree, as in Drain's constant positions. It
     yields `python -m pytest`, `npm test --` and `go test`. Environment prefixes chained with
     `&&` are kept: `source .venv/bin/activate && python -m pytest`.
3. **Intents: learned action equivalence.**
   - **Vectors.** Each head is represented by the mean of the potion-base-8M embedding of its
     text and the embedding of its outputs.
   - **Clustering.** Average-linkage cosine clustering at τ = 0.25.
   - **Result.** `python -m pytest`, `.venv/bin/pytest` and `source .venv/bin/activate && python
     -m pytest` form one intent. `pip install …` is a different intent, because its outputs look
     like an installer's.
4. **Dead ends: learned success and failure.**
   - **Behavioural label.** A call is labelled a dead end when the agent re-attempts the same
     intent within 8 calls, with no file edited in between and mostly the same arguments, and
     changes either the head or something else in between.
   - **Classifier.** A logistic regression predicts that label from the output text, using word
     1–2-grams seen in at least 2 repositories, plus `is_error`.
   - **What it learned, unprompted.** `no module named`, `ModuleNotFoundError`, `no tests ran`
     and `python: no` mark dead ends. `assert`, `AssertionError` and `failed` mark *productive*
     calls: a failing test run is progress.
5. **Hierarchical empirical Bayes** (`outcome-optimisation.md` §2.3).
   - **Unit.** For each (repository, head), each session's first attempt is either productive or
     a dead end.
   - **Prior.** Rates are pooled across the other repositories as Beta(κm, κ(1−m)), with κ fitted
     by method of moments and clipped to [1, 50]. A head the pool never ran gets its intent's
     pooled rate.
   - **Posterior.** The prior is updated with the target repository's own first attempts.
   - **Directive.** For a head h0 in intent K, the directive is the head c of the same intent that
     meets all of these:
     - it binds in the target repository;
     - it runs the same *subject*;
     - its posterior mean is at least 0.7;
     - its 10% posterior quantile is at least 0.5, over at least 3 trials (SPIBB-style);
     - it beats h0 by at least 0.2.
   - **Delivery.**
     - *Post*: after a call the learned detector scores as a dead end.
     - *Pre* (predictive, no hindsight): at `PreToolUse`, when h0's posterior P(productive) is
       below 0.5, rewrite the call to c followed by the agent's own arguments.
   - **Arms.** The learner never sees the `adage` or `agents-md` arms. They were steered by U11
     or by a hand-written AGENTS.md, and learning from them would smuggle the hand rule in.

## Held-out results (harness; Haiku 4.5, 158 held-out control sessions, 38 tasks, test set `6a795a362ccb`; 90% task-clustered CIs)

Baseline: $0.260 and 32.3 calls per task. `results/harness.json` has everything, including
`to_json()` for each report.

| policy | own sessions | calls saved/task [90% CI] | $ saved/task [90% CI] | share of $ [90% CI] | directives | useful LB90 | harmful |
| --- | ---: | --- | --- | --- | ---: | ---: | ---: |
| U11 post-hoc (hand-written, as live) | – | +0.88 [0.58, 1.19] | +0.0049 [0.0033, 0.0065] | 1.9% [1.2, 2.7] | 62 | 0.93 | 0 |
| U11 predictive (hand-written; spellings from own training) | 10 | +1.63 [1.22, 2.04] | +0.0091 [0.0068, 0.0114] | 3.5% [2.5, 4.6] | 98 | 0.90 | 0 |
| **learned transfer, LORO day 0, pre** | **0** | **+1.45 [1.08, 1.81]** | **+0.0078 [0.0058, 0.0097]** | **3.0% [2.2, 3.9]** | 103 | 0.88 | 0 |
| learned transfer, LORO day 0, post | 0 | +0.93 [0.66, 1.22] | +0.0051 [0.0036, 0.0066] | 2.0% [1.3, 2.7] | 99 | 0.87 | 0 |
| … pre at p_follow = 0.5 | 0 | +0.72 [0.54, 0.89] | +0.0038 [0.0028, 0.0047] | 1.4% [1.0, 1.9] | | | |
| … pre, Haiku + Sonnet (190 sessions, 54 tasks, test set `731472b94a1f`) | 0 | +1.31 [1.00, 1.61] | +0.0075 [0.0057, 0.0092] | 2.4% [1.7, 3.1] | 121 | 0.88 | 0 |
| hierarchical (other repos + own), pre | 1 | +1.62 [1.22, 2.01] | +0.0089 [0.0067, 0.0111] | 3.4% [2.5, 4.4] | 109 | 0.89 | 0 |
| hierarchical, pre | 3 | +1.60 [1.21, 1.99] | +0.0086 [0.0065, 0.0107] | 3.3% [2.4, 4.3] | 113 | 0.87 | 0 |
| hierarchical, pre | 10 | +1.61 [1.22, 1.99] | +0.0088 [0.0067, 0.0110] | 3.4% [2.4, 4.4] | 103 | 0.87 | 0 |
| own repository only (same learner, no pool), pre | 1 / 3 / 10 | 0.00 / 0.00 / 0.00 | 0 | 0% | 0 | – | – |

Paired differences on the same 158 sessions (task-clustered, 90%):

| comparison | calls/task | $/task |
| --- | --- | --- |
| learned LORO pre − U11 predictive (own training) | −0.19 [−0.36, −0.02] | −0.0013 [−0.0026, −0.0001] |
| learned LORO post − U11 post-hoc | +0.05 [−0.08, +0.17] | +0.0002 [−0.0006, +0.0010] |
| learned LORO pre − U11 post-hoc | +0.57 [+0.39, +0.74] | +0.0029 [+0.0018, +0.0040] |

Per held-out repository (day 0, pre; harness):

| repo | calls/task [90% CI] | share of $ | U11 predictive (own data) |
| --- | --- | ---: | --- |
| attrs | +2.68 [1.83, 3.50] | 7.0% | +2.70 [1.81, 3.57] |
| click | +1.65 [1.35, 1.92] | 3.5% | +1.48 [1.16, 1.78] |
| marshmallow | +1.86 [1.39, 2.32] | 4.5% | +3.02 [2.48, 3.52] |
| urfave-cli (Go) | 0 | 0 | 0 |
| vue-core (pnpm) | 0 | 0 | 0 |

At n = 1, marshmallow rises from 1.86 to 2.88. One own session shows that `python3 -m pytest`
also dead-ends there, and the pool had rated it productive.

**Cross-check with the learned instrument (`run.py`, `results/tables.md`).** Here episodes, dead
ends and verification come from the learner itself, not from the harness's hand-written Sat/W.
The measurement learner is fixed per held-out repository. A directive counts as useful only if c
is **verified** to work in that repository: productive in at least 50% of its runs across all of
that repository's sessions. A directive that fails, or that reaches another intent, is a misfire
and costs −1 call.
- **Pooled real repositories, day 0.** Learned pre +1.94 calls/session [1.47, 2.39] vs U11 pre
  +1.70 [1.29, 2.08]. Learned post +1.47 vs U11 post +1.23.
- **Ceiling.** The oracle "every recovered dead end removed" is +5.69 (17% of calls, in this
  broader Bash+failed window).
- **At n = 10** the pre variant drops on attrs (3.13 → 2.26). Attrs' own training traces show
  `python -m pytest` succeeding on first attempt after agents `pip install`ed pytest into the
  system Python. That is confounding by environment mutation, and it can switch the predictive
  trigger off. The harness curve does not show the drop (1.61 at n = 10); the two instruments
  differ.

## Proof point: U11 rediscovered, and what else was found (day 0, `results/tables.md`)

| held-out repo | dead-end head | learned replacement | prior P(productive), pool trials | verified in held-out repo |
| --- | --- | --- | --- | --- |
| attrs | `python -m pytest`, `python3 -m pytest`, `bin/pytest` | `source .venv/bin/activate && python -m pytest` | 1.00, 12 | 17/19 runs productive |
| click | `python -m pytest`, `python3 -m pytest`, `.venv/bin/pytest`, `bin/pytest` | `source .venv/bin/activate && python -m pytest` | 1.00, 4 | 112/126 |
| marshmallow | `python -m pytest`, `python3 -m pytest` | `source .venv/bin/activate && python -m pytest` | 1.00, 12 | 11/11 (**no marshmallow training session used `.venv`**) |
| click | `bin/python` (a repro script via system Python) | `source .venv/bin/activate && python3` | 0.83, 6 | 36/36 |
| marshmallow | `source .venv/bin/activate && python3` | `.venv/bin/python` | 0.80, 7 | 21/23 (a no-op rewrite: both are the venv) |
| click | `pip install -e && python` | `pip install -e && python3` | 0.73, 15 | 5/5 (noise) |

Four failure modes appeared and were fixed while developing:
- Rewrites to a coarser spelling of the same command (`pip install -e .` → `pip install`).
  Fixed by the granularity rule.
- Off-intent swaps (`pip install pytest && python -m pytest` → `pip install -e .`), which the
  harness's "other command" class would have credited. Fixed by the same-subject rule.
- Own-data noise with 2–3 trials. Fixed by the 10%-quantile gate.
- Test-run output with failures looking like a dead end. The behavioural label already treats
  `failed` / `AssertionError` as productive.

Two more checks:
- **Dead-end detector** (`diag.py`, day 0, measured against the replay's hand-written signatures
  as an instrument only):

  | repo | AUC vs its own label | signature recall | exit-0 piped signature recall |
  | --- | ---: | ---: | ---: |
  | attrs | 0.79 | 0.94 | 0.75 |
  | click | 0.69 | 0.97 | 0.97 |
  | marshmallow | 0.84 | 1.00 | 1.00 |
  | vue-core | 0.54 | 0.00 (26 calls) | – |
  | urfave-cli | 0.50 | no signature calls | – |

- **Pool dependence** (`poolsize.py`, day 0, learned instrument, pre):

  | target | source pool | calls/session |
  | --- | --- | --- |
  | marshmallow | {attrs} | 3.76 |
  | marshmallow | {click} | 2.43 |
  | marshmallow | {attrs, click} | 3.76 |
  | click | {attrs} | 2.02 |
  | click | {marshmallow} | 1.50 |
  | attrs | {click} | 2.26 |
  | attrs | **{marshmallow}** | **−0.01** |
  | any | {gen-py-clinic} | 0 |

  One source repository is enough, but only if its agents *did* the right thing at least 3
  times. Marshmallow's agents never used the `.venv`, so it teaches attrs nothing. The synthetic
  Python repository has no `.venv`, so it teaches nothing either.

## How much data the pooled approach needs (for the data-expansion agent)

- **What transfer needs.** A new repository benefits only when the pool contains at least one
  repository of the same *friction family*, ecosystem × environment layout (`.venv`,
  `node_modules`/pnpm, go, cargo, uv, poetry, tox…), in which agents took the working action on
  first attempt at least 3 times. Here that held for 3 of 5 real repositories (the Python/.venv
  ones) and failed for Go and the pnpm monorepo: they had no neighbour.
- **Coverage model.** Suppose friction families follow a Zipf(1) distribution over 20–30 common
  families. The probability that a new repository's family has at least 1 / at least 2 source
  repositories in a pool of N is:

  | N | ≥ 1 source | ≥ 2 sources |
  | ---: | --- | --- |
  | 5 | 0.35–0.42 | 0.12–0.15 |
  | 10 | 0.51–0.58 | 0.27–0.33 |
  | 20 | 0.66–0.73 | 0.45–0.52 |
  | 40 | 0.79–0.86 | 0.61–0.70 |
  | 80 | 0.90–0.95 | 0.77–0.85 |
  | 160 | 0.97–0.99 | 0.90–0.96 |

  Our 5 repositories (3/5 covered) sit on this curve.
- **About a third of sources don't count.** Roughly 1 in 3 same-family sources contributes
  nothing, as with marshmallow's agents never using the `.venv`.
- **Estimate.** About **40–60 repositories** (about 10 sessions each, `train` + `control` arms,
  no steering) for day-0 transfer on about 80% of new repositories. About **120–160** for 90%.
  With 5 repositories the answer is "Python/.venv only".
- **Diversity, not depth.** Own sessions are worth +0.17 calls once, at n = 1, and nothing after.
  Extra sessions in click/attrs/marshmallow add nothing to transfer. New families add all of it.
  - Priority families: Node npm/pnpm/yarn workspaces, Go, Rust/cargo, uv/poetry/tox Python, and
    Makefile-driven repositories.
  - Aim for 2–3 repositories each, with fresh checkouts that have the environment set up, as the
    bind sandbox did.

## Ceiling

- **This lever's class.** Environment dead ends. The harness's oracle, deleting every
  environment-friction call, is 3.83 calls and **8.4% of $** [6.6, 10.4] on the 82 v2 Haiku
  sessions. Deleting every failed call is 10.3%.
- **What transfer reaches.** Day 0 reaches 3.0%, about a third of that ceiling. With one own
  session it reaches 3.4%, the same as U11 predictive.
- **Other ecosystems.** Even perfect transfer to them only spreads the same ~2-call reflex to
  more repositories. It does not raise the per-task share.

## Hard-coded parts (honest list)

- **Shell grammar** (`shlex` operators, heredoc, redirection) is a parser, not domain knowledge.
- **`WRITE_TOOLS = {Edit, Write, MultiEdit, NotebookEdit}`** is used only to say "no file changed
  between two attempts" in the behavioural dead-end label.
- **The leading `Exit code N` line** is stripped from outputs, because that is the harness's
  framing; `is_error` carries it.
- **Candidate rules:**
  - *Granularity*: a rewrite is not a prefix of the original.
  - *Same subject*: the basename of the head's last token, trailing version digits dropped, must
    match, so `python3` ≡ `python`.
- **Thresholds:** horizon 8, Jaccard 0.5, τ 0.25, P ≥ 0.7, 10% quantile ≥ 0.5, gain ≥ 0.2, at
  least 3 trials, "shared" = at least 2 repositories.
- **Protocol:** the `adage` and `agents-md` arms are excluded from learning. The tree snapshot is
  the bind checkout at depth 3.
- **Measurement only:** the harness's Sat/W (the hand-written Rust taxonomy), and `diag.py`'s
  signature regex. The U11 baseline is hand-written by definition.
- **Not used anywhere in the learner:** tool-name taxonomies, error strings, `.venv`, `pytest`,
  or any list of programs.

## What to build (Rust)

Replace the hand-written instinct *content* with a **pack of cross-repository priors**. The
shipping mechanism (bind, rewrite at `PreToolUse`) stays as it is.

1. **At pack-build time (offline, in `evals/`).** For each (learned intent, head), store
   first-attempt sufficient statistics pooled over the corpus: s, n, κ and the intent rate. Keep
   the head templates with tree-grounded literals (`source .venv/bin/activate && python -m
   pytest`). No commands from task content are stored.
2. **At bind time (`adage-instincts::bind`).** Keep a candidate only if its literal paths exist in
   the checkout. This is the existing binder, fed by data instead of `catalogue.toml` entries.
3. **At learn time (`adage-graph` score).** Compute the posterior from the prior and the
   repository's own first attempts, and gate on the 10% quantile ≥ 0.5 over at least 3 trials.
   This is the Beta backoff that `score.rs` already has, with the parent set to "the same head in
   other repositories" and κ fitted.
4. **At hook time.** Rewrite at `PreToolUse` when the posterior of the head being run is below
   0.5 and a gated candidate exists. It is an O(1) table lookup on (repo, head).

Expected value on Python/.venv repositories:
- On day 0 it is at parity with U11 predictive, +1.45 to +1.6 calls and about 3–3.4% of $ per
  task, **with no hand-written instinct**.
- Over hand-written U11, the incremental value is 0 to +0.2 calls.
- The real value is coverage: every new friction family the corpus contains becomes a reflex
  without anyone writing it. How much that is worth depends on the data-expansion corpus (see
  above). The learned-intent clustering and the dead-end detector can run offline in the
  pack-building script. The runtime only needs the head abstraction and the table.

## Reproduce

```sh
cd evals/lab/research/cross-repo-transfer
OMP_NUM_THREADS=1 nice -n 10 python3 data.py            # cache sessions (~6 s)
OMP_NUM_THREADS=1 nice -n 10 python3 harness_eval.py    # headline: U11, LORO, p_follow 0.5, curves (~25 min)
OMP_NUM_THREADS=1 nice -n 10 python3 run.py             # learned-instrument LORO + curve (~10 min)
OMP_NUM_THREADS=1 nice -n 10 python3 diag.py            # detector and intent checks
OMP_NUM_THREADS=1 nice -n 10 python3 poolsize.py        # 1- and 2-source pools
python3 report.py                                        # results/tables.md
```

`harness.leakage.scan_memory` over the stored heads (131 in the marshmallow fold) finds nothing, and `assert_clean` runs in every fold.

The paired differences are printed by `harness_eval.py`: see `logs/harness-loro.log` under
`/tmp/adage-research/cross-repo-transfer`. Caches are kept under that directory too (about
20 MB). Models are not committed; `ADAGE_LAB_MODELS` supplies potion-base-8M.
