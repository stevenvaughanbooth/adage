# Model economics: is Adage's value "make the cheap model good enough"?

2026-09-29, slug `model-economics`. Offline, $0; `crates/` untouched. The code is `analysis.py`
(`OMP_NUM_THREADS=1 nice -n 10 python3 model-economics/analysis.py` from `evals/lab/research`, about
3 minutes). It writes every number to `results.json`, and the summary is in `result.json`.

## Question

Waste is concentrated in weaker models: Haiku has about 2–3× Sonnet's friction and dead-end calls.
So is Adage's real pitch "make Haiku good enough"? Five parts:

1. What are $/task, $/resolved task, calls and pass rate for each model on the shared tasks?
2. How much of the Haiku–Sonnet gap could procedural-waste levers remove?
3. What share of Haiku's failures are wrong fixes, which hooks can't close?
4. Is "Haiku + Adage" cheaper per resolved task than Sonnet, and under both cache TTLs?
5. Are the prices right?

## Verdict (the "so what" first)

- **Haiku is already cheaper per resolved task than Sonnet on these tasks, without Adage.**
  - $/resolved task: **$1.04 vs $1.45**, a Sonnet/Haiku ratio of **1.40× [0.98, 2.04]**.
  - Pass rate: **40% vs 44%**, a gap of +4 pts [−10, +17].
  - Sonnet costs 1.55× Haiku per session, not 2×, because Haiku makes 39% more calls (43.9 vs 31.6).
- **Adage makes that ratio bigger; it doesn't create it.** Across the lever scenarios, Haiku + Adage
  costs 1.5–15% less, and the ratio moves from 1.40× to **1.43–1.65×**.
  - Central scenario: **1.53× [1.07, 2.22]**, a saving of **$0.50 per resolved task [0.07, 1.52]**.
  - Choosing the model is worth $0.42 per resolved task. Adage adds about $0.08 on top.
- **The quality gap that exists is not procedural.**
  - **16 of 24 Haiku failures (67%) are wrong fixes.** The agent edited the gold file, stopped on
    its own believing it was done, and failed a median of 4% of the hidden tests.
  - Another 2 break the build. 6 hit the 60-turn cap after editing.
  - Sonnet fails the same way: 7 of its 9 failures are wrong fixes.
  - Half the tasks (8 of 16) are failed by *every* session of both models. They are hard, and no
    model or hook choice changes that.
- **The claim that Haiku wastes more holds, and it sizes the levers rather than the pitch.**
  - Deleting every failed or friction call (oracle) is worth **12.2% [9.6, 14.7] of Haiku $** and
    **5.8% [2.8, 9.5] of Sonnet $**, which is 4.9 against 2.0 calls.
  - That explains about 24% of Haiku's 12-call excess.
  - So Adage's cost levers are worth about 2× more on Haiku, and they should be model-conditional.

**So what.** Haiku + Adage (central, −8% of Haiku $) is 1.53× [1.07, 2.22] cheaper per resolved
task than Sonnet 5 on click and vue-core. Most of that is Haiku's price (1.40× without Adage).
Adage's own contribution is −1.5% to −15% of Haiku $, +0 to +1 pass-rate points, and −1 to −2 calls
net.

- **"Haiku ≤ Sonnet per resolved task" surviving a live A/B: moderate confidence.** The Sonnet side
  is 16 sessions, one per task. The 30-turn v1 data show a bigger pass gap (+16 pts [3, 28]) but the
  same cost ratio, 1.47× [1.02, 1.95].
- **Adage "making the cheap model good enough" in quality: low confidence.** Two thirds or more of
  Haiku's failures are wrong fixes.
- **Confirming micro-slice:** 3 more Sonnet repeats on each of the 8 non-trivial tasks (about $15) to
  pin down the Sonnet pass rate.

## Data

- **Matched set:** `harness.load().held_out(repo=["click", "vue-core"], source="v2")`, control arm,
  60-turn cap.
  - Haiku 4.5: **40 sessions / 16 tasks**, 2–3 per task.
  - Sonnet 5: **16 sessions / 16 tasks**, one per task.
  - Both models ran on the same 16 tasks.
- **Robustness sets:**
  - v1 control, 30-turn cap: 32 Haiku and 16 Sonnet sessions, same 16 tasks;
  - v2 with all eval arms pooled (control, adage, agents-md): 80 and 32. The randomised arms moved
    pass rate by about 0 in success-uplift.
- **CIs:**
  - per-model rows: 90% task-clustered bootstrap (`harness.stats`);
  - paired rows: both models resampled on the same drawn tasks (5000 draws);
  - pass rates: also Wilson.
- **Weighting.** Headline numbers are **task-balanced**: per-task means, then the mean over tasks.
  Haiku has 2–3 sessions per task and Sonnet 1, so this keeps the task mix identical. Pooled
  (session-weighted) ratio estimates are in `results.json` and agree (Haiku $0.98, Sonnet $1.45).

## Prices (checked)

| $/MTok | input | output | cache read | cache write 5m (1.25×) | cache write 1h (2×) |
| --- | ---: | ---: | ---: | ---: | ---: |
| Haiku 4.5 | 1.00 | 5.00 | 0.10 | 1.25 | 2.00 |
| Sonnet 5 | 2.00 | 10.00 | 0.20 | 2.50 | 4.00 |

- **Sources agree.** These match the claude-api reference (cached 2026-09-25) and `pricing.toml`
  (as_of 2026-06-24). Sonnet 5.5, the current Sonnet, has the same price.
- **These runs pay the 1-hour rate.** 100% of cache-write tokens in these sessions are 1-hour writes.
  The harness cost model reads that from each usage record and reproduces the billed
  `total_cost_usd` with a ratio of 1.000 for both models.
- **The 5-minute column re-prices every write at 1.25×.** token-cost found almost no inter-call gap
  over 5 minutes, so that TTL would have served the same reads.

## Results

### 1. Per model, same 16 tasks (v2 control, 60-turn cap)

| | Haiku 4.5 (40) | Sonnet 5 (16) | Sonnet ÷ Haiku or gap |
| --- | ---: | ---: | ---: |
| $/task, 1h TTL (actual) | 0.410 [0.343, 0.479] | 0.636 [0.448, 0.841] | 1.55× [1.24, 1.85] |
| $/task, 5m TTL | 0.369 | 0.571 | 1.55× |
| calls/task | 43.9 [38.4, 49.3] | 31.6 [24.4, 39.1] | −12.3 |
| pass rate | 39.6% [20.8, 58.3] | 43.8% [25.0, 62.5] | +4.2 pts [−10.4, +16.7] |
| **$/resolved, 1h** | **1.04 [0.65, 2.05]** | **1.45 [0.78, 3.14]** | **1.40× [0.98, 2.04]** |
| $/resolved, 5m | 0.93 [0.58, 1.85] | 1.31 [0.70, 2.84] | 1.40× [0.97, 2.04] |
| break-even Haiku pass rate | 28% [14, 46] | | |

The break-even pass rate is the Haiku pass rate at which the two models cost the same per resolved
task.

Robustness:

| set | pass gap (pts) | Sonnet ÷ Haiku $/resolved |
| --- | ---: | ---: |
| v1, 30-turn cap | +15.6 [3.1, 28.1] | 1.47× [1.02, 1.95] |
| v2, all eval arms | +6.8 [−2.1, +15.6] | 1.22× [0.94, 1.57] |

The turn cap hurts Haiku more: its sessions are longer, and 10 of its 21 v1 failures were capped.
The cost ratio stays near 1.2–1.5× in every cut.

### 2. Procedural waste, and how much of the gap it is

| oracle class (harness instrument) | Haiku calls | Haiku % of $ | Sonnet calls | Sonnet % of $ |
| --- | ---: | ---: | ---: | ---: |
| failed calls (is_error or env signature) | 3.9 | 10.5 [8.0, 12.8] | 1.4 | 4.6 [2.1, 7.5] |
| environment friction | 2.5 | 4.7 | 1.4 | 3.8 |
| **failed ∪ friction** | **4.9** | **12.2 [9.6, 14.7]** | **2.0** | **5.8 [2.8, 9.5]** |
| U11 predictive, measured here (p_follow 1) | 0.86 | 1.3 [0.7, 2.0] | 0.43 | 0.5 [−0.4, 1.4] |
| U11 post-hoc (as live) | 0.29 | 0.5 | 0 | 0 |

- **U11 is small here.** vue-core has no `.venv`, and click's friction is light. The brief's +3.9%
  is the Python-only 82-session set. Test-set ids: Haiku `b871bcbd310f`, Sonnet `6383e51aa75d`.
- **Procedural waste is about a quarter of the calls gap.** It is 2.9 of Haiku's 12.3 extra calls
  per task.
- **It explains none of the $ gap, which runs the other way.** Haiku is cheaper per session.
- **It explains little of the pass gap.** Haiku's failed sessions carry 13.4% procedural $, against
  9.9% in its passed sessions. Every procedural lever measured in this sprint has a pass-rate effect
  of about 0 (success-uplift: ≤ +1 pt, upper CI ≤ +3.3).

**The lever stack applied to Haiku.** Shares are imported from other agents' results; the U11 row is
re-measured per session here. Stacking is multiplicative.

| scenario | contents | Haiku $ removed |
| --- | --- | ---: |
| conservative | U11 predictive at p_follow 0.5; output shaping at token-cost's central +1% | 1.5% |
| central | U11 predictive at p_follow 1; half of waste-anatomy's learned output cap (click 10%, vue-core 6.3%, leave-one-repo-out); file-pointer hint at p_follow 0.5 (+1.9%); half the commit habit (1.5%) | 8.1% |
| optimistic | as central, but the full output cap, the hint at p_follow 1 (+4.0%) and the full commit habit (3%) | 15.0% |
| oracle | every failed or friction call deleted | 11.5% |

Caveat: two sprint results disagree on the learned output cap. waste-anatomy says −11%, token-cost
says −2 to +4% under its harm models. The scenarios span that disagreement.

### 3. Why Haiku fails (hand-coded instrument over the scorer's `state.json`)

| failure class | Haiku v2 control | Haiku v2 all arms | Haiku v1 (cap 30) | Sonnet v2 control |
| --- | ---: | ---: | ---: | ---: |
| **wrong fix, agent finished on its own** | **16 / 24 (67%)** | **34 / 50 (68%)** | 4 / 21 | **7 / 9** |
| hit the turn cap after editing | 6 (25%) | 12 | 10 | 0 |
| breaks the build or import | 2 | 4 | 0 | 1 |
| no source change | 0 | 0 | 7 | 1 |
| touched the gold file | 23 / 24 | 48 / 50 | 12 / 21 | 8 / 9 |

- **Most failures are close but wrong.** The median failing session fails **4%** of the hidden
  tests: the right file, and a nearly right but wrong fix.
- **These are not closable by hooks: 67–75% of Haiku failures,** counting the build breaks. The
  learned-outcomes and success-uplift results found no reflex that turns these into passes.
- **Turn-cap failures (25%) are the only procedural slice.** Doubling the cap from 30 to 60 turns
  took Haiku's pass rate from 34% to 40%, for +14 calls per session. A lever that frees about 3–5
  calls is worth roughly +1 pt at most.
- **Task structure.**
  - Every v2 session of both models fails 8 of the 16 tasks; 7 of those 8 also fail every v1 session.
  - On the other 8 tasks, Sonnet beats Haiku on 3 (1/1 against 1/3, 1/3 and 2/3).
  - Haiku beats Sonnet on 1 (vue-core-54097087: 2/2 against 0/1).

### 4. Routing: Haiku + Adage vs Sonnet, per resolved task

| policy | Haiku $/resolved (1h) | Sonnet ÷ Haiku (1h) | saving per resolved (1h) | Haiku $/resolved (5m) | Sonnet ÷ Haiku (5m) |
| --- | ---: | ---: | ---: | ---: | ---: |
| plain Haiku | 1.04 | 1.40× [0.98, 2.04] | $0.42 [−0.03, 1.39] | 0.93 | 1.40× [0.97, 2.04] |
| + Adage, conservative (−1.5%) | 1.02 | 1.43× [0.99, 2.07] | $0.43 [−0.01, 1.41] | 0.92 | 1.42× |
| **+ Adage, central (−8.1%)** | **0.95** | **1.53× [1.07, 2.22]** | **$0.50 [0.07, 1.52]** | 0.86 | 1.53× [1.07, 2.21] |
| + Adage, optimistic (−15%) | 0.88 | 1.65× [1.16, 2.39] | $0.57 [0.14, 1.62] | 0.79 | 1.65× [1.15, 2.39] |
| + optimistic and +3.3 pass pts (unmeasured) | 0.81 | 1.79× [1.28, 2.62] | $0.64 | 0.73 | 1.79× |

Sonnet costs $1.45 per resolved task on the 1-hour TTL and $1.31 on the 5-minute TTL.

- **The TTL doesn't change the ratio.** The 5-minute TTL is about −10% for *both* models: it is a
  pricing configuration, not a routing lever.
- **Adage is a small part of the answer.** Without Adage the routing answer is already "Haiku", with
  a CI lower bound just under parity (0.98). Adage's contribution is what lifts that lower bound
  above 1.
- **Scope.** This is for two repositories, bug-fix tasks, and a repository Adage has learned (the
  train split). It is not a claim about harder tasks, where the v1 pass gap of 16 points would erase
  it.

## Honest limits

- **Few Sonnet sessions.** There are 16 Sonnet sessions, one per task, so the Sonnet pass rate has a
  ±19-point CI. A 15-point true gap is inside every CI shown. Even so, the Haiku pass rate at which
  Haiku stops being cheaper per resolved task is 28% [14, 46], against the 40% it has.
- **Half the tasks are unsolvable by either model,** so the pass comparison rests on the other 8.
- **The lever shares are imported and uncalibrated.** Only U11 is calibrated live. They come from
  other agents' replays, and one of them is disputed (the output cap).
- **The failure taxonomy is a hand-written instrument.** It uses the scorer's changed-files list,
  the turn-cap subtype, touched-gold, and regexes on the hidden-test tail.

## Files

| file | what |
| --- | --- |
| `analysis.py` | all computations (harness loader, cost model, U11, oracle; paired bootstrap) |
| `results.json` | every number, including the pooled estimators, the per-task table and U11 summaries |
| `result.json` | the sprint summary fields |
