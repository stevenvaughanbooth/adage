"""What a hook KNOWS when a tool output arrives (no hindsight): anchor lines of each output.

An anchor line holds an identifier/path token that the prompt or the agent had already written
before (including the tool call's own input), and that is rare in this output (<= max(2, 1% of its
lines)); for a Read, also a line whose number an earlier output named as `<file>:<line>` (a grep
hit). A relevance filter keeps anchors +- w lines; this is available at PostToolUse (the output
is known) and, for Read, the grep-hit anchors are available at PreToolUse (offset/limit).

Writes /tmp/adage-research/token-cost/anchors.pkl: {sid: [(anchors, grep_anchors) per result]}.
"""
from __future__ import annotations

import collections
import os
import pickle
import re

from usage import toks

HIT = re.compile(r"([\w./-]+):(\d+)[:-]")


def main():
    raw = pickle.load(open("/tmp/adage-research/token-cost/sessions.pkl", "rb"))
    out = {}
    for s in raw:
        by_call = collections.defaultdict(list)
        for ci, t in s["agent"]:
            by_call[ci].append(t)
        known = toks(s["prompt"])
        known_upto = {}
        for k in range(len(s["calls"]) + 1):
            for t in by_call.get(k, []):
                known |= toks(t)
            known_upto[k] = set(known)  # content written at calls <= k
        hits = collections.defaultdict(set)  # basename -> line numbers named in outputs so far
        res = []
        for r in s["results"]:
            k = r["use_call"] if r["use_call"] is not None else 0
            Q = known_upto[k]
            lines = r["text"].split("\n")
            body = [re.sub(r"^\s*\d+\t", "", l) for l in lines]
            lt = [toks(b) for b in body]
            freq = collections.Counter(t for ts in lt for t in ts)
            cap = max(2, 0.01 * len(lines))
            anc = [i for i, ts in enumerate(lt) if any(t in Q and freq[t] <= cap for t in ts)]
            g = []
            if r["name"] == "Read":
                p = os.path.basename((r["input"] or {}).get("file_path", ""))
                try:
                    off = int((r["input"] or {}).get("offset") or 1)
                except (TypeError, ValueError):
                    off = 1
                g = sorted(i for i in (n - off for n in hits.get(p, ())) if 0 <= i < len(lines))
            res.append((anc, g))
            for m in HIT.finditer(r["text"]):
                hits[os.path.basename(m.group(1))].add(int(m.group(2)))
        out[s["sid"]] = res
    pickle.dump(out, open("/tmp/adage-research/token-cost/anchors.pkl", "wb"))


if __name__ == "__main__":
    main()
