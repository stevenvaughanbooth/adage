"""What mattered in a session, and what finding it cost.

Per session (all measurement instruments, written by hand, not part of any learner):

- `access[i]`: the base-snapshot files call i opened (Read/Edit/Write/MultiEdit path; a shell
  command naming a snapshot path, e.g. `cat`/`sed -n`/`grep … file`).
- `E`: files edited that exist at the base commit (new files cannot be prefetched; counted apart).
- `N` (necessary reads): files this session accessed before its last edit that at least half of
  the *other* sessions of the same task also accessed before their last edit (independent agents
  converging on a file = it mattered). Needs >= 2 other sessions (held-out tasks have 6-12).
- `T = E ∪ N`: the targets.
- `G`: the task's gold source files (from the commit; agent-independent).

Exploration calls (instrument): Grep, Glob, LS, and shell commands `normalize.py` marks exploratory
(ls/find/grep/cat/sed/head…), plus Reads of files that are not targets. Each exploration call is
*attributed* to the next target first-access after it (the hunt it was part of). A path hint that
contains target f makes the calls attributed to f unnecessary; a read batch that contains f also
removes f's first Read. Calls after the last target first-access are never attributed (verification,
other work).
"""

from __future__ import annotations

import collections
import re

import data
import repo as R

EDIT = {"Edit", "Write", "MultiEdit", "NotebookEdit"}
SEARCH = {"Grep", "Glob", "LS"}
TOK = re.compile(r"[\w./@-]+")
CHARS_PER_TOKEN = 3.5


def base_of(s) -> str | None:
    t = R.tasks(s.repo).get(s.task)
    return t["base"] if t else None


def accesses(s, tree: dict) -> list:
    out = []
    for c in s.calls:
        fs = set()
        if c.tool in ("Read", "Edit", "Write", "MultiEdit", "NotebookEdit", "NotebookRead"):
            fs = {p for p in c.paths if p in tree or c.tool in EDIT}
        elif c.tool == "Bash":
            cmd = data.rel(c.inp.get("command", ""))
            fs = {t.removeprefix("./") for t in TOK.findall(cmd) if t.removeprefix("./") in tree}
        out.append(fs)
    return out


def last_edit(s) -> int:
    idx = [i for i, c in enumerate(s.calls) if c.tool in EDIT]
    return idx[-1] if idx else len(s.calls)


def annotate(sessions: list) -> dict:
    """sid -> dict(E, N, T, G, new, access, explore, first)."""
    by_task = collections.defaultdict(list)
    info = {}
    for s in sessions:
        base = base_of(s)
        if not base:
            continue
        tree = R.snapshot(s.repo, base)
        acc = accesses(s, tree)
        le = last_edit(s)
        E = {p for c, a in zip(s.calls, acc) if c.tool in EDIT for p in a if p in tree}
        new = {p for c, a in zip(s.calls, acc) if c.tool in EDIT for p in a if p not in tree}
        pre = set().union(*acc[:le + 1]) & set(tree) if acc else set()
        t = R.tasks(s.repo)[s.task]
        G = {p for p in t.get("src_files", []) if p in tree}
        info[s.sid] = dict(E=E, new=new, pre=pre, G=G, access=acc, tree=tree, base=base)
        by_task[s.task].append(s.sid)
    for task, sids in by_task.items():
        for sid in sids:
            others = [o for o in sids if o != sid]
            cnt = collections.Counter(p for o in others for p in info[o]["pre"])
            N = {p for p in info[sid]["pre"] if len(others) >= 2 and cnt[p] >= len(others) / 2}
            info[sid]["N"] = N - info[sid]["E"]
            info[sid]["T"] = info[sid]["E"] | N
    for s in sessions:
        if s.sid not in info:
            continue
        d = info[s.sid]
        T = d["T"]
        first = {}
        for i, a in enumerate(d["access"]):
            for p in a & T:
                first.setdefault(p, i)
        explore = []
        for i, c in enumerate(s.calls):
            a = d["access"][i]
            is_read = c.tool in ("Read", "NotebookRead") or (c.tool == "Bash" and c.explore and a)
            if c.tool in SEARCH or (c.tool == "Bash" and c.explore and not (a & T)) or (is_read and not (a & T)):
                explore.append(i)
        d["first"] = first
        d["explore"] = explore
        # attribution: exploration call i -> the next target first-accessed after i
        order = sorted((i, p) for p, i in first.items())
        attr = {}
        for i in explore:
            nxt = [p for j, p in order if j > i]
            if nxt:
                attr[i] = nxt[0]
        d["attr"] = attr
    return info


def savings(s, removed: set, inject_tokens: float = 0.0) -> tuple[int, float]:
    """(calls removed, dollars saved) under the common harness's cumulative-context cost model
    (`harness/costmodel.py`, validated against the billed totals): a removed call's share of the
    context growth is no longer written and read by every later response, and a response all of
    whose calls are removed is not made. `inject_tokens` placed at the prompt is carried by every
    response and is subtracted."""
    cm = data.hcost(s.sid)
    usd, _ = cm.removal(sorted(removed))
    inj, _ = cm.injection(int(round(inject_tokens)), -1)
    return len(removed), usd - inj


def removable(s, d: dict, H: set, batch: bool = False, tier: str = "all") -> set:
    """Calls a hint H (path hint, or read batch when batch=True) makes unnecessary.

    tier "search": only attributed calls that opened no file (Grep/Glob/find/ls/grep -r: pure
    location work a path replaces); tier "all": also the attributed reads of non-target files (dead
    ends on the way to a target, which a hint may or may not prevent)."""
    out = {i for i, f in d["attr"].items() if f in H and (tier == "all" or not d["access"][i])}
    if batch:
        # Claude Code refuses an Edit of a file not Read in this session, so an injected file the
        # agent edits still needs its Read: only non-edited targets lose their first Read.
        for f in (H & d["T"]) - d["E"]:
            i = d["first"].get(f)
            if i is not None and s.calls[i].tool in ("Read", "NotebookRead", "Bash"):
                out.add(i)
    return out
