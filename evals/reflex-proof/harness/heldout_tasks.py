#!/usr/bin/env python3
"""Held-out Marshmallow tasks for launch-gate-1: authored, frozen and validated offline. No paid calls.

The tasks follow the earlier mm-curve tasks exactly: the same variant schema, the same prompt
(`curve_tasks.prompt`: the lab template showing only the first valid and first invalid example),
the same hidden test (the lab `mm_hidden`, scored by the unchanged showcase scorer) and the same
generated gold (the lab `mm_gold`). The only addition is one realism line, `Do not create git
commits.`, decided for every held-out arm before any development canary-v8 result was read.
Prompts carry no work order, no test block and no verification instruction; agents write their
own tests.

`curve_tasks.py` stays byte-identical, so TRAIN/HELDOUT, the nineteen validated controls and the
source hashes of frozen development roots keep their meaning. A process that needs these tasks
calls `install()` (it installs curve_tasks first). Each task also carries broken implementations
(another `check` expression in the same generated class) that validation requires to fail.

`validate ROOT --authority A` copies the authority's pinned runtime, uv cache and git mirror into
ROOT (never links them, so nothing is written into the authority), then scores base, gold and
every broken implementation per task with the existing confined scorer, and freezes prompts,
hidden tests and gold/broken patches with their hashes.
"""
from __future__ import annotations

import argparse
import ast
import json
import os
from pathlib import Path
import shutil
import subprocess
import time

import curve_tasks

HERE = Path(__file__).resolve().parent
PROTOCOL = "heldout-tasks-1"
REALISM = "\n\nDo not create git commits."
BEHAVIOR_TESTS = ("test_hidden_valid", "test_hidden_invalid", "test_hidden_custom_error")


def _src(values):
    return [repr(v) for v in values]


HAS_KEYS = dict(
    id="has-keys", cls="HasKeys", sig="HasKeys(keys, *, error=None)", args="['id', 'name']",
    desc="which succeeds if the input is a mapping (such as a `dict`) that contains every key in `keys`; "
         "extra keys are allowed, and input that is not a mapping fails.",
    msg="Missing required keys.", keys=["input", "keys"], formatted="Missing required keys.",
    # The container type `keys` is stored as is not specified, so the repr check names the argument only.
    repr_arg="keys=",
    valid=["{'id': 1, 'name': 'x'}", "{'id': None, 'name': '', 'extra': 0}"],
    invalid=["{'id': 1}", "{}", "['id', 'name']", "'id name'"],
    custom=("{input} is missing keys", "{'id': 1} is missing keys"), init=["keys"],
    check="isinstance(value, __import__('collections.abc').abc.Mapping) and all(k in value for k in self.keys)")

ISO_DATE = curve_tasks.simple(
    "iso-date", "IsoDate",
    "which accepts strings written exactly as `YYYY-MM-DD` (a four-digit year, two-digit month and two-digit day) "
    "that name a real calendar date; other forms such as `20240105`, `2024-1-05` or a date with a time fail.",
    _src(["2024-02-29", "1999-12-31", "2000-02-29"]),
    _src(["2023-02-29", "2024-13-01", "2024-1-05", "20240105", "2024-01-05T00:00"]),
    "__import__('re').fullmatch('[0-9]{4}-[0-9]{2}-[0-9]{2}', value) is not None "
    "and bool(__import__('datetime').date.fromisoformat(value))")

JSON_OBJECT = curve_tasks.simple(
    "json-object", "JsonObject",
    "which accepts strings holding a JSON document whose top-level value is an object, as parsed by `json.loads` "
    "(surrounding whitespace is allowed); arrays, other JSON values and malformed JSON fail.",
    _src(['{"a": 1}', "{}", '{"items": [1, 2], "meta": {"ok": true, "next": null}}', '  {"a": 1}  ']),
    _src(["[1, 2]", "null", '"{}"', "{'a': 1}", '{"a": 1', ""]),
    "isinstance(__import__('json').loads(value), dict)")

LAT_LONG = curve_tasks.simple(
    "lat-long", "LatLong",
    "which accepts a `(latitude, longitude)` pair: a sequence of exactly two real numbers, the latitude between "
    "-90 and 90 and the longitude between -180 and 180, bounds included.",
    ["(0, 0)", "[51.5, -0.12]", "(-90, 180)", "(90.0, -180.0)"],
    ["(91, 0)", "(0, 180.5)", "(10, 20, 30)", "(10,)"],
    "len(value) == 2 and -90 <= value[0] <= 90 and -180 <= value[1] <= 180")

MAX_SUM = dict(
    id="max-sum", cls="MaxSum", sig="MaxSum(max, *, error=None)", args="10",
    desc="which succeeds if the input is a sequence of numbers whose sum is at most `max` (an empty sequence sums to 0).",
    msg="Sum must be at most {max}.", keys=["input", "max"], formatted="Sum must be at most 10.",
    repr_arg="max=10", valid=["[1, 2, 3]", "[]", "[2.5, 7.5]", "[-5, 12]"], invalid=["[5, 6]", "[10.5]", "[4, 4, 4]"],
    custom=("{input} sums to more than {max}", "[5, 6] sums to more than 10"), init=["max"],
    check="sum(value) <= self.max")

# Every authored variant, in a fixed order. Evaluation uses EVALUATION only.
VARIANTS = [HAS_KEYS, ISO_DATE, JSON_OBJECT, MAX_SUM, LAT_LONG]
GROUPS = {"has-keys": "mapping-keys", "iso-date": "temporal-format", "json-object": "structured-text",
          "lat-long": "geo-coordinate", "max-sum": "size-limit"}
# max-sum was registered (revision 2) as its own group. Authoring showed it has the size-limit group's
# interface and predicate shape, the reason max-utf8-bytes is grouped with max-words, so it is withdrawn
# from evaluation and recorded in that training group; it has never run and is never dispatched.
WITHDRAWN = {"max-sum": "near-duplicate of the size-limit training group (max-words, max-utf8-bytes): the same "
                        "`(max, *, error=None)` interface and a one-line measure-at-most-max predicate; replaced "
                        "by lat-long"}
EVALUATION = ["mm-curve--" + v["id"] for v in VARIANTS if v["id"] not in WITHDRAWN]
AUTHORED = ["mm-curve--" + v["id"] for v in VARIANTS]
IDS = {v["id"] for v in VARIANTS}

# Plausible wrong implementations: the same generated class with another check. Each must fail the
# hidden behaviour tests and nothing else. "fromisoformat" relies on Python >= 3.11 accepting `20240105`
# (the pinned environment is 3.12).
BROKEN = {
    "has-keys": {
        "no-mapping-check": "all(k in value for k in self.keys)",
        "any-key": "isinstance(value, dict) and any(k in value for k in self.keys)",
        "truthy-values": "isinstance(value, dict) and all(value.get(k) for k in self.keys)",
    },
    "iso-date": {
        "fromisoformat-only": "bool(__import__('datetime').date.fromisoformat(value))",
        "strptime": "bool(__import__('datetime').datetime.strptime(value, '%Y-%m-%d'))",
        "pattern-only": "__import__('re').fullmatch('[0-9]{4}-[0-9]{2}-[0-9]{2}', value) is not None",
    },
    "json-object": {
        "any-json": "__import__('json').loads(value) is not None",
        "brace-sniffing": "value.strip().startswith('{') and value.strip().endswith('}')",
        "no-leading-whitespace": "value.startswith('{') and isinstance(__import__('json').loads(value), dict)",
    },
    "lat-long": {
        "swapped": "len(value) == 2 and -180 <= value[0] <= 180 and -90 <= value[1] <= 90",
        "exclusive-bounds": "len(value) == 2 and -90 < value[0] < 90 and -180 < value[1] < 180",
        "no-length-check": "-90 <= value[0] <= 90 and -180 <= value[1] <= 180",
    },
    "max-sum": {
        "strict": "sum(value) < self.max",
        "per-element": "all(v <= self.max for v in value)",
        "count": "len(value) <= self.max",
    },
}
VERSION_DEPENDENT = {("iso-date", "fromisoformat-only"): (3, 11)}
# Recorded before any run (protocol item): does the natural workflow edit tests/*.py then CHANGELOG.rst
# within eight events, the learned context of run-175d85? The prompt template, deliverables and files are
# those of every earlier mm-curve task; see the archived base-rate scan. Not tuned to trigger the reflex.
EXPECTATION = {
    "context": "edit tests/*.py, then CHANGELOG.rst within 8 events (run-175d85)",
    "expected": {vid: "fires in most sessions" for vid in IDS},
    "basis": "same prompt template and deliverables as the 31 earlier unguided Marshmallow sessions, of which 27 "
             "produced the context and all 31 edited validate.py, then tests, then CHANGELOG.rst",
    "family_specific_risk": {
        "has-keys": "none known",
        "iso-date": "more own-test iteration (calendar cases) may separate the tests and changelog edits",
        "json-object": "more own-test iteration (whitespace and non-object cases) may separate the edits",
        "lat-long": "none known",
        "max-sum": "withdrawn",
    },
}


def prompt(variant):
    """The earlier mm-curve prompt, unchanged, plus the realism line."""
    return curve_tasks.prompt(variant) + REALISM


def _family_prompt(variant):
    return prompt(variant) if variant["id"] in IDS else curve_tasks.prompt(variant)


FAMILY = dict(curve_tasks.FAMILY, variants=curve_tasks.VARIANTS + VARIANTS, prompt=_family_prompt)


def install(registry):
    """Register the earlier mm-curve tasks and these, under the one `mm-curve` family."""
    curve_tasks.install(registry)
    registry.FAMILIES[FAMILY["id"]] = FAMILY


def variant(vid):
    return next(v for v in VARIANTS if v["id"] == vid)


def broken(vid, name):
    return dict(variant(vid), check=BROKEN[vid][name])


def cases(v):
    """(value, should_pass) for every private boundary case, as Python values."""
    return [(ast.literal_eval(x), True) for x in v["valid"]] + [(ast.literal_eval(x), False) for x in v["invalid"]]


def sources():
    import real_repo_pairs as pairs
    lab = HERE.parents[1] / "lab/research/recurring-workflows/families.py"
    return {str(p.relative_to(HERE.parents[2])): pairs.sha(p) for p in (
        Path(__file__), HERE / "curve_tasks.py", HERE.parents[1] / "showcase/fam_python.py", lab,
        HERE.parents[1] / "showcase/sc_score.py", HERE / "real_repo_gate.py", HERE / "real_repo_pairs.py")}


def copy_substrate(root, authority):
    """Copy the pinned runtime, uv cache and mirror; repoint absolute links into the copy."""
    for relative in ("runtime/python", "cache/uv", "cache/repobench"):
        shutil.copytree(authority / relative, root / relative, symlinks=True)
    for directory, dirs, files in os.walk(root):
        for name in dirs + files:
            path = Path(directory) / name
            if path.is_symlink() and os.path.isabs(os.readlink(path)):
                target = Path(os.readlink(path))
                if authority not in target.parents:
                    raise ValueError("unexpected absolute link in the substrate: %s -> %s" % (path, target))
                path.unlink()
                path.symlink_to(root / target.relative_to(authority))


def patch(checkout):
    """The implementation as a git patch against the base commit (tracked changes and new files)."""
    def git(*args, ok=(0,)):
        p = subprocess.run(["git", "-C", str(checkout), "--no-optional-locks", *args], capture_output=True, text=True,
                           env=dict(os.environ, GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL="/dev/null"))
        if p.returncode not in ok:
            raise RuntimeError(p.stderr)
        return p.stdout
    status = git("status", "--porcelain", "-uall")
    new = sorted(line[3:] for line in status.splitlines() if line.startswith("??") and not line[3:].startswith(".venv/"))
    text = git("diff", "--no-color", "HEAD")
    for name in new:
        text += git("diff", "--no-color", "--no-index", "/dev/null", name, ok=(0, 1))
    return text


def freeze_material(root, families):
    out = {}
    for task in AUTHORED:
        family, v = families.parse_task(task)
        private = root / "private/tasks" / task
        private.mkdir(parents=True)
        (private / "prompt.txt").write_text(families.prompt(task))
        (private / "hidden.py").write_text(family["hidden"](v))
        out[task] = {name: _sha(private / name) for name in ("prompt.txt", "hidden.py")}
    return out


def _sha(path):
    import hashlib
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


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


def validate(root, authority):
    import real_repo_gate as gate
    import real_repo_pairs as pairs
    root, authority = Path(root).resolve(), Path(authority).resolve(strict=True)
    prior = pairs.read(authority / "manifest.json")
    plan = dict(next(p for p in prior["families"] if p["family"] == "mm-validator"), family="mm-curve",
                train=[], heldout=list(AUTHORED))
    root.mkdir(parents=True, exist_ok=False)
    copy_substrate(root, authority)
    python = root / Path(prior["preparation_python"]).relative_to(authority)
    families, _, _, _ = gate.modules(root)
    install(families)
    task_sha = freeze_material(root, families)
    manifest = dict(protocol=PROTOCOL, status="offline-validation", root=str(root), substrate_from=str(authority),
                    substrate="copied runtime/python, cache/uv and cache/repobench; absolute links repointed",
                    preparation_python=str(python), python_requested=prior["python_requested"], families=[plan],
                    evaluation=EVALUATION, withdrawn=WITHDRAWN, task_sha256=task_sha, source_sha256=sources(),
                    source_revision=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=HERE, text=True).strip(),
                    paid_calls=0)
    gate.write(root / "manifest.json", manifest)
    folder = root / "private/validation" / str(time.time_ns())
    rows, patches = [], {}
    for task in AUTHORED:
        family, v = families.parse_task(task)
        implementations = [("base", None), ("gold", v)] + [("broken-" + n, broken(v["id"], n)) for n in BROKEN[v["id"]]]
        arms = {}
        for arm, impl in implementations:
            checkout = folder / task / arm
            gate.create_checkout(root, plan["repo"], plan["base_full"], checkout)
            pairs.frozen_setup(root, checkout, plan, prior["python_requested"])
            if impl is not None:
                family["gold"](impl, checkout)
                (checkout.parent / (arm + ".patch")).write_text(patch(checkout))
                patches.setdefault(task, {})[arm + ".patch"] = _sha(checkout.parent / (arm + ".patch"))
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
    report = dict(protocol=PROTOCOL, paid_calls=0, complete=len(rows) == len(AUTHORED),
                  valid=all(r["valid"] for r in rows), evaluation_valid=all(r["valid"] for r in rows if r["task"] in EVALUATION),
                  rows=rows, patch_sha256=patches, manifest_sha256=_sha(root / "manifest.json"),
                  python=subprocess.run([str(python), "--version"], capture_output=True, text=True).stdout.strip())
    gate.write(folder / "report.json", report)
    gate.write(root / "validation.json", report)
    return report


def archive(root, destination):
    """Copy the frozen material (prompts, hidden tests, patches, verdicts) and write the hash manifest."""
    root, destination = Path(root).resolve(strict=True), Path(destination)
    report = json.loads((root / "validation.json").read_text())
    folder = sorted((root / "private/validation").iterdir())[-1]
    tasks = {}
    for task in AUTHORED:
        target = destination / "tasks" / task
        target.mkdir(parents=True, exist_ok=True)
        files = {}
        for name in ("prompt.txt", "hidden.py"):
            shutil.copyfile(root / "private/tasks" / task / name, target / name)
            files[name] = _sha(target / name)
        for p in sorted((folder / task).glob("*.patch")):
            shutil.copyfile(p, target / p.name)
            files[p.name] = _sha(target / p.name)
        gold = folder / task / "gold"
        vid = task.split("--", 1)[1]
        gold_files = {rel: _sha(gold / rel) for rel in ("src/marshmallow/validate.py", "CHANGELOG.rst",
                                                        "tests/test_gold_" + vid.replace("-", "_") + ".py")}
        tasks[task] = dict(group=GROUPS[vid], split="withdrawn" if vid in WITHDRAWN else "evaluation", files=files,
                           gold_files_sha256=gold_files, broken=sorted(BROKEN[vid]),
                           valid=next(r["valid"] for r in report["rows"] if r["task"] == task))
    shutil.copyfile(root / "validation.json", destination / "validation.json")
    manifest = dict(protocol=PROTOCOL, frozen_at_utc=time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                    evaluation=EVALUATION, withdrawn=WITHDRAWN, realism_line=REALISM.strip(),
                    base_commit=json.loads((root / "manifest.json").read_text())["families"][0]["base_full"],
                    python=report["python"], validation_sha256=_sha(destination / "validation.json"),
                    validation_root=str(root), source_sha256=sources(), tasks=tasks, expectation=EXPECTATION,
                    paid_calls=0)
    (destination / "manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("command", choices=("validate", "archive", "prompts"))
    parser.add_argument("root", type=Path, nargs="?")
    parser.add_argument("--authority", type=Path)
    parser.add_argument("--destination", type=Path)
    args = parser.parse_args()
    if args.command == "prompts":
        for v in VARIANTS:
            print("== mm-curve--" + v["id"] + ("  (withdrawn)" if v["id"] in WITHDRAWN else ""))
            print(prompt(v) + "\n")
        return
    if args.command == "validate":
        result = validate(args.root, args.authority)
        print(json.dumps({k: result[k] for k in ("protocol", "complete", "valid", "evaluation_valid", "paid_calls", "python")}))
        raise SystemExit(0 if result["valid"] else 1)
    result = archive(args.root, args.destination)
    print(json.dumps(dict(evaluation=result["evaluation"], frozen_at_utc=result["frozen_at_utc"]), indent=2))


if __name__ == "__main__":
    main()
