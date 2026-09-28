#!/usr/bin/env python3
"""Write each task's prompt: an issue-style paraphrase of the real fix, never the fix itself.

  python3 evals/repobench/prompts.py <repo-id> [--model M] [--batch N] [--redo]

The writer (one tool-less `claude -p` call per batch of tasks) sees, per task, the fix commit's
message and the diff of its TEST files (to describe the symptom and a reproduction, as an issue
would). It never sees the source diff. It is told to describe the observable problem and the
expected behaviour as a user reporting it would, and never the fix, the files to change, tests,
commits or issue numbers. `leaks()` then refuses a prompt that names a test file, a test function
the fix added, an issue or PR number, a commit SHA, or GitHub; refused prompts are rewritten once
and otherwise left empty (the task is then dropped from the benchmark).
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

import common
import llm

INSTRUCTIONS = """You write the task prompts of a coding benchmark built from real bug fixes in the open-source \
repository {name} ({url}). For each item below you get the fix commit's message and the diff of the tests that \
were added or changed with the fix. You never see the fix itself.

For each item, write the message a developer would type to a coding agent working in a checkout of the \
repository, asking it to fix the problem. Write it like a good bug report:
- describe the observable problem (what the user does, what happens) and the expected behaviour;
- include a minimal reproduction (a few lines of code using the library's public API, or a command) when \
the test diff shows one;
- 2 to 6 sentences, plus the optional short snippet; end by asking the agent to fix it.

Never:
- say how to fix it, or which source file, class or internal function to change (public API names that the \
user calls are fine);
- mention tests, test files or test names, and never ask for a test to be added;
- mention commits, pull requests, issue numbers, GitHub, upstream, or that a fix exists anywhere.

Reply with one JSON object and nothing else: {{"<item id>": "<prompt text>", ...}} for every item.

Items:
{items}
"""


def item_text(r: dict, t: dict, max_diff: int = 3500) -> str:
    m = common.mirror(r)
    msg = common.git(m, "show", "-s", "--format=%B", t["sha"]).strip()
    tests = [p for p in t["test_files"] if not p.endswith(".snap")]
    diff = common.git(m, "diff", "--unified=3", t["base"], t["sha"], "--", *tests) if tests else ""
    if len(diff) > max_diff:
        diff = diff[:max_diff] + "\n... (diff truncated)"
    return f"### id: {t['id']}\nCommit message:\n{msg[:1500]}\n\nTest diff:\n{diff}\n"


def added_test_names(r: dict, t: dict) -> list[str]:
    m = common.mirror(r)
    tests = [p for p in t["test_files"] if not p.endswith(".snap")]
    diff = common.git(m, "diff", t["base"], t["sha"], "--", *tests) if tests else ""
    names = re.findall(r"^\+\s*(?:async\s+)?def (test_\w+)", diff, re.M)
    names += re.findall(r"^\+\s*func (Test\w+)", diff, re.M)
    names += re.findall(r"^\+\s*(?:test|it)\(\s*['\"`]([^'\"`]{12,})['\"`]", diff, re.M)
    return names


def leaks(r: dict, t: dict, text: str) -> list[str]:
    """Why a prompt gives the task away, or [] if it does not."""
    out = []
    low = text.lower()
    if not text.strip():
        return ["empty"]
    if re.search(r"(?<![\w.])#\d{2,}", text) or re.search(r"\b(pull request|github|upstream|commit [0-9a-f]{6,})\b", low):
        out.append("names an issue, PR, commit or GitHub")
    if t["sha"][:7] in low:
        out.append("names the fix commit")
    for p in t["test_files"]:
        if p.split("/")[-1].lower() in low:
            out.append(f"names the test file {p}")
    for n in added_test_names(r, t):
        if n.lower() in low:
            out.append(f"names the added test {n!r}")
    if re.search(r"\b(add|write) (a |an )?(regression )?tests?\b", low):
        out.append("asks for a test")
    return out


def parse_json(text: str) -> dict:
    s = text.strip()
    s = re.sub(r"^```(?:json)?\s*|\s*```$", "", s)
    i, j = s.find("{"), s.rfind("}")
    return json.loads(s[i:j + 1])


def write(rid: str, model: str, batch: int, redo: bool) -> dict:
    r = common.repo(rid)
    data = common.load_tasks(rid)
    todo = [t for t in data["tasks"] if redo or not t.get("prompt")]
    cost = 0.0
    for attempt in range(2):
        for i in range(0, len(todo), batch):
            chunk = todo[i:i + batch]
            items = "\n".join(item_text(r, t) for t in chunk)
            reply, c = llm.ask(INSTRUCTIONS.format(name=rid, url=r["url"], items=items), model=model, kind="prompts")
            cost += c
            try:
                got = parse_json(reply)
            except ValueError:
                print(f"  batch {i // batch}: unparseable reply", flush=True)
                continue
            for t in chunk:
                text = (got.get(t["id"]) or "").strip()
                why = leaks(r, t, text)
                t["prompt"] = "" if why else text
                t["prompt_source"] = f"{model}: commit message + test diff (never the source diff)"
                if why:
                    t["prompt_refused"] = why
                else:
                    t.pop("prompt_refused", None)
        todo = [t for t in todo if not t.get("prompt")]
        if not todo:
            break
    p = common.TASKS_DIR / f"{rid}.json"
    p.write_text(json.dumps(data, indent=1) + "\n")
    n = sum(1 for t in data["tasks"] if t.get("prompt"))
    print(f"{rid}: {n}/{len(data['tasks'])} tasks have a prompt; ${cost:.3f}")
    return data


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("repo")
    ap.add_argument("--model", default="claude-sonnet-5")
    ap.add_argument("--batch", type=int, default=8)
    ap.add_argument("--redo", action="store_true")
    a = ap.parse_args(argv)
    write(a.repo, a.model, a.batch, a.redo)
    return 0


if __name__ == "__main__":
    sys.exit(main())
