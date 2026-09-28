#!/usr/bin/env python3
"""The agents-md baseline: a context file written from the same training traces Adage learns from.

  python3 evals/repobench/agents_md.py <repo-id> <runner-root> [--model M]

As in Gloaguen et al. (2026), an LLM writes the repository's context file (AGENTS.md; committed
as CLAUDE.md, the file Claude Code reads) from the agents' own experience: one tool-less
`claude -p` call over the command lists of the training sessions in <runner-root>/state.json
(every Bash command in order, whether it failed, and whether the session's fix passed). It never
sees the tasks' prompts, diffs or tests. The result goes to context/<repo-id>.md; the sandbox
commits it for the agents-md arm only.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

import common
import llm

INSTRUCTIONS = """Below are the shell commands that coding agents ran in {n} earlier sessions in the repository \
{url}, in order, each marked FAILED when it exited non-zero, and whether the session's fix passed the \
repository's tests afterwards.

Write the repository's AGENTS.md: a short context file a future coding agent reads before it starts. Tell it \
the conventions it needs to work here efficiently: how to set up or use the environment, how to run the tests \
(the whole suite and a single file), commands that fail here and what to run instead, and any step that is \
owed after an edit. At most 15 bullet points, terse and imperative. Include only what the sessions below \
support, and nothing about particular bugs or tasks. Reply with the file's content only (Markdown), starting \
with the heading "# AGENTS.md".

Sessions:
{sessions}
"""


def session_lines(state: dict, max_cmds: int = 60) -> list[str]:
    out = []
    for rid, r in sorted(state["runs"].items()):
        if r.get("phase") != "train" or not r.get("metrics"):
            continue
        cmds = [("FAILED " if e.get("failed") else "") + " ".join((e.get("cmd") or "").split())[:200]
                for e in r.get("events", []) if e.get("tool") == "Bash"]
        passed = "passed" if r["metrics"].get("passed") else "did not pass"
        out.append(f"## session {len(out) + 1} ({passed})\n" + "\n".join(f"- {c}" for c in cmds[:max_cmds]))
    return out


def write(rid: str, root: Path, model: str) -> Path:
    r = common.repo(rid)
    state = json.loads((root / "state.json").read_text())
    # Paths of the run directories say nothing about the repository: show them relative.
    sessions = [s.replace(str(root), "<run>") for s in session_lines(state)]
    text, cost = llm.ask(INSTRUCTIONS.format(n=len(sessions), url=r["url"], sessions="\n\n".join(sessions)),
                         model=model, kind="agents-md")
    text = text.strip()
    if text.startswith("```"):
        text = text.strip("`").split("\n", 1)[1]
    p = common.HERE / "context" / f"{rid}.md"
    p.parent.mkdir(exist_ok=True)
    p.write_text(text.rstrip() + "\n")
    print(f"{rid}: {len(sessions)} sessions -> {p} ({len(text.splitlines())} lines, ${cost:.3f})")
    return p


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("repo")
    ap.add_argument("root")
    ap.add_argument("--model", default="claude-sonnet-5")
    a = ap.parse_args(argv)
    write(a.repo, Path(a.root), a.model)
    return 0


if __name__ == "__main__":
    sys.exit(main())
