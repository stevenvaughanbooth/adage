# S7 · Intent clustering at scale

**Informs:** the intent-family pipeline (Design §8.2.1, Architecture §4.7) · **Wave:** 2 — starts
after S2 picks an embedding model · **Budget:** one day, no tokens

## Question

Which clustering method finds intent families that are **deterministic**, handle **noise** (most
prompts belong to no family), and **scale** to hundreds of thousands of prompts on a laptop — and
does behavioural purity splitting and merging work as designed?

## Why it matters

The review found the Rust `hdbscan` crate uses a kd-tree only, which degrades at 384 dimensions, and
makes no determinism claim. Intent families are central to discovery routes; the pipeline needs a
method that is stable run to run and fast enough for a nightly job.

## Method

1. **Corpus.** Start from S2's labelled benchmark set. Scale it synthetically to 10k, 100k, and 300k
   prompts by templated paraphrase with injected IDs and paths, keeping ground-truth family labels
   and a large noise fraction (≥ 60%). Embed with the model S2 recommends.
2. **Candidates:**
   - HDBSCAN (Python `hdbscan` / `sklearn`) directly on embeddings
   - Dimensionality reduction first (PCA to 32–64 dims; UMAP only if it can be made deterministic)
     then HDBSCAN
   - kNN graph (exact or HNSW) + Leiden/Louvain community detection with a fixed seed
   - Per-repository clustering (smaller n) versus global
   - The Rust `hdbscan` crate, for scale and determinism, as a data point
3. **Metrics:** ARI and purity against labels; noise recall (noise prompts left unclustered); run-to-
   run stability (same input, same output — and under input reordering); wall-clock and peak memory
   at each scale.
4. **Behavioural purity.** Attach synthetic "next route" labels to each prompt, with deliberately
   mixed families (e.g. check-ticket vs close-ticket sharing words). Implement the split/merge step
   from Design §8.2.1 and measure how much it corrects embedding clusters.

## Pass / fail

A method with ARI ≥ 0.8 on the benchmark, identical output across reruns and input reorderings,
≤ 10 minutes and ≤ 4 GB at 300k prompts — and a working purity split/merge.

## Deliverables

`FINDINGS.md` with the comparison table, the recommended method and parameters, and the purity
step's measured effect.
