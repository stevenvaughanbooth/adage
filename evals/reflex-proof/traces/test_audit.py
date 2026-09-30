"""Contract tests for source fidelity and conservative temporal availability."""
import json
from pathlib import Path
import tempfile
import unittest

from audit import (Call, Prompt, Trace, audit, binding_audit, load_trace, precedes,
                   shape, simple_argv, split)


class AuditTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.path = Path(self.tmp.name) / "source.jsonl"

    def load(self, rows, schema="claude"):
        self.path.write_text("\n".join(json.dumps(x) for x in rows))
        return load_trace(self.path, dataset="test", kind="fixture", schema=schema,
                          source_id="source.jsonl", task_group="repo:task")

    def test_original_args_results_unknown_fields_and_all_prompts(self):
        rows = [
            {"type": "user", "message": {"content": "First request"}},
            {"type": "assistant", "uuid": "event-a", "future_extension": {"x": 3},
             "message": {"id": "batch-a", "model": "m", "content": [
                 {"type": "tool_use", "id": "call-a", "name": "mcp__any__new_tool",
                  "input": {"nested": ["a b", {"unknown": False}]}}]}},
            {"type": "user", "message": {"content": [
                {"type": "tool_result", "tool_use_id": "call-a", "content": [{"text": "raw result"}], "is_error": False}]}},
            {"type": "user", "message": {"content": "Changed request"}},
        ]
        trace = self.load(rows)
        self.assertEqual([p.content for p in trace.prompts], ["First request", "Changed request"])
        self.assertEqual(trace.calls[0].original_args, rows[1]["message"]["content"][0]["input"])
        self.assertEqual(trace.calls[0].output, [{"text": "raw result"}])
        self.assertEqual(trace.original("/1")["future_extension"], {"x": 3})
        self.assertEqual(trace.calls[0].event_id, "event-a")
        self.assertIsNone(trace.calls[0].exit_code)  # false error flag is not numeric exit 0

    def test_partial_order_does_not_linearize_same_batch(self):
        a = Call(1, "A", {}, {}, "", batch="same", result_order=2)
        b = Call(3, "B", {}, {}, "", batch="same")
        self.assertFalse(precedes(a, b))
        b.batch = "later"
        self.assertTrue(precedes(a, b))
        a.result_order = 4
        self.assertFalse(precedes(a, b))

    def test_original_shell_arguments_do_not_become_templates(self):
        call = Call(1, "Bash", {"command": "python3 scripts/view 'a b.txt'"}, {}, "")
        self.assertEqual(simple_argv(call), ["python3", "scripts/view", "a b.txt"])
        call.args["command"] = "cat x && rm y"
        self.assertIsNone(simple_argv(call))
        call.args["command"] = "cat $(secret)"
        self.assertIsNone(simple_argv(call))
        call.args["command"] = ["program", "literal;not-shell"]
        self.assertEqual(simple_argv(call), ["program", "literal;not-shell"])

    def test_interpreter_script_identity_preserved(self):
        self.assertNotEqual(shape(["python3", "scripts/a", "W-1"]), shape(["python3", "scripts/b", "W-1"]))
        self.assertEqual(shape(["python3", "scripts/a", "W-1"]), shape(["python3", "scripts/a", "W-2"]))

    def test_no_hindsight_and_multiple_source_ambiguity(self):
        current = Call(3, "Bash", {"command": "lookup W-7"}, {}, "", batch="b")
        future = Call(4, "Bash", {}, {}, "", output="W-7", result_order=5, batch="c")
        trace = Trace("d", "k", "s", "x", self.path, "hash", "task", calls=[current, future])
        trace.prompts = [Prompt(6, "W-7")]
        self.assertEqual(binding_audit(trace, current, ["lookup", "W-7"])["unavailable_before_call"], 1)
        trace.prompts = [Prompt(0, "W-7")]
        self.assertEqual(binding_audit(trace, current, ["lookup", "W-7"])["unique_prior_prompt"], 1)
        prior = Call(1, "Bash", {}, {}, "", output="W-7", result_order=2, batch="a")
        trace.calls.insert(0, prior)
        self.assertEqual(binding_audit(trace, current, ["lookup", "W-7"])["ambiguous_multiple_prior_sources"], 1)

    def test_codex_keeps_original_json_and_call_cwd(self):
        args = '{ "cmd": "pwd", "workdir": "/repo/sub", "env": {"X": "v"} }'
        trace = self.load([
            {"type": "session_meta", "payload": {"id": "s", "cwd": "/repo"}},
            {"type": "turn_context", "payload": {"model": "model-x", "cwd": "/repo"}},
            {"type": "response_item", "payload": {"type": "function_call", "name": "exec_command", "call_id": "c", "arguments": args}},
            {"type": "response_item", "payload": {"type": "function_call_output", "call_id": "c", "output": "Process exited with code 2\nOutput:\nx"}},
        ], "codex")
        call = trace.calls[0]
        self.assertEqual(call.original_args, args)
        self.assertEqual(call.cwd, "/repo/sub")
        self.assertEqual(call.env, {"X": "v"})
        self.assertEqual(call.exit_code, 2)
        self.assertIsNone(call.request_id)

    def test_ambiguous_duplicate_call_ids_are_not_paired(self):
        trace = self.load([
            {"type": "assistant", "message": {"content": [
                {"type": "tool_use", "id": "dup", "name": "A", "input": {}},
                {"type": "tool_use", "id": "dup", "name": "B", "input": {}}]}},
            {"type": "user", "message": {"content": [
                {"type": "tool_result", "tool_use_id": "dup", "content": "ambiguous"}]}}
        ])
        self.assertTrue(all(c.result_ref is None and c.link_ambiguous for c in trace.calls))
        self.assertEqual(trace.notices["unmatched_or_ambiguous_result"], 1)

    def test_task_group_split_keeps_models_harnesses_and_repeats_together(self):
        group = "swebench:django__django-1"
        samples = [Trace(f"model-{i}", "public", "h", str(i), self.path, str(i), group) for i in range(6)]
        self.assertEqual(len({split(t.task_group) for t in samples}), 1)
        self.assertEqual(split(group), split(group))

    def test_unknown_task_is_quarantined_not_hashed_into_training(self):
        self.assertEqual(split("unknown:any-checksum"), "quarantine")

    def test_recurrence_training_never_uses_heldout_or_steered_support(self):
        groups = {name: next(f"task-{i}" for i in range(100) if split(f"task-{i}") == name)
                  for name in ("train", "test")}
        ts = []
        for i, (group, origin) in enumerate([(groups["train"], "unsteered"), (groups["test"], "unsteered"),
                                             (groups["train"] + "other", "steered")]):
            ts.append(Trace("d", "k", "h", str(i), self.path, str(i), group,
                calls=[Call(1, "Bash", {"command": "lookup W-7"}, {}, "")], provenance=origin))
        self.assertEqual(audit(ts)["d"]["train_shapes_seen_in_two_distinct_tasks"], 0)


if __name__ == "__main__":
    unittest.main()
