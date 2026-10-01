"""Read-only: how often unguided Marshmallow sessions edit tests/*.py then CHANGELOG.rst within 8 tool events.

The base rate behind heldout_tasks.EXPECTATION, recorded before any held-out run. Unguided means organic
training or the no-hook agent arm on the neutral prompt template. Launch-gate cohorts (canaries and the
development arms, whose prompts add a work order and a test block) are excluded. Tool events approximate
the runtime's context window of eight events.
"""
import collections
import fnmatch
import json
import os
import sys

PRUNE = {"repo", ".venv", "private-score", "cache", "runtime", "bases", "adage-home", "history", "source", "bin", ".git"}
EDITS = {"Edit", "Write", "MultiEdit", "NotebookEdit"}


def tool_events(path, repo):
    out = []
    for line in open(path):
        o = json.loads(line)
        if o.get("type") != "assistant":
            continue
        for c in o["message"].get("content", []):
            if c.get("type") == "tool_use":
                fp = c["input"].get("file_path") or ""
                rel = os.path.relpath(fp, repo) if fp.startswith("/") else fp
                out.append((c["name"], rel, c["input"].get("command", "")))
    return out


def fired(events, within=8):
    last_tests = None
    for i, (name, rel, _) in enumerate(events):
        if name in EDITS and fnmatch.fnmatch(rel, "tests/*.py") and "/" not in rel[len("tests/"):]:
            last_tests = i
        if name in EDITS and rel == "CHANGELOG.rst" and last_tests is not None and i - last_tests <= within:
            return True
    return False


def order(events):
    seen = []
    for name, rel, _ in events:
        if name in EDITS:
            label = "tests" if rel.startswith("tests/") else ("changelog" if rel == "CHANGELOG.rst" else
                     ("validate" if rel.endswith("validate.py") else rel))
            if label not in seen:
                seen.append(label)
    return seen


rows = []
for auth in sys.argv[1:]:
    for d, dirs, files in os.walk(auth):
        dirs[:] = [x for x in dirs if x not in PRUNE and not x.startswith("launch-gate")]
        if "transcript-provenance.json" not in files or "native-transcript.jsonl" not in files:
            continue
        m = json.load(open(os.path.join(d, "manifest.json"))) if "manifest.json" in files else {}
        task = m.get("task") or ""
        if not task.startswith(("mm-curve--", "mm-validator--")):
            continue
        unguided = m.get("kind") == "organic-training" or (m.get("arm") == "agent")
        if not unguided:
            continue
        repo = m.get("execution_root") or m.get("checkout") or os.path.join(d, "repo")
        ev = tool_events(os.path.join(d, "native-transcript.jsonl"), repo)
        rows.append(dict(path=d, task=task, kind=m.get("kind"), arm=m.get("arm"), fired=fired(ev), order=order(ev),
                         tests_edits=sum(1 for n, r, _ in ev if n in EDITS and r.startswith("tests/")),
                         changelog_edits=sum(1 for n, r, _ in ev if n in EDITS and r == "CHANGELOG.rst")))
print(json.dumps(dict(sessions=len(rows), fired=sum(r["fired"] for r in rows),
                      first_edit_orders=collections.Counter(" > ".join(r["order"][:3]) for r in rows).most_common(),
                      rows=rows), indent=1))
