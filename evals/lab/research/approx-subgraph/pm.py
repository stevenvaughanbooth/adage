"""Process mining over learned activities: Heuristics Miner and Inductive Miner (infrequent).

- **Heuristics Miner** (Weijters & van der Aalst 2003): dependency a => b =
  (|a>b| - |b>a|) / (|a>b| + |b>a| + 1) over the directly-follows counts; an edge is kept when
  the dependency is >= 0.9 and |a>b| >= s_min. Each kept edge is a two-step pattern (a, b) and is
  scored like the mined patterns (usefulness and delivery replay).
- **Inductive Miner - infrequent** (Leemans, Fahland & van der Aalst 2013/2014): recursive cuts on
  the directly-follows graph filtered at f = 0.2 of each activity's strongest outgoing edge:
  exclusive choice (connected components), sequence (strongly connected components, pairwise
  unreachable merged), parallel (components of the non-mutual graph, each with a start and an end
  activity), loop (start/end body and redo components); the fall-through is the flower model
  (any activity, any order). A textbook implementation; pm4py is not installed here.

Diagnostics: how much of the activity mass ends in flowers (a flower allows every order, i.e.
the model says nothing), and the sequence nodes outside flowers (candidate procedures).
"""

from __future__ import annotations

import math
import sys
from collections import Counter, defaultdict

sys.setrecursionlimit(10000)


def dfg(log):
    df, starts, ends = Counter(), Counter(), Counter()
    for t in log:
        if not t:
            continue
        starts[t[0]] += 1
        ends[t[-1]] += 1
        for a, b in zip(t, t[1:]):
            df[(a, b)] += 1
    return df, starts, ends


def heuristics(log, smin, dep_min=0.9):
    df, _, _ = dfg(log)
    sess = defaultdict(set)
    for i, t in enumerate(log):
        for a, b in zip(t, t[1:]):
            sess[(a, b)].add(i)
    edges = []
    for (a, b), n in df.items():
        if a == b:
            dep = n / (n + 1)
        else:
            m = df.get((b, a), 0)
            dep = (n - m) / (n + m + 1)
        if dep >= dep_min and len(sess[(a, b)]) >= smin:
            edges.append(((a, b), dep, n, len(sess[(a, b)])))
    return sorted(edges, key=lambda e: (-e[3], -e[1], e[0]))


def _components(nodes, adj):
    seen, comps = set(), []
    for n in sorted(nodes):
        if n in seen:
            continue
        stack, comp = [n], set()
        while stack:
            x = stack.pop()
            if x in seen:
                continue
            seen.add(x)
            comp.add(x)
            stack.extend(adj[x] - seen)
        comps.append(comp)
    return comps


def _reach(nodes, succ):
    R = {}
    for n in nodes:
        seen, st = set(), [n]
        while st:
            x = st.pop()
            for y in succ[x]:
                if y not in seen:
                    seen.add(y)
                    st.append(y)
        R[n] = seen
    return R


def _filter(df, f):
    best = defaultdict(int)
    for (a, b), n in df.items():
        best[a] = max(best[a], n)
    return {(a, b): n for (a, b), n in df.items() if n >= f * best[a]}


def im(log, f=0.2, depth=0):
    log = [t for t in log]
    if not log:
        return ("tau",)
    nonempty = [t for t in log if t]
    if len(nonempty) < len(log):
        if not nonempty:
            return ("tau",)
        if len(nonempty) < f * len(log):
            return ("tau",)
        return ("xor", ("tau",), im(nonempty, f, depth + 1))
    acts = {a for t in log for a in t}
    if len(acts) == 1:
        a = next(iter(acts))
        return (a,) if all(len(t) == 1 for t in log) else ("loop", (a,), ("tau",))
    if depth > 60:
        return ("flower", sorted(acts))
    df0, starts, ends = dfg(log)
    df = _filter(df0, f)
    succ, und = defaultdict(set), defaultdict(set)
    for (a, b) in df:
        if a != b:
            succ[a].add(b)
            und[a].add(b)
            und[b].add(a)
    # xor
    comps = _components(acts, und)
    if len(comps) > 1:
        subs = [[] for _ in comps]
        for t in log:
            i = max(range(len(comps)), key=lambda k: sum(a in comps[k] for a in t))
            subs[i].append([a for a in t if a in comps[i]])
        return ("xor",) + tuple(im(s, f, depth + 1) for s in subs if s)
    # sequence
    R = _reach(acts, succ)
    groups = [{a} for a in sorted(acts)]
    # merge mutually reachable (SCC) and pairwise unreachable
    changed = True
    while changed:
        changed = False
        for i in range(len(groups)):
            for j in range(i + 1, len(groups)):
                a, b = next(iter(groups[i])), next(iter(groups[j]))
                ab, ba = b in R[a], a in R[b]
                if ab == ba:
                    groups[i] |= groups[j]
                    del groups[j]
                    changed = True
                    break
            if changed:
                break
    if len(groups) > 1:
        groups.sort(key=lambda g: -sum(1 for x in groups if x is not g and next(iter(x)) in R[next(iter(g))]))
        return ("seq",) + tuple(im([[a for a in t if a in g] for t in log], f, depth + 1) for g in groups)
    # parallel
    nonmut = defaultdict(set)
    for a in acts:
        for b in acts:
            if a != b and not ((a, b) in df and (b, a) in df):
                nonmut[a].add(b)
                nonmut[b].add(a)
    comps = _components(acts, nonmut)
    if len(comps) > 1 and all(c & set(starts) and c & set(ends) for c in comps):
        return ("and",) + tuple(im([[a for a in t if a in c] for t in log], f, depth + 1) for c in comps)
    # loop
    body = set(starts) | set(ends)
    rest = acts - body
    if rest:
        adj = defaultdict(set)
        for (a, b) in df:
            if a in rest and b in rest:
                adj[a].add(b)
                adj[b].add(a)
        rc = _components(rest, adj)
        redo = [c for c in rc if any((e, x) in df for e in ends for x in c) and any((x, s) in df for s in starts for x in c)]
        if redo:
            bodyset = acts - set().union(*redo)
            bl, rl = [], [[] for _ in redo]
            for t in log:
                cur, mode = [], None
                for a in t:
                    m = -1 if a in bodyset else next(i for i, c in enumerate(redo) if a in c)
                    if mode is not None and m != mode:
                        (bl if mode == -1 else rl[mode]).append(cur)
                        cur = []
                    mode = m
                    cur.append(a)
                if cur:
                    (bl if mode == -1 else rl[mode]).append(cur)
            return ("loop", im(bl, f, depth + 1)) + tuple(im(r, f, depth + 1) for r in rl if r)
    return ("flower", sorted(acts))


def tree_stats(tree, freq):
    """(activity mass under flowers, leaves outside flowers, sequence nodes with >= 2 leaf children)."""
    flower_acts, leaves, seqs = set(), set(), []

    def walk(t):
        op = t[0]
        if op == "flower":
            flower_acts.update(t[1])
        elif op in ("seq", "xor", "and", "loop"):
            kids = t[1:]
            if op == "seq":
                ls = [k[0] for k in kids if len(k) == 1 and k[0] != "tau"]
                if len(ls) >= 2:
                    seqs.append(ls)
            for k in kids:
                walk(k)
        elif op != "tau":
            leaves.add(op)

    walk(tree)
    tot = sum(freq.values())
    fm = sum(freq[a] for a in flower_acts) / tot if tot else 0.0
    return {"flower_mass": fm, "flower_acts": len(flower_acts), "leaves": len(leaves), "seq_nodes": seqs}


def render(tree, desc, depth=0, maxd=4):
    op = tree[0]
    pad = "  " * depth
    if op == "flower":
        return f"{pad}flower({len(tree[1])} activities)\n"
    if op in ("seq", "xor", "and", "loop"):
        if depth >= maxd:
            return f"{pad}{op}(...)\n"
        return f"{pad}{op}\n" + "".join(render(k, desc, depth + 1, maxd) for k in tree[1:])
    if op == "tau":
        return f"{pad}tau\n"
    return f"{pad}{desc(op)}\n"
