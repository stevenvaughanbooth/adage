# Literature scout: what the state of the art says will move cost and pass rate, checked against our traces

2026-09-29, slug `literature`. Offline, $0. The code in this directory runs only measurement
instruments: `cost_anatomy.py`, `call_anatomy.py`, `tail_after_green.py` and `localise.py`. None of
it is a learner. arXiv, Semantic Scholar and OpenReview are blocked by the egress proxy here, so
the numbers below come from each paper's abstract or from a search-engine summary of it, plus
GitHub READMEs. **Numbers marked (abs.) have not been checked against the paper's tables.**

## So what (read this first)

**The literature's large gains (>20% steps or cost from experience reuse) almost all come from
templated benchmarks, from an LLM at runtime, or from token counts that ignore prompt caching.**
The one study of *hook-based* compression on *billed* Claude Code cost found a 38% cut in tool-output
tokens and a **6.8% rise** in cost. Held-out RepoBench control sessions show where the dollars
actually go, and the result changes the ranking:

| lever family (literature) | oracle ceiling, Haiku 4.5 (n=176) | Sonnet 5 (n=32) | what Adage does today |
| --- | ---: | ---: | --- |
| **E. Stop after the post-edit green test** (termination, over-verification) | **17.8%** of $ [16.1, 19.7] | 3.1% [1.8, 4.6] | nothing |
| everything after the last edit (loose bound of E) | 27.7% [25.3, 29.9] | 10.1% [7.2, 13.2] | nothing |
| **B. Cap each tool result at 1,000 tokens** (observation compression) | 14.8% [13.6, 16.1] | 4.7% [3.5, 6.1] | nothing |
| B at 250 / 500 / 2,000 tokens | 21.2 / 18.0 / 11.1% | 9.3 / 6.9 / 2.9% | |
| C. Localisation: no requests between the prompt and the first touch of a gold file | 5.6% [4.2, 7.0] | 1.1% [0.1, 2.3] | nothing |
| C with BM25 top-5 at the prompt (hit@5 = 0.85) | 5.3% [3.9, 6.7] | 1.1% | |
| A. Remove every failed call (the dead-end family, token accounting only) | 2.0% [1.8, 2.3] | 0.5% [0.2, 0.8] | U11 plus learned reflexes: 1–2 calls (~3–6%) |
| D. Remove exact repeat calls (a result cache) | 0.2% | 0.0% | – |
| *config, not learning:* 5-minute instead of 1-hour cache TTL | 12.3% | ≈12% | – |

(90% bootstrap CIs over sessions. Ceilings are *oracle* counterfactuals under the
cumulative-context cost model with provider-reported `usage`; they assume no behaviour change.)

- **The biggest learnable ceiling is termination, and it depends on the model.** After Haiku's last
  edit and its first green test run, it goes on for **7.6 more requests**. These are re-runs of the
  tests, repro heredocs, `pip install`, and `git add/commit/log/diff` that nobody asked for. That
  tail costs 17.8% of session dollars, and a long tail shows no pass-rate benefit (0.62 long vs 0.66
  short, n=138, underpowered). Sonnet 5 stops on its own (1.4 requests, 3%).
- **Observation compression has the next-largest ceiling but the worst evidence for a hook.**
  `Read` produces 72% of all tool-result characters (Haiku; 64% Sonnet), so "compression" mostly
  means truncating files the agent asked to read. The one Claude Code study of hook-based
  compression with billed cost found cost went *up*.
- **The dead-end family is capped, which confirms the brief.** Deleting *every* failed call and its
  tokens would save 2% of dollars (0.5% on Sonnet). Today's 1–2 calls saved already sit near what
  that family can deliver.
- **Localisation is not the bottleneck on RepoBench.** The agent first touches a gold source file at
  request 2 (median; Sonnet at 1). Retrieval works well (BM25 hit@5 0.85; BM25 + potion-code-16M-v2
  hit@10 0.96), but there is little left to save.
- **The expected value if shipped:** a learned "done" reflex plus insertion-time Read windowing
  plausibly captures a third to half of their ceilings on Haiku-class agents. That is about **6–12%
  $/task** (my estimate, not a measurement), with essentially nothing on Sonnet-class agents.
  **No lever in the literature that can be delivered by a hook has a credible ≥ 15–20% cost effect
  on a strong model.** I have low-to-moderate confidence that even the 6–12% survives a live A/B:
  termination advice can be ignored, and truncation can trigger re-reads.
- **The pass-rate levers with credible several-point effects** are the following. Tuned repository
  guidance: +7.5 pp on SWE-bench Verified (probe-and-refine), though LLM-generated AGENTS.md costs
  −3 pp and +20% cost. Fail-fast plus restart: +5.2 pp. Subtask-level memory: +4.7 pp, rising to
  +9–10 pp once 300+ same-benchmark instances are in memory. Linting edit guardrails: +3 pp. All
  need either an LLM at learn time or a restart, which a hook cannot do, except the guardrail, and
  the guardrail is not learned from experience.

## Data and cost model

Held-out **control** sessions (hooks on, directives withheld), v1 + v2, de-duplicated by session
id: 176 Haiku 4.5 sessions (attrs 49, click 47, marshmallow 30, urfave-cli 16, vue-core 34) and 32
Sonnet 5 sessions (click, vue-core). Cost is repriced from each API request's `usage` at list price:
Haiku $1 input, $2 1-hour cache write, $0.10 cache read, $5 output; Sonnet at 3× the same. All
cache writes in these runs are 1-hour writes, and no gap between requests exceeds 5 minutes
(p95 of the longest gap is 114 s).

| (Haiku, per session) | value |
| --- | --- |
| mean cost | $0.259 [0.242, 0.274] |
| requests / tool calls | 33.0 / 32.4 |
| failed calls | 2.7 |
| max context | 48.9k tokens |
| cost share: cache read / cache write / output (incl. thinking) / uncached input | 44% / 33% / 23% / 0.1% |
| calls before the first edit / edit loop / after the last edit | 11.2 / 11.9 / 9.3 (Sonnet 10.8 / 14.3 / 3.3) |
| share of tool-result characters from `Read` | 72% |

Consequences for every lever:

1. **A saved call is worth about its share of requests.** Every request re-reads the context, so
   the cost of a call scales with the size of the context at that point. Saving 15–20% of cost
   means removing about 5–7 of 33 calls, or shrinking what later calls re-read.
2. **Thinking and output tokens are 23–27% of cost.** Hooks cannot touch them except by removing
   whole requests.
3. **The $ figures depend on the TTL.** These runs wrote the cache at 1-hour TTL (2× input price).
   At 5-minute TTL (1.25×), the cache-write share would fall by about 12 points of total cost. That
   is a measurement caveat for every $ claim in this sprint. It is also a trivially learnable
   config lever (choose the TTL from the user's observed inter-request gaps) for API-key users who
   have enabled `ENABLE_PROMPT_CACHING_1H`. Subscription users do not pay per token.

## The literature, ranked by measured effect on cost and success for *coding* agents

"Hook-cheap" means the part that can be replicated deterministically, locally, with no LLM call at
runtime and small latency. "Trust" is my judgement of whether the number measures what it claims.

| # | work | mechanism | measured effect (benchmark) | trust / artefacts | hook-cheap part | experiment on our traces |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | **Token Reduction Is Not Cost Reduction** (Weinberger & Hozez 2026) [1] | Claude Code with hook-based tool-output compression (RTK) and an API proxy | ML compression arm: tool-output tokens −38.4%, **billed cost +6.8%**; per-task r(output reduction, cost change) = 0.15, ρ = 0.01; cache traffic ≈ 87% of cost (2,848 billed runs, 103 tasks, 7 repos, 3 models) | **High.** Pre-registered, paired, billed. It is the closest analogue to Adage | it *is* a hook; it is the warning | Reproduce its cost decomposition on our runs (done above: cache read + write = 77%) |
| 2 | **AgentDiet** (2509.23586) [2] | a reflection LLM removes useless, redundant and expired trajectory content | input tokens −39.9 to −59.7%, **cost −21.1 to −35.9%**, success −1.0 to +2.0 pp (SWE-bench Verified, Multi-SWE-bench Flash; Claude 4 Sonnet, Gemini 2.5 Pro) | Medium. It edits *past* context, which breaks Claude Code's prompt cache; the cost model needs checking against #1 | only insertion-time filtering (PostToolUse `updatedToolOutput`) | B ceiling above; E2 below |
| 3 | **The Complexity Trap: observation masking** (2508.21433) [3] | replace old observations with placeholders | **~50% cheaper** than the raw agent, solve rate equal or +2.6 pp (SWE-agent, SWE-bench Verified, 5 model configs) | Medium. SWE-agent's own loop controls caching; masking history is not possible from a Claude Code hook | none (no hook can rewrite history) | – |
| 4 | **SWE-Pruner** (2601.16746) [4] | a 0.6B skimmer keeps the lines relevant to a goal hint | tokens −23 to −38% (SWE-bench Verified), rounds up to −26%, success within 1% | Medium. Tokens, not billed cost | the *policy* (keep lines near symbols the agent will use) could be learned from traces | E2 |
| 5 | **CoACT** (2607.02911) [5] | a compressor trained so that the compressed observation induces the **same next action** (NAP) | combined with trajectory compression, total cost $45.65 → $25.88 (−43%) with success preserved | Medium–high on the criterion; the compressor is a model | **NAP is an offline test we can run**: does the next action use the removed lines? | E2 |
| 6 | **OpenHands condenser** [6] | LLM summarisation of old events | per-turn cost bounded at < ½, **but on full SWE-bench Verified it resolved 200 vs 203 and cost $40 *more* because of lower cache use** | High. It is the counter-example to its own headline | – | – |
| 7 | **Harness-design study** (2609.20804) [7] | 176 harness settings × 4 models; planning ablations | strong models: planning **removes redundant post-edit verification**, lowering cost; weak models: +11.6 pp success at higher cost (SWE-bench Verified, Terminal-Bench 2.1) | High (ablation) | a post-edit "you're done" signal is the hook form of that effect | **E1** |
| 8 | **Failure as a Process** (2607.09510) [8]; **Overthinking** (2502.08235) [9] | trajectory anatomy; overthinking score | 82% of failed recoveries keep running (verification that cannot change the outcome, repeated strategies); picking the low-overthinking run: ~+30% performance, **−43% cost** (SWE-bench Verified, 4,018 trajectories) | Medium. The −43% is *selection among samples*, not an intervention | detecting "the outcome is settled" | E1 |
| 9 | **Fail-Fast, Restart-Smart** (2608.03222) [10]; **Automata from Agent Traces** (2608.23670) [11] | prefix failure monitor (0.6B), or an FSM mined from traces (7–43 states, ≥ 0.997 replay fitness), then stop and restart | tokens −14.6 to −20.4% at 5% FPR; resolution 66.6 → 71.8% with restart (SWE tasks); FSM failure AUROC up to 0.94, beats AWM for next step | Medium. The false-alarm cost is real and the restart needs harness control | FSM or process-mining monitor is deterministic and ms-fast | E4 |
| 10 | **To Run or Not to Run** (2606.26978) [12] | vary code-execution policy across Claude Code, Codex, OpenCode (3,000 repairs, 200 instances) | 8.8 test runs per task; banning execution costs only **1.25 pp (n.s.)** and saves substantial tokens | High (controlled) | learned "one test run per edit is enough here" | part of E1 |
| 11 | **Probe-and-refine repository guidance** (2606.20512) [13] | synthetic bug probes diagnose and patch AGENTS.md (LLM at learn time only) | **33.0% vs 25.5%** resolve (SWE-bench Verified, p < 0.001); gain is coverage, not precision; **does not transfer across models** | Medium–high; one model family | the delivery is static text; the learning needs an LLM | out of scope (LLM at learn time) |
| 12 | **Evaluating AGENTS.md** (2602.11988) [14]; **SkillsBench** (2602.12670) [15] | context files; skills | LLM-generated context: **−3% success, +20% cost**; human-written: +4% success, up to +19% cost. Curated skills +16.6 pp, but **+4.5 pp in software engineering**; self-generated skills −1.3 pp | High (multi-agent, multi-model) | – | a warning: anything Adage injects must pay for itself (principle 3) |
| 13 | **Subtask-level memory for SWE agents** (2602.21611) [16] | memory aligned to the agent's subtask decomposition | **+4.7 pp** mean Pass@1 (SWE-bench Verified; +6.8 on Gemini 2.5 Pro); −1 pp for the first ~200 instances, **+9–10 pp after 300+** | Medium. The gain grows with memory density, which is repository overlap in SWE-bench (django-heavy). That is "a repo it has worked in before", our premise, but at 30× our memory | subtask segmentation plus retrieval can be done with kNN | E5 |
| 14 | **ReasoningBank** (2509.25140, ICLR 2026) [17] | LLM-distilled strategies from successes *and failures*; MaTTS | +8.3 pp WebArena, **+4.6 pp SWE-bench Verified, −2.8 steps**; with MaTTS 46.7 → 56.3% SR, 8.8 → 7.1 steps (WebArena) (abs.) | Medium. LLM judge self-labels; LLM at runtime | failure-derived "don't" items ≈ Adage's avoid shape | E5 |
| 15 | **SWE-Replay** (2601.22129) [18] | resume from archived intermediate states instead of fresh rollouts | test-time scaling cost −17.4%, resolve +3.8 pp | High, but it applies to multi-sample scaling, not one session | – | – |
| 16 | **SWE-Exp** (2507.23361) [19]; **ExpeRepair** (2506.10484) [20]; **Agent KB** (2507.06229) [21] | experience banks for repair | SWE-Exp 73.0% (Claude 4 Sonnet: about the base model's own score, so **not an experience effect**); +7.2% relative (DeepSeek-V3). Agent KB SWE-bench Lite 24.3 → 38.0/45.7% at 50 iterations | Low. Weak baselines at iteration caps; knowledge from related benchmarks | – | – |
| 17 | **SWE-agent ACI** (2405.15793) [22] | bounded viewer, summarised search, linting edit guard | linting guard +3.0 pp; summarised search 18.0 vs 12.0%; shell-only 11 → 18% (SWE-bench Lite) | High (ablation), but designed, not learned | lint-on-edit is a PostToolUse hook | not an experience lever |
| 18 | **Cursor semantic search** [23]; **Semble / potion-code** [24] | a custom embedding model trained *on agent traces* (what should have been retrieved earlier); static code embeddings + BM25 | Cursor: +12.5% QA accuracy (6.5–23.5%), code retention +0.3% (+2.6% on 1,000+ file repos). Semble: 98% fewer tokens than grep+read *on a retrieval benchmark* | Low–medium for cost (accuracy of Q&A, retrieval tokens) | potion-code-16M-v2 is on npm (below); BM25 | C above (measured: small ceiling on RepoBench) |
| 19 | **Agentless** (2407.01489) [25]; SWE-bench BM25 [26] | hierarchical localisation, then repair | 27.3% at $0.34 (SWE-bench Lite); BM25 top-30 recall 87.7% | High, but non-agentic | BM25 at the prompt | C (done) |

### Web and embodied memory work (for mechanisms; effects do not transfer to our setting)

| work | mechanism | effect | why it is inflated for us |
| --- | --- | --- | --- |
| AWM [27] | LLM induces workflows (reusable sub-routines) offline or online | +24.6% / +51.1% *relative* SR (Mind2Web / WebArena), fewer steps | WebArena has 812 tasks from 241 templates, so workflows reuse near-duplicates. A 2026 budget-matched study [28] finds **vanilla matches or beats AWM, skills and memory** once tokens are equalised, and says run-to-run variance must be reported |
| ASI [29] | programmatic skills, verified before being admitted | +23.5 pp vs static, +11.3 vs AWM; **10.7–15.3% fewer steps** (WebArena) | same templating; skills are site APIs |
| SkillWeaver [30] | self-explored website APIs | +31.8% / +39.8% relative (WebArena / live sites) | exploration on a fixed site |
| SpeedRunner (2608.11338) [31] | skills as programs, refactored from trajectories | "best cost reduction" across 3 embodied environments | repeated environment |
| TraceCompiler (2608.02680) [32] | def–use dependency mining (value provenance), compile traces to mostly deterministic workflows | producer–consumer edges P 0.928 / R 0.943 vs 0.71 F1 for adjacency; an intent went from 34 API calls → 11 | API workflows repeat; coding tasks do not |
| BPOP (2602.02806) [33] | Bayesian partial order from linearised traces, then parallelise | "near-zero tokens" once compiled (cloud IaC) | procedural domains |
| ExpeL [34] | insights (ADD/EDIT/UPVOTE) + trajectory recall | ALFWorld 40 → 59%, HotpotQA 28 → 39%, WebShop 0.665 → 0.701 | 6 ALFWorld task types |
| Memp [35] | procedural memory build/retrieve/update | ALFWorld 42.1 → 77.9% SR, steps 23.8 → 15.0 (−37%) | same |
| AutoManual [36], CLIN [37] | rule manuals; causal abstractions | ALFWorld 97.4%; ScienceWorld +23 vs Reflexion | same |
| Synapse [38] | state abstraction + trajectory-as-exemplar + kNN memory | MiniWoB++ 99.2%; Mind2Web +56% rel. step SR | exemplar tasks near-duplicate |
| Voyager [39] | code skill library | 3.3× items, 15.3× faster tech tree (Minecraft) | open-ended game |
| Evo-Memory / ExpRAG / ReMem [40] | streaming test-time memory; ExpRAG = kNN over past task records | consistent SR gains, fewer steps (10 datasets) (abs.) | streams of similar tasks |
| Agentic Plan Caching [41] | keyword-matched plan templates, adapted by a small LM | **−50.3% cost**, −27.3% latency | repeated queries in the same app |
| Procedural memory retrieval benchmark (2511.21730) [42] | dual corpora on ALFWorld; 6 retrievers | embeddings are strong on familiar contexts and **fall off a cliff on novel vocabularies**; LLM abstractions transfer | this is our lab's "cross-repo precision 0.00" finding |
| Memory management (2505.16067) [43] | experience-following | error propagation and misaligned replay; selective add/delete +10 pp | a harm model for mis-firing reflexes |
| Managing procedural memory / AFTER (2606.23127) [44] | enterprise skills | one refinement round +3.7–6.7 points | – |
| DreamBench-SWE (2608.20664) [45] | multi-session memory hygiene with hidden oracles | **deterministic verbatim event memory ≈ elaborate memory** (82/180 vs 83/180; no memory 21/180) | supports storing raw events plus retrieval over LLM-distilled notes |

### Classic methods the brief names (for building the instruments)

- **Process mining.** Inductive Miner [46] gives sound, block-structured models from event logs.
  The 2026 "Automata from Agent Traces" [11] shows compact FSMs (7–43 states) mined in
  milliseconds that replay held-out agent traces at ≥ 0.997 fitness and predict failure (AUROC
  ≤ 0.94). This is the most direct fit for a *learned situation* alphabet with no hand-written
  taxonomy.
- **Log templates.** Drain [47] (fixed-depth parse tree) and LogPAI/loghub [48]. Drain3 is on PyPI.
  It over-partitions high-cardinality lines (FGA 0.22 in one 2026 report), so mask paths and
  numbers first, using *learned* masks such as tokens with high value entropy across the corpus,
  not a hand-written regex. Use it to template **tool outputs**: that is how a goal event ("the
  test run actually ran and passed") can be *learned* rather than written as the signature lists of
  `evals/replay`.
- **Anti-unification and library learning.** Plotkin/Reynolds least general generalisation (the
  `abstraction/lgg.py` lab already has this), Stitch [49] (fast top-down compression) and babble
  [50] (e-graph anti-unification, modulo an equational theory). For coding traces the reusable
  structure is short (next-action top-1 is 0.18–0.32 in `evals/lab`), so compression-based skills
  have a small ceiling here. `git add → git commit → git log` (≈ 1.9 calls per Haiku session) is
  the one clear macro, and it is better removed (E1) than compressed.
- **Offline RL and credit assignment.** Retrospex [51] (an offline critic re-ranks LLM actions,
  weighted more on long tasks); TD and IQL-style value estimates. At 7–10 training sessions per
  repository, `evals/launch/outcome-optimisation.md` is right that tabular or empirical-Bayes
  values over a learned state alphabet are the most the data supports.
- **Off-policy evaluation.** IPS / SNIPS [52], doubly robust [53], and the replay method [54]. All
  need logged propensities, so every lever below must ship with per-trigger randomisation (a
  withhold rate) or it cannot be evaluated offline afterwards.
- **Static code embeddings.** **potion-code-16M-v2 is obtainable here** from the npm package
  `zvec-grep-potion-code-16m-v2-model@0.1.0` (MIT). Its `manifest.json` names
  `minishlab/potion-code-16M-v2@e9d2a44c`, and the SHA-256 of `model.safetensors` (`75cf7a6c…623c`)
  matches the manifest (a self-consistency check only; Hugging Face is unreachable). It is
  unpacked at `/tmp/adage-lab-models/potion-code-16m-v2/package/assets/`. The package's
  `install.cjs` was never run.
- **Repository maps.** aider [55] uses tree-sitter tags, then PageRank over the def/ref graph
  personalised to the chat files, then a token-budgeted signature map (1k tokens by default).

## Measured on our traces: localisation (`localise.py`)

Query = task prompt. Corpus = the code files at the task's starting commit (`git show HEAD:`, never
the agent's working tree). Gold = the task's `src_files`, used as an evaluation label only. There
are 53 task instances: tasks × {Haiku, Sonnet} repositories.

| retriever | hit@1 | hit@3 | hit@5 | hit@10 |
| --- | ---: | ---: | ---: | ---: |
| BM25 (path ×3 + content) | 0.42 | 0.74 | 0.85 | 0.91 |
| potion-code-16M-v2 (max over 60-line chunks) | 0.30 | 0.70 | 0.72 | 0.91 |
| potion-base-8M | 0.19 | 0.62 | 0.66 | 0.81 |
| BM25 + potion-code (RRF) | **0.49** | 0.68 | **0.89** | **0.96** |
| trace prior (files edited in the repo's training sessions, a count) | 0.09 | 0.34 | 0.47 | 0.47 |
| BM25 + trace prior | 0.15 | 0.45 | 0.49 | 0.55 |

- Potion-code beats potion-base (+0.08 hit@3, +0.10 hit@10) and helps BM25 at hit@1 and hit@10.
- The trace prior *hurts*: bug fixes in a repository do not recur in the same files, which is the
  lab's "file directives rarely pay off" again.
- Translated to cost: first touch of a gold file is at request 2.6 (Haiku; median 2) and 1.1
  (Sonnet). The requests before it are worth 5.6% of Haiku cost and 1.1% of Sonnet cost. BM25@5 at
  the prompt would recover 5.3% at most, and only if the agent then skipped every intermediate
  step.
- **Verdict:** a negative result *on RepoBench*. Its prompts are written from the commit message
  and the test diff, and are informative enough that the agent finds the file itself. It may be
  larger on vague real-world prompts, which is untested.

## Ranked ideas for Adage, each with a pre-registered experiment

The expected value (EV) is the ceiling × a plausible capture rate × the model mix. All experiments
train on the `train` split, report on held-out control sessions with 90% bootstrap CIs, and add
leave-one-repo-out where something is learned.

**E1. A learned "done" reflex. EV: highest for Haiku-class agents (ceiling 17.8%); ~3% for
Sonnet-class.**

Literature: [7][8][9][10][12]. The mechanism is to learn the goal event and then deliver a
PostToolUse `additionalContext` note once the settled state is reached: "the change is in place and
its tests pass here; further verification in this repo has not changed outcomes; finish now".
Nothing is hard-coded:

- *Goal event:* template tool outputs with Drain3 (learned masks). Label each template by its
  association with the session's hidden-test outcome and with "no further edits followed" in
  training sessions. A template that predicts "no later edit, and the session passed" is a learned
  G_test. Validate it against the hand-written signature in `tail_after_green.py`, which is the
  instrument and the baseline to beat.
- *Trigger:* the first occurrence of a learned G_test after an edit, when the session's edited
  files are covered by that run.
- *Offline test:* the precision of "no edit follows" at the trigger (harm = sessions where an edit
  followed and changed the outcome), and the cost share after the trigger.
- *Held-out:* control sessions, with leave-one-repo-out.
- *Pass-rate risk:* by a loose regex (it over-counts heredocs), 101 of 170 Haiku tails contain
  Bash that may modify the tree. Examples: `git commit --amend`, `git stash`, `git reset --soft`,
  `rm` of repro files, `pip install -e`. So stopping is *not* outcome-neutral by construction. Measure how often the tail's net effect on
  the diff is non-empty.
- *Live:* per-trigger withhold at 20% for OPE; target ≥ 8% $/task on Haiku with a non-inferior
  pass rate (margin −3 pp).

**E2. Insertion-time Read windowing with a next-action-preservation check. EV: second (ceiling
14.8% at 1k tokens; 4.7% on Sonnet), with high risk.**

Literature: [1][2][4][5]. From training traces, learn which parts of a `Read` result the session
later *uses*: lines that appear in a later `Edit` `old_string`, lines whose symbols appear in later
grep patterns or commands, and lines re-read later. This is CoACT's NAP criterion as an offline
label. The lever is either a PreToolUse `updatedInput` that sets `offset/limit` around the learned
span (symbols named in the prompt or the last error), or a PostToolUse `updatedToolOutput` that
elides unused regions with a "lines a–b elided; Read with offset to see them" marker.

- *Offline metric:* the share of later-used lines that the policy would have hidden (the misfire
  rate, which predicts a re-read) against the tokens saved, priced with the counterfactual in
  `cost_anatomy.py`. That includes the re-read penalty: each hidden-but-used line costs one extra
  request.
- *Kill criterion:* if net saving after the re-read penalty is below 5% on held-out sessions, drop
  it. Given [1], that is the likely outcome.

**E3. Test-run budget (overlaps with E1). EV: medium.**

Literature: [12]. Haiku averages 5.6 test-named calls per session. Learn per repository which test
invocations were followed by *no change in the pass/fail template*, meaning a redundant re-run,
and advise "the last run already covers this". The offline measure is the count of redundant
re-runs and their cost share.

**E4. Failure monitor from a mined automaton (pass-rate lever). EV: medium, needs live testing.**

Literature: [10][11]. Mine an FSM or process model (Inductive Miner, or the FSM of [11]) over
learned action classes from training sessions, and score held-out prefixes at 25/50% of their
length.

- *Offline:* AUROC for the hidden-test outcome, leave-one-repo-out. A hook cannot restart the
  session, so the intervention would be a "step back and re-read the failing test" advisory. Its
  effect is not measurable offline, and a false alarm aborts a run that would have succeeded [10].
- Pursue it only if AUROC ≥ 0.8 at 25% of the session.

**E5. Within-repository subtask memory. EV: low at our memory size, rises with sessions.**

Literature: [16][17][45]. Segment sessions into subtasks with the learned FSM states from E4.
Store verbatim event snippets per subtask ([45] says verbatim events are as good as distilled
notes) and retrieve with potion-code + BM25. [16] shows *negative* effects below about 200
instances and gains above 300. Our 7–10 training sessions per repository are far below that, so
run it only on the grown-memory variant: every other task, leave-task-out, as in
`evals/lab/README.md`.

**E6 (config, not learning).** Learn the inter-request gap distribution per user and recommend
`FORCE_PROMPT_CACHING_5M=1` when p99 < 5 minutes. That is −12.3% of $ on these runs for API-key
users with 1-hour caching on, and zero risk to success. Worth one line of product, and a caveat on
every $ number in this sprint.

**Deprioritised, with reasons:**

- Programmatic skills and macros (ASI, SpeedRunner, TraceCompiler): next-action predictability is
  0.18–0.32, and the only frequent macro (git add/commit/log) is waste better removed by E1.
- Localisation hints: measured ceiling 5.6%.
- Result caching of exact repeats: 0.2%.
- More dead-end reflexes: family ceiling 2%.
- LLM-distilled memory (ReasoningBank, AWM, ExpeL): an LLM at runtime, and budget-matched studies
  erase the gains.

## Scepticism checklist (applied above)

1. **Templated benchmarks inflate reuse.** Look out for WebArena (812 tasks, 241 templates),
   ALFWorld (6 types), MiniWoB++ and Mind2Web cross-task. Every >20% step or cost reduction in the
   memory literature comes from these, from repeated-query serving (plan caching), or from
   multi-sample scaling (SWE-Replay, overthinking selection).
2. **Token counts are not dollars under prompt caching.** See [1], and [6]'s own full-benchmark
   run. Compressing or summarising *history* breaks the cache prefix; only insertion-time changes
   are cache-safe.
3. **Relative vs absolute.** AWM's "+51.1%" is relative; SkillWeaver's +31.8% is 22.6 → 29.8.
4. **Base-model confounds.** SWE-Exp's 73% ≈ Claude 4 Sonnet unaided; Agent KB's baselines are
   capped at 50 iterations.
5. **Budget matching and variance.** [28]: vanilla matches the augmented agents at equal tokens.
   The Adage launch report's "30× run-to-run variance" says the same for session-level outcomes.
6. **Injected context costs.** AGENTS.md +20% cost [14]; self-generated skills −1.3 pp [15].
   Adage's directives are small, but the same accounting applies.
7. **Memory density.** Gains need hundreds of prior instances in the same distribution [16]. Our
   own lab: cross-repository memory precision 0.00, grown memory helps a little.

## Hard-coded parts (all instruments)

- `tail_after_green.py`: the test-runner regex and pass/fail signatures, used to *measure* the
  ceiling. E1 must replace them with learned output templates.
- `call_anatomy.py`: uses the lab's hand-written `normalize.py` names.
- `localise.py`: a code-file extension list; task `src_files` as labels.
- `cost_anatomy.py`: Claude Code's own `is_error` flag, and edit = Edit/Write/MultiEdit.

## Reproduce

```sh
cd evals/lab/research/literature
OMP_NUM_THREADS=1 nice -n 10 python3 cost_anatomy.py      # -> cost_anatomy.json   (~20 s)
OMP_NUM_THREADS=1 nice -n 10 python3 call_anatomy.py      # -> call_anatomy.json
OMP_NUM_THREADS=1 nice -n 10 python3 tail_after_green.py  # -> tail_after_green.json
# potion-code-16M-v2 (npm, MIT) into the shared model dir, then:
#   curl -sO https://registry.npmjs.org/zvec-grep-potion-code-16m-v2-model/-/zvec-grep-potion-code-16m-v2-model-0.1.0.tgz
#   mkdir -p /tmp/adage-lab-models/potion-code-16m-v2 && tar xzf zvec-grep-*.tgz -C /tmp/adage-lab-models/potion-code-16m-v2
OMP_NUM_THREADS=1 nice -n 10 python3 localise.py          # -> localise.json       (~3 min)
```

## References

1. Weinberger, Hozez. *Token Reduction Is Not Cost Reduction: An Empirical Study of End-to-End Efficiency in API-Based Coding Agents.* 2026. https://arxiv.org/abs/2607.12161
2. *Reducing Cost of LLM Agents with Trajectory Reduction* (AgentDiet). 2025. https://arxiv.org/abs/2509.23586
3. Lindenbauer et al. *The Complexity Trap: Simple Observation Masking Is as Efficient as LLM Summarization for Agent Context Management.* DL4Code @ NeurIPS 2025. https://arxiv.org/abs/2508.21433 · https://github.com/JetBrains-Research/the-complexity-trap
4. *SWE-Pruner: Self-Adaptive Context Pruning for Coding Agents.* 2026. https://arxiv.org/abs/2601.16746
5. Chen et al. *CoACT: Action-Preserving Observation Compression for Coding Agents.* 2026. https://arxiv.org/abs/2607.02911 · https://github.com/THU-Agent/CoACT
6. OpenHands. *Context condensation for more efficient AI agents.* 2025. https://www.openhands.dev/blog/openhands-context-condensensation-for-more-efficient-ai-agents
7. *An Empirical Study of Harness Design for Coding Agents.* 2026. https://arxiv.org/abs/2609.20804
8. *Failure as a Process: An Anatomy of CLI Coding Agent Trajectories.* 2026. https://arxiv.org/abs/2607.09510
9. Cuadron et al. *The Danger of Overthinking: Examining the Reasoning-Action Dilemma in Agentic Tasks.* 2025. https://arxiv.org/abs/2502.08235
10. *Fail-Fast, Restart-Smart: Early Failure Prediction and Restart for SWE Agentic Tasks.* 2026. https://arxiv.org/abs/2608.03222
11. *Automata from Agent Traces: Failure and Next-Step Prediction.* 2026. https://arxiv.org/abs/2608.23670
12. *To Run or Not to Run: Analyzing the Cost-Effectiveness of Code Execution in LLM-Based Program Repair.* ISSTA 2026. https://arxiv.org/abs/2606.26978
13. Shepard, Albrecht. *Probe-and-Refine Tuning of Repository Guidance for Coding Agents.* 2026. https://arxiv.org/abs/2606.20512
14. Gloaguen et al. *Evaluating AGENTS.md: Are Repository-Level Context Files Helpful for Coding Agents?* 2026. https://arxiv.org/abs/2602.11988
15. *SkillsBench: Benchmarking How Well Agent Skills Work Across Diverse Tasks.* 2026. https://arxiv.org/abs/2602.12670
16. *Structurally Aligned Subtask-Level Memory for Software Engineering Agents.* 2026. https://arxiv.org/abs/2602.21611
17. Ouyang et al. *ReasoningBank: Scaling Agent Self-Evolving with Reasoning Memory.* ICLR 2026. https://arxiv.org/abs/2509.25140 · https://research.google/blog/reasoningbank-enabling-agents-to-learn-from-experience/
18. *SWE-Replay: Efficient Test-Time Scaling for Software Engineering Agents.* 2026. https://arxiv.org/abs/2601.22129
19. Chen et al. *SWE-Exp: Experience-Driven Software Issue Resolution.* 2025. https://arxiv.org/abs/2507.23361
20. *ExpeRepair: Dual-Memory Enhanced LLM-based Repository-Level Program Repair.* 2025. https://arxiv.org/abs/2506.10484
21. *Agent KB: Leveraging Cross-Domain Experience for Agentic Problem Solving.* 2025. https://arxiv.org/abs/2507.06229
22. Yang et al. *SWE-agent: Agent-Computer Interfaces Enable Automated Software Engineering.* NeurIPS 2024. https://arxiv.org/abs/2405.15793
23. Cursor. *Improving agent with semantic search.* 2025. https://cursor.com/blog/semsearch
24. MinishLab. *Semble* and *potion-code-16M-v2*. https://github.com/MinishLab/semble · https://github.com/MinishLab/model2vec · https://huggingface.co/minishlab/potion-code-16M-v2 · npm mirror https://www.npmjs.com/package/zvec-grep-potion-code-16m-v2-model
25. Xia et al. *Agentless: Demystifying LLM-based Software Engineering Agents.* 2024. https://arxiv.org/abs/2407.01489
26. Jimenez et al. *SWE-bench: Can Language Models Resolve Real-World GitHub Issues?* ICLR 2024. https://arxiv.org/abs/2310.06770
27. Wang, Mao, Fried, Neubig. *Agent Workflow Memory.* ICML 2025. https://arxiv.org/abs/2409.07429 · https://github.com/zorazrw/agent-workflow-memory
28. Hajimiri et al. *Are Online Skill and Memory Modules Always Worth Their Tokens? A Budget-Constrained Study of Web Agents.* 2026. https://arxiv.org/abs/2606.15017
29. Wang et al. *Inducing Programmatic Skills for Agentic Tasks* (ASI). COLM 2025. https://arxiv.org/abs/2504.06821 · https://github.com/zorazrw/agent-skill-induction
30. Zheng et al. *SkillWeaver: Web Agents can Self-Improve by Discovering and Honing Skills.* 2025. https://arxiv.org/abs/2504.07079
31. Huang et al. *Better, Faster, Stronger: Programmatic Skill Learning Best Reduces Agent Cost.* 2026. https://arxiv.org/abs/2608.11338
32. El Yadouni, Li. *TraceCompiler: Skill-Guided Mining and Compilation of LLM Agent Traces into Mostly Deterministic Workflows.* 2026. https://arxiv.org/abs/2608.02680
33. Li et al. *De-Linearizing Agent Traces: Bayesian Inference of Latent Partial Orders for Efficient Execution.* ICML 2026. https://arxiv.org/abs/2602.02806
34. Zhao et al. *ExpeL: LLM Agents Are Experiential Learners.* AAAI 2024. https://arxiv.org/abs/2308.10144
35. Fang et al. *Memp: Exploring Agent Procedural Memory.* 2025. https://arxiv.org/abs/2508.06433
36. Chen et al. *AutoManual.* NeurIPS 2024. https://arxiv.org/abs/2405.16247
37. Majumder et al. *CLIN: A Continually Learning Language Agent.* 2023. https://arxiv.org/abs/2310.10134
38. Zheng et al. *Synapse: Trajectory-as-Exemplar Prompting with Memory for Computer Control.* ICLR 2024. https://arxiv.org/abs/2306.07863
39. Wang et al. *Voyager: An Open-Ended Embodied Agent with Large Language Models.* 2023. https://arxiv.org/abs/2305.16291
40. Wei et al. *Evo-Memory: Benchmarking LLM Agent Test-time Learning with Self-Evolving Memory.* 2025. https://arxiv.org/abs/2511.20857
41. Zhang et al. *Agentic Plan Caching: Test-Time Memory for Fast and Cost-Efficient LLM Agents.* NeurIPS 2025. https://arxiv.org/abs/2506.14852
42. Kohar, Krishnan. *A Benchmark for Procedural Memory Retrieval in Language Agents.* MemAgents @ ICLR 2026. https://arxiv.org/abs/2511.21730 · https://github.com/qpiai/Proced_mem_bench
43. Xiong et al. *How Memory Management Impacts LLM Agents: An Empirical Study of Experience-Following Behavior.* ACL 2026. https://arxiv.org/abs/2505.16067
44. Belikova et al. *Managing Procedural Memory in LLM Agents: Control, Adaptation, and Evaluation.* 2026. https://arxiv.org/abs/2606.23127
45. *DreamBench-SWE: A Multi-Session Memory-Hygiene Benchmark for Software Agents.* 2026. https://arxiv.org/abs/2608.20664
46. Leemans, Fahland, van der Aalst. *Discovering Block-Structured Process Models from Event Logs: A Constructive Approach.* Petri Nets 2013. https://doi.org/10.1007/978-3-642-38697-8_17
47. He, Zhu, Zheng, Lyu. *Drain: An Online Log Parsing Approach with Fixed Depth Tree.* ICWS 2017. https://doi.org/10.1109/ICWS.2017.13 · https://github.com/logpai/Drain3
48. Zhu et al. *Tools and Benchmarks for Automated Log Parsing* (LogPAI). ICSE-SEIP 2019. https://github.com/logpai/logparser · loghub https://github.com/logpai/loghub
49. Bowers et al. *Top-Down Synthesis for Library Learning* (Stitch). POPL 2023. https://arxiv.org/abs/2211.16605
50. Cao et al. *babble: Learning Better Abstractions with E-Graphs and Anti-Unification.* POPL 2023. https://arxiv.org/abs/2212.04596
51. Xiang et al. *Retrospex: Language Agent Meets Offline Reinforcement Learning Critic.* EMNLP 2024. https://aclanthology.org/2024.emnlp-main.268/
52. Swaminathan, Joachims. *The Self-Normalized Estimator for Counterfactual Learning.* NeurIPS 2015. https://papers.nips.cc/paper/5748-the-self-normalized-estimator-for-counterfactual-learning
53. Dudík, Langford, Li. *Doubly Robust Policy Evaluation and Learning.* ICML 2011. https://arxiv.org/abs/1103.4601
54. Li, Chu, Langford, Wang. *Unbiased Offline Evaluation of Contextual-Bandit-based News Article Recommendation Algorithms.* WSDM 2011. https://arxiv.org/abs/1003.5956
55. aider. *Repository map* / *Building a better repository map with tree sitter.* https://aider.chat/docs/repomap.html · https://aider.chat/2023/10/22/repomap.html
56. Claude Code docs: hooks (`updatedInput`, `updatedToolOutput`, `additionalContext`) https://code.claude.com/docs/en/hooks · memory https://code.claude.com/docs/en/memory · prompt caching https://code.claude.com/docs/en/prompt-caching · Devin Knowledge https://docs.devin.ai/product-guides/knowledge
57. *Don't Break the Cache: An Evaluation of Prompt Caching for Long-Horizon Agentic Tasks.* 2026. https://arxiv.org/abs/2601.06007
