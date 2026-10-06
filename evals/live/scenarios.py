"""Offline, advisory ordering audit of native Claude Code tool evidence.

This does not grade test outcomes, establish checkout/revision identity, or authorize a
publication. A non-error tool result is only a reported tool completion. Text such as
"tests passed" is never used as evidence. Keep independent graders and frozen receipts.
Commands are declared before the study and matched exactly (outer whitespace aside).
"""
from __future__ import annotations

import argparse
import fnmatch
import hashlib
import json
from pathlib import Path, PurePosixPath

MAX_STREAM_BYTES = 64 * 1024 * 1024
MAX_LINE_BYTES = 8 * 1024 * 1024
EDIT_TOOLS = {"Edit", "Write", "MultiEdit", "NotebookEdit"}
COMMAND_FIELDS = ("verification_commands", "publication_commands", "lookup_commands")


def validate_spec(spec):
    if not isinstance(spec, dict) or type(spec.get("schema")) is not int or spec["schema"] != 1:
        raise ValueError("scenario specification needs schema: 1")
    if set(spec) - {"schema", "relevant_paths", "checkout_root", *COMMAND_FIELDS}:
        raise ValueError("unknown scenario specification field")
    for name in ("relevant_paths", *COMMAND_FIELDS):
        values = spec.get(name, [])
        if not isinstance(values, list) or any(not isinstance(v, str) or not v.strip() for v in values):
            raise ValueError(f"{name} must be a list of nonempty strings")
        if len(set(v.strip() for v in values)) != len(values):
            raise ValueError(f"{name} contains duplicate entries")
    if not spec.get("relevant_paths") or not spec.get("verification_commands"):
        raise ValueError("declare relevant_paths and verification_commands before auditing")
    for pattern in spec["relevant_paths"]:
        if pattern.startswith("/") or ".." in PurePosixPath(pattern).parts:
            raise ValueError("relevant_paths must be checkout-relative globs without '..'")
    root = spec.get("checkout_root")
    if root is not None and (not isinstance(root, str) or not root.startswith("/") or ".." in PurePosixPath(root).parts):
        raise ValueError("checkout_root must be an absolute POSIX path without '..'")
    groups = [set(v.strip() for v in spec.get(name, [])) for name in COMMAND_FIELDS]
    if any(groups[i] & groups[j] for i in range(3) for j in range(i)):
        raise ValueError("a command cannot have more than one declared action role")
    return spec


def relevant_path(value, spec):
    """None means path identity is unavailable, never 'unrelated'. No filesystem access."""
    if not isinstance(value, str) or not value or ".." in PurePosixPath(value).parts:
        return None
    path = PurePosixPath(value)
    if path.is_absolute():
        if not spec.get("checkout_root"):
            return None
        try:
            path = path.relative_to(spec["checkout_root"])
        except ValueError:
            return None
    return any(fnmatch.fnmatchcase(str(path), pattern) for pattern in spec["relevant_paths"])


def tool_status(action):
    result = action.get("result")
    if result is None or type(result.get("is_error")) is not bool:
        return "unknown"
    return "reported_error" if result["is_error"] else "reported_nonerror_completion"


def audit(rows, spec, *, diagnostics=()):
    """Report observed ordering only. Unobserved or ambiguous evidence is never a pass."""
    validate_spec(spec)
    problems, sessions, actions = set(diagnostics), set(), {}
    terminal, terminal_position = None, None
    commands = {name: {v.strip() for v in spec.get(name, [])} for name in COMMAND_FIELDS}
    for position, row in enumerate(rows):
        if not isinstance(row, dict):
            problems.add("invalid_stream_record")
            continue
        session = row.get("session_id")
        if isinstance(session, str) and session:
            sessions.add(session)
        if row.get("parent_tool_use_id") is not None:
            problems.add("subagent_actions_unsupported")
        kind = row.get("type")
        if kind == "result":
            if terminal is not None and terminal != row:
                problems.add("conflicting_terminal_results")
            if terminal is None:
                terminal, terminal_position = row, position
            continue
        if kind not in ("assistant", "user"):
            continue
        if terminal is not None:
            problems.add("message_after_terminal")
        message = row.get("message")
        if not isinstance(message, dict):
            problems.add("invalid_message")
            continue
        content = message.get("content")
        if isinstance(content, str):
            continue
        if not isinstance(content, list):
            problems.add("invalid_message_content")
            continue
        for block in content:
            if not isinstance(block, dict):
                problems.add("invalid_content_block")
                continue
            if kind == "assistant" and block.get("type") == "tool_use":
                tool_id = block.get("id")
                if (not isinstance(tool_id, str) or not tool_id or not isinstance(block.get("input"), dict)
                        or not isinstance(block.get("name"), str) or not block["name"]):
                    problems.add("invalid_tool_use")
                    continue
                if tool_id in actions:
                    if actions[tool_id]["call"] != block:
                        problems.add("conflicting_tool_use")
                    continue
                actions[tool_id] = {"call": block, "start": position, "finish": None, "result": None}
            elif kind == "user" and block.get("type") == "tool_result":
                tool_id = block.get("tool_use_id")
                action = actions.get(tool_id) if isinstance(tool_id, str) else None
                if action is None:
                    problems.add("orphan_tool_result")
                elif action["result"] is not None:
                    if action["result"] != block:
                        problems.add("conflicting_tool_result")
                else:
                    action.update(result=block, finish=position)
    if len(sessions) != 1:
        problems.add("missing_or_mixed_session_identity")
    if terminal is None:
        problems.add("missing_terminal_result")
    elif type(terminal.get("is_error")) is not bool:
        problems.add("missing_terminal_status")
    for action in actions.values():
        if action["result"] is None:
            problems.add("unfinished_tool_use")

    edits, verification, publications, lookups, other_commands, unknown_edits = [], [], [], [], [], []
    for tool_id, action in actions.items():
        call, value = action["call"], action["call"]["input"]
        entry = {"tool_use_id": tool_id, "tool": call.get("name"), "start_record": action["start"],
                 "finish_record": action["finish"], "status": tool_status(action)}
        if call.get("name") in EDIT_TOOLS:
            path = value.get("file_path", value.get("notebook_path"))
            entry["path"] = path
            relevant = relevant_path(path, spec)
            if relevant is True:
                edits.append(entry)
            elif relevant is None:
                unknown_edits.append(entry)
        elif call.get("name") == "Bash":
            command = value.get("command")
            entry["command"] = command
            if not isinstance(command, str):
                other_commands.append(entry)
                continue
            for name, target in (("verification_commands", verification), ("publication_commands", publications),
                                 ("lookup_commands", lookups)):
                if command.strip() in commands[name]:
                    target.append(entry)
                    break
            else:
                other_commands.append(entry)

    def at_boundary(position):
        # Calls in one assistant record may execute in parallel. A new edit in the
        # publication's record therefore invalidates earlier verification as well.
        before = [e for e in edits if e["start_record"] <= position]
        unknown_before = [e for e in unknown_edits if e["start_record"] <= position]
        if problems or unknown_before or not before:
            return {"state": "unknown", "tool_use_id": None,
                    "reason": "incomplete/ambiguous evidence or no observed relevant edit"}
        if any(e["finish_record"] is None or e["finish_record"] >= position for e in before):
            return {"state": "unknown", "tool_use_id": None, "reason": "relevant edit still unresolved at boundary"}
        last_edit = max(e["finish_record"] for e in before)
        after = [v for v in verification if v["start_record"] > last_edit and v["start_record"] < position]
        completed = [v for v in after if v["status"] == "reported_nonerror_completion"
                     and v["finish_record"] < position]
        if completed:
            return {"state": "observed", "tool_use_id": completed[-1]["tool_use_id"],
                    "reason": "declared tool completed without reported error after all observed relevant edits"}
        if any(v["status"] == "unknown" or v["finish_record"] >= position for v in after):
            return {"state": "unknown", "tool_use_id": None, "reason": "verification result unresolved at boundary"}
        return {"state": "not_observed", "tool_use_id": None,
                "reason": "no declared non-error verification completion after the last observed relevant edit"}

    for publication in publications:
        publication["verification_before_attempt"] = at_boundary(publication["start_record"])
    completions = [v["finish_record"] for v in lookups if v["status"] == "reported_nonerror_completion"]
    first_lookup = min(completions) if completions else None
    prior_lookup = [v for v in lookups if v["finish_record"] is not None
                    and (first_lookup is None or v["finish_record"] < first_lookup)]
    final = at_boundary(terminal_position) if terminal_position is not None else {
        "state": "unknown", "tool_use_id": None, "reason": "missing terminal boundary"}
    if terminal and terminal.get("is_error") is not False:
        final = {"state": "unknown", "tool_use_id": None, "reason": "terminal did not report successful completion"}
    return {"schema": 1, "evidence_kind": "advisory_native_tool_ordering", "diagnostics": sorted(problems),
            "final_verification_after_observed_edits": final, "observed_relevant_edits": edits,
            "unresolved_edit_paths": unknown_edits, "verification_attempts": verification,
            "publication_attempts": publications, "lookup_attempts": lookups,
            "observed_lookup_errors_before_first_nonerror": sum(v["status"] == "reported_error" for v in prior_lookup),
            "lookup_results_unknown": sum(v["status"] == "unknown" for v in lookups),
            "other_shell_commands": other_commands,
            "independent_test_outcome": "unknown", "exact_revision_verified": "unknown",
            "publication_remote_state": "unknown", "lookup_task_correct": "unknown",
            "limitations": ["Non-error tool results do not prove tests ran, passed, or covered the task.",
                            "Shell commands and other tools can mutate files; not every edit is observed.",
                            "No filesystem snapshot, external-system state, or source authenticity is established.",
                            "Exact command matching requires predeclared recipes; unknown commands are not classified."]}


def read_audit(path, spec):
    with Path(path).open("rb") as handle:
        body = handle.read(MAX_STREAM_BYTES + 1)
    problems, rows = [], []
    oversized = len(body) > MAX_STREAM_BYTES
    if oversized:
        problems.append("stream_size_limit")
    for line in body[:MAX_STREAM_BYTES].splitlines():
        if len(line) > MAX_LINE_BYTES:
            problems.append("stream_record_size_limit")
            continue
        try:
            row = json.loads(line)
            if isinstance(row, dict) and isinstance(row.get("line"), str):
                row = json.loads(row["line"])
            rows.append(row)
        except (ValueError, UnicodeDecodeError):
            problems.append("malformed_stream_record")
    result = audit(rows, spec, diagnostics=problems)
    result["source_sha256"] = None if oversized else hashlib.sha256(body).hexdigest()
    result["spec_sha256"] = hashlib.sha256(json.dumps(spec, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
    return result


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("stream", type=Path, help="raw or runner-wrapped native Claude Code JSONL")
    parser.add_argument("--spec", required=True, type=Path, help="predeclared scenario action specification JSON")
    args = parser.parse_args(argv)
    try:
        result = read_audit(args.stream, json.loads(args.spec.read_text()))
    except (OSError, ValueError) as error:
        parser.error(str(error))
    print(json.dumps(result, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
