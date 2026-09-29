# Embedding lab: case-based retrieval vs the frequent-subgraph miner

2026-09-29. An offline experiment (no live agent runs, $0): does a case-based engine, retrieving
similar *situations* from a repository's past traces with a local embedding model and predicting
what to do next, find more useful reflexes on **real repositories** than the current Rust engine,
which forms none there? Python, in this directory; nothing here touches the Rust crates.

## Verdict: reject kNN as the engine; adopt its one lesson (hybrid, and small)

- **The embedding model is not what matters.** Four real local models (potion-base-8M/32M,
  bge-small-en-v1.5, all-MiniLM-L6-v2) and a TF-IDF fallback all land within 3 points of each
  other on next-action accuracy, and a **structure-only** kNN (last actions, error/test state; no
  text model at all) is within a point of the best on 3 of 5 repositories (the text helps on
  marshmallow, +4, and vuejs/core, +7). For directives no model beats structure-only or the
  frequency baseline. The transformer models cost 400-850x the latency of the static potion models
  for nothing measurable.
- **Case-based retrieval predicts the next action a little better than n-grams**: +1 to +6
  points top-1 at the command-template level over order-1/2 on all five repositories
  (e.g. urfave-cli 0.32 vs 0.26, marshmallow 0.26 vs 0.21). That is too small, and top-1 is too low
  (0.18-0.32), to deliver as directives.
- **Useful directives at precision ≥ 0.7 exist, but they are one reflex per repository, and a
  one-line frequency count finds them as well as kNN does.** Tuned on attrs+click+marshmallow and
  applied unchanged to the two held-out repositories, the best kNN (potion-32M, usefulness-targeted,
  commands only) and the trivial baseline "the command most often useful in this repository's
  training traces, once at the start of every session" both reach the target: pooled precision
  0.71 vs 0.73 on the tuning repos; on held-out urfave-cli 0.89 vs **1.00**, on held-out vuejs/core
  0.67 vs **0.91**. What they say is the same thing: *how to run the tests here*
  (`go test`, `npm test <spec file>`, `python -m pytest <test file>`) or *install the package
  first* (`pip install -e .`).
- **Turns saved are small**: about **0.3-1.9 tool calls per session** (failed commands plus
  environment/manifest discovery steps between the directive and the agent doing it), on sessions
  of 27-38 tool calls. The loose upper bound (every wasted step in the window) is 2-15, but most
  of those are reads of the code the agent needed anyway.
- **The current engine** compiles **0 reflexes** on the same split at `default` and at `demo`
  thresholds (replayed with `adage eval`), so it delivers nothing; its own next-action model
  (`adage eval mine`) is at order-1 level on 4 of 5 repositories.
- **More history helps a little, other repositories not at all.** With every other task's
  sessions as memory (58-123 sessions per repository, leave-task-out), kNN precision at the fixed
  threshold is 0.47-0.86 and it overtakes the prior on click and marshmallow; memory from the other
  four repositories gives precision 0.00 on both held-out repositories.

**Recommendation: reject** a kNN/embedding engine as the reflex learner (no gain over a count, a
larger runtime surface, and the text model contributes nothing measurable). **Adopt, in Rust, the
usefulness target** that made the difference here (below), as a small addition to the existing
learner: learn, per repository, the *successful* command template that was most often a useful
directive in training (taken later, after a dead end or ≥ 2 exploratory steps), with its slots'
learned value types, and deliver it once per session as a `run`/`advise` reflex when its training
usefulness rate is ≥ 0.7 over ≥ 2 sessions. That is the reflex the frequent-subgraph miner misses
at default thresholds, it needs no model, and it fits the existing typed shapes. Revisit kNN
only when repositories have hundreds of sessions (the grown-memory rows are the one place it beats
the prior), and then with potion-8M (already pinned) or no text model at all.

## Data

The RepoBench v1 (`/tmp/adage-repobench`) and v2 (`/tmp/adage-rb2`) archives were still on disk:
1,783 transcript files, many of them copies. De-duplicated by session id (v2 re-used v1's training traces), per
repository: the 10 (marshmallow 7) chronological-split training sessions as memory, and every
held-out **control-arm** session (v1 + v2, Claude Haiku 4.5; hooks on, directives withheld) as the
test set; `adage`- and `agents-md`-arm sessions, which directives or an AGENTS.md steered, are used
only as extra memory in the grown-memory variant. The split is `evals/repobench/splits`; outcome
(hidden tests passed) comes from each run's `state.json`.

| repo | train sessions | train situations | held-out sessions (control) | held-out decision points |
| --- | ---: | ---: | ---: | ---: |
| attrs | 10 | 287 | 49 | 1495 |
| click | 10 | 277 | 47 | 1638 |
| marshmallow | 7 | 207 | 30 | 809 |
| urfave-cli | 10 | 273 | 16 | 467 |
| vue-core | 10 | 268 | 34 | 1288 |

## Models

Hugging Face, hf-mirror.com, ModelScope, jsDelivr and sbert.net are refused by this environment's
egress proxy (403); GitHub release pages too. The npm registry and the qdrant-fastembed bucket on
Google Cloud Storage are reachable, and they carry four of the candidates (`fetch_models.sh`):

| model | source | licence | size | dim | embed one situation, median / p95 |
| --- | --- | --- | ---: | ---: | --- |
| potion-base-8M | npm `@yarflam/potion-base-8m` 1.0.4 (**byte-identical** to the SHA-256s Adage pins) | MIT | 31 MB | 256 | 0.30 / 0.82 ms |
| potion-base-32M | npm `@yarflam/potion-base-32m` 1.1.5 | MIT | 131 MB | 512 | 0.30 / 0.69 ms |
| bge-small-en-v1.5 | GCS `qdrant-fastembed/fast-bge-small-en-v1.5.tar.gz` (ONNX) | MIT | 134 MB | 384 | 256 / 414 ms |
| all-MiniLM-L6-v2 | GCS `qdrant-fastembed/sentence-transformers-all-MiniLM-L6-v2.tar.gz` (ONNX) | Apache-2.0 | 91 MB | 384 | 114 / 205 ms |
| TF-IDF word + char 3-5-grams (fallback) | scikit-learn, fitted per repository on training situations | – | – | sparse | 2.0 / 5.9 ms |

Not obtainable here: potion-retrieval-32M, gte-small, jina-embeddings-v2-small-code, nomic-embed-text
(no reachable mirror). Latencies: ONNX Runtime CPU, 4 threads, 256-token cap, on a shared 4-vCPU
container at load average ~10, so the transformer numbers are inflated perhaps 2-3x; the ordering
(static models ~500x cheaper) is not in doubt. Brute-force kNN, single thread (`bench_knn.py`):
0.01 ms over 300 vectors (a repository's table here), 0.22 ms over 5,000 × 256, 0.86 ms over
20,000 × 256 (float32).

## Method

**Normalisation** (`normalize.py`). Each tool call becomes an action at three levels: `key`
(`Read`, `Bash:grep`, `Bash:python -m pytest`, `Bash:npm test`), `template` (key + flags + typed
slots: `Bash:python -m pytest -x {test_file}`, `Read {src_file}`) and `full` (concrete
repository-relative arguments). Run-specific sandbox roots are stripped; heredocs and `-c` scripts
are one argument; `cd …&&`, `. .venv/bin/activate &&`, env assignments and pipes are peeled off.
A *directive* is matched at `directive_key`: a command's key plus its file slots (flags and free
text are the agent's choice), or a file read/edit's concrete path. Exploratory shell
(grep/find/cat/ls/sed/git log…) and destructive commands (rm, mv, git reset/checkout/push…) are
never directives.

**Situations** (`situations.py`). A decision point follows the prompt and each tool batch (one
assistant message's calls). Its situation: the prompt (600 chars); the last 5 actions with their
failures and the first error line of recent failures; and the outcome so far (edited yet, tests
run, last test failing, number of failures, phase). Two texts are embedded, prompt and recent
history, plus a structural record (last key, key set, the five flags).

**Predictor** (`predict.py`). Similarity = w_p·cos(prompt) + w_h·cos(history) + w_s·structural.
The top-k training situations vote, weighted by similarity² × (1 if that session passed its hidden
tests, else 0.5). Next action: the weighted majority per level. Directive candidates, two targets:

- `next`: the eligible actions a neighbour took **successfully** in its next 5 steps (decayed);
- `useful`: the actions that *would have been useful directives at the neighbour's own situation*:
  taken later, successfully, after a failed command or ≥ 2 exploratory steps. Confidence is then
  the similarity-weighted share of neighbours for which that directive would have paid off, a
  direct estimate of P(useful | situation).

A candidate needs support from ≥ 2 distinct training sessions; below the threshold θ the
predictor abstains. Baselines: order-0/1/2 Markov models over templates (with back-off) that emit
next actions and directive candidates the same two ways, so every comparison has the same
candidate generation and safety filters; only the notion of "situation" differs.

**Useful directive** (`metrics.py`), as specified: a directive at step i naming action a is
*useful* if the agent later took a (at t' ≥ i) and [i, t') holds a failed command or ≥ 2
exploratory steps; *redundant* if it took a with no waste in between; *wrong* otherwise. Issued at
most once per session per action. Precision = useful / issued. Recall = useful distinct actions /
the session's opportunities (distinct eligible actions some occurrence of which was preceded by
waste since the session start or its previous occurrence). Turns saved, per session, over the union
of useful directives' windows: **failed commands** (conservative), **discovery** (failed commands
plus exploratory steps about how to work in the repository: manifests, tool config, `.venv`,
`which`, `pip list`, `--version`; the headline), and every wasted step (a loose upper bound).
Strict precision counts a directive as useful only when a failed command was in its window.

**Protocol.** 432 kNN configurations (5 text models + structure-only; 8 weightings; k ∈ {10, 20,
40}; both targets; all directives or commands only) and 12 baselines were scored on attrs, click
and marshmallow; per method the threshold is the lowest θ whose **pooled** precision is ≥ 0.7 (≥ 10
directives). Those parameters and thresholds are then applied **unchanged** to urfave-cli (Go) and
vuejs/core (TypeScript), which took no part in tuning. The current engine was replayed on the same
split with `adage eval <dataset> --split chrono --thresholds default|demo` and measured with
`adage eval mine` (`engine.py` builds the datasets in a scratch dir).

## Results

### (a) Next-action top-1 accuracy (held-out control sessions; key / template / full)

Best configuration per text model by mean template accuracy on the tuning repositories.

| method | attrs | click | marshmallow | urfave-cli | vue-core |
| --- | ---: | ---: | ---: | ---: | ---: |
| order-0 (most frequent) | 0.19 / 0.10 / 0.08 | 0.20 / 0.13 / 0.05 | 0.13 / 0.06 / 0.11 | 0.28 / 0.16 / 0.13 | 0.29 / 0.17 / 0.05 |
| order-1 | 0.26 / 0.18 / 0.13 | 0.25 / 0.19 / 0.08 | 0.27 / 0.21 / 0.12 | 0.43 / 0.26 / 0.14 | 0.36 / 0.17 / 0.06 |
| order-2 | 0.26 / 0.19 / 0.13 | 0.24 / 0.18 / 0.08 | 0.27 / 0.20 / 0.12 | 0.41 / 0.26 / 0.10 | 0.36 / 0.22 / 0.05 |
| kNN structure-only (no text model) | 0.31 / 0.20 / 0.14 | 0.32 / 0.20 / 0.11 | 0.32 / 0.22 / 0.16 | 0.51 / 0.32 / 0.12 | 0.43 / 0.17 / 0.07 |
| kNN TF-IDF | 0.27 / 0.19 / 0.14 | 0.34 / 0.21 / 0.13 | 0.32 / 0.24 / 0.18 | 0.50 / 0.31 / 0.12 | 0.43 / 0.23 / 0.10 |
| kNN potion-8M | 0.28 / 0.18 / 0.13 | 0.32 / 0.21 / 0.12 | 0.33 / 0.24 / 0.16 | 0.47 / 0.32 / 0.14 | 0.43 / 0.23 / 0.11 |
| kNN potion-32M | 0.28 / 0.18 / 0.13 | 0.32 / 0.21 / 0.11 | 0.33 / 0.26 / 0.18 | 0.48 / 0.31 / 0.14 | 0.44 / 0.22 / 0.10 |
| kNN bge-small | 0.29 / 0.18 / 0.13 | 0.30 / 0.20 / 0.12 | 0.32 / 0.24 / 0.17 | 0.46 / 0.31 / 0.13 | 0.44 / 0.21 / 0.09 |
| kNN MiniLM-L6 | 0.27 / 0.18 / 0.13 | 0.32 / 0.21 / 0.12 | 0.31 / 0.24 / 0.17 | 0.48 / 0.31 / 0.13 | 0.42 / 0.24 / 0.09 |

The current engine's next-action model (`adage eval mine`, same split) scores in its own event
alphabet over exec/read/edit positions, so its numbers compare with its own baselines, not with
the rows above: engine / its order-1 = attrs 0.42 / 0.41, click 0.21 / 0.22, marshmallow 0.61 /
0.60, urfave-cli 0.18 / 0.17, vuejs/core 0.55 / 0.31. Like kNN it adds little over order-1 except
on one repository.

### (b) Useful directives, precision ≥ 0.7 operating point

Pooled over the three tuning repositories, at each method's tuned θ (126 sessions):

| method | θ | directives | precision | strict precision | failed cmds avoided/session | discovery turns saved/session |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| baseline: most-often-useful command, once per session (order-0, `useful`, commands) | 0.05 | 126 | 0.73 | 0.40 | 0.96 | 1.02 |
| kNN structure-only (k 20, `useful`, commands) | 0.65 | 125 | 0.71 | 0.50 | 0.99 | 1.03 |
| kNN TF-IDF (0.2/0.3/0.5, k 40) | 0.60 | 103 | 0.72 | 0.56 | 0.86 | 0.88 |
| kNN potion-8M (prompt 0.5, history 0.5, k 20) | 0.70 | 103 | 0.72 | 0.59 | 0.88 | 0.97 |
| **kNN potion-32M (prompt 0.5, history 0.5, k 20)**, chosen | 0.65 | 119 | 0.71 | 0.60 | 1.04 | 1.14 |
| kNN bge-small (prompt 0.5, struct 0.5, k 10) | 0.75 | 113 | 0.71 | 0.52 | 0.91 | 0.93 |
| kNN MiniLM-L6 (history 0.5, struct 0.5, k 40) | 0.60 | 88 | 0.74 | 0.65 | 0.76 | 0.79 |
| `next` target (what neighbours did next), best of 144 configurations | – | ≥ 10 | ≤ 0.55 | | | |
| current Rust engine (replay, default and demo) | – | 0 | – | – | 0 | 0 |

All 62 kNN configurations that reached 0.7 used the `useful` target, and 59 of them delivered
commands only: file-read and file-edit directives rarely pay off (a new bug's files are not in ten
other bugs' traces). With the `next`
target, confidence is anti-calibrated: the confident predictions are the obvious next steps, which
the agent takes immediately (redundant), so precision *falls* as θ rises.

Held-out repositories, same parameters and θ (no tuning on them):

| repo | method | directives/session | precision | strict | failed cmds avoided/session | discovery saved/session | window upper bound | steps/session |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| urfave-cli | baseline (once per session) | 1.00 | **1.00** | 0.56 | 0.56 | 0.56 | 13.4 | 29.2 |
| urfave-cli | kNN potion-32M (chosen) | 1.12 | 0.89 | 0.50 | 0.56 | 0.56 | 13.4 | 29.2 |
| urfave-cli | kNN structure-only | 1.19 | 0.84 | 0.47 | 0.56 | 0.56 | 13.4 | 29.2 |
| urfave-cli | kNN bge-small | 1.19 | 0.89 | 0.53 | 0.69 | 0.69 | 13.7 | 29.2 |
| vue-core | baseline (once per session) | 1.00 | **0.91** | 0.24 | 0.35 | 0.38 | 13.2 | 38.0 |
| vue-core | kNN potion-32M (chosen) | 1.62 | 0.67 | 0.22 | 0.82 | 0.85 | 15.3 | 38.0 |
| vue-core | kNN structure-only | 1.21 | 0.78 | 0.20 | 0.35 | 0.38 | 13.2 | 38.0 |
| vue-core | kNN bge-small | 1.38 | 0.77 | 0.21 | 0.71 | 0.74 | 14.2 | 38.0 |
| both | current Rust engine | 0 | – | – | 0 | 0 | 0 | |

What was said (held-out sessions; useful / issued): urfave-cli `go test` 16/16 (all methods);
vuejs/core `npm test {test_file}` 31/34 (baseline), 25/27 plus `npm test {test_dir}` 3/13
(potion-8M); click `python -m pytest {test_file}` 40/47 (baseline); marshmallow `pip install -e .`
24/30 and a `python3 <<heredoc` repro script (kNN); attrs `python -m pytest {test_file}` 29/49
(baseline). Recall is 0.05-0.13 of the sessions' opportunities throughout.

### (c) The current engine on the same split

`adage eval` (learned routes only; instincts off) on the same train/test sessions: **0 routes, 0
directives** in all five repositories at `default` and at `demo` thresholds, so its useful-directive
precision is undefined and its turns saved are 0. (`adage eval mine` does find 12-46 candidate
patterns per repository when mining every session in-sample, of every shape; none becomes a route
from the training side.) The
instinct U11 (`.venv` test runner), which the launch report measured live, is not a learned reflex
and is not replayed here: the archived checkouts no longer have their `.venv`, so it binds
differently offline.

### Grown memory and cross-repository memory

Chosen kNN and the baseline at their fixed thresholds, memory = training + every other task's
held-out sessions (all arms), leave-task-out:

| repo | sessions | kNN directives/session | kNN precision | kNN discovery saved | baseline precision | baseline discovery saved |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| attrs | 123 | 1.47 | 0.47 | 1.16 | 0.53 | 1.69 |
| click | 120 | 1.64 | 0.52 | 1.66 | 0.47 | 0.87 |
| marshmallow | 79 | 1.87 | 0.62 | 1.70 | 0.37 | 1.03 |
| urfave-cli | 58 | 1.31 | 0.86 | 0.56 | 1.00 | 0.56 |
| vue-core | 94 | 1.26 | 0.77 | 0.41 | 0.91 | 0.38 |

With 8-12x the memory, kNN finds more directives per session and beats the prior on click and
marshmallow (where the single most-useful command drifts once more tasks are in memory), but the
threshold tuned on ten sessions no longer yields 0.7 on the Python repositories: it would need
re-tuning as memory grows. Cross-repository memory (the other four repositories' training traces)
gives precision **0.00** on both held-out repositories: what is useful is repository-specific.

## Typed output and a Rust integration sketch

`compile.py` turns a retrieved-case prediction into Adage's delivery shapes without giving up the
safety properties:

- **Only commands agents ran successfully** can be directives: a candidate is an action some
  training session ran without failure, supported by ≥ 2 sessions; destructive commands never.
- **Slots come from learned value types**: each slot's type is learned from the concrete values
  that sink took in training (`python -m pytest {test_file}` → `tests/**/test_*.py`;
  `npm test {test_file}` → `packages/**/*.spec.ts`; `pip install {dir}` → the constant `.`). At
  delivery a slot binds only to a path from the session's own recent actions that the type
  accepts; when nothing binds yet, the step becomes fixed `advise` text with the typed
  placeholder, never a guessed value; a slot with no learned type is never delivered.
- **Shape from the situation**: after a failure → `avoid` (not that, this); after an edit when the
  step is a test run → `obligation`; at the prompt → `run` (or `read` for a file); otherwise
  `after`. Examples it produced on held-out sessions:

```json
{"shape": "run", "trigger": {"on": "prompt"}, "step": {"run": "go test", "slots": {}}, "strength": "suggest", "evidence": {"confidence": 0.811, "sessions": 8}}
{"shape": "advise", "trigger": {"on": "prompt"}, "step": {"advice": "In this repository, run `npm test <test_file: packages/**/*.spec.ts>`", "slots": {"{test_file}": {"type": "packages/**/*.spec.ts", "value": null}}}, "strength": "suggest", "evidence": {"confidence": 0.707, "sessions": 8}}
{"shape": "run", "trigger": {"on": "prompt"}, "step": {"run": "pip install {dir}", "slots": {"{dir}": {"type": "enum:.", "value": "."}}}, "strength": "suggest", "evidence": {"confidence": 0.653, "sessions": 5}}
{"shape": "avoid", "trigger": {"on": "PostToolUseFailure", "command": "python3 <<heredoc"}, "step": {"run": "pip install", "slots": {}}, "strength": "suggest", "evidence": {"confidence": 0.752, "sessions": 2}}
```

If kNN were ever adopted, the Rust side is small: at learn time (`adage-graph`), store per
repository a neighbour table: each training situation's two potion-8M vectors (quantised to i16,
as `adage_graph::embed` already stores node vectors), its structural record and its candidate
directive keys, each already compiled to a typed step. About 1 MB per 300 situations at 512
dimensions (0.3 MB with potion-8M and i16). At hook time: two static-model embeddings (0.3 ms
each), a brute-force dot product over ≤ a few thousand vectors (0.01 ms at 300, 0.2 ms at 5,000,
single thread), a vote, then the same typed-step rendering the hook already does: 0.8 ms median,
1.1-3.3 ms p95 end to end in Python (`compile.py`), well inside the hook's 25 ms budget. The
cheaper exact alternative, a situation-hash table (last action template + state flags →
precomputed prediction), is the structure-only row above and scores the same. **What should be
ported instead is the usefulness target**: at learn time, label each training decision point's
"would-have-been-useful" commands (successful, taken after a dead end or ≥ 2 exploratory steps,
support ≥ 2 sessions, rate ≥ 0.7), and compile the top one per repository to a `run`/`advise`
route with learned slot types. That is a counting pass over events the learner already has.

## Caveats

- Small memory: 7-10 training sessions per repository, one model (Haiku 4.5), bug-fix tasks. The
  held-out precision on urfave-cli rests on 16 sessions and one directive.
- "Useful" is the specified proxy. It credits "run the tests" when the agent ran them after
  exploring; that directive may not change what the agent does (it would run tests anyway), which
  is why the discovery and failed-command estimates, not the window upper bound, are the headline.
  Only a live A/B run would show real savings.
- The control arm's sessions ran with hooks installed but directives withheld; training traces
  ran without hooks. `adage`-arm sessions (which received the U11 instinct) are excluded from
  testing.
- Latencies were measured on a shared, oversubscribed container.

## Reproduce

```sh
bash evals/lab/fetch_models.sh                         # models -> $ADAGE_LAB_MODELS (default /tmp/adage-lab-models)
cargo build --release -p adage-cli
python3 evals/lab/engine.py                            # current engine on the same split -> results/engine.json
python3 evals/lab/lab.py                               # the lab (about 10 min with a warm embedding cache) -> results/lab.json
python3 evals/lab/report.py                            # every table, from the two JSON files
python3 evals/lab/compile.py --repo vue-core           # typed directives + hook latency
python3 evals/lab/bench_knn.py                         # brute-force kNN cost
```

Files: `traces.py` (loading), `normalize.py`, `situations.py`, `predict.py` (kNN and n-gram
predictors), `metrics.py`, `embed.py` (model wrappers and cache), `lab.py` (protocol),
`engine.py`, `compile.py`, `report.py`, `bench_knn.py`. Models, embedding caches, datasets and raw
transcripts are never committed.
