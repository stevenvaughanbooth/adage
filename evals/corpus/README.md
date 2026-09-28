# The first live corpus: real agent traces, and the reflexes learned from them

Adage's pitch is "agent traces become reflexes". This corpus is the first test of it with nothing
scripted: 354 real headless Claude Code sessions (`claude -p`, Claude Code 2.1.283) on twelve
sandbox runs (ten sandboxes, two of them also with a stronger model), 2026-09-28. Every trace came from
an agent working a task; every reflex below was learned from those traces by `adage learn`; every
effect was measured on held-out tasks the learner never saw.

**The short version.**

- Where training agents used a convention often enough, Adage learned it and it paid off, a lot: in
  the four Haiku sandboxes where a reflex was learned, held-out sessions with Adage needed **15.5 fewer
  turns** (median 12.5 vs 28; task-bootstrap 95% CI [−18.5, −9]; Holm p = 0.001, sign-flip over the 15
  tasks p = 0.0002), reached their first edit 12 turns sooner, and cost **48% less** ($0.063 vs $0.120 a
  session). With Sonnet 5 the same held: −3 turns, −5 turns to first edit, −24% cost (both Holm p < 0.05).
- Over all ten Haiku sandboxes, learned or not, Adage still cut median turns from 26 to 18 (CI [−13, −1.5],
  Holm p = 0.03) and cost by 20% (Holm p = 0.055): the four learned sandboxes carry that.
- Pass rates did not move measurably. Every pooled pass-rate difference is small and positive (+3 to
  +11 points) and none survives Holm; with Sonnet 5 the control already passed 92%.
- **In six of ten Haiku sandboxes Adage learned no reflex at all.** Haiku agents found the sandbox's
  lookup in only 1–3 of 9–12 training sessions there, below `--demo`'s 50% rate threshold. There the
  adage arm had only the instincts, and instincts alone had **no measurable effect** (turns −1, CI [−4, 3];
  pass +8 points, CI [−3, +18], Holm p = 1). The learner needs the behaviour to be common in the traces;
  a convention the agents rarely find is exactly the one it cannot learn.
- No learned reflex was false: 0 false routes and 0 decoy hits across the corpus. Four instinct routes in
  one sandbox were scored false by the ground truth (see below).
- Where Adage did not help or harmed: node-clinic-s23 passed 4/6 with Adage against 5/6 without; py-billing-hash-s32
  ran longer with Adage's instincts (median 30 vs 9.5 turns, though it passed 2 of 6 against 0 of 6);
  the finish-line obligation was met *less* often with Adage in the reflex sandboxes (1/8 vs 2/8; far
  too few to conclude anything, but no sign of help). All in the tables.

## How it was run

- **Harness.** `evals/live/runner.py`, isolated (fresh HOME and `CLAUDE_CONFIG_DIR` per session),
  `--host-auth` (only the host-managed login's variables forwarded: never `CLAUDECODE` or a session id),
  and the new `confine` mode: every session ran under bubblewrap, seeing only its own run directory, its
  arm's `ADAGE_HOME` and the hook binaries. Tool set pinned with `--tools` (see findings 1 and 2).
- **Train.** Plain agents with no hooks: 6–12 sessions per sandbox (training tasks x 2–3 repeats).
- **Learn.** `adage learn --demo` over each sandbox's training transcripts, in an ADAGE_HOME of its own,
  instincts bound. A second, day-0 home per sandbox: `adage learn --instincts-only` over a fresh sandbox.
- **Held out.** Fresh tasks never trained on, 2 repeats, arms randomised per (task, repeat) with a fixed
  seed: `holdout` (hooks on, directives logged, not delivered: the control), `adage` (reflexes and
  instincts delivered) and `instincts` (the day-0 index only). Where nothing was learned the adage index
  *is* the instincts, so only `holdout` and `adage` ran and that adage arm counts as instincts-only below.
- **Scored.** Hidden tests (`passed`, `hidden_passed`), plus the runner's metrics. Statistics per
  `evals/live/README.md#statistics`: the task is the unit; CIs resample tasks; p permutes arms within
  tasks; Holm across the metrics of each comparison; the sign-flip test asks whether the effect holds for
  new tasks. Pooled comparisons treat (sandbox, task) as the task.
- **Models.** Claude Haiku 4.5 (`claude-haiku-4-5-20251001`) for the ten sandboxes; a Claude Sonnet 5
  slice on acme-billing and harbor-dispatch (6 training sessions each, 36 held-out).
- **E1 prefetch** was tried and could not be delivered (finding 9); there is no E1 arm.

## Sandboxes, sessions, reflexes

"Conventions found" is `adage eval routes` against each sandbox's `conventions.json`: by the learned
reflexes alone, by the live index (reflexes + instincts), and by the instincts-only index. Gen
sandboxes' ground truth was converted to `adage-conventions/1` (their instinct-violation convention
counts as a dead end; decoys are checked separately). "Drafts" were learned but left awaiting review,
so they were never delivered.

| Sandbox | Model | Sessions (train + eval) | Spend | Train pass | Train used lookup | Reflexes learned | Drafts | Conventions found: reflexes / +instincts / instincts only | False routes (reflexes / instincts) | Decoy hits | Pass rate adage vs holdout (diff) | Turns adage vs holdout (diff) |
| --- | --- | ---: | ---: | ---: | ---: | --- | --- | --- | --- | ---: | --- | --- |
| acme-billing | Haiku 4.5 | 12 + 24 | $3.61 | 4/12 | 3/12 | none | – | 0/4 / 2/4 / 2/4 | 0 / 0 | 0 | 0.58 vs 0.42 (0.17) | 25.5 vs 22.0 (3.5) |
| harbor-dispatch | Haiku 4.5 | 12 + 36 | $5.30 | 9/12 | 9/12 | work-items/lookup-work-item, work-items/read-linked-files, finish-line/finish-line-04e5d4 | – | 3/4 / 4/4 / 2/4 | 0 / 0 | 0 | 1.00 vs 0.92 (0.08) | 12.0 vs 27.5 (-15.5) |
| py-billing-s11 | Haiku 4.5 | 9 + 12 | $2.01 | 3/9 | 3/9 | none | – | 0/5 / 2/5 / 2/5 | 0 / 0 | 0 | 0.17 vs 0.17 (0.00) | 25.5 vs 25.0 (0.5) |
| py-clinic-s12 | Haiku 4.5 | 9 + 18 | $2.30 | 7/9 | 8/9 | work-items/read-linked-files | work-items/lookup-work-item | 1/5 / 3/5 / 2/5 | 0 / 0 | 0 | 0.67 vs 0.67 (0.00) | 20.0 vs 29.5 (-9.5) |
| py-logistics-s13 | Haiku 4.5 | 9 + 12 | $2.34 | 6/9 | 3/9 | none | – | 0/4 / 2/4 / 2/4 | 0 / 4 | 0 | 0.83 vs 0.67 (0.17) | 26.5 vs 30.0 (-3.5) |
| node-logistics-s21 | Haiku 4.5 | 9 + 18 | $2.55 | 8/9 | 8/9 | work-items/lookup-work-item, work-items/read-linked-files | – | 2/5 / 4/5 / 2/5 | 0 / 0 | 0 | 1.00 vs 0.83 (0.17) | 11.5 vs 29.5 (-18.0) |
| node-billing-s22 | Haiku 4.5 | 9 + 12 | $2.42 | 1/9 | 2/9 | none | – | 0/5 / 2/5 / 2/5 | 0 / 0 | 0 | 0.67 vs 0.33 (0.33) | 24.5 vs 24.0 (0.5) |
| node-clinic-s23 | Haiku 4.5 | 9 + 18 | $1.87 | 5/9 | 6/9 | work-items/read-linked-files | work-items/lookup-work-item | 1/4 / 3/4 / 2/4 | 0 / 0 | 0 | 0.67 vs 0.83 (-0.17) | 16.5 vs 23.5 (-7.0) |
| py-billing-hash-s32 | Haiku 4.5 | 9 + 12 | $1.63 | 1/9 | 1/9 | none | – | 0/5 / 2/5 / 2/5 | 0 / 0 | 0 | 0.33 vs 0.00 (0.33) | 30.0 vs 9.5 (20.5) |
| node-billing-uuid-s42 | Haiku 4.5 | 9 + 12 | $0.82 | 1/9 | 1/9 | none | – | 0/5 / 2/5 / 2/5 | 0 / 0 | 0 | 0.00 vs 0.00 (0.00) | 1.0 vs 10.5 (-9.5) |
| acme-billing-sonnet5 | Sonnet 5 | 6 + 36 | $5.74 | 6/6 | 4/6 | work-items/lookup-work-item, work-items/read-linked-files | – | 2/4 / 4/4 / 2/4 | 0 / 0 | 0 | 1.00 vs 0.83 (0.17) | 13.0 vs 15.5 (-2.5) |
| harbor-dispatch-sonnet5 | Sonnet 5 | 6 + 36 | $7.69 | 6/6 | 6/6 | work-items/lookup-work-item, work-items/read-linked-files | – | 2/4 / 4/4 / 2/4 | 0 / 0 | 0 | 1.00 vs 1.00 (0.00) | 13.0 vs 19.0 (-6.0) |

Per-sandbox effect tables (with CIs) are in each dataset's README. Per sandbox there are only 3–6
held-out tasks, so single-sandbox CIs are wide; read the pooled tables.

## Effect tables (pooled over sandboxes, clustered by task)

Aggregates as the runner defines them (median for turns and times, mean for rates and cost).
`follow_rate` compares arms on routes they were exposed to (would-have directives); its arms see
different indexes, so read it as "did the agent do what the route says", not as an effect.

**Haiku 4.5, every sandbox: adage vs holdout** (10 sandboxes; 36 tasks; runs adage 72, holdout 72; left out as infra errors adage 0, holdout 0; hit max_turns adage 11, holdout 20)

| Metric | adage | holdout | diff | 95% CI (tasks) | p | p (Holm) | p (tasks, sign-flip) |
| --- | ---: | ---: | ---: | --- | ---: | ---: | ---: |
| pass_rate (avg) | 0.62 | 0.51 | 0.11 | [0.01, 0.21] | 0.152 | 0.760 | 0.073 |
| hidden_pass_rate (avg) | 0.62 | 0.51 | 0.11 | [0.01, 0.21] | 0.152 | 0.760 | 0.073 |
| total_turns (med) | 18.00 | 26.00 | -8.00 | [-13.00, -1.50] | 0.003 | 0.029 | 0.006 |
| turns_to_first_edit (med) | 11.00 | 15.50 | -4.50 | [-10.00, 1.00] | 0.003 | 0.029 | 0.004 |
| explore_calls_before_edit (med) | 9.00 | 13.50 | -4.50 | [-9.50, 0.00] | 0.005 | 0.038 | 0.010 |
| cost_usd (avg) | 0.09 | 0.11 | -0.02 | [-0.04, -0.00] | 0.009 | 0.055 | 0.019 |
| elapsed_s (med) | 48.75 | 62.58 | -13.82 | [-22.13, -4.96] | 0.003 | 0.029 | 0.005 |
| dead_end_hits (avg) | 0.17 | 0.21 | -0.04 | [-0.14, 0.04] | 0.595 | 1.000 | 0.560 |
| obligation_met_rate (avg) | 0.14 | 0.18 | -0.04 | [-0.38, 0.31] | 0.894 | 1.000 | 1.000 |
| follow_rate (avg) | 0.59 | 0.20 | 0.39 | [0.21, 0.44] | 0.000 | 0.001 | 0.000 |

**Haiku 4.5, sandboxes where a reflex was learned: adage vs holdout** (4 sandboxes; 15 tasks; runs adage 30, holdout 30; left out as infra errors adage 0, holdout 0; hit max_turns adage 0, holdout 7)

| Metric | adage | holdout | diff | 95% CI (tasks) | p | p (Holm) | p (tasks, sign-flip) |
| --- | ---: | ---: | ---: | --- | ---: | ---: | ---: |
| pass_rate (avg) | 0.87 | 0.83 | 0.03 | [-0.10, 0.17] | 1.000 | 1.000 | 1.000 |
| hidden_pass_rate (avg) | 0.87 | 0.83 | 0.03 | [-0.10, 0.17] | 1.000 | 1.000 | 1.000 |
| total_turns (med) | 12.50 | 28.00 | -15.50 | [-18.50, -9.00] | 0.000 | 0.001 | 0.000 |
| turns_to_first_edit (med) | 3.00 | 15.00 | -12.00 | [-13.50, -8.50] | 0.000 | 0.001 | 0.000 |
| explore_calls_before_edit (med) | 2.00 | 13.00 | -11.00 | [-13.00, -7.50] | 0.000 | 0.001 | 0.000 |
| cost_usd (avg) | 0.06 | 0.12 | -0.06 | [-0.07, -0.04] | 0.000 | 0.001 | 0.000 |
| elapsed_s (med) | 40.87 | 66.53 | -25.66 | [-34.32, -17.01] | 0.000 | 0.001 | 0.000 |
| dead_end_hits (avg) | 0.30 | 0.40 | -0.10 | [-0.30, 0.07] | 0.437 | 1.000 | 0.500 |
| obligation_met_rate (avg) | 0.12 | 0.25 | -0.12 | [-0.57, 0.25] | 0.673 | 1.000 | 0.750 |
| follow_rate (avg) | 0.48 | 0.16 | 0.32 | [0.19, 0.42] | 0.000 | 0.001 | 0.001 |

**Haiku 4.5, sandboxes where a reflex was learned: adage vs instincts-only** (4 sandboxes; 15 tasks; runs adage 30, instincts 30; left out as infra errors adage 0, instincts 0; hit max_turns adage 0, instincts 7)

| Metric | adage | instincts | diff | 95% CI (tasks) | p | p (Holm) | p (tasks, sign-flip) |
| --- | ---: | ---: | ---: | --- | ---: | ---: | ---: |
| pass_rate (avg) | 0.87 | 0.80 | 0.07 | [0.00, 0.17] | 0.713 | 1.000 | 0.500 |
| hidden_pass_rate (avg) | 0.87 | 0.80 | 0.07 | [0.00, 0.17] | 0.713 | 1.000 | 0.500 |
| total_turns (med) | 12.50 | 25.00 | -12.50 | [-15.50, -6.00] | 0.001 | 0.005 | 0.001 |
| turns_to_first_edit (med) | 3.00 | 13.50 | -10.50 | [-14.00, -6.00] | 0.000 | 0.004 | 0.003 |
| explore_calls_before_edit (med) | 2.00 | 11.00 | -9.00 | [-11.00, -5.00] | 0.000 | 0.004 | 0.003 |
| cost_usd (avg) | 0.06 | 0.10 | -0.03 | [-0.05, -0.02] | 0.001 | 0.005 | 0.001 |
| elapsed_s (med) | 40.87 | 61.00 | -20.13 | [-29.62, -12.12] | 0.001 | 0.008 | 0.001 |
| dead_end_hits (avg) | 0.30 | 0.50 | -0.20 | [-0.47, 0.07] | 0.187 | 0.933 | 0.250 |
| obligation_met_rate (avg) | 0.12 | 0.29 | -0.16 | [-0.38, 0.00] | 1.000 | 1.000 | 1.000 |
| follow_rate (avg) | 0.48 | 0.93 | -0.45 | [-0.39, -0.20] | 0.000 | 0.005 | 0.008 |

**Haiku 4.5, every sandbox: instincts-only (day 0) vs holdout** (10 sandboxes; 36 tasks; runs instincts 72, holdout 72; left out as infra errors instincts 0, holdout 0; hit max_turns instincts 18, holdout 20)

| Metric | instincts | holdout | diff | 95% CI (tasks) | p | p (Holm) | p (tasks, sign-flip) |
| --- | ---: | ---: | ---: | --- | ---: | ---: | ---: |
| pass_rate (avg) | 0.60 | 0.51 | 0.08 | [-0.03, 0.18] | 0.327 | 1.000 | 0.207 |
| hidden_pass_rate (avg) | 0.60 | 0.51 | 0.08 | [-0.03, 0.18] | 0.327 | 1.000 | 0.207 |
| total_turns (med) | 25.00 | 26.00 | -1.00 | [-4.00, 3.00] | 0.452 | 1.000 | 0.353 |
| turns_to_first_edit (med) | 16.00 | 15.50 | 0.50 | [-3.50, 3.00] | 0.332 | 1.000 | 0.228 |
| explore_calls_before_edit (med) | 12.00 | 13.50 | -1.50 | [-3.00, 1.50] | 0.450 | 1.000 | 0.356 |
| cost_usd (avg) | 0.10 | 0.11 | -0.01 | [-0.02, 0.01] | 0.355 | 1.000 | 0.342 |
| elapsed_s (med) | 59.52 | 62.58 | -3.05 | [-10.56, 3.71] | 0.216 | 1.000 | 0.149 |
| dead_end_hits (avg) | 0.25 | 0.21 | 0.04 | [-0.06, 0.15] | 0.640 | 1.000 | 0.612 |
| obligation_met_rate (avg) | 0.23 | 0.18 | 0.05 | [-0.33, 0.42] | 1.000 | 1.000 | 1.000 |
| follow_rate (avg) | 0.95 | 0.20 | 0.75 | [0.48, 0.78] | 0.000 | 0.001 | 0.002 |

**Sonnet 5 slice: adage vs holdout** (2 sandboxes; 12 tasks; runs adage 24, holdout 24; left out as infra errors adage 0, holdout 0; hit max_turns adage 0, holdout 0)

| Metric | adage | holdout | diff | 95% CI (tasks) | p | p (Holm) | p (tasks, sign-flip) |
| --- | ---: | ---: | ---: | --- | ---: | ---: | ---: |
| pass_rate (avg) | 1.00 | 0.92 | 0.08 | [0.00, 0.21] | 0.503 | 1.000 | 0.500 |
| hidden_pass_rate (avg) | 1.00 | 0.92 | 0.08 | [0.00, 0.21] | 0.503 | 1.000 | 0.500 |
| total_turns (med) | 13.00 | 16.00 | -3.00 | [-6.00, -1.50] | 0.007 | 0.046 | 0.001 |
| turns_to_first_edit (med) | 6.00 | 11.00 | -5.00 | [-6.50, -2.50] | 0.000 | 0.001 | 0.000 |
| explore_calls_before_edit (med) | 4.50 | 8.00 | -3.50 | [-6.00, -0.50] | 0.011 | 0.064 | 0.016 |
| cost_usd (avg) | 0.13 | 0.17 | -0.04 | [-0.05, -0.03] | 0.000 | 0.004 | 0.001 |
| elapsed_s (med) | 44.07 | 60.33 | -16.26 | [-29.66, -8.79] | 0.006 | 0.045 | 0.000 |
| dead_end_hits (avg) | 0.17 | 0.17 | 0.00 | [-0.21, 0.25] | 1.000 | 1.000 | 1.000 |
| obligation_met_rate (avg) | 0.75 | 0.57 | 0.18 | [-0.32, 0.75] | 1.000 | 1.000 | 1.000 |
| follow_rate (avg) | 0.56 | 0.14 | 0.42 | [0.31, 0.52] | 0.000 | 0.001 | 0.000 |

**Sonnet 5 slice: adage vs instincts-only** (2 sandboxes; 12 tasks; runs adage 24, instincts 24; left out as infra errors adage 0, instincts 0; hit max_turns adage 0, instincts 0)

| Metric | adage | instincts | diff | 95% CI (tasks) | p | p (Holm) | p (tasks, sign-flip) |
| --- | ---: | ---: | ---: | --- | ---: | ---: | ---: |
| pass_rate (avg) | 1.00 | 0.96 | 0.04 | [0.00, 0.12] | 1.000 | 1.000 | 1.000 |
| hidden_pass_rate (avg) | 1.00 | 0.96 | 0.04 | [0.00, 0.12] | 1.000 | 1.000 | 1.000 |
| total_turns (med) | 13.00 | 17.00 | -4.00 | [-6.00, -1.00] | 0.002 | 0.020 | 0.005 |
| turns_to_first_edit (med) | 6.00 | 10.00 | -4.00 | [-5.50, -1.50] | 0.000 | 0.002 | 0.002 |
| explore_calls_before_edit (med) | 4.50 | 7.00 | -2.50 | [-5.00, -0.50] | 0.004 | 0.028 | 0.018 |
| cost_usd (avg) | 0.13 | 0.18 | -0.04 | [-0.06, -0.02] | 0.000 | 0.002 | 0.002 |
| elapsed_s (med) | 44.07 | 55.73 | -11.66 | [-23.09, -5.30] | 0.001 | 0.013 | 0.002 |
| dead_end_hits (avg) | 0.17 | 0.12 | 0.04 | [-0.12, 0.25] | 1.000 | 1.000 | 1.000 |
| obligation_met_rate (avg) | 0.75 | 0.71 | 0.04 | [0.00, 0.17] | 1.000 | 1.000 | 1.000 |
| follow_rate (avg) | 0.56 | 0.88 | -0.32 | [-0.33, -0.25] | 0.169 | 1.000 | 0.500 |

**Sonnet 5 slice: instincts-only vs holdout** (2 sandboxes; 12 tasks; runs instincts 24, holdout 24; left out as infra errors instincts 0, holdout 0; hit max_turns instincts 0, holdout 0)

| Metric | instincts | holdout | diff | 95% CI (tasks) | p | p (Holm) | p (tasks, sign-flip) |
| --- | ---: | ---: | ---: | --- | ---: | ---: | ---: |
| pass_rate (avg) | 0.96 | 0.92 | 0.04 | [0.00, 0.12] | 1.000 | 1.000 | 1.000 |
| hidden_pass_rate (avg) | 0.96 | 0.92 | 0.04 | [0.00, 0.12] | 1.000 | 1.000 | 1.000 |
| total_turns (med) | 17.00 | 16.00 | 1.00 | [-3.00, 2.00] | 0.715 | 1.000 | 0.558 |
| turns_to_first_edit (med) | 10.00 | 11.00 | -1.00 | [-2.00, 2.00] | 1.000 | 1.000 | 1.000 |
| explore_calls_before_edit (med) | 7.00 | 8.00 | -1.00 | [-2.50, 3.50] | 0.963 | 1.000 | 0.958 |
| cost_usd (avg) | 0.18 | 0.17 | 0.00 | [-0.01, 0.02] | 0.803 | 1.000 | 0.727 |
| elapsed_s (med) | 55.73 | 60.33 | -4.60 | [-14.39, 8.01] | 0.496 | 1.000 | 0.476 |
| dead_end_hits (avg) | 0.12 | 0.17 | -0.04 | [-0.21, 0.12] | 1.000 | 1.000 | 1.000 |
| obligation_met_rate (avg) | 0.71 | 0.57 | 0.14 | [-0.38, 0.75] | 1.000 | 1.000 | 1.000 |
| follow_rate (avg) | 0.88 | 0.14 | 0.74 | [0.46, 0.54] | 0.057 | 0.688 | 0.500 |


## Spend

| Item | Sessions | USD |
| --- | ---: | ---: |
| Corpus sessions (12 runner roots, as each session reported `total_cost_usd`) | 354 | 38.27 |
| Discarded: first attempts before confinement and `--tools` (metered) | 35 | 3.08 |
| Discarded: killed mid-session (estimated from their token usage) | 27 | 0.39 (est.) |
| `adage-train` real run (7 sessions) | 7 | 0.23 |
| Auth and confinement probes | 3 | 0.03 |
| **Total** | **426** | **42.00** |

The cap was $60; the Sonnet 5 slice cost $13.43 of its $15.

## Datasets

`evals/datasets/live-20260928-<sandbox>/`, one per row above, in `build_demo.py`'s format (manifest,
curated sessions, `sandbox.txt`, recorded decisions and indexes, redacted paths), plus
`conventions.json`, the instincts-only index, and `packs/` with the learned reflex tracks signed by a
throwaway test signer (public key in each dataset README; never trust it). `adage eval <dataset> --split round1`
and `adage eval routes` run on each. Raw run roots (runs, homes, streams) are not committed.

## Problems found in Adage during real use

1. **Agents walk out of their sandbox.** Unconfined, Haiku sessions ran `find /tmp -name "*hidden*"`,
   read their own task's hidden test (`gen/.../hidden/test_hidden_bil_7793.py`) and read the runner's
   `state.json` (other sessions' prompts and commands), then "fixed" the ticket from it. The first 62
   sessions were discarded for this. Fixed here: `confine` (bubblewrap) in the runner.
2. **The harness's own tools derail the task.** Claude Code 2.1.283 exposes `TaskGet`/`TaskList`
   (and Cron*, ToolSearch, ...) even with `--allowedTools`; asked to "pick up W-1042", Haiku called
   `TaskGet W-1042`, found nothing, and asked the user what to do. Fixed: the runner passes `--tools`.
   `adage-train`'s ClaudeCode harness has the same exposure (not changed here).
3. **An agent that asks instead of acting was filed as an infrastructure error** ("no tool calls in 1
   turn"), which removed it from both arms: exactly the failure an ID lookup reflex should prevent. Now
   `ok` (a failed task) when the harness returned a successful reply (`demo/report.py`). This corpus
   re-files the older runs the same way.
4. **The instincts arm's decisions were read from the wrong ADAGE_HOME** (the runner always read
   `adage-home`'s spools). Fixed; this corpus re-read them.
5. **Rarely-used conventions cannot be learned.** `--demo` needs the behaviour in ≥50% of sessions
   (`yellow_rate`); Haiku used the lookup in 1–3 of 9–12 sessions in six sandboxes, and outcomes do not
   enter the threshold, although across all Haiku training sessions those that used the lookup passed
   40 of 44 and those that did not passed 5 of 52. Nothing was learned there.
6. **Effect classification by script name decides whether a reflex is ever delivered.** `python3 tools/case {item}`
   (wardline-clinic's lookup, 7/9 sessions, rate 0.78) was classified `environment` and left as a
   draft, as was node-clinic's `node tools/case {item}`; `python3 scripts/wi-lookup` and `node bin/fetch-incident` were `observe`
   and active. `adage learn`'s summary lists the draft as `lookup-work-item [yellow]` with no sign it is
   a draft (it is missing from the index).
7. **Packs drop the lookup reflex.** `adage share` pins repository scripts only when the repository it
   runs in has the track's key, but tracks learned with `adage learn --repo <name>` (as the live runner
   does) have a bare-name key no repository matches, so every lookup reflex was "Not carried: ... pass
   the repository root to pin it (L-ref)". There is no flag to pass it: the message points at nothing.
   Every pack in this corpus carries `read-linked-files` (and finish lines) but not the lookup.
8. **`adage learn --instincts-only` keys the repository `<name>@<root commit>`, `adage learn --repo <name>`
   keys it `<name>`**, so the two indexes of one sandbox disagree, and `adage eval routes` (which reads
   the dataset's repo key) scores an instincts-only index as empty unless it is re-keyed.
9. **E1 prefetch could not be delivered.** (a) `allow-exec --level e1` refuses a plain-text lookup ("no
   typed output"; the learned route is `node bin/fetch-incident {item}`, not `--json`), so it needs
   `--raw`, which policy must allow. (b) Then every prefetch was withheld at `sandbox`: as root the
   sandbox runs the command as `nobody`, and the repository sat under a mode-700 directory; nothing but
   the spool says so (`allow-exec` approved it; `exec status` shows a grant). (c) With a world-readable
   copy it was withheld at `render`, with no reason given.
10. **`build_demo.py` redacts the account name as a bare substring.** With HOME=/root it would rewrite
    every "root" in every transcript ("root commit", "rootDir") to "user". It also misses Claude Code's
    encoded project paths (`-tmp-...-scratchpad-runs-...`), which agents' `ls` output shows. Worked
    around in packaging (HOME set to an unused path; encoded roots redacted).
11. **`adage eval routes --index <file>`** prints "learned from N train sessions" and a LEAKY warning
    for the default split although it learned nothing and scores the given index.
12. **The runner rewrote the shared settings file for every session**, while other sessions were
    starting and reading it (now written once, atomically), and it has **no resume**: re-running a phase
    re-runs every session of it, so adding a repeat means re-running (and paying for) all of them.
13. **Gen bundles' `conventions.json` is not `adage-conventions/1`**, so `adage eval routes` cannot read
    it; there is no converter (this corpus converts them).
14. **harbor-dispatch needs Node ≥ 23.6**; the container has 22 (a Node 24 build was used, exposed read-only
    into the confined sessions).
15. **`adage-train` compiled nothing from a real run.** 7 real sessions (plan from the gen sandbox's
    `.ci/pipeline.yml`, $0.23): `make check` "was followed in 3/3 sessions without a failure to learn from;
    no miner compiles a plain verify step yet", so 0 routes were staged. `--host-auth` worked, under
    `script -qc`, with `CLAUDECODE` unset; its dry run listed forwarded variables twice (fixed).
16. **Instincts scored false on a narrow ground truth.** py-logistics-s13's four U3 dead ends
    (after `pytest`, `python -m pytest`, `python3 -m pytest`, `python -m unittest` fail, run `make check`)
    are scored false because the convention lists only `python3 -m unittest` as avoided; given the
    sandbox's env guard they are probably right. A ground-truth gap, but it shows instincts binding
    speculative dead ends for runners nobody ran.

## Reproducing

```bash
cargo build --release -p adage-cli -p adage-exec
python3 evals/gen/gen.py --suite smoke --out <gen>/smoke; python3 evals/gen/gen.py --suite idshapes --out <gen>/idshapes
python3 evals/gen/selfcheck.py <gen>/smoke        # and idshapes; harbor's selfcheck needs Node >= 23.6
# per sandbox: its config (the bundle's config.json, or evals/live/configs/*.json) with
#   "host_auth": true, "confine": true, "eval": {..., "repeats": 2, "arms": ["holdout", "adage", "instincts"]},
#   train repeats 3 (gen) or 2 (acme, harbor), seed 20260928
python3 evals/live/runner.py <config> train --execute --root <root>
python3 evals/live/runner.py <config> learn --execute --root <root>
python3 evals/live/runner.py <config> eval report --execute --root <root>
python3 evals/datasets/tools/build_demo.py <root> evals/datasets/live-20260928-<name> --name ... --repo ... --sandbox <fresh sandbox>
```
