> Embeddings, made real and then measured. 2026-09-29. Replay only ($0): RepoBench v2 held-out
> control sessions (Haiku 4.5; click 24, attrs 24, marshmallow 18, vue-core 16; 30 tasks), learned
> from each repository's 7–10 training traces. Instruments: `adage eval replay-savings --models
> <dir> --cases off|on|ungated` and `adage eval embeddings` (both new). Raw outputs and the scripts
> that produced them: [embeddings/](embeddings/).

# Embeddings: what ships, and what the replay metric says they are worth

## 0. Verdict

- **Distribution is fixed and ships on.** The `adage` binary now carries potion-base-8M quantised to
  int8 (7.9 MB, SHA-256 pinned, rebuilt reproducibly from the pinned upstream files). A fresh install
  embeds with no download and no user action. `adage models fetch [<key>]` falls back from Hugging
  Face to a pinned npm tarball. `adage doctor`, the status card and `adage models status` say what
  embeds, and the hashed fallback is explicit when no model exists.
- **potion-code-16M-v2 is obtainable** (npm, MIT, byte-for-byte the pinned upstream files) and
  selectable (`adage models fetch potion-code-16m-v2 && adage models use potion-code-16m-v2`). It is
  not bundled: nothing on the default path uses its strength yet (below).
- **On replay, a real model changes nothing by itself.** Hashed-only, bundled potion-8M and
  potion-code-16M give *identical* Ŝ with the case store off: **+1.71** calls/session post-hoc
  [90% CI +1.15, +2.32] and **+2.29** pre-emptive [+1.70, +2.88], 0 harmful. Learning intent families
  with a model formed no intent route from 7–10 traces per repository, so there is nothing for the
  model to fire.
- **The case store (situation kNN, outcome-weighted, value-admitted) stays off.** Behind the value
  gate it adds +0.02 to +0.03 calls/session pooled (+0.10 to +0.16 on vue-core), CIs across 0, no
  harm. Without the gate (a diagnostic), vue-core gains **+1.7 to +2.3** calls/session, but precision
  is 0.73 Wilson LB (the §5.2 gate is 0.8) and the hashed and bundled runs each carry a harmful
  directive. The gain is one proposal, "run `npm test -- packages/runtime-core`", which is also
  what a frequency count says (area kNN hit@1 = frequency hit@1 = 0.62). It is embeddings
  rediscovering the most-edited package, not similarity doing work.
- **Where embeddings do carry signal: prompt → file retrieval with the code model on the monorepo.**
  On vue-core, potion-code-16M-v2 puts an edited file in the top 5 for 0.69 of held-out sessions,
  against 0.00–0.25 for potion-8M, hashed features, kNN over past prompts, and the frequency prior;
  area hit@1 0.75 vs 0.25. That matches the research lab's localisation study (BM25 + potion-code
  hit@5 0.89). It is not shipped as a lever, because the same study found localisation to be a
  small ceiling on RepoBench (first gold-file touch at request 2), and the replay metric cannot yet
  score a read hint.

## 1. Distribution

| Option | Size | Reaches | Verdict |
| --- | --- | --- | --- |
| **Bundle int8 in the binary** (`adage-intent` feature `bundled`, on in `adage`) | +7.9 MB (7,677,438-byte table with one f32 scale per row, plus 219,689-byte vocabulary) | every channel: shell and PowerShell installers, Homebrew, npm, the marketplace plugin, `cargo install` | **chosen** |
| Bundle f16 in the binary | +15.3 MB | every channel | twice the size for a cosine gain nobody can measure (int8 vs f32 already 0.99998) |
| Ship the file in the plugin bundle (next to the binaries) | 7.9 MB per plugin | the marketplace plugin only | leaves npm, Homebrew and installer users without a model |
| Fetch from npm `@yarflam/potion-base-8m@1.0.4` | 28.3 MB tarball | anywhere `registry.npmjs.org` is reachable | **fallback of `adage models fetch`**: the three files are byte-identical to the pinned SHA-256s (checked here) |
| Fetch from the qdrant-fastembed GCS bucket | – | reachable here | holds ONNX transformer models (bge-small, MiniLM), not potion; transformers do not belong in the hook |
| Hugging Face (pinned revision) | 30.9 MB | blocked here (403) | still the first source of `fetch`; `ADAGE_MODEL_BASE_URL` for mirrors |

- **Pins.** The upstream digests are unchanged. The bundled bytes are pinned too
  (`BUNDLED_TABLE_SHA256`, `BUNDLED_VOCAB_SHA256` in `crates/adage-intent/src/model.rs`), a test
  checks them, and `cargo run -p adage-intent --example derive_bundle -- <upstream files or unpacked npm package>`
  re-derives them. The npm tarballs are also pinned (size and SHA-256); the gzip and tar are read in
  process (miniz_oxide, already a dependency), so nothing new is spawned.
- **Ids.** The bundled model's id is `potion-base-8m-i8@b9c0e329a2ce` (its own table digest), not the
  f32 model's `potion-base-8m@f65d0f325faa`. Their vectors agree to cosine ≥ 0.99998, but an intent
  threshold learned with one is only matched against the same model. Learner and hook resolve the
  model with one function (`model::resolve`: installed, else bundled, else hashed), so they agree.
  Pack binding accepts both ids.
- **potion-code-16M-v2**: npm `zvec-grep-potion-code-16m-v2-model@0.1.0` (MIT; its manifest names
  `minishlab/potion-code-16M-v2@e9d2a44c`), f16 63,457 × 256, the same BERT-uncased WordPiece
  tokenizer, so the in-process loader reads it as is (f16 tables are now supported).
- **Licences.** Both models are MIT. NOTICE and `about.hbs` carry the potion-base-8M attribution
  and licence text for the bundled derivative.
- **Status is never silent.**
  - `adage models status` prints `embeddings: potion-base-8m-i8@b9c0e329a2ce (bundled in this binary)`, whether
    the bundled bytes match their pins, and each known model's install state.
  - `adage doctor` passes with the same line plus any failed download ("the last download failed:
    huggingface.co blocked by proxy (403) …").
  - The status card's footer reads `embeddings: potion-base-8m-i8 (bundled)`.
  - With no model at all (a build without the bundle, or an unusable install), the card says
    `(no embedding model: hashed fallback)` in grey and doctor warns.
  - `adage models use hashed` chooses the fallback explicitly.
- **Checked live here** (Hugging Face blocked):
  - `adage models fetch` went Hugging Face 403 → npm → verified → installed, in 5.3 s.
  - `adage models fetch potion-code-16m-v2` installed the same way.
  - The integration test `model_blocked` now asserts the two-source attempt and the bundled model
    serving meanwhile.

## 2. The case store

`crates/adage-learn/src/cases.rs`, as the brief specified:

- **Situations and search.** A decision point's situation is the prompt, plus a compact text in the
  action grammar: phase, last 3 actions (`test-run|narrow pytest system failed`), error signature,
  edited directories. Each is embedded and stored as i16. Search is exact kNN (k = 10) by integer
  dot products: deterministic, ties to the earlier case.
- **Proposals and votes.** Proposals come from the first test run within the horizon that executed
  and passed in the project's environment. They are either the runner (`.venv/bin/python -m pytest`,
  `npm test`) or, in a monorepo, the area (`npm test -- packages/runtime-core`). Votes are
  sim² × outcome weight; neighbours that never reached a passing run vote for silence.
- **Admission.** Leave one session out, through the value model's
  `A = p_follow · (1 − p_sat) · (Q(other) − Q(Sat d)) − C − p_misfire · m` with its priors and
  costs, admitted on a positive one-sided 90% lower bound with ≥ 3 effective samples (the SPIBB
  rule).

Replay (task-clustered bootstrap, 2000 draws, 90% percentile CI; full table with every
configuration in [embeddings/replay-ablation.md](embeddings/replay-ablation.md)):

| Embedder | Case store | Mode | Ŝ, all 4 repos (82 sessions) | vue-core (16) | Directives | Useful (Wilson90 LB) | Harmful |
| --- | --- | --- | --- | --- | ---: | --- | ---: |
| hashed (= today) | off | post-hoc | **+1.71** [+1.15, +2.32] | −0.04 [−0.04, −0.03] | 66 | 66 (0.96) | 0 |
| hashed (= today) | off | pre-emptive | **+2.29** [+1.70, +2.88] | −0.04 | 78 | 76 (0.93) | 0 |
| potion-8M int8 (bundled) | off | post-hoc / pre-emptive | +1.71 / +2.29 (identical) | −0.04 | 66 / 78 | as hashed | 0 |
| potion-code-16M-v2 | off | post-hoc / pre-emptive | +1.71 / +2.29 (identical) | −0.04 | 66 / 78 | as hashed | 0 |
| hashed | on (gated) | post-hoc / pre-emptive | +1.71 / +2.29 (nothing admitted) | −0.04 | 66 / 78 | – | 0 |
| potion-8M int8 | on (gated) | post-hoc | +1.73 [+1.17, +2.33] | +0.03 [−0.04, +0.16] | 70 | 70 (0.96) | 0 |
| potion-8M int8 | on (gated) | pre-emptive | +2.31 [+1.73, +2.90] | +0.10 [−0.04, +0.34] | 82 | 80 (0.93) | 0 |
| potion-code-16M-v2 | on (gated) | post-hoc | +1.74 [+1.17, +2.34] | +0.10 [−0.03, +0.23] | 70 | 70 (0.96) | 0 |
| potion-code-16M-v2 | on (gated) | pre-emptive | +2.32 [+1.75, +2.90] | +0.16 [−0.03, +0.36] | 82 | 80 (0.93) | 0 |
| hashed | ungated (diagnostic) | post-hoc | +2.14 [+1.57, +2.73] | +2.15 [+1.14, +3.27] | 82 | 77 (0.88) | **1** |
| potion-8M int8 | ungated (diagnostic) | post-hoc | +2.13 [+1.55, +2.71] | +2.09 [+1.02, +3.21] | 80 | 76 (0.89) | **1** |
| potion-code-16M-v2 | ungated (diagnostic) | post-hoc | +2.06 [+1.48, +2.66] | +1.72 [+0.60, +2.91] | 79 | 75 (0.89) | 0 |
| potion-code-16M-v2 | ungated (diagnostic) | pre-emptive | +2.66 [+2.12, +3.21] | +1.90 [+0.84, +3.09] | 91 | 85 (0.88) | 0 |

Reading:

- **The Python repositories are untouched in every configuration.** Their value is the dead-end and
  test-target reflexes, which structure already finds. Of the store's proposals there, one per
  repository (the project runner), none is admitted, and the runtime has already spoken wherever
  one would fire.
- **What the gate admits on vue-core** is `npm test` (A lower bound +0.06 to +0.12): useful 14–15
  times of 14–15, but agents run it anyway, so it saves 2–3 calls over 16 sessions.
- **What the gate refuses** is `npm test -- packages/runtime-core` (lower bound −0.4 to −1.0). On
  replay it saves 25–32 test-discovery calls but is useful only 5–8 times of 9–13 (and is harmful
  once, hashed and bundled). The two measures disagree because the value model's local reward is
  "calls to the first test run that ran", which an area hint does not shorten. The replay counts the
  discovery searches in between. Aligning them (a discovery-aware Q for area proposals) is the one
  follow-up this ablation suggests. It needs a pre-registered test on other monorepos: vue-core's
  16 sessions and 8 tasks are the only monorepo here, and "runtime-core" is its frequency prior.
- **Agrees with** the research lab's scale-up (`evals/lab/research/case-retrieval-scale/`: +0.01
  calls/session at 554 memory sessions; a per-repository count gives +0.29) and the embedding lab's
  verdict (structure-only kNN within a point of every text model).

## 3. Prompt → area and prompt → file (hit@k), task identity

`adage eval embeddings` ([embeddings/hitk-*.json](embeddings/)): held-out control sessions with a
source edit. An area is an edited file's first two directories (vue-core `packages/*`; the Python
repositories have 1–2 source areas, so area routing is trivial there). kNN votes with the training
sessions' edits; retrieval is cosine between the prompt and the words of each area's or file's path
and file names; frequency is the training sessions' edit counts.

| vue-core (17 areas, 326 files), hit@1 / @3 / @5 | area kNN | area retrieval | file kNN | file retrieval | file hybrid |
| --- | --- | --- | --- | --- | --- |
| frequency prior (no embedding) | 0.62 / 0.62 / 0.75 | – | 0.00 / 0.06 / 0.06 | – | – |
| hashed | 0.62 / 0.75 / 0.75 | 0.38 / 0.50 / 0.50 | 0.00 / 0.06 / 0.06 | 0.00 / 0.12 / 0.25 | 0.06 / 0.06 / 0.06 |
| potion-8M (f32 and bundled int8: identical) | 0.62 / 0.75 / 0.75 | 0.25 / 0.75 / 1.00 | 0.00 / 0.06 / 0.06 | 0.00 / 0.00 / 0.00 | 0.00 / 0.06 / 0.06 |
| **potion-code-16M-v2** | 0.62 / 0.62 / 0.75 | **0.75 / 1.00 / 1.00** | 0.06 / 0.06 / 0.06 | **0.44 / 0.69 / 0.69** | 0.06 / 0.06 / 0.19 |

- **Python repositories** (file level, 19–32 source files): the best method is within ±0.25 of the
  frequency prior at every k, with no consistent winner (click file hybrid hit@5 0.88 for potion-8M
  vs 0.62 frequency; marshmallow all methods 0.83 at hit@3). Too few files for retrieval to matter.
- **kNN over past prompts never beats the frequency prior**: bug fixes do not recur in the same
  files (the lab's and the research agent's finding again).
- **Task identity** (pairs of held-out sessions grouped by prompt cosine ≥ τ, against the manifest's
  task ids). The learner's exact first-prompt rule is already perfect here (P = R = 1.00):
  RepoBench reruns a task with the same prompt. Embeddings match it at τ ≥ 0.9 and lose precision
  at τ = 0.8 (0.62–0.77). So there is no gain to take on these traces, and the learner keeps exact
  text; `cases::task_groups` is there for paraphrased real-world prompts, untested.
- **Quantisation**: bundled int8 vs f32 potion-8M, cosine over every training prompt: min 1.0000,
  median 0.99998. Every hit@k is identical.

## 4. Cost: latency and size

- **Binary.** Release `adage` (x86_64 Linux, LTO): 26,983,384 bytes, of which the bundled model is
  7,897,127 bytes (stored verbatim, so without it the binary is about 19.1 MB). The installed f32
  model it replaces as the default was 30.9 MB on disk.
- **In process** (debug build, 4 cores at load average ~25).
  - Opening the embedder: bundled 52 ms, installed f32 99 ms, code-16M 162 ms, hashed 0.02 ms.
    That is the vocabulary hash map; release is about 15× faster (installed open 3.4 ms below).
  - Embedding one prompt, p50: 0.8 ms bundled, 0.9 ms code-16M, 7.5 ms hashed (SHA-256 per
    feature, unoptimised).
- **Hook, end to end** (`crates/adage-cli/examples/hook_latency.rs`, release, 200 runs per scenario;
  [embeddings/hook-latency.json](embeddings/hook-latency.json)). This container ran at load average
  16 on 4 cores during the measurement, and the structured-route prompt took p95 33.7 ms against
  the committed 3.18 ms, so **absolute numbers are inflated about 5–10×; compare rows, not budgets**:

  | Scenario (a prompt, intent routes in the index) | p50 ms | p95 ms | p99 ms |
  | --- | ---: | ---: | ---: |
  | installed f32 model (the path S7 measured at 6.39 ms p95 and accepted) | 63.4 | 94.6 | 106.8 |
  | **bundled int8 model, nothing installed, route fires** | **51.5** | **81.1** | **87.5** |
  | bundled model, index learned with another model (no match) | 41.6 | 76.3 | 85.9 |
  | no model (hashed selected): intent routes cannot fire | 11.8 | 29.4 | 43.6 |
  | structured routes only (no embedding) | 20.0 | 33.7 | 47.3 |

  The bundled path is faster than the installed path that already met the ≤ 25 ms budget: its
  table is in the binary's pages, not read from a file. A model is only opened at a prompt in a
  repository whose index has intent routes. None formed on these repositories, so their hooks never
  pay it. The case store is not in the hook: it is off, and replay-only.

## 5. What ships

| Part | Default | How to change it |
| --- | --- | --- |
| Bundled potion-base-8M int8, resolved by hook and learner | **on** | `adage models use hashed` or `ADAGE_EMBED_MODEL=hashed` |
| npm fallback for `adage models fetch`, pinned | **on** | `ADAGE_MODEL_BASE_URL` for a mirror |
| Embedding status in `adage doctor`, the card, `adage models status` | **on** | – |
| potion-code-16M-v2 | off (fetch on request) | `adage models fetch potion-code-16m-v2 && adage models use potion-code-16m-v2` |
| Case store (`adage_learn::cases`) | **off**, replay only | `adage eval replay-savings … --models <dir> --cases on\|ungated` |
| Embedding task identity (`cases::task_groups`) | off (exact text stays) | – |

Reproduce:

```text
python3 evals/replay/manifest.py /tmp/adage-rb2/runs/{click,attrs,marshmallow,vue-core} --out <dir> --cochange
ADAGE_EMBED_MODEL=<hashed|auto|potion-code-16m-v2> adage eval replay-savings <dir>/<repo>.json --policy learned \
    --arm control --mode <post-hoc|pre-emptive> --models <models dir> --cases <off|on|ungated> --json
ADAGE_EMBED_MODEL=<…> adage eval embeddings <dir>/*.json --models <models dir>
```

([embeddings/ablate.sh](embeddings/ablate.sh), [embeddings/pool.py](embeddings/pool.py) and
[embeddings/embed_ablation.sh](embeddings/embed_ablation.sh) are the exact scripts, with the session's
scratch paths.)

## 6. Limits and follow-ups

- **Monorepos are under-sampled.** One monorepo, 16 held-out sessions, 8 tasks. The ungated area
  result is the single thing here that might become a lever, and it needs a discovery-aware value
  model plus a replay on other monorepos before any live slice.
- **Read hints are unscored.** The replay metric has no Sat class for "read these files", so the
  code model's retrieval strength cannot be priced on replay yet; the research lab's cost anatomy
  puts its ceiling at ~5% of Haiku cost on RepoBench.
- **Hook open cost.** The vocabulary hash map is built per process (29,528 entries; 63,457 for the
  code model). A sorted, binary-searched vocabulary laid out in the binary would remove most of it.
- **Latency was measured on a loaded machine.** Re-run `hook_latency` on a quiet one before quoting
  absolute numbers.
