"""Hindsight relabelling of one agent trace: which calls achieved something, which were waste, and
which *shortcut* an oracle could have told the agent, how early, and what it would have removed.

Nothing here matches tool names, error strings or command words written by hand. Everything is
derived from the trace's own data flow and from statistics learned on the training pool:

- **Tokens.** Identifier/path-like tokens (``[A-Za-z_][\\w./-]*``, length >= 3) of each call's input
  and output. A token's weight is its IDF over training sessions, so ubiquitous words (``self``,
  ``the``) carry nothing; tokens of the task prompt are known to the agent and never count as flow.
- **Locations** are tokens with a ``/`` or that name a tracked file or directory of the repository's
  own checkout (``git ls-files``).
- **Data flow** k -> j: tokens of output(k) that reappear in input(j) (credited to the most recent
  output containing the token and to the output that first introduced it). An edge needs total IDF
  weight >= ``W_FLOW``.
- **Writer tools** are learned: tools whose median input size / output size over the training pool is
  >= 5 (they put more into the world than they take out; here: Edit and Write). A *write* is a writer
  call that names a file of the final diff (``state.json`` ``changed_files``) or a path named again later.
- **Retry chains**: calls of the same tool with similar inputs (IDF-weighted Jaccard >= ``T_IN``), not
  across a write unless the earlier attempt was flagged. In a chain x1..xn, x_i (i < n) is a *failed
  attempt* if its output fed nothing outside the chain and it was flagged (``is_error``) or its output
  is unlike the final attempt's (output Jaccard < ``T_OUT``): the agent tried again and got a different
  result. The call just before each retry is its *repair*.
- **Achieving calls**: writes, the final attempt of a chain that had failed attempts ("the command
  that finally worked"), calls later repeated with the same result ("a command later repeated
  successfully"), repairs, and the backward data-flow closure of all of these.
- **Waste**: every other call.

Shortcuts (what an oracle could have said), the earliest point t0 it could have said it and the set D
of calls it would have removed (kind-specific, deliberately tight):

- ``command``: the command that finally worked. D = its failed attempts plus the wasted calls between
  the first attempt and it; t0 = the first attempt. ``procedure``: the repairs of the same chain
  ("first run X"), same D.
- ``file``: an achieving call whose input got a new location by data flow from achieving calls that
  fed nothing else (the search that found the file). D = those "finders" plus the wasted calls from
  the first finder to the use; t0 = the first finder. A call whose only informative content is a
  location is also a ``file`` shortcut.
- ``fact``: the same with non-location tokens (an identifier, a config key, a test class name).
- ``action``: any other achieving call; D = the wasted calls since the previous achieving call whose
  input resembles it (its near-misses). Instances with an empty D are kept so that a delivery the
  agent needed no help with scores as redundant rather than wrong.
"""

from __future__ import annotations

import math
import re
import subprocess
from pathlib import Path
from collections import Counter, defaultdict
from dataclasses import dataclass, field

TOK = re.compile(r"[A-Za-z_][A-Za-z0-9_./\-]*[A-Za-z0-9_]")
W_FLOW = 3.0  # min IDF mass for a data-flow edge
T_IN = 0.5  # input similarity for "same attempt"
T_OUT = 0.3  # output similarity below which an earlier attempt "got a different result"
CHAIN_GAP = 12  # max calls between two attempts of one chain
FACT_IDF = 2.0  # a fact token must be rarer than in ~1 of 7 sessions
ATTR_AHEAD = 8  # a wasted call is attributed among the next achieving calls
T_ATTR = 0.3  # ... to the most similar one if this similar, else to the next one


def toks(text: str) -> set:
    return {t for t in TOK.findall(text) if len(t) >= 3}


CHECKOUTS = {r: [Path(b) / r / "private" / d / "repo" for b in ("/tmp/adage-rb2/runs", "/tmp/adage-repobench/runs")
                 for d in ("bind-sandbox", "instincts-sandbox")]
             for r in ("attrs", "click", "marshmallow", "urfave-cli", "vue-core")}
_NAMES: dict = {}
_DIRS: dict = {}


def repo_names(repo: str) -> set:
    """Every tracked file of the repository's checkout, its directories and basenames (`git ls-files`):
    what counts as a location in this repository, read from the repository itself."""
    if repo not in _NAMES:
        names = set()
        for c in CHECKOUTS.get(repo, []):
            if c.is_dir():
                out = subprocess.run(["git", "-C", str(c), "ls-files"], capture_output=True, text=True).stdout
                for f in out.split():
                    parts = f.split("/")
                    names.add(f)
                    names.add(parts[-1])
                    for i in range(1, len(parts)):
                        names.add("/".join(parts[:i]))
                        _DIRS.setdefault(repo, set()).add("/".join(parts[:i]))
                break
        _NAMES[repo] = names
    return _NAMES[repo]


def is_dir(t: str, repo: str) -> bool:
    repo_names(repo)
    return t.rstrip("/") in _DIRS.get(repo, set())


def is_pathlike(t: str, repo: str | None = None) -> bool:
    """A token that names a location: it has a directory separator, or it is a file/directory name
    of the repository's checkout."""
    return "/" in t or (repo is not None and t in repo_names(repo))


class Idf:
    def __init__(self, sessions):
        df = Counter()
        for s in sessions:
            seen = set()
            for c in s.calls:
                seen |= toks(c.inp_text)
                seen |= toks(c.out)
            df.update(seen)
        self.n = max(len(sessions), 1)
        self.df = df
        self.max = math.log(self.n + 1)

    def __call__(self, t: str) -> float:
        return math.log((self.n + 1) / (self.df.get(t, 0) + 1))

    def mass(self, ts) -> float:
        return sum(self(t) for t in ts)

    def wjacc(self, a: set, b: set) -> float:
        if not a and not b:
            return 1.0
        u = self.mass(a | b)
        return self.mass(a & b) / u if u > 0 else 0.0


def learn_writers(sessions, min_calls: int = 20, ratio: float = 5.0) -> set:
    """Tools that put more into the world than they take out: the median of input size / output
    size over their calls is >= `ratio` (learned from the training pool, not listed by name)."""
    import statistics
    r = defaultdict(list)
    for s in sessions:
        for c in s.calls:
            r[c.tool].append(len(c.inp_text) / max(c.out_chars, 1))
    return {t for t, v in r.items() if len(v) >= min_calls and statistics.median(v) >= ratio}


@dataclass
class Shortcut:
    kind: str  # command | procedure | file | fact
    content: list  # indices of the calls whose content is the shortcut (repairs + final, or the use)
    anchor: int  # the achieving call the shortcut leads to (t*)
    t0: int  # earliest call index at which an oracle could have told it
    D: list  # calls it would remove
    tokens: frozenset = frozenset()  # for file/fact: the flowed tokens; for command: the final input tokens


@dataclass
class Label:
    achieving: set
    waste: set
    failed_attempt: set
    repairs: set
    writes: set
    flow_out: dict  # k -> {j: weight}
    shortcuts: list = field(default_factory=list)
    intok: list = field(default_factory=list)
    tail_waste: list = field(default_factory=list)
    owner: dict = field(default_factory=dict)


def label(s, idf: Idf, writers: set) -> Label:
    calls = s.calls
    n = len(calls)
    ptoks = toks(s.prompt)
    intok = [toks(c.inp_text) - ptoks for c in calls]
    outtok = [toks(c.out) - ptoks for c in calls]

    # --- data flow: output(k) -> input(j)
    first_out, last_out = {}, {}
    flow_out = defaultdict(dict)
    flow_toks = defaultdict(set)  # (k, j) -> tokens
    for j in range(n):
        srcs = defaultdict(set)
        for t in intok[j]:
            if t in last_out:
                srcs[last_out[t]].add(t)
                srcs[first_out[t]].add(t)
        for k, ts in srcs.items():
            w = idf.mass(ts)
            if w >= W_FLOW:
                flow_out[k][j] = w
                flow_toks[(k, j)] = ts
        for t in outtok[j]:
            first_out.setdefault(t, j)
            last_out[t] = j

    # --- writes whose effect persists: a (learned) writer tool's call aimed at a path that is in the
    # final diff or that a later call names again (the effect is reused).
    pathtok = [{t for t in intok[j] if is_pathlike(t, s.repo)} for j in range(n)]
    later_paths = [set() for _ in range(n + 1)]
    for j in range(n - 1, -1, -1):
        later_paths[j] = later_paths[j + 1] | pathtok[j]
    changed = set(s.changed_files)
    writes = {j for j in range(n) if calls[j].tool in writers and not calls[j].is_error
              and pathtok[j] & (changed | later_paths[j + 1])}

    # --- retry chains
    nxt = {}
    for k in range(n):
        for j in range(k + 1, min(n, k + 1 + CHAIN_GAP)):
            if j - 1 in writes and j - 1 > k and not calls[k].is_error:
                break  # the state changed in between: a new attempt, not a retry (unless k plainly failed)
            if calls[j].tool == calls[k].tool and idf.wjacc(intok[k], intok[j]) >= T_IN and intok[k]:
                nxt[k] = j
                break
    has_prev = set(nxt.values())
    chains = []
    for k in range(n):
        if k in has_prev or k not in nxt:
            continue
        ch = [k]
        while ch[-1] in nxt:
            ch.append(nxt[ch[-1]])
        chains.append(ch)
    failed_attempt, repairs, finals_worked, repeated = set(), set(), {}, set()
    for ch in chains:
        last = ch[-1]
        if calls[last].is_error:
            continue
        lo = outtok[last]
        fails = []
        chs = set(ch)
        for x in ch[:-1]:
            if x in writes or set(flow_out.get(x, {})) - chs:
                continue  # its output was used, or it wrote something that persisted
            if calls[x].is_error or idf.wjacc(outtok[x], lo) < T_OUT:
                fails.append(x)
            else:
                repeated.add(x)
                repeated.add(last)
        if fails:
            failed_attempt.update(fails)
            finals_worked[last] = ch
            for a, b in zip(ch, ch[1:]):
                if a in fails and b - a > 1:
                    repairs.add(b - 1)

    # --- achieving closure
    seeds = set(writes) | set(finals_worked) | repeated | repairs
    seeds -= failed_attempt
    ach = set()
    stack = list(seeds)
    rev = defaultdict(set)
    for k, js in flow_out.items():
        for j in js:
            rev[j].add(k)
    while stack:
        j = stack.pop()
        if j in ach:
            continue
        ach.add(j)
        for k in rev[j]:
            if k not in ach and k not in failed_attempt:
                stack.append(k)
    waste = set(range(n)) - ach
    lab = Label(ach, waste, failed_attempt, repairs, writes, dict(flow_out), intok=intok)

    # --- shortcuts. What each kind of shortcut could remove is specific to it (tight, not the
    # whole gap before it):
    #   command/procedure: the failed attempts and the wasted calls between the first attempt and
    #     the one that worked;
    #   file/fact: the achieving calls that found it and fed nothing else ("finders"), and the wasted
    #     calls from the first finder to its first use;
    #   action (an achieving call with nothing newly found in it): the wasted calls since the previous
    #     achieving call that look like it (similar input: the agent's own near-misses).
    order = sorted(ach - failed_attempt)
    owner = {}  # (reporting only) wasted call -> the achieving call it was on its way to
    for w in sorted(waste):
        ahead = [a for a in order if a > w][:ATTR_AHEAD]
        if not ahead:
            continue
        best = max(ahead, key=lambda a: (idf.wjacc(intok[w], intok[a]), -a))
        owner[w] = best if idf.wjacc(intok[w], intok[best]) >= T_ATTR else ahead[0]
    prev = -1
    done_chain = set()
    seen_in = set()
    seen_paths = set()
    for a in order:
        prev_a, prev = prev, a
        if a in finals_worked or a in repairs:
            last = a if a in finals_worked else \
                next((l for l, ch in finals_worked.items() if ch[0] < a < l), None)
            if last is not None and last not in done_chain:
                done_chain.add(last)
                ch = finals_worked[last]
                fails = [x for x in ch[:-1] if x in failed_attempt]
                reps = sorted(r for r in repairs if ch[0] < r < last)
                D = set(fails) | {g for g in range(ch[0], last) if g in waste}
                lab.shortcuts.append(Shortcut("command", [last], last, min(D), sorted(D), frozenset(intok[last])))
                if reps:  # the repair steps are a shortcut of their own ("first run X"), same D
                    rk = frozenset().union(*(intok[r] for r in reps))
                    lab.shortcuts.append(Shortcut("procedure", reps + [last], last, min(D), sorted(D), rk))
            seen_in |= intok[a]
            seen_paths |= pathtok[a]
            continue
        if a in writes:  # the fix itself: not a shortcut
            seen_in |= intok[a]
            seen_paths |= pathtok[a]
            continue
        files = {t for t in intok[a] if is_pathlike(t, s.repo) and not is_dir(t, s.repo)}
        new_paths = files - seen_paths
        flowed = {t for k in rev[a] for t in flow_toks.get((k, a), ())} - seen_in
        key = new_paths if new_paths else {t for t in flowed if idf(t) >= FACT_IDF}
        finders = set()
        if key:
            for k in rev[a]:
                if k in ach and k not in writes and flow_toks.get((k, a), set()) & key:
                    outs = set(flow_out.get(k, {}))
                    if all(j == a or j in waste or (intok[j] & key) for j in outs):
                        finders.add(k)
        if key and finders:
            kind = "file" if new_paths else "fact"
            D = finders | {w for w in range(min(finders), a) if w in waste}
        else:
            informative = {t for t in intok[a] - files if idf(t) >= 0.5 and not is_pathlike(t, s.repo)}
            kind = "action" if informative or not files else "file"  # a call that only names a location
            key = intok[a] if kind == "action" else files
            D = {w for w in range(prev_a + 1, a) if w in waste and idf.wjacc(intok[w], intok[a]) >= T_ATTR}
        # also with an empty D: the agent took it without waste (a delivery naming it is redundant,
        # not wrong); such instances never enter a library and never count as savings
        lab.shortcuts.append(Shortcut(kind, [a], a, min(D) if D else a, sorted(D), frozenset(key)))
        seen_in |= intok[a]
        seen_paths |= pathtok[a]
    lab.tail_waste = [w for w in sorted(waste) if w not in owner]
    lab.owner = owner
    return lab


def describe(s, lab: Label, width: int = 110) -> str:
    """Measurement/debug view of a labelled session."""
    lines = []
    for i, c in enumerate(s.calls):
        tag = ("A" if i in lab.achieving else "-") + ("F" if i in lab.failed_attempt else " ") + \
              ("R" if i in lab.repairs else " ") + ("W" if i in lab.writes else " ") + ("E" if c.is_error else " ")
        lines.append(f"{i:3d} {tag} {c.tool:5s} {c.inp_text[:width]!r}")
    for sc in lab.shortcuts:
        lines.append(f"  {sc.kind:9s} t0={sc.t0} t*={sc.anchor} D={sc.D} content={sc.content} key={sorted(sc.tokens)[:6]}")
    return "\n".join(lines)
