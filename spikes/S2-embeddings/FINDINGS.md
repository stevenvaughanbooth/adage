# S2 · Embeddings for intent — Findings

**Rerun everything:** `./run_all.sh` (needs network once, for model downloads; `source
"$HOME/.cargo/env"` first if you're not already set up — the script does this itself for
the Rust step).

## Verdict

**Mixed, and the mixed-ness is the finding.** By the SPIKE's literal pass/fail rule, static
embeddings do **not** clear the bar: after normalisation, static's family top-1 (0.852) trails
the best transformer, bge-small-en-v1.5 fp32 (0.930), by **7.8 points** — over the 5-point
margin — so the letter of the rule says **"transformer needed."**

But the *reason* the hypothesis gave for that outcome is wrong. The hypothesis predicted static
would fail specifically on **verb-contrast pairs** ("check the ticket" vs "close the ticket").
It doesn't: static scores 92.7-99.3% on verb-contrast (pairwise and full-multiclass framings),
comfortably over the 70% failure threshold, and its open-set precision at 90% recall is
0.965-0.972, comfortably over the 0.85 bar. Every model — static included — separates
verb-contrast pairs almost perfectly. The actual 7.8-point gap comes from broader confusion
among the 14 families as a whole (e.g. `write_docs` vs `explain_code`, `add_feature` vs
`refactor_code` — topically adjacent, not verb-contrastive), which is a different failure mode
than the one S2 was designed to catch.

So: **recommend the transformer for A6, but the case for forcing a daemon purely to keep it
warm (A1) is weaker than the architecture doc assumed** — see below.

## Quality (family top-1, verb-contrast, open-set, clustering)

922 prompts: 14 intent families (542 rows, ~39/family), a 150-row verb-contrast hard subset
(10 contrast groups), 230 noise rows. Full table: `results/quality.md` / `results/quality.json`.

| Model | Norm | dim | Family top-1 (LOO) | Verb-contrast (pairwise) | Verb-contrast (multiclass) | Open-set P@90R | ARI | Purity | Noise frac |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| static: potion-base-8M | raw | 256 | 0.716 | 0.973 | 0.927 | 0.965 | 0.144 | 0.454 | 0.464 |
| static: potion-base-8M | **normalized** | 256 | **0.852** | 0.993 | 0.967 | **0.972** | 0.150 | 0.503 | 0.503 |
| bge-small-en-v1.5 fp32 | raw | 384 | 0.886 | 0.993 | 0.967 | 1.000 | 0.063 | 0.450 | 0.599 |
| bge-small-en-v1.5 fp32 | **normalized** | 384 | **0.930** | 0.993 | 0.953 | 0.998 | 0.149 | 0.573 | 0.501 |
| bge-small-en-v1.5 int8 (dynamic, local) | raw | 384 | 0.900 | 0.993 | 0.973 | 1.000 | 0.047 | 0.322 | 0.226 |
| bge-small-en-v1.5 int8 (dynamic, local) | **normalized** | 384 | **0.928** | 0.993 | 0.960 | 1.000 | 0.121 | 0.549 | 0.548 |
| all-MiniLM-L6-v2 int8 (arm64, v1 baseline) | raw | 384 | 0.812 | 0.973 | 0.940 | 0.989 | 0.021 | 0.316 | 0.175 |
| all-MiniLM-L6-v2 int8 (arm64, v1 baseline) | **normalized** | 384 | 0.880 | 0.980 | 0.920 | 0.992 | 0.159 | 0.517 | 0.475 |

**Reading it:**

- **Normalisation matters more than model choice**, exactly as Design Spec S8.2.1 claims: it is
  worth +13.6pts of top-1 for static, +4.4 for bge-fp32, +2.8 for bge-int8, +6.8 for MiniLM.
  Every model, every metric, improves with it (one negligible exception: multiclass
  verb-contrast dips 1.4pts for bge-fp32 under normalisation — noise-level, not a real effect).
- **Quantisation is free here.** bge-int8 (locally dynamic-quantized from the fp32 ONNX graph,
  since BAAI doesn't publish a pre-quantized export) scores *equal or better* than fp32 on every
  metric except top-1, where it trails by 0.2pts — noise-level. This validates the architecture's
  existing A6 choice (S4.8: "exported to ONNX and quantised to int8") directly.
- **MiniLM (the v1-design baseline) is worse than both bge variants** on every quality metric,
  supporting a move to `bge-small-en-v1.5` if v1 shipped MiniLM.
- **Verb-contrast is a non-issue for every model tested**, both in the easier pairwise framing
  (choose between the 2 families in the contrast group: 97.3-99.3%) and the harder, more
  realistic full-multiclass framing (choose among all 14 families: 92.0-97.3%). This directly
  contradicts the spike's own hypothesis.
- **Open-set precision (family vs. noise) is excellent everywhere**, static included
  (0.965-0.972 for static, >=0.989 for every transformer). Rejecting off-topic prompts is not
  where the gap lives.
- **Clustering (HDBSCAN, `sklearn.cluster.HDBSCAN`, default `min_cluster_size=5`) is weak and
  hyperparameter-sensitive for every model** — ARI 0.02-0.16, purity 0.32-0.57. A sweep of
  `min_cluster_size` (3/5/8/10/15/20) on the best embedding (bge-fp32 normalized) swung ARI
  between 0.004 and 0.15 and noise fraction between 0.18 and 0.78 with no clearly-best setting.
  **This is inconclusive by design** — S7 ("which clustering method is deterministic and scales")
  is the spike scoped to answer it. Treat the clustering numbers here as a smoke test, not a
  clustering-method recommendation.

## Cost

Measured on an Apple M3 Pro, **under heavy concurrent load** (other spikes running in
parallel — `uptime` load averages 8.4-19.7 on 11 logical CPUs during these runs, logged in
`results/cost.json` / `results/rust_cost.jsonl`). **Treat every latency number here as an
upper bound, not a clean-room result** — rerun `./run_all.sh` on an idle machine for tighter
numbers; the script is unchanged either way.

### Python (onnxruntime / model2vec bindings — NOT the target runtime, see Rust below)

| Model | Cold start, 1st run (ms) | Cold start, median of runs 2-5 (ms) | Embed p50 @1 thread (ms) | Embed p95 @1 thread (ms) | RSS after load (MB) | Determinism (same config) | Determinism (1 vs default threads) |
| --- | --- | --- | --- | --- | --- | --- | --- |
| potion-8m (static) | 294.7 | 299.3 | 0.042 | 0.129 | 107.8 | 0 | 0 |
| bge-fp32 | 269.8 | 197.4 | 6.958 | 57.062 | 366.9 | 0 | 0 |
| bge-int8 (local dynamic) | 228.6 | 209.7 | 5.253 | 49.652 | 157.0 | 0 | 0 |
| minilm-int8 (arm64) | 179.4 | 161.9 | 2.437 | 23.245 | 118.7 | 0 | 0 |

**Surprise:** in Python, static does **not** cold-load faster than the transformers — it's
the *slowest* to cold-load (~300ms vs 160-270ms for the ONNX models), because `model2vec`'s
Python import chain (huggingface_hub, jinja2, rich, tokenizers wrapper) costs more than
`onnxruntime.InferenceSession()` construction, even though the ONNX files are 4-13x bigger on
disk. **This number is an artifact of the Python binding, not the model.** It's exactly why the
SPIKE asks for a Rust check — the real target runtime for A6 is `adaged`'s Rust process via the
`ort` crate (transformer) or `model2vec-rs` (static), not Python.

Determinism is exact (max abs diff = 0.0) for every model, both across repeated runs at the
same thread count and across 1-thread vs default-thread-count runs — no quantization or
threading noise in any of these four models on this hardware.

### Rust (`model2vec-rs`, potion-base-8M) — confirms the in-process hook-client path

8 trials, `rust/run_bench.sh`, results in `results/rust_cost.jsonl`:

| Metric | Value |
| --- | --- |
| Cold load (median of 8 runs) | **17.3 ms** (min 16.8, max 20.3 — one earlier ad hoc run under worse contention hit 134.8ms, excluded from the 8-run set) |
| Embed p50 | **0.014 ms** |
| Embed p95 | **0.096 ms** |
| Determinism (encode same batch twice) | 0.0 max abs diff |

This is the number that actually matters for A1: **static, loaded and run from the same
in-process Rust binary the hook client would use, loads in ~17ms and embeds in well under
0.1ms** — both far inside the 25ms p95 prompt-intent budget (S13.2), even counting the load.
No ONNX/`ort` equivalent was built for this spike (out of time budget; see "what we could not
do" below) — that comparison is the natural follow-up if A6 lands on the transformer.

## Recommendation for A6

**Ship `bge-small-en-v1.5`, ONNX, int8 dynamic-quantized** — this is what the architecture doc
already specifies (S4.8), and S2 confirms it: quantization costs ~0.2pt of top-1 (0.928 vs
0.930) while shrinking the model 4x (33.8MB vs 133.1MB) and roughly halving Python cold-load
and per-call latency versus fp32. No reason to ship fp32.

Digests (SHA-256, for the pinning table in S4.8):

- `bge-small-en-v1.5/model.onnx` (fp32 source): `828e1496d7fabb79cfa4dcd84fa38625c0d3d21da474a00f08db0f559940cf35`
- `bge-small-en-v1.5/model_int8_dynamic.onnx` (recommended, quantized locally via
  `onnxruntime.quantization.quantize_dynamic`, onnxruntime==1.19.2 — **pin the onnxruntime
  version**, since dynamic quantization output is not guaranteed byte-stable across versions):
  `eada1f7800f6f460a0d1823e103f0d65cab9b4d663188b5eb817ba1768702348`
- `minishlab/potion-base-8M/model.safetensors` (fallback / daemonless candidate, see below):
  `f65d0f325faadc1e121c319e2faa41170d3fa07d8c89abd48ca5358d9a223de2`

**Caveat worth carrying forward:** static (potion-base-8M) is not a bad choice either — it
clears 2 of 3 pass/fail gates cleanly (open-set precision, verb-contrast) and only misses the
top-1 gate by a modest, diffuse margin that isn't concentrated in any one failure mode found
here. If A1 lands daemonless (see below), potion-base-8M normalized is a credible fallback
worth re-testing once real session data exists in S7/S8, rather than assuming the transformer
is required everywhere.

## Implication for A1 (daemon or not)

**S2 weakens, but does not remove, the "warm embedding model needs a daemon" argument in
Architecture S4.1 item 1 — and the weakening is asymmetric between the two candidate models:**

- **If A6 is static:** the case for a daemon *purely to keep the embedding model warm*
  effectively disappears. Rust cold load is ~17ms and per-call embed is ~0.014ms p50 — this can
  run inside the hook-client process on every call, no residency needed. This is the strongest
  version of the SPIKE's own hypothesis, and it holds.
- **If A6 is the recommended transformer (bge-int8):** Python cold load is ~200-270ms
  (dominated by `onnxruntime.InferenceSession()` construction, not disk I/O — RSS after load is
  only 155-160MB), well over the 25ms p95 hot-path budget, roughly matching (not contradicting)
  the architecture doc's own "hundreds of ms" estimate for this path. **No Rust `ort`-crate
  number was collected in this spike** (see below), so we can't yet say whether the real runtime
  halves that cost the way it did for static — that's the missing data point before A1 can be
  closed out on the transformer path.
- **Independent of both:** Architecture S4.1 gives two more reasons for a daemon — session state
  across hook calls, and a single writer for observations/evidence — that this spike says
  nothing about. **A1 is S3's call, not S2's**, regardless of which embedding model wins.

Net: **S2 alone does not let A1 drop the daemon.** It does suggest that *if* S3 shows
per-session state can move to files, and *if* the team is willing to accept static's modest
quality gap, a fully daemonless design becomes credible — a combination the architecture doc's
current text doesn't consider a live option. If the team instead wants the higher-quality
int8 transformer, the daemon's embedding-residency rationale still stands, though a Rust `ort`
measurement (not done here) could still narrow the gap.

## Does a behaviour-trained projection (O13) look necessary?

**No, not on this evidence — but this spike can't fully test it, and says so honestly.**

O13 was motivated by exactly the verb-contrast failure ("check the ticket" vs "close the
ticket") this spike built a dedicated hard subset to probe. That failure didn't show up: every
model, static included, resolves verb-contrast pairs at 92-99% accuracy without any projection.
If the projection's job is specifically to fix verb-contrast confusion, there's no confusion
here to fix.

But two honest caveats keep this from being a clean "no":

1. **This is a synthetic benchmark by design** (the spike rules forbid real session data). The
   templates that generate verb-contrast pairs use genuinely different verbs ("close" vs "check"
   vs "start") applied to the same noun — a projection trained on *behavioural* divergence
   (Design Spec S8.2.2: sessions that followed different routes) is solving a subtly different
   problem than lexical verb contrast, and might catch cases this benchmark's templates don't
   produce (e.g. two prompts that are near-paraphrases lexically but provoke different agent
   behaviour for reasons the text doesn't surface).
2. **The actual 7.8-point gap that DID show up — diffuse multiclass confusion among topically
   adjacent families — is closer to what a behaviour-trained projection is designed for**
   (pulling same-route prompts together, pushing different-route prompts apart) **than what O13
   was originally scoped against.** If that gap matters in practice, a projection is a more
   targeted fix for it than switching models.

Recommendation: don't build the projection yet. Re-evaluate once S7/S8 produce real (or
proving-ground) session data with actual route outcomes, so "does the projection beat the
plain embedding on the credibility test" (O13's own ship gate, D S8.2.2) can be measured
directly instead of guessed at from a synthetic set.

## What we could not do

- **No Rust `ort`-crate benchmark for the transformer.** The SPIKE only requires the Rust check
  for the static model (Method step 5); we built and ran that (see above), but did not extend
  it to bge-int8 via the `ort` crate, which is the actual A6/A1 decision-relevant number if the
  transformer ships. This is the single most valuable follow-up measurement.
- **Budget-driven download cuts** (approved envelope was <200MB total; we used 188.2MB):
  skipped `minishlab/potion-base-32M` (the "one larger potion variant if available" the SPIKE
  asks for — would have added ~129MB) and MiniLM's fp32 ONNX (~90MB; kept only its pre-quantized
  int8-arm64 export as a single-point v1 baseline rather than a full fp32/int8 pair). Neither
  gap changes the A6/A1 recommendation, since MiniLM already loses to both bge variants and a
  larger static model would only move the top-1 gap, not the verb-contrast or open-set numbers
  that actually decided the verdict.
- **Clustering is inconclusive**, as noted above — by design, deferred to S7.
- **Latency numbers are contention-inflated.** This machine ran other spikes concurrently
  throughout (`uptime` load average up to 19.7 on 11 logical CPUs); p95 numbers especially
  should be re-measured idle before being used for a final go/no-go on the 25ms budget.

## Reproducing

```
cd spikes/S2-embeddings
./run_all.sh
```

Individual steps (all read `data/prompts.jsonl`, regenerated deterministically by
`data/gen_data.py --seed 20260922`):

- `src/s2/download_models.py` — pinned HF downloads into `models/` (git-ignored), SHA-256
  manifest at `models/MANIFEST.json`.
- `src/s2/quantize_int8.py` — local dynamic quantization of bge-small fp32 -> int8.
- `data/gen_data.py` — regenerates `data/prompts.jsonl` (922 rows: 14 families x ~39
  paraphrases, 150-row verb-contrast hard subset across 10 contrast groups, 230 noise rows —
  synthetic, not real session data, per the spike rules; reusable as-is by S7).
- `src/s2/bench_quality.py` — writes `results/quality.json` / `results/quality.md`.
- `src/s2/run_cost.py` (drives `src/s2/bench_cost.py` per-model/per-thread-count subprocesses)
  — writes `results/cost.json` / `results/cost.md`.
- `rust/run_bench.sh` (drives `rust/src/main.rs` via `model2vec-rs`) — writes
  `results/rust_cost.jsonl`.
