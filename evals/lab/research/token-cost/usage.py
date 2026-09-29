"""Learned data flow: which lines of each tool output did the agent later use?

A line of a tool output counts as *referenced* when
  (a) its stripped text (>= 8 chars) re-appears verbatim in something the agent wrote later
      (an Edit old_string, a command, a message), or
  (b) it holds an identifier/number/path token that the agent writes later, that neither the
      prompt nor anything the agent wrote before this output contained, and that is rare in this
      output (<= max(2, 1% of its lines)), so that a common word does not mark every line.
The agent's thinking is not in the transcripts (display omitted), so this is a LOWER bound on use.
It is a measurement instrument (a regex tokeniser), not part of any learned policy.

Adds to each result: n_lines, used (sorted line indices), used_strict (rule (a) only), tok (estimated tokens), carry ($/token).
Writes /tmp/adage-research/token-cost/annotated.pkl
"""
from __future__ import annotations

import collections
import pickle
import re

from common import PRICES, carry_multiplier, fam, fit_cpt, load

TOK = re.compile(r"[A-Za-z_][A-Za-z0-9_]{3,}|\d{3,}|[\w./-]*[/.][\w./-]+")


def toks(s):
    return set(TOK.findall(s))


def annotate(s, cpt):
    agent = s["agent"]
    ncalls = len(s["calls"])
    # agent content written at call index >= k, as one string and a token set, per k
    by_call = collections.defaultdict(list)
    for ci, t in agent:
        by_call[ci].append(t)
    later_str = [""] * (ncalls + 1)
    later_tok = [set() for _ in range(ncalls + 1)]
    acc_s, acc_t = [], set()
    for k in range(ncalls - 1, -1, -1):
        for t in by_call.get(k, []):
            acc_s.append(t)
            acc_t |= toks(t)
        later_str[k] = "\n".join(acc_s)
        later_tok[k] = set(acc_t)
    before_tok = [set() for _ in range(ncalls + 1)]
    acc = toks(s["prompt"])
    for k in range(ncalls + 1):
        before_tok[k] = set(acc)
        for t in by_call.get(k, []):
            acc |= toks(t)
    tpc = cpt[fam(s["model"])]["tok_per_char"]
    for r in s["results"]:
        k = r["seen_call"]
        lines = r["text"].split("\n")
        r["n_lines"] = len(lines)
        r["tok"] = r["chars"] * tpc
        r["carry"] = carry_multiplier(s, k)
        if k >= ncalls:
            r["used"] = []
            r["used_strict"] = []
            continue
        L, B, Ls = later_tok[k], before_tok[k], later_str[k]
        # Read output lines look like "123\tcode": strip the line-number prefix
        body = [re.sub(r"^\s*\d+\t", "", l) for l in lines]
        ltoks = [toks(b) for b in body]
        freq = collections.Counter(t for ts in ltoks for t in ts)
        cap = max(2, 0.01 * len(lines))
        used, strict = [], []
        for i, (b, ts) in enumerate(zip(body, ltoks)):
            sb = b.strip()
            if len(sb) >= 8 and sb in Ls:
                used.append(i)
                strict.append(i)
                continue
            if any(t in L and t not in B and freq[t] <= cap for t in ts):
                used.append(i)
        r["used"] = used
        r["used_strict"] = strict


def main():
    S = load()
    cpt = fit_cpt(S)
    for s in S:
        annotate(s, cpt)
        s["agent"] = None
        for r in s["results"]:
            r["text_head"] = r["text"][:300]
            r["line_chars"] = [len(l) + 1 for l in r["text"].split("\n")]
            del r["text"]
    with open("/tmp/adage-research/token-cost/annotated.pkl", "wb") as fh:
        pickle.dump(dict(sessions=S, cpt=cpt), fh)


if __name__ == "__main__":
    main()
