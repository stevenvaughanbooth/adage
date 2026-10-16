# Local model mechanism pilot fixtures

This pack supports a bounded paid pilot on an operator's model-enabled machine. It contains **15 distinct semantic issue groups and 30 tasks**, with two variants per group. Variants never count as independent groups, and an entire group belongs to exactly one split.

| Split | Independent groups | Task variants | Purpose |
| --- | ---: | ---: | --- |
| Training | 10 | 20 | Eight distinct programming fixes share a generated-artifact workflow; two documentation groups provide unrelated contrast |
| Validation | 2 | 4 | List chunking and inventory aggregation, unseen during training |
| Test | 3 | 6 | Interval merging, invoice rounding, and an unrelated documentation correction |

This is a **synthetic mechanism pilot**, not a representative customer benchmark or evidence for a 20–30% commercial savings claim. The test cohort has only three groups. Its useful outcome is whether real model traces produce an eligible learned reflex, whether a reviewed reflex reaches a real held-out model session, and whether correctness, action completion, and measured usage remain credible. No candidate, no delivery, no measurable benefit, or a regression are valid findings.

## Commands and registry

```sh
python3 evals/live/sandboxes/pilot/selfcheck.py
python3 evals/live/sandboxes/pilot/generate.py --list
python3 evals/live/sandboxes/pilot/generate.py --out /tmp/pilot-example --task validation-chunk-list-01
python3 evals/live/sandboxes/pilot/grade.py --workdir /tmp/pilot-example --task validation-chunk-list-01
```

The generator requires an empty output directory and creates a deterministic real Git repository with origin `https://github.com/adage-evals/pilot`. The runtime repository key is `github.com/adage-evals/pilot`. Requirements are Linux, Python 3, Make, and Git; fixture generation, checks, and grading perform no network requests or package installations.

The `--list` command prints a registry compatible with the learning bridge:

```json
{
  "schema": 1,
  "repo": "github.com/adage-evals/pilot",
  "evidence_kind": "synthetic_mechanism_pilot",
  "tasks": {
    "train-median-01": {
      "split": "train",
      "group_id": "pilot/median",
      "variant": 1,
      "label": "median",
      "prompt": "Read TASK.md and complete the requested fix."
    }
  }
}
```

Runner argv integration:

```json
{
  "repo_key": "github.com/adage-evals/pilot",
  "sandbox": ["python3", "{repo}/evals/live/sandboxes/pilot/generate.py", "--out", "{dir}", "--task", "{task}"],
  "score": ["python3", "{repo}/evals/live/sandboxes/pilot/grade.py", "--workdir", "{dir}", "--task", "{task}"],
  "prompts": ["Read TASK.md and complete the requested fix."]
}
```

Pin the complete generator/grader dependency closure: `cases.py`, `generate.py`, `grade.py`, `project.py`, and `safe_python.py`. These files contain hidden acceptance cases and reference repairs, so keep the pack outside the agent's filesystem mounts. Only each generated checkout belongs in the agent's workspace. Ordinary harness isolation is still required; do not give an unrestricted agent filesystem access to the surrounding experiment or this pack.

## What the tasks establish

Each programming issue fixes a different behavior: text normalization, ASCII slugging, stable deduplication, range clamping, boolean parsing, median calculation, CSV escaping, nested brackets, list chunking, inventory aggregation, interval merging, or invoice rounding. Shared project commands are `make generate` followed by `make check`. The generated JSON contains source-bound client examples; a source repair alone leaves a stale artifact and fails grading.

The documentation tasks change configuration, API, or support documentation. They have already-correct source and generated artifacts. They provide genuine opportunities for the source-edit reflex to abstain. A correct documentation change passes; unrelated source or generated-artifact churn fails. Running an unnecessary check without changing files is observable only in the native trace and usage, not in this final-state score.

The generated README explains the project workflow equally in every arm. This intentionally makes a strong baseline. Do not remove useful baseline instructions to manufacture an Adage effect. Learn solely from real training model sessions, preserve the frozen group split, and stop rather than authoring substitute reflexes or lowering learning thresholds if training yields none.

The external grader uses fixed expected outputs and protected original verification files. It detects missing behavior, stale or forged generated content, changed source digests, and rewritten visible checks. It uses a deliberately restricted JSON-only Python evaluator with no imports, I/O, reflection, or dynamic calls; CPU, memory, output size, and wall time are bounded in a separate process. This is a controlled programming fixture, not a general-purpose sandbox for arbitrary Python repositories.

Final-state grading does **not** show that the model ran a check, waited for it, followed a delivered reflex, or verified the revision it published. Every grade therefore sets `verification_execution_observed: false`. Derive those mechanism metrics from retained native tool events and hook evidence outside the checkout. Neither visible test output nor an in-repository receipt establishes execution evidence.

The selfcheck makes no model calls. It verifies every untouched issue fails, every external reference repair passes, every programming repair without regeneration fails, documentation controls reject unrelated churn, fake green checks cannot spoof success, groups are disjoint, generation is reproducible, and candidate evaluation rejects dangerous or unbounded behavior.
