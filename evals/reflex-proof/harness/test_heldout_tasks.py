"""Held-out Marshmallow tasks: registered split, natural prompts, generated gold/broken behaviour, frozen hashes."""
import ast
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest

import curve_tasks
import heldout_tasks as tasks
import split_audit

PROTOCOL = json.loads(split_audit.PROTOCOL.read_text())
ARCHIVE = Path(__file__).resolve().parent / "results/20261001-heldout-tasks"
STUB = '''from __future__ import annotations


class ValidationError(Exception):
    def __init__(self, message):
        super().__init__(message)
        self.messages = [message]


class Validator:
    error = None

    def __repr__(self):
        args = self._repr_args()
        args = f"{args}, " if args else ""
        return f"<{self.__class__.__name__}({args}error={self.error!r})>"

    def _repr_args(self):
        return ""
'''


def sha_text(text):
    return hashlib.sha256(text.encode()).hexdigest()


def generated(variant):
    """The lab's generated gold class for `variant`, imported from a stub module (no marshmallow needed)."""
    d = Path(tempfile.mkdtemp())
    (d / "src/marshmallow").mkdir(parents=True)
    (d / "tests").mkdir()
    (d / "src/marshmallow/validate.py").write_text(STUB)
    (d / "CHANGELOG.rst").write_text("Changelog\n=========\n\n9.9.9 (unreleased)\n------------------\n\nFeatures:\n\n")
    tasks.FAMILY["gold"](variant, d)
    spec = importlib.util.spec_from_file_location("stub_validate_" + variant["id"].replace("-", "_"),
                                                  d / "src/marshmallow/validate.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module, d


def make(module, variant, **kw):
    cls = getattr(module, variant["cls"])
    return cls(ast.literal_eval(variant["args"]), **kw) if variant["args"] else cls(**kw)


def wrong_cases(module, variant):
    """The private cases the generated class gets wrong, as the hidden test would judge them."""
    wrong = []
    for value, ok in tasks.cases(variant):
        try:
            result = make(module, variant)(value)
            if not ok or result != value:
                wrong.append(value)
        except module.ValidationError as e:
            if ok or e.messages != [variant["formatted"]]:
                wrong.append(value)
        except Exception:  # noqa: BLE001 - any other exception fails the hidden test
            wrong.append(value)
    return wrong


class Registration(unittest.TestCase):
    def test_evaluation_tasks_are_the_registered_evaluation_groups(self):
        table = split_audit.splits(PROTOCOL, "marshmallow")
        spec = PROTOCOL["splits"]["marshmallow"]
        self.assertEqual(sorted(split_audit.variant(t) for t in tasks.EVALUATION),
                         sorted(t for t, (split, _) in table.items() if split == "evaluation"))
        self.assertEqual(PROTOCOL["repositories"]["marshmallow"]["heldout"], tasks.EVALUATION)
        for task in tasks.EVALUATION:
            vid = split_audit.variant(task)
            self.assertEqual(table[vid], ("evaluation", tasks.GROUPS[vid]))
            self.assertEqual(spec["markers"][vid], [tasks.variant(vid)["cls"]])
        self.assertEqual(len(tasks.EVALUATION), 4)

    def test_withdrawn_max_sum_shares_the_size_limit_group_and_is_never_evaluated(self):
        table = split_audit.splits(PROTOCOL, "marshmallow")
        self.assertEqual(table["max-sum"], table["max-words"])
        self.assertEqual(table["max-sum"], ("training", "size-limit"))
        self.assertNotIn("mm-curve--max-sum", tasks.EVALUATION)
        self.assertIn("mm-curve--max-sum", PROTOCOL["repositories"]["marshmallow"]["withdrawn_heldout"])
        self.assertNotIn("MaxSum", split_audit.markers(PROTOCOL, "marshmallow"))

    def test_no_authored_task_reuses_a_training_or_development_name(self):
        earlier = {v["id"] for v in curve_tasks.VARIANTS} | {v["cls"] for v in curve_tasks.VARIANTS}
        self.assertFalse({v["id"] for v in tasks.VARIANTS} & earlier)
        self.assertFalse({v["cls"] for v in tasks.VARIANTS} & earlier)

    def test_install_adds_tasks_without_changing_earlier_tasks(self):
        registry = SimpleNamespace(FAMILIES={})
        tasks.install(registry)
        family = registry.FAMILIES["mm-curve"]
        ids = [v["id"] for v in family["variants"]]
        self.assertEqual(len(ids), len(set(ids)))
        self.assertEqual(ids, [v["id"] for v in curve_tasks.VARIANTS] + [v["id"] for v in tasks.VARIANTS])
        self.assertEqual((len(curve_tasks.TASKS), len(curve_tasks.TRAIN), len(curve_tasks.HELDOUT)), (19, 15, 4))
        for v in curve_tasks.VARIANTS:
            self.assertEqual(family["prompt"](v), curve_tasks.prompt(v))
        for v in tasks.VARIANTS:
            self.assertEqual(family["prompt"](v), tasks.prompt(v))


class Prompts(unittest.TestCase):
    def test_prompts_are_the_earlier_neutral_template_plus_the_realism_line(self):
        for v in tasks.VARIANTS:
            text = tasks.prompt(v)
            self.assertEqual(text, curve_tasks.prompt(v) + "\n\nDo not create git commits.")
            self.assertIn("include tests and a docstring, and add a changelog entry.", text)
            self.assertIn("marshmallow.validate." + v["sig"], text)
            for word in ("pytest", "Adage", "adage", "```", "order", "run ", "verify", "CHANGELOG.rst", "test_validate"):
                self.assertNotIn(word, text)

    def test_only_the_first_example_of_each_kind_is_shown(self):
        for v in tasks.VARIANTS:
            examples = tasks.prompt(v).split("Examples of valid input:", 1)[1].split("\n\n", 1)[0]
            self.assertIn("`" + v["valid"][0] + "`", examples)
            self.assertIn("`" + v["invalid"][0] + "`", examples)
            for hidden in v["valid"][1:] + v["invalid"][1:]:
                self.assertNotIn("`" + hidden + "`", examples)


class Behaviour(unittest.TestCase):
    def test_generated_gold_passes_every_private_case_and_the_hidden_conventions(self):
        for v in tasks.VARIANTS:
            module, d = generated(v)
            self.assertEqual(wrong_cases(module, v), [], v["id"])
            first = ast.literal_eval(v["invalid"][0])
            with self.assertRaises(module.ValidationError) as e:
                make(module, v, error=v["custom"][0])(first)
            self.assertEqual(e.exception.messages, [v["custom"][1]])
            r = repr(make(module, v))
            self.assertTrue(r.startswith("<" + v["cls"] + "("), r)
            self.assertIn(v["msg"], r)
            self.assertIn(v["repr_arg"], r)
            self.assertTrue((getattr(module, v["cls"]).__doc__ or "").strip())
            self.assertIn(v["cls"], (d / "CHANGELOG.rst").read_text())
            self.assertTrue((d / "tests" / ("test_gold_" + v["id"].replace("-", "_") + ".py")).is_file())

    def test_every_broken_implementation_gets_a_private_case_wrong(self):
        for v in tasks.VARIANTS:
            self.assertGreaterEqual(len(tasks.BROKEN[v["id"]]), 2)
            for name in tasks.BROKEN[v["id"]]:
                needs = tasks.VERSION_DEPENDENT.get((v["id"], name))
                if needs and sys.version_info < needs:
                    continue  # covered by the archived validation on the pinned Python 3.12
                broken = tasks.broken(v["id"], name)
                module, _ = generated(broken)
                self.assertTrue(wrong_cases(module, broken), (v["id"], name))

    def test_verdict_requires_base_failure_gold_pass_and_behaviour_only_broken_failures(self):
        base = dict(passed=False, hidden_failed=["test_hidden_docstring"], suite_failed=[], n_tests=10)
        gold = dict(passed=True, hidden_failed=[], suite_failed=[], n_tests=11)
        bad = dict(passed=False, hidden_failed=["test_hidden_invalid[x]"], suite_failed=[], n_tests=11)
        self.assertTrue(tasks.row_verdict("t", dict(base=base, gold=gold, **{"broken-a": bad}))["valid"])
        for broken in (dict(bad, hidden_failed=["test_hidden_docstring"]), dict(bad, passed=True, hidden_failed=[]),
                       dict(bad, suite_failed=["tests/test_validate.py::x"])):
            self.assertFalse(tasks.row_verdict("t", dict(base=base, gold=gold, **{"broken-a": broken}))["valid"])
        self.assertFalse(tasks.row_verdict("t", dict(base=base, gold=dict(gold, n_tests=10)))["valid"])
        self.assertFalse(tasks.row_verdict("t", dict(base=dict(base, suite_failed=["x"]), gold=gold))["valid"])


class Substrate(unittest.TestCase):
    def test_copy_repoints_absolute_links_into_the_copy_and_refuses_foreign_links(self):
        with tempfile.TemporaryDirectory() as tmp:
            authority, root = Path(tmp) / "authority", Path(tmp) / "root"
            full = authority / "runtime/python/cpython-3.12.14"
            full.mkdir(parents=True)
            (full / "python").write_text("x")
            (authority / "runtime/python/cpython-3.12").symlink_to(full)
            (authority / "cache/uv/wheels").mkdir(parents=True)
            (authority / "cache/uv/wheels/rel").symlink_to("../")
            (authority / "cache/repobench").mkdir(parents=True)
            root.mkdir()
            tasks.copy_substrate(root, authority)
            self.assertEqual(Path(os.readlink(root / "runtime/python/cpython-3.12")), root / "runtime/python/cpython-3.12.14")
            self.assertEqual(os.readlink(root / "cache/uv/wheels/rel"), "../")
            self.assertTrue((authority / "runtime/python/cpython-3.12").resolve() == full.resolve())
            (authority / "cache/repobench/escape").symlink_to("/usr/bin")
            other = Path(tmp) / "other"
            other.mkdir()
            with self.assertRaises(ValueError):
                tasks.copy_substrate(other, authority)


class Frozen(unittest.TestCase):
    def setUp(self):
        self.manifest = json.loads((ARCHIVE / "manifest.json").read_text())
        self.validation = json.loads((ARCHIVE / "validation.json").read_text())

    def test_frozen_prompts_and_hidden_tests_match_the_definitions(self):
        self.assertEqual(self.manifest["evaluation"], tasks.EVALUATION)
        self.assertEqual(sorted(self.manifest["tasks"]), sorted(tasks.AUTHORED))
        for task, entry in self.manifest["tasks"].items():
            v = tasks.variant(split_audit.variant(task))
            self.assertEqual(entry["files"]["prompt.txt"], sha_text(tasks.prompt(v)), task)
            self.assertEqual(entry["files"]["hidden.py"], sha_text(tasks.FAMILY["hidden"](v)), task)
            for name, digest in entry["files"].items():
                self.assertEqual(hashlib.sha256((ARCHIVE / "tasks" / task / name).read_bytes()).hexdigest(), digest)
            self.assertEqual(sorted(n[len("broken-"):-len(".patch")] for n in entry["files"] if n.startswith("broken-")),
                             sorted(tasks.BROKEN[v["id"]]))
            self.assertIn("gold.patch", entry["files"])

    def test_archived_validation_passed_on_the_pinned_environment(self):
        self.assertEqual(hashlib.sha256((ARCHIVE / "validation.json").read_bytes()).hexdigest(),
                         self.manifest["validation_sha256"])
        self.assertTrue(self.validation["complete"] and self.validation["valid"])
        self.assertEqual(self.validation["paid_calls"], 0)
        self.assertTrue(self.validation["python"].startswith("Python 3.12"))
        for row in self.validation["rows"]:
            self.assertTrue(row["valid"], row["task"])
            self.assertTrue(row["gold"]["passed"] and not row["base"]["passed"])
            broken = [k for k in row if k.startswith("broken-")]
            self.assertEqual(sorted(b[len("broken-"):] for b in broken), sorted(tasks.BROKEN[split_audit.variant(row["task"])]))
            for b in broken:
                self.assertFalse(row[b]["passed"])
                self.assertTrue(row[b]["hidden_failed"])
                self.assertEqual(row[b]["suite_failed"], [])

    def test_workflow_expectation_was_recorded_for_every_family(self):
        self.assertEqual(self.manifest["expectation"], json.loads(json.dumps(tasks.EXPECTATION)))
        self.assertEqual(set(tasks.EXPECTATION["expected"]), {v["id"] for v in tasks.VARIANTS})


if __name__ == "__main__":
    unittest.main()
