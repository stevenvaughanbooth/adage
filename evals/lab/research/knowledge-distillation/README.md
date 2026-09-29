# Knowledge distillation: a repository card learned from traces

2026-09-29. Slug `knowledge-distillation`. Offline, over traces already on disk; $0 spent (no LLM calls, no live sessions).

## Question

Can we automatically distil durable repository knowledge from past traces into a compact card, and deliver the right piece at the right moment? The card would cover how to build and test, where things live, gotchas, and which commands fail and why. Does it beat the AGENTS.md baseline (RepoBench's `agents-md` arm)?

## Verdict: negative

1. **The ceiling is small.** At perfect memory and 100% uptake, durable knowledge is worth about 13% of $ and 7–8% of calls per session. That is below the North star's 15–20%.
2. **The learned card beats AGENTS.md on paper.** At 100% uptake it saves 8.1% of $ against AGENTS.md's 2.2%.
3. **Agents act on only 26–44% of a start-of-session card.** The live agents-md arm shows this. Calibrated to that uptake, the card is worth **+1.2% of $ [−3.5, +4.6]**.
4. **The harness headline is null.** Delivered contextually at PreToolUse and scored by the common harness, it is **−0.2% of $ [−1.9, +1.5]**.
5. **Most of the card's value needs no learning.** A plain `git ls-files` file map gets 76% of it.

## Method

Code: `kd_load.py`, `facts.py`, `learner.py`, `evaluate.py`, `calibrate.py`, `final.py` and `harness_policy.py`. No tool-name or error-string lists are used anywhere in the learner.

1. **Discovery events (`facts.py`), from learned structure.**
   - **Information flow.** Each call's input and output is split into atoms: identifier/path tokens, with path prefixes and basenames. An atom is *discovered* by call k when it first appears in k's output and a later call's input uses it.
   - **Prior knowledge is removed.** Two kinds of atom are dropped, both learned from the *other* repositories' traces only:
     - atoms that occur in ≥2 other repositories (English, stdlib, `tests`, `src`, …);
     - atoms the model usually writes with no provenance.
   - **Failure → recovery.**
     - A call flagged `is_error` is followed within 8 calls by a *roughly equivalent* call (potion-8M cosine ≥ 0.8) that succeeded.
     - The chain is the failed attempts plus the probes whose output flowed into the recovery.
     - The fix is what the recovery added, plus any in-between actions (≤2, e.g. `pip install -e .`).
     - An edit in between means the failure was not a knowledge gap, so the chain is dropped.
2. **Candidate facts and clustering (`learner.py`).**
   - Each event's needed atom set is a candidate fact.
   - Fix facts are clustered by potion-8M embedding of "failing call → fix" (average linkage, cosine ≥ 0.85). Each cluster is rendered by its medoid.
   - Locator facts are paths and identifiers, so they are counted exactly.
3. **Ranking and selection.**
   - Selection is greedy budgeted max-coverage. The value is the calls a fact saves in the train sessions, under a leave-task-out support requirement m.
   - Facts are added until the saving no longer pays for carrying their tokens, or the budget is reached.
   - Rendering is verbatim from the trace; no LLM is used.
4. **Delivery.**
   - (a) **Card**: at session start.
   - (b) **Contextual**: at PreToolUse, retrieved by potion-8M cosine between the proposed call and the situations (the preceding call) in which each fact was needed. There is a pre-emptive variant.
5. **Scoring.**
   - A discovery is obviated if the delivered text contains every atom it needed, and was delivered before it was needed. The same scorer is used for the learned card, AGENTS.md and a file map.
   - Savings are in calls and in $ under the cumulative-context model (`common.dollars`). A removed call saves its response share plus its tokens' cache write and every later read. The cost of carrying the delivered text is subtracted.
   - **Strict mode** (`KD_STRICT=1`, the headline) counts only "where things live" (repository paths per `git ls-files`) and failure fixes. Code identifiers read out of files are dropped: a card that names them cannot replace reading the file.
6. **Uptake calibration (`calibrate.py`).**
   - In live v1 agents-md vs control sessions of the same (task, round), I count the calls spent on discoveries that the repository's AGENTS.md already covers.
   - uptake = 1 − agents-md/control.
7. **Protocol.**
   - Train on the train split; test on held-out **control** sessions: 158 sessions, 38 tasks, Haiku 4.5. The common harness's loader was used, which excludes the 35 discarded-run transcripts.
   - Everything is **leave-one-repo-out in method**. The prior uses the other repos only, and the hyperparameters (m, budget, θ; and θ and cap in the harness) are chosen on the other four repositories. The facts come from the repository's own train split, since facts don't transfer.
   - CIs are 90% task-clustered bootstraps.

## Results (held-out, 158 sessions / 38 tasks; baseline $0.260 and 32.3 calls per session)

### Oracle ceilings (`ceiling.py`, lenient scorer, 100% uptake)

| class | calls/session | share of calls | share of $ |
| --- | ---: | ---: | ---: |
| every information-flow discovery (mostly task-specific code reading) | 9.46 [8.75, 10.23] | 29.3% | 59.3% |
| **durable**: re-discovered in sessions of ≥2 *other* tasks (perfect memory) | 2.68 [2.36, 3.04] | 8.3% | 14.2% |
| learnable from the train split (no budget) | 2.35 [2.05, 2.64] | 7.3% | 13.0% |
| durable failure→recovery chains only | 0.67 [0.52, 0.82] | 2.1% | 2.8% |

More history would not help much: the durable oracle, with every other held-out session as memory, is only 1 point above the train-learnable oracle.

### Card vs contextual vs baselines (strict scorer, 100% uptake, parameters chosen leave-one-repo-out)

| method | calls obviated | calls net of carry | share of $ (net) |
| --- | ---: | ---: | ---: |
| **learned card** (m=0, ≤1000 tokens; 8–17 facts, 400–970 tokens) | 1.60 [1.38, 1.82] | **1.18 [0.94, 1.42]** | **8.1% [6.1, 10.1]** |
| same facts, contextual at PreToolUse (θ=0.6, pre-emptive) | 1.35 [1.14, 1.57] | 0.89 [0.64, 1.15] | 6.7% [4.7, 8.9] |
| all candidate facts, contextual (no selection) | 1.32 [1.12, 1.53] | 0.58 [0.32, 0.84] | 6.0% [4.2, 7.9] |
| AGENTS.md (the agents-md arm's file) | 0.49 [0.35, 0.64] | 0.24 [0.10, 0.39] | 2.2% [1.1, 3.4] |
| file map (`git ls-files`, no learning) | 1.99 [1.79, 2.22] | 0.42 [−0.04, 0.85] | 6.1% [3.9, 8.3] |
| AGENTS.md + file map | 2.32 [2.11, 2.52] | 0.49 [0.00, 0.96] | 8.3% [5.9, 10.6] |
| oracle (every train-discovered fact) | 1.57 [1.34, 1.80] | 1.57 | 9.2% [7.3, 11.3] |

Paired, task-clustered comparisons of the learned card:

| comparison | calls net | share of $ |
| --- | ---: | ---: |
| card − AGENTS.md | +0.94 [0.74, 1.13] | +6.4 pts [4.7, 8.2] |
| card − file map | +0.76 [0.41, 1.14] | +1.3 pts [−0.3, +2.8] |
| card − contextual (same facts) | +0.29 [0.19, 0.39] | +1.4 pts [0.9, 1.9] |

The lenient scorer (code identifiers counted) gives card 9.4% of $ and 1.39 calls net, against AGENTS.md at 3.1% (`results/summary.json`).

What the card obviates, by kind (instrument, `breakdown.py`, strict): other Bash 1.0 per session (mostly `find`/`grep` listings), failed calls 0.48 and Grep/Glob 0.11. The top facts are partial file maps, such as "`find . -name "*.go" | head -20` shows: args.go, command_setup.go, flag.go…". Examples: urfave-cli +0.69 calls [0.50, 0.88] in 11 of 16 sessions, and marshmallow +0.53 [0.17, 0.90] (`results/strict/fact_value.json`). Cards are in `results/strict/cards/`.

Leave-one-repo-out per repository (strict card; share of $ / calls net):

| repository | learned card | AGENTS.md | file map | oracle |
| --- | ---: | ---: | ---: | ---: |
| attrs | 6.1% / 0.88 | 1.9% / 0.19 | 8.6% / 0.96 | 7.4% |
| click | 9.6% / 1.58 | 4.5% / 0.60 | 9.8% / 1.38 | 10.7% |
| marshmallow | 16.6% / 1.80 | 4.6% / 0.59 | 11.2% / 1.02 | 17.3% |
| urfave-cli | 13.1% / 1.80 | −0.9% / −0.25 | 14.0% / 1.83 | 14.2% |
| vue-core | 1.0% / 0.18 | −0.7% / −0.21 | −4.3% / −2.73 | 2.8% |

### Uptake: what the live agents-md arm says (`calibrate.py`, v1, 76 pairs, 38 tasks)

| | control | agents-md | uptake [90% CI] |
| --- | ---: | ---: | ---: |
| calls on discoveries AGENTS.md covers (lenient) | 0.66 | 0.37 | **0.44 [0.17, 0.65]** |
| … locator facts | 0.42 | 0.16 | 0.63 [0.38, 0.80] |
| … failure fixes | 0.25 | 0.21 | 0.16 [−0.58, 0.58] |
| calls on discoveries AGENTS.md covers (strict) | 0.41 | 0.30 | 0.26 [−0.23, 0.58] |
| total calls per session (censored at 30 turns) | 28.4 | 28.6 | – |

The whole-session live result for agents-md vs control is $ −1.6% [−7, +4], calls +0.25 [−0.6, +1.1] and pass rate +0.0 pts [−9, +8]. 57 of 76 control sessions hit the 30-turn cap (`live_agents_md.py`).

Calibrated, with a joint bootstrap of uptake and savings; carry is always paid:

| method | share of $ | calls net |
| --- | ---: | ---: |
| learned card, strict | **+1.2% [−3.5, +4.6]** | −0.01 [−0.81, +0.56] |
| learned card, lenient | +2.7% [−0.9, +5.7] | +0.11 [−0.57, +0.65] |
| contextual, same facts (strict) | +0.7% [−3.4, +3.7] | −0.11 [−0.79, +0.38] |
| AGENTS.md (strict) | +0.0% [−1.4, +1.2] | −0.12 [−0.37, +0.06] |
| file map (strict) | −2.2% [−8.1, +2.2] | −1.06 [−2.21, −0.23] |

The card breaks even at an uptake of about 0.22.

### Common harness headline (`harness_policy.py`, test set `6a795a362ccb`)

The policy is the learned facts delivered contextually:
- at SessionStart, the facts first needed at session start, as one `read_files` directive;
- at PreToolUse, the best-matching undelivered fact above θ: a `run` for a fix, `read_files` for a locator.

θ and a per-session cap were chosen leave-one-repo-out: θ=0.8 with cap 1 on click, marshmallow and urfave-cli; θ=0.95 with cap 3 on attrs and vue-core.

| policy (158 Haiku held-out control sessions, 38 tasks, baseline $0.260, 32.3 calls) | $ saved/task | share of $ | calls saved/task |
| --- | ---: | ---: | ---: |
| KD contextual, p_follow 1.0 | −$0.0006 [−0.0049, +0.0038] | −0.2% [−1.9, +1.5] | +0.23 [−0.31, +0.77] |
| KD contextual, p_follow 0.5 | −$0.0010 [−0.0032, +0.0013] | −0.4% [−1.2, +0.5] | +0.06 [−0.22, +0.33] |
| (reference) U11 predictive, split, same 158 sessions (harness README) | | +3.5% | +1.63 |

Other harness details:
- Ŝ(Rust) is +1.12 [0.74, 1.52], with 184 directives, useful rate 0.62 and harm 0.
- The two-call misfire charge on the 38% of directives that are not useful eats the gross saving.
- With no cap, delivering every matching fact goes to −7.6% of $.
- By repository: urfave-cli +5.0%, marshmallow +2.4%, attrs +1.8%, click +0.5%, vue-core −5.3%.
- `read_files` is an uncalibrated class.
- `scan_memory` found no held-out prompt text in the stored facts.
- Harness `loro()` isn't meaningful for per-repo facts, because with no memory of the repository nothing is delivered.

## Why it fails, and what it tells us

- **Durable repository knowledge is a small slice of the waste.** 29% of calls consume information found earlier in the session, but four fifths of that is task-specific: reading the code of this bug. What recurs across tasks is the file layout, the test command and a few environment fixes. That is about 2.4 calls, or 7% of calls.
- **"Where things live" dominates, and it needs no learning.** A file list recovers most of it. Learning only picks the relevant slice more compactly: +0.76 calls net over the file map, with $ inside noise.
- **Uptake is the binding constraint.**
  - Live agents skip 44% of the discoveries AGENTS.md covers (63% for locations, and ~16% for failure fixes, which is indistinguishable from 0). Whole-session totals don't move.
  - Carrying ~500–1000 tokens for the whole session costs about 2.5% of $, so a card needs an uptake above ~0.22 just to break even.
- **Contextual delivery does not rescue it.**
  - At 100% uptake it is *worse* than the card: facts are needed early, often before any matching situation.
  - In the harness, each extra delivery risks a misfire.
  - U11's live 41/41 follow rate applies to a verified command after a visible failure, and there is no evidence it transfers to "here are the files" facts.

## Build recommendation

- **Don't build** a distilled card or a PreToolUse fact retriever as a lever. Expected value if shipped: +1% of $ [−3.5, +4.6] as a card, and −0.2% of $ [−1.9, +1.5] as contextual delivery.
- **Reuse the failure→recovery chain extractor** (`facts.extract`, chain part) as a candidate generator for the existing reflex learner. It finds U11-like "X fails here; Y worked" pairs in any repository with no error-string list: similarity-matched retries after `is_error`, with the in-between actions as the fix. Its ceiling is 0.67 calls/session (2.8% of $), in U11's class.
- If a context file ships anyway for other reasons, a `git ls-files` map beats a hand-written AGENTS.md on this scorer, at zero learning cost. Its net value is also inside noise once uptake is applied.

**So what:** shipped, a distilled card is expected to save about 1% of $ per task [−3.5, +4.6] and about 0 calls [−0.8, +0.6]. Contextual PreToolUse delivery is expected to save −0.2% of $ [−1.9, +1.5]. There is no pass-rate effect: live agents-md was +0.0 pts [−9, +8]. Confidence that the offline 8% survives a live A/B is low, because the live AGENTS.md arm already shows the uptake it would need doesn't happen. The lever's ceiling, about 13% of $ at perfect uptake, is below the North star anyway.

## Caveats

- **The scorer is an information-sufficiency counterfactual.** It says a call was unnecessary if the text contained what the call found. It does not model compensation. The lenient mode can credit a card that names code identifiers, so strict is the headline.
- **The uptake calibration comes from one card type (AGENTS.md) and v1 sessions capped at 30 turns.**
- **Only 7–10 training sessions per repository**, all bug-fix tasks, all Haiku 4.5.
- **The chain window (8) and the similarity thresholds (0.8, 0.85) were set a priori, not tuned.**

## Reproduce

```text
cd evals/lab/research/knowledge-distillation
export OMP_NUM_THREADS=1
nice -n 10 python3 live_agents_md.py      # live agents-md vs control (state.json only)
nice -n 10 python3 ceiling.py             # oracle ceilings
for m in 0 1; do KD_STRICT=$m nice -n 10 python3 evaluate.py; KD_STRICT=$m python3 calibrate.py; \
  KD_STRICT=$m python3 final.py; KD_STRICT=$m python3 fact_value.py; KD_STRICT=$m python3 breakdown.py; done
cd .. && nice -n 10 python3 knowledge-distillation/harness_policy.py   # common-harness headline
```

The model is potion-base-8M, from `/tmp/adage-lab-models`. Caches (≈40 MB) are in `/tmp/adage-research/knowledge-distillation`. The whole pipeline runs in about 5 CPU-minutes.
