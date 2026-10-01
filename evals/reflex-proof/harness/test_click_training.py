"""Click training (r13): the guided recipe, recipe binding, the learning step's selection rule and its guards."""
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import click_tasks
import click_training as training
import real_repo_gate as gate
import split_audit

PROTOCOL = json.loads(split_audit.PROTOCOL.read_text())


def route(rid, trigger, command=training.COMMAND, certified=True, effect="verify", track="github-com-pallets-click/learned"):
    evidence = dict(support=10, opportunities=10)
    if certified:
        evidence["execution"] = dict(opportunities=10, sessions=9, successful=9)
    return dict(track=track, route=dict(id=rid, effect=effect, trigger=trigger, step=dict(do="run", command=command),
                                        evidence=evidence))


TESTS_THEN_CHANGES = dict(on="context", within=8, items=[dict(item="edit", glob="tests/**/*.py"),
                                                        dict(item="edit", glob="CHANGES.md")])
SOURCE_LAST = dict(on="context", within=8, items=[dict(item="edit", glob="src/click/*.py"),
                                                 dict(item="edit", glob="tests/**/*.py"), dict(item="edit", glob="CHANGES.md")])


class Recipe(unittest.TestCase):
    def test_ten_guided_sessions_over_the_registered_training_tasks(self):
        rows = training.sessions()
        self.assertEqual(len(rows), 10)
        self.assertEqual(len({r["key"] for r in rows}), 10)
        self.assertEqual([r["task"] for r in rows], training.TASKS * 2)
        self.assertEqual([r["repetition"] for r in rows], [1] * 5 + [2] * 5)
        self.assertEqual(training.TASKS, PROTOCOL["repositories"]["click"]["training_tasks"])
        table = split_audit.splits(PROTOCOL, "click")
        self.assertTrue(all(table[split_audit.variant(t)][0] == "training" for t in training.TASKS))

    def test_training_prompt_is_the_heldout_template_the_realism_line_and_the_instruction(self):
        for task in training.TASKS:
            v = click_tasks.variant(task.split("--", 1)[1])
            text = training.training_prompt(task)
            self.assertEqual(text, click_tasks.prompt(v) + training.GUIDANCE)
            self.assertEqual(text.count("Do not create git commits."), 1)
            self.assertNotIn("adage", text.lower())
        self.assertIn("`" + training.COMMAND + "`", training.GUIDANCE)
        self.assertEqual(training.COMMAND, ".venv/bin/python -m pytest tests/test_types -q")

    def test_the_guided_protocol_is_registered_with_the_gate(self):
        import learning_curve
        self.assertEqual(gate.guided_guidance(training.PROTOCOL), training.GUIDANCE)
        self.assertEqual(gate.guided_guidance("instruction-guided-development-1"), learning_curve.GUIDANCE)
        self.assertIsNone(gate.guided_guidance("unregistered"))

    def test_the_gate_binds_a_click_guided_recipe_to_its_frozen_prompt(self):
        with tempfile.TemporaryDirectory() as d:
            cohort = Path(d)
            task = training.TASKS[0]
            frozen = cohort / "private/tasks" / task / "prompt.txt"
            frozen.parent.mkdir(parents=True)
            frozen.write_text(training.training_prompt(task))
            gate.write(cohort / "manifest.json", dict(protocol=training.PROTOCOL, training_kind=training.GUIDED,
                                                      workflow_guidance=training.GUIDANCE,
                                                      task_sha256={task: {"prompt.txt": gate.sha(frozen)}}))
            value = dict(experiment_root=d, cohort_root=d, kind=training.GUIDED, task=task, workflow_guidance=training.GUIDANCE,
                         prompt_sha256=gate.sha(frozen), argv=[frozen.read_text()])
            gate.validate_training_recipe(value)
            for field, bad in [("kind", "organic-training"), ("argv", ["other"]), ("workflow_guidance", None),
                               ("prompt_sha256", "bad")]:
                with self.subTest(field=field), self.assertRaises(ValueError):
                    gate.validate_training_recipe(dict(value, **{field: bad}))
            gate.write(cohort / "manifest.json", dict(pairs_read(cohort / "manifest.json"), protocol="unregistered"))
            with self.assertRaises(ValueError):
                gate.validate_training_recipe(value)

    def test_session_keys_are_plain_and_the_native_session_keeps_its_own(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            gate.write(root / "manifest.json", dict(families=[dict(train=["click-type--port"])]))
            gate.write(root / "confinement.json", dict(passed=True))
            with patch.object(gate, "modules", return_value=(None, None, None, None)):
                for bad in ("../x", "Guided 1", "/abs"):
                    with self.subTest(key=bad), self.assertRaises(ValueError):
                        gate.prepare_session(root, "click-type--port", "m", key=bad)
                with self.assertRaises(ValueError):
                    gate.prepare_session(root, "native-confinement", "m", native_probe=True, key="guided-1-port")


def pairs_read(path):
    return json.loads(Path(path).read_text())


class Selection(unittest.TestCase):
    def test_globs_follow_the_runtime_rule(self):
        for pattern, path, expected in [("tests/*.py", "tests/test_a.py", True), ("tests/*.py", "tests/test_types/a.py", False),
                                        ("tests/**/*.py", "tests/test_types/a.py", True), ("tests/**/*.py", "tests/a.py", True),
                                        ("CHANGES.md", "CHANGES.md", True), ("docs/*.md", "docs/api.md", True),
                                        ("src/click/*.py", "src/click/types.py", True), ("CHANGES.md", "docs/CHANGES.md", False)]:
            self.assertEqual(training._glob(pattern, path), expected, (pattern, path))

    def test_control_fit(self):
        self.assertTrue(training.control_fit(dict(route=dict(trigger=TESTS_THEN_CHANGES)))["fits"])
        self.assertTrue(training.control_fit(dict(route=dict(trigger=dict(on="after_edit", paths="CHANGES.md"))))["fits"])
        self.assertFalse(training.control_fit(dict(route=dict(trigger=SOURCE_LAST)))["fits"])
        narrow = dict(TESTS_THEN_CHANGES, within=1)
        self.assertFalse(training.control_fit(dict(route=dict(trigger=narrow)))["fits"])
        act = dict(on="context", within=8, items=[dict(item="act", keys=["pytest"]), dict(item="edit", glob="CHANGES.md")])
        self.assertFalse(training.control_fit(dict(route=dict(trigger=act)))["fits"])
        self.assertFalse(training.control_fit(dict(route=dict(trigger=dict(on="intent"))))["fits"])

    def test_the_first_eligible_certified_context_route_is_selected(self):
        index = dict(routes=[
            route("run-000", TESTS_THEN_CHANGES, certified=False),
            route("run-001", SOURCE_LAST),
            route("run-002", TESTS_THEN_CHANGES, command="make test"),
            route("run-003", TESTS_THEN_CHANGES, effect="observe"),
            route("run-004", TESTS_THEN_CHANGES, track="github-com-pallets-click/instincts"),
            route("run-009", TESTS_THEN_CHANGES),
            route("run-005", dict(on="after_edit", paths="CHANGES.md")),
            route("run-006", TESTS_THEN_CHANGES),
        ])
        chosen, verdicts = training.select_route(index)
        self.assertEqual(chosen["route"], "github-com-pallets-click/learned/run-006")
        self.assertEqual([v["route"].rsplit("/", 1)[1] for v in verdicts], ["run-001", "run-002", "run-006", "run-009"])
        self.assertEqual([v["eligible"] for v in verdicts], [False, False, True, True])
        self.assertIsNone(training.select_route(dict(routes=[route("run-001", SOURCE_LAST)]))[0])

    def test_conditioning_refuses_mixed_duplicate_or_synthetic_histories(self):
        row = dict(kind=training.GUIDED, workflow_guidance=training.GUIDANCE, session_id="a", sha256="1", synthetic=False,
                   hooks_enabled=False, source="/dev/null")
        with tempfile.TemporaryDirectory() as d:
            for rows in ([row, dict(row, session_id="b", sha256="2", kind="organic-training", workflow_guidance=None)],
                         [row, dict(row, sha256="2")], [dict(row, synthetic=True)], [dict(row, hooks_enabled=True)]):
                with self.subTest(rows=rows), self.assertRaises(ValueError):
                    training.condition(Path(d) / "out", rows, Path("/bin/sh"), "x")
            self.assertFalse((Path(d) / "out").exists())

    def test_expectation_was_recorded_before_learning(self):
        self.assertEqual(set(training.EXPECTATION), {"organic_replay", "guided", "control_fit", "if_none"})
        spec = PROTOCOL["click_heldout_analysis"]["training"]
        self.assertIn("no certified route", spec["expectation_before_learning"]["organic_replay"])
        self.assertEqual(spec["per_session_cap_usd"], training.SESSION_CAP_USD)
        self.assertIn(training.COMMAND, spec["prompt"])


if __name__ == "__main__":
    unittest.main()
