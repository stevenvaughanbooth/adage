#!/usr/bin/env python3
"""The discoverable-conventions control: a generated sandbox whose conventions are DOCUMENTED.

  python3 evals/repobench/gen_doc.py config <bundle-dir> <out-config.json> [--repeats N] [--train-repeats N]
  python3 evals/repobench/gen_doc.py sandbox <bundle-dir> <dir> <task>      (the runner calls this)

The first corpus's gen sandboxes hide their conventions on purpose (an opaque tracker, a test
runner declared only in CI), which flatters a learner. Here the same sandbox gets a CLAUDE.md, as a
well-kept repository would have, that states every convention in `conventions.json`: how to read a
work item and which files it names, the command that runs the tests (and the ones that fail), and
the step owed after an edit. Decoys are not mentioned. The file is committed on top of the
generated commit, in every phase and every arm, so training agents read it too and Adage learns
from traces of agents that had it. The question: what does Adage add once a good context file
exists?
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
IDENT = ["-c", "user.name=maintainer", "-c", "user.email=maintainer@localhost", "-c", "commit.gpgsign=false"]


def doc_text(conv: dict) -> str:
    """CLAUDE.md for a gen sandbox, from its conventions.json."""
    lines = ["# Working in this repository", ""]
    avoided_by = {}
    for c in conv["conventions"]:
        if c["kind"] == "dead_end":
            avoided_by.setdefault(c["command"], []).extend(a for a in c.get("avoided", [])
                                                            if a not in avoided_by.get(c["command"], []))
    for c in conv["conventions"]:
        k, cmd = c["kind"], c.get("command", "")
        if k == "lookup":
            ex = (c.get("samples") or ["<ID>"])[0]
            lines.append(f"- Work items (IDs like `{ex}`) are not in the README or the code: read one with "
                         f"`{cmd.replace('{item}', ex)}` (add `--json` for JSON).")
        elif k == "linked_files":
            lines.append("- A work item's `Files:` line lists the files to change; start there.")
        elif k == "dead_end":
            avoided = " and ".join(f"`{a}`" for a in avoided_by.get(cmd, []))
            lines.append(f"- Run the test suite with `{cmd}`." + (f" {avoided} do not work in this repository."
                                                                   if avoided else ""))
        elif k in ("finish_line", "sequence"):
            glob = c.get("glob", "")
            steps = c.get("steps") or [cmd]
            lines.append(f"- After editing `{glob}`, run {' then '.join(f'`{s}`' for s in steps)}: the generated "
                         "code must be fresh, and the checks fail otherwise.")
        elif k == "scope":
            lines.append(f"- After changing a package, run its suite: `{cmd}`.")
        elif k == "env_sync":
            lines.append(f"- After editing `{c.get('glob', '')}`, run `{cmd}`.")
    # Dead-end lines repeat per convention; keep the first of identical ones.
    out, seen = [], set()
    for x in lines:
        if x not in seen or not x:
            out.append(x)
        seen.add(x)
    return "\n".join(out) + "\n"


def cmd_sandbox(bundle: Path, d: str, task: str) -> int:
    cfg = json.loads((bundle / "config.json").read_text())
    argv = [a.format(repo=str(REPO), dir=d, task=task, arm="") for a in cfg["sandbox"]]
    subprocess.run(argv, check=True, capture_output=True)
    conv = json.loads((bundle / "conventions.json").read_text())
    Path(d, "CLAUDE.md").write_text(doc_text(conv))
    subprocess.run(["git", "-C", d, "add", "CLAUDE.md"], check=True)
    subprocess.run(["git", "-C", d, *IDENT, "commit", "-q", "--no-verify", "-m", "Document the conventions"], check=True)
    return 0


def cmd_config(bundle: Path, out: Path, repeats: int, train_repeats: int) -> int:
    cfg = json.loads((bundle / "config.json").read_text())
    name = cfg["name"]
    cfg["name"] = f"{name}-documented"
    cfg["sandbox"] = ["python3", "{repo}/evals/repobench/gen_doc.py", "sandbox", str(bundle), "{dir}", "{task}"]
    cfg["train"]["repeats"] = train_repeats
    cfg["eval"]["repeats"] = repeats
    cfg["eval"]["arms"] = ["control", "adage"]
    cfg.update(host_auth=True, confine=True, seed=20260928, budget_usd=6.0)
    cfg["description"] = cfg.get("description", "") + " -- conventions documented in a committed CLAUDE.md"
    out.write_text(json.dumps(cfg, indent=1) + "\n")
    print(f"{cfg['name']}: train {len(cfg['train']['tasks'])}x{train_repeats}, eval {len(cfg['eval']['tasks'])}x{repeats}x2 -> {out}")
    return 0


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    c = sub.add_parser("config")
    c.add_argument("bundle")
    c.add_argument("out")
    c.add_argument("--repeats", type=int, default=2)
    c.add_argument("--train-repeats", type=int, default=3)
    s = sub.add_parser("sandbox")
    s.add_argument("bundle")
    s.add_argument("dir")
    s.add_argument("task")
    a = ap.parse_args(argv)
    if a.cmd == "sandbox":
        return cmd_sandbox(Path(a.bundle), a.dir, a.task)
    return cmd_config(Path(a.bundle), Path(a.out), a.repeats, a.train_repeats)


if __name__ == "__main__":
    sys.exit(main())
