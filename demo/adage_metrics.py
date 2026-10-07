"""Metrics the demo runner derives from a run's debug log, stream and Adage spool (chunk E1).

Pure functions over text, parsed stream lines and spool records, so every one is unit-tested
offline (demo/test_run_arms.py). Nothing here calls the model or the binary.

- `requests()` mirrors `evals/reflex-proof/harness/heldout_analysis.py::requests()` exactly: the same
  `DEBUG_REQUEST` and `API_ERROR` regexes, and the same cross-check of the session's own turns
  (`STREAMED_SOURCES`) against the stream's distinct explicit request ids
  (`probe.explicit_request_ids`: literal `request_id`/`requestId` fields only, never message ids).
  It adds `requests_streamed` and `requests_by_source` beside the harness's keys.
  `test_run_arms.RequestsTest.test_mirrors_the_harness` pins the patterns and the harness's keys to
  the harness's own when the harness is importable.
- `read_spool()` reads an Adage session spool: CRC-framed records (`magic | len | crc | payload`,
  `crates/adage-store/src/spool.rs`), or JSON lines as a fallback. Corrupt frames are skipped.
- `spool_metrics()` counts what the spool says Adage did, failing soft: an unknown or missing
  field counts as nothing, never as an error.
"""

import json
import re
import zlib
from pathlib import Path

# Byte-for-byte the harness's patterns (heldout_analysis.py:67-68).
DEBUG_REQUEST = re.compile(r"^\S+ \[DEBUG\] \[API REQUEST\] /v1/messages x-client-request-id=([\w-]+) source=(\w+)$")
API_ERROR = re.compile(r"^\S+ \[ERROR\] API error \(attempt \d+/\d+\)")
# The dispatch sources that are the session's own turns, whose request ids stream-json carries:
# `sdk` under -p, `repl_main_thread` interactively. Claude Code 2.1.29x also sends requests the
# stream never shows (`generate_session_title` as the session starts, `side_query` after its last
# turn): they are counted in `requests`, but they are not the stream's to confirm.
STREAMED_SOURCES = ("sdk", "repl_main_thread")


def explicit_request_ids(value):
    """Only literal request-id fields: never substitute message ids (probe.py:155-166)."""
    found = []
    if isinstance(value, dict):
        for key, item in value.items():
            if key in ("request_id", "requestId") and isinstance(item, str):
                found.append(item)
            else:
                found.extend(explicit_request_ids(item))
    elif isinstance(value, list):
        for item in value:
            found.extend(explicit_request_ids(item))
    return found


def requests(debug_text, stream_observations):
    """Debug-log dispatches (the primary count: every source, retries included) and the
    per-session cross-check.

    Verified when the stream's distinct request ids equal the dispatches of the session's own turns
    (`STREAMED_SOURCES`), or when their surplus is fully explained by logged `[ERROR] API error
    (attempt ...)` entries; and the dispatch ids must be unique. Dispatches from other sources are
    counted and never streamed, so they explain themselves. `stream_observations` is
    [{"value": <parsed stream line>}]."""
    lines = (debug_text or "").splitlines()
    dispatches = [(m[1], m[2]) for line in lines if (m := DEBUG_REQUEST.fullmatch(line))]
    streamed = sum(1 for _, source in dispatches if source in STREAMED_SOURCES)
    errors = sum(1 for line in lines if API_ERROR.match(line))
    ids = []
    for item in stream_observations:
        for rid in explicit_request_ids(item.get("value")):
            if rid not in ids:
                ids.append(rid)
    surplus = streamed - len(ids)
    by_source = {}
    for _, source in dispatches:
        by_source[source] = by_source.get(source, 0) + 1
    unique = len({rid for rid, _ in dispatches}) == len(dispatches)
    return dict(requests=len(dispatches), stream_request_ids=len(ids), api_error_entries=errors,
                cross_check=surplus == 0, verified=0 <= surplus <= errors and unique,
                requests_streamed=streamed, requests_by_source=by_source)


# ------------------------------------------------------------------------------------------ spool

SPOOL_MAGIC = bytes([0xAD, 0xA6, 0xE5, 0x01])
SPOOL_HEADER = 12
SPOOL_MAX_RECORD = 16 << 20


def parse_spool(data):
    """Every valid record in spool bytes, in file order (as `spool::read_all`): frames whose CRC
    or JSON is bad are skipped by resynchronising on the next magic. Bytes with no frame at all are
    read as JSON lines, so a hand-written fixture spool works too."""
    out, i, framed = [], 0, False
    while i + SPOOL_HEADER <= len(data):
        if data[i:i + 4] != SPOOL_MAGIC:
            i += 1
            continue
        framed = True
        n = int.from_bytes(data[i + 4:i + 8], "little")
        crc = int.from_bytes(data[i + 8:i + 12], "little")
        end = i + SPOOL_HEADER + n
        if n > SPOOL_MAX_RECORD or end > len(data) or zlib.crc32(data[i + SPOOL_HEADER:end]) != crc:
            i += 1
            continue
        try:
            rec = json.loads(data[i + SPOOL_HEADER:end])
            if isinstance(rec, dict):
                out.append(rec)
        except ValueError:
            pass
        i = end
    if framed:
        return out
    for line in data.decode("utf-8", "replace").splitlines():
        try:
            rec = json.loads(line)
        except ValueError:
            continue
        if isinstance(rec, dict):
            out.append(rec)
    return out


def frame(rec):
    """One spool frame for `rec` (for fixtures and tests)."""
    payload = json.dumps(rec).encode()
    return SPOOL_MAGIC + len(payload).to_bytes(4, "little") + zlib.crc32(payload).to_bytes(4, "little") + payload


def read_spool(path):
    try:
        return parse_spool(Path(path).read_bytes())
    except OSError:
        return []


def _kind(rec):
    return str(rec.get("kind") or "").replace(".", "_").replace("-", "_").lower()


def _decision(rec):
    d = rec.get("decision")
    return d if isinstance(d, dict) else {}


def _judgement_kinds(rec):
    js = _decision(rec).get("judgements")
    return [str(j.get("kind", "")).lower() for j in js if isinstance(j, dict)] if isinstance(js, list) else []


def is_predicted_step(rec):
    """A `turn_step` record (TurnStep, turn.step) whose `predicted`/`predict` is set, at the top
    level or in its decision."""
    if _kind(rec) not in ("turn_step", "turnstep"):
        return False
    d = _decision(rec)
    return any(bool(x.get(k)) for x in (rec, d) for k in ("predicted", "predict"))


def is_receipt_as_result(rec):
    """A `pretool` record answered from a receipt (`answered: receipt`, T-C3-03)."""
    if _kind(rec) != "pretool":
        return False
    a = rec.get("answered")
    if isinstance(a, dict):
        a = a.get("by") or a.get("answered")
    v = rec.get("verdict")
    if a is None and isinstance(v, dict):
        a = v.get("answered")
    return str(a or "").lower() == "receipt"


def is_substitution(rec):
    """A dead end substituted before it ran: a `substituted` judgement (T-C6-02), or a `pretool`
    record that says it substituted (`substituted: true`, `answered: substitute`, or
    `pretool: {"substitute": ...}`). The control arm's `would_substitute` is not a substitution."""
    if "substituted" in _judgement_kinds(rec):
        return True
    if _kind(rec) != "pretool" or rec.get("withheld"):
        return False
    if rec.get("substituted") is True or str(rec.get("answered") or "").lower() == "substitute":
        return True
    p = rec.get("pretool")
    return isinstance(p, dict) and "substitute" in p


def delivered_directives(rec):
    """Directives the agent received from this record (empty in control: `would_have` is not
    delivered)."""
    ds = _decision(rec).get("directives")
    return len(ds) if isinstance(ds, list) else 0


def spool_metrics(records):
    """The spool-derived metrics of one session; counts only, failing soft."""
    out = {"predicted_steps": 0, "receipts_as_results": 0, "dead_end_substituted": 0,
           "directives": 0, "session_starts": 0, "spool_records": 0}
    for rec in records:
        if not isinstance(rec, dict):
            continue
        out["spool_records"] += 1
        try:
            out["predicted_steps"] += is_predicted_step(rec)
            out["receipts_as_results"] += is_receipt_as_result(rec)
            out["dead_end_substituted"] += is_substitution(rec)
            out["directives"] += delivered_directives(rec)
            out["session_starts"] += _kind(rec) == "session_start"
        except (AttributeError, TypeError, ValueError):
            continue
    return out


# ----------------------------------------------------------------------------------------- stream

def stream_metrics(stream_lines):
    """Counts from parsed stream-json lines: tool calls made inside subagents (assistant messages
    with a `parent_tool_use_id`), and `<adage-reflex` blocks seen in hook events (what was delivered
    as the transcript shows it)."""
    sub, reflex = 0, 0
    for m in stream_lines:
        if not isinstance(m, dict):
            continue
        if m.get("type") == "assistant" and m.get("parent_tool_use_id"):
            content = (m.get("message") or {}).get("content") or []
            sub += sum(1 for c in content if isinstance(c, dict) and c.get("type") == "tool_use")
        elif m.get("type") == "system" and m.get("subtype") == "hook_response":
            # The hook's stdout when the event carries it (other fields may repeat it), else the
            # whole event.
            text = m.get("stdout") if isinstance(m.get("stdout"), str) else json.dumps(m)
            reflex += text.count("<adage-reflex")
    return {"subagent_tool_calls": sub, "directives_in_stream": reflex}
