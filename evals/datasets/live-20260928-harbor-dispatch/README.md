# live-20260928-harbor-dispatch: 48 real Claude Code sessions

Real headless Claude Code sessions (`claude -p`, Claude Code 2.1.283, model `claude-haiku-4-5-20251001`) on the `harbor-dispatch` sandbox, run on 2026-09-28 by `evals/live/runner.py` with `--host-auth` and `confine` (each session under bubblewrap, seeing only its own run directory). Part of the corpus in [evals/corpus](../../corpus/README.md). Every session ran in a fresh sandbox; hidden tests scored it afterwards.

| Phase | Arm | Sessions | Notes |
| --- | --- | ---: | --- |
| train | no hooks | 12 | 6 training tasks x 2 |
| eval | holdout | 12 | 6 held-out tasks x 2; control: hooks on, directives computed and logged, not delivered |
| eval | adage | 12 | 6 held-out tasks x 2; learned reflexes and instincts delivered |
| eval | instincts | 12 | 6 held-out tasks x 2; day-0 index: instincts bound from the repository's files, no learned reflex |

Training: 9/12 passed; 9/12 used the sandbox's lookup. Spend: $5.30 (the sessions' own `total_cost_usd`).

## What was learned

`adage learn --demo` over the 12 training transcripts (the runner's learn phase):

- reflexes (active): `work-items/lookup-work-item`, `work-items/read-linked-files`, `finish-line/finish-line-04e5d4`
- reflexes left as drafts (awaiting review, never delivered): none
- instincts bound: 3

`adage eval routes` against `conventions.json` (the sandbox's ground truth):

| Index | Routes | True | False | Conventions found |
| --- | ---: | ---: | ---: | ---: |
| learned reflexes only | 3 | 3 | 0 | 3/4 |
| live index (reflexes + instincts) | 6 | 6 | 0 | 4/4 |
| instincts-only (day 0) | 4 | 4 | 0 | 2/4 |

## Effect on held-out tasks

Task-clustered: the CI resamples tasks, p permutes arms within tasks, Holm across the metrics. With 6 tasks per sandbox these are wide; see the corpus README for pooled numbers.

**adage vs holdout**

| Metric | adage / holdout | diff [95% CI] | p (Holm) |
| --- | --- | --- | --- |
| pass_rate | 1.00 / 0.92 | 0.08 [0.00, 0.25] | 1.00 |
| hidden_pass_rate | 1.00 / 0.92 | 0.08 [0.00, 0.25] | 1.00 |
| total_turns | 12.00 / 27.50 | -15.50 [-18.50, -12.00] | 0.00 |
| turns_to_first_edit | 2.50 / 15.00 | -12.50 [-13.00, -10.00] | 0.00 |
| cost_usd | 0.07 / 0.14 | -0.07 [-0.09, -0.05] | 0.00 |
| dead_end_hits | 0.58 / 0.92 | -0.33 [-0.67, -0.08] | 0.77 |
| follow_rate | 0.64 / 0.26 | 0.38 [0.28, 0.51] | 0.01 |

**instincts vs holdout**

| Metric | instincts / holdout | diff [95% CI] | p (Holm) |
| --- | --- | --- | --- |
| pass_rate | 0.92 / 0.92 | 0.00 [-0.25, 0.25] | 1.00 |
| hidden_pass_rate | 0.92 / 0.92 | 0.00 [-0.25, 0.25] | 1.00 |
| total_turns | 26.50 / 27.50 | -1.00 [-4.51, 2.00] | 1.00 |
| turns_to_first_edit | 14.50 / 15.00 | -0.50 [-4.00, 2.50] | 1.00 |
| cost_usd | 0.12 / 0.14 | -0.02 [-0.05, 0.01] | 1.00 |
| dead_end_hits | 0.92 / 0.92 | 0.00 [-0.25, 0.25] | 1.00 |
| follow_rate | 0.90 / 0.26 | 0.64 [0.43, 0.84] | 0.00 |

**adage vs instincts**

| Metric | adage / instincts | diff [95% CI] | p (Holm) |
| --- | --- | --- | --- |
| pass_rate | 1.00 / 0.92 | 0.08 [0.00, 0.25] | 1.00 |
| hidden_pass_rate | 1.00 / 0.92 | 0.08 [0.00, 0.25] | 1.00 |
| total_turns | 12.00 / 26.50 | -14.50 [-18.00, -9.50] | 0.00 |
| turns_to_first_edit | 2.50 / 14.50 | -12.00 [-15.00, -6.50] | 0.00 |
| cost_usd | 0.07 / 0.12 | -0.05 [-0.06, -0.04] | 0.00 |
| dead_end_hits | 0.58 / 0.92 | -0.33 [-0.67, 0.00] | 1.00 |
| follow_rate | 0.64 / 0.90 | -0.26 [-0.36, -0.13] | 0.03 |

## Files

- `manifest.json`, `sessions/`, `sandbox.txt`, `recorded/` as `evals/datasets/tools/build_demo.py` writes them (see `evals/datasets/demo-haiku45/README.md`); the split `round1` trains on the training sessions and tests on the holdout arm. `manifest.json` also names `conventions` and the `arms`.
- `recorded/index-round1.json`: the live index (the adage arm's); `recorded/index-instincts-only.json`: the instincts arm's (keyed `<name>@<root commit>`, as `adage learn --instincts-only` keys a repository).
- `conventions.json`: ground truth in `adage-conventions/1`.
- `packs/`: the learned reflex tracks as signed packs (`adage share`), when any was learned. They are signed by a throwaway **test signer** made for this corpus, never trust it for anything else:

```text
adage-corpus-test-signer namespaces="adage-pack" ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAig8V6VAqWQra6pKAJiHOhIgqfOYdKPHneZLsEb++Ox
```

- Instincts-arm decisions are not in `recorded/decisions.json` (build_demo.py reads adage-home's spools only).

Paths are redacted: the run root is `/sandbox`, and Claude Code's encoded project directory names are rewritten the same way.

```bash
adage eval evals/datasets/live-20260928-harbor-dispatch --split round1
adage eval routes evals/datasets/live-20260928-harbor-dispatch --split round1 --index evals/datasets/live-20260928-harbor-dispatch/recorded/index-round1.json
```
