"""Alphabets, pattern usefulness (prefix -> suffix), and a replay of compiled deliveries.

Usefulness of a pattern split (prefix, suffix) on held-out sessions: at the first point a session
completes the prefix (gaps <= G), does the suffix's first item follow within W calls, and how many
calls lie in between? `lift` divides that rate by the rate the suffix item follows a random
position within W in the same sessions.

Delivery replay (what compiling a pattern into a delivered step would be worth): at the first
prefix completion e in a session the suffix is delivered (`run` for one step, `sequence` for
two or three). It is **useful** if the agent later (anywhere after e) makes a successful call
with the suffix's first label (t*). Calls saved are counted over the union of useful windows
(e, t*), in three tiers:

- `fail`: calls the harness reported as errors (conservative);
- `fail+disc`: plus repository-discovery calls (the embedding lab's hand-written instrument,
  measurement only);
- `all`: every call in the window (the skip-ahead upper bound: assumes nothing in between was
  needed).

Net = p_follow (0.8) x saved - 0.1 per directive (carrying cost) - 0.3 per directive that is not
useful (misfire). The $ value is net x the session's $ per call.
"""

from __future__ import annotations

from collections import Counter, defaultdict

import nodes


class Exact:
    """Exact-match alphabets (baselines). `literal`: the call's first line as issued;
    `full` / `key`: the hand-written normaliser's levels (evals/lab/normalize.py)."""

    def __init__(self, train, level: str):
        self.level = level
        self.k = len({self._v(e) for s in train for e in s.events})

    def _v(self, e):
        if self.level == "literal":
            return e.text.split("\n")[0][:120]
        return getattr(e, self.level)

    def labels(self, s):
        return [self._v(e) + ("!" if e.fail else "") for e in s.events]

    def describe(self, label, n=1):
        return label[:70] + (" FAILED" if label.endswith("!") else "")


def make(kind: str, train, group="task", min_groups=2):
    if kind in ("literal", "full", "key"):
        return Exact(train, kind)
    tau = float(kind.split("@")[1])
    return nodes.Labeler(train, tau, group=group, min_groups=min_groups)


def first_completion(seq, pat, gap):
    """Earliest end position of a gapped occurrence of pat (greedy earliest per start)."""
    best = None
    for i, a in enumerate(seq):
        if a != pat[0]:
            continue
        pos = i
        ok = True
        for item in pat[1:]:
            nxt = None
            for j in range(pos + 1, min(len(seq), pos + gap + 1)):
                if seq[j] == item:
                    nxt = j
                    break
            if nxt is None:
                ok = False
                break
            pos = nxt
        if ok and (best is None or pos < best):
            best = pos
            break  # starts are scanned left to right; the first full match ends earliest enough
    return best


def splits(pat):
    k = len(pat)
    out = [(pat[:k - 1], pat[k - 1:])]
    if k >= 3:
        out.append((pat[:1], pat[1:]))
    return out


def usefulness(patterns, seqs, gap=8, window=8):
    """Per (prefix, suffix): sessions with the prefix, sessions where the suffix followed within
    `window`, calls in between, base rate."""
    # base rate of each label following a random position within the window
    rows = []
    for pat in patterns:
        for pre, suf in splits(pat):
            n_pre = n_fol = 0
            between = []
            for s in seqs:
                e = first_completion(s, pre, gap)
                if e is None:
                    continue
                n_pre += 1
                t = next((j for j in range(e + 1, min(len(s), e + window + 1)) if s[j] == suf[0]), None)
                if t is not None and (len(suf) == 1 or first_completion(s[t:], suf, gap) is not None):
                    n_fol += 1
                    between.append(t - e - 1)
            hits = tot = 0
            for s in seqs:
                for i in range(len(s)):
                    tot += 1
                    if suf[0] in s[i + 1:i + window + 1]:
                        hits += 1
            base = hits / tot if tot else 0.0
            rows.append({"pattern": pat, "prefix": pre, "suffix": suf, "n_prefix": n_pre, "n_follow": n_fol,
                         "between": between, "base": base})
    return rows


def compile_suffix(lab, train, suffix):
    """Track G §2's rule: a directed item compiles to its dominant literal when that variant holds
    >= 80% of the item's training occurrences; otherwise it is `advise` text (medoid) only."""
    raw = defaultdict(Counter)
    for s in train:
        for l, e in zip(lab.labels(s), s.events):
            raw[l][e.text.split("\n")[0][:120]] += 1
    out = []
    for item in suffix:
        c = raw.get(item, Counter())
        tot = sum(c.values())
        if not tot:
            out.append(("none", "", 0.0))
            continue
        v, n = c.most_common(1)[0]
        out.append(("run" if n / tot >= 0.8 else "advise", v, n / tot))
    return out


def replay(directives, sessions, seqs, gap=8, p_follow=0.8, carry=0.1, misfire=0.3, at_start=False):
    """directives: list of (prefix, suffix). Per session: issued, useful, saved per tier, net.
    `at_start`: the control that ignores the prefix and delivers every suffix after the first call
    (what the pattern's context adds is the difference)."""
    per = []
    for s, seq in zip(sessions, seqs):
        issued = useful = 0
        wins = []
        for pre, suf in directives:
            e = 0 if at_start else first_completion(seq, pre, gap)
            if e is None or e >= len(seq) - 1:
                continue
            issued += 1
            t = next((j for j in range(e + 1, len(seq)) if seq[j] == suf[0]), None)
            if t is None:
                continue
            useful += 1
            wins.append((e, t))
        cover = set()
        for e, t in wins:
            cover.update(range(e + 1, t))
        ev = s.events
        fail = sum(ev[i].fail for i in cover)
        disc = sum(ev[i].fail or ev[i].discovery for i in cover)
        allc = len(cover)
        dpc = (s.cost / max(1, s.calls)) if s.cost else None
        net = {k: p_follow * v - carry * issued - misfire * (issued - useful)
               for k, v in (("fail", fail), ("fail+disc", disc), ("all", allc))}
        per.append({"sid": s.sid, "task": s.task, "calls": len(ev), "issued": issued, "useful": useful,
                    "saved": {"fail": fail, "fail+disc": disc, "all": allc}, "net": net, "dpc": dpc})
    return per
