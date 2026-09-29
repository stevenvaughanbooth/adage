# The common harness: score any policy in $ and calls per task

2026-09-29. The evaluation-methodology lead's deliverable for the research sprint (slug `harness`).
Every lever's claim is re-scored here in the adversarial round. It is Python, CPU-light (a warm
corpus loads in about 2 s, and U11 scores over 82 sessions in under a second) and offline ($0).

**Status: usable now.** The API below is stable. Numbers are in `results/*.json`, and
`python3 -m harness all` regenerates them.

## Quick start (copy this)

```python
import sys; sys.path.insert(0, "/home/user/adage/evals/lab/research")
from harness import load, score, Directive

corpus = load()                                  # 519 de-duplicated sessions, cached
test = corpus.held_out(model="haiku")            # held-out CONTROL sessions (no directive steered them)
train = corpus.train(repo="click")               # training-split sessions of one repository

def my_policy(sit):                              # called at every hook event of a held-out session
    if sit.event == "PostToolUse" and sit.failed and "No module named" in sit.call.output:
        return Directive("run", command=".venv/bin/python -m pytest", route="my-venv")
    return None

rep = score(my_policy, test)                     # replay + score
print(rep.headline())                            # $ / calls / pass-rate, with 90% CIs
rep.summary(); rep.by(lambda r: r.slice); rep.to_json()
```

Learners take training sessions and return a policy. The protocol drivers handle splits, leakage
and pooling:

```python
from harness.drivers import split, loro, curve, leave_task_out
fit = lambda train_sessions: MyPolicy(train_sessions)
split(fit, corpus, model="haiku").headline()     # train split → held-out control, per repository
loro(fit, corpus, model="haiku").headline()      # leave one repository out ("any repo")
curve(fit, corpus, sizes=(1, 2, 4, 10))          # {n: Report}: does more history help?
leave_task_out(fit, corpus)                      # grown memory in the same repository
```

Run Python as `OMP_NUM_THREADS=1 nice -n 10 python3 …` from `evals/lab/research`. The CLI is
`python3 -m harness corpus|u11|calibrate|leakage|parity|oracle|all`. The tests are
`python3 -m unittest discover -s harness/tests -t .`.

## What a result must report (the North star)

`rep.headline()` prints the target metrics first:

```
U11 post-hoc: 82 sessions / 30 tasks [test set …] (baseline $0.301, 36.0 calls per task)
  $ saved/task     +0.0058  [90% CI +0.0037, +0.0081]  = +1.9% of $ [+1.2, +2.9]
  calls saved/task +1.002  [90% CI +0.652, +1.385]  = +2.8% of calls
  pass rate        not estimable by replay; worst case -0.0 pts [-3.2, -0.0]
  proxies: Ŝ(Rust) +1.002 [95% +0.586, +1.464], 35 directives, useful 1.00 (LB90 0.93), harmful 0.00, …
```

- **$ per task** (primary). This is API-list dollars under the cumulative-context cost model
  described below, net of the directive's own carried tokens and of misfires. It is also given as
  a **share of the baseline $**. The North star asks for at least 15–20%.
- **Calls per task** (primary). This is tool calls, net, and also given as a share.
- **Pass rate**, which is only a non-inferiority check. Replay cannot see pass-rate gains. It
  reports a worst-case loss bound (every harmful directive, followed, flips a pass to a fail) with
  a Wilson CI. Pass rate is a weak live signal here: 79% of pass/fail variance is between tasks
  (success-uplift agent), and a +3-point effect needs about 900 sessions per arm. Claims of
  pass-rate *gains* need the live protocol below.
- **Proxies** (Ŝ in Rust's definition, useful rate, harm, directives per 10 calls, the §5.2 gates)
  are instruments. Ŝ(Rust) is S − C with no follow or misfire assumption, and it is identical to
  `adage eval replay-savings` (see Parity).
- CIs are **90%, task-clustered percentile bootstraps**: all sessions of a resampled task come
  with it, and the generator is the Rust replay's SplitMix64. The unit of the headline is a task
  attempt (one session).

## Reporting checklist for lever agents (what the adversarial round re-runs)

1. Score on `corpus.held_out(...)`. Say which filter (`model=`, `source=`) you used, and quote the
   `test_set_id` the headline prints. The data-expansion corpus joins `held_out()` once
   `data-expansion/corpus.json` exists, with its own slices (`click-dx` for RepoBench
   repositories), so pin `source=` if you need a stable set.
2. Report the headline's **$ per task and calls per task with 90% CIs, and the share of $**. Put
   `rep.to_json()` in your `result.json`.
3. For any directive kind other than a test-runner `run`, also report `p_follow=0.5`, and name
   the Sat/W class you relied on. `read_files`, `test_files`, `command` and `custom` are
   uncalibrated.
4. If you used `hindsight=True`, show a predictive variant next to it.
5. For learners, give `split(...)` and `loro(...)`, and `curve(...)` if you claim that more data
   helps. `scan_memory(...)` should find nothing in what the learner stores.
6. Compare against U11 (`harness.policies.u11.U11("post-hoc")`, and `"predictive"` for
   pre-emptive levers) and against the oracle ceiling of your class of calls.
7. End with the "so what" line: the expected $, calls and pass-rate effect if shipped, its CI, and
   the micro-slice that would confirm it.

## Data (`harness.traces`)

`load()` discovers every finished run in the RepoBench v2 (`/tmp/adage-rb2/runs`) and v1
(`/tmp/adage-repobench/runs`) runner roots, plus the data-expansion corpus manifest
(`evals/lab/research/data-expansion/corpus.json`) when it exists. It parses each transcript as
`crates/adage-eval/src/transcript.rs` does and caches the result as a pickle in
`/tmp/adage-research/harness/cache` (about 40 MB). The cache key covers the files' size and
mtime, so it rebuilds by itself.

- **One session per run, the one `state.json` names.** A run that was discarded and rerun
  (v2 click/attrs `dropped_contaminated`, retries) leaves its transcript beside the rerun's. Only
  the transcript whose `sessionId` equals the run's `metrics.session_id` is loaded; the others go
  to `corpus.excluded` (35). A directory's sole transcript is used as a fallback only when the
  state names no session id. The older lab loader (`evals/lab/traces.py`) loads every transcript
  in the directory and gives discarded sessions the rerun's outcome. Don't reuse it for outcome
  work. This is unit-tested.
- **De-duplicated by transcript `sessionId`.** v2 is loaded first, so the v2 copy wins. There
  are 57 copies (37 v1→v2, 20 within v2, which are the Sonnet slices' copies of the training
  traces). All 57 have the same outcome and task (`corpus.duplicates`).
- **Splits** come from `evals/repobench/splits` (`train`/`buffer`/`test` tasks, chronological).
  - `corpus.held_out()` returns eval-phase sessions on test tasks in the control arm.
  - `corpus.train()` returns training-phase sessions on train tasks.
  - `corpus.select(...)` filters on anything else: `arm="adage"`, `source="v1"` and so on.
- **What a `Session` has:**
  - `calls` (tool, input, output as the model saw it, clipped to 6k characters; `is_error`) and
    `items` (prompts and model turns in order);
  - `acts`: the Rust replay's per-call features, computed on the full output;
  - `responses`: each API response's usage (cache read, write and 1h-write, input and output,
    and the calls it issued);
  - `passed` (hidden tests), `cost_usd` (the harness's billed total), `model`, `slice`, `task`,
    `split` and `arm`;
  - `delivered_at`: where the live hook delivered each directive, in adage-arm sessions.

| slice | train | held-out control | adage | agents-md |
| --- | ---: | ---: | ---: | ---: |
| attrs (v1+v2, Haiku) | 10 | 40 | 40 | 16 |
| click (v1+v2, Haiku) | 10 | 40 | 40 | 16 |
| marshmallow (v1+v2, Haiku) | 7 | 30 | 30 | 12 |
| vue-core (v1+v2, Haiku) | 10 | 32 | 32 | 16 |
| urfave-cli (v1, Haiku) | 10 | 16 | 16 | 16 |
| click-sonnet5, vue-core-sonnet5 | (the base repo's) | 16 each | 16 each | 8 each |

v1 sessions were capped at 30 turns and v2 sessions at 60. Filter with `source="v2"` to compare
with the Rust baseline table (82 Haiku sessions).

## Policy interface (`harness.replay`)

`policy(sit: Situation) -> Directive | None`. It is called once per hook event of each held-out
session, in hook order, and may give at most one directive per event:

| event | `sit.t` | what the policy sees |
| --- | --- | --- |
| `SessionStart` | −1 | `sit.session` (repo, cwd, model, first prompt) |
| `UserPromptSubmit` | last call before it | `sit.prompt` |
| `PreToolUse` | k, the call about to run | `sit.call` with its **input only** (`output == ""`) |
| `PostToolUse` | k | `sit.call` with its output; `sit.failed` means Claude Code raised `PostToolUseFailure` (`is_error`, not rejected) |
| `PostToolBatch` | last call of the model turn | Adage's batch hook |
| `Stop` | last call | only for sessions that ended with a Stop hook |

- `sit.history` holds the calls before the event, and `sit.state` is a per-session dict the
  policy may keep.
- Set `policy.events = {...}` to skip events you don't use (it's faster).
- A directive given at `PreToolUse` is **pre-emptive**: it can replace call t, and if call t
  would have failed and isn't in Sat(d), it counts as one saved call.
- **Hindsight is opt-in and flagged.** `score(..., hindsight=True)` exposes
  `sit.hindsight = {"will_fail", "output"}` at `PreToolUse`. That is what the Rust
  `--mode pre-emptive` does. It is an upper bound, not a policy, and the headline carries a
  HINDSIGHT flag.

`Directive(kind, text="", command="", files=(), route="", sat=None, waste=None, contradicts=None, p_follow=None)`.
The kind decides how it is scored:

| kind | Sat(d): the agent did what d says | W(d): the waste d removes | calibrated? |
| --- | --- | --- | --- |
| `run` + test-runner command | a run of d's runner that executed tests, in any environment (goal class) | environment friction: env-signature outputs, env probes/repairs, failed runner calls that ran nothing | **yes** (live U11) |
| `run` + other command | a later call starting with its first two words that did not fail | failed calls | no |
| `test_files` | a test run naming one of `files` that executed tests | test-discovery searches | no |
| `read_files` | a Read/Edit of one of `files` | searches and listings (Grep, Glob, find, grep, ls, rg) | no |
| `advice` | never | none: costed, never credited | – |
| `custom` | your `sat(act, call)` | your `waste(act, call)` | **flagged** |

The Sat/W features (`harness.features`) are a line-for-line port of the Rust replay's
**hand-written** taxonomy. They are a *measurement instrument*. A learner that imports them to
decide what to say is hard-coded under principle 1. Custom classes are counted in the headline
so that a reviewer checks them.

## Metrics (`score()`)

For each directive d given at t (Rust semantics, `evals/replay/README.md`):

- **t\*** is the first t′ > t with a_t′ ∈ Sat(d). d is **useful** if t\* exists.
- **S(d)** = |{k ∈ (t, t\*) : a_k ∈ W(d)}| + 𝟙[pre ∧ a_t ∉ Sat(d) ∧ a_t failed].
- d is **superseded** when it falls inside an earlier useful directive's window. It is then
  neither scored nor costed.
- d is **harmful** when it is not useful, the session passed, a contradicting action happened
  after t, and d's own action never happened.
- **C(d)** is Rust's cost in calls.

Headline per session, with assumptions `p_follow` (default 1.0, as live U11 was followed 41/41)
and `misfire_calls` (default 2, the learner's prior):

- **calls** = Σ p_follow·S − C − p_follow·𝟙[scored ∧ ¬useful]·misfire_calls
- **$** = Σ p_follow·$removal(W calls of the window) − $injection(d) − p_follow·𝟙[misfire]·misfire_calls·$/call
- **tokens** is the same as $ but in context tokens processed.

For any directive kind that no live run has measured, **report p_follow = 0.5 as a
sensitivity** (`score(..., p_follow=0.5)`). A directive's own `p_follow` overrides the default.

### The cumulative-context cost model (`harness.costmodel`)

- **What a response costs.** Every API response j reads the whole context. Its bill is
  `cache_read·r_read + cache_write·r_write + input·r_in + output·r_out`, taken from its own usage
  record and priced by `crates/adage-core/src/pricing.toml`.
- **What a removed call saves.** The context grows between responses by g_j. It is split among
  response j's calls by their input and output size, then written once and read by every later
  response. So removing call k saves Δ_k·(r_write + r_read·L_j), where L_j is the number of later
  readers. If k was the response's only call, the response's own read and output are saved too.
- **What an injected directive costs.** A directive of τ tokens after call t costs
  τ·(r_write + r_read·(responses after t − 1)).
- **Cache-write rate.** Claude Code in these runs writes the **1-hour cache (2× input)**, not the
  5-minute rate (1.25×) that `pricing.toml` assumes. The model reads the TTL split from the usage
  record.

Validated (`calibrate.cost_model_check()`):

- Its session total equals the harness's billed `total_cost_usd` on all 519 sessions (ratio
  1.000, p5 and p95 1.000). With `pricing.toml`'s 5-minute write rate it would be 0.877, a 12%
  under-price that affects the Rust ledger too.
- Its average $ for removing one call ($0.0104) matches the within-task cross-sectional slope of
  billed $ on calls ($0.0107).

## Parity with `adage eval replay-savings`

The Python U11 (`harness/policies/u11.py`, routes read from the live bound index) reproduces the
Rust replay **exactly**: every directive's (t, pre-emptive, S, C) in every session, and every Ŝ
with its 95% CI, on all six v2 slices in both modes (`results/parity.json`). Examples:

- click post-hoc: +0.4904 [0.2046, 0.7806]
- attrs pre-emptive: +3.3078 [2.1056, 4.4259]
- click-sonnet5 pre-emptive: +1.3626

## Example policy: U11 (the hand-written baseline to beat)

Rows use the 82 Haiku v2 held-out control sessions, 30 tasks. Full rows, including v1+v2 and all
held-out sessions, are in `results/u11.json`.

| policy (82 Haiku v2 held-out control sessions, 30 tasks; baseline $0.301 and 36.0 calls per task) | $ saved/task [90% CI] | share of $ [90% CI] | calls saved/task [90% CI] | Ŝ Rust [95% CI] | directives | useful LB90 |
| --- | --- | --- | --- | --- | ---: | ---: |
| U11 post-hoc (as live) | +$0.0058 [0.0037, 0.0081] | +1.9% [1.2, 2.9] | +1.00 [0.65, 1.39] | +1.00 [0.59, 1.46] | 35 | 0.93 |
| U11 pre-emptive, **hindsight** (Rust `--mode pre-emptive`) | +$0.0118 [0.0087, 0.0149] | +3.9% [2.7, 5.4] | +2.08 [1.55, 2.60] | +2.11 [1.49, 2.71] | 58 | 0.93 |
| U11 pre-emptive, **predictive** (no hindsight) | +$0.0116 [0.0084, 0.0148] | +3.9% [2.6, 5.3] | +2.03 [1.48, 2.55] | +2.06 [1.43, 2.69] | 58 | 0.93 |
| … predictive at p_follow = 0.5 | +$0.0057 [0.0041, 0.0072] | +1.9% [1.3, 2.6] | +1.01 [0.74, 1.26] | | | |

The predictive variant only knows which runner spellings failed with a U11 signature in the
repository's training sessions.

- **These reproduce the known numbers.** Post-hoc is +1.00 calls per session and the Rust
  pre-emptive mode is +2.11.
- **Hindsight costs almost nothing here.** A `PreToolUse` trigger that knows only what failed in
  training recovers 98% of the hindsight value.
- **The drivers, run with the predictive learner** over all Haiku held-out sessions (v1+v2, 158
  sessions, 38 tasks):
  - split: +1.63 calls [1.22, 2.04], +3.5% of $;
  - **leave one repository out: +0.88 calls [0.58, 1.19], +1.9% of $**. The learned spellings
    don't transfer, so it falls back to post-hoc;
  - learning curve for n = 0, 1, 2, 4 and 10 training sessions: 0.88, 1.38, 1.38, 1.63 and 1.63
    calls.
- **The ceiling is small.** At its best U11 saves about 2 calls, or about 4% of $ per task. That
  is the "5%" the North star says to beat by 3–4×.

### Oracle ceilings (what vanishing a whole class of calls would be worth)

`harness.oracle.ceiling(sessions, predicate)` deletes every call the predicate marks. There is no
directive cost, follow is perfect, and there is no compensation. Rows use the 82 Haiku v2
held-out sessions (`results/oracle.json`).

| class of calls deleted | calls/task | share of calls | $/task | share of $ [90% CI] |
| --- | ---: | ---: | ---: | --- |
| every failed call (is_error or environment signature) | 3.55 | 9.9% | $0.031 | 10.3% [8.7, 12.0] |
| environment friction (U11's W) | 3.83 | 10.6% | $0.025 | 8.4% [6.6, 10.4] |
| test-discovery searches | 2.68 | 7.4% | $0.021 | 6.9% [5.7, 8.2] |
| failed test-runner calls | 1.60 | 4.4% | $0.010 | 3.4% [2.7, 4.2] |
| every search or listing (Grep, Glob, find, grep, ls) | 6.32 | 17.5% | $0.047 | 15.5% [13.6, 17.4] |

**So what.** No lever that works by "skip a kind of wasted call" can reach the North star's
15–20% of $. Even deleting *every* failed call is worth 10%, and deleting every search is worth
15.5%, with perfect follow and zero harm. A 15–20% lever has to either shorten the productive
path itself (fewer reads, edits and test runs: knowledge that changes what the agent does, not
what it avoids) or change the cost structure (smaller carried context: output trimming, early
compaction). This is because $ is dominated by context carried across about 40 responses, not by
the calls themselves. Levers should be framed and measured against these ceilings.

## Calibration: does the harness predict live effects?

Everything is in `results/calibration.json` (`python3 -m harness calibrate`).

1. **The local mechanism is calibrated.**
   - On the 21 tasks where the live A/B measured U11's dead-end recovery, the replay predicts
     **1.40 calls [0.87, 1.98]**. Live was **1.85 [1.17, 2.56]**, so the prediction is inside the
     CI.
   - Per repository: click 0.54 vs 0.46, attrs 2.33 vs 2.88, marshmallow 1.44 vs 2.50. Replay is
     slightly conservative.
   - The pre-emptive variants predict 2.9, which is outside the live CI, as they should: they were
     never shipped.
2. **Delivery matches exactly.** In the 66 v2 Python adage-arm sessions, the Python U11 fires at
   the same route and call as all **41 of 41** live deliveries, and nowhere else.
3. **Residual.** Replaying U11 over the adage arm, where it was delivered and followed, leaves
   +0.06 calls [0.00, 0.12] to save.
4. **Whole-session live deltas cannot see effects of this size, and one looks contradictory.**
   - Pooled v2 Haiku Python: live adage − control is **+1.55 calls [+0.15, +2.89]** and
     +$0.006 [−0.003, +0.015] per session. The replay predicts −1.25 calls and −$0.007, which is
     outside the live 90% CI.
   - The difference isn't the directive's doing. Adage-arm sessions that received **no**
     directive also ran +1.85 calls longer than control on the same tasks. It is an arm-level
     shift or noise, with a per-session SD of about 6 calls.
   - A difference-in-differences removes that shift: (delivered − would-fire) − (not delivered −
     would not). It gives −0.20 calls [−6.4, +5.3] against a predicted −2.35 per firing session.
     That is consistent, and uninformative.
   - **v1 is an A/A test.** U11 wasn't bound in v1, and only 2 advisory directives were
     delivered. Of 18 per-slice 90% CIs, 3 exclude 0 (17%, against a nominal 10%). Task-paired
     percentile bootstraps over 6–8 tasks under-cover. Read single-slice live CIs as about 80%.
5. **Conclusion.**
   - The harness is calibrated for the **local** effect of a runner-class directive: calls from
     the trigger to the goal.
   - It is **not** validated for whole-session net effects. No live run has had the power to
     test that: the whole-session MDE is about 3 calls per session at n=66 pairs.
   - It is not validated for any other directive class. That is why levers must pass the live
     micro-slice below before a number is believed.

## Leakage (`harness.leakage`)

- `assert_clean(train, test)` runs inside every driver. It raises if a training session is a
  held-out session (by id), is on a held-out task, or is a content copy of one (a hash of the
  tool-call sequence with paths made relative).
- `scan_memory(texts, test, train)` finds held-out task statements in whatever strings your
  learner stores: 8-word shingles of held-out prompts that no training session contains. Run it
  on your memory before you report.
- `audit(corpus)` gives the corpus-level checks (`results/leakage.json`), and all of them are
  clean:
  - train/buffer/test tasks are disjoint in every repository;
  - no train-phase session is off a train task, and no eval-phase session is off a test task;
  - the 57 v1/v2 copies all agree on outcome and task;
  - no content copies remain under distinct ids;
  - no held-out task or prompt appears in training;
  - the 35 discarded or retried transcripts are excluded.

## Methodological critique: what counterfactual replay can't see

The replay deletes the calls in W(d) between t and t\*, keeps everything else the agent did, and
assumes the agent follows. Its biases, with their direction:

1. **Compensating behaviour (sign unknown, possibly large).** The counterfactual agent is not the
   factual agent minus some calls.
   - Freed of a dead end, it may explore more, re-run tests more often (it now can), or finish
     earlier. A directive may also prompt extra verification.
   - None of this is visible to replay. The live whole-session delta that would reveal it is
     buried in about 6 calls of per-session noise (check 4).
2. **Counting only W(d) (conservative).** Reads and edits inside the window aren't credited,
   though the live agent skipped some of them. This is why replay predicts 1.40 against a live
   1.85.
3. **p_follow = 1 (optimistic outside U11).** Follow was 41/41 for a verified command after a
   visible failure. Advice, file lists and pre-emptive rewrites have no measured follow rate.
   Sonnet pipes where Haiku doesn't.
4. **Sat is a hand-written class (large definitional variance).** U11's prediction moved 8× (0.16
   → 1.40) when Sat changed from strict to goal. Every uncalibrated class (`read_files`,
   `test_files`, `command`, `custom`) may be off by a similar factor, in either direction.
5. **Harm is undercounted (optimistic).**
   - Harm is scored only in *passed* sessions with a contradicting action.
   - A misleading directive in a failed session, or one that *causes* the failure, is invisible,
     because the trace never shows the counterfactual failure.
   - The misfire charge (2 calls when Sat never comes) is a prior, not a measurement.
6. **No pass-rate effect (blind).** Replay holds the outcome fixed, so success levers score zero
   on it by construction.
7. **Hindsight (optimistic).** Pre-emptive scoring may look at the future output. It is flagged
   here, and for U11 a predictive trigger recovers 98% of it, but other levers must show a
   predictive variant.
8. **Winner's curse across about 15 agents (optimistic).**
   - Everyone tunes on the same 82–220 held-out sessions and 30–38 tasks, and the best lever is
     selected on the same data.
   - With 90% CIs that under-cover (check 4), expect about one spurious "significant" lever in 15
     by chance.
   - Mitigation: the adversarial re-score uses a fixed protocol, pre-registered thresholds and
     (when it exists) the data-expansion corpus as a sealed test set.
9. **Distribution (unknown).** These are bug-fix tasks from 5 repositories, 7–10 training
   sessions each, mostly Haiku 4.5. Leave-one-repo-out is the only "any repo" evidence.

## The correction: a live micro-slice per lever (≤ $6 each)

Replay ranks levers. A short, paired live run validates the top few before anyone builds in Rust.
It measures what replay assumes: p_follow, the local effect, and compensation.

1. **Pick the slice from the replay.**
   - Choose the held-out tasks where the lever fired most often in replay, from its
     `rep.rows[*].scored`. Take up to 5 tasks, one repository first.
   - Pre-register: the replay's predicted local saving per firing session with its CI, and the
     predicted $ per task.
2. **Paired runs.**
   - Run 2 repeats × (lever on, control) per task, which is 10 pairs and 20 sessions. Haiku 4.5
     costs about $0.27 a session (v2 mean), so that is about **$5.4**. The spend goes through
     the data-expansion agent's runner (`evals/repobench/rerun.py` configs, `max_turns` 60).
   - Control is hooks-on with directives withheld, as in RepoBench, so any hook overhead is
     identical.
3. **Measure the local outcome, not the whole session.** From each trigger to its Sat, the local
   outcome has an SD of about 2 calls per pair, where the whole session has about 6.
   - **p_follow**: the share of delivered directives followed by a Sat action within 5 calls.
   - **Local saving**: control-arm calls from the matched trigger to Sat, minus lever-arm calls.
     Match the trigger with the same `policy()` over the control transcript.
   - **Compensation**: whole-session calls and $, lever − control, reported but not gated on.
     These accumulate across micro-slices into a pooled estimate of the replay's whole-session
     bias.
4. **Decide.**
   - Pass if p_follow ≥ 0.6 **and** the local saving's one-sided 90% interval excludes 0 **and**
     it is ≥ 50% of the replay's prediction.
   - The replay's $/task estimate is then scaled by the measured p_follow and by (live local ÷
     replay local), and that becomes the number carried into the build decision.
   - With 10 pairs, the minimum detectable local effect is **1.6 calls** at 80% power
     (`calibrate.power()`). A lever predicting less than that needs 20 pairs ($11) or is not worth
     building.
   - **Stop early**: if the first 3 delivered directives are all ignored, stop and report
     p_follow ≈ 0.
5. **Pass rate** is never gated on a micro-slice (it needs hundreds of sessions). Micro-slices
   check it only for non-inferiority, with no drop of more than one task in five.

## Files

| file | what |
| --- | --- |
| `traces.py` | loader, splits, de-duplication, cache |
| `features.py` | the Rust replay's hand-written per-call features (instrument) |
| `replay.py` | `Situation`, `Directive`, events, `score()`, `Report` |
| `costmodel.py` | the cumulative-context cost model |
| `stats.py` | cluster bootstrap (SplitMix64, as in Rust), Wilson, paired task differences |
| `drivers.py` | `split`, `loro`, `curve`, `leave_task_out` |
| `leakage.py` | `assert_clean`, `scan_memory`, `audit` |
| `calibrate.py` | mechanism, arm deltas, A/A coverage, DiD, residual, delivery match, cost model, power |
| `oracle.py` | ceilings: delete a class of calls, report $ and calls |
| `policies/u11.py` | the example (baseline) policy |
| `tests/` | unit tests: synthetic transcripts of known S, cost model, discarded runs, leakage, parity |
| `results/` | `u11.json`, `calibration.json`, `leakage.json`, `parity.json`, `oracle.json` |

## Verdict and what to build

- **The harness is ready.**
  - It is exact against `adage eval replay-savings` (every directive in every session).
  - It is calibrated on the one live local measurement (1.40 against 1.85, inside the CI).
  - Its $ model reproduces the billed cost of all 519 sessions.
  - The corpus is clean.
- **It is not validated for whole-session effects.** No live run has had the power to test that,
  and one live contrast (the Python arm shift) points the other way, but for reasons unrelated to
  the directive. Treat a replay number as the lever's *local* effect, and confirm it with the
  micro-slice protocol before building.
- **What to build in Rust** (someone else's crate: this sprint doesn't touch `crates/`):
  1. **`pricing.toml` / the ledger should price 1-hour cache writes at 2× input.** Claude Code
     writes them, and the transcript's `usage.cache_creation.ephemeral_1h_input_tokens` says so.
     Today every Adage $ figure is about 12% low. Expected value: correct dollars, and no
     behaviour change.
  2. **`adage eval replay-savings` should report $ under the cumulative-context model** (this
     package's `costmodel.py`, about 60 lines). A call's price depends on its position: early
     waste is carried for the rest of the session. Today's C(d) and `net_usd` use a flat $/call.
  3. **U11 as a predictive `PreToolUse` rewrite** (already the top recommendation). Here it keeps
     98% of the hindsight value without hindsight: +2.03 calls and +3.9% of $ per task, against
     +1.00 and +1.9% post-hoc. At p_follow 0.5 it is +1.9%. Its predicted live local effect is
     about 2.9 calls per dead-end session.
- **For the sprint:** a lever that works by skipping a class of wasted calls has an oracle
  ceiling of about 10–15% of $. Levers claiming more must change the productive path or the
  carried context, and should say which.

