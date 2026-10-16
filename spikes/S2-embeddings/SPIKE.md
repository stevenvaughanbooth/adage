# S2 · Embeddings for intent

**Informs:** A6 (embedding model), A1 (whether a daemon is needed at all) · **Wave:** 1 ·
**Budget:** one day, model downloads < 200 MB, no Claude tokens

## Question

Are **static embeddings** (model2vec / potion family: lookup table + pooling, near-zero load time)
good enough to recognise intent families, or do we need a **small transformer** (e.g.
`bge-small-en-v1.5`, int8 ONNX) — and what does each cost to load and run?

## Why it matters

A transformer needs to stay warm in a resident process, which is the main remaining reason for a
daemon. If static embeddings are good enough, each hook call can embed in-process in about a
millisecond and the daemon may disappear (A1).

## Hypothesis

Static embeddings load in a few ms and embed in well under 1 ms, but separate intent families
worse — especially **verb-contrast pairs** ("check the ticket" vs "close the ticket"). A transformer
separates better but costs hundreds of ms to load and a few ms per query. Normalisation (replacing
IDs and paths with typed placeholders) narrows the gap.

## Method

1. **Benchmark set** (`data/`). Build a labelled prompt set of realistic coding-agent prompts:
   - ≥ 12 intent families (start work on a ticket, fix failing test, investigate CI failure, review
     a PR, add a feature, refactor, update dependencies, write docs, explain code, close/triage a
     ticket, deploy-related question, set up environment…), ≥ 25 paraphrases each, varied phrasing
     and length, with realistic IDs, paths, and pasted snippets.
   - A **hard subset** of verb-contrast pairs that share nouns but differ in intent.
   - ≥ 200 "noise" prompts that belong to no family.
   - Write the set by hand/programmatically in the spike; do not use real session data.
2. **Normaliser** for prompts: typed placeholders for IDs, paths, SHAs, URLs; strip fenced code and
   logs; keep the head (~128 tokens). Evaluate every model with and without it.
3. **Models** (download into `models/`, git-ignored):
   - static: `minishlab/potion-base-8M` (and one larger potion variant if available)
   - transformer: `BAAI/bge-small-en-v1.5` (fp32 and int8 ONNX), `sentence-transformers/all-MiniLM-L6-v2` as the v1-design baseline
4. **Quality** metrics, per model × normalisation:
   - family retrieval: top-1 nearest-centroid accuracy with leave-one-out centroids
   - verb-contrast accuracy on the hard subset
   - open-set: precision at 90% recall for "belongs to a family" vs noise, with per-family thresholds
   - clustering: run HDBSCAN (Python `hdbscan` or `sklearn.cluster.HDBSCAN`) and report ARI / purity
     against labels, and the noise fraction
5. **Cost** metrics, per model:
   - cold load time (process start → first embedding ready)
   - per-prompt embed latency p50/p95 at ≤128 tokens, single thread
   - resident memory
   - For the static model, also measure load+embed from **Rust** (`model2vec-rs` if it builds) to
     confirm the in-process hook-client path.
6. Determinism: embed the set twice (and with 1 vs N threads) and report max abs difference.

Use `uv` with a spike-local virtualenv. Pin package versions in `pyproject.toml`/`requirements.txt`.

## Pass / fail

- **Static is enough** if, after normalisation, its family top-1 is within 5 points of the best
  transformer and its open-set precision at 90% recall is ≥ 0.85 → recommend daemonless intent
  matching.
- **Transformer needed** if the gap is larger, or verb-contrast accuracy for static is < 70%.
- Either way, report whether a **behaviour-trained projection** (O13) looks necessary.

## Deliverables

`FINDINGS.md` with the quality and cost tables, the verdict for A6, the implication for A1, and the
recommended model(s) with digests. Reusable benchmark set and scripts for S7.
