# success-uplift: what makes the same task pass?

2026-09-29, offline, $0. This covers the "increase capabilities" half of the pitch. On the same task, what distinguishes sessions
that pass the hidden tests from sessions that fail, and would Adage raise pass rate by making that
behaviour happen?

## Verdict: negative. Behaviour reflexes are not a success lever here.

- **Pass/fail is mostly the task, not the trajectory.** 79% of the pass/fail variance lies
  between task × model × bench strata. Only 24 of 100 strata have mixed outcomes, covering 126 of 472 eval
  sessions. Learned behaviour features separate passing from failing attempts at the same task
  barely better than chance: within-task AUC is **0.57 [0.48, 0.66]** on held-out tasks, and
  **0.52 [0.41, 0.66]** without censored sessions.
- **The one learned lever that survives held-out tasks is an artefact.** It is "made any edit"
  (the learned Edit cluster): held-out RD +0.22 [+0.08, +0.40]. All 20 sessions with no edit
  are v1 sessions cut off at 30 turns before they edited. Without censored sessions, the CV-selected
  lever gives **−0.012 [−0.072, +0.041]** on held-out tasks.
- **The brief's candidate obligation reflexes are all within ±1 pass-rate point.** This is the
  uplift if Adage made X happen in every v2 control session where it did not, with p_follow = 1:

| obligation X (before finishing…) | P(¬X) v2 control | within-task RD | uplift, pp [90% CI] | sessions/arm to detect the CI upper bound (task-stratified) |
|---|---:|---:|---:|---:|
| H1 run tests after the last edit | 0.17 | +0.023 | **+0.40** [−0.95, +2.01] | 2,005 |
| H1b … and they end green | 0.17 | +0.028 | +0.48 [−0.86, +2.11] | 1,822 |
| H2b run the task's own test module after the last edit (gold instrument) | 0.30 | +0.035 | **+1.02** [−0.80, +3.28] | 754 |
| H3b run an ad-hoc repro script after the last edit | 0.46 | −0.039 | −1.80 [−4.90, +1.49] | – |
| H4 keep going after the first green test run | 0.12 | +0.031 | +0.38 [−0.25, +1.21] | 5,556 |
| H5b run the broad/full suite after the last edit (hand instrument) | 0.50 | +0.005 | +0.24 [−2.07, +2.49] | 1,306 |
| E0 make an edit before the turn cap (v1, 30 turns) | 0.09 (v1) | +0.368 | +3.20 [+0.66, +5.84] | 238 |
| E0 at 60 turns (v2) | 0.00 | – | 0 | – |

  Without censored sessions every RD is 0.00 to +0.06. "Reading the right file" cannot be a lever
  here: 100% of sessions read a gold source file, and 100% of uncensored sessions edit one. Failures
  are about *what* the fix is, not *where* it goes.
- **The randomised arms agree.** AGENTS.md raised "kept going after first green" by +9.8 pts
  [+2.3, +16.5] and moved pass by −2.2 [−9.7, +4.6]. The v2 adage arm shifted H3b by +8 and H4 by +5, and H2
  by −8 [−14, −3], with pass −2.0 [−8.8, +4.0]. Randomised 5–10-point shifts in these behaviours
  produced no pass change, which is consistent with RD ≈ 0.03.
- **Live arm effects on pass rate, task-stratified MH difference, 90% task bootstrap:**

| comparison | pooled | Haiku 4.5 | Sonnet 5 |
|---|---|---|---|
| v2 adage − control (98 vs 98) | **−0.020 [−0.087, +0.041]** | −0.024 [−0.087, +0.025] | 0.000 [−0.125, +0.125] |
| v1 agents-md − control (92 vs 92; v2 has no agents-md arm) | **−0.022 [−0.097, +0.052]** | 0.000 [−0.092, +0.092] | −0.125 [−0.250, 0.000] |
| v1 adage − control (92 vs 92) | −0.011 [−0.071, +0.044] | 0.000 [−0.066, +0.066] | −0.062 [−0.188, 0.000] |

Per repo × model rates are in `arms.json`, with Wilson 90% intervals per arm and Newcombe 90% intervals per difference. Example
v2 rows: attrs 0.79 vs 0.79; click 0.38 [0.23, 0.54] vs 0.42 [0.27, 0.58]; marshmallow 1.00 vs 1.00;
vue-core 0.31 vs 0.38; Sonnet click 0.50 vs 0.62, Sonnet vue-core 0.38 vs 0.25. Every per-slice difference
CI spans about ±0.2 to ±0.45.

**So what:** shipped, the best success reflex found (targeted or full tests after the last edit)
adds about **+0.4 to +1.0 pass-rate points** (90% CI about −1 to +3.3), which is about 1–1.6% lower $ per
resolved task. **Confidence it survives a live A/B: low (<20%).** A live test would need 750
(upper bound) to 35,000 (point estimate) sessions per arm to tell. The behaviour-only ceiling on
these tasks (every attempt as good as the best attempt at that task) is +10.2 points in v2
(0.612 → 0.714). The obligation reflexes reach about a tenth of it.

## Learnable per repo, or a cross-repo prior?

- **Per repo: no.** Mixed-outcome tasks per repo are attrs 3, click 9, marshmallow 0, urfave-cli 5 and
  vue-core 7. Per-repo 90% CIs on H1 are ±0.10 to ±0.25, about ten times the effect. No repository can
  learn a success lever from its own traces at 10–60 sessions.
- **Cross-repo prior: the only option, and it is ≈ 0.** Leave-one-repo-out is not evaluable: in 4 of 5
  held-out repos the selected lever has zero MH weight. If one prior is carried, it should be H2b
  ("run the test module that covers the edited file after the last edit"), whose *slot* (which
  test module) is learnable per repo from the co-change map. Its *value* is a weak cross-repo
  prior of about +1 pt.
- **Efficiency is what converts to pass rate, and only under a binding budget.** With a 30-turn cap, 9% of
  v1 sessions never reached an edit, and reaching one is worth +3.2 [+0.7, +5.8] points. At 60 turns this
  is 0. Other agents' call-saving levers therefore have a pass-rate dividend only where users run
  under tight turn or cost caps.

## Power for a live test (α 0.05 two-sided, power 0.8, p0 = 0.612 v2 control)

| Δ pass rate | sessions/arm, unstratified | sessions/arm, task-stratified (each task in both arms, σ²_within = 0.052) |
|---:|---:|---:|
| 2 pp | 9,221 | 2,031 |
| 3 pp | 4,075 | 903 |
| 5 pp | 1,448 | 325 |
| 10 pp | 348 | 82 |
| 15 pp | 147 | 37 |

The existing v2 design (82–98 per arm, task-paired) detects about 10 points. Pass rate should stay a
non-inferiority guardrail. It is not a target for reflexes of this kind.

## Method

- **Data** (`load.py`). There are 519 RepoBench v1 and v2 sessions (including the Sonnet 5 slices), de-duplicated by
  session id. v2 click and attrs runs that were discarded and rerun are dropped by matching `metrics.session_id`: the
  archive still holds their transcripts under the rerun's run id, which gives them the wrong
  outcome. The analysis uses 472 eval sessions in 100 task × model × bench strata with ≥ 2 sessions.
  Outcome is `hidden_passed`.
- **Learned action clusters** (`clusters.py`). Each call is a one-line rendering (`tool` +
  command, path or pattern). Drain (drain3 0.9.11, vendored from the sdist into
  `/tmp/adage-research/success-uplift/pkg`, because its wheel will not build here) learns templates
  from **training-split** sessions only. potion-base-8M embeds each template, and k-means (k = 12
  and 40) fitted on training templates gives clusters; every call goes to its nearest centroid. No outcomes are used.
  The clusters are sensible: pytest runs of any interpreter or flags form one cluster; npm test and vitest,
  go test, Edit, Read, git wrap-up, pip install, and ad-hoc `python -c`/heredoc scripts each form their own.
- **Behaviour features** (`levers.py`). Per cluster c: `any`, `tail` (in the last 5 calls), `endok`
  (last c exited 0), `again` (c again after its first exit-0), `many` and `early` (count and first position
  relative to the stratum median), and `late:c|d` (c after the last d, k = 12 pairs): 444 features.
  Success of a call is Claude Code's `is_error` exit flag.
- **Estimator.** The Mantel–Haenszel risk difference over task × model × bench strata controls task
  difficulty, model and the v1 30-turn cap. CIs are a task-cluster bootstrap at 90%.
- **Held-out tasks.** 5-fold GroupKFold over tasks × 4 seeds. Features are ranked on training folds by
  the one-sided 90% lower bound of RD (≥ 4 varying strata, top 5 kept), and scored on held-out
  tasks. The same selection runs leave-one-repo-out, with clusters refitted without the held-out repo's
  training sessions. `separability.py` fits a within-stratum-centred L2 logistic model and scores it
  leave-one-task-out with within-task AUC.
- **Obligation uplift** (`report.py`) is U = P(¬X) × RD_within × p_follow, with p_follow = 1
  (an upper bound; U11 measured 41/41). P(¬X) comes from v2 control. A joint task bootstrap covers both terms.
- **Randomised arms as instruments.** `report.py` reports the arm's shift in X and in pass rate.

## Weak held-out leads (not levers)

Without censored sessions, the features selected most often on held-out tasks were "git
status/log/diff used at all (early relative to stratum)", +6.2 [+2.1, +11.5], and "a go/npm test-or-find
call after the last Read", +7.4 [+1.4, +14.4]. The first is a wrap-up marker (`git diff`/`commit`
when the agent believes it is done). Forcing git status at the start is rare (6% in the first 15 calls)
and has no clean effect. The second mixes `find` into a test cluster. In both, the arrow most likely runs from
"finished confidently" to the behaviour, not the other way round.

## Hard-coded parts (all measurement, disclosed)

- The mapping of the brief's named hypotheses to learned clusters is a regex over cluster names
  (`pytest|go test|npm test|vitest`, `^Edit|^Write`, `python -c|heredoc|python {test_file}`).
- The "targeted test" and "gold file" features use each task's upstream `src_files` and `test_files`. These are oracle
  instruments.
- "Broad suite" (a test command with no file, `::`, `-k` or `-run` selector) is a hand heuristic.
- Cluster names use `normalize.py` templates, for display only.

## Files

`load.py` (data), `clusters.py` (Drain + potion + k-means), `levers.py` (features, CV, LORO,
prespecified hypotheses; `python3 levers.py [all|uncensored]` writes `levers_<pop>.json`),
`separability.py` (within-task AUC), `report.py` (obligation uplift, IV, per-repo, power writes
`report.json`), `arms.py` (live arm effects writes `arms.json`), `stats_util.py`. Caches are in
`/tmp/adage-research/success-uplift/` (15 MB). The whole pipeline runs in about 3 CPU-minutes.
