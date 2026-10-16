# Context prefetch: tell the agent which files matter, at the first prompt

2026-09-29. Research sprint, slug `context-prefetch`. Offline, $0. Nothing here touches `crates/`.

## The question

The hypothesis was that orientation is the biggest waste: agents spend many calls finding the right
files. If Adage could say "these are the files that mattered for tasks like this" at the first
prompt, the agent would skip most of that exploration. So: can a learned retriever find those
files, and what would that be worth in **$ per task, calls per task and pass rate** on held-out
work?

## Verdict: not the lever. Retrieval works; the money isn't there

- **Retrieval works and transfers to new repositories.** A fusion of BM25 and potion-code-16M-v2
  chunk embeddings, with weights trained on the other repositories, puts 70% of the files the agent
  will edit in its top 5 and 89% in its top 10 (held-out sessions). That holds on a repository it
  has never seen. Past-trace memory (kNN) and git co-change add almost nothing.
- **The oracle ceiling for path hints is below the North star.** Assume a perfect hint that lists
  every file that mattered, perfectly followed, and repeated after each file is opened so that
  every search for every target counts. It saves **11.9% of $ [10.2, 13.4]** and 4.2 calls per
  task (harness, all 190 held-out sessions). Hinted only once at the prompt, the same oracle saves
  **3.6–4.2%**, because the agent opens the first right file after about 1.6 calls anyway.
- **The shippable number is about U11's size.** The learned top-5 hint at the prompt saves
  **+4.0% of $ [3.2, 4.8], +1.62 calls [1.28, 1.98]** per task on held-out Haiku with p_follow = 1.
  At p_follow = 0.5 it is **+1.9% [1.5, 2.3]**. Leave-one-repository-out gives the same result
  (+4.0% [3.1, 4.8]).
- **The harness does not charge its harm.** A listed file the agent didn't need, if the agent reads
  it whole, costs about 6 times a hunting call. The whole file is carried for the rest of the
  session. The break-even misfire rate is **6–7%** of wrong listed files read (p_follow 0.5),
  or 13% (p_follow 1). Nobody has measured it.
- **Pre-loading file content loses money.** Even the oracle's own files injected at the prompt cost
  −36% of $. This agrees with waste-anatomy.
- **Targeted ranges are no better.** Shrinking the agent's whole-file reads of the right files to
  the lines that matter has an oracle ceiling of **10.3% of $ [9.1, 11.7]**. The window around the
  edit is ±30 lines, assumed to be enough. But retrieval picks a window that covers the edit only
  28–52% of the time, which nets **+1.8% [0.6, 3.0]**.
- **The pass-rate upside is small and not measurable here.** 15 of 190 held-out sessions (7.9%)
  never edited a gold source file, and none of them passed. The fused top 5 contained the gold file
  in 93% of those sessions. But most of those sessions made no edit at all. They ran out of turns,
  which is not a localisation failure.

**So what.** Shipped as a one-shot "likely relevant files" hint at `UserPromptSubmit`, prefetch
would save about **+1.9% of $ [1.5, 2.3] and +0.8 calls per task** at p_follow 0.5, or +4.0% and
+1.6 calls if always followed. That is before charging misfire reads, which can flip the sign if the
agent opens more than about 1 in 15 of the wrong listed files. The pass-rate effect is unmeasurable
offline. Confidence that a positive effect survives a live A/B: **low**. Build nothing now beyond
a micro-slice (below).

## Data

- **Sessions:** the common harness's corpus (`harness.load()`: 519 sessions, which excludes the 35
  discarded-run transcripts). Held-out sets:
  - 158 held-out Haiku control sessions on 38 test tasks (test set `6a795a362ccb`);
  - 190 sessions / 54 task-slices with Sonnet (`731472b94a1f`).
  - Train: the 47 training-split sessions.
- **Repository at each task's base commit:** from the upstream mirrors in
  `/tmp/adage-repobench/mirrors` (`git ls-tree` + `cat-file`), never the agent's edited working
  tree. Repositories are small: 97 files (marshmallow) to 703 (vue-core).
- **Git history:** the last 2,000 non-merge commits reachable from the base, so the task's own
  commit is never included. Commits touching more than 40 files are skipped, as in
  `adage-learn/testmap.rs`.
- **Models:** potion-base-8M, potion-base-32M (`fetch_models.sh`) and **potion-code-16M-v2**. The
  last came from npm `zvec-grep-potion-code-16m-v2-model@0.1.0`, MIT. Its SHA-256s match the
  manifest of HF `minishlab/potion-code-16M-v2` rev `e9d2a44`.
  - Cached at `/tmp/adage-research/context-prefetch/pc16`.

## What "mattered" means (the labels; hand-written measurement instruments)

- **E**: files the session edited that exist at base. On average 1.7 per session; 0.36 new files
  per session can't be prefetched.
- **N** (necessary reads): files this session opened before its last edit that at least half of the
  *other* sessions on the same task also opened. Independent agents converging on a file means it
  mattered. On average 1.3 per session.
- **T** = E ∪ N.
- **G**: the task's gold source files (from the commit).
- **Exploration calls**: Grep, Glob, LS, exploratory shell (`normalize.py`'s list), and Reads of
  non-target files.
  - Each one is **attributed to the next target that is opened for the first time after it**.
  - A hint that contains that target makes the call unnecessary.
  - Tier *search* counts only calls that opened no file. Tier *all* also counts dead-end reads.
  - `targets.py` holds all of this. It is a measurement instrument, not part of any learner.
- **Cost:** everything is priced with the harness's cumulative-context model
  (`harness/costmodel.py`, which reproduces billed totals).
  - My first cost model counted a removed response's cache write twice. That overstated the
    ceilings by about 1.6×. I dropped it.

## Method (retrievers; `retrieve.py`, `features.py`)

Every scorer sees only paths, contents, git history and past traces. None knows a language, a tool
or a file kind.

| scorer | what |
| --- | --- |
| `bm25` / `bm25c` | Okapi BM25 over identifier tokens (camel/snake split). Per file: path ×3 + content. Per 60-line chunk: max-pooled. |
| `p8`, `p32`, `code16` | model2vec static embeddings of 60-line chunks prefixed by the path. The score is the maximum cosine to the prompt. |
| `co_*` | One-step propagation of a content ranking over the co-change graph (commits touching both files, normalised by the square root of each file's commit count). `hot` is log(1 + commits). |
| `ref_content` | Propagation over "file mentions another file's stem", a language-agnostic stand-in for imports. |
| `knn` | Similarity-weighted vote of the files (E ∪ opened before the last edit) that the 5 most similar past sessions touched, by prompt embedding with code16. `prior` is their frequency. |
| `fusion` | Logistic regression over the z-score and 1/(1 + rank) of every scorer. It is trained on the training sessions' labels. `fusion_loro` is trained on the other repositories only, with no kNN. |

## Results

### Retrieval (instrument)

**Task level: all 188 RepoBench tasks.** The target is the gold source files, G, and G plus the
gold test files. The fusion is leave-one-repository-out. 90% bootstrap CIs over tasks.

| scorer | G@5 | G@10 | G+tests@5 | G+tests@10 |
| --- | --- | --- | --- | --- |
| fusion (LORO) | **0.62** [0.56, 0.67] | 0.75 [0.70, 0.79] | **0.65** [0.60, 0.68] | **0.81** [0.78, 0.84] |
| RRF(bm25, code16) | 0.61 [0.55, 0.66] | **0.77** [0.72, 0.81] | 0.63 | 0.80 |
| potion-code-16M-v2 | 0.55 [0.49, 0.60] | 0.68 | 0.61 | 0.76 |
| BM25 | 0.52 [0.47, 0.58] | 0.68 | 0.46 | 0.66 |
| potion-base-8M / 32M | 0.42 / 0.41 | 0.55 / 0.57 | 0.43 / 0.47 | 0.61 / 0.65 |
| co-change propagation | 0.34 | 0.51 | 0.37 | 0.51 |
| hot files (git) | 0.22 | 0.31 | 0.25 | 0.34 |
| random | 0.03 | 0.07 | 0.04 | 0.06 |

**Session level: 190 held-out control sessions.** Recall of E (edited), N (necessary reads) and
T = E ∪ N. 90% bootstrap CIs over sessions.

| scorer | E@5 | E@10 | N@5 | N@10 | T@5 | T@10 |
| --- | --- | --- | --- | --- | --- | --- |
| fusion (in-repo, with kNN) | **0.74** [0.69, 0.78] | 0.86 [0.83, 0.89] | **0.56** [0.51, 0.62] | 0.70 | **0.67** [0.62, 0.70] | 0.82 [0.79, 0.85] |
| fusion, leave-one-repo-out | 0.70 [0.65, 0.75] | **0.89** [0.86, 0.92] | 0.49 [0.43, 0.55] | 0.67 | 0.62 [0.58, 0.65] | **0.82** [0.79, 0.85] |
| fusion, grown memory (all other tasks' sessions) | 0.72 | 0.88 | 0.45 | **0.77** | 0.62 | 0.83 |
| potion-code-16M-v2 | 0.65 [0.60, 0.70] | 0.78 | 0.48 | 0.59 | 0.58 | 0.72 |
| BM25 | 0.50 | 0.74 | 0.41 | 0.48 | 0.47 | 0.66 |
| kNN, 10 training sessions per repo | 0.45 | 0.57 | 0.29 | 0.50 | 0.39 | 0.53 |
| kNN, grown memory | 0.61 | 0.63 | 0.55 | 0.63 | 0.57 | 0.61 |
| frequency prior over traces | 0.33 | 0.54 | 0.25 | 0.53 | 0.31 | 0.52 |
| co-change propagation | 0.36 | 0.62 | 0.34 | 0.52 | 0.37 | 0.60 |

- **Content does the work.** potion-code-16M-v2 is the best single scorer. It beats
  potion-base-8M/32M by about 0.13 at @5.
- **Traces add little.** In-repo kNN adds +0.05 T@5 over the LORO fusion. The learning curve over
  0, 1, 2, 5 and 10 training sessions per repository gives T@5 0.61, 0.60, 0.60, 0.65 and 0.67.
  Bug fixes don't recur in the same files, which confirms the literature agent's "trace prior
  hurts".
- **Co-change and reference propagation don't help** once content is in the fusion.
- **Recall transfers** to an unseen repository: LORO matches in-repo within the CIs.

### The target metrics

**Oracle ceilings** (`ceiling.py`, `harness_policy.py`). Everything is the share of session $
under the harness cost model, with p_follow = 1 and no compensation.

| hint containing the session's own targets T | Haiku (158) | all (190) |
| --- | --- | --- |
| harness `read_files`, once at the prompt | **+4.2%** [3.7, 4.6], 1.66 calls | +3.6% [3.0, 4.1], 1.60 calls |
| harness `read_files`, re-listed after each target is opened (per-target windows) | **+11.6%** [9.8, 13.4], 4.28 calls | **+11.9%** [10.2, 13.4], 4.18 calls |
| my attribution, search calls only | 10.8%, 3.7 calls | 11.1% |
| my attribution, plus dead-end reads | 13.4% [11.7, 15.1], 4.1 calls | 13.6% [11.9, 15.3] |
| every request before the first touch of a gold file (the literature agent's C, with carry) | 10.6% | 9.2% |
| **read batch**: the targets' full content at the prompt | **−34%** | −36% |
| harness reference: every search or listing deleted | 15.5% (82 v2 sessions) | |

**Learned hint, scored by the common harness** (`harness_policy.py`). This is a
`read_files` directive at the first `UserPromptSubmit`, listing the fused top k. Held-out control
sessions, 90% task-clustered CIs.

| policy (Haiku, 158 sessions / 38 tasks, `6a795a362ccb`; baseline $0.260, 32.3 calls) | p_follow | $ saved/task | share of $ | calls saved/task |
| --- | --- | --- | --- | --- |
| split, top-3 | 1.0 | +$0.0100 | +3.8% [3.1, 4.7] | +1.57 [1.21, 1.95] |
| **split, top-5** | 1.0 | +$0.0104 [0.0080, 0.0129] | **+4.0% [3.2, 4.8]** | **+1.62 [1.28, 1.98]** |
| **split, top-5** | 0.5 | +$0.0049 [0.0038, 0.0062] | **+1.9% [1.5, 2.3]** | +0.79 [0.62, 0.97] |
| split, top-10 | 1.0 | +$0.0102 | +3.9% [3.2, 4.6] | +1.56 |
| split, top-5, re-listed after each opened file | 1.0 | +$0.0112 | +4.3% [2.4, 6.3] | +1.70 [0.99, 2.51] |
| split, top-10, re-listed | 1.0 | +$0.0129 | +5.0% [3.0, 6.9] | +2.10 [1.37, 2.91] |
| **LORO, top-5** (a repository never seen) | 1.0 | +$0.0104 | **+4.0% [3.1, 4.8]** | +1.63 [1.25, 2.01] |
| LORO, top-5 | 0.5 | +$0.0049 | +1.9% [1.5, 2.3] | +0.80 |
| learning curve, top-5 re-listed, n = 1, 2, 4, 10 training sessions | 1.0 | | 3.9, 2.7, 2.9, 4.3% (flat) | |
| *U11 post-hoc (reference)* | 1.0 | +$0.0058 | +1.9% [1.2, 2.9] | +1.00 |

- **All held-out sessions** (190 / 54, `731472b94a1f`; baseline $0.312):
  - split top-5: +3.3% [2.6, 4.0], +1.54 calls;
  - LORO top-5: +3.4% [2.7, 4.1];
  - top-10 re-listed: +6.0% [3.8, 8.3].
- **What the harness does not charge.** It charges a misfire only when no listed file is ever
  opened, which is rare. It does not charge the agent opening listed files it didn't need, and
  that is the real risk.
- **Misfire sensitivity** (`evaluate.py`, part C, my attribution, harness prices). A misfire is a
  whole-file Read of a listed non-target file, carried for the rest of the session.
  - Learned top-5, p_follow = 1, no misfires: +8.9% [7.4, 10.5] per-target.
  - With p_follow 0.5 and 10% of wrong files read: **−2.6%** [−3.9, −1.5].
  - With search calls only, p_follow 0.5, and 20% misfire: −10%.
  - The break-even misfire rate is about 6–7% at p_follow 0.5.
  - Top-3 is the most robust choice: +0.8% [0.1, 1.5] for code16 alone and −1.2% for the fusion
    at p_follow 0.5, q = 0.1.
- **Carry cost of the hint text itself** (15 tokens per path + 40): 0.2% of $ at k = 5 and 0.4% at
  k = 10. It is negligible. The risk is misfires, not carry.

**Uptake anchor** (`natural.py`). RepoBench prompts never name a path, but 28% of targets have their
file stem as a word in the prompt. When the stem is named, the agent spends **25% fewer hunting calls
[13, 37]** on that target (stratified by repository; confounded, since named files are more
central). A free textual cue removes a quarter of the hunt, not all of it. p_follow = 0.5 for an
explicit path list is plausible, not proven.

**Targeted ranges** (`ranges.py`: the coordinator's follow-up, "ranges or symbols, not files").

- Read results are 28.9% of session $.
- The first whole-file Reads of target files are 16.2%.
- Oracle: those reads return only the edited lines ±30: +10.3% [9.1, 11.7].
- Realistic: the window is the top-m 60-line chunks by BM25 + code16 against the prompt. It must
  contain every edited line; if it doesn't, the agent re-reads the whole file (one extra request
  plus the window's tokens).

| window | covers every edited line | net share of $ (edited files; plus necessary reads at the same rate) |
| --- | --- | --- |
| top 1 chunk (60 lines) | 0.28 | −0.4% / −0.3% |
| top 2 chunks (120 lines) | 0.42 | +1.2% / **+1.8% [0.6, 3.0]** |
| top 3 (180) | 0.43 | +0.9% / +1.2% |
| top 4 (240) | 0.52 | +0.9% / +1.4% |

Edits span 1.75 chunks on average (often a source hunk plus a test appended far away), and the
prompt doesn't say where in a file to look. Other chunk scorers do no better: embedding alone,
token overlap, IDF-weighted overlap and their sums reach 0.28–0.37 at 2 chunks. The oracle ceiling
also assumes the agent needs nothing beyond ±30 lines, which is doubtful for reasoning. **Negative.**

**Pass rate.**

- 15 of 190 held-out sessions (7.9%) never edited a gold source file, and none passed. Sessions that
  did edit one passed 61% of the time.
- The fused top 5 contained the gold file in 93% of the 15. Most of them made no edit at all (turn
  limit, or a test-only edit), and 2 edited a wrong source file.
- If the hint fixed every one of them, the ceiling would be about 7.9% × 0.93 × 0.61 ≈ 4.5 points.
  That is not credible. These tasks are hard for reasons other than localisation.
- Replay can't see pass rate, and the harness puts a +3-point MDE at about 900 sessions per arm.

**Hook latency and precompute** (`bench_index.py`, Python, shared 4 vCPUs at load average 30).

- Per query: tokenise and embed the prompt with potion-code, one sparse BM25 product, one dense
  chunk product (3,597 × 256), max-pool, one co-change propagation, top-k.
  - Median **1.0 ms** (click) and **1.4 ms** (vue-core).
  - p95 16–37 ms, due to CPU contention.
- Precompute per repository snapshot: BM25 postings 1.3–3.9 s, chunk embeddings 4.4–12 s, index
  0.8–3.7 MB float32. This happens at SessionStart or learn time, keyed by the HEAD tree and
  incremental per changed blob oid.
- In Rust with the already-pinned model2vec loader this would be sub-millisecond per query.
  Latency is not a constraint.

## Hard-coded parts (honest list)

- **Measurement instruments only, not used by any learner:**
  - the E/N/T definitions and the exploration-call classes (`targets.py`; exploratory shell comes
    from `normalize.py`'s list);
  - the harness's `read_files` Sat/W classes;
  - the edit-line localisation in `ranges.py`;
  - p_follow and the misfire rate q (assumptions, reported as sensitivities).
- **In the learner:**
  - the tokeniser (identifier regex, camel/snake split);
  - the 60-line chunk size;
  - BM25 k1/b;
  - the co-change caps (2,000 commits, 40 files, as in `testmap.rs`);
  - "stem mentions" for the reference graph;
  - the fusion's feature set.
  - No tool names, languages, file kinds or error strings.

## What to build

1. **Don't build file prefetch as a directive yet.** Its oracle ceiling (about 12% of $, perfect
   follow, per-target re-hints) is below 15–20%. The shippable one-shot version is worth about
   what U11 is (+1.9% at p_follow 0.5), and its sign depends on an unmeasured misfire read rate.
2. **If anyone runs it live, run the micro-slice first** (≤ $6):
   - **Tasks:** 5 vue-core/urfave-cli test tasks, where replay predicts the most (+4.5–8.3% of $,
     2.4–2.9 calls).
   - **Arms:** 2 repeats × (hint on, control).
   - **Directive:** a `UserPromptSubmit` `additionalContext` with the top-3 paths from the LORO
     fusion (BM25 + potion-code-16M-v2), with no content.
   - **Pre-registered prediction:** +1.6 calls, +4% of $ at p_follow 1.
   - **Measure:**
     - p_follow: search calls before the first listed-file Read, against control;
     - q: the share of listed non-target files opened.
   - **Kill** if q ≥ 0.1 or p_follow < 0.6.
3. **What is worth keeping regardless** (cheap, and it transfers across repositories): a Rust port
   of the BM25 + potion-code-16M-v2 chunk index, keyed by the tree and incremental per blob,
   answering prompt → files in about 1 ms. It is a better *slot filler* for existing typed
   directives (U-series test-file binding, `edit_tests`) than co-change alone, which scores 0.34
   against 0.62 G@5 here. It is not a lever by itself.
4. **Where the money is instead:** the costs are the agent's own productive reads, carried across
   about 30 responses (Read results are 29% of $). Termination and output trimming are the
   literature and waste-anatomy agents' levers. Prefetch doesn't touch either.

## Files

| file | what |
| --- | --- |
| `data.py` | sessions (filtered to the harness corpus), per-call paths and result sizes; harness cost model access |
| `repo.py` | base-commit snapshots and git history from the upstream mirrors (cached) |
| `retrieve.py`, `features.py` | scorers; per-task feature cache (all 188 tasks) |
| `targets.py` | E/N/T labels, exploration-call attribution, removal pricing (instrument) |
| `evaluate.py` → `results.json` | A (task level), B (session recall), C (counterfactual with p_follow and misfire scenarios), D (curve), E (latency) |
| `ceiling.py` → `ceiling.json` | oracle ceilings by repo/model; read-batch; pre-gold |
| `harness_policy.py` → `harness_results.json` | the lever as a harness policy: split, LORO, curve, oracle, scan_memory (clean) |
| `ranges.py` → `ranges.json` | targeted-range variant |
| `natural.py` → `natural.json` | the stem-mention uptake anchor |
| `bench_index.py` | precompute and hook latency |

Run from this directory with `OMP_NUM_THREADS=1 nice -n 10 python3 <script>`, in this order:
`repo.py`, `features.py` (about 10 CPU-minutes, cached in `/tmp/adage-research/context-prefetch`),
then the others.
