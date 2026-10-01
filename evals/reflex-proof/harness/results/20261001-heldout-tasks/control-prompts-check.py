"""Offline re-validation of the r9 control prompts (development prompt + realism line + canary suffix).

Composes the five control prompts with heldout_cohort.prompts, checks their structure, extracts every
python test block from each composed prompt and runs it against a gold `Finite` and broken variants,
as canary-test-block-check does. Run with a Python that has the pinned marshmallow and pytest
(the validation substrate's 3.12 venv). Spends nothing; launches nothing.
"""
import math
from pathlib import Path
import re
import sys
import tempfile

HARNESS = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(HARNESS))
import pytest  # noqa: E402
from marshmallow import ValidationError, validate  # noqa: E402

import curve_tasks  # noqa: E402
import heldout_analysis as analysis  # noqa: E402
import heldout_cohort as runner  # noqa: E402
import heldout_tasks  # noqa: E402

REALISM = "Do not create git commits."


class Finite(validate.Validator):
    default_message = "Must satisfy Finite."

    def __init__(self, *, error=None):
        self.error = error or self.default_message

    def _repr_args(self):
        return ""

    def _format_error(self, value):
        return self.error.format(input=value)

    def __call__(self, value):
        if not math.isfinite(value):
            raise ValidationError(self._format_error(value))
        return value


class RejectsBooleans(Finite):
    def __call__(self, value):
        if isinstance(value, bool):
            raise ValidationError(self._format_error(value))
        return super().__call__(value)


class AcceptsInf(Finite):
    def __call__(self, value):
        if value != value:
            raise ValidationError(self._format_error(value))
        return value


class WrongMessage(Finite):
    default_message = "Not finite."


def run(cls, block):
    validate.Finite = cls
    ns = dict(pytest=pytest, ValidationError=ValidationError, validate=validate)
    exec(block, ns)
    failures = []
    for name, fn in ns.items():
        if name.startswith("test_"):
            try:
                fn()
            except BaseException as e:  # noqa: BLE001
                failures.append((name, type(e).__name__))
    return failures


finite = next(v for v in curve_tasks.VARIANTS if v["id"] == "finite")
with tempfile.TemporaryDirectory() as tmp:
    root = Path(tmp)
    d = root / "private/tasks" / analysis.CONTROL_TASK
    d.mkdir(parents=True)
    (d / "prompt.txt").write_text(curve_tasks.prompt(finite))
    rows = runner.prompts(root, [r for r in analysis.matrix(heldout_tasks.EVALUATION) if r["condition"] != "positive"])

ok = True
for row in rows:
    prompt, suffix = row["prompt"], runner.canary.CONDITIONS[row["condition"]][0]
    structure = dict(
        starts_with_development_prompt=prompt.startswith(curve_tasks.prompt(finite)),
        realism_line_once=prompt.count(REALISM) == 1,
        realism_before_suffix=prompt.index(REALISM) < prompt.index("Work in this order"),
        suffix_intact=prompt.endswith(suffix),
        test_block_intact=runner.canary.TESTS in prompt,
        no_commit_instruction_elsewhere="commit" not in suffix.lower(),
    )
    blocks = re.findall(r"```python\n(.*?)```", prompt, re.S)
    combined = "\n".join(blocks)
    results = {cls.__name__: run(cls, combined) for cls in (RejectsBooleans, Finite, AcceptsInf, WrongMessage)}
    gold = "RejectsBooleans" if row["condition"] == "later-edit" else "Finite"
    behaviour = dict(gold_passes=results[gold] == [],
                     broken_fail=all(results[name] for name in ("AcceptsInf", "WrongMessage")))
    ok &= all(structure.values()) and all(behaviour.values())
    print(row["condition"], dict(structure, **behaviour, blocks=len(blocks)))
    for name, failures in results.items():
        print("   ", name, failures)
print("ALL CONTROL PROMPTS VALID" if ok else "CONTROL PROMPT CHECK FAILED")
sys.exit(0 if ok else 1)
