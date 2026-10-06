"""Native Claude Code accounting for frozen experiments. No model calls and no price estimates.

Use --include-partial-messages: ordinary assistant chunks can report output_tokens=1 for an
entire response. Only completed request usage reconciled to the terminal totals is complete.
"""

from __future__ import annotations

import hashlib
import json
from dataclasses import dataclass
from pathlib import Path

MAX_STREAM_BYTES = 64 * 1024 * 1024
MAX_LINE_BYTES = 8 * 1024 * 1024
U64_MAX = 2**64 - 1
COUNTERS = ("input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens", "output_tokens")


def integer(value):
    return type(value) is int and 0 <= value <= U64_MAX


def counters(value):
    if not isinstance(value, dict) or any(not integer(value.get(k)) for k in COUNTERS):
        return None
    return {k: value[k] for k in COUNTERS}


def canonical_usage(raw):
    if raw is None:
        return None
    total = raw["input_tokens"] + raw["cache_creation_input_tokens"] + raw["cache_read_input_tokens"]
    if total > U64_MAX:
        return None
    return {"input_tokens": total, "output_tokens": raw["output_tokens"],
            "cached_input_tokens": raw["cache_read_input_tokens"], "model_calls": 1,
            "cost_microusd": None}


@dataclass
class Accounting:
    calls: list[dict]
    complete: bool
    reasons: list[str]
    terminal: dict | None
    stream_sha256: str | None = None


class ClaudeUsage:
    def __init__(self):
        self.messages, self.active = {}, {}
        self.sessions, self.reasons = set(), set()
        self.terminal = None

    def message(self, message):
        if not isinstance(message, dict) or not isinstance(message.get("id"), str) or not message["id"].strip():
            self.reasons.add("missing_message_id")
            return None
        key = message["id"]
        model = message.get("model")
        if not isinstance(model, str) or not model.strip():
            self.reasons.add("missing_model")
            model = ""
        current = self.messages.setdefault(key, {"model": model, "usage": None, "request": "",
                                                "delta": False, "complete": False, "started": False})
        if current["model"] != model:
            self.reasons.add("message_model_changed")
        return current

    def feed(self, row):
        if not isinstance(row, dict):
            self.reasons.add("invalid_stream_record")
            return
        session = row.get("session_id")
        if isinstance(session, str) and session:
            self.sessions.add(session)
        kind = row.get("type")
        if kind == "result":
            if self.terminal is not None and self.terminal != row:
                self.reasons.add("conflicting_terminal_results")
            self.terminal = row
            return
        if kind not in ("assistant", "stream_event"):
            return
        if self.terminal is not None:
            self.reasons.add("provider_event_after_terminal")
        parent = row.get("parent_tool_use_id")
        if parent is not None:
            self.reasons.add("subagent_accounting_unsupported")
        lane = parent or "root"
        if kind == "assistant":
            message = row.get("message")
            current = self.message(message)
            if current is None:
                return
            request = row.get("request_id", row.get("requestId"))
            if row.get("request_id") and row.get("requestId") and row["request_id"] != row["requestId"]:
                self.reasons.add("conflicting_request_id")
            if not isinstance(request, str) or not request.strip():
                self.reasons.add("missing_provider_request_id")
            elif current["request"] and current["request"] != request:
                self.reasons.add("message_request_changed")
            else:
                current["request"] = request
            raw = counters(message.get("usage"))
            # Chunks repeat a provisional usage object. They never overwrite completed deltas.
            if not current["started"] and not current["complete"]:
                current["usage"] = raw
            if message.get("stop_reason") is not None and not current["started"]:
                if current["complete"] and current["usage"] != raw:
                    self.reasons.add("conflicting_completed_usage")
                current.update(usage=raw, complete=raw is not None)
            return
        event = row.get("event")
        if not isinstance(event, dict):
            self.reasons.add("invalid_stream_event")
            return
        event_type = event.get("type")
        if event_type == "message_start":
            message = event.get("message")
            current = self.message(message)
            if current is None:
                return
            if lane in self.active or current["started"]:
                self.reasons.add("repeated_or_overlapping_message_start")
            self.active[lane] = message["id"]
            current.update(usage=counters(message.get("usage")), started=True)
        elif event_type == "message_delta":
            current = self.messages.get(self.active.get(lane))
            delta = event.get("usage")
            if current is None or not isinstance(delta, dict) or not integer(delta.get("output_tokens")):
                self.reasons.add("invalid_usage_delta")
                return
            raw = current["usage"]
            if raw is None:
                self.reasons.add("incomplete_request_usage")
                return
            for key in COUNTERS:
                if key in delta:
                    if not integer(delta[key]) or delta[key] < raw[key]:
                        self.reasons.add("invalid_usage_counter")
                    else:
                        raw[key] = delta[key]
            current["delta"] = True
        elif event_type == "message_stop":
            current = self.messages.get(self.active.pop(lane, None))
            if current is None:
                self.reasons.add("unmatched_message_stop")
            else:
                current["complete"] = current["delta"] and current["usage"] is not None

    def finish(self):
        if len(self.sessions) != 1:
            self.reasons.add("missing_or_mixed_session_identity")
        if self.active:
            self.reasons.add("unfinished_provider_message")
        if not self.messages:
            self.reasons.add("no_provider_requests")
        totals = {k: 0 for k in COUNTERS}
        by_model, requests, calls = {}, set(), []
        for message in self.messages.values():
            raw = message["usage"]
            request = message["request"]
            if not request:
                self.reasons.add("missing_provider_request_id")
            elif request in requests:
                self.reasons.add("reused_request_id")
            requests.add(request)
            if not message["complete"] or raw is None:
                self.reasons.add("incomplete_request_usage")
            usage = canonical_usage(raw) if message["complete"] else None
            if usage is None:
                self.reasons.add("incomplete_request_usage")
            calls.append({"id": f"anthropic/{request}" if request else "", "model": message["model"], "usage": usage})
            if raw is not None:
                model = by_model.setdefault(message["model"], {k: 0 for k in COUNTERS})
                for key in COUNTERS:
                    totals[key] += raw[key]
                    model[key] += raw[key]
        terminal_usage = counters((self.terminal or {}).get("usage"))
        if terminal_usage is None:
            self.reasons.add("missing_terminal_usage")
        elif totals != terminal_usage:
            self.reasons.add("terminal_usage_mismatch")
        reported = (self.terminal or {}).get("modelUsage")
        if not isinstance(reported, dict) or set(reported) != set(by_model):
            self.reasons.add("model_usage_mismatch")
        else:
            keys = dict(zip(COUNTERS, ("inputTokens", "cacheCreationInputTokens", "cacheReadInputTokens", "outputTokens")))
            for model, raw in by_model.items():
                value = reported[model]
                got = {key: value.get(alias) for key, alias in keys.items()} if isinstance(value, dict) else None
                if counters(got) != raw:
                    self.reasons.add("model_usage_mismatch")
        if not self.terminal or type(self.terminal.get("is_error")) is not bool:
            self.reasons.add("missing_terminal_status")
        # CLI total_cost_usd/modelUsage.costUSD can explicitly have costBasis=list. Neither is
        # a provider invoice, so costs stay unknown even when the admission budget knows them.
        return Accounting(sorted(calls, key=lambda c: (c["id"], c["model"])), not self.reasons,
                          sorted(self.reasons), self.terminal)


def read_stream(path: str | Path) -> Accounting:
    parser = ClaudeUsage()
    with Path(path).open("rb") as handle:
        body = handle.read(MAX_STREAM_BYTES + 1)
    if len(body) > MAX_STREAM_BYTES:
        parser.reasons.add("stream_size_limit")
        body = body[:MAX_STREAM_BYTES]
    for line in body.splitlines():
        if len(line) > MAX_LINE_BYTES:
            parser.reasons.add("stream_record_size_limit")
            continue
        try:
            row = json.loads(line)
            if isinstance(row, dict) and isinstance(row.get("line"), str):
                row = json.loads(row["line"])
            parser.feed(row)
        except (ValueError, UnicodeDecodeError):
            parser.reasons.add("malformed_stream_record")
    result = parser.finish()
    result.stream_sha256 = hashlib.sha256(body).hexdigest() if "stream_size_limit" not in result.reasons else None
    return result


def independent_grade(stdout, returncode, timed_out=False):
    """A grader failure or a truthy string is not a successful task grade."""
    if returncode != 0 or timed_out or len(stdout) > 1024 * 1024:
        return None
    try:
        value = json.loads(stdout)
    except (ValueError, UnicodeDecodeError):
        return None
    return value["passed"] if isinstance(value, dict) and type(value.get("passed")) is bool else None


def receipt(protocol_sha256, job_id, run_id, observed, accounting, grade, *, timed_out=False, exit_code=0):
    terminal = accounting.terminal or {}
    status = "timeout" if timed_out else "failed" if exit_code != 0 or terminal.get("is_error") is not False else "completed"
    return {"schema": 1, "protocol_sha256": protocol_sha256, "job_id": job_id, "run_id": run_id,
            "observed": observed, "status": status, "passed": grade if status == "completed" else False,
            "usage_complete": accounting.complete, "calls": accounting.calls}
