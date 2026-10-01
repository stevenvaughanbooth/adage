#!/usr/bin/env python3
"""Offline replay of CPU receipt consumption from native hook records. No paid calls or grants.

A timeline is the ordered agent tool requests (with the whole-source digest the hook observed
before each) and Adage receipts (with the digest at execution). Every later agent pytest request
is classified against the latest receipt. The predeclared equivalence is deliberately narrow:
identical interpreter token and targets, differing only in OUTPUT_FLAGS. A narrower node inside
the receipt's targets is `covered_subset`, reported separately and never counted as a repeat.
Failed receipts, source changes (the digest excludes only declared caches and the environment),
explicitly requested reruns and broader scopes keep verification necessary.
"""
import argparse
import json
from pathlib import Path
import re
import shlex

VERSION = 1
OUTPUT_FLAGS = {"-q", "-qq", "-v", "-vv", "-rA", "-ra", "-s", "-x", "--no-header", "--disable-warnings",
                "--quiet", "--verbose"}
OUTPUT_PREFIXES = ("--tb=", "--color=")
CACHE_PAIRS = (("-p", "no:cacheprovider"),)
VALUED = {"-k", "-m", "-p", "-c", "-o", "--deselect", "--rootdir", "--ignore", "--maxfail", "--junitxml"}
REDIRECTS = re.compile(r"(?<=\s)(?:2>&1|[12]?>\s*/dev/null)(?=\s|$)")
REDUNDANT = ("exact_repeat", "equivalent_repeat")
NECESSARY = ("after_failed_receipt", "after_relevant_change", "requested_rerun", "different_scope")
RAN = re.compile(r"Adage ran `([^`]+)`")


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


def extract(hooks_path, arm=None):
    """Compact timeline from a `production-hooks.jsonl` capture (the collected hook wrapper log)."""
    events, root, final = [], None, None
    for line in Path(hooks_path).read_text().splitlines():
        row = json.loads(line)
        payload = row.get("payload", {})
        root = root or payload.get("cwd")
        if row.get("event") == "PreToolUse":
            tool = payload.get("tool_name")
            data = payload.get("tool_input", {})
            events.append(dict(kind="tool", id=payload.get("tool_use_id"), name=tool,
                command=data.get("command") if tool == "Bash" else None,
                path=data.get("file_path") if tool in ("Edit", "Write", "MultiEdit") else None,
                digest=row.get("before", {}).get("digest"),
                adage_pre_context="additionalContext" in (row.get("output") or "")))
        for entry in row.get("journal_new") or []:
            text = json.loads(row["output"])["hookSpecificOutput"]["additionalContext"] if row.get("output") else ""
            match = RAN.search(text)
            outcome = entry.get("outcome", {})
            events.append(dict(kind="receipt", operation=entry.get("id"), route=entry.get("route"),
                command=match.group(1) if match else None, phase=entry.get("phase"),
                outcome=outcome.get("outcome"), changed=[c.get("path") for c in outcome.get("changed", [])],
                digest=row.get("after", {}).get("digest"), completion_delivered=bool(match)))
        if row.get("event") in ("Stop", "SessionEnd"):
            final = row.get("after", {}).get("digest")
    return dict(arm=arm, root=root, events=events, final_digest=final)


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
            receipt = dict(command=event["command"], call=call, digest=event["digest"],
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
                classification=classify(request, receipt, rerun_requested)))
    final = timeline.get("final_digest")
    succeeded = [r for r in receipts if r["succeeded"]]
    redundant = [r for r in rows if r["classification"] in REDUNDANT]
    opaque = [r for r in rows if r["classification"] == "opaque_verification"]
    fresh_final = any(r["digest"] == final for r in succeeded)
    at_final = [pytest_call(shlex.split(r["segment"]), root) for r in rows if "segment" in r and r["digest"] == final]
    if broader_required is not None:
        satisfied = any(c[2] in {tuple(t) for t in broader_required} for c in at_final)
    elif rerun_requested:
        satisfied = bool(at_final)
    else:
        satisfied = fresh_final or bool(at_final)
    necessary = bool(rerun_requested or broader_required is not None or (receipts and not fresh_final))
    return dict(version=VERSION, arm=timeline.get("arm"), requests=rows, receipts=len(receipts),
        successful_receipts=len(succeeded), redundant_repeats=len(redundant),
        covered_subsets=sum(r["classification"] == "covered_subset" for r in rows), opaque=len(opaque),
        confirmed_elimination=bool(fresh_final and not redundant and not opaque and not necessary),
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
