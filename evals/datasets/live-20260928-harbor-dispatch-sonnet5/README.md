# live-20260928-harbor-dispatch-sonnet5: 42 real Claude Code sessions

Real headless Claude Code sessions (`claude -p`, Claude Code 2.1.283, model `claude-sonnet-5`) on the `harbor-dispatch` sandbox, run on 2026-09-28 by `evals/live/runner.py` with `--host-auth` and `confine` (each session under bubblewrap, seeing only its own run directory). Part of the corpus in [evals/corpus](../../corpus/README.md). Every session ran in a fresh sandbox; hidden tests scored it afterwards.

| Phase | Arm | Sessions | Notes |
| --- | --- | ---: | --- |
| train | no hooks | 6 | 6 training tasks x 1 |
| eval | holdout | 12 | 6 held-out tasks x 2; control: hooks on, directives computed and logged, not delivered |
| eval | adage | 12 | 6 held-out tasks x 2; learned reflexes and instincts delivered |
| eval | instincts | 12 | 6 held-out tasks x 2; day-0 index: instincts bound from the repository's files, no learned reflex |

Training: 6/6 passed; 6/6 used the sandbox's lookup. Spend: $7.69 (the sessions' own `total_cost_usd`).

## What was learned

`adage learn --demo` over the 6 training transcripts (the runner's learn phase):

- reflexes (active): `work-items/lookup-work-item`, `work-items/read-linked-files`
- reflexes left as drafts (awaiting review, never delivered): none
- instincts bound: 4

`adage eval routes` against `conventions.json` (the sandbox's ground truth):

| Index | Routes | True | False | Conventions found |
| --- | ---: | ---: | ---: | ---: |
| learned reflexes only | 2 | 2 | 0 | 2/4 |
| live index (reflexes + instincts) | 6 | 6 | 0 | 4/4 |
| instincts-only (day 0) | 4 | 4 | 0 | 2/4 |

## Effect on held-out tasks

Task-clustered: the CI resamples tasks, p permutes arms within tasks, Holm across the metrics. With 6 tasks per sandbox these are wide; see the corpus README for pooled numbers.

**adage vs holdout**

| Metric | adage / holdout | diff [95% CI] | p (Holm) |
| --- | --- | --- | --- |
| pass_rate | 1.00 / 1.00 | 0.00 [0.00, 0.00] | 1.00 |
| hidden_pass_rate | 1.00 / 1.00 | 0.00 [0.00, 0.00] | 1.00 |
| total_turns | 13.00 / 19.00 | -6.00 [-8.00, -3.00] | 0.01 |
| turns_to_first_edit | 7.50 / 11.00 | -3.50 [-6.00, -2.00] | 0.01 |
| cost_usd | 0.15 / 0.20 | -0.06 [-0.07, -0.05] | 0.00 |
| dead_end_hits | 0.00 / 0.25 | -0.25 [-0.42, -0.08] | 1.00 |
| follow_rate | 0.49 / 0.18 | 0.31 [0.19, 0.43] | 0.02 |

**instincts vs holdout**

| Metric | instincts / holdout | diff [95% CI] | p (Holm) |
| --- | --- | --- | --- |
| pass_rate | 1.00 / 1.00 | 0.00 [0.00, 0.00] | 1.00 |
| hidden_pass_rate | 1.00 / 1.00 | 0.00 [0.00, 0.00] | 1.00 |
| total_turns | 17.50 / 19.00 | -1.50 [-3.00, 2.50] | 1.00 |
| turns_to_first_edit | 10.00 / 11.00 | -1.00 [-2.00, 3.00] | 1.00 |
| cost_usd | 0.21 / 0.20 | 0.00 [-0.02, 0.02] | 1.00 |
| dead_end_hits | 0.08 / 0.25 | -0.17 [-0.42, 0.17] | 1.00 |
| follow_rate | 0.88 / 0.18 | 0.69 [0.46, 0.54] | 0.69 |

**adage vs instincts**

| Metric | adage / instincts | diff [95% CI] | p (Holm) |
| --- | --- | --- | --- |
| pass_rate | 1.00 / 1.00 | 0.00 [0.00, 0.00] | 1.00 |
| hidden_pass_rate | 1.00 / 1.00 | 0.00 [0.00, 0.00] | 1.00 |
| total_turns | 13.00 / 17.50 | -4.50 [-8.00, -1.00] | 0.01 |
| turns_to_first_edit | 7.50 / 10.00 | -2.50 [-5.50, -0.50] | 0.02 |
| cost_usd | 0.15 / 0.21 | -0.06 [-0.08, -0.04] | 0.01 |
| dead_end_hits | 0.00 / 0.08 | -0.08 [-0.25, 0.00] | 1.00 |
| follow_rate | 0.49 / 0.88 | -0.39 [-0.33, -0.25] | 1.00 |

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
adage eval evals/datasets/live-20260928-harbor-dispatch-sonnet5 --split round1
adage eval routes evals/datasets/live-20260928-harbor-dispatch-sonnet5 --split round1 --index evals/datasets/live-20260928-harbor-dispatch-sonnet5/recorded/index-round1.json
```
