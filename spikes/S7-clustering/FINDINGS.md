# S7 · Intent clustering at scale — Findings

**Rerun everything:** `./run_all.sh`. It needs network once for the 31 MB model and takes about
1.5 h, most of it Python baselines running into their timeouts. Machine: Apple M3 Pro, 11 cores,
18 GB. `uptime` is recorded in every `results/bench_*.json`. The Rust timings were taken with
nothing else heavy running.

## Verdict

**Recommend exact cosine kNN + deterministic Louvain, per repository, over prompts in canonical
order, followed by the behavioural split/merge.** Its agreement with the labels is the best of any
method tried. It is deterministic across reruns and input reorderings, byte for byte. It
finishes 300k prompts in 8.0 min / 768 MB even as one global graph, and in 17 s / 652 MB per
repository, which is how production runs it. The implementation is about 250 lines of std-only Rust
(no BLAS, no index, no RNG). The purity step lifts held-out directive precision from 0.57 to 0.87.

Against the SPIKE's pass/fail:

| Criterion | Result |
| --- | --- |
| ARI ≥ 0.8 | **Fail, for every method, and out of reach for this embedding.** A *supervised* oracle (true family centroids, per-family thresholds) gets only **ARI 0.74–0.76** with potion-base-8M + S2 normalisation. The best unsupervised method (Louvain) gets **0.42** (0.40 on S2's benchmark); every HDBSCAN variant gets 0.09–0.32. |
| Identical output across reruns and reorderings | **Pass** for Louvain with a canonical input order (also kNN-DBSCAN and mutual-kNN). **Fail** for sklearn HDBSCAN, the `hdbscan` crate, and leidenalg: rerun-stable but not order-invariant. |
| ≤ 10 min and ≤ 4 GB at 300k | **Pass**: 481.7 s, 768 MB global; 17.0 s, 652 MB per repository. Every HDBSCAN variant hit its 15-minute timeout, at 100k (sklearn, crate) or 300k (PCA-32). Leiden took 11.3 min and 5.3 GB. |
| Purity split/merge works | **Pass.** On held-out prompts, directive precision goes from 0.57 to **0.87** and recall from 0.51 to **0.69**. The oracle partition scores 0.91 / 0.75. |

The ARI failure comes from the embedding plus realistic paraphrase noise, not from the method, and
behaviour repairs most of it. ARI is also the wrong production metric. It punishes a family split
into two halves with the same route, which is harmless: the merge step handles it, or so do two
routes with the same command. What matters is whether a matched prompt gets the right directive,
and after purity that happens 87% of the time. If precision needs to be higher, the lever is the
embedding (S2: bge-small is 7.8 points better at top-1), not the clustering.

## Corpora

* **S2 benchmark:** 922 prompts, 14 families, 25% noise, unchanged, from
  `spikes/S2-embeddings/data/prompts.jsonl`.
* **Scaled test corpora:** 10k, 100k, and 300k prompts (`src/s7/corpus.py`, seed 20260922).
  They use S2's 14 families and templates, paraphrased further with openers, closers, context
  sentences, dropped words, typos, and fresh IDs, paths and names. 99.4% of prompts are unique
  before normalisation. **62% of each corpus is noise**, of two kinds:
  * *one-off* (80% of noise): frames filled with random content words from the model's pinned
    vocabulary, excluding every family word. They are diffuse (10th-neighbour cosine ≈ 0.28).
  * *chatter* (20% of noise): S2's noise templates (acknowledgements, trivia), paraphrased. They
    are textually dense (≈ 0.60, close to the families' ≈ 0.66). Any density method will find
    clusters here. They carry no consistent behaviour, and filtering them is the purity step's job.
* **Repositories:** the 100k and 300k corpora are spread over 10 and 30 repositories of about 10k
  prompts each, to measure production's per-repository shape.
* **Behaviour:** each prompt carries its session's *next route*, with deliberate mixing:
  * start / check / close / triage ticket share nouns but lead to four different routes. These
    are the split candidates.
  * add_feature + refactor_code, and write_docs + explain_code, each share a route. These are the
    merge candidates.
  * update_dependencies is split 50/50 by something the text cannot show, so it must never
    trigger.
  * Every family follows its route with p = 0.85.
* **Parameters:** tuned only on a separate development corpus (10k, seed 1; `sweep.py`,
  `sweep2.py`, `louvain_py.py` → `results/sweep*.json`), then frozen for every test corpus.

Embedding: potion-base-8M (S2's static model; implementation plan A6) with S2's normaliser.

## Comparison (10k test corpus, 62% noise)

How to read the columns:

* **Noise recall:** the share of true-noise prompts left unclustered.
* **Coverage:** the share of family prompts placed in some cluster.
* **Scoring:** noise, true or predicted, counts as singletons, so lumping all noise into one
  cluster earns nothing.

`results/bench_quality.json` has the same table for S2's 922-prompt benchmark.

| Method | Params | ARI | Purity | Coverage | Noise recall (one-off / chatter) | Rerun same | Reorder same | Wall | Peak RSS | In Rust? |
| --- | --- | ---: | ---: | ---: | --- | :---: | :---: | ---: | ---: | --- |
| *Oracle (supervised)* | true centroids, per-family threshold | *0.745* | *0.89* | *0.90* | *1.00 / 0.85* | — | — | — | — | — |
| **kNN + Louvain (Rust)** | k=10, cos ≥ 0.6, res 2.0, min 10 | **0.418** | 0.75 | 0.98 | 1.00 / 0.13 | ✅ | ✅ (canonical order) | 0.64 s | 27 MB | **yes, std only** |
| kNN + Leiden (leidenalg, seed 0) | k=15, cos ≥ 0.6, res 2.0, min 10 | 0.398 | 0.70 | 0.98 | 1.00 / 0.15 | ✅ | ❌ (ARI 0.986) | 1.35 s | 227 MB | no (C++ library) |
| kNN-DBSCAN (Rust) | k=15, cos ≥ 0.75, minPts 8 | 0.234 | 0.75 | 0.43 | 1.00 / 0.73 | ✅ | ✅ | 0.55 s | 23 MB | yes |
| HDBSCAN, sklearn, 256-d | mcs 20, ms 5 | 0.194 | 0.67 | 0.36 | 1.00 / 0.89 | ✅ | ❌ (0.999) | 23 s | 165 MB | no |
| HDBSCAN, Rust `hdbscan` crate 0.12 | mcs 20, ms 5 | 0.194 | 0.67 | 0.36 | 1.00 / 0.89 | ✅ | ❌ | 25 s | 38 MB | yes (see below) |
| HDBSCAN on the kNN graph (portable) | mcs 40, ms 5, k 15 | 0.173 | 0.59 | 0.26 | 1.00 / 0.97 | ✅ | ❌ (0.999) | 0.93 s | 216 MB | portable |
| Mutual-kNN connected components | k=10, cos ≥ 0.75, min 5 | 0.113 | 0.51 | 0.65 | 1.00 / 0.30 | ✅ | ✅ | 1.1 s | 218 MB | yes |
| HDBSCAN, hdbscan lib, PCA-32 | mcs 5 | 0.089 | 0.85 | 0.33 | 0.85 / 0.70 | ✅ | ❌ (0.979) | 2.3 s | 183 MB | no |

What the table shows:

* **HDBSCAN does not fit these data.** It is precise but finds only a quarter to a third of the
  family prompts (coverage 0.26–0.36). A family is not one dense core: it has one mode per
  phrasing, joined by lower-density bridges, and HDBSCAN calls the bridges noise. On S2's
  benchmark, PCA-32 and kNN-HDBSCAN collapse to two clusters (ARI ≈ 0). This confirms, at scale,
  S2's finding that HDBSCAN is "weak and hyperparameter-sensitive".
* **Community detection keeps almost every family prompt** (coverage 0.98) but over-splits:
  63 clusters for 14 families. That is harmless here, because the behavioural merge and
  per-family routes absorb it.
  * It also clusters chatter (chatter noise recall 0.13). Textually that is correct, and it does
    no harm: chatter has no consistent next route, so the purity bar drops it. After purity, 0.4%
    of noise prompts get a directive.
* **The Rust `hdbscan` crate** now has brute-force and kd-tree modes. At 10k its output matches
  sklearn's, and it is just as slow (a quadratic MST). It timed out at 100k and is not
  order-invariant.
* **Louvain and Leiden reach the same quality** (0.418 vs 0.398). Leiden's refinement phase buys
  nothing here. leidenalg is not order-invariant, and porting it to Rust would be a large job.

## Scale

| Method | 10k | 100k | 300k |
| --- | --- | --- | --- |
| **kNN + Louvain (Rust), per repository** (30 × 10k at 300k) | 0.56 s · 27 MB · ARI 0.42 | **5.7 s · 216 MB** · mean per-repository ARI 0.41 | **17.0 s · 652 MB** · mean per-repository ARI 0.41 |
| **kNN + Louvain (Rust), one global graph** | 0.57 s · 27 MB · ARI 0.42 | 58.9 s · 255 MB · ARI 0.31 | **481.7 s (8.0 min) · 768 MB** · ARI 0.22, purity 0.85 |
| kNN + Leiden (numpy kNN + leidenalg), global | 1.1 s · 226 MB · 0.40 | 76.9 s · 1.9 GB · 0.34 | 676 s (11.3 min) · **5.3 GB** · 0.24 |
| HDBSCAN, hdbscan lib, PCA-32 | 2.5 s · 184 MB · 0.09 | 155 s · 737 MB · 0.007 | **timeout** (> 15 min) |
| HDBSCAN, sklearn, 256-d | 22.5 s · 165 MB · 0.19 | **timeout** (> 15 min; about 37 min extrapolated) | not run (about 6 h extrapolated) |
| HDBSCAN, Rust `hdbscan` crate | 25.7 s · 38 MB · 0.19 | **timeout** (> 15 min) | not run |

What the scale runs show:

* **The exact kNN is almost all of the cost.** It is O(n²·d) and takes 480.9 s of Louvain's
  481.7 s at 300k. Louvain itself takes 0.55 s. Global 300k passes the budget with little
  headroom: at this rate, about 330k prompts would take 10 min on this laptop.
* **Per repository is the production shape**, since Design §8.2.1 learns families per repository.
  * It is 28× cheaper at 300k, because the kNN is quadratic in each repository, not in the whole
    corpus.
  * Its quality equals the 10k result: mean ARI 0.41, range 0.37–0.45 over 30 repositories.
  * Scoring per-repository clusters with one global ARI gives 0.02–0.07, which means nothing. The
    same intent found in ten repositories is ten correct families.
* **Global ARI falls as n grows** (0.42 → 0.31 → 0.22) while purity rises (0.75 → 0.85). With k
  and resolution fixed, a bigger graph splits into more, purer communities (63 → 192 → 292). The
  families are split, not mixed. For a global run, raise the resolution or merge by behaviour.
* **If one repository ever has hundreds of thousands of prompts**, the fix is known and keeps the
  algorithm deterministic:
  * Deduplicate normalised prompts first; real prompts repeat a lot.
  * Then use a block-tiled or int8 dot-product kNN.
  * An approximate index (HNSW) would give up exactness, and with it the determinism argument.

## Behavioural purity: split and merge (Design §8.2.1 step 5)

Implemented in `src/s7/purity.py` and ported to Rust as-is in `crates/adage-intent`:

1. **Split.** Within a cluster, take the route groups (each at least max(10, 15%) of members).
   Check whether the text separates them: leave-one-out nearest-group-centroid accuracy ≥ 0.85.
   If it does, move every member to its nearest group centroid. If it does not, do not split: the
   runtime could not reproduce the split from a prompt.
2. **Merge.** Clusters with the same dominant route and centroid cosine ≥ 0.8 merge, most similar
   pair first.
3. **Bar.** A family is usable only if its dominant route's share is ≥ 0.6.

Evaluation mirrors the runtime:

* Cluster and purify on 70% of the prompts.
* Deploy to the held-out 30%: nearest usable-family centroid, per-family threshold at the 10th
  percentile of members' similarity, floor 0.5.
* Truth is the route of the prompt's true family.

| 10k test corpus, global Louvain | Families | Directive precision | Directive recall | Noise prompts directed | Impure-family prompts directed |
| --- | ---: | ---: | ---: | ---: | ---: |
| No purity step (every cluster, its dominant route) | 29 | 0.573 | 0.514 | 1.05% | 83.9% |
| Purity bar only | 13 | 0.683 | 0.415 | 0.28% | 16.1% |
| **Split + merge + bar** | 28 | **0.874** | **0.693** | 0.44% | 24.1% |
| *Oracle partition + bar* | 13 | *0.911* | *0.746* | 0.99% | 19.5% |
| HDBSCAN on kNN graph, split + merge + bar | 9 | 0.971 | 0.276 | 0% | 0% |

Per repository at 100k (10 repositories, mean over repositories), from no purity step to bar only
to the full step:

* Precision: 0.644 → 0.798 → **0.846**.
* Recall: 0.591 → 0.476 → **0.623**.

What this shows:

* **The step works as designed.** The bar alone removes behaviourally random families: chatter,
  and the 50/50 dependency family, whose prompts get a directive 16% of the time instead of 84%.
  It costs recall. Split and merge then recover that recall and more, finishing near the oracle.
* **Verb-contrast is where the split earns its keep.** Louvain puts close / check / start ticket
  into one mixed cluster. The route groups separate on text (LOO accuracy ≥ 0.85), so the split
  produces three families, one per route.
* **Residual:** 24% of the impure family's held-out prompts still get a directive after split and
  merge. Its prompts sit near the add and refactor families, whose centroids and thresholds absorb
  some of them. The lever is calibrating each family's threshold against behaviour. The Rust
  learner does that, instead of the 10th-percentile rule used here.
* **HDBSCAN with purity** is very precise (0.97) but directs almost nothing (recall 0.28).

## Recommended method and parameters (implemented in slice 9)

```text
per repository:
  prompts   → S2 normaliser → potion-base-8M (unit vectors)
  order     → sort by (normalised text, session id)           # canonical: order-invariant
  kNN       → exact cosine, k = 10, ties by lower index        # f32 dot, fixed 8-lane summation
  graph     → undirected edges with cosine ≥ 0.6
  Louvain   → resolution 2.0; nodes visited in index order; candidate communities in id order;
              move only on a strictly better gain (> 1e-12); aggregate by first appearance
  families  → communities with ≥ min_support prompts (5; 3 in demo mode)
  purity    → split (sep ≥ 0.85), merge (cos ≥ 0.8), bar (dominant route ≥ 0.6)
  threshold → per family, calibrated on the repository's own prompts
```

**In Rust, deterministic, with no heavy dependencies:** `rust/src/cluster.rs` is std only (it uses
`std::thread::scope` for the kNN). On the dev corpus it reproduces the Python reference partition
exactly (`src/s7/check_rust.py`). What makes it deterministic:

* **The kNN is exact**, not approximate. Each query row's top-k does not depend on any other row,
  so the thread count cannot change the result; a unit test checks 1 vs 7 threads. The dot product
  sums in a fixed 8-lane order, so sim(i, j) == sim(j, i) bit for bit.
* **Louvain has no randomness:** nodes are visited in index order, moves need a strict
  improvement, candidate communities are scanned in id order, and aggregation numbers communities
  by first appearance.
* **Order invariance comes from the canonical sort**, not from the algorithm. Without the sort,
  Louvain is order-dependent (`reorder_identical_without_canonical_sort: false`). Architecture
  §4.7 already requires sorted inputs; this makes the requirement concrete.

**Not recommended:**

* HDBSCAN in any form: weaker agreement with the labels, slower, and order-dependent (all shown
  above).
* UMAP: it has no deterministic multithreaded mode, and it is not needed.
* An approximate kNN index: it gives up exactness.

## Surprises

1. **The ceiling is the embedding, not the clustering.** Even given the true families' centroids,
   potion + S2 normalisation cannot separate these 14 families beyond ARI ≈ 0.75
   (`results/ceiling.json`). The ceiling is 0.76 on S2's benchmark and 0.74 on the scaled corpus,
   so the scale-up is no harder than S2's own set.
2. **Normalisation variants** (`results/norm_variants.json`): keep S2's normaliser.
   * Bare or dropped placeholders do worse than S2's `{ticket}` tokens.
   * Keeping only the first sentence helps S2's clean benchmark (+0.08 ARI). It hurts realistic
     prompts that open with "hi --" or "quick one:" (−0.19).
3. **S2's ticket regex misses one-letter prefixes.** `[A-Z]{2,6}-\d+` does not match `W-4821`,
   the demo's own IDs. The production normaliser uses adage-core's ticket span pattern instead.

## What we could not do

* **No real session data** (spike rules), so the route labels are synthetic. The behavioural
  numbers show that the mechanism works; they do not predict precision on real sessions.
* **No transformer comparison at scale.** How far S2's bge-small would raise the ceiling was not
  measured.
* **The HDBSCAN baselines at 100k and 300k are timeouts, not measurements.** Their extrapolations
  assume the quadratic growth observed below the timeout.

## Addendum: applied in slice 9 (`crates/adage-intent`)

The recommendation now ships as production Rust. Two checks close the loop between the spike and
the product.

**The production embedder equals the Python reference** (`src/s7/check_rust_embed.py` →
`results/rust_embed_check.json`). The embedder is a plain-Rust WordPiece tokenizer, safetensors rows
read on demand, and mean pooling, with no `tokenizers`, `model2vec-rs` or `hf-hub` dependency.
Over 3,922 prompts (S2's benchmark plus 3,000 scaled prompts):

* The embedding cosine to model2vec's is **1.000000** at the minimum; every vector is identical to
  1e-5.
* The normalised text is identical for 3,839 prompts. The other 83 differ only by the intended
  one-letter ticket fix (`W-4821` → `{ticket}`).

**Hook latency with the model**, from `crates/adage-cli/examples/hook_latency.rs` →
`results/hook_latency.json`. Each scenario spawns the release binary in exec form, the way Claude
Code runs it, 500 times after 20 warm-up runs:

| `UserPromptSubmit` scenario | p50 | p95 | p99 |
| --- | ---: | ---: | ---: |
| Repository with no routes (the common path) | 1.68 ms | 1.76 ms | 1.81 ms |
| Structured routes only (no model touched) | 3.09 ms | 3.18 ms | 3.32 ms |
| **Intent routes, model installed (open + embed + match)** | **6.25 ms** | **6.39 ms** | 6.73 ms |
| Intent routes, ticket prompt (still embeds) | 6.20 ms | 6.33 ms | 6.79 ms |
| Intent routes, model missing (fails open) | 3.09 ms | 3.19 ms | 3.27 ms |

Per-process cost of the intent path, about 3.2 ms in total:

* 1.25 ms compiling the normaliser's regexes;
* 1.25 ms building the 29,528-token vocabulary map;
* 0.01 ms for the embedding itself (a dozen 1 KB row reads).

Loading the model in every hook process is within budget: 6.4 ms p95 against the 25 ms p95
prompt-with-intent budget (Design §13.2), with no daemon. If it ever needs to be faster, two
optimisations would roughly halve the added cost, and neither is needed now:

* ASCII-only regex classes in the normaliser;
* a precomputed sorted vocabulary file searched in place, instead of a hash map built on every
  call.
