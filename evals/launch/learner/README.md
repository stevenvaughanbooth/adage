# Learner diagnosis tooling

The scripts behind [../learner-diagnosis.md](../learner-diagnosis.md). They read the RepoBench run
roots (`/tmp/adage-repobench`, `/tmp/adage-rb2`) and the local mirrors, and write to `$LD`
(default `/tmp/adage-learner`). Nothing here spends money except the live slice (see below).

| Script | What it does |
|---|---|
| `mkds.py $LD/ds` | One `adage eval` dataset per repository: v1 training traces (train), v1+v2 unsteered control eval traces (test) |
| `mkhist.py` | `$LD/roots/<repo>`: the repository from its mirror at the base commit of its last training task, with the `.venv` marker the sandbox created |
| `mkroots.py` | The same without history (a training checkout's files) |
| `build.sh` | The `adage` binary and the `ledger` / `outcome_priors` examples (fast release profile) |
| `loo.py <outcome_priors>` | Leave-one-repository-out priors: `priors-<repo>.json`, `params-<repo>.json` (urfave-cli is never a source) |
| `led.py <ledger> <repo> <out.json> [--root D] [--priors F] [--demo]` | The drop ledger of a repository's training traces, summarised by gate; `ledsum.py <tag>` tabulates it |
| `ev.sh`, `evall.sh <adage> <tag>` | `adage eval` per repository (instincts off, with its priors and root) |
| `shat.py` | The interim replay value metric (review §5.2): useful, harmful, calls saved per session, task-clustered bootstrap; `--pair ds ev` pools repositories, `--routes RE` filters, `--v2` keeps v2 sessions |
| `loop.sh <tag>`, `report.sh <tag>` | One iteration: priors, ledgers, replay, Ŝ |
| `pairs.py`, `situ.py`, `tr.py` | Trace exploration: edited file → test file pairs, dead-end situations and recoveries |
| `slice_an.py` | The live slice's per-session delivery, follow and dead-end summary |

The live slice ran `evals/live/runner.py evals/repobench/configs/v2/click-slice.json learn`, then the
held-out phase and `report`, with `--execute --host-auth`, after `evals/repobench/rerun.py click
--suffix slice --max-turns 40 --repeats 1 --eval-budget 6` (six tasks kept).
