"""Gap-tolerant sequential pattern mining and SQS-style MDL selection.

- `mine`: PrefixSpan pattern growth with a maximum gap G between consecutive items (Pei et al.
  2004; gap constraints as in GSP/cSPADE), support = distinct sessions, then a closure filter (a
  pattern is dropped when a one-item-longer pattern containing it has the same support, the BIDE
  criterion checked against the mined set).
- `CodeTable`: an SQS-style code table (Tatti & Vreeken, KDD 2012) with a greedy Krimp-style
  cover: patterns in cover order (longer first, then higher support), each occurrence taken at the
  earliest uncovered positions with every gap ≤ G, the rest singletons. Description length
  `L(CT) + L(D|CT)`: one pattern stream (usage-based codes over patterns and singletons), per
  pattern a gap/fill stream (SQS's gap codes), `L(CT)` = universal integer codes for length,
  usage and gap count plus each item's standard code. Candidates are added best estimated gain
  first and kept only when the total length on the training sessions falls; a final pass removes
  any pattern whose removal lowers it (Krimp pruning).
- Held-out coding uses the training code lengths, fixed (Laplace-smoothed, with an escape for
  labels unseen in training), against the singleton-only table fitted on the same training data.
"""

from __future__ import annotations

import math
from collections import Counter, defaultdict

LOG2_C0 = math.log2(2.865064)


def l_n(n: int) -> float:
    """Rissanen's universal code length for an integer n >= 1."""
    n = max(1, int(n))
    s, x = LOG2_C0, math.log2(n)
    while x > 0:
        s += x
        x = math.log2(x) if x > 1 else 0
    return s


def mine(seqs: list[list[str]], smin: int, gap: int, maxlen: int = 5, cap: int = 200_000,
         skip: set[str] | None = None) -> dict[tuple, frozenset]:
    """Frequent gapped sequential patterns (length 2..maxlen) -> supporting session indices."""
    skip = skip or set()
    occ1: dict[str, dict[int, list[int]]] = defaultdict(lambda: defaultdict(list))
    for si, s in enumerate(seqs):
        for p, a in enumerate(s):
            if a not in skip:
                occ1[a][si].append(p)
    out: dict[tuple, frozenset] = {}

    def grow(pat: tuple, occ: dict[int, list[int]]):
        if len(out) >= cap or len(pat) >= maxlen:
            return
        ext: dict[str, dict[int, set]] = defaultdict(lambda: defaultdict(set))
        for si, ends in occ.items():
            s = seqs[si]
            for e in ends:
                for q in range(e + 1, min(len(s), e + gap + 1)):
                    if s[q] not in skip:
                        ext[s[q]][si].add(q)
        for x in sorted(ext):
            o = ext[x]
            if len(o) >= smin:
                np_ = pat + (x,)
                out[np_] = frozenset(o)
                grow(np_, {si: sorted(v) for si, v in o.items()})

    for a in sorted(occ1):
        if len(occ1[a]) >= smin:
            grow((a,), dict(occ1[a]))
    return out


def closed(pats: dict[tuple, frozenset]) -> dict[tuple, frozenset]:
    drop = set()
    for q, sq in pats.items():
        if len(q) < 3:
            continue
        for i in range(len(q)):
            p = q[:i] + q[i + 1:]
            if p in pats and len(pats[p]) == len(sq):
                drop.add(p)
    return {p: s for p, s in pats.items() if p not in drop}


def cover(seq: list[str], table: list[tuple], gap: int):
    n = len(seq)
    used = [False] * n
    occs = []
    for p in table:
        for i in range(n):
            if used[i] or seq[i] != p[0]:
                continue
            pos = [i]
            for item in p[1:]:
                prev, found = pos[-1], None
                for j in range(prev + 1, min(n, prev + gap + 1)):
                    if not used[j] and seq[j] == item:
                        found = j
                        break
                if found is None:
                    break
                pos.append(found)
            if len(pos) == len(p):
                for j in pos:
                    used[j] = True
                occs.append((p, pos))
    singles = [(seq[i], i) for i in range(n) if not used[i]]
    return occs, singles


def _stats(covers):
    use, gaps = Counter(), Counter()
    for occs, singles in covers:
        for p, pos in occs:
            use[p] += 1
            gaps[p] += (pos[-1] - pos[0] + 1) - len(p)
        for a, _ in singles:
            use[a] += 1
    return use, gaps


def _ulog(u: float, tot: float) -> float:
    return -u * math.log2(u / tot) if u > 0 else 0.0


class CodeTable:
    def __init__(self, train: list[list[str]], gap: int = 8):
        self.train, self.gap = train, gap
        self.freq = Counter(a for s in train for a in s)
        self.N = sum(self.freq.values())
        self.st = {a: -math.log2(c / self.N) for a, c in self.freq.items()}
        self.table: list[tuple] = []
        self.supp: dict[tuple, int] = {}
        self.covers = [cover(s, [], gap) for s in train]

    def order(self, pats):
        return sorted(pats, key=lambda p: (-len(p), -self.supp.get(p, 0), p))

    def length(self, table=None, covers=None) -> float:
        table = self.table if table is None else table
        covers = self.covers if covers is None else covers
        use, gaps = _stats(covers)
        U = sum(use.values())
        ld = sum(_ulog(u, U) for u in use.values())
        lct = 0.0
        for p in table:
            u = use.get(p, 0)
            f = u * (len(p) - 1)
            g = gaps.get(p, 0)
            ld += _ulog(g, g + f) + _ulog(f, g + f)
            lct += l_n(len(p)) + sum(self.st[a] for a in p) + l_n(u + 1) + l_n(g + 1)
        used_singles = {a for a in use if isinstance(a, str)}
        lct += l_n(len(used_singles) + 1) + sum(self.st[a] for a in used_singles)
        return ld + lct

    def fit(self, cands: dict[tuple, frozenset], max_cands: int = 3000, verbose: bool = False):
        est = sorted(cands, key=lambda p: (-(len(cands[p]) * (len(p) - 1)), -len(p), p))[:max_cands]
        best = self.length()
        self.base_bits = best
        for p in est:
            self.supp[p] = len(cands[p])
            t2 = self.order(self.table + [p])
            sess = cands[p]
            c2 = [cover(s, t2, self.gap) if i in sess else c for i, (s, c) in enumerate(zip(self.train, self.covers))]
            L = self.length(t2, c2)
            if L < best - 1e-9:
                self.table, self.covers, best = t2, c2, L
        # prune
        changed = True
        while changed:
            changed = False
            use, _ = _stats(self.covers)
            for p in sorted(self.table, key=lambda p: use.get(p, 0)):
                t2 = [q for q in self.table if q != p]
                c2 = [cover(s, t2, self.gap) for s in self.train]
                L = self.length(t2, c2)
                if L < best - 1e-9:
                    self.table, self.covers, best = t2, c2, L
                    changed = True
                    break
        self.bits = best
        use, gaps = _stats(self.covers)
        self.use, self.gaps = use, gaps
        return self

    # ---- held-out coding with fixed training code lengths ----
    def _model(self, table, alpha=0.5):
        covers = [cover(s, table, self.gap) for s in self.train]
        use, gaps = _stats(covers)
        keys = list(self.freq) + list(table)
        U = sum(use.values()) + alpha * (len(keys) + 1)
        code = {k: -math.log2((use.get(k, 0) + alpha) / U) for k in keys}
        esc = -math.log2(alpha / U)
        gp = {}
        for p in table:
            f = use.get(p, 0) * (len(p) - 1)
            g = gaps.get(p, 0)
            gp[p] = (-math.log2((g + alpha) / (g + f + 2 * alpha)), -math.log2((f + alpha) / (g + f + 2 * alpha)))
        return code, esc, gp

    def heldout_bits(self, seqs, table=None):
        """Per held-out session: (bits with the table, bits singleton-only, used patterns)."""
        table = self.table if table is None else table
        code, esc, gp = self._model(table)
        code0, esc0, _ = self._model([])
        unseen = math.log2(max(2, len(self.freq)))  # identical in both: naming an unseen label
        out = []
        for s in seqs:
            b0 = sum(code0.get(a, esc0 + unseen) for a in s)
            occs, singles = cover(s, table, self.gap)
            b = sum(code.get(a, esc + unseen) for a, _ in singles)
            for p, pos in occs:
                g = (pos[-1] - pos[0] + 1) - len(p)
                b += code[p] + g * gp[p][0] + (len(p) - 1) * gp[p][1]
            out.append((b, b0, [(p, pos) for p, pos in occs]))
        return out
