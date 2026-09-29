# GO validation: results

Run on 2026-09-29 against the shipped product: a release build of `main` at `acdb0a4`
(`adage 0.1.0`, Claude Code 2.1.285, `claude-haiku-4-5-20251001`). The run followed
`prereg.json` and `README.md` in order. The analysis is `analyze.py --out analysis.json`,
unchanged from the pre-registration.

## Verdict: **GO-WITH-CAVEATS**

One caveat: **G2's 90% CI includes 0 (underpowered).** Every other criterion holds.

The point estimate of −10.3% clears the pre-registered −10% threshold by only 0.3 points. A slightly
different sample could have given NO-GO, so this is a weak GO-WITH-CAVEATS. Read it as "consistent
with a saving of about 10% and no harm", not as a demonstrated saving. The prereg's own operating
characteristics give this design about 50% power at a true −20% effect, and P(GO or GO-WITH-CAVEATS)
of 0.65 there.

| criterion | result | holds |
| --- | --- | --- |
| G1: the product formed a card by itself | `card-ea0bf5cfc2`, formed by the background learner after training session 2, 4/4 traces after session 4 | yes |
| G2: primary ITT $/task ≤ −10% and 90% CI upper bound < 0 | **−10.3%**, 90% CI [−20.1%, +2.0%] | **no**: the point estimate qualifies, the CI does not |
| G3: net pass loss ≤ 3 of 16 | adage 16/16, control 15/16 (net **+1** for adage) | yes |
| G4a: card delivered live in ≥ 75% of adage sessions | 16/16 | yes |
| G4b: 0/8 negatives get a card in the probe | 0/8 (Wilson 90% upper bound 25%) | yes |
| G4c: 0 false fires on the regression slice | 0 (4 in-area deliveries, see below) | yes |
| G5: regression net pass loss ≤ 2 of 6 and $ ≤ +25% | net **+2** for adage; $ **+5.2%** | yes |

## Family `ufo-util` (unjs/ufo, 8 held-out variants × 2 repeats, paired)

**Primary (pre-registered, intention to treat).** $/task is adage/control − 1, the ratio of paired
sums, with a percentile bootstrap over the 8 variants (both repeats resampled together), 10,000
resamples, seed 20260929.

| | adage | control | adage vs control (90% CI, 8 clusters) |
| --- | ---: | ---: | --- |
| $/task (n = 16 each) | $0.1816 | $0.2026 | **−10.3%** [−20.1%, +2.0%] |
| tool calls / session | 24.3 | 26.5 | −8.3% [−19.2%, +5.3%] |
| turns / session | 25.3 | 27.5 | −8.0% [−18.5%, +5.1%] |
| passed (Wilson 90%) | 16/16 [0.86, 1.00] | 15/16 [0.76, 0.99] | +0.06 [0.00, +0.19] |
| $ per resolved task | $0.1816 | $0.2161 | |
| censored (60-turn cap) | 0 | 0 | |

- **Known limit.** With 8 clusters the percentile interval under-covers: the dry run measured
  85–86% coverage for a nominal 90%. Read it as roughly an 85% interval.
- **Per variant.** $ changes of −31% (dot-segments), −27% (without-auth), −25% (get-extension),
  −9% (query-param), −8% (sort-query), −2% (is-http), +7% (lowercase-host) and +36% (same-origin).
  The effect is heterogeneous and the interval is wide.
- **Interim (R4, after repeat 1, not a stopping point for efficacy).** −14.6% [−25.0%, −2.1%],
  8/8 against 8/8 passes, no harm stop. Repeat 2 pulled the estimate toward zero. The interim is
  reported only because it was computed; the verdict uses all 16 pairs, as pre-registered.
- **G2b (delivered subset).** The probe delivered the card for all 8 held-out prompts, so G2b
  equals the primary: −10.3% [−20.1%, +2.0%].
- **Sensitivity (censored pairs dropped).** No sessions were censored, so this is identical to the
  primary.
- **Checks.** The one failure is the control session `query-param` r2, on the test suite. Every
  other session in both arms passed lint, the README heading, automd regeneration and its own test.
  The prediction that "lint and automd are where controls fail" did not hold: pass rate is at the
  ceiling, and pass-rate effects cannot be seen here.

**Card delivery**

- **Probe after training ($0):** 8/8 held-out prompts: 5 at the prompt (without-auth, sort-query,
  lowercase-host, get-extension, dot-segments), 3 at the first edit of `src/utils.ts` (same-origin,
  query-param, is-http). 0/8 negatives.
- **Why more than predicted:** the prereg predicted 5/8, with none at the first edit. The
  grey-zone fast-path bug found in the dry run is fixed on `main`, so the card's file now fires at
  the first edit.
- **Live:** delivered in 16/16 adage sessions. It would have fired in 15/16 control sessions; the
  one exception is `query-param` r2, the failing control, where no card directive was logged.
- **Dead-end directives:** none delivered in the family.

## Regression slice (click, 6 one-off fixes × 1 repeat, paired)

Conditioned by `adage condition` over the 10 archived click transcripts, without outcomes. As in the
dry run, this formed one card, `card-5c06f78810`, from 4 shell-completion fixes.

| | adage | control | adage vs control |
| --- | ---: | ---: | --- |
| $/task | $0.2965 | $0.2820 | +5.2% [−10.1%, +21.6%] (6 clusters) |
| tool calls | 35.2 | 33.5 | +5.0% [−15.5%, +28.3%] |
| passed | 4/6 | 2/6 | +2 net for adage |
| censored (40-turn cap) | 3 | 2 | |
| card delivered / would-have | 2 / 2 | 0 / 2 | |
| dead-end directives delivered | 5 | 0 | |

- **Card deliveries:** only to `click-19fd4d6e` and `click-c3535905`, the two held-out fish
  shell-completion fixes whose real fix edits `src/click/shell_completion.py`. That is what the
  prereg predicted: in-area detections, **0 false fires**.
  - `click-c3535905`: adage passed and control failed.
  - `click-19fd4d6e`: both failed; the adage session hit the turn cap and cost 53% more than its
    control, the largest per-task increase on the slice.
- **Dead-end directives:** delivered in 5 of the 6 adage sessions (reported, not gated).
- **G5** is a gross-harm check only. Twelve sessions cannot detect small effects, and 5 of the 12
  hit the turn cap. The uncensored sensitivity analysis rests on 2 pairs and is not informative.

## Spend: $11.34 of the $13.50 cap

| | sessions | $ |
| --- | ---: | ---: |
| voided first training run (see below) | 4 | 0.86 |
| training | 4 | 0.87 |
| held out, repeat 1 | 16 | 3.08 |
| held out, repeat 2 | 16 | 3.07 |
| regression | 12 | 3.47 |
| **total** | 52 | **$11.34** |

- **Per-session means:** $0.19 (family) and $0.29 (regression). The prereg's central estimates were
  $0.20 and $0.30.
- **Cap:** the run enforced a $13.50 global cap in the configs, below the prereg's $15.

## Deviations and incidents (none changes the analysis plan)

1. **The first training run was void.** It cost $0.86 for 4 sessions, all passed, but the product
   learned nothing.
   - **Cause:** `runner.confine_argv` mounts `ADAGE_HOME` back into the bubblewrap sandbox only when
     the directory already exists. Before the first training session nothing had created it. The
     hooks and the background learner wrote into the tmpfs over `/tmp`, and their data vanished
     with each sandbox.
   - **Symptoms:** no index and no learn log; the learner was idle after every session.
   - **Fix:** `gv_run.py` now creates `ADAGE_HOME` before the training phase. The dry run did not
     catch this because it conditions from synthetic transcripts outside the sandbox.
   - **Handling:** that root is kept as `runs/ufo-util-void-train1` (`live/ufo-util/void-train1-state.json`)
     and counted in the spend ledger. Training was re-run in a fresh root. No held-out session had
     run.
   - **Product relevance:** this is a harness bug, not a product bug. A user's `~/.adage` is not
     hidden by a sandbox.
2. **The card reached training sessions 3 and 4** (`sessions_steered` 1, then 2). The product
   delivers its card as soon as it forms, which is the intended user flow. The prereg did not
   mention it.
3. **The global cap was lowered from $15 to $13.50**, the hard cap for this run.

## Files

| file | what |
| --- | --- |
| `analysis.json` | the full pre-registered analysis and verdict |
| `live/ufo-util/` | `state.json` (training and held-out sessions), `summary.json`, `gate.json`, `interim.json`, the learner log, learner waits, the index after training, the voided training state |
| `live/click-regression/` | `state.json`, `summary.json`, the conditioned index |
| `dryrun_shipped.json`, `dryrun_shipped_gate.json` | the $0 dry run of the same release build (8/8 held out, 0/8 negatives) |
