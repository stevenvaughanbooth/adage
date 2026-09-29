> Adversarial architecture review, 2026-09-29. Read-only research pass over the RepoBench traces plus prior art. Its recommendations drive the next engine iteration.

# Adage on real repositories: where the waste is, what prior work shows, and what to build next

## 0. Verdict

The NO-GO is correct, and the cause runs deeper than the three bugs. Adage's core, frequent closed-episode mining over what agents usually did, gated by support and rate, learns the wrong thing on real repositories:

- **Frequency is not value.** In the training traces, 16 of 23 recoveries from the `.venv` dead end were `pip install` into the system Python (unsafe). Only 3 used the `.venv`. Imitating the majority would teach the harmful action.
- **Agents rarely repeat a command exactly.** 74% of test command strings are distinct.
- **Real traces are sparse.** There are 7–10 sessions per repo, against `min_support = 5` and `prime_rate = 0.5` (`crates/adage-tracks/src/params.rs:129-131`).

The trace data below shows the waste a deterministic hook can reach on these repos is about 10–20% of cost at best. Almost all of it is environment/test-invocation friction and context bloat, not hidden procedures.

The design I recommend is an affordance map computed statically and verified by execution, which traces then calibrate. Directives would be delivered pre-emptively at `PreToolUse`, not after the failure. Recurring-sequence mining (the current engine) would be demoted to a verifier. Embeddings are off the critical path.

## 1. The value source, measured from the RepoBench traces

**Method.**
- **Data.** I parsed every `stream.jsonl` under `/tmp/adage-rb2/runs/*/runs/*` (v2 held-out: 164 Haiku 4.5 and 32 Sonnet 5 sessions, 60-turn cap) and `/tmp/adage-repobench/runs/*/runs/*` (v1: 275 Haiku sessions, 30-turn cap, censored).
- **Classification.** Each tool call is classed as read / search / test / env / repro / edit / vcs / other with regex rules. Waste is flagged from the tool result: env dead end, failed call, re-read, scratch file, rerun without an edit.
- **Cost.** Each API call's cost (Haiku $1/$2 (1h cache write)/$0.10/$5 per MTok; Sonnet ×3) is split evenly over the tool calls it emitted. This attributes about 75% of reported `total_cost_usd`; the rest is calls with no tool use.
- **Context carry.** The "carry" cost of an output is its tokens × (cache-write price + cache-read price × the number of later calls).
- **Localisation and tests.** These use the task's `src_files` and `test_files` from `evals/repobench/tasks/*.json`.
- **Scripts** are in the session scratchpad (`waste.py`, `agg*.py`, `cochange.py`). They are heuristic; treat the shares as ±2 points.

### 1.1 Waste taxonomy (v2 Haiku held-out, 164 sessions)

Baseline: 36.0 tool calls and $0.297 per session, about $0.0082 per call.

| Bucket | Calls/session | Share of calls | Sessions affected | Can a deterministic hook remove it? |
|---|---:|---:|---:|---|
| **Env/test-invocation friction** (dead end, setup, calls until the first test run that actually ran) | 3.14 | 8.7% | 87% hit a dead end | **Yes, mostly.** Signatures: `No module named pytest` 125×, project package missing from the system Python via repro scripts (`click`, `marshmallow`, `attrs`) 95×, test dependencies missing (`simplejson`, `hypothesis`) 38×, node `Cannot find module` 18× |
| Test runs (all) | 7.21 | 20.0% | – | Mostly productive. 86% are narrowed by path/`::`/`-k`, 51% piped (Sonnet: 98%) |
| Test discovery (searches mentioning tests before the first targeted run) | 2.04 | 5.7% | – | Partly, with a file→test map (below). Some of these are productive reads |
| Repro scripts (`python -c`, scratch `.py`) | 4.86 | 13.5% | 46% of sessions have ≥5 | No: this is legitimate reproduction. Sonnet uses 1.3 |
| `git add` / `git commit` | 1.63 | 4.5% | – | Yes, but it is a harness/user preference, not a learned reflex |
| `git status`/`diff`/`log` | 2.33 | 6.5% | – | No (cheap and useful) |
| Re-read of an unchanged file | 2.90 | 8.0% | 72% | **Mostly not waste.** Only 0.75/session re-read a range contained in an earlier read (2%). The rest read another part of the file |
| Tool-usage errors (bad `Read`/`Edit`) | 0.52 | 1.5% | – | Marginal |
| Reruns of an identical test with no edit in between | 0.1–0.2 | 0.3–0.5% | – | Negligible |
| **Localisation** (calls before first reading a gold source file) | **median 1**, mean 2.1 | – | gold file edited in 99% | **No headroom here.** Caveat: RepoBench prompts were written from the commit message and test diff, and the repos are small |
| **Post-last-edit tail** | 12.0 (Sonnet 3.7) | 33% | – | Only its git/cleanup part. Composition: tests 35%, git 18%, repro 16% |

Context and money:

| Measure | Haiku | Sonnet |
|---|---:|---:|
| Carried context cost of tool outputs per session | $0.084 (28% of cost) | $0.146 (25%) |
| …from file reads | 76% of carry (≈12.3k tokens/session) | 72% |
| Carry removed if read/test/search outputs were capped at 2k tokens (upper bound, ignoring compensating calls) | $0.034 (11%) | $0.037 (6%) |
| Share of spend in failed sessions | 48% (37% of sessions failed; median 47 calls vs 28.5 for passes) | 69% |

### 1.2 What this means

- **Failure dominates cost.** $ per resolved task is driven by resolve rate and by failed sessions' long flails. No reflex in Adage's typed vocabulary addresses "this approach is wrong". This is why session-level metrics never move.
- **The reachable waste is small.** Environment/invocation friction (≈3 calls ≈ $0.025) plus context bloat (≤ $0.03) is roughly 10–20% of session cost at the very best. Realistically, after partial follow and compensating behaviour, it is 5–10%. The one delivered instinct measured −1.85 calls (`evals/launch/README.md` §1), consistent with this ceiling.
- **Localisation and navigation are not where Haiku wastes turns on these repos.** Repo maps and localisation hints have near-zero headroom here. That could change on large monorepos or with vaguer prompts; vue-core is the only candidate, and its first-edit median was 11.5–14.5 calls.
- **The hook timing matters.** Most friction happens *before* anything fails visibly: 51% of Haiku test commands and 98% of Sonnet's are piped (bug 2). A directive that fires after a failure gives up the failing call and misses a third of the cases. Claude Code's `PreToolUse` hook can rewrite a call's input (`updatedInput`) or deny it with a reason, and `PostToolUse` supports `updatedToolOutput` (per the [hooks reference](https://code.claude.com/docs/en/hooks); check for Bash specifically). **Intercepting before the call is strictly better than advising after the failure** for every dead end in the table.
- **Agents' own recoveries are bad teachers.** In the training traces a learner sees, recoveries from the env dead end were 16 `pip install` into the environment, 3 `.venv`, and 4 other. In v2 control: 35 pip, 15 `.venv`, 8 other. The instinct written from static knowledge beat everything the traces contained.

## 2. Prior art, with measured effects

| Work | Mechanism | Measured effect | Lesson for Adage |
|---|---|---|---|
| **AWM** ([Wang et al. 2024](https://arxiv.org/abs/2409.07429)) | LLM-induced workflows from successful trajectories, injected into the prompt | +51.1% relative success on WebArena, +24.6% on Mind2Web | Works where tasks repeat on the same site. The workflow is abstracted by an LLM, not mined literally |
| **ReasoningBank** ([2509.25140](https://arxiv.org/abs/2509.25140), [Google blog](https://research.google/blog/reasoningbank-enabling-agents-to-learn-from-experience/)) | Strategies distilled from successes **and failures**, retrieved by embedding | SWE-bench Verified: about 2.8 fewer steps than no memory, 1.3 fewer than other memories | About 3 steps, the same order as U11's −1.85 calls. Learning from failures matters |
| **Budget-matched critique** ([Hajimiri et al. 2026, 2606.15017](https://arxiv.org/abs/2606.15017)) | Gives the plain agent the same token budget | Vanilla matches or beats AWM, ASI and ReasoningBank on WebArena, often with fewer tokens. Run-to-run variance is material | Any Adage claim must be budget-matched, and must count directive tokens as cost |
| **Skills in the wild** ([2604.04323](https://arxiv.org/abs/2604.04323)) | Agents retrieve skills from 34k real skills | Gains fall to the no-skill baseline in the most realistic setting | Retrieval noise wipes out the gains, so precision gates are right. Adage's problem is recall, not precision |
| **Procedural memory retrieval benchmark** ([2511.21730](https://arxiv.org/abs/2511.21730)) | Embedding vs LLM-abstracted procedure retrieval | Embeddings act as bag-of-words, fine on familiar contexts and degrading on novel ones. LLM abstractions transfer | Don't expect embeddings to generalise procedures. Use them for familiar-context matching only |
| **SWE-Exp** ([2507.23361](https://arxiv.org/abs/2507.23361)), **STAIR** ([2607.29658](https://arxiv.org/abs/2607.29658)), **Agent KB** ([2507.06229](https://arxiv.org/abs/2507.06229)), **Memp** ([2508.06433](https://arxiv.org/abs/2508.06433)) | Experience banks and hierarchical trajectory abstraction | SWE-Exp: 73.0% SWE-bench Verified with Claude 4 Sonnet. STAIR: 79–81% with a strong scaffold. Agent KB: OpenHands +4.0 points | Gains come from issue-type *repair knowledge* across many repos, not repo conventions. They are LLM-authored and non-deterministic |
| **Evo-Memory / ExpRAG** ([2511.20857](https://arxiv.org/abs/2511.20857)) | kNN retrieval of past trajectories as demonstrations | Simple kNN (ExpRAG) is a strong baseline; ReMem (refine) is better | Case-based retrieval is the baseline to beat, and Adage never ran it |
| **Gloaguen et al. 2026** ([2602.11988](https://arxiv.org/abs/2602.11988)) | AGENTS.md: none vs LLM-generated vs human-written | LLM-generated: −3% success and >20% more cost. Human-written: +4% success, still >20% more cost | Minimal, accurate instructions only. Every token of guidance has a cost |
| **Lulla et al. 2026** ([2601.20404](https://arxiv.org/abs/2601.20404)) | AGENTS.md on 124 PRs | Median runtime −28.6%, output tokens −16.6% | Efficiency wins exist on focused tasks, mostly from build and test instructions |
| **Probe-and-Refine** ([2606.20512](https://arxiv.org/abs/2606.20512)) | Iteratively tunes the guidance file with synthetic bug-fix probes | SWE-bench Verified 25.5% → 33.0% | Validating guidance against executed probes beats counting frequencies. `adage train` is halfway there |
| **Adage's own AGENTS.md arm** (RepoBench v1) | Committed conventions file | $0.42 vs $0.42 per resolved task | On these repos even written conventions don't move session metrics. The headroom really is small |
| **AgentDiet** ([2509.23586](https://arxiv.org/abs/2509.23586), FSE 2026) | Removes useless, redundant and expired content from the trajectory | Input tokens −39.9–59.7%, cost −21–36%, same success | The largest $ lever in the literature is **context**, not procedure |
| **"How do AI agents spend your money?"** ([2604.22750](https://arxiv.org/abs/2604.22750)) | OpenHands on SWE-bench Verified, 8 models | Input tokens dominate cost; 40–60% of input tokens are removable; runs of the same task vary 30× | Agrees with the carry measurement in §1.1. The variance explains the A/B power problem |
| **HarnessFix** ([2606.06324](https://arxiv.org/abs/2606.06324)) | Diagnoses harness flaws from failed traces | +6.3–18.4% | Trace-grounded diagnosis of the *harness and environment* is where traces pay off |
| **aider repo map** ([docs](https://aider.chat/docs/repomap.html), [design](https://aider.chat/2023/10/22/repomap.html)) | tree-sitter defs/refs + PageRank, token-budgeted | – | A good static map, but localisation has no headroom here |
| **Test-impact tools**: [pytest-testmon](https://github.com/tarpas/pytest-testmon), [jest `--findRelatedTests`](https://jestjs.io/docs/cli), [`vitest related`](https://vitest.dev/guide/cli), Nx affected, Bazel | Coverage or import graph maps a changed file to its tests | Deterministic and exact | This is the right shape for Adage's parametric test reflex. See the co-change result below |
| **Process mining, next-activity prediction** (Tax et al. 2017; [RAG for next activity](https://www.sciencedirect.com/science/article/pii/S0306437925001280)) | LSTM / GNN / retrieval over event logs | About 0.72–0.78 accuracy on standard logs, which have thousands of cases | Needs orders of magnitude more cases than 10–60 sessions. On small logs, retrieval and prefix baselines are what is feasible |

**What to take from this:**
1. Validate candidate guidance by executing it, as Probe-and-Refine and `adage train` do, instead of requiring n ≥ 5 traces.
2. Learn from failures and contrasts, not only from majorities (ReasoningBank).
3. Treat context reduction as a first-class target (AgentDiet).
4. Report budget-matched results and count directive tokens as cost (Hajimiri).
5. Use test-impact style mappings for "which test".

**What not to take:** LLM-authored free-text memory. It breaks Adage's determinism and typed boundary, and its gains on benchmarks with repeated tasks do not transfer to one repo with 10 distinct bugs.

## 3. Challenging the architecture

### 3.1 Why frequent-subgraph mining with strict gates is the wrong core here

1. **The support math.** Real repos give 7–10 training sessions. A route needs ≥ 5 supporting sessions at a 0.5 rate. Only habits every session shares can pass. Those are exactly what a context file or the model already knows (`cargo fmt` after Rust edits, from the dogfood traces).
2. **The literal-dominance requirement.** Directed items must be one command covering ≥ 80% of the family (`track-g-graph-engine.md` §2). Agents narrow 86% of test runs, and 74% of test strings are unique. The covering-variant fix helps only when some trace ran the repo-wide form.
3. **It imitates behaviour instead of optimising it.** Support counts what agents did, and on real repos that is often wrong (pip installs). The `outcome_lift` path exists but needs more data than exists.
4. **The value is parametric and relational**: interpreter × runner × edited area → test file. A pattern language over prototype labels encodes this poorly. A typed function (a map from edited path to test targets) encodes it exactly.
5. **It fires at the wrong time.** Reflexes trigger after an observable event, but the cheapest intervention is before the bad call.

Engineering quality is not the issue: gates, determinism and the typed compiler are all good. The problem is that the *generator* of candidates is frequency.

### 3.2 Option (a): case-based retrieval

**Idea.** Key each decision point by (phase, last action class, failure signature, edited area, prompt vector). Retrieve the k nearest past situations in this repo. Propose the next action that successful neighbours took, weighted by outcome and by calls-to-goal.

- **Pros:** it needs no dominance, so parametric actions survive (neighbours ran `.venv/bin/python -m pytest tests/test_x.py`, and the slot is abstracted). Confidence can come from agreement and outcome. Explanations are natural ("3 of 4 similar sessions did X next and reached a passing test run 2 calls later"). Deterministic exact kNN over i16 vectors of a few thousand situations takes microseconds.
- **Cons, specific to 10 click sessions:** neighbours are different bugs. The only situations that recur across tasks are environment and invocation situations. At those points the neighbours' majority action is the wrong one (pip install) unless outcome-weighted, and outcome labels (task passed) are confounded with task difficulty. Weight instead by *local* outcome: did the next test run execute tests, and how many calls did it take. With 10 sessions, CBR would at best rediscover U11 and the test-file choice, which static analysis gives on day 0.
- **Verdict:** a good stage-3 generalisation mechanism. Not the first thing to build.

### 3.3 Option (b): an affordance map, static plus trace-refined

Contents, per repo:
- **Environments:** interpreters (`.venv`, `uv`, `tox` environments), package manager, `node_modules/.bin`.
- **Invocations per area:** test, build, lint and generate commands from `pyproject`/`tox.ini`/`noxfile`/`Makefile`/`package.json`/CI workflows, each **verified by running it once in a sandbox**. For tests, `--collect-only` style checks are enough.
- **A file→test map.**
- **A failure-signature → correct-invocation table:** today's instincts.

**Measured file→test headroom.** For the 45 held-out tasks across all five repos, I ranked test files by how often they co-changed with the gold source file in git history before the task's base commit (commits of < 40 files):

| Repo | Tasks | Co-change hit@1 | hit@3 | hit@5 | Naming rule (`test_<stem>`) | Gold source file seen in training tasks |
|---|---:|---:|---:|---:|---:|---:|
| click | 8 | 0.75 | 0.88 | 0.88 | 0.50 | 0.62 |
| attrs | 12 | 0.67 | 0.75 | 0.83 | 0.58 | 0.50 |
| marshmallow | 6 | 0.67 | 1.00 | 1.00 | 0.67 | 1.00 |
| urfave-cli | 9 | 0.78 | 0.78 | 0.89 | 0.22 | 0.89 |
| vue-core | 10 | 0.70 | 0.80 | 0.90 | 0.50 | 0.40 |
| **All** | **45** | **0.71** | **0.82** | **0.89** | **0.49** | **0.64** |

This is deterministic, works on day 0, and beats anything learnable from 10 traces: 36% of gold files never appear in training traces at all. Import graphs and pytest-testmon coverage can be fused in later. Traces then **calibrate** the map: which invocations succeeded, which signatures occurred, whether agents pipe output (suggesting `-q --tb=short`), and whether commits are wanted.

### 3.4 Recommendation: (c), a hybrid in which traces verify and do not generate

- **Generation:** the static affordance map plus the instinct catalogue, and optionally kNN proposals (stage 3).
- **Admission:** a candidate enters when (i) it is statically derived and verified by sandbox execution, or (ii) at least one trace shows it succeeding **and** it replays successfully, or (iii) CBR proposes it with outcome-weighted agreement. Frequency gates stay only for pure habit reflexes (obligations).
- **Keep:** the typed shapes, effect ceilings, determinism rules and review.
- **Change the follow judge** to an equivalence class (same interpreter and runner, arguments free), which fixes bug 1. Match failure signatures in the output of exit-0 calls, which fixes bug 2.

### 3.5 What 10 click sessions can teach under this design

1. **The working interpreter.** The static prior is `.venv` with `pyvenv.cfg`. Evidence: 1 trace where `.venv/bin/python -m pytest` printed `collected N items` and 5 traces where system `python -m pytest` failed. Admit it at n = 1 because it is verified.
2. **The dead-end family beyond pytest.** Repro scripts failed with `No module named 'click'` (bug 5). Delivered pre-emptively: a `PreToolUse` rewrite of `python`/`python3` → `.venv/bin/python` when the call imports the project, or deny-with-reason.
3. **Test-dependency gaps** (`simplejson`, `hypothesis` missing). Direct "use `.venv`" (which has them), never "pip install".
4. **Area → test confirmations** for the co-change map.
5. **Output norms.** Agents pipe to `head` and `tail`, so suggest or rewrite to `-q --tb=short` to shrink carry.

The current miner can learn none of these from those traces, except item 1 in the zero-config instinct form.

### 3.6 The remaining constraints

- **Latency:** all map lookups are O(1) per event. CBR is exact kNN over ≤ 10k × 256 i16 in under 1 ms.
- **Safety:** proposals always pass the typed compiler. Rewrites only change the interpreter prefix, never add installs.
- **Explainability:** "verified: `.venv/bin/python -m pytest` collects 1,203 tests; system python lacks pytest" beats a Wilson bound.

## 4. Embeddings done right with a local model

**Where they matter.** None of the measured value sources needs them:
- Commands and paths are structured. Tokenised argv plus hashed features (current) and the repo graph (imports, co-change) beat semantic vectors for these.
- Error messages are best handled by normalised signatures; embeddings are only an offline aid for clustering families.
- Prompts matter only for (i) task identity (`engine::one_task` uses the first prompt today) and (ii) CBR and prompt→area routing in monorepos (vue-core `packages/*`).

The procedural-retrieval benchmark predicts bag-of-words behaviour anyway.

**Candidates** (Apache-2.0 or MIT only; check each model card before shipping):

| Model | Params / dims | Licence | Runtime | CPU latency per short text | Use |
|---|---|---|---|---|---|
| potion-base-8M (current) | ~7.5M, 256 | MIT | static lookup (in-house, [model2vec-rs](https://github.com/MinishLab/model2vec-rs)) | microseconds | prompts in the hook |
| potion-base-32M / potion-retrieval-32M | 32M, 512 | MIT | static | microseconds | better prompt retrieval |
| [potion-code-16M(-v2)](https://huggingface.co/minishlab/potion-code-16M) | 16M, 256, distilled from CodeRankEmbed | MIT | static | microseconds | prompt→file/symbol routing: **best fit for the hook** |
| bge-small-en-v1.5 / gte-small / e5-small-v2 | 33M, 384 | MIT | ONNX via `ort`/[fastembed-rs](https://github.com/Anush008/fastembed-rs), candle | ~5–20 ms, plus 100–300 ms model load per process | offline learner only |
| snowflake-arctic-embed-xs | 22M, 384 | Apache-2.0 | ONNX | ~3–10 ms | offline |
| nomic-embed-text-v1.5 / CodeRankEmbed | 137M, 768 (Matryoshka) | Apache-2.0 / MIT | ONNX | 30–80 ms | offline code retrieval only |
| jina-embeddings-v2-base-code | 161M, 768, 8k context | Apache-2.0 | ONNX (ALiBi) | 50+ ms | too heavy |
| CodeSage-small | ~130M | check the licence | ONNX | 40+ ms | too heavy |

**Rules:**
- Hooks are short-lived processes, so only static models belong in the hook. Transformer models belong in the detached learner, if anywhere. `tract` is viable for BERT-small if avoiding `ort`'s native library matters.
- **Distribution:** huggingface is blocked here, and `adage models fetch` fails, which silently disables intent routing (`crates/adage-intent/src/model.rs`, `crates/adage-cli/src/models.rs`). Fix it by bundling:
  - potion-base-8M at f16 is about 15 MB (int8 about 7.5 MB, or PCA to 128 dimensions) as a pinned-digest **GitHub release asset**, shipped inside the plugin bundle next to the binaries.
  - Keep the SHA-256 pins. Mirrors (hf-mirror, ModelScope) are an optional fallback only.
  - Alternatively, distil a project-owned static model with model2vec from an MIT/Apache teacher, so provenance and licence are yours.

**Expected gain: small.** Test it on replay, not by belief, with three ablations: hashed-only vs potion-8M vs potion-code-16M (offline bge-small as a ceiling). Measures:
1. Prompt→edited-directory hit@k on the 45 held-out tasks, retrieving over files or over training prompts.
2. Prototype successor purity (already in G2).
3. CBR next-action agreement and replay savings (metric below).

Adopt only if ΔS ≥ 0.2 calls per session at equal precision. I expect < 0.1 on these repos and a measurable gain only on vue-core-like monorepos.

## 5. Target architecture, metric and plan

### 5.1 Architecture

```
static scan (manifests, CI, Makefile, tox/uv/venv, git co-change, imports) ─┐
sandbox verification (collect-only / dry runs, like adage train) ──────────┼─> Affordance map (typed, versioned by HEAD)
traces (existing importers) → situations & outcomes ──> calibrator ─────────┘         │
                                                   └─> case store (stage 3) ─────────┤
                                                                                      v
               typed compiler (existing shapes, ceilings, review) → index → hooks:
               SessionStart: ≤ 8-line affordance card (interpreter, test cmd, how to target a file)
               PreToolUse: rewrite or deny-with-reason for known dead-end invocations (pre-emptive)
               PostToolUse: after src edit → "tests co-changed with X: …" (prime); output diet (opt-in)
               after-failure + output-signature scan on exit-0 (fallback)
```

### 5.2 The offline replay metric

For a held-out trace τ = (a₁…aₙ) and policy π, at each hook event t with prefix h_t, π emits at most one directive d_t. Each directive class has:
- **Sat(d):** the equivalence class of actions that satisfy it. Example: a test run whose interpreter/runner equals d's and that executed tests.
- **W(d):** the waste class it targets. Example: env-friction calls, or test-discovery searches.

Define t* = min{t′ > t : a_{t′} ∈ Sat(d)}.

- **Useful** if t* exists (the trace itself eventually did it) or a sandbox replay of d succeeds.
- **Harmful** if a trace succeeded with an action that contradicts d (a different runner), or the replay fails.
- **Saved:** S(d_t) = |{a_k ∈ W(d) : t < k < t*}| + 𝟙[pre-emptive ∧ a_t ∉ Sat(d) ∧ a_t failed].
- **Cost of the directive:** C(d) = directive tokens × cache-read price × the remaining calls, expressed in calls-equivalent.

**Objective:** maximise Ŝ = (1/|sessions|) Σ_τ Σ_t [S(d_t) − C(d_t)], subject to:
- the Wilson 90% lower bound of useful/emitted ≥ 0.8;
- harmful ≤ 1% of emitted;
- ≤ 1 directive per 10 calls.

Report Ŝ × $/call as expected $ per session. For output diet, add Δcarry tokens and a loss penalty: count later actions that reference removed content (line numbers or paths beyond the cap).

**Calibration:** replay must predict U11's live −1.85 calls within the CI [−2.56, −1.17] before any other replay number is trusted.

### 5.3 Micro-live-slice protocol

Session-level A/B cannot see these effects. With about 15 calls of per-session sd, detecting 2 calls needs roughly 900 sessions per arm (unpaired), and the 30× run variance reported in 2604.22750 makes it worse. Instead:

1. **Randomise at the trigger.** At each qualifying event, draw directive vs silent with p = 0.5 (logged propensity). The outcome is calls and tokens from the trigger to Sat-goal, such as the first test run that executed tests. Pair by task and cluster by session.
2. **Use forced-situation starts** to raise the event rate: a short task ("run the tests covering `src/click/core.py` and report failures") on a fresh checkout with `.venv`, `max_turns` 15, Haiku. That is about $0.03 per session, so 100 sessions ≈ $3, giving a CI of about ±0.5 calls.
3. **Only then** run a session-level check, reported budget-matched, with CUPED or task pairing.

### 5.4 Ranked plan (expected value ÷ effort)

| # | Item | Effort | Expected value (these repos) | Try first? |
|---|---|---|---|---|
| 1 | Fix bug 1 (equivalence-class follow judge) and bug 2 (signature scan on exit-0 output); move U11 and its bug-5 extension (repro `python` → `.venv/bin/python`) to **`PreToolUse`** rewrite or deny-with-reason | 2–3 days | ≈2.5–3 calls in ~87% of Python sessions in this harness (the env bucket is 3.1). Replay measurable today | **Yes** |
| 2 | Sandbox-verified affordance map plus a ≤ 8-line SessionStart card | 3–5 days | Prevents dead ends before they happen; zero-trace cold start; cost is ~300 tokens × calls (count it) | Yes, alongside #1 |
| 3 | Co-change + import file→test map as a `PostToolUse` prime after source edits (hit@3 0.82) | 2–3 days | ≤ 1 call per session (discovery is 2.0 searches, partly productive); larger in monorepos | Second |
| 4 | Output diet: pytest `-q --tb=short` rewrites; opt-in cap on large reads via `updatedToolOutput` | 3 days + micro-slice | Up to ~$0.03 per session (≈10%) if no compensating reads; highest-risk item | Third, micro-slice only |
| 5 | Harness preferences (no commit when the user never commits) as user-set policy, not learned | 1 day | 1.6 calls in headless runs; zero for most real users | Optional |
| 6 | Case store (kNN situations, outcome-weighted), shadow mode, replay-gated | 1–2 weeks | Unknown. Likely matters only at ≥ 50 sessions per repo | Later |
| 7 | Embedding upgrades (bundled potion-code-16M; offline bge-small ablation) | 2–3 days | < 0.1 calls expected here | Last; also fix distribution now so intent doesn't silently fail |

**Start with #1 and #2 together**, measured first by the replay metric on the existing v1/v2 traces (no spend), then by one ~$5 micro-slice.

Even perfectly executed, the honest product claim on ordinary repos is a few percent of cost plus eliminated environment dead ends. The large effects exist only where a convention is necessary, undiscoverable and absent from any context file (the synthetic corpus). The pitch should become "your agent's environment, verified and delivered before it fails", with traces as the calibration evidence rather than the source.

**Repo files cited:**
- `/home/user/adage/evals/launch/README.md`
- `/home/user/adage/evals/bench/results/2026-09-29-repobench-v2/README.md`
- `/home/user/adage/evals/bench/results/2026-09-28-repobench/README.md`
- `/home/user/adage/docs/tracks/track-g-graph-engine.md` (§2 dominance rule, real-traces status note)
- `/home/user/adage/docs/instincts.md`
- `/home/user/adage/crates/adage-tracks/src/params.rs` (thresholds, lines 129–131)
- `/home/user/adage/crates/adage-learn/src/engine.rs` (Wilson gates)
- `/home/user/adage/crates/adage-intent/src/model.rs`
- `/home/user/adage/crates/adage-cli/src/models.rs`
- `/home/user/adage/evals/repobench/tasks/*.json`
- `/home/user/adage/evals/repobench/splits/*.json`

**Analysis scripts** (session scratchpad, not in the repo): `/tmp/claude-0/-home-user-adage/d6ae021f-492b-5125-b4ff-8e8a0676a66b/scratchpad/{waste.py,agg.py,agg2.py,agg3.py,cochange.py}`.

**Sources:**
[AWM](https://arxiv.org/abs/2409.07429) · [ReasoningBank](https://arxiv.org/abs/2509.25140) · [ReasoningBank blog](https://research.google/blog/reasoningbank-enabling-agents-to-learn-from-experience/) · [Budget-constrained study](https://arxiv.org/abs/2606.15017) · [Skills in the wild](https://arxiv.org/abs/2604.04323) · [Procedural memory retrieval benchmark](https://arxiv.org/abs/2511.21730) · [Memp](https://arxiv.org/abs/2508.06433) · [SWE-Exp](https://arxiv.org/abs/2507.23361) · [STAIR](https://arxiv.org/abs/2607.29658) · [Agent KB](https://arxiv.org/abs/2507.06229) · [Socratic-SWE](https://arxiv.org/abs/2606.07412) · [Evo-Memory](https://arxiv.org/abs/2511.20857) · [Gloaguen et al.](https://arxiv.org/abs/2602.11988) · [Lulla et al.](https://arxiv.org/abs/2601.20404) · [Probe-and-Refine](https://arxiv.org/abs/2606.20512) · [AgentDiet](https://arxiv.org/abs/2509.23586) · [Token consumption study](https://arxiv.org/abs/2604.22750) · [HarnessFix](https://arxiv.org/abs/2606.06324) · [TRIM](https://arxiv.org/abs/2607.18161) · [aider repo map](https://aider.chat/docs/repomap.html) · [aider repo map design](https://aider.chat/2023/10/22/repomap.html) · [pytest-testmon](https://github.com/tarpas/pytest-testmon) · [Jest CLI](https://jestjs.io/docs/cli) · [Vitest CLI](https://vitest.dev/guide/cli) · [Claude Code hooks](https://code.claude.com/docs/en/hooks) · [model2vec](https://github.com/MinishLab/model2vec) · [model2vec-rs](https://github.com/MinishLab/model2vec-rs) · [potion-code-16M](https://huggingface.co/minishlab/potion-code-16M) · [fastembed-rs](https://github.com/Anush008/fastembed-rs) · [Next-activity prediction with RAG](https://www.sciencedirect.com/science/article/pii/S0306437925001280)