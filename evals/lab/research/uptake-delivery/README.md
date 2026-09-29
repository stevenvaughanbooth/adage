# Uptake and delivery: what agents do with a delivered reflex

Slug `uptake-delivery`, 2026-09-29. Offline, $0, over live A/B traces already on disk.

## Question

A reflex is worth only what the agent does with it. From the sessions where directives were actually
delivered, this study measures three things:

- **p_follow**, broken down by directive kind, delivery hook, timing, wording and model.
- **Behaviour change**: how much more often the agent took the directed action than in the matched
  control situation.
- **How wrong today's literal follow judge is.**

It then estimates, off-policy, the value of three alternative delivery choices:

- pre-emptive vs post-hoc;
- once per session vs every time;
- terse vs explained wording.

## Verdict (short)

- **Uptake is not the bottleneck.**
  - Delivered directives are followed **95.8%** of the time (183/191, Wilson 90% [0.93, 0.98]), judged
    by learned similarity.
  - The runtime's literal judge **misjudges 58% of deliveries** (118/202, [0.53, 0.64]). Almost all of
    these are follows it calls "ignored" or never judges.
- **Most deliveries change nothing.**
  - Against the randomised control's `would_have` situations, the agent does the same thing unprompted
    in 50% of cases ([0.41, 0.59]).
  - Only **21% of deliveries changed behaviour**.
  - Every directive family except two has an uplift of about 0: work-item lookup, read-batch, the
    regenerate obligation, the finish sequence, and "run tests" advice at prompt time.
  - The two families with real uplift are the environment dead end (U11 `.venv`, +0.69) and the
    after-edit test run (+0.4 to +0.6).
- **The realised-vs-offered gap is timing and visibility, not uptake.**
  - For the one situation with value (Python env dead end, Haiku), post-hoc delivery realises
    **2.09 calls per trigger** ([1.54, 2.64]). That is 99% of what an immediate post-hoc follow could
    realise.
  - But it is only **62% of the 3.36 calls** ([2.84, 3.90]) that a pre-emptive directive offers,
    because post-hoc delivery pays for the failed attempt first.
  - It also fires in only **64% of the Haiku dead-end sessions** (36% [0.30, 0.44] hit the dead end
    only through exit-0 piped commands) and in **1 of 13 Sonnet dead-end sessions**.
  - As logged, the policy realises **1.33 calls per dead-end session: 40% of the offered ceiling**.
- **Best policy.** Deliver **pre-emptively, once per session, and only where uplift > 0**. Wording is
  immaterial: every wording cell has p_follow ≥ 0.88, and directive carry cost C ≈ $0.0002.
  - Every-time delivery adds nothing: 0 relapses were found in 46 followed sessions.
  - Pre-emptive delivery is worth **3.36 calls and $0.018 per dead-end session** (direct method). That
    is about **3.0 calls (≈9%) and $0.016 (≈6%) per Haiku Python session**, versus 1.2 calls today.
- **Size flag.** This lever tops out below the sprint's 15–20% target on the RepoBench repositories.
  - Delivery policy can only realise what the directive offers, and only one situation there offers
    anything.
  - On a generated repository with several traps (acme), the same pre-emptive delivery cuts calls by
    **59%** and raises the pass rate by **42 points** against hooks off. A static AGENTS.md does even
    better there. So the ceiling is set by what the directives know, not by how they are delivered.

## Data

- **Primary A/B units.** RepoBench v1 (`/tmp/adage-repobench/runs`) and v2 (`/tmp/adage-rb2/runs`):
  attrs, click, marshmallow, vue-core, urfave-cli, the two generated repositories, and the Sonnet 5
  slices. Also the click slice (`scratchpad/ld/slice/click-slice`). Sessions are de-duplicated by id.
- **Secondary units.** Other live slices with logged decisions:
  - click-a3slice;
  - the skill slices on click and on a generated `acme` repository (UserPromptSubmit and Stop
    delivery; their comparison arm is `agents-md`, and acme also has a hooks-`off` arm);
  - the delegation slices;
  - the vue-core/click learning curves.
- **Decision unit.** The first trigger of a route in a session. In the `adage` arm it is delivered
  (`directives`). In `control` it is withheld and logged as `would_have` at the same hook.
  - The arm was randomised per session before any trigger, so first-trigger units are exchangeable
    across arms.
  - That makes the control's `would_have` units the matched control situation, with propensity 0.5
    (known).
  - 375 units in total: 191 delivered and 80 withheld in the clean adage/control pairs.
- **Coverage.**

  | Hook | Delivered in the logs |
  | --- | --- |
  | PostToolUseFailure | 51 |
  | PostToolBatch | 66 |
  | UserPromptSubmit | 67 |
  | Stop | 7 |
  | SessionStart | 0 |
  | PreToolUse | 0 |

  Sonnet has one delivered directive in total. `<adage-signal>` appears in no transcript: every
  delivery uses `<adage-reflex>`.

## Method

### 1. Follow judges

The follow judges are in `judge.py` and `judges_eval.py`. For every unit, each judge looks at the calls
after delivery. The window is 5 calls after a post-failure avoid; for pre-emptive advice and after-edit
obligations it is the rest of the session.

- **literal**: the runtime's own logged judgement (`followed`/`obligation_met` vs `ignored`/none).
- **learned (proposed)**: weighted token containment of the directive in the agent's action.
  - Token weight = log(1 + number of distinct training tasks whose actions contain the token), fitted
    on training-split traces only.
  - Tokens that recur across tasks are how the repository does things (`.venv`, `bin`, `python`, `-m`,
    `pytest`, `make`, `gen`). Tokens seen in one task are arguments, so agents' own paths and flags
    count for nothing.
  - Lexing is generic: split on non-word characters and `/`, first line only.
  - The threshold τ is fitted **leave-one-repo-out**. It comes out as 0.85–0.86 on every fold.
- **Compared against** potion-base-8M cosine, char n-gram TF-IDF cosine, and a "contrastive" variant
  that up-weights directive tokens absent from the failed trigger.
- **Instrument (hand-written, measurement only).** The directive's program part (tokens before the
  first file argument, with `-m module` kept) must start some shell segment of the agent's command,
  after peeling `cd`, env assignments and a venv `activate`. For read-batch directives, the listed
  file must be read.
- **Manual audit.** I read all disagreements between the learned judge and the instrument by hand.
  - The one learned false follow (`~/.local/bin/python -m pytest` taken for `.venv/bin/python -m pytest`) is a genuine error.
  - The literal judge's disagreements are genuine follows (examples below).

### 2. Behaviour change

`analysis.py` compares the follow rate among delivered units (p_follow) with the rate in the withheld
arm (p_self). The difference is the uplift, reported with a task-clustered 90% bootstrap (2,000
resamples).

### 3. Local value

For the environment dead end, local value is the calls and $ from the trigger to the first test run
that ran tests.

- The goal event is **G_test, a hand-written instrument**, as in the outcome-optimisation report.
- $ = the issuing turns at list price plus cache-read carry to session end.
- Censored at 15 calls.
- A learned goal (same intent as the failed call, and output not near a logged failure) is reported
  as a sensitivity. It fires too early on `pip install && python -m pytest` chains, so it understates
  the effect.

### 4. Offered vs realised

| Quantity | Definition |
| --- | --- |
| Q(∅) | calls to goal in control |
| Q(delivered) | calls to goal in the adage arm |
| Q(Sat d) | calls to goal among delivered units followed on the next call |
| Pre-emptive offer | calls from the first failing attempt (inclusive) to the goal in control, minus the goal call |

- **Dead-end hits Adage never saw** are found by learned output similarity to *logged* trigger outputs
  from other sessions. That detector agrees with a hand-written `No module named|command not found`
  instrument on 160/160 v2 sessions.

### 5. Off-policy evaluation

- **Inside the logged support** (deliver vs withhold, per directive family), I use **IPS, SNIPS and
  doubly robust** estimates with the known propensity 0.5. The outcome model is the task's leave-one-out
  mean in each arm.
- **Outside the support** (pre-emptive delivery, every-time delivery, rewrite), the logs have
  propensity 0, so IPS/DR are impossible and I report **direct-method** estimates, labelled as such.
  - The pre-emptive p_follow comes from the skill slice on click. There, UserPromptSubmit advice
    ("running python -m pytest fails … use `.venv/bin/python -m pytest`") was delivered on 4 of the same
    tasks as RepoBench click.

### 6. p_follow model

The p_follow model is a hierarchical empirical-Bayes Beta per (kind × hook), pooled to the global
rate, with κ by method of moments. The same model is fitted for p_self. A logistic regression on hook,
kind, wording and model shows what is identifiable.

## Results (held-out live sessions; 90% CIs)

### Follow judges

| Judge | Agreement with instrument, leave-one-repo-out (375 units) | False follows | Missed follows |
| --- | ---: | ---: | ---: |
| **learned containment (proposed)** | **0.997** | 1 | 0 |
| contrastive containment | 0.979 | 8 | 0 |
| potion-base-8M cosine | 0.949 | 15 | 4 |
| char TF-IDF cosine | 0.936 | 11 | 13 |
| literal (runtime today) | 0.467 | 12 | 188 |

- **The embedding models fail on exactly the distinction that matters.**
  - potion rates the dead end `python -m pytest tests/x.py` at 0.84–0.93 cosine to the directed
    `.venv/bin/python -m pytest …`.
  - It rates a correct `source .venv/bin/activate && python -m pytest …` at 0.34–0.54.
  - Recurrence-weighted containment gets both right with no command knowledge.
- **The literal judge on delivered units** (202):

  | | Value |
  | --- | --- |
  | literal p_follow | 0.376 |
  | true p_follow | 0.950 |
  | misjudged | 118 (0.584 [0.526, 0.640]) |

  By family, literal vs learned:

  | Family | Literal | Learned |
  | --- | ---: | ---: |
  | dead-end avoid | 0.11 | 0.94 |
  | read-batch | 0.00 | 1.00 (never judged) |
  | pre-emptive avoid advice | 0.00 | 1.00 (never judged) |
  | Stop reminder | 0.00 | 0.86 |
  | lookup | 1.00 | 1.00 |
  | run | 0.80 | 0.80 |

  - Its errors run almost all one way: 117 follows called not-followed, 1 the other way.
  - Fed to `should_extinguish`, the literal judge kills the only valuable reflex.

### Uptake vs the matched control

Clean adage/control pairs; learned judge.

| Directive (hook) | Delivered | p_follow | Withheld | p_self | Uplift [90% CI] | Literal p_follow |
| --- | ---: | ---: | ---: | ---: | --- | ---: |
| dead-end avoid `.venv` (PostToolUseFailure), primary | 46 | 0.98 | 41 | 0.29 | **+0.69 [0.52, 0.84]** | 0.02 |
| after-edit test run (PostToolBatch), primary | 5 | 0.80 | 5 | 0.40 | +0.40 [0.00, 0.80] | 1.00 |
| after-edit test run (PostToolBatch), secondary | 5 | 0.80 | 5 | 0.20 | +0.60 [0.20, 1.00] | 0.60 |
| work-item lookup (UserPromptSubmit) | 12 | 1.00 | 12 | 1.00 | 0.00 | 1.00 |
| read-batch (PostToolBatch) | 12 | 1.00 | 8 | 1.00 | 0.00 | 0.00 |
| regenerate obligation (PostToolBatch) | 4 | 1.00 | 4 | 1.00 | 0.00 | 1.00 |
| **pooled** | 191 | **0.958 [0.93, 0.98]** | 80 | 0.50 [0.41, 0.59] | +0.46 [0.32, 0.61] | 0.38 |

- **Share of deliveries that changed behaviour**: 0.21.
- **Against an AGENTS.md arm** (skill slices), every skill directive has uplift 0: avoid-advice,
  run, read-batch, obligation and sequence. The static file already told the agent. The after-edit
  run advice is −0.125 [−0.25, 0].
- **By hook.**

  | Hook | p_follow | p_self | Uplift |
  | --- | ---: | ---: | --- |
  | PostToolUseFailure | 0.94 | 0.29 | +0.65 [0.51, 0.79] |
  | PostToolBatch | 0.94 | 0.65 | +0.29 [0.09, 0.51] |
  | UserPromptSubmit | 1.00 | 1.00 | 0 |
  | Stop | 0.86 | no control | – |

  The hook is confounded with directive kind. The only prompt-time directive with a clean control is
  the lookup.
- **By model.** Haiku dead-end p_follow 0.94, uplift +0.66 [0.52, 0.80]. **Sonnet: one delivered
  directive (followed).** There are no Sonnet p_follow estimates to be had: 12 of 13 Sonnet dead-end
  sessions never trigger (piped `| head`).
- **By wording.**

  | Wording | p_follow | n |
  | --- | ---: | ---: |
  | terse | 0.97 | 110 |
  | explains why | 0.98 | 47 |
  | "must … Agents in this repository run it" | 0.88 | 34 |

  The lower third row is the after-edit `run` kind, not the wording. In the logistic model on the
  delivered units (8 non-follows in 191), no coefficient is identified.
- **Timing (lag from delivery to follow).**

  | Directive kind | Median lag | p90 lag |
  | --- | ---: | ---: |
  | post-failure avoid | 0 calls | 0 |
  | after-edit `run` | 4 calls | 30 |
  | pre-emptive avoid advice | 5 calls | 18 (the need arrives later) |

  In every case the agent acted at the first moment of need.

### Local value of the dead-end reflex (Python, primary)

Task-clustered, first trigger per session, Haiku.

| | Delivered (n=44) | Control (n=39) | Diff [90% CI] |
| --- | ---: | ---: | --- |
| calls, trigger → test run that ran tests | 1.27 | 3.36 | **−2.09 [−2.64, −1.54]** |
| $, same window (turns + carry) | 0.0076 | 0.0169 | **−0.009 [−0.012, −0.007]** |
| calls, first failing attempt → goal | 2.55 | 4.36 | −1.81 [−2.53, −1.13] |
| calls, learned goal (sensitivity) | 2.61 | 2.90 | −0.28 [−1.53, +1.05] |

- **Per repository** (the judge's τ is fitted leave-one-repo-out; nothing else is fitted):

  | Repo | Post-hoc realised (calls) | Pre-emptive offered (calls) | p_self |
  | --- | --- | --- | ---: |
  | attrs | −3.13 [−3.94, −2.34] | 4.31 | 0.25 |
  | click | −1.02 [−1.24, −0.80] | 2.09 | 0.46 |
  | marshmallow | −2.02 [−2.77, −1.02] | 3.75 | 0.00 |
  | click slice | −1.25 | – | – |

- **vue-core** has two v1 `pnpm test` triggers. Both delivered units ended far worse (16 and 14 calls vs
  2 and 1). That is n = 2, but it is the only misfire signal in the data.

### Offered vs realised gap (Python dead end, calls per dead-end session)

| Policy | Calls saved | Share of pre-emptive ceiling |
| --- | ---: | ---: |
| oracle post-hoc (next call is the directed one and runs tests) | 2.36 | – |
| post-hoc as delivered, when it fires (p_follow 0.98) | 2.09 [1.54, 2.64] | 62% |
| post-hoc as logged (fires in 64% of Haiku dead-end sessions) | 1.33 | **40%** |
| post-hoc + exit-0 signature scan (bug 2 fixed) | 2.09 | 62% |
| **pre-emptive, once** (PreToolUse rewrite, or advice at SessionStart/UserPromptSubmit) | **3.36 [2.84, 3.90]**; lower bound at p_follow's Wilson LB: 2.51; $0.0176 [0.015, 0.020] | 100% |

The gap decomposes into three parts; none is uptake:

- **p_follow** costs about 1% of the offer.
- **Post-hoc timing** costs 38%: the failed attempt and its context are already paid.
- **Invisibility** costs a further 22%: piped failures raise no trigger.

### Off-policy estimates

- **Inside the support: deliver vs withhold at the dead-end trigger** (83 units, propensity 0.5).
  Local calls to goal:

  | Policy | DR [90% CI] | IPS | SNIPS |
  | --- | --- | ---: | ---: |
  | deliver | **1.33 [1.08, 1.50]** | 1.35 | 1.27 |
  | withhold | 3.40 [2.90, 3.97] | 3.16 | 3.36 |

  Local $: deliver $0.0077 vs withhold $0.0170.
- **Session-level** (117 primary sessions with a unit). DR difference vs withhold-all:

  | Policy | Calls/session | $/session | Pass rate |
  | --- | --- | --- | --- |
  | deliver-all | +0.56 [−0.85, +2.06] | +$0.007 [−0.004, +0.017] | −8.2 pts [−16, −0.6] |
  | deliver only where uplift > 0 | +0.96 [−0.42, +2.24] | – | −5.4 pts [−11.6, −0.7] |

  - The local 2-call saving is invisible at session level: session sd is about 15 calls.
  - The pass-rate estimate is conditional on sessions with a unit, uncorrected for 6 comparisons, and
    driven by click and a 6-task slice. The full-arm ITT in `narrative-v2.md` is −0.02 [−0.10, 0.04].
    Treat it as a guardrail to watch, not a finding.
- **Outside the support (direct method).**
  - **Pre-emptive vs post-hoc.**
    - On the same 4 click tasks, the first test attempt used the directed form:

      | Arm | First attempt directed | Friction (first attempt → test run that ran) |
      | --- | --- | --- |
      | prompt-time advice (skill slice) | 8 of 8 [0.75, 1.0] | 0.0 calls |
      | RepoBench click post-hoc | 3 of 19 | 1.42 calls |
      | RepoBench click control | 0 of 18 | 1.94 calls |

      The skill slice is a separate run, so this is observational support for the DM, not a
      randomised result.
    - DM value: 3.36 calls per dead-end session for pre-emptive vs 1.33 for post-hoc as logged
      (**+2.0**), or +1.27 vs post-hoc once bug 2 is fixed.
  - **Every time vs once.** After a followed post-hoc directive, 0 of 46 sessions went back to the
    failing form, so re-delivery has value 0. Once is optimal, and every repeat costs C.
  - **Terse vs explained.** Not identifiable at p_follow ≈ 1. The carry cost of any directive is
    C = $0.0002 (mean; p90 $0.0004): about 1% of the dead-end saving. Choose terse for context economy.
- **Generated repository (acme, secondary, not randomised within task).** Pre-emptive advice at
  UserPromptSubmit plus after-edit directives, against hooks off:

  | Metric | Change vs hooks off (task-clustered) | Hooks-off base |
  | --- | --- | --- |
  | calls | **−13.8 [−16.3, −11.3]** | 23.3 |
  | $ | −$0.047 [−0.056, −0.039] | $0.093 |
  | pass rate | +42 pts [+17, +67] | – |

  AGENTS.md does better still: −17.3 calls, the same pass rate.

## Model of p_follow to plug into A(s,d)

Replace p_follow in `A(s,d) = p_follow·ΔQ − C − λ·p_misfire` with the **uplift**:

```
A(s,d) = (p_follow(s,d) − p_self(s,d)) · ΔQ(s,d) − C(d) − λ·p_misfire(s,d)·m(s)
```

- **p_follow** is empirical-Bayes Beta per (kind @ hook), pooled to the global 0.958 with κ = 11.8:

  | kind @ hook | p_follow | Beta |
  | --- | ---: | --- |
  | avoid@PostToolUseFailure | 0.944 | (59.3, 3.5) |
  | avoid@UserPromptSubmit | 0.990 | (51.3, 0.5) |
  | run@PostToolBatch | 0.881 | (33.3, 4.5) |
  | run@UserPromptSubmit | 0.987 | – |
  | read-batch@PostToolBatch | 0.986 | – |
  | obligation / sequence@PostToolBatch | 0.975 | – |
  | stop@Stop | 0.92 | – |

  - Model and wording add nothing identifiable.
  - A PreToolUse rewrite has p_follow = 1 by construction.
- **p_self** has the pooled prior 0.50 with κ = 1.3:

  | kind @ hook | p_self |
  | --- | ---: |
  | avoid@PostToolUseFailure | 0.29 |
  | run@PostToolBatch | 0.32 |
  | read-batch | 0.93 |
  | run@UserPromptSubmit (lookup) | 0.95 |
  | obligation | 0.88 |

  - The runtime already logs `would_have` in the control arm. Scoring those with the learned judge
    estimates p_self per route for free.
- **Resulting uplift**:

  | Directive | Uplift |
  | --- | ---: |
  | avoid@PostToolUseFailure | 0.65 |
  | run@PostToolBatch | 0.56 |
  | obligation | 0.10 |
  | read-batch | 0.06 |
  | lookup | 0.04 |

- **Other terms.**
  - ΔQ for the dead end is 2.10 calls post-hoc and 3.36 pre-emptive.
  - C ≈ $0.0002.
  - The only misfire evidence is vue-core `pnpm test`, 2 of 2 bad; that is too small to fit λ·p_misfire.

## What to build (Rust)

1. **Replace the literal follow judge** (`pattern::meets`, used by `should_extinguish`) with learned
   weighted containment.
   - Token weights are log(1 + tasks containing the token) from the repository's own traces, stored
     with the learned pack. τ = 0.86.
   - It is O(tokens) at hook time with no model.
   - Expected value: stops U11 extinguishing, which is worth the whole 2.09 calls per trigger.
     Judgement error falls from 58% to about 0%.
2. **Rank and gate by uplift, not p_follow.**
   - Estimate p_self per route from the control/`would_have` units with the same judge.
   - Suppress routes whose posterior uplift is below 0.1: lookup, read-batch, regenerate/finish
     obligations and "how to run tests" advice, in these repositories. That is 79% of today's
     deliveries.
   - Expected value: about 0 calls, but less context and less misfire exposure.
   - **Built** (`adage_learn::value::uplift` and `assess`): the p_self and p_follow tables above are
     the kind priors (p_self κ = 4, the spread of the per-repository dead-end rates), updated by
     each route's unexposed training traces and the control arm's `would_have` judgements. A route
     below +0.1 with at least 3 situations of evidence is kept nascent.
3. **Deliver the valuable routes pre-emptively, once per session.**
   - Use a PreToolUse rewrite or deny-with-reason at the first matching attempt, or the SessionStart
     line, instead of PostToolUseFailure. This also covers piped exit-0 failures and Sonnet.
   - Expected value: **+2.0 calls and +$0.012 per dead-end session** over today (3.36 vs 1.33). That is
     about +1.8 calls per Haiku Python session (90% of them hit the dead end), taking the total to
     about 3.0 calls, or 9% of calls and 6% of $.
4. **Do not add every-time repetition or explanatory wording.** The data shows no value for either.

## So what

- **If shipped** (learned judge + uplift gating + pre-emptive once-per-session delivery), the expected
  effect on Haiku Python sessions is **−3.0 calls/session** (the DM lower bound is about −2.3) and
  **−$0.016/session**, i.e. **≈9% of calls and ≈6% of $**. The current system saves about 1.2 calls.
  The effect on pass rate is 0.
- **Confidence that it survives a live A/B.**
  - **High** for the local mechanism: the post-hoc effect is already randomised live, and pre-emptive
    first attempts were 8/8 on the same tasks.
  - **Low** that a session-level A/B at current sizes shows it: detecting 3 calls at sd about 15 needs
    roughly 300+ sessions per arm.
- **The delivery lever is exhausted at this size.** p_follow is already 0.96. To reach 15–20%, Adage
  needs directives with uplift in more situations: the acme result shows the ceiling rises with what
  the directives know. Better delivery of the same directive will not get there.

## Hard-coded parts (honest list)

- **Measurement instruments** (never shipped):
  - `judge.instrument_match`: shell segment peeling, `activate` → venv python, `-m module`, and file
    argument detection.
  - `analysis.gtest`: the G_test regex.
  - The `No module named|command not found` check on the learned hit detector.
- **Fixed thresholds that were not fitted:**
  - `TAU_INTENT` = 0.6 (same-intent containment, used for need detection and relapse detection);
  - `TAU_SAME_OUT` = 0.9 (potion output similarity).
- **Directive text parsing** (`units.directed_of`) reads Adage's own directive template: indented
  command lines, and "lists these files:". The wording features (`explains`, `must`) are regexes over
  the same templates.
- **The learned judge itself** uses a generic lexer (non-word characters and `/`, first line of the
  command) and corpus statistics. There are no tool names or command lists.

## Files

- `data.py`: loader for sessions, calls, delivered/withheld events and per-turn $.
- `units.py`: decision units.
- `judge.py`: the judges.
- `judges_eval.py`: leave-one-repo-out judge comparison → `results/judges.json`.
- `value.py`: learned goal and cost windows.
- `analysis.py`: uptake, gap, OPE and p_follow model → `results/analysis.json`.
- `extras.py`: acme, lags, C(d) and per-repo results → `results/extras.json`.
- **Run order:** `python3 data.py && python3 judges_eval.py && python3 analysis.py && python3 extras.py`.
  Caches are in `/tmp/adage-research/uptake-delivery`.
