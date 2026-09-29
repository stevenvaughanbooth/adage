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
            # files a shell command wrote by redirection (`go doc -all . > godoc-current.txt`)
            first = cmd.strip().splitlines()[0] if cmd.strip() else ""
            for tgt in re.findall(r"(?<![<>&0-9])>{1,2}\s*([^\s;&|<>]+)", first):
                if tgt.startswith(("/dev/", "&")) or os.path.isabs(tgt):
                    continue
                edits.append({"k": k, "path": os.path.normpath(tgt), "tool": "Shell", "text": first,
                              "failed": c.is_error})
    return {"sid": s.sid, "n": len(s.calls), "edits": edits, "cmds": cmds, "cwd": cwd, "path": path}


def name_tokens(name: str) -> list[str]:
    return re.split(r"([_.\-])", name)


def anti_unify(paths: list[str]) -> str:
    """Files of the same role whose names vary by session (`flag_url.go`, `flag_port.go`): names in
    one directory with the same number of tokens (split on _ . -) are aligned; positions that agree
    are kept, the others become <x> (`flag_<x>.go`)."""
    d = str(Path(paths[0]).parent)
    toks = [name_tokens(Path(p).name) for p in paths]
    name = "".join(t if all(x[i] == t for x in toks) else "<x>" for i, t in enumerate(toks[0]))
    return name if d in (".", "") else f"{d}/{name}"


def pattern_glob(p: str) -> str:
    return p.replace("<x>", "*")


def matches_pattern(path: str, pattern: str) -> bool:
    import fnmatch
    return "<x>" in pattern and str(Path(path).parent) == str(Path(pattern).parent) and fnmatch.fnmatch(path, pattern_glob(pattern))


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
    files = [(p, len(s), statistics.median(first_pos[p])) for p, s in per_file.items() if len(s) >= need]
    # files whose exact name differs per session (edited in one session only), grouped by directory
    # and name shape, then anti-unified (`flag_<x>.go`, `flag_<x>_test.go`)
    if n > 1:
        groups = {}
        for p, sids in per_file.items():
            if len(sids) == 1:
                t = name_tokens(Path(p).name)  # same role: same directory, token count, first token, extension
                groups.setdefault((str(Path(p).parent), len(t), t[0], t[-1]), []).append(p)
        for _key, ps in groups.items():
            sids = {next(iter(per_file[p])) for p in ps}
            pat = anti_unify(ps)
            if len(sids) >= need and "<x>" in pat:
                files.append((pat, len(sids), statistics.median([first_pos[p][0] for p in ps])))
    files.sort(key=lambda x: x[2])
    # 2. exemplars: most recent passing session (views are in chronological order)
    order = [v for v in views if passed.get(v["sid"], True)][::-1] + [v for v in views if not passed.get(v["sid"], True)][::-1]
    exemplars = {}
    for p, _, _ in files:
        for v in order:
            hunks = [e["text"] for e in v["edits"] if not e["failed"] and (
                e["path"] == p or matches_pattern(e["path"], p))]
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


def render(card: dict, max_tokens: int | None = None) -> str:
    """The card as Markdown. With `max_tokens` (4 characters a token), exemplars are clipped to
    fewer lines (14, 10, 6, 3, then dropped) until the card fits."""
    if max_tokens:
        for k in (HUNK_LINES, 10, 6, 3, 0):
            clipped = {p: "\n".join(t.splitlines()[:k]) + ("\n…" if len(t.splitlines()) > k else "")
                       for p, t in card["exemplars"].items()} if k else {}
            text = _render({**card, "exemplars": clipped})
            if len(text) <= max_tokens * 4:
                return text
        return text[: max_tokens * 4]
    return _render(card)


def _render(card: dict) -> str:
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
