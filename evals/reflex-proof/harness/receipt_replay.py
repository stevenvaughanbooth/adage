#!/usr/bin/env python3
"""Offline replay of CPU receipt consumption from native hook records. No paid calls or grants.

A timeline is the ordered agent tool requests (with the whole-source digest the hook observed
before each) and Adage receipts (with the digest at execution). Every later agent pytest request
is classified against the latest receipt. The predeclared equivalence is deliberately narrow:
identical interpreter token and targets, differing only in OUTPUT_FLAGS. A narrower node inside
the receipt's targets is `covered_subset`, reported separately and never counted as a repeat.
Failed receipts, source changes (the digest excludes only declared caches and the environment),
explicitly requested reruns and broader scopes keep verification necessary.

Version 2 (launch-gate-1 revision 7, before any held-out dispatch): a request is a *repeat
attempt* when a fresh successful receipt already selected every test it selects (same non-output
options, targets equal or inside), whatever its interpreter and whether or not it launched. A
bare `python` that fails with "command not found" is still a request the model made. Any repeat
attempt blocks a confirmed elimination. Only requests that completed (a PostToolUse exists) can
satisfy necessary verification; timelines without that field are treated as completed.

Version 3 (launch-gate-1 revision 11, before any dev-v9 positive block): every receipt carries its
origin, from the journal (`context`, `obligation`, `stop-gate`, `manual`) or, for a journal written
before origins were recorded, inferred from its hook event (a run journaled at Stop is the gate's).
Whether verification is necessary is decided by the learned context's runs alone: a failed or
stale context run makes it necessary. A passing run on the final sources satisfies it, whoever ran
it: the agent, the context, or the finish-line gate. Two exceptions apply. A requested rerun is
satisfied only by the agent's own completed run, because the user asked the agent. A requested
wider scope is satisfied only by a run of that scope. Gate runs are also reported on their own.
dev-v9 stopped at its first control because version 2 read the gate's passing run as the
context's.
"""
import argparse
import json
from pathlib import Path
import re
import shlex

VERSION = 3
OUTPUT_FLAGS = {"-q", "-qq", "-v", "-vv", "-rA", "-ra", "-s", "-x", "--no-header", "--disable-warnings",
                "--quiet", "--verbose"}
OUTPUT_PREFIXES = ("--tb=", "--color=")
CACHE_PAIRS = (("-p", "no:cacheprovider"),)
VALUED = {"-k", "-m", "-p", "-c", "-o", "--deselect", "--rootdir", "--ignore", "--maxfail", "--junitxml"}
REDIRECTS = re.compile(r"(?<=\s)(?:2>&1|[12]?>\s*/dev/null)(?=\s|$)")
REDUNDANT = ("exact_repeat", "equivalent_repeat")
NECESSARY = ("after_failed_receipt", "after_relevant_change", "requested_rerun", "different_scope")
RAN = re.compile(r"(?:Adage|adage:) ran `([^`]+)`")
GATE = "stop-gate"


def segments(command):
    """Split a shell command at list/pipe operators. Anything else unusual is `opaque`."""
    try:
        lexer = shlex.shlex(REDIRECTS.sub("", command), posix=True, punctuation_chars=";&|<>()")
        lexer.whitespace_split = True
        tokens = list(lexer)
    except ValueError:
        return None
    out, current = [], []
    for token in tokens:
        if token in ("&&", "||", ";", "|", "&"):
            out.append(current)
            current = []
        elif set(token) <= set(";&|<>()"):
            return None
        else:
            current.append(token)
    out.append(current)
    return [s for s in out if s]


def pytest_call(argv, root=""):
    """(interpreter, flags, targets) for `<python> -m pytest ...` or `pytest ...`; else None."""
    argv = [a[len(root) + 1:] if root and a.startswith(root + "/") else a for a in argv]
    if argv[:1] == ["pytest"] or argv[:1] == [".venv/bin/pytest"]:
        interpreter, rest = argv[0], argv[1:]
    elif len(argv) >= 3 and argv[1:3] == ["-m", "pytest"] and re.search(r"python[0-9.]*$", argv[0]):
        interpreter, rest = argv[0], argv[3:]
    else:
        return None
    flags, targets, i = [], [], 0
    while i < len(rest):
        a = rest[i]
        pair = next((p for p in CACHE_PAIRS if a == p[0] and i + 1 < len(rest) and rest[i + 1] == p[1]), None)
        if pair:
            i += 2
            continue
        if a in OUTPUT_FLAGS or a.startswith(OUTPUT_PREFIXES) or re.fullmatch(r"-[qvsx]+", a):
            pass
        elif a in VALUED and i + 1 < len(rest):
            flags.extend(rest[i:i + 2])
            i += 1
        elif a.startswith("-"):
            flags.append(a)
        else:
            targets.append(a.rstrip("/"))
        i += 1
    return interpreter, tuple(flags), tuple(sorted(set(targets)))


def within(target, scope):
    """Whether pytest target `target` selects a subset of target `scope` (file/dir/node ids)."""
    if not scope:
        return True
    return any(target == s or target.startswith(s + "::") or target.startswith(s + "/") for s in scope)


def covered(request, receipt, rerun_requested=False):
    """Whether a fresh successful receipt already ran every test this request selects."""
    if receipt is None or not receipt["succeeded"] or request["digest"] != receipt["digest"] or rerun_requested:
        return False
    call, done = request["call"], receipt["call"]
    return call[1] == done[1] and (call[2] == done[2] or bool(call[2]) and all(within(t, done[2]) for t in call[2]))


def classify(request, receipt, rerun_requested=False):
    if receipt is None:
        return "before_receipt"
    if not receipt["succeeded"]:
        return "after_failed_receipt"
    if request["digest"] != receipt["digest"]:
        return "after_relevant_change"
    if rerun_requested:
        return "requested_rerun"
    call, done = request["call"], receipt["call"]
    if call[0] != done[0]:
        return "different_environment"
    if call[1] != done[1]:
        return "different_scope"
    if call[2] == done[2]:
        return "exact_repeat" if request["command_segment"] == receipt["command"] else "equivalent_repeat"
    if call[2] and all(within(t, done[2]) for t in call[2]):
        return "covered_subset"
    return "different_scope"


def hook_text(output, model_only=False):
    """The model- or person-facing text of a hook's output: additionalContext, a Stop reason, or
    (unless `model_only`) a person notice."""
    if not output:
        return ""
    try:
        value = json.loads(output)
    except json.JSONDecodeError:
        return ""
    model = (value.get("hookSpecificOutput") or {}).get("additionalContext") or value.get("reason")
    return model or ("" if model_only else value.get("systemMessage") or "")


def origin(event):
    """A receipt event's origin: recorded by the journal, or inferred from the hook event for a
    journal that predates the field (a run journaled at Stop is the finish-line gate's)."""
    return event.get("origin") or (GATE if event.get("at") == "Stop" else "context")


def extract(hooks_path, arm=None):
    """Compact timeline from a `production-hooks.jsonl` capture (the collected hook wrapper log).

    Works with both registrations: the older per-tool one (PreToolUse/PostToolUse for every tool)
    and the production one (PreToolUse for Bash only, PostToolBatch, PostToolUseFailure). A tool seen
    only in a batch is placed at that batch with the digest observed then. A call is completed when
    a PostToolUse or a PostToolBatch reports it and no PostToolUseFailure does.
    """
    events, root, final = [], None, None
    rows = [json.loads(line) for line in Path(hooks_path).read_text().splitlines()]
    reported, failed, seen = set(), set(), set()
    for row in rows:
        payload = row.get("payload", {})
        if row.get("event") == "PostToolUse":
            reported.add(payload.get("tool_use_id"))
        elif row.get("event") == "PostToolBatch":
            reported.update(c.get("tool_use_id") for c in payload.get("tool_calls", []))
        elif row.get("event") == "PostToolUseFailure":
            failed.add(payload.get("tool_use_id"))
    completed = reported - failed

    def tool(call, digest, pre_output=""):
        name, data = call.get("tool_name"), call.get("tool_input", {}) or {}
        return dict(kind="tool", id=call.get("tool_use_id"), name=name,
            command=data.get("command") if name == "Bash" else None,
            path=data.get("file_path") if name in ("Edit", "Write", "MultiEdit") else None,
            digest=digest, completed=call.get("tool_use_id") in completed,
            adage_pre_context="additionalContext" in (pre_output or ""),
            receipt_note="<adage-receipt" in (pre_output or ""))

    for row in rows:
        payload = row.get("payload", {})
        root = root or payload.get("cwd")
        event = row.get("event")
        if event == "PreToolUse":
            seen.add(payload.get("tool_use_id"))
            events.append(tool(payload, row.get("before", {}).get("digest"), row.get("output")))
        elif event == "PostToolBatch":
            for call in payload.get("tool_calls", []):
                if call.get("tool_use_id") not in seen:
                    seen.add(call.get("tool_use_id"))
                    events.append(tool(call, row.get("before", {}).get("digest")))
        for entry in row.get("journal_new") or []:
            match = RAN.search(hook_text(row.get("output")))
            outcome = entry.get("outcome", {})
            # The command: as the hook's text names it, or else this route's last named command
            # (a gate run is the route's own command; a silent notice may not name it).
            command = match.group(1) if match else next(
                (e["command"] for e in reversed(events) if e["kind"] == "receipt" and e.get("route") == entry.get("route")
                 and e.get("command")), None)
            events.append(dict(kind="receipt", operation=entry.get("id"), route=entry.get("route"),
                command=command, phase=entry.get("phase"),
                outcome=outcome.get("outcome"), changed=[c.get("path") for c in outcome.get("changed", [])],
                digest=row.get("after", {}).get("digest"),
                completion_delivered=bool(RAN.search(hook_text(row.get("output"), model_only=True))), at=event,
                origin=entry.get("origin") or (GATE if event == "Stop" else "context"),
                origin_recorded="origin" in entry))
        if event in ("Stop", "SessionEnd"):
            final = row.get("after", {}).get("digest")
    return dict(arm=arm, root=root, events=events, final_digest=final)


def gate_runs(timeline):
    """The finish-line gate's runs, on their own: outcome, digest, and whether they verified the
    final sources (passed on them)."""
    final = timeline.get("final_digest")
    return [dict(operation=e.get("operation"), outcome=e.get("outcome"), digest=e.get("digest"),
                 covers_final=e.get("phase") == "completed" and e.get("outcome") == "applied" and e.get("digest") == final)
            for e in timeline["events"] if e["kind"] == "receipt" and origin(e) == GATE]


def stop_gate_replay(timeline):
    """Offline prediction of the revision-8 Stop gate on a recorded session: it runs when a learned
    context run happened and no successful run covers the final sources (a later edit, or only a
    failed run). Whether it would pass is unknowable offline."""
    receipts = [e for e in timeline["events"] if e["kind"] == "receipt" and origin(e) == "context"]
    ok = [e for e in receipts if e.get("phase") == "completed" and e.get("outcome") == "applied"]
    final = timeline.get("final_digest")
    runs = bool(receipts) and not any(e["digest"] == final for e in ok)
    return dict(context_ran=bool(receipts), gate_runs=runs,
                reason=("no context run" if not receipts else "a passing run covers the final sources" if not runs
                        else "only failed runs" if not ok else "files changed after the last passing run"))


def replay(timeline, rerun_requested=False, broader_required=None):
    """Classify every pytest request; summarize repeats, eliminations and necessary work.

    `broader_required` lists acceptable target tuples for a requested wider run, e.g. [(), ("tests",)].
    """
    root, receipt, rows = timeline.get("root") or "", None, []
    receipts = []
    for event in timeline["events"]:
        if event["kind"] == "receipt":
            parts = segments(event["command"] or "")
            call = pytest_call(parts[0], root) if parts and len(parts) == 1 else None
            receipt = dict(command=event["command"], call=call, digest=event["digest"], origin=origin(event),
                succeeded=event["phase"] == "completed" and event["outcome"] == "applied" and call is not None)
            receipts.append(receipt)
            continue
        if event.get("name") != "Bash" or not event.get("command"):
            continue
        parts = segments(event["command"])
        if parts is None:
            # Unparsed shell counts against an elimination only when it could run the verifier.
            rows.append(dict(id=event["id"], command=event["command"], digest=event["digest"],
                classification="opaque_verification" if "pytest" in event["command"] else "opaque_shell"))
            continue
        for argv in parts:
            call = pytest_call(argv, root)
            if call is None:
                continue
            request = dict(call=call, digest=event["digest"], command_segment=shlex.join(argv))
            rows.append(dict(id=event["id"], command=event["command"], segment=request["command_segment"],
                digest=event["digest"], adage_pre_context=event.get("adage_pre_context", False),
                receipt_note=event.get("receipt_note", False), completed=event.get("completed", True),
                covered=covered(request, receipt, rerun_requested),
                classification=classify(request, receipt, rerun_requested)))
    final = timeline.get("final_digest")
    succeeded = [r for r in receipts if r["succeeded"]]
    context = [r for r in receipts if r["origin"] == "context"]
    redundant = [r for r in rows if r["classification"] in REDUNDANT]
    opaque = [r for r in rows if r["classification"] == "opaque_verification"]
    # Any passing run on the final sources verified them: the context's, an obligation's or the gate's.
    fresh_final = any(r["digest"] == final for r in succeeded)
    context_final = any(r["digest"] == final for r in context if r["succeeded"])
    attempts = [r for r in rows if r.get("covered")]
    at_final = [pytest_call(shlex.split(r["segment"]), root) for r in rows
                if "segment" in r and r["digest"] == final and r.get("completed", True)]
    if broader_required is not None:
        wide = {tuple(t) for t in broader_required}
        satisfied = any(c[2] in wide for c in at_final) or any(
            r["digest"] == final and r["call"][2] in wide for r in succeeded)
    elif rerun_requested:
        satisfied = bool(at_final)  # the user asked the agent to run it: only its own run counts
    else:
        satisfied = fresh_final or bool(at_final)
    # Necessity comes from the learned context's runs: a failed or stale one makes verification
    # necessary; a passing gate run never creates or removes the need, it can only meet it.
    necessary = bool(rerun_requested or broader_required is not None or (context and not context_final))
    return dict(version=VERSION, arm=timeline.get("arm"), requests=rows, receipts=len(receipts),
        context_receipts=len(context), gate_runs=gate_runs(timeline),
        successful_receipts=len(succeeded), redundant_repeats=len(redundant), repeat_attempts=len(attempts),
        repeat_attempts_not_completed=sum(not r.get("completed", True) for r in attempts),
        covered_subsets=sum(r["classification"] == "covered_subset" for r in rows), opaque=len(opaque),
        confirmed_elimination=bool(context_final and not attempts and not redundant and not opaque and not necessary),
        verification_necessary=necessary, necessary_work_skipped=necessary and not satisfied,
        final_source_verified=fresh_final or bool(at_final))

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("extract", "replay"))
    parser.add_argument("path", type=Path)
    parser.add_argument("--arm")
    args = parser.parse_args()
    timeline = extract(args.path, args.arm) if args.command == "extract" else json.loads(args.path.read_text())
    print(json.dumps(timeline if args.command == "extract" else replay(timeline), indent=2))
