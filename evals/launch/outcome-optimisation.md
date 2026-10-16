> Exploration report, 2026-09-29: optimise outcomes, not frequency. Read-only research; drives the next engine iteration.

# Report: making Adage optimise outcomes instead of mining frequencies

## Summary

Adage should stop asking what agents usually do next and start asking which verified directive, or silence, gets the agent to its next productive event fastest in this situation. Four changes do that, and all of them fit the existing determinism and hook-latency rules:

1. **The candidate set is built from safe actions only.** Candidates come from typed, verified commands (instincts, the static affordance map, and trace commands that passed the effect ceilings). "Silent" is always one of them. `pip install` is never a candidate, so the harm problem is solved by construction rather than by learning. The 16 pip recoveries are still useful data: they show how badly the silent baseline does in that situation, which is exactly why intervening there is worth a lot.
2. **The reward is local and deterministic.** It counts calls and tokens from the trigger to a typed goal event, such as a test run that actually ran tests. Session-level resolve rate and cost become non-inferiority guardrails, not targets.
3. **Directive value is split into parts that can each be estimated from little data:** `A(s,d) = p_follow(s,d)·[Q(s, Sat d) − Q(s, ∅)] − C(d) − λ·p_misfire·m`. Each part gets a hierarchical empirical-Bayes posterior. Instincts ship the priors ("instincts as priors, reflexes as posteriors").
4. **Randomisation happens at the trigger.** The existing per-session propensity is extended with per-route knockouts, so off-policy evaluation (SNIPS and doubly robust) can check and correct the posteriors. Exploration only ever chooses between silence and verified safe candidates.

Contextual-bandit exploration inside the hook, full offline RL (IQL, CQL), and learned function approximators are not feasible at 10–60 sessions per repo. They are the wrong tools here, not something to add later.

---

## 1. Reward definition

### 1.1 Hierarchy

The reward has four levels. Level 0 is a hard constraint, levels 1 and 2 are optimised, and level 3 is monitored only.

| Level | Signal | Role | Why |
|---|---|---|---|
| **L0 Safety** | The directed or rewritten action has an effect class above its ceiling; the agent runs a harm-class action (install into the system or user site, `git push`, deleting files outside the repo) within k calls of a directive | **Constraint.** Never traded against reward | Harm is not a cost to average out. It is a veto |
| **L1 Local goal** | Calls, tokens and $ from the trigger to the first goal event G(s), censored at a horizon H (default 15 calls or session end) | **Primary objective** | Dense, low variance, and close to the directive in time, so less confounded by how hard the task is |
| **L2 Local side effects** | Waste-class calls in the window (W(d) from review §5.2); misfire (the directed action failed); directive token cost C(d) | Terms in the objective | Captures dead ends avoided and the cost of speaking |
| **L3 Session outcome** | Resolve / proxy outcome, `asked_for_help`, total cost | **Guardrail only** (non-inferiority on the control arm) | Noisy and confounded. The 30× run-to-run variance in 2604.22750 makes it useless for learning |

### 1.2 Goal events

Goal events are typed, deterministic detectors, one per situation family, living in `adage-learn` beside `proxy_outcome`:

- **G_test (a test run that ran tests).** The runner is known (pytest, go test, cargo test, vitest, jest) and the output matches a runner-specific executed-tests signature: `collected \d+ items` with N > 0, or `\d+ (passed|failed)`, `ok  \t`, `test result:`, `Tests:`. The exit code is ignored, because a failing test run is still productive. It must not match `no tests ran`, `ModuleNotFoundError`, or `command not found`. **This detector must scan exit-0 output**, which is the same fix as bug 2.
- **G_import.** A `python -c`/script run that imports the project package without `No module named '<pkg>'`.
- **G_locate.** The first edit of a file that is in the session's final diff. This is hindsight, but deterministic from the trace.
- **G_build.** A build or typecheck command that exits 0 after a failing one.

### 1.3 Local reward for one trigger event

For a trigger at call t in situation s, with the first goal at t*:

```
r = −[ Σ_{k=t}^{min(t*,t+H)} cost_k ]  − c_cens·𝟙[t* > t+H]  − C(d)  − λ_m·𝟙[misfire]
cost_k = calls (unit 1) or $ (from the ledger's per-call split)
```

- Censoring is handled by the fixed penalty c_cens = H. A Kaplan–Meier variant is possible but unnecessary at this sample size.
- A harm-class action inside the window sets an L0 flag. It is counted and gated separately and never folded into r.
- Everything is computed from the parse cache, so it is a pure function of the trace bytes. The same code serves learning, replay (Ŝ), and the live micro-slice analysis. **It must be one function** (`adage_learn::outcome::local`), so the estimates stay consistent with each other.

### 1.4 Why local beats session reward here

- **Measured effect size.** U11's effect is 1.85 calls, and the per-trigger sd is about 2–3 calls. The session-level sd is about 15 calls. The same data therefore gives roughly 25–50× more statistical information per unit at the local level.
- **Less confounding.** Task difficulty mostly affects what happens after the goal, not the path from the env dead end to the first working test run.

---

## 2. Learning algorithm under tiny data

### 2.1 Situations and actions

- **Situation key `s`** is a small, typed, coarse-to-fine tuple:
  `(ecosystem, env_kind, trigger_family, phase)`
  - ecosystem: `py`, `node`, `go`, `rust`
  - env_kind: `venv`, `uv`, `poetry`, `none`, `node_modules`
  - trigger_family: a failure signature family (for example `no-module:pytest`, `no-module:<project>`, `cmd-not-found:<runner>`), `after-edit:<role>`, or `session-start`
  - phase: before the first goal, or after an edit
  - Per repo, the key is extended with `repo` and optionally `area`.
  - The hook matches the key exactly with the existing trigger and context machinery. Nothing new happens at hook time.
- **Action set `A(s)`** is `{∅}` plus the typed candidates that pass `io::validate`, the effect ceilings, and verification:
  - verified by a sandbox or static check (affordance map), **or**
  - seen succeeding at least once in a trace **and** replayed successfully.
- Delivery variants are separate actions: SessionStart card line, PreToolUse rewrite, deny-with-reason, after-failure advice. The same command delivered at a different time has a different value.

### 2.2 Method comparison

| Method | Feasible at 10–60 sessions/repo? | Verdict |
|---|---|---|
| **Hierarchical empirical-Bayes action values** | **Yes.** Closed-form conjugate updates in the learner; a table lookup in the hook | **The core. Ship at launch** |
| **Decomposed "value of a directive"** (VoI) | **Yes.** Each factor is estimated from its own abundant data | **The objective the posteriors feed. Launch** |
| **Advantage-weighted candidate selection** (AWR-lite) | **Yes**, as a replacement for support counting when picking among trace-derived candidates | **Launch.** It is the minimal change to the miner |
| **Contextual bandit with safe exploration** | Only as *deterministic, hash-randomised knockouts compiled per learn*. No online updates in the hook | **v0.2** (needs trigger-level propensities) |
| **Off-policy evaluation (IPW/SNIPS/DR)** | Underpowered per repo, workable pooled per situation across repos, and for regression gates | **v0.2** as a gate and calibration check, not the decision-maker |
| **Preference learning from followed/ignored/misfired** | Only as p_follow and p_misfire estimates. Bradley–Terry over directives needs pairwise comparisons that do not exist | **Launch, in reduced form** |
| **Offline RL (IQL, CQL, decision transformers)** | **No.** State space is huge, data is tiny, and a function approximator in the hook breaks explainability | **Never** for this product |

### 2.3 Hierarchical empirical Bayes

For each situation and candidate there are three quantities, each with its own conjugate family:

- **Q(s,a)**, the expected local cost-to-goal when the *agent* takes an action in Sat(a). It is estimated observationally from traces, including traces from before Adage was installed.
  - Model calls-to-goal as Gamma–Poisson (negative binomial), or log-normal on tokens.
  - For the binary "goal within H", use Beta–binomial.
- **p_follow(s,d)**, from follow judgements (after the equivalence-class fix to bug 1). Beta prior.
- **p_misfire(s,d)**, Beta prior.

**Partial pooling.** The pooling levels are global → ecosystem → env_kind → situation family → repo. Each level's posterior mean is the next level's prior mean, with a concentration κ fitted by empirical Bayes (marginal likelihood, or method of moments across the units at that level). This is the same backoff chain `score.rs` already uses (`Beta(s+κm, n−s+κ(1−m))` with the parent as prior), with two changes:

1. The parent is an **abstract situation across repos**, not a shorter context in the same repo.
2. κ is **fitted**, not fixed at 2.

References: Efron–Morris shrinkage, and Gelman et al. ch. 5 for hierarchical beta-binomial.

### 2.4 Directive value, the VoI formula

```
Q(s, ∅)       = cost-to-goal when agents got nothing (control arm plus unexposed traces)
Q(s, Sat d)   = cost-to-goal when the agent's next action was in Sat(d)
A(s,d) = p_follow(s,d)·(Q(s,∅) − Q(s,Sat d))  − C(d)  − λ_m·p_misfire(s,d)·m(s)
```

- This is Howard's value of information adapted to advice. A directive is worth what it changes, times how often it changes it.
- It automatically suppresses directives for things agents already do. If ∅ already leads to Sat(d), then Q(s,∅) ≈ Q(s,Sat d), which is Track G's `1−p_soon` factor derived rather than bolted on.
- With a PreToolUse rewrite, p_follow ≈ 1 by construction, which is why pre-emptive delivery wins.

**Worked example: click, env dead end.**
- Q(∅) comes from the 16 pip and 4 other recoveries in the training traces, which take about 3.3 calls to G_test (control mechanism table).
- Q(Sat `.venv`) comes from the 3 `.venv` recoveries plus the cross-repo prior from attrs and marshmallow, which gives about 1.4 calls.
- p_follow for the after-failure advice is 41/41. As a rewrite it is about 1.
- The advantage is about 1.9 calls minus roughly 0.1 of directive cost.
- **Three good trajectories are enough, because the prior carries the rest.** Frequency needed five, and would have picked pip.

### 2.5 Confounding in Q(s, Sat d)

Q(s, Sat d) is observational: agents that chose `.venv` may be different agents. Mitigations, in order of importance:

1. Condition on s. The situation abstraction is the adjustment set.
2. Use local outcomes, which are much less confounded than session outcomes.
3. Treat the observational estimate as a **prior** that trigger-randomised data (knockouts, and the control arm's `would_have`) overrides.

The per-trigger `delivered` vs `would_have` comparison is already an unbiased estimate of the local effect *conditional on the trigger firing*, because the arm was randomised before it fired. `evals/launch/mechanism.py` is that estimator. It needs to be productised.

### 2.6 AWR-lite for candidate generation

Advantage-weighted regression (Peng et al. 2019) and CRR (Wang et al. 2020) weight behaviour cloning by exp(A/β). In Adage's discrete, tiny setting this becomes a small change in two places:

- In `count_avoid`, `recovery_traces` and the covering-variant choice (`compile::variants`), rank candidate recoveries by **Σ over traces of w_i = exp(−(calls-to-goal_i − baseline)/β)** instead of by count.
- An environment-effect candidate is excluded before weighting.

This is how "learn from good trajectories, not frequent ones" enters the miner without an RL stack. It is also the deterministic, typed counterpart of ReasoningBank's success- and failure-contrastive distillation.

### 2.7 Preference signals

- **Followed** is a follow. **Ignored** only lowers p_follow; it is *not* a negative reward, because an agent may rightly ignore advice. **Misfired** is a direct negative.
- `should_extinguish` (ignore Wilson lower bound ≥ 0.5) should be replaced by: extinguish when P(A(s,d) > 0 | data) < 0.1 with at least n_min = 5 judgements, or when misfires reach 3.
- Bug 1 shows why. A broken judge that feeds the learner destroys value silently. The follow judge (`Sat(d)` as an equivalence class of interpreter and runner) should live in one place and be shared by the runtime and replay.

---

## 3. Cross-repo priors: the "applies to any repo" answer

**What is shared** is only sufficient statistics keyed by abstract situation:
`(ecosystem, env_kind, trigger_family, action_role, delivery) → {n, Σcalls, Σcalls², n_goal, follow a/b, misfire a/b}`.
- `action_role` is typed: `use-project-interpreter`, `targeted-test-for-edited-file`, `repo-wide-check`, `regenerate`, `deny-system-install`.
- No commands, paths, prompts or code are shared. Concrete commands are re-bound per repo by the existing instinct binder (`adage-instincts::bind`).

**How it ships:**
- Each instinct in `data/catalogue.toml` gains a `[instinct.prior]` table per situation: `q_null`, `q_sat`, `kappa_q`, `follow = [a, b]`, `misfire = [a, b]`, and delivery variant.
- Values are fitted offline by an eval script over RepoBench v1/v2, the corpus and dogfood traces, then pinned with the pack version.
- On day 0, a bound instinct's strength comes from its **prior advantage**, replacing today's flat "prime at 0.7 × 3 pseudo-traces".
- Per repo, the learner updates the posterior with the repo's traces and judgements. That is the reflex. When the posterior contradicts the prior, the posterior wins by construction, because κ is finite.

**Later (opt-in):** users may contribute their aggregate sufficient statistics to refit the pack's priors, optionally with DP noise on the counts. That is the only network path, and it needs explicit consent (zero-config §1).

---

## 4. Safety

1. **Action sets are closed and typed.** Exploration and optimisation only choose from `A(s)`, which contains ∅ and candidates that passed `io::validate`, the effect ceilings (`model.rs::ceiling`: Environment is prime and nascent until review; Author and Publish are never directed), `safety.rs` body classification, and verification. The worst an exploratory choice can do is equal the silent baseline plus C(d). This is the conservative-bandit idea (Wu et al. 2016) enforced structurally.
2. **Safe policy improvement.** Following SPIBB (Laroche et al., ICML 2019), deviate from the baseline policy (∅, or the current deployed choice) only in situations where (s,d) has at least N∧ effective samples (prior plus data). Elsewhere, keep the baseline.
   - Strength mapping:
     - **fire** needs the 90% posterior lower bound of A > 0 *and* a fire ceiling;
     - **prime** needs the posterior mean of A > 0 and P(A > 0) ≥ 0.8;
     - anything else is silent or dormant.
   - Value can only lower strength below the ceiling, never raise it past it.
3. **The harm guardrail is a veto, not a term.** For each route, track the harm-in-window rate on treated vs `would_have` triggers. Suspend the route if its Wilson upper bound exceeds 1% and it is above control, and flag it in `review`. A second safe action is also worth shipping: a deny-with-reason on `pip install` into the system or user site when a verified `.venv` exists. It is an avoid whose `instead` is verified, and it directly attacks the 16/23 majority.
4. **Interlocking stays hard where it is about safety.** "Inhibit beats fire" (`approach_lock`) stays unconditional. Where two *safe* routes compete at the same point, which today resolves as "learned beats instinct", the new rule is higher posterior advantage wins, with ties going to the learned route. This fixes instincts losing to a noisy learned route (bug 6's `git log` reflex would have A ≈ 0).
5. **Auto-rollback.** Run a CUSUM on each route's local reward against its `would_have` counterparts, as Track G §5 E4 planned. Two consecutive alarms weaken the route to forming.

---

## 5. Evaluation

### 5.1 Offline

- **Ŝ (review §5.2) with follow correction.** Replay assumes the directive is followed. Weight each S(d_t) by the posterior p_follow(s,d), and add a misfire penalty.
- **Calibration gate (unchanged):** replay must predict U11's live −1.85 calls within [−2.56, −1.17].
- Report Ŝ, the harmful share, and a precision lower bound for three policies: **Frequency** (the current engine plus a majority-recovery candidate), **Outcome** (EB + VoI), and **Instinct-only**.
- **Off-policy estimators on logged spools.** Spools already hold `decision.would_have` in both arms plus session propensity. At the trigger level:
  - **Now:** a difference in means between triggers delivered in treated sessions and triggers that would have been delivered in control sessions (randomised, so unbiased). This is mechanism.py, generalised per route.
  - **After knockouts:** SNIPS and doubly robust (Dudík et al. 2011) for *new* policies. The direct-method model is the EB posterior Q̂. Clip propensities to [0.05, 0.95] and report effective sample size. A policy with ESS below 20 at a situation is "not evaluable", which is an honest result at this data size.
  - This is the estimator stack of the Microsoft Decision Service and Vowpal Wabbit (Agarwal et al. 2016) and the Yahoo news replay evaluation (Li et al. 2011), scaled down.

### 5.2 Live micro-slices

These follow review §5.3, randomised at the trigger with logged propensity, using forced-situation starts on fresh checkouts at about $0.03 per session.

### 5.3 The minimal experiment that shows outcome beats frequency on RepoBench

The two policies only differ where frequency picks a different action than outcome. So test exactly there.

- **Setting:** the Python dead-end situations (`no-module:pytest`, `no-module:<project>`) on click, attrs and marshmallow. Instincts are **off** in both policies, so the comparison is learned vs learned.
- **Learning:** leave-one-repo-out. Priors are fitted on the other two repos' training traces, and the posterior on the target repo's training traces (7–10 sessions).
- **Policies:**
  - **F:** the current engine at `--demo` thresholds, plus the most frequent safe recovery (whatever passes the ceilings; with pip excluded it is usually nothing or a rerun).
  - **O:** EB + VoI over the same candidate set plus the affordance-map candidates.
- **Offline stage (no spend):** Ŝ on the v2 held-out traces for F vs O, plus the delivered-vs-`would_have` OPE on v2 spools for the situations O would fire.
- **Pre-registered success criterion:** Ŝ_O − Ŝ_F ≥ 1 call per affected session, with the task-clustered bootstrap lower bound > 0, and harmful share of O ≤ F.
- **Live stage (about $5):** 150 Haiku forced-situation sessions (50 per repo). At the first qualifying trigger, randomise three ways with p = 1/3: ∅, F's directive, O's directive.
  - Primary endpoint: calls from trigger to G_test.
  - Secondary endpoint: harm-in-window (a pip install into the system Python).
  - With a per-trigger sd of about 2.5 and 50 per arm, the SE of the difference is about 0.5, which detects 1.5 calls.
  - **Success:** O − F ≤ −1 call with the upper 95% bound < 0, and O's harm rate non-inferior (margin 2 pts).
- **Session-level follow-up:** only then run a budget-matched RepoBench v2 rerun, with non-inferiority on resolve rate and task pairing / CUPED.

**Honest expectation.** O should win in this situation because F has nothing safe to say, but the whole-repo gain stays in the 5–10% ceiling from review §1.2. What this proves is the *mechanism*: the learner now picks the rare good action, and it will keep doing so on repos where the gains are larger.

---

## 6. Staged implementation plan

### Launch (v0.1, about 1–1.5 weeks after bugs 1–3)

| # | Crate / file | Change |
|---|---|---|
| L1 | `adage-core::pattern` + `adage-runtime` | `Sat(d)` equivalence classes (interpreter + runner, arguments free) used by `meets` for follows. Signature scan of exit-0 output (bug 2). Prerequisite for any outcome learning |
| L2 | new `adage-learn/src/outcome.rs` | Goal detectors (G_test, G_import, G_locate, G_build), `SituationKey`, `local(trace, t, d) -> LocalReward`. One function, reused by `adage-eval` replay |
| L3 | `adage-graph/src/score.rs`, `stats.rs` | Add `Q`/`A` beside `V`: Gamma–Poisson or Beta posteriors with a *supplied* parent prior and fitted κ. `V` becomes the VoI formula (§2.4). Keep `Scored` fields for `why` |
| L4 | `adage-learn/src/engine.rs` | Admission changes from `n ≥ min_support` to verified ∨ (posterior A lower bound > 0 at ≥ N∧ effective samples). `count_avoid`/`recovery_traces` choose `instead` by advantage weighting (§2.6). `strength_for` maps from posterior A under the ceiling. `one_task` stays |
| L5 | `adage-instincts/data/catalogue.toml` + `bind.rs` | `[instinct.prior]` per situation. U11 and a "project-package import" sibling (bug 5) with PreToolUse rewrite as the preferred delivery. Priors fitted by a committed script in `evals/launch/` over v1/v2 traces |
| L6 | `adage-learn/src/instincts.rs` | `should_extinguish` → posterior P(A > 0) < 0.1, or misfires ≥ 3. Ignores only lower p_follow |
| L7 | `adage-runtime` `Decision` / spool | Per directive: `trigger_seq`, `situation`, `candidates`, `propensity` (1.0 until knockouts). `would_have` already exists |
| L8 | `adage-eval` | Ŝ with follow correction, plus a `delivered vs would_have` per-route estimator (mechanism.py in Rust). Gate: U11 calibration |

### v0.2

| # | Crate | Change |
|---|---|---|
| E1 | `adage-runtime::assign_route(session, route, trigger_seq, share) -> (deliver, p)` | SHA-256 hash knockouts, **compiled share per route**: a floor of 0.05, rising to 0.3 while the posterior is uncertain (P(A > 0) between 0.2 and 0.9). This is Thompson-flavoured exploration done at learn time, deterministic given (index, session). The hook stays O(1) |
| E2 | `adage-graph::compile` | Multi-candidate situations compile to a small policy table `{action: prob}` (clipped). The hook draws by hash, never an RNG |
| E3 | `adage-eval ope` | SNIPS and DR per route and situation, ESS reported. Used as a release gate ("no route with DR upper bound < 0") and to refit priors |
| E4 | `adage-learn` | CUSUM auto-weakening. Interlocking by posterior advantage among safe routes |
| E5 | `adage-cli savings` | Per-route indicative savings from the trigger-level estimator (more power than session ITT), labelled as such |

### Later

- Opt-in aggregate sufficient-statistic sharing for prior refits, with DP.
- A case store (review §3.2) as another *candidate generator*, with outcome weighting from the same `outcome.rs`.
- The session-level control share could drop to 0.05 once trigger-level knockouts carry the measurement.

### Never

- Model updates at hook time.
- Neural offline RL.
- Letting session pass/fail drive per-repo decisions.
- Any exploration over actions outside `A(s)`.

---

## 7. What already exists, and what is missing

- **Exists:**
  - Session-level arms with logged propensity (`adage-runtime/src/lib.rs::assign`, spooled in `adage-hook/src/lib.rs:338,478`).
  - `would_have` exposure logging in both arms.
  - The Hajek-IPW savings estimator with a gate (`adage-cli/src/savings.rs`).
  - Beta posteriors with parent backoff (`adage-graph/src/score.rs`).
  - Outcome lift using session outcomes (`adage-tracks/src/lift.rs`). Its unit is wrong for this purpose; it should be local reward.
  - Ceilings (`adage-tracks/src/model.rs:436`) and interlocking.
- **Missing:**
  - Per-route knockouts. The Architecture doc marks A12 knockouts "Not built", and there is no `knockout` in code.
  - Trigger-level propensity.
  - Goal detectors and local reward.
  - Cross-repo priors: instincts carry no statistics today.
  - A follow judge that is correct for narrowed runners (bug 1).

---

**Repo files read:**
- `/home/user/adage/evals/launch/architecture-review.md`
- `/home/user/adage/evals/launch/README.md`
- `/home/user/adage/docs/tracks/track-g-graph-engine.md` (§5 scoring, §6.3 priors, real-traces note)
- `/home/user/adage/crates/adage-runtime/src/lib.rs`
- `/home/user/adage/crates/adage-learn/src/engine.rs`
- `/home/user/adage/crates/adage-learn/src/evidence.rs`
- `/home/user/adage/crates/adage-learn/src/instincts.rs`
- `/home/user/adage/crates/adage-tracks/src/lift.rs`
- `/home/user/adage/crates/adage-tracks/src/model.rs`
- `/home/user/adage/crates/adage-tracks/src/params.rs`
- `/home/user/adage/crates/adage-graph/src/score.rs`
- `/home/user/adage/crates/adage-instincts/src/{lib,catalogue,safety}.rs`
- `/home/user/adage/crates/adage-instincts/data/catalogue.toml`
- `/home/user/adage/crates/adage-cli/src/savings.rs`
- `/home/user/adage/docs/zero-config.md`

**Prior art cited:**
- **Bandits and off-policy evaluation:**
  - Li et al. 2010 (LinUCB, Yahoo news)
  - Li et al. 2011 (unbiased offline replay evaluation)
  - Agarwal et al. 2016 (Decision Service / Vowpal Wabbit, "Making Contextual Decisions with Low Technical Debt")
  - Dudík, Langford and Li 2011 (doubly robust)
  - Swaminathan and Joachims 2015 (SNIPS)
  - Wu et al. 2016 (conservative bandits)
- **Safe policy improvement:**
  - [Laroche et al., SPIBB, ICML 2019](https://arxiv.org/abs/1712.06924)
  - [Soft-SPIBB](https://arxiv.org/abs/1907.05079)
  - Thomas et al. 2015 (high-confidence off-policy evaluation)
- **Offline RL:**
  - Peng et al. 2019 (AWR, arXiv 1910.00177)
  - Wang et al. 2020 (CRR)
  - Kostrikov et al. 2021 (IQL)
- **Process mining:**
  - Teinemaa et al. 2019 (outcome-oriented predictive process monitoring benchmark)
  - [Shoush & Dumas 2022, "When to intervene?"](https://arxiv.org/abs/2206.07745)
  - [Bozorgi et al., causal prescriptive monitoring](https://arxiv.org/abs/2109.02894)
- **Agent memory:** ReasoningBank ([2509.25140](https://arxiv.org/abs/2509.25140)), AWM, and Hajimiri et al. 2026 on budget matching, all as in the review.
- **Statistics:** Efron–Morris / Gelman et al. on hierarchical empirical Bayes; Howard 1966 on value of information.