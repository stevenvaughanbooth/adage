"""Deterministic native-accounting contracts. No provider credentials or API calls."""
import copy
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import evidence


def rows():
    return [json.loads(line) for line in (HERE / "fixtures" / "claude-usage.jsonl").read_text().splitlines()]


def account(events):
    parser = evidence.ClaudeUsage()
    for row in events:
        parser.feed(row)
    return parser.finish()


class Accounting(unittest.TestCase):
    def test_complete_requests_reconcile_without_counting_chunks_or_cached_input_twice(self):
        r = account(rows())
        self.assertTrue(r.complete, r.reasons)
        self.assertEqual([call["id"] for call in r.calls], ["anthropic/req_1", "anthropic/req_2"])
        usage = [c["usage"] for c in r.calls]
        self.assertEqual([u["input_tokens"] for u in usage], [510, 620])
        self.assertEqual([u["output_tokens"] for u in usage], [40, 60])
        self.assertEqual(sum(u["cached_input_tokens"] for u in usage), 1000)
        self.assertEqual(sum(u["model_calls"] for u in usage), 2)
        self.assertTrue(all(u["cost_microusd"] is None for u in usage), "list-price estimates are not billed cost")

    def test_legacy_provisional_assistant_usage_never_becomes_complete_accounting(self):
        r = account([row for row in rows() if row["type"] != "stream_event"])
        self.assertFalse(r.complete)
        self.assertIn("incomplete_request_usage", r.reasons)
        self.assertIn("terminal_usage_mismatch", r.reasons)
        self.assertTrue(all(call["usage"] is None for call in r.calls))

    def test_explicit_completed_messages_are_supported_without_partial_stream_events(self):
        events = [r for r in rows() if r["type"] != "stream_event"]
        for row in events:
            if row["type"] == "assistant":
                message = row["message"]
                message["stop_reason"] = "end_turn"
                message["usage"]["output_tokens"] = 40 if message["id"] == "msg_1" else 60
        self.assertTrue(account(events).complete)
        events[2]["message"]["usage"]["output_tokens"] += 1
        self.assertIn("conflicting_completed_usage", account(events).reasons)

    def test_omitted_requests_hidden_auxiliary_models_and_partial_results_are_incomplete(self):
        for problem in ("request", "terminal", "model", "delta", "stop", "request-id", "session", "counter"):
            with self.subTest(problem=problem):
                events = rows()
                if problem == "request":
                    events = events[:6] + events[-1:]
                elif problem == "terminal":
                    events.pop()
                elif problem == "model":
                    events[-1]["modelUsage"]["untracked-helper-model"] = {}
                elif problem == "delta":
                    events.pop(8)
                elif problem == "stop":
                    events.pop(9)
                elif problem == "request-id":
                    events[7].pop("request_id")
                elif problem == "session":
                    events[7]["session_id"] = "another-session"
                else:
                    events[8]["event"]["usage"]["output_tokens"] = True
                self.assertFalse(account(events).complete)

    def test_reused_provider_requests_and_mutating_message_metadata_are_not_independent_calls(self):
        for problem, expected in (("reuse", "reused_request_id"), ("model", "message_model_changed"),
                                  ("request", "message_request_changed"), ("both", "conflicting_request_id")):
            events = rows()
            if problem == "reuse":
                events[7]["request_id"] = "req_1"
            elif problem == "model":
                events[3]["message"]["model"] = "other-model"
            elif problem == "request":
                events[3]["request_id"] = "different-request"
            else:
                events[3]["requestId"] = "different-request"
            result = account(events)
            self.assertFalse(result.complete)
            self.assertIn(expected, result.reasons)

    def test_conflicting_results_and_provider_events_after_the_terminal_are_visible(self):
        events = rows()
        events.append(copy.deepcopy(events[-1]))
        self.assertTrue(account(events).complete, "identical terminal redelivery is idempotent")
        events[-1]["usage"]["output_tokens"] += 1
        self.assertIn("conflicting_terminal_results", account(events).reasons)
        events = rows() + [rows()[2]]
        self.assertIn("provider_event_after_terminal", account(events).reasons)

    def test_negative_fractional_missing_and_overflowing_counters_do_not_get_coerced(self):
        for bad in (-1, 0.5, True, None, 2**64):
            events = rows()
            events[1]["event"]["message"]["usage"]["input_tokens"] = bad
            self.assertFalse(account(events).complete, repr(bad))
        self.assertIsNone(evidence.canonical_usage(dict(input_tokens=2**64-1, cache_creation_input_tokens=1,
                                                       cache_read_input_tokens=0, output_tokens=0)))

    def test_wrapped_and_raw_streams_agree_but_malformed_or_oversized_logs_stay_incomplete(self):
        with tempfile.TemporaryDirectory() as temp:
            raw, wrapped = Path(temp) / "raw.jsonl", Path(temp) / "wrapped.jsonl"
            raw.write_text("\n".join(json.dumps(row) for row in rows()) + "\n")
            wrapped.write_text("\n".join(json.dumps({"t": 1.0, "line": json.dumps(row)}) for row in rows()) + "\n")
            first, second = evidence.read_stream(raw), evidence.read_stream(wrapped)
            self.assertEqual(first.calls, second.calls)
            self.assertTrue(first.complete and second.complete)
            self.assertNotEqual(first.stream_sha256, second.stream_sha256)
            raw.write_text(raw.read_text() + "bad json\n")
            self.assertIn("malformed_stream_record", evidence.read_stream(raw).reasons)
            with patch.object(evidence, "MAX_STREAM_BYTES", 20):
                result = evidence.read_stream(wrapped)
                self.assertFalse(result.complete)
                self.assertIsNone(result.stream_sha256)
            with patch.object(evidence, "MAX_LINE_BYTES", 20):
                self.assertIn("stream_record_size_limit", evidence.read_stream(wrapped).reasons)

    def test_subagent_accounting_is_explicitly_unsupported_in_this_adapter(self):
        events = rows()
        events[7]["parent_tool_use_id"] = "child-tool"
        result = account(events)
        self.assertFalse(result.complete)
        self.assertIn("subagent_accounting_unsupported", result.reasons)


class Grading(unittest.TestCase):
    def test_only_a_successfully_completed_independent_boolean_grade_is_usable(self):
        self.assertIs(evidence.independent_grade('{"passed":true}', 0), True)
        self.assertIs(evidence.independent_grade('{"passed":false}', 0), False)
        for text, code, timeout in (('{"passed":true}', 1, False), ('{"passed":true}', 0, True),
                                    ('{"passed":"false"}', 0, False), ('{"passed":1}', 0, False),
                                    ('[]', 0, False), ('{}', 0, False), ('broken', 0, False)):
            self.assertIsNone(evidence.independent_grade(text, code, timeout))

    def test_agent_success_cannot_replace_an_independent_grade_and_timeouts_stay_failures(self):
        usage = account(rows())
        r = evidence.receipt("protocol", "job", "run", {}, usage, None)
        self.assertIsNone(r["passed"])
        self.assertEqual(r["status"], "completed")
        r = evidence.receipt("protocol", "job", "run", {}, usage, True, timed_out=True)
        self.assertIs(r["passed"], False)
        self.assertEqual(r["status"], "timeout")
        r = evidence.receipt("protocol", "job", "run", {}, usage, True, exit_code=1)
        self.assertIs(r["passed"], False)
        self.assertEqual(r["status"], "failed")


if __name__ == "__main__":
    unittest.main()
