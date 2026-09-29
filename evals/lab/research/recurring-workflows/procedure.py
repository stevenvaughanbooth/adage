#!/usr/bin/env python3
"""Learn a procedure card from a family's earlier sessions (Claude Code transcripts).

  python3 procedure.py --out CARD.md [--outcomes outcomes.json] TRANSCRIPT ...

The card is what the family's earlier sessions had in common, found in the traces, not written by
hand:

  1. the files a change of this kind touches: every file edited in at least half of the sessions
     (all of them with one session), in the median order of their first edit; a file that is new in
     each session (a different name every time) is anti-unified to its directory ("a new file in
     tests/test_types/");
  2. for each such file, an exemplar of the edit from the most recent passing session (the hunk
     that was written, clipped), so the format (a changelog bullet, an export line, a docs block)
     is copied rather than rediscovered;
  3. commands: shell commands normalised to their first tokens (a leading `cd …&&` and trailing
     pipes dropped), kept when run in at least half of the sessions, with how often they failed; a
     command that failed in most sessions where it ran is listed as one to avoid, with the error's
     first line;
  4. the last command the passing sessions ran successfully (the finishing check).

Hand-written parts (instrument-level, listed in the README): which tools edit files (Edit, Write,
MultiEdit, NotebookEdit), the shell normalisation (strip `cd X &&`, `| tail`, `2>&1`), the clip
lengths. Nothing names a file, a command or an error of these repositories.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import re
import shlex
import statistics
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
from harness import traces  # noqa: E402

EDIT_TOOLS = ("Edit", "Write", "MultiEdit", "NotebookEdit")
HUNK_LINES = 14


def rel(path: str, cwd: str) -> str:
    try:
        return os.path.relpath(path, cwd) if os.path.isabs(path) else path
    except ValueError:
        return path


def norm_cmd(cmd: str) -> str:
    """A shell command's stable head: the last `&&`/`;` segment that is not a `cd`, pipes dropped,
    redirections dropped, at most 4 tokens, arguments that look like values of this variant kept
    only when they are paths or flags."""
    first = cmd.strip().splitlines()[0] if cmd.strip() else cmd
    first = first.split("<<")[0]
    segs = [s.strip() for s in re.split(r"&&|;", first) if s.strip()]
    segs = [s for s in segs if not s.startswith("cd ")] or segs
    s = " && ".join(x.split("|")[0].strip() for x in segs)
    s = re.sub(r"\d?>&?\d?\s*\S*", "", s)
    try:
        toks = shlex.split(s)
    except ValueError:
        toks = s.split()
    toks = [t for t in toks if t]
    out = []
    for t in toks:
        if len(out) >= 6:
            break
        if "::" in t:
            t = t.split("::")[0]
        out.append(t)
    return " ".join(out)


def added(old: str, new: str) -> str:
    """The lines an edit added (a Write adds all of them): what to imitate, without the context."""
    if not old:
        return new
    olds = set(old.splitlines())
    return "\n".join(ln for ln in new.splitlines() if not ln.strip() or ln not in olds).strip("\n")


def session_view(path: str) -> dict | None:
    s = traces.session_from_transcript(path)
    if s is None:
        return None
    cwd = s.cwd
    edits, cmds = [], []
    for k, c in enumerate(s.calls):
        inp = c.input or {}
        if c.tool in EDIT_TOOLS and inp.get("file_path"):
            p = rel(inp["file_path"], cwd)
            pairs = [(inp.get("old_string") or "", inp.get("new_string") or inp.get("content") or "")]
            if c.tool == "MultiEdit":
                pairs = [(e.get("old_string", ""), e.get("new_string", "")) for e in inp.get("edits") or []]
            text = "\n".join(added(o, n) for o, n in pairs)
            edits.append({"k": k, "path": p, "tool": c.tool, "text": text, "failed": c.is_error})
        elif c.tool == "Bash" and inp.get("command"):
            cmd = inp["command"].replace(cwd + "/", "").replace(cwd, ".")
            cmds.append({"k": k, "cmd": cmd, "norm": norm_cmd(cmd), "failed": c.is_error,
                         "out": (c.output or "").strip()})
    return {"sid": s.sid, "n": len(s.calls), "edits": edits, "cmds": cmds, "cwd": cwd, "path": path}


def anti_unify(paths: list[str]) -> str:
    """Different new files of the same role: their common directory and extension."""
    dirs = {str(Path(p).parent) for p in paths}
    exts = {Path(p).suffix for p in paths}
    d = dirs.pop() if len(dirs) == 1 else os.path.commonpath(paths) if paths else ""
    return f"{d}/<new>{exts.pop() if len(exts) == 1 else ''}"


def learn(views: list[dict], passed: dict | None = None) -> dict:
    n = len(views)
    need = 1 if n == 1 else math.ceil(n / 2)
    passed = passed or {}
    # 1. files
    per_file, first_pos, new_files = {}, {}, {}
    for v in views:
        seen = []
        for e in v["edits"]:
            if e["failed"] or e["path"] in seen:
                continue
            seen.append(e["path"])
        for i, p in enumerate(seen):
            per_file.setdefault(p, set()).add(v["sid"])
            first_pos.setdefault(p, []).append(i / max(1, len(seen)))
        for e in v["edits"]:
            if e["tool"] == "Write" and not e["failed"] and len(per_file.get(e["path"], ())) == 1:
                new_files.setdefault(str(Path(e["path"]).parent), {})[v["sid"]] = e["path"]
    files = [(p, len(s), statistics.median(first_pos[p])) for p, s in per_file.items() if len(s) >= need]
    # new files whose exact name differs per session, but whose directory recurs
    for d, by_sid in new_files.items():
        if len(by_sid) >= need and n > 1 and all(len(per_file[p]) == 1 for p in by_sid.values()):
            ps = list(by_sid.values())
            files.append((anti_unify(ps), len(by_sid), statistics.median(
                [first_pos[p][0] for p in ps])))
    files.sort(key=lambda x: x[2])
    # 2. exemplars: most recent passing session (views are in chronological order)
    order = [v for v in views if passed.get(v["sid"], True)][::-1] + [v for v in views if not passed.get(v["sid"], True)][::-1]
    exemplars = {}
    for p, _, _ in files:
        for v in order:
            hunks = [e["text"] for e in v["edits"] if not e["failed"] and (
                e["path"] == p or (p.endswith(("<new>.py", "<new>")) and str(Path(e["path"]).parent) == p.split("/<new>")[0]
                                   and e["tool"] == "Write"))]
            if hunks:
                lines = max(hunks, key=lambda h: len(h.splitlines())).strip("\n").splitlines()
                exemplars[p] = "\n".join(lines[:HUNK_LINES]) + ("\n…" if len(lines) > HUNK_LINES else "")
                break
    # 3. commands
    per_cmd = {}
    for v in views:
        for c in v["cmds"]:
            d = per_cmd.setdefault(c["norm"], {"sids": set(), "runs": 0, "fails": 0, "err": ""})
            d["sids"].add(v["sid"])
            d["runs"] += 1
            if c["failed"]:
                d["fails"] += 1
                d["err"] = d["err"] or next((ln for ln in c["out"].splitlines() if ln.strip()
                                             and not ln.startswith("Exit code")), "")[:160]
    cmds = [(k, len(d["sids"]), d["runs"], d["fails"], d["err"]) for k, d in per_cmd.items() if len(d["sids"]) >= need]
    use = sorted([c for c in cmds if c[3] * 2 < c[2]], key=lambda c: -c[1])
    avoid = sorted([c for c in cmds if c[3] * 2 >= c[2]], key=lambda c: -c[1])
    # 4. the finishing check: the last successful command of passing sessions, most common
    finals, loops = {}, set()
    for v in views:
        last_edit = max((e["k"] for e in v["edits"] if not e["failed"]), default=-1)
        heads = [" ".join(c["norm"].split()[:2]) for c in v["cmds"] if c["k"] < last_edit]
        loops |= {h for h in heads if heads.count(h) >= 2}
    for v in views:
        if not passed.get(v["sid"], True):
            continue
        last_edit = max((e["k"] for e in v["edits"] if not e["failed"]), default=-1)
        after = [c["norm"] for c in v["cmds"] if c["k"] > last_edit and not c["failed"]
                 and " ".join(c["norm"].split()[:2]) in loops]
        for x in after:
            finals.setdefault(" ".join(x.split()[:2]), {}).setdefault(v["sid"], x)
    final = None
    if finals:
        head, by_sid = max(finals.items(), key=lambda x: len(x[1]))
        toks = [x.split() for x in by_sid.values()]
        m = min(len(t) for t in toks)
        final = " ".join(t0 if all(t[i] == t0 for t in toks) else "<…>" for i, t0 in enumerate(toks[0][:m]))
        if final.count("<…>") * 2 >= len(final.split()):  # nothing stable left to say
            final = None
    return {"n": n, "files": files, "exemplars": exemplars, "use": use[:6], "avoid": avoid[:4], "final": final,
            "passed": sum(1 for v in views if passed.get(v["sid"], True))}


def render(card: dict) -> str:
    n = card["n"]
    out = [f"# How changes of this kind were done here before\n",
           f"Learned from {n} earlier session(s) of the same kind of task in this repository "
           f"({card['passed']} passed the project's checks). Use it as a checklist; the specifics of "
           f"your task differ.\n", "## Files such a change touched (in order)\n"]
    for p, k, _ in card["files"]:
        out.append(f"- `{p}` ({k}/{n} sessions)")
    out.append("")
    if card["exemplars"]:
        out.append("## What the edit looked like last time (another variant; copy the format, not the content)\n")
        for p, _, _ in card["files"]:
            if p in card["exemplars"]:
                out.append(f"`{p}`:\n```\n{card['exemplars'][p]}\n```")
        out.append("")
    if card["use"] or card["avoid"]:
        out.append("## Commands\n")
        for c, k, runs, fails, _ in card["use"]:
            out.append(f"- `{c}` (ran in {k}/{n} sessions, {runs - fails}/{runs} succeeded)")
        for c, k, runs, fails, err in card["avoid"]:
            out.append(f"- avoid `{c}`: failed {fails}/{runs} times" + (f" (`{err}`)" if err else ""))
        out.append("")
    if card["final"]:
        out.append(f"## Finishing check\n\nThe passing sessions ended by running `{card['final']}` successfully.\n")
    return "\n".join(out)


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("transcripts", nargs="+")
    ap.add_argument("--out", required=True)
    ap.add_argument("--outcomes")
    a = ap.parse_args(argv)
    views = [v for v in (session_view(p) for p in a.transcripts) if v]
    passed = json.loads(Path(a.outcomes).read_text()) if a.outcomes else {}
    card = learn(views, passed)
    text = render(card)
    Path(a.out).write_text(text)
    print(text)
    print(f"-- {len(text)} chars, ~{len(text) // 4} tokens", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
