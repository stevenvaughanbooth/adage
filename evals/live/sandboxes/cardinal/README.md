# Cardinal diagnostic fixtures

These no-download fixtures test the local evaluation plumbing and two concrete programming workflows. They are **synthetic diagnostic tasks**, not independent evidence for customer savings, broad tool recovery, or completion/PR enforcement.

```sh
python3 evals/live/sandboxes/cardinal/selfcheck.py
python3 evals/live/sandboxes/cardinal/generate.py --list
python3 evals/live/sandboxes/cardinal/generate.py --out /tmp/cardinal-example --task dev-manifest-01
python3 evals/live/sandboxes/cardinal/grade.py --workdir /tmp/cardinal-example --task dev-manifest-01
```

The generator requires an empty output directory, Python 3, and Git. Visible checks require Make. No model, package installation, API, or external service is used by the selfcheck. Every checkout has a real deterministic initial Git commit and origin `https://github.com/adage-evals/cardinal`, matching runtime repository key `github.com/adage-evals/cardinal`.

## Task families

| Family | Intended workflow | Independent final-state criterion |
| --- | --- | --- |
| `manifest` | Edit a product price, run `make generate`, then `make check` | Requested source data and generated application catalog agree with an external reference; protected checks are unchanged |
| `layout` | Fix a subtotal helper, then use `python3 tools/check.py` or `make check` to verify a Python src-layout project | The helper computes correct arithmetic across 25 cases; protected tests and environment setup are unchanged |
| `control` | Answer a read-only product question | No changes to visible fixture files; no unrelated new files, except harness metadata and Python caches |

The layout case demonstrates a concrete environment-specific command: naive `python3 -m unittest discover -s tests` fails to import a src-layout module, while the documented project command sets its module search path correctly. It does not prove general external-tool recovery. To avoid executing candidate-controlled code in the hidden grader, this deliberately small task requires a single pure arithmetic return expression; the grader evaluates a bounded AST directly. This is not a general Python solution grader.

The control grader measures absence of file churn, **not whether the natural-language answer is correct**. Keep that endpoint separate from task-quality success in a real study.

## Runner interface

Use these argv arrays in a local runner configuration:

```json
{
  "repo_key": "github.com/adage-evals/cardinal",
  "sandbox": ["python3", "{repo}/evals/live/sandboxes/cardinal/generate.py", "--out", "{dir}", "--task", "{task}"],
  "score": ["python3", "{repo}/evals/live/sandboxes/cardinal/grade.py", "--workdir", "{dir}", "--task", "{task}"],
  "prompts": ["Read TASK.md and complete the request."]
}
```

This is an integration fragment, not a paid-run config: model, budget, tasks, arms, learning inputs, and execution bounds must be selected explicitly. No authored reflex or compiled index is supplied. A treatment run is meaningful only if an independently learned and reviewed index contains applicable reflexes for this repository. A baseline or generator/grader rehearsal remains useful without one.

The grader and this directory must remain outside the agent's filesystem mounts. Give the model only its generated checkout and required harness files. Do not run an unrestricted agent next to hidden grading code. Final grading uses externally retained fixture definitions and never trusts an in-repository receipt, rewritten test output, or candidate-controlled grader.

## Splits and evidence limits

`generate.py --list` emits 42 entries: eight training-pool, two development-pool, and four test-pool variants per family. The `train-*`, `dev-*`, and `test-*` IDs provide explicit rehearsal pools only. Every registry entry has `split: "diagnostic"`, a `pool` field, and `semantic_group: "cardinal/<family>-v1"`. Changing numbers or IDs does **not** create an independent group.

Consequently, these variants must not be fed into a frozen train/test protocol as independent tasks: their groups overlap. Eight training variants also do not establish eight independent production learning examples. Use distinct, independently collected task groups and a separate real registry to qualify learning support and commercial savings. Keep these fixtures as cheap diagnostics and workflow reproductions.

The grader reports `verification_execution_observed: false` because final-state correctness cannot prove that an agent actually ran tests, waited for completion, used a hook, or verified the revision before publishing a PR. Those outcomes require native trace and revision evidence collected outside the checkout. Neither unchanged check files nor passing final grading is a substitute.

The selfcheck exercises every task and demonstrates unchanged tasks failing, correct reference solutions passing, source-only changes failing, successful visible checks being insufficient after tampering, wrong environment commands failing, and read-only controls rejecting churn. It spends no model tokens and cannot establish token savings.
