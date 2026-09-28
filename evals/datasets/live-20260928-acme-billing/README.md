# live-20260928-acme-billing: 36 real Claude Code sessions

Real headless Claude Code sessions (`claude -p`, Claude Code 2.1.283, model `claude-haiku-4-5-20251001`) on the `acme-billing` sandbox, run on 2026-09-28 by `evals/live/runner.py` with `--host-auth` and `confine` (each session under bubblewrap, seeing only its own run directory). Part of the corpus in [evals/corpus](../../corpus/README.md). Every session ran in a fresh sandbox; hidden tests scored it afterwards.

| Phase | Arm | Sessions | Notes |
| --- | --- | ---: | --- |
| train | no hooks | 12 | 6 training tasks x 2 |
| eval | control | 12 | 6 held-out tasks x 2; control: hooks on, directives computed and logged, not delivered |
| eval | adage | 12 | 6 held-out tasks x 2; learned reflexes and instincts delivered (no reflex was learned here, so this index is the instincts alone) |

Training: 4/12 passed; 3/12 used the sandbox's lookup. Spend: $3.61 (the sessions' own `total_cost_usd`).

## What was learned

`adage condition --demo` over the 12 training transcripts (the runner's learn phase):

- reflexes (active): none
- reflexes left as nascent reflexes (awaiting review, never delivered): none
- instincts bound: 7

`adage eval routes` against `conventions.json` (the sandbox's ground truth):

| Index | Routes | True | False | Conventions found |
| --- | ---: | ---: | ---: | ---: |
| learned reflexes only | 0 | 0 | 0 | 0/4 |
| live index (reflexes + instincts) | 7 | 7 | 0 | 2/4 |
| instincts-only (day 0) | 7 | 7 | 0 | 2/4 |

## Effect on held-out tasks

Task-clustered: the CI resamples tasks, p permutes arms within tasks, Holm across the metrics. With 6 tasks per sandbox these are wide; see the corpus README for pooled numbers.

**adage vs control**

| Metric | adage / control | diff [95% CI] | p (Holm) |
| --- | --- | --- | --- |
| pass_rate | 0.58 / 0.42 | 0.17 [-0.08, 0.42] | 1.00 |
| hidden_pass_rate | 0.58 / 0.42 | 0.17 [-0.08, 0.42] | 1.00 |
| total_turns | 25.50 / 22.00 | 3.50 [1.00, 6.50] | 1.00 |
| turns_to_first_edit | 19.50 / 16.50 | 3.00 [-1.50, 8.50] | 1.00 |
| cost_usd | 0.11 / 0.09 | 0.01 [-0.02, 0.04] | 1.00 |
| dead_end_hits | 0.00 / 0.00 | 0.00 [0.00, 0.00] | 1.00 |
| follow_rate | 1.00 / 1.00 | 0.00 – | – |

## Files

- `manifest.json`, `sessions/`, `sandbox.txt`, `recorded/` as `evals/datasets/tools/build_demo.py` writes them (see `evals/datasets/demo-haiku45/README.md`); the split `round1` trains on the training sessions and tests on the control arm. `manifest.json` also names `conventions` and the `arms`.
- `recorded/index-round1.json`: the live index (the adage arm's); `recorded/index-instincts-only.json`: the instincts arm's (keyed `<name>@<root commit>`, as `adage condition --instincts-only` keys a repository).
- `conventions.json`: ground truth in `adage-conventions/1`.
- `packs/`: the learned reflex tracks as signed packs (`adage share`), when any was learned. They are signed by a throwaway **test signer** made for this corpus, never trust it for anything else:

```text
adage-corpus-test-signer namespaces="adage-pack" ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIPfFX/VS3I6LwImxGgMiwQmJtrWmGJqifLcJrzqE6rtf
```

- Instincts-arm decisions are not in `recorded/decisions.json` (build_demo.py reads adage-home's spools only).

Paths are redacted: the run root is `/sandbox`, and Claude Code's encoded project directory names are rewritten the same way.

```bash
adage eval evals/datasets/live-20260928-acme-billing --split round1
adage eval routes evals/datasets/live-20260928-acme-billing --split round1 --index evals/datasets/live-20260928-acme-billing/recorded/index-round1.json
```
