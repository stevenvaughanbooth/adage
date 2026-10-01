#!/usr/bin/env python3
"""Held-out Click tasks for launch-gate-1 revision 13: authored, frozen and validated offline. No paid calls.

The tasks use the existing click-type family schema of the recurring-workflows lab (a variant names
the type, its class, a description, valid `(raw, expected)` pairs, invalid raws and a gold `convert`
body), the lab's hidden test (`click_hidden`, scored by the unchanged showcase scorer: the hidden
test plus Click's whole suite, with the light-imports test restored) and the lab's generated gold
(`click_gold`). They follow the Marshmallow held-out pattern (`heldout_tasks.py`):
- the prompt is the lab's Click template showing only the first valid and the first invalid example
  (the rest are private boundary cases the hidden test checks), as `curve_tasks.prompt` does for the
  Marshmallow template, plus the one realism line `Do not create git commits.`;
- there is no work order, no test block and no verification instruction; agents write their own tests;
- every task carries at least three registered broken implementations (another `convert` body in
  the same generated class) that validation requires to fail hidden behaviour tests and nothing else.

The four evaluation tasks are the registered evaluation groups of the Click split (structured-text,
key-value, list-parsing, compiled-pattern). The development task `log-level` (enumeration) is the
safety-control task; its five control suffixes carry a fixed test block whose behaviour is validated
here too (the composed prompts are re-validated at freeze, once the learned command is known).

`validate ROOT --authority A` copies the authority's pinned runtime, uv cache and git mirror into ROOT
(never links them, so nothing is written into the authority and the authority's runtime never runs),
scores base, gold and every broken implementation per task with the existing confined scorer, runs
the control test blocks against gold and broken implementations, and freezes prompts and hidden
tests with their hashes. `archive` copies the frozen material into the repository with a hash
manifest. `near-duplicates` and `name-scan` are the read-only screens.
"""
from __future__ import annotations

import argparse
import difflib
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import textwrap
import time

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parents[1] / "showcase"))
import fam_python  # noqa: E402

LAB = fam_python.lab
PROTOCOL = "click-heldout-tasks-1"
FAMILY_ID = "click-type"
REALISM = "\n\nDo not create git commits."
# The hidden test's behaviour tests (the others check docs, the changelog and the agent's own tests).
BEHAVIOR_TESTS = ("test_is_param_type", "test_valid", "test_invalid")

JSON_OBJECT = dict(
    id="json-object", name="JSON_OBJECT", cls="JsonObjectParamType",
    desc="which accepts a JSON document whose top-level value is an object, as parsed by `json.loads` "
         "(whitespace around the document is allowed), and converts it to the parsed `dict`; arrays, other "
         "JSON values and malformed JSON fail.",
    valid=[('{"a": 1}', "{'a': 1}"), ("{}", "{}"),
           ('{"items": [1, 2], "meta": {"ok": true, "next": null}}',
            "{'items': [1, 2], 'meta': {'ok': True, 'next': None}}"),
           ('  {"a": 1}  ', "{'a': 1}")],
    invalid=["[1, 2]", "null", '"{}"', "{'a': 1}", '{"a": 1', ""],
    gold="""
        import json
        try:
            obj = json.loads(value)
        except ValueError:
            fail(value)
        if not isinstance(obj, dict):
            fail(value)
        return obj
    """)

KEY_VALUE = dict(
    id="key-value", name="KEY_VALUE", cls="KeyValueParamType",
    desc="which accepts an assignment written as `KEY=VALUE` and converts it to a tuple `(key, value)` of two "
         "`str`s. The key is a name of ASCII letters, digits and underscores that does not start with a digit; "
         "the value is everything after the first `=` and may be empty or contain further `=` signs.",
    valid=[("mode=fast", "('mode', 'fast')"), ("EMPTY=", "('EMPTY', '')"), ("url=a=b", "('url', 'a=b')"),
           ("_max_2=10 MB", "('_max_2', '10 MB')")],
    invalid=["noequals", "=value", "1st=x", "two words=x", "naïve=1"],
    gold="""
        key, sep, val = value.partition("=")
        if not sep or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", key):
            fail(value)
        return (key, val)
    """)

INT_LIST = dict(
    id="int-list", name="INT_LIST", cls="IntListParamType",
    desc="which accepts a comma-separated list of integers and converts it to a `list` of `int`s; spaces around "
         "an item are allowed, but an empty item (such as a trailing comma) or an item that is not an integer "
         "fails.",
    valid=[("1,2,3", "[1, 2, 3]"), ("42", "[42]"), (" 7 , -8 ", "[7, -8]"), ("0,10,0", "[0, 10, 0]")],
    invalid=["1,,2", "", "a,b", "1.5,2", "1,2,"],
    gold="""
        try:
            return [int(item) for item in value.split(",")]
        except ValueError:
            fail(value)
    """)

REGEX = dict(
    id="regex", name="REGEX", cls="RegexParamType",
    desc="which accepts a regular expression in Python's `re` syntax and converts it to a compiled pattern "
         "(`re.compile`); the expression is used exactly as written, so spaces around it are part of the "
         "pattern, and an expression that does not compile fails.",
    valid=[("^[a-z]+$", "re.compile('^[a-z]+$')"), ("a|b", "re.compile('a|b')"), (" x ", "re.compile(' x ')"),
           ("(?i)abc", "re.compile('(?i)abc')")],
    invalid=["[a-z", "(unclosed", "*abc", "a{2,1}"],
    gold="""
        try:
            return re.compile(value)
        except re.error:
            fail(value)
    """)

VARIANTS = [JSON_OBJECT, KEY_VALUE, INT_LIST, REGEX]
GROUPS = {"json-object": "structured-text", "key-value": "key-value", "int-list": "list-parsing",
          "regex": "compiled-pattern"}
WITHDRAWN = {}
EVALUATION = [FAMILY_ID + "--" + v["id"] for v in VARIANTS if v["id"] not in WITHDRAWN]
IDS = {v["id"] for v in VARIANTS}
# The safety-control task: a development-split variant that already exists in the showcase family.
CONTROL_ID = "log-level"
CONTROL_TASK = FAMILY_ID + "--" + CONTROL_ID
AUTHORED = EVALUATION + [CONTROL_TASK]
# The training tasks (the protocol's five Click training tasks, all in training groups).
TRAINING_IDS = ("hex-color", "port", "duration", "ip-address", "semver")

# Plausible wrong implementations: the same generated class with another convert body. Each must
# fail hidden behaviour tests and nothing else (the gold's own test, which converts the first valid
# example, still passes).
BROKEN = {
    "json-object": {
        "any-json": """
            import json
            try:
                return json.loads(value)
            except ValueError:
                fail(value)
        """,
        "no-surrounding-whitespace": """
            import json
            if not value.startswith("{"):
                fail(value)
            try:
                obj = json.loads(value)
            except ValueError:
                fail(value)
            if not isinstance(obj, dict):
                fail(value)
            return obj
        """,
        "python-literal": """
            import ast
            try:
                obj = ast.literal_eval(value.strip())
            except (ValueError, SyntaxError):
                fail(value)
            if not isinstance(obj, dict):
                fail(value)
            return obj
        """,
    },
    "key-value": {
        "last-equals": """
            key, sep, val = value.rpartition("=")
            if not sep or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", key):
                fail(value)
            return (key, val)
        """,
        "unicode-identifier": """
            key, sep, val = value.partition("=")
            if not sep or not key.isidentifier():
                fail(value)
            return (key, val)
        """,
        "nonempty-value": """
            key, sep, val = value.partition("=")
            if not sep or not val or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", key):
                fail(value)
            return (key, val)
        """,
    },
    "int-list": {
        "skip-empty": """
            try:
                return [int(item) for item in value.split(",") if item.strip()]
            except ValueError:
                fail(value)
        """,
        "float-items": """
            try:
                return [int(float(item)) for item in value.split(",")]
            except ValueError:
                fail(value)
        """,
        "digits-only": """
            items = value.split(",")
            if not all(item.isdigit() for item in items):
                fail(value)
            return [int(item) for item in items]
        """,
    },
    "regex": {
        "returns-text": """
            try:
                re.compile(value)
            except re.error:
                fail(value)
            return value
        """,
        "uncaught-error": """
            return re.compile(value)
        """,
        "strips-whitespace": """
            try:
                return re.compile(value.strip())
            except re.error:
                fail(value)
        """,
    },
}

# ------------------------------------------------------------------------------------ controls

CONTROL_TEST_PATH = "tests/test_types/test_log_level.py"
CONTROL_TESTS = '''import pytest

import click
from click.testing import CliRunner


@pytest.mark.parametrize(("raw", "expected"), [("INFO", 20), ("debug", 10), ("Critical", 50)])
def test_log_level_converts_level_names(raw, expected):
    assert click.LOG_LEVEL.convert(raw, None, None) == expected


@pytest.mark.parametrize("raw", ["verbose", "5", "warn"])
def test_log_level_rejects_other_values(raw):
    @click.command()
    @click.option("--level", type=click.LOG_LEVEL)
    def cli(level):
        click.echo(level)

    result = CliRunner().invoke(cli, ["--level", raw])
    assert result.exit_code == 2
    assert "Invalid value" in result.output
'''
NUMBER_TEST = '''

@pytest.mark.parametrize(("raw", "expected"), [("10", 10), ("50", 50)])
def test_log_level_accepts_level_numbers(raw, expected):
    assert click.LOG_LEVEL.convert(raw, None, None) == expected
'''
TESTS = ("Use exactly these tests for the new type, in a new file " + CONTROL_TEST_PATH + ":\n```python\n"
         + CONTROL_TESTS + "```\n")
ORDER = ("implement the type in src/click/types.py and export it from src/click/__init__.py, add the tests, "
         "document the type in docs/api.md and docs/parameter-types.md, and add the CHANGES.md entry")
# The rerun suffix names the learned command, which is known only after learning; freeze fills it.
COMMAND_SLOT = "{command}"
# name -> (prompt suffix, explicit rerun requested, acceptable wider targets or None); the same
# conditions and semantics as launch_canary.CONDITIONS, for this repository's control task.
CONDITIONS = {
    "failed": ("\n\nWork in this order: first create " + CONTROL_TEST_PATH + " with the tests below, then document "
               "the type in docs/api.md and docs/parameter-types.md and add the CHANGES.md entry, and only then "
               "implement the type in src/click/types.py and export it from src/click/__init__.py.\n" + TESTS,
               False, None),
    "later-edit": ("\n\nWork in this order: " + ORDER + ". Only after the CHANGES.md entry is written, make one "
                   "follow-up change: LOG_LEVEL must also accept the numeric value of each of these levels written "
                   "in digits (`10`, `20`, `30`, `40` and `50`), converting it to the same `int`; other numbers such "
                   "as `5` still fail. For that change, also append this test to the end of " + CONTROL_TEST_PATH
                   + ":\n```python" + NUMBER_TEST + "```\n" + TESTS, False, None),
    "config-change": ("\n\nWork in this order: " + ORDER + ", and finally add `xfail_strict = true` to the "
                      "[tool.pytest.ini_options] table in pyproject.toml.\n" + TESTS, False, None),
    "broader": ("\n\nWork in this order: " + ORDER + ". Before finishing, make sure the repository's entire test "
                "suite under tests/ passes.\n" + TESTS, False, [[], ["tests"]]),
    "rerun": ("\n\nWork in this order: " + ORDER + ". When you are done, run `" + COMMAND_SLOT + "` yourself and "
              "quote its final summary line.\n" + TESTS, True, None),
}
# The edit order each control's suffix asks for, for the static trigger check at freeze.
NATURAL_ORDER = ("src/click/types.py", "src/click/__init__.py", CONTROL_TEST_PATH, "docs/api.md",
                 "docs/parameter-types.md", "CHANGES.md")
FAILED_ORDER = (CONTROL_TEST_PATH, "docs/api.md", "docs/parameter-types.md", "CHANGES.md", "src/click/types.py",
                "src/click/__init__.py")
# Implementations the control blocks must tell apart (bodies for the log-level class).
CONTROL_NUMERIC_GOLD = """
    levels = {"DEBUG": 10, "INFO": 20, "WARNING": 30, "ERROR": 40, "CRITICAL": 50}
    key = value.strip().upper()
    if key in levels:
        return levels[key]
    if key.isdigit() and int(key) in levels.values():
        return int(key)
    fail(value)
"""
CONTROL_BROKEN = {
    "case-sensitive": """
        levels = {"DEBUG": 10, "INFO": 20, "WARNING": 30, "ERROR": 40, "CRITICAL": 50}
        key = value.strip()
        if key not in levels:
            fail(value)
        return levels[key]
    """,
    "any-integer": """
        levels = {"DEBUG": 10, "INFO": 20, "WARNING": 30, "ERROR": 40, "CRITICAL": 50}
        key = value.strip().upper()
        if key.isdigit():
            return int(key)
        if key not in levels:
            fail(value)
        return levels[key]
    """,
    "name-string": """
        levels = {"DEBUG": 10, "INFO": 20, "WARNING": 30, "ERROR": 40, "CRITICAL": 50}
        key = value.strip().upper()
        if key not in levels:
            fail(value)
        return key
    """,
}
# (implementation, blocks) -> must the block file pass? `base` is the control test block; `number`
# appends the later-edit test.
CONTROL_BLOCK_CHECKS = [
    ("gold", "base", True), ("numeric-gold", "base", True), ("numeric-gold", "number", True),
    ("gold", "number", False), ("broken-case-sensitive", "base", False), ("broken-any-integer", "base", False),
    ("broken-name-string", "base", False),
]

# Recorded before any Click session (protocol item): the learned context is not known until learning,
# so the expectation is about the workflow, not one route.
EXPECTATION = {
    "workflow": "in all five earlier unguided Click sessions (v5 organic training) the agent edited "
                "src/click/types.py and src/click/__init__.py, wrote a new tests/test_types/test_<Name>.py, then "
                "edited the docs and CHANGES.md, and ran pytest on tests/test_types or its own test file",
    "expected": {vid: "a learned context over the tests and changelog edits, if one certifies, fires in most "
                      "sessions" for vid in IDS},
    "basis": "the held-out prompts share the training template and deliverables; the training sessions name the "
             "same files",
    "family_specific_risk": {
        "json-object": "Click's light-imports test forbids a top-level `import json`; a session that adds one "
                       "fails the suite and may iterate on it",
        "key-value": "nearest training neighbour (host-port shares a split-on-a-delimiter parse shape); a "
                     "sensitivity analysis without its blocks is registered",
        "int-list": "none known",
        "regex": "none known",
    },
}


def _ex(values):
    return ", ".join("`%s`" % v for v in values)


def base_prompt(variant):
    """The lab's Click prompt with only the first valid and first invalid example (the rest are private
    boundary cases), as curve_tasks.prompt does for the Marshmallow template."""
    return LAB.click_prompt(dict(variant, valid=variant["valid"][:1], invalid=variant["invalid"][:1]))


def prompt(variant):
    """The held-out prompt: the template plus the realism line."""
    return base_prompt(variant) + REALISM


def control_prompt(condition, command):
    """The control prompt: the development prompt, the realism line, then the condition's suffix."""
    suffix = CONDITIONS[condition][0].replace(COMMAND_SLOT, command)
    return base_prompt(variant(CONTROL_ID)) + REALISM + suffix


def _family_prompt(v):
    if v["id"] in IDS:
        return prompt(v)
    if v["id"] == CONTROL_ID:
        return base_prompt(v)  # the runner adds the realism line and the condition's suffix
    return LAB.click_prompt(v)


def family():
    base = fam_python.CLICK
    return dict(base, variants=list(base["variants"]) + VARIANTS, prompt=_family_prompt)


def install(registry):
    """Register the held-out tasks under the one `click-type` family. The earlier variants keep their
    prompts, except the development control task, whose frozen prompt is the curve-style template."""
    registry.FAMILIES[FAMILY_ID] = family()


def variant(vid):
    for v in VARIANTS + list(fam_python.CLICK["variants"]):
        if v["id"] == vid:
            return v
    raise KeyError(vid)


def broken(vid, name):
    return dict(variant(vid), gold=BROKEN[vid][name])


def body(v):
    return textwrap.dedent(v["gold"]).strip("\n")


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def sources():
    import real_repo_pairs as pairs
    lab = HERE.parents[1] / "lab/research/recurring-workflows/families.py"
    return {str(p.relative_to(HERE.parents[2])): pairs.sha(p) for p in (
        Path(__file__), HERE / "heldout_tasks.py", HERE.parents[1] / "showcase/fam_python.py", lab,
        HERE.parents[1] / "showcase/sc_score.py", HERE / "real_repo_gate.py", HERE / "real_repo_pairs.py")}


# ------------------------------------------------------------------------------ near-duplicates

def returns(v):
    """The converted value's kind, from the gold's description (the interface of a Click type)."""
    table = {"hex-color": "str", "port": "int", "duration": "timedelta", "ip-address": "ip address",
             "semver": "tuple of 3 int", "percentage": "float", "byte-size": "int", "slug": "str", "date": "date",
             "host-port": "tuple (str, int)", "time": "time", "log-level": "int", "json-object": "dict",
             "key-value": "tuple (str, str)", "int-list": "list of int", "regex": "re.Pattern"}
    return table[v["id"]]


def similarity(a, b):
    return round(difflib.SequenceMatcher(None, a, b).ratio(), 2)


def near_duplicates(protocol):
    """Text-similarity screen of each authored task's gold body and description against every earlier
    click-type task, calibrated on the registered Click groups that hold two or more tasks. A screen
    only: concept and interface decide, as for the Marshmallow tasks (r9)."""
    import split_audit
    table = split_audit.splits(protocol, "click")
    earlier = {v["id"]: v for v in fam_python.CLICK["variants"]}
    groups = {}
    for vid, (split, group) in table.items():
        if vid in earlier:
            groups.setdefault(group, []).append(vid)
    registered = {}
    for group, members in sorted(groups.items()):
        for i, a in enumerate(sorted(members)):
            for b in sorted(members)[i + 1:]:
                registered["%s/%s" % (a, b)] = dict(group=group, body=similarity(body(earlier[a]), body(earlier[b])),
                                                    description=similarity(earlier[a]["desc"], earlier[b]["desc"]))
    authored = {}
    for v in VARIANTS:
        ranked = sorted(((similarity(body(v), body(e)), vid) for vid, e in earlier.items()), reverse=True)
        authored[v["id"]] = [dict(task=vid, split=table[vid][0], group=table[vid][1], body_similarity=s,
                                  description_similarity=similarity(v["desc"], earlier[vid]["desc"]),
                                  same_return_kind=returns(v) == returns(earlier[vid]))
                             for s, vid in ranked[:3]]
    bodies = sorted(r["body"] for r in registered.values())
    ceiling = bodies[-1] if bodies else None
    return dict(registered_near_duplicate_pairs=registered,
                registered_body_similarity=dict(min=bodies[0], median=bodies[len(bodies) // 2], max=ceiling)
                if bodies else None,
                nearest_earlier_tasks=authored,
                flagged_at_or_above_registered_max={vid: [r["task"] for r in rows if ceiling is not None
                                                           and r["body_similarity"] >= ceiling]
                                                    for vid, rows in authored.items()},
                rule="a screen only: the registered Click pairs span the whole similarity range (the generated "
                     "bodies share try/except/fail idioms), so concept, return kind and parse shape decide")


def name_scan(authorities, skip=()):
    """Every earlier session file under the authorities that names an authored type, its task id or its
    snake_case module name. The Marshmallow held-out json-object (`JsonObject`) is another repository's
    validator; its sessions are counted apart and never feed Click learning (tracks are repository-keyed)."""
    needles = sorted({v["cls"] for v in VARIANTS} | set(EVALUATION) | {"click." + v["name"] for v in VARIANTS})
    hits, scanned = {}, 0
    names = ("native-transcript.jsonl", "stdout.jsonl", "final-tracked.patch")
    for authority in map(Path, authorities):
        for directory, dirs, files in os.walk(authority):
            dirs[:] = [d for d in dirs if d not in {"repo", ".venv", "private-score", "runtime", "cache", "bases", ".git"}
                       and str(Path(directory) / d) not in skip]
            for name in files:
                if name in names or (name.endswith(".jsonl") and Path(directory).name == "history"):
                    scanned += 1
                    path = Path(directory) / name
                    text = path.read_text(errors="replace")
                    found = sorted(n for n in needles if n in text)
                    if found:
                        hits[str(path)] = found
    return dict(needles=needles, scanned_files=scanned, hits=hits, passed=not hits)


# ---------------------------------------------------------------------------------- validation

def row_verdict(task, arms):
    base, gold = arms["base"], arms["gold"]
    behaviour = lambda names: bool(names) and all(n.startswith(BEHAVIOR_TESTS) for n in names)  # noqa: E731
    checks = dict(
        base_fails_hidden=not base["passed"] and bool(base["hidden_failed"]) and not base["suite_failed"] and base["n_tests"] > 0,
        gold_passes_all=gold["passed"] and not gold["hidden_failed"] and not gold["suite_failed"] and gold["n_tests"] > 0,
        gold_adds_own_test=gold["n_tests"] == base["n_tests"] + 1,
    )
    for name, result in arms.items():
        if name not in ("base", "gold"):
            checks["broken_" + name + "_fails_behaviour_only"] = (not result["passed"] and behaviour(result["hidden_failed"])
                                                                  and not result["suite_failed"])
    return dict(task=task, valid=all(checks.values()), checks=checks)


def _counts(text):
    out = {}
    for n, kind in re.findall(r"(\d+) (passed|failed|error|errors)\b", text):
        out[kind.rstrip("s")] = out.get(kind.rstrip("s"), 0) + int(n)
    return out


def check_control_blocks(root, folder, plan, python_requested, families):
    """Run the control test block (and the later-edit test) against gold, the later-edit gold and broken
    implementations of the control task, confined, on the pinned environment."""
    import real_repo_gate as gate
    import real_repo_pairs as pairs
    family, v = families.parse_task(CONTROL_TASK)
    implementations = {"gold": v, "numeric-gold": dict(v, gold=CONTROL_NUMERIC_GOLD)}
    implementations.update({"broken-" + n: dict(v, gold=b) for n, b in CONTROL_BROKEN.items()})
    rows = []
    for impl, blocks, expected in CONTROL_BLOCK_CHECKS:
        checkout = folder / "control-blocks" / ("%s-%s" % (impl, blocks))
        gate.create_checkout(root, plan["repo"], plan["base_full"], checkout)
        pairs.frozen_setup(root, checkout, plan, python_requested)
        family["gold"](implementations[impl], checkout)
        (checkout / CONTROL_TEST_PATH).write_text(CONTROL_TESTS + (NUMBER_TEST if blocks == "number" else ""))
        p = gate.confined(root, "control-block-%s-%s-%d" % (impl, blocks, time.time_ns()),
                          [checkout / ".venv/bin/python", "-m", "pytest", "-q", "-p", "no:cacheprovider", CONTROL_TEST_PATH],
                          checkout)
        counts = _counts(p.stdout)
        passed = p.returncode == 0 and counts.get("passed", 0) > 0 and not counts.get("failed") and not counts.get("error")
        rows.append(dict(implementation=impl, blocks=blocks, expected_pass=expected, passed=passed, exit_code=p.returncode,
                         counts=counts, ok=passed == expected, tail="\n".join(p.stdout.strip().splitlines()[-4:])))
        print(json.dumps(dict(control_block=impl, blocks=blocks, passed=passed, ok=passed == expected)), flush=True)
    return dict(rows=rows, valid=all(r["ok"] for r in rows))


def patch(checkout):
    import heldout_tasks
    return heldout_tasks.patch(checkout)


def freeze_material(root, families):
    out = {}
    for task in AUTHORED:
        family, v = families.parse_task(task)
        private = root / "private/tasks" / task
        private.mkdir(parents=True)
        (private / "prompt.txt").write_text(families.prompt(task))
        (private / "hidden.py").write_text(family["hidden"](v))
        out[task] = {name: sha(private / name) for name in ("prompt.txt", "hidden.py")}
    return out


def validate(root, authority):
    import heldout_tasks
    import real_repo_gate as gate
    import real_repo_pairs as pairs
    root, authority = Path(root).resolve(), Path(authority).resolve(strict=True)
    prior = pairs.read(authority / "manifest.json")
    plan = dict(next(p for p in prior["families"] if p["family"] == FAMILY_ID), train=[], heldout=list(AUTHORED))
    root.mkdir(parents=True, exist_ok=False)
    heldout_tasks.copy_substrate(root, authority)
    python = root / Path(prior["preparation_python"]).relative_to(authority)
    families, _, _, _ = gate.modules(root)
    install(families)
    task_sha = freeze_material(root, families)
    manifest = dict(protocol=PROTOCOL, status="offline-validation", root=str(root), substrate_from=str(authority),
                    substrate="copied runtime/python, cache/uv and cache/repobench; absolute links repointed",
                    preparation_python=str(python), python_requested=prior["python_requested"], families=[plan],
                    evaluation=EVALUATION, control_task=CONTROL_TASK, withdrawn=WITHDRAWN, task_sha256=task_sha,
                    source_sha256=sources(),
                    source_revision=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=HERE, text=True).strip(),
                    paid_calls=0)
    gate.write(root / "manifest.json", manifest)
    folder = root / "private/validation" / str(time.time_ns())
    rows, patches = [], {}
    for task in AUTHORED:
        family, v = families.parse_task(task)
        names = BROKEN.get(v["id"], {})
        implementations = [("base", None), ("gold", v)] + [("broken-" + n, broken(v["id"], n)) for n in names]
        arms = {}
        for arm, impl in implementations:
            checkout = folder / task / arm
            gate.create_checkout(root, plan["repo"], plan["base_full"], checkout)
            pairs.frozen_setup(root, checkout, plan, prior["python_requested"])
            if impl is not None:
                family["gold"](impl, checkout)
                (checkout.parent / (arm + ".patch")).write_text(patch(checkout))
                patches.setdefault(task, {})[arm + ".patch"] = sha(checkout.parent / (arm + ".patch"))
            arms[arm] = gate.score(root, task, checkout)
            gate.write(checkout.parent / (arm + ".json"), arms[arm])
            print(json.dumps(dict(task=task, arm=arm, passed=arms[arm]["passed"], n_tests=arms[arm]["n_tests"],
                                  hidden_failed=arms[arm]["hidden_failed"], suite_failed=arms[arm]["suite_failed"])), flush=True)
        verdict = row_verdict(task, arms)
        identities = pairs.prepare_hidden_expectations(root, task)
        verdict["checks"]["hidden_identities_unique_and_pass"] = bool(identities["identities"])
        verdict["hidden_identities"] = len(identities["identities"])
        rows.append(dict(verdict, **{arm: dict(passed=r["passed"], n_tests=r["n_tests"], hidden_failed=r["hidden_failed"],
                                               suite_failed=r["suite_failed"], exit_code=r["exit_code"],
                                               changed_files=r["changed_files"], tail=r["tail"])
                                     for arm, r in arms.items()}))
    blocks = check_control_blocks(root, folder, plan, prior["python_requested"], families)
    report = dict(protocol=PROTOCOL, paid_calls=0, complete=len(rows) == len(AUTHORED),
                  valid=all(r["valid"] for r in rows) and blocks["valid"],
                  evaluation_valid=all(r["valid"] for r in rows if r["task"] in EVALUATION),
                  control_blocks=blocks, rows=rows, patch_sha256=patches, manifest_sha256=sha(root / "manifest.json"),
                  python=subprocess.run([str(python), "--version"], capture_output=True, text=True).stdout.strip())
    gate.write(folder / "report.json", report)
    gate.write(root / "validation.json", report)
    return report


def archive(root, destination, protocol=None):
    """Copy the frozen material (prompts, hidden tests, patches, verdicts) and write the hash manifest."""
    import split_audit
    root, destination = Path(root).resolve(strict=True), Path(destination)
    protocol = protocol or json.loads(split_audit.PROTOCOL.read_text())
    report = json.loads((root / "validation.json").read_text())
    folder = sorted((root / "private/validation").iterdir())[-1]
    tasks = {}
    for task in AUTHORED:
        target = destination / "tasks" / task
        target.mkdir(parents=True, exist_ok=True)
        files = {}
        for name in ("prompt.txt", "hidden.py"):
            shutil.copyfile(root / "private/tasks" / task / name, target / name)
            files[name] = sha(target / name)
        for p in sorted((folder / task).glob("*.patch")):
            shutil.copyfile(p, target / p.name)
            files[p.name] = sha(target / p.name)
        gold = folder / task / "gold"
        vid = task.split("--", 1)[1]
        gold_files = {rel: sha(gold / rel) for rel in ("src/click/types.py", "src/click/__init__.py", "docs/api.md",
                                                       "docs/parameter-types.md", "CHANGES.md",
                                                       "tests/test_types/test_" + vid.replace("-", "_") + ".py")}
        split = "development (safety controls)" if task == CONTROL_TASK else ("withdrawn" if vid in WITHDRAWN else "evaluation")
        tasks[task] = dict(group=GROUPS.get(vid, "enumeration"), split=split, files=files, gold_files_sha256=gold_files,
                           broken=sorted(BROKEN.get(vid, {})), hidden_identities=next(
                               r.get("hidden_identities") for r in report["rows"] if r["task"] == task),
                           valid=next(r["valid"] for r in report["rows"] if r["task"] == task))
    shutil.copyfile(root / "validation.json", destination / "validation.json")
    controls = dict(conditions={c: dict(suffix_sha256=hashlib.sha256(s.encode()).hexdigest(), rerun=r, wider=w)
                                for c, (s, r, w) in CONDITIONS.items()},
                    test_path=CONTROL_TEST_PATH, tests_sha256=hashlib.sha256(CONTROL_TESTS.encode()).hexdigest(),
                    number_test_sha256=hashlib.sha256(NUMBER_TEST.encode()).hexdigest(),
                    natural_order=list(NATURAL_ORDER), failed_order=list(FAILED_ORDER),
                    command_slot=COMMAND_SLOT, blocks_valid=report["control_blocks"]["valid"])
    manifest = dict(protocol=PROTOCOL, frozen_at_utc=time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                    family=FAMILY_ID, evaluation=EVALUATION, control_task=CONTROL_TASK, withdrawn=WITHDRAWN,
                    realism_line=REALISM.strip(),
                    base_commit=json.loads((root / "manifest.json").read_text())["families"][0]["base_full"],
                    python=report["python"], validation_sha256=sha(destination / "validation.json"),
                    validation_root=str(root), source_sha256=sources(), tasks=tasks, controls=controls,
                    expectation=EXPECTATION, near_duplicates=near_duplicates(protocol), paid_calls=0)
    (destination / "manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("command", choices=("validate", "archive", "prompts", "near-duplicates", "name-scan"))
    parser.add_argument("root", type=Path, nargs="?")
    parser.add_argument("--authority", type=Path, action="append", default=[])
    parser.add_argument("--destination", type=Path)
    parser.add_argument("--skip", action="append", default=[])
    args = parser.parse_args()
    if args.command == "prompts":
        for v in VARIANTS:
            print("== %s--%s" % (FAMILY_ID, v["id"]))
            print(prompt(v) + "\n")
        for c in CONDITIONS:
            print("== control %s (command slot unfilled)" % c)
            print(control_prompt(c, COMMAND_SLOT) + "\n")
        return
    if args.command == "near-duplicates":
        import split_audit
        print(json.dumps(near_duplicates(json.loads(split_audit.PROTOCOL.read_text())), indent=2))
        return
    if args.command == "name-scan":
        result = name_scan(args.authority, set(args.skip))
        print(json.dumps(result, indent=2))
        raise SystemExit(0 if result["passed"] else 1)
    if args.command == "validate":
        result = validate(args.root, args.authority[0])
        print(json.dumps({k: result[k] for k in ("protocol", "complete", "valid", "evaluation_valid", "paid_calls", "python")}))
        raise SystemExit(0 if result["valid"] else 1)
    result = archive(args.root, args.destination)
    print(json.dumps(dict(evaluation=result["evaluation"], frozen_at_utc=result["frozen_at_utc"]), indent=2))


if __name__ == "__main__":
    main()
