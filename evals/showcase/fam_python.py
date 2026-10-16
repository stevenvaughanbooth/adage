"""Showcase families on Python repositories, ported from the recurring-workflows research.

  click-type    "add a new built-in parameter type `click.<NAME>`" (pallets/click @ 06b2a67)
  mm-validator  "add a new validator `marshmallow.validate.<Name>`" (marshmallow @ 7f0792bd)

The prompts, hidden checks and gold patches are the research's own (evals/lab/research/
recurring-workflows/families.py, validated 20/20 there), imported unchanged. Each family gets 2 new
variants here so that it has 12 (4 training + 8 held out). The research's training variants stay the
training variants, and its held-out variants stay held out; the 2 new variants are appended to the
held-out list. The order is fixed here, before any showcase session.
"""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
LAB = HERE.parent / "lab" / "research" / "recurring-workflows"


def _load(name: str, path: Path):
    """The lab module under its own name (the showcase has its own `families`)."""
    if name in sys.modules:
        return sys.modules[name]
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    spec.loader.exec_module(mod)
    return mod


lab = _load("rw_lab_families", LAB / "families.py")

# ------------------------------------------------------------------------------------- click-type

CLICK_NEW = [
    dict(id="time", name="TIME", cls="TimeParamType",
         desc="which accepts a time of day written as `HH:MM` or `HH:MM:SS` (24-hour clock, two digits per "
              "field) and converts it to a `datetime.time`.",
         valid=[("09:30", "datetime.time(9, 30)"), ("23:59:59", "datetime.time(23, 59, 59)"),
                ("00:00", "datetime.time(0, 0)")],
         invalid=["24:00", "9:3", "noon"],
         gold="""
            import datetime as _dt
            m = re.fullmatch(r"(\\d{2}):(\\d{2})(?::(\\d{2}))?", value.strip())
            if not m:
                fail(value)
            try:
                return _dt.time(int(m.group(1)), int(m.group(2)), int(m.group(3) or 0))
            except ValueError:
                fail(value)
         """),
    dict(id="log-level", name="LOG_LEVEL", cls="LogLevelParamType",
         desc="which accepts a logging level name, one of `DEBUG`, `INFO`, `WARNING`, `ERROR` and `CRITICAL` "
              "(case-insensitive), and converts it to the level's `int` value in the `logging` module "
              "(`INFO` becomes `20`).",
         valid=[("INFO", "20"), ("debug", "10"), ("Critical", "50")],
         invalid=["verbose", "5", "warn"],
         gold="""
            levels = {"DEBUG": 10, "INFO": 20, "WARNING": 30, "ERROR": 40, "CRITICAL": 50}
            key = value.strip().upper()
            if key not in levels:
                fail(value)
            return levels[key]
         """),
]

CLICK_VARIANTS = lab.CLICK + CLICK_NEW  # 4 training (as the research) + 6 + 2 held out

CLICK_NEGATIVES = [
    # RepoBench click fixes whose real fix edits src/click/types.py or the Choice type (the card's area)
    dict(id="click-d251cb0a", source="repobench"),
    dict(id="click-1a4d8c1b", source="repobench"),
    dict(id="click-eaf33a95", source="repobench"),
    # hard negatives written for the research's detection study (prompts_extra.json), same vocabulary
    dict(id="neg-click-choice-hidden", source="handwritten", edits=["src/click/types.py", "tests/test_types.py"],
         prompt="Add a `hidden` parameter to `click.Choice` so that some choices are accepted but not listed in "
                "the help text."),
    dict(id="neg-click-doc-custom-type", source="handwritten", edits=["docs/parameter-types.md"],
         prompt="Document in the parameter types guide how to write a custom `ParamType` subclass, with an "
                "example that converts a comma-separated list."),
    dict(id="neg-click-path-resolve", source="handwritten", edits=["src/click/types.py", "tests/test_types.py"],
         prompt="The `click.Path` type's `resolve_path` option does not expand `~`. Please fix it and add a test."),
]

# ----------------------------------------------------------------------------------- mm-validator

MM_NEW = [
    dict(id="alphanumeric", cls="Alphanumeric", sig="Alphanumeric(*, error=None)", args="",
         desc="which succeeds if the input is a non-empty string made only of ASCII letters and digits.",
         msg="Must contain only letters and digits.", keys=["input"],
         formatted="Must contain only letters and digits.", repr_arg="",
         valid=["'abc123'", "'Z'"], invalid=["'a-b'", "''", "'héllo'"],
         custom=("{input} is not alphanumeric", "a-b is not alphanumeric"), init=[],
         check="bool(value) and value.isascii() and value.isalnum()"),
    dict(id="max-words", cls="MaxWords", sig="MaxWords(max, *, error=None)", args="3",
         desc="which succeeds if the input string has at most `max` words (separated by whitespace).",
         msg="Must have at most {max} words.", keys=["input", "max"], formatted="Must have at most 3 words.",
         repr_arg="max=3", valid=["'one two three'", "''", "'  a  b '"], invalid=["'a b c d'", "'w x y z q'"],
         custom=("{input} has more than {max} words", "a b c d has more than 3 words"), init=["max"],
         check="len(value.split()) <= self.max"),
]

MM_VARIANTS = lab.MM + MM_NEW

MM_NEGATIVES = [
    dict(id="marshmallow-39e7c833", source="repobench"),
    dict(id="marshmallow-65374df0", source="repobench"),
    dict(id="marshmallow-f07eadc8", source="repobench"),
    dict(id="neg-mm-email-plus", source="handwritten", edits=["src/marshmallow/validate.py", "tests/test_validate.py"],
         prompt="The `Email` validator rejects addresses with a plus sign in the local part. Fix it and add a test."),
    dict(id="neg-mm-range-bounds", source="handwritten", edits=["src/marshmallow/validate.py", "tests/test_validate.py"],
         prompt="Make `validate.Range` report both bounds in its error message when min and max are given."),
    dict(id="neg-mm-hex-field", source="handwritten",
         edits=["src/marshmallow/fields.py", "CHANGELOG.rst", "tests/test_fields.py"],
         prompt="Add a new field `fields.Hex` that serializes integers as hexadecimal strings, with tests and a "
                "changelog entry."),
]


def _family(fid: str, base: dict, variants: list, negatives: list, name_key: str, card_files: list) -> dict:
    return dict(base, id=fid, variants=variants, negatives=negatives, name_key=name_key, lang="python",
                probe_edits=card_files, repo_key={"click": "github.com/pallets/click",
                                                  "marshmallow": "github.com/marshmallow-code/marshmallow"}[base["repo"]],
                checks=None, forward_env=[], confine_expose=[], prune=[])


CLICK = _family("click-type", lab.FAMILIES["click-type"], CLICK_VARIANTS, CLICK_NEGATIVES, "name",
                ["src/click/types.py", "src/click/__init__.py", "docs/api.md", "CHANGES.md"])
MM = _family("mm-validator", lab.FAMILIES["mm-validator"], MM_VARIANTS, MM_NEGATIVES, "cls",
             ["src/marshmallow/validate.py", "CHANGELOG.rst", "tests/test_validate.py"])
