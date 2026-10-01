"""Held-out Click tasks (r13): registered split, neutral prompts, gold and broken behaviour, frozen hashes."""
import hashlib
import json
import re
from pathlib import Path
import textwrap
from types import SimpleNamespace
import unittest

import click_tasks as tasks
import split_audit

PROTOCOL = json.loads(split_audit.PROTOCOL.read_text())
ARCHIVE = Path(__file__).resolve().parent / "results/20261001-click-tasks"


class Fail(Exception):
    pass


def convert(body):
    """The gold class's convert, as plain Python: `fail` raises; non-strings pass through."""
    src = ("def convert(value):\n    def fail(v):\n        raise Fail(v)\n    if not isinstance(value, str):\n"
           "        return value\n" + textwrap.indent(textwrap.dedent(body).strip("\n"), "    "))
    ns = {"Fail": Fail, "re": re}
    exec(src, ns)  # noqa: S102 - the registered gold and broken bodies, nothing else
    return ns["convert"]


def wrong_cases(v, body):
    """The cases the hidden test would judge wrong: a valid value not converted to the expected value, an
    invalid value accepted, or an invalid value failing other than through Click's usage error."""
    fn, wrong = convert(body), []
    for raw, expected in v["valid"]:
        try:
            if fn(raw) != eval(expected, {"re": re}):  # noqa: S307 - the registered expected expressions
                wrong.append(raw)
        except Exception:  # noqa: BLE001
            wrong.append(raw)
    for raw in v["invalid"]:
        try:
            fn(raw)
            wrong.append(raw)
        except Fail:
            pass
        except Exception:  # noqa: BLE001 - not the usage error: the CLI exits 1, not 2
            wrong.append(raw)
    return wrong


def sha_text(text):
    return hashlib.sha256(text.encode()).hexdigest()


class Registration(unittest.TestCase):
    def test_evaluation_tasks_are_the_registered_click_evaluation_groups(self):
        table = split_audit.splits(PROTOCOL, "click")
        spec = PROTOCOL["splits"]["click"]
        self.assertEqual(sorted(split_audit.variant(t) for t in tasks.EVALUATION),
                         sorted(t for t, (split, _) in table.items() if split == "evaluation"))
        self.assertEqual(PROTOCOL["repositories"]["click"]["heldout"], tasks.EVALUATION)
        for task in tasks.EVALUATION:
            vid = split_audit.variant(task)
            self.assertEqual(table[vid], ("evaluation", tasks.GROUPS[vid]))
            self.assertEqual(spec["markers"][vid], [tasks.variant(vid)["cls"]])
        self.assertEqual(len(tasks.EVALUATION), 4)
        self.assertEqual(tasks.WITHDRAWN, {})

    def test_control_task_is_development_and_training_tasks_are_training(self):
        table = split_audit.splits(PROTOCOL, "click")
        self.assertEqual(table[tasks.CONTROL_ID][0], "development")
        self.assertIn(tasks.CONTROL_TASK, PROTOCOL["repositories"]["click"]["development"])
        for vid in tasks.TRAINING_IDS:
            self.assertEqual(table[vid][0], "training")
        self.assertEqual(["click-type--" + v for v in tasks.TRAINING_IDS], PROTOCOL["repositories"]["click"]["training_tasks"])

    def test_no_authored_task_reuses_an_earlier_name(self):
        earlier = list(tasks.fam_python.CLICK["variants"])
        for key in ("id", "name", "cls"):
            self.assertFalse({v[key] for v in tasks.VARIANTS} & {v[key] for v in earlier}, key)

    def test_install_adds_tasks_and_keeps_earlier_prompts(self):
        registry = SimpleNamespace(FAMILIES={})
        tasks.install(registry)
        family = registry.FAMILIES["click-type"]
        ids = [v["id"] for v in family["variants"]]
        self.assertEqual(len(ids), len(set(ids)))
        self.assertEqual(ids[-4:], [v["id"] for v in tasks.VARIANTS])
        for v in tasks.fam_python.CLICK["variants"]:
            expected = tasks.base_prompt(v) if v["id"] == tasks.CONTROL_ID else tasks.LAB.click_prompt(v)
            self.assertEqual(family["prompt"](v), expected)
        for v in tasks.VARIANTS:
            self.assertEqual(family["prompt"](v), tasks.prompt(v))
        self.assertIs(family["hidden"], tasks.LAB.click_hidden)
        self.assertIs(family["gold"], tasks.LAB.click_gold)


class Prompts(unittest.TestCase):
    def test_prompts_are_the_template_plus_the_realism_line_and_nothing_else(self):
        for v in tasks.VARIANTS:
            text = tasks.prompt(v)
            self.assertTrue(text.endswith("\n\nDo not create git commits."))
            self.assertIn("Include tests, document the new type in the API reference and in the parameter types guide, "
                          "and add a changelog entry.", text)
            self.assertIn("`click.%s`" % v["name"], text)
            for word in ("pytest", "Adage", "adage", "```", "order", "run ", "verify", "CHANGES.md", "test_types"):
                self.assertNotIn(word, text)

    def test_only_the_first_example_of_each_kind_is_shown(self):
        for v in tasks.VARIANTS:
            examples = tasks.prompt(v).split("Examples of valid input:", 1)[1].split("\n\n", 1)[0]
            self.assertIn("`" + v["valid"][0][0] + "`", examples)
            self.assertIn("`" + v["invalid"][0] + "`", examples)
            for raw, _ in v["valid"][1:]:
                self.assertNotIn("`" + raw + "`", examples)
            for raw in v["invalid"][1:]:
                self.assertNotIn("`" + raw + "`", examples)

    def test_control_prompts_carry_the_realism_line_the_work_order_and_the_test_block(self):
        command = ".venv/bin/python -m pytest tests/test_types -q"
        for condition, (suffix, rerun, wider) in tasks.CONDITIONS.items():
            text = tasks.control_prompt(condition, command)
            self.assertTrue(text.startswith(tasks.base_prompt(tasks.variant(tasks.CONTROL_ID))))
            self.assertEqual(text.count("Do not create git commits."), 1)
            self.assertLess(text.index("Do not create git commits."), text.index("Work in this order"))
            self.assertIn(tasks.TESTS, text)
            self.assertNotIn(tasks.COMMAND_SLOT, text)
            self.assertEqual(command in text, condition == "rerun")
            self.assertEqual(rerun, condition == "rerun")
            self.assertEqual(wider, [[], ["tests"]] if condition == "broader" else None)
        self.assertEqual(sorted(tasks.CONDITIONS), sorted(("failed", "later-edit", "config-change", "broader", "rerun")))
        self.assertEqual(tasks.FAILED_ORDER[-2:], ("src/click/types.py", "src/click/__init__.py"))
        self.assertEqual(sorted(tasks.FAILED_ORDER), sorted(tasks.NATURAL_ORDER))


class Behaviour(unittest.TestCase):
    def test_gold_converts_every_private_case(self):
        for v in tasks.VARIANTS + [tasks.variant(tasks.CONTROL_ID)]:
            self.assertEqual(wrong_cases(v, v["gold"]), [], v["id"])
            self.assertGreaterEqual(len(v["valid"]), 2)
            self.assertGreaterEqual(len(v["invalid"]), 2)

    def test_every_broken_implementation_gets_a_case_wrong_but_converts_the_first_example(self):
        for v in tasks.VARIANTS:
            self.assertGreaterEqual(len(tasks.BROKEN[v["id"]]), 3)
            for name, body in tasks.BROKEN[v["id"]].items():
                self.assertTrue(wrong_cases(v, body), (v["id"], name))
                self.assertIsNotNone(convert(body)(v["valid"][0][0]), (v["id"], name))  # the gold's own test still passes

    def test_control_implementations_are_told_apart_by_the_blocks(self):
        v = tasks.variant(tasks.CONTROL_ID)
        self.assertEqual(wrong_cases(v, tasks.CONTROL_NUMERIC_GOLD), [])
        self.assertEqual(convert(tasks.CONTROL_NUMERIC_GOLD)("20"), 20)
        with self.assertRaises(Fail):
            convert(v["gold"])("20")
        for name, body in tasks.CONTROL_BROKEN.items():
            self.assertTrue(wrong_cases(v, body), name)

    def test_verdict_requires_base_failure_gold_pass_and_behaviour_only_broken_failures(self):
        base = dict(passed=False, hidden_failed=["test_documented"], suite_failed=[], n_tests=10)
        gold = dict(passed=True, hidden_failed=[], suite_failed=[], n_tests=11)
        bad = dict(passed=False, hidden_failed=["test_invalid[x]"], suite_failed=[], n_tests=11)
        self.assertTrue(tasks.row_verdict("t", dict(base=base, gold=gold, **{"broken-a": bad}))["valid"])
        for broken in (dict(bad, hidden_failed=["test_changelog"]), dict(bad, passed=True, hidden_failed=[]),
                       dict(bad, suite_failed=["tests/test_imports.py::test_light_imports"])):
            self.assertFalse(tasks.row_verdict("t", dict(base=base, gold=gold, **{"broken-a": broken}))["valid"])
        self.assertFalse(tasks.row_verdict("t", dict(base=base, gold=dict(gold, n_tests=10)))["valid"])


class Frozen(unittest.TestCase):
    def setUp(self):
        self.manifest = json.loads((ARCHIVE / "manifest.json").read_text())
        self.validation = json.loads((ARCHIVE / "validation.json").read_text())

    def test_frozen_prompts_and_hidden_tests_match_the_definitions(self):
        self.assertEqual(self.manifest["evaluation"], tasks.EVALUATION)
        self.assertEqual(sorted(self.manifest["tasks"]), sorted(tasks.AUTHORED))
        for task, entry in self.manifest["tasks"].items():
            v = tasks.variant(split_audit.variant(task))
            prompt = tasks.prompt(v) if task in tasks.EVALUATION else tasks.base_prompt(v)
            self.assertEqual(entry["files"]["prompt.txt"], sha_text(prompt), task)
            self.assertEqual(entry["files"]["hidden.py"], sha_text(tasks.LAB.click_hidden(v)), task)
            for name, digest in entry["files"].items():
                self.assertEqual(hashlib.sha256((ARCHIVE / "tasks" / task / name).read_bytes()).hexdigest(), digest)
            self.assertEqual(sorted(n[len("broken-"):-len(".patch")] for n in entry["files"] if n.startswith("broken-")),
                             sorted(tasks.BROKEN.get(v["id"], {})))
            self.assertIn("gold.patch", entry["files"])

    def test_archived_validation_passed_on_the_pinned_environment(self):
        self.assertEqual(hashlib.sha256((ARCHIVE / "validation.json").read_bytes()).hexdigest(),
                         self.manifest["validation_sha256"])
        self.assertTrue(self.validation["complete"] and self.validation["valid"] and self.validation["evaluation_valid"])
        self.assertEqual(self.validation["paid_calls"], 0)
        self.assertTrue(self.validation["python"].startswith("Python 3.12"))
        for row in self.validation["rows"]:
            self.assertTrue(row["valid"], row["task"])
            self.assertTrue(row["gold"]["passed"] and not row["base"]["passed"])
            self.assertGreater(row["hidden_identities"], 0)
            broken = [k for k in row if k.startswith("broken-")]
            self.assertEqual(sorted(b[len("broken-"):] for b in broken),
                             sorted(tasks.BROKEN.get(split_audit.variant(row["task"]), {})))
            for b in broken:
                self.assertFalse(row[b]["passed"])
                self.assertTrue(row[b]["hidden_failed"])
                self.assertEqual(row[b]["suite_failed"], [])
        blocks = self.validation["control_blocks"]
        self.assertTrue(blocks["valid"])
        self.assertEqual([(r["implementation"], r["blocks"], r["expected_pass"]) for r in blocks["rows"]],
                         [tuple(c) for c in tasks.CONTROL_BLOCK_CHECKS])

    def test_screens_and_expectation_were_recorded(self):
        self.assertEqual(self.manifest["expectation"], json.loads(json.dumps(tasks.EXPECTATION)))
        self.assertEqual(set(tasks.EXPECTATION["expected"]), {v["id"] for v in tasks.VARIANTS})
        self.assertEqual(set(self.manifest["near_duplicates"]["nearest_earlier_tasks"]), {v["id"] for v in tasks.VARIANTS})
        scan = json.loads((ARCHIVE / "name-scan.json").read_text())
        self.assertTrue(scan["passed"])
        self.assertGreater(scan["scanned_files"], 500)
        controls = self.manifest["controls"]
        self.assertEqual(controls["tests_sha256"], sha_text(tasks.CONTROL_TESTS))
        for condition, (suffix, _, _) in tasks.CONDITIONS.items():
            self.assertEqual(controls["conditions"][condition]["suffix_sha256"], sha_text(suffix))

    def test_protocol_registers_this_manifest(self):
        spec = PROTOCOL["click_heldout_analysis"]
        self.assertEqual(spec["task_manifest_sha256"], hashlib.sha256((ARCHIVE / "manifest.json").read_bytes()).hexdigest())
        self.assertEqual(spec["tasks"], tasks.EVALUATION)
        self.assertEqual(spec["task_validation_sha256"], self.manifest["validation_sha256"])


if __name__ == "__main__":
    unittest.main()
